//! `selection.get` and `selection.set` (AGR-09): the selected objects and, inside surface models,
//! the selected faces and edges.
//!
//! Whole objects are in `Session::selection`. The faces and edges are in `Session::parts`: a
//! plain set of ids, the same ids that `mesh.faces` and `mesh.edges` use. The 3D editor of the
//! modeler (`kadakas-modeler`) keeps its own set of picked parts and mirrors it into the session
//! each frame, and it takes a change that `selection.set` made. A host without such an editor
//! keeps the parts that `selection.set` gave and nothing more.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Map, Value, json};

use kadakas_core::model::{Document, ObjectId, Shape};

use crate::errors::bad;
use crate::mesh_api::{model_of, object_id, typed};
use crate::{RpcError, Session};

/// The most ids of faces and edges in one `selection.set`.
pub const PARTS_MAX: usize = 100_000;

/// The selected faces and edges of surface models, as ids. The model ids are object ids, the
/// face and edge ids are the ones of `mesh.faces` and `mesh.edges`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PartSelection {
    faces: BTreeMap<ObjectId, BTreeSet<u32>>,
    edges: BTreeMap<ObjectId, BTreeSet<u32>>,
}

impl PartSelection {
    /// A selection of the given faces and edges, as (model, id) pairs.
    pub fn new(
        faces: impl IntoIterator<Item = (ObjectId, u32)>,
        edges: impl IntoIterator<Item = (ObjectId, u32)>,
    ) -> Self {
        let mut s = PartSelection::default();
        for (m, f) in faces {
            s.faces.entry(m).or_default().insert(f);
        }
        for (m, e) in edges {
            s.edges.entry(m).or_default().insert(e);
        }
        s
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty() && self.edges.is_empty()
    }

    /// How many faces and edges are selected.
    pub fn len(&self) -> usize {
        self.faces
            .values()
            .chain(self.edges.values())
            .map(BTreeSet::len)
            .sum()
    }

    pub fn clear(&mut self) {
        self.faces.clear();
        self.edges.clear();
    }

    /// The selected faces as (model, face id) pairs, in order.
    pub fn faces(&self) -> impl Iterator<Item = (ObjectId, u32)> + '_ {
        self.faces
            .iter()
            .flat_map(|(m, set)| set.iter().map(move |f| (*m, *f)))
    }

    /// The selected edges as (model, edge id) pairs, in order.
    pub fn edges(&self) -> impl Iterator<Item = (ObjectId, u32)> + '_ {
        self.edges
            .iter()
            .flat_map(|(m, set)| set.iter().map(move |e| (*m, *e)))
    }

    /// Takes out what the document does not have (after a delete or an undo).
    pub fn retain_existing(&mut self, doc: &Document) {
        for (map, face) in [(&mut self.faces, true), (&mut self.edges, false)] {
            map.retain(|m, set| match doc.object(*m).map(|o| &o.shape) {
                Some(Shape::SurfaceModel(model)) => {
                    set.retain(|n| {
                        if face {
                            model.face(kadakas_core::surface::FaceId(*n)).is_some()
                        } else {
                            model.edge(kadakas_core::surface::EdgeId(*n)).is_some()
                        }
                    });
                    !set.is_empty()
                }
                _ => false,
            });
        }
    }

    /// `{faces: {model: [ids]}, edges: {model: [ids]}}`. A map is empty when nothing is selected.
    pub fn to_json(&self) -> Value {
        let map = |m: &BTreeMap<ObjectId, BTreeSet<u32>>| -> Value {
            Value::Object(
                m.iter()
                    .map(|(id, set)| (id.to_string(), json!(set)))
                    .collect::<Map<_, _>>(),
            )
        };
        json!({"faces": map(&self.faces), "edges": map(&self.edges)})
    }
}

/// The answer of `selection.get`: `{ids, faces, edges}`.
pub(crate) fn get(s: &Session) -> Value {
    // The parts are checked against the document now, so a face that an edit removed is not listed
    // even when the host did not prune yet.
    let mut out = s.parts_now().to_json();
    out["ids"] = json!(s.selection);
    out
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetParams {
    ids: Option<Vec<ObjectId>>,
    faces: Option<BTreeMap<String, Vec<u32>>>,
    edges: Option<BTreeMap<String, Vec<u32>>>,
}

/// Reads and checks `{model: [ids]}`: the models are surface models and the ids exist.
fn parse_parts(
    doc: &Document,
    what: &'static str,
    given: Option<BTreeMap<String, Vec<u32>>>,
) -> Result<BTreeMap<ObjectId, BTreeSet<u32>>, RpcError> {
    let mut out: BTreeMap<ObjectId, BTreeSet<u32>> = BTreeMap::new();
    for (text, list) in given.unwrap_or_default() {
        let id = object_id(&text)?;
        let model = model_of(doc, id)?;
        if doc.object(id).is_some_and(|o| doc.is_grey(o)) {
            return Err(RpcError::validation(format!(
                "object {id} is on a grey layer or in a grey class and cannot be selected"
            )));
        }
        let missing: Vec<u32> = list
            .iter()
            .copied()
            .filter(|n| {
                if what == "face" {
                    model.face(kadakas_core::surface::FaceId(*n)).is_none()
                } else {
                    model.edge(kadakas_core::surface::EdgeId(*n)).is_none()
                }
            })
            .take(20)
            .collect();
        if !missing.is_empty() {
            let error_id = if what == "face" {
                "no_such_face"
            } else {
                "no_such_edge"
            };
            return Err(typed(
                RpcError::not_found(format!(
                    "model {id} has no {what} {}: read the ids with mesh.{what}s",
                    missing
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                error_id,
                json!({"model": id, "missing": missing}),
            ));
        }
        if !list.is_empty() {
            out.entry(id).or_default().extend(list);
        }
    }
    Ok(out)
}

/// `selection.set {ids?, faces?, edges?}`: the new selection. A part that is left out is empty.
pub(crate) fn set(s: &mut Session, p: Value) -> Result<Value, RpcError> {
    let SetParams { ids, faces, edges } = crate::params(p)?;
    if ids.is_none() && faces.is_none() && edges.is_none() {
        return Err(bad(
            "selection.set needs ids, faces or edges: ids is a list of object ids, faces and edges are {model id: [ids]}",
        ));
    }
    let total: usize = [&faces, &edges]
        .iter()
        .filter_map(|m| m.as_ref())
        .map(|m| m.values().map(Vec::len).sum::<usize>())
        .sum();
    if total > PARTS_MAX {
        return Err(RpcError::limit(format!(
            "selection.set takes at most {PARTS_MAX} faces and edges, got {total}"
        )));
    }
    if !s.parts_supported
        && [&faces, &edges].iter().any(|m| {
            m.as_ref()
                .is_some_and(|m| m.values().any(|l| !l.is_empty()))
        })
    {
        return Err(typed(
            RpcError::validation(
                "this program has no way to show selected faces and edges, so it refuses them. The 3D modeler and kadakas serve take them",
            ),
            "parts_not_supported",
            json!({}),
        ));
    }
    let doc = s.editor.doc();
    let ids = ids.unwrap_or_default();
    if let Some(bad_id) = ids.iter().find(|id| doc.object(**id).is_none()) {
        return Err(RpcError::not_found(format!(
            "object {bad_id} does not exist"
        )));
    }
    if let Some(g) = ids
        .iter()
        .find(|id| doc.object(**id).is_some_and(|o| doc.is_grey(o)))
    {
        return Err(RpcError::validation(format!(
            "object {g} is on a grey layer or in a grey class and cannot be selected"
        )));
    }
    let faces = parse_parts(doc, "face", faces)?;
    let edges = parse_parts(doc, "edge", edges)?;
    let parts = PartSelection { faces, edges };
    if !parts.is_empty() && !ids.is_empty() {
        return Err(bad(
            "ids and faces or edges cannot be selected together: the editor selects whole objects or parts of surface models, never both",
        ));
    }
    s.selection = ids;
    s.parts = parts;
    s.parts_changed = true;
    Ok(get(s))
}
