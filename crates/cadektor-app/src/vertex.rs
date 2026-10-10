//! Vertex types for the GPU and the conversion from the core scene. No GPU calls, so
//! the selection overlay and the tests can use them without the renderer.

use bytemuck::{Pod, Zeroable};
use cadektor_core::mesh::Scene;
use cadektor_render::look::Look;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct SurfaceVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [u8; 4],
    /// Texture coordinates in repeats.
    pub uv: [f32; 2],
    /// Index into the material table, or `NO_MATERIAL`.
    pub material: u32,
}

/// The material index of a vertex without material.
pub const NO_MATERIAL: u32 = u32::MAX;

/// Most materials the shaders know. Vertices of other materials keep their vertex color.
pub const MAX_MATERIALS: usize = 64;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct LineVertex {
    pub pos: [f32; 3],
    pub color: [u8; 4],
}

/// The vertices of the scene's triangles. With a look, vertices carry texture coordinates and the
/// index of their material.
pub fn surface_vertices(scene: &Scene, look: Option<&Look>) -> Vec<SurfaceVertex> {
    let m = &scene.mesh;
    (0..m.positions.len())
        .map(|i| {
            let material = look
                .and_then(|l| l.vertex_material.get(i).copied())
                .filter(|m| (*m as usize) < MAX_MATERIALS)
                .unwrap_or(NO_MATERIAL);
            SurfaceVertex {
                pos: m.positions[i].to_array(),
                normal: m.normals[i].to_array(),
                color: m.colors[i],
                uv: if material == NO_MATERIAL {
                    [0.0, 0.0]
                } else {
                    look.map_or([0.0, 0.0], |l| l.uvs[i])
                },
                material,
            }
        })
        .collect()
}

/// The end points of the scene's line segments.
pub fn line_vertices(scene: &Scene) -> Vec<LineVertex> {
    let l = &scene.lines;
    (0..l.positions.len())
        .map(|i| LineVertex {
            pos: l.positions[i].to_array(),
            color: l.colors[i / 2],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_core::command::{Command, Editor};
    use cadektor_core::mesh::build;
    use cadektor_core::model::{Attributes, Color, Shape};
    use glam::DVec2;

    #[test]
    fn vertex_conversion_matches_the_scene() {
        let mut ed = Editor::default();
        ed.execute(&Command::AddObject {
            shape: Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::splat(100.0),
            },
            layer: None,
            class: None,
            attrs: Attributes {
                fill: Some(Color([1, 2, 3, 255])),
                ..Default::default()
            },
        })
        .unwrap();
        let scene = build(ed.doc());
        let s = surface_vertices(&scene, None);
        assert_eq!(s.len(), scene.mesh.positions.len());
        assert_eq!(s[0].color, [1, 2, 3, 255]);
        assert_eq!(s[0].normal, [0.0, 0.0, 1.0]);
        let l = line_vertices(&scene);
        assert_eq!(l.len(), scene.lines.positions.len());
        assert_eq!(l[0].color, l[1].color);
        assert_eq!(std::mem::size_of::<SurfaceVertex>(), 40);
        assert_eq!(s[0].material, NO_MATERIAL);
        assert_eq!(std::mem::size_of::<LineVertex>(), 16);
    }
}
