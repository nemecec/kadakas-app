//! The look of a scene: which material each triangle has, and the texture coordinates.
//!
//! [`Look::build`] resolves the material slots of the document (see `kadakas_core::present`)
//! for every triangle of a scene. The CPU renderer, the GPU renderer in the app and the hidden
//! line output all use the same result, so they agree.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use kadakas_core::mesh::Scene;
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::present::{self, SlotIndex, Texture, TextureMapping};
use kadakas_core::resource::ResourceId;
use kadakas_core::site::effective_tile;

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

/// A 64 bit hash (FNV-1a) of bytes. It only tells whether content changed.
pub fn hash_bytes(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed ^ 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Depth that each step of the drawing order moves an object toward the viewer, in normalized
/// device depth. Surfaces of different objects at the same height (a lawn and a bed on it, a
/// draped path on the terrain) then do not flicker: the later object in the document is on top.
pub const DEPTH_RANK_STEP: f32 = 1e-6;

/// Most steps of the drawing order that count. Later objects share the last step.
pub const DEPTH_RANK_MAX: u32 = 200;

/// The place of each object of the scene in the drawing order of the document, capped at
/// [`DEPTH_RANK_MAX`].
pub fn object_ranks(scene: &Scene) -> HashMap<ObjectId, u32> {
    scene
        .objects()
        .enumerate()
        .map(|(i, id)| (id, (i as u32).min(DEPTH_RANK_MAX)))
        .collect()
}

/// Fingerprint of the geometry of a scene (positions, normals, colors and indices).
pub fn scene_key(scene: &Scene) -> u64 {
    let m = &scene.mesh;
    let mut h = hash_bytes(1, &(m.positions.len() as u64).to_le_bytes());
    for p in &m.positions {
        for c in p.to_array() {
            h = hash_bytes(h, &c.to_bits().to_le_bytes());
        }
    }
    for c in &m.colors {
        h = hash_bytes(h, c);
    }
    for i in &m.indices {
        h = hash_bytes(h, &i.to_le_bytes());
    }
    for s in &scene.lines.positions {
        for c in s.to_array() {
            h = hash_bytes(h, &c.to_bits().to_le_bytes());
        }
    }
    h
}

/// Fingerprint of what a texture looks like: the image data, or the pattern with its tint.
fn texture_hash(tx: &Texture, base: [u8; 3]) -> u64 {
    match tx {
        Texture::Procedural { kind, seed } => {
            let mut h = hash_bytes(2, kind.id().as_bytes());
            h = hash_bytes(h, &seed.to_le_bytes());
            hash_bytes(h, &base)
        }
        Texture::Image { png_base64, .. } => hash_bytes(3, png_base64.as_bytes()),
    }
}

/// Decoded textures that are kept between looks, by material and content. Editing a drawing does
/// not decode the same image again.
#[derive(Default)]
pub struct TextureCache {
    map: HashMap<(ResourceId, u64), Option<Arc<TextureImage>>>,
    used: HashSet<(ResourceId, u64)>,
}

impl TextureCache {
    pub fn new() -> TextureCache {
        TextureCache::default()
    }

    /// How many textures are kept.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    fn get(
        &mut self,
        id: ResourceId,
        tx: &Texture,
        base: [u8; 3],
    ) -> (Option<Arc<TextureImage>>, u64) {
        let hash = texture_hash(tx, base);
        let key = (id, hash);
        self.used.insert(key);
        let img = self
            .map
            .entry(key)
            .or_insert_with(|| load(tx, base).map(|i| Arc::new(downscale(&i, MAX_LOOK_TEXTURE))))
            .clone();
        (img, hash)
    }

    /// Forgets the textures that the last look did not use.
    fn finish(&mut self) {
        let used = std::mem::take(&mut self.used);
        self.map.retain(|k, _| used.contains(k));
    }
}

#[derive(Clone, Debug, Default)]
pub struct Look {
    pub materials: Vec<LookMaterial>,
    pub textures: Vec<Arc<TextureImage>>,
    /// Fingerprint of the materials and textures (not of the triangles that use them). The GPU
    /// uploads its material table and texture array again only when it changes.
    pub materials_key: u64,
    /// Fingerprint of the per-triangle and per-vertex assignment and the texture coordinates.
    pub layout_key: u64,
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
        Look::build_cached(doc, scene, textures, &mut TextureCache::new())
    }

    /// Like `build`, with textures kept between calls in `cache`.
    pub fn build_cached(
        doc: &Document,
        scene: &Scene,
        textures: bool,
        cache: &mut TextureCache,
    ) -> Look {
        let mut look = Look::plain(scene);
        let slots = SlotIndex::new(doc);
        if slots.is_empty() {
            cache.finish();
            return look;
        }
        let mut texture_hashes: Vec<u64> = Vec::new();
        let mesh = &scene.mesh;
        let mut by_resource: HashMap<ResourceId, u32> = HashMap::new();
        let mut by_part: HashMap<(ObjectId, u8), u32> = HashMap::new();
        // Middle and half size of each object that has a material with cylindrical mapping.
        let mut frames: HashMap<ObjectId, ([f64; 3], f64)> = HashMap::new();
        let mut mappings: Vec<TextureMapping> = Vec::new();
        // Objects that force their own mapping (a site model with the texture look), with the
        // whole repeats to take away from their texture coordinates.
        // A texture bed has its own mapping, so this is by object and part.
        let mut forced: HashMap<(ObjectId, u8), Option<(TextureMapping, [f64; 2])>> =
            HashMap::new();
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
                let texture = m
                    .texture
                    .as_ref()
                    .filter(|_| textures)
                    .and_then(|tx| {
                        let base = [m.base_color.0[0], m.base_color.0[1], m.base_color.0[2]];
                        let (img, hash) = cache.get(rid, tx, base);
                        img.map(|i| (i, hash))
                    })
                    .map(|(img, hash)| {
                        look.textures.push(img);
                        texture_hashes.push(hash);
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
            let frame = *frames
                .entry(object)
                .or_insert_with(|| frame_of(scene, object));
            let forced = *forced.entry((object, part)).or_insert_with(|| {
                let m = slots.mapping_override(doc.object(object)?, part)?;
                Some(bounded_mapping(m, frame))
            });
            let mapping = forced.map_or(mappings[index as usize], |f| f.0);
            let shift = forced.map_or([0.0; 2], |f| f.1);
            for k in 0..3 {
                let v = mesh.indices[t * 3 + k] as usize;
                if look.vertex_material[v] != NO_MATERIAL {
                    continue;
                }
                look.vertex_material[v] = index;
                look.uvs[v] = vertex_uv(scene, &mapping, v, frame, shift);
            }
        }
        cache.finish();
        look.materials_key = look.materials.iter().fold(7, |h, m| {
            let mut h = hash_bytes(h, &m.color);
            for f in [m.roughness, m.metallic, m.opacity] {
                h = hash_bytes(h, &f.to_bits().to_le_bytes());
            }
            hash_bytes(h, &m.texture.map_or(0, |t| texture_hashes[t]).to_le_bytes())
        });
        look.layout_key = {
            let mut h = hash_bytes(11, &look.materials_key.to_le_bytes());
            for m in &look.tri_material {
                h = hash_bytes(h, &m.to_le_bytes());
            }
            for m in &look.vertex_material {
                h = hash_bytes(h, &m.to_le_bytes());
            }
            for uv in &look.uvs {
                h = hash_bytes(h, &uv[0].to_bits().to_le_bytes());
                h = hash_bytes(h, &uv[1].to_bits().to_le_bytes());
            }
            h
        };
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

/// The tile size that a renderer uses for the terrain texture of a site model in this scene:
/// the tile of the settings, or a larger one on a very big terrain.
pub fn terrain_tile(scene: &Scene, object: ObjectId, tile: f64) -> f64 {
    effective_tile(tile, 2.0 * frame_of(scene, object).1)
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

/// The mapping of a terrain texture, made safe for a big terrain, and the whole repeats at the
/// middle of the object. The tile grows when the terrain would hold more than
/// [`MAX_TEXTURE_REPEATS`] of them. Taking the repeats at the middle away changes no pixel (a
/// texture repeats), and keeps the coordinates small, so single precision is enough.
fn bounded_mapping(mut m: TextureMapping, frame: ([f64; 3], f64)) -> (TextureMapping, [f64; 2]) {
    for s in &mut m.scale {
        *s = effective_tile(*s, 2.0 * frame.1);
    }
    let mid = present::texture_uv_f64(&m, frame.0, [0.0, 0.0, 1.0], frame.0, frame.1);
    (m, [mid[0].floor(), mid[1].floor()])
}

fn vertex_uv(
    scene: &Scene,
    m: &TextureMapping,
    v: usize,
    frame: ([f64; 3], f64),
    shift: [f64; 2],
) -> [f32; 2] {
    let p = scene.mesh.positions[v];
    let o = scene.origin;
    let world = [o.x + p.x as f64, o.y + p.y as f64, o.z + p.z as f64];
    let uv = present::texture_uv_f64(m, world, scene.mesh.normals[v].to_array(), frame.0, frame.1);
    [(uv[0] - shift[0]) as f32, (uv[1] - shift[1]) as f32]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;
    use glam::DVec2;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::model::Shape;
    use kadakas_core::present::{
        MappingKind, Material, Procedural, RenderResource, SlotTarget, Texture,
    };
    use kadakas_core::resource::ResourceBody;

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

    #[test]
    fn textures_are_decoded_once_and_keys_tell_what_changed() {
        let mut ed = Editor::default();
        let a = add(&mut ed, square(500.0), None);
        let grass = add_material(
            &mut ed,
            Material {
                texture: Some(Texture::Procedural {
                    kind: Procedural::Grass,
                    seed: 1,
                }),
                ..Default::default()
            },
        );
        assign(&mut ed, a, None, grass);
        let mut cache = TextureCache::new();
        assert!(cache.is_empty());
        let scene = scene_of(&ed);
        let first = Look::build_cached(ed.doc(), &scene, true, &mut cache);
        assert_eq!(cache.len(), 1);
        // An edit that has nothing to do with materials: the same texture object, the same keys.
        let b = add(
            &mut ed,
            Shape::Rect {
                min: DVec2::new(5000.0, 0.0),
                max: DVec2::new(6000.0, 500.0),
            },
            None,
        );
        let _ = b;
        let scene2 = scene_of(&ed);
        let second = Look::build_cached(ed.doc(), &scene2, true, &mut cache);
        assert!(Arc::ptr_eq(&first.textures[0], &second.textures[0]));
        assert_eq!(first.materials_key, second.materials_key);
        assert_ne!(scene_key(&scene), scene_key(&scene2));
        assert_ne!(first.layout_key, second.layout_key);
        assert_eq!(scene_key(&scene), scene_key(&scene_of(&ed_without(&scene))));
        // Another opacity changes the material key. Another texture seed decodes a new texture and
        // drops the old one.
        ed.execute(&Command::ModifyResource {
            id: grass,
            name: None,
            body: Some(ResourceBody::Render(Box::new(RenderResource::Material(
                Material {
                    opacity: 0.5,
                    texture: Some(Texture::Procedural {
                        kind: Procedural::Grass,
                        seed: 2,
                    }),
                    ..Default::default()
                },
            )))),
        })
        .unwrap();
        let third = Look::build_cached(ed.doc(), &scene2, true, &mut cache);
        assert_ne!(second.materials_key, third.materials_key);
        assert!(!Arc::ptr_eq(&second.textures[0], &third.textures[0]));
        assert_eq!(cache.len(), 1);
        // Without textures nothing is kept.
        let _ = Look::build_cached(ed.doc(), &scene2, false, &mut cache);
        assert!(cache.is_empty());
        // A plain look has zero keys.
        let plain = Look::plain(&scene);
        assert_eq!((plain.materials_key, plain.layout_key), (0, 0));
    }

    // --- Terrain texture (3D-05) ---

    /// A site model that is drawn as a flat grid of `n` by `n` cells of `cell` mm at (x0, y0),
    /// by a stand-in for the extension of the site crate.
    fn terrain_scene(
        ed: &mut Editor,
        x0: f64,
        cell: f64,
        n: usize,
        settings: kadakas_core::site::SiteSettings,
    ) -> (ObjectId, Scene) {
        terrain_scene_with(ed, x0, cell, n, settings, |_, _| {})
    }

    /// Like `terrain_scene`, with a step between the site model and its scene.
    fn terrain_scene_with(
        ed: &mut Editor,
        x0: f64,
        cell: f64,
        n: usize,
        settings: kadakas_core::site::SiteSettings,
        prep: impl FnOnce(&mut Editor, ObjectId),
    ) -> (ObjectId, Scene) {
        let id = add(
            ed,
            Shape::SiteModel {
                points: vec![],
                lines: vec![],
                settings,
            },
            None,
        );
        let mut pos = Vec::new();
        let mut idx = Vec::new();
        for j in 0..=n {
            for i in 0..=n {
                pos.push(glam::DVec3::new(
                    x0 + i as f64 * cell,
                    j as f64 * cell,
                    (i + j) as f64 * 10.0,
                ));
            }
        }
        for j in 0..n {
            for i in 0..n {
                let a = (j * (n + 1) + i) as u32;
                let b = a + 1;
                let c = a + (n + 1) as u32;
                idx.extend([a, b, c + 1, a, c + 1, c]);
            }
        }
        let ext = move |_: &Document,
                        obj: &kadakas_core::model::Object,
                        sink: &mut kadakas_core::mesh::SceneSink<'_>| {
            if matches!(obj.shape, Shape::SiteModel { .. }) {
                sink.mesh(&pos, &idx, [10, 20, 30, 255]);
            }
        };
        prep(ed, id);
        let scene = kadakas_core::mesh::build_with(ed.doc(), glam::DVec3::ZERO, Some(&ext));
        (id, scene)
    }

    fn grass() -> Material {
        Material {
            texture: Some(Texture::Procedural {
                kind: Procedural::Grass,
                seed: 1,
            }),
            ..Default::default()
        }
    }

    fn texture_settings(m: ResourceId) -> kadakas_core::site::SiteSettings {
        kadakas_core::site::SiteSettings {
            surface_look: kadakas_core::site::SurfaceLook::Texture,
            texture: Some(m),
            texture_tile: 1000.0,
            texture_offset: [500.0, 0.0],
            ..Default::default()
        }
    }

    #[test]
    fn a_terrain_texture_maps_from_above_in_world_units() {
        let mut ed = Editor::default();
        let m = add_material(&mut ed, grass());
        let (_, scene) = terrain_scene(&mut ed, 0.0, 1000.0, 4, texture_settings(m));
        let look = Look::build(ed.doc(), &scene, true);
        assert_eq!(look.materials.len(), 1);
        assert!(look.materials[0].texture.is_some());
        assert!(look.tri_material.iter().all(|i| *i == 0));
        // Neighbors 1000 mm apart differ by exactly one repeat; the offset shifts U by half.
        let u = |x: usize| look.uvs[x][0];
        for i in 0..4 {
            assert!((u(i + 1) - u(i) - 1.0).abs() < 1e-5);
        }
        assert!(((u(0) - u(0).floor()) - 0.5).abs() < 1e-5, "{}", u(0));
        let v = |j: usize| look.uvs[j * 5][1];
        assert!((v(1) - v(0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_terrain_texture_turns() {
        let mut ed = Editor::default();
        let m = add_material(&mut ed, grass());
        let mut s = texture_settings(m);
        s.texture_offset = [0.0, 0.0];
        s.texture_rotation = 90.0;
        let (_, scene) = terrain_scene(&mut ed, 0.0, 1000.0, 2, s);
        let look = Look::build(ed.doc(), &scene, true);
        // A step along +X is now a step along -V.
        assert!((look.uvs[1][1] - look.uvs[0][1] + 1.0).abs() < 1e-5);
        assert!((look.uvs[1][0] - look.uvs[0][0]).abs() < 1e-5);
    }

    #[test]
    fn terrain_material_comes_from_the_slot_when_no_texture_is_set() {
        let mut ed = Editor::default();
        let m = add_material(&mut ed, grass());
        let mut s = texture_settings(m);
        s.texture = None;
        let (_, scene) =
            terrain_scene_with(&mut ed, 0.0, 1000.0, 2, s, |ed, id| assign(ed, id, None, m));
        let look = Look::build(ed.doc(), &scene, true);
        assert!(!look.is_plain());
        // The mapping of the site model is used, not the box mapping of the material.
        assert!((look.uvs[1][0] - look.uvs[0][0] - 1.0).abs() < 1e-5);
        assert_eq!(ed.doc().objects.len(), 1);
    }

    #[test]
    fn the_elevation_look_takes_no_material_and_no_forced_mapping() {
        let mut ed = Editor::default();
        let m = add_material(&mut ed, grass());
        let mut s = texture_settings(m);
        s.surface_look = kadakas_core::site::SurfaceLook::Elevation;
        let (id, scene) =
            terrain_scene_with(&mut ed, 0.0, 1000.0, 2, s, |ed, id| assign(ed, id, None, m));
        // Elevation colors are vertex colors: the slot does not cover them.
        assert!(Look::build(ed.doc(), &scene, true).is_plain());
        let obj = ed.doc().object(id).unwrap();
        assert!(present::mapping_override(obj).is_none());
        assert!(SlotIndex::new(ed.doc()).resolve(obj, 0).is_none());
    }

    #[test]
    fn a_tiny_tile_on_a_big_terrain_stays_precise() {
        let mut ed = Editor::default();
        let m = add_material(&mut ed, grass());
        let mut s = texture_settings(m);
        s.texture_tile = 10.0;
        s.texture_offset = [0.0, 0.0];
        // 40 000 km wide, far from the origin: 4 million repeats without the bound.
        let (id, scene) = terrain_scene(&mut ed, 50.0e6, 10.0e6, 4, s);
        let look = Look::build(ed.doc(), &scene, true);
        let limit = kadakas_core::site::MAX_TEXTURE_REPEATS as f32;
        assert!(look.uvs.iter().all(|uv| uv[0].is_finite()
            && uv[1].is_finite()
            && uv[0].abs() <= limit
            && uv[1].abs() <= limit));
        let again = Look::build(ed.doc(), &scene, true);
        assert_eq!(look.layout_key, again.layout_key);
        // The tile that was used is reported.
        let used = terrain_tile(&scene, id, 10.0);
        assert!((used - 4.0 * 10.0e6 / kadakas_core::site::MAX_TEXTURE_REPEATS).abs() < 1e-6);
        assert_eq!(terrain_tile(&scene, id, 1.0e9), 1.0e9);
    }

    /// A thin strip of cells, one tile wide, that is exactly `MAX_TEXTURE_REPEATS` tiles long
    /// (the limit where the tile is not made larger yet). Neighbors one tile apart must differ by
    /// one repeat, up to the precision of single numbers.
    #[test]
    fn neighbors_one_tile_apart_differ_by_one_repeat_at_the_limit() {
        let tile = 1000.0;
        let n = kadakas_core::site::MAX_TEXTURE_REPEATS as usize;
        for x0 in [0.0, 123_456_789.5, -4.0e8] {
            let mut ed = Editor::default();
            let m = add_material(&mut ed, grass());
            let mut s = texture_settings(m);
            s.texture_tile = tile;
            s.texture_offset = [250.0, 0.0];
            s.texture_rotation = 0.0;
            let id = add(
                &mut ed,
                Shape::SiteModel {
                    points: vec![],
                    lines: vec![],
                    settings: s,
                },
                None,
            );
            let mut pos = Vec::new();
            let mut idx = Vec::new();
            for i in 0..=n {
                pos.push(glam::DVec3::new(x0 + i as f64 * tile, 0.0, 0.0));
                pos.push(glam::DVec3::new(x0 + i as f64 * tile, tile, 0.0));
            }
            for i in 0..n as u32 {
                let a = 2 * i;
                idx.extend([a, a + 2, a + 3, a, a + 3, a + 1]);
            }
            let ext = move |_: &Document,
                            obj: &kadakas_core::model::Object,
                            sink: &mut kadakas_core::mesh::SceneSink<'_>| {
                if matches!(obj.shape, Shape::SiteModel { .. }) {
                    sink.mesh(&pos, &idx, [1, 2, 3, 255]);
                }
            };
            // The scene origin is in the middle of the strip, as in a real scene.
            let origin = glam::DVec3::new(x0 + 0.5 * n as f64 * tile, 0.0, 0.0);
            let scene = kadakas_core::mesh::build_with(ed.doc(), origin, Some(&ext));
            assert!((terrain_tile(&scene, id, tile) - tile).abs() < 1e-3);
            let look = Look::build(ed.doc(), &scene, true);
            for i in 0..n {
                let du = look.uvs[2 * (i + 1)][0] - look.uvs[2 * i][0];
                assert!((du - 1.0).abs() < 1e-3, "x0 {x0}, cell {i}: {du}");
            }
        }
    }

    proptest::proptest! {
        #[test]
        fn terrain_uvs_are_finite_and_deterministic(
            tile in 10.0f64..1.0e7,
            ox in -1.0e9f64..1.0e9,
            oy in -1.0e9f64..1.0e9,
            rot in -720.0f64..720.0,
            x0 in -1.0e8f64..1.0e8,
            cell in 1.0f64..1.0e6,
        ) {
            let mut ed = Editor::default();
            let m = add_material(&mut ed, grass());
            let mut s = texture_settings(m);
            s.texture_tile = tile;
            s.texture_offset = [ox, oy];
            s.texture_rotation = rot;
            let (_, scene) = terrain_scene(&mut ed, x0, cell, 3, s);
            let a = Look::build(ed.doc(), &scene, true);
            let b = Look::build(ed.doc(), &scene, true);
            proptest::prop_assert_eq!(&a.uvs, &b.uvs);
            proptest::prop_assert!(a.uvs.iter().all(|uv| uv[0].is_finite() && uv[1].is_finite()));
        }
    }

    /// A site model with a texture bed. The stand-in extension draws the terrain as two parts:
    /// the base and the bed (a square at (1000, 1000) to (3000, 3000)).
    fn bed_scene(
        ed: &mut Editor,
        site: kadakas_core::site::SiteSettings,
        bed: kadakas_core::site::ModifierKind,
    ) -> (ObjectId, Scene) {
        use kadakas_core::present::{PART_BED, PART_MAIN};
        let id = add(
            ed,
            Shape::SiteModel {
                points: vec![
                    glam::DVec3::ZERO,
                    glam::DVec3::new(4000.0, 0.0, 0.0),
                    glam::DVec3::new(0.0, 4000.0, 0.0),
                ],
                lines: vec![],
                settings: site,
            },
            None,
        );
        add(
            ed,
            Shape::SiteModifier {
                modifier: bed,
                boundary: kadakas_core::curve::corners(&[
                    DVec2::new(1000.0, 1000.0),
                    DVec2::new(3000.0, 1000.0),
                    DVec2::new(3000.0, 3000.0),
                    DVec2::new(1000.0, 3000.0),
                ]),
            },
            None,
        );
        let quad = |x0: f64, x1: f64| {
            vec![
                glam::DVec3::new(x0, 0.0, 0.0),
                glam::DVec3::new(x1, 0.0, 0.0),
                glam::DVec3::new(x1, 4000.0, 0.0),
                glam::DVec3::new(x0, 4000.0, 0.0),
            ]
        };
        let ext = move |_: &Document,
                        obj: &kadakas_core::model::Object,
                        sink: &mut kadakas_core::mesh::SceneSink<'_>| {
            if matches!(obj.shape, Shape::SiteModel { .. }) {
                sink.mesh(&quad(0.0, 1000.0), &[0, 1, 2, 0, 2, 3], [10, 20, 30, 255]);
                sink.set_part(PART_BED);
                sink.mesh(&quad(1000.0, 3000.0), &[0, 1, 2, 0, 2, 3], [10, 20, 30, 255]);
                sink.set_part(PART_MAIN);
            }
        };
        let scene = kadakas_core::mesh::build_with(ed.doc(), glam::DVec3::ZERO, Some(&ext));
        (id, scene)
    }

    #[test]
    fn a_bed_part_takes_the_material_and_the_mapping_of_its_bed() {
        use kadakas_core::site::{ModifierKind, SurfaceLook};
        use kadakas_core::texture_bed::BedMapping;
        let mut ed = Editor::default();
        let m = add_material(&mut ed, grass());
        let own = ModifierKind::TextureBed {
            texture: Some(m),
            mapping: Some(BedMapping {
                tile: 250.0,
                ..Default::default()
            }),
        };
        let site = kadakas_core::site::SiteSettings {
            texture_tile: 1000.0,
            ..Default::default()
        };
        let (_, scene) = bed_scene(&mut ed, site.clone(), own);
        let look = Look::build(ed.doc(), &scene, true);
        // The base triangles keep their color; the bed triangles have the material.
        let base: Vec<u32> = (0..2).map(|t| look.tri_material[t]).collect();
        let bed: Vec<u32> = (2..4).map(|t| look.tri_material[t]).collect();
        assert_eq!(base, vec![NO_MATERIAL; 2]);
        assert!(bed.iter().all(|i| *i != NO_MATERIAL));
        assert_eq!(look.materials.len(), 1);
        assert!(look.materials[0].texture.is_some());
        // Corners 0 and 1 of the bed quad (vertices 4 and 5) are 2000 mm apart: 8 repeats of 250 mm.
        let du = look.uvs[5][0] - look.uvs[4][0];
        assert!((du - 8.0).abs() < 1e-4, "{du}");
        // Without its own mapping the bed uses the tile of the site model: 2 repeats.
        let mut ed2 = Editor::default();
        let m2 = add_material(&mut ed2, grass());
        let shared = ModifierKind::TextureBed {
            texture: Some(m2),
            mapping: None,
        };
        let (_, scene2) = bed_scene(&mut ed2, site.clone(), shared);
        let look2 = Look::build(ed2.doc(), &scene2, true);
        let du = look2.uvs[5][0] - look2.uvs[4][0];
        assert!((du - 2.0).abs() < 1e-4, "{du}");
        // The elevation look has vertex colors outside, and the bed still has its texture.
        let mut ed3 = Editor::default();
        let m3 = add_material(&mut ed3, grass());
        let elev = kadakas_core::site::SiteSettings {
            surface_look: SurfaceLook::Elevation,
            ..site
        };
        let (_, scene3) = bed_scene(
            &mut ed3,
            elev,
            ModifierKind::TextureBed {
                texture: Some(m3),
                mapping: None,
            },
        );
        let look3 = Look::build(ed3.doc(), &scene3, true);
        assert_eq!(look3.tri_material[0], NO_MATERIAL);
        assert_ne!(look3.tri_material[2], NO_MATERIAL);
        // The layout key tells the bed apart from no bed.
        assert_ne!(look.layout_key, look3.layout_key);
        // With textures off the material gives a color and no texture.
        let flat = Look::build(ed.doc(), &scene, false);
        assert!(flat.materials[0].texture.is_none());
    }

    /// An empty editor: a stand-in so that `scene_key` of equal content can be compared.
    fn ed_without(scene: &Scene) -> Editor {
        let _ = scene;
        let mut ed = Editor::default();
        add(&mut ed, square(500.0), None);
        ed
    }
}
