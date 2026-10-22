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
fn a_square_moved_against_another_in_one_plane_welds_and_both_stay_faces() {
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
    // The two vertices on the shared side welded and the two sides became one edge. The faces
    // are coplanar now but stay two faces: a move never merges faces.
    assert_eq!(m.face_count(), 2);
    assert_eq!(m.vertex_count(), 6);
    assert_eq!(m.edge_count(), 7);
    assert!((m.area() - 2_000_000.0).abs() < 1e-6);
}

#[test]
fn a_face_that_becomes_coplanar_with_its_neighbour_stays_a_face() {
    // A roof: a slab with a wing at an angle. Pulling the wing flat makes it coplanar with the
    // slab; the two stay apart with their materials, and the edge between them stays.
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
    let edges = m.edge_count();
    let notes = apply(&mut m, mv_vertices(&tip, DVec3::new(0.0, 0.0, -400.0))).unwrap();
    sound(&m);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(
        m.face_count(),
        2,
        "the wing is flat with the slab now, and apart"
    );
    assert_eq!(m.edge_count(), edges);
    for (k, id) in ids.iter().enumerate() {
        assert_eq!(
            m.face(*id).unwrap().attrs.get("material"),
            Some(&format!("m{k}").into())
        );
    }
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
    // The big face is cut into triangles that do not cover the hole: seen from above they cover
    // the square less the hole exactly, and the hole's own face is still there.
    let inner = face_with(&m, &[[1000.0, 1000.0, 0.0], [2000.0, 2000.0, 0.0]]);
    let seen: f64 = m
        .faces()
        .filter(|(id, _)| *id != inner)
        .map(|(id, f)| m.face_area(id).unwrap() * f.plane.normal.z)
        .sum();
    assert!((seen - 8_000_000.0).abs() < 1e-6, "{seen}");
    assert!(
        m.faces()
            .filter(|(id, _)| *id != inner)
            .all(|(_, f)| f.outer.len() == 3 && f.holes.is_empty())
    );
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
    assert_same_geometry(&a, &b);
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
    // One vertex (the fixed cost of an edit), up and back.
    let one = vertex_at(&m, [20_000.0, 20_000.0, 0.0]);
    let start = Instant::now();
    m.apply(&mv_vertices(&[one], DVec3::new(0.0, 0.0, 300.0)))
        .unwrap();
    println!("lift 1 vertex: {:?}", start.elapsed());
    let start = Instant::now();
    m.apply(&mv_vertices(&[one], DVec3::new(0.0, 0.0, -300.0)))
        .unwrap();
    println!("lift it back: {:?}", start.elapsed());
    assert_eq!(m.face_count(), n * n, "the cells unfold and stay apart");
    // (Back down, the folds unfold: the four faces are as they were.)
    let other = vertex_at(&m, [30_000.0, 30_000.0, 0.0]);
    let start = Instant::now();
    m.apply(&mv_vertices(&[other], DVec3::new(100.0, 50.0, 0.0)))
        .unwrap();
    println!("slide 1 vertex in its plane: {:?}", start.elapsed());
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
    assert_eq!(m.face_count(), n * n);
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

// ---- review round (MOD-05 round A) ------------------------------------------------------------

/// The face whose outer loop has a vertex at each of the points (in any order).
fn face_with(m: &SurfaceModel, corners: &[P]) -> FaceId {
    m.faces()
        .find(|(_, f)| {
            corners.iter().all(|c| {
                f.outer
                    .iter()
                    .any(|v| m.vertex(*v).unwrap().distance((*c).into()) < 1e-6)
            })
        })
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("no face at {corners:?}"))
}

fn mv_faces(fs: &[FaceId], d: DVec3) -> Edit {
    Edit::TransformVertices {
        vertices: Vec::new(),
        faces: fs.to_vec(),
        edges: Vec::new(),
        matrix: shift(d),
    }
}

/// A loop from its lowest id.
fn canon(l: &[VertexId]) -> Vec<VertexId> {
    let i = (0..l.len()).min_by_key(|&i| l[i]).unwrap();
    let mut out = l.to_vec();
    out.rotate_left(i);
    out
}

/// The same vertices, edges and faces (with their ids, loops, attributes, and positions and planes
/// within 1e-9), the same edge attributes and curves.
fn assert_same_geometry(a: &SurfaceModel, b: &SurfaceModel) {
    let va: Vec<_> = a.vertices().collect();
    let vb: Vec<_> = b.vertices().collect();
    assert_eq!(va.len(), vb.len(), "vertex count");
    for ((ia, pa), (ib, pb)) in va.iter().zip(&vb) {
        assert_eq!(ia, ib);
        assert!(pa.distance(*pb) <= 1e-9, "vertex {ia}: {pa} vs {pb}");
    }
    assert_eq!(
        a.edges().collect::<Vec<_>>(),
        b.edges().collect::<Vec<_>>(),
        "edges"
    );
    assert_eq!(a.edge_attrs, b.edge_attrs, "edge attributes");
    assert_eq!(a.curves, b.curves, "curves");
    let fa: Vec<_> = a.faces().collect();
    let fb: Vec<_> = b.faces().collect();
    assert_eq!(
        fa.iter().map(|f| f.0).collect::<Vec<_>>(),
        fb.iter().map(|f| f.0).collect::<Vec<_>>(),
        "face ids"
    );
    for ((id, x), (_, y)) in fa.iter().zip(&fb) {
        assert_eq!(canon(&x.outer), canon(&y.outer), "outer loop of face {id}");
        let holes = |f: &Face| {
            let mut h: Vec<Vec<VertexId>> = f.holes.iter().map(|l| canon(l)).collect();
            h.sort();
            h
        };
        assert_eq!(holes(x), holes(y), "holes of face {id}");
        assert!(
            x.plane.normal.distance(y.plane.normal) <= 1e-9
                && (x.plane.offset - y.plane.offset).abs() <= 1e-9,
            "plane of face {id}: {:?} vs {:?}",
            x.plane,
            y.plane
        );
        assert_eq!(x.attrs, y.attrs, "attributes of face {id}");
        assert_eq!(x.extra, y.extra, "fields of face {id}");
    }
}

#[test]
fn a_face_moved_into_a_coplanar_face_is_refused() {
    // Fable (1): B moved whole into A, and one corner of B moved into A.
    let scene = || {
        let mut m = SurfaceModel::new();
        poly(&mut m, &square(0.0, 0.0, 3000.0, 0.0));
        poly(&mut m, &square(5000.0, 0.0, 1000.0, 0.0));
        m
    };
    let mut m = scene();
    let before = m.clone();
    let b = face_with(&m, &[[5000.0, 0.0, 0.0], [6000.0, 1000.0, 0.0]]);
    let e = apply(&mut m, mv_faces(&[b], DVec3::new(-4000.0, 1000.0, 0.0)));
    let msg = e.unwrap_err().to_string();
    assert!(msg.contains("overlap"), "{msg}");
    assert_eq!(m, before);
    let mut m = scene();
    let corner = vertex_at(&m, [5000.0, 0.0, 0.0]);
    let e = apply(
        &mut m,
        mv_vertices(&[corner], DVec3::new(-3500.0, 1500.0, 0.0)),
    );
    assert!(e.is_err(), "{e:?}");
    assert_eq!(m, before);
    // Every corner of B moved into A, one at a time: refused too, never an error of `check`.
    for c in square(5000.0, 0.0, 1000.0, 0.0) {
        let mut m = scene();
        let v = vertex_at(&m, c);
        let to = DVec3::new(1500.0, 1500.0, 0.0) - DVec3::from(c);
        match apply(&mut m, mv_vertices(&[v], to)) {
            Ok(_) => sound(&m),
            Err(_) => assert_eq!(m, before),
        }
    }
}

#[test]
fn an_edge_slid_along_another_becomes_one_edge_and_the_faces_stay_apart() {
    // Fable (2): B's bottom edge slides onto a part of A's top edge.
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    poly(&mut m, &square(500.0, 2000.0, 1000.0, 0.0));
    let b = face_with(&m, &[[500.0, 2000.0, 0.0], [1500.0, 3000.0, 0.0]]);
    let area = m.area();
    apply(&mut m, mv_faces(&[b], DVec3::new(0.0, -1000.0, 0.0))).unwrap();
    let r = m.check();
    assert!(r.is_valid(), "{:?}", r.problems);
    assert!(
        !r.problems
            .iter()
            .any(|p| matches!(p, check::Problem::DoubleEdge(..)))
    );
    sound(&m);
    assert_eq!(m.face_count(), 2, "the coplanar faces stay apart");
    assert!((m.area() - area).abs() < 1e-6);
    // No two edges join the same vertices.
    let keys: std::collections::BTreeSet<_> = m.edges().map(|(_, e)| e.key()).collect();
    assert_eq!(keys.len(), m.edge_count());
}

#[test]
fn a_fold_does_not_depend_on_the_order_the_square_was_drawn_in() {
    // Fable (3): the same square from (0, 0) and from (1000, 1000); the corner (1000, 1000) up.
    let diagonal = |pts: Vec<P>| {
        let mut m = SurfaceModel::new();
        poly(&mut m, &pts);
        let c = vertex_at(&m, [1000.0, 1000.0, 0.0]);
        apply(&mut m, mv_vertices(&[c], DVec3::new(0.0, 0.0, 400.0))).unwrap();
        sound(&m);
        let soft: Vec<[[i64; 3]; 2]> = m
            .edges()
            .filter(|(id, _)| m.edge_attrs(*id).is_some_and(|a| a.contains_key("soft")))
            .map(|(_, e)| {
                let mut ends = e.v.map(|v| {
                    m.vertex(v)
                        .unwrap()
                        .to_array()
                        .map(|x| (x * 1000.0).round() as i64)
                });
                ends.sort();
                ends
            })
            .collect();
        assert_eq!(soft.len(), 1);
        soft[0]
    };
    let a = diagonal(square(0.0, 0.0, 1000.0, 0.0));
    let mut b_pts = square(0.0, 0.0, 1000.0, 0.0);
    b_pts.rotate_left(2);
    let b = diagonal(b_pts.clone());
    b_pts.reverse();
    let c = diagonal(b_pts);
    assert_eq!(a, b);
    assert_eq!(a, c);
    // The diagonal that bends the least does not touch the lifted corner.
    assert_eq!(a, [[0, 1_000_000, 0], [1_000_000, 0, 0]]);
}

#[test]
fn the_fold_of_an_n_gon_does_not_depend_on_the_ids_either() {
    // A hexagon drawn from each of its corners, in both directions; one corner up.
    let pts: Vec<P> = (0..6)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 6.0 + 0.2;
            [1000.0 * a.cos(), 700.0 * a.sin(), 0.0]
        })
        .collect();
    let lift = pts[1];
    let shape = |order: Vec<P>| {
        let mut m = SurfaceModel::new();
        poly(&mut m, &order);
        let c = vertex_at(&m, lift);
        apply(&mut m, mv_vertices(&[c], DVec3::new(0.0, 0.0, 300.0))).unwrap();
        sound(&m);
        let mut tris: Vec<Vec<[i64; 3]>> = m
            .faces()
            .map(|(_, f)| {
                let mut t: Vec<[i64; 3]> = f
                    .outer
                    .iter()
                    .map(|v| {
                        m.vertex(*v)
                            .unwrap()
                            .to_array()
                            .map(|x| (x * 1000.0).round() as i64)
                    })
                    .collect();
                t.sort();
                t
            })
            .collect();
        tris.sort();
        tris
    };
    let first = shape(pts.clone());
    for k in 0..6 {
        let mut o = pts.clone();
        o.rotate_left(k);
        assert_eq!(shape(o.clone()), first);
        o.reverse();
        assert_eq!(shape(o), first);
    }
}

#[test]
fn a_landing_within_the_tolerance_lands_exactly() {
    // Fable (4): a corner lands 0.004 mm beside and above an edge.
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    poly(&mut m, &square(0.0, 2000.0, 1000.0, 0.0));
    let a = face_with(&m, &[[0.0, 0.0, 0.0], [1000.0, 1000.0, 0.0]]);
    let corner = vertex_at(&m, [0.0, 2000.0, 0.0]);
    apply(
        &mut m,
        mv_vertices(&[corner], DVec3::new(700.0, -999.996, 0.004)),
    )
    .unwrap();
    sound(&m);
    let landed = m.vertex(corner).unwrap();
    assert_eq!(
        landed,
        DVec3::new(700.0, 1000.0, 0.0),
        "on the edge exactly"
    );
    let fa = m.face(a).unwrap();
    assert_eq!(
        fa.outer.len(),
        5,
        "A is one face with the vertex in its side"
    );
    assert!(fa.outer.contains(&corner));
    assert_eq!(m.face_count(), 2);
    assert!(
        m.edges()
            .all(|(id, _)| m.edge_attrs(id).is_none_or(|a| !a.contains_key("soft"))),
        "nothing folded"
    );
    // A whole circle face moved onto a vertex 0.005 mm off its plane: it lands on the vertex
    // and stays one flat face.
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    let circle: Vec<[f64; 3]> = (0..24)
        .map(|i| {
            let t = std::f64::consts::TAU * f64::from(i) / 24.0;
            [3000.0 + 400.0 * t.cos(), 400.0 * t.sin(), 0.0]
        })
        .collect();
    m.apply(&Edit::AddCurve {
        points: circle,
        closed: true,
    })
    .unwrap();
    let disc = face_with(&m, &[[2600.0, 0.0, 0.0]]);
    let faces = m.face_count();
    // The point (2600, 0) goes to the corner (1000, 1000) of the square, 0.005 mm too high.
    apply(
        &mut m,
        mv_faces(&[disc], DVec3::new(-1600.0, 1000.0, 0.005)),
    )
    .unwrap();
    sound(&m);
    assert_eq!(m.face_count(), faces, "nothing folded");
    assert!(
        m.faces()
            .all(|(_, f)| (f.plane.normal.z.abs() - 1.0).abs() < 1e-12
                && f.plane.offset.abs() < 1e-9),
        "both faces stay at z = 0"
    );
    assert_eq!(m.vertex_count(), 4 + 24 - 1, "the vertices welded");
}

#[test]
fn a_tiny_uniform_scale_is_applied_and_a_squash_is_refused() {
    // A site model scaled to a sketch: 1e-5 (a determinant of 1e-15).
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 3_000_000.0, 0.0));
    let all: Vec<VertexId> = m.vertices().map(|(v, _)| v).collect();
    let scale = |k: DVec3| Edit::TransformVertices {
        vertices: all.clone(),
        faces: Vec::new(),
        edges: Vec::new(),
        matrix: mat(DAffine3::from_scale(k)),
    };
    let mut a = m.clone();
    apply(&mut a, scale(DVec3::splat(1e-5))).unwrap();
    sound(&a);
    assert!((a.area() - 900.0).abs() < 1e-6, "{}", a.area());
    let mut b = m.clone();
    apply(&mut b, scale(DVec3::splat(0.00009))).unwrap();
    sound(&b);
    // A squash below the relative tolerance is refused, whatever the determinant.
    for k in [
        DVec3::new(1.0, 1.0, 1e-10),
        DVec3::new(1e4, 1e4, 1e-7),
        DVec3::new(1.0, 1.0, 0.0),
    ] {
        let mut c = m.clone();
        let e = apply(&mut c, scale(k)).unwrap_err().to_string();
        assert!(e.contains("flattens"), "{e}");
        assert_eq!(c, m);
    }
    // A stretch and a turn far from flat are applied.
    let mut d = m.clone();
    apply(&mut d, scale(DVec3::new(2.0, 0.5, 4.0))).unwrap();
    sound(&d);
}

/// A square of 3000 with a triangle hole that touches the outer loop at the corner (0, 0).
fn pinched() -> SurfaceModel {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 3000.0, 0.0));
    poly(
        &mut m,
        &[[0.0, 0.0, 0.0], [1000.0, 500.0, 0.0], [500.0, 1000.0, 0.0]],
    );
    m
}

#[test]
fn a_face_with_a_hole_at_its_corner_folds() {
    let mut m = pinched();
    let big = m
        .faces()
        .max_by(|a, b| {
            m.face_area(a.0)
                .unwrap()
                .total_cmp(&m.face_area(b.0).unwrap())
        })
        .unwrap()
        .0;
    let f = m.face(big).unwrap();
    assert_eq!(f.holes.len(), 1, "the triangle is a hole");
    assert!(
        f.holes[0].iter().any(|v| f.outer.contains(v)),
        "the hole touches the outer loop"
    );
    let faces = m.face_count();
    let corner = vertex_at(&m, [3000.0, 3000.0, 0.0]);
    apply(&mut m, mv_vertices(&[corner], DVec3::new(0.0, 0.0, 500.0))).unwrap();
    sound(&m);
    assert!(m.face_count() > faces, "the face folded");
    // The pieces and the triangle's own face cover the square exactly, seen from above.
    let seen: f64 = m
        .faces()
        .map(|(id, f)| m.face_area(id).unwrap() * f.plane.normal.z)
        .sum();
    assert!((seen - 9_000_000.0).abs() < 1e-6, "{seen}");
}

#[test]
fn a_hole_moved_into_another_hole_is_refused() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 10_000.0, 0.0));
    poly(&mut m, &square(1000.0, 1000.0, 3000.0, 0.0));
    poly(&mut m, &square(6000.0, 6000.0, 1000.0, 0.0));
    let big = face_with(&m, &[[0.0, 0.0, 0.0], [10_000.0, 10_000.0, 0.0]]);
    // Take the small square's face away, so its loop is a hole of the big face only.
    let small = face_with(&m, &[[6000.0, 6000.0, 0.0], [7000.0, 7000.0, 0.0]]);
    let mid = face_with(&m, &[[1000.0, 1000.0, 0.0], [4000.0, 4000.0, 0.0]]);
    m.apply(&Edit::EraseFaces {
        faces: vec![small, mid],
    })
    .unwrap();
    assert_eq!(m.face(big).unwrap().holes.len(), 2);
    let before = m.clone();
    let ring: Vec<VertexId> = square(6000.0, 6000.0, 1000.0, 0.0)
        .iter()
        .map(|p| vertex_at(&m, *p))
        .collect();
    let e = apply(
        &mut m,
        mv_vertices(&ring, DVec3::new(-4000.0, -4000.0, 0.0)),
    );
    let msg = e.unwrap_err().to_string();
    assert!(msg.contains("hole"), "{msg}");
    assert_eq!(m, before);
    // The check names it in a model that has it.
    let mut bad = before.clone();
    for v in &ring {
        let p = bad.vertices[v];
        bad.vertices.insert(*v, p - DVec3::new(4000.0, 4000.0, 0.0));
    }
    assert!(
        bad.check()
            .problems
            .contains(&check::Problem::HoleInHole(big)),
        "{:?}",
        bad.check().problems
    );
}

#[test]
fn a_vertex_moved_onto_another_side_of_its_face_is_refused_with_the_reason() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    let before = m.clone();
    let c = vertex_at(&m, [1000.0, 1000.0, 0.0]);
    let e = apply(&mut m, mv_vertices(&[c], DVec3::new(-500.0, -1000.0, 0.0)));
    let msg = e.unwrap_err().to_string();
    assert!(msg.contains("own face"), "{msg}");
    assert_eq!(m, before);
}

#[test]
fn a_fold_moved_back_gives_the_model_before_with_its_ids_and_a_note_for_the_material() {
    // Fable (5): fold the top of a box, paint the new piece, move the corner back.
    let original = box_model();
    let mut m = original.clone();
    let top = top_face(&m);
    let corner = vertex_at(&m, [1000.0, 1000.0, 500.0]);
    apply(&mut m, mv_vertices(&[corner], DVec3::new(0.0, 0.0, 400.0))).unwrap();
    let pieces: Vec<FaceId> = m
        .faces()
        .filter(|(id, _)| original.face(*id).is_none())
        .map(|(id, _)| id)
        .collect();
    assert_eq!(pieces.len(), 1);
    assert!(pieces[0] > top);
    m.faces
        .get_mut(&pieces[0])
        .unwrap()
        .attrs
        .insert("material".into(), "stone".into());
    let notes = apply(&mut m, mv_vertices(&[corner], DVec3::new(0.0, 0.0, -400.0))).unwrap();
    sound(&m);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("stone"), "{notes:?}");
    assert!(
        m.face(top).unwrap().attrs.is_empty(),
        "the look of the top stays"
    );
    assert_same_geometry(&m, &original);
    // Without the paint: no note, the same model.
    let mut m = original.clone();
    apply(&mut m, mv_vertices(&[corner], DVec3::new(0.0, 0.0, 400.0))).unwrap();
    let notes = apply(&mut m, mv_vertices(&[corner], DVec3::new(0.0, 0.0, -400.0))).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    assert_same_geometry(&m, &original);
}

#[test]
fn a_copy_that_moves_less_than_the_tolerance_is_refused_and_no_step_changes_nothing() {
    use crate::command::{Command, Editor};
    use crate::model::{Attributes, Document, Shape};
    let m0 = box_model();
    let all: Vec<FaceId> = m0.faces().map(|(id, _)| id).collect();
    let mut m = m0.clone();
    let e = apply(
        &mut m,
        Edit::CopyGeometry {
            faces: all.clone(),
            edges: Vec::new(),
            matrix: shift(DVec3::new(1e-6, 0.0, 0.0)),
            count: 1,
        },
    );
    assert!(e.unwrap_err().to_string().contains("lie on the original"));
    assert_eq!(m, m0);
    // An edit that changes nothing: the model is the same (its revision and id counter too).
    let mut m = m0.clone();
    let none = Edit::TransformVertices {
        vertices: Vec::new(),
        faces: all.clone(),
        edges: Vec::new(),
        matrix: mat(DAffine3::IDENTITY),
    };
    apply(&mut m, none.clone()).unwrap();
    assert_eq!(m, m0);
    assert_eq!(m.rev(), m0.rev());
    // Through the editor: no undo step.
    let mut ed = Editor::new(Document::default());
    let id = crate::model::ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::new(m0.clone())),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let depth = ed.undo_depth();
    ed.execute(&Command::EditSurface { id, edit: none })
        .unwrap();
    assert_eq!(ed.undo_depth(), depth, "no undo step for no change");
    let r = ed.execute(&Command::EditSurface {
        id,
        edit: Edit::CopyGeometry {
            faces: all,
            edges: Vec::new(),
            matrix: shift(DVec3::new(1e-6, 0.0, 0.0)),
            count: 2,
        },
    });
    assert!(r.is_err());
    assert_eq!(ed.undo_depth(), depth);
}

/// Boxes and flat squares 1500 apart (they may touch) beside a large sheet, so that moves of up
/// to 2000 land on and over each other and into the sheet.
fn close_scene(seed: u64) -> SurfaceModel {
    let mut rng = Rng(seed | 1);
    let mut m = SurfaceModel::new();
    // A large sheet to the left: a square or a sheet moved by up to 2000 can land inside it.
    poly(
        &mut m,
        &[
            [-3500.0, -1500.0, 0.0],
            [-500.0, -1500.0, 0.0],
            [-500.0, 1500.0, 0.0],
            [-3500.0, 1500.0, 0.0],
        ],
    );
    for i in 0..rng.below(3) + 2 {
        let x = 1500.0 * i as f64;
        // On a grid of 250, so that moves of the same grid land on vertices and edges.
        let grid = |x: f64| (x / 250.0).round() * 250.0;
        let y = grid(rng.range(-500.0, 500.0));
        let w = grid(rng.range(500.0, 1500.0));
        let h = grid(rng.range(250.0, 1000.0));
        let known: std::collections::BTreeSet<FaceId> = m.faces().map(|(id, _)| id).collect();
        if m.apply(&Edit::AddEdges {
            edges: (0..4)
                .map(|k| {
                    let s = square(x, y, w, 0.0);
                    [s[k], s[(k + 1) % 4]]
                })
                .collect(),
        })
        .is_err()
        {
            continue;
        }
        let Some(f) = m.faces().map(|(id, _)| id).find(|id| !known.contains(id)) else {
            continue;
        };
        // Some squares stay flat sheets in one plane, so that moves in the plane overlap them.
        if rng.next() < 0.4 {
            continue;
        }
        let _ = m.apply(&Edit::PushPull {
            face: f,
            distance: h,
        });
    }
    m
}

/// A random move of vertices or of a face of `m`: a translation of up to `reach` (on a grid of
/// 250, so that landings happen), now and then off the grid by less than the tolerance, or a turn.
fn random_edit(rng: &mut Rng, m: &SurfaceModel, reach: f64) -> Edit {
    let vs: Vec<VertexId> = m.vertices().map(|(v, _)| v).collect();
    let fs: Vec<FaceId> = m.faces().map(|(f, _)| f).collect();
    let mut d = DVec3::new(
        (rng.range(-reach, reach) / 250.0).round() * 250.0,
        (rng.range(-reach, reach) / 250.0).round() * 250.0,
        (rng.range(-reach, reach) / 250.0).round() * 250.0,
    );
    if rng.next() < 0.4 {
        // In the plane of the sheets.
        d.z = 0.0;
    }
    if rng.next() < 0.3 {
        d += DVec3::new(
            rng.range(-0.004, 0.004),
            rng.range(-0.004, 0.004),
            rng.range(-0.004, 0.004),
        );
    }
    let (vertices, faces) = if rng.next() < 0.5 || fs.is_empty() {
        let k = rng.below(3) + 1;
        (
            (0..k).map(|_| vs[rng.below(vs.len())]).collect(),
            Vec::new(),
        )
    } else {
        (Vec::new(), vec![fs[rng.below(fs.len())]])
    };
    let xf = if rng.next() < 0.2 {
        let c = m.vertex(vs[rng.below(vs.len())]).unwrap();
        DAffine3::from_translation(c)
            * DAffine3::from_rotation_z(rng.range(-1.5, 1.5))
            * DAffine3::from_translation(-c)
    } else {
        DAffine3::from_translation(d)
    };
    Edit::TransformVertices {
        vertices,
        faces,
        edges: Vec::new(),
        matrix: mat(xf),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(160))]

    #[test]
    fn moves_that_land_on_other_boxes_never_make_an_error_of_the_model(seed in 0u64..u64::MAX) {
        use crate::command::{Command, Editor};
        use crate::model::{Attributes, Document, Shape};
        let mut rng = Rng(seed | 1);
        let mut m = close_scene(seed);
        for _ in 0..3 {
            if m.vertex_count() == 0 {
                break;
            }
            let edit = random_edit(&mut rng, &m, 2000.0);
            let before = m.clone();
            let mut twin = m.clone();
            match m.apply_with_notes(&edit) {
                Ok(_) => {
                    let r = m.check();
                    prop_assert!(r.is_valid(), "{:?} after {:?}", r.problems, edit);
                    prop_assert!(m.problem().is_none(), "{:?}", m.problem());
                    twin.apply(&edit).unwrap();
                    prop_assert_eq!(&m, &twin, "the same edit gives the same model");
                }
                Err(_) => prop_assert_eq!(&m, &before),
            }
            // Through the editor: one step, and undo gives the model back byte for byte.
            let mut ed = Editor::new(Document::default());
            let id = crate::model::ObjectId(
                ed.execute(&Command::AddObject {
                    shape: Shape::SurfaceModel(Box::new(before.clone())),
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                })
                .unwrap()
                .created[0],
            );
            let text = |ed: &Editor| serde_json::to_string(&ed.doc().object(id).unwrap().shape).unwrap();
            let start = text(&ed);
            if ed.execute(&Command::EditSurface { id, edit }).is_ok() && text(&ed) != start {
                ed.undo();
            }
            prop_assert_eq!(text(&ed), start);
        }
    }
}

#[test]
fn most_close_moves_work_and_the_others_change_nothing() {
    let (mut ok, mut refused) = (0, 0);
    for seed in 0..300u64 {
        let mut rng = Rng(seed * 104_729 + 11);
        let mut m = close_scene(seed);
        if m.vertex_count() == 0 {
            continue;
        }
        let edit = random_edit(&mut rng, &m, 2000.0);
        let before = m.clone();
        match m.apply(&edit) {
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
    assert!(ok > 100, "{ok} worked, {refused} were refused");
}

#[test]
fn the_local_work_gives_the_same_results_as_the_whole_model() {
    // The edit looks only at the part of the model around what moved (a topology index of that
    // part, checks of what changed); the reference way indexes and checks the whole model. Both
    // give the same model, or the same refusal.
    use super::edit::reference::{self, Mode};
    let (mut ok, mut landed) = (0, 0);
    for seed in 0..500u64 {
        let mut rng = Rng(seed * 7 + 5);
        let mut fast = close_scene(seed);
        let mut slow = fast.clone();
        for _ in 0..3 {
            if fast.vertex_count() == 0 {
                break;
            }
            let edit = random_edit(&mut rng, &fast, 2000.0);
            let before = fast.clone();
            let a = reference::with(Mode::Index, || fast.apply_with_notes(&edit));
            let b = reference::with(Mode::Scan, || slow.apply_with_notes(&edit));
            assert_eq!(a, b, "seed {seed}: {edit:?}");
            assert_eq!(fast, slow, "seed {seed}: {edit:?}");
            if a.is_ok() {
                ok += 1;
                landed += usize::from(fast.vertex_count() < before.vertex_count());
                let r = fast.check();
                assert!(r.is_valid(), "seed {seed}: {:?} after {edit:?}", r.problems);
            } else {
                assert_eq!(fast, before);
            }
        }
    }
    // (Welds only: a move no longer merges faces, so no vertex heals away.)
    assert!(ok > 800 && landed >= 5, "{ok} worked, {landed} welded");
}

#[test]
fn an_edge_that_a_merge_joins_is_checked_for_crossings() {
    // Found by the property test: the move welds, the faces merge, and the heal of the merge joins
    // two edges into one that no moved vertex ends; that edge crosses another edge.
    let seed = 2803922998918617984u64;
    let mut rng = Rng(seed | 1);
    let mut m = close_scene(seed);
    let edit = random_edit(&mut rng, &m, 2000.0);
    let before = m.clone();
    match m.apply(&edit) {
        Ok(()) => sound(&m),
        Err(_) => assert_eq!(m, before),
    }
}

// ---- no merge after a move; only folds unfold ---------------------------------------------------

/// A floor of `n` x `n` separate square cells of 1000 at z = 0, drawn as lines.
fn grid(n: usize) -> SurfaceModel {
    let mut m = SurfaceModel::new();
    let len = n as f64 * 1000.0;
    let mut edges = Vec::new();
    for i in 0..=n {
        let t = i as f64 * 1000.0;
        edges.push([[t, 0.0, 0.0], [t, len, 0.0]]);
        edges.push([[0.0, t, 0.0], [len, t, 0.0]]);
    }
    m.apply(&Edit::AddEdges { edges }).unwrap();
    assert_eq!(m.face_count(), n * n);
    m
}

/// The number of edges with the attribute `name`.
fn has_attr(m: &SurfaceModel, name: &str) -> usize {
    m.edges()
        .filter(|(id, _)| m.edge_attrs(*id).is_some_and(|a| a.contains_key(name)))
        .count()
}

/// A floor of 2000 x 1000 in two faces, and the two ends of the edge between them.
fn two_faces() -> (SurfaceModel, VertexId, VertexId) {
    let mut m = SurfaceModel::new();
    poly(
        &mut m,
        &[
            [0.0, 0.0, 0.0],
            [2000.0, 0.0, 0.0],
            [2000.0, 1000.0, 0.0],
            [0.0, 1000.0, 0.0],
        ],
    );
    m.apply(&Edit::AddEdges {
        edges: vec![[[1000.0, 0.0, 0.0], [1000.0, 1000.0, 0.0]]],
    })
    .unwrap();
    assert_eq!(m.face_count(), 2);
    let lo = vertex_at(&m, [1000.0, 0.0, 0.0]);
    let hi = vertex_at(&m, [1000.0, 1000.0, 0.0]);
    (m, lo, hi)
}

#[test]
fn a_grid_vertex_lifted_and_moved_back_gives_the_grid_with_its_lines() {
    let original = grid(3);
    let mut m = original.clone();
    let v = vertex_at(&m, [1000.0, 1000.0, 0.0]);
    apply(&mut m, mv_vertices(&[v], DVec3::new(0.0, 0.0, 400.0))).unwrap();
    sound(&m);
    assert_eq!(m.face_count(), 9 + 4, "the four cells at the vertex fold");
    assert_eq!(has_attr(&m, "fold"), 4, "each fold edge is marked");
    let notes = apply(&mut m, mv_vertices(&[v], DVec3::new(0.0, 0.0, -400.0))).unwrap();
    sound(&m);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(m.face_count(), 9, "the cells stay apart");
    assert_eq!(has_attr(&m, "fold") + has_attr(&m, "soft"), 0);
    assert_same_geometry(&m, &original);
}

#[test]
fn an_unfold_takes_the_look_of_the_larger_piece_with_a_note() {
    let original = grid(3);
    let mut m = original.clone();
    let cell = |m: &SurfaceModel, x: f64, y: f64| {
        face_with(m, &[[x, y, 0.0], [x + 1000.0, y + 1000.0, 0.0]])
    };
    let a = cell(&m, 0.0, 0.0);
    let b = cell(&m, 1000.0, 0.0);
    m.faces
        .get_mut(&a)
        .unwrap()
        .attrs
        .insert("material".into(), "grass".into());
    let mut expected = m.clone();
    let v = vertex_at(&m, [1000.0, 1000.0, 0.0]);
    apply(&mut m, mv_vertices(&[v], DVec3::new(0.0, 0.0, 400.0))).unwrap();
    // The pieces of a cell: the faces with all vertices in its square, the larger first.
    let pieces = |m: &SurfaceModel, x: f64, y: f64| {
        let mut p: Vec<(FaceId, f64)> = m
            .faces()
            .filter(|(_, f)| {
                f.outer.iter().all(|v| {
                    let p = m.vertex(*v).unwrap();
                    (x..=x + 1000.0).contains(&p.x) && (y..=y + 1000.0).contains(&p.y)
                })
            })
            .map(|(id, _)| (id, m.face_area(id).unwrap()))
            .collect();
        p.sort_by(|s, t| t.1.total_cmp(&s.1));
        p
    };
    let pa = pieces(&m, 0.0, 0.0);
    let pb = pieces(&m, 1000.0, 0.0);
    assert_eq!((pa.len(), pb.len()), (2, 2));
    assert_eq!((pa[0].0, pb[0].0), (a, b), "the larger piece keeps the id");
    // Paint the smaller piece of A and the larger piece of B while they are folded.
    m.faces
        .get_mut(&pa[1].0)
        .unwrap()
        .attrs
        .insert("material".into(), "stone".into());
    m.faces
        .get_mut(&b)
        .unwrap()
        .attrs
        .insert("material".into(), "brick".into());
    let notes = apply(&mut m, mv_vertices(&[v], DVec3::new(0.0, 0.0, -400.0))).unwrap();
    sound(&m);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("stone"), "{notes:?}");
    assert_eq!(m.face_count(), 9);
    // A keeps grass (its larger piece decides), B takes brick (its larger piece was painted).
    expected
        .faces
        .get_mut(&b)
        .unwrap()
        .attrs
        .insert("material".into(), "brick".into());
    assert_same_geometry(&m, &expected);
}

#[test]
fn two_floor_faces_that_stay_coplanar_stay_two_faces() {
    let (mut m, lo, hi) = two_faces();
    let shared = m.edge_between(lo, hi).unwrap();
    let corner = vertex_at(&m, [0.0, 0.0, 0.0]);
    for (v, d) in [
        (lo, DVec3::new(200.0, 0.0, 0.0)),
        (corner, DVec3::new(-100.0, -300.0, 0.0)),
        (hi, DVec3::new(-150.0, 0.0, 0.0)),
    ] {
        let notes = apply(&mut m, mv_vertices(&[v], d)).unwrap();
        sound(&m);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(m.face_count(), 2);
        assert_eq!(m.vertex_count(), 6);
        assert_eq!(
            m.edge_between(lo, hi),
            Some(shared),
            "the edge between them stays"
        );
    }
}

#[test]
fn a_bump_brought_level_with_its_neighbours_keeps_every_cell() {
    let original = grid(3);
    let mut m = original.clone();
    let up = DVec3::new(0.0, 0.0, 400.0);
    for c in square(1000.0, 1000.0, 1000.0, 0.0) {
        let v = vertex_at(&m, c);
        apply(&mut m, mv_vertices(&[v], up)).unwrap();
        sound(&m);
    }
    // The centre cell is flat again (it unfolded), the side cells are flat slopes (unfolded too),
    // the corner cells are folded.
    assert_eq!(m.face_count(), 1 + 4 + 4 * 2);
    assert_eq!(has_attr(&m, "fold"), 4);
    let rest: Vec<VertexId> = m
        .vertices()
        .filter(|(_, p)| p.z == 0.0)
        .map(|(v, _)| v)
        .collect();
    assert_eq!(rest.len(), 12);
    let notes = apply(&mut m, mv_vertices(&rest, up)).unwrap();
    sound(&m);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(m.face_count(), 9, "all cells are coplanar and stay apart");
    assert_eq!(has_attr(&m, "fold") + has_attr(&m, "soft"), 0);
    let mut expected = original.clone();
    let all: Vec<VertexId> = expected.vertices().map(|(v, _)| v).collect();
    apply(&mut expected, mv_vertices(&all, up)).unwrap();
    assert_same_geometry(&m, &expected);
}

#[test]
fn a_user_soft_edge_between_coplanar_faces_stays() {
    let (mut m, lo, hi) = two_faces();
    let shared = m.edge_between(lo, hi).unwrap();
    m.edge_attrs
        .entry(shared)
        .or_default()
        .insert("soft".into(), true.into());
    let original = m.clone();
    let corner = vertex_at(&m, [2000.0, 1000.0, 0.0]);
    apply(&mut m, mv_vertices(&[corner], DVec3::new(0.0, 0.0, 400.0))).unwrap();
    assert_eq!(m.face_count(), 3);
    apply(&mut m, mv_vertices(&[corner], DVec3::new(0.0, 0.0, -400.0))).unwrap();
    sound(&m);
    assert_eq!(m.face_count(), 2);
    assert_same_geometry(&m, &original);
    // Moved in the plane: the edge and its attribute stay.
    apply(&mut m, mv_vertices(&[lo], DVec3::new(100.0, 0.0, 0.0))).unwrap();
    sound(&m);
    assert_eq!(m.edge_between(lo, hi), Some(shared));
    assert_eq!(
        m.edge_attrs(shared).unwrap().get("soft"),
        Some(&true.into())
    );
}

#[test]
fn a_landing_welds_the_vertices_and_the_faces_stay_faces() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    poly(&mut m, &square(1500.0, 0.0, 1000.0, 0.0));
    let near = [
        vertex_at(&m, [1500.0, 0.0, 0.0]),
        vertex_at(&m, [1500.0, 1000.0, 0.0]),
    ];
    let kept = [
        vertex_at(&m, [1000.0, 0.0, 0.0]),
        vertex_at(&m, [1000.0, 1000.0, 0.0]),
    ];
    let notes = apply(&mut m, mv_vertices(&near, DVec3::new(-500.0, 0.0, 0.0))).unwrap();
    sound(&m);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(m.face_count(), 2);
    assert_eq!(
        m.vertex_count(),
        6,
        "each landed vertex is one with the one there"
    );
    assert_eq!(m.edge_count(), 7, "the two sides are one edge");
    assert!(near.iter().all(|v| m.vertex(*v).is_none()));
    let shared = m.edge_between(kept[0], kept[1]).unwrap();
    let users = m
        .faces()
        .filter(|(_, f)| f.uses().any(|(a, b)| m.edge_between(a, b) == Some(shared)))
        .count();
    assert_eq!(users, 2, "both faces use the shared edge");
}

#[test]
fn a_circle_and_a_face_with_holes_unfold_into_the_face_they_were() {
    // A circle of 48 sides (far pieces have no vertex near the lifted one), and a square with a
    // hole and a hole that touches its corner.
    let mut circle = SurfaceModel::new();
    circle
        .apply(&Edit::AddCurve {
            points: (0..48)
                .map(|i| {
                    let t = std::f64::consts::TAU * f64::from(i) / 48.0;
                    [2000.0 * t.cos(), 2000.0 * t.sin(), 0.0]
                })
                .collect(),
            closed: true,
        })
        .unwrap();
    let mut holes = pinched();
    poly(&mut holes, &square(1500.0, 1500.0, 500.0, 0.0));
    for (original, lift) in [(circle, [2000.0, 0.0, 0.0]), (holes, [3000.0, 3000.0, 0.0])] {
        let mut m = original.clone();
        let v = vertex_at(&m, lift);
        apply(&mut m, mv_vertices(&[v], DVec3::new(0.0, 0.0, 300.0))).unwrap();
        sound(&m);
        assert!(m.face_count() > original.face_count() + 2, "it folded");
        let notes = apply(&mut m, mv_vertices(&[v], DVec3::new(0.0, 0.0, -300.0))).unwrap();
        sound(&m);
        assert!(notes.is_empty(), "{notes:?}");
        assert_same_geometry(&m, &original);
    }
}

#[test]
fn a_fold_with_a_line_drawn_on_a_piece_does_not_unfold() {
    let mut m = box_model();
    let corner = vertex_at(&m, [1000.0, 1000.0, 500.0]);
    apply(&mut m, mv_vertices(&[corner], DVec3::new(0.0, 0.0, 400.0))).unwrap();
    assert_eq!(has_attr(&m, "fold"), 1);
    // A line across the flat piece (at the corner (0, 0) of the top) splits it.
    m.apply(&Edit::AddEdges {
        edges: vec![[[500.0, 0.0, 500.0], [0.0, 500.0, 500.0]]],
    })
    .unwrap();
    sound(&m);
    let faces = m.face_count();
    apply(&mut m, mv_vertices(&[corner], DVec3::new(0.0, 0.0, -400.0))).unwrap();
    sound(&m);
    assert_eq!(m.face_count(), faces, "nothing unfolds and nothing merges");
    assert_eq!(has_attr(&m, "fold"), 1, "the fold edge stays");
    assert_eq!(has_attr(&m, "soft"), 1);
}
