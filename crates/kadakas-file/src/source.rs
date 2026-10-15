//! The open file of a document: where its lazy parts are read from.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use kadakas_container::{Container, ContainerError, ContainerSource, PartSource, parse_hash};
use kadakas_core::blob::{BlobError, BlobSource, HEAD_CHARS, LazyBlob};

use crate::layout::blob_name;

/// A container that a document was read from. It is the [`BlobSource`] of the lazy pictures of
/// the document and the source of its history. It keeps no open file: each read opens the path
/// again. A part is known by its name and its SHA-256, not by the stamp of the file: if the file
/// was touched or copied the source goes on, and if it was replaced the part is looked for in the
/// file at the path and in the `.bak` of the last save (see `kadakas_container::ContainerSource`).
pub struct FileSource {
    part: ContainerSource,
    /// The pictures that are in use: the weak handles for each hash. A save keeps the pictures
    /// that a handle still uses, also when the document no longer has them (the undo steps hold
    /// them).
    live: Mutex<Live>,
}

/// The weak handles of the pictures in use, and the size at which the dead ones are dropped. The
/// size doubles after each drop, so that a document with 250 000 pictures does not scan the list
/// at every picture. All the handles of a hash are kept (a handle that is made again from the same
/// file, for example by the read of the anchor of the history, does not hide the older ones).
struct Live {
    map: HashMap<String, Vec<Weak<LazyBlob>>>,
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

    /// A copy of the container as it was opened (or as it is after a refresh).
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

    /// Checks that the file has the content that was opened. A new stamp with the same manifest
    /// is fine; another content is an error.
    pub fn check_unchanged(&self) -> Result<(), ContainerError> {
        self.part.check_unchanged()
    }

    /// True if some file (the one at the path, or its `.bak`) can give the blob now.
    pub fn has_blob(&self, hash: &str) -> bool {
        parse_hash(hash).is_some_and(|sha| self.part.has_by_content(&blob_name(hash), &sha))
    }

    /// True if some file can give the entry that the opened file has under this name.
    pub fn has_entry(&self, name: &str) -> bool {
        self.part.has_part(name)
    }

    /// A container that has the entry `name` as the opened file has it: for reading the drawing
    /// as a stream.
    pub fn container_with_entry(&self, name: &str) -> Result<Container, ContainerError> {
        let sha = self
            .part
            .part_sha256(name)
            .ok_or_else(|| ContainerError::Entry {
                name: name.to_string(),
                reason: "it is not in the container".into(),
            })?;
        self.part.container_with(name, &sha)
    }

    /// The hashes of the pictures that some handle still uses.
    pub fn live_hashes(&self) -> Vec<String> {
        let live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        let mut v: Vec<String> = live
            .map
            .iter()
            .filter(|(_, ws)| ws.iter().any(|w| w.strong_count() > 0))
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
        (kadakas_container::hex(&e.sha256) == hash).then_some(e.size)
    }

    fn blob_head(&self, hash: &str) -> Result<String, BlobError> {
        let name = blob_name(hash);
        let sha = parse_hash(hash).ok_or_else(|| BlobError("not a hash".into()))?;
        let bytes = self
            .part
            .read_prefix_by_content(&name, &sha, HEAD_CHARS)
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
        // The hash is the SHA-256 of the text, so the entry is found and checked by it.
        let sha = parse_hash(hash).ok_or_else(|| BlobError("not a hash".into()))?;
        let bytes = self
            .part
            .read_by_content(&name, &sha)
            .map_err(FileSource::error)?;
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

    fn check_blob(&self, hash: &str) -> Result<(), BlobError> {
        if self.has_blob(hash) {
            return Ok(());
        }
        let short: String = hash.chars().take(8).collect();
        Err(BlobError(format!(
            "the file {} changed on disk after it was opened, and the picture {short} is not in it now",
            self.part.label()
        )))
    }

    fn note_handle(&self, blob: &Arc<LazyBlob>) {
        let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        if live.map.len() > live.prune_at {
            live.map.retain(|_, ws| {
                ws.retain(|w| w.strong_count() > 0);
                !ws.is_empty()
            });
            live.prune_at = (live.map.len() * 2).max(4096);
        }
        let ws = live.map.entry(blob.hash().to_string()).or_default();
        if ws.len() >= 8 {
            ws.retain(|w| w.strong_count() > 0);
        }
        ws.push(Arc::downgrade(blob));
    }
}
