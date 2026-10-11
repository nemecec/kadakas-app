use super::*;
use crate::geom::{polygon_area, signed_area};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn open(pts: &[DVec2], w: f64, j: Join) -> Vec<DVec2> {
    path_outline(pts, w, j, false)
}

/// Area of the points within `h` of the path, without the parts beyond the two ends: the
/// reference for a stroke with round joins and butt caps, found by sampling a grid.
fn swept_reference(pts: &[DVec2], h: f64) -> f64 {
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in pts {
        lo = lo.min(*p - h);
        hi = hi.max(*p + h);
    }
    let n = 500;
    let cell = (hi - lo) / n as f64;
    let mut count = 0usize;
    for i in 0..n {
        for j in 0..n {
            let p = lo + DVec2::new(i as f64 + 0.5, j as f64 + 0.5) * cell;
            // In a rectangle of a leg (inside its ends), or in the round wedge on the outer side of a bend.
            let in_leg = pts.windows(2).any(|w| {
                let ab = w[1] - w[0];
                let t = (p - w[0]).dot(ab) / ab.length_squared();
                (0.0..=1.0).contains(&t) && p.distance(w[0] + ab * t) <= h
            });
            let in_bend = pts.windows(3).any(|w| {
                let (d0, d1) = ((w[1] - w[0]).normalize(), (w[2] - w[1]).normalize());
                let turn = d0.perp_dot(d1);
                let v = p - w[1];
                if v.length() > h || turn.abs() < 1e-12 {
                    return false;
                }
                let side = if turn > 0.0 { -1.0 } else { 1.0 };
                let (n0, n1) = (
                    DVec2::new(-d0.y, d0.x) * side,
                    DVec2::new(-d1.y, d1.x) * side,
                );
                if turn > 0.0 {
                    n0.perp_dot(v) >= 0.0 && v.perp_dot(n1) >= 0.0
                } else {
                    n0.perp_dot(v) <= 0.0 && v.perp_dot(n1) <= 0.0
                }
            });
            if in_leg || in_bend {
                count += 1;
            }
        }
    }
    count as f64 * cell.x * cell.y
}

#[test]
fn straight_path_is_a_rectangle() {
    let r = open(&[v(0.0, 0.0), v(4000.0, 0.0)], 1200.0, Join::Miter);
    assert!((signed_area(&r) - 4000.0 * 1200.0).abs() < 1e-3);
    assert!(r.iter().all(|p| (p.y.abs() - 600.0).abs() < 1e-3));
}

#[test]
fn collinear_points_keep_the_exact_area() {
    let r = open(
        &[v(0.0, 0.0), v(1000.0, 0.0), v(3000.0, 0.0), v(3000.0, 0.0)],
        500.0,
        Join::Round,
    );
    assert!((polygon_area(&r) - 3000.0 * 500.0).abs() < 1e-3);
}

#[test]
fn right_angle_joins() {
    let path = [v(0.0, 0.0), v(3000.0, 0.0), v(3000.0, 3000.0)];
    let m = open(&path, 1000.0, Join::Miter);
    assert!((polygon_area(&m) - 6.0e6).abs() < 1.0);
    assert!(
        m.iter().any(|p| p.distance(v(3500.0, -500.0)) < 1e-2),
        "the outer miter point"
    );
    assert!(
        m.iter().any(|p| p.distance(v(2500.0, 500.0)) < 1e-2),
        "the inner corner"
    );
    let b = open(&path, 1000.0, Join::Bevel);
    assert!(
        (polygon_area(&b) - (6.0e6 - 125_000.0)).abs() < 1.0,
        "the bevel cuts a triangle of 0.5 x 0.5 x 0.5 m"
    );
    let r = open(&path, 1000.0, Join::Round);
    assert!(polygon_area(&r) > polygon_area(&b) && polygon_area(&r) < polygon_area(&m));
}

#[test]
fn right_turn_and_left_turn_are_mirror_images() {
    let left = open(
        &[v(0.0, 0.0), v(3000.0, 0.0), v(3000.0, 3000.0)],
        800.0,
        Join::Round,
    );
    let right = open(
        &[v(0.0, 0.0), v(3000.0, 0.0), v(3000.0, -3000.0)],
        800.0,
        Join::Round,
    );
    assert!((polygon_area(&left) - polygon_area(&right)).abs() < 1.0);
}

#[test]
fn tight_curves_and_short_legs_give_the_swept_area() {
    // A U-turn with a center radius of 300 mm and a width of 1000 mm: the inner side folds over itself.
    let n = 48;
    let mut pts = vec![v(-3000.0, 300.0), v(0.0, 300.0)];
    pts.extend((1..n).map(|k| {
        v(0.0, 0.0)
            + DVec2::from_angle(
                std::f64::consts::FRAC_PI_2 - std::f64::consts::PI * k as f64 / n as f64,
            ) * 300.0
    }));
    pts.extend([v(0.0, -300.0), v(-3000.0, -300.0)]);
    let swept = swept_reference(&pts, 500.0);
    let r = open(&pts, 1000.0, Join::Round);
    assert!(
        (polygon_area(&r) - swept).abs() < 0.02 * swept,
        "{} vs {swept}",
        polygon_area(&r)
    );
    // The outline has no loops: a triangulation of the ring covers exactly its area.
    let tris = crate::mesh::triangulate(&crate::mesh::clean_ring(&r));
    let ring = crate::mesh::clean_ring(&r);
    let covered: f64 = tris
        .iter()
        .map(|t| polygon_area(&[ring[t[0]], ring[t[1]], ring[t[2]]]))
        .sum();
    assert!((covered - polygon_area(&r)).abs() < 1e-3 * covered);
    // Short legs: every leg is shorter than the width.
    let zig = [
        v(0.0, 0.0),
        v(200.0, 100.0),
        v(300.0, -150.0),
        v(520.0, 90.0),
        v(640.0, -60.0),
    ];
    let swept = swept_reference(&zig, 400.0);
    let r = open(&zig, 800.0, Join::Round);
    assert!(
        (polygon_area(&r) - swept).abs() < 0.03 * swept,
        "{} vs {swept}",
        polygon_area(&r)
    );
}

#[test]
fn joins_bracket_the_reference() {
    let pts = [
        v(0.0, 0.0),
        v(2000.0, 0.0),
        v(2300.0, 800.0),
        v(1000.0, 1000.0),
    ];
    let swept = swept_reference(&pts, 400.0);
    let (b, r, m) = (
        open(&pts, 800.0, Join::Bevel),
        open(&pts, 800.0, Join::Round),
        open(&pts, 800.0, Join::Miter),
    );
    assert!(
        polygon_area(&b) <= polygon_area(&r) * 1.001
            && polygon_area(&m) >= polygon_area(&b) * 0.999
    );
    assert!((polygon_area(&r) - swept).abs() < 0.02 * swept);
}

#[test]
fn parts_that_only_touch_do_not_lose_area() {
    // A found case: the union returned the last leg as a second shape, and the area was short.
    let pts = [
        v(0.0, 0.0),
        v(-272.96, -221.61),
        v(-161.66, -317.65),
        v(-98.48, -850.06),
        v(-48.17, -1273.95),
    ];
    let (m, b) = (
        open(&pts, 100.0, Join::Miter),
        open(&pts, 100.0, Join::Bevel),
    );
    assert!(polygon_area(&m) >= polygon_area(&b));
    // The legs add up: the last leg of 426 mm by 100 mm is in the area.
    let first_four = open(&pts[..4], 100.0, Join::Bevel);
    assert!(polygon_area(&b) - polygon_area(&first_four) > 0.9 * 42_600.0 - 3000.0);
}

#[test]
fn closed_paths_have_no_ends_and_a_hole() {
    // A square ring of center side 4000 and width 1000: the area is 4500 x 4500 minus 3500 x 3500.
    let sq = [
        v(0.0, 0.0),
        v(4000.0, 0.0),
        v(4000.0, 4000.0),
        v(0.0, 4000.0),
    ];
    let r = path_outline(&sq, 1000.0, Join::Miter, true);
    let want = 5000.0 * 5000.0 - 3000.0 * 3000.0;
    assert!((signed_area(&r) - want).abs() < 1.0, "{}", signed_area(&r));
    // The same with a repeated first point.
    let mut again = sq.to_vec();
    again.push(sq[0]);
    assert!((signed_area(&path_outline(&again, 1000.0, Join::Miter, true)) - want).abs() < 1.0);
    // The ring can be triangulated to exactly its area.
    let ring = crate::mesh::clean_ring(&r);
    let covered: f64 = crate::mesh::triangulate(&ring)
        .iter()
        .map(|t| polygon_area(&[ring[t[0]], ring[t[1]], ring[t[2]]]))
        .sum();
    assert!((covered - want).abs() < 1e-3 * want, "{covered} vs {want}");
    assert!(
        path_outline(&sq[..2], 1000.0, Join::Miter, true).is_empty(),
        "two points are not a ring"
    );
}

#[test]
fn a_hairpin_and_a_reversal_do_not_break() {
    let path = [v(0.0, 0.0), v(5000.0, 0.0), v(0.0, 300.0)];
    let m = open(&path, 400.0, Join::Miter);
    assert!(
        m.iter().all(|p| p.x < 5000.0 + 4.0 * 400.0),
        "the miter is limited"
    );
    let back = open(
        &[v(0.0, 0.0), v(100.0, 0.0), v(0.0, 0.0)],
        20.0,
        Join::Miter,
    );
    assert!(back.iter().all(|p| p.is_finite()));
}

#[test]
fn bad_input_gives_no_outline() {
    assert!(open(&[v(0.0, 0.0)], 100.0, Join::Miter).is_empty());
    assert!(open(&[v(1.0, 1.0), v(1.0, 1.0)], 100.0, Join::Miter).is_empty());
    assert!(open(&[v(0.0, 0.0), v(1.0, 0.0)], 0.0, Join::Miter).is_empty());
    assert!(open(&[v(0.0, 0.0), v(1.0, 0.0)], f64::NAN, Join::Miter).is_empty());
    assert!(open(&[], 5.0, Join::Round).is_empty());
}

#[test]
fn shifted_lines_and_alignment() {
    let s = shifted(&[v(0.0, 0.0), v(10.0, 0.0)], 2.0);
    assert_eq!(
        s,
        vec![v(0.0, 2.0), v(10.0, 2.0)],
        "positive is to the left"
    );
    assert_eq!(
        shifted(&[v(0.0, 0.0), v(10.0, 0.0)], -2.0),
        vec![v(0.0, -2.0), v(10.0, -2.0)]
    );
    assert_eq!(shifted(&[v(0.0, 0.0), v(10.0, 0.0)], 0.0).len(), 2);
    assert_eq!(shifted(&[v(0.0, 0.0)], 1.0).len(), 1);
    for a in Align::ALL {
        assert_eq!(Align::from_name(a.id()), Some(a));
    }
    assert_eq!(Align::from_name("top"), None);
}

#[test]
fn bridging_joins_pieces_without_losing_area() {
    let square = |x: f64, y: f64| vec![v(x, y), v(x + 1.0, y), v(x + 1.0, y + 1.0), v(x, y + 1.0)];
    // A second piece next to the first.
    let two = bridge(square(0.0, 0.0), square(3.0, 0.0));
    assert!((signed_area(&two) - 2.0).abs() < 1e-5);
    // A hole (clockwise) takes its area away.
    let mut hole = square(0.25, 0.25)
        .into_iter()
        .map(|p| p * 0.5)
        .collect::<Vec<_>>();
    hole.reverse();
    let ring = bridge(square(0.0, 0.0), hole);
    assert!((signed_area(&ring) - (1.0 - 0.25)).abs() < 1e-5);
    // A contour with fewer than three points changes nothing.
    assert_eq!(bridge(square(0.0, 0.0), vec![v(5.0, 5.0)]).len(), 4);
}

#[test]
fn helpers() {
    let (l, r) = sides(&[v(0.0, 0.0), v(10.0, 0.0)], 4.0);
    assert_eq!(
        (l, r),
        (
            vec![v(0.0, 2.0), v(10.0, 2.0)],
            vec![v(0.0, -2.0), v(10.0, -2.0)]
        )
    );
    assert_eq!(sides(&[v(0.0, 0.0)], 4.0), (vec![], vec![]));
    // At a bend the outer edge is mitered, and a sharp bend is cut.
    let (l, r) = sides(&[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0)], 2.0);
    assert!(l.contains(&v(9.0, 1.0)) && r.contains(&v(11.0, -1.0)));
    let (_, r) = sides(&[v(0.0, 0.0), v(100.0, 0.0), v(0.0, 1.0)], 2.0);
    assert!(r.len() >= 4, "a hairpin is cut: {r:?}");
    assert_eq!(dedupe(&[v(0.0, 0.0), v(0.0, 0.0), v(1.0, 0.0)]).len(), 2);
    assert_eq!(length(&[v(0.0, 0.0), v(3.0, 4.0), v(3.0, 5.0)]), 6.0);
    for j in Join::ALL {
        assert_eq!(Join::from_name(j.id()), Some(j));
    }
    assert_eq!(Join::from_name("x"), None);
}

/// The outline has no loop: a triangulation covers exactly its area, and no two edges cross.
fn assert_simple_outline(pts: &[DVec2], width: f64) {
    for j in [Join::Round, Join::Bevel, Join::Miter] {
        let ring = open(pts, width, j);
        let clean = crate::mesh::clean_ring(&ring);
        let covered: f64 = crate::mesh::triangulate(&clean)
            .iter()
            .map(|t| polygon_area(&[clean[t[0]], clean[t[1]], clean[t[2]]]))
            .sum();
        let area = polygon_area(&clean);
        assert!(
            (covered - area).abs() <= 1e-6 * area,
            "{j:?}: {covered} vs {area}"
        );
        let n = ring.len();
        for a in 0..n {
            for b in a + 2..n {
                if a == 0 && b == n - 1 {
                    continue;
                }
                let (p, q) = (ring[a], ring[(a + 1) % n]);
                let (r, s) = (ring[b], ring[(b + 1) % n]);
                // Edges that meet at a bridge share a point.
                let share = |x: DVec2, y: DVec2| x.distance(y) < 1e-3;
                if share(p, r) || share(p, s) || share(q, r) || share(q, s) {
                    continue;
                }
                assert!(!segments_meet(p, q, r, s), "{j:?}: edges {a} and {b} cross");
            }
        }
    }
}

/// Found by the property test: the outline has two holes. The 24 closest pairs for the second
/// bridge were all blocked, and the fallback crossed an edge of the first hole.
#[test]
fn a_second_hole_is_bridged_without_crossing_the_first() {
    assert_simple_outline(
        &[
            v(0.0, 0.0),
            v(2233.5517853766314, 0.0),
            v(1748.3124935721771, -556.6333364192927),
            v(-213.98429311535733, -231.49262687701645),
            v(-383.30611496418226, -2122.1377799708835),
            v(-1862.0873582661125, 128.22311856221177),
            v(1829.1845654195213, 1954.8617892412788),
        ],
        1002.2221588239406,
    );
}

/// Found by the property test: a corner of the second hole lies 6e-8 mm from the line of the
/// bridge to the first hole. The bridge made a thin slit around it, and ear clipping covered the hole.
#[test]
fn a_bridge_keeps_clear_of_the_corner_of_another_hole() {
    assert_simple_outline(
        &[
            v(0.0, 0.0),
            v(20.0, 0.0),
            v(-105.98531597744656, -236.24028388646025),
            v(78.98429658174325, -370.0202842747546),
            v(32.9483137692117, -148.87483533347236),
            v(15.917552316185407, -159.3606907744993),
        ],
        100.0,
    );
}

/// A hostile case: a hole that must be bridged, and a long row of small holes that are still to
/// be joined. Every candidate bridge has to be tested against all of them.
fn hostile() -> (Vec<DVec2>, Vec<DVec2>, Vec<Vec<DVec2>>) {
    let square = |x: f64, y: f64, s: f64| vec![v(x, y), v(x + s, y), v(x + s, y + s), v(x, y + s)];
    let outer = square(0.0, 0.0, 100_000.0);
    let hole = square(40_000.0, 40_000.0, 10.0);
    let others = (0..400)
        .map(|k| square(41_000.0 + 20.0 * k as f64, 40_000.0, 10.0))
        .collect();
    (outer, hole, others)
}

#[test]
fn bridge_work_is_counted_and_never_goes_over_the_budget() {
    let (outer, hole, others) = hostile();
    // A small budget runs out, and the work stays within it.
    let small = Budget::new(1_000);
    bridge_among(outer.clone(), hole.clone(), &others, &small);
    assert!(small.out.get());
    assert!(small.spent(1_000) <= 1_000);
    // The same input with a big budget ends with a clean bridge and spends less than all of it.
    let big = Budget::new(WORK_BUDGET);
    let ring = bridge_among(outer, hole, &others, &big);
    assert!(!big.out.get());
    assert_eq!(ring.len(), 4 + 4 + 2);
    assert!(big.spent(WORK_BUDGET) > 1_000);
}

#[test]
fn a_path_that_needs_too_much_work_has_no_outline() {
    let pts = [
        v(0.0, 0.0),
        v(2233.5517853766314, 0.0),
        v(1748.3124935721771, -556.6333364192927),
        v(-213.98429311535733, -231.49262687701645),
        v(-383.30611496418226, -2122.1377799708835),
        v(-1862.0873582661125, 128.22311856221177),
        v(1829.1845654195213, 1954.8617892412788),
    ];
    assert!(path_outline_within(&pts, 1002.2, Join::Miter, false, 10).is_empty());
    assert!(!path_outline_within(&pts, 1002.2, Join::Miter, false, WORK_BUDGET).is_empty());
}
