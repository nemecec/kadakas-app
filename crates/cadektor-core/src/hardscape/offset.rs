//! The outline of a path hardscape: a center line with a width, turned into a polygon.
//!
//! Each end is flat (a butt cap). At a bend, the outer side gets a join (miter, round or
//! bevel). The inner side meets at the point where the two offset lines cross.

use std::f64::consts::PI;

use glam::DVec2;
use serde::{Deserialize, Serialize};

/// How the outer side of a bend is joined.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Join {
    /// A sharp corner. Very sharp bends fall back to a bevel.
    #[default]
    Miter,
    /// A circular arc around the bend point.
    Round,
    /// A straight cut.
    Bevel,
}

impl Join {
    pub const ALL: [Join; 3] = [Join::Miter, Join::Round, Join::Bevel];

    pub fn id(self) -> &'static str {
        match self {
            Join::Miter => "miter",
            Join::Round => "round",
            Join::Bevel => "bevel",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|j| j.id().eq_ignore_ascii_case(s.trim()))
    }
}

/// A miter longer than this many half widths becomes a bevel.
pub const MITER_LIMIT: f64 = 4.0;
/// Largest angle between two points of a round join, in radians.
const ROUND_STEP: f64 = PI / 12.0;
/// Points closer than this (mm) are one point.
const SAME: f64 = 1e-6;

/// Removes points that repeat the one before.
pub fn dedupe(points: &[DVec2]) -> Vec<DVec2> {
    let mut out: Vec<DVec2> = Vec::with_capacity(points.len());
    for &p in points {
        if out.last().is_none_or(|q| q.distance(p) > SAME) {
            out.push(p);
        }
    }
    out
}

/// Length of a polyline.
pub fn length(points: &[DVec2]) -> f64 {
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// One side of the outline, from the start of the path to its end. `side` is 1 for the left of
/// the travel direction and -1 for the right.
fn side_points(pts: &[DVec2], h: f64, side: f64, join: Join) -> Vec<DVec2> {
    let n = pts.len();
    let dirs: Vec<DVec2> = pts.windows(2).map(|w| (w[1] - w[0]).normalize_or_zero()).collect();
    let normal = |d: DVec2| DVec2::new(-d.y, d.x) * side;
    let mut out = vec![pts[0] + normal(dirs[0]) * h];
    for i in 1..n - 1 {
        let (d0, d1) = (dirs[i - 1], dirs[i]);
        let (n0, n1) = (normal(d0), normal(d1));
        let turn = d0.perp_dot(d1);
        let dot = n0.dot(n1);
        let p = pts[i];
        // The side is the outer side of the bend when the path turns away from it.
        let outer = turn * side < 0.0;
        let miter = if 1.0 + dot > 1e-9 { Some((n0 + n1) / (1.0 + dot) * h) } else { None };
        if !outer {
            // Inner side: the offset lines cross at the miter point.
            match miter {
                Some(m) => out.push(p + m),
                None => out.extend([p + n0 * h, p + n1 * h]),
            }
            continue;
        }
        match (join, miter) {
            (Join::Miter, Some(m)) if m.length() <= MITER_LIMIT * h => out.push(p + m),
            (Join::Round, _) => {
                let a0 = n0.y.atan2(n0.x);
                let mut sweep = n1.y.atan2(n1.x) - a0;
                // The outer arc turns the same way as the path.
                if turn > 0.0 {
                    sweep = sweep.rem_euclid(2.0 * PI);
                } else {
                    sweep = -((-sweep).rem_euclid(2.0 * PI));
                }
                let steps = ((sweep.abs() / ROUND_STEP).ceil() as usize).max(1);
                for k in 0..=steps {
                    out.push(p + DVec2::from_angle(a0 + sweep * k as f64 / steps as f64) * h);
                }
            }
            _ => out.extend([p + n0 * h, p + n1 * h]),
        }
    }
    out.push(pts[n - 1] + normal(dirs[n - 2]) * h);
    out
}

/// The left and the right edge of a path, each from the start to the end. Empty lists if the
/// path has no length or the width is not positive and finite.
pub fn sides(center: &[DVec2], width: f64, join: Join) -> (Vec<DVec2>, Vec<DVec2>) {
    let pts = dedupe(center);
    if pts.len() < 2 || !width.is_finite() || width <= 0.0 {
        return (Vec::new(), Vec::new());
    }
    (side_points(&pts, width / 2.0, 1.0, join), side_points(&pts, width / 2.0, -1.0, join))
}

/// The outline of a path, counterclockwise and without a repeated first point. `center` is the
/// flattened center line. Returns an empty list if the path has no length or the width is not
/// positive and finite.
pub fn path_outline(center: &[DVec2], width: f64, join: Join) -> Vec<DVec2> {
    let pts = dedupe(center);
    if pts.len() < 2 || !width.is_finite() || width <= 0.0 {
        return Vec::new();
    }
    let h = width / 2.0;
    let mut ring = side_points(&pts, h, 1.0, join);
    let mut right = side_points(&pts, h, -1.0, join);
    right.reverse();
    ring.extend(right);
    let ring = dedupe(&ring);
    let o = ring[0];
    let twice: f64 = (0..ring.len()).map(|i| (ring[i] - o).perp_dot(ring[(i + 1) % ring.len()] - o)).sum();
    if twice < 0.0 {
        return ring.into_iter().rev().collect();
    }
    ring
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::polygon_area;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn straight_path_is_a_rectangle() {
        let r = path_outline(&[v(0.0, 0.0), v(4000.0, 0.0)], 1200.0, Join::Miter);
        assert_eq!(r.len(), 4);
        assert!((polygon_area(&r) - 4000.0 * 1200.0).abs() < 1e-6);
        for p in &r {
            assert!(p.y.abs() == 600.0);
        }
        // Counterclockwise.
        let twice: f64 = (0..4).map(|i| r[i].perp_dot(r[(i + 1) % 4])).sum();
        assert!(twice > 0.0);
    }

    #[test]
    fn collinear_points_keep_the_exact_area() {
        let r = path_outline(&[v(0.0, 0.0), v(1000.0, 0.0), v(3000.0, 0.0), v(3000.0, 0.0)], 500.0, Join::Round);
        assert!((polygon_area(&r) - 3000.0 * 500.0).abs() < 1e-6);
    }

    #[test]
    fn right_angle_joins() {
        let path = [v(0.0, 0.0), v(3000.0, 0.0), v(3000.0, 3000.0)];
        // Miter: two rectangles overlap in a square at the bend, and the outer corner is square.
        let m = path_outline(&path, 1000.0, Join::Miter);
        assert!((polygon_area(&m) - 6.0e6).abs() < 1e-3);
        assert_eq!(m.len(), 6);
        // The outer miter point.
        assert!(m.iter().any(|p| (p.x - 3500.0).abs() < 1e-9 && (p.y + 500.0).abs() < 1e-9));
        // The inner corner.
        assert!(m.iter().any(|p| (p.x - 2500.0).abs() < 1e-9 && (p.y - 500.0).abs() < 1e-9));
        // Bevel cuts the outer corner.
        let b = path_outline(&path, 1000.0, Join::Bevel);
        assert_eq!(b.len(), 7);
        assert!(polygon_area(&b) < polygon_area(&m));
        // Round lies between bevel and miter.
        let r = path_outline(&path, 1000.0, Join::Round);
        assert!(polygon_area(&r) > polygon_area(&b) && polygon_area(&r) < polygon_area(&m));
    }

    #[test]
    fn right_turn_and_left_turn_are_mirror_images() {
        let left = path_outline(&[v(0.0, 0.0), v(3000.0, 0.0), v(3000.0, 3000.0)], 800.0, Join::Round);
        let right = path_outline(&[v(0.0, 0.0), v(3000.0, 0.0), v(3000.0, -3000.0)], 800.0, Join::Round);
        assert!((polygon_area(&left) - polygon_area(&right)).abs() < 1e-6);
        assert_eq!(left.len(), right.len());
    }

    #[test]
    fn sharp_miter_falls_back_to_bevel() {
        // A hairpin bend: the miter would be very long.
        let path = [v(0.0, 0.0), v(5000.0, 0.0), v(0.0, 300.0)];
        let m = path_outline(&path, 400.0, Join::Miter);
        assert!(m.iter().all(|p| p.x < 5200.0), "no long spike: {m:?}");
        // A reversal on the same line (180 degrees) does not break.
        let back = path_outline(&[v(0.0, 0.0), v(100.0, 0.0), v(0.0, 0.0)], 20.0, Join::Miter);
        assert!(back.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
    }

    #[test]
    fn bad_input_gives_no_outline() {
        assert!(path_outline(&[v(0.0, 0.0)], 100.0, Join::Miter).is_empty());
        assert!(path_outline(&[v(1.0, 1.0), v(1.0, 1.0)], 100.0, Join::Miter).is_empty());
        assert!(path_outline(&[v(0.0, 0.0), v(1.0, 0.0)], 0.0, Join::Miter).is_empty());
        assert!(path_outline(&[v(0.0, 0.0), v(1.0, 0.0)], f64::NAN, Join::Miter).is_empty());
        assert!(path_outline(&[], 5.0, Join::Round).is_empty());
    }

    #[test]
    fn helpers() {
        let (l, r) = sides(&[v(0.0, 0.0), v(10.0, 0.0)], 4.0, Join::Miter);
        assert_eq!((l, r), (vec![v(0.0, 2.0), v(10.0, 2.0)], vec![v(0.0, -2.0), v(10.0, -2.0)]));
        assert_eq!(sides(&[v(0.0, 0.0)], 4.0, Join::Miter), (vec![], vec![]));
        assert_eq!(dedupe(&[v(0.0, 0.0), v(0.0, 0.0), v(1.0, 0.0)]).len(), 2);
        assert_eq!(length(&[v(0.0, 0.0), v(3.0, 4.0), v(3.0, 5.0)]), 6.0);
        for j in Join::ALL {
            assert_eq!(Join::from_name(j.id()), Some(j));
        }
        assert_eq!(Join::from_name("x"), None);
    }
}
