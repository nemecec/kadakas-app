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

fn pull(m: &mut SurfaceModel, face: FaceId, d: f64) -> Result<(), SurfaceError> {
    m.apply(&Edit::PushPull { face, distance: d })
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
