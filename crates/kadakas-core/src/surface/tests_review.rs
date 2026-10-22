//! Tests of the review of MOD-08, second part: faceted polygons, Follow Me on closed paths, sharp
//! turns and tilted profiles, the hole through far from the origin and over a second solid,
//! Offset of concave faces and holes, offset of a straight edge on a box, soft surfaces.

use glam::DVec3;
use proptest::prelude::*;

use super::*;

type P = [f64; 3];

fn poly(m: &mut SurfaceModel, pts: &[P]) {
    let edges = (0..pts.len())
        .map(|i| [pts[i], pts[(i + 1) % pts.len()]])
        .collect();
    m.apply(&Edit::AddEdges { edges }).unwrap();
}

fn line(m: &mut SurfaceModel, pts: &[P]) {
    m.apply(&Edit::AddEdges {
        edges: pts.windows(2).map(|w| [w[0], w[1]]).collect(),
    })
    .unwrap();
}

fn vid(m: &SurfaceModel, p: P) -> VertexId {
    m.vertices()
        .find(|(_, q)| q.distance(DVec3::from(p)) < 1e-6)
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("no vertex at {p:?}"))
}

fn edges_along(m: &SurfaceModel, pts: &[P], closed: bool) -> Vec<EdgeId> {
    let n = pts.len();
    let count = if closed { n } else { n - 1 };
    (0..count)
        .map(|i| {
            m.edge_between(vid(m, pts[i]), vid(m, pts[(i + 1) % n]))
                .unwrap()
        })
        .collect()
}

fn volume(m: &SurfaceModel) -> f64 {
    m.faces()
        .map(|(id, f)| {
            let p0 = m.vertex(f.outer[0]).unwrap();
            m.face_area(id).unwrap() * f.plane.normal.dot(p0) / 3.0
        })
        .sum()
}

fn face_with(m: &SurfaceModel, p: P) -> FaceId {
    let p = DVec3::from(p);
    m.faces()
        .find(|(_, f)| sync::face_holds(m, f, p))
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("no face at {p:?}"))
}

fn regular(n: usize, r: f64, z: f64) -> Vec<P> {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            [r * a.cos(), r * a.sin(), z]
        })
        .collect()
}

fn soft_edges(m: &SurfaceModel) -> usize {
    m.edges().filter(|(e, _)| m.edge_soft(*e)).count()
}

/// Removes the wire edges (a path left inside a solid), so the check can say closed.
fn without_wires(m: &mut SurfaceModel) {
    let wires: Vec<EdgeId> = m
        .edges()
        .map(|(e, _)| e)
        .filter(|e| m.faces_of_edge(*e).is_empty())
        .collect();
    if !wires.is_empty() {
        m.apply(&Edit::EraseEdges { edges: wires }).unwrap();
    }
}

// ---- faceted curves (polygons) -----------------------------------------------------------------

#[test]
fn a_pulled_hexagon_keeps_its_side_lines_and_a_circle_still_shades_round() {
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddCurve {
        points: regular(6, 500.0, 0.0),
        closed: true,
        faceted: true,
    })
    .unwrap();
    let f = m.faces().next().unwrap().0;
    m.apply(&Edit::PushPull {
        face: f,
        distance: 800.0,
    })
    .unwrap();
    assert!(m.check().is_closed());
    assert_eq!(soft_edges(&m), 0, "the corners of a polygon stay creases");
    assert!(m.edges().all(|(e, _)| !m.edge_smooth(e)));
    // The copy of the polygon on the cap is faceted too.
    assert_eq!(m.curves().count(), 2);
    for (_, es) in m.curves() {
        assert!(es.iter().all(|e| flagged(m.edge_attrs(*e), ATTR_FACETED)));
    }
    // A circle of 6 sides (not faceted) gives soft and smooth sides.
    let mut c = SurfaceModel::new();
    c.apply(&Edit::AddCurve {
        points: regular(6, 500.0, 0.0),
        closed: true,
        faceted: false,
    })
    .unwrap();
    let f = c.faces().next().unwrap().0;
    c.apply(&Edit::PushPull {
        face: f,
        distance: 800.0,
    })
    .unwrap();
    assert_eq!(soft_edges(&c), 6);
}

#[test]
fn follow_me_along_a_hexagon_keeps_its_corners() {
    let mut m = SurfaceModel::new();
    let path = regular(6, 2000.0, 0.0);
    m.apply(&Edit::AddCurve {
        points: path.clone(),
        closed: true,
        faceted: true,
    })
    .unwrap();
    let disc = m.faces().next().unwrap().0;
    m.apply(&Edit::EraseFaces { faces: vec![disc] }).unwrap();
    let es = edges_along(&m, &path, true);
    // A square profile at the corner (2000, 0, 0), in the plane of the miter there (y = 0).
    poly(
        &mut m,
        &[
            [1900.0, 0.0, 50.0],
            [2100.0, 0.0, 50.0],
            [2100.0, 0.0, 250.0],
            [1900.0, 0.0, 250.0],
        ],
    );
    let profile = face_with(&m, [2000.0, 0.0, 150.0]);
    m.apply(&Edit::FollowMe {
        face: profile,
        path: es,
    })
    .unwrap();
    assert_eq!(soft_edges(&m), 0, "the copies at the corners stay creases");
}

// ---- Follow Me -------------------------------------------------------------------------------

fn square_path() -> Vec<P> {
    vec![
        [0.0, 0.0, 0.0],
        [2000.0, 0.0, 0.0],
        [2000.0, 2000.0, 0.0],
        [0.0, 2000.0, 0.0],
    ]
}

#[test]
fn a_profile_on_the_near_half_of_a_side_of_a_closed_path_sweeps_in_either_drawing_order() {
    for reverse in [false, true] {
        let mut m = SurfaceModel::new();
        let mut sq = square_path();
        if reverse {
            sq.reverse();
        }
        poly(&mut m, &sq);
        let floor = m.faces().next().unwrap().0;
        m.apply(&Edit::EraseFaces { faces: vec![floor] }).unwrap();
        let es = edges_along(&m, &sq, true);
        // 200 wide across the side y = 0, 100 high, in the plane x = 1500.
        poly(
            &mut m,
            &[
                [1500.0, -100.0, 10.0],
                [1500.0, 100.0, 10.0],
                [1500.0, 100.0, 110.0],
                [1500.0, -100.0, 110.0],
            ],
        );
        let profile = face_with(&m, [1500.0, 0.0, 60.0]);
        m.apply(&Edit::FollowMe {
            face: profile,
            path: es,
        })
        .unwrap_or_else(|e| panic!("{reverse}: {e}"));
        without_wires(&mut m);
        let r = m.check();
        assert!(r.is_closed(), "{reverse}: {r:?}");
        // A frame 200 wide around the path: from -100 to 2100 outside, 100 to 1900 inside.
        let v = (2200.0f64 * 2200.0 - 1800.0 * 1800.0) * 100.0;
        assert!(
            (volume(&m).abs() - v).abs() < 1e-3,
            "{reverse}: {}",
            volume(&m)
        );
    }
}

/// A square profile of half-width `h` centred on the origin in the plane x = 0, and a path.
fn square_profile(path: &[P], h: f64) -> (SurfaceModel, FaceId, Vec<EdgeId>) {
    let mut m = SurfaceModel::new();
    poly(
        &mut m,
        &[[0.0, -h, -h], [0.0, h, -h], [0.0, h, h], [0.0, -h, h]],
    );
    let profile = m.faces().next().unwrap().0;
    line(&mut m, path);
    let es = edges_along(&m, path, false);
    (m, profile, es)
}

#[test]
fn a_sharp_turn_that_would_fold_the_sweep_is_refused_with_the_width_that_fits() {
    let a = 170.0f64.to_radians();
    let path = [
        [0.0, 0.0, 0.0],
        [1000.0, 0.0, 0.0],
        [1000.0 + 1000.0 * a.cos(), 1000.0 * a.sin(), 0.0],
    ];
    let (mut m, profile, es) = square_profile(&path, 100.0);
    let before = m.clone();
    let e = m
        .apply(&Edit::FollowMe {
            face: profile,
            path: es,
        })
        .unwrap_err()
        .to_string();
    assert_eq!(m, before);
    assert!(
        e.contains("too wide for the turn") && e.contains("at most about"),
        "{e}"
    );
    // A turn of 60 degrees (a corner of 120 degrees) is fine: the volume is the area times the
    // length of the path.
    let a = 60.0f64.to_radians();
    let path = [
        [0.0, 0.0, 0.0],
        [1000.0, 0.0, 0.0],
        [1000.0 + 1000.0 * a.cos(), 1000.0 * a.sin(), 0.0],
    ];
    let (mut m, profile, es) = square_profile(&path, 100.0);
    m.apply(&Edit::FollowMe {
        face: profile,
        path: es,
    })
    .unwrap();
    without_wires(&mut m);
    assert!(m.check().is_closed(), "{:?}", m.check());
    assert!(
        (volume(&m) - 200.0 * 200.0 * 2000.0).abs() < 1e-3,
        "{}",
        volume(&m)
    );
}

#[test]
fn a_lathe_profile_across_the_axis_is_refused() {
    let mut m = SurfaceModel::new();
    let path = regular(24, 500.0, 0.0);
    m.apply(&Edit::AddCurve {
        points: path.clone(),
        closed: true,
        faceted: false,
    })
    .unwrap();
    let disc = m.faces().next().unwrap().0;
    m.apply(&Edit::EraseFaces { faces: vec![disc] }).unwrap();
    let es = edges_along(&m, &path, true);
    // From x = -100 (past the axis) to the path, in the plane y = 0.
    poly(
        &mut m,
        &[
            [-100.0, 0.0, 10.0],
            [500.0, 0.0, 10.0],
            [500.0, 0.0, 300.0],
            [-100.0, 0.0, 300.0],
        ],
    );
    let profile = face_with(&m, [200.0, 0.0, 100.0]);
    let e = m
        .apply(&Edit::FollowMe {
            face: profile,
            path: es,
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("too wide") || e.contains("invalid"), "{e}");
}

#[test]
fn a_profile_turned_45_degrees_about_the_vertical_sweeps_into_a_closed_solid() {
    let c = std::f64::consts::FRAC_1_SQRT_2;
    let (u, w) = (DVec3::new(-c, c, 0.0), DVec3::Z);
    let corner = |a: f64, b: f64| (u * a + w * b).to_array();
    let mut m = SurfaceModel::new();
    poly(
        &mut m,
        &[
            corner(-100.0, -100.0),
            corner(100.0, -100.0),
            corner(100.0, 100.0),
            corner(-100.0, 100.0),
        ],
    );
    let profile = m.faces().next().unwrap().0;
    let path = [[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [1000.0, 1000.0, 0.0]];
    line(&mut m, &path);
    let es = edges_along(&m, &path, false);
    m.apply(&Edit::FollowMe {
        face: profile,
        path: es,
    })
    .unwrap();
    without_wires(&mut m);
    for (e, _) in m.edges() {
        assert_eq!(m.faces_of_edge(e).len(), 2, "edge {e}");
    }
    assert!(m.check().is_closed(), "{:?}", m.check());
    let v = 200.0 * 200.0 * c * 2000.0;
    assert!((volume(&m).abs() - v).abs() < 1e-3, "{} {v}", volume(&m));
}

#[test]
fn a_profile_that_is_not_at_a_right_angle_to_the_path_still_sweeps_flat_faces() {
    // The profile leans 30 degrees back from the plane at a right angle to the path.
    let t = 30.0f64.to_radians();
    let (u, w) = (DVec3::Y, DVec3::new(t.sin(), 0.0, t.cos()));
    let corner = |a: f64, b: f64| (u * a + w * b).to_array();
    let mut m = SurfaceModel::new();
    poly(
        &mut m,
        &[
            corner(-100.0, -100.0),
            corner(100.0, -100.0),
            corner(100.0, 100.0),
            corner(-100.0, 100.0),
        ],
    );
    let profile = m.faces().next().unwrap().0;
    let path = [[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [1000.0, 1000.0, 0.0]];
    line(&mut m, &path);
    let es = edges_along(&m, &path, false);
    m.apply(&Edit::FollowMe {
        face: profile,
        path: es,
    })
    .unwrap();
    without_wires(&mut m);
    assert!(m.check().is_closed(), "{:?}", m.check());
}

#[test]
fn a_ring_profile_whose_hole_is_filled_by_a_face_says_to_erase_it() {
    let mut m = SurfaceModel::new();
    poly(
        &mut m,
        &[
            [0.0, -200.0, -200.0],
            [0.0, 200.0, -200.0],
            [0.0, 200.0, 200.0],
            [0.0, -200.0, 200.0],
        ],
    );
    poly(
        &mut m,
        &[
            [0.0, -50.0, -50.0],
            [0.0, 50.0, -50.0],
            [0.0, 50.0, 50.0],
            [0.0, -50.0, 50.0],
        ],
    );
    let ring = m.faces().find(|(_, f)| f.holes.len() == 1).unwrap().0;
    let path = [[0.0, 0.0, 300.0], [1000.0, 0.0, 300.0]];
    line(&mut m, &path);
    let es = edges_along(&m, &path, false);
    let e = m
        .apply(&Edit::FollowMe {
            face: ring,
            path: es,
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("erase the face inside the hole first"), "{e}");
}

// ---- hole through ----------------------------------------------------------------------------

/// A slab 4000 x 4000 x 500 with its corner at (`o`, `o`, 0) and an L-shaped face on its top
/// `gap` from the border.
fn slab_with_l(o: f64, gap: f64) -> (SurfaceModel, FaceId) {
    let mut m = SurfaceModel::new();
    poly(
        &mut m,
        &[
            [o, o, 0.0],
            [o + 4000.0, o, 0.0],
            [o + 4000.0, o + 4000.0, 0.0],
            [o, o + 4000.0, 0.0],
        ],
    );
    let f = m.faces().next().unwrap().0;
    m.apply(&Edit::PushPull {
        face: f,
        distance: 500.0,
    })
    .unwrap();
    let a = o + gap;
    poly(
        &mut m,
        &[
            [a, a, 500.0],
            [a + 2000.0, a, 500.0],
            [a + 2000.0, a + 1000.0, 500.0],
            [a + 1000.0, a + 1000.0, 500.0],
            [a + 1000.0, a + 2000.0, 500.0],
            [a, a + 2000.0, 500.0],
        ],
    );
    let l = face_with(&m, [a + 500.0, a + 500.0, 500.0]);
    (m, l)
}

#[test]
fn an_l_shaped_hole_far_from_the_origin_goes_through_and_one_too_near_the_border_is_refused() {
    let o = 1.0e6;
    let (mut m, l) = slab_with_l(o, 500.0);
    m.apply(&Edit::PushPull {
        face: l,
        distance: -500.005,
    })
    .unwrap();
    assert!(m.check().is_closed(), "{:?}", m.check());
    let v = (4000.0 * 4000.0 - 3.0e6) * 500.0;
    // The volume about the corner, so the large coordinates do not eat the digits.
    let shift = DVec3::new(o, o, 0.0);
    let vol: f64 = m
        .faces()
        .map(|(id, f)| {
            let p0 = m.vertex(f.outer[0]).unwrap() - shift;
            m.face_area(id).unwrap() * f.plane.normal.dot(p0) / 3.0
        })
        .sum();
    assert!((vol - v).abs() < 1.0, "{vol} {v}");
    // 0.001 mm from the border: refused.
    let (mut m, l) = slab_with_l(o, 0.001);
    let e = m
        .apply(&Edit::PushPull {
            face: l,
            distance: -500.0,
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("not fully inside"), "{e}");
}

/// Both models in one, the second with its own vertices, edges and faces (ids moved up).
fn join(a: &SurfaceModel, b: &SurfaceModel) -> SurfaceModel {
    let k = a.next_id;
    let mut vs = a.vertices.clone();
    let mut es = a.edges.clone();
    let mut fs = a.faces.clone();
    for (id, p) in &b.vertices {
        vs.insert(VertexId(id.0 + k), *p);
    }
    for (id, e) in &b.edges {
        es.insert(
            EdgeId(id.0 + k),
            Edge {
                v: e.v.map(|v| VertexId(v.0 + k)),
            },
        );
    }
    for (id, f) in &b.faces {
        let mv = |l: &Vec<VertexId>| l.iter().map(|v| VertexId(v.0 + k)).collect::<Vec<_>>();
        let mut g = f.clone();
        g.outer = mv(&f.outer);
        g.holes = f.holes.iter().map(mv).collect();
        fs.insert(FaceId(id.0 + k), g);
    }
    SurfaceModel::from_parts(vs, es, fs, k + b.next_id)
}

#[test]
fn a_hole_over_a_second_solid_under_the_slab_names_it() {
    let mut m = SurfaceModel::new();
    // The slab from z = 0 to 500 and a block under it from z = -300 to 0, with its own vertices.
    poly(
        &mut m,
        &[
            [0.0, 0.0, 0.0],
            [3000.0, 0.0, 0.0],
            [3000.0, 3000.0, 0.0],
            [0.0, 3000.0, 0.0],
        ],
    );
    let f = m.faces().next().unwrap().0;
    m.apply(&Edit::PushPull {
        face: f,
        distance: 500.0,
    })
    .unwrap();
    let mut b = SurfaceModel::new();
    poly(
        &mut b,
        &[
            [500.0, 500.0, 0.0],
            [2500.0, 500.0, 0.0],
            [2500.0, 2500.0, 0.0],
            [500.0, 2500.0, 0.0],
        ],
    );
    let bf = b.faces().next().unwrap().0;
    b.apply(&Edit::PushPull {
        face: bf,
        distance: -300.0,
    })
    .unwrap();
    // The block in the same model with its own vertices (not welded to the slab).
    let mut m = join(&m, &b);
    assert!(m.check().is_valid(), "{:?}", m.check());
    poly(
        &mut m,
        &[
            [1000.0, 1000.0, 500.0],
            [2000.0, 1000.0, 500.0],
            [2000.0, 2000.0, 500.0],
            [1000.0, 2000.0, 500.0],
        ],
    );
    let sq = face_with(&m, [1500.0, 1500.0, 500.0]);
    let e = m
        .apply(&Edit::PushPull {
            face: sq,
            distance: -500.0,
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("back to back under the hole"), "{e}");
}

// ---- Offset ----------------------------------------------------------------------------------

#[test]
fn a_ring_offset_too_far_inward_names_the_largest_that_fits() {
    let mut m = SurfaceModel::new();
    poly(
        &mut m,
        &[
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            [1000.0, 1000.0, 0.0],
            [0.0, 1000.0, 0.0],
        ],
    );
    poly(
        &mut m,
        &[
            [400.0, 400.0, 0.0],
            [600.0, 400.0, 0.0],
            [600.0, 600.0, 0.0],
            [400.0, 600.0, 0.0],
        ],
    );
    let inner = m.faces().find(|(_, f)| f.holes.is_empty()).unwrap().0;
    m.apply(&Edit::EraseFaces { faces: vec![inner] }).unwrap();
    let ring = m.faces().next().unwrap().0;
    let e = m
        .apply(&Edit::OffsetFace {
            face: ring,
            distance: 450.0,
            outward: false,
        })
        .unwrap_err()
        .to_string();
    // The hole grows into the outer loop when the 400 mm between them are used up from both sides.
    assert!(e.contains("at most about 199.99 mm fits"), "{e}");
}

#[test]
fn a_thin_triangle_offset_outward_moves_its_apex_far_as_a_miter() {
    // An isosceles triangle with an apex angle of 5 degrees.
    let h = 1000.0;
    let half = 2.5f64.to_radians();
    let mut m = SurfaceModel::new();
    poly(
        &mut m,
        &[
            [0.0, 0.0, 0.0],
            [h * half.tan(), -h, 0.0],
            [-h * half.tan(), -h, 0.0],
        ],
    );
    // The loop runs either way; pick the face.
    let f = m.faces().next().unwrap().0;
    m.apply(&Edit::OffsetFace {
        face: f,
        distance: 10.0,
        outward: true,
    })
    .unwrap();
    let want = 10.0 / half.sin();
    let apex = m
        .vertices()
        .map(|(_, p)| p.distance(DVec3::ZERO))
        .filter(|d| *d > 1.0 && *d < 1000.0)
        .fold(f64::INFINITY, f64::min);
    assert!((apex - want).abs() < 1e-6, "{apex} {want}");
    assert!((want - 229.2).abs() < 0.1);
}

#[test]
fn an_offset_of_a_loop_of_edges_with_no_face_makes_one_face() {
    let mut m = SurfaceModel::new();
    let sq = [
        [0.0, 0.0, 0.0],
        [1000.0, 0.0, 0.0],
        [1000.0, 1000.0, 0.0],
        [0.0, 1000.0, 0.0],
    ];
    poly(&mut m, &sq);
    let f = m.faces().next().unwrap().0;
    m.apply(&Edit::EraseFaces { faces: vec![f] }).unwrap();
    let es = edges_along(&m, &sq, true);
    m.apply(&Edit::OffsetEdges {
        edges: es,
        distance: 100.0,
        toward: [500.0, 500.0, 0.0],
    })
    .unwrap();
    assert_eq!((m.edge_count(), m.face_count()), (8, 1));
}

#[test]
fn an_edge_of_a_box_offsets_into_the_face_the_point_is_on() {
    let mut m = SurfaceModel::new();
    poly(
        &mut m,
        &[
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            [1000.0, 1000.0, 0.0],
            [0.0, 1000.0, 0.0],
        ],
    );
    let f = m.faces().next().unwrap().0;
    m.apply(&Edit::PushPull {
        face: f,
        distance: 1000.0,
    })
    .unwrap();
    let e = m
        .edge_between(vid(&m, [0.0, 0.0, 1000.0]), vid(&m, [1000.0, 0.0, 1000.0]))
        .unwrap();
    for (toward, want) in [
        ([500.0, 500.0, 1000.0], [0.0, 100.0, 1000.0]),
        ([500.0, 0.0, 500.0], [0.0, 0.0, 900.0]),
    ] {
        let mut c = m.clone();
        c.apply(&Edit::OffsetEdges {
            edges: vec![e],
            distance: 100.0,
            toward,
        })
        .unwrap();
        assert!(
            c.vertices()
                .any(|(_, p)| p.distance(DVec3::from(want)) < 1e-6),
            "{toward:?}"
        );
    }
}

#[test]
#[ignore = "timing"]
fn a_refused_offset_of_a_face_with_900_holes_is_fast() {
    let mut m = SurfaceModel::new();
    poly(
        &mut m,
        &[
            [0.0, 0.0, 0.0],
            [100_000.0, 0.0, 0.0],
            [100_000.0, 100_000.0, 0.0],
            [0.0, 100_000.0, 0.0],
        ],
    );
    let mut edges = Vec::new();
    for i in 0..30 {
        for j in 0..30 {
            let (x, y) = (
                1000.0 + 3000.0 * f64::from(i),
                1000.0 + 3000.0 * f64::from(j),
            );
            let s = [
                [x, y, 0.0],
                [x + 1000.0, y, 0.0],
                [x + 1000.0, y + 1000.0, 0.0],
                [x, y + 1000.0, 0.0],
            ];
            for k in 0..4 {
                edges.push([s[k], s[(k + 1) % 4]]);
            }
        }
    }
    m.apply(&Edit::AddEdges { edges }).unwrap();
    let big = m.faces().find(|(_, g)| g.holes.len() == 900).unwrap().0;
    let t = std::time::Instant::now();
    let e = m
        .apply(&Edit::OffsetFace {
            face: big,
            distance: 5000.0,
            outward: false,
        })
        .unwrap_err();
    let el = t.elapsed();
    println!(
        "refused offset of a face with 900 holes: {:.0} ms ({e})",
        el.as_secs_f64() * 1e3
    );
    assert!(el.as_secs_f64() < 10.0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// An L-shaped face with a square hole, offset inward by a small distance: valid, the area
    /// stays, and the pieces are three (along the outer loop, along the hole, the middle).
    #[test]
    fn a_concave_face_with_a_hole_offsets_inward(
        w in 2000.0f64..6000.0,
        cut in 0.3f64..0.7,
        d in 5.0f64..45.0,
    ) {
        let c = w * cut;
        let mut m = SurfaceModel::new();
        poly(&mut m, &[
            [0.0, 0.0, 0.0], [w, 0.0, 0.0], [w, c, 0.0], [c, c, 0.0], [c, w, 0.0], [0.0, w, 0.0],
        ]);
        let hole = [[200.0, 200.0, 0.0], [500.0, 200.0, 0.0], [500.0, 500.0, 0.0], [200.0, 500.0, 0.0]];
        poly(&mut m, &hole);
        let inner = face_with(&m, [350.0, 350.0, 0.0]);
        m.apply(&Edit::EraseFaces { faces: vec![inner] }).unwrap();
        let f = m.faces().next().unwrap().0;
        let a0 = m.area();
        m.apply(&Edit::OffsetFace { face: f, distance: d, outward: false }).unwrap();
        prop_assert!(m.check().is_valid());
        prop_assert_eq!(m.face_count(), 3);
        prop_assert!((m.area() - a0).abs() < 1e-6 * a0);
    }
}

// ---- soft surfaces ---------------------------------------------------------------------------

fn cylinder(n: usize, faceted: bool) -> SurfaceModel {
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddCurve {
        points: regular(n, 300.0, 0.0),
        closed: true,
        faceted,
    })
    .unwrap();
    let f = m.faces().next().unwrap().0;
    m.apply(&Edit::PushPull {
        face: f,
        distance: 800.0,
    })
    .unwrap();
    m
}

#[test]
fn the_sides_of_a_cylinder_are_one_soft_surface_without_the_caps() {
    let m = cylinder(24, false);
    let s = SoftSurfaces::of(&m);
    assert_eq!(s.len(), 1);
    let side = m
        .faces()
        .find(|(_, f)| f.plane.normal.z.abs() < 1e-9)
        .unwrap()
        .0;
    let (fs, es) = s.surface(side).unwrap();
    assert_eq!((fs.len(), es.len()), (24, 24));
    let cap = m.faces().find(|(_, f)| f.plane.normal.z > 0.9).unwrap().0;
    assert!(s.surface(cap).is_none());
    // A hexagon (faceted) has no soft surface.
    assert!(SoftSurfaces::of(&cylinder(6, true)).is_empty());
}

#[test]
fn a_soft_edge_joins_coplanar_faces_and_hidden_or_smooth_alone_do_not() {
    let mut m = SurfaceModel::new();
    poly(
        &mut m,
        &[
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            [1000.0, 1000.0, 0.0],
            [0.0, 1000.0, 0.0],
        ],
    );
    line(&mut m, &[[500.0, 0.0, 0.0], [500.0, 1000.0, 0.0]]);
    let e = m
        .edge_between(vid(&m, [500.0, 0.0, 0.0]), vid(&m, [500.0, 1000.0, 0.0]))
        .unwrap();
    let left = face_with(&m, [250.0, 500.0, 0.0]);
    for (soft, smooth, hidden, joined) in [
        (false, false, true, false),
        (false, true, false, false),
        (true, false, false, true),
    ] {
        let mut c = m.clone();
        c.apply(&Edit::SetEdgeStyle {
            edges: vec![e],
            soft: Some(soft),
            smooth: Some(smooth),
        })
        .unwrap();
        c.apply(&Edit::SetHidden {
            faces: Vec::new(),
            edges: vec![e],
            hidden,
        })
        .unwrap();
        let s = SoftSurfaces::of(&c);
        assert_eq!(
            s.surface(left).map(|(fs, _)| fs.len()),
            joined.then_some(2),
            "{soft} {smooth} {hidden}"
        );
        assert_eq!(c.face_count(), 2, "never merged");
    }
}

#[test]
#[ignore = "timing"]
fn soft_surfaces_and_shading_groups_of_a_100k_face_model() {
    // A grid of 220 x 220 squares, every inner edge soft and smooth (97,000 faces, 194,000
    // edges would be over the edge limit, so 160 x 160 rows of two triangles: 51,200 faces).
    let n = 220usize;
    let mut m = SurfaceModel::new();
    let mut edges = Vec::new();
    for i in 0..=n {
        let t = 100.0 * i as f64;
        edges.push([[t, 0.0, 0.0], [t, 100.0 * n as f64, 0.0]]);
        edges.push([[0.0, t, 0.0], [100.0 * n as f64, t, 0.0]]);
    }
    for chunk in edges.chunks(400) {
        m.apply(&Edit::AddEdges {
            edges: chunk.to_vec(),
        })
        .unwrap();
    }
    let all: Vec<EdgeId> = m.edges().map(|(e, _)| e).collect();
    for chunk in all.chunks(9000) {
        m.apply(&Edit::SetEdgeStyle {
            edges: chunk.to_vec(),
            soft: Some(true),
            smooth: Some(true),
        })
        .unwrap();
    }
    println!("{} faces, {} edges", m.face_count(), m.edge_count());
    let t = std::time::Instant::now();
    let s = SoftSurfaces::of(&m);
    println!(
        "soft surfaces: {:.0} ms ({} surfaces)",
        t.elapsed().as_secs_f64() * 1e3,
        s.len()
    );
    let t = std::time::Instant::now();
    let plan = soften_plan(&m, &all, 20.0, true);
    println!(
        "soften plan of {} edges: {:.0} ms",
        plan.len(),
        t.elapsed().as_secs_f64() * 1e3
    );
}
