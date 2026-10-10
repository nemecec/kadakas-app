//! A WebSocket for the Kadakas collaboration client.
//!
//! [`WsEndpoint`] implements [`kadakas_collab::transport::Endpoint`] and makes [`WsTransport`]s
//! (the `Transport` trait). The collaboration crate has no network code; this crate has it, so the
//! API crate and the pure crates stay free of sockets.
//!
//! - **Native** (`not(wasm32)`): a small blocking client. One thread per connection moves the
//!   frames, so `Transport::send` and `recv` never wait, as the trait asks. `ws://` and
//!   `wss://` (TLS with rustls and the `ring` provider; roots from `webpki-roots`, and with the
//!   feature `native-roots` also from the operating system).
//! - **Browser** (`wasm32`): the `WebSocket` of the browser through `web-sys`. A browser cannot
//!   set headers, so the token goes in the sub-protocol.
//!
//! # Signing in
//!
//! The token goes with the upgrade request, so the server can refuse a caller before it loads
//! the document (see `docs/collaboration.md`, section 4.1). [`TokenMode::Auto`] (the default)
//! sends `Authorization: Bearer <token>`; if the server answers the upgrade with an HTTP error
//! (for example a proxy removed the header), it tries once more with the sub-protocols
//! `kadakas.v1, kadakas.token.<token>`. The client also sends the token in its `hello`.
//!
//! # Limits
//!
//! The frame limits are those of the protocol (`kadakas_collab::op`): the client sends no frame
//! above `MAX_FRAME_BYTES` (64 MiB) and reads a frame of up to `MAX_SNAPSHOT_BYTES` (1 GiB).
//!
//! # Tokens are secrets
//!
//! The token lives in memory. This crate never writes it to a file or a log, the `Debug` text of
//! [`WsEndpoint`] hides it, and `describe()` and error texts show the address without a query.
//!
//! # Reconnecting
//!
//! A transport is one connection. To connect again after a loss, use
//! [`kadakas_collab::transport::Reconnector`] (a back-off of 250 ms up to 30 s) with a
//! [`WsEndpoint`]. The API session does this by itself.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{WsEndpoint, WsTransport};

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::{WsEndpoint, WsTransport};

#[cfg(all(feature = "test-server", not(target_arch = "wasm32")))]
pub mod testserver;

pub use kadakas_collab::url::{CollabUrl, UrlError};

use kadakas_collab::transport::{Endpoint, TransportError};

/// How the token goes with the upgrade request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TokenMode {
    /// `Authorization: Bearer`, and the sub-protocol if the server answers with an HTTP error.
    #[default]
    Auto,
    /// Only `Authorization: Bearer`.
    Header,
    /// Only the sub-protocols `kadakas.v1, kadakas.token.<token>`.
    Subprotocol,
}

/// The sub-protocol that the server speaks.
pub const SUBPROTOCOL: &str = "kadakas.v1";
/// The prefix of the sub-protocol that carries the token.
pub const TOKEN_SUBPROTOCOL_PREFIX: &str = "kadakas.token.";

/// True if the token can travel as a sub-protocol (HTTP token characters). A JWT can.
pub fn token_fits_subprotocol(token: &str) -> bool {
    !token.is_empty()
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
}

/// Makes an endpoint for the address of `collab.connect`. The API session uses this function as
/// its factory on a host with a network.
pub fn endpoint_for(url: &str, token: Option<&str>) -> Result<Box<dyn Endpoint>, TransportError> {
    let ep = WsEndpoint::new(url, token).map_err(|e| TransportError::Failed(e.to_string()))?;
    Ok(Box::new(ep))
}
