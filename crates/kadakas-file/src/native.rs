//! Files on disk: open by path, the atomic save, `convert`, and a quick look at a file.
//!
//! The save keeps the rules of DOC-19: the content goes through a buffered writer into a
//! temporary file in the folder of the target, which is flushed and synced, and then renamed over
//! the target, and the folder is synced (`kadakas_sys::fsio::write_atomic_with`). The old file is
//! copied to `<path>.bak` first. If anything fails the old file stays as it was and the
//! temporary file is deleted. Saving over the file that the lazy parts of the document read from
//! works: the copy of unchanged entries reads the old file while the temporary file is written,
//! and the rename comes last. After the rename the sources that read from that path open it again.

use std::io::{Read, Seek};
use std::path::Path;
use std::sync::Arc;

use kadakas_container::Limits;
use kadakas_core::model::Document;
use kadakas_sys::fsio::{self, OpenError};

use crate::error::FileError;
use crate::layout::{DOCUMENT, Form, HISTORY_BLOBS, sniff};
use crate::read::{Opened, ReadMode, read_source};
use crate::source::FileSource;
use crate::write::{WriteOptions, Written, write_container, write_json};

fn open_error(path: &Path, e: OpenError) -> FileError {
    let m = format!("cannot read {}", path.display());
    match e {
        OpenError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => {
            FileError::NotFound(format!("{m}: {e}"))
        }
        OpenError::Io(e) => FileError::Io(format!("{m}: {e}")),
        OpenError::NotRegular => FileError::NotRegular(format!("{m}: {}", OpenError::NotRegular)),
        OpenError::TooLarge { len, max } => FileError::TooLarge { len, max },
    }
}

/// Reads up to `buf.len()` bytes.
fn read_head(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// Opens a document file of either form. The file is a regular file of at most `max_bytes`
/// (checked before anything is read). A container is also read with `max_bytes` as the limit of
/// its uncompressed bytes (the sum of the sizes in the manifest), so a small file cannot expand
/// to more than the limit. The plain JSON form is parsed from a buffered reader.
pub fn open_path(path: &Path, max_bytes: u64, mode: ReadMode) -> Result<Opened, FileError> {
    let mut reader = fsio::open_regular(path, max_bytes).map_err(|e| open_error(path, e))?;
    let mut head = [0u8; 64];
    let n = read_head(&mut reader, &mut head)
        .map_err(|e| FileError::Io(format!("cannot read {}: {e}", path.display())))?;
    match sniff(&head[..n])? {
        Form::Json => {
            reader
                .rewind()
                .map_err(|e| FileError::Io(format!("cannot read {}: {e}", path.display())))?;
            Ok(Opened {
                doc: Document::from_reader(reader)?,
                form: Form::Json,
                source: None,
            })
        }
        Form::Container => {
            drop(reader);
            let source = FileSource::open_path(path, Limits::default().with_total(max_bytes))?;
            let doc = read_source(&source, mode)?;
            Ok(Opened {
                doc,
                form: Form::Container,
                source: (mode == ReadMode::Lazy).then_some(source),
            })
        }
    }
}

/// What a save did.
#[derive(Clone, Debug)]
pub struct Saved {
    pub form: Form,
    /// The container save report (`None` for the JSON form).
    pub written: Option<Written>,
    /// Things the person should know (a source that could not be opened again).
    pub notes: Vec<String>,
}

/// Saves the document to `path` in the given form, atomically. `opts.keep` names the files the
/// document was opened from (unchanged entries are copied from them).
pub fn save_path(
    path: &Path,
    doc: &Document,
    form: Form,
    opts: &WriteOptions,
) -> Result<Saved, FileError> {
    let mut written: Option<Written> = None;
    let mut failure: Option<FileError> = None;
    let result = fsio::write_atomic_with(path, |w| {
        let r = match form {
            Form::Json => write_json(w, doc),
            Form::Container => write_container(w, doc, opts).map(|(_, rep)| written = Some(rep)),
        };
        r.map_err(|e| {
            let io = std::io::Error::other(e.to_string());
            failure = Some(e);
            io
        })
    });
    if let Err(e) = result {
        return Err(failure
            .unwrap_or_else(|| FileError::Io(format!("cannot write {}: {e}", path.display()))));
    }
    // The pictures of the document are in the new file now (by hash). The sources that were read
    // from move there: after a save over their file it is a new file, and after a save to a new
    // path the document no longer depends on the old file.
    let mut notes = Vec::new();
    if form == Form::Container
        && let Some(rep) = &written
    {
        for s in &rep.sources {
            if s.path().is_some()
                && let Err(e) = s.rebind(path)
            {
                notes.push(format!(
                    "the saved file {} could not be opened again for the pictures: {e}",
                    path.display()
                ));
            }
        }
    }
    Ok(Saved {
        form,
        written,
        notes,
    })
}

/// Opens `from` and saves it as `to` in the given form (`kadakas convert`). The document is
/// checked on the way in, so a convert is also a check of the file.
pub fn convert(from: &Path, to: &Path, form: Form, max_bytes: u64) -> Result<Saved, FileError> {
    let opened = open_path(from, max_bytes, ReadMode::Lazy)?;
    let opts = WriteOptions {
        generator: crate::generator(),
        keep: opened.source.iter().cloned().collect(),
    };
    save_path(to, &opened.doc, form, &opts)
}

/// A quick look at a file, without loading the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inspect {
    pub form: Form,
    pub file_bytes: u64,
    /// The bytes of all entries, uncompressed (the manifest sizes). For the JSON form, the file
    /// size.
    pub uncompressed_bytes: u64,
    /// The bytes of `document.json` (the JSON form: the file size).
    pub document_bytes: u64,
    /// The bytes of the blobs (pictures); they stay in the file when it is opened.
    pub blob_bytes: u64,
    /// The bytes of the history entries.
    pub history_bytes: u64,
    pub entries: usize,
}

impl Inspect {
    /// The bytes that opening the file reads into memory: the drawing and the history. The blobs
    /// stay in the file.
    pub fn resident_bytes(&self) -> u64 {
        self.document_bytes.saturating_add(self.history_bytes)
    }
}

/// Looks at a file: its form and sizes. A container is opened (its directory and manifest are
/// read and checked); nothing else is read.
pub fn inspect(path: &Path) -> Result<Inspect, FileError> {
    let mut reader = fsio::open_regular(path, u64::MAX).map_err(|e| open_error(path, e))?;
    let mut head = [0u8; 64];
    let n = read_head(&mut reader, &mut head)
        .map_err(|e| FileError::Io(format!("cannot read {}: {e}", path.display())))?;
    let file_bytes = std::fs::metadata(path).map_or(0, |m| m.len());
    match sniff(&head[..n])? {
        Form::Json => Ok(Inspect {
            form: Form::Json,
            file_bytes,
            uncompressed_bytes: file_bytes,
            document_bytes: file_bytes,
            blob_bytes: 0,
            history_bytes: 0,
            entries: 0,
        }),
        Form::Container => {
            drop(reader);
            let source = FileSource::open_path(path, Limits::default())?;
            let c = source.container();
            let size = |name: &str| c.entry(name).map_or(0, |e| e.size);
            let blob_bytes = c
                .entries()
                .iter()
                .filter(|e| e.name.starts_with(crate::layout::BLOB_DIR))
                .map(|e| e.size)
                .sum();
            let history_bytes = c
                .entries()
                .iter()
                .filter(|e| e.name.starts_with("history/"))
                .map(|e| e.size)
                .sum();
            let _ = HISTORY_BLOBS;
            Ok(Inspect {
                form: Form::Container,
                file_bytes,
                uncompressed_bytes: c.uncompressed_bytes(),
                document_bytes: size(DOCUMENT),
                blob_bytes,
                history_bytes,
                entries: c.entries().len(),
            })
        }
    }
}

/// Opens a file and keeps the source for the pictures: a shortcut for tests and tools.
pub fn open_with_source(
    path: &Path,
    max_bytes: u64,
) -> Result<(Document, Option<Arc<FileSource>>), FileError> {
    let o = open_path(path, max_bytes, ReadMode::Lazy)?;
    Ok((o.doc, o.source))
}
