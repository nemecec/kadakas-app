//! Contour lines of a TIN.

use std::collections::HashMap;

use glam::DVec2;

use crate::SiteError;
use crate::tin::Tin;

/// Most contour levels that one call makes. A smaller interval is refused.
pub const MAX_LEVELS: i64 = 20_000;

/// Interval and datum of a contour map. Lengths are in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContourSpec {
    pub minor_interval: f64,
    pub major_interval: f64,
    /// Levels are the datum plus a whole number of minor intervals.
    pub datum: f64,
    /// Round the corners of the lines.
    pub smooth: bool,
}

/// One contour line.
#[derive(Clone, Debug, PartialEq)]
pub struct Contour {
    pub level: f64,
    /// True if the level is a whole number of major intervals from the datum.
    pub major: bool,
    pub closed: bool,
    /// The points of the line. A closed line does not repeat its first point.
    pub points: Vec<DVec2>,
}

/// Key of a triangle edge: its two vertex numbers, smaller first.
type EdgeKey = (u32, u32);

fn key(a: u32, b: u32) -> EdgeKey {
    (a.min(b), a.max(b))
}

/// A piece of a contour inside one triangle: it joins two edges.
struct Seg {
    ends: [EdgeKey; 2],
    pts: [DVec2; 2],
}

/// The point on edge `a`-`b` where the surface reaches `level`.
fn crossing(tin: &Tin, a: u32, b: u32, level: f64) -> DVec2 {
    let (pa, pb) = (tin.vertices[a as usize], tin.vertices[b as usize]);
    let t = (level - pa.z) / (pb.z - pa.z);
    pa.truncate().lerp(pb.truncate(), t)
}

/// Makes the contours of `tin`, from the lowest to the highest level.
/// A vertex that lies exactly on a level counts as above it, so no line runs through a vertex.
pub fn contours(tin: &Tin, spec: &ContourSpec) -> Result<Vec<Contour>, SiteError> {
    let ok = |v: f64| v.is_finite() && v > 0.0;
    if !ok(spec.minor_interval) || !spec.major_interval.is_finite() || !spec.datum.is_finite() {
        return Err(SiteError::BadInterval);
    }
    let (zlo, zhi) = tin.z_range();
    // Count in f64 first: a tiny interval would overflow the integers.
    let (f_lo, f_hi) = ((zlo - spec.datum) / spec.minor_interval, (zhi - spec.datum) / spec.minor_interval);
    if !(f_hi.ceil() - f_lo.floor()).is_finite() || f_hi.ceil() - f_lo.floor() > MAX_LEVELS as f64 {
        return Err(SiteError::TooManyContours);
    }
    let (k_lo, k_hi) = (f_lo.floor() as i64, f_hi.ceil() as i64);
    let level = |k: i64| spec.datum + k as f64 * spec.minor_interval;
    let n_levels = (k_hi - k_lo + 1) as usize;
    let mut per_level: Vec<Vec<Seg>> = (0..n_levels).map(|_| Vec::new()).collect();
    for t in &tin.triangles {
        let z = t.map(|v| tin.vertices[v as usize].z);
        let (tlo, thi) = (z[0].min(z[1]).min(z[2]), z[0].max(z[1]).max(z[2]));
        if tlo == thi {
            continue;
        }
        let first = (((tlo - spec.datum) / spec.minor_interval).floor() as i64 - 1).max(k_lo);
        let last = (((thi - spec.datum) / spec.minor_interval).ceil() as i64 + 1).min(k_hi);
        for k in first..=last {
            let l = level(k);
            if !(tlo < l && l <= thi) {
                continue;
            }
            // The edges where one end is at or above the level and the other is below.
            let mut cut = Vec::with_capacity(2);
            for e in 0..3 {
                let (a, b) = (t[e], t[(e + 1) % 3]);
                if (z[e] >= l) != (z[(e + 1) % 3] >= l) {
                    cut.push((key(a, b), crossing(tin, a, b, l)));
                }
            }
            if let [(ea, pa), (eb, pb)] = cut[..] {
                per_level[(k - k_lo) as usize].push(Seg { ends: [ea, eb], pts: [pa, pb] });
            }
        }
    }
    let mut out = Vec::new();
    for (i, segs) in per_level.iter().enumerate() {
        let l = level(k_lo + i as i64);
        let major = spec.major_interval > 0.0 && {
            let r = ((l - spec.datum) / spec.major_interval).rem_euclid(1.0);
            r < 1e-9 || 1.0 - r < 1e-9
        };
        for (points, closed) in chain(segs) {
            let (points, closed) = if spec.smooth { (smooth(&points, closed), closed) } else { (points, closed) };
            out.push(Contour { level: l, major, closed, points });
        }
    }
    Ok(out)
}

/// Joins the pieces of one level into lines. Two pieces join where they share an edge.
fn chain(segs: &[Seg]) -> Vec<(Vec<DVec2>, bool)> {
    let mut at_edge: HashMap<EdgeKey, [Option<usize>; 2]> = HashMap::with_capacity(segs.len() * 2);
    for (i, s) in segs.iter().enumerate() {
        for e in s.ends {
            let slot = at_edge.entry(e).or_default();
            if slot[0].is_none() {
                slot[0] = Some(i);
            } else {
                slot[1] = Some(i);
            }
        }
    }
    let other = |edge: EdgeKey, seg: usize| -> Option<usize> {
        let slot = at_edge[&edge];
        slot.into_iter().flatten().find(|s| *s != seg)
    };
    let mut used = vec![false; segs.len()];
    let mut lines = Vec::new();
    // Open lines first: they start at a piece that has a free end (on the edge of the surface).
    // Closed loops are what remains.
    for pass in 0..2 {
        for start in 0..segs.len() {
            if used[start] {
                continue;
            }
            let free_end = (0..2).find(|&e| other(segs[start].ends[e], start).is_none());
            if pass == 0 && free_end.is_none() {
                continue;
            }
            let e_in = free_end.unwrap_or(0);
            used[start] = true;
            let mut pts = vec![segs[start].pts[e_in], segs[start].pts[1 - e_in]];
            let (mut cur, mut out_edge) = (start, segs[start].ends[1 - e_in]);
            let mut closed = false;
            while let Some(next) = other(out_edge, cur) {
                if used[next] {
                    closed = next == start;
                    break;
                }
                used[next] = true;
                let s = &segs[next];
                let e = if s.ends[0] == out_edge { 0 } else { 1 };
                pts.push(s.pts[1 - e]);
                out_edge = s.ends[1 - e];
                cur = next;
            }
            if closed {
                // The last point repeats the first one.
                pts.pop();
            }
            lines.push((pts, closed));
        }
    }
    lines
}

/// Rounds the corners with Chaikin's corner cutting, twice. An open line keeps its end points.
pub fn smooth(points: &[DVec2], closed: bool) -> Vec<DVec2> {
    let mut pts = points.to_vec();
    for _ in 0..2 {
        let n = pts.len();
        if n < 3 {
            break;
        }
        let mut next = Vec::with_capacity(n * 2);
        if !closed {
            next.push(pts[0]);
        }
        let segs = if closed { n } else { n - 1 };
        for i in 0..segs {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            next.push(a.lerp(b, 0.25));
            next.push(a.lerp(b, 0.75));
        }
        if !closed {
            next.push(pts[n - 1]);
        }
        pts = next;
    }
    pts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tin::{SourceData, Tin};
    use glam::DVec3;

    fn spec(minor: f64, major: f64) -> ContourSpec {
        ContourSpec { minor_interval: minor, major_interval: major, datum: 0.0, smooth: false }
    }

    fn grid(n: usize, step: f64, f: impl Fn(f64, f64) -> f64) -> Tin {
        let mut pts = Vec::new();
        for i in 0..n {
            for j in 0..n {
                let (x, y) = (i as f64 * step, j as f64 * step);
                pts.push(DVec3::new(x, y, f(x, y)));
            }
        }
        Tin::build(&SourceData { points: pts, lines: vec![] }).unwrap()
    }

    #[test]
    fn plane_gives_straight_parallel_open_lines() {
        // z = x / 10 on a 100 by 100 square: levels 1..=9 at x = 10..=90, and 0 and 10 at the edge.
        let tin = grid(11, 10.0, |x, _| x / 10.0);
        let cs = contours(&tin, &spec(1.0, 5.0)).unwrap();
        // Level 0 is at or below every vertex, so no line. Level 10 touches the far edge: it counts
        // as above there, so a line runs along x = 100 between the last two columns.
        let levels: Vec<f64> = cs.iter().map(|c| c.level).collect();
        assert_eq!(levels, (1..=10).map(f64::from).collect::<Vec<_>>());
        for c in &cs {
            assert!(!c.closed);
            assert!(c.points.iter().all(|p| (p.x - c.level * 10.0).abs() < 1e-9), "level {}", c.level);
            let (lo, hi) = c.points.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
            assert!((lo, hi) == (0.0, 100.0), "spans the model: {lo} {hi}");
        }
        let majors: Vec<f64> = cs.iter().filter(|c| c.major).map(|c| c.level).collect();
        assert_eq!(majors, vec![5.0, 10.0]);
    }

    #[test]
    fn cone_gives_closed_circles() {
        // A pyramid-like hill: z = 10 - distance from the middle (L-infinity), steps of 1.
        let tin = grid(21, 5.0, |x, y| 10.0 - (x - 50.0).abs().max((y - 50.0).abs()) / 5.0);
        let cs = contours(&tin, &spec(2.0, 4.0)).unwrap();
        assert!(!cs.is_empty());
        assert!(cs.iter().all(|c| c.closed && c.points.len() >= 4));
        let levels: Vec<f64> = cs.iter().map(|c| c.level).collect();
        assert_eq!(levels, vec![2.0, 4.0, 6.0, 8.0, 10.0].into_iter().filter(|l| *l <= 10.0).collect::<Vec<f64>>());
        // Higher levels are smaller loops.
        let extent = |c: &Contour| c.points.iter().map(|p| (p.x - 50.0).abs()).fold(0.0, f64::max);
        assert!(cs.windows(2).all(|w| extent(&w[0]) > extent(&w[1])));
    }

    #[test]
    fn flat_surface_and_levels_at_vertices_give_nothing_odd() {
        let tin = grid(3, 10.0, |_, _| 5.0);
        assert!(contours(&tin, &spec(1.0, 5.0)).unwrap().is_empty());
        // Every vertex is exactly on a level: the line follows the step between rows, no gaps.
        let tin = grid(3, 10.0, |_, y| if y > 5.0 { 1.0 } else { 0.0 });
        let cs = contours(&tin, &spec(1.0, 5.0)).unwrap();
        assert_eq!(cs.len(), 1);
    }

    #[test]
    fn datum_and_intervals_are_checked() {
        let tin = grid(3, 10.0, |x, _| x);
        let c = contours(&tin, &ContourSpec { datum: 0.5, ..spec(5.0, 10.0) }).unwrap();
        assert_eq!(c.iter().map(|c| c.level).collect::<Vec<_>>(), vec![0.5, 5.5, 10.5, 15.5]);
        assert_eq!(contours(&tin, &spec(0.0, 5.0)).unwrap_err(), SiteError::BadInterval);
        assert_eq!(contours(&tin, &spec(f64::NAN, 5.0)).unwrap_err(), SiteError::BadInterval);
        assert_eq!(contours(&tin, &spec(1.0, f64::NAN)).unwrap_err(), SiteError::BadInterval);
        assert_eq!(contours(&tin, &ContourSpec { datum: f64::INFINITY, ..spec(1.0, 5.0) }).unwrap_err(), SiteError::BadInterval);
        let steep = grid(3, 10.0, |x, _| x * 1e6);
        assert_eq!(contours(&steep, &spec(1.0, 5.0)).unwrap_err(), SiteError::TooManyContours);
        assert_eq!(contours(&steep, &spec(1e-300, 5.0)).unwrap_err(), SiteError::TooManyContours);
        assert_eq!(contours(&steep, &spec(f64::MIN_POSITIVE, 5.0)).unwrap_err(), SiteError::TooManyContours);
        // A major interval of zero means no major lines.
        assert!(contours(&tin, &spec(5.0, 0.0)).unwrap().iter().all(|c| !c.major));
    }

    #[test]
    fn smoothing_keeps_ends_and_closedness() {
        let line = vec![DVec2::ZERO, DVec2::new(10.0, 0.0), DVec2::new(10.0, 10.0)];
        let s = smooth(&line, false);
        assert_eq!((s[0], *s.last().unwrap()), (line[0], line[2]));
        assert!(s.len() > line.len());
        let ring = vec![DVec2::ZERO, DVec2::new(10.0, 0.0), DVec2::new(10.0, 10.0), DVec2::new(0.0, 10.0)];
        assert_eq!(smooth(&ring, true).len(), 4 * 4);
        assert_eq!(smooth(&line[..2], false), line[..2].to_vec());
        let tin = grid(11, 10.0, |x, y| 10.0 - ((x - 50.0).powi(2) + (y - 50.0).powi(2)).sqrt() / 5.0);
        let mut sp = spec(2.0, 4.0);
        let plain = contours(&tin, &sp).unwrap();
        sp.smooth = true;
        let rounded = contours(&tin, &sp).unwrap();
        assert_eq!(plain.len(), rounded.len());
        let total = |cs: &[Contour]| cs.iter().map(|c| c.points.len()).sum::<usize>();
        assert!(total(&rounded) > total(&plain));
    }
}
