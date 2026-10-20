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

use crate::app::{Cmd, ModelerApp};
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

fn raw(events: Vec<egui::Event>, time: f64) -> egui::RawInput {
    egui::RawInput {
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
    ctx.run_ui(raw(events, time), |ui| app.draw(ui))
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
    for key in [
        egui::Key::L,
        egui::Key::R,
        egui::Key::C,
        egui::Key::A,
        egui::Key::P,
        egui::Key::M,
        egui::Key::E,
        egui::Key::B,
        egui::Key::T,
        egui::Key::Q,
        egui::Key::S,
    ] {
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
fn unsaved_changes_are_not_lost_without_a_yes() {
    let d = temp("discard");
    let path = d.join("other.kad");
    let mut other = app();
    other.save_to(path.clone());
    let mut a = ModelerApp::headless(Prefs::default(), Box::new(Always(false)));
    add_square(&mut a);
    a.open_path(path.clone());
    assert_eq!(
        a.session.editor.doc().objects.len(),
        1,
        "the person said no: nothing was replaced"
    );
    a.new_document();
    assert_eq!(a.session.editor.doc().objects.len(), 1);
    let mut yes = ModelerApp::headless(Prefs::default(), Box::new(Always(true)));
    add_square(&mut yes);
    yes.new_document();
    assert_eq!(yes.session.editor.doc().objects.len(), 0);
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
