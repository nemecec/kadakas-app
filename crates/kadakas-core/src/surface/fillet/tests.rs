//! Unit tests of the planner, on shapes made here (no surface model).

use std::f64::consts::PI;

use super::*;

/// A prism: the outline `base` (counterclockwise seen from above, at z = 0) and the same outline
/// at `h`, moved by `shift` and scaled by `scale` about its first point (a frustum when the scale
/// is below 1). Vertices 1..=n at the bottom, n+1..=2n at the top; faces from 1000.
pub(super) fn prism(base: &[[f64; 2]], h: f64, scale: f64) -> Shape {
    let n = base.len();
    let c: DVec2 = base.iter().map(|p| DVec2::from(*p)).sum::<DVec2>() / n as f64;
    let mut s = Shape::default();
    for (i, p) in base.iter().enumerate() {
        let q = DVec2::from(*p);
        let top = c + (q - c) * scale;
        s.points
            .insert(VertexId(1 + i as u32), DVec3::new(q.x, q.y, 0.0));
        s.points
            .insert(VertexId(1 + (n + i) as u32), DVec3::new(top.x, top.y, h));
    }
    let bottom: Vec<VertexId> = (0..n).rev().map(|i| VertexId(1 + i as u32)).collect();
    let top: Vec<VertexId> = (0..n).map(|i| VertexId(1 + (n + i) as u32)).collect();
    let add = |s: &mut Shape, id: u32, l: Vec<VertexId>| {
        let pts: Vec<DVec3> = l.iter().map(|v| s.points[v]).collect();
        let plane = Plane::new(pts[0], geom::newell(&pts)).unwrap();
        s.faces.insert(
            FaceId(id),
            ShapeFace {
                plane,
                loops: vec![l],
            },
        );
    };
    add(&mut s, 1000, bottom);
    add(&mut s, 1001, top);
    for i in 0..n {
        let j = (i + 1) % n;
        add(
            &mut s,
            1002 + i as u32,
            vec![
                VertexId(1 + i as u32),
                VertexId(1 + j as u32),
                VertexId(1 + (n + j) as u32),
                VertexId(1 + (n + i) as u32),
            ],
        );
    }
    s
}

pub(super) fn cuboid(a: f64, b: f64, c: f64) -> Shape {
    prism(&[[0.0, 0.0], [a, 0.0], [a, b], [0.0, b]], c, 1.0)
}

/// All edges of the shape as (id, a, b), with ids from 5000 in key order.
fn edges_of(s: &Shape) -> Vec<(EdgeId, VertexId, VertexId)> {
    let mut keys: BTreeSet<(VertexId, VertexId)> = BTreeSet::new();
    for f in s.faces.values() {
        for l in &f.loops {
            for i in 0..l.len() {
                keys.insert(key(l[i], l[(i + 1) % l.len()]));
            }
        }
    }
    keys.into_iter()
        .enumerate()
        .map(|(i, (a, b))| (EdgeId(5000 + i as u32), a, b))
        .collect()
}

fn edge(s: &Shape, a: u32, b: u32) -> (EdgeId, VertexId, VertexId) {
    *edges_of(s)
        .iter()
        .find(|e| key(e.1, e.2) == key(VertexId(a), VertexId(b)))
        .expect("an edge of the shape")
}

/// The faces of the result: the faces that stay, the changed ones and the new ones.
fn result_faces(s: &Shape, p: &Plan) -> Vec<Vec<Vec<DVec3>>> {
    let pos = |x: &Pt| match x {
        Pt::Old(v) => s.points[v],
        Pt::New(i) => p.points[*i],
    };
    let mut out = Vec::new();
    for (id, f) in &s.faces {
        let loops: Vec<Vec<DVec3>> = match p.faces.get(id) {
            Some(ls) => ls.iter().map(|l| l.iter().map(pos).collect()).collect(),
            None => f
                .loops
                .iter()
                .map(|l| l.iter().map(|v| s.points[v]).collect())
                .collect(),
        };
        out.push(loops);
    }
    for f in &p.new_faces {
        out.push(vec![f.points.iter().map(pos).collect()]);
    }
    out
}

/// Checks that every edge of the result is used twice, once each way, and that every face is
/// flat; returns the volume.
fn closed_volume(s: &Shape, p: &Plan) -> f64 {
    let mut uses: BTreeMap<(Pt, Pt), i32> = BTreeMap::new();
    let ptof = |id: &FaceId, f: &ShapeFace| -> Vec<Vec<Pt>> {
        p.faces.get(id).cloned().unwrap_or_else(|| {
            f.loops
                .iter()
                .map(|l| l.iter().map(|v| Pt::Old(*v)).collect())
                .collect()
        })
    };
    let mut all: Vec<Vec<Pt>> = Vec::new();
    for (id, f) in &s.faces {
        all.extend(ptof(id, f));
    }
    for f in &p.new_faces {
        all.push(f.points.clone());
    }
    for l in &all {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            assert_ne!(a, b, "a loop repeats a point");
            *uses.entry(pair(a, b)).or_default() += if a < b { 1 } else { -1 };
        }
    }
    let mut count: BTreeMap<(Pt, Pt), usize> = BTreeMap::new();
    for l in &all {
        for i in 0..l.len() {
            *count.entry(pair(l[i], l[(i + 1) % l.len()])).or_default() += 1;
        }
    }
    for (e, n) in &count {
        assert_eq!(*n, 2, "edge {e:?} is used {n} times");
        assert_eq!(uses[e], 0, "edge {e:?} is used twice the same way");
    }
    let mut v = 0.0;
    for loops in result_faces(s, p) {
        let n = geom::newell(&loops[0]).normalize();
        for l in &loops {
            for q in l {
                assert!(
                    (q - l[0]).dot(n).abs() < 1e-6,
                    "a face is not flat: {} off",
                    (q - l[0]).dot(n)
                );
            }
            v += geom::newell(l).dot(l[0]) / 6.0;
        }
    }
    v
}

fn round(r: f64, n: u32) -> Profile {
    Profile::Round {
        radius: r,
        segments: n,
    }
}

/// The area that a round of `n` segments takes from a right-angled edge (a convex 90 degree
/// edge): the square of the setback less the polygon in the quarter circle.
fn removed_area(r: f64, phi: f64, n: u32) -> f64 {
    let kite = r * r * (phi / 2.0).tan();
    let poly = n as f64 * 0.5 * r * r * (phi / n as f64).sin();
    kite - poly
}

#[test]
fn one_edge_of_a_box_takes_the_exact_prism() {
    let s = cuboid(100.0, 60.0, 40.0);
    // The top front edge: vertices 5 and 6 (y = 0, z = 40).
    let e = edge(&s, 5, 6);
    for n in [1, 2, 6, 24] {
        let p = plan(&s, &[e], &round(10.0, n)).unwrap();
        let v = closed_volume(&s, &p);
        let want = 100.0 * 60.0 * 40.0 - 100.0 * removed_area(10.0, PI / 2.0, n);
        assert!((v - want).abs() < 1e-6, "n {n}: {v} against {want}");
        assert_eq!(p.new_faces.len(), n as usize);
        // The rails and the lines between the segments.
        assert_eq!(p.rails.len(), 2);
        assert_eq!(p.blend.len(), n as usize - 1);
    }
}

#[test]
fn a_chamfer_of_one_edge_takes_a_triangle_prism() {
    let s = cuboid(100.0, 60.0, 40.0);
    let e = edge(&s, 5, 6);
    let p = plan(&s, &[e], &Profile::Flat { distance: 5.0 }).unwrap();
    let v = closed_volume(&s, &p);
    assert!((v - (240_000.0 - 100.0 * 12.5)).abs() < 1e-6);
    assert!(p.blend.is_empty());
}

/// The volume of a box with all edges rounded: the box less the edges and the corners
/// (Steiner), for a true round.
fn rounded_box(a: f64, b: f64, c: f64, r: f64) -> f64 {
    let (a2, b2, c2) = (a - 2.0 * r, b - 2.0 * r, c - 2.0 * r);
    a2 * b2 * c2
        + 2.0 * r * (a2 * b2 + b2 * c2 + a2 * c2)
        + PI * r * r * (a2 + b2 + c2)
        + 4.0 / 3.0 * PI * r * r * r
}

#[test]
fn all_edges_of_a_box_close_with_ball_corners() {
    let s = cuboid(100.0, 60.0, 40.0);
    let all = edges_of(&s);
    let r = 8.0;
    let truth = rounded_box(100.0, 60.0, 40.0, r);
    let mut last = 0.0;
    for n in [1, 2, 4, 8, 16, 32, 64] {
        let p = plan(&s, &all, &round(r, n)).unwrap();
        let v = closed_volume(&s, &p);
        // The polygons lie inside the true round, so they take more.
        assert!(v <= truth + 1e-6, "n {n}: {v} above {truth}");
        assert!(v > last, "more segments keep more: {v} after {last}");
        last = v;
        // Each corner: n^2 triangles.
        let corners = p
            .new_faces
            .iter()
            .filter(|f| matches!(f.role, Role::Corner { .. }))
            .count();
        assert_eq!(corners, 8 * (n * n) as usize);
    }
    assert!(
        (truth - last).abs() / truth < 1e-4,
        "{last} against {truth}"
    );
}

#[test]
fn the_error_of_the_segments_stays_within_the_bound() {
    // Edges: the sagitta areas along the edges; corners: an eighth ball, bounded by its area
    // times the largest gap of a triangle from the ball.
    let (a, b, c, r) = (90.0, 70.0, 50.0, 12.0);
    let s = cuboid(a, b, c);
    let all = edges_of(&s);
    let truth = rounded_box(a, b, c, r);
    for n in [1u32, 3, 6, 12] {
        let p = plan(&s, &all, &round(r, n)).unwrap();
        let v = closed_volume(&s, &p);
        let phi = PI / 2.0;
        let per_len = 0.5 * r * r * (phi - n as f64 * (phi / n as f64).sin());
        let lengths = 4.0 * ((a - 2.0 * r) + (b - 2.0 * r) + (c - 2.0 * r));
        let gap = r * (1.0 - (phi / n as f64).cos());
        let corners = 8.0 * (PI * r * r / 2.0) * gap;
        let bound = per_len * lengths + corners;
        let err = truth - v;
        assert!(
            err >= -1e-6 && err <= bound,
            "n {n}: error {err}, bound {bound}"
        );
    }
}

#[test]
fn three_edges_at_one_corner() {
    let s = cuboid(100.0, 60.0, 40.0);
    // At vertex 7 (100, 60, 40): edges to 6, 8 and 3.
    let es = [edge(&s, 7, 6), edge(&s, 7, 8), edge(&s, 7, 3)];
    let p = plan(&s, &es, &round(10.0, 6)).unwrap();
    let v = closed_volume(&s, &p);
    assert!(v < 240_000.0);
    // Chamfer of the same corner.
    let p = plan(&s, &es, &Profile::Flat { distance: 10.0 }).unwrap();
    closed_volume(&s, &p);
}

#[test]
fn two_edges_meet_as_a_miter_next_to_a_sharp_edge() {
    let s = cuboid(100.0, 60.0, 40.0);
    // The top edges at vertex 7 stay with a sharp vertical edge 7-3.
    let es = [edge(&s, 7, 6), edge(&s, 7, 8)];
    let p = plan(&s, &es, &round(10.0, 6)).unwrap();
    let v = closed_volume(&s, &p);
    // No flat piece: the ends are the same points.
    assert!(
        !p.new_faces
            .iter()
            .any(|f| matches!(f.role, Role::Corner { .. }))
    );
    // The overlap of the two removed regions at the corner, counted twice in the edges.
    let n = 6;
    let a = removed_area(10.0, PI / 2.0, n);
    let lone = 240_000.0 - 60.0 * a - 100.0 * a;
    assert!(v > lone, "{v} must be above {lone}");
    let true_overlap = 1000.0 * (5.0 / 3.0 - PI / 2.0);
    let want_true = 240_000.0 - 160.0 * 10.0 * 10.0 * (1.0 - PI / 4.0) + true_overlap;
    assert!(
        (v - want_true).abs() < 160.0 * 0.5 * 100.0 * (PI / 2.0 - 6.0 * (PI / 12.0).sin()) + 50.0
    );
}

#[test]
fn a_tapered_leg_rounds_its_slanted_edges() {
    // A leg of 50 x 50 at the floor, 30 x 30 at 700 mm.
    let s = prism(
        &[[0.0, 0.0], [50.0, 0.0], [50.0, 50.0], [0.0, 50.0]],
        700.0,
        0.6,
    );
    // The four slanted edges.
    let es: Vec<_> = (1..=4).map(|i| edge(&s, i, i + 4)).collect();
    let p = plan(&s, &es, &round(5.0, 6)).unwrap();
    let v = closed_volume(&s, &p);
    let full = 700.0 / 3.0 * (2500.0 + 900.0 + 1500.0);
    assert!(v < full && v > full * 0.95);
    // The top edges too (two chosen at each top corner, the slanted one sharp).
    let es: Vec<_> = (5..=8)
        .map(|i| edge(&s, i, if i == 8 { 5 } else { i + 1 }))
        .collect();
    let p = plan(&s, &es, &round(5.0, 6)).unwrap();
    closed_volume(&s, &p);
    // And all of them.
    let p = plan(&s, &edges_of(&s), &round(4.0, 4)).unwrap();
    closed_volume(&s, &p);
}

#[test]
fn a_prism_of_many_sides_rounds_its_top_ring() {
    let n = 24;
    let base: Vec<[f64; 2]> = (0..n)
        .map(|i| {
            let a = 2.0 * PI * i as f64 / n as f64;
            [100.0 * a.cos(), 100.0 * a.sin()]
        })
        .collect();
    let s = prism(&base, 80.0, 1.0);
    let top: Vec<_> = (0..n)
        .map(|i| edge(&s, 1 + n + i, 1 + n + (i + 1) % n))
        .collect();
    let p = plan(&s, &top, &round(10.0, 6)).unwrap();
    closed_volume(&s, &p);
    // The rings meet as miters: no corner pieces.
    assert!(
        !p.new_faces
            .iter()
            .any(|f| matches!(f.role, Role::Corner { .. }))
    );
    // All edges: the side edges have a small angle (15 degrees).
    let p = plan(&s, &edges_of(&s), &round(10.0, 4)).unwrap();
    closed_volume(&s, &p);
}

#[test]
fn a_radius_that_does_not_fit_a_thin_arm_is_refused_with_the_largest_that_fits() {
    // A plate 17 mm thick: both long top edges with a radius of 10 meet in the middle.
    let s = cuboid(200.0, 17.0, 60.0);
    let es = [edge(&s, 5, 6), edge(&s, 7, 8)];
    let err = plan(&s, &es, &round(10.0, 6)).unwrap_err();
    assert!(matches!(err, FilletError::DoesNotFit { .. }), "{err}");
    let most = largest_fit(&s, &es, &round(10.0, 6), 10.0);
    assert!(most > 8.4 && most < 8.5, "{most}");
    assert!(plan(&s, &es, &round(most, 6)).is_ok());
}

#[test]
fn an_edge_too_short_for_its_ends_is_refused() {
    let s = cuboid(100.0, 60.0, 15.0);
    // All edges with a radius of 8: the vertical edges (15 mm) are shorter than 2 x 8.
    let err = plan(&s, &edges_of(&s), &round(8.0, 4)).unwrap_err();
    assert!(matches!(err, FilletError::DoesNotFit { .. }), "{err}");
    let most = largest_fit(&s, &edges_of(&s), &round(8.0, 4), 8.0);
    assert!(most > 7.4 && most <= 7.5, "{most}");
}

#[test]
fn bad_inputs_are_refused() {
    let s = cuboid(100.0, 60.0, 40.0);
    let e = edge(&s, 5, 6);
    assert!(matches!(
        plan(&s, &[], &round(5.0, 4)),
        Err(FilletError::NoEdges)
    ));
    assert!(matches!(
        plan(&s, &[e], &round(0.0, 4)),
        Err(FilletError::BadSize { .. })
    ));
    assert!(matches!(
        plan(&s, &[e], &round(f64::NAN, 4)),
        Err(FilletError::BadSize { .. })
    ));
    assert!(matches!(
        plan(&s, &[e], &round(5.0, 0)),
        Err(FilletError::Segments { .. })
    ));
    assert!(matches!(
        plan(&s, &[e], &round(0.05, 64)),
        Err(FilletError::TooSmall { .. })
    ));
}

#[test]
fn a_concave_edge_adds_material() {
    // An L outline: the inner corner at (40, 40).
    let s = prism(
        &[
            [0.0, 0.0],
            [100.0, 0.0],
            [100.0, 40.0],
            [40.0, 40.0],
            [40.0, 100.0],
            [0.0, 100.0],
        ],
        50.0,
        1.0,
    );
    let inner = edge(&s, 4, 10);
    let before = closed_volume(&s, &Plan::default());
    for n in [1u32, 4, 16] {
        let p = plan(&s, &[inner], &round(10.0, n)).unwrap();
        let v = closed_volume(&s, &p);
        let want = before + 50.0 * removed_area(10.0, PI / 2.0, n);
        assert!((v - want).abs() < 1e-6, "{v} against {want}");
    }
    // A convex and a concave edge at one vertex.
    let err = plan(&s, &[inner, edge(&s, 10, 9)], &round(5.0, 4)).unwrap_err();
    assert!(matches!(err, FilletError::Corner { .. }), "{err}");
    // The convex top edges at the inner corner, with the concave edge sharp: they extend past the
    // vertex and meet as a miter.
    let p = plan(&s, &[edge(&s, 10, 9), edge(&s, 10, 11)], &round(5.0, 4)).unwrap();
    let v = closed_volume(&s, &p);
    assert!(v < before);
}

#[test]
fn edges_at_different_angles_get_a_flat_corner_piece() {
    // A wedge-like prism: a triangle outline gives side edges of 120 degrees and top edges of 90.
    let s = prism(&[[0.0, 0.0], [120.0, 0.0], [40.0, 90.0]], 60.0, 1.0);
    // At top vertex 4: the top edge 4-5 and the side edge 1-4 (the other top edge 4-6 sharp).
    let es = [edge(&s, 4, 5), edge(&s, 1, 4)];
    let p = plan(&s, &es, &round(6.0, 4)).unwrap();
    closed_volume(&s, &p);
    let p = plan(&s, &es, &Profile::Flat { distance: 6.0 }).unwrap();
    closed_volume(&s, &p);
}

#[test]
fn a_vertex_of_four_faces_is_refused() {
    // A pyramid with a square base: the apex joins four faces.
    let mut s = Shape::default();
    let pts = [
        [0.0, 0.0, 0.0],
        [100.0, 0.0, 0.0],
        [100.0, 100.0, 0.0],
        [0.0, 100.0, 0.0],
        [50.0, 50.0, 80.0],
    ];
    for (i, p) in pts.iter().enumerate() {
        s.points.insert(VertexId(1 + i as u32), DVec3::from(*p));
    }
    let faces: [&[u32]; 5] = [
        &[1, 4, 3, 2],
        &[1, 2, 5],
        &[2, 3, 5],
        &[3, 4, 5],
        &[4, 1, 5],
    ];
    for (k, f) in faces.iter().enumerate() {
        let l: Vec<VertexId> = f.iter().map(|v| VertexId(*v)).collect();
        let p: Vec<DVec3> = l.iter().map(|v| s.points[v]).collect();
        s.faces.insert(
            FaceId(100 + k as u32),
            ShapeFace {
                plane: Plane::new(p[0], geom::newell(&p)).unwrap(),
                loops: vec![l],
            },
        );
    }
    let err = plan(&s, &[edge(&s, 1, 5)], &round(5.0, 4)).unwrap_err();
    let FilletError::Corner { vertex, why } = err else {
        panic!("{err}");
    };
    assert_eq!(vertex, VertexId(5));
    assert!(why.contains("4 faces"), "{why}");
    // The base edges work: their corners have three faces.
    let p = plan(&s, &[edge(&s, 1, 2)], &round(5.0, 4)).unwrap();
    closed_volume(&s, &p);
}

#[test]
fn default_segments_follow_the_radius_and_the_angle() {
    assert_eq!(default_segments(10.0, PI / 2.0), 6);
    assert_eq!(default_segments(1.0, PI / 2.0), 6);
    assert!(default_segments(500.0, PI / 2.0) > 6);
    assert_eq!(default_segments(5000.0, PI / 2.0), MAX_DEFAULT_SEGMENTS);
    assert_eq!(default_segments(10.0, 0.1), 1);
}
