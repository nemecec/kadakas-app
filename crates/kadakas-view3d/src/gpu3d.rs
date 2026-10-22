//! GPU drawing of the 3D scene.
//!
//! egui draws into a pass without a depth buffer, so the scene is drawn in two steps.
//! `Gpu3d::prepare` renders it into its own color and depth textures. `Gpu3d::paint` then copies
//! that texture into the egui pass, into the rectangle of the canvas. `SceneCallback` ties both
//! to an egui paint callback.
//!
//! The renderer draws:
//!
//! - Surfaces with textured, PBR-lite shading. The shader uses the same constants and formulas as
//!   the CPU renderer (`kadakas_render::shade`): a hemisphere ambient light, one sun with a
//!   shadow map, a diffuse part scaled by `1 - metallic` and a Blinn-Phong highlight.
//! - The sun shadow map (one cascade, fitted to the scene): a depth pass from the sun with 3 by 3
//!   comparison filtering. It is redrawn only when the scene, the materials or the sun change.
//! - Transparent surfaces (opacity below 1) in a second pass: alpha blended, far to near, without
//!   writing depth. Their triangles are sorted again whenever the camera moves.
//! - Edges with a depth test. Hidden line modes draw white surfaces and the edges that they do not
//!   cover. The dashed mode adds the covered edges, dashed and gray.
//! - 4 times multisampling (MSAA).
//!
//! The mesh buffers, the material table and the texture array are uploaded only when the scene or
//! the look changes, not every frame. A large plant count stays cheap because plants have a coarse
//! canopy mesh, and one draw call covers all opaque surfaces.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use eframe::egui_wgpu::{self, wgpu};
use glam::Vec3;
use kadakas_core::mesh::Scene;
use kadakas_core::present::RenderMode;
use kadakas_render::look::Look;
use kadakas_render::shade::{
    GROUND_AMBIENT, Light, NIGHT_AMBIENT, SKY_AMBIENT, SUN_INTENSITY, sun_view,
};
use kadakas_render::texture::mip_chain;
use kadakas_render::{AMBIENT, LIGHT_DIR};
use wgpu::util::DeviceExt;

use crate::vertex::{LineVertex, MAX_MATERIALS, SurfaceVertex, line_vertices, surface_vertices};

const SHADER: &str = r#"
struct Globals {
    mvp: mat4x4<f32>,
    light_mvp: mat4x4<f32>,
    // xyz: direction toward the light. w: 0 for the fixed light, 1 for the sun, 2 for night.
    light: vec4<f32>,
    // Position of the eye, or the direction to it for a parallel projection.
    eye: vec4<f32>,
    // x: ambient light of the fixed light, y: depth bias of lines, z: 1 for a parallel projection,
    // w: height of the target in pixels.
    params: vec4<f32>,
    // x: hemisphere sky light, y: hemisphere ground light, z: sun strength, w: night light.
    sunlight: vec4<f32>,
    // x: shadow depth bias per texel, y: texel size in uv, z: 1 when shadows are on,
    // w: width of the target in pixels.
    shadow: vec4<f32>,
    // Background color, used by the flat surfaces of the hidden line modes.
    bg: vec4<f32>,
};
struct Material {
    color: vec4<f32>,
    // x: roughness, y: metallic, z: opacity, w: texture layer or -1.
    params: vec4<f32>,
};
struct Materials {
    m: array<Material, 64>,
};
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var<uniform> mats: Materials;
@group(0) @binding(2) var tex: texture_2d_array<f32>;
@group(0) @binding(3) var samp: sampler;
@group(0) @binding(4) var shadow_tex: texture_depth_2d;
@group(0) @binding(5) var shadow_samp: sampler_comparison;

struct SurfaceIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) material: u32,
    @location(5) order: f32,
    @location(6) flags: u32,
};
struct SurfaceOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world: vec3<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) @interpolate(flat) material: u32,
    @location(5) @interpolate(flat) flags: u32,
};

@vertex
fn vs_surface(v: SurfaceIn) -> SurfaceOut {
    var o: SurfaceOut;
    var c = g.mvp * vec4<f32>(v.pos, 1.0);
    // Later objects move a little toward the viewer, so coplanar surfaces do not flicker.
    c.z = c.z - v.order * 1.0e-6 * c.w;
    o.clip = c;
    o.normal = v.normal;
    o.color = v.color;
    o.world = v.pos;
    o.uv = v.uv;
    o.material = v.material;
    o.flags = v.flags;
    return o;
}

@vertex
fn vs_shadow(@location(0) pos: vec3<f32>) -> @builtin(position) vec4<f32> {
    return g.light_mvp * vec4<f32>(pos, 1.0);
}

// The color and the opacity of a surface point.
fn shade(i: SurfaceOut, front: bool) -> vec4<f32> {
    var albedo = i.color.rgb;
    var alpha = 1.0;
    var rough = 0.9;
    var metal = 0.0;
    let has_material = i.material < 64u;
    let m = mats.m[min(i.material, 63u)];
    // Sampling needs uniform control flow, so always sample and select afterwards.
    let layer = max(i32(m.params.w), 0);
    let texel = textureSample(tex, samp, vec2<f32>(i.uv.x, 1.0 - i.uv.y), layer).rgb;
    if (has_material) {
        albedo = select(m.color.rgb, texel, m.params.w >= 0.0);
        rough = m.params.x;
        metal = m.params.y;
        alpha = m.params.z;
    }

    var to_eye = g.eye.xyz;
    if (g.params.z < 0.5) {
        to_eye = normalize(g.eye.xyz - i.world);
    }
    var n = normalize(i.normal);
    // Both sides are lit: turn the normal toward the viewer.
    if (dot(n, to_eye) < 0.0) {
        n = -n;
    }
    // A face of a surface model (flag 1) shows its back in the back color, unless it has a
    // material: then the material shows on both sides. The side is the one of the triangle
    // (its winding), not of the smoothed normal, so it agrees with the CPU renderers.
    if (!front && (i.flags & 1u) != 0u && !has_material) {
        albedo = vec3<f32>(0.5804, 0.6588, 0.7804);
    }

    // The shadow map is read in uniform control flow too.
    let lp = g.light_mvp * vec4<f32>(i.world, 1.0);
    let suv = vec2<f32>(lp.x * 0.5 + 0.5, 0.5 - lp.y * 0.5);
    let l = normalize(g.light.xyz);
    let ndl = max(dot(n, l), 0.0);
    let slope = min(sqrt(max(1.0 - ndl * ndl, 0.0)) / max(ndl, 0.05), 8.0);
    let bias = g.shadow.x * (1.5 + slope * 1.2);
    var acc = 0.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let o = vec2<f32>(f32(dx), f32(dy)) * g.shadow.y;
            acc += textureSampleCompare(shadow_tex, shadow_samp, suv + o, lp.z - bias);
        }
    }
    let inside = suv.x >= 0.0 && suv.x <= 1.0 && suv.y >= 0.0 && suv.y <= 1.0 && lp.z <= 1.0;
    let lit = select(1.0, acc / 9.0, g.shadow.z > 0.5 && inside && ndl > 0.0);

    var light = g.params.x + (1.0 - g.params.x) * ndl;
    var spec = 0.0;
    if (g.light.w > 1.5) {
        light = g.sunlight.w + 0.1 * (n.z * 0.5 + 0.5);
    } else if (g.light.w > 0.5) {
        let hemi = g.sunlight.y + (g.sunlight.x - g.sunlight.y) * (n.z * 0.5 + 0.5);
        light = (hemi + g.sunlight.z * ndl * lit) * (1.0 - metal * 0.85);
        let alpha2 = pow(rough * rough, 2.0);
        let shin = clamp(2.0 / (alpha2 + 0.001) - 2.0, 1.0, 512.0);
        let hv = normalize(l + to_eye);
        let f0 = 0.04 + 0.96 * metal;
        if (ndl > 0.0) {
            spec = f0 * (shin + 2.0) / 25.1 * pow(max(dot(n, hv), 0.0), shin) * lit * ndl * g.sunlight.z;
        }
    }
    let spec_color = vec3<f32>(1.0 - metal) + albedo * metal;
    return vec4<f32>(clamp(albedo * light + spec_color * spec, vec3<f32>(0.0), vec3<f32>(1.0)), alpha);
}

// True for a triangle that is drawn from one side only (flag 2: the front, flag 4: the back,
// Paint Bucket) and is seen from the other side. `front` is the side of the triangle itself.
fn culled(i: SurfaceOut, front: bool) -> bool {
    return ((i.flags & 2u) != 0u && !front) || ((i.flags & 4u) != 0u && front);
}

@fragment
fn fs_surface(i: SurfaceOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let c = shade(i, front);
    if (culled(i, front)) {
        discard;
    }
    return vec4<f32>(c.rgb, 1.0);
}

@fragment
fn fs_surface_blend(i: SurfaceOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let c = shade(i, front);
    if (culled(i, front)) {
        discard;
    }
    return c;
}

@fragment
fn fs_flat(i: SurfaceOut) -> @location(0) vec4<f32> {
    return vec4<f32>(g.bg.rgb, 1.0);
}

struct LineIn {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) start: vec3<f32>,
};
struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Where the segment starts on the screen, in pixels.
    @location(1) @interpolate(flat) start_px: vec2<f32>,
};

@vertex
fn vs_line(v: LineIn) -> LineOut {
    var o: LineOut;
    var c = g.mvp * vec4<f32>(v.pos, 1.0);
    // Pull lines a little toward the viewer, so edges are not hidden by the faces they bound.
    c.z = c.z - g.params.y * c.w;
    o.clip = c;
    o.color = v.color;
    let s = g.mvp * vec4<f32>(v.start, 1.0);
    // Window pixels: x grows to the right and y down, like the fragment position.
    o.start_px = vec2<f32>(
        (s.x / s.w * 0.5 + 0.5) * g.shadow.w,
        (0.5 - s.y / s.w * 0.5) * g.params.w,
    );
    return o;
}

@fragment
fn fs_line(i: LineOut) -> @location(0) vec4<f32> {
    return vec4<f32>(i.color.rgb, 1.0);
}

// Covered edges: gray dashes, 6 pixels on and 4 off, measured along the line from its start.
@fragment
fn fs_line_hidden(i: LineOut) -> @location(0) vec4<f32> {
    if (length(i.clip.xy - i.start_px) % 10.0 > 6.0) {
        discard;
    }
    return vec4<f32>(0.47, 0.47, 0.47, 1.0);
}

struct BlitOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var scene_tex: texture_2d<f32>;
@group(0) @binding(1) var scene_samp: sampler;

@vertex
fn vs_blit(@builtin(vertex_index) i: u32) -> BlitOut {
    // One triangle that covers the viewport.
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var o: BlitOut;
    o.clip = vec4<f32>(p[i], 0.0, 1.0);
    o.uv = vec2<f32>((p[i].x + 1.0) * 0.5, (1.0 - p[i].y) * 0.5);
    return o;
}

@fragment
fn fs_blit(i: BlitOut) -> @location(0) vec4<f32> {
    return textureSample(scene_tex, scene_samp, i.uv);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

// For a target with an sRGB format: the hardware encodes what we write, so write linear values.
@fragment
fn fs_blit_to_srgb(i: BlitOut) -> @location(0) vec4<f32> {
    let c = textureSample(scene_tex, scene_samp, i.uv);
    return vec4<f32>(srgb_to_linear(c.rgb), c.a);
}
"#;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// The scene is drawn into a texture of this format, whatever the window uses. It has no sRGB
/// encoding, so the colors that the shaders write are the values stored (the same as the CPU
/// renderer writes). The blit step converts them if the window surface is sRGB.
const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Samples per pixel while drawing the scene.
const MSAA: u32 = 4;

/// Depth that lines are pulled toward the viewer, in NDC depth.
const LINE_BIAS: f32 = 3e-4;

/// Side of the shadow map in texels.
const SHADOW_MAP: u32 = 2048;

/// Side of the texture layers for materials, in pixels.
const LAYER_SIZE: u32 = 256;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    mvp: [[f32; 4]; 4],
    light_mvp: [[f32; 4]; 4],
    light: [f32; 4],
    eye: [f32; 4],
    params: [f32; 4],
    sunlight: [f32; 4],
    shadow: [f32; 4],
    bg: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MaterialData {
    color: [f32; 4],
    params: [f32; 4],
}

/// How the scene looks: the mode, the light and the materials.
#[derive(Clone)]
pub struct GpuStyle {
    pub mode: RenderMode,
    /// Draw the edges over shaded surfaces.
    pub edges: bool,
    pub light: Light,
    pub shadows: bool,
    /// The materials of the scene. `None`: every surface keeps its vertex color.
    pub look: Option<Arc<Look>>,
}

impl Default for GpuStyle {
    fn default() -> Self {
        GpuStyle {
            mode: RenderMode::Shaded,
            edges: true,
            light: Light::Fixed,
            shadows: false,
            look: None,
        }
    }
}

/// What to draw in one frame.
pub struct FrameInput<'a> {
    pub scene: &'a Scene,
    /// A fingerprint of the geometry of `scene`. The buffers are rebuilt only when it changes.
    pub scene_rev: u64,
    pub mvp: [[f32; 4]; 4],
    pub eye: [f32; 3],
    /// Unit vector toward the eye, used for the parallel projection.
    pub eye_dir: [f32; 3],
    pub ortho: bool,
    pub style: &'a GpuStyle,
    /// Line list that is drawn over everything (the selection).
    pub overlay: &'a [LineVertex],
    pub overlay_rev: u64,
    pub size: [u32; 2],
    pub background: [f64; 3],
}

/// A transparent triangle, for sorting.
struct BlendTri {
    indices: [u32; 3],
    center: Vec3,
}

/// Splits the triangles of a scene: the indices of the opaque ones (they draw in one call) and
/// the see-through ones (a material with an opacity below 1, such as the ghosts of the compare
/// view), which are sorted for the camera later.
fn split_blend(scene: &Scene, look: Option<&Look>) -> (Vec<u32>, Vec<BlendTri>) {
    let mesh = &scene.mesh;
    let mut opaque: Vec<u32> = Vec::with_capacity(mesh.indices.len());
    let mut blend_tris = Vec::new();
    for t in 0..mesh.triangle_count() {
        let idx = [
            mesh.indices[t * 3],
            mesh.indices[t * 3 + 1],
            mesh.indices[t * 3 + 2],
        ];
        let see_through = look
            .and_then(|l| l.material_of(t))
            .is_some_and(|m| m.opacity < 0.999);
        if see_through {
            let p = mesh.triangle(t);
            blend_tris.push(BlendTri {
                indices: idx,
                center: (p[0] + p[1] + p[2]) / 3.0,
            });
        } else {
            opaque.extend_from_slice(&idx);
        }
    }
    (opaque, blend_tris)
}

/// The indices of the see-through triangles, far to near for a camera at `eye`. `dir` is the
/// direction from the target to the eye (`Camera::eye_dir`), so the camera looks along `-dir`
/// (`ortho`: by the depth along `-dir`, else by the distance from the eye). Equal depths keep the
/// order of the mesh, so the picture is stable.
fn blend_order(tris: &[BlendTri], eye: [f32; 3], dir: [f32; 3], ortho: bool) -> Vec<u32> {
    let (e, d) = (Vec3::from_array(eye), Vec3::from_array(dir));
    let depth = |c: Vec3| {
        if ortho {
            -(c - e).dot(d)
        } else {
            (c - e).length()
        }
    };
    let mut order: Vec<usize> = (0..tris.len()).collect();
    order.sort_by(|a, c| {
        depth(tris[*c].center)
            .partial_cmp(&depth(tris[*a].center))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(c))
    });
    order.iter().flat_map(|i| tris[*i].indices).collect()
}

struct Buffers {
    rev: u64,
    layout_key: u64,
    vertices: Option<wgpu::Buffer>,
    opaque: Option<(wgpu::Buffer, u32)>,
    blend: Option<(wgpu::Buffer, u32)>,
    blend_tris: Vec<BlendTri>,
    /// The camera for which `blend` was sorted.
    sorted_for: Option<[f32; 6]>,
    lines: Option<(wgpu::Buffer, u32)>,
    /// Creases and open edges of the mesh, drawn in the hidden line modes. Built when such a
    /// mode is first used, because finding them costs time on a large scene.
    features: Option<Option<(wgpu::Buffer, u32)>>,
}

/// The sun view that the shadow map was drawn for.
#[derive(PartialEq)]
struct ShadowKey {
    scene_rev: u64,
    layout_key: u64,
    to_sun: [f32; 3],
}

struct Target {
    size: [u32; 2],
    /// Kept for the tests that read the pixels back.
    #[cfg_attr(not(test), allow(dead_code))]
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    msaa_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    blit_group: wgpu::BindGroup,
}

pub struct Gpu3d {
    opaque_pipeline: wgpu::RenderPipeline,
    flat_pipeline: wgpu::RenderPipeline,
    blend_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    line_hidden_pipeline: wgpu::RenderPipeline,
    overlay_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    materials: wgpu::Buffer,
    main_layout: wgpu::BindGroupLayout,
    main_group: wgpu::BindGroup,
    shadow_group: wgpu::BindGroup,
    blit_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    tex_sampler: wgpu::Sampler,
    shadow_sampler: wgpu::Sampler,
    /// The texture array of the materials and its look revision.
    tex_view: wgpu::TextureView,
    tex_rev: Option<u64>,
    /// The shadow map. A 1 by 1 texture until shadows are used.
    shadow_view: wgpu::TextureView,
    shadow_size: u32,
    shadow_key: Option<ShadowKey>,
    buffers: Option<Buffers>,
    overlay: Option<(u64, wgpu::Buffer, u32)>,
    target: Option<Target>,
    /// How often the mesh buffers were uploaded. For tests.
    pub uploads: u64,
    /// How often the shadow map was drawn. For tests.
    pub shadow_draws: u64,
}

/// The key of the vertex data that depends on the look (material indices and texture coordinates).
fn layout_key_of(style: &GpuStyle) -> u64 {
    style.look.as_ref().map_or(0, |l| l.layout_key)
}

fn line_buffer(device: &wgpu::Device, data: &[LineVertex]) -> Option<(wgpu::Buffer, u32)> {
    (!data.is_empty() && (std::mem::size_of_val(data) as u64) < device.limits().max_buffer_size)
        .then(|| {
            let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("kadakas_lines"),
                contents: bytemuck::cast_slice(data),
                usage: wgpu::BufferUsages::VERTEX,
            });
            (buf, data.len() as u32)
        })
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// A 1 by 1 white texture array, for scenes without textures.
fn dummy_array(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
    array_view(device, queue, &[], 1)
}

/// Uploads layers of `LAYER_SIZE` pixels with a full mip chain. With no layers, a white 1 by 1 one.
fn array_view(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layers: &[Vec<u8>],
    count: u32,
) -> wgpu::TextureView {
    let (size, mips, data) = if layers.is_empty() {
        (1, 1, vec![255u8; 4])
    } else {
        (
            LAYER_SIZE,
            LAYER_SIZE.ilog2() + 1,
            layers.iter().flatten().copied().collect::<Vec<u8>>(),
        )
    };
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("kadakas_material_textures"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: count.max(1),
            },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

fn shadow_texture(device: &wgpu::Device, size: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("kadakas_shadow_map"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

impl Gpu3d {
    /// `format` is the format of the surface that egui draws to, sRGB or not.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Gpu3d {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("kadakas_3d"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let both = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let frag = wgpu::ShaderStages::FRAGMENT;
        let main_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kadakas_main_layout"),
            entries: &[
                uniform_entry(0, both),
                uniform_entry(1, frag),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: frag,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: frag,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kadakas_shadow_layout"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX)],
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("kadakas_globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let materials = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("kadakas_materials"),
            size: (std::mem::size_of::<MaterialData>() * MAX_MATERIALS) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tex_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("kadakas_texture_sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 4,
            ..Default::default()
        });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("kadakas_shadow_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let tex_view = dummy_array(device, queue);
        let shadow_view = shadow_texture(device, 1);
        let main_group = make_main_group(
            device,
            &main_layout,
            &globals,
            &materials,
            &tex_view,
            &tex_sampler,
            &shadow_view,
            &shadow_sampler,
        );
        let shadow_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kadakas_shadow_group"),
            layout: &shadow_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("kadakas_3d_layout"),
            bind_group_layouts: &[Some(&main_layout)],
            immediate_size: 0,
        });
        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("kadakas_shadow_pipeline_layout"),
                bind_group_layouts: &[Some(&shadow_layout)],
                immediate_size: 0,
            });
        let depth = |write: bool, compare: wgpu::CompareFunction| {
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(write),
                depth_compare: Some(compare),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            })
        };
        let alpha_blend = wgpu::BlendState::ALPHA_BLENDING;
        let make = |label: &str,
                    vs: &str,
                    fs: &str,
                    stride: u64,
                    attrs: &[wgpu::VertexAttribute],
                    topology,
                    depth_stencil,
                    blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: stride,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: attrs,
                    }],
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    ..Default::default()
                },
                depth_stencil,
                multisample: wgpu::MultisampleState {
                    count: MSAA,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: OFFSCREEN_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let surface_attrs = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Float32x2, 4 => Uint32, 5 => Float32, 6 => Uint32
        ];
        let line_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4, 2 => Float32x3];
        let surface_stride = std::mem::size_of::<SurfaceVertex>() as u64;
        let tri = wgpu::PrimitiveTopology::TriangleList;
        let opaque_pipeline = make(
            "kadakas_surface",
            "vs_surface",
            "fs_surface",
            surface_stride,
            &surface_attrs,
            tri,
            depth(true, wgpu::CompareFunction::LessEqual),
            None,
        );
        let flat_pipeline = make(
            "kadakas_surface_flat",
            "vs_surface",
            "fs_flat",
            surface_stride,
            &surface_attrs,
            tri,
            depth(true, wgpu::CompareFunction::LessEqual),
            None,
        );
        let blend_pipeline = make(
            "kadakas_surface_blend",
            "vs_surface",
            "fs_surface_blend",
            surface_stride,
            &surface_attrs,
            tri,
            depth(false, wgpu::CompareFunction::LessEqual),
            Some(alpha_blend),
        );
        let line_stride = std::mem::size_of::<LineVertex>() as u64;
        let lines = wgpu::PrimitiveTopology::LineList;
        let line_pipeline = make(
            "kadakas_lines",
            "vs_line",
            "fs_line",
            line_stride,
            &line_attrs,
            lines,
            depth(false, wgpu::CompareFunction::LessEqual),
            None,
        );
        let line_hidden_pipeline = make(
            "kadakas_lines_hidden",
            "vs_line",
            "fs_line_hidden",
            line_stride,
            &line_attrs,
            lines,
            depth(false, wgpu::CompareFunction::Greater),
            None,
        );
        let overlay_pipeline = make(
            "kadakas_overlay",
            "vs_line",
            "fs_line",
            line_stride,
            &line_attrs,
            lines,
            depth(false, wgpu::CompareFunction::Always),
            None,
        );
        let shadow_attrs = wgpu::vertex_attr_array![0 => Float32x3];
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("kadakas_shadow"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_shadow"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: surface_stride,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &shadow_attrs,
                }],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });

        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("kadakas_blit_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("kadakas_blit_pipeline_layout"),
            bind_group_layouts: &[Some(&blit_layout)],
            immediate_size: 0,
        });
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("kadakas_blit"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_blit"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(if format.is_srgb() {
                    "fs_blit_to_srgb"
                } else {
                    "fs_blit"
                }),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("kadakas_blit_sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        Gpu3d {
            opaque_pipeline,
            flat_pipeline,
            blend_pipeline,
            line_pipeline,
            line_hidden_pipeline,
            overlay_pipeline,
            shadow_pipeline,
            blit_pipeline,
            globals,
            materials,
            main_layout,
            main_group,
            shadow_group,
            blit_layout,
            sampler,
            tex_sampler,
            shadow_sampler,
            tex_view,
            tex_rev: None,
            shadow_view,
            shadow_size: 1,
            shadow_key: None,
            buffers: None,
            overlay: None,
            target: None,
            uploads: 0,
            shadow_draws: 0,
        }
    }

    fn rebuild_group(&mut self, device: &wgpu::Device) {
        self.main_group = make_main_group(
            device,
            &self.main_layout,
            &self.globals,
            &self.materials,
            &self.tex_view,
            &self.tex_sampler,
            &self.shadow_view,
            &self.shadow_sampler,
        );
    }

    fn ensure_target(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        let size = [size[0].clamp(1, 8192), size[1].clamp(1, 8192)];
        if self.target.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("kadakas_scene_color"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let msaa = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("kadakas_scene_color_msaa"),
            size: extent,
            mip_level_count: 1,
            sample_count: MSAA,
            dimension: wgpu::TextureDimension::D2,
            format: OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("kadakas_scene_depth"),
            size: extent,
            mip_level_count: 1,
            sample_count: MSAA,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let blit_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("kadakas_blit_group"),
            layout: &self.blit_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.target = Some(Target {
            size,
            color,
            color_view,
            msaa_view: msaa.create_view(&wgpu::TextureViewDescriptor::default()),
            depth_view: depth.create_view(&wgpu::TextureViewDescriptor::default()),
            blit_group,
        });
    }

    /// Uploads the mesh, the lines and the material table when the scene or the look changed.
    fn upload_scene(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, input: &FrameInput) {
        let style = input.style;
        let look = style.look.as_deref();
        if self
            .buffers
            .as_ref()
            .is_some_and(|b| b.rev == input.scene_rev && b.layout_key == layout_key_of(style))
        {
            return;
        }
        self.uploads += 1;
        let scene = input.scene;
        let max = device.limits().max_buffer_size;
        let verts = surface_vertices(scene, look);
        let (opaque, blend_tris) = split_blend(scene, look);
        let fits = (std::mem::size_of_val(verts.as_slice()) as u64) < max
            && (std::mem::size_of_val(opaque.as_slice()) as u64) < max;
        let vertices = (fits && !verts.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("kadakas_surface_vertices"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
        let opaque_buf = (vertices.is_some() && !opaque.is_empty()).then(|| {
            let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("kadakas_surface_indices"),
                contents: bytemuck::cast_slice(&opaque),
                usage: wgpu::BufferUsages::INDEX,
            });
            (ib, opaque.len() as u32)
        });
        let blend_buf = (vertices.is_some() && !blend_tris.is_empty()).then(|| {
            let ib = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("kadakas_blend_indices"),
                size: (blend_tris.len() * 12) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            (ib, (blend_tris.len() * 3) as u32)
        });
        let lines = line_buffer(device, &line_vertices(scene));
        self.buffers = Some(Buffers {
            rev: input.scene_rev,
            layout_key: layout_key_of(style),
            vertices,
            opaque: opaque_buf,
            blend: blend_buf,
            blend_tris,
            sorted_for: None,
            lines,
            features: None,
        });
        self.upload_look(device, queue, look, look.map_or(0, |l| l.materials_key));
    }

    /// Writes the material table and the texture array.
    fn upload_look(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        look: Option<&Look>,
        rev: u64,
    ) {
        if self.tex_rev == Some(rev) {
            return;
        }
        self.tex_rev = Some(rev);
        let mut table = [MaterialData::zeroed(); MAX_MATERIALS];
        let mut layers: Vec<Vec<u8>> = Vec::new();
        if let Some(look) = look {
            // Textures that materials use, at most one layer each and 64 in all.
            let mut layer_of = vec![-1.0f32; look.textures.len()];
            for (i, m) in look.materials.iter().take(MAX_MATERIALS).enumerate() {
                let layer = m.texture.filter(|t| *t < MAX_MATERIALS).map_or(-1.0, |t| {
                    if layer_of[t] < 0.0 {
                        layer_of[t] = layers.len() as f32;
                        layers.push(mip_chain(&look.textures[t], LAYER_SIZE));
                    }
                    layer_of[t]
                });
                table[i] = MaterialData {
                    color: m.color.map(|c| c as f32 / 255.0),
                    params: [m.roughness, m.metallic, m.opacity, layer],
                };
            }
        }
        queue.write_buffer(&self.materials, 0, bytemuck::cast_slice(&table));
        self.tex_view = if layers.is_empty() {
            dummy_array(device, queue)
        } else {
            array_view(device, queue, &layers, layers.len() as u32)
        };
        self.rebuild_group(device);
    }

    /// Sorts the see-through triangles far to near for the camera, when the camera moved.
    fn sort_blend(&mut self, queue: &wgpu::Queue, eye: [f32; 3], dir: [f32; 3], ortho: bool) {
        let Some(b) = self.buffers.as_mut() else {
            return;
        };
        let Some((ib, _)) = &b.blend else {
            return;
        };
        let key = [eye[0], eye[1], eye[2], dir[0], dir[1], dir[2]];
        if b.sorted_for == Some(key) {
            return;
        }
        b.sorted_for = Some(key);
        let data = blend_order(&b.blend_tris, eye, dir, ortho);
        queue.write_buffer(ib, 0, bytemuck::cast_slice(&data));
    }

    /// Draws the depth of the opaque surfaces from the sun, if the sun, the scene or the look
    /// changed since the last time.
    fn draw_shadow_map(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        input: &FrameInput,
        to_sun: [f32; 3],
    ) {
        let key = ShadowKey {
            scene_rev: input.scene_rev,
            layout_key: layout_key_of(input.style),
            to_sun,
        };
        if self.shadow_key.as_ref() == Some(&key) {
            return;
        }
        if self.shadow_size != SHADOW_MAP {
            self.shadow_view = shadow_texture(device, SHADOW_MAP);
            self.shadow_size = SHADOW_MAP;
            self.rebuild_group(device);
        }
        self.shadow_key = Some(key);
        self.shadow_draws += 1;
        let Some(b) = self.buffers.as_ref() else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("kadakas_shadow_pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.shadow_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let (Some(vb), Some((ib, count))) = (&b.vertices, &b.opaque) {
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &self.shadow_group, &[]);
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..*count, 0, 0..1);
        }
    }

    /// Draws the scene into the offscreen texture. Call before `paint`, with the egui encoder.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        input: &FrameInput,
    ) {
        let style = input.style;
        self.ensure_target(device, input.size);
        self.upload_scene(device, queue, input);
        if self
            .overlay
            .as_ref()
            .is_none_or(|(rev, _, _)| *rev != input.overlay_rev)
        {
            self.overlay =
                line_buffer(device, input.overlay).map(|(b, n)| (input.overlay_rev, b, n));
        }
        // The sun, its view of the scene and the table of light constants.
        let (light_dir, light_mode) = match style.light {
            Light::Fixed => (LIGHT_DIR.normalize().to_array(), 0.0),
            Light::Sun(d) => (d, 1.0),
            Light::Night => ([0.0, 0.0, 1.0], 2.0),
        };
        let sun = match style.light {
            Light::Sun(d) if style.shadows => sun_view(input.scene, Vec3::from_array(d)),
            _ => None,
        };
        let shadows_on = sun.is_some() && !matches!(style.mode, RenderMode::Wireframe);
        let (light_mvp, bias, texel) = match &sun {
            Some(v) if shadows_on => {
                // The size of a shadow texel in scene units, as a share of the depth range.
                let world_texel = v.extent.x.max(v.extent.y) / SHADOW_MAP as f32;
                (
                    v.matrix.to_cols_array_2d(),
                    world_texel / v.extent.z,
                    1.0 / SHADOW_MAP as f32,
                )
            }
            _ => (glam::Mat4::IDENTITY.to_cols_array_2d(), 0.0, 1.0),
        };
        let globals = Globals {
            mvp: input.mvp,
            light_mvp,
            light: [light_dir[0], light_dir[1], light_dir[2], light_mode],
            eye: if input.ortho {
                [input.eye_dir[0], input.eye_dir[1], input.eye_dir[2], 0.0]
            } else {
                [input.eye[0], input.eye[1], input.eye[2], 1.0]
            },
            params: [
                AMBIENT,
                LINE_BIAS,
                f32::from(input.ortho),
                input.size[1].clamp(1, 8192) as f32,
            ],
            sunlight: [SKY_AMBIENT, GROUND_AMBIENT, SUN_INTENSITY, NIGHT_AMBIENT],
            shadow: [
                bias,
                texel,
                f32::from(shadows_on),
                input.size[0].clamp(1, 8192) as f32,
            ],
            bg: [
                input.background[0] as f32,
                input.background[1] as f32,
                input.background[2] as f32,
                1.0,
            ],
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        if shadows_on && let Light::Sun(d) = style.light {
            self.draw_shadow_map(device, encoder, input, d);
        }
        self.sort_blend(queue, input.eye, input.eye_dir, input.ortho);
        if style.mode.is_hidden_line()
            && let Some(b) = self.buffers.as_mut()
            && b.features.is_none()
        {
            let features: Vec<LineVertex> = kadakas_render::hlr::feature_edges(input.scene)
                .into_iter()
                .flat_map(|(seg, color)| {
                    LineVertex::segment(seg[0].to_array(), seg[1].to_array(), color)
                })
                .collect();
            b.features = Some(line_buffer(device, &features));
        }

        let target = self.target.as_ref().expect("target was just created");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("kadakas_scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.msaa_view,
                depth_slice: None,
                resolve_target: Some(&target.color_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: input.background[0],
                        g: input.background[1],
                        b: input.background[2],
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.main_group, &[]);
        let buffers = self.buffers.as_ref().expect("buffers were just created");
        let mode = style.mode;
        let draw_surfaces = mode != RenderMode::Wireframe;
        if draw_surfaces && let Some(vb) = &buffers.vertices {
            // Hidden line modes fill with the background color, so only the edges that no surface covers show.
            let pipeline = if mode.is_hidden_line() {
                &self.flat_pipeline
            } else {
                &self.opaque_pipeline
            };
            if let Some((ib, count)) = &buffers.opaque {
                pass.set_pipeline(pipeline);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
            }
            if !mode.is_hidden_line()
                && let Some((ib, count)) = &buffers.blend
                && !style.edges
            {
                // Without edges the see-through surfaces are drawn now. With edges they come after the lines.
                pass.set_pipeline(&self.blend_pipeline);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
            } else if mode.is_hidden_line()
                && let Some((ib, count)) = &buffers.blend
            {
                pass.set_pipeline(&self.flat_pipeline);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
            }
        }
        let lines_on = mode != RenderMode::Shaded || style.edges;
        if lines_on {
            let features = buffers.features.as_ref().and_then(|f| f.as_ref());
            // Mesh edges only help in the hidden line modes.
            let features = features.filter(|_| mode.is_hidden_line());
            for (vb, count) in [buffers.lines.as_ref(), features].into_iter().flatten() {
                if mode == RenderMode::DashedHiddenLine {
                    pass.set_pipeline(&self.line_hidden_pipeline);
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.draw(0..*count, 0..1);
                }
                pass.set_pipeline(&self.line_pipeline);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.draw(0..*count, 0..1);
            }
        }
        // See-through surfaces over the edges, far to near.
        if mode == RenderMode::Shaded
            && style.edges
            && let (Some(vb), Some((ib, count))) = (&buffers.vertices, &buffers.blend)
        {
            pass.set_pipeline(&self.blend_pipeline);
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..*count, 0, 0..1);
        }
        if let Some((_, vb, count)) = &self.overlay {
            pass.set_pipeline(&self.overlay_pipeline);
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.draw(0..*count, 0..1);
        }
    }

    /// Copies the offscreen texture into the current pass, over the viewport of the callback.
    pub fn paint(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(t) = &self.target {
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &t.blit_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// The offscreen color texture. For tests.
    #[cfg(test)]
    pub fn color_texture(&self) -> Option<&wgpu::Texture> {
        self.target.as_ref().map(|t| &t.color)
    }
}

#[allow(clippy::too_many_arguments)]
fn make_main_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    globals: &wgpu::Buffer,
    materials: &wgpu::Buffer,
    tex: &wgpu::TextureView,
    tex_sampler: &wgpu::Sampler,
    shadow: &wgpu::TextureView,
    shadow_sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("kadakas_main_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: materials.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(tex),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(tex_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(shadow),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Sampler(shadow_sampler),
            },
        ],
    })
}

/// Paint callback of the 3D canvas. It owns what it needs, because egui runs it later,
/// from the render thread.
pub struct SceneCallback {
    pub scene: Arc<Scene>,
    pub scene_rev: u64,
    pub style: GpuStyle,
    pub overlay: Arc<Vec<LineVertex>>,
    pub overlay_rev: u64,
    pub mvp: [[f32; 4]; 4],
    pub eye: [f32; 3],
    pub eye_dir: [f32; 3],
    pub ortho: bool,
    pub size: [u32; 2],
    pub background: [f64; 3],
}

impl egui_wgpu::CallbackTrait for SceneCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(gpu) = resources.get_mut::<Gpu3d>() {
            gpu.prepare(
                device,
                queue,
                encoder,
                &FrameInput {
                    scene: &self.scene,
                    scene_rev: self.scene_rev,
                    mvp: self.mvp,
                    eye: self.eye,
                    eye_dir: self.eye_dir,
                    ortho: self.ortho,
                    style: &self.style,
                    overlay: &self.overlay,
                    overlay_rev: self.overlay_rev,
                    size: self.size,
                    background: self.background,
                },
            );
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(gpu) = resources.get::<Gpu3d>() {
            gpu.paint(pass);
        }
    }
}

#[cfg(test)]
mod tests;
