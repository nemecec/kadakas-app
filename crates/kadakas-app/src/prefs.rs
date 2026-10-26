//! Per-user preferences of the window: theme, snapping set, cues and the dock layouts.
//!
//! They live in `prefs.json` and `layouts.json` in the per-user config folder, next to
//! `keymap.json` (see `keymap::user_file`). `KADAKAS_PREFS_DIR` sets another folder. A file that
//! cannot be read is ignored and the defaults apply; a write that fails shows in the status line.

use kadakas_l10n::tr;
use std::path::PathBuf;

use kadakas_core::snapping::Settings;
use serde::{Deserialize, Serialize};

use crate::icon_size::IconSize;
use crate::theme::ThemeChoice;

/// Reads the icon size; any value that is not a known size gives the default.
fn icon_size_or_default<'de, D: serde::Deserializer<'de>>(d: D) -> Result<IconSize, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

/// Reads the navigation style; any value that is not known gives the mouse.
fn nav_style_or_default<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<kadakas_ui_core::nav3d::NavStyle, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(kadakas_ui_core::nav3d::NavStyle::from_value(&v))
}

/// Settings that the user changes in the window.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    #[serde(deserialize_with = "kadakas_units::lenient")]
    pub theme: ThemeChoice,
    pub snap: Settings,
    /// Show the SmartCursor cue label at the pointer.
    pub cues: bool,
    /// Small, Medium or Large icons (palettes 16/24/32 pt, bars 16/20/24 pt, menus 16 pt).
    /// An unknown value in the file gives Medium; the rest of the file still counts.
    #[serde(deserialize_with = "icon_size_or_default")]
    pub icon_size: IconSize,
    /// The language of the program: the language of the system, or a fixed one.
    pub language: crate::locale::LanguageChoice,
    /// Units and formats. What is not chosen follows the language.
    pub units: kadakas_units::Settings,
    /// The most named versions in a file (default: the most a reader reads, 1000). A save over it
    /// is stopped and the person is asked which versions to delete (REV-01). No version is
    /// dropped by itself.
    pub max_versions: u32,
    /// The most size of the versions in a file, in MiB (default 256, the highest safe value). The
    /// same.
    pub max_version_mib: u32,
    /// Animate the view when an agent moves it (AGR-02). Off: the view jumps.
    pub animate_agent_view: bool,
    /// The answer to "Soften edges under an angle?" in the import of meshes (MOD-12): off, 20
    /// degrees, until the person changes it. The next import starts with the last answer.
    pub import_soften: kadakas_ui_core::mesh_import::SoftenPref,
    /// What the wheel and the two-finger scroll do in the 3D view: mouse or trackpad (PLAT-26).
    #[serde(deserialize_with = "nav_style_or_default")]
    pub navigation: kadakas_ui_core::nav3d::NavStyle,
}

/// The default of `Prefs::max_versions`: the most a reader reads (no practical limit).
pub const DEFAULT_MAX_VERSIONS: u32 = kadakas_api::VERSIONS_MAX as u32;
/// The default of `Prefs::max_version_mib`: the highest safe write limit.
pub const DEFAULT_MAX_VERSION_MIB: u32 = (kadakas_api::VERSION_BYTES_MAX / (1024 * 1024)) as u32;
/// The defaults of the versions in the earlier program. A stored value that equals one was never
/// chosen by the person, so it moves to the new default when the prefs load.
const OLD_MAX_VERSIONS: u32 = 100;
const OLD_MAX_VERSION_MIB: u32 = 64;

impl Prefs {
    /// Moves the stored values that equal the old defaults to the new defaults.
    fn migrate(mut self) -> Prefs {
        if self.max_versions == OLD_MAX_VERSIONS {
            self.max_versions = DEFAULT_MAX_VERSIONS;
        }
        if self.max_version_mib == OLD_MAX_VERSION_MIB {
            self.max_version_mib = DEFAULT_MAX_VERSION_MIB;
        }
        self
    }
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            theme: ThemeChoice::System,
            snap: Settings::default(),
            cues: true,
            icon_size: IconSize::default(),
            language: crate::locale::LanguageChoice::SYSTEM,
            units: kadakas_units::Settings::default(),
            max_versions: DEFAULT_MAX_VERSIONS,
            max_version_mib: DEFAULT_MAX_VERSION_MIB,
            animate_agent_view: true,
            import_soften: kadakas_ui_core::mesh_import::SoftenPref::default(),
            navigation: kadakas_ui_core::nav3d::NavStyle::default(),
        }
    }
}

/// The folder of the files. `None` if there is no home folder. `KADAKAS_PREFS_DIR` or the folder of
/// `KADAKAS_KEYMAP` replace the config folder (`kadakas_sys::dirs::prefs_dir`).
pub fn dir() -> Option<PathBuf> {
    kadakas_sys::dirs::prefs_dir()
}

/// The prefs in `dir`, or the defaults if the file is missing or damaged. The second value is a
/// warning for the status line.
pub fn load_from(dir: &std::path::Path) -> (Prefs, Option<String>) {
    let path = dir.join("prefs.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Prefs>(&text) {
            Ok(p) if p.snap.problem().is_none() => (p.migrate(), None),
            Ok(_) => (
                Prefs::default(),
                Some(tr!(
                    "prefs-has-bad-snapping-numbers-and-is-ignored",
                    path = path.display()
                )),
            ),
            Err(e) => (
                Prefs::default(),
                Some(tr!("prefs-is-ignored", path = path.display(), e = e)),
            ),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Prefs::default(), None),
        Err(e) => (
            Prefs::default(),
            Some(tr!("prefs-is-ignored", path = path.display(), e = e)),
        ),
    }
}

/// Writes a file of the prefs folder. The file is written whole in a temporary file first, so a
/// crash cannot leave half a file.
pub fn write_file(dir: &std::path::Path, name: &str, text: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| tr!("prefs-cannot-make", path = dir.display(), e = e))?;
    let path = dir.join(name);
    let tmp = dir.join(format!("{name}.tmp"));
    std::fs::write(&tmp, text)
        .map_err(|e| tr!("prefs-cannot-write", path = tmp.display(), e = e))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| tr!("prefs-cannot-write", path = path.display(), e = e))
}

/// Writes the prefs whole. The program itself writes only the keys it changed
/// (`save_changes_to`); this is for tests.
#[cfg(test)]
pub fn save_to(dir: &std::path::Path, prefs: &Prefs) -> Result<(), String> {
    let text = serde_json::to_string_pretty(prefs).map_err(|e| e.to_string())?;
    write_file(dir, "prefs.json", &text)
}

/// Writes the keys that differ between `saved` (the prefs as they were read or written last) and
/// `now`, into the file as it is on disk. A key that this program did not change (the welcome
/// dialog changed it in the meantime) and a key that it does not know stay as they are.
pub fn save_changes_to(dir: &std::path::Path, saved: &Prefs, now: &Prefs) -> Result<(), String> {
    let value = |p: &Prefs| serde_json::to_value(p).unwrap_or(serde_json::Value::Null);
    let changes = kadakas_sys::jsonfile::diff(&value(saved), &value(now));
    let path = dir.join("prefs.json");
    kadakas_sys::jsonfile::write_changes(&path, &changes)
        .map_err(|e| tr!("prefs-cannot-write", path = path.display(), e = e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kadakas-prefs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_old_version_defaults_move_to_the_new_ones_and_chosen_values_stay() {
        let d = temp("old-version-defaults");
        write_file(
            &d,
            "prefs.json",
            r#"{"max_versions": 100, "max_version_mib": 64}"#,
        )
        .unwrap();
        let (p, _) = load_from(&d);
        assert_eq!((p.max_versions, p.max_version_mib), (1000, 256));
        write_file(
            &d,
            "prefs.json",
            r#"{"max_versions": 7, "max_version_mib": 9}"#,
        )
        .unwrap();
        let (p, _) = load_from(&d);
        assert_eq!((p.max_versions, p.max_version_mib), (7, 9));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn values_from_a_newer_version_give_defaults_for_those_fields_only() {
        let d = temp("newer-values");
        write_file(
            &d,
            "prefs.json",
            r#"{"theme":"high_contrast","cues":false,"units":{"grouping":"thin","length":"ft_in","decimal":"comma"},"language":"et"}"#,
        )
        .unwrap();
        let (p, w) = load_from(&d);
        assert_eq!(w, None, "the file is not ignored");
        assert_eq!(p.theme, ThemeChoice::System);
        assert!(!p.cues);
        assert_eq!(p.units.grouping, None);
        assert_eq!(p.units.length, kadakas_units::LengthUnit::FtIn);
        assert_eq!(p.units.decimal, Some(kadakas_units::Decimal::Comma));
        assert_eq!(p.language.tag(), Some("et"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_save_writes_only_what_changed_and_keeps_what_another_program_wrote() {
        use kadakas_ui_core::nav3d::NavStyle;
        let d = temp("save-changes");
        let saved = Prefs::default();
        save_to(&d, &saved).unwrap();
        // The welcome dialog stores a theme, a language and an unknown key.
        let mut text: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(d.join("prefs.json")).unwrap()).unwrap();
        text["theme"] = "dark".into();
        text["language"] = "et".into();
        text["units"]["length"] = "ft_in".into();
        text["from_the_future"] = serde_json::json!({"a": 1});
        std::fs::write(d.join("prefs.json"), text.to_string()).unwrap();
        // This program still holds the old copy; the person toggles a cue and picks a style.
        let now = Prefs {
            cues: !saved.cues,
            navigation: NavStyle::Trackpad,
            ..saved
        };
        save_changes_to(&d, &saved, &now).unwrap();
        let (back, warning) = load_from(&d);
        assert_eq!(warning, None);
        assert_eq!(
            back.theme,
            ThemeChoice::Dark,
            "the welcome dialog's theme stays"
        );
        assert_eq!(back.language.tag(), Some("et"));
        assert_eq!(back.units.length, kadakas_units::LengthUnit::FtIn);
        assert_eq!(
            (back.cues, back.navigation),
            (!saved.cues, NavStyle::Trackpad)
        );
        let text: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(d.join("prefs.json")).unwrap()).unwrap();
        assert_eq!(text["from_the_future"]["a"], 1, "unknown keys survive");
        // No change, no write.
        let before = std::fs::metadata(d.join("prefs.json"))
            .unwrap()
            .modified()
            .unwrap();
        save_changes_to(&d, &now, &now).unwrap();
        assert_eq!(
            std::fs::metadata(d.join("prefs.json"))
                .unwrap()
                .modified()
                .unwrap(),
            before
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_navigation_style_is_stored_and_an_unknown_value_gives_the_mouse() {
        use kadakas_ui_core::nav3d::NavStyle;
        let p: Prefs =
            serde_json::from_str(r#"{"navigation": "trackpad", "cues": false}"#).unwrap();
        assert_eq!(p.navigation, NavStyle::Trackpad);
        assert!(!p.cues);
        for bad in ["\"pen\"", "3", "null"] {
            let p: Prefs = serde_json::from_str(&format!(r#"{{"navigation": {bad}}}"#)).unwrap();
            assert_eq!(p.navigation, NavStyle::Mouse);
        }
        assert_eq!(Prefs::default().navigation, NavStyle::Mouse);
        let text = serde_json::to_string(&Prefs {
            navigation: NavStyle::Trackpad,
            ..Prefs::default()
        })
        .unwrap();
        assert!(text.contains(r#""navigation":"trackpad""#), "{text}");
    }

    #[test]
    fn an_unknown_icon_size_does_not_reset_the_file() {
        for bad in ["\"huge\"", "3", "null", "{}"] {
            let text = format!(r#"{{"cues": false, "icon_size": {bad}}}"#);
            let p: Prefs = serde_json::from_str(&text).unwrap();
            assert_eq!(p.icon_size, IconSize::Medium);
            assert!(!p.cues);
        }
    }

    #[test]
    fn an_old_prefs_file_gets_the_medium_icon_size() {
        let p: Prefs = serde_json::from_str(r#"{"cues": false}"#).unwrap();
        assert_eq!(p.icon_size, IconSize::Medium);
    }

    #[test]
    fn prefs_round_trip_and_defaults() {
        let d = temp("rt");
        assert_eq!(load_from(&d), (Prefs::default(), None));
        let mut p = Prefs {
            theme: ThemeChoice::Dark,
            cues: false,
            icon_size: IconSize::Large,
            ..Prefs::default()
        };
        p.snap.set.angle = true;
        p.snap.grey = true;
        save_to(&d, &p).unwrap();
        assert_eq!(load_from(&d), (p, None));
        assert!(!d.join("prefs.json.tmp").exists());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn damaged_files_are_ignored_with_a_warning() {
        let d = temp("bad");
        write_file(&d, "prefs.json", "{not json").unwrap();
        let (p, w) = load_from(&d);
        assert_eq!(p, Prefs::default());
        assert!(w.unwrap().contains("ignored"));
        write_file(&d, "prefs.json", r#"{"snap": {"angle_step": 0}}"#).unwrap();
        let (p, w) = load_from(&d);
        assert_eq!(p, Prefs::default());
        assert!(w.unwrap().contains("bad snapping numbers"));
        // Missing keys use the defaults.
        write_file(&d, "prefs.json", r#"{"theme": "light"}"#).unwrap();
        let (p, w) = load_from(&d);
        assert_eq!((p.theme, p.cues, w), (ThemeChoice::Light, true, None));
        let _ = std::fs::remove_dir_all(d);
    }
}
