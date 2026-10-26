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
    assert_eq!(
        err,
        CommandError::Surface(kadakas_core::surface::SurfaceError::NoEdge(
            kadakas_core::surface::EdgeId(999)
        ))
    );
    assert_eq!(err.to_string(), "invalid value: edge 999 does not exist");
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
    let err = Editor::with_sequential_ids()
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
fn a_merge_that_drops_a_material_says_so_in_the_result() {
    // Two halves of a square, the right one with a material.
    let mut m = SurfaceModel::new();
    m.apply(&square(1000.0)).unwrap();
    let line = Edit::AddEdges {
        edges: vec![[[500.0, 0.0, 0.0], [500.0, 1000.0, 0.0]]],
    };
    m.apply(&line).unwrap();
    let mut v = serde_json::to_value(&m).unwrap();
    v["faces"][1]["attrs"] = serde_json::json!({"material": "brick"});
    let m: SurfaceModel = serde_json::from_value(v).unwrap();
    let edge = m
        .edges()
        .find(|(_, e)| {
            e.v.iter()
                .all(|x| (m.vertex(*x).unwrap().x - 500.0).abs() < 1e-9)
        })
        .unwrap()
        .0;
    let mut ed = Editor::with_sequential_ids();
    let id = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::new(m)),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let out = ed
        .execute(&Command::EditSurface {
            id,
            edit: Edit::EraseEdges { edges: vec![edge] },
        })
        .unwrap();
    assert_eq!(model(&ed, id).face_count(), 1);
    assert_eq!(out.notes.len(), 1, "{:?}", out.notes);
    assert!(out.notes[0].contains("material"), "{:?}", out.notes);
    // The notes are in the JSON of the result; an edit without notes has none.
    assert!(serde_json::to_value(&out).unwrap()["notes"].is_array());
    let out = ed
        .execute(&Command::EditSurface { id, edit: line })
        .unwrap();
    assert!(serde_json::to_value(&out).unwrap().get("notes").is_none());
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
    objects[0]["shape"]["v"] = serde_json::json!(3);
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

#[test]
fn push_pull_and_add_curve_are_one_undo_step_each() {
    let mut ed = Editor::with_sequential_ids();
    let id = add_model(&mut ed);
    let depth = ed.undo_depth();
    edit(&mut ed, id, square(1000.0)).unwrap();
    let face = model(&ed, id).faces().next().unwrap().0;
    edit(
        &mut ed,
        id,
        Edit::PushPull {
            face,
            distance: 400.0,
        },
    )
    .unwrap();
    assert_eq!(ed.undo_depth(), depth + 2);
    let boxed = model(&ed, id).clone();
    assert_eq!(
        (boxed.vertex_count(), boxed.edge_count(), boxed.face_count()),
        (8, 12, 6)
    );
    assert!(boxed.check().is_closed());
    // The JSON form of the command.
    let cmd: Command = serde_json::from_value(serde_json::json!({
        "op": "edit_surface", "id": id,
        "edit": {"action": "push_pull", "face": face.0, "distance": 100.0}
    }))
    .unwrap();
    ed.execute(&cmd).unwrap();
    assert_eq!(ed.undo_depth(), depth + 3);
    assert!(ed.undo());
    assert_eq!(model(&ed, id), &boxed);
    // A failed push changes nothing and adds no step.
    let top = boxed
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.9)
        .unwrap()
        .0;
    let steps = ed.undo_depth();
    assert!(
        edit(
            &mut ed,
            id,
            Edit::PushPull {
                face: top,
                distance: -400.0
            }
        )
        .is_err()
    );
    assert_eq!(ed.undo_depth(), steps);
    assert_eq!(model(&ed, id), &boxed);
    // A circle is a curve in one step.
    edit(
        &mut ed,
        id,
        Edit::AddCurve {
            points: (0..12)
                .map(|i| {
                    let a = std::f64::consts::TAU * f64::from(i) / 12.0;
                    [5000.0 + 300.0 * a.cos(), 300.0 * a.sin(), 0.0]
                })
                .collect(),
            closed: true,
            faceted: false,
            analytic: None,
        },
    )
    .unwrap();
    assert_eq!(ed.undo_depth(), steps + 1);
    assert_eq!(model(&ed, id).curves().count(), 1);
    assert!(ed.undo());
    assert_eq!(model(&ed, id), &boxed);
}

#[test]
fn smooth_edges_are_not_lines_and_the_faces_of_a_model_are_two_sided() {
    let mut ed = Editor::with_sequential_ids();
    let id = add_model(&mut ed);
    edit(
        &mut ed,
        id,
        Edit::AddCurve {
            points: (0..12)
                .map(|i| {
                    let a = std::f64::consts::TAU * f64::from(i) / 12.0;
                    [300.0 * a.cos(), 300.0 * a.sin(), 0.0]
                })
                .collect(),
            closed: true,
            faceted: false,
            analytic: None,
        },
    )
    .unwrap();
    let face = model(&ed, id).faces().next().unwrap().0;
    edit(
        &mut ed,
        id,
        Edit::PushPull {
            face,
            distance: 800.0,
        },
    )
    .unwrap();
    let scene = mesh::build(ed.doc());
    // 12 + 12 edges of the two circles; the 12 smooth edges of the side are not drawn.
    assert_eq!(scene.lines.segment_count(), 24);
    assert_eq!(model(&ed, id).edge_count(), 36);
    assert!(!scene.mesh.positions.is_empty());
    assert!((0..scene.mesh.positions.len()).all(|i| scene.mesh.is_two_sided(i)));
    // Other objects are one-sided.
    let mut ed2 = Editor::default();
    ed2.execute(&Command::AddObject {
        shape: Shape::Rect {
            min: DVec2::ZERO,
            max: DVec2::splat(100.0),
        },
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let s2 = mesh::build(ed2.doc());
    assert!((0..s2.mesh.positions.len()).all(|i| !s2.mesh.is_two_sided(i)));
}

#[test]
fn a_refused_surface_edit_keeps_the_reason_typed() {
    use kadakas_core::surface::{FaceId, SurfaceError};
    let mut ed = Editor::with_sequential_ids();
    let id = add_model(&mut ed);
    edit(&mut ed, id, square(100.0)).unwrap();
    let err = edit(
        &mut ed,
        id,
        Edit::EraseFaces {
            faces: vec![FaceId(999)],
        },
    )
    .unwrap_err();
    assert_eq!(
        err,
        CommandError::Surface(SurfaceError::NoFace(FaceId(999)))
    );
    // The text is the one the Invalid variant had.
    assert_eq!(err.to_string(), "invalid value: face 999 does not exist");
}

#[test]
fn analytic_curves_follow_each_command_and_undo_restores_them_exactly() {
    use kadakas_core::surface::Analytic;
    let mut ed = Editor::with_sequential_ids();
    let id = add_model(&mut ed);
    let ring = |r: f64, n: usize| -> Vec<[f64; 3]> {
        (0..n)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / n as f64;
                [r * a.cos(), r * a.sin(), 0.0]
            })
            .collect()
    };
    let mut states = vec![model(&ed, id).clone()];
    edit(
        &mut ed,
        id,
        Edit::AddCurve {
            points: ring(300.0, 12),
            closed: true,
            faceted: false,
            analytic: Some(Analytic::circle(DVec3::ZERO, DVec3::Z, DVec3::X, 300.0)),
        },
    )
    .unwrap();
    states.push(model(&ed, id).clone());
    let curve = model(&ed, id).curves().next().unwrap().0;
    assert!(model(&ed, id).curve_geometry(curve).is_some());
    // A new number of segments is one step.
    edit(
        &mut ed,
        id,
        Edit::SetCurveSegments {
            curve,
            segments: 40,
        },
    )
    .unwrap();
    assert_eq!(model(&ed, id).curve_segments(curve), Some(40));
    states.push(model(&ed, id).clone());
    // A non-uniform scale drops the field and says so in the result.
    let all: Vec<_> = model(&ed, id).vertices().map(|(v, _)| v).collect();
    let out = ed
        .execute(&Command::EditSurface {
            id,
            edit: Edit::TransformVertices {
                vertices: all,
                faces: Vec::new(),
                edges: Vec::new(),
                matrix: glam::DAffine3::from_scale(DVec3::new(2.0, 1.0, 1.0)).to_cols_array(),
            },
        })
        .unwrap();
    assert!(
        out.notes
            .iter()
            .any(|n| n.contains("lost its circle description"))
    );
    assert!(model(&ed, id).curve_geometry(curve).is_none());
    states.push(model(&ed, id).clone());
    // Undo goes back through every state, redo forward again.
    for want in states.iter().rev().skip(1) {
        assert!(ed.undo());
        assert_eq!(model(&ed, id), want);
    }
    for want in states.iter().skip(1) {
        assert!(ed.redo());
        assert_eq!(model(&ed, id), want);
    }
}

#[test]
fn exploding_a_group_with_a_member_scaled_unequally_says_which_circles_were_lost() {
    use kadakas_core::surface::Analytic;
    let mut ed = Editor::with_sequential_ids();
    let mut ids = Vec::new();
    for x in [0.0, 3000.0] {
        let id = add_model(&mut ed);
        let points = (0..12)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 12.0;
                [x + 300.0 * a.cos(), 300.0 * a.sin(), 0.0]
            })
            .collect();
        edit(
            &mut ed,
            id,
            Edit::AddCurve {
                points,
                closed: true,
                faceted: false,
                analytic: Some(Analytic::circle(
                    DVec3::new(x, 0.0, 0.0),
                    DVec3::Z,
                    DVec3::X,
                    300.0,
                )),
            },
        )
        .unwrap();
        ids.push(id);
    }
    // The second model is scaled on its own, so that its circle lands as an ellipse in the first.
    ed.execute(&Command::TransformObjects3d {
        ids: vec![ids[1]],
        matrix: glam::DAffine3::from_scale(DVec3::new(2.0, 1.0, 1.0)).to_cols_array(),
        copy: false,
        count: 1,
    })
    .unwrap();
    let g = ObjectId(
        ed.execute(&Command::MakeGroup {
            model: None,
            faces: vec![],
            edges: vec![],
            objects: ids.clone(),
            name: None,
        })
        .unwrap()
        .created[0],
    );
    let out = ed.execute(&Command::Explode { ids: vec![g] }).unwrap();
    let kept: usize = ed
        .doc()
        .objects
        .values()
        .filter_map(|o| match &o.shape {
            Shape::SurfaceModel(m) => Some(m.curve_geometries().count()),
            _ => None,
        })
        .sum();
    let lost = out
        .notes
        .iter()
        .filter(|n| n.contains("lost its circle description"))
        .count();
    // Every circle is either kept or named in the notes.
    assert_eq!(kept + lost, 2, "{kept} kept, notes {:?}", out.notes);
    assert!(lost >= 1, "{:?}", out.notes);
}
