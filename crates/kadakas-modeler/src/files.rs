//! Open and save of `.kad` files: the same files, the same container, the same rules as
//! `kadakas-app`. The work is `kadakas_api::files::file_call` (the call behind `file.open` and
//! `file.save`); this module adds the questions to the person that the app also asks.
//!
//! - A big file: the person is asked first, or the file is refused when it cannot fit in memory.
//! - A save over a file that another program changed since it was opened: the person decides.
//! - A save when the parts of the file that the pictures stay in cannot be found: the person
//!   decides. Nothing is dropped without a yes.
//!
//! The questions go through [`Ask`], so the tests answer them without a window.

use std::path::{Path, PathBuf};

use kadakas_api::Session;
use kadakas_api::files::{
    FileDone, SaveSpec, WriteFn, consent_by_token, file_call, write_document,
};
use kadakas_file::Form;
use kadakas_l10n::tr;
use kadakas_ui_core::open_limit::OpenDecision;
use serde_json::{Value, json};

/// A question to the person. The answer is yes or no.
pub enum Question {
    /// A big file: the text says how much memory it needs.
    OpenBig(String),
    /// Replace a file that another program changed since it was opened. The file name.
    Overwrite(String),
    /// Save without the parts that cannot be found. The file and what is missing.
    SaveWithout { file: String, what: String },
}

/// Answers the questions: a dialog in the program, a fixed answer in a test.
pub trait Ask {
    fn ask(&mut self, q: &Question) -> bool;
}

/// The answers of a person at the window: message dialogs. The default answer is no.
pub struct Dialogs;

impl Ask for Dialogs {
    fn ask(&mut self, q: &Question) -> bool {
        let (title, text) = match q {
            Question::OpenBig(text) => (tr!("modeler-open-big-title"), text.clone()),
            Question::Overwrite(file) => (
                tr!("modeler-save-overwrite-title"),
                tr!("modeler-save-overwrite-text", file = file.as_str()),
            ),
            Question::SaveWithout { file, what } => (
                tr!("modeler-save-missing-title"),
                tr!(
                    "modeler-save-missing-text",
                    file = file.as_str(),
                    what = what.as_str()
                ),
            ),
        };
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title(title)
            .set_description(text)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            == rfd::MessageDialogResult::Yes
    }
}

/// How to open the file at `path`, from what it loads into memory (DOC-19), as the app does.
pub fn open_decision(path: &Path) -> OpenDecision {
    use kadakas_file::native;
    let Some(meta) = std::fs::metadata(path).ok().filter(|m| m.is_file()) else {
        return OpenDecision::Open;
    };
    let memory = kadakas_sys::sysinfo::physical_memory();
    match native::inspect(path) {
        Ok(i) if i.form == Form::Container => {
            kadakas_ui_core::open_limit::decide_container(i.file_bytes, i.resident_bytes(), memory)
        }
        Ok(i) => kadakas_ui_core::open_limit::decide(i.file_bytes, memory),
        Err(_) => kadakas_ui_core::open_limit::decide(meta.len(), memory),
    }
}

/// The write step of the session, without the bookkeeping of the app's autosave (the modeler has
/// no autosave yet).
fn writer() -> Box<WriteFn<'static>> {
    Box::new(
        |path: &Path, doc: &kadakas_core::model::Document, spec: &SaveSpec| {
            write_document(path, doc, spec)
        },
    )
}

/// Opens the file into the session. `Ok` holds the notes the person should read (what the load
/// repaired). `Err` is the text for the status line; the document is then unchanged.
pub fn open(session: &mut Session, path: &Path, ask: &mut dyn Ask) -> Result<Vec<String>, String> {
    match open_decision(path) {
        OpenDecision::Open => {}
        OpenDecision::Ask(e) => {
            if !ask.ask(&Question::OpenBig(e.ask_text())) {
                return Err(tr!("modeler-open-cancelled"));
            }
        }
        OpenDecision::Refuse(e) => return Err(e.refuse_text()),
    }
    let mut write = writer();
    let mut consent = consent_by_token;
    match file_call(
        session,
        "file.open",
        &json!({"path": path.to_string_lossy()}),
        &mut *write,
        &mut consent,
    ) {
        Ok(FileDone::Opened(_, notes, _)) => Ok(notes),
        Ok(FileDone::Saved(..)) => Err(tr!("modeler-open-failed", path = path.display())),
        Err(e) => Err(tr!(
            "modeler-open-error",
            path = path.display(),
            error = e.message.as_str()
        )),
    }
}

/// What a save did.
#[derive(Debug, PartialEq)]
pub struct Saved {
    pub path: PathBuf,
    /// What the person should know (a source that could not be opened again, parts left out).
    pub notes: Vec<String>,
}

/// Saves the document to `path`. The form is told by the ending: `.json` is the plain form,
/// anything else the container. When the file changed on disk, or the parts of the file cannot be
/// found, the person is asked; a no stops the save and nothing is written.
pub fn save(session: &mut Session, path: &Path, ask: &mut dyn Ask) -> Result<Saved, String> {
    let path = with_extension(path);
    let mut params = json!({"path": path.to_string_lossy()});
    // At most one question for each kind of refusal.
    for _ in 0..3 {
        let mut write = writer();
        let mut consent = consent_by_token;
        match file_call(session, "file.save", &params, &mut *write, &mut consent) {
            Ok(FileDone::Saved(p, _, notes)) => return Ok(Saved { path: p, notes }),
            Ok(FileDone::Opened(..)) => {
                return Err(tr!("modeler-save-failed", path = path.display()));
            }
            Err(e) => {
                let token = e
                    .details
                    .as_ref()
                    .and_then(|d| d.get("consent_token"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let (key, question) = match (e.reason, &token) {
                    (Some(r), Some(_)) if r == kadakas_api::REASON_FILE_CHANGED => (
                        "allow_overwrite",
                        Question::Overwrite(path.display().to_string()),
                    ),
                    (Some(r), Some(_)) if r == kadakas_api::REASON_PARTS_MISSING => {
                        let m = session.missing_for_save();
                        (
                            "allow_missing",
                            Question::SaveWithout {
                                file: m.file.clone(),
                                what: m.message(),
                            },
                        )
                    }
                    _ => {
                        return Err(tr!(
                            "modeler-save-error",
                            path = path.display(),
                            error = e.message.as_str()
                        ));
                    }
                };
                if !ask.ask(&question) {
                    return Err(tr!("modeler-save-cancelled"));
                }
                params[key] = Value::String(token.expect("matched above"));
            }
        }
    }
    Err(tr!("modeler-save-failed", path = path.display()))
}

/// Adds `.kad` when the name has no ending the program knows.
pub fn with_extension(path: &Path) -> PathBuf {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
    {
        Some(e) if e == "kad" || e == "json" => path.to_path_buf(),
        _ => {
            let mut p = path.as_os_str().to_os_string();
            p.push(".kad");
            PathBuf::from(p)
        }
    }
}

/// The picker for Open: a `.kad` (or `.json`) file.
pub fn pick_open() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Kadakas", &["kad"])
        .add_filter("JSON", &["json"])
        .pick_file()
}

/// The picker for Save As.
pub fn pick_save(current: Option<&Path>) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new()
        .add_filter("Kadakas", &["kad"])
        .add_filter("JSON", &["json"])
        .set_file_name(
            current
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("model.kad"),
        );
    if let Some(dir) = current.and_then(Path::parent).filter(|p| p.is_dir()) {
        d = d.set_directory(dir);
    }
    d.save_file().map(|p| with_extension(&p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;
    use kadakas_core::command::Command;
    use kadakas_core::model::{Attributes, Shape};

    /// Answers every question the same way, and counts them.
    struct Fixed(bool, usize);
    impl Ask for Fixed {
        fn ask(&mut self, _q: &Question) -> bool {
            self.1 += 1;
            self.0
        }
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kadakas-modeler-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn add_square(s: &mut Session) {
        s.editor
            .execute(&Command::AddObject {
                shape: Shape::Hardscape {
                    boundary: kadakas_core::curve::corners(&[
                        DVec2::ZERO,
                        DVec2::new(2000.0, 0.0),
                        DVec2::new(2000.0, 1000.0),
                        DVec2::new(0.0, 1000.0),
                    ]),
                    material: "m".into(),
                    thickness: Some(100.0),
                    elevation: None,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
    }

    #[test]
    fn the_ending_decides_the_form_and_kad_is_added() {
        assert_eq!(with_extension(Path::new("a/b")), PathBuf::from("a/b.kad"));
        assert_eq!(
            with_extension(Path::new("a/b.kad")),
            PathBuf::from("a/b.kad")
        );
        assert_eq!(
            with_extension(Path::new("a/B.JSON")),
            PathBuf::from("a/B.JSON")
        );
        assert_eq!(
            with_extension(Path::new("a/b.txt")),
            PathBuf::from("a/b.txt.kad")
        );
    }

    #[test]
    fn a_saved_model_opens_again_in_this_and_in_a_second_session() {
        let d = temp("roundtrip");
        let path = d.join("yard.kad");
        let mut a = Session::with_sequential_ids();
        add_square(&mut a);
        let mut no = Fixed(false, 0);
        let saved = save(&mut a, &path, &mut no).unwrap();
        assert_eq!(saved.path, path);
        assert!(path.is_file());
        assert_eq!(no.1, 0, "no question for a plain save");
        // The container is a ZIP file, the same as the app writes.
        assert_eq!(&std::fs::read(&path).unwrap()[..2], b"PK");

        let mut b = Session::with_sequential_ids();
        open(&mut b, &path, &mut no).unwrap();
        assert_eq!(b.editor.doc().objects.len(), 1);
        assert_eq!(
            a.editor.doc().objects.values().next().unwrap().shape,
            b.editor.doc().objects.values().next().unwrap().shape
        );
        // A save of the second session over the same file is allowed (it is the file it opened).
        save(&mut b, &path, &mut no).unwrap();
        assert_eq!(no.1, 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_that_another_program_changed_is_not_replaced_without_a_yes() {
        let d = temp("changed");
        let path = d.join("yard.kad");
        let mut a = Session::with_sequential_ids();
        add_square(&mut a);
        save(&mut a, &path, &mut Fixed(false, 0)).unwrap();
        let mut b = Session::with_sequential_ids();
        open(&mut b, &path, &mut Fixed(false, 0)).unwrap();
        // Another program writes the file.
        std::thread::sleep(std::time::Duration::from_millis(30));
        let mut other = Session::with_sequential_ids();
        add_square(&mut other);
        add_square(&mut other);
        save(&mut other, &path, &mut Fixed(true, 0)).unwrap();
        let before = std::fs::read(&path).unwrap();

        let mut no = Fixed(false, 0);
        let e = save(&mut b, &path, &mut no).unwrap_err();
        assert_eq!(no.1, 1, "the person was asked once");
        assert_eq!(e, tr!("modeler-save-cancelled"));
        assert_eq!(std::fs::read(&path).unwrap(), before, "nothing was written");

        let mut yes = Fixed(true, 0);
        save(&mut b, &path, &mut yes).unwrap();
        assert_eq!(yes.1, 1);
        assert_ne!(std::fs::read(&path).unwrap(), before, "now it is written");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_or_damaged_file_gives_a_message_and_keeps_the_model() {
        let d = temp("bad");
        let mut s = Session::with_sequential_ids();
        add_square(&mut s);
        let mut no = Fixed(false, 0);
        let e = open(&mut s, &d.join("nope.kad"), &mut no).unwrap_err();
        assert!(e.contains("nope.kad"), "{e}");
        let junk = d.join("junk.kad");
        std::fs::write(&junk, b"this is not a model").unwrap();
        assert!(open(&mut s, &junk, &mut no).is_err());
        assert_eq!(s.editor.doc().objects.len(), 1, "the model is still there");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_plain_json_form_is_written_for_a_json_name() {
        let d = temp("json");
        let path = d.join("yard.json");
        let mut s = Session::with_sequential_ids();
        add_square(&mut s);
        save(&mut s, &path, &mut Fixed(false, 0)).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.trim_start().starts_with('{'));
        let mut t = Session::with_sequential_ids();
        open(&mut t, &path, &mut Fixed(false, 0)).unwrap();
        assert_eq!(t.editor.doc().objects.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_small_file_opens_without_a_question() {
        let d = temp("small");
        let path = d.join("a.kad");
        let mut s = Session::with_sequential_ids();
        save(&mut s, &path, &mut Fixed(false, 0)).unwrap();
        assert_eq!(open_decision(&path), OpenDecision::Open);
        assert_eq!(open_decision(&d.join("missing.kad")), OpenDecision::Open);
        let _ = std::fs::remove_dir_all(&d);
    }
}
