//! Courses along a path (PAT-08): a row of units along a polyline or a curve, on one side of the
//! line or centered on it. Soldier courses along a kerb and borders round an island are courses.
//!
//! The units are rectangles of `along` (the size in the direction of the path) by `depth` (the
//! size across it), with a joint of `gap` between them, like the units of a bond. This module is
//! pure geometry: it makes the polygons of the units and the area that the course takes. The
//! document side (the region the course is cut to, the field pattern next to it, counts) is in
//! [`super::plan`].
//!
//! **Straight runs.** Between two corners of the path the units sit at a fixed step of
//! `along + gap`, starting where the run starts. The last unit of a run is cut by whatever ends the
//! run.
//!
//! **Curved runs.** The units stay straight. Each unit stands on a chord of the path (its path side
//! is a chord of exactly `along`), and the next unit is moved along the curve until the narrow side
//! of the joint is exactly `gap`. So no two units overlap, and the joints taper: they are wider on
//! the outer side of the bend. [`Course::joints`] reports the narrowest and the widest joint.
//!
//! **Corners** (a vertex of the path where the direction changes by more than half a degree and
//! that is not inside a curve) follow the corner rule:
//!
//! - `miter`: the units of the two runs end at the mitre line, each moved back by half a gap, so
//!   the joint is a gap wide. The units at the mitre are cut.
//! - `butt`: the first run goes on to the end of the corner; the second run starts a gap behind its
//!   side. The units at the end of the second run are cut square to the first.
//! - `corner_unit`: one unit of its own fills the corner (the square of the depth for a right
//!   angle), and both runs stop a gap short of it.
//!
//! A corner that turns by less than [`MIN_RULE_TURN_DEG`] is always a mitre. A corner sharper than
//! [`MAX_TURN_DEG`] is refused: its mitre would be very long.
//!
//! **The area that the course takes.** The patterns next to a course must not overlap it, and must
//! keep a gap from it. [`Course::band`] is the union of the strip along the path (the depth plus a
//! gap on each side) and the units that stick out of it (cut units, units on bends) grown by a gap.
//! The field is cut by this area.

use glam::DVec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use serde::{Deserialize, Serialize};

use crate::curve::{self, Seg, Vertex};

/// Largest turn at a corner, in degrees. A sharper corner is refused.
pub const MAX_TURN_DEG: f64 = 150.0;
/// A turn smaller than this, in degrees, at a vertex where two segments meet is no corner.
pub const SMOOTH_DEG: f64 = 0.5;
/// A corner that turns by less than this, in degrees, is always a mitre: the butt and the corner
/// unit need a real angle (on a gentle bend the second leg would have to start metres later).
pub const MIN_RULE_TURN_DEG: f64 = 30.0;
/// Largest step of the points on a curve, in radians (2 degrees).
const CURVE_STEP: f64 = 0.0349;
/// Chord error of the points on a curve, in mm.
const CURVE_ERR: f64 = 0.02;
/// Most points of a path.
pub const MAX_PATH_POINTS: usize = 200_000;
/// Smallest unit size, in mm.
pub const MIN_SIZE: f64 = 10.0;

/// Which side of the path the course is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// To the left of the direction of the path.
    #[default]
    Left,
    Right,
    /// The path runs along the middle of the course.
    Center,
}

impl Side {
    pub fn id(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
            Side::Center => "center",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        [Side::Left, Side::Right, Side::Center]
            .into_iter()
            .find(|x| x.id().eq_ignore_ascii_case(s.trim()))
    }

    /// The offsets across the path (to the left, in mm) that the units cover.
    pub fn extents(self, depth: f64) -> (f64, f64) {
        match self {
            Side::Left => (0.0, depth),
            Side::Right => (-depth, 0.0),
            Side::Center => (-depth / 2.0, depth / 2.0),
        }
    }
}

/// What happens at a corner of the path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    /// The units of both runs are cut at the mitre line.
    #[default]
    Miter,
    /// The first run goes on to the end of the corner, the second butts against it.
    Butt,
    /// One unit of its own fills the corner.
    CornerUnit,
}

impl Corner {
    pub fn id(self) -> &'static str {
        match self {
            Corner::Miter => "miter",
            Corner::Butt => "butt",
            Corner::CornerUnit => "corner_unit",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        let key = s.trim().replace([' ', '-'], "_");
        [Corner::Miter, Corner::Butt, Corner::CornerUnit]
            .into_iter()
            .find(|x| x.id().eq_ignore_ascii_case(&key))
    }
}

/// The input of a course.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    /// The reference line (curves allowed).
    pub path: Vec<Vertex>,
    pub closed: bool,
    pub side: Side,
    /// Moves the reference line sideways, to the left of its direction (mm).
    pub offset: f64,
    /// The unit size along the path (mm).
    pub along: f64,
    /// The unit size across the path (mm).
    pub depth: f64,
    /// The joint width (mm).
    pub gap: f64,
    pub corner: Corner,
}

impl Spec {
    /// Checks the numbers.
    pub fn validate(&self) -> Result<(), String> {
        let ok = |v: f64| v.is_finite() && (MIN_SIZE..=crate::model::MAX_COORD).contains(&v);
        if !ok(self.along) || !ok(self.depth) {
            return Err(format!(
                "the unit size of a course is at least {MIN_SIZE} mm along and across"
            ));
        }
        if !(self.gap.is_finite() && self.gap >= 0.0 && self.gap <= crate::model::MAX_COORD) {
            return Err("the gap must be a finite number of zero or more".into());
        }
        if !(self.offset.is_finite() && self.offset.abs() <= crate::model::MAX_COORD) {
            return Err("the offset must be a finite number".into());
        }
        if self.path.len() < if self.closed { 3 } else { 2 } {
            return Err(format!(
                "a course needs a path of {} or more points",
                if self.closed { 3 } else { 2 }
            ));
        }
        if self.path.len() > MAX_PATH_POINTS || self.path.iter().any(|v| !v.is_finite()) {
            return Err(format!(
                "the path of a course has at most {MAX_PATH_POINTS} finite points"
            ));
        }
        Ok(())
    }
}

/// One unit of the course (a polygon, counterclockwise and convex).
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub poly: Vec<DVec2>,
    /// The unit that fills a corner (its own unit type).
    pub corner_unit: bool,
    /// The area of the whole unit this piece is cut from, in mm2.
    pub full_area: f64,
    /// True if the piece is a whole unit on a straight run: the strip of the course holds it.
    pub inside_strip: bool,
}

/// The joints between units of a run, in mm.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Joints {
    /// The number of joints measured.
    pub count: usize,
    pub narrowest: f64,
    pub widest: f64,
}

impl Joints {
    fn add(&mut self, w: f64) {
        if self.count == 0 {
            self.narrowest = w;
            self.widest = w;
        } else {
            self.narrowest = self.narrowest.min(w);
            self.widest = self.widest.max(w);
        }
        self.count += 1;
    }
}

/// The result of laying a course.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Course {
    pub pieces: Vec<Piece>,
    /// The area that the course takes from the field: shapes of an outline and holes.
    pub band: Vec<Vec<Vec<DVec2>>>,
    pub joints: Joints,
    /// The corners of the path that the corner rule was applied to.
    pub corners: usize,
    /// The length of the reference line, in mm.
    pub length: f64,
}

#[derive(Clone, Copy, Debug)]
struct Plane {
    p: DVec2,
    n: DVec2,
}

impl Plane {
    fn side(&self, x: DVec2) -> f64 {
        (x - self.p).dot(self.n)
    }
}

fn left(u: DVec2) -> DVec2 {
    DVec2::new(-u.y, u.x)
}

/// The flat path: points and, for each, whether the path is smooth there.
struct Flat {
    pts: Vec<DVec2>,
    smooth: Vec<bool>,
}

fn start_dir(seg: &Seg) -> DVec2 {
    let d = match *seg {
        Seg::Line { from, to } => to - from,
        Seg::Arc { start, sweep, .. } => DVec2::from_angle(start).perp() * sweep.signum(),
        Seg::Quad { from, c, to } => {
            if c != from {
                c - from
            } else {
                to - from
            }
        }
        Seg::Cubic { from, c1, c2, to } => [c1 - from, c2 - from, to - from]
            .into_iter()
            .find(|d| d.length_squared() > 0.0)
            .unwrap_or(DVec2::ZERO),
    };
    d.normalize_or_zero()
}

fn flatten(path: &[Vertex], closed: bool) -> Flat {
    let p = curve::path(path, closed);
    let mut pts = vec![p.start];
    let mut smooth = vec![false];
    let mut prev_end: Option<DVec2> = None;
    for seg in &p.segs {
        // A boundary between two segments is smooth if the direction does not change.
        if let Some(pe) = prev_end {
            let s = start_dir(seg);
            let last = smooth.len() - 1;
            smooth[last] = pe.length_squared() > 0.0
                && s.length_squared() > 0.0
                && pe.angle_to(s).abs() < SMOOTH_DEG.to_radians();
        }
        prev_end = Some(seg.end_direction().normalize_or_zero());
        match *seg {
            Seg::Line { to, .. } => {
                pts.push(to);
                smooth.push(false);
            }
            Seg::Arc {
                to,
                center,
                radius,
                start,
                sweep,
                ..
            } => {
                let step = CURVE_STEP.min(2.0 * (1.0 - (CURVE_ERR / radius).min(1.0)).acos());
                let n = ((sweep.abs() / step.max(1e-4)).ceil() as usize).clamp(1, 100_000);
                for k in 1..n {
                    pts.push(
                        center + DVec2::from_angle(start + sweep * k as f64 / n as f64) * radius,
                    );
                    smooth.push(true);
                }
                pts.push(to);
                smooth.push(false);
            }
            s @ (Seg::Quad { .. } | Seg::Cubic { .. }) => {
                let one = curve::Path {
                    start: s.from(),
                    segs: vec![s],
                    closed: false,
                };
                let f = curve::flatten_path(&one, CURVE_ERR);
                for q in &f[1..f.len() - 1] {
                    pts.push(*q);
                    smooth.push(true);
                }
                pts.push(s.to());
                smooth.push(false);
            }
        }
    }
    if closed && pts.len() > 1 {
        // The closing point repeats the start; its smoothness is the one of the start.
        pts.pop();
        let last_smooth = smooth.pop().unwrap_or(false);
        // The first segment starts here, and the last ends here: compare them.
        if let (Some(first), Some(lastseg)) = (p.segs.first(), p.segs.last()) {
            let a = lastseg.end_direction().normalize_or_zero();
            let b = start_dir(first);
            smooth[0] = a.length_squared() > 0.0
                && b.length_squared() > 0.0
                && a.angle_to(b).abs() < SMOOTH_DEG.to_radians();
        } else {
            smooth[0] = last_smooth;
        }
    }
    // Drop repeated points.
    let mut out = Flat {
        pts: Vec::with_capacity(pts.len()),
        smooth: Vec::with_capacity(pts.len()),
    };
    for (q, s) in pts.into_iter().zip(smooth) {
        if out.pts.last().is_none_or(|l| l.distance(q) > 1e-9) {
            out.pts.push(q);
            out.smooth.push(s);
        } else if let Some(last) = out.smooth.last_mut() {
            *last = *last && s;
        }
    }
    if closed && out.pts.len() > 1 && out.pts[0].distance(out.pts[out.pts.len() - 1]) <= 1e-9 {
        out.pts.pop();
        out.smooth.pop();
    }
    out
}

/// A polyline with its cumulative length, for walking along it.
struct Poly {
    pts: Vec<DVec2>,
    cum: Vec<f64>,
}

impl Poly {
    fn new(pts: Vec<DVec2>) -> Poly {
        let mut cum = Vec::with_capacity(pts.len());
        let mut s = 0.0;
        for (i, p) in pts.iter().enumerate() {
            if i > 0 {
                s += pts[i - 1].distance(*p);
            }
            cum.push(s);
        }
        Poly { pts, cum }
    }

    fn len(&self) -> f64 {
        *self.cum.last().unwrap_or(&0.0)
    }

    /// The index of the segment that holds `s`.
    fn seg(&self, s: f64) -> usize {
        match self.cum.binary_search_by(|c| c.total_cmp(&s)) {
            Ok(i) => i.min(self.pts.len() - 2),
            Err(i) => i.saturating_sub(1).min(self.pts.len() - 2),
        }
    }

    fn at(&self, s: f64) -> DVec2 {
        let s = s.clamp(0.0, self.len());
        let i = self.seg(s);
        let l = self.cum[i + 1] - self.cum[i];
        if l <= 0.0 {
            return self.pts[i];
        }
        self.pts[i].lerp(self.pts[i + 1], (s - self.cum[i]) / l)
    }

    /// The point at distance `a` (straight) from the point at `s`, further along: its position and
    /// its arc length. `None` if the line ends first.
    fn chord_end(&self, s: f64, a: f64) -> Option<(f64, DVec2)> {
        let p0 = self.at(s);
        let i0 = self.seg(s);
        let mut from = p0;
        let mut from_s = s;
        for j in i0 + 1..self.pts.len() {
            let q = self.pts[j];
            if q.distance(p0) >= a - 1e-12 {
                // Solve |from + t (q - from) - p0| = a for t in 0..1.
                let d = q - from;
                let w = from - p0;
                let (qa, qb, qc) = (
                    d.length_squared(),
                    2.0 * w.dot(d),
                    w.length_squared() - a * a,
                );
                let t = if qa == 0.0 {
                    1.0
                } else {
                    let disc = (qb * qb - 4.0 * qa * qc).max(0.0).sqrt();
                    let r1 = (-qb - disc) / (2.0 * qa);
                    let r2 = (-qb + disc) / (2.0 * qa);
                    [r1, r2]
                        .into_iter()
                        .filter(|t| *t >= -1e-9 && *t <= 1.0 + 1e-9)
                        .fold(f64::INFINITY, f64::min)
                };
                let t = if t.is_finite() {
                    t.clamp(0.0, 1.0)
                } else {
                    1.0
                };
                return Some((from_s + t * d.length(), from + d * t));
            }
            from = q;
            from_s = self.cum[j];
        }
        None
    }
}

/// A rectangle standing on a chord of the path.
#[derive(Clone, Copy)]
struct Rect {
    /// The arc length where the chord starts.
    s0: f64,
    p0: DVec2,
    p1: DVec2,
    /// The unit vector along the chord.
    t: DVec2,
    /// The arc length where the chord ends.
    s1: f64,
}

fn rect_at(poly: &Poly, s: f64, a: f64) -> Option<Rect> {
    let p0 = poly.at(s);
    let (s1, p1) = poly.chord_end(s, a)?;
    let t = (p1 - p0).try_normalize()?;
    Some(Rect {
        s0: s,
        p0,
        p1,
        t,
        s1,
    })
}

fn rect_poly(r: &Rect, lo: f64, hi: f64) -> Vec<DVec2> {
    let n = left(r.t);
    vec![r.p0 + n * lo, r.p1 + n * lo, r.p1 + n * hi, r.p0 + n * hi]
}

/// The joint between two rectangles on a bend: the distance along the mean direction between the
/// facing corners on the side at `lo` and on the side at `hi`.
fn joint_widths(prev: &Rect, next: &Rect, lo: f64, hi: f64) -> (f64, f64) {
    let tm = (prev.t + next.t).try_normalize().unwrap_or(prev.t);
    let w = |e: f64| ((next.p0 + left(next.t) * e) - (prev.p1 + left(prev.t) * e)).dot(tm);
    (w(lo), w(hi))
}

/// Clips a convex polygon to the half-plane of `plane` (the side where `side` is not negative).
fn clip_plane(poly: &[DVec2], plane: &Plane) -> Vec<DVec2> {
    let mut out = Vec::with_capacity(poly.len() + 1);
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (da, db) = (plane.side(a), plane.side(b));
        if da >= 0.0 {
            out.push(a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            let t = da / (da - db);
            out.push(a.lerp(b, t));
        }
    }
    out
}

fn area(poly: &[DVec2]) -> f64 {
    crate::geom::signed_area(poly)
}

/// A run of the path between two corners (or the ends), with what ends it.
struct Run {
    pts: Vec<DVec2>,
    start: Option<Plane>,
    end: Option<Plane>,
    /// The arc length from the start of the run where the first unit of the lattice starts.
    anchor: f64,
    closed_loop: bool,
}

fn unit_dir(a: DVec2, b: DVec2) -> DVec2 {
    (b - a).normalize_or_zero()
}

/// The path shifted to the left by `shift`: every vertex moves by the mitre of its two segments.
fn shifted(pts: &[DVec2], closed: bool, shift: f64) -> Vec<DVec2> {
    if shift == 0.0 {
        return pts.to_vec();
    }
    let n = pts.len();
    (0..n)
        .map(|i| {
            let prev = if i > 0 {
                Some(pts[i - 1])
            } else if closed {
                Some(pts[n - 1])
            } else {
                None
            };
            let next = if i + 1 < n {
                Some(pts[i + 1])
            } else if closed {
                Some(pts[0])
            } else {
                None
            };
            let n0 = prev.map(|p| left(unit_dir(p, pts[i])));
            let n1 = next.map(|q| left(unit_dir(pts[i], q)));
            let m = match (n0, n1) {
                (Some(a), Some(b)) => {
                    let s = a + b;
                    let c = 1.0 + a.dot(b);
                    if c > 1e-3 { s / c } else { a }
                }
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => DVec2::ZERO,
            };
            pts[i] + m * shift
        })
        .collect()
}

/// The first point of the course path (on the path itself, not moved by `offset`) and the
/// direction of its first leg. The pattern next to a course can start from them (see
/// [`field_origin`]).
pub fn first_leg(path: &[Vertex], closed: bool) -> Option<(DVec2, DVec2)> {
    let flat = flatten(path, closed);
    let a = *flat.pts.first()?;
    let b = flat.pts.iter().find(|p| p.distance(a) > 1e-9)?;
    Some((a, (*b - a).normalize()))
}

/// Where the pattern of the field must start so that its first row is whole at the edge of the
/// course: the origin and the angle in degrees. `unit_width` is the size of a unit across the
/// rows of the field. The field lies on the far side of the course, one gap from its units.
pub fn field_origin(spec: &Spec, unit_width: f64) -> Option<(DVec2, f64)> {
    let (p0, u) = first_leg(&spec.path, spec.closed)?;
    let n = left(u);
    let (lo, hi) = spec.side.extents(spec.depth);
    let g = spec.gap;
    // The field is on the left of a left or centered course, and on the right of a right course.
    let (e, up) = match spec.side {
        Side::Left | Side::Center => (spec.offset + hi + g, true),
        Side::Right => (spec.offset + lo - g, false),
    };
    let y0 = if up {
        e - g / 2.0
    } else {
        e - unit_width - g / 2.0
    };
    let along = p0.dot(u) - g / 2.0;
    let origin = u * along + n * (p0.dot(n) + y0);
    Some((origin, u.y.atan2(u.x).to_degrees()))
}

/// Lays a course. `max_units` is the most pieces that may be made: a job over it is an error that
/// says how many it would make.
pub fn lay(spec: &Spec, max_units: usize) -> Result<Course, String> {
    spec.validate()?;
    let flat = flatten(&spec.path, spec.closed);
    let n = flat.pts.len();
    if n < 2 || (spec.closed && n < 3) {
        return Err("the path has no length".into());
    }
    let pts = shifted(&flat.pts, spec.closed, spec.offset);
    let step = spec.along + spec.gap;
    let (lo, hi) = spec.side.extents(spec.depth);
    let g = spec.gap;
    let total_len: f64 = (0..if spec.closed { n } else { n - 1 })
        .map(|i| pts[i].distance(pts[(i + 1) % n]))
        .sum();
    if total_len <= 0.0 {
        return Err("the path has no length".into());
    }
    let estimated = (total_len / step).ceil() as u64 + 2;
    if estimated > max_units as u64 {
        return Err(format!(
            "the course makes about {estimated} units, more than the limit of {max_units}. Use a shorter path or larger units"
        ));
    }
    // The corners.
    let dir_in = |i: usize| unit_dir(pts[(i + n - 1) % n], pts[i]);
    let dir_out = |i: usize| unit_dir(pts[i], pts[(i + 1) % n]);
    let mut corners: Vec<usize> = Vec::new();
    let range = if spec.closed { 0..n } else { 1..n - 1 };
    for i in range {
        if flat.smooth[i] {
            continue;
        }
        let (a, b) = (dir_in(i), dir_out(i));
        let turn = a.angle_to(b).abs();
        if turn < SMOOTH_DEG.to_radians() {
            continue;
        }
        if turn.to_degrees() > MAX_TURN_DEG {
            return Err(format!(
                "the path turns by {:.0} degrees at point {} ({:.0}, {:.0}), more than the {MAX_TURN_DEG} degrees that a course corner takes. Split the path there",
                turn.to_degrees(),
                i,
                pts[i].x,
                pts[i].y
            ));
        }
        corners.push(i);
    }
    // The runs between corners.
    let mut runs: Vec<Run> = Vec::new();
    let mut extra: Vec<Piece> = Vec::new();
    let slice = |a: usize, b: usize| -> Vec<DVec2> {
        // From point a to point b along the path, wrapping for a closed path. The same point
        // twice is the whole way round.
        let count = if a == b { n } else { (b + n - a) % n };
        let mut v = vec![pts[a]];
        let mut i = a;
        for _ in 0..count {
            i = (i + 1) % n;
            v.push(pts[i]);
        }
        v
    };
    let bounds: Vec<usize> = if spec.closed {
        corners.clone()
    } else {
        let mut b = vec![0];
        b.extend(&corners);
        b.push(n - 1);
        b
    };
    if spec.closed && corners.is_empty() {
        let mut v = pts.clone();
        v.push(pts[0]);
        let u0 = unit_dir(pts[0], pts[1]);
        runs.push(Run {
            pts: v,
            start: Some(Plane { p: pts[0], n: u0 }),
            end: Some(Plane {
                p: pts[0] - u0 * g,
                n: -u0,
            }),
            anchor: 0.0,
            closed_loop: true,
        });
    } else {
        let count = if spec.closed {
            bounds.len()
        } else {
            bounds.len() - 1
        };
        for k in 0..count {
            let (a, b) = (bounds[k], bounds[(k + 1) % bounds.len()]);
            let v = slice(a, b);
            runs.push(Run {
                pts: v,
                start: None,
                end: None,
                anchor: 0.0,
                closed_loop: false,
            });
        }
        // Ends of an open path are flush.
        if !spec.closed {
            let u = unit_dir(runs[0].pts[0], runs[0].pts[1]);
            runs[0].start = Some(Plane {
                p: runs[0].pts[0],
                n: u,
            });
            let last = runs.len() - 1;
            let m = runs[last].pts.len();
            let u = unit_dir(runs[last].pts[m - 2], runs[last].pts[m - 1]);
            runs[last].end = Some(Plane {
                p: runs[last].pts[m - 1],
                n: -u,
            });
        }
        // The corner rules.
        let nr = runs.len();
        for (ci, &c) in corners.iter().enumerate() {
            // Run before the corner and run after it.
            let (r1, r2) = if spec.closed {
                ((ci + nr - 1) % nr, ci % nr)
            } else {
                (ci, ci + 1)
            };
            let cp = pts[c];
            let (u1, u2) = (dir_in(c), dir_out(c));
            let rule = if u1.angle_to(u2).abs().to_degrees() < MIN_RULE_TURN_DEG {
                Corner::Miter
            } else {
                spec.corner
            };
            match rule {
                Corner::Miter => {
                    let nn = (u1 + u2).try_normalize().unwrap_or(u1);
                    runs[r1].end = Some(Plane {
                        p: cp - nn * (g / 2.0),
                        n: -nn,
                    });
                    let pl = Plane {
                        p: cp + nn * (g / 2.0),
                        n: nn,
                    };
                    runs[r2].anchor = (pl.p - cp).dot(pl.n) / u2.dot(pl.n);
                    runs[r2].start = Some(pl);
                }
                Corner::Butt => {
                    let ts = u1.perp_dot(u2).signum();
                    let (l1, l2) = (left(u1), left(u2));
                    let s_end = [lo, hi].iter().map(|e| e * l2.dot(u1)).fold(0.0, f64::max);
                    runs[r1].end = Some(Plane {
                        p: cp + u1 * s_end,
                        n: -u1,
                    });
                    let t = l1 * ts;
                    let hi_t = [lo, hi].iter().map(|e| e * ts).fold(f64::MIN, f64::max);
                    let pl = Plane {
                        p: cp + t * (hi_t + g),
                        n: t,
                    };
                    runs[r2].anchor = (pl.p - cp).dot(pl.n) / u2.dot(pl.n);
                    runs[r2].start = Some(pl);
                }
                Corner::CornerUnit => {
                    let (l1, l2) = (left(u1), left(u2));
                    let cross = u1.perp_dot(u2);
                    let meet = |e1: f64, e2: f64| -> DVec2 {
                        let w = l2 * e2 - l1 * e1;
                        let s = w.perp_dot(u2) / cross;
                        cp + l1 * e1 + u1 * s
                    };
                    let mut q = vec![meet(lo, lo), meet(hi, lo), meet(hi, hi), meet(lo, hi)];
                    if area(&q) < 0.0 {
                        q.reverse();
                    }
                    let min_u1 = q.iter().map(|v| (*v - cp).dot(u1)).fold(f64::MAX, f64::min);
                    let max_u2 = q.iter().map(|v| (*v - cp).dot(u2)).fold(f64::MIN, f64::max);
                    runs[r1].end = Some(Plane {
                        p: cp + u1 * (min_u1 - g),
                        n: -u1,
                    });
                    let pl = Plane {
                        p: cp + u2 * (max_u2 + g),
                        n: u2,
                    };
                    runs[r2].anchor = (pl.p - cp).dot(pl.n) / u2.dot(pl.n);
                    runs[r2].start = Some(pl);
                    let a = area(&q).abs();
                    extra.push(Piece {
                        poly: q,
                        corner_unit: true,
                        full_area: a,
                        inside_strip: false,
                    });
                }
            }
        }
    }
    let mut out = Course {
        length: total_len,
        corners: corners.len(),
        pieces: extra,
        ..Default::default()
    };
    let mut made = out.pieces.len();
    for run in &runs {
        lay_run(run, spec, (lo, hi), max_units, &mut made, &mut out)?;
    }
    out.band = band(&pts, spec.closed, &flat.smooth, (lo, hi), g, &out.pieces);
    Ok(out)
}

fn lay_run(
    run: &Run,
    spec: &Spec,
    (lo, hi): (f64, f64),
    max_units: usize,
    made: &mut usize,
    out: &mut Course,
) -> Result<(), String> {
    let (a, g) = (spec.along, spec.gap);
    let step = a + g;
    let full = a * spec.depth;
    let m = run.pts.len();
    let u_start = unit_dir(run.pts[0], run.pts[1]);
    let u_end = unit_dir(run.pts[m - 2], run.pts[m - 1]);
    // The line is extended by straight pieces at both ends, so that a mitre that reaches past the
    // end of the run (the outside of a corner) is still covered by units.
    let ext = a + 4.0 * spec.depth + g;
    let mut e = Vec::with_capacity(m + 2);
    e.push(run.pts[0] - u_start * ext);
    e.extend_from_slice(&run.pts);
    e.push(run.pts[m - 1] + u_end * ext);
    let poly = Poly::new(e);
    let straight = m == 2;
    // On a gentle curve the straight units stay inside the strip of the course (a unit of length a
    // on a bend of radius R is off the curve by a^2 / 8R at its middle, and its far corners by about
    // depth * (a / 2R)^2 / 2): such units need no area of their own.
    let gentle = straight || {
        let mut kmax: f64 = 0.0;
        for i in 1..m - 1 {
            let (d0, d1) = (run.pts[i] - run.pts[i - 1], run.pts[i + 1] - run.pts[i]);
            let len = (d0.length() + d1.length()) / 2.0;
            if len > 0.0 {
                kmax = kmax.max(d0.angle_to(d1).abs() / len);
            }
        }
        let sag = kmax * a * a / 8.0 + spec.depth * (kmax * a / 2.0).powi(2) / 2.0;
        sag <= g / 2.0
    };
    let s_anchor = ext + run.anchor;
    let mut starts: Vec<f64> = Vec::new();
    // Back from the anchor on the straight piece before the run.
    let mut k = 1.0;
    while s_anchor - k * step >= -a {
        starts.push(s_anchor - k * step);
        k += 1.0;
    }
    starts.reverse();
    let mut rects: Vec<Rect> = Vec::new();
    let limit_total = poly.len();
    // The length of the run itself, without the pieces that were added at its ends.
    let run_len = poly.len() - 2.0 * ext;
    let past_end = |r: &Rect| -> bool {
        if run.closed_loop {
            // A loop ends where it started: the units stop one gap before the seam.
            return r.s0 > ext + run_len - g;
        }
        run.end.as_ref().is_some_and(|pl| {
            // Every corner of the rectangle is beyond the end plane.
            rect_poly(r, lo, hi).iter().all(|p| pl.side(*p) < -1e-9)
        })
    };
    for s in &starts {
        if *s >= 0.0
            && *s <= limit_total
            && let Some(r) = rect_at(&poly, *s, a)
        {
            rects.push(r);
        }
    }
    // Forward from the anchor.
    let mut s = s_anchor;
    while let Some(r) = rect_at(&poly, s, a) {
        if past_end(&r) {
            break;
        }
        *made += 1;
        if *made > max_units {
            return Err(format!(
                "the course makes more than the limit of {max_units} units. Use a shorter path or larger units"
            ));
        }
        rects.push(r);
        // The next start.
        let next_s = if straight {
            s + step
        } else {
            let f = |sn: f64| -> Option<f64> {
                let nr = rect_at(&poly, sn, a)?;
                let (wl, wh) = joint_widths(&r, &nr, lo, hi);
                Some(wl.min(wh) - g)
            };
            let mut hi_s = r.s1 + g + (hi - lo) + a;
            let mut tries = 0;
            let mut ok = true;
            while f(hi_s).is_some_and(|v| v < 0.0) {
                hi_s += g + (hi - lo) + a;
                tries += 1;
                if tries > 20 {
                    ok = false;
                    break;
                }
            }
            if !ok || f(hi_s).is_none() {
                // The line ends before the next unit fits: stop.
                break;
            }
            let mut lo_s = r.s1;
            for _ in 0..40 {
                let mid = (lo_s + hi_s) / 2.0;
                match f(mid) {
                    Some(v) if v < 0.0 => lo_s = mid,
                    _ => hi_s = mid,
                }
            }
            hi_s
        };
        if next_s <= s + 1e-9 {
            break;
        }
        s = next_s;
    }
    // Clip and keep.
    let mut last_whole: Option<Rect> = None;
    for r in &rects {
        let mut p = rect_poly(r, lo, hi);
        let mut clipped = false;
        // The planes of a loop are both at the seam: the first half of the loop has the start
        // plane, the second half the end plane.
        let planes: [Option<&Plane>; 2] = if run.closed_loop {
            if r.s0 < ext + run_len / 2.0 {
                [run.start.as_ref(), None]
            } else {
                [None, run.end.as_ref()]
            }
        } else {
            [run.start.as_ref(), run.end.as_ref()]
        };
        for pl in planes.into_iter().flatten() {
            if p.is_empty() {
                break;
            }
            let before = p.len();
            let all_in = p.iter().all(|v| pl.side(*v) >= -1e-9);
            if !all_in {
                p = clip_plane(&p, pl);
                clipped = true;
            }
            let _ = before;
        }
        if p.len() < 3 || area(&p) < 1e-6 {
            last_whole = None;
            continue;
        }
        let whole = !clipped && area(&p) >= full * (1.0 - 1e-9);
        out.pieces.push(Piece {
            poly: p,
            corner_unit: false,
            full_area: full,
            inside_strip: whole && gentle,
        });
        if whole {
            if let Some(pr) = &last_whole {
                if straight {
                    out.joints.add(g);
                } else {
                    let (wl, wh) = joint_widths(pr, r, lo, hi);
                    out.joints.add(wl.min(wh));
                    out.joints.add(wl.max(wh));
                }
            }
            last_whole = Some(*r);
        } else {
            last_whole = None;
        }
    }
    Ok(())
}

/// A convex polygon grown by `r` all round (corners are rounded).
fn dilate(poly: &[DVec2], r: f64) -> Vec<DVec2> {
    let n = poly.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (p, q, s) = (poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]);
        // The outward normal of a counterclockwise edge is its right normal.
        let n0 = -left(unit_dir(p, q));
        let n1 = -left(unit_dir(q, s));
        let (a0, a1) = (n0.to_angle(), n1.to_angle());
        let mut sweep = (a1 - a0).rem_euclid(std::f64::consts::TAU);
        if sweep > std::f64::consts::PI {
            sweep -= std::f64::consts::TAU;
        }
        let steps = ((sweep.abs() / 0.26).ceil() as usize).max(1);
        for k in 0..=steps {
            out.push(q + DVec2::from_angle(a0 + sweep * k as f64 / steps as f64) * r);
        }
    }
    out
}

fn ccw(mut v: Vec<DVec2>) -> Vec<DVec2> {
    if area(&v) < 0.0 {
        v.reverse();
    }
    v
}

fn ring(p: &[DVec2]) -> Vec<[f64; 2]> {
    p.iter().map(|q| [q.x, q.y]).collect()
}

/// The area that the course takes: the strip along the path grown by a gap on each side, and the
/// units that stick out of it, grown by a gap.
fn band(
    pts: &[DVec2],
    closed: bool,
    smooth: &[bool],
    (lo, hi): (f64, f64),
    g: f64,
    pieces: &[Piece],
) -> Vec<Vec<Vec<DVec2>>> {
    let n = pts.len();
    let (e_lo, e_hi) = (lo - g, hi + g);
    let mut subject: Vec<Vec<[f64; 2]>> = Vec::new();
    let segs = if closed { n } else { n - 1 };
    let normals: Vec<DVec2> = (0..segs)
        .map(|i| left(unit_dir(pts[i], pts[(i + 1) % n])))
        .collect();
    for i in 0..segs {
        let (mut p, mut q, nn) = (pts[i], pts[(i + 1) % n], normals[i]);
        // The ends of an open path keep a gap from the field, too.
        if !closed {
            if i == 0 {
                p -= unit_dir(p, q) * g;
            }
            if i + 1 == segs {
                q += unit_dir(p, q) * g;
            }
        }
        subject.push(ring(&ccw(vec![
            p + nn * e_lo,
            q + nn * e_lo,
            q + nn * e_hi,
            p + nn * e_hi,
        ])));
    }
    // Fill the wedge on the outer side of every bend: a mitre at a corner, a bevel on a curve.
    let vertices = if closed { 0..n } else { 1..n - 1 };
    for i in vertices {
        let (n0, n1) = (normals[(i + segs - 1) % segs], normals[i % segs]);
        for e in [e_lo, e_hi] {
            if e == 0.0 {
                continue;
            }
            let (a, b) = (pts[i] + n0 * e, pts[i] + n1 * e);
            let dot = n0.dot(n1);
            if !smooth[i] && 1.0 + dot > 0.02 {
                let m = pts[i] + (n0 + n1) / (1.0 + dot) * e;
                subject.push(ring(&ccw(vec![pts[i], a, m, b])));
            } else {
                subject.push(ring(&ccw(vec![pts[i], a, b])));
            }
        }
    }
    for pc in pieces {
        if !pc.inside_strip {
            subject.push(ring(&ccw(dilate(&pc.poly, g.max(0.001)))));
        }
    }
    let empty: Vec<Vec<[f64; 2]>> = Vec::new();
    subject
        .overlay(&empty, OverlayRule::Union, FillRule::NonZero)
        .into_iter()
        .map(|s| {
            s.into_iter()
                .map(|r| r.into_iter().map(|p| DVec2::new(p[0], p[1])).collect())
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests;
