//! Cadektor core: document model, commands, geometry, curves, the 3D mesh and SVG output.
//! The agent API is in `cadektor-api`, and the 3D camera and CPU renderer are in `cadektor-render`.
//! This crate has no UI dependencies and compiles to native and WebAssembly.

pub mod command;
pub mod curve;
pub mod fsio;
pub mod geom;
pub mod mesh;
pub mod model;
pub mod svg;

pub use glam::DVec2;
