//! Move, turn, scale and copy of whole objects in 3D (MOD-05): `Command::TransformObjects3d`.

use glam::{DAffine3, DQuat, DVec2, DVec3};
use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::model::*;
use kadakas_core::surface::{Edit, SurfaceModel};

fn add_shape(ed: &mut Editor, shape: Shape) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

fn cube_edges(s: f64) -> Edit {
    let c = |i: usize| {
        [
            if i & 1 == 1 { s } else { 0.0 },
            if i & 2 == 2 { s } else { 0.0 },
            if i & 4 == 4 { s } else { 0.0 },
        ]
    };
    let mut edges = Vec::new();
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                edges.push([c(i), c(i | bit)]);
            }
        }
    }
    Edit::AddEdges { edges }
}

fn cube(ed: &mut Editor) -> ObjectId {
    let m = add_shape(ed, Shape::SurfaceModel(Box::default()));
    ed.execute(&Command::EditSurface {
        id: m,
        edit: cube_edges(1000.0),
    })
    .unwrap();
    m
}

fn model(ed: &Editor, id: ObjectId) -> &SurfaceModel {
    match &ed.doc().object(id).unwrap().shape {
        Shape::SurfaceModel(m) => m,
        other => panic!("not a surface model: {}", other.kind()),
    }
}

fn run(
    ed: &mut Editor,
    ids: &[ObjectId],
    xf: DAffine3,
    copy: bool,
    count: u32,
) -> Vec<kadakas_core::uid::Uid> {
    ed.execute(&Command::TransformObjects3d {
        ids: ids.to_vec(),
        matrix: xf.to_cols_array(),
        copy,
        count,
    })
    .unwrap()
    .created
}

fn plant(ed: &mut Editor) -> ObjectId {
    add_shape(
        ed,
        Shape::Plant {
            at: DVec2::new(5000.0, 0.0),
            species: "x".into(),
            spread: 2000.0,
            height: 3000.0,
        },
    )
}

#[test]
fn a_surface_model_turns_about_any_axis_in_one_undo_step() {
    let mut ed = Editor::with_sequential_ids();
    let m = cube(&mut ed);
    let before = serde_json::to_string(&ed.doc().object(m).unwrap().shape).unwrap();
    let depth = ed.undo_depth();
    let c = DVec3::new(500.0, 500.0, 500.0);
    let axis = DVec3::new(1.0, 1.0, 0.0).normalize();
    let xf = DAffine3::from_translation(c)
        * DAffine3::from_quat(DQuat::from_axis_angle(axis, 0.8))
        * DAffine3::from_translation(-c);
    run(&mut ed, &[m], xf, false, 1);
    assert_eq!(ed.undo_depth(), depth + 1);
    let (lo, hi) = model(&ed, m).bounds().unwrap();
    // The centre of the cube stays where it was.
    assert!(((lo + hi) / 2.0 - c).length() < 1e-6, "{lo:?} {hi:?}");
    assert!(model(&ed, m).check().is_valid());
    // The geometry itself is untouched: only the transform changed.
    assert_eq!(model(&ed, m).vertex_count(), 8);
    ed.undo();
    assert_eq!(
        serde_json::to_string(&ed.doc().object(m).unwrap().shape).unwrap(),
        before
    );
}

#[test]
fn a_surface_model_scales_without_keeping_its_proportions() {
    let mut ed = Editor::with_sequential_ids();
    let m = cube(&mut ed);
    run(
        &mut ed,
        &[m],
        DAffine3::from_scale(DVec3::new(2.0, 1.0, 3.0)),
        false,
        1,
    );
    let (lo, hi) = model(&ed, m).bounds().unwrap();
    assert!((hi - lo - DVec3::new(2000.0, 1000.0, 3000.0)).length() < 1e-6);
}

#[test]
fn a_group_moves_as_one_unit_and_a_copy_makes_a_row() {
    let mut ed = Editor::with_sequential_ids();
    let a = cube(&mut ed);
    let b = cube(&mut ed);
    let g = ObjectId(
        ed.execute(&Command::Group {
            ids: vec![a, b],
            name: None,
        })
        .unwrap()
        .created[0],
    );
    let depth = ed.undo_depth();
    run(
        &mut ed,
        &[g],
        DAffine3::from_translation(DVec3::new(0.0, 0.0, 700.0)),
        false,
        1,
    );
    assert_eq!(ed.undo_depth(), depth + 1);
    for id in [a, b] {
        assert_eq!(model(&ed, id).bounds().unwrap().0.z, 700.0);
    }
    let n = ed.doc().objects.len();
    let created = run(
        &mut ed,
        &[g],
        DAffine3::from_translation(DVec3::new(3000.0, 0.0, 0.0)),
        true,
        3,
    );
    // Each copy is a group and its two members: three objects; three copies.
    assert_eq!(ed.doc().objects.len(), n + 9);
    assert_eq!(created.len(), 3);
    assert_eq!(
        ed.undo_depth(),
        depth + 2,
        "one undo step for the whole row"
    );
    let xs: Vec<f64> = created
        .iter()
        .flat_map(|id| ed.doc().children(ObjectId(*id)))
        .map(|c| model(&ed, c).bounds().unwrap().0.x)
        .collect();
    for k in 1..=3 {
        assert_eq!(
            xs.iter().filter(|x| **x == 3000.0 * f64::from(k)).count(),
            2
        );
    }
    ed.undo();
    assert_eq!(ed.doc().objects.len(), n);
}

#[test]
fn a_component_instance_scales_through_its_transform_and_the_others_stay() {
    let mut ed = Editor::with_sequential_ids();
    let m = cube(&mut ed);
    ed.execute(&Command::MakeComponent {
        name: "Cube".into(),
        model: None,
        faces: Vec::new(),
        edges: Vec::new(),
        objects: vec![m],
        replace: true,
    })
    .unwrap();
    let inst = ed
        .doc()
        .objects
        .values()
        .find(|o| matches!(o.shape, Shape::Component { .. }))
        .map(|o| o.id)
        .unwrap();
    let second = run(
        &mut ed,
        &[inst],
        DAffine3::from_translation(DVec3::new(4000.0, 0.0, 0.0)),
        true,
        1,
    )[0];
    let second = ObjectId(second);
    run(
        &mut ed,
        &[inst],
        DAffine3::from_scale(DVec3::new(1.0, 2.0, 1.0)),
        false,
        1,
    );
    let t = |id: ObjectId| match &ed.doc().object(id).unwrap().shape {
        Shape::Component { transform, .. } => DAffine3::from_cols_array(transform),
        _ => panic!(),
    };
    assert!((t(inst).matrix3.y_axis.y - 2.0).abs() < 1e-12);
    assert!((t(second).matrix3.y_axis.y - 1.0).abs() < 1e-12);
    assert_eq!(t(second).translation.x, 4000.0);
}

#[test]
fn a_plant_moves_in_the_plan_with_a_note_about_its_height() {
    let mut ed = Editor::with_sequential_ids();
    let p = plant(&mut ed);
    let out = ed
        .execute(&Command::TransformObjects3d {
            ids: vec![p],
            matrix: DAffine3::from_translation(DVec3::new(100.0, 200.0, 300.0)).to_cols_array(),
            copy: false,
            count: 1,
        })
        .unwrap();
    let Shape::Plant { at, height, .. } = ed.doc().object(p).unwrap().shape else {
        panic!()
    };
    assert_eq!(at, DVec2::new(5100.0, 200.0));
    assert_eq!(height, 3000.0);
    assert!(
        out.notes.iter().any(|n| n.contains("height")),
        "{:?}",
        out.notes
    );
    // A move in the plan has no such note.
    let out = ed
        .execute(&Command::TransformObjects3d {
            ids: vec![p],
            matrix: DAffine3::from_translation(DVec3::new(100.0, 0.0, 0.0)).to_cols_array(),
            copy: false,
            count: 1,
        })
        .unwrap();
    assert!(out.notes.is_empty());
}

#[test]
fn a_plant_turns_about_a_vertical_axis_but_not_about_a_tilted_one() {
    let mut ed = Editor::with_sequential_ids();
    let p = plant(&mut ed);
    run(
        &mut ed,
        &[p],
        DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2),
        false,
        1,
    );
    let Shape::Plant { at, .. } = ed.doc().object(p).unwrap().shape else {
        panic!()
    };
    assert!((at - DVec2::new(0.0, 5000.0)).length() < 1e-6);
    let before = ed.doc().clone();
    let r = ed.execute(&Command::TransformObjects3d {
        ids: vec![p],
        matrix: DAffine3::from_rotation_x(0.5).to_cols_array(),
        copy: false,
        count: 1,
    });
    assert!(matches!(r, Err(CommandError::Invalid(_))), "{r:?}");
    assert_eq!(ed.doc().object(p), before.object(p));
}

#[test]
fn copies_of_plants_go_along_the_vector() {
    let mut ed = Editor::with_sequential_ids();
    let p = plant(&mut ed);
    let created = run(
        &mut ed,
        &[p],
        DAffine3::from_translation(DVec3::new(0.0, 1000.0, 0.0)),
        true,
        5,
    );
    assert_eq!(created.len(), 5);
    for (k, id) in created.iter().enumerate() {
        let Shape::Plant { at, .. } = ed.doc().object(ObjectId(*id)).unwrap().shape else {
            panic!()
        };
        assert_eq!(at, DVec2::new(5000.0, 1000.0 * (k + 1) as f64));
    }
}

#[test]
fn refused_values_change_nothing() {
    let mut ed = Editor::with_sequential_ids();
    let m = cube(&mut ed);
    let before = ed.doc().clone();
    let bad = |xf: [f64; 12], copy: bool, count: u32, ids: Vec<ObjectId>| {
        ed_try(&before, ids, xf, copy, count)
    };
    let flat = DAffine3::from_scale(DVec3::new(1.0, 1.0, 0.0)).to_cols_array();
    assert!(bad(flat, false, 1, vec![m]).is_err());
    let ok = DAffine3::IDENTITY.to_cols_array();
    assert!(bad(ok, false, 3, vec![m]).is_err(), "a count needs copy");
    assert!(bad(ok, true, 0, vec![m]).is_err());
    assert!(bad(ok, false, 1, vec![]).is_err());
    assert!(bad(ok, false, 1, vec![ObjectId::from_num(9999)]).is_err());
    let mut nan = ok;
    nan[9] = f64::NAN;
    assert!(bad(nan, false, 1, vec![m]).is_err());
    let r = bad(ok, true, 1_000_000, vec![m]);
    assert!(r.is_err(), "too many copies are refused before any work");
}

fn ed_try(
    doc: &Document,
    ids: Vec<ObjectId>,
    matrix: [f64; 12],
    copy: bool,
    count: u32,
) -> Result<(), CommandError> {
    let mut ed = Editor::new(doc.clone());
    let n = ed.doc().objects.len();
    let r = ed
        .execute(&Command::TransformObjects3d {
            ids,
            matrix,
            copy,
            count,
        })
        .map(|_| ());
    if r.is_err() {
        assert_eq!(ed.doc().objects.len(), n);
    }
    r
}

#[test]
fn the_layer_elevation_is_part_of_the_world() {
    let mut o = Object::new(
        ObjectId::from_num(1),
        FIRST_LAYER,
        ClassId::from_num(1),
        Shape::SurfaceModel(Box::default()),
    );
    let mut notes = Vec::new();
    let e = 1000.0;
    let world = DAffine3::from_rotation_x(std::f64::consts::FRAC_PI_2);
    kadakas_core::command::transform_object3(&mut o, &world, e, &mut notes).unwrap();
    let Shape::SurfaceModel(m) = &o.shape else {
        panic!()
    };
    // The local origin is at world (0, 0, e); turned about the world x axis it goes to (0, -e, 0).
    let at = m.transform.transform_point3(DVec3::ZERO) + DVec3::new(0.0, 0.0, e);
    assert!((at - DVec3::new(0.0, -e, 0.0)).length() < 1e-6, "{at:?}");
}
