use super::*;
use crate::testutil::*;
use crate::views::StandardView;
use glam::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Color, ObjectId, Shape};
use kadakas_core::present::{
    MappingKind, Material, Procedural, RenderResource, SlotTarget, Texture, TextureMapping,
};
use kadakas_core::resource::{ResourceBody, ResourceId};

fn add_material(ed: &mut Editor, m: Material) -> ResourceId {
    ResourceId(
        ed.execute(&Command::AddResource {
            name: format!("m{}", ed.doc().resources.len()),
            body: ResourceBody::Render(Box::new(RenderResource::Material(m))),
        })
        .unwrap()
        .created[0],
    )
}

fn assign(ed: &mut Editor, object: ObjectId, m: ResourceId) {
    ed.execute(&Command::SetMaterialSlot {
        target: SlotTarget::Object { object, slot: None },
        material: Some(m),
    })
    .unwrap();
}

fn lum(p: [u8; 4]) -> f32 {
    0.3 * p[0] as f32 + 0.59 * p[1] as f32 + 0.11 * p[2] as f32
}

/// Mean and variance of the luminance in a pixel box.
fn stats(img: &Image, x0: u32, y0: u32, x1: u32, y1: u32) -> (f32, f32) {
    let mut v = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            v.push(lum(img.pixel(x, y)));
        }
    }
    let mean = v.iter().sum::<f32>() / v.len() as f32;
    let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / v.len() as f32;
    (mean, var)
}

fn opts(w: u32, h: u32) -> ShadeOptions {
    ShadeOptions {
        width: w,
        height: h,
        ..Default::default()
    }
}

fn render_doc(ed: &Editor, cam: &Camera, o: &ShadeOptions) -> Image {
    let scene = scene_of(ed);
    let look = Look::build(ed.doc(), &scene, true);
    render_scene(&scene, &look, cam, o)
}

#[test]
fn a_textured_surface_has_texture_detail_and_the_flat_one_has_none() {
    let mut ed = Editor::default();
    let ground = add(&mut ed, square(2000.0), Some([200, 200, 200, 255]));
    let m = add_material(
        &mut ed,
        Material {
            base_color: Color([100, 140, 60, 255]),
            texture: Some(Texture::Procedural {
                kind: Procedural::Grass,
                seed: 1,
            }),
            mapping: TextureMapping {
                kind: MappingKind::Planar,
                scale: [500.0, 500.0],
                ..Default::default()
            },
            ..Default::default()
        },
    );
    assign(&mut ed, ground, m);
    let cam = top_ortho(2500.0);
    let img = render_doc(&ed, &cam, &opts(200, 200));
    let (mean, var) = stats(&img, 60, 60, 140, 140);
    assert!(var > 40.0, "texture detail: variance {var}");
    // Green wins over red and blue on average.
    let c = img.pixel(100, 100);
    assert!(c[1] > c[2], "{c:?}");
    // Same scene without the material: a flat color.
    let mut plain = Editor::default();
    add(&mut plain, square(2000.0), Some([200, 200, 200, 255]));
    let flat = render_doc(&plain, &cam, &opts(200, 200));
    let (fm, fv) = stats(&flat, 60, 60, 140, 140);
    assert!(fv < 1e-3, "{fv}");
    assert!(mean > 0.0 && fm > 0.0);
    // The background outside the surface is the same.
    assert_eq!(img.pixel(2, 2), flat.pixel(2, 2));
    // Determinism.
    assert_eq!(img, render_doc(&ed, &cam, &opts(200, 200)));
}

#[test]
fn texture_scale_in_world_units_sets_the_repeat_size() {
    // Pavers: 4 pavers per texture repeat. With a scale of 1600 mm, a joint every 400 mm.
    let mut ed = Editor::default();
    let ground = add(&mut ed, square(2000.0), Some([200, 200, 200, 255]));
    let m = add_material(
        &mut ed,
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
    // 4000 mm across 400 px: 10 mm per pixel, so a paver is 40 px wide.
    let cam = top_ortho(2000.0);
    let img = render_doc(&ed, &cam, &opts(400, 400));
    let row = 200;
    let dark: Vec<u32> = (0..400)
        .filter(|x| lum(img.pixel(*x, row)) < 110.0 && lum(img.pixel(*x, row)) > 0.0)
        .collect();
    // Joints repeat every 40 px: collect their groups.
    let mut centers = Vec::new();
    let mut i = 0;
    while i < dark.len() {
        let mut j = i;
        while j + 1 < dark.len() && dark[j + 1] == dark[j] + 1 {
            j += 1;
        }
        centers.push((dark[i] + dark[j]) as f32 / 2.0);
        i = j + 1;
    }
    assert!(centers.len() >= 5, "{centers:?}");
    for pair in centers.windows(2) {
        assert!((pair[1] - pair[0] - 40.0).abs() <= 2.0, "{centers:?}");
    }
}

fn sun_scene() -> Editor {
    let mut ed = Editor::default();
    add(&mut ed, square(4000.0), Some([200, 200, 200, 255]));
    let pts = [
        DVec2::new(0.0, 0.0),
        DVec2::new(1000.0, 0.0),
        DVec2::new(1000.0, 1000.0),
        DVec2::new(0.0, 1000.0),
    ];
    add(
        &mut ed,
        Shape::Hardscape {
            boundary: kadakas_core::curve::corners(&pts),
            material: "m".into(),
            thickness: Some(1000.0),
            elevation: Some(1000.0),
        },
        Some([180, 180, 180, 255]),
    );
    ed
}

#[test]
fn the_sun_casts_a_shadow_to_the_far_side() {
    let ed = sun_scene();
    let cam = top_ortho(4000.0);
    let sun = Light::Sun([0.866, 0.0, 0.5]);
    let shaded = ShadeOptions {
        light: sun,
        shadows: true,
        ..opts(200, 200)
    };
    let img = render_doc(&ed, &cam, &shaded);
    // 8000 mm in 200 px: 40 mm per pixel. The cube is at pixels x 100 to 125, rows 75 to 100.
    // The sun is in +X, so the shadow lies at x from -1730 to 0: pixels 57 to 100.
    let in_shadow = lum(img.pixel(80, 88));
    let in_light = lum(img.pixel(80, 150));
    assert!(in_shadow < in_light * 0.75, "{in_shadow} vs {in_light}");
    // Beyond the shadow length the ground is lit again.
    assert!((lum(img.pixel(40, 88)) - in_light).abs() < 2.0);
    // Without shadows the ground is evenly lit.
    let no_shadow = render_doc(
        &ed,
        &cam,
        &ShadeOptions {
            shadows: false,
            ..shaded
        },
    );
    assert!((lum(no_shadow.pixel(80, 88)) - lum(no_shadow.pixel(80, 150))).abs() < 2.0);
    // A lower sun makes a longer shadow.
    let low = render_doc(
        &ed,
        &cam,
        &ShadeOptions {
            light: Light::Sun([0.966, 0.0, 0.26]),
            ..shaded
        },
    );
    assert!(lum(low.pixel(50, 88)) < lum(low.pixel(50, 150)) * 0.8);
    assert_eq!(img, render_doc(&ed, &cam, &shaded));
}

#[test]
fn light_modes_differ_in_brightness() {
    let ed = sun_scene();
    let cam = top_ortho(4000.0);
    let day = render_doc(
        &ed,
        &cam,
        &ShadeOptions {
            light: Light::Sun([0.0, 0.0, 1.0]),
            ..opts(100, 100)
        },
    );
    let night = render_doc(
        &ed,
        &cam,
        &ShadeOptions {
            light: Light::Night,
            ..opts(100, 100)
        },
    );
    let fixed = render_doc(&ed, &cam, &opts(100, 100));
    let (d, n, f) = (
        lum(day.pixel(20, 20)),
        lum(night.pixel(20, 20)),
        lum(fixed.pixel(20, 20)),
    );
    assert!(n < f && n < d, "night {n} fixed {f} day {d}");
    assert!(
        d > f * 0.9,
        "overhead sun is at least as bright as the fixed light"
    );
}

#[test]
fn transparent_surfaces_blend_with_what_is_behind() {
    let mut ed = Editor::default();
    add(&mut ed, square(2000.0), Some([220, 40, 40, 255]));
    // A pond over the left half of the ground: a thin slab a little above it.
    let water = add(
        &mut ed,
        Shape::Hardscape {
            boundary: kadakas_core::curve::corners(&[
                DVec2::new(-1000.0, -1000.0),
                DVec2::new(0.0, -1000.0),
                DVec2::new(0.0, 1000.0),
                DVec2::new(-1000.0, 1000.0),
            ]),
            material: "w".into(),
            thickness: Some(5.0),
            elevation: Some(10.0),
        },
        Some([60, 120, 200, 255]),
    );
    let m = add_material(
        &mut ed,
        Material {
            base_color: Color([60, 120, 200, 255]),
            opacity: 0.5,
            ..Default::default()
        },
    );
    assign(&mut ed, water, m);
    let cam = top_ortho(2500.0);
    let img = render_doc(&ed, &cam, &opts(200, 200));
    let dry = img.pixel(130, 100);
    let wet = img.pixel(70, 100);
    assert!(dry[0] > 150 && dry[2] < 100, "{dry:?}");
    // Between the red ground and the blue water: some red is left, and blue has grown.
    assert!(wet[0] > 40 && wet[0] < dry[0], "{wet:?} {dry:?}");
    assert!(wet[2] > dry[2] + 20, "{wet:?} {dry:?}");
}

#[test]
fn hidden_line_modes_draw_dark_lines_on_white() {
    let ed = sun_scene();
    let scene = scene_of(&ed);
    let look = Look::plain(&scene);
    let mut cam = Camera::standard(StandardView::RightIsometric, Projection::Orthographic);
    cam.fit(scene.bounds, 1.0);
    let hidden = render_scene(
        &scene,
        &look,
        &cam,
        &ShadeOptions {
            mode: RenderMode::HiddenLine,
            background: [255, 255, 255, 255],
            ..opts(300, 300)
        },
    );
    let dashed = render_scene(
        &scene,
        &look,
        &cam,
        &ShadeOptions {
            mode: RenderMode::DashedHiddenLine,
            background: [255, 255, 255, 255],
            ..opts(300, 300)
        },
    );
    let count =
        |img: &Image, f: &dyn Fn(&[u8]) -> bool| img.rgba.chunks_exact(4).filter(|p| f(p)).count();
    let white = count(&hidden, &|p| p[0] == 255 && p[1] == 255);
    let dark = count(&hidden, &|p| p[0] < 100);
    assert!(white > 300 * 300 * 8 / 10, "mostly white: {white}");
    assert!(dark > 100, "{dark}");
    // The dashed mode adds gray pixels for the covered edges.
    let gray = |p: &[u8]| (100..200).contains(&p[0]) && p[0] == p[1];
    assert!(count(&dashed, &gray) >= count(&hidden, &gray));
}

#[test]
fn wireframe_and_empty_scenes_and_supersampling() {
    let ed = sun_scene();
    let cam = top_ortho(4000.0);
    let wire = render_doc(
        &ed,
        &cam,
        &ShadeOptions {
            mode: RenderMode::Wireframe,
            ..opts(100, 100)
        },
    );
    // Surfaces are not filled in wireframe: the middle of the ground is background.
    assert_eq!(wire.pixel(10, 10), crate::raster::BACKGROUND);
    let empty = Editor::default();
    let img = render_doc(&empty, &cam, &opts(32, 24));
    assert!(
        img.rgba
            .chunks_exact(4)
            .all(|p| p == crate::raster::BACKGROUND)
    );
    // Supersampling gives intermediate values on the edge of the cube.
    let flat = render_doc(&ed, &cam, &opts(100, 100));
    let smooth = render_doc(
        &ed,
        &cam,
        &ShadeOptions {
            samples: 2,
            ..opts(100, 100)
        },
    );
    let levels = |img: &Image| {
        let mut s: Vec<u8> = img.rgba.chunks_exact(4).map(|p| p[0]).collect();
        s.sort_unstable();
        s.dedup();
        s.len()
    };
    assert!(levels(&smooth) >= levels(&flat));
    assert_eq!((smooth.width, smooth.height), (100, 100));
}

#[test]
fn edges_can_be_switched_off() {
    let ed = sun_scene();
    let cam = top_ortho(4000.0);
    let with = render_doc(&ed, &cam, &opts(100, 100));
    let without = render_doc(
        &ed,
        &cam,
        &ShadeOptions {
            edges: false,
            ..opts(100, 100)
        },
    );
    assert_ne!(with, without);
    let dark = |img: &Image| img.rgba.chunks_exact(4).filter(|p| p[0] < 60).count();
    assert!(dark(&with) > dark(&without));
}

#[test]
fn shiny_metal_has_a_highlight_and_rough_stone_does_not() {
    let mut ed = Editor::default();
    let plate = add(&mut ed, square(2000.0), Some([150, 150, 150, 255]));
    let metal = add_material(
        &mut ed,
        Material {
            base_color: Color([150, 150, 150, 255]),
            roughness: 0.05,
            metallic: 1.0,
            ..Default::default()
        },
    );
    let rough = add_material(
        &mut ed,
        Material {
            base_color: Color([150, 150, 150, 255]),
            roughness: 0.95,
            metallic: 0.0,
            ..Default::default()
        },
    );
    // The camera looks straight down and the sun is straight above: the mirror reflection
    // goes straight to the eye.
    let cam = top_ortho(2500.0);
    let o = ShadeOptions {
        light: Light::Sun([0.0, 0.0, 1.0]),
        ..opts(100, 100)
    };
    assign(&mut ed, plate, metal);
    let m = render_doc(&ed, &cam, &o);
    assign(&mut ed, plate, rough);
    let r = render_doc(&ed, &cam, &o);
    let (mm, rr) = (lum(m.pixel(50, 50)), lum(r.pixel(50, 50)));
    assert!(mm > rr + 15.0, "metal {mm} rough {rr}");
}

#[test]
fn shadow_map_lookups() {
    let ed = sun_scene();
    let scene = scene_of(&ed);
    let map = ShadowMap::build(&scene, Vec3::new(0.866, 0.0, 0.5), 256, &|_| true).unwrap();
    // A point on the ground west of the cube is in shadow, one east of it is lit.
    assert!(map.lit(Vec3::new(-500.0, 500.0, 0.0), 0.5) < 0.2);
    assert!(map.lit(Vec3::new(2500.0, 500.0, 0.0), 0.5) > 0.8);
    // Outside the map counts as lit.
    assert_eq!(map.lit(Vec3::new(1e6, 1e6, 0.0), 0.5), 1.0);
    // The top of the cube is lit.
    assert!(map.lit(Vec3::new(500.0, 500.0, 1000.0), 0.5) > 0.8);
    // No scene and no direction give no map.
    assert!(ShadowMap::build(&Scene::default(), Vec3::Z, 64, &|_| true).is_none());
    assert!(ShadowMap::build(&scene, Vec3::ZERO, 64, &|_| true).is_none());
    // A sun straight above uses another helper axis and still works.
    assert!(ShadowMap::build(&scene, Vec3::Z, 64, &|_| true).is_some());
}

#[test]
fn sun_matrix_maps_the_scene_into_the_unit_box() {
    let ed = sun_scene();
    let scene = scene_of(&ed);
    let (lo, hi) = scene.bounds.unwrap();
    for to_sun in [
        Vec3::new(0.866, 0.0, 0.5),
        Vec3::Z,
        Vec3::new(-0.3, 0.4, 0.8),
    ] {
        let view = sun_view(&scene, to_sun).unwrap();
        let m = view.matrix;
        assert!(view.extent.min_element() >= 1.0);
        for i in 0..8 {
            let c = Vec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            let p = m * c.extend(1.0);
            assert!(p.x.abs() <= 1.0001 && p.y.abs() <= 1.0001, "{p:?}");
            assert!((-0.0001..=1.0001).contains(&p.z), "{p:?}");
        }
        // A higher point is nearer to the sun: its depth is smaller.
        let near = m * Vec3::new(0.0, 0.0, 1000.0).extend(1.0);
        let far = m * Vec3::new(0.0, 0.0, 0.0).extend(1.0);
        assert!(near.z < far.z);
    }
    assert!(sun_view(&Scene::default(), Vec3::Z).is_none());
    assert!(sun_view(&scene, Vec3::ZERO).is_none());
}

#[test]
fn perspective_cameras_inside_the_scene_clip_at_the_near_plane() {
    let mut ed = Editor::default();
    // A big ground, a see-through slab beside the camera and a plant.
    add(&mut ed, square(50_000.0), Some([60, 160, 60, 255]));
    let pond = add(
        &mut ed,
        Shape::Hardscape {
            boundary: kadakas_core::curve::corners(&[
                DVec2::new(-800.0, 200.0),
                DVec2::new(800.0, 200.0),
                DVec2::new(800.0, 900.0),
                DVec2::new(-800.0, 900.0),
            ]),
            material: "w".into(),
            thickness: Some(100.0),
            elevation: Some(50.0),
        },
        Some([60, 120, 200, 255]),
    );
    let water = add_material(
        &mut ed,
        Material {
            opacity: 0.5,
            base_color: Color([60, 120, 200, 255]),
            ..Default::default()
        },
    );
    assign(&mut ed, pond, water);
    add(
        &mut ed,
        Shape::Plant {
            at: DVec2::new(1500.0, 800.0),
            species: "Oak".into(),
            spread: 1200.0,
            height: 2500.0,
        },
        None,
    );
    let scene = scene_of(&ed);
    // The eye is 300 mm from the target, in the middle of the 100 m wide ground: the ground
    // triangles reach behind the eye, and so do its outline segments.
    let mut cam = Camera::standard(StandardView::Front, Projection::Perspective);
    cam.target = Vec3::new(0.0, 0.0, 400.0);
    cam.distance = 300.0;
    let o = ShadeOptions {
        light: Light::Sun([0.5, -0.3, 0.8]),
        shadows: true,
        ..opts(120, 90)
    };
    let look = Look::build(ed.doc(), &scene, true);
    let img = render_scene(&scene, &look, &cam, &o);
    assert_eq!(img, render_scene(&scene, &look, &cam, &o));
    // Below the horizon is the green ground (and the pond in front); above it is the background.
    let ground = img.pixel(2, 55);
    assert!(
        ground[1] > ground[0] + 40 && ground[1] > ground[2] + 40,
        "{ground:?}"
    );
    assert_eq!(img.pixel(60, 3), crate::raster::BACKGROUND);
}

/// Two rectangles at the same height: the later one must cover the earlier one everywhere.
pub(crate) fn coplanar_scene() -> Editor {
    let mut ed = Editor::default();
    add(&mut ed, square(3000.0), Some([220, 30, 30, 255]));
    add(&mut ed, square(1500.0), Some([30, 200, 30, 255]));
    ed
}

#[test]
fn coplanar_surfaces_of_different_objects_do_not_flicker() {
    let ed = coplanar_scene();
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut cam = Camera::standard(StandardView::RightIsometric, projection);
        let scene = scene_of(&ed);
        cam.fit(scene.bounds, 4.0 / 3.0);
        let img = render_doc(&ed, &cam, &opts(400, 300));
        let green = |p: [u8; 4]| p[1] as i32 > p[0] as i32 + 60 && p[1] > 120;
        let red = |p: [u8; 4]| p[0] as i32 > p[1] as i32 + 60 && p[0] > 120;
        let mut inside = 0;
        let mut bad = 0;
        for y in 0..300 {
            for x in 0..400 {
                let p = img.pixel(x, y);
                // A pixel with a green neighbour on both sides is inside the small rectangle.
                if x > 2 && x < 397 && green(img.pixel(x - 2, y)) && green(img.pixel(x + 2, y)) {
                    inside += 1;
                    if red(p) {
                        bad += 1;
                    }
                }
            }
        }
        assert!(inside > 2000, "{projection:?}: {inside}");
        assert_eq!(
            bad, 0,
            "{projection:?}: red pixels show through the green rectangle"
        );
    }
}

#[test]
fn object_ranks_follow_the_document_order() {
    let ed = coplanar_scene();
    let scene = scene_of(&ed);
    let ranks = crate::look::object_ranks(&scene);
    let ids: Vec<_> = scene.objects().collect();
    assert_eq!(ranks[&ids[0]], 0);
    assert_eq!(ranks[&ids[1]], 1);
    assert!(crate::look::DEPTH_RANK_STEP * crate::look::DEPTH_RANK_MAX as f32 <= 3e-4);
}
