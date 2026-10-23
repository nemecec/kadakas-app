//! glTF 2.0 reader: `.gltf` (JSON, buffers embedded as data URIs or in other files) and `.glb`.
//!
//! The format comes from the public Khronos specification. The reader takes the triangle
//! primitives (modes 4, 5 and 6) of the meshes that the default scene reaches, with the
//! transforms of the nodes. Positions are meters with Y up, as the specification says. A part
//! is a pair of a node name and a material, and its color is the base color factor of the
//! material (converted from linear to sRGB). Textures, normals, tangents, texture coordinates,
//! vertex colors, skins, animations and cameras are ignored. Sparse accessors, Draco and Meshopt
//! compression, and quantized positions are refused with a message.

use std::collections::HashMap;

use base64::Engine;
use serde::Deserialize;

use crate::mesh::model_point;
use crate::{Format, Frame, Limits, MeshError, MeshSet, Part, Unit, Up};

/// What a glTF read does.
#[derive(Clone, Debug, Default)]
pub struct GltfOptions {
    pub limits: Limits,
}

/// The frame of glTF: meters, Y up.
pub const FRAME: Frame = Frame {
    unit: Unit::Metre,
    up: Up::Y,
};

#[derive(Deserialize, Default)]
#[serde(default)]
struct Root {
    asset: Asset,
    scene: Option<usize>,
    scenes: Vec<Scene>,
    nodes: Vec<Node>,
    meshes: Vec<Mesh>,
    accessors: Vec<Accessor>,
    #[serde(rename = "bufferViews")]
    buffer_views: Vec<View>,
    buffers: Vec<Buffer>,
    materials: Vec<Material>,
    #[serde(rename = "extensionsRequired")]
    extensions_required: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Asset {
    version: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Scene {
    nodes: Vec<usize>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Node {
    name: Option<String>,
    mesh: Option<usize>,
    children: Vec<usize>,
    matrix: Option<Vec<f64>>,
    translation: Option<Vec<f64>>,
    rotation: Option<Vec<f64>>,
    scale: Option<Vec<f64>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Mesh {
    name: Option<String>,
    primitives: Vec<Primitive>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Primitive {
    attributes: HashMap<String, usize>,
    indices: Option<usize>,
    material: Option<usize>,
    mode: Option<u32>,
    extensions: Option<serde_json::Value>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Accessor {
    #[serde(rename = "bufferView")]
    buffer_view: Option<usize>,
    #[serde(rename = "byteOffset")]
    byte_offset: u64,
    #[serde(rename = "componentType")]
    component_type: u32,
    count: u64,
    #[serde(rename = "type")]
    kind: String,
    normalized: bool,
    sparse: Option<serde_json::Value>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct View {
    buffer: usize,
    #[serde(rename = "byteOffset")]
    byte_offset: u64,
    #[serde(rename = "byteLength")]
    byte_length: u64,
    #[serde(rename = "byteStride")]
    byte_stride: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Buffer {
    uri: Option<String>,
    #[serde(rename = "byteLength")]
    byte_length: u64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Material {
    name: Option<String>,
    #[serde(rename = "pbrMetallicRoughness")]
    pbr: Option<Pbr>,
    extensions: Option<serde_json::Value>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Pbr {
    #[serde(rename = "baseColorFactor")]
    base_color_factor: Option<Vec<f64>>,
    #[serde(rename = "baseColorTexture")]
    base_color_texture: Option<serde_json::Value>,
}

const MAGIC: &[u8; 4] = b"glTF";
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;
const REFUSED_EXTENSIONS: [&str; 4] = [
    "KHR_draco_mesh_compression",
    "EXT_meshopt_compression",
    "KHR_mesh_quantization",
    "EXT_mesh_gpu_instancing",
];

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// The JSON text and the binary chunk of a GLB file.
fn split_glb(bytes: &[u8]) -> Result<(&[u8], Option<&[u8]>), MeshError> {
    if bytes.len() < 12 {
        return Err(MeshError::Truncated {
            what: "the GLB header".into(),
            have: bytes.len() as u64,
            need: 12,
        });
    }
    if u32_at(bytes, 4) != 2 {
        return Err(MeshError::Unsupported(format!(
            "GLB version {} is not supported, only 2",
            u32_at(bytes, 4)
        )));
    }
    let total = u32_at(bytes, 8) as u64;
    if total > bytes.len() as u64 {
        return Err(MeshError::Truncated {
            what: "the GLB file".into(),
            have: bytes.len() as u64,
            need: total,
        });
    }
    let end = (total as usize).max(12).min(bytes.len());
    let mut at = 12usize;
    let (mut json, mut bin) = (None, None);
    while at + 8 <= end {
        let len = u32_at(bytes, at) as usize;
        let kind = u32_at(bytes, at + 4);
        let start = at + 8;
        let stop = start
            .checked_add(len)
            .filter(|s| *s <= end)
            .ok_or_else(|| MeshError::Truncated {
                what: "a GLB chunk".into(),
                have: (end - start) as u64,
                need: len as u64,
            })?;
        match kind {
            CHUNK_JSON if json.is_none() => json = Some(&bytes[start..stop]),
            CHUNK_BIN if bin.is_none() => bin = Some(&bytes[start..stop]),
            _ => {}
        }
        // Chunks are padded to four bytes.
        at = stop + (4 - stop % 4) % 4;
    }
    let json = json.ok_or_else(|| MeshError::Malformed("the GLB file has no JSON chunk".into()))?;
    Ok((json, bin))
}

/// Linear to sRGB for one channel in 0..=1.
fn srgb(c: f64) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

type Mat = [f64; 16];

const IDENTITY: Mat = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// `a * b` for column-major matrices.
fn mul(a: &Mat, b: &Mat) -> Mat {
    let mut o = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            o[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    o
}

fn node_matrix(n: &Node, idx: usize) -> Result<Mat, MeshError> {
    let bad = || MeshError::Malformed(format!("node {idx} has a transform that is not valid"));
    if let Some(m) = &n.matrix {
        if m.len() != 16 || !m.iter().all(|v| v.is_finite()) {
            return Err(bad());
        }
        let mut o = [0.0; 16];
        o.copy_from_slice(m);
        return Ok(o);
    }
    let get = |v: &Option<Vec<f64>>, len: usize, default: &[f64]| -> Result<Vec<f64>, MeshError> {
        match v {
            None => Ok(default.to_vec()),
            Some(v) if v.len() == len && v.iter().all(|x| x.is_finite()) => Ok(v.clone()),
            _ => Err(bad()),
        }
    };
    let t = get(&n.translation, 3, &[0.0; 3])?;
    let r = get(&n.rotation, 4, &[0.0, 0.0, 0.0, 1.0])?;
    let s = get(&n.scale, 3, &[1.0; 3])?;
    let len = r.iter().map(|x| x * x).sum::<f64>().sqrt();
    if len == 0.0 {
        return Err(bad());
    }
    let (x, y, z, w) = (r[0] / len, r[1] / len, r[2] / len, r[3] / len);
    let rot = [
        1.0 - 2.0 * (y * y + z * z),
        2.0 * (x * y + z * w),
        2.0 * (x * z - y * w),
        0.0,
        2.0 * (x * y - z * w),
        1.0 - 2.0 * (x * x + z * z),
        2.0 * (y * z + x * w),
        0.0,
        2.0 * (x * z + y * w),
        2.0 * (y * z - x * w),
        1.0 - 2.0 * (x * x + y * y),
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ];
    let mut o = rot;
    for c in 0..3 {
        for row in 0..3 {
            o[c * 4 + row] *= s[c];
        }
    }
    o[12] = t[0];
    o[13] = t[1];
    o[14] = t[2];
    Ok(o)
}

fn det3(m: &Mat) -> f64 {
    m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2])
        + m[8] * (m[1] * m[6] - m[5] * m[2])
}

fn apply(m: &Mat, p: [f32; 3]) -> [f64; 3] {
    let (x, y, z) = (p[0] as f64, p[1] as f64, p[2] as f64);
    [
        m[0] * x + m[4] * y + m[8] * z + m[12],
        m[1] * x + m[5] * y + m[9] * z + m[13],
        m[2] * x + m[6] * y + m[10] * z + m[14],
    ]
}

struct Data<'a> {
    root: &'a Root,
    buffers: Vec<Vec<u8>>,
    bin: Option<&'a [u8]>,
}

impl Data<'_> {
    fn buffer(&self, i: usize) -> &[u8] {
        match self.buffers.get(i) {
            Some(b) if !b.is_empty() => b,
            _ => {
                if i == 0 {
                    self.bin.unwrap_or(&[])
                } else {
                    &[]
                }
            }
        }
    }

    /// The bytes of an accessor: the slice, the stride and the element size.
    fn span(
        &self,
        acc: usize,
        components: usize,
        what: &str,
    ) -> Result<(&[u8], usize, usize, &Accessor), MeshError> {
        let a = self.root.accessors.get(acc).ok_or_else(|| {
            MeshError::Malformed(format!("{what}: accessor {acc} does not exist"))
        })?;
        if a.sparse.is_some() {
            return Err(MeshError::Unsupported(format!(
                "{what}: sparse accessors are not supported"
            )));
        }
        let size = match a.component_type {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            t => {
                return Err(MeshError::Malformed(format!(
                    "{what}: component type {t} is not valid"
                )));
            }
        };
        let elem = size * components;
        let Some(v) = a.buffer_view else {
            return Err(MeshError::Unsupported(format!(
                "{what}: an accessor without a buffer view is not supported"
            )));
        };
        let view = self.root.buffer_views.get(v).ok_or_else(|| {
            MeshError::Malformed(format!("{what}: buffer view {v} does not exist"))
        })?;
        let buf = self.buffer(view.buffer);
        let stride = view.byte_stride.unwrap_or(elem as u64).max(elem as u64);
        let start = view.byte_offset.saturating_add(a.byte_offset);
        let need = if a.count == 0 {
            0
        } else {
            (a.count - 1)
                .checked_mul(stride)
                .and_then(|x| x.checked_add(elem as u64))
                .ok_or_else(|| MeshError::Malformed(format!("{what}: the accessor is too large")))?
        };
        let end = start.saturating_add(need);
        let view_end = view.byte_offset.saturating_add(view.byte_length);
        if end > view_end || end > buf.len() as u64 {
            return Err(MeshError::Truncated {
                what: format!("{what}: accessor {acc} reads past its buffer, and"),
                have: (buf.len() as u64).min(view_end),
                need: end,
            });
        }
        Ok((&buf[start as usize..end as usize], stride as usize, elem, a))
    }

    fn positions(
        &self,
        acc: usize,
        limits: &Limits,
        what: &str,
    ) -> Result<Vec<[f32; 3]>, MeshError> {
        let a = self.root.accessors.get(acc).ok_or_else(|| {
            MeshError::Malformed(format!("{what}: accessor {acc} does not exist"))
        })?;
        if a.count > limits.max_vertices {
            return Err(MeshError::TooManyVertices {
                count: a.count,
                limit: limits.max_vertices,
            });
        }
        if a.component_type != 5126 || a.kind != "VEC3" || a.normalized {
            return Err(MeshError::Unsupported(format!(
                "{what}: positions must be VEC3 of floats (component type {}, type {})",
                a.component_type, a.kind
            )));
        }
        let (data, stride, _, a) = self.span(acc, 3, what)?;
        let mut out = Vec::with_capacity(a.count as usize);
        for i in 0..a.count as usize {
            let o = i * stride;
            let f = |k: usize| {
                f32::from_le_bytes([
                    data[o + k],
                    data[o + k + 1],
                    data[o + k + 2],
                    data[o + k + 3],
                ])
            };
            let p = [f(0), f(4), f(8)];
            if !p.iter().all(|v| v.is_finite()) {
                return Err(MeshError::NotFinite {
                    what: format!("{what}: position {i}"),
                });
            }
            out.push(p);
        }
        Ok(out)
    }

    fn indices(&self, acc: usize, limits: &Limits, what: &str) -> Result<Vec<u32>, MeshError> {
        let a = self.root.accessors.get(acc).ok_or_else(|| {
            MeshError::Malformed(format!("{what}: accessor {acc} does not exist"))
        })?;
        if a.count > limits.max_triangles.saturating_mul(3) {
            return Err(MeshError::TooManyTriangles {
                count: a.count / 3,
                limit: limits.max_triangles,
            });
        }
        if a.kind != "SCALAR" || !matches!(a.component_type, 5121 | 5123 | 5125) {
            return Err(MeshError::Unsupported(format!(
                "{what}: indices must be SCALAR of unsigned bytes, shorts or ints"
            )));
        }
        let (data, stride, _, a) = self.span(acc, 1, what)?;
        let mut out = Vec::with_capacity(a.count as usize);
        for i in 0..a.count as usize {
            let o = i * stride;
            out.push(match a.component_type {
                5121 => data[o] as u32,
                5123 => u16::from_le_bytes([data[o], data[o + 1]]) as u32,
                _ => u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]),
            });
        }
        Ok(out)
    }
}

/// A decoded primitive: positions and triangles (strips and fans already expanded).
struct Decoded {
    positions: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
    degenerate: u64,
}

fn safe_name(uri: &str) -> Option<String> {
    let mut out = String::new();
    let b = uri.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let h = std::str::from_utf8(&b[i + 1..i + 3]).ok()?;
            let v = u8::from_str_radix(h, 16).ok()?;
            out.push(v as char);
            i += 3;
        } else {
            out.push(b[i] as char);
            i += 1;
        }
    }
    let bad = out.is_empty()
        || out.starts_with('/')
        || out.starts_with('\\')
        || out.contains("..")
        || out.contains(':')
        || out.contains('\0');
    (!bad).then_some(out)
}

/// Reads a glTF or GLB file. `external` returns the bytes of a buffer file next to the glTF file
/// (the name is checked: no folders, no `..`), or `None`.
pub fn read(
    bytes: &[u8],
    opts: &GltfOptions,
    external: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<MeshSet, MeshError> {
    let lim = &opts.limits;
    lim.check_file(bytes.len() as u64)?;
    let (json, bin) = if bytes.starts_with(MAGIC) {
        split_glb(bytes)?
    } else {
        (bytes, None)
    };
    let root: Root = serde_json::from_slice(json)
        .map_err(|e| MeshError::Malformed(format!("the glTF JSON is not valid: {e}")))?;
    if !root.asset.version.starts_with('2') {
        return Err(MeshError::Unsupported(format!(
            "glTF version '{}' is not supported, only 2.x",
            root.asset.version
        )));
    }
    if let Some(e) = root
        .extensions_required
        .iter()
        .find(|e| REFUSED_EXTENSIONS.contains(&e.as_str()))
    {
        return Err(MeshError::Unsupported(format!(
            "the file needs the extension {e}, which is not supported"
        )));
    }
    let mut notes: Vec<String> = Vec::new();
    // Buffers.
    let mut buffers: Vec<Vec<u8>> = Vec::with_capacity(root.buffers.len());
    for (i, b) in root.buffers.iter().enumerate() {
        match &b.uri {
            None => buffers.push(Vec::new()),
            Some(u) if u.starts_with("data:") => {
                let (_, data) = u.split_once(',').ok_or_else(|| {
                    MeshError::Malformed(format!("buffer {i}: the data URI has no comma"))
                })?;
                let est = data.len() as u64 / 4 * 3;
                if est > lim.max_bytes {
                    return Err(MeshError::FileTooLarge {
                        bytes: est,
                        limit: lim.max_bytes,
                    });
                }
                let d = base64::engine::general_purpose::STANDARD
                    .decode(data.trim())
                    .map_err(|e| {
                        MeshError::Malformed(format!(
                            "buffer {i}: the base64 text is not valid: {e}"
                        ))
                    })?;
                buffers.push(d);
            }
            Some(u) => {
                let name = safe_name(u).ok_or_else(|| {
                    MeshError::Malformed(format!("buffer {i}: the file name '{u}' is not allowed"))
                })?;
                let d = external(&name).ok_or_else(|| {
                    MeshError::Malformed(format!("buffer {i}: the file '{name}' was not found"))
                })?;
                if d.len() as u64 > lim.max_bytes {
                    return Err(MeshError::FileTooLarge {
                        bytes: d.len() as u64,
                        limit: lim.max_bytes,
                    });
                }
                buffers.push(d);
            }
        }
        let have = if b.uri.is_none() {
            bin.map_or(0, |x| x.len() as u64)
        } else {
            buffers[i].len() as u64
        };
        if have < b.byte_length {
            return Err(MeshError::Truncated {
                what: format!("buffer {i}"),
                have,
                need: b.byte_length,
            });
        }
    }
    let data = Data {
        root: &root,
        buffers,
        bin,
    };
    // Roots of the scene.
    let roots: Vec<usize> = match root.scene.or(if root.scenes.is_empty() {
        None
    } else {
        Some(0)
    }) {
        Some(s) => root
            .scenes
            .get(s)
            .ok_or_else(|| MeshError::Malformed(format!("scene {s} does not exist")))?
            .nodes
            .clone(),
        None => {
            let mut child = vec![false; root.nodes.len()];
            for n in &root.nodes {
                for c in &n.children {
                    if let Some(f) = child.get_mut(*c) {
                        *f = true;
                    }
                }
            }
            (0..root.nodes.len()).filter(|i| !child[*i]).collect()
        }
    };
    let mut parts: Vec<Part> = Vec::new();
    let mut index: HashMap<(String, Option<usize>), usize> = HashMap::new();
    let mut cache: HashMap<(usize, usize), Decoded> = HashMap::new();
    let (mut vertex_total, mut triangle_total, mut degenerate) = (0u64, 0u64, 0u64);
    let mut skipped_modes = 0u64;
    let mut textured = 0u64;
    let mut visits = 0u64;
    let mut stack: Vec<(usize, Mat, u32)> = roots.iter().rev().map(|n| (*n, IDENTITY, 0)).collect();
    while let Some((ni, parent, depth)) = stack.pop() {
        visits += 1;
        if depth > 256 || visits > 2_000_000 {
            return Err(MeshError::Malformed(
                "the nodes are nested too deep, or refer to each other in a loop".into(),
            ));
        }
        let node = root
            .nodes
            .get(ni)
            .ok_or_else(|| MeshError::Malformed(format!("node {ni} does not exist")))?;
        let world = mul(&parent, &node_matrix(node, ni)?);
        for c in node.children.iter().rev() {
            stack.push((*c, world, depth + 1));
        }
        let Some(mi) = node.mesh else { continue };
        let mesh = root
            .meshes
            .get(mi)
            .ok_or_else(|| MeshError::Malformed(format!("node {ni}: mesh {mi} does not exist")))?;
        let flip = det3(&world) < 0.0;
        for (pi, prim) in mesh.primitives.iter().enumerate() {
            let what = format!("mesh {mi} primitive {pi}");
            let mode = prim.mode.unwrap_or(4);
            if !matches!(mode, 4..=6) {
                skipped_modes += 1;
                continue;
            }
            if prim
                .extensions
                .as_ref()
                .and_then(|e| e.as_object())
                .is_some_and(|o| REFUSED_EXTENSIONS.iter().any(|x| o.contains_key(*x)))
            {
                return Err(MeshError::Unsupported(format!(
                    "{what}: compressed or quantized geometry is not supported"
                )));
            }
            if let std::collections::hash_map::Entry::Vacant(slot) = cache.entry((mi, pi)) {
                let acc = *prim.attributes.get("POSITION").ok_or_else(|| {
                    MeshError::Malformed(format!("{what} has no POSITION attribute"))
                })?;
                let positions = data.positions(acc, lim, &what)?;
                let raw: Vec<u32> = match prim.indices {
                    Some(i) => data.indices(i, lim, &what)?,
                    None => (0..positions.len() as u32).collect(),
                };
                if let Some(bad) = raw.iter().find(|i| **i as usize >= positions.len()) {
                    return Err(MeshError::BadIndex {
                        what,
                        index: *bad as i64,
                        count: positions.len() as u64,
                    });
                }
                let mut tris: Vec<[u32; 3]> = Vec::new();
                match mode {
                    4 => tris.extend(raw.as_chunks::<3>().0.iter().copied()),
                    5 => {
                        for k in 0..raw.len().saturating_sub(2) {
                            // The winding alternates in a strip.
                            tris.push(if k % 2 == 0 {
                                [raw[k], raw[k + 1], raw[k + 2]]
                            } else {
                                [raw[k + 1], raw[k], raw[k + 2]]
                            });
                        }
                    }
                    _ => {
                        for k in 1..raw.len().saturating_sub(1) {
                            tris.push([raw[0], raw[k], raw[k + 1]]);
                        }
                    }
                }
                let before = tris.len();
                tris.retain(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
                let dropped = (before - tris.len()) as u64;
                slot.insert(Decoded {
                    positions,
                    triangles: tris,
                    degenerate: dropped,
                });
            }
            let dec = &cache[&(mi, pi)];
            vertex_total += dec.positions.len() as u64;
            triangle_total += dec.triangles.len() as u64;
            degenerate += dec.degenerate;
            lim.check_counts(vertex_total, triangle_total)?;
            let name = node
                .name
                .clone()
                .or_else(|| mesh.name.clone())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("node {ni}"));
            let key = (name.clone(), prim.material);
            let at = *index.entry(key).or_insert_with(|| {
                let (color, material) = material_of(&root, prim.material, &mut textured);
                parts.push(Part {
                    name,
                    material,
                    color,
                    ..Default::default()
                });
                parts.len() - 1
            });
            if parts.len() as u64 > lim.max_parts.saturating_mul(20) {
                return Err(MeshError::TooManyParts {
                    count: parts.len() as u64,
                    limit: lim.max_parts,
                });
            }
            let part = &mut parts[at];
            let base = part.positions.len() as u32;
            for (k, p) in dec.positions.iter().enumerate() {
                let w = apply(&world, *p);
                part.positions.push(model_point(FRAME, w, lim, &|| {
                    format!("{what}: position {k}")
                })?);
            }
            part.triangles.extend(dec.triangles.iter().map(|t| {
                if flip {
                    [base + t[0], base + t[2], base + t[1]]
                } else {
                    [base + t[0], base + t[1], base + t[2]]
                }
            }));
        }
    }
    if skipped_modes > 0 {
        notes.push(format!(
            "{skipped_modes} primitives of points or lines were skipped"
        ));
    }
    if textured > 0 {
        notes.push(format!(
            "{textured} materials have a texture: textures are not read, the base color is kept"
        ));
    }
    MeshSet::finish(Format::Gltf, FRAME, parts, degenerate, notes, lim)
}

fn material_of(root: &Root, m: Option<usize>, textured: &mut u64) -> (Option<[u8; 4]>, String) {
    let Some(mat) = m.and_then(|i| root.materials.get(i)) else {
        return (None, String::new());
    };
    let name = mat.name.clone().unwrap_or_default();
    let mut factor: Option<Vec<f64>> = mat.pbr.as_ref().and_then(|p| p.base_color_factor.clone());
    if mat
        .pbr
        .as_ref()
        .is_some_and(|p| p.base_color_texture.is_some())
    {
        *textured += 1;
    }
    if factor.is_none()
        && let Some(d) = mat
            .extensions
            .as_ref()
            .and_then(|e| e.get("KHR_materials_pbrSpecularGlossiness"))
            .and_then(|e| e.get("diffuseFactor"))
            .and_then(|f| serde_json::from_value::<Vec<f64>>(f.clone()).ok())
    {
        factor = Some(d);
    }
    let f = factor.unwrap_or_else(|| vec![1.0; 4]);
    if f.len() != 4 || !f.iter().all(|v| v.is_finite()) {
        return (None, name);
    }
    (
        Some([
            srgb(f[0]),
            srgb(f[1]),
            srgb(f[2]),
            (f[3].clamp(0.0, 1.0) * 255.0).round() as u8,
        ]),
        name,
    )
}
