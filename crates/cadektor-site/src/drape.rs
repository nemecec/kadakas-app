//! Draping: putting a 2D polyline on the terrain.

use glam::{DVec2, DVec3};

use crate::SiteError;
use crate::tin::Tin;

/// Parameters t in (0, 1) where the segment a-b crosses an edge of the TIN.
fn edge_crossings(tin: &Tin, a: DVec2, b: DVec2) -> Vec<f64> {
    let r = b - a;
    let mut ts = Vec::new();
    for ti in tin.triangles_in_box(a.min(b), a.max(b)) {
        let c = tin.corners(ti as usize).map(|p| p.truncate());
        for k in 0..3 {
            let (q0, q1) = (c[k], c[(k + 1) % 3]);
            let s = q1 - q0;
            let denom = r.perp_dot(s);
            if denom.abs() < 1e-18 {
                continue;
            }
            let t = (q0 - a).perp_dot(s) / denom;
            let u = (q0 - a).perp_dot(r) / denom;
            if t > 1e-12 && t < 1.0 - 1e-12 && (-1e-12..=1.0 + 1e-12).contains(&u) {
                ts.push(t);
            }
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    ts
}

/// Puts a polyline on the surface. Each segment gets an extra point wherever it crosses a
/// triangle edge, so the 3D line follows the ground between the original points.
/// A closed polyline does not repeat its first point. Fails if a point is off the surface.
pub fn drape(tin: &Tin, points: &[DVec2], closed: bool) -> Result<Vec<DVec3>, SiteError> {
    let n = points.len();
    let at = |p: DVec2, index: usize| tin.elevation_at(p).map(|z| p.extend(z)).ok_or(SiteError::Outside(index));
    let mut out = Vec::with_capacity(n);
    let segs = if closed && n > 2 { n } else { n.saturating_sub(1) };
    for i in 0..n {
        out.push(at(points[i], i)?);
        if i < segs {
            let (a, b) = (points[i], points[(i + 1) % n]);
            for t in edge_crossings(tin, a, b) {
                out.push(at(a.lerp(b, t), i)?);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tin::SourceData;

    fn bumpy() -> Tin {
        // z = x on a 3 by 3 grid with spacing 10: a ramp, with a flat part beyond x = 20.
        let mut points = Vec::new();
        for i in 0..4 {
            for j in 0..4 {
                points.push(DVec3::new(i as f64 * 10.0, j as f64 * 10.0, (i as f64 * 10.0).min(20.0)));
            }
        }
        Tin::build(&SourceData { points, lines: vec![] }).unwrap()
    }

    #[test]
    fn a_line_over_a_crease_gets_a_point_at_the_crease() {
        let tin = bumpy();
        let line = drape(&tin, &[DVec2::new(5.0, 5.0), DVec2::new(25.0, 5.0)], false).unwrap();
        // Crossing the vertical edges at x = 10 and x = 20, and any diagonals.
        assert!(line.len() >= 4);
        assert_eq!(line[0], DVec3::new(5.0, 5.0, 5.0));
        assert_eq!(*line.last().unwrap(), DVec3::new(25.0, 5.0, 20.0));
        assert!(line.iter().any(|p| (p.x - 20.0).abs() < 1e-9 && (p.z - 20.0).abs() < 1e-9));
        assert!(line.windows(2).all(|w| w[0].x <= w[1].x));
        // Every added point lies on the surface.
        assert!(line.iter().all(|p| (tin.elevation_at(p.truncate()).unwrap() - p.z).abs() < 1e-9));
    }

    #[test]
    fn closed_lines_close_and_off_surface_points_fail() {
        let tin = bumpy();
        let ring = [DVec2::new(2.0, 2.0), DVec2::new(28.0, 2.0), DVec2::new(15.0, 28.0)];
        let line = drape(&tin, &ring, true).unwrap();
        assert!(line.len() > 3);
        assert_eq!(line[0].truncate(), ring[0]);
        assert_ne!(line.last().unwrap().truncate(), ring[0]);
        assert_eq!(drape(&tin, &[DVec2::new(5.0, 5.0), DVec2::new(50.0, 5.0)], false).unwrap_err(), SiteError::Outside(1));
        assert_eq!(drape(&tin, &[], false).unwrap(), Vec::<DVec3>::new());
        assert_eq!(drape(&tin, &[DVec2::new(1.0, 1.0)], true).unwrap().len(), 1);
    }
}
