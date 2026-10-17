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
    assert_eq!(
        a.open_dead_ends, 0,
        "an overshoot is a stub, not an open end"
    );
    assert!(
        a.dead_ends
            .iter()
            .all(|d| !d.open && (d.length - 50.0).abs() < 1e-9)
    );
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

#[test]
fn two_circles_make_a_lens_and_two_crescents() {
    let c = vec![
        Curve::arc_ccw(v(0.0, 0.0), 500.0, 0.0, PI),
        Curve::arc_ccw(v(0.0, 0.0), 500.0, PI, TAU),
        Curve::arc_ccw(v(600.0, 0.0), 500.0, 0.0, PI),
        Curve::arc_ccw(v(600.0, 0.0), 500.0, PI, TAU),
    ];
    let a = build(&c, 0.5).unwrap();
    assert_eq!(a.faces.len(), 3);
    let total: f64 = a.faces.iter().map(|f| f.area).sum();
    let lens = 2.0 * 500.0 * 500.0 * (300.0f64 / 500.0).acos()
        - 300.0 * 2.0 * (500.0f64 * 500.0 - 300.0 * 300.0).sqrt();
    let union = 2.0 * PI * 500.0 * 500.0 - lens;
    assert!((total - union).abs() < 1e-2, "{total} vs {union}");
}

#[test]
fn arcs_with_a_negative_sweep_and_the_helpers() {
    let arc = Curve::Arc {
        c: v(0.0, 0.0),
        r: 10.0,
        start: PI,
        sweep: -PI,
    };
    assert!(arc.from().distance(v(-10.0, 0.0)) < 1e-9 && arc.to().distance(v(10.0, 0.0)) < 1e-9);
    let (lo, hi) = arc.bounds();
    // From (-10, 0) clockwise over the top to (10, 0).
    assert!(
        lo.y.abs() < 1e-9 && (hi.y - 10.0).abs() < 1e-9,
        "{lo:?} {hi:?}"
    );
    assert!(arc.start_tangent().distance(v(0.0, 1.0)) < 1e-9);
    let (q, t, d) = arc.closest(v(0.0, 15.0));
    assert!(q.distance(v(0.0, 10.0)) < 1e-9 && (t - 0.5).abs() < 1e-9 && (d - 5.0).abs() < 1e-9);
    // The center of the circle: an end is the closest point.
    let (_, _, d) = arc.closest(v(0.0, 0.0));
    assert!((d - 10.0).abs() < 1e-9);
    let part = arc.part(0.25, 0.75);
    assert!((part.length() - 10.0 * PI / 2.0).abs() < 1e-9);
    assert!((Curve::line(v(0.0, 0.0), v(3.0, 4.0)).length() - 5.0).abs() < 1e-12);
    assert!(
        Curve::line(v(0.0, 0.0), v(3.0, 4.0))
            .part(0.0, 0.5)
            .to()
            .distance(v(1.5, 2.0))
            < 1e-12
    );
    assert_eq!(Curve::line(v(1.0, 1.0), v(2.0, 1.0)).curvature(), 0.0);
    assert_eq!(
        Curve::line(v(1.0, 1.0), v(1.0, 1.0)).closest(v(5.0, 1.0)).2,
        4.0
    );
    assert!(BuildError::TooMany("x".into()).to_string() == "x");
    assert!((Curve::arc_ccw(v(0.0, 0.0), 1.0, 0.0, TAU).length() - TAU).abs() < 1e-12);
}

#[test]
fn a_line_that_touches_an_arc_and_a_branching_dead_end() {
    // A line tangent to the arc at its top: a node, no face.
    let c = vec![
        Curve::arc_ccw(v(0.0, 0.0), 500.0, 0.0, PI),
        Curve::line(v(-100.0, 500.0), v(100.0, 500.0)),
    ];
    let a = build(&c, 0.5).unwrap();
    assert!(a.faces.is_empty());
    // A square with a dead branch that forks.
    let mut c = rect_lines(0.0, 0.0, 1000.0, 1000.0);
    c.push(Curve::line(v(0.0, 500.0), v(-200.0, 500.0)));
    c.push(Curve::line(v(-100.0, 500.0), v(-100.0, 800.0)));
    let a = build(&c, 0.5).unwrap();
    assert_eq!(a.faces.len(), 1);
    assert_eq!(a.open_dead_ends, 0);
    assert_eq!(a.dead_end_count, 2);
    // Lines that nearly meet at a corner meet where they cross.
    let c = vec![
        Curve::line(v(0.0, 0.0), v(1000.0, 0.4)),
        Curve::line(v(1000.3, 0.0), v(1000.0, 1000.0)),
        Curve::line(v(1000.0, 1000.0), v(0.0, 1000.0)),
        Curve::line(v(0.0, 1000.0), v(0.0, 0.0)),
    ];
    let a = build(&c, 1.0).unwrap();
    assert_eq!(a.faces.len(), 1);
    // Nearly parallel lines that nearly meet use the mean of the ends.
    let c = vec![
        Curve::line(v(0.0, 0.0), v(1000.0, 0.0)),
        Curve::line(v(1000.5, 0.0), v(2000.0, 0.001)),
    ];
    let a = build(&c, 1.0).unwrap();
    assert_eq!(a.closed_gap_count, 1);
}

#[test]
fn a_small_face_far_from_the_origin_keeps_its_area() {
    // Survey coordinates in mm: 650 km east, 6,588 km north.
    let (x, y) = (650_000_000.0, 6_588_000_000.0);
    let a = build(&rect_lines(x, y, x + 100.0, y + 100.0), 1.0).unwrap();
    assert_eq!(a.faces.len(), 1);
    assert!(
        (a.faces[0].area - 10_000.0).abs() < 1e-3,
        "{}",
        a.faces[0].area
    );
}

#[test]
fn an_arc_that_nearly_closes_on_itself_is_a_face() {
    // 359.9 degrees of a circle of 500 mm: the ends are 0.87 mm apart.
    let c = vec![Curve::arc_ccw(
        v(0.0, 0.0),
        500.0,
        0.0,
        359.9f64.to_radians(),
    )];
    let a = build(&c, 1.0).unwrap();
    assert_eq!(a.faces.len(), 1, "{a:?}");
    assert!((a.faces[0].area - PI * 500.0 * 500.0).abs() < 600.0);
    assert_eq!(a.closed_gap_count, 1);
    assert_eq!(a.ignored_short, 0);
    assert!(a.lost_curves.iter().all(|l| !l));
}

#[test]
fn an_end_near_the_middle_and_near_the_end_of_a_line_joins() {
    // The right side stops 0.9 mm short of the bottom line, 0.6 mm before its end.
    let c = vec![
        Curve::line(v(0.0, 0.0), v(1000.0, 0.0)),
        Curve::line(v(999.4, 0.9), v(999.4, 1000.0)),
        Curve::line(v(999.4, 1000.0), v(0.0, 1000.0)),
        Curve::line(v(0.0, 1000.0), v(0.0, 0.0)),
    ];
    let a = build(&c, 1.0).unwrap();
    assert_eq!(a.faces.len(), 1, "{a:?}");
    assert_eq!(a.open_dead_ends, 0);
}

#[test]
fn near_coincident_arcs_are_one_and_islands_that_touch_are_enclosed() {
    let c = vec![
        Curve::through(v(0.0, 0.0), v(500.0, 100.0), v(1000.0, 0.0)),
        Curve::through(v(0.0, 0.0), v(500.0, 100.6), v(1000.0, 0.0)),
    ];
    let a = build(&c, 1.0).unwrap();
    assert_eq!(a.duplicates, 1);
    // A lot, an island in it, and a stall line from the island to the outline.
    let mut c = rect_lines(0.0, 0.0, 1000.0, 1000.0);
    c.extend(rect_lines(400.0, 400.0, 600.0, 600.0));
    c.push(Curve::line(v(500.0, 600.0), v(500.0, 1000.0)));
    let a = build(&c, 1.0).unwrap();
    let island = a
        .faces
        .iter()
        .find(|f| (f.area - 40_000.0).abs() < 1.0)
        .unwrap();
    assert!(island.enclosed && island.depth == 0);
    let lot = a.faces.iter().find(|f| f.area > 900_000.0).unwrap();
    assert!(!lot.enclosed);
    // A lot split by a line is not enclosed.
    let mut c = rect_lines(0.0, 0.0, 1000.0, 1000.0);
    c.push(Curve::line(v(500.0, 0.0), v(500.0, 1000.0)));
    let a = build(&c, 1.0).unwrap();
    assert!(a.faces.iter().all(|f| !f.enclosed));
}

#[test]
fn directions_at_the_seam_and_many_crossings() {
    // Four lines leave one node, two of them pointing to -x (angle +pi and -pi within noise).
    let c = vec![
        Curve::line(v(0.0, 0.0), v(-1000.0, 1e-7)),
        Curve::line(v(0.0, 0.0), v(-1000.0, -1e-7)),
        Curve::line(v(-1000.0, 1e-7), v(-1000.0, -1e-7)),
    ];
    assert!(build(&c, 1e-3).is_ok());
    // Lines that all cross each other: a bound on the crossings is an error, not memory.
    let n = 1500;
    let many: Vec<Curve> = (0..n)
        .map(|k| {
            let t = k as f64;
            Curve::line(v(t, 0.0), v(t * 0.37, 1.0e6 + t))
        })
        .collect();
    let r = build(&many, 0.1);
    assert!(r.is_ok() || matches!(r, Err(BuildError::TooMany(_))));
}

#[test]
fn a_closed_loop_in_a_drawing_is_found_only_when_nothing_else_touches_its_outline() {
    // Two lots side by side, drawn as separate squares: both touch the outside.
    let mut c = rect_lines(0.0, 0.0, 1000.0, 1000.0);
    c.extend(rect_lines(1000.0, 0.0, 2000.0, 1000.0));
    let a = build(&c, 1.0).unwrap();
    assert_eq!(a.faces.len(), 2);
    assert!(a.faces.iter().all(|f| !f.closed_loop));
    // A lot with a divider: the halves share curves with the lot.
    let mut c = rect_lines(0.0, 0.0, 1000.0, 1000.0);
    c.push(Curve::line(v(500.0, 0.0), v(500.0, 1000.0)));
    let a = build(&c, 1.0).unwrap();
    assert!(a.faces.iter().all(|f| !f.closed_loop));
    // A square island joined to the lot by two lines that end on it: one loop.
    let mut c = rect_lines(0.0, 0.0, 4000.0, 2000.0);
    c.extend(rect_lines(1500.0, 750.0, 2500.0, 1250.0));
    c.push(Curve::line(v(0.0, 1000.0), v(1500.0, 1000.0)));
    c.push(Curve::line(v(2500.0, 1000.0), v(4000.0, 1000.0)));
    let a = build(&c, 1.0).unwrap();
    let loops: Vec<_> = a.faces.iter().filter(|f| f.closed_loop).collect();
    assert_eq!(
        loops.len(),
        1,
        "{:?}",
        a.faces.iter().map(|f| f.area).collect::<Vec<_>>()
    );
    assert!((loops[0].area - 500_000.0).abs() < 1e-6);
}

#[test]
fn the_sliver_width_is_capped_and_dropped_faces_are_listed() {
    // A long thin triangle: 10 m long and `w` mm wide at the far end.
    let thin = |w: f64| {
        vec![
            Curve::line(v(0.0, 0.0), v(10_000.0, 0.0)),
            Curve::line(v(0.0, 0.0), v(10_000.0, w)),
            Curve::line(v(10_000.0, 0.0), v(10_000.0, w)),
        ]
    };
    // 6 mm wide at the far end is a real face at a tolerance of 5 mm, although 0.5 * 5 mm is
    // more than its mean width of 3 mm.
    let a = build(&thin(6.0), 5.0).unwrap();
    assert_eq!(a.faces.len(), 1);
    assert_eq!(a.dropped_slivers, 0);
    // 1.5 mm wide at the far end (0.75 mm on the mean) is a sliver at a tolerance of 1 mm.
    let a = build(&thin(1.5), 1.0).unwrap();
    assert_eq!(a.dropped_slivers, 1);
    assert_eq!(a.dropped_sliver_list.len(), 1);
    let s = a.dropped_sliver_list[0];
    assert!(
        (s.area - 7_500.0).abs() < 1.0 && (s.at.x - 6_666.0).abs() < 10.0,
        "{s:?}"
    );
}

#[test]
fn a_broken_outline_with_overshooting_corners_is_open_and_its_gap_is_listed() {
    // A survey outline (file 07a of the messy corpus): each edge overshoots its corners by 400 mm,
    // and the bottom edge is two lines with a gap of 3 mm, at survey coordinates. The pieces of
    // the bottom edge end at the corners, where the overshoots meet them, so a check that only
    // followed each chain to its first junction called the two sides of the gap harmless stubs
    // and the drawing complete.
    let o = v(650_000.0, 6_588_000.0);
    let (w, h, over) = (30_000.0, 20_000.0, 400.0);
    let c = vec![
        Curve::line(o + v(-over, 0.0), o + v(12_000.0, 0.0)),
        Curve::line(o + v(12_003.0, 0.0), o + v(w + over, 0.0)),
        Curve::line(o + v(w, -over), o + v(w, h + over)),
        Curve::line(o + v(w + over, h), o + v(-over, h)),
        Curve::line(o + v(0.0, h + over), o + v(0.0, -over)),
    ];
    let a = build(&c, 2.0).unwrap();
    assert!(a.faces.is_empty(), "{:?}", a.faces);
    assert!(a.open_dead_ends > 0, "{:?}", a.dead_ends);
    let gap_ends = a
        .dead_ends
        .iter()
        .filter(|d| d.at.distance(o + v(12_001.5, 0.0)) < 2.0)
        .collect::<Vec<_>>();
    assert_eq!(gap_ends.len(), 2);
    assert!(gap_ends.iter().all(|d| d.open), "{gap_ends:?}");
    assert_eq!(a.open_gap_count, 1, "{:?}", a.open_gaps);
    let g = &a.open_gaps[0];
    assert!((g.size - 3.0).abs() < 1e-6, "{g:?}");
    // The overshoots at the corners are no gap: their ends are as far apart as they are long.
    // With a tolerance of the gap the outline closes, and the overshoots are stubs again.
    let a = build(&c, 3.0).unwrap();
    assert_eq!(a.faces.len(), 1);
    assert_eq!(a.open_dead_ends, 0, "{:?}", a.dead_ends);
    assert_eq!(a.dead_end_count, 8);
    assert_eq!(a.open_gap_count, 0);
}

#[test]
fn a_line_that_stops_short_of_a_curve_is_an_open_gap_and_an_acute_overshoot_is_not() {
    // A divider that stops 4 mm above the bottom edge would cut the square in two.
    let mut c = rect_lines(0.0, 0.0, 1000.0, 1000.0);
    c.push(Curve::line(v(500.0, 1000.0), v(500.0, 4.0)));
    let a = build(&c, 2.0).unwrap();
    assert_eq!(a.faces.len(), 1);
    assert_eq!(a.open_dead_ends, 1, "{:?}", a.dead_ends);
    assert_eq!(a.open_gap_count, 1);
    let g = &a.open_gaps[0];
    assert!(g.to_curve && (g.size - 4.0).abs() < 1e-9, "{g:?}");
    assert!(g.b.distance(v(500.0, 0.0)) < 1e-9, "{g:?}");
    let a = build(&c, 4.0).unwrap();
    assert_eq!(a.faces.len(), 2);
    assert_eq!((a.dead_end_count, a.open_gap_count), (0, 0));
    // A corner of 20 degrees where both lines overshoot by 300 mm: the tip of one overshoot is
    // near the overshoot of the other line, but both are stubs at a closed shape.
    let t = 20_f64.to_radians();
    let tip = v(2000.0 * t.cos(), 2000.0 * t.sin());
    let u = tip.normalize();
    let c = vec![
        Curve::line(v(-300.0, 0.0), v(2000.0, 0.0)),
        Curve::line(v(2000.0, 0.0), tip),
        Curve::line(tip, -u * 300.0),
    ];
    let a = build(&c, 1.0).unwrap();
    assert_eq!(a.faces.len(), 1);
    assert_eq!(a.dead_end_count, 2);
    assert_eq!(
        (a.open_dead_ends, a.open_gap_count),
        (0, 0),
        "{:?}",
        a.open_gaps
    );
}
