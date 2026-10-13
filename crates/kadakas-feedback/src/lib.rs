//! Feedback reports (API-47, API-48): bug reports and feature requests from agents and people.
//!
//! - [`model`]: the report, the kinds, the limits.
//! - [`scrub`]: removes paths, tokens, secrets and e-mail addresses.
//! - [`hub`]: the outbox, dedupe, caps, the rate limit and the consent rules.
//! - [`payload`]: the message to the server (protocol 1, `docs/feedback.md`).
//! - [`send`]: the transport trait, retry with a back-off, and fakes for tests.
//! - [`store`]: the files of the outbox: memory, and (native) a folder.
//! - [`http`] and [`dispatch`] (native only): the HTTP transport and the sending thread.
//!
//! The crate has no Kadakas dependency but `kadakas-sys` (native) and no document types. The
//! model excerpt, which needs the document, is built in `kadakas-api` (`feedback_api`).
//!
//! Consent. A report with attachments waits as `NeedsReview` until a person approves it with
//! [`Hub::approve`]. Only the app and `kadakas feedback review` call that function.

pub mod error;
pub mod hub;
pub mod model;
pub mod payload;
pub mod scrub;
pub mod send;
pub mod store;

#[cfg(not(target_arch = "wasm32"))]
pub mod dispatch;
#[cfg(not(target_arch = "wasm32"))]
pub mod http;

pub use error::FeedbackError;
pub use hub::{Hub, Outgoing, Settings, Submitted};
pub use model::*;
pub use payload::PayloadParts;
pub use send::{
    DEFAULT_URL, Delivery, FakeSender, NoSleep, Reply, RetryPolicy, SendError, Sender, Sleeper,
};
pub use store::{MemStorage, Storage};
