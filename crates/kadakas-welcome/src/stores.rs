//! Where each choice of the dialog is stored, and how it is read back.
//!
//! | Choice | File | Key |
//! |---|---|---|
//! | how to work | `launcher.json` (install folder) | `flavor` |
//! | updates | `launcher.json` | `mode` |
//! | the dialog was seen | `welcome.json` (install folder) | `done` |
//! | language | `prefs.json` (prefs folder) and `modeler.json` | `language` |
//! | units | `prefs.json` | `units.length`, `units.area` |
//! | number format | `prefs.json` | `units.decimal`, `units.grouping` |
//! | theme | `prefs.json` and `modeler.json` | `theme` |
//! | mouse or trackpad | `prefs.json` (`modeler.json` loses its own `navigation`) | `navigation` |
//! | crash reports | `settings.json` (feedback folder) | `crash_policy` |
//!
//! Rules:
//!
//! - A file is changed key by key under a lock (`kadakas_sys::jsonfile`). Keys that this version
//!   does not know stay.
//! - Only the keys that the person changed are written ([`apply`] with the choices that the
//!   dialog showed at the start). A first start has nothing shown to compare with and writes
//!   every key; "Use defaults" writes the keys that are missing and never replaces a value.
//! - What the dialog shows as current is what the programs really use ([`current`]): the same
//!   files, the same defaults, the same precedence (the modeler's own file wins for the modeler).
//! - A file that cannot be parsed is kept as `<name>.bak`; a read error other than "not found"
//!   stops the write and is reported.

use crate::choices::{Choices, Mode, Navigation, NumberStyle, Theme, UnitSystem, detect};
use crate::gate::WELCOME_VERSION;
use crate::sysloc::SystemLocale;
use kadakas_feedback::store::DirStorage;
use kadakas_feedback::{CrashPolicy, Hub};
use kadakas_sys::jsonfile;
use kadakas_units::{Decimal, Grouping, LengthUnit};
use kadakas_update::Settings;
use serde_json::{Map, Value};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The folders of the stores.
#[derive(Clone, Debug)]
pub struct Dirs {
    /// The install folder of the launcher (`launcher.json`, `welcome.json`).
    pub launcher: PathBuf,
    /// The prefs folder of the app and the modeler (`prefs.json`, `modeler.json`). `None`: there
    /// is no home folder, so nothing is written there.
    pub prefs: Option<PathBuf>,
    /// The folder of the feedback settings (the crash policy and the installation ID).
    pub feedback: Option<PathBuf>,
}

impl Dirs {
    /// The folders for a prefs folder and the value of `KADAKAS_FEEDBACK_DIR`.
    pub fn new(launcher: PathBuf, prefs: Option<PathBuf>, feedback_env: Option<OsString>) -> Dirs {
        Dirs {
            launcher,
            prefs,
            feedback: kadakas_feedback::store::default_dir_for(feedback_env),
        }
    }

    /// The folders that the app and the modeler use (`kadakas_sys::dirs::prefs_dir`).
    pub fn from_env(launcher: PathBuf) -> Dirs {
        Dirs::new(
            launcher,
            kadakas_sys::dirs::prefs_dir(),
            std::env::var_os("KADAKAS_FEEDBACK_DIR"),
        )
    }

    fn prefs_file(&self) -> Option<PathBuf> {
        self.prefs.as_ref().map(|d| d.join("prefs.json"))
    }

    fn modeler_file(&self) -> Option<PathBuf> {
        self.prefs.as_ref().map(|d| d.join("modeler.json"))
    }

    fn feedback_file(&self) -> Option<PathBuf> {
        self.feedback.as_ref().map(|d| d.join("settings.json"))
    }
}

/// The names in the install folder that show an earlier use of Kadakas.
const INSTALL_TRACES: [&str; 4] = ["launcher.json", "versions", "current", "runs"];

/// True when something shows that Kadakas was used on this computer: the install folder of the
/// launcher, or a settings file of the app, the modeler or the feedback hub (a reinstall, or a
/// launcher folder that moved, keeps the settings of the person).
pub fn has_traces(dirs: &Dirs) -> bool {
    INSTALL_TRACES
        .iter()
        .any(|n| dirs.launcher.join(n).exists())
        || [dirs.prefs_file(), dirs.modeler_file(), dirs.feedback_file()]
            .into_iter()
            .flatten()
            .any(|f| f.exists())
}

fn to_value<T: serde::Serialize>(v: T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// One key to write: the path in the file and the value (`None` removes the key).
type Edit = (Vec<String>, Option<Value>);

/// The keys to write, by file.
#[derive(Debug, Default, PartialEq)]
struct Plan {
    flavor: Option<Mode>,
    update_mode: Option<kadakas_update::settings::UpdateMode>,
    prefs: Vec<Edit>,
    modeler: Vec<Edit>,
    crash: Option<CrashPolicy>,
}

impl Plan {
    fn is_empty(&self) -> bool {
        *self == Plan::default()
    }
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// The keys for the choices that differ from `base` (every choice when there is no `base`).
fn plan(shown: &Choices, base: Option<&Choices>) -> Plan {
    let mut p = Plan::default();
    if base.is_none_or(|b| b.mode != shown.mode) {
        p.flavor = Some(shown.mode);
    }
    if base.is_none_or(|b| b.updates != shown.updates) {
        p.update_mode = Some(shown.updates);
    }
    if base.is_none_or(|b| b.language != shown.language) {
        let tag = shown
            .language
            .as_deref()
            .map_or_else(|| Value::from("system"), Value::from);
        p.prefs.push((path(&["language"]), Some(tag)));
        let own = shown.language.as_deref().map_or(Value::Null, Value::from);
        p.modeler.push((path(&["language"]), Some(own)));
    }
    if base.is_none_or(|b| b.theme != shown.theme) {
        let theme = Value::from(shown.theme.key());
        p.prefs.push((path(&["theme"]), Some(theme.clone())));
        p.modeler.push((path(&["theme"]), Some(theme)));
    }
    if base.is_none_or(|b| b.navigation != shown.navigation) {
        p.prefs.push((
            path(&["navigation"]),
            Some(Value::from(shown.navigation.key())),
        ));
        // The person chose; the choice of the modeler's own View menu no longer counts.
        p.modeler.push((path(&["navigation"]), None));
    }
    if base.is_none_or(|b| b.units != shown.units) {
        p.prefs.push((
            path(&["units", "length"]),
            Some(to_value(shown.units.length())),
        ));
        p.prefs
            .push((path(&["units", "area"]), Some(to_value(shown.units.area()))));
    }
    if base.is_none_or(|b| b.number != shown.number) {
        p.prefs.push((
            path(&["units", "decimal"]),
            Some(to_value(shown.number.decimal())),
        ));
        p.prefs.push((
            path(&["units", "grouping"]),
            Some(to_value(shown.number.grouping())),
        ));
    }
    if base.is_none_or(|b| b.crash != shown.crash) {
        p.crash = Some(shown.crash);
    }
    p
}

/// Writes the edits into a JSON file. With `only_missing`, a key that has a value stays.
fn write_edits(file: &Path, edits: &[Edit], only_missing: bool) -> std::io::Result<()> {
    if edits.is_empty() {
        return Ok(());
    }
    jsonfile::update(file, |o| {
        for (p, v) in edits {
            let parts: Vec<&str> = p.iter().map(String::as_str).collect();
            if only_missing && jsonfile::get_path(o, &parts).is_some() {
                continue;
            }
            jsonfile::set_path(o, p, v.clone());
        }
    })
}

fn run_plan(dirs: &Dirs, p: &Plan, only_missing: bool) -> Vec<String> {
    let mut problems = Vec::new();
    let mut note = |what: &str, r: std::io::Result<()>| {
        if let Err(e) = r {
            problems.push(format!("{what}: {e}"));
        }
    };
    if p.flavor.is_some() || p.update_mode.is_some() {
        let file = Settings::path(&dirs.launcher);
        let have = jsonfile::read_object(&file);
        note(
            "launcher.json",
            Settings::update(&dirs.launcher, |s| {
                if let Some(m) = p.flavor
                    && !(only_missing && have.contains_key("flavor"))
                {
                    s.flavor = m.flavor().to_string();
                }
                if let Some(u) = p.update_mode
                    && !(only_missing && have.contains_key("mode"))
                {
                    s.mode = u;
                }
            })
            .map(|_| ()),
        );
    }
    if let (Some(f), false) = (dirs.prefs_file(), p.prefs.is_empty()) {
        note("prefs.json", write_edits(&f, &p.prefs, only_missing));
    }
    if let (Some(f), false) = (dirs.modeler_file(), p.modeler.is_empty()) {
        // Removing a key that is not there writes nothing worth a file: skip a file that does
        // not exist and only removes.
        let only_removes = p.modeler.iter().all(|(_, v)| v.is_none());
        if !(only_removes && !f.exists()) {
            note("modeler.json", write_edits(&f, &p.modeler, only_missing));
        }
    }
    if let (Some(dir), Some(policy)) = (&dirs.feedback, p.crash) {
        let mut hub = Hub::new(Box::new(DirStorage::new(dir.clone())));
        let has = dirs
            .feedback_file()
            .is_some_and(|f| jsonfile::read_object(&f).contains_key("crash_policy"));
        if !(only_missing && has) {
            // A damaged file is kept as `.bak`, and a read error stops the write, as for the
            // other files (the hub itself would read a damaged file as empty).
            let guard = dirs
                .feedback_file()
                .map_or(Ok(Map::new()), |f| jsonfile::read_for_update(&f));
            note(
                "settings.json (crash reports)",
                guard.map(|_| ()).and_then(|()| {
                    hub.set_crash_policy(policy)
                        .map_err(|e| std::io::Error::other(e.to_string()))
                }),
            );
        }
    }
    problems
}

/// Stores the choices and marks the dialog as seen. With `base` (the choices that the dialog
/// showed at the start) only the choices that differ are written; without it, all of them.
/// Returns one line for each file that could not be written; the other files are written anyway.
pub fn apply(dirs: &Dirs, shown: &Choices, base: Option<&Choices>) -> Vec<String> {
    let p = plan(shown, base);
    let mut problems = run_plan(dirs, &p, false);
    problems.extend(mark(dirs, !p.is_empty()));
    problems
}

/// "Use defaults" on a first start: writes the defaults of the system for the keys that are
/// missing, and replaces nothing.
pub fn apply_missing(dirs: &Dirs, detected: &Choices) -> Vec<String> {
    let p = plan(detected, None);
    let mut problems = run_plan(dirs, &p, true);
    problems.extend(mark(dirs, true));
    problems
}

/// Marks the dialog as seen and changes nothing else.
pub fn mark_done(dirs: &Dirs) -> Vec<String> {
    mark(dirs, false)
}

fn mark(dirs: &Dirs, changed: bool) -> Vec<String> {
    match kadakas_update::welcome::write_mark(&dirs.launcher, WELCOME_VERSION, changed) {
        Ok(()) => Vec::new(),
        Err(e) => vec![format!("welcome.json: {e}")],
    }
}

fn text<'a>(o: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    o.get(key).and_then(Value::as_str)
}

fn known_language(tag: &str) -> Option<String> {
    kadakas_l10n::languages()
        .iter()
        .find(|l| l.tag.eq_ignore_ascii_case(tag))
        .map(|l| l.tag.to_string())
}

/// The choices that the programs use now. A key that is missing has the default of the program
/// that reads it (the app and the modeler have no region detection: lengths are millimeters, the
/// number format follows the language), not the default of this dialog.
///
/// For a person who works in the 3D modeler, language, theme and navigation come from
/// `modeler.json` first: its own values win for the modeler.
pub fn current(dirs: &Dirs, languages: &[String], modeler_installed: bool) -> Choices {
    let mut c = detect(
        &SystemLocale::from_languages(languages.to_vec()),
        modeler_installed,
    );
    let s = Settings::load(&dirs.launcher);
    c.mode = Mode::from_flavor(&s.flavor);
    c.updates = s.mode;
    let prefs = dirs
        .prefs_file()
        .map(|f| jsonfile::read_object(&f))
        .unwrap_or_default();
    let modeler = dirs
        .modeler_file()
        .map(|f| jsonfile::read_object(&f))
        .unwrap_or_default();
    let own = c.mode == Mode::Modeler;
    // Language: "system" or a tag in prefs.json; a tag or null in modeler.json.
    c.language = if own {
        text(&modeler, "language").and_then(known_language)
    } else {
        text(&prefs, "language").and_then(known_language)
    };
    c.theme = if own {
        text(&modeler, "theme")
    } else {
        text(&prefs, "theme")
    }
    .map_or(Theme::System, Theme::from_key);
    let app_navigation = text(&prefs, "navigation").map_or(Navigation::Mouse, Navigation::from_key);
    c.navigation = if own {
        text(&modeler, "navigation").map_or(app_navigation, Navigation::from_key)
    } else {
        app_navigation
    };
    // Units and number format are the app's, the modeler reads them from prefs.json too.
    let units = prefs.get("units").and_then(Value::as_object);
    let get = |key: &str| units.and_then(|u| u.get(key)).cloned();
    let length = get("length")
        .and_then(|v| serde_json::from_value::<LengthUnit>(v).ok())
        .unwrap_or_default();
    c.units = UnitSystem::from_length(length);
    let settings = kadakas_units::Settings {
        decimal: get("decimal").and_then(|v| serde_json::from_value::<Decimal>(v).ok()),
        grouping: get("grouping").and_then(|v| serde_json::from_value::<Grouping>(v).ok()),
        ..kadakas_units::Settings::default()
    };
    let tag = c.language_tag(languages);
    c.number = NumberStyle::nearest(settings.resolve(tag).number);
    if let Some(dir) = &dirs.feedback {
        c.crash = Hub::new(Box::new(DirStorage::new(dir.clone()))).crash_policy();
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::tests::tmp;
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

    fn langs(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn sys(list: &[&str]) -> SystemLocale {
        SystemLocale::from_languages(langs(list))
    }

    fn cleanup(d: &Dirs) {
        let _ = std::fs::remove_dir_all(d.launcher.parent().unwrap());
    }

    fn everything() -> Choices {
        Choices {
            mode: Mode::Landscape,
            language: Some("et".into()),
            units: UnitSystem::Imperial,
            number: NumberStyle::DotComma,
            theme: Theme::Dark,
            navigation: Navigation::Trackpad,
            crash: CrashPolicy::Never,
            updates: UpdateMode::NotifyOnly,
        }
    }

    #[test]
    fn a_first_start_writes_every_choice_to_its_file_and_key() {
        let d = dirs("stores-all");
        let c = everything();
        assert!(apply(&d, &c, None).is_empty());
        let l = json(&Settings::path(&d.launcher));
        assert_eq!(l["flavor"], "kadakas-app");
        assert_eq!(l["mode"], "notify-only");
        assert!(
            l.get("welcome_done").is_none(),
            "the mark is in welcome.json"
        );
        assert_eq!(
            kadakas_update::welcome::done_version(&d.launcher),
            WELCOME_VERSION
        );
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
        // It reads back as it was written.
        assert_eq!(current(&d, &langs(&["en-US"]), true), c);
        cleanup(&d);
    }

    #[test]
    fn the_language_of_the_system_is_stored_as_system() {
        let d = dirs("stores-system");
        let c = detect(&sys(&["en-GB"]), true);
        assert!(apply(&d, &c, None).is_empty());
        let prefs = d.prefs.clone().unwrap();
        assert_eq!(json(&prefs.join("prefs.json"))["language"], "system");
        assert!(json(&prefs.join("modeler.json"))["language"].is_null());
        assert_eq!(
            json(&Settings::path(&d.launcher))["flavor"],
            "kadakas-modeler"
        );
        assert_eq!(current(&d, &langs(&["en-GB"]), true).language, None);
        cleanup(&d);
    }

    #[test]
    fn only_the_choices_that_the_person_changed_are_written() {
        let d = dirs("stores-diff");
        // The programs' own state: a modeler user with a View > Trackpad choice, a language of
        // the modeler, comma decimals from the language, no units key (millimeters).
        let prefs = d.prefs.clone().unwrap();
        std::fs::create_dir_all(&prefs).unwrap();
        std::fs::create_dir_all(&d.launcher).unwrap();
        std::fs::write(
            Settings::path(&d.launcher),
            r#"{"flavor":"kadakas-modeler"}"#,
        )
        .unwrap();
        std::fs::write(
            prefs.join("prefs.json"),
            r#"{"language":"system","cues":false}"#,
        )
        .unwrap();
        std::fs::write(
            prefs.join("modeler.json"),
            r#"{"language":"et","theme":"dark","navigation":"trackpad","tray":false}"#,
        )
        .unwrap();
        let languages = langs(&["en-US"]);
        let before = current(&d, &languages, true);
        assert_eq!(before.mode, Mode::Modeler);
        assert_eq!(
            before.language.as_deref(),
            Some("et"),
            "the modeler's own language"
        );
        assert_eq!(before.theme, Theme::Dark);
        assert_eq!(
            before.navigation,
            Navigation::Trackpad,
            "the modeler's own override"
        );
        assert_eq!(
            before.units,
            UnitSystem::Metric,
            "no key: millimeters, not the region"
        );
        assert_eq!(
            before.number,
            NumberStyle::SpaceComma,
            "follows the language et"
        );
        // The person changes the updates mode only.
        let mut shown = before.clone();
        shown.updates = UpdateMode::AutoDownload;
        assert!(apply(&d, &shown, Some(&before)).is_empty());
        let m = json(&prefs.join("modeler.json"));
        assert_eq!(m["navigation"], "trackpad", "not removed");
        assert_eq!(
            (m["language"].as_str(), m["theme"].as_str()),
            (Some("et"), Some("dark"))
        );
        let p = json(&prefs.join("prefs.json"));
        assert!(
            p.get("units").is_none(),
            "units untouched, number format still follows"
        );
        assert_eq!(p["cues"], false);
        assert_eq!(json(&Settings::path(&d.launcher))["mode"], "auto-download");
        // The person changes the navigation: the modeler's override goes, the shared key is set.
        let mut shown2 = shown.clone();
        shown2.navigation = Navigation::Mouse;
        assert!(apply(&d, &shown2, Some(&shown)).is_empty());
        assert!(
            json(&prefs.join("modeler.json"))
                .get("navigation")
                .is_none()
        );
        assert_eq!(json(&prefs.join("prefs.json"))["navigation"], "mouse");
        // The person changes the number format: only decimal and grouping are written.
        let mut shown3 = shown2.clone();
        shown3.number = NumberStyle::CommaDot;
        assert!(apply(&d, &shown3, Some(&shown2)).is_empty());
        let p = json(&prefs.join("prefs.json"));
        assert_eq!(
            (
                p["units"]["decimal"].as_str(),
                p["units"]["grouping"].as_str()
            ),
            (Some("point"), Some("comma"))
        );
        assert!(p["units"].get("length").is_none());
        cleanup(&d);
    }

    #[test]
    fn the_dialog_keeps_the_keys_of_the_programs_and_of_the_future() {
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
            r#"{"tray":false,"import_soften":{"x":1}}"#,
        )
        .unwrap();
        std::fs::write(
            Settings::path(&d.launcher),
            r#"{"channel":"beta","keep_older":4,"future_key":"x"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(d.feedback.as_ref().unwrap()).unwrap();
        std::fs::write(
            d.feedback.as_ref().unwrap().join("settings.json"),
            r#"{"installation":"abc","omit_installation":true}"#,
        )
        .unwrap();
        let before = current(&d, &langs(&["en-US"]), true);
        let mut shown = before.clone();
        shown.units = UnitSystem::Imperial;
        shown.language = Some("et".into());
        shown.theme = Theme::Light;
        shown.crash = CrashPolicy::Always;
        shown.updates = UpdateMode::NotifyOnly;
        assert!(apply(&d, &shown, Some(&before)).is_empty());
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
    fn use_defaults_writes_missing_keys_and_never_replaces_a_value() {
        let d = dirs("stores-missing");
        let prefs = d.prefs.clone().unwrap();
        std::fs::create_dir_all(&prefs).unwrap();
        std::fs::create_dir_all(&d.launcher).unwrap();
        std::fs::write(
            prefs.join("prefs.json"),
            r#"{"language":"et","theme":"dark","units":{"length":"mm"}}"#,
        )
        .unwrap();
        std::fs::write(Settings::path(&d.launcher), r#"{"flavor":"kadakas-app"}"#).unwrap();
        std::fs::create_dir_all(d.feedback.as_ref().unwrap()).unwrap();
        std::fs::write(
            d.feedback.as_ref().unwrap().join("settings.json"),
            r#"{"crash_policy":"never"}"#,
        )
        .unwrap();
        let mut detected = detect(&sys(&["en-US"]), true);
        detected.crash = CrashPolicy::Always;
        assert!(apply_missing(&d, &detected).is_empty());
        let p = json(&prefs.join("prefs.json"));
        assert_eq!(p["language"], "et", "kept");
        assert_eq!(p["theme"], "dark", "kept");
        assert_eq!(p["units"]["length"], "mm", "kept");
        assert_eq!(
            p["units"]["area"], "ft2",
            "missing, so the default is written"
        );
        assert_eq!(p["navigation"], "mouse");
        assert_eq!(
            json(&Settings::path(&d.launcher))["flavor"],
            "kadakas-app",
            "kept"
        );
        assert_eq!(json(&Settings::path(&d.launcher))["mode"], "auto-update");
        let f = json(&d.feedback.clone().unwrap().join("settings.json"));
        assert_eq!(f["crash_policy"], "never", "kept");
        assert_eq!(
            kadakas_update::welcome::done_version(&d.launcher),
            WELCOME_VERSION
        );
        cleanup(&d);
    }

    #[test]
    fn traces_are_the_install_folder_and_every_settings_file() {
        let d = dirs("stores-traces");
        assert!(!has_traces(&d));
        let prefs = d.prefs.clone().unwrap();
        std::fs::create_dir_all(&prefs).unwrap();
        std::fs::write(prefs.join("modeler.json"), "{}").unwrap();
        assert!(has_traces(&d));
        std::fs::remove_file(prefs.join("modeler.json")).unwrap();
        assert!(!has_traces(&d));
        let fb = d.feedback.clone().unwrap();
        std::fs::create_dir_all(&fb).unwrap();
        std::fs::write(fb.join("settings.json"), "{}").unwrap();
        assert!(has_traces(&d));
        std::fs::remove_file(fb.join("settings.json")).unwrap();
        std::fs::create_dir_all(d.launcher.join("runs")).unwrap();
        assert!(has_traces(&d));
        cleanup(&d);
    }

    #[test]
    fn a_damaged_file_is_replaced_and_the_old_text_is_kept_aside() {
        let d = dirs("stores-damaged");
        let prefs = d.prefs.clone().unwrap();
        std::fs::create_dir_all(&prefs).unwrap();
        std::fs::write(prefs.join("prefs.json"), "{ nope").unwrap();
        assert!(apply(&d, &detect(&sys(&[]), true), None).is_empty());
        assert_eq!(json(&prefs.join("prefs.json"))["theme"], "system");
        assert_eq!(
            std::fs::read_to_string(prefs.join("prefs.json.bak")).unwrap(),
            "{ nope"
        );
        // A damaged launcher.json too.
        std::fs::write(Settings::path(&d.launcher), "[[").unwrap();
        assert!(apply(&d, &detect(&sys(&[]), true), None).is_empty());
        assert_eq!(
            std::fs::read_to_string(d.launcher.join("launcher.json.bak")).unwrap(),
            "[["
        );
        cleanup(&d);
    }

    #[test]
    fn a_read_error_stops_the_write_and_is_reported() {
        let d = dirs("stores-eio");
        let prefs = d.prefs.clone().unwrap();
        // A folder where prefs.json should be: not "not found", so no replacement.
        std::fs::create_dir_all(prefs.join("prefs.json")).unwrap();
        let problems = apply(&d, &detect(&sys(&[]), true), None);
        assert!(
            problems.iter().any(|p| p.starts_with("prefs.json")),
            "{problems:?}"
        );
        assert!(prefs.join("prefs.json").is_dir());
        // The other files were still written.
        assert!(Settings::path(&d.launcher).exists());
        assert!(prefs.join("modeler.json").exists());
        cleanup(&d);
    }

    #[test]
    fn a_damaged_feedback_file_is_kept_aside_and_a_read_error_stops_the_write() {
        let d = dirs("stores-feedback");
        let fb = d.feedback.clone().unwrap();
        std::fs::create_dir_all(&fb).unwrap();
        std::fs::write(fb.join("settings.json"), "{ nope").unwrap();
        let mut c = detect(&sys(&[]), true);
        c.crash = CrashPolicy::Always;
        assert!(apply(&d, &c, None).is_empty());
        assert_eq!(
            std::fs::read_to_string(fb.join("settings.json.bak")).unwrap(),
            "{ nope"
        );
        assert_eq!(json(&fb.join("settings.json"))["crash_policy"], "always");
        // A folder in place of the file: reported, nothing replaced.
        let d2 = dirs("stores-feedback-eio");
        let fb2 = d2.feedback.clone().unwrap();
        std::fs::create_dir_all(fb2.join("settings.json")).unwrap();
        let problems = apply(&d2, &c, None);
        assert!(
            problems.iter().any(|p| p.contains("crash reports")),
            "{problems:?}"
        );
        assert!(fb2.join("settings.json").is_dir());
        cleanup(&d);
        cleanup(&d2);
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
        let problems = apply(&broken, &detect(&sys(&[]), true), None);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].starts_with("prefs.json"));
        assert_eq!(
            kadakas_update::welcome::done_version(&d.launcher),
            WELCOME_VERSION
        );
        // A launcher folder that cannot be made.
        let bad_launcher = Dirs {
            launcher: blocker.join("l"),
            ..d.clone()
        };
        assert!(!mark_done(&bad_launcher).is_empty());
        assert!(
            apply(&bad_launcher, &detect(&sys(&[]), true), None)[0].starts_with("launcher.json")
        );
        cleanup(&d);
    }

    #[test]
    fn what_the_programs_use_is_what_the_dialog_shows() {
        let d = dirs("stores-current");
        // Nothing stored: the defaults of the programs, not of the region.
        let c = current(&d, &langs(&["en-US"]), true);
        assert_eq!(c.units, UnitSystem::Metric);
        assert_eq!(c.number, NumberStyle::CommaDot);
        assert_eq!((c.mode, c.crash), (Mode::Landscape, CrashPolicy::Ask));
        let c = current(&d, &langs(&["et-EE"]), true);
        assert_eq!(c.number, NumberStyle::SpaceComma);
        // Bad values give the defaults of the programs.
        std::fs::create_dir_all(d.prefs.as_ref().unwrap()).unwrap();
        std::fs::write(
            d.prefs.as_ref().unwrap().join("prefs.json"),
            r#"{"language":"et","theme":"neon","navigation":3,"units":{"length":"banana","decimal":"point"}}"#,
        )
        .unwrap();
        let c = current(&d, &langs(&["en-US"]), true);
        assert_eq!(c.language.as_deref(), Some("et"));
        assert_eq!(
            (c.theme, c.navigation, c.units),
            (Theme::System, Navigation::Mouse, UnitSystem::Metric)
        );
        // decimal point + the Estonian space grouping is not offered: the nearest style.
        assert_eq!(c.number, NumberStyle::CommaDot);
        cleanup(&d);
    }

    #[test]
    fn the_folders_follow_the_arguments() {
        let d = Dirs::new(PathBuf::from("/l"), Some("/p".into()), Some("/f".into()));
        assert_eq!(
            (d.prefs, d.feedback),
            (Some(PathBuf::from("/p")), Some(PathBuf::from("/f")))
        );
        let d = Dirs::new(PathBuf::from("/l"), None, Some(OsString::new()));
        assert!(d.prefs.is_none());
        let _ = Dirs::from_env(PathBuf::from("/l"));
    }
}
