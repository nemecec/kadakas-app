//! Circles and arcs with an exact description in the modeler (MOD-15): the Circle, Arc and
//! Polygon tools fill it, Entity Info reads it, and the number of segments is set in one step.

use kadakas_core::command::Command;
use kadakas_core::surface::{CurveId, Edit};

use super::*;
use crate::modeler::{EntityInfo, entity_info_with_parts};

fn circle_rig(sides: u32) -> Rig {
    let mut r = rig(ModelerTool::Circle);
    r.tools.sides = sides;
    r.click(v(100.0, 200.0, 0.0));
    r.click(v(400.0, 200.0, 0.0));
    r
}

fn only_curve_of_len(r: &Rig, n: usize) -> CurveId {
    r.model()
        .curves()
        .map(|(c, _)| c)
        .find(|c| r.model().curve_segments(*c) == Some(n))
        .unwrap()
}

fn only_curve(r: &Rig) -> CurveId {
    r.model().curves().next().unwrap().0
}

#[test]
fn the_circle_tool_gives_the_curve_its_exact_circle() {
    let r = circle_rig(24);
    let a = r.model().curve_geometry(only_curve(&r)).unwrap().clone();
    assert_eq!(a.kind, "circle");
    assert!((a.radius - 300.0).abs() < 1e-9);
    assert!((a.center[0] - 100.0).abs() < 1e-9 && (a.center[1] - 200.0).abs() < 1e-9);
    assert_eq!(r.model().curve_segments(only_curve(&r)), Some(24));
}

#[test]
fn a_typed_radius_redoes_the_circle_and_its_description() {
    let mut r = circle_rig(24);
    let depth = r.ed.undo_depth();
    r.type_text("500");
    assert_eq!(r.ed.undo_depth(), depth);
    let a = r.model().curve_geometry(only_curve(&r)).unwrap().clone();
    assert!((a.radius - 500.0).abs() < 1e-9);
}

#[test]
fn the_polygon_tool_describes_the_circle_of_its_corners() {
    let mut r = rig(ModelerTool::Polygon);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(500.0, 0.0, 0.0));
    let a = r.model().curve_geometry(only_curve(&r)).unwrap().clone();
    assert_eq!(a.kind, "polygon");
    assert!((a.radius - 500.0).abs() < 1e-9);
    // Circumscribed: the radius goes to the middle of a side, the corners are further out.
    let mut c = rig(ModelerTool::Polygon);
    c.tools.toggle_polygon_kind();
    c.click(v(0.0, 0.0, 0.0));
    c.click(v(500.0, 0.0, 0.0));
    let a = c.model().curve_geometry(only_curve(&c)).unwrap().clone();
    let corner = c
        .model()
        .vertices()
        .map(|(_, p)| p.length())
        .fold(0.0, f64::max);
    assert!((a.radius - corner).abs() < 1e-6);
    assert!(a.radius > 500.0);
}

#[test]
fn the_arc_tool_describes_an_arc_with_start_and_end() {
    let mut r = rig(ModelerTool::Arc);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 0.0, 0.0));
    r.click(v(500.0, 300.0, 0.0));
    let id = only_curve(&r);
    let a = r.model().curve_geometry(id).unwrap().clone();
    assert_eq!(a.kind, "arc");
    // The circle through (0,0), (1000,0) and (500,300): radius (500^2 + 300^2) / (2 * 300).
    assert!((a.radius - 566.6666666).abs() < 1e-3, "{}", a.radius);
    assert!(a.start.is_some() && a.end.is_some());
    let sweep = a.sweep().unwrap();
    assert!(sweep > 0.0 && sweep < std::f64::consts::PI);
    // A bulge to the other side is an arc on the other side of the chord.
    let mut b = rig(ModelerTool::Arc);
    b.click(v(0.0, 0.0, 0.0));
    b.click(v(1000.0, 0.0, 0.0));
    b.click(v(500.0, -300.0, 0.0));
    let g = b.model().curve_geometry(only_curve(&b)).unwrap().clone();
    assert!((g.radius - a.radius).abs() < 1e-6);
    assert!(
        g.center()[1] > 0.0 && a.center()[1] < 0.0 || g.center()[1] < 0.0 && a.center()[1] > 0.0
    );
}

#[test]
fn a_circle_drawn_on_a_scaled_and_moved_model_is_described_in_local_units_and_read_in_the_drawing()
{
    let mut r = rig(ModelerTool::Circle);
    r.tools.sides = 16;
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(100.0, 0.0, 0.0));
    let id = r.model_id();
    // Scale the whole object by 2 (the model gets a scaled transform), then draw another circle.
    r.ed.execute(&Command::TransformObjects3d {
        ids: vec![id],
        matrix: glam::DAffine3::from_scale(DVec3::splat(2.0)).to_cols_array(),
        copy: false,
        count: 1,
    })
    .unwrap();
    // Another circle, drawn by an action in the drawing (center 500, 0, radius 100, 8 sides)
    // into the scaled model: the model holds half of it, and the description follows.
    let plane = Plane3::ground(0.0);
    let center = v(500.0, 0.0, 0.0);
    run_actions(
        &mut r.ed,
        &[Action::AddCurve {
            target: Target::Model(id),
            points: circle_points(center, &plane, 100.0, 0.0, 8),
            closed: true,
            faceted: false,
            analytic: Some(kadakas_core::surface::Analytic::circle(
                center, plane.n, plane.u, 100.0,
            )),
        }],
        false,
    )
    .unwrap();
    let second = r
        .model()
        .curves()
        .map(|(c, _)| c)
        .find(|c| r.model().curve_segments(*c) == Some(8))
        .unwrap();
    let local = r.model().curve_geometry(second).unwrap().clone();
    assert!((local.radius - 50.0).abs() < 1e-6, "{}", local.radius);
    assert!((local.center[0] - 250.0).abs() < 1e-6);
    let first = only_curve_of_len(&r, 16);
    // Entity Info reads the radius in the drawing.
    let mut sel = SubSelection::default();
    let e = r.model().curve_edges(first).unwrap()[0];
    sel.set(vec![Item::Edge(id, e)]);
    let rev = r.ed.revision();
    let scene = kadakas_core::mesh::build(r.ed.doc());
    let _ = rev;
    match entity_info_with_parts(r.ed.doc(), &scene, &[], &sel) {
        EntityInfo::Edge { shape: Some(s), .. } => {
            assert!((s.radius - 200.0).abs() < 1e-6, "{}", s.radius);
            assert_eq!(s.segments, 16);
            assert_eq!(s.kind, "circle");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn entity_info_shows_the_circle_for_an_edge_and_for_the_whole_curve() {
    let r = circle_rig(12);
    let id = r.model_id();
    let curve = only_curve(&r);
    let edges = r.model().curve_edges(curve).unwrap().to_vec();
    let scene = kadakas_core::mesh::build(r.ed.doc());
    let mut sel = SubSelection::default();
    sel.set(vec![Item::Edge(id, edges[3])]);
    match entity_info_with_parts(r.ed.doc(), &scene, &[], &sel) {
        EntityInfo::Edge {
            shape: Some(s),
            curve: Some((c, n)),
            ..
        } => {
            assert_eq!((c, n), (curve.0, 12));
            assert!((s.radius - 300.0).abs() < 1e-9);
            assert_eq!(s.segments, 12);
            assert_eq!(s.center, [100.0, 200.0, 0.0]);
            assert!(s.sweep_deg.is_none());
            assert!((s.length - 300.0 * std::f64::consts::TAU).abs() < 1e-6);
        }
        other => panic!("{other:?}"),
    }
    // All edges of the curve: the curve itself.
    sel.set(edges.iter().map(|e| Item::Edge(id, *e)));
    match entity_info_with_parts(r.ed.doc(), &scene, &[], &sel) {
        EntityInfo::Curve { shape } => {
            assert_eq!(shape.segments, 12);
            assert_eq!(shape.model, id);
        }
        other => panic!("{other:?}"),
    }
    // Some of the edges, or edges of a face: parts, as before.
    sel.set(edges[..3].iter().map(|e| Item::Edge(id, *e)));
    assert!(matches!(
        entity_info_with_parts(r.ed.doc(), &scene, &[], &sel),
        EntityInfo::Parts { .. }
    ));
}

#[test]
fn an_edge_of_a_curve_without_the_description_has_no_shape() {
    let mut r = circle_rig(12);
    let id = r.model_id();
    let all: Vec<_> = r.model().vertices().map(|(v, _)| v).collect();
    // A non-uniform scale in the plane of the circle drops the description.
    r.ed.execute(&Command::EditSurface {
        id,
        edit: Edit::TransformVertices {
            vertices: all,
            faces: Vec::new(),
            edges: Vec::new(),
            matrix: glam::DAffine3::from_scale(DVec3::new(2.0, 1.0, 1.0)).to_cols_array(),
        },
    })
    .unwrap();
    let e = r.model().curve_edges(only_curve(&r)).unwrap()[0];
    let mut sel = SubSelection::default();
    sel.set(vec![Item::Edge(id, e)]);
    let scene = kadakas_core::mesh::build(r.ed.doc());
    assert!(matches!(
        entity_info_with_parts(r.ed.doc(), &scene, &[], &sel),
        EntityInfo::Edge { shape: None, .. }
    ));
}

#[test]
fn the_segments_of_a_circle_are_set_in_one_undo_step_and_a_refusal_is_in_words() {
    let mut r = circle_rig(12);
    let id = r.model_id();
    let curve = only_curve(&r).0;
    let depth = r.ed.undo_depth();
    let notes = curve::set_segments(&mut r.ed, id, curve, 48).unwrap();
    assert!(notes.is_empty());
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert_eq!(r.model().curve_segments(CurveId(curve)), Some(48));
    assert!(r.ed.undo());
    assert_eq!(r.model().curve_segments(CurveId(curve)), Some(12));
    // Out of range.
    let e = curve::set_segments(&mut r.ed, id, curve, 2).unwrap_err();
    assert_eq!(e, kadakas_l10n::tr!("uic-curve-refused-range"));
    assert_eq!(r.ed.undo_depth(), depth);
    // A pushed circle: a face on another plane uses it.
    let face = r.model().faces().next().unwrap().0;
    r.tools.choose(ModelerTool::PushPull);
    r.click_inf(
        on_face(v(100.0, 200.0, 0.0), face),
        down_ray(v(100.0, 200.0, 0.0)),
    );
    r.type_text("400");
    let e = curve::set_segments(&mut r.ed, id, curve, 24).unwrap_err();
    assert_eq!(e, kadakas_l10n::tr!("uic-curve-refused-plane"));
    assert!(!e.chars().any(|c| c.is_ascii_digit()), "{e}");
    // A curve that is not there.
    let e = curve::set_segments(&mut r.ed, id, 9999, 24).unwrap_err();
    assert_eq!(e, kadakas_l10n::tr!("uic-curve-refused-no-curve"));
}

#[test]
fn the_notes_of_the_description_are_told_in_words_without_ids() {
    for note in [
        "curve 5 lost its circle description: its points are not on one circle any more",
        "curve 5: its start and end angle are updated",
        "curve 5 is now an arc: its centre and radius stay",
        "curve 5 is now a circle: its centre and radius stay",
        "no curve was made (an edge is in a curve already), so the circle description is left out",
    ] {
        let t = messages::localize(note);
        assert_ne!(t, note);
        assert!(!t.chars().any(|c| c.is_ascii_digit()), "{t}");
    }
    assert!(messages::localize("curve 5 is now an arc: x").contains("arc"));
}

#[test]
fn an_arc_drawn_on_a_mirrored_model_keeps_its_angles_true_and_says_nothing() {
    let mut r = circle_rig(12);
    let id = r.model_id();
    r.ed.execute(&Command::TransformObjects3d {
        ids: vec![id],
        matrix: glam::DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0)).to_cols_array(),
        copy: false,
        count: 1,
    })
    .unwrap();
    let plane = Plane3::ground(0.0);
    let center = v(-900.0, 100.0, 0.0);
    let (a, b, h) = (v(-1200.0, 100.0, 0.0), v(-600.0, 100.0, 0.0), 150.0);
    let arc = arc_circle(a, b, h, &plane).unwrap();
    let ran = run_actions_full(
        &mut r.ed,
        &[Action::AddCurve {
            target: Target::Model(id),
            points: arc_points(a, b, h, &plane, 8),
            closed: false,
            faceted: false,
            analytic: Some(arc.analytic(&plane)),
        }],
        false,
    )
    .unwrap();
    let _ = center;
    assert!(ran.notes.is_empty(), "{:?}", ran.notes);
    let drawn = only_curve_of_len(&r, 8);
    let g = r.model().curve_geometry(drawn).unwrap();
    assert_eq!(g.kind, "arc");
    // The sweep is the same in the model as in the drawing.
    assert!(
        (g.sweep().unwrap() - arc.sweep.abs()).abs() < 1e-9,
        "{} {}",
        g.sweep().unwrap(),
        arc.sweep
    );
}

#[test]
fn the_radius_is_true_when_the_placement_keeps_the_plane_of_the_circle() {
    let mut r = circle_rig(12);
    let id = r.model_id();
    let horizontal = only_curve(&r);
    // A vertical circle (in the plane x z) in the same model.
    let plane = Plane3::through(v(2000.0, 0.0, 0.0), DVec3::Y).unwrap();
    run_actions(
        &mut r.ed,
        &[Action::AddCurve {
            target: Target::Model(id),
            points: circle_points(v(2000.0, 0.0, 500.0), &plane, 100.0, 0.0, 10),
            closed: true,
            faceted: false,
            analytic: Some(kadakas_core::surface::Analytic::circle(
                v(2000.0, 0.0, 500.0),
                plane.n,
                plane.u,
                100.0,
            )),
        }],
        false,
    )
    .unwrap();
    let vertical = only_curve_of_len(&r, 10);
    let shape = |r: &Rig, c| curve_shape(r.ed.doc(), id, c).unwrap();
    let place = |r: &mut Rig, a: glam::DAffine3| {
        r.ed.execute(&Command::TransformObjects3d {
            ids: vec![id],
            matrix: a.to_cols_array(),
            copy: false,
            count: 1,
        })
        .unwrap();
    };
    assert!(shape(&r, horizontal).uniform && shape(&r, vertical).uniform);
    // Equal scales and a turn keep both true.
    place(
        &mut r,
        glam::DAffine3::from_rotation_z(0.6) * glam::DAffine3::from_scale(DVec3::splat(2.0)),
    );
    assert!((shape(&r, horizontal).radius - 600.0).abs() < 1e-6);
    assert!((shape(&r, vertical).radius - 200.0).abs() < 1e-6);
    // A height scale (a plan remap) keeps the horizontal circle true and bends the vertical one.
    place(
        &mut r,
        glam::DAffine3::from_scale(DVec3::new(1.0, 1.0, 3.0)),
    );
    let h = shape(&r, horizontal);
    assert!(h.uniform && (h.radius - 600.0).abs() < 1e-6, "{h:?}");
    assert!(!shape(&r, vertical).uniform);
    // A scale in the plane of the horizontal circle bends it, and not the vertical one in y.
    place(
        &mut r,
        glam::DAffine3::from_scale(DVec3::new(2.0, 1.0, 1.0)),
    );
    assert!(!shape(&r, horizontal).uniform);
}

#[test]
fn the_note_that_the_chain_is_broken_has_its_own_text() {
    let broken = messages::localize("curve 3 lost its circle description: the chain is broken");
    let off = messages::localize(
        "curve 3 lost its circle description: its points are not on one circle any more",
    );
    assert_eq!(broken, kadakas_l10n::tr!("uic-curve-note-broken"));
    assert_ne!(broken, off);
    assert_eq!(off, kadakas_l10n::tr!("uic-curve-note-dropped"));
}

#[test]
fn a_circle_of_an_unknown_kind_is_known_as_such_and_its_note_has_its_own_text() {
    let r = circle_rig(12);
    let id = r.model_id();
    let curve = only_curve(&r);
    let mut v = serde_json::to_value(r.ed.doc()).unwrap();
    v["objects"][0]["shape"]["curve_geometry"][0][1]["kind"] = serde_json::json!("ellipse");
    let doc = kadakas_core::model::Document::from_slice(&serde_json::to_vec(&v).unwrap()).unwrap();
    let s = curve_shape(&doc, id, curve).unwrap();
    assert!(!s.known);
    assert_eq!(s.kind, "ellipse");
    let known = curve_shape(r.ed.doc(), id, curve).unwrap();
    assert!(known.known);
    // The note of a copy that could not take the description along.
    let t = messages::localize(
        "curve 4 lost its circle description: its kind is not known to this program, so it cannot follow the change",
    );
    assert_eq!(t, kadakas_l10n::tr!("uic-curve-note-unknown-kind"));
}
