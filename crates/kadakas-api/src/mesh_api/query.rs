//! `mesh.near` and `mesh.pick`: spatial questions about the surface models.
//!
//! The triangle tests are in `kadakas_core::surface` (`faces_near`, `ray_faces`). This file keeps
//! the triangles of each model, in drawing coordinates, and cuts a model again only when that
//! model changed, so that many questions in a row cut the faces once. A ray first meets the box
//! around a model, and only a ray that meets the box goes through the triangles.

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{DAffine3, DVec3};
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
    /// The box around the positions (drawing mm), a little wider than the model.
    lo: DVec3,
    hi: DVec3,
}

impl Tris {
    /// True if the ray meets the box of the model.
    fn ray_meets_box(&self, origin: DVec3, dir: DVec3) -> bool {
        ray_meets_box(origin, dir, self.lo, self.hi)
    }
}

/// True if the ray (a unit direction, from `origin`, forward only) meets the box.
pub(crate) fn ray_meets_box(origin: DVec3, dir: DVec3, lo: DVec3, hi: DVec3) -> bool {
    let (mut t0, mut t1) = (0.0f64, f64::INFINITY);
    for k in 0..3 {
        let (o, d) = (origin[k], dir[k]);
        if d.abs() < 1e-12 {
            if o < lo[k] || o > hi[k] {
                return false;
            }
            continue;
        }
        let (a, b) = ((lo[k] - o) / d, (hi[k] - o) / d);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
        if t0 > t1 {
            return false;
        }
    }
    true
}

/// What tells that a model is as it was when its triangles were cut: where it is in memory, its
/// count of edits, its place and the size of its parts.
#[derive(PartialEq)]
struct ModelKey {
    addr: usize,
    rev: u64,
    transform: DAffine3,
    counts: [usize; 3],
}

impl ModelKey {
    fn of(m: &SurfaceModel) -> ModelKey {
        ModelKey {
            addr: m as *const SurfaceModel as usize,
            rev: m.rev(),
            transform: m.transform,
            counts: [m.vertex_count(), m.edge_count(), m.face_count()],
        }
    }
}

/// The triangles of the models. A model whose faces cannot be cut into triangles is kept as
/// `None`. Each model is cut again only when it changed, so an edit of one model does not cost
/// the cut of all the others.
#[derive(Default)]
pub(crate) struct QueryCache {
    models: BTreeMap<ObjectId, (ModelKey, Option<Arc<Tris>>)>,
    /// How many times a model was cut (for the tests).
    pub(crate) builds: usize,
}

impl QueryCache {
    fn get(&mut self, id: ObjectId, m: &SurfaceModel) -> Option<Arc<Tris>> {
        let key = ModelKey::of(m);
        if let Some((old, tris)) = self.models.get(&id)
            && *old == key
        {
            return tris.clone();
        }
        self.builds += 1;
        let built = m.mesh().ok().map(|mesh| {
            let world: Vec<DVec3> = mesh
                .positions
                .iter()
                .map(|p| m.transform.transform_point3(*p))
                .collect();
            let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
            for p in &world {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            // The triangle test has a tolerance; the box is wider than it.
            let pad = DVec3::splat(1e-3 + 1e-6 * (hi - lo).max_element().max(0.0));
            Arc::new(Tris {
                mesh,
                world,
                lo: lo - pad,
                hi: hi + pad,
            })
        });
        self.models.insert(id, (key, built.clone()));
        built
    }

    /// Forgets the models that are not in the document any more.
    fn prune(&mut self, doc: &Document) {
        if self.models.len() > doc.objects.len() {
            self.models.retain(|id, _| doc.object(*id).is_some());
        }
    }
}

pub(super) fn handle(s: &mut Session, op: &str, p: Value) -> Result<Value, RpcError> {
    let doc = s.editor.doc();
    let cache = &mut s.mesh_query;
    if op == "near" {
        near(doc, cache, p)
    } else {
        pick(doc, cache, p, false)
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

/// `pointer`: the models that the person cannot pick are left out too (grey layers and classes).
fn look<'a>(
    doc: &'a Document,
    cache: &mut QueryCache,
    only: Option<&str>,
    pointer: bool,
) -> Result<Looked<'a>, RpcError> {
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    cache.prune(doc);
    for (id, m) in visible_models(doc, only)? {
        if pointer && doc.object(id).is_none_or(|o| doc.is_grey(o)) {
            continue;
        }
        match cache.get(id, m) {
            Some(t) => found.push((id, m, t)),
            None => skipped.push(json!({
                "model": id,
                "reason": "a face of the model cannot be cut into triangles, so it is not searched"
            })),
        }
    }
    Ok((found, skipped))
}

fn near(doc: &Document, cache: &mut QueryCache, p: Value) -> Result<Value, RpcError> {
    let q: MeshNearParams = serde_json::from_value(p).map_err(bad)?;
    let at = finite(q.point, "point")?;
    if !(q.radius.is_finite() && q.radius >= 0.0) {
        return Err(bad("radius: a number, 0 or more"));
    }
    let limit = limit_of(q.limit)?;
    let hidden = q.include_hidden == Some(true);
    let kinds = q.kinds.unwrap_or_else(|| vec![MeshKind::Faces]);
    let (list, skipped) = look(doc, cache, q.id.as_ref().map(|r| r.0.as_str()), false)?;
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

/// The nearest hit of a ray with the visible surface models: the `hit` of `mesh.pick` for that
/// ray, and `None` when nothing is hit. The pointer of the person (AGR-10) uses it, so the hit
/// under the pointer is the same answer that `mesh.pick` gives.
pub(crate) fn nearest_hit(
    doc: &Document,
    cache: &mut QueryCache,
    origin: DVec3,
    direction: DVec3,
) -> Option<Value> {
    let params = json!({
        "origin": [origin.x, origin.y, origin.z],
        "direction": [direction.x, direction.y, direction.z],
    });
    let mut out = pick(doc, cache, params, true).ok()?;
    match out["hit"].take() {
        Value::Null => None,
        hit => Some(hit),
    }
}

fn pick(
    doc: &Document,
    cache: &mut QueryCache,
    p: Value,
    pointer: bool,
) -> Result<Value, RpcError> {
    let q: MeshPickParams = serde_json::from_value(p).map_err(bad)?;
    let origin = finite(q.origin, "origin")?;
    let dir = finite(q.direction, "direction")?
        .try_normalize()
        .ok_or_else(|| bad("direction: not zero"))?;
    let hidden = q.include_hidden == Some(true);
    let all = q.all == Some(true);
    let (list, skipped) = look(doc, cache, q.id.as_ref().map(|r| r.0.as_str()), pointer)?;
    let mut hits: Vec<(f64, ObjectId, FaceId)> = Vec::new();
    for (oid, m, tris) in &list {
        if !tris.ray_meets_box(origin, dir) {
            continue;
        }
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

    #[test]
    fn an_edit_of_one_model_cuts_only_that_model_again() {
        let mut s = Session::with_sequential_ids();
        let mut faces = Vec::new();
        let mut ids = Vec::new();
        for k in 0..3 {
            let id = s
                .handle("mesh.create", json!({"origin": [k * 1000, 0, 0]}))
                .unwrap()["id"]
                .clone();
            let rect: Vec<Value> = [
                [[0, 0, 0], [100, 0, 0]],
                [[100, 0, 0], [100, 100, 0]],
                [[100, 100, 0], [0, 100, 0]],
                [[0, 100, 0], [0, 0, 0]],
            ]
            .iter()
            .map(|e| json!(e))
            .collect();
            let r = s
                .handle("mesh.add_edges", json!({"id": id, "edges": rect}))
                .unwrap();
            faces.push(r["created"]["faces"][0].clone());
            ids.push(id);
        }
        let ray = json!({"origin": [50, 50, 100], "direction": [0, 0, -1]});
        s.handle("mesh.pick", ray.clone()).unwrap();
        assert_eq!(s.mesh_query.builds, 3);
        s.handle("mesh.pick", ray.clone()).unwrap();
        assert_eq!(s.mesh_query.builds, 3);
        // One model changes: only that one is cut again.
        s.handle(
            "mesh.push_pull",
            json!({"id": ids[1], "face": faces[1], "distance": 10}),
        )
        .unwrap();
        s.handle("mesh.pick", ray.clone()).unwrap();
        assert_eq!(s.mesh_query.builds, 4);
        // A move of a model (its place, not its edits) cuts it again too.
        s.handle(
            "execute",
            json!({"command": {"op": "move_objects", "ids": [ids[2]], "delta": [5, 0]}}),
        )
        .unwrap();
        s.handle("mesh.pick", ray).unwrap();
        assert_eq!(s.mesh_query.builds, 5);
        // A model that is deleted leaves the cache.
        s.handle("objects.delete", json!({"ids": [ids[0]]}))
            .unwrap();
        s.handle(
            "mesh.pick",
            json!({"origin": [50, 50, 100], "direction": [0, 0, -1]}),
        )
        .unwrap();
        assert_eq!(s.mesh_query.models.len(), 2);
    }
}
