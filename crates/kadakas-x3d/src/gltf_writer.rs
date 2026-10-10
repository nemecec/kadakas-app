//! The glTF 2.0 writer: `.gltf` with an embedded buffer, and `.glb`.
//!
//! Every part becomes a node with a mesh. Parts with the same signature share one mesh (see
//! `parts`), which is how plants and symbols are instanced. The materials are PBR metallic
//! roughness. Nodes carry `extras` with the object ID, the kind, the layer, the class and the
//! data of the object.

use std::collections::HashMap;

use base64::Engine;
use serde_json::{Value, json};

use kadakas_core::model::Document;

use crate::options::ExportOptions;
use crate::parts::{Model, Prim};
use crate::{ExportStats, X3dError};

const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;
const MODE_LINES: u32 = 1;

/// The name of the extension that holds the place of the model on the earth.
pub const GEOREF_EXTENSION: &str = "KADAKAS_georeference";

struct Buffer {
    bytes: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Buffer {
    fn view(&mut self, data: &[u8], target: Option<u32>) -> usize {
        while !self.bytes.len().is_multiple_of(4) {
            self.bytes.push(0);
        }
        let mut v = json!({"buffer": 0, "byteOffset": self.bytes.len(), "byteLength": data.len()});
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        self.bytes.extend_from_slice(data);
        self.views.push(v);
        self.views.len() - 1
    }

    fn accessor(&mut self, view: usize, component: u32, count: usize, kind: &str) -> usize {
        self.accessors.push(json!({
            "bufferView": view, "componentType": component, "count": count, "type": kind,
        }));
        self.accessors.len() - 1
    }

    fn floats(&mut self, values: &[f32], count: usize, kind: &str, minmax: bool) -> usize {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(ARRAY_BUFFER));
        let a = self.accessor(view, FLOAT, count, kind);
        if minmax {
            let width = values.len() / count.max(1);
            let (mut lo, mut hi) = (vec![f32::INFINITY; width], vec![f32::NEG_INFINITY; width]);
            for c in values.chunks_exact(width) {
                for k in 0..width {
                    lo[k] = lo[k].min(c[k]);
                    hi[k] = hi[k].max(c[k]);
                }
            }
            self.accessors[a]["min"] = json!(lo);
            self.accessors[a]["max"] = json!(hi);
        }
        a
    }

    fn indices(&mut self, idx: &[u32], vertices: usize) -> usize {
        let (bytes, component): (Vec<u8>, u32) = if vertices <= 65_535 {
            (
                idx.iter().flat_map(|i| (*i as u16).to_le_bytes()).collect(),
                UNSIGNED_SHORT,
            )
        } else {
            (
                idx.iter().flat_map(|i| i.to_le_bytes()).collect(),
                UNSIGNED_INT,
            )
        };
        let view = self.view(&bytes, Some(ELEMENT_ARRAY_BUFFER));
        self.accessor(view, component, idx.len(), "SCALAR")
    }
}

fn primitive(b: &mut Buffer, p: &Prim) -> Value {
    let flat = |v: &[[f32; 3]]| -> Vec<f32> { v.iter().flatten().copied().collect() };
    let mut attrs = json!({
        "POSITION": b.floats(&flat(&p.positions), p.positions.len(), "VEC3", true),
    });
    if !p.lines {
        attrs["NORMAL"] = json!(b.floats(&flat(&p.normals), p.normals.len(), "VEC3", false));
    }
    if !p.uvs.is_empty() {
        let uv: Vec<f32> = p.uvs.iter().flatten().copied().collect();
        attrs["TEXCOORD_0"] = json!(b.floats(&uv, p.uvs.len(), "VEC2", false));
    }
    let mut v = json!({
        "attributes": attrs,
        "indices": b.indices(&p.indices, p.positions.len()),
        "material": p.material,
    });
    if p.lines {
        v["mode"] = json!(MODE_LINES);
    }
    v
}

fn georef(doc: &Document, model: &Model, opts: &ExportOptions) -> Value {
    let mut v = json!({
        "crs": "",
        "axes": if opts.y_up { "y_up" } else { "z_up" },
        "unit": opts.units.id(),
        "model_unit": "mm",
        // The model position of the local point (0, 0, 0) of the file, in the axes of the model.
        "local_origin": [model.origin.x, model.origin.y, model.origin.z],
    });
    if let Some(g) = &doc.georef {
        v["crs"] = json!(g.crs);
        v["map_origin"] = json!([g.origin.x, g.origin.y]);
        v["rotation"] = json!(g.rotation);
    }
    v
}

/// The JSON of the file and the binary buffer. With `embed`, the buffer is a data URI.
fn assemble(
    model: &Model,
    doc: &Document,
    opts: &ExportOptions,
    embed: bool,
) -> (Value, Vec<u8>, ExportStats) {
    let mut b = Buffer {
        bytes: Vec::new(),
        views: Vec::new(),
        accessors: Vec::new(),
    };
    // Textures: images and textures are shared between materials.
    let mut images: Vec<Value> = Vec::new();
    let mut textures: Vec<Value> = Vec::new();
    let mut tex_index: Vec<(usize, usize)> = Vec::new();
    let mut materials: Vec<Value> = Vec::new();
    for (mi, m) in model.materials.iter().enumerate() {
        let mut pbr = json!({
            "baseColorFactor": m.base_color,
            "metallicFactor": m.metallic,
            "roughnessFactor": m.roughness,
        });
        if let Some(t) = &m.texture {
            let existing = model.materials[..mi]
                .iter()
                .position(|o| o.texture.as_ref() == Some(t))
                .and_then(|j| tex_index.iter().find(|(k, _)| *k == j).map(|(_, x)| *x));
            let ti = existing.unwrap_or_else(|| {
                let view = b.view(&t.data, None);
                images.push(json!({"bufferView": view, "mimeType": t.mime, "name": t.name}));
                textures.push(json!({"sampler": 0, "source": images.len() - 1, "name": t.name}));
                textures.len() - 1
            });
            tex_index.push((mi, ti));
            pbr["baseColorTexture"] = json!({"index": ti});
        }
        let mut mat = json!({"name": m.name, "pbrMetallicRoughness": pbr});
        if m.is_transparent() {
            mat["alphaMode"] = json!("BLEND");
        }
        if m.double_sided {
            mat["doubleSided"] = json!(true);
        }
        if m.emissive != [0.0; 3] {
            mat["emissiveFactor"] = json!(m.emissive);
        }
        materials.push(mat);
    }

    let mut meshes: Vec<Value> = Vec::new();
    let mut shared: HashMap<u64, usize> = HashMap::new();
    let mut nodes: Vec<Value> = Vec::new();
    let mut stats = ExportStats::default();
    for part in &model.parts {
        let cached = if part.signature != 0 {
            shared.get(&part.signature).copied()
        } else {
            None
        };
        let mesh = match cached {
            Some(i) => {
                stats.instances += 1;
                i
            }
            None => {
                let prims: Vec<Value> = part.prims.iter().map(|p| primitive(&mut b, p)).collect();
                for p in &part.prims {
                    if p.lines {
                        stats.lines += p.indices.len() / 2;
                    } else {
                        stats.triangles += p.indices.len() / 3;
                    }
                }
                meshes.push(json!({"name": part.name, "primitives": prims}));
                let i = meshes.len() - 1;
                if part.signature != 0 {
                    shared.insert(part.signature, i);
                }
                i
            }
        };
        let mut node = json!({"name": part.name, "mesh": mesh, "extras": part.extras});
        if part.anchor != [0.0; 3] {
            node["translation"] = json!(part.anchor);
        }
        nodes.push(node);
        stats.objects += part.ids.len();
    }
    stats.nodes = nodes.len();
    stats.meshes = meshes.len();
    stats.materials = materials.len();
    stats.textures = textures.len();
    stats.skipped = model.skipped.clone();

    let node_ids: Vec<usize> = (0..nodes.len()).collect();
    let mut scene = json!({"name": doc.drawing.project.name.clone(), "nodes": node_ids});
    let mut asset = json!({
        "version": "2.0",
        "generator": concat!("Kadakas x3d ", env!("CARGO_PKG_VERSION")),
        "extras": {
            "source_unit": "mm",
            "unit": opts.units.id(),
            "up_axis": if opts.y_up { "Y" } else { "Z" },
            "grouping": opts.grouping,
        },
    });
    if scene["name"] == "" {
        scene.as_object_mut().expect("object").remove("name");
    }
    if !doc.drawing.project.name.is_empty() {
        asset["extras"]["project"] = json!(doc.drawing.project.name);
    }
    let mut root = json!({
        "asset": asset,
        "scene": 0,
        "scenes": [scene],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
    });
    if opts.georef {
        root["scenes"][0]["extensions"] = json!({ GEOREF_EXTENSION: georef(doc, model, opts) });
        root["extensionsUsed"] = json!([GEOREF_EXTENSION]);
    }
    if !textures.is_empty() {
        root["images"] = json!(images);
        root["textures"] = json!(textures);
        root["samplers"] =
            json!([{"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497}]);
    }
    while !b.bytes.len().is_multiple_of(4) {
        b.bytes.push(0);
    }
    let mut buffer = json!({"byteLength": b.bytes.len()});
    if embed {
        buffer["uri"] = json!(format!(
            "data:application/octet-stream;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&b.bytes)
        ));
    }
    root["buffers"] = json!([buffer]);
    root["bufferViews"] = json!(b.views);
    root["accessors"] = json!(b.accessors);
    (root, b.bytes, stats)
}

/// A `.gltf` file: JSON with the buffer embedded as a data URI.
pub fn write_gltf(
    model: &Model,
    doc: &Document,
    opts: &ExportOptions,
) -> Result<(String, ExportStats), X3dError> {
    let (root, bin, mut stats) = assemble(model, doc, opts, true);
    let text = serde_json::to_string_pretty(&root).map_err(|e| X3dError::Write(e.to_string()))?;
    stats.bytes = text.len() + bin.len();
    Ok((text, stats))
}

/// A `.glb` file: a header, a JSON chunk and a binary chunk.
pub fn write_glb(
    model: &Model,
    doc: &Document,
    opts: &ExportOptions,
) -> Result<(Vec<u8>, ExportStats), X3dError> {
    let (root, mut bin, mut stats) = assemble(model, doc, opts, false);
    let mut json = serde_json::to_vec(&root).map_err(|e| X3dError::Write(e.to_string()))?;
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    if total > u32::MAX as usize {
        return Err(X3dError::Write(
            "the model is too large for a GLB file".into(),
        ));
    }
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004E_4942u32.to_le_bytes());
    out.extend_from_slice(&bin);
    stats.bytes = out.len();
    Ok((out, stats))
}
