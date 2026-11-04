//! Hit testing and box selection against document objects. The plan, the 3D view and the agent
//! API (the marks of a markup, REV-11) use the same rules.

use crate::annot_pick::Prims;
use glam::DVec2;
use kadakas_core::curve;
use kadakas_core::geom::{dist_to_segment, point_in_polygon};
use kadakas_core::irrigation::IrrigationShape;
use kadakas_core::model::{Document, Object, ObjectId, Shape, arc_sweep};

fn edge_dist(points: &[DVec2], closed: bool, p: DVec2) -> f64 {
    let n = points.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    (0..segs)
        .map(|i| dist_to_segment(p, points[i], points[(i + 1) % n]))
        .fold(f64::INFINITY, f64::min)
}

/// Chord tolerance when testing curves: a tenth of the pick distance.
fn curve_tolerance(tol: f64) -> f64 {
    (tol / 10.0).max(1e-3)
}

/// True if the direction `a` is inside the counterclockwise sweep from `start` to `end`.
/// A sweep of 2π is a full circle. A sweep of zero is only the start direction.
fn angle_in_arc(a: f64, start: f64, end: f64) -> bool {
    (a - start).rem_euclid(std::f64::consts::TAU) <= arc_sweep(start, end)
}

/// True if `p` picks an irrigation object: a line within `tol`, other objects near their symbol.
pub fn irrigation_hit(i: &IrrigationShape, p: DVec2, tol: f64) -> bool {
    match i {
        IrrigationShape::Line { points, .. } => points
            .windows(2)
            .any(|w| dist_to_segment(p, w[0], w[1]) <= tol),
        other => p.distance(other.anchor()) <= tol * 2.0,
    }
}

/// True if `p` is within `tol` of the shape (or inside it, when `filled`).
pub fn shape_hit(shape: &Shape, filled: bool, p: DVec2, tol: f64) -> bool {
    match shape {
        Shape::Line { a, b } => dist_to_segment(p, *a, *b) <= tol,
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
            edge_dist(&corners, true, p) <= tol
                || (filled && p.cmpge(lo).all() && p.cmple(hi).all())
        }
        Shape::Circle { center, radius } => {
            let d = p.distance(*center);
            (d - radius).abs() <= tol || (filled && d < *radius)
        }
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let v = p - *center;
            (v.length() - radius).abs() <= tol && angle_in_arc(v.y.atan2(v.x), *start, *end)
        }
        Shape::Polyline { points, closed } => {
            let flat = curve::flatten(points, *closed, curve_tolerance(tol));
            edge_dist(&flat, *closed, p) <= tol || (*closed && filled && point_in_polygon(p, &flat))
        }
        Shape::LandscapeArea { boundary, .. }
        | Shape::Hardscape { boundary, .. }
        | Shape::SiteModifier { boundary, .. } => {
            let flat = curve::flatten(boundary, true, curve_tolerance(tol));
            edge_dist(&flat, true, p) <= tol || point_in_polygon(p, &flat)
        }
        Shape::Massing(m) => {
            let flat = curve::flatten(&m.footprint, true, curve_tolerance(tol));
            edge_dist(&flat, true, p) <= tol || point_in_polygon(p, &flat)
        }
        Shape::Extrude(e) => {
            let t = curve_tolerance(tol);
            let outline = curve::flatten(&e.profile, true, t);
            let holes: Vec<Vec<DVec2>> =
                e.holes.iter().map(|h| curve::flatten(h, true, t)).collect();
            edge_dist(&outline, true, p) <= tol
                || holes.iter().any(|h| edge_dist(h, true, p) <= tol)
                || (point_in_polygon(p, &outline) && !holes.iter().any(|h| point_in_polygon(p, h)))
        }
        Shape::Sweep(sw) => {
            let (lo, hi) = sw.bounds3();
            let corners = [
                lo.truncate(),
                DVec2::new(hi.x, lo.y),
                hi.truncate(),
                DVec2::new(lo.x, hi.y),
            ];
            edge_dist(&corners, true, p) <= tol || point_in_polygon(p, &corners)
        }
        // The faces seen from above and the edges that are shown, as the plan draws them
        // (MOD-13). Soft and hidden edges and hidden faces do not pick.
        Shape::SurfaceModel(m) => kadakas_core::surface::plan::hit(
            &[kadakas_core::surface::plan::Part {
                model: m,
                xf: m.transform,
                fill: None,
            }],
            p,
            tol,
        ),
        Shape::Mesh3d(m) => {
            let (lo, hi) = m.bounds3();
            let corners = [
                lo.truncate(),
                DVec2::new(hi.x, lo.y),
                hi.truncate(),
                DVec2::new(lo.x, hi.y),
            ];
            edge_dist(&corners, true, p) <= tol || point_in_polygon(p, &corners)
        }
        // A map picks at its frame only, so the objects on it stay pickable.
        Shape::Underlay(u) => {
            let corners = [
                u.min,
                DVec2::new(u.max.x, u.min.y),
                u.max,
                DVec2::new(u.min.x, u.max.y),
            ];
            edge_dist(&corners, true, p) <= tol
        }
        // Only the outline picks a site model, so that objects on the terrain stay pickable.
        Shape::SiteModel { .. } => {
            edge_dist(&kadakas_core::svg::site_outline(shape), true, p) <= tol
        }
        Shape::Locus3d { p: q } => p.distance(q.truncate()) <= tol,
        Shape::Polyline3d { points, closed } => {
            let flat: Vec<DVec2> = points.iter().map(|q| q.truncate()).collect();
            edge_dist(&flat, *closed, p) <= tol || (*closed && filled && point_in_polygon(p, &flat))
        }
        Shape::Plant { at, spread, .. } => p.distance(*at) <= spread / 2.0 + tol,
        // The canopy picks the tree. The root protection zone does not, so it never hides what is under it.
        Shape::ExistingTree { at, spread, .. } => p.distance(*at) <= spread / 2.0 + tol,
        Shape::PlantTag {
            at, anchor, height, ..
        } => {
            // The label is about eight characters wide.
            let label_end = *at + DVec2::new(height * 0.6 * 8.0, *height);
            let in_label = p.cmpge(*at - tol).all() && p.cmple(label_end + tol).all();
            in_label || dist_to_segment(p, *anchor, *at) <= tol
        }
        Shape::Text { .. } => {
            let (lo, hi) = shape.bounds();
            p.cmpge(lo - tol).all() && p.cmple(hi + tol).all()
        }
        // A viewport is picked inside its frame. A title block is picked at its border.
        // Guides are not drawn in the plan: nothing to hit.
        Shape::Group { .. } | Shape::Unknown { .. } | Shape::Guide(_) => false,
        Shape::Irrigation(i) => irrigation_hit(i, p, tol),
        // A symbol is picked through its expanded shapes, see `hit_test`.
        Shape::Symbol { .. } => false,
        // A component instance is picked at the box around its definition, see `object_hit`.
        Shape::Component { .. } => false,
        // Annotations need the document (see `object_hit`).
        Shape::Annotation(_) => false,
        Shape::Viewport(_) | Shape::TitleBlock(_) => {
            let (lo, hi) = shape.bounds();
            let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
            edge_dist(&corners, true, p) <= tol
                || (matches!(shape, Shape::Viewport(_)) && p.cmpge(lo).all() && p.cmple(hi).all())
        }
    }
}

/// Visible objects on unlocked layers can be picked. A grey layer or class, and a locked layer,
/// cannot. The plan and the 3D view use the same rule.
pub fn pickable(doc: &Document, o: &Object) -> bool {
    doc.is_pickable(o) && doc.layer(o.layer).is_some_and(|l| !l.locked)
}

/// True if the object is hit at `p`. A symbol instance is hit through the shapes of its definition.
pub fn object_hit(pk: Prims, doc: &Document, o: &Object, p: DVec2, tol: f64) -> bool {
    if matches!(o.shape, Shape::Symbol { .. }) {
        return kadakas_core::symbols::hit_instance(doc, o, p, tol);
    }
    if matches!(o.shape, Shape::Annotation(_)) {
        return pk.hit(doc, o, p, tol);
    }
    if matches!(o.shape, Shape::Component { .. }) {
        // The box around the instance first (its definition is read from the document), then the
        // faces and edges of its parts as the plan draws them (MOD-13). An instance that cannot
        // be drawn is picked at its mark.
        let (lo, hi) = doc.object_bounds(o);
        if !(lo.is_finite() && hi.is_finite())
            || p.x < lo.x - tol
            || p.x > hi.x + tol
            || p.y < lo.y - tol
            || p.y > hi.y + tol
        {
            return false;
        }
        return match kadakas_core::surface::plan::parts_of(doc, o) {
            Some(Ok(parts)) => kadakas_core::surface::plan::hit(&parts, p, tol),
            _ => true,
        };
    }
    let filled = doc.resolved_attrs(o).fill.is_some_and(|c| c.0[3] > 0);
    shape_hit(&o.shape, filled, p, tol)
}

/// The topmost pickable object at `p` itself, not the group that holds it. Annotation tools use it.
pub fn hit_object(pk: Prims, doc: &Document, p: DVec2, tol: f64) -> Option<ObjectId> {
    doc.objects
        .values()
        .rev()
        .find(|o| {
            pickable(doc, o)
                && !matches!(o.shape, Shape::Group { .. })
                && object_hit(pk, doc, o, p, tol)
        })
        .map(|o| o.id)
}

/// What a click at `p` selects when you are in the group `context` (none: in no group): the topmost
/// object under the point, or the group that holds it, as a direct member of the context.
pub fn hit_test_in(
    pk: Prims,
    doc: &Document,
    p: DVec2,
    tol: f64,
    context: Option<ObjectId>,
) -> Option<ObjectId> {
    doc.objects
        .values()
        .rev()
        .filter(|o| {
            pickable(doc, o)
                && (context.is_none() && o.parent.is_none()
                    || doc.selection_target(o.id, context).is_some())
        })
        .find(|o| object_hit(pk, doc, o, p, tol))
        .and_then(|o| doc.selection_target(o.id, context))
}

pub fn box_select_in(
    pk: Prims,
    doc: &Document,
    a: DVec2,
    b: DVec2,
    crossing: bool,
    context: Option<ObjectId>,
) -> Vec<ObjectId> {
    let (lo, hi) = (a.min(b), a.max(b));
    let mut out: Vec<ObjectId> = Vec::new();
    for o in doc
        .objects
        .values()
        .filter(|o| pickable(doc, o) && !matches!(o.shape, Shape::Group { .. }))
    {
        let Some(target) = doc.selection_target(o.id, context) else {
            continue;
        };
        let (s0, s1) = if matches!(o.shape, Shape::Annotation(_)) {
            pk.bounds(doc, o)
        } else {
            doc.object_bounds(o)
        };
        if !(s0.is_finite() && s1.is_finite()) {
            continue;
        }
        let hit = if crossing {
            s0.cmple(hi).all() && s1.cmpge(lo).all()
        } else {
            s0.cmpge(lo).all() && s1.cmple(hi).all()
        };
        if hit && !out.contains(&target) {
            out.push(target);
        }
    }
    if !crossing {
        // A group counts only if all of its members are inside.
        out.retain(|id| {
            doc.leaves(&[*id]).iter().all(|l| {
                doc.object(*l).is_some_and(|o| {
                    let (s0, s1) = doc.object_bounds(o);
                    s0.cmpge(lo).all() && s1.cmple(hi).all()
                })
            })
        });
    }
    out
}

fn segments_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let (r, s) = (b - a, d - c);
    let den = r.perp_dot(s);
    if den.abs() < 1e-12 {
        return false;
    }
    let t = (c - a).perp_dot(s) / den;
    let u = (c - a).perp_dot(r) / den;
    (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)
}

/// True if the box `lo..hi` is inside the polygon (`crossing` false), or touches it (`crossing` true).
pub fn box_in_polygon(lo: DVec2, hi: DVec2, poly: &[DVec2], crossing: bool) -> bool {
    let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
    if !crossing {
        return corners.iter().all(|c| point_in_polygon(*c, poly));
    }
    if corners.iter().any(|c| point_in_polygon(*c, poly))
        || poly.iter().any(|v| v.cmpge(lo).all() && v.cmple(hi).all())
    {
        return true;
    }
    let n = poly.len();
    (0..n).any(|i| {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        (0..4).any(|k| segments_cross(a, b, corners[k], corners[(k + 1) % 4]))
    })
}

/// Objects selected by a lasso, a free-hand polygon. Like the box: `crossing` selects objects that
/// touch the lasso, otherwise objects must be fully inside. In a group, the members of the group
/// are selected. A group counts when any member (crossing) or all members (inside) are selected.
pub fn lasso_select_in(
    pk: Prims,
    doc: &Document,
    polygon: &[DVec2],
    crossing: bool,
    context: Option<ObjectId>,
) -> Vec<ObjectId> {
    if polygon.len() < 3 {
        return Vec::new();
    }
    let bounds = |o: &Object| {
        if matches!(o.shape, Shape::Annotation(_)) {
            pk.bounds(doc, o)
        } else {
            doc.object_bounds(o)
        }
    };
    let mut out: Vec<ObjectId> = Vec::new();
    for o in doc
        .objects
        .values()
        .filter(|o| pickable(doc, o) && !matches!(o.shape, Shape::Group { .. }))
    {
        let Some(target) = doc.selection_target(o.id, context) else {
            continue;
        };
        let (s0, s1) = bounds(o);
        if !(s0.is_finite() && s1.is_finite()) {
            continue;
        }
        if box_in_polygon(s0, s1, polygon, crossing) && !out.contains(&target) {
            out.push(target);
        }
    }
    if !crossing {
        out.retain(|id| {
            doc.leaves(&[*id]).iter().all(|l| {
                doc.object(*l).is_some_and(|o| {
                    let (s0, s1) = doc.object_bounds(o);
                    box_in_polygon(s0, s1, polygon, false)
                })
            })
        });
    }
    out
}
