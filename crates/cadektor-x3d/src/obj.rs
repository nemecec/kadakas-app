//! The Wavefront OBJ writer, with an MTL file for the materials.
//!
//! The options are the same as for glTF: grouping, axes and unit. OBJ has no instancing, so
//! every object writes its own vertices. Positions are written in the axes and unit of the
//! options, around the scene origin (the georeference goes to a comment in the header).

use std::fmt::Write;

use cadektor_core::model::Document;

use crate::options::ExportOptions;
use crate::parts::Model;
use crate::{ExportStats, X3dError};

/// The files of an OBJ export.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjFiles {
    pub obj: String,
    pub mtl: String,
    /// Texture files, by the name that the MTL refers to.
    pub textures: Vec<(String, Vec<u8>)>,
}

pub(crate) fn clean(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_whitespace() || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    if s.is_empty() { "_".into() } else { s }
}

/// A file name for a texture: the name without path characters, with the extension of the type.
fn texture_file(name: &str, mime: &str, index: usize) -> String {
    let ext = if mime == "image/jpeg" { "jpg" } else { "png" };
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!(
        "{}_{index}.{ext}",
        if stem.is_empty() { "texture" } else { &stem }
    )
}

/// Writes the OBJ and MTL text. `mtl_name` is the file name of the MTL, for the `mtllib` line.
pub fn write_obj(
    model: &Model,
    doc: &Document,
    opts: &ExportOptions,
    mtl_name: &str,
) -> Result<(ObjFiles, ExportStats), X3dError> {
    let mut obj = String::new();
    let mut stats = ExportStats::default();
    let decimals = if opts.units.factor() < 1.0 { 6 } else { 3 };
    let w = |e: std::fmt::Error| X3dError::Write(e.to_string());
    writeln!(obj, "# Cadektor x3d {}", env!("CARGO_PKG_VERSION")).map_err(w)?;
    writeln!(
        obj,
        "# unit {}, up axis {}",
        opts.units.id(),
        if opts.y_up { "Y" } else { "Z" }
    )
    .map_err(w)?;
    if opts.georef {
        let o = model.origin;
        writeln!(obj, "# local origin in model mm: {} {} {}", o.x, o.y, o.z).map_err(w)?;
        if let Some(g) = &doc.georef {
            writeln!(
                obj,
                "# crs {} map origin {} {} rotation {}",
                g.crs, g.origin.x, g.origin.y, g.rotation
            )
            .map_err(w)?;
        }
    }
    writeln!(obj, "mtllib {}", clean(mtl_name)).map_err(w)?;
    let (mut vbase, mut nbase, mut tbase) = (1usize, 1usize, 1usize);
    for part in &model.parts {
        writeln!(obj, "o {}", clean(&part.name)).map_err(w)?;
        for prim in &part.prims {
            let m = &model.materials[prim.material];
            writeln!(obj, "usemtl {}", clean(&m.name)).map_err(w)?;
            for p in &prim.positions {
                writeln!(
                    obj,
                    "v {:.d$} {:.d$} {:.d$}",
                    p[0] as f64 + part.anchor[0],
                    p[1] as f64 + part.anchor[1],
                    p[2] as f64 + part.anchor[2],
                    d = decimals
                )
                .map_err(w)?;
            }
            if !prim.lines {
                for n in &prim.normals {
                    writeln!(obj, "vn {:.5} {:.5} {:.5}", n[0], n[1], n[2]).map_err(w)?;
                }
                for t in &prim.uvs {
                    writeln!(obj, "vt {:.5} {:.5}", t[0], t[1]).map_err(w)?;
                }
            }
            let textured = !prim.uvs.is_empty();
            if prim.lines {
                for s in prim.indices.chunks_exact(2) {
                    writeln!(obj, "l {} {}", vbase + s[0] as usize, vbase + s[1] as usize)
                        .map_err(w)?;
                }
                stats.lines += prim.indices.len() / 2;
            } else {
                for t in prim.indices.chunks_exact(3) {
                    let f = |i: u32| {
                        let v = vbase + i as usize;
                        let n = nbase + i as usize;
                        if textured {
                            format!("{v}/{}/{n}", tbase + i as usize)
                        } else {
                            format!("{v}//{n}")
                        }
                    };
                    writeln!(obj, "f {} {} {}", f(t[0]), f(t[1]), f(t[2])).map_err(w)?;
                }
                stats.triangles += prim.indices.len() / 3;
            }
            vbase += prim.positions.len();
            if !prim.lines {
                nbase += prim.normals.len();
                tbase += prim.uvs.len();
            }
        }
        stats.objects += part.ids.len();
        stats.nodes += 1;
        stats.meshes += 1;
    }
    let mut mtl = String::new();
    let mut textures: Vec<(String, Vec<u8>)> = Vec::new();
    writeln!(mtl, "# Cadektor x3d {}", env!("CARGO_PKG_VERSION")).map_err(w)?;
    for m in &model.materials {
        let c = m.srgb();
        writeln!(mtl, "newmtl {}", clean(&m.name)).map_err(w)?;
        writeln!(mtl, "Ka 0.0 0.0 0.0").map_err(w)?;
        writeln!(mtl, "Kd {:.4} {:.4} {:.4}", c[0], c[1], c[2]).map_err(w)?;
        writeln!(mtl, "Ks 0.04 0.04 0.04").map_err(w)?;
        if m.emissive != [0.0; 3] {
            writeln!(
                mtl,
                "Ke {:.4} {:.4} {:.4}",
                m.emissive[0], m.emissive[1], m.emissive[2]
            )
            .map_err(w)?;
        }
        writeln!(mtl, "Ns {:.1}", (1.0 - m.roughness.clamp(0.0, 1.0)) * 100.0).map_err(w)?;
        writeln!(mtl, "d {:.4}", m.base_color[3]).map_err(w)?;
        writeln!(mtl, "illum 2").map_err(w)?;
        if let Some(t) = &m.texture {
            let file = match textures.iter().position(|(_, d)| *d == t.data) {
                Some(i) => textures[i].0.clone(),
                None => {
                    let f = texture_file(&t.name, &t.mime, textures.len());
                    textures.push((f.clone(), t.data.clone()));
                    f
                }
            };
            writeln!(mtl, "map_Kd {file}").map_err(w)?;
            stats.textures = textures.len();
        }
    }
    stats.materials = model.materials.len();
    stats.skipped = model.skipped.clone();
    stats.bytes = obj.len() + mtl.len() + textures.iter().map(|(_, d)| d.len()).sum::<usize>();
    Ok((ObjFiles { obj, mtl, textures }, stats))
}
