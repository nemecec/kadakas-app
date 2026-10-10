//! Cut and fill volumes between two surfaces.
//!
//! Both surfaces are piecewise linear. The code cuts each triangle of the proposed surface
//! against each triangle of the existing surface. On every piece both surfaces are planes, so
//! their difference is linear and its integral is exact (a prismoidal volume, up to rounding).

use glam::{DVec2, DVec3};

use crate::tin::Tin;

/// Volumes in mm³ and the area they cover in mm². Cut is material removed (the proposed surface
/// is below the existing one). Fill is material added.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CutFill {
    pub cut: f64,
    pub fill: f64,
    /// Area where the two surfaces overlap, seen from above.
    pub area: f64,
}

impl CutFill {
    /// Fill minus cut. Positive means the site needs material.
    pub fn net(&self) -> f64 {
        self.fill - self.cut
    }
}

/// Height of the plane through triangle `t` at `p`.
fn plane_z(t: &[DVec3; 3], p: DVec2) -> f64 {
    let (a, b, c) = (t[0].truncate(), t[1].truncate(), t[2].truncate());
    let area = (b - a).perp_dot(c - a);
    let w0 = (b - p).perp_dot(c - p) / area;
    let w1 = (c - p).perp_dot(a - p) / area;
    w0 * t[0].z + w1 * t[1].z + (1.0 - w0 - w1) * t[2].z
}

/// Clips a convex polygon to the left side of the line a-b.
fn clip_half_plane(poly: &[DVec2], a: DVec2, b: DVec2) -> Vec<DVec2> {
    let side = |p: DVec2| (b - a).perp_dot(p - a);
    let mut out = Vec::with_capacity(poly.len() + 1);
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        let (sp, sq) = (side(p), side(q));
        if sp >= 0.0 {
            out.push(p);
        }
        if (sp > 0.0 && sq < 0.0) || (sp < 0.0 && sq > 0.0) {
            out.push(p.lerp(q, sp / (sp - sq)));
        }
    }
    out
}

/// Positive and negative parts of the integral of a linear function over a triangle.
/// `d` holds the values at the corners. Returns (positive volume, negative volume as a positive
/// number).
pub fn split_integral(d: [f64; 3], area: f64) -> (f64, f64) {
    let total = area * (d[0] + d[1] + d[2]) / 3.0;
    let has_pos = d.iter().any(|v| *v > 0.0);
    let has_neg = d.iter().any(|v| *v < 0.0);
    if !has_neg {
        return (total, 0.0);
    }
    if !has_pos {
        return (0.0, -total);
    }
    // Mixed signs. Find the corner whose sign is alone: the part of the triangle on its side of
    // the zero line is a small triangle, and the integral over it is a pyramid.
    let npos = d.iter().filter(|v| **v > 0.0).count();
    let lone = if npos == 1 {
        d.iter().position(|v| *v > 0.0).unwrap()
    } else {
        d.iter().position(|v| *v < 0.0).unwrap()
    };
    let (o1, o2) = (d[(lone + 1) % 3], d[(lone + 2) % 3]);
    let dl = d[lone];
    let frac = (dl / (dl - o1)) * (dl / (dl - o2));
    let part = area * frac * dl.abs() / 3.0;
    if dl > 0.0 {
        (part, part - total)
    } else {
        (part + total, part)
    }
}

/// Cut and fill of `proposed` against `existing`, over the area where both exist.
pub fn cut_fill(existing: &Tin, proposed: &Tin) -> CutFill {
    cut_fill_in(existing, proposed, None)
}

/// Like `cut_fill`, but only inside `region`, which must be a convex polygon. Without a region
/// the whole overlap counts. Corners are counterclockwise or clockwise.
pub fn cut_fill_in(existing: &Tin, proposed: &Tin, region: Option<&[DVec2]>) -> CutFill {
    let mut region: Option<Vec<DVec2>> = region.map(<[DVec2]>::to_vec);
    if let Some(r) = region.as_mut() {
        let twice: f64 = (0..r.len())
            .map(|i| r[i].perp_dot(r[(i + 1) % r.len()]))
            .sum();
        if twice < 0.0 {
            r.reverse();
        }
    }
    let mut r = CutFill::default();
    for pi in 0..proposed.triangles.len() {
        let pt = proposed.corners(pi);
        let pxy = pt.map(|p| p.truncate());
        let lo = pxy[0].min(pxy[1]).min(pxy[2]);
        let hi = pxy[0].max(pxy[1]).max(pxy[2]);
        for ei in existing.triangles_in_box(lo, hi) {
            let et = existing.corners(ei as usize);
            let exy = et.map(|p| p.truncate());
            let mut poly = pxy.to_vec();
            for k in 0..3 {
                poly = clip_half_plane(&poly, exy[k], exy[(k + 1) % 3]);
                if poly.len() < 3 {
                    break;
                }
            }
            if let Some(reg) = &region {
                for k in 0..reg.len() {
                    if poly.len() < 3 {
                        break;
                    }
                    poly = clip_half_plane(&poly, reg[k], reg[(k + 1) % reg.len()]);
                }
            }
            if poly.len() < 3 {
                continue;
            }
            let d: Vec<f64> = poly
                .iter()
                .map(|p| plane_z(&pt, *p) - plane_z(&et, *p))
                .collect();
            for k in 1..poly.len() - 1 {
                let area = (poly[k] - poly[0]).perp_dot(poly[k + 1] - poly[0]) / 2.0;
                if area <= 0.0 {
                    continue;
                }
                let (pos, neg) = split_integral([d[0], d[k], d[k + 1]], area);
                r.fill += pos;
                r.cut += neg;
                r.area += area;
            }
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tin::{SourceData, Tin};

    fn plane(n: usize, step: f64, f: impl Fn(f64, f64) -> f64) -> Tin {
        let mut pts = Vec::new();
        for i in 0..n {
            for j in 0..n {
                let (x, y) = (i as f64 * step, j as f64 * step);
                pts.push(DVec3::new(x, y, f(x, y)));
            }
        }
        Tin::build(&SourceData {
            points: pts,
            lines: vec![],
        })
        .unwrap()
    }

    #[test]
    fn split_integral_cases() {
        // All positive, all negative, zeros.
        assert_eq!(split_integral([3.0, 3.0, 3.0], 2.0), (6.0, 0.0));
        assert_eq!(split_integral([-3.0, -3.0, -3.0], 2.0), (0.0, 6.0));
        assert_eq!(split_integral([0.0, 0.0, 0.0], 2.0), (0.0, 0.0));
        // One positive corner: the region is 1/4 of the area (1/2 * 1/2), a pyramid of height 1.
        let (p, n) = split_integral([1.0, -1.0, -1.0], 1.0);
        assert!((p - 1.0 / 4.0 / 3.0).abs() < 1e-12);
        // Total = 1 * (1 - 1 - 1) / 3 = -1/3, so the negative part is 1/3 + 1/12.
        assert!((n - (1.0 / 3.0 + 1.0 / 12.0)).abs() < 1e-12);
        // One negative corner: mirror image.
        let (p2, n2) = split_integral([-1.0, 1.0, 1.0], 1.0);
        assert!((p2 - n).abs() < 1e-12 && (n2 - p).abs() < 1e-12);
        // A zero corner between a positive and a negative one: halves of a ramp.
        let (p, n) = split_integral([1.0, 0.0, -1.0], 1.0);
        assert!((p - 1.0 / 6.0).abs() < 1e-12 && (n - 1.0 / 6.0).abs() < 1e-12);
    }

    #[test]
    fn lowering_flat_ground_is_area_times_depth() {
        let existing = plane(5, 25.0, |_, _| 10.0);
        // Different vertices: a coarser, shifted surface over the same square.
        let proposed = plane(3, 50.0, |_, _| 7.5);
        let r = cut_fill(&existing, &proposed);
        assert!((r.cut - 100.0 * 100.0 * 2.5).abs() < 1e-6);
        assert!(r.fill.abs() < 1e-9);
        assert!((r.area - 10000.0).abs() < 1e-6);
        assert!((r.net() + 25000.0).abs() < 1e-6);
    }

    #[test]
    fn crossing_planes_split_into_cut_and_fill() {
        // Existing is flat at 0. Proposed rises from -1 at x=0 to +1 at x=100. Cut and fill are equal.
        let existing = plane(5, 25.0, |_, _| 0.0);
        let proposed = plane(6, 20.0, |x, _| x / 50.0 - 1.0);
        let r = cut_fill(&existing, &proposed);
        // Each half is a wedge: 50 * 100 * 1 / 2.
        assert!((r.cut - 2500.0).abs() < 1e-6, "{r:?}");
        assert!((r.fill - 2500.0).abs() < 1e-6, "{r:?}");
    }

    #[test]
    fn only_the_overlap_counts() {
        let existing = plane(3, 50.0, |_, _| 0.0);
        let mut pts = Vec::new();
        for (x, y) in [(50.0, 0.0), (150.0, 0.0), (150.0, 100.0), (50.0, 100.0)] {
            pts.push(DVec3::new(x, y, 1.0));
        }
        let proposed = Tin::build(&SourceData {
            points: pts,
            lines: vec![],
        })
        .unwrap();
        let r = cut_fill(&existing, &proposed);
        assert!((r.area - 5000.0).abs() < 1e-6);
        assert!((r.fill - 5000.0).abs() < 1e-6);
    }

    #[test]
    fn same_surface_has_no_volume() {
        let t = plane(5, 10.0, |x, y| (x * 0.1).sin() + y * 0.01);
        let r = cut_fill(&t, &t);
        assert!(r.cut.abs() < 1e-9 && r.fill.abs() < 1e-9);
        assert!((r.area - 1600.0).abs() < 1e-6);
    }
}
