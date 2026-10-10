//! Materials of the exports.
//!
//! The exports ask a `MaterialSource` for the material of every group of triangles.
//! `DocumentMaterials` is the source for documents: it resolves the material resources of the
//! document (slots on parts, objects, styles and classes, the same as the renderers) into PBR
//! values and base color textures. `BaseColors` gives the plain base color of the scene. The
//! writers put the result into glTF (PBR metallic roughness, with embedded images) and into MTL
//! (with texture files).

use cadektor_core::mesh::Scene;
use cadektor_core::model::{Document, Object};
use cadektor_render::look::{Look, NO_MATERIAL};
use cadektor_render::texture::encode_png;

/// An image for the base color of a material. The writers make texture coordinates from the
/// model positions (box projection): one repeat of the image covers `repeat` metres.
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub name: String,
    /// `image/png` or `image/jpeg`.
    pub mime: String,
    pub data: Vec<u8>,
    /// Metres that one repeat covers.
    pub repeat: f64,
}

/// A material, in the terms of glTF PBR metallic roughness.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    /// Linear RGBA, 0 to 1.
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: [f32; 3],
    pub double_sided: bool,
    pub texture: Option<Texture>,
}

impl Material {
    /// A plain material from an 8 bit sRGB color. Transparent colors blend.
    pub fn from_color(c: [u8; 4]) -> Material {
        let lin = |v: u8| {
            let s = v as f32 / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        Material {
            name: format!("color_{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]),
            base_color: [lin(c[0]), lin(c[1]), lin(c[2]), c[3] as f32 / 255.0],
            metallic: 0.0,
            roughness: 0.9,
            emissive: [0.0; 3],
            double_sided: false,
            texture: None,
        }
    }

    /// True if the color needs blending.
    pub fn is_transparent(&self) -> bool {
        self.base_color[3] < 1.0
    }

    /// The color as 8 bit sRGB, for formats that have no linear colors (MTL).
    pub fn srgb(&self) -> [f32; 3] {
        let enc = |l: f32| {
            let l = l.clamp(0.0, 1.0);
            if l <= 0.003_130_8 {
                l * 12.92
            } else {
                1.055 * l.powf(1.0 / 2.4) - 0.055
            }
        };
        [
            enc(self.base_color[0]),
            enc(self.base_color[1]),
            enc(self.base_color[2]),
        ]
    }
}

/// Gives the material of a group of triangles. `color` is the color that the 3D scene gave them.
pub trait MaterialSource {
    fn material(&self, doc: &Document, obj: &Object, color: [u8; 4]) -> Material;

    /// The material of the group that triangle `tri` of the scene starts. The default does not
    /// look at the triangle.
    fn material_for(&self, doc: &Document, obj: &Object, _tri: usize, color: [u8; 4]) -> Material {
        self.material(doc, obj, color)
    }

    /// Triangles with different groups never share a primitive, even with the same color.
    fn group(&self, _tri: usize) -> u32 {
        0
    }

    /// Texture coordinates of a vertex of the scene, in repeats of the image with V up. `None`
    /// lets the writers project the texture from the box of the normal.
    fn uv(&self, _vertex: usize) -> Option<[f32; 2]> {
        None
    }
}

/// The materials of the document: the material resources that the slots give to the triangles
/// of the scene, with the texture mapping of each material. Triangles without a slot keep the
/// color of the scene. Textures are embedded as PNG images.
pub struct DocumentMaterials {
    look: Look,
    /// The material for each entry of `look.materials`, with the name of the resource.
    materials: Vec<Material>,
}

impl DocumentMaterials {
    pub fn new(doc: &Document, scene: &Scene) -> DocumentMaterials {
        let look = Look::build(doc, scene, true);
        let materials = look
            .materials
            .iter()
            .map(|m| {
                let name = doc
                    .resources
                    .get(m.id)
                    .map_or_else(|| format!("material_{}", m.id.0), |r| r.name.clone());
                let mut out = Material::from_color([m.color[0], m.color[1], m.color[2], 255]);
                out.name = name.clone();
                out.base_color[3] = m.opacity.clamp(0.0, 1.0);
                out.metallic = m.metallic;
                out.roughness = m.roughness;
                // Glass and water are seen from both sides.
                out.double_sided = m.opacity < 0.999;
                if let Some(img) = m.texture.and_then(|i| look.textures.get(i))
                    && let Ok(data) = encode_png(img.width, img.height, &img.rgba)
                {
                    // The texture is tinted already (generated patterns carry the base color).
                    out.base_color = [1.0, 1.0, 1.0, out.base_color[3]];
                    out.texture = Some(Texture {
                        name: name.clone(),
                        mime: "image/png".into(),
                        data,
                        repeat: 1.0,
                    });
                }
                out
            })
            .collect();
        DocumentMaterials { look, materials }
    }

    /// True if no triangle has a material of the document.
    pub fn is_plain(&self) -> bool {
        self.look.is_plain()
    }
}

impl MaterialSource for DocumentMaterials {
    fn material(&self, _doc: &Document, _obj: &Object, color: [u8; 4]) -> Material {
        Material::from_color(color)
    }

    fn material_for(&self, _doc: &Document, _obj: &Object, tri: usize, color: [u8; 4]) -> Material {
        match self.look.tri_material.get(tri) {
            Some(&i) if i != NO_MATERIAL => self
                .materials
                .get(i as usize)
                .cloned()
                .unwrap_or_else(|| Material::from_color(color)),
            _ => Material::from_color(color),
        }
    }

    fn group(&self, tri: usize) -> u32 {
        self.look
            .tri_material
            .get(tri)
            .copied()
            .unwrap_or(NO_MATERIAL)
    }

    fn uv(&self, vertex: usize) -> Option<[f32; 2]> {
        match self.look.vertex_material.get(vertex) {
            Some(&i) if i != NO_MATERIAL => {
                self.look.uvs.get(vertex).map(|uv| [uv[0], 1.0 - uv[1]])
            }
            _ => None,
        }
    }
}

/// The base color of the scene, nothing else. This is what the exports use until the material
/// resources of the document are connected.
#[derive(Clone, Copy, Debug, Default)]
pub struct BaseColors;

impl MaterialSource for BaseColors {
    fn material(&self, _doc: &Document, _obj: &Object, color: [u8; 4]) -> Material {
        Material::from_color(color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_convert_both_ways() {
        let m = Material::from_color([255, 128, 0, 255]);
        assert_eq!(m.name, "color_ff8000ff");
        assert!((m.base_color[0] - 1.0).abs() < 1e-6);
        assert!((m.base_color[1] - 0.2158).abs() < 1e-3);
        assert_eq!(m.base_color[2], 0.0);
        assert!(!m.is_transparent());
        let s = m.srgb();
        assert!((s[1] - 128.0 / 255.0).abs() < 1e-3);
        assert!(Material::from_color([0, 0, 0, 100]).is_transparent());
        // The dark end uses the linear segment.
        let dark = Material::from_color([5, 5, 5, 255]);
        assert!((dark.srgb()[0] - 5.0 / 255.0).abs() < 1e-3);
    }

    #[test]
    fn base_colors_ignore_the_object() {
        let doc = Document::default();
        let obj = Object::new(
            cadektor_core::model::ObjectId::from_num(1),
            cadektor_core::model::LayerId::from_num(1),
            cadektor_core::model::ClassId::from_num(1),
            cadektor_core::model::Shape::Locus3d {
                p: glam::DVec3::ZERO,
            },
        );
        let m = BaseColors.material(&doc, &obj, [1, 2, 3, 255]);
        assert_eq!(m, Material::from_color([1, 2, 3, 255]));
    }
}
