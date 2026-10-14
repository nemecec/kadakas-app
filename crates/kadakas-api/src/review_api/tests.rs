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

/// What a save writes, read as a file: a session that opened it.
fn through_file(s: &mut Session) -> (Value, Session) {
    let file = serde_json::to_value(&*s.document_for_save().unwrap()).unwrap();
    let doc = Document::from_json(&file.to_string()).unwrap();
    (file, Session::with_document(doc))
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
    assert!(r.get("dropped").is_none(), "a save never drops a version");
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
    assert_eq!(ok(&mut s, "version.list", json!({}))["read_only"], false);
    rect(&mut s, 0);
    let one = create(&mut s, "One");
    rect(&mut s, 20);
    let two = create(&mut s, "Two");
    let l = ok(&mut s, "version.list", json!({}));
    assert_eq!(l["count"], 2);
    assert_eq!(l["versions"][0]["name"], "One");
    assert!(l["versions"][0]["delta_bytes"].as_u64().unwrap() > 0);
    assert_eq!(
        l["versions"][1]["delta_bytes"], 0,
        "the newest has no delta"
    );
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
    assert!(r.get("dropped").is_none());
    assert_eq!(object_count(&mut s), 1);
    assert_eq!(s.editor.undo_depth(), depth + 1);
    let l = ok(&mut s, "version.list", json!({}));
    assert_eq!(l["count"], 2);
    assert_eq!(l["versions"][1]["auto"], true);
    assert_eq!(l["versions"][1]["time_ms"], 5);
    assert!(l["versions"][1]["name"].as_str().unwrap().contains("One"));
    // The change event names the history, the step and what it did.
    let notes = s.take_notifications(LOCAL);
    let ev = &notes.last().unwrap()["params"];
    assert_eq!(ev["op"], "version_restore");
    assert_eq!(ev["document"]["history"], true);
    assert_eq!(ev["removed"]["objects"].as_array().unwrap().len(), 1);
    assert_eq!(ev["detail"]["restored"], json!({"id": one, "name": "One"}));
    assert_eq!(ev["detail"]["saved_version"], json!({"id": 2, "new": true}));
    // The work can be brought back from the automatic version.
    let back = ok(&mut s, "version.restore", json!({"id": 2}));
    assert_eq!(back["saved_version"]["new"], true);
    assert_eq!(object_count(&mut s), 2);
    // One undo takes the last restore away, and another the first, exactly.
    assert!(s.editor.undo());
    assert_eq!(object_count(&mut s), 1);
    assert!(s.editor.undo());
    assert_eq!(s.editor.doc(), &before);
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
fn delete_names_what_it_deleted_and_is_one_undo_step() {
    let mut s = Session::with_sequential_ids();
    rect(&mut s, 0);
    let one = create(&mut s, "One");
    rect(&mut s, 20);
    let two = create(&mut s, "Two");
    rect(&mut s, 40);
    let three = create(&mut s, "Three");
    ok(&mut s, "events.subscribe", json!({}));
    let depth = s.editor.undo_depth();
    let d = ok(&mut s, "version.delete", json!({"id": two}));
    assert_eq!(d["count"], 2);
    assert_eq!(d["deleted"][0]["id"], two);
    assert_eq!(d["deleted"][0]["name"], "Two");
    assert!(d["deleted"][0]["bytes"].as_u64().unwrap() > 0);
    assert_eq!(s.editor.undo_depth(), depth + 1);
    // The change event names the deleted version too.
    let notes = s.take_notifications(LOCAL);
    let ev = &notes.last().unwrap()["params"];
    assert_eq!(ev["op"], "version_delete");
    assert_eq!(ev["detail"]["deleted"][0]["name"], "Two");
    assert_eq!(ev["document"]["history"], true);
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
    // Several at once, in one step; nothing is deleted if one is missing.
    let depth = s.editor.undo_depth();
    assert_eq!(
        fail(&mut s, "version.delete", json!({"ids": [one, 99]})).kind,
        "not_found"
    );
    assert_eq!(ok(&mut s, "version.list", json!({}))["count"], 3);
    let d = ok(&mut s, "version.delete", json!({"ids": [one, three]}));
    assert_eq!(d["deleted"].as_array().unwrap().len(), 2);
    assert_eq!(s.editor.undo_depth(), depth + 1);
    // The newest version can go too: the one before it takes its place.
    assert_eq!(
        ok(&mut s, "version.get", json!({"id": two, "document": true}))["things"]["objects"],
        2
    );
    ok(&mut s, "version.delete", json!({"id": two}));
    assert!(s.editor.doc().history.is_none());
    assert_eq!(ok(&mut s, "version.list", json!({}))["count"], 0);
    for bad in [
        json!({}),
        json!({"ids": []}),
        json!({"ids": ["a"]}),
        json!({"id": 1, "ids": [1]}),
    ] {
        assert_eq!(fail(&mut s, "version.delete", bad).code, -32602);
    }
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
fn a_limit_stops_the_save_and_the_error_lists_the_versions() {
    let mut s = Session::with_sequential_ids();
    s.set_limits(SessionLimits {
        max_versions: 3,
        ..SessionLimits::default()
    });
    for i in 0..3 {
        rect(&mut s, i * 20);
        create(&mut s, &format!("v{i}"));
    }
    rect(&mut s, 100);
    let history_before = s.editor.doc().history.clone();
    let depth = s.editor.undo_depth();
    let e = fail(&mut s, "version.create", json!({"name": "one too many"}));
    assert_eq!((e.code, e.kind), (6, "limit"));
    assert_eq!(e.reason, Some(REASON_VERSION_LIMIT));
    let d = e.details.as_ref().unwrap();
    assert_eq!(
        d["limit"],
        json!({"kind": "versions", "max": 3, "needed": 4})
    );
    assert_eq!(d["versions"].as_array().unwrap().len(), 3);
    assert_eq!(d["versions"][0]["name"], "v0");
    assert!(d["versions"][0]["bytes"].as_u64().unwrap() > 0);
    assert!(d["versions"][0].get("time_ms").is_some());
    assert_eq!(d["ask_the_person"], true);
    assert!(d["options"].as_array().unwrap().len() == 2);
    assert!(e.message.contains("nothing was deleted"));
    // Nothing changed: not the history, not the undo steps.
    assert_eq!(s.editor.doc().history, history_before);
    assert_eq!(s.editor.undo_depth(), depth);
    // The error goes over the wire with the facts in data.
    let line = s
        .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"version.create","params":{"name":"x"}}"#)
        .unwrap();
    let wire: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(wire["error"]["code"], 6);
    assert_eq!(wire["error"]["data"]["kind"], "limit");
    assert_eq!(
        wire["error"]["data"]["versions"].as_array().unwrap().len(),
        3
    );
    // A restore whose automatic version would go over the limit is refused the same way.
    let doc_before = s.editor.doc().clone();
    let e = fail(&mut s, "version.restore", json!({"id": 1}));
    assert_eq!(e.reason, Some(REASON_VERSION_LIMIT));
    assert_eq!(s.editor.doc(), &doc_before);
    // The person deletes the versions that they choose (not the oldest), and the save goes on.
    ok(&mut s, "version.delete", json!({"ids": [2]}));
    create(&mut s, "now it fits");
    let names: Vec<String> = ok(&mut s, "version.list", json!({}))["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["v0", "v2", "now it fits"]);
    // Or the person raises the limit.
    rect(&mut s, 200);
    s.set_limits(SessionLimits {
        max_versions: 4,
        ..SessionLimits::default()
    });
    create(&mut s, "after raising the limit");
    // Off.
    s.set_limits(SessionLimits {
        max_versions: 0,
        ..SessionLimits::default()
    });
    assert_eq!(
        fail(&mut s, "version.create", json!({"name": "x"})).kind,
        "limit"
    );
    assert_eq!(SessionLimits::strict().max_versions, 20);
}

#[test]
fn the_byte_limit_stops_the_save_too() {
    let mut s = Session::with_sequential_ids();
    for i in 0..4 {
        rect(&mut s, i * 20);
        create(&mut s, &format!("v{i}"));
    }
    let used = ok(&mut s, "version.list", json!({}))["bytes"]
        .as_u64()
        .unwrap();
    s.set_limits(SessionLimits {
        max_version_bytes: used + 10,
        ..SessionLimits::default()
    });
    rect(&mut s, 500);
    let e = fail(&mut s, "version.create", json!({"name": "big"}));
    assert_eq!(e.reason, Some(REASON_VERSION_LIMIT));
    let d = e.details.unwrap();
    assert_eq!(d["limit"]["kind"], "bytes");
    assert!(d["limit"]["needed"].as_u64().unwrap() > used + 10);
    assert_eq!(ok(&mut s, "version.list", json!({}))["count"], 4);
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
}

#[test]
fn document_get_leaves_the_history_out_and_a_save_writes_the_file_form() {
    let mut s = Session::with_sequential_ids();
    rect(&mut s, 0);
    create(&mut s, "One");
    rect(&mut s, 20);
    create(&mut s, "Two");
    rect(&mut s, 40);
    assert!(
        ok(&mut s, "document.get", json!({}))
            .get("history")
            .is_none()
    );
    // The file: the newest version is a change from the drawing, and the drawing is complete.
    let (file, mut t) = through_file(&mut s);
    let h = &file["history"];
    assert_eq!(h["format"], 2);
    assert!(h["newest"].get("snapshot").is_none() && h["newest"]["delta"].is_array());
    assert_eq!(h["newest"]["anchor"].as_str().unwrap().len(), 64);
    assert!(h.get("base").is_none());
    assert_eq!(file["objects"].as_array().unwrap().len(), 3);
    // The session that opened the file has the versions and the same content for each.
    assert!(t.load_notes().is_empty(), "{:?}", t.load_notes());
    let l = ok(&mut t, "version.list", json!({}));
    assert_eq!(l["count"], 2);
    assert_eq!(l["read_only"], false);
    for id in [1u64, 2] {
        let a = ok(&mut s, "version.get", json!({"id": id, "document": true}));
        let b = ok(&mut t, "version.get", json!({"id": id, "document": true}));
        assert_eq!(
            a["document"]["objects"], b["document"]["objects"],
            "version {id}"
        );
    }
    // The opened session keeps the versions in the self contained form: the next change and
    // save work, and a drawing that changed since is saved as a new change.
    rect(&mut t, 60);
    create(&mut t, "Three");
    rect(&mut t, 80);
    let (file, mut u) = through_file(&mut t);
    assert_eq!(ok(&mut u, "version.list", json!({}))["count"], 3);
    assert_eq!(file["objects"].as_array().unwrap().len(), 5);
    let d = ok(&mut u, "version.diff", json!({"from": 3, "to": "current"}));
    assert_eq!(d["totals"], json!({"added": 1, "removed": 0, "changed": 0}));
    // The file written by `file.save` is the same form.
    let mut dir = std::env::temp_dir();
    dir.push(format!("kadakas-review-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.kad");
    let mut w = Session::with_sequential_ids();
    rect(&mut w, 0);
    let one = create(&mut w, "One");
    crate::files::write_document_file(&path, &w.document_for_save().unwrap()).unwrap();
    let read = crate::files::read_document(&path).unwrap();
    assert!(Session::with_document(read).editor.doc().history.is_some());
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(one, 1);
}

#[test]
fn a_history_that_cannot_be_read_stays_as_it_is_and_is_read_only() {
    let mut donor = Session::with_sequential_ids();
    rect(&mut donor, 0);
    create(&mut donor, "One");
    rect(&mut donor, 20);
    create(&mut donor, "Two");
    let mut file = serde_json::to_value(&*donor.document_for_save().unwrap()).unwrap();
    // A delta that does not fit: the older version cannot be rebuilt.
    file["history"]["versions"][0]["delta"] = json!([{"op": "delete",
        "target": {"kind": "object", "id": "00000000-0000-0000-0000-0000000000ee"}}]);
    let original = file["history"].clone();

    let mut t = Session::with_sequential_ids();
    let r = ok(&mut t, "document.load", json!({"document": file.clone()}));
    assert!(!r["notes"].as_array().unwrap().is_empty(), "{r}");
    assert!(r["notes"].to_string().contains("read only"));
    // The history is the file's history, not a repaired one.
    assert_eq!(
        &t.editor.doc().history.as_ref().unwrap().to_value(),
        &original
    );
    // Reading works on what could be read; the list says what is the matter.
    let l = ok(&mut t, "version.list", json!({}));
    assert_eq!(l["read_only"], true);
    assert_eq!(l["count"], 1);
    assert!(l["notes"][0].as_str().unwrap().contains("left out"));
    // Nothing changes the history until the person decides.
    for (m, p) in [
        ("version.create", json!({"name": "x"})),
        ("version.restore", json!({"id": 2})),
        ("version.delete", json!({"id": 2})),
    ] {
        let e = fail(&mut t, m, p);
        assert_eq!(
            (e.kind, e.reason),
            ("conflict", Some(REASON_HISTORY_READONLY))
        );
        assert!(e.details.as_ref().unwrap()["notes"].is_array());
    }
    assert_eq!(
        &t.editor.doc().history.as_ref().unwrap().to_value(),
        &original
    );
    // A save writes it back as it is.
    let saved = serde_json::to_value(&*t.document_for_save().unwrap()).unwrap();
    assert_eq!(saved["history"], original);
    // The repair needs the explicit yes.
    assert_eq!(fail(&mut t, "version.repair", json!({})).code, -32602);
    assert_eq!(
        fail(&mut t, "version.repair", json!({"confirm": false})).code,
        -32602
    );
    ok(&mut t, "events.subscribe", json!({}));
    let r = ok(&mut t, "version.repair", json!({"confirm": true}));
    assert_eq!(r["kept"], 1);
    let ev = &t.take_notifications(LOCAL).last().unwrap()["params"].clone();
    assert_eq!(ev["op"], "version_repair");
    assert_eq!(ev["detail"]["repaired"]["kept"], 1);
    let l = ok(&mut t, "version.list", json!({}));
    assert_eq!(
        (l["read_only"].clone(), l["count"].clone()),
        (json!(false), json!(1))
    );
    create(&mut t, "works again");
    assert_eq!(
        fail(&mut t, "version.repair", json!({"confirm": true})).kind,
        "conflict",
        "nothing left to repair"
    );
    // One undo brings the unreadable history back, untouched.
    assert!(t.editor.undo());
    assert!(t.editor.undo());
    assert_eq!(
        &t.editor.doc().history.as_ref().unwrap().to_value(),
        &original
    );
    assert_eq!(ok(&mut t, "version.list", json!({}))["read_only"], true);
}

#[test]
fn a_drawing_that_changed_after_the_versions_were_saved_keeps_the_history_unread() {
    let mut donor = Session::with_sequential_ids();
    rect(&mut donor, 0);
    create(&mut donor, "One");
    rect(&mut donor, 20);
    create(&mut donor, "Two");
    let mut file = serde_json::to_value(&*donor.document_for_save().unwrap()).unwrap();
    // A property of an object is not what the versions have (a newer program reads a file in
    // another way): the same things, so the versions are rebuilt from the drawing as it is.
    let mut moved = file.clone();
    moved["objects"][0]["shape"]["min"] = json!([1.0, 0.0]);
    let mut t = Session::with_sequential_ids();
    let r = ok(&mut t, "document.load", json!({"document": moved}));
    assert!(
        r["notes"].to_string().contains("not exactly the one"),
        "{r}"
    );
    let l = ok(&mut t, "version.list", json!({}));
    assert_eq!(
        (l["read_only"].clone(), l["count"].clone()),
        (json!(false), json!(2))
    );
    create(&mut t, "after");
    // Another program deleted an object: other things, no way to rebuild the versions.
    let mut fewer = file.clone();
    fewer["objects"].as_array_mut().unwrap().remove(0);
    let original = file["history"].clone();
    let mut t = Session::with_sequential_ids();
    file = fewer;
    let r = ok(&mut t, "document.load", json!({"document": file}));
    assert!(
        r["notes"]
            .to_string()
            .contains("does not match the one that the 2 versions"),
        "{r}"
    );
    assert_eq!(
        &t.editor.doc().history.as_ref().unwrap().to_value(),
        &original
    );
    let l = ok(&mut t, "version.list", json!({}));
    assert_eq!(
        (l["read_only"].clone(), l["count"].clone()),
        (json!(true), json!(0))
    );
    assert_eq!(l["unreadable_versions"], 2, "what a repair would delete");
    // The file is saved with the history as it was.
    let saved = serde_json::to_value(&*t.document_for_save().unwrap()).unwrap();
    assert_eq!(saved["history"], original);
}

#[test]
fn a_history_of_another_form_is_kept_as_it_is() {
    let mut donor = Session::with_sequential_ids();
    rect(&mut donor, 0);
    create(&mut donor, "One");
    let mut file = serde_json::to_value(&*donor.document_for_save().unwrap()).unwrap();
    for bad in [json!("nonsense"), json!([1, 2]), json!({"format": 99})] {
        file["history"] = bad.clone();
        let mut t = Session::with_sequential_ids();
        let r = ok(&mut t, "document.load", json!({"document": file.clone()}));
        assert!(r["notes"].as_array().unwrap().len() >= 2, "{r}");
        assert_eq!(&t.editor.doc().history.as_ref().unwrap().to_value(), &bad);
        assert_eq!(ok(&mut t, "version.list", json!({}))["read_only"], true);
        assert_eq!(
            serde_json::to_value(&*t.document_for_save().unwrap()).unwrap()["history"],
            bad
        );
    }
    // The constructor for an opened file checks it too and keeps the history.
    let mut doc = Document::with_sequential_ids();
    doc.history = Some(HistoryData::new(json!({"format": 7})));
    let v = Session::with_document(doc);
    assert!(v.editor.doc().history.is_some());
    assert!(!v.load_notes().is_empty());
}

#[test]
fn a_change_of_the_history_is_a_document_event_with_a_detail() {
    let mut s = Session::with_sequential_ids();
    ok(&mut s, "events.subscribe", json!({"kinds": ["document"]}));
    rect(&mut s, 0);
    assert!(s.take_notifications(LOCAL).is_empty());
    create(&mut s, "One");
    let notes = s.take_notifications(LOCAL);
    assert_eq!(notes.len(), 1);
    let p = &notes[0]["params"];
    assert_eq!(p["op"], "version_create");
    assert_eq!(p["document"]["history"], true);
    assert_eq!(p["detail"]["created"]["name"], "One");
    ok(&mut s, "version.delete", json!({"ids": [1]}));
    let notes = s.take_notifications(LOCAL);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0]["params"]["detail"]["deleted"][0]["id"], 1);
    ok(&mut s, "undo", json!({}));
    let notes = s.take_notifications(LOCAL);
    assert_eq!(notes.len(), 1);
    assert!(notes[0]["params"].get("detail").is_none());
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
        "version.repair",
    ] {
        assert!(methods.contains(&m), "{m}");
    }
    for e in d["review"]["examples"].as_array().unwrap() {
        let mut t = Session::with_sequential_ids();
        rect(&mut t, 0);
        create(&mut t, "A");
        rect(&mut t, 20);
        create(&mut t, "B");
        let r = t.handle(e["method"].as_str().unwrap(), e["params"].clone());
        assert!(r.is_ok(), "{e}: {:?}", r.err().map(|e| e.message));
    }
    assert!(d["review"]["local"].as_str().unwrap().contains("REV-09"));
    // The texts say what an agent must do.
    let text = |name: &str| {
        d["methods"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["name"] == name)
            .unwrap()["doc"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert!(text("version.create").contains("Pass this to the person"));
    assert!(text("version.create").contains("Never delete versions by yourself"));
    assert!(text("version.restore").contains("ask the person"));
    assert!(
        d["review"]["consent"]
            .as_str()
            .unwrap()
            .contains("without the person")
    );
}
