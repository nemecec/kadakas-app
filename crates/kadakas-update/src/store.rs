//! The install folder: versioned program folders and the `current` pointer file.
//!
//! ```text
//! <root>/
//!   current                 pointer file (JSON): which version is active, which one was before
//!   versions/1.2.0/...      one folder per installed version
//!   versions/.tmp-1.3.0/    unpacking in progress (deleted at the next start)
//!   downloads/              archives that are downloading
//!   launcher.json           launcher settings
//! ```
//!
//! An install has three steps. Only the last one changes what runs:
//!
//! 1. Unpack the archive into `versions/.tmp-<ver>`.
//! 2. Rename that folder to `versions/<ver>`. A rename is atomic.
//! 3. Write the pointer file `current` (temporary file, flush, rename). This is the atomic switch.
//!
//! A crash before step 3 leaves the pointer as it was, so the old version still runs. The next
//! start removes the half-made folder. The previous version stays on disk for rollback.
//!
//! A version that ships next to the launcher (from the installer) is the `bundled` slot. It is
//! the fallback when no update was installed, and a rollback can go back to it.

use crate::archive::{self, Limits};
use crate::error::{Result, UpdateError};
use crate::version::Version;
use kadakas_sys::fsio::{self, Access};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const BUNDLED: &str = "bundled";
const TMP_PREFIX: &str = ".tmp-";

/// A place where a program version lives.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    /// The version that the installer put next to the launcher.
    Bundled,
    /// A version in `versions/<ver>`.
    Version(Version),
}

impl Slot {
    fn parse(text: &str) -> Result<Slot> {
        if text == BUNDLED {
            Ok(Slot::Bundled)
        } else {
            Ok(Slot::Version(Version::parse(text)?))
        }
    }
}

impl std::fmt::Display for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Slot::Bundled => f.write_str(BUNDLED),
            Slot::Version(v) => write!(f, "{v}"),
        }
    }
}

impl Serialize for Slot {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Slot {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let t = String::deserialize(d)?;
        Slot::parse(&t).map_err(serde::de::Error::custom)
    }
}

/// The program that came with the installer.
#[derive(Clone, Debug)]
pub struct Bundled {
    pub version: Version,
    pub dir: PathBuf,
}

/// The content of the `current` file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pointer {
    pub current: Option<Slot>,
    pub previous: Option<Slot>,
}

/// A step of the install. Tests stop the install after a step, as if the computer had crashed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Extracted,
    Moved,
    PointerWritten,
}

impl Step {
    fn name(self) -> &'static str {
        match self {
            Step::Extracted => "extracted",
            Step::Moved => "moved",
            Step::PointerWritten => "pointer written",
        }
    }
}

/// One line of the list of installed versions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub slot: Slot,
    pub version: Version,
    pub dir: PathBuf,
    pub active: bool,
    /// The version that a rollback would go to.
    pub previous: bool,
}

/// The program that runs now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Active {
    pub slot: Slot,
    pub version: Version,
    pub dir: PathBuf,
}

pub struct Store {
    root: PathBuf,
    bundled: Option<Bundled>,
    fault: Option<Step>,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>, bundled: Option<Bundled>) -> Store {
        Store {
            root: root.into(),
            bundled,
            fault: None,
        }
    }

    /// For tests: stop an install after this step with [`UpdateError::Interrupted`], and clean
    /// up nothing, as a crash would.
    pub fn with_fault(mut self, step: Step) -> Store {
        self.fault = Some(step);
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn versions_dir(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.root.join("downloads")
    }

    fn pointer_path(&self) -> PathBuf {
        self.root.join("current")
    }

    fn hit(&self, step: Step) -> Result<()> {
        if self.fault == Some(step) {
            Err(UpdateError::Interrupted(step.name()))
        } else {
            Ok(())
        }
    }

    /// Reads the pointer file. A missing or damaged file means "nothing installed yet".
    pub fn pointer(&self) -> Pointer {
        std::fs::read(self.pointer_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn write_pointer(&self, p: &Pointer) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let text = serde_json::to_vec_pretty(p).expect("pointer serializes");
        fsio::write_durable(&self.pointer_path(), &text, Access::KeepExisting, ".tmp")?;
        Ok(())
    }

    /// The folder and version of a slot, if the folder exists.
    fn resolve(&self, slot: &Slot) -> Option<Active> {
        let (version, dir) = match slot {
            Slot::Bundled => {
                let b = self.bundled.as_ref()?;
                (b.version.clone(), b.dir.clone())
            }
            Slot::Version(v) => (v.clone(), self.versions_dir().join(v.to_string())),
        };
        dir.is_dir().then(|| Active {
            slot: slot.clone(),
            version,
            dir,
        })
    }

    /// The program to run. It is the current slot. If that folder is gone, the previous one, then
    /// the bundled one. So a damaged install still starts something.
    pub fn active(&self) -> Option<Active> {
        let p = self.pointer();
        [p.current, p.previous, Some(Slot::Bundled)]
            .into_iter()
            .flatten()
            .find_map(|s| self.resolve(&s))
    }

    /// The version that a rollback would switch to.
    pub fn rollback_target(&self) -> Option<Active> {
        let active = self.active()?;
        let p = self.pointer();
        let target = if p.current.as_ref() == Some(&active.slot) {
            p.previous?
        } else {
            return None;
        };
        self.resolve(&target)
    }

    /// All installed versions: the bundled one, then the folders in `versions/`, oldest first.
    pub fn list(&self) -> Vec<Installed> {
        let active = self.active().map(|a| a.slot);
        let previous = self.rollback_target().map(|a| a.slot);
        let mut slots = Vec::new();
        if self.bundled.is_some() {
            slots.push(Slot::Bundled);
        }
        if let Ok(rd) = std::fs::read_dir(self.versions_dir()) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Ok(v) = Version::parse(&name)
                    && e.path().is_dir()
                {
                    slots.push(Slot::Version(v));
                }
            }
        }
        let mut out: Vec<Installed> = slots
            .into_iter()
            .filter_map(|s| self.resolve(&s))
            .map(|a| Installed {
                active: active.as_ref() == Some(&a.slot),
                previous: previous.as_ref() == Some(&a.slot),
                slot: a.slot,
                version: a.version,
                dir: a.dir,
            })
            .collect();
        out.sort_by(|a, b| a.version.cmp(&b.version));
        out
    }

    /// Removes half-made folders from an install that did not finish.
    pub fn recover(&self) {
        if let Ok(rd) = std::fs::read_dir(self.versions_dir()) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with(TMP_PREFIX) {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
        if let Ok(rd) = std::fs::read_dir(self.downloads_dir()) {
            for e in rd.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let _ = std::fs::remove_file(self.root.join("current.tmp"));
    }

    /// Installs `version` from the archive file and makes it the active one.
    ///
    /// It refuses a version that is older than the active one, unless `allow_downgrade` is set.
    /// The updater never sets it. To go back, the user calls [`Store::rollback`].
    pub fn install(
        &self,
        version: &Version,
        archive_file: &Path,
        allow_downgrade: bool,
    ) -> Result<()> {
        let old = self.active();
        if let Some(a) = &old {
            if a.version == *version {
                return Err(UpdateError::AlreadyCurrent(version.to_string()));
            }
            if a.version > *version && !allow_downgrade {
                return Err(UpdateError::Downgrade {
                    current: a.version.to_string(),
                    offered: version.to_string(),
                });
            }
        }
        let dest = self.versions_dir().join(version.to_string());
        if !dest.is_dir() {
            std::fs::create_dir_all(self.versions_dir())?;
            let staging = self.versions_dir().join(format!("{TMP_PREFIX}{version}"));
            let _ = std::fs::remove_dir_all(&staging);
            std::fs::create_dir(&staging)?;
            let unpacked = std::fs::File::open(archive_file)
                .map_err(UpdateError::from)
                .and_then(|f| archive::extract_tar_gz(f, &staging, Limits::default()));
            if let Err(e) = unpacked {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(e);
            }
            self.hit(Step::Extracted)?;
            std::fs::rename(&staging, &dest)?;
            fsio::sync_dir(&self.versions_dir());
            self.hit(Step::Moved)?;
        }
        let pointer = Pointer {
            current: Some(Slot::Version(version.clone())),
            previous: old.map(|a| a.slot),
        };
        self.write_pointer(&pointer)?;
        self.hit(Step::PointerWritten)?;
        self.prune(&pointer);
        Ok(())
    }

    /// Switches back to the previous version. The version that was active becomes the previous
    /// one, so a second rollback goes forward again.
    pub fn rollback(&self) -> Result<Slot> {
        let target = self
            .rollback_target()
            .ok_or(UpdateError::NothingToRollBack)?;
        let p = self.pointer();
        let new = Pointer {
            current: Some(target.slot.clone()),
            previous: p.current,
        };
        self.write_pointer(&new)?;
        Ok(target.slot)
    }

    /// Deletes version folders that are neither active nor the rollback target.
    fn prune(&self, keep: &Pointer) {
        let Ok(rd) = std::fs::read_dir(self.versions_dir()) else {
            return;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let kept = [&keep.current, &keep.previous]
                .into_iter()
                .flatten()
                .any(|s| matches!(s, Slot::Version(v) if v.to_string() == name));
            if !kept {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::archive::tests::{build, file};

    pub fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kadakas-store-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    pub fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    /// Writes an archive file whose app prints its version.
    pub fn archive_file(dir: &Path, ver: &str) -> PathBuf {
        let bytes = build(&[
            file("kadakas-app", format!("app {ver}").as_bytes()),
            file("kadakas", b"cli"),
        ]);
        let p = dir.join(format!("a-{ver}.tar.gz"));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn app_text(a: &Active) -> String {
        std::fs::read_to_string(a.dir.join("kadakas-app")).unwrap()
    }

    pub fn bundled(root: &Path, ver: &str) -> Bundled {
        let dir = root.join("bundled-dir");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kadakas-app"), format!("app {ver}")).unwrap();
        Bundled {
            version: v(ver),
            dir,
        }
    }

    #[test]
    fn empty_store_has_nothing() {
        let root = tmp("empty");
        let s = Store::new(root.join("r"), None);
        assert!(s.active().is_none());
        assert!(s.list().is_empty());
        assert!(matches!(s.rollback(), Err(UpdateError::NothingToRollBack)));
        assert_eq!(s.pointer(), Pointer::default());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn install_switches_and_keeps_the_previous_version() {
        let root = tmp("install");
        let s = Store::new(root.join("r"), Some(bundled(&root, "1.0.0")));
        assert_eq!(s.active().unwrap().slot, Slot::Bundled);

        s.install(&v("1.1.0"), &archive_file(&root, "1.1.0"), false)
            .unwrap();
        let a = s.active().unwrap();
        assert_eq!(a.version, v("1.1.0"));
        assert_eq!(app_text(&a), "app 1.1.0");
        assert_eq!(s.rollback_target().unwrap().slot, Slot::Bundled);

        s.install(&v("1.2.0"), &archive_file(&root, "1.2.0"), false)
            .unwrap();
        assert_eq!(s.active().unwrap().version, v("1.2.0"));
        assert_eq!(s.rollback_target().unwrap().version, v("1.1.0"));
        // 1.0.0 is the bundled copy. It stays. The folders: 1.1.0 and 1.2.0 only.
        let list = s.list();
        let names: Vec<String> = list.iter().map(|i| i.slot.to_string()).collect();
        assert_eq!(names, ["bundled", "1.1.0", "1.2.0"]);
        assert!(list[2].active && list[1].previous && !list[0].active);

        // A third install prunes 1.1.0's predecessor: only current and previous stay.
        s.install(&v("1.3.0"), &archive_file(&root, "1.3.0"), false)
            .unwrap();
        let names: Vec<String> = s.list().iter().map(|i| i.slot.to_string()).collect();
        assert_eq!(names, ["bundled", "1.2.0", "1.3.0"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn downgrade_and_repeat_are_refused() {
        let root = tmp("down");
        let s = Store::new(root.join("r"), None);
        s.install(&v("2.0.0"), &archive_file(&root, "2.0.0"), false)
            .unwrap();
        let old = archive_file(&root, "1.0.0");
        let e = s.install(&v("1.0.0"), &old, false).unwrap_err();
        assert!(matches!(e, UpdateError::Downgrade { .. }), "{e}");
        let e = s
            .install(&v("2.0.0"), &archive_file(&root, "2.0.0"), false)
            .unwrap_err();
        assert!(matches!(e, UpdateError::AlreadyCurrent(_)), "{e}");
        assert_eq!(s.active().unwrap().version, v("2.0.0"));
        // A pre-release of the same number is older than the release.
        let pre = archive_file(&root, "2.0.0-beta.1");
        assert!(s.install(&v("2.0.0-beta.1"), &pre, false).is_err());
        // Only an explicit flag allows it.
        s.install(&v("1.0.0"), &old, true).unwrap();
        assert_eq!(s.active().unwrap().version, v("1.0.0"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rollback_goes_back_and_forth() {
        let root = tmp("rollback");
        let s = Store::new(root.join("r"), Some(bundled(&root, "1.0.0")));
        s.install(&v("1.1.0"), &archive_file(&root, "1.1.0"), false)
            .unwrap();
        assert_eq!(s.rollback().unwrap(), Slot::Bundled);
        assert_eq!(s.active().unwrap().slot, Slot::Bundled);
        assert_eq!(app_text(&s.active().unwrap()), "app 1.0.0");
        // The version that we left is the new rollback target.
        assert_eq!(s.rollback_target().unwrap().version, v("1.1.0"));
        assert_eq!(s.rollback().unwrap(), Slot::Version(v("1.1.0")));
        assert_eq!(s.active().unwrap().version, v("1.1.0"));
        // The rollback survives a new Store value (it is on disk).
        let again = Store::new(root.join("r"), Some(bundled(&root, "1.0.0")));
        assert_eq!(again.active().unwrap().version, v("1.1.0"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rollback_fails_when_the_old_folder_is_gone() {
        let root = tmp("rollback-gone");
        let s = Store::new(root.join("r"), None);
        s.install(&v("1.0.0"), &archive_file(&root, "1.0.0"), false)
            .unwrap();
        s.install(&v("1.1.0"), &archive_file(&root, "1.1.0"), false)
            .unwrap();
        std::fs::remove_dir_all(s.versions_dir().join("1.0.0")).unwrap();
        assert!(matches!(s.rollback(), Err(UpdateError::NothingToRollBack)));
        assert_eq!(s.active().unwrap().version, v("1.1.0"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn interrupted_install_keeps_the_old_version_working() {
        for step in [Step::Extracted, Step::Moved, Step::PointerWritten] {
            let root = tmp(&format!("crash-{step:?}"));
            let r = root.join("r");
            let good = Store::new(&r, None);
            good.install(&v("1.0.0"), &archive_file(&root, "1.0.0"), false)
                .unwrap();

            // The crash.
            let crashing = Store::new(&r, None).with_fault(step);
            let e = crashing
                .install(&v("1.1.0"), &archive_file(&root, "1.1.0"), false)
                .unwrap_err();
            assert!(matches!(e, UpdateError::Interrupted(_)), "{step:?}");

            // A new process starts: it cleans up and runs.
            let after = Store::new(&r, None);
            after.recover();
            let a = after.active().unwrap();
            if step == Step::PointerWritten {
                // The switch had happened: the new version is complete and active.
                assert_eq!(a.version, v("1.1.0"));
                assert_eq!(app_text(&a), "app 1.1.0");
            } else {
                // The switch had not happened: the old version runs, whole.
                assert_eq!(a.version, v("1.0.0"), "{step:?}");
                assert_eq!(app_text(&a), "app 1.0.0");
                assert!(after.pointer().previous.is_none());
                // No half-made folder is left.
                let leftovers: Vec<_> = std::fs::read_dir(after.versions_dir())
                    .unwrap()
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with(TMP_PREFIX))
                    .collect();
                assert!(leftovers.is_empty(), "{leftovers:?}");
                // And the update can be tried again.
                after
                    .install(&v("1.1.0"), &archive_file(&root, "1.1.0"), false)
                    .unwrap();
                assert_eq!(app_text(&after.active().unwrap()), "app 1.1.0");
                assert_eq!(after.rollback_target().unwrap().version, v("1.0.0"));
            }
            std::fs::remove_dir_all(&root).unwrap();
        }
    }

    #[test]
    fn a_damaged_pointer_or_missing_folder_still_starts_something() {
        let root = tmp("damaged");
        let r = root.join("r");
        let s = Store::new(&r, Some(bundled(&root, "1.0.0")));
        s.install(&v("1.1.0"), &archive_file(&root, "1.1.0"), false)
            .unwrap();
        // The current folder is deleted: the previous (bundled) one runs.
        std::fs::remove_dir_all(s.versions_dir().join("1.1.0")).unwrap();
        assert_eq!(s.active().unwrap().slot, Slot::Bundled);
        assert!(s.rollback_target().is_none());
        // The pointer file is garbage: the bundled one runs.
        std::fs::write(r.join("current"), b"{ not json").unwrap();
        assert_eq!(s.pointer(), Pointer::default());
        assert_eq!(s.active().unwrap().slot, Slot::Bundled);
        // A pointer with a path trick is refused when it is read.
        std::fs::write(
            r.join("current"),
            br#"{"current":"../../etc","previous":null}"#,
        )
        .unwrap();
        assert_eq!(s.pointer(), Pointer::default());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_bad_archive_leaves_nothing_behind() {
        let root = tmp("badarchive");
        let s = Store::new(root.join("r"), None);
        s.install(&v("1.0.0"), &archive_file(&root, "1.0.0"), false)
            .unwrap();
        let bad = root.join("bad.tar.gz");
        std::fs::write(&bad, b"garbage").unwrap();
        assert!(s.install(&v("1.1.0"), &bad, false).is_err());
        assert!(
            s.install(&v("1.1.0"), &root.join("missing.tar.gz"), false)
                .is_err()
        );
        assert_eq!(s.active().unwrap().version, v("1.0.0"));
        let names: Vec<String> = std::fs::read_dir(s.versions_dir())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["1.0.0"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn recover_clears_downloads() {
        let root = tmp("recover");
        let s = Store::new(root.join("r"), None);
        std::fs::create_dir_all(s.downloads_dir()).unwrap();
        std::fs::write(s.downloads_dir().join("x.part"), b"1").unwrap();
        s.recover();
        assert!(!s.downloads_dir().join("x.part").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn slot_text() {
        assert_eq!(Slot::parse("bundled").unwrap(), Slot::Bundled);
        assert!(Slot::parse("../x").is_err());
        assert_eq!(Slot::Version(v("1.0.0")).to_string(), "1.0.0");
        assert_eq!(Step::Moved.name(), "moved");
    }
}
