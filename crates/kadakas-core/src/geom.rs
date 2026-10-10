//! 2D geometry helpers.

use glam::DVec2;

/// Signed area of a polygon: positive when it runs counterclockwise (shoelace formula).
/// Fewer than three points have no area.
pub fn signed_area(points: &[DVec2]) -> f64 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    // Relative to the first point: sums of absolute coordinates lose precision far from the origin.
    let o = points[0];
    let twice: f64 = (0..n)
        .map(|i| (points[i] - o).perp_dot(points[(i + 1) % n] - o))
        .sum();
    twice / 2.0
}

/// Unsigned area of a simple polygon.
pub fn polygon_area(points: &[DVec2]) -> f64 {
    signed_area(points).abs()
}

/// The polygon in counterclockwise order.
pub fn ccw(points: &[DVec2]) -> Vec<DVec2> {
    let mut v = points.to_vec();
    if signed_area(&v) < 0.0 {
        v.reverse();
    }
    v
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

/// True if the point is inside the polygon or on its border (border counts as inside).
pub fn point_in_polygon_closed(p: DVec2, poly: &[DVec2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if dist_to_segment(p, a, b) < 1e-9 {
            return true;
        }
    }
    point_in_polygon(p, poly)
}

/// Removes points that lie within `eps` of the point before them.
pub fn dedupe_points(points: &[DVec2], eps: f64) -> Vec<DVec2> {
    let limit = eps * eps;
    let mut out: Vec<DVec2> = Vec::with_capacity(points.len());
    for &p in points {
        if out.last().is_none_or(|q| q.distance_squared(p) > limit) {
            out.push(p);
        }
    }
    out
}

/// A closed ring without repeated points and without the closing point (the last point equal to
/// the first one).
pub fn open_ring(points: &[DVec2], eps: f64) -> Vec<DVec2> {
    let mut ring = dedupe_points(points, eps);
    let limit = eps * eps;
    while ring.len() > 1 && ring[0].distance_squared(ring[ring.len() - 1]) <= limit {
        ring.pop();
    }
    ring
}

/// Cleans a ring: no repeated points, no closing point, counterclockwise. Returns an empty ring
/// for a point that is not finite, for fewer than 3 points, and for no area.
pub fn clean_ring(points: &[DVec2]) -> Vec<DVec2> {
    if points.iter().any(|p| !p.is_finite()) {
        return Vec::new();
    }
    let mut ring = open_ring(points, 1.0e-6);
    let area = signed_area(&ring);
    if ring.len() < 3 || area.abs() < 1.0e-6 {
        return Vec::new();
    }
    if area < 0.0 {
        ring.reverse();
    }
    ring
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
    fn signed_area_and_winding() {
        let sq = [DVec2::ZERO, DVec2::X, DVec2::ONE, DVec2::Y];
        assert_eq!(signed_area(&sq), 1.0);
        let rev: Vec<DVec2> = sq.iter().rev().copied().collect();
        assert_eq!(signed_area(&rev), -1.0);
        assert_eq!(polygon_area(&rev), 1.0);
        assert_eq!(ccw(&rev), sq.to_vec());
        assert!(signed_area(&sq[..2]) == 0.0 && signed_area(&[]) == 0.0);
    }

    #[test]
    fn closed_test_counts_the_border() {
        let sq = [DVec2::ZERO, DVec2::X, DVec2::ONE, DVec2::Y];
        assert!(point_in_polygon_closed(DVec2::new(0.5, 0.0), &sq));
        assert!(point_in_polygon_closed(DVec2::splat(0.5), &sq));
        assert!(!point_in_polygon_closed(DVec2::new(1.5, 0.5), &sq));
        assert!(!point_in_polygon_closed(DVec2::ZERO, &sq[..2]));
    }

    #[test]
    fn ring_cleaning() {
        let ring = [
            DVec2::ZERO,
            DVec2::ZERO,
            DVec2::new(0.0, 1.0),
            DVec2::ONE,
            DVec2::X,
            DVec2::ZERO,
        ];
        let c = clean_ring(&ring);
        assert_eq!(
            c,
            vec![DVec2::X, DVec2::ONE, DVec2::new(0.0, 1.0), DVec2::ZERO]
        );
        assert_eq!(dedupe_points(&ring, 1e-6).len(), 5);
        assert_eq!(open_ring(&ring, 1e-6).len(), 4);
        assert!(clean_ring(&[DVec2::ZERO, DVec2::X, DVec2::new(2.0, 0.0)]).is_empty());
        assert!(clean_ring(&[DVec2::ZERO, DVec2::X, DVec2::new(f64::NAN, 1.0)]).is_empty());
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
