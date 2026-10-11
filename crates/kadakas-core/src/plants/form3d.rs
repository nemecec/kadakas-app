//! Parametric 3D forms of plants, and their levels of detail.
//!
//! A form is a small triangle mesh around the foot of the plant, in mm, Z up. The same
//! parameters and the same seed always give the same mesh. The seed comes from the object
//! (see `seed_for`), so two plants of one species differ a little, and a plant keeps its
//! look every time the scene is built.
//!
//! Levels of detail: `Fine` is the normal mesh, `Coarse` is for scenes with many plants, and
//! `Billboard` is two crossed, upright quads with the outline of the plant. A renderer uses
//! `lod_for` to pick the level from the distance of the plant.
//!
//! Triangle budgets: a form has at most `MAX_FINE_TRIANGLES` triangles, a coarse form at most
//! `MAX_COARSE_TRIANGLES`, and a billboard at most `MAX_BILLBOARD_TRIANGLES`.

use glam::{DVec2, Vec3};

use super::style::PlantForm;
use crate::present::{PART_CANOPY, PART_TRUNK};

/// Largest triangle count of a plant at `Fine` detail (the old forms stay below it as well).
pub const MAX_FINE_TRIANGLES: usize = 400;
/// Largest triangle count of a plant at `Coarse` detail.
pub const MAX_COARSE_TRIANGLES: usize = 200;
/// Largest triangle count of a billboard.
pub const MAX_BILLBOARD_TRIANGLES: usize = 48;

/// How much detail a plant mesh has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Detail {
    #[default]
    Fine,
    Coarse,
    /// Two crossed quads.
    Billboard,
}

/// The detail for a plant at a distance, in mm. A plant that covers only a few pixels does not
/// need a mesh. `pixel_size` is the size of one pixel at that distance, in mm.
pub fn lod_for(spread: f64, pixel_size: f64) -> Detail {
    if !(spread.is_finite() && pixel_size.is_finite()) || pixel_size <= 0.0 {
        return Detail::Fine;
    }
    let pixels = spread / pixel_size;
    if pixels < 24.0 {
        Detail::Billboard
    } else if pixels < 80.0 {
        Detail::Coarse
    } else {
        Detail::Fine
    }
}

/// How many looks a species has: plants of one style and size share one of this many meshes, so
/// exports can instance them (a thousand plants make at most this many meshes).
pub const VARIANTS: u64 = 8;

/// The seed of a plant: the same variant (the object ID modulo `VARIANTS`) and species always
/// give the same seed.
pub fn seed_for(id: u64, species: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ id.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    for b in species.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A small, fast generator (SplitMix64). It has no state outside the struct.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number from 0 up to (not including) 1.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// A number from `-a` up to `a`.
    pub fn spread(&mut self, a: f32) -> f32 {
        (self.unit() * 2.0 - 1.0) * a
    }
}

/// What a form needs.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub form: PlantForm,
    /// Canopy diameter, mm.
    pub spread: f64,
    pub height: f64,
    pub color: [u8; 4],
    pub trunk_color: [u8; 4],
    /// Trunk radius, or `None` for a size that follows the spread.
    pub trunk_radius: Option<f64>,
    pub seed: u64,
    pub detail: Detail,
}

/// A triangle mesh with a normal and a color for every vertex.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormMesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub colors: Vec<[u8; 4]>,
    pub indices: Vec<u32>,
    /// The part (`PART_*` in `crate::present`) of every triangle, for materials: the trunk or the canopy.
    pub tri_part: Vec<u8>,
    /// The part that new triangles get.
    pub part: u8,
}

impl FormMesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    fn vertex(&mut self, p: Vec3, n: Vec3, c: [u8; 4]) -> u32 {
        self.positions.push(p);
        self.normals.push(n);
        self.colors.push(c);
        (self.positions.len() - 1) as u32
    }

    fn tri(&mut self, a: u32, b: u32, c: u32) {
        let (pa, pb, pc) = (
            self.positions[a as usize],
            self.positions[b as usize],
            self.positions[c as usize],
        );
        if (pb - pa).cross(pc - pa).length_squared() < 1e-12 {
            return;
        }
        self.indices.extend_from_slice(&[a, b, c]);
        self.tri_part.push(self.part);
    }

    /// Box around the vertices, or `None` for an empty mesh.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let first = *self.positions.first()?;
        Some(
            self.positions
                .iter()
                .fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))),
        )
    }

    /// Moves and scales the mesh so that its box is centered on the Z axis, sits on Z = 0, is
    /// exactly `height` tall and is at most `spread` wide. The random lean and wobble of a
    /// form change its box a little, and this keeps every plant at its size.
    fn fit(&mut self, spread: f32, height: f32) {
        let Some((lo, hi)) = self.bounds() else {
            return;
        };
        let ext = (hi - lo).max(Vec3::splat(1e-3));
        let k = (spread / ext.x.max(ext.y)).min(1.0e3);
        let kz = height / ext.z;
        let mid = (lo + hi) / 2.0;
        for p in &mut self.positions {
            *p = Vec3::new((p.x - mid.x) * k, (p.y - mid.y) * k, (p.z - lo.z) * kz);
        }
    }

    /// A surface of revolution around the Z axis through `center`. `profile` runs from the
    /// bottom to the top as (radius, height) pairs. `wobble` moves each vertex outward by a
    /// fraction of its radius, drawn from `rng`, so the plant is not perfectly round.
    fn lathe(
        &mut self,
        center: Vec3,
        profile: &[(f32, f32)],
        sides: usize,
        color: [u8; 4],
        wobble: f32,
        rng: &mut Rng,
    ) {
        let n = profile.len();
        if n < 2 || sides < 3 {
            return;
        }
        // The normal of each profile point in the (radius, height) plane.
        let seg_normal = |i: usize| -> (f32, f32) {
            let (dr, dz) = (
                profile[i + 1].0 - profile[i].0,
                profile[i + 1].1 - profile[i].1,
            );
            let len = (dr * dr + dz * dz).sqrt();
            if len > 0.0 {
                (dz / len, -dr / len)
            } else {
                (1.0, 0.0)
            }
        };
        let base = self.positions.len() as u32;
        for (i, (r, z)) in profile.iter().enumerate() {
            let (a, b) = (
                seg_normal(i.saturating_sub(1).min(n - 2)),
                seg_normal(i.min(n - 2)),
            );
            let (nr, nz) = (a.0 + b.0, a.1 + b.1);
            let len = (nr * nr + nz * nz).sqrt().max(1e-6);
            let (nr, nz) = (nr / len, nz / len);
            for s in 0..sides {
                let th = std::f32::consts::TAU * s as f32 / sides as f32;
                let (sin, cos) = th.sin_cos();
                let k = if *r > 0.0 {
                    1.0 + rng.spread(wobble)
                } else {
                    1.0
                };
                let rr = r * k;
                self.vertex(
                    center + Vec3::new(rr * cos, rr * sin, *z),
                    Vec3::new(nr * cos, nr * sin, nz),
                    color,
                );
            }
        }
        for i in 0..n - 1 {
            for s in 0..sides {
                let s2 = (s + 1) % sides;
                let at = |i: usize, s: usize| base + (i * sides + s) as u32;
                let (a, b, c, d) = (at(i, s), at(i, s2), at(i + 1, s2), at(i + 1, s));
                if profile[i].0 > 0.0 {
                    self.tri(a, b, c);
                }
                if profile[i + 1].0 > 0.0 {
                    self.tri(a, c, d);
                }
            }
        }
    }

    /// A tube from `a` to `b` through `rings` rings (at least 2), with a radius that goes from
    /// `r0` to `r1`. The ends are open (a lobe or the ground hides them).
    fn tube(&mut self, pts: &[Vec3], radii: &[f32], sides: usize, color: [u8; 4]) {
        let keep = self.part;
        self.part = PART_TRUNK;
        self.tube_rings(pts, radii, sides, color);
        self.part = keep;
    }

    fn tube_rings(&mut self, pts: &[Vec3], radii: &[f32], sides: usize, color: [u8; 4]) {
        let n = pts.len();
        if n < 2 || radii.len() != n || sides < 3 {
            return;
        }
        let base = self.positions.len() as u32;
        for i in 0..n {
            let t = (pts[(i + 1).min(n - 1)] - pts[i.saturating_sub(1)]).normalize_or_zero();
            let u = if t.z.abs() > 0.9 { Vec3::X } else { Vec3::Z };
            let e1 = t.cross(u).normalize_or_zero();
            let e2 = t.cross(e1);
            for s in 0..sides {
                let th = std::f32::consts::TAU * s as f32 / sides as f32;
                let dir = e1 * th.cos() + e2 * th.sin();
                self.vertex(pts[i] + dir * radii[i], dir, color);
            }
        }
        for i in 0..n - 1 {
            for s in 0..sides {
                let s2 = (s + 1) % sides;
                let at = |i: usize, s: usize| base + (i * sides + s) as u32;
                let (a, b, c, d) = (at(i, s), at(i, s2), at(i + 1, s2), at(i + 1, s));
                // The sign of the winding follows the direction of the axis.
                self.tri(a, b, c);
                self.tri(a, c, d);
            }
        }
        // Caps, so that a cut through the tube is a closed face.
        for (i, outward) in [(0, -1.0f32), (n - 1, 1.0)] {
            let axis =
                (pts[(i + 1).min(n - 1)] - pts[i.saturating_sub(1)]).normalize_or_zero() * outward;
            let center = self.vertex(pts[i], axis, color);
            let ring: Vec<u32> = (0..sides)
                .map(|s| {
                    let p = self.positions[(base as usize) + i * sides + s];
                    self.vertex(p, axis, color)
                })
                .collect();
            for s in 0..sides {
                let (a, b) = (ring[s], ring[(s + 1) % sides]);
                let (pa, pb, pc) = (
                    self.positions[center as usize],
                    self.positions[a as usize],
                    self.positions[b as usize],
                );
                if (pb - pa).cross(pc - pa).dot(axis) >= 0.0 {
                    self.tri(center, a, b);
                } else {
                    self.tri(center, b, a);
                }
            }
        }
    }

    /// An ellipsoid with `segments` around and `rings` from pole to pole.
    fn ellipsoid(
        &mut self,
        center: Vec3,
        radii: Vec3,
        segments: usize,
        rings: usize,
        color: [u8; 4],
    ) {
        let base = self.positions.len() as u32;
        for r in 0..=rings {
            let phi = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * r as f32 / rings as f32;
            for s in 0..segments {
                let th = std::f32::consts::TAU * s as f32 / segments as f32;
                let d = Vec3::new(phi.cos() * th.cos(), phi.cos() * th.sin(), phi.sin());
                self.vertex(center + d * radii, (d / radii).normalize_or_zero(), color);
            }
        }
        for r in 0..rings {
            for s in 0..segments {
                let s2 = (s + 1) % segments;
                let at = |r: usize, s: usize| base + (r * segments + s) as u32;
                let (a, b, c, d) = (at(r, s), at(r, s2), at(r + 1, s2), at(r + 1, s));
                if r > 0 {
                    self.tri(a, b, c);
                }
                if r + 1 < rings {
                    self.tri(a, c, d);
                }
            }
        }
    }

    /// A strip of quads along two rails, seen from both sides.
    fn two_sided_strip(&mut self, left: &[Vec3], right: &[Vec3], normal: Vec3, color: [u8; 4]) {
        let n = left.len().min(right.len());
        if n < 2 {
            return;
        }
        for (flip, nrm) in [(false, normal), (true, -normal)] {
            let base = self.positions.len() as u32;
            for i in 0..n {
                self.vertex(left[i], nrm, color);
                self.vertex(right[i], nrm, color);
            }
            for i in 0..n - 1 {
                let (a, b, c, d) = (
                    base + 2 * i as u32,
                    base + 2 * i as u32 + 1,
                    base + 2 * i as u32 + 3,
                    base + 2 * i as u32 + 2,
                );
                if flip {
                    self.tri(a, c, b);
                    self.tri(a, d, c);
                } else {
                    self.tri(a, b, c);
                    self.tri(a, c, d);
                }
            }
        }
    }
}

fn shade(c: [u8; 4], f: f32) -> [u8; 4] {
    [
        (c[0] as f32 * f) as u8,
        (c[1] as f32 * f) as u8,
        (c[2] as f32 * f) as u8,
        c[3],
    ]
}

/// The default trunk radius for a spread.
fn trunk_radius(p: &Params) -> f32 {
    p.trunk_radius
        .unwrap_or((p.spread * 0.04).clamp(15.0, 300.0))
        .min(p.spread * 0.25) as f32
}

fn scaled(profile: &[(f32, f32)], r: f32, h: f32) -> Vec<(f32, f32)> {
    profile.iter().map(|(a, b)| (a * r, b * h)).collect()
}

/// The mesh of a plant, with its foot at the origin. Returns an empty mesh for sizes that are
/// not positive and finite.
pub fn build(p: &Params) -> FormMesh {
    let mut m = FormMesh {
        part: PART_CANOPY,
        ..FormMesh::default()
    };
    if !(p.spread.is_finite() && p.height.is_finite() && p.spread > 0.0 && p.height > 0.0) {
        return m;
    }
    if p.detail == Detail::Billboard {
        billboard(p, &mut m);
        m.fit(p.spread as f32, p.height as f32);
        return m;
    }
    let mut rng = Rng::new(p.seed);
    let coarse = p.detail == Detail::Coarse;
    let sides = if coarse { 8 } else { 12 };
    let (r, h) = ((p.spread / 2.0) as f32, p.height as f32);
    let tr = trunk_radius(p);
    let trunk_sides = if coarse { 6 } else { 8 };
    match p.form {
        PlantForm::Weeping => {
            let lean = Vec3::new(rng.spread(0.03 * h), rng.spread(0.03 * h), 0.0);
            m.tube(
                &[
                    Vec3::ZERO,
                    Vec3::new(0.0, 0.0, 0.3 * h) + lean * 0.5,
                    Vec3::new(0.0, 0.0, 0.6 * h) + lean,
                ],
                &[tr, tr * 0.8, tr * 0.6],
                trunk_sides,
                p.trunk_color,
            );
            let profile = [
                (0.0, 0.52),
                (0.55, 0.40),
                (0.92, 0.12),
                (1.0, 0.30),
                (0.95, 0.58),
                (0.70, 0.84),
                (0.30, 0.97),
                (0.0, 1.0),
            ];
            m.lathe(
                lean,
                &scaled(&profile, r, h),
                sides,
                p.color,
                0.06,
                &mut rng,
            );
        }
        PlantForm::Palm => {
            let lean = Vec3::new(rng.spread(0.06 * h), rng.spread(0.06 * h), 0.0);
            let top = Vec3::new(0.0, 0.0, 0.82 * h) + lean;
            let pts: Vec<Vec3> = (0..=3)
                .map(|i| {
                    let t = i as f32 / 3.0;
                    Vec3::new(lean.x * t * t, lean.y * t * t, top.z * t)
                })
                .collect();
            let rads: Vec<f32> = (0..=3).map(|i| tr * (1.0 - 0.3 * i as f32 / 3.0)).collect();
            m.tube(&pts, &rads, trunk_sides, p.trunk_color);
            let fronds = if coarse { 5 } else { 7 };
            let phase = rng.unit() * std::f32::consts::TAU;
            for k in 0..fronds {
                let th =
                    phase + std::f32::consts::TAU * k as f32 / fronds as f32 + rng.spread(0.15);
                let dir = Vec3::new(th.cos(), th.sin(), 0.0);
                let side = Vec3::new(-dir.y, dir.x, 0.0);
                let len = r * (0.85 + rng.unit() * 0.15);
                let (mut left, mut right) = (Vec::new(), Vec::new());
                for j in 0..=4 {
                    let t = j as f32 / 4.0;
                    let at = top
                        + dir * (len * t)
                        + Vec3::new(0.0, 0.0, 0.17 * h * (t * 1.6).sin() - 0.35 * h * t * t);
                    let w = 0.09 * len * (std::f32::consts::PI * (0.15 + 0.85 * t)).sin().max(0.05);
                    left.push(at - side * w);
                    right.push(at + side * w);
                }
                m.two_sided_strip(&left, &right, Vec3::Z, p.color);
            }
        }
        PlantForm::ColumnarConifer => {
            let profile = [
                (0.0, 0.0),
                (0.7, 0.02),
                (0.95, 0.20),
                (0.70, 0.22),
                (0.85, 0.45),
                (0.58, 0.47),
                (0.65, 0.68),
                (0.40, 0.70),
                (0.40, 0.86),
                (0.0, 1.0),
            ];
            m.lathe(
                Vec3::ZERO,
                &scaled(&profile, r, h),
                sides,
                p.color,
                0.04,
                &mut rng,
            );
        }
        PlantForm::MultiStem => {
            let stems = if coarse {
                3
            } else {
                3 + (rng.next_u64() % 3) as usize
            };
            let phase = rng.unit() * std::f32::consts::TAU;
            // Five stems with the full detail would pass the triangle budget: they get fewer sides.
            let (seg, rings, stem_sides) = match (coarse, stems) {
                (true, _) => (6, 3, 5),
                (false, 5) => (7, 4, 5),
                (false, _) => (8, 4, 6),
            };
            for k in 0..stems {
                let th = phase + std::f32::consts::TAU * k as f32 / stems as f32 + rng.spread(0.3);
                let dir = Vec3::new(th.cos(), th.sin(), 0.0);
                let top = dir * (0.5 * r) + Vec3::new(0.0, 0.0, 0.62 * h);
                let mid = top * 0.5 + dir * (0.05 * r);
                let sr = (tr * 0.55).max(8.0);
                m.tube(
                    &[dir * (0.05 * r), mid, top],
                    &[sr, sr * 0.8, sr * 0.6],
                    stem_sides,
                    p.trunk_color,
                );
                let k = 0.9 + rng.unit() * 0.1;
                m.ellipsoid(
                    top,
                    Vec3::new(0.5 * r, 0.5 * r, 0.38 * h * k),
                    seg,
                    rings,
                    p.color,
                );
            }
        }
        PlantForm::Hedge => {
            // A short stem inside the block, so that the hedge has a trunk part too.
            m.tube(
                &[Vec3::ZERO, Vec3::new(0.0, 0.0, 0.1 * h)],
                &[tr, tr],
                trunk_sides,
                p.trunk_color,
            );
            let path = [
                DVec2::new(-p.spread / 2.0, 0.0),
                DVec2::new(p.spread / 2.0, 0.0),
            ];
            hedge_along(
                &path,
                false,
                p.spread * 0.5,
                p.height,
                p.color,
                Vec3::ZERO,
                &mut m,
            );
        }
        // The old forms: one trunk and an ellipsoid or a cone. They are drawn by the scene
        // builder at `Fine` and `Coarse`; this is the same shape for callers that need it alone.
        PlantForm::Tree | PlantForm::Spreading | PlantForm::Columnar => {
            let (bottom, trunk_top) = match p.form {
                PlantForm::Tree => (0.3, 0.65),
                PlantForm::Spreading => (0.5, 0.6),
                _ => (0.1, 0.3),
            };
            m.tube(
                &[Vec3::ZERO, Vec3::new(0.0, 0.0, trunk_top * h)],
                &[tr, tr],
                trunk_sides,
                p.trunk_color,
            );
            let ch = h * (1.0 - bottom);
            m.ellipsoid(
                Vec3::new(0.0, 0.0, h * bottom + ch / 2.0),
                Vec3::new(r, r, ch / 2.0),
                sides,
                if coarse { 4 } else { 8 },
                p.color,
            );
        }
        PlantForm::Conical => {
            let profile = [(0.0, 0.0), (1.0, 0.08), (0.5, 0.55), (0.0, 1.0)];
            m.lathe(
                Vec3::ZERO,
                &scaled(&profile, r, h),
                sides,
                p.color,
                0.03,
                &mut rng,
            );
        }
        PlantForm::ShrubMound => {
            let profile = [
                (0.0, 0.0),
                (1.0, 0.0),
                (0.92, 0.45),
                (0.6, 0.85),
                (0.0, 1.0),
            ];
            m.lathe(
                Vec3::ZERO,
                &scaled(&profile, r, h),
                sides,
                p.color,
                0.06,
                &mut rng,
            );
        }
    }
    // A short stub inside the form, so that every form has a trunk part and a canopy part.
    if !m.tri_part.contains(&PART_TRUNK) {
        m.tube(
            &[Vec3::ZERO, Vec3::new(0.0, 0.0, 0.08 * h)],
            &[tr.min(r * 0.5), tr.min(r * 0.5)],
            if coarse { 4 } else { 6 },
            p.trunk_color,
        );
    }
    m.fit(p.spread as f32, p.height as f32);
    m
}

/// A block along a path, as a hedge: a section with a flat bottom, upright sides and a
/// chamfered top, extruded along the path with mitered corners. A path that is not closed gets
/// end caps. `origin` is added to every position. Paths with fewer than two distinct points
/// give nothing.
pub fn hedge_along(
    path: &[DVec2],
    closed: bool,
    width: f64,
    height: f64,
    color: [u8; 4],
    origin: Vec3,
    m: &mut FormMesh,
) {
    let mut pts: Vec<DVec2> = Vec::with_capacity(path.len());
    for p in path {
        if pts.last().is_none_or(|q| q.distance_squared(*p) > 1e-6) {
            pts.push(*p);
        }
    }
    if closed && pts.len() > 1 && pts[0].distance_squared(pts[pts.len() - 1]) <= 1e-6 {
        pts.pop();
    }
    let n = pts.len();
    if n < 2 || !(width > 0.0 && height > 0.0) || (closed && n < 3) {
        return;
    }
    let half = width / 2.0;
    // The section as (offset to the left, height), counterclockwise seen from the path start.
    let section: [(f64, f64); 6] = [
        (-half, 0.0),
        (half, 0.0),
        (half, 0.85 * height),
        (0.35 * half, height),
        (-0.35 * half, height),
        (-half, 0.85 * height),
    ];
    // Direction of each segment, and the left normal at each point.
    let seg_dir = |i: usize| (pts[(i + 1) % n] - pts[i]).normalize_or_zero();
    let mut left: Vec<(DVec2, f64)> = Vec::with_capacity(n);
    for i in 0..n {
        let d1 = if i > 0 || closed {
            seg_dir((i + n - 1) % n)
        } else {
            seg_dir(0)
        };
        let d2 = if i + 1 < n || closed {
            seg_dir(i)
        } else {
            seg_dir(n - 2)
        };
        let (n1, n2) = (d1.perp(), d2.perp());
        let sum = n1 + n2;
        // The miter: the offset is stretched at a corner. Cap the stretch for sharp turns.
        let k = if sum.length_squared() > 1e-9 {
            let m = sum.normalize();
            (1.0 / m.dot(n1).max(0.3), m)
        } else {
            (1.0, n1)
        };
        left.push((k.1, k.0));
    }
    // d.perp() turns left (counterclockwise): positive offsets are on the left.
    let ring = section.len();
    let base = m.positions.len() as u32;
    // The outward normal of each section edge, in the (offset, height) plane.
    let edge_normal = |j: usize| -> (f64, f64) {
        let (a, b) = (section[j], section[(j + 1) % ring]);
        let (dx, dz) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dz * dz).sqrt();
        (dz / len, -dx / len)
    };
    // Two vertices for each edge of each section, so the edges stay hard.
    for i in 0..n {
        let (dir, k) = left[i];
        for j in 0..ring {
            let (nx, nz) = edge_normal(j);
            let nrm = dir * nx;
            let normal = Vec3::new(nrm.x as f32, nrm.y as f32, nz as f32).normalize_or_zero();
            for end in [j, (j + 1) % ring] {
                let (off, z) = section[end];
                let p = pts[i] + dir * (off * k);
                m.vertex(
                    origin + Vec3::new(p.x as f32, p.y as f32, z as f32),
                    normal,
                    color,
                );
            }
        }
    }
    let at = |i: usize, j: usize, end: usize| base + (((i * ring + j) * 2) + end) as u32;
    let spans = if closed { n } else { n - 1 };
    for i in 0..spans {
        let i2 = (i + 1) % n;
        for j in 0..ring {
            let (a, b, c, d) = (at(i, j, 0), at(i, j, 1), at(i2, j, 1), at(i2, j, 0));
            m.tri(a, b, c);
            m.tri(a, c, d);
        }
    }
    if !closed {
        for (i, start) in [(0usize, true), (n - 1, false)] {
            let dir = if start { -seg_dir(0) } else { seg_dir(n - 2) };
            let cap = m.positions.len() as u32;
            let (d, k) = left[i];
            for (off, z) in &section {
                let p = pts[i] + d * (off * k);
                m.vertex(
                    origin + Vec3::new(p.x as f32, p.y as f32, *z as f32),
                    Vec3::new(dir.x as f32, dir.y as f32, 0.0),
                    shade(color, 0.9),
                );
            }
            // A fan over the six points. The cap at the start faces backward.
            for j in 1..ring as u32 - 1 {
                if start {
                    m.tri(cap, cap + j + 1, cap + j);
                } else {
                    m.tri(cap, cap + j, cap + j + 1);
                }
            }
        }
    }
}

/// The half widths of the outline of a plant, as (fraction of the height, fraction of the radius).
fn outline(form: PlantForm) -> &'static [(f32, f32)] {
    match form {
        PlantForm::Tree => &[
            (0.0, 0.06),
            (0.3, 0.1),
            (0.4, 0.75),
            (0.65, 1.0),
            (0.9, 0.6),
            (1.0, 0.0),
        ],
        PlantForm::Spreading => &[(0.0, 0.06), (0.5, 0.1), (0.6, 0.8), (0.8, 1.0), (1.0, 0.1)],
        PlantForm::Columnar | PlantForm::ColumnarConifer => {
            &[(0.0, 0.5), (0.3, 0.9), (0.7, 0.8), (1.0, 0.0)]
        }
        PlantForm::Conical => &[(0.0, 1.0), (0.5, 0.5), (1.0, 0.0)],
        PlantForm::ShrubMound => &[(0.0, 1.0), (0.45, 0.92), (0.85, 0.6), (1.0, 0.0)],
        PlantForm::Weeping => &[
            (0.1, 0.9),
            (0.3, 1.0),
            (0.6, 0.95),
            (0.85, 0.65),
            (1.0, 0.0),
        ],
        PlantForm::Palm => &[(0.0, 0.05), (0.75, 0.08), (0.85, 1.0), (1.0, 0.3)],
        PlantForm::MultiStem => &[(0.0, 0.1), (0.25, 0.4), (0.5, 1.0), (0.8, 0.9), (1.0, 0.0)],
        PlantForm::Hedge => &[(0.0, 1.0), (0.85, 1.0), (1.0, 0.7)],
    }
}

/// Two upright quads that cross at the middle, with the outline of the plant. Each is seen
/// from both sides.
fn billboard(p: &Params, m: &mut FormMesh) {
    let (r, h) = ((p.spread / 2.0) as f32, p.height as f32);
    let outline = outline(p.form);
    for angle in [0.0_f32, std::f32::consts::FRAC_PI_2] {
        let dir = Vec3::new(angle.cos(), angle.sin(), 0.0);
        let normal = (Vec3::new(-dir.y, dir.x, 0.0) + Vec3::Z).normalize();
        let left: Vec<Vec3> = outline
            .iter()
            .map(|(t, w)| dir * (-r * w) + Vec3::new(0.0, 0.0, h * t))
            .collect();
        let right: Vec<Vec3> = outline
            .iter()
            .map(|(t, w)| dir * (r * w) + Vec3::new(0.0, 0.0, h * t))
            .collect();
        m.two_sided_strip(&left, &right, normal, p.color);
    }
    // A narrow trunk quad at the foot, so that the billboard has a trunk part too.
    let tw = trunk_radius(p).min(r * 0.2);
    m.part = PART_TRUNK;
    {
        let dir = Vec3::X;
        let top = Vec3::new(0.0, 0.0, 0.25 * h);
        m.two_sided_strip(
            &[dir * -tw, top + dir * -tw],
            &[dir * tw, top + dir * tw],
            Vec3::Y,
            p.trunk_color,
        );
    }
    m.part = PART_CANOPY;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(form: PlantForm, detail: Detail) -> Params {
        Params {
            form,
            spread: 4000.0,
            height: 8000.0,
            color: [60, 140, 60, 255],
            trunk_color: [110, 80, 55, 255],
            trunk_radius: None,
            seed: 42,
            detail,
        }
    }

    #[test]
    fn every_form_is_deterministic_and_within_budget() {
        for form in PlantForm::ALL {
            for (detail, max) in [
                (Detail::Fine, MAX_FINE_TRIANGLES),
                (Detail::Coarse, MAX_COARSE_TRIANGLES),
                (Detail::Billboard, MAX_BILLBOARD_TRIANGLES),
            ] {
                let a = build(&params(form, detail));
                let b = build(&params(form, detail));
                assert_eq!(a, b, "{form:?} {detail:?}");
                assert!(a.triangle_count() > 0, "{form:?} {detail:?}");
                assert!(
                    a.triangle_count() <= max,
                    "{form:?} {detail:?}: {} triangles",
                    a.triangle_count()
                );
                assert_eq!(a.positions.len(), a.normals.len());
                assert_eq!(a.positions.len(), a.colors.len());
                assert!(a.indices.iter().all(|i| (*i as usize) < a.positions.len()));
                assert!(a.positions.iter().all(|p| p.is_finite()));
                assert!(a.normals.iter().all(|n| n.is_finite()));
            }
        }
    }

    #[test]
    fn every_form_stays_within_budget_for_every_seed() {
        // The seed comes from the object ID and the species, so any seed can happen.
        for form in PlantForm::ALL {
            for seed in 0..400u64 {
                for (detail, max) in [
                    (Detail::Fine, MAX_FINE_TRIANGLES),
                    (Detail::Coarse, MAX_COARSE_TRIANGLES),
                    (Detail::Billboard, MAX_BILLBOARD_TRIANGLES),
                ] {
                    let p = Params {
                        seed,
                        ..params(form, detail)
                    };
                    let n = build(&p).triangle_count();
                    assert!(n <= max, "{form:?} {detail:?} seed {seed}: {n} triangles");
                }
            }
        }
    }

    #[test]
    fn every_form_has_a_trunk_part_and_a_canopy_part_at_every_level() {
        for form in PlantForm::ALL {
            for detail in [Detail::Fine, Detail::Coarse, Detail::Billboard] {
                let m = build(&params(form, detail));
                assert_eq!(m.tri_part.len(), m.triangle_count(), "{form:?} {detail:?}");
                let count = |part: u8| m.tri_part.iter().filter(|p| **p == part).count();
                assert!(count(PART_TRUNK) > 0, "{form:?} {detail:?}: no trunk");
                assert!(count(PART_CANOPY) > 0, "{form:?} {detail:?}: no canopy");
                assert_eq!(
                    count(PART_TRUNK) + count(PART_CANOPY),
                    m.triangle_count(),
                    "{form:?} {detail:?}: other parts"
                );
            }
        }
    }

    #[test]
    fn forms_stay_inside_their_size() {
        for form in PlantForm::ALL {
            let m = build(&params(form, Detail::Fine));
            let (lo, hi) = m.bounds().unwrap();
            assert!(lo.z >= -20.0, "{form:?} low {lo}");
            assert!(hi.z <= 8000.0 * 1.02, "{form:?} high {hi}");
            // Palm leans and hedges are wider in X; the others stay within the spread.
            let reach = lo.x.abs().max(hi.x.abs()).max(lo.y.abs()).max(hi.y.abs());
            assert!(reach <= 2000.0 * 1.4, "{form:?} reach {reach}");
            assert!(hi.z > 8000.0 * 0.6, "{form:?} should be tall: {hi}");
        }
    }

    #[test]
    fn seeds_change_the_look() {
        let mut p = params(PlantForm::MultiStem, Detail::Fine);
        let a = build(&p);
        p.seed = 43;
        let b = build(&p);
        assert_ne!(a, b);
        assert_eq!(seed_for(7, "Acer"), seed_for(7, "Acer"));
        assert_ne!(seed_for(7, "Acer"), seed_for(8, "Acer"));
        assert_ne!(seed_for(7, "Acer"), seed_for(7, "Betula"));
    }

    #[test]
    fn bad_sizes_give_nothing() {
        for (s, h) in [
            (0.0, 1.0),
            (1.0, 0.0),
            (f64::NAN, 1.0),
            (1.0, f64::INFINITY),
            (-1.0, 1.0),
        ] {
            let mut p = params(PlantForm::Tree, Detail::Fine);
            p.spread = s;
            p.height = h;
            assert_eq!(build(&p).triangle_count(), 0);
        }
        assert!(FormMesh::default().bounds().is_none());
    }

    #[test]
    fn billboards_cross() {
        let m = build(&params(PlantForm::Tree, Detail::Billboard));
        let (lo, hi) = m.bounds().unwrap();
        assert!(lo.x < -1000.0 && hi.x > 1000.0 && lo.y < -1000.0 && hi.y > 1000.0);
        assert!((hi.z - 8000.0).abs() < 1e-3);
    }

    #[test]
    fn lod_follows_the_pixel_size() {
        assert_eq!(lod_for(4000.0, 10.0), Detail::Fine);
        assert_eq!(lod_for(4000.0, 100.0), Detail::Coarse);
        assert_eq!(lod_for(4000.0, 1000.0), Detail::Billboard);
        assert_eq!(lod_for(4000.0, 0.0), Detail::Fine);
        assert_eq!(lod_for(f64::NAN, 1.0), Detail::Fine);
    }

    #[test]
    fn rng_is_in_range_and_repeatable() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(1);
        for _ in 0..100 {
            let x = a.unit();
            assert!((0.0..1.0).contains(&x));
            assert_eq!(x, b.unit());
            assert!(a.spread(2.0).abs() <= 2.0);
            b.spread(2.0);
        }
    }

    #[test]
    fn hedge_follows_a_path() {
        let mut m = FormMesh::default();
        let path = [
            DVec2::new(0.0, 0.0),
            DVec2::new(3000.0, 0.0),
            DVec2::new(3000.0, 2000.0),
        ];
        hedge_along(
            &path,
            false,
            600.0,
            1500.0,
            [30, 100, 30, 255],
            Vec3::ZERO,
            &mut m,
        );
        let (lo, hi) = m.bounds().unwrap();
        assert!(lo.z.abs() < 1e-3 && (hi.z - 1500.0).abs() < 1e-3);
        assert!(lo.x < 1.0 && hi.x > 3000.0 && hi.y > 1999.0);
        assert!(m.indices.iter().all(|i| (*i as usize) < m.positions.len()));
        // Closed ring: no caps, the same path closed has fewer triangles per span count.
        let mut c = FormMesh::default();
        let ring = [
            DVec2::new(0.0, 0.0),
            DVec2::new(3000.0, 0.0),
            DVec2::new(3000.0, 2000.0),
            DVec2::new(0.0, 2000.0),
        ];
        hedge_along(
            &ring,
            true,
            600.0,
            1500.0,
            [30, 100, 30, 255],
            Vec3::ZERO,
            &mut c,
        );
        assert_eq!(c.triangle_count(), 4 * 6 * 2);
        // Bad input gives nothing.
        let mut e = FormMesh::default();
        hedge_along(&path[..1], false, 600.0, 1500.0, [0; 4], Vec3::ZERO, &mut e);
        hedge_along(&path, false, 0.0, 1500.0, [0; 4], Vec3::ZERO, &mut e);
        hedge_along(&path[..2], true, 600.0, 1500.0, [0; 4], Vec3::ZERO, &mut e);
        assert_eq!(e.triangle_count(), 0);
    }

    #[test]
    fn hedge_faces_point_outward() {
        let mut m = FormMesh::default();
        let path = [DVec2::new(0.0, 0.0), DVec2::new(3000.0, 0.0)];
        hedge_along(
            &path,
            false,
            600.0,
            1500.0,
            [30, 100, 30, 255],
            Vec3::ZERO,
            &mut m,
        );
        let center = Vec3::new(1500.0, 0.0, 600.0);
        // Triangles on the side and the top must face away from the middle of the block.
        for t in m.indices.as_chunks::<3>().0 {
            let [a, b, c] = [
                m.positions[t[0] as usize],
                m.positions[t[1] as usize],
                m.positions[t[2] as usize],
            ];
            let n = (b - a).cross(c - a);
            if n.length() < 1e-3 {
                continue;
            }
            let mid = (a + b + c) / 3.0;
            assert!(n.dot(mid - center) > 0.0, "inward triangle {a} {b} {c}");
        }
    }
}
