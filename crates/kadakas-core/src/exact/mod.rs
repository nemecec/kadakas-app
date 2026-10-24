//! Exact solids (stage 1 of the hybrid exact and faceted model): the boundary representation
//! that a STEP file holds, kept next to the triangles of a `mesh3d` object.
//!
//! The triangles stay the source for drawing and for every tool. The payload ([`ExactSolid`])
//! says what the surfaces really are: the radius of a cylinder, the normal of a plane, the
//! curve of an edge. It has everything that is needed to cut the faces into triangles again at
//! another tolerance. All numbers are in millimeters, in the frame of the mesh (the vertices
//! of the `mesh3d` object), so a move or a turn of the object moves the payload too.
//!
//! The form is plain JSON (see `docs/exact-solids.md`). Fields that a later version adds are
//! kept in `extra`, so a program that does not know them writes them back.
//!
//! - [`ExactSolid::validate`] checks a payload that comes from a file before it is used.
//! - [`ExactSolid::transformed`] moves, turns and scales it, with a uniform scale only.
//! - [`ExactSolid::info`] lists the faces with their true sizes, for dimensioning.
//! - [`fingerprint`] says whether the triangles are still those that the payload belongs to.

pub mod curve;
pub mod nurbs;
pub mod surface;

use std::collections::BTreeMap;

use glam::{DAffine3, DVec3};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

pub use curve::Curve;
pub use surface::Surface;

/// The version of the payload form that this program writes.
pub const VERSION: u32 = 1;
/// The most faces, edges and vertices that a payload may hold.
pub const MAX_FACES: usize = 200_000;
pub const MAX_EDGES: usize = 600_000;
pub const MAX_VERTICES: usize = 600_000;
/// The largest payload of one object, by [`ExactSolid::approx_bytes`].
pub const MAX_OBJECT_BYTES: usize = 8 * 1024 * 1024;
/// The largest sum of the payloads in one document.
pub const MAX_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

/// A shell: the faces that point to it by `shell`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExactShell {
    /// True for a closed shell. An open shell is a piece of surface.
    pub closed: bool,
    /// For the shells of a solid: `Some(true)` for the outside, `Some(false)` for a void.
    /// The triangles of a shell are turned inside out if their volume has the other sign.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outward: Option<bool>,
}

/// An edge: a curve between two vertices. `same_sense` says that the edge runs from `start`
/// to `end` along the curve. A missing curve is a straight line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExactEdge {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Curve>,
    pub start: u32,
    pub end: u32,
    pub same_sense: bool,
}

/// One use of an edge in a loop, or one corner of a polygon.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoopItem {
    /// The edge, walked forward (from its start to its end) or backward.
    Edge { edge: u32, forward: bool },
    /// A corner of a polygon loop (a faceted face).
    Vertex { vertex: u32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExactLoop {
    /// True for the outer bound of the face.
    pub outer: bool,
    pub items: Vec<LoopItem>,
}

/// A face on a surface, with its loops. `flip` turns the face inside out (an oriented face or
/// shell). A missing surface is the plane through the loop (a face of a faceted solid).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExactFace {
    pub shell: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<Surface>,
    pub same_sense: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip: bool,
    pub loops: Vec<ExactLoop>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 4]>,
    /// The number of the entity in the STEP file, for the report.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub source: u64,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// The exact data of one `mesh3d` object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExactSolid {
    pub version: u32,
    /// The product of the STEP file that this comes from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub product: String,
    /// Millimeters in one length unit of the file (for the report).
    pub unit_mm: f64,
    /// The chord tolerance and the angle step of the triangles that were cut from this.
    pub tolerance_mm: f64,
    pub angle_deg: f64,
    pub vertices: Vec<DVec3>,
    pub edges: Vec<ExactEdge>,
    pub shells: Vec<ExactShell>,
    pub faces: Vec<ExactFace>,
    /// [`fingerprint`] of the triangles that this belongs to. Another value means that the
    /// triangles were edited since: the payload is stale.
    #[serde(default)]
    pub mesh_hash: u64,
    /// Fields of a later version.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Is a payload in step with the triangles?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Fresh,
    Stale,
}

/// About how many bytes the JSON of a value takes, found without writing it. The caps count a
/// payload that cannot be used (`Mesh3d::exact_bytes`) by this.
pub fn json_bytes(v: &Value) -> usize {
    match v {
        Value::Null => 4,
        Value::Bool(_) => 5,
        Value::Number(_) => 12,
        Value::String(s) => s.len() + 2,
        Value::Array(a) => 2 + a.iter().map(|x| json_bytes(x) + 1).sum::<usize>(),
        Value::Object(m) => {
            2 + m
                .iter()
                .map(|(k, x)| k.len() + 4 + json_bytes(x))
                .sum::<usize>()
        }
    }
}

/// A hash of the vertices and triangles (FNV-1a over the bits).
pub fn fingerprint(vertices: &[DVec3], triangles: &[[u32; 3]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |x: u64| {
        h ^= x;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    mix(vertices.len() as u64);
    for v in vertices {
        mix(v.x.to_bits());
        mix(v.y.to_bits());
        mix(v.z.to_bits());
    }
    mix(triangles.len() as u64);
    for t in triangles {
        mix(u64::from(t[0]) | u64::from(t[1]) << 21 | u64::from(t[2]) << 42);
    }
    h
}

/// A map with a uniform scale: the parts that the transform of a payload needs.
#[derive(Clone, Copy, Debug)]
pub struct Similarity {
    pub map: DAffine3,
    /// The factor of length.
    pub scale: f64,
    /// True when the map reflects (it turns right-handed into left-handed).
    pub mirror: bool,
}

impl Similarity {
    /// The parts of a map, or `None` when it does not keep angles (a stretch or a shear) or
    /// flattens.
    pub fn of(map: &DAffine3) -> Option<Similarity> {
        let m = map.matrix3;
        let (x, y, z) = (m.x_axis, m.y_axis, m.z_axis);
        let s = x.length();
        if !(s.is_finite() && s > 1e-12 && map.translation.is_finite()) {
            return None;
        }
        let tol = 1e-9 * s;
        let ok = (y.length() - s).abs() <= tol
            && (z.length() - s).abs() <= tol
            && x.dot(y).abs() <= tol * s
            && y.dot(z).abs() <= tol * s
            && x.dot(z).abs() <= tol * s;
        ok.then(|| Similarity {
            map: *map,
            scale: s,
            mirror: m.determinant() < 0.0,
        })
    }

    pub fn point(&self, p: DVec3) -> DVec3 {
        self.map.transform_point3(p)
    }

    /// A direction with its length kept (times the scale).
    pub fn vector(&self, v: DVec3) -> DVec3 {
        self.map.transform_vector3(v)
    }

    /// A frame (unit axes) moved by the map. The scale goes to the sizes, not to the axes.
    pub fn frame(&self, f: &DAffine3) -> DAffine3 {
        let m = self.map * *f;
        DAffine3 {
            matrix3: m.matrix3 * (1.0 / self.scale),
            translation: m.translation,
        }
    }
}

/// Why a payload cannot be used.
fn bad<T>(what: impl Into<String>) -> Result<T, String> {
    Err(what.into())
}

impl ExactSolid {
    /// A rough size in bytes of the JSON form, from the counts. The caps use it.
    pub fn approx_bytes(&self) -> usize {
        let mut n = 300 + self.vertices.len() * 56 + self.shells.len() * 30;
        for e in &self.edges {
            n += 60 + e.curve.as_ref().map_or(0, curve_bytes);
        }
        for f in &self.faces {
            n += 120 + f.surface.as_ref().map_or(0, surface_bytes);
            n += f
                .loops
                .iter()
                .map(|l| 30 + l.items.len() * 40)
                .sum::<usize>();
        }
        n
    }

    /// Reads a payload from JSON. A payload of a newer version, with a kind that this program
    /// does not know, or with damaged data is an error with a reason, never a panic.
    pub fn from_json(v: &Value) -> Result<ExactSolid, String> {
        let version = v.get("version").and_then(Value::as_u64).unwrap_or(0);
        if version > u64::from(VERSION) {
            return Err(format!(
                "the data has version {version}, this program reads up to version {VERSION}"
            ));
        }
        let e =
            ExactSolid::deserialize(v).map_err(|e| format!("the data is not understood: {e}"))?;
        e.validate()
            .map_err(|e| format!("the data is not valid: {e}"))?;
        Ok(e)
    }

    /// Checks a payload from an untrusted file: counts, indices, finite numbers and the
    /// invariants that the evaluation of the curves and surfaces relies on.
    pub fn validate(&self) -> Result<(), String> {
        if self.version == 0 || self.version > VERSION {
            return bad(format!("exact data version {} is not known", self.version));
        }
        if self.faces.len() > MAX_FACES
            || self.edges.len() > MAX_EDGES
            || self.vertices.len() > MAX_VERTICES
        {
            return bad("the exact data is too large");
        }
        if self.approx_bytes() > MAX_OBJECT_BYTES {
            return bad("the exact data is larger than the limit of one object");
        }
        if !(self.unit_mm.is_finite()
            && self.tolerance_mm.is_finite()
            && self.angle_deg.is_finite())
            || self.vertices.iter().any(|v| !v.is_finite())
        {
            return bad("the exact data has a number that is not finite");
        }
        let nv = self.vertices.len() as u32;
        for e in &self.edges {
            if e.start >= nv || e.end >= nv {
                return bad("an edge names a vertex that does not exist");
            }
            if let Some(c) = &e.curve {
                c.validate()?;
            }
        }
        let ne = self.edges.len() as u32;
        for f in &self.faces {
            if f.shell as usize >= self.shells.len() {
                return bad("a face names a shell that does not exist");
            }
            if let Some(s) = &f.surface {
                s.validate()?;
            }
            for l in &f.loops {
                for it in &l.items {
                    match it {
                        LoopItem::Edge { edge, .. } if *edge >= ne => {
                            return bad("a loop names an edge that does not exist");
                        }
                        LoopItem::Vertex { vertex } if *vertex >= nv => {
                            return bad("a loop names a vertex that does not exist");
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    /// The payload moved by `map`. `Err` when the map is not a similarity (non-uniform scale,
    /// shear) because the exact surfaces cannot follow it; the caller drops the payload then.
    pub fn transformed(&self, map: &DAffine3) -> Result<ExactSolid, String> {
        let Some(sim) = Similarity::of(map) else {
            return bad("the map is not a move, a turn, a mirror and a uniform scale");
        };
        let mut out = self.clone();
        out.vertices = self.vertices.iter().map(|v| sim.point(*v)).collect();
        for e in &mut out.edges {
            e.curve = e.curve.as_ref().map(|c| c.transformed(&sim));
        }
        for f in &mut out.faces {
            f.surface = f.surface.as_ref().map(|s| s.transformed(&sim));
            if sim.mirror {
                // A mirror turns the sense of every angle. The normal `dP/du x dP/dv` of a
                // surface that keeps its parameters turns around, so the face turns with it. A
                // face with no surface has the plane of its loop, and that follows the loop.
                // The loops run the other way round: the inside of a face is on the left of
                // its loops, seen from the normal, and a mirror changes the left into the right.
                if f.surface.is_some() {
                    f.same_sense = !f.same_sense;
                }
                for l in &mut f.loops {
                    l.items.reverse();
                    for it in &mut l.items {
                        if let LoopItem::Edge { forward, .. } = it {
                            *forward = !*forward;
                        }
                    }
                }
            }
        }
        out.unit_mm = self.unit_mm;
        out.tolerance_mm = self.tolerance_mm * sim.scale;
        Ok(out)
    }

    /// Adds the shells, faces and edges of another payload. The numbers of the other are moved.
    pub fn merge(&mut self, other: &ExactSolid) {
        let (nv, ne, ns) = (
            self.vertices.len() as u32,
            self.edges.len() as u32,
            self.shells.len() as u32,
        );
        self.vertices.extend_from_slice(&other.vertices);
        self.edges.extend(other.edges.iter().map(|e| ExactEdge {
            start: e.start + nv,
            end: e.end + nv,
            ..e.clone()
        }));
        self.shells.extend(other.shells.iter().cloned());
        self.faces.extend(other.faces.iter().map(|f| {
            let mut f = f.clone();
            f.shell += ns;
            for l in &mut f.loops {
                for it in &mut l.items {
                    match it {
                        LoopItem::Edge { edge, .. } => *edge += ne,
                        LoopItem::Vertex { vertex } => *vertex += nv,
                    }
                }
            }
            f
        }));
        if self.product.is_empty() {
            self.product = other.product.clone();
        }
    }

    /// Keeps the faces for which `keep` is true, and the edges, vertices and shells that they
    /// use. Everything is numbered again.
    pub fn retain_faces(&mut self, keep: impl Fn(&ExactFace) -> bool) {
        self.faces.retain(|f| keep(f));
        let mut vmap: Vec<Option<u32>> = vec![None; self.vertices.len()];
        let mut emap: Vec<Option<u32>> = vec![None; self.edges.len()];
        let mut smap: Vec<Option<u32>> = vec![None; self.shells.len()];
        let (mut vertices, mut edges, mut shells) = (Vec::new(), Vec::new(), Vec::new());
        let use_vertex = |v: u32,
                          vmap: &mut Vec<Option<u32>>,
                          vertices: &mut Vec<DVec3>,
                          all: &[DVec3]|
         -> u32 {
            *vmap[v as usize].get_or_insert_with(|| {
                vertices.push(all[v as usize]);
                (vertices.len() - 1) as u32
            })
        };
        for f in &mut self.faces {
            f.shell = *smap[f.shell as usize].get_or_insert_with(|| {
                shells.push(self.shells[f.shell as usize].clone());
                (shells.len() - 1) as u32
            });
            for l in &mut f.loops {
                for it in &mut l.items {
                    match it {
                        LoopItem::Vertex { vertex } => {
                            *vertex = use_vertex(*vertex, &mut vmap, &mut vertices, &self.vertices);
                        }
                        LoopItem::Edge { edge, .. } => {
                            *edge = match emap[*edge as usize] {
                                Some(e) => e,
                                None => {
                                    let old = &self.edges[*edge as usize];
                                    let start = use_vertex(
                                        old.start,
                                        &mut vmap,
                                        &mut vertices,
                                        &self.vertices,
                                    );
                                    let end = use_vertex(
                                        old.end,
                                        &mut vmap,
                                        &mut vertices,
                                        &self.vertices,
                                    );
                                    edges.push(ExactEdge {
                                        start,
                                        end,
                                        ..old.clone()
                                    });
                                    let n = (edges.len() - 1) as u32;
                                    emap[*edge as usize] = Some(n);
                                    n
                                }
                            };
                        }
                    }
                }
            }
        }
        self.vertices = vertices;
        self.edges = edges;
        self.shells = shells;
    }

    /// The faces by kind of surface.
    pub fn counts(&self) -> BTreeMap<&'static str, usize> {
        let mut m = BTreeMap::new();
        for f in &self.faces {
            *m.entry(f.surface.as_ref().map_or("plane", Surface::short_kind))
                .or_default() += 1;
        }
        m
    }

    /// A sentence for Object Info: `Exact data kept: 12 faces (plane x 8, cylinder x 4)`.
    pub fn summary(&self) -> String {
        let parts: Vec<String> = self
            .counts()
            .iter()
            .map(|(k, n)| format!("{k} x {n}"))
            .collect();
        format!(
            "Exact data kept: {} faces ({})",
            self.faces.len(),
            parts.join(", ")
        )
    }

    /// The true sizes for dimensioning: for each face (up to `limit`) its kind and the
    /// radius, axis, normal and so on in millimeters. A plane gives its normal and a point.
    pub fn info(&self, limit: usize) -> Value {
        let faces: Vec<Value> = self
            .faces
            .iter()
            .enumerate()
            .take(limit)
            .map(|(i, f)| {
                let mut o = f
                    .surface
                    .as_ref()
                    .map_or_else(|| self.polygon_plane(f), Surface::describe);
                if let Value::Object(m) = &mut o {
                    m.insert("face".into(), json!(i));
                    // The face normal is the normal of the surface, or the opposite one.
                    let outward = f.same_sense != f.flip;
                    // The face looks along the normal of the surface (outward) or against it,
                    // and the normal of the surface turns with the handedness of the frame.
                    let handed = f.surface.as_ref().and_then(Surface::natural_outward);
                    if m.get("kind") == Some(&json!("plane")) {
                        if !outward && let Some(n) = m.get("normal").and_then(|n| n.as_array()) {
                            let neg: Vec<Value> = n
                                .iter()
                                .map(|x| json!(-x.as_f64().unwrap_or(0.0)))
                                .collect();
                            m.insert("normal".into(), Value::Array(neg));
                        }
                    } else if handed.is_some_and(|h| outward != h) {
                        // A hole: the face looks into the surface (a cylinder bore).
                        m.insert("concave".into(), json!(true));
                    }
                    if let Some(c) = f.color {
                        m.insert("color".into(), json!(c));
                    }
                }
                o
            })
            .collect();
        json!({
            "product": self.product,
            "faces": self.faces.len(),
            "by_kind": self.counts(),
            "tolerance_mm": self.tolerance_mm,
            "listed": faces.len(),
            "list": faces,
        })
    }

    /// The plane of a face that has no surface: the Newell normal of its outer loop.
    fn polygon_plane(&self, f: &ExactFace) -> Value {
        let pts: Vec<DVec3> = f
            .loops
            .iter()
            .find(|l| l.outer)
            .or(f.loops.first())
            .map(|l| {
                l.items
                    .iter()
                    .filter_map(|it| match it {
                        LoopItem::Vertex { vertex } => self.vertices.get(*vertex as usize).copied(),
                        LoopItem::Edge { edge, forward } => {
                            let e = self.edges.get(*edge as usize)?;
                            let v = if *forward { e.start } else { e.end };
                            self.vertices.get(v as usize).copied()
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut n = DVec3::ZERO;
        let mut c = DVec3::ZERO;
        for i in 0..pts.len() {
            n += pts[i].cross(pts[(i + 1) % pts.len()]);
            c += pts[i];
        }
        let len = n.length();
        if len > 0.0 && !pts.is_empty() {
            json!({"kind": "plane", "point": (c / pts.len() as f64).to_array(), "normal": (n / len).to_array()})
        } else {
            json!({"kind": "plane"})
        }
    }
}

fn curve_bytes(c: &Curve) -> usize {
    match c {
        Curve::Spline(s) => 100 + s.points.len() * 60 + s.knots.values.len() * 22,
        Curve::Poly { points } => 40 + points.len() * 56,
        Curve::Trimmed { basis, .. } => 60 + curve_bytes(basis),
        Curve::Composite { segments } => {
            40 + segments
                .iter()
                .map(|s| 20 + curve_bytes(&s.0))
                .sum::<usize>()
        }
        _ => 150,
    }
}

fn surface_bytes(s: &Surface) -> usize {
    match s {
        Surface::Spline(b) => {
            200 + b.points.len() * 60
                + b.weights.as_ref().map_or(0, |w| w.len() * 22)
                + (b.u.values.len() + b.v.values.len()) * 22
        }
        Surface::Revolution { curve, .. } | Surface::Extrusion { curve, .. } => {
            150 + curve_bytes(curve)
        }
        _ => 250,
    }
}

#[cfg(test)]
mod tests;
