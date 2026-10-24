//! The exact data of meshes is capped for the whole document: a copy that would pass the cap
//! keeps its triangles, loses the data and says so.

use glam::DVec3;
use kadakas_core::command::{Command, Editor};
use kadakas_core::exact::{self, ExactSolid};
use kadakas_core::mesh3d::Mesh3d;
use kadakas_core::model::*;

/// A mesh with a payload of about `bytes` (padded with vertices; the payload is not checked
/// when it is set).
fn mesh(bytes: usize) -> Mesh3d {
    let mut m = Mesh3d {
        name: "m".into(),
        vertices: vec![DVec3::ZERO, DVec3::X, DVec3::Y],
        triangles: vec![[0, 1, 2]],
        ..Default::default()
    };
    m.set_exact(ExactSolid {
        version: exact::VERSION,
        product: "p".into(),
        unit_mm: 1.0,
        tolerance_mm: 0.1,
        angle_deg: 15.0,
        vertices: vec![DVec3::ZERO; bytes / 56],
        edges: vec![],
        shells: vec![],
        faces: vec![],
        mesh_hash: 0,
        extra: Default::default(),
    });
    m
}

fn add(ed: &mut Editor, m: Mesh3d) -> ObjectId {
    let out = ed
        .execute(&Command::AddObject {
            shape: Shape::Mesh3d(Box::new(m)),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    ObjectId(out.created[0])
}

fn copy(ed: &mut Editor, id: ObjectId) -> kadakas_core::command::Outcome {
    let matrix = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 100.0, 0.0, 0.0];
    ed.execute(&Command::TransformObjects3d {
        ids: vec![id],
        matrix,
        copy: true,
        count: 1,
    })
    .unwrap()
}

#[test]
fn a_copy_over_the_cap_of_the_document_keeps_the_triangles_only() {
    let mut ed = Editor::new(Document::default());
    // Seven objects of 8 MiB less a little: 56 MiB of the 64 MiB.
    let ids: Vec<ObjectId> = (0..7)
        .map(|_| add(&mut ed, mesh(exact::MAX_OBJECT_BYTES - 1000)))
        .collect();
    let first = ids[0];
    // The first copy fits (the 8th object), the next ones do not.
    let out = copy(&mut ed, first);
    assert!(out.notes.is_empty(), "{:?}", out.notes);
    let new = ObjectId(out.created[0]);
    let kept = |ed: &Editor, id| match &ed.doc().object(id).unwrap().shape {
        Shape::Mesh3d(m) => m.exact.is_some(),
        _ => panic!(),
    };
    assert!(kept(&ed, new));
    let out = copy(&mut ed, first);
    assert!(
        out.notes
            .iter()
            .any(|n| n.contains("keep their triangles only")),
        "{:?}",
        out.notes
    );
    let new = ObjectId(out.created[0]);
    assert!(!kept(&ed, new));
    // The copy has its triangles and is a valid mesh.
    let Shape::Mesh3d(m) = &ed.doc().object(new).unwrap().shape else {
        panic!()
    };
    assert_eq!(m.triangles.len(), 1);
    // The original keeps its data.
    assert!(kept(&ed, first));
}
