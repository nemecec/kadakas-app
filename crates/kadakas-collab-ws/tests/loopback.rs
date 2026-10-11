//! The WebSocket transport against the in-process test server (the reference sequencer behind
//! real sockets): sign-in, convergence, reconnects and the frame limit.

use std::time::{Duration, Instant};

use kadakas_collab::client::{Client, ClientConfig, ConnState};
use kadakas_collab::op::Op;
use kadakas_collab::transport::{Endpoint, Link, Reconnector, Transport, TransportError};
use kadakas_collab_ws::testserver::TestServer;
use kadakas_collab_ws::{TokenMode, WsEndpoint};
use kadakas_core::uid::Uid;
use serde_json::json;

fn uid(n: u64) -> Uid {
    Uid::from_num(n)
}

struct Peer {
    link: Link,
    ep: WsEndpoint,
    rec: Reconnector,
    started: Instant,
}

impl Peer {
    fn new(url: &str, token: Option<&str>, user: u64, name: &str) -> Peer {
        let client = Client::new(ClientConfig {
            user: uid(user),
            client: uid(1000 + user),
            name: name.into(),
            token: token.unwrap_or_default().into(),
        });
        let mut rec = Reconnector::new();
        rec.enabled = true;
        Peer {
            link: Link::new(client),
            ep: WsEndpoint::new(url, token).unwrap(),
            rec,
            started: Instant::now(),
        }
    }

    fn pump(&mut self) {
        let now = self.started.elapsed().as_millis() as u64;
        self.rec.poll(&mut self.link, &mut self.ep, now);
        self.link.pump(now);
    }

    fn until(&mut self, what: &str, mut f: impl FnMut(&Client) -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while !f(&self.link.client) {
            assert!(Instant::now() < end, "timeout: {what}");
            self.pump();
            std::thread::sleep(Duration::from_millis(3));
        }
    }

    fn online(&mut self) {
        self.until("online", |c| c.state() == ConnState::Online);
    }
}

fn create(id: u64) -> Vec<Op> {
    serde_json::from_value(
        json!([{"op": "create", "target": {"kind": "object", "id": uid(id)},
        "props": {"layer": uid(1), "class": uid(0),
                  "shape": {"kind": "rect", "min": [0, 0], "max": [5000, 3000]}}}]),
    )
    .unwrap()
}

fn set_width(id: u64, w: i64) -> Vec<Op> {
    serde_json::from_value(
        json!([{"op": "set", "target": {"kind": "object", "id": uid(id)},
        "prop": "shape", "value": {"kind": "rect", "min": [0, 0], "max": [w, 3000]}}]),
    )
    .unwrap()
}

fn converge(a: &mut Peer, b: &mut Peer) {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        a.pump();
        b.pump();
        let ready = |p: &Peer| {
            p.link.client.state() == ConnState::Online && p.link.client.pending().is_empty()
        };
        if ready(a)
            && ready(b)
            && a.link.client.view().canonical_text() == b.link.client.view().canonical_text()
            && a.link.client.confirmed_seq() == b.link.client.confirmed_seq()
        {
            return;
        }
        assert!(Instant::now() < end, "no convergence");
        std::thread::sleep(Duration::from_millis(3));
    }
}

#[test]
fn two_clients_converge_over_real_sockets() {
    let server = TestServer::start();
    let url = server.url("doc");
    let (mut a, mut b) = (
        Peer::new(&url, None, 1, "Ann"),
        Peer::new(&url, None, 2, "Bob"),
    );
    a.link.connect(&mut a.ep).unwrap();
    b.link.connect(&mut b.ep).unwrap();
    a.online();
    b.online();
    a.link.client.edit(create(10)).unwrap();
    b.link.client.edit(create(11)).unwrap();
    converge(&mut a, &mut b);
    a.link.client.edit(set_width(10, 1234)).unwrap();
    b.link.client.edit(set_width(10, 4321)).unwrap();
    converge(&mut a, &mut b);
    let text = a.link.client.view().canonical_text();
    assert!(text.contains(&uid(10).to_string()) && text.contains(&uid(11).to_string()));
    server.with(|s| assert_eq!(s.connections(), 2));
}

#[test]
fn the_token_goes_in_the_authorization_header() {
    let server = TestServer::start();
    server.require_token("secret-token", uid(1));
    let url = server.url("doc");
    let mut a = Peer::new(&url, Some("secret-token"), 1, "Ann");
    a.link.connect(&mut a.ep).unwrap();
    a.online();
    let seen = server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].path, "/ws/doc");
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some("Bearer secret-token")
    );
    assert_eq!(seen[0].protocols, None);
}

/// Reads from the transport until it ends. Returns how it ended.
fn end_of(t: &mut dyn Transport) -> TransportError {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Err(e) = t.recv() {
            return e;
        }
        assert!(Instant::now() < end, "the transport never ended");
        std::thread::sleep(Duration::from_millis(3));
    }
}

#[test]
fn it_falls_back_to_the_subprotocol_only_in_auto_mode_and_only_for_400() {
    let server = TestServer::start();
    server.require_token("tok.en-1", uid(1));
    server.refuse_authorization_header(true);
    let url = server.url("doc");
    let mut a = Peer::new(&url, Some("tok.en-1"), 1, "Ann");
    a.ep = WsEndpoint::new(&url, Some("tok.en-1"))
        .unwrap()
        .with_token_mode(TokenMode::Auto);
    a.link.connect(&mut a.ep).unwrap();
    a.online();
    let seen = server.seen();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(seen[0].authorization.is_some());
    assert_eq!(seen[1].authorization, None);
    assert_eq!(
        seen[1].protocols.as_deref(),
        Some("kadakas.v1, kadakas.token.tok.en-1")
    );

    // The default mode (header only) does not try again.
    let before = server.seen().len();
    let mut ep = WsEndpoint::new(&url, Some("tok.en-1")).unwrap();
    let mut t = ep.connect().unwrap();
    assert!(matches!(end_of(&mut *t), TransportError::Closed));
    assert!(t.failure().unwrap().contains("400"));
    assert_eq!(server.seen().len(), before + 1);

    // A 401 is a refusal, also in auto mode: no second try with the sub-protocol.
    server.refuse_authorization_header(false);
    let before = server.seen().len();
    let mut ep = WsEndpoint::new(&url, Some("wrong-token"))
        .unwrap()
        .with_token_mode(TokenMode::Auto);
    let mut t = ep.connect().unwrap();
    assert!(matches!(end_of(&mut *t), TransportError::Refused(_)));
    assert_eq!(server.seen().len(), before + 1);
}

#[test]
fn the_subprotocol_mode_and_a_refused_token() {
    let server = TestServer::start();
    server.require_token("good", uid(1));
    let url = server.url("doc");
    let mut p = Peer::new(&url, Some("good"), 1, "Ann");
    p.ep = WsEndpoint::new(&url, Some("good"))
        .unwrap()
        .with_token_mode(TokenMode::Subprotocol);
    p.link.connect(&mut p.ep).unwrap();
    p.online();
    assert_eq!(server.seen()[0].authorization, None);

    // A wrong token: the upgrade is refused (401), and the error says so without the token.
    let mut ep = WsEndpoint::new(&url, Some("wrong-token")).unwrap();
    let mut t = ep.connect().unwrap();
    let err = end_of(&mut *t);
    let TransportError::Refused(text) = err else {
        panic!("{err:?}");
    };
    assert!(text.contains("401"), "{text}");
    assert!(!text.contains("wrong-token"));
    assert!(!t.failure().unwrap().contains("wrong-token"));

    // 403 says 403, not 401.
    server.refuse_with_status(Some(403));
    let mut t = WsEndpoint::new(&url, Some("good"))
        .unwrap()
        .connect()
        .unwrap();
    let TransportError::Refused(text) = end_of(&mut *t) else {
        panic!("403 is a refusal");
    };
    assert!(text.contains("HTTP 403") && !text.contains("401"), "{text}");

    // Another status is a failure to connect and not a refusal.
    server.refuse_with_status(Some(503));
    let mut t = WsEndpoint::new(&url, Some("good"))
        .unwrap()
        .connect()
        .unwrap();
    assert!(matches!(end_of(&mut *t), TransportError::Closed));
    assert!(t.failure().unwrap().contains("503"));
}

#[test]
fn a_refused_token_fails_the_client_for_good_and_the_tries_stop() {
    let server = TestServer::start();
    server.require_token("good", uid(1));
    let mut a = Peer::new(&server.url("doc"), Some("wrong"), 1, "Ann");
    a.rec.jitter = false;
    a.link.connect(&mut a.ep).unwrap();
    a.until("failed", |c| c.state() == ConnState::Failed);
    let events = a.link.client.take_events();
    assert!(events.iter().any(|e| matches!(
        e,
        kadakas_collab::client::ClientEvent::Failed {
            code: kadakas_collab::protocol::ErrorCode::AuthFailed,
            ..
        }
    )));
    assert!(a.link.last_error.as_deref().unwrap().contains("401"));
    let tries = server.seen().len();
    for _ in 0..60 {
        a.pump();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(server.seen().len(), tries, "no new tries");
    assert_eq!(a.link.client.state(), ConnState::Failed);
}

#[test]
fn connect_returns_at_once_and_close_does_not_wait_for_a_connection_that_is_not_there() {
    // A listener that never answers: the TCP connection opens (the system queues it) and the
    // WebSocket handshake never ends. A call that waited for it would block this test for good.
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}/ws/x", silent.local_addr().unwrap());
    let mut ep = WsEndpoint::new(&url, None).unwrap();
    let mut t = ep.connect().unwrap();
    assert!(matches!(t.recv(), Ok(None)), "nothing arrives yet");
    t.close();
    // Messages sent while the socket is not open yet wait in the queue.
    let server = TestServer::start();
    let mut t = WsEndpoint::new(&server.url("doc"), None)
        .unwrap()
        .connect()
        .unwrap();
    t.send(&kadakas_collab::protocol::ClientMsg::Bye).unwrap();
    t.close();
}

#[test]
fn a_token_never_goes_over_plain_ws_to_another_host() {
    for url in [
        "ws://example.org/ws/x",
        "ws://localhost.evil.org/ws/x",
        "ws://127.0.0.1.nip.io/ws/x",
        "ws://10.0.0.5/ws/x",
    ] {
        assert!(WsEndpoint::new(url, Some("t")).is_err(), "{url}");
        assert!(WsEndpoint::new(url, None).is_ok(), "{url}");
    }
    assert!(WsEndpoint::new("wss://example.org/ws/x", Some("t")).is_ok());
    assert!(WsEndpoint::new("ws://localhost:1/ws/x", Some("t")).is_ok());
    assert!(WsEndpoint::new("ws://[::1]:1/ws/x", Some("t")).is_ok());
}

#[test]
fn the_debug_text_and_the_description_hide_the_token() {
    let ep = WsEndpoint::new("wss://example.org:81/ws/x", Some("very-secret")).unwrap();
    assert!(!format!("{ep:?}").contains("very-secret"));
    assert_eq!(ep.describe(), "wss://example.org:81/ws/x");
    assert!(WsEndpoint::new("ws://example.org/ws/x?token=abc", None).is_err());
    assert!(WsEndpoint::new("ws://u:p@example.org/ws/x", None).is_err());
    // The Debug text of the messages and of the configuration hides it too.
    let cfg = ClientConfig {
        user: uid(1),
        client: uid(2),
        name: "Ann".into(),
        token: "very-secret".into(),
    };
    assert!(!format!("{cfg:?}").contains("very-secret"));
    let mut client = Client::new(cfg);
    client.connect();
    let hello = client.take_outbox();
    assert!(!format!("{hello:?}").contains("very-secret"));
}

#[test]
fn a_dead_server_is_an_error_and_not_a_hang() {
    let mut server = TestServer::start();
    let url = server.url("doc");
    server.stop();
    let mut ep = WsEndpoint::new(&url, None).unwrap();
    let mut t = ep.connect().unwrap();
    assert!(matches!(end_of(&mut *t), TransportError::Closed));
    assert!(t.failure().unwrap().contains("cannot connect"));
    // wss to a server that speaks no TLS fails with an error too.
    let live = TestServer::start();
    let wss = live.url("doc").replace("ws://", "wss://");
    let mut t = WsEndpoint::new(&wss, None).unwrap().connect().unwrap();
    assert!(matches!(end_of(&mut *t), TransportError::Closed));
}

#[test]
fn reconnects_after_a_restart_and_keeps_offline_edits() {
    let mut server = TestServer::start();
    let url = server.url("doc");
    let (mut a, mut b) = (
        Peer::new(&url, None, 1, "Ann"),
        Peer::new(&url, None, 2, "Bob"),
    );
    a.link.connect(&mut a.ep).unwrap();
    b.link.connect(&mut b.ep).unwrap();
    a.online();
    b.online();
    a.link.client.edit(create(10)).unwrap();
    converge(&mut a, &mut b);

    server.stop();
    // Both notice the loss and work offline.
    a.until("offline", |c| c.state() == ConnState::Offline);
    b.until("offline", |c| c.state() == ConnState::Offline);
    a.link.client.edit(set_width(10, 111)).unwrap();
    b.link.client.edit(create(11)).unwrap();
    // While the server is down, the reconnector only fails and waits.
    for _ in 0..20 {
        a.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_ne!(a.link.client.state(), ConnState::Online);
    server.restart();
    // The back-off is at most a few seconds after a few failed tries.
    converge(&mut a, &mut b);
    let text = b.link.client.view().canonical_text();
    assert!(
        text.contains("111") && text.contains(&uid(11).to_string()),
        "{text}"
    );
}

#[test]
fn a_frame_above_the_limit_is_refused_before_it_is_sent() {
    let server = TestServer::start();
    let mut t = kadakas_collab_ws::WsTransport::connect(&server.url("doc"), None).unwrap();
    let msg = kadakas_collab::protocol::ClientMsg::Hello {
        protocol: 1,
        user: uid(1),
        client: uid(2),
        name: "x".repeat(kadakas_collab::op::MAX_FRAME_BYTES + 1),
        token: String::new(),
        last_seq: None,
        log_id: None,
        last_hash: None,
    };
    let err = t.send(&msg).unwrap_err().to_string();
    assert!(err.contains("limit"), "{err}");
    t.close();
}

#[test]
fn the_test_server_ignores_other_frames_and_drops_a_client_that_sends_bad_json() {
    let server = TestServer::start();
    assert_eq!(
        server.base_url(),
        format!("ws://127.0.0.1:{}", server.port())
    );
    assert_eq!(server.url("d"), format!("{}/ws/d", server.base_url()));
    let stream = std::net::TcpStream::connect(("127.0.0.1", server.port())).unwrap();
    let (mut ws, _) = tungstenite::client(server.url("d"), stream).unwrap();
    // A binary frame and a ping are not messages: the server goes on.
    ws.send(tungstenite::Message::binary(vec![1, 2, 3]))
        .unwrap();
    ws.send(tungstenite::Message::Ping(Default::default()))
        .unwrap();
    // Text that is not a message ends the connection.
    ws.send(tungstenite::Message::text("this is not json"))
        .unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        match ws.read() {
            Ok(tungstenite::Message::Close(_)) | Err(_) => break,
            Ok(_) => {}
        }
        assert!(Instant::now() < end, "the server kept the connection");
    }
}
