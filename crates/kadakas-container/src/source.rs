//! Lazy parts: a way to read an entry of a container by name when it is needed.
//!
//! The content is the authority, not the stamp of the file (size, time of change). A part is
//! known by its name and the SHA-256 that the manifest of the opened file gives. When the stamp of
//! the file differs from the one at the open, the source opens the file again and compares the
//! SHA-256 of the manifest: if it is the same, only the stamp is new (a touch, a copy, a backup
//! tool) and the source goes on. If the file really changed, a part is still read when the file at
//! the same path, or the `.bak` that the last save made, holds an entry of this name with the same
//! SHA-256. Only when no file has the part, the read fails, and the error says which part.

use std::sync::RwLock;

use crate::error::ContainerError;
use crate::reader::{Container, RawEntry};

/// A place that holds parts by name. A document keeps a handle to the source of the file it was
/// opened from, and reads a picture or the history from it when a program needs it. The source
/// checks every read (size, CRC-32, hash), so a part is never wrong silently.
pub trait PartSource: Send + Sync + std::fmt::Debug {
    /// The size of the part, uncompressed, if the source has it.
    fn part_size(&self, name: &str) -> Option<u64>;

    /// The SHA-256 of the part, as the manifest of the opened file says.
    fn part_sha256(&self, name: &str) -> Option<[u8; 32]>;

    /// The bytes of the part, checked.
    fn read_part(&self, name: &str) -> Result<Vec<u8>, ContainerError>;

    /// The first bytes of the part, not checked (see [`Container::read_prefix`]).
    fn read_prefix(&self, name: &str, n: usize) -> Result<Vec<u8>, ContainerError>;

    /// The part as it is stored, for a copy into a new container.
    fn raw_part(&self, name: &str) -> Result<RawEntry, ContainerError>;

    /// True if some file can give the part now (the directory is looked at, nothing is read).
    fn has_part(&self, name: &str) -> bool;

    /// Checks that the file under the source has the content that was opened. A new time of
    /// change with the same manifest is fine (the stamp is taken again).
    fn check_unchanged(&self) -> Result<(), ContainerError>;

    /// What the source is, for messages (a path).
    fn label(&self) -> String;

    /// SHA-256 of the manifest of the container as it was opened, as hex.
    fn manifest_sha256(&self) -> String;
}

/// Where a [`ContainerSource`] reads from.
struct Place {
    container: Container,
    /// The file, if the container is in a file (native only).
    #[cfg(not(target_arch = "wasm32"))]
    path: Option<std::path::PathBuf>,
    /// The limits that the file is opened with again.
    #[cfg(not(target_arch = "wasm32"))]
    limits: crate::limits::Limits,
}

/// A source over a [`Container`]. A source over a file can be refreshed after a save over the
/// file, and moved to another file after a save to a new path (`rebind`): the parts are found by
/// name (the hash) in the new file.
pub struct ContainerSource {
    inner: RwLock<Place>,
}

impl std::fmt::Debug for ContainerSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ContainerSource({})", self.label())
    }
}

impl ContainerSource {
    /// A source over a container that is not opened again (memory).
    pub fn new(container: Container) -> ContainerSource {
        ContainerSource {
            inner: RwLock::new(Place {
                container,
                #[cfg(not(target_arch = "wasm32"))]
                path: None,
                #[cfg(not(target_arch = "wasm32"))]
                limits: crate::limits::Limits::default(),
            }),
        }
    }

    fn place(&self) -> std::sync::RwLockReadGuard<'_, Place> {
        self.inner.read().unwrap_or_else(|e| e.into_inner())
    }

    /// A source over a file path (native only). The file is opened again for each read.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_path(
        path: &std::path::Path,
        limits: crate::limits::Limits,
    ) -> Result<ContainerSource, ContainerError> {
        let container = open_file(path, limits)?;
        Ok(ContainerSource {
            inner: RwLock::new(Place {
                container,
                path: Some(path.to_path_buf()),
                limits,
            }),
        })
    }

    /// The path of the file, if the container is in a file.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn path(&self) -> Option<std::path::PathBuf> {
        self.place().path.clone()
    }

    /// A copy of the container as it is now.
    pub fn container(&self) -> Container {
        self.place().container.clone()
    }

    /// Opens the file again and keeps the new container. Call it after the file was replaced by
    /// a save of this program. If the new file cannot be opened, the old container stays.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn refresh(&self) -> Result<(), ContainerError> {
        let (path, limits) = {
            let p = self.place();
            match &p.path {
                Some(path) => (path.clone(), p.limits),
                None => return Ok(()),
            }
        };
        let fresh = open_file(&path, limits)?;
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .container = fresh;
        Ok(())
    }

    /// Moves the source to another file (after a save to a new path). The parts are found by
    /// name in the new file. If the new file cannot be opened, the source stays where it was.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn rebind(&self, path: &std::path::Path) -> Result<(), ContainerError> {
        let limits = self.place().limits;
        let fresh = open_file(path, limits)?;
        let mut p = self.inner.write().unwrap_or_else(|e| e.into_inner());
        p.container = fresh;
        p.path = Some(path.to_path_buf());
        Ok(())
    }

    /// True if the container is in a file that can be opened again.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn can_refresh(&self) -> bool {
        self.place().path.is_some()
    }

    /// Opens the file again and compares the manifest with the one of the container that was
    /// opened. If it is the same, the source takes the new stamp of the file and the new
    /// directory (the layout of the file may differ) and returns `Ok`. If the file is gone or
    /// has another content, it is `Changed`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn reconcile(&self) -> Result<(), ContainerError> {
        let (path, limits, was) = {
            let p = self.place();
            let Some(path) = p.path.clone() else {
                return Ok(());
            };
            (path, p.limits, p.container.manifest_sha256())
        };
        let now = open_file(&path, limits).map_err(|e| match e {
            ContainerError::Changed(m) => ContainerError::Changed(m),
            other => ContainerError::Changed(format!(
                "the file {} changed on disk after it was opened and cannot be read now: {other}. Open it again; nothing from the changed file is used",
                path.display()
            )),
        })?;
        if now.manifest_sha256() != was {
            return Err(ContainerError::Changed(format!(
                "the file {} changed on disk after it was opened (its manifest is another). Open it again; nothing from the changed file is used",
                path.display()
            )));
        }
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .container = now;
        Ok(())
    }

    /// Kept for callers of the old name: the same as [`ContainerSource::reconcile`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn verify_manifest(&self) -> Result<(), ContainerError> {
        self.reconcile()
    }

    /// Without files there is nothing to compare.
    #[cfg(target_arch = "wasm32")]
    pub fn reconcile(&self) -> Result<(), ContainerError> {
        Ok(())
    }

    /// Without files there is nothing to compare.
    #[cfg(target_arch = "wasm32")]
    pub fn verify_manifest(&self) -> Result<(), ContainerError> {
        Ok(())
    }

    /// Without files there is nothing to open again.
    #[cfg(target_arch = "wasm32")]
    pub fn refresh(&self) -> Result<(), ContainerError> {
        Ok(())
    }

    /// Without files there is nothing to open again.
    #[cfg(target_arch = "wasm32")]
    pub fn can_refresh(&self) -> bool {
        false
    }

    /// The other files that can have the same part: the file at the path (it may have been
    /// replaced) and the `.bak` of the last save.
    #[cfg(not(target_arch = "wasm32"))]
    fn alternates(&self) -> Vec<(std::path::PathBuf, crate::limits::Limits)> {
        let p = self.place();
        let Some(path) = p.path.clone() else {
            return Vec::new();
        };
        let mut bak = path.clone().into_os_string();
        bak.push(".bak");
        vec![(path, p.limits), (bak.into(), p.limits)]
    }

    /// Runs `f` on a container that has the entry `name` with this SHA-256. The order: the opened
    /// container; the same file with a new stamp (see [`ContainerSource::reconcile`]); the file at
    /// the path; the `.bak`. The first result is kept if no container works.
    fn with_entry<T>(
        &self,
        name: &str,
        sha: &[u8; 32],
        f: impl Fn(&Container) -> Result<T, ContainerError>,
    ) -> Result<T, ContainerError> {
        let has = |c: &Container| c.entry(name).is_some_and(|e| &e.sha256 == sha);
        let c = self.container();
        let first = if has(&c) {
            match f(&c) {
                Ok(v) => return Ok(v),
                Err(e) => e,
            }
        } else {
            ContainerError::entry(name, "it is not in the container")
        };
        #[cfg(target_arch = "wasm32")]
        {
            Err(first)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.place().path.is_none() {
                return Err(first);
            }
            // The stamp changed, the content did not: take the stamp and read again.
            let mut error = match self.reconcile() {
                Ok(()) => {
                    let c = self.container();
                    if has(&c) {
                        match f(&c) {
                            Ok(v) => return Ok(v),
                            // The file is the one that was opened and the part is damaged.
                            Err(e) => return Err(e),
                        }
                    }
                    first
                }
                Err(changed) => changed,
            };
            for (path, limits) in self.alternates() {
                if let Ok(c) = open_file(&path, limits)
                    && has(&c)
                {
                    match f(&c) {
                        Ok(v) => return Ok(v),
                        Err(e) => {
                            if !matches!(error, ContainerError::Changed(_)) {
                                error = e;
                            }
                        }
                    }
                }
            }
            Err(match error {
                ContainerError::Changed(m) => ContainerError::Changed(format!(
                    "{m} (the part \"{name}\" is not in the file now)"
                )),
                other => other,
            })
        }
    }

    /// True if a file can give the part with this SHA-256 (only the directories are looked at).
    pub fn has_by_content(&self, name: &str, sha: &[u8; 32]) -> bool {
        let has = |c: &Container| c.entry(name).is_some_and(|e| &e.sha256 == sha);
        let c = self.container();
        #[cfg(not(target_arch = "wasm32"))]
        {
            if has(&c) && c.verify().is_ok() {
                return true;
            }
            if self.place().path.is_none() {
                return has(&c);
            }
            if self.reconcile().is_ok() {
                return has(&self.container());
            }
            self.alternates()
                .into_iter()
                .any(|(path, limits)| open_file(&path, limits).is_ok_and(|c| has(&c)))
        }
        #[cfg(target_arch = "wasm32")]
        {
            has(&c)
        }
    }

    /// The bytes of the part with this name and SHA-256, from any file that has it, checked.
    pub fn read_by_content(&self, name: &str, sha: &[u8; 32]) -> Result<Vec<u8>, ContainerError> {
        self.with_entry(name, sha, |c| c.read(name))
    }

    /// The part with this name and SHA-256 as it is stored, from any file that has it.
    pub fn raw_by_content(&self, name: &str, sha: &[u8; 32]) -> Result<RawEntry, ContainerError> {
        self.with_entry(name, sha, |c| c.raw(name))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn open_file(
    path: &std::path::Path,
    limits: crate::limits::Limits,
) -> Result<Container, ContainerError> {
    let medium = crate::medium::FileMedium::open_path(path)?;
    Container::open(std::sync::Arc::new(medium), limits)
}

impl PartSource for ContainerSource {
    fn part_size(&self, name: &str) -> Option<u64> {
        self.place().container.entry(name).map(|e| e.size)
    }

    fn part_sha256(&self, name: &str) -> Option<[u8; 32]> {
        self.place().container.entry(name).map(|e| e.sha256)
    }

    fn read_part(&self, name: &str) -> Result<Vec<u8>, ContainerError> {
        let sha = self
            .part_sha256(name)
            .ok_or_else(|| ContainerError::entry(name, "it is not in the container"))?;
        self.read_by_content(name, &sha)
    }

    fn read_prefix(&self, name: &str, n: usize) -> Result<Vec<u8>, ContainerError> {
        let sha = self
            .part_sha256(name)
            .ok_or_else(|| ContainerError::entry(name, "it is not in the container"))?;
        self.with_entry(name, &sha, |c| c.read_prefix(name, n))
    }

    fn raw_part(&self, name: &str) -> Result<RawEntry, ContainerError> {
        let sha = self
            .part_sha256(name)
            .ok_or_else(|| ContainerError::entry(name, "it is not in the container"))?;
        self.raw_by_content(name, &sha)
    }

    fn has_part(&self, name: &str) -> bool {
        self.part_sha256(name)
            .is_some_and(|sha| self.has_by_content(name, &sha))
    }

    fn check_unchanged(&self) -> Result<(), ContainerError> {
        // The manifest is read again also when the stamp is the same: a copy that keeps the time
        // and the size is found here.
        self.reconcile()
    }

    fn label(&self) -> String {
        self.place().container.medium().label()
    }

    fn manifest_sha256(&self) -> String {
        self.place().container.manifest_sha256()
    }
}
