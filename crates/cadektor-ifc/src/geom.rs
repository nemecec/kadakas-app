//! Geometry helpers for the IFC bodies. Units are mm.

use cadektor_core::hardscape::surface::{self, DRAPE_STEP};
use glam::{DVec2, DVec3};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

/// Rounds to 0.1 micrometer, so files stay small. Numbers that are not finite become 0.
pub fn r4(x: f64) -> f64 {
    if x.is_finite() { (x * 1.0e4).round() / 1.0e4 } else { 0.0 }
}

/// Twice the signed area of a ring: positive for counterclockwise.
pub fn signed_area(ring: &[DVec2]) -> f64 {
    let n = ring.len();
    if n < 3 {
        return 0.0;
    }
    let o = ring[0];
    (0..n).map(|i| (ring[i] - o).perp_dot(ring[(i + 1) % n] - o)).sum::<f64>() / 2.0
}

/// Removes repeated points and the closing point, and turns the ring counterclockwise.
/// Returns an empty ring if fewer than 3 points or no area remain.
pub fn clean_ring(points: &[DVec2]) -> Vec<DVec2> {
    let mut ring: Vec<DVec2> = Vec::with_capacity(points.len());
    for p in points {
        if !p.is_finite() {
            return Vec::new();
        }
        if ring.last().is_none_or(|l| l.distance_squared(*p) > 1.0e-12) {
            ring.push(*p);
        }
    }
    while ring.len() > 1 && ring[0].distance_squared(ring[ring.len() - 1]) <= 1.0e-12 {
        ring.pop();
    }
    let area = signed_area(&ring);
    if ring.len() < 3 || area.abs() < 1.0e-6 {
        return Vec::new();
    }
    if area < 0.0 {
        ring.reverse();
    }
    ring
}

/// Splits a ring with holes into simple polygons without holes. It cuts the area into horizontal
/// strips at the height of every hole vertex. Inside a strip, a hole crosses the whole strip, so the
/// pieces have no holes. `outer` is counterclockwise. At most 400 cuts are used. With more, a piece
/// may still have a hole, and the hole is filled.
pub fn split_with_holes(outer: &[DVec2], holes: &[Vec<DVec2>]) -> Vec<Vec<DVec2>> {
    if holes.is_empty() {
        return vec![outer.to_vec()];
    }
    let path = |r: &[DVec2]| r.iter().map(|p| [p.x, p.y]).collect::<Vec<[f64; 2]>>();
    let mut subject = vec![path(outer)];
    subject.extend(holes.iter().map(|h| path(h)));
    let (lo, hi) = outer.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
    let mut cuts: Vec<f64> = holes.iter().flatten().map(|p| p.y).filter(|y| *y > lo && *y < hi).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    if cuts.len() > 400 {
        let step = cuts.len() as f64 / 400.0;
        cuts = (0..400).map(|i| cuts[(i as f64 * step) as usize]).collect();
    }
    let (min_x, max_x) = outer.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
    let (min_x, max_x) = (min_x - 1.0, max_x + 1.0);
    let mut levels = vec![lo - 1.0];
    levels.extend(cuts);
    levels.push(hi + 1.0);
    let mut out = Vec::new();
    for w in levels.windows(2) {
        let rect = vec![[min_x, w[0]], [max_x, w[0]], [max_x, w[1]], [min_x, w[1]]];
        for shape in subject.overlay(&vec![rect], OverlayRule::Intersect, FillRule::EvenOdd) {
            if let Some(contour) = shape.first() {
                let piece = clean_ring(&contour.iter().map(|p| DVec2::new(p[0], p[1])).collect::<Vec<_>>());
                if !piece.is_empty() {
                    out.push(piece);
                }
            }
        }
    }
    out
}

/// A closed triangle mesh of a slab: the top surface at `z`, the bottom `thickness` below, and the
/// walls, also around the holes. With `step`, the top follows a surface that is not flat, and long
/// edges are split. `ring` must be counterclockwise (see [`clean_ring`]), and so must the holes.
/// Triangles face outward.
pub fn solid_mesh(ring: &[DVec2], holes: &[Vec<DVec2>], z: &dyn Fn(DVec2) -> f64, thickness: f64, step: Option<f64>) -> (Vec<DVec3>, Vec<u32>) {
    let mut top: Vec<DVec3> = Vec::new();
    let mut top_idx: Vec<u32> = Vec::new();
    for piece in split_with_holes(ring, holes) {
        let (p, i) = surface::top_surface(&piece, z, step);
        let off = top.len() as u32;
        top.extend(p);
        top_idx.extend(i.iter().map(|k| k + off));
    }
    let mut points = top.clone();
    let mut idx = top_idx.clone();
    // Bottom: the same triangles, turned around.
    let off = points.len() as u32;
    points.extend(top.iter().map(|p| DVec3::new(p.x, p.y, p.z - thickness)));
    for t in top_idx.chunks_exact(3) {
        idx.extend([off + t[0], off + t[2], off + t[1]]);
    }
    // Walls along the outline, and around the holes (clockwise, so the outside of the solid is on the right).
    // A draped outline gets extra points so it follows the ground.
    let mut loops: Vec<Vec<DVec2>> = vec![ring.to_vec()];
    loops.extend(holes.iter().map(|h| h.iter().rev().copied().collect()));
    for ring in &loops {
        let n = ring.len();
        let mut outline: Vec<DVec2> = Vec::new();
        for i in 0..n {
            let (p, q) = (ring[i], ring[(i + 1) % n]);
            let parts = step.map_or(1, |s| ((p.distance(q) / s).ceil() as usize).clamp(1, 400));
            for k in 0..parts {
                outline.push(p.lerp(q, k as f64 / parts as f64));
            }
        }
        let m = outline.len();
        for i in 0..m {
            let (p, q) = (outline[i], outline[(i + 1) % m]);
            let (zp, zq) = (z(p), z(q));
            let base = points.len() as u32;
            points.extend([p.extend(zp - thickness), q.extend(zq - thickness), q.extend(zq), p.extend(zp)]);
            idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    (points, idx)
}

/// The step that splits triangles of a draped surface.
pub const STEP: f64 = DRAPE_STEP;

/// Volume enclosed by a closed triangle mesh (positive when the triangles face outward).
pub fn signed_volume(points: &[DVec3], idx: &[u32]) -> f64 {
    idx.chunks_exact(3).map(|t| points[t[0] as usize].dot(points[t[1] as usize].cross(points[t[2] as usize]))).sum::<f64>() / 6.0
}

/// Points of an arc, counterclockwise from `start` over `sweep` radians. The distance of the polyline from
/// the true arc is at most `tolerance`.
pub fn arc_points(center: DVec2, radius: f64, start: f64, sweep: f64, tolerance: f64) -> Vec<DVec2> {
    let radius = radius.abs();
    let step = if radius <= tolerance { std::f64::consts::FRAC_PI_2 } else { 2.0 * (1.0 - tolerance / radius).acos() };
    let n = ((sweep / step.max(1.0e-3)).ceil() as usize).clamp(4, 720);
    (0..=n).map(|i| center + DVec2::from_angle(start + sweep * i as f64 / n as f64) * radius).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(s: f64) -> Vec<DVec2> {
        vec![DVec2::new(0.0, 0.0), DVec2::new(s, 0.0), DVec2::new(s, s), DVec2::new(0.0, s)]
    }

    #[test]
    fn rounding() {
        assert_eq!(r4(1.23456789), 1.2346);
        assert_eq!(r4(f64::NAN), 0.0);
        assert_eq!(r4(f64::INFINITY), 0.0);
        assert_eq!(r4(-0.00004), 0.0);
    }

    #[test]
    fn rings_are_cleaned() {
        let mut open = square(1000.0);
        let c = clean_ring(&open);
        assert_eq!(c.len(), 4);
        assert!(signed_area(&c) > 0.0);
        // A clockwise ring with a closing point and a repeated point.
        open.reverse();
        open.insert(1, open[0]);
        open.push(open[0]);
        let c = clean_ring(&open);
        assert_eq!(c.len(), 4);
        assert!((signed_area(&c) - 1.0e6).abs() < 1e-6);
        assert!(clean_ring(&[DVec2::ZERO, DVec2::X]).is_empty());
        assert!(clean_ring(&[DVec2::ZERO, DVec2::X, DVec2::X * 2.0]).is_empty());
        assert!(clean_ring(&[DVec2::ZERO, DVec2::X, DVec2::new(f64::NAN, 1.0)]).is_empty());
        assert_eq!(signed_area(&[DVec2::ZERO]), 0.0);
    }

    #[test]
    fn slab_mesh_is_closed_and_faces_outward() {
        let ring = clean_ring(&square(2000.0));
        let (p, i) = solid_mesh(&ring, &[], &|_| 100.0, 150.0, None);
        assert!((signed_volume(&p, &i) - 2000.0 * 2000.0 * 150.0).abs() < 1.0);
        assert!(i.iter().all(|k| (*k as usize) < p.len()));
        // A sloped, split top keeps the volume of a wedge.
        let (p, i) = solid_mesh(&ring, &[], &|q| 100.0 + q.x * 0.1, 150.0, Some(500.0));
        let expected = 2000.0 * 2000.0 * (150.0);
        assert!((signed_volume(&p, &i) - expected).abs() < 1.0, "{}", signed_volume(&p, &i));
    }

    #[test]
    fn slabs_with_holes_keep_the_holes() {
        let ring = clean_ring(&square(2000.0));
        let hole = clean_ring(&[DVec2::new(500.0, 500.0), DVec2::new(1000.0, 600.0), DVec2::new(900.0, 1200.0), DVec2::new(600.0, 1100.0)]);
        let circle = clean_ring(&arc_points(DVec2::new(1500.0, 1500.0), 200.0, 0.0, std::f64::consts::TAU, 1.0));
        let holes = vec![hole.clone(), circle.clone()];
        let pieces = split_with_holes(&ring, &holes);
        let area: f64 = pieces.iter().map(|p| signed_area(p)).sum();
        let expected = 2000.0 * 2000.0 - signed_area(&hole) - signed_area(&circle);
        assert!((area - expected).abs() < 1.0, "{area} {expected}");
        for step in [None, Some(400.0)] {
            let (p, i) = solid_mesh(&ring, &holes, &|q| 50.0 + q.x * 0.05, 150.0, step);
            assert!((signed_volume(&p, &i) - expected * 150.0).abs() < 1.0e3, "{}", signed_volume(&p, &i));
        }
        // No holes: the ring itself. A hole outside the ring changes nothing.
        assert_eq!(split_with_holes(&ring, &[]), vec![ring.clone()]);
        let far = clean_ring(&[DVec2::new(5000.0, 5000.0), DVec2::new(5100.0, 5000.0), DVec2::new(5100.0, 5100.0)]);
        let area: f64 = split_with_holes(&ring, &[far]).iter().map(|p| signed_area(p)).sum();
        assert!((area - 4.0e6).abs() < 1.0);
        // Many cut heights are thinned, and the area is still close.
        let big = clean_ring(&arc_points(DVec2::new(1000.0, 1000.0), 900.0, 0.0, std::f64::consts::TAU, 0.01));
        assert!(big.len() > 400);
        let area: f64 = split_with_holes(&ring, std::slice::from_ref(&big)).iter().map(|p| signed_area(p)).sum();
        assert!((area - (4.0e6 - signed_area(&big))).abs() < 4.0e4, "{area}");
    }

    #[test]
    fn arcs() {
        let a = arc_points(DVec2::ZERO, 1000.0, 0.0, std::f64::consts::PI, 1.0);
        assert!(a.len() > 8);
        assert!((a[0] - DVec2::new(1000.0, 0.0)).length() < 1e-9);
        assert!((a[a.len() - 1] - DVec2::new(-1000.0, 0.0)).length() < 1e-6);
        assert!(a.iter().all(|p| (p.length() - 1000.0).abs() < 1e-6));
        // Tiny radius still works.
        assert!(arc_points(DVec2::ZERO, 0.1, 0.0, 1.0, 1.0).len() >= 5);
    }
}
