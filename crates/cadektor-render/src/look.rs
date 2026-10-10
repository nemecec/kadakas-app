//! The look of a scene: which material each triangle has, and the texture coordinates.
//!
//! [`Look::build`] resolves the material slots of the document (see `cadektor_core::present`)
//! for every triangle of a scene. The CPU renderer, the GPU renderer in the app and the hidden
//! line output all use the same result, so they agree.

use std::collections::HashMap;

use cadektor_core::mesh::Scene;
use cadektor_core::model::{Document, ObjectId};
use cadektor_core::present::{self, SlotIndex, TextureMapping, texture_uv};
use cadektor_core::resource::ResourceId;

use crate::texture::{TextureImage, downscale, load};

/// Index value for a triangle without material: it keeps the color of its vertices.
pub const NO_MATERIAL: u32 = u32::MAX;

/// Largest side of an imported texture in a look, in pixels. Larger images are scaled down.
pub const MAX_LOOK_TEXTURE: u32 = 1024;

/// A material as the renderers use it.
#[derive(Clone, Debug, PartialEq)]
pub struct LookMaterial {
    pub id: ResourceId,
    pub color: [u8; 4],
    /// Index into [`Look::textures`].
    pub texture: Option<usize>,
    pub roughness: f32,
    pub metallic: f32,
    pub opacity: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Look {
    pub materials: Vec<LookMaterial>,
    pub textures: Vec<TextureImage>,
    /// One material index per triangle, or [`NO_MATERIAL`].
    pub tri_material: Vec<u32>,
    /// One material index per vertex (the material of the first triangle that uses the vertex).
    pub vertex_material: Vec<u32>,
    /// Texture coordinates in repeats, one per vertex.
    pub uvs: Vec<[f32; 2]>,
}

impl Look {
    /// A look without materials: every triangle keeps its vertex color.
    pub fn plain(scene: &Scene) -> Look {
        Look {
            tri_material: vec![NO_MATERIAL; scene.mesh.triangle_count()],
            vertex_material: vec![NO_MATERIAL; scene.mesh.positions.len()],
            uvs: vec![[0.0, 0.0]; scene.mesh.positions.len()],
            ..Look::default()
        }
    }

    /// Resolves the materials of the document for the triangles of the scene. With `textures`
    /// off, materials give colors and opacity but no texture.
    pub fn build(doc: &Document, scene: &Scene, textures: bool) -> Look {
        let mut look = Look::plain(scene);
        let slots = SlotIndex::new(doc);
        if slots.is_empty() {
            return look;
        }
        let mesh = &scene.mesh;
        let mut by_resource: HashMap<ResourceId, u32> = HashMap::new();
        let mut by_part: HashMap<(ObjectId, u8), u32> = HashMap::new();
        // Middle and half size of each object that has a material with cylindrical mapping.
        let mut frames: HashMap<ObjectId, ([f64; 3], f64)> = HashMap::new();
        let mut mappings: Vec<TextureMapping> = Vec::new();
        for t in 0..mesh.triangle_count() {
            let (object, part) = (
                mesh.tri_object[t],
                mesh.tri_part.get(t).copied().unwrap_or(0),
            );
            let index = *by_part.entry((object, part)).or_insert_with(|| {
                let Some(obj) = doc.object(object) else {
                    return NO_MATERIAL;
                };
                let Some(rid) = slots.resolve(obj, part) else {
                    return NO_MATERIAL;
                };
                if let Some(i) = by_resource.get(&rid) {
                    return *i;
                }
                let Some(m) = present::material(doc, rid) else {
                    return NO_MATERIAL;
                };
                let texture = textures
                    .then(|| m.texture.as_ref())
                    .flatten()
                    .and_then(|tx| {
                        load(
                            tx,
                            [m.base_color.0[0], m.base_color.0[1], m.base_color.0[2]],
                        )
                    })
                    .map(|img| {
                        look.textures.push(downscale(&img, MAX_LOOK_TEXTURE));
                        look.textures.len() - 1
                    });
                look.materials.push(LookMaterial {
                    id: rid,
                    color: m.base_color.0,
                    texture,
                    roughness: m.roughness,
                    metallic: m.metallic,
                    opacity: m.opacity,
                });
                mappings.push(m.mapping);
                let i = (look.materials.len() - 1) as u32;
                by_resource.insert(rid, i);
                i
            });
            look.tri_material[t] = index;
            if index == NO_MATERIAL {
                continue;
            }
            let mapping = mappings[index as usize];
            let frame = *frames
                .entry(object)
                .or_insert_with(|| frame_of(scene, object));
            for k in 0..3 {
                let v = mesh.indices[t * 3 + k] as usize;
                if look.vertex_material[v] != NO_MATERIAL {
                    continue;
                }
                look.vertex_material[v] = index;
                look.uvs[v] = vertex_uv(scene, &mapping, v, frame);
            }
        }
        look
    }

    /// The material of triangle `t`.
    pub fn material_of(&self, t: usize) -> Option<&LookMaterial> {
        match self.tri_material.get(t) {
            Some(&i) if i != NO_MATERIAL => self.materials.get(i as usize),
            _ => None,
        }
    }

    /// True if any triangle uses a material that lets light through.
    pub fn has_transparent(&self) -> bool {
        self.tri_material
            .iter()
            .any(|&i| i != NO_MATERIAL && self.materials[i as usize].opacity < 0.999)
    }

    /// True if no triangle has a material.
    pub fn is_plain(&self) -> bool {
        self.materials.is_empty()
    }
}

fn frame_of(scene: &Scene, object: ObjectId) -> ([f64; 3], f64) {
    scene
        .object_bounds(object)
        .map_or(([0.0; 3], 1.0), |(lo, hi)| {
            let o = scene.origin;
            let c = [
                o.x + ((lo.x + hi.x) / 2.0) as f64,
                o.y + ((lo.y + hi.y) / 2.0) as f64,
                o.z + ((lo.z + hi.z) / 2.0) as f64,
            ];
            (c, (((hi.x - lo.x).max(hi.y - lo.y)) / 2.0) as f64)
        })
}

fn vertex_uv(scene: &Scene, m: &TextureMapping, v: usize, frame: ([f64; 3], f64)) -> [f32; 2] {
    let p = scene.mesh.positions[v];
    let o = scene.origin;
    let world = [o.x + p.x as f64, o.y + p.y as f64, o.z + p.z as f64];
    texture_uv(m, world, scene.mesh.normals[v].to_array(), frame.0, frame.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;
    use cadektor_core::command::{Command, Editor};
    use cadektor_core::model::Shape;
    use cadektor_core::present::{
        MappingKind, Material, Procedural, RenderResource, SlotTarget, Texture,
    };
    use cadektor_core::resource::ResourceBody;
    use glam::DVec2;

    fn add_material(ed: &mut Editor, m: Material) -> ResourceId {
        ResourceId(
            ed.execute(&Command::AddResource {
                name: format!("m{}", ed.doc().resources.len()),
                body: ResourceBody::Render(Box::new(RenderResource::Material(m))),
            })
            .unwrap()
            .created[0],
        )
    }

    fn assign(ed: &mut Editor, object: ObjectId, slot: Option<&str>, m: ResourceId) {
        ed.execute(&Command::SetMaterialSlot {
            target: SlotTarget::Object {
                object,
                slot: slot.map(String::from),
            },
            material: Some(m),
        })
        .unwrap();
    }

    #[test]
    fn a_document_without_slots_gives_a_plain_look() {
        let mut ed = Editor::default();
        add(&mut ed, square(500.0), None);
        let scene = scene_of(&ed);
        let look = Look::build(ed.doc(), &scene, true);
        assert!(look.is_plain() && !look.has_transparent());
        assert!(look.tri_material.iter().all(|m| *m == NO_MATERIAL));
        assert_eq!(look.uvs.len(), scene.mesh.positions.len());
        assert!(look.material_of(0).is_none() && look.material_of(99).is_none());
    }

    #[test]
    fn slots_give_materials_textures_and_uvs() {
        let mut ed = Editor::default();
        let a = add(&mut ed, square(500.0), None);
        let b = add(
            &mut ed,
            Shape::Rect {
                min: DVec2::new(2000.0, 0.0),
                max: DVec2::new(3000.0, 1000.0),
            },
            None,
        );
        let plant = add(
            &mut ed,
            Shape::Plant {
                at: DVec2::new(-5000.0, 0.0),
                species: "Oak".into(),
                spread: 3000.0,
                height: 6000.0,
            },
            None,
        );
        let grass = add_material(
            &mut ed,
            Material {
                texture: Some(Texture::Procedural {
                    kind: Procedural::Grass,
                    seed: 1,
                }),
                mapping: TextureMapping {
                    kind: MappingKind::Planar,
                    scale: [1000.0, 1000.0],
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let water = add_material(
            &mut ed,
            Material {
                opacity: 0.5,
                ..Default::default()
            },
        );
        assign(&mut ed, a, None, grass);
        assign(&mut ed, b, None, water);
        assign(&mut ed, plant, Some("canopy"), grass);
        let scene = scene_of(&ed);
        let look = Look::build(ed.doc(), &scene, true);
        assert_eq!(look.materials.len(), 2, "one entry per material resource");
        assert_eq!(look.textures.len(), 1, "water has no texture");
        assert!(look.has_transparent());
        let mesh = &scene.mesh;
        for t in 0..mesh.triangle_count() {
            let obj = mesh.tri_object[t];
            let part = mesh.tri_part[t];
            let m = look.material_of(t).map(|m| m.id);
            if obj == a {
                assert_eq!(m, Some(grass));
            } else if obj == b {
                assert_eq!(m, Some(water));
            } else if part == present::PART_CANOPY {
                assert_eq!(m, Some(grass));
            } else {
                assert_eq!(m, None, "the trunk has no material");
            }
        }
        // Planar uv of the first square: world position divided by 1000 mm.
        let t0 = (0..mesh.triangle_count())
            .find(|t| mesh.tri_object[*t] == a)
            .unwrap();
        for k in 0..3 {
            let v = mesh.indices[t0 * 3 + k] as usize;
            let p = mesh.positions[v];
            assert!((look.uvs[v][0] - p.x / 1000.0).abs() < 1e-4);
            assert!((look.uvs[v][1] - p.y / 1000.0).abs() < 1e-4);
        }
        // With textures off the colors stay and the images go.
        let flat = Look::build(ed.doc(), &scene, false);
        assert!(flat.textures.is_empty() && flat.materials.len() == 2);
        assert!(flat.materials.iter().all(|m| m.texture.is_none()));
    }

    #[test]
    fn a_broken_image_texture_falls_back_to_the_color() {
        let mut ed = Editor::default();
        let a = add(&mut ed, square(500.0), None);
        let m = add_material(
            &mut ed,
            Material {
                texture: Some(Texture::Image {
                    png_base64: "AAAA".into(),
                    width: 2,
                    height: 2,
                }),
                ..Default::default()
            },
        );
        assign(&mut ed, a, None, m);
        let look = Look::build(ed.doc(), &scene_of(&ed), true);
        assert_eq!(look.materials.len(), 1);
        assert!(look.materials[0].texture.is_none() && look.textures.is_empty());
    }

    #[test]
    fn cylindrical_mapping_uses_the_middle_of_the_object() {
        let mut ed = Editor::default();
        let a = add(
            &mut ed,
            Shape::Rect {
                min: DVec2::new(1000.0, 1000.0),
                max: DVec2::new(3000.0, 3000.0),
            },
            None,
        );
        let m = add_material(
            &mut ed,
            Material {
                mapping: TextureMapping {
                    kind: MappingKind::Cylindrical,
                    scale: [1000.0, 1000.0],
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        assign(&mut ed, a, None, m);
        let scene = scene_of(&ed);
        let look = Look::build(ed.doc(), &scene, true);
        // The corners at (3000, 3000) and (3000, 1000) are on opposite sides of the angle 0.
        let by_pos = |x: f32, y: f32| {
            scene
                .mesh
                .positions
                .iter()
                .position(|p| p.x == x && p.y == y)
                .unwrap()
        };
        let ne = look.uvs[by_pos(3000.0, 3000.0)][0];
        let se = look.uvs[by_pos(3000.0, 1000.0)][0];
        assert!(ne > 0.0 && se < 0.0 && (ne + se).abs() < 1e-4, "{ne} {se}");
    }
}
