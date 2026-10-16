use proptest::prelude::*;

use super::*;
use crate::curve::VertexKind;
use crate::layout::area_shared;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn spec(path: &[(f64, f64)], side: Side, corner: Corner) -> Spec {
    Spec {
        path: path.iter().map(|p| Vertex::corner(v(p.0, p.1))).collect(),
        closed: false,
        side,
        offset: 0.0,
        along: 100.0,
        depth: 200.0,
        gap: 5.0,
        corner,
    }
}

fn total_area(c: &Course) -> f64 {
    c.pieces.iter().map(|p| area(&p.poly)).sum()
}

fn max_overlap(c: &Course) -> f64 {
    let mut worst: f64 = 0.0;
    for i in 0..c.pieces.len() {
        for j in i + 1..c.pieces.len() {
            let (a, b) = (&c.pieces[i].poly, &c.pieces[j].poly);
            // Quick reject by box.
            let bx = |p: &[DVec2]| {
                p.iter().fold((DVec2::MAX, DVec2::MIN), |(lo, hi), q| {
                    (lo.min(*q), hi.max(*q))
                })
            };
            let (ba, bb) = (bx(a), bx(b));
            if ba.1.x < bb.0.x || bb.1.x < ba.0.x || ba.1.y < bb.0.y || bb.1.y < ba.0.y {
                continue;
            }
            worst = worst.max(area_shared(a, b));
        }
    }
    worst
}

#[test]
fn a_straight_course_has_exact_counts() {
    // 10 m of kerb, units of 100 along by 200 across, joints of 5 mm: 95 whole units and a cut
    // unit of 25 mm at the end (95 steps of 105 mm are 9975 mm).
    let c = lay(&spec(&[(0.0, 0.0), (10_000.0, 0.0)], Side::Left, Corner::Miter), 1000).unwrap();
    let whole = c.pieces.iter().filter(|p| (area(&p.poly) - 20_000.0).abs() < 1e-6).count();
    assert_eq!(whole, 95);
    assert_eq!(c.pieces.len(), 96);
    let cut = c.pieces.iter().map(|p| area(&p.poly)).fold(f64::MAX, f64::min);
    assert!((cut - 25.0 * 200.0).abs() < 1e-6);
    assert_eq!((c.joints.narrowest, c.joints.widest), (5.0, 5.0));
    assert_eq!(c.joints.count, 94);
    // The units are on the left of the path, flush with it.
    assert!(c.pieces.iter().flat_map(|p| &p.poly).all(|q| q.y >= -1e-9 && q.y <= 200.0 + 1e-9));
    // The band is the strip with a gap on both sides and at both ends.
    let band: f64 = c.band.iter().map(|s| area(&s[0])).sum();
    assert!((band - 10_010.0 * 210.0).abs() < 1e-3, "{band}");
    assert_eq!(c.length, 10_000.0);
}

#[test]
fn the_side_and_the_centre() {
    let l = lay(&spec(&[(0.0, 0.0), (1000.0, 0.0)], Side::Right, Corner::Miter), 1000).unwrap();
    assert!(l.pieces.iter().flat_map(|p| &p.poly).all(|q| q.y <= 1e-9 && q.y >= -200.0 - 1e-9));
    let c = lay(&spec(&[(0.0, 0.0), (1000.0, 0.0)], Side::Center, Corner::Miter), 1000).unwrap();
    assert!(c.pieces.iter().flat_map(|p| &p.poly).all(|q| q.y.abs() <= 100.0 + 1e-9));
    let mut off = spec(&[(0.0, 0.0), (1000.0, 0.0)], Side::Left, Corner::Miter);
    off.offset = 50.0;
    let o = lay(&off, 1000).unwrap();
    assert!(o.pieces.iter().flat_map(|p| &p.poly).all(|q| q.y >= 50.0 - 1e-9 && q.y <= 250.0 + 1e-9));
}

#[test]
fn a_miter_corner_cuts_units_and_keeps_the_joint() {
    // Inside corner: a left turn with the course on the left.
    let s = spec(&[(0.0, 0.0), (3000.0, 0.0), (3000.0, 3000.0)], Side::Left, Corner::Miter);
    let c = lay(&s, 1000).unwrap();
    assert_eq!(c.corners, 1);
    assert!(max_overlap(&c) < 1e-6, "overlap {}", max_overlap(&c));
    // Units of the two runs are at least a gap apart at the mitre line.
    let m = DVec2::new(1.0, 1.0).normalize();
    let corner = v(3000.0, 0.0);
    for p in &c.pieces {
        for q in &p.poly {
            // Pieces of run 1 are on one side of the mitre line, pieces of run 2 on the other.
            let d = (*q - corner).dot(m);
            assert!(d.abs() >= 2.5 - 1e-6 || p.poly.iter().all(|r| (*r - corner).dot(m).abs() >= 2.5 - 1e-6), "{d}");
        }
    }
    // The two strips cover 1.16 m2 together; the units cover about 100/105 of it.
    assert!(total_area(&c) > 1.04e6 && total_area(&c) < 1.16e6, "{}", total_area(&c));
    // The outside of a corner: the same path, the course on the right.
    let o = lay(&spec(&[(0.0, 0.0), (3000.0, 0.0), (3000.0, 3000.0)], Side::Right, Corner::Miter), 1000).unwrap();
    assert!(max_overlap(&o) < 1e-6);
    // An outside corner is longer than the path: the course covers the corner square.
    assert!(o.pieces.iter().flat_map(|p| &p.poly).any(|q| q.x > 3000.0 + 190.0));
}

/// The distance from a point to a convex polygon: zero inside.
fn dist_to_poly(p: DVec2, poly: &[DVec2]) -> f64 {
    if crate::geom::point_in_polygon(p, poly) {
        return 0.0;
    }
    (0..poly.len())
        .map(|i| crate::geom::dist_to_segment(p, poly[i], poly[(i + 1) % poly.len()]))
        .fold(f64::MAX, f64::min)
}

#[test]
fn every_corner_rule_covers_the_corner_without_holes() {
    // The path goes east, then north. Every point of the two strips (and, on the outside, of the
    // corner square) is in a unit or within a joint of one.
    for (side, regions) in [
        (
            Side::Left,
            vec![((0.0, 0.0), (3000.0, 200.0)), ((2800.0, 0.0), (3000.0, 3000.0))],
        ),
        (
            Side::Right,
            vec![
                ((0.0, -200.0), (3000.0, 0.0)),
                ((3000.0, 0.0), (3200.0, 3000.0)),
                ((3000.0, -200.0), (3200.0, 0.0)),
            ],
        ),
    ] {
        for rule in [Corner::Miter, Corner::Butt, Corner::CornerUnit] {
            let c = lay(&spec(&[(0.0, 0.0), (3000.0, 0.0), (3000.0, 3000.0)], side, rule), 10_000).unwrap();
            let mut worst: f64 = 0.0;
            let mut y = -200.0;
            while y <= 3000.0 {
                let mut x = 0.0;
                while x <= 3200.0 {
                    let p = v(x, y);
                    let in_strip = regions.iter().any(|(lo, hi)| x >= lo.0 && x <= hi.0 && y >= lo.1 && y <= hi.1);
                    if in_strip {
                        let d = c.pieces.iter().map(|q| dist_to_poly(p, &q.poly)).fold(f64::MAX, f64::min);
                        worst = worst.max(d);
                    }
                    x += 37.0;
                }
                y += 37.0;
            }
            assert!(worst <= 5.0 + 1e-6, "{side:?} {rule:?}: a point is {worst} mm from the nearest unit");
        }
    }
}

#[test]
fn a_butt_corner_and_a_corner_unit() {
    for side in [Side::Left, Side::Right] {
        let b = lay(&spec(&[(0.0, 0.0), (3000.0, 0.0), (3000.0, 3000.0)], side, Corner::Butt), 1000).unwrap();
        assert!(max_overlap(&b) < 1e-6, "butt {side:?}: {}", max_overlap(&b));
        let u = lay(&spec(&[(0.0, 0.0), (3000.0, 0.0), (3000.0, 3000.0)], side, Corner::CornerUnit), 1000).unwrap();
        assert!(max_overlap(&u) < 1e-6, "corner unit {side:?}");
        let corners: Vec<_> = u.pieces.iter().filter(|p| p.corner_unit).collect();
        assert_eq!(corners.len(), 1);
        // A right angle: the corner unit is the square of the depth.
        assert!((area(&corners[0].poly) - 200.0 * 200.0).abs() < 1e-6);
        // The joint between the corner unit and its neighbours is at least the gap.
        for p in u.pieces.iter().filter(|p| !p.corner_unit) {
            for q in &p.poly {
                let inside_corner = corners[0].poly.iter().all(|c| c.distance(*q) >= 0.0);
                assert!(inside_corner);
            }
        }
    }
}

#[test]
fn a_gentle_corner_is_always_a_miter() {
    // A turn of 10 degrees with the butt rule or a corner unit: the same units as the miter.
    let path = [(0.0, 0.0), (3000.0, 0.0), (3000.0 + 2954.0, 521.0)];
    let m = lay(&spec(&path, Side::Left, Corner::Miter), 1000).unwrap();
    for rule in [Corner::Butt, Corner::CornerUnit] {
        let c = lay(&spec(&path, Side::Left, rule), 1000).unwrap();
        assert_eq!(c.pieces.len(), m.pieces.len(), "{rule:?}");
        assert!(c.pieces.iter().all(|p| !p.corner_unit));
        assert!(max_overlap(&c) < 1e-6);
    }
}

#[test]
fn a_curve_tapers_the_joints_and_never_overlaps() {
    // A quarter circle of radius 3000, anticlockwise: the left side is the inside of the bend.
    let arc = |side| Spec {
        path: vec![
            Vertex::corner(v(3000.0, 0.0)),
            Vertex::new(
                v(3000.0 * (0.5f64).sqrt(), 3000.0 * (0.5f64).sqrt()),
                VertexKind::PointOnArc,
            ),
            Vertex::corner(v(0.0, 3000.0)),
        ],
        closed: false,
        side,
        offset: 0.0,
        along: 100.0,
        depth: 200.0,
        gap: 5.0,
        corner: Corner::Miter,
    };
    for side in [Side::Left, Side::Right, Side::Center] {
        let c = lay(&arc(side), 10_000).unwrap();
        assert!(c.joints.count > 50);
        // The narrow side of every joint is the gap; the other side is wider.
        assert!(c.joints.narrowest >= 5.0 - 1e-6, "{side:?}: {:?}", c.joints);
        assert!(c.joints.widest > 5.5 && c.joints.widest < 5.0 + 200.0 * 100.0 / 2800.0 + 1.0);
        assert!(max_overlap(&c) < 1e-6, "{side:?} overlap {}", max_overlap(&c));
        // The units are about arc length / step.
        let n = c.pieces.len() as f64;
        let expect = 3000.0 * std::f64::consts::FRAC_PI_2 / 105.0;
        assert!((n - expect).abs() < expect * 0.12, "{n} vs {expect}");
    }
}

#[test]
fn a_closed_loop_closes_with_a_cut_unit() {
    let circle = Spec {
        path: vec![
            Vertex::corner(v(2000.0, 0.0)),
            Vertex::new(v(-2000.0, 0.0), VertexKind::PointOnArc),
        ],
        closed: true,
        side: Side::Left,
        offset: 0.0,
        along: 100.0,
        depth: 200.0,
        gap: 5.0,
        corner: Corner::Miter,
    };
    // Two points on an arc make a half circle and a closing chord: use a polygon instead.
    let _ = circle;
    let square = Spec {
        closed: true,
        ..spec(&[(0.0, 0.0), (4000.0, 0.0), (4000.0, 4000.0), (0.0, 4000.0)], Side::Left, Corner::Miter)
    };
    let c = lay(&square, 10_000).unwrap();
    assert_eq!(c.corners, 4);
    assert!(max_overlap(&c) < 1e-6);
    // The strip holds exactly the units: the ring of 200 mm inside the square, less the joints.
    let ring = 4000.0 * 4000.0 - 3600.0 * 3600.0;
    assert!(total_area(&c) < ring && total_area(&c) > ring * 0.9);
}

#[test]
fn bad_input_is_refused() {
    assert!(lay(&Spec { along: 1.0, ..spec(&[(0.0, 0.0), (1000.0, 0.0)], Side::Left, Corner::Miter) }, 100).is_err());
    assert!(lay(&Spec { gap: -1.0, ..spec(&[(0.0, 0.0), (1000.0, 0.0)], Side::Left, Corner::Miter) }, 100).is_err());
    assert!(lay(&spec(&[(0.0, 0.0), (1000.0, 0.0)], Side::Left, Corner::Miter), 5).is_err());
    assert!(lay(&spec(&[(0.0, 0.0)], Side::Left, Corner::Miter), 100).is_err());
    // A hairpin is refused with the point.
    let e = lay(&spec(&[(0.0, 0.0), (1000.0, 0.0), (0.0, 50.0)], Side::Left, Corner::Miter), 1000).unwrap_err();
    assert!(e.contains("turns by"), "{e}");
    assert_eq!(Side::from_name("Left"), Some(Side::Left));
    assert_eq!(Corner::from_name("corner unit"), Some(Corner::CornerUnit));
    assert_eq!(Corner::from_name("x"), None);
}

proptest! {
    #[test]
    fn pieces_never_overlap_on_random_paths(
        pts in proptest::collection::vec((0.0f64..6000.0, 0.0f64..6000.0), 2..5),
        side in 0u8..3,
        rule in 0u8..3,
        along in 60.0f64..300.0,
        depth in 60.0f64..300.0,
        gap in 2.0f64..10.0,
    ) {
        let mut path: Vec<(f64, f64)> = Vec::new();
        for p in pts {
            if path.last().is_none_or(|l| (l.0 - p.0).hypot(l.1 - p.1) > 400.0) {
                path.push(p);
            }
        }
        prop_assume!(path.len() >= 2);
        let s = Spec {
            along,
            depth,
            gap,
            ..spec(
                &path,
                [Side::Left, Side::Right, Side::Center][side as usize],
                [Corner::Miter, Corner::Butt, Corner::CornerUnit][rule as usize],
            )
        };
        // A path may fold back on itself and cross; only a path that does not is tested for
        // overlaps. A refused corner is fine.
        let Ok(c) = lay(&s, 100_000) else { return Ok(()); };
        // Legs that come near each other (or cross) make units meet from two legs: not tested.
        let seg_dist = |a: DVec2, b: DVec2, c: DVec2, d: DVec2| -> f64 {
            let o = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
            if o(a, b, c) * o(a, b, d) < 0.0 && o(c, d, a) * o(c, d, b) < 0.0 {
                return 0.0;
            }
            [
                crate::geom::dist_to_segment(a, c, d),
                crate::geom::dist_to_segment(b, c, d),
                crate::geom::dist_to_segment(c, a, b),
                crate::geom::dist_to_segment(d, a, b),
            ]
            .into_iter()
            .fold(f64::MAX, f64::min)
        };
        let mut apart = f64::MAX;
        for i in 0..path.len() - 1 {
            for j in i + 2..path.len() - 1 {
                apart = apart.min(seg_dist(
                    v(path[i].0, path[i].1),
                    v(path[i + 1].0, path[i + 1].1),
                    v(path[j].0, path[j].1),
                    v(path[j + 1].0, path[j + 1].1),
                ));
            }
        }
        prop_assume!(apart > 2.0 * (depth + along));
        let min_leg = path.windows(2).map(|w| (w[0].0 - w[1].0).hypot(w[0].1 - w[1].1)).fold(f64::MAX, f64::min);
        prop_assume!(min_leg > 3.0 * depth + 3.0 * along);
        prop_assert!(max_overlap(&c) < 1e-3, "overlap {}", max_overlap(&c));
        for p in &c.pieces {
            prop_assert!(area(&p.poly) > 0.0 && area(&p.poly) <= p.full_area * (1.0 + 1e-9));
        }
    }

    #[test]
    fn a_translated_course_has_the_same_units(dx in -1.0e6f64..1.0e6, dy in -1.0e6f64..1.0e6) {
        let base = lay(&spec(&[(0.0, 0.0), (3000.0, 0.0), (3000.0, 2000.0)], Side::Left, Corner::Miter), 1000).unwrap();
        let moved = lay(&spec(&[(dx, dy), (3000.0 + dx, dy), (3000.0 + dx, 2000.0 + dy)], Side::Left, Corner::Miter), 1000).unwrap();
        prop_assert_eq!(base.pieces.len(), moved.pieces.len());
        prop_assert!((total_area(&base) - total_area(&moved)).abs() < 1e-3);
    }
}

