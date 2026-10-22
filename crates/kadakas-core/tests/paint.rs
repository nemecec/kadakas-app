//! Paint Bucket in the core (MOD-05): materials on the faces of surface models, the material of
//! whole objects, the scene, and the deletion of a material that is in use.

use glam::DVec2;
use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::mesh::{self, SIDE_BACK, SIDE_BOTH, SIDE_FRONT};
use kadakas_core::model::*;
use kadakas_core::present::{self, SlotTarget};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_core::surface::{
    ATTR_BACK_MATERIAL, ATTR_MATERIAL, Edit, FaceId, PaintSide, SurfaceModel,
};

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

fn edit(ed: &mut Editor, id: ObjectId, e: Edit) -> Result<(), CommandError> {
    ed.execute(&Command::EditSurface { id, edit: e })
        .map(|_| ())
}

fn model(ed: &Editor, id: ObjectId) -> &SurfaceModel {
    match &ed.doc().object(id).unwrap().shape {
        Shape::SurfaceModel(m) => m,
        other => panic!("not a surface model: {}", other.kind()),
    }
}

fn add_material(ed: &mut Editor, name: &str, color: [u8; 4]) -> ResourceId {
    let m = present::Material {
        base_color: Color(color),
        ..Default::default()
    };
    let out = ed
        .execute(&Command::AddResource {
            name: name.into(),
            body: ResourceBody::Render(Box::new(present::RenderResource::Material(m))),
        })
        .unwrap();
    ResourceId(out.created[0].into())
}

/// A model with two squares side by side (two faces) and two materials.
fn two_faces() -> (Editor, ObjectId, [FaceId; 2], ResourceId, ResourceId) {
    let mut ed = Editor::with_sequential_ids();
    let id = add_model(&mut ed);
    let rect = |x: f64| {
        let p = [
            [x, 0.0, 0.0],
            [x + 1000.0, 0.0, 0.0],
            [x + 1000.0, 1000.0, 0.0],
            [x, 1000.0, 0.0],
        ];
        Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        }
    };
    edit(&mut ed, id, rect(0.0)).unwrap();
    edit(&mut ed, id, rect(2000.0)).unwrap();
    let faces: Vec<FaceId> = model(&ed, id).faces().map(|(f, _)| f).collect();
    assert_eq!(faces.len(), 2);
    let brick = add_material(&mut ed, "Brick", [158, 78, 58, 255]);
    let grass = add_material(&mut ed, "Grass", [96, 142, 62, 255]);
    (ed, id, [faces[0], faces[1]], brick, grass)
}

fn paint(
    ed: &mut Editor,
    id: ObjectId,
    faces: &[FaceId],
    side: PaintSide,
    m: Option<ResourceId>,
) -> Result<(), CommandError> {
    edit(
        ed,
        id,
        Edit::SetFaceMaterial {
            faces: faces.to_vec(),
            side,
            material: m.map(|m| m.to_string()),
        },
    )
}

fn attr(ed: &Editor, id: ObjectId, f: FaceId, key: &str) -> Option<String> {
    model(ed, id)
        .face(f)?
        .attrs
        .get(key)?
        .as_str()
        .map(str::to_string)
}

#[test]
fn front_back_and_both_go_to_their_attributes_and_each_paint_is_one_undo_step() {
    let (mut ed, id, [a, b], brick, grass) = two_faces();
    let depth = ed.undo_depth();
    paint(&mut ed, id, &[a], PaintSide::Front, Some(brick)).unwrap();
    paint(&mut ed, id, &[a], PaintSide::Back, Some(grass)).unwrap();
    paint(&mut ed, id, &[b], PaintSide::Both, Some(brick)).unwrap();
    assert_eq!(ed.undo_depth(), depth + 3);
    assert_eq!(attr(&ed, id, a, ATTR_MATERIAL), Some(brick.to_string()));
    assert_eq!(
        attr(&ed, id, a, ATTR_BACK_MATERIAL),
        Some(grass.to_string())
    );
    assert_eq!(attr(&ed, id, b, ATTR_MATERIAL), Some(brick.to_string()));
    assert_eq!(
        attr(&ed, id, b, ATTR_BACK_MATERIAL),
        Some(brick.to_string())
    );
    // Clearing one side leaves the other.
    paint(&mut ed, id, &[a], PaintSide::Front, None).unwrap();
    assert_eq!(attr(&ed, id, a, ATTR_MATERIAL), None);
    assert_eq!(
        attr(&ed, id, a, ATTR_BACK_MATERIAL),
        Some(grass.to_string())
    );
    // Undo and redo.
    let painted = model(&ed, id).clone();
    assert!(ed.undo());
    assert_eq!(attr(&ed, id, a, ATTR_MATERIAL), Some(brick.to_string()));
    assert!(ed.redo());
    assert_eq!(model(&ed, id), &painted);
    // The same paint again changes nothing and makes no step.
    let depth = ed.undo_depth();
    paint(&mut ed, id, &[b], PaintSide::Both, Some(brick)).unwrap();
    assert_eq!(ed.undo_depth(), depth);
}

#[test]
fn a_paint_is_refused_for_unknown_materials_faces_and_other_objects() {
    let (mut ed, id, [a, _], brick, _) = two_faces();
    let before = model(&ed, id).clone();
    // A name is not an id, and an id of no material is refused.
    let bad = edit(
        &mut ed,
        id,
        Edit::SetFaceMaterial {
            faces: vec![a],
            side: PaintSide::Front,
            material: Some("Brick".into()),
        },
    );
    assert!(bad.is_err());
    let none = ResourceId::from_num(9999);
    assert!(paint(&mut ed, id, &[a], PaintSide::Front, Some(none)).is_err());
    assert!(paint(&mut ed, id, &[FaceId(777)], PaintSide::Front, Some(brick)).is_err());
    assert_eq!(model(&ed, id), &before);
    // A plan object is no surface model.
    let out = ed
        .execute(&Command::AddObject {
            shape: Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::splat(100.0),
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    let rect = ObjectId(out.created[0]);
    assert!(paint(&mut ed, rect, &[a], PaintSide::Front, Some(brick)).is_err());
    assert!(present::paint_object_refusal(&ed.doc().object(rect).unwrap().shape).is_some());
    assert!(present::paint_object_refusal(&ed.doc().object(id).unwrap().shape).is_none());
}

#[test]
fn the_paint_is_deterministic_and_stays_through_reverse_with_the_sides_swapped() {
    let run = || {
        let (mut ed, id, [a, _], brick, grass) = two_faces();
        paint(&mut ed, id, &[a], PaintSide::Front, Some(brick)).unwrap();
        paint(&mut ed, id, &[a], PaintSide::Back, Some(grass)).unwrap();
        (ed, id, a, brick, grass)
    };
    let (mut e1, id1, a1, brick, grass) = run();
    let (e2, id2, _, _, _) = run();
    assert_eq!(model(&e1, id1), model(&e2, id2), "same edits, same model");
    edit(&mut e1, id1, Edit::ReverseFaces { faces: vec![a1] }).unwrap();
    assert_eq!(attr(&e1, id1, a1, ATTR_MATERIAL), Some(grass.to_string()));
    assert_eq!(
        attr(&e1, id1, a1, ATTR_BACK_MATERIAL),
        Some(brick.to_string())
    );
}

#[test]
fn the_paint_survives_the_file_and_a_replay_of_the_log() {
    let (mut ed, id, [a, _], brick, _) = two_faces();
    paint(&mut ed, id, &[a], PaintSide::Both, Some(brick)).unwrap();
    let m = model(&ed, id);
    let json = serde_json::to_value(m).unwrap();
    let back: SurfaceModel = serde_json::from_value(json).unwrap();
    assert_eq!(&back, m);
    // The log holds the paint as an intent that a replay on a fresh model repeats.
    let mut fresh = SurfaceModel::new();
    for entry in m.log() {
        kadakas_core::surface::sync::replay_intent(&mut fresh, &entry.intent).unwrap();
    }
    let painted = fresh
        .faces()
        .filter(|(_, f)| {
            f.attrs.get(ATTR_MATERIAL).and_then(|v| v.as_str()) == Some(&brick.to_string())
        })
        .count();
    assert_eq!(painted, 1);
}

#[test]
fn the_scene_draws_a_painted_face_with_its_sides() {
    let (mut ed, id, [a, b], brick, grass) = two_faces();
    let unpainted = mesh::build(ed.doc()).mesh.triangle_count();
    assert!(unpainted > 0);
    assert!(mesh::build(ed.doc()).mesh.paints.is_empty());
    // Both sides the same: one set of triangles with the material.
    paint(&mut ed, id, &[a], PaintSide::Both, Some(brick)).unwrap();
    let s = mesh::build(ed.doc());
    assert_eq!(s.mesh.triangle_count(), unpainted);
    let painted: Vec<usize> = (0..s.mesh.triangle_count())
        .filter(|t| s.mesh.paint_of(*t) == Some(brick))
        .collect();
    assert_eq!(painted.len(), 2, "two triangles of the square");
    assert!(painted.iter().all(|t| s.mesh.side_of(*t) == SIDE_BOTH));
    // The vertex color is the color of the material.
    let v = s.mesh.indices[painted[0] * 3] as usize;
    assert_eq!(s.mesh.colors[v], [158, 78, 58, 255]);
    // Front only: a front triangle with the material and a back triangle without.
    paint(&mut ed, id, &[b], PaintSide::Front, Some(grass)).unwrap();
    let s = mesh::build(ed.doc());
    assert_eq!(s.mesh.triangle_count(), unpainted + 2);
    let count = |side| {
        (0..s.mesh.triangle_count())
            .filter(|t| s.mesh.side_of(*t) == side)
            .count()
    };
    assert_eq!((count(SIDE_FRONT), count(SIDE_BACK)), (2, 2));
    assert!(
        (0..s.mesh.triangle_count())
            .filter(|t| s.mesh.side_of(*t) == SIDE_BACK)
            .all(|t| s.mesh.paint_of(t).is_none())
    );
    // Back with another material.
    paint(&mut ed, id, &[b], PaintSide::Back, Some(brick)).unwrap();
    let s = mesh::build(ed.doc());
    let back_paint: Vec<_> = (0..s.mesh.triangle_count())
        .filter(|t| s.mesh.side_of(*t) == SIDE_BACK)
        .map(|t| s.mesh.paint_of(t))
        .collect();
    assert_eq!(back_paint, vec![Some(brick); 2]);
    // The scene filter keeps the marks.
    let kept = s.retain(&|o| o == id);
    assert_eq!(kept.mesh.tri_side.len(), s.mesh.tri_side.len());
    assert_eq!(kept.mesh.paint_of(painted[0]), s.mesh.paint_of(painted[0]));
}

#[test]
fn a_material_on_a_group_goes_to_its_members_that_have_none() {
    let (mut ed, id, _, brick, grass) = two_faces();
    let other = add_model(&mut ed);
    let grp = ed
        .execute(&Command::Group {
            ids: vec![id, other],
            name: None,
        })
        .map(|o| ObjectId(o.created[0]))
        .unwrap();
    ed.execute(&Command::SetMaterialSlot {
        target: SlotTarget::Object {
            object: grp,
            slot: None,
        },
        material: Some(brick),
    })
    .unwrap();
    let slots = present::SlotIndex::new(ed.doc());
    for m in [id, other] {
        let obj = ed.doc().object(m).unwrap();
        assert_eq!(slots.resolve(obj, 0), Some(brick), "member {m}");
    }
    // A member with its own material keeps it.
    ed.execute(&Command::SetMaterialSlot {
        target: SlotTarget::Object {
            object: other,
            slot: None,
        },
        material: Some(grass),
    })
    .unwrap();
    let slots = present::SlotIndex::new(ed.doc());
    assert_eq!(
        slots.resolve(ed.doc().object(other).unwrap(), 0),
        Some(grass)
    );
    assert_eq!(slots.resolve(ed.doc().object(id).unwrap(), 0), Some(brick));
    assert_eq!(present::object_material(ed.doc(), grp), Some(brick));
    assert!(present::can_paint_object(
        &ed.doc().object(grp).unwrap().shape
    ));
}

#[test]
fn a_material_that_paints_faces_is_not_deleted_silently() {
    let (mut ed, id, [a, _], brick, grass) = two_faces();
    paint(&mut ed, id, &[a], PaintSide::Both, Some(brick)).unwrap();
    let uses = present::material_uses(ed.doc(), brick);
    assert_eq!(uses.faces, 2, "front and back of one face");
    assert!(!uses.is_unused());
    assert_eq!(uses.models.len(), 1);
    let err = ed
        .execute(&Command::DeleteResource {
            id: brick,
            reassign: None,
        })
        .unwrap_err();
    assert!(matches!(err, CommandError::ResourceInUse(_, 2)), "{err:?}");
    assert!(ed.doc().resources.get(brick).is_some());
    // An unused material goes, and after the faces are cleared the used one goes too, in one step.
    assert!(present::material_uses(ed.doc(), grass).is_unused());
    ed.execute(&Command::DeleteResource {
        id: grass,
        reassign: None,
    })
    .unwrap();
    let depth = ed.undo_depth();
    ed.execute(&Command::Batch {
        commands: vec![
            Command::EditSurface {
                id,
                edit: Edit::SetFaceMaterial {
                    faces: vec![a],
                    side: PaintSide::Both,
                    material: None,
                },
            },
            Command::DeleteResource {
                id: brick,
                reassign: None,
            },
        ],
    })
    .unwrap();
    assert_eq!(ed.undo_depth(), depth + 1);
    assert!(ed.doc().resources.get(brick).is_none());
    assert!(ed.undo());
    assert!(ed.doc().resources.get(brick).is_some());
    assert_eq!(attr(&ed, id, a, ATTR_MATERIAL), Some(brick.to_string()));
}

#[test]
fn a_face_value_may_name_the_material() {
    let (ed, _, _, brick, _) = two_faces();
    assert_eq!(present::material_ref(ed.doc(), "brick"), Some(brick));
    assert_eq!(
        present::material_ref(ed.doc(), &brick.to_string()),
        Some(brick)
    );
    assert_eq!(present::material_ref(ed.doc(), "nothing"), None);
}
