//! The saved form of a surface model (version 1, or 2 when it has circle descriptions, see
//! [`FORMAT`]):
//!
//! ```json
//! {"kind": "surface_model", "v": 1, "name": "Shed", "next_id": 9,
//!  "transform": [1,0,0, 0,1,0, 0,0,1, 500,0,0],
//!  "vertices": [[1, 0, 0, 0], [2, 1000, 0, 0], ...],
//!  "edges": [[5, 1, 2], ...],
//!  "faces": [{"id": 8, "normal": [0, 0, 1], "offset": 0, "outer": [1, 2, 3, 4], "holes": [[...]]}]}
//! ```
//!
//! A vertex is `[id, x, y, z]`, an edge `[id, first vertex, second vertex]` or, with attributes,
//! `[id, a, b, {attrs}]`. A face may have `attrs`. `curves` is a list of `[id, edge, edge, ...]`.
//! `curve_geometry` (MOD-15, optional) is a list of `[curve id, {kind, center, normal, axis,
//! radius, start?, end?}]`: the exact circle of a curve (see `analytic`). A row that this
//! version cannot read is kept and written back while its chain is as it was. A model with
//! `curve_geometry`, or with a log entry of segments or of another version, is written as `v` 2.
//! `rev` counts the edits and `log` holds the recent ones (see `sync`). `transform` is the three
//! columns of the matrix and then the move (left out for the identity). Everything is in id order
//! and each loop starts at its smallest vertex id, so the same model gives the same text.
//!
//! Room to grow without a new version: attributes of edges and faces are free maps, and fields of
//! the model and of a face that this version does not know are kept and written back. A change
//! that an older program must not read raises `v`; such a shape is kept as it is by
//! `Shape::Unknown` and cannot be edited.

use std::collections::BTreeMap;

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use serde_json::Value;

use super::sync::Entry;
use super::{
    Analytic, Attrs, CurveId, Edge, EdgeId, FORMAT, Face, FaceId, Plane, SurfaceModel, VertexId,
};

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
    edges: Vec<Vec<Value>>,
    #[serde(default)]
    faces: Vec<RawFace>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    curves: Vec<Vec<u32>>,
    /// The exact circles of some curves (MOD-15): `[curve id, {description}]`. A row that this
    /// version cannot read is left out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    curve_geometry: Vec<(u32, Value)>,
    #[serde(default, skip_serializing_if = "is_zero")]
    rev: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    log: Vec<Entry>,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

#[derive(Serialize, Deserialize)]
struct RawFace {
    id: u32,
    normal: [f64; 3],
    offset: f64,
    outer: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    holes: Vec<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Attrs::is_empty")]
    attrs: Attrs,
    #[serde(flatten)]
    extra: Attrs,
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
            v: self.format_needed(),
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
                .map(|(id, e)| {
                    let mut row = vec![
                        Value::from(id.0),
                        Value::from(e.v[0].0),
                        Value::from(e.v[1].0),
                    ];
                    if let Some(a) = self.edge_attrs.get(id).filter(|a| !a.is_empty()) {
                        row.push(Value::Object(a.clone().into_iter().collect()));
                    }
                    row
                })
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
                    attrs: f.attrs.clone(),
                    extra: f.extra.clone(),
                })
                .collect(),
            curves: self
                .curves
                .iter()
                .map(|(id, es)| {
                    std::iter::once(id.0)
                        .chain(es.iter().map(|e| e.0))
                        .collect()
                })
                .collect(),
            curve_geometry: {
                let mut rows: Vec<(u32, Value)> = self
                    .analytic
                    .iter()
                    .filter_map(|(id, a)| Some((id.0, serde_json::to_value(a).ok()?)))
                    .collect();
                rows.extend(
                    self.analytic_raw
                        .iter()
                        .map(|(id, r)| (id.0, r.value.clone())),
                );
                rows.sort_by_key(|r| r.0);
                rows
            },
            rev: self.rev,
            log: self.log.clone(),
            extra: self.extra.clone(),
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
        let mut edge_attrs = BTreeMap::new();
        let num = |v: &Value| v.as_u64().and_then(|n| u32::try_from(n).ok());
        for row in raw.edges {
            let bad = || {
                D::Error::custom("an edge is [id, vertex, vertex] or [id, vertex, vertex, {attrs}]")
            };
            let (Some(id), Some(a), Some(b)) = (
                row.first().and_then(num),
                row.get(1).and_then(num),
                row.get(2).and_then(num),
            ) else {
                return Err(bad());
            };
            let e = Edge {
                v: [VertexId(a), VertexId(b)],
            };
            if edges.insert(EdgeId(id), e).is_some() {
                return Err(twice("edge", id));
            }
            match &row[3..] {
                [] => {}
                [Value::Object(m)] => {
                    if !m.is_empty() {
                        edge_attrs.insert(EdgeId(id), m.clone().into_iter().collect::<Attrs>());
                    }
                }
                _ => return Err(bad()),
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
                attrs: f.attrs,
                extra: f.extra,
            };
            if faces.insert(FaceId(f.id), face).is_some() {
                return Err(twice("face", f.id));
            }
        }
        let mut curves = BTreeMap::new();
        for c in raw.curves {
            let Some((&id, es)) = c.split_first() else {
                return Err(D::Error::custom("a curve is [id, edge, ...]"));
            };
            if curves
                .insert(CurveId(id), es.iter().map(|e| EdgeId(*e)).collect())
                .is_some()
            {
                return Err(twice("curve", id));
            }
        }
        let mut m = SurfaceModel::from_parts(vertices, edges, faces, raw.next_id);
        m.edge_attrs = edge_attrs;
        m.curves = curves;
        let mut unread = Vec::new();
        for (id, v) in raw.curve_geometry {
            // A description that this version cannot read stays as it is (see `analytic`).
            match serde_json::from_value::<Analytic>(v.clone()) {
                Ok(a) => {
                    if m.analytic.insert(CurveId(id), a).is_some() {
                        m.load_notes.push(format!(
                            "the file has two circle descriptions for curve {id}: the later one is used"
                        ));
                    }
                }
                Err(_) => unread.push((id, v)),
            }
        }
        super::analytic::validate_loaded(&mut m, unread);
        m.rev = raw.rev;
        m.log = raw.log;
        for e in &m.log {
            if let super::sync::Intent::Unknown(v) = &e.intent {
                // Written by another version of the program, or damaged: kept either way.
                let other_version = serde_json::from_value::<super::sync::Intent>(v.clone())
                    .err()
                    .is_some_and(|err| err.to_string().starts_with("unknown variant"));
                m.load_notes.push(if other_version {
                    format!(
                        "edit {} of the log was written by another version of the program: it is kept as it is and not replayed",
                        e.n
                    )
                } else {
                    format!(
                        "edit {} of the log is damaged (a field does not fit): it is kept as it is and not replayed",
                        e.n
                    )
                });
            }
        }
        m.extra = raw.extra;
        m.name = raw.name;
        if let Some(t) = raw.transform {
            m.transform = DAffine3::from_cols_array(&t);
        }
        Ok(m)
    }
}
