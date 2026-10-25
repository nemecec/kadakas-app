//! `mesh.summary|vertices|edges|faces|solid_report|bounds`: read a surface model.

use std::collections::BTreeMap;

use glam::DVec3;
use serde_json::{Value, json};

use kadakas_core::model::{Document, ObjectId};
use kadakas_core::surface::{
    ATTR_BACK_MATERIAL, ATTR_MATERIAL, Analytic, CurveId, EdgeId, FaceId, SurfaceModel, VertexId,
};

use super::{model_of, models, object_id};
use crate::RpcError;
use crate::errors::bad;
use crate::params::mesh::*;

/// The most items of one page.
pub const PAGE_MAX: usize = 1000;
const PAGE_DEFAULT: usize = 100;

pub(super) fn handle(doc: &Document, op: &str, p: Value) -> Result<Value, RpcError> {
    match op {
        "summary" => summary(doc, p),
        "vertices" => vertices(doc, p),
        "edges" => edges(doc, p),
        "faces" => faces(doc, p),
        "solid_report" => solid_report(doc, p),
        "curve_info" => curve_info(doc, p),
        _ => bounds(doc, p),
    }
}

/// The exact circle of a curve as JSON (MOD-15).
fn geometry_json(a: &Analytic) -> Value {
    let mut v = json!({
        "kind": a.kind,
        "center": a.center,
        "normal": a.normal,
        "axis": a.axis,
        "radius": a.radius,
    });
    if let (Some(s), Some(e)) = (a.start, a.end) {
        v["start_deg"] = json!(s.to_degrees());
        v["end_deg"] = json!(e.to_degrees());
        v["radians"] = json!({"start": s, "end": e});
    }
    v
}

fn curve_json(m: &SurfaceModel, id: CurveId) -> Value {
    json!({
        "curve": id,
        "edges": m.curve_edges(id).unwrap_or_default(),
        "segments": m.curve_segments(id).unwrap_or(0),
        "closed": m.curve_closed(id),
        "geometry": m.curve_geometry(id).map(geometry_json),
        "length_mm": m.curve_length(id),
    })
}

fn curve_info(doc: &Document, p: Value) -> Result<Value, RpcError> {
    let q: MeshCurveInfoParams = serde_json::from_value(p).map_err(bad)?;
    let id = object_id(&q.id.0)?;
    let m = model_of(doc, id)?;
    if let Some(c) = q.curve {
        if m.curve_segments(CurveId(c)).is_none() {
            return Err(super::typed(
                RpcError::not_found(format!("curve {c} does not exist")),
                "no_such_curve",
                json!({"object": id, "curve": c}),
            ));
        }
        return Ok(json!({"total": 1, "offset": 0, "items": [curve_json(m, CurveId(c))]}));
    }
    let pg = page(
        m.curves().map(|(c, _)| c.0),
        m.curves().count(),
        None,
        q.offset,
        q.limit,
        |_| true,
    )?;
    let items = pg.ids.iter().map(|c| curve_json(m, CurveId(*c))).collect();
    Ok(paged(pg, items))
}

fn point(p: DVec3) -> Value {
    json!([p.x, p.y, p.z])
}

fn boxed(b: Option<(DVec3, DVec3)>) -> Value {
    match b {
        Some((lo, hi)) => json!({"min": point(lo), "max": point(hi), "size": point(hi - lo)}),
        None => Value::Null,
    }
}

fn summary(doc: &Document, p: Value) -> Result<Value, RpcError> {
    let q: MeshSummaryParams = serde_json::from_value(p).map_err(bad)?;
    let list = models(doc, q.id.as_ref().map(|r| r.0.as_str()))?;
    let rows: Vec<Value> = list
        .into_iter()
        .map(|(id, m)| {
            let o = doc.object(id).expect("listed");
            let mut row = json!({
                "id": id,
                "name": m.name,
                "layer": o.layer,
                "class": o.class,
                "vertices": m.vertex_count(),
                "edges": m.edge_count(),
                "faces": m.face_count(),
                "curves": m.curves().count(),
                "revision": m.rev(),
                "problem": m.problem(),
                "hidden_faces": m.hidden_faces().len(),
                "hidden_edges": m.hidden_edges().len(),
                "soft_edges": m.edges().filter(|(e, _)| m.edge_soft(*e)).count(),
                "transform": m.transform.to_cols_array(),
                "bounds": {"local": boxed(m.local_bounds()), "world": boxed(m.bounds())},
            });
            if let Some(parent) = o.parent {
                row["parent"] = json!(parent);
            }
            row
        })
        .collect();
    let instances = doc
        .objects
        .values()
        .filter(|o| matches!(o.shape, kadakas_core::model::Shape::Component { .. }))
        .count();
    Ok(json!({ "component_instances": instances, "models": rows }))
}

/// The page of a list: the ids asked for, else a window of all ids.
struct Page {
    total: usize,
    offset: usize,
    ids: Vec<u32>,
    missing: Vec<u32>,
}

fn page(
    all: impl Iterator<Item = u32>,
    count: usize,
    ids: Option<Vec<u32>>,
    offset: Option<u32>,
    limit: Option<u32>,
    has: impl Fn(u32) -> bool,
) -> Result<Page, RpcError> {
    let limit = limit.map_or(PAGE_DEFAULT, |l| l as usize);
    if limit == 0 || limit > PAGE_MAX {
        return Err(bad(format!("limit: 1 to {PAGE_MAX}")));
    }
    let offset = offset.unwrap_or(0) as usize;
    if let Some(ids) = ids {
        if ids.len() > PAGE_MAX {
            return Err(bad(format!(
                "ids: at most {PAGE_MAX} ids in one call, read the rest with another call"
            )));
        }
        let (found, missing): (Vec<u32>, Vec<u32>) = ids.into_iter().partition(|i| has(*i));
        return Ok(Page {
            total: found.len(),
            offset,
            ids: found.into_iter().skip(offset).take(limit).collect(),
            missing,
        });
    }
    Ok(Page {
        total: count,
        offset,
        ids: all.skip(offset).take(limit).collect(),
        missing: Vec::new(),
    })
}

fn paged(page: Page, items: Vec<Value>) -> Value {
    let mut out = json!({"total": page.total, "offset": page.offset, "items": items});
    if !page.missing.is_empty() {
        out["missing"] = json!(page.missing.iter().take(PAGE_MAX).collect::<Vec<_>>());
    }
    out
}

fn vertices(doc: &Document, p: Value) -> Result<Value, RpcError> {
    let q: MeshVerticesParams = serde_json::from_value(p).map_err(bad)?;
    let m = model_of(doc, object_id(&q.id.0)?)?;
    let pg = page(
        m.vertices().map(|(v, _)| v.0),
        m.vertex_count(),
        q.ids,
        q.offset,
        q.limit,
        |i| m.vertex(VertexId(i)).is_some(),
    )?;
    let items = pg
        .ids
        .iter()
        .map(|i| json!({"id": i, "p": point(m.vertex(VertexId(*i)).expect("found"))}))
        .collect();
    Ok(paged(pg, items))
}

/// The faces of each edge (by its pair of vertices), made in one pass over the faces.
type EdgeFaces = BTreeMap<(VertexId, VertexId), Vec<FaceId>>;

fn edge_faces(m: &SurfaceModel) -> EdgeFaces {
    let mut map: EdgeFaces = BTreeMap::new();
    for (fid, f) in m.faces() {
        for (a, b) in f.uses() {
            let list = map.entry(if a <= b { (a, b) } else { (b, a) }).or_default();
            if list.last() != Some(&fid) {
                list.push(fid);
            }
        }
    }
    map
}

pub(super) fn edge_json(m: &SurfaceModel, id: EdgeId, faces: &EdgeFaces) -> Value {
    let e = m.edge(id).expect("exists");
    let length = match (m.vertex(e.v[0]), m.vertex(e.v[1])) {
        (Some(a), Some(b)) => a.distance(b),
        _ => f64::NAN,
    };
    json!({
        "id": id,
        "v": [e.v[0], e.v[1]],
        "faces": faces.get(&e.key()).cloned().unwrap_or_default(),
        "soft": m.edge_soft(id),
        "smooth": m.edge_smooth(id),
        "hidden": m.edge_hidden(id),
        "curve": m.curve_of(id),
        "length_mm": length,
    })
}

fn edges(doc: &Document, p: Value) -> Result<Value, RpcError> {
    let q: MeshEdgesParams = serde_json::from_value(p).map_err(bad)?;
    let m = model_of(doc, object_id(&q.id.0)?)?;
    let pg = page(
        m.edges().map(|(e, _)| e.0),
        m.edge_count(),
        q.ids,
        q.offset,
        q.limit,
        |i| m.edge(EdgeId(i)).is_some(),
    )?;
    let faces = edge_faces(m);
    let items = pg
        .ids
        .iter()
        .map(|i| edge_json(m, EdgeId(*i), &faces))
        .collect();
    Ok(paged(pg, items))
}

fn text_attr(m: &SurfaceModel, id: FaceId, key: &str) -> Value {
    m.face(id)
        .and_then(|f| f.attrs.get(key))
        .cloned()
        .unwrap_or(Value::Null)
}

fn faces(doc: &Document, p: Value) -> Result<Value, RpcError> {
    let q: MeshFacesParams = serde_json::from_value(p).map_err(bad)?;
    let m = model_of(doc, object_id(&q.id.0)?)?;
    let pg = page(
        m.faces().map(|(f, _)| f.0),
        m.face_count(),
        q.ids,
        q.offset,
        q.limit,
        |i| m.face(FaceId(i)).is_some(),
    )?;
    let points = q.with_points == Some(true);
    let items = pg
        .ids
        .iter()
        .map(|i| {
            let id = FaceId(*i);
            let f = m.face(id).expect("found");
            let n = f.plane.normal;
            let mut row = json!({
                "id": id,
                "outer": f.outer,
                "holes": f.holes,
                "normal": point(n),
                "area_mm2": m.face_area(id),
                "hidden": m.face_hidden(id),
                "material": text_attr(m, id, ATTR_MATERIAL),
                "back_material": text_attr(m, id, ATTR_BACK_MATERIAL),
            });
            if points {
                let loop_points = |l: &[VertexId]| -> Vec<Value> {
                    m.loop_points(l).into_iter().map(point).collect()
                };
                row["points"] = json!({
                    "outer": loop_points(&f.outer),
                    "holes": f.holes.iter().map(|h| loop_points(h)).collect::<Vec<_>>(),
                });
            }
            row
        })
        .collect();
    Ok(paged(pg, items))
}

fn solid_report(doc: &Document, p: Value) -> Result<Value, RpcError> {
    let q: MeshSolidReportParams = serde_json::from_value(p).map_err(bad)?;
    let id = object_id(&q.id.0)?;
    let m = model_of(doc, id)?;
    let r = m.solid_report();
    let mut v = serde_json::to_value(&r).map_err(RpcError::internal)?;
    v["id"] = json!(id);
    v["closed"] = json!(r.is_closed());
    Ok(v)
}

fn bounds(doc: &Document, p: Value) -> Result<Value, RpcError> {
    let q: MeshBoundsParams = serde_json::from_value(p).map_err(bad)?;
    let id: ObjectId = object_id(&q.id.0)?;
    let m = model_of(doc, id)?;
    Ok(json!({
        "id": id,
        "empty": m.is_empty(),
        "local": boxed(m.local_bounds()),
        "world": boxed(m.bounds()),
    }))
}
