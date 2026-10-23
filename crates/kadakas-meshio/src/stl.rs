//! STL: reader (ASCII and binary) and writer.
//!
//! The format comes from its public description. A binary file has an 80 byte header, a count of
//! triangles (`u32`, little endian) and 50 bytes for each triangle: a normal, three corners
//! (`f32` each) and two attribute bytes. An ASCII file has `solid`, `facet normal`, `outer loop`,
//! `vertex`, `endloop`, `endfacet` and `endsolid` lines. Normals are not read: the winding
//! decides which side is the front. The corners are joined by their exact values, so the mesh is
//! indexed. The header may hold `COLOR=` and four bytes (red, green, blue, alpha), which is the
//! default color of the part. The colors of single triangles are not read.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::mesh::model_point;
use crate::{Format, Frame, Limits, MeshError, MeshSet, Part};

/// What an STL read does.
#[derive(Clone, Debug)]
pub struct StlOptions {
    pub frame: Frame,
    pub limits: Limits,
}

impl Default for StlOptions {
    fn default() -> Self {
        StlOptions {
            frame: Format::Stl.default_frame(),
            limits: Limits::default(),
        }
    }
}

const HEADER: usize = 80;
const RECORD: usize = 50;

/// True if the bytes are a binary STL file: the size agrees with the count of triangles.
fn binary_size_matches(bytes: &[u8]) -> bool {
    if bytes.len() < HEADER + 4 {
        return false;
    }
    let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as u64;
    HEADER as u64 + 4 + n * RECORD as u64 == bytes.len() as u64
}

fn looks_ascii(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    if head.contains(&0) {
        return false;
    }
    let text = String::from_utf8_lossy(head).to_ascii_lowercase();
    let t = text.trim_start();
    t.starts_with("solid") && (t.contains("facet") || t.contains("endsolid"))
}

/// Reads an STL file, binary or ASCII.
pub fn read(bytes: &[u8], opts: &StlOptions) -> Result<MeshSet, MeshError> {
    opts.limits.check_file(bytes.len() as u64)?;
    if binary_size_matches(bytes) {
        read_binary(bytes, opts)
    } else if looks_ascii(bytes) {
        read_ascii(bytes, opts)
    } else {
        // A binary file with a wrong size, or a file that is not STL: the binary reader says what
        // is missing.
        read_binary(bytes, opts)
    }
}

/// Joins equal corners.
struct Welder {
    map: HashMap<[u64; 3], u32>,
    positions: Vec<[f64; 3]>,
}

impl Welder {
    fn new() -> Self {
        Welder {
            map: HashMap::new(),
            positions: Vec::new(),
        }
    }

    fn add(&mut self, key: [u64; 3], p: [f64; 3]) -> u32 {
        *self.map.entry(key).or_insert_with(|| {
            self.positions.push(p);
            (self.positions.len() - 1) as u32
        })
    }
}

fn bits(v: f64) -> u64 {
    // -0.0 and 0.0 are one value.
    if v == 0.0 { 0 } else { v.to_bits() }
}

fn read_binary(bytes: &[u8], opts: &StlOptions) -> Result<MeshSet, MeshError> {
    let lim = &opts.limits;
    if bytes.len() < HEADER + 4 {
        return Err(MeshError::Truncated {
            what: "the STL header".into(),
            have: bytes.len() as u64,
            need: (HEADER + 4) as u64,
        });
    }
    let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as u64;
    // The counts come first, so a header that claims millions of triangles is refused at once.
    lim.check_counts(0, n)?;
    let need = HEADER as u64 + 4 + n * RECORD as u64;
    let mut notes = Vec::new();
    if (bytes.len() as u64) < need {
        return Err(MeshError::Truncated {
            what: format!("the STL data (the header says {n} triangles)"),
            have: bytes.len() as u64,
            need,
        });
    }
    if bytes.len() as u64 > need {
        notes.push(format!(
            "{} bytes after the last triangle were ignored",
            bytes.len() as u64 - need
        ));
    }
    let mut color = None;
    if let Some(at) = bytes[..HEADER].windows(6).position(|w| w == b"COLOR=")
        && at + 10 <= HEADER
    {
        let c = &bytes[at + 6..at + 10];
        color = Some([c[0], c[1], c[2], if c[3] == 0 { 255 } else { c[3] }]);
    }
    let mut w = Welder::new();
    let mut tris: Vec<[u32; 3]> = Vec::with_capacity(n as usize);
    let mut degenerate = 0u64;
    let f = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    for (i, rec) in bytes[HEADER + 4..need as usize]
        .as_chunks::<RECORD>()
        .0
        .iter()
        .enumerate()
    {
        let mut t = [0u32; 3];
        for (k, slot) in t.iter_mut().enumerate() {
            let o = 12 + k * 12;
            let v = [f(&rec[o..]), f(&rec[o + 4..]), f(&rec[o + 8..])];
            let p = model_point(
                opts.frame,
                [v[0] as f64, v[1] as f64, v[2] as f64],
                lim,
                &|| format!("triangle {}", i + 1),
            )?;
            *slot = w.add([bits(p[0]), bits(p[1]), bits(p[2])], p);
            if w.positions.len() as u64 > lim.max_vertices {
                return Err(MeshError::TooManyVertices {
                    count: w.positions.len() as u64,
                    limit: lim.max_vertices,
                });
            }
        }
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            degenerate += 1;
        } else {
            tris.push(t);
        }
    }
    let part = Part {
        name: "mesh".into(),
        color,
        positions: w.positions,
        triangles: tris,
        ..Default::default()
    };
    MeshSet::finish(Format::Stl, opts.frame, vec![part], degenerate, notes, lim)
}

fn read_ascii(bytes: &[u8], opts: &StlOptions) -> Result<MeshSet, MeshError> {
    let lim = &opts.limits;
    let text = String::from_utf8_lossy(bytes);
    let mut parts: Vec<Part> = Vec::new();
    let mut welder = Welder::new();
    let mut tris: Vec<[u32; 3]> = Vec::new();
    let mut name = String::new();
    let mut open = false;
    let mut corners: Vec<u32> = Vec::new();
    let mut in_facet = false;
    let mut total = 0u64;
    let mut degenerate = 0u64;
    let mut notes = Vec::new();
    let mut all_vertices = 0u64;
    let close = |name: &mut String,
                 welder: &mut Welder,
                 tris: &mut Vec<[u32; 3]>,
                 parts: &mut Vec<Part>,
                 all: &mut u64| {
        let w = std::mem::replace(welder, Welder::new());
        *all += w.positions.len() as u64;
        parts.push(Part {
            name: if name.is_empty() {
                "mesh".into()
            } else {
                std::mem::take(name)
            },
            positions: w.positions,
            triangles: std::mem::take(tris),
            ..Default::default()
        });
        name.clear();
    };
    for (n, line) in text.lines().enumerate() {
        let line_no = n + 1;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (key, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        match key.to_ascii_lowercase().as_str() {
            "solid" => {
                if open {
                    close(
                        &mut name,
                        &mut welder,
                        &mut tris,
                        &mut parts,
                        &mut all_vertices,
                    );
                }
                open = true;
                name = rest.trim().to_string();
            }
            "endsolid" => {
                if in_facet {
                    return Err(MeshError::Truncated {
                        what: format!("line {line_no}: a facet"),
                        have: line_no as u64,
                        need: line_no as u64 + 1,
                    });
                }
                if open {
                    close(
                        &mut name,
                        &mut welder,
                        &mut tris,
                        &mut parts,
                        &mut all_vertices,
                    );
                    open = false;
                }
            }
            "facet" => {
                if !open {
                    // Facets after an endsolid line, or in a file with no solid line.
                    open = true;
                    name.clear();
                    if !parts.is_empty() && !notes.iter().any(|n: &String| n.contains("after")) {
                        notes.push(
                            "facets after an endsolid line were put in a part of their own"
                                .to_string(),
                        );
                    }
                }
                if in_facet {
                    return Err(MeshError::Malformed(format!(
                        "line {line_no}: a facet starts before the last one ends"
                    )));
                }
                in_facet = true;
                corners.clear();
            }
            "vertex" => {
                if !in_facet || corners.len() >= 3 {
                    return Err(MeshError::Malformed(format!(
                        "line {line_no}: a vertex outside a triangle, or a fourth vertex"
                    )));
                }
                let mut v = [0.0f64; 3];
                let mut it = rest.split_whitespace();
                for slot in v.iter_mut() {
                    let t = it.next().ok_or_else(|| {
                        MeshError::Malformed(format!(
                            "line {line_no}: a vertex needs three numbers"
                        ))
                    })?;
                    *slot = t.parse().map_err(|_| {
                        MeshError::Malformed(format!("line {line_no}: '{t}' is not a number"))
                    })?;
                }
                let p = model_point(opts.frame, v, lim, &|| {
                    format!("the vertex on line {line_no}")
                })?;
                let id = welder.add([bits(p[0]), bits(p[1]), bits(p[2])], p);
                corners.push(id);
                if all_vertices + welder.positions.len() as u64 > lim.max_vertices {
                    return Err(MeshError::TooManyVertices {
                        count: all_vertices + welder.positions.len() as u64,
                        limit: lim.max_vertices,
                    });
                }
            }
            "endfacet" => {
                if !in_facet || corners.len() != 3 {
                    return Err(MeshError::Malformed(format!(
                        "line {line_no}: a facet needs exactly three vertices, it has {}",
                        corners.len()
                    )));
                }
                in_facet = false;
                total += 1;
                if total > lim.max_triangles {
                    return Err(MeshError::TooManyTriangles {
                        count: total,
                        limit: lim.max_triangles,
                    });
                }
                let t = [corners[0], corners[1], corners[2]];
                if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                    degenerate += 1;
                } else {
                    tris.push(t);
                }
            }
            _ => {}
        }
    }
    if in_facet {
        return Err(MeshError::Truncated {
            what: "the last facet".into(),
            have: corners.len() as u64,
            need: 3,
        });
    }
    if open {
        notes.push("the file has no endsolid line".to_string());
        close(
            &mut name,
            &mut welder,
            &mut tris,
            &mut parts,
            &mut all_vertices,
        );
    }
    MeshSet::finish(Format::Stl, opts.frame, parts, degenerate, notes, lim)
}

/// The normal of a triangle as `f32`, or zero for a triangle with no area.
fn normal(t: &[[f32; 3]; 3]) -> [f32; 3] {
    let a = [
        (t[1][0] - t[0][0]) as f64,
        (t[1][1] - t[0][1]) as f64,
        (t[1][2] - t[0][2]) as f64,
    ];
    let b = [
        (t[2][0] - t[0][0]) as f64,
        (t[2][1] - t[0][1]) as f64,
        (t[2][2] - t[0][2]) as f64,
    ];
    let n = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len > 0.0 && len.is_finite() {
        [
            (n[0] / len) as f32,
            (n[1] / len) as f32,
            (n[2] / len) as f32,
        ]
    } else {
        [0.0; 3]
    }
}

/// A binary STL file. `header` is written into the 80 byte header (cut to 79 bytes, and never
/// starting with the word `solid`, which would make readers think of ASCII).
pub fn write_binary<I>(header: &str, triangles: I) -> Result<Vec<u8>, MeshError>
where
    I: ExactSizeIterator<Item = [[f32; 3]; 3]>,
{
    let n = triangles.len();
    if n as u64 > u32::MAX as u64 {
        return Err(MeshError::TooManyTriangles {
            count: n as u64,
            limit: u32::MAX as u64,
        });
    }
    let mut out = Vec::with_capacity(HEADER + 4 + n * RECORD);
    let mut h: Vec<u8> = header
        .bytes()
        .filter(|b| (0x20..0x7f).contains(b))
        .take(HEADER - 1)
        .collect();
    if h.len() >= 5 && h[..5].eq_ignore_ascii_case(b"solid") {
        h.splice(0..0, *b"Kadakas ");
        h.truncate(HEADER - 1);
    }
    h.resize(HEADER, b' ');
    out.extend_from_slice(&h);
    out.extend_from_slice(&(n as u32).to_le_bytes());
    for t in triangles {
        for v in normal(&t) {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for c in t {
            for v in c {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out.extend_from_slice(&[0, 0]);
    }
    Ok(out)
}

/// An ASCII STL file with one `solid` named `name`.
pub fn write_ascii<I>(name: &str, triangles: I) -> String
where
    I: Iterator<Item = [[f32; 3]; 3]>,
{
    let clean: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let clean = if clean.trim().is_empty() {
        "mesh".to_string()
    } else {
        clean.trim().to_string()
    };
    let mut out = String::new();
    let _ = writeln!(out, "solid {clean}");
    for t in triangles {
        let n = normal(&t);
        let _ = writeln!(out, "  facet normal {:e} {:e} {:e}", n[0], n[1], n[2]);
        let _ = writeln!(out, "    outer loop");
        for c in t {
            let _ = writeln!(out, "      vertex {:e} {:e} {:e}", c[0], c[1], c[2]);
        }
        let _ = writeln!(out, "    endloop");
        let _ = writeln!(out, "  endfacet");
    }
    let _ = writeln!(out, "endsolid {clean}");
    out
}
