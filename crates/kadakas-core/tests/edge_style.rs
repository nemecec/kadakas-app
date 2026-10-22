//! Soft and smooth edges in the document (MOD-08): one undo step each, byte for byte, and what
//! the 3D scene draws and shades.

use kadakas_core::command::{Command, Editor};
use kadakas_core::mesh;
use kadakas_core::model::*;
use kadakas_core::surface::{EdgeId, Edit, SurfaceModel};

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

/// A box 1000 x 1000 x 500.
fn boxed(ed: &mut Editor) -> ObjectId {
    let id = add_model(ed);
    let p = [
        [0.0, 0.0, 0.0],
        [1000.0, 0.0, 0.0],
        [1000.0, 1000.0, 0.0],
        [0.0, 1000.0, 0.0],
    ];
    edit(
        ed,
        id,
        Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        },
    );
    let f = model(ed, id).faces().next().unwrap().0;
    edit(
        ed,
        id,
        Edit::PushPull {
            face: f,
            distance: 500.0,
        },
    );
    id
}

/// The edge of the top at y = 0 (between the top and the front wall).
fn front_top_edge(m: &SurfaceModel) -> EdgeId {
    m.edges()
        .find(|(_, e)| {
            let (a, b) = (m.vertex(e.v[0]).unwrap(), m.vertex(e.v[1]).unwrap());
            a.y == 0.0 && b.y == 0.0 && a.z == 500.0 && b.z == 500.0
        })
        .unwrap()
        .0
}

/// The distinct normals of the scene at the corner (1000, 0, 500), rounded.
fn normals_at_corner(ed: &Editor) -> Vec<[i32; 3]> {
    let s = mesh::build(ed.doc());
    // The scene is relative to its origin: the corner is the one with the largest x and z and
    // the smallest y.
    let ps = &s.mesh.positions;
    let hx = ps.iter().map(|p| p.x).fold(f32::MIN, f32::max);
    let ly = ps.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    let hz = ps.iter().map(|p| p.z).fold(f32::MIN, f32::max);
    let mut out: Vec<[i32; 3]> = ps
        .iter()
        .zip(&s.mesh.normals)
        .filter(|(p, _)| {
            (p.x - hx).abs() < 1e-3 && (p.y - ly).abs() < 1e-3 && (p.z - hz).abs() < 1e-3
        })
        .map(|(_, n)| {
            [
                (n.x * 100.0) as i32,
                (n.y * 100.0) as i32,
                (n.z * 100.0) as i32,
            ]
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

#[test]
fn soft_edges_are_one_undo_step_each_and_undo_byte_for_byte() {
    let mut ed = Editor::with_sequential_ids();
    let id = boxed(&mut ed);
    let before = serde_json::to_string(&ed.doc().object(id).unwrap().shape).unwrap();
    let depth = ed.undo_depth();
    let e = front_top_edge(model(&ed, id));
    edit(
        &mut ed,
        id,
        Edit::SetEdgeStyle {
            edges: vec![e],
            soft: Some(true),
            smooth: Some(true),
            hidden: None,
        },
    );
    assert_eq!(ed.undo_depth(), depth + 1);
    let all: Vec<EdgeId> = model(&ed, id).edges().map(|(e, _)| e).collect();
    edit(
        &mut ed,
        id,
        Edit::SoftenEdges {
            edges: all,
            angle: 100.0,
            smooth: true,
            coplanar: false,
        },
    );
    assert_eq!(ed.undo_depth(), depth + 2);
    assert!(ed.undo());
    assert!(ed.undo());
    let after = serde_json::to_string(&ed.doc().object(id).unwrap().shape).unwrap();
    assert_eq!(before, after);
    // The command reads from JSON as an agent sends it.
    let c: Command = serde_json::from_value(serde_json::json!({
        "op": "edit_surface", "id": id.0,
        "edit": {"action": "soften_edges", "edges": [e.0], "angle": 95.0}
    }))
    .unwrap();
    ed.execute(&c).unwrap();
    assert!(kadakas_core::surface::flagged(
        model(&ed, id).edge_attrs(e),
        kadakas_core::surface::ATTR_SMOOTH
    ));
}

#[test]
fn a_soft_edge_is_not_drawn_and_shades_round_a_smooth_one_is_drawn() {
    let mut ed = Editor::with_sequential_ids();
    let id = boxed(&mut ed);
    let lines = mesh::build(ed.doc()).lines.segment_count();
    assert_eq!(lines, 12);
    // A hard box: three normals at a corner.
    assert_eq!(normals_at_corner(&ed).len(), 3);
    let e = front_top_edge(model(&ed, id));
    // Smooth only: still drawn, the top and the front shade as one at the corner.
    edit(
        &mut ed,
        id,
        Edit::SetEdgeStyle {
            edges: vec![e],
            soft: None,
            smooth: Some(true),
            hidden: None,
        },
    );
    assert_eq!(mesh::build(ed.doc()).lines.segment_count(), 12);
    let n = normals_at_corner(&ed);
    assert_eq!(n.len(), 2, "{n:?}");
    // One normal between the top (+z) and the front (-y).
    assert!(n.iter().any(|v| v[0] == 0 && v[1] < 0 && v[2] > 0), "{n:?}");
    // Soft: not drawn.
    edit(
        &mut ed,
        id,
        Edit::SetEdgeStyle {
            edges: vec![e],
            soft: Some(true),
            smooth: None,
            hidden: None,
        },
    );
    assert_eq!(mesh::build(ed.doc()).lines.segment_count(), 11);
    // Soft without smooth: not drawn and still a crease in the shading.
    edit(
        &mut ed,
        id,
        Edit::SetEdgeStyle {
            edges: vec![e],
            soft: None,
            smooth: Some(false),
            hidden: None,
        },
    );
    assert_eq!(
        normals_at_corner(&ed).len(),
        3,
        "soft alone keeps the crease"
    );
}

#[test]
fn every_edit_of_the_second_part_is_one_step_and_undoes_byte_for_byte() {
    let mut ed = Editor::with_sequential_ids();
    let id = boxed(&mut ed);
    // A square on the top for the offset and the hole.
    let p = [
        [250.0, 250.0, 500.0],
        [750.0, 250.0, 500.0],
        [750.0, 750.0, 500.0],
        [250.0, 750.0, 500.0],
    ];
    edit(
        &mut ed,
        id,
        Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        },
    );
    let inner = |ed: &Editor| {
        let m = model(ed, id);
        m.faces()
            .find(|(f, g)| {
                g.plane.normal.z > 0.9 && g.holes.is_empty() && m.face_area(*f).unwrap() < 300_000.0
            })
            .unwrap()
            .0
    };
    let line = |ed: &Editor| -> Vec<EdgeId> {
        let m = model(ed, id);
        m.edges()
            .filter(|(_, e)| {
                let (a, b) = (m.vertex(e.v[0]).unwrap(), m.vertex(e.v[1]).unwrap());
                a.z == 0.0 && b.z == 0.0 && a.y == 0.0 && b.y == 0.0
            })
            .map(|(e, _)| e)
            .collect()
    };
    type Make<'a> = Box<dyn Fn(&Editor) -> Edit + 'a>;
    let cases: Vec<Make<'_>> = vec![
        Box::new(move |ed| Edit::OffsetFace {
            face: inner(ed),
            distance: 50.0,
            outward: false,
        }),
        Box::new(move |ed| Edit::PushPull {
            face: inner(ed),
            distance: -500.0,
        }),
        Box::new(move |ed| Edit::OffsetEdges {
            edges: line(ed),
            distance: 100.0,
            toward: [500.0, 500.0, 0.0],
        }),
        Box::new(move |ed| {
            // The square on the top along the front edge of the bottom: a sweep.
            Edit::FollowMe {
                face: inner(ed),
                path: line(ed),
            }
        }),
    ];
    for (i, make) in cases.iter().enumerate() {
        let before = serde_json::to_string(&ed.doc().object(id).unwrap().shape).unwrap();
        let depth = ed.undo_depth();
        let e = make(&ed);
        let r = ed.execute(&Command::EditSurface { id, edit: e });
        if r.is_err() {
            // A refused edit changes nothing.
            assert_eq!(ed.undo_depth(), depth, "{i}");
            continue;
        }
        assert_eq!(ed.undo_depth(), depth + 1, "{i}");
        assert!(ed.undo());
        let after = serde_json::to_string(&ed.doc().object(id).unwrap().shape).unwrap();
        assert_eq!(before, after, "{i}: undo gives the same text");
        assert!(ed.redo());
        assert!(ed.undo());
    }
}
