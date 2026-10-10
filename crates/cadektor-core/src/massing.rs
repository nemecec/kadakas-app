//! Massing models: simple building blocks for context buildings and site studies.
//!
//! A massing model has a footprint polygon (curves are allowed), an eave height (or a number of
//! floors times a floor height) and a simple roof: flat, gable or hip. The solid is closed, so its
//! volume is exact. The base sits at `base` above the layer, or on the site model when `base` is
//! empty (the site crate gives the ground height, see `Massing::base_on`).
//!
//! Units are millimeters. The roof is a height field over the oriented bounding rectangle of the
//! footprint. A gable roof rises from the two long eaves to a ridge. A hip roof also slopes at the
//! two short ends. On a footprint that is not a rectangle the roof is cut at the footprint, so an
//! L-shaped building gets a roof that follows the rectangle around it.

use std::collections::HashMap;

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

use crate::curve::{self, Vertex};
use crate::mesh::{clean_ring, triangulate};

/// Default eave height (three floors of 3 m would be 9 m; one floor is the default).
pub const DEFAULT_HEIGHT: f64 = 3000.0;
/// Default height of one floor, in mm.
pub const DEFAULT_FLOOR_HEIGHT: f64 = 3000.0;
/// Default roof pitch in degrees.
pub const DEFAULT_PITCH_DEG: f64 = 30.0;
/// Largest roof pitch in degrees.
pub const MAX_PITCH_DEG: f64 = 80.0;
/// Largest distance between a curve and its flat segments, in mm.
const TOLERANCE: f64 = 2.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoofStyle {
    #[default]
    Flat,
    Gable,
    Hip,
}

impl RoofStyle {
    pub fn from_name(s: &str) -> Option<RoofStyle> {
        match s.trim().to_ascii_lowercase().as_str() {
            "flat" => Some(RoofStyle::Flat),
            "gable" | "gabled" => Some(RoofStyle::Gable),
            "hip" | "hipped" => Some(RoofStyle::Hip),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RoofStyle::Flat => "flat",
            RoofStyle::Gable => "gable",
            RoofStyle::Hip => "hip",
        }
    }
}

fn default_height() -> f64 {
    DEFAULT_HEIGHT
}
fn default_floor_height() -> f64 {
    DEFAULT_FLOOR_HEIGHT
}
fn default_pitch() -> f64 {
    DEFAULT_PITCH_DEG
}

/// A building block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Massing {
    /// Closed outline. Curved segments are allowed.
    pub footprint: Vec<Vertex>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Eave height in mm. Used when `floors` is empty.
    #[serde(default = "default_height")]
    pub height: f64,
    /// Number of floors. The eave height is then `floors` times `floor_height`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floors: Option<u32>,
    #[serde(default = "default_floor_height")]
    pub floor_height: f64,
    #[serde(default)]
    pub roof: RoofStyle,
    /// Roof pitch in degrees (gable and hip).
    #[serde(default = "default_pitch")]
    pub pitch_deg: f64,
    /// Direction of the ridge in radians. Empty: along the long side of the oriented bounding rectangle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ridge_angle: Option<f64>,
    /// Height of the base above the layer, in mm. Empty: the building sits on the site model
    /// of its layer, or at the layer when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<f64>,
}

impl Massing {
    /// A flat-roof block with an eave height.
    pub fn new(footprint: Vec<Vertex>, height: f64) -> Massing {
        Massing {
            footprint,
            name: String::new(),
            height,
            floors: None,
            floor_height: DEFAULT_FLOOR_HEIGHT,
            roof: RoofStyle::Flat,
            pitch_deg: DEFAULT_PITCH_DEG,
            ridge_angle: None,
            base: None,
        }
    }

    pub fn is_finite(&self) -> bool {
        self.footprint.iter().all(|v| v.p.is_finite())
            && self.height.is_finite()
            && self.floor_height.is_finite()
            && self.pitch_deg.is_finite()
            && self.ridge_angle.is_none_or(f64::is_finite)
            && self.base.is_none_or(f64::is_finite)
    }

    /// The first problem with the values, as text.
    pub fn problem(&self) -> Option<String> {
        if !self.is_finite() {
            return Some("a massing model needs finite numbers".into());
        }
        if self.footprint.len() < 3 {
            return Some("the footprint needs at least 3 vertices".into());
        }
        if self.height <= 0.0 && self.floors.is_none() {
            return Some("the height must be positive".into());
        }
        if self.floor_height <= 0.0 {
            return Some("the floor height must be positive".into());
        }
        if self.floors == Some(0) {
            return Some("the number of floors must be at least 1".into());
        }
        if !(0.0..=MAX_PITCH_DEG).contains(&self.pitch_deg) {
            return Some(format!(
                "the roof pitch must be between 0 and {MAX_PITCH_DEG} degrees"
            ));
        }
        if self.footprint_area() <= 0.0 {
            return Some("the footprint has no area".into());
        }
        None
    }

    /// The roof that is built. A gable or hip roof is built over the bounding rectangle of the
    /// footprint, which is right for a convex footprint only. An L-shaped or other non-convex
    /// footprint gets a flat roof (see [`Massing::roof_warning`]).
    pub fn effective_roof(&self) -> RoofStyle {
        if self.roof != RoofStyle::Flat && !is_convex(&self.ring()) {
            RoofStyle::Flat
        } else {
            self.roof
        }
    }

    /// A message for the user when the roof that is built is not the roof that was asked for.
    pub fn roof_warning(&self) -> Option<String> {
        (self.effective_roof() != self.roof).then(|| {
            format!(
                "the footprint is not convex, so the {} roof is built flat. Split the building into convex parts for a pitched roof",
                self.roof.name()
            )
        })
    }

    /// Eave height in mm.
    pub fn eave_height(&self) -> f64 {
        match self.floors {
            Some(n) => f64::from(n) * self.floor_height,
            None => self.height,
        }
    }

    /// Number of floors: the given number, or the height divided by the floor height (at least 1).
    pub fn floor_count(&self) -> u32 {
        self.floors
            .unwrap_or_else(|| ((self.height / self.floor_height).round().max(1.0)) as u32)
    }

    /// The outline as a counterclockwise ring of points.
    pub fn ring(&self) -> Vec<DVec2> {
        clean_ring(&curve::flatten(&self.footprint, true, TOLERANCE))
    }

    /// Footprint area in mm2 (curves count).
    pub fn footprint_area(&self) -> f64 {
        curve::area(&self.footprint).abs()
    }

    /// The oriented rectangle of the roof: (center, ridge direction, half length along the ridge, half width).
    pub fn roof_frame(&self, ring: &[DVec2]) -> (DVec2, DVec2, f64, f64) {
        let angle = match self.ridge_angle {
            Some(a) => a,
            None => min_rect_angle(ring),
        };
        let mut u = DVec2::from_angle(angle);
        let frame = |u: DVec2| {
            let v = u.perp();
            let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
            for p in ring {
                let q = DVec2::new(p.dot(u), p.dot(v));
                lo = lo.min(q);
                hi = hi.max(q);
            }
            (lo, hi)
        };
        let (mut lo, mut hi) = frame(u);
        // Without a given ridge direction the ridge runs along the longer side.
        if self.ridge_angle.is_none() && (hi.y - lo.y) > (hi.x - lo.x) {
            u = u.perp();
            (lo, hi) = frame(u);
        }
        let mid = (lo + hi) / 2.0;
        let center = u * mid.x + u.perp() * mid.y;
        (center, u, (hi.x - lo.x) / 2.0, (hi.y - lo.y) / 2.0)
    }

    /// Height of the roof above the eave at `p` (0 for a flat roof).
    pub fn roof_at(&self, p: DVec2, frame: &(DVec2, DVec2, f64, f64)) -> f64 {
        if self.effective_roof() == RoofStyle::Flat || self.pitch_deg <= 0.0 {
            return 0.0;
        }
        let (c, u, a, b) = *frame;
        let d = p - c;
        let (x, y) = (d.dot(u).abs(), d.dot(u.perp()).abs());
        let t = self.pitch_deg.to_radians().tan();
        let h = match self.roof {
            RoofStyle::Gable => b - y,
            _ => (a - x).min(b - y),
        };
        (h * t).max(0.0)
    }

    /// Height of the ridge above the eave in mm.
    pub fn roof_rise(&self) -> f64 {
        if self.effective_roof() == RoofStyle::Flat {
            return 0.0;
        }
        let ring = self.ring();
        if ring.len() < 3 {
            return 0.0;
        }
        let f = self.roof_frame(&ring);
        let t = self.pitch_deg.to_radians().tan();
        match self.roof {
            RoofStyle::Gable => f.3 * t,
            _ => f.2.min(f.3) * t,
        }
    }

    /// The closed solid with its base at `base_z` (absolute z in mm). Triangles are
    /// counterclockwise seen from outside. `bottom` false leaves out the floor face.
    pub fn solid(&self, base_z: f64, bottom: bool) -> Solid {
        let ring = self.ring();
        let mut s = Solid::default();
        if ring.len() < 3 {
            return s;
        }
        let eave = self.eave_height();
        let frame = self.roof_frame(&ring);
        let mut tris: Vec<[DVec2; 3]> = triangulate(&ring)
            .into_iter()
            .map(|[a, b, c]| [ring[a], ring[b], ring[c]])
            .collect();
        if self.effective_roof() != RoofStyle::Flat && self.pitch_deg > 0.0 {
            for line in crease_lines(self.roof, &frame) {
                tris = tris.into_iter().flat_map(|t| split(t, line)).collect();
            }
        }
        // Vertices are shared by position.
        let mut ids: HashMap<(i64, i64), u32> = HashMap::new();
        let mut xy: Vec<DVec2> = Vec::new();
        let mut index = |p: DVec2| -> u32 {
            *ids.entry(key(p)).or_insert_with(|| {
                xy.push(p);
                (xy.len() - 1) as u32
            })
        };
        let top: Vec<[u32; 3]> = tris
            .iter()
            .map(|t| [index(t[0]), index(t[1]), index(t[2])])
            .filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
            .collect();
        let n = xy.len() as u32;
        s.positions = xy
            .iter()
            .map(|p| p.extend(base_z + eave + self.roof_at(*p, &frame)))
            .collect();
        s.positions.extend(xy.iter().map(|p| p.extend(base_z)));
        s.tris.extend(top.iter().copied());
        if bottom {
            s.tris
                .extend(top.iter().map(|t| [n + t[0], n + t[2], n + t[1]]));
        }
        // Walls on the edges that have no neighbor.
        let mut dir: HashMap<(u32, u32), u32> = HashMap::new();
        for t in &top {
            for k in 0..3 {
                *dir.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
            }
        }
        for (&(a, b), _) in dir.iter() {
            if !dir.contains_key(&(b, a)) {
                s.tris.push([n + a, n + b, b]);
                s.tris.push([n + a, b, a]);
            }
        }
        s.tris.sort_unstable();
        s
    }

    /// Quantities for a solid at any height. Volume is the volume of the closed solid.
    pub fn quantities(&self) -> MassingQuantities {
        let solid = self.solid(0.0, true);
        let footprint = self.footprint_area();
        let eave = self.eave_height();
        let (roof_area, wall_area) = solid.areas();
        MassingQuantities {
            footprint_area: footprint,
            floors: self.floor_count(),
            gross_floor_area: footprint * f64::from(self.floor_count()),
            eave_height: eave,
            ridge_height: eave + self.roof_rise(),
            volume: solid.volume(),
            volume_to_eave: footprint * eave,
            roof_area,
            wall_area,
        }
    }
}

/// Default color of a massing model in 3D.
pub const DEFAULT_COLOR: [u8; 4] = [214, 208, 198, 255];
const ROOF_TINT: f64 = 0.72;

/// The ridge and hip lines of the roof, cut at the footprint, for the 2D drawing.
pub fn roof_lines(m: &Massing) -> Vec<(DVec2, DVec2)> {
    if m.effective_roof() == RoofStyle::Flat || m.pitch_deg <= 0.0 {
        return Vec::new();
    }
    let ring = m.ring();
    if ring.len() < 3 {
        return Vec::new();
    }
    let (c, u, a, b) = m.roof_frame(&ring);
    let v = u.perp();
    let mut segs: Vec<(DVec2, DVec2)> = Vec::new();
    match m.roof {
        RoofStyle::Gable => segs.push((c - u * a, c + u * a)),
        _ => {
            let k = (a - b).max(0.0);
            let (r0, r1) = (c - u * k, c + u * k);
            if k > 1e-6 {
                segs.push((r0, r1));
            }
            for (end, sx) in [(r0, -1.0), (r1, 1.0)] {
                for sy in [-1.0, 1.0] {
                    segs.push((end, c + u * (sx * a) + v * (sy * b)));
                }
            }
        }
    }
    segs.into_iter()
        .flat_map(|(p, q)| clip_segment(&ring, p, q))
        .collect()
}

/// The parts of the segment `p..q` that lie inside the polygon.
fn clip_segment(ring: &[DVec2], p: DVec2, q: DVec2) -> Vec<(DVec2, DVec2)> {
    let d = q - p;
    let mut ts = vec![0.0, 1.0];
    let n = ring.len();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let e = b - a;
        let den = d.perp_dot(e);
        if den.abs() < 1e-12 {
            continue;
        }
        let t = (a - p).perp_dot(e) / den;
        let w = (a - p).perp_dot(d) / den;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&w) {
            ts.push(t);
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.windows(2)
        .filter(|w| w[1] - w[0] > 1e-9 && inside(ring, p + d * ((w[0] + w[1]) / 2.0)))
        .map(|w| (p + d * w[0], p + d * w[1]))
        .collect()
}

fn inside(ring: &[DVec2], pt: DVec2) -> bool {
    let n = ring.len();
    let mut c = false;
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if (a.y > pt.y) != (b.y > pt.y) && pt.x < (b.x - a.x) * (pt.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
    }
    c
}

/// Draws the solid in a 3D scene. Faces are flat shaded, the roof a little darker. `base` is
/// the height of the base above the origin of the sink.
pub fn emit(
    m: &Massing,
    base: f64,
    color: [u8; 4],
    edge: [u8; 4],
    sink: &mut crate::mesh::SceneSink<'_>,
) {
    let solid = m.solid(base, true);
    let roof_color = [
        (f64::from(color[0]) * ROOF_TINT) as u8,
        (f64::from(color[1]) * ROOF_TINT) as u8,
        (f64::from(color[2]) * ROOF_TINT) as u8,
        color[3],
    ];
    let (mut walls, mut roofs, mut floors) = (
        (Vec::new(), Vec::new()),
        (Vec::new(), Vec::new()),
        (Vec::new(), Vec::new()),
    );
    for t in &solid.tris {
        let pts = t.map(|i| solid.positions[i as usize]);
        let n = (pts[1] - pts[0]).cross(pts[2] - pts[0]);
        let part = if n.z > 1e-9 * n.length() {
            &mut roofs
        } else if n.z < -1e-9 * n.length() {
            &mut floors
        } else {
            &mut walls
        };
        let at = part.0.len() as u32;
        part.0.extend(pts);
        part.1.extend([at, at + 1, at + 2]);
    }
    // Each face has its own vertices, so the shading stays flat.
    sink.mesh_oriented(&walls.0, &walls.1, color);
    sink.mesh_oriented(&roofs.0, &roofs.1, roof_color);
    // The floor faces down, so that the solid is closed.
    sink.mesh_oriented(&floors.0, &floors.1, color);
    let ring = m.ring();
    let eave = base + m.eave_height();
    for z in [base, eave] {
        let pts: Vec<DVec3> = ring.iter().map(|p| p.extend(z)).collect();
        sink.polyline(&pts, true, edge);
    }
    let frame = m.roof_frame(&ring);
    for (p, q) in roof_lines(m) {
        sink.polyline(
            &[
                p.extend(eave + m.roof_at(p, &frame)),
                q.extend(eave + m.roof_at(q, &frame)),
            ],
            false,
            edge,
        );
    }
}

/// Areas in mm2, volumes in mm3, heights in mm.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MassingQuantities {
    pub footprint_area: f64,
    pub floors: u32,
    pub gross_floor_area: f64,
    pub eave_height: f64,
    pub ridge_height: f64,
    pub volume: f64,
    pub volume_to_eave: f64,
    pub roof_area: f64,
    pub wall_area: f64,
}

/// A triangle mesh with shared vertices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Solid {
    pub positions: Vec<DVec3>,
    pub tris: Vec<[u32; 3]>,
}

impl Solid {
    /// Volume by the divergence theorem. Positive when the triangles face outward.
    pub fn volume(&self) -> f64 {
        let o = self.positions.first().copied().unwrap_or(DVec3::ZERO);
        self.tris
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| self.positions[i as usize] - o);
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    /// (area of the roof faces, area of the wall faces): upward faces are the roof, vertical
    /// faces are the walls. The floor is not counted.
    fn areas(&self) -> (f64, f64) {
        let (mut roof, mut wall) = (0.0, 0.0);
        let base = self
            .positions
            .iter()
            .map(|p| p.z)
            .fold(f64::INFINITY, f64::min);
        for t in &self.tris {
            let [a, b, c] = t.map(|i| self.positions[i as usize]);
            let n = (b - a).cross(c - a);
            let area = n.length() / 2.0;
            if n.z.abs() > 1e-9 * n.length().max(1e-300) && n.z > 0.0 {
                roof += area;
            } else if n.z.abs() <= 1e-9 * n.length().max(1e-300)
                && [a, b, c].iter().any(|p| p.z > base + 1e-9)
            {
                wall += area;
            }
        }
        (roof, wall)
    }

    /// True if every edge is shared by exactly two triangles, in opposite directions.
    pub fn is_closed(&self) -> bool {
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for t in &self.tris {
            for k in 0..3 {
                *edges.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
            }
        }
        !edges.is_empty()
            && edges
                .iter()
                .all(|(&(a, b), &n)| n == 1 && edges.get(&(b, a)) == Some(&1))
    }
}

/// True if the counterclockwise ring turns left at every corner (a small tolerance lets the
/// flat segments of a curve count as convex).
fn is_convex(ring: &[DVec2]) -> bool {
    let n = ring.len();
    let scale = ring
        .iter()
        .fold(0.0f64, |m, p| m.max(p.abs().max_element()))
        .max(1.0);
    (0..n).all(|i| {
        let (a, b, c) = (ring[i], ring[(i + 1) % n], ring[(i + 2) % n]);
        (b - a).perp_dot(c - b) >= -1e-9 * scale * scale
    })
}

fn key(p: DVec2) -> (i64, i64) {
    ((p.x * 1.0e4).round() as i64, (p.y * 1.0e4).round() as i64)
}

/// Angle of the longest-side-aligned bounding rectangle of least area, from the edge directions of the ring.
fn min_rect_angle(ring: &[DVec2]) -> f64 {
    let mut best = (f64::INFINITY, 0.0);
    let n = ring.len();
    for i in 0..n {
        let e = ring[(i + 1) % n] - ring[i];
        if e.length_squared() < 1e-12 {
            continue;
        }
        let u = e.normalize();
        let v = u.perp();
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for p in ring {
            let q = DVec2::new(p.dot(u), p.dot(v));
            lo = lo.min(q);
            hi = hi.max(q);
        }
        let area = (hi.x - lo.x) * (hi.y - lo.y);
        if best.0.is_infinite() || area < best.0 * (1.0 - 1e-9) {
            best = (area, u.to_angle());
        }
    }
    best.1
}

/// A line `n . p = d`, as (n, d).
type Line = (DVec2, f64);

/// Lines where the roof surface bends, in the world frame.
fn crease_lines(roof: RoofStyle, frame: &(DVec2, DVec2, f64, f64)) -> Vec<Line> {
    let (c, u, a, b) = *frame;
    let v = u.perp();
    let mut lines = vec![(v, v.dot(c))];
    if roof == RoofStyle::Hip {
        // The slopes meet where the distance to the long eave equals the distance to the end.
        let k = a - b;
        for (s, t) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            let n = u * s + v * t;
            lines.push((n, n.dot(c) + k));
        }
    }
    lines
}

/// Cuts a triangle at a line. Pieces on both sides are returned as triangles. The cut point on
/// an edge does not depend on the direction of the edge, so neighbors agree.
fn split(t: [DVec2; 3], (n, d): Line) -> Vec<[DVec2; 3]> {
    let len = n.length();
    let eps = 1e-7 * len.max(1.0);
    let s: Vec<f64> = t
        .iter()
        .map(|p| {
            let v = n.dot(*p) - d;
            if v.abs() <= eps { 0.0 } else { v }
        })
        .collect();
    if s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0) {
        return vec![t];
    }
    let mut pos: Vec<DVec2> = Vec::new();
    let mut neg: Vec<DVec2> = Vec::new();
    for i in 0..3 {
        let j = (i + 1) % 3;
        let (p, q, sp, sq) = (t[i], t[j], s[i], s[j]);
        if sp >= 0.0 {
            pos.push(p);
        }
        if sp <= 0.0 {
            neg.push(p);
        }
        if (sp > 0.0 && sq < 0.0) || (sp < 0.0 && sq > 0.0) {
            // Same point from both directions.
            let (a, b, sa, sb) = if key(p) <= key(q) {
                (p, q, sp, sq)
            } else {
                (q, p, sq, sp)
            };
            let x = a + (b - a) * (sa / (sa - sb));
            pos.push(x);
            neg.push(x);
        }
    }
    let mut out = Vec::new();
    for poly in [pos, neg] {
        for k in 1..poly.len().saturating_sub(1) {
            let tri = [poly[0], poly[k], poly[k + 1]];
            let area2 = (tri[1] - tri[0]).perp_dot(tri[2] - tri[0]);
            if area2.abs() > 1e-9 {
                out.push(tri);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn rect(w: f64, d: f64) -> Vec<Vertex> {
        curve::corners(&[
            DVec2::ZERO,
            DVec2::new(w, 0.0),
            DVec2::new(w, d),
            DVec2::new(0.0, d),
        ])
    }

    fn block(roof: RoofStyle, w: f64, d: f64) -> Massing {
        let mut m = Massing::new(rect(w, d), 6000.0);
        m.roof = roof;
        m
    }

    #[test]
    fn flat_box_has_exact_volume() {
        let m = block(RoofStyle::Flat, 10_000.0, 8000.0);
        let s = m.solid(0.0, true);
        assert!(s.is_closed());
        assert!((s.volume() - 10_000.0 * 8000.0 * 6000.0).abs() < 1.0);
        let q = m.quantities();
        assert_eq!(q.floors, 2);
        assert!((q.gross_floor_area - 2.0 * 80.0e6).abs() < 1.0);
        assert!((q.roof_area - 80.0e6).abs() < 1.0);
        assert!((q.wall_area - 2.0 * (10_000.0 + 8000.0) * 6000.0).abs() < 1.0);
        assert_eq!(q.ridge_height, 6000.0);
    }

    #[test]
    fn gable_and_hip_volumes_match_formulas() {
        let (l, w) = (12_000.0_f64, 8000.0_f64);
        let t = 30.0_f64.to_radians().tan();
        let rise = w / 2.0 * t;
        let gable = block(RoofStyle::Gable, l, w);
        let sg = gable.solid(0.0, true);
        assert!(sg.is_closed());
        let want = l * w * 6000.0 + l * w * rise / 2.0;
        assert!((sg.volume() - want).abs() / want < 1e-9, "{}", sg.volume());
        assert!((gable.roof_rise() - rise).abs() < 1e-6);
        let hip = block(RoofStyle::Hip, l, w);
        let sh = hip.solid(0.0, true);
        assert!(sh.is_closed());
        let want = l * w * 6000.0 + w * rise * (3.0 * l - w) / 6.0;
        assert!((sh.volume() - want).abs() / want < 1e-9, "{}", sh.volume());
        // A square hip roof is a pyramid.
        let sq = block(RoofStyle::Hip, 6000.0, 6000.0);
        let want = 36.0e6 * 6000.0 + 36.0e6 * (3000.0 * t) / 3.0;
        assert!((sq.solid(0.0, true).volume() - want).abs() / want < 1e-9);
    }

    #[test]
    fn ridge_follows_the_long_side_or_the_given_angle() {
        let long_y = block(RoofStyle::Gable, 6000.0, 12_000.0);
        let ring = long_y.ring();
        let (_, u, a, b) = long_y.roof_frame(&ring);
        assert!(u.dot(DVec2::Y).abs() > 0.999 && a > b);
        let mut turned = block(RoofStyle::Gable, 6000.0, 12_000.0);
        turned.ridge_angle = Some(0.0);
        let (_, u, a, b) = turned.roof_frame(&turned.ring());
        assert!(u.dot(DVec2::X).abs() > 0.999 && a < b);
        assert!(turned.solid(0.0, true).is_closed());
    }

    #[test]
    fn curved_and_l_shaped_footprints_stay_closed() {
        let mut v = rect(10_000.0, 10_000.0);
        v[2].kind = curve::VertexKind::Arc { radius: 3000.0 };
        for roof in [RoofStyle::Flat, RoofStyle::Gable, RoofStyle::Hip] {
            let mut m = Massing::new(v.clone(), 3000.0);
            m.roof = roof;
            let s = m.solid(100.0, true);
            assert!(s.is_closed(), "{roof:?}");
            assert!(s.volume() > 0.0);
        }
        let l = curve::corners(&[
            DVec2::new(0.0, 0.0),
            DVec2::new(10_000.0, 0.0),
            DVec2::new(10_000.0, 4000.0),
            DVec2::new(4000.0, 4000.0),
            DVec2::new(4000.0, 9000.0),
            DVec2::new(0.0, 9000.0),
        ]);
        for roof in [RoofStyle::Gable, RoofStyle::Hip] {
            let mut m = Massing::new(l.clone(), 3000.0);
            m.roof = roof;
            m.pitch_deg = 20.0;
            let s = m.solid(0.0, true);
            assert!(s.is_closed(), "{roof:?}");
            // A non-convex footprint gets a flat roof: the volume is the exact prism.
            assert!((s.volume() - m.footprint_area() * 3000.0).abs() < 1.0);
            assert_eq!(m.effective_roof(), RoofStyle::Flat);
            assert!(m.roof_warning().unwrap().contains("not convex"));
            assert_eq!(m.roof_rise(), 0.0);
            assert!(roof_lines(&m).is_empty());
            assert_eq!(m.quantities().ridge_height, 3000.0);
        }
        let mut convex = Massing::new(l[..3].to_vec(), 3000.0);
        convex.roof = RoofStyle::Hip;
        assert!(convex.roof_warning().is_none());
    }

    #[test]
    fn floors_and_validation() {
        let mut m = block(RoofStyle::Flat, 5000.0, 5000.0);
        m.floors = Some(4);
        m.floor_height = 2800.0;
        assert_eq!(m.eave_height(), 11_200.0);
        assert_eq!(m.floor_count(), 4);
        assert!(m.problem().is_none());
        let mut bad = m.clone();
        bad.floors = Some(0);
        assert!(bad.problem().unwrap().contains("floors"));
        bad = m.clone();
        bad.pitch_deg = 90.0;
        assert!(bad.problem().unwrap().contains("pitch"));
        bad = m.clone();
        bad.floor_height = 0.0;
        assert!(bad.problem().is_some());
        bad = m.clone();
        bad.height = f64::NAN;
        assert!(!bad.is_finite() && bad.problem().is_some());
        bad = m.clone();
        bad.floors = None;
        bad.height = 0.0;
        assert!(bad.problem().is_some());
        bad = m.clone();
        bad.footprint.truncate(2);
        assert!(bad.problem().unwrap().contains("3"));
        bad = m.clone();
        bad.footprint = curve::corners(&[DVec2::ZERO, DVec2::X, DVec2::X * 2.0]);
        assert!(bad.problem().unwrap().contains("area"));
        let mut by_height = Massing::new(rect(1000.0, 1000.0), 7000.0);
        by_height.floor_height = 3000.0;
        assert_eq!(by_height.floor_count(), 2);
        assert_eq!(RoofStyle::from_name("Hipped"), Some(RoofStyle::Hip));
        assert_eq!(RoofStyle::from_name("gable").unwrap().name(), "gable");
        assert_eq!(RoofStyle::from_name("flat").unwrap().name(), "flat");
        assert_eq!(RoofStyle::Hip.name(), "hip");
        assert!(RoofStyle::from_name("dome").is_none());
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Massing>(&json).unwrap(), m);
        let minimal: Massing =
            serde_json::from_str(r#"{"footprint": [[0,0],[1000,0],[1000,1000]]}"#).unwrap();
        assert_eq!(minimal.height, DEFAULT_HEIGHT);
        assert_eq!(minimal.pitch_deg, DEFAULT_PITCH_DEG);
    }

    #[test]
    fn floor_face_is_optional_and_pitch_zero_is_flat() {
        let mut m = block(RoofStyle::Gable, 5000.0, 4000.0);
        let open = m.solid(0.0, false);
        assert!(!open.is_closed());
        m.pitch_deg = 0.0;
        assert_eq!(
            m.roof_at(DVec2::new(100.0, 100.0), &m.roof_frame(&m.ring())),
            0.0
        );
        assert!((m.solid(0.0, true).volume() - 5000.0 * 4000.0 * 6000.0).abs() < 1.0);
        let empty = Massing::new(vec![], 1000.0);
        assert!(empty.solid(0.0, true).tris.is_empty());
        assert_eq!(empty.roof_rise(), 0.0);
        assert_eq!(PI, std::f64::consts::PI);
    }

    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn rectangle_roofs_are_closed_with_exact_volume(
                w in 2000.0..40_000.0f64,
                d in 2000.0..40_000.0f64,
                h in 500.0..30_000.0f64,
                pitch in 1.0..70.0f64,
                roof in 0..3usize,
                angle in 0.0..PI,
            ) {
                let rot = DVec2::from_angle(angle);
                let ring: Vec<DVec2> = [
                    DVec2::ZERO, DVec2::new(w, 0.0), DVec2::new(w, d), DVec2::new(0.0, d)
                ].iter().map(|p| rot.rotate(*p)).collect();
                let mut m = Massing::new(curve::corners(&ring), h);
                m.roof = [RoofStyle::Flat, RoofStyle::Gable, RoofStyle::Hip][roof];
                m.pitch_deg = pitch;
                let s = m.solid(0.0, true);
                prop_assert!(s.is_closed());
                let (l, sh) = (w.max(d), w.min(d));
                let rise = sh / 2.0 * pitch.to_radians().tan();
                let extra = match m.roof {
                    RoofStyle::Flat => 0.0,
                    RoofStyle::Gable => l * sh * rise / 2.0,
                    RoofStyle::Hip => sh * rise * (3.0 * l - sh) / 6.0,
                };
                let want = w * d * h + extra;
                prop_assert!((s.volume() - want).abs() / want < 1e-6);
            }
        }
    }
}
