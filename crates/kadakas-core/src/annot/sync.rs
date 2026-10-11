//! Keeps dimensions, callouts and data tags tied to their objects.
//!
//! The commands call [`resync`] after an object moved or changed. An anchor that points at an
//! object that is gone is removed, and its point stays where it was.

use glam::DVec2;

use crate::model::{Document, Shape};

use super::{Anchor, AnchorTarget, Annot, DimMode, ref_point, shape_edges, shape_vertices};

/// The annotation after its anchors are applied, or `None` if nothing changes.
///
/// `paper` is the number of layer units in one paper mm (the layer scale). `rederive_offset` is
/// set when a data tag was moved by hand: its label position is the new truth, and the offset
/// is calculated from it.
pub fn resync(doc: &Document, a: &Annot, paper: f64, rederive_offset: bool) -> Option<Annot> {
    let mut out = a.clone();
    match &mut out {
        Annot::Dimension(d) => {
            if let Some(Some(Anchor {
                object,
                target: AnchorTarget::Center,
                ..
            })) = d.anchors.first().copied()
            {
                match (d.mode, doc.object(object).map(|o| &o.shape)) {
                    (
                        DimMode::Radial | DimMode::Diameter,
                        Some(Shape::Circle { center, radius } | Shape::Arc { center, radius, .. }),
                    ) => {
                        let dir = (d.points[1] - d.points[0]).normalize_or(DVec2::X);
                        d.points[0] = *center;
                        d.points[1] = *center + dir * *radius;
                    }
                    (
                        DimMode::ArcLength,
                        Some(Shape::Arc {
                            center,
                            radius,
                            start,
                            end,
                        }),
                    ) => {
                        let sweep = crate::model::arc_sweep(*start, *end);
                        d.points = vec![
                            *center,
                            *center + DVec2::from_angle(*start) * *radius,
                            *center + DVec2::from_angle(*start + sweep) * *radius,
                        ];
                    }
                    (DimMode::Area, Some(shape)) => {
                        if let (Some(area), Some(at)) = (shape.area(), ref_point(shape)) {
                            d.area = Some(area);
                            d.points = vec![at];
                        }
                    }
                    _ => {}
                }
            } else {
                for i in 0..d.anchors.len() {
                    let Some(anchor) = d.anchors[i] else { continue };
                    if doc.object(anchor.object).is_none() {
                        // The object is gone. The point keeps its place.
                        d.anchors[i] = None;
                        continue;
                    }
                    match rebind(doc, &anchor, d.points[i]) {
                        Some((new, p)) => {
                            d.anchors[i] = Some(new);
                            d.points[i] = p;
                        }
                        None => {
                            d.anchors[i] = None;
                            d.detached = true;
                        }
                    }
                }
            }
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
                let Some(x) = l.anchor else { continue };
                if doc.object(x.object).is_none() {
                    l.anchor = None;
                    continue;
                }
                let last = l.points.first().copied().unwrap_or(DVec2::ZERO);
                match rebind(doc, &x, last) {
                    Some((new, p)) => {
                        l.anchor = Some(new);
                        if let Some(tip) = l.points.first_mut() {
                            *tip = p;
                        }
                    }
                    None => {
                        l.anchor = None;
                        c.detached = true;
                    }
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

/// How close a vertex must be to its last place for an anchor to bind to it again, in mm.
const REBIND_TOLERANCE: f64 = 1e-3;

/// Applies an anchor to the current shape. `last` is where the anchored point was.
///
/// - The vertex count is unchanged (or was never recorded): the index stays, the vertex moved.
/// - The count changed (a vertex was inserted or deleted): the anchor binds to the vertex or
///   edge that is still at `last`.
/// - Nothing is there any more: `None`. The caller detaches and keeps `last`.
fn rebind(doc: &Document, a: &Anchor, last: DVec2) -> Option<(Anchor, DVec2)> {
    let shape = &doc.object(a.object)?.shape;
    let verts = shape_vertices(shape);
    let n = verts.len();
    let same = a.count.is_none_or(|c| c == n);
    let bound = |target, p| {
        Some((
            Anchor {
                object: a.object,
                target,
                count: Some(n),
            },
            p,
        ))
    };
    match a.target {
        AnchorTarget::Center => bound(AnchorTarget::Center, ref_point(shape)?),
        AnchorTarget::Vertex { index } if same => bound(a.target, *verts.get(index)?),
        AnchorTarget::Vertex { .. } => {
            let (i, p) = verts
                .iter()
                .copied()
                .enumerate()
                .find(|(_, p)| p.distance(last) <= REBIND_TOLERANCE)?;
            bound(AnchorTarget::Vertex { index: i }, p)
        }
        AnchorTarget::Edge { index, t } if same => {
            let (p, q) = *shape_edges(shape).get(index)?;
            bound(
                AnchorTarget::Edge { index, t },
                p + (q - p) * t.clamp(0.0, 1.0),
            )
        }
        AnchorTarget::Edge { .. } => {
            shape_edges(shape)
                .iter()
                .enumerate()
                .find_map(|(i, (p, q))| {
                    let d = q - p;
                    let len2 = d.length_squared();
                    let t = if len2 > 0.0 {
                        ((last - p).dot(d) / len2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let at = p + d * t;
                    (at.distance(last) <= REBIND_TOLERANCE).then_some((
                        Anchor {
                            object: a.object,
                            target: AnchorTarget::Edge { index: i, t },
                            count: Some(n),
                        },
                        at,
                    ))
                })
        }
    }
}
