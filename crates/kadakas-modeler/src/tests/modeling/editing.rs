//! Tests of the edits of MOD-08 that the review asked for: refused values, undo with a running
//! tool, the view while the model grows, and the selection of connected geometry.

use super::*;
use kadakas_core::surface::Edit;

/// A box (1000 wide, 500 high) with a square (400 wide) drawn in the middle of its top, in the
/// document.
fn box_with_square(app: &mut ModelerApp) {
    let id = app
        .session
        .editor
        .execute(&Command::AddObject {
            shape: ModelShape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .map(|o| kadakas_core::model::ObjectId(o.created[0]))
        .unwrap();
    let ring = |x0: f64, w: f64, z: f64| -> Edit {
        let p = [
            [x0, x0, z],
            [x0 + w, x0, z],
            [x0 + w, x0 + w, z],
            [x0, x0 + w, z],
        ];
        Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        }
    };
    let run = |app: &mut ModelerApp, edit: Edit| {
        app.session
            .editor
            .execute(&Command::EditSurface { id, edit })
            .unwrap();
    };
    run(app, ring(-500.0, 1000.0, 0.0));
    let face = surface(app).faces().next().unwrap().0;
    run(
        app,
        Edit::PushPull {
            face,
            distance: 500.0,
        },
    );
    run(app, ring(-200.0, 400.0, 500.0));
}

fn minus(ctx: &egui::Context, app: &mut ModelerApp, t: &mut f64) {
    let mut ev = tap(egui::Key::Minus, egui::Modifiers::NONE);
    ev.push(egui::Event::Text("-".into()));
    *t += 0.1;
    let _ = frame(ctx, app, ev, *t);
}

#[test]
fn a_refused_typed_value_keeps_the_step_so_a_corrected_value_works() {
    let (ctx, mut app, mut t) = top_view();
    box_with_square(&mut app);
    let c = center(&app);
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![], t);
    choose(&ctx, &mut app, egui::Key::P, &mut t);
    click_at(&ctx, &mut app, c, &mut t);
    assert!(
        app.edit.modeling.in_progress(),
        "the square on top was picked"
    );
    // A pocket of 200.
    minus(&ctx, &mut app, &mut t);
    type_keys(&ctx, &mut app, "200", &mut t);
    enter(&ctx, &mut app, &mut t);
    let depth = app.session.editor.undo_depth();
    let faces = surface(&app).face_count();
    // Too deep: refused, and nothing is lost.
    minus(&ctx, &mut app, &mut t);
    type_keys(&ctx, &mut app, "9000", &mut t);
    enter(&ctx, &mut app, &mut t);
    assert!(
        app.status
            .starts_with(&tr!("modeler-edit-failed", reason = "")[..8]),
        "{}",
        app.status
    );
    assert_eq!(
        app.session.editor.undo_depth(),
        depth,
        "the pocket is still there"
    );
    assert_eq!(surface(&app).face_count(), faces);
    // A corrected value redoes the pocket and does not say that a start is needed.
    app.status.clear();
    minus(&ctx, &mut app, &mut t);
    type_keys(&ctx, &mut app, "100", &mut t);
    enter(&ctx, &mut app, &mut t);
    assert!(
        !app.status.contains(&tr!("uic-modeling-need-start")),
        "{}",
        app.status
    );
    assert_eq!(app.session.editor.undo_depth(), depth);
    let m = surface(&app);
    assert!(m.check().is_closed());
    let floor = m
        .faces()
        .filter(|(_, f)| f.plane.normal.z > 0.9)
        .map(|(_, f)| f.plane.offset)
        .fold(f64::MAX, f64::min);
    assert!(
        (floor - 400.0).abs() < 1e-6,
        "the pocket is 100 deep: {floor}"
    );
}

#[test]
fn undo_and_redo_end_a_tool_that_is_running() {
    let (ctx, mut app, mut t) = top_view();
    box_with_square(&mut app);
    let c = center(&app);
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![], t);
    choose(&ctx, &mut app, egui::Key::P, &mut t);
    click_at(&ctx, &mut app, c, &mut t);
    assert!(app.edit.modeling.in_progress());
    type_keys(&ctx, &mut app, "5", &mut t);
    app.run(Cmd::Undo, size(&app));
    assert!(!app.edit.modeling.in_progress(), "undo ends Push/Pull");
    assert_eq!(app.edit.modeling.typed(), "");
    app.run(Cmd::Redo, size(&app));
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![], t);
    click_at(&ctx, &mut app, c, &mut t);
    assert!(app.edit.modeling.in_progress());
    app.run(Cmd::Undo, size(&app));
    app.run(Cmd::Redo, size(&app));
    assert!(!app.edit.modeling.in_progress(), "redo ends it too");
}

#[test]
fn a_growing_model_does_not_move_the_view_or_its_history() {
    let (ctx, mut app, mut t) = top_view();
    let world_target = |app: &mut ModelerApp| {
        let o = app.session.scene().origin;
        app.nav.camera.target.as_dvec3() + o
    };
    let before = world_target(&mut app);
    t += 1.0;
    let _ = frame(&ctx, &mut app, vec![], t);
    let c = center(&app);
    choose(&ctx, &mut app, egui::Key::R, &mut t);
    click_at(&ctx, &mut app, c + egui::vec2(100.0, 50.0), &mut t);
    click_at(&ctx, &mut app, c + egui::vec2(220.0, 130.0), &mut t);
    t += 1.0;
    let _ = frame(&ctx, &mut app, vec![], t);
    assert_ne!(
        app.session.scene().origin,
        glam::DVec3::ZERO,
        "the origin moved"
    );
    let after = world_target(&mut app);
    assert!((after - before).length() < 1e-2, "{before:?} {after:?}");
    if app.nav.can_go_back() {
        app.run(Cmd::Nav(NavAction::Previous), size(&app));
        assert!((world_target(&mut app) - before).length() < 1e-2);
    }
}

#[test]
fn a_triple_click_selects_everything_connected_and_a_double_click_on_an_edge_its_faces() {
    let (ctx, mut app, mut t) = top_view();
    // The tray changes its width with its content; keep the view still.
    app.run(Cmd::ToggleTray, size(&app));
    for _ in 0..3 {
        t += 0.1;
        let _ = frame(&ctx, &mut app, vec![], t);
    }
    let c = center(&app);
    choose(&ctx, &mut app, egui::Key::R, &mut t);
    click_at(&ctx, &mut app, c - egui::vec2(60.0, 35.0), &mut t);
    click_at(&ctx, &mut app, c + egui::vec2(60.0, 35.0), &mut t);
    choose(&ctx, &mut app, egui::Key::Space, &mut t);
    let b = egui::PointerButton::Primary;
    let none = egui::Modifiers::NONE;
    let multi =
        |ctx: &egui::Context, app: &mut ModelerApp, p: egui::Pos2, n: usize, t: &mut f64| {
            // A pause, so that the clicks before are not counted with these.
            *t += 1.0;
            let _ = frame(ctx, app, vec![moved(p)], *t);
            for _ in 0..n {
                *t += 0.04;
                let _ = frame(ctx, app, vec![button(p, b, true, none)], *t);
                *t += 0.04;
                let _ = frame(ctx, app, vec![button(p, b, false, none)], *t);
            }
            *t += 0.5;
            let _ = frame(ctx, app, vec![], *t);
        };
    multi(&ctx, &mut app, c + egui::vec2(20.0, 10.0), 3, &mut t);
    assert_eq!(
        app.edit.sub.len(),
        5,
        "the face and the four edges: {:?}",
        app.edit.sub
    );
    multi(&ctx, &mut app, c + egui::vec2(0.0, 35.0), 2, &mut t);
    assert_eq!(
        app.edit.sub.len(),
        2,
        "the edge and its face: {:?}",
        app.edit.sub
    );
    assert!(app.session.selection.is_empty(), "not the whole object");
}
