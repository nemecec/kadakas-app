//! Tests of the `NotIntersected` warning and of Intersect Faces (MOD-08).

use glam::{DMat3, DVec3};
use proptest::prelude::*;

use super::check::Problem;
use super::*;

type P = [f64; 3];

/// The 12 edges of a box.
fn cuboid(lo: P, hi: P) -> Vec<[P; 2]> {
    let c = |i: usize| -> P {
        [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ]
    };
    let mut out = Vec::new();
    for i in 0..8 {
        for bit in [1, 2, 4] {
            if i & bit == 0 {
                out.push([c(i), c(i | bit)]);
            }
        }
    }
    out
}

fn add(m: &mut SurfaceModel, edges: Vec<[P; 2]>) {
    m.apply(&Edit::AddEdges { edges }).unwrap();
}

fn square(x0: f64, y0: f64, w: f64, z: f64) -> Vec<[P; 2]> {
    let p = [
        [x0, y0, z],
        [x0 + w, y0, z],
        [x0 + w, y0 + w, z],
        [x0, y0 + w, z],
    ];
    (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect()
}

fn model_scope() -> Edit {
    Edit::IntersectFaces {
        scope: IntersectScope::Model,
        faces: Vec::new(),
        edges: Vec::new(),
    }
}

fn not_intersected(m: &SurfaceModel) -> Vec<Problem> {
    m.check()
        .warnings
        .into_iter()
        .filter(|p| matches!(p, Problem::NotIntersected(..)))
        .collect()
}

/// Runs Intersect Faces on the whole model and checks the rules: valid before and after, no
/// `NotIntersected` left, the area kept (faces are only split), the same result on a copy, and a
/// second run that changes nothing. Returns the notes.
fn intersect_all(m: &mut SurfaceModel) -> Vec<String> {
    let r = m.check();
    assert!(r.is_valid(), "{:?}", r.problems);
    let area = m.area();
    let mut twin = m.clone();
    let notes = m.apply_with_notes(&model_scope()).unwrap();
    let r = m.check();
    assert!(r.is_valid(), "{:?}", r.problems);
    assert!(m.problem().is_none());
    assert_eq!(r.not_intersected(), 0, "{:?} {notes:?}", r.warnings);
    assert!(
        (m.area() - area).abs() <= 1e-6 * area,
        "{} {area}",
        m.area()
    );
    let twin_notes = twin.apply_with_notes(&model_scope()).unwrap();
    assert_eq!(*m, twin, "deterministic");
    assert_eq!(notes, twin_notes);
    let again = m.clone();
    let second = m.apply_with_notes(&model_scope()).unwrap();
    assert_eq!(*m, again, "a second run changes nothing");
    assert_eq!(
        second,
        vec!["Intersect Faces: nothing to intersect".to_string()]
    );
    notes
}

#[test]
fn an_edge_through_a_face_is_a_warning_with_its_place_and_not_an_error() {
    // The case found by the random edits: a triangle and an edge through it (both orders).
    let tri = [
        [[0.0, 3000.0, 500.0], [0.0, 2000.0, 1000.0]],
        [[0.0, 3000.0, 500.0], [2500.0, 2000.0, 1000.0]],
        [[2500.0, 2000.0, 1000.0], [0.0, 2000.0, 1000.0]],
    ];
    let through = [[0.0, 0.0, 500.0], [500.0, 2500.0, 1000.0]];
    for order in [
        vec![tri[0], tri[1], through, tri[2]],
        vec![tri[0], tri[1], tri[2], through],
    ] {
        let mut m = SurfaceModel::new();
        for e in &order {
            add(&mut m, vec![*e]);
        }
        let r = m.check();
        assert!(r.is_valid(), "{:?}", r.problems);
        assert!(m.problem().is_none(), "the fast check agrees");
        let w = not_intersected(&m);
        assert_eq!(w.len(), 1, "{w:?}");
        let Problem::NotIntersected(_, _, at) = w[0] else {
            unreachable!()
        };
        // The edge meets the plane of the triangle at a known point.
        let p = at.point();
        let f = m.faces().next().unwrap().1;
        assert!(f.plane.distance(p).abs() < 0.01, "{p:?}");
        assert!(w[0].to_string().contains(&at.to_string()), "{}", w[0]);
        assert!(!r.is_closed(), "an open sheet, and not intersected");
        let notes = intersect_all(&mut m);
        assert!(
            notes.iter().any(|n| n.contains("it is split there")),
            "{notes:?}"
        );
        // The line is split in two at the face.
        assert_eq!(m.edge_count(), 5);
    }
}

#[test]
fn a_closed_box_with_an_edge_through_it_is_not_closed() {
    let mut m = SurfaceModel::new();
    add(&mut m, cuboid([0.0; 3], [1000.0; 3]));
    assert!(m.check().is_closed());
    // A line from outside, through the top, to the inside (ends in the air inside the box).
    add(
        &mut m,
        vec![[[500.0, 500.0, 1500.0], [500.0, 400.0, 500.0]]],
    );
    let r = m.check();
    assert!(r.is_valid() && r.not_intersected() == 1, "{r:?}");
    assert!(!r.is_closed());
}

#[test]
fn two_overlapping_boxes_are_intersected() {
    let mut m = SurfaceModel::new();
    add(&mut m, cuboid([0.0; 3], [1000.0; 3]));
    add(&mut m, cuboid([500.0; 3], [1500.0; 3]));
    assert_eq!(m.face_count(), 12);
    // Three edges of each box pass through three faces of the other one.
    assert_eq!(not_intersected(&m).len(), 6, "{:?}", not_intersected(&m));
    intersect_all(&mut m);
    // Each of the six faces that the other box passes through is split in two (an L and a
    // square), and the corner edges are split where they pass through.
    assert_eq!(m.face_count(), 18);
    // Every vertex of the cube where the boxes overlap is there.
    for x in [500.0, 1000.0] {
        for y in [500.0, 1000.0] {
            for z in [500.0, 1000.0] {
                let p = DVec3::new(x, y, z);
                assert!(m.vertices().any(|(_, q)| q.distance(p) < 1e-6), "{p:?}");
            }
        }
    }
}

#[test]
fn a_box_standing_through_a_slab_is_intersected() {
    let mut m = SurfaceModel::new();
    add(&mut m, cuboid([0.0, 0.0, 0.0], [3000.0, 3000.0, 200.0]));
    add(
        &mut m,
        cuboid([1000.0, 1000.0, -500.0], [2000.0, 2000.0, 500.0]),
    );
    // Four corner edges through the top and through the bottom of the slab.
    assert_eq!(not_intersected(&m).len(), 8);
    intersect_all(&mut m);
    // The slab top and bottom get a hole and an inner face each; the four walls of the box are
    // cut into three parts each.
    assert_eq!(m.face_count(), 6 + 2 + 4 * 3 + 2);
    let holed = m.faces().filter(|(_, f)| f.holes.len() == 1).count();
    assert_eq!(holed, 2);
}

#[test]
fn a_plane_through_a_box_is_intersected() {
    let mut m = SurfaceModel::new();
    add(&mut m, cuboid([0.0; 3], [1000.0; 3]));
    add(&mut m, square(-500.0, -500.0, 2000.0, 500.0));
    assert_eq!(not_intersected(&m).len(), 4);
    intersect_all(&mut m);
    // The plane: the ring outside and the face inside the box; each wall in two.
    assert_eq!(m.face_count(), 4 * 2 + 2 + 2);
    let plane_faces = m
        .faces()
        .filter(|(_, f)| f.plane.normal.z.abs() > 0.99 && f.plane.offset.abs() == 500.0)
        .count();
    assert_eq!(plane_faces, 2);
}

#[test]
fn a_hole_is_no_part_of_a_face_that_is_intersected() {
    // A vertical plate with a square hole; a horizontal plate passes through the plate and the
    // hole. The new edges lie only where the vertical plate has area.
    let mut m = SurfaceModel::new();
    let rect =
        |pts: [P; 4]| -> Vec<[P; 2]> { (0..4).map(|i| [pts[i], pts[(i + 1) % 4]]).collect() };
    add(
        &mut m,
        rect([
            [0.0, 0.0, 0.0],
            [3000.0, 0.0, 0.0],
            [3000.0, 0.0, 3000.0],
            [0.0, 0.0, 3000.0],
        ]),
    );
    add(
        &mut m,
        rect([
            [1000.0, 0.0, 1000.0],
            [2000.0, 0.0, 1000.0],
            [2000.0, 0.0, 2000.0],
            [1000.0, 0.0, 2000.0],
        ]),
    );
    // Erase the inner face: the plate keeps the hole.
    let inner = m
        .faces()
        .find(|(id, _)| (m.face_area(*id).unwrap() - 1e6).abs() < 1.0)
        .unwrap()
        .0;
    m.apply(&Edit::EraseFaces { faces: vec![inner] }).unwrap();
    assert_eq!(m.face_count(), 1);
    // The horizontal plate at z = 1500 from y = -500 to 500.
    let z = 1500.0;
    add(
        &mut m,
        rect([
            [-500.0, -500.0, z],
            [3500.0, -500.0, z],
            [3500.0, 500.0, z],
            [-500.0, 500.0, z],
        ]),
    );
    assert_eq!(not_intersected(&m).len(), 4, "{:?}", not_intersected(&m));
    intersect_all(&mut m);
    // No new edge runs inside the hole (1000 < x < 2000 on the line y = 0, z = 1500).
    for (_, e) in m.edges() {
        let (a, b) = (m.vertex(e.v[0]).unwrap(), m.vertex(e.v[1]).unwrap());
        let mid = (a + b) / 2.0;
        let in_hole = mid.y.abs() < 1e-6
            && (mid.z - z).abs() < 1e-6
            && mid.x > 1000.0 + 1e-6
            && mid.x < 2000.0 - 1e-6;
        assert!(!in_hole, "{a:?} {b:?}");
    }
}

#[test]
fn split_edges_keep_their_curve_and_attributes_and_split_faces_keep_theirs() {
    let mut m = SurfaceModel::new();
    // A vertical circle of 12 sides (a curve), through a horizontal plate.
    let points: Vec<P> = (0..12)
        .map(|i| {
            let a = std::f64::consts::TAU * (f64::from(i) + 0.5) / 12.0;
            [500.0 * a.cos(), 0.0, 500.0 * a.sin()]
        })
        .collect();
    m.apply(&Edit::AddCurve {
        points,
        closed: true,
    })
    .unwrap();
    let disc = m.faces().next().unwrap().0;
    let soft = Attrs::from([("soft".to_string(), serde_json::json!(true))]);
    let ring: Vec<EdgeId> = m.curves().next().unwrap().1.clone();
    for e in &ring {
        m.edge_attrs.insert(*e, soft.clone());
    }
    let paint = Attrs::from([("material".to_string(), serde_json::json!("brick"))]);
    m.faces.get_mut(&disc).unwrap().attrs = paint.clone();
    add(&mut m, square(-1000.0, -1000.0, 2000.0, 0.0));
    assert_eq!(not_intersected(&m).len(), 2);
    intersect_all(&mut m);
    assert_eq!(check::curves(&m), None);
    let curves: Vec<_> = m.curves().collect();
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].1.len(), 14, "two sides split, still one chain");
    for e in curves[0].1 {
        assert_eq!(m.edge_attrs(*e), Some(&soft));
    }
    // The disc is split in two halves that both keep the material.
    let painted = m.faces().filter(|(_, f)| f.attrs == paint).count();
    assert_eq!(painted, 2);
}

#[test]
fn intersect_with_selection_only_resolves_the_chosen_faces() {
    let mut m = SurfaceModel::new();
    // Two separate places where a plane passes through a box.
    add(&mut m, cuboid([0.0; 3], [1000.0; 3]));
    add(&mut m, square(-500.0, -500.0, 2000.0, 500.0));
    add(&mut m, cuboid([5000.0, 0.0, 0.0], [6000.0, 1000.0, 1000.0]));
    add(&mut m, square(4500.0, -500.0, 2000.0, 500.0));
    assert_eq!(not_intersected(&m).len(), 8);
    let first_plane = m
        .faces()
        .find(|(_, f)| f.plane.offset.abs() == 500.0 && f.plane.normal.z.abs() > 0.99)
        .map(|(id, _)| id)
        .unwrap();
    let mut twin = m.clone();
    let e = Edit::IntersectFaces {
        scope: IntersectScope::Selection,
        faces: vec![first_plane],
        edges: Vec::new(),
    };
    m.apply(&e).unwrap();
    twin.apply(&e).unwrap();
    assert_eq!(m, twin);
    let r = m.check();
    assert!(r.is_valid(), "{:?}", r.problems);
    // The other box and plane are left as they were.
    assert_eq!(r.not_intersected(), 4);
    for w in &r.warnings {
        if let Problem::NotIntersected(_, _, at) = w {
            assert!(at.point().x > 4000.0, "{w}");
        }
    }
    // A line chosen as an edge is split where it passes through a face.
    let mut m = SurfaceModel::new();
    add(&mut m, square(0.0, 0.0, 1000.0, 0.0));
    add(
        &mut m,
        vec![[[500.0, 500.0, -500.0], [500.0, 500.0, 500.0]]],
    );
    let line = m
        .edges()
        .find(|(id, _)| m.faces_of_edge(*id).is_empty())
        .map(|(id, _)| id)
        .unwrap();
    m.apply(&Edit::IntersectFaces {
        scope: IntersectScope::Selection,
        faces: Vec::new(),
        edges: vec![line],
    })
    .unwrap();
    assert_eq!(m.check().not_intersected(), 0);
    assert_eq!(m.edge_count(), 6);
}

#[test]
fn unknown_ids_are_refused_and_change_nothing() {
    let mut m = SurfaceModel::new();
    add(&mut m, square(0.0, 0.0, 1000.0, 0.0));
    let before = m.clone();
    for e in [
        Edit::IntersectFaces {
            scope: IntersectScope::Selection,
            faces: vec![FaceId(999)],
            edges: Vec::new(),
        },
        Edit::IntersectFaces {
            scope: IntersectScope::Selection,
            faces: Vec::new(),
            edges: vec![EdgeId(999)],
        },
    ] {
        assert!(m.apply(&e).is_err());
        assert_eq!(m, before);
    }
}

#[test]
fn the_edit_reads_from_json_and_replays_from_the_log() {
    let e: Edit =
        serde_json::from_str(r#"{"action": "intersect_faces", "scope": "model"}"#).unwrap();
    assert_eq!(e, model_scope());
    let mut m = SurfaceModel::new();
    add(&mut m, cuboid([0.0; 3], [1000.0; 3]));
    add(&mut m, square(-500.0, -500.0, 2000.0, 500.0));
    let base = m.clone();
    m.apply(&e).unwrap();
    let last = m.log().last().unwrap().intent.clone();
    assert_eq!(last, sync::Intent::IntersectModel);
    let mut other = base.clone();
    sync::replay_intent(&mut other, &last).unwrap();
    assert_eq!(other.check().not_intersected(), 0);
    assert_eq!(other.face_count(), m.face_count());
}

#[test]
fn faces_in_one_plane_that_overlap_stay_an_error() {
    // Coplanar overlap is not a crossing: the check reports it as a problem, as before.
    let a = Face::new(
        Plane::new(DVec3::ZERO, DVec3::Z).unwrap(),
        vec![VertexId(1), VertexId(2), VertexId(3)],
        Vec::new(),
    );
    let mut b = a.clone();
    b.outer = vec![VertexId(1), VertexId(2), VertexId(4)];
    let vs = [
        (1, [0.0, 0.0, 0.0]),
        (2, [1000.0, 0.0, 0.0]),
        (3, [0.0, 1000.0, 0.0]),
        (4, [500.0, 1000.0, 0.0]),
    ]
    .into_iter()
    .map(|(i, p)| (VertexId(i), DVec3::from(p)))
    .collect();
    let es = [(5, 1, 2), (6, 2, 3), (7, 3, 1), (8, 2, 4), (9, 4, 1)]
        .into_iter()
        .map(|(i, a, b)| {
            (
                EdgeId(i),
                Edge {
                    v: [VertexId(a), VertexId(b)],
                },
            )
        })
        .collect();
    let fs = [(FaceId(10), a), (FaceId(11), b)].into_iter().collect();
    let m = SurfaceModel::from_parts(vs, es, fs, 12);
    assert!(!m.check().is_valid());
}

#[derive(Clone, Debug)]
enum Op {
    Add(P, P),
    Box(P, P),
    Erase(usize),
    Intersect,
}

fn grid_point() -> impl Strategy<Value = P> {
    (0..7i32, 0..7i32, 0..3i32).prop_map(|(x, y, z)| {
        [
            f64::from(x) * 500.0,
            f64::from(y) * 500.0,
            f64::from(z) * 500.0,
        ]
    })
}

fn free_point() -> impl Strategy<Value = P> {
    (0.0..3000.0f64, 0.0..3000.0f64, 0.0..1500.0f64).prop_map(|(x, y, z)| [x, y, z])
}

fn op() -> impl Strategy<Value = Op> {
    let size = (1..5i32, 1..5i32, 1..4i32).prop_map(|(x, y, z)| {
        [
            f64::from(x) * 400.0,
            f64::from(y) * 400.0,
            f64::from(z) * 400.0,
        ]
    });
    prop_oneof![
        4 => (grid_point(), grid_point()).prop_map(|(a, b)| Op::Add(a, b)),
        1 => (free_point(), free_point()).prop_map(|(a, b)| Op::Add(a, b)),
        3 => (prop_oneof![grid_point(), free_point()], size).prop_map(|(a, s)| Op::Box(a, s)),
        1 => (0..64usize).prop_map(Op::Erase),
        2 => Just(Op::Intersect),
    ]
}

/// Where a case draws: turned and moved, so that the planes are not along the axes and the
/// tolerance grows with the coordinates (650 km from the origin: 0.65 mm).
fn place() -> impl Strategy<Value = (DMat3, DVec3)> {
    let turn = prop_oneof![
        2 => Just(DMat3::IDENTITY),
        1 => (0.0..6.3f64, 0.0..6.3f64, 0.0..6.3f64)
            .prop_map(|(a, b, c)| DMat3::from_euler(glam::EulerRot::XYZ, a, b, c)),
    ];
    let offset = prop_oneof![
        3 => Just(DVec3::ZERO),
        1 => Just(DVec3::new(6.5e8, 6.4e8, 12_000.0)),
    ];
    (turn, offset)
}

fn run(m: &mut SurfaceModel, op: &Op, at: &(DMat3, DVec3)) -> Result<Vec<String>, SurfaceError> {
    let put = |p: P| (at.0 * DVec3::from(p) + at.1).to_array();
    match op {
        Op::Add(a, b) => m.apply_with_notes(&Edit::AddEdges {
            edges: vec![[put(*a), put(*b)]],
        }),
        Op::Box(lo, s) => {
            let hi = [lo[0] + s[0], lo[1] + s[1], lo[2] + s[2]];
            m.apply_with_notes(&Edit::AddEdges {
                edges: cuboid(*lo, hi).into_iter().map(|e| e.map(put)).collect(),
            })
        }
        Op::Erase(k) => match m.edge_count() {
            0 => Ok(Vec::new()),
            n => {
                let id = m.edges().nth(k % n).unwrap().0;
                m.apply_with_notes(&Edit::EraseEdges { edges: vec![id] })
            }
        },
        Op::Intersect => m.apply_with_notes(&model_scope()),
    }
}

/// True if a vertex, or an edge other than `e`, is within twice the length tolerance of `p`.
fn near_other(m: &SurfaceModel, e: EdgeId, p: DVec3) -> bool {
    let tol = 2.0 * geom::tol(&[p]) + 0.002;
    m.vertices().any(|(_, q)| q.distance(p) <= tol)
        || m.edges().any(|(id, ed)| {
            let (a, b) = (m.vertex(ed.v[0]).unwrap(), m.vertex(ed.v[1]).unwrap());
            id != e && geom::point_segment(p, a, b).0 <= tol
        })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|s| s.parse().ok()).unwrap_or(48),
        ..ProptestConfig::default()
    })]

    /// Random sketches with crossings: every edit keeps the model valid (warnings allowed), the
    /// same edits give the same model, and Intersect Faces leaves no `NotIntersected`, keeps the
    /// area, and changes nothing when it runs again.
    #[test]
    fn intersect_faces_resolves_random_crossings(
        ops in prop::collection::vec(op(), 1..14),
        at in place(),
    ) {
        let mut m = SurfaceModel::new();
        let mut twin = SurfaceModel::new();
        let mut ops = ops;
        ops.push(Op::Intersect);
        for op in &ops {
            let before = m.clone();
            let r = run(&mut m, op, &at);
            let r2 = run(&mut twin, op, &at);
            prop_assert_eq!(&r, &r2);
            prop_assert_eq!(&m, &twin);
            let report = m.check();
            prop_assert!(report.is_valid(), "{:?} after {:?}", report.problems, op);
            prop_assert!(m.problem().is_none());
            match (&r, op) {
                (Err(_), _) => prop_assert_eq!(&m, &before),
                (Ok(notes), Op::Intersect) => {
                    // What stays is within the length tolerance of other geometry: no edge can be
                    // made there.
                    for w in &report.warnings {
                        if let Problem::NotIntersected(e, _, at) = w {
                            prop_assert!(near_other(&m, *e, at.point()), "{} {:?}", w, notes);
                            prop_assert!(notes.iter().any(|n| n.contains("still passes")));
                        }
                    }
                    let (a, b) = (m.area(), before.area());
                    prop_assert!((a - b).abs() <= 1e-6 * b.max(1.0), "area {} -> {}", b, a);
                    let again = m.clone();
                    m.apply(&model_scope()).unwrap();
                    prop_assert_eq!(&m, &again);
                    twin = m.clone();
                }
                _ => {}
            }
        }
    }
}

/// The time of Intersect Faces on a model of about 5,000 faces: 840 boxes that stand through one
/// slab (each corner edge passes through the top and the bottom of the slab).
/// `cargo test -p kadakas-core --release --lib surface::tests_intersect::intersect_timing -- --ignored --nocapture`
#[test]
#[ignore = "timing test: run in release mode"]
fn intersect_timing() {
    let mut m = SurfaceModel::new();
    add(
        &mut m,
        cuboid([-500.0, -500.0, 0.0], [30_500.0, 28_500.0, 200.0]),
    );
    let mut batch = Vec::new();
    for i in 0..30 {
        for j in 0..28 {
            let (x, y) = (f64::from(i) * 1000.0, f64::from(j) * 1000.0);
            batch.extend(cuboid([x, y, -300.0], [x + 600.0, y + 600.0, 500.0]));
            if batch.len() >= 9000 {
                add(&mut m, std::mem::take(&mut batch));
            }
        }
    }
    add(&mut m, batch);
    let faces = m.face_count();
    let t = std::time::Instant::now();
    let warned = m.check().not_intersected();
    let check_s = t.elapsed().as_secs_f64();
    let t = std::time::Instant::now();
    let notes = m.apply_with_notes(&model_scope()).unwrap();
    let run_s = t.elapsed().as_secs_f64();
    let t = std::time::Instant::now();
    let again = m.apply_with_notes(&model_scope()).unwrap();
    let again_s = t.elapsed().as_secs_f64();
    let r = m.check();
    eprintln!(
        "intersect: {faces} faces, {warned} warnings before; check {check_s:.3} s; intersect \
         {run_s:.3} s -> {} faces, {} edges ({notes:?}); second run {again_s:.3} s ({again:?}); \
         {} warnings left",
        m.face_count(),
        m.edge_count(),
        r.not_intersected()
    );
    assert!(r.is_valid());
    assert_eq!(r.not_intersected(), 0);
}
