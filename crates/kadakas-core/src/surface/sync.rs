//! Edits as intents that can be replayed, so that two users who edit one surface model at the same
//! time both keep their work.
//!
//! In collaboration the whole model is one property (`shape`), and the last writer wins. Without
//! more, an edit that another user made at the same time would vanish. So the model keeps a short
//! log: each edit is an [`Entry`] with a `stamp` (a hash of the entry and of the stamp before it,
//! so a stamp names one edit after one history) and its intent in a form that does not depend on
//! ids:
//!
//! - added edges by their coordinates;
//! - erased edges by the middle point of each edge, faces (erased or reversed) by a point inside
//!   each face, found again by position in the other model;
//! - a new placement (transform).
//!
//! **Rule.** A client that receives a model from another user checks whether the model it had
//! holds entries that this client made and that the incoming model lacks (its work would be lost).
//! Only then, and only the author, replays those entries on the incoming model ([`repair`]) and
//! sends the result as a new edit. The other user's work is in the incoming model and the author's
//! work comes back on top, so both stay; no one else repairs, so two clients never send repairs
//! back and forth. If a replay fails (the edge or face is gone, or the change was no edit of the
//! log, such as an undo or a whole new model), nothing is dropped silently: the caller gets
//! [`Repair::Conflict`] with the model that would be lost, and shows it to the person.
//!
//! The log holds the last [`LOG_ENTRIES`] edits and at most [`LOG_POINTS`] points; older entries
//! go. A replay that needs an entry that is gone is a conflict. Full sync of intents (each edit as
//! its own operation) is a later row.

use std::collections::BTreeSet;

use glam::{DAffine3, DVec2, DVec3};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::arrange::{inside, sample_rings};
use super::geom::{self, on_plane};
use super::{EdgeId, Edit, FaceId, IntersectScope, SurfaceError, SurfaceModel};

/// Most entries in the log of a model.
pub const LOG_ENTRIES: usize = 64;
/// Most points in all entries of the log (about 1.5 MB of text).
pub const LOG_POINTS: usize = 20_000;

/// What an edit did, without ids.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Intent {
    /// Edges drawn, by their end points.
    AddEdges { edges: Vec<[[f64; 3]; 2]> },
    /// Edges erased, by their middle points.
    EraseEdgesAt { points: Vec<[f64; 3]> },
    /// Faces erased, by a point inside each.
    EraseFacesAt { points: Vec<[f64; 3]> },
    /// Faces reversed, by a point inside each.
    ReverseFacesAt { points: Vec<[f64; 3]> },
    /// A curve (circle or arc) drawn as a chain of edges, by its corner points.
    AddCurve { points: Vec<[f64; 3]>, closed: bool },
    /// A face pushed or pulled along its normal, by a point inside it.
    PushPullAt { point: [f64; 3], distance: f64 },
    /// Intersect Faces on the whole model.
    IntersectModel,
    /// Intersect Faces on chosen faces and edges, by a point inside each face and the middle of
    /// each edge.
    IntersectAt {
        faces: Vec<[f64; 3]>,
        edges: Vec<[f64; 3]>,
    },
    /// Vertices moved by an affine map, by their positions before the move.
    TransformAt {
        vertices: Vec<[f64; 3]>,
        matrix: [f64; 12],
    },
    /// Faces (by a point inside each) and edges (by the middle of each) copied `count` times.
    CopyAt {
        faces: Vec<[f64; 3]>,
        edges: Vec<[f64; 3]>,
        matrix: [f64; 12],
        count: u32,
    },
    /// A new transform.
    Place { transform: [f64; 12] },
    /// A change that cannot be replayed (for example a new curve).
    Unlogged,
}

impl Intent {
    /// The intent of an edit, read from the model before the edit.
    pub(super) fn of(m: &SurfaceModel, edit: &Edit) -> Intent {
        let faces = |ids: &[FaceId]| -> Vec<[f64; 3]> {
            unique(ids)
                .filter_map(|id| face_point(m, id))
                .map(|p| p.to_array())
                .collect()
        };
        let middles = |ids: &[EdgeId]| -> Vec<[f64; 3]> {
            unique(ids)
                .filter_map(|id| m.edges.get(&id))
                .map(|e| ((m.vertices[&e.v[0]] + m.vertices[&e.v[1]]) / 2.0).to_array())
                .collect()
        };
        match edit {
            Edit::AddEdges { edges } => Intent::AddEdges {
                edges: edges.clone(),
            },
            Edit::EraseEdges { edges } => Intent::EraseEdgesAt {
                points: middles(edges),
            },
            Edit::IntersectFaces {
                scope,
                faces: fs,
                edges,
            } => match scope {
                IntersectScope::Model => Intent::IntersectModel,
                IntersectScope::Selection => Intent::IntersectAt {
                    faces: faces(fs),
                    edges: middles(edges),
                },
            },
            Edit::EraseFaces { faces: ids } => Intent::EraseFacesAt { points: faces(ids) },
            Edit::ReverseFaces { faces: ids } => Intent::ReverseFacesAt { points: faces(ids) },
            Edit::AddCurve { points, closed } => Intent::AddCurve {
                points: points.clone(),
                closed: *closed,
            },
            Edit::TransformVertices {
                vertices,
                faces: fs,
                edges,
                matrix,
            } => {
                let mut ids: BTreeSet<super::VertexId> = vertices.iter().copied().collect();
                for f in fs {
                    if let Some(f) = m.faces.get(f) {
                        ids.extend(f.loops().flatten().copied());
                    }
                }
                for e in edges {
                    if let Some(e) = m.edges.get(e) {
                        ids.extend(e.v);
                    }
                }
                Intent::TransformAt {
                    vertices: ids
                        .iter()
                        .filter_map(|v| m.vertices.get(v))
                        .map(|p| p.to_array())
                        .collect(),
                    matrix: *matrix,
                }
            }
            Edit::CopyGeometry {
                faces: fs,
                edges,
                matrix,
                count,
            } => Intent::CopyAt {
                faces: faces(fs),
                edges: middles(edges),
                matrix: *matrix,
                count: *count,
            },
            Edit::PushPull { face, distance } => match face_point(m, *face) {
                Some(p) => Intent::PushPullAt {
                    point: p.to_array(),
                    distance: *distance,
                },
                None => Intent::Unlogged,
            },
        }
    }

    fn points(&self) -> usize {
        match self {
            Intent::AddEdges { edges } => 2 * edges.len(),
            Intent::EraseEdgesAt { points }
            | Intent::EraseFacesAt { points }
            | Intent::ReverseFacesAt { points } => points.len(),
            Intent::AddCurve { points, .. } => points.len(),
            Intent::TransformAt { vertices, .. } => vertices.len().max(1),
            Intent::CopyAt { faces, edges, .. } => (faces.len() + edges.len()).max(1),
            Intent::IntersectAt { faces, edges } => (faces.len() + edges.len()).max(1),
            Intent::Place { .. }
            | Intent::Unlogged
            | Intent::PushPullAt { .. }
            | Intent::IntersectModel => 1,
        }
    }
}

fn unique<T: Ord + Copy>(ids: &[T]) -> impl Iterator<Item = T> {
    ids.iter().copied().collect::<BTreeSet<T>>().into_iter()
}

/// One edit in the log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Names this edit after this history.
    pub stamp: u64,
    /// The stamp of the entry before (or of the content of the model when the log was empty).
    pub base: u64,
    /// The revision that this edit made.
    pub n: u64,
    #[serde(flatten)]
    pub intent: Intent,
}

/// A hash in 53 bits, so that the number is exact in JSON readers that use doubles.
fn hash(parts: &[&[u8]]) -> u64 {
    let mut h = Sha256::new();
    for p in parts {
        h.update((p.len() as u64).to_le_bytes());
        h.update(p);
    }
    let d = h.finalize();
    u64::from_le_bytes(d[..8].try_into().expect("8 bytes")) & ((1 << 53) - 1)
}

/// Adds the edit that turned `before` into `work` to the log of `work`, and one to its revision.
pub(super) fn record(work: &mut SurfaceModel, before: &SurfaceModel, intent: Intent) {
    let base = match before.log.last() {
        Some(e) => e.stamp,
        None => hash(&[&serde_json::to_vec(before).unwrap_or_default()]),
    };
    let text = serde_json::to_vec(&intent).unwrap_or_default();
    let stamp = hash(&[&base.to_le_bytes(), &before.rev.to_le_bytes(), &text]);
    work.log = before.log.clone();
    work.log.push(Entry {
        stamp,
        base,
        n: before.rev + 1,
        intent,
    });
    work.rev = before.rev + 1;
    let mut points: usize = work.log.iter().map(|e| e.intent.points()).sum();
    while work.log.len() > 1 && (work.log.len() > LOG_ENTRIES || points > LOG_POINTS) {
        points -= work.log.remove(0).intent.points();
    }
}

/// Like [`record`], for a model that was changed in place. `base_model` is the model as it was
/// before the change; it is read only when the log is empty (the first edit takes the hash of the
/// content as its base), so a caller keeps a copy only then.
pub(super) fn record_in_place(
    work: &mut SurfaceModel,
    base_model: Option<&SurfaceModel>,
    intent: Intent,
) {
    let base = match work.log.last() {
        Some(e) => e.stamp,
        None => hash(&[
            &serde_json::to_vec(base_model.expect("a copy of the model before"))
                .unwrap_or_default(),
        ]),
    };
    let text = serde_json::to_vec(&intent).unwrap_or_default();
    let stamp = hash(&[&base.to_le_bytes(), &work.rev.to_le_bytes(), &text]);
    let n = work.rev + 1;
    work.log.push(Entry {
        stamp,
        base,
        n,
        intent,
    });
    work.rev = n;
    let mut points: usize = work.log.iter().map(|e| e.intent.points()).sum();
    while work.log.len() > 1 && (work.log.len() > LOG_ENTRIES || points > LOG_POINTS) {
        points -= work.log.remove(0).intent.points();
    }
}

/// A point inside a face, in local coordinates.
pub(super) fn face_point(m: &SurfaceModel, id: FaceId) -> Option<DVec3> {
    let f = m.faces.get(&id)?;
    let rings: Vec<Vec<DVec2>> = f
        .loops()
        .map(|l| l.iter().map(|v| f.plane.project(m.vertices[v])).collect())
        .collect();
    let p = sample_rings(&rings)?;
    let (u, v) = f.plane.basis();
    Some(u * p.x + v * p.y + f.plane.normal * f.plane.offset)
}

/// The face whose area holds the point.
pub(super) fn face_at(m: &SurfaceModel, p: DVec3) -> Option<FaceId> {
    m.faces
        .iter()
        .find_map(|(id, f)| face_holds(m, f, p).then_some(*id))
}

/// True if the point is in the plane of the face and inside its area.
pub(super) fn face_holds(m: &SurfaceModel, f: &super::Face, p: DVec3) -> bool {
    if !on_plane(&f.plane, p) {
        return false;
    }
    let rings: Vec<Vec<DVec2>> = f
        .loops()
        .map(|l| l.iter().map(|v| f.plane.project(m.vertices[v])).collect())
        .collect();
    inside(f.plane.project(p), &rings)
}

/// The edge that passes through the point (within the length tolerance).
pub(super) fn edge_at(m: &SurfaceModel, p: DVec3) -> Option<EdgeId> {
    nearest_edge(m, p, m.edges.keys().copied())
}

/// The edge among `ids` that passes through the point: the nearest, the lower id on a tie.
pub(super) fn nearest_edge(
    m: &SurfaceModel,
    p: DVec3,
    ids: impl Iterator<Item = EdgeId>,
) -> Option<EdgeId> {
    ids.filter_map(|id| {
        let e = m.edges.get(&id)?;
        let (a, b) = (m.vertices[&e.v[0]], m.vertices[&e.v[1]]);
        Some((geom::point_segment(p, a, b).0, id, geom::tol(&[a, b, p])))
    })
    .filter(|(d, _, t)| d <= t)
    .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)))
    .map(|(_, id, _)| id)
}

fn gone(what: &str, p: &[f64; 3]) -> SurfaceError {
    SurfaceError::Unsolved(format!("the {what} at {:?} is not in the other model", p))
}

/// The vertices at the given positions (the nearest within the length tolerance of each).
fn vertices_at(
    m: &SurfaceModel,
    points: &[[f64; 3]],
) -> Result<Vec<super::VertexId>, SurfaceError> {
    let tree = super::index::BoxTree::new(
        m.vertices
            .iter()
            .map(|(v, p)| (*p, *p, v.0))
            .collect::<Vec<_>>(),
    );
    let pad = DVec3::splat(super::check::window(m));
    points
        .iter()
        .map(|q| {
            let p = DVec3::from(*q);
            let mut best: Option<(f64, super::VertexId)> = None;
            tree.query(p - pad, p + pad, &mut |c| {
                let id = super::VertexId(c);
                let d = m.vertices[&id].distance(p);
                if d <= geom::tol(&[p, m.vertices[&id]]) && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, id));
                }
            });
            best.map(|(_, id)| id).ok_or_else(|| gone("vertex", q))
        })
        .collect()
}

/// Runs an intent on a model (a new log entry for it).
pub fn replay_intent(m: &mut SurfaceModel, intent: &Intent) -> Result<(), SurfaceError> {
    match intent {
        Intent::AddEdges { edges } => m.apply(&Edit::AddEdges {
            edges: edges.clone(),
        }),
        Intent::EraseEdgesAt { points } => {
            let edges = points
                .iter()
                .map(|p| edge_at(m, DVec3::from(*p)).ok_or_else(|| gone("edge", p)))
                .collect::<Result<Vec<_>, _>>()?;
            m.apply(&Edit::EraseEdges { edges })
        }
        Intent::EraseFacesAt { points } | Intent::ReverseFacesAt { points } => {
            let faces = points
                .iter()
                .map(|p| face_at(m, DVec3::from(*p)).ok_or_else(|| gone("face", p)))
                .collect::<Result<Vec<_>, _>>()?;
            if matches!(intent, Intent::EraseFacesAt { .. }) {
                m.apply(&Edit::EraseFaces { faces })
            } else {
                m.apply(&Edit::ReverseFaces { faces })
            }
        }
        Intent::AddCurve { points, closed } => m.apply(&Edit::AddCurve {
            points: points.clone(),
            closed: *closed,
        }),
        Intent::PushPullAt { point, distance } => {
            let face = face_at(m, DVec3::from(*point)).ok_or_else(|| gone("face", point))?;
            m.apply(&Edit::PushPull {
                face,
                distance: *distance,
            })
        }
        Intent::IntersectModel => m.apply(&Edit::IntersectFaces {
            scope: IntersectScope::Model,
            faces: Vec::new(),
            edges: Vec::new(),
        }),
        Intent::IntersectAt { faces, edges } => {
            let faces = faces
                .iter()
                .map(|p| face_at(m, DVec3::from(*p)).ok_or_else(|| gone("face", p)))
                .collect::<Result<Vec<_>, _>>()?;
            let edges = edges
                .iter()
                .map(|p| edge_at(m, DVec3::from(*p)).ok_or_else(|| gone("edge", p)))
                .collect::<Result<Vec<_>, _>>()?;
            m.apply(&Edit::IntersectFaces {
                scope: IntersectScope::Selection,
                faces,
                edges,
            })
        }
        Intent::TransformAt { vertices, matrix } => {
            let ids = vertices_at(m, vertices)?;
            m.apply(&Edit::TransformVertices {
                vertices: ids,
                faces: Vec::new(),
                edges: Vec::new(),
                matrix: *matrix,
            })
        }
        Intent::CopyAt {
            faces,
            edges,
            matrix,
            count,
        } => {
            let faces = faces
                .iter()
                .map(|p| face_at(m, DVec3::from(*p)).ok_or_else(|| gone("face", p)))
                .collect::<Result<Vec<_>, _>>()?;
            let edges = edges
                .iter()
                .map(|p| edge_at(m, DVec3::from(*p)).ok_or_else(|| gone("edge", p)))
                .collect::<Result<Vec<_>, _>>()?;
            m.apply(&Edit::CopyGeometry {
                faces,
                edges,
                matrix: *matrix,
                count: *count,
            })
        }
        Intent::Place { transform } => {
            m.place(DAffine3::from_cols_array(transform));
            Ok(())
        }
        Intent::Unlogged => Err(SurfaceError::Unsolved(
            "this change cannot be replayed on another model".into(),
        )),
    }
}

/// The entries of `old` that `new` does not hold, oldest first.
pub fn missing(old: &SurfaceModel, new: &SurfaceModel) -> Vec<Entry> {
    let have: BTreeSet<u64> = new.log.iter().map(|e| e.stamp).collect();
    old.log
        .iter()
        .filter(|e| !have.contains(&e.stamp))
        .cloned()
        .collect()
}

/// The answer of [`repair`].
#[derive(Clone, Debug, PartialEq)]
pub enum Repair {
    /// Nothing of ours is lost.
    Nothing,
    /// Our lost edits replayed on the incoming model: the shape to send as a new edit.
    Merged(Value),
    /// Our edits could not be replayed. `lost` is the shape that held them; show it to the person.
    Conflict { reason: String, lost: Value },
}

/// The stamps of the log of a shape value (empty for any other shape).
pub fn stamps(shape: &Value) -> Vec<u64> {
    if shape.get("kind").and_then(Value::as_str) != Some("surface_model") {
        return Vec::new();
    }
    shape
        .get("log")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| e.get("stamp").and_then(Value::as_u64))
        .collect()
}

/// Checks a shape that another user wrote (`new`) over the shape this client had (`old`): if
/// `old` holds entries with a stamp in `own` (made here) that `new` lacks, they are replayed on
/// `new`. Values that are not surface models give [`Repair::Nothing`].
pub fn repair(old: &Value, new: &Value, own: &BTreeSet<u64>) -> Repair {
    let have: BTreeSet<u64> = stamps(new).into_iter().collect();
    if new.get("kind").and_then(Value::as_str) != Some("surface_model")
        || !stamps(old)
            .iter()
            .any(|s| own.contains(s) && !have.contains(s))
    {
        return Repair::Nothing;
    }
    let parse = |v: &Value| match serde_json::from_value::<crate::model::Shape>(v.clone()) {
        Ok(crate::model::Shape::SurfaceModel(m)) => Some(*m),
        _ => None,
    };
    let conflict = |reason: String| Repair::Conflict {
        reason,
        lost: old.clone(),
    };
    let (Some(o), Some(n)) = (parse(old), parse(new)) else {
        return conflict("a model could not be read".into());
    };
    let lost: Vec<Entry> = missing(&o, &n)
        .into_iter()
        .filter(|e| own.contains(&e.stamp))
        .collect();
    // An entry older than the log of the incoming model may only have been trimmed from it.
    let window = n.log.first().map_or(0, |e| e.n);
    if let Some(e) = lost.iter().find(|e| e.n < window) {
        return conflict(format!(
            "edit {} is older than the log of the other model",
            e.n
        ));
    }
    let mut m = n;
    for e in &lost {
        if let Err(err) = replay_intent(&mut m, &e.intent) {
            return conflict(err.to_string());
        }
    }
    match serde_json::to_value(crate::model::Shape::SurfaceModel(Box::new(m))) {
        Ok(v) => Repair::Merged(v),
        Err(e) => conflict(e.to_string()),
    }
}
