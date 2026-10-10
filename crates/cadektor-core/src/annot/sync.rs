//! Keeps dimensions, callouts and data tags tied to their objects.
//!
//! The commands call [`resync`] after an object moved or changed. An anchor that points at an
//! object that is gone is removed, and its point stays where it was.

use glam::DVec2;

use crate::model::{Document, Shape};

use super::{Anchor, AnchorTarget, Annot, DimMode, anchor_point, ref_point};

/// The annotation after its anchors are applied, or `None` if nothing changes.
///
/// `paper` is the number of layer units in one paper mm (the layer scale). `rederive_offset` is
/// set when a data tag was moved by hand: its label position is the new truth, and the offset
/// is calculated from it.
pub fn resync(doc: &Document, a: &Annot, paper: f64, rederive_offset: bool) -> Option<Annot> {
    let mut out = a.clone();
    match &mut out {
        Annot::Dimension(d) => {
            if let Some(Some(Anchor { object, target: AnchorTarget::Center })) = d.anchors.first().copied() {
                match (d.mode, doc.object(object).map(|o| &o.shape)) {
                    (DimMode::Radial | DimMode::Diameter, Some(Shape::Circle { center, radius } | Shape::Arc { center, radius, .. })) => {
                        let dir = (d.points[1] - d.points[0]).normalize_or(DVec2::X);
                        d.points[0] = *center;
                        d.points[1] = *center + dir * *radius;
                    }
                    (DimMode::ArcLength, Some(Shape::Arc { center, radius, start, end })) => {
                        let sweep = crate::model::arc_sweep(*start, *end);
                        d.points = vec![*center, *center + DVec2::from_angle(*start) * *radius, *center + DVec2::from_angle(*start + sweep) * *radius];
                    }
                    _ => {}
                }
            } else {
                for (i, anchor) in d.anchors.iter().enumerate() {
                    if let Some(p) = anchor.as_ref().and_then(|x| anchor_point(doc, x)) {
                        d.points[i] = p;
                    }
                }
            }
            // An anchor to an object that is gone ends here. The point keeps its place.
            for anchor in &mut d.anchors {
                if anchor.is_some_and(|x| doc.object(x.object).is_none()) {
                    *anchor = None;
                }
            }
            while d.anchors.last().is_some_and(Option::is_none) {
                d.anchors.pop();
            }
        }
        Annot::Callout(c) => {
            for l in &mut c.leaders {
                match l.anchor {
                    Some(x) if doc.object(x.object).is_none() => l.anchor = None,
                    Some(x) => {
                        if let (Some(p), Some(tip)) = (anchor_point(doc, &x), l.points.first_mut()) {
                            *tip = p;
                        }
                    }
                    None => {}
                }
            }
        }
        Annot::DataTag(t) => {
            if let Some(p) = doc.object(t.object).and_then(|o| ref_point(&o.shape)) {
                if rederive_offset && paper > 0.0 {
                    t.offset = (t.at - p) / paper;
                }
                t.anchor = p;
                t.at = p + t.offset * paper;
            }
        }
        _ => return None,
    }
    (out != *a).then_some(out)
}
