//! `mesh3d.exact_info` and `mesh3d.retessellate`: what a mesh object that comes from a STEP
//! file still knows about its surfaces (the exact payload, `kadakas_core::exact`).
//!
//! The triangles stay the source for drawing and for every tool. The payload gives the true
//! radii and normals for dimensioning, and the triangles can be cut again from it at another
//! tolerance. A payload is stale when the triangles were edited after the import: both
//! methods say so and `retessellate` refuses to use it.

use serde_json::{Value, json};

use kadakas_core::command::Command;
use kadakas_core::exact::State;
use kadakas_core::mesh3d::{self, Mesh3d};
use kadakas_core::model::{Document, ObjectId, Shape};

use super::{object_id, typed};
use crate::errors::bad;
use crate::params::mesh::{Mesh3dExactInfoParams, Mesh3dRetessellateParams};
use crate::{RpcError, Session};

/// The mesh of an object with its exact data, or the typed error.
fn mesh_with_exact(doc: &Document, id: ObjectId) -> Result<&Mesh3d, RpcError> {
    let Some(o) = doc.object(id) else {
        return Err(typed(
            RpcError::not_found(format!("object {id} does not exist")),
            "no_such_object",
            json!({"object": id}),
        ));
    };
    let Shape::Mesh3d(m) = &o.shape else {
        return Err(typed(
            RpcError::validation(format!(
                "object {id} is not a mesh (it is a {})",
                o.shape.kind()
            )),
            "not_a_mesh",
            json!({"object": id, "kind": o.shape.kind()}),
        ));
    };
    if m.exact.is_none() && m.exact_unusable.is_none() {
        return Err(typed(
            RpcError::validation(format!(
                "object {id} has no exact data: only a mesh from a STEP import with keep_exact has it"
            )),
            "no_exact_data",
            json!({"object": id}),
        ));
    }
    Ok(m)
}

pub(super) fn exact_info(s: &mut Session, p: Value) -> Result<Value, RpcError> {
    let q: Mesh3dExactInfoParams = serde_json::from_value(p).map_err(bad)?;
    let id = object_id(&q.id.0)?;
    let limit = q.limit.map_or(200, |n| n.0.clamp(1, 5000) as usize);
    let m = mesh_with_exact(s.editor.doc(), id)?;
    if let Some(u) = &m.exact_unusable {
        return Ok(json!({
            "id": id,
            "state": "unusable",
            "reason": u.reason,
            "note": "this program cannot use the exact data. The file keeps it as it is.",
        }));
    }
    let e = m.exact.as_ref().expect("checked");
    let stale = m.exact_state() == Some(State::Stale);
    let mut out = e.info(limit);
    out["id"] = json!(id);
    out["state"] = json!(if stale { "stale" } else { "fresh" });
    out["summary"] = json!(e.summary());
    out["unit"] = json!("mm");
    out["approx_bytes"] = json!(e.approx_bytes());
    if stale {
        out["note"] = json!(
            "the triangles were edited after the import, so the exact data may no longer fit them"
        );
    }
    Ok(out)
}

pub(super) fn retessellate(s: &mut Session, p: Value) -> Result<Value, RpcError> {
    let q: Mesh3dRetessellateParams = serde_json::from_value(p).map_err(bad)?;
    let id = object_id(&q.id.0)?;
    if !(q.tolerance.is_finite() && (0.001..=1000.0).contains(&q.tolerance)) {
        return Err(bad("tolerance: mm, 0.001 to 1000"));
    }
    let angle = q.angle.unwrap_or(15.0);
    if !(angle.is_finite() && (1.0..=90.0).contains(&angle)) {
        return Err(bad("angle: degrees, 1 to 90"));
    }
    let m = mesh_with_exact(s.editor.doc(), id)?;
    if let Some(u) = &m.exact_unusable {
        return Err(typed(
            RpcError::conflict(format!(
                "object {id}: this program cannot use the exact data ({})",
                u.reason
            )),
            "exact_unusable",
            json!({"object": id, "reason": u.reason}),
        ));
    }
    if m.exact_state() == Some(State::Stale) {
        return Err(typed(
            RpcError::conflict(format!(
                "object {id}: the triangles were edited after the import, so the exact data no longer fits them"
            )),
            "exact_stale",
            json!({"object": id}),
        ));
    }
    let mut exact = (**m.exact.as_ref().expect("checked")).clone();
    let before = (m.vertices.len(), m.triangles.len());
    let limits = kadakas_meshio::Limits {
        max_triangles: mesh3d::MAX_TRIANGLES as u64,
        max_vertices: mesh3d::MAX_VERTICES as u64,
        ..Default::default()
    };
    let r = kadakas_step::retessellate(&exact, q.tolerance, angle, &limits)
        .map_err(|e| crate::errors::mesh_rpc(&e))?;
    let mut mesh = Mesh3d {
        name: m.name.clone(),
        vertices: r.positions.iter().map(|p| glam::DVec3::from(*p)).collect(),
        triangles: r.triangles,
        color: m.color,
        exact: None,
        exact_unusable: None,
    };
    exact.tolerance_mm = q.tolerance;
    exact.angle_deg = angle;
    mesh.set_exact(exact);
    if let Some(why) = mesh.problem() {
        return Err(
            RpcError::validation(format!("the new mesh is not valid: {why}"))
                .with_reason("mesh_malformed"),
        );
    }
    let after = (mesh.vertices.len(), mesh.triangles.len());
    let volume = mesh.volume();
    let kinds: Vec<Value> = r
        .kinds
        .iter()
        .map(|(k, f, t)| json!({"surface": k, "faces": f, "triangles": t}))
        .collect();
    let skipped: Vec<Value> = r
        .skipped
        .iter()
        .map(|(f, why)| json!({"face": f, "reason": why}))
        .collect();
    s.editor
        .execute(&Command::ModifyObject {
            id,
            shape: Some(Shape::Mesh3d(Box::new(mesh))),
            layer: None,
            class: None,
            attrs: None,
        })
        .map_err(RpcError::from)?;
    s.prune_selection();
    let mut out = json!({
        "id": id,
        "tolerance_mm": q.tolerance,
        "angle_deg": angle,
        "vertices": {"before": before.0, "after": after.0},
        "triangles": {"before": before.1, "after": after.1},
        "volume_mm3": volume,
        "faces": kinds,
    });
    if !skipped.is_empty() {
        out["skipped_faces"] = json!(skipped);
    }
    if !r.notes.is_empty() {
        out["notes"] = json!(r.notes);
    }
    Ok(out)
}
