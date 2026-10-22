use super::*;
use glam::{DAffine3, DVec2, DVec3};
use kadakas_core::surface::{Edit, SurfaceModel};
use proptest::prelude::*;

fn cam() -> Camera {
    Camera::look_at(
        DVec3::new(0.0, -10000.0, 6000.0),
        DVec3::new(1000.0, 0.0, 0.0),
        DVec3::Z,
        Projection::Perspective { fov_y: 0.8 },
        DVec2::new(1000.0, 800.0),
    )
    .unwrap()
}

fn ortho() -> Camera {
    Camera::look_at(
        DVec3::new(0.0, -10000.0, 6000.0),
        DVec3::new(1000.0, 0.0, 0.0),
        DVec3::Z,
        Projection::Orthographic { height: 6000.0 },
        DVec2::new(1000.0, 800.0),
    )
    .unwrap()
}

fn px(c: &Camera, p: DVec3) -> DVec2 {
    c.project(p).unwrap()
}

fn scene(f: impl FnOnce(&mut SceneBuilder)) -> Scene {
    let mut b = SceneBuilder::new();
    f(&mut b);
    b.finish()
}

fn edge(b: &mut SceneBuilder, a: [f64; 3], c: [f64; 3], tag: u64) {
    b.add_edge(DVec3::from(a), DVec3::from(c), Tag(tag));
}

/// One edge along X, 2 m long.
fn one_edge() -> Scene {
    scene(|b| edge(b, [0.0, 0.0, 0.0], [2000.0, 0.0, 0.0], 1))
}

fn ask(i: &mut Inferencer, s: &Scene, c: &Camera, at: DVec2, r: Option<DVec3>) -> Inference {
    i.infer(s, c, at, r)
}

#[test]
fn an_endpoint_wins_within_its_radius_and_not_outside() {
    let (s, c) = (one_edge(), cam());
    let mut i = Inferencer::default();
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::ZERO) + DVec2::new(4.0, 3.0),
        None,
    );
    assert_eq!(r.kind, Kind::Endpoint);
    assert_eq!(r.point, DVec3::ZERO);
    assert_eq!(r.color, Kind::Endpoint.color());
    assert_eq!(r.label, "Endpoint");
    assert_eq!(r.source, Tag(1));
    assert!(!r.locked);
    assert!((r.screen - px(&c, DVec3::ZERO)).length() < 1e-9);
    // Too far from the end: on the edge instead.
    let r = ask(&mut i, &s, &c, px(&c, DVec3::new(300.0, 0.0, 0.0)), None);
    assert_eq!(r.kind, Kind::OnEdge);
}

#[test]
fn a_midpoint_is_found_and_both_ends_work() {
    let (s, c) = (one_edge(), cam());
    let mut i = Inferencer::default();
    let m = DVec3::new(1000.0, 0.0, 0.0);
    let r = ask(&mut i, &s, &c, px(&c, m) + DVec2::new(0.0, 5.0), None);
    assert_eq!(r.kind, Kind::Midpoint);
    assert!(r.point.distance(m) < 1e-9);
    assert_eq!(r.label, "Midpoint");
    let e = DVec3::new(2000.0, 0.0, 0.0);
    assert_eq!(ask(&mut i, &s, &c, px(&c, e), None).point, e);
}

#[test]
fn on_edge_keeps_the_point_on_the_edge_in_perspective() {
    let s = scene(|b| edge(b, [0.0, -3000.0, 0.0], [2000.0, 4000.0, 0.0], 5));
    let c = cam();
    let mut i = Inferencer::default();
    for t in [0.2, 0.37, 0.8] {
        let p = DVec3::new(0.0, -3000.0, 0.0).lerp(DVec3::new(2000.0, 4000.0, 0.0), t);
        let r = ask(&mut i, &s, &c, px(&c, p) + DVec2::new(2.0, 0.0), None);
        assert_eq!(r.kind, Kind::OnEdge, "{t}");
        // The point is on the edge and under (almost) the pointer.
        let d = DVec3::new(2000.0, 7000.0, 0.0);
        let u = (r.point - DVec3::new(0.0, -3000.0, 0.0)).cross(d).length() / d.length();
        assert!(u < 1e-6);
        assert!((r.screen - px(&c, p)).length() < 3.0);
        assert!(matches!(r.constraint, Some(Constraint::Line { .. })));
        assert_eq!(r.label, "On Edge");
    }
}

#[test]
fn on_face_with_a_hole_and_no_face_behind_the_hole() {
    let sq = |a: f64, b: f64, z: f64| {
        vec![
            DVec3::new(a, a, z),
            DVec3::new(b, a, z),
            DVec3::new(b, b, z),
            DVec3::new(a, b, z),
        ]
    };
    let s = scene(|b| {
        b.add_face(
            sq(0.0, 3000.0, 500.0),
            vec![sq(1000.0, 2000.0, 500.0)],
            Tag(9),
        );
    });
    let c = cam();
    let mut i = Inferencer::default();
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(500.0, 500.0, 500.0)),
        None,
    );
    assert_eq!(r.kind, Kind::OnFace);
    assert!((r.point.z - 500.0).abs() < 1e-6);
    assert_eq!(r.source, Tag(9));
    assert_eq!(r.label, "On Face");
    assert_eq!(r.color, Kind::OnFace.color());
    assert!(matches!(r.constraint, Some(Constraint::Plane { .. })));
    let hole = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(1500.0, 1500.0, 500.0)),
        None,
    );
    assert_eq!(hole.kind, Kind::Free);
    assert!(hole.point.z.abs() < 1e-6, "the ground, not the face");
    assert!(hole.label.is_empty());
    assert!(hole.constraint.is_none());
}

#[test]
fn centers_of_points_circles_and_regular_faces() {
    let s = scene(|b| {
        b.add_point(DVec3::new(0.0, 0.0, 300.0), PointKind::Center, Tag(7));
        b.add_point(DVec3::new(3000.0, 0.0, 0.0), PointKind::Endpoint, Tag(8));
    });
    let c = cam();
    let mut i = Inferencer::default();
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(0.0, 0.0, 300.0)) + DVec2::new(3.0, 3.0),
        None,
    );
    assert_eq!((r.kind, r.source), (Kind::Center, Tag(7)));
    assert_eq!(r.label, "Center");
    let r = ask(&mut i, &s, &c, px(&c, DVec3::new(3000.0, 0.0, 0.0)), None);
    assert_eq!(r.kind, Kind::Endpoint);
}

fn model_with_circle() -> SurfaceModel {
    let mut m = SurfaceModel::new();
    m.transform = DAffine3::from_translation(DVec3::new(0.0, 0.0, 100.0));
    let n = 16;
    let pts: Vec<[f64; 3]> = (0..n)
        .map(|k| {
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            [800.0 * a.cos(), 800.0 * a.sin(), 0.0]
        })
        .collect();
    m.apply(&Edit::AddEdges {
        edges: (0..n).map(|k| [pts[k], pts[(k + 1) % n]]).collect(),
    })
    .unwrap();
    let ids: Vec<_> = m.edges().map(|(id, _)| id).collect();
    m.add_curve(ids).unwrap();
    m
}

#[test]
fn a_surface_model_gives_its_circle_center_and_its_corners() {
    let m = model_with_circle();
    let s = scene(|b| {
        b.add_surface_model(&m, 2);
    });
    let c = cam();
    let mut i = Inferencer::default();
    let ctr = DVec3::new(0.0, 0.0, 100.0);
    let r = ask(&mut i, &s, &c, px(&c, ctr), None);
    assert_eq!(r.kind, Kind::Center);
    assert!(r.point.distance(ctr) < 1e-6);
    // The disc is a regular polygon too: its center belongs to the curve or to the face.
    assert!(matches!(r.source.kind(), TagKind::Curve | TagKind::Face));
    let corner = DVec3::new(800.0, 0.0, 100.0);
    let r = ask(&mut i, &s, &c, px(&c, corner), None);
    assert_eq!(r.kind, Kind::Endpoint);
    assert_eq!(r.source.model(), 2);
    assert_eq!(r.source.kind(), TagKind::Edge);
    // Over the disc away from the center and the edges: on the face.
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(300.0, 250.0, 100.0)),
        None,
    );
    assert_eq!(r.kind, Kind::OnFace);
    assert_eq!(r.source.kind(), TagKind::Face);
}

#[test]
fn intersection_of_two_edges_and_of_an_edge_with_a_face() {
    let s = scene(|b| {
        edge(b, [0.0, 0.0, 0.0], [2000.0, 0.0, 0.0], 1);
        edge(b, [1000.0, -1000.0, 0.0], [1000.0, 1500.0, 0.0], 2);
    });
    let c = cam();
    let mut i = Inferencer::default();
    let x = DVec3::new(1000.0, 0.0, 0.0);
    // The crossing is also the midpoint of edge 1: a midpoint (rank 2) beats the intersection.
    assert_eq!(ask(&mut i, &s, &c, px(&c, x), None).kind, Kind::Midpoint);
    // Move the crossing away from every midpoint.
    let s = scene(|b| {
        edge(b, [0.0, 0.0, 0.0], [3000.0, 0.0, 0.0], 1);
        edge(b, [1000.0, -1000.0, 0.0], [1000.0, 2500.0, 0.0], 2);
    });
    let r = ask(&mut i, &s, &c, px(&c, x) + DVec2::new(2.0, 2.0), None);
    assert_eq!(r.kind, Kind::Intersection);
    assert!(r.point.distance(x) < 1e-6);
    assert_eq!((r.source, r.other), (Tag(1), Tag(2)));
    assert_eq!(r.label, "Intersection");
    // An edge through a face.
    let s = scene(|b| {
        edge(b, [300.0, 300.0, -800.0], [300.0, 300.0, 1200.0], 3);
        b.add_face(
            vec![
                DVec3::new(-1000.0, -1000.0, 0.0),
                DVec3::new(2000.0, -1000.0, 0.0),
                DVec3::new(2000.0, 1500.0, 0.0),
                DVec3::new(-1000.0, 1500.0, 0.0),
            ],
            vec![],
            Tag(4),
        );
    });
    let p = DVec3::new(300.0, 300.0, 0.0);
    let r = ask(&mut i, &s, &c, px(&c, p), None);
    assert_eq!(r.kind, Kind::Intersection);
    assert!(r.point.distance(p) < 1e-6);
    assert_eq!((r.source, r.other), (Tag(3), Tag(4)));
}

#[test]
fn skew_edges_and_parallel_edges_do_not_intersect() {
    let s = scene(|b| {
        edge(b, [0.0, 0.0, 0.0], [3000.0, 0.0, 0.0], 1);
        edge(b, [1000.0, -1000.0, 400.0], [1000.0, 2500.0, 400.0], 2);
        edge(b, [0.0, 1.0, 0.0], [3000.0, 1.0, 0.0], 3);
    });
    let c = cam();
    let mut i = Inferencer::default();
    let r = ask(&mut i, &s, &c, px(&c, DVec3::new(1000.0, 0.0, 0.0)), None);
    assert_ne!(r.kind, Kind::Intersection);
}

#[test]
fn the_priority_order_is_endpoint_center_midpoint_intersection_edge_lines_face() {
    let order = [
        Kind::Endpoint,
        Kind::Center,
        Kind::Midpoint,
        Kind::Intersection,
        Kind::OnEdge,
        Kind::OnAxis(Axis::X),
        Kind::OnFace,
        Kind::Free,
    ];
    for w in order.windows(2) {
        assert!(w[0].priority() < w[1].priority(), "{:?}", w);
    }
    assert_eq!(
        Kind::Parallel.priority(),
        Kind::FromPoint(Axis::Z).priority()
    );
    assert_eq!(
        Kind::Perpendicular.priority(),
        Kind::OnAxis(Axis::Y).priority()
    );
    assert!(Kind::Endpoint.is_point() && !Kind::OnEdge.is_point());
}

#[test]
fn a_far_endpoint_beats_a_near_midpoint_and_a_near_edge() {
    // A short edge: both ends and the middle are within a few pixels of the pointer.
    let s = scene(|b| edge(b, [0.0, 0.0, 0.0], [160.0, 0.0, 0.0], 1));
    let c = cam();
    let mut i = Inferencer::default();
    let a = px(&c, DVec3::ZERO);
    let m = px(&c, DVec3::new(80.0, 0.0, 0.0));
    let cursor = a.lerp(m, 0.7);
    let r = ask(&mut i, &s, &c, cursor, None);
    assert!((cursor - a).length() <= 10.0 && (cursor - m).length() <= 9.0);
    assert_eq!(r.kind, Kind::Endpoint);
}

#[test]
fn a_midpoint_beats_an_intersection_and_an_intersection_beats_on_edge() {
    let c = cam();
    let mut i = Inferencer::default();
    // The crossing sits near the midpoint of edge 1 but is not exactly there.
    let s = scene(|b| {
        edge(b, [0.0, 0.0, 0.0], [2000.0, 0.0, 0.0], 1);
        edge(b, [1030.0, -1000.0, 0.0], [1030.0, 2500.0, 0.0], 2);
    });
    let cursor = px(&c, DVec3::new(1010.0, 0.0, 0.0));
    let r = ask(&mut i, &s, &c, cursor, None);
    assert_eq!(r.kind, Kind::Midpoint);
    // With the midpoint out of reach the crossing wins over the two edges under the pointer.
    let s = scene(|b| {
        edge(b, [0.0, 0.0, 0.0], [4000.0, 0.0, 0.0], 1);
        edge(b, [1030.0, -1000.0, 0.0], [1030.0, 2500.0, 0.0], 2);
    });
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(1030.0, 0.0, 0.0)) + DVec2::new(3.0, 0.0),
        None,
    );
    assert_eq!(r.kind, Kind::Intersection);
}

#[test]
fn on_edge_beats_on_face_and_the_face_is_the_last_resort() {
    let s = scene(|b| {
        edge(b, [0.0, 0.0, 0.0], [3000.0, 0.0, 0.0], 1);
        b.add_face(
            vec![
                DVec3::new(-500.0, -500.0, 0.0),
                DVec3::new(3500.0, -500.0, 0.0),
                DVec3::new(3500.0, 2000.0, 0.0),
                DVec3::new(-500.0, 2000.0, 0.0),
            ],
            vec![],
            Tag(2),
        );
    });
    let c = cam();
    let mut i = Inferencer::default();
    let on = DVec3::new(700.0, 0.0, 0.0);
    assert_eq!(
        ask(&mut i, &s, &c, px(&c, on) + DVec2::new(0.0, 2.0), None).kind,
        Kind::OnEdge
    );
    assert_eq!(
        ask(&mut i, &s, &c, px(&c, DVec3::new(700.0, 900.0, 0.0)), None).kind,
        Kind::OnFace
    );
}

#[test]
fn axis_inference_in_the_axis_colors_within_the_angle_tolerance() {
    let s = Scene::default();
    let c = cam();
    let mut i = Inferencer::default();
    let r0 = DVec3::new(500.0, 200.0, 100.0);
    for (axis, color, label) in [
        (Axis::X, RED, "On Red Axis"),
        (Axis::Y, GREEN, "On Green Axis"),
        (Axis::Z, BLUE, "On Blue Axis"),
    ] {
        let target = r0 + axis.dir() * 1800.0;
        // Two degrees off on the screen.
        let o2 = px(&c, r0);
        let t2 = px(&c, target);
        let v = t2 - o2;
        let rot = DVec2::new(v.x * 0.9994 - v.y * 0.0349, v.x * 0.0349 + v.y * 0.9994);
        let r = ask(&mut i, &s, &c, o2 + rot, Some(r0));
        assert_eq!(r.kind, Kind::OnAxis(axis), "{axis:?}");
        assert_eq!(r.color, color);
        assert_eq!(r.label, label);
        // The point is on the axis line through the reference.
        assert!((r.point - r0).cross(axis.dir()).length() < 1e-6);
        assert_eq!(r.lines.len(), 1);
        assert_eq!(r.lines[0].from, r0);
        assert!(!r.lines[0].dotted);
        assert!(matches!(r.constraint, Some(Constraint::Line { .. })));
    }
    // Ten degrees off is no inference.
    let o2 = px(&c, r0);
    let v = px(&c, r0 + DVec3::X * 1800.0) - o2;
    let a = 10f64.to_radians();
    let rot = DVec2::new(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos());
    let r = ask(&mut i, &s, &c, o2 + rot, Some(r0));
    assert!(!matches!(r.kind, Kind::OnAxis(_)), "{:?}", r.kind);
}

#[test]
fn the_pointer_must_leave_the_reference_before_an_axis_is_inferred() {
    let s = Scene::default();
    let c = cam();
    let mut i = Inferencer::default();
    let r0 = DVec3::ZERO;
    let r = ask(&mut i, &s, &c, px(&c, r0) + DVec2::new(2.0, 0.0), Some(r0));
    assert_eq!(r.kind, Kind::Free);
}

#[test]
fn axis_loses_to_points_and_edges_and_beats_a_face() {
    let c = cam();
    let mut i = Inferencer::default();
    let r0 = DVec3::new(0.0, 0.0, 0.0);
    // An endpoint on the X axis line: the endpoint.
    let s = scene(|b| edge(b, [2000.0, 0.0, 0.0], [2000.0, 1500.0, 700.0], 1));
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(2000.0, 0.0, 0.0)),
        Some(r0),
    );
    assert_eq!(r.kind, Kind::Endpoint);
    // A face under the pointer: the axis.
    let s = scene(|b| {
        b.add_face(
            vec![
                DVec3::new(-500.0, -500.0, 0.0),
                DVec3::new(3500.0, -500.0, 0.0),
                DVec3::new(3500.0, 2000.0, 0.0),
                DVec3::new(-500.0, 2000.0, 0.0),
            ],
            vec![],
            Tag(2),
        );
    });
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(1300.0, 0.0, 0.0)),
        Some(r0),
    );
    assert_eq!(r.kind, Kind::OnAxis(Axis::X));
}

#[test]
fn an_axis_line_that_crosses_an_edge_gives_an_intersection() {
    // The edge crosses the X axis line from the reference at x = 1700.
    let s = scene(|b| edge(b, [1700.0, -1200.0, 0.0], [1700.0, 1500.0, 0.0], 1));
    let c = cam();
    let mut i = Inferencer::default();
    let r0 = DVec3::ZERO;
    let cross = DVec3::new(1700.0, 0.0, 0.0);
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, cross) + DVec2::new(0.0, 3.0),
        Some(r0),
    );
    assert_eq!(r.kind, Kind::Intersection);
    assert!(r.point.distance(cross) < 1e-6);
    assert_eq!(r.lines[0].color, RED);
    // The same with the lock.
    assert!(i.key(Key::Right));
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, cross) + DVec2::new(0.0, 4.0),
        Some(r0),
    );
    assert!(r.locked);
    assert_eq!(r.kind, Kind::Intersection);
}

fn diagonal_edge() -> Scene {
    scene(|b| edge(b, [0.0, 0.0, 0.0], [2400.0, 1800.0, 600.0], 11))
}

#[test]
fn parallel_to_an_edge_after_hovering_it() {
    let s = diagonal_edge();
    let c = cam();
    let mut i = Inferencer::default();
    let r0 = DVec3::new(3500.0, -2500.0, 0.0);
    // Before the hover there is no parallel inference.
    let e = DVec3::new(2400.0, 1800.0, 600.0).normalize();
    let target = px(&c, r0 + e * 1200.0);
    assert!(!matches!(
        ask(&mut i, &s, &c, target, Some(r0)).kind,
        Kind::Parallel
    ));
    // Hover the edge (on edge), remember it.
    let on = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(600.0, 450.0, 150.0)),
        Some(r0),
    );
    assert_eq!(on.kind, Kind::OnEdge);
    assert_eq!(i.hovered_edge().unwrap().2, Tag(11));
    let r = ask(&mut i, &s, &c, target, Some(r0));
    assert_eq!(r.kind, Kind::Parallel);
    assert_eq!(r.color, MAGENTA);
    assert_eq!(r.label, "Parallel to Edge");
    assert!((r.point - r0).cross(e).length() < 1e-6);
    // The line from the reference and the dotted reference edge.
    assert_eq!(r.lines.len(), 2);
    assert!(r.lines[1].dotted);
    assert_eq!(r.source, Tag(11));
    // The same direction the other way round.
    let back = px(&c, r0 - e * 1200.0);
    assert_eq!(ask(&mut i, &s, &c, back, Some(r0)).kind, Kind::Parallel);
}

#[test]
fn perpendicular_to_an_edge_after_hovering_it() {
    let s = scene(|b| edge(b, [0.0, 0.0, 0.0], [3000.0, 3000.0, 0.0], 12));
    let c = cam();
    let mut i = Inferencer::default();
    let r0 = DVec3::new(2000.0, 0.0, 0.0);
    ask(&mut i, &s, &c, px(&c, DVec3::new(800.0, 800.0, 0.0)), None);
    // The perpendicular from the reference to the line of the edge goes along (-1, 1, 0).
    let d = DVec3::new(-1.0, 1.0, 0.0).normalize();
    let r = ask(&mut i, &s, &c, px(&c, r0 + d * 900.0), Some(r0));
    assert_eq!(r.kind, Kind::Perpendicular);
    assert_eq!(r.label, "Perpendicular to Edge");
    assert!((r.point - r0).cross(d).length() < 1e-6);
    assert_eq!(r.color, MAGENTA);
}

#[test]
fn an_edge_along_an_axis_gives_the_axis_and_not_a_second_parallel() {
    let s = one_edge();
    let c = cam();
    let mut i = Inferencer::default();
    ask(&mut i, &s, &c, px(&c, DVec3::new(500.0, 0.0, 0.0)), None);
    let r0 = DVec3::new(0.0, -1500.0, 0.0);
    let r = ask(&mut i, &s, &c, px(&c, r0 + DVec3::X * 1500.0), Some(r0));
    assert_eq!(r.kind, Kind::OnAxis(Axis::X));
}

#[test]
fn from_point_along_an_axis_and_the_crossing_with_an_axis_line() {
    let s = scene(|b| {
        b.add_point(DVec3::new(500.0, 500.0, 0.0), PointKind::Endpoint, Tag(1));
    });
    let c = cam();
    let mut i = Inferencer::default();
    // Rest on the point.
    let r = ask(&mut i, &s, &c, px(&c, DVec3::new(500.0, 500.0, 0.0)), None);
    assert_eq!(r.kind, Kind::Endpoint);
    i.pause();
    assert_eq!(i.anchor(), Some(DVec3::new(500.0, 500.0, 0.0)));
    // A dotted line along the red axis from the point.
    let along = DVec3::new(2300.0, 500.0, 0.0);
    let r = ask(&mut i, &s, &c, px(&c, along), None);
    // There is no reference yet: from-point lines are still offered through the anchor.
    assert_eq!(r.kind, Kind::Free);
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, along),
        Some(DVec3::new(0.0, 3000.0, 700.0)),
    );
    assert_eq!(r.kind, Kind::FromPoint(Axis::X));
    assert!(r.lines[0].dotted);
    assert_eq!(r.color, RED);
    assert_eq!(r.label, "From Point");
    assert!(
        (r.point - DVec3::new(500.0, 500.0, 0.0))
            .cross(DVec3::X)
            .length()
            < 1e-6
    );
    // The reference line along Y from (3000, 3000, 0) meets the from-point line along X at y = 500
    // only if it is not coplanar... use a reference at the same height: they meet.
    let r0 = DVec3::new(2300.0, 3000.0, 0.0);
    let meet = DVec3::new(2300.0, 500.0, 0.0);
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, meet) + DVec2::new(1.0, 1.0),
        Some(r0),
    );
    assert_eq!(r.kind, Kind::FromPoint(Axis::X));
    assert!(r.point.distance(meet) < 1e-6);
    assert_eq!(r.lines.len(), 2);
    assert!(matches!(r.constraint, Some(Constraint::Point(_))));
    // A different anchor can be set and cleared by the caller.
    i.set_anchor(None);
    assert!(i.anchor().is_none());
    i.set_anchor(Some(DVec3::NAN));
    assert!(i.anchor().is_none());
    // Pause on nothing does not set an anchor.
    i.reset();
    i.pause();
    assert!(i.anchor().is_none());
    ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(5000.0, 5000.0, 0.0)),
        None,
    );
    i.pause();
    assert!(i.anchor().is_none());
}

#[test]
fn arrow_keys_lock_the_axes_and_a_second_press_unlocks() {
    let s = Scene::default();
    let c = cam();
    let mut i = Inferencer::default();
    let r0 = DVec3::new(100.0, 100.0, 100.0);
    // No reference yet: a key does nothing.
    assert!(!i.key(Key::Right));
    ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(900.0, 900.0, 0.0)),
        Some(r0),
    );
    for (key, axis) in [
        (Key::Right, Axis::X),
        (Key::Left, Axis::Y),
        (Key::Up, Axis::Z),
    ] {
        assert!(i.key(key));
        assert_eq!(i.locked_kind(), Some(Kind::OnAxis(axis)));
        // The pointer is far from the axis; the point still lies on it.
        let r = ask(
            &mut i,
            &s,
            &c,
            px(&c, DVec3::new(1500.0, 1300.0, 100.0)),
            Some(r0),
        );
        assert!(r.locked);
        assert_eq!(r.kind, Kind::OnAxis(axis));
        assert_eq!(r.color, axis.color());
        assert!((r.point - r0).cross(axis.dir()).length() < 1e-6);
        assert!(i.key(key));
        assert_eq!(i.locked_kind(), None);
    }
    // One lock replaces the other.
    i.key(Key::Right);
    i.key(Key::Up);
    assert_eq!(i.locked_kind(), Some(Kind::OnAxis(Axis::Z)));
    // A lock follows the new reference point after a click.
    let r1 = DVec3::new(-700.0, 400.0, 0.0);
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(0.0, 0.0, 1000.0)),
        Some(r1),
    );
    assert!((r.point - r1).cross(DVec3::Z).length() < 1e-6);
    // It is not in force without a reference point.
    let r = ask(&mut i, &s, &c, px(&c, DVec3::new(0.0, 0.0, 1000.0)), None);
    assert!(!r.locked);
    i.release_locks();
    assert_eq!(i.locked_kind(), None);
    // Shift up does not release an arrow lock.
    ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(900.0, 900.0, 0.0)),
        Some(r0),
    );
    i.key(Key::Right);
    assert!(!i.key(Key::ShiftUp));
    assert_eq!(i.locked_kind(), Some(Kind::OnAxis(Axis::X)));
}

#[test]
fn the_down_arrow_cycles_parallel_perpendicular_and_none() {
    let s = scene(|b| edge(b, [0.0, 0.0, 0.0], [3000.0, 3000.0, 0.0], 12));
    let c = cam();
    let mut i = Inferencer::default();
    let r0 = DVec3::new(2000.0, 0.0, 0.0);
    // Without a hovered edge the key does nothing.
    ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(2500.0, -1000.0, 0.0)),
        Some(r0),
    );
    assert!(!i.key(Key::Down));
    ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(800.0, 800.0, 0.0)),
        Some(r0),
    );
    assert!(i.key(Key::Down));
    assert_eq!(i.locked_kind(), Some(Kind::Parallel));
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(2800.0, -1400.0, 0.0)),
        Some(r0),
    );
    assert!(r.locked && r.kind == Kind::Parallel);
    let e = DVec3::new(1.0, 1.0, 0.0).normalize();
    assert!((r.point - r0).cross(e).length() < 1e-6);
    assert_eq!(r.lines.len(), 2, "the line and the dotted edge");
    assert!(i.key(Key::Down));
    assert_eq!(i.locked_kind(), Some(Kind::Perpendicular));
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(1000.0, 500.0, 0.0)),
        Some(r0),
    );
    assert!((r.point - r0).dot(e).abs() < 1e-6);
    assert!(i.key(Key::Down));
    assert_eq!(i.locked_kind(), None);
    // From an axis lock the key goes to parallel.
    i.key(Key::Right);
    i.key(Key::Down);
    assert_eq!(i.locked_kind(), Some(Kind::Parallel));
    // When the pointer is on the perpendicular inference, the key locks that one first.
    i.release_locks();
    let d = DVec3::new(-1.0, 1.0, 0.0).normalize();
    let r = ask(&mut i, &s, &c, px(&c, r0 + d * 900.0), Some(r0));
    assert_eq!(r.kind, Kind::Perpendicular);
    assert!(i.key(Key::Down));
    assert_eq!(i.locked_kind(), Some(Kind::Perpendicular));
}

#[test]
fn the_down_arrow_without_a_perpendicular_goes_back_to_none() {
    // The reference lies on the line of the edge: no perpendicular exists.
    let s = scene(|b| edge(b, [0.0, 0.0, 0.0], [3000.0, 3000.0, 0.0], 12));
    let c = cam();
    let mut i = Inferencer::default();
    let r0 = DVec3::new(1000.0, 1000.0, 0.0);
    ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(2000.0, 2000.0, 0.0)),
        Some(r0),
    );
    assert!(i.key(Key::Down));
    assert_eq!(i.locked_kind(), Some(Kind::Parallel));
    assert!(i.key(Key::Down));
    assert_eq!(i.locked_kind(), None);
}

#[test]
fn shift_locks_the_current_inference_until_it_is_released() {
    let s = one_edge();
    let c = cam();
    let mut i = Inferencer::default();
    let r0 = DVec3::new(300.0, -2000.0, 0.0);
    // An axis inference.
    let r = ask(&mut i, &s, &c, px(&c, r0 + DVec3::Y * 1500.0), Some(r0));
    assert_eq!(r.kind, Kind::OnAxis(Axis::Y));
    assert!(i.key(Key::ShiftDown));
    assert!(!i.key(Key::ShiftDown), "a repeat of the key does nothing");
    let far = px(&c, DVec3::new(2500.0, 2000.0, 0.0));
    let r = ask(&mut i, &s, &c, far, Some(r0));
    assert!(r.locked);
    assert_eq!(r.kind, Kind::OnAxis(Axis::Y));
    assert!((r.point - r0).cross(DVec3::Y).length() < 1e-6);
    assert!(i.key(Key::ShiftUp));
    assert_eq!(i.locked_kind(), None);
    assert!(!i.key(Key::ShiftUp));
    let r = ask(&mut i, &s, &c, far, Some(r0));
    assert!(!r.locked);
}

#[test]
fn shift_on_an_edge_a_point_a_face_and_nothing() {
    let c = cam();
    let mut i = Inferencer::default();
    // On edge: the line of the edge.
    let s = one_edge();
    let r = ask(&mut i, &s, &c, px(&c, DVec3::new(400.0, 0.0, 0.0)), None);
    assert_eq!(r.kind, Kind::OnEdge);
    assert!(i.key(Key::ShiftDown));
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(5000.0, 0.0, 0.0)) + DVec2::new(0.0, 60.0),
        None,
    );
    assert!(r.locked && r.kind == Kind::OnEdge);
    assert!(r.point.y.abs() < 1e-6 && r.point.z.abs() < 1e-6);
    assert_eq!(r.source, Tag(1));
    i.key(Key::ShiftUp);
    // Endpoint: the point stays.
    let r = ask(&mut i, &s, &c, px(&c, DVec3::ZERO), None);
    assert_eq!(r.kind, Kind::Endpoint);
    assert!(i.key(Key::ShiftDown));
    let r = ask(&mut i, &s, &c, px(&c, DVec3::new(1500.0, 0.0, 0.0)), None);
    assert!(r.locked);
    assert_eq!((r.kind, r.point), (Kind::Endpoint, DVec3::ZERO));
    i.key(Key::ShiftUp);
    // Face: the plane of the face.
    let s = scene(|b| {
        b.add_face(
            vec![
                DVec3::new(0.0, 0.0, 400.0),
                DVec3::new(3000.0, 0.0, 400.0),
                DVec3::new(3000.0, 2000.0, 400.0),
                DVec3::new(0.0, 2000.0, 400.0),
            ],
            vec![],
            Tag(5),
        );
    });
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(600.0, 600.0, 400.0)),
        None,
    );
    assert_eq!(r.kind, Kind::OnFace);
    assert!(i.key(Key::ShiftDown));
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(8000.0, 6000.0, 400.0)),
        None,
    );
    assert!(r.locked && r.kind == Kind::OnFace);
    assert!((r.point.z - 400.0).abs() < 1e-6);
    assert_eq!(r.source, Tag(5));
    i.key(Key::ShiftUp);
    // Nothing to lock on free space.
    let r = ask(
        &mut i,
        &Scene::default(),
        &c,
        px(&c, DVec3::new(100.0, 100.0, 0.0)),
        None,
    );
    assert_eq!(r.kind, Kind::Free);
    assert!(!i.key(Key::ShiftDown));
    // Nothing before the first inference.
    i.reset();
    assert!(!i.key(Key::ShiftDown));
    assert!(i.last().is_none());
}

#[test]
fn a_plane_lock_that_the_ray_cannot_reach_keeps_the_plane_point() {
    let s = scene(|b| {
        b.add_face(
            vec![
                DVec3::new(-500.0, -500.0, 400.0),
                DVec3::new(500.0, -500.0, 400.0),
                DVec3::new(500.0, 500.0, 400.0),
                DVec3::new(-500.0, 500.0, 400.0),
            ],
            vec![],
            Tag(5),
        );
    });
    let mut i = Inferencer::default();
    let r = i.infer(
        &s,
        &cam(),
        px(&cam(), DVec3::new(100.0, 150.0, 400.0)),
        None,
    );
    assert_eq!(r.kind, Kind::OnFace);
    assert!(i.key(Key::ShiftDown));
    // Now the camera looks along the plane: the pointer ray never meets it.
    let flat = Camera::look_at(
        DVec3::new(0.0, -5000.0, 400.0),
        DVec3::new(0.0, 0.0, 400.0),
        DVec3::Z,
        Projection::Perspective { fov_y: 0.8 },
        DVec2::new(800.0, 600.0),
    )
    .unwrap();
    let r2 = i.infer(&s, &flat, DVec2::new(400.0, 300.0), None);
    assert!(r2.locked);
    assert!(r2.point.distance(r.point) < 1e-6);
}

#[test]
fn the_result_does_not_depend_on_the_order_of_the_geometry() {
    let c = cam();
    let items: Vec<([f64; 3], [f64; 3])> = vec![
        ([0.0, 0.0, 0.0], [2000.0, 0.0, 0.0]),
        ([0.0, 0.0, 0.0], [0.0, 2000.0, 0.0]),
        ([0.0, 0.0, 0.0], [0.0, 0.0, 2000.0]),
        ([0.0, 0.0, 0.0], [1500.0, 1500.0, 0.0]),
    ];
    let build = |rev: bool| {
        scene(|b| {
            let mut v: Vec<_> = items.iter().enumerate().collect();
            if rev {
                v.reverse();
            }
            for (k, (a, z)) in v {
                edge(b, *a, *z, k as u64);
            }
        })
    };
    let (s1, s2) = (build(false), build(true));
    // Four edges share an endpoint: a tie that only the tie-break decides.
    let mut i1 = Inferencer::default();
    let mut i2 = Inferencer::default();
    let at = px(&c, DVec3::ZERO) + DVec2::new(2.0, 1.0);
    let (a, b) = (
        ask(&mut i1, &s1, &c, at, None),
        ask(&mut i2, &s2, &c, at, None),
    );
    assert_eq!(a.kind, Kind::Endpoint);
    assert_eq!(a, b);
    // And the same input twice gives the same answer.
    assert_eq!(ask(&mut i1, &s1, &c, at, None), a);
}

#[test]
fn an_orthographic_camera_and_a_scaled_ui_work_the_same_way() {
    let (s, c) = (one_edge(), ortho());
    let mut i = Inferencer::default();
    let at = px(&c, DVec3::ZERO) + DVec2::new(8.0, 0.0);
    assert_eq!(ask(&mut i, &s, &c, at, None).kind, Kind::Endpoint);
    // The same offset is outside the radius at a scale of 0.5 and inside at a scale of 2.
    let mut small = Inferencer::new(Options {
        ui_scale: 0.5,
        ..Options::default()
    });
    assert_ne!(ask(&mut small, &s, &c, at, None).kind, Kind::Endpoint);
    let mut big = Inferencer::new(Options {
        ui_scale: 2.0,
        ..Options::default()
    });
    assert_eq!(
        ask(&mut big, &s, &c, at + DVec2::new(8.0, 0.0), None).kind,
        Kind::Endpoint
    );
    // Axis in parallel projection.
    let r0 = DVec3::new(0.0, 500.0, 0.0);
    let r = ask(
        &mut i,
        &Scene::default(),
        &c,
        px(&c, r0 + DVec3::X * 900.0),
        Some(r0),
    );
    assert_eq!(r.kind, Kind::OnAxis(Axis::X));
}

#[test]
fn geometry_behind_the_camera_is_ignored() {
    let c = cam();
    let behind = c.eye - c.forward() * 500.0;
    let s = scene(|b| {
        b.add_edge(behind, behind + DVec3::X * 300.0, Tag(1));
        b.add_point(behind, PointKind::Center, Tag(2));
        b.add_face(
            vec![behind, behind + DVec3::X * 100.0, behind + DVec3::Z * 100.0],
            vec![],
            Tag(3),
        );
    });
    let mut i = Inferencer::default();
    let r = ask(&mut i, &s, &c, DVec2::new(500.0, 400.0), Some(behind));
    assert!(matches!(r.kind, Kind::Free | Kind::OnFace));
    // An edge that crosses the camera plane still picks on the part in front.
    let (a, b) = (
        c.eye - c.forward() * 100.0,
        c.eye + c.forward() * 4000.0 + DVec3::Z * 1000.0,
    );
    let s = scene(|sb| {
        sb.add_edge(a, b, Tag(1));
    });
    let r = ask(&mut i, &s, &c, px(&c, a.lerp(b, 0.2)), None);
    assert_eq!(r.kind, Kind::OnEdge);
}

#[test]
fn free_points_lie_on_the_ground_or_along_the_ray() {
    let c = cam();
    let mut i = Inferencer::default();
    let r = ask(
        &mut i,
        &Scene::default(),
        &c,
        DVec2::new(500.0, 500.0),
        None,
    );
    assert_eq!(r.kind, Kind::Free);
    assert!(r.point.z.abs() < 1e-6);
    // Looking up: no ground. The point is along the ray.
    let up = Camera::look_at(
        DVec3::new(0.0, 0.0, 100.0),
        DVec3::new(0.0, 100.0, 500.0),
        DVec3::Z,
        Projection::Perspective { fov_y: 0.8 },
        DVec2::new(800.0, 600.0),
    )
    .unwrap();
    let r = ask(
        &mut i,
        &Scene::default(),
        &up,
        DVec2::new(400.0, 300.0),
        None,
    );
    assert!((r.point.distance(up.eye) - 10_000.0).abs() < 1e-6);
    let mut flying = Inferencer::new(Options {
        ground_z: None,
        ..Options::default()
    });
    let r = ask(
        &mut flying,
        &Scene::default(),
        &c,
        DVec2::new(500.0, 500.0),
        None,
    );
    assert!((r.point.distance(c.eye) - 10_000.0).abs() < 1e-6);
}

#[test]
fn reset_forgets_the_locks_the_anchor_and_the_hover() {
    let s = one_edge();
    let c = cam();
    let mut i = Inferencer::default();
    ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(400.0, 0.0, 0.0)),
        Some(DVec3::ZERO),
    );
    i.key(Key::Right);
    i.pause();
    assert!(i.hovered_edge().is_some() && i.anchor().is_some() && i.locked_kind().is_some());
    i.reset();
    assert!(i.hovered_edge().is_none() && i.anchor().is_none() && i.locked_kind().is_none());
}

#[test]
fn a_cube_from_a_surface_model_snaps_like_a_cube() {
    let mut m = SurfaceModel::new();
    let corner = |x: f64, y: f64, z: f64| [x * 1000.0, y * 1000.0, z * 1000.0];
    let mut edges = Vec::new();
    for z in [0.0, 1.0] {
        for (a, b) in [
            ((0.0, 0.0), (1.0, 0.0)),
            ((1.0, 0.0), (1.0, 1.0)),
            ((1.0, 1.0), (0.0, 1.0)),
            ((0.0, 1.0), (0.0, 0.0)),
        ] {
            edges.push([corner(a.0, a.1, z), corner(b.0, b.1, z)]);
        }
    }
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        edges.push([corner(x, y, 0.0), corner(x, y, 1.0)]);
    }
    m.apply(&Edit::AddEdges { edges }).unwrap();
    assert!(m.face_count() >= 6);
    let s = scene(|b| {
        b.add_surface_model(&m, 0);
    });
    assert_eq!(s.edge_count(), 12);
    let c = cam();
    let mut i = Inferencer::default();
    let top = DVec3::new(1000.0, 1000.0, 1000.0);
    assert_eq!(ask(&mut i, &s, &c, px(&c, top), None).kind, Kind::Endpoint);
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(500.0, 500.0, 1000.0)),
        None,
    );
    // The square top face is not a circle or a polygon: no center.
    assert_eq!(r.kind, Kind::OnFace);
    let r = ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(250.0, 250.0, 1000.0)),
        None,
    );
    assert_eq!(r.kind, Kind::OnFace);
    // An edge against the top face is an intersection with the face only when it crosses it:
    // none of the cube edges does.
    assert!(r.point.is_finite());
}

#[test]
fn marks_and_labels_in_estonian() {
    kadakas_l10n::with_language("et", || {
        assert_eq!(Kind::Endpoint.label(), "Otspunkt");
        assert_eq!(Kind::OnAxis(Axis::X).label(), "Punasel teljel");
    });
    assert_eq!(Kind::Free.label(), "");
    assert_eq!(Axis::ALL.len(), 3);
    for k in [
        Kind::Endpoint,
        Kind::Center,
        Kind::Midpoint,
        Kind::Intersection,
        Kind::OnEdge,
        Kind::OnFace,
        Kind::OnAxis(Axis::Y),
        Kind::FromPoint(Axis::Z),
        Kind::Parallel,
        Kind::Perpendicular,
    ] {
        assert!(!k.label().starts_with("uic-"), "{k:?}");
    }
    const { assert!(PAUSE_SECONDS > 0.0) };
}

// ----- property tests -------------------------------------------------------------------------

fn arb_edges() -> impl Strategy<Value = Vec<([f64; 3], [f64; 3])>> {
    prop::collection::vec(
        (
            prop::array::uniform3(-4000.0f64..4000.0),
            prop::array::uniform3(-4000.0f64..4000.0),
        ),
        1..24,
    )
}

fn arb_camera() -> impl Strategy<Value = Camera> {
    (
        0.0f64..std::f64::consts::TAU,
        0.2f64..1.3,
        6000.0f64..20000.0,
        any::<bool>(),
    )
        .prop_map(|(az, el, dist, persp)| {
            let eye = DVec3::new(az.cos() * el.cos(), az.sin() * el.cos(), el.sin()) * dist;
            let p = if persp {
                Projection::Perspective { fov_y: 0.8 }
            } else {
                Projection::Orthographic { height: 9000.0 }
            };
            Camera::look_at(eye, DVec3::ZERO, DVec3::Z, p, DVec2::new(1000.0, 800.0)).unwrap()
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn an_endpoint_in_reach_always_wins(edges in arb_edges(), cam in arb_camera(), pick in 0usize..48, dx in -6.0f64..6.0, dy in -6.0f64..6.0) {
        let s = scene(|b| for (k, (a, z)) in edges.iter().enumerate() { edge(b, *a, *z, k as u64); });
        let ends: Vec<DVec3> = edges.iter().flat_map(|(a, z)| [DVec3::from(*a), DVec3::from(*z)]).collect();
        let target = ends[pick % ends.len()];
        prop_assume!(cam.project(target).is_some());
        let at = px(&cam, target) + DVec2::new(dx, dy);
        let mut i = Inferencer::default();
        let r = i.infer(&s, &cam, at, None);
        // A zero-length edge is dropped from the scene; the endpoint must belong to a kept edge.
        prop_assume!(edges.iter().any(|(a, z)| a != z && (DVec3::from(*a) == target || DVec3::from(*z) == target)));
        prop_assert_eq!(r.kind, Kind::Endpoint);
        let near = ends.iter().filter(|e| cam.project(**e).is_some_and(|p| (p - at).length() <= 10.0)).count();
        prop_assert!(near >= 1);
        prop_assert!(ends.contains(&r.point));
        prop_assert!((px(&cam, r.point) - at).length() <= 10.0 + 1e-9);
    }

    #[test]
    fn every_result_is_consistent(edges in arb_edges(), cam in arb_camera(), x in 0.0f64..1000.0, y in 0.0f64..800.0, with_ref in any::<bool>(), r0 in prop::array::uniform3(-3000.0f64..3000.0)) {
        let s = scene(|b| for (k, (a, z)) in edges.iter().enumerate() { edge(b, *a, *z, k as u64); });
        let reference = with_ref.then(|| DVec3::from(r0));
        let mut i = Inferencer::default();
        let r = i.infer(&s, &cam, DVec2::new(x, y), reference);
        prop_assert!(r.point.is_finite());
        prop_assert!(!r.locked);
        match r.kind {
            Kind::OnEdge => {
                let ok = edges.iter().any(|(a, z)| {
                    let (a, z) = (DVec3::from(*a), DVec3::from(*z));
                    a != z && (r.point - a).cross(z - a).length() <= 1e-6 * (z - a).length().max(1.0) * 1e3
                });
                prop_assert!(ok);
            }
            Kind::OnAxis(a) => {
                let r0 = reference.unwrap();
                prop_assert!((r.point - r0).cross(a.dir()).length() < 1e-5 * (1.0 + (r.point - r0).length()));
            }
            Kind::Endpoint | Kind::Midpoint | Kind::Intersection | Kind::Center => {
                prop_assert!((px(&cam, r.point) - DVec2::new(x, y)).length() <= 10.0 + 1e-6);
            }
            _ => {}
        }
        // The same question to a fresh engine gives the same answer.
        let again = Inferencer::default().infer(&s, &cam, DVec2::new(x, y), reference);
        prop_assert_eq!(again.point, r.point);
        prop_assert_eq!(again.kind, r.kind);
    }

    #[test]
    fn an_axis_lock_keeps_the_point_on_the_axis(cam in arb_camera(), x in 0.0f64..1000.0, y in 0.0f64..800.0, r0 in prop::array::uniform3(-3000.0f64..3000.0), key in 0usize..3) {
        let r0 = DVec3::from(r0);
        prop_assume!(cam.project(r0).is_some());
        let s = Scene::default();
        let mut i = Inferencer::default();
        i.infer(&s, &cam, DVec2::new(500.0, 400.0), Some(r0));
        let (k, axis) = [(Key::Right, Axis::X), (Key::Left, Axis::Y), (Key::Up, Axis::Z)][key];
        prop_assert!(i.key(k));
        let r = i.infer(&s, &cam, DVec2::new(x, y), Some(r0));
        prop_assert!(r.locked);
        prop_assert_eq!(r.kind, Kind::OnAxis(axis));
        prop_assert!((r.point - r0).cross(axis.dir()).length() < 1e-5 * (1.0 + (r.point - r0).length()));
    }

    #[test]
    fn the_order_of_the_edges_does_not_change_the_result(edges in arb_edges(), cam in arb_camera(), x in 0.0f64..1000.0, y in 0.0f64..800.0) {
        let build = |rev: bool| scene(|b| {
            let mut v: Vec<_> = edges.iter().enumerate().collect();
            if rev { v.reverse(); }
            for (k, (a, z)) in v { edge(b, *a, *z, k as u64); }
        });
        let (s1, s2) = (build(false), build(true));
        let (mut i1, mut i2) = (Inferencer::default(), Inferencer::default());
        let (a, b) = (i1.infer(&s1, &cam, DVec2::new(x, y), None), i2.infer(&s2, &cam, DVec2::new(x, y), None));
        prop_assert_eq!(a.kind, b.kind);
        prop_assert_eq!(a.point, b.point);
    }
}

// ----- performance ------------------------------------------------------------------------------

/// A surface model with a grid of `n` by `n` vertices on a ramp: about `2 n^2` edges and `n^2` faces.
pub(super) fn grid_model(n: u32) -> SurfaceModel {
    use kadakas_core::surface::{Edge, EdgeId, Face, FaceId, Plane, VertexId};
    use std::collections::BTreeMap;
    let id = |x: u32, y: u32| VertexId(y * n + x);
    let mut vertices = BTreeMap::new();
    for y in 0..n {
        for x in 0..n {
            vertices.insert(
                id(x, y),
                DVec3::new(x as f64 * 100.0, y as f64 * 100.0, x as f64 * 10.0),
            );
        }
    }
    let mut edges = BTreeMap::new();
    let mut k = 0;
    for y in 0..n {
        for x in 0..n {
            if x + 1 < n {
                edges.insert(
                    EdgeId(k),
                    Edge {
                        v: [id(x, y), id(x + 1, y)],
                    },
                );
                k += 1;
            }
            if y + 1 < n {
                edges.insert(
                    EdgeId(k),
                    Edge {
                        v: [id(x, y), id(x, y + 1)],
                    },
                );
                k += 1;
            }
        }
    }
    let mut faces = BTreeMap::new();
    let mut f = 0;
    for y in 0..n - 1 {
        for x in 0..n - 1 {
            let o = vertices[&id(x, y)];
            let plane = Plane::new(o, DVec3::new(-0.1, 0.0, 1.0)).unwrap();
            faces.insert(
                FaceId(f),
                Face::new(
                    plane,
                    vec![id(x, y), id(x + 1, y), id(x + 1, y + 1), id(x, y + 1)],
                    vec![],
                ),
            );
            f += 1;
        }
    }
    SurfaceModel::from_parts(vertices, edges, faces, n * n + 1)
}

pub(super) fn percentile(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p) as usize]
}

/// Run with `cargo test -p kadakas-ui-core --release -- --ignored --nocapture infer3d`.
#[test]
#[ignore]
fn perf_a_50k_edge_surface_model_infers_within_a_frame() {
    let m = grid_model(160);
    assert!(m.edge_count() >= 50_000, "{}", m.edge_count());
    let t = std::time::Instant::now();
    let s = scene(|b| {
        b.add_surface_model(&m, 0);
    });
    let build = t.elapsed();
    let views = [
        // The whole model on the screen: about 6 pixels per grid cell, the densest case.
        (
            "overview",
            DVec3::new(8000.0, -9000.0, 14000.0),
            DVec3::new(8000.0, 8000.0, 0.0),
        ),
        // Closer: a few cells fill the pointer area.
        (
            "close",
            DVec3::new(8000.0, 4000.0, 1500.0),
            DVec3::new(8000.0, 8000.0, 0.0),
        ),
        // Eye level inside the model: half of the boxes of the index are behind the camera.
        (
            "inside",
            DVec3::new(8000.0, 6000.0, 1000.0),
            DVec3::new(8000.0, 12000.0, 700.0),
        ),
    ];
    let mut worst99 = 0.0f64;
    for (name, eye, target) in views {
        let c = Camera::look_at(
            eye,
            target,
            DVec3::Z,
            Projection::Perspective { fov_y: 0.8 },
            DVec2::new(1600.0, 900.0),
        )
        .unwrap();
        let mut i = Inferencer::default();
        let mut times = Vec::new();
        let mut kinds = std::collections::BTreeMap::new();
        for k in 0..3000u32 {
            // A point on the model under the pointer, plus a jitter of a few pixels.
            let (gx, gy) = ((k * 7919 % 15900) as f64, (k * 104729 % 15900) as f64);
            let p = DVec3::new(gx, gy, gx * 0.1);
            let Some(base) = c.project(p) else { continue };
            let at = base + DVec2::new((k % 9) as f64 - 4.0, (k % 7) as f64 - 3.0);
            let r0 = (k % 3 != 0).then(|| DVec3::new(8000.0, 8000.0, 800.0));
            let t = std::time::Instant::now();
            let r = i.infer(&s, &c, at, r0);
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            *kinds
                .entry(
                    format!("{:?}", std::mem::discriminant(&r.kind)).len()
                        + r.kind.priority() as usize,
                )
                .or_insert(0) += 1;
            assert!(r.point.is_finite());
        }
        let (p50, p99, max) = (
            percentile(&mut times.clone(), 0.5),
            percentile(&mut times.clone(), 0.99),
            percentile(&mut times, 1.0),
        );
        worst99 = worst99.max(p99);
        eprintln!(
            "infer3d {name}: {} edges, {} faces; build {:.1} ms; infer p50 {p50:.3} ms, p99 {p99:.3} ms, max {max:.3} ms",
            s.edge_count(),
            s.face_count(),
            build.as_secs_f64() * 1000.0
        );
    }
    let p99 = worst99;
    assert!(p99 < 4.0, "p99 {p99} ms is over a quarter of a 60 Hz frame");
}

// ----- the review fixes ---------------------------------------------------------------------

/// A cube of `size` with its corner at the origin: six faces and twelve edges. The tags run
/// from `tag0` (faces) and `tag0 + 100` (edges).
fn cube(b: &mut SceneBuilder, size: f64, tag0: u64) {
    let c = |x: f64, y: f64, z: f64| DVec3::new(x, y, z) * size;
    let quad = |p: [[f64; 3]; 4]| p.map(|q| c(q[0], q[1], q[2])).to_vec();
    let faces = [
        quad([[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]]),
        quad([[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]]),
        quad([[0., 0., 0.], [0., 1., 0.], [0., 1., 1.], [0., 0., 1.]]),
        quad([[1., 0., 0.], [1., 1., 0.], [1., 1., 1.], [1., 0., 1.]]),
        quad([[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]]),
        quad([[0., 1., 0.], [1., 1., 0.], [1., 1., 1.], [0., 1., 1.]]),
    ];
    for (k, f) in faces.into_iter().enumerate() {
        for j in 0..4 {
            b.add_edge(f[j], f[(j + 1) % 4], Tag(tag0 + 100 + (k * 4 + j) as u64));
        }
        b.add_face(f, vec![], Tag(tag0 + k as u64));
    }
}

/// A 3/4 view from the front left above.
fn three_quarter() -> Camera {
    Camera::look_at(
        DVec3::new(-3000.0, -4000.0, 3000.0),
        DVec3::new(500.0, 500.0, 500.0),
        DVec3::Z,
        Projection::Perspective { fov_y: 0.8 },
        DVec2::new(800.0, 800.0),
    )
    .unwrap()
}

#[test]
fn a_hidden_back_corner_is_not_offered_through_a_front_face() {
    let s = scene(|b| cube(b, 1000.0, 1));
    let c = three_quarter();
    let hidden = DVec3::new(1000.0, 1000.0, 0.0);
    let mut i = Inferencer::default();
    let r = ask(&mut i, &s, &c, px(&c, hidden), None);
    assert_ne!(r.kind, Kind::Endpoint, "{r:?}");
    assert!(r.point.distance(hidden) > 100.0, "{r:?}");
    assert_eq!(r.kind, Kind::OnFace, "{r:?}");
    // The front corner is still found, and so is an edge on the near side.
    let front = DVec3::new(0.0, 0.0, 1000.0);
    assert_eq!(ask(&mut i, &s, &c, px(&c, front), None).point, front);
    let on = DVec3::new(0.0, 0.0, 300.0);
    assert_eq!(ask(&mut i, &s, &c, px(&c, on), None).kind, Kind::OnEdge);
    // Hidden midpoints too: the midpoint of the far bottom edge.
    let mid = DVec3::new(500.0, 1000.0, 0.0);
    let r = ask(&mut i, &s, &c, px(&c, mid), None);
    assert!(r.point.distance(mid) > 100.0, "{r:?}");
    // Across scenes: the face in one scene hides a point of another.
    let faces = scene(|b| cube(b, 1000.0, 1));
    let lone = scene(|b| {
        b.add_point(hidden, PointKind::Endpoint, Tag(900));
    });
    let r = i.infer_scenes(&[&lone, &faces], &c, px(&c, hidden), None, &|_| false);
    assert_ne!(r.source, Tag(900), "{r:?}");
    assert_eq!(r.kind, Kind::OnFace);
    // Without the face in the way the point is there.
    let r = i.infer_scenes(&[&lone], &c, px(&c, hidden), None, &|_| false);
    assert_eq!((r.kind, r.source), (Kind::Endpoint, Tag(900)));
}

#[test]
fn a_skipped_tag_neither_snaps_nor_hides() {
    let s = scene(|b| cube(b, 1000.0, 1));
    let c = three_quarter();
    let hidden = DVec3::new(1000.0, 1000.0, 0.0);
    let mut i = Inferencer::default();
    // Skip the faces: the back corner shows.
    let no_faces = |t: Tag| (1..=6).contains(&t.0);
    let r = i.infer_scenes(&[&s], &c, px(&c, hidden), None, &no_faces);
    assert_eq!(r.kind, Kind::Endpoint);
    assert_eq!(r.point, hidden);
    // Skip everything: nothing to snap to.
    let r = i.infer_scenes(&[&s], &c, px(&c, hidden), None, &|_| true);
    assert_eq!(r.kind, Kind::Free);
    // Skip the edges: the front corner is gone.
    let front = DVec3::new(0.0, 0.0, 1000.0);
    let r = i.infer_scenes(&[&s], &c, px(&c, front), None, &|_| false);
    assert_eq!(r.kind, Kind::Endpoint);
    let r = i.infer_scenes(&[&s], &c, px(&c, front), None, &|t| t.0 >= 100);
    assert_ne!(r.kind, Kind::Endpoint);
}

#[test]
fn several_scenes_give_the_result_of_one_merged_scene() {
    let a = scene(|b| edge(b, [0.0, 0.0, 0.0], [2000.0, 0.0, 0.0], 1));
    let b2 = scene(|b| edge(b, [1000.0, -1000.0, 0.0], [1000.0, 1000.0, 0.0], 2));
    let both = scene(|b| {
        edge(b, [0.0, 0.0, 0.0], [2000.0, 0.0, 0.0], 1);
        edge(b, [1000.0, -1000.0, 0.0], [1000.0, 1000.0, 0.0], 2);
    });
    let c = cam();
    let mut i = Inferencer::default();
    for at in [
        DVec3::new(1000.0, 0.0, 0.0),
        DVec3::new(300.0, 0.0, 0.0),
        DVec3::new(1000.0, 600.0, 0.0),
        DVec3::new(2000.0, 0.0, 0.0),
    ] {
        let p = px(&c, at) + DVec2::new(1.0, 2.0);
        let one = ask(&mut i, &both, &c, p, None);
        let two = i.infer_scenes(&[&a, &b2], &c, p, None, &|_| false);
        assert_eq!(one, two, "{at:?}");
        let two = i.infer_scenes(&[&b2, &a], &c, p, None, &|_| false);
        assert_eq!(one, two, "{at:?}");
    }
    // The crossing of an edge of one scene with an edge of the other: the middle of both.
    let p = px(&c, DVec3::new(1000.0, 0.0, 0.0));
    let r = i.infer_scenes(&[&a, &b2], &c, p, None, &|_| false);
    assert!(
        matches!(r.kind, Kind::Midpoint | Kind::Intersection),
        "{r:?}"
    );
}

#[test]
fn scenes_can_be_built_on_another_thread() {
    fn is_send<T: Send + Sync>() {}
    is_send::<Scene>();
    is_send::<SceneBuilder>();
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddEdges {
        edges: vec![[[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]]],
    })
    .unwrap();
    let h = std::thread::spawn(move || Scene::from_surface_model(m, 3));
    let s = h.join().unwrap();
    assert_eq!(s.edge_count(), 1);
    let c = cam();
    let mut i = Inferencer::default();
    let r = ask(&mut i, &s, &c, px(&c, DVec3::ZERO), None);
    assert_eq!((r.kind, r.source.model()), (Kind::Endpoint, 3));
    // A builder made by hand goes to a thread as well.
    let mut b = SceneBuilder::new();
    b.add_edge(DVec3::ZERO, DVec3::X, Tag(1));
    let s = std::thread::spawn(move || b.finish()).join().unwrap();
    assert_eq!(s.edge_count(), 1);
}

#[test]
fn under_a_blue_lock_a_corner_gives_its_height_on_the_line() {
    let s = scene(|b| edge(b, [0.0, 0.0, 800.0], [2000.0, 0.0, 800.0], 1));
    let c = cam();
    let mut i = Inferencer::default();
    let r0 = DVec3::new(500.0, -500.0, 0.0);
    ask(
        &mut i,
        &s,
        &c,
        px(&c, DVec3::new(500.0, -500.0, 300.0)),
        Some(r0),
    );
    assert!(i.key(Key::Up));
    // The pointer is on the corner (0, 0, 800), which is not on the blue line.
    let corner = DVec3::new(0.0, 0.0, 800.0);
    let r = ask(&mut i, &s, &c, px(&c, corner), Some(r0));
    assert!(r.locked);
    assert_eq!(r.kind, Kind::OnAxis(Axis::Z));
    let want = DVec3::new(500.0, -500.0, 800.0);
    assert!(r.point.distance(want) < 1e-6, "{:?}", r.point);
    assert_eq!(r.source, Tag(1));
    assert!(matches!(r.constraint, Some(Constraint::Line { .. })));
    // The line from the reference point and a dotted helper from the corner to the point.
    assert!(r.lines.iter().any(|l| !l.dotted && l.from == r0));
    assert!(
        r.lines
            .iter()
            .any(|l| l.dotted && l.from == corner && l.to == r.point)
    );
    // A midpoint works the same way.
    let mid = DVec3::new(1000.0, 0.0, 800.0);
    let r = ask(&mut i, &s, &c, px(&c, mid), Some(r0));
    assert!(r.point.distance(want) < 1e-6, "{:?}", r.point);
    // Away from any point the line is followed as before.
    let far = DVec3::new(500.0, -500.0, 1500.0);
    let r = ask(&mut i, &s, &c, px(&c, far), Some(r0));
    assert!((r.point - r0).cross(DVec3::Z).length() < 1e-6);
}

#[test]
fn a_box_behind_the_camera_is_not_traversed() {
    use super::engine::overlaps_rect;
    let c = Camera::look_at(
        DVec3::new(0.0, 0.0, 1500.0),
        DVec3::new(0.0, 10000.0, 1500.0),
        DVec3::Z,
        Projection::Perspective { fov_y: 0.8 },
        DVec2::new(1000.0, 800.0),
    )
    .unwrap();
    let centre = DVec2::new(500.0, 400.0);
    let bx = |y0: f64, y1: f64| Aabb {
        min: DVec3::new(-100.0, y0, 1400.0),
        max: DVec3::new(100.0, y1, 1600.0),
    };
    // Fully behind the camera: no, wherever the pointer is.
    assert!(!overlaps_rect(&c, &bx(-3000.0, -1000.0), centre, 10.0));
    let corner = DVec2::new(10.0, 10.0);
    assert!(!overlaps_rect(&c, &bx(-3000.0, -1000.0), corner, 10.0));
    // In front: yes. Across the camera: yes (the part in front can reach the pointer).
    assert!(overlaps_rect(&c, &bx(1000.0, 2000.0), centre, 10.0));
    assert!(overlaps_rect(&c, &bx(-1000.0, 2000.0), centre, 10.0));
    // A box in front but far to the side of the view is outside the pad.
    let side = Aabb {
        min: DVec3::new(50_000.0, 1000.0, 1400.0),
        max: DVec3::new(50_100.0, 2000.0, 1600.0),
    };
    assert!(!overlaps_rect(&c, &side, centre, 10.0));
}

#[test]
fn shift_on_a_point_with_a_reference_locks_the_direction_through_it() {
    let s = one_edge();
    let c = cam();
    let r0 = DVec3::new(500.0, -1500.0, 600.0);
    let end = DVec3::new(2000.0, 0.0, 0.0);
    for through in [end, DVec3::new(1000.0, 0.0, 0.0)] {
        let mut i = Inferencer::default();
        let r = ask(&mut i, &s, &c, px(&c, through), Some(r0));
        assert!(matches!(r.kind, Kind::Endpoint | Kind::Midpoint));
        assert!(i.key(Key::ShiftDown));
        assert_eq!(i.locked_kind(), Some(Kind::ThroughPoint));
        // The pointer moves away along the line and across it: the point stays on the line.
        let dir = (through - r0).normalize();
        for t in [0.5, 1.5, 2.5] {
            let on_line = r0 + dir * ((through - r0).length() * t);
            for off in [DVec2::ZERO, DVec2::new(40.0, 30.0)] {
                let r = ask(&mut i, &s, &c, px(&c, on_line) + off, Some(r0));
                assert!(r.locked && r.kind == Kind::ThroughPoint, "{r:?}");
                assert!((r.point - r0).cross(dir).length() < 1e-6, "{r:?}");
                assert_eq!(r.label, "Through Point");
            }
        }
        assert!(i.key(Key::ShiftUp));
        assert!(!ask(&mut i, &s, &c, px(&c, end), Some(r0)).locked);
    }
    // Without a reference the point itself is kept.
    let mut i = Inferencer::default();
    ask(&mut i, &s, &c, px(&c, end), None);
    assert!(i.key(Key::ShiftDown));
    assert_eq!(i.locked_kind(), Some(Kind::Endpoint));
    let r = ask(&mut i, &s, &c, px(&c, DVec3::new(500.0, 0.0, 0.0)), None);
    assert_eq!((r.kind, r.point), (Kind::Endpoint, end));
    kadakas_l10n::with_language("et", || {
        assert_eq!(Kind::ThroughPoint.label(), "Läbi punkti")
    });
}

#[test]
fn the_nearest_faces_are_kept_not_the_first_ones() {
    // Three tiny decoy faces next to the pointer (added first) and the face that the edge
    // crosses under the pointer (added last). Only two faces are kept.
    let o = Options {
        max_near_faces: 2,
        ..Options::default()
    };
    let c = cam();
    let s = scene(|b| {
        for (k, dx) in [40.0, -40.0, 30.0].into_iter().enumerate() {
            let q = |x: f64, y: f64| DVec3::new(dx + x, y, 0.0);
            b.add_face(
                vec![q(0.0, 0.0), q(8.0, 0.0), q(8.0, 8.0), q(0.0, 8.0)],
                vec![],
                Tag(10 + k as u64),
            );
        }
        // The crossed face: a square around the origin.
        b.add_face(
            vec![
                DVec3::new(-200.0, -200.0, 0.0),
                DVec3::new(200.0, -200.0, 0.0),
                DVec3::new(200.0, 200.0, 0.0),
                DVec3::new(-200.0, 200.0, 0.0),
            ],
            vec![],
            Tag(99),
        );
        edge(b, [0.0, 0.0, -300.0], [0.0, 0.0, 900.0], 5);
    });
    let mut i = Inferencer::new(o);
    let r = ask(&mut i, &s, &c, px(&c, DVec3::ZERO), None);
    assert_eq!(r.kind, Kind::Intersection, "{r:?}");
    assert_eq!((r.source, r.other), (Tag(5), Tag(99)));
}

#[test]
fn the_face_normal_comes_from_the_face_that_was_hit() {
    // Two faces with the same tag: a horizontal one and a vertical one.
    let s = scene(|b| {
        b.add_face(
            vec![
                DVec3::new(0.0, 0.0, 0.0),
                DVec3::new(1000.0, 0.0, 0.0),
                DVec3::new(1000.0, 1000.0, 0.0),
                DVec3::new(0.0, 1000.0, 0.0),
            ],
            vec![],
            Tag(5),
        );
        b.add_face(
            vec![
                DVec3::new(3000.0, 0.0, 0.0),
                DVec3::new(3000.0, 1000.0, 0.0),
                DVec3::new(3000.0, 1000.0, 1000.0),
                DVec3::new(3000.0, 0.0, 1000.0),
            ],
            vec![],
            Tag(5),
        );
    });
    let c = three_quarter();
    let mut i = Inferencer::default();
    let at = px(&c, DVec3::new(3000.0, 400.0, 400.0));
    let r = ask(&mut i, &s, &c, at, None);
    assert_eq!(r.kind, Kind::OnFace);
    let Some(Constraint::Plane { normal, .. }) = r.constraint else {
        panic!("{r:?}")
    };
    assert!(normal.dot(DVec3::X).abs() > 0.999, "{normal:?}");
}

mod instances {
    use super::*;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::component;
    use kadakas_core::model::{Attributes, ObjectId, Shape};
    use kadakas_core::resource::ResourceId;

    /// A component of one square (1 m) with two instances: at the origin and 3 m to the right.
    fn two_instances() -> (Editor, ResourceId, ObjectId, ObjectId) {
        let mut ed = Editor::with_sequential_ids();
        let m = ObjectId(
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
            id: m,
            edit: Edit::AddEdges {
                edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
            },
        })
        .unwrap();
        let face = match &ed.doc().object(m).unwrap().shape {
            Shape::SurfaceModel(s) => s.faces().next().unwrap().0,
            _ => unreachable!(),
        };
        let out = ed
            .execute(&Command::MakeComponent {
                name: "Tile".into(),
                model: Some(m),
                faces: vec![face],
                edges: vec![],
                objects: vec![],
                replace: true,
            })
            .unwrap();
        let (sym, a) = (ResourceId(out.created[0]), ObjectId(out.created[1]));
        let b = ObjectId(
            ed.execute(&Command::CopyObjects {
                ids: vec![a],
                delta: DVec2::new(3000.0, 0.0),
            })
            .unwrap()
            .created[0],
        );
        (ed, sym, a, b)
    }

    #[test]
    fn instances_share_the_definition_and_each_is_placed() {
        let (ed, sym, a, b) = two_instances();
        let r = scene_of_instances(ed.doc(), &[a, b], 10).unwrap();
        assert_eq!(r.placed.len(), 2);
        assert_eq!((r.placed[0].first_model, r.placed[1].first_model), (10, 11));
        assert_eq!(r.scene.edge_count(), 8);
        assert_eq!(r.scene.face_count(), 2);
        let def = DefinitionScene::of(&ed.doc().resources, sym).unwrap();
        assert_eq!(def.model_count(), 1);
        // The pointer on a corner of the second instance: the endpoint, and the tag names it.
        let (c, mut i) = (cam(), Inferencer::default());
        let corner = DVec3::new(4000.0, 1000.0, 0.0);
        let hit = i.infer(&r.scene, &c, px(&c, corner), None);
        assert_eq!(hit.kind, Kind::Endpoint);
        assert!(hit.point.distance(corner) < 1e-9);
        assert_eq!(r.instance_of(hit.source), Some((b, 0)));
        let first = i.infer(&r.scene, &c, px(&c, DVec3::new(0.0, 0.0, 0.0)), None);
        assert_eq!(r.instance_of(first.source), Some((a, 0)));
        assert_eq!(r.instance_of(Tag::NONE), None);
        assert_eq!(
            r.instance_of(Tag::edge(99, kadakas_core::surface::EdgeId(1))),
            None
        );
    }

    #[test]
    fn a_turned_and_mirrored_instance_is_placed_by_its_transform() {
        let (mut ed, _, a, _) = two_instances();
        ed.execute(&Command::RotateObjects {
            ids: vec![a],
            center: DVec2::ZERO,
            angle: std::f64::consts::FRAC_PI_2,
        })
        .unwrap();
        let r = scene_of_instances(ed.doc(), &[a], 0).unwrap();
        // The square is now to the left of the y axis: its corner (1000, 1000) went to (-1000, 1000).
        let (c, mut i) = (cam(), Inferencer::default());
        let corner = DVec3::new(-1000.0, 1000.0, 0.0);
        let hit = i.infer(&r.scene, &c, px(&c, corner), None);
        assert_eq!(hit.kind, Kind::Endpoint);
        assert!(hit.point.distance(corner) < 1e-6);
        // A mirror: still a scene with the same edges and face.
        let Shape::Component { symbol, transform } = ed.doc().object(a).unwrap().shape.clone()
        else {
            panic!()
        };
        let flipped = component::raw(
            &(DAffine3::from_scale(DVec3::new(-1.0, 1.0, 1.0)) * component::affine(&transform)),
        );
        ed.execute(&Command::ModifyObject {
            id: a,
            shape: Some(Shape::Component {
                symbol,
                transform: flipped,
            }),
            layer: None,
            class: None,
            attrs: None,
        })
        .unwrap();
        let r = scene_of_instances(ed.doc(), &[a], 0).unwrap();
        assert_eq!((r.scene.edge_count(), r.scene.face_count()), (4, 1));
    }

    #[test]
    fn the_definition_scene_is_built_once_and_forgotten_on_demand() {
        let (ed, sym, a, b) = two_instances();
        let mut cache = DefinitionCache::default();
        assert!(cache.is_empty());
        scene_of_instances_cached(ed.doc(), &[a, b], 0, &mut cache).unwrap();
        assert_eq!(cache.len(), 1);
        let first = cache.get(&ed.doc().resources, sym).unwrap();
        let second = cache.get(&ed.doc().resources, sym).unwrap();
        assert!(
            std::sync::Arc::ptr_eq(&first, &second),
            "the data of the definition is shared"
        );
        cache.forget(sym);
        assert!(cache.is_empty());
        cache.get(&ed.doc().resources, sym).unwrap();
        cache.clear();
        assert!(cache.is_empty());
        // The scene is empty for no instances.
        assert!(
            scene_of_instances(ed.doc(), &[], 0)
                .unwrap()
                .scene
                .is_empty()
        );
    }

    /// Adds a second square next to the one of the definition through an edit in place.
    fn widen(ed: &mut Editor, instance: ObjectId) {
        let g = ObjectId(
            ed.execute(&Command::EditComponent { id: instance })
                .unwrap()
                .created[0],
        );
        let model = ed
            .doc()
            .children(g)
            .into_iter()
            .find(|c| matches!(ed.doc().object(*c).unwrap().shape, Shape::SurfaceModel(_)))
            .unwrap();
        let p = [
            [1000.0, 0.0, 0.0],
            [2000.0, 0.0, 0.0],
            [2000.0, 1000.0, 0.0],
            [1000.0, 1000.0, 0.0],
        ];
        ed.execute(&Command::EditSurface {
            id: model,
            edit: Edit::AddEdges {
                edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
            },
        })
        .unwrap();
        ed.execute(&Command::CloseComponent {
            group: g,
            resolve: None,
        })
        .unwrap();
    }

    #[test]
    fn the_cache_follows_a_change_of_the_definition_and_its_undo() {
        let (mut ed, _, a, _) = two_instances();
        let mut cache = DefinitionCache::default();
        let faces = |ed: &Editor, cache: &mut DefinitionCache| {
            scene_of_instances_cached(ed.doc(), &[a], 0, cache)
                .unwrap()
                .scene
                .face_count()
        };
        assert_eq!(faces(&ed, &mut cache), 1);
        widen(&mut ed, a);
        // Nobody called `forget`: the changed content is what decides.
        assert_eq!(faces(&ed, &mut cache), 2, "stale after the edit");
        assert!(ed.undo());
        assert_eq!(faces(&ed, &mut cache), 1, "stale after the undo");
        assert!(ed.redo());
        assert_eq!(faces(&ed, &mut cache), 2, "stale after the redo");
    }

    #[test]
    fn a_nested_definition_that_changes_shows_in_the_outer_one() {
        let (mut ed, _, a, _) = two_instances();
        // An outer component that holds an instance of the tile.
        let out = ed
            .execute(&Command::MakeComponent {
                name: "Pair".into(),
                model: None,
                faces: vec![],
                edges: vec![],
                objects: vec![a],
                replace: true,
            })
            .unwrap();
        let outer = ObjectId(out.created[1]);
        let mut cache = DefinitionCache::default();
        let faces = |ed: &Editor, cache: &mut DefinitionCache| {
            scene_of_instances_cached(ed.doc(), &[outer], 0, cache)
                .unwrap()
                .scene
                .face_count()
        };
        assert_eq!(faces(&ed, &mut cache), 1);
        // Change the tile by editing the other instance of it in place.
        let b = ed
            .doc()
            .objects
            .values()
            .find(|o| {
                matches!(o.shape, Shape::Component { .. }) && o.id != outer && o.parent.is_none()
            })
            .map(|o| o.id)
            .unwrap();
        widen(&mut ed, b);
        assert_eq!(faces(&ed, &mut cache), 2, "stale in the outer definition");
    }

    #[test]
    fn what_is_not_an_instance_is_refused_with_a_message() {
        let (mut ed, _, a, _) = two_instances();
        let plain = ObjectId(
            ed.execute(&Command::AddObject {
                shape: Shape::Circle {
                    center: DVec2::ZERO,
                    radius: 5.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        let e = scene_of_instances(ed.doc(), &[plain], 0).unwrap_err();
        assert!(e.to_string().contains("not a component instance"));
        let gone = ObjectId::from_num(31337);
        assert!(
            scene_of_instances(ed.doc(), &[gone], 0)
                .unwrap_err()
                .to_string()
                .contains("does not exist")
        );
        // A model number too large for the tags.
        let e = scene_of_instances(ed.doc(), &[a], 0x3fff_ffff).unwrap_err();
        assert!(e.to_string().contains("too many"));
        // A definition that is gone.
        assert!(DefinitionScene::of(&ed.doc().resources, ResourceId::from_num(4040)).is_err());
    }

    #[test]
    fn tags_of_other_kinds_keep_their_number() {
        assert_eq!(Tag::NONE.shifted(5), Tag::NONE);
        let t = Tag::edge(3, kadakas_core::surface::EdgeId(9));
        assert_eq!(t.shifted(4).model(), 7);
        assert_eq!(t.shifted(4).id(), 9);
        assert_eq!(t.shifted(4).kind(), TagKind::Edge);
        // A guide has no model: it keeps its number.
        let guide = Tag(3u64 << 62 | 5);
        assert_eq!(guide.kind(), TagKind::Guide);
        assert_eq!(guide.shifted(1), guide);
        assert_eq!(Tag::NONE.kind(), TagKind::Other);
    }
}
