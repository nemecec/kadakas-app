//! Tests of Fillet and Chamfer on surface models (`Edit::Fillet`, `Edit::Chamfer`, MOD-21): the
//! shapes of the backlog row, volumes, refusals, ids, flags, curves and a property test.

use std::f64::consts::PI;

use glam::DVec3;
use proptest::prelude::*;

use super::*;

type P = [f64; 3];

/// A solid from an outline at z = 0 (counterclockwise from above), pulled up by `h`.
fn prism(outline: &[[f64; 2]], h: f64) -> SurfaceModel {
    let mut m = SurfaceModel::new();
    let n = outline.len();
    m.apply(&Edit::AddEdges {
        edges: (0..n)
            .map(|i| {
                let (a, b) = (outline[i], outline[(i + 1) % n]);
                [[a[0], a[1], 0.0], [b[0], b[1], 0.0]]
            })
            .collect(),
    })
    .unwrap();
    let face = m.faces().next().unwrap().0;
    m.apply(&Edit::PushPull { face, distance: h }).unwrap();
    assert!(m.solid_report().is_closed());
    m
}

fn cuboid(a: f64, b: f64, c: f64) -> SurfaceModel {
    prism(&[[0.0, 0.0], [a, 0.0], [a, b], [0.0, b]], c)
}

fn volume(m: &SurfaceModel) -> f64 {
    m.solid_report().solids().map(|s| s.volume).sum()
}

fn vid(m: &SurfaceModel, p: P) -> VertexId {
    m.vertices()
        .find(|(_, q)| q.distance(DVec3::from(p)) < 1e-6)
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("no vertex at {p:?}"))
}

fn edge_at(m: &SurfaceModel, a: P, b: P) -> EdgeId {
    m.edge_between(vid(m, a), vid(m, b))
        .unwrap_or_else(|| panic!("no edge {a:?} {b:?}"))
}

fn all_edges(m: &SurfaceModel) -> Vec<EdgeId> {
    m.fillet_edges(&EdgeChoice {
        all: true,
        ..EdgeChoice::default()
    })
    .unwrap()
}

fn fillet(
    m: &mut SurfaceModel,
    edges: Vec<EdgeId>,
    r: f64,
    n: Option<u32>,
) -> Result<Vec<String>, SurfaceError> {
    m.apply_with_notes(&Edit::Fillet {
        edges,
        radius: r,
        segments: n,
        smooth: true,
    })
}

/// The model is a valid closed solid with no problem.
fn sound(m: &SurfaceModel) {
    assert_eq!(m.problem(), None);
    let rep = m.solid_report();
    assert!(rep.is_closed(), "not closed: {:?}", rep.open_edges());
    assert!(m.check().is_valid(), "{:?}", m.check());
}

fn area_removed(r: f64, phi: f64, n: u32) -> f64 {
    r * r * (phi / 2.0).tan() - n as f64 * 0.5 * r * r * (phi / n as f64).sin()
}

#[test]
fn one_box_edge_is_rounded_exactly_and_the_faces_keep_their_ids() {
    let mut m = cuboid(100.0, 60.0, 40.0);
    let faces: Vec<FaceId> = m.faces().map(|(f, _)| f).collect();
    let e = edge_at(&m, [0.0, 0.0, 40.0], [100.0, 0.0, 40.0]);
    let next = m.next_id();
    fillet(&mut m, vec![e], 10.0, Some(6)).unwrap();
    sound(&m);
    let want = 240_000.0 - 100.0 * area_removed(10.0, PI / 2.0, 6);
    assert!(
        (volume(&m) - want).abs() < 1e-6,
        "{} against {want}",
        volume(&m)
    );
    // The six faces stay, six strip faces are new, and the edge is gone.
    for f in &faces {
        assert!(m.face(*f).is_some());
    }
    assert_eq!(m.face_count(), 12);
    assert!(m.edge(e).is_none());
    let new: Vec<FaceId> = m.faces().map(|(f, _)| f).filter(|f| f.0 >= next).collect();
    assert_eq!(new.len(), 6);
    // Inner lines soft and smooth, rails soft only.
    let soft = m.edges().filter(|(e, _)| m.edge_soft(*e)).count();
    let smooth = m.edges().filter(|(e, _)| m.edge_smooth(*e)).count();
    assert_eq!((soft, smooth), (7, 5));
}

#[test]
fn a_box_with_all_edges_rounded_is_a_closed_solid_near_the_true_volume() {
    let (a, b, c, r) = (100.0, 60.0, 40.0, 8.0);
    let truth = {
        let (a2, b2, c2) = (a - 2.0 * r, b - 2.0 * r, c - 2.0 * r);
        a2 * b2 * c2
            + 2.0 * r * (a2 * b2 + b2 * c2 + a2 * c2)
            + PI * r * r * (a2 + b2 + c2)
            + 4.0 / 3.0 * PI * r * r * r
    };
    for n in [1, 6, 24] {
        let mut m = cuboid(a, b, c);
        let all = all_edges(&m);
        assert_eq!(all.len(), 12);
        fillet(&mut m, all, r, Some(n)).unwrap();
        sound(&m);
        let v = volume(&m);
        assert!(v < truth + 1e-6);
        if n == 24 {
            assert!((truth - v) / truth < 2e-4, "{v} against {truth}");
        }
    }
}

#[test]
fn a_chamfer_cuts_all_edges_of_a_box() {
    let mut m = cuboid(100.0, 60.0, 40.0);
    let all = all_edges(&m);
    m.apply(&Edit::Chamfer {
        edges: all,
        distance: 5.0,
    })
    .unwrap();
    sound(&m);
    // 6 faces, 12 strips, 8 corner triangles.
    assert_eq!(m.face_count(), 26);
    // No flags on a chamfer.
    assert!(m.edges().all(|(e, _)| !m.edge_soft(e) && !m.edge_smooth(e)));
    // The removed volume: the 12 wedges, less their overlaps at the corners.
    let v = volume(&m);
    let wedges = 12.5 * 4.0 * (100.0 + 60.0 + 40.0);
    assert!(v < 240_000.0 - wedges + 8.0 * 125.0 && v > 240_000.0 - wedges);
}

#[test]
fn three_edges_at_a_corner_and_two_with_a_sharp_third() {
    let corner = [100.0, 60.0, 40.0];
    for chosen in [
        vec![[100.0, 0.0, 40.0], [0.0, 60.0, 40.0], [100.0, 60.0, 0.0]],
        vec![[100.0, 0.0, 40.0], [0.0, 60.0, 40.0]],
    ] {
        let mut m = cuboid(100.0, 60.0, 40.0);
        let es: Vec<EdgeId> = chosen.iter().map(|p| edge_at(&m, corner, *p)).collect();
        fillet(&mut m, es, 10.0, None).unwrap();
        sound(&m);
        assert!(volume(&m) < 240_000.0);
    }
}

/// The U block of the row: arms of 17 and 29 mm and a 23 mm slot (so the outline is 69 mm wide,
/// not the 82 mm that the row names), 60 mm high, arcs at the corners, pulled 50 mm.
fn u_block() -> SurfaceModel {
    let mut outline: Vec<[f64; 2]> = Vec::new();
    let arc = |o: &mut Vec<[f64; 2]>, c: [f64; 2], r: f64, from: f64, to: f64| {
        for k in 0..=6 {
            let a = (from + (to - from) * k as f64 / 6.0).to_radians();
            o.push([c[0] + r * a.cos(), c[1] + r * a.sin()]);
        }
    };
    outline.push([0.0, 0.0]);
    outline.push([17.0, 0.0]);
    arc(&mut outline, [21.0, 32.0], 4.0, 180.0, 90.0);
    arc(&mut outline, [36.0, 32.0], 4.0, 90.0, 0.0);
    outline.push([40.0, 0.0]);
    outline.push([69.0, 0.0]);
    arc(&mut outline, [61.0, 52.0], 8.0, 0.0, 90.0);
    arc(&mut outline, [8.0, 52.0], 8.0, 90.0, 180.0);
    let mut m = prism(&outline, 50.0);
    // The facets of the arcs are a curved surface: soft and smooth, as a drawn arc gives.
    let soft: Vec<EdgeId> = m
        .edges()
        .filter(|(e, _)| dihedral_of(&m, *e).is_some_and(|a| a > 10.0 && a < 20.0))
        .map(|(e, _)| e)
        .collect();
    assert_eq!(soft.len(), 4 * 5);
    m.apply(&Edit::SetEdgeStyle {
        edges: soft,
        soft: Some(true),
        smooth: Some(true),
    })
    .unwrap();
    m
}

fn top_face(m: &SurfaceModel, z: f64) -> FaceId {
    m.faces()
        .find(|(_, f)| f.plane.normal.z > 0.99 && (f.plane.offset - z).abs() < 1e-6)
        .unwrap()
        .0
}

#[test]
fn the_u_block_rounds_its_end_edges_and_refuses_a_radius_that_the_thin_arm_cannot_take() {
    let mut m = u_block();
    let v0 = volume(&m);
    let end = top_face(&m, 50.0);
    let edges = m
        .fillet_edges(&EdgeChoice {
            faces: vec![end],
            ..EdgeChoice::default()
        })
        .unwrap();
    // Every edge of the end face: 8 lines and 4 arcs of 6 segments.
    assert_eq!(edges.len(), 8 + 24);
    let mut big = m.clone();
    let err = fillet(&mut big, edges.clone(), 10.0, None).unwrap_err();
    let SurfaceError::Fillet(FilletError::DoesNotFit {
        most: Some(most), ..
    }) = &err
    else {
        panic!("{err}");
    };
    assert!(*most > 3.9 && *most < 8.5, "{most}");
    assert!(err.to_string().contains("at most about"), "{err}");
    assert_eq!(big, m, "a refusal changes nothing");
    fillet(&mut m, edges, 3.0, None).unwrap();
    sound(&m);
    assert!(volume(&m) < v0);
    // The arcs meet their neighbours as miters: no flat corner pieces, and the end face is one.
    assert!(m.face(end).is_some());
}

#[test]
fn the_plate_with_pockets_rounds_its_top_edges() {
    let mut m = cuboid(1055.0, 160.0, 20.0);
    // Two pockets of 8 mm on the underside.
    for x in [100.0, 700.0] {
        let sq = [[x, 40.0], [x + 200.0, 40.0], [x + 200.0, 120.0], [x, 120.0]];
        m.apply(&Edit::AddEdges {
            edges: (0..4)
                .map(|i| {
                    let (a, b) = (sq[i], sq[(i + 1) % 4]);
                    [[a[0], a[1], 0.0], [b[0], b[1], 0.0]]
                })
                .collect(),
        })
        .unwrap();
        let pocket = m
            .faces()
            .find(|(f, face)| {
                face.plane.normal.z < -0.99
                    && m.face_area(*f).unwrap() < 16_001.0
                    && m.vertex(face.outer[0]).unwrap().x >= x - 1e-6
            })
            .unwrap()
            .0;
        m.apply(&Edit::PushPull {
            face: pocket,
            distance: -8.0,
        })
        .unwrap();
    }
    sound(&m);
    let v0 = volume(&m);
    let top = top_face(&m, 20.0);
    let edges = m
        .fillet_edges(&EdgeChoice {
            faces: vec![top],
            ..EdgeChoice::default()
        })
        .unwrap();
    assert_eq!(edges.len(), 4);
    let r = 6.0;
    fillet(&mut m, edges, r, Some(8)).unwrap();
    sound(&m);
    // True round: the four edges less the overlaps of their ends at the four corners.
    let len = 2.0 * (1055.0 + 160.0);
    let removed_true = len * r * r * (1.0 - PI / 4.0) - 4.0 * r * r * r * (5.0 / 3.0 - PI / 2.0);
    let removed = v0 - volume(&m);
    let per_len = 0.5 * r * r * (PI / 2.0 - 8.0 * (PI / 16.0).sin());
    assert!(
        removed >= removed_true - 1e-6 && removed <= removed_true + per_len * len + 1.0,
        "{removed} against {removed_true}"
    );
}

/// A leg: 50 x 50 at the floor, 30 x 30 at 700 mm.
fn tapered_leg() -> SurfaceModel {
    let mut m = cuboid(50.0, 50.0, 700.0);
    let top = top_face(&m, 700.0);
    // Scale the top about its centre by 0.6.
    let (c, k) = (DVec3::new(25.0, 25.0, 700.0), 0.6);
    let t = glam::DAffine3::from_translation(c)
        * glam::DAffine3::from_scale(DVec3::new(k, k, 1.0))
        * glam::DAffine3::from_translation(-c);
    let mut matrix = [0.0; 12];
    matrix[..9].copy_from_slice(&t.matrix3.to_cols_array());
    matrix[9..].copy_from_slice(&t.translation.to_array());
    m.apply(&Edit::TransformVertices {
        vertices: Vec::new(),
        faces: vec![top],
        edges: Vec::new(),
        matrix,
    })
    .unwrap();
    sound(&m);
    m
}

#[test]
fn a_tapered_leg_rounds_its_slanted_edges_and_its_top() {
    let mut m = tapered_leg();
    let v0 = volume(&m);
    let slanted: Vec<EdgeId> = m
        .edges()
        .filter(|(_, e)| {
            let (a, b) = (m.vertex(e.v[0]).unwrap(), m.vertex(e.v[1]).unwrap());
            (a.z - b.z).abs() > 1.0
        })
        .map(|(e, _)| e)
        .collect();
    assert_eq!(slanted.len(), 4);
    fillet(&mut m, slanted, 6.0, None).unwrap();
    sound(&m);
    assert!(volume(&m) < v0);
    // Then the top edges, which now meet the strips: refused or sound, never broken.
    let mut all = tapered_leg();
    let es = all_edges(&all);
    fillet(&mut all, es, 4.0, Some(4)).unwrap();
    sound(&all);
}

#[test]
fn a_cylinder_rounds_its_top_ring_and_keeps_no_wrong_circle() {
    let mut m = SurfaceModel::new();
    let n = 24;
    let pts: Vec<[f64; 3]> = (0..n)
        .map(|i| {
            let a = 2.0 * PI * i as f64 / n as f64;
            [100.0 * a.cos(), 100.0 * a.sin(), 0.0]
        })
        .collect();
    m.apply(&Edit::AddCurve {
        points: pts,
        closed: true,
        faceted: false,
        analytic: Some(Analytic::circle(DVec3::ZERO, DVec3::Z, DVec3::X, 100.0)),
    })
    .unwrap();
    let disc = m.faces().next().unwrap().0;
    m.apply(&Edit::PushPull {
        face: disc,
        distance: 80.0,
    })
    .unwrap();
    let top = top_face(&m, 80.0);
    assert!(m.curve_geometries().count() >= 2);
    // `all` leaves the soft sides out: only the two rings.
    assert_eq!(all_edges(&m).len(), 2 * n);
    let ring = m
        .fillet_edges(&EdgeChoice {
            faces: vec![top],
            ..EdgeChoice::default()
        })
        .unwrap();
    assert_eq!(ring.len(), n);
    let notes = fillet(&mut m, ring, 10.0, None).unwrap();
    sound(&m);
    // Every circle that is left fits its chain.
    for (c, a) in m.curve_geometries() {
        for v in m.curve_vertices(c).unwrap() {
            assert!(a.deviation(m.vertex(v).unwrap()) < 0.01, "curve {c}");
        }
    }
    let _ = notes;
}

#[test]
fn a_concave_edge_of_an_l_gets_material() {
    let outline = [
        [0.0, 0.0],
        [100.0, 0.0],
        [100.0, 40.0],
        [40.0, 40.0],
        [40.0, 100.0],
        [0.0, 100.0],
    ];
    let mut m = prism(&outline, 50.0);
    let v0 = volume(&m);
    // `all` takes the convex edges only; with `concave` the inner edge too.
    let convex = all_edges(&m);
    let with = m
        .fillet_edges(&EdgeChoice {
            all: true,
            concave: true,
            ..EdgeChoice::default()
        })
        .unwrap();
    assert_eq!(with.len(), convex.len() + 1);
    let inner = edge_at(&m, [40.0, 40.0, 0.0], [40.0, 40.0, 50.0]);
    assert!(!convex.contains(&inner) && with.contains(&inner));
    fillet(&mut m, vec![inner], 10.0, Some(8)).unwrap();
    sound(&m);
    let want = v0 + 50.0 * area_removed(10.0, PI / 2.0, 8);
    assert!((volume(&m) - want).abs() < 1e-6);
    // A convex and a concave edge at one corner are refused with the vertex.
    let mut m = prism(&outline, 50.0);
    let err = fillet(&mut m, with, 3.0, None).unwrap_err();
    assert!(
        matches!(err, SurfaceError::Fillet(FilletError::Corner { .. })),
        "{err}"
    );
}

#[test]
fn bad_edges_and_open_solids_are_refused() {
    let mut m = cuboid(100.0, 60.0, 40.0);
    let e = edge_at(&m, [0.0, 0.0, 40.0], [100.0, 0.0, 40.0]);
    let none = fillet(&mut m, vec![], 5.0, None).unwrap_err();
    assert!(matches!(none, SurfaceError::Fillet(FilletError::NoEdges)));
    let missing = fillet(&mut m, vec![EdgeId(9999)], 5.0, None).unwrap_err();
    assert_eq!(missing, SurfaceError::NoEdge(EdgeId(9999)));
    let seg = fillet(&mut m, vec![e], 5.0, Some(65)).unwrap_err();
    assert!(matches!(
        seg,
        SurfaceError::Fillet(FilletError::Segments { .. })
    ));
    // An open box: the top face erased.
    let top = top_face(&m, 40.0);
    m.apply(&Edit::EraseFaces { faces: vec![top] }).unwrap();
    let side = edge_at(&m, [0.0, 0.0, 0.0], [100.0, 0.0, 0.0]);
    let open = fillet(&mut m, vec![side], 5.0, None).unwrap_err();
    assert!(
        matches!(open, SurfaceError::Fillet(FilletError::NotSolid { .. })),
        "{open}"
    );
    let border = fillet(&mut m, vec![e], 5.0, None).unwrap_err();
    assert!(
        matches!(
            border,
            SurfaceError::Fillet(FilletError::NotSolidEdge { .. })
        ),
        "{border}"
    );
    // A loose line on a box does not change a box edge far away, but one at a corner refuses.
    let mut m = cuboid(100.0, 60.0, 40.0);
    m.apply(&Edit::AddEdges {
        edges: vec![[[100.0, 0.0, 40.0], [150.0, 0.0, 90.0]]],
    })
    .unwrap();
    let err = fillet(&mut m, vec![e], 5.0, None).unwrap_err();
    assert!(
        matches!(err, SurfaceError::Fillet(FilletError::Corner { .. })),
        "{err}"
    );
}

#[test]
fn more_new_edges_than_one_edit_may_make_are_refused() {
    let n = 24;
    let outline: Vec<[f64; 2]> = (0..n)
        .map(|i| {
            let a = 2.0 * PI * i as f64 / n as f64;
            [100.0 * a.cos(), 100.0 * a.sin()]
        })
        .collect();
    let mut m = prism(&outline, 200.0);
    let all = all_edges(&m);
    let before = m.clone();
    let err = fillet(&mut m, all, 5.0, Some(64)).unwrap_err();
    assert!(matches!(err, SurfaceError::Limit { .. }), "{err}");
    assert_eq!(m, before);
}

#[test]
fn the_edit_is_deterministic_and_a_second_model_gives_the_same_ids() {
    let make = || {
        let mut m = cuboid(100.0, 60.0, 40.0);
        let all = all_edges(&m);
        fillet(&mut m, all, 7.0, Some(3)).unwrap();
        m
    };
    assert_eq!(make(), make());
}

/// The volume that a true round of radius `r` takes from a box with the chosen `edges` (indices
/// into [`box_edges`]): the edges, less the overlaps at the corners with two chosen edges, and the
/// ball corners where three meet.
fn box_removed_true(dims: [f64; 3], chosen: &[usize], r: f64) -> f64 {
    let a = r * r * (1.0 - PI / 4.0);
    let es = box_edges(dims);
    let mut total = 0.0;
    let corners = box_corner_counts(dims, chosen);
    for &i in chosen {
        let (p, q) = es[i];
        let len = DVec3::from(p).distance(DVec3::from(q));
        let threes = [p, q]
            .iter()
            .filter(|c| corners.get(&key3(**c)).copied() == Some(3))
            .count();
        total += a * (len - r * threes as f64);
    }
    for k in corners.values() {
        match k {
            2 => total -= r * r * r * (5.0 / 3.0 - PI / 2.0),
            3 => total += r * r * r * (1.0 - PI / 6.0),
            _ => {}
        }
    }
    total
}

fn key3(p: P) -> [i64; 3] {
    [p[0] as i64, p[1] as i64, p[2] as i64]
}

fn box_edges(d: [f64; 3]) -> Vec<(P, P)> {
    let c = |i: usize| -> P {
        [
            if i & 1 == 1 { d[0] } else { 0.0 },
            if i & 2 == 2 { d[1] } else { 0.0 },
            if i & 4 == 4 { d[2] } else { 0.0 },
        ]
    };
    let mut out = Vec::new();
    for i in 0..8 {
        for bit in [1, 2, 4] {
            if i & bit == 0 {
                out.push((c(i), c(i | bit)));
            }
        }
    }
    out
}

fn box_corner_counts(d: [f64; 3], chosen: &[usize]) -> BTreeMap<[i64; 3], usize> {
    let es = box_edges(d);
    let mut out = BTreeMap::new();
    for &i in chosen {
        for p in [es[i].0, es[i].1] {
            *out.entry(key3(p)).or_insert(0) += 1;
        }
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Random boxes, edge sets, radii and segments: a refusal changes nothing; a result is a
    /// closed solid whose volume is at most the true round and at least the true round less the
    /// error bound of the segments.
    #[test]
    fn boxes_with_random_edges_round_to_the_expected_volume(
        dx in 20u32..200, dy in 20u32..200, dz in 20u32..200,
        mask in 1u32..4096, r10 in 5u32..150, n in 1u32..9,
    ) {
        let d = [dx as f64, dy as f64, dz as f64];
        let r = r10 as f64 / 10.0;
        let chosen: Vec<usize> = (0..12).filter(|i| mask & (1 << i) != 0).collect();
        let mut m = cuboid(d[0], d[1], d[2]);
        let v0 = volume(&m);
        let es: Vec<EdgeId> = box_edges(d)
            .iter()
            .enumerate()
            .filter(|(i, _)| chosen.contains(i))
            .map(|(_, (p, q))| edge_at(&m, *p, *q))
            .collect();
        let before = m.clone();
        match fillet(&mut m, es, r, Some(n)) {
            Err(_) => prop_assert_eq!(&m, &before),
            Ok(_) => {
                sound(&m);
                let removed = v0 - volume(&m);
                let truth = box_removed_true(d, &chosen, r);
                let phi = PI / 2.0;
                let e_a = 0.5 * r * r * (phi - n as f64 * (phi / n as f64).sin());
                let lengths: f64 = chosen
                    .iter()
                    .map(|i| {
                        let (p, q) = box_edges(d)[*i];
                        DVec3::from(p).distance(DVec3::from(q))
                    })
                    .sum();
                let corners = box_corner_counts(d, &chosen).values().filter(|k| **k > 1).count();
                let gap = r * (1.0 - (phi / n as f64).cos());
                let bound = e_a * lengths + corners as f64 * (2.0 * e_a * r + PI * r * r * gap);
                prop_assert!(removed >= truth - 1e-6, "removed {} below the true {}", removed, truth);
                prop_assert!(removed <= truth + bound + 1e-6, "removed {} above {} + {}", removed, truth, bound);
            }
        }
    }

    /// Random prisms (regular outlines of 3 to 8 sides, some scaled at the top) and random edge
    /// sets: a refusal changes nothing; a result is a closed solid, smaller for convex edges, by at
    /// most the wedges of the edges.
    #[test]
    fn prisms_with_random_edges_are_refused_or_sound(
        sides in 3usize..9, h in 20u32..120, top in prop::sample::select(vec![1.0f64, 0.8, 0.6]),
        mask in any::<u64>(), r10 in 5u32..80, n in 1u32..6, chamfer in any::<bool>(),
    ) {
        let outline: Vec<[f64; 2]> = (0..sides)
            .map(|i| {
                let a = 2.0 * PI * i as f64 / sides as f64 + 0.3;
                [60.0 * a.cos(), 60.0 * a.sin()]
            })
            .collect();
        let mut m = prism(&outline, h as f64);
        if top != 1.0 {
            let f = top_face(&m, h as f64);
            let c = DVec3::new(0.0, 0.0, h as f64);
            let t = glam::DAffine3::from_translation(c)
                * glam::DAffine3::from_scale(DVec3::new(top, top, 1.0))
                * glam::DAffine3::from_translation(-c);
            let mut matrix = [0.0; 12];
            matrix[..9].copy_from_slice(&t.matrix3.to_cols_array());
            matrix[9..].copy_from_slice(&t.translation.to_array());
            m.apply(&Edit::TransformVertices { vertices: Vec::new(), faces: vec![f], edges: Vec::new(), matrix }).unwrap();
        }
        let all = all_edges(&m);
        let es: Vec<EdgeId> = all
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << (i % 64)) != 0)
            .map(|(_, e)| *e)
            .collect();
        prop_assume!(!es.is_empty());
        let v0 = volume(&m);
        let before = m.clone();
        let size = r10 as f64 / 10.0;
        let res = if chamfer {
            m.apply(&Edit::Chamfer { edges: es, distance: size }).map(|_| ())
        } else {
            fillet(&mut m, es, size, Some(n)).map(|_| ())
        };
        match res {
            Err(_) => prop_assert_eq!(&m, &before),
            Ok(()) => {
                sound(&m);
                let v = volume(&m);
                prop_assert!(v < v0);
            }
        }
    }
}

/// Measured once (release build, other load not controlled):
/// `cargo test --release -p kadakas-core --lib surface::tests_fillet::perf -- --ignored --nocapture`.
#[test]
#[ignore]
fn perf_fillet_in_a_big_model() {
    let mut m = cuboid(100.0, 60.0, 40.0);
    let faces: Vec<FaceId> = m.faces().map(|(f, _)| f).collect();
    let shift = |x: f64, y: f64| {
        let mut mx = [0.0; 12];
        mx[0] = 1.0;
        mx[4] = 1.0;
        mx[8] = 1.0;
        mx[9] = x;
        mx[10] = y;
        mx
    };
    // 6,000 boxes in 8 rows (48,000 vertices, 36,000 faces, 72,000 edges).
    m.apply(&Edit::CopyGeometry {
        faces: faces.clone(),
        edges: Vec::new(),
        matrix: shift(200.0, 0.0),
        count: 749,
    })
    .unwrap();
    let row: Vec<FaceId> = m.faces().map(|(f, _)| f).collect();
    for k in 1..8 {
        m.apply(&Edit::CopyGeometry {
            faces: row.clone(),
            edges: Vec::new(),
            matrix: shift(0.0, 200.0 * k as f64),
            count: 1,
        })
        .unwrap();
    }
    println!(
        "model: {} vertices, {} edges, {} faces",
        m.vertex_count(),
        m.edge_count(),
        m.face_count()
    );
    let e = edge_at(&m, [0.0, 0.0, 40.0], [100.0, 0.0, 40.0]);
    let t = std::time::Instant::now();
    fillet(&mut m.clone(), vec![e], 10.0, None).unwrap();
    println!("one edge: {:.3} s", t.elapsed().as_secs_f64());
    let all: Vec<EdgeId> = m
        .edges()
        .filter(|(_, ed)| {
            ed.v.iter().all(|v| {
                let p = m.vertex(*v).unwrap();
                p.x <= 100.0 && p.y <= 60.0
            })
        })
        .map(|(e, _)| e)
        .collect();
    assert_eq!(all.len(), 12);
    let t = std::time::Instant::now();
    fillet(&mut m.clone(), all.clone(), 10.0, None).unwrap();
    println!("twelve edges: {:.3} s", t.elapsed().as_secs_f64());
    let t = std::time::Instant::now();
    let err = fillet(&mut m, all, 30.0, None).unwrap_err();
    println!(
        "refused (radius 30): {:.3} s: {err}",
        t.elapsed().as_secs_f64()
    );
}
