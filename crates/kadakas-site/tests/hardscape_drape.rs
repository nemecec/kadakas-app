//! Hardscapes and landscape areas on a site model: the draped surface lies on the ground, and
//! the reports use the same ground.

use kadakas_core::command::{Command, Editor};
use kadakas_core::curve;
use kadakas_core::ground::GroundSource;
use kadakas_core::hardscape::{self, HardscapePatch};
use kadakas_core::landscape::{self, LandscapePatch};
use kadakas_core::mesh;
use kadakas_core::model::{Attributes, ObjectId, Shape, SiteSettings};
use kadakas_core::{DVec2, DVec3};
use kadakas_site::draw::scene_ext;
use kadakas_site::{SiteCache, SiteResult, SiteTerrain};
use proptest::prelude::*;

const N: usize = 7;
const STEP: f64 = 1000.0;

fn site(heights: &[f64]) -> Shape {
    let mut points = Vec::new();
    for i in 0..N {
        for j in 0..N {
            points.push(DVec3::new(
                i as f64 * STEP,
                j as f64 * STEP,
                heights[i * N + j],
            ));
        }
    }
    Shape::SiteModel {
        points,
        lines: vec![],
        settings: SiteSettings::default(),
    }
}

fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

fn rect(lo: DVec2, hi: DVec2) -> Vec<curve::Vertex> {
    curve::corners(&[lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)])
}

proptest! {
    #[test]
    fn draped_hardscape_vertices_equal_the_site_elevation(
        heights in prop::collection::vec(-500.0..500.0f64, N * N),
        lo in (500.0..2500.0f64, 500.0..2500.0f64),
        size in (500.0..3000.0f64, 500.0..3000.0f64),
        lift in 0.0..300.0f64,
    ) {
        let mut ed = Editor::default();
        add(&mut ed, site(&heights));
        let (lo, hi) = (DVec2::new(lo.0, lo.1), DVec2::new(lo.0 + size.0, lo.1 + size.1));
        let h = add(&mut ed, Shape::Hardscape { boundary: rect(lo, hi), material: "m".into(), thickness: Some(100.0), elevation: Some(lift) });
        ed.execute(&Command::SetHardscape { ids: vec![h], patch: HardscapePatch { drape: Some(true), ..Default::default() } }).unwrap();
        let cache = SiteCache::default();
        let terrain = SiteTerrain::new(&cache, ed.revision(), ed.doc());
        let obj = ed.doc().object(h).unwrap();
        let ground = |p: DVec2| terrain.elevation(obj, p);
        // The surface itself.
        let surf = hardscape::surface::surface_of(obj, hardscape::data(obj).as_ref(), Some(&ground));
        let z = |p: DVec2| surf.z(p);
        let ring = curve::positions(obj.shape.vertices().unwrap());
        let (points, indices) = hardscape::surface::top_surface(&ring, &z, Some(hardscape::surface::DRAPE_STEP));
        prop_assert!(!points.is_empty());
        let site_id = ed.doc().objects.values().find(|o| matches!(o.shape, Shape::SiteModel { .. })).unwrap().id;
        let result = SiteResult::compute(ed.doc(), site_id).unwrap();
        for p in &points {
            let want = result.elevation_at(p.truncate()).unwrap() + lift;
            prop_assert!((p.z - (want + hardscape::surface::LIFT)).abs() < 1e-9, "{} vs {}", p.z, want);
        }
        let plan = hardscape::surface::plan_mesh_area(&points, &indices);
        prop_assert!((plan - size.0 * size.1).abs() < 1e-3 * size.0 * size.1);
        prop_assert!(hardscape::surface::mesh_area(&points, &indices) >= plan);
        // The 3D scene: every top vertex lies on the ground plus the lift.
        let ext = scene_ext(&cache, ed.revision());
        let scene = mesh::build_with(ed.doc(), DVec3::ZERO, Some(&ext));
        let mut checked = 0;
        for t in 0..scene.mesh.triangle_count() {
            if scene.mesh.tri_object[t] != h {
                continue;
            }
            for k in 0..3 {
                let i = scene.mesh.indices[t * 3 + k] as usize;
                let (p, n) = (scene.mesh.positions[i], scene.mesh.normals[i]);
                if n.z > 0.3 {
                    let want = result.elevation_at(DVec2::new(p.x as f64, p.y as f64)).unwrap() + lift + hardscape::surface::LIFT;
                    prop_assert!((p.z as f64 - want).abs() < 0.05, "{} vs {}", p.z, want);
                    checked += 1;
                }
            }
        }
        prop_assert!(checked >= 4);
        // The report finds the same ground.
        let report = hardscape::report::report(ed.doc(), &terrain);
        prop_assert!(report.rows[0].quantities.draped);
        prop_assert!(report.rows[0].quantities.surface_area_m2 >= report.rows[0].quantities.plan_area_m2 - 1e-9);
    }
}

#[test]
fn draped_landscape_area_follows_the_ground_in_3d_and_in_the_report() {
    let mut ed = Editor::default();
    let heights: Vec<f64> = (0..N * N).map(|k| (k / N) as f64 * 100.0).collect();
    add(&mut ed, site(&heights));
    let a = add(
        &mut ed,
        Shape::LandscapeArea {
            boundary: rect(DVec2::new(1000.0, 1000.0), DVec2::new(4000.0, 3000.0)),
            name: "Lawn".into(),
        },
    );
    ed.execute(&Command::SetLandscapeArea {
        ids: vec![a],
        patch: LandscapePatch {
            drape: Some(true),
            ..Default::default()
        },
    })
    .unwrap();
    let cache = SiteCache::default();
    let ext = scene_ext(&cache, ed.revision());
    let scene = mesh::build_with(ed.doc(), DVec3::ZERO, Some(&ext));
    let (lo, hi) = scene.object_bounds(a).unwrap();
    // Heights 100 to 400 mm at x = 1000 to 4000, plus the lift of 5 mm.
    assert!(
        (lo.z - 105.0).abs() < 1e-3 && (hi.z - 405.0).abs() < 1e-3,
        "{lo:?} {hi:?}"
    );
    let terrain = SiteTerrain::new(&cache, ed.revision(), ed.doc());
    let r = landscape::report(ed.doc(), &terrain);
    let plan = 6.0;
    let want = plan * (1.0 + 0.1f64.powi(2)).sqrt();
    assert!(
        (r.rows[0].surface_area_m2.unwrap() - want).abs() < 1e-6,
        "{:?}",
        r.rows[0].surface_area_m2
    );
    // Hardscapes on another layer have no ground.
    let layer = ed
        .execute(&Command::AddLayer {
            name: "Other".into(),
            elevation: 0.0,
            scale: None,
            kind: None,
        })
        .unwrap()
        .created[0];
    let h = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Hardscape {
                boundary: rect(DVec2::new(1000.0, 1000.0), DVec2::new(2000.0, 2000.0)),
                material: "m".into(),
                thickness: None,
                elevation: None,
            },
            layer: Some(kadakas_core::model::LayerId(layer)),
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::SetHardscape {
        ids: vec![h],
        patch: HardscapePatch {
            drape: Some(true),
            ..Default::default()
        },
    })
    .unwrap();
    let terrain = SiteTerrain::new(&cache, ed.revision(), ed.doc());
    let report = hardscape::report::report(ed.doc(), &terrain);
    assert!(!report.rows[0].quantities.draped);
}

#[test]
fn a_hardscape_half_on_the_site_model_reports_sensible_quantities() {
    let mut ed = Editor::default();
    // A ramp of 10 percent over x = 0 to 6000. The model ends at x = 6000.
    let heights: Vec<f64> = (0..N * N).map(|k| (k / N) as f64 * 100.0).collect();
    add(&mut ed, site(&heights));
    // A slab of 2 m by 2 m from x = 5000 to 7000: half on the model, half beyond its edge.
    let h = add(
        &mut ed,
        Shape::Hardscape {
            boundary: rect(DVec2::new(5000.0, 1000.0), DVec2::new(7000.0, 3000.0)),
            material: "m".into(),
            thickness: Some(100.0),
            elevation: None,
        },
    );
    ed.execute(&Command::SetHardscape {
        ids: vec![h],
        patch: HardscapePatch {
            drape: Some(true),
            ..Default::default()
        },
    })
    .unwrap();
    let cache = SiteCache::default();
    let terrain = SiteTerrain::new(&cache, ed.revision(), ed.doc());
    let q = &hardscape::report::report(ed.doc(), &terrain).rows[0].quantities;
    assert!(q.draped && q.partially_draped);
    // 1 m by 2 m on the ramp, and 1 m by 2 m level at the height of the edge.
    let want = 2.0 * (1.0f64 + 0.01).sqrt() + 2.0;
    assert!(
        (q.surface_area_m2 - want).abs() < 1e-6,
        "{} vs {want}",
        q.surface_area_m2
    );
    assert!((q.volume_m3 - want * 0.1).abs() < 1e-6, "{}", q.volume_m3);
    // The terrain answers beyond the edge with the edge height.
    let o = ed.doc().object(h).unwrap();
    assert_eq!(
        terrain.elevation(o, DVec2::new(6500.0, 2000.0)),
        Some(600.0)
    );
    assert!(
        !terrain.covered(o, DVec2::new(6500.0, 2000.0))
            && terrain.covered(o, DVec2::new(5500.0, 2000.0))
    );
    // The 3D solid follows the ramp and then stays level at the edge height.
    let ext = scene_ext(&cache, ed.revision());
    let scene = mesh::build_with(ed.doc(), DVec3::ZERO, Some(&ext));
    let (lo, hi) = scene.object_bounds(h).unwrap();
    assert!(
        (hi.z - (600.0 + hardscape::surface::LIFT as f32)).abs() < 1e-2,
        "{hi:?}"
    );
    assert!(
        lo.z < 600.0 - 99.0,
        "the walls go down the thickness of 100 mm: {lo:?}"
    );
}
