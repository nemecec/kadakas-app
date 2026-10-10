//! Property tests of the site model algorithms.

use std::collections::HashSet;

use cadektor_site::*;
use glam::{DVec2, DVec3};
use proptest::prelude::*;

/// Distinct points on an integer grid (so no two are close), with random heights.
fn points(max: usize) -> impl Strategy<Value = Vec<DVec3>> {
    prop::collection::vec((0i32..60, 0i32..60, -500i32..500), 3..max).prop_map(|v| {
        let mut seen = HashSet::new();
        v.into_iter()
            .filter(|(x, y, _)| seen.insert((*x, *y)))
            .map(|(x, y, z)| DVec3::new(x as f64 * 10.0, y as f64 * 10.0, z as f64 / 10.0))
            .collect()
    })
}

fn build(points: Vec<DVec3>) -> Option<Tin> {
    Tin::build(&SourceData {
        points,
        lines: vec![],
    })
    .ok()
}

fn flat(n: i32, step: f64, from: f64, z: f64) -> SourceData {
    let mut points = Vec::new();
    for i in 0..n {
        for j in 0..n {
            points.push(DVec3::new(
                from + i as f64 * step,
                from + j as f64 * step,
                z,
            ));
        }
    }
    SourceData {
        points,
        lines: vec![],
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<DVec2> {
    vec![
        DVec2::new(x0, y0),
        DVec2::new(x1, y0),
        DVec2::new(x1, y1),
        DVec2::new(x0, y1),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn tin_elevation_at_source_points_is_source_z(pts in points(60)) {
        if let Some(tin) = build(pts.clone()) {
            for p in &pts {
                let z = tin.elevation_at(p.truncate());
                prop_assert!(z.is_some(), "point {p:?} not found");
                prop_assert!((z.unwrap() - p.z).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn elevation_stays_between_lowest_and_highest(pts in points(60), fx in 0.0f64..1.0, fy in 0.0f64..1.0) {
        if let Some(tin) = build(pts) {
            let (lo, hi) = tin.bounds();
            let q = lo + (hi - lo) * DVec2::new(fx, fy);
            if let Some(z) = tin.elevation_at(q) {
                let (zlo, zhi) = tin.z_range();
                prop_assert!(z >= zlo - 1e-9 && z <= zhi + 1e-9);
            }
        }
    }

    #[test]
    fn triangles_are_counterclockwise_and_tile_the_hull(pts in points(60)) {
        if let Some(tin) = build(pts.clone()) {
            for i in 0..tin.triangles.len() {
                let [a, b, c] = tin.corners(i);
                prop_assert!((b - a).truncate().perp_dot((c - a).truncate()) > 0.0);
            }
            let hull: Vec<DVec2> = {
                let p: Vec<DVec2> = pts.iter().map(|p| p.truncate()).collect();
                cadektor_core::svg::convex_hull(&p)
            };
            prop_assert!((tin.area_xy() - cadektor_core::geom::polygon_area(&hull)).abs() < 1e-6 * (1.0 + tin.area_xy()));
        }
    }

    #[test]
    fn contour_points_lie_at_their_level(pts in points(50), minor in 5.0f64..40.0) {
        if let Some(tin) = build(pts) {
            let spec = ContourSpec { minor_interval: minor, major_interval: minor * 5.0, datum: 0.0, smooth: false };
            for c in contours(&tin, &spec).unwrap() {
                prop_assert!(c.points.len() >= 2);
                for p in &c.points {
                    // Contour points are on triangle edges, where the lookup may pick either side.
                    let z = tin.elevation_at(*p).expect("on the surface");
                    prop_assert!((z - c.level).abs() < 1e-6 * (1.0 + c.level.abs()), "{z} vs {}", c.level);
                }
            }
        }
    }

    #[test]
    fn cut_and_fill_swap_when_the_surfaces_swap(a in points(40), b in points(40)) {
        if let (Some(ta), Some(tb)) = (build(a), build(b)) {
            let ab = cut_fill(&ta, &tb);
            let ba = cut_fill(&tb, &ta);
            let tol = 1e-6 * (1.0 + ab.cut + ab.fill);
            prop_assert!((ab.cut - ba.fill).abs() < tol, "{ab:?} {ba:?}");
            prop_assert!((ab.fill - ba.cut).abs() < tol);
            prop_assert!((ab.area - ba.area).abs() < 1e-6 * (1.0 + ab.area));
        }
    }

    #[test]
    fn net_volume_is_the_integral_of_the_difference_of_planes(
        ax in -0.5f64..0.5, ay in -0.5f64..0.5, a0 in -50.0f64..50.0,
        bx in -0.5f64..0.5, by in -0.5f64..0.5, b0 in -50.0f64..50.0,
    ) {
        // Two planes over a 100 by 100 square, made with different vertices.
        let plane = |n: i32, step: f64, ax: f64, ay: f64, a0: f64| {
            let mut points = Vec::new();
            for i in 0..n { for j in 0..n {
                let (x, y) = (i as f64 * step, j as f64 * step);
                points.push(DVec3::new(x, y, a0 + ax * x + ay * y));
            } }
            Tin::build(&SourceData { points, lines: vec![] }).unwrap()
        };
        let existing = plane(5, 25.0, ax, ay, a0);
        let proposed = plane(6, 20.0, bx, by, b0);
        let r = cut_fill(&existing, &proposed);
        // Mean difference over the square is the difference at the middle.
        let mid = (b0 - a0) + (bx - ax) * 50.0 + (by - ay) * 50.0;
        prop_assert!((r.net() - mid * 10000.0).abs() < 1e-6 * (1.0 + r.cut + r.fill), "{r:?} {mid}");
        prop_assert!((r.area - 10000.0).abs() < 1e-6);
    }

    #[test]
    fn a_box_pad_on_flat_ground_is_area_times_height(
        x0 in 5.0f64..40.0, w in 5.0f64..40.0, y0 in 5.0f64..40.0, h in 5.0f64..40.0, rise in -20.0f64..20.0,
    ) {
        // Flat ground at 3 over 0..100. The pad is `rise` above the ground.
        let src = flat(21, 5.0, 0.0, 3.0);
        let existing = Tin::build(&src).unwrap();
        let boundary = rect(x0, y0, x0 + w, y0 + h);
        let mods = vec![
            Modifier::GradeLimit { boundary: rect(x0 - 2.0, y0 - 2.0, x0 + w + 2.0, y0 + h + 2.0) },
            Modifier::Pad { boundary: boundary.clone(), elevation: 3.0 + rise, slope: None },
        ];
        let proposed = proposed_surface(&existing, &mods).unwrap();
        let r = cut_fill_in(&existing, &proposed, Some(&boundary));
        let expect = w * h * rise.abs();
        prop_assert!((r.fill + r.cut - expect).abs() < 1e-6 * (1.0 + expect), "{r:?} {expect}");
        prop_assert!((r.net() - w * h * rise).abs() < 1e-6 * (1.0 + expect));
        // The ground outside the grade limit does not change.
        let outside = proposed.elevation_at(DVec2::new(99.0, 99.0)).unwrap();
        prop_assert!((outside - 3.0).abs() < 1e-9);
    }

    #[test]
    fn proposed_surface_equals_existing_outside_limits_and_pads_on_curved_terrain(
        fx in 0.02f64..0.15, fy in 0.02f64..0.15, amp in 0.5f64..6.0,
        x0 in 20.0f64..60.0, w in 8.0f64..30.0, y0 in 20.0f64..60.0, h in 8.0f64..30.0,
        margin in 0.0f64..12.0, with_limit in any::<bool>(), rise in -8.0f64..8.0,
    ) {
        let mut points = Vec::new();
        for i in 0..16 { for j in 0..16 {
            let (x, y) = (i as f64 * 7.0, j as f64 * 7.0);
            points.push(DVec3::new(x, y, amp * (x * fx).sin() + amp * (y * fy).cos()));
        } }
        let existing = Tin::build(&SourceData { points, lines: vec![] }).unwrap();
        let pad = rect(x0, y0, x0 + w, y0 + h);
        let limit = rect(x0 - margin - 1.0, y0 - margin - 1.0, x0 + w + margin + 1.0, y0 + h + margin + 1.0);
        let mut mods = vec![Modifier::Pad { boundary: pad.clone(), elevation: rise, slope: None }];
        if with_limit { mods.insert(0, Modifier::GradeLimit { boundary: limit.clone() }); }
        let proposed = proposed_surface(&existing, &mods).unwrap();
        let region = if with_limit { &limit } else { &pad };
        for i in 0..37 { for j in 0..37 {
            let q = DVec2::new(0.37 + i as f64 * 2.8, 0.61 + j as f64 * 2.8);
            let inside = q.x >= region[0].x - 1e-6 && q.x <= region[2].x + 1e-6 && q.y >= region[0].y - 1e-6 && q.y <= region[2].y + 1e-6;
            if !inside {
                let (a, b) = (proposed.elevation_at(q).unwrap(), existing.elevation_at(q).unwrap());
                prop_assert!((a - b).abs() < 1e-9, "{q} {a} {b}");
            }
        } }
        prop_assert!((proposed.area_xy() - existing.area_xy()).abs() < 1e-6);
        // Net volume of the pad itself: plane minus ground, over the pad.
        let in_pad = cut_fill_in(&existing, &proposed, Some(&pad));
        let all = cut_fill(&existing, &proposed);
        if !with_limit {
            prop_assert!((in_pad.fill - all.fill).abs() < 1e-6 * (1.0 + all.fill) && (in_pad.cut - all.cut).abs() < 1e-6 * (1.0 + all.cut));
        }
    }

    #[test]
    fn draped_points_lie_on_the_surface(pts in points(40), a in prop::collection::vec((0.1f64..0.9, 0.1f64..0.9), 2..6)) {
        if let Some(tin) = build(pts) {
            let (lo, hi) = tin.bounds();
            let line: Vec<DVec2> = a.iter().map(|(fx, fy)| lo + (hi - lo) * DVec2::new(*fx, *fy)).collect();
            if let Ok(out) = drape(&tin, &line, false) {
                prop_assert!(out.len() >= line.len());
                for p in out {
                    prop_assert!((tin.elevation_at(p.truncate()).unwrap() - p.z).abs() < 1e-6);
                }
            }
        }
    }
}
