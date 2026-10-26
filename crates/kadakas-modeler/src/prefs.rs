//! Settings of the modeler: the theme, the language and whether the tray shows. They live in
//! `modeler.json` in the per-user config folder (`KADAKAS_PREFS_DIR` names another folder), next
//! to the `prefs.json` of `kadakas-app`.
//!
//! The units and formats are the ones of the app: the modeler reads the `units` entry of
//! `prefs.json` and never writes that file, so the person sets the units once. A file that cannot
//! be read is ignored and the defaults apply.

use std::path::{Path, PathBuf};

use kadakas_l10n::tr;
use kadakas_ui_core::nav3d::NavStyle;
use serde::{Deserialize, Serialize};

/// What the person chose for the colors of the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    /// Follow the system.
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];

    pub fn label(self) -> String {
        match self {
            ThemeChoice::System => tr!("theme-system"),
            ThemeChoice::Light => tr!("theme-light"),
            ThemeChoice::Dark => tr!("theme-dark"),
        }
    }

    pub fn preference(self) -> egui::ThemePreference {
        match self {
            ThemeChoice::System => egui::ThemePreference::System,
            ThemeChoice::Light => egui::ThemePreference::Light,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
        }
    }
}

/// The settings of the window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    #[serde(deserialize_with = "kadakas_units::lenient")]
    pub theme: ThemeChoice,
    /// The tag of a fixed language (one of `kadakas_l10n::languages()`); `None` follows the system.
    pub language: Option<String>,
    /// The tray on the right shows.
    pub tray: bool,
    /// The camera moves in a short animation when a scene is shown (MOD-10).
    pub animate_scenes: bool,
    /// The answer to "Soften edges under an angle?" in the import of meshes (MOD-12): off, 20
    /// degrees, until the person changes it. The next import starts with the last answer.
    pub import_soften: kadakas_ui_core::mesh_import::SoftenPref,
    /// Units and formats of the app (`prefs.json`). Never written here.
    #[serde(skip)]
    pub units: kadakas_units::Settings,
    /// What the wheel and the two-finger scroll do (mouse or trackpad). `None`: the choice of the
    /// app in `prefs.json` counts (`app_navigation`), set once in the welcome dialog (PLAT-26).
    #[serde(
        deserialize_with = "nav_or_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub navigation: Option<NavStyle>,
    /// The choice of the app (`prefs.json`). Never written here.
    #[serde(skip)]
    pub app_navigation: NavStyle,
}

/// A navigation style that is not known gives `None`; the rest of the file still counts.
fn nav_or_none<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<NavStyle>, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).ok())
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            theme: ThemeChoice::System,
            language: None,
            tray: true,
            animate_scenes: true,
            import_soften: kadakas_ui_core::mesh_import::SoftenPref::default(),
            units: kadakas_units::Settings::default(),
            navigation: None,
            app_navigation: NavStyle::Mouse,
        }
    }
}

impl Prefs {
    /// The navigation style in force: the modeler's own choice, else the app's, else the mouse.
    pub fn nav_style(&self) -> NavStyle {
        self.navigation.unwrap_or(self.app_navigation)
    }

    /// The language to use when the system asks for `system`: the chosen one, or the best match.
    pub fn language_tag(&self, system: &[String]) -> &'static str {
        match self.language.as_deref().and_then(|t| {
            kadakas_l10n::languages()
                .iter()
                .find(|l| l.tag.eq_ignore_ascii_case(t))
        }) {
            Some(l) => l.tag,
            None => kadakas_l10n::negotiate(system),
        }
    }

    /// Makes the language and the units current for the whole program. Returns the language tag.
    pub fn apply_language(&self, system: &[String]) -> &'static str {
        let tag = self.language_tag(system);
        kadakas_l10n::set_language(tag);
        let units = self.units.resolve(tag);
        kadakas_units::set_current(units);
        kadakas_l10n::set_decimal_symbol(units.number.decimal.symbol());
        tag
    }
}

/// The languages that the system asks for, the first one is the most wanted.
pub fn system_languages() -> Vec<String> {
    sys_locale::get_locales().collect()
}

/// The folder of the files. `None` if there is no home folder. The same as the folder of the
/// app (`kadakas_sys::dirs::prefs_dir`).
pub fn dir() -> Option<PathBuf> {
    kadakas_sys::dirs::prefs_dir()
}

/// The settings in `dir`. The second value is a warning for the status line, when a file was
/// ignored.
pub fn load_from(dir: &Path) -> (Prefs, Option<String>) {
    let path = dir.join("modeler.json");
    // An empty or damaged file gives the last good version (`modeler.json.bak`), with a warning.
    let (mut prefs, warning) = match kadakas_sys::jsonfile::read_for_load(&path) {
        Ok(Some((bytes, from_backup))) => match serde_json::from_slice::<Prefs>(&bytes) {
            Ok(p) => (
                p,
                from_backup.then(|| tr!("modeler-prefs-restored", path = path.display())),
            ),
            Err(e) => (
                Prefs::default(),
                Some(tr!(
                    "modeler-prefs-ignored",
                    path = path.display(),
                    error = e
                )),
            ),
        },
        Ok(None) => (Prefs::default(), None),
        Err(e) => (
            Prefs::default(),
            Some(tr!(
                "modeler-prefs-ignored",
                path = path.display(),
                error = e
            )),
        ),
    };
    prefs.units = app_units(dir);
    prefs.app_navigation = app_navigation(dir);
    (prefs, warning)
}

/// The units that the person set in the app. A missing or damaged file gives the defaults.
fn app_units(dir: &Path) -> kadakas_units::Settings {
    app_prefs_value(dir)
        .and_then(|v| v.get("units").cloned())
        .and_then(|u| serde_json::from_value(u).ok())
        .unwrap_or_default()
}

/// The navigation style that the person set in the app. A missing value gives the mouse.
fn app_navigation(dir: &Path) -> NavStyle {
    app_prefs_value(dir)
        .and_then(|v| v.get("navigation").map(NavStyle::from_value))
        .unwrap_or_default()
}

/// `prefs.json` of the app as JSON (the last good version if the file is damaged).
fn app_prefs_value(dir: &Path) -> Option<serde_json::Value> {
    let (bytes, _) = kadakas_sys::jsonfile::read_for_load(&dir.join("prefs.json"))
        .ok()
        .flatten()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn load() -> (Prefs, Option<String>) {
    match dir() {
        Some(d) => load_from(&d),
        None => (Prefs::default(), None),
    }
}

/// Writes the keys that differ between `saved` (the settings as they were read or written last)
/// and `now`, into `modeler.json` as it is on disk. A key that this program did not change (the
/// welcome dialog stored it in the meantime) and a key that it does not know stay as they are.
pub fn save_changes_to(dir: &Path, saved: &Prefs, now: &Prefs) -> Result<(), String> {
    let value = |p: &Prefs| serde_json::to_value(p).unwrap_or(serde_json::Value::Null);
    let changes = kadakas_sys::jsonfile::diff(&value(saved), &value(now));
    let path = dir.join("modeler.json");
    kadakas_sys::jsonfile::write_changes(&path, &changes).map_err(|e| {
        tr!(
            "modeler-prefs-cannot-write",
            path = path.display(),
            error = e
        )
    })
}

/// True when the language or the units differ: then the language of the program and the units
/// are applied again (they are process wide).
pub fn locale_differs(a: &Prefs, b: &Prefs) -> bool {
    a.language != b.language || a.units != b.units
}

/// The settings after a reload: the files as they are now (`reloaded`) with the changes that this
/// window made and could not save yet (the difference between `saved` and `now`) on top. Without
/// pending changes this is `reloaded`.
pub fn with_pending(reloaded: &Prefs, saved: &Prefs, now: &Prefs) -> Prefs {
    let value = |p: &Prefs| serde_json::to_value(p).unwrap_or(serde_json::Value::Null);
    let changes = kadakas_sys::jsonfile::diff(&value(saved), &value(now));
    if changes.is_empty() {
        return reloaded.clone();
    }
    let mut v = value(reloaded);
    if let serde_json::Value::Object(o) = &mut v {
        kadakas_sys::jsonfile::apply(o, &changes);
    }
    // The fields that are not in the file (the units and the navigation of the app) come from
    // the reloaded settings.
    let mut out: Prefs = serde_json::from_value(v).unwrap_or_else(|_| reloaded.clone());
    out.units = reloaded.units;
    out.app_navigation = reloaded.app_navigation;
    out
}

/// Writes the settings whole into a temporary file first, so a crash cannot leave half a file.
/// The program itself writes only the keys it changed (`save_changes_to`); this is for tests.
#[cfg(test)]
pub fn save_to(dir: &Path, prefs: &Prefs) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| {
        tr!(
            "modeler-prefs-cannot-write",
            path = dir.display(),
            error = e
        )
    })?;
    let text = serde_json::to_string_pretty(prefs).map_err(|e| e.to_string())?;
    let path = dir.join("modeler.json");
    let tmp = dir.join("modeler.json.tmp");
    std::fs::write(&tmp, text).map_err(|e| {
        tr!(
            "modeler-prefs-cannot-write",
            path = tmp.display(),
            error = e
        )
    })?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        tr!(
            "modeler-prefs-cannot-write",
            path = path.display(),
            error = e
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "kadakas-modeler-prefs-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sys(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_defaults_follow_the_system_and_show_the_tray() {
        let p = Prefs::default();
        assert_eq!(
            (p.theme, p.language.clone(), p.tray),
            (ThemeChoice::System, None, true)
        );
        assert_eq!(p.language_tag(&sys(&["et_EE.UTF-8", "en"])), "et");
        assert_eq!(p.language_tag(&sys(&["fi"])), "en-US");
        assert_eq!(p.language_tag(&[]), "en-US");
    }

    #[test]
    fn a_fixed_language_wins_over_the_system_and_an_unknown_one_is_ignored() {
        let p = Prefs {
            language: Some("et".into()),
            ..Prefs::default()
        };
        assert_eq!(p.language_tag(&sys(&["en-US"])), "et");
        let p = Prefs {
            language: Some("xx".into()),
            ..Prefs::default()
        };
        assert_eq!(
            p.language_tag(&sys(&["et"])),
            "et",
            "falls back to the system"
        );
    }

    #[test]
    fn only_a_changed_language_or_units_are_applied_again_after_a_reload() {
        let a = Prefs::default();
        assert!(!locale_differs(&a, &a.clone()));
        let b = Prefs {
            language: Some("et".into()),
            ..a.clone()
        };
        assert!(locale_differs(&a, &b));
        let mut c = a.clone();
        c.units.decimal = Some(kadakas_units::Decimal::Comma);
        assert!(locale_differs(&a, &c));
        // Theme, tray and the like are not part of the locale.
        let d = Prefs {
            tray: false,
            theme: ThemeChoice::Dark,
            ..a.clone()
        };
        assert!(!locale_differs(&a, &d));
    }

    #[test]
    fn a_damaged_or_empty_file_gives_the_last_good_version_with_a_warning() {
        let d = temp("restore");
        let good = Prefs {
            tray: false,
            ..Prefs::default()
        };
        save_to(&d, &good).unwrap();
        save_changes_to(
            &d,
            &good,
            &Prefs {
                animate_scenes: false,
                ..good.clone()
            },
        )
        .unwrap();
        for bad in ["", "{ nope"] {
            std::fs::write(d.join("modeler.json"), bad).unwrap();
            let (p, warning) = load_from(&d);
            assert!(!p.tray, "{bad:?}: the settings come back");
            assert!(warning.unwrap().contains("modeler.json"));
        }
        // The units of the app come from its last good file too.
        std::fs::write(d.join("prefs.json"), r#"{"navigation":"trackpad"}"#).unwrap();
        std::fs::write(d.join("prefs.json.bak"), r#"{"navigation":"trackpad"}"#).unwrap();
        std::fs::write(d.join("prefs.json"), "").unwrap();
        let (p, _) = load_from(&d);
        assert_eq!(p.app_navigation, NavStyle::Trackpad);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_theme_from_a_newer_version_gives_the_default_and_the_rest_of_the_file_stays() {
        let d = temp("newer-theme");
        std::fs::write(
            d.join("modeler.json"),
            r#"{"theme":"high_contrast","tray":false,"language":"et"}"#,
        )
        .unwrap();
        let (p, w) = load_from(&d);
        assert_eq!(w, None, "the file is not ignored");
        assert_eq!(
            (p.theme, p.tray, p.language.as_deref()),
            (ThemeChoice::System, false, Some("et"))
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_save_writes_only_what_changed_and_keeps_what_another_program_wrote() {
        let d = temp("save-changes");
        let saved = Prefs::default();
        save_to(&d, &saved).unwrap();
        let mut text: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(d.join("modeler.json")).unwrap())
                .unwrap();
        text["theme"] = "dark".into();
        text["language"] = "et".into();
        text["from_the_future"] = serde_json::json!([1, 2]);
        std::fs::write(d.join("modeler.json"), text.to_string()).unwrap();
        let now = Prefs {
            tray: false,
            navigation: Some(NavStyle::Trackpad),
            ..saved.clone()
        };
        save_changes_to(&d, &saved, &now).unwrap();
        let (back, warning) = load_from(&d);
        assert_eq!(warning, None);
        assert_eq!(back.theme, ThemeChoice::Dark);
        assert_eq!(back.language.as_deref(), Some("et"));
        assert!(!back.tray);
        assert_eq!(back.navigation, Some(NavStyle::Trackpad));
        let text: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(d.join("modeler.json")).unwrap())
                .unwrap();
        assert_eq!(text["from_the_future"][1], 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_navigation_style_comes_from_the_app_until_the_modeler_has_its_own() {
        let d = temp("nav");
        std::fs::write(d.join("prefs.json"), r#"{"navigation": "trackpad"}"#).unwrap();
        let (p, w) = load_from(&d);
        assert_eq!(
            (w, p.navigation, p.nav_style()),
            (None, None, NavStyle::Trackpad)
        );
        std::fs::write(d.join("modeler.json"), r#"{"navigation": "mouse"}"#).unwrap();
        let (p, _) = load_from(&d);
        assert_eq!(
            (p.navigation, p.nav_style()),
            (Some(NavStyle::Mouse), NavStyle::Mouse)
        );
        // An unknown value in either file gives the default and keeps the rest.
        std::fs::write(
            d.join("modeler.json"),
            r#"{"navigation": "pen", "tray": false}"#,
        )
        .unwrap();
        std::fs::write(d.join("prefs.json"), r#"{"navigation": 3}"#).unwrap();
        let (p, w) = load_from(&d);
        assert_eq!((w, p.tray, p.nav_style()), (None, false, NavStyle::Mouse));
        // A choice of the modeler is written; the default is not.
        let own = Prefs {
            navigation: Some(NavStyle::Trackpad),
            ..Prefs::default()
        };
        save_to(&d, &own).unwrap();
        assert!(
            std::fs::read_to_string(d.join("modeler.json"))
                .unwrap()
                .contains("trackpad")
        );
        save_to(&d, &Prefs::default()).unwrap();
        assert!(
            !std::fs::read_to_string(d.join("modeler.json"))
                .unwrap()
                .contains("navigation")
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_settings_survive_a_save_and_a_load() {
        let d = temp("roundtrip");
        let p = Prefs {
            theme: ThemeChoice::Dark,
            language: Some("et".into()),
            tray: false,
            ..Prefs::default()
        };
        save_to(&d, &p).unwrap();
        let (back, warning) = load_from(&d);
        assert_eq!(warning, None);
        assert_eq!(back, p);
        assert!(!d.join("modeler.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_file_gives_the_defaults_and_a_damaged_one_a_warning() {
        let d = temp("damaged");
        let (p, w) = load_from(&d);
        assert_eq!((p, w), (Prefs::default(), None));
        std::fs::write(d.join("modeler.json"), "{ nope").unwrap();
        let (p, w) = load_from(&d);
        assert_eq!(p, Prefs::default());
        assert!(w.unwrap().contains("modeler.json"));
        // A value of a newer version that this one does not know does not break the rest.
        std::fs::write(d.join("modeler.json"), r#"{"tray": false, "future": 1}"#).unwrap();
        let (p, w) = load_from(&d);
        assert_eq!((p.tray, w), (false, None));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_units_come_from_the_app_and_are_not_written_back() {
        let d = temp("units");
        std::fs::write(
            d.join("prefs.json"),
            r#"{"cues": false, "units": {"decimal": "comma"}}"#,
        )
        .unwrap();
        let (p, _) = load_from(&d);
        assert_eq!(p.units.decimal, Some(kadakas_units::Decimal::Comma));
        save_to(&d, &p).unwrap();
        let written = std::fs::read_to_string(d.join("modeler.json")).unwrap();
        assert!(
            !written.contains("comma") && !written.contains("units"),
            "{written}"
        );
        // The prefs file of the app is untouched.
        assert!(
            std::fs::read_to_string(d.join("prefs.json"))
                .unwrap()
                .contains("comma")
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn every_choice_has_a_name_and_a_theme() {
        for c in ThemeChoice::ALL {
            assert!(!c.label().is_empty());
        }
        assert_eq!(
            serde_json::to_string(&ThemeChoice::Dark).unwrap(),
            "\"dark\""
        );
    }
}
