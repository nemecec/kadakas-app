//! Platform helpers for Cadektor, in one place.
//!
//! - [`fsio`]: atomic file writes with a `.bak` copy and a flush to disk.
//! - [`process`]: process groups, kill of a whole process tree, process liveness.
//! - [`dirs`]: the per-user config, state, runtime and cache folders.
//! - [`tmp`]: private temporary folders.
//! - [`agent`]: the runtime file that tells agent clients how to reach a running app.
//!
//! The crate has no document types and no dependency on other Cadektor crates. It compiles on
//! every target, but only native code calls it.

pub mod agent;
pub mod dirs;
pub mod fsio;
pub mod process;
pub mod tmp;
