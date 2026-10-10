use super::*;
use cadektor_core::command::{Command, Editor};
use cadektor_core::mesh::build;
use cadektor_core::model::{Attributes, Color, Shape};
use cadektor_core::present::{
    MappingKind, Material, Procedural, RenderResource, SlotTarget, SunSettings, Texture,
    TextureMapping,
};
use cadektor_core::resource::{ResourceBody, ResourceId};
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

/// The device, or a loud note that the test is skipped.
fn device_or_skip(name: &str) -> Option<(wgpu::Device, wgpu::Queue)> {
    let d = device();
    if d.is_none() {
        // Rust tests cannot be marked skipped at run time, so say it loudly. CI that must have a GPU sets the variable.
        assert!(
            std::env::var_os("CADEKTOR_REQUIRE_GPU").is_none(),
            "CADEKTOR_REQUIRE_GPU is set but there is no GPU adapter"
        );
        eprintln!("SKIPPED {name}: no GPU adapter on this machine");
    }
    d
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

fn add_rect(ed: &mut Editor, half: f64, fill: [u8; 4]) -> cadektor_core::model::ObjectId {
    cadektor_core::model::ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Rect {
                min: DVec2::splat(-half),
                max: DVec2::splat(half),
            },
            layer: None,
            class: None,
            attrs: Attributes {
                fill: Some(Color(fill)),
                ..Default::default()
            },
        })
        .unwrap()
        .created[0],
    )
}

fn add_slab(
    ed: &mut Editor,
    x0: f64,
    y0: f64,
    side: f64,
    thickness: f64,
    fill: [u8; 4],
) -> cadektor_core::model::ObjectId {
    let pts = [
        DVec2::new(x0, y0),
        DVec2::new(x0 + side, y0),
        DVec2::new(x0 + side, y0 + side),
        DVec2::new(x0, y0 + side),
    ];
    cadektor_core::model::ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Hardscape {
                boundary: cadektor_core::curve::corners(&pts),
                material: "m".into(),
                thickness: Some(thickness),
                elevation: Some(thickness),
            },
            layer: None,
            class: None,
            attrs: Attributes {
                fill: Some(Color(fill)),
                ..Default::default()
            },
        })
        .unwrap()
        .created[0],
    )
}

fn add_material(ed: &mut Editor, name: &str, m: Material) -> ResourceId {
    ResourceId(
        ed.execute(&Command::AddResource {
            name: name.into(),
            body: ResourceBody::Render(Box::new(RenderResource::Material(m))),
        })
        .unwrap()
        .created[0],
    )
}

fn assign(ed: &mut Editor, object: cadektor_core::model::ObjectId, m: ResourceId) {
    ed.execute(&Command::SetMaterialSlot {
        target: SlotTarget::Object { object, slot: None },
        material: Some(m),
    })
    .unwrap();
}

/// Draws one frame of the editor's scene and returns the pixels.
struct Rig {
    device: wgpu::Device,
    queue: wgpu::Queue,
    gpu: Gpu3d,
    size: [u32; 2],
}

impl Rig {
    fn new(device: wgpu::Device, queue: wgpu::Queue, size: [u32; 2]) -> Rig {
        let gpu = Gpu3d::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
        Rig {
            device,
            queue,
            gpu,
            size,
        }
    }

    fn draw(
        &mut self,
        ed: &Editor,
        cam: &Camera,
        style: &GpuStyle,
        overlay: &[LineVertex],
    ) -> Vec<u8> {
        let scene = build(ed.doc());
        self.draw_scene(&scene, ed.revision(), cam, style, overlay)
    }

    fn draw_scene(
        &mut self,
        scene: &Scene,
        rev: u64,
        cam: &Camera,
        style: &GpuStyle,
        overlay: &[LineVertex],
    ) -> Vec<u8> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let aspect = self.size[0] as f32 / self.size[1] as f32;
        self.gpu.prepare(
            &self.device,
            &self.queue,
            &mut encoder,
            &FrameInput {
                scene,
                scene_rev: rev,
                mvp: cam.view_proj(aspect, scene.bounds).to_cols_array_2d(),
                eye: cam.eye().to_array(),
                eye_dir: cam.eye_dir().to_array(),
                ortho: cam.projection == Projection::Orthographic,
                style,
                overlay,
                overlay_rev: 1,
                size: self.size,
                background: [1.0, 1.0, 1.0],
            },
        );
        read(&self.device, &self.queue, &self.gpu, self.size, encoder)
    }
}

fn px(img: &[u8], w: usize, x: usize, y: usize) -> [u8; 3] {
    [
        img[(y * w + x) * 4],
        img[(y * w + x) * 4 + 1],
        img[(y * w + x) * 4 + 2],
    ]
}

fn lum(p: [u8; 3]) -> f32 {
    0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32
}

fn top_cam(half: f32) -> Camera {
    let mut cam = Camera::standard(StandardView::Top, Projection::Orthographic);
    cam.distance = half / (cadektor_render::FOV_Y / 2.0).tan();
    cam
}

fn style_for(ed: &Editor, scene_look: bool, f: impl FnOnce(&mut GpuStyle)) -> GpuStyle {
    let mut s = GpuStyle::default();
    if scene_look {
        let scene = build(ed.doc());
        s.look = Some(Arc::new(Look::build(ed.doc(), &scene, true)));
        s.look_rev = ed.revision();
    }
    f(&mut s);
    s
}

/// Draws a scene with the real shaders on the machine's GPU and checks some pixels.
/// It does nothing on a machine without a GPU.
#[test]
fn shaders_compile_and_draw_what_the_cpu_renderer_draws() {
    let Some((device, queue)) =
        device_or_skip("shaders_compile_and_draw_what_the_cpu_renderer_draws")
    else {
        return;
    };
    assert_eq!(std::mem::size_of::<Globals>(), 224);
    assert_eq!(std::mem::size_of::<MaterialData>(), 32);
    // Window formats with sRGB encoding use the other blit entry point; both must be valid.
    let _ = Gpu3d::new(&device, &queue, wgpu::TextureFormat::Bgra8UnormSrgb);
    let mut rig = Rig::new(device, queue, [100, 100]);
    let mut ed = Editor::default();
    add_rect(&mut ed, 500.0, [200, 30, 30, 255]);
    let cam = top_cam(1000.0);
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
    let style = GpuStyle::default();
    let img = rig.draw(&ed, &cam, &style, &overlay);
    let lit = cadektor_render::raster::lambert(glam::Vec3::Z);
    let c = px(&img, 100, 50, 50);
    assert!(
        (c[0] as f32 - 200.0 * lit).abs() <= 2.0 && c[1] < 100,
        "face color {c:?}"
    );
    assert_eq!(px(&img, 100, 10, 50), [255, 255, 255], "background");
    assert_eq!(px(&img, 100, 50, 9), [0, 0, 255], "overlay line");
    assert_eq!(rig.gpu.uploads, 1);

    // A second frame of the same scene revision does not upload the mesh again.
    let img2 = rig.draw(&ed, &cam, &style, &overlay);
    assert_eq!(rig.gpu.uploads, 1);
    assert_eq!(img, img2);

    // Wireframe: no fill, but the outline.
    let wire = GpuStyle {
        mode: RenderMode::Wireframe,
        ..GpuStyle::default()
    };
    let img = rig.draw(&ed, &cam, &wire, &overlay);
    assert_eq!(px(&img, 100, 50, 50), [255, 255, 255]);
    assert!(
        px(&img, 100, 24, 50) != [255, 255, 255] || px(&img, 100, 25, 50) != [255, 255, 255],
        "outline"
    );

    // Edges off: the outline is gone from the filled face.
    let no_edges = GpuStyle {
        edges: false,
        ..GpuStyle::default()
    };
    let img = rig.draw(&ed, &cam, &no_edges, &[]);
    assert_eq!(px(&img, 100, 50, 50)[0], c[0]);

    // A new revision uploads again.
    let before = rig.gpu.uploads;
    let mut encoder = rig
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let empty_style = GpuStyle::default();
    rig.gpu.prepare(
        &rig.device,
        &rig.queue,
        &mut encoder,
        &FrameInput {
            scene: &Scene::default(),
            scene_rev: ed.revision() + 1,
            mvp: cam.view_proj(1.0, None).to_cols_array_2d(),
            eye: [0.0; 3],
            eye_dir: [0.0, 0.0, 1.0],
            ortho: false,
            style: &empty_style,
            overlay: &[],
            overlay_rev: 2,
            size: [8, 8],
            background: [0.0; 3],
        },
    );
    rig.queue.submit([encoder.finish()]);
    assert_eq!(rig.gpu.uploads, before + 1);
}

#[test]
fn textures_reach_the_screen_and_match_the_cpu_colors_on_average() {
    let Some((device, queue)) = device_or_skip("textures_reach_the_screen") else {
        return;
    };
    let mut rig = Rig::new(device, queue, [128, 128]);
    let mut ed = Editor::default();
    let ground = add_rect(&mut ed, 2000.0, [200, 200, 200, 255]);
    let m = add_material(
        &mut ed,
        "Pavers",
        Material {
            base_color: Color([190, 190, 185, 255]),
            texture: Some(Texture::Procedural {
                kind: Procedural::Pavers,
                seed: 1,
            }),
            mapping: TextureMapping {
                kind: MappingKind::Planar,
                scale: [1600.0, 1600.0],
                ..Default::default()
            },
            ..Default::default()
        },
    );
    assign(&mut ed, ground, m);
    let cam = top_cam(2500.0);
    let plain = rig.draw(&ed, &cam, &GpuStyle::default(), &[]);
    let textured = rig.draw(&ed, &cam, &style_for(&ed, true, |_| {}), &[]);
    // With the look the ground has joints (dark lines): more variation than the flat one.
    let spread = |img: &[u8]| {
        let v: Vec<f32> = (30..100).map(|x| lum(px(img, 128, x, 64))).collect();
        v.iter().cloned().fold(f32::MIN, f32::max) - v.iter().cloned().fold(f32::MAX, f32::min)
    };
    assert!(spread(&plain) < 3.0, "{}", spread(&plain));
    assert!(spread(&textured) > 20.0, "{}", spread(&textured));
    // The look changed, so the table and the texture array were uploaded again.
    assert_eq!(rig.gpu.uploads, 2);

    // The same frame on the CPU renderer: the average brightness of the ground is close.
    let scene = build(ed.doc());
    let look = Look::build(ed.doc(), &scene, true);
    let cpu = cadektor_render::shade::render_scene(
        &scene,
        &look,
        &cam,
        &cadektor_render::shade::ShadeOptions {
            width: 128,
            height: 128,
            background: [255, 255, 255, 255],
            ..Default::default()
        },
    );
    let avg_gpu: f32 = (40..90)
        .flat_map(|y| (40..90).map(move |x| (x, y)))
        .map(|(x, y)| lum(px(&textured, 128, x, y)))
        .sum::<f32>()
        / 2500.0;
    let avg_cpu: f32 = (40..90)
        .flat_map(|y| (40..90).map(move |x| (x, y)))
        .map(|(x, y)| lum(px(&cpu.rgba, 128, x, y)))
        .sum::<f32>()
        / 2500.0;
    assert!(
        (avg_gpu - avg_cpu).abs() < 12.0,
        "gpu {avg_gpu} cpu {avg_cpu}"
    );
}

#[test]
fn the_sun_casts_a_shadow_that_follows_the_light() {
    let Some((device, queue)) = device_or_skip("the_sun_casts_a_shadow") else {
        return;
    };
    let mut rig = Rig::new(device, queue, [200, 200]);
    let mut ed = Editor::default();
    add_rect(&mut ed, 4000.0, [200, 200, 200, 255]);
    add_slab(&mut ed, 0.0, 0.0, 1000.0, 1000.0, [180, 180, 180, 255]);
    let cam = top_cam(4000.0);
    let sun = Light::Sun([0.866, 0.0, 0.5]);
    let style = GpuStyle {
        light: sun,
        shadows: true,
        ..GpuStyle::default()
    };
    let img = rig.draw(&ed, &cam, &style, &[]);
    // 8000 mm over 200 px. The cube is at pixels 100 to 125; the shadow falls to the west.
    let shadow = lum(px(&img, 200, 80, 88));
    let light = lum(px(&img, 200, 80, 150));
    assert!(shadow < light * 0.8, "{shadow} vs {light}");
    assert!(
        (lum(px(&img, 200, 40, 88)) - light).abs() < 4.0,
        "far from the cube"
    );
    assert_eq!(rig.gpu.shadow_draws, 1);
    // Another frame with the same sun does not draw the shadow map again.
    let _ = rig.draw(&ed, &cam, &style, &[]);
    assert_eq!(rig.gpu.shadow_draws, 1);
    // A different sun draws it again, and the shadow moves.
    let style2 = GpuStyle {
        light: Light::Sun([-0.866, 0.0, 0.5]),
        ..style.clone()
    };
    let img2 = rig.draw(&ed, &cam, &style2, &[]);
    assert_eq!(rig.gpu.shadow_draws, 2);
    assert!(lum(px(&img2, 200, 80, 88)) > lum(px(&img2, 200, 140, 88)) + 10.0);
    // Shadows off: even light.
    let none = GpuStyle {
        shadows: false,
        ..style
    };
    let img3 = rig.draw(&ed, &cam, &none, &[]);
    assert!((lum(px(&img3, 200, 80, 88)) - lum(px(&img3, 200, 80, 150))).abs() < 4.0);
    // Night is darker than day.
    let night = GpuStyle {
        light: Light::Night,
        ..none
    };
    let img4 = rig.draw(&ed, &cam, &night, &[]);
    assert!(lum(px(&img4, 200, 20, 20)) < lum(px(&img3, 200, 20, 20)));
}

#[test]
fn see_through_surfaces_blend_and_hidden_line_modes_remove_covered_edges() {
    let Some((device, queue)) = device_or_skip("see_through_surfaces") else {
        return;
    };
    let mut rig = Rig::new(device, queue, [160, 160]);
    let mut ed = Editor::default();
    add_rect(&mut ed, 2000.0, [220, 40, 40, 255]);
    let water = add_slab(&mut ed, -1000.0, -1000.0, 1000.0, 20.0, [60, 120, 200, 255]);
    let m = add_material(
        &mut ed,
        "Water",
        Material {
            base_color: Color([60, 120, 200, 255]),
            opacity: 0.5,
            ..Default::default()
        },
    );
    assign(&mut ed, water, m);
    let cam = top_cam(2500.0);
    let blended = rig.draw(&ed, &cam, &style_for(&ed, true, |_| {}), &[]);
    let (dry, wet) = (px(&blended, 160, 110, 80), px(&blended, 160, 60, 100));
    assert!(dry[0] > 150 && dry[2] < 100, "{dry:?}");
    assert!(
        wet[0] > 40 && wet[0] < dry[0] && wet[2] > dry[2] + 20,
        "{wet:?} {dry:?}"
    );
    // The same without edges.
    let no_edges = rig.draw(&ed, &cam, &style_for(&ed, true, |s| s.edges = false), &[]);
    let wet2 = px(&no_edges, 160, 60, 100);
    assert!(wet2[2] > dry[2] + 20, "{wet2:?}");

    // Hidden line: a cube in an isometric view.
    let mut ed = Editor::default();
    add_slab(&mut ed, 0.0, 0.0, 1000.0, 1000.0, [180, 180, 180, 255]);
    let scene = build(ed.doc());
    let mut cam = Camera::standard(StandardView::RightIsometric, Projection::Orthographic);
    cam.fit(scene.bounds, 1.0);
    // Count the pixels that are not the white background.
    let inked = |img: &[u8]| (0..160 * 160).filter(|i| img[i * 4] < 235).count();
    let wire = rig.draw(
        &ed,
        &cam,
        &GpuStyle {
            mode: RenderMode::Wireframe,
            ..GpuStyle::default()
        },
        &[],
    );
    let hidden = rig.draw(
        &ed,
        &cam,
        &GpuStyle {
            mode: RenderMode::HiddenLine,
            ..GpuStyle::default()
        },
        &[],
    );
    let dashed = rig.draw(
        &ed,
        &cam,
        &GpuStyle {
            mode: RenderMode::DashedHiddenLine,
            ..GpuStyle::default()
        },
        &[],
    );
    // The faces are the background color, so the inside of the cube is white.
    assert_eq!(px(&hidden, 160, 80, 60), [255, 255, 255]);
    // Wireframe shows 12 edges and hidden line 9. The dashed mode adds dashes for the other 3.
    let (w, h, d) = (inked(&wire), inked(&hidden), inked(&dashed));
    assert!(w as f32 > h as f32 * 1.12, "wireframe {w} hidden {h}");
    assert!(d > h + 15 && d < w, "dashed {d} hidden {h} wireframe {w}");
}

#[test]
fn ten_thousand_plants_draw_in_reasonable_time() {
    let Some((device, queue)) = device_or_skip("ten_thousand_plants") else {
        return;
    };
    let mut rig = Rig::new(device, queue, [400, 300]);
    let mut ed = Editor::default();
    let positions: Vec<DVec2> = (0..10_000)
        .map(|i| DVec2::new((i % 100) as f64 * 2500.0, (i / 100) as f64 * 2500.0))
        .collect();
    ed.execute(&Command::AddPlants {
        style: None,
        overrides: serde_json::Map::new(),
        positions,
        layer: None,
        class: None,
        variation: None,
    })
    .unwrap();
    ed.execute(&Command::SetSun {
        sun: Some(SunSettings::default()),
    })
    .unwrap();
    let scene = build(ed.doc());
    let mut cam = Camera::standard(StandardView::RightIsometric, Projection::Perspective);
    cam.fit(scene.bounds, 400.0 / 300.0);
    let style = GpuStyle {
        light: Light::Sun([0.4, 0.3, 0.8]),
        shadows: true,
        ..GpuStyle::default()
    };
    // The first frame uploads the mesh and draws the shadow map; the next frames are plain draws.
    let first = std::time::Instant::now();
    let _ = rig.draw_scene(&scene, ed.revision(), &cam, &style, &[]);
    let first = first.elapsed();
    let frames = std::time::Instant::now();
    for _ in 0..5 {
        let _ = rig.draw_scene(&scene, ed.revision(), &cam, &style, &[]);
    }
    let per_frame = frames.elapsed() / 5;
    eprintln!(
        "10k plants: {} triangles, first frame {first:?}, next frames {per_frame:?}",
        scene.mesh.triangle_count()
    );
    // Each frame includes reading the picture back, so the limit is loose. It only catches a frame
    // that takes seconds.
    assert!(
        per_frame < std::time::Duration::from_secs(5),
        "{per_frame:?}"
    );
    assert_eq!(rig.gpu.uploads, 1);
}
