//! The cut: where a vertical plane crosses the triangles of an object, joined to rings (cut faces
//! of solids) and open chains (the profile of a surface such as the terrain).

use std::collections::HashMap;

use glam::DVec2;
use kadakas_core::geom::{point_in_polygon, signed_area};

/// Points closer than this (mm) are one point.
pub const SNAP: f64 = 0.05;

/// A line of connected cut points.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub points: Vec<DVec2>,
    pub closed: bool,
}

/// Merges points that lie within `SNAP` of each other.
struct Snapper {
    cells: HashMap<(i64, i64), Vec<usize>>,
    points: Vec<DVec2>,
}

impl Snapper {
    fn cell(p: DVec2) -> (i64, i64) {
        (
            (p.x / (SNAP * 4.0)).floor() as i64,
            (p.y / (SNAP * 4.0)).floor() as i64,
        )
    }

    fn id(&mut self, p: DVec2) -> usize {
        let (cx, cy) = Self::cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(list) = self.cells.get(&(cx + dx, cy + dy)) {
                    for &i in list {
                        if self.points[i].distance(p) <= SNAP {
                            return i;
                        }
                    }
                }
            }
        }
        self.points.push(p);
        let i = self.points.len() - 1;
        self.cells.entry((cx, cy)).or_default().push(i);
        i
    }
}

/// Joins segments that share end points into chains. Segments of zero length are dropped.
pub fn chain(segments: &[(DVec2, DVec2)]) -> Vec<Chain> {
    let mut snap = Snapper {
        cells: HashMap::new(),
        points: Vec::new(),
    };
    let mut edges: Vec<(usize, usize)> = Vec::with_capacity(segments.len());
    for (a, b) in segments {
        let (i, j) = (snap.id(*a), snap.id(*b));
        if i != j {
            edges.push((i, j));
        }
    }
    let mut at: Vec<Vec<usize>> = vec![Vec::new(); snap.points.len()];
    for (e, (i, j)) in edges.iter().enumerate() {
        at[*i].push(e);
        at[*j].push(e);
    }
    let mut used = vec![false; edges.len()];
    let mut out = Vec::new();
    // Open chains start at a point with one edge (or more than two); the rest are rings.
    let mut starts: Vec<usize> = (0..at.len())
        .filter(|v| at[*v].len() % 2 == 1 || at[*v].len() > 2)
        .collect();
    starts.extend((0..at.len()).filter(|v| at[*v].len() == 2));
    for start in starts {
        while let Some(&first) = at[start].iter().find(|e| !used[**e]) {
            let mut pts = vec![snap.points[start]];
            let mut cur = start;
            let mut e = first;
            loop {
                used[e] = true;
                let (i, j) = edges[e];
                cur = if i == cur { j } else { i };
                pts.push(snap.points[cur]);
                match at[cur].iter().find(|x| !used[**x]) {
                    Some(&next) => e = next,
                    None => break,
                }
            }
            let closed = cur == start && pts.len() > 3;
            if closed {
                pts.pop();
            }
            out.push(Chain {
                points: pts,
                closed,
            });
        }
    }
    out
}

/// Removes points on a straight run, with a tolerance in mm.
pub fn simplify(points: &[DVec2], closed: bool, tol: f64) -> Vec<DVec2> {
    let mut pts: Vec<DVec2> = points.to_vec();
    loop {
        let n = pts.len();
        let min = if closed { 3 } else { 2 };
        if n <= min {
            return pts;
        }
        let range = if closed { 0..n } else { 1..n - 1 };
        // The point that lies closest to the line of its neighbours goes first. The first point
        // within the tolerance is not enough: next to a short edge it can be a real corner (a
        // 0.07 mm jog beside a 5 m edge), and dropping it tilts the long edge and loses area.
        let drop = range
            .into_iter()
            .filter_map(|i| {
                let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
                let ac = c - a;
                let len = ac.length();
                if len < 1e-12 {
                    return (b.distance(a) <= tol).then_some((i, b.distance(a)));
                }
                let off = ac.perp_dot(b - a).abs() / len;
                let along = (b - a).dot(ac) / len;
                (off <= tol && along >= -tol && along <= len + tol).then_some((i, off))
            })
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .map(|(i, _)| i);
        match drop {
            Some(i) => {
                pts.remove(i);
            }
            None => return pts,
        }
    }
}

/// Sets the winding of each ring from how deep it lies in the others: outer rings run
/// counterclockwise, holes run clockwise. Rings with no area are dropped.
pub fn orient(rings: Vec<Vec<DVec2>>) -> Vec<Vec<DVec2>> {
    let rings: Vec<Vec<DVec2>> = rings
        .into_iter()
        .filter(|r| r.len() >= 3 && signed_area(r).abs() > 5e-7)
        .collect();
    // Nesting is only worked out for a sensible number of rings.
    let nest = rings.len() <= 2000;
    rings
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let depth = if nest {
                // A point of the ring that is not shared: the middle of its first edge.
                let probe = (r[0] + r[1]) / 2.0 + (r[1] - r[0]).perp().normalize_or_zero() * 1e-6;
                let probe = if point_in_polygon(probe, r) {
                    probe
                } else {
                    (r[0] + r[1]) / 2.0 - (r[1] - r[0]).perp().normalize_or_zero() * 1e-6
                };
                rings
                    .iter()
                    .enumerate()
                    .filter(|(j, o)| *j != i && point_in_polygon(probe, o))
                    .count()
            } else {
                0
            };
            let mut r = r.clone();
            let ccw = signed_area(&r) > 0.0;
            if ccw != (depth % 2 == 0) {
                r.reverse();
            }
            r
        })
        .collect()
}

/// Clips a ring to `0 <= x <= max` (Sutherland and Hodgman). The result may be empty.
pub fn clip_ring(ring: &[DVec2], max: f64) -> Vec<DVec2> {
    let mut pts = ring.to_vec();
    for (keep_ge, limit) in [(true, 0.0), (false, max)] {
        let inside = |p: DVec2| if keep_ge { p.x >= limit } else { p.x <= limit };
        let n = pts.len();
        if n == 0 {
            break;
        }
        let mut out = Vec::with_capacity(n + 2);
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            match (inside(a), inside(b)) {
                (true, true) => out.push(b),
                (true, false) => out.push(cross_x(a, b, limit)),
                (false, true) => {
                    out.push(cross_x(a, b, limit));
                    out.push(b);
                }
                (false, false) => {}
            }
        }
        pts = out;
    }
    if pts.len() < 3 { Vec::new() } else { pts }
}

fn cross_x(a: DVec2, b: DVec2, x: f64) -> DVec2 {
    let t = if (b.x - a.x).abs() < 1e-300 {
        0.0
    } else {
        (x - a.x) / (b.x - a.x)
    };
    DVec2::new(x, a.y + (b.y - a.y) * t)
}

/// Clips an open line to `0 <= x <= max`. A line that leaves and comes back gives several pieces.
pub fn clip_line(points: &[DVec2], max: f64) -> Vec<Vec<DVec2>> {
    let mut out: Vec<Vec<DVec2>> = Vec::new();
    let mut cur: Vec<DVec2> = Vec::new();
    for w in points.windows(2) {
        match clip_segment_x(w[0], w[1], max) {
            Some((a, b)) => {
                if cur.last().is_none_or(|l| l.distance(a) > 1e-9) {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.push(a);
                }
                cur.push(b);
            }
            None => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.retain(|c| c.len() >= 2);
    out
}

/// The part of a segment with `0 <= x <= max`.
pub fn clip_segment_x(a: DVec2, b: DVec2, max: f64) -> Option<(DVec2, DVec2)> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let dx = b.x - a.x;
    for (p, q) in [(-dx, a.x), (dx, max - a.x)] {
        if p.abs() < 1e-300 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    if t0 > t1 {
        return None;
    }
    Some((a.lerp(b, t0), a.lerp(b, t1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn segments_join_to_a_ring() {
        let sq = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)];
        // Shuffled, with small errors at the joints.
        let segs = vec![
            (sq[2], sq[3]),
            (sq[0] + v(0.01, 0.0), sq[1]),
            (sq[1] + v(0.0, 0.01), sq[2]),
            (sq[3], sq[0]),
        ];
        let c = chain(&segs);
        assert_eq!(c.len(), 1);
        assert!(c[0].closed);
        assert_eq!(c[0].points.len(), 4);
        assert!((signed_area(&c[0].points).abs() - 100.0).abs() < 0.5);
    }

    #[test]
    fn open_lines_stay_open() {
        let segs = vec![
            (v(0.0, 0.0), v(5.0, 1.0)),
            (v(10.0, 3.0), v(5.0, 1.0)),
            (v(10.0, 3.0), v(10.0, 3.0)),
        ];
        let c = chain(&segs);
        assert_eq!(c.len(), 1);
        assert!(!c[0].closed);
        assert_eq!(c[0].points.len(), 3);
        assert!(chain(&[]).is_empty());
    }

    #[test]
    fn simplify_drops_the_straightest_point_first() {
        // A long bottom edge, a corner and a 0.07 mm jog on a vertical wall: the jog is on the
        // wall line, the corner is not. Dropping the corner would tilt the 5 m edge.
        let ring = [
            v(5000.0, -216.10),
            v(0.0, -216.10),
            v(0.0, -216.03),
            v(0.0, 0.0),
            v(5000.0, 0.0),
        ];
        let r = simplify(&ring, true, 0.1);
        let area = kadakas_core::geom::signed_area(&r).abs();
        assert!((area - 5000.0 * 216.10).abs() < 1.0, "{r:?} {area}");
    }

    #[test]
    fn simplify_removes_straight_points() {
        let r = simplify(
            &[
                v(0.0, 0.0),
                v(5.0, 0.0),
                v(10.0, 0.0),
                v(10.0, 10.0),
                v(0.0, 10.0),
            ],
            true,
            0.01,
        );
        assert_eq!(r.len(), 4);
        let l = simplify(&[v(0.0, 0.0), v(5.0, 0.001), v(10.0, 0.0)], false, 0.01);
        assert_eq!(l.len(), 2);
        let keep = simplify(&[v(0.0, 0.0), v(5.0, 5.0), v(10.0, 0.0)], false, 0.01);
        assert_eq!(keep.len(), 3);
        // A spike that goes out and back is not straight.
        let spike = simplify(
            &[v(0.0, 0.0), v(10.0, 0.0), v(5.0, 0.0), v(5.0, 5.0)],
            false,
            0.01,
        );
        assert!(spike.len() >= 3);
    }

    #[test]
    fn holes_run_clockwise() {
        let outer = [v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(0.0, 100.0)];
        // The hole starts counterclockwise and must be turned.
        let hole = vec![v(40.0, 40.0), v(60.0, 40.0), v(60.0, 60.0), v(40.0, 60.0)];
        let cw_outer: Vec<DVec2> = outer.iter().rev().copied().collect();
        let rings = orient(vec![cw_outer, hole, vec![v(0.0, 0.0), v(1.0, 1.0)]]);
        assert_eq!(rings.len(), 2);
        assert!(signed_area(&rings[0]) > 0.0);
        assert!(signed_area(&rings[1]) < 0.0);
    }

    #[test]
    fn rings_clip_to_the_width() {
        let r = vec![v(-5.0, 0.0), v(5.0, 0.0), v(5.0, 10.0), v(-5.0, 10.0)];
        let c = clip_ring(&r, 3.0);
        let a = signed_area(&c).abs();
        assert!((a - 30.0).abs() < 1e-9);
        assert!(clip_ring(&r, -1.0).is_empty());
        assert!(clip_ring(&[], 1.0).is_empty());
    }

    #[test]
    fn lines_clip_to_the_width() {
        let line = [
            v(-2.0, 0.0),
            v(2.0, 4.0),
            v(8.0, 4.0),
            v(12.0, 0.0),
            v(14.0, 0.0),
        ];
        let pieces = clip_line(&line, 10.0);
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0][0], v(0.0, 2.0));
        assert!((pieces[0].last().unwrap().x - 10.0).abs() < 1e-9);
        // Leaves and comes back: two pieces.
        let two = clip_line(
            &[v(1.0, 0.0), v(-3.0, 0.0), v(-3.0, 1.0), v(2.0, 1.0)],
            10.0,
        );
        assert_eq!(two.len(), 2);
        assert!(clip_line(&[v(20.0, 0.0), v(30.0, 0.0)], 10.0).is_empty());
        assert!(clip_segment_x(v(0.0, 0.0), v(10.0, 0.0), 5.0).is_some());
        // A vertical segment outside the range.
        assert!(clip_segment_x(v(-1.0, 0.0), v(-1.0, 5.0), 5.0).is_none());
        assert!(clip_segment_x(v(1.0, 0.0), v(1.0, 5.0), 5.0).is_some());
    }
}
