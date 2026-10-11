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
//! - [`stub`]: the engine of the browser build. It says that plug-ins are not available there yet.
//!   A browser engine can implement the same two traits later.
//! - [`plugin`]: [`Plugin`], one loaded module, with the checks of the manifest and the answer.
//! - [`trust`]: trust by the SHA-256 of the module file.
//! - [`registry`]: the installed plug-ins of a session.
//!
//! The crate never touches a document. The API crate turns the shapes into commands.

#[cfg(not(target_arch = "wasm32"))]
pub mod native;
pub mod plugin;
pub mod registry;
pub mod runtime;
pub mod stub;
pub mod trust;

pub use plugin::{Generated, Plugin, ToolCall, ToolDone, ToolStep};
pub use registry::Registry;
pub use runtime::{Limits, LoadedModule, PluginError, PluginRuntime, default_runtime};
pub use trust::{ModuleTrust, Permit};

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
