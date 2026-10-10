//! From the 3D scene to parts: the common input of the glTF and OBJ writers.
//!
//! A part is one node (glTF) or one group (OBJ). It holds primitives, one for each material,
//! with positions in the axes and unit of the file, relative to the anchor of the part. The
//! anchor is the corner of the box of the part. A node puts the anchor back with its translation,
//! so a mesh keeps small numbers and two equal shapes at different places have equal meshes.

use std::collections::{BTreeMap, HashMap};

use glam::{DVec3, Vec3};
use serde_json::{Value, json};

use cadektor_core::mesh::Scene;
use cadektor_core::model::{Document, Object, ObjectId, Shape};

use crate::X3dError;
use crate::material::{Material, MaterialSource};
use crate::options::{ExportOptions, Grouping};

/// The change from model coordinates to file coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Conv {
    pub y_up: bool,
    pub k: f64,
}

impl Conv {
    pub fn new(opts: &ExportOptions) -> Conv {
        Conv {
            y_up: opts.y_up,
            k: opts.units.factor(),
        }
    }

    /// A point or a vector of the model, in the file axes and unit.
    pub fn point(&self, p: DVec3) -> [f64; 3] {
        let p = p * self.k;
        if self.y_up {
            [p.x, p.z, -p.y]
        } else {
            [p.x, p.y, p.z]
        }
    }

    /// A direction (a normal): the axes change, the length does not.
    pub fn dir(&self, n: Vec3) -> [f32; 3] {
        if self.y_up {
            [n.x, n.z, -n.y]
        } else {
            [n.x, n.y, n.z]
        }
    }
}

/// The triangles or lines of one material.
#[derive(Clone, Debug, PartialEq)]
pub struct Prim {
    /// Index into `Model::materials`.
    pub material: usize,
    /// True for line segments (two indices each), false for triangles.
    pub lines: bool,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Empty unless the material has a texture.
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// One node of the output.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub name: String,
    /// Goes to `extras` of the node.
    pub extras: Value,
    /// The objects in this part.
    pub ids: Vec<u64>,
    /// Where the origin of the primitives is, in file axes and unit.
    pub anchor: [f64; 3],
    pub prims: Vec<Prim>,
    /// Equal for parts with the same shape. Zero when instancing is off.
    pub signature: u64,
}

/// Everything the writers need.
#[derive(Clone, Debug)]
pub struct Model {
    pub parts: Vec<Part>,
    pub materials: Vec<Material>,
    pub conv: Conv,
    /// The scene origin: the model position (mm) of the local point (0, 0, 0).
    pub origin: DVec3,
    /// Objects that were left out, with the reason.
    pub skipped: Vec<(u64, String)>,
}

/// A name for a node: the kind, the ID and the species of a plant.
pub fn object_name(obj: &Object) -> String {
    match &obj.shape {
        Shape::Plant { species, .. } | Shape::ExistingTree { species, .. }
            if !species.is_empty() =>
        {
            format!("{} {} ({})", obj.shape.kind(), obj.id, species)
        }
        s => format!("{} {}", s.kind(), obj.id),
    }
}

fn object_extras(doc: &Document, obj: &Object, opts: &ExportOptions) -> Value {
    let layer = doc.layer(obj.layer).map(|l| l.name.as_str()).unwrap_or("");
    let class = doc.class(obj.class).map(|c| c.name.as_str()).unwrap_or("");
    let mut x = json!({
        "id": obj.id.0,
        "kind": obj.shape.kind(),
        "name": object_name(obj),
        "layer": layer,
        "layer_id": obj.layer.0,
        "class": class,
        "class_id": obj.class.0,
    });
    if opts.data && !obj.data.is_empty() {
        x["data"] = json!(obj.data);
    }
    x
}

/// What belongs to one object in the scene.
#[derive(Default)]
struct Geometry {
    tris: Vec<usize>,
    segs: Vec<usize>,
}

/// FNV-1a over 64 bit words.
fn mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x0000_0100_0000_01b3)
}

/// The prims of a set of triangles and segments of the scene. Positions are relative to
/// `anchor` (model mm).
struct Builder<'a> {
    doc: &'a Document,
    scene: &'a Scene,
    conv: Conv,
    src: &'a dyn MaterialSource,
    materials: Vec<Material>,
}

impl Builder<'_> {
    fn material_index(&mut self, obj: &Object, tri: Option<usize>, color: [u8; 4]) -> usize {
        let m = match tri {
            Some(t) => self.src.material_for(self.doc, obj, t, color),
            None => self.src.material(self.doc, obj, color),
        };
        match self.materials.iter().position(|x| *x == m) {
            Some(i) => i,
            None => {
                self.materials.push(m);
                self.materials.len() - 1
            }
        }
    }

    /// The primitives of one object. `anchor` is a local scene position in mm.
    fn prims(
        &mut self,
        obj: &Object,
        g: &Geometry,
        anchor: Vec3,
        lines: bool,
    ) -> Vec<(Prim, [u8; 4])> {
        let scene = self.scene;
        let mesh = &scene.mesh;
        let mut out: Vec<(Prim, [u8; 4])> = Vec::new();
        let mut by_color: BTreeMap<(u32, [u8; 4]), usize> = BTreeMap::new();
        let mut remaps: Vec<HashMap<u32, u32>> = Vec::new();
        for t in &g.tris {
            let idx = &mesh.indices[t * 3..t * 3 + 3];
            let color = mesh.colors[idx[0] as usize];
            let slot = *by_color
                .entry((self.src.group(*t), color))
                .or_insert_with(|| {
                    let material = self.material_index(obj, Some(*t), color);
                    out.push((
                        Prim {
                            material,
                            lines: false,
                            positions: Vec::new(),
                            normals: Vec::new(),
                            uvs: Vec::new(),
                            indices: Vec::new(),
                        },
                        color,
                    ));
                    remaps.push(HashMap::new());
                    out.len() - 1
                });
            let repeat = self.materials[out[slot].0.material]
                .texture
                .as_ref()
                .map(|t| t.repeat);
            for &i in idx {
                let prim = &mut out[slot].0;
                let next = prim.positions.len() as u32;
                let v = *remaps[slot].entry(i).or_insert(next);
                if v == next {
                    let p = mesh.positions[i as usize];
                    let n = mesh.normals[i as usize];
                    prim.positions.push(self.rel(p, anchor));
                    prim.normals.push(self.conv.dir(n));
                    if let Some(repeat) = repeat {
                        prim.uvs.push(
                            self.src
                                .uv(i as usize)
                                .unwrap_or_else(|| uv(p, n, repeat, scene.origin)),
                        );
                    }
                }
                prim.indices.push(v);
            }
        }
        if lines {
            let l = &scene.lines;
            let mut by_color: BTreeMap<[u8; 4], usize> = BTreeMap::new();
            for s in &g.segs {
                let color = l.colors[*s];
                let slot = *by_color.entry(color).or_insert_with(|| {
                    let material = self.material_index(obj, None, color);
                    out.push((
                        Prim {
                            material,
                            lines: true,
                            positions: Vec::new(),
                            normals: Vec::new(),
                            uvs: Vec::new(),
                            indices: Vec::new(),
                        },
                        color,
                    ));
                    out.len() - 1
                });
                let prim = &mut out[slot].0;
                for p in [l.positions[2 * s], l.positions[2 * s + 1]] {
                    prim.indices.push(prim.positions.len() as u32);
                    prim.positions.push(self.rel(p, anchor));
                }
            }
        }
        out
    }

    fn rel(&self, p: Vec3, anchor: Vec3) -> [f32; 3] {
        let c = self.conv.point((p - anchor).as_dvec3());
        [c[0] as f32, c[1] as f32, c[2] as f32]
    }
}

/// Texture coordinates by box projection: the plane that faces the normal most.
fn uv(p: Vec3, n: Vec3, repeat: f64, origin: DVec3) -> [f32; 2] {
    let w = (p.as_dvec3() + origin) * 0.001;
    let (a, b) = if n.z.abs() >= n.x.abs() && n.z.abs() >= n.y.abs() {
        (w.x, w.y)
    } else if n.x.abs() >= n.y.abs() {
        (w.y, w.z)
    } else {
        (w.x, w.z)
    };
    let r = if repeat.is_finite() && repeat > 0.0 {
        repeat
    } else {
        1.0
    };
    [(a / r) as f32, (b / r) as f32]
}

fn bounds_of(scene: &Scene, g: &Geometry, lines: bool) -> Option<(Vec3, Vec3)> {
    let mut acc: Option<(Vec3, Vec3)> = None;
    let mut add = |p: Vec3| {
        acc = Some(match acc {
            None => (p, p),
            Some((lo, hi)) => (lo.min(p), hi.max(p)),
        });
    };
    for t in &g.tris {
        for i in &scene.mesh.indices[t * 3..t * 3 + 3] {
            add(scene.mesh.positions[*i as usize]);
        }
    }
    if lines {
        for s in &g.segs {
            add(scene.lines.positions[2 * s]);
            add(scene.lines.positions[2 * s + 1]);
        }
    }
    acc
}

/// Makes the parts of the scene.
pub fn build(
    doc: &Document,
    scene: &Scene,
    opts: &ExportOptions,
    src: &dyn MaterialSource,
) -> Result<Model, X3dError> {
    opts.validate().map_err(X3dError::Options)?;
    let conv = Conv::new(opts);
    // Triangles and segments by object, in the order of the scene.
    let mut order: Vec<ObjectId> = Vec::new();
    let mut geo: BTreeMap<u64, Geometry> = BTreeMap::new();
    let wanted = |id: ObjectId| opts.objects.as_ref().is_none_or(|list| list.contains(&id));
    for (t, id) in scene.mesh.tri_object.iter().enumerate() {
        if !wanted(*id) {
            continue;
        }
        geo.entry(id.0)
            .or_insert_with(|| {
                order.push(*id);
                Geometry::default()
            })
            .tris
            .push(t);
    }
    if opts.lines {
        for (s, id) in scene.lines.objects.iter().enumerate() {
            // Objects with triangles are solids: their edges would only clutter the model.
            if !wanted(*id) || geo.get(&id.0).is_some_and(|g| !g.tris.is_empty()) {
                continue;
            }
            geo.entry(id.0)
                .or_insert_with(|| {
                    order.push(*id);
                    Geometry::default()
                })
                .segs
                .push(s);
        }
    }
    let mut b = Builder {
        doc,
        scene,
        conv,
        src,
        materials: Vec::new(),
    };
    let mut skipped = Vec::new();
    let mut parts: Vec<Part> = Vec::new();
    let mut group_index: HashMap<u64, usize> = HashMap::new();
    // Parts of a group are joined at the end: collect (object, geometry) per group first.
    let mut groups: Vec<(u64, String, Vec<ObjectId>)> = Vec::new();
    for id in &order {
        let Some(obj) = doc.object(*id) else {
            skipped.push((id.0, "the object does not exist".to_string()));
            continue;
        };
        let g = &geo[&id.0];
        if bounds_of(scene, g, true).is_none() {
            continue;
        }
        let finite = g.tris.iter().all(|t| {
            scene.mesh.indices[t * 3..t * 3 + 3]
                .iter()
                .all(|i| scene.mesh.positions[*i as usize].is_finite())
        });
        if !finite {
            skipped.push((
                id.0,
                "the object has coordinates that are not finite".to_string(),
            ));
            continue;
        }
        match opts.grouping {
            Grouping::Object => {
                let anchor = bounds_of(scene, g, true).map_or(Vec3::ZERO, |b| b.0);
                let prims = b.prims(obj, g, anchor, true);
                parts.push(finish_part(
                    object_name(obj),
                    object_extras(doc, obj, opts),
                    vec![id.0],
                    anchor,
                    prims,
                    conv,
                    opts,
                ));
            }
            Grouping::Layer | Grouping::Class => {
                let (key, name) = if opts.grouping == Grouping::Layer {
                    (obj.layer.0, doc.layer(obj.layer).map(|l| l.name.clone()))
                } else {
                    (obj.class.0, doc.class(obj.class).map(|c| c.name.clone()))
                };
                let slot = *group_index.entry(key).or_insert_with(|| {
                    groups.push((
                        key,
                        name.unwrap_or_else(|| format!("group {key}")),
                        Vec::new(),
                    ));
                    groups.len() - 1
                });
                groups[slot].2.push(*id);
            }
        }
    }
    for (key, name, ids) in groups {
        // The anchor is the corner of the box of all objects in the group.
        let mut acc: Option<(Vec3, Vec3)> = None;
        for id in &ids {
            if let Some((lo, hi)) = bounds_of(scene, &geo[&id.0], true) {
                acc = Some(match acc {
                    None => (lo, hi),
                    Some((a, c)) => (a.min(lo), c.max(hi)),
                });
            }
        }
        let anchor = acc.map_or(Vec3::ZERO, |b| b.0);
        let mut joined: Vec<(Prim, [u8; 4])> = Vec::new();
        for id in &ids {
            let obj = doc.object(*id).expect("checked above");
            for (p, color) in b.prims(obj, &geo[&id.0], anchor, true) {
                match joined
                    .iter_mut()
                    .find(|(q, c)| q.material == p.material && q.lines == p.lines && *c == color)
                {
                    Some((q, _)) => append(q, &p),
                    None => joined.push((p, color)),
                }
            }
        }
        let extras = json!({
            "group": if opts.grouping == Grouping::Layer { "layer" } else { "class" },
            "id": key,
            "name": name,
            "object_ids": ids.iter().map(|i| i.0).collect::<Vec<_>>(),
        });
        parts.push(finish_part(
            name,
            extras,
            ids.iter().map(|i| i.0).collect(),
            anchor,
            joined,
            conv,
            opts,
        ));
    }
    if parts.is_empty() {
        return Err(X3dError::Empty);
    }
    Ok(Model {
        parts,
        materials: b.materials,
        conv,
        origin: scene.origin,
        skipped,
    })
}

/// Adds the vertices and indices of `p` to `q`.
fn append(q: &mut Prim, p: &Prim) {
    let base = q.positions.len() as u32;
    q.positions.extend_from_slice(&p.positions);
    q.normals.extend_from_slice(&p.normals);
    q.uvs.extend_from_slice(&p.uvs);
    q.indices.extend(p.indices.iter().map(|i| i + base));
}

#[allow(clippy::too_many_arguments)]
fn finish_part(
    name: String,
    extras: Value,
    ids: Vec<u64>,
    anchor: Vec3,
    prims: Vec<(Prim, [u8; 4])>,
    conv: Conv,
    opts: &ExportOptions,
) -> Part {
    let signature = if opts.instancing && opts.grouping == Grouping::Object {
        signature(&prims, opts.instance_tolerance * opts.units.factor())
    } else {
        0
    };
    Part {
        name,
        extras,
        ids,
        anchor: conv.point(anchor.as_dvec3()),
        prims: prims.into_iter().map(|(p, _)| p).collect(),
        signature,
    }
}

/// A hash of the shape of a part: positions to the tolerance, normals, indices and colors.
/// `tolerance` is in file units.
fn signature(prims: &[(Prim, [u8; 4])], tolerance: f64) -> u64 {
    let q = 1.0 / tolerance.max(1e-9);
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for (p, color) in prims {
        h = mix(h, p.lines as u64);
        h = mix(h, u32::from_le_bytes(*color) as u64);
        h = mix(h, p.material as u64);
        h = mix(h, p.positions.len() as u64);
        for v in &p.positions {
            for c in v {
                h = mix(h, (*c as f64 * q).round() as i64 as u64);
            }
        }
        for n in &p.normals {
            for c in n {
                h = mix(h, (*c * 100.0).round() as i32 as u32 as u64);
            }
        }
        for i in &p.indices {
            h = mix(h, *i as u64);
        }
        // Equal shapes with different texture coordinates are different meshes.
        for uv in &p.uvs {
            for c in uv {
                h = mix(h, c.to_bits() as u64);
            }
        }
    }
    // Zero means "no instancing", so a real hash never is zero.
    h | 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_turns_z_up_into_y_up() {
        let c = Conv {
            y_up: true,
            k: 0.001,
        };
        assert_eq!(
            c.point(DVec3::new(1000.0, 2000.0, 3000.0)),
            [1.0, 3.0, -2.0]
        );
        assert_eq!(c.dir(Vec3::new(0.0, 0.0, 1.0)), [0.0, 1.0, -0.0]);
        let z = Conv {
            y_up: false,
            k: 1.0,
        };
        assert_eq!(z.point(DVec3::new(1.0, 2.0, 3.0)), [1.0, 2.0, 3.0]);
        assert_eq!(z.dir(Vec3::Y), [0.0, 1.0, 0.0]);
    }

    #[test]
    fn textures_project_on_the_dominant_plane() {
        let o = DVec3::ZERO;
        assert_eq!(
            uv(Vec3::new(2000.0, 4000.0, 0.0), Vec3::Z, 2.0, o),
            [1.0, 2.0]
        );
        assert_eq!(
            uv(Vec3::new(0.0, 2000.0, 4000.0), Vec3::X, 2.0, o),
            [1.0, 2.0]
        );
        assert_eq!(
            uv(Vec3::new(2000.0, 0.0, 4000.0), Vec3::Y, 2.0, o),
            [1.0, 2.0]
        );
        // A bad repeat falls back to one metre.
        assert_eq!(
            uv(Vec3::new(1000.0, 1000.0, 0.0), Vec3::Z, 0.0, o),
            [1.0, 1.0]
        );
    }
}
