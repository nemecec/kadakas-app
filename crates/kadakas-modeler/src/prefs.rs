//! Settings of the modeler: the theme, the language and whether the tray shows. They live in
//! `modeler.json` in the per-user config folder (`KADAKAS_PREFS_DIR` names another folder), next
//! to the `prefs.json` of `kadakas-app`.
//!
//! The units and formats are the ones of the app: the modeler reads the `units` entry of
//! `prefs.json` and never writes that file, so the person sets the units once. A file that cannot
//! be read is ignored and the defaults apply.

use std::path::{Path, PathBuf};

use kadakas_l10n::tr;
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
    pub theme: ThemeChoice,
    /// The tag of a fixed language (one of `kadakas_l10n::languages()`); `None` follows the system.
    pub language: Option<String>,
    /// The tray on the right shows.
    pub tray: bool,
    /// Units and formats of the app (`prefs.json`). Never written here.
    #[serde(skip)]
    pub units: kadakas_units::Settings,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            theme: ThemeChoice::System,
            language: None,
            tray: true,
            units: kadakas_units::Settings::default(),
        }
    }
}

impl Prefs {
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
        kadakas_units::set_current(self.units.resolve(tag));
        tag
    }
}

/// The languages that the system asks for, the first one is the most wanted.
pub fn system_languages() -> Vec<String> {
    sys_locale::get_locales().collect()
}

/// The folder of the files. `None` if there is no home folder.
pub fn dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("KADAKAS_PREFS_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(d));
    }
    kadakas_sys::dirs::config_dir()
}

/// The settings in `dir`. The second value is a warning for the status line, when a file was
/// ignored.
pub fn load_from(dir: &Path) -> (Prefs, Option<String>) {
    let path = dir.join("modeler.json");
    let (mut prefs, warning) = match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Prefs>(&text) {
            Ok(p) => (p, None),
            Err(e) => (
                Prefs::default(),
                Some(tr!(
                    "modeler-prefs-ignored",
                    path = path.display(),
                    error = e
                )),
            ),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Prefs::default(), None),
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
    (prefs, warning)
}

/// The units that the person set in the app. A missing or damaged file gives the defaults.
fn app_units(dir: &Path) -> kadakas_units::Settings {
    std::fs::read_to_string(dir.join("prefs.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("units").cloned())
        .and_then(|u| serde_json::from_value(u).ok())
        .unwrap_or_default()
}

pub fn load() -> (Prefs, Option<String>) {
    match dir() {
        Some(d) => load_from(&d),
        None => (Prefs::default(), None),
    }
}

/// Writes the settings whole into a temporary file first, so a crash cannot leave half a file.
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
