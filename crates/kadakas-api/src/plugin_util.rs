//! Small helpers that the plug-in methods share (`plugin_api` for Python, `wasm_plugin_api` for
//! WebAssembly). They compile on every target.

use kadakas_core::command::Command;
use kadakas_core::model::{ObjectId, Shape};
use kadakas_core::uid::AsUid;
use serde_json::{Map, Value};

use crate::errors::bad;
use crate::{RpcError, Session};

pub(crate) fn sel<'a>(m: &'a Map<String, Value>, keys: &[&str]) -> Result<&'a Value, RpcError> {
    keys.iter()
        .find_map(|k| m.get(*k))
        .ok_or_else(|| bad(format!("give {}", keys.join(" or "))))
}

pub(crate) fn name_of(m: &Map<String, Value>, key: &str) -> Result<String, RpcError> {
    let n = m
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .ok_or_else(|| bad(format!("{key} is required")))?;
    if n.chars().count() > 100 {
        return Err(bad("a name has at most 100 characters"));
    }
    Ok(n.to_string())
}

pub(crate) fn exec(
    s: &mut Session,
    cmd: &Command,
) -> Result<Vec<kadakas_core::uid::Uid>, RpcError> {
    let cmd = s.in_context(cmd.clone());
    // Plug-in objects change because a script ran, so the change events say so.
    let before = s.editor.origin();
    s.editor.set_origin(kadakas_core::change::Origin::Script);
    let out = s.editor.execute(&cmd).map_err(RpcError::from);
    s.editor.set_origin(before);
    let out = out?;
    s.prune_selection();
    Ok(out.created)
}

/// The lowest corner of the shapes, to notice that someone moved the object.
pub(crate) fn origin_of(shapes: impl Iterator<Item = Shape>) -> [f64; 2] {
    let mut lo = [f64::INFINITY; 2];
    for sh in shapes {
        let (a, _) = sh.bounds();
        lo = [lo[0].min(a.x), lo[1].min(a.y)];
    }
    if lo.iter().all(|c| c.is_finite()) {
        lo
    } else {
        [0.0, 0.0]
    }
}

pub(crate) fn opt_params(m: &Map<String, Value>) -> Result<Map<String, Value>, RpcError> {
    match m.get("params") {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(o)) => Ok(o.clone()),
        Some(_) => Err(bad("params must be an object")),
    }
}

pub(crate) fn object_id(m: &Map<String, Value>, key: &str) -> Result<ObjectId, RpcError> {
    m.get(key)
        .and_then(AsUid::as_uid)
        .map(ObjectId)
        .ok_or_else(|| bad(format!("{key} must be an object ID")))
}
