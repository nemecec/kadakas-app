//! Tests of Offset (`Edit::OffsetFace`, MOD-08).

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

fn square(x0: f64, y0: f64, w: f64, z: f64) -> Vec<P> {
    vec![
        [x0, y0, z],
        [x0 + w, y0, z],
        [x0 + w, y0 + w, z],
        [x0, y0 + w, z],
    ]
}

fn offset(m: &mut SurfaceModel, face: FaceId, d: f64, outward: bool) -> Result<(), SurfaceError> {
    let r = m.apply(&Edit::OffsetFace {
        face,
        distance: d,
        outward,
    });
    if r.is_ok() {
        let rep = m.check();
        assert!(rep.is_valid(), "{:?}", rep.problems);
    }
    r
}

fn lone(pts: &[P]) -> (SurfaceModel, FaceId) {
    let mut m = SurfaceModel::new();
    poly(&mut m, pts);
    let f = m.faces().next().unwrap().0;
    (m, f)
}

/// The areas of the faces, largest first.
fn areas(m: &SurfaceModel) -> Vec<f64> {
    let mut a: Vec<f64> = m.faces().map(|(id, _)| m.face_area(id).unwrap()).collect();
    a.sort_by(|x, y| y.total_cmp(x));
    a
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0)
}

#[test]
fn an_inward_offset_of_a_square_splits_it_into_a_ring_and_a_square() {
    let (mut m, f) = lone(&square(0.0, 0.0, 1000.0, 0.0));
    let rev = m.rev();
    offset(&mut m, f, 100.0, false).unwrap();
    assert_eq!(m.rev(), rev + 1);
    assert_eq!(
        (m.vertex_count(), m.edge_count(), m.face_count()),
        (8, 8, 2)
    );
    let a = areas(&m);
    assert!(
        close(a[0], 1.0e6 - 640_000.0) || close(a[0], 640_000.0),
        "{a:?}"
    );
    assert!(close(m.area(), 1.0e6));
    // The new corners are 100 inside.
    for p in [[100.0, 100.0, 0.0], [900.0, 900.0, 0.0]] {
        assert!(m.vertices().any(|(_, q)| q.distance(DVec3::from(p)) < 1e-9));
    }
    // The pieces look the same way as the face.
    assert!(m.faces().all(|(_, g)| g.plane.normal.z > 0.9));
}

#[test]
fn an_outward_offset_of_a_lone_square_adds_a_ring_around_it() {
    let (mut m, f) = lone(&square(0.0, 0.0, 1000.0, 0.0));
    offset(&mut m, f, 100.0, true).unwrap();
    assert_eq!(m.face_count(), 2);
    assert!(close(m.area(), 1200.0 * 1200.0));
    assert!(
        close(m.face_area(f).unwrap(), 1.0e6),
        "the face keeps its area and id"
    );
}

#[test]
fn an_outward_offset_inside_a_larger_face_splits_that_face() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 3000.0, 0.0));
    poly(&mut m, &square(1000.0, 1000.0, 1000.0, 0.0));
    let inner = m
        .faces()
        .find(|(_, g)| g.holes.is_empty() && g.outer.len() == 4)
        .filter(|(id, _)| m.face_area(*id).unwrap() < 2.0e6)
        .unwrap()
        .0;
    offset(&mut m, inner, 200.0, true).unwrap();
    assert_eq!(m.face_count(), 3);
    assert!(close(m.area(), 9.0e6));
    let a = areas(&m);
    assert!(
        a.iter().any(|x| close(*x, 1400.0 * 1400.0 - 1.0e6)),
        "{a:?}"
    );
}

#[test]
fn a_concave_face_offsets_with_miter_corners() {
    // An L: 2000 x 2000 less the square 1000 x 1000 at the top right.
    let l = vec![
        [0.0, 0.0, 0.0],
        [2000.0, 0.0, 0.0],
        [2000.0, 1000.0, 0.0],
        [1000.0, 1000.0, 0.0],
        [1000.0, 2000.0, 0.0],
        [0.0, 2000.0, 0.0],
    ];
    let (mut m, f) = lone(&l);
    offset(&mut m, f, 100.0, false).unwrap();
    assert_eq!(m.face_count(), 2);
    // The inner L: 1800 x 1800 less 1000 x 1000 (the concave corner moves out to 1100, 1100).
    let inner = 1800.0 * 1800.0 - 1000.0 * 1000.0;
    assert!(
        areas(&m).iter().any(|a| close(*a, inner)),
        "{:?}",
        areas(&m)
    );
    assert!(
        m.vertices()
            .any(|(_, p)| p.distance(DVec3::new(900.0, 900.0, 0.0)) < 1e-9)
    );
    // Outward too: the concave corner is cut by the miter.
    let (mut m, f) = lone(&l);
    offset(&mut m, f, 100.0, true).unwrap();
    assert!(close(m.area(), 2200.0 * 2200.0 - 1000.0 * 1000.0));
}

#[test]
fn a_face_with_a_hole_offsets_both_loops() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    poly(&mut m, &square(400.0, 400.0, 200.0, 0.0));
    // Take the hole's face out: a ring with a hole.
    let inner = m.faces().find(|(_, g)| g.holes.is_empty()).unwrap().0;
    m.apply(&Edit::EraseFaces { faces: vec![inner] }).unwrap();
    let ring = m.faces().next().unwrap().0;
    let a0 = m.area();
    offset(&mut m, ring, 100.0, false).unwrap();
    // Along the outer loop, along the hole, and the middle.
    assert_eq!(m.face_count(), 3);
    assert!(close(m.area(), a0));
    let middle = 800.0 * 800.0 - 400.0 * 400.0;
    assert!(
        areas(&m).iter().any(|a| close(*a, middle)),
        "{:?}",
        areas(&m)
    );
    // Outward the hole shrinks (into the hole, where there is no face: a ring face in it).
    let mut m2 = SurfaceModel::new();
    poly(&mut m2, &square(0.0, 0.0, 1000.0, 0.0));
    poly(&mut m2, &square(400.0, 400.0, 200.0, 0.0));
    let inner = m2.faces().find(|(_, g)| g.holes.is_empty()).unwrap().0;
    m2.apply(&Edit::EraseFaces { faces: vec![inner] }).unwrap();
    let ring = m2.faces().next().unwrap().0;
    offset(&mut m2, ring, 50.0, true).unwrap();
    assert!(
        m2.vertices()
            .any(|(_, p)| p.distance(DVec3::new(450.0, 450.0, 0.0)) < 1e-9)
    );
    // A hole that would close is refused.
    let e = m2
        .apply(&Edit::OffsetFace {
            face: ring,
            distance: 400.0,
            outward: true,
        })
        .unwrap_err();
    assert!(e.to_string().contains("does not fit"), "{e}");
}

#[test]
fn an_offset_too_large_is_refused_with_the_largest_that_fits() {
    let (mut m, f) = lone(&square(0.0, 0.0, 1000.0, 0.0));
    let before = m.clone();
    let e = m
        .apply(&Edit::OffsetFace {
            face: f,
            distance: 600.0,
            outward: false,
        })
        .unwrap_err();
    assert_eq!(m, before);
    let why = e.to_string();
    assert!(why.contains("at most about 499.99 mm"), "{why}");
    for d in [0.0, -5.0, 0.005] {
        let e = m
            .apply(&Edit::OffsetFace {
                face: f,
                distance: d,
                outward: false,
            })
            .unwrap_err();
        assert!(
            e.to_string().contains("larger than the length tolerance"),
            "{d}: {e}"
        );
    }
    assert!(
        m.apply(&Edit::OffsetFace {
            face: f,
            distance: f64::NAN,
            outward: false
        })
        .is_err()
    );
    assert_eq!(
        m.apply(&Edit::OffsetFace {
            face: FaceId(999),
            distance: 5.0,
            outward: false
        }),
        Err(SurfaceError::NoFace(FaceId(999)))
    );
    assert_eq!(m, before);
}

fn circle(n: usize, r: f64, cx: f64, cy: f64) -> Vec<P> {
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            [cx + r * a.cos(), cy + r * a.sin(), 0.0]
        })
        .collect()
}

#[test]
fn the_offset_of_a_circle_is_one_curve() {
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddCurve {
        points: circle(24, 500.0, 0.0, 0.0),
        closed: true,
    })
    .unwrap();
    let f = m.faces().next().unwrap().0;
    offset(&mut m, f, 100.0, false).unwrap();
    let curves: Vec<_> = m.curves().map(|(_, es)| es.len()).collect();
    assert_eq!(curves, vec![24, 24]);
    assert!(m.check().is_valid());
    // Outward too.
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddCurve {
        points: circle(24, 500.0, 0.0, 0.0),
        closed: true,
    })
    .unwrap();
    let f = m.faces().next().unwrap().0;
    offset(&mut m, f, 100.0, true).unwrap();
    assert_eq!(m.curves().count(), 2);
}

#[test]
fn an_arc_in_the_outline_offsets_as_one_curve_and_the_straight_sides_stay_lines() {
    // A rectangle 2000 x 1000 whose top side is an arc of 8 pieces.
    let mut m = SurfaceModel::new();
    let mut arc: Vec<P> = (0..=8)
        .map(|i| {
            let t = std::f64::consts::PI * f64::from(i) / 8.0;
            [1000.0 + 1000.0 * t.cos(), 1000.0 + 300.0 * t.sin(), 0.0]
        })
        .collect();
    arc.reverse();
    m.apply(&Edit::AddCurve {
        points: arc.iter().rev().copied().collect(),
        closed: false,
    })
    .unwrap();
    m.apply(&Edit::AddEdges {
        edges: vec![
            [[2000.0, 1000.0, 0.0], [2000.0, 0.0, 0.0]],
            [[2000.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
            [[0.0, 0.0, 0.0], [0.0, 1000.0, 0.0]],
        ],
    })
    .unwrap();
    assert_eq!(m.face_count(), 1);
    let f = m.faces().next().unwrap().0;
    offset(&mut m, f, 50.0, false).unwrap();
    let mut curves: Vec<usize> = m.curves().map(|(_, es)| es.len()).collect();
    curves.sort();
    assert_eq!(curves, vec![8, 8]);
}

#[test]
fn the_pieces_keep_the_paint_and_the_offset_replays() {
    let (mut m, f) = lone(&square(0.0, 0.0, 1000.0, 0.0));
    m.apply(&Edit::SetFaceMaterial {
        faces: vec![f],
        side: PaintSide::Both,
        material: Some("grass".into()),
    })
    .unwrap();
    let m0 = m.clone();
    offset(&mut m, f, 250.0, false).unwrap();
    assert!(
        m.faces()
            .all(|(_, g)| g.attrs.get(ATTR_MATERIAL) == Some(&serde_json::json!("grass")))
    );
    let mut again = m0.clone();
    offset(&mut again, f, 250.0, false).unwrap();
    assert_eq!(again, m, "the same ids every time");
    let entry = m.log().last().unwrap().clone();
    let mut c = m0.clone();
    super::sync::replay_intent(&mut c, &entry.intent).unwrap();
    assert_eq!(c.face_count(), 2);
    assert!(close(c.area(), 1.0e6));
}

#[test]
fn the_top_of_a_box_offset_inward_can_be_pulled_up() {
    let (mut m, f) = lone(&square(0.0, 0.0, 1000.0, 0.0));
    m.apply(&Edit::PushPull {
        face: f,
        distance: 500.0,
    })
    .unwrap();
    offset(&mut m, f, 100.0, false).unwrap();
    let inner = m
        .faces()
        .find(|(id, g)| {
            g.plane.normal.z > 0.9
                && g.holes.is_empty()
                && close(m.face_area(*id).unwrap(), 640_000.0)
        })
        .unwrap()
        .0;
    m.apply(&Edit::PushPull {
        face: inner,
        distance: 200.0,
    })
    .unwrap();
    assert!(m.check().is_closed());
}

#[test]
fn a_slanted_face_offsets_in_its_plane() {
    let n = DVec3::new(0.0, -1.0, 1.0).normalize();
    let (u, v) = (DVec3::X, n.cross(DVec3::X));
    let pts: Vec<P> = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 600.0), (0.0, 600.0)]
        .iter()
        .map(|(a, b)| (u * *a + v * *b + DVec3::new(10.0, 20.0, 30.0)).to_array())
        .collect();
    let (mut m, f) = lone(&pts);
    let plane = m.face(f).unwrap().plane;
    offset(&mut m, f, 50.0, false).unwrap();
    assert!(m.vertices().all(|(_, p)| on_plane(&plane, p)));
    assert!(areas(&m).iter().any(|a| close(*a, 900.0 * 500.0)));
}

#[test]
#[ignore = "timing"]
fn offset_timing_on_a_face_with_many_holes() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 100_000.0, 0.0));
    let mut edges = Vec::new();
    for i in 0..30 {
        for j in 0..30 {
            let sq = square(
                1000.0 + 3000.0 * i as f64,
                1000.0 + 3000.0 * j as f64,
                1000.0,
                0.0,
            );
            for k in 0..4 {
                edges.push([sq[k], sq[(k + 1) % 4]]);
            }
        }
    }
    m.apply(&Edit::AddEdges { edges }).unwrap();
    let big = m.faces().find(|(_, g)| g.holes.len() == 900).unwrap().0;
    let t = std::time::Instant::now();
    m.apply(&Edit::OffsetFace {
        face: big,
        distance: 100.0,
        outward: false,
    })
    .unwrap();
    let el = t.elapsed();
    println!(
        "offset of a face with 900 holes: {:.0} ms",
        el.as_secs_f64() * 1e3
    );
    assert!(m.check().is_valid());
    assert!(el.as_secs_f64() < 10.0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// A regular polygon offset inward by a share of its apothem: the area stays, the inner face
    /// is the polygon scaled by `(a - d) / a`, and the result is valid and the same every time.
    #[test]
    fn a_regular_polygon_offsets_to_a_scaled_copy(
        n in 3usize..40,
        r in 100.0f64..50_000.0,
        share in 0.02f64..0.95,
        turn in 0.0f64..std::f64::consts::TAU,
    ) {
        let pts: Vec<P> = (0..n)
            .map(|i| {
                let a = turn + std::f64::consts::TAU * i as f64 / n as f64;
                [r * a.cos(), r * a.sin(), 0.0]
            })
            .collect();
        let (m0, f) = lone(&pts);
        let apothem = r * (std::f64::consts::PI / n as f64).cos();
        let d = apothem * share;
        let mut m = m0.clone();
        offset(&mut m, f, d, false).unwrap();
        prop_assert_eq!(m.face_count(), 2);
        prop_assert!(close(m.area(), m0.area()));
        let k = (apothem - d) / apothem;
        let want = m0.area() * k * k;
        prop_assert!(areas(&m).iter().any(|a| (a - want).abs() <= 1e-6 * m0.area()));
        let mut again = m0.clone();
        offset(&mut again, f, d, false).unwrap();
        prop_assert_eq!(again, m);
    }
}

fn edges_of(m: &SurfaceModel) -> Vec<EdgeId> {
    m.edges().map(|(id, _)| id).collect()
}

fn has_point(m: &SurfaceModel, p: P) -> bool {
    m.vertices().any(|(_, q)| q.distance(DVec3::from(p)) < 1e-9)
}

#[test]
fn an_open_chain_of_lines_offsets_to_the_side_of_the_point() {
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddEdges {
        edges: vec![
            [[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]],
            [[1000.0, 0.0, 0.0], [1000.0, 1000.0, 0.0]],
        ],
    })
    .unwrap();
    let es = edges_of(&m);
    let m0 = m.clone();
    m.apply(&Edit::OffsetEdges {
        edges: es.clone(),
        distance: 100.0,
        toward: [500.0, 500.0, 0.0],
    })
    .unwrap();
    assert_eq!(m.edge_count(), 4);
    for p in [[0.0, 100.0, 0.0], [900.0, 100.0, 0.0], [900.0, 1000.0, 0.0]] {
        assert!(has_point(&m, p), "{p:?}");
    }
    // The other side.
    let mut m = m0.clone();
    m.apply(&Edit::OffsetEdges {
        edges: es.clone(),
        distance: 100.0,
        toward: [1500.0, -500.0, 0.0],
    })
    .unwrap();
    assert!(has_point(&m, [1100.0, -100.0, 0.0]));
    // Replays by position.
    let entry = m.log().last().unwrap().clone();
    let mut c = m0.clone();
    super::sync::replay_intent(&mut c, &entry.intent).unwrap();
    assert_eq!(c.edge_count(), 4);
}

#[test]
fn a_line_across_a_face_offsets_in_the_face_and_splits_it() {
    let mut m = SurfaceModel::new();
    poly(&mut m, &square(0.0, 0.0, 1000.0, 0.0));
    m.apply(&Edit::AddEdges {
        edges: vec![[[500.0, 0.0, 0.0], [500.0, 1000.0, 0.0]]],
    })
    .unwrap();
    assert_eq!(m.face_count(), 2);
    let line = m
        .edges()
        .find(|(_, e)| {
            let (a, b) = (m.vertex(e.v[0]).unwrap(), m.vertex(e.v[1]).unwrap());
            a.x == 500.0 && b.x == 500.0
        })
        .unwrap()
        .0;
    m.apply(&Edit::OffsetEdges {
        edges: vec![line],
        distance: 200.0,
        toward: [900.0, 500.0, 0.0],
    })
    .unwrap();
    assert_eq!(m.face_count(), 3);
    assert!(close(m.area(), 1.0e6));
    assert!(has_point(&m, [700.0, 0.0, 0.0]));
}

#[test]
fn an_arc_offsets_as_one_curve() {
    let mut m = SurfaceModel::new();
    let arc: Vec<P> = (0..=12)
        .map(|i| {
            let t = std::f64::consts::PI * f64::from(i) / 12.0;
            [1000.0 * t.cos(), 1000.0 * t.sin(), 0.0]
        })
        .collect();
    m.apply(&Edit::AddCurve {
        points: arc,
        closed: false,
    })
    .unwrap();
    let es = edges_of(&m);
    m.apply(&Edit::OffsetEdges {
        edges: es,
        distance: 100.0,
        toward: [0.0, 0.0, 0.0],
    })
    .unwrap();
    let lens: Vec<usize> = m.curves().map(|(_, es)| es.len()).collect();
    assert_eq!(lens, vec![12, 12]);
}

#[test]
fn offset_edges_refuses_what_has_no_offset() {
    let mut m = SurfaceModel::new();
    m.apply(&Edit::AddEdges {
        edges: vec![
            [[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]],
            [[0.0, 0.0, 0.0], [0.0, 1000.0, 0.0]],
            [[0.0, 0.0, 0.0], [0.0, 0.0, 1000.0]],
        ],
    })
    .unwrap();
    let es = edges_of(&m);
    let before = m.clone();
    let e = m
        .apply(&Edit::OffsetEdges {
            edges: es.clone(),
            distance: 10.0,
            toward: [5.0, 5.0, 0.0],
        })
        .unwrap_err();
    assert!(e.to_string().contains("without branches"), "{e}");
    let vertical = es
        .iter()
        .copied()
        .find(|id| {
            let ed = m.edge(*id).unwrap();
            m.vertex(ed.v[1]).unwrap().z > 0.0 || m.vertex(ed.v[0]).unwrap().z > 0.0
        })
        .unwrap();
    let e = m
        .apply(&Edit::OffsetEdges {
            edges: vec![vertical],
            distance: 10.0,
            toward: [5.0, 0.0, 500.0],
        })
        .unwrap_err();
    assert!(e.to_string().contains("vertical"), "{e}");
    let flat = es.iter().copied().find(|id| *id != vertical).unwrap();
    let ed = m.edge(flat).unwrap();
    let mid = (m.vertex(ed.v[0]).unwrap() + m.vertex(ed.v[1]).unwrap()) / 2.0;
    let e = m
        .apply(&Edit::OffsetEdges {
            edges: vec![flat],
            distance: 10.0,
            toward: mid.to_array(),
        })
        .unwrap_err();
    assert!(e.to_string().contains("lies on the edges"), "{e}");
    assert_eq!(m, before);
}

#[test]
fn the_preview_of_an_offset_gives_the_loops_or_a_short_refusal() {
    let (m, f) = lone(&square(0.0, 0.0, 1000.0, 0.0));
    let loops = m.offset_outline(f, 100.0, false).unwrap();
    assert_eq!(loops.len(), 1);
    assert!(
        loops[0]
            .iter()
            .any(|p| p.distance(DVec3::new(100.0, 100.0, 0.0)) < 1e-9)
    );
    // Too large: refused, without the search for the largest that fits.
    let e = m.offset_outline(f, 600.0, false).unwrap_err().to_string();
    assert!(!e.contains("at most"), "{e}");
    let mut c = SurfaceModel::new();
    c.apply(&Edit::AddEdges {
        edges: vec![[[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]]],
    })
    .unwrap();
    let es: Vec<EdgeId> = c.edges().map(|(id, _)| id).collect();
    let (line, closed) = c
        .offset_chain(&es, 50.0, DVec3::new(500.0, 500.0, 0.0))
        .unwrap();
    assert!(!closed);
    assert!(line.iter().all(|p| (p.y - 50.0).abs() < 1e-9));
}
