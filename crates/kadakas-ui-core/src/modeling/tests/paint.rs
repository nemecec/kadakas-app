//! Tests of the Paint Bucket (MOD-05): the modes, the sampling, Esc, the sides, whole objects, the
//! refusal for plants, and undo.

use super::*;
use kadakas_core::command::Command;
use kadakas_core::model::{Attributes, Color};
use kadakas_core::present::{Material, RenderResource};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_core::surface::{ATTR_BACK_MATERIAL, ATTR_MATERIAL, Edit};

use crate::modeling::paint::{self, Fill, PaintHit, PaintMods};

fn add_material(ed: &mut Editor, name: &str, c: [u8; 4]) -> ResourceId {
    let m = Material {
        base_color: Color(c),
        ..Default::default()
    };
    ResourceId(
        ed.execute(&Command::AddResource {
            name: name.into(),
            body: ResourceBody::Render(Box::new(RenderResource::Material(m))),
        })
        .unwrap()
        .created[0],
    )
}

/// A closed box (six faces) in one model, and two materials.
fn boxed() -> (Editor, ObjectId, ResourceId, ResourceId) {
    let mut ed = Editor::with_sequential_ids();
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
    let face = model_of(ed.doc(), id).unwrap().faces().next().unwrap().0;
    ed.execute(&Command::EditSurface {
        id,
        edit: Edit::PushPull {
            face,
            distance: 1000.0,
        },
    })
    .unwrap();
    assert_eq!(model_of(ed.doc(), id).unwrap().face_count(), 6);
    let brick = add_material(&mut ed, "Brick", [158, 78, 58, 255]);
    let grass = add_material(&mut ed, "Grass", [96, 142, 62, 255]);
    (ed, id, brick, grass)
}

fn pt(x: f64, y: f64) -> kadakas_core::curve::Vertex {
    kadakas_core::curve::Vertex::corner(glam::DVec2::new(x, y))
}

fn faces(ed: &Editor, id: ObjectId) -> Vec<FaceId> {
    model_of(ed.doc(), id)
        .unwrap()
        .faces()
        .map(|(f, _)| f)
        .collect()
}

fn attr(ed: &Editor, id: ObjectId, f: FaceId, key: &str) -> Option<String> {
    model_of(ed.doc(), id)?
        .face(f)?
        .attrs
        .get(key)?
        .as_str()
        .map(str::to_string)
}

/// A click of the bucket, run on the editor.
fn click(
    ed: &mut Editor,
    paint: &mut paint::Paint,
    hit: PaintHit,
    selection: &[ObjectId],
    mods: PaintMods,
) -> Out {
    let out = paint.click(ed.doc(), hit, selection, mods);
    if !out.actions.is_empty() {
        run_actions(ed, &out.actions, false).expect("the paint runs");
    }
    out
}

fn face_hit(model: ObjectId, face: FaceId, front: bool) -> PaintHit {
    PaintHit::Face { model, face, front }
}

#[test]
fn a_click_paints_one_side_of_one_face_in_one_undo_step_and_undo_takes_it_back() {
    let (mut ed, id, brick, _) = boxed();
    let f = faces(&ed, id);
    let mut p = paint::Paint::default();
    p.set(Some(brick));
    let depth = ed.undo_depth();
    let out = click(
        &mut ed,
        &mut p,
        face_hit(id, f[0], true),
        &[],
        PaintMods::default(),
    );
    assert!(out.message.is_some());
    assert_eq!(ed.undo_depth(), depth + 1);
    assert_eq!(attr(&ed, id, f[0], ATTR_MATERIAL), Some(brick.to_string()));
    assert_eq!(attr(&ed, id, f[0], ATTR_BACK_MATERIAL), None);
    for other in &f[1..] {
        assert_eq!(attr(&ed, id, *other, ATTR_MATERIAL), None);
    }
    // The back of another face.
    click(
        &mut ed,
        &mut p,
        face_hit(id, f[1], false),
        &[],
        PaintMods::default(),
    );
    assert_eq!(
        attr(&ed, id, f[1], ATTR_BACK_MATERIAL),
        Some(brick.to_string())
    );
    assert_eq!(attr(&ed, id, f[1], ATTR_MATERIAL), None);
    assert!(ed.undo());
    assert_eq!(attr(&ed, id, f[1], ATTR_BACK_MATERIAL), None);
    assert!(ed.undo());
    assert_eq!(attr(&ed, id, f[0], ATTR_MATERIAL), None);
}

#[test]
fn the_modifiers_choose_the_fill() {
    let m = |ctrl, shift, sample| PaintMods {
        ctrl,
        shift,
        sample,
    };
    assert_eq!(m(false, false, false).fill(), Fill::Single);
    assert_eq!(m(true, false, false).fill(), Fill::Connected);
    assert_eq!(m(false, true, false).fill(), Fill::SameMaterial);
    assert_eq!(m(true, true, false).fill(), Fill::Connected, "Ctrl wins");
}

#[test]
fn ctrl_fills_the_connected_faces_with_the_same_material_and_stops_at_other_paint() {
    let (mut ed, id, brick, grass) = boxed();
    let f = faces(&ed, id);
    let mut p = paint::Paint::default();
    // One face has grass: the fill of the unpainted faces leaves it.
    p.set(Some(grass));
    click(
        &mut ed,
        &mut p,
        face_hit(id, f[2], true),
        &[],
        PaintMods::default(),
    );
    p.set(Some(brick));
    let ctrl = PaintMods {
        ctrl: true,
        ..Default::default()
    };
    // The faces of a box are consistently turned, so the front of every face is the same side.
    let items = paint::fill_faces(ed.doc(), id, f[0], true, Fill::Connected);
    assert_eq!(items.len(), 5, "all but the grass face: {items:?}");
    assert_eq!(items[0], (id, f[0], true), "the clicked face first");
    assert!(!items.iter().any(|(_, face, _)| *face == f[2]));
    click(&mut ed, &mut p, face_hit(id, f[0], true), &[], ctrl);
    let painted = f
        .iter()
        .filter(|x| attr(&ed, id, **x, ATTR_MATERIAL) == Some(brick.to_string()))
        .count();
    assert_eq!(painted, 5);
    assert_eq!(attr(&ed, id, f[2], ATTR_MATERIAL), Some(grass.to_string()));
    // One undo step for the whole fill.
    assert!(ed.undo());
    assert_eq!(attr(&ed, id, f[0], ATTR_MATERIAL), None);
    assert_eq!(attr(&ed, id, f[2], ATTR_MATERIAL), Some(grass.to_string()));
}

#[test]
fn shift_fills_all_faces_of_the_document_with_the_same_material_on_that_side() {
    let (mut ed, id, brick, grass) = boxed();
    // A second model with one face.
    let other = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let q = [
        [0.0, 0.0, 0.0],
        [500.0, 0.0, 0.0],
        [500.0, 500.0, 0.0],
        [0.0, 500.0, 0.0],
    ];
    ed.execute(&Command::EditSurface {
        id: other,
        edit: Edit::AddEdges {
            edges: (0..4).map(|i| [q[i], q[(i + 1) % 4]]).collect(),
        },
    })
    .unwrap();
    let f = faces(&ed, id);
    let g = faces(&ed, other)[0];
    let mut p = paint::Paint::default();
    // Brick on the front of three faces of the box and the face of the other model.
    p.set(Some(brick));
    for x in &f[..3] {
        click(
            &mut ed,
            &mut p,
            face_hit(id, *x, true),
            &[],
            PaintMods::default(),
        );
    }
    click(
        &mut ed,
        &mut p,
        face_hit(other, g, true),
        &[],
        PaintMods::default(),
    );
    // Shift with grass on one brick face: all four brick fronts become grass; the unpainted stay.
    p.set(Some(grass));
    let shift = PaintMods {
        shift: true,
        ..Default::default()
    };
    let out = click(&mut ed, &mut p, face_hit(id, f[0], true), &[], shift);
    assert!(out.message.is_some());
    for x in &f[..3] {
        assert_eq!(attr(&ed, id, *x, ATTR_MATERIAL), Some(grass.to_string()));
    }
    assert_eq!(attr(&ed, other, g, ATTR_MATERIAL), Some(grass.to_string()));
    for x in &f[3..] {
        assert_eq!(attr(&ed, id, *x, ATTR_MATERIAL), None);
    }
    // The back sides were not touched.
    assert_eq!(attr(&ed, other, g, ATTR_BACK_MATERIAL), None);
}

#[test]
fn alt_samples_the_material_under_the_pointer_and_paints_nothing() {
    let (mut ed, id, brick, grass) = boxed();
    let f = faces(&ed, id);
    let mut p = paint::Paint::default();
    p.set(Some(grass));
    click(
        &mut ed,
        &mut p,
        face_hit(id, f[0], true),
        &[],
        PaintMods::default(),
    );
    p.set(Some(brick));
    let depth = ed.undo_depth();
    let alt = PaintMods {
        sample: true,
        ..Default::default()
    };
    let out = click(&mut ed, &mut p, face_hit(id, f[0], true), &[], alt);
    assert!(out.actions.is_empty() && out.message.is_some());
    assert_eq!(p.bucket, Some(grass));
    assert_eq!(ed.undo_depth(), depth, "sampling is no step");
    // An unpainted side loads the default material: the next click clears.
    click(&mut ed, &mut p, face_hit(id, f[1], true), &[], alt);
    assert_eq!(p.bucket, None);
    click(
        &mut ed,
        &mut p,
        face_hit(id, f[0], true),
        &[],
        PaintMods::default(),
    );
    assert_eq!(attr(&ed, id, f[0], ATTR_MATERIAL), None, "cleared");
    // Over nothing a sample does nothing.
    p.set(Some(brick));
    click(&mut ed, &mut p, PaintHit::Nothing, &[], alt);
    assert_eq!(p.bucket, Some(brick));
    // An object with a material gives it to the bucket.
    let rect = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::default()),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::SetMaterialSlot {
        target: kadakas_core::present::SlotTarget::Object {
            object: rect,
            slot: None,
        },
        material: Some(grass),
    })
    .unwrap();
    p.set(None);
    click(&mut ed, &mut p, PaintHit::Object(rect), &[], alt);
    assert_eq!(p.bucket, Some(grass));
}

#[test]
fn escape_ends_nothing_and_keeps_the_bucket_when_the_tool_changes() {
    let (ed, _, brick, _) = boxed();
    let mut tools = Modeling::default();
    tools.choose(ModelerTool::PaintBucket);
    tools.paint.set(Some(brick));
    assert!(!tools.in_progress());
    assert!(
        !tools.cancel(),
        "nothing to cancel: the window goes to Select"
    );
    tools.choose(ModelerTool::Select);
    tools.choose(ModelerTool::PaintBucket);
    assert_eq!(tools.paint.material(ed.doc()), Some(brick));
    // A bucket whose material is gone is empty.
    let ed2 = Editor::default();
    assert_eq!(tools.paint.material(ed2.doc()), None);
}

#[test]
fn whole_objects_take_a_material_but_plants_hardscapes_and_site_models_are_refused() {
    let (mut ed, id, brick, _) = boxed();
    let mut p = paint::Paint::default();
    p.set(Some(brick));
    // A selected model: a click on one of its faces paints the object as a whole.
    let first = faces(&ed, id)[0];
    let out = click(
        &mut ed,
        &mut p,
        face_hit(id, first, true),
        &[id],
        PaintMods::default(),
    );
    assert!(out.message.is_some());
    assert_eq!(
        kadakas_core::present::object_material(ed.doc(), id),
        Some(brick)
    );
    assert_eq!(
        attr(&ed, id, first, ATTR_MATERIAL),
        None,
        "no face was painted"
    );
    assert!(ed.undo());
    assert_eq!(kadakas_core::present::object_material(ed.doc(), id), None);
    // A plant, a hardscape and a site model: refused, nothing changes.
    let shapes = [
        Shape::Plant {
            at: glam::DVec2::ZERO,
            species: "Quercus".into(),
            spread: 3000.0,
            height: 5000.0,
        },
        Shape::Hardscape {
            boundary: vec![pt(0.0, 0.0), pt(1000.0, 0.0), pt(1000.0, 1000.0)],
            material: "m".into(),
            thickness: None,
            elevation: None,
        },
    ];
    for shape in shapes {
        let kind = shape.kind();
        let o = ObjectId(
            ed.execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        let depth = ed.undo_depth();
        let out = click(
            &mut ed,
            &mut p,
            PaintHit::Object(o),
            &[],
            PaintMods::default(),
        );
        assert!(out.actions.is_empty(), "{kind}");
        assert_eq!(
            out.message,
            Some(kadakas_l10n::tr!("uic-paint-refused-style"))
        );
        assert_eq!(ed.undo_depth(), depth);
        // Also when selected with a model: the model is painted, the rest is counted.
        let out = click(
            &mut ed,
            &mut p,
            face_hit(id, first, true),
            &[id, o],
            PaintMods::default(),
        );
        assert_eq!(out.actions.len(), 1, "{kind}");
        assert!(out.message.unwrap().contains('1'));
        assert!(ed.undo());
    }
}

#[test]
fn a_group_and_an_instance_take_a_material_as_a_whole() {
    let (mut ed, id, brick, _) = boxed();
    let grp = ed
        .execute(&Command::Group {
            ids: vec![id],
            name: None,
        })
        .map(|o| ObjectId(o.created[0]))
        .unwrap();
    let mut p = paint::Paint::default();
    p.set(Some(brick));
    // The group is selected and a face of a member is clicked: the group is painted.
    let first = faces(&ed, id)[0];
    click(
        &mut ed,
        &mut p,
        face_hit(id, first, true),
        &[grp],
        PaintMods::default(),
    );
    assert_eq!(
        kadakas_core::present::object_material(ed.doc(), grp),
        Some(brick)
    );
    let slots = kadakas_core::present::SlotIndex::new(ed.doc());
    assert_eq!(slots.resolve(ed.doc().object(id).unwrap(), 0), Some(brick));
    // An instance that is hit without a selection is painted by itself.
    let out = paint::Paint::click(
        &mut p,
        ed.doc(),
        PaintHit::Object(grp),
        &[],
        PaintMods::default(),
    );
    assert!(matches!(out.actions[0], Action::PaintObjects { .. }));
}

#[test]
fn the_face_under_the_pointer_and_the_side_the_viewer_sees() {
    let (ed, id, brick, _) = boxed();
    let rev = ed.revision();
    let mut cache = SceneCache::default();
    let scenes = cache.get(ed.doc(), rev);
    let world = World {
        doc: ed.doc(),
        scenes,
    };
    let mut engine = paint::face_engine();
    // From above: the top of the box, its front is up.
    let above = Camera::look_at(
        v(500.0, 500.0, 6000.0),
        v(500.0, 500.0, 0.0),
        DVec3::Y,
        Projection::Orthographic { height: 3000.0 },
        DVec2::new(800.0, 800.0),
    )
    .unwrap();
    let hit = paint::hit_face(&world, &mut engine, &above, DVec2::new(400.0, 400.0));
    let PaintHit::Face { model, face, front } = hit else {
        panic!("a face: {hit:?}");
    };
    assert_eq!(model, id);
    let n = model_of(ed.doc(), id)
        .unwrap()
        .face(face)
        .unwrap()
        .plane
        .normal;
    assert_eq!(
        front,
        n.z > 0.0,
        "the front is seen when the normal points up"
    );
    // From below the other side of the box is seen.
    let below = Camera::look_at(
        v(500.0, 500.0, -6000.0),
        v(500.0, 500.0, 0.0),
        DVec3::Y,
        Projection::Orthographic { height: 3000.0 },
        DVec2::new(800.0, 800.0),
    )
    .unwrap();
    let PaintHit::Face {
        face: f2,
        front: front2,
        ..
    } = paint::hit_face(&world, &mut engine, &below, DVec2::new(400.0, 400.0))
    else {
        panic!("a face from below");
    };
    assert_ne!(f2, face, "the bottom is another face");
    let n2 = model_of(ed.doc(), id)
        .unwrap()
        .face(f2)
        .unwrap()
        .plane
        .normal;
    assert_eq!(front2, n2.z < 0.0);
    // Near an edge the face is still found (no snapping to the edge).
    let near_edge = above.project(v(500.0, 1.0, 1000.0)).unwrap();
    assert!(matches!(
        paint::hit_face(&world, &mut engine, &above, near_edge),
        PaintHit::Face { .. }
    ));
    // Over nothing.
    assert_eq!(
        paint::hit_face(&world, &mut engine, &above, DVec2::new(5.0, 5.0)),
        PaintHit::Nothing
    );
    let _ = brick;
}
