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

/// Settings that the user changes in the window.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
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
        }
    }
}

/// The folder of the files. `None` if there is no home folder.
pub fn dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("KADAKAS_PREFS_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(d));
    }
    crate::keymap::user_file().and_then(|p| p.parent().map(PathBuf::from))
}

/// The prefs in `dir`, or the defaults if the file is missing or damaged. The second value is a
/// warning for the status line.
pub fn load_from(dir: &std::path::Path) -> (Prefs, Option<String>) {
    let path = dir.join("prefs.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Prefs>(&text) {
            Ok(p) if p.snap.problem().is_none() => (p, None),
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

pub fn load() -> (Prefs, Option<String>) {
    match dir() {
        Some(d) => load_from(&d),
        None => (Prefs::default(), None),
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

pub fn save_to(dir: &std::path::Path, prefs: &Prefs) -> Result<(), String> {
    let text = serde_json::to_string_pretty(prefs).map_err(|e| e.to_string())?;
    write_file(dir, "prefs.json", &text)
}

pub fn save(prefs: &Prefs) -> Result<(), String> {
    match dir() {
        Some(d) => save_to(&d, prefs),
        None => Ok(()),
    }
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
