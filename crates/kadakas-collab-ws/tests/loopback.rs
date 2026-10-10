//! The WebSocket transport against the in-process test server (the reference sequencer behind
//! real sockets): sign-in, convergence, reconnects and the frame limit.

use std::time::{Duration, Instant};

use kadakas_collab::client::{Client, ClientConfig, ConnState};
use kadakas_collab::op::Op;
use kadakas_collab::transport::{Endpoint, Link, Reconnector, Transport};
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

#[test]
fn it_falls_back_to_the_subprotocol_when_the_header_is_refused() {
    let server = TestServer::start();
    server.require_token("tok.en-1", uid(1));
    server.refuse_authorization_header(true);
    let mut a = Peer::new(&server.url("doc"), Some("tok.en-1"), 1, "Ann");
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
}

#[test]
fn the_subprotocol_mode_and_a_bad_token() {
    let server = TestServer::start();
    server.require_token("good", uid(1));
    let url = server.url("doc");
    let mut ep = WsEndpoint::new(&url, Some("good"))
        .unwrap()
        .with_token_mode(TokenMode::Subprotocol);
    assert!(ep.connect().is_ok());
    // A wrong token: the upgrade is refused, and the error says so without the token.
    let mut ep = WsEndpoint::new(&url, Some("wrong-token")).unwrap();
    let err = ep.connect().err().expect("refused").to_string();
    assert!(err.contains("401"), "{err}");
    assert!(!err.contains("wrong-token"), "{err}");
    // The header-only mode does not try again.
    server.refuse_with_status(Some(503));
    let before = server.seen().len();
    let mut ep = WsEndpoint::new(&url, Some("good"))
        .unwrap()
        .with_token_mode(TokenMode::Header);
    assert!(ep.connect().is_err());
    assert_eq!(server.seen().len(), before + 1);
}

#[test]
fn the_debug_text_and_the_description_hide_the_token() {
    let ep = WsEndpoint::new("ws://example.org:81/ws/x", Some("very-secret")).unwrap();
    assert!(!format!("{ep:?}").contains("very-secret"));
    assert_eq!(ep.describe(), "ws://example.org:81/ws/x");
    assert!(WsEndpoint::new("ws://example.org/ws/x?token=abc", None).is_err());
    assert!(WsEndpoint::new("ws://u:p@example.org/ws/x", None).is_err());
}

#[test]
fn a_dead_server_is_an_error_and_not_a_hang() {
    let mut server = TestServer::start();
    let url = server.url("doc");
    server.stop();
    let mut ep = WsEndpoint::new(&url, None).unwrap();
    let t = Instant::now();
    assert!(ep.connect().is_err());
    assert!(t.elapsed() < Duration::from_secs(6));
    // wss to a server that speaks no TLS fails with an error too.
    let live = TestServer::start();
    let wss = live.url("doc").replace("ws://", "wss://");
    let mut ep = WsEndpoint::new(&wss, None).unwrap();
    assert!(ep.connect().is_err());
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
    assert_eq!(a.link.client.state(), ConnState::Offline);
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
