//! `mesh.near` and `mesh.pick`: spatial questions about the surface models.
//!
//! The triangle tests are in `kadakas_core::surface` (`faces_near`, `ray_faces`). This file keeps
//! the triangles of each model for a revision of the document, in drawing coordinates, so that
//! many questions in a row cut the faces once.

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::DVec3;
use serde_json::{Value, json};

use kadakas_core::model::{Document, ObjectId};
use kadakas_core::surface::{
    FaceId, SurfaceMesh, SurfaceModel, closest_on_segment, faces_near, ray_faces,
};

use super::visible_models;
use crate::errors::bad;
use crate::params::mesh::*;
use crate::{RpcError, Session};

const LIMIT_DEFAULT: usize = 20;
/// The most items of one kind in an answer.
pub const LIMIT_MAX: usize = 200;

/// The triangles of one model, with the positions in drawing coordinates.
pub(crate) struct Tris {
    mesh: SurfaceMesh,
    world: Vec<DVec3>,
}

/// The triangles of the models for one revision of the document. A model whose faces cannot be
/// cut into triangles is kept as `None`.
#[derive(Default)]
pub(crate) struct QueryCache {
    revision: u64,
    models: BTreeMap<ObjectId, Option<Arc<Tris>>>,
    /// How many times a model was cut (for the tests).
    pub(crate) builds: usize,
}

impl QueryCache {
    fn get(&mut self, revision: u64, id: ObjectId, m: &SurfaceModel) -> Option<Arc<Tris>> {
        if self.revision != revision {
            self.revision = revision;
            self.models.clear();
        }
        if let Some(t) = self.models.get(&id) {
            return t.clone();
        }
        self.builds += 1;
        let built = m.mesh().ok().map(|mesh| {
            let world = mesh
                .positions
                .iter()
                .map(|p| m.transform.transform_point3(*p))
                .collect();
            Arc::new(Tris { mesh, world })
        });
        self.models.insert(id, built.clone());
        built
    }
}

pub(super) fn handle(s: &mut Session, op: &str, p: Value) -> Result<Value, RpcError> {
    let revision = s.editor.revision();
    let doc = s.editor.doc();
    let cache = &mut s.mesh_query;
    if op == "near" {
        near(doc, cache, revision, p)
    } else {
        pick(doc, cache, revision, p)
    }
}

fn point(p: DVec3) -> Value {
    json!([p.x, p.y, p.z])
}

fn finite(p: [f64; 3], what: &str) -> Result<DVec3, RpcError> {
    let v = DVec3::from(p);
    if v.is_finite() {
        Ok(v)
    } else {
        Err(bad(format!("{what}: three finite numbers")))
    }
}

fn limit_of(limit: Option<u32>) -> Result<usize, RpcError> {
    let l = limit.map_or(LIMIT_DEFAULT, |l| l as usize);
    if l == 0 || l > LIMIT_MAX {
        return Err(bad(format!("limit: 1 to {LIMIT_MAX}")));
    }
    Ok(l)
}

/// The models to look at, with their triangles, and the models that could not be cut.
type Looked<'a> = (Vec<(ObjectId, &'a SurfaceModel, Arc<Tris>)>, Vec<Value>);

fn look<'a>(
    doc: &'a Document,
    cache: &mut QueryCache,
    revision: u64,
    only: Option<&str>,
) -> Result<Looked<'a>, RpcError> {
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    for (id, m) in visible_models(doc, only)? {
        match cache.get(revision, id, m) {
            Some(t) => found.push((id, m, t)),
            None => skipped.push(json!({
                "model": id,
                "reason": "a face of the model cannot be cut into triangles, so it is not searched"
            })),
        }
    }
    Ok((found, skipped))
}

fn near(
    doc: &Document,
    cache: &mut QueryCache,
    revision: u64,
    p: Value,
) -> Result<Value, RpcError> {
    let q: MeshNearParams = serde_json::from_value(p).map_err(bad)?;
    let at = finite(q.point, "point")?;
    if !(q.radius.is_finite() && q.radius >= 0.0) {
        return Err(bad("radius: a number, 0 or more"));
    }
    let limit = limit_of(q.limit)?;
    let hidden = q.include_hidden == Some(true);
    let kinds = q.kinds.unwrap_or_else(|| vec![MeshKind::Faces]);
    let (list, skipped) = look(doc, cache, revision, q.id.as_ref().map(|r| r.0.as_str()))?;
    let mut out = json!({"point": point(at), "radius": q.radius});
    if !skipped.is_empty() {
        out["skipped"] = json!(skipped);
    }
    for kind in [MeshKind::Faces, MeshKind::Edges, MeshKind::Vertices] {
        if !kinds.contains(&kind) {
            continue;
        }
        let mut found: Vec<(f64, ObjectId, u32, DVec3)> = Vec::new();
        for (oid, m, tris) in &list {
            match kind {
                MeshKind::Faces => {
                    let near = faces_near(&tris.mesh, &tris.world, at, q.radius, |f| {
                        !hidden && m.face_hidden(f)
                    });
                    found.extend(near.into_iter().map(|(f, d, c)| (d, *oid, f.0, c)));
                }
                MeshKind::Edges => {
                    for (e, ed) in m.edges() {
                        // A hidden or soft edge is not drawn or picked, so it is left out too.
                        if !hidden && !m.edge_shown(e) {
                            continue;
                        }
                        let (Some(a), Some(b)) = (m.vertex(ed.v[0]), m.vertex(ed.v[1])) else {
                            continue;
                        };
                        let (a, b) = (
                            m.transform.transform_point3(a),
                            m.transform.transform_point3(b),
                        );
                        let c = closest_on_segment(at, a, b);
                        let d = c.distance(at);
                        if d <= q.radius {
                            found.push((d, *oid, e.0, c));
                        }
                    }
                }
                MeshKind::Vertices => {
                    for (v, pos) in m.vertices() {
                        let w = m.transform.transform_point3(pos);
                        let d = w.distance(at);
                        if d <= q.radius {
                            found.push((d, *oid, v.0, w));
                        }
                    }
                }
            }
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let rows: Vec<Value> = found
            .into_iter()
            .take(limit)
            .map(|(d, oid, id, c)| json!({"model": oid, "id": id, "distance": d, "closest": point(c)}))
            .collect();
        let key = match kind {
            MeshKind::Faces => "faces",
            MeshKind::Edges => "edges",
            MeshKind::Vertices => "vertices",
        };
        out[key] = json!(rows);
    }
    Ok(out)
}

fn pick(
    doc: &Document,
    cache: &mut QueryCache,
    revision: u64,
    p: Value,
) -> Result<Value, RpcError> {
    let q: MeshPickParams = serde_json::from_value(p).map_err(bad)?;
    let origin = finite(q.origin, "origin")?;
    let dir = finite(q.direction, "direction")?
        .try_normalize()
        .ok_or_else(|| bad("direction: not zero"))?;
    let hidden = q.include_hidden == Some(true);
    let all = q.all == Some(true);
    let (list, skipped) = look(doc, cache, revision, q.id.as_ref().map(|r| r.0.as_str()))?;
    let mut hits: Vec<(f64, ObjectId, FaceId)> = Vec::new();
    for (oid, m, tris) in &list {
        for (f, d) in ray_faces(&tris.mesh, &tris.world, origin, dir, |f| {
            !hidden && m.face_hidden(f)
        }) {
            hits.push((d, *oid, f));
        }
    }
    hits.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let row = |(d, oid, f): &(f64, ObjectId, FaceId)| -> Value {
        let (_, m, _) = list.iter().find(|(i, _, _)| i == oid).expect("listed");
        let world = origin + dir * *d;
        let local = m.to_local(world).unwrap_or(DVec3::NAN);
        let n = m.face_normal(*f).unwrap_or(DVec3::Z);
        // The normal in the drawing: the inverse transpose of the linear part.
        let wn = m.transform.matrix3.inverse().transpose() * n;
        let wn = wn.try_normalize().unwrap_or(wn);
        json!({
            "model": oid,
            "face": f,
            "distance": d,
            "point": point(world),
            "local": point(local),
            "side": if dir.dot(wn) < 0.0 { "front" } else { "back" },
            "normal": point(wn),
        })
    };
    let mut out = if all {
        let rows: Vec<Value> = hits.iter().take(LIMIT_MAX).map(row).collect();
        let mut o = json!({"hit": rows.first().cloned().unwrap_or(Value::Null), "hits": rows});
        if hits.len() > LIMIT_MAX {
            o["truncated"] = json!(true);
        }
        o
    } else {
        json!({"hit": hits.first().map(row).unwrap_or(Value::Null)})
    };
    if !skipped.is_empty() {
        out["skipped"] = json!(skipped);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_triangles_are_cut_once_for_a_revision_of_the_document() {
        let mut s = Session::with_sequential_ids();
        let id = s.handle("mesh.create", json!({})).unwrap()["id"].clone();
        let edges: Vec<Value> = [
            [[0, 0, 0], [100, 0, 0]],
            [[100, 0, 0], [100, 100, 0]],
            [[100, 100, 0], [0, 100, 0]],
            [[0, 100, 0], [0, 0, 0]],
        ]
        .iter()
        .map(|e| json!(e))
        .collect();
        let face = s
            .handle("mesh.add_edges", json!({"id": id, "edges": edges}))
            .unwrap()["created"]["faces"][0]
            .clone();
        for _ in 0..3 {
            s.handle("mesh.near", json!({"point": [50, 50, 10], "radius": 20}))
                .unwrap();
            s.handle(
                "mesh.pick",
                json!({"origin": [50, 50, 10], "direction": [0, 0, -1]}),
            )
            .unwrap();
        }
        assert_eq!(s.mesh_query.builds, 1);
        // An edit makes a new revision, and so a new cut. Undo and redo too.
        s.handle(
            "mesh.push_pull",
            json!({"id": id, "face": face, "distance": 10}),
        )
        .unwrap();
        let hit = s
            .handle(
                "mesh.pick",
                json!({"origin": [50, 50, 100], "direction": [0, 0, -1]}),
            )
            .unwrap();
        assert_eq!(s.mesh_query.builds, 2);
        assert!((hit["hit"]["distance"].as_f64().unwrap() - 90.0).abs() < 1e-6);
        s.handle("undo", json!({})).unwrap();
        let hit = s
            .handle(
                "mesh.pick",
                json!({"origin": [50, 50, 100], "direction": [0, 0, -1]}),
            )
            .unwrap();
        assert_eq!(s.mesh_query.builds, 3);
        assert!((hit["hit"]["distance"].as_f64().unwrap() - 100.0).abs() < 1e-6);
    }
}
