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
    ResourceId(out.created[0])
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

#[test]
fn material_uses_counts_slots_site_textures_and_both_sides_of_faces() {
    use kadakas_core::site::{SiteSettings, SurfaceLook};
    let (mut ed, id, [a, b], brick, grass) = two_faces();
    paint(&mut ed, id, &[a], PaintSide::Front, Some(brick)).unwrap();
    paint(&mut ed, id, &[b], PaintSide::Back, Some(brick)).unwrap();
    // An object slot and a class slot.
    ed.execute(&Command::SetMaterialSlot {
        target: SlotTarget::Object {
            object: id,
            slot: None,
        },
        material: Some(brick),
    })
    .unwrap();
    let class = ed.doc().active_class;
    ed.execute(&Command::SetMaterialSlot {
        target: SlotTarget::Class { class },
        material: Some(brick),
    })
    .unwrap();
    // The texture of a site model counts as a use too.
    let settings = SiteSettings {
        surface_look: SurfaceLook::Texture,
        texture: Some(brick),
        ..Default::default()
    };
    ed.execute(&Command::AddObject {
        shape: Shape::SiteModel {
            points: Vec::new(),
            lines: Vec::new(),
            settings,
        },
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let uses = present::material_uses(ed.doc(), brick);
    assert_eq!((uses.slots, uses.faces), (3, 2));
    assert_eq!(uses.total(), 5);
    assert_eq!(uses.models.len(), 1);
    assert_eq!(uses.models[0].1, vec![a], "front");
    assert_eq!(uses.models[0].2, vec![b], "back");
    assert!(present::material_uses(ed.doc(), grass).is_unused());
    let whole = SlotTarget::Object {
        object: id,
        slot: None,
    };
    let part = SlotTarget::Object {
        object: id,
        slot: Some("trunk".into()),
    };
    assert!(present::is_whole_object_slot(&whole, id));
    assert!(!present::is_whole_object_slot(&part, id));
    assert_eq!(
        present::face_materials(ed.doc(), &model(&ed, id).face(a).unwrap().attrs),
        (Some(brick), None)
    );
}

// ---- the paint through the other edits of a surface model ---------------------------------------

use glam::{DAffine3, DVec3};
use kadakas_core::surface::VertexId;

/// A square of 1000 split into two halves, `a` (the lower id) and `b`, and the two materials.
fn split_square() -> (Editor, ObjectId, [FaceId; 2], ResourceId, ResourceId) {
    let mut ed = Editor::with_sequential_ids();
    let id = add_model(&mut ed);
    let p = [
        [0.0, 0.0, 0.0],
        [1000.0, 0.0, 0.0],
        [1000.0, 1000.0, 0.0],
        [0.0, 1000.0, 0.0],
    ];
    let mut edges: Vec<[[f64; 3]; 2]> = (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect();
    edges.push([[500.0, 0.0, 0.0], [500.0, 1000.0, 0.0]]);
    edit(&mut ed, id, Edit::AddEdges { edges }).unwrap();
    let mut faces: Vec<FaceId> = model(&ed, id).faces().map(|(f, _)| f).collect();
    faces.sort();
    assert_eq!(faces.len(), 2);
    let brick = add_material(&mut ed, "Brick", [158, 78, 58, 255]);
    let grass = add_material(&mut ed, "Grass", [96, 142, 62, 255]);
    (ed, id, [faces[0], faces[1]], brick, grass)
}

fn vertex_at(m: &SurfaceModel, p: [f64; 3]) -> VertexId {
    m.vertices()
        .find(|(_, q)| q.distance(DVec3::from(p)) < 1e-6)
        .map(|(v, _)| v)
        .expect("a vertex there")
}

#[test]
fn erasing_the_edge_between_two_halves_keeps_the_paint_of_the_face_that_stays() {
    let (mut ed, id, [a, _], brick, grass) = split_square();
    paint(&mut ed, id, &[a], PaintSide::Front, Some(brick)).unwrap();
    paint(&mut ed, id, &[a], PaintSide::Back, Some(grass)).unwrap();
    let (v1, v2) = {
        let m = model(&ed, id);
        (
            vertex_at(m, [500.0, 0.0, 0.0]),
            vertex_at(m, [500.0, 1000.0, 0.0]),
        )
    };
    let e = model(&ed, id)
        .edge_between(v1, v2)
        .expect("the shared edge");
    edit(&mut ed, id, Edit::EraseEdges { edges: vec![e] }).unwrap();
    let m = model(&ed, id);
    assert_eq!(m.face_count(), 1);
    // Equal halves: the lower id stays, with its front and back paint.
    let (f, _) = m.faces().next().unwrap();
    assert_eq!(f, a);
    assert_eq!(attr(&ed, id, f, ATTR_MATERIAL), Some(brick.to_string()));
    assert_eq!(
        attr(&ed, id, f, ATTR_BACK_MATERIAL),
        Some(grass.to_string())
    );
}

#[test]
fn a_split_and_an_intersection_leave_the_paint_on_every_part() {
    // A split by a new edge.
    let (mut ed, id, [a, b], brick, grass) = split_square();
    paint(&mut ed, id, &[a, b], PaintSide::Both, Some(brick)).unwrap();
    edit(
        &mut ed,
        id,
        Edit::AddEdges {
            edges: vec![[[0.0, 500.0, 0.0], [500.0, 500.0, 0.0]]],
        },
    )
    .unwrap();
    assert_eq!(model(&ed, id).face_count(), 3);
    assert!(model(&ed, id).faces().all(|(f, _)| {
        attr(&ed, id, f, ATTR_MATERIAL) == Some(brick.to_string())
            && attr(&ed, id, f, ATTR_BACK_MATERIAL) == Some(brick.to_string())
    }));
    // Intersect Faces: a painted square cut by a vertical, unpainted square.
    let mut ed = Editor::with_sequential_ids();
    let id = add_model(&mut ed);
    let grass = {
        let _ = grass;
        add_material(&mut ed, "Grass", [96, 142, 62, 255])
    };
    let flat = [
        [0.0, 0.0, 0.0],
        [1000.0, 0.0, 0.0],
        [1000.0, 1000.0, 0.0],
        [0.0, 1000.0, 0.0],
    ];
    edit(
        &mut ed,
        id,
        Edit::AddEdges {
            edges: (0..4).map(|i| [flat[i], flat[(i + 1) % 4]]).collect(),
        },
    )
    .unwrap();
    let floor = model(&ed, id).faces().next().unwrap().0;
    paint(&mut ed, id, &[floor], PaintSide::Both, Some(grass)).unwrap();
    let wall = [
        [500.0, -500.0, -500.0],
        [500.0, 1500.0, -500.0],
        [500.0, 1500.0, 500.0],
        [500.0, -500.0, 500.0],
    ];
    edit(
        &mut ed,
        id,
        Edit::AddEdges {
            edges: (0..4).map(|i| [wall[i], wall[(i + 1) % 4]]).collect(),
        },
    )
    .unwrap();
    edit(
        &mut ed,
        id,
        Edit::IntersectFaces {
            scope: kadakas_core::surface::IntersectScope::Model,
            faces: Vec::new(),
            edges: Vec::new(),
        },
    )
    .unwrap();
    let m = model(&ed, id);
    let flat_faces: Vec<FaceId> = m
        .faces()
        .filter(|(_, f)| f.plane.normal.z.abs() > 0.9)
        .map(|(f, _)| f)
        .collect();
    assert_eq!(flat_faces.len(), 2, "the floor was cut in two");
    for f in flat_faces {
        assert_eq!(attr(&ed, id, f, ATTR_MATERIAL), Some(grass.to_string()));
    }
    let wall_painted = m
        .faces()
        .filter(|(_, f)| f.plane.normal.z.abs() < 0.1)
        .filter(|(f, _)| attr(&ed, id, *f, ATTR_MATERIAL).is_some())
        .count();
    assert_eq!(wall_painted, 0, "the wall was not painted");
}

#[test]
fn a_fold_and_its_unfold_keep_the_paint() {
    let (mut ed, id, [a, b], brick, grass) = split_square();
    // Make it one square first.
    let (v1, v2) = {
        let m = model(&ed, id);
        (
            vertex_at(m, [500.0, 0.0, 0.0]),
            vertex_at(m, [500.0, 1000.0, 0.0]),
        )
    };
    let e = model(&ed, id).edge_between(v1, v2).unwrap();
    edit(&mut ed, id, Edit::EraseEdges { edges: vec![e] }).unwrap();
    let _ = b;
    paint(&mut ed, id, &[a], PaintSide::Front, Some(brick)).unwrap();
    paint(&mut ed, id, &[a], PaintSide::Back, Some(grass)).unwrap();
    // Lift a corner: the square is not flat any more and folds into two triangles.
    let corner = vertex_at(model(&ed, id), [1000.0, 1000.0, 0.0]);
    let lift = |d: f64| Edit::TransformVertices {
        vertices: vec![corner],
        faces: Vec::new(),
        edges: Vec::new(),
        matrix: DAffine3::from_translation(DVec3::new(0.0, 0.0, d)).to_cols_array(),
    };
    edit(&mut ed, id, lift(300.0)).unwrap();
    assert_eq!(model(&ed, id).face_count(), 2, "folded");
    for (f, _) in model(&ed, id).faces() {
        assert_eq!(attr(&ed, id, f, ATTR_MATERIAL), Some(brick.to_string()));
        assert_eq!(
            attr(&ed, id, f, ATTR_BACK_MATERIAL),
            Some(grass.to_string())
        );
    }
    // Back to flat: the pieces of the fold unfold into one face with the paint.
    edit(&mut ed, id, lift(-300.0)).unwrap();
    assert_eq!(model(&ed, id).face_count(), 1, "unfolded");
    let (f, _) = model(&ed, id).faces().next().unwrap();
    assert_eq!(attr(&ed, id, f, ATTR_MATERIAL), Some(brick.to_string()));
    assert_eq!(
        attr(&ed, id, f, ATTR_BACK_MATERIAL),
        Some(grass.to_string())
    );
}

#[test]
fn a_mirrored_model_keeps_its_front_in_front() {
    let (mut ed, id, [a, _], brick, _) = split_square();
    paint(&mut ed, id, &[a], PaintSide::Front, Some(brick)).unwrap();
    let mirror = DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0));
    ed.execute(&Command::TransformObjects3d {
        ids: vec![id],
        matrix: mirror.to_cols_array(),
        copy: false,
        count: 1,
    })
    .unwrap();
    let scene = mesh::build(ed.doc());
    // The front of the face is still up: the triangles of the front set and the back set both
    // wind counterclockwise seen from above, so the front is the side seen from above.
    let mut front = 0;
    for t in 0..scene.mesh.triangle_count() {
        let [p0, p1, p2] = scene.mesh.triangle(t);
        assert!((p1 - p0).cross(p2 - p0).z > 0.0, "triangle {t} faces up");
        if scene.mesh.side_of(t) == SIDE_FRONT {
            assert_eq!(scene.mesh.paint_of(t), Some(brick));
            front += 1;
        }
    }
    assert!(front > 0);
}

// ---- push/pull ---------------------------------------------------------------------------------

#[test]
fn push_pull_gives_walls_the_paint_and_keeps_the_paint_on_its_side() {
    let (mut ed, id, [a, b], brick, grass) = {
        let (ed, id, f, b, g) = two_faces();
        (ed, id, f, b, g)
    };
    let _ = b;
    paint(&mut ed, id, &[a], PaintSide::Front, Some(brick)).unwrap();
    paint(&mut ed, id, &[a], PaintSide::Back, Some(grass)).unwrap();
    edit(
        &mut ed,
        id,
        Edit::PushPull {
            face: a,
            distance: 500.0,
        },
    )
    .unwrap();
    let m = model(&ed, id);
    let side = |f: FaceId, key: &str| {
        model(&ed, id)
            .face(f)?
            .attrs
            .get(key)?
            .as_str()
            .map(str::to_string)
    };
    // The cap keeps the face, its id and its paint.
    assert_eq!(side(a, ATTR_MATERIAL), Some(brick.to_string()));
    assert_eq!(side(a, ATTR_BACK_MATERIAL), Some(grass.to_string()));
    // The four walls have the paint of the face; the old place became the bottom, turned over,
    // so the paint that was on its front is on its back now (the same physical side).
    let mut walls = 0;
    let mut bottoms = 0;
    for (f, face) in m
        .faces()
        .filter(|(f, _)| m.face_area(*f).is_some() && *f != a)
    {
        // The second square of the model is not part of the solid.
        if face.plane.normal.z.abs() > 0.9 {
            if side(f, ATTR_BACK_MATERIAL) == Some(brick.to_string()) {
                assert_eq!(side(f, ATTR_MATERIAL), Some(grass.to_string()));
                bottoms += 1;
            }
        } else if side(f, ATTR_MATERIAL) == Some(brick.to_string()) {
            assert_eq!(side(f, ATTR_BACK_MATERIAL), Some(grass.to_string()));
            walls += 1;
        }
    }
    assert_eq!((walls, bottoms), (4, 1));
    // Turning a face over by hand does the same to the paint: the two agree.
    let before = model(&ed, id).face(a).unwrap().attrs.clone();
    edit(&mut ed, id, Edit::ReverseFaces { faces: vec![a] }).unwrap();
    let after = model(&ed, id).face(a).unwrap().attrs.clone();
    assert_eq!(after[ATTR_MATERIAL], before[ATTR_BACK_MATERIAL]);
    assert_eq!(after[ATTR_BACK_MATERIAL], before[ATTR_MATERIAL]);
}

#[test]
fn a_free_face_pushed_the_other_way_keeps_the_old_face_and_turns_the_cap() {
    let (mut ed, id, [a, _], brick, grass) = two_faces();
    paint(&mut ed, id, &[a], PaintSide::Front, Some(brick)).unwrap();
    paint(&mut ed, id, &[a], PaintSide::Back, Some(grass)).unwrap();
    let old: Vec<FaceId> = model(&ed, id).faces().map(|(f, _)| f).collect();
    edit(
        &mut ed,
        id,
        Edit::PushPull {
            face: a,
            distance: -500.0,
        },
    )
    .unwrap();
    // The cap (the id of the face) moved down and looks to the back: its paint is swapped. The
    // old face stayed in place with the paint it had.
    assert_eq!(attr(&ed, id, a, ATTR_MATERIAL), Some(grass.to_string()));
    assert_eq!(
        attr(&ed, id, a, ATTR_BACK_MATERIAL),
        Some(brick.to_string())
    );
    let stayed = model(&ed, id)
        .faces()
        .filter(|(f, _)| !old.contains(f))
        .filter(|(f, face)| {
            face.plane.normal.z > 0.9
                && attr(&ed, id, *f, ATTR_MATERIAL) == Some(brick.to_string())
                && attr(&ed, id, *f, ATTR_BACK_MATERIAL) == Some(grass.to_string())
        })
        .count();
    assert_eq!(stayed, 1, "the old face stays where it was, with its paint");
}

// ---- groups, plants and Explode ------------------------------------------------------------------

fn add_plant(ed: &mut Editor) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Plant {
                at: DVec2::new(5000.0, 5000.0),
                species: "Quercus".into(),
                spread: 1000.0,
                height: 2000.0,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

fn set_object_material(ed: &mut Editor, object: ObjectId, m: ResourceId) {
    ed.execute(&Command::SetMaterialSlot {
        target: SlotTarget::Object { object, slot: None },
        material: Some(m),
    })
    .unwrap();
}

#[test]
fn a_material_on_a_group_does_not_turn_its_plants_and_hardscapes_into_that_material() {
    let (mut ed, id, _, brick, _) = two_faces();
    let plant = add_plant(&mut ed);
    let slab = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Hardscape {
                boundary: kadakas_core::curve::corners(&[
                    DVec2::new(0.0, 0.0),
                    DVec2::new(1000.0, 0.0),
                    DVec2::new(1000.0, 1000.0),
                ]),
                material: "m".into(),
                thickness: None,
                elevation: None,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let grp = ed
        .execute(&Command::Group {
            ids: vec![id, plant, slab],
            name: None,
        })
        .map(|o| ObjectId(o.created[0]))
        .unwrap();
    set_object_material(&mut ed, grp, brick);
    let slots = present::SlotIndex::new(ed.doc());
    let obj = |o: ObjectId| ed.doc().object(o).unwrap();
    assert_eq!(slots.resolve(obj(id), 0), Some(brick), "the model takes it");
    for (name, o) in [("plant", plant), ("hardscape", slab)] {
        for part in [0, present::PART_TRUNK, present::PART_CANOPY] {
            assert_eq!(slots.resolve(obj(o), part), None, "{name} part {part}");
        }
    }
}

#[test]
fn exploding_a_painted_group_or_instance_leaves_the_paint_on_what_was_in_it() {
    // A group with a material and a plain model.
    let (mut ed, id, [a, _], brick, grass) = two_faces();
    let plant = add_plant(&mut ed);
    let grp = ed
        .execute(&Command::Group {
            ids: vec![id, plant],
            name: None,
        })
        .map(|o| ObjectId(o.created[0]))
        .unwrap();
    set_object_material(&mut ed, grp, brick);
    // A face of its own paint keeps it.
    paint(&mut ed, id, &[a], PaintSide::Front, Some(grass)).unwrap();
    ed.execute(&Command::Explode { ids: vec![grp] }).unwrap();
    assert!(ed.doc().object(grp).is_none());
    let models: Vec<ObjectId> = ed
        .doc()
        .objects
        .values()
        .filter(|o| matches!(o.shape, Shape::SurfaceModel(_)))
        .map(|o| o.id)
        .collect();
    assert_eq!(models.len(), 1);
    let m = models[0];
    let mut front = std::collections::BTreeMap::new();
    for (f, _) in model(&ed, m).faces() {
        *front.entry(attr(&ed, m, f, ATTR_MATERIAL)).or_insert(0) += 1;
        assert_eq!(
            attr(&ed, m, f, ATTR_BACK_MATERIAL),
            Some(brick.to_string()),
            "the back takes the material of the group"
        );
    }
    assert_eq!(front.get(&Some(grass.to_string())), Some(&1), "{front:?}");
    assert_eq!(front.get(&Some(brick.to_string())), Some(&1), "{front:?}");
    // The plant stays as it was: no material, and no slot is left behind.
    let slots = present::SlotIndex::new(ed.doc());
    assert_eq!(slots.resolve(ed.doc().object(plant).unwrap(), 0), None);
    assert!(
        ed.doc()
            .drawing
            .presentation
            .slots
            .iter()
            .all(|e| { !matches!(&e.target, SlotTarget::Object { object, .. } if *object == grp) })
    );

    // An instance with a material.
    let (mut ed, id, _, brick, _) = two_faces();
    let all: Vec<FaceId> = model(&ed, id).faces().map(|(f, _)| f).collect();
    let out = ed
        .execute(&Command::MakeComponent {
            name: "Pair".into(),
            model: Some(id),
            faces: all,
            edges: vec![],
            objects: vec![],
            replace: true,
        })
        .unwrap();
    let inst = ObjectId(out.created[1]);
    set_object_material(&mut ed, inst, brick);
    ed.execute(&Command::Explode { ids: vec![inst] }).unwrap();
    let m = ed
        .doc()
        .objects
        .values()
        .find(|o| matches!(o.shape, Shape::SurfaceModel(_)))
        .map(|o| o.id)
        .expect("the model of the instance");
    assert!(model(&ed, m).faces().all(|(f, _)| {
        attr(&ed, m, f, ATTR_MATERIAL) == Some(brick.to_string())
            && attr(&ed, m, f, ATTR_BACK_MATERIAL) == Some(brick.to_string())
    }));
}

// ---- who uses a material -------------------------------------------------------------------------

#[test]
fn texture_beds_and_the_parts_of_definitions_count_as_uses() {
    use kadakas_core::curve;
    use kadakas_core::model::ModifierKind;
    let (mut ed, id, [a, _], brick, grass) = two_faces();
    // A texture bed holds the grass.
    ed.execute(&Command::AddObject {
        shape: Shape::SiteModifier {
            modifier: ModifierKind::TextureBed {
                texture: Some(grass),
                mapping: None,
            },
            boundary: curve::corners(&[
                DVec2::new(0.0, 0.0),
                DVec2::new(100.0, 0.0),
                DVec2::new(100.0, 100.0),
            ]),
        },
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let uses = present::material_uses(ed.doc(), grass);
    assert_eq!((uses.slots, uses.faces), (1, 0));
    assert!(!uses.is_unused());
    // The faces of the parts of a component definition count, and keep the material.
    paint(&mut ed, id, &[a], PaintSide::Both, Some(brick)).unwrap();
    let all: Vec<FaceId> = model(&ed, id).faces().map(|(f, _)| f).collect();
    ed.execute(&Command::MakeComponent {
        name: "Pair".into(),
        model: Some(id),
        faces: all,
        edges: vec![],
        objects: vec![],
        replace: true,
    })
    .unwrap();
    let uses = present::material_uses(ed.doc(), brick);
    assert_eq!(uses.faces, 2);
    assert!(uses.models.is_empty() && uses.parts.len() == 1, "{uses:?}");
    let err = ed
        .execute(&Command::DeleteResource {
            id: brick,
            reassign: None,
        })
        .unwrap_err();
    assert!(matches!(err, CommandError::ResourceInUse(_, 2)), "{err:?}");
    // The map gives every material in one pass and agrees with the single question.
    let map = present::material_use_map(ed.doc());
    assert_eq!(map.get(&brick), Some(&uses));
    assert_eq!(map.get(&grass).map(|u| u.slots), Some(1));
}

#[test]
fn a_face_that_names_a_material_that_is_gone_shows_the_default_and_counts_for_nothing() {
    let (ed, id, [a, _], brick, _) = two_faces();
    // The text of an id that no material has (a material that another user deleted).
    let ghost = ResourceId::from_num(31337).to_string();
    let mut m = model(&ed, id).clone();
    m.apply(&Edit::SetFaceMaterial {
        faces: vec![a],
        side: PaintSide::Both,
        material: Some(ghost),
    })
    .unwrap();
    let resolver = present::PaintResolver::new(ed.doc());
    assert_eq!(
        resolver.faces(&m.face(a).unwrap().attrs),
        (None, None),
        "nothing to draw"
    );
    assert!(present::material_uses(ed.doc(), brick).is_unused());
}

#[test]
fn a_face_that_names_the_material_shows_it_and_counts_as_a_use_and_can_edit_follows_the_layer() {
    // A file made by hand: the face holds the name of the material, not its id.
    let (mut ed, _, _, brick, _) = two_faces();
    let mut m = SurfaceModel::new();
    let p = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
    ];
    m.apply(&Edit::AddEdges {
        edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
    })
    .unwrap();
    let face = m.faces().next().unwrap().0;
    m.apply(&Edit::SetFaceMaterial {
        faces: vec![face],
        side: PaintSide::Front,
        material: Some("BRICK".into()),
    })
    .unwrap();
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
    let uses = present::material_uses(ed.doc(), brick);
    assert_eq!((uses.faces, uses.total()), (1, 1));
    assert_eq!(uses.models[0].0, id);
    let scene = mesh::build(ed.doc());
    assert!(
        (0..scene.mesh.triangle_count()).any(|t| scene.mesh.paint_of(t) == Some(brick)),
        "the name draws the material"
    );
    assert_eq!(
        present::material_ref(ed.doc(), "brick"),
        Some(brick),
        "any case"
    );
    // An object can be edited unless its layer is locked.
    let obj = ed.doc().object(id).unwrap().clone();
    assert!(present::can_edit(ed.doc(), &obj));
    ed.execute(&Command::ModifyLayer {
        id: obj.layer,
        name: None,
        visible: None,
        locked: Some(true),
        elevation: None,
        scale: None,
    })
    .unwrap();
    assert!(!present::can_edit(ed.doc(), &obj));
}

#[test]
fn exploding_a_painted_group_gives_the_material_to_a_group_inside() {
    let (mut ed, id, _, brick, _) = two_faces();
    let inner = ed
        .execute(&Command::Group {
            ids: vec![id],
            name: None,
        })
        .map(|o| ObjectId(o.created[0]))
        .unwrap();
    let outer = ed
        .execute(&Command::Group {
            ids: vec![inner],
            name: None,
        })
        .map(|o| ObjectId(o.created[0]))
        .unwrap();
    set_object_material(&mut ed, outer, brick);
    ed.execute(&Command::Explode { ids: vec![outer] }).unwrap();
    assert!(ed.doc().object(outer).is_none());
    assert_eq!(present::object_material(ed.doc(), inner), Some(brick));
}

#[test]
fn every_kind_of_edit_replays_from_the_log_to_the_same_model() {
    use kadakas_core::surface::IntersectScope;
    let mut m = SurfaceModel::new();
    let sq = |x: f64| {
        let p = [
            [x, 0.0, 0.0],
            [x + 1000.0, 0.0, 0.0],
            [x + 1000.0, 1000.0, 0.0],
            [x, 1000.0, 0.0],
        ];
        (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect::<Vec<_>>()
    };
    m.apply(&Edit::AddEdges { edges: sq(0.0) }).unwrap();
    let floor = m.faces().next().unwrap().0;
    m.apply(&Edit::PushPull {
        face: floor,
        distance: 500.0,
    })
    .unwrap();
    m.apply(&Edit::IntersectFaces {
        scope: IntersectScope::Model,
        faces: Vec::new(),
        edges: Vec::new(),
    })
    .unwrap();
    // A second square far away, copied, turned over, painted and erased.
    m.apply(&Edit::AddEdges { edges: sq(5000.0) }).unwrap();
    let far = m
        .faces()
        .find(|(_, f)| {
            f.outer
                .iter()
                .any(|v| m.vertex(*v).is_some_and(|p| p.x >= 5000.0))
        })
        .map(|(id, _)| id)
        .unwrap();
    m.apply(&Edit::IntersectFaces {
        scope: IntersectScope::Selection,
        faces: vec![far],
        edges: Vec::new(),
    })
    .unwrap();
    let corner = vertex_at(&m, [5000.0, 0.0, 0.0]);
    m.apply(&Edit::TransformVertices {
        vertices: vec![corner],
        faces: Vec::new(),
        edges: Vec::new(),
        matrix: DAffine3::from_translation(DVec3::new(-100.0, 0.0, 0.0)).to_cols_array(),
    })
    .unwrap();
    m.apply(&Edit::CopyGeometry {
        faces: vec![far],
        edges: Vec::new(),
        matrix: DAffine3::from_translation(DVec3::new(0.0, 3000.0, 0.0)).to_cols_array(),
        count: 1,
    })
    .unwrap();
    m.apply(&Edit::ReverseFaces { faces: vec![far] }).unwrap();
    m.apply(&Edit::SetFaceMaterial {
        faces: vec![far],
        side: PaintSide::Both,
        material: Some(ResourceId::from_num(7).to_string()),
    })
    .unwrap();
    m.apply(&Edit::AddCurve {
        points: (0..8)
            .map(|i| {
                let a = std::f64::consts::TAU * f64::from(i) / 8.0;
                [-3000.0 + 300.0 * a.cos(), 300.0 * a.sin(), 0.0]
            })
            .collect(),
        closed: true,
        faceted: false,
    })
    .unwrap();
    let circle = m
        .faces()
        .filter(|(_, f)| f.outer.len() == 8)
        .map(|(id, _)| id)
        .next()
        .unwrap();
    m.apply(&Edit::EraseFaces {
        faces: vec![circle],
    })
    .unwrap();
    let some_edge = m.edges().next().unwrap().0;
    m.apply(&Edit::EraseEdges {
        edges: vec![some_edge],
    })
    .unwrap();
    // Replay the log on an empty model: the same model comes out.
    assert!(m.log().len() >= 10, "{} entries", m.log().len());
    let mut fresh = SurfaceModel::new();
    for entry in m.log() {
        kadakas_core::surface::sync::replay_intent(&mut fresh, &entry.intent)
            .unwrap_or_else(|e| panic!("{:?}: {e}", entry.intent));
    }
    assert_eq!(
        (fresh.vertex_count(), fresh.edge_count(), fresh.face_count()),
        (m.vertex_count(), m.edge_count(), m.face_count())
    );
    let paint_of = |s: &SurfaceModel| {
        s.faces()
            .filter(|(_, f)| f.attrs.contains_key(ATTR_MATERIAL))
            .count()
    };
    assert_eq!(paint_of(&fresh), paint_of(&m));
}
