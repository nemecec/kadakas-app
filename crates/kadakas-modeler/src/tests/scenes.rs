//! Tests of scenes and styles in the window (MOD-10): the panels, the commands, the camera
//! transition, the file round trip, and the one-to-one map to the saved views that the Kadakas
//! app and the API use.

use serde_json::json;

use super::*;
use crate::scenes_panel::Action;
use kadakas_render::Camera;
use kadakas_ui_core::scene_panel::{self, TRANSITION_SECONDS};

const SIZE: glam::Vec2 = glam::Vec2::new(800.0, 600.0);

fn views(app: &ModelerApp) -> Vec<String> {
    app.session
        .editor
        .doc()
        .drawing
        .saved_views
        .iter()
        .map(|v| v.name.clone())
        .collect()
}

fn turn(app: &mut ModelerApp, yaw: f32) {
    app.nav.camera.yaw += yaw;
}

#[test]
fn add_stores_the_camera_and_starts_a_rename() {
    let mut app = app();
    app.scene_action(Action::Add, 0.0, SIZE);
    assert_eq!(views(&app), ["Scene 1"]);
    assert_eq!(app.status, tr!("modeler-scenes-added", name = "Scene 1"));
    assert_eq!(app.scenes.active.as_deref(), Some("Scene 1"));
    assert_eq!(
        app.scenes.renaming,
        Some(("Scene 1".into(), "Scene 1".into()))
    );
    let origin = app.session.scene().origin;
    let v = &app.session.editor.doc().drawing.saved_views[0];
    let cam = scene_panel::camera_from_saved(v.camera.as_ref().unwrap(), origin);
    assert_eq!(cam, app.nav.camera);
    app.scene_action(Action::Add, 0.0, SIZE);
    assert_eq!(views(&app), ["Scene 1", "Scene 2"]);
}

#[test]
fn every_scene_command_is_one_undo_step() {
    let mut app = app();
    app.scene_action(Action::Add, 0.0, SIZE);
    app.scene_action(Action::Add, 0.0, SIZE);
    let base = app.session.editor.doc().drawing.saved_views.clone();
    let steps = [
        Action::Rename("Scene 1".into(), "Front".into()),
        Action::Move("Scene 2".into(), -1),
        Action::Style("Hidden Line".into()),
        Action::Update("Front".into()),
        Action::Delete("Scene 2".into()),
    ];
    turn(&mut app, 1.0);
    for a in steps {
        let before = app.session.editor.doc().clone();
        app.scene_action(a.clone(), 0.0, SIZE);
        let after = app.session.editor.doc().clone();
        assert_ne!(before.drawing, after.drawing, "{a:?} changed nothing");
        assert!(app.session.editor.undo(), "{a:?}");
        assert_eq!(app.session.editor.doc().drawing, before.drawing, "{a:?}");
        assert!(app.session.editor.redo());
        assert_eq!(app.session.editor.doc().drawing, after.drawing, "{a:?}");
    }
    assert_ne!(app.session.editor.doc().drawing.saved_views, base);
}

#[test]
fn refusals_say_why_and_change_nothing() {
    let mut app = app();
    app.scene_action(Action::Add, 0.0, SIZE);
    app.scene_action(Action::Add, 0.0, SIZE);
    let depth = app.session.editor.undo_depth();
    app.scene_action(
        Action::Rename("Scene 1".into(), "scene 2".into()),
        0.0,
        SIZE,
    );
    assert_eq!(app.status, tr!("modeler-scenes-name-taken"));
    app.scene_action(Action::Rename("Scene 1".into(), "  ".into()), 0.0, SIZE);
    assert_eq!(app.status, tr!("modeler-scenes-name-empty"));
    app.scene_action(Action::Delete("Nope".into()), 0.0, SIZE);
    assert_eq!(app.status, tr!("modeler-scenes-missing"));
    app.scene_action(Action::Move("Scene 1".into(), -1), 0.0, SIZE);
    assert_eq!(app.session.editor.undo_depth(), depth);
    assert_eq!(views(&app), ["Scene 1", "Scene 2"]);
}

#[test]
fn rename_delete_and_the_marks_of_the_panel_follow() {
    let mut app = app();
    app.scene_action(Action::Add, 0.0, SIZE);
    app.scene_action(
        Action::Rename("Scene 1".into(), " Garden ".into()),
        0.0,
        SIZE,
    );
    assert_eq!(views(&app), ["Garden"]);
    assert_eq!(app.scenes.active.as_deref(), Some("Garden"));
    assert_eq!(app.scenes.chosen.as_deref(), Some("Garden"));
    app.scene_action(Action::Delete("garden".into()), 0.0, SIZE);
    assert!(views(&app).is_empty());
    assert_eq!(app.scenes.chosen, None);
}

#[test]
fn showing_a_scene_restores_tags_style_and_moves_the_camera_in_an_animation() {
    let ctx = egui::Context::default();
    let mut app = app();
    app.gpu_ready = true;
    // The first frames fit the view to the model. The scene is made after that.
    for i in 0..3 {
        let _ = frame(&ctx, &mut app, vec![], f64::from(i) * 0.1);
    }
    let start = app.nav.camera;
    app.scene_action(Action::Style("Realistic".into()), 0.0, SIZE);
    app.scene_action(Action::Add, 0.0, SIZE);
    turn(&mut app, 1.5);
    app.nav.camera.distance *= 2.0;
    app.scene_action(Action::Style("Wireframe".into()), 0.0, SIZE);
    let away = app.nav.camera;
    let depth = app.session.editor.undo_depth();
    app.scene_action(Action::Activate("Scene 1".into()), 10.0, SIZE);
    assert_eq!(
        scene_panel::active_style_name(app.session.editor.doc()).as_deref(),
        Some("Realistic")
    );
    assert_eq!(app.session.editor.undo_depth(), depth + 1, "one undo step");
    assert!(app.scene_animating());
    let mut t = 10.0;
    let mut seen_between = false;
    while app.scene_animating() && t < 12.0 {
        t += 0.05;
        let _ = frame(&ctx, &mut app, vec![], t);
        let c = app.nav.camera;
        if c != away && c != start {
            seen_between = true;
        }
    }
    assert!(seen_between, "the camera passed through views between");
    assert!(!app.scene_animating());
    assert!((app.nav.camera.yaw - start.yaw).abs() < 1e-4);
    assert!((app.nav.camera.distance - start.distance).abs() < 1.0);
    assert!(t >= 10.0 + TRANSITION_SECONDS - 0.1);
    // The camera is where the scene put it, so the panel shows no change mark.
    let origin = app.session.scene().origin;
    assert!(!scene_panel::is_modified(
        app.session.editor.doc(),
        "Scene 1",
        &app.nav.camera,
        origin
    ));
}

#[test]
fn the_transition_can_be_switched_off_and_the_person_can_interrupt_it() {
    let ctx = egui::Context::default();
    let mut app = app();
    app.scene_action(Action::Add, 0.0, SIZE);
    let stored = app.nav.camera;
    turn(&mut app, 2.0);
    app.scene_action(Action::Animate(false), 0.0, SIZE);
    assert!(!app.prefs.animate_scenes);
    app.scene_action(Action::Activate("Scene 1".into()), 5.0, SIZE);
    assert!(!app.scene_animating());
    assert_eq!(app.nav.camera, stored, "the camera is there at once");
    // The menu command switches it on again.
    app.run(Cmd::ToggleSceneAnimation, SIZE);
    assert!(app.prefs.animate_scenes);
    turn(&mut app, 2.0);
    app.scene_action(Action::Activate("Scene 1".into()), 5.0, SIZE);
    assert!(app.scene_animating());
    let _ = frame(&ctx, &mut app, vec![], 5.1);
    // The person turns the view: the transition gives way.
    turn(&mut app, 0.3);
    let moved = app.nav.camera;
    let _ = frame(&ctx, &mut app, vec![], 5.2);
    assert!(!app.scene_animating());
    assert_eq!(app.nav.camera, moved);
    app.run(Cmd::ToggleSceneAnimation, SIZE);
    assert!(!app.prefs.animate_scenes);
}

#[test]
fn the_style_picker_changes_the_style_in_one_step_and_adds_a_built_in_style_once() {
    let mut app = app();
    let before = app.session.editor.doc().resources.len();
    app.scene_action(Action::Style("Hidden Line".into()), 0.0, SIZE);
    assert_eq!(app.session.editor.doc().resources.len(), before + 1);
    assert_eq!(
        app.status,
        tr!("modeler-style-changed", name = "Hidden Line")
    );
    app.scene_action(Action::Style("Shaded".into()), 0.0, SIZE);
    app.scene_action(Action::Style("hidden line".into()), 0.0, SIZE);
    assert_eq!(app.session.editor.doc().resources.len(), before + 2);
    assert!(app.session.editor.undo());
    assert!(app.session.editor.undo());
    assert_eq!(app.session.editor.doc().resources.len(), before + 1);
    // The style that is active already is not a step.
    let depth = app.session.editor.undo_depth();
    app.scene_action(Action::Style("Hidden Line".into()), 0.0, SIZE);
    assert_eq!(app.session.editor.undo_depth(), depth);
}

#[test]
fn a_saved_file_shows_the_same_scenes_after_it_is_opened() {
    let d = temp("scenes-roundtrip");
    let path = d.join("yard.kad");
    let mut a = app();
    add_square(&mut a);
    a.scene_action(Action::Style("Realistic".into()), 0.0, SIZE);
    a.scene_action(Action::Add, 0.0, SIZE);
    turn(&mut a, 0.7);
    a.scene_action(Action::Add, 0.0, SIZE);
    a.scene_action(Action::Move("Scene 2".into(), -1), 0.0, SIZE);
    a.save_to(path.clone());
    let saved = a.session.editor.doc().drawing.saved_views.clone();
    assert_eq!(saved.len(), 2);

    let mut b = app();
    b.open_path(path);
    let loaded = b.session.editor.doc().drawing.saved_views.clone();
    assert_eq!(loaded, saved);
    assert_eq!(views(&b), ["Scene 2", "Scene 1"]);
    assert_eq!(b.scenes, crate::scenes_panel::ScenesState::default());
    b.scene_action(Action::Activate("Scene 2".into()), 0.0, SIZE);
    b.scene_action(Action::Animate(false), 0.0, SIZE);
    b.scene_action(Action::Activate("Scene 2".into()), 0.0, SIZE);
    let origin = b.session.scene().origin;
    let want = scene_panel::camera_from_saved(saved[0].camera.as_ref().unwrap(), origin);
    assert_eq!(b.nav.camera, want);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_scene_of_the_modeler_is_a_saved_view_of_the_app_and_the_reverse() {
    let mut app = app();
    app.scene_action(Action::Style("Hidden Line".into()), 0.0, SIZE);
    app.scene_action(Action::Add, 0.0, SIZE);
    turn(&mut app, 0.9);
    app.scene_action(Action::Add, 0.0, SIZE);
    app.scene_action(Action::Rename("Scene 2".into(), "Side".into()), 0.0, SIZE);
    // The app and the agents read and apply the same list through saved_view.* and view.list.
    let list = app.session.handle("saved_view.list", json!({})).unwrap();
    let names: Vec<&str> = list["views"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Scene 1", "Side"]);
    assert_eq!(list["views"][0]["render_style"], "Hidden Line");
    assert!(list["views"][1]["camera"]["distance"].as_f64().unwrap() > 0.0);
    let vl = app.session.handle("view.list", json!({})).unwrap();
    assert_eq!(vl["saved_views"][1]["name"], "Side");
    app.session
        .handle("view.set", json!({"view": "Side"}))
        .unwrap();

    // The reverse: a view that an agent (or the app) saves is a scene of the modeler.
    app.session
        .handle(
            "saved_view.save",
            json!({"name": "From agent", "camera": {
                "target": [1000.0, 2000.0, 0.0], "yaw": 0.5, "pitch": 0.4,
                "distance": 8000.0, "fov": 40.0, "projection": "perspective"
            }, "render_style": "Wireframe"}),
        )
        .unwrap();
    let rows = scene_panel::rows(app.session.editor.doc(), None);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2].name, "From agent");
    assert!(rows[2].has_camera);
    assert_eq!(rows[2].style.as_deref(), Some("Wireframe"));
    app.scene_action(Action::Animate(false), 0.0, SIZE);
    app.scene_action(Action::Activate("From agent".into()), 0.0, SIZE);
    assert_eq!(app.nav.camera.distance, 8000.0);
    assert_eq!(
        scene_panel::active_style_name(app.session.editor.doc()).as_deref(),
        Some("Wireframe")
    );
    // A view that has only a standard view (the app) is shown through that view.
    app.session
        .handle("saved_view.save", json!({"name": "Plan"}))
        .unwrap();
    let v = app
        .session
        .editor
        .doc()
        .drawing
        .saved_views
        .iter()
        .position(|v| v.name == "Plan")
        .unwrap();
    assert!(
        app.session.editor.doc().drawing.saved_views[v]
            .camera
            .is_none()
    );
    app.scene_action(Action::Activate("Plan".into()), 0.0, SIZE);
    assert!(app.nav.camera.distance > 0.0);
    // The order is shared too: saved_view.move sets it for both programs.
    app.session
        .handle("saved_view.move", json!({"name": "Plan", "to": 0}))
        .unwrap();
    assert_eq!(views(&app)[0], "Plan");
    app.session
        .handle(
            "saved_view.rename",
            json!({"name": "Plan", "new_name": "Top"}),
        )
        .unwrap();
    assert_eq!(views(&app)[0], "Top");
}

#[test]
fn the_panels_draw_in_both_languages_with_scenes() {
    for lang in ["en-US", "et"] {
        let ctx = egui::Context::default();
        let mut app = app_with(Prefs {
            language: Some(lang.to_string()),
            ..Prefs::default()
        });
        app.scene_action(Action::Add, 0.0, SIZE);
        app.scenes.renaming = None;
        let mut t = 0.0;
        let mut all = String::new();
        for _ in 0..3 {
            t += 0.1;
            all = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
        }
        assert!(all.contains(&tr!("modeler-tray-scenes")), "{lang}");
        assert!(all.contains(&tr!("modeler-tray-styles")), "{lang}");
        assert!(all.contains("Scene 1"), "{lang}");
        assert!(all.contains(&tr!("modeler-scenes-add")), "{lang}");
        assert!(all.contains(&tr!("modeler-scenes-animate")), "{lang}");
        assert!(!all.contains(&tr!("modeler-scenes-none")), "{lang}");
    }
}

#[test]
fn a_camera_that_is_not_finite_never_becomes_a_scene() {
    let mut app = app();
    app.nav.camera = Camera {
        distance: f32::NAN,
        ..app.nav.camera
    };
    app.scene_action(Action::Add, 0.0, SIZE);
    assert!(views(&app).is_empty());
    assert_eq!(app.status, tr!("modeler-scenes-bad-camera"));
}

#[test]
fn a_scene_looks_the_same_in_the_app_and_in_the_modeler() {
    use kadakas_core::present::RenderMode;
    // Modeler to app: the scene stores the mode, and an API restore sets the view mode.
    let mut a = app();
    a.scene_action(Action::Style("Hidden Line".into()), 0.0, SIZE);
    a.scene_action(Action::Add, 0.0, SIZE);
    let v = &a.session.editor.doc().drawing.saved_views[0];
    assert_eq!(v.mode.as_deref(), Some("hidden_line"));
    a.scene_action(Action::Style("Shaded".into()), 0.0, SIZE);
    a.session.view.mode = RenderMode::Shaded;
    a.session
        .handle("saved_view.restore", json!({"name": "Scene 1"}))
        .unwrap();
    assert_eq!(a.session.view.mode, RenderMode::HiddenLine);
    // A scene without a mode (an older or foreign file) gets the mode from its style.
    a.session
        .editor
        .execute(&Command::SaveView {
            view: Box::new(kadakas_core::sheets::SavedView {
                name: "Styled".into(),
                render_style: Some("Wireframe".into()),
                ..Default::default()
            }),
        })
        .unwrap();
    a.session
        .handle("saved_view.restore", json!({"name": "Styled"}))
        .unwrap();
    assert_eq!(a.session.view.mode, RenderMode::Wireframe);

    // App to modeler: a view with a mode and no style shows with the style of that mode.
    let mut b = app();
    b.session
        .editor
        .execute(&Command::SaveView {
            view: Box::new(kadakas_core::sheets::SavedView {
                name: "Lines".into(),
                mode: Some("wireframe".into()),
                ..Default::default()
            }),
        })
        .unwrap();
    b.scene_action(Action::Animate(false), 0.0, SIZE);
    b.scene_action(Action::Activate("Lines".into()), 0.0, SIZE);
    assert_eq!(
        scene_panel::active_style_name(b.session.editor.doc()).as_deref(),
        Some("Wireframe")
    );
}

#[test]
fn a_scene_with_a_missing_style_is_marked_in_the_panel() {
    let ctx = egui::Context::default();
    let mut app = app();
    app.session
        .editor
        .execute(&Command::SaveView {
            view: Box::new(kadakas_core::sheets::SavedView {
                name: "Lost".into(),
                render_style: Some("Nowhere".into()),
                ..Default::default()
            }),
        })
        .unwrap();
    let mut all = String::new();
    for i in 0..3 {
        all = texts(&frame(&ctx, &mut app, vec![], f64::from(i) * 0.1)).join("\n");
    }
    assert!(all.contains(&tr!("modeler-scenes-style-missing", name = "Nowhere")));
}

#[test]
fn a_scene_keeps_its_place_in_the_drawing_when_the_origin_of_the_scene_moves() {
    let mut app = app();
    app.session
        .handle(
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [1000, 1000]}}}),
        )
        .unwrap();
    app.scene_action(Action::Add, 0.0, SIZE);
    let saved = app.session.editor.doc().drawing.saved_views[0]
        .camera
        .clone()
        .unwrap();
    let first = app.session.scene().origin;
    // An object far away moves the origin of the scene.
    app.session
        .handle(
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "rect", "min": [600000, 0], "max": [601000, 1000]}}}),
        )
        .unwrap();
    let moved = app.session.scene().origin;
    assert!((moved - first).length() > 1000.0, "{first} {moved}");
    app.scene_action(Action::Animate(false), 0.0, SIZE);
    app.scene_action(Action::Activate("Scene 1".into()), 0.0, SIZE);
    // The camera looks at the same point of the drawing as when the scene was saved.
    let t = app.nav.camera.target;
    for (k, want) in saved.target.iter().enumerate() {
        let got = f64::from([t.x, t.y, t.z][k]) + moved[k];
        assert!((got - want).abs() < 0.5, "axis {k}: {got} against {want}");
    }
    // And the panel shows no change mark.
    assert!(!scene_panel::is_modified(
        app.session.editor.doc(),
        "Scene 1",
        &app.nav.camera,
        moved
    ));
}
