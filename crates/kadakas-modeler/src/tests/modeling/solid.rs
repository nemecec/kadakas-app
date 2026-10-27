//! The solid tools in the window (MOD-09): the Solid Tools submenu of the Edit menu, the status
//! line with the preview and the refusal, one undo step for an operation, and Check Solid.

use super::*;
use kadakas_core::model::ObjectId;
use kadakas_core::surface::{Edit, FaceId, ShellState, SolidOp};
use kadakas_ui_core::modeling::Item;
use kadakas_ui_core::modeling::solid::Problem;

type P = [f64; 3];

fn cuboid(lo: P, hi: P) -> Vec<[P; 2]> {
    let c =
        |i: usize| -> P { std::array::from_fn(|k| if i >> k & 1 == 0 { lo[k] } else { hi[k] }) };
    let mut out = Vec::new();
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                out.push([c(i), c(i | bit)]);
            }
        }
    }
    out
}

fn edit(app: &mut ModelerApp, id: ObjectId, e: Edit) {
    app.session
        .editor
        .execute(&Command::EditSurface { id, edit: e })
        .unwrap();
}

/// Two crossing boxes in one model, as two solids with outward faces.
fn two_solids(app: &mut ModelerApp) -> (ObjectId, Vec<FaceId>, Vec<FaceId>) {
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
    for (lo, hi) in [([0.0; 3], [1000.0; 3]), ([500.0; 3], [1500.0; 3])] {
        edit(
            app,
            id,
            Edit::AddEdges {
                edges: cuboid(lo, hi),
            },
        );
    }
    let inward: Vec<FaceId> = surface(app)
        .solid_report()
        .shells
        .iter()
        .filter(|s| s.state == ShellState::Inverted)
        .flat_map(|s| s.faces.clone())
        .collect();
    edit(app, id, Edit::ReverseFaces { faces: inward });
    let r = surface(app).solid_report();
    let s: Vec<_> = r.solids().collect();
    (id, s[0].faces.clone(), s[1].faces.clone())
}

fn select_faces(app: &mut ModelerApp, id: ObjectId, faces: &[FaceId]) {
    app.edit.sub.set(faces.iter().map(|f| Item::Face(id, *f)));
}

fn text_at(out: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    fn walk(shape: &Shape, text: &str) -> Option<egui::Pos2> {
        match shape {
            Shape::Text(t) if t.galley.text() == text => {
                Some(t.pos + t.galley.rect.center().to_vec2())
            }
            Shape::Vec(v) => v.iter().find_map(|s| walk(s, text)),
            _ => None,
        }
    }
    out.shapes.iter().find_map(|c| walk(&c.shape, text))
}

#[test]
fn an_operation_runs_from_the_selection_in_one_undo_step() {
    let (_ctx, mut app, _t) = top_view();
    let (id, a, b) = two_solids(&mut app);
    // Nothing selected: the status line says what to select.
    app.run(Cmd::Solid(SolidOp::Union), size(&app));
    assert_eq!(app.status, tr!("uic-solid-need-selection"));
    select_faces(&mut app, id, &[a[0]]);
    app.run(Cmd::Solid(SolidOp::Union), size(&app));
    assert_eq!(app.status, tr!("uic-solid-need-two", found = 1));
    // One face of each solid.
    select_faces(&mut app, id, &[a[0], b[0]]);
    let depth = app.session.editor.undo_depth();
    app.run(Cmd::Solid(SolidOp::Union), size(&app));
    assert_eq!(
        app.status,
        tr!("uic-solid-done-union", volume = "1.875"),
        "{}",
        app.status
    );
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    assert_eq!(surface(&app).solid_report().solids().count(), 1);
    assert!(app.edit.sub.is_empty(), "the parts have new ids");
    app.run(Cmd::Undo, size(&app));
    assert_eq!(surface(&app).solid_report().solids().count(), 2);
}

#[test]
fn the_order_of_the_solids_can_be_swapped() {
    let (_ctx, mut app, _t) = top_view();
    let (id, a, b) = two_solids(&mut app);
    select_faces(&mut app, id, &[a[0], b[0]]);
    let o = app.solid_operands().unwrap();
    assert_eq!(o.first, a);
    app.run(Cmd::SolidSwap, size(&app));
    assert!(app.solid_swap);
    assert_eq!(app.status, tr!("modeler-solid-order-swapped"));
    assert_eq!(app.solid_operands().unwrap().first, b);
    // The hint is cached for the same selection and order.
    let one = app.solid_hint(SolidOp::Subtract);
    assert_eq!(one, app.solid_hint(SolidOp::Subtract));
    assert_ne!(one, app.solid_hint(SolidOp::Union));
    app.run(Cmd::SolidSwap, size(&app));
    assert_eq!(app.status, tr!("modeler-solid-order-normal"));
}

#[test]
fn a_refusal_is_shown_in_the_status_line_and_changes_nothing() {
    let (_ctx, mut app, _t) = top_view();
    let (id, a, b) = two_solids(&mut app);
    edit(&mut app, id, Edit::EraseFaces { faces: vec![a[4]] });
    select_faces(&mut app, id, &[a[0], b[0]]);
    let before = surface(&app).clone();
    let depth = app.session.editor.undo_depth();
    app.run(Cmd::Solid(SolidOp::Subtract), size(&app));
    assert_eq!(
        app.status,
        tr!("uic-solid-refused-open", which = "first"),
        "{}",
        app.status
    );
    assert_eq!(surface(&app), &before);
    assert_eq!(app.session.editor.undo_depth(), depth);
    // The hint of the item says the same.
    assert_eq!(app.solid_hint(SolidOp::Subtract), app.status);
}

#[test]
fn check_solid_selects_the_open_edges() {
    let (_ctx, mut app, _t) = top_view();
    let (id, a, _) = two_solids(&mut app);
    app.session.selection = vec![id];
    app.run(Cmd::CheckSolid, size(&app));
    assert_eq!(
        app.status,
        tr!("uic-solid-check-closed", solids = 2, volume = "2.000")
    );
    edit(&mut app, id, Edit::EraseFaces { faces: vec![a[4]] });
    app.run(Cmd::CheckSolid, size(&app));
    assert_eq!(
        app.status,
        tr!("uic-solid-check-open", edges = 4, solids = 1)
    );
    assert_eq!(app.edit.sub.edges().len(), 4);
    assert!(app.session.selection.is_empty());
}

#[test]
fn the_solid_tools_menu_lists_the_operations_with_their_previews() {
    let (ctx, mut app, mut t) = top_view();
    let (id, a, b) = two_solids(&mut app);
    select_faces(&mut app, id, &[a[0], b[0]]);
    let _ = frame(&ctx, &mut app, vec![], t);
    // Open the Edit menu, then Solid Tools.
    let out = frame(&ctx, &mut app, vec![], t + 0.05);
    let edit_menu = text_at(&out, &tr!("modeler-menu-edit")).expect("the Edit menu");
    let none = egui::Modifiers::NONE;
    let left = egui::PointerButton::Primary;
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![moved(edit_menu)], t);
    t += 0.05;
    let _ = frame(&ctx, &mut app, vec![button(edit_menu, left, true, none)], t);
    t += 0.05;
    let _ = frame(
        &ctx,
        &mut app,
        vec![button(edit_menu, left, false, none)],
        t,
    );
    t += 0.05;
    let out = frame(&ctx, &mut app, vec![], t);
    let item = text_at(&out, &tr!("modeler-edit-solid")).expect("the submenu is in the Edit menu");
    let mut shown = String::new();
    for _ in 0..8 {
        t += 0.1;
        let out = frame(&ctx, &mut app, vec![moved(item)], t);
        shown = texts(&out).join("\n");
        if shown.contains(&tr!("modeler-solid-check")) {
            break;
        }
    }
    for want in [
        tr!("modeler-solid-check"),
        tr!("uic-solid-op-union"),
        tr!("uic-solid-op-subtract"),
        tr!("uic-solid-op-intersect"),
        tr!("uic-solid-op-trim"),
        tr!("uic-solid-op-split"),
        tr!("modeler-solid-swap"),
    ] {
        assert!(shown.contains(&want), "{want}: {shown}");
    }
    // Without two solids the same menu draws with the operations disabled.
    app.edit.sub.clear();
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![moved(item)], t);
    assert!(app.solid_operands().is_err());
}

#[test]
fn the_solid_strings_are_in_both_languages() {
    for key in [
        "modeler-edit-solid",
        "modeler-solid-check",
        "modeler-solid-swap",
        "modeler-solid-order-normal",
        "modeler-solid-order-swapped",
        "modeler-entity-open-edges",
        "modeler-entity-open-edges-value",
    ] {
        let en = kadakas_l10n::with_language("en-US", || tr!(key));
        let et = kadakas_l10n::with_language("et", || tr!(key));
        assert_ne!(en, et, "{key}");
    }
}

#[test]
fn entity_info_names_the_open_edges() {
    let (ctx, mut app, mut t) = top_view();
    let (id, a, _) = two_solids(&mut app);
    app.session.selection = vec![id];
    let _ = frame(&ctx, &mut app, vec![], t);
    t += 0.1;
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    assert!(
        !shown.contains(&tr!("modeler-entity-open-edges")),
        "{shown}"
    );
    edit(&mut app, id, Edit::EraseFaces { faces: vec![a[4]] });
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![], t);
    t += 0.1;
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    assert!(shown.contains(&tr!("modeler-entity-open-edges")), "{shown}");
    assert!(
        shown.contains(&tr!("modeler-entity-open-edges-value", count = 4)),
        "{shown}"
    );
}

#[test]
fn a_preview_is_made_once_for_each_operation_and_the_click_reuses_it() {
    let (_ctx, mut app, _t) = top_view();
    let (id, a, b) = two_solids(&mut app);
    select_faces(&mut app, id, &[a[0], b[0]]);
    // Many frames ask for the five items: each operation runs once on a copy, not once a frame.
    for _ in 0..3 {
        for op in SolidOp::ALL {
            assert!(!app.solid_hint(op).is_empty());
        }
    }
    assert_eq!(app.solid_runs.get(), 5);
    // Running an item uses the preview that is there.
    app.run(Cmd::Solid(SolidOp::Union), size(&app));
    assert_eq!(app.solid_runs.get(), 5);
    assert_eq!(app.status, tr!("uic-solid-done-union", volume = "1.875"));
    // Another order or selection is another key.
    let r = surface(&app).solid_report();
    let s: Vec<_> = r.solids().collect();
    assert_eq!(s.len(), 1);
    let f = s[0].faces[0];
    select_faces(&mut app, id, &[f]);
    let _ = app.solid_hint(SolidOp::Union);
    assert_eq!(app.solid_runs.get(), 6, "a new selection is a new key");
    assert_eq!(
        app.solid_hint(SolidOp::Union),
        Problem::Count(1).text(),
        "one solid: there is nothing to join"
    );
    assert_eq!(app.solid_runs.get(), 6);
}

// ---- snap near-touches with consent (MOD-19) -------------------------------------------------

/// The box `[0, 1000]^3` and a turned, tilted box whose corner lies 0.0058 mm off the plane
/// `y = 1000`, inside its face: every solid operation is refused.
fn near_touch(app: &mut ModelerApp) -> (ObjectId, Vec<FaceId>, Vec<FaceId>) {
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
    edit(
        app,
        id,
        Edit::AddEdges {
            edges: cuboid([0.0; 3], [1000.0; 3]),
        },
    );
    let (angle, tilt, d): (f64, f64, P) = (
        1.539_623_148_363_857_6,
        0.394_573_062_789_783_6,
        [200.0, 796.195_805_697_005_2, 200.0],
    );
    let (sin, cos) = angle.sin_cos();
    let (st, ct) = tilt.sin_cos();
    let corners: Vec<P> = (0..8usize)
        .map(|i| {
            let p = |bit: usize| if i & bit == 0 { -400.0 } else { 400.0 };
            let (x, y, z) = (p(1), p(2), p(4));
            let (zx, zy) = (x * cos - y * sin, x * sin + y * cos);
            let (ty, tz) = (zy * ct - z * st, zy * st + z * ct);
            [zx + d[0], ty + d[1], tz + d[2]]
        })
        .collect();
    let mut edges = Vec::new();
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                edges.push([corners[i], corners[i | bit]]);
            }
        }
    }
    edit(app, id, Edit::AddEdges { edges });
    let inward: Vec<FaceId> = surface(app)
        .solid_report()
        .shells
        .iter()
        .filter(|s| s.state == ShellState::Inverted)
        .flat_map(|s| s.faces.clone())
        .collect();
    edit(app, id, Edit::ReverseFaces { faces: inward });
    let r = surface(app).solid_report();
    let s: Vec<_> = r.solids().collect();
    (id, s[0].faces.clone(), s[1].faces.clone())
}

#[test]
fn a_near_touch_refusal_has_a_snap_and_retry_button_that_runs_in_one_undo_step() {
    let (ctx, mut app, mut t) = top_view();
    let (id, a, b) = near_touch(&mut app);
    select_faces(&mut app, id, &[a[0], b[0]]);
    let before = surface(&app).clone();
    let depth = app.session.editor.undo_depth();
    // The refusal names the distance, changes nothing and offers the snap.
    app.run(Cmd::Solid(SolidOp::Union), size(&app));
    assert_eq!(
        app.status,
        tr!(
            "uic-solid-refused-near-touch",
            count = 1,
            distance = "0.0058"
        )
    );
    assert_eq!(app.solid_snap_offer(), Some(SolidOp::Union));
    assert_eq!(app.session.editor.undo_depth(), depth);
    assert_eq!(surface(&app), &before);
    // The button is on the status bar. A click on it runs the operation with the snap.
    let _ = frame(&ctx, &mut app, vec![], t);
    t += 0.1;
    let out = frame(&ctx, &mut app, vec![], t);
    let label = tr!("modeler-solid-snap-retry");
    let at = text_at(&out, &label).expect("the Snap and retry button");
    click_at(&ctx, &mut app, at, &mut t);
    assert!(
        app.status
            .starts_with("Snapped one vertex by 0.0058 mm at most."),
        "{}",
        app.status
    );
    assert_eq!(app.session.editor.undo_depth(), depth + 1);
    assert_eq!(surface(&app).solid_report().solids().count(), 1);
    assert_eq!(app.solid_snap_offer(), None);
    // One undo gives the model back.
    app.run(Cmd::Undo, size(&app));
    assert_eq!(surface(&app), &before);
    // The button is gone.
    t += 0.1;
    let out = frame(&ctx, &mut app, vec![], t);
    assert!(text_at(&out, &label).is_none());
}

#[test]
fn the_snap_button_goes_when_the_selection_or_the_text_changes() {
    let (ctx, mut app, mut t) = top_view();
    let (id, a, b) = near_touch(&mut app);
    select_faces(&mut app, id, &[a[0], b[0]]);
    app.run(Cmd::Solid(SolidOp::Subtract), size(&app));
    assert_eq!(app.solid_snap_offer(), Some(SolidOp::Subtract));
    // Another order: another key.
    app.run(Cmd::SolidSwap, size(&app));
    assert_eq!(app.solid_snap_offer(), None);
    app.run(Cmd::SolidSwap, size(&app));
    app.run(Cmd::Solid(SolidOp::Subtract), size(&app));
    assert_eq!(app.solid_snap_offer(), Some(SolidOp::Subtract));
    // Another selection.
    select_faces(&mut app, id, &[a[0]]);
    assert_eq!(app.solid_snap_offer(), None);
    select_faces(&mut app, id, &[a[0], b[0]]);
    app.run(Cmd::Solid(SolidOp::Subtract), size(&app));
    // Another text on the status line.
    app.status = "something else".into();
    assert_eq!(app.solid_snap_offer(), None);
    t += 0.1;
    let out = frame(&ctx, &mut app, vec![], t);
    assert!(text_at(&out, &tr!("modeler-solid-snap-retry")).is_none());
    // A refusal that no snap resolves has no button, and neither has a successful operation.
    select_faces(&mut app, id, &[a[0]]);
    app.run(Cmd::Solid(SolidOp::Union), size(&app));
    assert_eq!(app.solid_snap_offer(), None);
}

#[test]
fn snap_and_retry_without_a_near_touch_or_a_selection_says_why() {
    let (_ctx, mut app, _t) = top_view();
    let (id, a, b) = two_solids(&mut app);
    app.run(Cmd::SolidSnap(SolidOp::Union), size(&app));
    assert_eq!(app.status, tr!("uic-solid-need-selection"));
    select_faces(&mut app, id, &[a[0], b[0]]);
    // The model has no near-touch: the same result as the plain operation.
    app.run(Cmd::SolidSnap(SolidOp::Union), size(&app));
    assert_eq!(app.status, tr!("uic-solid-done-union", volume = "1.875"));
    assert_eq!(surface(&app).solid_report().solids().count(), 1);
}

#[test]
fn the_snap_strings_are_in_both_languages() {
    for key in [
        "modeler-solid-snap-retry",
        "modeler-solid-snap-retry-tip",
        "uic-solid-refused-snap-shared",
        "uic-solid-refused-snap-degenerate",
        "uic-solid-refused-snap-move",
        "uic-solid-refused-snap-open",
        "uic-solid-refused-snap-new",
        "uic-solid-refused-snap-still",
    ] {
        let en = kadakas_l10n::with_language("en-US", || tr!(key));
        let et = kadakas_l10n::with_language("et", || tr!(key));
        assert_ne!(en, et, "{key}");
        assert!(!et.contains("{ "), "{key}");
    }
}
