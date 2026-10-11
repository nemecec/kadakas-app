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
    /// The server refused the credentials (for example HTTP 401 or 403 on the upgrade). Trying
    /// again with the same token cannot work, so a host stops.
    #[error("{0}")]
    Refused(String),
}

/// One connection to a server, from the client side.
pub trait Transport: Send {
    /// Sends a message.
    fn send(&mut self, msg: &ClientMsg) -> Result<(), TransportError>;
    /// Takes the next message, or `None` if there is none now. Never waits.
    fn recv(&mut self) -> Result<Option<ServerMsg>, TransportError>;
    /// Closes the connection.
    fn close(&mut self);
    /// Why the connection failed or ended, if the transport knows. The text never holds a
    /// token.
    fn failure(&self) -> Option<String> {
        None
    }
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
    /// True if the server refused the credentials (the client is `Failed`).
    pub refused: bool,
}

/// A client with its connection.
pub struct Link {
    pub client: Client,
    transport: Option<Box<dyn Transport>>,
    /// Why the last connection failed or ended, if the transport said. Never holds a token.
    pub last_error: Option<String>,
}

impl Link {
    pub fn new(client: Client) -> Link {
        Link {
            client,
            transport: None,
            last_error: None,
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
                Err(TransportError::Refused(why)) => {
                    self.client.auth_refused(&why);
                    self.last_error = Some(why);
                    report.lost = true;
                    report.refused = true;
                    break;
                }
                Err(e) => {
                    self.last_error = Some(t.failure().unwrap_or_else(|| e.to_string()));
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

/// How long to wait before the next try to connect: 250 ms, then 500 ms, 1 s and so on, up to
/// 30 s (the same steps as the retry of section 4.6 of the protocol). It has no random part, so
/// it is easy to test; a host that runs many clients may add its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Backoff {
    next_ms: u64,
}

impl Backoff {
    pub const FIRST_MS: u64 = 250;
    pub const MAX_MS: u64 = 30_000;

    pub fn new() -> Backoff {
        Backoff {
            next_ms: Self::FIRST_MS,
        }
    }

    /// The wait for this try. The next one is twice as long, up to the limit.
    pub fn next_delay_ms(&mut self) -> u64 {
        let now = self.next_ms;
        self.next_ms = (self.next_ms * 2).min(Self::MAX_MS);
        now
    }

    /// Start again with the short wait (after a connection that worked).
    pub fn reset(&mut self) {
        self.next_ms = Self::FIRST_MS;
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff::new()
    }
}

/// How long a connection must stay online before the back-off starts again with the short wait.
/// An ack or an entry from the server counts too (the server works).
pub const BACKOFF_RESET_AFTER_MS: u64 = 10_000;

/// Connects a [`Link`] again after the connection was lost, with a [`Backoff`] between tries
/// (with up to 20% more or less, so many clients do not come back at the same moment). The host
/// calls [`Reconnector::poll`] before every [`Link::pump`].
///
/// The back-off starts again with the short wait only after the client has been online for
/// [`BACKOFF_RESET_AFTER_MS`], or after it got a sequence number from the server. A server that
/// accepts the connection and closes it at once does not reset it.
///
/// It does not connect a client that is `Failed` (a refused token or protocol, a frame that
/// cannot be read), and it stops for good (`enabled` becomes false) when the server refuses the
/// credentials at the door ([`TransportError::Refused`]).
#[derive(Debug)]
pub struct Reconnector {
    pub enabled: bool,
    /// Vary each wait by up to 20%. Tests switch it off.
    pub jitter: bool,
    backoff: Backoff,
    next_at_ms: u64,
    /// When the client went online, and its sequence number then.
    online: Option<(u64, u64)>,
    rng: u64,
}

impl Default for Reconnector {
    fn default() -> Self {
        Reconnector {
            enabled: false,
            jitter: true,
            backoff: Backoff::new(),
            next_at_ms: 0,
            online: None,
            rng: 0,
        }
    }
}

impl Reconnector {
    pub fn new() -> Reconnector {
        Reconnector::default()
    }

    /// The next wait, in milliseconds, with the jitter.
    fn wait_ms(&mut self, now_ms: u64) -> u64 {
        let base = self.backoff.next_delay_ms();
        if !self.jitter {
            return base;
        }
        if self.rng == 0 {
            self.rng = now_ms ^ 0x9E37_79B9_7F4A_7C15;
            if self.rng == 0 {
                self.rng = 1;
            }
        }
        // xorshift64
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        let unit = (self.rng % 10_000) as f64 / 10_000.0; // [0, 1)
        (base as f64 * (0.8 + 0.4 * unit)) as u64
    }

    /// Makes one try if the link is offline and the wait is over. Returns the error of a try that
    /// failed, `None` otherwise.
    pub fn poll(
        &mut self,
        link: &mut Link,
        endpoint: &mut dyn Endpoint,
        now_ms: u64,
    ) -> Option<TransportError> {
        use crate::client::ConnState;
        if !self.enabled {
            return None;
        }
        match link.client.state() {
            ConnState::Online => {
                let seq = link.client.confirmed_seq();
                match self.online {
                    None => self.online = Some((now_ms, seq)),
                    Some((since, at)) => {
                        if now_ms.saturating_sub(since) >= BACKOFF_RESET_AFTER_MS || seq != at {
                            self.backoff.reset();
                        }
                    }
                }
                return None;
            }
            ConnState::Offline => self.online = None,
            // Connecting, syncing, or failed for good.
            _ => return None,
        }
        if link.is_connected() || now_ms < self.next_at_ms {
            return None;
        }
        // The wait grows with every try.
        self.next_at_ms = now_ms + self.wait_ms(now_ms);
        match link.connect(endpoint) {
            Ok(()) => None,
            Err(TransportError::Refused(why)) => {
                link.client.auth_refused(&why);
                link.last_error = Some(why.clone());
                self.enabled = false;
                Some(TransportError::Refused(why))
            }
            Err(e) => {
                link.last_error = Some(e.to_string());
                Some(e)
            }
        }
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
            chain: None,
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

    #[test]
    fn backoff_doubles_up_to_the_limit_and_resets() {
        let mut b = Backoff::new();
        let steps: Vec<u64> = (0..10).map(|_| b.next_delay_ms()).collect();
        assert_eq!(
            steps,
            [250, 500, 1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000]
        );
        b.reset();
        assert_eq!(b.next_delay_ms(), 250);
    }

    #[test]
    fn reconnector_waits_between_tries() {
        struct Failing(usize);
        impl Endpoint for Failing {
            fn describe(&self) -> String {
                "failing".into()
            }
            fn connect(&mut self) -> Result<Box<dyn Transport>, TransportError> {
                self.0 += 1;
                Err(TransportError::Failed("down".into()))
            }
        }
        let mut link = Link::new(client());
        let mut ep = Failing(0);
        let mut r = Reconnector::new();
        r.jitter = false;
        // Not enabled: nothing happens.
        assert!(r.poll(&mut link, &mut ep, 0).is_none());
        assert_eq!(ep.0, 0);
        r.enabled = true;
        assert!(r.poll(&mut link, &mut ep, 1000).is_some());
        assert_eq!(ep.0, 1);
        // 250 ms wait, then 500 ms.
        assert!(r.poll(&mut link, &mut ep, 1100).is_none());
        assert!(r.poll(&mut link, &mut ep, 1250).is_some());
        assert!(r.poll(&mut link, &mut ep, 1600).is_none());
        assert!(r.poll(&mut link, &mut ep, 1750).is_some());
        assert_eq!(ep.0, 3);
    }

    /// An endpoint whose transports fail in a way the test chooses.
    struct Scripted {
        tries: usize,
        refuse: bool,
    }

    struct Dead(Option<TransportError>);

    impl Transport for Dead {
        fn send(&mut self, _: &ClientMsg) -> Result<(), TransportError> {
            Ok(())
        }
        fn recv(&mut self) -> Result<Option<ServerMsg>, TransportError> {
            Err(self.0.take().unwrap_or(TransportError::Closed))
        }
        fn close(&mut self) {}
        fn failure(&self) -> Option<String> {
            Some("it died".into())
        }
    }

    impl Endpoint for Scripted {
        fn describe(&self) -> String {
            "scripted".into()
        }
        fn connect(&mut self) -> Result<Box<dyn Transport>, TransportError> {
            self.tries += 1;
            Ok(Box::new(Dead(self.refuse.then(|| {
                TransportError::Refused("the server refused the token (HTTP 401)".into())
            }))))
        }
    }

    #[test]
    fn a_refused_token_fails_the_client_and_stops_the_reconnects() {
        let mut link = Link::new(client());
        let mut ep = Scripted {
            tries: 0,
            refuse: true,
        };
        let mut r = Reconnector::new();
        r.enabled = true;
        r.jitter = false;
        assert!(r.poll(&mut link, &mut ep, 0).is_none());
        let rep = link.pump(1);
        assert!(rep.refused && rep.lost);
        assert_eq!(link.client.state(), ConnState::Failed);
        let events = link.client.take_events();
        assert!(events.iter().any(|e| matches!(
            e,
            crate::client::ClientEvent::Failed {
                code: crate::protocol::ErrorCode::AuthFailed,
                ..
            }
        )));
        assert!(link.last_error.as_deref().unwrap().contains("401"));
        // No more tries, however long we wait.
        for t in [100, 10_000, 100_000] {
            assert!(r.poll(&mut link, &mut ep, t).is_none());
        }
        assert_eq!(ep.tries, 1);
    }

    #[test]
    fn a_lost_connection_keeps_the_reason() {
        let mut link = Link::new(client());
        let mut ep = Scripted {
            tries: 0,
            refuse: false,
        };
        link.connect(&mut ep).unwrap();
        let rep = link.pump(1);
        assert!(rep.lost && !rep.refused);
        assert_eq!(link.last_error.as_deref(), Some("it died"));
        assert_eq!(link.client.state(), ConnState::Offline);
    }

    #[test]
    fn the_wait_has_a_jitter_of_20_percent() {
        let mut r = Reconnector::new();
        let mut seen = std::collections::BTreeSet::new();
        for i in 0..200 {
            r.backoff.reset();
            let w = r.wait_ms(1000 + i);
            assert!((200..=300).contains(&w), "{w}");
            seen.insert(w);
        }
        assert!(seen.len() > 10, "the waits vary");
    }

    #[test]
    fn the_backoff_resets_after_ten_seconds_online_or_a_sequence_number() {
        use crate::replica::Replica;
        let mut link = Link::new(client());
        let mut ep = Scripted {
            tries: 0,
            refuse: false,
        };
        let mut r = Reconnector::new();
        r.enabled = true;
        r.jitter = false;
        // Fail a few times: the wait grows.
        for t in [0, 300, 900] {
            r.poll(&mut link, &mut ep, t);
            link.pump(t);
        }
        assert_eq!(r.backoff.next_ms, 2000);
        // Online for a short time only (a server that closes at once): no reset.
        let mut done = Link::new(client());
        done.client.connect();
        let _ = done.client.take_outbox();
        done.client.handle(
            ServerMsg::Welcome {
                protocol: 1,
                log_id: "l".into(),
                head_seq: 0,
                your_last_op: 0,
                peers: vec![],
            },
            0,
        );
        done.client.handle(
            ServerMsg::Snapshot {
                log_id: "l".into(),
                seq: 0,
                state: Replica::new(),
                chain: None,
            },
            0,
        );
        assert_eq!(done.client.state(), ConnState::Online);
        r.poll(&mut done, &mut ep, 5_000);
        r.poll(&mut done, &mut ep, 8_000);
        assert_eq!(r.backoff.next_ms, 2000);
        r.poll(&mut done, &mut ep, 15_001);
        assert_eq!(r.backoff.next_ms, Backoff::FIRST_MS);
    }
}
