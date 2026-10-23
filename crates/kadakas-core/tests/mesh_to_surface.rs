//! The command `mesh3d_to_surface` (MOD-12): a mesh object becomes a surface model, in place,
//! in one undo step, or the command is refused and nothing changes.

use glam::DVec3;
use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::mesh3d::Mesh3d;
use kadakas_core::model::*;
use kadakas_core::surface::{ConvertError, SurfaceModel};

fn cube_mesh() -> Mesh3d {
    let v: Vec<DVec3> = (0..8)
        .map(|i| {
            DVec3::new(
                5000.0 + (i & 1) as f64 * 1000.0,
                7000.0 + ((i >> 1) & 1) as f64 * 1000.0,
                300.0 + ((i >> 2) & 1) as f64 * 1000.0,
            )
        })
        .collect();
    let quads: [[u32; 4]; 6] = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    Mesh3d {
        name: "Block".into(),
        vertices: v,
        triangles: quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .collect(),
        color: Some(Color([200, 10, 10, 255])),
    }
}

fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
    let out = ed
        .execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    ObjectId(out.created[0])
}

fn run(id: ObjectId, extra: serde_json::Value) -> Command {
    let mut v = serde_json::json!({"op": "mesh3d_to_surface", "id": id.0});
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(v).unwrap()
}

#[test]
fn a_mesh_becomes_a_surface_model_and_undo_gives_it_back() {
    let mut ed = Editor::new(Document::default());
    let id = add(&mut ed, Shape::Mesh3d(Box::new(cube_mesh())));
    let before = ed.doc().clone();
    let out = ed.execute(&run(id, serde_json::json!({}))).unwrap();
    let report = out.report.expect("a report");
    assert_eq!(report["faces"], 6);
    assert_eq!(report["closed"], true);
    assert_eq!(report["softened_edges"], 0);
    let o = ed.doc().object(id).unwrap();
    let Shape::SurfaceModel(m) = &o.shape else {
        panic!("{}", o.shape.kind())
    };
    assert_eq!(m.name, "Block");
    assert_eq!(m.face_count(), 6);
    assert!(m.problem().is_none());
    // The color became the fill, and the placement is the center of the box.
    assert_eq!(o.attrs.fill, Some(Color([200, 10, 10, 255])));
    let (lo, hi) = m.bounds().unwrap();
    assert!(
        (lo - DVec3::new(5000.0, 7000.0, 300.0)).length() < 1e-6,
        "{lo}"
    );
    assert!((hi - DVec3::new(6000.0, 8000.0, 1300.0)).length() < 1e-6);
    assert!(out.notes.iter().any(|n| n.contains("fill")));
    // One undo step gives the mesh back, byte for byte.
    assert!(ed.undo());
    assert_eq!(ed.doc(), &before);
    assert!(ed.redo());
    assert!(matches!(
        ed.doc().object(id).unwrap().shape,
        Shape::SurfaceModel(_)
    ));
}

#[test]
fn the_layer_elevation_is_taken_off_the_heights() {
    let mut ed = Editor::new(Document::default());
    let id = add(&mut ed, Shape::Mesh3d(Box::new(cube_mesh())));
    let layer = ed.doc().object(id).unwrap().layer;
    ed.execute(
        &serde_json::from_value(serde_json::json!({
            "op": "modify_layer", "id": layer.0, "elevation": 100.0
        }))
        .unwrap(),
    )
    .unwrap();
    ed.execute(&run(id, serde_json::json!({"soften_below_angle": 100.0})))
        .unwrap();
    let Shape::SurfaceModel(m) = &ed.doc().object(id).unwrap().shape else {
        panic!()
    };
    // In the drawing, the height is measured from the layer: 300 - 100 = 200.
    let (lo, _) = m.bounds().unwrap();
    assert!((lo.z - 200.0).abs() < 1e-6, "{lo}");
    assert!(m.edges().all(|(e, _)| m.edge_soft(e)));
}

#[test]
fn a_refusal_changes_nothing() {
    let mut ed = Editor::new(Document::default());
    // A terrain mesh with 130,000 triangles is over the limits of a surface model.
    let n = 260usize;
    let mut v = Vec::new();
    for j in 0..=n {
        for i in 0..=n {
            v.push(DVec3::new(
                i as f64 * 100.0,
                j as f64 * 100.0,
                (((i * 73_856_093) ^ (j * 19_349_663)) % 1000) as f64,
            ));
        }
    }
    let at = |i: usize, j: usize| (j * (n + 1) + i) as u32;
    let mut t = Vec::new();
    for j in 0..n {
        for i in 0..n {
            t.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
            t.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
        }
    }
    let big = add(
        &mut ed,
        Shape::Mesh3d(Box::new(Mesh3d {
            name: String::new(),
            vertices: v,
            triangles: t,
            color: None,
        })),
    );
    let before = ed.doc().clone();
    let depth = ed.undo_depth();
    let e = ed.execute(&run(big, serde_json::json!({}))).unwrap_err();
    match e {
        CommandError::MeshToSurface(ConvertError::OverLimit { over, counts }) => {
            assert!(over.iter().any(|o| o.what == "faces" && o.limit == 100_000));
            assert_eq!(counts.faces, 135_200);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(ed.doc(), &before);
    assert_eq!(ed.undo_depth(), depth);
    // Not a mesh, a missing object, bad options.
    let model = add(&mut ed, Shape::SurfaceModel(Box::new(SurfaceModel::new())));
    assert!(matches!(
        ed.execute(&run(model, serde_json::json!({}))),
        Err(CommandError::Invalid(m)) if m.contains("not a mesh")
    ));
    assert!(matches!(
        ed.execute(&run(
            ObjectId(
                kadakas_core::uid::Uid::parse("00000000-0000-0000-0000-00000000270f").unwrap()
            ),
            serde_json::json!({})
        )),
        Err(CommandError::NoObject(_))
    ));
    let small = add(&mut ed, Shape::Mesh3d(Box::new(cube_mesh())));
    for extra in [
        serde_json::json!({"soften_below_angle": 400.0}),
        serde_json::json!({"tolerance": 0.0}),
    ] {
        assert!(matches!(
            ed.execute(&run(small, extra)),
            Err(CommandError::MeshToSurface(ConvertError::Options(_)))
        ));
        assert!(matches!(
            ed.doc().object(small).unwrap().shape,
            Shape::Mesh3d(_)
        ));
    }
}

#[test]
fn merge_false_keeps_every_triangle() {
    let mut ed = Editor::new(Document::default());
    let id = add(&mut ed, Shape::Mesh3d(Box::new(cube_mesh())));
    let out = ed
        .execute(&run(id, serde_json::json!({"merge": false})))
        .unwrap();
    assert_eq!(out.report.unwrap()["faces"], 12);
}
