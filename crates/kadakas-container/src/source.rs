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

type Reopen = Box<dyn Fn() -> Result<Container, ContainerError> + Send + Sync>;

/// A source over a [`Container`]. A source over a file can be refreshed after a save over the
/// file: it opens the new file and keeps working for the parts that the new file holds.
pub struct ContainerSource {
    inner: RwLock<Container>,
    reopen: Option<Reopen>,
    #[cfg(not(target_arch = "wasm32"))]
    path: Option<std::path::PathBuf>,
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
            inner: RwLock::new(container),
            reopen: None,
            #[cfg(not(target_arch = "wasm32"))]
            path: None,
        }
    }

    /// A source that can open its file again with `reopen` (see [`ContainerSource::refresh`]).
    pub fn with_reopen(
        container: Container,
        reopen: impl Fn() -> Result<Container, ContainerError> + Send + Sync + 'static,
    ) -> ContainerSource {
        ContainerSource {
            inner: RwLock::new(container),
            reopen: Some(Box::new(reopen)),
            #[cfg(not(target_arch = "wasm32"))]
            path: None,
        }
    }

    /// A source over a file path (native only). The file is opened again for each read.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_path(
        path: &std::path::Path,
        limits: crate::limits::Limits,
    ) -> Result<ContainerSource, ContainerError> {
        let owned = path.to_path_buf();
        let open = move || -> Result<Container, ContainerError> {
            let medium = crate::medium::FileMedium::open_path(&owned)?;
            Container::open(std::sync::Arc::new(medium), limits)
        };
        let mut source = ContainerSource::with_reopen(open()?, open);
        source.path = Some(path.to_path_buf());
        Ok(source)
    }

    /// The path of the file, if the container is in a file.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }

    /// A copy of the container as it is now.
    pub fn container(&self) -> Container {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Opens the file again and keeps the new container. Call it after the file was replaced by
    /// a save of this program. If the new file cannot be opened, the old container stays.
    pub fn refresh(&self) -> Result<(), ContainerError> {
        let Some(reopen) = &self.reopen else {
            return Ok(());
        };
        let fresh = reopen()?;
        *self.inner.write().unwrap_or_else(|e| e.into_inner()) = fresh;
        Ok(())
    }

    /// True if [`ContainerSource::refresh`] can open the file again.
    pub fn can_refresh(&self) -> bool {
        self.reopen.is_some()
    }
}

impl PartSource for ContainerSource {
    fn part_size(&self, name: &str) -> Option<u64> {
        self.container().entry(name).map(|e| e.size)
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
        self.container().medium().label()
    }

    fn manifest_sha256(&self) -> String {
        self.container().manifest_sha256()
    }
}
