//! Vertex types for the GPU and the conversion from the core scene. No GPU calls, so
//! the selection overlay and the tests can use them without the renderer.

use bytemuck::{Pod, Zeroable};
use cadektor_core::mesh::Scene;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct SurfaceVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct LineVertex {
    pub pos: [f32; 3],
    pub color: [u8; 4],
}

/// The vertices of the scene's triangles.
pub fn surface_vertices(scene: &Scene) -> Vec<SurfaceVertex> {
    let m = &scene.mesh;
    (0..m.positions.len())
        .map(|i| SurfaceVertex { pos: m.positions[i].to_array(), normal: m.normals[i].to_array(), color: m.colors[i] })
        .collect()
}

/// The end points of the scene's line segments.
pub fn line_vertices(scene: &Scene) -> Vec<LineVertex> {
    let l = &scene.lines;
    (0..l.positions.len()).map(|i| LineVertex { pos: l.positions[i].to_array(), color: l.colors[i / 2] }).collect()
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
            shape: Shape::Rect { min: DVec2::ZERO, max: DVec2::splat(100.0) },
            layer: None,
            class: None,
            attrs: Attributes { fill: Some(Color([1, 2, 3, 255])), ..Default::default() },
        })
        .unwrap();
        let scene = build(ed.doc());
        let s = surface_vertices(&scene);
        assert_eq!(s.len(), scene.mesh.positions.len());
        assert_eq!(s[0].color, [1, 2, 3, 255]);
        assert_eq!(s[0].normal, [0.0, 0.0, 1.0]);
        let l = line_vertices(&scene);
        assert_eq!(l.len(), scene.lines.positions.len());
        assert_eq!(l[0].color, l[1].color);
        assert_eq!(std::mem::size_of::<SurfaceVertex>(), 28);
        assert_eq!(std::mem::size_of::<LineVertex>(), 16);
    }
}
