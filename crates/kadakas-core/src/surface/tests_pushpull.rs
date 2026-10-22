//! Tests of Push/Pull and of curves drawn with `AddCurve` (MOD-08).

use glam::DVec3;

use super::geom::{self, Plane};
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
fn erasing_edges_of_a_curve_cuts_it_into_chains() {
    let circle = || {
        let mut m = SurfaceModel::new();
        m.apply(&Edit::AddCurve {
            points: circle_points(12, 500.0),
            closed: true,
        })
        .unwrap();
        let ring = m.curves().next().unwrap().1.clone();
        (m, ring)
    };
    let lens = |m: &SurfaceModel| {
        let mut l: Vec<usize> = m.curves().map(|(_, es)| es.len()).collect();
        l.sort_unstable();
        l
    };
    // One edge of a circle: the rest is one arc.
    let (mut m, ring) = circle();
    m.apply(&Edit::EraseEdges {
        edges: vec![ring[5]],
    })
    .unwrap();
    assert_eq!(m.problem(), None);
    assert_eq!(check::curves(&m), None);
    assert_eq!(lens(&m), vec![11]);
    // Two edges that do not meet: two arcs, each a chain.
    let (mut m, ring) = circle();
    m.apply(&Edit::EraseEdges {
        edges: vec![ring[2], ring[7]],
    })
    .unwrap();
    assert_eq!(m.problem(), None);
    assert_eq!(lens(&m), vec![4, 6]);
    // The same through two erases, and the same model twice.
    let (mut a, ring) = circle();
    for e in [ring[2], ring[7]] {
        a.apply(&Edit::EraseEdges { edges: vec![e] }).unwrap();
        assert_eq!(a.problem(), None);
    }
    assert_eq!(lens(&a), vec![4, 6]);
    // An open arc that loses a middle edge.
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddCurve {
        points: circle_points(12, 500.0)[..7].to_vec(),
        closed: false,
    })
    .unwrap();
    let arc = m.curves().next().unwrap().1.clone();
    m.apply(&Edit::EraseEdges {
        edges: vec![arc[2]],
    })
    .unwrap();
    assert_eq!(m.problem(), None);
    assert_eq!(lens(&m), vec![2, 3]);
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

/// `count` copies of the box of `box_model`, in a grid with gaps, built from parts (fast).
fn many_boxes(count: usize) -> SurfaceModel {
    let one = box_model();
    let stride = one.next_id();
    let side = (count as f64).sqrt().ceil() as usize;
    let (mut vs, mut es, mut fs) = (
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::new(),
        std::collections::BTreeMap::new(),
    );
    for k in 0..count {
        let off = u32::try_from(k).unwrap() * stride;
        let shift = DVec3::new((k % side) as f64 * 2000.0, (k / side) as f64 * 2000.0, 0.0);
        for (id, p) in one.vertices() {
            vs.insert(VertexId(id.0 + off), p + shift);
        }
        for (id, e) in one.edges() {
            es.insert(
                EdgeId(id.0 + off),
                Edge {
                    v: e.v.map(|v| VertexId(v.0 + off)),
                },
            );
        }
        for (id, f) in one.faces() {
            let mv = |l: &Vec<VertexId>| l.iter().map(|v| VertexId(v.0 + off)).collect();
            let mut g = Face::new(
                Plane {
                    normal: f.plane.normal,
                    offset: f.plane.offset + f.plane.normal.dot(shift),
                },
                mv(&f.outer),
                f.holes.iter().map(mv).collect(),
            );
            g.attrs = f.attrs.clone();
            fs.insert(FaceId(id.0 + off), g);
        }
    }
    let next = u32::try_from(count).unwrap() * stride;
    SurfaceModel::from_parts(vs, es, fs, next)
}

/// Timing of one push on a model with about 50 000 edges, the best of five (run with
/// `--ignored --nocapture`, in release).
#[test]
#[ignore = "timing"]
fn push_pull_timing_on_a_large_model() {
    let m0 = many_boxes(4200);
    println!("edges {}, faces {}", m0.edge_count(), m0.face_count());
    assert!(m0.check().is_valid());
    let top = m0
        .faces()
        .find(|(_, f)| f.plane.normal.z > 0.9)
        .map(|(id, _)| id)
        .unwrap();
    let (mut push, mut check) = (f64::INFINITY, f64::INFINITY);
    for _ in 0..5 {
        let mut m = m0.clone();
        let t = std::time::Instant::now();
        m.apply(&Edit::PushPull {
            face: top,
            distance: 250.0,
        })
        .unwrap();
        push = push.min(t.elapsed().as_secs_f64());
        let t = std::time::Instant::now();
        assert!(m.check().is_valid());
        check = check.min(t.elapsed().as_secs_f64());
    }
    println!(
        "stretch pull: {:.0} ms, one check: {:.0} ms",
        push * 1e3,
        check * 1e3
    );
}

fn vid(m: &SurfaceModel, p: P) -> VertexId {
    let p = DVec3::from(p);
    m.vertices()
        .find(|(_, q)| q.distance(p) < 1e-6)
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("no vertex at {p}"))
}

fn eid(m: &SurfaceModel, a: P, b: P) -> EdgeId {
    m.edge_between(vid(m, a), vid(m, b)).unwrap()
}

fn volume(m: &SurfaceModel) -> f64 {
    m.faces()
        .map(|(id, f)| {
            let p0 = m.vertex(f.outer[0]).unwrap();
            m.face_area(id).unwrap() * f.plane.normal.dot(p0) / 3.0
        })
        .sum()
}

fn refused_with(m: &mut SurfaceModel, face: FaceId, d: f64, words: &str) -> String {
    let before = m.clone();
    let e = m
        .apply(&Edit::PushPull { face, distance: d })
        .expect_err("the push must be refused");
    assert_eq!(*m, before, "a refused push changes nothing");
    let why = e.to_string();
    assert!(why.contains(words), "{why}");
    why
}

/// A plate (a lone square face) at height `z`, from -500 to 1500 in x and y: wider than the box.
fn plate(m: &mut SurfaceModel, z: f64) {
    poly(m, &square(-500.0, -500.0, 2000.0, z));
}

#[test]
fn stretched_walls_that_would_cut_a_plate_are_refused() {
    // All edges of the top stretch: no new edge is made, but the walls would pass through the
    // plate above the box.
    let mut m = box_model();
    let top = top_face(&m);
    plate(&mut m, 800.0);
    assert!(m.check().is_valid());
    refused_with(&mut m, top, 500.0, "through");
    // Up to the plate: the cap would lie in it.
    refused_with(&mut m, top, 300.0, "through");
    // Below the plate it is fine.
    pull(&mut m, top, 200.0).unwrap();
}

#[test]
fn push_pull_refuses_new_crossings_but_not_old_ones() {
    // Box A with a plate through it (four `NotIntersected` warnings, accepted in a sketch), and
    // box B with a plate above it.
    let mut m = box_model();
    plate(&mut m, 300.0);
    poly(&mut m, &square(3000.0, 0.0, 1000.0, 0.0));
    let b_base = m
        .faces()
        .find(|(_, f)| m.vertex(f.outer[0]).unwrap().x >= 3000.0)
        .unwrap()
        .0;
    pull(&mut m, b_base, 500.0).unwrap();
    poly(&mut m, &square(2500.0, -500.0, 2000.0, 800.0));
    let r = m.check();
    assert!(r.is_valid(), "{:?}", r.problems);
    assert_eq!(r.not_intersected(), 4);
    let top_of = |m: &SurfaceModel, x: f64| {
        m.faces()
            .find(|(id, f)| {
                f.plane.normal.z > 0.99
                    && m.face_bounds(*id).unwrap().0.x == x
                    && m.face_area(*id).unwrap() == 1e6
            })
            .unwrap()
            .0
    };
    let (a, b) = (top_of(&m, 0.0), top_of(&m, 3000.0));
    // New crossings with the plate above B: through it, and the cap in it.
    refused_with(&mut m, b, 500.0, "through");
    refused_with(&mut m, b, 300.0, "ends on");
    // The old crossings of A do not stop a push that makes no new one.
    pull(&mut m, a, 100.0).unwrap();
    pull(&mut m, b, 100.0).unwrap();
    assert_eq!(m.check().not_intersected(), 4);
    // The cap of A landing in the plate is new.
    refused_with(&mut m, a, -300.0, "cut through other faces");
    // Below the plate the old crossings go.
    pull(&mut m, a, -400.0).unwrap();
    assert_eq!(m.check().not_intersected(), 0);
}

#[test]
fn the_check_finds_an_edge_through_a_face_and_overlapping_coplanar_faces() {
    // A plate across the middle of the box: the corner edges pass through it.
    let mut m = box_model();
    plate(&mut m, 300.0);
    let r = m.check();
    let through = r
        .warnings
        .iter()
        .filter(|p| matches!(p, Problem::NotIntersected(..)))
        .count();
    assert_eq!(through, 4, "{:?}", r.warnings);
    assert!(r.is_valid() && !r.is_closed(), "a warning, not a problem");
    // A push that does not change the contacts is still allowed on such a model.
    let top = top_face(&m);
    let r2 = m.apply(&Edit::PushPull {
        face: top,
        distance: 100.0,
    });
    assert!(r2.is_ok(), "{r2:?}");

    // Two coplanar faces, one inside the other, with their own vertices.
    let mut vs = std::collections::BTreeMap::new();
    let mut es = std::collections::BTreeMap::new();
    let mut fs = std::collections::BTreeMap::new();
    let mut id = 1;
    for (x0, w) in [(0.0, 1000.0), (250.0, 500.0)] {
        let ids: Vec<VertexId> = square(x0, x0, w, 0.0)
            .iter()
            .map(|p| {
                vs.insert(VertexId(id), DVec3::from(*p));
                id += 1;
                VertexId(id - 1)
            })
            .collect();
        for i in 0..4 {
            es.insert(
                EdgeId(id),
                Edge {
                    v: [ids[i], ids[(i + 1) % 4]],
                },
            );
            id += 1;
        }
        fs.insert(
            FaceId(id),
            Face::new(
                Plane {
                    normal: DVec3::Z,
                    offset: 0.0,
                },
                ids,
                Vec::new(),
            ),
        );
        id += 1;
    }
    let m = SurfaceModel::from_parts(vs, es, fs, id);
    assert!(m.problem().is_none());
    let r = m.check();
    assert!(
        r.problems
            .contains(&Problem::FacesOverlap(FaceId(9), FaceId(18))),
        "{:?}",
        r.problems
    );
    // The edges of the small face lie in the big one.
    assert!(
        r.problems
            .contains(&Problem::EdgeInFace(EdgeId(14), FaceId(9)))
    );
}

#[test]
fn a_model_with_a_structural_problem_is_still_checked_where_the_push_moves() {
    // A line from the place where a corner of the top lands after the pull, away from the box.
    let mut m = box_model();
    let top = top_face(&m);
    m.apply(&Edit::AddEdges {
        edges: vec![[[0.0, 0.0, 800.0], [-500.0, -500.0, 1200.0]]],
    })
    .unwrap();
    // A loose vertex far away: a structural problem, so the distance checks of the whole model
    // do not run.
    let loose = VertexId(m.next_id);
    m.vertices.insert(loose, DVec3::new(9000.0, 9000.0, 0.0));
    m.next_id += 1;
    assert!(m.problem().is_some());
    refused_with(&mut m, top, 300.0, "");
    // A pull that keeps clear of the line is fine; the old problem stays and does not count.
    m.apply(&Edit::PushPull {
        face: top,
        distance: 200.0,
    })
    .unwrap();
}

#[test]
fn a_push_that_fixes_one_problem_and_makes_another_is_refused_and_names_the_new_one() {
    let mut m = box_model();
    let top = top_face(&m);
    // Two lines put in by hand (not welded): one ends on the front edge of the top (a problem the
    // pull takes away), one ends where that edge will be after the pull (a new problem).
    let line = |m: &mut SurfaceModel, a: P, b: P| -> EdgeId {
        let (va, vb, e) = (
            VertexId(m.next_id),
            VertexId(m.next_id + 1),
            EdgeId(m.next_id + 2),
        );
        m.next_id += 3;
        m.vertices.insert(va, DVec3::from(a));
        m.vertices.insert(vb, DVec3::from(b));
        m.edges.insert(e, Edge { v: [va, vb] });
        e
    };
    let _old = line(&mut m, [500.0, 0.0, 500.0], [500.0, -500.0, 500.0]);
    let new = line(&mut m, [300.0, 0.0, 800.0], [300.0, -500.0, 800.0]);
    // The touch is a vertex on an edge and a crossing; after the pull the new line has the same
    // two, so the number of problems would not change.
    let before = m.check().problems.len();
    assert_eq!(before, 2, "{:?}", m.check().problems);
    let why = refused_with(&mut m, top, 300.0, "");
    assert!(why.contains(&format!("edge {new} ")), "{why}");
}

#[test]
fn a_replaced_stretch_edge_keeps_its_attributes_and_its_place_in_a_curve() {
    let (mut m, left, _right) = split_top();
    let front = eid(&m, [0.0, 0.0, 500.0], [500.0, 0.0, 500.0]);
    let side = eid(&m, [0.0, 0.0, 500.0], [0.0, 1000.0, 500.0]);
    m.edge_attrs.insert(
        front,
        [("layer".to_string(), serde_json::json!("fence"))].into(),
    );
    let c = m.add_curve(vec![side, front]).unwrap();
    pull(&mut m, left, -200.0).unwrap();
    // The front edge of the half had a vertex that stays (on the middle line): it is replaced.
    assert!(m.edge(front).is_none());
    let now = eid(&m, [0.0, 0.0, 300.0], [500.0, 0.0, 300.0]);
    assert_eq!(
        m.edge_attrs(now).and_then(|a| a.get("layer")),
        Some(&serde_json::json!("fence"))
    );
    let es = m
        .curves()
        .find(|(id, _)| *id == c)
        .map(|(_, es)| es.clone());
    assert_eq!(es, Some(vec![side, now]));
}

/// The box with the top moved by `dx` in x: the walls at x = 0 and x = 1000 lean a little.
fn leaning_box(dx: f64) -> SurfaceModel {
    let b = box_model();
    let vs: std::collections::BTreeMap<VertexId, DVec3> = b
        .vertices()
        .map(|(id, p)| (id, if p.z > 1.0 { p + DVec3::X * dx } else { p }))
        .collect();
    let fs = b
        .faces()
        .map(|(id, f)| {
            let pts: Vec<DVec3> = f.outer.iter().map(|v| vs[v]).collect();
            let mut g = f.clone();
            g.plane = Plane::new(pts[0], geom::newell(&pts)).unwrap();
            (id, g)
        })
        .collect();
    let es = b.edges().collect();
    SurfaceModel::from_parts(vs, es, fs, b.next_id())
}

#[test]
fn a_nearly_upright_wall_stretches_for_a_short_push_and_gets_a_new_wall_for_a_long_one() {
    // The lean is 5e-7 (below the old angle bound of 1e-6).
    let m0 = leaning_box(2.5e-4);
    assert!(m0.check().is_closed(), "{:?}", m0.check());
    let top = top_face(&m0);
    let mut m = m0.clone();
    pull(&mut m, top, 100.0).unwrap();
    assert_eq!(m.face_count(), 6, "a short pull stretches all walls");
    // 100 m: the corners would leave the leaning walls by 0.05 mm, more than the plane tolerance.
    let mut m = m0.clone();
    pull(&mut m, top, 1.0e5).unwrap();
    assert_eq!(
        m.face_count(),
        8,
        "the two leaning walls get a new wall above them"
    );
    assert!(m.check().is_closed());
}

#[test]
fn a_top_split_corner_to_corner_pushed_down_merges_the_vertical_with_the_corner_edge() {
    let mut m = box_model();
    m.apply(&Edit::AddEdges {
        edges: vec![[[0.0, 0.0, 500.0], [1000.0, 1000.0, 500.0]]],
    })
    .unwrap();
    // The half with the corner (1000, 0).
    let half = m
        .faces()
        .find(|(_, f)| {
            f.plane.normal.z > 0.9
                && f.outer.iter().any(|v| {
                    m.vertex(*v)
                        .unwrap()
                        .distance(DVec3::new(1000.0, 0.0, 500.0))
                        < 1e-6
                })
        })
        .unwrap()
        .0;
    let corner = eid(&m, [0.0, 0.0, 0.0], [0.0, 0.0, 500.0]);
    pull(&mut m, half, -200.0).unwrap();
    let r = m.check();
    assert!(r.is_closed(), "{r:?}");
    // The corner edges at (0, 0) and (1000, 1000) are split at the height of the new floor; the
    // lower part keeps the id.
    assert_eq!(m.edge(corner).unwrap().v[0], vid(&m, [0.0, 0.0, 0.0]));
    assert_eq!(m.edge(corner).unwrap().v[1], vid(&m, [0.0, 0.0, 300.0]));
    eid(&m, [0.0, 0.0, 300.0], [0.0, 0.0, 500.0]);
    eid(&m, [1000.0, 1000.0, 300.0], [1000.0, 1000.0, 500.0]);
    // 7 faces and one new wall on the diagonal.
    assert_eq!(m.face_count(), 8);
    assert!((volume(&m) - 4.0e8).abs() < 1.0, "{}", volume(&m));
    // The step closes up: the front wall no longer reaches the old top corner.
    let front = m.faces().find(|(_, f)| f.plane.normal.y < -0.9).unwrap().1;
    assert_eq!(front.outer.len(), 4, "{front:?}");
}

#[test]
fn a_fin_edge_pushed_into_its_wall_is_refused_and_pulled_away_works() {
    // An apron around the top: each edge of the top has the top, a side wall and the apron.
    let mut m = box_model();
    let top = top_face(&m);
    poly(&mut m, &square(-500.0, -500.0, 2000.0, 500.0));
    let e = eid(&m, [0.0, 0.0, 500.0], [1000.0, 0.0, 500.0]);
    assert_eq!(m.faces_of_edge(e).len(), 3);
    assert!(m.check().is_valid());
    refused_with(&mut m, top, -200.0, "would lie on face");
    pull(&mut m, top, 200.0).unwrap();
}

/// A slab `side` x `side` x 500 with `k` squares (side 500, in a grid) on its top, each pulled up
/// by `h` (a bump) or pushed down (a pocket, `h < 0`). Built from parts.
fn slab_with(k: usize, side: f64, h: f64) -> SurfaceModel {
    let mut vs = std::collections::BTreeMap::new();
    let mut fs = std::collections::BTreeMap::new();
    let mut next = 1u32;
    let mut vert = |vs: &mut std::collections::BTreeMap<VertexId, DVec3>, p: DVec3| {
        let id = VertexId(next);
        next += 1;
        vs.insert(id, p);
        id
    };
    let face = |vs: &std::collections::BTreeMap<VertexId, DVec3>,
                outer: Vec<VertexId>,
                holes: Vec<Vec<VertexId>>| {
        let pts: Vec<DVec3> = outer.iter().map(|v| vs[v]).collect();
        Face::new(
            Plane::new(pts[0], geom::newell(&pts)).unwrap(),
            outer,
            holes,
        )
    };
    let top_z = 500.0;
    let corners = |x0: f64, y0: f64, w: f64, z: f64| {
        [
            DVec3::new(x0, y0, z),
            DVec3::new(x0 + w, y0, z),
            DVec3::new(x0 + w, y0 + w, z),
            DVec3::new(x0, y0 + w, z),
        ]
    };
    let b: Vec<VertexId> = corners(0.0, 0.0, side, 0.0)
        .iter()
        .map(|p| vert(&mut vs, *p))
        .collect();
    let t: Vec<VertexId> = corners(0.0, 0.0, side, top_z)
        .iter()
        .map(|p| vert(&mut vs, *p))
        .collect();
    let mut faces: Vec<Face> = Vec::new();
    faces.push(face(&vs, b.iter().rev().copied().collect(), Vec::new()));
    for i in 0..4 {
        let j = (i + 1) % 4;
        faces.push(face(&vs, vec![b[i], b[j], t[j], t[i]], Vec::new()));
    }
    let per_row = (k as f64).sqrt().ceil() as usize;
    let pitch = side / (per_row as f64 + 1.0);
    let mut holes = Vec::new();
    for n in 0..k {
        let (x0, y0) = (
            pitch * ((n % per_row) as f64 + 1.0) - 250.0,
            pitch * ((n / per_row) as f64 + 1.0) - 250.0,
        );
        let c: Vec<VertexId> = corners(x0, y0, 500.0, top_z)
            .iter()
            .map(|p| vert(&mut vs, *p))
            .collect();
        let u: Vec<VertexId> = corners(x0, y0, 500.0, top_z + h)
            .iter()
            .map(|p| vert(&mut vs, *p))
            .collect();
        holes.push(c.iter().rev().copied().collect::<Vec<_>>());
        for i in 0..4 {
            let j = (i + 1) % 4;
            // Outward for a bump, into the pocket for a pocket: the same loop.
            faces.push(face(&vs, vec![c[i], c[j], u[j], u[i]], Vec::new()));
        }
        faces.push(face(&vs, u.clone(), Vec::new()));
    }
    faces.push(face(&vs, t.clone(), holes));
    let mut es = std::collections::BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    for f in &faces {
        for (a, b) in f.uses() {
            if seen.insert(key(a, b)) {
                es.insert(EdgeId(next), Edge { v: [a, b] });
                next += 1;
            }
        }
    }
    for f in faces {
        fs.insert(FaceId(next), f);
        next += 1;
    }
    SurfaceModel::from_parts(vs, es, fs, next)
}

/// The top of a slab with many pockets or bumps pulled up: the touched face has thousands of
/// holes. Release build, `--ignored --nocapture`.
#[test]
fn a_slab_with_pockets_and_bumps_from_parts_is_closed() {
    for h in [-200.0, 300.0] {
        let m = slab_with(9, 5000.0, h);
        assert!(m.check().is_closed(), "{h}: {:?}", m.check());
    }
}

#[test]
#[ignore = "timing"]
fn push_pull_timing_on_a_slab_with_many_holes() {
    for (k, h) in [(400, -200.0), (4200, 300.0)] {
        let m0 = slab_with(k, 100_000.0, h);
        assert!(m0.check().is_closed(), "{:?}", m0.check().problems.first());
        let top = m0
            .faces()
            .find(|(_, f)| f.holes.len() == k)
            .map(|(id, _)| id)
            .unwrap();
        let mut m = m0.clone();
        let t = std::time::Instant::now();
        m.apply(&Edit::PushPull {
            face: top,
            distance: 100.0,
        })
        .unwrap();
        let push = t.elapsed();
        let t = std::time::Instant::now();
        assert!(m.check().is_closed());
        let check = t.elapsed();
        println!(
            "{k} holes ({} edges): pull {:.0} ms, check {:.0} ms",
            m.edge_count(),
            push.as_secs_f64() * 1e3,
            check.as_secs_f64() * 1e3
        );
        assert!(push.as_secs_f64() < 5.0 && check.as_secs_f64() < 5.0);
    }
}

/// Coplanar rectangles at z = 0, each with its own vertices and edges, as faces of one model.
fn rects(rs: &[(f64, f64, f64, f64)]) -> SurfaceModel {
    let mut vs = std::collections::BTreeMap::new();
    let mut es = std::collections::BTreeMap::new();
    let mut fs = std::collections::BTreeMap::new();
    let mut id = 1;
    for &(x0, y0, x1, y1) in rs {
        let ids: Vec<VertexId> = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
            .iter()
            .map(|p| {
                vs.insert(VertexId(id), DVec3::new(p[0], p[1], 0.0));
                id += 1;
                VertexId(id - 1)
            })
            .collect();
        for i in 0..4 {
            es.insert(
                EdgeId(id),
                Edge {
                    v: [ids[i], ids[(i + 1) % 4]],
                },
            );
            id += 1;
        }
        fs.insert(
            FaceId(id),
            Face::new(
                Plane {
                    normal: DVec3::Z,
                    offset: 0.0,
                },
                ids,
                Vec::new(),
            ),
        );
        id += 1;
    }
    SurfaceModel::from_parts(vs, es, fs, id)
}

#[test]
fn the_check_finds_a_partial_overlap_of_coplanar_faces_and_not_a_touch() {
    for x0 in [600.0, 500.0] {
        let m = rects(&[(0.0, 0.0, 1000.0, 1000.0), (x0, 0.0, x0 + 1000.0, 1000.0)]);
        let r = m.check();
        assert!(
            r.problems
                .contains(&Problem::FacesOverlap(FaceId(9), FaceId(18))),
            "{x0}: {:?}",
            r.problems
        );
        assert!(r.warnings.is_empty());
        // The second one turned over: the two lie back to back, a warning and no error.
        let mut m = m;
        m.apply(&Edit::ReverseFaces {
            faces: vec![FaceId(18)],
        })
        .unwrap();
        let r = m.check();
        assert!(
            !r.problems
                .iter()
                .any(|p| matches!(p, Problem::FacesOverlap(..) | Problem::EdgeInFace(..))),
            "{x0}: {:?}",
            r.problems
        );
        assert_eq!(
            r.warnings,
            vec![Problem::BackToBack(FaceId(9), FaceId(18))],
            "{x0}"
        );
    }
    // With holes: see `flush::a_hole_under_the_sample_point_does_not_hide_an_overlap`.
    // Next to each other, sharing a side (with own vertices: those are close vertices, but no
    // overlap), and apart.
    let r = rects(&[(0.0, 0.0, 1000.0, 1000.0), (1000.0, 0.0, 2000.0, 1000.0)]).check();
    assert!(
        !r.problems
            .iter()
            .any(|p| matches!(p, Problem::FacesOverlap(..) | Problem::EdgeInFace(..))),
        "{:?}",
        r.problems
    );
    assert!(
        rects(&[(0.0, 0.0, 1000.0, 1000.0), (1500.0, 0.0, 2000.0, 1000.0)])
            .check()
            .is_valid()
    );
}

#[test]
fn touching_faces_and_solids_do_not_block_a_push() {
    // A lone plate on the level of the top, sharing the edge at x = 1000.
    let mut m = box_model();
    let top = top_face(&m);
    poly(
        &mut m,
        &[
            [1000.0, 0.0, 500.0],
            [2000.0, 0.0, 500.0],
            [2000.0, 1000.0, 500.0],
            [1000.0, 1000.0, 500.0],
        ],
    );
    assert!(m.check().is_valid());
    pull(&mut m, top, 100.0).unwrap();
    // A second box that shares one corner vertex.
    let mut m = box_model();
    let top = top_face(&m);
    poly(&mut m, &square(1000.0, 1000.0, 1000.0, 0.0));
    let other = m
        .faces()
        .find(|(_, f)| f.outer.iter().any(|v| m.vertex(*v).unwrap().x > 1500.0))
        .unwrap()
        .0;
    pull(&mut m, other, -500.0).unwrap();
    assert!(m.check().is_closed());
    pull(&mut m, top, 100.0).unwrap();
    assert!(m.check().is_closed(), "{:?}", m.check());
}

fn counts(m: &SurfaceModel) -> (usize, usize, usize) {
    (m.vertex_count(), m.edge_count(), m.face_count())
}

#[test]
fn a_bump_pushed_back_down_is_flush_and_merges() {
    let plain = box_model();
    let (mut m, top, inner) = box_with_inner_square();
    m.faces
        .get_mut(&top)
        .unwrap()
        .attrs
        .insert("material".into(), serde_json::json!("stone"));
    let walls: Vec<(FaceId, Attrs)> = m
        .faces()
        .filter(|(_, f)| f.plane.normal.z.abs() < 0.5)
        .map(|(id, f)| (id, f.attrs.clone()))
        .collect();
    pull(&mut m, inner, 300.0).unwrap();
    let rev = m.rev();
    pull(&mut m, inner, -300.0).unwrap();
    assert_eq!(m.rev(), rev + 1, "one undo step");
    assert!(m.check().is_closed(), "{:?}", m.check());
    assert_eq!(counts(&m), (8, 12, 6));
    let t = m.face(top).expect("the top stays");
    assert!(t.holes.is_empty());
    assert_eq!(t.attrs["material"], "stone", "the top keeps its attributes");
    assert!(m.face(inner).is_none(), "the bump top merged into the top");
    // The box is the box again: the same vertex and edge ids, the old walls with their
    // attributes, and no curve.
    let vs = |m: &SurfaceModel| m.vertices().collect::<Vec<_>>();
    assert_eq!(vs(&m), vs(&plain));
    let es = |m: &SurfaceModel| m.edges().map(|(id, _)| id).collect::<Vec<_>>();
    assert_eq!(es(&m), es(&plain));
    for (id, attrs) in walls {
        assert_eq!(m.face(id).unwrap().attrs, attrs);
    }
    assert_eq!(m.curves().count(), 0);
}

#[test]
fn a_pocket_floor_pulled_back_up_restores_the_single_top() {
    let (mut m, top, inner) = box_with_inner_square();
    let plain = box_model();
    pull(&mut m, inner, -200.0).unwrap();
    pull(&mut m, inner, 200.0).unwrap();
    assert!(m.check().is_closed(), "{:?}", m.check());
    assert_eq!(counts(&m), counts(&plain));
    assert_eq!(
        m.face(top).unwrap().outer,
        plain.face(top_face(&plain)).unwrap().outer
    );
    assert!(m.face(top).unwrap().holes.is_empty());
}

#[test]
fn the_second_half_of_a_split_top_pushed_to_the_level_of_the_first_merges() {
    let plain = box_model();
    let (mut m, left, right) = split_top();
    let (keep, gone) = (left.min(right), left.max(right));
    for (f, name) in [(keep, "stone"), (gone, "brick")] {
        m.faces
            .get_mut(&f)
            .unwrap()
            .attrs
            .insert("material".into(), serde_json::json!(name));
    }
    pull(&mut m, left, -200.0).unwrap();
    let notes = m
        .apply_with_notes(&Edit::PushPull {
            face: right,
            distance: -200.0,
        })
        .unwrap();
    assert!(m.check().is_closed(), "{:?}", m.check());
    assert_eq!(counts(&m), (8, 12, 6));
    assert_eq!(m.local_bounds().unwrap().1.z, 300.0);
    assert!((volume(&m) - 3.0e8).abs() < 1.0, "{}", volume(&m));
    // The half with the lower id stays with its own material; the other one is named.
    assert!(m.face(gone).is_none());
    assert_eq!(m.face(keep).unwrap().attrs["material"], "stone");
    // A wall that the push made carries the paint of the face, so it may also be named when it
    // is lost in the flush; the dropped material is named once.
    let dropped: Vec<_> = notes
        .iter()
        .filter(|n| n.contains("brick") && n.contains("dropped"))
        .collect();
    assert_eq!(dropped.len(), 1, "{notes:?}");
    // The vertices that moved down keep their ids; the ones that met them were welded away.
    let ids = |m: &SurfaceModel| m.vertices().map(|(id, _)| id).collect::<Vec<_>>();
    assert_eq!(ids(&m), ids(&plain));
    assert_eq!(m.curves().count(), 0);
}

#[test]
fn the_top_pushed_past_a_line_across_a_wall_goes_in_steps() {
    let mut m = box_model();
    let top = top_face(&m);
    m.apply(&Edit::AddEdges {
        edges: vec![[[0.0, 0.0, 250.0], [1000.0, 0.0, 250.0]]],
    })
    .unwrap();
    assert_eq!(m.face_count(), 7);
    let rev = m.rev();
    pull(&mut m, top, -300.0).unwrap();
    assert_eq!(m.rev(), rev + 1, "one undo step");
    assert!(m.check().is_closed(), "{:?}", m.check());
    assert_eq!(counts(&m), (8, 12, 6), "the line went with the upper part");
    assert_eq!(m.local_bounds().unwrap().1.z, 200.0);
    assert!((volume(&m) - 2.0e8).abs() < 1.0);
    // Exactly onto the line works too.
    let mut m = box_model();
    let top = top_face(&m);
    m.apply(&Edit::AddEdges {
        edges: vec![[[0.0, 0.0, 250.0], [1000.0, 0.0, 250.0]]],
    })
    .unwrap();
    pull(&mut m, top, -250.0).unwrap();
    assert_eq!(counts(&m), (8, 12, 6));
}

#[test]
fn a_slab_with_a_bump_pulled_up_around_it() {
    let make = || {
        let (mut m, top, inner) = box_with_inner_square();
        pull(&mut m, inner, 300.0).unwrap();
        (m, top)
    };
    let (mut m, top) = make();
    pull(&mut m, top, 100.0).unwrap();
    assert!(m.check().is_closed());
    let walls: Vec<f64> = m
        .faces()
        .filter(|(_, f)| f.plane.normal.z.abs() < 1e-9)
        .filter(|(_, f)| {
            f.outer.iter().all(|v| {
                let p = m.vertex(*v).unwrap();
                p.x > 200.0 && p.x < 800.0 && p.y > 200.0 && p.y < 800.0
            })
        })
        .flat_map(|(_, f)| f.outer.iter().map(|v| m.vertex(*v).unwrap().z))
        .collect();
    assert!(
        walls.iter().all(|z| *z == 600.0 || *z == 800.0),
        "{walls:?}"
    );
    // Up to the top of the bump: the two tops are flush and merge (as a bump pushed back down).
    let (mut m, top) = make();
    pull(&mut m, top, 300.0).unwrap();
    assert!(m.check().is_closed());
    assert_eq!(counts(&m), (8, 12, 6));
    // Past it: refused, nothing changes, and the same edit gives the same ids.
    let (mut m, top) = make();
    let m2 = m.clone();
    refused_with(&mut m, top, 400.0, "flush");
    assert_eq!(m, m2);
    let (mut a, mut b) = (make().0, make().0);
    pull(&mut a, top, 100.0).unwrap();
    pull(&mut b, top, 100.0).unwrap();
    assert_eq!(a, b);
}

#[test]
fn a_contact_that_was_there_before_counts_as_old_on_a_split_off_part() {
    // The corners of the box pass through a plate at z = 400 (a model with that problem). The
    // half of a top split corner to corner pushed down splits two corner edges; the upper parts
    // (new ids) still pass through the plate at the same points.
    let mut m = box_model();
    m.apply(&Edit::AddEdges {
        edges: vec![[[0.0, 0.0, 500.0], [1000.0, 1000.0, 500.0]]],
    })
    .unwrap();
    plate(&mut m, 400.0);
    let half = m
        .faces()
        .find(|(_, f)| {
            f.plane.normal.z > 0.9
                && f.plane.offset == 500.0
                && f.outer.iter().any(|v| {
                    m.vertex(*v)
                        .unwrap()
                        .distance(DVec3::new(1000.0, 0.0, 500.0))
                        < 1e-6
                })
        })
        .unwrap()
        .0;
    let r = m.apply(&Edit::PushPull {
        face: half,
        distance: -200.0,
    });
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn an_old_problem_at_a_new_place_counts_as_new() {
    // A line put in by hand along the corner at (0, 0), from 400 to 900: the top corner lies on
    // it. Pulling the top moves the corner along the line: still on it, but at a new place.
    let mut m = box_model();
    let top = top_face(&m);
    let (va, vb, e) = (
        VertexId(m.next_id),
        VertexId(m.next_id + 1),
        EdgeId(m.next_id + 2),
    );
    m.next_id += 3;
    m.vertices.insert(va, DVec3::new(0.0, 0.0, 400.0));
    m.vertices.insert(vb, DVec3::new(0.0, 0.0, 900.0));
    m.edges.insert(e, Edge { v: [va, vb] });
    assert!(!m.check().is_valid());
    // The line crosses the plane of the top at the corner, now at 600 instead of 500.
    let why = refused_with(&mut m, top, 100.0, "");
    assert!(why.contains(&format!("edge {e} ")), "{why}");
}

#[test]
fn a_cylinder_on_a_slanted_face_pulls_with_smooth_walls() {
    // A box tilted by 30 degrees about x.
    let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let (u, v) = (DVec3::X, DVec3::new(0.0, c, s));
    let at = |x: f64, y: f64, h: f64| -> P {
        let p = u * x + v * y + u.cross(v) * h;
        [p.x, p.y, p.z]
    };
    let mut m = SurfaceModel::new();
    poly(
        &mut m,
        &[
            at(0.0, 0.0, 0.0),
            at(2000.0, 0.0, 0.0),
            at(2000.0, 2000.0, 0.0),
            at(0.0, 2000.0, 0.0),
        ],
    );
    let f = only_face(&m);
    pull(&mut m, f, 500.0).unwrap();
    assert!(m.check().is_closed());
    let circle: Vec<P> = (0..96)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 96.0;
            at(1000.0 + 400.0 * a.cos(), 1000.0 + 400.0 * a.sin(), 500.0)
        })
        .collect();
    m.apply(&Edit::AddCurve {
        points: circle,
        closed: true,
    })
    .unwrap();
    let disc = m.faces().find(|(_, f)| f.outer.len() == 96).unwrap().0;
    let before = m.face_count();
    pull(&mut m, disc, 300.0).unwrap();
    assert!(m.check().is_closed(), "{:?}", m.check());
    assert_eq!(m.face_count(), before + 96);
    let smooth = m
        .edges()
        .filter(|(id, _)| {
            m.edge_attrs(*id)
                .and_then(|a| a.get("smooth"))
                .is_some_and(|v| v == &serde_json::Value::Bool(true))
        })
        .count();
    assert_eq!(smooth, 96);
    assert_eq!(
        m.curves().count(),
        2,
        "one curve at the bottom, one at the top"
    );
    assert!(m.curves().all(|(_, es)| es.len() == 96));
}

mod flush;
