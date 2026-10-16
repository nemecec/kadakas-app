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
    /// The most bytes of pictures that [`FileSource::rescue_if_changed`] may hold in memory (a
    /// limit of the session).
    rescue_limit: std::sync::atomic::AtomicU64,
    /// How many pictures the last rescue left alone because of the limit.
    rescue_skipped: std::sync::atomic::AtomicUsize,
    /// A number that names this source (it is not an address, and no other source has it).
    id: u64,
    /// How many pictures that only undo could bring back the person was told about (see
    /// [`FileSource::tell_undo_lost`]).
    undo_told: std::sync::atomic::AtomicUsize,
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

/// The default of the limit of [`FileSource::set_rescue_limit`]: 256 MiB.
pub const DEFAULT_RESCUE_BYTES: u64 = 256 * 1024 * 1024;

fn next_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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
            rescue_limit: std::sync::atomic::AtomicU64::new(DEFAULT_RESCUE_BYTES),
            rescue_skipped: Default::default(),
            id: next_id(),
            undo_told: Default::default(),
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
            rescue_limit: std::sync::atomic::AtomicU64::new(DEFAULT_RESCUE_BYTES),
            rescue_skipped: Default::default(),
            id: next_id(),
            undo_told: Default::default(),
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

    /// A number that names this source for the length of the process.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Sets the most bytes of pictures that a rescue may hold in memory (a limit of the session).
    pub fn set_rescue_limit(&self, bytes: u64) {
        self.rescue_limit
            .store(bytes, std::sync::atomic::Ordering::Relaxed);
    }

    /// Looks at the file and, if it is not the file that was opened, reads the pictures that only
    /// an undo step holds into memory, as far as some file (the one at the path, or the `.bak`)
    /// still has them. `in_drawing` gives the hashes of the pictures that the current drawing
    /// uses; those are left alone, because a picture of the drawing that cannot be found is
    /// handled by the refusal that asks the person (`parts_missing`), and reading all of them at
    /// once could take hundreds of megabytes. At most the limit of the session is held (the
    /// pictures over it are counted, and the note to the person says so). It is done at the
    /// first look at the change, because a second save by another program replaces the `.bak`;
    /// the pictures that were not read stay lazy, so a file that did not change costs nothing.
    /// Returns the number of pictures read.
    pub fn rescue_if_changed(
        &self,
        in_drawing: impl FnOnce() -> std::collections::HashSet<String>,
    ) -> usize {
        if self.part.same_file() {
            return 0;
        }
        let drawing = in_drawing();
        let handles: Vec<Arc<LazyBlob>> = {
            let live = self.live.lock().unwrap_or_else(|e| e.into_inner());
            live.map
                .values()
                .flat_map(|ws| ws.iter().filter_map(Weak::upgrade))
                .collect()
        };
        let limit = self.rescue_limit.load(std::sync::atomic::Ordering::Relaxed);
        let mut held: u64 = handles
            .iter()
            .filter(|h| h.is_pinned())
            .map(|h| h.text_len() as u64)
            .sum();
        let (mut n, mut skipped) = (0, 0);
        for h in handles {
            if h.is_pinned() || drawing.contains(h.hash()) || !self.has_blob(h.hash()) {
                continue;
            }
            let len = h.text_len() as u64;
            if held.saturating_add(len) > limit {
                skipped += 1;
                continue;
            }
            if h.pin().is_ok() {
                held += len;
                n += 1;
            }
        }
        self.rescue_skipped
            .store(skipped, std::sync::atomic::Ordering::Relaxed);
        n
    }

    /// The words for the person when a save left out `lost` pictures that only an undo step held
    /// (`Written::undo_pictures_lost`): `Some` the first time, and again only when the number
    /// changes. A save that lost none resets it. The words say how many could not be kept in
    /// memory because of the limit of the session, if any.
    pub fn tell_undo_lost(&self, lost: usize) -> Option<String> {
        let before = self
            .undo_told
            .swap(lost, std::sync::atomic::Ordering::Relaxed);
        if lost == 0 || before == lost {
            return None;
        }
        let mut text = undo_lost_text(lost);
        let skipped = self
            .rescue_skipped
            .load(std::sync::atomic::Ordering::Relaxed);
        if skipped > 0 {
            let mib = self.rescue_limit.load(std::sync::atomic::Ordering::Relaxed) / (1024 * 1024);
            text.push_str(&format!(
                ". {skipped} of them could not be kept in memory when the file changed, because of the limit of {mib} MiB for that (the limit of the session can be raised)"
            ));
        }
        Some(text)
    }

    /// The text of a picture that was read into memory by [`FileSource::rescue_if_changed`].
    pub fn pinned_text(&self, hash: &str) -> Option<Arc<str>> {
        let live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        live.map
            .get(hash)?
            .iter()
            .filter_map(Weak::upgrade)
            .find_map(|h| h.pinned_text())
    }

    /// True if some file (the one at the path, or its `.bak`) can give the blob now.
    pub fn has_blob(&self, hash: &str) -> bool {
        parse_hash(hash).is_some_and(|sha| self.part.has_by_content(&blob_name(hash), &sha))
    }

    /// True if the file at the path has the content that was opened (its manifest is read again).
    /// A save asks it once for each source.
    pub fn same_file(&self) -> bool {
        self.part.same_file()
    }

    /// True if the directory of the opened file has the blob (nothing is looked at on disk).
    pub fn lists_blob(&self, hash: &str) -> bool {
        parse_hash(hash).is_some_and(|sha| {
            self.part
                .container()
                .entry(&blob_name(hash))
                .is_some_and(|e| e.sha256 == sha)
        })
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

/// What the person is told when pictures that only undo could bring back are in no file any more.
pub fn undo_lost_text(lost: usize) -> String {
    format!(
        "{lost} picture(s) that only undo could bring back are no longer in any file; undo will show them empty"
    )
}

impl BlobSource for FileSource {
    fn blob_len(&self, hash: &str) -> Option<u64> {
        // The name of a blob is the hash of its text. An entry with another content is not it. The
        // file that was opened first, then the file at the path and the .bak (a picture that
        // came back there).
        let sha = parse_hash(hash)?;
        self.part.size_by_content(&blob_name(hash), &sha)
    }

    fn is_import_base(&self, hash: &str) -> Option<bool> {
        // Every file of this format marks the blobs of its import bases (the writer always does),
        // so a blob that is not marked is not one, also in a file that marks none: the open
        // estimate counts the marked blobs, and nothing else is parsed as a base.
        let c = self.part.container();
        Some(
            c.entry(&blob_name(hash))
                .is_some_and(|e| e.role.as_deref() == Some(crate::layout::ROLE_IMPORT_BASE)),
        )
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
