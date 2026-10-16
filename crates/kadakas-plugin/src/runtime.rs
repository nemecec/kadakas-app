//! The runtime-neutral interface to a WebAssembly engine.
//!
//! The host of Kadakas never names an engine outside this module and its backends. A backend
//! turns the bytes of a module into a [`LoadedModule`] that answers two byte-level calls,
//! `describe` and `reset`. The checks of the manifest, the parameters and the shapes happen above
//! this interface (see [`crate::plugin::Plugin`]), so every backend behaves the same.
//!
//! - Native builds use `wasmtime` (`crate::native`).
//! - The browser build uses the `WebAssembly` API of the page (`crate::web`) through the metered
//!   engine (`crate::metered`), which gets its fuel and memory cap from a rewrite of the module
//!   (`crate::harden`).

use std::fmt;

/// `Send + Sync` where threads exist. In the browser build there is only one thread, and a
/// backend that holds a JavaScript object cannot be `Send`.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> MaybeSend for T {}
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}

/// The limits of the sandbox and of the answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Work units for one call (about one per WebAssembly instruction). A call that needs more stops.
    /// This is the budget of `reset`, and of a click or finish of a tool.
    pub fuel: u64,
    /// Work units for `describe`. The native engine uses [`Limits::fuel`] for every call. The
    /// browser engine has no deadline, so it gives the light calls a smaller budget (see
    /// [`Limits::browser`]). The budget of a call is never above `fuel`.
    pub fuel_describe: u64,
    /// Work units for a pointer move of a tool, which comes many times per second.
    pub fuel_move: u64,
    /// Wall-clock time for one call, in milliseconds. Fuel counts instructions only. A loop of
    /// `memory.fill` costs little fuel but much time, so the time limit stops it.
    pub max_millis: u64,
    /// Most bytes of linear memory for one call.
    pub memory_bytes: usize,
    /// Largest module, in bytes.
    pub max_module_bytes: usize,
    /// Largest function body, in bytes. Compiling a huge function takes long.
    pub max_function_bytes: usize,
    /// Most functions in a module.
    pub max_functions: usize,
    /// Most local variables in one function.
    pub max_locals: usize,
    /// Most bytes in an answer.
    pub max_output_bytes: usize,
    /// Most shapes in an answer.
    pub max_shapes: usize,
    /// Most vertices in all polylines of an answer together.
    pub max_vertices: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            fuel: 200_000_000,
            fuel_describe: 200_000_000,
            fuel_move: 200_000_000,
            max_millis: 1_500,
            memory_bytes: 64 << 20,
            max_module_bytes: 2 << 20,
            max_function_bytes: 256 << 10,
            max_functions: 5_000,
            max_locals: 5_000,
            max_output_bytes: 2 << 20,
            max_shapes: 2_000,
            max_vertices: 100_000,
        }
    }
}

impl Limits {
    /// The limits of the browser build. There is no wall-clock deadline there, so the fuel is the
    /// only bound of the time that a call blocks the page, and it is smaller: `describe` 5M, a
    /// pointer move 15M, everything else 50M instructions.
    pub fn browser() -> Limits {
        Limits {
            fuel: 50_000_000,
            fuel_describe: 5_000_000,
            fuel_move: 15_000_000,
            ..Limits::default()
        }
    }

    /// The fuel of one kind of call.
    pub fn fuel_for(&self, call: CallKind) -> u64 {
        match call {
            CallKind::Describe => self.fuel_describe,
            CallKind::ToolMove => self.fuel_move,
            CallKind::Reset | CallKind::ToolStep => self.fuel,
        }
        .min(self.fuel)
    }

    /// The fuel that a rewritten module starts with before the host sets the fuel of a call: the
    /// smallest budget, so a start function cannot use more than the lightest call.
    pub fn start_fuel(&self) -> u64 {
        self.fuel_for(CallKind::Describe)
            .min(self.fuel_for(CallKind::ToolMove))
    }
}

/// The limits that the registry of this build starts with.
pub fn default_limits() -> Limits {
    #[cfg(target_arch = "wasm32")]
    {
        Limits::browser()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Limits::default()
    }
}

/// Which call of a module this is. The browser engine gives each kind its own fuel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallKind {
    Describe,
    Reset,
    /// A pointer move of a tool.
    ToolMove,
    /// Any other event of a tool.
    ToolStep,
}

/// What can go wrong with a plug-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginError {
    /// The module or its manifest is not valid: bad bytes, an import, a missing export, a bad manifest.
    Invalid(String),
    /// The call used up its fuel, its memory, or a size limit.
    Limit(String),
    /// The module trapped (for example `unreachable`, or an out of range memory access).
    Trap(String),
    /// The answer is broken or breaks the rules.
    BadOutput(String),
    /// The plug-in said that it cannot make the object (for example, the object is too small).
    Guest(String),
    /// The host did not run the module: it is not trusted, or it asks for a permission.
    Refused(String),
    /// This build has no engine.
    Unsupported(String),
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginError::Invalid(m) => write!(f, "the plug-in module is not valid: {m}"),
            PluginError::Limit(m) => write!(f, "the plug-in went over a limit: {m}"),
            PluginError::Trap(m) => write!(f, "the plug-in crashed: {m}"),
            PluginError::BadOutput(m) => write!(f, "the plug-in gave a bad answer: {m}"),
            PluginError::Guest(m) => write!(f, "the plug-in failed: {m}"),
            PluginError::Refused(m) | PluginError::Unsupported(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for PluginError {}

/// A compiled module that is ready to run. Each call runs in a new, empty instance, so one call
/// cannot see the memory of another.
pub trait LoadedModule: MaybeSend {
    /// Calls `describe` and returns the bytes of the manifest JSON.
    fn describe(&self) -> Result<Vec<u8>, PluginError>;
    /// Calls `reset` with the bytes of a request and returns the bytes of the answer.
    fn reset(&self, request: &[u8]) -> Result<Vec<u8>, PluginError>;
    /// Calls `tool` with the bytes of a request and returns the bytes of the answer. `light` is
    /// true for a pointer move, which gets less fuel in the browser.
    fn tool(&self, request: &[u8], light: bool) -> Result<Vec<u8>, PluginError>;
}

/// The answer of the host to a call that a hosted module makes (see [`HostedModule`]).
pub enum HostReply {
    /// The bytes of the reply. The module reads them with `kadakas.read`.
    Reply(Vec<u8>),
    /// Stop the run now with this error (a time limit of the whole run, for example).
    Abort(PluginError),
}

/// The function that serves the calls of a hosted module. It gets the bytes of a request frame.
pub type HostCall<'a> = &'a mut dyn FnMut(&[u8]) -> HostReply;

/// Asked before every call of a hosted module and at every tick of the clock of the engine (native
/// only) while it runs. A reason stops the run with that error.
pub type Watch<'a> = &'a dyn Fn() -> Option<PluginError>;

/// The exports that a hosted module must have: the memory, `alloc`, `dealloc` and `run`. An export
/// `_initialize` is called first when it is there.
pub const HOSTED_EXPORTS: [&str; 4] = ["memory", "alloc", "dealloc", "run"];

/// The two imports that a hosted module may have (module [`HOST_MODULE`]): `call(ptr, len) -> i32`
/// sends a request to the host and returns the length of the reply (or -1); `read(dst)` copies the
/// reply to the memory of the module. A hosted module imports nothing else: no files, no network
/// and no clock.
pub const HOST_MODULE: &str = "kadakas";
pub const HOST_IMPORTS: [&str; 2] = ["call", "read"];

/// The largest memory of a hosted module, on every engine. The browser rewrite gives a hosted module
/// this maximum whatever the limits of the session are, and the native engine caps the memory of a
/// run at the limit you give, never above it. The rewritten module is compiled once in advance (a large module cannot be
/// compiled in the call), so it must not depend on the limits; the engine inside the module
/// (QuickJS) keeps its own limit, three quarters of the memory limit of the run.
pub const HOSTED_MEMORY_BYTES: usize = 128 << 20;

/// A module that can call back into the host while it runs (the script engine, SCR-08). It makes
/// the calls through the two imports above, so it can ask the host to do the heavy work, and the
/// host decides what it answers. The limits are the same as for a plug-in: fuel and memory for the
/// whole run, and a time limit for each stretch of the module between two calls (the time that the
/// host spends in a call is the business of the host, which can stop the run with
/// [`HostReply::Abort`]).
pub trait HostedModule: MaybeSend {
    /// Runs the export `run` with the bytes of a request and returns the bytes of the answer.
    /// `host` serves the calls of the module.
    fn run(
        &self,
        request: &[u8],
        host: HostCall<'_>,
        watch: Watch<'_>,
    ) -> Result<Vec<u8>, PluginError>;
}

/// A WebAssembly engine.
pub trait PluginRuntime: MaybeSend {
    /// The name of the engine, for display.
    fn name(&self) -> &'static str;
    /// Checks and compiles a module. The module must have no imports and must export `memory`,
    /// `alloc`, `dealloc`, `describe`, `reset` and `tool`. The limits apply to every call of the module.
    fn load(&self, wasm: &[u8], limits: &Limits) -> Result<Box<dyn LoadedModule>, PluginError>;
    /// Checks and compiles a hosted module (see [`HostedModule`]). An engine that cannot run one
    /// says so.
    fn load_hosted(
        &self,
        _wasm: &[u8],
        _limits: &Limits,
    ) -> Result<Box<dyn HostedModule>, PluginError> {
        Err(PluginError::Unsupported(
            "this engine cannot run scripts".into(),
        ))
    }
}

/// The engine of this build: wasmtime on native targets, the browser engine on wasm32.
pub fn default_runtime() -> Box<dyn PluginRuntime> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Box::new(crate::native::WasmtimeRuntime::new())
    }
    #[cfg(target_arch = "wasm32")]
    {
        Box::new(crate::web::runtime())
    }
}

/// The names that a module must export, with a short description for error messages.
pub const REQUIRED_EXPORTS: [&str; 6] = ["memory", "alloc", "dealloc", "describe", "reset", "tool"];
