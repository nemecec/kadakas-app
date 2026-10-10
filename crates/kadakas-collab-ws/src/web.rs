//! The WebSocket of the browser (`wasm32`).
//!
//! A browser cannot set request headers, so the token goes in the sub-protocols
//! `kadakas.v1, kadakas.token.<token>`. The callbacks of the browser put the frames into a queue,
//! and `recv` reads the queue, so the `Transport` calls never wait. The host calls
//! `Link::pump` (for example once per frame, or from a timer).
//!
//! The state is only in this file and only for the main thread of the browser. `Transport` asks
//! for `Send`; a browser tab has one thread, so the types here say so with an `unsafe impl`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use kadakas_collab::op::MAX_FRAME_BYTES;
use kadakas_collab::protocol::{ClientMsg, ServerMsg};
use kadakas_collab::transport::{Endpoint, Transport, TransportError};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{CloseEvent, MessageEvent, WebSocket};

use crate::{CollabUrl, SUBPROTOCOL, TOKEN_SUBPROTOCOL_PREFIX, UrlError, token_fits_subprotocol};

#[derive(Default)]
struct Shared {
    frames: VecDeque<String>,
    open: bool,
    closed: bool,
}

/// One browser WebSocket.
pub struct WsTransport {
    socket: WebSocket,
    shared: Rc<RefCell<Shared>>,
    out: VecDeque<String>,
    // The closures must live as long as the socket.
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_open: Closure<dyn FnMut()>,
    _on_close: Closure<dyn FnMut(CloseEvent)>,
}

// SAFETY: a browser tab runs this code on one thread. The value is never shared with a worker.
unsafe impl Send for WsTransport {}

impl WsTransport {
    /// Starts to connect. The socket opens later: `send` waits in the browser's own buffer.
    pub fn connect(url: &str, token: Option<&str>) -> Result<WsTransport, TransportError> {
        let ep = WsEndpoint::new(url, token).map_err(|e| TransportError::Failed(e.to_string()))?;
        ep.connect_now()
    }
}

impl WsTransport {
    fn flush(&mut self) -> Result<(), TransportError> {
        while let Some(text) = self.out.front() {
            match self.socket.ready_state() {
                0 => return Ok(()),
                1 => {
                    self.socket
                        .send_with_str(text)
                        .map_err(|_| TransportError::Closed)?;
                    self.out.pop_front();
                }
                _ => return Err(TransportError::Closed),
            }
        }
        Ok(())
    }
}

impl Transport for WsTransport {
    fn send(&mut self, msg: &ClientMsg) -> Result<(), TransportError> {
        if self.shared.borrow().closed {
            return Err(TransportError::Closed);
        }
        let text = serde_json::to_string(msg).expect("a message is JSON");
        if text.len() > MAX_FRAME_BYTES {
            return Err(TransportError::Failed(
                "the frame is above the limit".into(),
            ));
        }
        // While the socket is still connecting, messages wait here (the client sends `hello`
        // at once). They go out in order when the socket is open.
        self.out.push_back(text);
        self.flush()
    }

    fn recv(&mut self) -> Result<Option<ServerMsg>, TransportError> {
        self.flush()?;
        let mut s = self.shared.borrow_mut();
        match s.frames.pop_front() {
            Some(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|e| TransportError::BadMessage(e.to_string())),
            None if s.closed => Err(TransportError::Closed),
            None => Ok(None),
        }
    }

    fn close(&mut self) {
        let _ = self.socket.close();
        self.shared.borrow_mut().closed = true;
    }
}

impl Drop for WsTransport {
    fn drop(&mut self) {
        self.socket.set_onmessage(None);
        self.socket.set_onopen(None);
        self.socket.set_onclose(None);
        let _ = self.socket.close();
    }
}

/// Makes browser WebSockets for one document.
#[derive(Clone)]
pub struct WsEndpoint {
    url: CollabUrl,
    token: Option<String>,
}

impl std::fmt::Debug for WsEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsEndpoint")
            .field("url", &self.url.display())
            .field("token", &self.token.as_ref().map(|_| "<hidden>"))
            .finish()
    }
}

// SAFETY: see `WsTransport`.
unsafe impl Send for WsEndpoint {}

impl WsEndpoint {
    pub fn new(url: &str, token: Option<&str>) -> Result<WsEndpoint, UrlError> {
        Ok(WsEndpoint {
            url: CollabUrl::parse(url)?,
            token: token.filter(|t| !t.is_empty()).map(str::to_string),
        })
    }

    fn connect_now(&self) -> Result<WsTransport, TransportError> {
        let target = format!(
            "{}://{}:{}{}",
            self.url.scheme, self.url.host, self.url.port, self.url.path
        );
        let fail = |what: &str| TransportError::Failed(format!("{}: {what}", self.url.display()));
        let socket = match self.token.as_deref() {
            Some(t) if token_fits_subprotocol(t) => {
                let protocols = js_sys::Array::new();
                protocols.push(&SUBPROTOCOL.into());
                protocols.push(&format!("{TOKEN_SUBPROTOCOL_PREFIX}{t}").into());
                WebSocket::new_with_str_sequence(&target, &protocols)
            }
            Some(_) => return Err(fail("the token cannot travel in a sub-protocol")),
            None => WebSocket::new(&target),
        }
        .map_err(|_| fail("the browser refused the address"))?;
        let shared = Rc::new(RefCell::new(Shared::default()));
        let s = shared.clone();
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            if let Some(text) = e.data().as_string() {
                s.borrow_mut().frames.push_back(text);
            }
        });
        let s = shared.clone();
        let on_open = Closure::<dyn FnMut()>::new(move || s.borrow_mut().open = true);
        let s = shared.clone();
        let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |_| s.borrow_mut().closed = true);
        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        Ok(WsTransport {
            socket,
            shared,
            out: VecDeque::new(),
            _on_message: on_message,
            _on_open: on_open,
            _on_close: on_close,
        })
    }
}

impl Endpoint for WsEndpoint {
    fn describe(&self) -> String {
        self.url.display()
    }

    fn connect(&mut self) -> Result<Box<dyn Transport>, TransportError> {
        Ok(Box::new(self.connect_now()?))
    }
}
