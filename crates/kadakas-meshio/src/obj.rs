//! Wavefront OBJ reader, with colors from the MTL file.
//!
//! The format comes from the public description of the OBJ and MTL files. The reader takes `v`
//! (position), `f` (face with any number of corners, positive and negative indices), `o` and `g`
//! (names) and `usemtl` and `mtllib` (colors). Normals, texture coordinates, lines, points,
//! curves and smoothing groups are ignored. A face with more than three corners is cut into
//! triangles. A face may refer only to the vertices that came before it.
//!
//! A part is a pair of an object name and a material. The color of a part is the diffuse color
//! `Kd` of its material, with the opacity `d` (or `1 - Tr`).

use std::collections::HashMap;

use crate::mesh::model_point;
use crate::triangulate;
use crate::{Format, Frame, Limits, MeshError, MeshSet, Part};

/// What an OBJ read does.
#[derive(Clone, Debug)]
pub struct ObjOptions {
    pub frame: Frame,
    pub limits: Limits,
}

impl Default for ObjOptions {
    fn default() -> Self {
        ObjOptions {
            frame: Format::Obj.default_frame(),
            limits: Limits::default(),
        }
    }
}

/// The largest face in corners.
const MAX_CORNERS: usize = 10_000;
/// The most parts while reading.
const MAX_PARTS_WHILE_READING: usize = 100_000;
/// The largest MTL file that is read.
const MAX_MTL_BYTES: usize = 16 * 1024 * 1024;

/// Colors by material name from the text of an MTL file: `Kd` with `d` or `Tr`. Lines that do not
/// parse are skipped.
pub fn parse_mtl(bytes: &[u8]) -> HashMap<String, [u8; 4]> {
    let mut out = HashMap::new();
    if bytes.len() > MAX_MTL_BYTES {
        return out;
    }
    let text = String::from_utf8_lossy(bytes);
    let mut name: Option<String> = None;
    let mut kd: Option<[f64; 3]> = None;
    let mut alpha = 1.0f64;
    let mut flush = |name: &mut Option<String>, kd: &mut Option<[f64; 3]>, alpha: &mut f64| {
        if let (Some(n), Some(c)) = (name.take(), kd.take()) {
            let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            out.insert(n, [b(c[0]), b(c[1]), b(c[2]), b(*alpha)]);
        }
        *kd = None;
        *alpha = 1.0;
    };
    for line in text.lines() {
        let line = line.trim();
        let (key, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        match key {
            "newmtl" => {
                flush(&mut name, &mut kd, &mut alpha);
                name = Some(rest.to_string());
            }
            "Kd" => {
                let v: Vec<f64> = rest
                    .split_whitespace()
                    .filter_map(|t| t.parse().ok())
                    .collect();
                if v.len() >= 3 && v.iter().all(|x| x.is_finite()) {
                    kd = Some([v[0], v[1], v[2]]);
                }
            }
            "d" => {
                if let Some(v) = rest
                    .split_whitespace()
                    .next()
                    .and_then(|t| t.parse::<f64>().ok())
                    && v.is_finite()
                {
                    alpha = v;
                }
            }
            "Tr" => {
                if let Some(v) = rest
                    .split_whitespace()
                    .next()
                    .and_then(|t| t.parse::<f64>().ok())
                    && v.is_finite()
                {
                    alpha = 1.0 - v;
                }
            }
            _ => {}
        }
    }
    flush(&mut name, &mut kd, &mut alpha);
    out
}

struct Pending {
    name: String,
    material: String,
    triangles: Vec<[u32; 3]>,
}

fn corner(tok: &str, count: usize, line: usize) -> Result<u32, MeshError> {
    let first = tok.split('/').next().unwrap_or("");
    let i: i64 = first
        .parse()
        .map_err(|_| MeshError::Malformed(format!("line {line}: '{tok}' is not a vertex index")))?;
    let abs = if i > 0 {
        i - 1
    } else if i < 0 {
        count as i64 + i
    } else {
        -1
    };
    if abs < 0 || abs >= count as i64 {
        return Err(MeshError::BadIndex {
            what: format!("line {line}"),
            index: i,
            count: count as u64,
        });
    }
    Ok(abs as u32)
}

struct State<'a> {
    opts: &'a ObjOptions,
    mtl: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    positions: Vec<[f64; 3]>,
    parts: Vec<Pending>,
    index: HashMap<(String, String), usize>,
    obj_name: String,
    group_name: String,
    material: String,
    current: Option<usize>,
    colors: HashMap<String, [u8; 4]>,
    triangle_count: u64,
    degenerate: u64,
    short_faces: u64,
    bad_fans: u64,
    missing_mtl: Vec<String>,
    big_mtl: Vec<String>,
    origin: Option<[f64; 3]>,
    corners: Vec<u32>,
}

/// Reads OBJ text. `mtl` returns the bytes of the file that a `mtllib` line names, or `None`.
pub fn read(
    bytes: &[u8],
    opts: &ObjOptions,
    mtl: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<MeshSet, MeshError> {
    let lim = &opts.limits;
    lim.check_file(bytes.len() as u64)?;
    let mut st = State {
        opts,
        mtl,
        positions: Vec::new(),
        parts: Vec::new(),
        index: HashMap::new(),
        obj_name: String::new(),
        group_name: String::new(),
        material: String::new(),
        current: None,
        colors: HashMap::new(),
        triangle_count: 0,
        degenerate: 0,
        short_faces: 0,
        bad_fans: 0,
        missing_mtl: Vec::new(),
        big_mtl: Vec::new(),
        origin: None,
        corners: Vec::new(),
    };
    for (n, raw) in bytes.split(|b| *b == b'\n').enumerate() {
        match std::str::from_utf8(raw) {
            Ok(line) => st.line(line, n + 1)?,
            // Names in another encoding: read them with replacement characters.
            Err(_) => st.line(&String::from_utf8_lossy(raw), n + 1)?,
        }
    }
    st.finish()
}

impl State<'_> {
    fn finish(self) -> Result<MeshSet, MeshError> {
        let mut notes: Vec<String> = Vec::new();
        if self.short_faces > 0 {
            notes.push(format!(
                "{} faces with fewer than three corners were skipped",
                self.short_faces
            ));
        }
        if self.bad_fans > 0 {
            notes.push(format!(
                "{} faces cross themselves or are not convex and have more than {} corners: they were cut as fans",
                self.bad_fans,
                triangulate::MAX_EAR_CORNERS
            ));
        }
        for m in &self.big_mtl {
            notes.push(format!(
                "the material file {m} is larger than 16 MB: the colors of the materials are not read"
            ));
        }
        for m in &self.missing_mtl {
            notes.push(format!(
                "the material file {m} was not found: the colors of the materials are not read"
            ));
        }
        // Local vertices for each part.
        let positions = self.positions;
        let mut remap = vec![u32::MAX; positions.len()];
        let mut out: Vec<Part> = Vec::with_capacity(self.parts.len());
        for p in self.parts {
            let mut used: Vec<u32> = Vec::new();
            let mut tris = Vec::with_capacity(p.triangles.len());
            for t in &p.triangles {
                let mut l = [0u32; 3];
                for k in 0..3 {
                    let g = t[k] as usize;
                    if remap[g] == u32::MAX {
                        remap[g] = used.len() as u32;
                        used.push(t[k]);
                    }
                    l[k] = remap[g];
                }
                tris.push(l);
            }
            let verts: Vec<[f64; 3]> = used.iter().map(|g| positions[*g as usize]).collect();
            for g in used {
                remap[g as usize] = u32::MAX;
            }
            out.push(Part {
                color: self.colors.get(&p.material).copied(),
                name: if p.name.is_empty() {
                    "mesh".into()
                } else {
                    p.name
                },
                material: p.material,
                positions: verts,
                triangles: tris,
            });
        }
        let mut set = MeshSet::finish(
            Format::Obj,
            self.opts.frame,
            out,
            self.degenerate,
            notes,
            &self.opts.limits,
        )?;
        set.origin_mm = self.origin;
        Ok(set)
    }

    fn line(&mut self, line: &str, line_no: usize) -> Result<(), MeshError> {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("# local origin in model mm:") {
            // Kadakas writes where the local point (0, 0, 0) of the file is in the model.
            let v: Vec<f64> = rest
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();
            if v.len() == 3 && v.iter().all(|x| x.is_finite()) {
                self.origin = Some([v[0], v[1], v[2]]);
            }
            return Ok(());
        }
        if line.is_empty() || line.starts_with('#') {
            return Ok(());
        }
        let (key, rest) = match line.split_once(char::is_whitespace) {
            Some((k, r)) => (k, r.trim()),
            None => (line, ""),
        };
        match key {
            "v" => self.vertex(rest, line_no),
            "o" | "g" => {
                if key == "o" {
                    self.obj_name = rest.to_string();
                    self.group_name.clear();
                } else {
                    self.group_name = rest.to_string();
                }
                self.current = None;
                Ok(())
            }
            "mtllib" => {
                // The names may hold spaces: try the whole text, then each word.
                let mut tries: Vec<&str> = vec![rest];
                tries.extend(rest.split_whitespace());
                let found = tries.into_iter().find_map(|name| (self.mtl)(name));
                match found {
                    Some(b) if b.len() > MAX_MTL_BYTES => {
                        self.big_mtl.push(rest.to_string());
                    }
                    Some(b) => self.colors.extend(parse_mtl(&b)),
                    None => {
                        if !rest.is_empty() && !self.missing_mtl.iter().any(|m| m == rest) {
                            self.missing_mtl.push(rest.to_string());
                        }
                    }
                }
                Ok(())
            }
            "usemtl" => {
                self.material = rest.to_string();
                self.current = None;
                Ok(())
            }
            "f" => self.face(rest, line_no),
            _ => Ok(()),
        }
    }

    fn vertex(&mut self, rest: &str, line_no: usize) -> Result<(), MeshError> {
        let lim = &self.opts.limits;
        let mut it = rest.split_whitespace();
        let mut p = [0.0f64; 3];
        for (k, slot) in p.iter_mut().enumerate() {
            let t = it.next().ok_or_else(|| {
                MeshError::Malformed(format!("line {line_no}: a vertex needs three numbers"))
            })?;
            *slot = t.parse().map_err(|_| {
                MeshError::Malformed(format!(
                    "line {line_no}: '{t}' is not a number (coordinate {})",
                    k + 1
                ))
            })?;
        }
        let q = model_point(self.opts.frame, p, lim, &|| {
            format!("the vertex on line {line_no}")
        })?;
        if self.positions.len() as u64 >= lim.max_vertices {
            return Err(MeshError::TooManyVertices {
                count: self.positions.len() as u64 + 1,
                limit: lim.max_vertices,
            });
        }
        self.positions.push(q);
        Ok(())
    }

    fn part(&mut self) -> Result<usize, MeshError> {
        if let Some(i) = self.current {
            return Ok(i);
        }
        let name = if self.group_name.is_empty() {
            self.obj_name.clone()
        } else {
            self.group_name.clone()
        };
        let key = (name.clone(), self.material.clone());
        let i = match self.index.get(&key) {
            Some(i) => *i,
            None => {
                if self.parts.len() >= MAX_PARTS_WHILE_READING {
                    return Err(MeshError::TooManyParts {
                        count: self.parts.len() as u64 + 1,
                        limit: MAX_PARTS_WHILE_READING as u64,
                    });
                }
                self.parts.push(Pending {
                    name,
                    material: self.material.clone(),
                    triangles: Vec::new(),
                });
                self.index.insert(key, self.parts.len() - 1);
                self.parts.len() - 1
            }
        };
        self.current = Some(i);
        Ok(i)
    }

    fn face(&mut self, rest: &str, line_no: usize) -> Result<(), MeshError> {
        let lim = self.opts.limits;
        self.corners.clear();
        for tok in rest.split_whitespace() {
            if self.corners.len() >= MAX_CORNERS {
                return Err(MeshError::Malformed(format!(
                    "line {line_no}: a face has more than {MAX_CORNERS} corners"
                )));
            }
            let c = corner(tok, self.positions.len(), line_no)?;
            self.corners.push(c);
        }
        if self.corners.len() < 3 {
            self.short_faces += 1;
            return Ok(());
        }
        // The budget comes before the work: a face of n corners makes n - 2 triangles.
        let most = self.corners.len() as u64 - 2;
        if self.triangle_count + most > lim.max_triangles {
            return Err(MeshError::TooManyTriangles {
                count: self.triangle_count + most,
                limit: lim.max_triangles,
            });
        }
        let at = self.part()?;
        let tris: Vec<[usize; 3]> = if self.corners.len() == 3 {
            vec![[0, 1, 2]]
        } else {
            let pts: Vec<[f64; 3]> = self
                .corners
                .iter()
                .map(|c| self.positions[*c as usize])
                .collect();
            let (t, ok) = triangulate::polygon(&pts);
            if !ok {
                self.bad_fans += 1;
            }
            t
        };
        let added = tris.len() as u64;
        if self.triangle_count + added > lim.max_triangles {
            return Err(MeshError::TooManyTriangles {
                count: self.triangle_count + added,
                limit: lim.max_triangles,
            });
        }
        for t in tris {
            let tri = [self.corners[t[0]], self.corners[t[1]], self.corners[t[2]]];
            if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
                self.degenerate += 1;
                continue;
            }
            self.parts[at].triangles.push(tri);
            self.triangle_count += 1;
        }
        let mem = Limits::memory_of(self.positions.len() as u64, self.triangle_count);
        if mem > lim.max_memory {
            return Err(MeshError::TooMuchMemory {
                bytes: mem,
                limit: lim.max_memory,
            });
        }
        Ok(())
    }
}
