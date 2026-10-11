//! A cache of annotation primitives and legend rows, by document revision. Painting, picking and
//! SVG output ask for the same primitives again and again. Tags and legends read the whole
//! document, so they are made once per revision.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use kadakas_core::annot::draw::{self, Ctx, LegendRow, Prim, TextSource};
use kadakas_core::annot::style::TagStyle;
use kadakas_core::annot::{DataTag, Legend};
use kadakas_core::model::{Document, Object, ObjectId, Shape};

#[derive(Default)]
struct Inner {
    revision: u64,
    prims: HashMap<ObjectId, Arc<Vec<Prim>>>,
    rows: HashMap<(String, String), Arc<Vec<LegendRow>>>,
}

/// Primitives and legend rows by document revision. Revision 0 means "unknown": nothing is kept.
#[derive(Default)]
pub struct AnnotCache {
    inner: Mutex<Inner>,
    /// How often the primitives of an object were made (a count of the work, for tests).
    prim_builds: AtomicU64,
    /// How often the rows of a legend were made.
    row_builds: AtomicU64,
}

struct Cached<'a> {
    cache: &'a AnnotCache,
    revision: u64,
}

impl TextSource for Cached<'_> {
    fn tag_lines(&self, doc: &Document, tag: &DataTag, style: &TagStyle) -> Vec<String> {
        crate::tag_lines(doc, tag, style)
    }

    fn legend_rows(&self, doc: &Document, legend: &Legend) -> Vec<LegendRow> {
        // Legends with the same criteria and grouping share their rows.
        let key = (legend.criteria.clone(), legend.group_by.clone());
        if let Some(rows) = self.cache.lock(self.revision).rows.get(&key) {
            return rows.as_ref().clone();
        }
        self.cache.row_builds.fetch_add(1, Ordering::Relaxed);
        let rows = Arc::new(crate::legend_rows(doc, legend));
        self.cache
            .lock(self.revision)
            .rows
            .insert(key, rows.clone());
        rows.as_ref().clone()
    }
}

impl AnnotCache {
    /// The inner state, emptied first if it belongs to another revision.
    fn lock(&self, revision: u64) -> MutexGuard<'_, Inner> {
        let mut g = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if g.revision != revision {
            *g = Inner {
                revision,
                ..Inner::default()
            };
        }
        g
    }

    /// How many times the primitives of an object were made, and how many times the rows of a
    /// legend. A call that finds its answer in the cache does not count.
    pub fn builds(&self) -> (u64, u64) {
        (
            self.prim_builds.load(Ordering::Relaxed),
            self.row_builds.load(Ordering::Relaxed),
        )
    }

    /// The primitives of an annotation object on its layer. `revision` is the revision of the
    /// document (0: do not keep anything).
    pub fn prims(&self, doc: &Document, obj: &Object, revision: u64) -> Arc<Vec<Prim>> {
        let Shape::Annotation(a) = &obj.shape else {
            return Arc::new(Vec::new());
        };
        if revision == 0 {
            return Arc::new(draw::prims(
                doc,
                a,
                &Ctx::layer(doc, obj.layer),
                &crate::Sheets,
            ));
        }
        if let Some(p) = self.lock(revision).prims.get(&obj.id) {
            return p.clone();
        }
        self.prim_builds.fetch_add(1, Ordering::Relaxed);
        let made = Arc::new(draw::prims(
            doc,
            a,
            &Ctx::layer(doc, obj.layer),
            &Cached {
                cache: self,
                revision,
            },
        ));
        self.lock(revision).prims.insert(obj.id, made.clone());
        made
    }
}
