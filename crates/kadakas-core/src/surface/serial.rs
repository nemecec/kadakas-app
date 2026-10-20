//! The saved form of a surface model (version [`FORMAT`]):
//!
//! ```json
//! {"kind": "surface_model", "v": 1, "name": "Shed", "next_id": 9,
//!  "transform": [1,0,0, 0,1,0, 0,0,1, 500,0,0],
//!  "vertices": [[1, 0, 0, 0], [2, 1000, 0, 0], ...],
//!  "edges": [[5, 1, 2], ...],
//!  "faces": [{"id": 8, "normal": [0, 0, 1], "offset": 0, "outer": [1, 2, 3, 4], "holes": [[...]]}]}
//! ```
//!
//! A vertex is `[id, x, y, z]`, an edge `[id, first vertex, second vertex]`. `transform` is the
//! three columns of the matrix and then the move (left out for the identity). Everything is in id
//! order and each loop starts at its smallest vertex id, so the same model gives the same text.
//! A newer version (a higher `v`) is kept as it is by `Shape::Unknown` and cannot be edited.

use std::collections::BTreeMap;

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{Edge, EdgeId, FORMAT, Face, FaceId, Plane, SurfaceModel, VertexId};

#[derive(Serialize, Deserialize)]
struct Raw {
    v: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transform: Option<[f64; 12]>,
    next_id: u32,
    #[serde(default)]
    vertices: Vec<(u32, f64, f64, f64)>,
    #[serde(default)]
    edges: Vec<(u32, u32, u32)>,
    #[serde(default)]
    faces: Vec<RawFace>,
}

#[derive(Serialize, Deserialize)]
struct RawFace {
    id: u32,
    normal: [f64; 3],
    offset: f64,
    outer: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    holes: Vec<Vec<u32>>,
}

fn ids(l: &[VertexId]) -> Vec<u32> {
    l.iter().map(|v| v.0).collect()
}

fn vids(l: &[u32]) -> Vec<VertexId> {
    l.iter().map(|v| VertexId(*v)).collect()
}

impl Serialize for SurfaceModel {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        Raw {
            v: FORMAT,
            name: self.name.clone(),
            transform: (self.transform != DAffine3::IDENTITY)
                .then(|| self.transform.to_cols_array()),
            next_id: self.next_id,
            vertices: self
                .vertices
                .iter()
                .map(|(id, p)| (id.0, p.x, p.y, p.z))
                .collect(),
            edges: self
                .edges
                .iter()
                .map(|(id, e)| (id.0, e.v[0].0, e.v[1].0))
                .collect(),
            faces: self
                .faces
                .iter()
                .map(|(id, f)| RawFace {
                    id: id.0,
                    normal: f.plane.normal.to_array(),
                    offset: f.plane.offset,
                    outer: ids(&f.outer),
                    holes: f.holes.iter().map(|h| ids(h)).collect(),
                })
                .collect(),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for SurfaceModel {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let raw = Raw::deserialize(d)?;
        if raw.v == 0 || raw.v > FORMAT {
            return Err(D::Error::custom(format!(
                "surface model version {} is not supported (this program reads 1 to {FORMAT})",
                raw.v
            )));
        }
        let twice = |what: &str, id: u32| D::Error::custom(format!("{what} id {id} is used twice"));
        let mut vertices = BTreeMap::new();
        for (id, x, y, z) in raw.vertices {
            if vertices.insert(VertexId(id), DVec3::new(x, y, z)).is_some() {
                return Err(twice("vertex", id));
            }
        }
        let mut edges = BTreeMap::new();
        for (id, a, b) in raw.edges {
            let e = Edge {
                v: [VertexId(a), VertexId(b)],
            };
            if edges.insert(EdgeId(id), e).is_some() {
                return Err(twice("edge", id));
            }
        }
        let mut faces = BTreeMap::new();
        for f in raw.faces {
            let face = Face {
                plane: Plane {
                    normal: DVec3::from(f.normal),
                    offset: f.offset,
                },
                outer: vids(&f.outer),
                holes: f.holes.iter().map(|h| vids(h)).collect(),
            };
            if faces.insert(FaceId(f.id), face).is_some() {
                return Err(twice("face", f.id));
            }
        }
        let mut m = SurfaceModel::from_parts(vertices, edges, faces, raw.next_id);
        m.name = raw.name;
        if let Some(t) = raw.transform {
            m.transform = DAffine3::from_cols_array(&t);
        }
        Ok(m)
    }
}
