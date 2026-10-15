//! The history of a document that waits in its file (DOC-21).

use std::sync::Arc;

use kadakas_core::history::{HistoryData, HistoryLoader};
use kadakas_core::model::Document;

use crate::history;
use crate::layout::HISTORY_BLOBS;
use crate::read::{ReadMode, read_source};
use crate::source::FileSource;

/// The history entries of an open container. A document that was opened from a container holds
/// this as `Document::lazy_history`, and the session reads the history when a program uses it.
#[derive(Debug)]
pub struct FileHistory {
    source: Arc<FileSource>,
    bytes: usize,
}

impl FileHistory {
    /// A loader over the history entries of the source, if it has any.
    pub fn of(source: &Arc<FileSource>) -> Option<Arc<FileHistory>> {
        let c = source.container();
        if !history::present(&c) {
            return None;
        }
        let bytes = c
            .entries()
            .iter()
            .filter(|e| e.name.starts_with("history/"))
            .map(|e| e.size)
            .sum::<u64>();
        Some(Arc::new(FileHistory {
            source: source.clone(),
            bytes: usize::try_from(bytes).unwrap_or(usize::MAX),
        }))
    }

    /// The file that holds the entries.
    pub fn source(&self) -> &Arc<FileSource> {
        &self.source
    }

    /// The names of the history entries, in order.
    pub fn entry_names(&self) -> Vec<String> {
        let c = self.source.container();
        c.entries()
            .iter()
            .filter(|e| e.name.starts_with("history/"))
            .map(|e| e.name.clone())
            .collect()
    }

    /// The hashes of the pictures that the history holds (the entries of `blobs/` it refers to).
    pub fn blob_hashes(&self) -> Result<Vec<String>, String> {
        let c = self.source.container();
        if !c.contains(HISTORY_BLOBS) {
            return Ok(Vec::new());
        }
        let bytes = c.read(HISTORY_BLOBS).map_err(|e| e.to_string())?;
        serde_json::from_slice::<Vec<String>>(&bytes)
            .map_err(|e| format!("the entry \"{HISTORY_BLOBS}\" cannot be read: {e}"))
    }
}

impl HistoryLoader for FileHistory {
    fn load(&self) -> Result<HistoryData, String> {
        history::read(&self.source.container())
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "the file holds no history".to_string())
    }

    fn anchor(&self) -> Result<Document, String> {
        read_source(&self.source, ReadMode::Lazy).map_err(|e| e.to_string())
    }

    fn bytes(&self) -> usize {
        self.bytes
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
