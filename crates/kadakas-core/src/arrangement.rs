//! The planar arrangement of loose lines and arcs: where they cross, where they nearly meet, and
//! the faces (areas) that they enclose, with holes.
//!
//! This is the geometry under `region.from_curves` (PAT-06) and under the zones of a region
//! (PAT-07). It is pure and has no knowledge of the document.
//!
//! How it works:
//!
//! 1. Every input curve is a line or a circular arc ([`Curve`]).
//! 2. Curves are split where they cross, and where an end lies within the tolerance of the middle
//!    of another curve (a T-junction with a small gap or a small overshoot).
//! 3. Points within the tolerance of each other become one node. A node that joins ends which were
//!    not at one point is a closed gap: it is reported with its position and size.
//! 4. Edges that lead to a dead end are removed, and the dead ends are reported (position, length,
//!    and the nearest other open end). Nothing is guessed: a gap bigger than the tolerance stays
//!    open, and the faces on its two sides are not made.
//! 5. The faces are traced with a half-edge walk. A component of the drawing that lies inside a face
//!    of another component (an island) is a hole of that face. The nesting depth is kept.
//!
//! Two faces that share an edge use the same nodes, so they share the edge exactly.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use glam::DVec2;

use crate::curve::{Vertex, VertexKind};

/// Most input curves of one arrangement.
pub const MAX_CURVES: usize = 200_000;
/// Most nodes of one arrangement after splitting.
pub const MAX_EDGES: usize = 1_000_000;
/// Most dead ends and closed gaps that a report lists. The counts are always exact.
pub const MAX_LISTED: usize = 1000;
/// Chord error when an arc is cut into straight pieces for tests (mm).
const FLAT_ERR: f64 = 0.02;

/// A line or a circular arc.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Line {
        a: DVec2,
        b: DVec2,
    },
    /// The arc starts at angle `start` (radians) and turns by `sweep` (positive: counterclockwise).
    Arc {
        c: DVec2,
        r: f64,
        start: f64,
        sweep: f64,
    },
}

impl Curve {
    pub fn line(a: DVec2, b: DVec2) -> Curve {
        Curve::Line { a, b }
    }

    /// An arc from angle `start` to angle `end`, counterclockwise.
    pub fn arc_ccw(c: DVec2, r: f64, start: f64, end: f64) -> Curve {
        let mut sweep = (end - start).rem_euclid(TAU);
        if sweep == 0.0 && end != start {
            sweep = TAU;
        }
        Curve::Arc { c, r, start, sweep }
    }

    /// The arc through three points, or a line if the points are on one line.
    pub fn through(a: DVec2, m: DVec2, b: DVec2) -> Curve {
        match circumcircle(a, m, b) {
            Some((c, r)) => {
                let sa = (a - c).to_angle();
                let sm = (m - c).to_angle();
                let sb = (b - c).to_angle();
                let ccw = (sm - sa).rem_euclid(TAU);
                let end = (sb - sa).rem_euclid(TAU);
                let sweep = if ccw <= end { end } else { end - TAU };
                Curve::Arc {
                    c,
                    r,
                    start: sa,
                    sweep,
                }
            }
            None => Curve::Line { a, b },
        }
    }

    pub fn from(&self) -> DVec2 {
        self.at(0.0)
    }

    pub fn to(&self) -> DVec2 {
        self.at(1.0)
    }

    /// The point at `t` in 0..1 (1 is the end).
    pub fn at(&self, t: f64) -> DVec2 {
        match *self {
            Curve::Line { a, b } => a.lerp(b, t),
            Curve::Arc { c, r, start, sweep } => c + DVec2::from_angle(start + sweep * t) * r,
        }
    }

    pub fn length(&self) -> f64 {
        match *self {
            Curve::Line { a, b } => a.distance(b),
            Curve::Arc { r, sweep, .. } => r * sweep.abs(),
        }
    }

    /// The curve the other way round.
    pub fn reversed(&self) -> Curve {
        match *self {
            Curve::Line { a, b } => Curve::Line { a: b, b: a },
            Curve::Arc { c, r, start, sweep } => Curve::Arc {
                c,
                r,
                start: start + sweep,
                sweep: -sweep,
            },
        }
    }

    /// The direction of travel at the start.
    pub fn start_tangent(&self) -> DVec2 {
        match *self {
            Curve::Line { a, b } => (b - a).normalize_or_zero(),
            Curve::Arc { start, sweep, .. } => {
                let d = DVec2::from_angle(start).perp();
                if sweep >= 0.0 { d } else { -d }
            }
        }
    }

    /// The signed curvature: positive turns left.
    pub fn curvature(&self) -> f64 {
        match *self {
            Curve::Line { .. } => 0.0,
            Curve::Arc { r, sweep, .. } => sweep.signum() / r,
        }
    }

    /// The box of the curve.
    pub fn bounds(&self) -> (DVec2, DVec2) {
        match *self {
            Curve::Line { a, b } => (a.min(b), a.max(b)),
            Curve::Arc { c, r, start, sweep } => {
                let (a, b) = (self.from(), self.to());
                let (mut lo, mut hi) = (a.min(b), a.max(b));
                let (s0, s1) = if sweep >= 0.0 {
                    (start, start + sweep)
                } else {
                    (start + sweep, start)
                };
                for k in 0..4 {
                    let ang = k as f64 * PI / 2.0;
                    // The extreme angle lies in the sweep if some multiple of a quarter turn does.
                    let n = ((s0 - ang) / TAU).ceil();
                    if ang + n * TAU <= s1 {
                        let p = c + DVec2::from_angle(ang) * r;
                        lo = lo.min(p);
                        hi = hi.max(p);
                    }
                }
                (lo, hi)
            }
        }
    }

    /// The closest point of the curve to `p`: the point, the parameter t in 0..1 and the distance.
    pub fn closest(&self, p: DVec2) -> (DVec2, f64, f64) {
        match *self {
            Curve::Line { a, b } => {
                let ab = b - a;
                let l2 = ab.length_squared();
                let t = if l2 == 0.0 {
                    0.0
                } else {
                    ((p - a).dot(ab) / l2).clamp(0.0, 1.0)
                };
                let q = a + ab * t;
                (q, t, q.distance(p))
            }
            Curve::Arc { c, r, start, sweep } => {
                let v = p - c;
                if v.length_squared() > 0.0 {
                    let ang = v.to_angle();
                    if let Some(t) = arc_param(start, sweep, ang, 0.0) {
                        let q = c + v.normalize() * r;
                        return (q, t, q.distance(p));
                    }
                }
                let (e0, e1) = (self.from(), self.to());
                if p.distance(e0) <= p.distance(e1) {
                    (e0, 0.0, p.distance(e0))
                } else {
                    (e1, 1.0, p.distance(e1))
                }
            }
        }
    }

    /// Points of the curve, cut so that a chord is at most `err` from the curve. Includes both ends.
    pub fn flatten(&self, err: f64) -> Vec<DVec2> {
        match *self {
            Curve::Line { a, b } => vec![a, b],
            Curve::Arc { r, .. } => {
                let step = 2.0 * (1.0 - (err / r).min(1.0)).acos().max(1e-4);
                let n = ((self.sweep_abs() / step).ceil() as usize).clamp(1, 20_000);
                (0..=n).map(|i| self.at(i as f64 / n as f64)).collect()
            }
        }
    }

    fn sweep_abs(&self) -> f64 {
        match *self {
            Curve::Line { .. } => 0.0,
            Curve::Arc { sweep, .. } => sweep.abs(),
        }
    }

    /// The part between the parameters `t0` and `t1`.
    pub fn part(&self, t0: f64, t1: f64) -> Curve {
        match *self {
            Curve::Line { .. } => Curve::Line {
                a: self.at(t0),
                b: self.at(t1),
            },
            Curve::Arc { c, r, start, sweep } => Curve::Arc {
                c,
                r,
                start: start + sweep * t0,
                sweep: sweep * (t1 - t0),
            },
        }
    }
}

/// The circle through three points. `None` if they lie on one line (within a very small angle).
fn circumcircle(a: DVec2, b: DVec2, c: DVec2) -> Option<(DVec2, f64)> {
    let (ba, ca) = (b - a, c - a);
    let d = 2.0 * ba.perp_dot(ca);
    let scale = ba.length() * ca.length();
    if scale == 0.0 || (d / (2.0 * scale)).abs() < 1e-10 {
        return None;
    }
    let (b2, c2) = (ba.length_squared(), ca.length_squared());
    let ux = (ca.y * b2 - ba.y * c2) / d;
    let uy = (ba.x * c2 - ca.x * b2) / d;
    let centre = a + DVec2::new(ux, uy);
    let r = centre.distance(a);
    (r.is_finite() && r > 0.0).then_some((centre, r))
}

/// The parameter in 0..1 of the angle `ang` on an arc, or `None` if it is outside the sweep by
/// more than `slack` radians.
fn arc_param(start: f64, sweep: f64, ang: f64, slack: f64) -> Option<f64> {
    if sweep.abs() >= TAU - 1e-12 {
        let d = (ang - start).rem_euclid(TAU);
        let d = if sweep < 0.0 { (TAU - d) % TAU } else { d };
        return Some((d / sweep.abs()).clamp(0.0, 1.0));
    }
    let mut d = if sweep >= 0.0 {
        (ang - start).rem_euclid(TAU)
    } else {
        (start - ang).rem_euclid(TAU)
    };
    let s = sweep.abs();
    if d > s {
        // Just before the start or just after the end.
        if d - s <= slack {
            d = s;
        } else if TAU - d <= slack {
            d = 0.0;
        } else {
            return None;
        }
    }
    Some(d / s)
}

/// Points where two curves cross or touch. Parallel lines that overlap give none: their ends are
/// handled as T-junctions.
fn intersections(p: &Curve, q: &Curve, out: &mut Vec<DVec2>) {
    match (*p, *q) {
        (Curve::Line { a: a0, b: a1 }, Curve::Line { a: b0, b: b1 }) => {
            let (d1, d2) = (a1 - a0, b1 - b0);
            let den = d1.perp_dot(d2);
            if den.abs() < 1e-14 * d1.length() * d2.length() {
                return;
            }
            let w = b0 - a0;
            let t = w.perp_dot(d2) / den;
            let u = w.perp_dot(d1) / den;
            let eps = 1e-9;
            if (-eps..=1.0 + eps).contains(&t) && (-eps..=1.0 + eps).contains(&u) {
                out.push(a0 + d1 * t.clamp(0.0, 1.0));
            }
        }
        (Curve::Line { a, b }, arc @ Curve::Arc { .. })
        | (arc @ Curve::Arc { .. }, Curve::Line { a, b }) => {
            if let Curve::Arc { c, r, start, sweep } = arc {
                let d = b - a;
                let f = a - c;
                let qa = d.length_squared();
                if qa == 0.0 {
                    return;
                }
                let qb = 2.0 * f.dot(d);
                let qc = f.length_squared() - r * r;
                let disc = qb * qb - 4.0 * qa * qc;
                if disc < 0.0 {
                    // Tangent within noise?
                    if disc > -1e-12 * qb * qb {
                    } else {
                        return;
                    }
                }
                let sq = disc.max(0.0).sqrt();
                for t in [(-qb - sq) / (2.0 * qa), (-qb + sq) / (2.0 * qa)] {
                    if (-1e-9..=1.0 + 1e-9).contains(&t) {
                        let pt = a + d * t.clamp(0.0, 1.0);
                        if arc_param(start, sweep, (pt - c).to_angle(), 1e-9).is_some() {
                            out.push(pt);
                        }
                    }
                }
            }
        }
        (
            Curve::Arc {
                c: c0,
                r: r0,
                start: s0,
                sweep: w0,
            },
            Curve::Arc {
                c: c1,
                r: r1,
                start: s1,
                sweep: w1,
            },
        ) => {
            let d = c0.distance(c1);
            if d < 1e-12 || d > r0 + r1 + 1e-9 || d < (r0 - r1).abs() - 1e-9 {
                return;
            }
            let a = (r0 * r0 - r1 * r1 + d * d) / (2.0 * d);
            let h = (r0 * r0 - a * a).max(0.0).sqrt();
            let u = (c1 - c0) / d;
            let m = c0 + u * a;
            let pts = if h < 1e-9 {
                vec![m]
            } else {
                vec![m + u.perp() * h, m - u.perp() * h]
            };
            for pt in pts {
                if arc_param(s0, w0, (pt - c0).to_angle(), 1e-9).is_some()
                    && arc_param(s1, w1, (pt - c1).to_angle(), 1e-9).is_some()
                {
                    out.push(pt);
                }
            }
        }
    }
}

/// A closed gap: the ends of curves that were joined, or an end that was joined to the middle of a
/// curve.
#[derive(Clone, Debug, PartialEq)]
pub struct ClosedGap {
    pub at: DVec2,
    /// The size of the gap that was closed, in mm.
    pub size: f64,
    /// How far the points of the node moved to the node, at most, in mm.
    pub moved: f64,
}

/// A dead end: a curve end that joins nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct DeadEnd {
    pub at: DVec2,
    /// True if the chain from this end is part of a shape that does not close: the node where it
    /// meets other curves is cut off as well when the dead ends are removed (a gap in an outline
    /// makes all of the outline a dead branch, also its corners). False if the chain leads to a
    /// node that stays, on a closed shape: a stub, such as a line that overshoots a corner of a
    /// closed outline, which is harmless.
    pub open: bool,
    /// The length of the dangling curves from this end to the first node that has other curves, mm.
    pub length: f64,
    /// The nearest other dead end, and the distance to it (mm).
    pub nearest: Option<(DVec2, f64)>,
}

/// A gap that stays open: two dead ends that are each other's nearest end, or a dead end and the
/// middle of a curve, much nearer than the curves behind the ends are long (the place where an
/// outline is broken), so that a tolerance of `size` would close it.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenGap {
    /// The two ends.
    pub a: DVec2,
    pub b: DVec2,
    /// The distance between the ends, in mm.
    pub size: f64,
    /// True if `b` is a point on the middle of a curve that the end at `a` stops short of; false
    /// if `b` is another dead end.
    pub to_curve: bool,
}

/// One node-to-node piece of the drawing.
#[derive(Clone, Debug)]
pub struct Edge {
    pub a: usize,
    pub b: usize,
    pub curve: Curve,
    /// The index of the input curve it comes from.
    pub source: usize,
}

/// A closed boundary: the edges in order and whether each is walked forward.
#[derive(Clone, Debug, PartialEq)]
pub struct Cycle {
    pub edges: Vec<(usize, bool)>,
}

/// An area of the drawing, with its holes.
#[derive(Clone, Debug)]
pub struct Face {
    pub outer: Cycle,
    /// The outer boundaries of the components that lie inside this face.
    pub holes: Vec<Cycle>,
    /// The area with the holes taken off, in mm2.
    pub area: f64,
    /// How many faces enclose this one as a hole: 0 for the outermost.
    pub depth: usize,
    /// A point inside the face (not inside a hole).
    pub inside: DVec2,
    /// The component of the drawing that this face belongs to.
    pub component: usize,
    /// True if every edge of the outline has the same other face on its other side: the face is
    /// enclosed by one face, as an island is, although it is joined to it by lines (a stall line
    /// that ends on the kerb of an island).
    pub enclosed: bool,
    /// True if the whole outline is made of curves that lie on this outline and nowhere else (a
    /// circle, a closed shape, a closed chain of arcs) and no edge of it borders the outside of
    /// the drawing: a closed shape inside the outline of its drawing, joined to the rest by lines
    /// that end on it (stall lines that end on an island), so that no face encloses it.
    pub closed_loop: bool,
}

/// A face that was left out because it is thinner than the tolerance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DroppedSliver {
    pub at: DVec2,
    /// In mm2.
    pub area: f64,
}

/// The widest sliver that is left out, in mm: a bigger tolerance closes bigger gaps but does not
/// make a bigger face disappear.
pub const MAX_SLIVER_WIDTH: f64 = 1.0;

/// The result of [`build`].
#[derive(Clone, Debug, Default)]
pub struct Arrangement {
    pub nodes: Vec<DVec2>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
    pub closed_gaps: Vec<ClosedGap>,
    /// The number of closed gaps (the list is cut at [`MAX_LISTED`]).
    pub closed_gap_count: usize,
    pub dead_ends: Vec<DeadEnd>,
    pub dead_end_count: usize,
    /// How many of the dead ends are open (see [`DeadEnd::open`]).
    pub open_dead_ends: usize,
    /// The gaps that stay open (see [`OpenGap`]), the narrowest first, cut at [`MAX_LISTED`].
    pub open_gaps: Vec<OpenGap>,
    /// The number of open gaps.
    pub open_gap_count: usize,
    /// Curves shorter than the tolerance that were dropped (they do not make a piece of an edge).
    pub ignored_short: usize,
    /// Edges that lie on top of each other and were merged.
    pub duplicates: usize,
    /// Faces thinner than the tolerance (slivers between curves that nearly coincide) that were
    /// left out.
    pub dropped_slivers: usize,
    /// True when no thin face is dropped (the faces must add up to the area, as zones do).
    pub keep_slivers: bool,
    /// Where the first of them lie and their areas (cut at [`MAX_LISTED`]).
    pub dropped_sliver_list: Vec<DroppedSliver>,
    /// The sum of the distances that the points moved to their nodes, in mm.
    pub node_movement: f64,
    /// Nodes that joined points more than the tolerance apart through a chain of points.
    pub chained_nodes: usize,
    /// For each input curve: the edges that cover it (also an edge that a duplicate was merged
    /// into).
    pub curve_edges: Vec<Vec<usize>>,
    /// For each input curve: true if a piece of it was dropped (too short, or a dead end).
    pub lost_curves: Vec<bool>,
}

/// Why an arrangement was not made.
#[derive(Clone, Debug, PartialEq)]
pub enum BuildError {
    TooMany(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::TooMany(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for BuildError {}

struct Union {
    p: Vec<usize>,
}

impl Union {
    fn new(n: usize) -> Union {
        Union {
            p: (0..n).collect(),
        }
    }
    fn find(&mut self, mut x: usize) -> usize {
        while self.p[x] != x {
            self.p[x] = self.p[self.p[x]];
            x = self.p[x];
        }
        x
    }
    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.p[b] = a;
        }
    }
}

/// A node point of a curve: where it is, and its parameter on the curve.
#[derive(Clone, Copy)]
struct Hit {
    curve: usize,
    t: f64,
    p: DVec2,
    /// The size of the closed gap this hit stands for (0 for a crossing or an exact join).
    gap: f64,
    /// The point on a curve that the end of another curve was moved to (a T-junction).
    proj: bool,
}

/// Where two lines that end near each other would cross, if both ends move by at most `2 tol`.
fn line_corner(curves: &[Curve], a: &Hit, b: &Hit, tol: f64) -> Option<DVec2> {
    if a.curve == b.curve {
        return None;
    }
    let (Curve::Line { a: a0, b: a1 }, Curve::Line { a: b0, b: b1 }) =
        (curves[a.curve], curves[b.curve])
    else {
        return None;
    };
    let (d1, d2) = (a1 - a0, b1 - b0);
    let den = d1.perp_dot(d2);
    // Lines that are nearly parallel cross far away: use the mean.
    if den.abs() < 0.05 * d1.length() * d2.length() {
        return None;
    }
    let t = (b0 - a0).perp_dot(d2) / den;
    let p = a0 + d1 * t;
    (p.distance(a.p) <= 2.0 * tol && p.distance(b.p) <= 2.0 * tol).then_some(p)
}

/// Builds the arrangement of `curves` with the node tolerance `tol` (mm).
pub fn build(curves: &[Curve], tol: f64) -> Result<Arrangement, BuildError> {
    build_ranked(curves, &[], tol)
}

/// Like [`build`]. `ranks` says which curves are fixed: where ends of curves of different rank
/// join in one node, the node sits where the curves of the highest rank put it, and the others
/// move to it. Zones use this so that the boundary of a region never moves. An empty list gives
/// every curve rank 0 (the node is the mean of its points).
pub fn build_ranked(curves: &[Curve], ranks: &[u8], tol: f64) -> Result<Arrangement, BuildError> {
    if curves.len() > MAX_CURVES {
        return Err(BuildError::TooMany(format!(
            "an arrangement takes at most {MAX_CURVES} curves, not {}",
            curves.len()
        )));
    }
    // An arc of more than a half circle is cut in two, so that an arc that nearly closes on itself
    // is not taken for a short curve, and its ends can join.
    let mut split: Vec<Curve> = Vec::with_capacity(curves.len());
    let mut split_ranks: Vec<u8> = Vec::with_capacity(curves.len());
    let mut origin: Vec<usize> = Vec::with_capacity(curves.len());
    for (i, c) in curves.iter().enumerate() {
        let rank = ranks.get(i).copied().unwrap_or(0);
        match *c {
            Curve::Arc { sweep, .. } if sweep.abs() > PI => {
                for part in [c.part(0.0, 0.5), c.part(0.5, 1.0)] {
                    split.push(part);
                    split_ranks.push(rank);
                    origin.push(i);
                }
            }
            other => {
                split.push(other);
                split_ranks.push(rank);
                origin.push(i);
            }
        }
    }
    let mut out = build_inner(&split, &split_ranks, tol, !ranks.is_empty())?;
    for e in &mut out.edges {
        e.source = origin[e.source];
    }
    let mut curve_edges: Vec<Vec<usize>> = vec![Vec::new(); curves.len()];
    let mut lost = vec![false; curves.len()];
    for (k, edges) in out.curve_edges.iter().enumerate() {
        curve_edges[origin[k]].extend(edges);
    }
    for (k, l) in out.lost_curves.iter().enumerate() {
        lost[origin[k]] |= *l;
    }
    for list in &mut curve_edges {
        list.sort_unstable();
        list.dedup();
    }
    out.curve_edges = curve_edges;
    out.lost_curves = lost;
    Ok(out)
}

fn build_inner(
    curves: &[Curve],
    ranks: &[u8],
    tol: f64,
    keep_slivers: bool,
) -> Result<Arrangement, BuildError> {
    if curves.len() > MAX_CURVES * 2 {
        return Err(BuildError::TooMany(format!(
            "an arrangement takes at most {MAX_CURVES} curves, not {}",
            curves.len()
        )));
    }
    let tol = tol.max(1e-9);
    let mut out = Arrangement::default();
    // Curves shorter than the tolerance are noise: they could not make a piece of an edge.
    let mut live: Vec<usize> = Vec::new();
    let mut short: Vec<usize> = Vec::new();
    for (i, c) in curves.iter().enumerate() {
        if c.length() < tol {
            out.ignored_short += 1;
            short.push(i);
        } else {
            live.push(i);
        }
    }
    let boxes: Vec<(DVec2, DVec2)> = curves
        .iter()
        .map(|c| {
            let (lo, hi) = c.bounds();
            (lo - DVec2::splat(tol), hi + DVec2::splat(tol))
        })
        .collect();
    // Candidate pairs by a sweep over x.
    let mut order = live.clone();
    order.sort_by(|&a, &b| boxes[a].0.x.total_cmp(&boxes[b].0.x));
    let mut hits: Vec<Hit> = Vec::new();
    // Ends of every curve are nodes.
    for &i in &live {
        hits.push(Hit {
            curve: i,
            t: 0.0,
            p: curves[i].from(),
            gap: 0.0,
            proj: false,
        });
        hits.push(Hit {
            curve: i,
            t: 1.0,
            p: curves[i].to(),
            gap: 0.0,
            proj: false,
        });
    }
    let mut tmp: Vec<DVec2> = Vec::new();
    for (oi, &i) in order.iter().enumerate() {
        for &j in &order[oi + 1..] {
            if boxes[j].0.x > boxes[i].1.x {
                break;
            }
            if boxes[j].0.y > boxes[i].1.y || boxes[j].1.y < boxes[i].0.y {
                continue;
            }
            if hits.len() > MAX_EDGES {
                return Err(BuildError::TooMany(format!(
                    "the curves cross each other so often that the drawing would make more than {MAX_EDGES} pieces"
                )));
            }
            let (ci, cj) = (&curves[i], &curves[j]);
            tmp.clear();
            intersections(ci, cj, &mut tmp);
            for &p in &tmp {
                let (_, ti, _) = ci.closest(p);
                let (_, tj, _) = cj.closest(p);
                hits.push(Hit {
                    curve: i,
                    t: ti,
                    p,
                    gap: 0.0,
                    proj: false,
                });
                hits.push(Hit {
                    curve: j,
                    t: tj,
                    p,
                    gap: 0.0,
                    proj: false,
                });
            }
            // An end of one that lies near the middle of the other: a T-junction.
            for (a, b, ca, cb) in [(i, j, ci, cj), (j, i, cj, ci)] {
                for e in [ca.from(), ca.to()] {
                    let (q, t, d) = cb.closest(e);
                    // Where the closest point is an end of the other curve the two ends meet as
                    // ends do (the clustering and the corner rule below handle them).
                    if d <= tol && t > 1e-9 && t < 1.0 - 1e-9 {
                        // Also near an end of the other curve: the chain of points joins them.
                        hits.push(Hit {
                            curve: b,
                            t,
                            p: q,
                            gap: d,
                            proj: true,
                        });
                        // The end of `a` stays where it is: the clustering joins it to `q`.
                        let te = if e == ca.from() { 0.0 } else { 1.0 };
                        hits.push(Hit {
                            curve: a,
                            t: te,
                            p: e,
                            gap: d,
                            proj: false,
                        });
                    }
                }
            }
        }
    }
    // Cluster the hit points: all within tol of each other make one node.
    let n = hits.len();
    let mut uf = Union::new(n);
    let cell = tol.max(1e-9);
    let key = |p: DVec2| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, h) in hits.iter().enumerate() {
        let (kx, ky) = key(h.p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(v) = grid.get(&(kx + dx, ky + dy)) {
                    for &j in v {
                        if hits[j].p.distance(h.p) <= tol {
                            uf.join(i, j);
                        }
                    }
                }
            }
        }
        grid.entry((kx, ky)).or_default().push(i);
    }
    // Hits that stand for the same T-junction must be one node even if the end is far from the
    // projection by almost the tolerance: they were pushed with the same point `q` already, and
    // the end itself is within `gap` of it.
    let mut node_of_root: HashMap<usize, usize> = HashMap::new();
    let mut hit_node = vec![0usize; n];
    let mut members: Vec<Vec<usize>> = Vec::new();
    for (i, slot) in hit_node.iter_mut().enumerate() {
        let r = uf.find(i);
        let id = *node_of_root.entry(r).or_insert_with(|| {
            members.push(Vec::new());
            members.len() - 1
        });
        *slot = id;
        members[id].push(i);
    }
    // Which point a node takes: first the curves of the highest rank (a fixed boundary); among
    // those the point of a curve that another end was moved to (a T-junction: the end goes to the
    // line, the line does not bend), then the ends of curves, then crossings.
    let rank_of = |i: usize| {
        let h = &hits[i];
        let end = h.t == 0.0 || h.t == 1.0;
        let kind = if h.proj {
            2
        } else if end {
            1
        } else {
            0
        };
        4 * ranks.get(h.curve).copied().unwrap_or(0) + kind
    };
    for m in &members {
        let top = m.iter().map(|&i| rank_of(i)).max().unwrap_or(0);
        let first: Vec<usize> = m.iter().copied().filter(|&i| rank_of(i) == top).collect();
        let sum: DVec2 = first.iter().map(|&i| hits[i].p).sum();
        let mean = sum / first.len() as f64;
        // Two straight lines that nearly meet at a corner meet where they would cross: a drawing
        // that is square stays square, and the corner does not move by half of the gap.
        let corner = (first.len() == 2 && top % 4 == 1)
            .then(|| line_corner(curves, &hits[first[0]], &hits[first[1]], tol))
            .flatten();
        out.nodes.push(corner.unwrap_or(mean));
    }
    // A node whose members are not all at one point closed a gap.
    for (id, m) in members.iter().enumerate() {
        let mut size = 0.0f64;
        let mut moved = 0.0f64;
        for &i in m {
            size = size.max(hits[i].gap);
            moved = moved.max(hits[i].p.distance(out.nodes[id]));
            if m.len() <= 32 {
                for &j in m {
                    size = size.max(hits[i].p.distance(hits[j].p));
                }
            } else {
                size = size.max(2.0 * hits[i].p.distance(out.nodes[id]));
            }
        }
        out.node_movement += moved;
        if size > tol * (1.0 + 1e-9) {
            out.chained_nodes += 1;
        }
        if size > 1e-6 {
            out.closed_gap_count += 1;
            if out.closed_gaps.len() < MAX_LISTED {
                out.closed_gaps.push(ClosedGap {
                    at: out.nodes[id],
                    size,
                    moved,
                });
            }
        }
    }
    // Edges: the nodes of each curve in order along it.
    let mut per_curve: Vec<Vec<(f64, usize)>> = vec![Vec::new(); curves.len()];
    for (i, h) in hits.iter().enumerate() {
        per_curve[h.curve].push((h.t, hit_node[i]));
    }
    let mut seen: HashMap<(usize, usize), Vec<(DVec2, usize)>> = HashMap::new();
    // (curve, edge): a curve whose piece was merged into an edge of another curve.
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for &ci in &live {
        let list = &mut per_curve[ci];
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Equal nodes in a row are one.
        list.dedup_by(|b, a| a.1 == b.1);
        let curve = &curves[ci];
        for w in list.windows(2) {
            let ((t0, n0), (t1, n1)) = (w[0], w[1]);
            if n0 == n1 {
                continue;
            }
            let (p0, p1) = (out.nodes[n0], out.nodes[n1]);
            let part = curve.part(t0, t1);
            let geom = match part {
                Curve::Line { .. } => Curve::Line { a: p0, b: p1 },
                Curve::Arc { .. } => {
                    let moved = part.from().distance(p0).max(part.to().distance(p1));
                    if moved < 1e-7 {
                        part
                    } else {
                        Curve::through(p0, part.at(0.5), p1)
                    }
                }
            };
            if out.edges.len() >= MAX_EDGES {
                return Err(BuildError::TooMany(format!(
                    "the drawing makes more than {MAX_EDGES} pieces"
                )));
            }
            // Duplicates: the same ends and the same middle.
            let key = (n0.min(n1), n0.max(n1));
            let mid = geom.at(0.5);
            let list = seen.entry(key).or_default();
            if let Some((_, edge)) = list.iter().find(|(m, _)| m.distance(mid) <= tol) {
                out.duplicates += 1;
                merged.push((ci, *edge));
                continue;
            }
            list.push((mid, out.edges.len()));
            out.edges.push(Edge {
                a: n0,
                b: n1,
                curve: geom,
                source: ci,
            });
        }
    }
    // Zones (ranked curves) cut a region into parts that must add up to it: no face is dropped.
    out.keep_slivers = keep_slivers;
    prune_and_trace(&mut out, curves.len(), &short, &merged, tol);
    Ok(out)
}

/// A gap whose sides are nearer than this share of the chain behind either side is a gap that
/// keeps a shape open. The two ends of the overshoots at a corner are as far apart as the
/// overshoots are long (or farther), so they never are one.
const GAP_SHARE: f64 = 0.5;

/// The widest gap between an end and the middle of a curve that is looked for, in mm, for a
/// tolerance: such a gap needs a search near every dead end, so it is bounded. (Gaps between two
/// ends are found at any width.)
fn end_to_curve_reach(tol: f64) -> f64 {
    (25.0 * tol).max(50.0)
}

/// The nearest other end of `ends[i]`, by a sweep over x: its index and the distance.
fn nearest_end(ends: &[DVec2], i: usize) -> Option<(usize, f64)> {
    let p = ends[i];
    let mut best: Option<(usize, f64)> = None;
    for (j, q) in ends.iter().enumerate() {
        if j != i {
            let d = p.distance(*q);
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((j, d));
            }
        }
    }
    best
}

/// The gaps that keep a shape open (see [`OpenGap`]), the narrowest first, and for each dead end
/// whether it is a side of one. `dead` holds for each dead end its node, the length of its chain
/// whether the chain runs into another free end and the node where it ends; `chain_of_edge` the dead end whose chain
/// holds an edge (or `usize::MAX`).
fn find_open_gaps(
    out: &Arrangement,
    dead: &[(usize, f64, bool, usize)],
    ends: &[DVec2],
    chain_of_edge: &[usize],
    tol: f64,
) -> (Vec<OpenGap>, Vec<bool>) {
    let n = dead.len();
    let mut in_gap = vec![false; n];
    let mut gaps: Vec<OpenGap> = Vec::new();
    if n == 0 {
        return (gaps, in_gap);
    }
    // The nearest other end of each, by a sweep over x.
    let mut by_x: Vec<usize> = (0..n).collect();
    by_x.sort_by(|&a, &b| ends[a].x.total_cmp(&ends[b].x));
    let mut rank = vec![0usize; n];
    for (r, &i) in by_x.iter().enumerate() {
        rank[i] = r;
    }
    let nearest: Vec<Option<(usize, f64)>> = (0..n)
        .map(|i| {
            let p = ends[i];
            let mut best: Option<(usize, f64)> = None;
            let mut scan = |r: usize| -> bool {
                let j = by_x[r];
                if best.is_some_and(|(_, bd)| (ends[j].x - p.x).abs() > bd) {
                    return false;
                }
                let d = p.distance(ends[j]);
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((j, d));
                }
                true
            };
            for r in rank[i] + 1..n {
                if !scan(r) {
                    break;
                }
            }
            for r in (0..rank[i]).rev() {
                if !scan(r) {
                    break;
                }
            }
            best
        })
        .collect();
    // Two ends that are each other's nearest end, and near compared with their chains. Two
    // chains that end at the same junction (the overshoots of an acute corner) would only close
    // a sliver at that junction: no gap.
    for i in 0..n {
        let Some((j, d)) = nearest[i] else { continue };
        let mutual = nearest[j].is_some_and(|(k, _)| k == i);
        let same_junction = !dead[i].2 && dead[i].3 == dead[j].3;
        if mutual && !same_junction && d < GAP_SHARE * dead[i].1.min(dead[j].1) {
            in_gap[i] = true;
            if i < j {
                gaps.push(OpenGap {
                    a: ends[i],
                    b: ends[j],
                    size: d,
                    to_curve: false,
                });
            }
        }
    }
    // An end that stops short of the middle of a curve (not of its own chain): a grid of the
    // boxes of the edges finds the curves near each end that is in no gap yet.
    let reach = end_to_curve_reach(tol);
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    let boxes: Vec<(DVec2, DVec2)> = out.edges.iter().map(|e| e.curve.bounds()).collect();
    for b in &boxes {
        lo = lo.min(b.0);
        hi = hi.max(b.1);
    }
    let cell = reach.max((hi - lo).max_element() / 2048.0).max(1e-9);
    let nx = (((hi.x - lo.x) / cell).floor() as usize + 1).min(4096);
    let ny = (((hi.y - lo.y) / cell).floor() as usize + 1).min(4096);
    let cell_of = |p: DVec2| -> (usize, usize) {
        (
            (((p.x - lo.x) / cell).floor().max(0.0) as usize).min(nx - 1),
            (((p.y - lo.y) / cell).floor().max(0.0) as usize).min(ny - 1),
        )
    };
    let mut grid: std::collections::HashMap<(usize, usize), Vec<usize>> =
        std::collections::HashMap::new();
    // Edges that cover very many cells are tested with every end.
    let mut big: Vec<usize> = Vec::new();
    let wanted: Vec<usize> = (0..n).filter(|&i| !in_gap[i] && !dead[i].2).collect();
    if !wanted.is_empty() {
        for (ei, b) in boxes.iter().enumerate() {
            let (c0, c1) = (cell_of(b.0), cell_of(b.1));
            if (c1.0 - c0.0 + 1) * (c1.1 - c0.1 + 1) > 1024 {
                big.push(ei);
                continue;
            }
            for cx in c0.0..=c1.0 {
                for cy in c0.1..=c1.1 {
                    grid.entry((cx, cy)).or_default().push(ei);
                }
            }
        }
    }
    let mut cand: Vec<usize> = Vec::new();
    for i in wanted {
        let p = ends[i];
        let r = reach.min(GAP_SHARE * dead[i].1);
        let (c0, c1) = (cell_of(p - DVec2::splat(r)), cell_of(p + DVec2::splat(r)));
        cand.clear();
        for cx in c0.0..=c1.0 {
            for cy in c0.1..=c1.1 {
                if let Some(list) = grid.get(&(cx, cy)) {
                    cand.extend_from_slice(list);
                }
            }
        }
        cand.extend_from_slice(&big);
        let mut best: Option<(DVec2, f64)> = None;
        for &ei in &cand {
            let e = &out.edges[ei];
            // The own chain is no gap, and neither is a curve that meets the chain at its
            // junction (the overshoot of the other line at an acute corner lies near the tip).
            if chain_of_edge[ei] == i || e.a == dead[i].3 || e.b == dead[i].3 {
                continue;
            }
            let (q, _, d) = e.curve.closest(p);
            if d < r && d > 1e-9 && best.is_none_or(|(_, bd)| d < bd) {
                best = Some((q, d));
            }
        }
        if let Some((q, d)) = best {
            in_gap[i] = true;
            gaps.push(OpenGap {
                a: p,
                b: q,
                size: d,
                to_curve: true,
            });
        }
    }
    gaps.sort_by(|x, y| x.size.total_cmp(&y.size));
    (gaps, in_gap)
}

/// Removes dead ends, then traces the faces.
fn prune_and_trace(
    out: &mut Arrangement,
    ncurves: usize,
    short: &[usize],
    merged: &[(usize, usize)],
    tol: f64,
) {
    let nn = out.nodes.len();
    let mut deg = vec![0usize; nn];
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); nn];
    for (i, e) in out.edges.iter().enumerate() {
        deg[e.a] += 1;
        deg[e.b] += 1;
        adj[e.a].push(i);
        adj[e.b].push(i);
    }
    // The dead ends of the drawing as it is: each free end, with the length of its chain of
    // nodes of degree 2 up to the first node that joins more (or another free end), and the
    // edges of that chain.
    let free: Vec<usize> = (0..nn).filter(|&v| deg[v] == 1).collect();
    let mut dead: Vec<(usize, f64, bool, usize)> = Vec::new();
    let mut chain_of_edge: Vec<usize> = vec![usize::MAX; out.edges.len()];
    for (di, &v0) in free.iter().enumerate() {
        let (mut v, mut prev_edge, mut length) = (v0, usize::MAX, 0.0);
        while let Some(&ei) = adj[v].iter().find(|&&e| e != prev_edge) {
            length += out.edges[ei].curve.length();
            if chain_of_edge[ei] == usize::MAX {
                chain_of_edge[ei] = di;
            }
            let e = &out.edges[ei];
            let w = if e.a == v { e.b } else { e.a };
            prev_edge = ei;
            v = w;
            if deg[v] != 2 {
                break;
            }
        }
        dead.push((v0, length, deg[v] == 1, v));
    }
    let ends: Vec<DVec2> = dead.iter().map(|d| out.nodes[d.0]).collect();
    let (gaps, in_gap) = find_open_gaps(out, &dead, &ends, &chain_of_edge, tol);
    // A dead end is open when its chain runs into another free end (a shape that does not close)
    // or when it is a side of a gap that stays open (two ends, or an end and a curve, much nearer
    // to each other than the chain behind the end is long: the place where an outline is
    // broken). Otherwise it is a stub, such as a line that overshoots a corner. Following each
    // chain to its first junction only is not enough: the pieces of a broken outline whose lines
    // overshoot the corners all end at a corner, where an overshoot meets them.
    let open: Vec<bool> = dead.iter().zip(&in_gap).map(|(d, g)| d.2 || *g).collect();
    out.dead_end_count = dead.len();
    out.open_dead_ends = open.iter().filter(|&&o| o).count();
    for (i, (v, length, _, _)) in dead.iter().enumerate() {
        if out.dead_ends.len() >= MAX_LISTED {
            break;
        }
        out.dead_ends.push(DeadEnd {
            at: out.nodes[*v],
            open: open[i],
            length: *length,
            nearest: nearest_end(&ends, i).map(|(j, d)| (ends[j], d)),
        });
    }
    out.open_gap_count = gaps.len();
    out.open_gaps = gaps;
    out.open_gaps.truncate(MAX_LISTED);
    // Remove the dead branches: a node of degree 1 loses its edge, and so on.
    let mut removed = vec![false; out.edges.len()];
    let mut queue: Vec<usize> = free;
    while let Some(v) = queue.pop() {
        if deg[v] != 1 {
            continue;
        }
        let Some(&ei) = adj[v].iter().find(|&&e| !removed[e]) else {
            continue;
        };
        removed[ei] = true;
        let e = &out.edges[ei];
        let w = if e.a == v { e.b } else { e.a };
        deg[v] -= 1;
        deg[w] -= 1;
        if deg[w] == 1 {
            queue.push(w);
        }
    }
    let mut lost = vec![false; ncurves];
    for &c in short {
        lost[c] = true;
    }
    let mut remap: Vec<Option<usize>> = vec![None; out.edges.len()];
    let old = std::mem::take(&mut out.edges);
    for (i, e) in old.into_iter().enumerate() {
        if removed[i] {
            lost[e.source] = true;
        } else {
            remap[i] = Some(out.edges.len());
            out.edges.push(e);
        }
    }
    let mut curve_edges: Vec<Vec<usize>> = vec![Vec::new(); ncurves];
    for (i, e) in out.edges.iter().enumerate() {
        curve_edges[e.source].push(i);
    }
    for &(c, edge) in merged {
        match remap[edge] {
            Some(n) => curve_edges[c].push(n),
            None => lost[c] = true,
        }
    }
    out.curve_edges = curve_edges;
    out.lost_curves = lost;
    trace_faces(out, tol);
}

fn trace_faces(out: &mut Arrangement, tol: f64) {
    let ne = out.edges.len();
    if ne == 0 {
        return;
    }
    // Half-edge h = 2 * edge + (0 forward, 1 backward).
    let nn = out.nodes.len();
    let mut around: Vec<Vec<usize>> = vec![Vec::new(); nn];
    let curve_of = |e: &Edge, back: bool| if back { e.curve.reversed() } else { e.curve };
    for (i, e) in out.edges.iter().enumerate() {
        around[e.a].push(2 * i);
        around[e.b].push(2 * i + 1);
    }
    let mut key: Vec<(f64, f64)> = vec![(0.0, 0.0); 2 * ne];
    for (i, e) in out.edges.iter().enumerate() {
        for back in [false, true] {
            let c = curve_of(e, back);
            let t = c.start_tangent();
            // The direction is rounded, so that directions that are equal within noise are equal
            // for the order (a strict weak order), also at the seam of +-pi.
            let mut q = (t.y.atan2(t.x) * 1e9).round();
            if q >= (PI * 1e9).round() {
                q = -q;
            }
            key[2 * i + back as usize] = (q, c.curvature());
        }
    }
    let target = |h: usize| {
        let e = &out.edges[h / 2];
        if h.is_multiple_of(2) { e.b } else { e.a }
    };
    let mut pos = vec![0usize; 2 * ne];
    for list in around.iter_mut() {
        list.sort_by(|&a, &b| {
            let (ka, kb) = (key[a], key[b]);
            ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
        });
        for (k, &h) in list.iter().enumerate() {
            pos[h] = k;
        }
    }
    let next = |h: usize| -> usize {
        let v = target(h);
        let twin = h ^ 1;
        let list = &around[v];
        let k = pos[twin];
        list[(k + list.len() - 1) % list.len()]
    };
    let mut cycle_of = vec![usize::MAX; 2 * ne];
    let mut cycles: Vec<(Cycle, f64)> = Vec::new();
    for start in 0..2 * ne {
        if cycle_of[start] != usize::MAX {
            continue;
        }
        let id = cycles.len();
        let mut h = start;
        let mut edges = Vec::new();
        let mut guard = 0usize;
        while cycle_of[h] == usize::MAX {
            cycle_of[h] = id;
            edges.push((h / 2, h % 2 == 0));
            h = next(h);
            guard += 1;
            if guard > 2 * ne + 1 {
                break;
            }
        }
        let area = cycle_area(out, &edges);
        cycles.push((Cycle { edges }, area));
    }
    // Components of the edge graph.
    let mut uf = Union::new(nn);
    for e in &out.edges {
        uf.join(e.a, e.b);
    }
    let comp_of_edge: Vec<usize> = out.edges.iter().map(|e| uf.find(e.a)).collect();
    let mut comp_ids: HashMap<usize, usize> = HashMap::new();
    let mut comp_index = |c: usize| {
        let n = comp_ids.len();
        *comp_ids.entry(c).or_insert(n)
    };
    // Positive cycles are faces. For each component, the negative cycle is its outer boundary.
    struct FaceTmp {
        cycle: usize,
        comp: usize,
        ring: Vec<DVec2>,
        lo: DVec2,
        hi: DVec2,
        area: f64,
    }
    let mut faces: Vec<FaceTmp> = Vec::new();
    let mut outers: Vec<(usize, usize)> = Vec::new(); // (cycle, comp)
    for (ci, (cy, area)) in cycles.iter().enumerate() {
        let comp = comp_index(comp_of_edge[cy.edges[0].0]);
        if *area > 0.0 {
            let ring = cycle_ring(out, cy);
            let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
            for p in &ring {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            faces.push(FaceTmp {
                cycle: ci,
                comp,
                ring,
                lo,
                hi,
                area: *area,
            });
        } else if *area < 0.0 {
            outers.push((ci, comp));
        }
    }
    // Which face (of another component) holds each component?
    let mut by_area: Vec<usize> = (0..faces.len()).collect();
    by_area.sort_by(|&a, &b| faces[a].area.total_cmp(&faces[b].area));
    let mut parent_of_comp: HashMap<usize, usize> = HashMap::new();
    let mut holes_of: Vec<Vec<usize>> = vec![Vec::new(); faces.len()];
    for &(cy, comp) in &outers {
        let probe = out.nodes[out.edges[cycles[cy].0.edges[0].0].a];
        for &f in &by_area {
            let fc = &faces[f];
            if fc.comp == comp {
                continue;
            }
            if probe.x < fc.lo.x || probe.x > fc.hi.x || probe.y < fc.lo.y || probe.y > fc.hi.y {
                continue;
            }
            if crate::geom::point_in_polygon(probe, &fc.ring) {
                parent_of_comp.insert(comp, f);
                holes_of[f].push(cy);
                break;
            }
        }
    }
    // Depth of each component: 0 if it has no parent face, else the depth of that face's component + 1.
    let mut comp_depth: HashMap<usize, usize> = HashMap::new();
    fn depth_of(
        comp: usize,
        parent: &HashMap<usize, usize>,
        faces: &[FaceTmp],
        memo: &mut HashMap<usize, usize>,
    ) -> usize {
        if let Some(d) = memo.get(&comp) {
            return *d;
        }
        let d = match parent.get(&comp) {
            None => 0,
            Some(&f) => depth_of(faces[f].comp, parent, faces, memo) + 1,
        };
        memo.insert(comp, d);
        d
    }
    // Which face lies on the other side of each edge of a face?
    let mut owner: HashMap<(usize, bool), usize> = HashMap::new();
    for (fi, f) in faces.iter().enumerate() {
        for &(e, fwd) in &cycles[f.cycle].0.edges {
            owner.insert((e, fwd), fi);
        }
    }
    // The curves that cover each edge.
    let mut covering: Vec<Vec<usize>> = vec![Vec::new(); out.edges.len()];
    for (c, es) in out.curve_edges.iter().enumerate() {
        for &e in es {
            covering[e].push(c);
        }
    }
    for fi in 0..faces.len() {
        let comp = faces[fi].comp;
        // A sliver between curves that nearly coincide is noise, not an area.
        let perimeter: f64 = cycles[faces[fi].cycle]
            .0
            .edges
            .iter()
            .map(|&(e, _)| out.edges[e].curve.length())
            .sum();
        if !out.keep_slivers && faces[fi].area < 0.5 * tol.min(MAX_SLIVER_WIDTH) * perimeter {
            out.dropped_slivers += 1;
            if out.dropped_sliver_list.len() < MAX_LISTED {
                let ring = &faces[fi].ring;
                let at = ring.iter().copied().sum::<DVec2>() / ring.len().max(1) as f64;
                out.dropped_sliver_list.push(DroppedSliver {
                    at,
                    area: faces[fi].area,
                });
            }
            continue;
        }
        let mut others: Vec<Option<usize>> = cycles[faces[fi].cycle]
            .0
            .edges
            .iter()
            .map(|&(e, fwd)| owner.get(&(e, !fwd)).copied())
            .collect();
        others.sort_unstable();
        others.dedup();
        let enclosed = others.len() == 1 && others[0].is_some_and(|o| o != fi);
        let closed_loop =
            !others.is_empty() && others.iter().all(|o| o.is_some_and(|o| o != fi)) && {
                let ring: std::collections::HashSet<usize> = cycles[faces[fi].cycle]
                    .0
                    .edges
                    .iter()
                    .map(|&(e, _)| e)
                    .collect();
                let own: std::collections::HashSet<usize> =
                    ring.iter().map(|&e| out.edges[e].source).collect();
                own.iter()
                    .all(|&c| out.curve_edges[c].iter().all(|e| ring.contains(e)))
                    && ring
                        .iter()
                        .all(|&e| covering[e].iter().all(|c| own.contains(c)))
            };
        let d = depth_of(comp, &parent_of_comp, &faces, &mut comp_depth);
        let hole_cycles: Vec<Cycle> = holes_of[fi]
            .iter()
            .map(|&cy| {
                let mut c = cycles[cy].0.clone();
                // Walk it the other way round: counterclockwise like the outer boundary.
                c.edges = c.edges.iter().rev().map(|&(e, f)| (e, !f)).collect();
                c
            })
            .collect();
        let hole_area: f64 = holes_of[fi].iter().map(|&cy| -cycles[cy].1).sum();
        let ring_holes: Vec<Vec<DVec2>> = hole_cycles.iter().map(|c| cycle_ring(out, c)).collect();
        let inside = interior_point(&faces[fi].ring, &ring_holes);
        out.faces.push(Face {
            outer: cycles[faces[fi].cycle].0.clone(),
            holes: hole_cycles,
            area: faces[fi].area - hole_area,
            depth: d,
            inside,
            component: faces[fi].comp,
            enclosed,
            closed_loop,
        });
    }
}

/// The signed area of a cycle (counterclockwise is positive), arcs included.
fn cycle_area(out: &Arrangement, edges: &[(usize, bool)]) -> f64 {
    let mut poly = 0.0;
    let mut bulge = 0.0;
    // Relative to the first point: sums of absolute coordinates lose the area of a small face far
    // from the origin (survey coordinates are millions of mm).
    let o = edges.first().map_or(DVec2::ZERO, |&(ei, fwd)| {
        let e = &out.edges[ei];
        out.nodes[if fwd { e.a } else { e.b }]
    });
    for &(ei, fwd) in edges {
        let e = &out.edges[ei];
        let c = if fwd { e.curve } else { e.curve.reversed() };
        let (p, q) = if fwd {
            (out.nodes[e.a] - o, out.nodes[e.b] - o)
        } else {
            (out.nodes[e.b] - o, out.nodes[e.a] - o)
        };
        poly += p.perp_dot(q);
        if let Curve::Arc { r, sweep, .. } = c {
            bulge += r * r / 2.0 * (sweep - sweep.sin());
        }
    }
    poly / 2.0 + bulge
}

/// The ring of a cycle as points (arcs cut into chords), without a repeated first point.
pub fn cycle_ring(out: &Arrangement, cy: &Cycle) -> Vec<DVec2> {
    let mut ring = Vec::new();
    for &(ei, fwd) in &cy.edges {
        let e = &out.edges[ei];
        let c = if fwd { e.curve } else { e.curve.reversed() };
        let (p, q) = if fwd {
            (out.nodes[e.a], out.nodes[e.b])
        } else {
            (out.nodes[e.b], out.nodes[e.a])
        };
        ring.push(p);
        if let Curve::Arc { .. } = c {
            let pts = c.flatten(FLAT_ERR);
            ring.extend(pts[1..pts.len() - 1].iter().copied());
        }
        let _ = q;
    }
    ring
}

/// The vertices of a cycle for a curved polyline: corners, and a point on every arc.
pub fn cycle_vertices(out: &Arrangement, cy: &Cycle) -> Vec<Vertex> {
    let mut v = Vec::new();
    for &(ei, fwd) in &cy.edges {
        let e = &out.edges[ei];
        let c = if fwd { e.curve } else { e.curve.reversed() };
        let p = if fwd { out.nodes[e.a] } else { out.nodes[e.b] };
        v.push(Vertex::corner(p));
        if let Curve::Arc { .. } = c {
            v.push(Vertex::new(c.at(0.5), VertexKind::PointOnArc));
        }
    }
    v
}

/// A point inside the ring and outside the holes. Scans horizontal lines and takes the middle of
/// the widest stretch inside.
pub fn interior_point(ring: &[DVec2], holes: &[Vec<DVec2>]) -> DVec2 {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in ring {
        lo = lo.min(p.y);
        hi = hi.max(p.y);
    }
    let mut best: Option<(f64, DVec2)> = None;
    for k in 1..=15 {
        let y = lo + (hi - lo) * (k as f64 / 16.0);
        // The crossings of the line with the ring and the holes, sorted.
        let mut xs: Vec<f64> = Vec::new();
        let mut cross = |poly: &[DVec2]| {
            let n = poly.len();
            for i in 0..n {
                let (a, b) = (poly[i], poly[(i + 1) % n]);
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y));
                }
            }
        };
        cross(ring);
        for h in holes {
            cross(h);
        }
        xs.sort_by(f64::total_cmp);
        for w in xs.chunks(2) {
            if w.len() == 2 {
                let width = w[1] - w[0];
                if best.is_none_or(|(bw, _)| width > bw) {
                    best = Some((width, DVec2::new((w[0] + w[1]) / 2.0, y)));
                }
            }
        }
    }
    best.map_or_else(|| ring.first().copied().unwrap_or(DVec2::ZERO), |(_, p)| p)
}

#[cfg(test)]
mod tests;
