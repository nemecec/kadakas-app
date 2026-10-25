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
//! the length tolerance. The only exception is a vertex on a straight piece between two others
//! (a chain edge that another edge cut), which lies on the chord. The invariant is kept in one
//! place: [`reconcile`] runs after every edit (`edit::run`) and compares each curve with the
//! model before the edit.
//!
//! - The chain still lies on the circle: the field stays. The kind and the angles follow the
//!   chain: a circle that lost an edge is an arc, and an arc that lost edges at its ends has new
//!   start and end angles (the centre and the radius stay).
//! - The vertices moved and the chain is a similar copy of the old one (same ratios of all
//!   lengths, which is move, rotate, mirror and uniform scale): the circle is fitted again.
//! - Else the field is dropped, with a note in the result.
//!
//! New curves that come from a curve with the field (the copy at the top of a Push/Pull, the
//! profile copies of Follow Me, the Offset of a circle) get the field from their own points
//! ([`derive`]). Nothing here moves a vertex.
//!
//! Saved form: the model field `curve_geometry` is a list of `[curve id, {kind, center, normal,
//! axis, radius, start?, end?}]` in id order. An older program keeps the field it does not know.
//! A file without the field loads as before. Entries whose curve is gone or whose points are not on
//! the circle (an older program edited the chain) are dropped on load.

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

    /// The arc length of the chain's circle part, or the circumference of a full circle.
    pub fn length(&self) -> f64 {
        self.radius * self.sweep().unwrap_or(TAU)
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

/// True if every vertex lies on the circle, or on the straight piece between its two neighbours.
/// `pts` of a closed chain ends with its first point.
pub(super) fn holds(a: &Analytic, pts: &[DVec3]) -> bool {
    classify(a, pts).is_some()
}

/// The indices of the vertices that are not on the circle but on the straight piece between
/// their neighbours, or `None` if one vertex is on neither.
fn classify(a: &Analytic, pts: &[DVec3]) -> Option<Vec<usize>> {
    let n = pts.len();
    let tol = tolerance(a, pts);
    let closed = n >= 4 && pts[0].distance(pts[n - 1]) <= tol;
    let last = if closed { n - 1 } else { n };
    let mut out = Vec::new();
    for i in 0..last {
        if a.deviation(pts[i]) <= tol {
            continue;
        }
        let prev = match i {
            0 if closed => Some(pts[n - 2]),
            0 => None,
            _ => Some(pts[i - 1]),
        };
        let next = if i + 1 < last {
            Some(pts[i + 1])
        } else if closed {
            Some(pts[1])
        } else {
            None
        };
        match (prev, next) {
            (Some(p), Some(q)) if geom::point_segment(pts[i], p, q).0 <= tol => out.push(i),
            _ => return None,
        }
    }
    Some(out)
}

/// The circle through the points of a chain, fitted on three of them and checked on all.
/// `pts` of a closed chain ends with its first point. The angle 0 is the direction to the first
/// point. `None` if the points are on a line or not all on one circle.
pub(super) fn fit(pts: &[DVec3], kind: &str) -> Option<Analytic> {
    let n = pts.len();
    if n < 3 {
        return None;
    }
    let closed = n >= 4 && pts[0].distance(pts[n - 1]) <= geom::tol(pts);
    let (i, j, k) = if closed {
        let d = n - 1;
        (0, d / 3, 2 * d / 3)
    } else {
        (0, (n - 1) / 2, n - 1)
    };
    if i == j || j == k {
        return None;
    }
    let (a, b, c) = (pts[i], pts[j], pts[k]);
    let (ab, ac) = (b - a, c - a);
    let w = ab.cross(ac);
    let w2 = w.length_squared();
    let scale = ab.length_squared().max(ac.length_squared());
    if !w2.is_finite() || w2 <= 1e-12 * scale * scale {
        return None;
    }
    let center =
        a + (ac.length_squared() * w.cross(ab) + ab.length_squared() * ac.cross(w)) / (2.0 * w2);
    let radius = center.distance(a);
    let normal = w.normalize();
    let axis = (pts[0] - center).normalize_or_zero();
    if radius <= 0.0 || !radius.is_finite() || axis == DVec3::ZERO {
        return None;
    }
    let mut out = Analytic::new(kind, center, normal, axis, radius, None);
    if !holds(&out, pts) {
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

/// Sets the angles: none for a closed chain, else from the two ends of the chain (the direction
/// of the chain decides which end is the start).
fn set_shape(a: &mut Analytic, pts: &[DVec3], closed: bool) {
    if closed {
        a.start = None;
        a.end = None;
        return;
    }
    let (first, last) = (pts[0], pts[pts.len() - 1]);
    let second = pts[1];
    let (a0, a1) = (a.angle_of(first), a.angle_of(second));
    let mut d = (a1 - a0).rem_euclid(TAU);
    if d > std::f64::consts::PI {
        d -= TAU;
    }
    let (s, e) = (a0, a.angle_of(last));
    if d >= 0.0 {
        a.start = Some(s);
        a.end = Some(e);
    } else {
        a.start = Some(e);
        a.end = Some(s);
    }
}

/// Keeps the kind and the angles in step with the chain. Angles that still fit the two ends of
/// the chain stay as they are (no rounding noise).
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

    /// Makes a curve like [`SurfaceModel::add_curve`] and gives it a description. A description
    /// that does not fit the points (see the module documentation) is not stored.
    pub fn add_curve_with(
        &mut self,
        edges: Vec<EdgeId>,
        analytic: Option<Analytic>,
    ) -> Result<CurveId, super::SurfaceError> {
        self.add_curve_shaped(edges, analytic.map(Shape::Given))
    }

    /// Makes a curve and gives it a description (see [`Shape`]).
    pub(super) fn add_curve_shaped(
        &mut self,
        edges: Vec<EdgeId>,
        shape: Option<Shape<'_>>,
    ) -> Result<CurveId, super::SurfaceError> {
        let mut work = self.clone();
        let id = super::CurveId(work.alloc());
        work.curves.insert(id, edges);
        if let Some(p) = super::check::curves(&work) {
            return Err(super::SurfaceError::Unsolved(p.to_string()));
        }
        match shape {
            Some(Shape::Given(a)) => {
                let before = work.clone();
                work.analytic.insert(id, a.normalised());
                reconcile(&mut work, &before, &mut Vec::new());
            }
            Some(Shape::Like(a)) => {
                derive(&mut work, id, a);
            }
            None => {}
        }
        super::sync::record(&mut work, self, super::sync::Intent::Unlogged);
        *self = work;
        Ok(id)
    }
}

/// Where the description of a new curve comes from.
pub(super) enum Shape<'a> {
    /// Given by a tool.
    Given(Analytic),
    /// Fitted to the points of the new chain, with the kind of this one.
    Like(&'a Analytic),
}

/// Checks a description that a tool gives with a new curve.
pub(super) fn sanitize(a: &Analytic) -> Result<Analytic, super::SurfaceError> {
    if !a.is_valid() {
        return Err(super::SurfaceError::Refused(
            "the circle description of the curve is not valid (a finite centre, a radius above 0, a normal and an axis)"
                .into(),
        ));
    }
    Ok(a.clone().normalised())
}

/// Gives the new curve `id` the description that its own points fit, of the same kind as `from`.
/// Returns false if the points are not on one circle (the curve has no description then).
pub(super) fn derive(m: &mut SurfaceModel, id: CurveId, from: &Analytic) -> bool {
    let Some(vs) = m.curves.get(&id).and_then(|es| chain_vertices(m, es)) else {
        return false;
    };
    let Some(pts) = positions(m, &vs) else {
        return false;
    };
    match fit(&pts, &from.kind) {
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
    if m.analytic.is_empty() {
        return;
    }
    let ids: Vec<CurveId> = m.analytic.keys().copied().collect();
    for id in ids {
        let Some(es) = m.curves.get(&id) else {
            m.analytic.remove(&id);
            continue;
        };
        let a = m.analytic[&id].clone();
        let chain = chain_vertices(m, es).and_then(|vs| positions(m, &vs).map(|p| (vs, p)));
        let Some((vs, pts)) = chain else {
            m.analytic.remove(&id);
            notes.push(format!(
                "curve {id} lost its circle description: the chain is broken"
            ));
            continue;
        };
        let closed = is_closed(&vs);
        let mut kept = None;
        if holds(&a, &pts) {
            let mut k = a.clone();
            follow_chain(&mut k, &pts, closed);
            if k.kind != a.kind {
                let article = if k.kind == KIND_ARC { "an" } else { "a" };
                notes.push(format!(
                    "curve {id} is now {article} {}: its centre and radius stay",
                    k.kind
                ));
            } else if (k.start, k.end) != (a.start, a.end) {
                notes.push(format!("curve {id}: its start and end angle are updated"));
            }
            kept = Some(k);
        } else {
            let old = before
                .curves
                .get(&id)
                .filter(|_| before.analytic.contains_key(&id))
                .and_then(|es| chain_vertices(before, es))
                .and_then(|vs| positions(before, &vs));
            if let Some(old) = old
                && similar(&old, &pts)
                && let Some(mut k) = fit(&pts, &a.kind)
            {
                k.extra = a.extra.clone();
                kept = Some(k);
            }
        }
        match kept {
            Some(k) => {
                if m.analytic[&id] != k {
                    m.analytic.insert(id, k);
                }
            }
            None => {
                m.analytic.remove(&id);
                notes.push(format!(
                    "curve {id} lost its circle description: its points are not on one circle any more"
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

/// Drops the descriptions of curves that do not exist or whose points are off the circle (a file
/// that an older program changed).
pub(super) fn validate_loaded(m: &mut SurfaceModel) {
    let ids: Vec<CurveId> = m.analytic.keys().copied().collect();
    for id in ids {
        let ok = m
            .curves
            .get(&id)
            .and_then(|es| chain_vertices(m, es))
            .and_then(|vs| positions(m, &vs))
            .is_some_and(|pts| m.analytic[&id].is_valid() && holds(&m.analytic[&id], &pts));
        if !ok {
            m.analytic.remove(&id);
        }
    }
}
