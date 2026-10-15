//! A history that waits in a file is bounded before it is read.

use std::sync::Arc;

use kadakas_core::history::{HistoryData, HistoryLoader};
use kadakas_core::model::Document;
use serde_json::json;

use super::*;

/// A loader that says that its history is huge and panics when it is read.
#[derive(Debug)]
struct Huge(usize);

impl HistoryLoader for Huge {
    fn load(&self) -> Result<HistoryData, String> {
        panic!("a history over the bound must not be read");
    }

    fn anchor(&self) -> Result<Document, String> {
        panic!("a history over the bound must not be read");
    }

    fn bytes(&self) -> usize {
        self.0
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn a_history_over_the_bound_is_not_read_and_stays_read_only() {
    let mut doc = Document::with_sequential_ids();
    let limits = SessionLimits::default();
    let max = limits.read_caps().max_bytes.saturating_mul(JSON_FACTOR);
    doc.lazy_history = Some(Arc::new(Huge(max + 1)));
    let mut s = Session::with_document(doc);
    let list = s.handle("version.list", json!({"current": false})).unwrap();
    assert_eq!(list["read_only"], true, "{list}");
    assert!(
        list["notes"].to_string().contains("more than is read"),
        "{list}"
    );
    // Asking again does not read it either.
    assert!(s.handle("version.list", json!({"current": false})).is_ok());
}
