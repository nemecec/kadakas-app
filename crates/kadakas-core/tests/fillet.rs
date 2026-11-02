//! Fillet and chamfer in the document (MOD-21): one undo step each, undo and redo byte for byte,
//! the commands read from JSON as an agent sends them, the solid tools take the result, and the
//! scene draws it.

use kadakas_core::command::{Command, Editor};
use kadakas_core::mesh;
use kadakas_core::model::*;
use kadakas_core::surface::{EdgeChoice, Edit, SolidOp, SurfaceError, SurfaceModel};

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

fn model(ed: &Editor, id: ObjectId) -> &SurfaceModel {
    match &ed.doc().object(id).unwrap().shape {
        Shape::SurfaceModel(m) => m,
        other => panic!("not a surface model: {}", other.kind()),
    }
}

fn edit(ed: &mut Editor, id: ObjectId, e: Edit) {
    ed.execute(&Command::EditSurface { id, edit: e }).unwrap();
}

/// A box of `x` by `y` with its floor corner at `at`, `z` high.
fn draw_box(ed: &mut Editor, id: ObjectId, at: [f64; 3], x: f64, y: f64, z: f64) {
    let p = [
        [at[0], at[1], at[2]],
        [at[0] + x, at[1], at[2]],
        [at[0] + x, at[1] + y, at[2]],
        [at[0], at[1] + y, at[2]],
    ];
    edit(
        ed,
        id,
        Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        },
    );
    let m = model(ed, id);
    let f = m
        .faces()
        .find(|(_, f)| {
            f.outer
                .iter()
                .all(|v| m.vertex(*v).unwrap().z == at[2] && m.vertex(*v).unwrap().x >= at[0])
        })
        .unwrap()
        .0;
    edit(
        ed,
        id,
        Edit::PushPull {
            face: f,
            distance: z,
        },
    );
}

fn shape_text(ed: &Editor, id: ObjectId) -> String {
    serde_json::to_string(&ed.doc().object(id).unwrap().shape).unwrap()
}

#[test]
fn a_fillet_is_one_undo_step_and_undo_and_redo_are_exact() {
    let mut ed = Editor::with_sequential_ids();
    let id = add_model(&mut ed);
    draw_box(&mut ed, id, [0.0, 0.0, 0.0], 1000.0, 600.0, 400.0);
    let before = shape_text(&ed, id);
    let depth = ed.undo_depth();
    let all = model(&ed, id)
        .fillet_edges(&EdgeChoice {
            all: true,
            ..EdgeChoice::default()
        })
        .unwrap();
    edit(
        &mut ed,
        id,
        Edit::Fillet {
            edges: all.clone(),
            radius: 50.0,
            segments: None,
            smooth: true,
        },
    );
    assert_eq!(ed.undo_depth(), depth + 1);
    let after = shape_text(&ed, id);
    assert!(model(&ed, id).solid_report().is_closed());
    assert!(ed.undo());
    assert_eq!(shape_text(&ed, id), before);
    assert!(ed.redo());
    assert_eq!(shape_text(&ed, id), after);
    // The scene draws the round, smooth across the strips.
    let s = mesh::build(ed.doc());
    assert!(s.mesh.positions.len() > 100);
    // A refused fillet is no step and changes nothing.
    let depth = ed.undo_depth();
    assert!(ed.undo());
    let err = ed
        .execute(&Command::EditSurface {
            id,
            edit: Edit::Fillet {
                edges: all,
                radius: 450.0,
                segments: None,
                smooth: true,
            },
        })
        .unwrap_err();
    assert!(err.to_string().contains("at most about"), "{err}");
    assert_eq!(ed.undo_depth(), depth - 1);
    assert_eq!(shape_text(&ed, id), before);
}

#[test]
fn the_commands_read_from_json_as_an_agent_sends_them() {
    let mut ed = Editor::with_sequential_ids();
    let id = add_model(&mut ed);
    draw_box(&mut ed, id, [0.0, 0.0, 0.0], 1000.0, 600.0, 400.0);
    let e = model(&ed, id)
        .fillet_edges(&EdgeChoice {
            all: true,
            ..EdgeChoice::default()
        })
        .unwrap()[0];
    for edit in [
        serde_json::json!({"action": "fillet", "edges": [e.0], "radius": 20.0}),
        serde_json::json!({"action": "fillet", "edges": [e.0], "radius": 20.0, "segments": 3, "smooth": false}),
        serde_json::json!({"action": "chamfer", "edges": [e.0], "distance": 20.0}),
    ] {
        let c: Command = serde_json::from_value(
            serde_json::json!({"op": "edit_surface", "id": id.0, "edit": edit}),
        )
        .unwrap();
        ed.execute(&c).unwrap();
        assert!(model(&ed, id).solid_report().is_closed());
        assert!(ed.undo());
    }
}

#[test]
fn the_solid_tools_take_a_rounded_box() {
    let mut ed = Editor::with_sequential_ids();
    let id = add_model(&mut ed);
    draw_box(&mut ed, id, [0.0, 0.0, 0.0], 1000.0, 600.0, 400.0);
    let all = model(&ed, id)
        .fillet_edges(&EdgeChoice {
            all: true,
            ..EdgeChoice::default()
        })
        .unwrap();
    edit(
        &mut ed,
        id,
        Edit::Fillet {
            edges: all,
            radius: 60.0,
            segments: Some(4),
            smooth: true,
        },
    );
    // A second box through the rounded one, then Subtract.
    draw_box(&mut ed, id, [800.0, 200.0, 100.0], 500.0, 200.0, 400.0);
    let m = model(&ed, id).clone();
    let first_face = m
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.99 && f.plane.offset == 400.0)
        .unwrap()
        .0;
    let second_face = m
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.99 && f.plane.offset == 500.0)
        .unwrap()
        .0;
    let first = m.solid_of(first_face).unwrap();
    let second = m.solid_of(second_face).unwrap();
    let res = ed.execute(&Command::EditSurface {
        id,
        edit: Edit::Solid {
            op: SolidOp::Subtract,
            first,
            second,
            snap_near: false,
        },
    });
    res.unwrap();
    assert!(model(&ed, id).solid_report().is_closed());
}

#[test]
fn a_refusal_names_its_stable_id() {
    let mut m = SurfaceModel::new();
    let err = m
        .apply(&Edit::Chamfer {
            edges: vec![],
            distance: 5.0,
        })
        .unwrap_err();
    let SurfaceError::Fillet(f) = err else {
        panic!("{err}");
    };
    assert_eq!(f.code(), "fillet_no_edges");
}
