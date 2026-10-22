//! Tests of the Tape Measure, guides, Hide and Tags in the window (MOD-05): the real input path
//! (keys, pointer events and typed text) through headless frames, and the result in the document.

use super::transform::{make_box, ready, screen_of, toggle_copy, type_more};
use super::*;
use glam::DVec3;
use kadakas_core::guide::Guide;
use kadakas_core::model::{ClassId, ObjectId};
use kadakas_core::surface::FaceId;
use kadakas_ui_core::modeling::{Item, ShowOptions};
use kadakas_ui_core::tags;

use crate::app::Confirm;
use crate::tags_panel;

/// The same view from a corner above: no two vertices of the box share a pixel.
fn iso(ctx: &egui::Context, app: &mut ModelerApp, t: &mut f64) {
    app.nav.camera.set_view(StandardView::RightIsometric);
    for _ in 0..3 {
        *t += 0.1;
        let _ = frame(ctx, app, vec![], *t);
    }
}

fn guides(app: &ModelerApp) -> Vec<Guide> {
    app.session
        .editor
        .doc()
        .guides()
        .map(|(_, g)| g.clone())
        .collect()
}

fn add_guide(app: &mut ModelerApp, g: Guide) -> ObjectId {
    ObjectId(
        app.session
            .editor
            .execute(&Command::AddObject {
                shape: ModelShape::Guide(Box::new(g)),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
    )
}

fn shown(ctx: &egui::Context, app: &mut ModelerApp, t: &mut f64) -> String {
    *t += 0.1;
    texts(&frame(ctx, app, vec![], *t)).join("\n")
}

fn top_face(app: &ModelerApp, id: ObjectId) -> FaceId {
    match &app.session.editor.doc().object(id).unwrap().shape {
        ModelShape::SurfaceModel(m) => m
            .faces()
            .filter(|(_, f)| f.plane.normal.z > 0.9)
            .max_by(|a, b| a.1.plane.offset.total_cmp(&b.1.plane.offset))
            .map(|(f, _)| f)
            .unwrap(),
        _ => panic!("not a model"),
    }
}

fn triangles(app: &mut ModelerApp) -> usize {
    app.session.scene().mesh.triangle_count()
}

#[test]
fn the_key_t_chooses_the_tape_measure_and_it_runs() {
    let ctx = egui::Context::default();
    let mut app = app();
    let mut t = 0.0;
    choose(&ctx, &mut app, egui::Key::T, &mut t);
    assert_eq!(app.tools.active(), ModelerTool::TapeMeasure);
    assert!(app.status.is_empty(), "{}", app.status);
    assert!(app.edit.modeling.tape_creates_guides());
}

#[test]
fn measuring_a_box_edge_shows_its_length_in_the_box_and_the_status_line() {
    let (ctx, mut app, mut t, _) = ready();
    iso(&ctx, &mut app, &mut t);
    choose(&ctx, &mut app, egui::Key::T, &mut t);
    // Ctrl alone: measure only.
    toggle_copy(&ctx, &mut app, &mut t);
    assert!(!app.edit.modeling.tape_creates_guides());
    let _ = shown(&ctx, &mut app, &mut t);
    assert_eq!(app.status, tr!("modeler-tape-measure-only"));
    toggle_copy(&ctx, &mut app, &mut t);
    assert!(app.edit.modeling.tape_creates_guides());
    let _ = shown(&ctx, &mut app, &mut t);
    assert_eq!(app.status, tr!("modeler-tape-guides"));
    toggle_copy(&ctx, &mut app, &mut t);
    assert!(!app.edit.modeling.tape_creates_guides());
    let depth = app.session.editor.undo_depth();
    let a = screen_of(&mut app, DVec3::new(-500.0, -500.0, 500.0));
    let b = screen_of(&mut app, DVec3::new(500.0, -500.0, 500.0));
    click_at(&ctx, &mut app, a, &mut t);
    // The distance shows live while the pointer moves.
    let _ = frame(&ctx, &mut app, vec![moved(b)], t);
    let on_screen = shown(&ctx, &mut app, &mut t);
    assert!(on_screen.contains("1000"), "{on_screen}");
    assert!(
        on_screen.contains(&tr!("uic-modeler-measure-distance")),
        "{on_screen}"
    );
    let status = tr!("modeler-tape-distance", value = "");
    assert!(
        on_screen.contains(status.trim_end()),
        "the status line has the distance too: {on_screen}"
    );
    click_at(&ctx, &mut app, b, &mut t);
    assert_eq!(
        app.session.editor.undo_depth(),
        depth,
        "a measure changes nothing"
    );
    assert!(guides(&app).is_empty());
    // The value stays in the box after the second click.
    let after = shown(&ctx, &mut app, &mut t);
    assert!(after.contains("1000"), "{after}");
}

#[test]
fn a_click_on_an_edge_and_one_away_make_a_parallel_guide_that_the_inference_snaps_to() {
    let (ctx, mut app, mut t, _) = ready();
    iso(&ctx, &mut app, &mut t);
    choose(&ctx, &mut app, egui::Key::T, &mut t);
    let depth = app.session.editor.undo_depth();
    let on_edge = screen_of(&mut app, DVec3::new(-200.0, -500.0, 500.0));
    let away = screen_of(&mut app, DVec3::new(-500.0, 500.0, 500.0));
    click_at(&ctx, &mut app, on_edge, &mut t);
    assert!(app.edit.modeling.in_progress());
    // The offset shows while the pointer moves: the distance of the pointer from the edge.
    let _ = frame(&ctx, &mut app, vec![moved(away)], t);
    let live = shown(&ctx, &mut app, &mut t);
    assert!(live.contains("1000"), "{live}");
    click_at(&ctx, &mut app, away, &mut t);
    assert_eq!(app.session.editor.undo_depth(), depth + 1, "one undo step");
    let g = guides(&app);
    assert_eq!(g.len(), 1);
    assert!(
        g[0].at.distance(DVec3::new(-500.0, 500.0, 500.0)) < 1e-6,
        "{:?}",
        g[0].at
    );
    assert!(
        g[0].unit_dir().unwrap().dot(DVec3::X).abs() > 0.999999,
        "parallel to the edge"
    );
    assert_eq!(g[0].length, None);
    // The Line tool snaps to the guide: "On Guide" in the label of the inference.
    choose(&ctx, &mut app, egui::Key::L, &mut t);
    let over = screen_of(&mut app, DVec3::new(1500.0, 500.0, 500.0)) + egui::vec2(0.0, 2.0);
    let _ = frame(&ctx, &mut app, vec![moved(over)], t);
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![moved(over)], t);
    let inf = app
        .edit
        .hover_inference()
        .expect("an inference under the pointer");
    assert_eq!(inf.label, tr!("uic-infer3d-on-guide"), "{inf:?}");
    assert!((inf.point.y - 500.0).abs() < 1e-6 && (inf.point.z - 500.0).abs() < 1e-6);
    // Undo takes the guide back.
    assert!(app.session.editor.undo());
    assert!(guides(&app).is_empty());
}

#[test]
fn a_click_on_a_corner_and_one_in_a_direction_make_a_guide_line_through_the_corner() {
    let (ctx, mut app, mut t, _) = ready();
    iso(&ctx, &mut app, &mut t);
    choose(&ctx, &mut app, egui::Key::T, &mut t);
    let corner = screen_of(&mut app, DVec3::new(500.0, -500.0, 500.0));
    let toward = screen_of(&mut app, DVec3::new(500.0, 500.0, 500.0));
    click_at(&ctx, &mut app, corner, &mut t);
    click_at(&ctx, &mut app, toward, &mut t);
    let g = guides(&app);
    assert_eq!(g.len(), 1);
    assert!(
        g[0].at.distance(DVec3::new(500.0, -500.0, 500.0)) < 1e-6,
        "{:?}",
        g[0].at
    );
    assert!(g[0].unit_dir().unwrap().dot(DVec3::Y).abs() > 0.999999);
    assert_eq!(g[0].length, None);
}

#[test]
fn a_typed_distance_sets_the_offset_and_escape_cancels() {
    let (ctx, mut app, mut t, _) = ready();
    iso(&ctx, &mut app, &mut t);
    choose(&ctx, &mut app, egui::Key::T, &mut t);
    let depth = app.session.editor.undo_depth();
    let on_edge = screen_of(&mut app, DVec3::new(-200.0, -500.0, 500.0));
    // The pointer is on the side of +y, where the other corner is.
    let side = screen_of(&mut app, DVec3::new(-500.0, 500.0, 500.0));
    click_at(&ctx, &mut app, on_edge, &mut t);
    let _ = frame(&ctx, &mut app, vec![moved(side)], t);
    type_keys(&ctx, &mut app, "250", &mut t);
    assert_eq!(app.edit.modeling.typed(), "250");
    enter(&ctx, &mut app, &mut t);
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    let g = guides(&app);
    assert_eq!(g.len(), 1);
    assert!(
        g[0].at.distance(DVec3::new(-200.0, -250.0, 500.0)) < 0.1,
        "{:?}",
        g[0].at
    );
    assert!(g[0].unit_dir().unwrap().dot(DVec3::X).abs() > 0.999999);
    // Escape: the second click does not come.
    click_at(&ctx, &mut app, on_edge, &mut t);
    assert!(app.edit.modeling.in_progress());
    let _ = frame(
        &ctx,
        &mut app,
        tap(egui::Key::Escape, egui::Modifiers::NONE),
        t + 0.1,
    );
    assert!(!app.edit.modeling.in_progress());
    assert_eq!(guides(&app).len(), 1);
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
}

#[test]
fn a_measure_and_a_typed_length_resize_the_model_after_a_yes() {
    let (ctx, mut app, mut t, id) = ready();
    iso(&ctx, &mut app, &mut t);
    choose(&ctx, &mut app, egui::Key::T, &mut t);
    toggle_copy(&ctx, &mut app, &mut t);
    let a = screen_of(&mut app, DVec3::new(-500.0, -500.0, 500.0));
    let b = screen_of(&mut app, DVec3::new(500.0, -500.0, 500.0));
    click_at(&ctx, &mut app, a, &mut t);
    click_at(&ctx, &mut app, b, &mut t);
    let before = super::transform::bounds(&app, id);
    let depth = app.session.editor.undo_depth();
    type_keys(&ctx, &mut app, "2000", &mut t);
    enter(&ctx, &mut app, &mut t);
    // The question is open; nothing changed yet.
    assert!(
        matches!(app.confirm, Some(Confirm::Resize(r)) if (r.factor - 2.0).abs() < 1e-9),
        "{:?}",
        app.confirm
    );
    assert_eq!(super::transform::bounds(&app, id), before);
    let text = shown(&ctx, &mut app, &mut t);
    assert!(text.contains(&tr!("modeler-resize-title")), "{text}");
    // No: nothing changes.
    app.answer_confirm(false);
    assert_eq!(super::transform::bounds(&app, id), before);
    assert_eq!(app.session.editor.undo_depth(), depth);
    // Yes: the box is twice as large about the first point, in one undo step.
    type_keys(&ctx, &mut app, "2000", &mut t);
    enter(&ctx, &mut app, &mut t);
    app.answer_confirm(true);
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    let (lo, hi) = super::transform::bounds(&app, id);
    assert!(((hi - lo).x - 2000.0).abs() < 1.0, "{lo:?} {hi:?}");
    assert!(((hi - lo).y - 2000.0).abs() < 1.0);
    assert!(app.status.contains("2.0000"), "{}", app.status);
    assert!(app.session.editor.undo());
    assert_eq!(super::transform::bounds(&app, id), before);
}

#[test]
fn the_eraser_removes_a_guide_in_one_step() {
    let (ctx, mut app, mut t, _) = ready();
    add_guide(&mut app, Guide::line(DVec3::new(0.0, 800.0, 0.0), DVec3::X));
    choose(&ctx, &mut app, egui::Key::E, &mut t);
    let depth = app.session.editor.undo_depth();
    let over = screen_of(&mut app, DVec3::new(800.0, 800.0, 0.0));
    click_at(&ctx, &mut app, over, &mut t);
    assert!(guides(&app).is_empty());
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    assert!(app.session.editor.undo());
    assert_eq!(guides(&app).len(), 1);
}

#[test]
fn select_takes_a_guide_by_a_click_and_by_a_window() {
    let (ctx, mut app, mut t, _) = ready();
    let g = add_guide(&mut app, Guide::line(DVec3::new(0.0, 800.0, 0.0), DVec3::X));
    let p = add_guide(&mut app, Guide::point(DVec3::new(-900.0, -800.0, 0.0)));
    choose(&ctx, &mut app, egui::Key::Space, &mut t);
    let on_line = screen_of(&mut app, DVec3::new(900.0, 800.0, 0.0));
    click_at(&ctx, &mut app, on_line, &mut t);
    assert_eq!(app.session.selection, vec![g]);
    // A crossing window (right to left) takes the line without ends; a window would not.
    let none = egui::Modifiers::NONE;
    let a = screen_of(&mut app, DVec3::new(-300.0, 650.0, 0.0));
    let b = screen_of(&mut app, DVec3::new(300.0, 950.0, 0.0));
    app.session.selection.clear();
    let t2 = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        b,
        a,
        none,
        none,
        t,
    );
    assert_eq!(app.session.selection, vec![g]);
    app.session.selection.clear();
    let t3 = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        a,
        b,
        none,
        none,
        t2,
    );
    assert!(
        app.session.selection.is_empty(),
        "a window does not hold a line without ends"
    );
    // A window around the guide point takes it.
    let a = screen_of(&mut app, DVec3::new(-1100.0, -1000.0, 0.0));
    let b = screen_of(&mut app, DVec3::new(-700.0, -600.0, 0.0));
    let _ = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        a,
        b,
        none,
        none,
        t3,
    );
    assert_eq!(app.session.selection, vec![p]);
    // With View > Guides off, Select All takes no guide and the guides are not offered.
    app.run(Cmd::ToggleGuides, size(&app));
    assert!(!app.edit.cache.show.guides);
    app.run(Cmd::SelectAll, size(&app));
    assert!(!app.session.selection.contains(&g) && !app.session.selection.contains(&p));
    app.run(Cmd::ToggleGuides, size(&app));
    app.run(Cmd::SelectAll, size(&app));
    assert!(app.session.selection.contains(&g) && app.session.selection.contains(&p));
}

#[test]
fn view_guides_off_hides_the_guides_from_the_inference() {
    let (ctx, mut app, mut t, _) = ready();
    add_guide(&mut app, Guide::point(DVec3::new(1200.0, 1200.0, 0.0)));
    let _ = shown(&ctx, &mut app, &mut t);
    let rev = app.session.editor.revision();
    let have = |app: &mut ModelerApp| {
        let rev = app.session.editor.revision();
        let doc = app.session.editor.doc().clone();
        !app.edit.cache.get(&doc, rev).guide_ids().is_empty()
    };
    let _ = rev;
    assert!(have(&mut app));
    app.run(Cmd::ToggleGuides, size(&app));
    assert!(!have(&mut app));
    app.run(Cmd::ToggleGuides, size(&app));
    assert!(have(&mut app));
}

#[test]
fn delete_guides_asks_when_there_are_more_than_a_few_and_is_one_undo_step() {
    let (_ctx, mut app, _t, _) = ready();
    // Nothing to delete.
    app.run(Cmd::DeleteGuides, size(&app));
    assert_eq!(app.status, tr!("modeler-guides-none"));
    // A few go at once.
    for i in 0..crate::app::FEW_GUIDES {
        add_guide(&mut app, Guide::point(DVec3::new(i as f64, 0.0, 0.0)));
    }
    let depth = app.session.editor.undo_depth();
    app.run(Cmd::DeleteGuides, size(&app));
    assert!(app.confirm.is_none());
    assert!(guides(&app).is_empty());
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    assert!(app.session.editor.undo());
    assert_eq!(guides(&app).len(), crate::app::FEW_GUIDES);
    // More than a few ask first; No keeps them.
    add_guide(&mut app, Guide::point(DVec3::new(9.0, 0.0, 0.0)));
    let depth = app.session.editor.undo_depth();
    app.run(Cmd::DeleteGuides, size(&app));
    assert_eq!(
        app.confirm,
        Some(Confirm::DeleteGuides(crate::app::FEW_GUIDES + 1))
    );
    assert_eq!(guides(&app).len(), crate::app::FEW_GUIDES + 1);
    app.answer_confirm(false);
    assert!(app.confirm.is_none());
    assert_eq!(guides(&app).len(), crate::app::FEW_GUIDES + 1);
    // Yes deletes all of them in one step.
    app.run(Cmd::DeleteGuides, size(&app));
    app.answer_confirm(true);
    assert!(guides(&app).is_empty());
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    assert_eq!(
        app.status,
        tr!("modeler-guides-deleted", count = crate::app::FEW_GUIDES + 1)
    );
}

#[test]
fn a_hidden_face_is_not_drawn_not_picked_and_comes_back_with_hidden_geometry_and_unhide() {
    let (ctx, mut app, mut t, id) = ready();
    let top = top_face(&app, id);
    let full = triangles(&mut app);
    app.edit.sub.set([Item::Face(id, top)]);
    let depth = app.session.editor.undo_depth();
    app.run(Cmd::Hide, size(&app));
    assert_eq!(app.session.editor.undo_depth(), depth + 1, "one undo step");
    assert_eq!(triangles(&mut app), full - 2, "the top face is not drawn");
    assert!(app.edit.sub.is_empty());
    assert!(app.status.contains('1'), "{}", app.status);
    // A click on the place of the face does not pick it.
    choose(&ctx, &mut app, egui::Key::Space, &mut t);
    let over = screen_of(&mut app, DVec3::new(100.0, 100.0, 500.0));
    click_at(&ctx, &mut app, over, &mut t);
    assert!(
        !app.edit.sub.contains(Item::Face(id, top)),
        "a hidden face is not picked: {:?}",
        app.edit.sub
    );
    // Hidden Geometry shows it ghosted and lets the click pick it.
    app.run(Cmd::ToggleHiddenGeometry, size(&app));
    click_at(&ctx, &mut app, over, &mut t);
    assert!(
        app.edit.sub.contains(Item::Face(id, top)),
        "{:?}",
        app.edit.sub
    );
    assert_eq!(
        triangles(&mut app),
        full - 2,
        "the renderer still leaves it out"
    );
    // Unhide on the selected hidden face.
    assert!(app.can_unhide());
    app.run(Cmd::Unhide, size(&app));
    assert_eq!(triangles(&mut app), full);
    // Hide again, then Unhide All.
    app.edit.sub.set([Item::Face(id, top)]);
    app.run(Cmd::Hide, size(&app));
    assert_eq!(triangles(&mut app), full - 2);
    app.run(Cmd::UnhideAll, size(&app));
    assert_eq!(triangles(&mut app), full);
    app.run(Cmd::UnhideAll, size(&app));
    assert_eq!(app.status, tr!("modeler-unhide-all-nothing"));
}

#[test]
fn a_hidden_edge_is_not_drawn_and_does_not_merge_the_faces() {
    let (_ctx, mut app, _t, id) = ready();
    let edges_before = app.session.scene().lines.segment_count();
    let model_faces = |app: &ModelerApp| match &app.session.editor.doc().object(id).unwrap().shape {
        ModelShape::SurfaceModel(m) => m.face_count(),
        _ => 0,
    };
    let faces = model_faces(&app);
    let edge = match &app.session.editor.doc().object(id).unwrap().shape {
        ModelShape::SurfaceModel(m) => m.edges().next().unwrap().0,
        _ => panic!(),
    };
    app.edit.sub.set([Item::Edge(id, edge)]);
    app.run(Cmd::Hide, size(&app));
    assert_eq!(app.session.scene().lines.segment_count(), edges_before - 1);
    assert_eq!(model_faces(&app), faces, "no face merged");
}

#[test]
fn hiding_an_object_removes_it_from_the_view_and_unhide_all_brings_it_back() {
    let (ctx, mut app, mut t, id) = ready();
    let full = triangles(&mut app);
    app.session.selection = vec![id];
    let depth = app.session.editor.undo_depth();
    app.run(Cmd::Hide, size(&app));
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    assert_eq!(triangles(&mut app), 0);
    assert!(app.session.selection.is_empty());
    // The inference scene has no model either, until Hidden Objects shows it.
    let objects = |app: &mut ModelerApp| {
        let rev = app.session.editor.revision();
        let doc = app.session.editor.doc().clone();
        app.edit.cache.get(&doc, rev).objects().len()
    };
    assert_eq!(objects(&mut app), 0);
    app.run(Cmd::ToggleHiddenObjects, size(&app));
    assert_eq!(objects(&mut app), 1);
    assert_eq!(
        triangles(&mut app),
        0,
        "the renderer never draws hidden objects"
    );
    // Select All takes it only now.
    app.run(Cmd::SelectAll, size(&app));
    assert_eq!(app.session.selection, vec![id]);
    app.run(Cmd::ToggleHiddenObjects, size(&app));
    app.run(Cmd::SelectAll, size(&app));
    assert!(app.session.selection.is_empty());
    app.run(Cmd::UnhideAll, size(&app));
    assert_eq!(triangles(&mut app), full);
    assert!(app.session.editor.undo());
    assert_eq!(triangles(&mut app), 0);
    let _ = (&ctx, &mut t);
}

#[test]
fn hide_with_nothing_selected_says_so() {
    let (_ctx, mut app, _t, _) = ready();
    app.run(Cmd::Hide, size(&app));
    assert_eq!(app.status, tr!("modeler-hide-nothing"));
    app.run(Cmd::Unhide, size(&app));
    assert_eq!(app.status, tr!("modeler-unhide-nothing"));
    assert!(!app.can_hide() && !app.can_unhide());
}

#[test]
fn a_hidden_tag_hides_its_objects_in_the_view_the_picking_and_the_inference() {
    let (ctx, mut app, mut t, id) = ready();
    let full = triangles(&mut app);
    // A tag, and the box in it.
    let tag = ClassId(
        app.session
            .editor
            .execute(&Command::AddClass {
                name: "Trees".into(),
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
    );
    app.apply_tags(tags_panel::Outcome {
        commands: tags::set_tag(app.session.editor.doc(), &[id], tag)
            .into_iter()
            .collect(),
        ..tags_panel::Outcome::default()
    });
    assert_eq!(app.session.editor.doc().object(id).unwrap().class, tag);
    assert_eq!(triangles(&mut app), full, "a visible tag shows its objects");
    // The eye hides the tag.
    let depth = app.session.editor.undo_depth();
    app.apply_tags(tags_panel::Outcome {
        commands: tags::toggle_tag(app.session.editor.doc(), tag)
            .into_iter()
            .collect(),
        ..tags_panel::Outcome::default()
    });
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    assert_eq!(triangles(&mut app), 0, "not drawn");
    let rev = app.session.editor.revision();
    let doc = app.session.editor.doc().clone();
    assert!(
        app.edit.cache.get(&doc, rev).objects().is_empty(),
        "not in the inference"
    );
    choose(&ctx, &mut app, egui::Key::Space, &mut t);
    let over = screen_of(&mut app, DVec3::new(100.0, 100.0, 500.0));
    click_at(&ctx, &mut app, over, &mut t);
    assert!(
        app.edit.sub.is_empty() && app.session.selection.is_empty(),
        "not picked"
    );
    // The export leaves it out too: the same rule as for layers.
    assert!(
        !app.session
            .editor
            .doc()
            .is_visible(app.session.editor.doc().object(id).unwrap())
    );
    // Show it again.
    app.apply_tags(tags_panel::Outcome {
        commands: tags::toggle_tag(app.session.editor.doc(), tag)
            .into_iter()
            .collect(),
        ..tags_panel::Outcome::default()
    });
    assert_eq!(triangles(&mut app), full);
}

#[test]
fn the_tags_panel_lists_tags_and_layers_and_new_geometry_goes_to_the_active_tag() {
    let ctx = egui::Context::default();
    let mut app = app();
    let mut t = 0.0;
    app.apply_tags(tags_panel::Outcome {
        commands: vec![tags::new_tag(app.session.editor.doc(), "Paths").unwrap()],
        ..tags_panel::Outcome::default()
    });
    let on_screen = shown(&ctx, &mut app, &mut t);
    for want in [
        tr!("modeler-tray-tags"),
        tr!("modeler-tags-untagged"),
        "Paths".to_string(),
        tr!("modeler-tags-layers"),
        "Layer 1".to_string(),
        tr!("modeler-tags-new"),
        tr!("modeler-tags-delete"),
    ] {
        assert!(
            on_screen.contains(&want),
            "{want:?} is not on the screen:\n{on_screen}"
        );
    }
    // The active tag takes the new geometry.
    let doc = app.session.editor.doc();
    let paths = doc.classes.iter().find(|c| c.name == "Paths").unwrap().id;
    app.apply_tags(tags_panel::Outcome {
        commands: vec![tags::set_active_tag(paths)],
        ..tags_panel::Outcome::default()
    });
    let id = add_guide(&mut app, Guide::point(DVec3::ZERO));
    assert_eq!(app.session.editor.doc().object(id).unwrap().class, paths);
}

#[test]
fn the_tags_panel_is_in_estonian_too() {
    kadakas_l10n::with_language("et", || {
        let ctx = egui::Context::default();
        let mut app = app_with(Prefs {
            language: Some("et".into()),
            ..Prefs::default()
        });
        let mut t = 0.0;
        let on_screen = shown(&ctx, &mut app, &mut t);
        assert!(
            on_screen.contains("Sildid") && on_screen.contains("Sildita"),
            "{on_screen}"
        );
        assert!(on_screen.contains("Kihid"), "{on_screen}");
        assert!(!on_screen.contains("Untagged"), "{on_screen}");
    });
}

#[test]
fn deleting_a_tag_with_objects_asks_first_and_moves_them_to_untagged() {
    let (ctx, mut app, mut t, id) = ready();
    // The tray was hidden by `ready`; the panel needs it.
    app.run(Cmd::ToggleTray, size(&app));
    let tag = ClassId(
        app.session
            .editor
            .execute(&Command::AddClass {
                name: "Trees".into(),
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
    );
    let cmd = tags::set_tag(app.session.editor.doc(), &[id], tag).unwrap();
    app.session.editor.execute(&cmd).unwrap();
    // The window asks: the panel keeps the tag and states the question.
    app.tags.chosen = Some(tag);
    app.tags.deleting = Some((
        tag,
        "Trees".into(),
        kadakas_core::classes::uses(app.session.editor.doc(), tag),
    ));
    // A window takes one frame to find its size, then it shows.
    let _ = shown(&ctx, &mut app, &mut t);
    let text = shown(&ctx, &mut app, &mut t);
    assert!(text.contains(&tr!("modeler-tags-delete-title")), "{text}");
    assert!(
        app.session.editor.doc().class(tag).is_some(),
        "nothing moved without the answer"
    );
    // The answer through the plan: tag deleted, objects in Untagged, one undo step.
    let plan = tags::delete_tag(app.session.editor.doc(), tag).unwrap();
    assert_eq!(plan.uses.objects, 1);
    let depth = app.session.editor.undo_depth();
    app.apply_tags(tags_panel::Outcome {
        commands: vec![plan.command],
        after: Some(tr!("modeler-tags-deleted", count = 1, name = "Trees")),
        ..tags_panel::Outcome::default()
    });
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    assert!(app.session.editor.doc().class(tag).is_none());
    assert_eq!(
        app.session.editor.doc().object(id).unwrap().class,
        kadakas_core::model::NONE_CLASS
    );
    assert!(app.status.contains("Trees"));
}

#[test]
fn the_exports_of_a_modeler_document_say_what_they_left_out() {
    let (_ctx, mut app, _t, id) = ready();
    add_guide(&mut app, Guide::line(DVec3::ZERO, DVec3::X));
    add_guide(&mut app, Guide::point(DVec3::Y));
    let top = top_face(&app, id);
    app.edit.sub.set([Item::Face(id, top)]);
    app.run(Cmd::Hide, size(&app));
    let dir = temp("export-left-out");
    let path = dir.join("model.glb");
    let r = app
        .session
        .handle(
            "export.file",
            serde_json::json!({"path": path, "format": "glb"}),
        )
        .unwrap();
    assert_eq!(r["left_out"]["guides"], 2, "{r}");
    assert_eq!(r["left_out"]["hidden_faces"], 1, "{r}");
    let notes = r["left_out"]["notes"].as_array().unwrap();
    assert!(
        notes
            .iter()
            .any(|n| n == "2 guides left out: construction geometry"),
        "{notes:?}"
    );
    // The document in the window is unchanged by the export.
    assert_eq!(guides(&app).len(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_edit_and_view_menus_have_the_new_items_and_the_texts_exist_in_both_languages() {
    for id in [
        "modeler-edit-hide",
        "modeler-edit-unhide",
        "modeler-edit-unhide-all",
        "modeler-edit-delete-guides",
        "modeler-view-guides",
        "modeler-view-hidden-geometry",
        "modeler-view-hidden-objects",
    ] {
        let en = kadakas_l10n::with_language("en-US", || kadakas_l10n::translate(id, &[]));
        let et = kadakas_l10n::with_language("et", || kadakas_l10n::translate(id, &[]));
        assert!(!en.is_empty() && en != id, "{id}");
        assert!(!et.is_empty() && et != id && et != en, "{id}");
    }
}

#[test]
fn undo_ends_a_tape_measure_that_is_running_and_show_options_have_defaults() {
    let (ctx, mut app, mut t, _) = ready();
    iso(&ctx, &mut app, &mut t);
    choose(&ctx, &mut app, egui::Key::T, &mut t);
    let a = screen_of(&mut app, DVec3::new(-200.0, -500.0, 500.0));
    click_at(&ctx, &mut app, a, &mut t);
    assert!(app.edit.modeling.in_progress());
    app.run(Cmd::Undo, size(&app));
    assert!(!app.edit.modeling.in_progress());
    assert_eq!(app.edit.cache.show, ShowOptions::default());
    let _ = (type_more, make_box);
}
