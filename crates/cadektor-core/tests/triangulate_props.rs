//! Random simple polygons keep their area when triangulated.

use cadektor_core::geom::polygon_area;
use cadektor_core::mesh::{clean_ring, triangulate};
use glam::DVec2;
use proptest::prelude::*;

/// A simple polygon: a star-shaped ring around the origin (sorted angles, random radii).
/// It is concave whenever the radii differ. Each angle step is below 180 degrees
/// (weights 1.0..1.9 with 3 or more points), so the origin stays inside and the ring
/// never crosses itself.
fn star() -> impl Strategy<Value = Vec<DVec2>> {
    prop::collection::vec((1.0..1.9f64, 0.5..100.0f64), 3..40).prop_map(|items| {
        let total: f64 = items.iter().map(|(w, _)| w).sum();
        let mut angle = 0.0;
        items
            .iter()
            .map(|(w, r)| {
                let p = DVec2::from_angle(angle) * *r;
                angle += w / total * std::f64::consts::TAU;
                p
            })
            .collect()
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn triangles_cover_exactly_the_polygon(poly in star(), clockwise in any::<bool>(), shift in -1.0e6..1.0e6f64) {
        let mut pts: Vec<DVec2> = poly.iter().map(|p| *p + DVec2::splat(shift)).collect();
        if clockwise {
            pts.reverse();
        }
        let area = polygon_area(&pts);
        let tris = triangulate(&pts);
        let signed: f64 = tris.iter().map(|[a, b, c]| (pts[*b] - pts[*a]).perp_dot(pts[*c] - pts[*a]) / 2.0).sum();
        prop_assert!((signed - area).abs() <= 1e-6 * area.max(1.0), "area {area}, triangles {signed}");
        // Every triangle faces up, whichever way the polygon runs.
        for [a, b, c] in &tris {
            prop_assert!((pts[*b] - pts[*a]).perp_dot(pts[*c] - pts[*a]) > 0.0);
        }
        prop_assert!(tris.len() <= pts.len() - 2);
    }

    #[test]
    fn cleaning_a_ring_keeps_its_area_and_makes_it_counterclockwise(poly in star(), clockwise in any::<bool>()) {
        let mut pts = poly;
        if clockwise {
            pts.reverse();
        }
        let area = polygon_area(&pts);
        let ring = clean_ring(&pts);
        prop_assert!(ring.len() >= 3);
        let signed: f64 = (0..ring.len()).map(|i| ring[i].perp_dot(ring[(i + 1) % ring.len()])).sum::<f64>() / 2.0;
        prop_assert!(signed > 0.0);
        prop_assert!((signed - area).abs() <= 1e-6 * area.max(1.0));
    }
}
