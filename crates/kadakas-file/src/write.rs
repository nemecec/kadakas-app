//! Writing a document: the container and the plain JSON form.
//!
//! A container is written as a stream: `document.json` first (deflate, the pictures as
//! references), then the blobs (stored, once each, sorted by hash), then the history, and the
//! manifest last. A blob that is still in the file the document was opened from is copied from
//! that file without decoding it (`Writer::copy_raw`); so is a history part that did not change.
//! A picture that the document no longer has, but that a live handle (an undo step) still uses,
//! is kept in the new file too, so that an undo after a save finds it.
//!
//! Everything that is copied is found by its name and SHA-256 (see `kadakas_container`): the file
//! may have a new stamp, or be replaced by a file that still has the part, or the part may be in
//! the `.bak`. What cannot be found anywhere is reported before anything is written
//! ([`FileError::Missing`]), and so are the limits of the format ([`FileError::Limit`]): a save
//! writes the whole file or nothing, and a file that this code writes always opens.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{BufWriter, Read, Write};
use std::sync::Arc;

use kadakas_container::{
    Compression, ContainerError, MANIFEST_NAME, MAX_ENTRIES, PartSource, Writer, check_entry_count,
    hex, parse_hash,
};
use kadakas_core::blob::{
    BlobText, MAX_BLOB_TEXT, MIN_BLOB_TEXT, SaveRefs, text_hash, with_blob_refs,
};
use kadakas_core::model::{DocBlob, Document};
use kadakas_core::underlay::PngData;
use serde_json::Value;

use crate::error::{FileError, Missing};
use crate::history::{self, Plan};
use crate::layout::{BLOB_DIR, DOCUMENT, HISTORY_ANCHOR, HISTORY_ANCHOR_BLOBS, blob_name};
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
    /// The document was edited since it was read from the file, and its history waits in the
    /// file (it was not read). The drawing of the file is written as the anchor of the history
    /// (`history/anchor.json`), so that the versions can still be read against the drawing they
    /// belong to.
    pub anchor: bool,
    /// The person chose to save without what cannot be found: pictures that only an undo step
    /// holds, the anchor drawing, and the entries of a newer program are left out (and counted in
    /// [`Written::skipped`]). A picture of the drawing is never skipped: the caller replaces it
    /// first (`Document::replace_missing_pictures`).
    pub skip_missing: bool,
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
    /// Entries of the old file that this code does not know (from a newer program), copied as
    /// they are.
    pub unknown_copied: usize,
    /// The drawing of the old file was written as the anchor of the history.
    pub anchor_written: bool,
    /// Parts that could not be found and were left out (`WriteOptions::skip_missing`).
    pub skipped: usize,
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
    /// Copied from a file: only a live handle uses it, or the history or its anchor.
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

impl Planned {
    /// The file that gives this blob, if it comes from a file. A text in memory (also one that a
    /// file has too) is written from memory, because the file may have changed.
    fn file(&self) -> Option<Arc<FileSource>> {
        match self {
            Planned::Picture(p) => p.lazy().and_then(|l| file_of(l)),
            Planned::Text(t) => t.lazy().and_then(|l| file_of(l)),
            Planned::Kept(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// The length of the text in bytes.
    fn len(&self) -> usize {
        match self {
            Planned::Picture(p) => p.text_len(),
            Planned::Text(t) => t.len(),
            // An import base is at most 8 MiB (the limit of the resource).
            Planned::Base(_) => 0,
            Planned::History(t) => t.len(),
            Planned::Kept(_) => 0,
        }
    }
}

/// The pictures that the drawing in `entry` refers to: every `{"$blob": "<hash>"}` of the text,
/// found by a scan of the stream (the document is not parsed, so this costs no memory). A text
/// that looks like a reference but is not one gives a hash that no entry has; the caller keeps
/// only the hashes that the file has.
pub(crate) fn scan_refs(source: &FileSource, entry: &str) -> Result<BTreeSet<String>, FileError> {
    let container = source.container_with_entry(entry)?;
    let mut reader = container.reader(entry)?;
    let mut out = BTreeSet::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 256 * 1024];
    let key = b"\"$blob\"";
    loop {
        let n = reader.read(&mut chunk).map_err(FileError::from)?;
        buf.extend_from_slice(&chunk[..n]);
        let eof = n == 0;
        let mut at = 0usize;
        let mut keep_from = buf.len();
        while let Some(i) = find(&buf[at..], key).map(|i| i + at) {
            // After the key: white space, a colon, white space, a quote, 64 digits, a quote.
            let mut j = i + key.len();
            let skip = |j: &mut usize, b: &[u8]| {
                while *j < b.len() && b[*j].is_ascii_whitespace() {
                    *j += 1;
                }
            };
            skip(&mut j, &buf);
            if j >= buf.len() {
                if !eof {
                    keep_from = i;
                    break;
                }
                break;
            }
            if buf[j] != b':' {
                at = i + 1;
                continue;
            }
            j += 1;
            skip(&mut j, &buf);
            if j + 66 > buf.len() {
                if !eof {
                    keep_from = i;
                    break;
                }
                break;
            }
            if buf[j] == b'"' && buf[j + 65] == b'"' {
                if let Ok(h) = std::str::from_utf8(&buf[j + 1..j + 65])
                    && kadakas_core::blob::is_hash(h)
                {
                    out.insert(h.to_string());
                }
            }
            at = i + 1;
        }
        if eof {
            break;
        }
        // Keep the end of the buffer: a reference can be cut by the chunk.
        let tail_from = keep_from.min(buf.len().saturating_sub(key.len() + 80));
        buf.drain(..tail_from);
    }
    Ok(out)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// What a container save writes besides the drawing and the blobs: the entries of the history
/// that wait in a file, the anchor, and the entries of the old file that this code does not know.
struct Extras {
    /// The history that waits in its file.
    waiting: Option<Arc<FileHistory>>,
    /// The drawing of the old file as the anchor of the waiting history, and the pictures it
    /// refers to.
    anchor: Option<(Arc<FileSource>, Vec<String>)>,
    /// Entries of the old files with another name than the ones this code writes.
    unknown: Vec<(Arc<FileSource>, String)>,
}

fn waiting_history(doc: &Document) -> Result<Option<Arc<FileHistory>>, FileError> {
    match (&doc.history, &doc.lazy_history) {
        (None, Some(loader)) => {
            let h = loader.as_any().downcast_ref::<FileHistory>().cloned();
            match h {
                Some(h) => Ok(Some(Arc::new(h))),
                None => Err(FileError::Io(
                    "the versions of this document wait in a file that cannot be copied from"
                        .into(),
                )),
            }
        }
        _ => Ok(None),
    }
}

/// The entries of the files that the document was opened from that this code does not write itself
/// (a newer program may add entries): they are copied, not dropped.
fn unknown_entries(
    sources: &[Arc<FileSource>],
    known: &dyn Fn(&str) -> bool,
) -> Vec<(Arc<FileSource>, String)> {
    let mut out: Vec<(Arc<FileSource>, String)> = Vec::new();
    for s in sources {
        for e in s.container().entries() {
            let n = e.name.as_str();
            if n == DOCUMENT
                || n == MANIFEST_NAME
                || n.starts_with(BLOB_DIR)
                || n.starts_with("history/")
                || known(n)
                || out.iter().any(|(_, o)| o == n)
            {
                continue;
            }
            out.push((s.clone(), e.name.clone()));
        }
    }
    out
}

/// Checks the limits of the format and finds what cannot be copied, before anything is written.
/// With `skip` (the person chose to save without what is missing) the parts that only an undo step,
/// the anchor or a newer program's entry need are left out and counted; a picture of the drawing
/// and the versions that wait in the file cannot be skipped here (the caller replaces the pictures
/// and drops the versions first).
fn preflight(
    plans: &mut Plans,
    history_parts: usize,
    extras: &mut Extras,
    root: Option<&Arc<FileSource>>,
    skip: bool,
) -> Result<usize, FileError> {
    // The size of the texts: a file that is written must be a file that can be read.
    for (hash, planned) in &plans.blobs {
        let len = planned.len();
        if len > MAX_BLOB_TEXT {
            let short: String = hash.chars().take(8).collect();
            return Err(FileError::Limit(format!(
                "a picture ({short}) has {len} bytes as text, more than the {MAX_BLOB_TEXT} bytes (128 MiB) that a container holds. Reduce the picture, or save the plain JSON form"
            )));
        }
    }
    let mut missing = Missing {
        file: root.map_or_else(String::new, |s| s.label()),
        ..Missing::default()
    };
    let mut skipped = 0usize;
    let mut drop_blobs: Vec<String> = Vec::new();
    for (hash, planned) in &plans.blobs {
        if let Some(source) = planned.file()
            && !source.has_blob(hash)
        {
            if missing.file.is_empty() {
                missing.file = source.label();
            }
            if skip && matches!(planned, Planned::Kept(_)) {
                drop_blobs.push(hash.clone());
            } else {
                missing.pictures.push(hash.clone());
            }
        }
    }
    for h in &drop_blobs {
        plans.blobs.remove(h);
        skipped += 1;
    }
    if let Some(h) = &extras.waiting {
        for name in h.entry_names() {
            if !h.source().has_entry(&name) {
                missing.history = true;
            }
        }
    }
    if let Some((source, hashes)) = &extras.anchor {
        let mut gone = !source.has_entry(DOCUMENT);
        for hash in hashes {
            if !source.has_blob(hash) {
                gone = true;
            }
        }
        if gone {
            if skip {
                extras.anchor = None;
                skipped += 1;
            } else {
                missing.history = true;
            }
        }
    }
    let unknown = std::mem::take(&mut extras.unknown);
    for (source, name) in unknown {
        if source.has_entry(&name) {
            extras.unknown.push((source, name));
        } else if skip {
            skipped += 1;
        } else {
            missing.other.push(name);
        }
    }
    if !missing.is_empty() {
        missing.pictures.sort();
        missing.pictures.dedup();
        return Err(FileError::Missing(missing));
    }
    let waiting = extras.waiting.as_ref().map_or(0, |h| h.entry_names().len());
    let anchor = extras.anchor.as_ref().map_or(0, |_| 2);
    let planned = 1 + plans.blobs.len() + history_parts + waiting + anchor + extras.unknown.len();
    check_entry_count(planned).map_err(|_| {
        FileError::Limit(format!(
            "the document would have {planned} entries in a container, more than the {MAX_ENTRIES} that it holds. Save the plain JSON form instead"
        ))
    })?;
    Ok(skipped)
}

/// Copies an entry of an old file as it is. The entry is the one with this name and hash (it may
/// be found in the file at the path, or in its `.bak`).
fn copy_entry<W: Write>(
    writer: &mut Writer<W>,
    source: &FileSource,
    name: &str,
    new_name: Option<&str>,
) -> Result<(), FileError> {
    let sha = source
        .parts()
        .part_sha256(name)
        .ok_or_else(|| ContainerError::Entry {
            name: name.to_string(),
            reason: "it is not in the file that was opened".into(),
        })?;
    let mut raw = source.parts().raw_by_content(name, &sha)?;
    if let Some(n) = new_name {
        raw.name = n.to_string();
    }
    writer.copy_raw(raw)?;
    Ok(())
}

/// Writes the container form of a document into `w`.
pub fn write_container<W: Write>(
    w: W,
    doc: &Document,
    opts: &WriteOptions,
) -> Result<(W, Written), FileError> {
    let history_plan = doc.history.as_ref().map(history::plan).transpose()?;
    let mut plans = plan_blobs(doc, history_plan.as_ref(), opts);
    let waiting = waiting_history(doc)?;
    let mut extras = Extras {
        waiting: waiting.clone(),
        anchor: None,
        unknown: Vec::new(),
    };
    if let Some(h) = &waiting {
        // The history waits in a file: its entries and its pictures are copied from there.
        match h.blob_hashes() {
            Ok(hashes) => {
                for hash in hashes {
                    plans
                        .blobs
                        .entry(hash)
                        .or_insert_with(|| Planned::Kept(h.source().clone()));
                }
            }
            // The file is gone or another file: the versions cannot be found.
            Err(e) if e.contains("changed on disk") => {
                return Err(FileError::Missing(Missing {
                    file: h.source().label(),
                    history: true,
                    ..Missing::default()
                }));
            }
            // The list is damaged: the error names the entry.
            Err(e) => return Err(FileError::Io(e)),
        }
        if !plans.sources.iter().any(|s| Arc::ptr_eq(s, h.source())) {
            plans.sources.push(h.source().clone());
        }
        // An edited drawing: the drawing of the file is the anchor of the versions.
        if opts.anchor && !h.source().container().contains(HISTORY_ANCHOR) {
            let refs = scan_refs(h.source(), DOCUMENT)?;
            let c = h.source().container();
            let hashes: Vec<String> = refs
                .into_iter()
                .filter(|r| c.contains(&blob_name(r)))
                .collect();
            for hash in &hashes {
                plans
                    .blobs
                    .entry(hash.clone())
                    .or_insert_with(|| Planned::Kept(h.source().clone()));
            }
            extras.anchor = Some((h.source().clone(), hashes));
        }
    }
    let history_names: Vec<(&'static str, &[u8])> = history_plan
        .as_ref()
        .map(history::part_names)
        .unwrap_or_default();
    let all_sources = plans.sources.clone();
    let known = |n: &str| history_names.iter().any(|(k, _)| *k == n);
    extras.unknown = unknown_entries(&all_sources, &known);
    let skipped = preflight(
        &mut plans,
        history_names.len(),
        &mut extras,
        waiting
            .as_ref()
            .map(|h| h.source())
            .or_else(|| opts.keep.first()),
        opts.skip_missing,
    )?;

    let mut writer = Writer::new(w).generator(&opts.generator);
    let mut report = Written {
        skipped,
        ..Written::default()
    };
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
        if let Some(source) = planned.file() {
            if !report.sources.iter().any(|s| Arc::ptr_eq(s, &source)) {
                report.sources.push(source.clone());
            }
            let sha = parse_hash(hash).ok_or_else(|| ContainerError::Entry {
                name: name.clone(),
                reason: "not a hash".into(),
            })?;
            let raw = source.parts().raw_by_content(&name, &sha)?;
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
        if matches!(planned, Planned::Base(_)) {
            writer.set_role(&name, crate::layout::ROLE_IMPORT_BASE);
        }
        report.blobs_written += 1;
    }

    if let Some(h) = &waiting {
        for name in h.entry_names() {
            copy_entry(&mut writer, h.source(), &name, None)?;
            report.parts_copied += 1;
        }
    }
    if let Some((source, hashes)) = &extras.anchor {
        copy_entry(&mut writer, source, DOCUMENT, Some(HISTORY_ANCHOR))?;
        let list = serde_json::to_vec(hashes)
            .map_err(|e| FileError::Io(format!("the anchor list cannot be written: {e}")))?;
        writer.add_bytes(HISTORY_ANCHOR_BLOBS, history::PART_COMPRESSION, &list)?;
        report.anchor_written = true;
    }
    if let Some(plan) = &history_plan {
        for (name, bytes) in history::part_names(plan) {
            // A part that is equal to the one of an old file is copied without deflating it
            // again. The bytes in memory are the truth: if the old file cannot give the part,
            // it is written from memory.
            let want = kadakas_container::sha256_hex(bytes);
            let old = opts.keep.iter().find(|s| {
                s.container()
                    .entry(name)
                    .is_some_and(|e| hex(&e.sha256) == want)
            });
            match old.map(|s| s.parts().raw_part(name)) {
                Some(Ok(raw)) => {
                    if !report.sources.iter().any(|k| Arc::ptr_eq(k, old.unwrap())) {
                        report.sources.push(old.unwrap().clone());
                    }
                    writer.copy_raw(raw)?;
                    report.parts_copied += 1;
                }
                _ => writer.add_bytes(name, history::PART_COMPRESSION, bytes)?,
            }
        }
    }
    for (source, name) in &extras.unknown {
        copy_entry(&mut writer, source, name, None)?;
        if !report.sources.iter().any(|s| Arc::ptr_eq(s, source)) {
            report.sources.push(source.clone());
        }
        report.unknown_copied += 1;
    }
    report.entries = writer.len();
    for s in std::mem::take(&mut plans.sources) {
        if !report.sources.iter().any(|k| Arc::ptr_eq(k, &s)) {
            report.sources.push(s);
        }
    }
    Ok((writer.finish()?, report))
}
