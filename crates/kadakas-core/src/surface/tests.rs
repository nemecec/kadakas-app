use std::collections::BTreeMap;

use glam::{DAffine3, DVec2, DVec3};
use proptest::prelude::*;

use super::arrange::{Arrangement, canonical, inside, reversed};
use super::geom::{self, Plane};
use super::*;

type P = [f64; 3];

fn add(m: &mut SurfaceModel, segs: &[(P, P)]) -> Result<(), SurfaceError> {
    m.apply(&Edit::AddEdges {
        edges: segs.iter().map(|(a, b)| [*a, *b]).collect(),
    })
}

/// The closed polygon through the points.
fn poly(m: &mut SurfaceModel, pts: &[P]) {
    let segs: Vec<(P, P)> = (0..pts.len())
        .map(|i| (pts[i], pts[(i + 1) % pts.len()]))
        .collect();
    add(m, &segs).unwrap();
}

fn square(x0: f64, y0: f64, w: f64, z: f64) -> Vec<P> {
    vec![
        [x0, y0, z],
        [x0 + w, y0, z],
        [x0 + w, y0 + w, z],
        [x0, y0 + w, z],
    ]
}

fn valid(m: &SurfaceModel) {
    let r = m.check();
    assert!(r.is_valid(), "{:?}", r.problems);
    assert!(m.problem().is_none(), "{:?}", m.problem());
}

fn areas(m: &SurfaceModel) -> Vec<f64> {
    let mut a: Vec<f64> = m.faces().map(|(id, _)| m.face_area(id).unwrap()).collect();
    a.sort_by(f64::total_cmp);
    a
}

fn erase(m: &mut SurfaceModel, ids: &[EdgeId]) -> Result<(), SurfaceError> {
    m.apply(&Edit::EraseEdges {
        edges: ids.to_vec(),
    })
}

fn edge_at(m: &SurfaceModel, a: P, b: P) -> EdgeId {
    let find = |p: P| {
        m.vertices()
            .find(|(_, q)| q.distance(DVec3::from(p)) < 1e-6)
            .map(|(id, _)| id)
            .unwrap()
    };
    m.edge_between(find(a), find(b)).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0)
}

// ---------------------------------------------------------------------------
// Geometry pieces

#[test]
fn tolerances_follow_the_size_of_the_coordinates() {
    assert_eq!(tolerance_at(0.0), LENGTH_TOLERANCE);
    assert_eq!(tolerance_at(1e6), LENGTH_TOLERANCE);
    assert!(close(tolerance_at(1e8), 0.1));
    assert!(close(tolerance_at(1e9), 1.0));
    assert_eq!(
        plane_tolerance_at(0.0),
        LENGTH_TOLERANCE * PLANE_TOLERANCE_FACTOR
    );
    assert_eq!(
        geom::tol(&[DVec3::ZERO, DVec3::splat(-1e8)]),
        tolerance_at(1e8)
    );
    assert_eq!(geom::plane_tol(&[DVec3::ZERO]), plane_tolerance_at(0.0));
}

#[test]
fn planes_project_counterclockwise_from_the_front() {
    for n in [
        DVec3::Z,
        DVec3::NEG_Z,
        DVec3::X,
        DVec3::Y,
        DVec3::new(1.0, 2.0, 3.0),
        DVec3::new(-3.0, 0.5, 0.0),
    ] {
        let p = Plane::new(DVec3::new(5.0, 6.0, 7.0), n).unwrap();
        let (u, v) = p.basis();
        assert!(close(u.length(), 1.0) && close(v.length(), 1.0));
        assert!(u.cross(v).distance(p.normal) < 1e-12);
        assert!(close(p.distance(DVec3::new(5.0, 6.0, 7.0)), 0.0));
        assert!(on_plane(&p, DVec3::new(5.0, 6.0, 7.0) + u * 100.0));
        assert!(!on_plane(&p, DVec3::new(5.0, 6.0, 7.0) + p.normal * 0.01));
        assert!(on_plane(&p, DVec3::new(5.0, 6.0, 7.0) + p.normal * 0.002));
        let f = p.flipped();
        assert_eq!(f.normal, -p.normal);
        assert!(close(f.distance(DVec3::ZERO), -p.distance(DVec3::ZERO)));
        assert_eq!(project(&p, u), DVec2::new(1.0, 0.0));
    }
    assert!(Plane::new(DVec3::ZERO, DVec3::ZERO).is_none());
}

#[test]
fn distances_between_points_segments_and_lines() {
    let (a, b) = (DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0));
    assert_eq!(
        geom::point_segment(DVec3::new(5.0, 3.0, 0.0), a, b),
        (3.0, 0.5)
    );
    assert_eq!(
        geom::point_segment(DVec3::new(-4.0, 3.0, 0.0), a, b),
        (5.0, 0.0)
    );
    assert_eq!(geom::point_segment(DVec3::ONE, a, a), (3f64.sqrt(), 0.0));
    let (s, t, d) =
        geom::segment_segment(a, b, DVec3::new(4.0, -5.0, 2.0), DVec3::new(4.0, 5.0, 2.0)).unwrap();
    assert!(close(s, 0.4) && close(t, 0.5) && close(d, 2.0));
    // Clamped at the end of the second segment, on both sides.
    let (_, t, d) =
        geom::segment_segment(a, b, DVec3::new(4.0, 1.0, 0.0), DVec3::new(4.0, 5.0, 0.0)).unwrap();
    assert!(close(t, 0.0) && close(d, 1.0));
    let (_, t, _) =
        geom::segment_segment(a, b, DVec3::new(4.0, -5.0, 0.0), DVec3::new(4.0, -1.0, 0.0))
            .unwrap();
    assert!(close(t, 1.0));
    assert!(geom::segment_segment(a, b, DVec3::Y, DVec3::new(10.0, 1.0, 0.0)).is_none());
    assert!(geom::segment_segment(a, a, DVec3::Y, DVec3::X).is_none());
    assert!(close(
        geom::point_line(DVec3::new(50.0, 2.0, 0.0), a, b),
        2.0
    ));
    assert!(close(
        geom::point_line(DVec3::new(3.0, 4.0, 0.0), a, a),
        5.0
    ));
    assert_eq!(geom::newell(&[]), DVec3::ZERO);
    assert_eq!(geom::area2(&[DVec2::ZERO, DVec2::X]), 0.0);
    assert_eq!(geom::bounds(std::iter::empty()), None);
}

#[test]
fn loops_turn_and_test_points() {
    let l = [VertexId(5), VertexId(2), VertexId(9)];
    assert_eq!(canonical(&l), vec![VertexId(2), VertexId(9), VertexId(5)]);
    assert_eq!(reversed(&l), vec![VertexId(2), VertexId(5), VertexId(9)]);
    assert!(canonical(&[]).is_empty());
    let sq = vec![
        DVec2::ZERO,
        DVec2::new(4.0, 0.0),
        DVec2::new(4.0, 4.0),
        DVec2::new(0.0, 4.0),
    ];
    let hole = vec![
        DVec2::new(1.0, 1.0),
        DVec2::new(1.0, 3.0),
        DVec2::new(3.0, 3.0),
        DVec2::new(3.0, 1.0),
    ];
    assert!(inside(DVec2::new(0.5, 0.5), &[sq.clone(), hole.clone()]));
    assert!(!inside(DVec2::new(2.0, 2.0), &[sq.clone(), hole]));
    assert!(!inside(DVec2::new(5.0, 2.0), &[sq]));
}

// ---------------------------------------------------------------------------
// Faces from edges

#[test]
fn closed_coplanar_edges_make_a_face() {
    let mut m = SurfaceModel::new();
    let s = square(0.0, 0.0, 1000.0, 0.0);
    add(&mut m, &[(s[0], s[1]), (s[1], s[2]), (s[2], s[3])]).unwrap();
    assert_eq!(m.face_count(), 0);
    assert_eq!((m.vertex_count(), m.edge_count()), (4, 3));
    add(&mut m, &[(s[3], s[0])]).unwrap();
    assert_eq!(m.face_count(), 1);
    let (id, f) = m.faces().next().unwrap();
    assert_eq!(f.plane.normal, DVec3::Z, "a face on the ground faces up");
    assert!(f.holes.is_empty() && f.outer.len() == 4);
    assert!(close(m.face_area(id).unwrap(), 1e6));
    assert!(close(m.area(), 1e6));
    assert_eq!(m.face_normal(id), Some(DVec3::Z));
    assert_eq!(
        m.face_bounds(id),
        Some((DVec3::ZERO, DVec3::new(1000.0, 1000.0, 0.0)))
    );
    valid(&m);
    let r = m.check();
    assert_eq!(r.border_edges.len(), 4);
    assert!(!r.is_closed());
    // Drawn the other way round, the face still faces up.
    let mut m2 = SurfaceModel::new();
    let mut back = s.clone();
    back.reverse();
    poly(&mut m2, &back);
    assert_eq!(m2.faces().next().unwrap().1.plane.normal, DVec3::Z);
}

#[test]
fn a_loop_that_is_not_flat_makes_no_face() {
    let mut m = SurfaceModel::new();
    let mut s = square(0.0, 0.0, 1000.0, 0.0);
    s[2][2] = 100.0;
    poly(&mut m, &s);
    assert_eq!((m.face_count(), m.edge_count()), (0, 4));
    valid(&m);
}

#[test]
fn vertical_faces_face_y_then_x() {
    let mut m = SurfaceModel::new();
    poly(
        &mut m,
        &[
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            [1000.0, 0.0, 1000.0],
            [0.0, 0.0, 1000.0],
        ],
    );
    assert_eq!(m.faces().next().unwrap().1.plane.normal, DVec3::Y);
    let mut m = SurfaceModel::new();
    poly(
        &mut m,
        &[
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1000.0],
            [0.0, 1000.0, 1000.0],
            [0.0, 1000.0, 0.0],
        ],
    );
    assert_eq!(m.faces().next().unwrap().1.plane.normal, DVec3::X);
}

#[test]
fn a_new_edge_across_a_face_splits_it() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    let (old, _) = m.faces().next().unwrap();
    add(&mut m, &[([300.0, 0.0, 0.0], [300.0, 1000.0, 0.0])]).unwrap();
    assert_eq!(m.face_count(), 2);
    assert_eq!((m.vertex_count(), m.edge_count()), (6, 7));
    // The larger part keeps the id.
    assert!(close(m.face_area(old).unwrap(), 0.7e6));
    assert_eq!(areas(&m).len(), 2);
    assert!(close(areas(&m)[0], 0.3e6));
    for (_, f) in m.faces() {
        assert_eq!(f.plane.normal, DVec3::Z);
    }
    valid(&m);
    // The splitting edge is used by both faces in opposite directions.
    let e = edge_at(&m, [300.0, 0.0, 0.0], [300.0, 1000.0, 0.0]);
    assert_eq!(m.faces_of_edge(e).len(), 2);
    assert!(m.check().nonmanifold_edges.is_empty());
}

#[test]
fn an_edge_inside_a_face_that_ends_on_its_border_splits_it() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    // From the border into the face: a stray edge on the face, no split.
    add(&mut m, &[([0.0, 500.0, 0.0], [500.0, 500.0, 0.0])]).unwrap();
    assert_eq!(m.face_count(), 1);
    assert!(close(m.area(), 1e6));
    valid(&m);
    // On to the border again: the face splits into an L and a square.
    add(&mut m, &[([500.0, 500.0, 0.0], [500.0, 1000.0, 0.0])]).unwrap();
    assert_eq!(m.face_count(), 2);
    let a = areas(&m);
    assert!(close(a[0], 0.25e6) && close(a[1], 0.75e6));
    valid(&m);
    // A line from outside into the face splits the border edge only.
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    add(&mut m, &[([-500.0, 200.0, 0.0], [400.0, 200.0, 0.0])]).unwrap();
    assert_eq!(m.face_count(), 1);
    assert_eq!(m.faces().next().unwrap().1.outer.len(), 5);
    assert!(close(m.area(), 1e6));
    valid(&m);
}

#[test]
fn a_closed_loop_inside_a_face_makes_a_hole_and_an_inner_face() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    let (outer_id, _) = m.faces().next().unwrap();
    poly(&mut m, &square(300.0, 300.0, 400.0, 0.0));
    assert_eq!(m.face_count(), 2);
    let outer = m.face(outer_id).unwrap();
    assert_eq!(
        outer.holes.len(),
        1,
        "the outer face keeps its id and gets a hole"
    );
    assert!(close(m.face_area(outer_id).unwrap(), 1e6 - 0.16e6));
    let (inner_id, inner) = m.faces().find(|(id, _)| *id != outer_id).unwrap();
    assert!(inner.holes.is_empty());
    assert!(close(m.face_area(inner_id).unwrap(), 0.16e6));
    assert_eq!(inner.plane.normal, DVec3::Z);
    valid(&m);
    // The triangles of the outer face cover its area and leave the hole open.
    let t = m.triangulate_face(outer_id).unwrap();
    let tri_area: f64 = t
        .triangles
        .iter()
        .map(|k| {
            let [a, b, c] = k.map(|i| t.positions[i as usize]);
            (b - a).cross(c - a).dot(t.normal) / 2.0
        })
        .sum();
    assert!(close(tri_area, 0.84e6));
    assert!(t.triangles.iter().all(|k| {
        let [a, b, c] = k.map(|i| t.positions[i as usize]);
        (b - a).cross(c - a).dot(t.normal) >= 0.0
    }));
    let mesh = m.mesh().unwrap();
    assert_eq!(mesh.positions.len(), 8);
    assert_eq!(mesh.triangles.len(), mesh.faces.len());
    assert!(mesh.faces.contains(&inner_id) && mesh.faces.contains(&outer_id));
    assert!(m.triangulate_face(FaceId(999)).is_err());
}

#[test]
fn a_loop_inside_an_empty_region_fills_only_the_loop() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    let (id, _) = m.faces().next().unwrap();
    m.apply(&Edit::EraseFaces { faces: vec![id] }).unwrap();
    poly(&mut m, &square(300.0, 300.0, 400.0, 0.0));
    assert_eq!(m.face_count(), 1);
    assert!(close(m.area(), 0.16e6));
    valid(&m);
}

#[test]
fn a_loop_drawn_around_a_face_gets_it_as_a_hole() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(300.0, 300.0, 400.0, 0.0));
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    assert_eq!(m.face_count(), 2);
    assert!(close(m.area(), 1e6));
    assert_eq!(m.faces().filter(|(_, f)| f.holes.len() == 1).count(), 1);
    valid(&m);
}

#[test]
fn erasing_the_edge_between_coplanar_faces_merges_them() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    let before = m.clone();
    let (old, _) = m.faces().next().unwrap();
    add(&mut m, &[([300.0, 0.0, 0.0], [300.0, 1000.0, 0.0])]).unwrap();
    let e = edge_at(&m, [300.0, 0.0, 0.0], [300.0, 1000.0, 0.0]);
    erase(&mut m, &[e]).unwrap();
    assert_eq!(m.face_count(), 1);
    assert!(close(m.face_area(old).unwrap(), 1e6));
    // The split vertices are healed: the square is back with its own ids.
    assert_eq!((m.vertex_count(), m.edge_count()), (4, 4));
    assert_eq!(m.face(old), before.face(old));
    for (id, e) in before.edges() {
        assert_eq!(m.edge(id), Some(e));
    }
    valid(&m);
}

#[test]
fn erasing_an_edge_of_one_face_removes_the_face() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    let e = edge_at(&m, [0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]);
    erase(&mut m, &[e]).unwrap();
    assert_eq!(
        (m.face_count(), m.edge_count(), m.vertex_count()),
        (0, 3, 4)
    );
    valid(&m);
    // Erase the rest: the vertices go with their last edge.
    let ids: Vec<EdgeId> = m.edges().map(|(id, _)| id).collect();
    erase(&mut m, &ids).unwrap();
    assert!(m.is_empty() && m.edge_count() == 0);
    assert_eq!(m.local_bounds(), None);
    assert_eq!(m.bounds(), None);
}

#[test]
fn erasing_an_edge_of_a_hole_merges_the_inner_face_or_removes_the_face() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    poly(&mut m, &square(300.0, 300.0, 400.0, 0.0));
    let e = edge_at(&m, [300.0, 300.0, 0.0], [700.0, 300.0, 0.0]);
    let mut merged = m.clone();
    erase(&mut merged, &[e]).unwrap();
    assert_eq!(merged.face_count(), 1);
    let (_, f) = merged.faces().next().unwrap();
    assert!(f.holes.is_empty() && close(merged.area(), 1e6));
    valid(&merged);
    // Without the inner face the edge bounds one face only, and the face goes.
    let inner = m.faces().find(|(_, f)| f.holes.is_empty()).unwrap().0;
    m.apply(&Edit::EraseFaces { faces: vec![inner] }).unwrap();
    assert_eq!(m.face_count(), 1);
    assert!(close(m.area(), 0.84e6), "the inner face left a hole");
    erase(&mut m, &[e]).unwrap();
    assert_eq!(m.face_count(), 0);
    valid(&m);
}

#[test]
fn erasing_faces_keeps_edges_and_redrawing_an_edge_brings_them_back() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    let (id, _) = m.faces().next().unwrap();
    m.apply(&Edit::EraseFaces { faces: vec![id] }).unwrap();
    assert_eq!((m.face_count(), m.edge_count()), (0, 4));
    add(&mut m, &[([0.0, 0.0, 0.0], [1000.0, 0.0, 0.0])]).unwrap();
    assert_eq!((m.face_count(), m.edge_count()), (1, 4));
    valid(&m);
    assert_eq!(
        m.apply(&Edit::EraseFaces {
            faces: vec![FaceId(999)]
        }),
        Err(SurfaceError::NoFace(FaceId(999)))
    );
}

#[test]
fn reversing_a_face_swaps_front_and_back() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    poly(&mut m, &square(300.0, 300.0, 400.0, 0.0));
    let ids: Vec<FaceId> = m.faces().map(|(id, _)| id).collect();
    let before = m.clone();
    m.apply(&Edit::ReverseFaces { faces: ids.clone() }).unwrap();
    for id in &ids {
        assert_eq!(m.face_normal(*id), Some(DVec3::NEG_Z));
        assert!(close(
            m.face_area(*id).unwrap(),
            before.face_area(*id).unwrap()
        ));
    }
    valid(&m);
    m.apply(&Edit::ReverseFaces { faces: ids.clone() }).unwrap();
    assert_eq!(m, before);
    assert_eq!(
        m.apply(&Edit::ReverseFaces {
            faces: vec![FaceId(0)]
        }),
        Err(SurfaceError::NoFace(FaceId(0)))
    );
    // A face drawn next to a face that looks down looks down too.
    m.apply(&Edit::ReverseFaces { faces: ids }).unwrap();
    poly(
        &mut m,
        &[
            [1000.0, 0.0, 0.0],
            [2000.0, 0.0, 0.0],
            [2000.0, 1000.0, 0.0],
            [1000.0, 1000.0, 0.0],
        ],
    );
    assert!(m.faces().all(|(_, f)| f.plane.normal == DVec3::NEG_Z));
    valid(&m);
}

/// The twelve edges of a box of size 1000.
fn cube_edges(z0: f64) -> Vec<(P, P)> {
    let c = |x: f64, y: f64, z: f64| [x * 1000.0, y * 1000.0, z0 + z * 1000.0];
    let mut out = Vec::new();
    for z in [0.0, 1.0] {
        out.push((c(0.0, 0.0, z), c(1.0, 0.0, z)));
        out.push((c(1.0, 0.0, z), c(1.0, 1.0, z)));
        out.push((c(1.0, 1.0, z), c(0.0, 1.0, z)));
        out.push((c(0.0, 1.0, z), c(0.0, 0.0, z)));
    }
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        out.push((c(x, y, 0.0), c(x, y, 1.0)));
    }
    out
}

#[test]
fn a_wire_box_gets_six_faces_that_close_a_solid() {
    let mut m = SurfaceModel::new();
    add(&mut m, &cube_edges(0.0)).unwrap();
    assert_eq!(
        (m.vertex_count(), m.edge_count(), m.face_count()),
        (8, 12, 6)
    );
    let r = m.check();
    assert!(r.is_closed(), "{r:?}");
    // The faces agree with each other: the signed volume is the volume of the box.
    let mesh = m.mesh().unwrap();
    let vol: f64 = mesh
        .triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
            a.dot(b.cross(c))
        })
        .sum::<f64>()
        / 6.0;
    assert!(close(vol.abs(), 1e9), "{vol}");
    // Drawn edge by edge, the faces come as their loops close, with the same result.
    let mut step = SurfaceModel::new();
    for s in cube_edges(0.0) {
        add(&mut step, &[s]).unwrap();
    }
    // Each face takes its side as its loop closes; the top closed with no neighbour and faces
    // up like the bottom, so the two disagree with the sides between them.
    assert_eq!(step.face_count(), 6);
    let r = step.check();
    assert!(r.is_valid() && !r.is_closed());
    assert!(!r.reversed_edges.is_empty() && r.nonmanifold_edges.is_empty());
}

#[test]
fn erasing_a_box_edge_removes_its_two_faces() {
    let mut m = SurfaceModel::new();
    add(&mut m, &cube_edges(0.0)).unwrap();
    let e = edge_at(&m, [0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]);
    erase(&mut m, &[e]).unwrap();
    assert_eq!(m.face_count(), 4);
    let r = m.check();
    assert!(r.is_valid() && !r.is_closed());
    assert_eq!(r.border_edges.len(), 6);
    // Drawing the edge again closes the box again.
    add(&mut m, &[([0.0, 0.0, 0.0], [1000.0, 0.0, 0.0])]).unwrap();
    assert_eq!(m.face_count(), 6);
    assert!(m.check().is_closed());
}

#[test]
fn a_fin_on_a_box_makes_a_non_manifold_edge() {
    let mut m = SurfaceModel::new();
    add(&mut m, &cube_edges(0.0)).unwrap();
    // A fin out of the top edge at y = 0, in the plane x-z... rising above the box.
    poly(
        &mut m,
        &[
            [0.0, 0.0, 1000.0],
            [1000.0, 0.0, 1000.0],
            [1000.0, 0.0, 2000.0],
            [0.0, 0.0, 2000.0],
        ],
    );
    assert_eq!(m.face_count(), 7);
    let r = m.check();
    assert!(r.is_valid(), "{:?}", r.problems);
    assert_eq!(r.nonmanifold_edges.len(), 1);
    assert!(r.border_edges.len() == 3 && !r.is_closed());
}

#[test]
fn ends_weld_within_the_tolerance_and_crossings_split() {
    let mut m = SurfaceModel::new();
    add(&mut m, &[([0.0, 0.0, 0.0], [1000.0, 0.0, 0.0])]).unwrap();
    add(&mut m, &[([1000.004, 0.003, 0.0], [1000.0, 1000.0, 0.0])]).unwrap();
    assert_eq!(m.vertex_count(), 3, "the end welds to the vertex");
    add(&mut m, &[([1000.02, 1000.0, 0.0], [1000.02, 2000.0, 0.0])]).unwrap();
    assert_eq!(m.vertex_count(), 5, "0.02 mm is more than the tolerance");
    valid(&m);
    // Two crossing edges split each other.
    let mut m = SurfaceModel::new();
    add(&mut m, &[([0.0, 0.0, 0.0], [1000.0, 1000.0, 0.0])]).unwrap();
    add(&mut m, &[([0.0, 1000.0, 0.0], [1000.0, 0.0, 0.0])]).unwrap();
    assert_eq!((m.vertex_count(), m.edge_count()), (5, 4));
    assert!(
        m.vertices()
            .any(|(_, p)| p.distance(DVec3::new(500.0, 500.0, 0.0)) < 1e-9)
    );
    // An edge through a vertex is split there; an end on an edge splits the edge.
    add(&mut m, &[([500.0, 0.0, 0.0], [500.0, 1000.0, 0.0])]).unwrap();
    assert_eq!(m.edge_count(), 6);
    add(&mut m, &[([250.0, 250.0, 0.0], [250.0, 250.0, 900.0])]).unwrap();
    assert_eq!((m.vertex_count(), m.edge_count()), (9, 8));
    // An end close to an end of the edge it lands on welds to that end.
    add(&mut m, &[([500.0, 999.995, 3.0e-3], [-300.0, 2000.0, 0.0])]).unwrap();
    assert_eq!(m.vertex_count(), 10);
    // Skew edges in 3D do not meet.
    add(&mut m, &[([0.0, 500.0, 10.0], [1000.0, 500.0, 10.0])]).unwrap();
    assert_eq!(m.vertex_count(), 12);
    valid(&m);
}

#[test]
fn overlapping_edges_share_their_common_part() {
    let mut m = SurfaceModel::new();
    add(&mut m, &[([0.0, 0.0, 0.0], [1000.0, 0.0, 0.0])]).unwrap();
    add(&mut m, &[([500.0, 0.0, 0.0], [2000.0, 0.0, 0.0])]).unwrap();
    assert_eq!((m.vertex_count(), m.edge_count()), (4, 3));
    // Drawn again over the whole length: nothing new.
    add(&mut m, &[([0.0, 0.0, 0.0], [2000.0, 0.0, 0.0])]).unwrap();
    assert_eq!((m.vertex_count(), m.edge_count()), (4, 3));
    valid(&m);
}

#[test]
fn bad_edits_are_refused_and_change_nothing() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    let before = m.clone();
    let tol = LENGTH_TOLERANCE;
    assert_eq!(
        add(&mut m, &[([0.0, 0.0, 0.0], [0.005, 0.0, 0.0])]),
        Err(SurfaceError::TooShort { tol })
    );
    // Both ends weld to one vertex.
    assert_eq!(
        add(&mut m, &[([0.008, 0.0, 0.0], [0.0, 0.009, 0.0])]),
        Err(SurfaceError::TooShort { tol })
    );
    assert_eq!(
        add(&mut m, &[([f64::NAN, 0.0, 0.0], [1.0, 0.0, 0.0])]),
        Err(SurfaceError::BadPoint)
    );
    assert_eq!(
        add(&mut m, &[([2e9, 0.0, 0.0], [1.0, 0.0, 0.0])]),
        Err(SurfaceError::BadPoint)
    );
    // With valid geometry the split points of an edge are always farther apart than the
    // tolerance. A model with two vertices closer than that (here loose ones) is refused.
    let mut near = SurfaceModel::from_parts(
        [
            (VertexId(1), DVec3::new(500.0, 0.0, 0.0)),
            (VertexId(2), DVec3::new(500.008, 0.0, 0.0)),
        ]
        .into_iter()
        .collect(),
        BTreeMap::new(),
        BTreeMap::new(),
        3,
    );
    let before_near = near.clone();
    assert_eq!(
        add(&mut near, &[([0.0, 0.0, 0.0], [1000.0, 0.0, 0.0])]),
        Err(SurfaceError::TooClose { tol })
    );
    assert_eq!(near, before_near);
    assert_eq!(
        erase(&mut m, &[EdgeId(999)]),
        Err(SurfaceError::NoEdge(EdgeId(999)))
    );
    let many = Edit::AddEdges {
        edges: vec![[[0.0; 3], [1.0, 0.0, 0.0]]; MAX_EDIT_EDGES + 1],
    };
    assert!(matches!(m.apply(&many), Err(SurfaceError::Limit { .. })));
    assert_eq!(m, before);
    assert!(
        SurfaceError::Unsolved("x".into())
            .to_string()
            .contains("could not be solved")
    );
}

#[test]
fn the_limits_are_errors() {
    let mut vertices = BTreeMap::new();
    for i in 1..=(MAX_VERTICES as u32 + 1) {
        vertices.insert(VertexId(i), DVec3::new(i as f64, 0.0, 0.0));
    }
    let m = SurfaceModel::from_parts(
        vertices,
        BTreeMap::new(),
        BTreeMap::new(),
        MAX_VERTICES as u32 + 2,
    );
    assert_eq!(
        m.check().problems,
        vec![Problem::Limit {
            what: "vertices",
            max: MAX_VERTICES,
            count: MAX_VERTICES + 1
        }]
    );
    assert!(edit::check_limits(&m).is_err());
    assert!(m.problem().unwrap().contains("at most"));
    // An edit that would pass the limit fails, names the limit and changes nothing.
    let mut vertices = BTreeMap::new();
    for i in 1..=(MAX_VERTICES as u32 - 1) {
        vertices.insert(VertexId(i), DVec3::new(i as f64, 0.0, 0.0));
    }
    let mut full = SurfaceModel::from_parts(
        vertices,
        BTreeMap::new(),
        BTreeMap::new(),
        MAX_VERTICES as u32,
    );
    let before = full.clone();
    assert_eq!(
        add(&mut full, &[([0.0, 5000.0, 0.0], [10.0, 5000.0, 0.0])]),
        Err(SurfaceError::Limit {
            what: "vertices",
            max: MAX_VERTICES,
            count: MAX_VERTICES + 1
        })
    );
    assert_eq!(full, before);
}

// ---------------------------------------------------------------------------
// Determinism, files, transforms

#[test]
fn the_same_edits_give_the_same_ids_and_text() {
    let build = || {
        let mut m = SurfaceModel::new();
        add(&mut m, &cube_edges(0.0)).unwrap();
        poly(&mut m, &square(200.0, 200.0, 300.0, 1000.0));
        add(&mut m, &[([0.0, 500.0, 0.0], [1000.0, 500.0, 1000.0])]).unwrap();
        m
    };
    let (a, b) = (build(), build());
    assert_eq!(a, b);
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
    valid(&a);
}

#[test]
fn json_round_trip_keeps_everything() {
    let mut m = SurfaceModel::new();
    m.name = "Shed".into();
    add(&mut m, &cube_edges(0.0)).unwrap();
    poly(&mut m, &square(200.0, 200.0, 300.0, 1000.0));
    m.transform =
        DAffine3::from_rotation_z(0.3) * DAffine3::from_translation(DVec3::new(5.5, -3.25, 1e-7));
    let text = serde_json::to_string(&m).unwrap();
    let back: SurfaceModel = serde_json::from_str(&text).unwrap();
    assert_eq!(back, m);
    assert_eq!(serde_json::to_string(&back).unwrap(), text);
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["v"], 1);
    assert_eq!(v["name"], "Shed");
    assert_eq!(v["transform"].as_array().unwrap().len(), 12);
    // The identity transform and an empty name are left out.
    let plain = serde_json::to_value(SurfaceModel::new()).unwrap();
    assert!(plain.get("transform").is_none() && plain.get("name").is_none());
    assert_eq!(
        serde_json::from_value::<SurfaceModel>(plain).unwrap(),
        SurfaceModel::new()
    );
}

#[test]
fn bad_files_are_refused() {
    let parse = |v: serde_json::Value| serde_json::from_value::<SurfaceModel>(v);
    let base = serde_json::json!({"v": 1, "next_id": 9, "vertices": [[1, 0, 0, 0]]});
    assert!(parse(base.clone()).is_ok());
    for (key, value) in [
        ("v", serde_json::json!(2)),
        ("v", serde_json::json!(0)),
        ("vertices", serde_json::json!([[1, 0, 0, 0], [1, 1, 0, 0]])),
        ("edges", serde_json::json!([[2, 1, 1], [2, 1, 1]])),
        (
            "faces",
            serde_json::json!([
                {"id": 3, "normal": [0, 0, 1], "offset": 0, "outer": [1]},
                {"id": 3, "normal": [0, 0, 1], "offset": 0, "outer": [1]}
            ]),
        ),
    ] {
        let mut v = base.clone();
        v[key] = value;
        assert!(parse(v).is_err(), "{key}");
    }
    assert!(is_newer(
        &serde_json::json!({"kind": "surface_model", "v": 2})
    ));
    assert!(!is_newer(
        &serde_json::json!({"kind": "surface_model", "v": 1})
    ));
    assert!(!is_newer(&serde_json::json!({"kind": "extrude", "v": 9})));
}

#[test]
fn transforms_place_the_model() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    m.translate(DVec3::new(100.0, 0.0, 50.0));
    assert_eq!(
        m.bounds(),
        Some((
            DVec3::new(100.0, 0.0, 50.0),
            DVec3::new(1100.0, 1000.0, 50.0)
        ))
    );
    assert_eq!(m.to_local(DVec3::new(100.0, 0.0, 50.0)), Some(DVec3::ZERO));
    let mut moved = m.clone();
    moved.remap_plan(
        DVec3::new(7.0, 8.0, 9.0),
        DVec2::new(0.0, 2.0),
        DVec2::new(-2.0, 0.0),
        3.0,
    );
    assert_eq!(
        moved.bounds(),
        Some((DVec3::new(-1993.0, 8.0, 9.0), DVec3::new(7.0, 2008.0, 9.0)))
    );
    let mut flat = m.clone();
    flat.transform.matrix3.z_axis = DVec3::ZERO;
    assert_eq!(flat.to_local(DVec3::ZERO), None);
    assert_eq!(flat.problem(), Some(Problem::Transform.to_string()));
    let mut nums = Vec::new();
    m.for_each_number(&mut |x| nums.push(x));
    assert_eq!(nums.len(), 12 + 3 * 4 + 6);
    assert!(m.is_finite() && m.approx_bytes() > 0);
    m.transform.translation.x = f64::NAN;
    assert!(!m.is_finite());
    assert_eq!(m.problem(), Some(Problem::Range.to_string()));
}

// ---------------------------------------------------------------------------
// The check

fn model_with(
    faces: Vec<(u32, Face)>,
    edges: Vec<(u32, [u32; 2])>,
    verts: Vec<(u32, P)>,
) -> SurfaceModel {
    SurfaceModel::from_parts(
        verts
            .into_iter()
            .map(|(i, p)| (VertexId(i), DVec3::from(p)))
            .collect(),
        edges
            .into_iter()
            .map(|(i, [a, b])| {
                (
                    EdgeId(i),
                    Edge {
                        v: [VertexId(a), VertexId(b)],
                    },
                )
            })
            .collect(),
        faces.into_iter().map(|(i, f)| (FaceId(i), f)).collect(),
        100,
    )
}

fn tri_face(l: &[u32]) -> Face {
    Face {
        plane: Plane {
            normal: DVec3::Z,
            offset: 0.0,
        },
        outer: l.iter().map(|v| VertexId(*v)).collect(),
        holes: Vec::new(),
    }
}

#[test]
fn the_check_finds_each_kind_of_problem() {
    let v = || {
        vec![
            (1, [0.0, 0.0, 0.0]),
            (2, [1000.0, 0.0, 0.0]),
            (3, [0.0, 1000.0, 0.0]),
        ]
    };
    let e = || vec![(4, [1, 2]), (5, [2, 3]), (6, [3, 1])];
    let first = |m: &SurfaceModel| m.check().problems.first().cloned();
    let good = model_with(vec![(7, tri_face(&[1, 2, 3]))], e(), v());
    valid(&good);
    let cases: Vec<(SurfaceModel, Problem)> = vec![
        (
            model_with(
                vec![],
                e(),
                vec![(0, [0.0; 3]), (2, [1.0, 0.0, 0.0]), (3, [0.0, 1.0, 0.0])],
            ),
            Problem::BadId(0),
        ),
        (
            model_with(vec![], vec![(4, [1, 9])], v()),
            Problem::EdgeVertex(EdgeId(4)),
        ),
        (
            model_with(vec![], vec![(4, [1, 1])], v()),
            Problem::LoopEdge(EdgeId(4)),
        ),
        (
            model_with(vec![], vec![(4, [1, 2]), (5, [2, 1]), (6, [2, 3])], v()),
            Problem::DoubleEdge(EdgeId(4), EdgeId(5)),
        ),
        (
            model_with(vec![], vec![(4, [1, 2])], v()),
            Problem::LooseVertex(VertexId(3)),
        ),
        (
            model_with(vec![(7, tri_face(&[1, 2]))], e(), v()),
            Problem::ShortLoop(FaceId(7)),
        ),
        (
            model_with(vec![(7, tri_face(&[1, 2, 9]))], e(), v()),
            Problem::FaceVertex(FaceId(7)),
        ),
        (
            model_with(
                vec![(7, tri_face(&[1, 2, 3]))],
                vec![(4, [1, 2]), (5, [2, 3])],
                v(),
            ),
            Problem::MissingEdge(FaceId(7), VertexId(3), VertexId(1)),
        ),
        (
            model_with(vec![(7, tri_face(&[1, 2, 3, 1, 2, 3]))], e(), v()),
            Problem::RepeatedVertex(FaceId(7), VertexId(1)),
        ),
        (
            model_with(
                vec![(
                    7,
                    Face {
                        holes: vec![vec![VertexId(1), VertexId(2), VertexId(3)]],
                        ..tri_face(&[1, 2, 3])
                    },
                )],
                e(),
                v(),
            ),
            Problem::RepeatedEdge(FaceId(7), EdgeId(4)),
        ),
        (
            model_with(
                vec![(
                    7,
                    Face {
                        plane: Plane {
                            normal: DVec3::splat(1.0),
                            offset: 0.0,
                        },
                        ..tri_face(&[1, 2, 3])
                    },
                )],
                e(),
                v(),
            ),
            Problem::BadNormal(FaceId(7)),
        ),
        (
            model_with(
                vec![(7, tri_face(&[1, 2, 3]))],
                e(),
                vec![
                    (1, [0.0; 3]),
                    (2, [1000.0, 0.0, 0.0]),
                    (3, [0.0, 1000.0, 1.0]),
                ],
            ),
            Problem::OffPlane(FaceId(7), VertexId(3)),
        ),
        (
            model_with(vec![(7, tri_face(&[1, 3, 2]))], e(), v()),
            Problem::OuterSide(FaceId(7)),
        ),
        (
            model_with(
                vec![],
                vec![(4, [1, 2]), (5, [2, 3])],
                vec![
                    (1, [0.0; 3]),
                    (2, [1000.0, 0.0, 0.0]),
                    (3, [1000.003, 0.0, 0.0]),
                ],
            ),
            Problem::CloseVertices(VertexId(2), VertexId(3)),
        ),
        (
            model_with(
                vec![],
                vec![(4, [1, 2])],
                vec![(1, [0.0; 3]), (2, [0.004, 0.0, 0.0])],
            ),
            Problem::CloseVertices(VertexId(1), VertexId(2)),
        ),
        (
            model_with(
                vec![],
                vec![(4, [1, 2]), (5, [3, 9])],
                vec![
                    (1, [0.0; 3]),
                    (2, [1000.0, 0.0, 0.0]),
                    (3, [500.0, 0.001, 0.0]),
                    (9, [500.0, 900.0, 0.0]),
                ],
            ),
            Problem::VertexOnEdge(VertexId(3), EdgeId(4)),
        ),
        (
            model_with(
                vec![],
                vec![(4, [1, 2]), (5, [3, 9])],
                vec![
                    (1, [0.0; 3]),
                    (2, [1000.0, 0.0, 0.0]),
                    (3, [500.0, -500.0, 0.0]),
                    (9, [500.0, 500.0, 0.0]),
                ],
            ),
            Problem::Crossing(EdgeId(4), EdgeId(5)),
        ),
        (
            model_with(
                vec![(7, tri_face(&[1, 2, 3])), (8, tri_face(&[1, 2, 3]))],
                e(),
                v(),
            ),
            Problem::Overlap(FaceId(7), FaceId(8), EdgeId(4)),
        ),
        (
            model_with(
                vec![],
                e(),
                vec![(1, [0.0; 3]), (2, [1e10, 0.0, 0.0]), (3, [0.0, 1.0, 0.0])],
            ),
            Problem::Range,
        ),
    ];
    for (m, want) in cases {
        assert_eq!(first(&m), Some(want.clone()), "{want}");
        assert!(!m.check().is_valid());
    }
    // Holes: the wrong way round, and outside the outer loop.
    let hv = vec![
        (1, [0.0, 0.0, 0.0]),
        (2, [1000.0, 0.0, 0.0]),
        (3, [1000.0, 1000.0, 0.0]),
        (4, [0.0, 1000.0, 0.0]),
        (5, [200.0, 200.0, 0.0]),
        (6, [400.0, 200.0, 0.0]),
        (8, [200.0, 400.0, 0.0]),
        (9, [2200.0, 200.0, 0.0]),
        (10, [2400.0, 200.0, 0.0]),
        (11, [2200.0, 400.0, 0.0]),
    ];
    let he = vec![
        (20, [1, 2]),
        (21, [2, 3]),
        (22, [3, 4]),
        (23, [4, 1]),
        (24, [5, 6]),
        (25, [6, 8]),
        (26, [8, 5]),
        (27, [9, 10]),
        (28, [10, 11]),
        (29, [11, 9]),
    ];
    let holed = |hole: &[u32]| Face {
        holes: vec![hole.iter().map(|v| VertexId(*v)).collect()],
        ..tri_face(&[1, 2, 3, 4])
    };
    let m = model_with(vec![(30, holed(&[5, 8, 6]))], he.clone(), hv.clone());
    assert!(m.check().is_valid(), "{:?}", m.check().problems);
    let m = model_with(vec![(30, holed(&[5, 6, 8]))], he.clone(), hv.clone());
    assert_eq!(first(&m), Some(Problem::HoleSide(FaceId(30))));
    let m = model_with(vec![(30, holed(&[9, 11, 10]))], he, hv);
    assert_eq!(first(&m), Some(Problem::HoleOutside(FaceId(30))));
    // A short edge, and the texts of the problems.
    let m = model_with(
        vec![],
        vec![(4, [1, 2])],
        vec![(1, [0.0; 3]), (2, [0.0, 0.0, 0.004])],
    );
    assert!(m.check().problems.contains(&Problem::ShortEdge(EdgeId(4))));
    for p in [
        Problem::ShortEdge(EdgeId(4)),
        Problem::Limit {
            what: "edges",
            max: 1,
            count: 2,
        },
    ] {
        assert!(!p.to_string().is_empty());
    }
    // An id at or above next_id.
    let mut m = good.clone();
    m.next_id = 5;
    assert_eq!(first(&m), Some(Problem::BadId(5)));
}

#[test]
fn the_arrangement_finds_regions_holes_and_touching_loops() {
    // Two squares that touch at one corner, one of them with a triangle hole that touches its
    // outer loop at a vertex, and a stray edge.
    let mut m = SurfaceModel::new();
    add(
        &mut m,
        &[
            ([0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]),
            ([1000.0, 0.0, 0.0], [1000.0, 1000.0, 0.0]),
            ([1000.0, 1000.0, 0.0], [0.0, 1000.0, 0.0]),
            ([0.0, 1000.0, 0.0], [0.0, 0.0, 0.0]),
            ([1000.0, 1000.0, 0.0], [2000.0, 1000.0, 0.0]),
            ([2000.0, 1000.0, 0.0], [2000.0, 2000.0, 0.0]),
            ([2000.0, 2000.0, 0.0], [1000.0, 2000.0, 0.0]),
            ([1000.0, 2000.0, 0.0], [1000.0, 1000.0, 0.0]),
            ([0.0, 0.0, 0.0], [300.0, 100.0, 0.0]),
            ([300.0, 100.0, 0.0], [100.0, 300.0, 0.0]),
            ([100.0, 300.0, 0.0], [0.0, 0.0, 0.0]),
            ([1200.0, 1200.0, 0.0], [1500.0, 1500.0, 0.0]),
        ],
    )
    .unwrap();
    valid(&m);
    assert_eq!(m.face_count(), 3);
    let a = areas(&m);
    let tri = 0.5 * (300.0 * 300.0 - 100.0 * 100.0);
    assert!(
        close(a[0], tri) && close(a[1], 1e6 - tri) && close(a[2], 1e6),
        "{a:?}"
    );
    let plane = Plane::new(DVec3::ZERO, DVec3::Z).unwrap();
    let edges = m.edges().map(|(_, e)| e.key()).collect();
    let arr = Arrangement::solve(&m, plane, &edges);
    assert_eq!(arr.regions.len(), 3);
    let holed = arr.regions.iter().find(|r| !r.holes.is_empty()).unwrap();
    assert!(close(holed.area, 1e6 - tri));
    assert!(arr.sample(holed).is_ok());
}

#[test]
fn a_face_on_a_tilted_plane_and_far_from_the_origin() {
    // A face in a sloped plane, 50 km out: the tolerances grow with the coordinates.
    let o = DVec3::new(5e7, -3e7, 1e6);
    let (u, v) = (DVec3::new(1.0, 0.0, 0.5), DVec3::new(0.0, 1.0, 0.25));
    let pts: Vec<P> = [(0.0, 0.0), (4000.0, 0.0), (4000.0, 3000.0), (0.0, 3000.0)]
        .iter()
        .map(|(a, b)| (o + u * *a + v * *b).to_array())
        .collect();
    let mut m = SurfaceModel::new();
    poly(&mut m, &pts);
    assert_eq!(m.face_count(), 1);
    let mid = |a: usize, b: usize| ((DVec3::from(pts[a]) + DVec3::from(pts[b])) / 2.0).to_array();
    add(&mut m, &[(mid(0, 1), mid(2, 3))]).unwrap();
    assert_eq!(m.face_count(), 2);
    let n = m.faces().next().unwrap().1.plane.normal;
    assert!(n.z > 0.0 && close(n.dot(u.cross(v).normalize()), 1.0));
    valid(&m);
}

// ---------------------------------------------------------------------------
// Property tests

#[derive(Clone, Debug)]
enum Op {
    Add(P, P),
    Erase(usize),
    EraseFace(usize),
    Reverse(usize),
}

fn grid_point() -> impl Strategy<Value = P> {
    (0..7i32, 0..7i32, 0..3i32)
        .prop_map(|(x, y, z)| [x as f64 * 500.0, y as f64 * 500.0, z as f64 * 500.0])
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => (grid_point(), grid_point()).prop_map(|(a, b)| Op::Add(a, b)),
        2 => (0..64usize).prop_map(Op::Erase),
        1 => (0..64usize).prop_map(Op::EraseFace),
        1 => (0..64usize).prop_map(Op::Reverse),
    ]
}

fn run_op(m: &mut SurfaceModel, op: &Op) -> Result<(), SurfaceError> {
    let pick = |n: usize, k: usize| (n > 0).then(|| k % n);
    match op {
        Op::Add(a, b) => add(m, &[(*a, *b)]),
        Op::Erase(k) => match pick(m.edge_count(), *k) {
            Some(i) => {
                let id = m.edges().nth(i).unwrap().0;
                erase(m, &[id])
            }
            None => Ok(()),
        },
        Op::EraseFace(k) => match pick(m.face_count(), *k) {
            Some(i) => {
                let id = m.faces().nth(i).unwrap().0;
                m.apply(&Edit::EraseFaces { faces: vec![id] })
            }
            None => Ok(()),
        },
        Op::Reverse(k) => match pick(m.face_count(), *k) {
            Some(i) => {
                let id = m.faces().nth(i).unwrap().0;
                m.apply(&Edit::ReverseFaces { faces: vec![id] })
            }
            None => Ok(()),
        },
    }
}

/// The faces as (area, normal direction up to its sign), sorted by area.
fn face_shapes(m: &SurfaceModel) -> Vec<(f64, [i64; 3])> {
    let mut out: Vec<(f64, [i64; 3])> = m
        .faces()
        .map(|(id, f)| {
            let n = f.plane.normal;
            let n = if (n.z, n.y, n.x) < (0.0, 0.0, 0.0) {
                -n
            } else {
                n
            };
            (
                m.face_area(id).unwrap(),
                (n * 1000.0).round().to_array().map(|x| x as i64),
            )
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    out
}

proptest! {
    // PROPTEST_CASES=5000 runs a longer search.
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|s| s.parse().ok()).unwrap_or(48),
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_edits_keep_the_model_valid_and_are_deterministic(ops in prop::collection::vec(op(), 1..40)) {
        let mut m = SurfaceModel::new();
        let mut twin = SurfaceModel::new();
        for op in &ops {
            let before = m.clone();
            let r = run_op(&mut m, op);
            let r2 = run_op(&mut twin, op);
            prop_assert_eq!(&r, &r2);
            if r.is_err() {
                prop_assert_eq!(&m, &before);
            }
            let report = m.check();
            prop_assert!(report.is_valid(), "{:?} after {:?}", report.problems, op);
            prop_assert_eq!(&m, &twin);
        }
        let text = serde_json::to_string(&m).unwrap();
        let back: SurfaceModel = serde_json::from_str(&text).unwrap();
        prop_assert_eq!(&back, &m);
    }

    #[test]
    fn drawing_an_erased_edge_again_restores_its_faces(
        adds in prop::collection::vec((grid_point(), grid_point()), 1..16),
        pick in 0..64usize,
    ) {
        let mut m = SurfaceModel::new();
        for (a, b) in &adds {
            let _ = add(&mut m, &[(*a, *b)]);
        }
        prop_assume!(m.edge_count() > 0);
        let (id, e) = m.edges().nth(pick % m.edge_count()).unwrap();
        // An edge of a hole is left out: a loop drawn inside an empty region fills only the loop,
        // so the face around an emptied hole does not come back.
        prop_assume!(!m.faces().any(|(_, f)| f.holes.iter().any(|h| {
            (0..h.len()).any(|i| key(h[i], h[(i + 1) % h.len()]) == e.key())
        })));
        let (a, b) = (m.vertex(e.v[0]).unwrap().to_array(), m.vertex(e.v[1]).unwrap().to_array());
        let before = face_shapes(&m);
        erase(&mut m, &[id]).unwrap();
        prop_assert!(m.check().is_valid());
        add(&mut m, &[(a, b)]).unwrap();
        prop_assert!(m.check().is_valid());
        let after = face_shapes(&m);
        // Every face is back (drawing may fill an empty region next to it too).
        let mut rest = after.clone();
        for f in &before {
            let i = rest.iter().position(|g| g.1 == f.1 && close(g.0, f.0));
            prop_assert!(i.is_some(), "face {:?} is missing: {:?} -> {:?}", f, before, after);
            rest.remove(i.unwrap());
        }
    }
}

fn free_point() -> impl Strategy<Value = P> {
    // Points anywhere in a box, and one plane in three on the ground, so faces still form.
    (
        0.0..3000.0f64,
        0.0..3000.0f64,
        prop_oneof![Just(0.0), 0.0..1000.0f64],
    )
        .prop_map(|(x, y, z)| [x, y, z])
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|s| s.parse().ok()).unwrap_or(48),
        ..ProptestConfig::default()
    })]

    #[test]
    fn edits_at_any_coordinates_keep_the_model_valid(
        adds in prop::collection::vec((free_point(), free_point()), 1..30),
        erases in prop::collection::vec(0..64usize, 0..6),
    ) {
        let mut m = SurfaceModel::new();
        for (a, b) in &adds {
            let before = m.clone();
            if add(&mut m, &[(*a, *b)]).is_err() {
                prop_assert_eq!(&m, &before);
            }
            let r = m.check();
            prop_assert!(r.is_valid(), "{:?}", r.problems);
        }
        for k in erases {
            if m.edge_count() == 0 {
                break;
            }
            let id = m.edges().nth(k % m.edge_count()).unwrap().0;
            erase(&mut m, &[id]).unwrap();
            let r = m.check();
            prop_assert!(r.is_valid(), "{:?}", r.problems);
        }
    }
}

// ---------------------------------------------------------------------------
// Measurement of the limits (run in release mode)

/// Builds models near the limits and prints the time of one edit and the size of the text:
/// `cargo test -p kadakas-core --release --lib surface::tests::perf_limits -- --ignored --nocapture`.
#[test]
#[ignore = "timing test: run in release mode"]
fn perf_limits() {
    use std::time::Instant;
    let n: usize = std::env::var("PERF_SIZES")
        .ok()
        .and_then(|s| s.parse().ok())
        .map_or(150, |k: usize| ((k as f64).sqrt() as usize).max(10))
        // 200 x 200 is the largest grid within the limits (40,401 vertices, 80,400 edges).
        .min(200);
    // A grid of n x n squares in one plane: the worst case for the solve of a plane.
    let mut m = SurfaceModel::new();
    let w = 1000.0;
    let mut segs = Vec::new();
    for i in 0..=n {
        let t = i as f64 * w;
        segs.push([[t, 0.0, 0.0], [t, n as f64 * w, 0.0]]);
        segs.push([[0.0, t, 0.0], [n as f64 * w, t, 0.0]]);
    }
    let start = Instant::now();
    m.apply(&Edit::AddEdges { edges: segs }).unwrap();
    println!(
        "grid {n}x{n}: {} vertices, {} edges, {} faces in {:?}",
        m.vertex_count(),
        m.edge_count(),
        m.face_count(),
        start.elapsed()
    );
    let start = Instant::now();
    m.apply(&Edit::AddEdges {
        edges: vec![
            [[250.0, 250.0, 0.0], [750.0, 750.0, 0.0]],
            [[750.0, 750.0, 0.0], [250.0, 750.0, 0.0]],
            [[250.0, 750.0, 0.0], [250.0, 250.0, 0.0]],
        ],
    })
    .unwrap();
    println!("one triangle in a crowded plane: {:?}", start.elapsed());
    let start = Instant::now();
    let e = m.edges().next().unwrap().0;
    m.apply(&Edit::EraseEdges { edges: vec![e] }).unwrap();
    println!("erase one edge: {:?}", start.elapsed());
    let start = Instant::now();
    assert!(m.problem().is_none());
    println!("quick check (each command): {:?}", start.elapsed());
    let start = Instant::now();
    let r = m.check();
    println!(
        "full check: {:?} ({} problems)",
        start.elapsed(),
        r.problems.len()
    );
    let text = serde_json::to_string(&m).unwrap();
    println!(
        "text: {} bytes, {:.1} bytes per vertex, {:.1} per edge",
        text.len(),
        text.len() as f64 / m.vertex_count() as f64,
        text.len() as f64 / m.edge_count() as f64
    );
    // Many small boxes, each in its own planes: one edit adds a box.
    let mut b = SurfaceModel::new();
    let start = Instant::now();
    let boxes = n * 4;
    for k in 0..boxes {
        let edges: Vec<[[f64; 3]; 2]> = cube_edges(k as f64 * 2000.0)
            .into_iter()
            .map(|(a, b)| [a, b])
            .collect();
        b.apply(&Edit::AddEdges { edges }).unwrap();
    }
    println!(
        "{boxes} boxes one by one: {:?}, {} faces",
        start.elapsed(),
        b.face_count()
    );
}
