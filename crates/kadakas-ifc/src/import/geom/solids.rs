//! More solids of the IFC geometry: revolved areas, swept disks (pipes), CSG primitives and the
//! advanced boundary representation. The meshes are made by plain functions ([`revolve`], [`pipe`],
//! [`sphere`], ...) that know nothing of IFC; the methods of [`Eval`] read the entities and call them.
//! All meshes are closed and face outward.

use std::f64::consts::{PI, TAU};

use glam::{DAffine3, DQuat, DVec2, DVec3};
use kadakas_core::curve;

use super::{Eval, MeshData, Piece, Region, clean};
use crate::import::ctx::{attr, num};
use crate::step::Entity;

/// The least tolerance of a curved solid, as a share of its radius (0.02%).
pub const RELATIVE_TOLERANCE: f64 = 2.0e-4;

/// The most triangles of one solid that the import makes (a sphere of 256 sides has 65,000).
/// The work (see `kadakas_step::StepLimits::max_work`) and the triangles of one face that an advanced
/// brep of a product may take.
const BREP_MAX_WORK: u64 = 12_000_000;
const BREP_MAX_FACE_TRIANGLES: usize = 50_000;

pub const MAX_SOLID_TRIANGLES: usize = 400_000;

/// The fewest and most corners of a circle.
const MIN_SIDES: usize = 12;
const MAX_SIDES: usize = 256;

/// Corners for an arc of `angle` radians and radius `r` so that no chord is `tol` away from the arc.
fn sides_for(r: f64, angle: f64, tol: f64) -> usize {
    if !(r > 0.0 && tol > 0.0 && angle > 0.0) {
        return MIN_SIDES;
    }
    // The tolerance is the given one or 0.02% of the radius, whichever is larger: a solid of 300 m
    // does not need chords of 2 mm.
    let tol = tol.max(r * RELATIVE_TOLERANCE);
    let step = 2.0 * (1.0 - (tol / r).min(1.0)).acos();
    let n = if step > 1e-9 {
        (angle / step).ceil()
    } else {
        1e9
    };
    let min = (MIN_SIDES as f64 * angle / TAU).ceil().max(2.0);
    (n.max(min) as usize).clamp(2, MAX_SIDES)
}

fn signed_volume(m: &MeshData) -> f64 {
    m.t.iter()
        .map(|t| {
            let (a, b, c) = (m.v[t[0] as usize], m.v[t[1] as usize], m.v[t[2] as usize]);
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

/// Turns the mesh inside out if its volume is negative; welds the points that coincide.
fn finish(mut m: MeshData, weld: f64) -> MeshData {
    m.weld(weld);
    if signed_volume(&m) < 0.0 {
        m.flip();
    }
    m
}

/// Pushes the triangle `a b c` so that it faces `want` (flips it when it does not).
fn tri_facing(m: &mut MeshData, a: u32, b: u32, c: u32, want: DVec3) {
    let n = (m.v[b as usize] - m.v[a as usize]).cross(m.v[c as usize] - m.v[a as usize]);
    if n.length_squared() < 1e-30 {
        return;
    }
    if n.dot(want) >= 0.0 {
        m.t.push([a, b, c]);
    } else {
        m.t.push([a, c, b]);
    }
}

/// The rings of a region as points: the outline counterclockwise, the holes clockwise.
fn rings(r: &Region, tol: f64) -> Option<(Vec<DVec2>, Vec<Vec<DVec2>>)> {
    let flat = |vs: &[curve::Vertex]| clean(&curve::flatten(vs, true, tol.max(1e-9)), true);
    let outer = flat(&r.outer);
    if outer.len() < 3 {
        return None;
    }
    let holes = r
        .holes
        .iter()
        .map(|h| flat(h))
        .filter(|h| h.len() >= 3)
        .collect();
    Some((outer, holes))
}

/// The triangles of a region (with its holes cut out) at a place: `place` maps the plane points.
fn cap(
    m: &mut MeshData,
    outer: &[DVec2],
    holes: &[Vec<DVec2>],
    place: &dyn Fn(DVec2) -> DVec3,
    want: DVec3,
) {
    for piece in super::split_counted(outer, holes) {
        let base = m.v.len() as u32;
        m.v.extend(piece.iter().map(|p| place(*p)));
        for [a, b, c] in kadakas_core::mesh::triangulate_edge_exact(&piece) {
            tri_facing(m, base + a as u32, base + b as u32, base + c as u32, want);
        }
    }
}

/// The solid that the regions make when they turn about the line through `axis_o` along `axis_d` by
/// `angle` radians. The regions lie in the plane Z = 0. `None` when the axis is not a direction.
pub fn revolve(
    regions: &[Region],
    axis_o: DVec3,
    axis_d: DVec3,
    angle: f64,
    tol: f64,
) -> Option<MeshData> {
    let a = axis_d.try_normalize()?;
    if !(angle.is_finite() && angle > 1e-9) {
        return None;
    }
    let theta = angle.min(TAU);
    let full = theta >= TAU - 1e-9;
    let mut m = MeshData::default();
    for region in regions {
        let Some((outer, holes)) = rings(region, tol) else {
            continue;
        };
        let rmax = outer
            .iter()
            .map(|p| {
                let v = p.extend(0.0) - axis_o;
                (v - a * v.dot(a)).length()
            })
            .fold(0.0, f64::max);
        let n = sides_for(rmax, theta, tol);
        let at =
            |p: DVec2, ang: f64| axis_o + DQuat::from_axis_angle(a, ang) * (p.extend(0.0) - axis_o);
        if !full {
            // The profile moves along `motion`; the start cap faces against it and the end cap along it.
            let c = outer.iter().fold(DVec2::ZERO, |s, p| s + *p) / outer.len() as f64;
            let motion = a.cross(c.extend(0.0) - axis_o);
            if motion.length_squared() > 1e-18 {
                let s = if motion.z >= 0.0 { 1.0 } else { -1.0 };
                cap(
                    &mut m,
                    &outer,
                    &holes,
                    &|p| at(p, 0.0),
                    DVec3::new(0.0, 0.0, -s),
                );
                // The end cap faces along the motion at the end angle: Z turned with the profile.
                let z_end = DQuat::from_axis_angle(a, theta) * DVec3::Z;
                cap(&mut m, &outer, &holes, &|p| at(p, theta), z_end * s);
            }
        }
        let mut all: Vec<&Vec<DVec2>> = vec![&outer];
        let reversed: Vec<Vec<DVec2>> = holes
            .iter()
            .map(|h| h.iter().rev().copied().collect())
            .collect();
        all.extend(reversed.iter());
        for ring in all {
            let k = ring.len();
            for i in 0..k {
                let (p, q) = (ring[i], ring[(i + 1) % k]);
                let d = q - p;
                if d.length_squared() < 1e-24 {
                    continue;
                }
                // Outward of the solid on this edge, in the plane of the profile.
                let out2 = DVec3::new(d.y, -d.x, 0.0);
                for j in 0..n {
                    let (a0, a1) = (
                        theta * j as f64 / n as f64,
                        theta * (j + 1) as f64 / n as f64,
                    );
                    let base = m.v.len() as u32;
                    m.v.extend([at(p, a0), at(q, a0), at(q, a1), at(p, a1)]);
                    let want = DQuat::from_axis_angle(a, (a0 + a1) / 2.0) * out2;
                    tri_facing(&mut m, base, base + 1, base + 2, want);
                    tri_facing(&mut m, base, base + 2, base + 3, want);
                }
            }
        }
    }
    (!m.t.is_empty()).then(|| finish(m, 1e-9 * (1.0 + axis_o.length())))
}

/// A tube of radius `r` (and inner radius `inner` when it is hollow) along the path. The corners of
/// the path are mitered, so the volume is that of the cylinders along the segments. A path whose
/// first and last points are the same is closed.
pub fn pipe(path: &[DVec3], r: f64, inner: Option<f64>, tol: f64) -> Option<MeshData> {
    let mut pts: Vec<DVec3> = Vec::with_capacity(path.len());
    for p in path {
        if !p.is_finite() {
            return None;
        }
        if pts.last().is_none_or(|l| l.distance(*p) > 1e-9 * (1.0 + r)) {
            pts.push(*p);
        }
    }
    let closed = pts.len() > 3 && pts[0].distance(pts[pts.len() - 1]) <= 1e-9 * (1.0 + r);
    if closed {
        pts.pop();
    }
    if pts.len() < 2
        || r.is_nan()
        || r <= 0.0
        || inner.is_some_and(|i| i.is_nan() || i <= 0.0 || i >= r)
    {
        return None;
    }
    let n = pts.len();
    let segments = if closed { n } else { n - 1 };
    let tangent = |s: usize| (pts[(s + 1) % n] - pts[s]).normalize();
    // The frame of each segment: the first one is any normal, the next ones turn with the path.
    let t0 = tangent(0);
    let mut u = t0.any_orthonormal_vector();
    let mut frames: Vec<(DVec3, DVec3)> = Vec::with_capacity(segments);
    for s in 0..segments {
        let t = tangent(s);
        if s > 0 {
            let prev = tangent(s - 1);
            let axis = prev.cross(t);
            if axis.length() > 1e-12 {
                let ang = prev.dot(t).clamp(-1.0, 1.0).acos();
                u = DQuat::from_axis_angle(axis.normalize(), ang) * u;
            }
        }
        u = (u - t * u.dot(t))
            .try_normalize()
            .unwrap_or_else(|| t.any_orthonormal_vector());
        frames.push((u, t.cross(u)));
    }
    let k = sides_for(r, TAU, tol / 4.0).max(MIN_SIDES);
    let mut m = MeshData::default();
    // The ring of one radius at a joint, seen from the segment `s` (projected on the miter plane).
    let ring_at = |joint: usize, s: usize, radius: f64| -> Vec<DVec3> {
        let (u, v) = frames[s];
        let t = tangent(s);
        let before = if closed || joint > 0 {
            Some(tangent((joint + segments - 1) % segments))
        } else {
            None
        };
        let after = if closed || joint < segments {
            Some(tangent(joint % segments))
        } else {
            None
        };
        let normal = match (before, after) {
            (Some(b), Some(a)) => (b + a).try_normalize().unwrap_or(t),
            (Some(b), None) => b,
            (None, Some(a)) => a,
            _ => t,
        };
        (0..k)
            .map(|i| {
                let ang = TAU * i as f64 / k as f64;
                let w = (u * ang.cos() + v * ang.sin()) * radius;
                let denom = t.dot(normal);
                let lambda = if denom.abs() > 1e-9 {
                    -w.dot(normal) / denom
                } else {
                    0.0
                };
                pts[joint % n] + w + t * lambda
            })
            .collect()
    };
    let radii: Vec<(f64, bool)> = match inner {
        Some(i) => vec![(r, true), (i, false)],
        None => vec![(r, true)],
    };
    for (radius, outward) in &radii {
        // Rings: the start of each segment seen from the segment before it (a mitered joint).
        let mut rings: Vec<Vec<DVec3>> = Vec::new();
        for j in 0..=segments {
            let s_for = if j == 0 { 0 } else { j - 1 };
            let s_for = if closed && j == segments {
                segments - 1
            } else {
                s_for
            };
            rings.push(ring_at(j, s_for.min(segments - 1), *radius));
        }
        if closed {
            // The last ring is the first one.
            let first = rings[0].clone();
            let last = rings.len() - 1;
            rings[last] = first;
        }
        for s in 0..segments {
            let base = m.v.len() as u32;
            m.v.extend(rings[s].iter().copied());
            m.v.extend(rings[s + 1].iter().copied());
            let kk = k as u32;
            for i in 0..kk {
                let j = (i + 1) % kk;
                let mid = pts[s] + (pts[(s + 1) % n] - pts[s]) / 2.0;
                let c = (m.v[(base + i) as usize]
                    + m.v[(base + j) as usize]
                    + m.v[(base + kk + i) as usize]
                    + m.v[(base + kk + j) as usize])
                    / 4.0;
                let out = (c - mid).normalize_or_zero() * if *outward { 1.0 } else { -1.0 };
                tri_facing(&mut m, base + i, base + j, base + kk + j, out);
                tri_facing(&mut m, base + i, base + kk + j, base + kk + i, out);
            }
        }
    }
    if !closed {
        // End caps: a disk, or an annulus.
        for (end, ring_idx) in [(0usize, 0usize), (1, segments)] {
            let want = if end == 0 {
                -tangent(0)
            } else {
                tangent(segments - 1)
            };
            let outer = ring_at(ring_idx, if end == 0 { 0 } else { segments - 1 }, r);
            let hole = inner.map(|i| ring_at(ring_idx, if end == 0 { 0 } else { segments - 1 }, i));
            let base = m.v.len() as u32;
            m.v.extend(outer.iter().copied());
            let kk = k as u32;
            match hole {
                None => {
                    let centre = m.v.len() as u32;
                    m.v.push(pts[if end == 0 { 0 } else { n - 1 }]);
                    for i in 0..kk {
                        tri_facing(&mut m, centre, base + i, base + (i + 1) % kk, want);
                    }
                }
                Some(h) => {
                    let hb = m.v.len() as u32;
                    m.v.extend(h.iter().copied());
                    for i in 0..kk {
                        let j = (i + 1) % kk;
                        tri_facing(&mut m, base + i, base + j, hb + j, want);
                        tri_facing(&mut m, base + i, hb + j, hb + i, want);
                    }
                }
            }
        }
    }
    Some(finish(m, 1e-9 * (1.0 + r)))
}

/// The triangles that [`sphere`] makes, counted before it is made.
pub fn sphere_triangles(r: f64, tol: f64) -> usize {
    let n = sides_for(r, TAU, tol).max(MIN_SIDES);
    n * (n / 2).max(6) * 2
}

/// The triangles that [`cone_frustum`] makes.
pub fn frustum_triangles(bottom: f64, top: f64, tol: f64) -> usize {
    sides_for(bottom.max(top), TAU, tol).max(MIN_SIDES) * 4
}

/// The triangles that [`revolve`] makes: for each edge of the rings, two for each step of the turn.
pub fn revolve_triangles(regions: &[Region], axis_o: DVec3, angle: f64, tol: f64) -> usize {
    let theta = angle.clamp(0.0, TAU);
    let mut total = 0usize;
    for region in regions {
        let Some((outer, holes)) = rings(region, tol) else {
            continue;
        };
        let rmax = outer
            .iter()
            .map(|p| (p.extend(0.0) - axis_o).length())
            .fold(0.0, f64::max);
        let n = sides_for(rmax, theta, tol);
        let edges: usize = outer.len() + holes.iter().map(Vec::len).sum::<usize>();
        total = total.saturating_add(edges.saturating_mul(n).saturating_mul(2));
        total = total.saturating_add(edges * 2);
    }
    total
}

/// The triangles that [`pipe`] makes.
pub fn pipe_triangles(points: usize, r: f64, hollow: bool, tol: f64) -> usize {
    let k = sides_for(r, TAU, tol / 4.0).max(MIN_SIDES);
    points.saturating_sub(1).max(1) * k * 2 * if hollow { 2 } else { 1 } + k * 4
}

/// A box with a corner at the origin that reaches `d` (the `IfcBlock`).
pub fn block(d: DVec3) -> MeshData {
    MeshData::cuboid(DVec3::ZERO, d)
}

/// A sphere of radius `r` about the origin.
pub fn sphere(r: f64, tol: f64) -> MeshData {
    let n = sides_for(r, TAU, tol).max(MIN_SIDES);
    let rows = (n / 2).max(6);
    let mut m = MeshData::default();
    m.v.push(DVec3::new(0.0, 0.0, r));
    for i in 1..rows {
        let phi = PI * i as f64 / rows as f64;
        for j in 0..n {
            let th = TAU * j as f64 / n as f64;
            m.v.push(DVec3::new(
                r * phi.sin() * th.cos(),
                r * phi.sin() * th.sin(),
                r * phi.cos(),
            ));
        }
    }
    m.v.push(DVec3::new(0.0, 0.0, -r));
    let south = (m.v.len() - 1) as u32;
    let ring = |i: usize, j: usize| 1 + ((i - 1) * n + j % n) as u32;
    for j in 0..n {
        m.t.push([0, ring(1, j), ring(1, j + 1)]);
        m.t.push([south, ring(rows - 1, j + 1), ring(rows - 1, j)]);
    }
    for i in 1..rows - 1 {
        for j in 0..n {
            m.t.push([ring(i, j), ring(i + 1, j), ring(i + 1, j + 1)]);
            m.t.push([ring(i, j), ring(i + 1, j + 1), ring(i, j + 1)]);
        }
    }
    finish(m, 1e-12)
}

/// A cylinder (`top` = the radius at the top 0 for a cone) along Z from 0 to `h`.
pub fn cone_frustum(bottom: f64, top: f64, h: f64, tol: f64) -> MeshData {
    let n = sides_for(bottom.max(top), TAU, tol).max(MIN_SIDES);
    let mut m = MeshData::default();
    let ring = |m: &mut MeshData, r: f64, z: f64| -> u32 {
        let base = m.v.len() as u32;
        for j in 0..n {
            let th = TAU * j as f64 / n as f64;
            m.v.push(DVec3::new(r * th.cos(), r * th.sin(), z));
        }
        base
    };
    let (b, t) = (ring(&mut m, bottom, 0.0), ring(&mut m, top, h));
    let nn = n as u32;
    for j in 0..nn {
        let k = (j + 1) % nn;
        m.t.push([b + j, b + k, t + k]);
        m.t.push([b + j, t + k, t + j]);
    }
    let cb = m.v.len() as u32;
    m.v.push(DVec3::ZERO);
    let ct = cb + 1;
    m.v.push(DVec3::new(0.0, 0.0, h));
    for j in 0..nn {
        let k = (j + 1) % nn;
        m.t.push([cb, b + k, b + j]);
        m.t.push([ct, t + j, t + k]);
    }
    finish(m, 1e-12)
}

/// A pyramid on the rectangle [0, x] by [0, y] with the apex above its middle.
pub fn pyramid(x: f64, y: f64, h: f64) -> MeshData {
    let v = vec![
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(x, 0.0, 0.0),
        DVec3::new(x, y, 0.0),
        DVec3::new(0.0, y, 0.0),
        DVec3::new(x / 2.0, y / 2.0, h),
    ];
    let t = vec![
        [0, 2, 1],
        [0, 3, 2],
        [0, 1, 4],
        [1, 2, 4],
        [2, 3, 4],
        [3, 0, 4],
    ];
    finish(MeshData { v, t, exact: None }, 1e-12)
}

// How many times a solid was read by the STEP reader (tests count it).
#[cfg(test)]
thread_local! {
    pub(crate) static BREP_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Eval<'_> {
    /// True if a solid of about `triangles` triangles may be made: it must fit what is left of the
    /// triangle limit of the import and the limit for one solid ([`MAX_SOLID_TRIANGLES`]). The work is
    /// counted before the solid is made, so a small file cannot ask for gigabytes.
    fn allow(&mut self, what: &str, triangles: usize) -> bool {
        let left = self
            .max_tris
            .saturating_sub(self.tris)
            .min(MAX_SOLID_TRIANGLES);
        if triangles > left {
            self.issue(format!(
                "{what} would need about {triangles} triangles, more than the {left} that are allowed: it is not made (the sizes may be in the wrong unit)"
            ));
            return false;
        }
        true
    }

    fn placement_of(&self, v: &crate::step::Value) -> DAffine3 {
        self.cx
            .get(v)
            .and_then(|p| self.cx.axis3(&p))
            .unwrap_or(DAffine3::IDENTITY)
    }

    /// `IfcRevolvedAreaSolid(SweptArea, Position, Axis, Angle)`.
    pub(super) fn revolved(&mut self, e: &Entity, xf: &DAffine3) {
        let Some(regions) = self.profile(attr(e, 0).as_ref_id().unwrap_or(0)) else {
            return;
        };
        let pos = self.placement_of(attr(e, 1));
        // The axis is an IfcAxis1Placement in the frame of the profile.
        let Some(ax) = self.cx.get(attr(e, 2)) else {
            self.issue("a revolved solid has no axis");
            return;
        };
        let o = self.cx.point(attr(&ax, 0)).unwrap_or(DVec3::ZERO);
        let d = self.cx.direction(attr(&ax, 1)).unwrap_or(DVec3::X);
        let angle = num(attr(e, 3)).unwrap_or(0.0) * self.cx.units.angle_rad;
        let est = revolve_triangles(&regions, DVec3::new(o.x, o.y, 0.0), angle, self.tol);
        if !self.allow("a revolved solid", est) {
            return;
        }
        match revolve(&regions, DVec3::new(o.x, o.y, 0.0), d, angle, self.tol) {
            Some(mut m) => {
                m.transform(&(*xf * pos));
                self.add_mesh(m);
            }
            None => self.issue("a revolved solid has no axis, no angle or no area"),
        }
    }

    /// `IfcSweptDiskSolid(Directrix, Radius, InnerRadius, StartParam, EndParam)`.
    pub(super) fn swept_disk(&mut self, e: &Entity, xf: &DAffine3) {
        let Some(curve_id) = attr(e, 0).as_ref_id() else {
            return;
        };
        let (Some(r), inner) = (num(attr(e, 1)), num(attr(e, 2))) else {
            self.issue("a swept disk has no radius");
            return;
        };
        let at = self.pieces.len();
        self.curve_piece(curve_id, &DAffine3::IDENTITY);
        let Some(Piece::Curve(c)) = self.pieces.get(at).cloned() else {
            self.pieces.truncate(at);
            self.issue("the directrix of a swept disk is not supported");
            return;
        };
        self.pieces.truncate(at);
        let mut pts = c.pts.clone();
        if c.closed && pts.len() > 2 {
            pts.push(pts[0]);
        }
        if (num(attr(e, 3)).is_some() || num(attr(e, 4)).is_some()) && !self.is_polyline(curve_id) {
            self.issue("the start and end parameters of a swept disk on a curved directrix are not applied: the whole curve is swept");
        } else if let (Some(s), Some(t)) = (num(attr(e, 3)), num(attr(e, 4))) {
            pts = trim_polyline(&pts, s, t);
        }
        if !self.allow(
            "a swept disk",
            pipe_triangles(pts.len(), r, inner.is_some(), self.tol),
        ) {
            return;
        }
        match pipe(&pts, r, inner, self.tol) {
            Some(mut m) => {
                m.transform(xf);
                self.add_mesh(m);
            }
            None => self.issue("a swept disk has no usable directrix or radius"),
        }
    }

    fn is_polyline(&self, id: u64) -> bool {
        self.cx
            .ent(id)
            .is_some_and(|c| c.name == "IFCPOLYLINE" || c.name == "IFCINDEXEDPOLYCURVE")
    }

    /// `IfcBlock`, `IfcSphere`, `IfcRightCircularCylinder`, `IfcRightCircularCone` and `IfcRectangularPyramid`.
    pub(super) fn primitive(&mut self, e: &Entity, xf: &DAffine3) {
        let pos = self.placement_of(attr(e, 0));
        let n = |i: usize| num(attr(e, i)).filter(|v| v.is_finite() && *v > 0.0);
        let est = match e.name.as_str() {
            "IFCSPHERE" => n(1).map_or(0, |r| sphere_triangles(r, self.tol)),
            "IFCRIGHTCIRCULARCYLINDER" => n(2).map_or(0, |r| frustum_triangles(r, r, self.tol)),
            "IFCRIGHTCIRCULARCONE" => n(2).map_or(0, |r| frustum_triangles(r, 0.0, self.tol)),
            _ => 0,
        };
        if !self.allow(&e.name, est) {
            return;
        }
        let mesh = match e.name.as_str() {
            "IFCBLOCK" => n(1)
                .zip(n(2))
                .zip(n(3))
                .map(|((x, y), z)| block(DVec3::new(x, y, z))),
            "IFCSPHERE" => n(1).map(|r| sphere(r, self.tol)),
            "IFCRIGHTCIRCULARCYLINDER" => {
                n(1).zip(n(2)).map(|(h, r)| cone_frustum(r, r, h, self.tol))
            }
            "IFCRIGHTCIRCULARCONE" => n(1)
                .zip(n(2))
                .map(|(h, r)| cone_frustum(r, 0.0, h, self.tol)),
            "IFCRECTANGULARPYRAMID" => n(1).zip(n(2)).zip(n(3)).map(|((x, y), h)| pyramid(x, y, h)),
            _ => None,
        };
        match mesh {
            Some(mut m) => {
                m.transform(&(*xf * pos));
                self.add_mesh(m);
            }
            None => self.issue(format!(
                "{} has a size that is not a positive number",
                e.name
            )),
        }
    }

    /// `IfcAdvancedBrep` and `IfcAdvancedBrepWithVoids` through the STEP reader (see `advanced`). A
    /// solid that many products use (a mapped type, a repeated item) is read once: the mesh comes
    /// from the cache of the import, and every copy is placed and counted by `add_mesh`.
    pub(super) fn advanced_brep(&mut self, e: &Entity, xf: &DAffine3) {
        let cached = self.brep_cache.and_then(|c| c.get(e.id));
        let read = match cached {
            Some(r) => r,
            None => {
                let r = std::rc::Rc::new(self.read_brep(e));
                if let Some(c) = self.brep_cache {
                    c.insert(e.id, r.clone());
                }
                r
            }
        };
        for why in &read.1 {
            self.issue(why.clone());
        }
        if let Some(m) = &read.0 {
            let mut m = m.clone();
            m.transform(xf);
            self.add_mesh(m);
        }
    }

    /// Reads the solid in its own frame (file units). The sentences go with the mesh, so a copy of
    /// the solid reports the same.
    fn read_brep(&self, e: &Entity) -> (Option<MeshData>, Vec<String>) {
        #[cfg(test)]
        BREP_READS.with(|c| c.set(c.get() + 1));
        let mut issues: Vec<String> = Vec::new();
        let bytes = match crate::import::advanced::to_step(self.cx, e) {
            Ok(b) => b,
            Err(why) => {
                issues.push(format!("unsupported geometry {} ({why})", e.name));
                return (None, issues);
            }
        };
        let opts = kadakas_step::StepOptions {
            unit: Some(kadakas_meshio::Unit::Millimetre),
            tolerance_mm: Some(self.tol),
            keep_exact: true,
            step: kadakas_step::StepLimits {
                // A product is a part of a building: the work of one B-rep is bounded far below
                // the limit of a whole STEP file (a file without units makes the sizes 1000 times
                // too big, and the tolerance in millimeters then asks for too many triangles).
                max_work: BREP_MAX_WORK,
                max_face_triangles: BREP_MAX_FACE_TRIANGLES,
                ..Default::default()
            },
            ..Default::default()
        };
        let imp = match kadakas_step::read(&bytes, &opts) {
            Ok(imp) => imp,
            Err(err) => {
                issues.push(format!("unsupported geometry {} ({err})", e.name));
                return (None, issues);
            }
        };
        let mut m = MeshData::default();
        for part in &imp.set.parts {
            let base = m.v.len() as u32;
            m.v.extend(part.positions.iter().map(|p| DVec3::from(*p)));
            m.t.extend(
                part.triangles
                    .iter()
                    .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
            );
        }
        let faces = |name: &str| name == "ADVANCED_FACE" || name == "FACE";
        let left_out: u64 = imp
            .report
            .skipped
            .iter()
            .filter(|(name, _, _)| faces(name))
            .map(|(_, _, n)| *n)
            .sum();
        if left_out > 0 {
            let why = imp
                .report
                .skipped
                .iter()
                .find(|(name, _, _)| faces(name))
                .map(|(n, r, _)| format!("{n}: {r}"))
                .unwrap_or_default();
            issues.push(format!(
                "{left_out} faces of an advanced brep were left out ({why})"
            ));
        }
        if m.t.is_empty() {
            let why = imp
                .report
                .skipped
                .first()
                .map(|(n, r, _)| format!(": {n}: {r}"))
                .unwrap_or_default();
            issues.push(format!("unsupported geometry {}{why}", e.name));
            return (None, issues);
        }
        // The exact surfaces of all parts, when every part has them.
        let mut exact: Option<kadakas_core::exact::ExactSolid> = None;
        if !imp.exact.is_empty() && imp.exact.iter().all(Option::is_some) {
            for x in imp.exact.iter().flatten() {
                match &mut exact {
                    Some(have) => have.merge(x),
                    None => exact = Some(x.clone()),
                }
            }
        }
        m.exact = exact.map(Box::new);
        (Some(m), issues)
    }

    /// True if a shell, a brep or a surface model has an advanced face or a face with an edge loop
    /// (a face of a faceted solid has polygon loops).
    pub(super) fn has_curved_faces(&self, e: &Entity) -> bool {
        let mut shells: Vec<std::rc::Rc<Entity>> = Vec::new();
        match e.name.as_str() {
            "IFCCLOSEDSHELL" | "IFCOPENSHELL" | "IFCCONNECTEDFACESET" => {
                return self.shell_has_curved_faces(e);
            }
            "IFCSHELLBASEDSURFACEMODEL" | "IFCFACEBASEDSURFACEMODEL" => {
                shells.extend(self.cx.get_all(attr(e, 0)));
            }
            _ => shells.extend(self.cx.get(attr(e, 0))),
        }
        shells.iter().any(|s| self.shell_has_curved_faces(s))
    }

    fn shell_has_curved_faces(&self, shell: &Entity) -> bool {
        self.cx.get_all(attr(shell, 0)).iter().any(|f| {
            f.name == "IFCADVANCEDFACE"
                || self.cx.get_all(attr(f, 0)).iter().any(|b| {
                    self.cx
                        .get(attr(b, 0))
                        .is_some_and(|l| l.name == "IFCEDGELOOP")
                })
        })
    }

    /// `IfcCsgSolid(TreeRootExpression)`: the tree is a boolean result or a primitive.
    pub(super) fn csg_solid(&mut self, e: &Entity, xf: &DAffine3, depth: usize) {
        if let Some(root) = attr(e, 0).as_ref_id() {
            self.item(root, xf, depth + 1);
        }
    }
}

/// The part of a polyline between the parameters `s` and `t` (the parameter of a point is its index).
fn trim_polyline(pts: &[DVec3], s: f64, t: f64) -> Vec<DVec3> {
    let last = (pts.len() - 1) as f64;
    let (s, t) = (s.clamp(0.0, last), t.clamp(0.0, last));
    if t <= s {
        return pts.to_vec();
    }
    let at = |u: f64| {
        let i = (u.floor() as usize).min(pts.len() - 2);
        pts[i].lerp(pts[i + 1], u - i as f64)
    };
    let mut out = vec![at(s)];
    let mut i = s.floor() as usize + 1;
    while (i as f64) < t {
        out.push(pts[i]);
        i += 1;
    }
    out.push(at(t));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use curve::Vertex;

    fn region(w: f64, h: f64, x0: f64) -> Region {
        Region {
            outer: vec![
                Vertex::corner(DVec2::new(x0, 0.0)),
                Vertex::corner(DVec2::new(x0 + w, 0.0)),
                Vertex::corner(DVec2::new(x0 + w, h)),
                Vertex::corner(DVec2::new(x0, h)),
            ],
            holes: vec![],
        }
    }

    fn vol(m: &MeshData) -> f64 {
        signed_volume(m)
    }

    #[test]
    fn a_rectangle_turned_a_full_circle_is_a_ring() {
        // 100 x 50 rectangle at distance 200 from the axis (Y): a ring with volume pi (r2^2 - r1^2) h.
        let m = revolve(
            &[region(100.0, 50.0, 200.0)],
            DVec3::ZERO,
            DVec3::Y,
            TAU,
            0.2,
        )
        .unwrap();
        let want = PI * (300.0f64.powi(2) - 200.0f64.powi(2)) * 50.0;
        assert!(
            ((vol(&m) - want) / want).abs() < 0.005,
            "{} {want}",
            vol(&m)
        );
        assert!(crate::import::csg::is_closed(&m));
    }

    #[test]
    fn an_i_shape_turned_follows_the_theorem_of_pappus() {
        // IPE 600 in metres, turned 87 degrees about an axis 7.25 m away. The tolerance is fine, so
        // that the flat pieces of the fillets (24 mm) do not add area.
        let (w, d, tw, tf, fr) = (0.22, 0.6, 0.012, 0.019, 0.024);
        let (hw, hd, ht) = (w / 2.0, d / 2.0, tw / 2.0);
        let pts = [
            (-hw, -hd),
            (hw, -hd),
            (hw, -hd + tf),
            (ht, -hd + tf),
            (ht, hd - tf),
            (hw, hd - tf),
            (hw, hd),
            (-hw, hd),
            (-hw, hd - tf),
            (-ht, hd - tf),
            (-ht, -hd + tf),
            (-hw, -hd + tf),
        ];
        let outer = pts
            .iter()
            .enumerate()
            .map(|(i, (x, y))| {
                let kind = if matches!(i, 3 | 4 | 9 | 10) {
                    curve::VertexKind::Arc { radius: fr }
                } else {
                    curve::VertexKind::Corner
                };
                Vertex::new(DVec2::new(*x, *y), kind)
            })
            .collect();
        let region = Region {
            outer,
            holes: vec![],
        };
        let area = 2.0 * w * tf + (d - 2.0 * tf) * tw + (4.0 - PI) * fr * fr;
        let theta = 1.52202550844946;
        let m = revolve(
            &[region],
            DVec3::new(7.25, 0.0, 0.0),
            DVec3::Y,
            theta,
            0.0001,
        )
        .unwrap();
        let want = area * 7.25 * theta;
        assert!(
            ((vol(&m) - want) / want).abs() < 0.002,
            "{} {want}",
            vol(&m)
        );
        assert!(crate::import::csg::is_closed(&m));
    }

    #[test]
    fn a_partial_turn_has_caps() {
        // A quarter turn about the Y axis.
        let m = revolve(
            &[region(100.0, 50.0, 200.0)],
            DVec3::ZERO,
            DVec3::Y,
            PI / 2.0,
            0.2,
        )
        .unwrap();
        let want = PI / 4.0 * (300.0f64.powi(2) - 200.0f64.powi(2)) * 50.0;
        assert!(
            ((vol(&m) - want) / want).abs() < 0.005,
            "{} {want}",
            vol(&m)
        );
        assert!(crate::import::csg::is_closed(&m));
        // Turning the other way (axis reversed) is the same solid.
        let m2 = revolve(
            &[region(100.0, 50.0, 200.0)],
            DVec3::ZERO,
            -DVec3::Y,
            PI / 2.0,
            0.2,
        )
        .unwrap();
        assert!((vol(&m2) - vol(&m)).abs() < 1e-6 * vol(&m));
        assert!(crate::import::csg::is_closed(&m2));
        assert!(revolve(&[], DVec3::ZERO, DVec3::Y, 1.0, 0.2).is_none());
        assert!(revolve(&[region(1.0, 1.0, 1.0)], DVec3::ZERO, DVec3::ZERO, 1.0, 0.2).is_none());
        assert!(revolve(&[region(1.0, 1.0, 1.0)], DVec3::ZERO, DVec3::Y, 0.0, 0.2).is_none());
    }

    #[test]
    fn a_profile_that_touches_the_axis_is_a_solid_of_revolution() {
        // A rectangle from the axis out to 50, 100 high turned about Z... the profile lies in Z = 0, so turn about Y.
        let m = revolve(&[region(50.0, 100.0, 0.0)], DVec3::ZERO, DVec3::Y, TAU, 0.1).unwrap();
        let want = PI * 50.0 * 50.0 * 100.0;
        assert!(
            ((vol(&m) - want) / want).abs() < 0.005,
            "{} {want}",
            vol(&m)
        );
        assert!(crate::import::csg::is_closed(&m));
    }

    #[test]
    fn pipes_have_the_volume_of_their_cylinders() {
        let r = 25.0;
        let straight = pipe(&[DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0)], r, None, 0.5).unwrap();
        let want = PI * r * r * 1000.0;
        assert!(
            ((vol(&straight) - want) / want).abs() < 0.01,
            "{}",
            vol(&straight)
        );
        assert!(crate::import::csg::is_closed(&straight));
        // A bent pipe: the corners are mitered, so the volume is that of the two cylinders.
        let bent = pipe(
            &[
                DVec3::ZERO,
                DVec3::new(1000.0, 0.0, 0.0),
                DVec3::new(1000.0, 500.0, 300.0),
                DVec3::new(1000.0, 500.0, 1300.0),
            ],
            r,
            None,
            0.5,
        )
        .unwrap();
        let len = 1000.0 + (500.0f64.powi(2) + 300.0f64.powi(2)).sqrt() + 1000.0;
        let want = PI * r * r * len;
        assert!(
            ((vol(&bent) - want) / want).abs() < 0.01,
            "{} {want}",
            vol(&bent)
        );
        assert!(crate::import::csg::is_closed(&bent));
        // A hollow pipe.
        let hollow = pipe(
            &[DVec3::ZERO, DVec3::new(0.0, 0.0, 800.0)],
            30.0,
            Some(20.0),
            0.2,
        )
        .unwrap();
        let want = PI * (30.0f64.powi(2) - 20.0f64.powi(2)) * 800.0;
        assert!(
            ((vol(&hollow) - want) / want).abs() < 0.01,
            "{}",
            vol(&hollow)
        );
        assert!(crate::import::csg::is_closed(&hollow));
        // A closed loop (a square ring of pipe) has no caps.
        let sq = [
            DVec3::ZERO,
            DVec3::new(500.0, 0.0, 0.0),
            DVec3::new(500.0, 500.0, 0.0),
            DVec3::new(0.0, 500.0, 0.0),
            DVec3::ZERO,
        ];
        let loop_ = pipe(&sq, 10.0, None, 0.2).unwrap();
        let want = PI * 100.0 * 2000.0;
        assert!(
            ((vol(&loop_) - want) / want).abs() < 0.01,
            "{} {want}",
            vol(&loop_)
        );
        assert!(crate::import::csg::is_closed(&loop_));
        // Refusals.
        assert!(pipe(&[DVec3::ZERO], 1.0, None, 0.1).is_none());
        assert!(pipe(&[DVec3::ZERO, DVec3::X], 0.0, None, 0.1).is_none());
        assert!(pipe(&[DVec3::ZERO, DVec3::X], 1.0, Some(2.0), 0.1).is_none());
    }

    #[test]
    fn primitives_have_their_analytic_volumes() {
        let s = sphere(100.0, 0.2);
        let want = 4.0 / 3.0 * PI * 1e6;
        assert!(((vol(&s) - want) / want).abs() < 0.01, "{}", vol(&s));
        assert!(crate::import::csg::is_closed(&s));
        let c = cone_frustum(50.0, 50.0, 300.0, 0.1);
        let want = PI * 2500.0 * 300.0;
        assert!(((vol(&c) - want) / want).abs() < 0.01);
        assert!(crate::import::csg::is_closed(&c));
        let k = cone_frustum(50.0, 0.0, 300.0, 0.1);
        let want = PI * 2500.0 * 300.0 / 3.0;
        assert!(((vol(&k) - want) / want).abs() < 0.01, "{}", vol(&k));
        assert!(crate::import::csg::is_closed(&k));
        let p = pyramid(100.0, 60.0, 90.0);
        assert!((vol(&p) - 100.0 * 60.0 * 90.0 / 3.0).abs() < 1e-6);
        assert!(crate::import::csg::is_closed(&p));
        assert!((vol(&block(DVec3::new(1.0, 2.0, 3.0))) - 6.0).abs() < 1e-9);
    }

    #[test]
    fn polyline_trims() {
        let pts = [DVec3::ZERO, DVec3::X * 10.0, DVec3::new(10.0, 10.0, 0.0)];
        let t = trim_polyline(&pts, 0.5, 1.5);
        assert_eq!(
            t,
            vec![
                DVec3::new(5.0, 0.0, 0.0),
                DVec3::X * 10.0,
                DVec3::new(10.0, 5.0, 0.0)
            ]
        );
        assert_eq!(trim_polyline(&pts, 1.0, 0.5), pts.to_vec());
    }
}
