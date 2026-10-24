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

fn has(ed: &Editor, id: ObjectId) -> (bool, bool) {
    match &ed.doc().object(id).unwrap().shape {
        Shape::Mesh3d(m) => (m.exact.is_some(), m.exact_unusable.is_some()),
        _ => panic!(),
    }
}

/// A mesh whose payload cannot be used (a later version), of about `bytes` of JSON.
fn unusable(bytes: usize) -> Mesh3d {
    let mut m = mesh(0);
    m.drop_exact();
    m.exact_unusable = Some(Box::new(kadakas_core::mesh3d::UnusableExact {
        raw: serde_json::json!({"version": 9, "pad": "x".repeat(bytes)}),
        reason: "version 9".into(),
    }));
    m
}

#[test]
fn a_new_object_over_the_cap_of_one_object_keeps_its_triangles_only() {
    let mut ed = Editor::new(Document::default());
    let out = ed
        .execute(&Command::AddObject {
            shape: Shape::Mesh3d(Box::new(mesh(exact::MAX_OBJECT_BYTES + 5000))),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("for one object")),
        "{:?}",
        out.notes
    );
    assert_eq!(has(&ed, ObjectId(out.created[0])), (false, false));
    // Undo and redo keep it that way.
    assert!(ed.undo());
    assert!(ed.redo());
    assert_eq!(has(&ed, ObjectId(out.created[0])), (false, false));
}

#[test]
fn copy_objects_and_unusable_data_obey_the_cap_of_the_document_too() {
    let mut ed = Editor::new(Document::default());
    // Seven objects of 8 MiB less a little: 56 MiB. Then a payload that cannot be used.
    let ids: Vec<ObjectId> = (0..7)
        .map(|_| add(&mut ed, mesh(exact::MAX_OBJECT_BYTES - 1000)))
        .collect();
    let copy_objects = |ed: &mut Editor, id| {
        ed.execute(&Command::CopyObjects {
            ids: vec![id],
            delta: glam::DVec2::new(10.0, 0.0),
        })
        .unwrap()
    };
    // The first copy fits (the 8th). The next ones do not.
    let out = copy_objects(&mut ed, ids[0]);
    assert!(out.notes.is_empty(), "{:?}", out.notes);
    assert_eq!(has(&ed, ObjectId(out.created[0])), (true, false));
    let out = copy_objects(&mut ed, ids[0]);
    assert!(
        out.notes.iter().any(|n| n.contains("document")),
        "{:?}",
        out.notes
    );
    assert_eq!(has(&ed, ObjectId(out.created[0])), (false, false));
    // A new object with a payload that cannot be used is counted by its JSON: no room for it.
    let out = ed
        .execute(&Command::AddObject {
            shape: Shape::Mesh3d(Box::new(unusable(6 * 1024 * 1024))),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("document")),
        "{:?}",
        out.notes
    );
    assert_eq!(has(&ed, ObjectId(out.created[0])), (false, false));
}

#[test]
fn ten_array_copies_of_an_unusable_payload_do_not_pass_the_cap() {
    // The payload that the file brings is kept as it is, because the program cannot judge it.
    // Copies of it are new payload bytes, and ten of them would pass the cap of the document.
    let mut ed = Editor::new(Document::default());
    let id = add(&mut ed, unusable(7 * 1024 * 1024));
    assert_eq!(has(&ed, id), (false, true));
    let out = ed
        .execute(&Command::TransformObjects3d {
            ids: vec![id],
            matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 100.0, 0.0, 0.0],
            copy: true,
            count: 10,
        })
        .unwrap();
    let kept = out
        .created
        .iter()
        .filter(|c| has(&ed, ObjectId(**c)).1)
        .count();
    assert!(kept <= 8, "{kept} copies keep 7 MiB each");
    assert!(
        out.notes
            .iter()
            .any(|n| n.contains("keep their triangles only")),
        "{:?}",
        out.notes
    );
}
