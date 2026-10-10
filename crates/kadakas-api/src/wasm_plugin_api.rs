//! Agent API for WebAssembly plug-ins (`plugins.*`, PLAT-10).
//!
//! A plug-in is a sandboxed WebAssembly module (see `kadakas-plugin`). The session keeps the
//! installed modules. An instance of a plug-in object is a group whose members are the shapes that
//! the module made, with the parameters in the data of the group under `kadakas.wasm_plugin`. The
//! shapes are plain document data, so a client without the plug-in draws them. Every change goes
//! through commands, so it is one undo step, and its change set has the origin `script`.
//!
//! Trust works as for Python plug-in objects: a module runs only if the user trusted its exact
//! bytes (SHA-256), or the host lets agents run plug-ins (`--allow-agent-scripts`). The app has the
//! dialog; [`trust_module`] is what the dialog calls. The method `plugins.trust` works only with
//! the flag of the host.
//!
//! The method names have `plugins` in the plural, so they stay apart from the `plugin.*` methods of
//! Python plug-in objects.

use std::collections::HashMap;

use kadakas_core::command::Command;
use kadakas_core::model::{ClassId, LayerId, Object, ObjectId};
use kadakas_core::uid::AsUid;
use kadakas_plugin::{Generated, Permit, Plugin, PluginError, Registry, trust};
use kadakas_plugin_abi::{Geometry, Kind};
use serde_json::{Map, Value, json};

use crate::errors::{app, bad};
use crate::plugin_api::{exec, name_of, object_id, opt_params, origin_of, sel};
use crate::{RpcError, Session};

/// The key in the data of an instance.
pub const DATA_KEY: &str = "kadakas.wasm_plugin";

pub const PLUGINS_INFO: (&str, &str) = (
    "plugins.info",
    "How WebAssembly plug-ins run here. Returns {engine (wasmtime, or unavailable in the browser build), agent_plugins_allowed, installed, limits: {fuel, memory_bytes, max_module_bytes, max_output_bytes, max_shapes, max_vertices}, abi_version}.",
);
pub const PLUGINS_LIST: (&str, &str) = (
    "plugins.list",
    "The installed plug-ins. Returns [{id, name, version, abi_version, description, kinds: [object | tool], permissions, hash (SHA-256 of the module), size, trusted, shape_type, params (the schema), instances}]. Only kind object can be placed in this version.",
);
pub const PLUGINS_INSTALL: (&str, &str) = (
    "plugins.install",
    "Load a plug-in module (.wasm) from a file. params: {path}. A path must end in .wasm and be inside an allowed folder. Loading runs the module's describe function inside the sandbox, but nothing else: placing objects needs trust. A module that imports anything, asks for a permission, or has a bad manifest is refused. Installing the same module again changes nothing. Returns the entry of plugins.list plus {existing}.",
);
pub const PLUGINS_TRUST: (&str, &str) = (
    "plugins.trust",
    "Trust an installed module, so that its objects can be placed and reset. params: {plugin: id | name, hash} where hash is the SHA-256 of the module as plugins.list shows it (you confirm what you reviewed). Needs the flag --allow-agent-scripts. The trust lasts until the session ends and is not saved. Without the flag, only a user can trust a plug-in, in the dialog of the app, and that trust is saved. Returns {id, hash, trusted, scope: session}.",
);
pub const PLUGINS_REMOVE: (&str, &str) = (
    "plugins.remove",
    "Remove an installed plug-in. params: {plugin: id | name}. Fails while objects of the document use it: delete those objects first. Returns {ok}.",
);
pub const PLUGINS_PLACE: (&str, &str) = (
    "plugins.place",
    "Place a plug-in object: run the module and add the shapes as a group, in one undo step with the change origin script. params: {plugin: id | name, params?: {name: value} (missing values use the defaults), layer?, class?, name?} plus the geometry for the shape type of the plug-in: point {at: [x, y], rotation?: radians}; linear {a, b}; rectangle {min, max}; path {points: [[x, y]], closed?: bool}. Nothing is added if the module fails. Returns {id, children: [ids], shapes}.",
);
pub const PLUGINS_SET_PARAMS: (&str, &str) = (
    "plugins.set_params",
    "Change parameters of a plug-in object and regenerate its shapes (one undo step, origin script). params: {id, params: {name: value}} (only the given values change). If the module fails, the old shapes stay, the parameters are saved and the object shows an error. Returns {id, ok, error?, children}.",
);
pub const PLUGINS_RESET: (&str, &str) = (
    "plugins.reset",
    "Run the module again for plug-in objects, for example after installing a new build. params: {id | ids | plugin}. One undo step. Returns {results: [{id, ok, error?}]}.",
);
pub const PLUGINS_OBJECT: (&str, &str) = (
    "plugins.object",
    "The state of a plug-in object. params: {id}. Returns {id, plugin, name, version, hash, installed, trusted, params, schema, geometry, error, children}. error is null when the last reset worked. installed is false when the module is not in this session; the shapes still exist.",
);
pub const PLUGINS_PREVIEW: (&str, &str) = (
    "plugins.preview",
    "Run a module without changing the document. params: {plugin: id | name, params?} plus the geometry, as in plugins.place. Returns {shapes: [...]}.",
);

/// The plug-ins of a session: the installed modules and the trust of the user.
#[derive(Default)]
pub struct WasmPluginState {
    pub registry: Registry,
}

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "summary": "A plug-in is a sandboxed WebAssembly module: it has no files, no network and no clock, it gets a fixed amount of work and memory per call, and it can only turn parameters and a placement into shapes. The same module runs on the desktop, on a server and (later) in the browser. Plug-in objects are stored as groups of plain shapes with the parameters in the group data, so a client without the plug-in still draws them. Python stays for automation scripts. To write a plug-in, see docs/plugins.md.",
        "trust": "A module runs only if a user trusted its exact bytes (SHA-256), or the host lets agents run plug-ins (--allow-agent-scripts). plugins.trust needs the flag and the hash that plugins.list shows.",
        "limits": "Per call: fuel (an endless loop stops), a memory cap, at most 2 MiB of output, 2000 shapes and 100000 polyline vertices. See plugins.info.",
        "examples": [
            {"method": "plugins.install", "params": {"path": "/home/me/plugins/raised_planter.wasm"}},
            {"method": "plugins.list", "params": {}},
            {"method": "plugins.trust", "params": {"plugin": "org.kadakas.raised-planter", "hash": "<hash from plugins.list>"}},
            {"method": "plugins.place", "params": {"plugin": "org.kadakas.raised-planter", "min": [0, 0], "max": [2400, 900], "params": {"wall": 150, "corners": "Cut"}}},
            {"method": "plugins.set_params", "params": {"id": "00000000-0000-0000-0000-00000000000c", "params": {"height": 600}}},
            {"method": "plugins.remove", "params": {"plugin": "org.kadakas.raised-planter"}}
        ]
    })
}

/// Handles a method of this module. `None` means that the method is not one of ours.
pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !method.starts_with("plugins.") {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    let Some(m) = p.as_object() else {
        return Some(Err(bad("params must be an object")));
    };
    Some(match method {
        "plugins.info" => Ok(info(s)),
        "plugins.list" => Ok(list(s)),
        "plugins.install" => install(s, m),
        "plugins.trust" => trust_method(s, m),
        "plugins.remove" => remove(s, m),
        "plugins.place" => place(s, m),
        "plugins.set_params" => set_params(s, m),
        "plugins.reset" => reset_many(s, m),
        "plugins.object" => object_info(s, m),
        "plugins.preview" => preview(s, m),
        other => Err(RpcError::method_not_found(other)),
    })
}

// ------------------------------------------------------------------- helpers

fn engine_error(e: PluginError) -> RpcError {
    match e {
        PluginError::Invalid(_) => bad(e),
        PluginError::Refused(_) => RpcError::policy(e),
        PluginError::Limit(_) => RpcError::limit(e),
        _ => app(e),
    }
}

fn registry(s: &Session) -> &Registry {
    &s.wasm_plugins.registry
}

/// The data of an instance.
fn instance_data(o: &Object) -> Option<&Value> {
    o.data.get(DATA_KEY).filter(|d| d.is_object())
}

fn plugin_id_of(o: &Object) -> Option<&str> {
    instance_data(o)?["plugin"].as_str()
}

/// The ids of the plug-ins that objects of the document use, with the number of objects.
fn usage(s: &Session) -> HashMap<String, usize> {
    let mut n = HashMap::new();
    for o in s.editor.doc().objects.values() {
        if let Some(id) = plugin_id_of(o) {
            *n.entry(id.to_string()).or_insert(0) += 1;
        }
    }
    n
}

fn instances_of(s: &Session, plugin: &str) -> Vec<ObjectId> {
    s.editor
        .doc()
        .objects
        .values()
        .filter(|o| plugin_id_of(o) == Some(plugin))
        .map(|o| o.id)
        .collect()
}

/// Finds an installed plug-in by id or by name (without case).
fn find<'a>(s: &'a Session, sel: &Value) -> Result<&'a Plugin, RpcError> {
    let text = sel
        .as_str()
        .ok_or_else(|| bad("plugin must be an id or a name"))?;
    let reg = registry(s);
    reg.get(text)
        .or_else(|| {
            reg.iter()
                .find(|p| p.manifest().name.eq_ignore_ascii_case(text))
        })
        .ok_or_else(|| RpcError::not_found(format!("there is no installed plug-in '{text}'")))
}

fn permit(s: &Session, p: &Plugin) -> Result<Permit, RpcError> {
    registry(s)
        .trust
        .permit(p.hash(), s.scripting.allow_agent_scripts)
        .ok_or_else(|| {
            RpcError::policy(format!(
                "the plug-in module '{}' (SHA-256 {}) is not trusted. A user must trust it in the app, or the host must allow agents with --allow-agent-scripts",
                p.manifest().id,
                p.hash()
            ))
        })
}

fn summary(s: &Session, p: &Plugin, used: &HashMap<String, usize>) -> Value {
    let m = p.manifest();
    json!({
        "id": m.id,
        "name": m.name,
        "version": m.version,
        "abi_version": m.abi_version,
        "description": m.description,
        "kinds": m.kinds.iter().map(|k| k.name()).collect::<Vec<_>>(),
        "permissions": m.permissions.iter().map(|k| k.name()).collect::<Vec<_>>(),
        "hash": p.hash(),
        "size": p.size(),
        "trusted": registry(s).trust.is_trusted(p.hash()),
        "shape_type": m.object.as_ref().map(|o| o.shape_type.name()),
        "params": m.object.as_ref().map(|o| o.params.clone()).unwrap_or_default(),
        "instances": used.get(&m.id).copied().unwrap_or(0),
    })
}

fn info(s: &Session) -> Value {
    let l = registry(s).limits();
    json!({
        "engine": registry(s).engine(),
        "agent_plugins_allowed": s.scripting.allow_agent_scripts,
        "installed": registry(s).len(),
        "abi_version": kadakas_plugin_abi::ABI_VERSION,
        "limits": {
            "fuel": l.fuel,
            "memory_bytes": l.memory_bytes,
            "max_module_bytes": l.max_module_bytes,
            "max_output_bytes": l.max_output_bytes,
            "max_shapes": l.max_shapes,
            "max_vertices": l.max_vertices,
        },
    })
}

fn list(s: &Session) -> Value {
    let used = usage(s);
    Value::Array(registry(s).iter().map(|p| summary(s, p, &used)).collect())
}

// ------------------------------------------------------------ install, trust

fn install(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let path = m
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("path is required"))?;
    let file = s
        .check_path(std::path::Path::new(path), false, &["wasm"])
        .map_err(RpcError::policy)?;
    let max = registry(s).limits().max_module_bytes;
    let len = std::fs::metadata(&file).map_err(app)?.len();
    if len > max as u64 {
        return Err(RpcError::limit(format!(
            "the module has {len} bytes. The limit is {max}"
        )));
    }
    let bytes = std::fs::read(&file).map_err(app)?;
    let existing = registry(s).by_hash(&trust::hash(&bytes)).is_some();
    let used = usage(s);
    let id = s
        .wasm_plugins
        .registry
        .install(&bytes, |id| used.contains_key(id))
        .map_err(engine_error)?
        .manifest()
        .id
        .clone();
    let p = registry(s).get(&id).expect("just installed");
    let mut v = summary(s, p, &used);
    v["existing"] = json!(existing);
    Ok(v)
}

fn trust_method(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    if !s.scripting.allow_agent_scripts {
        return Err(RpcError::policy(
            "only a user can trust a plug-in module, in the dialog of the app. Or the host must allow agents with --allow-agent-scripts",
        ));
    }
    let (id, hash) = {
        let p = find(s, sel(m, &["plugin", "id"])?)?;
        (p.manifest().id.clone(), p.hash().to_string())
    };
    let given = m.get("hash").and_then(Value::as_str).ok_or_else(|| {
        bad("hash is required: the SHA-256 of the module, as plugins.list shows it")
    })?;
    if !given.eq_ignore_ascii_case(&hash) {
        return Err(RpcError::conflict(
            "the hash is not the hash of the installed module. Read plugins.list again",
        ));
    }
    // For this session only: an agent cannot make trust that stays in the file of the user.
    s.wasm_plugins.registry.trust.trust_for_session(&hash);
    Ok(json!({"id": id, "hash": hash, "trusted": true, "scope": "session"}))
}

/// The user trusts the module that the dialog showed. `shown_hash` is the hash of the module on
/// the screen (`hash` in `plugins.list`). If the installed module has changed since, nothing is
/// trusted. The dialog of the app calls this; it is not an API method.
pub fn trust_module(s: &mut Session, plugin_id: &str, shown_hash: &str) -> Result<(), String> {
    let (hash, name) = {
        let p = registry(s)
            .get(plugin_id)
            .ok_or_else(|| format!("there is no installed plug-in '{plugin_id}'"))?;
        (p.hash().to_string(), p.manifest().name.clone())
    };
    if hash != shown_hash {
        return Err("the module changed after you read it. Review it again".into());
    }
    s.wasm_plugins
        .registry
        .trust
        .trust(&hash, &name)
        .map_err(|e| format!("cannot save the trust file: {e}"))
}

fn remove(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let id = find(s, sel(m, &["plugin", "id"])?)?.manifest().id.clone();
    let n = instances_of(s, &id).len();
    if n > 0 {
        return Err(app(format!(
            "{n} objects use the plug-in '{id}'. Delete them first"
        )));
    }
    s.wasm_plugins.registry.remove(&id);
    Ok(json!({"ok": true}))
}

// ---------------------------------------------------------------- instances

/// The facts about a plug-in that an instance stores.
struct Meta {
    id: String,
    name: String,
    version: String,
    hash: String,
}

impl Meta {
    fn of(p: &Plugin) -> Meta {
        Meta {
            id: p.manifest().id.clone(),
            name: p.manifest().name.clone(),
            version: p.manifest().version.clone(),
            hash: p.hash().to_string(),
        }
    }
}

fn instance_json(
    meta: &Meta,
    params: &Map<String, Value>,
    geometry: &Geometry,
    origin: [f64; 2],
    error: Option<&str>,
) -> Value {
    json!({
        "plugin": meta.id,
        "name": meta.name,
        "version": meta.version,
        "hash": meta.hash,
        "shape_type": geometry.shape_type().name(),
        "params": params,
        "geometry": geometry.to_value(),
        "origin": origin,
        "error": error,
    })
}

/// The object part of a plug-in, or an error if it provides none.
fn object_spec(p: &Plugin) -> Result<&kadakas_plugin_abi::ObjectSpec, RpcError> {
    match (&p.manifest().object, p.manifest().provides(Kind::Object)) {
        (Some(spec), true) => Ok(spec),
        _ => Err(app(format!(
            "the plug-in '{}' provides no object, so it cannot be placed in this version",
            p.manifest().id
        ))),
    }
}

fn geometry_of(p: &Plugin, m: &Map<String, Value>) -> Result<Geometry, RpcError> {
    let spec = object_spec(p)?;
    Geometry::from_value(spec.shape_type, &Value::Object(m.clone())).map_err(bad)
}

fn add_cmds(shapes: &[Generated], layer: Option<LayerId>, class: Option<ClassId>) -> Vec<Command> {
    shapes
        .iter()
        .map(|g| Command::AddObject {
            shape: g.shape.clone(),
            layer,
            class,
            attrs: g.attrs.clone(),
        })
        .collect()
}

fn place(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let (meta, params, geometry, shapes) = {
        let p = find(s, sel(m, &["plugin"])?)?;
        object_spec(p)?;
        let permit = permit(s, p)?;
        let params = p.manifest().resolve_params(&opt_params(m)?).map_err(bad)?;
        let geometry = geometry_of(p, m)?;
        let shapes = p.reset(&permit, &params, &geometry).map_err(engine_error)?;
        (Meta::of(p), params, geometry, shapes)
    };
    let layer = m.get("layer").and_then(AsUid::as_uid).map(LayerId);
    let class = m.get("class").and_then(AsUid::as_uid).map(ClassId);
    let n = shapes.len();
    let planned = s.editor.doc().next_ids(n + 1);
    let group = ObjectId(planned[n]);
    let origin = origin_of(shapes.iter().map(|g| g.shape.clone()));
    let mut commands = add_cmds(&shapes, layer, class);
    commands.push(Command::Group {
        ids: planned[..n].iter().copied().map(ObjectId).collect(),
        name: Some(match m.get("name") {
            Some(_) => name_of(m, "name")?,
            None => meta.name.clone(),
        }),
    });
    commands.push(Command::SetData {
        id: group,
        set: [(
            DATA_KEY.to_string(),
            instance_json(&meta, &params, &geometry, origin, None),
        )]
        .into(),
        remove: vec![],
    });
    let created = exec(s, &Command::Batch { commands })?;
    if created.last() != Some(&group.0) {
        // The IDs did not come out as planned. Take the step back, so no half-made object stays.
        s.editor.undo();
        s.prune_selection();
        return Err(app("could not place the object (unexpected IDs)"));
    }
    s.selection = vec![group];
    Ok(json!({"id": group, "children": created[..created.len() - 1], "shapes": n}))
}

/// How a reset ended.
struct Outcome {
    ok: bool,
    error: Option<String>,
}

impl Outcome {
    fn json(&self, id: ObjectId) -> Value {
        let mut v = json!({"id": id, "ok": self.ok});
        if let Some(e) = &self.error {
            v["error"] = json!(e);
        }
        v
    }
}

/// Builds the commands that regenerate one object. A failure of the module is part of the result:
/// the command then only saves the parameters and the error, and the old shapes stay.
fn plan_reset(
    s: &Session,
    id: ObjectId,
    new_params: Option<&Map<String, Value>>,
) -> Result<(Command, Outcome), RpcError> {
    let doc = s.editor.doc();
    let obj = doc
        .object(id)
        .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
    let data = instance_data(obj)
        .ok_or_else(|| RpcError::validation(format!("object {id} is not a plug-in object")))?;
    let plugin_id = data["plugin"].as_str().unwrap_or("");
    let p = registry(s).get(plugin_id).ok_or_else(|| {
        RpcError::not_found(format!(
            "the plug-in '{plugin_id}' is not installed. Install it with plugins.install. The shapes of the object stay as they are"
        ))
    })?;
    let spec = object_spec(p)?;
    let permit = permit(s, p)?;
    let mut geometry = Geometry::from_value(spec.shape_type, &data["geometry"]).map_err(app)?;
    // Someone may have moved the group. Follow the shapes.
    let children = doc.children(id);
    let old_origin = data["origin"]
        .as_array()
        .and_then(|a| Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?]))
        .unwrap_or([0.0, 0.0]);
    let now = origin_of(
        children
            .iter()
            .filter_map(|c| doc.object(*c))
            .map(|o| o.shape.clone()),
    );
    let (dx, dy) = (now[0] - old_origin[0], now[1] - old_origin[1]);
    if !children.is_empty() && (dx.abs() > 1e-6 || dy.abs() > 1e-6) {
        geometry.translate(dx, dy);
    }
    let mut merged = data["params"].as_object().cloned().unwrap_or_default();
    if let Some(n) = new_params {
        merged.extend(n.clone());
    }
    let params = p.manifest().resolve_params(&merged).map_err(bad)?;
    let meta = Meta::of(p);
    let result = p.reset(&permit, &params, &geometry);

    let set_data = |origin: [f64; 2], error: Option<&str>| Command::SetData {
        id,
        set: [(
            DATA_KEY.to_string(),
            instance_json(&meta, &params, &geometry, origin, error),
        )]
        .into(),
        remove: vec![],
    };
    Ok(match result {
        Ok(shapes) => {
            let origin = origin_of(shapes.iter().map(|g| g.shape.clone()));
            let mut commands: Vec<Command> = shapes
                .iter()
                .map(|g| Command::AddObjectIn {
                    parent: id,
                    shape: g.shape.clone(),
                    class: None,
                    attrs: g.attrs.clone(),
                })
                .collect();
            // New members first: a group that loses its last member would go.
            commands.push(Command::DeleteObjects { ids: children });
            commands.push(set_data(origin, None));
            (
                Command::Batch { commands },
                Outcome {
                    ok: true,
                    error: None,
                },
            )
        }
        Err(e) => {
            let msg = e.to_string();
            (
                set_data(now, Some(&msg)),
                Outcome {
                    ok: false,
                    error: Some(msg),
                },
            )
        }
    })
}

fn set_params(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let id = object_id(m, "id")?;
    let new = m
        .get("params")
        .and_then(Value::as_object)
        .ok_or_else(|| bad("params must be an object"))?;
    let (cmd, outcome) = plan_reset(s, id, Some(new))?;
    exec(s, &cmd)?;
    let mut v = json!({"id": id, "ok": outcome.ok, "children": s.editor.doc().children(id)});
    if let Some(e) = outcome.error {
        v["error"] = json!(e);
    }
    Ok(v)
}

fn reset_many(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let ids: Vec<ObjectId> = if let Some(p) = m.get("plugin") {
        let id = find(s, p)?.manifest().id.clone();
        instances_of(s, &id)
    } else if let Some(Value::Array(a)) = m.get("ids") {
        a.iter()
            .map(|v| {
                v.as_uid()
                    .map(ObjectId)
                    .ok_or_else(|| bad("ids must be object IDs"))
            })
            .collect::<Result<_, _>>()?
    } else {
        vec![object_id(m, "id")?]
    };
    let mut commands = Vec::new();
    let mut results = Vec::new();
    for id in ids {
        let (cmd, outcome) = plan_reset(s, id, None)?;
        commands.push(cmd);
        results.push(outcome.json(id));
    }
    if !commands.is_empty() {
        exec(s, &Command::Batch { commands })?;
    }
    Ok(json!({"results": results}))
}

fn object_info(s: &Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let id = object_id(m, "id")?;
    let doc = s.editor.doc();
    let obj = doc
        .object(id)
        .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
    let data = instance_data(obj)
        .ok_or_else(|| RpcError::validation(format!("object {id} is not a plug-in object")))?;
    let plugin = data["plugin"].as_str().and_then(|i| registry(s).get(i));
    Ok(json!({
        "id": id,
        "plugin": data["plugin"],
        "name": data["name"],
        "version": data["version"],
        "hash": data["hash"],
        "installed": plugin.is_some(),
        "trusted": plugin.is_some_and(|p| registry(s).trust.is_trusted(p.hash())),
        "params": data["params"],
        "schema": plugin.and_then(|p| p.manifest().object.as_ref()).map(|o| o.params.clone()),
        "geometry": data["geometry"],
        "error": data["error"],
        "children": doc.children(id),
    }))
}

fn preview(s: &Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let p = find(s, sel(m, &["plugin"])?)?;
    object_spec(p)?;
    let permit = permit(s, p)?;
    let params = p.manifest().resolve_params(&opt_params(m)?).map_err(bad)?;
    let geometry = geometry_of(p, m)?;
    let shapes = p.reset(&permit, &params, &geometry).map_err(engine_error)?;
    let list: Vec<Value> = shapes
        .iter()
        .map(|g| {
            let mut v = serde_json::to_value(&g.shape).expect("json");
            if g.attrs != kadakas_core::model::Attributes::default() {
                v["attrs"] = serde_json::to_value(&g.attrs).expect("json");
            }
            v
        })
        .collect();
    Ok(json!({"shapes": list}))
}

#[cfg(test)]
mod tests;
