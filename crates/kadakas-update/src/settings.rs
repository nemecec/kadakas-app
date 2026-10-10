//! The launcher settings: update channel, UI flavor and profile. They live in `launcher.json` in
//! the install folder. The file has no secrets.

use crate::manifest::Channel;
use kadakas_sys::fsio::{self, Access};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The id of the main app flavor.
pub const MAIN_FLAVOR: &str = "kadakas-app";

/// A profile or flavor name: 1 to 40 characters, letters, digits, `-` and `_`.
/// The same rule is used in the app for `--profile`.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub channel: Channel,
    /// The id of the UI flavor to start.
    pub flavor: String,
    /// The workspace profile that the app gets. Empty: none.
    pub profile: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            channel: Channel::Stable,
            flavor: MAIN_FLAVOR.to_string(),
            profile: String::new(),
        }
    }
}

impl Settings {
    pub fn path(root: &Path) -> PathBuf {
        root.join("launcher.json")
    }

    /// Reads the settings. A missing or damaged file gives the defaults. Bad names are dropped.
    pub fn load(root: &Path) -> Settings {
        let mut s: Settings = std::fs::read(Self::path(root))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if !valid_name(&s.flavor) {
            s.flavor = MAIN_FLAVOR.to_string();
        }
        if !s.profile.is_empty() && !valid_name(&s.profile) {
            s.profile.clear();
        }
        s
    }

    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(root)?;
        let text = serde_json::to_vec_pretty(self).expect("settings serialize");
        fsio::write_durable(&Self::path(root), &text, Access::KeepExisting, ".tmp")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::tmp;

    #[test]
    fn names() {
        for ok in ["a", "acad-like", "Vw_2026", &"x".repeat(40)] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in ["", "a b", "a/b", "..", "é", &"x".repeat(41)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn save_and_load() {
        let root = tmp("settings");
        assert_eq!(Settings::load(&root), Settings::default());
        let s = Settings {
            channel: Channel::Beta,
            flavor: "sketch".into(),
            profile: "acad".into(),
        };
        s.save(&root.join("sub")).unwrap();
        assert_eq!(Settings::load(&root.join("sub")), s);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn damaged_or_hostile_files_give_safe_values() {
        let root = tmp("settings-bad");
        std::fs::write(Settings::path(&root), b"{ nope").unwrap();
        assert_eq!(Settings::load(&root), Settings::default());
        std::fs::write(
            Settings::path(&root),
            br#"{"channel":"beta","flavor":"../../bin/sh","profile":"a b"}"#,
        )
        .unwrap();
        let s = Settings::load(&root);
        assert_eq!(s.channel, Channel::Beta);
        assert_eq!(s.flavor, MAIN_FLAVOR);
        assert_eq!(s.profile, "");
        // Missing fields take defaults.
        std::fs::write(Settings::path(&root), br#"{"profile":"p1"}"#).unwrap();
        let s = Settings::load(&root);
        assert_eq!((s.channel, s.profile.as_str()), (Channel::Stable, "p1"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
