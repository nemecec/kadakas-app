//! Reading a document from bytes or from an opened container.

use std::cell::RefCell;
use std::io::{BufReader, Read};
use std::sync::Arc;

use kadakas_container::{Container, ContainerError, Limits};
use kadakas_core::blob::{BlobSource, with_load_source};
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
    let container = source.container();
    if !container.contains(DOCUMENT) {
        return Err(ContainerError::Entry {
            name: DOCUMENT.to_string(),
            reason: "it is missing; this container holds no drawing".into(),
        }
        .into());
    }
    let error = RefCell::new(None);
    let history_error: RefCell<Option<FileError>> = RefCell::new(None);
    let mut entry = container.reader(DOCUMENT)?;
    let blobs: Arc<dyn BlobSource> = source.clone();
    let parsed = with_load_source(blobs, mode == ReadMode::Eager, || {
        let spy = Spy {
            inner: &mut entry,
            error: &error,
        };
        Document::from_reader_with(
            BufReader::with_capacity(256 * 1024, spy),
            || {
                container
                    .reader(DOCUMENT)
                    .map(|r| Spy {
                        inner: r,
                        error: &error,
                    })
                    .map_err(std::io::Error::from)
            },
            |doc| {
                if !history::present(&container) {
                    return Ok(());
                }
                if doc.history.is_some() {
                    return Err(LoadError::Invalid(
                        "the drawing and the container both hold a history of versions".into(),
                    ));
                }
                if mode == ReadMode::Lazy {
                    // The history stays in the file until a program uses it (DOC-21).
                    doc.lazy_history = FileHistory::of(source)
                        .map(|h| h as Arc<dyn kadakas_core::history::HistoryLoader>);
                    return Ok(());
                }
                match history::read(&container) {
                    Ok(h) => {
                        doc.history = h;
                        Ok(())
                    }
                    Err(e) => {
                        let m = e.to_string();
                        *history_error.borrow_mut() = Some(e);
                        Err(LoadError::Invalid(m))
                    }
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
            if let Some(h) = history_error.into_inner() {
                return Err(h);
            }
            Err(e.into())
        }
    }
}
