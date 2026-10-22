//! Tests of the Paint Bucket and the Materials panel in the window (MOD-05, round B): the real
//! input path (the key B, clicks with Ctrl, Shift and Alt, Esc), the attributes of the faces, the
//! picture that the renderer makes from the document, and the tray.

use super::transform::{make_box, ready, screen_of};
use super::*;
use glam::DVec3;
use kadakas_core::model::ObjectId;
use kadakas_core::resource::ResourceId;
use kadakas_core::surface::{ATTR_BACK_MATERIAL, ATTR_MATERIAL, Edit, FaceId};
use kadakas_ui_core::materials::Request;

use crate::tray::TrayAction;

fn material(app: &ModelerApp, name: &str) -> ResourceId {
    kadakas_ui_core::materials::rows(app.session.editor.doc())
        .into_iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no material {name}"))
        .id
}

/// The built-in materials are in the model and the bucket holds `name`.
fn load_bucket(app: &mut ModelerApp, name: &str) -> ResourceId {
    if kadakas_ui_core::materials::rows(app.session.editor.doc()).is_empty() {
        app.tray_action(TrayAction::Material(Request::AddDefaults));
    }
    let id = material(app, name);
    app.tray_action(TrayAction::Material(Request::Select(Some(id))));
    id
}

/// The face of the box that points up (the top).
fn top_face(app: &ModelerApp) -> FaceId {
    surface(app)
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.9)
        .expect("a top face")
        .0
}

fn attr(app: &ModelerApp, face: FaceId, key: &str) -> Option<String> {
    surface(app)
        .face(face)?
        .attrs
        .get(key)?
        .as_str()
        .map(str::to_string)
}

fn painted(app: &ModelerApp, key: &str, m: ResourceId) -> usize {
    surface(app)
        .faces()
        .filter(|(_, f)| f.attrs.get(key).and_then(|v| v.as_str()) == Some(&m.to_string()))
        .count()
}

/// A click of the primary button with keys held.
fn click_with(
    ctx: &egui::Context,
    app: &mut ModelerApp,
    p: egui::Pos2,
    mods: egui::Modifiers,
    t: &mut f64,
) {
    let b = egui::PointerButton::Primary;
    let _ = frame_with(ctx, app, vec![moved(p)], *t, mods);
    *t += 0.05;
    let _ = frame_with(ctx, app, vec![button(p, b, true, mods)], *t, mods);
    *t += 0.05;
    let _ = frame_with(ctx, app, vec![button(p, b, false, mods)], *t, mods);
    *t += 0.05;
    let _ = frame(ctx, app, vec![], *t);
    *t += 0.2;
}

/// The picture of the document from the camera of the window, as the exports make it.
fn picture(app: &mut ModelerApp) -> kadakas_render::Image {
    let scene = app.session.scene();
    let look = kadakas_render::look::Look::build(app.session.editor.doc(), &scene, true);
    kadakas_render::shade::render_scene(
        &scene,
        &look,
        &app.nav.camera,
        &kadakas_render::shade::ShadeOptions {
            width: 80,
            height: 80,
            edges: false,
            ..Default::default()
        },
    )
}

#[test]
fn the_key_b_chooses_the_paint_bucket() {
    let ctx = egui::Context::default();
    let mut app = app();
    let mut t = 0.0;
    choose(&ctx, &mut app, egui::Key::B, &mut t);
    assert_eq!(app.tools.active(), ModelerTool::PaintBucket);
    let hint = app.tools.active().hint();
    assert!(hint.contains("Alt") && hint.contains("Ctrl") && hint.contains("Shift"));
    choose(&ctx, &mut app, egui::Key::Space, &mut t);
    assert_eq!(app.tools.active(), ModelerTool::Select);
}

#[test]
fn a_box_is_painted_brick_on_the_top_and_the_picture_shows_brick() {
    let (ctx, mut app, mut t, _) = ready();
    let brick = load_bucket(&mut app, "Brick");
    let before = picture(&mut app).pixel(40, 40);
    let depth = app.session.editor.undo_depth();
    choose(&ctx, &mut app, egui::Key::B, &mut t);
    let c = center(&app);
    click_at(&ctx, &mut app, c, &mut t);
    let top = top_face(&app);
    assert_eq!(attr(&app, top, ATTR_MATERIAL), Some(brick.to_string()));
    assert_eq!(attr(&app, top, ATTR_BACK_MATERIAL), None);
    assert_eq!(painted(&app, ATTR_MATERIAL, brick), 1, "only that face");
    assert_eq!(app.session.editor.undo_depth(), depth + 1, "one undo step");
    assert!(app.status.contains("Brick"), "{}", app.status);
    // The picture: the top is brick red now, and was a light gray before.
    let after = picture(&mut app).pixel(40, 40);
    assert!(
        after[0] > after[2] + 25 && after[0] > before[0].saturating_sub(40) / 2,
        "brick {after:?} (was {before:?})"
    );
    assert!(
        before[0].abs_diff(before[2]) < 25,
        "gray before: {before:?}"
    );
    // Undo takes it back.
    app.run(Cmd::Undo, size(&app));
    assert_eq!(attr(&app, top, ATTR_MATERIAL), None);
}

#[test]
fn ctrl_shift_and_alt_choose_the_fill_and_sample() {
    let (ctx, mut app, mut t, _) = ready();
    let grass = load_bucket(&mut app, "Grass");
    let brick = material(&app, "Brick");
    let c = center(&app);
    choose(&ctx, &mut app, egui::Key::B, &mut t);
    // Ctrl: every face of the box is connected and has no material yet.
    let depth = app.session.editor.undo_depth();
    click_with(&ctx, &mut app, c, egui::Modifiers::CTRL, &mut t);
    assert_eq!(
        painted(&app, ATTR_MATERIAL, grass),
        6,
        "all connected faces"
    );
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    // Shift: all faces with grass become brick.
    app.tray_action(TrayAction::Material(Request::Select(Some(brick))));
    click_with(&ctx, &mut app, c, egui::Modifiers::SHIFT, &mut t);
    assert_eq!(painted(&app, ATTR_MATERIAL, brick), 6);
    assert_eq!(painted(&app, ATTR_MATERIAL, grass), 0);
    // A plain click paints one face back to grass.
    app.tray_action(TrayAction::Material(Request::Select(Some(grass))));
    click_at(&ctx, &mut app, c, &mut t);
    assert_eq!(painted(&app, ATTR_MATERIAL, grass), 1);
    // Alt samples the material under the pointer (brick is not on the top: grass is) and
    // paints nothing.
    app.tray_action(TrayAction::Material(Request::Select(None)));
    let depth = app.session.editor.undo_depth();
    click_with(&ctx, &mut app, c, egui::Modifiers::ALT, &mut t);
    assert_eq!(app.edit.modeling.paint.bucket, Some(grass));
    assert_eq!(app.session.editor.undo_depth(), depth);
    // Esc leaves the tool; the bucket keeps its material.
    let _ = frame(
        &ctx,
        &mut app,
        tap(egui::Key::Escape, egui::Modifiers::NONE),
        t,
    );
    assert_eq!(app.tools.active(), ModelerTool::Select);
    assert_eq!(app.edit.modeling.paint.bucket, Some(grass));
}

#[test]
fn the_back_of_a_turned_face_takes_the_back_material_and_the_picture_shows_it() {
    let (ctx, mut app, mut t, id) = ready();
    let top = top_face(&app);
    // Turn the top over: from above the viewer sees its back.
    app.session
        .editor
        .execute(&Command::EditSurface {
            id,
            edit: Edit::ReverseFaces { faces: vec![top] },
        })
        .unwrap();
    let grass = load_bucket(&mut app, "Grass");
    choose(&ctx, &mut app, egui::Key::B, &mut t);
    let c = center(&app);
    click_at(&ctx, &mut app, c, &mut t);
    assert_eq!(attr(&app, top, ATTR_BACK_MATERIAL), Some(grass.to_string()));
    assert_eq!(
        attr(&app, top, ATTR_MATERIAL),
        None,
        "the front is not painted"
    );
    let p = picture(&mut app).pixel(40, 40);
    assert!(p[1] > p[0] + 20 && p[1] > p[2] + 20, "grass green: {p:?}");
}

#[test]
fn selected_objects_are_painted_as_a_whole_and_plants_and_hardscapes_are_refused() {
    let (ctx, mut app, mut t, id) = ready();
    let brick = load_bucket(&mut app, "Brick");
    // A hardscape next to the box.
    let slab = app
        .session
        .editor
        .execute(&Command::AddObject {
            shape: ModelShape::Hardscape {
                boundary: kadakas_core::curve::corners(&[
                    DVec2::new(700.0, -300.0),
                    DVec2::new(1500.0, -300.0),
                    DVec2::new(1500.0, 300.0),
                    DVec2::new(700.0, 300.0),
                ]),
                material: "m".into(),
                thickness: Some(100.0),
                elevation: None,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .map(|o| ObjectId(o.created[0]))
        .unwrap();
    choose(&ctx, &mut app, egui::Key::B, &mut t);
    // The box is selected as a whole: a click on it paints the object.
    app.session.selection = vec![id];
    let c = center(&app);
    click_at(&ctx, &mut app, c, &mut t);
    assert_eq!(
        kadakas_core::present::object_material(app.session.editor.doc(), id),
        Some(brick)
    );
    assert_eq!(
        painted(&app, ATTR_MATERIAL, brick),
        0,
        "no face was painted"
    );
    // A click on the hardscape is refused with the reason and changes nothing.
    app.session.selection.clear();
    let depth = app.session.editor.undo_depth();
    let on_slab = screen_of(&mut app, DVec3::new(1100.0, 0.0, 0.0));
    click_at(&ctx, &mut app, on_slab, &mut t);
    let _ = frame(&ctx, &mut app, vec![], t);
    assert_eq!(app.status, tr!("uic-paint-refused-style"));
    assert_eq!(app.session.editor.undo_depth(), depth);
    assert_eq!(
        kadakas_core::present::object_material(app.session.editor.doc(), slab),
        None
    );
}

#[test]
fn the_materials_panel_lists_adds_edits_and_asks_before_deleting_a_used_material() {
    let (ctx, mut app, mut t) = top_view();
    make_box(&mut app);
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    assert!(
        shown.contains(&tr!("modeler-materials-add-defaults")),
        "{shown}"
    );
    assert!(shown.contains(&tr!("modeler-materials-empty")), "{shown}");
    // The button adds the built-in materials; the list shows them.
    app.tray_action(TrayAction::Material(Request::AddDefaults));
    t += 0.1;
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    for name in [
        "Grass",
        "Gravel",
        "Concrete pavers",
        "Bark mulch",
        "Water",
        "Brick",
        "Timber",
    ] {
        assert!(shown.contains(name), "{name}: {shown}");
    }
    // A new material opens the editor.
    app.tray_action(TrayAction::Material(Request::New));
    t += 0.1;
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    assert!(
        shown.contains(&tr!("modeler-materials-roughness")),
        "{shown}"
    );
    assert!(app.materials.editing.is_some());
    // A used material asks first.
    let brick = material(&app, "Brick");
    let top = top_face(&app);
    let model = *app.session.editor.doc().objects.keys().next().unwrap();
    app.tray_action(TrayAction::Material(Request::SetFaces {
        model,
        faces: vec![top],
        side: kadakas_core::surface::PaintSide::Front,
        material: Some(brick),
    }));
    app.tray_action(TrayAction::Material(Request::Delete(brick)));
    assert!(app.materials.pending_delete.is_some());
    assert!(app.session.editor.doc().resources.get(brick).is_some());
    t += 0.1;
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    assert!(
        shown.contains(&tr!("modeler-materials-delete-yes")),
        "{shown}"
    );
    app.tray_action(TrayAction::Material(Request::ConfirmDelete));
    assert!(app.session.editor.doc().resources.get(brick).is_none());
    assert_eq!(
        attr(&app, top, ATTR_MATERIAL),
        None,
        "the face is back to default"
    );
}

#[test]
fn entity_info_of_a_face_shows_its_materials_and_the_tray_changes_them() {
    let (ctx, mut app, mut t, id) = ready();
    app.run(Cmd::ToggleTray, size(&app));
    let brick = load_bucket(&mut app, "Brick");
    let top = top_face(&app);
    app.edit
        .sub
        .set([kadakas_ui_core::modeling::Item::Face(id, top)]);
    t += 0.1;
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    assert!(
        shown.contains(&tr!("modeler-entity-front-material")),
        "{shown}"
    );
    assert!(
        shown.contains(&tr!("modeler-entity-back-material")),
        "{shown}"
    );
    app.tray_action(TrayAction::Material(Request::SetFaces {
        model: id,
        faces: vec![top],
        side: kadakas_core::surface::PaintSide::Back,
        material: Some(brick),
    }));
    t += 0.1;
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    assert!(shown.contains("Brick"), "{shown}");
    assert_eq!(attr(&app, top, ATTR_BACK_MATERIAL), Some(brick.to_string()));
}

#[test]
fn the_materials_panel_has_no_english_in_estonian() {
    kadakas_l10n::with_language("et", || {
        let ctx = egui::Context::default();
        let mut app = app_with(Prefs {
            language: Some("et".into()),
            ..Prefs::default()
        });
        app.tray_action(TrayAction::Material(Request::AddDefaults));
        app.tray_action(TrayAction::Material(Request::New));
        let shown = texts(&frame(&ctx, &mut app, vec![], 0.0)).join("\n");
        for english in [
            "Purge unused",
            "Add built-in",
            "Roughness",
            "Metallic",
            "Mapping",
        ] {
            assert!(!shown.contains(english), "{english} in: {shown}");
        }
        assert!(shown.contains("Karedus"), "{shown}");
    });
}
