//! Big texts that can stay in the file (DOC-21): pictures of a document, by hash.
//!
//! A picture of an underlay, the file of an image resource and the summaries of a DXF import base
//! are the big values of a document. In a container file (`kadakas-file`, DOC-20) each one is an
//! entry `blobs/<sha256>`, and `document.json` holds the reference `{"$blob": "<sha256>"}` in its
//! place. These are the same blobs and the same hashes as in collaboration
//! (`kadakas_props::blob`): the blob is the text of the value (the base64 text of a picture), and
//! the hash is the SHA-256 of that text.
//!
//! When a container is read, a reference becomes a [`LazyBlob`]: the hash, the size and the first
//! bytes, and a handle to the [`BlobSource`] (the open file). The text is read only when a program
//! uses it ([`LazyBlob::text`]), checked against the hash, and it is not kept: the last reader that
//! drops it frees the memory. A document that is saved to a container writes a reference for each
//! blob of at least [`MIN_BLOB_TEXT`] bytes; a blob that came from the file is copied from the old
//! file without decoding it (that is the work of `kadakas-file`).
//!
//! The two scopes below carry the context of a read and a write to the (de)serialization code of
//! the picture types. They are scoped values of the current thread (set for the time of one call
//! and restored after it, also on a panic), because serde has no way to pass a context. Outside of
//! a scope the types read and write the plain text: a lazy blob is read from its file and written
//! inline, so `document.get`, collaboration and the JSON form keep working without any knowledge
//! of the container.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The key of a reference: `{"$blob": "<sha256 hex>"}`.
pub const REF_KEY: &str = "$blob";
/// The smallest text that becomes a blob (the same as in collaboration, `MIN_BLOB_BYTES`).
pub const MIN_BLOB_TEXT: usize = 4 * 1024;
/// The largest text of a blob that a file may hold. An image resource is at most 8 MiB, which is
/// 11.2 MiB of base64 text, and a part of an import base is at most 8 MiB of JSON. An underlay
/// has a PNG with at most 4096 x 4096 pixels (`MAX_SIDE_PX`); the worst case is a picture that does
/// not compress: 4096 x 4096 x 4 bytes (8 bit RGBA) is 64 MiB of PNG data, which is 89.6 MB of
/// base64 text (measured in `docs/doc-load-benchmark.md`). So the bound is 128 MiB. A save refuses
/// a bigger text before it writes anything (a file that Kadakas writes always opens), and a read
/// refuses a bigger entry when the reference is read, so a hostile file cannot make a read
/// allocate more.
pub const MAX_BLOB_TEXT: usize = 128 * 1024 * 1024;
/// The largest text of the summaries of an import base (a blob that `recs` refers to): 8 MiB of
/// JSON, the bound of the resource. A reference to a bigger blob is refused.
pub const MAX_BASE_PART_BYTES: usize = 8 * 1024 * 1024;
/// How many characters of the start of a blob a lazy handle keeps (to look at the type of a
/// picture without reading it).
pub const HEAD_CHARS: usize = 8;

/// A blob that could not be read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct BlobError(pub String);

/// Where the blobs of a file are. The container code of `kadakas-file` implements it over the
/// open file. Every read is checked against the hash.
pub trait BlobSource: Send + Sync + std::fmt::Debug {
    /// The length of the text of the blob, if the file has it.
    fn blob_len(&self, hash: &str) -> Option<u64>;
    /// The first [`HEAD_CHARS`] characters of the text (fewer if it is shorter).
    fn blob_head(&self, hash: &str) -> Result<String, BlobError>;
    /// Tells whether the blob is marked as the summaries of an import base: `Some(false)` if it is
    /// not marked, `None` if the source cannot say (a source in memory, a test), where the bound
    /// of the size is all that holds. The estimate of the memory of an open counts the marked
    /// blobs, so a blob that is not marked is not parsed as a base.
    fn is_import_base(&self, _hash: &str) -> Option<bool> {
        None
    }
    /// The text of the blob. It matches the hash, or this is an error.
    fn blob_text(&self, hash: &str) -> Result<Arc<str>, BlobError>;
    /// For a program that knows the kind of the source (the copy of a blob into a new file).
    fn as_any(&self) -> &dyn Any;
    /// The same, for a shared pointer: `Arc::downcast` gives the concrete source back.
    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
    /// Checks that the file under the source is still the one that was opened. A host calls it
    /// before it works out the state of a document for the versions: a picture that cannot be read
    /// would otherwise look like a thing that is not there. The default has nothing to check.
    fn check(&self) -> Result<(), BlobError> {
        Ok(())
    }
    /// Checks that the blob with this hash can be read now. The default asks [`BlobSource::check`]
    /// (the whole file is as it was opened). A source that finds a blob by its hash in another
    /// file overrides it: a blob can be there when the file is not the one that was opened.
    fn check_blob(&self, _hash: &str) -> Result<(), BlobError> {
        self.check()
    }
    /// Called for every handle that is made for this source while a document is read. A source
    /// can keep a weak pointer to know which pictures are still in use (the undo steps hold
    /// pictures that the document no longer has). The default does nothing.
    fn note_handle(&self, _blob: &Arc<LazyBlob>) {}
}

/// A blob that stays in the file until it is used.
pub struct LazyBlob {
    hash: String,
    len: usize,
    head: String,
    source: Arc<dyn BlobSource>,
    text: Mutex<Option<Weak<str>>>,
    bytes: Mutex<Weak<Vec<u8>>>,
    /// The text held in memory (see [`LazyBlob::pin`]), for a picture whose file went away.
    pinned: Mutex<Option<Arc<str>>>,
}

impl std::fmt::Debug for LazyBlob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LazyBlob({}, {} chars)", &self.hash[..8], self.len)
    }
}

impl LazyBlob {
    /// A handle for the blob with this hash. Fails if the source does not have it.
    pub fn new(source: Arc<dyn BlobSource>, hash: &str) -> Result<LazyBlob, BlobError> {
        let len = source.blob_len(hash).ok_or_else(|| {
            BlobError(format!(
                "the file refers to the picture {hash}, which is not in the file"
            ))
        })?;
        let len = usize::try_from(len)
            .ok()
            .filter(|n| *n <= MAX_BLOB_TEXT)
            .ok_or_else(|| {
                BlobError(format!(
                    "the picture {hash} has {len} bytes, more than the {MAX_BLOB_TEXT} that a picture may have"
                ))
            })?;
        let head = source.blob_head(hash)?;
        Ok(LazyBlob {
            hash: hash.to_string(),
            len,
            head,
            source,
            text: Mutex::new(None),
            bytes: Mutex::new(Weak::new()),
            pinned: Mutex::new(None),
        })
    }

    /// Reads the text now and keeps it in memory for the life of the handle. A host does this for
    /// a picture that is still readable when the file it comes from is found changed, because a
    /// later save over the file could take the last copy away.
    pub fn pin(&self) -> Result<(), BlobError> {
        if self.is_pinned() {
            return Ok(());
        }
        let t = self.text()?;
        *self.pinned.lock().unwrap_or_else(|e| e.into_inner()) = Some(t);
        Ok(())
    }

    /// True if the text is held in memory by [`LazyBlob::pin`].
    pub fn is_pinned(&self) -> bool {
        self.pinned
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// The text held by [`LazyBlob::pin`].
    pub fn pinned_text(&self) -> Option<Arc<str>> {
        self.pinned
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// The SHA-256 of the text, as 64 hex digits.
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// The length of the text in bytes.
    pub fn text_len(&self) -> usize {
        self.len
    }

    /// The first characters of the text.
    pub fn head(&self) -> &str {
        &self.head
    }

    /// The source that holds the blob.
    pub fn source(&self) -> &Arc<dyn BlobSource> {
        &self.source
    }

    /// The text. Read from the file, checked against the hash. While some caller holds the
    /// result, another call gives the same memory.
    pub fn text(&self) -> Result<Arc<str>, BlobError> {
        if let Some(t) = self.pinned_text() {
            return Ok(t);
        }
        let mut cache = self.text.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = cache.as_ref().and_then(Weak::upgrade) {
            return Ok(t);
        }
        let t = self.source.blob_text(&self.hash)?;
        *cache = Some(Arc::downgrade(&t));
        Ok(t)
    }

    /// The bytes that the base64 text stands for (for a picture). Shared like [`LazyBlob::text`].
    pub fn bytes(&self) -> Result<Arc<Vec<u8>>, BlobError> {
        {
            let cache = self.bytes.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(b) = cache.upgrade() {
                return Ok(b);
            }
        }
        let text = self.text()?;
        let b =
            Arc::new(crate::underlay::base64_decode(&text).ok_or_else(|| {
                BlobError(format!("the picture {} is not valid base64", self.hash))
            })?);
        *self.bytes.lock().unwrap_or_else(|e| e.into_inner()) = Arc::downgrade(&b);
        Ok(b)
    }
}

/// SHA-256 of a text as 64 lowercase hex digits.
pub fn text_hash(text: &str) -> String {
    use std::fmt::Write;
    Sha256::digest(text.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// True for 64 lowercase hex digits.
pub fn is_hash(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

// ---------------------------------------------------------------------------------------------
// The scopes.

/// How the references of a read are resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadKind {
    /// The pictures stay in the file and the source keeps a weak pointer to each handle
    /// ([`BlobSource::note_handle`]).
    Lazy,
    /// The same, but the handles are not noted: for a document that is read to be looked at and
    /// dropped (the anchor of the history), so that it does not stand for a picture in use.
    LazyUntracked,
    /// Every picture is read into memory now (a document that is imported from, or kept after its
    /// file is gone).
    Eager,
}

/// What a read has resolved so far. One in-memory copy for each hash: the same picture met again
/// (a copy and paste of an object, 50 underlays of one map) is the same handle, the same text, the
/// same bytes. The maps live for one load.
#[derive(Default)]
struct Memo {
    lazy: RefCell<HashMap<String, Arc<LazyBlob>>>,
    texts: RefCell<HashMap<String, Arc<str>>>,
    pngs: RefCell<HashMap<String, crate::underlay::PngData>>,
    blobs: RefCell<HashMap<String, BlobText>>,
    bases: RefCell<HashMap<String, Arc<serde_json::Value>>>,
    /// The bytes that this load may still read from the file.
    budget: std::cell::Cell<u64>,
}

/// The source of a read and how it is used.
#[derive(Clone)]
struct LoadScope {
    /// `None` for a plain read (no file): only the merge of equal pictures is active.
    source: Option<Arc<dyn BlobSource>>,
    kind: LoadKind,
    memo: Rc<Memo>,
}

thread_local! {
    static LOAD: RefCell<Option<LoadScope>> = const { RefCell::new(None) };
    static SAVE: RefCell<Option<SaveRefs>> = const { RefCell::new(None) };
}

/// What a save in the container form knows: write a reference for each blob.
#[derive(Default)]
pub struct SaveRefs {
    /// Leave out the field `history` (a container holds the history in its own entries).
    pub skip_history: bool,
    /// The hash of the text of each import base, by the address of its shared body (the text is
    /// the compact JSON of `recs`). Made by the caller before the write.
    pub import_bases: HashMap<usize, String>,
}

struct Restore<T: 'static>(
    &'static std::thread::LocalKey<RefCell<Option<T>>>,
    Option<T>,
);

impl<T: 'static> Drop for Restore<T> {
    fn drop(&mut self) {
        let old = self.1.take();
        self.0.with(|c| *c.borrow_mut() = old);
    }
}

/// Runs `f` with a source for the references that the pictures meet while they are read. The
/// previous scope comes back after `f`, also when `f` panics. The budget is the number of bytes
/// that the read may take from the file (the sum of the sizes that the file declares).
pub fn with_load_budget<R>(
    source: Arc<dyn BlobSource>,
    kind: LoadKind,
    budget: u64,
    f: impl FnOnce() -> R,
) -> R {
    let memo = Memo::default();
    memo.budget.set(budget);
    let scope = LoadScope {
        source: Some(source),
        kind,
        memo: Rc::new(memo),
    };
    let old = LOAD.with(|c| c.borrow_mut().replace(scope));
    let _restore = Restore(&LOAD, old);
    f()
}

/// [`with_load_budget`] without a bound on the bytes.
pub fn with_load_source<R>(
    source: Arc<dyn BlobSource>,
    kind: LoadKind,
    f: impl FnOnce() -> R,
) -> R {
    with_load_budget(source, kind, u64::MAX, f)
}

/// Runs `f` so that equal pictures of a plain read (JSON text, a paste, a collaboration snapshot)
/// are merged: a picture text that was met before, by its SHA-256, is the same memory. Inside
/// [`with_load_budget`] it does nothing (that scope merges already).
pub fn with_intern_scope<R>(f: impl FnOnce() -> R) -> R {
    if LOAD.with(|c| c.borrow().is_some()) {
        return f();
    }
    let scope = LoadScope {
        source: None,
        kind: LoadKind::Eager,
        memo: Rc::new(Memo::default()),
    };
    let old = LOAD.with(|c| c.borrow_mut().replace(scope));
    let _restore = Restore(&LOAD, old);
    f()
}

/// Runs `f` so that every picture of at least [`MIN_BLOB_TEXT`] bytes is written as a reference.
pub fn with_blob_refs<R>(refs: SaveRefs, f: impl FnOnce() -> R) -> R {
    let old = SAVE.with(|c| c.borrow_mut().replace(refs));
    let _restore = Restore(&SAVE, old);
    f()
}

/// True inside [`with_blob_refs`].
pub fn writing_refs() -> bool {
    SAVE.with(|c| c.borrow().is_some())
}

/// True inside [`with_blob_refs`] when the history is not part of `document.json`.
pub(crate) fn history_hidden() -> bool {
    SAVE.with(|c| c.borrow().as_ref().is_some_and(|r| r.skip_history))
}

/// The hash of an import base body that [`with_blob_refs`] knows.
pub(crate) fn import_base_ref(ptr: usize) -> Option<String> {
    SAVE.with(|c| c.borrow().as_ref()?.import_bases.get(&ptr).cloned())
}

/// The scope of a read.
fn load_scope() -> Option<LoadScope> {
    LOAD.with(|c| c.borrow().clone())
}

const NOT_A_CONTAINER: &str = "the text refers to a picture by hash ({\"$blob\": ...}), but it is not a container file, so the picture is not here";

fn ref_scope(hash: &str) -> Result<(LoadScope, Arc<dyn BlobSource>), String> {
    if !is_hash(hash) {
        return Err("a picture reference needs 64 lowercase hex digits".into());
    }
    let scope = load_scope().ok_or_else(|| NOT_A_CONTAINER.to_string())?;
    let source = scope
        .source
        .clone()
        .ok_or_else(|| NOT_A_CONTAINER.to_string())?;
    Ok((scope, source))
}

impl LoadScope {
    /// Takes `len` bytes from the budget of the read, or fails.
    fn charge(&self, len: usize, hash: &str) -> Result<(), String> {
        let left = self.memo.budget.get();
        let len = len as u64;
        if len > left {
            return Err(format!(
                "the pictures and texts that the file refers to (at {hash}) are bigger together than the file says; the file is damaged or was made to confuse a reader"
            ));
        }
        self.memo.budget.set(left - len);
        Ok(())
    }
}

/// The text of a reference, for a value that is read eagerly (the summaries of an import base).
/// The bound of a blob and the hash are checked like for a picture, and the bytes are charged to
/// the budget of the read before they are read.
pub(crate) fn read_ref_text(hash: &str) -> Result<Arc<str>, String> {
    let (scope, source) = ref_scope(hash)?;
    if source.is_import_base(hash) == Some(false) {
        return Err(format!(
            "the summaries of an import base refer to the blob {hash}, which the file does not mark as such"
        ));
    }
    let blob = LazyBlob::new(source, hash).map_err(|e| e.to_string())?;
    if blob.text_len() > MAX_BASE_PART_BYTES {
        return Err(format!(
            "the summaries of an import base are in the blob {hash}, which has {} bytes, more than the {MAX_BASE_PART_BYTES} that they may have",
            blob.text_len()
        ));
    }
    scope.charge(blob.text_len(), hash)?;
    blob.text().map_err(|e| e.to_string())
}

/// What a reference stands for after a read.
pub(crate) enum Resolved {
    /// The picture stays in the file.
    Lazy(Arc<LazyBlob>),
    /// The picture was read (an eager read), with its hash.
    Text(String, Arc<str>),
}

/// Resolves a reference met while reading. A hash that was resolved before in this read gives the
/// same handle or the same text.
pub(crate) fn resolve_ref(hash: &str) -> Result<Resolved, String> {
    let (scope, source) = ref_scope(hash)?;
    if scope.kind == LoadKind::Eager {
        if let Some(t) = scope.memo.texts.borrow().get(hash) {
            return Ok(Resolved::Text(hash.to_string(), t.clone()));
        }
        let blob = LazyBlob::new(source, hash).map_err(|e| e.to_string())?;
        scope.charge(blob.text_len(), hash)?;
        let text = blob.text().map_err(|e| e.to_string())?;
        scope
            .memo
            .texts
            .borrow_mut()
            .insert(hash.to_string(), text.clone());
        return Ok(Resolved::Text(hash.to_string(), text));
    }
    if let Some(l) = scope.memo.lazy.borrow().get(hash) {
        return Ok(Resolved::Lazy(l.clone()));
    }
    let blob = Arc::new(LazyBlob::new(source.clone(), hash).map_err(|e| e.to_string())?);
    if scope.kind == LoadKind::Lazy {
        source.note_handle(&blob);
    }
    scope
        .memo
        .lazy
        .borrow_mut()
        .insert(hash.to_string(), blob.clone());
    Ok(Resolved::Lazy(blob))
}

/// The picture for a text that was read from JSON: the same text gives the same bytes. `decode`
/// makes the picture from the text when it was not met before. Without a scope, or for a text
/// under [`MIN_BLOB_TEXT`], it is just `decode`.
pub(crate) fn intern_png(
    text: &str,
    decode: impl FnOnce() -> Option<crate::underlay::PngData>,
) -> Option<crate::underlay::PngData> {
    let Some(scope) = load_scope().filter(|_| text.len() >= MIN_BLOB_TEXT) else {
        return decode();
    };
    let hash = text_hash(text);
    if let Some(p) = scope.memo.pngs.borrow().get(&hash) {
        return Some(p.clone());
    }
    let p = decode()?;
    scope.memo.pngs.borrow_mut().insert(hash, p.clone());
    Some(p)
}

/// The picture for bytes that an eager read decoded from the blob with this hash.
pub(crate) fn intern_png_of_hash(
    hash: &str,
    decode: impl FnOnce() -> Option<crate::underlay::PngData>,
) -> Option<crate::underlay::PngData> {
    let Some(scope) = load_scope() else {
        return decode();
    };
    if let Some(p) = scope.memo.pngs.borrow().get(hash) {
        return Some(p.clone());
    }
    let p = decode()?;
    scope
        .memo
        .pngs
        .borrow_mut()
        .insert(hash.to_string(), p.clone());
    Some(p)
}

/// The blob text for a text that was read from JSON: the same text is the same memory.
fn intern_blob_text(text: &str) -> BlobText {
    let Some(scope) = load_scope().filter(|_| text.len() >= MIN_BLOB_TEXT) else {
        return BlobText::from(text);
    };
    let hash = text_hash(text);
    if let Some(t) = scope.memo.blobs.borrow().get(&hash) {
        return t.clone();
    }
    let t = BlobText::with_hash(Arc::from(text), hash.clone());
    scope.memo.blobs.borrow_mut().insert(hash, t.clone());
    t
}

/// The text of the blob with this hash, if this read has read it already (an eager read). A reader
/// of the history uses it, so the picture that the drawing and the history share is one copy.
pub fn loaded_text(hash: &str) -> Option<Arc<str>> {
    load_scope()?.memo.texts.borrow().get(hash).cloned()
}

/// Tells the read that the text of the blob with this hash is in memory (read by the caller), so
/// the next user of the hash gets the same memory.
pub fn remember_text(hash: &str, text: &Arc<str>) {
    if let Some(scope) = load_scope() {
        scope
            .memo
            .texts
            .borrow_mut()
            .entry(hash.to_string())
            .or_insert_with(|| text.clone());
    }
}

/// An import base that was resolved before in this read, by its key.
pub(crate) fn base_get(key: &str) -> Option<Arc<serde_json::Value>> {
    load_scope()?.memo.bases.borrow().get(key).cloned()
}

/// Remembers an import base for the rest of this read.
pub(crate) fn base_put(key: String, base: Arc<serde_json::Value>) {
    if let Some(scope) = load_scope() {
        scope.memo.bases.borrow_mut().insert(key, base);
    }
}

/// Reads a reference value: `{"$blob": "<hash>"}` as a visitor of a map.
pub(crate) struct RefVisitor<F, G>(pub F, pub G);

impl<'de, T, F: FnOnce(Resolved) -> Result<T, String>, G: FnOnce(&str) -> T> serde::de::Visitor<'de>
    for RefVisitor<F, G>
{
    type Value = T;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("base64 text or a {\"$blob\": hash} reference")
    }

    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<T, E> {
        Ok((self.1)(text))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<T, A::Error> {
        use serde::de::Error;
        let key: Option<String> = map.next_key()?;
        if key.as_deref() != Some(REF_KEY) {
            return Err(A::Error::custom("a picture is a text or {\"$blob\": hash}"));
        }
        let hash: String = map.next_value()?;
        if map.next_key::<String>()?.is_some() {
            return Err(A::Error::custom(
                "a picture reference has only the key $blob",
            ));
        }
        let resolved = resolve_ref(&hash).map_err(A::Error::custom)?;
        (self.0)(resolved).map_err(A::Error::custom)
    }
}

/// Writes `{"$blob": hash}`.
pub(crate) fn serialize_ref<S: serde::Serializer>(hash: &str, s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut m = s.serialize_map(Some(1))?;
    m.serialize_entry(REF_KEY, hash)?;
    m.end()
}

// ---------------------------------------------------------------------------------------------
// BlobText.

/// A big text that is in memory or in the file: the base64 file of a picture. A clone costs a
/// pointer. In memory the text is shared (the undo steps hold the same text).
#[derive(Clone)]
pub struct BlobText(Repr);

#[derive(Clone)]
enum Repr {
    Mem {
        text: Arc<str>,
        /// The hash of the text, made when it is first asked for.
        hash: Arc<OnceLock<String>>,
    },
    Lazy(Arc<LazyBlob>),
}

impl Default for BlobText {
    fn default() -> Self {
        BlobText::from("")
    }
}

impl std::fmt::Debug for BlobText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Repr::Mem { text, .. } => write!(f, "BlobText({} chars)", text.len()),
            Repr::Lazy(l) => write!(f, "BlobText({l:?})"),
        }
    }
}

impl BlobText {
    /// The text, shared. Reads it from the file if it is lazy. An error says that the file could
    /// not be read (it changed, a part is damaged).
    pub fn text(&self) -> Result<Arc<str>, BlobError> {
        match &self.0 {
            Repr::Mem { text, .. } => Ok(text.clone()),
            Repr::Lazy(l) => l.text(),
        }
    }

    /// The text if it is in memory. `None` for a blob in the file.
    pub fn mem_text(&self) -> Option<&Arc<str>> {
        match &self.0 {
            Repr::Mem { text, .. } => Some(text),
            Repr::Lazy(_) => None,
        }
    }

    /// The length of the text in bytes (known without reading a lazy blob).
    pub fn len(&self) -> usize {
        match &self.0 {
            Repr::Mem { text, .. } => text.len(),
            Repr::Lazy(l) => l.text_len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The bytes that the text holds in memory: 0 for a text that stays in the file.
    pub fn resident_len(&self) -> usize {
        self.mem_text().map_or(0, |t| t.len())
    }

    /// True if the text is in the file and not in memory.
    pub fn is_lazy(&self) -> bool {
        matches!(self.0, Repr::Lazy(_))
    }

    /// The lazy handle, if the text is in the file.
    pub fn lazy(&self) -> Option<&Arc<LazyBlob>> {
        match &self.0 {
            Repr::Lazy(l) => Some(l),
            Repr::Mem { .. } => None,
        }
    }

    /// The SHA-256 of the text (the name of its blob). For a text in memory it is made once.
    pub fn hash(&self) -> String {
        match &self.0 {
            Repr::Mem { text, hash } => hash.get_or_init(|| text_hash(text)).clone(),
            Repr::Lazy(l) => l.hash().to_string(),
        }
    }

    /// The first [`HEAD_CHARS`] characters.
    pub fn head(&self) -> String {
        match &self.0 {
            Repr::Mem { text, .. } => text.chars().take(HEAD_CHARS).collect(),
            Repr::Lazy(l) => l.head().to_string(),
        }
    }

    /// True if the text is written as a reference by a save in the container form.
    pub fn is_blob(&self) -> bool {
        self.len() >= MIN_BLOB_TEXT
    }
}

impl BlobText {
    /// A text in memory whose hash is known (it was made from the text already).
    pub(crate) fn with_hash(text: Arc<str>, hash: String) -> BlobText {
        let cell = OnceLock::new();
        let _ = cell.set(hash);
        BlobText(Repr::Mem {
            text,
            hash: Arc::new(cell),
        })
    }
}

impl From<String> for BlobText {
    fn from(s: String) -> Self {
        BlobText(Repr::Mem {
            text: Arc::from(s),
            hash: Arc::default(),
        })
    }
}

impl From<&str> for BlobText {
    fn from(s: &str) -> Self {
        BlobText(Repr::Mem {
            text: Arc::from(s),
            hash: Arc::default(),
        })
    }
}

impl From<Arc<str>> for BlobText {
    fn from(text: Arc<str>) -> Self {
        BlobText(Repr::Mem {
            text,
            hash: Arc::default(),
        })
    }
}

impl From<Arc<LazyBlob>> for BlobText {
    fn from(l: Arc<LazyBlob>) -> Self {
        BlobText(Repr::Lazy(l))
    }
}

impl PartialEq for BlobText {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Repr::Mem { text: a, .. }, Repr::Mem { text: b, .. }) => Arc::ptr_eq(a, b) || a == b,
            (Repr::Lazy(a), Repr::Lazy(b)) => Arc::ptr_eq(a, b) || a.hash() == b.hash(),
            // A text in memory and one in the file are equal when their hashes are.
            _ => self.len() == other.len() && self.hash() == other.hash(),
        }
    }
}

impl Eq for BlobText {}

impl Serialize for BlobText {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if writing_refs() && self.is_blob() {
            return serialize_ref(&self.hash(), s);
        }
        let text = self.text().map_err(serde::ser::Error::custom)?;
        s.serialize_str(&text)
    }
}

impl<'de> Deserialize<'de> for BlobText {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(RefVisitor(
            |r: Resolved| {
                Ok(match r {
                    Resolved::Lazy(l) => BlobText::from(l),
                    Resolved::Text(hash, t) => BlobText::with_hash(t, hash),
                })
            },
            intern_blob_text,
        ))
    }
}

impl From<BlobText> for serde_json::Value {
    fn from(t: BlobText) -> Self {
        serde_json::Value::String(t.text().map(|t| t.to_string()).unwrap_or_default())
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    /// A source over a table in memory, for tests.
    #[derive(Debug, Default)]
    pub struct MapSource(pub Mutex<HashMap<String, Arc<str>>>, pub Mutex<u32>);

    impl MapSource {
        pub fn put(&self, text: &str) -> String {
            let h = text_hash(text);
            self.0.lock().unwrap().insert(h.clone(), Arc::from(text));
            h
        }
    }

    impl BlobSource for MapSource {
        fn blob_len(&self, hash: &str) -> Option<u64> {
            self.0.lock().unwrap().get(hash).map(|t| t.len() as u64)
        }
        fn blob_head(&self, hash: &str) -> Result<String, BlobError> {
            self.0
                .lock()
                .unwrap()
                .get(hash)
                .map(|t| t.chars().take(HEAD_CHARS).collect())
                .ok_or_else(|| BlobError("gone".into()))
        }
        fn blob_text(&self, hash: &str) -> Result<Arc<str>, BlobError> {
            *self.1.lock().unwrap() += 1;
            self.0
                .lock()
                .unwrap()
                .get(hash)
                // A fresh copy, like a read from a file.
                .map(|t| Arc::from(&**t))
                .ok_or_else(|| BlobError("gone".into()))
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::MapSource;
    use super::*;

    #[test]
    fn hashes_are_sha256_of_the_text() {
        assert_eq!(
            text_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(is_hash(&text_hash("x")));
        assert!(!is_hash("ABC"));
        assert!(!is_hash(&"G".repeat(64)));
    }

    #[test]
    fn a_text_in_memory_is_plain() {
        let t = BlobText::from("hello");
        assert_eq!(&*t.text().unwrap(), "hello");
        assert_eq!(t.len(), 5);
        assert!(!t.is_empty() && !t.is_lazy() && !t.is_blob());
        assert_eq!(t.hash(), text_hash("hello"));
        assert_eq!(t.head(), "hello");
        assert!(t.lazy().is_none());
        assert_eq!(t.mem_text().map(|a| &**a), Some("hello"));
        assert_eq!(serde_json::to_string(&t).unwrap(), "\"hello\"");
        let back: BlobText = serde_json::from_str("\"hello\"").unwrap();
        assert_eq!(back, t);
        assert!(BlobText::default().is_empty());
        assert!(format!("{t:?}").contains("5 chars"));
        let from_arc = BlobText::from(Arc::<str>::from("hello"));
        assert_eq!(from_arc, t);
        assert_eq!(BlobText::from(String::from("other")).head(), "other");
        assert_eq!(serde_json::Value::from(t), serde_json::json!("hello"));
    }

    #[test]
    fn a_reference_without_a_scope_is_an_error() {
        let json = format!("{{\"$blob\": \"{}\"}}", text_hash("x"));
        let e = serde_json::from_str::<BlobText>(&json).unwrap_err();
        assert!(e.to_string().contains("not a container"), "{e}");
    }

    #[test]
    fn a_reference_in_a_scope_is_lazy_and_reads_once_while_held() {
        let src = Arc::new(MapSource::default());
        let text = "iVBORw0KGgo".repeat(1000);
        let h = src.put(&text);
        let json = format!("{{\"$blob\": \"{h}\"}}");
        let t: BlobText =
            with_load_source(src.clone(), LoadKind::Lazy, || serde_json::from_str(&json)).unwrap();
        assert!(t.is_lazy() && t.is_blob());
        assert_eq!(t.len(), text.len());
        assert_eq!(t.hash(), h);
        assert_eq!(t.head(), "iVBORw0K");
        assert_eq!(*src.1.lock().unwrap(), 0, "nothing was read");
        let a = t.text().unwrap();
        let b = t.text().unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(*src.1.lock().unwrap(), 1, "one read while it is held");
        drop((a, b));
        let _ = t.text().unwrap();
        assert_eq!(
            *src.1.lock().unwrap(),
            2,
            "read again after the memory was freed"
        );
        // Without a scope the text is written inline; with refs it is a reference.
        assert_eq!(serde_json::to_string(&t).unwrap(), format!("\"{text}\""));
        let refs = with_blob_refs(SaveRefs::default(), || serde_json::to_string(&t)).unwrap();
        assert_eq!(refs, json.replace(": ", ":"));
        assert!(!writing_refs());
        // An equal text in memory is equal.
        assert_eq!(t, BlobText::from(text.as_str()));
        assert_ne!(t, BlobText::from("other"));
        assert!(t.lazy().is_some() && t.mem_text().is_none());
        assert!(format!("{t:?}").contains("LazyBlob"));
        let lazy_again: BlobText =
            with_load_source(src.clone(), LoadKind::Lazy, || serde_json::from_str(&json)).unwrap();
        assert_eq!(t, lazy_again);
    }

    #[test]
    fn an_eager_read_puts_the_text_in_memory() {
        let src = Arc::new(MapSource::default());
        let text = "iVBORw0KGgo".repeat(1000);
        let h = src.put(&text);
        let json = format!("{{\"$blob\": \"{h}\"}}");
        let t: BlobText =
            with_load_source(src.clone(), LoadKind::Eager, || serde_json::from_str(&json)).unwrap();
        assert!(!t.is_lazy());
        assert_eq!(&*t.text().unwrap(), text.as_str());
        assert_eq!(*src.1.lock().unwrap(), 1);
        assert_eq!(t.hash(), h);
        // An eager read of a blob that cannot be read is an error at once.
        src.0.lock().unwrap().clear();
        assert!(
            with_load_source(src, LoadKind::Eager, || serde_json::from_str::<BlobText>(
                &json
            ))
            .is_err()
        );
    }

    #[test]
    fn small_texts_stay_inline_in_a_save_with_refs() {
        let t = BlobText::from("small");
        let out = with_blob_refs(SaveRefs::default(), || serde_json::to_string(&t)).unwrap();
        assert_eq!(out, "\"small\"");
    }

    /// A source that says it has a blob of any size, without the memory.
    #[derive(Debug)]
    struct Sized(u64, std::sync::atomic::AtomicU32);

    impl BlobSource for Sized {
        fn blob_len(&self, _hash: &str) -> Option<u64> {
            Some(self.0)
        }
        fn blob_head(&self, _hash: &str) -> Result<String, BlobError> {
            Ok(String::new())
        }
        fn blob_text(&self, _hash: &str) -> Result<Arc<str>, BlobError> {
            self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(Arc::from("x"))
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
            self
        }
    }

    #[test]
    fn a_picture_over_the_bound_is_refused_when_the_reference_is_read() {
        let json = format!("{{\"$blob\": \"{}\"}}", text_hash("x"));
        let over = Arc::new(Sized(MAX_BLOB_TEXT as u64 + 1, Default::default()));
        let e = with_load_source(over, LoadKind::Lazy, || {
            serde_json::from_str::<BlobText>(&json)
        })
        .unwrap_err()
        .to_string();
        assert!(e.contains("more than"), "{e}");
        let at = Arc::new(Sized(MAX_BLOB_TEXT as u64, Default::default()));
        assert!(
            with_load_source(at, LoadKind::Lazy, || serde_json::from_str::<BlobText>(
                &json
            ))
            .is_ok()
        );
    }

    #[test]
    fn the_reference_of_an_import_base_has_the_same_bound() {
        let json = format!("{{\"$blob\": \"{}\"}}", text_hash("x"));
        let over = Arc::new(Sized(MAX_BLOB_TEXT as u64 + 1, Default::default()));
        let e = with_load_source(over.clone(), LoadKind::Lazy, || {
            read_ref_text(&text_hash("x"))
        })
        .unwrap_err();
        assert!(e.contains("more than"), "{e}");
        assert_eq!(
            over.1.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "nothing was read"
        );
        let _ = json;
    }

    #[test]
    fn one_hash_is_one_handle_in_a_lazy_load() {
        let src = Arc::new(MapSource::default());
        let text = "iVBORw0KGgo".repeat(1000);
        let h = src.put(&text);
        let json = format!("[{0},{0},{0}]", format!("{{\"$blob\": \"{h}\"}}"));
        let list: Vec<BlobText> =
            with_load_source(src.clone(), LoadKind::Lazy, || serde_json::from_str(&json)).unwrap();
        assert_eq!(list.len(), 3);
        let a = list[0].lazy().unwrap();
        assert!(list.iter().all(|t| Arc::ptr_eq(a, t.lazy().unwrap())));
        // Another load makes another handle.
        let again: Vec<BlobText> =
            with_load_source(src, LoadKind::Lazy, || serde_json::from_str(&json)).unwrap();
        assert!(!Arc::ptr_eq(a, again[0].lazy().unwrap()));
    }

    #[test]
    fn one_hash_is_one_text_and_one_read_in_an_eager_load() {
        let src = Arc::new(MapSource::default());
        let text = "iVBORw0KGgo".repeat(1000);
        let h = src.put(&text);
        let json = format!("[{0},{0},{0}]", format!("{{\"$blob\": \"{h}\"}}"));
        let list: Vec<BlobText> =
            with_load_source(src.clone(), LoadKind::Eager, || serde_json::from_str(&json)).unwrap();
        let first = list[0].mem_text().unwrap();
        assert!(
            list.iter()
                .all(|t| Arc::ptr_eq(first, t.mem_text().unwrap()))
        );
        assert_eq!(
            *src.1.lock().unwrap(),
            1,
            "the file was read once for 3 references"
        );
    }

    /// A source that marks which of its blobs are summaries of an import base.
    #[derive(Debug)]
    struct Marked(MapSource, Option<bool>);

    impl BlobSource for Marked {
        fn blob_len(&self, hash: &str) -> Option<u64> {
            self.0.blob_len(hash)
        }
        fn blob_head(&self, hash: &str) -> Result<String, BlobError> {
            self.0.blob_head(hash)
        }
        fn is_import_base(&self, _hash: &str) -> Option<bool> {
            self.1
        }
        fn blob_text(&self, hash: &str) -> Result<Arc<str>, BlobError> {
            self.0.blob_text(hash)
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
            self
        }
    }

    #[test]
    fn the_summaries_of_an_import_base_are_a_marked_blob_of_at_most_8_mib() {
        let small = "s".repeat(100);
        let big = "b".repeat(MAX_BASE_PART_BYTES + 1);
        for (mark, text, want) in [
            (Some(true), &small, Ok(())),
            (None, &small, Ok(())),
            (Some(false), &small, Err("does not mark")),
            (Some(true), &big, Err("more than")),
            (None, &big, Err("more than")),
        ] {
            let src = Arc::new(Marked(MapSource::default(), mark));
            let h = src.0.put(text);
            let r = with_load_budget(src, LoadKind::Eager, u64::MAX, || read_ref_text(&h));
            match want {
                Ok(()) => assert_eq!(r.unwrap().len(), text.len()),
                Err(part) => {
                    let e = r.unwrap_err();
                    assert!(e.contains(part), "{mark:?}: {e}");
                }
            }
        }
    }

    #[test]
    fn the_budget_of_a_load_stops_a_read_that_asks_for_more() {
        let src = Arc::new(MapSource::default());
        let a = src.put(&"a".repeat(6000));
        let b = src.put(&"b".repeat(6000));
        let json = format!("[{{\"$blob\": \"{a}\"}},{{\"$blob\": \"{b}\"}}]");
        let e = with_load_budget(src.clone(), LoadKind::Eager, 10_000, || {
            serde_json::from_str::<Vec<BlobText>>(&json)
        })
        .unwrap_err()
        .to_string();
        assert!(e.contains("bigger together"), "{e}");
        assert!(
            with_load_budget(src, LoadKind::Eager, 12_000, || {
                serde_json::from_str::<Vec<BlobText>>(&json)
            })
            .is_ok()
        );
    }

    #[test]
    fn equal_texts_of_a_plain_read_are_one_copy() {
        let text = "iVBORw0KGgo".repeat(1000);
        let item = format!("\"{text}\"");
        let json = format!("[{item},{item},{item},\"short\",\"short\"]");
        let list: Vec<BlobText> = with_intern_scope(|| serde_json::from_str(&json)).unwrap();
        let first = list[0].mem_text().unwrap();
        assert!(
            list[..3]
                .iter()
                .all(|t| Arc::ptr_eq(first, t.mem_text().unwrap()))
        );
        // A text under the size of a blob is not merged.
        assert!(!Arc::ptr_eq(
            list[3].mem_text().unwrap(),
            list[4].mem_text().unwrap()
        ));
        // The hash is known from the merge.
        assert_eq!(list[0].hash(), text_hash(&text));
        // Outside of a scope nothing is merged.
        let plain: Vec<BlobText> = serde_json::from_str(&json).unwrap();
        assert!(!Arc::ptr_eq(
            plain[0].mem_text().unwrap(),
            plain[1].mem_text().unwrap()
        ));
    }

    #[test]
    fn equal_pictures_of_a_plain_read_are_one_copy_of_the_bytes() {
        use crate::underlay::{PngData, base64_encode};
        let bytes: Vec<u8> = (0..6000u32).map(|i| (i * 7) as u8).collect();
        let text = base64_encode(&bytes);
        let item = format!("\"{text}\"");
        let json = format!("[{item},{item},{item}]");
        let list: Vec<PngData> = with_intern_scope(|| serde_json::from_str(&json)).unwrap();
        let first = list[0].mem_bytes().unwrap();
        assert!(
            list.iter()
                .all(|p| Arc::ptr_eq(first, p.mem_bytes().unwrap()))
        );
        assert_eq!(**first, bytes);
        let plain: Vec<PngData> = serde_json::from_str(&json).unwrap();
        assert!(!Arc::ptr_eq(
            plain[0].mem_bytes().unwrap(),
            plain[1].mem_bytes().unwrap()
        ));
    }

    #[test]
    fn bad_references_are_errors() {
        let src = Arc::new(MapSource::default());
        let h = src.put("text");
        let read = |json: &str| {
            with_load_source(src.clone(), LoadKind::Lazy, || {
                serde_json::from_str::<BlobText>(json)
            })
            .unwrap_err()
            .to_string()
        };
        assert!(
            read(&format!("{{\"$blob\": \"{}\"}}", text_hash("missing")))
                .contains("not in the file")
        );
        assert!(read("{\"$blob\": \"abc\"}").contains("64 lowercase"));
        assert!(read("{\"other\": 1}").contains("$blob"));
        assert!(read(&format!("{{\"$blob\": \"{h}\", \"x\": 1}}")).contains("only the key"));
        assert!(read("5").contains("reference"));
    }

    #[test]
    fn scopes_nest_and_are_restored_after_a_panic() {
        let a = Arc::new(MapSource::default());
        let r = std::panic::catch_unwind(|| {
            with_blob_refs(SaveRefs::default(), || {
                assert!(writing_refs());
                panic!("boom");
            })
        });
        assert!(r.is_err());
        assert!(!writing_refs());
        with_blob_refs(SaveRefs::default(), || {
            with_blob_refs(SaveRefs::default(), || assert!(writing_refs()));
            assert!(writing_refs());
        });
        assert!(with_load_source(a.clone(), LoadKind::Lazy, || load_scope().is_some()));
        assert!(load_scope().is_none());
    }

    #[test]
    fn lazy_bytes_decode_and_share() {
        let src = Arc::new(MapSource::default());
        let text = crate::underlay::base64_encode(&[1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let h = src.put(&text);
        let lazy = LazyBlob::new(src.clone(), &h).unwrap();
        let a = lazy.bytes().unwrap();
        assert_eq!(*a, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert!(Arc::ptr_eq(&a, &lazy.bytes().unwrap()));
        assert!(lazy.source().as_any().is::<MapSource>());
        assert!(LazyBlob::new(src.clone(), &text_hash("none")).is_err());
        let bad = src.put("not base64!");
        assert!(LazyBlob::new(src, &bad).unwrap().bytes().is_err());
    }
}
