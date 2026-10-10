//! 2D geometry helpers.

use glam::DVec2;

/// Unsigned area of a simple polygon (shoelace formula).
pub fn polygon_area(points: &[DVec2]) -> f64 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    // Relative to the first point: sums of absolute coordinates lose precision far from the origin.
    let o = points[0];
    let twice: f64 = (0..n)
        .map(|i| (points[i] - o).perp_dot(points[(i + 1) % n] - o))
        .sum();
    twice.abs() / 2.0
}

/// Distance from `p` to the segment `a`-`b`.
pub fn dist_to_segment(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 == 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Even-odd point-in-polygon test.
pub fn point_in_polygon(p: DVec2, poly: &[DVec2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_area() {
        let sq = [DVec2::ZERO, DVec2::X, DVec2::ONE, DVec2::Y];
        assert_eq!(polygon_area(&sq), 1.0);
        assert!(point_in_polygon(DVec2::splat(0.5), &sq));
        assert!(!point_in_polygon(DVec2::splat(1.5), &sq));
    }

    #[test]
    fn segment_distance() {
        let d = dist_to_segment(DVec2::new(5.0, 3.0), DVec2::ZERO, DVec2::new(10.0, 0.0));
        assert_eq!(d, 3.0);
        assert_eq!(
            dist_to_segment(DVec2::new(-4.0, 3.0), DVec2::ZERO, DVec2::X),
            5.0
        );
    }
}
