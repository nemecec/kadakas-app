//! JavaScript scripts in the session (`script.eval`, SCR-08).
//!
//! A script runs on QuickJS inside the WebAssembly sandbox of the plug-in host (`kadakas-script-js`).
//! Its only way out is `kadakas.call(method, params)`, which comes here and goes through
//! [`Session::handle`], the same dispatch and the same parameter checks as a JSON-RPC request.
//! So a script can do no more than the API allows. On top of that, a **policy table**
//! ([`policy`]) classifies every method by what it can touch, and a method that nobody
//! classified is denied.
//!
//! - **One undo step.** All changes of a run are merged into one undo step
//!   ([`kadakas_core::command::Editor::squash_run`]). An error takes back the whole run, and the
//!   log of changes shows the run once. While a run is open, a call from outside the run is refused
//!   (a conflict), so nothing else is merged into it.
//! - **Modes.** `edit` is the default. `read` may call read-only methods only; any other method
//!   is refused before it runs.
//! - **Limits.** Fuel, a time limit for each stretch of the script between two calls, memory, the
//!   size of the answer and of each call, the number of calls (`kadakas_script_js::ScriptLimits`,
//!   set from the limits of the session), and a time limit for the whole run that is measured at
//!   every call and at every tick of the clock of the engine.
//! - **No code in documents.** The code is the text of the call. A document holds no scripts and
//!   runs none (PLAT-15): a generator pattern stores the last result of a generator and runs no
//!   code when it is drawn, counted or exported.
//! - **Threads.** [`Session::script_eval`] runs the engine on the thread of the caller. A host
//!   with a window runs it on a worker thread: [`Session::script_start`] and
//!   [`Session::script_pump`] (the worker asks for the calls, the thread of the session serves
//!   them).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use kadakas_core::change::Origin;
use kadakas_core::command::RunMark;
use kadakas_script_js::{
    CallFailure, HostError, Output, Request, ScriptEngine, ScriptError, ScriptLimits,
};
use serde_json::{Map, Value, json};

use crate::errors::bad;
use crate::{RpcError, Session};

pub mod policy;
#[cfg(not(target_arch = "wasm32"))]
mod run;
#[cfg(not(target_arch = "wasm32"))]
pub use run::ScriptRun;

pub const SCRIPT_EVAL: (&str, &str) = (
    "script.eval",
    "Run a JavaScript program in the session. The code is the body of a function, runs in a sandbox (QuickJS in WebAssembly: no files, no network, no clock) and calls the methods of this API: kadakas.call(method, params) returns the result or throws an Error with code, message, data and method. Everything heavy is a native call; the program only coordinates. params: {code, mode? (\"edit\" (default) | \"read\": a read-only run may call read-only methods only; any other method is refused before it runs), args? (any JSON; the program sees it as `args`), seed? (seeds Math.random; the same seed gives the same numbers)}. The program may call every method except the ones that would end the run or change the session from outside (script.*, undo, redo, document.load, collab.*, events.*, feedback.*, plugin.* (Python)); files, the network and plug-in installs follow the policy of the session, as for a direct call. All changes of the run are one undo step with the change origin script, and an error takes back all of them. While the run is open, calls from outside are refused (a conflict). Returns {result (the value the program returned; a typed array is written out as numbers), log (what console.log wrote, at most 64 KiB), calls (how many calls the program made), changed, undo_steps (0 or 1), elapsed_ms}. Limits: fuel, a time limit for each stretch and for the whole run, memory, the size of the answer and of each call, the number of calls; see script.js_info. A document holds no code: a generator pattern stores the shapes that a program made (describe, section unit_patterns). See describe, section scripting.javascript.",
);
pub const SCRIPT_JS_INFO: (&str, &str) = (
    "script.js_info",
    "How JavaScript programs run here. Returns {engine, upstream, toolchain, available, limits: {fuel, segment_ms (time for each stretch of the program between two calls), run_ms (time for the whole run), memory_bytes, output_bytes, max_calls}, agent_scripts_allowed}. available is false when this build has no script engine.",
);

pub const METHOD_NAMES: [&str; 2] = ["script.eval", "script.js_info"];

/// True if this module answers the method.
pub fn knows(method: &str) -> bool {
    METHOD_NAMES.contains(&method)
}

/// The engine of a session: loaded when first needed, and again when the limits changed.
#[derive(Default)]
pub(crate) struct EngineSlot(Mutex<Option<Loaded>>);

/// The engine for some limits, or why it could not be made.
type Loaded = (ScriptLimits, Result<Arc<ScriptEngine>, String>);

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

/// The run that is open in a session.
struct ActiveRun {
    mark: RunMark,
    origin: Origin,
    /// Tells this run from every other (a document that was replaced has no such run).
    id: u64,
    /// Set when the handle of the run was dropped without an end: the session takes the run back
    /// at its next call.
    abandoned: Arc<AtomicBool>,
}

/// The JavaScript engine of a session, and the run that is open.
#[derive(Default)]
pub struct ScriptJsState {
    pub(crate) engine: Arc<EngineSlot>,
    /// The run that is open: the commands of the session belong to it.
    current: Option<ActiveRun>,
    /// The session is serving a call of the run right now.
    serving: bool,
}

impl ScriptJsState {
    /// True if a run is open and the call that comes now is not one of its own.
    pub(crate) fn busy(&self) -> bool {
        self.current.is_some() && !self.serving
    }

    /// True if a run is open.
    pub(crate) fn open(&self) -> bool {
        self.current.is_some()
    }
}

impl Session {
    /// True while a JavaScript run is open. A host must not replace the document, open or save a
    /// file, run a job that writes to the document, or take a copy for recovery meanwhile: the
    /// session refuses its own calls, but these go around `handle`.
    pub fn script_running(&self) -> bool {
        self.script_js.open()
    }

    /// True if a call that comes now must wait for the run (see `Session::handle`).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn script_js_busy(&mut self) -> bool {
        self.recover_script_run();
        self.script_js.busy()
    }

    /// Takes back a run whose handle was dropped without an end (see [`ScriptRun`]).
    pub(crate) fn recover_script_run(&mut self) {
        let abandoned = self
            .script_js
            .current
            .as_ref()
            .is_some_and(|c| c.abandoned.load(Ordering::Relaxed));
        if abandoned && let Some(c) = self.script_js.current.take() {
            self.script_js.serving = false;
            self.editor.rollback_run(c.mark);
            self.editor.set_origin(c.origin);
        }
    }
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

/// True if the host lets agents run their own programs (`--allow-agent-scripts`). Information only:
/// `script.eval` does not need it.
fn agents_allowed(s: &Session) -> bool {
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

/// The section `scripting.javascript` of `describe`.
pub fn describe() -> Value {
    json!({
        "summary": "JavaScript (QuickJS) runs inside the WebAssembly sandbox of Kadakas, the same on the desktop, the server and the browser. A program has no files, no network and no clock (Date.now() is 0; Math.random() is a fixed sequence, set it with `seed`). It sees the API of this server and nothing else: kadakas.call(method, params) runs a method of `describe` (the same dispatch and the same checks as a JSON-RPC request) and returns its result, or throws an Error with code, message, data and method. No promises or await: the code is synchronous.",
        "rule": "Programs only coordinate. Every heavy operation (geometry, Boolean operations, offsets, clipping, pattern layout, meshing, analysis, rendering, import and export) is a native call that works on many items in one call. If a program needs a loop over many items to do geometry, a native call is missing: tell the user, do not write the loop.",
        "script": "The code is the body of a function. `args` is the value of the parameter args. `return` gives the result (JSON, or typed arrays). console.log writes the log (64 KiB at most). Example: `return kadakas.call(\"objects.list\", {kind: \"circle\"}).length;`",
        "undo": "All changes of a run are one undo step (origin script). An error takes back all of them. While a run is open, a call from outside is refused. mode \"read\" may call read-only methods only and refuses any other before it runs: use it for checks and reports.",
        "policy": "A table classifies every method (read, edit, ui, files, network, install, never). A program cannot call script.*, undo, redo, document.load, collab.*, events.*, feedback.* and plugin.* (Python). In mode read it can call read-only methods only. Files, the network and plug-in installs follow the policy of the session (allowed folders, allowed addresses). A method that is not classified is denied.",
        "typed_arrays": "Geometry can be a Float64Array with a stride: kadakas.points(arr, 2) marks [x, y] pairs, kadakas.pointsFrom([[x, y], ...]) makes one from a list. A typed array in the parameters of a call is written out as numbers (flat, or as a list of [x, y]) before the method sees it, so it fits any method that takes points. kadakas.call(method, params, {typed: true}) gives long lists of points and numbers in the answer (at least 8 points, or 64 numbers) back as Float64Array with .stride; with it the points do not go through JSON text. A program can return typed arrays too; script.eval writes them out as numbers.",
        "limits": "Fuel (about one unit per WebAssembly instruction; a bytecode costs about 100), a time limit for each stretch of the program between two calls (script_timeout, 5 s), a time limit for the whole run (script_js_time, 120 s, measured at every call and at every tick of the clock of the engine on the desktop), memory (128 MiB), the size of the answer and of each call (16 MiB), and the number of calls (1,000,000). See script.js_info.",
        "documents_hold_no_code": "A document holds no program. A generator pattern stores the shapes that a program made, in the pattern (describe, section unit_patterns, generators): drawing, reports and exports use the stored shapes and run no code. To make a result, an agent runs a program with script.eval that computes the shapes and writes them with hardscape_style.create/update or in the pattern of unit_pattern.layout.",
        "examples": [
            {"method": "script.eval", "params": {"code": "return kadakas.call('document.summary', {}).layers.length;", "mode": "read"}},
            {"method": "script.eval", "params": {"code": "const ids = []; for (let i = 0; i < args.n; i++) { ids.push(kadakas.call('execute', {command: {op: 'add_object', shape: {kind: 'circle', center: [i * 1000, 0], radius: 300}}}).created[0]); } return ids;", "args": {"n": 3}}},
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

// -------------------------------------------------------------------------- eval

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

/// The clock of a run. The browser has `Date.now()`; elsewhere it is an `Instant`.
#[derive(Clone, Copy)]
struct Clock {
    #[cfg(not(target_arch = "wasm32"))]
    start: std::time::Instant,
    #[cfg(target_arch = "wasm32")]
    start: f64,
}

impl Clock {
    fn start() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Clock {
                start: std::time::Instant::now(),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            Clock {
                start: js_sys::Date::now(),
            }
        }
    }

    fn millis(&self) -> u64 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
        }
        #[cfg(target_arch = "wasm32")]
        {
            (js_sys::Date::now() - self.start).max(0.0) as u64
        }
    }
}

/// A run that is ready to start: the checked parameters and the engine.
#[derive(Clone)]
struct Prepared {
    code: String,
    read_only: bool,
    args: Value,
    seed: u32,
    /// The engine is made when the run starts, on the thread of the run (the first time takes
    /// about a second).
    engine: Arc<EngineSlot>,
    limits: ScriptLimits,
    /// The time for the whole run, in milliseconds.
    total_ms: u64,
    clock: Clock,
}

impl Prepared {
    /// Why the run must stop now, if it must.
    fn expired(&self) -> Option<String> {
        (self.total_ms > 0 && self.clock.millis() > self.total_ms).then(|| {
            format!(
                "the program ran for more than {} ms in all (script_js_time)",
                self.total_ms
            )
        })
    }

    /// The engine, made now if it was not.
    fn engine(&self) -> Result<Arc<ScriptEngine>, kadakas_script_js::Failure> {
        self.engine
            .get(self.limits)
            .map_err(|e| kadakas_script_js::Failure::from(ScriptError::Unavailable(e)))
    }

    fn request(&self) -> Request<'_> {
        Request {
            code: &self.code,
            args: &self.args,
            buffers: &[],
            seed: self.seed,
        }
    }
}

fn prepare(s: &Session, m: &Map<String, Value>) -> Result<Prepared, RpcError> {
    only(m, &["code", "mode", "args", "seed"])?;
    let read_only = match m.get("mode") {
        None | Some(Value::Null) => false,
        Some(Value::String(t)) if t == "edit" => false,
        Some(Value::String(t)) if t == "read" => true,
        Some(_) => return Err(bad("'mode' must be \"edit\" or \"read\"")),
    };
    let seed = match m.get("seed") {
        None | Some(Value::Null) => 0,
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| bad("'seed' must be a whole number from 0 to 4294967295"))?,
    };
    let code = match m.get("code") {
        Some(Value::String(c)) => c.clone(),
        Some(_) => return Err(bad("'code' must be text")),
        None => return Err(bad("give 'code', the text of a program")),
    };
    if code.len() > MAX_CODE_BYTES {
        return Err(bad(format!(
            "the code may have at most {MAX_CODE_BYTES} bytes"
        )));
    }
    Ok(Prepared {
        code,
        read_only,
        args: m.get("args").cloned().unwrap_or(Value::Null),
        seed,
        engine: s.script_js.engine.clone(),
        limits: limits_of(s),
        total_ms: u64::try_from(s.limits.script_js_time.as_millis()).unwrap_or(u64::MAX),
        clock: Clock::start(),
    })
}

/// The most bytes of the code of one program.
pub const MAX_CODE_BYTES: usize = 256 * 1024;

/// Serves one call of a run on the session: the policy, the time, then `handle`.
fn serve(s: &mut Session, p: &Prepared, method: &str, params: Value) -> Result<Value, HostError> {
    if let Err(why) = policy::check(method, p.read_only) {
        return Err(HostError::Failure(failure_of(RpcError::policy(why))));
    }
    if let Some(why) = p.expired() {
        return Err(HostError::Stop(why));
    }
    s.script_js.serving = true;
    let result = s.handle(method, params);
    s.script_js.serving = false;
    // The history limit drops no step while a run is open. A run that keeps more than the limit
    // allows ends instead.
    if s.editor.undo_bytes() > s.limits.max_history_bytes {
        return Err(HostError::Stop(format!(
            "the run holds more undo data than the session allows ({} bytes, max_history_bytes)",
            s.limits.max_history_bytes
        )));
    }
    result.map_err(|e| HostError::Failure(failure_of(e)))
}

static RUN_IDS: AtomicU64 = AtomicU64::new(1);

/// Opens a run: the commands that follow are one undo step, with the origin `script`.
fn open_run(s: &mut Session) -> (u64, Arc<AtomicBool>) {
    let origin = s.editor.origin();
    s.editor.set_origin(Origin::Script);
    let mark = s.editor.begin_run();
    let id = RUN_IDS.fetch_add(1, Ordering::Relaxed);
    let abandoned = Arc::new(AtomicBool::new(false));
    s.script_js.current = Some(ActiveRun {
        mark,
        origin,
        id,
        abandoned: abandoned.clone(),
    });
    (id, abandoned)
}

/// Ends a run: one undo step if it worked, nothing if it did not. A run that is not the open one
/// (the document was replaced under it) is only reported.
fn close_run(
    s: &mut Session,
    p: &Prepared,
    id: u64,
    revision: u64,
    result: Result<Output, kadakas_script_js::Failure>,
    host_calls: u64,
) -> Result<Value, RpcError> {
    let Some(run) = s.script_js.current.take_if(|c| c.id == id) else {
        return Err(RpcError::conflict(
            "the document was replaced while the script ran; the run was dropped",
        ));
    };
    s.script_js.serving = false;
    let (mark, origin) = (run.mark, run.origin);
    let outcome = match result {
        Ok(out) => {
            if p.read_only && s.editor.run_steps(&mark) > 0 {
                // The table lets read-only methods through; a change here is a mistake of the
                // table, and the run does not keep it.
                s.editor.rollback_run(mark);
                s.editor.set_origin(origin);
                return Err(RpcError::policy(
                    "a program in mode read changed the document; the change was taken back",
                ));
            }
            s.editor.squash_run(mark, "script.eval");
            out.json()
                .map(|value| (value, out.log, out.calls))
                .map_err(|e| RpcError::validation(format!("the result cannot be written out: {e}")))
        }
        Err(f) => {
            s.editor.rollback_run(mark);
            Err(script_error(f.error, f.log, f.calls.max(host_calls)))
        }
    };
    s.editor.set_origin(origin);
    let (value, log, calls) = outcome?;
    Ok(json!({
        "result": value,
        "log": log,
        "calls": calls,
        "changed": s.editor.revision() != revision,
        "undo_steps": usize::from(s.editor.revision() != revision),
        "elapsed_ms": p.clock.millis(),
    }))
}

fn script_error(e: ScriptError, log: String, calls: u64) -> RpcError {
    let mut details = json!({"log": log, "calls": calls});
    let err = match &e {
        ScriptError::Script(t) => {
            details["message"] = json!(t.message);
            details["name"] = json!(t.name);
            details["stack"] = json!(t.stack);
            if let Some(m) = &t.method {
                details["method"] = json!(m);
            }
            if let Some(c) = t.code {
                details["code"] = json!(c);
            }
            if let Some(d) = &t.data {
                details["data"] = d.clone();
            }
            RpcError::app(&e)
        }
        ScriptError::Limit(_) => RpcError::limit(&e),
        ScriptError::Trap(_) => RpcError::internal(&e),
        ScriptError::Unavailable(_) => {
            RpcError::new(crate::errors::CODE_GENERIC, "unavailable", &e)
        }
    };
    err.with_details(details)
}

/// Serves the calls of a run that is on the thread of the session.
struct SyncHost<'a> {
    s: &'a mut Session,
    p: &'a Prepared,
    calls: u64,
}

impl kadakas_script_js::ScriptHost for SyncHost<'_> {
    fn call(&mut self, method: &str, params: Value, _typed: bool) -> Result<Value, HostError> {
        self.calls += 1;
        serve(self.s, self.p, method, params)
    }
}

fn eval(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let prepared = prepare(s, m)?;
    let revision = s.editor.revision();
    let (id, _abandoned) = open_run(s);
    let mut calls = 0;
    // A panic in a method or in the engine must not leave the run open: the session would refuse
    // every call from then on.
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let engine = prepared.engine()?;
        let mut host = SyncHost {
            s: &mut *s,
            p: &prepared,
            calls: 0,
        };
        let r = engine.run_watched(&prepared.request(), &mut host, &|| prepared.expired());
        calls = host.calls;
        r
    }));
    match ran {
        Ok(result) => close_run(s, &prepared, id, revision, result, calls),
        Err(panic) => {
            s.script_abort_run(id);
            std::panic::resume_unwind(panic)
        }
    }
}

impl Session {
    /// Takes back the run with this id, whatever state it is in.
    pub(crate) fn script_abort_run(&mut self, id: u64) {
        if let Some(c) = self.script_js.current.take_if(|c| c.id == id) {
            self.script_js.serving = false;
            self.editor.rollback_run(c.mark);
            self.editor.set_origin(c.origin);
        }
    }
}

#[cfg(all(test, feature = "script-js", not(target_arch = "wasm32")))]
mod tests;
