//! Plants for Cadektor: placement math, the plant schedule and plant list, and the plant
//! catalog CSV import and export. Pure functions on the document types of `cadektor-core`
//! (wasm-compatible).
//!
//! The data (plant styles, plant objects, variation, tags) and the commands stay in core.

pub mod catalog;
pub mod placement;
pub mod plant_list;
pub mod schedule;
