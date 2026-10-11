//! The user profile files (PLAT-04): reading the `profiles` folder of the config folder,
//! writing and deleting files, and the memory of the active profile.
//!
//! The id of a user profile is the stem of its file name. The store keeps the path of every
//! file, writes and deletes by that path, and renames a file when its id has to change (the
//! stem is no valid id, or another profile already has it). The store reads and writes nothing
//! when it has no config folder (the web build, and tests).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use kadakas_l10n::tr;
use kadakas_ui_core::keymap::Keymap;
use kadakas_ui_core::profile::layout::valid_id;
use kadakas_ui_core::profile::{MAX_FILE_BYTES, Profile, ProfileSet, VECTORWORKS_ID};

#[derive(Default)]
pub struct ProfileStore {
    /// The config folder. `profiles/` and `profile.txt` are in it.
    config: Option<PathBuf>,
    /// The file of each user profile, by id.
    files: HashMap<String, PathBuf>,
}

/// Reads a text file of at most [`MAX_FILE_BYTES`]. A larger file is refused before it is read.
pub fn read_limited(path: &Path) -> Result<String, String> {
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > MAX_FILE_BYTES as u64 {
        return Err(tr!("profiles-ui-too-large", path = path.display()));
    }
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

impl ProfileStore {
    /// A store that touches no file.
    pub fn none() -> ProfileStore {
        ProfileStore::default()
    }

    /// Reads the user profiles in `config/profiles`. `vectorworks_keys` is the keymap that the
    /// Vectorworks-like profile gets (the old `keymap.json` applied to the defaults), so the keys
    /// that work are the keys the profile shows and copies. Returns the profiles, the store
    /// and warnings about files that did not load.
    pub fn load(
        config: &Path,
        vectorworks_keys: Option<Keymap>,
    ) -> (ProfileSet, ProfileStore, Vec<String>) {
        let mut set = ProfileSet::new(Vec::new());
        if let Some(k) = vectorworks_keys {
            set.set_builtin_keymap(VECTORWORKS_ID, k);
        }
        let mut store = ProfileStore {
            config: Some(config.to_path_buf()),
            files: HashMap::new(),
        };
        let mut warnings = Vec::new();
        let Ok(entries) = std::fs::read_dir(config.join("profiles")) else {
            return (set, store, warnings);
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let parsed = read_limited(&path)
                .and_then(|text| Profile::parse(&text, &stem).map_err(|e| e.text()));
            let parsed = match parsed {
                Ok(p) => p,
                Err(e) => {
                    warnings.push(tr!(
                        "profiles-ui-file-ignored",
                        path = path.display(),
                        e = e
                    ));
                    continue;
                }
            };
            if let Some(first) = parsed.problems.first() {
                warnings.push(tr!(
                    "profiles-ui-file-warning",
                    path = path.display(),
                    problem = first.text()
                ));
            }
            // The id is the file stem, whatever the file says inside.
            let mut profile = parsed.profile;
            profile.id = stem.clone();
            let id = set.add_user(profile);
            let mut at = path.clone();
            if id != stem {
                // The stem is no valid id, or it is taken: the file gets the id as its name.
                let renamed = path.with_file_name(format!("{id}.json"));
                match std::fs::rename(&path, &renamed) {
                    Ok(()) => at = renamed,
                    Err(e) => warnings.push(tr!(
                        "profiles-ui-file-ignored",
                        path = path.display(),
                        e = e
                    )),
                }
            }
            store.files.insert(id, at);
        }
        (set, store, warnings)
    }

    /// Writes the user profile `id` to its file. A profile without a file gets
    /// `profiles/<id>.json`. Built-in profiles are never written.
    pub fn write(&mut self, set: &ProfileSet, id: &str) -> Result<(), String> {
        let Some(config) = self.config.clone() else {
            return Ok(());
        };
        let Some(p) = set.get(id).filter(|p| !p.builtin) else {
            return Ok(());
        };
        let path = self
            .files
            .entry(id.to_string())
            .or_insert_with(|| config.join("profiles").join(format!("{id}.json")))
            .clone();
        let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
        else {
            return Ok(());
        };
        crate::prefs::write_file(dir, name, &p.to_json())
            .map_err(|e| tr!("profiles-ui-cannot-save", name = p.name.clone(), e = e))
    }

    /// Deletes the file of a profile that is gone.
    pub fn delete(&mut self, id: &str) {
        if let Some(path) = self.files.remove(id) {
            let _ = std::fs::remove_file(path);
        }
    }

    /// The file of a user profile, if it has one.
    #[cfg(test)]
    pub fn file_of(&self, id: &str) -> Option<&Path> {
        self.files.get(id).map(PathBuf::as_path)
    }

    /// Remembers the active profile for the next start.
    pub fn save_active(&self, id: &str) {
        if let Some(dir) = &self.config {
            let _ = crate::prefs::write_file(dir, "profile.txt", id);
        }
    }

    /// The profile of the last session.
    pub fn saved_active(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.config.as_ref()?.join("profile.txt")).ok()?;
        Some(text.trim().to_string()).filter(|t| !t.is_empty() && valid_id(t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_ui_core::keymap::{Action, Chord};

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kadakas-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("profiles")).unwrap();
        d
    }

    fn json(name: &str) -> String {
        format!(r#"{{"format":1,"name":"{name}"}}"#)
    }

    #[test]
    fn files_are_read_by_stem_and_written_back_to_the_same_path() {
        let dir = temp("rw");
        let pdir = dir.join("profiles");
        std::fs::write(pdir.join("my_one.json"), json("One")).unwrap();
        // The id inside the file is ignored: the stem is the id.
        std::fs::write(
            pdir.join("two.json"),
            r#"{"format":1,"name":"Two","id":"other"}"#,
        )
        .unwrap();
        std::fs::write(pdir.join("bad.json"), "nope").unwrap();
        std::fs::write(pdir.join("note.txt"), "x").unwrap();
        let (mut set, mut store, warnings) = ProfileStore::load(&dir, None);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(set.get("my_one").is_some() && set.get("two").is_some());
        assert!(set.get("other").is_none());
        assert_eq!(
            store.file_of("my_one"),
            Some(pdir.join("my_one.json").as_path())
        );
        // A write goes to the file that was read; a new profile gets its own file.
        set.user_mut("my_one")
            .unwrap()
            .set_chords(Action::ToolLine, &[Chord::parse("Q").unwrap()]);
        store.write(&set, "my_one").unwrap();
        let copy = set.duplicate("my_one").unwrap();
        store.write(&set, &copy).unwrap();
        assert!(pdir.join(format!("{copy}.json")).exists());
        // Built-in and unknown profiles write nothing.
        store.write(&set, "autocad").unwrap();
        store.write(&set, "nope").unwrap();
        assert!(!pdir.join("autocad.json").exists());
        let again = ProfileStore::load(&dir, None).0;
        assert!(
            again
                .get("my_one")
                .unwrap()
                .keymap
                .chords(Action::ToolLine)
                .contains(&Chord::parse("Q").unwrap())
        );
        // Delete removes the file by its path.
        store.delete(&copy);
        assert!(!pdir.join(format!("{copy}.json")).exists());
        store.delete("nope");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_with_a_bad_or_taken_stem_is_renamed() {
        let dir = temp("rename");
        let pdir = dir.join("profiles");
        std::fs::write(pdir.join("My Profile.json"), json("A")).unwrap();
        std::fs::write(pdir.join("vectorworks.json"), json("B")).unwrap();
        let (set, store, warnings) = ProfileStore::load(&dir, None);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(set.get("my-profile").is_some() && set.get("vectorworks-2").is_some());
        assert!(pdir.join("my-profile.json").exists());
        assert!(pdir.join("vectorworks-2.json").exists());
        assert!(!pdir.join("My Profile.json").exists());
        assert_eq!(
            store.file_of("my-profile"),
            Some(pdir.join("my-profile.json").as_path())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn big_files_are_refused_before_they_are_read() {
        let dir = temp("big");
        let big = dir.join("profiles").join("big.json");
        std::fs::write(&big, vec![b' '; MAX_FILE_BYTES + 1]).unwrap();
        assert!(read_limited(&big).is_err());
        let (set, _, warnings) = ProfileStore::load(&dir, None);
        assert_eq!(warnings.len(), 1);
        assert!(set.get("big").is_none());
        assert!(read_limited(&dir.join("none.json")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_legacy_keymap_goes_into_the_vectorworks_profile_and_the_active_id_is_remembered() {
        let dir = temp("legacy");
        let (km, _) = Keymap::with_overrides(r#"{"tool.line": "G"}"#);
        let (set, store, _) = ProfileStore::load(&dir, Some(km.clone()));
        assert_eq!(set.get(VECTORWORKS_ID).unwrap().keymap, km);
        assert_eq!(store.saved_active(), None);
        store.save_active("autocad");
        assert_eq!(store.saved_active().as_deref(), Some("autocad"));
        store.save_active("Not Valid");
        assert_eq!(store.saved_active(), None);
        let none = ProfileStore::none();
        none.save_active("x");
        assert_eq!(none.saved_active(), None);
        let mut none = ProfileStore::none();
        assert!(none.write(&set, "autocad").is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
