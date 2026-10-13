//! The saved history of a document (REV-01), as the file keeps it.
//!
//! Core only holds the history and moves it with the document: it is saved in the field `history`
//! of the file, it is one value in the undo steps (a cheap clone), and a change of it is one undo
//! step (`Editor::set_history`). What the history means (versions, reverse deltas) is the
//! business of `kadakas-review`, which reads and writes the JSON. Core does one thing with it: at
//! load, a history that is not a JSON object or is too big gets a note. It is not dropped: the
//! versions of a file belong to the person, and the history is written back as it is.
//!
//! The history is local to the file. It is not part of the shared properties of collaboration.

use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// The most bytes (of JSON) that the history of a loaded file may have. A history above it is
/// noted at load (the history is kept, and read only). `kadakas-review` reads far less than this.
pub const MAX_HISTORY_BYTES: usize = 512 * 1024 * 1024;

/// The history as JSON, shared: a clone is cheap, and the check of equality stops at once for the
/// same value.
#[derive(Clone, Debug)]
pub struct HistoryData {
    value: Arc<Value>,
    bytes: usize,
    /// A number that is new for each value that is made (a clone keeps it). Caches compare it
    /// instead of the value, which can be tens of megabytes.
    serial: u64,
}

/// The source of serial numbers (an id allocator, like the editor revision).
static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A writer that counts the bytes and keeps none.
struct Counter(usize);

impl std::io::Write for Counter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(buf.len());
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl HistoryData {
    pub fn new(value: Value) -> HistoryData {
        let mut count = Counter(0);
        // Writing to a counter cannot fail.
        let _ = serde_json::to_writer(&mut count, &value);
        HistoryData {
            value: Arc::new(value),
            bytes: count.0,
            serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }

    /// Identifies this value for a cache: a clone has the same serial, and every value that was
    /// made by `new` (or read from a file) has another one, whatever its content.
    pub fn serial(&self) -> u64 {
        self.serial
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    /// The size of the JSON text in bytes.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Why the history cannot stay in a document, if it cannot.
    pub fn problem(&self) -> Option<String> {
        if !self.value.is_object() {
            Some(
                "the history of versions is not a JSON object: it is kept as it is and read only"
                    .into(),
            )
        } else if self.bytes > MAX_HISTORY_BYTES {
            Some(format!(
                "the history of versions has {} bytes, more than {MAX_HISTORY_BYTES}: it is kept as it is and read only",
                self.bytes
            ))
        } else {
            None
        }
    }
}

impl PartialEq for HistoryData {
    fn eq(&self, other: &Self) -> bool {
        self.serial == other.serial
            || Arc::ptr_eq(&self.value, &other.value)
            || self.value == other.value
    }
}

impl Serialize for HistoryData {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.value.serialize(s)
    }
}

impl<'de> Deserialize<'de> for HistoryData {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(HistoryData::new(Value::deserialize(d)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn counts_bytes_and_compares_by_value() {
        let a = HistoryData::new(json!({"format": 1, "versions": []}));
        assert_eq!(a.bytes(), a.value().to_string().len());
        let b = a.clone();
        assert_eq!(a, b);
        assert_eq!(a.serial(), b.serial());
        let same_text = HistoryData::new(json!({"format": 1, "versions": []}));
        assert_ne!(
            a.serial(),
            same_text.serial(),
            "a new value has a new serial"
        );
        assert_eq!(a, same_text, "the content is still equal");
        assert_eq!(a, HistoryData::new(json!({"versions": [], "format": 1})));
        assert_ne!(a, HistoryData::new(json!({"format": 2})));
        assert!(a.problem().is_none());
    }

    #[test]
    fn problems_are_named() {
        assert!(HistoryData::new(json!([1])).problem().is_some());
        assert!(
            HistoryData::new(json!("x"))
                .problem()
                .unwrap()
                .contains("object")
        );
        let mut big = HistoryData::new(json!({}));
        big.bytes = MAX_HISTORY_BYTES + 1;
        assert!(big.problem().unwrap().contains("limit"));
    }

    #[test]
    fn json_round_trip_keeps_the_value() {
        let a = HistoryData::new(json!({"format": 1}));
        let text = serde_json::to_string(&a).unwrap();
        assert_eq!(text, r#"{"format":1}"#);
        let back: HistoryData = serde_json::from_str(&text).unwrap();
        assert_eq!(back, a);
    }
}
