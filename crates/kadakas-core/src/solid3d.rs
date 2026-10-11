//! 3D solids from 2D profiles: Extrude and Sweep (3D-03).
//!
//! An **extrude** is a closed 2D profile (curves and holes are allowed) pushed up by a height. A
//! **sweep** is a profile turned around an axis, with a sweep angle, a segment angle and a pitch
//! (the rise along the axis for each full turn, so a spiral is possible). Both keep their
//! parameters. The mesh is made from them when it is needed and it is never saved.
//!
//! Units are millimeters. The `z` of a solid is measured from the elevation of its layer.
//! The 3D polygon (several points with a `z`) is the shape `Polyline3d`; this module only gives
//! the face of a filled one (`polygon_face`).
//!
//! Limits (they keep a bad file or a bad call from filling the memory): a profile has at most
//! [`MAX_RING_POINTS`] points once its curves are cut into lines, a sweep has at most
//! [`MAX_SEGMENTS`] segments, and a mesh has at most [`MAX_TRIANGLES`] triangles.

use glam::{DMat2, DVec2, DVec3};
use serde::{Deserialize, Serialize};

use crate::curve::{self, Vertex, VertexKind};
use crate::geom;
use crate::mesh::{clean_ring, triangulate, triangulate_edge_exact};
use crate::model::MAX_COORD;

/// Most points of a profile after its curves are cut into lines (all rings together).
pub const MAX_RING_POINTS: usize = 8_000;
/// Most segments of a sweep. A larger count is lowered to this number.
pub const MAX_SEGMENTS: u32 = 720;
/// Most holes of an extrude.
pub const MAX_HOLES: usize = 256;
/// Most triangles of the mesh of one solid.
pub const MAX_TRIANGLES: usize = 400_000;
/// Default sweep angle in degrees (a full turn).
pub const DEFAULT_SWEEP_ANGLE: f64 = 360.0;
/// Default segment angle in degrees.
pub const DEFAULT_SEGMENT_ANGLE: f64 = 10.0;
/// Smallest segment angle in degrees.
pub const MIN_SEGMENT_ANGLE: f64 = 0.1;
/// Largest sweep angle in degrees (ten turns, for spirals).
pub const MAX_SWEEP_ANGLE: f64 = 3600.0;
/// The grey of a solid without a fill.
pub const DEFAULT_COLOR: [u8; 4] = [178, 172, 162, 255];
/// Largest distance between a curve and its flat segments, in mm.
const TOLERANCE: f64 = 2.0;
/// A profile point this close to the axis counts as on it, in mm.
const AXIS_EPS: f64 = 1e-6;

/// A triangle mesh of a solid. The vertices are in model units, `z` measured from the layer.
/// Triangles run counterclockwise seen from outside.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SolidMesh {
    pub vertices: Vec<DVec3>,
    pub triangles: Vec<[u32; 3]>,
}

impl SolidMesh {
    /// Box around the vertices as (min, max).
    pub fn bounds(&self) -> (DVec3, DVec3) {
        bounds_of(self.vertices.iter().copied())
    }

    /// The signed volume in mm3 (positive when the triangles face outward on a closed mesh).
    pub fn signed_volume(&self) -> f64 {
        let p = |i: u32| self.vertices[i as usize];
        self.triangles
            .iter()
            .map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2]))))
            .sum::<f64>()
            / 6.0
    }

    /// Volume in mm3 of a closed mesh.
    pub fn volume(&self) -> f64 {
        self.signed_volume().abs()
    }

    /// Surface area in mm2.
    pub fn area(&self) -> f64 {
        let p = |i: u32| self.vertices[i as usize];
        self.triangles
            .iter()
            .map(|t| (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length() / 2.0)
            .sum()
    }

    /// True when every edge belongs to exactly two triangles, once in each direction.
    pub fn is_closed(&self) -> bool {
        use std::collections::HashMap;
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for t in &self.triangles {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                if a == b {
                    return false;
                }
                *edges.entry((a, b)).or_default() += 1;
            }
        }
        !edges.is_empty()
            && edges
                .iter()
                .all(|(&(a, b), &n)| n == 1 && edges.get(&(b, a)) == Some(&1))
    }

    fn flip(&mut self) {
        for t in &mut self.triangles {
            t.swap(1, 2);
        }
    }

    /// The mesh moved up by `dz`, for example by the elevation of a layer.
    pub fn lifted(mut self, dz: f64) -> SolidMesh {
        for v in &mut self.vertices {
            v.z += dz;
        }
        self
    }
}

fn bounds_of(points: impl Iterator<Item = DVec3>) -> (DVec3, DVec3) {
    points.fold(
        (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(p), hi.max(p)),
    )
}

// ---------------------------------------------------------------------------
// Rings

/// The outline of a profile as a counterclockwise ring of points: curves cut into lines, repeated
/// and straight-through points removed. Empty when the ring has no area.
pub fn ring_points(profile: &[Vertex]) -> Vec<DVec2> {
    ring_points_limited(profile, MAX_RING_POINTS).unwrap_or_default()
}

/// Like [`ring_points`], and `None` as soon as the curves need more than `budget` points. The
/// work stays near the budget, however the vertices curve.
fn ring_points_limited(profile: &[Vertex], budget: usize) -> Option<Vec<DVec2>> {
    let flat = curve::flatten_limited(profile, true, TOLERANCE, budget)?;
    Some(clean_ring(&flat))
}

fn orient(a: DVec2, b: DVec2, c: DVec2) -> f64 {
    (b - a).perp_dot(c - a)
}

fn within(a: DVec2, b: DVec2, p: DVec2) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// True if the segments share a point (touching counts).
fn segments_meet(p1: DVec2, p2: DVec2, q1: DVec2, q2: DVec2) -> bool {
    let (d1, d2) = (orient(q1, q2, p1), orient(q1, q2, p2));
    let (d3, d4) = (orient(p1, p2, q1), orient(p1, p2, q2));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    (d1 == 0.0 && within(q1, q2, p1))
        || (d2 == 0.0 && within(q1, q2, p2))
        || (d3 == 0.0 && within(p1, p2, q1))
        || (d4 == 0.0 && within(p1, p2, q2))
}

/// Checks that no ring crosses itself or another ring (touching counts as crossing), and that
/// the rings after the first lie inside the first and not inside each other.
pub fn rings_valid(rings: &[Vec<DVec2>]) -> Result<(), &'static str> {
    struct Edge {
        ring: usize,
        i: usize,
        a: DVec2,
        b: DVec2,
    }
    let mut edges: Vec<Edge> = Vec::new();
    for (r, ring) in rings.iter().enumerate() {
        let n = ring.len();
        for i in 0..n {
            edges.push(Edge {
                ring: r,
                i,
                a: ring[i],
                b: ring[(i + 1) % n],
            });
        }
    }
    // Sweep over x: an edge is tested against the later edges that overlap it in x.
    let mut order: Vec<usize> = (0..edges.len()).collect();
    order.sort_by(|&p, &q| {
        let (ep, eq) = (&edges[p], &edges[q]);
        ep.a.x
            .min(ep.b.x)
            .total_cmp(&eq.a.x.min(eq.b.x))
            .then(p.cmp(&q))
    });
    for (k, &p) in order.iter().enumerate() {
        let ep = &edges[p];
        let hi = ep.a.x.max(ep.b.x);
        for &q in &order[k + 1..] {
            let eq = &edges[q];
            if eq.a.x.min(eq.b.x) > hi {
                break;
            }
            if ep.ring == eq.ring {
                let n = rings[ep.ring].len();
                let (lo, up) = (ep.i.min(eq.i), ep.i.max(eq.i));
                let adjacent = up == lo + 1 || (lo == 0 && up == n - 1);
                if adjacent {
                    // A spike: the second edge goes back along the first one.
                    let (u, v) = (ep.b - ep.a, eq.b - eq.a);
                    if u.perp_dot(v) == 0.0 && u.dot(v) < 0.0 {
                        return Err("the outline doubles back on itself");
                    }
                } else if segments_meet(ep.a, ep.b, eq.a, eq.b) {
                    return Err("the outline crosses itself");
                }
            } else if segments_meet(ep.a, ep.b, eq.a, eq.b) {
                return Err("a hole touches the outline or another hole");
            }
        }
    }
    for (h, hole) in rings.iter().enumerate().skip(1) {
        if !geom::point_in_polygon(hole[0], &rings[0]) {
            return Err("a hole lies outside the outline");
        }
        for (g, other) in rings.iter().enumerate().skip(1) {
            if g != h && geom::point_in_polygon(hole[0], other) {
                return Err("a hole lies inside another hole");
            }
        }
    }
    Ok(())
}

/// The ring of a closed profile, or why it cannot be used. `budget` is the number of points that
/// may still be used; the points of the ring are taken off it.
fn profile_ring(profile: &[Vertex], what: &str, budget: &mut usize) -> Result<Vec<DVec2>, String> {
    if profile.len() < 3 {
        return Err(format!("the {what} needs at least 3 vertices"));
    }
    let too_many = || format!("the {what} has too many points (at most {MAX_RING_POINTS} in all)");
    let ring = ring_points_limited(profile, *budget).ok_or_else(too_many)?;
    if ring.len() < 3 {
        return Err(format!("the {what} has no area"));
    }
    *budget = budget.checked_sub(ring.len()).ok_or_else(too_many)?;
    Ok(ring)
}

// ---------------------------------------------------------------------------
// Extrude

/// A closed 2D profile pushed up by a height.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Extrude {
    /// Closed outline. Curved segments are allowed.
    pub profile: Vec<Vertex>,
    /// Holes: closed outlines inside the profile.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<Vertex>>,
    /// Extrusion height in mm. A negative height extrudes downward.
    pub height: f64,
    /// Height of the profile above the layer, in mm.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub base: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

impl Extrude {
    pub fn new(profile: Vec<Vertex>, height: f64) -> Extrude {
        Extrude {
            profile,
            holes: Vec::new(),
            height,
            base: 0.0,
            name: String::new(),
        }
    }

    pub fn is_finite(&self) -> bool {
        self.profile.iter().all(Vertex::is_finite)
            && self.holes.iter().flatten().all(Vertex::is_finite)
            && self.height.is_finite()
            && self.base.is_finite()
    }

    /// The first problem with the values, as text.
    pub fn problem(&self) -> Option<String> {
        self.scalar_problem().or_else(|| self.rings().err())
    }

    /// The problems of the numbers, before the profile is looked at.
    fn scalar_problem(&self) -> Option<String> {
        if !self.is_finite() {
            return Some("an extrude needs finite numbers".into());
        }
        if self.height == 0.0 {
            return Some("the extrusion height must not be zero".into());
        }
        if self.height.abs() > MAX_COORD || self.base.abs() > MAX_COORD {
            return Some("the height or the base is out of range".into());
        }
        if self.holes.len() > MAX_HOLES {
            return Some(format!("an extrude has at most {MAX_HOLES} holes"));
        }
        None
    }

    /// The outline and the holes as rings: the outline counterclockwise, the holes clockwise.
    fn rings(&self) -> Result<Vec<Vec<DVec2>>, String> {
        let mut budget = MAX_RING_POINTS;
        let mut rings = vec![profile_ring(&self.profile, "profile", &mut budget)?];
        for h in &self.holes {
            let mut ring = profile_ring(h, "hole", &mut budget)?;
            ring.reverse();
            rings.push(ring);
        }
        // The check wants every ring as given; the direction does not matter for it.
        rings_valid(&rings).map_err(|e| e.to_string())?;
        Ok(rings)
    }

    /// A number that changes when any value of the extrude changes. Object Info uses it to know
    /// when its figures need to be made again.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.holes.len().hash(&mut h);
        for ring in &self.holes {
            ring.len().hash(&mut h);
        }
        for v in self.profile.iter().chain(self.holes.iter().flatten()) {
            hash_vertex(v, &mut h);
        }
        self.height.to_bits().hash(&mut h);
        self.base.to_bits().hash(&mut h);
        h.finish()
    }

    /// The volume in mm3: the area of the profile minus the holes, times the height. No mesh.
    pub fn volume(&self) -> f64 {
        self.profile_area() * self.height.abs()
    }

    /// Area of the profile minus the holes, in mm2.
    pub fn profile_area(&self) -> f64 {
        let holes: f64 = self.holes.iter().map(|h| curve::area(h).abs()).sum();
        (curve::area(&self.profile).abs() - holes).max(0.0)
    }

    /// The bottom and top of the solid above the layer, in mm.
    pub fn z_range(&self) -> (f64, f64) {
        let top = self.base + self.height;
        (self.base.min(top), self.base.max(top))
    }

    /// Box around the solid as (min, max). The profile box is exact for curves.
    pub fn bounds3(&self) -> (DVec3, DVec3) {
        let (lo, hi) = curve::bounds(&self.profile, true);
        let (z0, z1) = self.z_range();
        (lo.extend(z0), hi.extend(z1))
    }

    /// The closed mesh, or `None` when [`Extrude::problem`] finds a problem.
    pub fn mesh(&self) -> Option<SolidMesh> {
        if self.scalar_problem().is_some() {
            return None;
        }
        let rings = self.rings().ok()?;
        let n: usize = rings.iter().map(Vec::len).sum();
        let (z0, z1) = self.z_range();
        let flat: Vec<DVec2> = rings.iter().flatten().copied().collect();
        let mut vertices: Vec<DVec3> = flat.iter().map(|p| p.extend(z0)).collect();
        vertices.extend(flat.iter().map(|p| p.extend(z1)));
        let mut triangles = Vec::new();
        let mut off = 0usize;
        for ring in &rings {
            let m = ring.len();
            for i in 0..m {
                let (a, b) = ((off + i) as u32, (off + (i + 1) % m) as u32);
                let up = n as u32;
                triangles.push([a, b, b + up]);
                triangles.push([a, b + up, a + up]);
            }
            off += m;
        }
        let cap = cap_triangles(&rings)?;
        for [a, b, c] in cap {
            let up = n as u32;
            triangles.push([a + up, b + up, c + up]);
            triangles.push([a, c, b]);
        }
        Some(SolidMesh {
            vertices,
            triangles,
        })
    }
}

/// Triangles of a polygon with holes, as indices into the rings written one after the other.
/// Each hole is joined to the outline by a bridge, then the ring is cut into ears.
/// True if the direction from `b` to `d` points into the corner `a`, `b`, `c` of a ring that has
/// its inside on the left: the angle from the way on (`c`) turning left to `d` is smaller than
/// the angle from the way on to the way back (`a`).
fn in_corner(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let angle = |from: DVec2, to: DVec2| {
        let t = from.perp_dot(to).atan2(from.dot(to));
        if t < 0.0 {
            t + std::f64::consts::TAU
        } else {
            t
        }
    };
    let (next, back) = (c - b, a - b);
    let back_angle = angle(next, back);
    // A straight or reversed corner: the whole half plane to the left counts.
    let back_angle = if back_angle == 0.0 {
        std::f64::consts::TAU
    } else {
        back_angle
    };
    angle(next, d - b) < back_angle
}

fn cap_triangles(rings: &[Vec<DVec2>]) -> Option<Vec<[u32; 3]>> {
    let mut pts: Vec<DVec2> = rings[0].clone();
    let mut ids: Vec<u32> = (0..rings[0].len() as u32).collect();
    let mut offsets = Vec::new();
    let mut off = 0usize;
    for r in rings {
        offsets.push(off);
        off += r.len();
    }
    // The hole with the largest x first, as it is the easiest to see from the outline.
    let rightmost = |h: usize| {
        rings[h]
            .iter()
            .enumerate()
            .fold((0usize, f64::NEG_INFINITY), |best, (j, p)| {
                if p.x > best.1 { (j, p.x) } else { best }
            })
    };
    let mut holes: Vec<usize> = (1..rings.len()).collect();
    holes.sort_by(|&a, &b| rightmost(b).1.total_cmp(&rightmost(a).1).then(a.cmp(&b)));
    for (k, &h) in holes.iter().enumerate() {
        let hole = &rings[h];
        let rest: Vec<&Vec<DVec2>> = holes[k + 1..].iter().map(|&g| &rings[g]).collect();
        // From the rightmost point of the hole to the nearest point of the ring that it can see.
        let (j, _) = rightmost(h);
        let q = hole[j];
        let mut order: Vec<usize> = (0..pts.len()).collect();
        order.sort_by(|&a, &b| {
            pts[a]
                .distance_squared(q)
                .total_cmp(&pts[b].distance_squared(q))
                .then(a.cmp(&b))
        });
        let blocked = |ring: &[DVec2], p: DVec2| {
            let n = ring.len();
            (0..n).any(|e| {
                let (a, b) = (ring[e], ring[(e + 1) % n]);
                a != p && a != q && b != p && b != q && segments_meet(p, q, a, b)
            })
        };
        let mut tried: Vec<DVec2> = Vec::new();
        let i = order.into_iter().find_map(|i| {
            let p = pts[i];
            if tried.contains(&p) {
                return None;
            }
            tried.push(p);
            if blocked(&pts, p) || blocked(hole, p) || rest.iter().any(|r| blocked(r, p)) {
                return None;
            }
            // A point that an earlier bridge used is twice in the ring. Take the copy whose
            // corner holds the direction to the hole.
            (0..pts.len()).filter(|&c| pts[c] == p).find(|&c| {
                let n = pts.len();
                in_corner(pts[(c + n - 1) % n], p, pts[(c + 1) % n], q)
            })
        });
        let i = i?;
        let m = hole.len();
        let mut np = Vec::with_capacity(pts.len() + m + 2);
        let mut ni = Vec::with_capacity(pts.len() + m + 2);
        np.extend_from_slice(&pts[..=i]);
        ni.extend_from_slice(&ids[..=i]);
        for t in 0..=m {
            np.push(hole[(j + t) % m]);
            ni.push((offsets[h] + (j + t) % m) as u32);
        }
        np.push(pts[i]);
        ni.push(ids[i]);
        np.extend_from_slice(&pts[i + 1..]);
        ni.extend_from_slice(&ids[i + 1..]);
        pts = np;
        ids = ni;
    }
    Some(
        triangulate_edge_exact(&pts)
            .into_iter()
            .map(|t| t.map(|v| ids[v]))
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Sweep

fn default_axis() -> DVec3 {
    DVec3::Z
}
fn default_sweep_angle() -> f64 {
    DEFAULT_SWEEP_ANGLE
}
fn default_segment_angle() -> f64 {
    DEFAULT_SEGMENT_ANGLE
}
fn default_true() -> bool {
    true
}
fn is_true(b: &bool) -> bool {
    *b
}

/// A profile turned around an axis.
///
/// The profile lies in a plane through the axis. A profile point `(x, y)` has `x` as the distance
/// from the axis and `y` as the position along it. At the start the plane holds the direction `e1`,
/// which is the x axis for a vertical axis. The profile turns toward `axis x e1` (counterclockwise
/// seen from the tip of the axis).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sweep {
    pub profile: Vec<Vertex>,
    /// A closed profile makes a solid with end caps. An open one makes a surface.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub closed: bool,
    /// A point of the axis, in mm. The `z` is measured from the layer.
    #[serde(default)]
    pub origin: DVec3,
    /// Direction of the axis (any length but zero). Default: up.
    #[serde(default = "default_axis")]
    pub axis: DVec3,
    /// Angle of the profile plane around the axis at the start, in degrees.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation_deg: f64,
    /// Sweep angle in degrees: 360 is a full turn, more is a spiral.
    #[serde(default = "default_sweep_angle")]
    pub sweep_angle_deg: f64,
    /// Angle of one segment in degrees: the smaller, the rounder.
    #[serde(default = "default_segment_angle")]
    pub segment_angle_deg: f64,
    /// Rise along the axis for each full turn, in mm. Zero: a flat turn.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pitch: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

impl Sweep {
    pub fn new(profile: Vec<Vertex>) -> Sweep {
        Sweep {
            profile,
            closed: true,
            origin: DVec3::ZERO,
            axis: DVec3::Z,
            rotation_deg: 0.0,
            sweep_angle_deg: DEFAULT_SWEEP_ANGLE,
            segment_angle_deg: DEFAULT_SEGMENT_ANGLE,
            pitch: 0.0,
            name: String::new(),
        }
    }

    pub fn is_finite(&self) -> bool {
        self.profile.iter().all(Vertex::is_finite)
            && self.origin.is_finite()
            && self.axis.is_finite()
            && self.rotation_deg.is_finite()
            && self.sweep_angle_deg.is_finite()
            && self.segment_angle_deg.is_finite()
            && self.pitch.is_finite()
    }

    /// The number of segments that the mesh gets: the sweep angle divided by the segment angle,
    /// rounded up, at least 1 (3 for a full turn) and at most [`MAX_SEGMENTS`].
    pub fn segments(&self) -> u32 {
        let raw =
            (self.sweep_angle_deg.abs() / self.segment_angle_deg.max(MIN_SEGMENT_ANGLE)).ceil();
        let min = if self.is_full_turn() { 3.0 } else { 1.0 };
        raw.clamp(min, f64::from(MAX_SEGMENTS)) as u32
    }

    /// True if the segment angle was too small and the count was lowered to the maximum.
    pub fn segments_capped(&self) -> bool {
        self.sweep_angle_deg.abs() / self.segment_angle_deg.max(MIN_SEGMENT_ANGLE)
            > f64::from(MAX_SEGMENTS)
    }

    fn is_full_turn(&self) -> bool {
        self.sweep_angle_deg >= 360.0 - 1e-9
            && self.sweep_angle_deg <= 360.0 + 1e-9
            && self.pitch == 0.0
    }

    /// The unit axis and the two unit vectors `e1` and `e2` of the profile plane.
    pub fn frame(&self) -> (DVec3, DVec3, DVec3) {
        let w = self.axis.normalize_or_zero();
        let reference = if w.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
        let e1 = (reference - w * reference.dot(w)).normalize_or_zero();
        let e2 = w.cross(e1);
        let (s, c) = self.rotation_deg.to_radians().sin_cos();
        let e1r = e1 * c + e2 * s;
        (w, e1r, w.cross(e1r))
    }

    /// The profile as points: a ring (counterclockwise) when closed, else the flattened path.
    /// `None` when the curves need more than [`MAX_RING_POINTS`] points.
    fn profile_points(&self) -> Option<Vec<DVec2>> {
        if self.closed {
            ring_points_limited(&self.profile, MAX_RING_POINTS)
        } else {
            let flat = curve::flatten_limited(&self.profile, false, TOLERANCE, MAX_RING_POINTS)?;
            Some(geom::dedupe_points(&flat, 1e-9))
        }
    }

    fn side(&self, pts: &[DVec2]) -> Result<f64, String> {
        let (neg, pos) = (
            pts.iter().any(|p| p.x < -AXIS_EPS),
            pts.iter().any(|p| p.x > AXIS_EPS),
        );
        if neg && pos {
            return Err("the profile crosses the axis".into());
        }
        Ok(if neg { -1.0 } else { 1.0 })
    }

    /// The first problem with the values, as text.
    pub fn problem(&self) -> Option<String> {
        self.prepare().err()
    }

    /// Checks everything and gives the points of the profile.
    fn prepare(&self) -> Result<Vec<DVec2>, String> {
        if !self.is_finite() {
            return Err("a sweep needs finite numbers".into());
        }
        if self.axis.length() < 1e-9 {
            return Err("the axis needs a direction".into());
        }
        if self.origin.abs().max_element() > MAX_COORD || self.pitch.abs() > MAX_COORD {
            return Err("the axis point or the pitch is out of range".into());
        }
        if !(self.sweep_angle_deg > 0.0 && self.sweep_angle_deg <= MAX_SWEEP_ANGLE) {
            return Err(format!(
                "the sweep angle must be between 0 and {MAX_SWEEP_ANGLE} degrees"
            ));
        }
        if !(self.segment_angle_deg >= MIN_SEGMENT_ANGLE && self.segment_angle_deg <= 180.0) {
            return Err(format!(
                "the segment angle must be between {MIN_SEGMENT_ANGLE} and 180 degrees"
            ));
        }
        if self.sweep_angle_deg > 360.0 && self.pitch == 0.0 {
            return Err(
                "a sweep angle over 360 degrees needs a pitch, or the turns lie on each other"
                    .into(),
            );
        }
        let pts = if self.closed {
            let mut budget = MAX_RING_POINTS;
            profile_ring(&self.profile, "profile", &mut budget)?
        } else {
            if self.profile.len() < 2 {
                return Err("an open profile needs at least 2 vertices".into());
            }
            let p = self
                .profile_points()
                .ok_or("the profile has too many points")?;
            if p.len() < 2 {
                return Err("the profile has no length".into());
            }
            p
        };
        if self.closed
            && let Err(e) = rings_valid(std::slice::from_ref(&pts))
        {
            return Err(format!("the profile is not simple: {e}"));
        }
        self.side(&pts)?;
        if self.sweep_angle_deg > 360.0 {
            let (lo, hi) = pts
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
                    (a.min(p.y), b.max(p.y))
                });
            if self.pitch.abs() < hi - lo {
                return Err(
                    "the pitch is smaller than the profile along the axis: the turns cross each other"
                        .into(),
                );
            }
        }
        // The walls, and the two end caps of a solid that is not a flat full turn.
        let m = pts.len() as u64;
        let caps = if self.closed && !self.is_full_turn() {
            2 * m
        } else {
            0
        };
        if m * u64::from(self.segments()) * 2 + caps > MAX_TRIANGLES as u64 {
            return Err("the sweep has too many triangles. Raise the segment angle".into());
        }
        Ok(pts)
    }

    fn at(&self, f: &(DVec3, DVec3, DVec3), p: DVec2, theta: f64) -> DVec3 {
        let (w, e1, e2) = *f;
        let rise = if self.pitch == 0.0 {
            0.0
        } else {
            self.pitch * theta / std::f64::consts::TAU
        };
        let (s, c) = theta.sin_cos();
        self.origin + w * (p.y + rise) + (e1 * c + e2 * s) * p.x
    }

    /// Box around the solid as (min, max). It is computed from the profile points and the angles
    /// where the turned points reach an extreme, without a mesh. The box of the mesh lies inside
    /// it (the chords of a segment cut a little off a round shape).
    pub fn bounds3(&self) -> (DVec3, DVec3) {
        let none = (self.origin, self.origin);
        let Some(pts) = self.profile_points() else {
            return none;
        };
        if pts.is_empty()
            || !self.is_finite()
            || self.axis.length() < 1e-9
            || self.sweep_angle_deg <= 0.0
        {
            return none;
        }
        let f = self.frame();
        let (w, e1, e2) = f;
        let tau = std::f64::consts::TAU;
        let total = self.sweep_angle_deg.min(MAX_SWEEP_ANGLE).to_radians();
        let mut acc = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
        let mut add = |q: DVec3| {
            acc.0 = acc.0.min(q);
            acc.1 = acc.1.max(q);
        };
        for p in &pts {
            add(self.at(&f, *p, 0.0));
            add(self.at(&f, *p, total));
            if p.x == 0.0 {
                continue;
            }
            // An extreme of a coordinate: the derivative has these roots.
            for c in 0..3 {
                let (a, b) = (p.x * e1[c], p.x * e2[c]);
                let rise = w[c] * self.pitch / tau;
                let r = a.hypot(b);
                if r < 1e-12 || (rise / r).abs() > 1.0 {
                    continue;
                }
                let psi = a.atan2(b);
                let base = (-rise / r).acos();
                for sign in [1.0, -1.0] {
                    let t0 = sign * base - psi;
                    let (k0, k1) = ((-t0 / tau).ceil(), ((total - t0) / tau).floor());
                    let mut k = k0;
                    while k <= k1 {
                        add(self.at(&f, *p, t0 + k * tau));
                        k += 1.0;
                    }
                }
            }
        }
        acc
    }

    /// A number that changes when any value of the sweep changes. Object Info uses it to know
    /// when its figures need to be made again.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.closed.hash(&mut h);
        for v in self.profile.iter() {
            hash_vertex(v, &mut h);
        }
        for x in [
            self.origin.x,
            self.origin.y,
            self.origin.z,
            self.axis.x,
            self.axis.y,
            self.axis.z,
            self.rotation_deg,
            self.sweep_angle_deg,
            self.segment_angle_deg,
            self.pitch,
        ] {
            x.to_bits().hash(&mut h);
        }
        h.finish()
    }

    /// The mesh, or `None` when [`Sweep::problem`] finds a problem. A closed profile gives a closed
    /// mesh (end caps unless the sweep is a flat full turn). An open profile gives a surface.
    pub fn mesh(&self) -> Option<SolidMesh> {
        let pts = self.prepare().ok()?;
        let m = pts.len();
        let f = self.frame();
        let n = self.segments() as usize;
        let full = self.is_full_turn();
        let total = self.sweep_angle_deg.to_radians();
        let on_axis = |p: DVec2| self.pitch == 0.0 && p.x.abs() <= AXIS_EPS;
        // One vertex per profile point and angle. A point on the axis of a flat sweep is one vertex.
        let columns = if full { n } else { n + 1 };
        let mut vertices: Vec<DVec3> = Vec::new();
        let mut index = vec![vec![0u32; m]; columns];
        let mut axis_vertex: Vec<Option<u32>> = vec![None; m];
        for (k, column) in index.iter_mut().enumerate() {
            let theta = total * k as f64 / n as f64;
            for (i, slot) in column.iter_mut().enumerate() {
                if on_axis(pts[i])
                    && let Some(v) = axis_vertex[i]
                {
                    *slot = v;
                    continue;
                }
                *slot = vertices.len() as u32;
                if on_axis(pts[i]) {
                    axis_vertex[i] = Some(*slot);
                }
                vertices.push(self.at(&f, pts[i], theta));
            }
        }
        let col = |k: usize| &index[if full { k % n } else { k }];
        let mut triangles = Vec::new();
        let edges = if self.closed { m } else { m - 1 };
        for k in 0..n {
            let (c0, c1) = (col(k), col(k + 1));
            for i in 0..edges {
                let j = (i + 1) % m;
                let (a0, b0, a1, b1) = (c0[i], c0[j], c1[i], c1[j]);
                let mut push = |t: [u32; 3]| {
                    if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                        triangles.push(t);
                    }
                };
                push([a0, b1, b0]);
                push([a0, a1, b1]);
            }
        }
        if self.closed && !full {
            let flat: Vec<[usize; 3]> = triangulate_edge_exact(&pts);
            for t in flat {
                // The start cap faces backward, the end cap forward.
                triangles.push([index[0][t[0]], index[0][t[1]], index[0][t[2]]]);
                triangles.push([index[n][t[0]], index[n][t[2]], index[n][t[1]]]);
            }
        }
        let mut mesh = SolidMesh {
            vertices,
            triangles,
        };
        if self.closed && mesh.signed_volume() < 0.0 {
            mesh.flip();
        }
        Some(mesh)
    }

    /// Turns the solid about the vertical line through `center` by `angle` radians
    /// (a rotation in the plan). The axis and the profile plane turn with it.
    pub fn rotate_z(&mut self, center: DVec2, angle: f64) {
        let r = DMat2::from_angle(angle);
        let o = center + r * (self.origin.truncate() - center);
        self.remap_plan(o, r, 1.0, false);
    }

    /// Moves the solid by a map of the plan: the axis point goes to `origin_xy`, plan vectors
    /// (the axis, the direction of the profile plane) go through `lin`, and all sizes along z and
    /// in the profile are multiplied by `scale` (a uniform scale; `lin` holds the same factor).
    /// With `reflect` the map mirrors: the solid is a mirror image, which is a sweep around the
    /// opposite axis with the profile and the pitch turned over.
    pub fn remap_plan(&mut self, origin_xy: DVec2, lin: DMat2, scale: f64, reflect: bool) {
        let (_, e1_old, _) = self.frame();
        self.origin = DVec3::new(origin_xy.x, origin_xy.y, self.origin.z * scale);
        let ax = lin * self.axis.truncate();
        self.axis = DVec3::new(ax.x, ax.y, self.axis.z * scale);
        let want_xy = lin * e1_old.truncate();
        let want = DVec3::new(want_xy.x, want_xy.y, e1_old.z * scale).normalize_or_zero();
        for v in &mut self.profile {
            v.p *= scale;
            if let VertexKind::Arc { radius } = &mut v.kind {
                *radius *= scale;
            }
        }
        self.pitch *= scale;
        if reflect {
            self.axis = -self.axis;
            for v in &mut self.profile {
                v.p.y = -v.p.y;
            }
            self.pitch = -self.pitch;
        }
        self.rotation_deg = 0.0;
        let (_, e1, e2) = self.frame();
        let phi = want.dot(e2).atan2(want.dot(e1));
        self.rotation_deg = if phi.is_finite() {
            phi.to_degrees()
        } else {
            0.0
        };
    }
}

fn hash_vertex(v: &Vertex, h: &mut impl std::hash::Hasher) {
    use std::hash::Hash;
    v.p.x.to_bits().hash(h);
    v.p.y.to_bits().hash(h);
    match v.kind {
        VertexKind::Corner => 0u8.hash(h),
        VertexKind::Arc { radius } => (1u8, radius.to_bits()).hash(h),
        VertexKind::PointOnArc => 2u8.hash(h),
        VertexKind::Bezier => 3u8.hash(h),
        VertexKind::Cubic => 4u8.hash(h),
    }
}

// ---------------------------------------------------------------------------
// 3D polygon

/// The triangles of a filled 3D polygon, as indices into `points`, facing along the normal of the
/// polygon (the Newell normal). The polygon is cut in the plane that suits its normal best.
/// `None` for fewer than 3 points or a polygon without area.
pub fn polygon_face(points: &[DVec3]) -> Option<Vec<[u32; 3]>> {
    let n = points.len();
    if !(3..=MAX_RING_POINTS).contains(&n) || points.iter().any(|p| !p.is_finite()) {
        return None;
    }
    let normal = (0..n).fold(DVec3::ZERO, |s, i| {
        let (a, b) = (points[i], points[(i + 1) % n]);
        s + DVec3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        )
    });
    if normal.length() < 1e-9 {
        return None;
    }
    // Project along the biggest component of the normal. The projection keeps the direction
    // when that component is positive, so a negative one flips the winding.
    let ax = [normal.x.abs(), normal.y.abs(), normal.z.abs()];
    let drop = if ax[0] >= ax[1] && ax[0] >= ax[2] {
        0
    } else if ax[1] >= ax[2] {
        1
    } else {
        2
    };
    let flat: Vec<DVec2> = points
        .iter()
        .map(|p| match drop {
            0 => DVec2::new(p.y, p.z),
            1 => DVec2::new(p.z, p.x),
            _ => DVec2::new(p.x, p.y),
        })
        .collect();
    let positive = [normal.x, normal.y, normal.z][drop] > 0.0;
    let tris = triangulate(&flat);
    if tris.is_empty() {
        return None;
    }
    Some(
        tris.into_iter()
            .map(|[a, b, c]| {
                if positive {
                    [a as u32, b as u32, c as u32]
                } else {
                    [a as u32, c as u32, b as u32]
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests;
