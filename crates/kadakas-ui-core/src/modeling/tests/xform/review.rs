//! Tests of the review of MOD-05a: the sign of a typed angle, the box of Scale, copies as the
//! selection, the note about the height, and the cost of measuring the selection.

use super::*;
use glam::DAffine3;
use kadakas_l10n::tr;

fn run_full_out(r: &mut Rig, out: &Out) -> Ran {
    let ran = run_actions_full(&mut r.ed, &out.actions, out.redo).expect("the actions run");
    r.tools.committed(ran.used, r.ed.revision());
    ran
}

fn click_ray(r: &mut Rig, p: DVec3, ray: Ray) -> Out {
    r.click_inf(free(p), ray)
}

fn up_ray(p: DVec3) -> Ray {
    Ray {
        origin: p - DVec3::Z * 10_000.0,
        dir: DVec3::Z,
    }
}

fn type_in(r: &mut Rig, text: &str) -> Out {
    for c in text.chars() {
        assert!(r.tools.type_char(c));
    }
    let rev = r.ed.revision();
    r.act_no_run(|t, w| t.enter(w, None, rev))
}

#[test]
fn a_positive_angle_turns_counterclockwise_as_the_viewer_sees_it() {
    // Seen from above: counterclockwise in the plan.
    let mut r = rig(ModelerTool::Rotate);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    click_ray(&mut r, v(0.0, 0.0, 0.0), down_ray(v(0.0, 0.0, 0.0)));
    click_ray(&mut r, v(500.0, 0.0, 0.0), down_ray(v(500.0, 0.0, 0.0)));
    enter(&mut r, "90");
    let (lo, hi) = bounds(&r, id);
    assert!(
        near(lo, v(-1000.0, 0.0, 0.0)) && near(hi, v(0.0, 1000.0, 500.0)),
        "{lo:?} {hi:?}"
    );
    // Seen from below, the same angle turns the other way in the world.
    let mut r = rig(ModelerTool::Rotate);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    click_ray(&mut r, v(0.0, 0.0, 0.0), up_ray(v(0.0, 0.0, 0.0)));
    click_ray(&mut r, v(500.0, 0.0, 0.0), up_ray(v(500.0, 0.0, 0.0)));
    enter(&mut r, "90");
    let (lo, hi) = bounds(&r, id);
    assert!(
        near(lo, v(0.0, -1000.0, 0.0)) && near(hi, v(1000.0, 0.0, 500.0)),
        "{lo:?} {hi:?}"
    );
}

#[test]
fn a_typed_angle_goes_the_way_the_pointer_was_dragged_and_a_minus_reverses_it() {
    let turned = |drag_to: DVec3, text: &str| {
        let mut r = rig(ModelerTool::Rotate);
        let id = make_box(&mut r);
        select_object(&mut r, id);
        click_ray(&mut r, v(0.0, 0.0, 0.0), down_ray(v(0.0, 0.0, 0.0)));
        click_ray(&mut r, v(500.0, 0.0, 0.0), down_ray(v(500.0, 0.0, 0.0)));
        r.hover(drag_to);
        enter(&mut r, text);
        bounds(&r, id)
    };
    let s = 0.5_f64.sqrt() * 500.0;
    // Dragged clockwise (to -45 degrees): 30 is 30 degrees clockwise.
    let (lo, hi) = turned(v(s, -s, 0.0), "30");
    assert!(
        (hi.x - 1366.0254).abs() < 1e-3 && (lo.y + 500.0).abs() < 1e-3,
        "{lo:?} {hi:?}"
    );
    // A minus reverses it: 30 degrees counterclockwise.
    let (lo, hi) = turned(v(s, -s, 0.0), "-30");
    assert!(
        (lo.x + 500.0).abs() < 1e-3 && (hi.y - 1366.0254).abs() < 1e-3,
        "{lo:?} {hi:?}"
    );
    // Dragged counterclockwise: counterclockwise. With no drag: counterclockwise.
    let (lo, _) = turned(v(s, s, 0.0), "30");
    assert!((lo.x + 500.0).abs() < 1e-3, "{lo:?}");
    let (lo, _) = turned(v(500.0, 0.0, 0.0), "30");
    assert!((lo.x + 500.0).abs() < 1e-3, "{lo:?}");
}

#[test]
fn a_length_typed_during_the_angle_step_is_refused_and_the_protractor_stays() {
    let mut r = rig(ModelerTool::Rotate);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    click_ray(&mut r, v(0.0, 0.0, 0.0), down_ray(v(0.0, 0.0, 0.0)));
    click_ray(&mut r, v(500.0, 0.0, 0.0), down_ray(v(500.0, 0.0, 0.0)));
    r.hover(v(0.0, 500.0, 0.0));
    let depth = r.ed.undo_depth();
    let out = enter(&mut r, "5m");
    assert!(out.message.is_some() && out.actions.is_empty());
    assert!(r.tools.in_progress(), "the protractor is still there");
    assert_eq!(r.tools.phase(), Phase::Step(2));
    let live = r.tools.live(&kadakas_units::current()).unwrap();
    assert!(live.1.starts_with("90.0"), "{}", live.1);
    assert_eq!(r.ed.undo_depth(), depth);
}

#[test]
fn a_scale_of_a_turned_box_keeps_its_own_axes_at_right_angles() {
    let mut r = rig(ModelerTool::Scale);
    let id = make_box(&mut r);
    r.ed.execute(&Command::TransformObjects3d {
        ids: vec![id],
        matrix: DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_4).to_cols_array(),
        copy: false,
        count: 1,
    })
    .unwrap();
    select_object(&mut r, id);
    // The grip in the middle of the box's own +x side.
    let own = DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_4);
    let grip = own.transform_point3(v(1000.0, 500.0, 250.0));
    start_scale(&mut r, grip);
    enter(&mut r, "2");
    let t = model_of(r.ed.doc(), id).unwrap().transform;
    let (x, y, z) = (t.matrix3.x_axis, t.matrix3.y_axis, t.matrix3.z_axis);
    assert!(
        (x.length() - 2.0).abs() < 1e-9 && (y.length() - 1.0).abs() < 1e-9,
        "{x:?} {y:?}"
    );
    assert!(
        x.dot(y).abs() < 1e-9 && x.dot(z).abs() < 1e-9,
        "the axes stay square"
    );
    let p = crate::modeler::Placement::of_transform(&t);
    assert!(!p.skewed);
    assert!((p.rotation_deg[2] - 45.0).abs() < 1e-6 && (p.scale[0] - 2.0).abs() < 1e-9);
}

#[test]
fn a_world_scale_of_a_turned_object_is_shown_as_skewed() {
    use crate::modeler::Placement;
    let turn = DAffine3::from_rotation_z(0.5);
    let stretch = DAffine3::from_scale(v(2.0, 1.0, 1.0));
    let skew = Placement::of_transform(&(stretch * turn));
    assert!(skew.skewed);
    assert!(skew.rotation_deg.iter().all(|a| a.is_finite()));
    let own = Placement::of_transform(&(turn * stretch));
    assert!(!own.skewed);
    assert!((own.rotation_deg[2] - 0.5_f64.to_degrees()).abs() < 1e-9);
    assert!((own.scale[0] - 2.0).abs() < 1e-9);
}

#[test]
fn a_typed_length_with_shift_from_a_face_grip_is_measured_along_the_grip() {
    let mut r = rig(ModelerTool::Scale);
    let id = make_box(&mut r);
    r.tools.set_context(XContext {
        objects: vec![id],
        shift: true,
        ..XContext::default()
    });
    start_scale(&mut r, v(1000.0, 500.0, 250.0));
    enter(&mut r, "2000mm");
    // The width 1000 becomes 2000: the factor 2, uniform about the middle of the far side.
    assert!(
        near(bounds(&r, id).1, v(2000.0, 1500.0, 750.0)),
        "{:?}",
        bounds(&r, id)
    );
}

#[test]
fn a_scale_of_minus_one_keeps_the_faces_of_a_box_looking_out() {
    let mut r = rig(ModelerTool::Scale);
    let id = make_box(&mut r);
    let mid = r.model_id();
    let mut parts = SubSelection::default();
    let items: Vec<Item> = r.model().faces().map(|(f, _)| Item::Face(mid, f)).collect();
    parts.add(items);
    r.tools.set_context(XContext {
        parts,
        ..XContext::default()
    });
    start_scale(&mut r, v(1000.0, 1000.0, 500.0));
    enter(&mut r, "-1");
    let m = model_of(r.ed.doc(), id).unwrap();
    assert!(m.check().is_valid(), "{:?}", m.check().problems);
    assert!(m.check().is_closed());
    let volume: f64 = m
        .faces()
        .map(|(f, face)| {
            let p = m.vertex(face.outer[0]).unwrap();
            m.face_area(f).unwrap() * face.plane.normal.dot(p) / 3.0
        })
        .sum();
    assert!((volume - 500_000_000.0).abs() < 1.0, "{volume}");
}

#[test]
fn five_x_after_a_division_keeps_the_step_and_a_division_after_five_x_divides_it() {
    let mut r = rig(ModelerTool::Move);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    r.tools.toggle_ctrl();
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(100.0, 0.0, 0.0));
    enter(&mut r, "2000");
    enter(&mut r, "/4");
    assert_eq!(r.ed.doc().objects.len(), 1 + 4);
    // 5x after /4: five copies with the step of 500.
    enter(&mut r, "5x");
    let mut xs: Vec<f64> =
        r.ed.doc()
            .objects
            .keys()
            .map(|k| bounds(&r, *k).0.x)
            .collect();
    xs.sort_by(f64::total_cmp);
    assert_eq!(xs, vec![0.0, 500.0, 1000.0, 1500.0, 2000.0, 2500.0]);
    // /5 divides the step again.
    enter(&mut r, "/5");
    assert_eq!(r.ed.doc().objects.len(), 1 + 5);
}

#[test]
fn a_plan_object_and_a_surface_model_move_up_together_and_the_note_is_localised() {
    let mut r = rig(ModelerTool::Move);
    let id = make_box(&mut r);
    let plant = ObjectId(
        r.ed.execute(&Command::AddObject {
            shape: Shape::Plant {
                at: glam::DVec2::new(3000.0, 0.0),
                species: "x".into(),
                spread: 1000.0,
                height: 2000.0,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    r.tools.set_context(XContext {
        objects: vec![id, plant],
        ..XContext::default()
    });
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(100.0, 0.0, 0.0));
    let out = type_in(&mut r, "<0,0,300>");
    let ran = run_full_out(&mut r, &out);
    assert!(
        (bounds(&r, id).0.z - 300.0).abs() < 1e-6,
        "the box moved up"
    );
    let want = tr!("uic-modeling-height-kept", kind = "plant");
    assert!(ran.notes.contains(&want), "{:?}", ran.notes);
    let Shape::Plant { at, height, .. } = r.ed.doc().object(plant).unwrap().shape else {
        panic!()
    };
    assert_eq!((at.x, height), (3000.0, 2000.0));
}

#[test]
fn the_copies_of_a_move_become_the_selection() {
    let mut r = rig(ModelerTool::Move);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    r.tools.toggle_ctrl();
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(100.0, 0.0, 0.0));
    let out = type_in(&mut r, "2000");
    let ran = run_full_out(&mut r, &out);
    assert_eq!(ran.objects.len(), 1);
    assert_ne!(ran.objects[0], id);
    assert!((bounds(&r, ran.objects[0]).0.x - 2000.0).abs() < 1e-6);
    // Faces: the copy of a face is selected.
    let mut r = rig(ModelerTool::Move);
    make_box(&mut r);
    let t = top(&r);
    select_face(&mut r, t);
    r.tools.toggle_ctrl();
    r.click(v(500.0, 500.0, 500.0));
    r.hover(v(600.0, 500.0, 500.0));
    let out = type_in(&mut r, "<2000,0,0>");
    let ran = run_full_out(&mut r, &out);
    assert_eq!(ran.parts.len(), 1, "{:?}", ran.parts);
    let Item::Face(m, f) = ran.parts[0] else {
        panic!()
    };
    assert!(f > t);
    let model = model_of(r.ed.doc(), m).unwrap();
    let face = model.face(f).unwrap();
    assert!(
        face.outer
            .iter()
            .all(|v| model.vertex(*v).unwrap().x >= 1999.9)
    );
}

#[test]
fn the_selection_is_measured_once_for_a_revision_not_on_every_event() {
    let mut r = rig(ModelerTool::Scale);
    let id = make_box(&mut r);
    select_object(&mut r, id);
    let eye = v(4000.0, 4000.0, 3000.0);
    for i in 0..6 {
        let p = v(1000.0, 1000.0, 500.0 + f64::from(i));
        r.act(|t, w| {
            let inf = free(p);
            t.moved(
                w,
                &Pointer {
                    inf: &inf,
                    ray: ray_to(eye, p),
                    selected: None,
                },
            );
            let _ = t.preview(w);
            Out::default()
        });
    }
    assert_eq!(r.tools.subject_builds(), 1);
    // An edit is a new revision: measured again.
    r.ed.execute(&Command::MoveObjects {
        ids: vec![id],
        delta: glam::DVec2::new(5.0, 0.0),
    })
    .unwrap();
    r.act(|t, w| {
        let _ = t.preview(w);
        Out::default()
    });
    assert_eq!(r.tools.subject_builds(), 2);
}
