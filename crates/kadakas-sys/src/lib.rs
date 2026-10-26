//! Platform helpers for Kadakas, in one place.
//!
//! - [`fsio`]: atomic file writes with a `.bak` copy and a flush to disk.
//! - [`jsonfile`]: settings files in JSON, changed key by key under a lock.
//! - [`paths`]: the form of a path that people and agents see (no `\\?\` prefix).
//! - [`process`]: process groups, kill of a whole process tree, process liveness.
//! - [`dirs`]: the per-user config, data, state, runtime and cache folders.
//! - [`sysinfo`]: the physical memory of the computer.
//! - [`tmp`]: private temporary folders.
//! - [`agent`]: the runtime file that tells agent clients how to reach a running app.
//!
//! The crate has no document types and no dependency on other Kadakas crates. It compiles on
//! every target, but only native code calls it.

pub mod agent;
pub mod dirs;
pub mod fsio;
pub mod jsonfile;
pub mod paths;
pub mod process;
pub mod sysinfo;
pub mod tmp;
