//! Update logic for the Kadakas launcher (PLAT-06). Native only.
//!
//! The launcher is a small program that keeps Kadakas up to date and starts it. This crate has
//! all of its logic, so that it can be tested without a window:
//!
//! - [`manifest`]: the update manifest and its signed envelope (ed25519).
//! - [`keys`]: the trusted public keys (two at most, for key rotation).
//! - [`http`]: the HTTP trait, the real client and a test client.
//! - [`archive`]: safe unpacking of `.tar.gz` archives.
//! - [`store`]: versioned folders, the `current` pointer, rollback, crash safety.
//! - [`updater`]: check, download, verify, install.
//! - [`settings`], [`flavors`], [`launch`]: channel, update mode, UI flavor and profile, and
//!   starting the app.
//! - [`choose`]: which version starts (mode, channel, crashed versions, fallback to the bundled app).
//! - [`runs`], [`handshake`]: the run of the app, the clean-exit mark, and the files that the
//!   launcher and the app use to talk (PLAT-16).
//! - [`inbox`]: documents that arrive while the app runs (a double click on a `.kad` file): sent to
//!   the running app, or kept for the next start.
//! - [`supervisor`]: the headless launcher: starts the app at once, checks in the background.
//! - [`local`]: local update sites and local builds (PLAT-17).
//! - [`tasks`]: the launcher actions as values.
//! - [`sign`]: signing, for CI and tests only (feature `sign`). The launcher does not have it.
//!
//! The launcher holds no secret: it has public keys only and no account or token.

pub mod archive;
pub mod choose;
pub mod error;
pub mod flavors;
pub mod handshake;
mod hex;
pub mod http;
pub mod inbox;
pub mod keys;
pub mod launch;
pub mod local;
pub mod manifest;
pub mod runs;
pub mod settings;
#[cfg(any(test, feature = "sign"))]
pub mod sign;
pub mod store;
pub mod supervisor;
pub mod tasks;
pub mod updater;
pub mod version;

pub use error::{Result, UpdateError};
pub use flavors::Flavor;
pub use http::{Http, MockHttp, UreqHttp};
pub use keys::TrustedKeys;
pub use manifest::{Channel, Manifest, PlatformFile, SignedManifest, Trust};
pub use settings::{Settings, UpdateMode};
pub use store::{Active, Bundled, Installed, Slot, Store, VersionMeta};
pub use tasks::{Outcome, Task};
pub use updater::{Offer, UpdateStatus, Updater, current_platform, sha256_hex};
pub use version::Version;
