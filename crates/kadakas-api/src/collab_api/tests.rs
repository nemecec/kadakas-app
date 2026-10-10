use kadakas_collab::client::ClientEvent;
use kadakas_collab::protocol::ErrorCode;
use kadakas_collab::testkit::{Hub, Sequencer};
use kadakas_core::model::{Document, ObjectId};
use serde_json::{Value, json};

use super::*;

fn call(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method} failed: {}", e.message))
}

fn fail(s: &mut Session, method: &str, p: Value) -> RpcError {
    s.handle(method, p).expect_err("the call should fail")
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

fn has(s: &Session, id: &str) -> bool {
    let id = ObjectId(Uid::parse(id).unwrap());
    s.editor.doc().objects.contains_key(&id)
}

fn object_count(s: &mut Session) -> usize {
    s.editor.doc().objects.len()
}

/// Two sessions on one hub. The shared document starts as the document of the first session.
fn pair() -> (Hub, Session, Session) {
    let mut a = Session::default();
    let hub = Hub::new(Sequencer::new(docsync::replica_of(a.editor.doc())));
    a.set_collab_endpoint(Box::new(hub.endpoint()));
    let mut b = Session::default();
    b.set_collab_endpoint(Box::new(hub.endpoint()));
    for (s, name, t) in [(&mut a, "Ann", 1_000), (&mut b, "Bo", 1_000)] {
        let st = call(s, "collab.connect", json!({"name": name, "now_ms": t}));
        assert_eq!(st["state"], "online");
    }
    (hub, a, b)
}

#[test]
fn without_an_endpoint_nothing_connects() {
    let mut s = Session::default();
    assert_eq!(call(&mut s, "collab.status", json!(null))["state"], "none");
    let e = fail(&mut s, "collab.connect", json!({"now_ms": 5}));
    assert_eq!(e.kind, "policy");
    for m in [
        "collab.sync",
        "collab.presence",
        "collab.undo",
        "collab.redo",
    ] {
        assert_eq!(
            fail(&mut s, m, json!({"now_ms": 5})).kind,
            "conflict",
            "{m}"
        );
    }
    // Disconnect with nothing to disconnect is fine.
    assert_eq!(
        call(&mut s, "collab.disconnect", json!({}))["state"],
        "none"
    );
    assert!(s.collab_client().is_none());
    // Wrong parameters.
    assert_eq!(
        fail(&mut s, "collab.status", json!([1])).kind,
        "invalid_params"
    );
    assert_eq!(
        fail(&mut s, "collab.nope", json!({})).kind,
        "method_not_found"
    );
    let hub = Hub::new(Sequencer::new(docsync::replica_of(s.editor.doc())));
    s.set_collab_endpoint(Box::new(hub.endpoint()));
    assert_eq!(
        fail(&mut s, "collab.connect", json!({"user": "nope"})).kind,
        "invalid_params"
    );
    assert_eq!(
        fail(&mut s, "collab.connect", json!({"now_ms": "x"})).kind,
        "invalid_params"
    );
}

#[test]
fn edits_travel_between_sessions() {
    let (_hub, mut a, mut b) = pair();
    let status = call(&mut a, "collab.status", json!({}));
    assert_eq!(status["endpoint"], "in-memory test hub");
    assert_eq!(status["name"], "Ann");
    assert!(a.collab_client().is_some());
    let id = add_rect(&mut a, 0);
    let r = call(&mut a, "collab.sync", json!({"now_ms": 2_000}));
    assert!(r["pushed"].is_u64());
    assert_eq!(r["document_changed"], false);
    let r = call(&mut b, "collab.sync", json!({"now_ms": 2_000}));
    assert_eq!(r["document_changed"], true);
    assert_eq!(object_count(&mut b), 1);
    assert!(has(&b, &id));
    // Nothing new: nothing happens.
    let r = call(&mut b, "collab.sync", json!({"now_ms": 2_100}));
    assert_eq!(r["pushed"], Value::Null);
    assert_eq!(r["document_changed"], false);
    // B edits a layer property; A hears of it.
    call(
        &mut b,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [id], "delta": [100, 0]}}),
    );
    call(&mut b, "collab.sync", json!({"now_ms": 2_200}));
    let r = call(&mut a, "collab.sync", json!({"now_ms": 2_200}));
    assert_eq!(r["document_changed"], true);
    assert_eq!(a.editor.doc(), b.editor.doc());
    // A deletes it; B's later edit is lost (delete wins).
    call(
        &mut a,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [id]}}),
    );
    call(
        &mut b,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [id], "delta": [1, 1]}}),
    );
    call(&mut a, "collab.sync", json!({"now_ms": 2_300}));
    call(&mut b, "collab.sync", json!({"now_ms": 2_300}));
    call(&mut a, "collab.sync", json!({"now_ms": 2_400}));
    assert_eq!(object_count(&mut a), 0);
    assert_eq!(object_count(&mut b), 0);
    assert_eq!(a.editor.doc(), b.editor.doc());
}

#[test]
fn undo_is_per_user() {
    let (_hub, mut a, mut b) = pair();
    let id = add_rect(&mut a, 0);
    call(&mut a, "collab.sync", json!({"now_ms": 2_000}));
    call(&mut b, "collab.sync", json!({"now_ms": 2_000}));
    // A moves the rectangle, B moves it too, later. A's undo skips the shape.
    call(
        &mut a,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [id], "delta": [100, 0]}}),
    );
    call(&mut a, "collab.sync", json!({"now_ms": 2_100}));
    call(&mut b, "collab.sync", json!({"now_ms": 2_100}));
    call(
        &mut b,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [id], "delta": [0, 50]}}),
    );
    call(&mut b, "collab.sync", json!({"now_ms": 2_200}));
    call(&mut a, "collab.sync", json!({"now_ms": 2_200}));
    let before = a.editor.doc().clone();
    let r = call(&mut a, "collab.undo", json!({"now_ms": 2_300}));
    assert_eq!(r["done"], true);
    assert_eq!(r["applied"], 0);
    assert_eq!(r["skipped"][0]["reason"], "changed_by_other");
    assert_eq!(a.editor.doc(), &before);
    // Undo of the create, after B's edit: skipped too. Then nothing is left.
    let r = call(&mut a, "collab.undo", json!({"now_ms": 2_300}));
    assert_eq!(r["applied"], 0);
    assert_eq!(
        call(&mut a, "collab.undo", json!({"now_ms": 2_300}))["done"],
        false
    );
    // B undoes its own move, A hears of it, and B redoes.
    let r = call(&mut b, "collab.undo", json!({"now_ms": 2_400}));
    assert_eq!(r["applied"], 1);
    assert_eq!(r["document_changed"], true);
    call(&mut a, "collab.sync", json!({"now_ms": 2_500}));
    assert_eq!(a.editor.doc(), b.editor.doc());
    let r = call(&mut b, "collab.redo", json!({"now_ms": 2_600}));
    assert_eq!(r["applied"], 1);
    call(&mut a, "collab.sync", json!({"now_ms": 2_700}));
    assert_eq!(a.editor.doc(), b.editor.doc());
    assert_eq!(call(&mut a, "collab.status", json!({}))["can_undo"], false);
}

#[test]
fn presence_between_sessions() {
    let (hub, mut a, mut b) = pair();
    hub.set_time(5_000);
    let id = add_rect(&mut a, 0);
    let r = call(
        &mut a,
        "collab.presence",
        json!({"cursor": [10.5, 20.5], "selection": [id],
               "viewport": {"center": [0.0, 0.0], "scale": 2.0, "size": [800.0, 600.0]},
               "now_ms": 5_000}),
    );
    assert_eq!(r["own"]["cursor"], json!([10.5, 20.5]));
    assert_eq!(r["peers"].as_array().unwrap().len(), 1);
    let r = call(&mut b, "collab.presence", json!({"now_ms": 5_100}));
    assert_eq!(r["own"]["cursor"], Value::Null);
    let peer = &r["peers"][0];
    assert_eq!(peer["name"], "Ann");
    assert_eq!(peer["cursor"], json!([10.5, 20.5]));
    assert_eq!(peer["selection"][0], json!(id));
    assert_eq!(peer["viewport"]["scale"], 2.0);
    // It is not in the document, and clear works.
    hub.set_time(6_000);
    assert_eq!(object_count(&mut b), 0);
    call(
        &mut a,
        "collab.presence",
        json!({"clear": true, "now_ms": 6_000}),
    );
    let r = call(&mut b, "collab.presence", json!({"now_ms": 6_100}));
    assert_eq!(r["peers"][0]["cursor"], Value::Null);
    // Bad numbers are refused.
    let e = fail(
        &mut a,
        "collab.presence",
        json!({"viewport": {"center": [0.0, 0.0], "scale": -1.0, "size": [1.0, 1.0]}, "now_ms": 7_000}),
    );
    assert_eq!(e.kind, "validation");
    assert_eq!(
        fail(&mut a, "collab.presence", json!({"cursor": "x"})).kind,
        "invalid_params"
    );
}

#[test]
fn offline_work_goes_out_after_reconnect() {
    let (_hub, mut a, mut b) = pair();
    let st = call(&mut a, "collab.disconnect", json!({}));
    assert_eq!(st["state"], "offline");
    let id = add_rect(&mut a, 0);
    let r = call(&mut a, "collab.sync", json!({"now_ms": 3_000}));
    assert!(r["pushed"].is_u64());
    assert_eq!(r["status"]["pending"], 1);
    assert!(
        r["events"]
            .as_array()
            .unwrap()
            .contains(&json!({"event": "disconnected"}))
    );
    // Connecting twice is refused; connecting again after disconnect keeps the queue.
    let st = call(&mut a, "collab.connect", json!({"now_ms": 3_100}));
    assert_eq!(st["state"], "online");
    assert_eq!(st["pending"], 0);
    assert_eq!(
        fail(&mut a, "collab.connect", json!({"now_ms": 3_200})).kind,
        "conflict"
    );
    call(&mut b, "collab.sync", json!({"now_ms": 3_300}));
    assert!(has(&b, &id));
    // Another user ID starts a new client.
    call(&mut a, "collab.disconnect", json!({}));
    let other = "00000000-0000-7000-8000-000000000042";
    let st = call(
        &mut a,
        "collab.connect",
        json!({"user": other, "now_ms": 3_400}),
    );
    assert_eq!(st["user"], other);
}

#[test]
fn a_refused_edit_and_a_failed_sign_in_are_reported() {
    // Two sessions with predictable IDs make the same ID: the server refuses the second.
    let mut a = Session::with_sequential_ids();
    let hub = Hub::new(Sequencer::new(docsync::replica_of(a.editor.doc())));
    a.set_collab_endpoint(Box::new(hub.endpoint()));
    let mut b = Session::with_sequential_ids();
    b.set_collab_endpoint(Box::new(hub.endpoint()));
    call(&mut a, "collab.connect", json!({"now_ms": 1}));
    call(&mut b, "collab.connect", json!({"now_ms": 1}));
    add_rect(&mut a, 0);
    add_rect(&mut b, 5_000);
    call(&mut a, "collab.sync", json!({"now_ms": 2}));
    let r = call(&mut b, "collab.sync", json!({"now_ms": 2}));
    let events = r["events"].as_array().unwrap();
    assert!(
        events
            .iter()
            .any(|e| e["event"] == "rejected" && e["code"] == "duplicate_uid")
    );
    assert_eq!(r["document_changed"], true);
    // B now has A's object only.
    call(&mut a, "collab.sync", json!({"now_ms": 3}));
    assert_eq!(a.editor.doc(), b.editor.doc());

    // A server that wants a token.
    let mut c = Session::default();
    let mut seq = Sequencer::new(docsync::replica_of(c.editor.doc()));
    seq.require_token("secret");
    let hub = Hub::new(seq);
    c.set_collab_endpoint(Box::new(hub.endpoint()));
    let st = call(&mut c, "collab.connect", json!({"now_ms": 1}));
    assert_eq!(st["state"], "failed");
    let r = call(&mut c, "collab.sync", json!({"now_ms": 2}));
    assert!(
        r["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["event"] == "failed")
    );
    let st = call(
        &mut c,
        "collab.connect",
        json!({"token": "secret", "now_ms": 3}),
    );
    // The client was made with the first token. A new user ID starts a client with the token.
    assert_eq!(st["state"], "failed");
    let st = call(
        &mut c,
        "collab.connect",
        json!({"user": "00000000-0000-7000-8000-000000000007", "token": "secret", "now_ms": 4}),
    );
    assert_eq!(st["state"], "online");
}

#[test]
fn replacing_the_document_keeps_the_collaboration() {
    let (_hub, mut a, _b) = pair();
    a.replace_document(Document::default());
    assert_eq!(call(&mut a, "collab.status", json!({}))["state"], "online");
}

#[test]
fn describe_lists_the_methods() {
    let mut s = Session::default();
    let d = call(&mut s, "describe", json!({}));
    assert!(d["collaboration"]["model"].is_string());
    let names: Vec<&str> = d["methods"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["name"].as_str())
        .filter(|n| n.starts_with("collab."))
        .collect();
    assert_eq!(names.len(), 7);
}

#[test]
fn every_event_has_a_json_form() {
    let events = [
        ClientEvent::Connected,
        ClientEvent::Disconnected,
        ClientEvent::Rejected {
            op_id: 1,
            code: ErrorCode::InvalidTxn,
            message: "m".into(),
        },
        ClientEvent::ServerError {
            code: ErrorCode::RateLimited,
            message: "m".into(),
        },
        ClientEvent::Failed {
            code: ErrorCode::AuthFailed,
            message: "m".into(),
        },
        ClientEvent::GapDetected {
            expected: 1,
            got: 3,
        },
        ClientEvent::StoreFailed("full".into()),
    ];
    let names: Vec<String> = events
        .iter()
        .map(|e| event_json(e)["event"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        [
            "connected",
            "disconnected",
            "rejected",
            "server_error",
            "failed",
            "gap",
            "store_failed"
        ]
    );
}

#[test]
fn state_names() {
    assert_eq!(state_name(ConnState::Connecting), "connecting");
    assert_eq!(state_name(ConnState::Syncing), "syncing");
}

#[test]
fn a_document_that_cannot_be_built_is_reported() {
    // A shape that the file format refuses, written by a (hostile) client straight to the log.
    let (hub, mut a, _b) = pair();
    let mut ep = hub.endpoint();
    let mut link = Link::new(Client::new(ClientConfig {
        user: Uid::from_num(78),
        name: "x".into(),
        token: String::new(),
    }));
    link.connect(&mut ep).unwrap();
    for t in 0..4 {
        link.pump(t);
    }
    let bad = kadakas_collab::Op::Create {
        target: kadakas_collab::Target::new(kadakas_collab::Kind::Object, Uid::from_num(99)),
        props: [("shape".to_string(), json!({"kind": "rect", "min": "x"}))].into(),
    };
    link.client.edit(vec![bad]).unwrap();
    for t in 5..9 {
        link.pump(t);
    }
    let e = fail(&mut a, "collab.sync", json!({"now_ms": 9_000}));
    assert_eq!(e.kind, "validation");
}
