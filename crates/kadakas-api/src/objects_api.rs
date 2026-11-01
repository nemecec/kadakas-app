//! `objects.delete` (AGR-09): the listed method for the command `delete_objects`.
//!
//! The method runs the command, so it has the checks of the command: a locked or grey layer, a
//! grey class, a group that goes with its members, the objects that a script or another person
//! protects. It is one undo step. The command is also part of `batch` in `execute`, which is the
//! way to delete objects together with other edits in one step.

use serde::Deserialize;
use serde_json::{Value, json};

use kadakas_core::model::ObjectId;

use crate::errors::bad;
use crate::mesh_api::typed;
use crate::{RpcError, Session};

/// The most ids that the error answer lists.
const LISTED: usize = 20;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteParams {
    ids: Vec<ObjectId>,
}

/// `objects.delete {ids}`. Returns `{deleted: [ids], count}`: every object that left the document,
/// also the members of a deleted group.
pub(crate) fn delete(s: &mut Session, p: Value) -> Result<Value, RpcError> {
    let DeleteParams { ids } = crate::params(p)?;
    if ids.is_empty() {
        return Err(typed(
            bad("ids is empty: give the ids of the objects to delete"),
            "ids_empty",
            json!({}),
        ));
    }
    let doc = s.editor.doc();
    let unknown: Vec<ObjectId> = ids
        .iter()
        .copied()
        .filter(|id| doc.object(*id).is_none())
        .collect();
    if !unknown.is_empty() {
        let shown: Vec<ObjectId> = unknown.iter().copied().take(LISTED).collect();
        return Err(typed(
            RpcError::not_found(format!(
                "no object {}: nothing was deleted. List the ids with objects.list",
                shown
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            "no_such_object",
            json!({"unknown": shown, "unknown_count": unknown.len()}),
        ));
    }
    let before: std::collections::BTreeSet<ObjectId> = doc.objects.keys().copied().collect();
    let mut command = serde_json::Map::new();
    command.insert("op".into(), json!("delete_objects"));
    command.insert("ids".into(), json!(ids));
    s.execute_json(command)?;
    s.prune_selection();
    let after = s.editor.doc();
    let deleted: Vec<ObjectId> = before
        .into_iter()
        .filter(|id| after.object(*id).is_none())
        .collect();
    Ok(json!({"count": deleted.len(), "deleted": deleted}))
}
