//! GPU drawing of the 3D scene.
//!
//! egui draws into a pass without a depth buffer, so the scene is drawn in two steps.
//! `Gpu3d::prepare` renders it into its own color and depth textures (Lambert shading
//! plus a line pipeline for the edges). `Gpu3d::paint` then copies that texture into
//! the egui pass, into the rectangle of the canvas. `SceneCallback` ties both to an
//! egui paint callback.
//!
//! The mesh buffers are uploaded only when the scene revision changes, not every frame.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use cadektor_core::mesh::Scene;
use cadektor_render::{AMBIENT, LIGHT_DIR};
use eframe::egui_wgpu::{self, wgpu};
use wgpu::util::DeviceExt;

use crate::vertex::{LineVertex, SurfaceVertex, line_vertices, surface_vertices};

const SHADER: &str = r#"
struct Globals {
    mvp: mat4x4<f32>,
    light: vec4<f32>,
    // Position of the eye, or the direction to it for a parallel projection.
    eye: vec4<f32>,
    // x: ambient light, y: depth bias of lines, z: 1 for a parallel projection.
    params: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;

struct SurfaceIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};
struct SurfaceOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world: vec3<f32>,
};

@vertex
fn vs_surface(v: SurfaceIn) -> SurfaceOut {
    var o: SurfaceOut;
    o.clip = g.mvp * vec4<f32>(v.pos, 1.0);
    o.normal = v.normal;
    o.color = v.color;
    o.world = v.pos;
    return o;
}

@fragment
fn fs_surface(i: SurfaceOut) -> @location(0) vec4<f32> {
    var n = normalize(i.normal);
    var to_eye = g.eye.xyz;
    if (g.params.z < 0.5) {
        to_eye = normalize(g.eye.xyz - i.world);
    }
    // Both sides are lit: turn the normal toward the viewer.
    if (dot(n, to_eye) < 0.0) {
        n = -n;
    }
    let lit = g.params.x + (1.0 - g.params.x) * max(dot(n, g.light.xyz), 0.0);
    return vec4<f32>(i.color.rgb * lit, 1.0);
}

struct LineIn {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec4<f32>,
};
struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(v: LineIn) -> LineOut {
    var o: LineOut;
    var c = g.mvp * vec4<f32>(v.pos, 1.0);
    // Pull lines a little toward the viewer, so edges are not hidden by the faces they bound.
    c.z = c.z - g.params.y * c.w;
    o.clip = c;
    o.color = v.color;
    return o;
}

@fragment
fn fs_line(i: LineOut) -> @location(0) vec4<f32> {
    return vec4<f32>(i.color.rgb, 1.0);
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

/// Depth that lines are pulled toward the viewer, in NDC depth.
const LINE_BIAS: f32 = 3e-4;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    mvp: [[f32; 4]; 4],
    light: [f32; 4],
    eye: [f32; 4],
    params: [f32; 4],
}

/// What to draw in one frame.
pub struct FrameInput<'a> {
    pub scene: &'a Scene,
    /// Changes whenever `scene` changes. The buffers are rebuilt only then.
    pub scene_rev: u64,
    pub mvp: [[f32; 4]; 4],
    pub eye: [f32; 3],
    /// Unit vector toward the eye, used for the parallel projection.
    pub eye_dir: [f32; 3],
    pub ortho: bool,
    pub wireframe: bool,
    /// Line list that is drawn over everything (the selection).
    pub overlay: &'a [LineVertex],
    pub overlay_rev: u64,
    pub size: [u32; 2],
    pub background: [f64; 3],
}

struct Buffers {
    rev: u64,
    surface: Option<(wgpu::Buffer, wgpu::Buffer, u32)>,
    lines: Option<(wgpu::Buffer, u32)>,
}

struct Target {
    size: [u32; 2],
    /// Kept for the tests that read the pixels back.
    #[cfg_attr(not(test), allow(dead_code))]
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    blit_group: wgpu::BindGroup,
}

pub struct Gpu3d {
    surface_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    overlay_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    blit_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    buffers: Option<Buffers>,
    overlay: Option<(u64, wgpu::Buffer, u32)>,
    target: Option<Target>,
    /// How often the mesh buffers were uploaded. For tests.
    pub uploads: u64,
}

fn line_buffer(device: &wgpu::Device, data: &[LineVertex]) -> Option<(wgpu::Buffer, u32)> {
    (!data.is_empty() && (std::mem::size_of_val(data) as u64) < device.limits().max_buffer_size)
        .then(|| {
            let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cadektor_lines"),
                contents: bytemuck::cast_slice(data),
                usage: wgpu::BufferUsages::VERTEX,
            });
            (buf, data.len() as u32)
        })
}

impl Gpu3d {
    /// `format` is the format of the surface that egui draws to, sRGB or not.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Gpu3d {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cadektor_3d"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cadektor_globals_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadektor_globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cadektor_globals_group"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cadektor_3d_layout"),
            bind_group_layouts: &[Some(&globals_layout)],
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
        let make = |label: &str,
                    vs: &str,
                    fs: &str,
                    stride: u64,
                    attrs: &[wgpu::VertexAttribute],
                    topology,
                    depth_stencil| {
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
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: OFFSCREEN_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let surface_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4];
        let line_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4];
        let surface_pipeline = make(
            "cadektor_surface",
            "vs_surface",
            "fs_surface",
            std::mem::size_of::<SurfaceVertex>() as u64,
            &surface_attrs,
            wgpu::PrimitiveTopology::TriangleList,
            depth(true, wgpu::CompareFunction::LessEqual),
        );
        let line_stride = std::mem::size_of::<LineVertex>() as u64;
        let line_pipeline = make(
            "cadektor_lines",
            "vs_line",
            "fs_line",
            line_stride,
            &line_attrs,
            wgpu::PrimitiveTopology::LineList,
            depth(false, wgpu::CompareFunction::LessEqual),
        );
        let overlay_pipeline = make(
            "cadektor_overlay",
            "vs_line",
            "fs_line",
            line_stride,
            &line_attrs,
            wgpu::PrimitiveTopology::LineList,
            depth(false, wgpu::CompareFunction::Always),
        );

        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cadektor_blit_layout"),
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
            label: Some("cadektor_blit_pipeline_layout"),
            bind_group_layouts: &[Some(&blit_layout)],
            immediate_size: 0,
        });
        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cadektor_blit"),
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
            label: Some("cadektor_blit_sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        Gpu3d {
            surface_pipeline,
            line_pipeline,
            overlay_pipeline,
            blit_pipeline,
            globals,
            globals_group,
            blit_layout,
            sampler,
            buffers: None,
            overlay: None,
            target: None,
            uploads: 0,
        }
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
            label: Some("cadektor_scene_color"),
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
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cadektor_scene_depth"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let blit_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cadektor_blit_group"),
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
            depth_view: depth.create_view(&wgpu::TextureViewDescriptor::default()),
            blit_group,
        });
    }

    fn upload_scene(&mut self, device: &wgpu::Device, scene: &Scene, rev: u64) {
        if self.buffers.as_ref().is_some_and(|b| b.rev == rev) {
            return;
        }
        self.uploads += 1;
        let max = device.limits().max_buffer_size;
        let verts = surface_vertices(scene);
        let surface = (!scene.mesh.indices.is_empty()
            && (std::mem::size_of_val(verts.as_slice()) as u64) < max
            && (std::mem::size_of_val(scene.mesh.indices.as_slice()) as u64) < max)
            .then(|| {
                let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cadektor_surface_vertices"),
                    contents: bytemuck::cast_slice(&verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("cadektor_surface_indices"),
                    contents: bytemuck::cast_slice(&scene.mesh.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });
                (vb, ib, scene.mesh.indices.len() as u32)
            });
        let lines = line_buffer(device, &line_vertices(scene));
        self.buffers = Some(Buffers {
            rev,
            surface,
            lines,
        });
    }

    /// Draws the scene into the offscreen texture. Call before `paint`, with the egui encoder.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        input: &FrameInput,
    ) {
        self.ensure_target(device, input.size);
        self.upload_scene(device, input.scene, input.scene_rev);
        if self
            .overlay
            .as_ref()
            .is_none_or(|(rev, _, _)| *rev != input.overlay_rev)
        {
            self.overlay =
                line_buffer(device, input.overlay).map(|(b, n)| (input.overlay_rev, b, n));
        }
        let globals = Globals {
            mvp: input.mvp,
            light: {
                let l = LIGHT_DIR.normalize();
                [l.x, l.y, l.z, 0.0]
            },
            eye: if input.ortho {
                [input.eye_dir[0], input.eye_dir[1], input.eye_dir[2], 0.0]
            } else {
                [input.eye[0], input.eye[1], input.eye[2], 1.0]
            },
            params: [AMBIENT, LINE_BIAS, f32::from(input.ortho), 0.0],
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        let target = self.target.as_ref().expect("target was just created");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("cadektor_scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: input.background[0],
                        g: input.background[1],
                        b: input.background[2],
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
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
        pass.set_bind_group(0, &self.globals_group, &[]);
        let buffers = self.buffers.as_ref().expect("buffers were just created");
        if !input.wireframe
            && let Some((vb, ib, count)) = &buffers.surface
        {
            pass.set_pipeline(&self.surface_pipeline);
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..*count, 0, 0..1);
        }
        if let Some((vb, count)) = &buffers.lines {
            pass.set_pipeline(&self.line_pipeline);
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.draw(0..*count, 0..1);
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

/// Paint callback of the 3D canvas. It owns what it needs, because egui runs it later,
/// from the render thread.
pub struct SceneCallback {
    pub scene: Arc<Scene>,
    pub scene_rev: u64,
    pub overlay: Arc<Vec<LineVertex>>,
    pub overlay_rev: u64,
    pub mvp: [[f32; 4]; 4],
    pub eye: [f32; 3],
    pub eye_dir: [f32; 3],
    pub ortho: bool,
    pub wireframe: bool,
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
                    wireframe: self.wireframe,
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
mod tests {
    use super::*;
    use cadektor_core::command::{Command, Editor};
    use cadektor_core::mesh::build;
    use cadektor_core::model::{Attributes, Color, Shape};
    use cadektor_render::{Camera, Projection, StandardView};
    use glam::DVec2;

    /// A device without a window, or `None` on a machine without a usable GPU.
    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::default();
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
    }

    /// Reads the RGBA8 pixels of the offscreen texture.
    fn read(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        gpu: &Gpu3d,
        size: [u32; 2],
        encoder: wgpu::CommandEncoder,
    ) -> Vec<u8> {
        let mut encoder = encoder;
        let row = (size[0] * 4).next_multiple_of(256);
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * size[1]) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: gpu.color_texture().unwrap(),
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(size[1]),
                },
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = buf.slice(..).get_mapped_range();
        let mut out = Vec::new();
        for y in 0..size[1] as usize {
            out.extend_from_slice(&data[y * row as usize..y * row as usize + size[0] as usize * 4]);
        }
        out
    }

    /// Draws a scene with the real shaders on the machine's GPU and checks some pixels.
    /// It does nothing on a machine without a GPU.
    #[test]
    fn shaders_compile_and_draw_what_the_cpu_renderer_draws() {
        let Some((device, queue)) = device() else {
            // Rust tests cannot be marked skipped at run time, so say it loudly. CI that must have a GPU sets the variable.
            assert!(
                std::env::var_os("CADEKTOR_REQUIRE_GPU").is_none(),
                "CADEKTOR_REQUIRE_GPU is set but there is no GPU adapter"
            );
            eprintln!(
                "SKIPPED shaders_compile_and_draw_what_the_cpu_renderer_draws: no GPU adapter on this machine"
            );
            return;
        };
        assert_eq!(std::mem::size_of::<Globals>(), 112);
        // Window formats with sRGB encoding use the other blit entry point; both must be valid.
        let _ = Gpu3d::new(&device, wgpu::TextureFormat::Bgra8UnormSrgb);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut gpu = Gpu3d::new(&device, format);
        let mut ed = Editor::default();
        ed.execute(&Command::AddObject {
            shape: Shape::Rect {
                min: DVec2::splat(-500.0),
                max: DVec2::splat(500.0),
            },
            layer: None,
            class: None,
            attrs: Attributes {
                fill: Some(Color([200, 30, 30, 255])),
                ..Default::default()
            },
        })
        .unwrap();
        let scene = build(ed.doc());
        let size = [100u32, 100u32];
        let mut cam = Camera::standard(StandardView::Top, Projection::Orthographic);
        cam.distance = 1000.0 / (cadektor_render::FOV_Y / 2.0).tan();
        let mvp = cam.view_proj(1.0, scene.bounds).to_cols_array_2d();
        let overlay = [
            LineVertex {
                pos: [-900.0, 810.0, 0.0],
                color: [0, 0, 255, 255],
            },
            LineVertex {
                pos: [900.0, 810.0, 0.0],
                color: [0, 0, 255, 255],
            },
        ];
        let draw = |gpu: &mut Gpu3d, wireframe: bool| {
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            gpu.prepare(
                &device,
                &queue,
                &mut encoder,
                &FrameInput {
                    scene: &scene,
                    scene_rev: ed.revision(),
                    mvp,
                    eye: cam.eye().to_array(),
                    eye_dir: cam.eye_dir().to_array(),
                    ortho: true,
                    wireframe,
                    overlay: &overlay,
                    overlay_rev: 1,
                    size,
                    background: [1.0, 1.0, 1.0],
                },
            );
            read(&device, &queue, gpu, size, encoder)
        };
        let px = |img: &[u8], x: usize, y: usize| {
            [
                img[(y * 100 + x) * 4],
                img[(y * 100 + x) * 4 + 1],
                img[(y * 100 + x) * 4 + 2],
            ]
        };

        let img = draw(&mut gpu, false);
        let lit = cadektor_render::raster::lambert(glam::Vec3::Z);
        let c = px(&img, 50, 50);
        assert!(
            (c[0] as f32 - 200.0 * lit).abs() <= 2.0 && c[1] < 100,
            "face color {c:?}"
        );
        assert_eq!(px(&img, 10, 50), [255, 255, 255], "background");
        assert_eq!(px(&img, 50, 9), [0, 0, 255], "overlay line");
        assert_eq!(gpu.uploads, 1);

        // A second frame of the same scene revision does not upload the mesh again.
        let img2 = draw(&mut gpu, false);
        assert_eq!(gpu.uploads, 1);
        assert_eq!(img, img2);

        // Wireframe: no fill, but the outline.
        let img = draw(&mut gpu, true);
        assert_eq!(px(&img, 50, 50), [255, 255, 255]);
        assert!(
            px(&img, 24, 50) != [255, 255, 255] || px(&img, 25, 50) != [255, 255, 255],
            "outline"
        );

        // A new revision uploads again.
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        gpu.prepare(
            &device,
            &queue,
            &mut encoder,
            &FrameInput {
                scene: &Scene::default(),
                scene_rev: ed.revision() + 1,
                mvp,
                eye: [0.0; 3],
                eye_dir: [0.0, 0.0, 1.0],
                ortho: false,
                wireframe: false,
                overlay: &[],
                overlay_rev: 2,
                size: [8, 8],
                background: [0.0; 3],
            },
        );
        queue.submit([encoder.finish()]);
        assert_eq!(gpu.uploads, 2);
    }
}
