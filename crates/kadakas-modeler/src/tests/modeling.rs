//! Tests of modeling in the window (MOD-08): the real input path (pointer events, keys and text)
//! through the headless frames. Nothing is drawn because no GPU runs.

use super::*;

/// A program with an empty model seen from above, so that a pointer position is a point of the
/// ground plane.
fn top_view() -> (egui::Context, ModelerApp, f64) {
    let ctx = egui::Context::default();
    let mut app = app();
    app.gpu_ready = true;
    app.nav.set_projection(Projection::Orthographic);
    app.nav.camera.set_view(StandardView::Top);
    let mut t = 0.0;
    for _ in 0..3 {
        let _ = frame(&ctx, &mut app, vec![], t);
        t += 0.1;
    }
    assert!(app.canvas_rect.is_some());
    (ctx, app, t)
}

fn choose(ctx: &egui::Context, app: &mut ModelerApp, key: egui::Key, t: &mut f64) {
    *t += 0.1;
    let _ = frame(ctx, app, tap(key, egui::Modifiers::NONE), *t);
}

/// A click of the primary button: move, press, release, one more frame.
fn click_at(ctx: &egui::Context, app: &mut ModelerApp, p: egui::Pos2, t: &mut f64) {
    let none = egui::Modifiers::NONE;
    let b = egui::PointerButton::Primary;
    let _ = frame(ctx, app, vec![moved(p)], *t);
    *t += 0.05;
    let _ = frame(ctx, app, vec![button(p, b, true, none)], *t);
    *t += 0.05;
    let _ = frame(ctx, app, vec![button(p, b, false, none)], *t);
    *t += 0.05;
    let _ = frame(ctx, app, vec![], *t);
    *t += 0.2;
}

/// Types text into the Measurements box: each character is a key and a text event, as in a window.
fn type_keys(ctx: &egui::Context, app: &mut ModelerApp, text: &str, t: &mut f64) {
    use egui::Key;
    for c in text.chars() {
        let key = match c {
            '0' => Key::Num0,
            '1' => Key::Num1,
            '2' => Key::Num2,
            '3' => Key::Num3,
            '4' => Key::Num4,
            '5' => Key::Num5,
            '6' => Key::Num6,
            '7' => Key::Num7,
            '8' => Key::Num8,
            '9' => Key::Num9,
            ';' => Key::Semicolon,
            's' => Key::S,
            'r' => Key::R,
            '.' => Key::Period,
            other => panic!("no key for {other:?}"),
        };
        *t += 0.05;
        let mut ev = tap(key, egui::Modifiers::NONE);
        ev.push(egui::Event::Text(c.to_string()));
        let _ = frame(ctx, app, ev, *t);
    }
}

fn enter(ctx: &egui::Context, app: &mut ModelerApp, t: &mut f64) {
    *t += 0.1;
    let _ = frame(ctx, app, tap(egui::Key::Enter, egui::Modifiers::NONE), *t);
    *t += 0.1;
    let _ = frame(ctx, app, vec![], *t);
}

fn surface(app: &ModelerApp) -> &kadakas_core::surface::SurfaceModel {
    app.session
        .editor
        .doc()
        .objects
        .values()
        .find_map(|o| match &o.shape {
            ModelShape::SurfaceModel(m) => Some(&**m),
            _ => None,
        })
        .expect("a surface model")
}

#[test]
fn the_drawing_tool_keys_choose_the_tools() {
    let ctx = egui::Context::default();
    let mut app = app();
    let mut t = 0.0;
    for (key, want) in [
        (egui::Key::L, ModelerTool::Line),
        (egui::Key::R, ModelerTool::Rectangle),
        (egui::Key::C, ModelerTool::Circle),
        (egui::Key::A, ModelerTool::Arc),
        (egui::Key::P, ModelerTool::PushPull),
        (egui::Key::E, ModelerTool::Eraser),
        (egui::Key::Space, ModelerTool::Select),
    ] {
        choose(&ctx, &mut app, key, &mut t);
        assert_eq!(app.tools.active(), want, "{key:?}");
    }
}

#[test]
fn a_rectangle_is_drawn_and_pulled_into_a_box_through_the_real_input_path() {
    let (ctx, mut app, mut t) = top_view();
    let depth = app.session.editor.undo_depth();
    let c = center(&app);
    choose(&ctx, &mut app, egui::Key::R, &mut t);
    assert_eq!(app.tools.active(), ModelerTool::Rectangle);
    click_at(&ctx, &mut app, c - egui::vec2(60.0, 35.0), &mut t);
    assert!(app.edit.modeling.in_progress());
    // The second corner: the Measurements box shows the size while the pointer moves.
    let _ = frame(&ctx, &mut app, vec![moved(c + egui::vec2(60.0, 35.0))], t);
    t += 0.1;
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    assert!(
        shown.contains(&tr!("uic-modeler-measure-dimensions")),
        "{shown}"
    );
    click_at(&ctx, &mut app, c + egui::vec2(60.0, 35.0), &mut t);
    assert_eq!(app.session.editor.undo_depth(), depth + 1, "one undo step");
    let m = surface(&app);
    assert_eq!(
        (m.vertex_count(), m.edge_count(), m.face_count()),
        (4, 4, 1)
    );
    // Push/Pull: click the face, type a distance.
    choose(&ctx, &mut app, egui::Key::P, &mut t);
    click_at(&ctx, &mut app, c + egui::vec2(10.0, 5.0), &mut t);
    assert!(app.edit.modeling.in_progress(), "the face was picked");
    type_keys(&ctx, &mut app, "500", &mut t);
    assert_eq!(app.edit.modeling.typed(), "500");
    enter(&ctx, &mut app, &mut t);
    assert_eq!(app.session.editor.undo_depth(), depth + 2, "one more step");
    let m = surface(&app);
    assert_eq!(
        (m.vertex_count(), m.edge_count(), m.face_count()),
        (8, 12, 6),
        "a box"
    );
    assert!(m.check().is_closed(), "{:?}", m.check());
    let (lo, hi) = m.local_bounds().unwrap();
    assert!((hi.z - lo.z - 500.0).abs() < 1e-6);
    // Entity Info says it is a closed volume once the box is selected as a whole.
    let id = *app.session.editor.doc().objects.keys().next().unwrap();
    app.session.selection = vec![id];
    let _ = frame(&ctx, &mut app, vec![], t + 0.1);
    let shown = texts(&frame(&ctx, &mut app, vec![], t + 0.2)).join("\n");
    assert!(
        shown.contains(&tr!("modeler-entity-solid-closed")),
        "{shown}"
    );
    // Undo twice takes everything back, the model with it.
    app.run(Cmd::Undo, size(&app));
    app.run(Cmd::Undo, size(&app));
    assert!(app.session.editor.doc().objects.is_empty());
}

#[test]
fn a_line_loop_makes_a_face_and_escape_ends_the_line() {
    let (ctx, mut app, mut t) = top_view();
    let c = center(&app);
    choose(&ctx, &mut app, egui::Key::L, &mut t);
    let pts = [
        c + egui::vec2(-50.0, -40.0),
        c + egui::vec2(50.0, -40.0),
        c + egui::vec2(50.0, 40.0),
        c + egui::vec2(-50.0, 40.0),
        c + egui::vec2(-50.0, -40.0),
    ];
    for p in pts {
        click_at(&ctx, &mut app, p, &mut t);
    }
    let m = surface(&app);
    assert_eq!(
        (m.edge_count(), m.face_count()),
        (4, 1),
        "a face from the loop"
    );
    assert!(
        !app.edit.modeling.in_progress(),
        "the closed loop ended the chain"
    );
    // Esc in the middle of a line ends it but keeps the tool.
    click_at(&ctx, &mut app, c + egui::vec2(100.0, 100.0), &mut t);
    assert!(app.edit.modeling.in_progress());
    let _ = frame(
        &ctx,
        &mut app,
        tap(egui::Key::Escape, egui::Modifiers::NONE),
        t,
    );
    assert!(!app.edit.modeling.in_progress());
    assert_eq!(app.tools.active(), ModelerTool::Line);
    // A second Esc goes back to Select.
    t += 0.2;
    let _ = frame(
        &ctx,
        &mut app,
        tap(egui::Key::Escape, egui::Modifiers::NONE),
        t,
    );
    assert_eq!(app.tools.active(), ModelerTool::Select);
}

#[test]
fn digits_go_to_the_box_and_do_not_choose_views_and_a_circle_takes_its_sides() {
    let (ctx, mut app, mut t) = top_view();
    let c = center(&app);
    choose(&ctx, &mut app, egui::Key::C, &mut t);
    click_at(&ctx, &mut app, c, &mut t);
    let view_before = (app.nav.camera.yaw, app.nav.camera.pitch);
    type_keys(&ctx, &mut app, "12s", &mut t);
    assert_eq!(app.edit.modeling.typed(), "12s");
    assert_eq!(
        (app.nav.camera.yaw, app.nav.camera.pitch),
        view_before,
        "digits are not standard views"
    );
    assert_eq!(app.tools.active(), ModelerTool::Circle);
    enter(&ctx, &mut app, &mut t);
    assert_eq!(app.edit.modeling.sides, 12);
    click_at(&ctx, &mut app, c + egui::vec2(60.0, 0.0), &mut t);
    let m = surface(&app);
    assert_eq!((m.vertex_count(), m.face_count()), (12, 1));
    assert_eq!(m.curves().count(), 1);
}

#[test]
fn a_click_selects_a_face_an_edge_and_delete_erases_it() {
    let (ctx, mut app, mut t) = top_view();
    let c = center(&app);
    choose(&ctx, &mut app, egui::Key::R, &mut t);
    click_at(&ctx, &mut app, c - egui::vec2(60.0, 35.0), &mut t);
    click_at(&ctx, &mut app, c + egui::vec2(60.0, 35.0), &mut t);
    choose(&ctx, &mut app, egui::Key::Space, &mut t);
    // The inside of the face.
    click_at(&ctx, &mut app, c + egui::vec2(20.0, 10.0), &mut t);
    assert_eq!(app.edit.sub.len(), 1, "{:?}", app.edit.sub);
    assert!(app.session.selection.is_empty());
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    assert!(shown.contains(&tr!("modeler-entity-face")), "{shown}");
    // Delete erases the face; the edges stay.
    let depth = app.session.editor.undo_depth();
    let _ = frame(
        &ctx,
        &mut app,
        tap(egui::Key::Delete, egui::Modifiers::NONE),
        t + 0.1,
    );
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    assert_eq!(surface(&app).face_count(), 0);
    assert_eq!(surface(&app).edge_count(), 4);
    // The bottom edge, near its middle.
    t += 0.5;
    click_at(&ctx, &mut app, c + egui::vec2(0.0, 35.0), &mut t);
    assert_eq!(app.edit.sub.len(), 1);
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    assert!(shown.contains(&tr!("modeler-entity-length")), "{shown}");
    // A click on empty space clears the selection.
    click_at(&ctx, &mut app, c + egui::vec2(300.0, 200.0), &mut t);
    assert!(app.edit.sub.is_empty());
}

#[test]
fn a_window_and_a_crossing_rectangle_select_the_parts_they_hold_or_touch() {
    let (ctx, mut app, mut t) = top_view();
    let c = center(&app);
    choose(&ctx, &mut app, egui::Key::R, &mut t);
    click_at(&ctx, &mut app, c - egui::vec2(60.0, 35.0), &mut t);
    click_at(&ctx, &mut app, c + egui::vec2(60.0, 35.0), &mut t);
    choose(&ctx, &mut app, egui::Key::Space, &mut t);
    let none = egui::Modifiers::NONE;
    // Left to right around everything: a window with the face and the four edges.
    t = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        c - egui::vec2(120.0, 90.0),
        c + egui::vec2(120.0, 90.0),
        none,
        none,
        t,
    );
    assert_eq!(app.edit.sub.len(), 5, "{:?}", app.edit.sub);
    // Right to left over a corner: a crossing rectangle with the two edges and the face.
    t = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        c + egui::vec2(100.0, 70.0),
        c + egui::vec2(40.0, 20.0),
        none,
        none,
        t,
    );
    assert_eq!(app.edit.sub.len(), 3, "{:?}", app.edit.sub);
    // A small window inside the face holds nothing.
    let _ = drag(
        &ctx,
        &mut app,
        egui::PointerButton::Primary,
        c - egui::vec2(10.0, 10.0),
        c + egui::vec2(10.0, 10.0),
        none,
        none,
        t,
    );
    assert!(app.edit.sub.is_empty());
}

#[test]
fn the_inference_scene_is_built_once_per_revision_not_every_frame() {
    let (ctx, mut app, mut t) = top_view();
    let c = center(&app);
    choose(&ctx, &mut app, egui::Key::R, &mut t);
    click_at(&ctx, &mut app, c - egui::vec2(60.0, 35.0), &mut t);
    let _ = frame(&ctx, &mut app, vec![moved(c)], t);
    let builds = app.edit.cache.builds;
    for i in 0..40 {
        t += 0.02;
        let _ = frame(
            &ctx,
            &mut app,
            vec![moved(c + egui::vec2(i as f32, 0.0))],
            t,
        );
    }
    assert_eq!(app.edit.cache.builds, builds, "40 frames, no build");
    click_at(&ctx, &mut app, c + egui::vec2(60.0, 35.0), &mut t);
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![moved(c)], t);
    assert!(app.edit.cache.builds > builds, "a new revision builds once");
    let after = app.edit.cache.builds;
    for i in 0..20 {
        t += 0.02;
        let _ = frame(
            &ctx,
            &mut app,
            vec![moved(c + egui::vec2(i as f32, 1.0))],
            t,
        );
    }
    assert_eq!(app.edit.cache.builds, after);
}

#[test]
fn the_modeling_texts_exist_in_both_languages() {
    for lang in ["en-US", "et"] {
        kadakas_l10n::with_language(lang, || {
            for t in [
                ModelerTool::Eraser,
                ModelerTool::Line,
                ModelerTool::Rectangle,
                ModelerTool::Circle,
                ModelerTool::Arc,
                ModelerTool::PushPull,
            ] {
                let h = t.hint();
                assert!(
                    !h.is_empty() && h != tr!("uic-modeler-coming-later"),
                    "{lang} {t:?}"
                );
            }
            for id in [
                "modeler-entity-face",
                "modeler-entity-edge",
                "modeler-entity-solid-closed",
                "uic-modeler-measure-length",
                "uic-modeling-pick-face",
            ] {
                assert!(!tr!(id).is_empty() && tr!(id) != id, "{lang} {id}");
            }
        });
    }
    let en = kadakas_l10n::with_language("en-US", || ModelerTool::Line.hint());
    let et = kadakas_l10n::with_language("et", || ModelerTool::Line.hint());
    assert_ne!(en, et);
}

mod editing;
mod guides;
mod intersect;
mod landscape;
mod mod08;
mod mod15;
mod paint;
mod solid;
mod transform;
