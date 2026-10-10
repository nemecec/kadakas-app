//! Worksheet results, cached by document revision.
//!
//! The document revision changes with every command and every undo or redo. A cached result is
//! valid for its revision only. The cache keeps results of one revision (see `RevCache`), so it
//! stays small.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use kadakas_core::cache::RevCache;
use kadakas_core::model::Document;
use kadakas_core::uid::Uid;

use crate::compute::{Computed, compute_with};
use crate::def::WorksheetDef;

/// Results of worksheets by (revision, worksheet ID).
pub struct WorksheetCache {
    inner: Mutex<RevCache<Uid, Arc<Computed>>>,
    runs: AtomicUsize,
}

/// Most worksheet results that the cache keeps.
const MAX_RESULTS: usize = 256;

impl Default for WorksheetCache {
    fn default() -> Self {
        WorksheetCache {
            inner: Mutex::new(RevCache::new(MAX_RESULTS)),
            runs: AtomicUsize::new(0),
        }
    }
}

impl WorksheetCache {
    /// The result for a worksheet at this revision. It is calculated once per revision.
    pub fn get(
        &self,
        revision: u64,
        sheet_id: Uid,
        def: &WorksheetDef,
        doc: &Document,
    ) -> Arc<Computed> {
        self.get_with(
            revision,
            sheet_id,
            def,
            doc,
            &kadakas_core::ground::NoTerrain,
        )
    }

    /// Like `get`, with the ground of the site models. The ground must belong to this revision.
    pub fn get_with(
        &self,
        revision: u64,
        sheet_id: Uid,
        def: &WorksheetDef,
        doc: &Document,
        ground: &dyn kadakas_core::ground::GroundSource,
    ) -> Arc<Computed> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = guard.get(revision, &sheet_id) {
            return c;
        }
        // Calculate without the lock held, so that a long calculation does not block readers of other sheets.
        drop(guard);
        let result = Arc::new(compute_with(def, doc, ground));
        self.runs.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.insert(revision, sheet_id, result.clone());
        result
    }

    /// How many times a worksheet was calculated. For tests and diagnostics.
    pub fn runs(&self) -> usize {
        self.runs.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;
    use kadakas_core::uid::Uid;

    #[test]
    fn one_calculation_per_revision() {
        let cache = WorksheetCache::default();
        let mut def = WorksheetDef::default();
        def.set_cell("A1", "=1+1", None).unwrap();
        let doc = Document::default();
        let a = cache.get(5, Uid::from_num(1), &def, &doc);
        let b = cache.get(5, Uid::from_num(1), &def, &doc);
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(cache.runs(), 1);
        assert_eq!(a.value(1, 1), Value::Number(2.0));
        // Another sheet at the same revision is its own entry.
        cache.get(5, Uid::from_num(2), &def, &doc);
        assert_eq!(cache.runs(), 2);
        // A new revision calculates again, and the old results are dropped.
        let c = cache.get(6, Uid::from_num(1), &def, &doc);
        assert!(!Arc::ptr_eq(&a, &c));
        assert_eq!(cache.runs(), 3);
        cache.get(6, Uid::from_num(1), &def, &doc);
        assert_eq!(cache.runs(), 3);
        // Going back to an older revision (undo) calculates again: the key is the revision number.
        cache.get(5, Uid::from_num(1), &def, &doc);
        assert_eq!(cache.runs(), 4);
    }

    #[test]
    fn a_poisoned_lock_does_not_stop_the_cache() {
        let cache = Arc::new(WorksheetCache::default());
        let c2 = cache.clone();
        let _ = std::thread::spawn(move || {
            let _g = c2.inner.lock().unwrap();
            panic!("poison the lock");
        })
        .join();
        let doc = Document::default();
        let r = cache.get(1, Uid::from_num(1), &WorksheetDef::default(), &doc);
        assert_eq!(r.rows, 0);
    }
}
