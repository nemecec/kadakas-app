//! Vertex types for the GPU and the conversion from the core scene. No GPU calls, so
//! the selection overlay and the tests can use them without the renderer.

use bytemuck::{Pod, Zeroable};
use kadakas_core::mesh::Scene;
use kadakas_render::look::{Look, object_ranks};

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
    /// Place of the object in the drawing order (capped). Later objects move toward the viewer by
    /// `order * DEPTH_RANK_STEP`, so coplanar surfaces of different objects do not flicker.
    pub order: f32,
    /// Bit flags, see `FLAG_TWO_SIDED`.
    pub flags: u32,
}

/// The vertex belongs to a two-sided surface: the shader draws a back face in the back color
/// (not when the vertex has a material: then the material shows on both sides).
pub const FLAG_TWO_SIDED: u32 = 1;
/// The vertex belongs to a triangle that is drawn only when seen from its front (Paint Bucket).
pub const FLAG_FRONT_ONLY: u32 = 2;
/// The vertex belongs to a triangle that is drawn only when seen from its back.
pub const FLAG_BACK_ONLY: u32 = 4;

/// The material index of a vertex without material.
pub const NO_MATERIAL: u32 = u32::MAX;

/// Most materials the shaders know. Vertices of other materials keep their vertex color.
pub const MAX_MATERIALS: usize = 64;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct LineVertex {
    pub pos: [f32; 3],
    pub color: [u8; 4],
    /// The first end point of the segment that this vertex belongs to. Dashes are measured from it.
    pub start: [f32; 3],
}

impl LineVertex {
    /// The two vertices of one segment.
    pub fn segment(a: [f32; 3], b: [f32; 3], color: [u8; 4]) -> [LineVertex; 2] {
        [
            LineVertex {
                pos: a,
                color,
                start: a,
            },
            LineVertex {
                pos: b,
                color,
                start: a,
            },
        ]
    }
}

/// The vertices of the scene's triangles. With a look, vertices carry texture coordinates and the
/// index of their material.
pub fn surface_vertices(scene: &Scene, look: Option<&Look>) -> Vec<SurfaceVertex> {
    let m = &scene.mesh;
    let ranks = object_ranks(scene);
    // The object of each vertex: the object of the first triangle that uses it.
    let mut owner = vec![None; m.positions.len()];
    let mut sided = vec![0u32; m.positions.len()];
    for t in 0..m.triangle_count() {
        let side = match m.side_of(t) {
            kadakas_core::mesh::SIDE_FRONT => FLAG_FRONT_ONLY,
            kadakas_core::mesh::SIDE_BACK => FLAG_BACK_ONLY,
            _ => 0,
        };
        for k in 0..3 {
            let v = m.indices[t * 3 + k] as usize;
            owner[v].get_or_insert(m.tri_object[t]);
            sided[v] |= side;
        }
    }
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
                order: owner[i]
                    .and_then(|o| ranks.get(&o))
                    .map_or(0.0, |r| *r as f32),
                flags: sided[i] | if m.is_two_sided(i) { FLAG_TWO_SIDED } else { 0 },
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
            start: l.positions[i - i % 2].to_array(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::mesh::build;
    use kadakas_core::model::{Attributes, Color, Shape};

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
        assert_eq!(std::mem::size_of::<SurfaceVertex>(), 48);
        assert_eq!(s[0].material, NO_MATERIAL);
        assert_eq!(std::mem::size_of::<LineVertex>(), 28);
    }
}
