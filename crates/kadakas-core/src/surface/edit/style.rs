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
/// The attribute of a hidden edge.
pub const ATTR_HIDDEN: &str = "hidden";

/// Sets or clears the flags of the given edges (`None` leaves a flag as it is).
pub(super) fn set_edge_style(
    m: &mut SurfaceModel,
    edges: &[EdgeId],
    soft: Option<bool>,
    smooth: Option<bool>,
    hidden: Option<bool>,
) -> Result<(), SurfaceError> {
    let ids = existing_edges(m, edges)?;
    for e in ids {
        for (key, v) in [
            (ATTR_SOFT, soft),
            (ATTR_SMOOTH, smooth),
            (ATTR_HIDDEN, hidden),
        ] {
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
    let ids = existing_edges(m, edges)?;
    let t = super::super::topo::Topo::of(m);
    for e in ids {
        let Some(a) = dihedral(m, &t, e) else {
            continue;
        };
        let flat = a <= 0.01;
        if flat && !coplanar {
            continue;
        }
        let soften = a < angle || (flat && coplanar);
        set_flag(m, e, ATTR_SOFT, soften);
        set_flag(m, e, ATTR_SMOOTH, soften && smooth);
    }
    Ok(())
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
