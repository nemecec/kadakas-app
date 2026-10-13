use kadakas_collab::client::ClientEvent;
use kadakas_collab::docsync;
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

fn sorted<T: Ord>(it: impl Iterator<Item = T>) -> Vec<T> {
    let mut v: Vec<T> = it.collect();
    v.sort();
    v
}

/// The same content, not looking at the order of the objects (the z-order is not shared).
fn same(a: &Session, b: &Session) -> bool {
    let (x, y) = (a.editor.doc(), b.editor.doc());
    // The stacking order is shared: the same objects in the same order, and the same layers in
    // the same order.
    x.objects.keys().eq(y.objects.keys())
        && x.layers
            .iter()
            .map(|l| l.id)
            .eq(y.layers.iter().map(|l| l.id))
        && x.objects.iter().all(|(k, v)| y.objects.get(k) == Some(v))
        && sorted(x.layers.iter().map(|l| (l.id.0, format!("{l:?}"))))
            == sorted(y.layers.iter().map(|l| (l.id.0, format!("{l:?}"))))
        && sorted(x.classes.iter().map(|c| (c.id.0, format!("{c:?}"))))
            == sorted(y.classes.iter().map(|c| (c.id.0, format!("{c:?}"))))
        && sorted(x.resources.iter().map(|r| (r.id.0, format!("{r:?}"))))
            == sorted(y.resources.iter().map(|r| (r.id.0, format!("{r:?}"))))
        && x.drawing == y.drawing
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
    assert_eq!(r["pushed"].as_array().unwrap().len(), 1);
    assert_eq!(r["document_changed"], false);
    let r = call(&mut b, "collab.sync", json!({"now_ms": 2_000}));
    assert_eq!(r["document_changed"], true);
    assert_eq!(object_count(&mut b), 1);
    assert!(has(&b, &id));
    // Nothing new: nothing happens.
    let r = call(&mut b, "collab.sync", json!({"now_ms": 2_100}));
    assert_eq!(r["pushed"], json!([]));
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
    assert!(same(&a, &b));
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
    assert!(same(&a, &b));
}

/// Versions belong to the file (REV-01): they are not shared, they stay when other users change
/// the document, and a restore travels to the others as an ordinary edit.
#[test]
fn versions_are_local_and_a_restore_is_an_ordinary_edit() {
    let (_hub, mut a, mut b) = pair();
    let first = add_rect(&mut a, 0);
    call(&mut a, "collab.sync", json!({"now_ms": 2_000}));
    call(&mut b, "collab.sync", json!({"now_ms": 2_000}));
    call(
        &mut a,
        "version.create",
        json!({"name": "Mine", "now_ms": 1}),
    );
    // B edits; A hears of it. A keeps its version, B has none.
    let second = add_rect(&mut b, 5_000);
    call(&mut b, "collab.sync", json!({"now_ms": 2_100}));
    call(&mut a, "collab.sync", json!({"now_ms": 2_100}));
    assert!(has(&a, &second));
    assert_eq!(call(&mut a, "version.list", json!({}))["count"], 1);
    assert_eq!(call(&mut b, "version.list", json!({}))["count"], 0);
    let d = call(&mut a, "version.diff", json!({"from": 1, "to": "current"}));
    assert_eq!(d["totals"]["added"], 1);
    // A restores. B sees the object go, and still has no version.
    call(&mut a, "version.restore", json!({"id": 1, "now_ms": 2}));
    assert!(!has(&a, &second) && has(&a, &first));
    call(&mut a, "collab.sync", json!({"now_ms": 2_200}));
    call(&mut b, "collab.sync", json!({"now_ms": 2_200}));
    assert!(!has(&b, &second) && has(&b, &first));
    assert_eq!(call(&mut b, "version.list", json!({}))["count"], 0);
    assert_eq!(call(&mut a, "version.list", json!({}))["count"], 2);
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
    assert!(same(&a, &b));
    let r = call(&mut b, "collab.redo", json!({"now_ms": 2_600}));
    assert_eq!(r["applied"], 1);
    call(&mut a, "collab.sync", json!({"now_ms": 2_700}));
    assert!(same(&a, &b));
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
    assert_eq!(object_count(&mut b), 1, "only the rectangle, no presence");
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
    assert_eq!(r["pushed"].as_array().unwrap().len(), 1);
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
    let e = fail(&mut a, "collab.connect", json!({"now_ms": 3_200}));
    assert_eq!(e.kind, "conflict");
    assert_eq!(e.reason, Some(crate::REASON_ALREADY_CONNECTED));
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
    // A session with predictable IDs cannot join: its IDs would meet the IDs of the others.
    let mut a = Session::with_sequential_ids();
    let hub = Hub::new(Sequencer::new(docsync::replica_of(a.editor.doc())));
    a.set_collab_endpoint(Box::new(hub.endpoint()));
    let e = fail(&mut a, "collab.connect", json!({"now_ms": 1}));
    assert_eq!(e.kind, "policy");
    assert!(e.message.contains("sequential"));

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
    // The same user keeps its client, and the new token goes in the next `hello`.
    assert_ne!(st["state"], "failed");
    call(&mut c, "collab.disconnect", json!({}));
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
fn a_thing_that_cannot_be_built_is_quarantined_and_reported() {
    // A hostile client writes a shape that the file format refuses, straight to the log of a
    // server that does not check it. The session shows everything else and says what is missing.
    let (hub, mut a, _b) = pair();
    hub.with(|s| s.check_entities(false));
    let mut ep = hub.endpoint();
    let mut link = Link::new(Client::new(ClientConfig {
        user: Uid::from_num(78),
        client: Uid::from_num(78),
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
        merge: false,
    };
    link.client.edit(vec![bad]).unwrap();
    for t in 5..9 {
        link.pump(t);
    }
    let r = call(&mut a, "collab.sync", json!({"now_ms": 9_000}));
    assert_eq!(r["quarantined"][0]["id"], json!(Uid::from_num(99)));
    assert_eq!(r["quarantined"][0]["kind"], "object");
    assert_eq!(object_count(&mut a), 0);
    assert_eq!(
        call(&mut a, "collab.status", json!({}))["quarantined"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn undo_and_load_are_refused_while_connected() {
    let (_hub, mut a, _b) = pair();
    add_rect(&mut a, 0);
    for m in ["undo", "redo"] {
        let e = fail(&mut a, m, json!(null));
        assert_eq!(e.kind, "conflict");
        assert!(e.message.contains("collab."));
    }
    let e = fail(&mut a, "document.load", json!({"document": {}}));
    assert_eq!(e.kind, "conflict");
    // After the disconnect they work again.
    call(&mut a, "collab.disconnect", json!({}));
    assert_eq!(call(&mut a, "undo", json!(null))["done"], true);
}

#[test]
fn a_remote_change_is_one_remote_change_set_and_keeps_the_session() {
    let (_hub, mut a, mut b) = pair();
    let x = add_rect(&mut a, 0);
    let y = add_rect(&mut a, 5_000);
    call(&mut a, "collab.sync", json!({"now_ms": 2_000}));
    call(&mut b, "collab.sync", json!({"now_ms": 2_000}));
    // B selects both and subscribes. A deletes one and adds another.
    call(&mut b, "selection.set", json!({"ids": [x, y]}));
    let rev = b.editor.revision();
    call(
        &mut a,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [x]}}),
    );
    let z = add_rect(&mut a, 9_000);
    call(&mut a, "collab.sync", json!({"now_ms": 2_100}));
    let r = call(&mut b, "collab.sync", json!({"now_ms": 2_100}));
    assert_eq!(r["document_changed"], true);
    // One change set, with the origin remote and exactly the things that changed.
    let polled = call(&mut b, "events.poll", json!({"since_revision": rev}));
    let changes = polled["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["origin"], "remote");
    assert_eq!(changes[0]["removed"]["objects"], json!([x]));
    assert_eq!(changes[0]["added"]["objects"], json!([z]));
    assert_eq!(changes[0]["changed"]["objects"], json!([]));
    // The selection stays, without the object that is gone.
    let sel = call(&mut b, "selection.get", json!(null));
    assert_eq!(sel["ids"], json!([y]));
    assert_eq!(a.editor.doc().objects.len(), b.editor.doc().objects.len());
}

#[test]
fn a_truncated_change_log_never_reverts_a_remote_change() {
    let (_hub, mut a, mut b) = pair();
    add_rect(&mut a, 0);
    call(&mut a, "collab.sync", json!({"now_ms": 2_000}));
    call(&mut b, "collab.sync", json!({"now_ms": 2_000}));
    // B adds an object; A hears of it. Then A's log is "truncated": the next push has to compare
    // the whole document.
    let from_b = add_rect(&mut b, 7_000);
    call(&mut b, "collab.sync", json!({"now_ms": 2_100}));
    call(&mut a, "collab.sync", json!({"now_ms": 2_100}));
    assert!(has(&a, &from_b));
    add_rect(&mut a, 20_000);
    a.collab.synced_rev = 1;
    call(&mut a, "collab.sync", json!({"now_ms": 2_200}));
    call(&mut b, "collab.sync", json!({"now_ms": 2_200}));
    assert!(has(&a, &from_b), "the object of B is still there");
    assert!(has(&b, &from_b));
    assert!(same(&a, &b));
    assert_eq!(object_count(&mut a), 3);
}

#[test]
fn presence_does_not_lose_an_edit_that_was_not_synced_yet() {
    let (_hub, mut a, mut b) = pair();
    // B makes an edit, A makes one and calls only presence. The edit of A must survive.
    add_rect(&mut b, 0);
    call(&mut b, "collab.sync", json!({"now_ms": 2_000}));
    let mine = add_rect(&mut a, 5_000);
    call(
        &mut a,
        "collab.presence",
        json!({"cursor": [1.0, 1.0], "now_ms": 2_100}),
    );
    assert!(has(&a, &mine));
    call(&mut a, "collab.sync", json!({"now_ms": 2_200}));
    call(&mut b, "collab.sync", json!({"now_ms": 2_200}));
    assert!(has(&b, &mine));
    assert_eq!(object_count(&mut b), 2);
    assert!(same(&a, &b));
}

#[test]
fn a_big_import_goes_out_in_several_transactions() {
    let (_hub, mut a, mut b) = pair();
    let cmds: Vec<Value> = (0..20_001)
        .map(|i| {
            json!({"op": "add_object", "shape": {"kind": "rect",
                "min": [i as f64 * 20.0, 0.0], "max": [i as f64 * 20.0 + 10.0, 5.0]}})
        })
        .collect();
    call(
        &mut a,
        "execute",
        json!({"command": {"op": "batch", "commands": cmds}}),
    );
    let r = call(&mut a, "collab.sync", json!({"now_ms": 2_000}));
    assert!(r["pushed"].as_array().unwrap().len() >= 5, "{r}");
    assert_eq!(r["problems"], json!([]));
    let r = call(&mut b, "collab.sync", json!({"now_ms": 2_000}));
    assert_eq!(r["document_changed"], true);
    assert_eq!(object_count(&mut b), 20_001);
    // The session goes on: a small edit afterwards is one transaction.
    add_rect(&mut a, -500);
    let r = call(&mut a, "collab.sync", json!({"now_ms": 2_100}));
    assert_eq!(r["pushed"].as_array().unwrap().len(), 1);
}

#[test]
fn the_offline_log_survives_a_restart_in_a_file_store() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("garden").to_string_lossy().to_string();
    let mut a = Session::default();
    let hub = Hub::new(Sequencer::new(docsync::replica_of(a.editor.doc())));
    a.set_collab_endpoint(Box::new(hub.endpoint()));
    let st = call(
        &mut a,
        "collab.connect",
        json!({"name": "Ann", "store": base, "now_ms": 1_000}),
    );
    let (user, client) = (st["user"].clone(), st["client"].clone());
    call(&mut a, "collab.disconnect", json!({}));
    let mine = add_rect(&mut a, 0);
    let r = call(&mut a, "collab.sync", json!({"now_ms": 2_000}));
    assert_eq!(r["status"]["pending"], 1);
    assert!(dir.path().join("garden.journal.json").exists());
    assert!(dir.path().join("garden.checkpoint.json").exists());
    // The program ends. A new session reads the files: same user, same writer, the edit waits.
    drop(a);
    let mut a2 = Session::default();
    a2.set_collab_endpoint(Box::new(hub.endpoint()));
    let st = call(
        &mut a2,
        "collab.connect",
        json!({"store": base, "now_ms": 3_000}),
    );
    assert_eq!(st["user"], user);
    assert_eq!(st["client"], client);
    assert_eq!(st["state"], "online");
    assert_eq!(st["pending"], 0, "the edit went out");
    assert!(has(&a2, &mine));
    assert_eq!(
        hub.with(|s| s.replica().alive_of(kadakas_collab::Kind::Object).count()),
        1
    );
}

#[test]
fn damaged_store_files_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("x").to_string_lossy().to_string();
    std::fs::write(dir.path().join("x.journal.json"), "{broken").unwrap();
    let mut a = Session::default();
    let hub = Hub::new(Sequencer::new(docsync::replica_of(a.editor.doc())));
    a.set_collab_endpoint(Box::new(hub.endpoint()));
    call(
        &mut a,
        "collab.connect",
        json!({"store": base, "now_ms": 1}),
    );
    let r = call(&mut a, "collab.sync", json!({"now_ms": 2}));
    assert!(
        r["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["event"] == "store_corrupt")
    );
    // The path policy applies to the store.
    a.paths = crate::PathPolicy::default();
}

#[test]
fn waiting_edits_of_one_user_are_not_dropped_for_another() {
    let (_hub, mut a, _b) = pair();
    call(&mut a, "collab.disconnect", json!({}));
    add_rect(&mut a, 0);
    call(&mut a, "collab.sync", json!({"now_ms": 2_000}));
    let e = fail(
        &mut a,
        "collab.connect",
        json!({"user": "00000000-0000-7000-8000-000000000042", "now_ms": 3_000}),
    );
    assert_eq!(e.kind, "conflict");
    assert_eq!(e.reason, Some(crate::REASON_EDITS_WAIT));
    assert!(e.message.contains("discard"));
    let st = call(
        &mut a,
        "collab.connect",
        json!({"user": "00000000-0000-7000-8000-000000000042", "discard": true, "now_ms": 3_000}),
    );
    assert_eq!(st["state"], "online");
    assert_eq!(st["pending"], 0);
}

#[test]
fn plug_in_objects_and_their_regeneration_flow_through_collab() {
    use kadakas_core::change::Origin;
    let (_hub, mut a, mut b) = pair();
    a.scripting.allow_agent_scripts = true;
    let planter = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../kadakas-plugin/tests/fixtures/raised_planter.wasm"
    );
    call(&mut a, "plugins.install", json!({"path": planter}));
    let rev_a = a.editor.revision();
    let placed = call(
        &mut a,
        "plugins.place",
        json!({"plugin": "org.kadakas.raised-planter", "min": [0, 0], "max": [2000, 800]}),
    );
    // The plug-in change set has the origin script on the session that ran the module.
    let sets = a.editor.changes_since(rev_a);
    assert_eq!(sets.changes[0].origin, Origin::Script);
    let id = placed["id"].as_str().unwrap().to_string();
    call(&mut a, "collab.sync", json!({"now_ms": 2_000}));
    let rev_b = b.editor.revision();
    call(&mut b, "collab.sync", json!({"now_ms": 2_000}));
    // The other session gets the group and its children as one remote change set.
    let sets = b.editor.changes_since(rev_b);
    assert_eq!(sets.changes.len(), 1);
    assert_eq!(sets.changes[0].origin, Origin::Remote);
    assert!(has(&b, &id));
    assert!(same(&a, &b));
    // A regeneration (new parameters) is one more change set; it reaches B the same way.
    call(
        &mut a,
        "plugins.set_params",
        json!({"id": id, "params": {"wall": 150.0}}),
    );
    call(&mut a, "collab.sync", json!({"now_ms": 2_100}));
    call(&mut b, "collab.sync", json!({"now_ms": 2_100}));
    assert!(same(&a, &b));
    assert_eq!(a.editor.doc().objects.len(), b.editor.doc().objects.len());
}

#[test]
fn two_users_who_regenerate_one_plug_in_object_at_once_do_not_double_it() {
    let (_hub, mut a, mut b) = pair();
    let planter = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../kadakas-plugin/tests/fixtures/raised_planter.wasm"
    );
    for s in [&mut a, &mut b] {
        s.scripting.allow_agent_scripts = true;
        call(s, "plugins.install", json!({"path": planter}));
    }
    let placed = call(
        &mut a,
        "plugins.place",
        json!({"plugin": "org.kadakas.raised-planter", "min": [0, 0], "max": [2000, 800]}),
    );
    let id = placed["id"].as_str().unwrap().to_string();
    let count_after_place = a.editor.doc().objects.len();
    for t in 0..2 {
        call(&mut a, "collab.sync", json!({"now_ms": 2_000 + t}));
        call(&mut b, "collab.sync", json!({"now_ms": 2_000 + t}));
    }
    assert!(same(&a, &b));
    let sync_all = |a: &mut Session, b: &mut Session, from: u64| {
        for t in 0..4 {
            call(a, "collab.sync", json!({"now_ms": from + t}));
            call(b, "collab.sync", json!({"now_ms": from + t}));
        }
    };

    // Both regenerate with the same parameters at the same time: the same IDs, one set of shapes.
    for s in [&mut a, &mut b] {
        call(
            s,
            "plugins.set_params",
            json!({"id": id, "params": {"wall": 150.0}}),
        );
    }
    sync_all(&mut a, &mut b, 3_000);
    assert!(same(&a, &b));
    assert_eq!(a.editor.doc().objects.len(), count_after_place);
    assert_eq!(b.editor.doc().objects.len(), count_after_place);

    // Different parameters at the same time: the last writer wins on the parameters, and the
    // shapes belong to the same regeneration. Nothing doubles.
    call(
        &mut a,
        "plugins.set_params",
        json!({"id": id, "params": {"wall": 120.0}}),
    );
    call(
        &mut b,
        "plugins.set_params",
        json!({"id": id, "params": {"wall": 180.0}}),
    );
    sync_all(&mut a, &mut b, 4_000);
    assert!(same(&a, &b));
    assert_eq!(a.editor.doc().objects.len(), count_after_place);
    let oid = ObjectId(Uid::parse(&id).unwrap());
    let field = |s: &mut Session, f: &str| -> Value {
        let o = s.editor.doc().object(oid).unwrap();
        let data = o.data.values().find(|v| v.get("plugin").is_some()).unwrap();
        if f == "wall" {
            data["params"]["wall"].clone()
        } else {
            data[f].clone()
        }
    };
    let wall = |s: &mut Session| field(s, "wall");
    assert_eq!(wall(&mut a), wall(&mut b));
    assert_eq!(
        field(&mut a, "generation"),
        json!(3),
        "both made generation 3 from 2"
    );
    assert_eq!(field(&mut b, "generation"), json!(3));
    // A further regeneration from either side goes on from the winning generation.
    call(
        &mut a,
        "plugins.set_params",
        json!({"id": id, "params": {"wall": 90.0}}),
    );
    sync_all(&mut a, &mut b, 5_000);
    assert!(same(&a, &b));
    assert_eq!(b.editor.doc().objects.len(), count_after_place);
    assert_eq!(wall(&mut b), json!(90.0));
}

/// A PNG of noise, so that it does not shrink: well above the size where a picture becomes a blob.
fn noisy_png_base64(side: u32, variant: u32) -> String {
    let mut seed = 0x1234_5678u32 ^ variant;
    let mut data = Vec::new();
    for _ in 0..side * side * 4 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        data.push((seed >> 24) as u8);
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, side, side);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&data).unwrap();
    }
    kadakas_core::underlay::base64_encode(&out)
}

#[test]
fn a_picture_is_sent_once_as_a_blob_and_not_with_every_edit() {
    let (hub, mut a, mut b) = pair();
    let png = noisy_png_base64(64, 0);
    assert!(png.len() > kadakas_collab::blob::MIN_BLOB_BYTES);
    let out = call(
        &mut a,
        "execute",
        json!({"command": {"op": "add_object", "shape": {
            "kind": "underlay", "min": [0, 0], "max": [6400, 6400],
            "width": 64, "height": 64, "png": png, "opacity": 1.0}}}),
    );
    let id = out["created"][0].as_str().unwrap().to_string();
    call(&mut a, "collab.sync", json!({"now_ms": 2_000}));
    // The log holds a reference, not the picture.
    let entry_size = hub.with(|s| {
        serde_json::to_string(s.log().last().unwrap())
            .unwrap()
            .len()
    });
    assert!(entry_size < 3_000, "{entry_size} bytes");
    assert!(hub.with(|s| serde_json::to_string(s.replica()).unwrap().len()) < 6_000);
    // B asks for the blob, gets it, and builds the same document.
    let r = call(&mut b, "collab.sync", json!({"now_ms": 2_000}));
    assert_eq!(r["document_changed"], true, "{r}");
    assert!(has(&b, &id));
    assert!(same(&a, &b));
    // A change of the opacity sends a small operation.
    call(
        &mut a,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [id], "delta": [100, 0]}}),
    );
    call(&mut a, "collab.sync", json!({"now_ms": 2_100}));
    let entry_size = hub.with(|s| {
        serde_json::to_string(s.log().last().unwrap())
            .unwrap()
            .len()
    });
    assert!(entry_size < 3_000, "a move sends {entry_size} bytes");
    call(&mut b, "collab.sync", json!({"now_ms": 2_100}));
    assert!(same(&a, &b));
    assert!(
        call(&mut b, "collab.status", json!({}))["quarantined"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_new_regeneration_after_an_undo_does_not_delete_the_shapes() {
    let (_hub, mut a, mut b) = pair();
    let planter = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../kadakas-plugin/tests/fixtures/raised_planter.wasm"
    );
    for s in [&mut a, &mut b] {
        s.scripting.allow_agent_scripts = true;
        call(s, "plugins.install", json!({"path": planter}));
    }
    let placed = call(
        &mut a,
        "plugins.place",
        json!({"plugin": "org.kadakas.raised-planter", "min": [0, 0], "max": [2000, 800]}),
    );
    let id = placed["id"].as_str().unwrap().to_string();
    let count = a.editor.doc().objects.len();
    let sync_all = |a: &mut Session, b: &mut Session, from: u64| {
        for t in 0..4 {
            call(a, "collab.sync", json!({"now_ms": from + t}));
            call(b, "collab.sync", json!({"now_ms": from + t}));
        }
    };
    sync_all(&mut a, &mut b, 2_000);
    // place, set_params, undo, set_params
    call(
        &mut a,
        "plugins.set_params",
        json!({"id": id, "params": {"wall": 150.0}}),
    );
    sync_all(&mut a, &mut b, 3_000);
    let r = call(&mut a, "collab.undo", json!({"now_ms": 3_100}));
    assert_eq!(r["done"], true);
    sync_all(&mut a, &mut b, 3_200);
    assert!(same(&a, &b));
    assert_eq!(a.editor.doc().objects.len(), count);
    call(
        &mut a,
        "plugins.set_params",
        json!({"id": id, "params": {"wall": 90.0}}),
    );
    sync_all(&mut a, &mut b, 4_000);
    // The shapes are there on both sessions, and the same.
    assert!(same(&a, &b));
    assert_eq!(a.editor.doc().objects.len(), count, "no shape was lost");
    assert_eq!(b.editor.doc().objects.len(), count);
    // The same from the other session, too.
    call(
        &mut b,
        "plugins.set_params",
        json!({"id": id, "params": {"wall": 110.0}}),
    );
    sync_all(&mut a, &mut b, 5_000);
    assert!(same(&a, &b));
    assert_eq!(b.editor.doc().objects.len(), count);
}

// ----- real commands on two sessions -----

mod real_commands {
    use super::*;
    use proptest::prelude::*;

    #[derive(Clone, Debug)]
    enum Act {
        AddRect { s: bool, x: i32 },
        AddLayer { s: bool, n: u8 },
        Delete { s: bool, i: u8 },
        Move { s: bool, i: u8 },
        Group { s: bool, i: u8, j: u8 },
        Ungroup { s: bool, i: u8 },
        ToLayer { s: bool, i: u8, l: u8 },
        DeleteLayer { s: bool, l: u8 },
        RenameLayer { s: bool, l: u8, n: u8 },
        AddResource { s: bool, n: u8 },
        Data { s: bool, i: u8, v: u8 },
        Arrange { s: bool, i: u8, j: u8, how: u8 },
        MoveLayer { s: bool, l: u8, at: u8 },
        Undo { s: bool },
        Redo { s: bool },
        Place { s: bool, x: i32 },
        SetParams { s: bool, i: u8, w: u16 },
        Underlay { s: bool, x: i32, v: u8 },
        AddSite { s: bool, n: u16 },
        EditSite { s: bool, i: u8, k: u16 },
        Sync { s: bool },
        Offline { s: bool },
        Online { s: bool },
    }

    fn act() -> impl Strategy<Value = Act> {
        prop_oneof![
            4 => (any::<bool>(), -500i32..500).prop_map(|(s, x)| Act::AddRect { s, x }),
            1 => (any::<bool>(), 0u8..4).prop_map(|(s, n)| Act::AddLayer { s, n }),
            2 => (any::<bool>(), any::<u8>()).prop_map(|(s, i)| Act::Delete { s, i }),
            2 => (any::<bool>(), any::<u8>()).prop_map(|(s, i)| Act::Move { s, i }),
            2 => (any::<bool>(), any::<u8>(), any::<u8>()).prop_map(|(s, i, j)| Act::Group { s, i, j }),
            1 => (any::<bool>(), any::<u8>()).prop_map(|(s, i)| Act::Ungroup { s, i }),
            1 => (any::<bool>(), any::<u8>(), any::<u8>()).prop_map(|(s, i, l)| Act::ToLayer { s, i, l }),
            1 => (any::<bool>(), any::<u8>()).prop_map(|(s, l)| Act::DeleteLayer { s, l }),
            1 => (any::<bool>(), any::<u8>(), 0u8..3).prop_map(|(s, l, n)| Act::RenameLayer { s, l, n }),
            1 => (any::<bool>(), 0u8..2).prop_map(|(s, n)| Act::AddResource { s, n }),
            1 => (any::<bool>(), any::<u8>(), 0u8..3).prop_map(|(s, i, v)| Act::Data { s, i, v }),
            3 => (any::<bool>(), any::<u8>(), any::<u8>(), 0u8..4)
                .prop_map(|(s, i, j, how)| Act::Arrange { s, i, j, how }),
            1 => (any::<bool>(), any::<u8>(), 0u8..4).prop_map(|(s, l, at)| Act::MoveLayer { s, l, at }),
            2 => any::<bool>().prop_map(|s| Act::Undo { s }),
            1 => any::<bool>().prop_map(|s| Act::Redo { s }),
            1 => (any::<bool>(), -500i32..500).prop_map(|(s, x)| Act::Place { s, x }),
            2 => (any::<bool>(), any::<u8>(), 50u16..200).prop_map(|(s, i, w)| Act::SetParams { s, i, w }),
            1 => (any::<bool>(), -500i32..500, 0u8..2).prop_map(|(s, x, v)| Act::Underlay { s, x, v }),
            1 => (any::<bool>(), 1u16..300).prop_map(|(s, n)| Act::AddSite { s, n }),
            1 => (any::<bool>(), any::<u8>(), 0u16..300).prop_map(|(s, i, k)| Act::EditSite { s, i, k }),
            5 => any::<bool>().prop_map(|s| Act::Sync { s }),
            1 => any::<bool>().prop_map(|s| Act::Offline { s }),
            2 => any::<bool>().prop_map(|s| Act::Online { s }),
        ]
    }

    /// A session whose IDs come from a seed, so the same actions make the same IDs and a failing
    /// case replays. The two sessions of a test use two salts, so their IDs never meet.
    pub(super) fn seeded(salt: u64) -> Session {
        let mut doc = kadakas_core::model::Document::default();
        doc.seed_ids(1_700_000_000_000, salt);
        let mut s = Session::with_document(doc);
        s.scripting.allow_agent_scripts = true;
        // Instrumented builds run slowly. The wall-clock limit of a plug-in call must not decide
        // the result of a test; the fuel is the exact limit.
        s.wasm_plugins.registry.set_limits(kadakas_plugin::Limits {
            max_millis: 60_000,
            ..kadakas_plugin::Limits::default()
        });
        s
    }

    pub(super) const PLANTER: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../kadakas-plugin/tests/fixtures/raised_planter.wasm"
    );

    fn object_ids(s: &Session) -> Vec<String> {
        s.editor
            .doc()
            .objects
            .keys()
            .map(|k| k.0.to_string())
            .collect()
    }

    fn layer_ids(s: &Session) -> Vec<String> {
        s.editor
            .doc()
            .layers
            .iter()
            .map(|l| l.id.0.to_string())
            .collect()
    }

    fn pick(v: &[String], i: u8) -> Option<String> {
        (!v.is_empty()).then(|| v[i as usize % v.len()].clone())
    }

    fn exec(s: &mut Session, cmd: Value) {
        // A command may fail (an ID is gone, a layer is not empty): this is normal.
        let _ = s.handle("execute", json!({"command": cmd}));
    }

    fn apply(sessions: &mut [Session; 2], hub: &Hub, now: &mut u64, a: &Act) {
        *now += 100;
        hub.set_time(*now);
        let idx = |s: bool| usize::from(s);
        match a {
            Act::AddRect { s, x } => exec(
                &mut sessions[idx(*s)],
                json!({"op": "add_object", "shape": {"kind": "rect",
                    "min": [x, 0], "max": [x + 100, 50]}}),
            ),
            Act::AddLayer { s, n } => exec(
                &mut sessions[idx(*s)],
                json!({"op": "add_layer", "name": format!("L{n}")}),
            ),
            Act::Delete { s, i } => {
                let ss = &mut sessions[idx(*s)];
                if let Some(id) = pick(&object_ids(ss), *i) {
                    exec(ss, json!({"op": "delete_objects", "ids": [id]}));
                }
            }
            Act::Move { s, i } => {
                let ss = &mut sessions[idx(*s)];
                if let Some(id) = pick(&object_ids(ss), *i) {
                    exec(
                        ss,
                        json!({"op": "move_objects", "ids": [id], "delta": [10, 5]}),
                    );
                }
            }
            Act::Group { s, i, j } => {
                let ss = &mut sessions[idx(*s)];
                let ids = object_ids(ss);
                if let (Some(p), Some(q)) = (pick(&ids, *i), pick(&ids, *j)) {
                    exec(ss, json!({"op": "group", "ids": [p, q]}));
                }
            }
            Act::Ungroup { s, i } => {
                let ss = &mut sessions[idx(*s)];
                if let Some(id) = pick(&object_ids(ss), *i) {
                    exec(ss, json!({"op": "ungroup", "ids": [id]}));
                }
            }
            Act::ToLayer { s, i, l } => {
                let ss = &mut sessions[idx(*s)];
                if let (Some(id), Some(layer)) =
                    (pick(&object_ids(ss), *i), pick(&layer_ids(ss), *l))
                {
                    exec(ss, json!({"op": "modify_object", "id": id, "layer": layer}));
                }
            }
            Act::DeleteLayer { s, l } => {
                let ss = &mut sessions[idx(*s)];
                if let Some(layer) = pick(&layer_ids(ss), *l) {
                    exec(ss, json!({"op": "delete_layer", "id": layer}));
                }
            }
            Act::RenameLayer { s, l, n } => {
                let ss = &mut sessions[idx(*s)];
                if let Some(layer) = pick(&layer_ids(ss), *l) {
                    exec(
                        ss,
                        json!({"op": "modify_layer", "id": layer, "name": format!("R{n}")}),
                    );
                }
            }
            Act::AddResource { s, n } => exec(
                &mut sessions[idx(*s)],
                json!({"op": "add_resource", "name": format!("W{n}"), "body": {"type": "worksheet"}}),
            ),
            Act::Data { s, i, v } => {
                let ss = &mut sessions[idx(*s)];
                if let Some(id) = pick(&object_ids(ss), *i) {
                    exec(
                        ss,
                        json!({"op": "set_data", "id": id, "set": {format!("k{v}"): v}}),
                    );
                }
            }
            Act::Arrange { s, i, j, how } => {
                let ss = &mut sessions[idx(*s)];
                let ids = object_ids(ss);
                if let (Some(p), Some(q)) = (pick(&ids, *i), pick(&ids, *j)) {
                    let how = ["front", "back", "forward", "backward"][usize::from(*how)];
                    exec(ss, json!({"op": "arrange", "ids": [p, q], "how": how}));
                }
            }
            Act::MoveLayer { s, l, at } => {
                let ss = &mut sessions[idx(*s)];
                if let Some(layer) = pick(&layer_ids(ss), *l) {
                    exec(ss, json!({"op": "move_layer", "id": layer, "index": at}));
                }
            }
            Act::Undo { s } => {
                let _ = sessions[idx(*s)].handle("collab.undo", json!({"now_ms": *now}));
            }
            Act::Redo { s } => {
                let _ = sessions[idx(*s)].handle("collab.redo", json!({"now_ms": *now}));
            }
            Act::Place { s, x } => {
                let _ = sessions[idx(*s)].handle(
                    "plugins.place",
                    json!({"plugin": "org.kadakas.raised-planter", "min": [x, 0], "max": [x + 2000, 800]}),
                );
            }
            Act::SetParams { s, i, w } => {
                let ss = &mut sessions[idx(*s)];
                let planters: Vec<String> = ss
                    .editor
                    .doc()
                    .objects
                    .values()
                    .filter(|o| o.data.values().any(|v| v.get("plugin").is_some()))
                    .map(|o| o.id.0.to_string())
                    .collect();
                if let Some(id) = pick(&planters, *i) {
                    let _ = ss.handle(
                        "plugins.set_params",
                        json!({"id": id, "params": {"wall": f64::from(*w)}}),
                    );
                }
            }
            Act::Underlay { s, x, v } => exec(
                &mut sessions[idx(*s)],
                json!({"op": "add_object", "shape": {"kind": "underlay",
                    "min": [x, 0], "max": [x + 6400, 6400], "width": 64, "height": 64,
                    "png": noisy_png_base64(64, u32::from(*v)), "opacity": 1.0}}),
            ),
            Act::AddSite { s, n } => {
                let points: Vec<Value> = (0..*n)
                    .map(|i| json!([f64::from(i) * 10.0, f64::from(i % 9) * 5.0, 0.0]))
                    .collect();
                exec(
                    &mut sessions[idx(*s)],
                    json!({"op": "add_object", "shape": {"kind": "site_model", "points": points}}),
                );
            }
            Act::EditSite { s, i, k } => {
                let ss = &mut sessions[idx(*s)];
                let sites: Vec<(String, Vec<Value>)> = ss
                    .editor
                    .doc()
                    .objects
                    .values()
                    .filter_map(|o| match &o.shape {
                        kadakas_core::model::Shape::SiteModel { points, .. } => Some((
                            o.id.0.to_string(),
                            points.iter().map(|p| json!([p.x, p.y, p.z])).collect(),
                        )),
                        _ => None,
                    })
                    .collect();
                if !sites.is_empty() {
                    let (id, mut points) = sites[usize::from(*i) % sites.len()].clone();
                    if !points.is_empty() {
                        let at = usize::from(*k) % points.len();
                        points[at] = json!([1.0, 1.0, 77.0]);
                        exec(
                            ss,
                            json!({"op": "modify_object", "id": id,
                                   "shape": {"kind": "site_model", "points": points}}),
                        );
                    }
                }
            }
            Act::Sync { s } => {
                let _ = sessions[idx(*s)].handle("collab.sync", json!({"now_ms": *now}));
            }
            Act::Offline { s } => {
                let _ = sessions[idx(*s)].handle("collab.disconnect", json!({}));
            }
            Act::Online { s } => {
                let _ = sessions[idx(*s)].handle("collab.connect", json!({"now_ms": *now}));
            }
        }
    }

    /// The IDs of the plug-in objects of a session.
    fn planters(s: &Session) -> Vec<String> {
        s.editor
            .doc()
            .objects
            .values()
            .filter(|o| o.data.values().any(|v| v.get("plugin").is_some()))
            .map(|o| o.id.0.to_string())
            .collect()
    }

    fn settle(sessions: &mut [Session; 2], hub: &Hub, now: &mut u64) {
        for _ in 0..8 {
            for s in sessions.iter_mut() {
                *now += 100;
                hub.set_time(*now);
                s.handle("collab.sync", json!({"now_ms": *now})).unwrap();
            }
        }
    }

    /// A plug-in that stops with an error (no fuel) on one side, while the other side changes
    /// the same objects, undoes and groups: nothing may get lost or split.
    #[test]
    fn a_plugin_error_during_concurrent_edits_still_converges() {
        let a = seeded(11);
        let hub = Hub::new(Sequencer::new(docsync::replica_of(a.editor.doc())));
        let mut sessions = [a, seeded(12)];
        for s in &mut sessions {
            s.handle("plugins.install", json!({"path": PLANTER}))
                .unwrap();
            s.set_collab_endpoint(Box::new(hub.endpoint()));
            s.handle("collab.connect", json!({"now_ms": 1})).unwrap();
        }
        let mut now = 1_000;
        for x in [0, 5_000] {
            sessions[0]
                .handle(
                    "plugins.place",
                    json!({"plugin": "org.kadakas.raised-planter", "min": [x, 0], "max": [x + 2000, 800]}),
                )
                .unwrap();
        }
        settle(&mut sessions, &hub, &mut now);
        let ids = planters(&sessions[1]);
        assert_eq!(ids.len(), 2, "both planters reached B");
        // B has no fuel: every plug-in call of B is an error. The error leaves the old shapes.
        // B gets a plug-in with almost no fuel: the module cannot even describe itself, so B
        // has no working plug-in. Every plug-in call of B is an error, and the shapes stay.
        {
            let b = &mut sessions[1];
            b.wasm_plugins.registry.remove("org.kadakas.raised-planter");
            b.wasm_plugins.registry.set_limits(kadakas_plugin::Limits {
                fuel: 1_000,
                max_millis: 60_000,
                ..kadakas_plugin::Limits::default()
            });
            assert!(
                b.handle("plugins.install", json!({"path": PLANTER}))
                    .is_err()
            );
        }
        now += 100;
        hub.set_time(now);
        // Both sides work at the same time, offline from each other until the end.
        let failed = sessions[1].handle(
            "plugins.set_params",
            json!({"id": ids[0], "params": {"wall": 120.0}}),
        );
        assert!(failed.is_err(), "B must see the plug-in error");
        sessions[0]
            .handle(
                "plugins.set_params",
                json!({"id": ids[0], "params": {"wall": 90.0}}),
            )
            .unwrap();
        exec(&mut sessions[0], json!({"op": "group", "ids": ids.clone()}));
        let _ = sessions[1].handle("collab.undo", json!({"now_ms": now}));
        let _ = sessions[1].handle(
            "plugins.set_params",
            json!({"id": ids[1], "params": {"wall": 200.0}}),
        );
        let _ = sessions[0].handle("collab.undo", json!({"now_ms": now}));
        exec(
            &mut sessions[1],
            json!({"op": "move_objects", "ids": [ids[1]], "delta": [10, 5]}),
        );
        settle(&mut sessions, &hub, &mut now);
        for s in &sessions {
            assert_eq!(s.collab_client().unwrap().pending().len(), 0);
            let mut doc = s.editor.doc().clone();
            assert!(doc.validate().is_ok(), "{:?}", doc.validate());
        }
        assert!(same(&sessions[0], &sessions[1]));
        let server = hub.with(|s| s.replica().canonical_text());
        for s in &sessions {
            let c = s.collab_client().unwrap();
            assert_eq!(c.confirmed().canonical_text(), server);
            assert_eq!(c.view().canonical_text(), server);
        }
    }

    /// Two users add an object to an empty list at the same time: both get the key `a0` and the
    /// list is sorted by ID. The next local insert of one user must go after both, not between
    /// keys that the shadow renumbered (a shrunk proptest case).
    #[test]
    fn an_insert_after_a_tie_of_keys_keeps_the_shared_order() {
        let acts = [
            Act::AddRect { s: false, x: 0 },
            Act::Place { s: false, x: 0 },
            Act::Underlay {
                s: true,
                x: 0,
                v: 0,
            },
            Act::Underlay {
                s: false,
                x: 0,
                v: 0,
            },
            Act::Undo { s: false },
            Act::Sync { s: true },
            Act::Arrange {
                s: false,
                i: 0,
                j: 197,
                how: 2,
            },
            Act::AddRect { s: false, x: 314 },
            Act::Delete { s: true, i: 191 },
            Act::Redo { s: false },
            Act::Sync { s: true },
            Act::Underlay {
                s: true,
                x: -148,
                v: 1,
            },
        ];
        let a = seeded(1);
        let hub = Hub::new(Sequencer::new(docsync::replica_of(a.editor.doc())));
        let mut sessions = [a, seeded(2)];
        for s in &mut sessions {
            s.handle("plugins.install", json!({"path": PLANTER}))
                .unwrap();
            s.set_collab_endpoint(Box::new(hub.endpoint()));
            s.handle("collab.connect", json!({"now_ms": 1})).unwrap();
        }
        let mut now = 1_000;
        for a in &acts {
            apply(&mut sessions, &hub, &mut now, a);
        }
        settle(&mut sessions, &hub, &mut now);
        assert!(same(&sessions[0], &sessions[1]));
        assert_eq!(
            object_count(&mut sessions[0]),
            object_count(&mut sessions[1])
        );
    }

    /// Two objects of two users tie on their key. One user then puts a new object between them: the
    /// tied run gets new keys, so the order is the same on both sides.
    #[test]
    fn an_object_between_two_tied_keys_converges() {
        let a = seeded(1);
        let hub = Hub::new(Sequencer::new(docsync::replica_of(a.editor.doc())));
        let mut sessions = [a, seeded(2)];
        for s in &mut sessions {
            s.set_collab_endpoint(Box::new(hub.endpoint()));
            s.handle("collab.connect", json!({"now_ms": 1})).unwrap();
        }
        let mut now = 1_000;
        // Both add at the same time (no sync between): the same key.
        apply(
            &mut sessions,
            &hub,
            &mut now,
            &Act::AddRect { s: false, x: 0 },
        );
        apply(
            &mut sessions,
            &hub,
            &mut now,
            &Act::AddRect { s: true, x: 500 },
        );
        settle(&mut sessions, &hub, &mut now);
        assert!(same(&sessions[0], &sessions[1]));
        // Both sides try a new object one step behind the top, between the tied pair.
        for s in [false, true] {
            let ss = &mut sessions[usize::from(s)];
            let before = object_ids(ss);
            apply(&mut sessions, &hub, &mut now, &Act::AddRect { s, x: 900 });
            let ss = &mut sessions[usize::from(s)];
            let new: Vec<String> = object_ids(ss)
                .into_iter()
                .filter(|i| !before.contains(i))
                .collect();
            let placed = ss.handle(
                "execute",
                json!({"command": {"op": "arrange", "ids": new, "how": "backward"}}),
            );
            assert!(placed.is_ok());
            settle(&mut sessions, &hub, &mut now);
            assert!(same(&sessions[0], &sessions[1]));
        }
        assert_eq!(object_count(&mut sessions[0]), 4);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(30))]

        #[test]
        fn two_sessions_with_real_commands_converge(acts in prop::collection::vec(act(), 1..45)) {
            let a = seeded(1);
            let hub = Hub::new(Sequencer::new(docsync::replica_of(a.editor.doc())));
            let mut sessions = [a, seeded(2)];
            for s in &mut sessions {
                let _ = s.handle("plugins.install", json!({"path": PLANTER}));
                s.set_collab_endpoint(Box::new(hub.endpoint()));
                let r = s.handle("collab.connect", json!({"now_ms": 1}));
                prop_assert!(r.is_ok());
            }
            let mut now = 1_000;
            for a in &acts {
                apply(&mut sessions, &hub, &mut now, a);
            }
            // Everybody comes back and the network settles.
            for s in &mut sessions {
                let _ = s.handle("collab.connect", json!({"now_ms": now}));
            }
            for _ in 0..8 {
                for s in &mut sessions {
                    now += 100;
                    hub.set_time(now);
                    let r = s.handle("collab.sync", json!({"now_ms": now}));
                    prop_assert!(r.is_ok());
                }
            }
            for s in &mut sessions {
                prop_assert_eq!(s.collab_client().unwrap().pending().len(), 0);
                // The document is valid for the core.
                let mut doc = s.editor.doc().clone();
                prop_assert!(doc.validate().is_ok(), "{:?}", doc.validate());
            }
            prop_assert!(same(&sessions[0], &sessions[1]));
            // Both match the server. Every acked transaction is in the log: the server state
            // equals what each client confirmed.
            let server = hub.with(|s| s.replica().canonical_text());
            for s in &sessions {
                let c = s.collab_client().unwrap();
                prop_assert_eq!(c.confirmed().canonical_text(), server.clone());
                prop_assert_eq!(c.view().canonical_text(), server.clone());
            }
        }
    }
}

#[test]
fn the_stacking_order_is_shared() {
    let (_hub, mut a, mut b) = pair();
    let ids: Vec<String> = (0..4).map(|i| add_rect(&mut a, i * 2_000)).collect();
    call(&mut a, "collab.sync", json!({"now_ms": 2_000}));
    call(&mut b, "collab.sync", json!({"now_ms": 2_000}));
    let order = |s: &Session| -> Vec<String> {
        s.editor
            .doc()
            .objects
            .keys()
            .map(|k| k.0.to_string())
            .collect()
    };
    assert_eq!(order(&a), ids);
    assert_eq!(order(&b), ids);
    // A sends the first object to the front. B, at the same time, adds a new one and sends the
    // last to the back. Both changes are kept, and the order is the same everywhere.
    call(
        &mut a,
        "execute",
        json!({"command": {"op": "arrange", "ids": [ids[0]], "how": "front"}}),
    );
    let new = add_rect(&mut b, 50_000);
    call(
        &mut b,
        "execute",
        json!({"command": {"op": "arrange", "ids": [ids[3]], "how": "back"}}),
    );
    for t in 0..3 {
        call(&mut a, "collab.sync", json!({"now_ms": 2_100 + t}));
        call(&mut b, "collab.sync", json!({"now_ms": 2_100 + t}));
    }
    assert_eq!(order(&a), order(&b));
    let o = order(&a);
    assert_eq!(o[0], ids[3], "sent to the back");
    let at = |id: &String| o.iter().position(|x| x == id).unwrap();
    assert!(at(&ids[0]) > at(&ids[2]), "sent to the front");
    assert!(at(&new) > at(&ids[2]));
    // Layers: move one, the other session sees the new order.
    call(
        &mut a,
        "execute",
        json!({"command": {"op": "add_layer", "name": "Two"}}),
    );
    call(&mut a, "collab.sync", json!({"now_ms": 3_000}));
    call(&mut b, "collab.sync", json!({"now_ms": 3_000}));
    let second = a.editor.doc().layers[1].id.0.to_string();
    call(
        &mut a,
        "execute",
        json!({"command": {"op": "move_layer", "id": second, "index": 0}}),
    );
    call(&mut a, "collab.sync", json!({"now_ms": 3_100}));
    call(&mut b, "collab.sync", json!({"now_ms": 3_100}));
    assert_eq!(b.editor.doc().layers[0].id.0.to_string(), second);
    assert!(same(&a, &b));
}

#[test]
fn a_join_with_unsaved_work_says_why_and_the_status_tells_when_to_wait() {
    let mut a = Session::default();
    let hub = Hub::new(Sequencer::new(docsync::replica_of(&Document::default())));
    a.set_collab_endpoint(Box::new(hub.endpoint()));
    add_rect(&mut a, 0);
    let e = fail(&mut a, "collab.connect", json!({"now_ms": 1_000}));
    assert_eq!(e.kind, "conflict");
    assert_eq!(e.reason, Some(crate::REASON_UNSAVED_CHANGES));
    // No other error has a reason.
    assert_eq!(fail(&mut a, "collab.nope", json!({})).reason, None);
    let st = call(
        &mut a,
        "collab.connect",
        json!({"replace": true, "now_ms": 1_000}),
    );
    assert_eq!(st["state"], "online");
    assert_eq!(st["waiting_for_snapshot"], false);
}
