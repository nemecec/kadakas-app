//! `collab.connect {url, token, doc}` over real WebSockets, against the in-process test server
//! (the reference sequencer behind sockets from `kadakas-collab-ws`).

#![cfg(not(target_arch = "wasm32"))]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kadakas_api::Session;
use kadakas_collab::docsync;
use kadakas_collab::testkit::Sequencer;
use kadakas_collab_ws::testserver::TestServer;
use kadakas_core::model::Document;
use kadakas_core::uid::Uid;
use serde_json::{Value, json};

const DOC: &str = "0190f1c2-7a3b-7c1d-8e2f-123456789abc";

fn call(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method} failed: {}", e.message))
}

fn fail(s: &mut Session, method: &str, p: Value) -> (String, String) {
    let e = s.handle(method, p).expect_err("the call should fail");
    (e.kind.to_string(), e.message)
}

fn server() -> TestServer {
    TestServer::start_with(Sequencer::new(docsync::replica_of(&Document::default())))
}

fn ws_session() -> Session {
    let mut s = Session::default();
    s.set_collab_endpoint_factory(Box::new(|url, token| {
        kadakas_collab_ws::endpoint_for(url, token).map_err(|e| e.to_string())
    }));
    s
}

fn user(n: u64) -> String {
    Uid::from_num(n).to_string()
}

fn add_rect(s: &mut Session, x: i64) -> String {
    let out = call(
        s,
        "execute",
        json!({"command": {"op": "add_object",
            "shape": {"kind": "rect", "min": [x, 0], "max": [x + 1000, 500]}}}),
    );
    out["created"][0].as_str().unwrap().to_string()
}

fn ids(s: &mut Session) -> Vec<String> {
    let v = call(s, "objects.list", json!({}));
    let mut ids: Vec<String> = v["objects"]
        .as_array()
        .or_else(|| v.as_array())
        .unwrap()
        .iter()
        .map(|o| o["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    ids
}

/// Syncs the sessions until `f` is true. Panics after 15 seconds.
fn settle(
    sessions: &mut [&mut Session],
    what: &str,
    mut f: impl FnMut(&mut [&mut Session]) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        for s in sessions.iter_mut() {
            call(s, "collab.sync", json!({}));
        }
        if f(sessions) {
            return;
        }
        assert!(Instant::now() < end, "timeout: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn state(s: &mut Session) -> String {
    call(s, "collab.status", json!({}))["state"]
        .as_str()
        .unwrap()
        .to_string()
}

fn connect(s: &mut Session, srv: &TestServer, n: u64, name: &str) {
    let st = call(
        s,
        "collab.connect",
        json!({"url": srv.base_url(), "doc": DOC, "user": user(n), "name": name}),
    );
    assert_eq!(st["reconnecting"], true);
    settle(&mut [s], "online", |ss| state(ss[0]) == "online");
}

#[test]
fn two_sessions_converge_over_websockets() {
    let srv = server();
    let (mut a, mut b) = (ws_session(), ws_session());
    connect(&mut a, &srv, 1, "Ann");
    connect(&mut b, &srv, 2, "Bo");
    let ra = add_rect(&mut a, 0);
    let rb = add_rect(&mut b, 5000);
    settle(&mut [&mut a, &mut b], "both objects everywhere", |ss| {
        ids(ss[0]).len() == 2 && ids(ss[0]) == ids(ss[1])
    });
    assert_eq!(ids(&mut a), {
        let mut v = vec![ra.clone(), rb.clone()];
        v.sort();
        v
    });
    // Undo takes back only your own edit.
    call(&mut b, "collab.undo", json!({}));
    settle(&mut [&mut a, &mut b], "undo seen by both", |ss| {
        ids(ss[0]) == vec![ra.clone()] && ids(ss[1]) == vec![ra.clone()]
    });
    // Presence reaches the other session.
    call(&mut a, "collab.presence", json!({"cursor": [10.0, 20.0]}));
    settle(&mut [&mut a, &mut b], "presence", |ss| {
        let p = call(ss[1], "collab.presence", json!({}));
        p["peers"].as_array().is_some_and(|l| {
            l.iter()
                .any(|x| x["name"] == "Ann" && x["cursor"] == json!([10.0, 20.0]))
        })
    });
    // The URL in the status has the document and no token.
    let st = call(&mut a, "collab.status", json!({}));
    assert_eq!(st["endpoint"], format!("{}/ws/{DOC}", srv.base_url()));
}

#[test]
fn the_token_stays_in_memory_and_out_of_every_answer() {
    let srv = server();
    let secret = "SECRET.token-0123456789";
    srv.require_token(secret, Uid::from_num(1));
    let mut a = ws_session();
    let mut all = Vec::new();
    all.push(call(
        &mut a,
        "collab.connect",
        json!({"url": format!("{}/ws/{DOC}", srv.base_url()), "token": secret, "user": user(1), "name": "Ann"}),
    ));
    settle(&mut [&mut a], "online", |ss| state(ss[0]) == "online");
    all.push(call(&mut a, "collab.status", json!({})));
    all.push(call(&mut a, "collab.sync", json!({})));
    all.push(call(&mut a, "collab.presence", json!({})));
    all.push(call(&mut a, "describe", json!({})));
    for v in &all {
        assert!(!v.to_string().contains(secret));
    }
    let seen = srv.seen();
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some(&format!("Bearer {secret}")[..])
    );

    // A wrong token: the error text has no token either.
    let mut b = ws_session();
    call(
        &mut b,
        "collab.connect",
        json!({"url": srv.base_url(), "doc": DOC, "token": "wrong-token-xyz", "user": user(2)}),
    );
    settle(&mut [&mut b], "failed", |ss| state(ss[0]) == "failed");
    let st = call(&mut b, "collab.status", json!({}));
    assert!(!st.to_string().contains("wrong-token-xyz"));
    assert!(st["last_error"].as_str().unwrap().contains("401"));
    let sync = call(&mut b, "collab.sync", json!({}));
    assert!(!sync.to_string().contains("wrong-token-xyz"));
}

#[test]
fn the_url_policy() {
    // A factory that only records which URLs got past the policy.
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let log = seen.clone();
    let mut s = Session::default();
    s.set_collab_endpoint_factory(Box::new(move |url, _| {
        log.lock().unwrap().push(url.to_string());
        Err("stop here".into())
    }));
    let try_url = |s: &mut Session, url: &str| {
        let e = s
            .handle("collab.connect", json!({"url": url, "doc": DOC}))
            .expect_err("the factory stops every try");
        (e.kind.to_string(), e.message)
    };
    // This computer is always allowed.
    for u in [
        "ws://127.0.0.1:1",
        "ws://localhost:1",
        "wss://[::1]:1",
        "ws://127.0.0.2:1/",
    ] {
        let (kind, msg) = try_url(&mut s, u);
        assert_eq!(msg, "stop here", "{u}: {kind}");
    }
    assert_eq!(seen.lock().unwrap().len(), 4);
    // Another server needs the list.
    let (kind, msg) = try_url(&mut s, "wss://collab.example.org");
    assert_eq!(kind, "policy");
    assert!(
        msg.contains("--allow-collab-url collab.example.org"),
        "{msg}"
    );
    s.set_collab_allowed_urls(vec!["*.example.org".into()]);
    assert_eq!(try_url(&mut s, "wss://collab.example.org").1, "stop here");
    // And it must be wss.
    let (kind, msg) = try_url(&mut s, "ws://collab.example.org");
    assert_eq!(kind, "policy");
    assert!(msg.contains("wss://"), "{msg}");
    // Secrets in the address are refused.
    for u in [
        "ws://u:p@127.0.0.1:1",
        "ws://127.0.0.1:1/?token=abc",
        "ftp://127.0.0.1",
    ] {
        assert_eq!(try_url(&mut s, u).0, "invalid_params", "{u}");
    }
    assert_eq!(seen.lock().unwrap().len(), 5);
    // The document is needed, and it must be the one in the address.
    let e = s
        .handle("collab.connect", json!({"url": "ws://127.0.0.1:1"}))
        .unwrap_err();
    assert_eq!(e.kind, "invalid_params");
    let e = s
        .handle(
            "collab.connect",
            json!({"url": format!("ws://127.0.0.1:1/ws/{DOC}"), "doc": user(9)}),
        )
        .unwrap_err();
    assert_eq!(e.kind, "invalid_params");
}

#[test]
fn a_session_without_websocket_support_says_so() {
    let mut s = Session::default();
    let (kind, msg) = fail(
        &mut s,
        "collab.connect",
        json!({"url": "ws://127.0.0.1:1", "doc": DOC}),
    );
    assert_eq!(kind, "policy");
    assert!(msg.contains("network"), "{msg}");
}

#[test]
fn defaults_from_the_host() {
    let srv = server();
    srv.require_token("tok-1", Uid::from_num(1));
    let mut a = ws_session();
    a.set_collab_defaults(Some(srv.base_url()), Some("tok-1".into()));
    // Only the document is left to give.
    call(
        &mut a,
        "collab.connect",
        json!({"doc": DOC, "user": user(1)}),
    );
    settle(&mut [&mut a], "online", |ss| state(ss[0]) == "online");
    assert_eq!(srv.seen()[0].authorization.as_deref(), Some("Bearer tok-1"));
}

#[test]
fn it_reconnects_by_itself_and_offline_edits_arrive() {
    let mut srv = server();
    let (mut a, mut b) = (ws_session(), ws_session());
    connect(&mut a, &srv, 1, "Ann");
    connect(&mut b, &srv, 2, "Bo");
    add_rect(&mut a, 0);
    settle(&mut [&mut a, &mut b], "first object", |ss| {
        ids(ss[1]).len() == 1
    });

    srv.stop();
    settle(&mut [&mut a], "not online", |ss| state(ss[0]) != "online");
    // Offline edit while the server is down.
    let offline = add_rect(&mut a, 3000);
    let st = call(&mut a, "collab.sync", json!({}))["status"].clone();
    assert_ne!(st["state"], "online");
    assert_eq!(st["reconnecting"], true);
    assert!(st["last_error"].is_string());
    srv.restart();
    settle(&mut [&mut a, &mut b], "offline edit reached Bo", |ss| {
        ids(ss[1]).contains(&offline) && ids(ss[0]) == ids(ss[1])
    });
    assert_eq!(state(&mut a), "online");
    assert!(call(&mut a, "collab.status", json!({}))["last_error"].is_null());

    // A disconnect on purpose stays disconnected.
    call(&mut a, "collab.disconnect", json!({}));
    let tries = srv.seen().len();
    for _ in 0..5 {
        call(&mut a, "collab.sync", json!({}));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(srv.seen().len(), tries, "no new tries after a disconnect");
    assert_eq!(state(&mut a), "offline");
    assert_eq!(
        call(&mut a, "collab.status", json!({}))["reconnecting"],
        false
    );
    // Connecting again keeps the user and sends the offline queue.
    let extra = add_rect(&mut a, 9000);
    call(&mut a, "collab.connect", json!({}));
    settle(
        &mut [&mut a, &mut b],
        "edit after disconnect arrived",
        |ss| ids(ss[1]).contains(&extra),
    );
}

#[test]
fn edits_stay_refused_between_the_tries_of_a_first_connection() {
    // The server is down while the first connection is made. The client is offline between
    // the tries, but the snapshot can still come and replace the document: an edit now would be
    // lost, so it is refused, and the status says why.
    let mut srv = server();
    srv.stop();
    let mut a = ws_session();
    call(
        &mut a,
        "collab.connect",
        json!({"url": srv.base_url(), "doc": DOC, "user": user(1), "name": "Ann"}),
    );
    let mut saw_offline = false;
    let end = Instant::now() + Duration::from_secs(15);
    while !saw_offline {
        let st = call(&mut a, "collab.sync", json!({}))["status"].clone();
        assert_eq!(st["waiting_for_snapshot"], true, "{st}");
        saw_offline = st["state"] == "offline" && st["last_error"].is_string();
        assert!(Instant::now() < end, "the first try never failed");
        std::thread::sleep(Duration::from_millis(10));
    }
    let e = a
        .handle(
            "execute",
            json!({"command": {"op": "add_object",
                "shape": {"kind": "rect", "min": [0, 0], "max": [10, 10]}}}),
        )
        .expect_err("refused");
    assert_eq!(e.kind, "conflict");
    assert_eq!(ids(&mut a).len(), 0);
    // When the server is back, the snapshot arrives and edits are allowed.
    srv.restart();
    settle(&mut [&mut a], "online", |ss| state(ss[0]) == "online");
    assert_eq!(
        call(&mut a, "collab.status", json!({}))["waiting_for_snapshot"],
        false
    );
    add_rect(&mut a, 0);
    // A disconnect on purpose ends the wait.
    call(&mut a, "collab.disconnect", json!({}));
    assert_eq!(
        call(&mut a, "collab.status", json!({}))["waiting_for_snapshot"],
        false
    );
}

// ----- which document, which token, what is refused -----

fn doc_id(n: u64) -> String {
    format!("0190f1c2-7a3b-7c1d-8e2f-{n:012x}")
}

fn rect_count(s: &mut Session) -> usize {
    ids(s).len()
}

#[test]
fn another_document_gets_a_new_client_and_the_old_one_is_never_copied_into_it() {
    let (a_srv, b_srv) = (server(), server());
    b_srv.with(|q| q.set_log_id("log-B"));
    let mut s = ws_session();
    call(
        &mut s,
        "collab.connect",
        json!({"url": a_srv.base_url(), "doc": doc_id(10), "user": user(1)}),
    );
    settle(&mut [&mut s], "online at A", |ss| state(ss[0]) == "online");
    add_rect(&mut s, 0);
    settle(&mut [&mut s], "A has the edit", |_| {
        a_srv.with(|q| q.seq()) == 1
    });

    // Edits that still wait for A are not carried to B: the call is refused.
    call(&mut s, "collab.disconnect", json!({}));
    add_rect(&mut s, 3000);
    let (kind, msg) = fail(
        &mut s,
        "collab.connect",
        json!({"url": b_srv.base_url(), "doc": doc_id(11)}),
    );
    assert_eq!(kind, "conflict", "{msg}");
    assert!(msg.contains("discard"), "{msg}");

    // With discard they are dropped, and B gets nothing of A.
    call(
        &mut s,
        "collab.connect",
        json!({"url": b_srv.base_url(), "doc": doc_id(11), "discard": true}),
    );
    settle(&mut [&mut s], "online at B", |ss| state(ss[0]) == "online");
    std::thread::sleep(Duration::from_millis(200));
    settle(&mut [&mut s], "quiet", |ss| state(ss[0]) == "online");
    assert_eq!(b_srv.with(|q| q.seq()), 0, "nothing of A reached B");
    assert_eq!(rect_count(&mut s), 0, "the document is the one of B");
    assert_eq!(a_srv.with(|q| q.seq()), 1, "A did not change");

    // The same address again is the same document: the client is kept.
    let client = call(&mut s, "collab.status", json!({}))["client"].clone();
    call(&mut s, "collab.disconnect", json!({}));
    call(
        &mut s,
        "collab.connect",
        json!({"url": b_srv.base_url(), "doc": doc_id(11)}),
    );
    assert_eq!(call(&mut s, "collab.status", json!({}))["client"], client);

    // The document in the same server (the path) counts too.
    call(&mut s, "collab.disconnect", json!({}));
    call(
        &mut s,
        "collab.connect",
        json!({"url": b_srv.base_url(), "doc": doc_id(12)}),
    );
    assert_ne!(call(&mut s, "collab.status", json!({}))["client"], client);
}

#[test]
fn a_store_of_another_document_is_refused() {
    let dir = std::env::temp_dir().join(format!("kadakas-collab-store-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let (a_srv, b_srv) = (server(), server());
    let store = dir.join("mine").to_string_lossy().to_string();
    let new = || {
        let mut s = ws_session();
        s.paths = kadakas_api::PathPolicy::restricted([dir.clone()]);
        s
    };
    let mut s = new();
    call(
        &mut s,
        "collab.connect",
        json!({"url": a_srv.base_url(), "doc": doc_id(1), "user": user(1), "store": store}),
    );
    settle(&mut [&mut s], "online", |ss| state(ss[0]) == "online");
    call(&mut s, "collab.disconnect", json!({}));
    drop(s);
    // A new program, the same store, another document.
    let mut t = new();
    let (kind, msg) = fail(
        &mut t,
        "collab.connect",
        json!({"url": b_srv.base_url(), "doc": doc_id(2), "store": store}),
    );
    assert_eq!(kind, "conflict");
    assert!(msg.contains("another document"), "{msg}");
    // The same document works.
    call(
        &mut t,
        "collab.connect",
        json!({"url": a_srv.base_url(), "doc": doc_id(1), "store": store}),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_default_token_goes_only_to_the_default_url() {
    let (home, other) = (server(), server());
    home.require_token("home-token", Uid::from_num(1));
    let mut s = ws_session();
    s.set_collab_defaults(Some(home.base_url()), Some("home-token".into()));
    // Another server on this computer is allowed, but gets no default token.
    let other_url = other.base_url();
    call(
        &mut s,
        "collab.connect",
        json!({"url": other_url, "doc": doc_id(1), "user": user(1)}),
    );
    settle(&mut [&mut s], "online at the other server", |ss| {
        state(ss[0]) == "online"
    });
    assert_eq!(other.seen()[0].authorization, None, "no default token");
    assert_eq!(home.seen().len(), 0);
    call(&mut s, "collab.disconnect", json!({}));
    // With its own token it gets that one.
    call(
        &mut s,
        "collab.connect",
        json!({"url": other.base_url(), "doc": doc_id(2), "token": "own", "user": user(1), "discard": true}),
    );
    settle(&mut [&mut s], "online again", |ss| state(ss[0]) == "online");
    assert_eq!(
        other.seen().last().unwrap().authorization.as_deref(),
        Some("Bearer own")
    );
    call(&mut s, "collab.disconnect", json!({}));
    // The default URL gets the default token.
    call(
        &mut s,
        "collab.connect",
        json!({"doc": doc_id(3), "user": user(1), "discard": true}),
    );
    settle(&mut [&mut s], "online at home", |ss| {
        state(ss[0]) == "online"
    });
    assert_eq!(
        home.seen()[0].authorization.as_deref(),
        Some("Bearer home-token")
    );
}

#[test]
fn a_remote_default_url_is_allowed_by_the_owner_but_must_use_wss() {
    let seen = Arc::new(Mutex::new(Vec::<(String, Option<String>)>::new()));
    let log = seen.clone();
    let mut s = Session::default();
    s.set_collab_endpoint_factory(Box::new(move |url, token| {
        log.lock()
            .unwrap()
            .push((url.to_string(), token.map(str::to_string)));
        Err("stop here".into())
    }));
    s.set_collab_defaults(Some("wss://collab.example.org".into()), Some("tok".into()));
    // The default URL needs no entry in the list, and it gets the default token.
    let e = s.handle("collab.connect", json!({"doc": DOC})).unwrap_err();
    assert_eq!(e.message, "stop here");
    assert_eq!(
        seen.lock().unwrap()[0],
        (
            format!("wss://collab.example.org/ws/{DOC}"),
            Some("tok".to_string())
        )
    );
    // Another host is refused, and the same host on another port is another origin.
    for u in ["wss://other.example.org", "wss://collab.example.org:444"] {
        let e = s
            .handle("collab.connect", json!({"url": u, "doc": DOC}))
            .unwrap_err();
        assert_eq!(e.kind, "policy", "{u}: {}", e.message);
    }
    assert_eq!(seen.lock().unwrap().len(), 1);
    // The map list of --allow-url is a different list: it does not help here (the session has
    // no such list at all, only the collaboration list).
    s.set_collab_allowed_urls(vec!["other.example.org".into()]);
    let e = s
        .handle(
            "collab.connect",
            json!({"url": "wss://other.example.org", "doc": DOC}),
        )
        .unwrap_err();
    assert_eq!(e.message, "stop here");
    assert_eq!(
        seen.lock().unwrap()[1].1,
        None,
        "no default token for another host"
    );
}

#[test]
fn a_new_token_for_the_same_address_rebuilds_the_endpoint() {
    let srv = server();
    srv.require_token("fresh", Uid::from_num(1));
    let mut s = ws_session();
    // The first token is wrong: the upgrade is refused and the client fails for good.
    call(
        &mut s,
        "collab.connect",
        json!({"url": srv.base_url(), "doc": DOC, "token": "old", "user": user(1)}),
    );
    settle(&mut [&mut s], "failed", |ss| state(ss[0]) == "failed");
    let tries = srv.seen().len();
    let st = call(&mut s, "collab.status", json!({}));
    assert!(st["last_error"].as_str().unwrap().contains("401"));
    assert_eq!(st["reconnecting"], false);
    for _ in 0..30 {
        call(&mut s, "collab.sync", json!({}));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        srv.seen().len(),
        tries,
        "a refused token is not tried again"
    );
    // The new token, with no address: the endpoint is made again for the same address.
    call(&mut s, "collab.connect", json!({"token": "fresh"}));
    settle(&mut [&mut s], "online", |ss| state(ss[0]) == "online");
    assert_eq!(
        srv.seen().last().unwrap().authorization.as_deref(),
        Some("Bearer fresh")
    );
    assert_eq!(
        call(&mut s, "collab.status", json!({}))["reconnecting"],
        true
    );
}

#[test]
fn an_edit_before_the_first_snapshot_is_taken_back_and_unsaved_work_is_not_replaced() {
    let srv = server();
    let mut s = ws_session();
    // The open document has an edit that nobody saved: connecting would lose it.
    add_rect(&mut s, 0);
    let (kind, msg) = fail(
        &mut s,
        "collab.connect",
        json!({"url": srv.base_url(), "doc": DOC, "user": user(1)}),
    );
    assert_eq!(kind, "conflict");
    assert!(msg.contains("replace: true"), "{msg}");
    call(
        &mut s,
        "collab.connect",
        json!({"url": srv.base_url(), "doc": DOC, "user": user(1), "replace": true}),
    );
    // Right after the call the client has no snapshot yet: an edit would be lost.
    let before = rect_count(&mut s);
    let e = s
        .handle(
            "execute",
            json!({"command": {"op": "add_object",
                "shape": {"kind": "rect", "min": [0, 0], "max": [10, 10]}}}),
        )
        .unwrap_err();
    assert_eq!(e.kind, "conflict");
    assert!(e.message.contains("online"), "{}", e.message);
    assert_eq!(rect_count(&mut s), before, "the edit was taken back");
    settle(&mut [&mut s], "online", |ss| state(ss[0]) == "online");
    add_rect(&mut s, 5);
}
