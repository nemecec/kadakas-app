//! 2D geometry helpers.

use glam::DVec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

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

/// The largest coordinate that [`split_with_holes`] accepts. The overlay of `i_overlay` puts the
/// shape on an integer grid, and a number beyond this bound (or one that is not finite) would
/// overflow it.
pub const MAX_OVERLAY_COORD: f64 = 1.0e12;

/// True when every point is finite and within [`MAX_OVERLAY_COORD`]: [`split_with_holes`] gives
/// pieces for these rings. A caller that gets none can ask this to tell a refusal from an empty area.
pub fn rings_in_range(outer: &[DVec2], holes: &[Vec<DVec2>]) -> bool {
    outer
        .iter()
        .chain(holes.iter().flatten())
        .all(|p| p.is_finite() && p.abs().max_element() <= MAX_OVERLAY_COORD)
}

/// Splits a ring with holes into simple polygons without holes. It cuts the area into horizontal
/// strips at the height of every hole vertex. Inside a strip, a hole crosses the whole strip, so the
/// pieces have no holes. `outer` is counterclockwise. At most 400 cuts are used. With more, a piece
/// may still have a hole, and the hole is filled.
pub fn split_with_holes(outer: &[DVec2], holes: &[Vec<DVec2>]) -> Vec<Vec<DVec2>> {
    // The overlay works on an integer grid: a number that is not finite or is beyond the bound
    // would overflow it. Such an outline has no pieces.
    if !rings_in_range(outer, holes) {
        return Vec::new();
    }
    if holes.is_empty() {
        return vec![outer.to_vec()];
    }
    let path = |r: &[DVec2]| r.iter().map(|p| [p.x, p.y]).collect::<Vec<[f64; 2]>>();
    let mut subject = vec![path(outer)];
    subject.extend(holes.iter().map(|h| path(h)));
    let (lo, hi) = outer
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.y), hi.max(p.y))
        });
    let mut cuts: Vec<f64> = holes
        .iter()
        .flatten()
        .map(|p| p.y)
        .filter(|y| *y > lo && *y < hi)
        .collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    if cuts.len() > 400 {
        let step = cuts.len() as f64 / 400.0;
        cuts = (0..400).map(|i| cuts[(i as f64 * step) as usize]).collect();
    }
    let (min_x, max_x) = outer
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.x), hi.max(p.x))
        });
    let (min_x, max_x) = (min_x - 1.0, max_x + 1.0);
    let mut levels = vec![lo - 1.0];
    levels.extend(cuts);
    levels.push(hi + 1.0);
    let mut out = Vec::new();
    for w in levels.windows(2) {
        let rect = vec![[min_x, w[0]], [max_x, w[0]], [max_x, w[1]], [min_x, w[1]]];
        for shape in subject.overlay(&vec![rect], OverlayRule::Intersect, FillRule::EvenOdd) {
            if let Some(contour) = shape.first() {
                let piece = clean_ring(
                    &contour
                        .iter()
                        .map(|p| DVec2::new(p[0], p[1]))
                        .collect::<Vec<_>>(),
                );
                if !piece.is_empty() {
                    out.push(piece);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_beyond_the_bound_or_not_finite_have_no_pieces() {
        let sq = |s: f64| {
            vec![
                DVec2::new(-s, -s),
                DVec2::new(s, -s),
                DVec2::new(s, s),
                DVec2::new(-s, s),
            ]
        };
        let hole = vec![
            DVec2::new(-1.0, -1.0),
            DVec2::new(-1.0, 1.0),
            DVec2::new(1.0, 1.0),
            DVec2::new(1.0, -1.0),
        ];
        assert!(!split_with_holes(&sq(10.0), std::slice::from_ref(&hole)).is_empty());
        for big in [1.0e308, 1.0e13, f64::INFINITY, f64::NAN] {
            assert!(split_with_holes(&sq(big), std::slice::from_ref(&hole)).is_empty());
            assert!(split_with_holes(&sq(10.0), &[sq(big)]).is_empty());
        }
    }

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
