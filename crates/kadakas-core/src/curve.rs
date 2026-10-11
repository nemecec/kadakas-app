//! Curved polylines: vertices with a kind, and the exact curve they describe.
//!
//! Each vertex has a position and a kind (see [`VertexKind`]). `path` turns a
//! list of vertices into a [`Path`] of exact segments (lines, circular arcs,
//! quadratic and cubic Bezier curves). `flatten` turns a path into points
//! with a chord tolerance. `path_area` finds the exact area.
//!
//! How each kind works:
//! - `Corner`: a sharp corner on the curve.
//! - `Arc { radius }`: a fillet. The curve leaves the first segment early and
//!   joins the second one late with a circular arc that touches both. The
//!   radius is made smaller when the arc does not fit. The fillet is a
//!   plain corner at the ends of an open path, and next to a curved
//!   neighbor (point on arc, Bezier or cubic).
//! - `PointOnArc`: the curve passes through this point on the circle that
//!   goes through the previous point, this point and the next point. When two
//!   of these vertices are neighbors, the segment between them follows the
//!   circle of the first one. Three points on one line make a corner.
//! - `Bezier`: a quadratic control point. The curve starts at the nearest
//!   on-curve vertex before it, and ends at the nearest on-curve vertex after
//!   it. A run of control points gets an implied on-curve point halfway
//!   between each pair (the same rule as TrueType outlines).
//! - `Cubic`: a smooth point that the curve passes through. A run of these,
//!   with the on-curve vertices at its ends, is a centripetal Catmull-Rom
//!   spline written as cubic Bezier pieces. A run that mixes
//!   `Bezier` and `Cubic` vertices is treated as `Bezier`.
//!
//! The first and last vertex of an open path are always on the curve, so
//! their kind has no effect (they act as corners).

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use glam::DVec2;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Default chord tolerance for bounds, hit testing and drawing, in model units.
pub const DEFAULT_TOLERANCE: f64 = 0.01;
/// The most pieces that one curved segment is cut into.
const MAX_PIECES: usize = 4096;

/// What a vertex does to the curve. See the module notes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VertexKind {
    #[default]
    Corner,
    /// A fillet arc. Radius 0 means a corner.
    Arc {
        radius: f64,
    },
    PointOnArc,
    Bezier,
    Cubic,
}

/// A point of a polyline, with the kind of curve at that point.
///
/// In JSON a corner is a plain `[x, y]`. Other kinds are an object such as
/// `{"p": [x, y], "kind": "arc", "radius": 50}`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vertex {
    pub p: DVec2,
    pub kind: VertexKind,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ReprOut {
    Plain(DVec2),
    Full {
        p: DVec2,
        #[serde(flatten)]
        kind: VertexKind,
    },
}

/// A vertex object without a `kind`: a corner.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BareVertex {
    p: DVec2,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ReprIn {
    Plain(DVec2),
    Full {
        p: DVec2,
        #[serde(flatten)]
        kind: VertexKind,
    },
    Bare(BareVertex),
}

impl Serialize for Vertex {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.kind {
            VertexKind::Corner => ReprOut::Plain(self.p),
            kind => ReprOut::Full { p: self.p, kind },
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for Vertex {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match ReprIn::deserialize(d)? {
            ReprIn::Plain(p) => Vertex {
                p,
                kind: VertexKind::Corner,
            },
            ReprIn::Full { p, kind } => Vertex { p, kind },
            ReprIn::Bare(BareVertex { p }) => Vertex {
                p,
                kind: VertexKind::Corner,
            },
        })
    }
}

impl Vertex {
    pub fn corner(p: DVec2) -> Self {
        Vertex {
            p,
            kind: VertexKind::Corner,
        }
    }

    pub fn new(p: DVec2, kind: VertexKind) -> Self {
        Vertex { p, kind }
    }

    /// True if the position is finite and any radius is finite.
    pub fn is_finite(&self) -> bool {
        self.p.is_finite()
            && !matches!(self.kind, VertexKind::Arc { radius } if !radius.is_finite())
    }

    /// True if this vertex never bends the curve: a corner, or an arc of radius 0 or less.
    pub fn is_straight(&self) -> bool {
        match self.kind {
            VertexKind::Corner => true,
            VertexKind::Arc { radius } => radius <= 0.0,
            _ => false,
        }
    }
}

/// Corner vertices at the given points.
pub fn corners(points: &[DVec2]) -> Vec<Vertex> {
    points.iter().map(|p| Vertex::corner(*p)).collect()
}

/// The positions of the vertices.
pub fn positions(vertices: &[Vertex]) -> Vec<DVec2> {
    vertices.iter().map(|v| v.p).collect()
}

/// True if every vertex is a plain corner, so the outline is a polygon.
pub fn all_straight(vertices: &[Vertex]) -> bool {
    vertices.iter().all(Vertex::is_straight)
}

/// One exact piece of a path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Line {
        from: DVec2,
        to: DVec2,
    },
    /// Circular arc. `sweep` is in radians: positive is counterclockwise.
    Arc {
        from: DVec2,
        to: DVec2,
        center: DVec2,
        radius: f64,
        start: f64,
        sweep: f64,
    },
    Quad {
        from: DVec2,
        c: DVec2,
        to: DVec2,
    },
    Cubic {
        from: DVec2,
        c1: DVec2,
        c2: DVec2,
        to: DVec2,
    },
}

impl Seg {
    pub fn from(&self) -> DVec2 {
        match *self {
            Seg::Line { from, .. }
            | Seg::Arc { from, .. }
            | Seg::Quad { from, .. }
            | Seg::Cubic { from, .. } => from,
        }
    }

    pub fn to(&self) -> DVec2 {
        match *self {
            Seg::Line { to, .. }
            | Seg::Arc { to, .. }
            | Seg::Quad { to, .. }
            | Seg::Cubic { to, .. } => to,
        }
    }

    /// Direction of travel at the end of the segment (not normalized, may be zero).
    pub fn end_direction(&self) -> DVec2 {
        match *self {
            Seg::Line { from, to } => to - from,
            Seg::Arc { start, sweep, .. } => {
                let a = start + sweep;
                DVec2::new(-a.sin(), a.cos()) * sweep.signum()
            }
            Seg::Quad { c, to, .. } => to - c,
            Seg::Cubic { from, c1, c2, to } => {
                // Control points can sit on the end point, so look at the next ones back.
                [to - c2, to - c1, to - from]
                    .into_iter()
                    .find(|d| d.length_squared() > 0.0)
                    .unwrap_or(DVec2::ZERO)
            }
        }
    }
}

/// An exact path: a start point and the segments that follow it.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub start: DVec2,
    pub segs: Vec<Seg>,
    pub closed: bool,
}

impl Path {
    /// Direction of travel at the end of the last segment.
    pub fn end_direction(&self) -> Option<DVec2> {
        self.segs
            .iter()
            .rev()
            .map(Seg::end_direction)
            .find(|d| d.length_squared() > 0.0)
    }

    /// The path as an SVG `d` attribute. The Y axis is flipped, because SVG
    /// has Y pointing down. Arcs, quadratic and cubic curves are exact.
    pub fn svg_d(&self) -> String {
        // `0.0 - y` gives 0 and not -0 for a zero y.
        let pt = |p: DVec2| format!("{} {}", p.x, 0.0 - p.y);
        let mut d = format!("M {}", pt(self.start));
        for (i, seg) in self.segs.iter().enumerate() {
            // Z draws the closing line, so a last line back to the start is not needed.
            if self.closed
                && i + 1 == self.segs.len()
                && matches!(seg, Seg::Line { to, .. } if *to == self.start)
            {
                continue;
            }
            match *seg {
                Seg::Line { to, .. } => d += &format!(" L {}", pt(to)),
                Seg::Arc {
                    to, radius, sweep, ..
                } => {
                    // Y is flipped, so a counterclockwise arc has sweep-flag 0.
                    let large = (sweep.abs() > PI) as u8;
                    let flag = (sweep < 0.0) as u8;
                    d += &format!(" A {radius} {radius} 0 {large} {flag} {}", pt(to));
                }
                Seg::Quad { c, to, .. } => d += &format!(" Q {} {}", pt(c), pt(to)),
                Seg::Cubic { c1, c2, to, .. } => {
                    d += &format!(" C {} {} {}", pt(c1), pt(c2), pt(to))
                }
            }
        }
        if self.closed {
            d += " Z";
        }
        d
    }
}

/// How one vertex is used after the checks for the curve.
#[derive(Clone, Copy, Debug)]
enum Eff {
    Corner,
    Fillet {
        t1: DVec2,
        t2: DVec2,
        center: DVec2,
        radius: f64,
        start: f64,
        sweep: f64,
    },
    Through,
    Quad,
    Cubic,
}

fn is_control(e: &Eff) -> bool {
    matches!(e, Eff::Quad | Eff::Cubic)
}

/// The circle through three points: its center, or `None` for points on a line.
fn circumcenter(a: DVec2, m: DVec2, b: DVec2) -> Option<DVec2> {
    let (bb, cc) = (m - a, b - a);
    let cross = bb.perp_dot(cc);
    if cross.abs() <= 1e-9 * bb.length() * cc.length() || !cross.is_finite() {
        return None;
    }
    let d = 2.0 * cross;
    let ux = (cc.y * bb.length_squared() - bb.y * cc.length_squared()) / d;
    let uy = (bb.x * cc.length_squared() - cc.x * bb.length_squared()) / d;
    Some(a + DVec2::new(ux, uy))
}

/// Builds an arc segment from `from` to `to` around `center`, in the given direction.
fn arc_seg(center: DVec2, from: DVec2, to: DVec2, ccw: bool) -> Seg {
    let a0 = (from - center).to_angle();
    let a1 = (to - center).to_angle();
    let sweep = if ccw {
        (a1 - a0).rem_euclid(TAU)
    } else {
        -((a0 - a1).rem_euclid(TAU))
    };
    Seg::Arc {
        from,
        to,
        center,
        radius: (from - center).length(),
        start: a0,
        sweep,
    }
}

/// The angle at `p` between the directions to `pp` and `np`.
struct CornerAngle {
    u: DVec2,
    w: DVec2,
    theta: f64,
    half_tan: f64,
    half_sin: f64,
}

/// Finds the angle at a corner, or `None` when the corner is nearly straight or nearly a spike,
/// or a point is repeated. Then no fillet is possible and the vertex is a corner.
fn corner_angle(pp: DVec2, p: DVec2, np: DVec2) -> Option<CornerAngle> {
    let (lu, lw) = ((pp - p).length(), (np - p).length());
    if !(lu > 1e-12 && lw > 1e-12) {
        return None;
    }
    let (u, w) = ((pp - p) / lu, (np - p) / lw);
    let (sin, cos) = (u.perp_dot(w).abs(), u.dot(w));
    let theta = sin.atan2(cos);
    const EDGE: f64 = 1e-6;
    if !theta.is_finite() || !(EDGE..=PI - EDGE).contains(&theta) {
        return None;
    }
    // (1 - cos) / sin is exact for a right angle and stays finite, because sin > 1e-6 here.
    let half_tan = (1.0 - cos) / sin;
    let half_sin = ((1.0 - cos) / 2.0).max(0.0).sqrt();
    (half_tan.is_finite() && half_sin > 0.0).then_some(CornerAngle {
        u,
        w,
        theta,
        half_tan,
        half_sin,
    })
}

/// Decides how every vertex acts. Fillet geometry is found here, with the radius clamped.
fn classify(v: &[Vertex], closed: bool) -> Vec<Eff> {
    let n = v.len();
    let ring = closed && n >= 3;
    let has_nb = |i: usize| ring || (i > 0 && i + 1 < n);
    let prev = |i: usize| (i + n - 1) % n;
    let next = |i: usize| (i + 1) % n;
    let curved = |j: usize| {
        matches!(
            v[j].kind,
            VertexKind::PointOnArc | VertexKind::Bezier | VertexKind::Cubic
        )
    };
    let ctrl = |j: usize| matches!(v[j].kind, VertexKind::Bezier | VertexKind::Cubic);

    // First pass: which vertices keep their kind.
    let mut eff: Vec<Eff> = (0..n)
        .map(|i| {
            if !has_nb(i) {
                return Eff::Corner;
            }
            let (pp, p, np) = (v[prev(i)].p, v[i].p, v[next(i)].p);
            match v[i].kind {
                VertexKind::Bezier => Eff::Quad,
                VertexKind::Cubic => Eff::Cubic,
                VertexKind::PointOnArc
                    if !ctrl(prev(i)) && !ctrl(next(i)) && circumcenter(pp, p, np).is_some() =>
                {
                    Eff::Through
                }
                VertexKind::Arc { radius }
                    if radius > 0.0 && !curved(prev(i)) && !curved(next(i)) =>
                {
                    if corner_angle(pp, p, np).is_some() {
                        // A marker: the real values are found in the second pass.
                        Eff::Fillet {
                            t1: p,
                            t2: p,
                            center: p,
                            radius,
                            start: 0.0,
                            sweep: 0.0,
                        }
                    } else {
                        Eff::Corner
                    }
                }
                _ => Eff::Corner,
            }
        })
        .collect();

    // Second pass: fillet geometry. The wanted tangent length of each fillet is
    // radius / tan(half angle). When the two fillets on one segment (or one fillet and
    // a corner) want more than the segment, both are scaled down in proportion.
    let want: Vec<f64> = (0..n)
        .map(|i| match eff[i] {
            Eff::Fillet { radius, .. } => {
                let (pp, p, np) = (v[prev(i)].p, v[i].p, v[next(i)].p);
                corner_angle(pp, p, np).map_or(0.0, |a| radius / a.half_tan)
            }
            _ => 0.0,
        })
        .collect();
    let mut scale = vec![1.0_f64; n];
    let segments = if ring { n } else { n.saturating_sub(1) };
    for a in 0..segments {
        let b = next(a);
        let total = want[a] + want[b];
        let len = v[a].p.distance(v[b].p);
        if total > len && total > 0.0 {
            let f = len / total;
            scale[a] = scale[a].min(f);
            scale[b] = scale[b].min(f);
        }
    }
    for i in 0..n {
        let Eff::Fillet { .. } = eff[i] else { continue };
        let (pp, p, np) = (v[prev(i)].p, v[i].p, v[next(i)].p);
        let Some(g) = corner_angle(pp, p, np) else {
            continue;
        };
        let d = want[i] * scale[i];
        let r = d * g.half_tan;
        let center = p + (g.u + g.w).normalize() * (r / g.half_sin);
        let (t1, t2) = (p + g.u * d, p + g.w * d);
        // Walking prev -> p -> next, a left turn means a counterclockwise arc.
        let ccw = (-g.u).perp_dot(g.w) > 0.0;
        let sweep = if ccw { PI - g.theta } else { g.theta - PI };
        let ok = [d, r, center.x, center.y, t1.x, t1.y, t2.x, t2.y, sweep]
            .iter()
            .all(|x| x.is_finite())
            && d > 0.0;
        eff[i] = if ok {
            Eff::Fillet {
                t1,
                t2,
                center,
                radius: r,
                start: (t1 - center).to_angle(),
                sweep,
            }
        } else {
            Eff::Corner
        };
    }
    eff
}

/// The Bezier piece from `p1` to `p2` of the centripetal Catmull-Rom spline
/// through four points. `p0` and `p3` are the neighbors that set the tangents.
fn catmull_rom_piece(p0: DVec2, p1: DVec2, p2: DVec2, p3: DVec2) -> Seg {
    let d1 = p1.distance(p0).sqrt();
    let d2 = p2.distance(p1).sqrt();
    let d3 = p3.distance(p2).sqrt();
    let third = (p2 - p1) / 3.0;
    let (d1s, d2s, d3s) = (d1 * d1, d2 * d2, d3 * d3);
    let den1 = 3.0 * d1 * (d1 + d2);
    let den2 = 3.0 * d3 * (d3 + d2);
    let c1 = if den1 > 0.0 {
        (p2 * d1s - p0 * d2s + p1 * (2.0 * d1s + 3.0 * d1 * d2 + d2s)) / den1
    } else {
        p1 + third
    };
    let c2 = if den2 > 0.0 {
        (p1 * d3s - p3 * d2s + p2 * (2.0 * d3s + 3.0 * d3 * d2 + d2s)) / den2
    } else {
        p2 - third
    };
    Seg::Cubic {
        from: p1,
        c1,
        c2,
        to: p2,
    }
}

/// Segments for a run of control points between two on-curve points.
/// `a` and `b` are the on-curve ends.
fn run_segs(out: &mut Vec<Seg>, a: DVec2, ctrls: &[DVec2], b: DVec2, cubic: bool) {
    if cubic {
        // The curve passes through a, every control point and b.
        let mut pts = vec![a];
        pts.extend_from_slice(ctrls);
        pts.push(b);
        let m = pts.len();
        for i in 0..m - 1 {
            let p0 = if i == 0 {
                2.0 * pts[0] - pts[1]
            } else {
                pts[i - 1]
            };
            let p3 = if i + 2 >= m {
                2.0 * pts[m - 1] - pts[m - 2]
            } else {
                pts[i + 2]
            };
            out.push(catmull_rom_piece(p0, pts[i], pts[i + 1], p3));
        }
    } else {
        let mut from = a;
        for (i, &c) in ctrls.iter().enumerate() {
            let to = if i + 1 == ctrls.len() {
                b
            } else {
                (c + ctrls[i + 1]) / 2.0
            };
            out.push(Seg::Quad { from, c, to });
            from = to;
        }
    }
}

/// Builds the exact path for the vertices.
pub fn path(v: &[Vertex], closed: bool) -> Path {
    let n = v.len();
    // Fewer than three points cannot enclose anything.
    let closed = closed && n >= 3;
    if n == 0 {
        return Path {
            start: DVec2::ZERO,
            segs: vec![],
            closed,
        };
    }
    let eff = classify(v, closed);
    let entry = |i: usize| {
        if let Eff::Fillet { t1, .. } = eff[i] {
            t1
        } else {
            v[i].p
        }
    };
    let exit = |i: usize| {
        if let Eff::Fillet { t2, .. } = eff[i] {
            t2
        } else {
            v[i].p
        }
    };
    let ring = closed;
    let anchors: Vec<usize> = (0..n).filter(|&i| !is_control(&eff[i])).collect();
    let mut segs: Vec<Seg> = Vec::new();

    if anchors.is_empty() {
        // Closed loop of control points only.
        let cubic = eff.iter().all(|e| matches!(e, Eff::Cubic));
        let pts: Vec<DVec2> = v.iter().map(|x| x.p).collect();
        let start;
        if cubic {
            for i in 0..n {
                segs.push(catmull_rom_piece(
                    pts[(i + n - 1) % n],
                    pts[i],
                    pts[(i + 1) % n],
                    pts[(i + 2) % n],
                ));
            }
            start = pts[0];
        } else {
            start = (pts[n - 1] + pts[0]) / 2.0;
            let mut from = start;
            for i in 0..n {
                let to = (pts[i] + pts[(i + 1) % n]) / 2.0;
                segs.push(Seg::Quad {
                    from,
                    c: pts[i],
                    to,
                });
                from = to;
            }
        }
        // Close the loop exactly.
        match segs.last_mut() {
            Some(Seg::Cubic { to, .. }) | Some(Seg::Quad { to, .. }) => *to = start,
            _ => {}
        }
        return Path {
            start,
            segs,
            closed,
        };
    }

    // Joins the exit of anchor `ia` to the entry of anchor `ib`.
    let connect = |segs: &mut Vec<Seg>, ia: usize, ib: usize| {
        let (pa, pb) = (exit(ia), entry(ib));
        let steps = if ia == ib {
            n - 1
        } else {
            (ib + n - ia) % n - 1
        };
        if steps == 0 {
            let through = |m: usize| {
                let c =
                    circumcenter(v[(m + n - 1) % n].p, v[m].p, v[(m + 1) % n].p).expect("checked");
                let ccw = (v[m].p - v[(m + n - 1) % n].p)
                    .perp_dot(v[(m + 1) % n].p - v[(m + n - 1) % n].p)
                    > 0.0;
                (c, ccw)
            };
            if matches!(eff[ia], Eff::Through) {
                let (c, ccw) = through(ia);
                segs.push(arc_seg(c, pa, pb, ccw));
            } else if matches!(eff[ib], Eff::Through) {
                let (c, ccw) = through(ib);
                segs.push(arc_seg(c, pa, pb, ccw));
            } else if pa.distance(pb) > 1e-12 {
                segs.push(Seg::Line { from: pa, to: pb });
            }
        } else {
            let idx: Vec<usize> = (1..=steps).map(|k| (ia + k) % n).collect();
            let ctrls: Vec<DVec2> = idx.iter().map(|&i| v[i].p).collect();
            let cubic = idx.iter().all(|&i| matches!(eff[i], Eff::Cubic));
            run_segs(segs, pa, &ctrls, pb, cubic);
        }
    };
    let fillet_arc = |segs: &mut Vec<Seg>, i: usize| {
        if let Eff::Fillet {
            t1,
            t2,
            center,
            radius,
            start,
            sweep,
        } = eff[i]
        {
            segs.push(Seg::Arc {
                from: t1,
                to: t2,
                center,
                radius,
                start,
                sweep,
            });
        }
    };

    let start;
    if ring {
        start = exit(anchors[0]);
        let m = anchors.len();
        for k in 0..m {
            let (a, b) = (anchors[k], anchors[(k + 1) % m]);
            connect(&mut segs, a, b);
            fillet_arc(&mut segs, b);
        }
    } else {
        start = v[0].p;
        for w in anchors.windows(2) {
            connect(&mut segs, w[0], w[1]);
            fillet_arc(&mut segs, w[1]);
        }
    }
    Path {
        start,
        segs,
        closed,
    }
}

fn arc_pieces(radius: f64, sweep: f64, tol: f64) -> usize {
    let step = if tol >= radius {
        FRAC_PI_2
    } else {
        (2.0 * (1.0 - tol / radius).acos()).min(FRAC_PI_2)
    };
    ((sweep.abs() / step).ceil() as usize).clamp(1, MAX_PIECES)
}

fn curve_pieces(second_diff: f64, factor: f64, tol: f64) -> usize {
    ((factor * second_diff / tol).sqrt().ceil() as usize).clamp(1, MAX_PIECES)
}

/// Appends the points of a segment after its start point. The last point is exact.
fn flatten_seg(out: &mut Vec<DVec2>, seg: &Seg, tol: f64) {
    match *seg {
        Seg::Line { to, .. } => out.push(to),
        Seg::Arc {
            to,
            center,
            radius,
            start,
            sweep,
            ..
        } => {
            let k = arc_pieces(radius, sweep, tol);
            for i in 1..k {
                out.push(center + DVec2::from_angle(start + sweep * i as f64 / k as f64) * radius);
            }
            out.push(to);
        }
        Seg::Quad { from, c, to } => {
            // The curve is at most |from - 2c + to| / 4 / k^2 away from its chord pieces.
            let k = curve_pieces((from - 2.0 * c + to).length(), 0.25, tol);
            for i in 1..k {
                let t = i as f64 / k as f64;
                out.push(from * ((1.0 - t) * (1.0 - t)) + c * (2.0 * (1.0 - t) * t) + to * (t * t));
            }
            out.push(to);
        }
        Seg::Cubic { from, c1, c2, to } => {
            let m = (from - 2.0 * c1 + c2)
                .length()
                .max((c1 - 2.0 * c2 + to).length());
            let k = curve_pieces(m, 0.75, tol);
            for i in 1..k {
                let t = i as f64 / k as f64;
                let s = 1.0 - t;
                out.push(
                    from * (s * s * s)
                        + c1 * (3.0 * s * s * t)
                        + c2 * (3.0 * s * t * t)
                        + to * (t * t * t),
                );
            }
            out.push(to);
        }
    }
}

/// Points along the path. No point is more than `tolerance` from the curve
/// between two neighbors. The ends of every segment are exact. A closed
/// path does not repeat its first point at the end.
pub fn flatten_path(path: &Path, tolerance: f64) -> Vec<DVec2> {
    let tol = if tolerance.is_finite() && tolerance > 0.0 {
        tolerance
    } else {
        DEFAULT_TOLERANCE
    };
    let mut out = vec![path.start];
    for seg in &path.segs {
        flatten_seg(&mut out, seg, tol);
    }
    if path.closed && out.len() > 1 {
        out.pop();
    }
    out
}

/// Like [`flatten`], with a budget: `None` as soon as the points pass `max_points`. The work is
/// at most `max_points` plus the pieces of one segment, whatever the vertices ask for.
pub fn flatten_limited(
    vertices: &[Vertex],
    closed: bool,
    tolerance: f64,
    max_points: usize,
) -> Option<Vec<DVec2>> {
    if vertices.len() > max_points {
        return None;
    }
    if all_straight(vertices) {
        return Some(positions(vertices));
    }
    let tol = if tolerance.is_finite() && tolerance > 0.0 {
        tolerance
    } else {
        DEFAULT_TOLERANCE
    };
    let path = path(vertices, closed);
    let mut out = vec![path.start];
    for seg in &path.segs {
        flatten_seg(&mut out, seg, tol);
        if out.len() > max_points {
            return None;
        }
    }
    if path.closed && out.len() > 1 {
        out.pop();
    }
    Some(out)
}

/// Points along the curve of the vertices. See `flatten_path`.
pub fn flatten(vertices: &[Vertex], closed: bool, tolerance: f64) -> Vec<DVec2> {
    if all_straight(vertices) {
        return positions(vertices);
    }
    flatten_path(&path(vertices, closed), tolerance)
}

fn grow(b: &mut (DVec2, DVec2), p: DVec2) {
    b.0 = b.0.min(p);
    b.1 = b.1.max(p);
}

/// Roots of `a t^2 + b t + c` inside the open interval (0, 1).
fn roots_in_unit(a: f64, b: f64, c: f64) -> Vec<f64> {
    let mut out = Vec::new();
    if a.abs() < 1e-14 {
        if b.abs() > 1e-14 {
            out.push(-c / b);
        }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc >= 0.0 {
            let sq = disc.sqrt();
            out.push((-b + sq) / (2.0 * a));
            out.push((-b - sq) / (2.0 * a));
        }
    }
    out.retain(|t| *t > 0.0 && *t < 1.0);
    out
}

/// Exact bounding box (min, max) of the path: the ends of every segment and the
/// extreme points of the arcs and Bezier pieces. An empty path gives (INF, -INF).
pub fn path_bounds(path: &Path) -> (DVec2, DVec2) {
    let mut b = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    if path.segs.is_empty() && path.start == DVec2::ZERO {
        return b;
    }
    grow(&mut b, path.start);
    for seg in &path.segs {
        grow(&mut b, seg.to());
        match *seg {
            Seg::Line { .. } => {}
            Seg::Arc {
                center,
                radius,
                start,
                sweep,
                ..
            } => {
                for k in 0..4 {
                    let angle = k as f64 * FRAC_PI_2;
                    let inside = if sweep >= 0.0 {
                        (angle - start).rem_euclid(TAU) <= sweep
                    } else {
                        (start - angle).rem_euclid(TAU) <= -sweep
                    };
                    if inside {
                        grow(&mut b, center + DVec2::from_angle(angle) * radius);
                    }
                }
            }
            Seg::Quad { from, c, to } => {
                for axis in 0..2 {
                    let (p0, p1, p2) = (from[axis], c[axis], to[axis]);
                    // The derivative p0(1-t) + ... is zero at t = (p0 - p1) / (p0 - 2 p1 + p2).
                    for t in roots_in_unit(0.0, p0 - 2.0 * p1 + p2, p1 - p0) {
                        let s = 1.0 - t;
                        grow(&mut b, from * (s * s) + c * (2.0 * s * t) + to * (t * t));
                    }
                }
            }
            Seg::Cubic { from, c1, c2, to } => {
                for axis in 0..2 {
                    let (d0, d1, d2) = (
                        c1[axis] - from[axis],
                        c2[axis] - c1[axis],
                        to[axis] - c2[axis],
                    );
                    for t in roots_in_unit(d0 - 2.0 * d1 + d2, 2.0 * (d1 - d0), d0) {
                        let s = 1.0 - t;
                        grow(
                            &mut b,
                            from * (s * s * s)
                                + c1 * (3.0 * s * s * t)
                                + c2 * (3.0 * s * t * t)
                                + to * (t * t * t),
                        );
                    }
                }
            }
        }
    }
    b
}

/// Exact bounding box of the curve of the vertices.
pub fn bounds(vertices: &[Vertex], closed: bool) -> (DVec2, DVec2) {
    if all_straight(vertices) {
        return vertices.iter().fold(
            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
            |(lo, hi), v| (lo.min(v.p), hi.max(v.p)),
        );
    }
    path_bounds(&path(vertices, closed))
}

/// Exact area enclosed by a closed path (zero if the path is open).
pub fn path_area(path: &Path) -> f64 {
    if !path.closed {
        return 0.0;
    }
    // Green's theorem: twice the area is the integral of x dy - y dx.
    // The Bezier integrands are polynomials of degree 5 or less, so
    // 3-point Gauss-Legendre is exact.
    const NODES: [(f64, f64); 3] = [
        (0.5 - 0.387_298_334_620_741_7, 5.0 / 18.0),
        (0.5, 8.0 / 18.0),
        (0.5 + 0.387_298_334_620_741_7, 5.0 / 18.0),
    ];
    let mut twice = 0.0;
    for seg in &path.segs {
        twice += match *seg {
            Seg::Line { from, to } => from.perp_dot(to),
            Seg::Arc {
                center,
                radius,
                start,
                sweep,
                ..
            } => {
                let (t0, t1) = (start, start + sweep);
                radius * radius * sweep + radius * center.x * (t1.sin() - t0.sin())
                    - radius * center.y * (t1.cos() - t0.cos())
            }
            Seg::Quad { from, c, to } => NODES
                .iter()
                .map(|&(t, w)| {
                    let s = 1.0 - t;
                    let p = from * (s * s) + c * (2.0 * s * t) + to * (t * t);
                    let d = (c - from) * (2.0 * s) + (to - c) * (2.0 * t);
                    w * p.perp_dot(d)
                })
                .sum(),
            Seg::Cubic { from, c1, c2, to } => NODES
                .iter()
                .map(|&(t, w)| {
                    let s = 1.0 - t;
                    let p = from * (s * s * s)
                        + c1 * (3.0 * s * s * t)
                        + c2 * (3.0 * s * t * t)
                        + to * (t * t * t);
                    let d = (c1 - from) * (3.0 * s * s)
                        + (c2 - c1) * (6.0 * s * t)
                        + (to - c2) * (3.0 * t * t);
                    w * p.perp_dot(d)
                })
                .sum(),
        };
    }
    twice.abs() / 2.0
}

/// Exact area of a closed outline.
pub fn area(vertices: &[Vertex]) -> f64 {
    if all_straight(vertices) {
        return crate::geom::polygon_area(&positions(vertices));
    }
    path_area(&path(vertices, true))
}

/// A vertex list that continues the last segment of an open path with an
/// arc that is tangent to it and ends at `end`. It returns the vertices to
/// append: a point on the arc, then `end`. It returns `None` when `end` is on
/// the tangent line (a straight line is the answer then) or there is no tangent.
pub fn tangent_arc_vertices(existing: &[Vertex], end: DVec2) -> Option<[Vertex; 2]> {
    let last = existing.last()?;
    // The last vertex is an end point, so it is a corner for the tangent.
    let dir = path(existing, false).end_direction()?.normalize();
    let from = last.p;
    let d = end - from;
    let normal = dir.perp();
    let nd = normal.dot(d);
    if nd.abs() <= 1e-9 * d.length() || d.length() <= 1e-12 {
        return None;
    }
    let s = d.length_squared() / (2.0 * nd);
    let center = from + normal * s;
    let r = s.abs();
    let ccw = s > 0.0;
    let a0 = (from - center).to_angle();
    let a1 = (end - center).to_angle();
    let phi = if ccw {
        (a1 - a0).rem_euclid(TAU)
    } else {
        (a0 - a1).rem_euclid(TAU)
    };
    let mid_angle = if ccw { a0 + phi / 2.0 } else { a0 - phi / 2.0 };
    let m = center + DVec2::from_angle(mid_angle) * r;
    Some([Vertex::new(m, VertexKind::PointOnArc), Vertex::corner(end)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn square(s: f64, kind: VertexKind) -> Vec<Vertex> {
        [v(0.0, 0.0), v(s, 0.0), v(s, s), v(0.0, s)]
            .iter()
            .map(|p| Vertex::new(*p, kind))
            .collect()
    }

    #[test]
    fn rounded_square_area_is_exact() {
        for (s, r) in [(100.0, 10.0), (1000.0, 123.0), (50.0, 25.0)] {
            let verts = square(s, VertexKind::Arc { radius: r });
            let expected = s * s - (4.0 - PI) * r * r;
            let got = area(&verts);
            assert!(
                (got - expected).abs() <= 1e-9 * expected,
                "{got} vs {expected}"
            );
        }
    }

    #[test]
    fn area_of_translated_rounded_square_is_unchanged() {
        let verts: Vec<Vertex> = square(100.0, VertexKind::Arc { radius: 10.0 })
            .into_iter()
            .map(|x| Vertex::new(x.p + v(1e4, -2e4), x.kind))
            .collect();
        let expected = 100.0 * 100.0 - (4.0 - PI) * 100.0;
        assert!((area(&verts) - expected).abs() < 1e-6 * expected);
    }

    #[test]
    fn fillet_radius_is_clamped_to_fit() {
        // Radius 1000 on a 100 square: the fillets touch at the middle of each side.
        let p = path(&square(100.0, VertexKind::Arc { radius: 1000.0 }), true);
        let radii: Vec<f64> = p
            .segs
            .iter()
            .filter_map(|s| {
                if let Seg::Arc { radius, .. } = s {
                    Some(*radius)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(radii.len(), 4);
        for r in radii {
            assert!((r - 50.0).abs() < 1e-9, "{r}");
        }
        // The shape is a circle of radius 50.
        let expected = PI * 50.0 * 50.0;
        assert!((path_area(&p) - expected).abs() < 1e-6 * expected);
    }

    #[test]
    fn fillet_next_to_a_corner_may_use_the_whole_segment() {
        let verts = vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::new(v(10.0, 0.0), VertexKind::Arc { radius: 100.0 }),
            Vertex::corner(v(10.0, 10.0)),
        ];
        let p = path(&verts, false);
        // The tangent length is 10 (the whole short side), so the radius is 10.
        let Seg::Arc {
            radius, from, to, ..
        } = p.segs[0]
        else {
            panic!("{p:?}")
        };
        assert!((radius - 10.0).abs() < 1e-9);
        assert!(from.distance(v(0.0, 0.0)) < 1e-9 && to.distance(v(10.0, 10.0)) < 1e-9);
    }

    #[test]
    fn zero_or_degenerate_radius_is_a_corner() {
        let verts = square(10.0, VertexKind::Arc { radius: 0.0 });
        assert_eq!(flatten(&verts, true, 0.1), positions(&verts));
        // Three points on a line cannot be filleted.
        let line = vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::new(v(5.0, 0.0), VertexKind::Arc { radius: 3.0 }),
            Vertex::corner(v(10.0, 0.0)),
        ];
        assert_eq!(flatten(&line, false, 0.1), positions(&line));
        // The ends of an open path are corners. Repeated points do not give NaN.
        let dup = vec![
            Vertex::new(v(0.0, 0.0), VertexKind::Arc { radius: 3.0 }),
            Vertex::new(v(0.0, 0.0), VertexKind::Arc { radius: 3.0 }),
            Vertex::new(v(4.0, 4.0), VertexKind::Arc { radius: 3.0 }),
        ];
        assert!(flatten(&dup, true, 0.1).iter().all(|p| p.is_finite()));
    }

    #[test]
    fn point_on_arc_with_collinear_points_is_a_corner() {
        let verts = vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::new(v(5.0, 0.0), VertexKind::PointOnArc),
            Vertex::corner(v(10.0, 0.0)),
        ];
        let p = path(&verts, false);
        assert!(p.segs.iter().all(|s| matches!(s, Seg::Line { .. })));
        assert_eq!(flatten(&verts, false, 0.1), positions(&verts));
    }

    #[test]
    fn point_on_arc_passes_through_the_point() {
        let verts = vec![
            Vertex::corner(v(-10.0, 0.0)),
            Vertex::new(v(0.0, 10.0), VertexKind::PointOnArc),
            Vertex::corner(v(10.0, 0.0)),
        ];
        let pts = flatten(&verts, false, 0.01);
        assert!(pts.contains(&v(0.0, 10.0)));
        // A half circle of radius 10 around the origin.
        for q in &pts {
            assert!((q.length() - 10.0).abs() < 0.011, "{q:?}");
        }
        let svg = path(&verts, false).svg_d();
        assert!(svg.contains("A 10 10 0 0 1"), "{svg}");
    }

    #[test]
    fn flatten_respects_the_tolerance_and_keeps_endpoints() {
        let verts = square(100.0, VertexKind::Arc { radius: 30.0 });
        let p = path(&verts, true);
        for tol in [1.0, 0.1, 0.001] {
            let pts = flatten_path(&p, tol);
            // The ends of every segment appear exactly.
            for seg in &p.segs {
                assert!(pts.contains(&seg.to()), "{seg:?}");
            }
            // The middle of each chord of an arc is within the tolerance of the circle.
            for w in pts.windows(2) {
                for seg in &p.segs {
                    if let Seg::Arc { center, radius, .. } = seg {
                        let mid = (w[0] + w[1]) / 2.0;
                        let on_arc = (w[0].distance(*center) - radius).abs() < 1e-9
                            && (w[1].distance(*center) - radius).abs() < 1e-9;
                        if on_arc {
                            assert!(radius - mid.distance(*center) <= tol + 1e-9, "tol {tol}");
                        }
                    }
                }
            }
        }
        // A smaller tolerance never gives fewer points.
        assert!(flatten_path(&p, 0.001).len() > flatten_path(&p, 1.0).len());
        // Bad tolerances fall back to the default.
        assert_eq!(
            flatten_path(&p, 0.0).len(),
            flatten_path(&p, DEFAULT_TOLERANCE).len()
        );
        assert_eq!(
            flatten_path(&p, f64::NAN).len(),
            flatten_path(&p, DEFAULT_TOLERANCE).len()
        );
    }

    #[test]
    fn quadratic_curve_and_area() {
        // Open: a control point between two corners.
        let verts = vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::new(v(5.0, 10.0), VertexKind::Bezier),
            Vertex::corner(v(10.0, 0.0)),
        ];
        let p = path(&verts, false);
        assert!(matches!(p.segs.as_slice(), [Seg::Quad { .. }]));
        let pts = flatten_path(&p, 0.001);
        assert_eq!((pts[0], *pts.last().unwrap()), (v(0.0, 0.0), v(10.0, 0.0)));
        let top = pts.iter().map(|q| q.y).fold(0.0, f64::max);
        assert!((top - 5.0).abs() < 0.001);
        // Closed with the base line: a parabola segment, area = 2/3 * base * height.
        let mut closed = verts.clone();
        closed.push(Vertex::corner(v(5.0, 0.0)));
        let a = area(&closed);
        let expected = 2.0 / 3.0 * 10.0 * 5.0;
        // The extra corner is on the base line, so the area is the same.
        assert!((a - expected).abs() < 1e-9, "{a}");
        // Two controls in a row get an implied point halfway between them.
        let two = vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::new(v(2.0, 4.0), VertexKind::Bezier),
            Vertex::new(v(6.0, 4.0), VertexKind::Bezier),
            Vertex::corner(v(8.0, 0.0)),
        ];
        let p = path(&two, false);
        assert_eq!(p.segs.len(), 2);
        assert_eq!(p.segs[0].to(), v(4.0, 4.0));
    }

    #[test]
    fn cubic_spline_passes_through_every_point() {
        let verts = vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::new(v(10.0, 20.0), VertexKind::Cubic),
            Vertex::new(v(20.0, 20.0), VertexKind::Cubic),
            Vertex::corner(v(30.0, 0.0)),
        ];
        let p = path(&verts, false);
        assert_eq!(p.start, v(0.0, 0.0));
        assert_eq!(p.segs.last().unwrap().to(), v(30.0, 0.0));
        for w in p.segs.windows(2) {
            assert!(w[0].to().distance(w[1].from()) < 1e-9);
        }
        // The curve goes through both inner points (each is the end of a piece).
        for x in &verts[1..] {
            assert!(p.segs.iter().any(|s| s.to() == x.p), "{:?}", x.p);
        }
        assert_eq!(p.segs.len(), 3);
        assert!(p.svg_d().contains(" C "));
        // Coincident points do not give NaN.
        let same = vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::new(v(0.0, 0.0), VertexKind::Cubic),
            Vertex::new(v(0.0, 0.0), VertexKind::Cubic),
            Vertex::corner(v(5.0, 5.0)),
        ];
        assert!(flatten(&same, false, 0.1).iter().all(|q| q.is_finite()));
    }

    #[test]
    fn closed_loops_of_controls_are_closed_curves() {
        for kind in [VertexKind::Bezier, VertexKind::Cubic] {
            let verts = square(10.0, kind);
            let p = path(&verts, true);
            assert!(p.segs.last().unwrap().to().distance(p.start) < 1e-12);
            let a = path_area(&p);
            assert!(a > 40.0 && a < 160.0, "{a}");
            if kind == VertexKind::Bezier {
                // The curve lies inside the control square.
                let pts = flatten_path(&p, 0.01);
                assert!(pts.iter().all(|q| q.x >= -1e-9 && q.x <= 10.0 + 1e-9));
            } else {
                // A cubic loop passes through every point of the square.
                for x in &verts {
                    assert!(p.segs.iter().any(|s| s.to() == x.p), "{:?}", x.p);
                }
            }
        }
        // Mixed kinds in a loop count as Bezier.
        let mut mixed = square(10.0, VertexKind::Cubic);
        mixed[0].kind = VertexKind::Bezier;
        assert!(
            path(&mixed, true)
                .segs
                .iter()
                .all(|s| matches!(s, Seg::Quad { .. }))
        );
    }

    #[test]
    fn mixed_kinds_stay_connected() {
        let verts = vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::new(v(20.0, 0.0), VertexKind::Arc { radius: 5.0 }),
            Vertex::new(v(20.0, 20.0), VertexKind::Bezier),
            Vertex::new(v(40.0, 20.0), VertexKind::PointOnArc),
            Vertex::new(v(60.0, 0.0), VertexKind::Arc { radius: 5.0 }),
            Vertex::corner(v(60.0, -20.0)),
        ];
        let p = path(&verts, false);
        let mut at = p.start;
        for s in &p.segs {
            assert!(s.from().distance(at) < 1e-9, "{s:?}");
            at = s.to();
        }
        assert_eq!(at, v(60.0, -20.0));
        // Closed variant ends where it starts.
        let p = path(&verts, true);
        assert!(p.segs.last().unwrap().to().distance(p.start) < 1e-9);
    }

    #[test]
    fn serde_is_compatible_with_plain_points() {
        let old: Vec<Vertex> = serde_json::from_str("[[0,0],[1.5,2]]").unwrap();
        assert_eq!(old, corners(&[v(0.0, 0.0), v(1.5, 2.0)]));
        assert_eq!(
            serde_json::to_string(&old).unwrap(),
            "[[0.0,0.0],[1.5,2.0]]"
        );
        let kinds = [
            VertexKind::Arc { radius: 12.5 },
            VertexKind::PointOnArc,
            VertexKind::Bezier,
            VertexKind::Cubic,
            VertexKind::Corner,
        ];
        for k in kinds {
            let x = Vertex::new(v(3.0, 4.0), k);
            let text = serde_json::to_string(&x).unwrap();
            assert_eq!(serde_json::from_str::<Vertex>(&text).unwrap(), x, "{text}");
        }
        let arc: Vertex = serde_json::from_str(r#"{"p":[1,2],"kind":"arc","radius":50}"#).unwrap();
        assert_eq!(
            arc,
            Vertex::new(v(1.0, 2.0), VertexKind::Arc { radius: 50.0 })
        );
        assert!(serde_json::from_str::<Vertex>(r#"{"p":[1,2],"kind":"wavy"}"#).is_err());
        assert!(serde_json::from_str::<Vertex>(r#""x""#).is_err());
    }

    #[test]
    fn vertex_helpers() {
        assert!(Vertex::corner(DVec2::ZERO).is_finite());
        assert!(!Vertex::new(DVec2::ZERO, VertexKind::Arc { radius: f64::NAN }).is_finite());
        assert!(!Vertex::corner(v(f64::INFINITY, 0.0)).is_finite());
        assert!(Vertex::new(DVec2::ZERO, VertexKind::Arc { radius: 0.0 }).is_straight());
        assert!(!Vertex::new(DVec2::ZERO, VertexKind::Bezier).is_straight());
        assert!(path(&[], true).segs.is_empty());
        assert!(path(&[Vertex::corner(v(1.0, 1.0))], false).segs.is_empty());
        assert_eq!(
            path_area(&path(
                &[Vertex::corner(DVec2::ZERO), Vertex::corner(DVec2::X)],
                false
            )),
            0.0
        );
    }

    #[test]
    fn end_direction_and_svg_of_every_segment() {
        let verts = vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::new(v(10.0, 0.0), VertexKind::Arc { radius: 2.0 }),
            Vertex::corner(v(10.0, 10.0)),
        ];
        let p = path(&verts, false);
        let d = p.end_direction().unwrap();
        assert!(d.normalize().distance(v(0.0, 1.0)) < 1e-9);
        let svg = p.svg_d();
        assert!(
            svg.starts_with("M 0 0 L 8 0 A 2 2 0 0 0 10 -2 L 10 -10"),
            "{svg}"
        );
        let verts = vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::new(v(10.0, 0.0), VertexKind::Arc { radius: 2.0 }),
            Vertex::corner(v(10.0, -10.0)),
        ];
        // A clockwise fillet has sweep-flag 1.
        assert!(path(&verts, true).svg_d().contains("A 2 2 0 0 1"));
        assert!(path(&verts, true).svg_d().ends_with(" Z"));
        // End directions of curves.
        let q = path(
            &[
                Vertex::corner(v(0.0, 0.0)),
                Vertex::new(v(0.0, 5.0), VertexKind::Bezier),
                Vertex::corner(v(5.0, 5.0)),
            ],
            false,
        );
        assert!(q.end_direction().unwrap().normalize().distance(v(1.0, 0.0)) < 1e-9);
        assert!(q.svg_d().contains(" Q "));
        let c = path(
            &[
                Vertex::corner(v(0.0, 0.0)),
                Vertex::new(v(0.0, 5.0), VertexKind::Cubic),
                Vertex::new(v(5.0, 5.0), VertexKind::Cubic),
                Vertex::corner(v(5.0, 0.0)),
            ],
            false,
        );
        assert!(c.end_direction().is_some());
        assert!(
            path(&[Vertex::corner(DVec2::ZERO)], false)
                .end_direction()
                .is_none()
        );
    }

    #[test]
    fn tangent_arc_continues_the_last_segment() {
        let existing = vec![Vertex::corner(v(0.0, 0.0)), Vertex::corner(v(10.0, 0.0))];
        let add = tangent_arc_vertices(&existing, v(20.0, 10.0)).unwrap();
        let mut all = existing.clone();
        all.extend(add);
        let p = path(&all, false);
        // A circle of radius 10 centered at (10, 10): tangent to the line at (10, 0).
        for s in &p.segs[1..] {
            let Seg::Arc { center, radius, .. } = s else {
                panic!("{s:?}")
            };
            assert!(center.distance(v(10.0, 10.0)) < 1e-9 && (radius - 10.0).abs() < 1e-9);
        }
        assert_eq!(p.segs.last().unwrap().to(), v(20.0, 10.0));
        let end_dir = p.end_direction().unwrap().normalize();
        assert!(end_dir.distance(v(0.0, 1.0)) < 1e-9);
        // Clockwise side and a point behind the start (long way around).
        let cw = tangent_arc_vertices(&existing, v(20.0, -10.0)).unwrap();
        assert!(cw[0].p.y < 0.0);
        let back = tangent_arc_vertices(&existing, v(5.0, 5.0)).unwrap();
        let mut all = existing.clone();
        all.extend(back);
        let p = path(&all, false);
        assert_eq!(p.segs.last().unwrap().to(), v(5.0, 5.0));
        // On the tangent line, or no segment yet: no arc.
        assert!(tangent_arc_vertices(&existing, v(20.0, 0.0)).is_none());
        assert!(tangent_arc_vertices(&existing, v(10.0, 0.0)).is_none());
        assert!(tangent_arc_vertices(&[Vertex::corner(DVec2::ZERO)], v(1.0, 1.0)).is_none());
        assert!(tangent_arc_vertices(&[], v(1.0, 1.0)).is_none());
    }

    #[test]
    fn nearly_straight_fillet_is_a_corner_not_nan() {
        // sin of the angle is about 2e-9 and cos rounds to -1.
        let verts = vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::new(v(1000.0, 1e-6), VertexKind::Arc { radius: 10.0 }),
            Vertex::corner(v(2000.0, 0.0)),
            Vertex::corner(v(1000.0, -500.0)),
        ];
        let p = path(&verts, true);
        assert!(p.segs.iter().all(|s| !matches!(s, Seg::Arc { .. })));
        assert!(area(&verts).is_finite());
        assert!(p.svg_d().find("NaN").is_none());
        assert!(flatten(&verts, true, 0.1).iter().all(|q| q.is_finite()));
        // Just past the limit still gives finite numbers.
        for y in [1e-3, 1e-2, 1.0] {
            let mut w = verts.clone();
            w[1].p.y = y;
            assert!(
                area(&w).is_finite() && !path(&w, true).svg_d().contains("NaN"),
                "{y}"
            );
        }
    }

    #[test]
    fn two_fillets_on_one_side_share_it_in_proportion() {
        let rect = |r0: f64, r1: f64| {
            vec![
                Vertex::new(v(0.0, 0.0), VertexKind::Arc { radius: r0 }),
                Vertex::new(v(100.0, 0.0), VertexKind::Arc { radius: r1 }),
                Vertex::corner(v(100.0, 1000.0)),
                Vertex::corner(v(0.0, 1000.0)),
            ]
        };
        let radii = |verts: &[Vertex]| -> Vec<f64> {
            let mut r: Vec<f64> = path(verts, true)
                .segs
                .iter()
                .filter_map(|s| {
                    if let Seg::Arc { radius, .. } = s {
                        Some(*radius)
                    } else {
                        None
                    }
                })
                .collect();
            r.sort_by(f64::total_cmp);
            r
        };
        // 90 + 5 fit on 100: both keep their radius.
        let r = radii(&rect(90.0, 5.0));
        assert!(
            (r[1] - 90.0).abs() < 1e-9 && (r[0] - 5.0).abs() < 1e-9,
            "{r:?}"
        );
        // 90 + 30 do not fit: both are scaled by 100/120.
        let r = radii(&rect(90.0, 30.0));
        assert!(
            (r[1] - 75.0).abs() < 1e-9 && (r[0] - 25.0).abs() < 1e-9,
            "{r:?}"
        );
    }

    #[test]
    fn bounds_are_exact_for_arcs_and_beziers() {
        // A circle of radius 50 made of four fillets.
        let (lo, hi) = bounds(&square(100.0, VertexKind::Arc { radius: 1000.0 }), true);
        assert!(lo.distance(v(0.0, 0.0)) < 1e-9 && hi.distance(v(100.0, 100.0)) < 1e-9);
        // Point on arc: the top of the circle is not a vertex.
        let b = bounds(
            &[
                Vertex::corner(v(-10.0, 5.0)),
                Vertex::new(v(0.0, 15.0), VertexKind::PointOnArc),
                Vertex::corner(v(10.0, 5.0)),
            ],
            false,
        );
        assert!((b.1.y - 15.0).abs() < 1e-9 || b.1.y > 15.0 - 1e-9);
        // The arc with the clockwise sweep through the top is bounded by the circle.
        let quad = bounds(
            &[
                Vertex::corner(v(0.0, 0.0)),
                Vertex::new(v(5.0, 10.0), VertexKind::Bezier),
                Vertex::corner(v(10.0, 0.0)),
            ],
            false,
        );
        assert!((quad.1.y - 5.0).abs() < 1e-12 && quad.0.y == 0.0);
        // A cubic bump stays within its exact extremes.
        let verts = [
            Vertex::corner(v(0.0, 0.0)),
            Vertex::new(v(10.0, 20.0), VertexKind::Cubic),
            Vertex::new(v(20.0, 20.0), VertexKind::Cubic),
            Vertex::corner(v(30.0, 0.0)),
        ];
        let b = bounds(&verts, false);
        let flat = flatten(&verts, false, 1e-4);
        let top = flat.iter().map(|q| q.y).fold(f64::MIN, f64::max);
        assert!(
            b.1.y >= top - 1e-12 && b.1.y - top < 1e-3,
            "{} {top}",
            b.1.y
        );
        // Empty and straight inputs.
        assert_eq!(bounds(&[], false).0, DVec2::splat(f64::INFINITY));
        assert_eq!(
            bounds(&corners(&[v(1.0, 2.0), v(3.0, -1.0)]), false),
            (v(1.0, -1.0), v(3.0, 2.0))
        );
        // A counterclockwise arc that crosses the right extreme.
        let ccw = bounds(
            &[
                Vertex::corner(v(0.0, -10.0)),
                Vertex::new(v(10.0, 0.0), VertexKind::PointOnArc),
                Vertex::corner(v(0.0, 10.0)),
            ],
            false,
        );
        assert!((ccw.1.x - 10.0).abs() < 1e-9);
    }

    #[test]
    fn vertex_object_without_kind_is_a_corner() {
        let x: Vertex = serde_json::from_str(r#"{"p":[1,2]}"#).unwrap();
        assert_eq!(x, Vertex::corner(v(1.0, 2.0)));
        // An unknown kind, or a radius without a kind, is still an error.
        assert!(serde_json::from_str::<Vertex>(r#"{"p":[1,2],"kind":"wavy"}"#).is_err());
        assert!(serde_json::from_str::<Vertex>(r#"{"p":[1,2],"radius":5}"#).is_err());
    }
}
