//! Materials of the exports.
//!
//! The exports ask a `MaterialSource` for the material of every group of triangles. The
//! default source, `BaseColors`, gives a plain base color. A source that knows the material
//! resources of the document (metals, roughness, textures) plugs in here, and the writers put
//! the result into glTF (PBR metallic roughness, with a base color texture) and into MTL.

use cadektor_core::model::{Document, Object};

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
            cadektor_core::model::ObjectId(1),
            cadektor_core::model::LayerId(1),
            cadektor_core::model::ClassId(1),
            cadektor_core::model::Shape::Locus3d {
                p: glam::DVec3::ZERO,
            },
        );
        let m = BaseColors.material(&doc, &obj, [1, 2, 3, 255]);
        assert_eq!(m, Material::from_color([1, 2, 3, 255]));
    }
}
