//! Tests of Move, Rotate and Scale in the window (MOD-05): the real input path (keys, pointer
//! events and typed text) through headless frames, and the result in the document.

use super::*;
use glam::DVec3;
use kadakas_core::model::ObjectId;
use kadakas_core::surface::Edit;

/// A box of 1000 x 1000 x 500 around the origin (its top at 500), in the document.
pub(super) fn make_box(app: &mut ModelerApp) -> ObjectId {
    let id = app
        .session
        .editor
        .execute(&Command::AddObject {
            shape: ModelShape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .map(|o| ObjectId(o.created[0]))
        .unwrap();
    let p = [
        [-500.0, -500.0, 0.0],
        [500.0, -500.0, 0.0],
        [500.0, 500.0, 0.0],
        [-500.0, 500.0, 0.0],
    ];
    app.session
        .editor
        .execute(&Command::EditSurface {
            id,
            edit: Edit::AddEdges {
                edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
            },
        })
        .unwrap();
    let face = surface(app).faces().next().unwrap().0;
    app.session
        .editor
        .execute(&Command::EditSurface {
            id,
            edit: Edit::PushPull {
                face,
                distance: 500.0,
            },
        })
        .unwrap();
    id
}

pub(super) fn bounds(app: &ModelerApp, id: ObjectId) -> (DVec3, DVec3) {
    match &app.session.editor.doc().object(id).unwrap().shape {
        ModelShape::SurfaceModel(m) => m.bounds().unwrap(),
        _ => panic!("not a surface model"),
    }
}

pub(super) fn near(a: DVec3, b: DVec3, tol: f64) -> bool {
    a.distance(b) < tol
}

/// Types text; `x`, `/`, `-`, `<`, `>` and `,` too.
pub(super) fn type_more(ctx: &egui::Context, app: &mut ModelerApp, text: &str, t: &mut f64) {
    use egui::Key;
    for c in text.chars() {
        let key = match c {
            'x' => Key::X,
            '/' => Key::Slash,
            '-' => Key::Minus,
            ',' => Key::Comma,
            '<' | '>' => Key::Comma,
            '0'..='9' | ';' | '.' | 's' | 'r' => {
                type_keys(ctx, app, &c.to_string(), t);
                continue;
            }
            other => panic!("no key for {other:?}"),
        };
        *t += 0.05;
        let mut ev = tap(key, egui::Modifiers::NONE);
        ev.push(egui::Event::Text(c.to_string()));
        let _ = frame(ctx, app, ev, *t);
    }
}

/// Where a world point is on the screen.
pub(super) fn screen_of(app: &mut ModelerApp, p: DVec3) -> egui::Pos2 {
    let origin = app.session.scene().origin;
    let r = app.canvas_rect.unwrap();
    let cam = kadakas_ui_core::modeling::infer_camera(
        &app.nav.camera,
        origin,
        glam::Vec2::new(r.width(), r.height()),
    )
    .unwrap();
    let q = cam.project(p).unwrap();
    egui::pos2(r.min.x + q.x as f32, r.min.y + q.y as f32)
}

/// The copy key goes down for a frame: Ctrl, or Option on a Mac.
pub(super) fn toggle_copy(ctx: &egui::Context, app: &mut ModelerApp, t: &mut f64) {
    let m = if cfg!(target_os = "macos") {
        egui::Modifiers::ALT
    } else {
        egui::Modifiers::CTRL
    };
    *t += 0.1;
    let _ = frame_with(ctx, app, vec![], *t, m);
    *t += 0.1;
    let _ = frame(ctx, app, vec![], *t);
}

pub(super) fn ready() -> (egui::Context, ModelerApp, f64, ObjectId) {
    let (ctx, mut app, mut t) = top_view();
    let id = make_box(&mut app);
    // The tray changes its width with its content; keep the view still.
    app.run(Cmd::ToggleTray, size(&app));
    for _ in 0..3 {
        t += 0.1;
        let _ = frame(&ctx, &mut app, vec![], t);
    }
    (ctx, app, t, id)
}

#[test]
fn the_keys_m_q_and_s_choose_move_rotate_and_scale() {
    let ctx = egui::Context::default();
    let mut app = app();
    let mut t = 0.0;
    for (key, want) in [
        (egui::Key::M, ModelerTool::Move),
        (egui::Key::Q, ModelerTool::Rotate),
        (egui::Key::S, ModelerTool::Scale),
        (egui::Key::Space, ModelerTool::Select),
    ] {
        choose(&ctx, &mut app, key, &mut t);
        assert_eq!(app.tools.active(), want, "{key:?}");
    }
}

#[test]
fn a_box_moves_a_thousand_mm_with_a_typed_value() {
    let (ctx, mut app, mut t, id) = ready();
    let c = center(&app);
    app.session.selection = vec![id];
    let depth = app.session.editor.undo_depth();
    choose(&ctx, &mut app, egui::Key::M, &mut t);
    assert_eq!(app.tools.active(), ModelerTool::Move);
    click_at(&ctx, &mut app, c, &mut t);
    assert!(app.edit.modeling.in_progress());
    // The red axis is locked, whatever the pointer does.
    let _ = frame(&ctx, &mut app, vec![moved(c + egui::vec2(60.0, 4.0))], t);
    t += 0.1;
    let _ = frame(
        &ctx,
        &mut app,
        tap(egui::Key::ArrowRight, egui::Modifiers::NONE),
        t,
    );
    type_keys(&ctx, &mut app, "1000", &mut t);
    assert_eq!(app.edit.modeling.typed(), "1000");
    enter(&ctx, &mut app, &mut t);
    assert_eq!(app.session.editor.undo_depth(), depth + 1, "one undo step");
    let (lo, hi) = bounds(&app, id);
    assert!(near(lo, DVec3::new(500.0, -500.0, 0.0), 0.5), "{lo:?}");
    assert!(near(hi, DVec3::new(1500.0, 500.0, 500.0), 0.5), "{hi:?}");
    assert!(!app.edit.modeling.in_progress());
    app.run(Cmd::Undo, size(&app));
    assert!(near(
        bounds(&app, id).0,
        DVec3::new(-500.0, -500.0, 0.0),
        1e-9
    ));
}

#[test]
fn move_copies_three_times_with_the_copy_key_and_5x() {
    let (ctx, mut app, mut t, id) = ready();
    let c = center(&app);
    app.session.selection = vec![id];
    let depth = app.session.editor.undo_depth();
    choose(&ctx, &mut app, egui::Key::M, &mut t);
    toggle_copy(&ctx, &mut app, &mut t);
    assert!(app.edit.modeling.copying(), "the copy key toggled copy");
    click_at(&ctx, &mut app, c, &mut t);
    let _ = frame(&ctx, &mut app, vec![moved(c + egui::vec2(60.0, 0.0))], t);
    t += 0.1;
    let _ = frame(
        &ctx,
        &mut app,
        tap(egui::Key::ArrowRight, egui::Modifiers::NONE),
        t,
    );
    type_keys(&ctx, &mut app, "2000", &mut t);
    enter(&ctx, &mut app, &mut t);
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    let boxes = |app: &ModelerApp| {
        app.session
            .editor
            .doc()
            .objects
            .values()
            .filter(|o| matches!(o.shape, ModelShape::SurfaceModel(_)))
            .count()
    };
    assert_eq!(boxes(&app), 2, "the original and one copy");
    // Three copies at the same step.
    type_more(&ctx, &mut app, "3x", &mut t);
    enter(&ctx, &mut app, &mut t);
    assert_eq!(boxes(&app), 4, "the original and three copies");
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    let mut xs: Vec<f64> = app
        .session
        .editor
        .doc()
        .objects
        .keys()
        .map(|k| bounds(&app, *k).0.x)
        .collect();
    xs.sort_by(f64::total_cmp);
    for (got, want) in xs.iter().zip([-500.0, 1500.0, 3500.0, 5500.0]) {
        assert!((got - want).abs() < 0.5, "{xs:?}");
    }
    assert!(bounds(&app, id).0.x < 0.0, "the original stayed");
}

#[test]
fn rotate_turns_a_box_by_a_typed_angle() {
    let (ctx, mut app, mut t, id) = ready();
    let c = center(&app);
    app.session.selection = vec![id];
    let depth = app.session.editor.undo_depth();
    choose(&ctx, &mut app, egui::Key::Q, &mut t);
    assert_eq!(app.tools.active(), ModelerTool::Rotate);
    // The center: the middle of the top face. The protractor lies on the face.
    let _ = frame(&ctx, &mut app, vec![moved(c)], t);
    click_at(&ctx, &mut app, c, &mut t);
    click_at(&ctx, &mut app, c + egui::vec2(70.0, 0.0), &mut t);
    type_keys(&ctx, &mut app, "45", &mut t);
    enter(&ctx, &mut app, &mut t);
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    let (lo, hi) = bounds(&app, id);
    // A square of 1000 turned 45 degrees about its center: the corners are 707.1 away.
    let r = 500.0 * 2.0_f64.sqrt();
    assert!(near(hi, DVec3::new(r, r, 500.0), 0.5), "{hi:?}");
    assert!(near(lo, DVec3::new(-r, -r, 0.0), 0.5), "{lo:?}");
    // Entity Info shows the new rotation.
    let scene = app.session.scene();
    let info = kadakas_ui_core::modeler::entity_info_with_parts(
        app.session.editor.doc(),
        &scene,
        &[id],
        &kadakas_ui_core::modeling::SubSelection::default(),
    );
    match info {
        kadakas_ui_core::modeler::EntityInfo::One {
            placement: Some(p), ..
        } => {
            assert!((p.rotation_deg[2] - 45.0).abs() < 1e-6, "{p:?}");
            assert!((p.scale[0] - 1.0).abs() < 1e-9);
        }
        other => panic!("{other:?}"),
    }
    // A negative angle redoes the step the other way: -90 degrees turns the square back onto
    // itself.
    type_more(&ctx, &mut app, "-90", &mut t);
    enter(&ctx, &mut app, &mut t);
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    let (_, hi) = bounds(&app, id);
    assert!(near(hi, DVec3::new(500.0, 500.0, 500.0), 0.01), "{hi:?}");
}

#[test]
fn scale_doubles_a_box_with_a_corner_grip() {
    let (ctx, mut app, mut t, id) = ready();
    app.session.selection = vec![id];
    let depth = app.session.editor.undo_depth();
    choose(&ctx, &mut app, egui::Key::S, &mut t);
    assert_eq!(app.tools.active(), ModelerTool::Scale);
    let corner = screen_of(&mut app, DVec3::new(500.0, 500.0, 500.0));
    // The grip is picked, then the factor is typed.
    let _ = frame(&ctx, &mut app, vec![moved(corner)], t);
    click_at(&ctx, &mut app, corner, &mut t);
    assert!(app.edit.modeling.in_progress(), "the grip was picked");
    type_keys(&ctx, &mut app, "2", &mut t);
    enter(&ctx, &mut app, &mut t);
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    let (lo, hi) = bounds(&app, id);
    // The opposite corner (-500, -500, 0) stays; the size doubles.
    assert!(near(lo, DVec3::new(-500.0, -500.0, 0.0), 0.5), "{lo:?}");
    assert!(near(hi, DVec3::new(1500.0, 1500.0, 1000.0), 0.5), "{hi:?}");
    // The Entity Info shows the scale of the object.
    let scene = app.session.scene();
    let info = kadakas_ui_core::modeler::entity_info_with_parts(
        app.session.editor.doc(),
        &scene,
        &[id],
        &kadakas_ui_core::modeling::SubSelection::default(),
    );
    match info {
        kadakas_ui_core::modeler::EntityInfo::One {
            placement: Some(p), ..
        } => assert!(
            (p.scale[0] - 2.0).abs() < 1e-9 && (p.scale[2] - 2.0).abs() < 1e-9,
            "{p:?}"
        ),
        other => panic!("{other:?}"),
    }
    app.run(Cmd::Undo, size(&app));
    assert!(near(
        bounds(&app, id).1,
        DVec3::new(500.0, 500.0, 500.0),
        1e-9
    ));
}

#[test]
fn scale_stretches_one_axis_with_a_face_grip_and_the_pointer() {
    let (ctx, mut app, mut t, id) = ready();
    // An oblique view, so that the grips do not lie behind each other.
    app.nav.set_projection(Projection::Perspective);
    app.run(
        Cmd::Nav(NavAction::View(StandardView::RightIsometric)),
        size(&app),
    );
    for _ in 0..3 {
        t += 0.1;
        let _ = frame(&ctx, &mut app, vec![], t);
    }
    app.session.selection = vec![id];
    choose(&ctx, &mut app, egui::Key::S, &mut t);
    // The face grip in the middle of the +x side.
    let grip = screen_of(&mut app, DVec3::new(500.0, 0.0, 250.0));
    let _ = frame(&ctx, &mut app, vec![moved(grip)], t);
    click_at(&ctx, &mut app, grip, &mut t);
    assert!(app.edit.modeling.in_progress(), "the grip was picked");
    // One and a half times as far from the opposite side: x = 1000.
    let to = screen_of(&mut app, DVec3::new(1000.0, 0.0, 250.0));
    assert!(app.canvas_rect.unwrap().contains(to), "{to:?}");
    let _ = frame(&ctx, &mut app, vec![moved(to)], t);
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![], t);
    click_at(&ctx, &mut app, to, &mut t);
    let (lo, hi) = bounds(&app, id);
    assert!(near(lo, DVec3::new(-500.0, -500.0, 0.0), 1.0), "{lo:?}");
    assert!(near(hi, DVec3::new(1000.0, 500.0, 500.0), 1.0), "{hi:?}");
}

#[test]
fn a_click_with_nothing_selected_picks_what_is_under_the_pointer_for_move() {
    let (ctx, mut app, mut t, id) = ready();
    let c = center(&app);
    assert!(app.session.selection.is_empty() && app.edit.sub.is_empty());
    choose(&ctx, &mut app, egui::Key::M, &mut t);
    click_at(&ctx, &mut app, c, &mut t);
    assert!(
        app.edit.modeling.in_progress(),
        "the click picked the top face and started the move"
    );
    assert_eq!(app.edit.sub.len(), 1);
    let depth = app.session.editor.undo_depth();
    let _ = frame(&ctx, &mut app, vec![moved(c + egui::vec2(60.0, 0.0))], t);
    type_more(&ctx, &mut app, "<0,0,300>", &mut t);
    enter(&ctx, &mut app, &mut t);
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    // Only the top face moved: the box is 800 high now and still closed.
    let (_, hi) = bounds(&app, id);
    assert!((hi.z - 800.0).abs() < 0.5, "{hi:?}");
    assert!(surface(&app).check().is_closed());
}

#[test]
fn escape_cancels_the_move_in_progress() {
    let (ctx, mut app, mut t, id) = ready();
    let c = center(&app);
    app.session.selection = vec![id];
    let depth = app.session.editor.undo_depth();
    choose(&ctx, &mut app, egui::Key::M, &mut t);
    click_at(&ctx, &mut app, c, &mut t);
    assert!(app.edit.modeling.in_progress());
    t += 0.1;
    let _ = frame(
        &ctx,
        &mut app,
        tap(egui::Key::Escape, egui::Modifiers::NONE),
        t,
    );
    assert!(!app.edit.modeling.in_progress());
    assert_eq!(app.session.editor.undo_depth(), depth);
    assert!(near(
        bounds(&app, id).0,
        DVec3::new(-500.0, -500.0, 0.0),
        1e-9
    ));
}

mod review;
