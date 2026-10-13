use serde_json::{Value, json};

use kadakas_core::change::Origin;

use super::*;
use crate::events::LOCAL;

fn ok(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

fn fail(s: &mut Session, method: &str, p: Value) -> RpcError {
    s.handle(method, p)
        .expect_err(&format!("{method} should fail"))
}

fn rect(s: &mut Session, x: i32) -> Value {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape":
            {"kind": "rect", "min": [x, 0], "max": [x + 10, 10]}}}),
    )["created"][0]
        .clone()
}

fn create(s: &mut Session, name: &str) -> u64 {
    ok(s, "version.create", json!({"name": name, "now_ms": 1000}))["version"]["id"]
        .as_u64()
        .unwrap()
}

fn object_count(s: &mut Session) -> usize {
    ok(s, "document.summary", json!({}))["object_count"]
        .as_u64()
        .unwrap() as usize
}

#[test]
fn a_version_has_its_data_and_one_undo_step() {
    let mut s = Session::with_sequential_ids();
    rect(&mut s, 0);
    let depth = s.editor.undo_depth();
    let r = s
        .handle_as(
            LOCAL,
            Origin::Agent,
            "version.create",
            json!({"name": "  First  ", "note": "for the client", "author": "Mari", "now_ms": 1234}),
        )
        .unwrap();
    let v = &r["version"];
    assert_eq!(v["id"], 1);
    assert_eq!(v["name"], "First");
    assert_eq!(v["note"], "for the client");
    assert_eq!(v["author"], "Mari");
    assert_eq!(v["time_ms"], 1234);
    assert_eq!(v["origin"], "agent");
    assert!(v["base_revision"].as_u64().unwrap() > 0);
    assert_eq!(r["count"], 1);
    assert_eq!(s.editor.undo_depth(), depth + 1);
    // Author and origin default from the session.
    let two = ok(&mut s, "version.create", json!({"name": "Second"}));
    assert_eq!(two["version"]["author"], "user");
    assert_eq!(two["version"]["origin"], "user");
    assert!(two["version"]["time_ms"].as_u64().unwrap() > 0 || cfg!(target_arch = "wasm32"));
    // Undo takes the version back, redo gives it again.
    assert!(s.editor.undo());
    assert_eq!(ok(&mut s, "version.list", json!({}))["count"], 1);
    assert!(s.editor.undo());
    assert_eq!(ok(&mut s, "version.list", json!({}))["count"], 0);
    assert!(s.editor.redo());
    assert_eq!(ok(&mut s, "version.list", json!({}))["count"], 1);
}

#[test]
fn the_list_and_get_describe_versions() {
    let mut s = Session::with_sequential_ids();
    assert_eq!(ok(&mut s, "version.list", json!({}))["count"], 0);
    assert!(ok(&mut s, "version.list", json!(null))["current_is_newest"].is_null());
    rect(&mut s, 0);
    let one = create(&mut s, "One");
    rect(&mut s, 20);
    let two = create(&mut s, "Two");
    let l = ok(&mut s, "version.list", json!({}));
    assert_eq!(l["count"], 2);
    assert_eq!(l["versions"][0]["name"], "One");
    assert_eq!(l["versions"][0]["delta_bytes"], 0);
    assert!(l["versions"][1]["delta_bytes"].as_u64().unwrap() > 0);
    assert_eq!(l["current_is_newest"], true);
    assert_eq!(l["limits"]["max_versions"], 100);
    assert!(l["bytes"].as_u64().unwrap() > 100);
    rect(&mut s, 40);
    assert_eq!(
        ok(&mut s, "version.list", json!({}))["current_is_newest"],
        false
    );
    let g = ok(&mut s, "version.get", json!({"id": one}));
    assert_eq!(g["version"]["name"], "One");
    assert_eq!(g["things"]["objects"], 1);
    assert!(g.get("document").is_none());
    let g = ok(&mut s, "version.get", json!({"id": two, "document": true}));
    assert_eq!(g["things"]["objects"], 2);
    assert_eq!(g["document"]["objects"].as_array().unwrap().len(), 2);
    assert!(g["document"].get("history").is_none());
    assert!(g["document"]["objects"][0].get("order").is_none());
    // It is a document that loads.
    let loaded = Document::from_value(g["document"].clone()).unwrap();
    assert_eq!(loaded.objects.len(), 2);
    assert_eq!(
        fail(&mut s, "version.get", json!({"id": 99})).kind,
        "not_found"
    );
    assert_eq!(fail(&mut s, "version.get", json!({})).code, -32602);
    assert_eq!(
        fail(&mut s, "version.get", json!({"id": one, "document": "yes"})).code,
        -32602
    );
}

#[test]
fn get_refuses_a_document_above_the_size_cap() {
    let mut s = Session::with_sequential_ids();
    let big = "x".repeat(GET_DOCUMENT_MAX_BYTES + 1000);
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "text", "at": [0, 0],
            "text": big, "height": 10}}}),
    );
    let id = create(&mut s, "Big");
    let e = fail(&mut s, "version.get", json!({"id": id, "document": true}));
    assert_eq!(e.kind, "limit");
    // The summary works, and the diff cuts the value.
    assert_eq!(
        ok(&mut s, "version.get", json!({"id": id}))["things"]["objects"],
        1
    );
    let d = ok(&mut s, "version.diff", json!({"from": id, "to": "current"}));
    assert_eq!(d["totals"]["added"], 0);
}

#[test]
fn restore_saves_the_work_and_is_one_undo_step() {
    let mut s = Session::with_sequential_ids();
    let a = rect(&mut s, 0);
    let one = create(&mut s, "One");
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [a], "delta": [5, 5]}}),
    );
    rect(&mut s, 30);
    let before = s.editor.doc().clone();
    let depth = s.editor.undo_depth();
    ok(&mut s, "events.subscribe", json!({}));
    let r = ok(&mut s, "version.restore", json!({"id": one, "now_ms": 5}));
    assert_eq!(r["restored"], one);
    assert_eq!(r["saved_version"], json!({"id": 2, "new": true}));
    assert_eq!(r["changed"], true);
    assert_eq!(object_count(&mut s), 1);
    assert_eq!(s.editor.undo_depth(), depth + 1);
    let l = ok(&mut s, "version.list", json!({}));
    assert_eq!(l["count"], 2);
    assert_eq!(l["versions"][1]["auto"], true);
    assert_eq!(l["versions"][1]["time_ms"], 5);
    assert!(l["versions"][1]["name"].as_str().unwrap().contains("One"));
    // The change event names the history and the objects.
    let notes = s.take_notifications(LOCAL);
    let ev = &notes.last().unwrap()["params"];
    assert_eq!(ev["op"], "restore");
    assert_eq!(ev["document"]["history"], true);
    assert_eq!(ev["removed"]["objects"].as_array().unwrap().len(), 1);
    // The work can be brought back from the automatic version.
    let back = ok(&mut s, "version.restore", json!({"id": 2}));
    assert_eq!(back["saved_version"]["new"], true);
    assert_eq!(object_count(&mut s), 2);
    // One undo takes the last restore away, and another the first, exactly.
    assert!(s.editor.undo());
    assert_eq!(object_count(&mut s), 1);
    assert!(s.editor.undo());
    assert_eq!(s.editor.doc(), &before);
    // Redo does it again.
    assert!(s.editor.redo());
    assert_eq!(object_count(&mut s), 1);
    // A document that equals the newest version needs no new version.
    let tip = create(&mut s, "Tip");
    let again = ok(&mut s, "version.restore", json!({"id": one}));
    assert_eq!(again["saved_version"], json!({"id": tip, "new": false}));
    assert_eq!(
        ok(&mut s, "version.list", json!({}))["versions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v["name"] == "Tip")
            .count(),
        1
    );
    assert_eq!(
        fail(&mut s, "version.restore", json!({"id": 99})).kind,
        "not_found"
    );
    assert_eq!(fail(&mut s, "version.restore", json!({})).code, -32602);
}

#[test]
fn restore_prunes_the_selection() {
    let mut s = Session::with_sequential_ids();
    let one = create(&mut s, "Empty");
    let a = rect(&mut s, 0);
    ok(&mut s, "selection.set", json!({"ids": [a]}));
    ok(&mut s, "version.restore", json!({"id": one}));
    assert!(s.selection.is_empty());
}

#[test]
fn delete_is_one_undo_step_and_keeps_the_other_versions() {
    let mut s = Session::with_sequential_ids();
    rect(&mut s, 0);
    let one = create(&mut s, "One");
    rect(&mut s, 20);
    let two = create(&mut s, "Two");
    rect(&mut s, 40);
    let three = create(&mut s, "Three");
    let depth = s.editor.undo_depth();
    let d = ok(&mut s, "version.delete", json!({"id": two}));
    assert_eq!(d, json!({"deleted": two, "count": 2}));
    assert_eq!(s.editor.undo_depth(), depth + 1);
    let g = ok(
        &mut s,
        "version.get",
        json!({"id": three, "document": true}),
    );
    assert_eq!(g["document"]["objects"].as_array().unwrap().len(), 3);
    assert_eq!(
        fail(&mut s, "version.get", json!({"id": two})).kind,
        "not_found"
    );
    assert_eq!(
        fail(&mut s, "version.delete", json!({"id": two})).kind,
        "not_found"
    );
    assert!(s.editor.undo());
    assert_eq!(ok(&mut s, "version.list", json!({}))["count"], 3);
    ok(&mut s, "version.delete", json!({"id": one}));
    ok(&mut s, "version.delete", json!({"id": two}));
    ok(&mut s, "version.delete", json!({"id": three}));
    assert!(s.editor.doc().history.is_none());
    assert_eq!(ok(&mut s, "version.list", json!({}))["count"], 0);
}

#[test]
fn diff_compares_versions_and_the_current_document() {
    let mut s = Session::with_sequential_ids();
    let a = rect(&mut s, 0);
    let b = rect(&mut s, 20);
    let one = create(&mut s, "One");
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [a], "delta": [5, 0]}}),
    );
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [b]}}),
    );
    let c = rect(&mut s, 50);
    let two = create(&mut s, "Two");
    let d = ok(&mut s, "version.diff", json!({"from": one, "to": two}));
    assert_eq!(d["from"], json!({"id": one, "name": "One"}));
    assert_eq!(d["to"], json!({"id": two, "name": "Two"}));
    assert_eq!(
        d["counts"]["objects"],
        json!({"added": 1, "removed": 1, "changed": 1})
    );
    assert_eq!(d["totals"], json!({"added": 1, "removed": 1, "changed": 1}));
    assert_eq!(d["truncated"], false);
    assert_eq!(d["exact"], true);
    assert_eq!(d["objects"]["added"][0]["id"], c);
    assert_eq!(d["objects"]["added"][0]["shape"], "rect");
    assert_eq!(d["objects"]["removed"][0]["id"], b);
    assert_eq!(d["objects"]["changed"][0]["id"], a);
    assert_eq!(d["objects"]["changed"][0]["properties"][0]["path"], "shape");
    // The current document, in both places.
    let same = ok(
        &mut s,
        "version.diff",
        json!({"from": two, "to": "current"}),
    );
    assert_eq!(
        same["totals"],
        json!({"added": 0, "removed": 0, "changed": 0})
    );
    assert_eq!(same["to"], json!({"current": true}));
    rect(&mut s, 90);
    let d = ok(
        &mut s,
        "version.diff",
        json!({"from": "Current", "to": one}),
    );
    assert_eq!(d["counts"]["objects"]["removed"], 2);
    // Kinds and limit.
    let d = ok(
        &mut s,
        "version.diff",
        json!({"from": one, "to": "current", "kinds": ["layers"]}),
    );
    assert!(d.get("objects").is_none() && d["counts"].get("objects").is_none());
    let d = ok(
        &mut s,
        "version.diff",
        json!({"from": one, "to": "current", "kinds": ["objects"], "limit": 1}),
    );
    assert_eq!(d["truncated"], true);
    assert_eq!(d["totals"]["added"], 2);
    let listed: usize = ["added", "removed", "changed"]
        .iter()
        .map(|k| d["objects"][k].as_array().unwrap().len())
        .sum();
    assert_eq!(listed, 1);
    // Bad parameters.
    for p in [
        json!({"from": one}),
        json!({"from": one, "to": "head"}),
        json!({"from": one, "to": -1}),
        json!({"from": one, "to": two, "kinds": ["nope"]}),
        json!({"from": one, "to": two, "kinds": "objects"}),
        json!({"from": one, "to": two, "limit": "ten"}),
    ] {
        assert_eq!(fail(&mut s, "version.diff", p).code, -32602);
    }
    assert_eq!(
        fail(&mut s, "version.diff", json!({"from": 99, "to": one})).kind,
        "not_found"
    );
    // Without any version, current against current is empty.
    let mut t = Session::with_sequential_ids();
    let d = ok(
        &mut t,
        "version.diff",
        json!({"from": "current", "to": "current"}),
    );
    assert_eq!(d["totals"]["added"], 0);
}

#[test]
fn bad_parameters_are_refused() {
    let mut s = Session::with_sequential_ids();
    assert_eq!(fail(&mut s, "version.create", json!({})).code, -32602);
    assert_eq!(
        fail(&mut s, "version.create", json!({"name": 5})).code,
        -32602
    );
    assert_eq!(fail(&mut s, "version.create", json!([1])).code, -32602);
    assert_eq!(
        fail(
            &mut s,
            "version.create",
            json!({"name": "a", "now_ms": "now"})
        )
        .code,
        -32602
    );
    assert_eq!(
        fail(&mut s, "version.create", json!({"name": "  "})).kind,
        "validation"
    );
    let long = "n".repeat(500);
    assert_eq!(
        fail(&mut s, "version.create", json!({"name": long})).kind,
        "validation"
    );
    assert_eq!(
        fail(
            &mut s,
            "version.create",
            json!({"name": "a", "note": null, "author": 4})
        )
        .code,
        -32602
    );
    assert!(s.editor.doc().history.is_none());
    assert_eq!(
        s.handle("version.nope", json!({})).unwrap_err().code,
        -32601
    );
}

#[test]
fn the_limits_of_the_session_cap_the_history() {
    let mut s = Session::with_sequential_ids();
    s.set_limits(SessionLimits {
        max_versions: 3,
        ..SessionLimits::default()
    });
    for i in 0..6 {
        rect(&mut s, i * 20);
        let r = ok(&mut s, "version.create", json!({"name": format!("v{i}")}));
        assert!(r["count"].as_u64().unwrap() <= 3);
        assert_eq!(r["dropped"].as_array().unwrap().len(), usize::from(i >= 3));
    }
    let l = ok(&mut s, "version.list", json!({}));
    let names: Vec<&str> = l["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["v3", "v4", "v5"]);
    // The kept versions keep their content.
    let g = ok(&mut s, "version.get", json!({"id": 4, "document": true}));
    assert_eq!(g["document"]["objects"].as_array().unwrap().len(), 4);
    // Off.
    s.set_limits(SessionLimits {
        max_versions: 0,
        ..SessionLimits::default()
    });
    assert_eq!(
        fail(&mut s, "version.create", json!({"name": "x"})).kind,
        "limit"
    );
    // Bytes.
    s.set_limits(SessionLimits {
        max_version_bytes: 800,
        ..SessionLimits::default()
    });
    assert_eq!(
        fail(&mut s, "version.create", json!({"name": "x"})).kind,
        "limit"
    );
    assert_eq!(SessionLimits::strict().max_versions, 20);
}

#[test]
fn history_counts_in_the_memory_of_the_session() {
    let mut s = Session::with_sequential_ids();
    for i in 0..10 {
        rect(&mut s, i * 20);
    }
    let before = s.estimated_bytes();
    create(&mut s, "One");
    assert!(s.estimated_bytes() > before);
    let h = s.editor.doc().history.clone().unwrap();
    assert!(document_bytes_of(&s) >= h.bytes() as u64);
}

fn document_bytes_of(s: &Session) -> u64 {
    s.estimated_bytes()
}

#[test]
fn document_get_leaves_the_history_out_and_a_saved_file_keeps_it() {
    let mut s = Session::with_sequential_ids();
    rect(&mut s, 0);
    create(&mut s, "One");
    assert!(
        ok(&mut s, "document.get", json!({}))
            .get("history")
            .is_none()
    );
    // A file has the history; a session that loads it has the versions.
    let file = serde_json::to_value(s.editor.doc()).unwrap();
    assert!(file["history"].is_object());
    let mut t = Session::with_sequential_ids();
    let r = ok(&mut t, "document.load", json!({"document": file}));
    assert!(r.get("notes").is_none());
    assert_eq!(ok(&mut t, "version.list", json!({}))["count"], 1);
    let g = ok(&mut t, "version.get", json!({"id": 1, "document": true}));
    assert_eq!(g["document"]["objects"].as_array().unwrap().len(), 1);
}

#[test]
fn a_hostile_history_in_a_file_is_repaired_or_dropped_with_notes() {
    let mut s = Session::with_sequential_ids();
    rect(&mut s, 0);
    create(&mut s, "One");
    rect(&mut s, 20);
    create(&mut s, "Two");
    let mut file = serde_json::to_value(s.editor.doc()).unwrap();
    // A delta that does not fit: the version is dropped.
    file["history"]["versions"][1]["delta"] = json!([{"op": "delete",
        "target": {"kind": "object", "id": "00000000-0000-0000-0000-0000000000ee"}}]);
    let mut t = Session::with_sequential_ids();
    let r = ok(&mut t, "document.load", json!({"document": file.clone()}));
    assert!(!r["notes"].as_array().unwrap().is_empty(), "{r}");
    assert_eq!(ok(&mut t, "version.list", json!({}))["count"], 1);
    // The repaired history is what the document keeps (and saves).
    assert_eq!(
        t.editor.doc().history.as_ref().unwrap().value()["versions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Nonsense in the field: dropped by the core with a note.
    file["history"] = json!("what");
    let r = ok(&mut t, "document.load", json!({"document": file.clone()}));
    assert_eq!(r["notes"].as_array().unwrap().len(), 1);
    assert_eq!(ok(&mut t, "version.list", json!({}))["count"], 0);
    // A history over the limits of the session is folded.
    let mut file = serde_json::to_value(s.editor.doc()).unwrap();
    file["history"]["next_id"] = json!(3);
    let mut u = Session::with_sequential_ids();
    u.set_limits(SessionLimits {
        max_versions: 1,
        ..SessionLimits::default()
    });
    let r = ok(&mut u, "document.load", json!({"document": file}));
    assert!(r["notes"][0].as_str().unwrap().contains("folded"));
    assert_eq!(ok(&mut u, "version.list", json!({}))["count"], 1);
    // The constructor for an opened file checks it too.
    let mut doc = Document::with_sequential_ids();
    doc.history = Some(HistoryData::new(json!({"format": 7})));
    let v = Session::with_document(doc);
    assert!(v.editor.doc().history.is_none());
}

#[test]
fn a_change_of_the_history_is_a_document_event() {
    let mut s = Session::with_sequential_ids();
    ok(&mut s, "events.subscribe", json!({"kinds": ["document"]}));
    rect(&mut s, 0);
    assert!(s.take_notifications(LOCAL).is_empty());
    create(&mut s, "One");
    let notes = s.take_notifications(LOCAL);
    assert_eq!(notes.len(), 1);
    let p = &notes[0]["params"];
    assert_eq!(p["op"], "set_history");
    assert_eq!(p["document"]["history"], true);
    ok(&mut s, "version.delete", json!({"id": 1}));
    assert_eq!(s.take_notifications(LOCAL).len(), 1);
    ok(&mut s, "undo", json!({}));
    assert_eq!(s.take_notifications(LOCAL).len(), 1);
}

#[test]
fn the_cache_follows_the_document() {
    let mut s = Session::with_sequential_ids();
    rect(&mut s, 0);
    create(&mut s, "One");
    // Undo and redo change the history without the cache knowing: the next read is right.
    assert!(s.editor.undo());
    assert_eq!(ok(&mut s, "version.list", json!({}))["count"], 0);
    assert!(s.editor.redo());
    assert_eq!(ok(&mut s, "version.list", json!({}))["count"], 1);
    // Same JSON in another value hits the cache.
    let data = s.editor.doc().history.clone().unwrap();
    assert!(s.review.get(&data).is_some());
    assert!(
        s.review
            .get(&HistoryData::new(json!({"format": 1})))
            .is_none()
    );
}

#[test]
fn describe_lists_the_methods_and_examples() {
    let mut s = Session::with_sequential_ids();
    let d = ok(&mut s, "describe", json!({}));
    let methods: Vec<&str> = d["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    for m in [
        "version.create",
        "version.list",
        "version.get",
        "version.restore",
        "version.delete",
        "version.diff",
    ] {
        assert!(methods.contains(&m), "{m}");
    }
    // Every example runs against a document that has a version.
    for e in d["review"]["examples"].as_array().unwrap() {
        let mut t = Session::with_sequential_ids();
        rect(&mut t, 0);
        create(&mut t, "A");
        rect(&mut t, 20);
        create(&mut t, "B");
        let r = t.handle(e["method"].as_str().unwrap(), e["params"].clone());
        match e["method"].as_str().unwrap() {
            "version.create" | "version.list" | "version.diff" | "version.get"
            | "version.restore" | "version.delete" => {
                assert!(r.is_ok(), "{e}: {:?}", r.err().map(|e| e.message))
            }
            other => panic!("unexpected example {other}"),
        }
    }
    assert!(d["review"]["local"].as_str().unwrap().contains("REV-09"));
}
