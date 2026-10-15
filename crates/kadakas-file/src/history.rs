//! The history of versions (REV-01) as entries of a container.
//!
//! `HistoryData` holds the history in parts: the value without the lists (`top`), the versions,
//! the operations of the newest change and the table of pictures. A history of the usual shape
//! is stored in the same parts: `history/top.json`, `history/versions.json`,
//! `history/newest.json` and `history/blobs.json` (the hashes of its pictures, in order), and the
//! pictures are entries of `blobs/`, shared with the pictures of the drawing. A history of any
//! other shape is stored whole in `history/raw.json`. Nothing is dropped or changed: the
//! history that is read is equal to the history that was written (`HistoryData` equality).
//!
//! What is *in* the history is the business of `kadakas-review`; this module only moves the
//! parts. A history that cannot be read as a part is an error that names the entry; a history
//! that `kadakas-review` cannot read completely is still read here as it is (the opaque,
//! read-only rule of REV-01).

use std::sync::Arc;

use kadakas_container::{Compression, Container, ContainerError, PartSource};
use kadakas_core::blob::{is_hash, text_hash};
use kadakas_core::history::HistoryData;
use serde_json::Value;

use crate::error::FileError;
use crate::layout::{
    HISTORY_BLOBS, HISTORY_NEWEST, HISTORY_RAW, HISTORY_TOP, HISTORY_VERSIONS, blob_name,
};
use crate::source::FileSource;

/// The parts of a history, ready to be written.
pub(crate) enum Plan {
    /// The usual shape: the parts and the pictures.
    Parts {
        top: Vec<u8>,
        versions: Vec<u8>,
        newest: Vec<u8>,
        blob_hashes: Vec<u8>,
        blobs: Vec<(String, Arc<str>)>,
    },
    /// Any other shape: the whole value.
    Raw(Vec<u8>),
}

fn json_bytes(v: &impl serde::Serialize) -> Result<Vec<u8>, FileError> {
    serde_json::to_vec(v).map_err(|e| FileError::Io(format!("the history cannot be written: {e}")))
}

/// Cuts a history into parts.
pub(crate) fn plan(h: &HistoryData) -> Result<Plan, FileError> {
    let regular = h.top().is_object()
        && h.top().get("newest").is_some_and(Value::is_object)
        && h.version_items().is_some()
        && h.newest_items().is_some()
        && h.has_blob_table()
        && h.blob_list()
            .iter()
            .all(|(k, t)| is_hash(k) && text_hash(t) == *k)
        && {
            let mut seen = std::collections::HashSet::new();
            h.blob_list().iter().all(|(k, _)| seen.insert(k.as_str()))
        };
    if !regular {
        // The value with the pictures inline. Any value can be written like this.
        return Ok(Plan::Raw(json_bytes(&h.to_value())?));
    }
    let versions: Vec<&Value> = h
        .version_items()
        .unwrap_or_default()
        .iter()
        .map(|v| &**v)
        .collect();
    let newest: Vec<&Value> = h
        .newest_items()
        .unwrap_or_default()
        .iter()
        .map(|v| &**v)
        .collect();
    let hashes: Vec<&str> = h.blob_list().iter().map(|(k, _)| k.as_str()).collect();
    Ok(Plan::Parts {
        top: json_bytes(h.top())?,
        versions: json_bytes(&versions)?,
        newest: json_bytes(&newest)?,
        blob_hashes: json_bytes(&hashes)?,
        blobs: h.blob_list().to_vec(),
    })
}

/// The names of the entries a plan writes (the pictures excluded).
pub(crate) fn part_names(plan: &Plan) -> Vec<(&'static str, &[u8])> {
    match plan {
        Plan::Parts {
            top,
            versions,
            newest,
            blob_hashes,
            ..
        } => vec![
            (HISTORY_TOP, top.as_slice()),
            (HISTORY_VERSIONS, versions.as_slice()),
            (HISTORY_NEWEST, newest.as_slice()),
            (HISTORY_BLOBS, blob_hashes.as_slice()),
        ],
        Plan::Raw(bytes) => vec![(HISTORY_RAW, bytes.as_slice())],
    }
}

/// True if the container holds a history.
pub(crate) fn present(c: &Container) -> bool {
    c.contains(HISTORY_RAW) || c.contains(HISTORY_TOP)
}

/// The hash of the blob that an entry name stands for, as bytes (the SHA-256 of the text).
fn sha_of(hash: &str) -> Option<[u8; 32]> {
    kadakas_container::parse_hash(hash)
}

/// Reads an entry of the history by name and the SHA-256 that the opened file gives it.
fn read_part(source: &FileSource, name: &str) -> Result<Vec<u8>, FileError> {
    Ok(source.parts().read_part(name)?)
}

fn json_array(source: &FileSource, name: &str) -> Result<Vec<Arc<Value>>, FileError> {
    let bytes = read_part(source, name)?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| ContainerError::Entry {
        name: name.to_string(),
        reason: format!("it cannot be read as JSON: {e}"),
    })?;
    match v {
        Value::Array(items) => Ok(items.into_iter().map(Arc::new).collect()),
        _ => Err(ContainerError::Entry {
            name: name.to_string(),
            reason: "it is not a list".into(),
        }
        .into()),
    }
}

/// The pictures that `history/blobs.json` lists. Each is a hash once: a list with a hash twice
/// would make a reader read and keep the same picture again for each mention.
pub(crate) fn listed_blobs(source: &FileSource) -> Result<Vec<String>, FileError> {
    let c = source.container();
    if !c.contains(HISTORY_BLOBS) {
        return Ok(Vec::new());
    }
    let list = json_array(source, HISTORY_BLOBS)?;
    let mut seen = std::collections::HashSet::with_capacity(list.len());
    let mut out = Vec::with_capacity(list.len());
    for h in &list {
        let hash = h
            .as_str()
            .filter(|h| is_hash(h))
            .ok_or_else(|| ContainerError::Entry {
                name: HISTORY_BLOBS.to_string(),
                reason: "it lists something that is not a hash".into(),
            })?;
        if !seen.insert(hash) {
            return Err(ContainerError::Entry {
                name: HISTORY_BLOBS.to_string(),
                reason: format!("it lists the picture {hash} twice"),
            }
            .into());
        }
        out.push(hash.to_string());
    }
    Ok(out)
}

/// The bytes of the pictures that `history/blobs.json` lists, from the directory (nothing is read).
/// Used to bound a read of the history before it is made.
pub(crate) fn listed_blob_bytes(source: &FileSource) -> u64 {
    let c = source.container();
    listed_blobs(source)
        .unwrap_or_default()
        .iter()
        .filter_map(|h| c.entry(&blob_name(h)).map(|e| e.size))
        .sum()
}

/// Reads the history of a container, if it has one. The pictures are read now (the history is
/// read when it is used, not with the drawing, see `kadakas_core::history`). Every part is read
/// by its name and hash from the file (or its `.bak`, if the file was replaced). A picture of the
/// history is read once, and its entry must be the picture of its hash.
pub(crate) fn read(source: &FileSource) -> Result<Option<HistoryData>, FileError> {
    let c = source.container();
    if c.contains(HISTORY_RAW) {
        let bytes = read_part(source, HISTORY_RAW)?;
        let v: Value = serde_json::from_slice(&bytes).map_err(|e| ContainerError::Entry {
            name: HISTORY_RAW.to_string(),
            reason: format!("it cannot be read as JSON: {e}"),
        })?;
        return Ok(Some(HistoryData::new(v)));
    }
    if !c.contains(HISTORY_TOP) {
        return Ok(None);
    }
    let top: Value = serde_json::from_slice(&read_part(source, HISTORY_TOP)?).map_err(|e| {
        ContainerError::Entry {
            name: HISTORY_TOP.to_string(),
            reason: format!("it cannot be read as JSON: {e}"),
        }
    })?;
    let versions = json_array(source, HISTORY_VERSIONS)?;
    let newest = json_array(source, HISTORY_NEWEST)?;
    let hashes = listed_blobs(source)?;
    // The bytes of the pictures are known from the directory before one is read.
    let mut budget = c.uncompressed_bytes();
    let mut blobs = Vec::with_capacity(hashes.len());
    for hash in &hashes {
        let name = blob_name(hash);
        let entry = c.entry(&name).ok_or_else(|| ContainerError::Entry {
            name: name.clone(),
            reason: "the history lists this picture, but it is not in the container".into(),
        })?;
        if entry.size > kadakas_core::blob::MAX_BLOB_TEXT as u64 {
            return Err(ContainerError::Entry {
                name,
                reason: "it is bigger than a picture of the history may be".into(),
            }
            .into());
        }
        budget = budget
            .checked_sub(entry.size)
            .ok_or_else(|| ContainerError::Entry {
                name: name.clone(),
                reason: "the pictures of the history are bigger together than the file".into(),
            })?;
        let sha = sha_of(hash).ok_or_else(|| ContainerError::Entry {
            name: name.clone(),
            reason: "not a hash".into(),
        })?;
        if entry.sha256 != sha {
            return Err(ContainerError::Entry {
                name,
                reason: "the entry is not the picture of its name (its SHA-256 is another)".into(),
            }
            .into());
        }
        // A picture that this load has read already (an eager read of the drawing) is the same
        // memory in the history.
        if let Some(text) = kadakas_core::blob::loaded_text(hash) {
            blobs.push((hash.clone(), text));
            continue;
        }
        let bytes = source.parts().read_by_content(&name, &sha)?;
        let text = String::from_utf8(bytes).map_err(|_| ContainerError::Entry {
            name: name.clone(),
            reason: "it is not text".into(),
        })?;
        let text = Arc::<str>::from(text);
        kadakas_core::blob::remember_text(hash, &text);
        blobs.push((hash.clone(), text));
    }
    Ok(Some(HistoryData::from_parts(top, versions, newest, blobs)))
}

/// The compression of a history part: text, deflated.
pub(crate) const PART_COMPRESSION: Compression = Compression::Deflate(6);
