//! Solid tools (MOD-09) in the document: one edit, one undo step, and the refusals.

use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::model::*;
use kadakas_core::surface::{Edit, FaceId, ShellState, SolidOp, SurfaceModel};

fn model(ed: &Editor, id: ObjectId) -> &SurfaceModel {
    match &ed.doc().object(id).unwrap().shape {
        Shape::SurfaceModel(m) => m,
        other => panic!("not a surface model: {}", other.kind()),
    }
}

fn edit(ed: &mut Editor, id: ObjectId, e: Edit) -> Result<(), CommandError> {
    ed.execute(&Command::EditSurface { id, edit: e })
        .map(|_| ())
}

fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> Edit {
    let c = |i: usize| -> [f64; 3] {
        std::array::from_fn(|k| if i >> k & 1 == 0 { lo[k] } else { hi[k] })
    };
    let mut edges = Vec::new();
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                edges.push([c(i), c(i | bit)]);
            }
        }
    }
    Edit::AddEdges { edges }
}

/// A document with two boxes that cross, as two solids.
fn two_boxes() -> (Editor, ObjectId, Vec<FaceId>, Vec<FaceId>) {
    let mut ed = Editor::with_sequential_ids();
    let out = ed
        .execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    let id = ObjectId(out.created[0]);
    edit(&mut ed, id, cuboid([0.0; 3], [1000.0; 3])).unwrap();
    edit(&mut ed, id, cuboid([500.0; 3], [1500.0; 3])).unwrap();
    // Boxes drawn from edges may look inward: turn those over.
    let inward: Vec<FaceId> = model(&ed, id)
        .solid_report()
        .shells
        .iter()
        .filter(|s| s.state == ShellState::Inverted)
        .flat_map(|s| s.faces.clone())
        .collect();
    edit(&mut ed, id, Edit::ReverseFaces { faces: inward }).unwrap();
    let report = model(&ed, id).solid_report();
    let solids: Vec<_> = report.solids().collect();
    assert_eq!(solids.len(), 2);
    let (a, b) = (solids[0].faces.clone(), solids[1].faces.clone());
    (ed, id, a, b)
}

#[test]
fn each_operation_is_one_undo_step_and_comes_back_with_undo() {
    for op in SolidOp::ALL {
        let (mut ed, id, a, b) = two_boxes();
        let before = model(&ed, id).clone();
        let depth = ed.undo_depth();
        edit(
            &mut ed,
            id,
            Edit::Solid {
                op,
                first: a.clone(),
                second: b.clone(),
            },
        )
        .unwrap_or_else(|e| panic!("{op}: {e}"));
        assert_eq!(ed.undo_depth(), depth + 1, "{op}");
        let after = model(&ed, id).clone();
        assert_ne!(after, before, "{op}");
        assert!(after.problem().is_none(), "{op}");
        assert!(ed.undo(), "{op}");
        assert_eq!(model(&ed, id), &before, "{op}");
        assert!(ed.redo(), "{op}");
        assert_eq!(model(&ed, id), &after, "{op}");
    }
}

#[test]
fn a_refused_operation_changes_nothing_and_makes_no_step() {
    let (mut ed, id, a, b) = two_boxes();
    let doc = ed.doc().clone();
    let depth = ed.undo_depth();
    let err = edit(
        &mut ed,
        id,
        Edit::Solid {
            op: SolidOp::Subtract,
            first: a[..3].to_vec(),
            second: b,
        },
    )
    .unwrap_err();
    let CommandError::Invalid(text) = err else {
        panic!("{err:?}")
    };
    assert!(
        text.contains("the first solid is not a closed solid"),
        "{text}"
    );
    assert_eq!(ed.undo_depth(), depth);
    assert_eq!(ed.doc(), &doc);
}

#[test]
fn the_edit_is_plain_json_for_agents() {
    let (mut ed, id, a, b) = two_boxes();
    let ids = |v: &[FaceId]| serde_json::to_value(v).unwrap();
    let cmd = serde_json::json!({
        "op": "edit_surface",
        "id": id.0,
        "edit": {"action": "solid", "op": "union", "first": ids(&a), "second": ids(&b)},
    });
    let cmd: Command = serde_json::from_value(cmd).unwrap();
    ed.execute(&cmd).unwrap();
    assert_eq!(model(&ed, id).solid_report().solids().count(), 1);
    // The report is JSON too.
    let report = serde_json::to_value(model(&ed, id).solid_report()).unwrap();
    assert_eq!(report["shells"][0]["state"], "solid");
    assert!(report["wire_edges"].as_array().unwrap().is_empty());
}
