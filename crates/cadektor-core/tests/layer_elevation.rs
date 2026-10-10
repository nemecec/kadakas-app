//! DOC-11: a design layer has an elevation, and objects have a 3D height. The elevation of the
//! layer is the zero of everything on it. Stories are not part of Cadektor yet: a layer with an
//! elevation does the same job for landscape work.

use cadektor_core::command::{Command, Editor};
use cadektor_core::curve::corners;
use cadektor_core::mesh;
use cadektor_core::model::{Attributes, LayerId, Shape};
use glam::DVec2;

fn square(side: f64) -> Vec<cadektor_core::curve::Vertex> {
    corners(&[
        DVec2::ZERO,
        DVec2::new(side, 0.0),
        DVec2::new(side, side),
        DVec2::new(0.0, side),
    ])
}

fn layer(ed: &mut Editor, name: &str, elevation: f64) -> LayerId {
    LayerId(
        ed.execute(&Command::AddLayer {
            name: name.into(),
            elevation,
            scale: None,
            kind: None,
        })
        .unwrap()
        .created[0],
    )
}

fn add(ed: &mut Editor, layer: LayerId, shape: Shape) {
    ed.execute(&Command::AddObject {
        shape,
        layer: Some(layer),
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
}

/// The lowest and highest world height of the scene, in mm.
fn z_range(ed: &Editor) -> (f64, f64) {
    let scene = mesh::build(ed.doc());
    let (lo, hi) = scene.bounds.expect("the scene has geometry");
    (
        scene.origin.z + f64::from(lo.z),
        scene.origin.z + f64::from(hi.z),
    )
}

#[test]
fn a_hardscape_stands_at_the_elevation_of_its_layer_with_its_thickness() {
    let mut ed = Editor::default();
    let upper = layer(&mut ed, "Terrace", 2000.0);
    add(
        &mut ed,
        upper,
        Shape::Hardscape {
            boundary: square(4000.0),
            material: "Paving".into(),
            thickness: Some(150.0),
            elevation: Some(300.0),
        },
    );
    // The top is 300 above the layer, the bottom is the thickness lower.
    let (lo, hi) = z_range(&ed);
    assert!((hi - 2300.0).abs() < 1.0, "{hi}");
    assert!((lo - 2150.0).abs() < 1.0, "{lo}");
    // Moving the layer moves the object.
    ed.execute(&Command::ModifyLayer {
        id: upper,
        name: None,
        visible: None,
        locked: None,
        elevation: Some(-500.0),
        scale: None,
    })
    .unwrap();
    let (lo, hi) = z_range(&ed);
    assert!(
        (hi + 200.0).abs() < 1.0 && (lo + 350.0).abs() < 1.0,
        "{lo} {hi}"
    );
    // One undo step.
    ed.undo();
    assert!((z_range(&ed).1 - 2300.0).abs() < 1.0);
}

#[test]
fn heights_add_up_on_layers_with_an_elevation() {
    let mut ed = Editor::default();
    let low = layer(&mut ed, "Ground", 0.0);
    let high = layer(&mut ed, "Roof", 3000.0);
    add(
        &mut ed,
        low,
        Shape::Plant {
            at: DVec2::new(500.0, 500.0),
            species: "Acer".into(),
            spread: 2000.0,
            height: 4000.0,
        },
    );
    let (_, plant_top) = z_range(&ed);
    assert!(plant_top > 3000.0 && plant_top <= 4100.0, "{plant_top}");
    add(
        &mut ed,
        high,
        Shape::Plant {
            at: DVec2::new(5000.0, 500.0),
            species: "Acer".into(),
            spread: 2000.0,
            height: 4000.0,
        },
    );
    let (lo, hi) = z_range(&ed);
    // The same plant on the layer 3 m higher reaches 3 m higher.
    assert!(hi > plant_top + 2900.0, "{hi} {plant_top}");
    assert!(lo < 100.0, "{lo}");
}

#[test]
fn bad_elevations_are_refused() {
    let mut ed = Editor::default();
    for e in [f64::NAN, f64::INFINITY] {
        let r = ed.execute(&Command::AddLayer {
            name: "X".into(),
            elevation: e,
            scale: None,
            kind: None,
        });
        assert!(r.is_err(), "{e}");
    }
    assert_eq!(ed.doc().layers.len(), 1);
}
