//! Reading a document from bytes or from an opened container.

use std::cell::RefCell;
use std::io::{BufReader, Read};
use std::sync::Arc;

use kadakas_container::{Container, ContainerError, Limits};
use kadakas_core::blob::{BlobSource, LoadKind, with_load_budget};
use kadakas_core::model::{Document, LoadError};

use crate::error::FileError;
use crate::history;
use crate::layout::{DOCUMENT, Form, sniff};
use crate::lazy::FileHistory;
use crate::source::FileSource;

/// How the pictures of a container are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadMode {
    /// The pictures stay in the file and are read when they are used (DOC-21). For the document
    /// that a person works on: it keeps a handle to the file, so the file must stay where it is.
    Lazy,
    /// Every picture is read into memory now. For a document that is only read from (a library,
    /// an import) and must not depend on its file after the read.
    Eager,
    /// Like `Lazy`, but the file does not note the pictures as in use: for a copy of the drawing
    /// that is read to be looked at and dropped (the anchor of the history).
    Untracked,
}

/// A document that was read, and where from.
pub struct Opened {
    pub doc: Document,
    pub form: Form,
    /// The container the document was read from, if it was one. The lazy pictures of `doc` read
    /// from it. Keep it as long as the document is open: a save copies unchanged parts from it.
    pub source: Option<Arc<FileSource>>,
}

impl std::fmt::Debug for Opened {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Opened")
            .field("form", &self.form)
            .field("objects", &self.doc.objects.len())
            .field("source", &self.source)
            .finish()
    }
}

/// Reads a document from bytes in memory (the file picker of the browser, a test): a container
/// or the plain JSON form, told by the first bytes. A container is read with `limits`. With
/// `ReadMode::Eager` the result holds no handle to the container.
pub fn read_bytes(
    bytes: impl Into<Arc<[u8]>>,
    limits: Limits,
    mode: ReadMode,
) -> Result<Opened, FileError> {
    let bytes: Arc<[u8]> = bytes.into();
    match sniff(&bytes[..bytes.len().min(64)])? {
        Form::Json => Ok(Opened {
            doc: Document::from_slice(&bytes)?,
            form: Form::Json,
            source: None,
        }),
        Form::Container => {
            let source = FileSource::from_container(Container::from_bytes(bytes, limits)?);
            let doc = read_source(&source, mode)?;
            Ok(Opened {
                doc,
                form: Form::Container,
                source: (mode == ReadMode::Lazy).then_some(source),
            })
        }
    }
}

/// Passes the bytes of an entry on and keeps the first error of the entry, because the JSON
/// parser turns a failed read into a message of its own.
struct Spy<'a, R> {
    inner: R,
    error: &'a RefCell<Option<ContainerError>>,
}

impl<R: Read> Read for Spy<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf).inspect_err(|e| {
            if let Some(c) = e.get_ref().and_then(|i| i.downcast_ref::<ContainerError>()) {
                self.error.borrow_mut().get_or_insert_with(|| c.clone());
            }
        })
    }
}

/// Reads the document of an opened container. The pictures stay in the file (lazy); the history
/// is read from its entries.
pub fn read_source(source: &Arc<FileSource>, mode: ReadMode) -> Result<Document, FileError> {
    read_entry_document(source, DOCUMENT, mode, true)
}

/// Reads the drawing that is in the entry `entry` (`document.json`, or the anchor drawing of the
/// history). `attach` puts the history of the container into the document.
pub(crate) fn read_entry_document(
    source: &Arc<FileSource>,
    entry_name: &str,
    mode: ReadMode,
    attach: bool,
) -> Result<Document, FileError> {
    // The directory of the file as it was opened says what the file holds; the drawing is read
    // from the file that has it (the same file, or the `.bak` if the file was replaced).
    let dir = source.container();
    if !dir.contains(entry_name) {
        return Err(ContainerError::Entry {
            name: entry_name.to_string(),
            reason: "it is missing; this container holds no drawing".into(),
        }
        .into());
    }
    let container = source.container_with_entry(entry_name)?;
    let error = RefCell::new(None);
    let mut entry = container.reader(entry_name)?;
    let blobs: Arc<dyn BlobSource> = source.clone();
    let kind = match mode {
        ReadMode::Lazy => LoadKind::Lazy,
        ReadMode::Untracked => LoadKind::LazyUntracked,
        ReadMode::Eager => LoadKind::Eager,
    };
    let parsed = with_load_budget(blobs, kind, dir.uncompressed_bytes(), || {
        let spy = Spy {
            inner: &mut entry,
            error: &error,
        };
        Document::from_reader_with(
            BufReader::with_capacity(256 * 1024, spy),
            || {
                container
                    .reader(entry_name)
                    .map(|r| Spy {
                        inner: r,
                        error: &error,
                    })
                    .map_err(std::io::Error::from)
            },
            |doc| {
                if !attach || !history::present(&dir) {
                    return Ok(Vec::new());
                }
                if doc.history.is_some() {
                    return Err(LoadError::Invalid(
                        "the drawing and the container both hold a history of versions".into(),
                    ));
                }
                if mode != ReadMode::Eager {
                    // The history stays in the file until a program uses it (DOC-21).
                    doc.lazy_history = FileHistory::of(source)
                        .map(|h| h as Arc<dyn kadakas_core::history::HistoryLoader>);
                    return Ok(Vec::new());
                }
                // A copy that is read to be used (a library, an import) has no use for the
                // versions. If they cannot be read the drawing still loads, and a note says so
                // (REV-01: a history that cannot be read does not stop the drawing).
                match history::read(source) {
                    Ok(h) => {
                        doc.history = h;
                        Ok(Vec::new())
                    }
                    Err(e) => Ok(vec![format!(
                        "the versions of this file could not be read and are not part of this copy: {e}"
                    )]),
                }
            },
        )
    });
    match parsed {
        Ok(doc) => {
            // The end of the entry: the size, the CRC-32 and the hash are checked there.
            entry.finish()?;
            Ok(doc)
        }
        Err(e) => {
            if let Some(c) = error.into_inner() {
                return Err(c.into());
            }
            Err(e.into())
        }
    }
}
