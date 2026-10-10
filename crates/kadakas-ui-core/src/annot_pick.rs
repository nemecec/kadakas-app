//! Picking of annotation objects. The primitives of an annotation come from `kadakas_annot`. A
//! cache keeps them by document revision, so a frame or a click does not read the whole document
//! again for every tag and legend.

use std::sync::Arc;

use glam::DVec2;
use kadakas_core::annot::draw::{Prim, prims_bounds};
use kadakas_core::geom::dist_to_segment;
use kadakas_core::model::{Document, Object};

thread_local! {
    static CACHE: (std::cell::Cell<u64>, kadakas_annot::AnnotCache) = Default::default();
}

/// Tells the cache which document revision is current. Call it before you draw or pick.
pub fn set_revision(revision: u64) {
    CACHE.with(|c| c.0.set(revision));
}

/// The primitives of an annotation object (from the cache).
pub fn object_prims(doc: &Document, o: &Object) -> Arc<Vec<Prim>> {
    CACHE.with(|c| c.1.prims(doc, o, c.0.get()))
}

/// True if `p` is within `tol` of a line of the annotation or inside the box of its text.
pub fn hit(doc: &Document, o: &Object, p: DVec2, tol: f64) -> bool {
    object_prims(doc, o).iter().any(|prim| match prim {
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
pub fn bounds(doc: &Document, o: &Object) -> (DVec2, DVec2) {
    prims_bounds(&object_prims(doc, o)).unwrap_or_else(|| o.shape.bounds())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_api::Session;
    use kadakas_core::annot::{Annot, NorthArrow};
    use kadakas_core::command::Command;
    use kadakas_core::model::{Attributes, ObjectId, Shape};

    #[test]
    fn a_north_arrow_is_picked_near_its_lines() {
        let mut s = Session::default();
        let r = s
            .editor
            .execute(&Command::AddObject {
                shape: Shape::Annotation(Box::new(Annot::NorthArrow(NorthArrow {
                    at: DVec2::new(1000.0, 1000.0),
                    size_mm: 15.0,
                    rotation_deg: 0.0,
                    use_georef: false,
                    style: Default::default(),
                }))),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        set_revision(s.editor.revision());
        let doc = s.editor.doc();
        let o = doc.object(ObjectId(r.created[0])).unwrap();
        let (lo, hi) = bounds(doc, o);
        assert!(lo.x < hi.x && lo.y < hi.y);
        assert!(hit(doc, o, (lo + hi) / 2.0, 1.0) || hit(doc, o, lo, 1.0));
        assert!(!hit(doc, o, hi + DVec2::splat(1.0e6), 1.0));
    }
}
