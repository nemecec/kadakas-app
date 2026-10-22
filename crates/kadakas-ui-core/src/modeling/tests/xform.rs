//! Tests of Move, Rotate and Scale (MOD-05): the state machines with typed values, Esc, copies
//! and `5x`, and the undo count.

use glam::DVec3;
use kadakas_core::command::Command;
use kadakas_core::model::{Attributes, ObjectId, Shape};
use kadakas_core::surface::{Edit, FaceId};

use super::*;
use crate::infer3d::{Key, Ray};
use crate::measure_box::Phase;

fn ray_to(from: DVec3, to: DVec3) -> Ray {
    Ray {
        origin: from,
        dir: (to - from).normalize(),
    }
}

/// A box of 1000 x 1000 x 500 with a corner at the origin, in the document.
fn make_box(r: &mut Rig) -> ObjectId {
    let id = ObjectId(
        r.ed.execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let p = [
        [0.0, 0.0, 0.0],
        [1000.0, 0.0, 0.0],
        [1000.0, 1000.0, 0.0],
        [0.0, 1000.0, 0.0],
    ];
    r.ed.execute(&Command::EditSurface {
        id,
        edit: Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        },
    })
    .unwrap();
    let face = r.model().faces().next().unwrap().0;
    r.ed.execute(&Command::EditSurface {
        id,
        edit: Edit::PushPull {
            face,
            distance: 500.0,
        },
    })
    .unwrap();
    id
}

fn top(r: &Rig) -> FaceId {
    r.model()
        .faces()
        .filter(|(_, f)| f.plane.normal.z > 0.9)
        .max_by(|a, b| a.1.plane.offset.total_cmp(&b.1.plane.offset))
        .unwrap()
        .0
}

fn select_object(r: &mut Rig, id: ObjectId) {
    r.tools.set_context(XContext {
        objects: vec![id],
        ..XContext::default()
    });
}

fn select_face(r: &mut Rig, f: FaceId) {
    let m = r.model_id();
    let mut parts = SubSelection::default();
    parts.add([Item::Face(m, f)]);
    r.tools.set_context(XContext {
        parts,
        ..XContext::default()
    });
}

fn bounds(r: &Rig, id: ObjectId) -> (DVec3, DVec3) {
    model_of(r.ed.doc(), id).unwrap().bounds().unwrap()
}

fn near(a: DVec3, b: DVec3) -> bool {
    a.distance(b) < 1e-6
}

fn enter(r: &mut Rig, text: &str) -> Out {
    r.type_text(text)
}

#[test]
fn move_needs_a_selection() {
    let mut r = rig(ModelerTool::Move);
    make_box(&mut r);
    let out = r.click(v(0.0, 0.0, 0.0));
    assert!(out.message.is_some() && !r.tools.in_progress());
}

#[test]
fn move_a_face_with_two_clicks_is_one_undo_step() {
    let mut r = rig(ModelerTool::Move);
    let id = make_box(&mut r);
    let top = top(&r);
    select_face(&mut r, top);
    let depth = r.ed.undo_depth();
    r.click(v(500.0, 500.0, 500.0));
    assert!(r.tools.in_progress());
    assert_eq!(r.tools.reference(), Some(v(500.0, 500.0, 500.0)));
    assert_eq!(r.ed.undo_depth(), depth, "the first click changes nothing");
    r.hover(v(500.0, 500.0, 800.0));
    let pv = r.act(|t, w| {
        assert!(!t.preview(w).shape.is_empty(), "the moving outline is drawn");
        Out::default()
    });
    assert!(pv.actions.is_empty());
    r.click(v(500.0, 500.0, 800.0));
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert!(!r.tools.in_progress());
    assert!(near(bounds(&r, id).1, v(1000.0, 1000.0, 800.0)));
    assert!(model_of(r.ed.doc(), id).unwrap().check().is_closed());
    r.ed.undo();
    assert!(near(bounds(&r, id).1, v(1000.0, 1000.0, 500.0)));
}

#[test]
fn move_takes_a_typed_distance_along_the_pointer_and_three_values() {
    let mut r = rig(ModelerTool::Move);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    let depth = r.ed.undo_depth();
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(100.0, 0.0, 0.0));
    let out = enter(&mut r, "1000");
    assert!(out.message.is_none(), "{out:?}");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    let (lo, hi) = bounds(&r, id);
    assert!(near(lo, v(1000.0, 0.0, 0.0)) && near(hi, v(2000.0, 1000.0, 500.0)));
    // A new value after the move redoes it: still one step, from the original place.
    assert_eq!(r.tools.phase(), Phase::Done);
    enter(&mut r, "300;0;100");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert!(near(bounds(&r, id).0, v(300.0, 0.0, 100.0)));
    // A relative and an absolute point, from a base.
    r.click(v(0.0, 0.0, 0.0));
    enter(&mut r, "<0,50,0>");
    assert!(near(bounds(&r, id).0, v(300.0, 50.0, 100.0)));
    r.click(v(10.0, 10.0, 10.0));
    enter(&mut r, "[0,0,0]");
    assert!(near(bounds(&r, id).0, v(290.0, 40.0, 90.0)));
}

#[test]
fn escape_cancels_a_move_and_changes_nothing() {
    let mut r = rig(ModelerTool::Move);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    let depth = r.ed.undo_depth();
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(50.0, 0.0, 0.0));
    r.tools.type_char('5');
    assert!(r.tools.cancel(), "the first Esc clears the text");
    assert!(r.tools.in_progress());
    assert!(r.tools.cancel(), "the second ends the move");
    assert!(!r.tools.in_progress());
    assert_eq!(r.ed.undo_depth(), depth);
    assert!(near(bounds(&r, id).0, v(0.0, 0.0, 0.0)));
}

#[test]
fn a_zero_move_is_refused_and_the_tool_keeps_waiting() {
    let mut r = rig(ModelerTool::Move);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    r.click(v(0.0, 0.0, 0.0));
    let out = r.click(v(0.0, 0.0, 0.0));
    assert!(out.message.is_some() && out.actions.is_empty());
    assert!(r.tools.in_progress());
}

#[test]
fn ctrl_makes_a_copy_and_5x_and_a_division_redo_it() {
    let mut r = rig(ModelerTool::Move);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    r.tools.toggle_ctrl();
    assert!(r.tools.copying());
    let depth = r.ed.undo_depth();
    let objects = r.ed.doc().objects.len();
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(100.0, 0.0, 0.0));
    enter(&mut r, "2000");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert_eq!(r.ed.doc().objects.len(), objects + 1, "one copy");
    assert!(near(bounds(&r, id).0, v(0.0, 0.0, 0.0)), "the original stays");
    assert_eq!(r.tools.phase(), Phase::Copied);
    // Five copies at the same step: the step before is taken back.
    enter(&mut r, "5x");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert_eq!(r.ed.doc().objects.len(), objects + 5);
    let mut xs: Vec<f64> = r
        .ed
        .doc()
        .objects
        .keys()
        .map(|k| bounds(&r, *k).0.x)
        .collect();
    xs.sort_by(f64::total_cmp);
    assert_eq!(xs, vec![0.0, 2000.0, 4000.0, 6000.0, 8000.0, 10000.0]);
    // /4 divides the distance into four copies: 2000 / 4 apart.
    enter(&mut r, "/4");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert_eq!(r.ed.doc().objects.len(), objects + 4);
    let mut xs: Vec<f64> = r
        .ed
        .doc()
        .objects
        .keys()
        .map(|k| bounds(&r, *k).0.x)
        .collect();
    xs.sort_by(f64::total_cmp);
    assert_eq!(xs, vec![0.0, 500.0, 1000.0, 1500.0, 2000.0]);
    // One undo takes back all the copies.
    r.ed.undo();
    assert_eq!(r.ed.doc().objects.len(), objects);
}

#[test]
fn a_count_is_refused_without_a_copy() {
    let mut r = rig(ModelerTool::Move);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(100.0, 0.0, 0.0));
    enter(&mut r, "1000");
    assert_eq!(r.tools.phase(), Phase::Done);
    r.tools.type_char('5');
    r.tools.type_char('x');
    let rev = r.ed.revision();
    let out = r.act(|t, w| t.enter(w, None, rev));
    assert!(out.message.is_some() && out.actions.is_empty(), "{out:?}");
}

#[test]
fn a_copy_of_a_face_makes_new_geometry_in_the_same_model() {
    let mut r = rig(ModelerTool::Move);
    let id = make_box(&mut r);
    let t = top(&r);
    select_face(&mut r, t);
    r.tools.toggle_ctrl();
    let depth = r.ed.undo_depth();
    r.click(v(500.0, 500.0, 500.0));
    r.hover(v(500.0, 500.0, 500.0 + 1.0));
    enter(&mut r, "<2000,0,0>");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    let m = model_of(r.ed.doc(), id).unwrap();
    assert_eq!((m.vertex_count(), m.face_count()), (12, 7));
    enter(&mut r, "3x");
    let m = model_of(r.ed.doc(), id).unwrap();
    assert_eq!((m.vertex_count(), m.face_count()), (8 + 12, 6 + 3));
    assert_eq!(r.ed.undo_depth(), depth + 1);
}

#[test]
fn moving_a_vertical_edge_up_folds_the_top_and_the_bottom_into_triangles() {
    let mut r = rig(ModelerTool::Move);
    let id = make_box(&mut r);
    let m = r.model();
    let (eid, _) = m
        .edges()
        .find(|(_, e)| {
            e.v.iter().all(|v| {
                let p = m.vertex(*v).unwrap();
                (p.x - 1000.0).abs() < 1e-9 && (p.y - 1000.0).abs() < 1e-9
            })
        })
        .unwrap();
    let mid = r.model_id();
    let mut parts = SubSelection::default();
    parts.add([Item::Edge(mid, eid)]);
    r.tools.set_context(XContext {
        parts,
        ..XContext::default()
    });
    let depth = r.ed.undo_depth();
    r.click(v(1000.0, 1000.0, 250.0));
    r.hover(v(1000.0, 1000.0, 300.0));
    enter(&mut r, "<0,0,100>");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    let m = model_of(r.ed.doc(), id).unwrap();
    assert!(m.check().is_valid(), "{:?}", m.check().problems);
    assert_eq!(m.face_count(), 8, "the top and the bottom folded in two");
    r.ed.undo();
    assert_eq!(model_of(r.ed.doc(), id).unwrap().face_count(), 6);
}

#[test]
fn rotate_by_a_typed_angle_about_the_center() {
    let mut r = rig(ModelerTool::Rotate);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    let depth = r.ed.undo_depth();
    // The protractor lies on the ground (nothing under the pointer).
    r.hover(v(0.0, 0.0, 0.0));
    r.click(v(0.0, 0.0, 0.0));
    assert!(r.tools.in_progress());
    // The angle is typed after the start direction.
    r.click(v(500.0, 0.0, 0.0));
    assert_eq!(r.tools.phase(), Phase::Step(2));
    let out = enter(&mut r, "90");
    assert!(out.message.is_none(), "{out:?}");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    let (lo, hi) = bounds(&r, id);
    // Turned 90 degrees counterclockwise about the corner: x in -1000..0, y in 0..1000.
    assert!(near(lo, v(-1000.0, 0.0, 0.0)), "{lo:?}");
    assert!(near(hi, v(0.0, 1000.0, 500.0)), "{hi:?}");
    // A new angle redoes it.
    enter(&mut r, "-90");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    let (lo, hi) = bounds(&r, id);
    assert!(near(lo, v(0.0, -1000.0, 0.0)) && near(hi, v(1000.0, 0.0, 500.0)));
}

#[test]
fn rotate_with_the_pointer_snaps_to_fifteen_degrees() {
    let mut r = rig(ModelerTool::Rotate);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1000.0, 0.0, 0.0));
    // About 31 degrees: snaps to 30.
    let a = 31.0_f64.to_radians();
    r.hover(v(1000.0 * a.cos(), 1000.0 * a.sin(), 0.0));
    let live = r.tools.live(&kadakas_units::current()).unwrap();
    assert_eq!(live.0, LiveLabel::Angle);
    assert!(live.1.starts_with("30.0"), "{}", live.1);
    r.click(v(1000.0 * a.cos(), 1000.0 * a.sin(), 0.0));
    let (_, hi) = bounds(&r, id);
    // The far corner (1000, 1000) turned by 30 degrees.
    let c30 = 30.0_f64.to_radians();
    let want_y = 1000.0 * c30.sin() + 1000.0 * c30.cos();
    assert!((hi.y - want_y).abs() < 1e-6, "{hi:?}");
}

#[test]
fn rotate_plane_comes_from_the_arrow_keys() {
    let mut r = rig(ModelerTool::Rotate);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    assert!(r.tools.arrow_plane(Key::Right), "red: the plane faces x");
    r.hover(v(0.0, 0.0, 0.0));
    r.click(v(0.0, 0.0, 0.0));
    // The start direction lies in the plane x = 0: along y.
    r.click(v(0.0, 500.0, 0.0));
    enter(&mut r, "90");
    let (lo, hi) = bounds(&r, id);
    // A turn of 90 degrees about the x axis: y goes to z.
    assert!(near(lo, v(0.0, -500.0, 0.0)), "{lo:?}");
    assert!(near(hi, v(1000.0, 0.0, 1000.0)), "{hi:?}");
}

#[test]
fn rotate_copies_turn_each_copy_further() {
    let mut r = rig(ModelerTool::Rotate);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    r.tools.toggle_ctrl();
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(500.0, 0.0, 0.0));
    let objects = r.ed.doc().objects.len();
    enter(&mut r, "90");
    assert_eq!(r.ed.doc().objects.len(), objects + 1);
    enter(&mut r, "3x");
    assert_eq!(r.ed.doc().objects.len(), objects + 3);
    // /3 divides 90 degrees into three copies of 30, 60 and 90.
    enter(&mut r, "/3");
    assert_eq!(r.ed.doc().objects.len(), objects + 3);
}

#[test]
fn rotating_a_face_in_its_plane_twists_the_walls_into_triangles() {
    let mut r = rig(ModelerTool::Rotate);
    let id = make_box(&mut r);
    let t = top(&r);
    select_face(&mut r, t);
    r.click(v(500.0, 500.0, 500.0));
    r.click(v(1000.0, 500.0, 500.0));
    enter(&mut r, "10");
    let m = model_of(r.ed.doc(), id).unwrap();
    assert!(m.check().is_valid(), "{:?}", m.check().problems);
    assert!(m.faces().all(|(_, f)| f.outer.len() >= 3));
    // The top turned in its plane: it stays one flat face (all its vertices turned), and each of
    // the four walls, with its top edge turned and its bottom edge fixed, folds in two.
    assert_eq!(m.face_count(), 10);
    assert!(m.faces().any(|(id, f)| id == t && f.outer.len() == 4));
}

#[test]
fn scale_with_a_corner_grip_is_uniform() {
    let mut r = rig(ModelerTool::Scale);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    let corner = v(1000.0, 1000.0, 500.0);
    let eye = v(4000.0, 4000.0, 3000.0);
    // The pointer over the grip.
    r.act(|t, w| {
        let inf = free(corner);
        t.moved(
            w,
            &Pointer {
                inf: &inf,
                ray: ray_to(eye, corner),
                selected: None,
            },
        );
        let pv = t.preview(w);
        assert_eq!(pv.grips.len(), 26);
        assert!(pv.grip_hover.is_some());
        Out::default()
    });
    let depth = r.ed.undo_depth();
    r.act(|t, w| {
        let inf = free(corner);
        t.click(
            w,
            &Pointer {
                inf: &inf,
                ray: ray_to(eye, corner),
                selected: None,
            },
        )
    });
    assert!(r.tools.in_progress());
    // The pointer goes to twice the distance from the anchor (the origin).
    let target = corner * 2.0;
    r.act(|t, w| {
        let inf = free(target);
        t.moved(
            w,
            &Pointer {
                inf: &inf,
                ray: ray_to(eye, target),
                selected: None,
            },
        );
        Out::default()
    });
    let live = r.tools.live(&kadakas_units::current()).unwrap();
    assert_eq!(live.0, LiveLabel::Factor);
    assert_eq!(live.1, "2.00");
    r.act(|t, w| {
        let inf = free(target);
        t.click(
            w,
            &Pointer {
                inf: &inf,
                ray: ray_to(eye, target),
                selected: None,
            },
        )
    });
    assert_eq!(r.ed.undo_depth(), depth + 1);
    let (lo, hi) = bounds(&r, id);
    assert!(near(lo, DVec3::ZERO) && near(hi, v(2000.0, 2000.0, 1000.0)), "{lo:?} {hi:?}");
}

fn start_scale(r: &mut Rig, grip_at: DVec3) {
    let eye = v(4000.0, 4000.0, 3000.0);
    r.act(|t, w| {
        let inf = free(grip_at);
        t.click(
            w,
            &Pointer {
                inf: &inf,
                ray: ray_to(eye, grip_at),
                selected: None,
            },
        )
    });
    assert!(r.tools.in_progress(), "the grip was picked");
}

#[test]
fn scale_takes_a_factor_three_factors_and_a_length() {
    let mut r = rig(ModelerTool::Scale);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    let depth = r.ed.undo_depth();
    start_scale(&mut r, v(1000.0, 1000.0, 500.0));
    let out = enter(&mut r, "2");
    assert!(out.message.is_none(), "{out:?}");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert!(near(bounds(&r, id).1, v(2000.0, 2000.0, 1000.0)));
    // Three factors, one for each axis, redo the step.
    enter(&mut r, "1;1;3");
    assert_eq!(r.ed.undo_depth(), depth + 1);
    assert!(near(bounds(&r, id).1, v(1000.0, 1000.0, 1500.0)));
    // A length on the face grip at the top: the box gets that height.
    start_scale(&mut r, v(500.0, 500.0, 1500.0));
    enter(&mut r, "2000mm");
    assert!(near(bounds(&r, id).1, v(1000.0, 1000.0, 2000.0)), "{:?}", bounds(&r, id));
}

#[test]
fn scale_from_a_face_grip_is_along_one_axis_and_shift_makes_it_uniform() {
    let mut r = rig(ModelerTool::Scale);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    start_scale(&mut r, v(1000.0, 500.0, 250.0));
    enter(&mut r, "2");
    assert!(near(bounds(&r, id).1, v(2000.0, 1000.0, 500.0)));
    r.ed.undo();
    r.tools.set_context(XContext {
        objects: vec![id],
        shift: true,
        ..XContext::default()
    });
    start_scale(&mut r, v(1000.0, 500.0, 250.0));
    let out = enter(&mut r, "2");
    assert!(out.message.is_none());
    // Uniform about the middle of the opposite face.
    assert!(near(bounds(&r, id).1, v(2000.0, 1500.0, 750.0)), "{:?}", bounds(&r, id));
}

#[test]
fn scale_about_the_center_with_ctrl() {
    let mut r = rig(ModelerTool::Scale);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    r.tools.toggle_ctrl();
    start_scale(&mut r, v(1000.0, 1000.0, 500.0));
    enter(&mut r, "2");
    let (lo, hi) = bounds(&r, id);
    assert!(near(lo, v(-500.0, -500.0, -250.0)), "{lo:?}");
    assert!(near(hi, v(1500.0, 1500.0, 750.0)), "{hi:?}");
}

#[test]
fn scale_asks_for_a_grip_and_a_selection() {
    let mut r = rig(ModelerTool::Scale);
    let id = make_box(&mut r);
    let out = r.click(v(0.0, 0.0, 0.0));
    assert!(out.message.is_some());
    select_object(&mut r, id);
    // A click far from every grip.
    let out = r.click(v(9000.0, 9000.0, 0.0));
    assert!(out.message.is_some() && !r.tools.in_progress());
    // A typed value needs a grip first.
    r.tools.type_char('2');
    let rev = r.ed.revision();
    let out = r.act(|t, w| t.enter(w, None, rev));
    assert!(out.message.is_some() && out.actions.is_empty());
}

#[test]
fn scaling_a_face_scales_its_vertices_and_keeps_the_model_sound() {
    let mut r = rig(ModelerTool::Scale);
    let id = make_box(&mut r);
    let t = top(&r);
    select_face(&mut r, t);
    // The face is flat: the grips on the axes with no size are not offered.
    r.act(|tl, w| {
        let inf = free(v(0.0, 0.0, 0.0));
        tl.moved(
            w,
            &Pointer {
                inf: &inf,
                ray: down_ray(v(0.0, 0.0, 0.0)),
                selected: None,
            },
        );
        let pv = tl.preview(w);
        assert_eq!(pv.grips.len(), 8, "four corners and four edges of the flat box");
        Out::default()
    });
    start_scale(&mut r, v(1000.0, 1000.0, 500.0));
    enter(&mut r, "0.5");
    let m = model_of(r.ed.doc(), id).unwrap();
    assert!(m.check().is_valid(), "{:?}", m.check().problems);
    assert!(near(m.bounds().unwrap().1, v(1000.0, 1000.0, 500.0)));
}

#[test]
fn choosing_another_tool_drops_the_step_and_the_redo() {
    let mut r = rig(ModelerTool::Move);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    r.click(v(0.0, 0.0, 0.0));
    assert!(r.tools.in_progress());
    r.tools.choose(ModelerTool::Rotate);
    assert!(!r.tools.in_progress());
    // The inference skips the model that moves.
    r.tools.choose(ModelerTool::Move);
    r.click(v(0.0, 0.0, 0.0));
    assert!(r.tools.excluded().models.contains(&id));
}
