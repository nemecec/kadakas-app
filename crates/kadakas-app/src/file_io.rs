//! Document files: the container (`.kad`, a ZIP file with the drawing, the pictures and the
//! versions) and the plain JSON form (`.json`). Both load the same model (DOC-20 to DOC-22); the
//! work is in `kadakas-file`, and this module adds the texts for the person.

use kadakas_l10n::tr;
use std::path::{Path, PathBuf};

use kadakas_api::Session;
use kadakas_core::model::Document;
#[cfg(not(target_arch = "wasm32"))]
use kadakas_file::native;
use kadakas_file::{FileError, Opened, ReadMode};
use kadakas_sys::fsio;
#[cfg(not(target_arch = "wasm32"))]
use kadakas_ui_core::open_limit::OpenDecision;
use serde_json::Value;

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // the browser build uses it
pub fn parse_document(text: &str) -> Result<Document, String> {
    parse_document_bytes(text.as_bytes())
}

/// The largest document that the browser build parses, in bytes: 2 GiB. The memory of the browser
/// is unknown, so the person is asked above 500 MB (`kadakas_ui_core::open_limit::decide` with
/// unknown memory) and is never refused below this guard. A wasm page cannot hold more than 4 GiB
/// in any case, and a file of 2 GiB needs about 3.4 GB to open.
pub const WEB_GUARD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Parses the bytes of a document file (the browser has them from the file picker). The bytes are
/// parsed in place (`Document::from_slice`): no copy as text and no `serde_json::Value`. The
/// browser asks the person about a big file first (see `WEB_GUARD_BYTES`); there is no file to
/// check before the read.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // the browser build uses it
pub fn parse_document_bytes(bytes: &[u8]) -> Result<Document, String> {
    parse_document_bytes_capped(bytes, WEB_GUARD_BYTES)
}

/// [`parse_document_bytes`] with a size limit.
fn parse_document_bytes_capped(bytes: &[u8], max: u64) -> Result<Document, String> {
    parse_opened_bytes(bytes, max, ReadMode::Eager).map(|o| o.doc)
}

/// Reads the bytes of a document file of either form, told by the first bytes. A container is
/// limited by the sum of the sizes of its entries as well (`max`).
pub fn parse_opened_bytes(bytes: &[u8], max: u64, mode: ReadMode) -> Result<Opened, String> {
    if bytes.len() as u64 > max {
        return Err(format!(
            "the file is {} bytes; the limit is {max} bytes",
            bytes.len()
        ));
    }
    kadakas_file::read_bytes(bytes, kadakas_file::Limits::default().with_total(max), mode)
        .map_err(|e| e.to_string())
}

/// The browser's picked file: a container or the plain JSON form. The container stays in memory
/// and the document reads its pictures from it when they are used.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // the browser build uses it
pub fn open_picked_bytes(bytes: Vec<u8>) -> Result<Opened, String> {
    parse_opened_bytes(&bytes, WEB_GUARD_BYTES, ReadMode::Lazy)
}

/// The bytes of the document in the container form, for the download of the browser build.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // the browser build uses it
pub fn container_bytes(session: &mut Session) -> Result<Vec<u8>, String> {
    let keep: Vec<_> = session.file_source.iter().cloned().collect();
    let anchor = session.needs_anchor();
    let doc = session.document_for_save_in(true).map_err(|e| e.message)?;
    let opts = kadakas_file::WriteOptions {
        generator: kadakas_file::generator(),
        keep,
        anchor,
        skip_missing: false,
        allow_overwrite: false,
    };
    kadakas_file::write_container(Vec::new(), &doc, &opts)
        .map(|(bytes, _)| bytes)
        .map_err(|e| e.to_string())
}

/// Reads a document file from a buffered reader. Only a regular file is read. There is no fixed
/// size limit for the file of a person: a big file is asked about before it gets here (see
/// [`open_decision`]), so a caller that skips the question must not call this.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_document(path: &Path) -> Result<Document, String> {
    native::open_path(path, u64::MAX, ReadMode::Eager)
        .map(|o| o.doc)
        .map_err(|e| read_error(path, e))
}

/// Opens the document of the person: a container keeps its pictures in the file and the result
/// holds the open file (`Opened::source`), so a save copies unchanged parts from it. Same rules
/// as [`read_document`].
#[cfg(not(target_arch = "wasm32"))]
pub fn open_file(path: &Path) -> Result<Opened, String> {
    native::open_path(path, u64::MAX, ReadMode::Lazy).map_err(|e| read_error(path, e))
}

/// The text for a file that cannot be read.
#[cfg(not(target_arch = "wasm32"))]
fn read_error(path: &Path, e: FileError) -> String {
    match e {
        FileError::NotFound(_) | FileError::NotRegular(_) | FileError::Io(_) => {
            tr!("file-io-cannot-read", path = path.display(), e = e)
        }
        other => format!("{}: {other}", path.display()),
    }
}

/// How to open the file at `path`, from its size on disk: open it, ask the person first, or refuse
/// it (DOC-19). Every way into a `.kad` file of the person goes through this: File > Open, the
/// library browser and the recovery of an autosave (the browser build asks with
/// `open_limit::decide(len, None)`). When the size cannot be read the answer is `Open`, and the
/// open reports its own error.
#[cfg(not(target_arch = "wasm32"))]
pub fn open_decision(path: &Path) -> OpenDecision {
    open_decision_for(&[path])
}

/// Like [`open_decision`] for several candidates (the versions of an autosave): the largest file
/// decides, because any of them may be the one that opens.
#[cfg(not(target_arch = "wasm32"))]
pub fn open_decision_for<P: AsRef<Path>>(paths: &[P]) -> OpenDecision {
    // What a file loads into memory decides, not its size on disk: a container keeps its pictures
    // in the file. The largest of the candidates decides.
    let largest = paths
        .iter()
        .filter_map(|p| {
            std::fs::metadata(p.as_ref())
                .ok()
                .filter(|m| m.is_file())
                .map(|m| (p, m))
        })
        // A file that is not a container (or is damaged) loads as much as it is long.
        .map(|(p, m)| match native::inspect(p.as_ref()) {
            Ok(i) => (
                kadakas_core::open_estimate::needed_bytes_for(
                    i.resident_bytes(),
                    i.form == kadakas_file::Form::Container,
                ),
                i.file_bytes,
                i.resident_bytes(),
                i.form == kadakas_file::Form::Container,
            ),
            Err(_) => (
                kadakas_core::open_estimate::needed_bytes(m.len()),
                m.len(),
                m.len(),
                false,
            ),
        })
        .max();
    let memory = kadakas_sys::sysinfo::physical_memory();
    match largest {
        Some((_, file, resident, true)) => {
            kadakas_ui_core::open_limit::decide_container(file, resident, memory)
        }
        Some((_, file, _, false)) => kadakas_ui_core::open_limit::decide(file, memory),
        None => OpenDecision::Open,
    }
}

/// The size of the file in bytes, if it is a file.
#[cfg(not(target_arch = "wasm32"))]
pub fn file_len(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len())
}

/// Saves atomically (temporary file, then rename) and keeps the old file as `.bak`.
/// It counts as a manual save: a late autosave of the same file is dropped (see `autosave_fs`).
#[cfg(all(not(target_arch = "wasm32"), test))]
pub fn write_document(path: &Path, doc: &Document) -> Result<(), String> {
    write_document_as(path, doc, &SaveSpec::from_extension(path, Vec::new())).map(|_| ())
}

/// How a save writes (see `kadakas_api::files::SaveSpec`).
#[cfg(not(target_arch = "wasm32"))]
pub use kadakas_api::files::SaveSpec;

/// Why a save did not happen.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub enum SaveFailure {
    /// The text for the person.
    Text(String),
    /// Parts of the document cannot be found in the file it was opened from (the file changed on
    /// disk). Nothing was written. The person decides (see [`ask_save_without`]).
    Missing(kadakas_file::Missing),
    /// Another program changed the file at the path since it was opened, and the save would
    /// replace it (the path). Nothing was written. The person decides (see [`ask_overwrite`]).
    Changed(String),
}

#[cfg(not(target_arch = "wasm32"))]
impl SaveFailure {
    /// The error of a call of the session that stops a save: a message, or the missing parts.
    pub fn from_rpc(e: kadakas_api::RpcError) -> SaveFailure {
        if e.reason == Some(kadakas_api::REASON_PARTS_MISSING) {
            SaveFailure::Missing(kadakas_file::Missing::default())
        } else if e.reason == Some(kadakas_api::REASON_FILE_CHANGED) {
            SaveFailure::Changed(String::new())
        } else {
            SaveFailure::Text(e.message)
        }
    }

    /// The text for the status line.
    pub fn text(self) -> String {
        match self {
            SaveFailure::Text(t) => t,
            SaveFailure::Missing(m) => m.message(),
            SaveFailure::Changed(file) => kadakas_file::FileError::FileChanged(file).to_string(),
        }
    }
}

/// Writes a document; the notes are what the person should know (a source that could not be
/// opened again, parts that were left out).
#[cfg(not(target_arch = "wasm32"))]
pub fn try_write_document(
    path: &Path,
    doc: &Document,
    spec: &SaveSpec,
) -> Result<Vec<String>, SaveFailure> {
    match crate::autosave::manual_save(path, || kadakas_api::files::write_document(path, doc, spec))
    {
        Ok(notes) => Ok(notes),
        Err(FileError::Missing(m)) => Err(SaveFailure::Missing(m)),
        Err(FileError::FileChanged(f)) => Err(SaveFailure::Changed(f)),
        Err(e) => Err(SaveFailure::Text(write_error(path, e))),
    }
}

/// [`write_document`] with the form and the files to copy from.
#[cfg(all(not(target_arch = "wasm32"), test))]
pub fn write_document_as(
    path: &Path,
    doc: &Document,
    spec: &SaveSpec,
) -> Result<Vec<String>, String> {
    try_write_document(path, doc, spec).map_err(SaveFailure::text)
}

/// Asks the person whether to save without the parts that cannot be found in the file the
/// document was opened from. The default is no.
#[cfg(not(target_arch = "wasm32"))]
pub fn ask_save_without(m: &kadakas_file::Missing) -> bool {
    #[cfg(test)]
    if let Some(answer) = test_answer::get() {
        return answer;
    }
    let what = crate::autosave::missing_text(m);
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title(tr!("app-save-missing-title"))
        .set_description(tr!(
            "app-save-missing-text",
            file = m.file.clone(),
            what = what
        ))
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes
}

/// Asks the person whether to replace a file that another program changed since it was opened
/// (two instances, a sync conflict copy). The default is no.
#[cfg(not(target_arch = "wasm32"))]
pub fn ask_overwrite(file: &str) -> bool {
    #[cfg(test)]
    if let Some(answer) = test_answer::get() {
        return answer;
    }
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title(tr!("app-save-overwrite-title"))
        .set_description(tr!("app-save-overwrite-text", file = file))
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes
}

/// The answer of the person to [`ask_save_without`], set by a test.
#[cfg(all(not(target_arch = "wasm32"), test))]
pub mod test_answer {
    use std::cell::Cell;

    use std::cell::RefCell;
    use std::collections::VecDeque;

    thread_local! {
        static ANSWER: Cell<Option<bool>> = const { Cell::new(None) };
        static QUEUE: RefCell<VecDeque<bool>> = const { RefCell::new(VecDeque::new()) };
    }

    /// The answer to every question (None: no test answer).
    pub fn set(answer: Option<bool>) {
        ANSWER.with(|a| a.set(answer));
    }

    /// The answers to the next questions, in order (a save can ask twice).
    pub fn queue(answers: &[bool]) {
        QUEUE.with(|q| *q.borrow_mut() = answers.iter().copied().collect());
    }

    pub fn get() -> Option<bool> {
        QUEUE
            .with(|q| q.borrow_mut().pop_front())
            .or_else(|| ANSWER.with(Cell::get))
    }
}

/// The text for a file that cannot be written.
#[cfg(not(target_arch = "wasm32"))]
fn write_error(path: &Path, e: FileError) -> String {
    match e {
        FileError::Io(_) => tr!("file-io-cannot-write", path = path.display(), e = e),
        other => format!("{}: {other}", path.display()),
    }
}

/// `write_document` without the manual save bookkeeping.
#[cfg(all(not(target_arch = "wasm32"), test))]
pub fn write_document_raw(path: &Path, doc: &Document) -> Result<(), String> {
    kadakas_api::files::write_document(path, doc, &SaveSpec::from_extension(path, Vec::new()))
        .map(|_| ())
        .map_err(|e| write_error(path, e))
}

/// Writes the SVG of the session's document (the same SVG as `render.svg`), atomically.
pub fn write_svg(path: &Path, session: &Session) -> Result<(), String> {
    fsio::write_atomic(path, session.render_svg(1600).as_bytes())
        .map_err(|e| tr!("file-io-cannot-write", path = path.display(), e = e))
}

/// Adds `.kad` unless the name already ends in `.kad` or `.json` (the plain JSON form, DOC-22).
pub fn with_extension(path: &Path) -> std::path::PathBuf {
    let s = path.to_string_lossy();
    if s.ends_with(".kad") || s.ends_with(".json") {
        path.to_path_buf()
    } else {
        format!("{s}.kad").into()
    }
}

/// What an agent request changed in the app besides the document.
#[derive(Debug, Default, PartialEq)]
#[cfg(not(target_arch = "wasm32"))]
pub struct AgentEffects {
    /// The document was replaced: the app must cancel the active tool and refit the view.
    pub replaced: bool,
    /// The file path changed: the app must update the window title.
    pub path_changed: bool,
}

/// Answers one request line from the agent connection `conn`. Like `Session::handle_line_as`, and
/// also `file.save {path}` and `file.open {path}`, which need the app's file path.
#[cfg(not(target_arch = "wasm32"))]
pub fn agent_line(
    session: &mut Session,
    path: &mut Option<PathBuf>,
    conn: u64,
    line: &str,
    fx: &mut AgentEffects,
) -> Option<String> {
    let req: Option<Value> = serde_json::from_str(line).ok();
    let method = req
        .as_ref()
        .and_then(|r| r["method"].as_str())
        .filter(|m| matches!(*m, "file.save" | "file.open"));
    let (Some(req), Some(method)) = (&req, method) else {
        let resp = session.handle_line_as(conn, line);
        if session.take_replaced() {
            // `document.load`: the new document does not come from the file that was open.
            *path = None;
            fx.replaced = true;
            fx.path_changed = true;
        }
        return resp;
    };
    // An agent may only use the allowed folders (see `Session::paths`) and only document files.
    // The same handler serves `kadakas serve` and `kadakas mcp`. A save here counts as a manual save.
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    // An agent that passes a consent token does not decide for the person: the app asks them (the
    // default answer is no).
    let result = kadakas_api::files::file_call(
        session,
        method,
        &params,
        &mut |p, doc, spec| {
            crate::autosave::manual_save(p, || kadakas_api::files::write_document(p, doc, spec))
        },
        &mut |ask, _token_matches| match ask {
            kadakas_api::files::Ask::Missing(m) => ask_save_without(m),
            kadakas_api::files::Ask::Overwrite(p) => ask_overwrite(&p.display().to_string()),
        },
    );
    match &result {
        Ok(kadakas_api::files::FileDone::Saved(p, ..)) => {
            *path = Some(p.clone());
            fx.path_changed = true;
        }
        Ok(kadakas_api::files::FileDone::Opened(p, ..)) => {
            *path = Some(p.clone());
            fx.replaced = true;
            fx.path_changed = true;
        }
        Err(_) => {}
    }
    let id = req.get("id")?.clone();
    Some(kadakas_api::files::file_reply(session, method, id, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;
    use kadakas_api::Session;
    use kadakas_core::command::Command;
    use kadakas_core::model::{Attributes, FORMAT_VERSION, Shape};
    use serde_json::json;

    #[test]
    fn save_and_load_roundtrip() {
        let mut s = Session::default();
        s.editor
            .execute(&Command::AddObject {
                shape: Shape::Circle {
                    center: DVec2::new(1.0, 2.0),
                    radius: 3.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        let path = std::env::temp_dir().join(format!("kadakas-test-{}.kad", std::process::id()));
        write_document(&path, s.editor.doc()).unwrap();
        assert_eq!(read_document(&path).unwrap(), s.editor.doc().clone());
        let svg = std::env::temp_dir().join(format!("kadakas-test-{}.svg", std::process::id()));
        write_svg(&svg, &s).unwrap();
        assert!(std::fs::read_to_string(&svg).unwrap().contains("<circle"));
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(svg);
    }

    #[test]
    fn rejects_garbage_and_newer_formats() {
        assert!(parse_document("{nope").is_err());
        let mut v = serde_json::to_value(Document::default()).unwrap();
        v["format_version"] = serde_json::json!(FORMAT_VERSION + 1);
        assert!(
            parse_document(&v.to_string())
                .unwrap_err()
                .contains("newer")
        );
    }

    #[test]
    fn streamed_save_and_load_match_the_old_text_and_the_size_limit_holds() {
        let dir = std::env::temp_dir().join(format!("kadakas-fileio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let doc = Document::with_sequential_ids();
        // The plain JSON form is the old text, byte for byte.
        let path = dir.join("a.json");
        write_document(&path, &doc).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            serde_json::to_string_pretty(&doc).unwrap()
        );
        assert_eq!(
            parse_document_bytes(&std::fs::read(&path).unwrap()).unwrap(),
            doc
        );
        // The container reads back the same.
        let kad = dir.join("a.kad");
        write_document(&kad, &doc).unwrap();
        assert!(std::fs::read(&kad).unwrap().starts_with(b"PK"));
        assert_eq!(
            parse_document_bytes(&std::fs::read(&kad).unwrap()).unwrap(),
            doc
        );
        assert_eq!(read_document(&kad).unwrap(), doc);
        assert_eq!(open_file(&kad).unwrap().doc, doc);
        assert!(parse_document_bytes(b"{\"format_version\":").is_err());
        let text = std::fs::read(&path).unwrap();
        assert!(parse_document_bytes_capped(&text, text.len() as u64).is_ok());
        let e = parse_document_bytes_capped(&text, text.len() as u64 - 1).unwrap_err();
        assert!(e.contains("limit"), "{e}");
        // A container is limited by the bytes of its entries too.
        let packed = std::fs::read(&kad).unwrap();
        let e = parse_document_bytes_capped(&packed, packed.len() as u64).unwrap_err();
        assert!(e.contains("limit"), "{e}");
        // A file of a person has no fixed cap: a hole of 3 GiB is judged by `open_decision`
        // (a question or a refusal, by the memory of the computer), and the read itself fails
        // on the first byte because the file is not JSON.
        let big = dir.join("big.kad");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(3 * 1024 * 1024 * 1024)
            .unwrap();
        assert!(matches!(
            open_decision(&big),
            OpenDecision::Ask(_) | OpenDecision::Refuse(_)
        ));
        assert!(read_document(&big).is_err());
        assert_eq!(open_decision(&path), OpenDecision::Open);
        assert_eq!(open_decision(&kad), OpenDecision::Open);
        assert_eq!(open_decision(&dir.join("missing.kad")), OpenDecision::Open);
        assert_eq!(open_decision(&dir), OpenDecision::Open);
        assert!(read_document(&dir.join("missing.kad")).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn extension_helper() {
        assert_eq!(with_extension(Path::new("a")).to_str(), Some("a.kad"));
        assert_eq!(with_extension(Path::new("a.kad")).to_str(), Some("a.kad"));
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kadakas-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn call(
        s: &mut Session,
        path: &mut Option<PathBuf>,
        fx: &mut AgentEffects,
        req: Value,
    ) -> Value {
        serde_json::from_str(&agent_line(s, path, 0, &req.to_string(), fx).unwrap()).unwrap()
    }

    #[test]
    fn saving_keeps_a_backup_and_leaves_no_temp_files() {
        let dir = temp("save");
        let p = dir.join("a.kad");
        let mut doc = Document::default();
        write_document(&p, &doc).unwrap();
        doc.layers[0].name = "Second".into();
        write_document(&p, &doc).unwrap();
        assert_eq!(read_document(&p).unwrap().layers[0].name, "Second");
        assert_eq!(
            read_document(&dir.join("a.kad.bak")).unwrap().layers[0].name,
            "Layer 1"
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn restricted_agents_may_use_kad_and_json_files_but_not_other_extensions() {
        let dir = temp("agent-ext");
        let mut s = Session::default();
        s.paths = kadakas_api::PathPolicy::restricted(vec![dir.clone()]);
        let (mut path, mut fx) = (None, AgentEffects::default());
        for (name, ok) in [("a.kad", true), ("b.json", true), ("c.txt", false)] {
            let p = dir.join(name);
            let r = call(
                &mut s,
                &mut path,
                &mut fx,
                json!({"id": 1, "method": "file.save", "params": {"path": p}}),
            );
            assert_eq!(r.get("error").is_none(), ok, "{name}: {r}");
            assert_eq!(p.exists(), ok, "{name}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn agent_file_save_and_open_use_the_app_path() {
        let dir = temp("agent-files");
        let p = dir.join("plan.kad");
        let mut s = Session::default();
        let mut path = None;
        let mut fx = AgentEffects::default();
        let circle =
            json!({"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}});
        call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 1, "method": "execute", "params": {"command": circle}}),
        );
        assert_eq!(
            fx,
            AgentEffects::default(),
            "normal requests change nothing in the app"
        );

        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 2, "method": "file.save", "params": {"path": p}}),
        );
        assert_eq!(r["result"], json!({"ok": true, "format": "container"}));
        assert_eq!(path.as_deref(), Some(p.as_path()));
        assert_eq!(
            fx,
            AgentEffects {
                replaced: false,
                path_changed: true
            }
        );
        assert_eq!(read_document(&p).unwrap().objects.len(), 1);

        // Open a different document: the session is replaced and the app is told.
        let other = dir.join("other.kad");
        write_document(&other, &Document::default()).unwrap();
        let mut fx = AgentEffects::default();
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 3, "method": "file.open", "params": {"path": other}}),
        );
        assert_eq!(r["result"], json!({"ok": true, "format": "container"}));
        assert_eq!(
            fx,
            AgentEffects {
                replaced: true,
                path_changed: true
            }
        );
        assert_eq!(path.as_deref(), Some(other.as_path()));
        assert!(s.editor.doc().objects.is_empty());
        assert!(
            !s.take_replaced(),
            "the flag was consumed by the app handler"
        );

        // Errors: missing path, missing file, newer format. The old state stays.
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 4, "method": "file.open", "params": {}}),
        );
        assert_eq!(r["error"]["code"], -32602);
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 5, "method": "file.open", "params": {"path": dir.join("nope")}}),
        );
        assert_eq!(r["error"]["code"], 2);
        let mut v = serde_json::to_value(Document::default()).unwrap();
        v["format_version"] = json!(FORMAT_VERSION + 1);
        std::fs::write(dir.join("new.json"), v.to_string()).unwrap();
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 6, "method": "file.open", "params": {"path": dir.join("new.json")}}),
        );
        assert!(r["error"]["message"].as_str().unwrap().contains("newer"));
        assert_eq!(path.as_deref(), Some(other.as_path()));
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 7, "method": "file.save", "params": {"path": dir.join("no").join("such").join("f")}}),
        );
        assert_eq!(r["error"]["code"], 1);

        // A notification gets no reply.
        let mut fx = AgentEffects::default();
        assert!(
            agent_line(
                &mut s,
                &mut path,
                0,
                &json!({"method": "file.save", "params": {"path": p}}).to_string(),
                &mut fx
            )
            .is_none()
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn agent_document_load_clears_the_file_path_and_tells_the_app() {
        let mut s = Session::default();
        let mut path = Some(PathBuf::from("/tmp/old.kad"));
        let mut fx = AgentEffects::default();
        let doc = serde_json::to_value(Document::default()).unwrap();
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 1, "method": "document.load", "params": {"document": doc}}),
        );
        assert_eq!(r["result"], json!({"ok": true}));
        assert_eq!(path, None, "the new document is not the file that was open");
        assert_eq!(
            fx,
            AgentEffects {
                replaced: true,
                path_changed: true
            }
        );

        // A refused load changes nothing.
        let mut path = Some(PathBuf::from("/tmp/keep.kad"));
        let mut fx = AgentEffects::default();
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 2, "method": "document.load", "params": {"document": {"format_version": 99}}}),
        );
        assert!(r.get("error").is_some());
        assert_eq!(path, Some(PathBuf::from("/tmp/keep.kad")));
        assert_eq!(fx, AgentEffects::default());

        // Lines that are not JSON still get the normal parse error.
        let line = agent_line(&mut s, &mut path, 0, "{nope", &mut fx).unwrap();
        assert!(line.contains("-32700"));
    }

    #[test]
    fn agent_can_render_and_switch_the_view_in_the_live_app() {
        let mut s = Session::default();
        let mut path = None;
        let mut fx = AgentEffects::default();
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 1, "method": "view.set", "params": {"view": "front", "mode": "wireframe"}}),
        );
        assert_eq!(
            r["result"],
            json!({"view": "front", "projection": "ortho", "mode": "wireframe"})
        );
        assert_eq!(
            fx,
            AgentEffects::default(),
            "the app polls the session for view changes"
        );
        assert!(s.take_view_changed());
        let r = call(
            &mut s,
            &mut path,
            &mut fx,
            json!({"id": 2, "method": "render.png", "params": {"width": 32, "height": 32}}),
        );
        assert_eq!(r["result"]["view"], "front");
        assert!(
            r["result"]["png_base64"]
                .as_str()
                .unwrap()
                .starts_with("iVBOR")
        );
    }
}
