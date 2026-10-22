//! Landscape objects in the modeler (MOD-05, round B): a document made with the API holds a plant,
//! a hardscape and a site model on a raised layer. The modeler shows them at the elevation of the
//! layer, selects them (click, window and crossing), says what they are in Entity Info, moves,
//! turns and scales them, and starts the Kadakas app for the rest.

use super::transform::{screen_of, type_more};
use super::*;
use glam::DVec3;
use kadakas_core::model::ObjectId;

use crate::app::Replace;
use crate::tray::TrayAction;

struct Site {
    plant: ObjectId,
    slab: ObjectId,
    terrain: ObjectId,
}

const LAYER_Z: f64 = 1500.0;

fn add(app: &mut ModelerApp, layer: kadakas_core::model::LayerId, shape: ModelShape) -> ObjectId {
    app.session
        .editor
        .execute(&Command::AddObject {
            shape,
            layer: Some(layer),
            class: None,
            attrs: Attributes::default(),
        })
        .map(|o| ObjectId(o.created[0]))
        .unwrap()
}

/// A plant, a hardscape and a site model on a layer 1,500 mm above zero.
fn landscape(app: &mut ModelerApp) -> Site {
    let layer = app
        .session
        .editor
        .execute(&Command::AddLayer {
            name: "Terrace".into(),
            elevation: LAYER_Z,
            scale: None,
            kind: None,
        })
        .map(|o| kadakas_core::model::LayerId(o.created[0]))
        .unwrap();
    let plant = add(
        app,
        layer,
        ModelShape::Plant {
            at: DVec2::new(-1200.0, 300.0),
            species: "Quercus robur".into(),
            spread: 1200.0,
            height: 3000.0,
        },
    );
    let slab = add(
        app,
        layer,
        ModelShape::Hardscape {
            boundary: kadakas_core::curve::corners(&[
                DVec2::new(400.0, -900.0),
                DVec2::new(1200.0, -900.0),
                DVec2::new(1200.0, -300.0),
                DVec2::new(400.0, -300.0),
            ]),
            material: "Setts".into(),
            thickness: Some(100.0),
            elevation: None,
        },
    );
    let p = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
    let terrain = add(
        app,
        layer,
        ModelShape::SiteModel {
            points: vec![
                p(-600.0, 500.0, 0.0),
                p(1200.0, 500.0, 0.0),
                p(1200.0, 1000.0, 200.0),
                p(-600.0, 1000.0, 200.0),
                p(300.0, 750.0, 400.0),
            ],
            lines: Vec::new(),
            settings: Default::default(),
        },
    );
    Site {
        plant,
        slab,
        terrain,
    }
}

/// A live top view with the landscape, the whole model in view and the tray hidden.
fn ready_site() -> (egui::Context, ModelerApp, f64, Site) {
    let (ctx, mut app, mut t) = top_view();
    let site = landscape(&mut app);
    app.run(Cmd::ToggleTray, size(&app));
    t += 0.1;
    let _ = frame(&ctx, &mut app, tap(egui::Key::Z, egui::Modifiers::SHIFT), t);
    for _ in 0..3 {
        t += 0.1;
        let _ = frame(&ctx, &mut app, vec![], t);
    }
    (ctx, app, t, site)
}

/// The box around an object in the scene, in world coordinates.
fn scene_box(app: &mut ModelerApp, id: ObjectId) -> (DVec3, DVec3) {
    let scene = app.session.scene();
    let (lo, hi) = scene.object_bounds(id).expect("the object is drawn");
    (scene.origin + lo.as_dvec3(), scene.origin + hi.as_dvec3())
}

fn plan_point(app: &mut ModelerApp, id: ObjectId, fx: f64, fy: f64) -> egui::Pos2 {
    let (lo, hi) = scene_box(app, id);
    let p = DVec3::new(lo.x + (hi.x - lo.x) * fx, lo.y + (hi.y - lo.y) * fy, hi.z);
    screen_of(app, p)
}

#[test]
fn the_modeler_draws_the_plant_the_hardscape_and_the_site_model_at_the_layer_elevation() {
    let (_ctx, mut app, _t, site) = ready_site();
    let scene = app.session.scene();
    for (name, id) in [
        ("plant", site.plant),
        ("hardscape", site.slab),
        ("site model", site.terrain),
    ] {
        assert!(scene.objects().any(|o| o == id), "{name} is in the scene");
        let (lo, hi) = scene_box(&mut app, id);
        assert!(
            hi.z >= LAYER_Z - 1.0,
            "{name} sits on the layer: {lo:?} {hi:?}"
        );
    }
    // The plant stands on the layer and reaches its height; the slab hangs below its top.
    let (lo, hi) = scene_box(&mut app, site.plant);
    assert!(
        lo.z >= LAYER_Z - 1.0 && hi.z > LAYER_Z + 2000.0,
        "{lo:?} {hi:?}"
    );
    let (lo, hi) = scene_box(&mut app, site.slab);
    assert!(
        (hi.z - LAYER_Z).abs() < 1.0 && (hi.z - lo.z - 100.0).abs() < 1.0,
        "{lo:?} {hi:?}"
    );
    // The picture has colour where the objects are.
    let look = kadakas_render::look::Look::build(app.session.editor.doc(), &scene, true);
    let img = kadakas_render::shade::render_scene(
        &scene,
        &look,
        &app.nav.camera,
        &kadakas_render::shade::ShadeOptions {
            width: 100,
            height: 70,
            edges: false,
            ..Default::default()
        },
    );
    let colored = (0..70)
        .flat_map(|y| (0..100).map(move |x| (x, y)))
        .filter(|(x, y)| img.pixel(*x, *y) != img.pixel(0, 0))
        .count();
    assert!(colored > 300, "the objects are on the picture: {colored}");
}

#[test]
fn a_click_selects_each_kind_and_entity_info_says_what_it_is() {
    let (ctx, mut app, mut t, site) = ready_site();
    app.run(Cmd::ToggleTray, size(&app));
    for (id, want) in [
        (site.plant, vec!["Quercus robur"]),
        (site.slab, vec!["Setts"]),
        (site.terrain, vec![]),
    ] {
        let at = plan_point(&mut app, id, 0.5, 0.5);
        click_at(&ctx, &mut app, at, &mut t);
        assert_eq!(app.session.selection, vec![id], "{want:?}");
        t += 0.1;
        let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
        for w in want {
            assert!(shown.contains(w), "{w}: {shown}");
        }
        // Plants, hardscapes and site models are edited in the Kadakas app.
        assert!(
            shown.contains(&tr!("modeler-entity-open-in-app")),
            "{shown}"
        );
    }
    // The summary of a site model: points and extent.
    let shown = texts(&frame(&ctx, &mut app, vec![], t + 0.1)).join("\n");
    assert!(
        shown.contains(&tr!("modeler-entity-survey-points")),
        "{shown}"
    );
    assert!(shown.contains(&tr!("modeler-entity-extent")), "{shown}");
    // The summary data.
    let scene = app.session.scene();
    let doc = app.session.editor.doc();
    use kadakas_ui_core::modeler::{Summary, summary_of};
    match summary_of(doc, doc.object(site.terrain).unwrap()) {
        Some(Summary::SiteModel {
            points,
            lines,
            extent: Some(e),
            ..
        }) => {
            assert_eq!((points, lines), (5, 0));
            assert!(
                (e[0] - 1800.0).abs() < 1e-9
                    && (e[1] - 500.0).abs() < 1e-9
                    && (e[2] - 400.0).abs() < 1e-9
            );
        }
        other => panic!("{other:?}"),
    }
    match summary_of(doc, doc.object(site.slab).unwrap()) {
        Some(Summary::Hardscape { area, .. }) => assert!((area - 480_000.0).abs() < 1.0),
        other => panic!("{other:?}"),
    }
    let _ = scene;
}

#[test]
fn a_window_takes_the_objects_inside_and_a_crossing_the_ones_it_touches() {
    let (ctx, mut app, mut t, site) = ready_site();
    choose(&ctx, &mut app, egui::Key::Space, &mut t);
    let none = egui::Modifiers::NONE;
    let corner = |app: &mut ModelerApp, id, fx, fy| plan_point(app, id, fx, fy);
    // Left to right around the slab only.
    let rect = |app: &mut ModelerApp, id, pad: f32| {
        let (a, b) = (corner(app, id, 0.0, 0.0), corner(app, id, 1.0, 1.0));
        (
            a.min(b) - egui::vec2(pad, pad),
            a.max(b) + egui::vec2(pad, pad),
        )
    };
    let (lo, hi) = rect(&mut app, site.slab, 12.0);
    t = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        lo,
        hi,
        none,
        none,
        t,
    );
    assert_eq!(app.session.selection, vec![site.slab], "window");
    // A window that cuts the plant does not take it.
    let (lo, hi) = rect(&mut app, site.plant, -10.0);
    t = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        lo,
        hi,
        none,
        none,
        t,
    );
    assert!(
        !app.session.selection.contains(&site.plant),
        "{:?}",
        app.session.selection
    );
    // Right to left over the middle of the plant and the slab: a crossing touches both.
    let p = corner(&mut app, site.plant, 0.5, 0.5);
    let s = corner(&mut app, site.slab, 0.5, 0.5);
    t = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        egui::pos2(s.x + 8.0, s.y.max(p.y) + 8.0),
        egui::pos2(p.x - 8.0, s.y.min(p.y) - 8.0),
        none,
        none,
        t,
    );
    let sel = &app.session.selection;
    assert!(
        sel.contains(&site.plant) && sel.contains(&site.slab),
        "{sel:?}"
    );
    // A window around everything takes all three.
    let a = egui::pos2(
        app.canvas_rect.unwrap().min.x + 3.0,
        app.canvas_rect.unwrap().min.y + 3.0,
    );
    let b = egui::pos2(
        app.canvas_rect.unwrap().max.x - 3.0,
        app.canvas_rect.unwrap().max.y - 3.0,
    );
    let _ = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        a,
        b,
        none,
        none,
        t,
    );
    let mut sel = app.session.selection.clone();
    sel.sort();
    let mut all = vec![site.plant, site.slab, site.terrain];
    all.sort();
    assert_eq!(sel, all);
}

#[test]
fn a_plant_moves_with_a_typed_distance_in_one_undo_step() {
    let (ctx, mut app, mut t, site) = ready_site();
    let at = plan_point(&mut app, site.plant, 0.5, 0.5);
    click_at(&ctx, &mut app, at, &mut t);
    assert_eq!(app.session.selection, vec![site.plant]);
    let depth = app.session.editor.undo_depth();
    choose(&ctx, &mut app, egui::Key::M, &mut t);
    click_at(&ctx, &mut app, at, &mut t);
    let _ = frame(&ctx, &mut app, vec![moved(at + egui::vec2(40.0, 3.0))], t);
    t += 0.1;
    let _ = frame(
        &ctx,
        &mut app,
        tap(egui::Key::ArrowRight, egui::Modifiers::NONE),
        t,
    );
    type_keys(&ctx, &mut app, "1000", &mut t);
    enter(&ctx, &mut app, &mut t);
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    match &app.session.editor.doc().object(site.plant).unwrap().shape {
        ModelShape::Plant { at, .. } => {
            assert!(
                (at.x - (-200.0)).abs() < 0.5 && (at.y - 300.0).abs() < 0.5,
                "{at:?}"
            );
        }
        other => panic!("{}", other.kind()),
    }
    app.run(Cmd::Undo, size(&app));
    match &app.session.editor.doc().object(site.plant).unwrap().shape {
        ModelShape::Plant { at, .. } => assert!((at.x + 1200.0).abs() < 1e-9),
        _ => unreachable!(),
    }
}

#[test]
fn a_plant_turns_a_hardscape_refuses_the_turn_and_a_site_model_scales_in_the_plan() {
    let (ctx, mut app, mut t, site) = ready_site();
    // Rotate the plant 90 degrees about its own middle: it stays and its turn changes.
    app.session.selection = vec![site.plant];
    let mid = plan_point(&mut app, site.plant, 0.5, 0.5);
    let depth = app.session.editor.undo_depth();
    choose(&ctx, &mut app, egui::Key::Q, &mut t);
    let _ = frame(&ctx, &mut app, vec![moved(mid)], t);
    click_at(&ctx, &mut app, mid, &mut t);
    click_at(&ctx, &mut app, mid + egui::vec2(60.0, 0.0), &mut t);
    type_keys(&ctx, &mut app, "90", &mut t);
    enter(&ctx, &mut app, &mut t);
    assert_eq!(app.session.editor.undo_depth(), depth + 1, "{}", app.status);
    let doc = app.session.editor.doc();
    let turn = kadakas_core::plants::link::rotation(doc.object(site.plant).unwrap());
    assert!((turn.to_degrees() - 90.0).abs() < 0.01, "{turn}");
    // A hardscape cannot be turned (the core says so); the status line tells the person.
    app.session.selection = vec![site.slab];
    let before = app
        .session
        .editor
        .doc()
        .object(site.slab)
        .unwrap()
        .shape
        .clone();
    let depth = app.session.editor.undo_depth();
    let mid = plan_point(&mut app, site.slab, 0.5, 0.5);
    let _ = frame(&ctx, &mut app, vec![moved(mid)], t);
    click_at(&ctx, &mut app, mid, &mut t);
    click_at(&ctx, &mut app, mid + egui::vec2(60.0, 0.0), &mut t);
    type_keys(&ctx, &mut app, "90", &mut t);
    enter(&ctx, &mut app, &mut t);
    let _ = frame(&ctx, &mut app, vec![], t + 0.1);
    assert!(app.status.contains("cannot be turned"), "{}", app.status);
    assert_eq!(app.session.editor.undo_depth(), depth);
    assert_eq!(
        app.session.editor.doc().object(site.slab).unwrap().shape,
        before
    );
    // A site model moves but cannot be scaled (the core refuses, the status line says so).
    app.session.selection = vec![site.terrain];
    let (slo, shi) = scene_box(&mut app, site.terrain);
    choose(&ctx, &mut app, egui::Key::Escape, &mut t);
    choose(&ctx, &mut app, egui::Key::S, &mut t);
    let corner = screen_of(&mut app, DVec3::new(shi.x, shi.y, shi.z));
    let _ = frame(&ctx, &mut app, vec![moved(corner)], t);
    click_at(&ctx, &mut app, corner, &mut t);
    type_keys(&ctx, &mut app, "2", &mut t);
    enter(&ctx, &mut app, &mut t);
    let _ = frame(&ctx, &mut app, vec![], t + 0.1);
    assert!(app.status.contains("cannot be scaled"), "{}", app.status);
    let (nlo, nhi) = scene_box(&mut app, site.terrain);
    assert!(nlo.distance(slo) < 1e-3 && nhi.distance(shi) < 1e-3);
    // A plant scales from a corner grip: the spread doubles and the height stays (the core keeps
    // it and says so).
    choose(&ctx, &mut app, egui::Key::Escape, &mut t);
    app.session.selection = vec![site.plant];
    let (plo, phi) = scene_box(&mut app, site.plant);
    choose(&ctx, &mut app, egui::Key::S, &mut t);
    let corner = screen_of(&mut app, DVec3::new(phi.x, phi.y, phi.z));
    let _ = frame(&ctx, &mut app, vec![moved(corner)], t);
    click_at(&ctx, &mut app, corner, &mut t);
    assert!(app.edit.modeling.in_progress(), "the grip was picked");
    type_keys(&ctx, &mut app, "2", &mut t);
    enter(&ctx, &mut app, &mut t);
    let _ = frame(&ctx, &mut app, vec![], t + 0.1);
    match &app.session.editor.doc().object(site.plant).unwrap().shape {
        ModelShape::Plant { spread, height, .. } => {
            assert!(
                (*spread - 2400.0).abs() < 1.0 && (*height - 3000.0).abs() < 1.0,
                "{spread} {height} {plo:?} {phi:?} {}",
                app.status
            );
        }
        other => panic!("{}", other.kind()),
    }
}

#[test]
fn open_in_the_kadakas_app_saves_first_and_starts_the_app_with_the_file() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let dir = temp("open-in-app");
    let path = dir.join("site.kad");
    let mut app = app();
    let started: Rc<RefCell<Vec<std::path::PathBuf>>> = Rc::default();
    let log = started.clone();
    app.starter = Box::new(move |p| {
        log.borrow_mut().push(p.to_path_buf());
        Ok(())
    });
    landscape(&mut app);
    app.path = Some(path.clone());
    assert!(app.dirty());
    // With unsaved changes the person is asked, and nothing starts yet.
    app.tray_action(TrayAction::OpenInApp);
    assert_eq!(app.pending, Some(Replace::OpenInApp));
    assert!(started.borrow().is_empty());
    let ctx = egui::Context::default();
    let shown = texts(&frame(&ctx, &mut app, vec![], 0.0)).join("\n");
    assert!(
        shown.contains(&tr!("modeler-unsaved-save-and-open-in-app")),
        "{shown}"
    );
    // Cancel starts nothing.
    app.answer(Answer::Cancel);
    assert!(started.borrow().is_empty() && app.dirty());
    // Save and open: the file is on disk, and then the app starts with it.
    app.tray_action(TrayAction::OpenInApp);
    app.answer(Answer::SaveAndGo);
    assert!(path.is_file());
    assert_eq!(*started.borrow(), vec![path.clone()]);
    assert!(!app.dirty());
    assert!(app.status.contains("site.kad"), "{}", app.status);
    // Without changes it starts at once; the menu command does the same.
    app.run(Cmd::OpenInApp, size_of_window());
    assert_eq!(started.borrow().len(), 2);
    // A start that fails says why.
    app.starter = Box::new(|_| Err("no app here".into()));
    app.run(Cmd::OpenInApp, size_of_window());
    assert_eq!(app.status, "no app here");
    let _ = std::fs::remove_dir_all(&dir);
}

fn size_of_window() -> glam::Vec2 {
    glam::Vec2::new(900.0, 600.0)
}

#[test]
fn the_entity_info_texts_of_the_landscape_exist_in_both_languages() {
    for key in [
        "modeler-entity-species",
        "modeler-entity-style",
        "modeler-entity-height",
        "modeler-entity-spread",
        "modeler-entity-trunk",
        "modeler-entity-area-total",
        "modeler-entity-survey-points",
        "modeler-entity-extent",
        "modeler-entity-open-in-app",
        "modeler-entity-needs-app",
    ] {
        let en = kadakas_l10n::with_language("en-US", || kadakas_l10n::translate(key, &[]));
        let et = kadakas_l10n::with_language("et", || kadakas_l10n::translate(key, &[]));
        assert_ne!(en, et, "{key}");
        assert!(!en.is_empty() && !et.is_empty());
    }
    let _ = type_more;
}
