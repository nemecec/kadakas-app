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

/// A closed box of 1000 x 1000 x 1000 mm with its corner at `(x, 0, 0)`: a square pulled up.
pub fn add_surface_box(ed: &mut Editor, x: f64) -> ObjectId {
    let id = add(ed, Shape::SurfaceModel(Box::default()), None);
    let p = [
        [x, 0.0, 0.0],
        [x + 1000.0, 0.0, 0.0],
        [x + 1000.0, 1000.0, 0.0],
        [x, 1000.0, 0.0],
    ];
    ed.execute(&Command::EditSurface {
        id,
        edit: kadakas_core::surface::Edit::AddEdges {
            edges: (0..4).map(|i| [p[i], p[(i + 1) % 4]]).collect(),
        },
    })
    .unwrap();
    let face = match &ed.doc().object(id).unwrap().shape {
        Shape::SurfaceModel(m) => m.faces().next().unwrap().0,
        _ => unreachable!(),
    };
    ed.execute(&Command::EditSurface {
        id,
        edit: kadakas_core::surface::Edit::PushPull {
            face,
            distance: 1000.0,
        },
    })
    .unwrap();
    id
}

/// A flat surface model of `n` x `n` squares of 10 mm, each cut into two triangles: `2 n n` faces,
/// `3 n n + 2 n` edges. Built from its parts, as a file has them (ids from one counter).
pub fn add_surface_grid(ed: &mut Editor, n: usize) -> ObjectId {
    use kadakas_core::surface::{Edge, EdgeId, Face, FaceId, Plane, SurfaceModel, VertexId};
    use std::collections::{BTreeMap, HashSet};
    let id_of = |x: usize, y: usize| VertexId((y * (n + 1) + x) as u32 + 1);
    let vertices: BTreeMap<VertexId, DVec3> = (0..=n)
        .flat_map(|y| (0..=n).map(move |x| (x, y)))
        .map(|(x, y)| {
            (
                id_of(x, y),
                DVec3::new(x as f64 * 10.0, y as f64 * 10.0, 0.0),
            )
        })
        .collect();
    let (mut edges, mut faces) = (BTreeMap::new(), BTreeMap::new());
    let (mut seen, mut e, mut f) = (HashSet::new(), 0u32, 0u32);
    let up = Plane::new(DVec3::ZERO, DVec3::Z).unwrap();
    for y in 0..n {
        for x in 0..n {
            let (a, b, c, d) = (
                id_of(x, y),
                id_of(x + 1, y),
                id_of(x + 1, y + 1),
                id_of(x, y + 1),
            );
            for tri in [[a, b, c], [a, c, d]] {
                for k in 0..3 {
                    let (p, q) = (tri[k], tri[(k + 1) % 3]);
                    if seen.insert((p.min(q), p.max(q))) {
                        e += 1;
                        edges.insert(EdgeId(1_000_000 + e), Edge { v: [p, q] });
                    }
                }
                f += 1;
                faces.insert(FaceId(2_000_000 + f), Face::new(up, tri.to_vec(), vec![]));
            }
        }
    }
    add(
        ed,
        Shape::SurfaceModel(Box::new(SurfaceModel::from_parts(
            vertices, edges, faces, 3_000_000,
        ))),
        None,
    )
}
