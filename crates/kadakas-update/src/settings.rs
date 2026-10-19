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

/// What the launcher does about new versions (PLAT-16).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateMode {
    /// Download new versions and start the newest one that did not crash at its last start.
    #[default]
    AutoUpdate,
    /// Download new versions, then ask when to switch.
    AutoDownload,
    /// Say that a version exists. Download and switch are actions of the person.
    NotifyOnly,
}

impl UpdateMode {
    pub const ALL: [UpdateMode; 3] = [
        UpdateMode::AutoUpdate,
        UpdateMode::AutoDownload,
        UpdateMode::NotifyOnly,
    ];

    /// Whether the launcher downloads without being asked.
    pub fn downloads_by_itself(self) -> bool {
        self != UpdateMode::NotifyOnly
    }
}

/// The most older versions that the settings accept.
pub const MAX_KEEP_OLDER: u32 = 50;

/// The default of [`Settings::keep_older`]: the newest version and the one before it.
pub const DEFAULT_KEEP_OLDER: u32 = 1;

/// A local update site is a `file://` address (PLAT-17).
pub fn valid_site(site: &str) -> bool {
    site.is_empty() || (site.starts_with("file://") && !site.contains(char::is_whitespace))
}

/// The settings of the launcher and of Preferences > Updates. One schema, one file
/// (`launcher.json` in the install folder): the launcher reads it, and the app writes the update
/// part of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub channel: Channel,
    /// What to do about new versions.
    pub mode: UpdateMode,
    /// How many older versions the store keeps besides the newest one. The bundled app is not
    /// counted. 0 keeps the newest only.
    pub keep_older: u32,
    /// The local update site for the channel `local`: a `file://` address. Empty: none.
    pub site: String,
    /// The id of the UI flavor to start.
    pub flavor: String,
    /// The workspace profile that the app gets. Empty: none.
    pub profile: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            channel: Channel::Stable,
            mode: UpdateMode::AutoUpdate,
            keep_older: DEFAULT_KEEP_OLDER,
            site: String::new(),
            flavor: MAIN_FLAVOR.to_string(),
            profile: String::new(),
        }
    }
}

impl Settings {
    pub fn path(root: &Path) -> PathBuf {
        root.join("launcher.json")
    }

    /// Reads the settings. A missing or damaged file gives the defaults. Bad values are dropped
    /// one by one (the rest of the file still counts).
    pub fn load(root: &Path) -> Settings {
        let bytes = std::fs::read(Self::path(root)).ok();
        let mut s: Settings = bytes
            .as_deref()
            .and_then(|b| serde_json::from_slice(b).ok())
            .unwrap_or_else(|| {
                // One bad field must not reset the others: read field by field.
                let mut s = Settings::default();
                if let Some(serde_json::Value::Object(o)) = bytes
                    .as_deref()
                    .and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok())
                {
                    s.apply_lenient(&o);
                }
                s
            });
        s.fix();
        s
    }

    fn apply_lenient(&mut self, o: &serde_json::Map<String, serde_json::Value>) {
        if let Some(c) = o
            .get("channel")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            self.channel = c;
        }
        if let Some(m) = o
            .get("mode")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            self.mode = m;
        }
        if let Some(k) = o.get("keep_older").and_then(serde_json::Value::as_u64) {
            self.keep_older = k.min(u64::from(MAX_KEEP_OLDER)) as u32;
        }
        for (key, slot) in [
            ("site", &mut self.site),
            ("flavor", &mut self.flavor),
            ("profile", &mut self.profile),
        ] {
            if let Some(t) = o.get(key).and_then(serde_json::Value::as_str) {
                *slot = t.to_string();
            }
        }
    }

    /// Replaces bad values with safe ones.
    fn fix(&mut self) {
        if !valid_name(&self.flavor) {
            self.flavor = MAIN_FLAVOR.to_string();
        }
        if !self.profile.is_empty() && !valid_name(&self.profile) {
            self.profile.clear();
        }
        if !valid_site(&self.site) {
            self.site.clear();
        }
        self.keep_older = self.keep_older.min(MAX_KEEP_OLDER);
    }

    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(root)?;
        let text = serde_json::to_vec_pretty(self).expect("settings serialize");
        fsio::write_durable(&Self::path(root), &text, Access::KeepExisting, ".tmp")
    }

    /// Reads the file, lets `change` edit it and writes it again, so a change of one part (the
    /// app writes the update part) keeps what another program changed in the other part.
    pub fn update(root: &Path, change: impl FnOnce(&mut Settings)) -> std::io::Result<Settings> {
        let mut s = Settings::load(root);
        change(&mut s);
        s.fix();
        s.save(root)?;
        Ok(s)
    }
}

/// The install folder: `KADAKAS_LAUNCHER_ROOT` (when set and not empty), else the folder
/// `launcher` in the per-user state folder. The launcher and the app both find it here.
pub fn launcher_root(env_root: Option<String>) -> Option<PathBuf> {
    env_root
        .filter(|r| !r.is_empty())
        .map(PathBuf::from)
        .or_else(|| kadakas_sys::dirs::state_dir().map(|d| d.join("launcher")))
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
            mode: UpdateMode::NotifyOnly,
            keep_older: 3,
            site: "file:///tmp/site".into(),
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

    #[test]
    fn modes_keep_number_and_site_have_safe_defaults_and_limits() {
        let d = Settings::default();
        assert_eq!(d.mode, UpdateMode::AutoUpdate);
        assert_eq!(d.keep_older, 1);
        assert_eq!(d.channel, Channel::Stable);
        assert!(d.site.is_empty());
        assert!(UpdateMode::AutoUpdate.downloads_by_itself());
        assert!(UpdateMode::AutoDownload.downloads_by_itself());
        assert!(!UpdateMode::NotifyOnly.downloads_by_itself());
        let root = tmp("settings-new");
        // Old files without the new fields give the defaults for them.
        std::fs::write(
            Settings::path(&root),
            br#"{"channel":"beta","profile":"p"}"#,
        )
        .unwrap();
        let s = Settings::load(&root);
        assert_eq!((s.mode, s.keep_older), (UpdateMode::AutoUpdate, 1));
        // `nightly` is read as the beta channel.
        std::fs::write(Settings::path(&root), br#"{"channel":"nightly"}"#).unwrap();
        assert_eq!(Settings::load(&root).channel, Channel::Beta);
        // A number over the limit is cut. A site that is not a file address is dropped.
        std::fs::write(
            Settings::path(&root),
            br#"{"keep_older":9999,"site":"https://evil.example/m","mode":"notify-only"}"#,
        )
        .unwrap();
        let s = Settings::load(&root);
        assert_eq!(s.keep_older, MAX_KEEP_OLDER);
        assert!(s.site.is_empty());
        assert_eq!(s.mode, UpdateMode::NotifyOnly);
        // One bad field does not reset the others.
        std::fs::write(
            Settings::path(&root),
            br#"{"channel":"weird","mode":"notify-only","keep_older":0,"site":"file:///x","flavor":5,"profile":"p1"}"#,
        )
        .unwrap();
        let s = Settings::load(&root);
        assert_eq!(s.channel, Channel::Stable);
        assert_eq!(s.mode, UpdateMode::NotifyOnly);
        assert_eq!(
            (s.keep_older, s.site.as_str(), s.profile.as_str()),
            (0, "file:///x", "p1")
        );
        assert_eq!(s.flavor, MAIN_FLAVOR);
        assert!(valid_site("") && valid_site("file:///a/b"));
        assert!(!valid_site("file:///a b") && !valid_site("http://x") && !valid_site("/a"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn update_keeps_what_another_program_wrote() {
        let root = tmp("settings-merge");
        Settings {
            flavor: "sketch".into(),
            profile: "acad".into(),
            ..Settings::default()
        }
        .save(&root)
        .unwrap();
        // The app changes the update part only.
        let s = Settings::update(&root, |s| {
            s.mode = UpdateMode::NotifyOnly;
            s.keep_older = 4;
            s.profile = "bad name".into();
        })
        .unwrap();
        assert_eq!(s.profile, "", "a bad value is dropped before the write");
        let again = Settings::load(&root);
        assert_eq!((again.mode, again.keep_older), (UpdateMode::NotifyOnly, 4));
        assert_eq!(again.flavor, "sketch");
        assert!(Settings::update(Path::new("/proc/none/x"), |_| {}).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_install_folder_comes_from_the_variable_or_the_state_folder() {
        assert_eq!(
            launcher_root(Some("/x/y".into())),
            Some(PathBuf::from("/x/y"))
        );
        assert!(launcher_root(Some(String::new())) != Some(PathBuf::new()));
        if let Some(r) = launcher_root(None) {
            assert!(r.ends_with("launcher"));
        }
    }
}
