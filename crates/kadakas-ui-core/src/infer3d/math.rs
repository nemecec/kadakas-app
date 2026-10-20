//! Small geometry helpers of the inference.

use glam::{DVec2, DVec3};

/// The parameters `(s, t)` of the closest points of the lines `o1 + s*d1` and `o2 + t*d2`.
/// `None` when the lines are parallel: the angle between them is under about 1e-6 radians
/// (relative to the lengths of the directions, so the size of the numbers does not matter).
///
/// The cross product of the directions is used, not `a*c - b*b`, which loses all digits for
/// lines that are almost parallel.
pub(crate) fn line_line(o1: DVec3, d1: DVec3, o2: DVec3, d2: DVec3) -> Option<(f64, f64)> {
    let n = d1.cross(d2);
    let n2 = n.length_squared();
    let (a, c) = (d1.length_squared(), d2.length_squared());
    if a == 0.0 || c == 0.0 || !n2.is_finite() || n2 <= 1e-12 * a * c {
        return None;
    }
    let w = o2 - o1;
    let s = w.cross(d2).dot(n) / n2;
    let t = w.cross(d1).dot(n) / n2;
    Some((s, t))
}

/// The closest point of a screen segment to a pixel: the parameter (0 to 1, clamped) and the
/// distance in pixels.
pub(crate) fn nearest_on_screen_seg(p: DVec2, a: DVec2, b: DVec2) -> (f64, f64) {
    let d = b - a;
    let l2 = d.length_squared();
    let s = if l2 < 1e-18 {
        0.0
    } else {
        ((p - a).dot(d) / l2).clamp(0.0, 1.0)
    };
    (s, (a + d * s - p).length())
}

/// The distance along a ray where it meets a plane (`None` when parallel or behind).
pub(crate) fn ray_plane(origin: DVec3, dir: DVec3, point: DVec3, normal: DVec3) -> Option<f64> {
    let den = dir.dot(normal);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = (point - origin).dot(normal) / den;
    (t > 0.0 && t.is_finite()).then_some(t)
}

/// Even-odd test of a point in the loops of a planar polygon. `drop` is the axis to leave out
/// (the dominant axis of the normal).
pub(crate) fn inside_loops(p: DVec3, loops: &[Vec<DVec3>], drop: usize) -> bool {
    let (u, v) = match drop {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let (px, py) = (p[u], p[v]);
    let mut inside = false;
    for l in loops {
        let n = l.len();
        let mut j = n - 1;
        for i in 0..n {
            let (a, b) = (l[i], l[j]);
            if (a[v] > py) != (b[v] > py) {
                let x = (b[u] - a[u]) * (py - a[v]) / (b[v] - a[v]) + a[u];
                if px < x {
                    inside = !inside;
                }
            }
            j = i;
        }
    }
    inside
}

/// The normal of a polygon by Newell's method (not normalized; zero for a degenerate polygon).
pub(crate) fn newell(points: &[DVec3]) -> DVec3 {
    let mut n = DVec3::ZERO;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        n += DVec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        );
    }
    n
}

/// The center of the circle through three points (`None` when they are on a line).
pub(crate) fn circumcenter(a: DVec3, b: DVec3, c: DVec3) -> Option<DVec3> {
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    let n2 = n.length_squared();
    if n2
        < 1e-18
            * ab.length_squared()
                .max(ac.length_squared())
                .powi(2)
                .max(1e-300)
    {
        return None;
    }
    let to = (n.cross(ab) * ac.length_squared() + ac.cross(n) * ab.length_squared()) / (2.0 * n2);
    Some(a + to)
}

/// A tolerance for "the same point" in world units, from the size of the numbers.
pub(crate) fn world_eps(scale: f64) -> f64 {
    1e-7 * (1.0 + scale.abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_cross_or_are_parallel() {
        let (s, t) =
            line_line(DVec3::ZERO, DVec3::X, DVec3::new(2.0, -3.0, 0.0), DVec3::Y).unwrap();
        assert!((s - 2.0).abs() < 1e-12 && (t - 3.0).abs() < 1e-12);
        assert!(line_line(DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::X * 2.0).is_none());
        assert!(line_line(DVec3::ZERO, DVec3::ZERO, DVec3::Y, DVec3::X).is_none());
    }

    #[test]
    fn near_parallel_lines_are_stable() {
        // Far from the origin, with long directions, at small angles.
        for scale in [1.0, 1.0e3, 1.0e6] {
            for angle in [1.0e-7f64, 5.0e-7, 2.0e-6, 1.0e-5, 1.0e-3] {
                let (o1, o2) = (
                    DVec3::new(1.0e5, -2.0e5, 3.0e4),
                    DVec3::new(1.0e5, -2.0e5 + 1.0, 3.0e4),
                );
                let d1 = DVec3::X * scale;
                let d2 = DVec3::new(angle.cos(), angle.sin(), 0.0) * (2.0 * scale);
                let r = line_line(o1, d1, o2, d2);
                if angle < 1.0e-6 {
                    assert!(r.is_none(), "{scale} {angle}");
                } else if let Some((s, t)) = r {
                    // The points are the closest ones: the line between them is orthogonal
                    // to both (checked against the exact crossing in the plane z = const).
                    let (p1, p2) = (o1 + d1 * s, o2 + d2 * t);
                    assert!(s.is_finite() && t.is_finite());
                    assert!(
                        p1.distance(p2) < 1e-6 * (1.0 + p1.length()),
                        "{scale} {angle}"
                    );
                    // Exact crossing: y of line 2 is o2.y + t*d2.y = o1.y.
                    let exact = (o1.y - o2.y) / d2.y;
                    assert!(
                        ((t - exact) / exact).abs() < 1e-6,
                        "{scale} {angle} {t} {exact}"
                    );
                } else {
                    panic!("{scale} {angle} was taken for parallel");
                }
            }
        }
    }

    #[test]
    fn nearest_point_on_a_screen_segment() {
        let (s, d) =
            nearest_on_screen_seg(DVec2::new(5.0, 3.0), DVec2::ZERO, DVec2::new(10.0, 0.0));
        assert_eq!((s, d), (0.5, 3.0));
        let (s, d) =
            nearest_on_screen_seg(DVec2::new(-4.0, 3.0), DVec2::ZERO, DVec2::new(10.0, 0.0));
        assert_eq!((s, d), (0.0, 5.0));
        let (s, d) = nearest_on_screen_seg(DVec2::new(3.0, 4.0), DVec2::ZERO, DVec2::ZERO);
        assert_eq!((s, d), (0.0, 5.0));
    }

    #[test]
    fn ray_and_plane() {
        assert_eq!(
            ray_plane(DVec3::new(0.0, 0.0, 5.0), -DVec3::Z, DVec3::ZERO, DVec3::Z),
            Some(5.0)
        );
        assert!(ray_plane(DVec3::new(0.0, 0.0, 5.0), DVec3::Z, DVec3::ZERO, DVec3::Z).is_none());
        assert!(ray_plane(DVec3::new(0.0, 0.0, 5.0), DVec3::X, DVec3::ZERO, DVec3::Z).is_none());
    }

    #[test]
    fn a_point_in_a_polygon_with_a_hole() {
        let sq = |a: f64, b: f64| {
            vec![
                DVec3::new(a, a, 0.0),
                DVec3::new(b, a, 0.0),
                DVec3::new(b, b, 0.0),
                DVec3::new(a, b, 0.0),
            ]
        };
        let loops = vec![sq(0.0, 10.0), sq(4.0, 6.0)];
        assert!(inside_loops(DVec3::new(1.0, 1.0, 0.0), &loops, 2));
        assert!(!inside_loops(DVec3::new(5.0, 5.0, 0.0), &loops, 2));
        assert!(!inside_loops(DVec3::new(11.0, 5.0, 0.0), &loops, 2));
        // The same polygon in the other planes.
        let xz: Vec<Vec<DVec3>> = loops
            .iter()
            .map(|l| l.iter().map(|p| DVec3::new(p.x, 0.0, p.y)).collect())
            .collect();
        assert!(inside_loops(DVec3::new(1.0, 0.0, 1.0), &xz, 1));
        let yz: Vec<Vec<DVec3>> = loops
            .iter()
            .map(|l| l.iter().map(|p| DVec3::new(0.0, p.x, p.y)).collect())
            .collect();
        assert!(inside_loops(DVec3::new(0.0, 1.0, 1.0), &yz, 0));
    }

    #[test]
    fn newell_and_circumcenter() {
        let sq = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y];
        assert!((newell(&sq) - DVec3::new(0.0, 0.0, 2.0)).length() < 1e-12);
        let c = circumcenter(
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(0.0, 1.0, 0.0),
            DVec3::new(-1.0, 0.0, 0.0),
        )
        .unwrap();
        assert!(c.length() < 1e-12);
        assert!(circumcenter(DVec3::ZERO, DVec3::X, DVec3::X * 2.0).is_none());
        assert!(world_eps(1e6) > world_eps(1.0));
    }
}
