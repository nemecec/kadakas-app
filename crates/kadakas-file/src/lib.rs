//! The file forms of a Kadakas document (DOC-20, DOC-21, DOC-22).
//!
//! A document has two forms on disk, and both load the same model:
//!
//! - the **container** (`.kad`): a ZIP file (`kadakas-container`) with `document.json` (the
//!   drawing, deflated), `blobs/<sha256>` (the pictures and other big texts, stored once, with
//!   the hashes of collaboration), `history/` (the versions of REV-01) and `manifest.json`
//!   (every entry with its size and SHA-256). The pictures stay in the file when a document is
//!   opened (lazy parts): the document holds a handle (`kadakas_core::blob::LazyBlob`) and the
//!   open file ([`FileSource`]) reads a picture by hash when a program uses it. A save copies the
//!   unchanged pictures from the old file without decoding them.
//! - the **plain JSON** form: one readable JSON file, pictures as base64 text, the history in the
//!   field `history`. For git, diffs and debugging.
//!
//! [`read_bytes`] and [`write_container`] / [`write_json`] work on bytes and streams and are
//! wasm-compatible (the browser reads the picker bytes and writes the download). The `native`
//! module adds paths, the atomic save and `convert`.
//!
//! No UI code.

mod error;
mod history;
mod layout;
mod lazy;
mod read;
#[cfg(not(target_arch = "wasm32"))]
mod seen;
mod source;
mod write;

#[cfg(not(target_arch = "wasm32"))]
pub mod native;

pub use error::{FileError, Missing};
pub use kadakas_container::{ContainerError, Limits};
pub use layout::{
    BLOB_DIR, DOCUMENT, Form, HISTORY_ANCHOR, HISTORY_ANCHOR_BLOBS, HISTORY_BLOBS, HISTORY_NEWEST,
    HISTORY_RAW, HISTORY_TOP, HISTORY_VERSIONS, blob_name, sniff,
};
pub use lazy::FileHistory;
pub use read::{Opened, ReadMode, read_bytes, read_source};
#[cfg(not(target_arch = "wasm32"))]
pub use seen::{SeenFiles, Stamp, stamp_of};
pub use source::FileSource;
pub use write::{
    DOCUMENT_LEVEL, WriteOptions, Written, resolve_history, write_container, write_json,
};

/// The text for the `generator` field of the manifest.
pub fn generator() -> String {
    format!("kadakas {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests;
