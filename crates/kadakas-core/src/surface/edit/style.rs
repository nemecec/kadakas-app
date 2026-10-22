//! Soft, smooth and hidden edges (MOD-08).
//!
//! Three attributes of an edge, each `true` or absent:
//!
//! - `soft`: the edge is not drawn (like a hidden edge, but it is still offered to the
//!   inference). A soft edge between two coplanar faces does not merge them.
//! - `smooth`: the faces on the two sides shade as one surface (smooth vertex normals); the edge
//!   itself is drawn unless it is also soft.
//!
//! So a curved surface has edges that are soft and smooth: the lines go and the shading is round
//! (Soften Edges with "Smooth normals", the default). A soft edge that is not smooth hides the
//! line and keeps the crease in the shading, as in other modelers of this kind.
//! - `hidden`: the edge is not drawn and not offered to the inference or a window selection.
//!
//! Edges made by Push/Pull from a curve and the fold edges of Move are soft and smooth. An edge
//! with no flag is hard: the faces on its two sides shade with their own normals.

use super::*;

/// The attribute of a soft edge.
pub const ATTR_SOFT: &str = "soft";
/// The attribute of a smooth edge.
pub const ATTR_SMOOTH: &str = "smooth";
/// The attribute of the edges of a faceted curve (a polygon drawn as one curve): the new edges
/// that a tool makes between its sides stay creases, drawn and hard, as the corners of a polygon
/// are real corners. The edges of a circle or an arc have no such mark and give soft and smooth
/// edges.
pub const ATTR_FACETED: &str = "faceted";

/// True if a curve is faceted: one of its edges has the mark [`ATTR_FACETED`].
pub(in crate::surface) fn curve_faceted(m: &SurfaceModel, edges: &[EdgeId]) -> bool {
    edges
        .iter()
        .any(|e| flagged(m.edge_attrs.get(e), ATTR_FACETED))
}

/// Marks the edges as the sides of a faceted curve.
pub(in crate::surface) fn mark_faceted(m: &mut SurfaceModel, edges: &[EdgeId]) {
    for e in edges {
        m.edge_attrs
            .entry(*e)
            .or_default()
            .insert(ATTR_FACETED.into(), serde_json::Value::Bool(true));
    }
}

/// Sets or clears the flags of the given edges (hidden is Hide, `Edit::SetHidden`; `None` leaves a flag as it is).
pub(super) fn set_edge_style(
    m: &mut SurfaceModel,
    edges: &[EdgeId],
    soft: Option<bool>,
    smooth: Option<bool>,
) -> Result<(), SurfaceError> {
    let ids = existing_edges(m, edges)?;
    for e in ids {
        for (key, v) in [(ATTR_SOFT, soft), (ATTR_SMOOTH, smooth)] {
            if let Some(on) = v {
                set_flag(m, e, key, on);
            }
        }
    }
    Ok(())
}

/// Soften Edges: each given edge between exactly two faces whose angle is below `angle` degrees
/// gets `soft` (and `smooth` when `smooth` is true); one at or above the angle loses both. An edge
/// between coplanar faces (an angle of at most 0.01 degrees) is softened only with `coplanar`
/// and is otherwise left as it is. Edges with no face, one face or more than two are left as they
/// are.
pub(super) fn soften_edges(
    m: &mut SurfaceModel,
    edges: &[EdgeId],
    angle: f64,
    smooth: bool,
    coplanar: bool,
) -> Result<(), SurfaceError> {
    if !angle.is_finite() || !(0.0..=180.0).contains(&angle) {
        return Err(SurfaceError::Refused(
            "the angle of Soften Edges must be from 0 to 180 degrees".into(),
        ));
    }
    existing_edges(m, edges)?;
    for (e, soften) in soften_plan(m, edges, angle, coplanar) {
        set_flag(m, e, ATTR_SOFT, soften);
        set_flag(m, e, ATTR_SMOOTH, soften && smooth);
    }
    Ok(())
}

/// What Soften Edges would do (for a preview): each given edge that it changes, and whether it
/// becomes soft (true) or hard (false). Edges that do not exist, that do not have two faces, or
/// that are between coplanar faces without `coplanar` are left out.
pub fn soften_plan(
    m: &SurfaceModel,
    edges: &[EdgeId],
    angle: f64,
    coplanar: bool,
) -> Vec<(EdgeId, bool)> {
    let ids: BTreeSet<EdgeId> = edges
        .iter()
        .copied()
        .filter(|e| m.edges.contains_key(e))
        .collect();
    let t = super::super::topo::Topo::of(m);
    let mut out = Vec::new();
    for e in ids {
        let Some(a) = dihedral(m, &t, e) else {
            continue;
        };
        let flat = a <= 0.01;
        if flat && !coplanar {
            continue;
        }
        out.push((e, a < angle || (flat && coplanar)));
    }
    out
}

/// The angle in degrees between the two faces of an edge (0 for faces in one plane that agree
/// about their sides), or `None` when the edge does not have exactly two faces.
pub fn dihedral_of(m: &SurfaceModel, e: EdgeId) -> Option<f64> {
    let t = super::super::topo::Topo::of(m);
    dihedral(m, &t, e)
}

fn dihedral(m: &SurfaceModel, t: &super::super::topo::Topo, e: EdgeId) -> Option<f64> {
    let ed = m.edges.get(&e)?;
    let fs: Vec<FaceId> = t.faces_of(e).collect();
    let [f, g] = fs.as_slice() else {
        return None;
    };
    let (ff, gf) = (&m.faces[f], &m.faces[g]);
    let way = |x: &Face| x.uses().any(|u| u == (ed.v[0], ed.v[1]));
    // Faces that agree about their sides use the edge in opposite directions.
    let sign = if way(ff) == way(gf) { -1.0 } else { 1.0 };
    let c = (sign * ff.plane.normal.dot(gf.plane.normal)).clamp(-1.0, 1.0);
    Some(c.acos().to_degrees())
}

fn existing_edges(m: &SurfaceModel, ids: &[EdgeId]) -> Result<BTreeSet<EdgeId>, SurfaceError> {
    let ids: BTreeSet<EdgeId> = ids.iter().copied().collect();
    match ids.iter().find(|id| !m.edges.contains_key(id)) {
        Some(id) => Err(SurfaceError::NoEdge(*id)),
        None => Ok(ids),
    }
}

/// Sets the flag `key` of edge `e` to `true`, or removes it (and an empty map).
fn set_flag(m: &mut SurfaceModel, e: EdgeId, key: &str, on: bool) {
    if on {
        m.edge_attrs
            .entry(e)
            .or_default()
            .insert(key.into(), serde_json::Value::Bool(true));
    } else if let Some(a) = m.edge_attrs.get_mut(&e) {
        a.remove(key);
        if a.is_empty() {
            m.edge_attrs.remove(&e);
        }
    }
}

/// True if the edge attributes have the flag `key` set to `true`.
pub fn flagged(attrs: Option<&super::super::Attrs>, key: &str) -> bool {
    attrs
        .and_then(|a| a.get(key))
        .is_some_and(|v| v == &serde_json::Value::Bool(true))
}

/// The soft surfaces of a model (MOD-08): the sets of faces that are connected through soft
/// edges. A soft edge joins the faces on its sides (also two coplanar faces); a smooth edge that is
/// not soft and a hidden edge do not. A face with no soft edge is in no soft surface.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoftSurfaces {
    /// The surface of each face that has a soft edge (an index into `groups`).
    of: BTreeMap<FaceId, usize>,
    /// The faces of each surface, in id order, and the soft edges between them.
    groups: Vec<(Vec<FaceId>, Vec<EdgeId>)>,
}

impl SoftSurfaces {
    /// Finds the soft surfaces: one pass over the faces and the soft edges.
    pub fn of(m: &SurfaceModel) -> SoftSurfaces {
        let soft: BTreeMap<(VertexId, VertexId), EdgeId> = m
            .edges
            .iter()
            .filter(|(id, _)| m.edge_soft(**id))
            .map(|(id, e)| (e.key(), *id))
            .collect();
        if soft.is_empty() {
            return SoftSurfaces::default();
        }
        // The faces of each soft edge.
        let mut faces_of: BTreeMap<EdgeId, Vec<FaceId>> = BTreeMap::new();
        for (fid, f) in &m.faces {
            for (a, b) in f.uses() {
                if let Some(e) = soft.get(&key(a, b)) {
                    let v = faces_of.entry(*e).or_default();
                    if v.last() != Some(fid) {
                        v.push(*fid);
                    }
                }
            }
        }
        let mut parent: BTreeMap<FaceId, FaceId> = BTreeMap::new();
        fn root(p: &mut BTreeMap<FaceId, FaceId>, f: FaceId) -> FaceId {
            let mut x = f;
            while let Some(&y) = p.get(&x) {
                if y == x {
                    break;
                }
                x = y;
            }
            // Path halving is not needed for correctness; keep the trees short.
            let r = x;
            let mut x = f;
            while let Some(&y) = p.get(&x) {
                if y == x {
                    break;
                }
                p.insert(x, r);
                x = y;
            }
            r
        }
        for fs in faces_of.values() {
            for f in fs {
                parent.entry(*f).or_insert(*f);
            }
            for w in fs.windows(2) {
                let (a, b) = (root(&mut parent, w[0]), root(&mut parent, w[1]));
                if a != b {
                    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                    parent.insert(hi, lo);
                }
            }
        }
        let mut out = SoftSurfaces::default();
        let mut index: BTreeMap<FaceId, usize> = BTreeMap::new();
        let faces: Vec<FaceId> = parent.keys().copied().collect();
        for f in faces {
            let r = root(&mut parent, f);
            let n = out.groups.len();
            let i = *index.entry(r).or_insert(n);
            if i == n {
                out.groups.push((Vec::new(), Vec::new()));
            }
            out.groups[i].0.push(f);
            out.of.insert(f, i);
        }
        for (e, fs) in &faces_of {
            if let Some(i) = fs.first().and_then(|f| out.of.get(f)) {
                out.groups[*i].1.push(*e);
            }
        }
        out
    }

    /// The faces and the soft edges of the surface of `face`, or `None` when the face has no soft
    /// edge.
    pub fn surface(&self, face: FaceId) -> Option<(&[FaceId], &[EdgeId])> {
        let i = *self.of.get(&face)?;
        let (fs, es) = &self.groups[i];
        Some((fs, es))
    }

    /// The number of soft surfaces.
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// True if the model has no soft surface.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}
