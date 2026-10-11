//! The outline of a path hardscape: a center line with a width, turned into a polygon.
//!
//! Each end is flat (a butt cap). At a bend, the outer side gets a join (miter, round or
//! bevel). The inner side meets at the point where the two offset lines cross.

use std::f64::consts::PI;

use glam::DVec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use serde::{Deserialize, Serialize};

use crate::geom;

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
        Self::ALL
            .into_iter()
            .find(|j| j.id().eq_ignore_ascii_case(s.trim()))
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
    geom::dedupe_points(points, SAME)
}

/// Length of a polyline.
pub fn length(points: &[DVec2]) -> f64 {
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// One side of the outline, from the start of the path to its end. `side` is 1 for the left of
/// the travel direction and -1 for the right.
fn side_points(pts: &[DVec2], h: f64, side: f64) -> Vec<DVec2> {
    let n = pts.len();
    let dirs: Vec<DVec2> = pts
        .windows(2)
        .map(|w| (w[1] - w[0]).normalize_or_zero())
        .collect();
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
        let miter = if 1.0 + dot > 1e-9 {
            Some((n0 + n1) / (1.0 + dot) * h)
        } else {
            None
        };
        if !outer {
            // Inner side: the offset lines cross at the miter point.
            match miter {
                Some(m) => out.push(p + m),
                None => out.extend([p + n0 * h, p + n1 * h]),
            }
            continue;
        }
        match miter {
            Some(m) if m.length() <= MITER_LIMIT * h => out.push(p + m),
            _ => out.extend([p + n0 * h, p + n1 * h]),
        }
    }
    out.push(pts[n - 1] + normal(dirs[n - 2]) * h);
    out
}

/// The left and the right edge of a path, each from the start to the end. Empty lists if the
/// path has no length or the width is not positive and finite.
pub fn sides(center: &[DVec2], width: f64) -> (Vec<DVec2>, Vec<DVec2>) {
    let pts = dedupe(center);
    if pts.len() < 2 || !width.is_finite() || width <= 0.0 {
        return (Vec::new(), Vec::new());
    }
    (
        side_points(&pts, width / 2.0, 1.0),
        side_points(&pts, width / 2.0, -1.0),
    )
}

/// Alignment of the drawn center line to the path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    /// The line is the middle of the path.
    #[default]
    Center,
    /// The line is the left edge of the path, so the path lies to the right of the line.
    Left,
    /// The line is the right edge of the path, so the path lies to the left of the line.
    Right,
}

impl Align {
    pub const ALL: [Align; 3] = [Align::Center, Align::Left, Align::Right];

    pub fn id(self) -> &'static str {
        match self {
            Align::Center => "center",
            Align::Left => "left",
            Align::Right => "right",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|a| a.id().eq_ignore_ascii_case(s.trim()))
    }
}

/// The center line moved sideways by `shift` (positive: to the left of the travel direction).
/// Bends are mitered, so the result can cross itself on a tight curve. The outline builder
/// accepts that.
pub fn shifted(center: &[DVec2], shift: f64) -> Vec<DVec2> {
    let pts = dedupe(center);
    if pts.len() < 2 || shift == 0.0 || !shift.is_finite() {
        return pts;
    }
    side_points(&pts, shift.abs(), shift.signum())
}

/// The outline of a path as one counterclockwise ring without a repeated first point. `center` is
/// the flattened center line. A closed path has no ends, and its outline is a ring with a hole,
/// joined by a bridge of zero width. The stroke is built as a union of all pieces, so tight
/// curves and short legs make no loops. Returns an empty list if the path has no length or the
/// width is not positive and finite.
pub fn path_outline(center: &[DVec2], width: f64, join: Join, closed: bool) -> Vec<DVec2> {
    path_outline_within(center, width, join, closed, WORK_BUDGET)
}

/// Work units that one `path_outline` call may spend on joining holes to the ring. One unit is
/// one segment test or one point pair. 20 million units take well under a second, and a normal
/// path needs a few thousand. A path that needs more gives an empty outline, the same answer as a
/// path without length, which every caller already handles.
const WORK_BUDGET: u64 = 20_000_000;

/// The work that a call may spend, counted down. `out` is set when a request was too big.
struct Budget {
    left: std::cell::Cell<u64>,
    out: std::cell::Cell<bool>,
}

impl Budget {
    fn new(units: u64) -> Self {
        Self {
            left: units.into(),
            out: false.into(),
        }
    }

    /// Takes `units` of work. False, and the budget is out, if not enough is left. The work is
    /// counted before it is done, so the total never goes over the budget.
    fn take(&self, units: usize) -> bool {
        let units = units as u64;
        let left = self.left.get();
        if units > left {
            self.left.set(0);
            self.out.set(true);
            return false;
        }
        self.left.set(left - units);
        true
    }

    fn spent(&self, total: u64) -> u64 {
        total - self.left.get()
    }
}

/// `path_outline` with an explicit work budget.
fn path_outline_within(
    center: &[DVec2],
    width: f64,
    join: Join,
    closed: bool,
    work: u64,
) -> Vec<DVec2> {
    let mut pts = dedupe(center);
    if closed && pts.len() > 1 && pts[0].distance(pts[pts.len() - 1]) <= SAME {
        pts.pop();
    }
    if pts.len() < if closed { 3 } else { 2 } || !width.is_finite() || width <= 0.0 {
        return Vec::new();
    }
    let h = width / 2.0;
    if pts.len() == 2 && !closed {
        // One straight leg is a rectangle. No rounding, so the area is exactly length times width.
        let nrm = (pts[1] - pts[0]).normalize_or_zero();
        let nrm = DVec2::new(-nrm.y, nrm.x) * h;
        return vec![pts[0] - nrm, pts[1] - nrm, pts[1] + nrm, pts[0] + nrm];
    }
    // Work relative to the first point, which keeps the precision for survey coordinates.
    let o = pts[0];
    let pts: Vec<DVec2> = pts.iter().map(|p| *p - o).collect();
    let pieces = path_pieces(&pts, h, join, closed);
    let shapes = pieces.overlay(
        &Vec::<Vec<[f64; 2]>>::new(),
        OverlayRule::Union,
        FillRule::NonZero,
    );
    // A path is connected, so the union is one shape. Where two parts only touch, the union can
    // still return them as two shapes. All shapes join with bridges of zero width, so no area is lost.
    let area = |r: &Vec<[f64; 2]>| -> f64 {
        (0..r.len())
            .map(|i| r[i][0] * r[(i + 1) % r.len()][1] - r[(i + 1) % r.len()][0] * r[i][1])
            .sum::<f64>()
            / 2.0
    };
    let mut shapes = shapes;
    shapes.sort_by(|a, b| area(&b[0]).abs().total_cmp(&area(&a[0]).abs()));
    let to_p = |v: &[f64; 2]| DVec2::new(v[0] + o.x, v[1] + o.y);
    // Every contour after the first one is a hole, or a part that only touches. Each is bridged to
    // the ring, and must not be crossed by the bridges of the others.
    let mut contours: Vec<Vec<DVec2>> = shapes
        .iter()
        .flat_map(|shape| shape.iter().map(|c| c.iter().map(to_p).collect()))
        .collect();
    if contours.is_empty() {
        return Vec::new();
    }
    let budget = Budget::new(work);
    let mut ring = contours.remove(0);
    for k in 0..contours.len() {
        let (done, rest) = contours.split_at_mut(k + 1);
        ring = bridge_among(ring, std::mem::take(&mut done[k]), rest, &budget);
        if budget.out.get() {
            // A bridge that is not checked can cross an edge. No outline is better than a wrong one.
            return Vec::new();
        }
    }
    let ring = dedupe(&ring);
    if ring.len() < 3 {
        return Vec::new();
    }
    let o = ring[0];
    let twice: f64 = (0..ring.len())
        .map(|i| (ring[i] - o).perp_dot(ring[(i + 1) % ring.len()] - o))
        .sum();
    if twice < 0.0 {
        ring.into_iter().rev().collect()
    } else {
        ring
    }
}

/// The parts of a stroke: one rectangle for each leg, and at each bend a wedge (miter, bevel or
/// round) that fills the outer side. The union of the parts is the stroke. Every part is counterclockwise.
fn path_pieces(pts: &[DVec2], h: f64, join: Join, closed: bool) -> Vec<Vec<[f64; 2]>> {
    let n = pts.len();
    let legs = if closed { n } else { n - 1 };
    let perp = |d: DVec2| DVec2::new(-d.y, d.x);
    let dir = |i: usize| (pts[(i + 1) % n] - pts[i]).normalize_or_zero();
    let ccw = |mut ring: Vec<DVec2>| -> Vec<[f64; 2]> {
        let o = ring[0];
        let twice: f64 = (0..ring.len())
            .map(|i| (ring[i] - o).perp_dot(ring[(i + 1) % ring.len()] - o))
            .sum();
        if twice < 0.0 {
            ring.reverse();
        }
        ring.into_iter().map(|p| [p.x, p.y]).collect()
    };
    let mut out = Vec::with_capacity(2 * legs);
    // Each leg reaches a hair into its neighbors, so that two legs overlap in an area and never
    // only touch in a point. Then the union is one shape. The ends of the path stay exact.
    let reach = h * 1e-6;
    for i in 0..legs {
        let (a, b, d) = (pts[i], pts[(i + 1) % n], dir(i));
        let nrm = perp(d) * h;
        let (a, b) = (
            if i > 0 || closed { a - d * reach } else { a },
            if i + 1 < legs || closed {
                b + d * reach
            } else {
                b
            },
        );
        out.push(ccw(vec![a - nrm, b - nrm, b + nrm, a + nrm]));
    }
    let bends = if closed { 0..n } else { 1..n - 1 };
    for i in bends {
        let (d0, d1) = (dir((i + n - 1) % n), dir(i));
        let (turn, dot) = (d0.perp_dot(d1), d0.dot(d1));
        if turn.abs() < 1e-12 && dot > 0.0 {
            continue;
        }
        let p = pts[i];
        if turn.abs() < 1e-12 {
            // A reversal: the join is a half disc, and a full disc does no harm.
            out.push(ccw((0..32)
                .map(|k| p + DVec2::from_angle(std::f64::consts::TAU * k as f64 / 32.0) * h)
                .collect()));
            continue;
        }
        // The outer side of the bend is the side the path turns away from.
        let side = if turn > 0.0 { -1.0 } else { 1.0 };
        let (n0, n1) = (perp(d0) * side, perp(d1) * side);
        let (a, b) = (p + n0 * h, p + n1 * h);
        let tip = (1.0 + n0.dot(n1) > 1e-9)
            .then(|| p + (n0 + n1) / (1.0 + n0.dot(n1)) * h)
            .filter(|t| t.distance(p) <= MITER_LIMIT * h);
        match (join, tip) {
            (Join::Miter, Some(t)) => out.push(ccw(vec![p, a, t, b])),
            (Join::Round, _) => {
                let a0 = n0.y.atan2(n0.x);
                let mut sweep = n1.y.atan2(n1.x) - a0;
                sweep = if turn > 0.0 {
                    sweep.rem_euclid(2.0 * PI)
                } else {
                    -((-sweep).rem_euclid(2.0 * PI))
                };
                let steps = ((sweep.abs() / ROUND_STEP).ceil() as usize).max(1);
                let mut ring = vec![p];
                ring.extend(
                    (0..=steps)
                        .map(|k| p + DVec2::from_angle(a0 + sweep * k as f64 / steps as f64) * h),
                );
                out.push(ccw(ring));
            }
            _ => out.push(ccw(vec![p, a, b])),
        }
    }
    out
}

/// True if the closed segment `p`-`q` meets the segment `a`-`b` anywhere.
fn segments_meet(p: DVec2, q: DVec2, a: DVec2, b: DVec2) -> bool {
    let (d1, d2) = (q - p, b - a);
    let side = |o: DVec2, u: DVec2, x: DVec2| u.perp_dot(x - o);
    let (o1, o2) = (side(p, d1, a), side(p, d1, b));
    let (o3, o4) = (side(a, d2, p), side(a, d2, q));
    if o1 == 0.0 && o2 == 0.0 {
        // Parallel on one line: they meet if their extents overlap.
        let (lo, hi) = (p.min(q), p.max(q));
        let (alo, ahi) = (a.min(b), a.max(b));
        return lo.cmple(ahi).all() && alo.cmple(hi).all();
    }
    o1 * o2 <= 0.0 && o3 * o4 <= 0.0
}

/// True if the bridge `p`-`q` crosses or touches an edge of the ring, apart from the edges that
/// start or end at one of its two ends.
fn bridge_blocked(p: DVec2, q: DVec2, ring: &[DVec2], budget: &Budget) -> bool {
    let n = ring.len();
    if !budget.take(n) {
        return true;
    }
    // A corner a hair from the middle of the bridge would end up inside its thin slit.
    let (pq, len2) = (q - p, p.distance_squared(q));
    let near = |c: DVec2| {
        let t = (c - p).dot(pq) / len2;
        t > 1e-3 && t < 1.0 - 1e-3 && c.distance(p + pq * t) <= 1e-6 * len2.sqrt()
    };
    (0..n).any(|k| {
        let (a, b) = (ring[k], ring[(k + 1) % n]);
        a != p && a != q && b != p && b != q && (segments_meet(p, q, a, b) || near(a))
    })
}

/// Joins a hole to an outer ring with a bridge of zero width, so that the result is one ring
/// with the same area (outer minus hole). The bridge joins the closest pair of points that see
/// each other: it crosses no edge of the outer ring or of the hole. A bridge that crosses an
/// edge would make a loop.
pub fn bridge(outer: Vec<DVec2>, hole: Vec<DVec2>) -> Vec<DVec2> {
    // No limit on the work here, as before: the other callers join few and small rings.
    bridge_among(outer, hole, &[], &Budget::new(u64::MAX))
}

/// Like `bridge`, and the bridge also keeps clear of the contours in `others`, which a later call
/// joins to the ring. Every segment test and point pair is counted in `budget`. When the budget
/// is out, no more bridges are checked: the result is then not safe, and the caller must look at
/// `budget.out` and drop it.
fn bridge_among(
    outer: Vec<DVec2>,
    hole: Vec<DVec2>,
    others: &[Vec<DVec2>],
    budget: &Budget,
) -> Vec<DVec2> {
    if hole.len() < 3 {
        return outer;
    }
    let clear = |p: DVec2, q: DVec2| {
        !bridge_blocked(p, q, &outer, budget)
            && !bridge_blocked(p, q, &hole, budget)
            && !others.iter().any(|o| bridge_blocked(p, q, o, budget))
    };
    // The closest pairs first. Only a few are tried; the closest pair is the fallback.
    let mut tried: Vec<(usize, usize)> = Vec::new();
    let mut chosen = None;
    let mut found = false;
    for _ in 0..24 {
        if !budget.take(outer.len() * hole.len()) {
            break;
        }
        let mut best = (f64::INFINITY, usize::MAX, usize::MAX);
        for (i, p) in outer.iter().enumerate() {
            for (j, q) in hole.iter().enumerate() {
                let d = p.distance_squared(*q);
                if d < best.0 && !tried.contains(&(i, j)) {
                    best = (d, i, j);
                }
            }
        }
        if best.1 == usize::MAX {
            break;
        }
        let (_, i, j) = best;
        if chosen.is_none() {
            chosen = Some((i, j));
        }
        if clear(outer[i], hole[j]) {
            chosen = Some((i, j));
            found = true;
            break;
        }
        tried.push((i, j));
    }
    // The closest pairs can all be blocked: other holes that are already part of the ring may
    // stand in the way. Then every pair is tried, the closest first, so the bridge never crosses
    // an edge as long as some pair sees each other.
    if !found
        && outer.len().saturating_mul(hole.len()) <= MAX_BRIDGE_PAIRS
        && budget.take(outer.len() * hole.len())
    {
        let mut pairs: Vec<(f64, usize, usize)> = Vec::with_capacity(outer.len() * hole.len());
        for (i, p) in outer.iter().enumerate() {
            for (j, q) in hole.iter().enumerate() {
                pairs.push((p.distance_squared(*q), i, j));
            }
        }
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        if let Some(&(_, i, j)) = pairs.iter().find(|&&(_, i, j)| clear(outer[i], hole[j])) {
            chosen = Some((i, j));
        }
    }
    let Some((i, j)) = chosen else { return outer };
    // The way back along the bridge is moved a hair to the side, away from the way out, so that
    // the ring has no repeated points and is a simple polygon. A repeated point confuses ear
    // clipping. A clockwise hole cuts a slit, and a counterclockwise piece adds a thin strip.
    let to = hole[j] - outer[i];
    let side = DVec2::new(-to.y, to.x).normalize_or_zero() * (to.length() * BRIDGE_WIDTH);
    let shift = if geom::signed_area(&hole) < 0.0 {
        -side
    } else {
        side
    };
    let mut out = Vec::with_capacity(outer.len() + hole.len() + 2);
    out.extend_from_slice(&outer[..=i]);
    for k in 0..hole.len() {
        out.push(hole[(j + k) % hole.len()]);
    }
    out.push(hole[j] + shift);
    out.push(outer[i] + shift);
    out.extend_from_slice(&outer[i + 1..]);
    out
}

/// The most pairs of points that the slow search for a bridge sorts (memory bound).
const MAX_BRIDGE_PAIRS: usize = 4_000_000;

/// Width of a bridge, as a fraction of its length. The area that this adds or takes is negligible.
const BRIDGE_WIDTH: f64 = 1e-7;

#[cfg(test)]
mod tests;
