//! `mesh.near` and `mesh.pick`: spatial questions about the surface models.
//!
//! Everything is computed in drawing coordinates on the triangles of `SurfaceModel::mesh`.

use std::collections::BTreeMap;

use glam::DVec3;
use serde_json::{Value, json};

use kadakas_core::model::{Document, ObjectId};
use kadakas_core::surface::{FaceId, SurfaceModel};

use super::models;
use crate::RpcError;
use crate::errors::bad;
use crate::params::mesh::*;

const LIMIT_DEFAULT: usize = 20;
const LIMIT_MAX: usize = 200;

pub(super) fn handle(doc: &Document, op: &str, p: Value) -> Result<Value, RpcError> {
    if op == "near" {
        near(doc, p)
    } else {
        pick(doc, p)
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

/// The closest point of the triangle `abc` to `p`.
fn closest_on_triangle(p: DVec3, a: DVec3, b: DVec3, c: DVec3) -> DVec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

fn closest_on_segment(p: DVec3, a: DVec3, b: DVec3) -> DVec3 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 == 0.0 {
        return a;
    }
    a + ab * (((p - a).dot(ab) / len2).clamp(0.0, 1.0))
}

/// The triangles of the visible faces in drawing coordinates, with their face.
fn triangles(m: &SurfaceModel, hidden: bool) -> Vec<(FaceId, [DVec3; 3])> {
    let Ok(mesh) = m.mesh() else {
        return Vec::new();
    };
    let world: Vec<DVec3> = mesh
        .positions
        .iter()
        .map(|p| m.transform.transform_point3(*p))
        .collect();
    mesh.triangles
        .iter()
        .zip(&mesh.faces)
        .filter(|(_, f)| hidden || !m.face_hidden(**f))
        .map(|(t, f)| {
            (
                *f,
                [
                    world[t[0] as usize],
                    world[t[1] as usize],
                    world[t[2] as usize],
                ],
            )
        })
        .collect()
}

fn near(doc: &Document, p: Value) -> Result<Value, RpcError> {
    let q: MeshNearParams = serde_json::from_value(p).map_err(bad)?;
    let at = finite(q.point, "point")?;
    if !(q.radius.is_finite() && q.radius >= 0.0) {
        return Err(bad("radius: a number, 0 or more"));
    }
    let limit = limit_of(q.limit)?;
    let hidden = q.include_hidden == Some(true);
    let kinds = q.kinds.unwrap_or_else(|| vec![MeshKind::Faces]);
    let list = models(doc, q.id.as_ref().map(|r| r.0.as_str()))?;
    let mut out = json!({"point": point(at), "radius": q.radius});
    for kind in [MeshKind::Faces, MeshKind::Edges, MeshKind::Vertices] {
        if !kinds.contains(&kind) {
            continue;
        }
        let mut found: Vec<(f64, ObjectId, u32, DVec3)> = Vec::new();
        for (oid, m) in &list {
            match kind {
                MeshKind::Faces => {
                    let mut best: BTreeMap<FaceId, (f64, DVec3)> = BTreeMap::new();
                    for (f, [a, b, c]) in triangles(m, hidden) {
                        let at_c = closest_on_triangle(at, a, b, c);
                        let d = at_c.distance(at);
                        if d <= q.radius && best.get(&f).is_none_or(|(bd, _)| d < *bd) {
                            best.insert(f, (d, at_c));
                        }
                    }
                    found.extend(best.into_iter().map(|(f, (d, c))| (d, *oid, f.0, c)));
                }
                MeshKind::Edges => {
                    for (e, ed) in m.edges() {
                        if !hidden && m.edge_hidden(e) {
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

/// The distance along the ray (unit direction) to the triangle, if it is hit from either side.
fn ray_triangle(o: DVec3, d: DVec3, t: &[DVec3; 3]) -> Option<f64> {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let pv = d.cross(e2);
    let det = e1.dot(pv);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(pv) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qv = s.cross(e1);
    let v = d.dot(qv) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let dist = e2.dot(qv) * inv;
    (dist > 0.0).then_some(dist)
}

fn pick(doc: &Document, p: Value) -> Result<Value, RpcError> {
    let q: MeshPickParams = serde_json::from_value(p).map_err(bad)?;
    let origin = finite(q.origin, "origin")?;
    let dir = finite(q.direction, "direction")?
        .try_normalize()
        .ok_or_else(|| bad("direction: not zero"))?;
    let hidden = q.include_hidden == Some(true);
    let all = q.all == Some(true);
    let list = models(doc, q.id.as_ref().map(|r| r.0.as_str()))?;
    let mut hits: Vec<(f64, ObjectId, FaceId, DVec3)> = Vec::new();
    for (oid, m) in &list {
        let mut best: BTreeMap<FaceId, f64> = BTreeMap::new();
        for (f, tri) in triangles(m, hidden) {
            if let Some(d) = ray_triangle(origin, dir, &tri)
                && best.get(&f).is_none_or(|b| d < *b)
            {
                best.insert(f, d);
            }
        }
        for (f, d) in best {
            hits.push((d, *oid, f, DVec3::ZERO));
        }
    }
    hits.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let row = |(d, oid, f, _): &(f64, ObjectId, FaceId, DVec3)| -> Value {
        let (_, m) = list.iter().find(|(i, _)| i == oid).expect("listed");
        let world = origin + dir * *d;
        let local = m.to_local(world).unwrap_or(DVec3::NAN);
        let n = m.face_normal(*f).unwrap_or(DVec3::Z);
        // The normal of the drawing: the inverse transpose of the linear part.
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
    if all {
        let rows: Vec<Value> = hits.iter().take(LIMIT_MAX).map(row).collect();
        Ok(json!({"hit": rows.first().cloned().unwrap_or(Value::Null), "hits": rows}))
    } else {
        Ok(json!({"hit": hits.first().map(row).unwrap_or(Value::Null)}))
    }
}
