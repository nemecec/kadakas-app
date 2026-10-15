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
use std::sync::{Arc, Mutex, OnceLock, Weak};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The key of a reference: `{"$blob": "<sha256 hex>"}`.
pub const REF_KEY: &str = "$blob";
/// The smallest text that becomes a blob (the same as in collaboration, `MIN_BLOB_BYTES`).
pub const MIN_BLOB_TEXT: usize = 4 * 1024;
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
    /// The text of the blob. It matches the hash, or this is an error.
    fn blob_text(&self, hash: &str) -> Result<Arc<str>, BlobError>;
    /// For a program that knows the kind of the source (the copy of a blob into a new file).
    fn as_any(&self) -> &dyn Any;
}

/// A blob that stays in the file until it is used.
pub struct LazyBlob {
    hash: String,
    len: usize,
    head: String,
    source: Arc<dyn BlobSource>,
    text: Mutex<Option<Weak<str>>>,
    bytes: Mutex<Weak<Vec<u8>>>,
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
        let len = usize::try_from(len).map_err(|_| BlobError("the picture is too big".into()))?;
        let head = source.blob_head(hash)?;
        Ok(LazyBlob {
            hash: hash.to_string(),
            len,
            head,
            source,
            text: Mutex::new(None),
            bytes: Mutex::new(Weak::new()),
        })
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
        let b = Arc::new(
            crate::underlay::base64_decode(&text)
                .ok_or_else(|| BlobError(format!("the picture {} is not valid base64", self.hash)))?,
        );
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

thread_local! {
    static LOAD: RefCell<Option<Arc<dyn BlobSource>>> = const { RefCell::new(None) };
    static SAVE: RefCell<Option<SaveRefs>> = const { RefCell::new(None) };
}

/// What a save in the container form knows: write a reference for each blob.
#[derive(Default)]
pub struct SaveRefs {
    /// The hash of the text of each import base, by the address of its shared body (the text is
    /// the compact JSON of `recs`). Made by the caller before the write.
    pub import_bases: HashMap<usize, String>,
}

struct Restore<T: 'static>(&'static std::thread::LocalKey<RefCell<Option<T>>>, Option<T>);

impl<T: 'static> Drop for Restore<T> {
    fn drop(&mut self) {
        let old = self.1.take();
        self.0.with(|c| *c.borrow_mut() = old);
    }
}

/// Runs `f` with a source for the references that the pictures meet while they are read. The
/// previous scope comes back after `f`, also when `f` panics.
pub fn with_load_source<R>(source: Arc<dyn BlobSource>, f: impl FnOnce() -> R) -> R {
    let old = LOAD.with(|c| c.borrow_mut().replace(source));
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

/// The hash of an import base body that [`with_blob_refs`] knows.
pub(crate) fn import_base_ref(ptr: usize) -> Option<String> {
    SAVE.with(|c| c.borrow().as_ref()?.import_bases.get(&ptr).cloned())
}

/// The source of the scope of a read.
fn load_source() -> Option<Arc<dyn BlobSource>> {
    LOAD.with(|c| c.borrow().clone())
}

/// The text of a reference, for a value that is read eagerly (the summaries of an import base).
pub(crate) fn read_ref_text(hash: &str) -> Result<Arc<str>, String> {
    let source = ref_source(hash)?;
    source.blob_text(hash).map_err(|e| e.to_string())
}

fn ref_source(hash: &str) -> Result<Arc<dyn BlobSource>, String> {
    if !is_hash(hash) {
        return Err("a picture reference needs 64 lowercase hex digits".into());
    }
    load_source().ok_or_else(|| {
        "the text refers to a picture by hash ({\"$blob\": ...}), but it is not a container file, so the picture is not here".into()
    })
}

/// A lazy handle for a reference met while reading.
pub(crate) fn lazy_from_ref(hash: &str) -> Result<Arc<LazyBlob>, String> {
    let source = ref_source(hash)?;
    LazyBlob::new(source, hash)
        .map(Arc::new)
        .map_err(|e| e.to_string())
}

/// Reads a reference value: `{"$blob": "<hash>"}` as a visitor of a map.
pub(crate) struct RefVisitor<F, G>(pub F, pub G);

impl<'de, T, F: FnOnce(Arc<LazyBlob>) -> T, G: FnOnce(&str) -> T> serde::de::Visitor<'de>
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
            return Err(A::Error::custom(
                "a picture is a text or {\"$blob\": hash}",
            ));
        }
        let hash: String = map.next_value()?;
        if map.next_key::<String>()?.is_some() {
            return Err(A::Error::custom("a picture reference has only the key $blob"));
        }
        let lazy = lazy_from_ref(&hash).map_err(A::Error::custom)?;
        Ok((self.0)(lazy))
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
            (Repr::Mem { text: a, .. }, Repr::Mem { text: b, .. }) => {
                Arc::ptr_eq(a, b) || a == b
            }
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
        d.deserialize_any(RefVisitor(BlobText::from, |t: &str| BlobText::from(t)))
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
        let t: BlobText = with_load_source(src.clone(), || serde_json::from_str(&json)).unwrap();
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
        assert_eq!(*src.1.lock().unwrap(), 2, "read again after the memory was freed");
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
            with_load_source(src.clone(), || serde_json::from_str(&json)).unwrap();
        assert_eq!(t, lazy_again);
    }

    #[test]
    fn small_texts_stay_inline_in_a_save_with_refs() {
        let t = BlobText::from("small");
        let out = with_blob_refs(SaveRefs::default(), || serde_json::to_string(&t)).unwrap();
        assert_eq!(out, "\"small\"");
    }

    #[test]
    fn bad_references_are_errors() {
        let src = Arc::new(MapSource::default());
        let h = src.put("text");
        let read = |json: &str| {
            with_load_source(src.clone(), || serde_json::from_str::<BlobText>(json))
                .unwrap_err()
                .to_string()
        };
        assert!(read(&format!("{{\"$blob\": \"{}\"}}", text_hash("missing"))).contains("not in the file"));
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
        assert!(with_load_source(a.clone(), || load_source().is_some()));
        assert!(load_source().is_none());
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
