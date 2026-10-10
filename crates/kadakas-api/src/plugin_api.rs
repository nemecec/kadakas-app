//! Agent API for scripting: plug-in objects (`plugin.*`) and scripts that run next to the app (`script.*`).
//!
//! A plug-in object definition is a resource of type `plugin_object`. An instance is a group whose
//! members are the shapes that the script made, with the parameters in the group's data under
//! `kadakas.plugin`. Every change is one undo step.
//!
//! Scripts run only when a user starts them. A request from an agent may run a script only if the
//! host allows it (`--allow-agent-scripts`), except for the scripts that ship with the app.

use kadakas_core::uid::AsUid;
use serde_json::{Map, Value, json};

use kadakas_core::command::Command;
use kadakas_core::model::{Attributes, ClassId, LayerId, Object, ObjectId, Shape};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_script::examples;
use kadakas_script::host::{RunSpec, ScriptHost, Status};
use kadakas_script::plugin::{
    self as plug, Generated, Geometry, PluginDef, ResetError, ResetLimits,
};
use kadakas_script::python::{self, Interpreter};
use kadakas_script::trust::{Permit, TrustStore};

use crate::errors::{app, bad};
use crate::{RpcError, Session};

/// The key in the data of an instance.
pub const DATA_KEY: &str = "kadakas.plugin";

pub const PLUGIN_DEFINE: (&str, &str) = (
    "plugin.define",
    "Create a plug-in object definition (a resource). params: {name, shape_type: point | linear | rectangle | path, params?: [{name, label?, type: number | length | text | bool | choice | color | record, default?, min?, max?, options? (choice), fields? (record: the same kind of entries, no nested records)}], script, description?, timeout_ms? (10 to 60000, default 5000)}. The script defines reset(params, geometry) and returns a list of shapes (see describe.scripting). Creating a definition runs nothing. Returns {id}.",
);
pub const PLUGIN_UPDATE: (&str, &str) = (
    "plugin.update",
    "Change a definition. params: {id | name, new_name?, shape_type?, params?, script?, description?, timeout_ms?, reset?: bool}. shape_type cannot change while objects use the definition. With reset: true the objects are regenerated in the same undo step (this runs the script). Returns {id, reset?: [{id, ok, error?}]}.",
);
pub const PLUGIN_DELETE: (&str, &str) = (
    "plugin.delete",
    "Delete a definition that no object uses. params: {id | name}. Returns {ok}.",
);
pub const PLUGIN_LIST: (&str, &str) = (
    "plugin.list",
    "All plug-in object definitions. Returns [{id, name, shape_type, description, params, instances, shipped}].",
);
pub const PLUGIN_GET: (&str, &str) = (
    "plugin.get",
    "One definition with its script. params: {id | name}. Returns {id, name, shape_type, description, params, script, timeout_ms, instances, shipped}.",
);
pub const PLUGIN_EXAMPLES: (&str, &str) = (
    "plugin.examples",
    "The plug-in objects that ship with the app (Bench, Stepping stones) and whether the document has them. Returns [{name, shape_type, description, installed}].",
);
pub const PLUGIN_INSTALL_EXAMPLE: (&str, &str) = (
    "plugin.install_example",
    "Add a shipped plug-in object to the document. params: {name}. Returns {id, existing} (existing is true if the document already had it).",
);
pub const PLUGIN_PLACE: (&str, &str) = (
    "plugin.place",
    "Place a plug-in object: run the script and add the shapes as a group, in one undo step. params: {definition: id | name, params?: {name: value} (missing values use the defaults), layer?, class?, name?} plus the geometry for the shape type: point {at: [x, y], rotation?: radians}; linear {a, b}; rectangle {min, max}; path {points: [[x, y]], closed?: bool}. Nothing is added if the script fails. Returns {id, children: [ids], shapes}.",
);
pub const PLUGIN_SET_PARAMS: (&str, &str) = (
    "plugin.set_params",
    "Change parameters of a plug-in object and regenerate it. params: {id, params: {name: value}} (only the given values change). If the script fails, the old shapes stay, the parameters are saved and the object shows an error. Returns {id, ok, error?, traceback?, children}.",
);
pub const PLUGIN_RESET: (&str, &str) = (
    "plugin.reset",
    "Run the reset again for plug-in objects, for example after a script change. params: {id | ids | definition}. One undo step. Returns {results: [{id, ok, error?}]}.",
);
pub const PLUGIN_OBJECT: (&str, &str) = (
    "plugin.object",
    "The state of a plug-in object. params: {id}. Returns {id, definition, name, shape_type, params, schema, geometry, error, children}. error is null when the last reset worked.",
);
pub const PLUGIN_PREVIEW: (&str, &str) = (
    "plugin.preview",
    "Run a script without changing the document. params: {definition: id | name, params?} plus the geometry, as in plugin.place. Returns {shapes: [...]}.",
);
pub const SCRIPT_INFO: (&str, &str) = (
    "script.info",
    "How scripts run here. Returns {agent_scripts_allowed, python (the interpreter, or null), live_session (true if scripts can connect to this session), limits}.",
);
pub const SCRIPT_RUN: (&str, &str) = (
    "script.run",
    "Start a Python script that connects to this live session (KADAKAS_ADDR and KADAKAS_TOKEN are set). Needs the flag --allow-agent-scripts, and a running app. params: {path | source, args?: [text]}. A path must end in .py and be inside an allowed folder. Returns {run} at once: poll with script.status.",
);
pub const SCRIPT_STATUS: (&str, &str) = (
    "script.status",
    "The state and the console output of a script run. params: {run, since?: line number}. Returns {run, status: running | finished | failed | stopped, exit_code?, lines: [{n, stream: out | err | info, text}], next}.",
);
pub const SCRIPT_STOP: (&str, &str) = (
    "script.stop",
    "Stop a running script. params: {run}. Returns {stopped}.",
);
pub const SCRIPT_LIST: (&str, &str) = (
    "script.list",
    "The script runs of this session. Returns [{run, name, status, seconds}].",
);

/// Scripting state of a session.
pub struct ScriptingState {
    /// May requests from agents run scripts that are not shipped with the app? Default: no.
    pub allow_agent_scripts: bool,
    /// The scripts that run now and their console output.
    pub host: ScriptHost,
    /// Address and token of the live session (`127.0.0.1:port`), for scripts. Set by the app.
    pub endpoint: Option<(String, String)>,
    /// Use this interpreter instead of the one that `python::discover` finds.
    pub interpreter: Option<Interpreter>,
    /// Limits of one reset. A definition can lower or raise the time limit.
    pub limits: ResetLimits,
    /// The scripts that the user trusted. A script that is not here (and not shipped) never runs
    /// from a user action. Only the dialog of the app adds to it, never an API call.
    pub trust: TrustStore,
    /// Private folders for the scripts of `script.run` with `source`. They go away with the session.
    temp_dirs: Vec<tempfile::TempDir>,
}

impl Default for ScriptingState {
    fn default() -> Self {
        ScriptingState {
            allow_agent_scripts: false,
            host: ScriptHost::new(),
            endpoint: None,
            interpreter: None,
            limits: ResetLimits::default(),
            trust: TrustStore::in_memory(),
            temp_dirs: Vec::new(),
        }
    }
}

fn forbidden() -> RpcError {
    RpcError::policy(
        "this script may not run. A user must review and trust it in the app, or the host must allow agents with --allow-agent-scripts. The shipped plug-in objects (Bench, Stepping stones) run anyway",
    )
}

/// The error for a user action on a script that nobody trusted. The app opens the trust dialog.
pub const UNTRUSTED_CODE: i64 = -32002;

fn untrusted() -> RpcError {
    RpcError::new(
        UNTRUSTED_CODE,
        "untrusted",
        "this plug-in script is not trusted. Review it, then trust it, before it can run",
    )
}

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "plugin_objects": "A plug-in object is a definition (name, shape_type, parameter schema, Python script) and any number of objects placed from it. The script defines reset(params, geometry) and returns a list of shapes. params has one entry per parameter (attribute access works: params.width). geometry holds absolute document coordinates in mm: point {at, rotation}, linear {a, b}, rectangle {min, max}, path {points, closed}. Shapes: line(a, b), rect(min, max), circle(center, radius), arc(center, radius, start, end), polyline(points, closed), text(at, text, height) with optional pen, fill and weight (colors as '#rrggbb'). The helpers rotate(), walk(points, spacing, start, closed) and path_length(), and the modules math, are ready to use. The result is stored as a group, so display, export and IFC need no script. Limits: 5 s per reset (timeout_ms), 2000 shapes, 100000 polyline vertices, 2 MiB of output. If the script fails, the old shapes stay and the object shows an error.",
        "agent_scripts": "Scripts run only when a user starts them. A request from an agent may run a script only if the host allows it (--allow-agent-scripts). The shipped plug-in objects always run.",
        "examples": [
            {"method": "plugin.install_example", "params": {"name": "Bench"}},
            {"method": "plugin.place", "params": {"definition": "Bench", "at": [2000, 1500], "rotation": 0.5, "params": {"length": 2000, "back": false}}},
            {"method": "plugin.set_params", "params": {"id": "00000000-0000-0000-0000-00000000000c", "params": {"slats": 7}}},
            {"method": "plugin.place", "params": {"definition": "Stepping stones", "points": [[0, 0], [4000, 0], [6000, 2000]], "params": {"spacing": 600, "stone": "Square"}}}
        ]
    })
}

/// Handles a method of this module. `None` means that the method is not one of ours.
pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !(method.starts_with("plugin.") || method.starts_with("script.")) {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    let Some(m) = p.as_object() else {
        return Some(Err(bad("params must be an object")));
    };
    Some(match method {
        "plugin.define" => define(s, m),
        "plugin.update" => update(s, m),
        "plugin.delete" => delete(s, m),
        "plugin.list" => Ok(list(s)),
        "plugin.get" => get(s, m),
        "plugin.examples" => Ok(examples_list(s)),
        "plugin.install_example" => install_example(s, m),
        "plugin.place" => place(s, m),
        "plugin.set_params" => set_params(s, m),
        "plugin.reset" => reset_many(s, m),
        "plugin.object" => object_info(s, m),
        "plugin.preview" => preview(s, m),
        "script.info" => Ok(script_info(s)),
        "script.run" => script_run(s, m),
        "script.status" => script_status(s, m),
        "script.stop" => script_stop(s, m),
        "script.list" => Ok(script_list(s)),
        other => Err(RpcError::method_not_found(other)),
    })
}

// ---------------------------------------------------------------- definitions

/// A definition found in the document.
struct Found {
    id: ResourceId,
    name: String,
    def: PluginDef,
}

fn plugin_json(body: &ResourceBody) -> Option<&Value> {
    match body {
        ResourceBody::Unknown(v) if v["type"] == plug::RESOURCE_TYPE => Some(v),
        _ => None,
    }
}

fn all_defs(s: &Session) -> Vec<Found> {
    s.editor
        .doc()
        .resources
        .iter()
        .filter_map(|r| {
            let v = plugin_json(&r.body)?;
            Some(Found {
                id: r.id,
                name: r.name.clone(),
                def: PluginDef::from_json(v).ok()?,
            })
        })
        .collect()
}

/// Finds a definition by ID or name (text, compared without case).
fn find_def(s: &Session, sel: &Value) -> Result<Found, RpcError> {
    let defs = all_defs(s);
    let found = match sel {
        Value::String(name) => {
            let id = kadakas_core::uid::Uid::parse(name.trim());
            let by_id = defs.iter().position(|d| Some(d.id.0) == id);
            let at = by_id.or_else(|| defs.iter().position(|d| d.name.eq_ignore_ascii_case(name)));
            at.and_then(|i| defs.into_iter().nth(i))
        }
        _ => return Err(bad("definition must be an ID or a name")),
    };
    found.ok_or_else(|| RpcError::not_found(format!("there is no plug-in object definition {sel}")))
}

fn sel<'a>(m: &'a Map<String, Value>, keys: &[&str]) -> Result<&'a Value, RpcError> {
    keys.iter()
        .find_map(|k| m.get(*k))
        .ok_or_else(|| bad(format!("give {}", keys.join(" or "))))
}

fn instances_of(s: &Session, def: ResourceId) -> Vec<ObjectId> {
    s.editor
        .doc()
        .objects
        .values()
        .filter(|o| instance_data(o).is_some_and(|d| d["definition"] == json!(def.0)))
        .map(|o| o.id)
        .collect()
}

fn def_summary(f: &Found, s: &Session, with_script: bool) -> Value {
    let mut v = json!({
        "id": f.id,
        "name": f.name,
        "shape_type": f.def.shape_type,
        "description": f.def.description,
        "params": f.def.params,
        "instances": instances_of(s, f.id).len(),
        "shipped": examples::is_shipped_script(&f.def.script),
    });
    if with_script {
        v["script"] = json!(f.def.script);
        v["timeout_ms"] = json!(f.def.timeout_ms);
    }
    v
}

fn list(s: &Session) -> Value {
    Value::Array(
        all_defs(s)
            .iter()
            .map(|f| def_summary(f, s, false))
            .collect(),
    )
}

fn get(s: &Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let f = find_def(s, sel(m, &["id", "name"])?)?;
    Ok(def_summary(&f, s, true))
}

fn name_of(m: &Map<String, Value>, key: &str) -> Result<String, RpcError> {
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

fn exec(s: &mut Session, cmd: &Command) -> Result<Vec<kadakas_core::uid::Uid>, RpcError> {
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

fn define(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let name = name_of(m, "name")?;
    if all_defs(s)
        .iter()
        .any(|d| d.name.eq_ignore_ascii_case(&name))
    {
        return Err(app(format!(
            "a plug-in object named '{name}' already exists"
        )));
    }
    let mut body = Value::Object(m.clone());
    body.as_object_mut().expect("object").remove("name");
    let def = PluginDef::from_json(&body).map_err(bad)?;
    let created = exec(
        s,
        &Command::AddResource {
            name,
            body: ResourceBody::Unknown(def.to_json()),
        },
    )?;
    Ok(json!({"id": created.first()}))
}

fn update(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let f = find_def(s, sel(m, &["id", "name"])?)?;
    let mut body = f.def.to_json();
    for k in [
        "shape_type",
        "params",
        "script",
        "description",
        "timeout_ms",
    ] {
        if let Some(v) = m.get(k) {
            body[k] = v.clone();
        }
    }
    let def = PluginDef::from_json(&body).map_err(bad)?;
    let instances = instances_of(s, f.id);
    if def.shape_type != f.def.shape_type && !instances.is_empty() {
        return Err(app(
            "shape_type cannot change while objects use the definition",
        ));
    }
    let new_name = match m.get("new_name") {
        Some(_) => {
            let n = name_of(m, "new_name")?;
            if all_defs(s)
                .iter()
                .any(|d| d.id != f.id && d.name.eq_ignore_ascii_case(&n))
            {
                return Err(app(format!("a plug-in object named '{n}' already exists")));
            }
            Some(n)
        }
        None => None,
    };
    let mut commands = vec![Command::ModifyResource {
        id: f.id,
        name: new_name.clone(),
        body: Some(ResourceBody::Unknown(def.to_json())),
    }];
    let mut results = Vec::new();
    let reset = m.get("reset").and_then(Value::as_bool).unwrap_or(false);
    if reset {
        if !instances.is_empty() && allowed(s, &def).is_err() {
            return Err(forbidden());
        }
        let found = Found {
            id: f.id,
            name: new_name.unwrap_or(f.name),
            def,
        };
        for id in instances {
            let (cmd, outcome) = plan_reset(s, id, &found, None)?;
            commands.push(cmd);
            results.push(outcome.json(id));
        }
    }
    exec(s, &Command::Batch { commands })?;
    let mut out = json!({"id": f.id});
    if reset {
        out["reset"] = json!(results);
    }
    Ok(out)
}

fn delete(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let f = find_def(s, sel(m, &["id", "name"])?)?;
    let n = instances_of(s, f.id).len();
    if n > 0 {
        return Err(app(format!(
            "{n} objects use '{}'. Delete them first",
            f.name
        )));
    }
    exec(
        s,
        &Command::DeleteResource {
            id: f.id,
            reassign: None,
        },
    )?;
    Ok(json!({"ok": true}))
}

fn examples_list(s: &Session) -> Value {
    let have = all_defs(s);
    Value::Array(
        examples::all()
            .into_iter()
            .map(|(name, d)| {
                json!({
                    "name": name,
                    "shape_type": d.shape_type,
                    "description": d.description,
                    "installed": have.iter().any(|f| f.name.eq_ignore_ascii_case(name)),
                })
            })
            .collect(),
    )
}

fn install_example(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let name = name_of(m, "name")?;
    let Some((canonical, def)) = examples::all()
        .into_iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(&name))
    else {
        let names: Vec<&str> = examples::all().iter().map(|(n, _)| *n).collect();
        return Err(bad(format!(
            "there is no shipped plug-in object '{name}'. Available: {}",
            names.join(", ")
        )));
    };
    if let Some(f) = all_defs(s)
        .into_iter()
        .find(|d| d.name.eq_ignore_ascii_case(canonical))
    {
        return Ok(json!({"id": f.id, "existing": true}));
    }
    let created = exec(
        s,
        &Command::AddResource {
            name: canonical.to_string(),
            body: ResourceBody::Unknown(def.to_json()),
        },
    )?;
    Ok(json!({"id": created.first(), "existing": false}))
}

// ------------------------------------------------------------------ instances

fn instance_data(o: &Object) -> Option<&Value> {
    o.data.get(DATA_KEY).filter(|d| d.is_object())
}

/// The lowest corner of the shapes, to notice that someone moved the object.
fn origin_of(shapes: impl Iterator<Item = Shape>) -> [f64; 2] {
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

/// How a reset ended.
struct Outcome {
    ok: bool,
    error: Option<String>,
    traceback: Option<String>,
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

/// The interpreter and limits for a definition.
fn runner(s: &Session, def: &PluginDef) -> Result<(Interpreter, ResetLimits), String> {
    let py = s
        .scripting
        .interpreter
        .clone()
        .or_else(python::discover)
        .ok_or("no Python found: install python3, or uv, or set KADAKAS_PYTHON")?;
    let mut limits = s.scripting.limits;
    if let Some(ms) = def.timeout_ms {
        limits.timeout = std::time::Duration::from_millis(ms);
    }
    Ok((py, limits))
}

fn run_script(
    s: &Session,
    name: &str,
    def: &PluginDef,
    params: &Map<String, Value>,
    geometry: &Geometry,
) -> Result<Vec<Generated>, ResetError> {
    let (py, limits) = runner(s, def).map_err(ResetError::Protocol)?;
    let permit = s
        .scripting
        .trust
        .permit(&def.script, s.scripting.allow_agent_scripts)
        .ok_or_else(|| ResetError::Refused("this script is not trusted".into()))?;
    plug::reset(&permit, &py, name, def, params, geometry, &limits)
}

fn allowed(s: &Session, def: &PluginDef) -> Result<(), RpcError> {
    if s.scripting.allow_agent_scripts || s.scripting.trust.is_trusted(&def.script) {
        Ok(())
    } else {
        Err(forbidden())
    }
}

/// Reads the geometry for the definition from the parameters of a request.
fn geometry_of(def: &PluginDef, m: &Map<String, Value>) -> Result<Geometry, RpcError> {
    Geometry::from_json(def.shape_type, &Value::Object(m.clone())).map_err(bad)
}

fn opt_params(m: &Map<String, Value>) -> Result<Map<String, Value>, RpcError> {
    match m.get("params") {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(o)) => Ok(o.clone()),
        Some(_) => Err(bad("params must be an object")),
    }
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

fn instance_json(
    f: &Found,
    params: &Map<String, Value>,
    geometry: &Geometry,
    origin: [f64; 2],
    error: Option<&str>,
) -> Value {
    json!({
        "definition": f.id,
        "name": f.name,
        "shape_type": f.def.shape_type,
        "params": params,
        "geometry": geometry.to_json(),
        "origin": origin,
        "error": error,
    })
}

fn place(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let f = find_def(s, sel(m, &["definition"])?)?;
    allowed(s, &f.def)?;
    let params = f.def.resolve_params(&opt_params(m)?).map_err(bad)?;
    let geometry = geometry_of(&f.def, m)?;
    let layer = m.get("layer").and_then(AsUid::as_uid).map(LayerId);
    let class = m.get("class").and_then(AsUid::as_uid).map(ClassId);
    let shapes = run_script(s, &f.name, &f.def, &params, &geometry).map_err(app)?;
    let n = shapes.len() as u64;
    let planned = s.editor.doc().next_ids(n as usize + 1);
    let group = ObjectId(planned[n as usize]);
    let origin = origin_of(shapes.iter().map(|g| g.shape.clone()));
    let mut commands = add_cmds(&shapes, layer, class);
    commands.push(Command::Group {
        ids: planned[..n as usize]
            .iter()
            .copied()
            .map(ObjectId)
            .collect(),
        name: Some(match m.get("name") {
            Some(_) => name_of(m, "name")?,
            None => f.name.clone(),
        }),
    });
    commands.push(Command::SetData {
        id: group,
        set: [(
            DATA_KEY.to_string(),
            instance_json(&f, &params, &geometry, origin, None),
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

/// Everything a reset needs, copied out of the session. It can run on another thread, so the
/// window stays alive while a slow script runs. Make it with [`user_prepare`].
pub struct ResetJob {
    pub id: ObjectId,
    found: Found,
    params: Map<String, Value>,
    geometry: Geometry,
    /// The lowest corner of the members now (they may have moved).
    now: [f64; 2],
    children: Vec<ObjectId>,
    interpreter: Option<Interpreter>,
    limits: ResetLimits,
    /// The right to run this exact script. It was made when the job was prepared.
    permit: Permit,
}

impl ResetJob {
    /// Runs the script. This is the only place where Python starts for a reset.
    pub fn run(&self) -> Result<Vec<Generated>, ResetError> {
        let py = self.interpreter.as_ref().ok_or_else(|| {
            ResetError::Protocol(
                "no Python found: install python3, or uv, or set KADAKAS_PYTHON".into(),
            )
        })?;
        plug::reset(
            &self.permit,
            py,
            &self.found.name,
            &self.found.def,
            &self.params,
            &self.geometry,
            &self.limits,
        )
    }
}

/// Collects the data of one reset. Runs nothing. `new_params` are merged over the old values.
fn prepare_reset(
    s: &Session,
    id: ObjectId,
    f: &Found,
    new_params: Option<&Map<String, Value>>,
    permit: Permit,
) -> Result<ResetJob, RpcError> {
    let doc = s.editor.doc();
    let obj = doc
        .object(id)
        .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
    let data = instance_data(obj)
        .ok_or_else(|| RpcError::validation(format!("object {id} is not a plug-in object")))?;
    let mut geometry = Geometry::from_json(f.def.shape_type, &data["geometry"]).map_err(app)?;
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
    let params = f.def.resolve_params(&merged).map_err(bad)?;
    let (interpreter, limits) = match runner(s, &f.def) {
        Ok((py, limits)) => (Some(py), limits),
        Err(_) => (None, s.scripting.limits),
    };
    Ok(ResetJob {
        id,
        found: Found {
            id: f.id,
            name: f.name.clone(),
            def: f.def.clone(),
        },
        params,
        geometry,
        now,
        children,
        interpreter,
        limits,
        permit,
    })
}

/// Turns the result of a reset into commands. A script failure is part of the result: the commands
/// then only save the parameters and the error, and the old shapes stay.
fn finish_reset(job: &ResetJob, result: Result<Vec<Generated>, ResetError>) -> (Command, Outcome) {
    let id = job.id;
    let set_data = |origin: [f64; 2], error: Option<&str>| Command::SetData {
        id,
        set: [(
            DATA_KEY.to_string(),
            instance_json(&job.found, &job.params, &job.geometry, origin, error),
        )]
        .into(),
        remove: vec![],
    };
    match result {
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
            commands.push(Command::DeleteObjects {
                ids: job.children.clone(),
            });
            commands.push(set_data(origin, None));
            (
                Command::Batch { commands },
                Outcome {
                    ok: true,
                    error: None,
                    traceback: None,
                },
            )
        }
        Err(e) => {
            let msg = e.to_string();
            let trace = match &e {
                ResetError::Script { trace, .. } => Some(trace.clone()),
                _ => None,
            };
            (
                set_data(job.now, Some(&msg)),
                Outcome {
                    ok: false,
                    error: Some(msg),
                    traceback: trace,
                },
            )
        }
    }
}

/// Builds the commands that regenerate one object (the caller has checked the permission).
fn plan_reset(
    s: &Session,
    id: ObjectId,
    f: &Found,
    new_params: Option<&Map<String, Value>>,
) -> Result<(Command, Outcome), RpcError> {
    let permit = s
        .scripting
        .trust
        .permit(&f.def.script, s.scripting.allow_agent_scripts)
        .ok_or_else(forbidden)?;
    let job = prepare_reset(s, id, f, new_params, permit)?;
    let result = job.run();
    Ok(finish_reset(&job, result))
}

fn instance_def(s: &Session, id: ObjectId) -> Result<Found, RpcError> {
    let obj = s
        .editor
        .doc()
        .object(id)
        .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
    let data = instance_data(obj)
        .ok_or_else(|| RpcError::validation(format!("object {id} is not a plug-in object")))?;
    find_def(s, &data["definition"])
}

fn object_id(m: &Map<String, Value>, key: &str) -> Result<ObjectId, RpcError> {
    m.get(key)
        .and_then(AsUid::as_uid)
        .map(ObjectId)
        .ok_or_else(|| bad(format!("{key} must be an object ID")))
}

fn set_params(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let id = object_id(m, "id")?;
    let f = instance_def(s, id)?;
    allowed(s, &f.def)?;
    let new = m
        .get("params")
        .and_then(Value::as_object)
        .ok_or_else(|| bad("params must be an object"))?;
    let (cmd, outcome) = plan_reset(s, id, &f, Some(new))?;
    exec(s, &cmd)?;
    let children = s.editor.doc().children(id);
    let mut v = json!({"id": id, "ok": outcome.ok, "children": children});
    if let Some(e) = outcome.error {
        v["error"] = json!(e);
        v["traceback"] = json!(outcome.traceback);
    }
    Ok(v)
}

fn reset_many(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let ids: Vec<ObjectId> = if let Some(def) = m.get("definition") {
        instances_of(s, find_def(s, def)?.id)
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
        let f = instance_def(s, id)?;
        allowed(s, &f.def)?;
        let (cmd, outcome) = plan_reset(s, id, &f, None)?;
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
    let f = instance_def(s, id)?;
    let doc = s.editor.doc();
    let data = instance_data(doc.object(id).expect("checked")).expect("checked");
    Ok(json!({
        "id": id,
        "definition": data["definition"],
        "name": f.name,
        "shape_type": f.def.shape_type,
        "params": data["params"],
        "schema": f.def.params,
        "geometry": data["geometry"],
        "error": data["error"],
        "children": doc.children(id),
        "param_problem": data["params"].as_object().and_then(|m| f.def.resolve_params(m).err()),
        "trusted": s.scripting.trust.is_trusted(&f.def.script),
        "origin": s.scripting.trust.origin(&f.def.script).name(),
        "script": f.def.script,
        "script_hash": kadakas_script::trust::hash(&f.def.script),
    }))
}

fn preview(s: &Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let f = find_def(s, sel(m, &["definition"])?)?;
    allowed(s, &f.def)?;
    let params = f.def.resolve_params(&opt_params(m)?).map_err(bad)?;
    let geometry = geometry_of(&f.def, m)?;
    let shapes = run_script(s, &f.name, &f.def, &params, &geometry).map_err(app)?;
    let list: Vec<Value> = shapes
        .iter()
        .map(|g| {
            let mut v = serde_json::to_value(&g.shape).expect("json");
            if g.attrs != Attributes::default() {
                v["attrs"] = serde_json::to_value(&g.attrs).expect("json");
            }
            v
        })
        .collect();
    Ok(json!({"shapes": list}))
}

// -------------------------------------------------------------------- scripts

fn script_info(s: &Session) -> Value {
    let py = s.scripting.interpreter.clone().or_else(python::discover);
    let l = s.scripting.limits;
    json!({
        "agent_scripts_allowed": s.scripting.allow_agent_scripts,
        "python": py.map(|p| p.describe()),
        "live_session": s.scripting.endpoint.is_some(),
        "limits": {
            "reset_timeout_ms": l.timeout.as_millis() as u64,
            "max_output_bytes": l.max_output,
            "max_shapes": l.max_shapes,
            "max_vertices": l.max_vertices,
        },
    })
}

fn script_run(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    if !s.scripting.allow_agent_scripts {
        return Err(forbidden());
    }
    let endpoint = s.scripting.endpoint.clone().ok_or_else(|| {
        app("scripts need a running Kadakas app: this session has no live address for them")
    })?;
    let args: Vec<String> = match m.get("args") {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| bad("args must be text"))
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(bad("args must be a list of text")),
    };
    let script = match (m.get("path"), m.get("source")) {
        (Some(Value::String(p)), None) => s
            .check_path(std::path::Path::new(p), false, &["py"])
            .map_err(RpcError::policy)?,
        (None, Some(Value::String(src))) => {
            if src.len() > plug::MAX_SCRIPT {
                return Err(bad("the source is too long"));
            }
            // A new private folder (mode 0700), and a file that must not exist yet: no fixed name, no
            // link to follow.
            let mut builder = tempfile::Builder::new();
            builder.prefix("kadakas-script-");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                builder.permissions(std::fs::Permissions::from_mode(0o700));
            }
            let dir = builder.tempdir().map_err(app)?;
            let file = dir.path().join("agent.py");
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&file)
                .and_then(|mut f| std::io::Write::write_all(&mut f, src.as_bytes()))
                .map_err(app)?;
            s.scripting.temp_dirs.push(dir);
            file
        }
        _ => return Err(bad("give path or source")),
    };
    let run = s
        .scripting
        .host
        .start(RunSpec {
            script,
            args,
            endpoint: Some(endpoint),
            interpreter: s.scripting.interpreter.clone(),
        })
        .map_err(app)?;
    Ok(json!({"run": run}))
}

fn run_id(m: &Map<String, Value>) -> Result<u64, RpcError> {
    m.get("run")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("run must be a run number"))
}

fn script_status(s: &Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let run = run_id(m)?;
    let since = m.get("since").and_then(Value::as_u64).unwrap_or(0) as usize;
    let (lines, status) = s
        .scripting
        .host
        .poll(run, since)
        .ok_or_else(|| RpcError::not_found(format!("there is no script run {run}")))?;
    let next = lines.last().map_or(since, |l| l.n + 1);
    let mut v = json!({
        "run": run,
        "status": status.name(),
        "lines": lines.iter().map(|l| json!({"n": l.n, "stream": l.stream.name(), "text": l.text})).collect::<Vec<_>>(),
        "next": next,
    });
    if let Status::Exited(c) = status {
        v["exit_code"] = json!(c);
    }
    Ok(v)
}

fn script_stop(s: &Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    if !s.scripting.allow_agent_scripts {
        return Err(forbidden());
    }
    Ok(json!({"stopped": s.scripting.host.stop(run_id(m)?)}))
}

fn script_list(s: &Session) -> Value {
    Value::Array(
        s.scripting
            .host
            .list()
            .iter()
            .map(|r| json!({"run": r.id, "name": r.name, "status": r.status.name(), "seconds": r.seconds}))
            .collect(),
    )
}

/// Starts a reset that the user asked for (Object Info or Reset). It does not run the script yet:
/// run the job (maybe on another thread), then give it to [`user_finish`]. Only a script that the
/// user trusted (or that ships with the app) may run. The flag for agents does not count here.
/// Otherwise the error code is [`UNTRUSTED_CODE`].
pub fn user_prepare(
    s: &Session,
    id: ObjectId,
    new: &Map<String, Value>,
) -> Result<ResetJob, RpcError> {
    let f = instance_def(s, id)?;
    // The flag for agents does not count for a user action.
    let permit = s
        .scripting
        .trust
        .permit(&f.def.script, false)
        .ok_or_else(untrusted)?;
    prepare_reset(s, id, &f, Some(new), permit)
}

/// Applies the result of a job from [`user_prepare`] as one undo step. Returns `{id, ok, error?}`.
/// It refuses if the object changed while the script ran.
pub fn user_finish(
    s: &mut Session,
    job: ResetJob,
    result: Result<Vec<Generated>, ResetError>,
) -> Result<Value, RpcError> {
    let doc = s.editor.doc();
    let same = doc
        .object(job.id)
        .is_some_and(|o| instance_data(o).is_some())
        && doc.children(job.id) == job.children;
    if !same {
        return Err(RpcError::conflict(
            "the object changed while its script ran. Try again",
        ));
    }
    let (cmd, outcome) = finish_reset(&job, result);
    exec(s, &cmd)?;
    let mut v =
        json!({"id": job.id, "ok": outcome.ok, "children": s.editor.doc().children(job.id)});
    if let Some(e) = outcome.error {
        v["error"] = json!(e);
    }
    Ok(v)
}

/// Changes parameters for the user and waits for the script. The same checks as [`user_prepare`].
pub fn user_set_params(
    s: &mut Session,
    id: ObjectId,
    new: &Map<String, Value>,
) -> Result<Value, RpcError> {
    let job = user_prepare(s, id, new)?;
    let result = job.run();
    user_finish(s, job, result)
}

/// The user trusts the script that the dialog showed. `shown_hash` is the hash (`script_hash` of
/// [`instance_info`]) of the text on the screen. If the script has changed since, nothing is
/// trusted. This is the only way to add trust. There is no API method for it.
pub fn trust_plugin(s: &mut Session, id: ObjectId, shown_hash: &str) -> Result<(), String> {
    let f = instance_def(s, id).map_err(|e| e.message)?;
    if kadakas_script::trust::hash(&f.def.script) != shown_hash {
        return Err("the script changed after you read it. Review it again".into());
    }
    s.scripting
        .trust
        .trust(&f.def.script, &f.name)
        .map_err(|e| format!("cannot save the trust file: {e}"))
}

/// The state of a plug-in object for the app, or `None` if the object is none.
pub fn instance_info(s: &Session, id: ObjectId) -> Option<Value> {
    object_info(s, json!({"id": id}).as_object().unwrap()).ok()
}

#[cfg(test)]
mod tests;
