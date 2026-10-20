//! Surface models (MOD-06) in the document: the edit command, undo and redo, the file, a newer
//! format, the plan (SVG), snapping and the 3D scene.

use glam::{DVec2, DVec3};
use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::mesh;
use kadakas_core::model::*;
use kadakas_core::snapping::Geometry;
use kadakas_core::surface::{Edit, SurfaceModel};
use kadakas_core::svg;

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

fn square(w: f64) -> Edit {
    let p = [[0.0, 0.0, 0.0], [w, 0.0, 0.0], [w, w, 0.0], [0.0, w, 0.0]];
    Edit::AddEdges {
        edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
    }
}

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

#[test]
fn each_edit_is_one_undo_step() {
    let mut ed = Editor::with_sequential_ids();
    let id = add_model(&mut ed);
    let depth = ed.undo_depth();
    edit(&mut ed, id, square(1000.0)).unwrap();
    assert_eq!(ed.undo_depth(), depth + 1);
    assert_eq!(model(&ed, id).face_count(), 1);
    edit(
        &mut ed,
        id,
        Edit::AddEdges {
            edges: vec![[[500.0, 0.0, 0.0], [500.0, 1000.0, 0.0]]],
        },
    )
    .unwrap();
    let split = model(&ed, id).clone();
    assert_eq!(split.face_count(), 2);
    assert!(ed.undo());
    assert_eq!(model(&ed, id).face_count(), 1);
    assert!(ed.undo());
    assert!(model(&ed, id).is_empty());
    assert!(ed.redo() && ed.redo());
    assert_eq!(model(&ed, id), &split);
    // A failed edit changes nothing and makes no step.
    let depth = ed.undo_depth();
    let before = ed.doc().clone();
    let err = edit(
        &mut ed,
        id,
        Edit::EraseEdges {
            edges: vec![kadakas_core::surface::EdgeId(999)],
        },
    )
    .unwrap_err();
    assert_eq!(err, CommandError::Invalid("edge 999 does not exist".into()));
    assert_eq!(ed.undo_depth(), depth);
    assert_eq!(ed.doc(), &before);
    // An edit that changes nothing makes no step either.
    edit(&mut ed, id, Edit::EraseFaces { faces: vec![] }).unwrap();
    assert_eq!(ed.undo_depth(), depth);
}

#[test]
fn the_command_checks_its_object() {
    let mut ed = Editor::with_sequential_ids();
    let line = ed
        .execute(&Command::AddObject {
            shape: Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0];
    assert_eq!(
        edit(&mut ed, ObjectId(line), square(10.0)),
        Err(CommandError::Invalid(format!(
            "object {line} is not a surface model"
        )))
    );
    let missing = ObjectId(kadakas_core::uid::Uid::from_num(4242));
    assert_eq!(
        edit(&mut ed, missing, square(10.0)),
        Err(CommandError::NoObject(missing))
    );
    let id = add_model(&mut ed);
    let layer = ed.doc().object(id).unwrap().layer;
    ed.execute(&Command::ModifyLayer {
        id: layer,
        name: None,
        visible: None,
        locked: Some(true),
        elevation: None,
        scale: None,
    })
    .unwrap();
    assert_eq!(
        edit(&mut ed, id, square(10.0)),
        Err(CommandError::LayerLocked(layer))
    );
    // A broken model is refused when it is added.
    let bad: SurfaceModel = serde_json::from_value(serde_json::json!({
        "v": 1, "next_id": 5, "vertices": [[1, 0, 0, 0]]
    }))
    .unwrap();
    let err = ed
        .execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::new(bad)),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap_err();
    assert!(matches!(err, CommandError::Invalid(m) if m.contains("has no edge")));
}

#[test]
fn the_command_reads_and_writes_as_json() {
    let cmd: Command = serde_json::from_value(serde_json::json!({
        "op": "edit_surface",
        "id": ObjectId(kadakas_core::uid::Uid::from_num(3)),
        "edit": {"action": "add_edges", "edges": [[[0, 0, 0], [1000, 0, 0]]]}
    }))
    .unwrap();
    let Command::EditSurface { edit, .. } = &cmd else {
        panic!()
    };
    assert_eq!(
        edit,
        &Edit::AddEdges {
            edges: vec![[[0.0; 3], [1000.0, 0.0, 0.0]]]
        }
    );
    let back: Command = serde_json::from_value(serde_json::to_value(&cmd).unwrap()).unwrap();
    assert_eq!(back, cmd);
}

#[test]
fn the_file_keeps_the_model() {
    let mut ed = Editor::with_sequential_ids();
    let id = add_model(&mut ed);
    edit(&mut ed, id, square(1000.0)).unwrap();
    ed.execute(&Command::MoveObjects {
        ids: vec![id],
        delta: DVec2::new(100.0, 200.0),
    })
    .unwrap();
    let mut text = Vec::new();
    ed.doc().write_pretty(&mut text).unwrap();
    let back = Document::from_slice(&text).unwrap();
    assert_eq!(&back, ed.doc());
    let Shape::SurfaceModel(m) = &back.object(id).unwrap().shape else {
        panic!()
    };
    assert_eq!(m.transform.translation, DVec3::new(100.0, 200.0, 0.0));
    assert_eq!(
        back.object(id).unwrap().shape.bounds(),
        (DVec2::new(100.0, 200.0), DVec2::new(1100.0, 1200.0))
    );
}

#[test]
fn a_newer_format_is_kept_as_it_is() {
    let mut ed = Editor::with_sequential_ids();
    let id = add_model(&mut ed);
    edit(&mut ed, id, square(1000.0)).unwrap();
    let mut v = serde_json::to_value(ed.doc()).unwrap();
    let objects = v["objects"].as_array_mut().unwrap();
    objects[0]["shape"]["v"] = serde_json::json!(2);
    objects[0]["shape"]["curves"] = serde_json::json!([{"edges": [5, 6]}]);
    let text = serde_json::to_vec(&v).unwrap();
    let doc = Document::from_slice(&text).unwrap();
    let shape = &doc.object(id).unwrap().shape;
    assert!(matches!(shape, Shape::Unknown { kind, .. } if kind == "surface_model"));
    let again = serde_json::to_value(&doc).unwrap();
    assert_eq!(again["objects"][0]["shape"], v["objects"][0]["shape"]);
    // It cannot be edited.
    let mut ed = Editor::new(doc);
    assert!(edit(&mut ed, id, square(10.0)).is_err());
}

#[test]
fn the_plan_snapping_and_the_3d_scene_show_the_model() {
    let mut ed = Editor::with_sequential_ids();
    let id = add_model(&mut ed);
    edit(&mut ed, id, square(1000.0)).unwrap();
    let out = svg::render(ed.doc(), 800);
    assert!(out.contains("data-kind=\"surface_model\""));
    let mut g = Geometry::default();
    g.add_shape(&ed.doc().object(id).unwrap().shape);
    assert!(g.points.iter().any(|p| p.p == DVec2::new(1000.0, 1000.0)));
    let scene = mesh::build(ed.doc());
    assert!(scene.mesh.positions.len() >= 4);
    // A mirrored model is still drawn, and an empty model draws nothing.
    let mut m = model(&ed, id).clone();
    m.transform = glam::DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0));
    ed.execute(&Command::ModifyObject {
        id,
        shape: Some(Shape::SurfaceModel(Box::new(m))),
        layer: None,
        class: None,
        attrs: None,
    })
    .unwrap();
    assert!(mesh::build(ed.doc()).mesh.positions.len() >= 4);
    let mut empty = Editor::with_sequential_ids();
    add_model(&mut empty);
    assert!(!svg::render(empty.doc(), 800).contains("data-kind=\"surface_model\""));
    assert!(mesh::build(empty.doc()).mesh.positions.is_empty());
    assert_eq!(Shape::SurfaceModel(Box::default()).kind(), "surface_model");
}
