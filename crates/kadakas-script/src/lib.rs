//! Scripting for Kadakas (native only).
//!
//! - [`python`]: find a Python interpreter (`python3` on PATH, or `uv run`).
//! - [`exec`]: run a child process with a time limit and size limits.
//! - [`plugin`]: plug-in object definitions, parameter checks and the reset protocol.
//! - [`examples`]: the plug-in objects that ship with the app.
//! - [`host`]: long-running scripts for the Scripts palette, with a console.
//!
//! The crate never touches a document. The API crate turns the shapes of a reset into commands.

pub mod examples;
pub mod exec;
pub mod host;
pub mod plugin;
pub mod python;
pub mod trust;
