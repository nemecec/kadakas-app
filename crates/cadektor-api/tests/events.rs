//! The change-event stream over the API (PLAT-02): subscribe, notifications, poll, ordering,
//! batches, origins, and the reset after a document is replaced.

use cadektor_api::Session;
use cadektor_api::events::LOCAL;
use cadektor_core::change::Origin;
use serde_json::{Value, json};

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    s.handle(method, params)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

fn add_rect(s: &mut Session) -> Value {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [10, 10]}}}),
    )["created"][0]
        .clone()
}

#[test]
fn notifications_follow_changes_in_order_and_arrive_once() {
    let mut s = Session::with_sequential_ids();
    let sub = ok(&mut s, "events.subscribe", json!({}));
    assert!(sub["revision"].as_u64().is_some());
    assert!(s.take_notifications(LOCAL).is_empty());
    let a = add_rect(&mut s);
    let b = add_rect(&mut s);
    ok(&mut s, "undo", json!({}));
    let notes = s.take_notifications(LOCAL);
    assert_eq!(notes.len(), 3);
    for n in &notes {
        assert_eq!(n["method"], "events.change");
        assert_eq!(n["jsonrpc"], "2.0");
        assert!(n.get("id").is_none());
    }
    assert_eq!(notes[0]["params"]["added"]["objects"], json!([a]));
    assert_eq!(notes[1]["params"]["added"]["objects"], json!([b]));
    assert_eq!(notes[2]["params"]["op"], "undo");
    assert_eq!(notes[2]["params"]["removed"]["objects"], json!([b]));
    let revs: Vec<u64> = notes
        .iter()
        .map(|n| n["params"]["revision"].as_u64().unwrap())
        .collect();
    assert!(revs.windows(2).all(|w| w[0] < w[1]));
    assert!(s.take_notifications(LOCAL).is_empty());
}

#[test]
fn a_batch_is_one_notification_and_a_failed_batch_is_none() {
    let mut s = Session::with_sequential_ids();
    ok(&mut s, "events.subscribe", json!({}));
    let rect =
        json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [10, 10]}});
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "batch", "commands": [rect, rect, rect]}}),
    );
    let notes = s.take_notifications(LOCAL);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0]["params"]["op"], "batch");
    assert_eq!(
        notes[0]["params"]["added"]["objects"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let bad = json!({"op": "delete_objects", "ids": ["00000000-0000-0000-0000-0000000000ff"]});
    let failed = s.handle(
        "execute",
        json!({"command": {"op": "batch", "commands": [rect, bad]}}),
    );
    assert!(failed.is_err());
    assert!(s.take_notifications(LOCAL).is_empty());
}

#[test]
fn kinds_filter_and_unsubscribe() {
    let mut s = Session::with_sequential_ids();
    ok(&mut s, "events.subscribe", json!({"kinds": ["layers"]}));
    add_rect(&mut s);
    assert!(s.take_notifications(LOCAL).is_empty());
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_layer", "name": "Planting", "elevation": 0}}),
    );
    let notes = s.take_notifications(LOCAL);
    assert_eq!(notes.len(), 1);
    assert_eq!(
        notes[0]["params"]["added"]["layers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(notes[0]["params"]["added"]["objects"], json!([]));
    assert!(
        s.handle("events.subscribe", json!({"kinds": ["bogus"]}))
            .is_err()
    );
    assert_eq!(ok(&mut s, "events.unsubscribe", json!({}))["done"], true);
    add_rect(&mut s);
    assert!(s.take_notifications(LOCAL).is_empty());
    assert_eq!(ok(&mut s, "events.unsubscribe", json!({}))["done"], false);
}

#[test]
fn connections_have_their_own_cursor_and_origin() {
    let mut s = Session::with_sequential_ids();
    s.handle_as(1, Origin::Agent, "events.subscribe", json!({}))
        .unwrap();
    s.handle_as(
        2,
        Origin::Agent,
        "events.subscribe",
        json!({"kinds": ["objects"]}),
    )
    .unwrap();
    s.handle_as(
        1,
        Origin::Agent,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [1, 1]}}}),
    )
    .unwrap();
    // A change by the app itself has the origin user.
    s.editor
        .execute(&cadektor_core::command::Command::AddLayer {
            name: "L".into(),
            elevation: 0.0,
            scale: None,
            kind: None,
        })
        .unwrap();
    let one = s.take_notifications(1);
    let two = s.take_notifications(2);
    assert_eq!(one.len(), 2);
    assert_eq!(one[0]["params"]["origin"], "agent");
    assert_eq!(one[1]["params"]["origin"], "user");
    assert_eq!(two.len(), 1);
    assert!(s.take_notifications(1).is_empty());
    assert_eq!(s.subscribers(), vec![1, 2]);
    s.drop_connection(1);
    assert!(!s.is_subscribed(1));
}

#[test]
fn poll_gives_changes_after_a_revision() {
    let mut s = Session::with_sequential_ids();
    let start = ok(&mut s, "events.poll", json!({"since_revision": 0}))["revision"]
        .as_u64()
        .unwrap();
    let a = add_rect(&mut s);
    add_rect(&mut s);
    let r = ok(&mut s, "events.poll", json!({"since_revision": start}));
    assert_eq!(r["truncated"], false);
    assert_eq!(r["changes"].as_array().unwrap().len(), 2);
    assert_eq!(r["changes"][0]["added"]["objects"], json!([a]));
    let latest = r["revision"].as_u64().unwrap();
    let r = ok(&mut s, "events.poll", json!({"since_revision": latest}));
    assert!(r["changes"].as_array().unwrap().is_empty());
    assert!(s.handle("events.poll", json!({})).is_err());
}

#[test]
fn replacing_the_document_resets_subscribers() {
    let mut s = Session::with_sequential_ids();
    ok(&mut s, "events.subscribe", json!({}));
    add_rect(&mut s);
    let doc = ok(&mut s, "document.get", json!({}));
    ok(&mut s, "document.load", json!({"document": doc}));
    let notes = s.take_notifications(LOCAL);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0]["method"], "events.reset");
    assert!(notes[0]["params"]["revision"].as_u64().is_some());
    // The subscription stays, and new changes arrive.
    add_rect(&mut s);
    assert_eq!(s.take_notifications(LOCAL).len(), 1);
}

#[test]
fn handle_line_marks_the_agent_and_two_sessions_do_not_mix() {
    let mut a = Session::with_sequential_ids();
    let mut b = Session::with_sequential_ids();
    ok(&mut a, "events.subscribe", json!({}));
    ok(&mut b, "events.subscribe", json!({}));
    let line = json!({"jsonrpc": "2.0", "id": 1, "method": "execute", "params": {"command": {"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [1, 1]}}}});
    a.handle_line(&line.to_string()).unwrap();
    let from_a = a.take_notifications(LOCAL);
    assert_eq!(from_a.len(), 1);
    assert_eq!(from_a[0]["params"]["origin"], "agent");
    assert!(b.take_notifications(LOCAL).is_empty());
}
