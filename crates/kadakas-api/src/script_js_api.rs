//! JavaScript scripts in the session (`script.eval`, SCR-08) and script resources (SCR-03, lite).
//!
//! A script runs on QuickJS inside the WebAssembly sandbox of the plug-in host (`kadakas-script-js`).
//! Its only way out is `kadakas.call(method, params)`, which comes here and goes through
//! [`Session::handle`], the same dispatch and the same parameter checks as a JSON-RPC request. So a
//! script can do no more than the API allows, and it needs no consent of its own.
//!
//! - **One undo step.** All changes of a run are merged into one undo step
//!   ([`kadakas_core::command::Editor::squash_last`]). An error takes back the whole run.
//! - **Modes.** `edit` is the default. `read` refuses every call that changes the document (the
//!   call is taken back) and every call that writes a file or uses the network.
//! - **Limits.** Fuel, a time limit for each stretch of the script, memory, the size of the
//!   answer, the number of calls (`kadakas_script_js::ScriptLimits`, set from the limits of the
//!   session) and a time limit for the whole run.
//! - **Trust.** The text that an agent sends in `script.eval` is its own and is trusted for that
//!   call. A script from a resource of the document runs only after a person trusted its exact
//!   text (SHA-256), or when the host lets agents run scripts (`--allow-agent-scripts`). No API
//!   method adds trust: [`trust_script`] is what the dialog of the app calls.
//!
//! The generator hook of PAT-09 is in [`generator`].

use std::sync::{Arc, Mutex};

use kadakas_core::command::Command;
use kadakas_core::model::Document;
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_core::script::ScriptResource;
use kadakas_script_js::{
    CallFailure, HostError, Request, ScriptEngine, ScriptError, ScriptHost, ScriptLimits,
};
use serde_json::{Map, Value, json};

use crate::errors::bad;
use crate::{RpcError, Session};

pub mod generator;

pub const SCRIPT_EVAL: (&str, &str) = (
    "script.eval",
    "Run a JavaScript script in the session. The script is the body of a function, runs in a sandbox (QuickJS in WebAssembly: no files, no network, no clock) and calls the methods of this API: kadakas.call(method, params) returns the result or throws an Error with code, message, data and method. Everything heavy is a native call; the script only coordinates. params: {code | script (the ID or name of a script resource; it must be trusted), mode? (\"edit\" (default) | \"read\": a read-only run refuses every method that changes the document, writes a file or uses the network), args? (any JSON; the script sees it as `args`), seed? (seeds Math.random; the same seed gives the same numbers)}. All changes of the run are one undo step with the change origin script, and an error takes back all of them. Returns {result (the value the script returned; a typed array is written out as numbers), log (what console.log wrote, at most 64 KiB), calls (how many calls the script made), changed, undo_steps (0 or 1), elapsed_ms}. Limits: fuel, a time limit, memory, output size, calls; see script.js_info. See describe, section scripting.javascript.",
);
pub const SCRIPT_JS_INFO: (&str, &str) = (
    "script.js_info",
    "How JavaScript scripts run here. Returns {engine, upstream, toolchain, available, limits: {fuel, segment_ms, run_ms, memory_bytes, output_bytes, max_calls}, agent_scripts_allowed}. available is false when this build has no script engine.",
);
pub const SCRIPT_RESOURCE_CREATE: (&str, &str) = (
    "script_resource.create",
    "Store a script in the document as a resource (SCR-03). params: {name, code, description?}. A script from a document runs only after a person trusted its exact text in the app. Returns {id, name, sha256, trusted}.",
);
pub const SCRIPT_RESOURCE_UPDATE: (&str, &str) = (
    "script_resource.update",
    "Change a script resource. params: {id (ID or name), name?, code?, description?}. A changed text has a new SHA-256 and needs trust again. Returns the script as script_resource.get does.",
);
pub const SCRIPT_RESOURCE_GET: (&str, &str) = (
    "script_resource.get",
    "One script resource. params: {id (ID or name)}. Returns {id, name, language, code, description, sha256, trusted (a person trusted this exact text), runnable (trusted, or the host lets agents run scripts), used_by: {patterns (hardscape styles with a generator that names it)}}.",
);
pub const SCRIPT_RESOURCE_LIST: (&str, &str) = (
    "script_resource.list",
    "The script resources of the document. Returns {scripts: [{id, name, description, sha256, bytes, trusted, runnable}]}. Delete one with resource.delete.",
);

pub const METHOD_NAMES: [&str; 6] = [
    "script.eval",
    "script.js_info",
    "script_resource.create",
    "script_resource.update",
    "script_resource.get",
    "script_resource.list",
];

/// True if this module answers the method.
pub fn knows(method: &str) -> bool {
    METHOD_NAMES.contains(&method)
}

/// The engine of a session: loaded when first needed, and again when the limits changed.
#[derive(Default)]
pub(crate) struct EngineSlot(Mutex<Option<(ScriptLimits, Result<Arc<ScriptEngine>, String>)>>);

impl EngineSlot {
    pub(crate) fn get(&self, limits: ScriptLimits) -> Result<Arc<ScriptEngine>, String> {
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((l, e)) = slot.as_ref()
            && *l == limits
        {
            return e.clone();
        }
        let made = ScriptEngine::new(limits)
            .map(Arc::new)
            .map_err(|e| e.to_string());
        *slot = Some((limits, made.clone()));
        made
    }
}

/// The JavaScript engine of a session, the cache of generated patterns and the generator that
/// is installed for the thread.
#[derive(Default)]
pub struct ScriptJsState {
    pub(crate) engine: Arc<EngineSlot>,
    pub(crate) generator: generator::GeneratorState,
}

/// The limits of the engine for a session.
pub(crate) fn limits_of(s: &Session) -> ScriptLimits {
    let l = &s.limits;
    ScriptLimits {
        fuel: l.script_js_fuel,
        segment_millis: u64::try_from(l.script_timeout.as_millis()).unwrap_or(u64::MAX),
        memory_bytes: l.script_js_memory_bytes,
        output_bytes: l.script_js_output_bytes,
        max_calls: l.script_js_calls,
    }
}

/// The engine of the session.
pub(crate) fn engine(s: &Session) -> Result<Arc<ScriptEngine>, String> {
    s.script_js.engine.get(limits_of(s))
}

/// True if the host lets agents run scripts of a document without the trust of a person.
pub(crate) fn agents_allowed(s: &Session) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        s.scripting.allow_agent_scripts
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = s;
        false
    }
}

pub(crate) fn trusted(s: &Session, sha: &str) -> bool {
    s.wasm_plugins.registry.trust.is_trusted(sha)
}

/// A person trusts the exact text of a script. `shown_sha256` is the hash of the text on the
/// screen; if the script has changed since, nothing is trusted. The dialog of the app calls this;
/// it is not an API method, and the API has none.
pub fn trust_script(s: &mut Session, script: &str, shown_sha256: &str) -> Result<(), String> {
    let (sha, name) = {
        let r = find_script(s.editor.doc(), &Value::String(script.to_string()))
            .map_err(|e| e.message)?;
        (r.1.sha256(), r.0.clone())
    };
    if sha != shown_sha256 {
        return Err("the script changed after you read it. Review it again".into());
    }
    s.wasm_plugins
        .registry
        .trust
        .trust(&sha, &name)
        .map_err(|e| format!("cannot save the trust file: {e}"))?;
    s.script_js.generator.invalidate();
    Ok(())
}

/// A person stops trusting the exact text of a script.
pub fn untrust_script(s: &mut Session, sha256: &str) -> Result<(), String> {
    s.wasm_plugins
        .registry
        .trust
        .forget(sha256)
        .map_err(|e| format!("cannot save the trust file: {e}"))?;
    s.script_js.generator.invalidate();
    Ok(())
}

/// The state of a script for a dialog: name, hash, size and whether it is trusted.
pub fn script_states(s: &Session) -> Vec<ScriptState> {
    s.editor
        .doc()
        .resources
        .iter()
        .filter_map(|r| match &r.body {
            ResourceBody::Script(c) => {
                let sha = c.sha256();
                Some(ScriptState {
                    id: r.id,
                    name: r.name.clone(),
                    trusted: trusted(s, &sha),
                    bytes: c.code.len(),
                    description: c.description.clone(),
                    sha256: sha,
                })
            }
            _ => None,
        })
        .collect()
}

/// A script resource for a dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptState {
    pub id: ResourceId,
    pub name: String,
    pub description: String,
    pub sha256: String,
    pub bytes: usize,
    pub trusted: bool,
}

/// The section `scripting.javascript` of `describe`.
pub fn describe() -> Value {
    json!({
        "summary": "JavaScript (QuickJS) runs inside the WebAssembly sandbox of Kadakas, the same on the desktop, the server and the browser. A script has no files, no network and no clock (Date.now() is 0; Math.random() is a fixed sequence, set it with `seed`). It sees the API of this server and nothing else: kadakas.call(method, params) runs a method of `describe` (the same dispatch and the same checks as a JSON-RPC request) and returns its result, or throws an Error with code, message, data and method.",
        "rule": "Scripts only coordinate. Every heavy operation (geometry, Boolean operations, offsets, clipping, pattern layout, meshing, analysis, rendering, import and export) is a native call that works on many items in one call. If a script needs a loop over many items to do geometry, a native call is missing: tell the user, do not write the loop.",
        "script": "The code is the body of a function. `args` is the value of the parameter args. `return` gives the result (JSON, or typed arrays). console.log writes the log (64 KiB at most). Example: `return kadakas.call(\"objects.list\", {kind: \"circle\"}).length;`",
        "undo": "All changes of a run are one undo step (origin script). An error takes back all of them. mode \"read\" refuses every call that changes the document (it is taken back at once), writes a file or uses the network: use it for checks and reports.",
        "typed_arrays": "Geometry can be a Float64Array with a stride: kadakas.points(arr, 2) marks [x, y] pairs, kadakas.pointsFrom([[x, y], ...]) makes one from a list. A typed array in the parameters of a call is written out as numbers (flat, or as a list of [x, y]) before the method sees it, so it fits any method that takes points. kadakas.call(method, params, {typed: true}) gives long lists of points and numbers in the answer (at least 8 points, or 64 numbers) back as Float64Array with .stride; with it the points do not go through JSON text. A script can return typed arrays too; script.eval writes them out as numbers.",
        "limits": "Fuel (about one unit per WebAssembly instruction; a bytecode of the script costs about 100), a time limit for each stretch of the script between two calls, memory (128 MiB), the size of the answer (16 MiB), the number of calls (1,000,000), and a time limit for the whole run. See script.js_info.",
        "not_allowed": "A script cannot call script.eval, script.run, undo, redo, document.load, collab.* and events.*. In mode read it cannot call export.*, import.*, file.*, gis.*, update.* and the like either.",
        "trust": "A script that an agent sends in script.eval is its own and runs without trust. A script in the document (script_resource.create) runs only after a person trusted its exact text (SHA-256) in the app, or when the host lets agents run scripts (--allow-agent-scripts). No API method adds trust.",
        "resources": "script_resource.create {name, code}, script_resource.update, script_resource.get, script_resource.list. Delete with resource.delete. A hardscape style can use a script as a generator for its units (describe, section unit_patterns, generators).",
        "examples": [
            {"method": "script.eval", "params": {"code": "return kadakas.call('document.summary', {}).layers.length;", "mode": "read"}},
            {"method": "script.eval", "params": {"code": "const ids = []; for (let i = 0; i < args.n; i++) { ids.push(kadakas.call('execute', {command: {op: 'add_object', shape: {kind: 'circle', center: [i * 1000, 0], radius: 300}}}).created[0]); } return ids;", "args": {"n": 3}}},
            {"method": "script_resource.create", "params": {"name": "Ashlar", "code": "return {types: ['a'], rects: {kind: new Uint32Array([0]), data: new Float64Array([500, 250, 1000, 500, 0])}};"}},
            {"method": "script.js_info", "params": {}}
        ]
    })
}

/// Handles a method of this module. `None` means that the method is not one of ours.
pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !knows(method) {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    let Some(m) = p.as_object() else {
        return Some(Err(bad("params must be an object")));
    };
    Some(match method {
        "script.eval" => eval(s, m),
        "script.js_info" => info(s, m),
        "script_resource.create" => resource_create(s, m),
        "script_resource.update" => resource_update(s, m),
        "script_resource.get" => resource_get(s, m),
        "script_resource.list" => resource_list(s, m),
        other => Err(RpcError::method_not_found(other)),
    })
}

fn only(m: &Map<String, Value>, allowed: &[&str]) -> Result<(), RpcError> {
    for k in m.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(bad(format!(
                "unknown parameter '{k}'. This method takes: {}",
                allowed.join(", ")
            )));
        }
    }
    Ok(())
}

fn info(s: &Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    only(m, &[])?;
    let l = limits_of(s);
    Ok(json!({
        "engine": "QuickJS in WebAssembly",
        "upstream": kadakas_script_js::UPSTREAM,
        "toolchain": kadakas_script_js::TOOLCHAIN,
        "available": ScriptEngine::built_in(),
        "limits": {
            "fuel": l.fuel,
            "segment_ms": l.segment_millis,
            "run_ms": u64::try_from(s.limits.script_js_time.as_millis()).unwrap_or(u64::MAX),
            "memory_bytes": l.memory_bytes,
            "output_bytes": l.output_bytes,
            "max_calls": l.max_calls,
        },
        "agent_scripts_allowed": agents_allowed(s),
    }))
}

// -------------------------------------------------------------------- resources

/// A script resource from an ID or a name.
pub(crate) fn find_script<'a>(
    doc: &'a Document,
    v: &Value,
) -> Result<(&'a String, &'a ScriptResource, ResourceId), RpcError> {
    let text = v
        .as_str()
        .ok_or_else(|| bad("a script is named by its ID or its name (text)"))?
        .trim();
    let by_id = kadakas_core::uid::Uid::parse(text).map(ResourceId);
    let found = doc.resources.iter().find_map(|r| match &r.body {
        ResourceBody::Script(c) if Some(r.id) == by_id => Some((&r.name, c.as_ref(), r.id)),
        _ => None,
    });
    found
        .or_else(|| {
            doc.resources.iter().find_map(|r| match &r.body {
                ResourceBody::Script(c) if r.name.eq_ignore_ascii_case(text) => {
                    Some((&r.name, c.as_ref(), r.id))
                }
                _ => None,
            })
        })
        .ok_or_else(|| RpcError::not_found(format!("no script '{text}' in this document")))
}

fn script_json(s: &Session, id: ResourceId) -> Value {
    let doc = s.editor.doc();
    let r = doc.resources.get(id).expect("the script exists");
    let ResourceBody::Script(c) = &r.body else {
        return Value::Null;
    };
    let sha = c.sha256();
    let patterns: Vec<String> = doc
        .resources
        .iter()
        .filter_map(|x| match &x.body {
            ResourceBody::HardscapeStyle(st) => st
                .joints
                .as_ref()
                .and_then(|j| j.generator.as_ref())
                .filter(|g| g.script.eq_ignore_ascii_case(&r.name) || g.script == id.to_string())
                .map(|_| x.name.clone()),
            _ => None,
        })
        .collect();
    json!({
        "id": id,
        "name": r.name,
        "language": c.language,
        "code": c.code,
        "description": c.description,
        "sha256": sha,
        "trusted": trusted(s, &sha),
        "runnable": trusted(s, &sha) || agents_allowed(s),
        "used_by": {"patterns": patterns},
    })
}

fn resource_list(s: &Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    only(m, &[])?;
    let scripts: Vec<Value> = script_states(s)
        .into_iter()
        .map(|x| {
            json!({
                "id": x.id, "name": x.name, "description": x.description,
                "sha256": x.sha256, "bytes": x.bytes, "trusted": x.trusted,
                "runnable": x.trusted || agents_allowed(s),
            })
        })
        .collect();
    Ok(json!({ "scripts": scripts }))
}

fn resource_get(s: &Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    only(m, &["id"])?;
    let id = find_script(
        s.editor.doc(),
        m.get("id").ok_or_else(|| bad("missing 'id'"))?,
    )?
    .2;
    Ok(script_json(s, id))
}

fn text_param(m: &Map<String, Value>, key: &str) -> Result<Option<String>, RpcError> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) => Ok(Some(t.clone())),
        Some(_) => Err(bad(format!("'{key}' must be text"))),
    }
}

fn resource_create(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    only(m, &["name", "code", "description"])?;
    let name = text_param(m, "name")?.ok_or_else(|| bad("missing 'name'"))?;
    if name.trim().is_empty() {
        return Err(bad("'name' must not be empty"));
    }
    let code = text_param(m, "code")?.ok_or_else(|| bad("missing 'code'"))?;
    let mut script = ScriptResource::new(code);
    script.description = text_param(m, "description")?.unwrap_or_default();
    script.validate().map_err(bad)?;
    let exists = s.editor.doc().resources.iter().any(|r| {
        matches!(r.body, ResourceBody::Script(_)) && r.name.eq_ignore_ascii_case(name.trim())
    });
    if exists {
        return Err(RpcError::conflict(format!(
            "a script named '{}' exists already. Use script_resource.update",
            name.trim()
        )));
    }
    let out = s
        .editor
        .execute(&Command::AddResource {
            name: name.trim().to_string(),
            body: ResourceBody::Script(Box::new(script)),
        })
        .map_err(RpcError::from)?;
    let id = ResourceId(out.created[0]);
    s.script_js.generator.invalidate();
    let v = script_json(s, id);
    Ok(json!({"id": id, "name": v["name"], "sha256": v["sha256"], "trusted": v["trusted"]}))
}

fn resource_update(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    only(m, &["id", "name", "code", "description"])?;
    let (cur, id) = {
        let r = find_script(
            s.editor.doc(),
            m.get("id").ok_or_else(|| bad("missing 'id'"))?,
        )?;
        (r.1.clone(), r.2)
    };
    let mut script = cur;
    if let Some(c) = text_param(m, "code")? {
        script.code = c;
    }
    if let Some(d) = text_param(m, "description")? {
        script.description = d;
    }
    script.validate().map_err(bad)?;
    let name = text_param(m, "name")?;
    s.editor
        .execute(&Command::ModifyResource {
            id,
            name,
            body: Some(ResourceBody::Script(Box::new(script))),
        })
        .map_err(RpcError::from)?;
    s.script_js.generator.invalidate();
    Ok(script_json(s, id))
}

// -------------------------------------------------------------------------- eval

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Edit,
    Read,
}

/// Methods that a script never calls: they would end the run, replace the document under it,
/// change the session from outside, or start another script.
const NEVER: [&str; 7] = [
    "script.eval",
    "script.run",
    "undo",
    "redo",
    "document.load",
    "file.open",
    "file.save",
];
const NEVER_PREFIX: [&str; 3] = ["collab.", "events.", "feedback."];
/// Methods that a read-only run never calls: they write files or use the network.
const NOT_READ: [&str; 12] = [
    "export.",
    "import.",
    "file.",
    "gis.",
    "update.",
    "library.",
    "resource.import_from_file",
    "resource.export_to_file",
    "issue.export_bcf",
    "issue.import_bcf",
    "plugins.install",
    "dxf.compare",
];

fn refusal(method: &str, mode: Mode) -> Option<String> {
    if NEVER.contains(&method) || NEVER_PREFIX.iter().any(|p| method.starts_with(p)) {
        return Some(format!(
            "a script cannot call {method}: it would end the run, replace the document under it or change the session from outside"
        ));
    }
    if mode == Mode::Read && NOT_READ.iter().any(|p| method.starts_with(p)) {
        return Some(format!(
            "{method} writes a file or uses the network. A script in mode read does not"
        ));
    }
    None
}

fn failure_of(e: RpcError) -> CallFailure {
    let mut data = json!({"kind": e.kind});
    if let Some(r) = e.reason {
        data["reason"] = json!(r);
    }
    if let Some(d) = e.details {
        data["details"] = d;
    }
    CallFailure {
        code: e.code,
        message: e.message,
        data: Some(data),
    }
}

/// The clock of a run. The browser has none.
struct Clock(#[cfg(not(target_arch = "wasm32"))] std::time::Instant);

impl Clock {
    fn start() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Clock(std::time::Instant::now())
        }
        #[cfg(target_arch = "wasm32")]
        {
            Clock()
        }
    }

    fn millis(&self) -> u64 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX)
        }
        #[cfg(target_arch = "wasm32")]
        {
            0
        }
    }
}

/// Serves the calls of a script from the session.
struct SessionHost<'a> {
    s: &'a mut Session,
    mode: Mode,
    clock: &'a Clock,
    budget_ms: u64,
    /// The calls that came here (the script cannot tell if it was stopped by a limit).
    calls: u64,
}

impl ScriptHost for SessionHost<'_> {
    fn call(&mut self, method: &str, params: Value, _typed: bool) -> Result<Value, HostError> {
        self.calls += 1;
        if let Some(why) = refusal(method, self.mode) {
            return Err(HostError::Failure(failure_of(RpcError::policy(why))));
        }
        if self.budget_ms > 0 && self.clock.millis() > self.budget_ms {
            return Err(HostError::Stop(format!(
                "the script ran for more than {} ms in all",
                self.budget_ms
            )));
        }
        let revision = self.s.editor.revision();
        let depth = self.s.editor.undo_depth();
        let result = self.s.handle(method, params);
        if self.mode == Mode::Read && self.s.editor.revision() != revision {
            let steps = self.s.editor.undo_depth().saturating_sub(depth);
            self.s.editor.rollback(steps);
            return Err(HostError::Failure(failure_of(RpcError::policy(format!(
                "{method} changes the document. A script in mode read does not; the change was taken back"
            )))));
        }
        result.map_err(|e| HostError::Failure(failure_of(e)))
    }
}

fn script_error(e: ScriptError, log: String, calls: u64) -> RpcError {
    let mut details = json!({"log": log, "calls": calls});
    let err = match &e {
        ScriptError::Script {
            message,
            name,
            stack,
            method,
            code,
            data,
        } => {
            details["message"] = json!(message);
            details["name"] = json!(name);
            details["stack"] = json!(stack);
            if let Some(m) = method {
                details["method"] = json!(m);
            }
            if let Some(c) = code {
                details["code"] = json!(c);
            }
            if let Some(d) = data {
                details["data"] = d.clone();
            }
            RpcError::app(&e)
        }
        ScriptError::Limit(_) => RpcError::limit(&e),
        ScriptError::Trap(_) => RpcError::internal(&e),
        ScriptError::Unavailable(_) => RpcError::new(crate::errors::CODE_GENERIC, "unavailable", &e),
    };
    err.with_details(details)
}

fn eval(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    only(m, &["code", "script", "mode", "args", "seed"])?;
    let mode = match m.get("mode") {
        None | Some(Value::Null) => Mode::Edit,
        Some(Value::String(t)) if t == "edit" => Mode::Edit,
        Some(Value::String(t)) if t == "read" => Mode::Read,
        Some(_) => return Err(bad("'mode' must be \"edit\" or \"read\"")),
    };
    let seed = match m.get("seed") {
        None | Some(Value::Null) => 0,
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| bad("'seed' must be a whole number from 0 to 4294967295"))?,
    };
    let args = m.get("args").cloned().unwrap_or(Value::Null);
    let code = match (m.get("code"), m.get("script")) {
        (Some(_), Some(_)) => return Err(bad("give 'code' or 'script', not both")),
        (Some(Value::String(c)), None) => {
            if c.len() > kadakas_core::script::MAX_CODE_BYTES {
                return Err(bad(format!(
                    "the code may have at most {} bytes",
                    kadakas_core::script::MAX_CODE_BYTES
                )));
            }
            c.clone()
        }
        (Some(_), None) => return Err(bad("'code' must be text")),
        (None, Some(v)) => {
            let (name, script, _) = find_script(s.editor.doc(), v)?;
            let sha = script.sha256();
            if !(trusted(s, &sha) || agents_allowed(s)) {
                return Err(RpcError::policy(format!(
                    "the script '{name}' is not trusted. A person must trust its exact text (SHA-256 {sha}) in the app, in the Scripts window. No API method does it"
                )));
            }
            script.code.clone()
        }
        (None, None) => return Err(bad("give 'code' (the text of a script) or 'script' (a script resource)")),
    };
    let engine = engine(s).map_err(|e| {
        RpcError::new(crate::errors::CODE_GENERIC, "unavailable", e)
    })?;
    let clock = Clock::start();
    let budget_ms = u64::try_from(s.limits.script_js_time.as_millis()).unwrap_or(u64::MAX);
    let depth = s.editor.undo_depth();
    let revision = s.editor.revision();
    let origin = s.editor.origin();
    s.editor.set_origin(kadakas_core::change::Origin::Script);
    let host_calls;
    let result = {
        let mut host = SessionHost {
            s,
            mode,
            clock: &clock,
            budget_ms,
            calls: 0,
        };
        let r = engine.run(
            &Request {
                code: &code,
                args: &args,
                buffers: &[],
                seed,
            },
            &mut host,
        );
        host_calls = host.calls;
        r
    };
    let steps = s.editor.undo_depth().saturating_sub(depth);
    match result {
        Ok(out) => {
            let merged = if steps > 1 {
                s.editor.squash_last(steps, "script.eval")
            } else {
                steps
            };
            s.editor.set_origin(origin);
            let value = out
                .json()
                .map_err(|e| RpcError::validation(format!("the result cannot be written out: {e}")))?;
            let _ = merged;
            Ok(json!({
                "result": value,
                "log": out.log,
                "calls": out.calls,
                "changed": s.editor.revision() != revision,
                "undo_steps": s.editor.undo_depth().saturating_sub(depth),
                "elapsed_ms": clock.millis(),
            }))
        }
        Err(f) => {
            s.editor.rollback(steps);
            s.editor.set_origin(origin);
            Err(script_error(f.error, f.log, f.calls.max(host_calls)))
        }
    }
}

#[cfg(all(test, feature = "script-js", not(target_arch = "wasm32")))]
mod tests;
