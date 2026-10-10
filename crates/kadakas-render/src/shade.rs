//! The CPU renderer with materials: textures, sun light, shadows and transparency.
//!
//! It is deterministic: the same scene, camera and style give the same pixels on every run,
//! with no threads and no random numbers. Agents use it through `render.png`, and sheets use it
//! for 3D viewports. The GPU renderer in the app uses the same constants for the light.
//!
//! The light model is small (PBR-lite):
//!
//! - Ambient light is a hemisphere: sky color from above, a darker ground color from below.
//! - The sun is one directional light with a shadow map (one cascade, fitted to the scene).
//! - Diffuse light scales with `1 - metallic`. A specular highlight uses roughness and metallic.
//! - Surfaces with an opacity below 1 are drawn last, far to near, and blend with what is behind.

use glam::{Vec2, Vec3, Vec4};
use kadakas_core::mesh::Scene;
use kadakas_core::present::RenderMode;

use crate::camera::Camera;
use crate::hlr::{HlOptions, hidden_lines};
use crate::look::Look;
use crate::raster::{AMBIENT, Image, LIGHT_DIR, RenderOptions};
use crate::views::Projection;

/// Hemisphere ambient light from above and from below, in the sun light model.
pub const SKY_AMBIENT: f32 = 0.36;
pub const GROUND_AMBIENT: f32 = 0.2;
/// Strength of the sun on a surface that faces it.
pub const SUN_INTENSITY: f32 = 0.72;
/// Ambient light when the sun is below the horizon.
pub const NIGHT_AMBIENT: f32 = 0.18;
/// Side of the shadow map in texels.
pub const SHADOW_SIZE: usize = 1024;

/// How the scene is lit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Light {
    /// The fixed light of the plain shaded view (`LIGHT_DIR`), no shadows.
    Fixed,
    /// The sun. The vector points toward the sun, in model axes.
    Sun([f32; 3]),
    /// The sun is below the horizon.
    Night,
}

/// Settings of one render.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadeOptions {
    pub width: u32,
    pub height: u32,
    pub mode: RenderMode,
    /// Draw the edges over shaded surfaces.
    pub edges: bool,
    pub shadows: bool,
    pub light: Light,
    pub background: [u8; 4],
    /// Line weight of hidden-line modes, in pixels.
    pub line_px: f32,
    /// Samples per pixel side. 2 gives smooth edges for 4 times the work.
    pub samples: u32,
}

impl Default for ShadeOptions {
    fn default() -> Self {
        ShadeOptions {
            width: 800,
            height: 600,
            mode: RenderMode::Shaded,
            edges: true,
            shadows: false,
            light: Light::Fixed,
            background: crate::raster::BACKGROUND,
            line_px: 1.2,
            samples: 1,
        }
    }
}

/// The depth of the scene as seen from the sun.
pub struct ShadowMap {
    size: usize,
    depth: Vec<f32>,
    right: Vec3,
    up: Vec3,
    axis: Vec3,
    min: Vec2,
    scale: Vec2,
    /// Size of one texel in scene units.
    texel: f32,
}

impl ShadowMap {
    /// Draws the depth of all triangles that cast shadows. `casts(t)` says if triangle `t` does.
    pub fn build(
        scene: &Scene,
        to_sun: Vec3,
        size: usize,
        casts: &dyn Fn(usize) -> bool,
    ) -> Option<ShadowMap> {
        let (lo, hi) = scene.bounds?;
        let l = to_sun.normalize_or_zero();
        if l == Vec3::ZERO {
            return None;
        }
        let helper = if l.z.abs() > 0.99 { Vec3::Y } else { Vec3::Z };
        let right = helper.cross(l).normalize();
        let up = l.cross(right);
        let axis = -l;
        let mut min = Vec2::splat(f32::MAX);
        let mut max = Vec2::splat(f32::MIN);
        for i in 0..8 {
            let c = Vec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            let p = Vec2::new(c.dot(right), c.dot(up));
            min = min.min(p);
            max = max.max(p);
        }
        let extent = (max - min).max(Vec2::splat(1.0));
        let scale = Vec2::splat(size as f32) / extent;
        let mut map = ShadowMap {
            size,
            depth: vec![f32::INFINITY; size * size],
            right,
            up,
            axis,
            min,
            scale,
            texel: extent.max_element() / size as f32,
        };
        let mesh = &scene.mesh;
        for t in 0..mesh.triangle_count() {
            if !casts(t) {
                continue;
            }
            let p = mesh.triangle(t).map(|v| map.project(v));
            map.fill(p);
        }
        Some(map)
    }

    /// Texel position and depth along the light of a point.
    fn project(&self, p: Vec3) -> Vec3 {
        let xy = (Vec2::new(p.dot(self.right), p.dot(self.up)) - self.min) * self.scale;
        Vec3::new(xy.x, xy.y, p.dot(self.axis))
    }

    fn fill(&mut self, p: [Vec3; 3]) {
        let area = (p[1].x - p[0].x) * (p[2].y - p[0].y) - (p[1].y - p[0].y) * (p[2].x - p[0].x);
        if area.abs() < 1e-9 {
            return;
        }
        let n = self.size as i64;
        let lo_x = (p.iter().map(|v| v.x).fold(f32::MAX, f32::min).floor() as i64).max(0);
        let hi_x = (p.iter().map(|v| v.x).fold(f32::MIN, f32::max).ceil() as i64).min(n - 1);
        let lo_y = (p.iter().map(|v| v.y).fold(f32::MAX, f32::min).floor() as i64).max(0);
        let hi_y = (p.iter().map(|v| v.y).fold(f32::MIN, f32::max).ceil() as i64).min(n - 1);
        let inv = 1.0 / area;
        for y in lo_y..=hi_y {
            for x in lo_x..=hi_x {
                let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 =
                    ((p[2].x - p[1].x) * (cy - p[1].y) - (p[2].y - p[1].y) * (cx - p[1].x)) * inv;
                let w1 =
                    ((p[0].x - p[2].x) * (cy - p[2].y) - (p[0].y - p[2].y) * (cx - p[2].x)) * inv;
                let w2 = 1.0 - w0 - w1;
                // A small tolerance closes the cracks between neighbors.
                if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                    continue;
                }
                let z = w0 * p[0].z + w1 * p[1].z + w2 * p[2].z;
                let i = y as usize * self.size + x as usize;
                if z < self.depth[i] {
                    self.depth[i] = z;
                }
            }
        }
    }

    /// 1.0 in the light, 0.0 in the shadow, with a soft edge. `ndl` is the cosine of the angle
    /// between the surface and the sun and scales the bias.
    pub fn lit(&self, p: Vec3, ndl: f32) -> f32 {
        let q = self.project(p);
        let slope = (1.0 - ndl * ndl).max(0.0).sqrt() / ndl.max(0.05);
        let bias = self.texel * (1.5 + slope.min(8.0) * 1.2);
        let (cx, cy) = (q.x.floor() as i64, q.y.floor() as i64);
        let n = self.size as i64;
        let mut lit = 0.0;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (x, y) = (cx + dx, cy + dy);
                if x < 0 || y < 0 || x >= n || y >= n {
                    lit += 1.0;
                    continue;
                }
                if q.z - bias <= self.depth[y as usize * self.size + x as usize] {
                    lit += 1.0;
                }
            }
        }
        lit / 9.0
    }
}

/// The view of the sun on a scene, for the GPU shadow map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunView {
    /// Maps the scene so that X and Y run from -1 to 1 and Z (distance from the sun) from 0 to 1.
    pub matrix: glam::Mat4,
    /// Size of the box around the scene in the sun view, in scene units (across, up, depth).
    pub extent: Vec3,
}

/// The view of the sun on the scene. `None` for an empty scene or no direction.
pub fn sun_view(scene: &Scene, to_sun: Vec3) -> Option<SunView> {
    let (lo, hi) = scene.bounds?;
    let l = to_sun.normalize_or_zero();
    if l == Vec3::ZERO {
        return None;
    }
    let helper = if l.z.abs() > 0.99 { Vec3::Y } else { Vec3::Z };
    let right = helper.cross(l).normalize();
    let up = l.cross(right);
    let axis = -l;
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for i in 0..8 {
        let c = Vec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        );
        let p = Vec3::new(c.dot(right), c.dot(up), c.dot(axis));
        min = min.min(p);
        max = max.max(p);
    }
    let ext = (max - min).max(Vec3::splat(1.0));
    // Rows of the matrix: clip = M * world.
    let row =
        |v: Vec3, lo: f32, e: f32, scale: f32, shift: f32| (v / e * scale, -lo / e * scale + shift);
    let (rx, cx) = row(right, min.x, ext.x, 2.0, -1.0);
    let (ry, cy) = row(up, min.y, ext.y, 2.0, -1.0);
    let (rz, cz) = row(axis, min.z, ext.z, 1.0, 0.0);
    Some(SunView {
        matrix: glam::Mat4::from_cols(
            glam::Vec4::new(rx.x, ry.x, rz.x, 0.0),
            glam::Vec4::new(rx.y, ry.y, rz.y, 0.0),
            glam::Vec4::new(rx.z, ry.z, rz.z, 0.0),
            glam::Vec4::new(cx, cy, cz, 1.0),
        ),
        extent: ext,
    })
}

/// A vertex while a triangle is clipped and drawn.
#[derive(Clone, Copy)]
struct Vtx {
    clip: Vec4,
    world: Vec3,
    normal: Vec3,
    uv: [f32; 2],
    color: [f32; 3],
}

impl Vtx {
    fn lerp(a: &Vtx, b: &Vtx, t: f32) -> Vtx {
        Vtx {
            clip: a.clip + (b.clip - a.clip) * t,
            world: a.world + (b.world - a.world) * t,
            normal: a.normal + (b.normal - a.normal) * t,
            uv: [
                a.uv[0] + (b.uv[0] - a.uv[0]) * t,
                a.uv[1] + (b.uv[1] - a.uv[1]) * t,
            ],
            color: [
                a.color[0] + (b.color[0] - a.color[0]) * t,
                a.color[1] + (b.color[1] - a.color[1]) * t,
                a.color[2] + (b.color[2] - a.color[2]) * t,
            ],
        }
    }
}

/// Clips a triangle against the near plane (clip z >= 0).
fn clip_near(v: [Vtx; 3]) -> Vec<Vtx> {
    let mut out = Vec::with_capacity(4);
    for i in 0..3 {
        let (a, b) = (v[i], v[(i + 1) % 3]);
        if a.clip.z >= 0.0 {
            out.push(a);
        }
        if (a.clip.z >= 0.0) != (b.clip.z >= 0.0) {
            out.push(Vtx::lerp(&a, &b, a.clip.z / (a.clip.z - b.clip.z)));
        }
    }
    out
}

/// What a pixel of a triangle knows.
struct Frag {
    world: Vec3,
    normal: Vec3,
    uv: [f32; 2],
    color: [f32; 3],
}

struct Target {
    w: usize,
    h: usize,
    color: Vec<[f32; 3]>,
    depth: Vec<f32>,
}

impl Target {
    fn to_screen(&self, c: Vec4) -> Vec3 {
        Vec3::new(
            (c.x / c.w * 0.5 + 0.5) * self.w as f32,
            (0.5 - c.y / c.w * 0.5) * self.h as f32,
            c.z / c.w,
        )
    }

    /// Draws one clipped triangle. `shade` returns the color and the opacity of a pixel.
    /// Opaque triangles write depth. Blended ones (`blend`) only test it.
    fn triangle(
        &mut self,
        v: [Vtx; 3],
        blend: bool,
        bias: f32,
        shade: &mut dyn FnMut(&Frag) -> ([f32; 3], f32),
    ) {
        let s = [
            self.to_screen(v[0].clip),
            self.to_screen(v[1].clip),
            self.to_screen(v[2].clip),
        ];
        let area = (s[1].x - s[0].x) * (s[2].y - s[0].y) - (s[1].y - s[0].y) * (s[2].x - s[0].x);
        if area == 0.0 || !area.is_finite() {
            return;
        }
        let iw = [1.0 / v[0].clip.w, 1.0 / v[1].clip.w, 1.0 / v[2].clip.w];
        let (w, h) = (self.w as i64, self.h as i64);
        let lo_x = (s.iter().map(|p| p.x).fold(f32::MAX, f32::min).floor() as i64).max(0);
        let hi_x = (s.iter().map(|p| p.x).fold(f32::MIN, f32::max).ceil() as i64).min(w - 1);
        let lo_y = (s.iter().map(|p| p.y).fold(f32::MAX, f32::min).floor() as i64).max(0);
        let hi_y = (s.iter().map(|p| p.y).fold(f32::MIN, f32::max).ceil() as i64).min(h - 1);
        let inv = 1.0 / area;
        for y in lo_y..=hi_y {
            for x in lo_x..=hi_x {
                let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                let b0 =
                    ((s[2].x - s[1].x) * (cy - s[1].y) - (s[2].y - s[1].y) * (cx - s[1].x)) * inv;
                let b1 =
                    ((s[0].x - s[2].x) * (cy - s[2].y) - (s[0].y - s[2].y) * (cx - s[2].x)) * inv;
                let b2 = 1.0 - b0 - b1;
                if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 {
                    continue;
                }
                let z = b0 * s[0].z + b1 * s[1].z + b2 * s[2].z;
                let i = y as usize * self.w + x as usize;
                // Later objects move a little toward the viewer, so coplanar surfaces do not flicker.
                let zb = z - bias;
                if !(0.0..=1.0).contains(&z) || zb > self.depth[i] {
                    continue;
                }
                // Perspective-correct weights.
                let (p0, p1, p2) = (b0 * iw[0], b1 * iw[1], b2 * iw[2]);
                let k = 1.0 / (p0 + p1 + p2);
                let (q0, q1, q2) = (p0 * k, p1 * k, p2 * k);
                let frag = Frag {
                    world: v[0].world * q0 + v[1].world * q1 + v[2].world * q2,
                    normal: v[0].normal * q0 + v[1].normal * q1 + v[2].normal * q2,
                    uv: [
                        v[0].uv[0] * q0 + v[1].uv[0] * q1 + v[2].uv[0] * q2,
                        v[0].uv[1] * q0 + v[1].uv[1] * q1 + v[2].uv[1] * q2,
                    ],
                    color: [
                        v[0].color[0] * q0 + v[1].color[0] * q1 + v[2].color[0] * q2,
                        v[0].color[1] * q0 + v[1].color[1] * q1 + v[2].color[1] * q2,
                        v[0].color[2] * q0 + v[1].color[2] * q1 + v[2].color[2] * q2,
                    ],
                };
                let (c, alpha) = shade(&frag);
                if blend {
                    let d = self.color[i];
                    self.color[i] = [
                        d[0] + (c[0] - d[0]) * alpha,
                        d[1] + (c[1] - d[1]) * alpha,
                        d[2] + (c[2] - d[2]) * alpha,
                    ];
                } else {
                    self.color[i] = c;
                    self.depth[i] = zb;
                }
            }
        }
    }

    fn line(&mut self, a: Vec3, b: Vec3, color: [f32; 3], thick: i64) {
        let (w, h) = (self.w as i64, self.h as i64);
        let steps = (b.x - a.x)
            .abs()
            .max((b.y - a.y).abs())
            .ceil()
            .clamp(1.0, 20_000.0);
        for k in 0..=steps as usize {
            let t = k as f32 / steps;
            let (x, y, z) = (
                a.x + (b.x - a.x) * t,
                a.y + (b.y - a.y) * t,
                a.z + (b.z - a.z) * t,
            );
            if !(0.0..=1.0).contains(&z) {
                continue;
            }
            for dy in 0..thick {
                for dx in 0..thick {
                    let (xi, yi) = (
                        x.floor() as i64 + dx - (thick - 1) / 2,
                        y.floor() as i64 + dy - (thick - 1) / 2,
                    );
                    if xi < 0 || yi < 0 || xi >= w || yi >= h {
                        continue;
                    }
                    let i = yi as usize * self.w + xi as usize;
                    if z <= self.depth[i] + 3e-4 {
                        self.color[i] = color;
                    }
                }
            }
        }
    }
}

fn unit(c: [u8; 4]) -> [f32; 3] {
    [c[0] as f32, c[1] as f32, c[2] as f32]
}

/// Draws the scene with its materials and light.
pub fn render_scene(scene: &Scene, look: &Look, camera: &Camera, opts: &ShadeOptions) -> Image {
    let (w, h) = (opts.width.max(1), opts.height.max(1));
    match opts.mode {
        RenderMode::Wireframe => {
            return crate::raster::render(
                scene,
                camera,
                &RenderOptions {
                    width: w,
                    height: h,
                    mode: RenderMode::Wireframe,
                    background: opts.background,
                },
            );
        }
        RenderMode::HiddenLine | RenderMode::DashedHiddenLine => {
            let lines = hidden_lines(
                scene,
                camera,
                w,
                h,
                &HlOptions {
                    include_hidden: opts.mode == RenderMode::DashedHiddenLine,
                    mesh_edges: true,
                },
            );
            return lines.rasterize(opts.background, opts.line_px);
        }
        RenderMode::Shaded => {}
    }
    let k = opts.samples.clamp(1, 3);
    let (sw, sh) = ((w * k) as usize, (h * k) as usize);
    let bg = unit(opts.background);
    let mut target = Target {
        w: sw,
        h: sh,
        color: vec![bg; sw * sh],
        depth: vec![f32::INFINITY; sw * sh],
    };
    if scene.is_empty() {
        return resolve(&target, w, h, k, opts.background[3]);
    }
    let aspect = w as f32 / h as f32;
    let mvp = camera.view_proj(aspect, scene.bounds);
    let mesh = &scene.mesh;
    let (eye, fwd) = (camera.eye(), camera.forward());
    let ortho = camera.projection == Projection::Orthographic;

    let opacity = |t: usize| look.material_of(t).map_or(1.0, |m| m.opacity);
    let ranks = crate::look::object_ranks(scene);
    let bias_of = |t: usize| {
        ranks.get(&mesh.tri_object[t]).copied().unwrap_or(0) as f32 * crate::look::DEPTH_RANK_STEP
    };
    let shadow_map = match (opts.light, opts.shadows) {
        (Light::Sun(dir), true) => {
            ShadowMap::build(scene, Vec3::from_array(dir), SHADOW_SIZE, &|t| {
                opacity(t) >= 0.5
            })
        }
        _ => None,
    };
    let sun = match opts.light {
        Light::Sun(d) => Some(Vec3::from_array(d).normalize_or_zero()),
        _ => None,
    };

    // Vertices of the scene.
    let vertex = |i: usize| -> Vtx {
        let uv = look.uvs.get(i).copied().unwrap_or([0.0, 0.0]);
        Vtx {
            clip: mvp * mesh.positions[i].extend(1.0),
            world: mesh.positions[i],
            normal: mesh.normals[i],
            uv,
            color: unit(mesh.colors[i]),
        }
    };

    let shade_frag = |t: usize, face_n: Vec3, f: &Frag| -> ([f32; 3], f32) {
        let mat = look.material_of(t);
        let (albedo, alpha, rough, metal) = match mat {
            Some(m) => {
                let base = match m.texture {
                    Some(ti) => {
                        let s = look.textures[ti].sample(f.uv[0], f.uv[1]);
                        [s[0], s[1], s[2]]
                    }
                    None => unit(m.color),
                };
                (base, m.opacity, m.roughness, m.metallic)
            }
            None => (f.color, 1.0, 0.9, 0.0),
        };
        let to_eye = if ortho {
            -fwd
        } else {
            (eye - f.world).normalize_or_zero()
        };
        let mut n = f.normal.normalize_or_zero();
        if n == Vec3::ZERO {
            n = face_n;
        }
        // Both sides are lit: turn the normal toward the viewer.
        if face_n.dot(to_eye) < 0.0 {
            n = -n;
        }
        let (light, spec) = match (opts.light, sun) {
            (Light::Sun(_), Some(l)) => {
                let ndl = n.dot(l).max(0.0);
                let lit = match &shadow_map {
                    Some(sm) if ndl > 0.0 => sm.lit(f.world, ndl),
                    _ => 1.0,
                };
                let hemi = GROUND_AMBIENT + (SKY_AMBIENT - GROUND_AMBIENT) * (n.z * 0.5 + 0.5);
                let diffuse = hemi + SUN_INTENSITY * ndl * lit;
                // Blinn-Phong highlight with a width from the roughness.
                let alpha2 = (rough * rough).powi(2);
                let shin = (2.0 / (alpha2 + 1e-3) - 2.0).clamp(1.0, 512.0);
                let hv = (l + to_eye).normalize_or_zero();
                let f0 = 0.04 + 0.96 * metal;
                let spec = if ndl > 0.0 {
                    f0 * (shin + 2.0) / 25.1
                        * n.dot(hv).max(0.0).powf(shin)
                        * lit
                        * ndl
                        * SUN_INTENSITY
                } else {
                    0.0
                };
                (diffuse * (1.0 - metal * 0.85), spec)
            }
            (Light::Night, _) => (NIGHT_AMBIENT + 0.1 * (n.z * 0.5 + 0.5), 0.0),
            _ => (
                AMBIENT + (1.0 - AMBIENT) * n.dot(LIGHT_DIR.normalize()).max(0.0),
                0.0,
            ),
        };
        let mut out = [0.0; 3];
        for c in 0..3 {
            let spec_color = 255.0 * (1.0 - metal) + albedo[c] * metal;
            out[c] = (albedo[c] * light + spec_color * spec).clamp(0.0, 255.0);
        }
        (out, alpha)
    };

    // Triangles: opaque first, transparent ones later, far to near.
    let mut transparent: Vec<(f32, usize)> = Vec::new();
    let tri_vertices = |t: usize| -> Option<([Vtx; 3], Vec3)> {
        let i = [
            mesh.indices[t * 3] as usize,
            mesh.indices[t * 3 + 1] as usize,
            mesh.indices[t * 3 + 2] as usize,
        ];
        let v = [vertex(i[0]), vertex(i[1]), vertex(i[2])];
        let c = [v[0].clip, v[1].clip, v[2].clip];
        if c.iter().all(|v| v.x < -v.w)
            || c.iter().all(|v| v.x > v.w)
            || c.iter().all(|v| v.y < -v.w)
            || c.iter().all(|v| v.y > v.w)
            || c.iter().all(|v| v.z > v.w)
            || c.iter().all(|v| v.z < 0.0)
        {
            return None;
        }
        let p = mesh.triangle(t);
        let face_n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        Some((v, face_n))
    };
    for t in 0..mesh.triangle_count() {
        if opacity(t) < 0.999 {
            let p = mesh.triangle(t);
            let centroid = (p[0] + p[1] + p[2]) / 3.0;
            let depth = if ortho {
                (centroid - eye).dot(fwd)
            } else {
                (centroid - eye).length()
            };
            transparent.push((depth, t));
            continue;
        }
        let Some((v, face_n)) = tri_vertices(t) else {
            continue;
        };
        let poly = clip_near(v);
        if poly.len() < 3 {
            continue;
        }
        for j in 1..poly.len() - 1 {
            target.triangle(
                [poly[0], poly[j], poly[j + 1]],
                false,
                bias_of(t),
                &mut |f| {
                    let (c, _) = shade_frag(t, face_n, f);
                    (c, 1.0)
                },
            );
        }
    }

    // Edges, with the depth of the opaque surfaces.
    if opts.edges {
        for s in 0..scene.lines.segment_count() {
            let [a, b] = scene.lines.segment(s);
            let (mut ca, mut cb) = (mvp * a.extend(1.0), mvp * b.extend(1.0));
            if ca.z < 0.0 && cb.z < 0.0 {
                continue;
            }
            if ca.z < 0.0 {
                ca = ca + (cb - ca) * (ca.z / (ca.z - cb.z));
            } else if cb.z < 0.0 {
                cb = cb + (ca - cb) * (cb.z / (cb.z - ca.z));
            }
            if ca.w <= 0.0 || cb.w <= 0.0 {
                continue;
            }
            let (pa, pb) = (target.to_screen(ca), target.to_screen(cb));
            target.line(pa, pb, unit(scene.lines.colors[s]), k as i64);
        }
    }

    // Transparent surfaces, far to near.
    transparent.sort_by(|a, b| b.0.partial_cmp(&a.0).expect("finite").then(a.1.cmp(&b.1)));
    for (_, t) in transparent {
        let Some((v, face_n)) = tri_vertices(t) else {
            continue;
        };
        let poly = clip_near(v);
        if poly.len() < 3 {
            continue;
        }
        for j in 1..poly.len() - 1 {
            target.triangle(
                [poly[0], poly[j], poly[j + 1]],
                true,
                bias_of(t),
                &mut |f| shade_frag(t, face_n, f),
            );
        }
    }
    resolve(&target, w, h, k, opts.background[3])
}

/// Averages `k` by `k` samples to one pixel.
fn resolve(t: &Target, w: u32, h: u32, k: u32, alpha: u8) -> Image {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    let n = (k * k) as f32;
    for y in 0..h as usize {
        for x in 0..w as usize {
            let mut sum = [0.0f32; 3];
            for dy in 0..k as usize {
                for dx in 0..k as usize {
                    let c = t.color[(y * k as usize + dy) * t.w + x * k as usize + dx];
                    for ch in 0..3 {
                        sum[ch] += c[ch];
                    }
                }
            }
            rgba.extend([
                (sum[0] / n).round().clamp(0.0, 255.0) as u8,
                (sum[1] / n).round().clamp(0.0, 255.0) as u8,
                (sum[2] / n).round().clamp(0.0, 255.0) as u8,
                alpha,
            ]);
        }
    }
    Image {
        width: w,
        height: h,
        rgba,
    }
}

#[cfg(test)]
mod tests;
