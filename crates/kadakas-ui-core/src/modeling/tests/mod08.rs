//! Tests of the tools of the second part of MOD-08: Polygon, Offset, Follow Me, the Soften Edges
//! panel and the edge styles, and the hole through a slab with Push/Pull.

use super::*;
use kadakas_core::surface::{ATTR_SMOOTH, ATTR_SOFT, flagged};

use crate::modeling::soften::{EdgeStyle, SoftenPanel, edge_style_actions};

impl Rig {
    /// The pointer moves to `p` (seen from above).
    fn hover_free(&mut self, p: DVec3) {
        self.hover(p);
    }

    fn set_parts(&mut self, items: Vec<Item>) {
        let mut s = SubSelection::default();
        s.set(items);
        self.tools.set_parts(s);
    }

    fn live(&mut self) -> Option<(LiveLabel, String)> {
        self.tools.live(&kadakas_units::current())
    }
}

fn square_rig(tool: ModelerTool) -> Rig {
    let mut r = rig(ModelerTool::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 1000.0, 0.0));
    r.tools.choose(tool);
    r
}

fn curve_lens(m: &SurfaceModel) -> Vec<usize> {
    let mut l: Vec<usize> = m.curves().map(|(_, es)| es.len()).collect();
    l.sort();
    l
}

#[test]
fn polygon_is_one_curve_with_six_sides_and_takes_sides_and_a_radius() {
    let mut r = rig(ModelerTool::Polygon);
    assert_eq!(r.tools.polygon_sides(), DEFAULT_POLYGON_SIDES);
    let depth = r.ed.undo_depth();
    r.click(v(0.0, 0.0, 0.0));
    assert!(r.tools.in_progress());
    r.hover_free(v(500.0, 0.0, 0.0));
    assert_eq!(r.live().unwrap().0, LiveLabel::Radius);
    r.click(v(500.0, 0.0, 0.0));
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert_eq!(r.counts(), (6, 6, 1));
    assert_eq!(curve_lens(r.model()), vec![6]);
    // Inscribed: the corner is on the radius.
    assert!(
        r.model()
            .vertices()
            .any(|(_, p)| p.distance(v(500.0, 0.0, 0.0)) < 1e-6)
    );
    // `8s` just after redoes it with eight sides; a radius after that too.
    r.type_text("8s");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert_eq!(r.counts(), (8, 8, 1));
    r.type_text("300");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    let far = r
        .model()
        .vertices()
        .map(|(_, p)| p.length())
        .fold(0.0, f64::max);
    assert!((far - 300.0).abs() < 1e-6, "{far}");
}

#[test]
fn a_circumscribed_polygon_has_the_middle_of_a_side_on_the_radius() {
    let mut r = rig(ModelerTool::Polygon);
    r.tools.toggle_polygon_kind();
    assert!(r.tools.polygon_circumscribed());
    r.click(v(0.0, 0.0, 0.0));
    r.type_text("1000");
    assert_eq!(r.counts(), (6, 6, 1));
    let m = r.model();
    // The sides are 1000 from the center: the corners 1000 / cos(30°).
    let corner = 1000.0 / (std::f64::consts::PI / 6.0).cos();
    assert!(
        m.vertices()
            .all(|(_, p)| (p.length() - corner).abs() < 1e-6)
    );
    // Typed sides before the first click count for the next polygon.
    r.tools.choose(ModelerTool::Polygon);
    r.type_text("5");
    assert_eq!(r.tools.polygon_sides(), 5);
}

#[test]
fn offset_with_the_pointer_then_a_typed_distance_redoes_and_a_negative_goes_outward() {
    let mut r = square_rig(ModelerTool::Offset);
    let face = r.model().faces().next().unwrap().0;
    let depth = r.ed.undo_depth();
    r.click_inf(
        on_face(v(500.0, 500.0, 0.0), face),
        down_ray(v(500.0, 500.0, 0.0)),
    );
    assert!(r.tools.in_progress());
    // 100 inside the left side.
    r.hover_free(v(100.0, 500.0, 0.0));
    let (label, text) = r.live().unwrap();
    assert_eq!(label, LiveLabel::Distance);
    assert!(text.contains("100"), "{text}");
    r.click(v(100.0, 500.0, 0.0));
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert_eq!(r.model().face_count(), 2);
    assert!(
        r.model()
            .vertices()
            .any(|(_, p)| p.distance(v(100.0, 100.0, 0.0)) < 1e-6)
    );
    // A typed value redoes it.
    r.type_text("50");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert!(
        r.model()
            .vertices()
            .any(|(_, p)| p.distance(v(50.0, 50.0, 0.0)) < 1e-6)
    );
    // A negative value goes the other way: outward.
    r.type_text("-50");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert!(
        r.model()
            .vertices()
            .any(|(_, p)| p.distance(v(-50.0, -50.0, 0.0)) < 1e-6)
    );
    // Offset on nothing says so.
    r.tools.choose(ModelerTool::Offset);
    let out = r.click(v(9000.0, 9000.0, 0.0));
    assert!(out.message.is_some() && !r.tools.in_progress());
}

#[test]
fn offset_of_a_selected_face_starts_anywhere_and_a_too_large_value_is_refused() {
    let mut r = square_rig(ModelerTool::Offset);
    let model = r.model_id();
    let face = r.model().faces().next().unwrap().0;
    r.set_parts(vec![Item::Face(model, face)]);
    r.click(v(2000.0, 500.0, 0.0));
    assert!(r.tools.in_progress());
    // Outside the face: outward, 1000 from the right side.
    r.hover_free(v(2000.0, 500.0, 0.0));
    let rev = r.ed.revision();
    // Typed while running: inward is refused when it does not fit.
    for c in "-700".chars() {
        assert!(r.tools.type_char(c));
    }
    let err = r
        .tools
        .enter_for_test(&mut r.ed, &mut r.cache, rev)
        .unwrap_err();
    assert!(err.contains("does not fit"), "{err}");
    assert_eq!(r.model().face_count(), 1);
}

#[test]
fn offset_of_selected_edges_goes_to_the_side_of_the_pointer() {
    let mut r = rig(ModelerTool::Line);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 0.0, 0.0));
    r.click(v(1000.0, 1000.0, 0.0));
    r.tools.cancel();
    let model = r.model_id();
    let edges: Vec<Item> = r
        .model()
        .edges()
        .map(|(e, _)| Item::Edge(model, e))
        .collect();
    r.tools.choose(ModelerTool::Offset);
    r.set_parts(edges);
    r.click(v(500.0, 300.0, 0.0));
    r.type_text("100");
    assert_eq!(r.model().edge_count(), 4);
    assert!(
        r.model()
            .vertices()
            .any(|(_, p)| p.distance(v(900.0, 100.0, 0.0)) < 1e-6)
    );
}

/// A square profile standing in the plane x = 0 and a path of two lines from its middle.
fn follow_rig() -> (Rig, FaceId, Vec<EdgeId>) {
    let mut r = rig(ModelerTool::Line);
    let sq = [
        v(0.0, -100.0, 0.0),
        v(0.0, 100.0, 0.0),
        v(0.0, 100.0, 200.0),
        v(0.0, -100.0, 200.0),
    ];
    for p in sq {
        r.click(p);
    }
    r.click(sq[0]);
    r.tools.cancel();
    let profile = r.model().faces().next().unwrap().0;
    r.tools.choose(ModelerTool::Line);
    r.click(v(0.0, 0.0, 100.0));
    r.click(v(2000.0, 0.0, 100.0));
    r.click(v(2000.0, 1000.0, 100.0));
    r.tools.cancel();
    let path: Vec<EdgeId> = r
        .model()
        .edges()
        .filter(|(_, e)| {
            let (a, b) = (
                r.model().vertex(e.v[0]).unwrap(),
                r.model().vertex(e.v[1]).unwrap(),
            );
            a.x > 0.0 || b.x > 0.0
        })
        .map(|(e, _)| e)
        .collect();
    assert_eq!(path.len(), 2);
    (r, profile, path)
}

fn profile_click(r: &mut Rig, profile: FaceId) -> Out {
    let mut i = free(v(0.0, 50.0, 50.0));
    i.kind = Kind::OnFace;
    i.source = Tag::face(0, profile);
    r.click_inf(i, down_ray(v(0.0, 50.0, 50.0)))
}

#[test]
fn follow_me_with_the_path_selected_sweeps_the_clicked_face() {
    let (mut r, profile, path) = follow_rig();
    let model = r.model_id();
    r.tools.choose(ModelerTool::FollowMe);
    assert!(!takes_values(ModelerTool::FollowMe));
    r.set_parts(path.iter().map(|e| Item::Edge(model, *e)).collect());
    let depth = r.ed.undo_depth();
    profile_click(&mut r, profile);
    assert_eq!(r.ed.undo_depth(), depth + 1);
    // 4 sides x 2, the top and the bottom of the L merged: 8 faces.
    assert_eq!(r.model().face_count(), 8);
}

#[test]
fn follow_me_without_a_selection_takes_the_face_then_the_path() {
    let (mut r, profile, path) = follow_rig();
    r.tools.choose(ModelerTool::FollowMe);
    // A click on nothing asks for the profile.
    let out = r.click(v(9000.0, 9000.0, 0.0));
    assert!(out.message.is_some());
    let out = profile_click(&mut r, profile);
    assert!(out.message.is_some(), "it asks for the path");
    assert!(r.tools.in_progress());
    // Hovering the path previews it; a click on one of its edges takes the chain.
    let mut i = free(v(1000.0, 0.0, 100.0));
    i.kind = Kind::OnEdge;
    i.source = Tag::edge(0, path[0]);
    r.act(|t, w| {
        t.moved(
            w,
            &Pointer {
                inf: &i,
                ray: down_ray(i.point),
                selected: None,
            },
        );
        assert_eq!(t.preview(w).pull.len(), 2, "the whole chain");
        Out::default()
    });
    r.click_inf(i.clone(), down_ray(i.point));
    assert!(!r.tools.in_progress());
    assert_eq!(r.model().face_count(), 8);
    // Esc while it waits for the path ends it.
    r.tools.choose(ModelerTool::FollowMe);
    let top = r.model().faces().next().unwrap().0;
    let mut j = free(v(0.0, 0.0, 0.0));
    j.kind = Kind::OnFace;
    j.source = Tag::face(0, top);
    r.click_inf(j, down_ray(v(0.0, 0.0, 0.0)));
    assert!(r.tools.cancel());
    assert!(!r.tools.in_progress());
}

#[test]
fn the_soften_panel_is_live_and_one_undo_step_and_the_menu_sets_the_flags() {
    let mut r = rig(ModelerTool::Circle);
    r.tools.sides = 12;
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(300.0, 0.0, 0.0));
    let face = r.model().faces().next().unwrap().0;
    r.tools.choose(ModelerTool::PushPull);
    r.click_inf(on_face(v(0.0, 0.0, 0.0), face), down_ray(v(0.0, 0.0, 0.0)));
    r.type_text("800");
    let model = r.model_id();
    let all: Vec<Item> = r
        .model()
        .edges()
        .map(|(e, _)| Item::Edge(model, e))
        .collect();
    let mut sub = SubSelection::default();
    sub.set(all.clone());
    let soft = |r: &Rig| {
        r.model()
            .edges()
            .filter(|(e, _)| flagged(r.model().edge_attrs(*e), ATTR_SOFT))
            .count()
    };
    assert_eq!(soft(&r), 12, "Push/Pull of a circle");
    let depth = r.ed.undo_depth();
    let mut p = SoftenPanel::default();
    // The slider moves: 10, then 45 (the model as it was: no step), then 100 degrees; each
    // applies at once, and the drag leaves at most one undo step.
    for (angle, want, steps) in [(10.0, 0, 1), (45.0, 12, 0), (100.0, 36, 1), (120.0, 36, 1)] {
        p.angle = angle;
        let rev = r.ed.revision();
        let out = p.change(r.ed.doc(), &sub, rev);
        let before = r.ed.undo_depth();
        run_actions(&mut r.ed, &out.actions, out.redo).unwrap();
        p.ran(out.redo, before, r.ed.undo_depth(), r.ed.revision());
        assert_eq!(soft(&r), want, "{angle}");
        assert_eq!(
            r.ed.undo_depth(),
            depth + steps,
            "{angle}: one undo step for the drag"
        );
    }
    // One undo gives the model before the drag.
    assert!(r.ed.undo());
    assert_eq!(soft(&r), 12);
    assert!(r.ed.redo());
    // Without smooth normals the edges are soft but not smooth.
    p.smooth = false;
    let rev = r.ed.revision();
    let out = p.change(r.ed.doc(), &sub, rev);
    run_actions(&mut r.ed, &out.actions, out.redo).unwrap();
    assert_eq!(
        r.model()
            .edges()
            .filter(|(e, _)| flagged(r.model().edge_attrs(*e), ATTR_SMOOTH))
            .count(),
        0
    );
    // Another selection is a new step.
    let mut one = SubSelection::default();
    one.set([all[0]]);
    let rev = r.ed.revision();
    p.committed(rev);
    let out = p.change(r.ed.doc(), &one, rev);
    assert!(!out.redo);
    // The context menu: Unsoften, then Soften, each one step.
    let depth = r.ed.undo_depth();
    let acts = edge_style_actions(r.ed.doc(), &sub, EdgeStyle::Unsoften);
    run_actions(&mut r.ed, &acts, false).unwrap();
    assert_eq!(soft(&r), 0);
    let acts = edge_style_actions(r.ed.doc(), &sub, EdgeStyle::Soften);
    run_actions(&mut r.ed, &acts, false).unwrap();
    assert_eq!(soft(&r), 36);
    assert_eq!(r.ed.undo_depth(), depth + 2);
    // Nothing selected: no action.
    assert!(edge_style_actions(r.ed.doc(), &SubSelection::default(), EdgeStyle::Smooth).is_empty());
    // Entity Info shows the flags of an edge.
    use crate::modeler::{EntityInfo, entity_info_with_parts};
    let scene = kadakas_core::mesh::build(r.ed.doc());
    match entity_info_with_parts(r.ed.doc(), &scene, &[], &one) {
        EntityInfo::Edge { style, .. } => assert!(style.soft && style.smooth && !style.hidden),
        other => panic!("{other:?}"),
    }
}

#[test]
fn push_pull_of_a_square_on_a_box_to_the_bottom_cuts_a_hole_in_one_step() {
    let mut r = square_rig(ModelerTool::PushPull);
    let face = r.model().faces().next().unwrap().0;
    r.click_inf(
        on_face(v(500.0, 500.0, 0.0), face),
        down_ray(v(500.0, 500.0, 0.0)),
    );
    r.type_text("500");
    // A square on the top.
    r.tools.choose(ModelerTool::Rectangle);
    let top = r
        .model()
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.9)
        .unwrap()
        .0;
    let mut a = on_face(v(300.0, 300.0, 500.0), top);
    a.constraint = None;
    r.click_inf(a, down_ray(v(300.0, 300.0, 500.0)));
    let b = on_face(v(700.0, 700.0, 500.0), top);
    r.click_inf(b, down_ray(v(700.0, 700.0, 500.0)));
    let inner = r
        .model()
        .faces()
        .find(|(id, f)| *id != top && f.plane.normal.z > 0.9)
        .unwrap()
        .0;
    r.tools.choose(ModelerTool::PushPull);
    let depth = r.ed.undo_depth();
    r.click_inf(
        on_face(v(500.0, 500.0, 500.0), inner),
        down_ray(v(500.0, 500.0, 500.0)),
    );
    r.type_text("-500");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert!(r.model().check().is_closed());
    assert!(r.model().face(inner).is_none(), "a hole through");
}
