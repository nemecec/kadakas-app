use super::*;
use kadakas_core::model::{Attributes, ObjectId, Shape};
use kadakas_core::surface::FaceId;

fn editor() -> Editor {
    Editor::with_sequential_ids()
}

fn names(ed: &Editor) -> Vec<String> {
    rows(ed.doc()).into_iter().map(|r| r.name).collect()
}

fn model_with_face(ed: &mut Editor) -> (ObjectId, FaceId) {
    let id = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let p = [
        [0.0, 0.0, 0.0],
        [1000.0, 0.0, 0.0],
        [1000.0, 1000.0, 0.0],
        [0.0, 1000.0, 0.0],
    ];
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        },
    })
    .unwrap();
    let face = match &ed.doc().object(id).unwrap().shape {
        Shape::SurfaceModel(m) => m.faces().next().unwrap().0,
        _ => unreachable!(),
    };
    (id, face)
}

#[test]
fn the_built_in_materials_are_added_once_in_one_step() {
    let mut ed = editor();
    let mut panel = MaterialsPanel::default();
    let mut bucket = None;
    assert_eq!(missing_defaults(ed.doc()).len(), 7);
    let depth = ed.undo_depth();
    panel
        .apply(&mut ed, &mut bucket, Request::AddDefaults)
        .unwrap();
    assert_eq!(ed.undo_depth(), depth + 1);
    assert_eq!(
        names(&ed),
        [
            "Grass",
            "Gravel",
            "Concrete pavers",
            "Bark mulch",
            "Water",
            "Brick",
            "Timber"
        ]
    );
    assert!(missing_defaults(ed.doc()).is_empty());
    // A second time adds nothing and says so.
    let msg = panel
        .apply(&mut ed, &mut bucket, Request::AddDefaults)
        .unwrap();
    assert_eq!(msg, tr!("uic-materials-defaults-there"));
    assert_eq!(ed.undo_depth(), depth + 1);
    assert!(ed.undo());
    assert!(rows(ed.doc()).is_empty());
}

#[test]
fn new_and_duplicate_make_unique_names_and_load_the_bucket() {
    let mut ed = editor();
    let mut panel = MaterialsPanel::default();
    let mut bucket = None;
    panel.apply(&mut ed, &mut bucket, Request::New).unwrap();
    panel.apply(&mut ed, &mut bucket, Request::New).unwrap();
    assert_eq!(names(&ed), ["Material", "Material 2"]);
    assert_eq!(bucket, panel.editing);
    let first = rows(ed.doc())[0].id;
    panel
        .apply(&mut ed, &mut bucket, Request::Duplicate(first))
        .unwrap();
    panel
        .apply(&mut ed, &mut bucket, Request::Duplicate(first))
        .unwrap();
    assert_eq!(
        names(&ed),
        ["Material", "Material 2", "Material copy", "Material copy 2"]
    );
    // A copy has the same content.
    let r = rows(ed.doc());
    assert_eq!(r[2].color, r[0].color);
}

#[test]
fn the_editor_changes_one_field_with_checks_and_one_undo_step_each() {
    let mut ed = editor();
    let mut panel = MaterialsPanel::default();
    let mut bucket = None;
    panel.apply(&mut ed, &mut bucket, Request::New).unwrap();
    panel.apply(&mut ed, &mut bucket, Request::New).unwrap();
    let (a, b) = (rows(ed.doc())[0].id, rows(ed.doc())[1].id);
    let depth = ed.undo_depth();
    for e in [
        MaterialEdit::Color([10, 20, 30]),
        MaterialEdit::Opacity(0.5),
        MaterialEdit::Roughness(0.25),
        MaterialEdit::Metallic(0.75),
        MaterialEdit::Pattern(Some(Procedural::Brick)),
        MaterialEdit::MappingKind(MappingKind::Planar),
        MaterialEdit::Scale([400.0, 200.0]),
        MaterialEdit::Rotation(30.0),
        MaterialEdit::Offset([5.0, 6.0]),
        MaterialEdit::Name("Teak".into()),
    ] {
        panel
            .apply(&mut ed, &mut bucket, Request::Edit(a, e))
            .unwrap();
    }
    assert_eq!(ed.undo_depth(), depth + 10);
    let m = present::material(ed.doc(), a).unwrap();
    assert_eq!(m.base_color.0, [10, 20, 30, 255]);
    assert_eq!((m.opacity, m.roughness, m.metallic), (0.5, 0.25, 0.75));
    assert!(matches!(
        m.texture,
        Some(Texture::Procedural {
            kind: Procedural::Brick,
            ..
        })
    ));
    assert_eq!(m.mapping.kind, MappingKind::Planar);
    assert_eq!(m.mapping.scale, [400.0, 200.0]);
    assert_eq!((m.mapping.rotation, m.mapping.offset), (30.0, [5.0, 6.0]));
    assert_eq!(ed.doc().resources.get(a).unwrap().name, "Teak");
    // Pattern none removes the texture.
    panel
        .apply(
            &mut ed,
            &mut bucket,
            Request::Edit(a, MaterialEdit::Pattern(None)),
        )
        .unwrap();
    assert!(present::material(ed.doc(), a).unwrap().texture.is_none());
    // Refused: a value out of range, an empty name, a name that is taken. Nothing changes.
    let depth = ed.undo_depth();
    for e in [
        MaterialEdit::Opacity(2.0),
        MaterialEdit::Scale([0.0, 1.0]),
        MaterialEdit::Name("  ".into()),
        MaterialEdit::Name("material 2".into()),
    ] {
        assert!(
            panel
                .apply(&mut ed, &mut bucket, Request::Edit(a, e))
                .is_err()
        );
    }
    // The same value again is no step.
    panel
        .apply(
            &mut ed,
            &mut bucket,
            Request::Edit(a, MaterialEdit::Opacity(0.5)),
        )
        .unwrap();
    assert_eq!(ed.undo_depth(), depth);
    let _ = b;
}

#[test]
fn an_unused_material_is_deleted_at_once_a_used_one_asks_first() {
    let mut ed = editor();
    let mut panel = MaterialsPanel::default();
    let mut bucket = None;
    panel
        .apply(&mut ed, &mut bucket, Request::AddDefaults)
        .unwrap();
    let r = rows(ed.doc());
    let (brick, grass) = (r[5].id, r[0].id);
    let (model, face) = model_with_face(&mut ed);
    panel
        .apply(
            &mut ed,
            &mut bucket,
            Request::SetFaces {
                model,
                faces: vec![face],
                side: PaintSide::Both,
                material: Some(brick),
            },
        )
        .unwrap();
    assert_eq!(rows(ed.doc())[5].uses, 2);
    // Unused: gone at once; the bucket lets go.
    bucket = Some(grass);
    let depth = ed.undo_depth();
    panel
        .apply(&mut ed, &mut bucket, Request::Delete(grass))
        .unwrap();
    assert_eq!(ed.undo_depth(), depth + 1);
    assert!(ed.doc().resources.get(grass).is_none());
    assert_eq!(bucket, None);
    // Used: nothing is deleted; the question is pending with the numbers.
    let depth = ed.undo_depth();
    panel
        .apply(&mut ed, &mut bucket, Request::Delete(brick))
        .unwrap();
    assert_eq!(ed.undo_depth(), depth);
    let pending = panel.pending_delete.clone().expect("it asks");
    assert_eq!((pending.id, pending.uses.faces), (brick, 2));
    // No is a cancel.
    panel
        .apply(&mut ed, &mut bucket, Request::CancelDelete)
        .unwrap();
    assert!(panel.pending_delete.is_none());
    assert!(ed.doc().resources.get(brick).is_some());
    // Yes: the faces go back to the default and the material goes, in one undo step.
    panel
        .apply(&mut ed, &mut bucket, Request::Delete(brick))
        .unwrap();
    panel
        .apply(&mut ed, &mut bucket, Request::ConfirmDelete)
        .unwrap();
    assert_eq!(ed.undo_depth(), depth + 1);
    assert!(ed.doc().resources.get(brick).is_none());
    let m = match &ed.doc().object(model).unwrap().shape {
        Shape::SurfaceModel(m) => m.clone(),
        _ => unreachable!(),
    };
    assert!(m.face(face).unwrap().attrs.is_empty());
    assert!(ed.undo());
    assert!(ed.doc().resources.get(brick).is_some());
    // A confirm with no question is refused.
    assert!(
        panel
            .apply(&mut ed, &mut bucket, Request::ConfirmDelete)
            .is_err()
    );
}

#[test]
fn a_material_in_a_slot_asks_too_and_purge_deletes_the_unused_ones() {
    let mut ed = editor();
    let mut panel = MaterialsPanel::default();
    let mut bucket = None;
    panel
        .apply(&mut ed, &mut bucket, Request::AddDefaults)
        .unwrap();
    let (model, _) = model_with_face(&mut ed);
    let timber = rows(ed.doc())[6].id;
    ed.execute(&Command::SetMaterialSlot {
        target: present::SlotTarget::Object {
            object: model,
            slot: None,
        },
        material: Some(timber),
    })
    .unwrap();
    panel
        .apply(&mut ed, &mut bucket, Request::Delete(timber))
        .unwrap();
    assert_eq!(panel.pending_delete.as_ref().unwrap().uses.slots, 1);
    // Purge keeps the used material and says how many went.
    bucket = Some(rows(ed.doc())[0].id);
    let depth = ed.undo_depth();
    let msg = panel.apply(&mut ed, &mut bucket, Request::Purge).unwrap();
    assert_eq!(msg, tr!("uic-materials-purged", count = 6));
    assert_eq!(ed.undo_depth(), depth + 1);
    assert_eq!(names(&ed), ["Timber"]);
    assert_eq!(bucket, None);
    assert!(
        panel.pending_delete.is_none(),
        "any other request closes the question"
    );
    let msg = panel.apply(&mut ed, &mut bucket, Request::Purge).unwrap();
    assert_eq!(msg, tr!("uic-materials-purge-none"));
}

#[test]
fn select_loads_the_bucket_and_a_swatch_shows_the_color_or_the_texture() {
    let mut ed = editor();
    let mut panel = MaterialsPanel::default();
    let mut bucket = None;
    panel
        .apply(&mut ed, &mut bucket, Request::AddDefaults)
        .unwrap();
    let r = rows(ed.doc());
    panel
        .apply(&mut ed, &mut bucket, Request::Select(Some(r[1].id)))
        .unwrap();
    assert_eq!(bucket, Some(r[1].id));
    panel
        .apply(&mut ed, &mut bucket, Request::Select(None))
        .unwrap();
    assert_eq!(bucket, None);
    // A plain material is one color; a textured one has detail; a clear one shows the checker.
    let plain = Material {
        base_color: Color([200, 10, 10, 255]),
        ..Default::default()
    };
    let s = swatch(&plain, 16);
    assert_eq!(s.len(), 256);
    assert!(s.iter().all(|p| *p == [200, 10, 10, 255]));
    let brick = present::material(ed.doc(), r[5].id).unwrap();
    let s = swatch(brick, 32);
    let first = s[0];
    assert!(s.iter().any(|p| *p != first), "texture detail");
    let clear = Material {
        opacity: 0.0,
        ..plain
    };
    let s = swatch(&clear, 16);
    assert!(s.iter().any(|p| *p == [235, 235, 235, 255]));
    assert!(s.iter().any(|p| *p == [205, 205, 205, 255]));
    assert!(resource(ed.doc(), r[0].id).is_some());
    assert!(resource(ed.doc(), ResourceId::from_num(9999)).is_none());
}

#[test]
fn a_gone_material_is_an_error_not_a_panic() {
    let mut ed = editor();
    let mut panel = MaterialsPanel::default();
    let mut bucket = None;
    let gone = ResourceId::from_num(4242);
    for req in [
        Request::Duplicate(gone),
        Request::Delete(gone),
        Request::Edit(gone, MaterialEdit::Opacity(0.5)),
    ] {
        assert!(panel.apply(&mut ed, &mut bucket, req).is_err());
    }
}
