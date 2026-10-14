//! Review of a Kadakas document (REV-01 and REV-02): named versions and the diff between them.
//!
//! The crate is pure: no I/O, no clock, no UI, and it builds for the browser. A host passes the
//! time and the author of a version. The parts:
//!
//! - [`state`]: the document as things with flat properties (the property model of
//!   `kadakas-props`, which collaboration shares), the operations between two states, and the
//!   build of a document from a state.
//! - [`history`]: the version store. A change from an anchor drawing for the newest version and a
//!   reverse delta of operations for each older one, thresholds on the count and the bytes (a
//!   save over one is refused, nothing is dropped), and a checked load of untrusted JSON.
//! - [`diff`]: the diff between two states, grouped by kind, bounded in work and size.
//! - [`restore`]: the document and the history for a restore of a version.
//!
//! `kadakas-api` joins these with the editor (`version.*` methods); the core keeps the history as
//! opaque JSON in the file (`kadakas_core::history`).

pub mod diff;
pub mod history;
pub mod restore;
pub mod state;

pub use diff::{Diff, DiffOptions, Group};
pub use history::{
    Added, History, LimitInfo, LimitKind, Limits, Loaded, NewVersion, ReviewError, VersionInfo,
    VersionMeta, anchor_digest,
};
pub use state::State;

#[cfg(test)]
mod testutil;
