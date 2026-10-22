//! The landscape objects in the modeler (MOD-05, round B): what Entity Info says about them and
//! the window and crossing selection of objects that are no surface models.

use super::*;
use glam::DVec3;
use kadakas_core::command::Command;
use kadakas_core::model::{Attributes, Shape};

use crate::modeler::{Summary, summary_of};

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

fn square(x: f64, y: f64, half: f64) -> Vec<kadakas_core::curve::Vertex> {
    kadakas_core::curve::corners(&[
        DVec2::new(x - half, y - half),
        DVec2::new(x + half, y - half),
        DVec2::new(x + half, y + half),
        DVec2::new(x - half, y + half),
    ])
}

#[test]
fn every_kind_of_landscape_object_has_a_summary() {
    let mut ed = Editor::with_sequential_ids();
    let plant = add(
        &mut ed,
        Shape::Plant {
            at: DVec2::ZERO,
            species: "Tilia cordata".into(),
            spread: 800.0,
            height: 2500.0,
        },
    );
    let tree = add(
        &mut ed,
        Shape::ExistingTree {
            at: DVec2::new(5000.0, 0.0),
            species: "Pinus".into(),
            dbh: 300.0,
            spread: 4000.0,
            height: 12000.0,
            condition: Default::default(),
            action: Default::default(),
            rpz_factor: None,
            rpz_cap: None,
        },
    );
    let slab = add(
        &mut ed,
        Shape::Hardscape {
            boundary: square(0.0, 5000.0, 1000.0),
            material: "Setts".into(),
            thickness: None,
            elevation: None,
        },
    );
    let lawn = add(
        &mut ed,
        Shape::LandscapeArea {
            boundary: square(5000.0, 5000.0, 500.0),
            name: "Lawn".into(),
        },
    );
    let site = add(
        &mut ed,
        Shape::SiteModel {
            points: vec![
                DVec3::new(0.0, 0.0, 0.0),
                DVec3::new(1000.0, 0.0, 100.0),
                DVec3::new(0.0, 1000.0, 300.0),
            ],
            lines: Vec::new(),
            settings: Default::default(),
        },
    );
    let empty_site = add(
        &mut ed,
        Shape::SiteModel {
            points: Vec::new(),
            lines: Vec::new(),
            settings: Default::default(),
        },
    );
    let text = add(
        &mut ed,
        Shape::Text {
            at: DVec2::ZERO,
            text: "x".into(),
            height: 100.0,
        },
    );
    let doc = ed.doc();
    let s = |id| summary_of(doc, doc.object(id).unwrap());
    assert_eq!(
        s(plant),
        Some(Summary::Plant {
            species: "Tilia cordata".into(),
            style: None,
            height: 2500.0,
            spread: 800.0
        })
    );
    assert_eq!(
        s(tree),
        Some(Summary::ExistingTree {
            species: "Pinus".into(),
            height: 12000.0,
            spread: 4000.0,
            dbh: 300.0
        })
    );
    match s(slab) {
        Some(Summary::Hardscape {
            area,
            style,
            material,
        }) => {
            assert!((area - 4_000_000.0).abs() < 1.0 && style.is_none() && material == "Setts");
        }
        other => panic!("{other:?}"),
    }
    match s(lawn) {
        Some(Summary::LandscapeArea { name, area, .. }) => {
            assert_eq!(name, "Lawn");
            assert!((area - 1_000_000.0).abs() < 1.0);
        }
        other => panic!("{other:?}"),
    }
    match s(site) {
        Some(Summary::SiteModel {
            points,
            extent: Some(e),
            tile: None,
            ..
        }) => {
            assert_eq!(points, 3);
            assert_eq!(e, [1000.0, 1000.0, 300.0]);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(s(empty_site), None, "no data, no extent, no summary");
    assert_eq!(s(text), None);
    for kind in [s(plant), s(tree), s(slab), s(lawn), s(site)] {
        assert!(kind.unwrap().needs_app());
    }
}

#[test]
fn a_site_model_with_the_texture_look_shows_its_tile() {
    let mut ed = Editor::with_sequential_ids();
    let settings = kadakas_core::site::SiteSettings {
        surface_look: kadakas_core::site::SurfaceLook::Texture,
        texture_tile: 2500.0,
        ..Default::default()
    };
    let site = add(
        &mut ed,
        Shape::SiteModel {
            points: vec![DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0)],
            lines: vec![kadakas_core::site::SiteLine {
                points: vec![DVec3::new(0.0, 5.0, 1.0), DVec3::new(10.0, 5.0, 1.0)],
                closed: false,
            }],
            settings,
        },
    );
    let doc = ed.doc();
    match summary_of(doc, doc.object(site).unwrap()) {
        Some(Summary::SiteModel {
            lines,
            tile: Some(t),
            extent: Some(e),
            ..
        }) => {
            assert_eq!((lines, t), (1, 2500.0));
            assert_eq!(e, [10.0, 5.0, 1.0], "the lines count for the extent");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn groups_and_components_have_summaries_that_need_no_app() {
    let mut ed = Editor::with_sequential_ids();
    let a = add(
        &mut ed,
        Shape::Plant {
            at: DVec2::ZERO,
            species: "A".into(),
            spread: 100.0,
            height: 100.0,
        },
    );
    let grp = ed
        .execute(&Command::Group {
            ids: vec![a],
            name: None,
        })
        .map(|o| ObjectId(o.created[0]))
        .unwrap();
    let doc = ed.doc();
    let sum = summary_of(doc, doc.object(grp).unwrap()).unwrap();
    assert_eq!(sum, Summary::Group { members: 1 });
    assert!(!sum.needs_app());
    assert!(!Summary::Component { name: "C".into() }.needs_app());
    assert!(Summary::Symbol { name: "S".into() }.needs_app());
}

/// The world camera looking straight down at the middle of the scene.
fn down_camera(scene: &kadakas_core::mesh::Scene, height: f64) -> Camera {
    let (lo, hi) = scene.bounds.unwrap();
    let mid = scene.origin + ((lo + hi) / 2.0).as_dvec3();
    Camera::look_at(
        mid + DVec3::Z * 50_000.0,
        mid,
        DVec3::Y,
        Projection::Orthographic { height },
        DVec2::new(800.0, 800.0),
    )
    .unwrap()
}

#[test]
fn a_window_takes_objects_inside_a_crossing_the_ones_it_touches_and_surface_models_are_left_out() {
    let mut ed = Editor::with_sequential_ids();
    let left = add(
        &mut ed,
        Shape::Hardscape {
            boundary: square(-3000.0, 0.0, 1000.0),
            material: "a".into(),
            thickness: Some(100.0),
            elevation: None,
        },
    );
    let right = add(
        &mut ed,
        Shape::Hardscape {
            boundary: square(3000.0, 0.0, 1000.0),
            material: "b".into(),
            thickness: Some(100.0),
            elevation: None,
        },
    );
    let plant = add(
        &mut ed,
        Shape::Plant {
            at: DVec2::new(0.0, 3000.0),
            species: "P".into(),
            spread: 1500.0,
            height: 2000.0,
        },
    );
    let model = add(&mut ed, Shape::SurfaceModel(Box::default()));
    ed.execute(&Command::EditSurface {
        id: model,
        edit: kadakas_core::surface::Edit::AddEdges {
            edges: (0..4)
                .map(|i| {
                    let p = [
                        [-500.0, -500.0, 0.0],
                        [500.0, -500.0, 0.0],
                        [500.0, 500.0, 0.0],
                        [-500.0, 500.0, 0.0],
                    ];
                    [p[i], p[(i + 1) % 4]]
                })
                .collect(),
        },
    })
    .unwrap();
    let scene = kadakas_core::mesh::build(ed.doc());
    let cam = down_camera(&scene, 9000.0);
    let doc = ed.doc();
    let px = |x: f64, y: f64| cam.project(DVec3::new(x, y, 0.0)).unwrap();
    let rect = |a: (f64, f64), b: (f64, f64), crossing: bool| {
        let mut ids =
            select::rect_select_objects(doc, &scene, &cam, px(a.0, a.1), px(b.0, b.1), crossing);
        ids.sort();
        ids
    };
    // A window around the left slab only.
    assert_eq!(
        rect((-4500.0, -1500.0), (-1500.0, 1500.0), false),
        vec![left]
    );
    // Around both slabs and the plant: all three, not the surface model.
    let mut all = vec![left, right, plant];
    all.sort();
    assert_eq!(rect((-4500.0, -1500.0), (4500.0, 4500.0), false), all);
    // A window that cuts the right slab does not take it.
    assert_eq!(
        rect((-4500.0, -1500.0), (3000.0, 1500.0), false),
        vec![left]
    );
    // A crossing that touches the right slab takes it, not the left slab and not the plant.
    assert_eq!(rect((2500.0, 500.0), (3500.0, -500.0), true), vec![right]);
    // A crossing over the plant and the edge of nothing else takes the plant.
    assert_eq!(rect((-200.0, 3500.0), (200.0, 4500.0), true), vec![plant]);
    // A crossing rectangle inside a large face still takes the object (the slab's top).
    assert_eq!(rect((2900.0, 100.0), (3100.0, -100.0), true), vec![right]);
    // A crossing over nothing takes nothing.
    assert!(rect((-300.0, 1500.0), (300.0, 1700.0), true).is_empty());
    // The surface model is never offered here.
    assert!(!rect((-600.0, -600.0), (600.0, 600.0), true).contains(&model));
}

#[test]
fn objects_behind_the_camera_are_not_taken_by_a_window() {
    let mut ed = Editor::with_sequential_ids();
    let slab = add(
        &mut ed,
        Shape::Hardscape {
            boundary: square(0.0, 0.0, 1000.0),
            material: "a".into(),
            thickness: Some(100.0),
            elevation: None,
        },
    );
    let scene = kadakas_core::mesh::build(ed.doc());
    // The eye is below the slab and looks down: the slab is behind it.
    let cam = Camera::look_at(
        DVec3::new(0.0, 0.0, -5000.0),
        DVec3::new(0.0, 0.0, -6000.0),
        DVec3::Y,
        Projection::Perspective { fov_y: 0.6 },
        DVec2::new(800.0, 800.0),
    )
    .unwrap();
    let all = select::rect_select_objects(
        ed.doc(),
        &scene,
        &cam,
        DVec2::new(0.0, 0.0),
        DVec2::new(800.0, 800.0),
        false,
    );
    assert!(!all.contains(&slab));
    let touched = select::rect_select_objects(
        ed.doc(),
        &scene,
        &cam,
        DVec2::new(0.0, 0.0),
        DVec2::new(800.0, 800.0),
        true,
    );
    assert!(!touched.contains(&slab));
}
