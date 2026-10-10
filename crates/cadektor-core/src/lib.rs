//! Cadektor core: document model, commands, geometry, curves, the 3D mesh and SVG output.
//! The agent API is in `cadektor-api`, and the 3D camera and CPU renderer are in `cadektor-render`.
//! This crate has no UI dependencies and compiles to native and WebAssembly.

pub mod annot;
pub mod command;
pub mod curve;
pub mod datamap;
pub mod fsio;
pub mod geom;
pub mod georef;
pub mod graphics;
pub mod group;
pub mod guid;
pub mod hardscape;
pub mod landscape;
pub mod library;
pub mod massing;
pub mod massing_batch;
pub mod mesh;
pub mod model;
pub mod pattern;
pub mod plants;
pub mod props;
pub mod record;
pub mod resource;
pub mod sheets;
pub mod site;
pub mod svg;
pub mod symbols;
pub mod thumbnail;
pub mod underlay;
pub mod worksheet_table;

pub use glam::{DVec2, DVec3};
