//! Intersect Faces in the window (MOD-08): Entity Info shows the places that are not intersected,
//! the context menu of the 3D view runs the command, and the status line shows its note.

use super::*;
use kadakas_core::model::ObjectId;
use kadakas_core::surface::{Edit, IntersectScope};
use kadakas_ui_core::modeling::Item;

type P = [f64; 3];

/// The 12 edges of a box.
fn cuboid(lo: P, hi: P) -> Vec<[P; 2]> {
    let c = |i: usize| -> P {
        [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ]
    };
    let mut out = Vec::new();
    for i in 0..8 {
        for bit in [1, 2, 4] {
            if i & bit == 0 {
                out.push([c(i), c(i | bit)]);
            }
        }
    }
    out
}

/// Two boxes that overlap, drawn into one surface model (the edges of each pass through the faces
/// of the other: accepted, as in a sketch).
fn two_boxes(app: &mut ModelerApp) -> ObjectId {
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
        app.session
            .editor
            .execute(&Command::EditSurface {
                id,
                edit: Edit::AddEdges {
                    edges: cuboid(lo, hi),
                },
            })
            .unwrap();
    }
    id
}

/// The middle of the first text `text` that egui painted.
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
fn intersect_faces_from_the_context_menu_resolves_two_overlapping_boxes() {
    let (ctx, mut app, mut t) = top_view();
    let id = two_boxes(&mut app);
    assert_eq!(surface(&app).check().not_intersected(), 6);
    // Entity Info: the count and the hint.
    app.session.selection = vec![id];
    let _ = frame(&ctx, &mut app, vec![], t);
    t += 0.1;
    let shown = texts(&frame(&ctx, &mut app, vec![], t)).join("\n");
    assert!(
        shown.contains(&tr!("modeler-entity-not-intersected-value", count = 6)),
        "{shown}"
    );
    assert!(shown.contains(&tr!("modeler-entity-solid-open")));
    // The context menu of the 3D view: a click of the secondary button.
    let c = app.canvas_rect.unwrap().center();
    let none = egui::Modifiers::NONE;
    let right = egui::PointerButton::Secondary;
    t += 0.1;
    let _ = frame(&ctx, &mut app, vec![moved(c)], t);
    t += 0.05;
    let _ = frame(&ctx, &mut app, vec![button(c, right, true, none)], t);
    t += 0.05;
    let _ = frame(&ctx, &mut app, vec![button(c, right, false, none)], t);
    t += 0.05;
    let out = frame(&ctx, &mut app, vec![], t);
    let item = text_at(&out, &tr!("modeler-edit-intersect")).expect("the context menu is open");
    // Over Intersect Faces the submenu opens.
    let mut sub = None;
    for _ in 0..6 {
        t += 0.1;
        let out = frame(&ctx, &mut app, vec![moved(item)], t);
        sub = text_at(&out, &tr!("modeler-edit-intersect-model"));
        if sub.is_some() {
            break;
        }
    }
    let with_model = sub.expect("the submenu With Model");
    let rev = app.session.editor.revision();
    // Along the item to the submenu, then a click.
    for k in 1..=4 {
        t += 0.05;
        let p = item + (with_model - item) * (k as f32 / 4.0);
        let _ = frame(&ctx, &mut app, vec![moved(p)], t);
    }
    let left = egui::PointerButton::Primary;
    t += 0.05;
    let _ = frame(
        &ctx,
        &mut app,
        vec![button(with_model, left, true, none)],
        t,
    );
    t += 0.05;
    let _ = frame(
        &ctx,
        &mut app,
        vec![button(with_model, left, false, none)],
        t,
    );
    t += 0.05;
    let out = frame(&ctx, &mut app, vec![], t);
    assert_ne!(app.session.editor.revision(), rev, "{}", app.status);
    // No place is left that is not intersected; one undo step; the note in the status line.
    let r = surface(&app).check();
    assert!(r.is_valid(), "{:?}", r.problems);
    assert_eq!(r.not_intersected(), 0, "{:?}", r.warnings);
    assert_eq!(surface(&app).face_count(), 18);
    assert!(app.status.contains("Intersect Faces"), "{}", app.status);
    assert!(texts(&out).join("\n").contains("Intersect Faces"));
    app.run(Cmd::Undo, size(&app));
    assert_eq!(surface(&app).check().not_intersected(), 6);
}

#[test]
fn intersect_faces_with_selection_works_on_the_selected_faces() {
    let (_ctx, mut app, _t) = top_view();
    let id = two_boxes(&mut app);
    assert!(!app.can_intersect(IntersectScope::Selection));
    assert!(
        app.can_intersect(IntersectScope::Model),
        "the models of the level"
    );
    // The top of the first box: the second box passes through it.
    let top = surface(&app)
        .faces()
        .find(|(_, f)| f.plane.normal.z.abs() > 0.99 && f.plane.offset.abs() == 1000.0)
        .map(|(f, _)| f)
        .unwrap();
    app.edit.sub.set([Item::Face(id, top)]);
    assert!(app.can_intersect(IntersectScope::Selection));
    app.run(Cmd::IntersectFaces(IntersectScope::Selection), size(&app));
    let r = surface(&app).check();
    assert!(r.is_valid(), "{:?}", r.problems);
    // The corner edge of the second box through that top and the two edges of the top through
    // the walls of the second box are split; the three places under the top stay.
    assert_eq!(r.not_intersected(), 3, "{:?}", r.warnings);
    // Only the selected top is cut (an L of new edges from border to border); the walls of the
    // second box only get the ends of the new edges.
    assert_eq!(surface(&app).face_count(), 13);
    let tops = surface(&app)
        .faces()
        .filter(|(_, f)| f.plane.normal.z.abs() > 0.99 && f.plane.offset.abs() == 1000.0)
        .count();
    assert_eq!(tops, 2);
    assert!(
        app.edit.sub.is_empty(),
        "the parts were split: the selection is cleared"
    );
    // With nothing selected, With Selection says what to do.
    app.run(Cmd::IntersectFaces(IntersectScope::Selection), size(&app));
    assert_eq!(app.status, tr!("modeler-intersect-nothing"));
    // With Model on the models of the level resolves the rest.
    app.run(Cmd::IntersectFaces(IntersectScope::Model), size(&app));
    assert_eq!(surface(&app).check().not_intersected(), 0);
}

#[test]
fn the_intersect_strings_are_in_both_languages() {
    for key in [
        "modeler-edit-intersect",
        "modeler-edit-intersect-model",
        "modeler-edit-intersect-selection",
        "modeler-intersect-nothing",
        "modeler-entity-not-intersected",
    ] {
        let en = kadakas_l10n::with_language("en-US", || tr!(key));
        let et = kadakas_l10n::with_language("et", || tr!(key));
        assert_ne!(en, et, "{key}");
    }
    let et = kadakas_l10n::with_language("et", || tr!("modeler-edit-intersect"));
    assert_eq!(et, "Lõika tahud");
}
