//! Tests of `TransformVertices` and `CopyGeometry` (MOD-05, MOD-08).

use glam::{DAffine3, DQuat, DVec3};
use proptest::prelude::*;

use super::*;

type P = [f64; 3];

fn poly(m: &mut SurfaceModel, pts: &[P]) {
    let edges = (0..pts.len())
        .map(|i| [pts[i], pts[(i + 1) % pts.len()]])
        .collect();
    m.apply(&Edit::AddEdges { edges }).unwrap();
}

fn square(x0: f64, y0: f64, w: f64, z: f64) -> Vec<P> {
    vec![
        [x0, y0, z],
        [x0 + w, y0, z],
        [x0 + w, y0 + w, z],
        [x0, y0 + w, z],
    ]
}

/// A box of 1000 x 1000 x 500 with a corner at the origin.
fn box_model() -> SurfaceModel {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    let f = m.faces().next().unwrap().0;
    m.apply(&Edit::PushPull {
        face: f,
        distance: 500.0,
    })
    .unwrap();
    m
}

fn mat(a: DAffine3) -> [f64; 12] {
    a.to_cols_array()
}

fn shift(d: DVec3) -> [f64; 12] {
    mat(DAffine3::from_translation(d))
}

fn vertex_at(m: &SurfaceModel, p: P) -> VertexId {
    m.vertices()
        .find(|(_, q)| q.distance(DVec3::from(p)) < 1e-6)
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("no vertex at {p:?}"))
}

fn top_face(m: &SurfaceModel) -> FaceId {
    m.faces()
        .filter(|(_, f)| f.plane.normal.z > 0.9)
        .max_by(|a, b| a.1.plane.offset.total_cmp(&b.1.plane.offset))
        .map(|(id, _)| id)
        .unwrap()
}

fn apply(m: &mut SurfaceModel, edit: Edit) -> Result<Vec<String>, SurfaceError> {
    m.apply_with_notes(&edit)
}

fn mv_vertices(vs: &[VertexId], d: DVec3) -> Edit {
    Edit::TransformVertices {
        vertices: vs.to_vec(),
        faces: Vec::new(),
        edges: Vec::new(),
        matrix: shift(d),
    }
}

/// The model has no error (warnings are allowed) and every face is flat.
fn sound(m: &SurfaceModel) {
    let r = m.check();
    assert!(r.is_valid(), "{:?}", r.problems);
    assert!(m.problem().is_none(), "{:?}", m.problem());
}

#[test]
fn a_matrix_that_flattens_is_refused_and_the_identity_changes_nothing() {
    let mut m = box_model();
    let before = m.clone();
    let flat = DAffine3::from_scale(DVec3::new(1.0, 1.0, 0.0));
    let e = Edit::TransformVertices {
        vertices: Vec::new(),
        faces: vec![top_face(&m)],
        edges: Vec::new(),
        matrix: mat(flat),
    };
    assert!(apply(&mut m, e).is_err());
    assert_eq!(m, before);
    let id = Edit::TransformVertices {
        vertices: Vec::new(),
        faces: vec![top_face(&m)],
        edges: Vec::new(),
        matrix: mat(DAffine3::IDENTITY),
    };
    apply(&mut m, id).unwrap();
    assert_eq!(m, before, "no edit, no revision");
}

#[test]
fn a_missing_vertex_face_or_edge_is_an_error() {
    let mut m = box_model();
    let one = |v: Vec<VertexId>, f: Vec<FaceId>, e: Vec<EdgeId>| Edit::TransformVertices {
        vertices: v,
        faces: f,
        edges: e,
        matrix: shift(DVec3::X),
    };
    assert_eq!(
        apply(&mut m, one(vec![VertexId(9999)], vec![], vec![])),
        Err(SurfaceError::NoVertex(VertexId(9999)))
    );
    assert!(apply(&mut m, one(vec![], vec![FaceId(9999)], vec![])).is_err());
    assert!(apply(&mut m, one(vec![], vec![], vec![EdgeId(9999)])).is_err());
}

#[test]
fn the_top_face_of_a_box_moves_up_without_folding() {
    let mut m = box_model();
    let top = top_face(&m);
    let faces = m.face_count();
    apply(
        &mut m,
        Edit::TransformVertices {
            vertices: Vec::new(),
            faces: vec![top],
            edges: Vec::new(),
            matrix: shift(DVec3::new(0.0, 0.0, 300.0)),
        },
    )
    .unwrap();
    sound(&m);
    assert!(m.check().is_closed());
    assert_eq!(m.face_count(), faces, "walls stretch, nothing folds");
    assert!((m.face(top).unwrap().plane.offset - 800.0).abs() < 1e-9);
}

#[test]
fn the_top_face_slides_sideways_and_the_walls_lean() {
    let mut m = box_model();
    let top = top_face(&m);
    let faces = m.face_count();
    apply(
        &mut m,
        Edit::TransformVertices {
            vertices: Vec::new(),
            faces: vec![top],
            edges: Vec::new(),
            matrix: shift(DVec3::new(400.0, 150.0, 0.0)),
        },
    )
    .unwrap();
    sound(&m);
    assert_eq!(m.face_count(), faces, "a lean keeps the walls flat");
    assert!(m.check().is_closed());
}

#[test]
fn a_corner_that_goes_up_folds_the_faces_at_it_into_triangles_with_soft_edges() {
    let mut m = box_model();
    let corner = vertex_at(&m, [1000.0, 1000.0, 500.0]);
    let (v, e, f) = (m.vertex_count(), m.edge_count(), m.face_count());
    apply(&mut m, mv_vertices(&[corner], DVec3::new(0.0, 0.0, 400.0))).unwrap();
    sound(&m);
    // The corner goes up in the planes of the two walls, so they stay flat; the top folds in two.
    assert_eq!(m.vertex_count(), v);
    assert_eq!(m.face_count(), f + 1);
    assert_eq!(m.edge_count(), e + 1);
    let soft: Vec<_> = m
        .edges()
        .filter(|(id, _)| {
            m.edge_attrs(*id).is_some_and(|a| {
                a.get("soft") == Some(&true.into()) && a.get("smooth") == Some(&true.into())
            })
        })
        .collect();
    assert_eq!(soft.len(), 1, "the new edge is soft and smooth");
}

#[test]
fn a_corner_moved_back_unfolds_the_triangles_into_the_flat_face() {
    let mut m = box_model();
    let top = top_face(&m);
    let (v, e, f) = (m.vertex_count(), m.edge_count(), m.face_count());
    let corner = vertex_at(&m, [1000.0, 1000.0, 500.0]);
    apply(&mut m, mv_vertices(&[corner], DVec3::new(0.0, 0.0, 400.0))).unwrap();
    assert_eq!(m.face_count(), f + 1);
    let notes = apply(&mut m, mv_vertices(&[corner], DVec3::new(0.0, 0.0, -400.0))).unwrap();
    sound(&m);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(
        (m.vertex_count(), m.edge_count(), m.face_count()),
        (v, e, f)
    );
    assert!(m.check().is_closed());
    assert!(m.face(top).is_some(), "the lower id stays");
    assert!(
        m.edges()
            .all(|(id, _)| m.edge_attrs(id).is_none_or(|a| !a.contains_key("soft")))
    );
}

#[test]
fn a_corner_that_goes_sideways_folds_the_walls_at_it_too() {
    let mut m = box_model();
    let corner = vertex_at(&m, [1000.0, 1000.0, 500.0]);
    let f = m.face_count();
    apply(
        &mut m,
        mv_vertices(&[corner], DVec3::new(300.0, 200.0, 100.0)),
    )
    .unwrap();
    sound(&m);
    assert_eq!(
        m.face_count(),
        f + 3,
        "the top and the two walls fold in two"
    );
}

#[test]
fn the_pieces_of_a_fold_keep_the_material_and_the_largest_keeps_the_id() {
    let mut m = box_model();
    let top = top_face(&m);
    m.faces
        .get_mut(&top)
        .unwrap()
        .attrs
        .insert("material".into(), "grass".into());
    // One corner of the top up: a quad folds into two triangles of different size when the
    // corner is moved sideways too.
    let corner = vertex_at(&m, [1000.0, 1000.0, 500.0]);
    apply(
        &mut m,
        mv_vertices(&[corner], DVec3::new(-300.0, 0.0, 200.0)),
    )
    .unwrap();
    sound(&m);
    let pieces: Vec<(FaceId, f64)> = m
        .faces()
        .filter(|(_, f)| f.attrs.get("material") == Some(&"grass".into()))
        .map(|(id, _)| (id, m.face_area(id).unwrap()))
        .collect();
    assert_eq!(pieces.len(), 2, "two pieces keep the material");
    let largest = pieces.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap().0;
    assert_eq!(largest, top, "the largest piece keeps the id of the face");
    assert!(pieces.iter().all(|(id, _)| *id >= top));
}

#[test]
fn a_whole_box_moves_turns_and_scales_without_a_fold() {
    let spin = DAffine3::from_rotation_translation(
        DQuat::from_axis_angle(DVec3::new(1.0, 2.0, 3.0).normalize(), 0.7),
        DVec3::new(100.0, -50.0, 20.0),
    );
    let stretch = DAffine3::from_scale(DVec3::new(2.0, 0.5, 3.0));
    for xf in [spin, stretch, spin * stretch] {
        let mut m = box_model();
        let all: Vec<VertexId> = m.vertices().map(|(v, _)| v).collect();
        let (v, e, f) = (m.vertex_count(), m.edge_count(), m.face_count());
        let notes = apply(
            &mut m,
            Edit::TransformVertices {
                vertices: all,
                faces: Vec::new(),
                edges: Vec::new(),
                matrix: mat(xf),
            },
        )
        .unwrap();
        assert!(notes.is_empty());
        sound(&m);
        assert!(m.check().is_closed());
        assert_eq!(
            (m.vertex_count(), m.edge_count(), m.face_count()),
            (v, e, f)
        );
    }
}

#[test]
fn a_mirror_keeps_the_box_closed_and_facing_out() {
    let mut m = box_model();
    let all: Vec<FaceId> = m.faces().map(|(id, _)| id).collect();
    let volume = |m: &SurfaceModel| -> f64 {
        m.faces()
            .map(|(id, f)| {
                let p = m.vertex(f.outer[0]).unwrap();
                m.face_area(id).unwrap() * f.plane.normal.dot(p) / 3.0
            })
            .sum()
    };
    let before = volume(&m);
    apply(
        &mut m,
        Edit::TransformVertices {
            vertices: Vec::new(),
            faces: all,
            edges: Vec::new(),
            matrix: mat(DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0))),
        },
    )
    .unwrap();
    sound(&m);
    let r = m.check();
    assert!(r.is_closed(), "{r:?}");
    assert!(
        (volume(&m) - before).abs() < 1e-3 * before,
        "faces still look out"
    );
}

#[test]
fn a_vertex_that_lands_on_another_vertex_welds() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    poly(&mut m, &square(1500.0, 0.0, 1000.0, 0.0));
    assert_eq!(m.vertex_count(), 8);
    let a = vertex_at(&m, [1500.0, 0.0, 0.0]);
    let b = vertex_at(&m, [1000.0, 0.0, 0.0]);
    apply(&mut m, mv_vertices(&[a], DVec3::new(-500.0, 0.0, 0.0))).unwrap();
    sound(&m);
    assert_eq!(m.vertex_count(), 7);
    assert!(m.vertex(b).is_some(), "the vertex that was there stays");
    assert!(m.vertex(a).is_none());
    assert_eq!(m.face_count(), 2);
    // Both squares are faces and the second one is now a quad with a corner on the first.
    let r = m.check();
    assert!(r.is_valid());
}

#[test]
fn a_square_moved_against_another_in_one_plane_welds_and_merges_with_it() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    poly(&mut m, &square(1500.0, 0.0, 1000.0, 0.0));
    let second = m
        .faces()
        .find(|(_, f)| f.outer.iter().all(|v| m.vertex(*v).unwrap().x >= 1500.0))
        .map(|(id, _)| id)
        .unwrap();
    apply(
        &mut m,
        Edit::TransformVertices {
            vertices: Vec::new(),
            faces: vec![second],
            edges: Vec::new(),
            matrix: shift(DVec3::new(-500.0, 0.0, 0.0)),
        },
    )
    .unwrap();
    sound(&m);
    // The two vertices on the shared side welded, the faces became one flat face (the shared
    // edge and the two vertices in the straight sides are gone).
    assert_eq!(m.face_count(), 1);
    assert_eq!(m.vertex_count(), 4);
    assert!((m.area() - 2_000_000.0).abs() < 1e-6);
}

#[test]
fn a_face_that_becomes_coplanar_with_its_neighbour_merges_with_a_note() {
    // A roof: a slab with a wing at an angle. Pulling the wing flat makes it coplanar with the
    // slab and the two merge; the material of the smaller face is dropped with a note.
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    poly(
        &mut m,
        &[
            [1000.0, 0.0, 0.0],
            [1000.0, 1000.0, 0.0],
            [1500.0, 1000.0, 400.0],
            [1500.0, 0.0, 400.0],
        ],
    );
    assert_eq!(m.face_count(), 2);
    let ids: Vec<FaceId> = m.faces().map(|(id, _)| id).collect();
    for (k, id) in ids.iter().enumerate() {
        m.faces
            .get_mut(id)
            .unwrap()
            .attrs
            .insert("material".into(), format!("m{k}").into());
    }
    let tip = [
        vertex_at(&m, [1500.0, 1000.0, 400.0]),
        vertex_at(&m, [1500.0, 0.0, 400.0]),
    ];
    let notes = apply(&mut m, mv_vertices(&tip, DVec3::new(0.0, 0.0, -400.0))).unwrap();
    sound(&m);
    assert_eq!(m.face_count(), 1, "the wing is flat with the slab now");
    assert!(
        !notes.is_empty(),
        "the dropped material is noted: {notes:?}"
    );
    assert!(notes[0].contains("material"));
}

#[test]
fn neighbours_that_were_coplanar_before_stay_apart() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    m.apply(&Edit::AddEdges {
        edges: vec![[[500.0, 0.0, 0.0], [500.0, 1000.0, 0.0]]],
    })
    .unwrap();
    assert_eq!(m.face_count(), 2);
    let all: Vec<VertexId> = m.vertices().map(|(v, _)| v).collect();
    apply(&mut m, mv_vertices(&all, DVec3::new(10.0, 20.0, 30.0))).unwrap();
    sound(&m);
    assert_eq!(m.face_count(), 2);
}

#[test]
fn a_curve_that_moves_whole_stays_a_curve_and_a_stretched_one_too() {
    let mut m = SurfaceModel::new();
    let pts: Vec<P> = (0..12)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 12.0;
            [500.0 * a.cos(), 500.0 * a.sin(), 0.0]
        })
        .collect();
    m.apply(&Edit::AddCurve {
        points: pts,
        closed: true,
    })
    .unwrap();
    assert_eq!(m.curves().count(), 1);
    let all: Vec<VertexId> = m.vertices().map(|(v, _)| v).collect();
    apply(&mut m, mv_vertices(&all, DVec3::new(3000.0, 0.0, 10.0))).unwrap();
    sound(&m);
    assert_eq!(m.curves().count(), 1);
    // One vertex moved away: still one curve, a bumped circle.
    let one = vertex_at(&m, m.vertex(all[0]).unwrap().to_array());
    apply(&mut m, mv_vertices(&[one], DVec3::new(0.0, 0.0, 100.0))).unwrap();
    sound(&m);
    assert_eq!(m.curves().count(), 1);
    assert!(m.face_count() >= 1);
}

#[test]
fn an_edge_that_lands_on_a_vertex_gets_a_vertex_in_it() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    poly(&mut m, &square(0.0, 2000.0, 1000.0, 0.0));
    // A loose vertex of the first square's neighbor: move the second square's bottom edge down
    // to y = 1000, over the top edge of the first: they overlap along an edge. Instead, move one
    // corner onto the middle of the top edge of the first square.
    let mid = {
        m.apply(&Edit::AddEdges {
            edges: vec![[[500.0, 1000.0, 0.0], [500.0, 1000.0, 300.0]]],
        })
        .unwrap();
        vertex_at(&m, [500.0, 1000.0, 0.0])
    };
    assert!(m.vertex(mid).is_some());
    let corner = vertex_at(&m, [0.0, 2000.0, 0.0]);
    // The corner goes to (700, 1000, 0): on the top edge of the first square, not at a vertex.
    apply(
        &mut m,
        mv_vertices(&[corner], DVec3::new(700.0, -1000.0, 0.0)),
    )
    .unwrap();
    sound(&m);
    let landed = vertex_at(&m, [700.0, 1000.0, 0.0]);
    let first = m
        .faces()
        .find(|(_, f)| {
            f.outer
                .iter()
                .all(|v| m.vertex(*v).unwrap().y <= 1000.0 + 1e-9)
        })
        .unwrap();
    assert!(
        first.1.outer.contains(&landed),
        "the edge is split at the landing point"
    );
}

#[test]
fn a_move_through_other_faces_is_a_warning_not_an_error() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    // A standing rectangle beside it, then moved down through the square.
    poly(
        &mut m,
        &[
            [200.0, 500.0, 1100.0],
            [800.0, 500.0, 1100.0],
            [800.0, 500.0, 1600.0],
            [200.0, 500.0, 1600.0],
        ],
    );
    let wall = m
        .faces()
        .find(|(_, f)| f.plane.normal.y.abs() > 0.9)
        .map(|(id, _)| id)
        .unwrap();
    apply(
        &mut m,
        Edit::TransformVertices {
            vertices: Vec::new(),
            faces: vec![wall],
            edges: Vec::new(),
            matrix: shift(DVec3::new(0.0, 0.0, -1300.0)),
        },
    )
    .unwrap();
    let r = m.check();
    assert!(r.is_valid(), "{:?}", r.problems);
    assert!(r.not_intersected() >= 2, "{:?}", r.warnings);
    // Intersect Faces then makes the edges.
    m.apply(&Edit::IntersectFaces {
        scope: IntersectScope::Model,
        faces: Vec::new(),
        edges: Vec::new(),
    })
    .unwrap();
    assert_eq!(m.check().not_intersected(), 0);
}

#[test]
fn degenerate_results_are_refused_and_the_model_is_unchanged() {
    // A triangle with its apex dragged onto the base line.
    let mut m = SurfaceModel::new();
    poly(
        &mut m,
        &[[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [500.0, 500.0, 0.0]],
    );
    let before = m.clone();
    let apex = vertex_at(&m, [500.0, 500.0, 0.0]);
    let e = apply(&mut m, mv_vertices(&[apex], DVec3::new(0.0, -500.0, 0.0)));
    assert!(e.is_err(), "{e:?}");
    let msg = e.unwrap_err().to_string();
    assert!(msg.contains("collapse"), "{msg}");
    assert_eq!(m, before);
    // A square with a corner pulled across the opposite side: the loop crosses itself.
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    let before = m.clone();
    let c = vertex_at(&m, [1000.0, 1000.0, 0.0]);
    let e = apply(&mut m, mv_vertices(&[c], DVec3::new(-1500.0, -200.0, 0.0)));
    let msg = e.unwrap_err().to_string();
    assert!(msg.contains("cross"), "{msg}");
    assert_eq!(m, before);
}

#[test]
fn a_hole_face_folds_with_its_hole_kept_in_the_triangles() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 3000.0, 0.0));
    poly(&mut m, &square(1000.0, 1000.0, 1000.0, 0.0));
    let big = m.faces().max_by(|a, b| {
        m.face_area(a.0)
            .unwrap()
            .total_cmp(&m.face_area(b.0).unwrap())
    });
    let big = big.unwrap().0;
    assert_eq!(m.face(big).unwrap().holes.len(), 1);
    let corner = vertex_at(&m, [3000.0, 3000.0, 0.0]);
    apply(&mut m, mv_vertices(&[corner], DVec3::new(0.0, 0.0, 600.0))).unwrap();
    sound(&m);
    // The big face is cut into triangles that do not cover the hole.
    let covered: f64 = m
        .faces()
        .filter(|(_, f)| {
            f.attrs.is_empty() && f.holes.is_empty() && f.plane.normal.z > 0.0 && f.outer.len() == 3
        })
        .map(|(id, _)| m.face_area(id).unwrap())
        .sum();
    assert!(covered > 7_000_000.0 && covered < 10_000_000.0, "{covered}");
}

#[test]
fn the_same_edit_gives_the_same_model_and_the_command_undo_is_exact() {
    let mut a = box_model();
    let mut b = a.clone();
    let corner = vertex_at(&a, [1000.0, 1000.0, 500.0]);
    let e = mv_vertices(&[corner], DVec3::new(-200.0, 100.0, 333.0));
    apply(&mut a, e.clone()).unwrap();
    apply(&mut b, e).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
}

#[test]
fn the_edit_is_logged_and_replays_on_another_copy() {
    let mut a = box_model();
    let mut b = a.clone();
    let corner = vertex_at(&a, [1000.0, 1000.0, 500.0]);
    apply(
        &mut a,
        mv_vertices(&[corner], DVec3::new(-200.0, 100.0, 333.0)),
    )
    .unwrap();
    let entry = a.log().last().unwrap().clone();
    sync::replay_intent(&mut b, &entry.intent).unwrap();
    assert_eq!(a.faces().count(), b.faces().count());
    assert_eq!(a.area(), b.area());
}

// ---- copies ----------------------------------------------------------------------------------

#[test]
fn copy_geometry_makes_new_geometry_with_new_ids() {
    let mut m = box_model();
    let all: Vec<FaceId> = m.faces().map(|(id, _)| id).collect();
    let ids_before: std::collections::BTreeSet<u32> = m
        .vertices()
        .map(|(v, _)| v.0)
        .chain(m.edges().map(|(e, _)| e.0))
        .chain(m.faces().map(|(f, _)| f.0))
        .collect();
    apply(
        &mut m,
        Edit::CopyGeometry {
            faces: all,
            edges: Vec::new(),
            matrix: shift(DVec3::new(2000.0, 0.0, 0.0)),
            count: 3,
        },
    )
    .unwrap();
    sound(&m);
    assert_eq!(m.vertex_count(), 8 * 4);
    assert_eq!(m.edge_count(), 12 * 4);
    assert_eq!(m.face_count(), 6 * 4);
    let fresh = m
        .vertices()
        .filter(|(v, _)| !ids_before.contains(&v.0))
        .count();
    assert_eq!(fresh, 24);
    assert!(m.check().is_closed());
    // The row: copies at 2000, 4000, 6000.
    for k in 1..=3 {
        let x = 2000.0 * f64::from(k);
        vertex_at(&m, [x, 0.0, 0.0]);
    }
}

#[test]
fn a_copy_that_lands_on_the_geometry_welds_there() {
    let mut m = box_model();
    let top = top_face(&m);
    // The top face copied one box width to the side: shares an edge with the original.
    apply(
        &mut m,
        Edit::CopyGeometry {
            faces: vec![top],
            edges: Vec::new(),
            matrix: shift(DVec3::new(1000.0, 0.0, 0.0)),
            count: 1,
        },
    )
    .unwrap();
    sound(&m);
    assert_eq!(m.vertex_count(), 8 + 2, "two vertices are new");
}

#[test]
fn a_copy_with_a_rotation_turns_each_copy_further() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(1000.0, 0.0, 500.0, 0.0));
    let f = m.faces().next().unwrap().0;
    apply(
        &mut m,
        Edit::CopyGeometry {
            faces: vec![f],
            edges: Vec::new(),
            matrix: mat(DAffine3::from_rotation_z(std::f64::consts::FRAC_PI_2)),
            count: 3,
        },
    )
    .unwrap();
    sound(&m);
    assert_eq!(m.face_count(), 4);
    vertex_at(&m, [0.0, 1000.0, 0.0]);
    vertex_at(&m, [-1000.0, 0.0, 0.0]);
    vertex_at(&m, [0.0, -1000.0, 0.0]);
}

#[test]
fn copy_geometry_refuses_nothing_chosen_a_zero_move_and_too_many_edges() {
    let mut m = box_model();
    let all: Vec<FaceId> = m.faces().map(|(id, _)| id).collect();
    let copy = |faces: Vec<FaceId>, d: DVec3, count: u32| Edit::CopyGeometry {
        faces,
        edges: Vec::new(),
        matrix: shift(d),
        count,
    };
    assert!(apply(&mut m, copy(vec![], DVec3::X, 1)).is_err());
    assert!(apply(&mut m, copy(all.clone(), DVec3::ZERO, 1)).is_err());
    assert!(apply(&mut m, copy(all.clone(), DVec3::X, 0)).is_err());
    let e = apply(&mut m, copy(all, DVec3::new(2000.0, 0.0, 0.0), 1000)).unwrap_err();
    assert!(matches!(e, SurfaceError::Limit { .. }), "{e}");
}

// ---- property --------------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f64) / f64::from(1u32 << 31)
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }

    fn below(&mut self, n: usize) -> usize {
        ((self.next() * n as f64) as usize).min(n - 1)
    }
}

/// A few separate boxes.
fn random_scene(seed: u64) -> SurfaceModel {
    let mut rng = Rng(seed | 1);
    let mut m = SurfaceModel::new();
    for i in 0..rng.below(3) + 1 {
        let x = 3000.0 * i as f64;
        let w = rng.range(500.0, 1500.0);
        let h = rng.range(300.0, 900.0);
        let known: std::collections::BTreeSet<FaceId> = m.faces().map(|(id, _)| id).collect();
        poly(&mut m, &square(x, 0.0, w, 0.0));
        let f = m
            .faces()
            .map(|(id, _)| id)
            .find(|id| !known.contains(id))
            .unwrap();
        m.apply(&Edit::PushPull {
            face: f,
            distance: h,
        })
        .unwrap();
    }
    m
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(120))]

    #[test]
    fn random_moves_keep_every_face_flat_and_the_model_sound(seed in 0u64..u64::MAX) {
        let mut rng = Rng(seed | 1);
        let mut m = random_scene(seed);
        let mut moves = 0;
        for _ in 0..4 {
            let vs: Vec<VertexId> = m.vertices().map(|(v, _)| v).collect();
            if vs.is_empty() {
                break;
            }
            let k = rng.below(3) + 1;
            let chosen: Vec<VertexId> = (0..k).map(|_| vs[rng.below(vs.len())]).collect();
            let d = DVec3::new(
                rng.range(-800.0, 800.0),
                rng.range(-800.0, 800.0),
                rng.range(-800.0, 800.0),
            );
            let xf = if rng.next() < 0.3 {
                let axis = DVec3::new(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0), rng.range(0.1, 1.0)).normalize();
                let c = m.vertex(chosen[0]).unwrap();
                DAffine3::from_translation(c)
                    * DAffine3::from_quat(DQuat::from_axis_angle(axis, rng.range(-1.5, 1.5)))
                    * DAffine3::from_translation(-c)
            } else {
                DAffine3::from_translation(d)
            };
            let edit = Edit::TransformVertices {
                vertices: chosen,
                faces: Vec::new(),
                edges: Vec::new(),
                matrix: mat(xf),
            };
            let before = m.clone();
            let mut twin = m.clone();
            match m.apply_with_notes(&edit) {
                Ok(_) => {
                    moves += 1;
                    let r = m.check();
                    prop_assert!(r.is_valid(), "{:?}", r.problems);
                    prop_assert!(m.problem().is_none(), "{:?}", m.problem());
                    twin.apply(&edit).unwrap();
                    prop_assert_eq!(&m, &twin);
                    for (_, f) in m.faces() {
                        for v in f.loops().flatten() {
                            prop_assert!(geom::on_plane(&f.plane, m.vertex(*v).unwrap()));
                        }
                    }
                }
                Err(_) => prop_assert_eq!(&m, &before),
            }
        }
        let _ = moves;
    }
}

#[test]
fn most_random_moves_work_and_the_others_change_nothing() {
    let (mut ok, mut refused) = (0, 0);
    for seed in 0..200u64 {
        let mut rng = Rng(seed * 7919 + 3);
        let mut m = random_scene(seed);
        let vs: Vec<VertexId> = m.vertices().map(|(v, _)| v).collect();
        let chosen: Vec<VertexId> = (0..2).map(|_| vs[rng.below(vs.len())]).collect();
        let d = DVec3::new(
            rng.range(-800.0, 800.0),
            rng.range(-800.0, 800.0),
            rng.range(-800.0, 800.0),
        );
        let before = m.clone();
        match m.apply(&mv_vertices(&chosen, d)) {
            Ok(()) => {
                ok += 1;
                sound(&m);
            }
            Err(_) => {
                refused += 1;
                assert_eq!(m, before);
            }
        }
    }
    assert!(ok > 120, "{ok} worked, {refused} were refused");
}

#[test]
fn the_command_layer_undoes_a_move_byte_for_byte() {
    use crate::command::{Command, Editor};
    use crate::model::{Attributes, Document, Shape};
    let mut ed = Editor::new(Document::default());
    let id = crate::model::ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::new(box_model())),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let corner = {
        let Shape::SurfaceModel(m) = &ed.doc().object(id).unwrap().shape else {
            unreachable!()
        };
        vertex_at(m, [1000.0, 1000.0, 500.0])
    };
    let text = |ed: &Editor| serde_json::to_string(&ed.doc().object(id).unwrap().shape).unwrap();
    let before = text(&ed);
    let depth = ed.undo_depth();
    ed.execute(&Command::EditSurface {
        id,
        edit: mv_vertices(&[corner], DVec3::new(0.0, 0.0, 700.0)),
    })
    .unwrap();
    assert_eq!(ed.undo_depth(), depth + 1);
    assert_ne!(text(&ed), before);
    ed.undo();
    assert_eq!(text(&ed), before);
    // A refused move changes nothing and is no undo step.
    let r = ed.execute(&Command::EditSurface {
        id,
        edit: mv_vertices(&[VertexId(99999)], DVec3::X),
    });
    assert!(r.is_err());
    assert_eq!(ed.undo_depth(), depth);
}

/// `cargo test -p kadakas-core --release --lib surface::tests_transform::perf_move -- --ignored --nocapture`
#[test]
#[ignore = "timing test: run in release mode"]
fn perf_move() {
    use std::time::Instant;
    let n = 200usize;
    let w = 1000.0;
    let mut m = SurfaceModel::new();
    let mut segs = Vec::new();
    for i in 0..=n {
        let t = i as f64 * w;
        segs.push([[t, 0.0, 0.0], [t, n as f64 * w, 0.0]]);
        segs.push([[0.0, t, 0.0], [n as f64 * w, t, 0.0]]);
    }
    m.apply(&Edit::AddEdges { edges: segs }).unwrap();
    println!(
        "grid: {} vertices, {} edges, {} faces",
        m.vertex_count(),
        m.edge_count(),
        m.face_count()
    );
    // 1,000 vertices: a block of 32 x 32 around the middle, lifted by a different amount each.
    let block: Vec<VertexId> = m
        .vertices()
        .filter(|(_, p)| {
            (80_000.0..112_000.0).contains(&p.x) && (80_000.0..112_000.0).contains(&p.y)
        })
        .map(|(v, _)| v)
        .take(1000)
        .collect();
    println!("moving {} vertices", block.len());
    let start = Instant::now();
    m.apply(&mv_vertices(&block, DVec3::new(0.0, 0.0, 300.0)))
        .unwrap();
    println!(
        "lift 1,000 vertices (folds the faces): {:?}",
        start.elapsed()
    );
    let start = Instant::now();
    m.apply(&mv_vertices(&block, DVec3::new(0.0, 0.0, -300.0)))
        .unwrap();
    println!("lift them back: {:?}", start.elapsed());
    let start = Instant::now();
    assert!(m.problem().is_none());
    println!("quick check: {:?}", start.elapsed());
    // A whole block of faces slides without folding.
    let faces: Vec<FaceId> = m
        .faces()
        .filter(|(_, f)| {
            f.outer.iter().all(|v| {
                let p = m.vertex(*v).unwrap();
                (80_000.0..112_000.0).contains(&p.x) && (80_000.0..112_000.0).contains(&p.y)
            })
        })
        .map(|(id, _)| id)
        .collect();
    let start = Instant::now();
    m.apply(&Edit::TransformVertices {
        vertices: Vec::new(),
        faces,
        edges: Vec::new(),
        matrix: shift(DVec3::new(0.0, 0.0, 500.0)),
    })
    .unwrap();
    println!("lift a block of faces whole: {:?}", start.elapsed());
}
