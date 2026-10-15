//! The container of a Kadakas file (DOC-20): a ZIP file with a manifest.
//!
//! This crate knows ZIP, the manifest and the bounds of reading. It knows nothing of documents:
//! the layout (`document.json`, `blobs/<sha256>`, `history/`) and the mapping of a document to
//! entries are in `kadakas-file`.
//!
//! - [`Container`] opens a medium (memory, or a file that is opened again for each read), checks
//!   the end record, the central directory and the manifest against [`Limits`], and reads
//!   entries through a checked [`EntryReader`] (size, CRC-32, SHA-256). A bad entry is an error
//!   that names the entry. Nothing panics on the bytes of a file.
//! - [`Writer`] writes entries to any `Write` (no seek), adds the manifest at the end, and copies
//!   an entry from another container without decoding it ([`Writer::copy_raw`]).
//! - [`PartSource`] reads a part by name. A document keeps a handle to the source of its file and
//!   reads pictures and the history from it when they are used (DOC-21).
//!
//! The format: the ZIP of PKWARE, stored and deflate entries, no encryption, no ZIP64 (so at
//! most 65 534 entries and 4 GiB less 2 bytes for the file and for each entry), UTF-8 names that
//! are plain relative paths, and a `manifest.json` that lists every other entry with its size and
//! its SHA-256. Any ZIP tool can open the file.
//!
//! Pure and wasm-compatible. The file medium is native only.

mod error;
mod limits;
mod manifest;
mod medium;
mod reader;
mod source;
mod writer;

pub use error::ContainerError;
pub use limits::{Limits, MANIFEST_NAME, MAX_ENTRIES, MAX_FIELD, check_name};
pub use manifest::{FORMAT, Manifest, ManifestEntry, VERSION, hex, parse_hash, sha256_hex};
#[cfg(not(target_arch = "wasm32"))]
pub use medium::FileMedium;
pub use medium::{Medium, MemMedium};
pub use reader::{Container, Entry, EntryReader, Method, RawEntry, has_zip_magic};
pub use source::{ContainerSource, PartSource};
pub use writer::{Compression, Writer};

#[cfg(test)]
mod tests;
