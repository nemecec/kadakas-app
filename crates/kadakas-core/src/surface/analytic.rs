//! Analytic curves (MOD-15): the exact description of a circle, an arc or a regular polygon next
//! to its chain of edges.
//!
//! A curve of a surface model is a chain of straight edges (see `SurfaceModel::curves`). The tools
//! Circle, Arc and Polygon also give the circle that the corners lie on: [`Analytic`] holds the
//! centre, the normal, a reference axis, the radius and, for an open chain, the start and end
//! angle. The number of segments is not stored: it is the number of edges of the chain. The field
//! is optional. A curve without it works as before.
//!
//! **The invariant.** If a curve has the field, all vertices of its chain lie on the circle within
//! the length tolerance. The only exception is a vertex that another edge meets (three or more
//! edges at it) and that lies on the straight piece between the two vertices on the circle at
//! its sides: a chain edge that another edge cut, so the vertex lies on the chord. A vertex with
//! only the two chain edges is never excused, so a fine circle (a thousand segments) cannot
//! follow its points off the circle. The invariant is kept in one place: [`reconcile`] runs after
//! every edit (`edit::run`) and compares each curve with the model before the edit.
//!
//! - Chain vertices moved (more than the tolerance) and the chain is a similar copy of the old
//!   one (same ratios of all lengths, which is move, rotate, mirror and uniform scale): the
//!   circle is fitted again, on the vertices that are on the circle. This is checked first.
//! - Else, if all vertices are on the circle (also when some of them moved along it), the field
//!   stays. The kind and the angles follow the chain: a circle that lost an edge is an arc, and
//!   an arc that lost edges at its ends has new start and end angles (the centre and the radius
//!   stay). A polygon whose corner slid along the circle stays a polygon: the kind says that the
//!   corners are corners, and the sides need not be equal.
//! - Else the field is dropped, with a note in the result.
//!
//! A description with a kind that this version does not know (a file of a newer program) is read
//! only: it stays while its chain is untouched and is dropped, with a note, by any change of the
//! chain. A set of segments is refused for it.
//!
//! New curves that come from a curve with the field (the copy at the top of a Push/Pull, the
//! profile copies of Follow Me, the Offset of a circle) get the field from their own points
//! ([`derive`]). Nothing here moves a vertex.
//!
//! Saved form: the model field `curve_geometry` is a list of `[curve id, {kind, center, normal,
//! axis, radius, start?, end?}]` in id order. An older program keeps the field it does not know.
//! A file without the field loads as before. Entries whose curve is gone or whose points are not on
//! the circle (an older program edited the chain) are dropped on load.

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec3;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::geom;
use super::{CurveId, EdgeId, SurfaceModel, VertexId};

/// The kind of a full circle.
pub const KIND_CIRCLE: &str = "circle";
/// The kind of an open chain on a circle.
pub const KIND_ARC: &str = "arc";
/// The kind of a regular polygon: the corners lie on the circle and stay corners.
pub const KIND_POLYGON: &str = "polygon";

/// True for the kinds that this version edits.
pub fn is_known_kind(kind: &str) -> bool {
    matches!(kind, KIND_CIRCLE | KIND_ARC | KIND_POLYGON)
}

const TAU: f64 = std::f64::consts::TAU;

/// The exact description of a circle, an arc or a polygon (MOD-15). All numbers are in the local
/// coordinates of the model (mm and radians). Angles run counterclockwise about `normal`, from
/// `axis`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Analytic {
    /// `circle`, `arc` or `polygon`. Another word from a newer program is kept.
    pub kind: String,
    /// The centre.
    pub center: [f64; 3],
    /// The unit normal of the plane of the circle.
    pub normal: [f64; 3],
    /// A unit vector in the plane: the direction of the angle 0.
    pub axis: [f64; 3],
    /// The radius of the circle that the vertices lie on.
    pub radius: f64,
    /// The start angle (radians, counterclockwise from `axis`) of an open chain; none for a full
    /// circle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<f64>,
    /// The end angle of an open chain, counterclockwise from `start`; none for a full circle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<f64>,
    /// Fields of a newer program. They are written back.
    #[serde(flatten, default, skip_serializing_if = "Map::is_empty")]
    #[schemars(skip)]
    pub extra: Map<String, Value>,
}

impl Analytic {
    /// A full circle with the given axis for the angle 0.
    pub fn circle(center: DVec3, normal: DVec3, axis: DVec3, radius: f64) -> Analytic {
        Analytic::new(KIND_CIRCLE, center, normal, axis, radius, None)
    }

    /// An arc from the angle `start` to the angle `end` (counterclockwise).
    pub fn arc(
        center: DVec3,
        normal: DVec3,
        axis: DVec3,
        radius: f64,
        start: f64,
        end: f64,
    ) -> Analytic {
        Analytic::new(KIND_ARC, center, normal, axis, radius, Some((start, end)))
    }

    /// A polygon whose corners lie on the circle.
    pub fn polygon(center: DVec3, normal: DVec3, axis: DVec3, radius: f64) -> Analytic {
        Analytic::new(KIND_POLYGON, center, normal, axis, radius, None)
    }

    fn new(
        kind: &str,
        center: DVec3,
        normal: DVec3,
        axis: DVec3,
        radius: f64,
        ends: Option<(f64, f64)>,
    ) -> Analytic {
        Analytic {
            kind: kind.into(),
            center: center.to_array(),
            normal: normal.to_array(),
            axis: axis.to_array(),
            radius,
            start: ends.map(|e| e.0),
            end: ends.map(|e| e.1),
            extra: Map::new(),
        }
    }

    /// The centre.
    pub fn center(&self) -> DVec3 {
        DVec3::from(self.center)
    }

    /// The normal of the plane.
    pub fn normal(&self) -> DVec3 {
        DVec3::from(self.normal)
    }

    /// The direction of the angle 0 made exactly perpendicular to the normal and of unit length.
    pub fn axis(&self) -> DVec3 {
        let n = self.normal().normalize_or_zero();
        let a = DVec3::from(self.axis);
        (a - n * a.dot(n)).normalize_or_zero()
    }

    /// The direction of the angle 90 degrees.
    fn side(&self) -> DVec3 {
        self.normal().normalize_or_zero().cross(self.axis())
    }

    /// True if the numbers make a circle: finite, a radius above 0, a normal and an axis that are
    /// not zero or parallel, and no or both angles.
    pub fn is_valid(&self) -> bool {
        let finite = |v: &[f64; 3]| v.iter().all(|x| x.is_finite());
        finite(&self.center)
            && finite(&self.normal)
            && finite(&self.axis)
            && self.radius.is_finite()
            && self.radius > 0.0
            && self.normal().length() > 0.0
            && self.axis().length() > 0.5
            && self.start.is_some() == self.end.is_some()
            && self.start.is_none_or(f64::is_finite)
            && self.end.is_none_or(f64::is_finite)
    }

    /// The unit normal and axis, so that stored vectors are exact.
    fn normalised(mut self) -> Analytic {
        self.normal = self.normal().normalize_or_zero().to_array();
        self.axis = self.axis().to_array();
        self
    }

    /// The point of the circle at an angle (radians).
    pub fn point_at(&self, angle: f64) -> DVec3 {
        self.center() + self.radius * (self.axis() * angle.cos() + self.side() * angle.sin())
    }

    /// The angle of the direction from the centre to `p`, in `[0, 2 pi)`.
    pub fn angle_of(&self, p: DVec3) -> f64 {
        let d = p - self.center();
        let a = d.dot(self.side()).atan2(d.dot(self.axis()));
        if a < 0.0 { a + TAU } else { a }
    }

    /// How far `p` is from the circle (a point out of the plane counts with its height).
    pub fn deviation(&self, p: DVec3) -> f64 {
        let d = p - self.center();
        let n = self.normal().normalize_or_zero();
        let h = d.dot(n);
        let radial = (d - n * h).length();
        (radial - self.radius).hypot(h)
    }

    /// The angle of an open chain from its start to its end, in `(0, 2 pi]`; none for a full
    /// circle.
    pub fn sweep(&self) -> Option<f64> {
        let (s, e) = (self.start?, self.end?);
        let w = (e - s).rem_euclid(TAU);
        Some(if w == 0.0 { TAU } else { w })
    }

    /// True if this is a full circle or a closed polygon (no angles).
    pub fn is_full(&self) -> bool {
        self.start.is_none()
    }

    /// The circumference of the circle.
    pub fn circumference(&self) -> f64 {
        self.radius * TAU
    }
}

/// Why a curve edit was refused (MOD-15). Each reason has a stable code for the API.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum CurveError {
    /// The curve does not exist.
    #[error("curve {0} does not exist")]
    NoCurve(CurveId),
    /// The curve has no exact description (an old curve, or one that an edit made inexact).
    #[error("curve {0} has no circle description")]
    NoDescription(CurveId),
    /// The number of segments is out of range.
    #[error("the number of segments must be from {min} to {max}, not {segments}")]
    Segments { segments: u32, min: u32, max: u32 },
    /// A vertex inside the chain has other edges than the two of the chain.
    #[error("vertex {vertex} of the curve has other edges, so the chain cannot be rebuilt")]
    VertexShared { vertex: VertexId },
    /// A face that uses the chain is not in the plane of the circle.
    #[error("face {face} uses the curve and is not in the plane of the circle")]
    BordersOtherPlane { face: super::FaceId },
    /// A loop of a face runs along the chain in a way that cannot be rebuilt.
    #[error("face {face} uses the chain in a way that cannot be rebuilt")]
    LoopUnclear { face: super::FaceId },
    /// The result would be invalid (too short edges, crossings and the like).
    #[error("the new chain would make the model invalid: {reason}")]
    Invalid { reason: String },
    /// The description has a kind that this version does not know, so it is read only.
    #[error("curve {0} has a kind of circle that this program does not know and does not change")]
    UnknownKind(CurveId),
}

impl CurveError {
    /// The stable code of the reason.
    pub fn code(&self) -> &'static str {
        match self {
            CurveError::NoCurve(_) => "no_such_curve",
            CurveError::NoDescription(_) => "curve_without_description",
            CurveError::Segments { .. } => "segments_out_of_range",
            CurveError::VertexShared { .. } => "curve_vertex_shared",
            CurveError::BordersOtherPlane { .. } => "curve_borders_other_plane",
            CurveError::LoopUnclear { .. } => "curve_loop_unclear",
            CurveError::Invalid { .. } => "curve_result_invalid",
            CurveError::UnknownKind(_) => "curve_unknown_kind",
        }
    }
}

/// The vertices of a chain in order. A closed chain ends with its first vertex. `None` if the
/// edges are not a chain.
pub(super) fn chain_vertices(m: &SurfaceModel, edges: &[EdgeId]) -> Option<Vec<VertexId>> {
    let first = m.edges.get(edges.first()?)?;
    let start = match edges.get(1) {
        None => first.v[0],
        Some(next) => {
            let next = m.edges.get(next)?;
            let a = next.v.contains(&first.v[0]);
            let b = next.v.contains(&first.v[1]);
            match (a, b) {
                (true, false) => first.v[1],
                (false, true) => first.v[0],
                // A closed chain of two edges, or no joint.
                _ => return None,
            }
        }
    };
    let mut out = vec![start];
    let mut cur = start;
    for e in edges {
        cur = m.edges.get(e)?.other(cur)?;
        out.push(cur);
    }
    Some(out)
}

fn positions(m: &SurfaceModel, vs: &[VertexId]) -> Option<Vec<DVec3>> {
    vs.iter().map(|v| m.vertices.get(v).copied()).collect()
}

fn is_closed(vs: &[VertexId]) -> bool {
    vs.len() >= 4 && vs.first() == vs.last()
}

/// The tolerance for the points of a curve and its circle.
fn tolerance(a: &Analytic, pts: &[DVec3]) -> f64 {
    let s = pts.iter().map(|p| geom::size(*p)).fold(0.0, f64::max);
    geom::tolerance_at(s.max(geom::size(a.center())))
}

/// The number of edges at each of the vertices `of`.
pub(super) fn degrees_of(m: &SurfaceModel, of: &BTreeSet<VertexId>) -> BTreeMap<VertexId, u32> {
    let mut out: BTreeMap<VertexId, u32> = of.iter().map(|v| (*v, 0)).collect();
    for e in m.edges.values() {
        for v in e.v {
            if let Some(d) = out.get_mut(&v) {
                *d += 1;
            }
        }
    }
    out
}

/// For each vertex of a chain: another edge meets it (three or more edges). Only such a vertex
/// may lie off the circle, on a chord.
fn shared_flags(vs: &[VertexId], deg: &BTreeMap<VertexId, u32>) -> Vec<bool> {
    vs.iter()
        .map(|v| deg.get(v).copied().unwrap_or(0) >= 3)
        .collect()
}

/// The rule of the module: every vertex is on the circle, except a run of vertices that other
/// edges meet (`shared`) between two vertices on the circle, on the straight piece between them.
/// `pts` of a closed chain ends with its first point.
pub(super) fn holds(a: &Analytic, pts: &[DVec3], shared: &[bool]) -> bool {
    let n = pts.len();
    if n == 0 {
        return false;
    }
    let tol = tolerance(a, pts);
    if !goes_one_way(a, pts) {
        return false;
    }
    let closed = n >= 4 && pts[0].distance(pts[n - 1]) <= tol;
    let last = if closed { n - 1 } else { n };
    let on: Vec<bool> = (0..last).map(|i| a.deviation(pts[i]) <= tol).collect();
    if on.iter().all(|b| *b) {
        return true;
    }
    // The order of the scan starts at a vertex on the circle (a closed chain may start on a
    // chord vertex).
    let Some(s) = on.iter().position(|b| *b) else {
        return false;
    };
    let order: Vec<usize> = if closed {
        (0..last).map(|k| (s + k) % last).collect()
    } else if s == 0 && on[last - 1] {
        (0..last).collect()
    } else {
        return false;
    };
    let mut p = 0;
    while p < order.len() {
        if on[order[p]] {
            p += 1;
            continue;
        }
        let mut q = p;
        while q < order.len() && !on[order[q]] {
            q += 1;
        }
        let prev = pts[order[p - 1]];
        let next = match order.get(q) {
            Some(i) => pts[*i],
            None => pts[order[0]],
        };
        for &i in &order[p..q] {
            if !shared.get(i).copied().unwrap_or(false)
                || geom::point_segment(pts[i], prev, next).0 > tol
            {
                return false;
            }
        }
        p = q;
    }
    true
}

/// The centre and the radius of the least squares circle of the points in the plane through `c0`
/// with the normal `n` (Kasa). `None` if the points do not fix a circle.
fn least_squares(pts: &[DVec3], c0: DVec3, n: DVec3, r0: f64) -> Option<(DVec3, f64)> {
    let u = n.any_orthonormal_vector();
    let v = n.cross(u);
    let mut mm = [[0.0f64; 3]; 3];
    let mut rhs = [0.0f64; 3];
    // Lengths in units of the radius, so that the system is well conditioned.
    for p in pts {
        let d = (*p - c0) / r0;
        let (x, y) = (d.dot(u), d.dot(v));
        let row = [2.0 * x, 2.0 * y, 1.0];
        let z = x * x + y * y;
        for r in 0..3 {
            rhs[r] += row[r] * z;
            for c in 0..3 {
                mm[r][c] += row[r] * row[c];
            }
        }
    }
    let m = glam::DMat3::from_cols_array_2d(&mm);
    let det = m.determinant();
    if !det.is_finite() || det.abs() < 1e-18 {
        return None;
    }
    let sol = m.inverse() * DVec3::from(rhs);
    let r2 = sol.z + sol.x * sol.x + sol.y * sol.y;
    if !r2.is_finite() || r2 <= 0.0 {
        return None;
    }
    let (c, r) = (c0 + (u * sol.x + v * sol.y) * r0, r2.sqrt() * r0);
    // A circle that is far from the first guess is not a refinement.
    (r.is_finite() && (r - r0).abs() <= 0.05 * r0 && c.distance(c0) <= 0.05 * r0).then_some((c, r))
}

/// The circle through the points of a chain: three points that are not `skip` give the circle,
/// and all the other points that are not `skip` refine it (least squares). The result must hold
/// the rule of the module (`shared` marks the vertices that other edges meet). `pts` of a closed
/// chain ends with its first point. The angle 0 is the direction to the first point that is not
/// skipped. `None` if the points are on a line or not on one circle.
pub(super) fn fit(pts: &[DVec3], kind: &str, skip: &[bool], shared: &[bool]) -> Option<Analytic> {
    let n = pts.len();
    if n < 3 {
        return None;
    }
    let closed = n >= 4 && pts[0].distance(pts[n - 1]) <= geom::tol(pts);
    let last = if closed { n - 1 } else { n };
    let skipped = |i: &usize| skip.get(*i).copied().unwrap_or(false);
    let met = |i: &usize| shared.get(*i).copied().unwrap_or(false);
    // Vertices that other edges meet may lie on a chord, so they do not fix the circle (while
    // enough others do).
    let mut idx: Vec<usize> = (0..last).filter(|i| !skipped(i) && !met(i)).collect();
    if idx.len() < 3 {
        idx = (0..last).filter(|i| !skipped(i)).collect();
    }
    let m = idx.len();
    if m < 3 {
        return None;
    }
    let (i, j, k) = if closed {
        (idx[0], idx[m / 3], idx[2 * m / 3])
    } else {
        (idx[0], idx[(m - 1) / 2], idx[m - 1])
    };
    let (a, b, c) = (pts[i], pts[j], pts[k]);
    let (ab, ac) = (b - a, c - a);
    let w = ab.cross(ac);
    let w2 = w.length_squared();
    let scale = ab.length_squared().max(ac.length_squared());
    if !w2.is_finite() || w2 <= 1e-12 * scale * scale {
        return None;
    }
    let mut center =
        a + (ac.length_squared() * w.cross(ab) + ab.length_squared() * ac.cross(w)) / (2.0 * w2);
    let mut radius = center.distance(a);
    let normal = w.normalize();
    let on: Vec<DVec3> = idx.iter().map(|i| pts[*i]).collect();
    if let Some((c2, r2)) = least_squares(&on, center, normal, radius) {
        (center, radius) = (c2, r2);
    }
    let axis = (pts[idx[0]] - center).normalize_or_zero();
    if radius <= 0.0 || !radius.is_finite() || axis == DVec3::ZERO {
        return None;
    }
    let mut out = Analytic::new(kind, center, normal, axis, radius, None);
    if !holds(&out, pts, shared) {
        return None;
    }
    let was = out.kind.clone();
    set_shape(&mut out, pts, closed);
    out.kind = was;
    fix_kind(&mut out, closed);
    Some(out)
}

/// A circle that is open is an arc and an arc that is closed is a circle.
fn fix_kind(a: &mut Analytic, closed: bool) {
    if closed {
        if a.kind == KIND_ARC {
            a.kind = KIND_CIRCLE.into();
        }
    } else if a.kind == KIND_CIRCLE {
        a.kind = KIND_ARC.into();
    }
}

/// The turns (radians, in `(-pi, pi]`) from each vertex of the chain to the next, about the
/// circle.
fn turns(a: &Analytic, pts: &[DVec3]) -> Vec<f64> {
    pts.windows(2)
        .map(|w| {
            let mut d = (a.angle_of(w[1]) - a.angle_of(w[0])).rem_euclid(TAU);
            if d > std::f64::consts::PI {
                d -= TAU;
            }
            d
        })
        .collect()
}

/// A chain of two edges or more goes one way round the circle and each edge turns less than half
/// a turn (a chain of one edge cannot tell, see [`follow_chain`]).
pub(super) fn goes_one_way(a: &Analytic, pts: &[DVec3]) -> bool {
    if pts.len() < 3 {
        return true;
    }
    let t = turns(a, pts);
    let limit = std::f64::consts::PI - 1e-9;
    (t.iter().all(|d| *d >= -1e-9) || t.iter().all(|d| *d <= 1e-9))
        && t.iter().all(|d| d.abs() < limit)
}

/// The direction of the chain about the normal of the circle: +1 counterclockwise, -1 clockwise.
pub(super) fn chain_direction(a: &Analytic, pts: &[DVec3]) -> f64 {
    if turns(a, pts).iter().sum::<f64>() < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// Sets the angles: none for a closed chain, else from the two ends of the chain (the direction
/// of the chain decides which end is the start; each edge of the chain is taken to sweep less
/// than half a turn).
fn set_shape(a: &mut Analytic, pts: &[DVec3], closed: bool) {
    if closed {
        a.start = None;
        a.end = None;
        return;
    }
    let (s, e) = (a.angle_of(pts[0]), a.angle_of(pts[pts.len() - 1]));
    if chain_direction(a, pts) >= 0.0 {
        a.start = Some(s);
        a.end = Some(e);
    } else {
        a.start = Some(e);
        a.end = Some(s);
    }
}

/// Keeps the kind and the angles in step with the chain. Angles that still fit the two ends of
/// the chain stay as they are (no rounding noise, and an arc of one edge that sweeps more than
/// half a turn keeps its direction).
fn follow_chain(a: &mut Analytic, pts: &[DVec3], closed: bool) {
    fix_kind(a, closed);
    if closed {
        a.start = None;
        a.end = None;
        return;
    }
    if let (Some(s), Some(e)) = (a.start, a.end) {
        let tol = tolerance(a, pts);
        let (ps, pe) = (a.point_at(s), a.point_at(e));
        let (f, l) = (pts[0], pts[pts.len() - 1]);
        let near = |p: DVec3, q: DVec3| p.distance(q) <= tol;
        if (near(ps, f) && near(pe, l)) || (near(ps, l) && near(pe, f)) {
            return;
        }
    }
    set_shape(a, pts, false);
}

/// True if `new` is a similar copy of `old` (the same shape up to a move, a turn, a mirror and
/// one scale factor): the lengths between the vertices keep one ratio.
fn similar(old: &[DVec3], new: &[DVec3]) -> bool {
    if old.len() != new.len() || old.len() < 3 {
        return false;
    }
    let total = |p: &[DVec3]| p.windows(2).map(|w| w[0].distance(w[1])).sum::<f64>();
    let (to, tn) = (total(old), total(new));
    if to <= 0.0 || tn <= 0.0 {
        return false;
    }
    let k = tn / to;
    let tol = geom::tol(new);
    let hub = [0, old.len() / 2, old.len() - 1];
    let ok =
        |a: DVec3, b: DVec3, c: DVec3, d: DVec3| (c.distance(d) - k * a.distance(b)).abs() <= tol;
    for i in 0..old.len() {
        if i + 1 < old.len() && !ok(old[i], old[i + 1], new[i], new[i + 1]) {
            return false;
        }
        for h in hub {
            if !ok(old[i], old[h], new[i], new[h]) {
                return false;
            }
        }
    }
    true
}

impl SurfaceModel {
    /// The exact description of a curve, if it has one (MOD-15).
    pub fn curve_geometry(&self, id: CurveId) -> Option<&Analytic> {
        self.analytic.get(&id)
    }

    /// The curves that have a description, in id order.
    pub fn curve_geometries(&self) -> impl Iterator<Item = (CurveId, &Analytic)> {
        self.analytic.iter().map(|(k, v)| (*k, v))
    }

    /// The edges of a curve in order.
    pub fn curve_edges(&self, id: CurveId) -> Option<&[EdgeId]> {
        self.curves.get(&id).map(Vec::as_slice)
    }

    /// The number of segments of a curve: the edges of its chain.
    pub fn curve_segments(&self, id: CurveId) -> Option<usize> {
        self.curves.get(&id).map(Vec::len)
    }

    /// The vertices of a curve in order (a closed curve ends with its first vertex).
    pub fn curve_vertices(&self, id: CurveId) -> Option<Vec<VertexId>> {
        chain_vertices(self, self.curves.get(&id)?)
    }

    /// True if the chain of the curve closes on itself.
    pub fn curve_closed(&self, id: CurveId) -> bool {
        self.curve_vertices(id).is_some_and(|v| is_closed(&v))
    }

    /// The length of a curve in local mm: the circumference for a circle, else the length of the
    /// chain (an arc, and the true perimeter of a polygon).
    pub fn curve_length(&self, id: CurveId) -> Option<f64> {
        if let Some(a) = self.analytic.get(&id)
            && a.kind == KIND_CIRCLE
        {
            return Some(a.circumference());
        }
        let pts = positions(self, &self.curve_vertices(id)?)?;
        Some(pts.windows(2).map(|w| w[0].distance(w[1])).sum())
    }

    /// Makes a curve like [`SurfaceModel::add_curve`] and gives it a description. A description
    /// that does not fit the points (see the module documentation) is not stored.
    pub fn add_curve_with(
        &mut self,
        edges: Vec<EdgeId>,
        analytic: Option<Analytic>,
    ) -> Result<CurveId, super::SurfaceError> {
        let shape = match analytic {
            Some(a) => Some(Shape::Given(sanitize(&a)?)),
            None => None,
        };
        self.add_curve_shaped(edges, shape).map(|(id, _)| id)
    }

    /// Makes a curve and gives it a description (see [`Shape`]). The second value is a note when
    /// the description was not kept.
    pub(super) fn add_curve_shaped(
        &mut self,
        edges: Vec<EdgeId>,
        shape: Option<Shape<'_>>,
    ) -> Result<(CurveId, Option<String>), super::SurfaceError> {
        let mut work = self.clone();
        let id = super::CurveId(work.alloc());
        work.curves.insert(id, edges);
        if let Some(p) = super::check::curves(&work) {
            return Err(super::SurfaceError::Unsolved(p.to_string()));
        }
        let mut note = None;
        match shape {
            Some(Shape::Given(a)) => {
                let before = work.clone();
                work.analytic.insert(id, a);
                let mut notes = Vec::new();
                reconcile(&mut work, &before, &mut notes);
                note = notes.into_iter().next();
            }
            Some(Shape::Like(a)) if !derive(&mut work, id, a) => {
                note = Some(format!(
                    "curve {id} lost its circle description: its points are not on one circle after the change"
                ));
            }
            Some(Shape::Like(_)) => {}
            None => {}
        }
        super::sync::record(&mut work, self, super::sync::Intent::Unlogged);
        *self = work;
        Ok((id, note))
    }
}

/// A row of `curve_geometry` that this version cannot read (for example a kind of a newer program
/// with other fields). It is kept as it is and written back while the chain is as it was at the
/// load (the same edges at the same places). Any change of the chain drops it, with a note.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RawRow {
    pub(super) value: Value,
    edges: Vec<EdgeId>,
    points: Vec<DVec3>,
}

impl RawRow {
    /// A row for the chain of curve `id` as it is now. `None` if the curve has no chain.
    pub(super) fn of(m: &SurfaceModel, id: CurveId, value: Value) -> Option<RawRow> {
        let edges = m.curves.get(&id)?.clone();
        let points = positions(m, &chain_vertices(m, &edges)?)?;
        Some(RawRow {
            value,
            edges,
            points,
        })
    }
}

/// Drops the unreadable rows whose chain changed, with a note each.
fn reconcile_raw(m: &mut SurfaceModel, notes: &mut Vec<String>) {
    let ids: Vec<CurveId> = m.analytic_raw.keys().copied().collect();
    for id in ids {
        let Some(es) = m.curves.get(&id) else {
            m.analytic_raw.remove(&id);
            continue;
        };
        let row = &m.analytic_raw[&id];
        let same = *es == row.edges
            && chain_vertices(m, es)
                .and_then(|vs| positions(m, &vs))
                .is_some_and(|p| p == row.points);
        if !same {
            m.analytic_raw.remove(&id);
            notes.push(format!(
                "curve {id} lost its circle description: its kind is not known to this program, so it cannot follow the change"
            ));
        }
    }
}

/// Where the description of a new curve comes from.
pub(super) enum Shape<'a> {
    /// Given by a tool.
    Given(Analytic),
    /// Fitted to the points of the new chain, with the kind of this one.
    Like(&'a Analytic),
}

/// Checks a description that a tool or the API gives with a new curve: valid numbers, a kind that
/// this version knows, and no unknown fields (a typo must not go into the file).
pub(super) fn sanitize(a: &Analytic) -> Result<Analytic, super::SurfaceError> {
    let refuse = |s: String| super::SurfaceError::Refused(s);
    if !a.is_valid() {
        return Err(refuse(
            "the circle description of the curve is not valid (a finite centre, a radius above 0, a normal and an axis)"
                .into(),
        ));
    }
    if !is_known_kind(&a.kind) {
        return Err(refuse(format!(
            "the kind of the circle description must be circle, arc or polygon, not {}",
            a.kind
        )));
    }
    if !a.extra.is_empty() {
        let names: Vec<&str> = a.extra.keys().map(String::as_str).collect();
        return Err(refuse(format!(
            "the circle description has unknown fields: {}",
            names.join(", ")
        )));
    }
    Ok(a.clone().normalised())
}

/// Gives the new curve `id` the description that its own points fit, of the same kind as `from`.
/// Returns false if the points are not on one circle (the curve has no description then) or the
/// kind is not known.
pub(super) fn derive(m: &mut SurfaceModel, id: CurveId, from: &Analytic) -> bool {
    if !is_known_kind(&from.kind) {
        return false;
    }
    let Some(vs) = m.curves.get(&id).and_then(|es| chain_vertices(m, es)) else {
        return false;
    };
    let Some(pts) = positions(m, &vs) else {
        return false;
    };
    let deg = degrees_of(m, &vs.iter().copied().collect());
    let shared = shared_flags(&vs, &deg);
    // First on all points. If a vertex that another edge meets is off the circle, again without
    // such vertices.
    let none = vec![false; pts.len()];
    match fit(&pts, &from.kind, &none, &shared).or_else(|| fit(&pts, &from.kind, &shared, &shared))
    {
        Some(a) => {
            m.analytic.insert(id, a);
            true
        }
        None => false,
    }
}

/// Keeps the descriptions in step with the chains after an edit (see the module documentation).
/// `before` is the model as it was. Notes name each description that was dropped or changed.
pub(super) fn reconcile(m: &mut SurfaceModel, before: &SurfaceModel, notes: &mut Vec<String>) {
    reconcile_raw(m, notes);
    if m.analytic.is_empty() {
        return;
    }
    struct Chain {
        id: CurveId,
        vs: Vec<VertexId>,
        pts: Vec<DVec3>,
    }
    let ids: Vec<CurveId> = m.analytic.keys().copied().collect();
    let mut chains: Vec<Chain> = Vec::new();
    for id in ids {
        let Some(es) = m.curves.get(&id) else {
            m.analytic.remove(&id);
            continue;
        };
        match chain_vertices(m, es)
            .and_then(|vs| positions(m, &vs).map(|pts| Chain { id, vs, pts }))
        {
            Some(c) => chains.push(c),
            None => {
                m.analytic.remove(&id);
                notes.push(format!(
                    "curve {id} lost its circle description: the chain is broken"
                ));
            }
        }
    }
    let all: BTreeSet<VertexId> = chains.iter().flat_map(|c| c.vs.iter().copied()).collect();
    let deg = degrees_of(m, &all);
    for c in chains {
        let a = m.analytic[&c.id].clone();
        let shared = shared_flags(&c.vs, &deg);
        let closed = is_closed(&c.vs);
        let tol = tolerance(&a, &c.pts);
        // A vertex of the chain that was in the model before and is now somewhere else.
        let moved =
            c.vs.iter()
                .zip(&c.pts)
                .any(|(v, p)| before.vertices.get(v).is_some_and(|q| q.distance(*p) > tol));
        let old = before
            .curves
            .get(&c.id)
            .filter(|_| before.analytic.contains_key(&c.id))
            .and_then(|es| chain_vertices(before, es))
            .and_then(|vs| positions(before, &vs));
        let mut kept: Option<Analytic> = None;
        let mut reason = "its points are not on one circle any more";
        if !is_known_kind(&a.kind) {
            // Read only: it stays while nothing in its chain changed.
            if !moved && before.curves.get(&c.id) == m.curves.get(&c.id) {
                kept = Some(a.clone());
            } else {
                reason = "its kind is not known to this program, so it cannot follow the change";
            }
        } else {
            if moved
                && let Some(old) = &old
                && similar(old, &c.pts)
            {
                // The chain moved as a similar copy: fit the circle again on the vertices that
                // were on the circle.
                let skip: Vec<bool> = old.iter().map(|p| a.deviation(*p) > tol).collect();
                kept = fit(&c.pts, &a.kind, &skip, &shared);
            }
            if kept.is_none() && holds(&a, &c.pts, &shared) {
                let mut k = a.clone();
                follow_chain(&mut k, &c.pts, closed);
                if k.kind != a.kind {
                    let article = if k.kind == KIND_ARC { "an" } else { "a" };
                    notes.push(format!(
                        "curve {} is now {article} {}: its centre and radius stay",
                        c.id, k.kind
                    ));
                } else if (k.start, k.end) != (a.start, a.end) {
                    notes.push(format!(
                        "curve {}: its start and end angle are updated",
                        c.id
                    ));
                }
                kept = Some(k);
            }
        }
        match kept {
            Some(k) => {
                if m.analytic[&c.id] != k {
                    m.analytic.insert(c.id, k);
                }
            }
            None => {
                m.analytic.remove(&c.id);
                notes.push(format!(
                    "curve {} lost its circle description: {reason}",
                    c.id
                ));
            }
        }
    }
}

/// The descriptions for the pieces that a curve is cut into (each piece starts with a copy; the
/// next reconcile gives each its own kind and angles).
pub(super) fn copy_to(m: &mut SurfaceModel, from: CurveId, to: CurveId) {
    if let Some(a) = m.analytic.get(&from).cloned() {
        m.analytic.insert(to, a);
    }
}

/// Checks the descriptions of a loaded model against their chains. `unread` are the rows that
/// could not be parsed. A row that does not fit its chain is left out, and the model keeps a note
/// of each (`SurfaceModel::take_load_notes`). A description of a kind that this version does not
/// know is read only: it is kept as it is, whatever its numbers say. The kind and the angles of a
/// known description are brought in step with the chain (an older program may have erased an edge
/// of a circle). Nothing is dropped without a note.
pub(super) fn validate_loaded(m: &mut SurfaceModel, unread: Vec<(u32, Value)>) {
    let mut notes: Vec<String> = Vec::new();
    let gone = |id: CurveId, why: &str| {
        format!("the file has a circle description for curve {id} that was left out: {why}")
    };
    let ids: Vec<CurveId> = m.analytic.keys().copied().collect();
    let mut chains: Vec<(CurveId, Vec<VertexId>, Vec<DVec3>)> = Vec::new();
    for id in ids {
        let chain = m
            .curves
            .get(&id)
            .map(|es| chain_vertices(m, es))
            .map(|c| c.and_then(|vs| positions(m, &vs).map(|p| (vs, p))));
        match chain {
            None => {
                m.analytic.remove(&id);
                notes.push(gone(id, "the model has no such curve"));
            }
            Some(None) => {
                m.analytic.remove(&id);
                notes.push(gone(id, "the edges of the curve are not a chain"));
            }
            Some(Some((vs, p))) if is_known_kind(&m.analytic[&id].kind) => {
                if m.analytic[&id].is_valid() {
                    chains.push((id, vs, p));
                } else {
                    m.analytic.remove(&id);
                    notes.push(gone(id, "its numbers are not valid"));
                }
            }
            // A kind of a newer program: kept as it is.
            Some(Some(_)) => {}
        }
    }
    let all: BTreeSet<VertexId> = chains.iter().flat_map(|c| c.1.iter().copied()).collect();
    let deg = degrees_of(m, &all);
    for (id, vs, pts) in chains {
        let shared = shared_flags(&vs, &deg);
        if holds(&m.analytic[&id], &pts, &shared) {
            if let Some(a) = m.analytic.get_mut(&id) {
                follow_chain(a, &pts, is_closed(&vs));
            }
        } else {
            m.analytic.remove(&id);
            notes.push(gone(
                id,
                "its points are not on the circle (the chain was changed)",
            ));
        }
    }
    for (id, value) in unread {
        let id = CurveId(id);
        match RawRow::of(m, id, value) {
            Some(row) if !m.analytic.contains_key(&id) => {
                m.analytic_raw.insert(id, row);
            }
            Some(_) => notes.push(gone(id, "the curve has two descriptions")),
            None => notes.push(gone(
                id,
                "the model has no such curve or its edges are not a chain",
            )),
        }
    }
    m.load_notes.extend(notes);
}
