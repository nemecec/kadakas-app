//! The host for Kadakas WebAssembly plug-ins (PLAT-10).
//!
//! A plug-in is a WebAssembly module that follows the ABI of `kadakas-plugin-abi`. The host loads
//! it, reads its manifest, and calls `reset` with the parameters and the geometry. The answer is a
//! list of shapes of the core types, which the caller stores in the document as plain data. Other
//! clients draw them without the plug-in.
//!
//! The parts:
//!
//! - [`runtime`]: the engine interface ([`PluginRuntime`], [`LoadedModule`]), the [`Limits`] and
//!   the errors. It names no engine.
//! - `native` (native targets only): the wasmtime engine. No imports, so no files or network. Fuel
//!   and a memory cap limit every call.
//! - [`harden`]: rewrites a module so that it enforces its own fuel and memory cap. For engines
//!   that have no limits of their own.
//! - [`metered`]: the engine for such hosts, on top of a small `Compiler` trait. The browser
//!   engine (`web`, wasm32 only) is a `Compiler` on the `WebAssembly` API of the page.
//! - [`store`]: keeps installed modules and the trust list between sessions, on a medium that the
//!   caller gives (a folder, the browser storage, memory).
//! - [`plugin`]: [`Plugin`], one loaded module, with the checks of the manifest and the answer.
//! - [`trust`]: trust by the SHA-256 of the module file.
//! - [`registry`]: the installed plug-ins of a session.
//!
//! The crate never touches a document. The API crate turns the shapes into commands.

pub mod harden;
pub mod metered;
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
pub mod plugin;
pub mod registry;
pub mod runtime;
pub mod store;
#[cfg(all(any(test, feature = "testkit"), not(target_arch = "wasm32")))]
pub mod testkit;
pub mod trust;
#[cfg(target_arch = "wasm32")]
pub mod web;

pub use plugin::{Generated, Plugin, ToolCall, ToolDone, ToolStep};
pub use registry::{Registry, StoreReport};
pub use runtime::{
    HostCall, HostReply, HostedModule, Limits, LoadedModule, PluginError, PluginRuntime,
    default_runtime,
};
pub use store::{MemoryBackend, ModuleStore, StoreBackend, StoreError};
pub use trust::{ModuleTrust, Permit};

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
