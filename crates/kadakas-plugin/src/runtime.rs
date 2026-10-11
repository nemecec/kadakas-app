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

/// A WebAssembly engine.
pub trait PluginRuntime: MaybeSend {
    /// The name of the engine, for display.
    fn name(&self) -> &'static str;
    /// Checks and compiles a module. The module must have no imports and must export `memory`,
    /// `alloc`, `dealloc`, `describe`, `reset` and `tool`. The limits apply to every call of the module.
    fn load(&self, wasm: &[u8], limits: &Limits) -> Result<Box<dyn LoadedModule>, PluginError>;
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
