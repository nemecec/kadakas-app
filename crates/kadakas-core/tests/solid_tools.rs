//! Solid tools (MOD-09) in the document: one edit, one undo step, and the refusals.

use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::model::*;
use kadakas_core::surface::{
    Edit, FaceId, ShellState, SolidError, SolidOp, SurfaceError, SurfaceModel,
};

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
                snap_near: false,
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
            snap_near: false,
        },
    )
    .unwrap_err();
    // The refusal is typed: the API gives a stable id without running the edit again.
    let CommandError::Surface(SurfaceError::Solid(SolidError::Open { which, .. })) = &err else {
        panic!("{err:?}")
    };
    assert_eq!(which.to_string(), "first");
    let text = err.to_string();
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

/// The document of a refused near-touch (MOD-19): the box `[0, 1000]^3` and a turned, tilted box
/// whose corner lies 0.0058 mm off the plane `y = 1000`, inside its face.
fn near_touch() -> (Editor, ObjectId, Vec<FaceId>, Vec<FaceId>) {
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
    let (angle, tilt, d): (f64, f64, [f64; 3]) = (
        1.539_623_148_363_857_6,
        0.394_573_062_789_783_6,
        [200.0, 796.195_805_697_005_2, 200.0],
    );
    let (sin, cos) = angle.sin_cos();
    let (st, ct) = tilt.sin_cos();
    let corners: Vec<[f64; 3]> = (0..8usize)
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
    edit(&mut ed, id, Edit::AddEdges { edges }).unwrap();
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
fn a_snap_is_one_undo_step_together_with_the_operation() {
    for op in SolidOp::ALL {
        let (mut ed, id, a, b) = near_touch();
        let before = model(&ed, id).clone();
        let depth = ed.undo_depth();
        let solid = |snap_near| Edit::Solid {
            op,
            first: a.clone(),
            second: b.clone(),
            snap_near,
        };
        // Without the flag: the refusal names the snap, nothing changes, no undo step.
        let err = edit(&mut ed, id, solid(false)).unwrap_err();
        assert!(
            matches!(
                err,
                CommandError::Surface(SurfaceError::Solid(SolidError::NearTouch { .. }))
            ),
            "{op}: {err:?}"
        );
        assert_eq!(ed.undo_depth(), depth, "{op}");
        assert_eq!(model(&ed, id), &before, "{op}");
        // With the flag: one step, the moved vertex is in the note.
        let out = ed
            .execute(&Command::EditSurface {
                id,
                edit: solid(true),
            })
            .unwrap_or_else(|e| panic!("{op}: {e}"));
        assert!(
            out.notes.iter().any(|n| n.contains("snapped vertex ")),
            "{op}: {:?}",
            out.notes
        );
        assert_eq!(ed.undo_depth(), depth + 1, "{op}");
        assert!(model(&ed, id).problem().is_none(), "{op}");
        // One undo brings back the model with the vertex where it was, and redo is the same.
        let after = model(&ed, id).clone();
        assert!(ed.undo(), "{op}");
        assert_eq!(model(&ed, id), &before, "{op}");
        assert!(ed.redo(), "{op}");
        assert_eq!(model(&ed, id), &after, "{op}");
    }
}

#[test]
fn the_flag_is_plain_json_and_is_left_out_when_false() {
    let e = Edit::Solid {
        op: SolidOp::Union,
        first: vec![FaceId(1)],
        second: vec![FaceId(2)],
        snap_near: false,
    };
    let v = serde_json::to_value(&e).unwrap();
    assert!(v.get("snap_near").is_none(), "{v}");
    let on = serde_json::json!({"action": "solid", "op": "union", "first": [1], "second": [2], "snap_near": true});
    let Edit::Solid { snap_near, .. } = serde_json::from_value(on).unwrap() else {
        panic!()
    };
    assert!(snap_near);
}
