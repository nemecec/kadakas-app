//! The history of a document that waits in its file (DOC-21).

use std::sync::Arc;

use kadakas_container::PartSource;
use kadakas_core::history::{HistoryData, HistoryLoader};
use kadakas_core::model::Document;

use crate::history;
use crate::layout::{HISTORY_ANCHOR, HISTORY_ANCHOR_BLOBS};
use crate::read::{ReadMode, read_entry_document};
use crate::source::FileSource;

/// The history entries of an open container. A document that was opened from a container holds
/// this as `Document::lazy_history`, and the session reads the history when a program uses it.
#[derive(Clone, Debug)]
pub struct FileHistory {
    source: Arc<FileSource>,
    /// The bytes of the parts and of the pictures that they list (known from the directory).
    bytes: usize,
}

/// True for the entries that are no part of the history value: the anchor drawing and its list.
pub(crate) fn is_anchor_entry(name: &str) -> bool {
    name == HISTORY_ANCHOR || name == HISTORY_ANCHOR_BLOBS
}

impl FileHistory {
    /// A loader over the history entries of the source, if it has any.
    pub fn of(source: &Arc<FileSource>) -> Option<Arc<FileHistory>> {
        let c = source.container();
        if !history::present(&c) {
            return None;
        }
        let parts = c
            .entries()
            .iter()
            .filter(|e| e.name.starts_with("history/") && !is_anchor_entry(&e.name))
            .map(|e| e.size)
            .sum::<u64>();
        let bytes = parts.saturating_add(history::listed_blob_bytes(source));
        Some(Arc::new(FileHistory {
            source: source.clone(),
            bytes: usize::try_from(bytes).unwrap_or(usize::MAX),
        }))
    }

    /// The file that holds the entries.
    pub fn source(&self) -> &Arc<FileSource> {
        &self.source
    }

    /// True if the file has its own drawing for the history (see [`crate::HISTORY_ANCHOR`]). The
    /// file can change under the loader (a save to a new path moves the source to the new file),
    /// so it is asked each time.
    fn anchored(&self) -> bool {
        self.source.container().contains(HISTORY_ANCHOR)
    }

    /// The names of the history entries, in order (the anchor drawing and its list included).
    pub fn entry_names(&self) -> Vec<String> {
        let c = self.source.container();
        c.entries()
            .iter()
            .filter(|e| e.name.starts_with("history/"))
            .map(|e| e.name.clone())
            .collect()
    }

    /// The hashes of the pictures that the history holds (the entries of `blobs/` it refers to),
    /// and those of its anchor drawing.
    pub fn blob_hashes(&self) -> Result<Vec<String>, String> {
        let mut out = history::listed_blobs(&self.source).map_err(|e| e.to_string())?;
        if self.source.container().contains(HISTORY_ANCHOR_BLOBS) {
            let bytes = self
                .source
                .parts()
                .read_part(HISTORY_ANCHOR_BLOBS)
                .map_err(|e| e.to_string())?;
            let more: Vec<String> = serde_json::from_slice(&bytes)
                .map_err(|e| format!("the entry \"{HISTORY_ANCHOR_BLOBS}\" cannot be read: {e}"))?;
            for h in more {
                if !out.contains(&h) {
                    out.push(h);
                }
            }
        }
        Ok(out)
    }
}

impl HistoryLoader for FileHistory {
    fn load(&self) -> Result<HistoryData, String> {
        history::read(&self.source)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "the file holds no history".to_string())
    }

    fn anchor(&self) -> Result<Document, String> {
        // The drawing that the versions were saved with: the anchor drawing of the file, else the
        // drawing of the file. It is read to be looked at, so its pictures are not noted as in
        // use.
        let entry = if self.anchored() {
            HISTORY_ANCHOR
        } else {
            crate::layout::DOCUMENT
        };
        read_entry_document(&self.source, entry, ReadMode::Untracked, false)
            .map_err(|e| e.to_string())
    }

    fn bytes(&self) -> usize {
        self.bytes
    }

    fn separate_anchor(&self) -> bool {
        self.anchored()
    }

    fn state_key(&self) -> String {
        #[cfg(not(target_arch = "wasm32"))]
        {
            // The state of the file and of its .bak: the versions can be found in either.
            let stamp = |p: &std::path::Path| {
                std::fs::metadata(p).map_or_else(
                    |_| "none".to_string(),
                    |m| {
                        let t = m
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map_or(0, |d| d.as_nanos());
                        format!("{}:{t}", m.len())
                    },
                )
            };
            if let Some(path) = self.source.path() {
                let mut bak = path.clone().into_os_string();
                bak.push(".bak");
                return format!(
                    "{}:{}:{}",
                    path.display(),
                    stamp(&path),
                    stamp(std::path::Path::new(&bak))
                );
            }
        }
        String::new()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
