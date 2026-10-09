//! Cadektor core: document model, commands, rendering and the agent API.
//! This crate has no UI dependencies and compiles to native and WebAssembly.

pub mod api;
pub mod command;
pub mod fsio;
pub mod geom;
pub mod model;
pub mod svg;

pub use glam::DVec2;
