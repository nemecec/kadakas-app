//! The plug-in ABI of Kadakas: what a WebAssembly plug-in and the host say to each other.
//!
//! This crate is pure. It compiles for the host and for `wasm32-unknown-unknown`, so a plug-in
//! author uses the same types as the host does. It has these parts:
//!
//! - [`manifest`]: the manifest that `describe` returns (id, name, version, ABI version, the kinds
//!   the plug-in provides, the parameter schema and the permissions it asks for).
//! - [`param`]: the parameter schema and the checks of parameter values.
//! - [`message`]: the messages of a reset: the request (parameters and geometry) and the answer
//!   (shapes or an error). They travel as JSON.
//! - [`guest`]: helpers and the [`export_plugin!`] macro for the plug-in side.
//!
//! The module interface has these exports and no imports:
//!
//! | Export | Meaning |
//! |---|---|
//! | `memory` | The linear memory of the module. |
//! | `alloc(len: u32) -> u32` | Reserve `len` bytes and return the address. |
//! | `dealloc(ptr: u32, len: u32)` | Free a block from `alloc` or from an answer. |
//! | `describe() -> u64` | Return the manifest as JSON. |
//! | `reset(ptr: u32, len: u32) -> u64` | Read a JSON request at `ptr`, return a JSON answer. |
//!
//! A `u64` result packs an address in the high 32 bits and a length in the low 32 bits
//! ([`guest::pack`]). A result of 0 means "no answer". The host frees the answer with `dealloc`.

pub mod guest;
pub mod manifest;
pub mod message;
pub mod param;

pub use manifest::{Kind, Manifest, ObjectSpec, Permission, ShapeType};
pub use message::{Attrs, Geometry, OutShape, ResetRequest, ResetResponse, ShapeData};
pub use param::{ParamSchema, ParamType, color_rgba};
/// Plug-ins use this to build parameter defaults (`json!`) without another dependency.
pub use serde_json;

/// The version of the ABI that this crate describes. A host runs only modules with this number.
pub const ABI_VERSION: u32 = 1;

/// Largest absolute value of a coordinate or of a number parameter, in mm.
pub const MAX_VALUE: f64 = 1.0e9;
