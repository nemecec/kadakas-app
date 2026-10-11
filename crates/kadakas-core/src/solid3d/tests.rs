use super::*;
use crate::curve::VertexKind;
use proptest::prelude::*;
use std::f64::consts::PI;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Vertex> {
    curve::corners(&[
        DVec2::new(x0, y0),
        DVec2::new(x1, y0),
        DVec2::new(x1, y1),
        DVec2::new(x0, y1),
    ])
}

fn circle(c: DVec2, r: f64) -> Vec<Vertex> {
    [DVec2::X, DVec2::Y, -DVec2::X, -DVec2::Y]
        .iter()
        .map(|d| Vertex::new(c + *d * r, VertexKind::PointOnArc))
        .collect()
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

fn all_finite(m: &SolidMesh) -> bool {
    m.vertices.iter().all(|v| v.is_finite())
        && m.triangles
            .iter()
            .all(|t| t.iter().all(|&i| (i as usize) < m.vertices.len()))
}

// ------------------------------------------------------------------ extrude

#[test]
fn a_box_is_closed_with_the_exact_volume_and_outward_faces() {
    let e = Extrude::new(rect(0.0, 0.0, 2000.0, 1000.0), 500.0);
    assert_eq!(e.problem(), None);
    let m = e.mesh().unwrap();
    assert!(m.is_closed() && all_finite(&m));
    assert!(
        close(m.signed_volume(), 1.0e9, 1e-9),
        "{}",
        m.signed_volume()
    );
    assert!(close(m.area(), 7.0e6, 1e-9));
    let (lo, hi) = e.bounds3();
    assert_eq!((lo, hi), m.bounds());
    assert_eq!((lo.z, hi.z), (0.0, 500.0));
    assert_eq!(e.profile_area(), 2.0e6);
}

#[test]
fn base_and_negative_height_move_the_box() {
    let mut e = Extrude::new(rect(0.0, 0.0, 100.0, 100.0), -300.0);
    e.base = 1000.0;
    assert_eq!(e.z_range(), (700.0, 1000.0));
    let m = e.mesh().unwrap();
    assert!(m.is_closed());
    assert!(m.signed_volume() > 0.0);
    assert_eq!(m.bounds().0.z, 700.0);
    assert_eq!(m.clone().lifted(50.0).bounds().1.z, 1050.0);
}

#[test]
fn clockwise_profiles_give_the_same_solid() {
    let mut p = rect(0.0, 0.0, 100.0, 100.0);
    p.reverse();
    let m = Extrude::new(p, 10.0).mesh().unwrap();
    assert!(m.is_closed() && m.signed_volume() > 0.0);
}

#[test]
fn a_curved_profile_has_the_exact_area_of_its_curve() {
    let e = Extrude::new(circle(DVec2::new(10.0, 20.0), 1000.0), 100.0);
    assert_eq!(e.problem(), None);
    let m = e.mesh().unwrap();
    assert!(m.is_closed());
    let exact = PI * 1000.0 * 1000.0 * 100.0;
    assert!((m.volume() - exact).abs() / exact < 0.01, "{}", m.volume());
    assert!(close(e.profile_area(), PI * 1e6, 1e-3));
}

#[test]
fn holes_are_cut_out_and_the_mesh_stays_closed() {
    let mut e = Extrude::new(rect(0.0, 0.0, 1000.0, 1000.0), 100.0);
    e.holes = vec![
        rect(200.0, 200.0, 400.0, 400.0),
        rect(600.0, 300.0, 800.0, 700.0),
    ];
    assert_eq!(e.problem(), None);
    assert_eq!(e.profile_area(), 1.0e6 - 4.0e4 - 8.0e4);
    let m = e.mesh().unwrap();
    assert!(m.is_closed() && all_finite(&m));
    assert!(close(m.signed_volume(), (1.0e6 - 1.2e5) * 100.0, 1e-9));
}

#[test]
fn a_hole_in_a_curved_profile() {
    let mut e = Extrude::new(circle(DVec2::ZERO, 1000.0), 50.0);
    e.holes = vec![circle(DVec2::new(100.0, 0.0), 300.0)];
    let m = e.mesh().unwrap();
    assert!(m.is_closed());
    let exact = PI * (1e6 - 9e4) * 50.0;
    assert!((m.volume() - exact).abs() / exact < 0.01);
}

#[test]
fn an_l_shaped_profile_with_a_hole_in_the_arm() {
    let l = curve::corners(&[
        DVec2::new(0.0, 0.0),
        DVec2::new(6000.0, 0.0),
        DVec2::new(6000.0, 2000.0),
        DVec2::new(2000.0, 2000.0),
        DVec2::new(2000.0, 6000.0),
        DVec2::new(0.0, 6000.0),
    ]);
    let mut e = Extrude::new(l, 300.0);
    e.holes = vec![rect(4000.0, 500.0, 5000.0, 1500.0)];
    assert_eq!(e.problem(), None);
    let m = e.mesh().unwrap();
    assert!(m.is_closed());
    assert!(close(m.signed_volume(), (20.0e6 - 1.0e6) * 300.0, 1e-9));
}

#[test]
fn bad_extrudes_say_what_is_wrong() {
    let ok = Extrude::new(rect(0.0, 0.0, 100.0, 100.0), 10.0);
    let problem = |f: &dyn Fn(&mut Extrude)| {
        let mut e = ok.clone();
        f(&mut e);
        assert!(e.mesh().is_none());
        e.problem().expect("a problem")
    };
    assert!(problem(&|e| e.height = 0.0).contains("zero"));
    assert!(problem(&|e| e.height = f64::NAN).contains("finite"));
    assert!(problem(&|e| e.base = f64::INFINITY).contains("finite"));
    assert!(problem(&|e| e.height = 5.0e9).contains("range"));
    assert!(problem(&|e| e.profile.truncate(2)).contains("3 vertices"));
    // Three points on a line have no area.
    assert!(
        problem(&|e| e.profile = curve::corners(&[DVec2::ZERO, DVec2::X, DVec2::X * 2.0]))
            .contains("no area")
    );
    // The same point three times.
    assert!(problem(&|e| e.profile = curve::corners(&[DVec2::ONE; 3])).contains("no area"));
    // A bow tie crosses itself.
    assert!(
        problem(&|e| {
            e.profile = curve::corners(&[
                DVec2::new(0.0, 0.0),
                DVec2::new(100.0, 100.0),
                DVec2::new(100.0, 0.0),
                DVec2::new(0.0, 100.0),
            ])
        })
        .contains("crosses itself")
    );
    // A spike that doubles back over the edge before it.
    assert!(
        problem(&|e| {
            e.profile = curve::corners(&[
                DVec2::new(0.0, 0.0),
                DVec2::new(100.0, 0.0),
                DVec2::new(100.0, 100.0),
                DVec2::new(50.0, 100.0),
                DVec2::new(80.0, 100.0),
                DVec2::new(0.0, 100.0),
            ])
        })
        .contains("itself")
    );
    assert!(problem(&|e| e.holes = vec![rect(200.0, 200.0, 300.0, 300.0)]).contains("outside"));
    assert!(problem(&|e| e.holes = vec![rect(0.0, 20.0, 50.0, 40.0)]).contains("touches"));
    assert!(
        problem(&|e| e.holes = vec![rect(20.0, 20.0, 60.0, 60.0), rect(40.0, 40.0, 80.0, 80.0)])
            .contains("touches")
    );
    assert!(
        problem(&|e| e.holes = vec![rect(10.0, 10.0, 90.0, 90.0), rect(20.0, 20.0, 30.0, 30.0)])
            .contains("inside another hole")
    );
    assert!(problem(&|e| e.holes = vec![rect(10.0, 10.0, 20.0, 10.0)]).contains("no area"));
}

#[test]
fn huge_profiles_are_refused() {
    let many: Vec<Vertex> = (0..MAX_RING_POINTS + 1)
        .map(|i| {
            let a = i as f64 * 0.0001;
            Vertex::corner(DVec2::from_angle(a) * 1000.0)
        })
        .collect();
    let e = Extrude::new(many, 10.0);
    assert!(e.problem().unwrap().contains("too many"));
    // Many holes together pass the limit of the points.
    let mut e = Extrude::new(rect(0.0, 0.0, 1.0e6, 1.0e6), 10.0);
    for k in 0..2100 {
        let x = 100.0 + 400.0 * k as f64;
        e.holes.push(rect(x, 100.0, x + 100.0, 200.0));
    }
    assert!(e.problem().unwrap().contains("too many"));
}

#[test]
fn extrude_json_round_trip_and_defaults() {
    let mut e = Extrude::new(circle(DVec2::ZERO, 50.0), 10.0);
    e.holes = vec![rect(-5.0, -5.0, 5.0, 5.0)];
    e.name = "Planter".into();
    e.base = 20.0;
    let text = serde_json::to_string(&e).unwrap();
    assert_eq!(serde_json::from_str::<Extrude>(&text).unwrap(), e);
    let min: Extrude =
        serde_json::from_str(r#"{"profile": [[0,0],[1,0],[1,1]], "height": 3}"#).unwrap();
    assert_eq!((min.base, min.holes.len(), min.name.as_str()), (0.0, 0, ""));
    assert_eq!(serde_json::to_value(&min).unwrap().get("holes"), None);
}

// ------------------------------------------------------------------ sweep

fn cylinder(r: f64, h: f64) -> Sweep {
    Sweep::new(rect(0.0, 0.0, r, h))
}

#[test]
fn a_flat_full_turn_is_a_closed_cylinder() {
    let s = cylinder(1000.0, 2000.0);
    assert_eq!(s.problem(), None);
    assert_eq!(s.segments(), 36);
    let m = s.mesh().unwrap();
    assert!(m.is_closed() && all_finite(&m));
    // A polygon of 36 sides: area is a little less than the circle.
    let exact = PI * 1e6 * 2000.0;
    assert!(m.signed_volume() > 0.0);
    assert!((m.volume() - exact).abs() / exact < 0.01, "{}", m.volume());
    let (lo, hi) = s.bounds3();
    assert!((hi.x - 1000.0).abs() < 1e-6 && (lo.x + 1000.0).abs() < 1e-6);
    assert_eq!((lo.z, hi.z), (0.0, 2000.0));
    assert_eq!((lo, hi), m.bounds());
}

#[test]
fn a_profile_on_the_negative_side_gives_a_closed_mesh_too() {
    let s = Sweep::new(rect(-1000.0, 0.0, 0.0, 1000.0));
    let m = s.mesh().unwrap();
    assert!(m.is_closed() && m.signed_volume() > 0.0);
}

#[test]
fn a_ring_profile_makes_a_torus() {
    let mut s = Sweep::new(circle(DVec2::new(3000.0, 0.0), 500.0));
    s.segment_angle_deg = 5.0;
    let m = s.mesh().unwrap();
    assert!(m.is_closed());
    let exact = 2.0 * PI * 3000.0 * PI * 500.0 * 500.0;
    assert!((m.volume() - exact).abs() / exact < 0.02, "{}", m.volume());
}

#[test]
fn a_partial_sweep_gets_end_caps() {
    let mut s = cylinder(1000.0, 1000.0);
    s.sweep_angle_deg = 90.0;
    s.segment_angle_deg = 5.0;
    assert_eq!(s.segments(), 18);
    let m = s.mesh().unwrap();
    assert!(m.is_closed() && m.signed_volume() > 0.0);
    let exact = PI * 1e6 * 1000.0 / 4.0;
    assert!((m.volume() - exact).abs() / exact < 0.01);
    // A quarter turn about the vertical axis reaches x and y from 0 to 1000.
    let (lo, hi) = s.bounds3();
    assert!(lo.x.abs() < 1e-6 && lo.y.abs() < 1e-6 && (hi.x - 1000.0).abs() < 1e-6);
}

#[test]
fn a_pitch_makes_a_spiral_with_two_caps() {
    let mut s = Sweep::new(rect(500.0, 0.0, 1000.0, 200.0));
    s.sweep_angle_deg = 720.0;
    s.pitch = 600.0;
    s.segment_angle_deg = 10.0;
    assert_eq!(s.problem(), None);
    let m = s.mesh().unwrap();
    assert!(m.is_closed() && m.signed_volume() > 0.0);
    let (lo, hi) = m.bounds();
    assert!((hi.z - (200.0 + 1200.0)).abs() < 1e-6 && lo.z == 0.0);
    // The volume is the area times the length of the path at the middle of the profile.
    let exact = 500.0 * 200.0 * 2.0 * PI * 750.0 * 2.0;
    assert!(m.volume() > 0.9 * exact && m.volume() < 1.1 * exact);
}

#[test]
fn a_full_turn_with_a_pitch_is_open_at_the_ends_and_capped() {
    let mut s = cylinder(1000.0, 500.0);
    s.pitch = 100.0;
    let m = s.mesh().unwrap();
    assert!(m.is_closed());
}

#[test]
fn a_profile_that_touches_the_axis_with_a_pitch_keeps_all_triangles() {
    let mut s = cylinder(500.0, 100.0);
    s.pitch = 50.0;
    s.sweep_angle_deg = 180.0;
    let m = s.mesh().unwrap();
    assert!(m.is_closed() || m.signed_volume().abs() > 0.0);
    assert!(all_finite(&m));
}

#[test]
fn an_open_profile_is_a_surface() {
    let mut s = Sweep::new(curve::corners(&[
        DVec2::new(100.0, 0.0),
        DVec2::new(1000.0, 500.0),
        DVec2::new(1000.0, 1000.0),
    ]));
    s.closed = false;
    assert_eq!(s.problem(), None);
    let m = s.mesh().unwrap();
    assert!(!m.is_closed() && m.area() > 0.0 && all_finite(&m));
    // A sweep of the whole way around has 36 segments of 2 edges, 2 triangles each.
    assert_eq!(m.triangles.len(), 36 * 2 * 2);
    s.sweep_angle_deg = 90.0;
    assert!(s.mesh().unwrap().area() < m.area());
}

#[test]
fn the_axis_can_lie_along_x_and_the_frame_is_right_handed() {
    let mut s = cylinder(100.0, 1000.0);
    s.axis = DVec3::new(5.0, 0.0, 0.0);
    s.origin = DVec3::new(10.0, 20.0, 30.0);
    let (w, e1, e2) = s.frame();
    assert!((w - DVec3::X).length() < 1e-12);
    assert!(w.dot(e1).abs() < 1e-12 && (e1.cross(e2) - w).length() < 1e-12);
    let m = s.mesh().unwrap();
    assert!(m.is_closed() && m.signed_volume() > 0.0);
    let (lo, hi) = m.bounds();
    assert!((lo.x - 10.0).abs() < 1e-6 && (hi.x - 1010.0).abs() < 1e-6);
    assert!((hi.y - 120.0).abs() < 1e-6 && (lo.z - (30.0 - 100.0)).abs() < 1e-6);
    // An axis along y and one that leans give closed solids too.
    for axis in [DVec3::Y, DVec3::new(1.0, 1.0, 1.0)] {
        s.axis = axis;
        let m = s.mesh().unwrap();
        assert!(m.is_closed() && m.signed_volume() > 0.0);
    }
    s.rotation_deg = 45.0;
    assert!(s.mesh().unwrap().is_closed());
}

#[test]
fn a_segment_count_above_the_limit_is_lowered() {
    let mut s = cylinder(100.0, 100.0);
    s.sweep_angle_deg = 3600.0;
    s.segment_angle_deg = MIN_SEGMENT_ANGLE;
    assert!(s.segments_capped());
    assert_eq!(s.segments(), MAX_SEGMENTS);
    // With a profile of 4 points the triangles stay below the budget.
    assert_eq!(s.problem(), None);
    s.segment_angle_deg = 10.0;
    assert!(!s.segments_capped());
    // A big profile with many segments passes the budget of triangles.
    let big: Vec<Vertex> = (0..4000)
        .map(|i| Vertex::corner(DVec2::new(100.0 + (i % 2) as f64, i as f64 * 10.0)))
        .collect();
    let mut s = Sweep::new(big);
    s.closed = false;
    s.sweep_angle_deg = 3600.0;
    s.segment_angle_deg = 1.0;
    assert!(s.problem().unwrap().contains("too many triangles"));
    assert!(s.mesh().is_none());
}

#[test]
fn bad_sweeps_say_what_is_wrong() {
    let ok = cylinder(100.0, 100.0);
    assert_eq!(ok.problem(), None);
    let problem = |f: &dyn Fn(&mut Sweep)| {
        let mut s = ok.clone();
        f(&mut s);
        assert!(s.mesh().is_none());
        s.problem().expect("a problem")
    };
    assert!(problem(&|s| s.pitch = f64::NAN).contains("finite"));
    assert!(problem(&|s| s.axis = DVec3::ZERO).contains("direction"));
    assert!(problem(&|s| s.origin.x = 5.0e9).contains("range"));
    assert!(problem(&|s| s.sweep_angle_deg = 0.0).contains("sweep angle"));
    assert!(problem(&|s| s.sweep_angle_deg = 3601.0).contains("sweep angle"));
    assert!(problem(&|s| s.segment_angle_deg = 0.0).contains("segment angle"));
    assert!(problem(&|s| s.segment_angle_deg = 181.0).contains("segment angle"));
    assert!(problem(&|s| s.profile.truncate(2)).contains("3 vertices"));
    assert!(problem(&|s| s.profile = rect(-50.0, 0.0, 50.0, 100.0)).contains("crosses the axis"));
    assert!(
        problem(&|s| {
            s.profile = curve::corners(&[
                DVec2::new(10.0, 0.0),
                DVec2::new(20.0, 10.0),
                DVec2::new(20.0, 0.0),
                DVec2::new(10.0, 10.0),
            ])
        })
        .contains("not simple")
    );
    assert!(
        problem(&|s| {
            s.closed = false;
            s.profile.truncate(1)
        })
        .contains("2 to 8000")
    );
    assert!(
        problem(&|s| {
            s.closed = false;
            s.profile = curve::corners(&[DVec2::new(5.0, 5.0); 3])
        })
        .contains("no length")
    );
}

#[test]
fn turning_a_sweep_in_the_plan_keeps_its_shape() {
    let mut s = Sweep::new(rect(200.0, 0.0, 700.0, 300.0));
    s.origin = DVec3::new(1000.0, 0.0, 50.0);
    s.sweep_angle_deg = 100.0;
    let before = s.mesh().unwrap();
    let mut t = s.clone();
    t.rotate_z(DVec2::ZERO, PI / 2.0);
    let after = t.mesh().unwrap();
    assert!(close(before.volume(), after.volume(), 1e-9));
    // The same points, turned a quarter about the z axis through the center.
    let mut b: Vec<DVec3> = before
        .vertices
        .iter()
        .map(|v| DVec3::new(-v.y, v.x, v.z))
        .collect();
    let mut a = after.vertices.clone();
    let key = |v: &DVec3| {
        (
            (v.x * 1e3).round() as i64,
            (v.y * 1e3).round() as i64,
            (v.z * 1e3).round() as i64,
        )
    };
    a.sort_by_key(key);
    b.sort_by_key(key);
    assert_eq!(a.len(), b.len());
    for (p, q) in a.iter().zip(&b) {
        assert!((*p - *q).length() < 1e-6, "{p} {q}");
    }
    // Axes that are not vertical turn too.
    let mut s = cylinder(100.0, 500.0);
    s.axis = DVec3::new(1.0, 0.0, 0.0);
    s.rotation_deg = 30.0;
    let mut t = s.clone();
    t.rotate_z(DVec2::new(5.0, 5.0), 1.0);
    assert!(close(
        s.mesh().unwrap().volume(),
        t.mesh().unwrap().volume(),
        1e-9
    ));
}

#[test]
fn sweep_json_round_trip_and_defaults() {
    let mut s = cylinder(10.0, 20.0);
    s.pitch = 5.0;
    s.rotation_deg = 10.0;
    s.closed = false;
    s.axis = DVec3::X;
    let text = serde_json::to_string(&s).unwrap();
    assert_eq!(serde_json::from_str::<Sweep>(&text).unwrap(), s);
    let min: Sweep = serde_json::from_str(r#"{"profile": [[1,0],[2,0],[2,1]]}"#).unwrap();
    assert_eq!(
        min,
        Sweep::new(curve::corners(&[
            DVec2::X,
            DVec2::new(2.0, 0.0),
            DVec2::new(2.0, 1.0)
        ]))
    );
    let v = serde_json::to_value(&min).unwrap();
    assert!(v.get("closed").is_none() && v.get("pitch").is_none());
}

// ------------------------------------------------------------------ 3D polygon

#[test]
fn a_polygon_face_follows_the_normal() {
    let flat = [
        DVec3::new(0.0, 0.0, 5.0),
        DVec3::new(10.0, 0.0, 5.0),
        DVec3::new(10.0, 10.0, 5.0),
        DVec3::new(0.0, 10.0, 5.0),
    ];
    let tris = polygon_face(&flat).unwrap();
    assert_eq!(tris.len(), 2);
    let normal = |t: &[u32; 3], pts: &[DVec3]| {
        let p = |i: u32| pts[i as usize];
        (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]))
    };
    assert!(normal(&tris[0], &flat).z > 0.0);
    let mut cw = flat;
    cw.reverse();
    let tris = polygon_face(&cw).unwrap();
    assert!(tris.iter().all(|t| normal(t, &cw).z < 0.0));
    // A wall (normal along x or y) and a leaning polygon.
    for pts in [
        [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(0.0, 10.0, 0.0),
            DVec3::new(0.0, 10.0, 10.0),
            DVec3::new(0.0, 0.0, 10.0),
        ],
        [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 10.0),
            DVec3::new(0.0, 0.0, 10.0),
        ],
        [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(10.0, 0.0, 1.0),
            DVec3::new(10.0, 10.0, 3.0),
            DVec3::new(0.0, 10.0, 2.0),
        ],
    ] {
        let tris = polygon_face(&pts).unwrap();
        let n = pts.iter().enumerate().fold(DVec3::ZERO, |s, (i, a)| {
            let b = pts[(i + 1) % 4];
            s + a.cross(b)
        });
        assert!(tris.iter().all(|t| normal(t, &pts).dot(n) > 0.0));
    }
    // Not enough points, no area and a bad number give no face.
    assert!(polygon_face(&flat[..2]).is_none());
    assert!(polygon_face(&[DVec3::ZERO, DVec3::X, DVec3::X * 2.0]).is_none());
    assert!(polygon_face(&[DVec3::ZERO, DVec3::X, DVec3::new(f64::NAN, 1.0, 0.0)]).is_none());
}

// ------------------------------------------------------------------ properties

fn star(radii: &[f64]) -> Vec<Vertex> {
    let n = radii.len();
    curve::corners(
        &radii
            .iter()
            .enumerate()
            .map(|(i, r)| DVec2::from_angle(std::f64::consts::TAU * i as f64 / n as f64) * *r)
            .collect::<Vec<_>>(),
    )
}

proptest! {
    // A star-shaped ring (one radius for each angle) is always simple.
    #[test]
    fn extruded_stars_are_closed_finite_and_outward(
        radii in proptest::collection::vec(100.0f64..5000.0, 3..40),
        height in prop_oneof![10.0f64..5000.0, -5000.0f64..-10.0],
        base in -1000.0f64..1000.0,
    ) {
        let mut e = Extrude::new(star(&radii), height);
        e.base = base;
        prop_assume!(e.problem().is_none());
        let m = e.mesh().unwrap();
        prop_assert!(m.is_closed());
        prop_assert!(all_finite(&m));
        prop_assert!(m.signed_volume() > 0.0);
        let want = e.profile_area() * height.abs();
        prop_assert!((m.signed_volume() - want).abs() <= 1e-6 * want);
        prop_assert_eq!(m.bounds(), e.bounds3());
    }

    #[test]
    fn extruded_rings_with_a_hole_are_closed(
        w in 2000.0f64..9000.0,
        d in 2000.0f64..9000.0,
        fx in 0.1f64..0.5,
        fy in 0.1f64..0.5,
        hx in 0.1f64..0.4,
        hy in 0.1f64..0.4,
        height in 10.0f64..3000.0,
    ) {
        let mut e = Extrude::new(rect(0.0, 0.0, w, d), height);
        let (x0, y0) = (w * fx, d * fy);
        e.holes = vec![rect(x0, y0, x0 + w * hx * 0.9, y0 + d * hy * 0.9)];
        prop_assume!(e.problem().is_none());
        let m = e.mesh().unwrap();
        prop_assert!(m.is_closed());
        prop_assert!(m.signed_volume() > 0.0);
        let want = e.profile_area() * height;
        prop_assert!((m.signed_volume() - want).abs() <= 1e-6 * want);
    }

    #[test]
    fn swept_profiles_are_closed_finite_and_outward(
        u0 in 0.0f64..2000.0,
        du in 50.0f64..2000.0,
        dv in 50.0f64..2000.0,
        angle in 5.0f64..720.0,
        seg in 2.0f64..45.0,
        pitch in prop_oneof![Just(0.0), -3000.0f64..3000.0],
        axis in (-1.0f64..1.0, -1.0f64..1.0, -1.0f64..1.0),
        rotation in 0.0f64..360.0,
    ) {
        let mut s = Sweep::new(rect(u0, 0.0, u0 + du, dv));
        s.sweep_angle_deg = angle;
        s.segment_angle_deg = seg;
        s.pitch = pitch;
        s.axis = DVec3::new(axis.0, axis.1, axis.2);
        s.rotation_deg = rotation;
        prop_assume!(s.axis.length() > 0.1);
        prop_assume!(s.problem().is_none());
        let m = s.mesh().unwrap();
        prop_assert!(all_finite(&m));
        prop_assert!(m.signed_volume() > 0.0);
        // A spiral may cross itself when the pitch is smaller than the profile; only flat
        // turns and wide pitches promise a closed manifold in every case.
        prop_assert!(m.is_closed());
        let (lo, hi) = s.bounds3();
        let (mlo, mhi) = m.bounds();
        prop_assert!((lo - mlo).length() < 1e-6 && (hi - mhi).length() < 1e-6);
    }

    #[test]
    fn any_numbers_give_a_mesh_that_is_valid_or_a_problem(
        a in -1.0e4f64..1.0e4, b in -1.0e4f64..1.0e4,
        c in -1.0e4f64..1.0e4, d in -1.0e4f64..1.0e4,
        e in -1.0e4f64..1.0e4, f in -1.0e4f64..1.0e4,
        h in -1.0e4f64..1.0e4,
    ) {
        let p = curve::corners(&[DVec2::new(a, b), DVec2::new(c, d), DVec2::new(e, f)]);
        let x = Extrude::new(p.clone(), h);
        match x.mesh() {
            Some(m) => { prop_assert!(all_finite(&m)); prop_assert!(m.is_closed()); }
            None => prop_assert!(x.problem().is_some()),
        }
        let s = Sweep::new(p);
        match s.mesh() {
            Some(m) => { prop_assert!(all_finite(&m)); prop_assert!(m.is_closed()); }
            None => prop_assert!(s.problem().is_some()),
        }
    }
}
