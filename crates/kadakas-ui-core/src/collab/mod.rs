//! The logic of the collaboration UI (PLAT-09), without egui.
//!
//! A UI program drives a shared document through the normal API (`collab.connect`,
//! `collab.sync`, `collab.presence`, `collab.undo`). This module holds the parts that do not
//! need a window, so they are tested alone and another UI program can reuse them:
//!
//! - [`args`]: the command line and environment rules (`--collab-url`, `--collab-token`,
//!   `--allow-collab-url`) and the defaults of the dialog.
//! - [`form`]: the state of the Connect dialog, its checks, the parameters of `collab.connect`,
//!   the answer to its errors, and the address fragment of the browser build
//!   (`#collab=<ws url>&doc=<id>&token=<t>`).
//! - [`status`]: the link state for the status bar, the error classes and their messages, and
//!   the choice between the normal undo and the per-user undo.
//! - [`peers`]: the other users: a color for each, idle times, and the presence of this user.
//!
//! The module takes and gives plain values (JSON from the API, numbers, texts). It never
//! reads a clock: the caller passes `now_ms`. A token is a secret: it is kept in memory only
//! and the `Debug` text of every type here hides it.

pub mod args;
pub mod form;
pub mod peers;
pub mod status;

pub use args::CollabArgs;
pub use form::{
    ConnectForm, ConnectIssue, ConnectRequest, Defaults, FormError, Fragment,
    classify_connect_error, token_expired, token_user,
};
pub use peers::{AGENT_CLIENT, AGENT_RGB, OwnPresence, PALETTE, Peer, PeerBook};
pub use status::{ErrorKind, Link, Notice, Severity, StatusView, Tracker, UndoRoute, undo_route};
