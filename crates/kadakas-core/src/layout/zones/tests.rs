use proptest::prelude::*;

use super::*;
use crate::layout::{Region, shapes_overlap};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn rect(w: f64, h: f64) -> Region {
    Region::from_rings(&[v(0.0, 0.0), v(w, 0.0), v(w, h), v(0.0, h)], &[]).unwrap()
}

#[test]
fn the_last_bits_of_a_node_are_cleaned() {
    assert_eq!(snap(403.99999999999994), 404.0);
    assert_eq!(snap(7500.000000000001), 7500.0);
    // A real difference stays.
    assert_eq!(snap(404.0000001), 404.0000001);
    assert_eq!(snap(-0.25), -0.25);
    // Lines in feet, converted to mm, cross an edge at x = 404 with the noise of the last bits:
    // every vertex of the left edge of the zones is exactly 404.
    let edge = 404.0;
    let r = Region::from_rings(
        &[
            v(edge, 404.0),
            v(30_000.0, 404.0),
            v(30_000.0, 20_000.0),
            v(edge, 20_000.0),
        ],
        &[],
    )
    .unwrap();
    let ft = |x: f64| x / 304.8 * 304.8;
    let lines = [
        vec![v(ft(0.0), ft(5000.0)), v(ft(30_000.0), ft(5000.0))],
        vec![v(ft(2500.0), ft(0.0)), v(ft(2500.0), ft(5000.0))],
    ];
    let s = split(&r, &lines, 3.0).unwrap();
    assert_eq!(s.zones.len(), 3);
    for z in &s.zones {
        for p in &z.shape[0] {
            assert!(p.x == snap(p.x) && p.y == snap(p.y), "{p:?}");
        }
    }
    let left = s
        .zones
        .iter()
        .find(|z| z.lo.x < 405.0 && z.hi.y < 6000.0)
        .unwrap();
    assert!(
        left.shape[0]
            .iter()
            .filter(|p| p.x < 1000.0)
            .all(|p| p.x == edge),
        "{:?}",
        left.shape[0]
    );
}

#[test]
fn one_line_makes_two_zones_that_add_up() {
    let r = rect(10_000.0, 5_000.0);
    let s = split(&r, &[vec![v(4000.0, -100.0), v(4000.0, 5100.0)]], 1.0).unwrap();
    assert_eq!(s.zones.len(), 2);
    let areas: Vec<f64> = s.zones.iter().map(|z| z.area).collect();
    assert_eq!(areas, vec![4000.0 * 5000.0, 6000.0 * 5000.0]);
    assert_eq!(s.overshoot, 2);
    assert!(s.loose_ends.is_empty() && s.unused_lines.is_empty());
}

#[test]
fn two_crossing_lines_make_four_zones() {
    let r = rect(10_000.0, 6_000.0);
    let s = split(
        &r,
        &[
            vec![v(5000.0, 0.0), v(5000.0, 6000.0)],
            vec![v(0.0, 2000.0), v(10_000.0, 2000.0)],
        ],
        1.0,
    )
    .unwrap();
    assert_eq!(s.zones.len(), 4);
    let sum: f64 = s.zones.iter().map(|z| z.area).sum();
    assert_eq!(sum, r.area);
    assert_eq!(s.overshoot, 0);
}

#[test]
fn a_line_that_stops_short_is_reported() {
    let r = rect(10_000.0, 5_000.0);
    // The line stops 40 mm before the top edge: it cuts nothing, and it says where and how far.
    let s = split(&r, &[vec![v(4000.0, 0.0), v(4000.0, 4960.0)]], 1.0).unwrap();
    assert_eq!(s.zones.len(), 1);
    assert_eq!(s.loose_ends.len(), 1);
    let e = &s.loose_ends[0];
    assert_eq!((e.line, e.at), (0, v(4000.0, 4960.0)));
    assert!(e.inside && (e.gap - 40.0).abs() < 1e-9);
}

#[test]
fn an_end_within_the_tolerance_joins_the_edge_and_the_region_does_not_move() {
    let r = rect(10_000.0, 5_000.0);
    let s = split(&r, &[vec![v(4000.0, 0.4), v(4000.0, 4999.7)]], 1.0).unwrap();
    assert_eq!(s.zones.len(), 2);
    assert_eq!(s.closed_gap_count, 2);
    let sum: f64 = s.zones.iter().map(|z| z.area).sum();
    assert_eq!(sum, r.area);
    // The new points are on the edges of the region.
    assert!(s.zones.iter().all(|z| z.shape[0].iter().all(|p| p.y == 0.0
        || p.y == 5000.0
        || p.x == 0.0
        || p.x == 10_000.0
        || p.x == 4000.0)));
}

#[test]
fn a_hole_stays_a_hole_and_a_line_through_it_cuts_the_zones() {
    let hole = vec![
        v(4000.0, 2000.0),
        v(6000.0, 2000.0),
        v(6000.0, 3000.0),
        v(4000.0, 3000.0),
    ];
    let outer = vec![
        v(0.0, 0.0),
        v(10_000.0, 0.0),
        v(10_000.0, 5_000.0),
        v(0.0, 5_000.0),
    ];
    let r = Region::from_rings(&outer, &[hole]).unwrap();
    let s = split(&r, &[vec![v(5000.0, 0.0), v(5000.0, 5000.0)]], 1.0).unwrap();
    assert_eq!(s.zones.len(), 2);
    let sum: f64 = s.zones.iter().map(|z| z.area).sum();
    assert!((sum - r.area).abs() < 1e-6);
    assert!(s.zones.iter().all(|z| z.shape.len() == 1));
}

#[test]
fn an_island_that_no_line_touches_stays_a_hole_of_its_zone() {
    let hole = vec![
        v(1000.0, 1000.0),
        v(2000.0, 1000.0),
        v(2000.0, 2000.0),
        v(1000.0, 2000.0),
    ];
    let outer = vec![
        v(0.0, 0.0),
        v(10_000.0, 0.0),
        v(10_000.0, 5_000.0),
        v(0.0, 5_000.0),
    ];
    let r = Region::from_rings(&outer, &[hole]).unwrap();
    let s = split(&r, &[vec![v(5000.0, 0.0), v(5000.0, 5000.0)]], 1.0).unwrap();
    assert_eq!(s.zones.len(), 2);
    let with_hole: Vec<_> = s.zones.iter().filter(|z| z.shape.len() == 2).collect();
    assert_eq!(with_hole.len(), 1);
    let sum: f64 = s.zones.iter().map(|z| z.area).sum();
    assert!((sum - r.area).abs() < 1e-6);
    assert!(region_contains(&r, s.zones[0].inside) && region_contains(&r, s.zones[1].inside));
}

#[test]
fn a_line_outside_the_region_cuts_nothing_and_is_named() {
    let r = rect(1000.0, 1000.0);
    let s = split(&r, &[vec![v(5000.0, 0.0), v(5000.0, 5000.0)]], 1.0).unwrap();
    assert_eq!(s.zones.len(), 1);
    assert_eq!(s.unused_lines, vec![0]);
}

#[test]
fn bounds_and_bad_numbers_are_refused() {
    let r = rect(1000.0, 1000.0);
    let many = vec![vec![v(0.0, 0.0), v(1.0, 1.0)]; MAX_LINES + 1];
    assert!(split(&r, &many, 1.0).is_err());
    assert!(split(&r, &[vec![v(f64::NAN, 0.0), v(1.0, 1.0)]], 1.0).is_err());
    let long = vec![vec![v(0.0, 0.0); MAX_LINE_POINTS + 1]];
    assert!(split(&r, &long, 1.0).is_err());
}

#[test]
fn from_shapes_keeps_the_points() {
    let a = vec![vec![v(0.0, 0.0), v(2.0, 0.0), v(2.0, 2.0), v(0.0, 2.0)]];
    let r = Region::from_shapes(vec![a.clone()]).unwrap();
    assert_eq!(r.shapes[0], a);
    assert_eq!(r.area, 4.0);
    assert!(Region::from_shapes(vec![]).is_err());
    assert!(Region::from_shapes(vec![vec![vec![v(0.0, 0.0), v(1.0, 0.0)]]]).is_err());
    let hole = vec![v(0.0, 0.0), v(2.0, 0.0), v(2.0, 2.0), v(0.0, 2.0)];
    assert!(Region::from_shapes(vec![vec![a[0].clone(), hole]]).is_err());
    assert!(
        Region::from_shapes(vec![vec![vec![
            v(0.0, 0.0),
            v(f64::INFINITY, 0.0),
            v(0.0, 2.0)
        ]]])
        .is_err()
    );
}

proptest! {
    #[test]
    fn zones_cover_the_region_once(
        lines in proptest::collection::vec(
            ((-2000.0f64..12_000.0, -2000.0f64..8_000.0), (-2000.0f64..12_000.0, -2000.0f64..8_000.0)),
            1..6,
        )
    ) {
        let r = rect(10_000.0, 6_000.0);
        let ls: Vec<Vec<DVec2>> = lines.iter().map(|((a, b), (c, d))| vec![v(*a, *b), v(*c, *d)]).collect();
        let s = split(&r, &ls, 1.0).unwrap();
        let sum: f64 = s.zones.iter().map(|z| z.area).sum();
        prop_assert!((sum - r.area).abs() <= 1e-6 * r.area, "sum {} area {}", sum, r.area);
        // No two zones overlap.
        for i in 0..s.zones.len() {
            for j in i + 1..s.zones.len() {
                let o = shapes_overlap(&s.zones[i].shape, &s.zones[j].shape);
                prop_assert!(o < 1e-3, "zones {} and {} overlap by {} mm2", i, j, o);
            }
        }
        // Every zone has a point inside the region.
        for z in &s.zones {
            prop_assert!(region_contains(&r, z.inside));
        }
    }

    #[test]
    fn zones_are_stable_when_everything_moves(dx in -5.0e6f64..5.0e6, dy in -5.0e6f64..5.0e6) {
        let shift = v(dx, dy);
        let base = rect(10_000.0, 6_000.0);
        let moved = Region::from_rings(
            &[v(0.0, 0.0) + shift, v(10_000.0, 0.0) + shift, v(10_000.0, 6_000.0) + shift, v(0.0, 6_000.0) + shift],
            &[],
        ).unwrap();
        let line = vec![v(3000.0, -10.0), v(7000.0, 6010.0)];
        let line2: Vec<DVec2> = line.iter().map(|p| *p + shift).collect();
        let a = split(&base, &[line], 1.0).unwrap();
        let b = split(&moved, &[line2], 1.0).unwrap();
        prop_assert_eq!(a.zones.len(), b.zones.len());
        let mut aa: Vec<f64> = a.zones.iter().map(|z| z.area).collect();
        let mut bb: Vec<f64> = b.zones.iter().map(|z| z.area).collect();
        aa.sort_by(f64::total_cmp);
        bb.sort_by(f64::total_cmp);
        for (x, y) in aa.iter().zip(&bb) {
            prop_assert!((x - y).abs() < 1.0, "{} vs {}", x, y);
        }
    }
}
