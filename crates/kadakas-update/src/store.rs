//! The install folder: versioned program folders and the `current` pointer file.
//!
//! ```text
//! <root>/
//!   current                 pointer file (JSON): which version is active, which one was before
//!   versions/1.2.0/...      one folder per installed version
//!   versions/.tmp-1.3.0/    unpacking in progress (deleted at the next start)
//!   downloads/              archives that are downloading
//!   seen.json               the highest manifest version seen per channel (refuses replays)
//!   lock                    advisory lock file: one launcher changes the install at a time
//!   launcher.json           launcher settings (channel, mode, versions to keep, flavor, profile)
//!   health.json             versions whose last start ended without a clean-exit mark
//!   runs/                   one record per running app (see `runs`)
//! ```
//!
//! Every version folder has a small file `.kadakas-meta.json`: the label (`stable`, `nightly`
//! or `local`), the git commit of a local build, the time it was added, and whether the person
//! keeps it. Pruning keeps the newest version and `keep_older` more, the running versions, the
//! current one and the kept ones; the bundled app is never a folder here and is never deleted.
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
//! Every folder in `versions/` has a marker file with the SHA-256 of the archive it came from. A
//! folder without the right marker is never reused: it is unpacked again.
//!
//! A version that ships next to the launcher (from the installer) is the `bundled` slot. It is
//! the fallback when no update was installed, and a rollback can go back to it.

use crate::archive::{self, Limits};
use crate::error::{Result, UpdateError};
use crate::manifest::Channel;
use crate::updater::sha256_hex;
use crate::version::Version;
use kadakas_sys::fsio::{self, Access};
use serde::{Deserialize, Serialize};
use std::io::Seek;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// The name of the marker file in a finished version folder.
const MARKER: &str = ".kadakas-complete";

/// The name of the file with the label and the origin of a version.
const META: &str = ".kadakas-meta.json";

/// The label of the app that came with the installer.
pub const BUNDLED_LABEL: &str = "bundled";

/// What the store knows about where a version came from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VersionMeta {
    /// `stable`, `nightly` or `local` (see `Channel::label`), or `bundled`.
    pub label: String,
    /// The git commit of a local build.
    pub commit: Option<String>,
    /// When the version was added, in seconds since 1970.
    pub added: u64,
    /// The person keeps this version: pruning never deletes it.
    pub kept: bool,
}

/// An archive file and the SHA-256 that it must have.
#[derive(Clone, Debug)]
pub struct ArchiveFile {
    pub path: PathBuf,
    pub sha256: String,
}

const BUNDLED: &str = "bundled";
const TMP_PREFIX: &str = ".tmp-";
const OLD_PREFIX: &str = ".old-";

/// A place where a program version lives.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
#[serde(default)]
pub struct Pointer {
    pub current: Option<Slot>,
    pub previous: Option<Slot>,
    /// The version of the bundled app when the pointer was written. A bundled app that is
    /// newer than this one (the person installed a new Kadakas) wins over the pointer; one that
    /// is the same does not, so the person can choose an older version or a local build.
    pub bundled: Option<Version>,
    /// Set when the person chose this version by hand (`Store::activate`): the time of the
    /// choice. In Auto-update the choice stands until a newer version is added, see `choose`.
    pub explicit_at: Option<u64>,
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
    /// Where the version came from.
    pub meta: VersionMeta,
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
    /// How long a waiting lock tries, in milliseconds.
    lock_wait_ms: u64,
    /// How many older versions pruning keeps besides the newest one.
    keep_older: AtomicU32,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
}

fn system_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Store {
    pub fn new(root: impl Into<PathBuf>, bundled: Option<Bundled>) -> Store {
        Store {
            root: root.into(),
            bundled,
            fault: None,
            lock_wait_ms: 10_000,
            keep_older: AtomicU32::new(crate::settings::DEFAULT_KEEP_OLDER),
            clock: Arc::new(system_seconds),
        }
    }

    /// Sets how many older versions pruning keeps (0 or more).
    pub fn with_keep_older(self, n: u32) -> Store {
        self.set_keep_older(n);
        self
    }

    pub fn set_keep_older(&self, n: u32) {
        self.keep_older
            .store(n.min(crate::settings::MAX_KEEP_OLDER), Ordering::Relaxed);
    }

    pub fn keep_older(&self) -> u32 {
        self.keep_older.load(Ordering::Relaxed)
    }

    /// Replaces the clock (seconds since 1970). For tests.
    pub fn with_clock(mut self, clock: Arc<dyn Fn() -> u64 + Send + Sync>) -> Store {
        self.clock = clock;
        self
    }

    pub fn bundled(&self) -> Option<&Bundled> {
        self.bundled.as_ref()
    }

    /// For tests: stop an install after this step with [`UpdateError::Interrupted`], and clean
    /// up nothing, as a crash would.
    pub fn with_fault(mut self, step: Step) -> Store {
        self.fault = Some(step);
        self
    }

    /// Sets how long an install or rollback waits for the lock of another launcher.
    pub fn with_lock_wait(mut self, wait: std::time::Duration) -> Store {
        self.lock_wait_ms = wait.as_millis() as u64;
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

    /// Takes the install lock. With `wait`, it tries for `lock_wait_ms` (10 s), then gives up with
    /// [`UpdateError::Busy`]. Without `wait`, it returns `None` when another launcher holds it.
    /// The lock goes when the returned file is dropped (or when the process ends).
    pub(crate) fn lock(&self, wait: bool) -> Result<Option<std::fs::File>> {
        if wait {
            self.lock_for(self.lock_wait_ms)?
                .map(Some)
                .ok_or(UpdateError::Busy)
        } else {
            self.lock_for(0)
        }
    }

    /// Tries to take the install lock for at most `ms` milliseconds (0: one try).
    fn lock_for(&self, ms: u64) -> Result<Option<std::fs::File>> {
        std::fs::create_dir_all(&self.root)?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.root.join("lock"))?;
        let tries = ms / 50 + 1;
        for i in 0..tries {
            match file.try_lock() {
                Ok(()) => return Ok(Some(file)),
                Err(std::fs::TryLockError::WouldBlock) => {
                    if i + 1 < tries {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                }
                Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
            }
        }
        Ok(None)
    }

    fn seen_path(&self) -> PathBuf {
        self.root.join("seen.json")
    }

    fn read_seen(&self) -> std::collections::BTreeMap<String, Version> {
        std::fs::read(self.seen_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// The highest manifest version that was accepted on a channel.
    pub fn seen(&self, channel: Channel) -> Option<Version> {
        self.read_seen().remove(channel.as_str())
    }

    /// Remembers a manifest version, if it is higher than the one remembered.
    pub fn note_seen(&self, channel: Channel, version: &Version) -> Result<()> {
        let _guard = self.lock(true)?;
        let mut map = self.read_seen();
        if map.get(channel.as_str()).is_some_and(|v| v >= version) {
            return Ok(());
        }
        map.insert(channel.as_str().to_string(), version.clone());
        let text = serde_json::to_vec_pretty(&map).expect("map serializes");
        fsio::write_durable(&self.seen_path(), &text, Access::KeepExisting, ".tmp")?;
        Ok(())
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
        let mut p = p.clone();
        p.bundled = self.bundled.as_ref().map(|b| b.version.clone());
        let text = serde_json::to_vec_pretty(&p).expect("pointer serializes");
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
    ///
    /// A bundled version that is newer than the pointer's choice wins. That happens when the
    /// user installs a new Kadakas installer over an older update.
    pub fn active(&self) -> Option<Active> {
        let p = self.pointer();
        let chosen = [p.current.clone(), p.previous.clone(), Some(Slot::Bundled)]
            .into_iter()
            .flatten()
            .find_map(|s| self.resolve(&s))?;
        match self.resolve(&Slot::Bundled) {
            Some(b) if b.version > chosen.version && p.bundled.as_ref() != Some(&b.version) => {
                Some(b)
            }
            _ => Some(chosen),
        }
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
                meta: self.meta(&a.slot),
                slot: a.slot,
                version: a.version,
                dir: a.dir,
            })
            .collect();
        out.sort_by(|a, b| a.version.cmp(&b.version));
        out
    }

    /// The origin of a version. The bundled app is labelled `bundled`. A folder without the
    /// file (from an older launcher) is a release of unknown origin: label `stable`.
    pub fn meta(&self, slot: &Slot) -> VersionMeta {
        match slot {
            Slot::Bundled => VersionMeta {
                label: BUNDLED_LABEL.into(),
                ..VersionMeta::default()
            },
            Slot::Version(v) => std::fs::read(self.versions_dir().join(v.to_string()).join(META))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_else(|| VersionMeta {
                    label: "stable".into(),
                    ..VersionMeta::default()
                }),
        }
    }

    fn write_meta(&self, dir: &Path, meta: &VersionMeta) -> Result<()> {
        let text = serde_json::to_vec_pretty(meta).expect("meta serializes");
        fsio::write_durable(&dir.join(META), &text, Access::KeepExisting, ".tmp")?;
        Ok(())
    }

    /// Says that the person keeps a version (or not). Pruning never deletes a kept version.
    pub fn set_kept(&self, version: &Version, kept: bool) -> Result<()> {
        let _guard = self.lock(true)?;
        let slot = Slot::Version(version.clone());
        let dir = self
            .resolve(&slot)
            .ok_or_else(|| UpdateError::NotInstalled(version.to_string()))?
            .dir;
        let mut meta = self.meta(&slot);
        meta.kept = kept;
        self.write_meta(&dir, &meta)
    }

    /// Removes half-made folders from an install that did not finish.
    /// It does nothing when another launcher holds the install lock.
    pub fn recover(&self) {
        // A short wait: a lock can stay a moment in a process that was just forked.
        let Ok(Some(_guard)) = self.lock_for(self.lock_wait_ms.min(300)) else {
            return;
        };
        if let Ok(rd) = std::fs::read_dir(self.versions_dir()) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with(TMP_PREFIX) {
                    let _ = std::fs::remove_dir_all(e.path());
                } else if let Some(ver) = name.strip_prefix(OLD_PREFIX) {
                    // A swap of `add_local` that stopped between its two renames: the old
                    // folder comes back if the new one never arrived, else it is deleted.
                    let dest = self.versions_dir().join(ver);
                    if dest.exists() {
                        let _ = std::fs::remove_dir_all(e.path());
                    } else {
                        let _ = std::fs::rename(e.path(), dest);
                    }
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
    /// The archive is opened once. The SHA-256 is computed from that open file, and the same file
    /// is unpacked, so the file cannot change between the check and the use.
    /// It refuses a version that is older than the active one, unless `allow_downgrade` is set.
    /// The updater never sets it. To go back, the user calls [`Store::rollback`].
    /// The whole install runs under the install lock.
    pub fn install(
        &self,
        version: &Version,
        archive_file: &ArchiveFile,
        allow_downgrade: bool,
    ) -> Result<()> {
        self.install_with(version, archive_file, allow_downgrade, "stable", true)
    }

    /// Unpacks a version into the store and does not switch to it. The pointer stays as it was
    /// (Auto-download and Notify only: the person decides when to switch, see [`Store::activate`]).
    pub fn stage(&self, version: &Version, archive_file: &ArchiveFile, label: &str) -> Result<()> {
        self.install_with(version, archive_file, false, label, false)
    }

    /// [`Store::install`] with a label for the version, and the choice to switch or not.
    pub fn install_with(
        &self,
        version: &Version,
        archive_file: &ArchiveFile,
        allow_downgrade: bool,
        label: &str,
        switch: bool,
    ) -> Result<()> {
        let _guard = self.lock(true)?;
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
        let reusable = std::fs::read_to_string(dest.join(MARKER))
            .is_ok_and(|m| m.trim() == archive_file.sha256)
            && dest.join(META).is_file();
        if !reusable {
            std::fs::create_dir_all(self.versions_dir())?;
            let mut file = std::fs::File::open(&archive_file.path)?;
            if sha256_hex(&mut file)? != archive_file.sha256 {
                return Err(UpdateError::HashMismatch);
            }
            file.rewind()?;
            let staging = self.versions_dir().join(format!("{TMP_PREFIX}{version}"));
            let _ = std::fs::remove_dir_all(&staging);
            std::fs::create_dir(&staging)?;
            let unpacked = archive::extract_tar_gz(&file, &staging, Limits::default())
                .and_then(|_| {
                    std::fs::write(staging.join(MARKER), &archive_file.sha256).map_err(Into::into)
                })
                .and_then(|()| {
                    self.write_meta(
                        &staging,
                        &VersionMeta {
                            label: label.to_string(),
                            commit: None,
                            added: (self.clock)(),
                            kept: false,
                        },
                    )
                });
            if let Err(e) = unpacked {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(e);
            }
            self.hit(Step::Extracted)?;
            // A folder without the right marker is not trusted: replace it.
            let _ = std::fs::remove_dir_all(&dest);
            std::fs::rename(&staging, &dest)?;
            fsio::sync_dir(&self.versions_dir());
            self.hit(Step::Moved)?;
        }
        // A new copy of a version that crashed before is a new start.
        self.forget_crash(&Slot::Version(version.clone()));
        if switch {
            let pointer = Pointer {
                current: Some(Slot::Version(version.clone())),
                previous: old.map(|a| a.slot),
                bundled: None,
                explicit_at: None,
            };
            self.write_pointer(&pointer)?;
            self.hit(Step::PointerWritten)?;
        }
        self.prune_locked();
        Ok(())
    }

    /// Makes an installed version the one that starts when the launcher does not choose by
    /// itself (the pointer, see the modes in `choose`). The version that was active becomes the
    /// rollback target.
    ///
    /// This is the person's choice: in Auto-update too it stands until a newer version is added
    /// (`Pointer::explicit_at`).
    pub fn activate(&self, slot: &Slot) -> Result<()> {
        let _guard = self.lock(true)?;
        self.activate_locked(slot, true)
    }

    /// Like [`Store::activate`], for a switch that the launcher makes itself (the restart to
    /// update in the manual modes): it is not a choice that holds back newer versions.
    pub fn switch_to(&self, slot: &Slot) -> Result<()> {
        let _guard = self.lock(true)?;
        self.activate_locked(slot, false)
    }

    /// The version that the person chose by hand, and when. `None` when there is no such
    /// choice, or its folder is gone.
    pub fn explicit_choice(&self) -> Option<(Slot, u64)> {
        let p = self.pointer();
        let slot = p.current?;
        let at = p.explicit_at?;
        self.resolve(&slot).map(|_| (slot, at))
    }

    fn activate_locked(&self, slot: &Slot, explicit: bool) -> Result<()> {
        if self.resolve(slot).is_none() {
            return Err(UpdateError::NotInstalled(slot.to_string()));
        }
        self.forget_crash(slot);
        let old = self.active().map(|a| a.slot);
        let p = self.pointer();
        if old.as_ref() == Some(slot)
            && p.current.as_ref() == Some(slot)
            && (!explicit || p.explicit_at.is_some())
        {
            return Ok(());
        }
        let pointer = Pointer {
            current: Some(slot.clone()),
            previous: old.filter(|o| o != slot).or(self.pointer().previous),
            bundled: None,
            explicit_at: explicit.then(|| (self.clock)()),
        };
        self.write_pointer(&pointer)
    }

    /// Puts a build from this machine into the store with the label `local` and its git commit
    /// (PLAT-17). This is one of the two places where an unsigned build may come in (the other
    /// is a manifest in a local folder). The pointer does not change: Auto-update picks a local
    /// version only on the channel `local`, and the person can switch to it by hand.
    pub fn add_local(
        &self,
        version: &Version,
        build: &crate::local::LocalBuild,
        commit: Option<&str>,
    ) -> Result<Slot> {
        let _guard = self.lock(true)?;
        let slot = Slot::Version(version.clone());
        // Only a local build may be replaced by a local build, and never the version that the
        // pointer names (it is not replaced in place; add another build and switch to it).
        let exists = self.resolve(&slot).is_some();
        if self.running_slots().contains(&slot)
            || self.pointer().current.as_ref() == Some(&slot)
            || (exists && self.meta(&slot).label != "local")
        {
            return Err(UpdateError::NotRemovable(version.to_string()));
        }
        std::fs::create_dir_all(self.versions_dir())?;
        let staging = self.versions_dir().join(format!("{TMP_PREFIX}{version}"));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir(&staging)?;
        let copied = (|| -> Result<()> {
            for rel in &build.files {
                let to = staging.join(rel);
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(build.dir.join(rel), &to)?;
            }
            std::fs::write(staging.join(MARKER), "local")?;
            self.write_meta(
                &staging,
                &VersionMeta {
                    label: "local".into(),
                    commit: commit.map(str::to_string),
                    added: (self.clock)(),
                    kept: false,
                },
            )
        })();
        if let Err(e) = copied {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
        let dest = self.versions_dir().join(version.to_string());
        // Swap, then delete: the old folder is renamed away, the new one renamed in, and only
        // then the old one is deleted. If the second rename fails, the old one comes back.
        let old = self.versions_dir().join(format!("{OLD_PREFIX}{version}"));
        let _ = std::fs::remove_dir_all(&old);
        let had_old = dest.exists();
        if had_old && let Err(e) = std::fs::rename(&dest, &old) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e.into());
        }
        if let Err(e) = std::fs::rename(&staging, &dest) {
            if had_old {
                let _ = std::fs::rename(&old, &dest);
            }
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e.into());
        }
        let _ = std::fs::remove_dir_all(&old);
        fsio::sync_dir(&self.versions_dir());
        self.forget_crash(&slot);
        self.prune_locked();
        Ok(slot)
    }

    /// Removes an installed version. The bundled app, a running version and the current one stay.
    pub fn remove(&self, version: &Version) -> Result<()> {
        let _guard = self.lock(true)?;
        let slot = Slot::Version(version.clone());
        let a = self
            .resolve(&slot)
            .ok_or_else(|| UpdateError::NotInstalled(version.to_string()))?;
        let p = self.pointer();
        if self.running_slots().contains(&slot) || p.current.as_ref() == Some(&slot) {
            return Err(UpdateError::NotRemovable(version.to_string()));
        }
        std::fs::remove_dir_all(&a.dir)?;
        if p.previous.as_ref() == Some(&slot) {
            self.write_pointer(&Pointer {
                previous: None,
                ..p
            })?;
        }
        self.forget_crash(&slot);
        Ok(())
    }

    /// Switches back to the previous version. The version that was active becomes the previous
    /// one, so a second rollback goes forward again.
    pub fn rollback(&self) -> Result<Slot> {
        let _guard = self.lock(true)?;
        let target = self
            .rollback_target()
            .ok_or(UpdateError::NothingToRollBack)?;
        let p = self.pointer();
        let left = p.current.clone();
        let new = Pointer {
            current: Some(target.slot.clone()),
            previous: p.current,
            bundled: None,
            explicit_at: None,
        };
        self.write_pointer(&new)?;
        // The version we left is not started again by Auto-update, until the person asks for it.
        if let Some(left) = left {
            self.note_rolled_back(&left);
        }
        self.forget_crash(&target.slot);
        Ok(target.slot)
    }

    /// Deletes old version folders (see [`Store::keep_set`]). The bundled app is never touched.
    pub fn prune(&self) -> Result<()> {
        let _guard = self.lock(true)?;
        self.prune_locked();
        Ok(())
    }

    fn prune_locked(&self) {
        let keep = self.keep_set();
        let Ok(rd) = std::fs::read_dir(self.versions_dir()) else {
            return;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            // Half-made folders belong to `recover`; a folder that is not a version is not ours.
            let Ok(v) = Version::parse(&name) else {
                continue;
            };
            if !keep.contains(&Slot::Version(v)) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }

    /// The versions that pruning keeps: the newest one and `keep_older` more (newest first, by
    /// the time they were added), the version that the pointer names, the versions that run now,
    /// and the versions that the person keeps. The bundled app is not a folder here and is
    /// always there, as the fallback.
    pub fn keep_set(&self) -> std::collections::HashSet<Slot> {
        let mut folders: Vec<(u64, Version)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(self.versions_dir()) {
            for e in rd.flatten() {
                if let Ok(v) = Version::parse(&e.file_name().to_string_lossy())
                    && e.path().is_dir()
                {
                    let added = self.meta(&Slot::Version(v.clone())).added;
                    folders.push((added, v));
                }
            }
        }
        folders.sort_by(|a, b| b.cmp(a));
        let mut keep: std::collections::HashSet<Slot> = folders
            .iter()
            .take(1 + self.keep_older() as usize)
            .map(|(_, v)| Slot::Version(v.clone()))
            .collect();
        // The version that the launcher falls back to when the newest ones are skipped (a crash,
        // a rollback) is never pruned, also with 0 older versions to keep.
        let skipped: std::collections::BTreeSet<Slot> = self
            .crashed_slots()
            .union(&self.rolled_back_slots())
            .cloned()
            .collect();
        for (_, v) in &folders {
            let slot = Slot::Version(v.clone());
            let stop = !skipped.contains(&slot);
            keep.insert(slot);
            if stop {
                break;
            }
        }
        keep.extend(self.running_slots());
        keep.extend(self.pointer().current);
        for (_, v) in &folders {
            let slot = Slot::Version(v.clone());
            if self.meta(&slot).kept {
                keep.insert(slot);
            }
        }
        keep
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
    pub fn archive_file(dir: &Path, ver: &str) -> ArchiveFile {
        let bytes = build(&[
            file("kadakas-app", format!("app {ver}").as_bytes()),
            file("kadakas", b"cli"),
        ]);
        let p = dir.join(format!("a-{ver}.tar.gz"));
        let sha256 = sha256_hex(&bytes[..]).unwrap();
        std::fs::write(&p, bytes).unwrap();
        ArchiveFile { path: p, sha256 }
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

    // ---- Versions to keep, stage, activate, local builds (PLAT-16, PLAT-17) ----

    /// A store whose clock counts up, so versions have a clear order of adding.
    fn counting(root: &Path, bundled_version: Option<&str>) -> Store {
        let tick = Arc::new(std::sync::atomic::AtomicU64::new(100));
        Store::new(root.join("r"), bundled_version.map(|b| bundled(root, b))).with_clock(Arc::new(
            move || tick.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ))
    }

    fn folders(s: &Store) -> Vec<String> {
        let mut out: Vec<String> = s
            .list()
            .into_iter()
            .filter(|i| i.slot != Slot::Bundled)
            .map(|i| i.version.to_string())
            .collect();
        out.sort();
        out
    }

    fn install_all(s: &Store, root: &Path, versions: &[&str]) {
        for ver in versions {
            s.install(&v(ver), &archive_file(root, ver), false).unwrap();
        }
    }

    #[test]
    fn pruning_keeps_the_newest_and_n_older() {
        let root = tmp("keep-n");
        for (n, want) in [
            (0, vec!["1.5.0"]),
            (1, vec!["1.4.0", "1.5.0"]),
            (2, vec!["1.3.0", "1.4.0", "1.5.0"]),
            (9, vec!["1.1.0", "1.2.0", "1.3.0", "1.4.0", "1.5.0"]),
        ] {
            let sub = root.join(format!("n{n}"));
            std::fs::create_dir_all(&sub).unwrap();
            let s = counting(&sub, Some("1.0.0")).with_keep_older(n);
            install_all(&s, &sub, &["1.1.0", "1.2.0", "1.3.0", "1.4.0", "1.5.0"]);
            assert_eq!(folders(&s), want, "keep {n}");
            // The bundled app is never counted and never deleted.
            assert!(s.list().iter().any(|i| i.slot == Slot::Bundled));
            assert!(sub.join("bundled-dir").join("kadakas-app").is_file());
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_keep_setting_is_limited_and_changes_live() {
        let root = tmp("keep-live");
        let s = counting(&root, Some("1.0.0"));
        assert_eq!(s.keep_older(), 1);
        s.set_keep_older(1000);
        assert_eq!(s.keep_older(), crate::settings::MAX_KEEP_OLDER);
        s.set_keep_older(3);
        install_all(&s, &root, &["1.1.0", "1.2.0", "1.3.0", "1.4.0"]);
        assert_eq!(folders(&s).len(), 4);
        // A lower number takes effect at the next prune.
        s.set_keep_older(0);
        s.prune().unwrap();
        assert_eq!(folders(&s), ["1.4.0"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn pruning_never_deletes_the_running_version_the_current_one_or_a_kept_one() {
        let root = tmp("keep-protected");
        let s = counting(&root, Some("1.0.0")).with_keep_older(0);
        install_all(&s, &root, &["1.1.0", "1.2.0"]);
        assert_eq!(folders(&s), ["1.2.0"]);
        // 1.2.0 runs. Newer versions arrive and the keep number is 0: 1.2.0 stays.
        let guard = crate::runs::RunGuard::begin(&s, &Slot::Version(v("1.2.0"))).unwrap();
        install_all(&s, &root, &["1.3.0", "1.4.0"]);
        assert_eq!(folders(&s), ["1.2.0", "1.4.0"]);
        // The person keeps 1.4.0, then newer ones come.
        s.set_kept(&v("1.4.0"), true).unwrap();
        install_all(&s, &root, &["1.5.0"]);
        assert_eq!(folders(&s), ["1.2.0", "1.4.0", "1.5.0"]);
        assert!(
            s.list()
                .iter()
                .any(|i| i.version == v("1.4.0") && i.meta.kept)
        );
        // The app ends. The running version is no longer protected; the kept one still is.
        guard.finish(&s);
        s.prune().unwrap();
        assert_eq!(folders(&s), ["1.4.0", "1.5.0"]);
        s.set_kept(&v("1.4.0"), false).unwrap();
        s.prune().unwrap();
        assert_eq!(folders(&s), ["1.5.0"]);
        assert!(matches!(
            s.set_kept(&v("9.9.9"), true),
            Err(UpdateError::NotInstalled(_))
        ));
        // The version that the pointer names stays, also when it is not the newest.
        s.stage(&v("1.6.0"), &archive_file(&root, "1.6.0"), "stable")
            .unwrap();
        s.stage(&v("1.7.0"), &archive_file(&root, "1.7.0"), "stable")
            .unwrap();
        assert_eq!(folders(&s), ["1.5.0", "1.7.0"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn stage_does_not_switch_and_activate_does() {
        let root = tmp("stage-activate");
        let s = counting(&root, Some("1.0.0"));
        let a = archive_file(&root, "1.1.0");
        s.stage(&v("1.1.0"), &a, "nightly").unwrap();
        assert_eq!(s.active().unwrap().slot, Slot::Bundled);
        assert_eq!(s.pointer(), Pointer::default());
        let item = s
            .list()
            .into_iter()
            .find(|i| i.version == v("1.1.0"))
            .unwrap();
        assert_eq!(item.meta.label, "nightly");
        // Staging the same archive again reuses the folder.
        s.stage(&v("1.1.0"), &a, "nightly").unwrap();
        s.activate(&Slot::Version(v("1.1.0"))).unwrap();
        assert_eq!(s.active().unwrap().version, v("1.1.0"));
        assert_eq!(s.rollback_target().unwrap().slot, Slot::Bundled);
        // Again: nothing changes.
        let p = s.pointer();
        s.activate(&Slot::Version(v("1.1.0"))).unwrap();
        assert_eq!(s.pointer(), p);
        // Back to the bundled app by choice.
        s.activate(&Slot::Bundled).unwrap();
        assert_eq!(s.active().unwrap().slot, Slot::Bundled);
        assert_eq!(s.pointer().previous, Some(Slot::Version(v("1.1.0"))));
        assert!(matches!(
            s.activate(&Slot::Version(v("9.0.0"))),
            Err(UpdateError::NotInstalled(_))
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_folder_without_the_meta_file_is_a_stable_release_and_is_not_reused() {
        let root = tmp("meta-missing");
        let s = counting(&root, Some("1.0.0"));
        let a = archive_file(&root, "1.1.0");
        s.stage(&v("1.1.0"), &a, "nightly").unwrap();
        let dir = s.versions_dir().join("1.1.0");
        std::fs::remove_file(dir.join(META)).unwrap();
        assert_eq!(s.meta(&Slot::Version(v("1.1.0"))).label, "stable");
        assert_eq!(s.meta(&Slot::Bundled).label, BUNDLED_LABEL);
        // The folder is not trusted without its meta file: it is unpacked again.
        s.stage(&v("1.1.0"), &a, "nightly").unwrap();
        assert_eq!(s.meta(&Slot::Version(v("1.1.0"))).label, "nightly");
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn fake_build(root: &Path, name: &str, with_cli: bool) -> crate::local::LocalBuild {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = |n: &str| format!("{n}{}", std::env::consts::EXE_SUFFIX);
        std::fs::write(dir.join(exe("kadakas-app")), format!("app {name}")).unwrap();
        let mut files = vec![PathBuf::from(exe("kadakas-app"))];
        if with_cli {
            std::fs::write(dir.join(exe("kadakas")), b"cli").unwrap();
            files.push(PathBuf::from(exe("kadakas")));
        }
        crate::local::LocalBuild { dir, files }
    }

    #[test]
    fn a_local_build_goes_into_the_store_with_its_commit() {
        let root = tmp("add-local");
        let s = counting(&root, Some("1.0.0"));
        let build = fake_build(&root, "build1", true);
        let ver = v("1.0.0-local.gabc");
        let slot = s.add_local(&ver, &build, Some("abc123")).unwrap();
        assert_eq!(slot, Slot::Version(ver.clone()));
        let item = s.list().into_iter().find(|i| i.version == ver).unwrap();
        assert_eq!(item.meta.label, "local");
        assert_eq!(item.meta.commit.as_deref(), Some("abc123"));
        assert_eq!(
            std::fs::read_to_string(item.dir.join("kadakas-app")).unwrap(),
            "app build1"
        );
        assert!(item.dir.join("kadakas").is_file());
        // The pointer did not move: Auto-update decides by channel.
        assert_eq!(s.active().unwrap().slot, Slot::Bundled);
        // A new build with the same number replaces the folder.
        let again = fake_build(&root, "build2", false);
        s.add_local(&ver, &again, None).unwrap();
        let item = s.list().into_iter().find(|i| i.version == ver).unwrap();
        assert_eq!(
            std::fs::read_to_string(item.dir.join("kadakas-app")).unwrap(),
            "app build2"
        );
        assert!(!item.dir.join("kadakas").exists());
        assert_eq!(item.meta.commit, None);
        // A running local version is not replaced.
        let g = crate::runs::RunGuard::begin(&s, &slot).unwrap();
        assert!(matches!(
            s.add_local(&ver, &again, None),
            Err(UpdateError::NotRemovable(_))
        ));
        drop(g);
        // A build with a missing file leaves nothing half-made.
        let mut broken = fake_build(&root, "build3", false);
        broken.files.push(PathBuf::from("missing"));
        let other = v("1.0.0-local.gdef");
        assert!(s.add_local(&other, &broken, None).is_err());
        assert!(
            !s.versions_dir()
                .join(format!("{TMP_PREFIX}{other}"))
                .exists()
        );
        assert!(!s.versions_dir().join(other.to_string()).exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn remove_refuses_the_bundled_the_running_and_the_current_version() {
        let root = tmp("remove");
        let s = counting(&root, Some("1.0.0")).with_keep_older(5);
        install_all(&s, &root, &["1.1.0", "1.2.0", "1.3.0"]);
        // Current: 1.3.0. Previous: 1.2.0.
        assert!(matches!(
            s.remove(&v("1.3.0")),
            Err(UpdateError::NotRemovable(_))
        ));
        let g = crate::runs::RunGuard::begin(&s, &Slot::Version(v("1.1.0"))).unwrap();
        assert!(matches!(
            s.remove(&v("1.1.0")),
            Err(UpdateError::NotRemovable(_))
        ));
        assert!(matches!(
            s.remove(&v("8.0.0")),
            Err(UpdateError::NotInstalled(_))
        ));
        drop(g);
        s.remove(&v("1.1.0")).unwrap();
        assert_eq!(folders(&s), ["1.2.0", "1.3.0"]);
        // Removing the rollback target clears it.
        s.remove(&v("1.2.0")).unwrap();
        assert!(s.rollback_target().is_none());
        assert_eq!(s.pointer().previous, None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_rollback_remembers_the_version_it_left() {
        let root = tmp("rollback-skip");
        let s = counting(&root, Some("1.0.0"));
        install_all(&s, &root, &["1.1.0"]);
        s.rollback().unwrap();
        assert!(s.rolled_back_slots().contains(&Slot::Version(v("1.1.0"))));
        // Going forward again takes the mark away.
        s.rollback().unwrap();
        assert!(!s.rolled_back_slots().contains(&Slot::Version(v("1.1.0"))));
        assert!(s.rolled_back_slots().contains(&Slot::Bundled));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_choice_by_hand_is_remembered_and_a_switch_of_the_launcher_is_not() {
        let root = tmp("explicit");
        let s = counting(&root, Some("1.0.0"));
        install_all(&s, &root, &["1.1.0", "1.2.0"]);
        assert!(s.explicit_choice().is_none(), "an install is no choice");
        s.activate(&Slot::Version(v("1.1.0"))).unwrap();
        let (slot, at) = s.explicit_choice().unwrap();
        assert_eq!(slot, Slot::Version(v("1.1.0")));
        assert!(at >= 100);
        // The same choice again changes nothing.
        let p = s.pointer();
        s.activate(&Slot::Version(v("1.1.0"))).unwrap();
        assert_eq!(s.pointer(), p);
        // A switch by the launcher, a rollback and an install are no choice.
        s.switch_to(&Slot::Version(v("1.2.0"))).unwrap();
        assert!(s.explicit_choice().is_none());
        s.activate(&Slot::Bundled).unwrap();
        assert!(s.explicit_choice().is_some());
        s.rollback().unwrap();
        assert!(s.explicit_choice().is_none());
        // A choice whose folder is gone does not count.
        s.activate(&Slot::Version(v("1.1.0"))).unwrap();
        s.activate(&Slot::Version(v("1.2.0"))).unwrap();
        assert!(s.explicit_choice().is_some());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn zero_to_keep_still_keeps_the_version_that_the_launcher_falls_back_to() {
        let root = tmp("keep-fallback");
        let s = counting(&root, Some("1.0.0")).with_keep_older(0);
        install_all(&s, &root, &["1.1.0", "1.2.0"]);
        assert_eq!(folders(&s), ["1.2.0"]);
        // 1.2.0 crashes twice in a row; 1.3.0 arrives and crashes too (a short run).
        s.note_run_end(&Slot::Version(v("1.2.0")), false).unwrap();
        install_all(&s, &root, &["1.3.0"]);
        s.note_run_end(&Slot::Version(v("1.3.0")), false).unwrap();
        // 1.2.0 is skipped too, so it stays: the pointer names 1.3.0, and 1.2.0 is skipped...
        s.prune().unwrap();
        assert!(folders(&s).contains(&"1.3.0".to_string()));
        install_all(&s, &root, &["1.4.0"]);
        s.note_run_end(&Slot::Version(v("1.4.0")), false).unwrap();
        s.prune().unwrap();
        // Newest 1.4.0 and 1.3.0 are skipped; 1.2.0 is skipped; no older good version exists
        // in the store: the bundled app is the fallback and nothing else is needed. Every
        // skipped folder down to the first good one stays.
        let f = folders(&s);
        assert!(f.contains(&"1.4.0".to_string()), "{f:?}");
        std::fs::remove_dir_all(&root).unwrap();

        // The case with a good version below the skipped newest ones.
        let root = tmp("keep-fallback-2");
        let s = counting(&root, Some("1.0.0")).with_keep_older(5);
        install_all(&s, &root, &["1.1.0", "1.2.0", "1.3.0"]);
        s.set_keep_older(0);
        s.note_run_end(&Slot::Version(v("1.3.0")), false).unwrap();
        s.note_run_end(&Slot::Version(v("1.2.0")), false).unwrap();
        s.prune().unwrap();
        // 1.3.0 (newest), 1.2.0 (skipped) and 1.1.0 (the first good one) stay.
        assert_eq!(folders(&s), ["1.1.0", "1.2.0", "1.3.0"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_local_build_is_swapped_in_and_never_replaces_the_current_version_in_place() {
        let root = tmp("add-swap");
        let s = counting(&root, Some("1.0.0"));
        let ver = v("1.0.0-local.gabc");
        s.add_local(&ver, &fake_build(&root, "b1", false), None)
            .unwrap();
        // No leftovers of the swap.
        let names: Vec<String> = std::fs::read_dir(s.versions_dir())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [ver.to_string()]);
        // The current version is not replaced in place.
        s.activate(&Slot::Version(ver.clone())).unwrap();
        let e = s
            .add_local(&ver, &fake_build(&root, "b2", false), None)
            .unwrap_err();
        assert!(matches!(e, UpdateError::NotRemovable(_)), "{e}");
        assert_eq!(
            std::fs::read_to_string(s.versions_dir().join(ver.to_string()).join("kadakas-app"))
                .unwrap(),
            "app b1"
        );
        // A swap that stopped after the first rename: recover puts the old folder back.
        let dir = s.versions_dir();
        std::fs::rename(dir.join(ver.to_string()), dir.join(format!(".old-{ver}"))).unwrap();
        assert!(s.list().iter().all(|i| i.version != ver));
        s.recover();
        assert!(s.list().iter().any(|i| i.version == ver));
        // And when the new folder did arrive, the old one is deleted.
        std::fs::create_dir_all(dir.join(format!(".old-{ver}"))).unwrap();
        s.recover();
        assert!(!dir.join(format!(".old-{ver}")).exists());
        assert!(s.list().iter().any(|i| i.version == ver));
        std::fs::remove_dir_all(&root).unwrap();
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
        // Right hash, but not an archive.
        let garbage = ArchiveFile {
            sha256: sha256_hex(&b"garbage"[..]).unwrap(),
            path: bad,
        };
        assert!(matches!(
            s.install(&v("1.1.0"), &garbage, false),
            Err(UpdateError::Archive(_))
        ));
        let missing = ArchiveFile {
            path: root.join("missing.tar.gz"),
            sha256: "0".repeat(64),
        };
        assert!(s.install(&v("1.1.0"), &missing, false).is_err());
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

    #[test]
    fn a_newer_bundled_version_wins_over_an_old_update() {
        let root = tmp("bundled-newer");
        let r = root.join("r");
        let old_installer = Store::new(&r, Some(bundled(&root, "1.0.0")));
        old_installer
            .install(&v("1.1.0"), &archive_file(&root, "1.1.0"), false)
            .unwrap();
        // The user installs Kadakas 1.5.0 over it. The folder of the bundled version changes.
        let new_installer = Store::new(&r, Some(bundled(&root, "1.5.0")));
        let a = new_installer.active().unwrap();
        assert_eq!((a.slot, a.version), (Slot::Bundled, v("1.5.0")));
        // An equal or lower bundled version does not win.
        let same = Store::new(&r, Some(bundled(&root, "1.1.0")));
        assert_eq!(same.active().unwrap().slot, Slot::Version(v("1.1.0")));
        // Updates below the bundled version are refused as downgrades.
        let e = new_installer
            .install(&v("1.2.0"), &archive_file(&root, "1.2.0"), false)
            .unwrap_err();
        assert!(matches!(e, UpdateError::Downgrade { .. }), "{e}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn only_one_instance_changes_the_install_at_a_time() {
        let root = tmp("lock");
        let r = root.join("r");
        let a = Store::new(&r, None);
        let b = Store::new(&r, None).with_lock_wait(std::time::Duration::from_millis(100));
        a.install(&v("1.0.0"), &archive_file(&root, "1.0.0"), false)
            .unwrap();
        // A half-made folder from a crash.
        std::fs::create_dir_all(a.versions_dir().join(".tmp-9.9.9")).unwrap();
        let held = a.lock(false).unwrap().expect("first lock");
        // Recover skips: it must not delete work of the instance that holds the lock.
        b.recover();
        assert!(a.versions_dir().join(".tmp-9.9.9").exists());
        // A non-waiting lock fails; a waiting one gives up with Busy.
        assert!(b.lock(false).unwrap().is_none());
        assert!(matches!(b.lock(true), Err(UpdateError::Busy)));
        drop(held);
        // The lock is free again: recover cleans up, and installs work.
        b.recover();
        assert!(!a.versions_dir().join(".tmp-9.9.9").exists());
        b.install(&v("1.1.0"), &archive_file(&root, "1.1.0"), false)
            .unwrap();
        assert_eq!(a.active().unwrap().version, v("1.1.0"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_waiting_install_goes_on_when_the_lock_is_freed() {
        let root = tmp("lock-wait");
        let r = root.join("r");
        let a = Store::new(&r, None);
        let held = a.lock(false).unwrap().unwrap();
        let archive = archive_file(&root, "1.0.0");
        let r2 = r.clone();
        let t =
            std::thread::spawn(move || Store::new(&r2, None).install(&v("1.0.0"), &archive, false));
        std::thread::sleep(std::time::Duration::from_millis(200));
        drop(held);
        t.join().unwrap().unwrap();
        assert_eq!(a.active().unwrap().version, v("1.0.0"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_existing_folder_is_reused_only_with_the_right_marker() {
        let root = tmp("marker");
        let s = Store::new(root.join("r"), None);
        s.install(&v("1.0.0"), &archive_file(&root, "1.0.0"), false)
            .unwrap();
        s.install(&v("1.1.0"), &archive_file(&root, "1.1.0"), false)
            .unwrap();
        let dir = s.versions_dir().join("1.0.0");
        assert!(dir.join(MARKER).is_file());
        // Somebody changed a file in the folder of the previous version. The marker is the
        // same, so the folder is reused as it is (we trust our own folder), but a wrong or
        // missing marker makes the install unpack again.
        std::fs::write(dir.join("kadakas-app"), b"damaged").unwrap();
        std::fs::remove_file(dir.join(MARKER)).unwrap();
        // Go to 1.2.0 and back to 1.0.0 as an explicit downgrade: the folder is made again.
        s.install(&v("1.0.0"), &archive_file(&root, "1.0.0"), true)
            .unwrap();
        let a = s.active().unwrap();
        assert_eq!(app_text(&a), "app 1.0.0");
        assert!(a.dir.join(MARKER).is_file());
        // Reuse with the right marker: no unpacking, so a missing archive file is no problem.
        s.install(&v("1.1.0"), &archive_file(&root, "1.1.0"), false)
            .unwrap();
        let gone = ArchiveFile {
            path: root.join("gone.tar.gz"),
            sha256: archive_file(&root, "1.0.0").sha256,
        };
        s.install(&v("1.0.0"), &gone, true).unwrap();
        assert_eq!(s.active().unwrap().version, v("1.0.0"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_hash_is_checked_on_the_file_that_is_unpacked() {
        let root = tmp("hash");
        let s = Store::new(root.join("r"), None);
        let mut a = archive_file(&root, "1.0.0");
        a.sha256 = "ab".repeat(32);
        assert!(matches!(
            s.install(&v("1.0.0"), &a, false),
            Err(UpdateError::HashMismatch)
        ));
        assert!(s.active().is_none());
        assert!(!s.versions_dir().join("1.0.0").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn highest_seen_version_is_remembered_per_channel() {
        let root = tmp("seen");
        let s = Store::new(root.join("r"), None);
        assert!(s.seen(Channel::Stable).is_none());
        s.note_seen(Channel::Stable, &v("1.2.0")).unwrap();
        s.note_seen(Channel::Stable, &v("1.1.0")).unwrap();
        s.note_seen(Channel::Beta, &v("2.0.0-beta.1")).unwrap();
        assert_eq!(s.seen(Channel::Stable), Some(v("1.2.0")));
        assert_eq!(s.seen(Channel::Beta), Some(v("2.0.0-beta.1")));
        // It is on disk.
        assert_eq!(
            Store::new(root.join("r"), None).seen(Channel::Stable),
            Some(v("1.2.0"))
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
