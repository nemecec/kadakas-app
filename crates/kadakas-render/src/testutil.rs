//! Helpers for the tests of this crate.

use glam::{DVec2, DVec3, Vec3};
use kadakas_core::command::{Command, Editor};
use kadakas_core::mesh::{Scene, build_at};
use kadakas_core::model::{Attributes, Color, ObjectId, Shape};

use crate::camera::{Camera, FOV_Y};
use crate::views::{Projection, StandardView};

pub fn add(ed: &mut Editor, shape: Shape, fill: Option<[u8; 4]>) -> ObjectId {
    let attrs = Attributes {
        fill: fill.map(Color),
        ..Default::default()
    };
    ObjectId(
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs,
        })
        .unwrap()
        .created[0],
    )
}

pub fn square(half: f64) -> Shape {
    Shape::Rect {
        min: DVec2::splat(-half),
        max: DVec2::splat(half),
    }
}

/// The scene of the editor's document, with the scene origin at the world origin.
pub fn scene_of(ed: &Editor) -> Scene {
    build_at(ed.doc(), DVec3::ZERO)
}

pub fn top_ortho(half_height: f32) -> Camera {
    // half_height = distance * tan(22.5 degrees)
    let mut c = Camera::standard(StandardView::Top, Projection::Orthographic);
    c.distance = half_height / (FOV_Y / 2.0).tan();
    c
}

pub fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

/// A surface model with one square face (2 * `half` mm wide, facing up) in the document.
pub fn add_surface_square(ed: &mut Editor, half: f64) -> ObjectId {
    let id = add(ed, Shape::SurfaceModel(Box::default()), None);
    let p = [
        [-half, -half, 0.0],
        [half, -half, 0.0],
        [half, half, 0.0],
        [-half, half, 0.0],
    ];
    ed.execute(&Command::EditSurface {
        id,
        edit: kadakas_core::surface::Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        },
    })
    .unwrap();
    id
}

/// The camera of `top_ortho` seen from below.
pub fn bottom_ortho(half_height: f32) -> Camera {
    let mut c = top_ortho(half_height);
    c.set_view(StandardView::Bottom);
    c
}
