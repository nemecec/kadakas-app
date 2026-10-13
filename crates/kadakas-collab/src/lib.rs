//! Collaboration for Kadakas (PLAT-07 and PLAT-08): the client side.
//!
//! This crate is pure. It has no sockets, no threads, no files and no clock, so it also builds
//! for the browser. A host moves the messages (see [`transport`]). The server is not here: it
//! lives in its own repository (`kadakas-server`). `docs/collaboration.md` describes the protocol
//! for it.
//!
//! The parts, from the bottom:
//!
//! - the property model (`kadakas-props`, re-exported): [`op`] (operations on things named by a
//!   [`Uid`]), [`replica`] (the document as things with properties, and the merge rules),
//!   [`docsync`] (document changes into operations) and `materialize` (a replica back into a
//!   document).
//! - [`protocol`]: the messages between a client and a server, with a version number.
//! - [`presence`]: cursors, selections and viewports of the users (never saved).
//! - [`client`]: the client state machine: optimistic apply, pending queue, rebase, per-user undo
//!   and the offline log.
//! - [`transport`]: the transport trait and an in-memory implementation.
//! - `testkit` (feature `testkit` or tests): a reference sequencer for tests. It is not a server.
//!
//! [`Uid`]: kadakas_core::uid::Uid

pub mod chain;
pub mod client;
pub mod presence;
pub mod protocol;
pub mod store;
pub mod transport;
pub mod undo;
pub mod url;

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "testkit"))]
pub mod testkit;

// The property model lives in `kadakas-props`; it is re-exported under the old paths.
pub use kadakas_props::{blob, docsync, materialize, op, order, replica, validate};

pub use client::{Client, ClientConfig, ClientEvent, ConnState};
pub use kadakas_props::{Materialized, Quarantined, materialize_with};
pub use op::{Kind, Op, Target, Txn, Writer};
pub use protocol::{ClientMsg, ErrorCode, PROTOCOL_VERSION, ServerMsg};
pub use kadakas_props::Replica;
