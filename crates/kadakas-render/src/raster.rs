//! A small CPU z-buffer rasterizer with flat Lambert shading and edge lines.
//!
//! The app draws the same scene on the GPU. This renderer lets an agent look at the model
//! (`render.png`) without a window or a GPU.

use glam::{Vec3, Vec4};
use kadakas_core::mesh::Scene;

use crate::camera::Camera;
use crate::views::{Projection, RenderMode};

/// Light direction (toward the light) and ambient part of the flat shading.
/// The GPU shader in the app uses the same numbers.
pub const LIGHT_DIR: Vec3 = Vec3::new(0.35, -0.45, 0.82);
pub const AMBIENT: f32 = 0.32;

/// Background of rendered images.
pub const BACKGROUND: [u8; 4] = [246, 247, 249, 255];

/// An RGBA image, 8 bits per channel, rows from the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32, fill: [u8; 4]) -> Image {
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..width as usize * height as usize {
            rgba.extend_from_slice(&fill);
        }
        Image {
            width,
            height,
            rgba,
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    fn put(&mut self, x: usize, y: usize, c: [u8; 4]) {
        let i = (y * self.width as usize + x) * 4;
        self.rgba[i..i + 4].copy_from_slice(&c);
    }

    /// PNG file contents.
    pub fn to_png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("png header");
        w.write_image_data(&self.rgba).expect("png data");
        w.finish().expect("png end");
        out
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RenderOptions {
    pub width: u32,
    pub height: u32,
    pub mode: RenderMode,
    pub background: [u8; 4],
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            width: 800,
            height: 600,
            mode: RenderMode::Shaded,
            background: BACKGROUND,
        }
    }
}

/// Depth that an edge line may be behind a surface and still show.
const LINE_DEPTH_BIAS: f32 = 3e-4;
/// Two surfaces that lie in one plane (a flat face under a slab, two slabs at one level) differ in
/// depth only by rounding. Within this much (of the depth range) the triangle drawn later wins, so
/// the picture is steady instead of speckled (zoomed out it looked like moire).
pub(crate) const DEPTH_TIE: f32 = 5e-6;

/// Flat Lambert factor for a unit normal that faces the viewer.
pub fn lambert(normal: Vec3) -> f32 {
    AMBIENT + (1.0 - AMBIENT) * normal.dot(LIGHT_DIR.normalize()).max(0.0)
}

/// Clips a polygon against the near plane (clip z >= 0). Returns the number of vertices.
fn clip_near(input: &[Vec4], out: &mut [Vec4; 4]) -> usize {
    let mut n = 0;
    for i in 0..input.len() {
        let (a, b) = (input[i], input[(i + 1) % input.len()]);
        if a.z >= 0.0 {
            out[n] = a;
            n += 1;
        }
        if (a.z >= 0.0) != (b.z >= 0.0) {
            out[n] = a + (b - a) * (a.z / (a.z - b.z));
            n += 1;
        }
    }
    n
}

struct Target {
    img: Image,
    depth: Vec<f32>,
}

impl Target {
    fn to_screen(&self, c: Vec4) -> Vec3 {
        let (w, h) = (self.img.width as f32, self.img.height as f32);
        Vec3::new(
            (c.x / c.w * 0.5 + 0.5) * w,
            (0.5 - c.y / c.w * 0.5) * h,
            c.z / c.w,
        )
    }

    fn triangle(&mut self, p: [Vec3; 3], color: [u8; 4]) {
        let area = (p[1].x - p[0].x) * (p[2].y - p[0].y) - (p[1].y - p[0].y) * (p[2].x - p[0].x);
        if area == 0.0 || !area.is_finite() {
            return;
        }
        let (w, h) = (self.img.width as i64, self.img.height as i64);
        let min_x = (p.iter().map(|v| v.x).fold(f32::INFINITY, f32::min).floor() as i64).max(0);
        let max_x = (p
            .iter()
            .map(|v| v.x)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil() as i64)
            .min(w - 1);
        let min_y = (p.iter().map(|v| v.y).fold(f32::INFINITY, f32::min).floor() as i64).max(0);
        let max_y = (p
            .iter()
            .map(|v| v.y)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil() as i64)
            .min(h - 1);
        let inv = 1.0 / area;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 =
                    ((p[2].x - p[1].x) * (cy - p[1].y) - (p[2].y - p[1].y) * (cx - p[1].x)) * inv;
                let w1 =
                    ((p[0].x - p[2].x) * (cy - p[2].y) - (p[0].y - p[2].y) * (cx - p[2].x)) * inv;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * p[0].z + w1 * p[1].z + w2 * p[2].z;
                let i = y as usize * w as usize + x as usize;
                if (0.0..=1.0).contains(&z) && z <= self.depth[i] + DEPTH_TIE {
                    self.depth[i] = z;
                    self.img.put(x as usize, y as usize, color);
                }
            }
        }
    }

    fn line(&mut self, a: Vec3, b: Vec3, color: [u8; 4]) {
        let (w, h) = (self.img.width as i64, self.img.height as i64);
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
            let (xi, yi) = (x.floor() as i64, y.floor() as i64);
            if xi < 0 || yi < 0 || xi >= w || yi >= h || !(0.0..=1.0).contains(&z) {
                continue;
            }
            if z <= self.depth[yi as usize * w as usize + xi as usize] + LINE_DEPTH_BIAS {
                self.img.put(xi as usize, yi as usize, color);
            }
        }
    }
}

/// Draws the scene with the camera.
pub fn render(scene: &Scene, camera: &Camera, opts: &RenderOptions) -> Image {
    let (w, h) = (opts.width.max(1), opts.height.max(1));
    let mut target = Target {
        img: Image::new(w, h, opts.background),
        depth: vec![f32::INFINITY; w as usize * h as usize],
    };
    if scene.is_empty() {
        return target.img;
    }
    let aspect = w as f32 / h as f32;
    let mvp = camera.view_proj(aspect, scene.bounds);
    let clip = |p: Vec3| mvp * p.extend(1.0);

    if opts.mode == RenderMode::Shaded {
        let mesh = &scene.mesh;
        let clips: Vec<Vec4> = mesh.positions.iter().map(|p| clip(*p)).collect();
        let (eye, fwd) = (camera.eye(), camera.forward());
        for t in 0..mesh.triangle_count() {
            let i = [
                mesh.indices[t * 3] as usize,
                mesh.indices[t * 3 + 1] as usize,
                mesh.indices[t * 3 + 2] as usize,
            ];
            let c = [clips[i[0]], clips[i[1]], clips[i[2]]];
            // Whole triangle outside one side of the view: skip it early.
            if c.iter().all(|v| v.x < -v.w)
                || c.iter().all(|v| v.x > v.w)
                || c.iter().all(|v| v.y < -v.w)
                || c.iter().all(|v| v.y > v.w)
                || c.iter().all(|v| v.z > v.w)
                || c.iter().all(|v| v.z < 0.0)
            {
                continue;
            }
            let mut n =
                (mesh.normals[i[0]] + mesh.normals[i[1]] + mesh.normals[i[2]]).normalize_or_zero();
            let to_eye = match camera.projection {
                Projection::Orthographic => -fwd,
                Projection::Perspective => (eye - mesh.positions[i[0]]).normalize_or_zero(),
            };
            let mut base = mesh.colors[i[0]];
            if n.dot(to_eye) < 0.0 {
                n = -n;
                if mesh.is_two_sided(i[0]) {
                    base = kadakas_core::mesh::SURFACE_BACK;
                }
            }
            let f = lambert(n);
            let color = [
                (base[0] as f32 * f) as u8,
                (base[1] as f32 * f) as u8,
                (base[2] as f32 * f) as u8,
                255,
            ];
            let mut poly = [Vec4::ZERO; 4];
            let count = clip_near(&c, &mut poly);
            if count < 3 {
                continue;
            }
            let s: Vec<Vec3> = poly[..count].iter().map(|v| target.to_screen(*v)).collect();
            for k in 1..count - 1 {
                target.triangle([s[0], s[k], s[k + 1]], color);
            }
        }
    }

    for s in 0..scene.lines.segment_count() {
        let [a, b] = scene.lines.segment(s);
        let (mut ca, mut cb) = (clip(a), clip(b));
        // Near plane: keep the part with z >= 0.
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
        target.line(pa, pb, scene.lines.colors[s]);
    }
    target.img
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;
    use crate::views::StandardView;
    use glam::DVec2;
    use kadakas_core::command::Editor;
    use kadakas_core::model::Shape;

    #[test]
    fn ortho_top_view_covers_the_expected_pixel_box() {
        let mut ed = Editor::default();
        add(&mut ed, square(500.0), Some([200, 30, 30, 255]));
        let scene = scene_of(&ed);
        // 100 px for 2000 mm: 20 mm per pixel. The square spans pixels 25 to 74.
        let img = render(
            &scene,
            &top_ortho(1000.0),
            &RenderOptions {
                width: 100,
                height: 100,
                ..Default::default()
            },
        );
        assert_eq!((img.width, img.height), (100, 100));
        for (x, y, inside) in [
            (24, 50, false),
            (25, 50, true),
            (74, 50, true),
            (76, 50, false),
            (50, 24, false),
            (50, 25, true),
            (50, 74, true),
            (50, 76, false),
            (0, 0, false),
            (99, 99, false),
        ] {
            let px = img.pixel(x, y);
            if inside {
                assert_ne!(px, BACKGROUND, "({x},{y}) should be the square");
            } else {
                assert_eq!(px, BACKGROUND, "({x},{y}) should be background");
            }
        }
        // The face is lit from above: a top face is brighter than the ambient part.
        let c = img.pixel(50, 50);
        assert!(c[0] > c[1] && c[0] > c[2], "{c:?}");
        let expect = lambert(Vec3::Z);
        assert!((c[0] as f32 - 200.0 * expect).abs() <= 1.0);
        // Y is up on screen: a mark at +Y is in the upper half.
        let mut ed = Editor::default();
        add(
            &mut ed,
            Shape::Rect {
                min: DVec2::new(-100.0, 400.0),
                max: DVec2::new(100.0, 800.0),
            },
            Some([0, 0, 200, 255]),
        );
        let img = render(
            &scene_of(&ed),
            &top_ortho(1000.0),
            &RenderOptions {
                width: 100,
                height: 100,
                ..Default::default()
            },
        );
        assert_ne!(img.pixel(50, 20), BACKGROUND);
        assert_eq!(img.pixel(50, 65), BACKGROUND);
    }

    #[test]
    fn nearer_surfaces_hide_farther_ones_whatever_the_draw_order() {
        let low = |ed: &mut Editor| {
            add(
                ed,
                Shape::Hardscape {
                    boundary: kadakas_core::curve::corners(&[
                        DVec2::splat(-500.0),
                        DVec2::new(500.0, -500.0),
                        DVec2::splat(500.0),
                        DVec2::new(-500.0, 500.0),
                    ]),
                    material: "a".into(),
                    thickness: Some(100.0),
                    elevation: Some(0.0),
                },
                Some([255, 0, 0, 255]),
            );
        };
        let high = |ed: &mut Editor| {
            add(
                ed,
                Shape::Hardscape {
                    boundary: kadakas_core::curve::corners(&[
                        DVec2::splat(-300.0),
                        DVec2::new(300.0, -300.0),
                        DVec2::splat(300.0),
                        DVec2::new(-300.0, 300.0),
                    ]),
                    material: "b".into(),
                    thickness: Some(100.0),
                    elevation: Some(500.0),
                },
                Some([0, 255, 0, 255]),
            );
        };
        let opts = RenderOptions {
            width: 100,
            height: 100,
            ..Default::default()
        };
        for order in [0, 1] {
            let mut ed = Editor::default();
            if order == 0 {
                low(&mut ed);
                high(&mut ed);
            } else {
                high(&mut ed);
                low(&mut ed);
            }
            let img = render(&scene_of(&ed), &top_ortho(1000.0), &opts);
            let center = img.pixel(50, 50);
            assert!(
                center[1] > center[0],
                "order {order}: the high green slab is in front: {center:?}"
            );
            let ring = img.pixel(33, 50); // inside the red slab, outside the green one (x about -340)
            assert!(
                ring[0] > ring[1],
                "order {order}: red visible at the edge: {ring:?}"
            );
        }
        // Seen from below, the low slab is nearest instead.
        let mut ed = Editor::default();
        high(&mut ed);
        low(&mut ed);
        let mut cam = top_ortho(1000.0);
        cam.set_view(StandardView::Bottom);
        let img = render(&scene_of(&ed), &cam, &opts);
        let center = img.pixel(50, 50);
        assert!(center[0] > center[1], "{center:?}");
    }

    #[test]
    fn faces_in_one_plane_give_a_steady_picture_not_speckles() {
        // Two big faces at one level, seen zoomed out: every pixel of the overlap is one color.
        let mut ed = Editor::default();
        let big = |h: f64| Shape::Rect {
            min: DVec2::new(-h, -h * 0.6),
            max: DVec2::new(h, h * 0.6),
        };
        add(&mut ed, big(25_000.0), Some([200, 30, 30, 255]));
        add(&mut ed, big(24_000.0), Some([30, 30, 200, 255]));
        let img = render(
            &scene_of(&ed),
            &top_ortho(16_000.0),
            &RenderOptions {
                width: 400,
                height: 300,
                ..Default::default()
            },
        );
        let mut seen = std::collections::BTreeSet::new();
        for y in 100..200 {
            for x in 100..300 {
                seen.insert(img.pixel(x, y));
            }
        }
        assert_eq!(seen.len(), 1, "{seen:?}");
    }

    #[test]
    fn perspective_makes_near_things_bigger() {
        let mut ed = Editor::default();
        add(
            &mut ed,
            Shape::Hardscape {
                boundary: kadakas_core::curve::corners(&[
                    DVec2::splat(-500.0),
                    DVec2::new(500.0, -500.0),
                    DVec2::splat(500.0),
                    DVec2::new(-500.0, 500.0),
                ]),
                material: "a".into(),
                thickness: Some(100.0),
                elevation: Some(2000.0),
            },
            Some([255, 0, 0, 255]),
        );
        let scene = scene_of(&ed);
        let count = |proj| {
            let mut cam = Camera::standard(StandardView::Top, proj);
            cam.distance = 4000.0;
            let img = render(
                &scene,
                &cam,
                &RenderOptions {
                    width: 120,
                    height: 120,
                    ..Default::default()
                },
            );
            img.rgba
                .chunks(4)
                .filter(|p| p[0] > 150 && p[1] < 60)
                .count()
        };
        // The slab top is 2000 mm closer than the target plane: it looks twice as big in perspective.
        let (o, p) = (
            count(Projection::Orthographic),
            count(Projection::Perspective),
        );
        assert!(p as f32 > o as f32 * 2.5, "ortho {o}, perspective {p}");
    }

    #[test]
    fn wireframe_draws_only_lines_and_the_camera_can_be_inside_the_scene() {
        let mut ed = Editor::default();
        add(&mut ed, square(500.0), Some([200, 30, 30, 255]));
        let scene = scene_of(&ed);
        let opts = RenderOptions {
            width: 100,
            height: 100,
            mode: RenderMode::Wireframe,
            ..Default::default()
        };
        let img = render(&scene, &top_ortho(1000.0), &opts);
        assert_eq!(img.pixel(50, 50), BACKGROUND, "no fill");
        assert_ne!(img.pixel(25, 50), BACKGROUND, "outline is drawn");
        // A perspective camera very close to the scene still renders.
        let mut cam = Camera::standard(StandardView::Front, Projection::Perspective);
        cam.distance = 5.0;
        let img = render(
            &scene,
            &cam,
            &RenderOptions {
                width: 50,
                height: 50,
                ..Default::default()
            },
        );
        assert_eq!(img.rgba.len(), 50 * 50 * 4);
        // A camera inside a big slab: triangles cross the near plane.
        let mut ed = Editor::default();
        add(&mut ed, square(50_000.0), None);
        let mut cam = Camera::standard(StandardView::RightIsometric, Projection::Perspective);
        cam.distance = 100.0;
        cam.target = Vec3::new(0.0, 0.0, 10.0);
        let img = render(
            &scene_of(&ed),
            &cam,
            &RenderOptions {
                width: 50,
                height: 50,
                ..Default::default()
            },
        );
        assert!(
            img.rgba.chunks(4).any(|p| p != BACKGROUND),
            "the floor is visible"
        );
    }

    #[test]
    fn empty_scene_is_background_and_png_is_valid() {
        let img = render(
            &Scene::default(),
            &Camera::default(),
            &RenderOptions {
                width: 32,
                height: 16,
                ..Default::default()
            },
        );
        assert!(img.rgba.chunks(4).all(|p| p == BACKGROUND));
        let png = img.to_png();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let dec = png::Decoder::new(std::io::Cursor::new(&png));
        let mut reader = dec.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (32, 16));
        assert_eq!(&buf[..info.buffer_size()], &img.rgba[..]);
        // Zero sizes are made one pixel.
        let img = render(
            &Scene::default(),
            &Camera::default(),
            &RenderOptions {
                width: 0,
                height: 0,
                ..Default::default()
            },
        );
        assert_eq!((img.width, img.height), (1, 1));
    }

    #[test]
    fn plants_render_in_isometric_view() {
        let mut ed = Editor::default();
        add(
            &mut ed,
            Shape::Plant {
                at: DVec2::ZERO,
                species: "Oak".into(),
                spread: 4000.0,
                height: 8000.0,
            },
            None,
        );
        let scene = scene_of(&ed);
        let mut cam = Camera::standard(StandardView::RightIsometric, Projection::Orthographic);
        cam.fit(scene.bounds, 1.0);
        let img = render(
            &scene,
            &cam,
            &RenderOptions {
                width: 120,
                height: 120,
                ..Default::default()
            },
        );
        let green = img
            .rgba
            .chunks(4)
            .filter(|p| p[1] > p[0] && p[1] > p[2] && *p != &BACKGROUND[..])
            .count();
        assert!(green > 500, "canopy pixels: {green}");
    }

    #[test]
    fn the_back_of_a_surface_model_face_has_the_back_color_like_on_the_gpu() {
        let mut ed = Editor::default();
        add_surface_square(&mut ed, 500.0);
        let scene = scene_of(&ed);
        let opts = RenderOptions {
            width: 100,
            height: 100,
            ..Default::default()
        };
        let front = render(&scene, &top_ortho(1000.0), &opts).pixel(50, 50);
        let back = render(&scene, &bottom_ortho(1000.0), &opts).pixel(50, 50);
        assert!(
            front[0].abs_diff(front[2]) < 30,
            "front is neutral: {front:?}"
        );
        assert!(back[2] > back[0] + 15, "back is bluish: {back:?}");
    }
}
