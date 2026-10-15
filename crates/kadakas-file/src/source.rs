//! The open file of a document: where its lazy parts are read from.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use kadakas_container::{Container, ContainerError, ContainerSource, PartSource, hex};
use kadakas_core::blob::{BlobError, BlobSource, HEAD_CHARS, LazyBlob};

use crate::layout::blob_name;

/// A container that a document was read from. It is the [`BlobSource`] of the lazy pictures of
/// the document and the source of its history. It keeps no open file: each read opens the path
/// again and checks the file against the first open (size, time of change), and every part
/// against the hash of the manifest, so a part is never wrong silently.
pub struct FileSource {
    part: ContainerSource,
    /// The pictures that are in use: one weak handle for each hash. A save keeps the pictures that
    /// a handle still uses, also when the document no longer has them (the undo steps hold them).
    live: Mutex<Live>,
}

/// The weak handles of the pictures in use, and the size at which the dead ones are dropped. The
/// size doubles after each drop, so that a document with 65 534 pictures does not scan the list at
/// every picture.
struct Live {
    map: HashMap<String, Weak<LazyBlob>>,
    prune_at: usize,
}

impl Default for Live {
    fn default() -> Self {
        Live {
            map: HashMap::new(),
            prune_at: 4096,
        }
    }
}

impl std::fmt::Debug for FileSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FileSource({})", self.part.label())
    }
}

impl FileSource {
    /// A source over a container in memory (the bytes of the browser's file picker, or a test).
    pub fn from_container(container: Container) -> Arc<FileSource> {
        Arc::new(FileSource {
            part: ContainerSource::new(container),
            live: Mutex::default(),
        })
    }

    /// A source over a container in a file (native only). The file is opened again for each
    /// read, and after a save over it by [`FileSource::refresh`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_path(
        path: &std::path::Path,
        limits: kadakas_container::Limits,
    ) -> Result<Arc<FileSource>, ContainerError> {
        Ok(Arc::new(FileSource {
            part: ContainerSource::from_path(path, limits)?,
            live: Mutex::default(),
        }))
    }

    /// A copy of the container as it is now.
    pub fn container(&self) -> Container {
        self.part.container()
    }

    /// The part source (for the raw copy of an entry).
    pub fn parts(&self) -> &ContainerSource {
        &self.part
    }

    /// What the source is, for messages: the path.
    pub fn label(&self) -> String {
        self.part.label()
    }

    /// The path of the file, if the container is in a file.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn path(&self) -> Option<std::path::PathBuf> {
        self.part.path()
    }

    /// Moves the source to another file, after a save to a new path: the pictures are found by
    /// hash in the new file.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn rebind(&self, path: &std::path::Path) -> Result<(), ContainerError> {
        self.part.rebind(path)
    }

    /// Opens the file again after a save over it, and keeps the new container. The pictures of
    /// the document are found by hash in the new file.
    pub fn refresh(&self) -> Result<(), ContainerError> {
        self.part.refresh()
    }

    /// Checks that the file is the one that was opened.
    pub fn check_unchanged(&self) -> Result<(), ContainerError> {
        self.part.check_unchanged()
    }

    /// The hashes of the pictures that some handle still uses.
    pub fn live_hashes(&self) -> Vec<String> {
        let live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        let mut v: Vec<String> = live
            .map
            .iter()
            .filter(|(_, w)| w.strong_count() > 0)
            .map(|(h, _)| h.clone())
            .collect();
        v.sort();
        v
    }

    fn error(e: ContainerError) -> BlobError {
        BlobError(e.to_string())
    }
}

impl BlobSource for FileSource {
    fn blob_len(&self, hash: &str) -> Option<u64> {
        let container = self.part.container();
        let e = container.entry(&blob_name(hash))?;
        // The name of a blob is the hash of its text. An entry with another content is not it.
        (hex(&e.sha256) == hash).then_some(e.size)
    }

    fn blob_head(&self, hash: &str) -> Result<String, BlobError> {
        let bytes = self
            .part
            .read_prefix(&blob_name(hash), HEAD_CHARS)
            .map_err(FileSource::error)?;
        // The first characters of a base64 text or of JSON are ASCII; anything else is cut at the
        // first byte that is not.
        Ok(bytes
            .iter()
            .take_while(|b| b.is_ascii())
            .map(|b| *b as char)
            .collect())
    }

    fn blob_text(&self, hash: &str) -> Result<Arc<str>, BlobError> {
        let name = blob_name(hash);
        let bytes = self.part.read_part(&name).map_err(FileSource::error)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| BlobError(format!("the entry \"{name}\" is not text")))?;
        Ok(Arc::from(text))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }

    fn check(&self) -> Result<(), BlobError> {
        self.part.check_unchanged().map_err(FileSource::error)
    }

    fn note_handle(&self, blob: &Arc<LazyBlob>) {
        let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        if live.map.len() > live.prune_at {
            live.map.retain(|_, w| w.strong_count() > 0);
            live.prune_at = (live.map.len() * 2).max(4096);
        }
        live.map
            .insert(blob.hash().to_string(), Arc::downgrade(blob));
    }
}
