//! Writing a document: the container and the plain JSON form.
//!
//! A container is written as a stream: `document.json` first (deflate, the pictures as
//! references), then the blobs (stored, once each, sorted by hash), then the history, and the
//! manifest last. A blob that is still in the file the document was opened from is copied from
//! that file without decoding it (`Writer::copy_raw`); so is a history part that did not change.
//! A picture that the document no longer has, but that a live handle (an undo step) still uses,
//! is kept in the new file too, so that an undo after a save finds it.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufWriter, Write};
use std::sync::Arc;

use kadakas_container::{Compression, ContainerError, PartSource, Writer, hex};
use kadakas_core::blob::{BlobText, MIN_BLOB_TEXT, SaveRefs, text_hash, with_blob_refs};
use kadakas_core::model::{DocBlob, Document};
use kadakas_core::underlay::PngData;
use serde_json::Value;

use crate::error::FileError;
use crate::history::{self, Plan};
use crate::layout::{DOCUMENT, blob_name};
use crate::lazy::FileHistory;
use crate::source::FileSource;

/// The level of deflate for `document.json` and the history parts. Level 1 gives a file three
/// times smaller than the plain form at about 170 MB/s of text; the numbers are in
/// `docs/doc-load-benchmark.md` section 4.
pub const DOCUMENT_LEVEL: u32 = 1;

/// Options of a container save.
#[derive(Clone, Default)]
pub struct WriteOptions {
    /// The program that writes, for the manifest.
    pub generator: String,
    /// The files the document was opened from. Their unchanged history parts are copied, and the
    /// pictures that their handles still use are kept.
    pub keep: Vec<Arc<FileSource>>,
}

/// What a container save did.
#[derive(Clone, Debug, Default)]
pub struct Written {
    /// Entries in the container, the manifest not counted.
    pub entries: usize,
    /// Blobs copied from an old file without decoding.
    pub blobs_copied: usize,
    /// Blobs written from memory.
    pub blobs_written: usize,
    /// Pictures kept only because a live handle uses them.
    pub blobs_kept: usize,
    /// History parts copied from an old file.
    pub parts_copied: usize,
    /// The old files that were read from. After the rename of the save the caller moves them to
    /// the new file (`kadakas_file::native::rebind_sources`, `FileSource::rebind`).
    pub sources: Vec<Arc<FileSource>>,
}

/// Writes the plain JSON form: the pretty text of the document, pictures as base64 text, the
/// history inside as the field `history`. A picture in a file is read when it is written, one at
/// a time.
pub fn write_json<W: Write>(w: W, doc: &Document) -> Result<(), FileError> {
    if doc.history.is_none() && doc.lazy_history.is_some() {
        return Err(FileError::Io(
            "the versions of this document were not read from the file yet, and the plain JSON form needs them (resolve the history first)"
                .into(),
        ));
    }
    let mut bw = BufWriter::with_capacity(256 * 1024, w);
    doc.write_pretty(&mut bw)?;
    bw.flush()?;
    Ok(())
}

/// Reads the history that waits in the file into the document (`Document::lazy_history`), as it is:
/// nothing is checked or changed. For a program that writes the plain JSON form of a document that
/// it did not edit (`kadakas convert`). A session resolves the history through `kadakas-api`,
/// which also checks it against the drawing.
pub fn resolve_history(doc: &mut Document) -> Result<(), FileError> {
    if let Some(loader) = doc.lazy_history.take() {
        match loader.load() {
            Ok(h) => doc.history = Some(h),
            Err(e) => {
                doc.lazy_history = Some(loader);
                return Err(FileError::Io(e));
            }
        }
    }
    Ok(())
}

/// A blob to write.
enum Planned {
    Picture(PngData),
    Text(BlobText),
    Base(Arc<Value>),
    History(Arc<str>),
    /// Only a live handle uses it; the old file has it.
    Kept(Arc<FileSource>),
}

fn base_text(body: &Value) -> Option<String> {
    serde_json::to_string(body.get("recs")?.as_array()?).ok()
}

struct Plans {
    blobs: BTreeMap<String, Planned>,
    bases: HashMap<usize, String>,
    sources: Vec<Arc<FileSource>>,
}

fn plan_blobs(doc: &Document, history: Option<&Plan>, opts: &WriteOptions) -> Plans {
    let mut plans = Plans {
        blobs: BTreeMap::new(),
        bases: HashMap::new(),
        sources: opts.keep.clone(),
    };
    doc.for_each_blob(&mut |b| match b {
        DocBlob::Picture(p) if p.is_blob() => {
            plans
                .blobs
                .entry(p.blob_hash())
                .or_insert_with(|| Planned::Picture(p.clone()));
        }
        DocBlob::Image(t) if t.is_blob() => {
            plans
                .blobs
                .entry(t.hash())
                .or_insert_with(|| Planned::Text(t.clone()));
        }
        DocBlob::ImportBase(body) => {
            if let Some(text) = base_text(body).filter(|t| t.len() >= MIN_BLOB_TEXT) {
                let hash = text_hash(&text);
                plans.bases.insert(Arc::as_ptr(body) as usize, hash.clone());
                plans
                    .blobs
                    .entry(hash)
                    .or_insert_with(|| Planned::Base(body.clone()));
            }
        }
        _ => {}
    });
    if let Some(Plan::Parts { blobs, .. }) = history {
        for (hash, text) in blobs {
            plans
                .blobs
                .entry(hash.clone())
                .or_insert_with(|| Planned::History(text.clone()));
        }
    }
    // Pictures that only a live handle uses (the undo steps).
    for source in &opts.keep {
        for hash in source.live_hashes() {
            plans
                .blobs
                .entry(hash)
                .or_insert_with(|| Planned::Kept(source.clone()));
        }
    }
    plans
}

/// The file behind a lazy handle, if it is a file of this crate.
fn file_of(blob: &kadakas_core::blob::LazyBlob) -> Option<Arc<FileSource>> {
    blob.source()
        .clone()
        .into_any()
        .downcast::<FileSource>()
        .ok()
}

/// Writes the container form of a document into `w`.
pub fn write_container<W: Write>(
    w: W,
    doc: &Document,
    opts: &WriteOptions,
) -> Result<(W, Written), FileError> {
    let mut writer = Writer::new(w).generator(&opts.generator);
    let mut report = Written::default();
    let history_plan = doc.history.as_ref().map(history::plan).transpose()?;
    let mut plans = plan_blobs(doc, history_plan.as_ref(), opts);
    // A history that still waits in its file is copied from there, entry by entry.
    let waiting: Option<&FileHistory> = match (&doc.history, &doc.lazy_history) {
        (None, Some(loader)) => Some(loader.as_any().downcast_ref::<FileHistory>().ok_or_else(
            || {
                FileError::Io(
                    "the versions of this document wait in a file that cannot be copied from"
                        .into(),
                )
            },
        )?),
        _ => None,
    };
    if let Some(h) = waiting {
        for hash in h.blob_hashes().map_err(FileError::Io)? {
            plans
                .blobs
                .entry(hash)
                .or_insert_with(|| Planned::Kept(h.source().clone()));
        }
        if !plans.sources.iter().any(|s| Arc::ptr_eq(s, h.source())) {
            plans.sources.push(h.source().clone());
        }
    }

    let refs = SaveRefs {
        skip_history: true,
        import_bases: std::mem::take(&mut plans.bases),
    };
    writer.add_with(DOCUMENT, Compression::Deflate(DOCUMENT_LEVEL), |sink| {
        let mut bw = BufWriter::with_capacity(256 * 1024, sink);
        with_blob_refs(refs, || doc.write_pretty(&mut bw))?;
        bw.flush()
    })?;

    for (hash, planned) in &plans.blobs {
        let name = blob_name(hash);
        let source = match planned {
            Planned::Picture(p) => p.lazy().and_then(|l| file_of(l)),
            Planned::Text(t) => t.lazy().and_then(|l| file_of(l)),
            Planned::Kept(s) => Some(s.clone()),
            _ => None,
        }
        // A text in memory that an old file has too (a picture of the history, the summaries of
        // an import base) is copied from there.
        .or_else(|| {
            opts.keep
                .iter()
                .find(|s| {
                    s.container()
                        .entry(&name)
                        .is_some_and(|e| hex(&e.sha256) == *hash)
                })
                .cloned()
        });
        if let Some(source) = source {
            if !report.sources.iter().any(|s| Arc::ptr_eq(s, &source)) {
                // The file must be the one that was opened: the size, the time of change and the
                // manifest. A part is checked against its hash when it is copied as well.
                source.check_unchanged()?;
                report.sources.push(source.clone());
            }
            let raw = source.parts().raw_part(&name)?;
            if hex(&raw.sha256) != *hash {
                return Err(ContainerError::Entry {
                    name,
                    reason: "the entry of the old file is not the picture of this hash".into(),
                }
                .into());
            }
            writer.copy_raw(raw)?;
            if matches!(planned, Planned::Kept(_)) {
                report.blobs_kept += 1;
            } else {
                report.blobs_copied += 1;
            }
            continue;
        }
        let text: Arc<str> = match planned {
            Planned::Picture(p) => p.base64().map_err(|e| ContainerError::Entry {
                name: name.clone(),
                reason: e.to_string(),
            })?,
            Planned::Text(t) => t.text().map_err(|e| ContainerError::Entry {
                name: name.clone(),
                reason: e.to_string(),
            })?,
            Planned::Base(body) => Arc::from(base_text(body).unwrap_or_default()),
            Planned::History(t) => t.clone(),
            Planned::Kept(_) => unreachable!("handled above"),
        };
        writer.add_bytes(&name, Compression::Stored, text.as_bytes())?;
        report.blobs_written += 1;
    }

    if let Some(h) = waiting {
        if !report.sources.iter().any(|s| Arc::ptr_eq(s, h.source())) {
            h.source().check_unchanged()?;
        }
        for name in h.entry_names() {
            writer.copy_raw(h.source().parts().raw_part(&name)?)?;
            report.parts_copied += 1;
        }
    }
    if let Some(plan) = &history_plan {
        for (name, bytes) in history::part_names(plan) {
            let old = opts.keep.iter().find_map(|s| {
                let c = s.container();
                let e = c.entry(name)?;
                (hex(&e.sha256) == kadakas_container::sha256_hex(bytes)).then(|| s.clone())
            });
            match old.map(|s| s.parts().raw_part(name)) {
                Some(Ok(raw)) => {
                    writer.copy_raw(raw)?;
                    report.parts_copied += 1;
                }
                // Not in an old file, or it cannot be copied: write it.
                _ => writer.add_bytes(name, history::PART_COMPRESSION, bytes)?,
            }
        }
    }
    report.entries = writer.len();
    for s in std::mem::take(&mut plans.sources) {
        if !report.sources.iter().any(|k| Arc::ptr_eq(k, &s)) {
            report.sources.push(s);
        }
    }
    Ok((writer.finish()?, report))
}
