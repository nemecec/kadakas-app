//! `mesh.summary|vertices|edges|faces|solid_report|bounds`: read a surface model.

use glam::DVec3;
use serde_json::{Value, json};

use kadakas_core::model::{Document, ObjectId};
use kadakas_core::surface::{
    ATTR_BACK_MATERIAL, ATTR_MATERIAL, EdgeId, FaceId, SurfaceModel, VertexId,
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
        _ => bounds(doc, p),
    }
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
    Ok(json!({ "models": rows }))
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
        out["missing"] = json!(page.missing);
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

pub(super) fn edge_json(m: &SurfaceModel, id: EdgeId) -> Value {
    let e = m.edge(id).expect("exists");
    let length = match (m.vertex(e.v[0]), m.vertex(e.v[1])) {
        (Some(a), Some(b)) => a.distance(b),
        _ => f64::NAN,
    };
    json!({
        "id": id,
        "v": [e.v[0], e.v[1]],
        "faces": m.faces_of_edge(id),
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
    let items = pg.ids.iter().map(|i| edge_json(m, EdgeId(*i))).collect();
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
