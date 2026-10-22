//! Hide and Unhide (MOD-05): the attribute `hidden` of faces and edges. A hidden face or edge
//! stays in the model. Views, picking and the inference leave it out, unless Hidden Geometry
//! shows it. Unlike a soft edge, a hidden edge does not join the faces on its sides: they stay
//! separate faces.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{EdgeId, FaceId, SurfaceError, SurfaceModel};

/// The name of the attribute.
pub const HIDDEN: &str = "hidden";

/// Sets (`hidden`) or clears the attribute on the faces and edges. All ids must exist, else the
/// model is not changed. An edge that carries only this attribute loses its attribute map.
pub(super) fn set_hidden(
    m: &mut SurfaceModel,
    faces: &[FaceId],
    edges: &[EdgeId],
    hidden: bool,
) -> Result<(), SurfaceError> {
    let faces: BTreeSet<FaceId> = faces.iter().copied().collect();
    let edges: BTreeSet<EdgeId> = edges.iter().copied().collect();
    if let Some(id) = faces.iter().find(|id| !m.faces.contains_key(id)) {
        return Err(SurfaceError::NoFace(*id));
    }
    if let Some(id) = edges.iter().find(|id| !m.edges.contains_key(id)) {
        return Err(SurfaceError::NoEdge(*id));
    }
    for id in faces {
        let f = m.faces.get_mut(&id).expect("checked");
        if hidden {
            f.attrs.insert(HIDDEN.into(), Value::Bool(true));
        } else {
            f.attrs.remove(HIDDEN);
        }
    }
    for id in edges {
        if hidden {
            m.edge_attrs
                .entry(id)
                .or_default()
                .insert(HIDDEN.into(), Value::Bool(true));
        } else if let Some(a) = m.edge_attrs.get_mut(&id) {
            a.remove(HIDDEN);
            if a.is_empty() {
                m.edge_attrs.remove(&id);
            }
        }
    }
    Ok(())
}
