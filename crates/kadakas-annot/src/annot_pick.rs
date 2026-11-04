//! Picking of annotation objects. The primitives of an annotation come from `kadakas_annot`.

use std::sync::Arc;

use crate::AnnotCache;
use glam::DVec2;
use kadakas_core::annot::draw::{Prim, prims_bounds};
use kadakas_core::geom::dist_to_segment;
use kadakas_core::model::{Document, Object};

/// Where the primitives of annotations come from. The caller owns the cache: a UI program keeps
/// one `AnnotCache` and passes it with the revision of the document, so a frame or a click does
/// not read the whole document again for every tag and legend. `Prims::default()` keeps nothing.
#[derive(Clone, Copy, Default)]
pub struct Prims<'a> {
    cache: Option<&'a AnnotCache>,
    revision: u64,
}

impl<'a> Prims<'a> {
    /// Primitives from `cache` for the document at `revision` (0 keeps nothing).
    pub fn cached(cache: &'a AnnotCache, revision: u64) -> Prims<'a> {
        Prims {
            cache: Some(cache),
            revision,
        }
    }

    /// The primitives of an annotation object.
    pub fn of(&self, doc: &Document, o: &Object) -> Arc<Vec<Prim>> {
        match self.cache {
            Some(c) => c.prims(doc, o, self.revision),
            None => AnnotCache::default().prims(doc, o, 0),
        }
    }

    /// True if `p` is within `tol` of a line of the annotation or inside the box of its text.
    pub fn hit(&self, doc: &Document, o: &Object, p: DVec2, tol: f64) -> bool {
        self.of(doc, o).iter().any(|prim| match prim {
            Prim::Poly { pts, closed, .. } => {
                let n = pts.len();
                let segs = if *closed { n } else { n.saturating_sub(1) };
                (0..segs).any(|i| dist_to_segment(p, pts[i], pts[(i + 1) % n]) <= tol)
            }
            text => {
                let (lo, hi) = text.bounds();
                p.cmpge(lo - tol).all() && p.cmple(hi + tol).all()
            }
        })
    }

    /// The box around what an annotation draws.
    pub fn bounds(&self, doc: &Document, o: &Object) -> (DVec2, DVec2) {
        prims_bounds(&self.of(doc, o)).unwrap_or_else(|| o.shape.bounds())
    }
}
