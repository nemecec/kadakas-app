//! Whatever the editor accepts, the 3D scene has finite bounds and the camera stays valid.

use glam::{DVec2, DVec3};
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::corners;
use kadakas_core::mesh::build;
use kadakas_core::model::{Attributes, Shape};
use kadakas_render::{Camera, Projection, RenderMode, RenderOptions, StandardView, render};
use proptest::prelude::*;

/// Numbers from tiny to far beyond what the document accepts, and some that are not numbers at all.
fn num() -> impl Strategy<Value = f64> {
    prop_oneof![
        4 => -5.0e4..5.0e4f64,
        2 => -2.0e9..2.0e9f64,
        1 => prop::sample::select(vec![0.0, 1.0, 1e9, -1e9, 1.0000001e9, 1e12, 1e39, 1e300, f64::NAN, f64::INFINITY, -0.0]),
    ]
}

fn point() -> impl Strategy<Value = DVec2> {
    (num(), num()).prop_map(|(x, y)| DVec2::new(x, y))
}

fn point3() -> impl Strategy<Value = DVec3> {
    (num(), num(), num()).prop_map(|(x, y, z)| DVec3::new(x, y, z))
}

fn shape() -> impl Strategy<Value = Shape> {
    prop_oneof![
        (point(), point()).prop_map(|(a, b)| Shape::Line { a, b }),
        (point(), point()).prop_map(|(min, max)| Shape::Rect { min, max }),
        (point(), num()).prop_map(|(center, radius)| Shape::Circle { center, radius }),
        (point(), num(), num()).prop_map(|(at, spread, height)| Shape::Plant {
            at,
            species: "p".into(),
            spread,
            height
        }),
        (prop::collection::vec(point(), 0..8), num(), num()).prop_map(|(pts, t, e)| {
            Shape::Hardscape {
                boundary: corners(&pts),
                material: "m".into(),
                thickness: Some(t),
                elevation: Some(e),
            }
        }),
        (prop::collection::vec(point(), 0..8), any::<bool>()).prop_map(|(pts, closed)| {
            Shape::Polyline {
                points: corners(&pts),
                closed,
            }
        }),
        point3().prop_map(|p| Shape::Locus3d { p }),
        (prop::collection::vec(point3(), 0..8), any::<bool>())
            .prop_map(|(points, closed)| Shape::Polyline3d { points, closed }),
        prop::collection::vec(point(), 0..8).prop_map(|pts| Shape::LandscapeArea {
            boundary: corners(&pts),
            name: "a".into()
        }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn accepted_documents_give_finite_scenes_and_valid_cameras(shapes in prop::collection::vec(shape(), 1..6), elevation in num()) {
        let mut ed = Editor::default();
        let _ = ed.execute(&Command::AddLayer { name: "L".into(), elevation, scale: None, kind: None });
        let layer = ed.doc().layers.last().map(|l| l.id);
        for shape in shapes {
            let _ = ed.execute(&Command::AddObject { shape, layer, class: None, attrs: Attributes::default() });
        }
        let scene = build(ed.doc());
        if let Some((lo, hi)) = scene.bounds {
            prop_assert!(lo.is_finite() && hi.is_finite(), "bounds {lo:?} {hi:?}");
        }
        prop_assert!(scene.mesh.positions.iter().all(|p| p.is_finite()));
        prop_assert!(scene.lines.positions.iter().all(|p| p.is_finite()));
        for view in StandardView::ALL {
            for projection in [Projection::Orthographic, Projection::Perspective] {
                for aspect in [0.0, 0.5, 1.7, f32::NAN] {
                    let mut cam = Camera::standard(view, projection);
                    cam.fit(scene.bounds, aspect);
                    prop_assert!(cam.target.is_finite() && cam.distance.is_finite() && cam.distance > 0.0, "{cam:?}");
                    let m = cam.view_proj(1.5, scene.bounds);
                    prop_assert!(m.to_cols_array().iter().all(|x| x.is_finite()), "matrix of {cam:?}");
                }
            }
        }
        let mut cam = Camera::standard(StandardView::RightIsometric, Projection::Perspective);
        cam.fit(scene.bounds, 1.3);
        let img = render(&scene, &cam, &RenderOptions { width: 24, height: 16, mode: RenderMode::Shaded, ..Default::default() });
        prop_assert_eq!(img.rgba.len(), 24 * 16 * 4);
    }
}
