//! The result of reading a mesh file, and the limits of a read.

use serde::Serialize;

use crate::{Format, Frame, MeshError};

/// What a read may use. A read stops at the first limit it passes and names the counts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// The largest file in bytes. The check comes before anything is parsed.
    pub max_bytes: u64,
    /// Vertices in all parts together.
    pub max_vertices: u64,
    /// Triangles in all parts together.
    pub max_triangles: u64,
    /// Parts (objects with a name and a color).
    pub max_parts: u64,
    /// The most memory that the parts may take, estimated from the counts.
    pub max_memory: u64,
    /// The largest coordinate in millimeters.
    pub max_coord_mm: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_bytes: 512 * 1024 * 1024,
            max_vertices: 4_000_000,
            max_triangles: 8_000_000,
            max_parts: 5_000,
            max_memory: 2 * 1024 * 1024 * 1024,
            max_coord_mm: 1.0e9,
        }
    }
}

impl Limits {
    /// The memory that a mesh of these counts takes: a vertex is three f64, a triangle three u32,
    /// and the file text and the working copies take about the same again.
    pub fn memory_of(vertices: u64, triangles: u64) -> u64 {
        (vertices * 24 + triangles * 12).saturating_mul(2)
    }

    pub(crate) fn check_file(&self, bytes: u64) -> Result<(), MeshError> {
        if bytes > self.max_bytes {
            return Err(MeshError::FileTooLarge {
                bytes,
                limit: self.max_bytes,
            });
        }
        Ok(())
    }

    pub(crate) fn check_counts(&self, vertices: u64, triangles: u64) -> Result<(), MeshError> {
        if triangles > self.max_triangles {
            return Err(MeshError::TooManyTriangles {
                count: triangles,
                limit: self.max_triangles,
            });
        }
        if vertices > self.max_vertices {
            return Err(MeshError::TooManyVertices {
                count: vertices,
                limit: self.max_vertices,
            });
        }
        let need = Limits::memory_of(vertices, triangles);
        if need > self.max_memory {
            return Err(MeshError::TooMuchMemory {
                bytes: need,
                limit: self.max_memory,
            });
        }
        Ok(())
    }
}

/// One object of a file: indexed triangles in millimeters, Z up, counterclockwise seen from
/// outside, with a name and a color when the file has them.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Part {
    pub name: String,
    /// The name of the material in the file, if any.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub material: String,
    /// RGBA, from the material of the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 4]>,
    pub positions: Vec<[f64; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

/// The counts of a read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Totals {
    pub parts: u64,
    pub vertices: u64,
    pub triangles: u64,
    /// Triangles that were left out because two corners were the same vertex.
    pub degenerate: u64,
}

/// The parts of a file, with what the reader did.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshSet {
    pub format: Format,
    /// The unit and axis that were applied.
    pub frame: Frame,
    pub parts: Vec<Part>,
    pub totals: Totals,
    /// What the reader left out or changed, in words.
    pub notes: Vec<String>,
}

impl MeshSet {
    pub(crate) fn finish(
        format: Format,
        frame: Frame,
        mut parts: Vec<Part>,
        degenerate: u64,
        mut notes: Vec<String>,
        limits: &Limits,
    ) -> Result<MeshSet, MeshError> {
        parts.retain(|p| !p.triangles.is_empty());
        if parts.is_empty() {
            return Err(MeshError::Empty);
        }
        if parts.len() as u64 > limits.max_parts {
            // Many small parts: join those with the same color.
            let before = parts.len();
            parts = join_by_color(parts);
            notes.push(format!(
                "{before} parts were joined by color into {}",
                parts.len()
            ));
            if parts.len() as u64 > limits.max_parts {
                return Err(MeshError::TooManyParts {
                    count: parts.len() as u64,
                    limit: limits.max_parts,
                });
            }
        }
        let totals = Totals {
            parts: parts.len() as u64,
            vertices: parts.iter().map(|p| p.positions.len() as u64).sum(),
            triangles: parts.iter().map(|p| p.triangles.len() as u64).sum(),
            degenerate,
        };
        Ok(MeshSet {
            format,
            frame,
            parts,
            totals,
            notes,
        })
    }

    /// The box around all vertices in millimeters as (min, max).
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for p in self.parts.iter().flat_map(|p| p.positions.iter()) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }

    /// Joins all parts into one, keeping the color when all parts have the same.
    pub fn joined(&self) -> Part {
        let mut out = Part {
            name: "mesh".into(),
            ..Default::default()
        };
        for p in &self.parts {
            let base = out.positions.len() as u32;
            out.positions.extend_from_slice(&p.positions);
            out.triangles.extend(
                p.triangles
                    .iter()
                    .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
            );
        }
        let first = self.parts.first().and_then(|p| p.color);
        if self.parts.iter().all(|p| p.color == first) {
            out.color = first;
        }
        out
    }
}

fn join_by_color(parts: Vec<Part>) -> Vec<Part> {
    let mut out: Vec<(Part, bool)> = Vec::new();
    for p in parts {
        match out.iter_mut().find(|(o, _)| o.color == p.color) {
            Some((o, joined)) => {
                let base = o.positions.len() as u32;
                o.positions.extend_from_slice(&p.positions);
                o.triangles.extend(
                    p.triangles
                        .iter()
                        .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
                );
                *joined = true;
            }
            None => out.push((p, false)),
        }
    }
    out.into_iter()
        .map(|(mut p, joined)| {
            if joined {
                p.name = if p.material.is_empty() {
                    "merged".into()
                } else {
                    p.material.clone()
                };
            }
            p
        })
        .collect()
}

/// Checks a point of the file after the frame: finite and within the range of the model.
pub(crate) fn model_point(
    frame: Frame,
    p: [f64; 3],
    limits: &Limits,
    what: &dyn Fn() -> String,
) -> Result<[f64; 3], MeshError> {
    if !p.iter().all(|v| v.is_finite()) {
        return Err(MeshError::NotFinite { what: what() });
    }
    let q = frame.to_model(p);
    let big = q.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    if !q.iter().all(|v| v.is_finite()) || big > limits.max_coord_mm {
        return Err(MeshError::OutOfRange {
            what: what(),
            value: big,
            limit: limits.max_coord_mm,
        });
    }
    Ok(q)
}
