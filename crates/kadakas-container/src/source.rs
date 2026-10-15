//! Lazy parts: a way to read an entry of a container by name when it is needed.

use std::sync::RwLock;

use crate::error::ContainerError;
use crate::reader::{Container, RawEntry};

/// A place that holds parts by name. A document keeps a handle to the source of the file it was
/// opened from, and reads a picture or the history from it when a program needs it. The source
/// checks every read (size, CRC-32, hash), so a part is never wrong silently.
pub trait PartSource: Send + Sync + std::fmt::Debug {
    /// The size of the part, uncompressed, if the source has it.
    fn part_size(&self, name: &str) -> Option<u64>;

    /// The bytes of the part, checked.
    fn read_part(&self, name: &str) -> Result<Vec<u8>, ContainerError>;

    /// The first bytes of the part, not checked (see [`Container::read_prefix`]).
    fn read_prefix(&self, name: &str, n: usize) -> Result<Vec<u8>, ContainerError>;

    /// The part as it is stored, for a copy into a new container.
    fn raw_part(&self, name: &str) -> Result<RawEntry, ContainerError>;

    /// Checks that the file under the source is the one that was opened.
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

    fn read_part(&self, name: &str) -> Result<Vec<u8>, ContainerError> {
        self.container().read(name)
    }

    fn read_prefix(&self, name: &str, n: usize) -> Result<Vec<u8>, ContainerError> {
        self.container().read_prefix(name, n)
    }

    fn raw_part(&self, name: &str) -> Result<RawEntry, ContainerError> {
        self.container().raw(name)
    }

    fn check_unchanged(&self) -> Result<(), ContainerError> {
        self.container().verify()
    }

    fn label(&self) -> String {
        self.place().container.medium().label()
    }

    fn manifest_sha256(&self) -> String {
        self.place().container.manifest_sha256()
    }
}
