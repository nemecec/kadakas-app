//! Where each choice of the dialog is stored.
//!
//! | Choice | File | Key |
//! |---|---|---|
//! | how to work | `launcher.json` (install folder) | `flavor` |
//! | updates | `launcher.json` | `mode` |
//! | the dialog was seen | `launcher.json` | `welcome_done` |
//! | language | `prefs.json` (config folder) and `modeler.json` | `language` |
//! | units | `prefs.json` | `units.length`, `units.area` |
//! | number format | `prefs.json` | `units.decimal`, `units.grouping` |
//! | theme | `prefs.json` and `modeler.json` | `theme` |
//! | mouse or trackpad | `prefs.json` | `navigation` (`modeler.json` loses its own `navigation`) |
//! | crash reports | `settings.json` (feedback folder) | `crash_policy` |
//!
//! A file is read as JSON, changed key by key and written again, so keys that this version does
//! not know stay. A file that cannot be read is replaced; the writer keeps the old one as `.bak`.

use crate::choices::{
    Choices, Mode, Navigation, NumberStyle, Theme, UnitSystem, detect, detect_number_style,
};
use crate::gate::WELCOME_VERSION;
use kadakas_feedback::Hub;
use kadakas_feedback::store::DirStorage;
use kadakas_units::{Decimal, Grouping, LengthUnit};
use kadakas_update::Settings;
use serde_json::{Map, Value};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The folders of the three stores.
#[derive(Clone, Debug)]
pub struct Dirs {
    /// The install folder of the launcher (`launcher.json`).
    pub launcher: PathBuf,
    /// The config folder of the app and the modeler (`prefs.json`, `modeler.json`). `None`: there
    /// is no home folder, so nothing is written there.
    pub prefs: Option<PathBuf>,
    /// The folder of the feedback settings (the crash policy and the installation ID).
    pub feedback: Option<PathBuf>,
}

impl Dirs {
    /// The folders for the values of `KADAKAS_PREFS_DIR` and `KADAKAS_FEEDBACK_DIR` (the same
    /// rules as in the app and the modeler).
    pub fn new(
        launcher: PathBuf,
        prefs_env: Option<OsString>,
        feedback_env: Option<OsString>,
    ) -> Dirs {
        Dirs {
            launcher,
            prefs: prefs_env
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .or_else(kadakas_sys::dirs::config_dir),
            feedback: kadakas_feedback::store::default_dir_for(feedback_env),
        }
    }

    pub fn from_env(launcher: PathBuf) -> Dirs {
        Dirs::new(
            launcher,
            std::env::var_os("KADAKAS_PREFS_DIR"),
            std::env::var_os("KADAKAS_FEEDBACK_DIR"),
        )
    }
}

/// Reads a JSON file as an object. A missing or damaged file gives an empty object.
fn read_object(path: &Path) -> Map<String, Value> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| match v {
            Value::Object(o) => Some(o),
            _ => None,
        })
        .unwrap_or_default()
}

/// Sets and removes keys of a JSON file and keeps all others.
pub fn merge_json(
    path: &Path,
    change: impl FnOnce(&mut Map<String, Value>),
) -> std::io::Result<()> {
    let mut object = read_object(path);
    change(&mut object);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_vec_pretty(&Value::Object(object)).map_err(std::io::Error::other)?;
    kadakas_sys::fsio::write_atomic(path, &text)
}

fn to_value<T: serde::Serialize>(v: T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// Stores the choices and marks the dialog as seen. Returns one line for each file that could
/// not be written; the other files are written anyway.
pub fn apply(dirs: &Dirs, c: &Choices) -> Vec<String> {
    let mut problems = Vec::new();
    let mut note = |what: &str, r: std::io::Result<()>| {
        if let Err(e) = r {
            problems.push(format!("{what}: {e}"));
        }
    };
    note(
        "launcher.json",
        Settings::update(&dirs.launcher, |s| {
            s.flavor = c.mode.flavor().to_string();
            s.mode = c.updates;
            s.welcome_done = WELCOME_VERSION;
        })
        .map(|_| ()),
    );
    if let Some(prefs) = &dirs.prefs {
        let language = c
            .language
            .as_deref()
            .map_or_else(|| Value::from("system"), Value::from);
        note(
            "prefs.json",
            merge_json(&prefs.join("prefs.json"), |o| {
                o.insert("language".into(), language);
                o.insert("theme".into(), Value::from(c.theme.key()));
                o.insert("navigation".into(), Value::from(c.navigation.key()));
                let mut units = match o.remove("units") {
                    Some(Value::Object(u)) => u,
                    _ => Map::new(),
                };
                units.insert("length".into(), to_value(c.units.length()));
                units.insert("area".into(), to_value(c.units.area()));
                units.insert("decimal".into(), to_value(c.number.decimal()));
                units.insert("grouping".into(), to_value(c.number.grouping()));
                o.insert("units".into(), Value::Object(units));
            }),
        );
        note(
            "modeler.json",
            merge_json(&prefs.join("modeler.json"), |o| {
                o.insert(
                    "language".into(),
                    c.language.as_deref().map_or(Value::Null, Value::from),
                );
                o.insert("theme".into(), Value::from(c.theme.key()));
                // The choice of the app counts for the modeler too, until the person changes it
                // in the modeler.
                o.remove("navigation");
            }),
        );
    }
    if let Some(dir) = &dirs.feedback {
        let mut hub = Hub::new(Box::new(DirStorage::new(dir.clone())));
        note(
            "settings.json (crash reports)",
            hub.set_crash_policy(c.crash)
                .map_err(|e| std::io::Error::other(e.to_string())),
        );
    }
    problems
}

/// Marks the dialog as seen and changes nothing else.
pub fn mark_done(dirs: &Dirs) -> Vec<String> {
    match Settings::update(&dirs.launcher, |s| s.welcome_done = WELCOME_VERSION) {
        Ok(_) => Vec::new(),
        Err(e) => vec![format!("launcher.json: {e}")],
    }
}

/// The choices that are stored now. What a store does not have is what the system gives
/// (`detect`).
pub fn current(dirs: &Dirs, system: &[String], modeler_installed: bool) -> Choices {
    let mut c = detect(system, modeler_installed);
    let s = Settings::load(&dirs.launcher);
    c.mode = Mode::from_flavor(&s.flavor);
    c.updates = s.mode;
    if let Some(prefs) = &dirs.prefs {
        let o = read_object(&prefs.join("prefs.json"));
        if let Some(l) = o.get("language").and_then(Value::as_str) {
            c.language = kadakas_l10n::languages()
                .iter()
                .find(|x| x.tag.eq_ignore_ascii_case(l))
                .map(|x| x.tag.to_string());
        }
        if let Some(t) = o.get("theme").and_then(Value::as_str) {
            c.theme = Theme::from_key(t);
        }
        if let Some(n) = o.get("navigation").and_then(Value::as_str) {
            c.navigation = Navigation::from_key(n);
        }
        let units = o.get("units").and_then(Value::as_object);
        let get = |key: &str| units.and_then(|u| u.get(key)).cloned();
        if let Some(l) = get("length").and_then(|v| serde_json::from_value::<LengthUnit>(v).ok()) {
            c.units = UnitSystem::from_length(l);
        }
        // The number style: the stored pair; a missing half follows the language.
        let language_list: Vec<String> = match &c.language {
            Some(t) => vec![t.clone()],
            None => system.to_vec(),
        };
        let convention = detect_number_style(&language_list);
        let decimal = get("decimal")
            .and_then(|v| serde_json::from_value::<Decimal>(v).ok())
            .unwrap_or(convention.decimal());
        let grouping = get("grouping")
            .and_then(|v| serde_json::from_value::<Grouping>(v).ok())
            .unwrap_or(convention.grouping());
        c.number = NumberStyle::from_pair(decimal, grouping).unwrap_or(convention);
    }
    if let Some(dir) = &dirs.feedback {
        c.crash = Hub::new(Box::new(DirStorage::new(dir.clone()))).crash_policy();
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::tests::tmp;
    use kadakas_feedback::CrashPolicy;
    use kadakas_update::settings::UpdateMode;

    fn dirs(name: &str) -> Dirs {
        let base = tmp(name);
        Dirs {
            launcher: base.join("launcher"),
            prefs: Some(base.join("config")),
            feedback: Some(base.join("feedback")),
        }
    }

    fn json(path: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn sys(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn cleanup(d: &Dirs) {
        let _ = std::fs::remove_dir_all(d.launcher.parent().unwrap());
    }

    #[test]
    fn every_choice_lands_in_its_file_and_key() {
        let d = dirs("stores-all");
        let c = Choices {
            mode: Mode::Landscape,
            language: Some("et".into()),
            units: UnitSystem::Imperial,
            number: NumberStyle::DotComma,
            theme: Theme::Dark,
            navigation: Navigation::Trackpad,
            crash: CrashPolicy::Never,
            updates: UpdateMode::NotifyOnly,
        };
        assert!(apply(&d, &c).is_empty());
        let l = json(&Settings::path(&d.launcher));
        assert_eq!(l["flavor"], "kadakas-app");
        assert_eq!(l["mode"], "notify-only");
        assert_eq!(l["welcome_done"], WELCOME_VERSION);
        let prefs = d.prefs.clone().unwrap();
        let p = json(&prefs.join("prefs.json"));
        assert_eq!(p["language"], "et");
        assert_eq!(p["theme"], "dark");
        assert_eq!(p["navigation"], "trackpad");
        assert_eq!(p["units"]["length"], "ft_in");
        assert_eq!(p["units"]["area"], "ft2");
        assert_eq!(p["units"]["decimal"], "comma");
        assert_eq!(p["units"]["grouping"], "point");
        let m = json(&prefs.join("modeler.json"));
        assert_eq!(
            (m["language"].as_str(), m["theme"].as_str()),
            (Some("et"), Some("dark"))
        );
        let hub = Hub::new(Box::new(DirStorage::new(d.feedback.clone().unwrap())));
        assert_eq!(hub.crash_policy(), CrashPolicy::Never);
        // And it reads back as it was written.
        assert_eq!(current(&d, &sys(&["en-US"]), true), c);
        cleanup(&d);
    }

    #[test]
    fn the_language_of_the_system_is_stored_as_system() {
        let d = dirs("stores-system");
        let c = detect(&sys(&["en-GB"]), true);
        assert!(apply(&d, &c).is_empty());
        let prefs = d.prefs.clone().unwrap();
        assert_eq!(json(&prefs.join("prefs.json"))["language"], "system");
        assert!(json(&prefs.join("modeler.json"))["language"].is_null());
        assert_eq!(
            json(&Settings::path(&d.launcher))["flavor"],
            "kadakas-modeler"
        );
        assert_eq!(current(&d, &sys(&["en-GB"]), true).language, None);
        cleanup(&d);
    }

    #[test]
    fn keys_that_the_dialog_does_not_know_stay_in_every_file() {
        let d = dirs("stores-keep");
        let prefs = d.prefs.clone().unwrap();
        std::fs::create_dir_all(&prefs).unwrap();
        std::fs::create_dir_all(&d.launcher).unwrap();
        std::fs::write(
            prefs.join("prefs.json"),
            r#"{"cues":false,"future":[1],"units":{"angle":"dms","inch_fraction":8,"length":"mm"}}"#,
        )
        .unwrap();
        std::fs::write(
            prefs.join("modeler.json"),
            r#"{"tray":false,"navigation":"trackpad","import_soften":{"x":1}}"#,
        )
        .unwrap();
        std::fs::write(
            Settings::path(&d.launcher),
            r#"{"channel":"beta","keep_older":4,"future_key":"x"}"#,
        )
        .unwrap();
        // The feedback settings: other keys are the hub's to keep.
        std::fs::create_dir_all(d.feedback.as_ref().unwrap()).unwrap();
        std::fs::write(
            d.feedback.as_ref().unwrap().join("settings.json"),
            r#"{"installation":"abc","omit_installation":true}"#,
        )
        .unwrap();
        let mut c = detect(&sys(&["en-US"]), true);
        c.crash = CrashPolicy::Always;
        assert!(apply(&d, &c).is_empty());
        let p = json(&prefs.join("prefs.json"));
        assert_eq!(
            (p["cues"].as_bool(), p["future"][0].as_i64()),
            (Some(false), Some(1))
        );
        assert_eq!(
            (
                p["units"]["angle"].as_str(),
                p["units"]["inch_fraction"].as_i64()
            ),
            (Some("dms"), Some(8))
        );
        assert_eq!(p["units"]["length"], "ft_in");
        let m = json(&prefs.join("modeler.json"));
        assert_eq!(
            (m["tray"].as_bool(), m["import_soften"]["x"].as_i64()),
            (Some(false), Some(1))
        );
        assert!(
            m.get("navigation").is_none(),
            "the app's choice counts for the modeler"
        );
        let l = json(&Settings::path(&d.launcher));
        assert_eq!(
            (
                l["channel"].as_str(),
                l["keep_older"].as_i64(),
                l["future_key"].as_str()
            ),
            (Some("beta"), Some(4), Some("x"))
        );
        let f = json(&d.feedback.clone().unwrap().join("settings.json"));
        assert_eq!(
            (f["installation"].as_str(), f["omit_installation"].as_bool()),
            (Some("abc"), Some(true))
        );
        assert_eq!(f["crash_policy"], "always");
        cleanup(&d);
    }

    #[test]
    fn a_damaged_file_is_replaced_and_the_old_text_is_kept_aside() {
        let d = dirs("stores-damaged");
        let prefs = d.prefs.clone().unwrap();
        std::fs::create_dir_all(&prefs).unwrap();
        std::fs::write(prefs.join("prefs.json"), "{ nope").unwrap();
        assert!(apply(&d, &detect(&[], true)).is_empty());
        assert_eq!(json(&prefs.join("prefs.json"))["theme"], "system");
        assert_eq!(
            std::fs::read_to_string(prefs.join("prefs.json.bak")).unwrap(),
            "{ nope"
        );
        cleanup(&d);
    }

    #[test]
    fn a_folder_that_cannot_be_made_is_reported_and_the_other_files_are_written() {
        let d = dirs("stores-fail");
        let blocker = d.launcher.parent().unwrap().join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        let broken = Dirs {
            prefs: Some(blocker.join("config")),
            ..d.clone()
        };
        let problems = apply(&broken, &detect(&[], true));
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].starts_with("prefs.json"));
        assert!(Settings::load(&d.launcher).welcome_done == WELCOME_VERSION);
        // A launcher folder that cannot be made.
        let bad_launcher = Dirs {
            launcher: blocker.join("l"),
            ..d.clone()
        };
        assert!(!mark_done(&bad_launcher).is_empty());
        assert!(apply(&bad_launcher, &detect(&[], true))[0].starts_with("launcher.json"));
        cleanup(&d);
    }

    #[test]
    fn nothing_stored_gives_the_defaults_of_the_system() {
        let d = dirs("stores-empty");
        let c = current(&d, &sys(&["et-EE"]), true);
        assert_eq!(c.number, NumberStyle::SpaceComma);
        assert_eq!(c.units, UnitSystem::Metric);
        assert_eq!(c.crash, CrashPolicy::Ask);
        // A stored language changes the number convention that is used when none is stored.
        std::fs::create_dir_all(d.prefs.as_ref().unwrap()).unwrap();
        std::fs::write(
            d.prefs.as_ref().unwrap().join("prefs.json"),
            r#"{"language":"et","theme":"neon","navigation":3,"units":{"length":"banana","decimal":"point"}}"#,
        )
        .unwrap();
        let c = current(&d, &sys(&["en-US"]), true);
        assert_eq!(c.language.as_deref(), Some("et"));
        assert_eq!(
            (c.theme, c.navigation, c.units),
            (Theme::System, Navigation::Mouse, UnitSystem::Imperial)
        );
        // decimal point + space grouping is not offered: the convention of the language is used.
        assert_eq!(c.number, NumberStyle::SpaceComma);
        cleanup(&d);
    }

    #[test]
    fn the_folders_follow_the_variables() {
        let d = Dirs::new(PathBuf::from("/l"), Some("/p".into()), Some("/f".into()));
        assert_eq!(
            (d.prefs, d.feedback),
            (Some(PathBuf::from("/p")), Some(PathBuf::from("/f")))
        );
        let d = Dirs::new(
            PathBuf::from("/l"),
            Some(OsString::new()),
            Some(OsString::new()),
        );
        if let Some(p) = d.prefs {
            assert!(p.ends_with("kadakas"));
        }
        let _ = Dirs::from_env(PathBuf::from("/l"));
    }
}
