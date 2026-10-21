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
fn the_check_finds_an_edge_through_a_face_and_overlapping_coplanar_faces() {
    // A plate across the middle of the box: the corner edges pass through it.
    let mut m = box_model();
    plate(&mut m, 300.0);
    let r = m.check();
    let through = r
        .problems
        .iter()
        .filter(|p| matches!(p, Problem::EdgeThroughFace(..)))
        .count();
    assert_eq!(through, 4, "{:?}", r.problems);
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
    assert_eq!(
        m.check().problems,
        vec![Problem::FacesOverlap(FaceId(9), FaceId(18))]
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
