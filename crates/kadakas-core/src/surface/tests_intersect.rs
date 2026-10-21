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
    // The edges on the line y = 0, z = 1500: exactly the two pieces where the plate has area,
    // from its border to the sides of the hole, and nothing inside the hole.
    let mut on_line = line_edges(&m, |p| p.y.abs() < 1e-6 && (p.z - z).abs() < 1e-6);
    on_line.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(on_line, vec![(0.0, 1000.0), (2000.0, 3000.0)]);
    // The vertical plate is cut into the part above and the part below (each with half of the
    // hole as a notch); the horizontal plate is not split.
    let vertical = m
        .faces()
        .filter(|(_, f)| f.plane.normal.y.abs() > 0.99)
        .count();
    assert_eq!(vertical, 2);
    assert_eq!(m.face_count(), 3);
}

/// The x ranges of the edges whose two ends pass `on`, merged where they meet.
fn line_edges(m: &SurfaceModel, on: impl Fn(DVec3) -> bool) -> Vec<(f64, f64)> {
    let mut spans: Vec<(f64, f64)> = m
        .edges()
        .filter_map(|(_, e)| {
            let (a, b) = (m.vertex(e.v[0]).unwrap(), m.vertex(e.v[1]).unwrap());
            (on(a) && on(b)).then(|| (a.x.min(b.x), a.x.max(b.x)))
        })
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (lo, hi) in spans {
        match out.last_mut() {
            Some(l) if (l.1 - lo).abs() < 1e-6 => l.1 = hi,
            _ => out.push((lo, hi)),
        }
    }
    out
}

/// Puts a line (an edge with no face) into the model as a file or an import can hold it, without
/// the rules of `add_edges` (no weld, no split). Returns its edge.
fn put_line(m: &mut SurfaceModel, a: P, b: P) -> EdgeId {
    let vertex = |m: &mut SurfaceModel, p: P| {
        let p = DVec3::from(p);
        let found = m.vertices().find(|(_, q)| q.distance(p) < 1e-9);
        match found {
            Some((v, _)) => v,
            None => {
                let v = VertexId(m.alloc());
                m.vertices.insert(v, p);
                v
            }
        }
    };
    let (va, vb) = (vertex(m, a), vertex(m, b));
    let e = EdgeId(m.alloc());
    m.edges.insert(e, Edge { v: [va, vb] });
    e
}

#[test]
fn a_crossing_left_out_splits_no_edge_and_stays_a_warning() {
    // A plate z = 0 with two lines 8 µm apart across the place where a wall stands through it:
    // the piece between the lines would be shorter than the tolerance.
    let mut m = SurfaceModel::new();
    add(&mut m, square(0.0, 0.0, 3000.0, 0.0));
    put_line(&mut m, [1500.0, 1200.0, 0.0], [1500.0, 1800.0, 0.0]);
    put_line(&mut m, [1500.008, 1200.0, 0.0], [1500.008, 1800.0, 0.0]);
    let rect = [
        [1000.0, 1500.0, -500.0],
        [2000.0, 1500.0, -500.0],
        [2000.0, 1500.0, 500.0],
        [1000.0, 1500.0, 500.0],
    ];
    add(
        &mut m,
        (0..4).map(|i| [rect[i], rect[(i + 1) % 4]]).collect(),
    );
    let r = m.check();
    assert!(r.is_valid(), "{:?}", r.problems);
    // The two verticals of the wall pass through the plate, and the two lines through the wall.
    assert_eq!(r.not_intersected(), 4);
    let (vertices, edges) = (m.vertex_count(), m.edge_count());
    let notes = m.apply_with_notes(&model_scope()).unwrap();
    // Only a line (no face) is split where it passes through the wall: the first one. The second
    // one passes within the tolerance of that split and stays whole. The piece of the crossing is
    // not drawn: no hidden T-vertex on the verticals, and their warnings stay.
    assert_eq!(
        (m.vertex_count(), m.edge_count()),
        (vertices + 1, edges + 1)
    );
    for x in [1000.0, 2000.0] {
        let p = DVec3::new(x, 1500.0, 0.0);
        assert!(
            m.vertices().all(|(_, q)| q.distance(p) > 1.0),
            "a vertex at {p:?}"
        );
    }
    let r = m.check();
    assert!(r.is_valid(), "{:?}", r.problems);
    assert_eq!(r.not_intersected(), 3);
    for w in &r.warnings {
        let Problem::NotIntersected(_, _, at) = w else {
            continue;
        };
        let x = at.point().x;
        assert!([1000.0, 2000.0, 1500.008].contains(&x), "{w}");
    }
    assert_eq!(
        notes
            .iter()
            .filter(|n| n.contains("it is split there"))
            .count(),
        1,
        "{notes:?}"
    );
    assert!(
        notes.iter().any(|n| n.contains("was left out")),
        "{notes:?}"
    );
    assert_eq!(
        notes.iter().filter(|n| n.contains("still passes")).count(),
        3,
        "{notes:?}"
    );
}

#[test]
fn a_shallow_crossing_far_from_the_origin_is_intersected() {
    // Survey coordinates (650 km): a vertical plate in the plane x = X0, and a face that is
    // turned 1e-7 radians from it. They cross on the line x = X0, y = Y0 for 1 m (z 0 to 1000).
    let (x0, y0) = (6.5e8, 6.4e8);
    let mut m = SurfaceModel::new();
    let quad =
        |pts: [P; 4]| -> Vec<[P; 2]> { (0..4).map(|i| [pts[i], pts[(i + 1) % 4]]).collect() };
    add(
        &mut m,
        quad([
            [x0, y0 - 3e6, 0.0],
            [x0, y0 + 3e6, 0.0],
            [x0, y0 + 3e6, 1000.0],
            [x0, y0 - 3e6, 1000.0],
        ]),
    );
    let tilt = |y: f64| x0 + (y - y0) * 1e-7;
    add(
        &mut m,
        quad([
            [tilt(y0 - 5e6), y0 - 5e6, -500.0],
            [tilt(y0 + 5e6), y0 + 5e6, -500.0],
            [tilt(y0 + 5e6), y0 + 5e6, 1500.0],
            [tilt(y0 - 5e6), y0 - 5e6, 1500.0],
        ]),
    );
    assert_eq!(m.face_count(), 2);
    assert_eq!(not_intersected(&m).len(), 2, "{:?}", not_intersected(&m));
    intersect_all(&mut m);
    // One edge on the line, from the bottom to the top of the plate.
    let tol = tolerance_at(x0);
    let on = |p: DVec3| (p.x - x0).abs() < tol && (p.y - y0).abs() < tol;
    let ends: Vec<(f64, f64)> = m
        .edges()
        .filter_map(|(_, e)| {
            let (a, b) = (m.vertex(e.v[0]).unwrap(), m.vertex(e.v[1]).unwrap());
            (on(a) && on(b)).then(|| (a.z.min(b.z), a.z.max(b.z)))
        })
        .collect();
    assert_eq!(ends.len(), 1, "{ends:?}");
    assert!(
        ends[0].0.abs() < tol && (ends[0].1 - 1000.0).abs() < tol,
        "{ends:?}"
    );
    // The plate is cut in two; the turned face only gets the line.
    assert_eq!(m.face_count(), 3);
}

#[test]
fn a_crossing_that_ends_on_a_border_makes_one_edge_and_splits_the_border() {
    // A square z = 0 (0 to 1000), and a vertical square in the plane y = 500 from x 500 to 1500:
    // the crossing runs from inside the first square to its border at x = 1000.
    let mut m = SurfaceModel::new();
    add(&mut m, square(0.0, 0.0, 1000.0, 0.0));
    let rect = [
        [500.0, 500.0, -500.0],
        [1500.0, 500.0, -500.0],
        [1500.0, 500.0, 500.0],
        [500.0, 500.0, 500.0],
    ];
    add(
        &mut m,
        (0..4).map(|i| [rect[i], rect[(i + 1) % 4]]).collect(),
    );
    assert_eq!(not_intersected(&m).len(), 2);
    let edges = m.edge_count();
    intersect_all(&mut m);
    // One new edge, and two splits: the border x = 1000 (a T-junction) and the vertical x = 500.
    assert_eq!(m.edge_count(), edges + 3);
    let line = line_edges(&m, |p| (p.y - 500.0).abs() < 1e-6 && p.z.abs() < 1e-6);
    assert_eq!(line, vec![(500.0, 1000.0)]);
    let t = m
        .vertices()
        .find(|(_, p)| p.distance(DVec3::new(1000.0, 500.0, 0.0)) < 1e-6)
        .map(|(v, _)| v)
        .expect("the border is split");
    let border = m
        .edges()
        .filter(|(_, e)| e.v.contains(&t))
        .filter(|(_, e)| {
            let o = m.vertex(e.other(t).unwrap()).unwrap();
            (o.x - 1000.0).abs() < 1e-6 && o.z.abs() < 1e-6
        })
        .count();
    assert_eq!(border, 2, "the border edge x = 1000 is in two parts");
    // Neither face is cut through: the new edge ends inside both.
    assert_eq!(m.face_count(), 2);
}

#[test]
fn a_crossing_through_a_corner_of_a_hole_welds_at_the_corner() {
    // A vertical plate y = 0 (x and z 0 to 3000) with a diamond hole whose left and right corners
    // lie on the line z = 1500, and a horizontal plate z = 1500 through it.
    let mut m = SurfaceModel::new();
    let ring = |pts: &[P]| -> Vec<[P; 2]> {
        (0..pts.len())
            .map(|i| [pts[i], pts[(i + 1) % pts.len()]])
            .collect()
    };
    add(
        &mut m,
        ring(&[
            [0.0, 0.0, 0.0],
            [3000.0, 0.0, 0.0],
            [3000.0, 0.0, 3000.0],
            [0.0, 0.0, 3000.0],
        ]),
    );
    add(
        &mut m,
        ring(&[
            [1500.0, 0.0, 1000.0],
            [2000.0, 0.0, 1500.0],
            [1500.0, 0.0, 2000.0],
            [1000.0, 0.0, 1500.0],
        ]),
    );
    let inner = m
        .faces()
        .find(|(id, _)| (m.face_area(*id).unwrap() - 5e5).abs() < 1.0)
        .unwrap()
        .0;
    m.apply(&Edit::EraseFaces { faces: vec![inner] }).unwrap();
    let corner = |x: f64| {
        m.vertices()
            .find(|(_, p)| p.distance(DVec3::new(x, 0.0, 1500.0)) < 1e-6)
            .unwrap()
            .0
    };
    let (left, right) = (corner(1000.0), corner(2000.0));
    add(
        &mut m,
        ring(&[
            [-500.0, -500.0, 1500.0],
            [3500.0, -500.0, 1500.0],
            [3500.0, 500.0, 1500.0],
            [-500.0, 500.0, 1500.0],
        ]),
    );
    assert_eq!(not_intersected(&m).len(), 2);
    let vertices = m.vertex_count();
    intersect_all(&mut m);
    // Two pieces, welded at the corners of the hole (no new vertex there).
    let line = line_edges(&m, |p| p.y.abs() < 1e-6 && (p.z - 1500.0).abs() < 1e-6);
    assert_eq!(line, vec![(0.0, 1000.0), (2000.0, 3000.0)]);
    assert_eq!(
        m.vertex_count(),
        vertices + 2,
        "only the two splits of the outer border"
    );
    for v in [left, right] {
        let flat = m
            .edges()
            .filter(|(_, e)| e.v.contains(&v))
            .filter(|(_, e)| {
                let o = m.vertex(e.other(v).unwrap()).unwrap();
                (o.z - 1500.0).abs() < 1e-6
            })
            .count();
        assert_eq!(flat, 1, "the piece ends at the corner {v}");
    }
    // The vertical plate is cut into the part above and the part below.
    assert_eq!(m.face_count(), 3);
}

#[test]
fn a_line_along_the_crossing_does_not_stop_the_faces_from_splitting() {
    // Two faces that cross on y = 0, z = 0 (their borders already meet at the ends of the line),
    // and a line along the crossing that no face uses, as a file can hold it. Drawn by
    // `add_edges`, such a line would split both faces at once, so it is put in directly.
    let mut m = SurfaceModel::new();
    let quad =
        |pts: [P; 4]| -> Vec<[P; 2]> { (0..4).map(|i| [pts[i], pts[(i + 1) % 4]]).collect() };
    add(
        &mut m,
        quad([
            [0.0, -500.0, 0.0],
            [1000.0, -500.0, 0.0],
            [1000.0, 500.0, 0.0],
            [0.0, 500.0, 0.0],
        ]),
    );
    add(
        &mut m,
        quad([
            [0.0, 0.0, -500.0],
            [1000.0, 0.0, -500.0],
            [1000.0, 0.0, 500.0],
            [0.0, 0.0, 500.0],
        ]),
    );
    assert_eq!(m.face_count(), 2);
    let line = put_line(&mut m, [0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]);
    assert!(m.check().is_valid());
    intersect_all(&mut m);
    // Both faces split along the line, which is now their edge.
    assert_eq!(m.face_count(), 4);
    assert_eq!(m.faces_of_edge(line).len(), 4);
}

#[test]
fn a_run_over_the_edge_limit_is_refused_and_changes_nothing() {
    // 3,420 small walls through one plate: each makes a new edge and two splits, more than
    // `MAX_EDIT_EDGES` new edges in one run, though there are fewer pieces than that.
    let mut m = SurfaceModel::new();
    add(&mut m, square(-500.0, -500.0, 19_000.0, 0.0));
    let mut batch = Vec::new();
    for i in 0..60 {
        for j in 0..57 {
            let (x, y) = (f64::from(i) * 300.0, f64::from(j) * 300.0);
            let r = [
                [x, y, -100.0],
                [x + 200.0, y, -100.0],
                [x + 200.0, y, 100.0],
                [x, y, 100.0],
            ];
            batch.extend((0..4).map(|k| [r[k], r[(k + 1) % 4]]));
            if batch.len() >= 9000 {
                add(&mut m, std::mem::take(&mut batch));
            }
        }
    }
    add(&mut m, batch);
    assert!(m.face_count() < MAX_EDIT_EDGES);
    let before = m.clone();
    let e = m.apply(&model_scope()).unwrap_err();
    assert!(
        matches!(
            e,
            SurfaceError::Limit {
                max: MAX_EDIT_EDGES,
                ..
            }
        ),
        "{e}"
    );
    assert!(e.to_string().contains("new edges"), "{e}");
    assert_eq!(m, before);
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
    // The first plane gets a hole and an inner face, the walls of the first box are cut in two;
    // the other box and plane are left as they were.
    assert_eq!(r.not_intersected(), 4);
    assert_eq!(m.face_count(), 6 + 1 + 6 + 1 + 1 + 4);
    let near_first = |m: &SurfaceModel, flat: bool| {
        m.faces()
            .filter(|(id, f)| {
                (f.plane.normal.z.abs() > 0.99) == flat && m.face_bounds(*id).unwrap().1.x <= 1500.0
            })
            .count()
    };
    assert_eq!(
        near_first(&m, true),
        2 + 2,
        "top and bottom, the ring and the inner face"
    );
    assert_eq!(near_first(&m, false), 4 * 2, "each wall in two");
    let far = |m: &SurfaceModel| {
        m.faces()
            .filter(|(id, _)| m.face_bounds(*id).unwrap().0.x >= 4500.0)
            .count()
    };
    assert_eq!(far(&m), 6 + 1, "the second box and plane unchanged");
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
    // The line is in two parts that meet at the face; the face is not split.
    assert_eq!((m.edge_count(), m.face_count()), (6, 1));
    let mid = m
        .vertices()
        .find(|(_, p)| p.distance(DVec3::new(500.0, 500.0, 0.0)) < 1e-6)
        .unwrap()
        .0;
    assert_eq!(m.edges().filter(|(_, e)| e.v.contains(&mid)).count(), 2);
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
                    // A crossing that was left out is still there to see.
                    if notes.iter().any(|n| n.contains("was left out")) {
                        prop_assert!(report.not_intersected() > 0, "{:?}", notes);
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
    // All at once would make about 13,440 new edges, more than `MAX_EDIT_EDGES`: refused.
    assert!(matches!(
        m.clone().apply(&model_scope()),
        Err(SurfaceError::Limit { .. })
    ));
    // So in three runs With Selection: the walls of a third of the boxes each.
    let walls: Vec<FaceId> = m
        .faces()
        .filter(|(_, f)| f.plane.normal.z.abs() < 0.5)
        .filter(|(id, _)| m.face_bounds(*id).unwrap().0.z < 0.0)
        .map(|(id, _)| id)
        .collect();
    let t = std::time::Instant::now();
    let mut notes = Vec::new();
    for part in walls.chunks(walls.len().div_ceil(3)) {
        notes.extend(
            m.apply_with_notes(&Edit::IntersectFaces {
                scope: IntersectScope::Selection,
                faces: part.to_vec(),
                edges: Vec::new(),
            })
            .unwrap(),
        );
    }
    let run_s = t.elapsed().as_secs_f64();
    let t = std::time::Instant::now();
    let again = m.apply_with_notes(&model_scope()).unwrap();
    let again_s = t.elapsed().as_secs_f64();
    let r = m.check();
    eprintln!(
        "intersect: {faces} faces, {warned} warnings before; check {check_s:.3} s; intersect in \
         three runs {run_s:.3} s -> {} faces, {} edges ({notes:?}); a run on the whole model \
         after it {again_s:.3} s ({again:?}); {} warnings left",
        m.face_count(),
        m.edge_count(),
        r.not_intersected()
    );
    assert!(r.is_valid());
    assert_eq!(r.not_intersected(), 0);
}
