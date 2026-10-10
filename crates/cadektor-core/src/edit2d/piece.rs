//! Exact pieces of a curve (line segments and circular arcs) and chains of them.
//!
//! The editing commands (trim, extend, split, fillet, chamfer, offset) work on chains. A chain comes
//! from a line, an arc, a circle, a rectangle or a polyline. Curved vertices of a polyline are exact:
//! fillet vertices and arcs through three points become arcs. Bezier pieces and splines are cut into
//! short lines at `FLATTEN_TOLERANCE`. A chain turns back into a shape with `Chain::to_shape`;
//! an arc inside a polyline then becomes a vertex "point on arc", which is exact too.

use std::f64::consts::{PI, TAU};

use glam::DVec2;

use crate::curve::{self, Seg, Vertex, VertexKind};
use crate::model::{Shape, arc_sweep};

use super::xform::FLATTEN_TOLERANCE;

/// Parameter tolerance for "on the piece".
pub const T_EPS: f64 = 1e-9;

/// A piece of a curve: a line segment or a circular arc.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Piece {
    Line {
        a: DVec2,
        b: DVec2,
    },
    /// `sweep` is signed: positive is counterclockwise. Its size is above 0 and at most a turn.
    Arc {
        c: DVec2,
        r: f64,
        start: f64,
        sweep: f64,
    },
}

fn at_angle(c: DVec2, r: f64, a: f64) -> DVec2 {
    c + DVec2::from_angle(a) * r
}

impl Piece {
    pub fn line(a: DVec2, b: DVec2) -> Piece {
        Piece::Line { a, b }
    }

    /// The arc about `c` from `from` to `to`, counterclockwise if `ccw`.
    pub fn arc_between(c: DVec2, r: f64, from: DVec2, to: DVec2, ccw: bool) -> Piece {
        let a0 = (from - c).to_angle();
        let a1 = (to - c).to_angle();
        let sweep = if ccw {
            (a1 - a0).rem_euclid(TAU)
        } else {
            -((a0 - a1).rem_euclid(TAU))
        };
        Piece::Arc {
            c,
            r,
            start: a0,
            sweep,
        }
    }

    pub fn from(&self) -> DVec2 {
        match *self {
            Piece::Line { a, .. } => a,
            Piece::Arc { c, r, start, .. } => at_angle(c, r, start),
        }
    }

    pub fn to(&self) -> DVec2 {
        match *self {
            Piece::Line { b, .. } => b,
            Piece::Arc { c, r, start, sweep } => at_angle(c, r, start + sweep),
        }
    }

    pub fn length(&self) -> f64 {
        match *self {
            Piece::Line { a, b } => a.distance(b),
            Piece::Arc { r, sweep, .. } => r * sweep.abs(),
        }
    }

    /// The point at parameter `t`: 0 is the start and 1 the end. Values outside go along the
    /// infinite line or the circle.
    pub fn point(&self, t: f64) -> DVec2 {
        match *self {
            Piece::Line { a, b } => a.lerp(b, t),
            Piece::Arc { c, r, start, sweep } => at_angle(c, r, start + sweep * t),
        }
    }

    /// Direction of travel at parameter `t` (unit length).
    pub fn tangent(&self, t: f64) -> DVec2 {
        match *self {
            Piece::Line { a, b } => (b - a).normalize_or_zero(),
            Piece::Arc { start, sweep, .. } => {
                let a = start + sweep * t;
                DVec2::new(-a.sin(), a.cos()) * sweep.signum()
            }
        }
    }

    pub fn reversed(&self) -> Piece {
        match *self {
            Piece::Line { a, b } => Piece::Line { a: b, b: a },
            Piece::Arc { c, r, start, sweep } => Piece::Arc {
                c,
                r,
                start: start + sweep,
                sweep: -sweep,
            },
        }
    }

    /// The part between the parameters `t0` and `t1` (`t0 < t1`; values outside 0..1 extend).
    pub fn sub(&self, t0: f64, t1: f64) -> Piece {
        match *self {
            Piece::Line { a, b } => Piece::Line {
                a: a.lerp(b, t0),
                b: a.lerp(b, t1),
            },
            Piece::Arc { c, r, start, sweep } => Piece::Arc {
                c,
                r,
                start: start + sweep * t0,
                sweep: sweep * (t1 - t0),
            },
        }
    }

    /// The parameter of the point on the infinite line or the circle that is nearest to `p`.
    /// Off an arc, the value is below 0 or above 1, on the side of the nearer end.
    pub fn param_of(&self, p: DVec2) -> f64 {
        match *self {
            Piece::Line { a, b } => {
                let d = b - a;
                let l2 = d.length_squared();
                if l2 == 0.0 { 0.0 } else { (p - a).dot(d) / l2 }
            }
            Piece::Arc {
                c, start, sweep, ..
            } => {
                let w = sweep.abs();
                let th = (p - c).to_angle();
                let u = ((th - start) * sweep.signum()).rem_euclid(TAU);
                let gap = TAU - w;
                let u = if u <= w + gap / 2.0 { u } else { u - TAU };
                u / w
            }
        }
    }

    /// Nearest point of the piece to `p`: (parameter in 0..=1, point, distance).
    pub fn closest(&self, p: DVec2) -> (f64, DVec2, f64) {
        let t = self.param_of(p).clamp(0.0, 1.0);
        let q = self.point(t);
        (t, q, q.distance(p))
    }

    /// True if the point of the infinite curve with parameter `t` is on the piece.
    pub fn has_param(t: f64) -> bool {
        (-T_EPS..=1.0 + T_EPS).contains(&t)
    }

    /// The piece moved sideways by `d`: to the left of the travel direction if `d > 0`.
    /// `None` if an arc would shrink to nothing.
    pub fn offset(&self, d: f64) -> Option<Piece> {
        match *self {
            Piece::Line { a, b } => {
                let n = (b - a).try_normalize()?;
                let n = DVec2::new(-n.y, n.x) * d;
                Some(Piece::Line { a: a + n, b: b + n })
            }
            Piece::Arc { c, r, start, sweep } => {
                // The left of a counterclockwise arc is towards the center.
                let r2 = if sweep > 0.0 { r - d } else { r + d };
                (r2 > 1e-12).then_some(Piece::Arc {
                    c,
                    r: r2,
                    start,
                    sweep,
                })
            }
        }
    }

    /// Points along the piece with at most `tol` between curve and chord, from the start to the
    /// end. Both ends are in the list.
    pub fn flatten(&self, tol: f64) -> Vec<DVec2> {
        match *self {
            Piece::Line { a, b } => vec![a, b],
            Piece::Arc { c, r, start, sweep } => {
                let k = arc_steps(r, sweep.abs(), tol);
                (0..=k)
                    .map(|i| at_angle(c, r, start + sweep * i as f64 / k as f64))
                    .collect()
            }
        }
    }

    /// The center and radius if the piece is an arc.
    pub fn circle(&self) -> Option<(DVec2, f64)> {
        match *self {
            Piece::Arc { c, r, .. } => Some((c, r)),
            Piece::Line { .. } => None,
        }
    }
}

/// How many chords cover an arc so that no chord is further than `tol` from the curve.
pub fn arc_steps(r: f64, sweep: f64, tol: f64) -> usize {
    let tol = if tol.is_finite() && tol > 0.0 {
        tol
    } else {
        0.01
    };
    let step = if tol >= r {
        PI
    } else {
        2.0 * (1.0 - tol / r).acos()
    };
    ((sweep / step.max(1e-6)).ceil() as usize).clamp(1, 20_000)
}

/// Points where the infinite curves of two pieces cross. The points may be off the pieces.
/// Lines that run in parallel and circles with the same center give no points.
pub fn curve_intersections(p: &Piece, q: &Piece) -> Vec<DVec2> {
    match (*p, *q) {
        (Piece::Line { a: a1, b: b1 }, Piece::Line { a: a2, b: b2 }) => {
            let (d1, d2) = (b1 - a1, b2 - a2);
            let den = d1.perp_dot(d2);
            if den.abs() <= 1e-12 * d1.length() * d2.length() {
                return Vec::new();
            }
            let t = (a2 - a1).perp_dot(d2) / den;
            vec![a1 + d1 * t]
        }
        (Piece::Line { a, b }, Piece::Arc { c, r, .. })
        | (Piece::Arc { c, r, .. }, Piece::Line { a, b }) => line_circle(a, b, c, r),
        (Piece::Arc { c: c1, r: r1, .. }, Piece::Arc { c: c2, r: r2, .. }) => {
            circle_circle(c1, r1, c2, r2)
        }
    }
}

fn line_circle(a: DVec2, b: DVec2, c: DVec2, r: f64) -> Vec<DVec2> {
    let d = b - a;
    let len = d.length();
    if len == 0.0 {
        return Vec::new();
    }
    let u = d / len;
    let foot = a + u * (c - a).dot(u);
    let h2 = r * r - foot.distance_squared(c);
    let tol = 1e-12 * r * r;
    if h2 < -tol {
        return Vec::new();
    }
    if h2 <= tol {
        return vec![foot];
    }
    let h = h2.sqrt();
    vec![foot - u * h, foot + u * h]
}

fn circle_circle(c1: DVec2, r1: f64, c2: DVec2, r2: f64) -> Vec<DVec2> {
    let d = c1.distance(c2);
    if d <= 1e-12 * (r1 + r2) {
        return Vec::new();
    }
    let x = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
    let h2 = r1 * r1 - x * x;
    let tol = 1e-12 * (r1 * r1 + r2 * r2);
    if h2 < -tol {
        return Vec::new();
    }
    let u = (c2 - c1) / d;
    let base = c1 + u * x;
    if h2 <= tol {
        return vec![base];
    }
    let h = h2.sqrt();
    let n = DVec2::new(-u.y, u.x);
    vec![base + n * h, base - n * h]
}

/// Points where two pieces cross, each with its parameter on `p` and on `q`.
pub fn piece_intersections(p: &Piece, q: &Piece) -> Vec<(f64, f64, DVec2)> {
    curve_intersections(p, q)
        .into_iter()
        .filter_map(|x| {
            let (tp, tq) = (p.param_of(x), q.param_of(x));
            (Piece::has_param(tp) && Piece::has_param(tq))
                .then(|| (tp.clamp(0.0, 1.0), tq.clamp(0.0, 1.0), x))
        })
        .collect()
}

/// A path of pieces. Each piece starts where the last one ended. A closed chain ends where it
/// starts.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub pieces: Vec<Piece>,
    pub closed: bool,
}

/// The end of a chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Start,
    End,
}

impl Chain {
    /// The chain of a shape, or `None` for shapes without a plain outline.
    pub fn from_shape(shape: &Shape) -> Option<Chain> {
        match shape {
            Shape::Line { a, b } => (a != b).then(|| Chain {
                pieces: vec![Piece::Line { a: *a, b: *b }],
                closed: false,
            }),
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let sweep = arc_sweep(*start, *end);
                if sweep <= 0.0 {
                    return None;
                }
                if sweep >= TAU {
                    return Chain::circle(*center, *radius);
                }
                Some(Chain {
                    pieces: vec![Piece::Arc {
                        c: *center,
                        r: *radius,
                        start: *start,
                        sweep,
                    }],
                    closed: false,
                })
            }
            Shape::Circle { center, radius } => Chain::circle(*center, *radius),
            Shape::Rect { min, max } => {
                let (lo, hi) = (min.min(*max), min.max(*max));
                if lo.x == hi.x || lo.y == hi.y {
                    return None;
                }
                let c = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
                Some(Chain {
                    pieces: (0..4)
                        .map(|i| Piece::Line {
                            a: c[i],
                            b: c[(i + 1) % 4],
                        })
                        .collect(),
                    closed: true,
                })
            }
            Shape::Polyline { points, closed } => Chain::from_vertices(points, *closed),
            _ => None,
        }
    }

    /// A circle as a closed chain of two half arcs, counterclockwise from angle 0.
    pub fn circle(c: DVec2, r: f64) -> Option<Chain> {
        (r > 0.0).then(|| Chain {
            pieces: vec![
                Piece::Arc {
                    c,
                    r,
                    start: 0.0,
                    sweep: PI,
                },
                Piece::Arc {
                    c,
                    r,
                    start: PI,
                    sweep: PI,
                },
            ],
            closed: true,
        })
    }

    pub fn from_vertices(v: &[Vertex], closed: bool) -> Option<Chain> {
        if v.len() < 2 {
            return None;
        }
        let path = curve::path(v, closed);
        let mut pieces = Vec::new();
        for seg in &path.segs {
            match *seg {
                Seg::Line { from, to } => {
                    if from != to {
                        pieces.push(Piece::Line { a: from, b: to });
                    }
                }
                Seg::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                    ..
                } => {
                    if sweep != 0.0 && radius > 0.0 {
                        pieces.push(Piece::Arc {
                            c: center,
                            r: radius,
                            start,
                            sweep,
                        });
                    }
                }
                Seg::Quad { .. } | Seg::Cubic { .. } => {
                    let one = Path1(*seg).flatten(FLATTEN_TOLERANCE);
                    for w in one.windows(2) {
                        if w[0] != w[1] {
                            pieces.push(Piece::Line { a: w[0], b: w[1] });
                        }
                    }
                }
            }
        }
        let mut chain = Chain {
            pieces,
            closed: path.closed,
        };
        if chain.closed {
            chain.close_up();
        }
        let chain = chain.merged();
        (!chain.pieces.is_empty()).then_some(chain)
    }

    /// Makes sure that a closed chain's last piece ends at the start, adding a line if not.
    fn close_up(&mut self) {
        if let (Some(f), Some(l)) = (self.pieces.first(), self.pieces.last()) {
            let (s, e) = (f.from(), l.to());
            if s.distance(e) > 1e-9 * (1.0 + s.length()) {
                self.pieces.push(Piece::Line { a: e, b: s });
            }
        }
    }

    pub fn start(&self) -> DVec2 {
        self.pieces[0].from()
    }

    pub fn end(&self) -> DVec2 {
        self.pieces[self.pieces.len() - 1].to()
    }

    pub fn length(&self) -> f64 {
        self.pieces.iter().map(Piece::length).sum()
    }

    /// The vertices that describe this chain. An arc gets a vertex "point on arc" in its middle.
    pub fn to_vertices(&self) -> Vec<Vertex> {
        let mut out = Vec::new();
        for p in &self.pieces {
            out.push(Vertex::corner(p.from()));
            if matches!(p, Piece::Arc { .. }) {
                out.push(Vertex::new(p.point(0.5), VertexKind::PointOnArc));
            }
        }
        if !self.closed {
            out.push(Vertex::corner(self.end()));
        }
        out
    }

    /// The shape of the chain. One line is a line, one open arc is an arc. Everything else is a
    /// polyline. A circle made of arcs stays a polyline with arc vertices.
    pub fn to_shape(&self) -> Shape {
        let me = self.merged();
        let this = &me;
        if this.closed
            && this.pieces.len() == 1
            && let Piece::Arc { c, r, sweep, .. } = this.pieces[0]
            && (sweep.abs() - TAU).abs() < 1e-9
        {
            return Shape::Circle {
                center: c,
                radius: r,
            };
        }
        if !this.closed && this.pieces.len() == 1 {
            match this.pieces[0] {
                Piece::Line { a, b } => return Shape::Line { a, b },
                Piece::Arc { c, r, start, sweep } => {
                    return if sweep > 0.0 {
                        Shape::Arc {
                            center: c,
                            radius: r,
                            start,
                            end: start + sweep,
                        }
                    } else {
                        Shape::Arc {
                            center: c,
                            radius: r,
                            start: start + sweep,
                            end: start,
                        }
                    };
                }
            }
        }
        Shape::Polyline {
            points: this.to_vertices(),
            closed: this.closed,
        }
    }

    /// Where the point on the chain nearest to `p` is: (piece index, parameter, point, distance).
    pub fn closest(&self, p: DVec2) -> (usize, f64, DVec2, f64) {
        let mut best = (0, 0.0, self.start(), f64::INFINITY);
        for (i, piece) in self.pieces.iter().enumerate() {
            let (t, q, d) = piece.closest(p);
            if d < best.3 {
                best = (i, t, q, d);
            }
        }
        best
    }

    /// The pieces from position `s0` to `s1`. A position is a piece index plus a parameter, and
    /// `s0 <= s1`. Empty pieces are left out.
    pub fn between(&self, s0: f64, s1: f64) -> Vec<Piece> {
        let n = self.pieces.len();
        let mut out = Vec::new();
        for (i, p) in self.pieces.iter().enumerate() {
            let (lo, hi) = (i as f64, (i + 1) as f64);
            let a = s0.max(lo);
            let b = s1.min(hi);
            if b - a > T_EPS && i < n {
                let piece = p.sub(a - lo, b - lo);
                if piece.length() > 0.0 {
                    out.push(piece);
                }
            }
        }
        out
    }

    /// The chain with neighboring arcs of one circle and one direction joined into one arc.
    pub fn merged(&self) -> Chain {
        let mut out: Vec<Piece> = Vec::with_capacity(self.pieces.len());
        for p in &self.pieces {
            if let (
                Some(Piece::Arc { c, r, start, sweep }),
                Piece::Arc {
                    c: c2,
                    r: r2,
                    sweep: s2,
                    ..
                },
            ) = (out.last().copied(), *p)
            {
                let tol = 1e-9 * (1.0 + r);
                if c.distance(c2) <= tol
                    && (r - r2).abs() <= tol
                    && sweep.signum() == s2.signum()
                    && sweep.abs() + s2.abs() <= TAU + 1e-9
                    && (Piece::Arc { c, r, start, sweep }).to().distance(p.from()) <= tol
                {
                    let n = out.len();
                    out[n - 1] = Piece::Arc {
                        c,
                        r,
                        start,
                        sweep: sweep + s2,
                    };
                    continue;
                }
            }
            out.push(*p);
        }
        // A closed chain may join its last and first arc too.
        let mut chain = Chain {
            pieces: out,
            closed: self.closed,
        };
        if chain.closed && chain.pieces.len() > 1 {
            let last = *chain.pieces.last().expect("not empty");
            let first = chain.pieces[0];
            if let (
                Piece::Arc { c, r, start, sweep },
                Piece::Arc {
                    c: c2,
                    r: r2,
                    sweep: s2,
                    ..
                },
            ) = (last, first)
            {
                let tol = 1e-9 * (1.0 + r);
                if c.distance(c2) <= tol
                    && (r - r2).abs() <= tol
                    && sweep.signum() == s2.signum()
                    && sweep.abs() + s2.abs() <= TAU + 1e-9
                {
                    chain.pieces.pop();
                    chain.pieces[0] = Piece::Arc {
                        c,
                        r,
                        start,
                        sweep: sweep + s2,
                    };
                }
            }
        }
        chain
    }

    /// Chain made from pieces. Open.
    pub fn open(pieces: Vec<Piece>) -> Chain {
        Chain {
            pieces,
            closed: false,
        }
    }
}

/// One segment of `curve::Seg` flattened to points.
struct Path1(Seg);

impl Path1 {
    fn flatten(&self, tol: f64) -> Vec<DVec2> {
        let path = curve::Path {
            start: self.0.from(),
            segs: vec![self.0],
            closed: false,
        };
        curve::flatten_path(&path, tol)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn line_parameters_and_closest() {
        let l = Piece::line(v(0.0, 0.0), v(10.0, 0.0));
        assert_eq!(l.length(), 10.0);
        assert_eq!(l.param_of(v(5.0, 3.0)), 0.5);
        assert_eq!(l.param_of(v(15.0, 3.0)), 1.5);
        let (t, q, d) = l.closest(v(15.0, 3.0));
        assert_eq!((t, q), (1.0, v(10.0, 0.0)));
        assert!((d - 34f64.sqrt()).abs() < 1e-12);
        assert_eq!(l.reversed().from(), v(10.0, 0.0));
    }

    #[test]
    fn arc_parameters_follow_the_sweep() {
        let a = Piece::Arc {
            c: v(0.0, 0.0),
            r: 2.0,
            start: 0.0,
            sweep: FRAC_PI_2,
        };
        assert!(a.from().distance(v(2.0, 0.0)) < 1e-12);
        assert!(a.to().distance(v(0.0, 2.0)) < 1e-12);
        assert!((a.param_of(v(1.0, 1.0)) - 0.5).abs() < 1e-12);
        // Just past each end the parameter leaves 0..1 on the right side.
        assert!(a.param_of(v(2.0, -0.1)) < 0.0);
        assert!(a.param_of(v(-0.1, 2.0)) > 1.0);
        // A clockwise arc has the same circle and the opposite direction.
        let r = a.reversed();
        assert!(r.from().distance(a.to()) < 1e-12);
        assert!((r.param_of(v(1.0, 1.0)) - 0.5).abs() < 1e-12);
        assert!(r.tangent(0.0).dot(v(1.0, 0.0)) > 0.99);
    }

    #[test]
    fn intersections_of_lines_and_circles() {
        let l1 = Piece::line(v(0.0, 0.0), v(4.0, 4.0));
        let l2 = Piece::line(v(0.0, 4.0), v(4.0, 0.0));
        let x = piece_intersections(&l1, &l2);
        assert_eq!(x.len(), 1);
        assert!(x[0].2.distance(v(2.0, 2.0)) < 1e-12);
        assert!(curve_intersections(&l1, &Piece::line(v(0.0, 1.0), v(4.0, 5.0))).is_empty());
        let circ = Piece::Arc {
            c: v(0.0, 0.0),
            r: 5.0,
            start: 0.0,
            sweep: TAU,
        };
        let line = Piece::line(v(-10.0, 0.0), v(10.0, 0.0));
        assert_eq!(piece_intersections(&line, &circ).len(), 2);
        let tangent = Piece::line(v(-10.0, 5.0), v(10.0, 5.0));
        assert_eq!(curve_intersections(&tangent, &circ).len(), 1);
        let c2 = Piece::Arc {
            c: v(6.0, 0.0),
            r: 5.0,
            start: 0.0,
            sweep: TAU,
        };
        let pts = curve_intersections(&circ, &c2);
        assert_eq!(pts.len(), 2);
        assert!(pts.iter().all(|p| (p.x - 3.0).abs() < 1e-12));
        let far = Piece::Arc {
            c: v(100.0, 0.0),
            r: 1.0,
            start: 0.0,
            sweep: TAU,
        };
        assert!(curve_intersections(&circ, &far).is_empty());
        assert!(curve_intersections(&circ, &circ).is_empty());
    }

    #[test]
    fn offsets_of_lines_and_arcs() {
        let l = Piece::line(v(0.0, 0.0), v(10.0, 0.0));
        assert_eq!(
            l.offset(2.0).unwrap(),
            Piece::line(v(0.0, 2.0), v(10.0, 2.0))
        );
        let a = Piece::Arc {
            c: v(0.0, 0.0),
            r: 5.0,
            start: 0.0,
            sweep: 1.0,
        };
        let Piece::Arc { r, .. } = a.offset(2.0).unwrap() else {
            panic!()
        };
        assert_eq!(r, 3.0);
        assert!(a.offset(5.0).is_none());
        let Piece::Arc { r, .. } = a.reversed().offset(2.0).unwrap() else {
            panic!()
        };
        assert_eq!(r, 7.0);
    }

    #[test]
    fn chains_of_shapes_and_back() {
        let rect = Shape::Rect {
            min: v(0.0, 0.0),
            max: v(4.0, 3.0),
        };
        let c = Chain::from_shape(&rect).unwrap();
        assert!(c.closed);
        assert!((c.length() - 14.0).abs() < 1e-12);
        let circle = Chain::from_shape(&Shape::Circle {
            center: v(1.0, 1.0),
            radius: 2.0,
        })
        .unwrap();
        assert!((circle.length() - TAU * 2.0).abs() < 1e-9);
        assert!(
            Chain::from_shape(&Shape::Line {
                a: v(1.0, 1.0),
                b: v(1.0, 1.0)
            })
            .is_none()
        );
        // A rounded polyline has arcs.
        let poly = Shape::Polyline {
            points: vec![
                Vertex::corner(v(0.0, 0.0)),
                Vertex::new(v(10.0, 0.0), VertexKind::Arc { radius: 2.0 }),
                Vertex::corner(v(10.0, 10.0)),
            ],
            closed: false,
        };
        let c = Chain::from_shape(&poly).unwrap();
        assert_eq!(c.pieces.len(), 3);
        assert!(matches!(c.pieces[1], Piece::Arc { .. }));
        // Back to a polyline: the arc keeps its exact shape.
        let back = Chain::from_shape(&c.to_shape()).unwrap();
        assert_eq!(back.pieces.len(), 3);
        assert!((back.length() - c.length()).abs() < 1e-6);
        let Piece::Arc { r, .. } = back.pieces[1] else {
            panic!()
        };
        assert!((r - 2.0).abs() < 1e-9);
    }

    #[test]
    fn single_pieces_become_lines_and_arcs() {
        let c = Chain::open(vec![Piece::line(v(0.0, 0.0), v(1.0, 0.0))]);
        assert!(matches!(c.to_shape(), Shape::Line { .. }));
        let a = Chain::open(vec![Piece::Arc {
            c: v(0.0, 0.0),
            r: 1.0,
            start: 1.0,
            sweep: -0.5,
        }]);
        let Shape::Arc { start, end, .. } = a.to_shape() else {
            panic!()
        };
        assert_eq!((start, end), (0.5, 1.0));
    }

    #[test]
    fn between_cuts_pieces() {
        let c = Chain::from_shape(&Shape::Polyline {
            points: crate::curve::corners(&[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0)]),
            closed: false,
        })
        .unwrap();
        let part = c.between(0.5, 1.5);
        assert_eq!(part.len(), 2);
        assert_eq!(part[0].from(), v(5.0, 0.0));
        assert_eq!(part[1].to(), v(10.0, 5.0));
        assert!(c.between(1.0, 1.0).is_empty());
    }
}
