//! Tests of the solid tools in the modeler (MOD-09): the operands of a selection, the preview and
//! the refusals in two languages, the step that runs an operation, and Check Solid.

use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Attributes, ObjectId, Shape};
use kadakas_core::surface::{Edit, FaceId, ShellState, SolidOp, SurfaceModel};
use kadakas_l10n::{tr, with_language};

use super::super::select::{Item, SubSelection};
use super::super::solid::{self, Problem};
use super::super::{model_of, run_actions};

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

fn add_model(ed: &mut Editor) -> ObjectId {
    let out = ed
        .execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    ObjectId(out.created[0])
}

fn edit(ed: &mut Editor, id: ObjectId, e: Edit) {
    ed.execute(&Command::EditSurface { id, edit: e }).unwrap();
}

/// Two boxes that cross, as two solids in one model.
fn two_boxes() -> (Editor, ObjectId, Vec<FaceId>, Vec<FaceId>) {
    let mut ed = Editor::with_sequential_ids();
    let id = add_model(&mut ed);
    for (lo, hi) in [([0.0; 3], [1000.0; 3]), ([500.0; 3], [1500.0; 3])] {
        edit(
            &mut ed,
            id,
            Edit::AddEdges {
                edges: cuboid(lo, hi),
            },
        );
    }
    let inward: Vec<FaceId> = model_of(ed.doc(), id)
        .unwrap()
        .solid_report()
        .shells
        .iter()
        .filter(|s| s.state == ShellState::Inverted)
        .flat_map(|s| s.faces.clone())
        .collect();
    edit(&mut ed, id, Edit::ReverseFaces { faces: inward });
    let r = model_of(ed.doc(), id).unwrap().solid_report();
    let s: Vec<_> = r.solids().collect();
    assert_eq!(s.len(), 2);
    let (a, b) = (s[0].faces.clone(), s[1].faces.clone());
    (ed, id, a, b)
}

fn select(id: ObjectId, faces: &[FaceId]) -> SubSelection {
    let mut s = SubSelection::default();
    s.set(faces.iter().map(|f| Item::Face(id, *f)));
    s
}

fn model(ed: &Editor, id: ObjectId) -> &SurfaceModel {
    model_of(ed.doc(), id).unwrap()
}

#[test]
fn the_operands_are_the_two_solids_of_the_selected_faces() {
    let (ed, id, a, b) = two_boxes();
    let doc = ed.doc();
    // Nothing selected.
    assert_eq!(
        solid::operands(doc, &SubSelection::default(), false),
        Err(Problem::NoSelection)
    );
    // One face of each solid: the older solid (lower face id) is the first one.
    let sel = select(id, &[b[1], a[2]]);
    let o = solid::operands(doc, &sel, false).unwrap();
    assert_eq!((&o.first, &o.second), (&a, &b));
    // Swapped.
    let o = solid::operands(doc, &sel, true).unwrap();
    assert_eq!((&o.first, &o.second), (&b, &a));
    // All the faces of both solids name the same two solids.
    let all: Vec<FaceId> = a.iter().chain(b.iter()).copied().collect();
    assert_eq!(
        solid::operands(doc, &select(id, &all), false)
            .unwrap()
            .first,
        a
    );
    // One solid, and no solids at all (an edge is no face).
    assert_eq!(
        solid::operands(doc, &select(id, &a), false),
        Err(Problem::Count(1))
    );
    let mut edges = SubSelection::default();
    edges.set([Item::Edge(id, kadakas_core::surface::EdgeId(3))]);
    assert_eq!(
        solid::operands(doc, &edges, false),
        Err(Problem::NoSelection)
    );
}

#[test]
fn faces_of_two_models_are_refused_and_every_problem_has_a_text() {
    let (mut ed, id, a, b) = two_boxes();
    let other = add_model(&mut ed);
    edit(
        &mut ed,
        other,
        Edit::AddEdges {
            edges: cuboid([0.0; 3], [100.0; 3]),
        },
    );
    let f = model(&ed, other).faces().next().unwrap().0;
    let mut sel = select(id, &[a[0], b[0]]);
    sel.add([Item::Face(other, f)]);
    let p = solid::operands(ed.doc(), &sel, false).unwrap_err();
    assert_eq!(p, Problem::Models);
    for p in [
        Problem::NoSelection,
        Problem::Models,
        Problem::Count(3),
        Problem::Refused("x".into()),
    ] {
        assert!(!p.text().is_empty());
    }
    assert_eq!(
        Problem::Count(3).text(),
        tr!("uic-solid-need-two", found = 3)
    );
    assert_eq!(Problem::Refused("x".into()).text(), "x");
}

#[test]
fn the_preview_names_the_operation_and_the_volume() {
    let (ed, id, a, b) = two_boxes();
    let doc = ed.doc();
    let sel = select(id, &[a[0], b[0]]);
    let ops = solid::operands(doc, &sel, false).unwrap();
    let union = solid::preview(doc, &ops, SolidOp::Union).unwrap();
    assert_eq!(union, tr!("uic-solid-done-union", volume = "1.875"));
    let sub = solid::preview(doc, &ops, SolidOp::Subtract).unwrap();
    assert_eq!(sub, tr!("uic-solid-done-subtract", volume = "0.875"));
    assert!(
        solid::preview(doc, &ops, SolidOp::Intersect)
            .unwrap()
            .contains("0.125")
    );
    assert!(
        solid::preview(doc, &ops, SolidOp::Trim)
            .unwrap()
            .contains("0.875")
    );
    let split = solid::preview(doc, &ops, SolidOp::Split).unwrap();
    assert!(
        split.contains("0.875") && split.contains("0.125"),
        "{split}"
    );
    // The same in Estonian.
    with_language("et", || {
        let t = solid::preview_text(doc, &sel, false, SolidOp::Union);
        assert!(t.starts_with("Ühend:"), "{t}");
        assert_eq!(solid::op_name(SolidOp::Split), "Lõhestamine");
    });
    for op in SolidOp::ALL {
        assert!(!solid::op_name(op).is_empty());
        assert!(!solid::preview_text(doc, &sel, false, op).is_empty());
    }
    // The preview text of a selection that names no two solids is the problem.
    assert_eq!(
        solid::preview_text(doc, &select(id, &a), false, SolidOp::Union),
        Problem::Count(1).text()
    );
}

#[test]
fn an_operation_is_one_undo_step_with_a_message() {
    let (mut ed, id, a, b) = two_boxes();
    let sel = select(id, &[a[0], b[0]]);
    let out = solid::out(ed.doc(), &sel, false, SolidOp::Union).unwrap();
    assert_eq!(out.actions.len(), 1);
    assert_eq!(
        out.message,
        Some(tr!("uic-solid-done-union", volume = "1.875"))
    );
    let before = model(&ed, id).clone();
    let depth = ed.undo_depth();
    run_actions(&mut ed, &out.actions, false).unwrap();
    assert_eq!(ed.undo_depth(), depth + 1);
    assert_eq!(model(&ed, id).solid_report().solids().count(), 1);
    assert!(ed.undo());
    assert_eq!(model(&ed, id), &before);
    // No selection: no step.
    assert_eq!(
        solid::out(ed.doc(), &SubSelection::default(), false, SolidOp::Union),
        Err(Problem::NoSelection)
    );
}

#[test]
fn a_refusal_is_in_the_language_of_the_window_without_ids() {
    let (mut ed, id, a, b) = two_boxes();
    // An open first solid: erase one face of the first box.
    edit(&mut ed, id, Edit::EraseFaces { faces: vec![a[4]] });
    let sel = select(id, &[a[0], b[0]]);
    let err = solid::out(ed.doc(), &sel, false, SolidOp::Union).unwrap_err();
    let Problem::Refused(text) = &err else {
        panic!("{err:?}")
    };
    assert_eq!(text, &tr!("uic-solid-refused-open", which = "first"));
    assert!(!text.chars().any(|c| c.is_ascii_digit()), "{text}");
    with_language("et", || {
        let err = solid::out(ed.doc(), &sel, false, SolidOp::Union).unwrap_err();
        assert!(
            err.text().starts_with("Esimene tahkkeha ei ole suletud"),
            "{}",
            err.text()
        );
    });
    // The step itself says the same through the editor.
    let ops = kadakas_core::surface::Edit::Solid {
        op: SolidOp::Union,
        first: a.iter().copied().filter(|f| *f != a[4]).collect(),
        second: b.clone(),
    };
    let e = ed
        .execute(&Command::EditSurface { id, edit: ops })
        .unwrap_err();
    assert!(e.to_string().contains("is not a closed solid"), "{e}");
}

#[test]
fn check_solid_says_closed_or_selects_the_open_edges() {
    let (mut ed, id, a, _) = two_boxes();
    let none = SubSelection::default();
    let c = solid::check(ed.doc(), &none, &[id]);
    assert!(c.closed && c.open.is_empty());
    assert_eq!(
        c.text,
        tr!("uic-solid-check-closed", solids = 2, volume = "2.000")
    );
    // From the selected faces too.
    let c = solid::check(ed.doc(), &select(id, &a[..1]), &[]);
    assert!(c.closed);
    // Nothing selected, or no model.
    let c = solid::check(ed.doc(), &none, &[]);
    assert!(!c.closed);
    assert_eq!(c.text, tr!("uic-solid-check-nothing"));
    // An erased face opens four edges; they come back as items to select.
    edit(&mut ed, id, Edit::EraseFaces { faces: vec![a[4]] });
    let c = solid::check(ed.doc(), &none, &[id]);
    assert!(!c.closed);
    assert_eq!(c.open.len(), 4);
    assert!(
        c.open
            .iter()
            .all(|i| matches!(i, Item::Edge(m, _) if *m == id))
    );
    assert_eq!(c.text, tr!("uic-solid-check-open", edges = 4, solids = 1));
    // A loose edge counts too.
    edit(
        &mut ed,
        id,
        Edit::AddEdges {
            edges: vec![[[5000.0, 0.0, 0.0], [6000.0, 0.0, 0.0]]],
        },
    );
    assert_eq!(solid::check(ed.doc(), &none, &[id]).open.len(), 5);
    // A shell that looks inward is odd, not open.
    let (ed2, id2, a2, _) = two_boxes();
    let mut ed2 = ed2;
    edit(&mut ed2, id2, Edit::ReverseFaces { faces: a2.clone() });
    let c = solid::check(ed2.doc(), &none, &[id2]);
    assert!(!c.closed && c.open.is_empty());
    assert_eq!(c.text, tr!("uic-solid-check-odd", count = 1, solids = 1));
    with_language("et", || {
        assert!(
            solid::check(ed2.doc(), &none, &[id2])
                .text
                .starts_with("Kõik ei ole suletud")
        );
    });
}
