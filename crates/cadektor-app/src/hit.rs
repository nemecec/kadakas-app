//! Hit testing and box selection against document objects.

use cadektor_core::curve;
use cadektor_core::geom::{dist_to_segment, point_in_polygon};
use cadektor_core::model::{Document, Object, ObjectId, Shape, arc_sweep};
use glam::DVec2;

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

/// True if `p` is within `tol` of the shape (or inside it, when `filled`).
pub fn shape_hit(shape: &Shape, filled: bool, p: DVec2, tol: f64) -> bool {
    match shape {
        Shape::Line { a, b } => dist_to_segment(p, *a, *b) <= tol,
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
            edge_dist(&corners, true, p) <= tol || (filled && p.cmpge(lo).all() && p.cmple(hi).all())
        }
        Shape::Circle { center, radius } => {
            let d = p.distance(*center);
            (d - radius).abs() <= tol || (filled && d < *radius)
        }
        Shape::Arc { center, radius, start, end } => {
            let v = p - *center;
            (v.length() - radius).abs() <= tol && angle_in_arc(v.y.atan2(v.x), *start, *end)
        }
        Shape::Polyline { points, closed } => {
            let flat = curve::flatten(points, *closed, curve_tolerance(tol));
            edge_dist(&flat, *closed, p) <= tol || (*closed && filled && point_in_polygon(p, &flat))
        }
        Shape::LandscapeArea { boundary, .. } | Shape::Hardscape { boundary, .. } | Shape::SiteModifier { boundary, .. } => {
            let flat = curve::flatten(boundary, true, curve_tolerance(tol));
            edge_dist(&flat, true, p) <= tol || point_in_polygon(p, &flat)
        }
        // Only the outline picks a site model, so that objects on the terrain stay pickable.
        Shape::SiteModel { .. } => edge_dist(&cadektor_core::svg::site_outline(shape), true, p) <= tol,
        Shape::Locus3d { p: q } => p.distance(q.truncate()) <= tol,
        Shape::Polyline3d { points, closed } => {
            let flat: Vec<DVec2> = points.iter().map(|q| q.truncate()).collect();
            edge_dist(&flat, *closed, p) <= tol || (*closed && filled && point_in_polygon(p, &flat))
        }
        Shape::Plant { at, spread, .. } => p.distance(*at) <= spread / 2.0 + tol,
        // The canopy picks the tree. The root protection zone does not, so it never hides what is under it.
        Shape::ExistingTree { at, spread, .. } => p.distance(*at) <= spread / 2.0 + tol,
        Shape::PlantTag { at, anchor, height, .. } => {
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
        Shape::Group { .. } => false,
        // A symbol is picked through its expanded shapes, see `hit_test`.
        Shape::Symbol { .. } => false,
        Shape::Viewport(_) | Shape::TitleBlock(_) => {
            let (lo, hi) = shape.bounds();
            let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
            edge_dist(&corners, true, p) <= tol || (matches!(shape, Shape::Viewport(_)) && p.cmpge(lo).all() && p.cmple(hi).all())
        }
    }
}

/// Visible objects on unlocked layers can be picked.
fn pickable(doc: &Document, o: &Object) -> bool {
    doc.is_pickable(o) && doc.layer(o.layer).is_some_and(|l| !l.locked)
}

/// True if the object is hit at `p`. A symbol instance is hit through the shapes of its definition.
fn object_hit(doc: &Document, o: &Object, p: DVec2, tol: f64) -> bool {
    if matches!(o.shape, Shape::Symbol { .. }) {
        return cadektor_core::symbols::hit_instance(doc, o, p, tol);
    }
    let filled = doc.resolved_attrs(o).fill.is_some_and(|c| c.0[3] > 0);
    shape_hit(&o.shape, filled, p, tol)
}

/// Topmost pickable object at `p`.
#[cfg(test)]
pub fn hit_test(doc: &Document, p: DVec2, tol: f64) -> Option<ObjectId> {
    hit_test_in(doc, p, tol, None)
}

/// What a click at `p` selects when you are in the group `context` (none: in no group): the topmost
/// object under the point, or the group that holds it, as a direct member of the context.
pub fn hit_test_in(doc: &Document, p: DVec2, tol: f64, context: Option<ObjectId>) -> Option<ObjectId> {
    doc.objects
        .values()
        .rev()
        .filter(|o| pickable(doc, o) && (context.is_none() && o.parent.is_none() || doc.selection_target(o.id, context).is_some()))
        .find(|o| object_hit(doc, o, p, tol))
        .and_then(|o| doc.selection_target(o.id, context))
}

/// Objects selected by a box. `crossing` selects objects that touch the box,
/// otherwise objects must be fully inside. In a group, the members of the group are selected. A group
/// is selected when any of its members is selected this way (crossing) or all of them (inside).
#[cfg(test)]
pub fn box_select(doc: &Document, a: DVec2, b: DVec2, crossing: bool) -> Vec<ObjectId> {
    box_select_in(doc, a, b, crossing, None)
}

pub fn box_select_in(doc: &Document, a: DVec2, b: DVec2, crossing: bool, context: Option<ObjectId>) -> Vec<ObjectId> {
    let (lo, hi) = (a.min(b), a.max(b));
    let mut out: Vec<ObjectId> = Vec::new();
    for o in doc.objects.values().filter(|o| pickable(doc, o) && !matches!(o.shape, Shape::Group { .. })) {
        let Some(target) = doc.selection_target(o.id, context) else { continue };
        let (s0, s1) = doc.object_bounds(o);
        if !(s0.is_finite() && s1.is_finite()) {
            continue;
        }
        let hit = if crossing { s0.cmple(hi).all() && s1.cmpge(lo).all() } else { s0.cmpge(lo).all() && s1.cmple(hi).all() };
        if hit && !out.contains(&target) {
            out.push(target);
        }
    }
    if !crossing {
        // A group counts only if all of its members are inside.
        out.retain(|id| doc.leaves(&[*id]).iter().all(|l| doc.object(*l).is_some_and(|o| { let (s0, s1) = doc.object_bounds(o); s0.cmpge(lo).all() && s1.cmple(hi).all() })));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_api::Session;
    use cadektor_core::command::Command;
    use cadektor_core::model::Attributes;

    fn doc_with(shapes: Vec<Shape>) -> Document {
        let mut s = Session::default();
        for shape in shapes {
            s.editor
                .execute(&Command::AddObject { shape, layer: None, class: None, attrs: Attributes::default() })
                .unwrap();
        }
        s.editor.doc().clone()
    }

    #[test]
    fn picks_line_and_topmost() {
        let doc = doc_with(vec![
            Shape::Line { a: DVec2::ZERO, b: DVec2::new(100.0, 0.0) },
            Shape::Line { a: DVec2::new(50.0, -10.0), b: DVec2::new(50.0, 10.0) },
        ]);
        let ids: Vec<_> = doc.objects.values().map(|o| o.id).collect();
        assert_eq!(hit_test(&doc, DVec2::new(50.0, 1.0), 2.0), Some(ids[1]));
        assert_eq!(hit_test(&doc, DVec2::new(20.0, 1.0), 2.0), Some(ids[0]));
        assert_eq!(hit_test(&doc, DVec2::new(20.0, 30.0), 2.0), None);
    }

    #[test]
    fn unfilled_rect_needs_edge_but_area_does_not() {
        let doc = doc_with(vec![
            Shape::Rect { min: DVec2::ZERO, max: DVec2::splat(100.0) },
            Shape::Hardscape {
                boundary: cadektor_core::curve::corners(&[DVec2::new(200.0, 0.0), DVec2::new(300.0, 0.0), DVec2::new(300.0, 100.0)]),
                material: "x".into(), thickness: None, elevation: None,
            },
        ]);
        assert_eq!(hit_test(&doc, DVec2::splat(50.0), 2.0), None);
        assert!(hit_test(&doc, DVec2::new(0.0, 50.0), 2.0).is_some());
        assert!(hit_test(&doc, DVec2::new(280.0, 30.0), 2.0).is_some());
    }

    #[test]
    fn arc_angle_range() {
        let arc = Shape::Arc { center: DVec2::ZERO, radius: 10.0, start: 0.0, end: std::f64::consts::FRAC_PI_2 };
        assert!(shape_hit(&arc, false, DVec2::new(7.0, 7.1), 0.5));
        assert!(!shape_hit(&arc, false, DVec2::new(-7.0, -7.0), 0.5));
    }

    #[test]
    fn full_circle_arc_is_hit_all_around_and_zero_sweep_only_at_its_start() {
        let tau = std::f64::consts::TAU;
        let full = Shape::Arc { center: DVec2::ZERO, radius: 10.0, start: 0.5, end: 0.5 + tau };
        for angle in [0.0f64, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0] {
            let p = DVec2::from_angle(angle) * 10.0;
            assert!(shape_hit(&full, false, p, 0.5), "angle {angle}");
        }
        // Not near the circle: no hit.
        assert!(!shape_hit(&full, false, DVec2::new(5.0, 0.0), 0.5));
        // An end angle of 2π below the start is the same full circle.
        let full_neg = Shape::Arc { center: DVec2::ZERO, radius: 10.0, start: 0.5, end: 0.5 - tau };
        assert!(shape_hit(&full_neg, false, DVec2::new(-10.0, 0.0), 0.5));
        // Zero sweep (not valid in a document, but must not panic or hit everywhere).
        let zero = Shape::Arc { center: DVec2::ZERO, radius: 10.0, start: 0.5, end: 0.5 };
        assert!(!shape_hit(&zero, false, DVec2::new(-10.0, 0.0), 0.5));
        assert!(shape_hit(&zero, false, DVec2::from_angle(0.5) * 10.0, 0.5));
    }

    #[test]
    fn locked_layer_not_pickable() {
        let mut s = Session::default();
        s.editor
            .execute(&Command::AddObject {
                shape: Shape::Line { a: DVec2::ZERO, b: DVec2::X },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        s.editor
            .execute(&Command::ModifyLayer { id: cadektor_core::model::FIRST_LAYER, name: None, visible: None, locked: Some(true), elevation: None, scale: None })
            .unwrap();
        assert_eq!(hit_test(s.editor.doc(), DVec2::new(0.5, 0.0), 1.0), None);
    }

    #[test]
    fn box_modes() {
        let doc = doc_with(vec![
            Shape::Circle { center: DVec2::ZERO, radius: 10.0 },
            Shape::Circle { center: DVec2::new(100.0, 0.0), radius: 10.0 },
        ]);
        let b = (DVec2::new(-20.0, -20.0), DVec2::new(105.0, 20.0));
        assert_eq!(box_select(&doc, b.0, b.1, false).len(), 1);
        assert_eq!(box_select(&doc, b.0, b.1, true).len(), 2);
    }

    #[test]
    fn rounded_corners_are_hit_on_the_curve_not_the_corner() {
        use cadektor_core::model::{Vertex, VertexKind};
        let arc = |x: f64, y: f64| Vertex::new(DVec2::new(x, y), VertexKind::Arc { radius: 100.0 });
        let shape = Shape::Polyline { points: vec![arc(0.0, 0.0), arc(1000.0, 0.0), arc(1000.0, 1000.0), arc(0.0, 1000.0)], closed: true };
        // The sharp corner is cut off: a click on it misses, a click on the arc hits.
        assert!(!shape_hit(&shape, false, DVec2::new(0.0, 0.0), 5.0));
        let on_arc = DVec2::new(100.0, 100.0) - DVec2::new(1.0, 1.0).normalize() * 100.0;
        assert!(shape_hit(&shape, false, on_arc, 5.0));
        assert!(shape_hit(&shape, false, DVec2::new(500.0, 0.0), 5.0));
        // Inside is hit when filled.
        assert!(shape_hit(&shape, true, DVec2::new(500.0, 500.0), 5.0));
        assert!(!shape_hit(&shape, false, DVec2::new(500.0, 500.0), 5.0));
        let area = Shape::Hardscape { boundary: shape.vertices().unwrap().clone(), material: "m".into(), thickness: None, elevation: None };
        assert!(shape_hit(&area, false, DVec2::new(500.0, 500.0), 5.0));
        assert!(!shape_hit(&area, false, DVec2::new(-20.0, -20.0), 5.0));
    }

    #[test]
    fn existing_trees_pick_by_canopy_and_tags_by_label_or_leader() {
        use cadektor_core::plants::tree::{TreeAction, TreeCondition};
        let tree = Shape::ExistingTree {
            at: DVec2::ZERO,
            species: String::new(),
            dbh: 500.0,
            spread: 4000.0,
            height: 9000.0,
            condition: TreeCondition::Good,
            action: TreeAction::Retain,
            rpz_factor: None,
            rpz_cap: None,
        };
        assert!(shape_hit(&tree, false, DVec2::new(1900.0, 0.0), 5.0));
        // The root protection zone (6000 mm) does not pick the tree.
        assert!(!shape_hit(&tree, false, DVec2::new(3000.0, 0.0), 5.0));
        let tag = Shape::PlantTag { at: DVec2::new(1000.0, 1000.0), anchor: DVec2::ZERO, style: cadektor_core::resource::ResourceId(1), height: 400.0 };
        assert!(shape_hit(&tag, false, DVec2::new(1500.0, 1200.0), 5.0), "label");
        assert!(shape_hit(&tag, false, DVec2::new(500.0, 500.0), 5.0), "leader");
        assert!(!shape_hit(&tag, false, DVec2::new(-500.0, 500.0), 5.0));
    }
}
