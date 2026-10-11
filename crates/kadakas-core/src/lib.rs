//! Kadakas core: document model, commands, geometry, curves, the 3D mesh and SVG output.
//! The agent API is in `kadakas-api`, and the 3D camera and CPU renderer are in `kadakas-render`.
//! This crate has no UI dependencies and compiles to native and WebAssembly.

pub mod annot;
pub mod cache;
pub mod change;
pub mod command;
pub mod csv;
pub mod curve;
pub mod datamap;
pub mod edit2d;
pub mod geom;
pub mod georef;
pub mod graphics;
pub mod ground;
pub mod group;
pub mod guid;
pub mod hardscape;
pub mod irrigation;
pub mod landscape;
pub mod library;
pub mod massing;
pub mod massing_batch;
pub mod mesh;
pub mod mesh3d;
pub mod model;
pub mod pattern;
pub mod plants;
pub mod present;
pub mod props;
pub mod record;
pub mod resource;
pub mod schema;
pub mod section;
pub mod sheets;
pub mod site;
pub mod snapping;
pub mod solid3d;
pub mod solid_batch;
pub mod sun;
pub mod survey;
pub mod svg;
pub mod symbols;
pub mod uid;
pub mod underlay;
pub mod viewrules;
pub mod visibility;
pub mod worksheet_table;

pub use glam::{DMat2, DVec2, DVec3};
