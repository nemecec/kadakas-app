use super::*;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn rect_lines(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Curve> {
    vec![
        Curve::line(v(x0, y0), v(x1, y0)),
        Curve::line(v(x1, y0), v(x1, y1)),
        Curve::line(v(x1, y1), v(x0, y1)),
        Curve::line(v(x0, y1), v(x0, y0)),
    ]
}

#[test]
fn a_square_of_four_lines_is_one_face() {
    let a = build(&rect_lines(0.0, 0.0, 1000.0, 500.0), 1.0).unwrap();
    assert_eq!(a.faces.len(), 1);
    assert!((a.faces[0].area - 500_000.0).abs() < 1e-6);
    assert_eq!(a.faces[0].depth, 0);
    assert_eq!(a.dead_end_count, 0);
    assert_eq!(a.closed_gap_count, 0);
}

#[test]
fn small_gaps_are_closed_and_reported() {
    // Each corner is open by 0.6 mm.
    let g = 0.6;
    let c = vec![
        Curve::line(v(g, 0.0), v(1000.0, 0.0)),
        Curve::line(v(1000.0, g), v(1000.0, 500.0)),
        Curve::line(v(1000.0 - g, 500.0), v(0.0, 500.0)),
        Curve::line(v(0.0, 500.0 - g), v(0.0, 0.0)),
    ];
    let a = build(&c, 1.0).unwrap();
    assert_eq!(a.faces.len(), 1, "{a:?}");
    assert_eq!(a.closed_gap_count, 4);
    for gap in &a.closed_gaps {
        assert!((gap.size - 0.6).abs() < 1e-6, "{gap:?}");
    }
    // The corners move by half a gap: the area changes by far less than a tenth of a percent.
    assert!((a.faces[0].area - 500_000.0).abs() < 500.0);
}

#[test]
fn a_gap_over_the_tolerance_stays_open_and_is_reported() {
    let c = vec![
        Curve::line(v(0.0, 0.0), v(1000.0, 0.0)),
        Curve::line(v(1000.0, 0.0), v(1000.0, 500.0)),
        Curve::line(v(1000.0, 500.0), v(0.0, 500.0)),
        Curve::line(v(0.0, 490.0), v(0.0, 0.0)),
    ];
    let a = build(&c, 2.0).unwrap();
    assert!(a.faces.is_empty());
    assert_eq!(a.dead_end_count, 2);
    assert_eq!(a.open_dead_ends, 2);
    assert!(a.dead_ends.iter().all(|d| d.open));
    let near = a.dead_ends[0].nearest.unwrap();
    assert!((near.1 - 10.0).abs() < 1e-9, "{near:?}");
    let ends: Vec<DVec2> = a.dead_ends.iter().map(|d| d.at).collect();
    assert!(ends.contains(&v(0.0, 500.0)) && ends.contains(&v(0.0, 490.0)));
}

#[test]
fn an_island_is_a_hole_and_a_face_of_its_own() {
    let mut c = rect_lines(0.0, 0.0, 1000.0, 1000.0);
    c.extend(rect_lines(400.0, 400.0, 600.0, 600.0));
    let a = build(&c, 1.0).unwrap();
    assert_eq!(a.faces.len(), 2);
    let lot = a.faces.iter().find(|f| f.depth == 0).unwrap();
    let island = a.faces.iter().find(|f| f.depth == 1).unwrap();
    assert_eq!(lot.holes.len(), 1);
    assert!((lot.area - (1_000_000.0 - 40_000.0)).abs() < 1e-6);
    assert!((island.area - 40_000.0).abs() < 1e-6);
}

#[test]
fn an_island_in_an_island_is_deeper() {
    let mut c = rect_lines(0.0, 0.0, 1000.0, 1000.0);
    c.extend(rect_lines(100.0, 100.0, 900.0, 900.0));
    c.extend(rect_lines(300.0, 300.0, 500.0, 500.0));
    let a = build(&c, 1.0).unwrap();
    let mut depths: Vec<usize> = a.faces.iter().map(|f| f.depth).collect();
    depths.sort();
    assert_eq!(depths, vec![0, 1, 2]);
}

#[test]
fn faces_that_share_an_edge_use_the_same_nodes() {
    let mut c = rect_lines(0.0, 0.0, 1000.0, 1000.0);
    // A line across the middle, drawn longer than the box (it overshoots on both sides).
    c.push(Curve::line(v(-50.0, 400.0), v(1050.0, 400.0)));
    let a = build(&c, 1.0).unwrap();
    assert_eq!(a.faces.len(), 2);
    let total: f64 = a.faces.iter().map(|f| f.area).sum();
    assert!((total - 1_000_000.0).abs() < 1e-6);
    assert_eq!(a.dead_end_count, 2, "the two overshoots are reported");
    assert_eq!(a.open_dead_ends, 0, "an overshoot is a stub, not an open end");
    assert!(a.dead_ends.iter().all(|d| !d.open && (d.length - 50.0).abs() < 1e-9));
    // The shared edge appears once in each face, walked in opposite directions.
    let shared: Vec<_> = a.faces[0]
        .outer
        .edges
        .iter()
        .filter(|(e, f)| a.faces[1].outer.edges.contains(&(*e, !*f)))
        .collect();
    assert_eq!(shared.len(), 1);
}

#[test]
fn a_t_junction_with_a_small_gap_is_joined() {
    let mut c = rect_lines(0.0, 0.0, 1000.0, 1000.0);
    // The divider stops 0.8 mm short of the top and 0.5 mm over the bottom edge.
    c.push(Curve::line(v(500.0, 0.5), v(500.0, 999.2)));
    let a = build(&c, 1.0).unwrap();
    assert_eq!(a.faces.len(), 2, "{a:?}");
    assert_eq!(a.closed_gap_count, 2);
    let total: f64 = a.faces.iter().map(|f| f.area).sum();
    assert!((total - 1_000_000.0).abs() < 1000.0);
}

#[test]
fn arcs_are_exact() {
    // A disc of radius 500 from two half circles, cut by a diameter.
    let c0 = v(0.0, 0.0);
    let c = vec![
        Curve::arc_ccw(c0, 500.0, 0.0, PI),
        Curve::arc_ccw(c0, 500.0, PI, TAU),
        Curve::line(v(-500.0, 0.0), v(500.0, 0.0)),
    ];
    let a = build(&c, 0.5).unwrap();
    assert_eq!(a.faces.len(), 2);
    for f in &a.faces {
        assert!(
            (f.area - PI * 500.0 * 500.0 / 2.0).abs() < 1e-3,
            "{}",
            f.area
        );
    }
}

#[test]
fn a_line_crossing_an_arc_splits_it() {
    let c = vec![
        Curve::arc_ccw(v(0.0, 0.0), 500.0, 0.0, PI),
        Curve::line(v(-500.0, 0.0), v(500.0, 0.0)),
        // A chord with two legs down to the diameter.
        Curve::line(v(-300.0, 100.0), v(300.0, 100.0)),
        Curve::line(v(-300.0, 100.0), v(-300.0, 0.0)),
        Curve::line(v(300.0, 100.0), v(300.0, 0.0)),
    ];
    let a = build(&c, 0.5).unwrap();
    // The half disc outside the box, and the box under the chord.
    assert_eq!(a.faces.len(), 2, "{a:?}");
    let total: f64 = a.faces.iter().map(|f| f.area).sum();
    assert!((total - PI * 500.0 * 500.0 / 2.0).abs() < 1e-3);
}

#[test]
fn duplicate_lines_are_merged() {
    let mut c = rect_lines(0.0, 0.0, 10.0, 10.0);
    c.extend(rect_lines(0.0, 0.0, 10.0, 10.0));
    let a = build(&c, 0.1).unwrap();
    assert_eq!(a.faces.len(), 1);
    assert_eq!(a.duplicates, 4);
}

#[test]
fn tiny_curves_are_counted() {
    let mut c = rect_lines(0.0, 0.0, 100.0, 100.0);
    c.push(Curve::line(v(50.0, 50.0), v(50.1, 50.0)));
    let a = build(&c, 1.0).unwrap();
    assert_eq!(a.ignored_short, 1);
    assert_eq!(a.faces.len(), 1);
}

#[test]
fn too_many_curves_are_refused() {
    let c = vec![Curve::line(v(0.0, 0.0), v(1.0, 0.0)); MAX_CURVES + 1];
    assert!(build(&c, 1.0).is_err());
}

#[test]
fn the_inside_point_is_inside_and_outside_holes() {
    let mut c = rect_lines(0.0, 0.0, 1000.0, 1000.0);
    c.extend(rect_lines(100.0, 100.0, 900.0, 900.0));
    let a = build(&c, 1.0).unwrap();
    let lot = a.faces.iter().find(|f| f.depth == 0).unwrap();
    let p = lot.inside;
    let in_island = p.x > 100.0 && p.x < 900.0 && p.y > 100.0 && p.y < 900.0;
    assert!(!in_island && p.x > 0.0 && p.x < 1000.0 && p.y > 0.0 && p.y < 1000.0);
    let island = a.faces.iter().find(|f| f.depth == 1).unwrap();
    assert!(island.inside.x > 100.0 && island.inside.x < 900.0);
}

#[test]
fn curve_helpers() {
    let arc = Curve::through(v(1.0, 0.0), v(0.0, 1.0), v(-1.0, 0.0));
    assert!(matches!(arc, Curve::Arc { .. }));
    assert!((arc.length() - PI).abs() < 1e-9);
    assert!(matches!(
        Curve::through(v(0.0, 0.0), v(1.0, 0.0), v(2.0, 0.0)),
        Curve::Line { .. }
    ));
    let (lo, hi) = arc.bounds();
    assert!((lo.x + 1.0).abs() < 1e-9 && (hi.y - 1.0).abs() < 1e-9 && lo.y.abs() < 1e-9);
    let back = arc.reversed();
    assert!(back.from().distance(arc.to()) < 1e-9);
    assert!(arc.start_tangent().distance(v(0.0, 1.0)) < 1e-9);
    assert!(back.curvature() < 0.0 && arc.curvature() > 0.0);
    let (q, t, d) = arc.closest(v(0.0, 3.0));
    assert!(q.distance(v(0.0, 1.0)) < 1e-9 && (t - 0.5).abs() < 1e-9 && (d - 2.0).abs() < 1e-9);
    // A point behind the arc end: the closest point is an end.
    let (q, _, _) = arc.closest(v(0.0, -3.0));
    assert!(q.distance(v(1.0, 0.0)) < 1e-9 || q.distance(v(-1.0, 0.0)) < 1e-9);
    assert!(arc.flatten(0.01).len() > 10);
}
