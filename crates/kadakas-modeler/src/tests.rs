//! Tests of the window without a GPU and without a screen: egui runs the frame headless, the keys
//! are events, and the questions to the person are answered by a test.

use egui::epaint::Shape;
use glam::{DVec2, Vec3};
use kadakas_core::command::Command;
use kadakas_core::model::{Attributes, Shape as ModelShape};
use kadakas_l10n::tr;
use kadakas_render::{Projection, StandardView};
use kadakas_ui_core::modeler::ModelerTool;
use kadakas_ui_core::nav3d::NavAction;

use crate::app::{Answer, Cmd, ModelerApp};
use crate::files::{Ask, Question};
use crate::prefs::{Prefs, ThemeChoice};

/// Answers every question the same way.
struct Always(bool);

impl Ask for Always {
    fn ask(&mut self, _q: &Question) -> bool {
        self.0
    }
}

fn app_with(prefs: Prefs) -> ModelerApp {
    ModelerApp::headless(prefs, Box::new(Always(true)))
}

fn app() -> ModelerApp {
    app_with(Prefs::default())
}

fn raw(events: Vec<egui::Event>, time: f64, modifiers: egui::Modifiers) -> egui::RawInput {
    egui::RawInput {
        modifiers,
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1400.0, 900.0),
        )),
        time: Some(time),
        events,
        ..Default::default()
    }
}

/// One frame. Returns what egui painted.
fn frame(
    ctx: &egui::Context,
    app: &mut ModelerApp,
    events: Vec<egui::Event>,
    time: f64,
) -> egui::FullOutput {
    frame_with(ctx, app, events, time, egui::Modifiers::NONE)
}

/// One frame with keys held.
fn frame_with(
    ctx: &egui::Context,
    app: &mut ModelerApp,
    events: Vec<egui::Event>,
    time: f64,
    modifiers: egui::Modifiers,
) -> egui::FullOutput {
    ctx.run_ui(raw(events, time, modifiers), |ui| app.draw(ui))
}

fn press(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// A key press and its release, so the next press of the same key is a new press.
fn tap(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
    let mut up = press(key, modifiers);
    if let egui::Event::Key { pressed, .. } = &mut up {
        *pressed = false;
    }
    vec![press(key, modifiers), up]
}

fn texts(out: &egui::FullOutput) -> Vec<String> {
    fn walk(shape: &Shape, out: &mut Vec<String>) {
        match shape {
            Shape::Text(t) => out.push(t.galley.text().to_string()),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut all = Vec::new();
    for clipped in &out.shapes {
        walk(&clipped.shape, &mut all);
    }
    all
}

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("kadakas-modeler-app-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn add_square(app: &mut ModelerApp) -> kadakas_core::model::ObjectId {
    let out = app
        .session
        .editor
        .execute(&Command::AddObject {
            shape: ModelShape::Hardscape {
                boundary: kadakas_core::curve::corners(&[
                    DVec2::new(5000.0, 5000.0),
                    DVec2::new(9000.0, 5000.0),
                    DVec2::new(9000.0, 8000.0),
                    DVec2::new(5000.0, 8000.0),
                ]),
                material: "m".into(),
                thickness: Some(100.0),
                elevation: None,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    kadakas_core::model::ObjectId(out.created[0])
}

#[test]
fn the_window_builds_in_both_languages_and_both_themes() {
    for lang in ["en-US", "et"] {
        for theme in [ThemeChoice::Light, ThemeChoice::Dark] {
            let prefs = Prefs {
                theme,
                language: Some(lang.to_string()),
                ..Prefs::default()
            };
            kadakas_l10n::with_language(lang, || {
                let ctx = egui::Context::default();
                let mut app = app_with(prefs);
                add_square(&mut app);
                let mut out = frame(&ctx, &mut app, vec![], 0.0);
                for t in 1..3 {
                    out = frame(&ctx, &mut app, vec![], f64::from(t) * 0.1);
                }
                let shown = texts(&out);
                // The menu bar, the tray and the status bar are in the language of the window.
                for id in [
                    "modeler-menu-file",
                    "modeler-menu-camera",
                    "modeler-tray-entity-info",
                    "modeler-tray-materials",
                    "modeler-tray-scenes",
                    "modeler-needs-wgpu",
                ] {
                    let want = tr!(id);
                    assert!(
                        shown.iter().any(|s| s.contains(&want)),
                        "{lang} {theme:?}: {want:?} is not on the screen: {shown:?}"
                    );
                }
                let expected = match theme {
                    ThemeChoice::Light => egui::Theme::Light,
                    ThemeChoice::Dark => egui::Theme::Dark,
                    ThemeChoice::System => unreachable!(),
                };
                assert_eq!(ctx.theme(), expected, "{lang}");
            });
        }
    }
}

#[test]
fn the_estonian_window_shows_no_english_names_of_the_panels() {
    kadakas_l10n::with_language("et", || {
        let ctx = egui::Context::default();
        let mut app = app_with(Prefs {
            language: Some("et".into()),
            ..Prefs::default()
        });
        let _ = frame(&ctx, &mut app, vec![], 0.0);
        let out = frame(&ctx, &mut app, vec![], 0.1);
        let all = texts(&out).join("\n");
        for english in [
            "Entity Info",
            "Materials",
            "Scenes",
            "Dimensions",
            "Camera",
            "File",
        ] {
            assert!(
                !all.contains(english),
                "{english:?} in the Estonian window:\n{all}"
            );
        }
        assert!(all.contains("Olemi info") && all.contains("Mõõtmed"));
    });
}

#[test]
fn the_tray_can_be_hidden_and_the_window_still_builds() {
    let ctx = egui::Context::default();
    let mut app = app();
    let with_tray = texts(&frame(&ctx, &mut app, vec![], 0.0));
    assert!(
        with_tray
            .iter()
            .any(|s| s == &tr!("modeler-tray-entity-info"))
    );
    app.run(Cmd::ToggleTray, glam::Vec2::new(800.0, 600.0));
    let _ = frame(&ctx, &mut app, vec![], 0.1);
    let without = texts(&frame(&ctx, &mut app, vec![], 0.2));
    assert!(
        !without
            .iter()
            .any(|s| s == &tr!("modeler-tray-entity-info"))
    );
}

#[test]
fn the_entity_info_panel_shows_the_selected_object() {
    let ctx = egui::Context::default();
    let mut app = app();
    let id = add_square(&mut app);
    let none = texts(&frame(&ctx, &mut app, vec![], 0.0)).join("\n");
    assert!(none.contains(&tr!("modeler-entity-nothing")));
    app.session.selection = vec![id];
    let _ = frame(&ctx, &mut app, vec![], 0.1);
    let one = texts(&frame(&ctx, &mut app, vec![], 0.2)).join("\n");
    assert!(one.contains("Hardscape"), "{one}");
    assert!(one.contains(&id.to_string()));
    assert!(one.contains(&tr!("modeler-entity-size")));
    // The Measurements box has the size of the selection, with the label of the box.
    assert!(one.contains(&tr!("uic-modeler-measure-dimensions")));
}

#[test]
fn the_keys_of_the_sketchup_like_profile_choose_the_navigation_tools() {
    let ctx = egui::Context::default();
    let mut app = app();
    let none = egui::Modifiers::NONE;
    let mut t = 0.0;
    for (key, want) in [
        (egui::Key::O, ModelerTool::Orbit),
        (egui::Key::H, ModelerTool::Pan),
        (egui::Key::Z, ModelerTool::Zoom),
        (egui::Key::Space, ModelerTool::Select),
        (egui::Key::O, ModelerTool::Orbit),
        (egui::Key::Escape, ModelerTool::Select),
    ] {
        t += 0.1;
        let _ = frame(&ctx, &mut app, tap(key, none), t);
        assert_eq!(
            app.tools.active(),
            want,
            "{key:?} at {t}: status {:?}, keyboard wanted {}",
            app.status,
            ctx.egui_wants_keyboard_input()
        );
    }
}

#[test]
fn a_tool_that_does_not_run_yet_stays_off_and_says_why() {
    let ctx = egui::Context::default();
    let mut app = app();
    let none = egui::Modifiers::NONE;
    // Paint Bucket is round B of MOD-05; Move, Rotate, Scale and Tape Measure run.
    for key in [egui::Key::B] {
        app.status.clear();
        let _ = frame(&ctx, &mut app, tap(key, none), 0.1);
        assert_eq!(app.tools.active(), ModelerTool::Select, "{key:?}");
        assert!(
            app.status.contains(&tr!("uic-modeler-coming-later")),
            "{key:?}: {}",
            app.status
        );
    }
}

#[test]
fn shift_z_shows_the_whole_model() {
    let ctx = egui::Context::default();
    let mut app = app();
    add_square(&mut app);
    // The first frame places the camera by itself only with a GPU; here the key does it.
    let _ = frame(&ctx, &mut app, vec![], 0.0);
    let (lo, hi) = app.session.scene().bounds.unwrap();
    app.nav.camera.target = Vec3::new(9000.0, -4000.0, 700.0);
    app.nav.camera.distance = 123.0;
    let _ = frame(
        &ctx,
        &mut app,
        tap(egui::Key::Z, egui::Modifiers::SHIFT),
        0.1,
    );
    assert!((app.nav.camera.target - (lo + hi) / 2.0).length() < 1.0);
    assert!(app.nav.camera.distance > 1000.0, "the box fits in the view");
    assert_eq!(
        app.tools.active(),
        ModelerTool::Select,
        "a command does not change the tool"
    );
}

#[test]
fn the_camera_commands_change_the_view() {
    let mut app = app();
    add_square(&mut app);
    let size = glam::Vec2::new(900.0, 600.0);
    app.run(Cmd::Nav(NavAction::View(StandardView::Top)), size);
    assert!((app.nav.camera.eye_dir() - Vec3::Z).length() < 1e-5);
    app.run(Cmd::Nav(NavAction::View(StandardView::Front)), size);
    assert!((app.nav.camera.eye_dir() - Vec3::NEG_Y).length() < 1e-5);
    app.run(Cmd::Projection(false), size);
    assert_eq!(app.nav.camera.projection, Projection::Orthographic);
    app.run(Cmd::Nav(NavAction::ToggleProjection), size);
    assert!(app.nav.is_perspective());
    app.run(Cmd::SetFov(60.0), size);
    assert!((app.nav.fov_deg() - 60.0).abs() < 1e-3);
    app.run(Cmd::ZoomWindow, size);
    assert_eq!(app.tools.active(), ModelerTool::Zoom);
    assert!(app.tools.window_armed());
}

#[test]
fn previous_and_next_view_walk_the_history() {
    let ctx = egui::Context::default();
    let mut app = app();
    add_square(&mut app);
    let size = glam::Vec2::new(900.0, 600.0);
    let mut t = 0.0;
    let settle = |app: &mut ModelerApp, t: &mut f64| {
        for _ in 0..3 {
            *t += 0.5;
            let _ = frame(&ctx, app, vec![], *t);
        }
    };
    settle(&mut app, &mut t);
    let first = app.nav.camera;
    app.run(Cmd::Nav(NavAction::View(StandardView::Top)), size);
    settle(&mut app, &mut t);
    let second = app.nav.camera;
    assert_ne!(first, second);
    app.run(Cmd::Nav(NavAction::Previous), size);
    assert_eq!(app.nav.camera, first);
    app.run(Cmd::Nav(NavAction::Next), size);
    assert_eq!(app.nav.camera, second);
}

#[test]
fn a_model_saved_by_the_modeler_opens_again_and_the_title_follows_the_file() {
    let d = temp("title");
    let path = d.join("yard.kad");
    let mut a = app();
    add_square(&mut a);
    assert!(a.title().contains(&tr!("modeler-untitled")));
    assert!(a.dirty(), "an edited model has unsaved changes");
    assert!(a.title().contains('*'));
    a.save_to(path.clone());
    assert!(a.status.contains("yard.kad"), "{}", a.status);
    assert!(!a.dirty());
    assert!(a.title().starts_with("yard.kad"));

    let mut b = app();
    b.open_path(path.clone());
    assert_eq!(b.session.editor.doc().objects.len(), 1);
    assert_eq!(b.path.as_deref(), Some(path.as_path()));
    assert!(!b.dirty());
    // Opening a model frames it at the next frame.
    let ctx = egui::Context::default();
    let _ = frame(&ctx, &mut b, vec![], 0.0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn select_all_and_none_and_undo_work_on_the_model() {
    let mut app = app();
    let size = glam::Vec2::new(900.0, 600.0);
    let a = add_square(&mut app);
    let b = add_square(&mut app);
    app.run(Cmd::SelectAll, size);
    assert_eq!(app.session.selection.len(), 2);
    assert!(app.session.selection.contains(&a) && app.session.selection.contains(&b));
    app.run(Cmd::SelectNone, size);
    assert!(app.session.selection.is_empty());
    app.run(Cmd::Undo, size);
    assert_eq!(app.session.editor.doc().objects.len(), 1);
    app.run(Cmd::Redo, size);
    assert_eq!(app.session.editor.doc().objects.len(), 2);
    app.run(Cmd::Redo, size);
    assert_eq!(app.status, tr!("modeler-nothing-to-redo"));
}

#[test]
fn the_about_window_opens_from_the_menu_command() {
    let ctx = egui::Context::default();
    let mut app = app();
    app.run(Cmd::About, glam::Vec2::new(900.0, 600.0));
    let _ = frame(&ctx, &mut app, vec![], 0.0);
    let out = frame(&ctx, &mut app, vec![], 0.1);
    assert!(
        texts(&out)
            .iter()
            .any(|s| s.contains(&tr!("modeler-about-description")))
    );
}

#[test]
fn every_key_action_of_the_profile_that_does_something_gives_a_command() {
    use kadakas_ui_core::keymap::Action;
    for a in [
        Action::ToolSelect,
        Action::NavOrbit,
        Action::NavPan,
        Action::NavZoom,
        Action::NavZoomWindow,
        Action::ZoomFit,
        Action::ViewTop,
        Action::ViewBottom,
        Action::ViewFront,
        Action::ViewBack,
        Action::ViewLeft,
        Action::ViewRight,
        Action::ViewRightIso,
        Action::ViewProjection,
        Action::ViewHistoryPrevious,
        Action::ViewHistoryNext,
        Action::FileNew,
        Action::FileOpen,
        Action::FileSave,
        Action::FileSaveAs,
        Action::Undo,
        Action::Redo,
        Action::SelectAll,
        Action::SelectNone,
        Action::Cancel,
    ] {
        assert!(ModelerApp::cmd_of(a).is_some(), "{a:?}");
    }
    assert_eq!(ModelerApp::cmd_of(Action::ToolText), None);
    assert_eq!(
        ModelerApp::cmd_of(Action::ZoomFit),
        Some(Cmd::Tool(ModelerTool::ZoomExtents)),
        "Shift+Z is the Zoom Extents tool of the tool bar"
    );
}

#[test]
fn the_theme_and_the_language_menu_commands_change_the_settings() {
    let mut app = app();
    let size = glam::Vec2::new(900.0, 600.0);
    app.run(Cmd::Theme(ThemeChoice::Dark), size);
    assert_eq!(app.prefs.theme, ThemeChoice::Dark);
    app.run(Cmd::Language(Some("et".into())), size);
    assert_eq!(app.prefs.language.as_deref(), Some("et"));
    app.run(Cmd::Language(None), size);
    assert_eq!(app.prefs.language, None);
}

// ---- the pointer: a test seam with a GPU that is only claimed ----------------------------------
//
// With `gpu_ready` set, the canvas lays out, reads the pointer and the wheel, navigates and puts a
// paint callback in the frame. Nothing is drawn, because no GPU runs, but every input path is real.

/// A program with a model and a canvas that reads the pointer. The first frames fit the view.
fn live() -> (egui::Context, ModelerApp, f64) {
    let ctx = egui::Context::default();
    let mut app = app();
    app.gpu_ready = true;
    add_square(&mut app);
    let mut t = 0.0;
    for _ in 0..3 {
        let _ = frame(&ctx, &mut app, vec![], t);
        t += 0.1;
    }
    assert!(app.canvas_rect.is_some());
    (ctx, app, t)
}

fn center(app: &ModelerApp) -> egui::Pos2 {
    app.canvas_rect.unwrap().center()
}

fn size(app: &ModelerApp) -> glam::Vec2 {
    let r = app.canvas_rect.unwrap();
    glam::Vec2::new(r.width(), r.height())
}

fn at(app: &ModelerApp, p: egui::Pos2) -> glam::Vec2 {
    let r = app.canvas_rect.unwrap();
    glam::Vec2::new(p.x - r.min.x, p.y - r.min.y)
}

fn moved(p: egui::Pos2) -> egui::Event {
    egui::Event::PointerMoved(p)
}

fn button(
    p: egui::Pos2,
    b: egui::PointerButton,
    pressed: bool,
    modifiers: egui::Modifiers,
) -> egui::Event {
    egui::Event::PointerButton {
        pos: p,
        button: b,
        pressed,
        modifiers,
    }
}

/// The pointer goes to `from`, presses `b`, moves to `to` in a few frames with `during` held,
/// and lets go with `release` held. Returns the time after the last frame.
#[allow(clippy::too_many_arguments)]
fn drag(
    ctx: &egui::Context,
    app: &mut ModelerApp,
    b: egui::PointerButton,
    from: egui::Pos2,
    to: egui::Pos2,
    at_press: egui::Modifiers,
    during: egui::Modifiers,
    mut t: f64,
) -> f64 {
    let _ = frame_with(ctx, app, vec![moved(from)], t, at_press);
    t += 0.05;
    let _ = frame_with(ctx, app, vec![button(from, b, true, at_press)], t, at_press);
    t += 0.05;
    for i in 1..=6 {
        let k = i as f32 / 6.0;
        let p = from + (to - from) * k;
        let _ = frame_with(
            ctx,
            app,
            vec![moved(p)],
            t,
            if i == 1 { at_press } else { during },
        );
        t += 0.05;
    }
    let _ = frame_with(ctx, app, vec![button(to, b, false, during)], t, during);
    t += 0.05;
    let _ = frame(ctx, app, vec![], t);
    t + 0.05
}

#[test]
fn the_wheel_zooms_to_the_pointer_and_the_point_under_it_stays() {
    let (ctx, mut app, mut t) = live();
    let sz = size(&app);
    // Look at the slab from the iso view, in perspective.
    assert!(app.nav.is_perspective());
    let p = center(&app) + egui::vec2(60.0, 40.0);
    let _ = frame(&ctx, &mut app, vec![moved(p)], t);
    t += 0.05;
    let scene = app.session.scene();
    let hit = kadakas_ui_core::nav3d::point_under(&scene, &app.nav.camera, at(&app, p), sz)
        .expect("the pointer is over the slab");
    let depth = app.nav.depth_of(hit);
    let want = {
        let n = app
            .nav
            .camera
            .project(hit, sz.x / sz.y, scene.bounds)
            .unwrap();
        glam::Vec2::new(n.x, n.y)
    };
    for _ in 0..12 {
        let ev = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            phase: egui::TouchPhase::Move,
            delta: egui::vec2(0.0, 25.0),
            modifiers: egui::Modifiers::NONE,
        };
        let _ = frame(&ctx, &mut app, vec![ev], t);
        t += 0.05;
    }
    for _ in 0..30 {
        let _ = frame(&ctx, &mut app, vec![], t);
        t += 0.05;
    }
    let now = app.nav.depth_of(hit);
    assert!(
        now < depth * 0.9,
        "zoomed in: the point is nearer ({depth} to {now})"
    );
    let got = {
        let n = app
            .nav
            .camera
            .project(hit, sz.x / sz.y, scene.bounds)
            .unwrap();
        glam::Vec2::new(n.x, n.y)
    };
    assert!(
        (got - want).length() < 0.02,
        "the point stayed under the pointer: {got:?} {want:?}"
    );
    // A turn the other way zooms out.
    for _ in 0..12 {
        let ev = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            phase: egui::TouchPhase::Move,
            delta: egui::vec2(0.0, -25.0),
            modifiers: egui::Modifiers::NONE,
        };
        let _ = frame(&ctx, &mut app, vec![ev], t);
        t += 0.05;
    }
    for _ in 0..30 {
        let _ = frame(&ctx, &mut app, vec![], t);
        t += 0.05;
    }
    assert!(app.nav.depth_of(hit) > now);
}

#[test]
fn a_middle_button_drag_orbits_in_every_tool_and_shift_pans() {
    let (ctx, mut app, t) = live();
    let c = center(&app);
    let before = app.nav.camera;
    let t = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Middle,
        c,
        c + egui::vec2(90.0, 20.0),
        egui::Modifiers::NONE,
        egui::Modifiers::NONE,
        t,
    );
    let after = app.nav.camera;
    assert!(
        (after.eye_dir() - before.eye_dir()).length() > 0.1,
        "the view turned"
    );
    assert_eq!(
        app.tools.active(),
        ModelerTool::Select,
        "the tool did not change"
    );
    // The point under the pointer when the drag started is the pivot: it stays where it was.
    let sz = size(&app);
    let scene = app.session.scene();
    let aspect = sz.x / sz.y;
    let pivot = kadakas_ui_core::nav3d::point_under(&scene, &before, at(&app, c), sz).unwrap();
    let a = before.project(pivot, aspect, scene.bounds).unwrap();
    let b = after.project(pivot, aspect, scene.bounds).unwrap();
    assert!(
        (a - b).truncate().length() < 0.02,
        "pivot moved {a:?} {b:?}"
    );

    // Shift and the middle button pan: the target moves, the direction does not.
    let before = app.nav.camera;
    let _ = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Middle,
        c,
        c + egui::vec2(-80.0, 30.0),
        egui::Modifiers::SHIFT,
        egui::Modifiers::SHIFT,
        t,
    );
    let after = app.nav.camera;
    assert!((after.target - before.target).length() > 10.0, "panned");
    assert!((after.eye_dir() - before.eye_dir()).length() < 1e-4);
    assert_eq!(after.distance, before.distance);
}

#[test]
fn the_orbit_pan_and_zoom_tools_use_the_primary_button() {
    let (ctx, mut app, mut t) = live();
    let c = center(&app);
    // The Select tool does not navigate with the primary button.
    let before = app.nav.camera;
    t = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        c,
        c + egui::vec2(60.0, 0.0),
        egui::Modifiers::NONE,
        egui::Modifiers::NONE,
        t,
    );
    assert_eq!(app.nav.camera, before);
    // Orbit tool.
    let _ = frame(&ctx, &mut app, tap(egui::Key::O, egui::Modifiers::NONE), t);
    t += 0.1;
    t = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        c,
        c + egui::vec2(60.0, 0.0),
        egui::Modifiers::NONE,
        egui::Modifiers::NONE,
        t,
    );
    assert!((app.nav.camera.eye_dir() - before.eye_dir()).length() > 0.05);
    // Pan tool.
    let _ = frame(&ctx, &mut app, tap(egui::Key::H, egui::Modifiers::NONE), t);
    t += 0.1;
    let before = app.nav.camera;
    t = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        c,
        c + egui::vec2(40.0, 40.0),
        egui::Modifiers::NONE,
        egui::Modifiers::NONE,
        t,
    );
    assert!((app.nav.camera.target - before.target).length() > 10.0);
    // Zoom tool: a drag up zooms in. Shift and a drag change the field of view.
    let _ = frame(&ctx, &mut app, tap(egui::Key::Z, egui::Modifiers::NONE), t);
    t += 0.1;
    let before = app.nav.camera;
    t = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        c,
        c - egui::vec2(0.0, 80.0),
        egui::Modifiers::NONE,
        egui::Modifiers::NONE,
        t,
    );
    assert!(app.nav.camera.distance < before.distance, "up zooms in");
    let fov = app.nav.fov_deg();
    let _ = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        c,
        c - egui::vec2(0.0, 80.0),
        egui::Modifiers::SHIFT,
        egui::Modifiers::SHIFT,
        t,
    );
    assert!(app.nav.fov_deg() < fov, "up narrows the field of view");
}

#[test]
fn zoom_window_draws_a_window_and_zooms_to_it() {
    let (ctx, mut app, mut t) = live();
    let _ = frame(&ctx, &mut app, tap(egui::Key::Z, egui::Modifiers::NONE), t);
    t += 0.1;
    let c = center(&app);
    let (from, to) = (c + egui::vec2(-100.0, -60.0), c + egui::vec2(100.0, 60.0));
    let cmd = egui::Modifiers::COMMAND;
    let before = app.nav.camera;
    // Press and move: the window shows.
    let _ = frame_with(&ctx, &mut app, vec![moved(from)], t, cmd);
    t += 0.05;
    let _ = frame_with(
        &ctx,
        &mut app,
        vec![button(from, egui::PointerButton::Primary, true, cmd)],
        t,
        cmd,
    );
    t += 0.05;
    for i in 1..=4 {
        let p = from + (to - from) * (i as f32 / 4.0);
        let _ = frame_with(&ctx, &mut app, vec![moved(p)], t, cmd);
        t += 0.05;
    }
    let w = crate::canvas::window_drag(&ctx).expect("the window shows while you drag");
    assert!((w.to - w.from).length() > 100.0);
    assert_eq!(
        app.nav.camera, before,
        "nothing zooms before the button is released"
    );
    // Release.
    let _ = frame_with(
        &ctx,
        &mut app,
        vec![button(to, egui::PointerButton::Primary, false, cmd)],
        t,
        cmd,
    );
    t += 0.05;
    let _ = frame(&ctx, &mut app, vec![], t);
    assert!(
        crate::canvas::window_drag(&ctx).is_none(),
        "the window is gone"
    );
    assert!(
        app.nav.camera.distance < before.distance,
        "zoomed to the window"
    );
}

#[test]
fn letting_go_of_the_key_first_does_not_leave_the_zoom_window_on_the_screen() {
    let (ctx, mut app, mut t) = live();
    let _ = frame(&ctx, &mut app, tap(egui::Key::Z, egui::Modifiers::NONE), t);
    t += 0.1;
    let c = center(&app);
    let (from, to) = (c + egui::vec2(-90.0, -50.0), c + egui::vec2(90.0, 50.0));
    let cmd = egui::Modifiers::COMMAND;
    let before = app.nav.camera;
    // Cmd is held when the drag starts, and let go while the button is still down.
    t = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        from,
        to,
        cmd,
        egui::Modifiers::NONE,
        t,
    );
    assert!(
        crate::canvas::window_drag(&ctx).is_none(),
        "no window stays"
    );
    assert!(
        app.nav.camera.distance < before.distance,
        "the drag was decided at its start: it is still a window"
    );
    // The next drag with no key is a plain zoom, not a window.
    let again = app.nav.camera;
    let _ = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        c,
        c - egui::vec2(0.0, 60.0),
        egui::Modifiers::NONE,
        egui::Modifiers::NONE,
        t,
    );
    assert!(crate::canvas::window_drag(&ctx).is_none());
    assert!(app.nav.camera.distance < again.distance);
    // Zoom Window armed by its key: the next drag is a window even without Cmd, and only once.
    app.run(Cmd::ZoomWindow, size(&app));
    assert!(app.tools.window_armed());
    let _ = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        from,
        to,
        egui::Modifiers::NONE,
        egui::Modifiers::NONE,
        t + 2.0,
    );
    assert!(!app.tools.window_armed());
    assert!(crate::canvas::window_drag(&ctx).is_none());
}

#[test]
fn a_click_with_the_select_tool_selects_the_object_under_the_pointer() {
    let (ctx, mut app, mut t) = live();
    let c = center(&app);
    assert!(app.session.selection.is_empty());
    let _ = frame(&ctx, &mut app, vec![moved(c)], t);
    t += 0.05;
    let _ = frame(
        &ctx,
        &mut app,
        vec![button(
            c,
            egui::PointerButton::Primary,
            true,
            egui::Modifiers::NONE,
        )],
        t,
    );
    t += 0.05;
    let _ = frame(
        &ctx,
        &mut app,
        vec![button(
            c,
            egui::PointerButton::Primary,
            false,
            egui::Modifiers::NONE,
        )],
        t,
    );
    t += 0.05;
    let _ = frame(&ctx, &mut app, vec![], t);
    t += 0.05;
    assert_eq!(app.session.selection.len(), 1, "the slab is selected");
    // The Entity Info panel shows it.
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    assert!(shown.contains("Hardscape"), "{shown}");
    // A click on empty space clears the selection.
    let corner = app.canvas_rect.unwrap().min + egui::vec2(6.0, 6.0);
    t += 0.05;
    let _ = frame(&ctx, &mut app, vec![moved(corner)], t);
    t += 0.05;
    let _ = frame(
        &ctx,
        &mut app,
        vec![button(
            corner,
            egui::PointerButton::Primary,
            true,
            egui::Modifiers::NONE,
        )],
        t,
    );
    t += 0.05;
    let _ = frame(
        &ctx,
        &mut app,
        vec![button(
            corner,
            egui::PointerButton::Primary,
            false,
            egui::Modifiers::NONE,
        )],
        t,
    );
    t += 0.05;
    let _ = frame(&ctx, &mut app, vec![], t);
    assert!(app.session.selection.is_empty());
    // Other tools do not select with a click.
    let _ = frame(
        &ctx,
        &mut app,
        tap(egui::Key::O, egui::Modifiers::NONE),
        t + 0.1,
    );
    let t = t + 0.2;
    let _ = frame(&ctx, &mut app, vec![moved(c)], t);
    let _ = frame(
        &ctx,
        &mut app,
        vec![button(
            c,
            egui::PointerButton::Primary,
            true,
            egui::Modifiers::NONE,
        )],
        t + 0.05,
    );
    let _ = frame(
        &ctx,
        &mut app,
        vec![button(
            c,
            egui::PointerButton::Primary,
            false,
            egui::Modifiers::NONE,
        )],
        t + 0.1,
    );
    assert!(app.session.selection.is_empty());
}

// ---- unsaved changes ---------------------------------------------------------------------------

fn saved_file(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let d = temp(name);
    let path = d.join("other.kad");
    let mut other = app();
    other.save_to(path.clone());
    (d, path)
}

#[test]
fn a_model_with_changes_is_not_replaced_until_the_person_answers() {
    let (d, path) = saved_file("unsaved");
    let mut a = app();
    add_square(&mut a);
    assert!(a.dirty());
    // Open: the file waits, and nothing changes.
    a.open_path(path.clone());
    assert_eq!(a.pending, Some(crate::app::Replace::Open(path.clone())));
    assert_eq!(a.session.editor.doc().objects.len(), 1);
    // A second request does not take the place of the first: it is dropped and said.
    a.new_document();
    assert_eq!(a.pending, Some(crate::app::Replace::Open(path.clone())));
    assert!(a.status.contains("other.kad"), "{}", a.status);
    // Cancel keeps the model.
    a.answer(Answer::Cancel);
    assert_eq!(a.pending, None);
    assert_eq!(a.session.editor.doc().objects.len(), 1);
    assert!(a.dirty());
    // Do not save: the file opens and the changes are gone.
    a.open_path(path.clone());
    a.answer(Answer::DontSave);
    assert_eq!(a.session.editor.doc().objects.len(), 0);
    assert_eq!(a.path.as_deref(), Some(path.as_path()));
    assert!(!a.dirty(), "an opened file has no unsaved changes");
    // New with changes asks too.
    add_square(&mut a);
    a.new_document();
    assert_eq!(a.pending, Some(crate::app::Replace::New));
    a.answer(Answer::DontSave);
    assert_eq!(a.session.editor.doc().objects.len(), 0);
    assert_eq!(a.path, None);
    // Without changes nothing is asked.
    a.open_path(path.clone());
    assert_eq!(a.pending, None);
    assert_eq!(a.path.as_deref(), Some(path.as_path()));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn save_and_open_saves_first_and_a_failed_save_stops_everything() {
    let (d, other) = saved_file("save-and-open");
    let mine = d.join("mine.kad");
    let mut a = app();
    add_square(&mut a);
    a.path = Some(mine.clone());
    a.open_path(other.clone());
    a.answer(Answer::SaveAndGo);
    assert!(mine.is_file(), "the changes were saved first");
    assert_eq!(
        a.path.as_deref(),
        Some(other.as_path()),
        "then the file opened"
    );
    assert!(!a.dirty());
    // A save that cannot be done keeps the model and does not open the file.
    let mut b = app();
    add_square(&mut b);
    b.path = Some(d.join("no-such-folder").join("x.kad"));
    b.open_path(other.clone());
    b.answer(Answer::SaveAndGo);
    assert!(b.dirty());
    assert_eq!(b.session.editor.doc().objects.len(), 1);
    assert_eq!(
        b.path.as_deref(),
        Some(d.join("no-such-folder").join("x.kad").as_path())
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn quit_and_the_close_button_ask_when_there_are_unsaved_changes() {
    let ctx = egui::Context::default();
    let mut a = app();
    let sz = glam::Vec2::new(900.0, 600.0);
    a.run(Cmd::Quit, sz);
    assert!(a.quitting(), "no changes: it ends");
    let mut b = app();
    add_square(&mut b);
    b.run(Cmd::Quit, sz);
    assert!(!b.quitting());
    assert_eq!(b.pending, Some(crate::app::Replace::Quit));
    b.answer(Answer::Cancel);
    assert!(!b.quitting() && b.pending.is_none());
    // The close button of the window: the window stays and the question comes.
    let mut c = app();
    add_square(&mut c);
    let mut input = raw(vec![], 0.0, egui::Modifiers::NONE);
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
    let _ = ctx.run_ui(input, |ui| c.draw(ui));
    assert_eq!(c.pending, Some(crate::app::Replace::Quit));
    assert!(!c.quitting());
    c.answer(Answer::DontSave);
    assert!(c.quitting());
    // The question is a window of the frame.
    let mut d = app();
    add_square(&mut d);
    d.open_path(std::path::PathBuf::from("x.kad"));
    let _ = frame(&ctx, &mut d, vec![], 1.0);
    let out = frame(&ctx, &mut d, vec![], 1.1);
    let all = texts(&out).join("\n");
    assert!(all.contains(&tr!("modeler-unsaved-title")), "{all}");
    assert!(all.contains(&tr!("modeler-unsaved-save-and-open")));
    assert!(all.contains(&tr!("modeler-unsaved-dont-save")));
    assert!(all.contains(&tr!("modeler-unsaved-cancel")));
}

// ---- the launcher ------------------------------------------------------------------------------

#[test]
fn a_document_that_the_launcher_sends_opens_in_the_running_modeler() {
    use kadakas_update::handshake::{AppLink, ENV_ROOT, ENV_RUN, heartbeat_exists, send_open};
    let (d, path) = saved_file("launcher-open");
    let root = d.join("launcher");
    std::fs::create_dir_all(&root).unwrap();
    let env = {
        let (root, id) = (root.display().to_string(), "1a-2b".to_string());
        move |k: &str| match k {
            ENV_ROOT => Some(root.clone()),
            ENV_RUN => Some(id.clone()),
            _ => None,
        }
    };
    let link = AppLink::from_env(&env).unwrap();
    let mut a = app();
    a.set_launch(crate::launch::Launch::new(Some(link)));
    // The alive stamp is written by a timer thread of the program.
    let mut alive = false;
    for _ in 0..100 {
        if heartbeat_exists(&root, "1a-2b") {
            alive = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    assert!(alive, "the program writes the alive stamp");
    let ctx = egui::Context::default();
    let _ = frame(&ctx, &mut a, vec![], 0.0);
    assert_eq!(a.session.editor.doc().objects.len(), 0);
    send_open(&root, "1a-2b", std::slice::from_ref(&path)).unwrap();
    // The requests are looked at twice a second at most.
    let _ = frame(&ctx, &mut a, vec![], 0.1);
    assert_eq!(a.path, None);
    let _ = frame(&ctx, &mut a, vec![], 0.7);
    assert_eq!(a.path.as_deref(), Some(path.as_path()), "the file opened");
    assert!(
        !kadakas_update::handshake::open_pending(&root, "1a-2b"),
        "the request was taken"
    );
    // With unsaved changes the sent file waits for the answer, like File > Open.
    add_square(&mut a);
    let second = d.join("second.kad");
    let mut s = app();
    s.save_to(second.clone());
    send_open(&root, "1a-2b", std::slice::from_ref(&second)).unwrap();
    let _ = frame(&ctx, &mut a, vec![], 1.5);
    assert_eq!(a.pending, Some(crate::app::Replace::Open(second.clone())));
    assert_eq!(a.path.as_deref(), Some(path.as_path()));
    a.answer(Answer::DontSave);
    assert_eq!(a.path.as_deref(), Some(second.as_path()));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_actions_of_the_modeler_are_the_ones_it_runs() {
    use kadakas_ui_core::keymap::Action;
    // Every action that only the modeler has is run by it, so it can list them.
    for a in Action::ALL.iter().filter(|a| a.modeler_only()) {
        assert!(ModelerApp::runs(*a), "{a:?}");
    }
    assert!(ModelerApp::runs(Action::FileSave) && ModelerApp::runs(Action::ZoomFit));
    // The tools of the Vectorworks-like app are not.
    for a in [
        Action::ToolText,
        Action::ToolPlant,
        Action::CmdExtrude,
        Action::ToolMirror,
    ] {
        assert!(!ModelerApp::runs(a), "{a:?}");
    }
}

mod modeling;

/// A surface model with a square, selected.
fn add_model_square(app: &mut ModelerApp, x: f64) -> kadakas_core::model::ObjectId {
    use kadakas_core::surface::Edit;
    let out = app
        .session
        .editor
        .execute(&Command::AddObject {
            shape: ModelShape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    let id = kadakas_core::model::ObjectId(out.created[0]);
    let p = [
        [x, 0.0, 0.0],
        [x + 1000.0, 0.0, 0.0],
        [x + 1000.0, 1000.0, 0.0],
        [x, 1000.0, 0.0],
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
    id
}

#[test]
fn the_edit_menu_makes_groups_and_components_and_enters_them() {
    let size = glam::Vec2::new(800.0, 600.0);
    let mut app = app();
    let a = add_model_square(&mut app, 0.0);
    let b = add_model_square(&mut app, 3000.0);
    // Nothing is selected: the commands say so and change nothing.
    assert!(!app.can_structure(&Cmd::MakeGroup));
    let depth = app.session.editor.undo_depth();
    app.run(Cmd::MakeGroup, size);
    assert_eq!(app.status, tr!("modeler-nothing-selected"));
    assert_eq!(app.session.editor.undo_depth(), depth);
    app.run(Cmd::EditSelected, size);
    assert_eq!(app.status, tr!("modeler-select-one-to-edit"));
    // Make Group of the two models: one undo step, the group is selected.
    app.session.selection = vec![a, b];
    assert!(app.can_structure(&Cmd::MakeGroup) && !app.can_structure(&Cmd::Explode));
    app.run(Cmd::MakeGroup, size);
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    assert_eq!(app.session.selection.len(), 1);
    let g = app.session.selection[0];
    assert!(app.session.editor.doc().is_group(g));
    // Edit Group: the context is open, the selection is clear, the rest cannot be chosen.
    assert!(app.can_structure(&Cmd::EditSelected));
    app.run(Cmd::EditSelected, size);
    assert_eq!(app.context.path(), vec![g]);
    assert!(app.session.selection.is_empty());
    assert!(app.can_structure(&Cmd::CloseContext));
    app.run(Cmd::CloseContext, size);
    assert!(app.context.is_root());
    assert_eq!(app.session.selection, vec![g]);
    // Make Component of the group: the definition has a name that is not taken.
    app.run(Cmd::MakeComponent, size);
    let inst = app.session.selection[0];
    assert!(matches!(
        app.session.editor.doc().object(inst).unwrap().shape,
        ModelShape::Component { .. }
    ));
    assert_eq!(
        app.session
            .editor
            .doc()
            .resources
            .iter()
            .next()
            .unwrap()
            .name,
        "Component 1"
    );
    assert!(app.can_structure(&Cmd::Explode) && app.can_structure(&Cmd::EditSelected));
    // Make Unique on the only instance says why it cannot.
    assert!(app.can_structure(&Cmd::MakeUnique));
    app.run(Cmd::MakeUnique, size);
    assert!(app.status.contains("already unique"), "{}", app.status);
    // Edit Component, then undo: the open component is dropped from the context.
    app.run(Cmd::EditSelected, size);
    assert_eq!(app.context.path(), vec![inst]);
    app.run(Cmd::Undo, size);
    assert!(app.context.is_root());
    // Enter again and close with the menu: one undo step each.
    app.run(Cmd::EditSelected, size);
    assert!(
        app.context.is_root(),
        "the selection was cleared when the context opened"
    );
    app.session.selection = vec![inst];
    app.run(Cmd::EditSelected, size);
    let open_depth = app.session.editor.undo_depth();
    app.run(Cmd::CloseContext, size);
    assert_eq!(app.session.editor.undo_depth(), open_depth + 1);
    // Explode the instance: the models are back, merged into one.
    app.session.selection = vec![inst];
    app.run(Cmd::Explode, size);
    assert!(app.session.selection.is_empty());
    assert!(app.session.editor.doc().resources.is_empty());
    // A new document starts with a closed context.
    app.run(Cmd::New, size);
    assert!(app.context.is_root());
}

#[test]
fn a_command_that_cannot_run_shows_its_message() {
    let size = glam::Vec2::new(800.0, 600.0);
    let mut app = app();
    let id = add_square(&mut app);
    app.session.selection = vec![id];
    // A hardscape cannot be part of a component.
    app.run(Cmd::MakeComponent, size);
    assert!(
        app.status.contains("cannot be part of a component"),
        "{}",
        app.status
    );
    // Nothing of the selection is a group: Explode has nothing to do.
    assert!(!app.can_structure(&Cmd::Explode));
    app.run(Cmd::Explode, size);
    assert!(app.status.contains("at least one"), "{}", app.status);
    // Edit on something that is neither a group nor a component.
    app.run(Cmd::EditSelected, size);
    assert!(
        app.status.contains("only a group or a component"),
        "{}",
        app.status
    );
    assert!(!app.can_structure(&Cmd::Tool(ModelerTool::Select)));
    // Close with nothing open does nothing.
    app.status.clear();
    app.run(Cmd::CloseContext, size);
    assert!(app.status.is_empty());
}

#[test]
fn the_edit_menu_shows_the_group_and_component_items_in_both_languages() {
    for lang in ["en-US", "et"] {
        kadakas_l10n::with_language(lang, || {
            for id in [
                "modeler-edit-make-group",
                "modeler-edit-make-component",
                "modeler-edit-explode",
                "modeler-edit-make-unique",
                "modeler-edit-edit-selected",
                "modeler-edit-close-context",
            ] {
                assert!(!tr!(id).is_empty());
                assert_ne!(tr!(id), id, "{lang}: {id}");
            }
            assert_ne!(
                tr!("modeler-component-name", n = 2),
                "modeler-component-name"
            );
        });
    }
}
