//! Moving messages: the transport trait, the endpoint trait, an in-memory transport and the
//! [`Link`] that connects a [`Client`] to a transport.
//!
//! This crate has no network code. A WebSocket transport (or any other) is a small type that
//! implements [`Transport`] in a native or browser crate. The trait is not async: `recv` never
//! waits, it returns `Ok(None)` when no message is ready. The host calls [`Link::pump`] often (for
//! example once per frame, or when the socket is readable).
//!
//! A transport sends and receives whole messages. A real transport writes each message as one JSON
//! text frame. The in-memory transport does the same, so tests also cover the JSON form.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::client::Client;
use crate::protocol::{ClientMsg, ServerMsg};

/// Why a transport call failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    /// The connection is closed.
    #[error("the connection is closed")]
    Closed,
    /// The other side sent something that is not a message.
    #[error("bad message: {0}")]
    BadMessage(String),
    /// The transport could not connect or send.
    #[error("transport failed: {0}")]
    Failed(String),
}

/// One connection to a server, from the client side.
pub trait Transport: Send {
    /// Sends a message.
    fn send(&mut self, msg: &ClientMsg) -> Result<(), TransportError>;
    /// Takes the next message, or `None` if there is none now. Never waits.
    fn recv(&mut self) -> Result<Option<ServerMsg>, TransportError>;
    /// Closes the connection.
    fn close(&mut self);
}

/// Where to connect: it makes transports. The host keeps the address and the sign-in details
/// that the transport needs.
pub trait Endpoint: Send {
    /// A short text for people: the address, or "in-memory".
    fn describe(&self) -> String;
    fn connect(&mut self) -> Result<Box<dyn Transport>, TransportError>;
}

#[derive(Default)]
struct Pipe {
    to_server: VecDeque<String>,
    to_client: VecDeque<String>,
    closed: bool,
}

/// The client end of an in-memory connection.
pub struct MemTransport {
    pipe: Arc<Mutex<Pipe>>,
}

/// The other end of an in-memory connection. A test or a reference server holds it.
pub struct MemPeer {
    pipe: Arc<Mutex<Pipe>>,
}

/// Makes the two ends of an in-memory connection.
pub fn mem_pair() -> (MemTransport, MemPeer) {
    let pipe = Arc::new(Mutex::new(Pipe::default()));
    (MemTransport { pipe: pipe.clone() }, MemPeer { pipe })
}

impl Transport for MemTransport {
    fn send(&mut self, msg: &ClientMsg) -> Result<(), TransportError> {
        let mut p = self.pipe.lock().unwrap();
        if p.closed {
            return Err(TransportError::Closed);
        }
        p.to_server
            .push_back(serde_json::to_string(msg).expect("a message is JSON"));
        Ok(())
    }

    fn recv(&mut self) -> Result<Option<ServerMsg>, TransportError> {
        let mut p = self.pipe.lock().unwrap();
        match p.to_client.pop_front() {
            Some(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|e| TransportError::BadMessage(e.to_string())),
            None if p.closed => Err(TransportError::Closed),
            None => Ok(None),
        }
    }

    fn close(&mut self) {
        self.pipe.lock().unwrap().closed = true;
    }
}

impl MemPeer {
    /// The next message of the client, or `None`. A text that is not a message is an error.
    pub fn recv(&mut self) -> Result<Option<ClientMsg>, TransportError> {
        let mut p = self.pipe.lock().unwrap();
        match p.to_server.pop_front() {
            Some(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|e| TransportError::BadMessage(e.to_string())),
            None => Ok(None),
        }
    }

    /// Sends a message to the client. Messages to a closed connection are lost.
    pub fn send(&mut self, msg: &ServerMsg) {
        let mut p = self.pipe.lock().unwrap();
        if !p.closed {
            p.to_client
                .push_back(serde_json::to_string(msg).expect("a message is JSON"));
        }
    }

    /// Sends a text as it is, to test how a client treats a bad message.
    pub fn send_raw(&mut self, text: &str) {
        self.pipe
            .lock()
            .unwrap()
            .to_client
            .push_back(text.to_string());
    }

    pub fn is_closed(&self) -> bool {
        self.pipe.lock().unwrap().closed
    }

    /// Closes the connection from the server side.
    pub fn close(&mut self) {
        self.pipe.lock().unwrap().closed = true;
    }
}

/// What one call of [`Link::pump`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PumpReport {
    pub received: usize,
    pub sent: usize,
    /// True if the connection ended during the call.
    pub lost: bool,
}

/// A client with its connection.
pub struct Link {
    pub client: Client,
    transport: Option<Box<dyn Transport>>,
}

impl Link {
    pub fn new(client: Client) -> Link {
        Link {
            client,
            transport: None,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.transport.is_some()
    }

    /// Connects through an endpoint and starts the sign-in. The server's answer comes with
    /// [`Link::pump`].
    pub fn connect(&mut self, endpoint: &mut dyn Endpoint) -> Result<(), TransportError> {
        self.drop_transport();
        self.transport = Some(endpoint.connect()?);
        self.client.connect();
        Ok(())
    }

    /// Uses a transport that the host made, and starts the sign-in.
    pub fn attach(&mut self, transport: Box<dyn Transport>) {
        self.drop_transport();
        self.transport = Some(transport);
        self.client.connect();
    }

    /// Says goodbye and closes the connection.
    pub fn disconnect(&mut self) {
        self.client.disconnect();
        self.flush();
        self.drop_transport();
    }

    fn drop_transport(&mut self) {
        if let Some(mut t) = self.transport.take() {
            t.close();
        }
    }

    /// Reads the messages that are ready, lets the client handle them, runs the client timers
    /// and sends what the client wants to send.
    pub fn pump(&mut self, now_ms: u64) -> PumpReport {
        let mut report = PumpReport::default();
        let Some(t) = self.transport.as_mut() else {
            return report;
        };
        loop {
            match t.recv() {
                Ok(Some(msg)) => {
                    report.received += 1;
                    self.client.handle(msg, now_ms);
                }
                Ok(None) => break,
                Err(TransportError::BadMessage(why)) => {
                    // Reconnecting would bring the same frame again: stop for good.
                    self.client.bad_frame(&why);
                    report.lost = true;
                    break;
                }
                Err(_) => {
                    report.lost = true;
                    break;
                }
            }
        }
        if !report.lost {
            self.client.tick(now_ms);
            let (sent, lost) = self.flush();
            report.sent = sent;
            report.lost = lost;
        }
        if report.lost {
            self.drop_transport();
            self.client.connection_lost();
        }
        report
    }

    /// Sends the queued messages. Returns how many went out and whether the connection broke.
    fn flush(&mut self) -> (usize, bool) {
        let Some(t) = self.transport.as_mut() else {
            self.client.take_outbox();
            return (0, false);
        };
        let mut sent = 0;
        for msg in self.client.take_outbox() {
            if t.send(&msg).is_err() {
                return (sent, true);
            }
            sent += 1;
        }
        (sent, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{ClientConfig, ConnState};
    use kadakas_core::uid::Uid;

    fn client() -> Client {
        Client::new(ClientConfig {
            user: Uid::from_num(1),
            client: Uid::from_num(11),
            name: "A".into(),
            token: String::new(),
        })
    }

    struct OneShot(Option<Box<dyn Transport>>);

    impl Endpoint for OneShot {
        fn describe(&self) -> String {
            "one shot".into()
        }
        fn connect(&mut self) -> Result<Box<dyn Transport>, TransportError> {
            self.0
                .take()
                .ok_or_else(|| TransportError::Failed("no more".into()))
        }
    }

    #[test]
    fn mem_pair_moves_json_messages() {
        let (mut c, mut s) = mem_pair();
        assert_eq!(c.recv(), Ok(None));
        c.send(&ClientMsg::Bye).unwrap();
        assert_eq!(s.recv(), Ok(Some(ClientMsg::Bye)));
        assert_eq!(s.recv(), Ok(None));
        s.send(&ServerMsg::PresenceLeave {
            client: Uid::from_num(2),
        });
        assert!(matches!(
            c.recv(),
            Ok(Some(ServerMsg::PresenceLeave { .. }))
        ));
        s.send_raw("nonsense");
        assert!(matches!(c.recv(), Err(TransportError::BadMessage(_))));
        assert!(!s.is_closed());
        c.close();
        assert!(s.is_closed());
        assert_eq!(c.send(&ClientMsg::Bye), Err(TransportError::Closed));
        assert_eq!(c.recv(), Err(TransportError::Closed));
        s.send(&ServerMsg::PresenceLeave {
            client: Uid::from_num(2),
        });
        let (mut c2, mut s2) = mem_pair();
        c2.pipe.lock().unwrap().to_server.push_back("{".into());
        assert!(matches!(s2.recv(), Err(TransportError::BadMessage(_))));
        s2.close();
        assert!(c2.recv().is_err());
    }

    #[test]
    fn link_connects_and_sends_hello() {
        let (t, mut s) = mem_pair();
        let mut link = Link::new(client());
        assert!(!link.is_connected());
        assert_eq!(link.pump(0), PumpReport::default());
        let mut ep = OneShot(Some(Box::new(t)));
        assert_eq!(ep.describe(), "one shot");
        link.connect(&mut ep).unwrap();
        assert!(link.is_connected());
        assert_eq!(link.client.state(), ConnState::Connecting);
        let r = link.pump(0);
        assert_eq!(r.sent, 1);
        assert!(matches!(s.recv(), Ok(Some(ClientMsg::Hello { .. }))));
        assert!(link.connect(&mut ep).is_err());
    }

    #[test]
    fn link_goes_offline_when_the_connection_breaks() {
        let (t, mut s) = mem_pair();
        let mut link = Link::new(client());
        link.attach(Box::new(t));
        link.pump(0);
        s.send(&ServerMsg::Welcome {
            protocol: crate::protocol::PROTOCOL_VERSION,
            log_id: "log".into(),
            head_seq: 0,
            your_last_op: 0,
            peers: vec![],
        });
        let r = link.pump(1);
        assert_eq!(r.received, 1);
        assert_eq!(link.client.state(), ConnState::Syncing);
        s.close();
        let r = link.pump(2);
        assert!(r.lost);
        assert!(!link.is_connected());
        assert_eq!(link.client.state(), ConnState::Offline);
    }

    #[test]
    fn link_detects_a_broken_send() {
        let (t, mut s) = mem_pair();
        let mut link = Link::new(client());
        link.attach(Box::new(t));
        s.close();
        // The close is seen on receive first.
        assert!(link.pump(0).lost);
        // A transport that fails only on send.
        struct SendFails;
        impl Transport for SendFails {
            fn send(&mut self, _: &ClientMsg) -> Result<(), TransportError> {
                Err(TransportError::Failed("x".into()))
            }
            fn recv(&mut self) -> Result<Option<ServerMsg>, TransportError> {
                Ok(None)
            }
            fn close(&mut self) {}
        }
        link.attach(Box::new(SendFails));
        let r = link.pump(0);
        assert!(r.lost);
        assert_eq!(r.sent, 0);
        assert!(!TransportError::Closed.to_string().is_empty());
    }

    #[test]
    fn a_frame_that_cannot_be_read_stops_the_client_for_good() {
        let (t, mut s) = mem_pair();
        let mut link = Link::new(client());
        link.attach(Box::new(t));
        link.pump(0);
        s.send_raw("{\"type\": ");
        let r = link.pump(1);
        assert!(r.lost);
        assert!(!link.is_connected());
        assert_eq!(link.client.state(), ConnState::Failed);
        let events = link.client.take_events();
        assert!(events.iter().any(|e| matches!(
            e,
            crate::client::ClientEvent::Failed {
                code: crate::protocol::ErrorCode::BadMessage,
                ..
            }
        )));
    }

    #[test]
    fn disconnect_says_bye() {
        let (t, mut s) = mem_pair();
        let mut link = Link::new(client());
        link.attach(Box::new(t));
        link.pump(0);
        s.send(&ServerMsg::Welcome {
            protocol: crate::protocol::PROTOCOL_VERSION,
            log_id: "log".into(),
            head_seq: 0,
            your_last_op: 0,
            peers: vec![],
        });
        link.pump(1);
        s.send(&ServerMsg::Snapshot {
            log_id: "log".into(),
            seq: 0,
            state: crate::replica::Replica::new(),
        });
        link.pump(2);
        assert_eq!(link.client.state(), ConnState::Online);
        link.disconnect();
        assert!(s.is_closed());
        let mut got = vec![];
        while let Ok(Some(m)) = s.recv() {
            got.push(m);
        }
        assert!(got.contains(&ClientMsg::Bye));
        // A second disconnect with no transport is fine.
        link.disconnect();
    }
}
