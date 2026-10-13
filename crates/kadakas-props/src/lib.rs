//! The property model of a Kadakas document (PLAT-07, shared with REV-02).
//!
//! A document is a set of things named by a [`Uid`], each with properties. Collaboration sends
//! changes of these properties to other users. Review (`kadakas-review`) stores the history of a
//! file and compares versions with the same properties. Both use this one model, so a diff and a
//! sync agree on what a change is. The crate is pure: no sockets, files, threads or clock, and it
//! builds for the browser.
//!
//! The parts, from the bottom:
//!
//! - [`op`]: operations (create, delete, restore, set, unset) on things named by a [`Uid`].
//! - [`replica`]: the document as things with properties, and the merge rules.
//! - [`docsync`]: turns a document change (`ChangeSet`) or a whole document into operations.
//! - [`materialize`]: builds a valid document from a replica. It is total.
//! - [`validate`], [`order`], [`blob`]: the checks of the merge, the stacking keys and the
//!   content-addressed pictures.
//!
//! The wire protocol, the client state machine and presence are in `kadakas-collab`, which
//! re-exports these modules.
//!
//! [`Uid`]: kadakas_core::uid::Uid

pub mod blob;
pub mod docsync;
pub mod materialize;
pub mod op;
pub mod order;
pub mod replica;
pub mod validate;

pub use materialize::{Materialized, Quarantined, materialize, materialize_with};
pub use op::{Kind, Op, Target, Txn, Writer};
pub use replica::Replica;
