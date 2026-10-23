//! `mesh.to_surface` (MOD-12): a `mesh3d` object becomes a surface model.
//!
//! The work is the command `mesh3d_to_surface` of the core (`surface_from_triangles` does the
//! welding, the merging and the softening). This module checks the call, runs the command in one
//! undo step and describes the result. A dry run converts a copy and changes nothing.

use serde_json::{Value, json};

use kadakas_core::command::Command;
use kadakas_core::model::{Document, ObjectId, Shape};
use kadakas_core::surface::{ConvertOptions, surface_from_triangles};

use super::{object_id, typed};
use crate::errors::{bad, convert_rpc};
use crate::params::mesh::MeshToSurfaceParams;
use crate::{RpcError, Session};

/// The mesh of an object that the person could edit now, or the typed error.
fn editable_mesh(doc: &Document, id: ObjectId) -> Result<&kadakas_core::mesh3d::Mesh3d, RpcError> {
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
                "object {id} is not a mesh (it is a {}): only a mesh3d object becomes a surface model",
                o.shape.kind()
            )),
            "not_a_mesh",
            json!({"object": id, "kind": o.shape.kind()}),
        ));
    };
    if doc.layer(o.layer).is_some_and(|l| l.locked) {
        return Err(typed(
            RpcError::conflict(format!("layer {} is locked", o.layer)),
            "layer_locked",
            json!({"object": id, "layer": o.layer}),
        ));
    }
    if doc.drawing.grey.layers.contains(&o.layer) {
        return Err(typed(
            RpcError::validation(format!(
                "layer {} is grey: its objects cannot be edited. Make it visible first",
                o.layer
            )),
            "layer_grey",
            json!({"object": id, "layer": o.layer}),
        ));
    }
    if doc.drawing.grey.classes.contains(&o.class) {
        return Err(typed(
            RpcError::validation(format!(
                "class {} is grey: its objects cannot be edited. Make it visible first",
                o.class
            )),
            "class_grey",
            json!({"object": id, "class": o.class}),
        ));
    }
    Ok(m)
}

pub(super) fn to_surface(s: &mut Session, p: Value) -> Result<Value, RpcError> {
    let q: MeshToSurfaceParams = serde_json::from_value(p).map_err(bad)?;
    let id = object_id(&q.id.0)?;
    if let Some(a) = q.soften_below_angle
        && !(a.is_finite() && (0.0..=180.0).contains(&a))
    {
        return Err(bad("soften_below_angle: degrees, 0 to 180"));
    }
    if let Some(t) = q.tolerance
        && !(t.is_finite() && (0.001..=1000.0).contains(&t))
    {
        return Err(bad("tolerance: mm, 0.001 to 1000"));
    }
    let mesh = editable_mesh(s.editor.doc(), id)?;
    let tolerance = q.tolerance;
    let soften_below_angle = q.soften_below_angle;
    let merge = q.merge.unwrap_or(true);
    if q.dry_run.unwrap_or(false) {
        let c = surface_from_triangles(
            &mesh.vertices,
            &mesh.triangles,
            &ConvertOptions {
                tolerance,
                soften_below_angle,
                merge,
            },
        )
        .map_err(|e| convert_rpc(&e))?;
        let mut out = json!({
            "id": id,
            "dry_run": true,
            "kind": "mesh3d",
            "softened_edges": c.report.softened_edges,
            "report": c.report,
        });
        if !c.notes.is_empty() {
            out["notes"] = json!(c.notes);
        }
        return Ok(out);
    }
    let outcome = s
        .editor
        .execute(&Command::Mesh3dToSurface {
            id,
            tolerance,
            soften_below_angle,
            merge,
        })
        .map_err(|e| match e {
            kadakas_core::command::CommandError::MeshToSurface(c) => convert_rpc(&c),
            other => RpcError::from(other),
        })?;
    s.prune_selection();
    let report = outcome.report.unwrap_or(Value::Null);
    let mut out = json!({
        "id": id,
        "kind": "surface_model",
        "softened_edges": report["softened_edges"].as_u64().unwrap_or(0),
        "report": report,
    });
    if !outcome.notes.is_empty() {
        out["notes"] = json!(outcome.notes);
    }
    Ok(out)
}
