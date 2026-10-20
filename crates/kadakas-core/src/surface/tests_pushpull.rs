//! Tests of Push/Pull and of curves drawn with `AddCurve` (MOD-08).

use glam::DVec3;

use super::*;

type P = [f64; 3];

fn square(x0: f64, y0: f64, w: f64, z: f64) -> Vec<P> {
    vec![
        [x0, y0, z],
        [x0 + w, y0, z],
        [x0 + w, y0 + w, z],
        [x0, y0 + w, z],
    ]
}

fn poly(m: &mut SurfaceModel, pts: &[P]) {
    let edges = (0..pts.len())
        .map(|i| [pts[i], pts[(i + 1) % pts.len()]])
        .collect();
    m.apply(&Edit::AddEdges { edges }).unwrap();
}

/// A push/pull that must leave a valid model whenever it works.
fn pull(m: &mut SurfaceModel, face: FaceId, d: f64) -> Result<(), SurfaceError> {
    let r = m.apply(&Edit::PushPull { face, distance: d });
    if r.is_ok() {
        let rep = m.check();
        assert!(rep.is_valid(), "{:?}", rep.problems);
    }
    r
}

fn only_face(m: &SurfaceModel) -> FaceId {
    m.faces().next().unwrap().0
}

fn top_face(m: &SurfaceModel) -> FaceId {
    m.faces()
        .filter(|(_, f)| f.plane.normal.z > 0.9)
        .max_by(|a, b| a.1.plane.offset.total_cmp(&b.1.plane.offset))
        .map(|(id, _)| id)
        .unwrap()
}

fn box_model() -> SurfaceModel {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    let f = only_face(&m);
    pull(&mut m, f, 500.0).unwrap();
    m
}

fn assert_closed_box(m: &SurfaceModel) {
    let r = m.check();
    assert!(r.is_closed(), "{r:?}");
    assert_eq!(
        (m.vertex_count(), m.edge_count(), m.face_count()),
        (8, 12, 6)
    );
}

#[test]
fn pulling_a_lone_square_makes_a_closed_box() {
    let m = box_model();
    assert_closed_box(&m);
    let (lo, hi) = m.local_bounds().unwrap();
    assert_eq!((lo, hi), (DVec3::ZERO, DVec3::new(1000.0, 1000.0, 500.0)));
    assert!((m.area() - 4.0e6).abs() < 1e-3);
    // Outside faces face out: the top looks up, the bottom looks down.
    let ups = m.faces().filter(|(_, f)| f.plane.normal.z > 0.9).count();
    let downs = m.faces().filter(|(_, f)| f.plane.normal.z < -0.9).count();
    assert_eq!((ups, downs), (1, 1));
}

#[test]
fn the_cap_keeps_the_id_of_the_face() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    let f = only_face(&m);
    pull(&mut m, f, 300.0).unwrap();
    assert!((m.face(f).unwrap().plane.offset - 300.0).abs() < 1e-9);
    assert!(m.face(f).unwrap().plane.normal.z > 0.9);
}

#[test]
fn pushing_a_lone_square_to_the_back_also_closes_a_box() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    let f = only_face(&m);
    pull(&mut m, f, -400.0).unwrap();
    assert_closed_box(&m);
    let (lo, hi) = m.local_bounds().unwrap();
    assert_eq!((lo.z, hi.z), (-400.0, 0.0));
    // The cap faces down, the old face still faces up.
    assert!(m.face(f).unwrap().plane.normal.z < -0.9);
}

#[test]
fn pulling_the_top_of_a_box_again_moves_it() {
    let mut m = box_model();
    let top = top_face(&m);
    let before = (m.vertex_count(), m.edge_count(), m.face_count());
    pull(&mut m, top, 250.0).unwrap();
    assert_eq!(
        (m.vertex_count(), m.edge_count(), m.face_count()),
        before,
        "the walls stretch, nothing new is made"
    );
    assert_closed_box(&m);
    assert_eq!(m.local_bounds().unwrap().1.z, 750.0);
    // Pushing it back down shortens the walls.
    pull(&mut m, top, -600.0).unwrap();
    assert_eq!(m.local_bounds().unwrap().1.z, 150.0);
    assert_closed_box(&m);
}

#[test]
fn pushing_through_to_the_opposite_face_is_refused_and_changes_nothing() {
    let mut m = box_model();
    let top = top_face(&m);
    let before = m.clone();
    let e = pull(&mut m, top, -500.0).unwrap_err();
    assert!(matches!(e, SurfaceError::Unsolved(_)), "{e:?}");
    assert_eq!(m, before);
    assert!(pull(&mut m, top, -900.0).is_err());
    assert_eq!(m, before);
    assert!(matches!(
        pull(&mut m, top, 0.0),
        Err(SurfaceError::TooShort { .. })
    ));
    assert!(pull(&mut m, top, f64::NAN).is_err());
    assert_eq!(
        pull(&mut m, FaceId(9999), 5.0),
        Err(SurfaceError::NoFace(FaceId(9999)))
    );
}

/// The inner face of a square drawn on the top of the box, and the top (with its hole).
fn box_with_inner_square() -> (SurfaceModel, FaceId, FaceId) {
    let mut m = box_model();
    let top = top_face(&m);
    poly(&mut m, &square(250.0, 250.0, 500.0, 500.0));
    assert_eq!(m.face(top).unwrap().holes.len(), 1);
    let inner = m
        .faces()
        .find(|(id, f)| *id != top && f.plane.normal.z > 0.9)
        .map(|(id, _)| id)
        .unwrap();
    (m, top, inner)
}

#[test]
fn a_square_drawn_on_the_top_pulls_out_a_bump() {
    let (mut m, top, inner) = box_with_inner_square();
    pull(&mut m, inner, 300.0).unwrap();
    let r = m.check();
    assert!(r.is_closed(), "{r:?}");
    // 6 + 4 walls of the bump; the cap is the inner face, the old position is the hole.
    assert_eq!(m.face_count(), 6 + 1 + 4);
    assert_eq!(m.local_bounds().unwrap().1.z, 800.0);
    assert_eq!(m.face(top).unwrap().holes.len(), 1, "the opening stays");
}

#[test]
fn pushing_a_square_into_the_top_digs_a_pocket_with_walls_facing_in() {
    let (mut m, _top, inner) = box_with_inner_square();
    pull(&mut m, inner, -200.0).unwrap();
    let r = m.check();
    assert!(r.is_closed(), "{r:?}");
    assert_eq!(m.face_count(), 11);
    // The floor of the pocket looks up, at 300.
    assert!((m.face(inner).unwrap().plane.offset - 300.0).abs() < 1e-9);
    assert!(m.face(inner).unwrap().plane.normal.z > 0.9);
    // Each wall of the pocket faces the middle of the pocket.
    let mid = DVec3::new(500.0, 500.0, 400.0);
    let walls: Vec<_> = m
        .faces()
        .filter(|(_, f)| {
            f.plane.normal.z.abs() < 1e-6
                && f.outer.iter().all(|v| {
                    let p = m.vertex(*v).unwrap();
                    p.x > 200.0 && p.x < 800.0 && p.y > 200.0 && p.y < 800.0
                })
        })
        .collect();
    assert_eq!(walls.len(), 4);
    for (_, f) in walls {
        assert!(f.plane.distance(mid) > 0.0, "{f:?}");
    }
}

#[test]
fn a_face_with_a_hole_gets_walls_around_the_hole_too() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    poly(&mut m, &square(250.0, 250.0, 500.0, 0.0));
    let outer = m
        .faces()
        .find(|(_, f)| !f.holes.is_empty())
        .map(|(id, _)| id)
        .unwrap();
    let inner = m
        .faces()
        .find(|(_, f)| f.holes.is_empty())
        .map(|(id, _)| id)
        .unwrap();
    // Take out the inner square: a ring.
    m.apply(&Edit::EraseFaces { faces: vec![inner] }).unwrap();
    // The ring is attached to the inner edges only through nothing: it is a lone face.
    pull(&mut m, outer, 200.0).unwrap();
    // 4 outer walls, 4 inner walls, cap, bottom. A tube.
    assert_eq!(m.face_count(), 4 + 4 + 2);
    let r = m.check();
    assert!(r.is_valid(), "{r:?}");
    assert_eq!(m.face(outer).unwrap().holes.len(), 1);
    // The walls of the hole face the middle of the hole.
    let mid = DVec3::new(500.0, 500.0, 100.0);
    let facing_in = m
        .faces()
        .filter(|(_, f)| f.plane.normal.z.abs() < 1e-6 && f.plane.distance(mid) > 0.0)
        .count();
    // The four inner walls face the middle; the four outer walls face away from it.
    assert_eq!(facing_in, 4);
}

#[test]
fn push_pull_gives_the_same_ids_every_time_and_replays() {
    let a = box_model();
    let b = box_model();
    assert_eq!(a, b);
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
    // The log holds the edit as an intent that finds the face by a point.
    let last = a.log().last().unwrap();
    assert!(matches!(
        last.intent,
        sync::Intent::PushPullAt { distance, .. } if distance == 500.0
    ));
    let mut c = SurfaceModel::new();
    poly(&mut c, &square(0.0, 0.0, 1000.0, 0.0));
    sync::replay_intent(&mut c, &last.intent).unwrap();
    assert_closed_box(&c);
}

#[test]
fn a_failed_push_pull_leaves_the_model_as_it_was() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    let f = only_face(&m);
    let before = m.clone();
    assert!(pull(&mut m, f, 1.0e30).is_err());
    assert_eq!(m, before);
    assert_eq!(m.rev(), before.rev());
}

fn circle_points(n: usize, r: f64) -> Vec<P> {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            [r * a.cos(), r * a.sin(), 0.0]
        })
        .collect()
}

#[test]
fn a_circle_is_one_curve_and_a_face() {
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddCurve {
        points: circle_points(24, 500.0),
        closed: true,
    })
    .unwrap();
    assert_eq!(
        (m.vertex_count(), m.edge_count(), m.face_count()),
        (24, 24, 1)
    );
    let curves: Vec<_> = m.curves().collect();
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].1.len(), 24);
    assert!(m.check().is_valid());
}

#[test]
fn an_open_curve_is_a_chain_and_a_cut_keeps_it_one_curve() {
    let mut m = SurfaceModel::new();
    let pts: Vec<P> = (0..=8)
        .map(|i| {
            let a = std::f64::consts::PI * f64::from(i) / 8.0;
            [500.0 * a.cos(), 500.0 * a.sin(), 0.0]
        })
        .collect();
    m.apply(&Edit::AddCurve {
        points: pts,
        closed: false,
    })
    .unwrap();
    assert_eq!(m.edge_count(), 8);
    assert_eq!(m.curves().count(), 1);
    // A chord closes it into a face.
    m.apply(&Edit::AddEdges {
        edges: vec![[[500.0, 0.0, 0.0], [-500.0, 0.0, 0.0]]],
    })
    .unwrap();
    assert_eq!(m.face_count(), 1);
    // An edge that cuts a piece of the curve splits it; the pieces stay in the curve.
    m.apply(&Edit::AddEdges {
        edges: vec![[[0.0, 0.0, 0.0], [0.0, 500.0, 0.0]]],
    })
    .unwrap();
    let (_, es) = m.curves().next().unwrap();
    assert_eq!(m.curves().count(), 1);
    assert!(es.len() >= 8);
}

#[test]
fn a_curve_across_an_edge_stays_one_curve_in_order() {
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddEdges {
        edges: vec![[[-800.0, 0.0, 0.0], [800.0, 0.0, 0.0]]],
    })
    .unwrap();
    m.apply(&Edit::AddCurve {
        points: circle_points(16, 500.0),
        closed: true,
    })
    .unwrap();
    assert_eq!(m.curves().count(), 1);
    let (_, es) = m.curves().next().unwrap();
    assert_eq!(es.len(), 16);
    assert!(m.check().is_valid(), "{:?}", m.check().problems);
}

#[test]
fn a_cylinder_keeps_its_curves_and_smooth_edges() {
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddCurve {
        points: circle_points(12, 300.0),
        closed: true,
    })
    .unwrap();
    let f = only_face(&m);
    pull(&mut m, f, 800.0).unwrap();
    let r = m.check();
    assert!(r.is_closed(), "{r:?}");
    // 12 walls, cap and bottom.
    assert_eq!(m.face_count(), 14);
    assert_eq!(
        m.curves().count(),
        2,
        "the circle at the top is a curve too"
    );
    let smooth = m
        .edges()
        .filter(|(id, _)| {
            m.edge_attrs(*id)
                .and_then(|a| a.get("smooth"))
                .is_some_and(|v| v == &serde_json::Value::Bool(true))
        })
        .count();
    assert_eq!(smooth, 12);
}

#[test]
fn a_second_circle_over_the_same_edges_makes_no_second_curve() {
    let mut m = SurfaceModel::new();
    for _ in 0..2 {
        m.apply(&Edit::AddCurve {
            points: circle_points(12, 300.0),
            closed: true,
        })
        .unwrap();
    }
    assert_eq!(m.curves().count(), 1);
    assert_eq!(m.edge_count(), 12);
}

#[test]
fn a_degenerate_curve_is_refused() {
    let mut m = SurfaceModel::new();
    assert!(
        m.apply(&Edit::AddCurve {
            points: vec![[0.0; 3]],
            closed: false
        })
        .is_err()
    );
    assert!(
        m.apply(&Edit::AddCurve {
            points: vec![[0.0; 3], [f64::NAN, 0.0, 0.0]],
            closed: false
        })
        .is_err()
    );
    assert!(m.is_empty());
}

/// The box (1000 x 1000 x 500) with its top split by a line at x = 500 into two faces. Returns the
/// left half (x < 500) and the right half.
fn split_top() -> (SurfaceModel, FaceId, FaceId) {
    let mut m = box_model();
    m.apply(&Edit::AddEdges {
        edges: vec![[[500.0, 0.0, 500.0], [500.0, 1000.0, 500.0]]],
    })
    .unwrap();
    let halves: Vec<(FaceId, f64)> = m
        .faces()
        .filter(|(_, f)| f.plane.normal.z > 0.9)
        .map(|(id, f)| {
            let x: f64 =
                f.outer.iter().map(|v| m.vertex(*v).unwrap().x).sum::<f64>() / f.outer.len() as f64;
            (id, x)
        })
        .collect();
    assert_eq!(halves.len(), 2);
    let (l, r) = if halves[0].1 < halves[1].1 {
        (halves[0].0, halves[1].0)
    } else {
        (halves[1].0, halves[0].0)
    };
    (m, l, r)
}

#[test]
fn half_of_a_split_top_pushed_down_makes_a_step_and_stretches_the_walls() {
    let (mut m, left, right) = split_top();
    assert_eq!(m.face_count(), 7);
    pull(&mut m, left, -200.0).unwrap();
    let r = m.check();
    assert!(r.is_closed(), "{r:?}");
    // One new wall (under the middle line); the side walls got a step, no wall overlaps them.
    assert_eq!(m.face_count(), 8);
    let (lo, hi) = m.local_bounds().unwrap();
    assert_eq!((lo.z, hi.z), (0.0, 500.0));
    assert!((m.face(left).unwrap().plane.offset - 300.0).abs() < 1e-9);
    assert!((m.face(right).unwrap().plane.offset - 500.0).abs() < 1e-9);
    // The volume: 1000 x 1000 x 500 less 500 x 1000 x 200 = 4e8 mm3, from the area projected.
    let mut vol = 0.0;
    for (id, f) in m.faces() {
        // Divergence theorem: sum of (area * n . p0) / 3.
        let p0 = m.vertex(f.outer[0]).unwrap();
        vol += m.face_area(id).unwrap() * f.plane.normal.dot(p0) / 3.0;
    }
    assert!((vol - 4.0e8).abs() < 1.0, "{vol}");
}

#[test]
fn half_of_a_split_top_pulled_up_stretches_the_walls_without_extra_walls() {
    let (mut m, left, _right) = split_top();
    pull(&mut m, left, 300.0).unwrap();
    let r = m.check();
    assert!(r.is_closed(), "{r:?}");
    assert_eq!(m.face_count(), 8, "one wall at the middle line only");
    // No two coplanar faces are neighbours with a line between them: every pair of faces that
    // share an edge is either at an angle, or the two halves of nothing.
    for (e, _) in m.edges() {
        let fs = m.faces_of_edge(e);
        if let [a, b] = fs.as_slice() {
            let (na, nb) = (
                m.face(*a).unwrap().plane.normal,
                m.face(*b).unwrap().plane.normal,
            );
            assert!(na.dot(nb) < 1.0 - 1e-9, "coplanar neighbours at edge {e}");
        }
    }
    assert_eq!(m.local_bounds().unwrap().1.z, 800.0);
    let mut vol = 0.0;
    for (id, f) in m.faces() {
        let p0 = m.vertex(f.outer[0]).unwrap();
        vol += m.face_area(id).unwrap() * f.plane.normal.dot(p0) / 3.0;
    }
    assert!((vol - 6.5e8).abs() < 1.0, "{vol}");
}

#[test]
fn the_outer_top_with_a_hole_is_pulled_and_pushed_with_a_pocket_in_the_middle() {
    for d in [100.0, -100.0] {
        let (mut m, top, _inner) = box_with_inner_square();
        pull(&mut m, top, d).unwrap();
        let r = m.check();
        assert!(r.is_closed(), "{d}: {r:?}");
        // 7 faces and 4 walls around the hole.
        assert_eq!(m.face_count(), 11, "{d}");
        assert_eq!(m.face(top).unwrap().holes.len(), 1);
    }
}

#[test]
fn a_pocket_deeper_than_the_solid_is_refused_with_a_reason() {
    let (m0, _top, inner) = box_with_inner_square();
    for d in [-600.0, -500.0, -9000.0] {
        let mut m = m0.clone();
        let e = m
            .apply(&Edit::PushPull {
                face: inner,
                distance: d,
            })
            .unwrap_err();
        match e {
            SurfaceError::Unsolved(why) => assert!(why.contains("through"), "{d}: {why}"),
            other => panic!("{d}: {other:?}"),
        }
        assert_eq!(m, m0, "{d}: nothing changed");
    }
    // A pocket that stays inside is fine.
    let mut m = m0.clone();
    pull(&mut m, inner, -400.0).unwrap();
    assert!(m.check().is_closed());
}

#[test]
fn a_bump_that_would_run_into_another_solid_is_refused() {
    // Two boxes: pulling the top of a bump up into the underside of a plate above it.
    let mut m = box_model();
    let mut plate = vec![];
    for (a, b) in [
        ([0.0, 0.0, 800.0], [1000.0, 0.0, 800.0]),
        ([1000.0, 0.0, 800.0], [1000.0, 1000.0, 800.0]),
        ([1000.0, 1000.0, 800.0], [0.0, 1000.0, 800.0]),
        ([0.0, 1000.0, 800.0], [0.0, 0.0, 800.0]),
    ] {
        plate.push([a, b]);
    }
    m.apply(&Edit::AddEdges { edges: plate }).unwrap();
    let top = m
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.9 && f.plane.offset == 500.0)
        .unwrap()
        .0;
    m.apply(&Edit::AddEdges {
        edges: (0..4)
            .map(|i| {
                let p = [
                    [250.0, 250.0, 500.0],
                    [750.0, 250.0, 500.0],
                    [750.0, 750.0, 500.0],
                    [250.0, 750.0, 500.0],
                ];
                [p[i], p[(i + 1) % 4]]
            })
            .collect(),
    })
    .unwrap();
    let inner = m
        .faces()
        .find(|(id, f)| *id != top && f.plane.normal.z > 0.9 && f.plane.offset == 500.0)
        .unwrap()
        .0;
    let before = m.clone();
    assert!(pull(&mut m, inner, 400.0).is_err());
    assert_eq!(m, before);
    assert!(pull(&mut m, inner, 200.0).is_ok());
}
