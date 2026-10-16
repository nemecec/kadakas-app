//! In-process JavaScript scripts (SCR-08).
//!
//! A script is JavaScript on QuickJS (quickjs-ng). QuickJS is built to WebAssembly and runs inside
//! the plug-in host (`kadakas-plugin`), so the same script runs the same way on the desktop, the
//! server and the browser. The module has no files, no network and no clock; its only way out is
//! `kadakas.call(method, params)`, which asks the host to run an API method. The host
//! ([`ScriptHost`]) decides what the call does: in Kadakas it goes through the same dispatch and
//! the same checks as a JSON-RPC request.
//!
//! This crate has no knowledge of documents. It has:
//!
//! - [`ScriptEngine`]: loads the module and runs a script with a host.
//! - [`typed`]: typed arrays across the boundary (`Float64Array` with a `stride`).
//! - [`frame`]: the byte format of the messages.
//!
//! **Scripts only coordinate.** Every heavy operation is a native call that works on many items.
//! A script that needs a loop over many items to do geometry shows that a native call is missing.
//!
//! The engine needs the cargo feature `engine` (it builds QuickJS with a wasi-sdk, see
//! `docs/dev-guide.md`). Without it [`ScriptEngine::new`] returns [`ScriptError::Unavailable`].

use std::fmt;

use kadakas_plugin::{HostReply, HostedModule, Limits, PluginError};
use serde_json::{Value, json};

pub mod frame;
pub mod typed;

#[cfg(feature = "engine")]
const MODULE: &[u8] = include_bytes!(env!("KADAKAS_JS_WASM"));
#[cfg(not(feature = "engine"))]
const MODULE: &[u8] = &[];

/// The toolchain that built the module, for display (for example `wasi-sdk 34.0 (...)`).
pub const TOOLCHAIN: &str = env!("KADAKAS_JS_TOOLCHAIN");

/// The upstream of the module, for the About dialog.
pub const UPSTREAM: &str = "quickjs-ng 0.17.0 (MIT)";

/// The limits of one run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptLimits {
    /// Work units for the whole run (about one per WebAssembly instruction). The time spent in
    /// the host (the calls) is not counted.
    pub fuel: u64,
    /// Wall-clock milliseconds for one stretch of the script between two calls (native only; the
    /// browser has no deadline).
    pub segment_millis: u64,
    /// Most bytes of memory of the script module.
    pub memory_bytes: usize,
    /// Most bytes in the answer of a run (the value, the log and the typed arrays).
    pub output_bytes: usize,
    /// Most calls of `kadakas.call` in one run.
    pub max_calls: u64,
}

impl ScriptLimits {
    /// The limits of the desktop and the server.
    pub fn native() -> Self {
        ScriptLimits {
            fuel: 2_000_000_000,
            segment_millis: 1_500,
            memory_bytes: 128 << 20,
            output_bytes: 16 << 20,
            max_calls: 1_000_000,
        }
    }

    /// The limits of the browser, where fuel is the only bound of the time (as for plug-ins).
    pub fn browser() -> Self {
        ScriptLimits {
            fuel: 400_000_000,
            ..Self::native()
        }
    }
}

impl Default for ScriptLimits {
    fn default() -> Self {
        if cfg!(target_arch = "wasm32") {
            Self::browser()
        } else {
            Self::native()
        }
    }
}

/// What a script run needs.
pub struct Request<'a> {
    /// The body of a function. It gets `args`, and its `return` value is the result.
    pub code: &'a str,
    /// The value of `args`.
    pub args: &'a Value,
    /// Seeds `Math.random`. The same seed gives the same numbers.
    pub seed: u32,
}

/// The result of a run.
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    /// The returned value as JSON. A typed array in it is a marker (see [`typed`]).
    pub value: Value,
    /// The bytes of the typed arrays of `value`.
    pub buffers: Vec<Vec<u8>>,
    /// What `console.log` wrote (at most 64 KiB; more lines are counted, not kept).
    pub log: String,
    /// How many times the script called the host.
    pub calls: u64,
}

impl Output {
    /// The value with the typed arrays written out as numbers.
    pub fn json(&self) -> Result<Value, String> {
        let mut v = self.value.clone();
        typed::resolve(&mut v, &self.buffers)?;
        Ok(v)
    }
}

/// An error that the host gives back to the script. The script sees an `Error` with `code`,
/// `data` and `method`.
#[derive(Clone, Debug, PartialEq)]
pub struct CallFailure {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

/// Why a call did not give a value.
#[derive(Clone, Debug, PartialEq)]
pub enum HostError {
    /// The call failed. The script can catch the error.
    Failure(CallFailure),
    /// Stop the whole run (a limit of the host). The script cannot catch it.
    Stop(String),
}

/// What serves `kadakas.call`.
pub trait ScriptHost {
    /// Runs `method`. `params` has the typed arrays written out already. With `typed`, the script
    /// wants long lists of points and numbers in the answer as typed arrays (the engine does that
    /// to the returned value).
    fn call(&mut self, method: &str, params: Value, typed: bool) -> Result<Value, HostError>;
}

/// Why a run failed.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptError {
    /// The script threw (or did not compile).
    Script {
        message: String,
        /// The `name` of the error (`TypeError`, `SyntaxError`, ...).
        name: String,
        stack: String,
        /// Set when a call to the host failed and the script did not catch it.
        method: Option<String>,
        code: Option<i64>,
        data: Option<Value>,
    },
    /// A limit stopped the run: fuel, time, memory, calls, output size or the stack.
    Limit(String),
    /// The module trapped, or the host did something that it should not.
    Trap(String),
    /// This build has no script engine.
    Unavailable(String),
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScriptError::Script {
                message,
                method,
                stack,
                ..
            } => {
                match method {
                    Some(m) => write!(f, "the script failed at {m}: {message}")?,
                    None => write!(f, "the script failed: {message}")?,
                }
                if let Some(line) = stack.lines().find(|l| l.contains("<eval>")) {
                    write!(f, " ({})", line.trim())?;
                }
                Ok(())
            }
            ScriptError::Limit(m) => write!(f, "the script went over a limit: {m}"),
            ScriptError::Trap(m) => write!(f, "the script engine crashed: {m}"),
            ScriptError::Unavailable(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ScriptError {}

/// A failed run, with what the script logged before it failed (if the engine could tell).
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub error: ScriptError,
    pub log: String,
    pub calls: u64,
}

impl From<ScriptError> for Failure {
    fn from(error: ScriptError) -> Self {
        Failure {
            error,
            log: String::new(),
            calls: 0,
        }
    }
}

/// The script engine: the loaded module and the limits.
pub struct ScriptEngine {
    module: Option<Box<dyn HostedModule>>,
    limits: ScriptLimits,
    /// The limits as the plug-in host knows them.
    unavailable: Option<String>,
}

impl ScriptEngine {
    /// Loads the module with the engine of this build (wasmtime on the desktop). Compiling the
    /// module takes a moment the first time in a process; the compiled module is shared.
    pub fn new(limits: ScriptLimits) -> Result<Self, ScriptError> {
        if MODULE.is_empty() {
            return Err(ScriptError::Unavailable(
                "this build has no script engine (kadakas-script-js was built without the feature `engine`)"
                    .into(),
            ));
        }
        let runtime = kadakas_plugin::default_runtime();
        let module = runtime
            .load_hosted(MODULE, &plugin_limits(&limits))
            .map_err(|e| match e {
                PluginError::Unsupported(m) => ScriptError::Unavailable(m),
                other => ScriptError::Trap(other.to_string()),
            })?;
        Ok(ScriptEngine {
            module: Some(module),
            limits,
            unavailable: None,
        })
    }

    pub fn limits(&self) -> &ScriptLimits {
        &self.limits
    }

    /// True if this build can run scripts.
    pub fn built_in() -> bool {
        !MODULE.is_empty()
    }

    /// Runs a script. Every call of the script goes to `host`.
    pub fn run(&self, request: &Request<'_>, host: &mut dyn ScriptHost) -> Result<Output, Failure> {
        let Some(module) = &self.module else {
            return Err(ScriptError::Unavailable(
                self.unavailable.clone().unwrap_or_default(),
            )
            .into());
        };
        let options = json!({
            "memory": self.limits.memory_bytes / 4 * 3,
            "stack": 256 * 1024,
            "seed": request.seed,
            "max_calls": self.limits.max_calls,
        });
        let args = serde_json::to_vec(request.args).map_err(|e| {
            Failure::from(ScriptError::Trap(format!("the arguments cannot be sent: {e}")))
        })?;
        let options = serde_json::to_vec(&options).expect("options are JSON");
        let frame = frame::encode(&[request.code.as_bytes(), &args, &options]);
        let mut serve = |bytes: &[u8]| serve_call(bytes, host);
        let answer = module.run(&frame, &mut serve).map_err(plugin_failure)?;
        parse_answer(&answer)
    }
}

fn plugin_limits(l: &ScriptLimits) -> Limits {
    Limits {
        fuel: l.fuel,
        fuel_describe: l.fuel,
        fuel_move: l.fuel,
        max_millis: l.segment_millis,
        memory_bytes: l.memory_bytes,
        max_output_bytes: l.output_bytes,
        ..Limits::default()
    }
}

fn plugin_failure(e: PluginError) -> Failure {
    let error = match e {
        PluginError::Limit(m) => ScriptError::Limit(m),
        PluginError::Unsupported(m) => ScriptError::Unavailable(m),
        other => ScriptError::Trap(other.to_string()),
    };
    error.into()
}

fn error_reply(code: i64, message: &str, data: Option<&Value>) -> HostReply {
    let body = json!({"code": code, "message": message, "data": data});
    HostReply::Reply(frame::encode(&[b"err", body.to_string().as_bytes()]))
}

/// Serves one `kadakas.call`: decode the request, ask the host, encode the reply.
fn serve_call(bytes: &[u8], host: &mut dyn ScriptHost) -> HostReply {
    let Some(parts) = frame::decode(bytes).filter(|p| p.len() >= 2) else {
        return error_reply(-32600, "the call is broken", None);
    };
    let Ok(method) = std::str::from_utf8(parts[0]) else {
        return error_reply(-32600, "the method is not text", None);
    };
    let mut params: Value = match serde_json::from_slice(parts[1]) {
        Ok(v) => v,
        Err(e) => return error_reply(-32602, &format!("the parameters are not JSON: {e}"), None),
    };
    let wants_typed = params
        .as_object_mut()
        .and_then(|m| m.remove("$typed"))
        .is_some_and(|v| v == Value::Bool(true));
    if let Err(e) = typed::resolve(&mut params, &parts[2..]) {
        return error_reply(-32602, &e, None);
    }
    match host.call(method, params, wants_typed) {
        Ok(mut value) => {
            let buffers = if wants_typed {
                typed::extract(&mut value)
            } else {
                Vec::new()
            };
            let text = value.to_string();
            let mut out: Vec<&[u8]> = vec![b"ok", text.as_bytes()];
            out.extend(buffers.iter().map(Vec::as_slice));
            HostReply::Reply(frame::encode(&out))
        }
        Err(HostError::Failure(f)) => error_reply(f.code, &f.message, f.data.as_ref()),
        Err(HostError::Stop(m)) => HostReply::Abort(PluginError::Limit(m)),
    }
}

fn parse_answer(answer: &[u8]) -> Result<Output, Failure> {
    let parts = frame::decode(answer)
        .filter(|p| p.len() >= 4)
        .ok_or_else(|| Failure::from(ScriptError::Trap("the engine gave a broken answer".into())))?;
    let text = |i: usize| String::from_utf8_lossy(parts[i]).into_owned();
    let calls = text(2).parse::<u64>().unwrap_or(0);
    let log = text(3);
    let status = text(0);
    let fail = |error: ScriptError| Failure {
        error,
        log: log.clone(),
        calls,
    };
    match status.as_str() {
        "ok" => {
            let value: Value = serde_json::from_slice(parts[1])
                .map_err(|e| fail(ScriptError::Trap(format!("the result is not JSON: {e}"))))?;
            Ok(Output {
                value,
                buffers: parts[4..].iter().map(|b| b.to_vec()).collect(),
                log,
                calls,
            })
        }
        "error" => {
            let info: Value = serde_json::from_slice(parts[1]).unwrap_or(Value::Null);
            let s = |k: &str| {
                info.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            let message = s("message");
            let name = s("name");
            // QuickJS reports a spent memory limit and a deep recursion as `InternalError`.
            if name == "InternalError"
                && (message.contains("out of memory") || message.contains("stack overflow"))
            {
                return Err(fail(ScriptError::Limit(format!(
                    "{message} (memory limit of the script, or recursion too deep)"
                ))));
            }
            if message.contains("more than") && message.contains("calls") {
                return Err(fail(ScriptError::Limit(message)));
            }
            Err(fail(ScriptError::Script {
                message,
                name,
                stack: s("stack"),
                method: info.get("method").and_then(Value::as_str).map(String::from),
                code: info.get("code").and_then(Value::as_i64),
                data: info.get("data").filter(|d| !d.is_null()).cloned(),
            }))
        }
        _ => Err(fail(ScriptError::Trap(format!("the engine failed: {}", text(1))))),
    }
}

#[cfg(all(test, feature = "engine", not(target_arch = "wasm32")))]
mod tests;
