//! Geometry helpers for the IFC bodies. Units are mm.

use cadektor_core::hardscape::surface::{self, DRAPE_STEP};
use glam::{DVec2, DVec3};

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

/// A closed triangle mesh of a slab: the top surface at `z`, the bottom `thickness` below, and the
/// walls. With `step`, the top follows a surface that is not flat, and long edges are split.
/// `ring` must be counterclockwise (see [`clean_ring`]). Triangles face outward.
pub fn solid_mesh(ring: &[DVec2], z: &dyn Fn(DVec2) -> f64, thickness: f64, step: Option<f64>) -> (Vec<DVec3>, Vec<u32>) {
    let (top, top_idx) = surface::top_surface(ring, z, step);
    let mut points = top.clone();
    let mut idx = top_idx.clone();
    // Bottom: the same triangles, turned around.
    let off = points.len() as u32;
    points.extend(top.iter().map(|p| DVec3::new(p.x, p.y, p.z - thickness)));
    for t in top_idx.chunks_exact(3) {
        idx.extend([off + t[0], off + t[2], off + t[1]]);
    }
    // Walls along the outline. A draped outline gets extra points so it follows the ground.
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
        let (p, i) = solid_mesh(&ring, &|_| 100.0, 150.0, None);
        assert!((signed_volume(&p, &i) - 2000.0 * 2000.0 * 150.0).abs() < 1.0);
        assert!(i.iter().all(|k| (*k as usize) < p.len()));
        // A sloped, split top keeps the volume of a wedge.
        let (p, i) = solid_mesh(&ring, &|q| 100.0 + q.x * 0.1, 150.0, Some(500.0));
        let expected = 2000.0 * 2000.0 * (150.0);
        assert!((signed_volume(&p, &i) - expected).abs() < 1.0, "{}", signed_volume(&p, &i));
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
