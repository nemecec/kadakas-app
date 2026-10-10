//! Massing models stand on the site model of their layer.

use kadakas_core::command::{Command, Editor};
use kadakas_core::curve;
use kadakas_core::massing::{Massing, RoofStyle};
use kadakas_core::mesh;
use kadakas_core::model::{Attributes, ObjectId, Shape, SiteSettings};
use kadakas_core::{DVec2, DVec3};
use kadakas_site::draw::scene_ext;
use kadakas_site::{SiteCache, SiteTerrain};
use proptest::prelude::*;

/// Ground that rises `slope` per mm toward +X, from `z0`, on a 9 by 9 grid of 1 m.
fn site(z0: f64, slope: f64) -> Shape {
    let mut points = Vec::new();
    for i in 0..9 {
        for j in 0..9 {
            let (x, y) = (f64::from(i) * 1000.0, f64::from(j) * 1000.0);
            points.push(DVec3::new(x, y, z0 + x * slope));
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

fn block(x: f64, y: f64, w: f64, d: f64, height: f64) -> Massing {
    Massing::new(
        curve::corners(&[
            DVec2::new(x, y),
            DVec2::new(x + w, y),
            DVec2::new(x + w, y + d),
            DVec2::new(x, y + d),
        ]),
        height,
    )
}

#[test]
fn the_building_stands_on_the_lowest_ground_under_it() {
    let mut ed = Editor::default();
    add(&mut ed, site(200.0, 0.1));
    let id = add(
        &mut ed,
        Shape::Massing(Box::new(block(2000.0, 2000.0, 3000.0, 2000.0, 6000.0))),
    );
    let cache = SiteCache::default();
    let terrain = SiteTerrain::new(&cache, ed.revision(), ed.doc());
    let o = ed.doc().object(id).unwrap();
    let Shape::Massing(m) = &o.shape else {
        panic!()
    };
    // The ground under x from 2000 mm is at 200 + 200 = 400 mm.
    assert!((terrain.massing_base(o, m) - 400.0).abs() < 1e-6);
    // The 3D scene agrees: the lowest point of the building is the lowest ground.
    let ext = scene_ext(&cache, ed.revision());
    let scene = mesh::build_with(ed.doc(), mesh::build(ed.doc()).origin, Some(&ext));
    let (lo, hi) = scene.object_bounds(id).expect("the building has geometry");
    assert!(
        (scene.origin.z + f64::from(lo.z) - 400.0).abs() < 1e-3,
        "{lo:?}"
    );
    assert!((f64::from(hi.z - lo.z) - 6000.0).abs() < 1e-3);
}

#[test]
fn an_own_base_wins_and_no_site_means_the_layer() {
    let mut ed = Editor::default();
    let mut m = block(2000.0, 2000.0, 3000.0, 2000.0, 3000.0);
    m.base = Some(-250.0);
    let id = add(&mut ed, Shape::Massing(Box::new(m)));
    let cache = SiteCache::default();
    let terrain = SiteTerrain::new(&cache, ed.revision(), ed.doc());
    let o = ed.doc().object(id).unwrap();
    let Shape::Massing(m) = &o.shape else {
        panic!()
    };
    assert_eq!(terrain.massing_base(o, m), -250.0);
    // Without a base and without a site model: the layer.
    let free = Massing::new(m.footprint.clone(), 3000.0);
    assert_eq!(terrain.massing_base(o, &free), 0.0);
    // A building next to the model (outside it) follows the nearest edge height.
    add(&mut ed, site(100.0, 0.0));
    let out = block(20_000.0, 2000.0, 2000.0, 2000.0, 3000.0);
    let id = add(&mut ed, Shape::Massing(Box::new(out)));
    let terrain = SiteTerrain::new(&cache, ed.revision(), ed.doc());
    let o = ed.doc().object(id).unwrap();
    let Shape::Massing(m) = &o.shape else {
        panic!()
    };
    assert!((terrain.massing_base(o, m) - 100.0).abs() < 1e-6);
}

proptest! {
    #[test]
    fn a_building_never_floats_or_sinks(
        z0 in -500.0..500.0f64,
        slope in -0.2..0.2f64,
        x in 500.0..4000.0f64,
        y in 500.0..4000.0f64,
        w in 500.0..3000.0f64,
        d in 500.0..3000.0f64,
        roof in 0..3usize,
    ) {
        let mut ed = Editor::default();
        add(&mut ed, site(z0, slope));
        let mut m = block(x, y, w, d, 3000.0);
        m.roof = [RoofStyle::Flat, RoofStyle::Gable, RoofStyle::Hip][roof];
        let id = add(&mut ed, Shape::Massing(Box::new(m)));
        let cache = SiteCache::default();
        let terrain = SiteTerrain::new(&cache, ed.revision(), ed.doc());
        let o = ed.doc().object(id).unwrap();
        let Shape::Massing(mm) = &o.shape else { unreachable!() };
        let base = terrain.massing_base(o, mm);
        // The base is the lowest ground height of the samples: no corner of the footprint is below it.
        let lowest = mm.ring().iter().map(|p| z0 + p.x * slope).fold(f64::INFINITY, f64::min);
        prop_assert!((base - lowest).abs() < 1e-6 || base <= lowest + 1e-6);
        prop_assert!(base <= lowest + 1e-6);
    }
}
