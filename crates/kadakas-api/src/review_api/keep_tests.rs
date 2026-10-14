//! The versions stay through `document.get` and `document.load`, an undo step keeps its anchor,
//! and an old `base` does not leave its pictures behind.

use serde_json::{Value, json};

use super::*;

fn ok(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
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

fn version_document(s: &mut Session, id: u64) -> Value {
    ok(s, "version.get", json!({"id": id, "document": true}))["document"].clone()
}

#[test]
fn a_document_taken_with_get_and_loaded_again_keeps_the_versions() {
    let mut s = Session::with_sequential_ids();
    let mut ids = Vec::new();
    for i in 0..5 {
        ids.push(rect(&mut s, i * 20));
        create(&mut s, &format!("v{i}"));
    }
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [ids[0].clone()], "delta": [3, 3]}}),
    );
    let expected: Vec<(u64, Value)> = (1..=5)
        .map(|id| (id, version_document(&mut s, id)))
        .collect();
    // The agent takes the document (without the versions), changes it and loads it again.
    let mut doc = ok(&mut s, "document.get", json!({}));
    assert!(doc.get("history").is_none());
    doc["objects"].as_array_mut().unwrap().pop();
    let r = ok(&mut s, "document.load", json!({"document": doc}));
    assert!(r.get("notes").is_none(), "{r}");
    let l = ok(&mut s, "version.list", json!({}));
    assert_eq!(
        (l["read_only"].clone(), l["count"].clone()),
        (json!(false), json!(5))
    );
    for (id, doc) in &expected {
        assert_eq!(&version_document(&mut s, *id), doc, "version {id}");
    }
    // The versions are written with the new drawing, and the file reads again.
    let mut text = Vec::new();
    s.document_for_save()
        .unwrap()
        .write_pretty(&mut text)
        .unwrap();
    let t =
        Session::with_document(Document::from_json(std::str::from_utf8(&text).unwrap()).unwrap());
    assert!(t.load_notes().is_empty(), "{:?}", t.load_notes());
    // A document with its own versions brings them.
    let mut other = Session::with_sequential_ids();
    rect(&mut other, 0);
    create(&mut other, "other");
    let with = serde_json::to_value(&*other.document_for_save().unwrap()).unwrap();
    assert!(with.get("history").is_some());
    let r = ok(&mut s, "document.load", json!({"document": with}));
    assert!(
        r["notes"]
            .to_string()
            .contains("5 versions of the document that was open are gone"),
        "the reply says that the versions were replaced: {r}"
    );
    let l = ok(&mut s, "version.list", json!({}));
    assert_eq!(l["count"], 1);
}

#[test]
fn an_undo_step_keeps_the_drawing_that_its_versions_are_written_against() {
    // Each time the last version is deleted, the next version has a new anchor. Many anchors
    // come and go; undo must read every history it goes back to, however many there were.
    let mut s = Session::with_sequential_ids();
    for round in 0..8 {
        rect(&mut s, round * 20);
        let id = create(&mut s, &format!("round {round}"));
        ok(&mut s, "version.delete", json!({"id": id}));
    }
    for round in (0..8).rev() {
        assert!(s.editor.undo(), "undo the delete of round {round}");
        let l = ok(&mut s, "version.list", json!({}));
        assert_eq!(l["read_only"], false, "round {round}: {l}");
        assert_eq!(l["count"], 1, "round {round}");
        assert!(s.editor.undo(), "undo the version of round {round}");
        assert!(s.editor.undo(), "undo the rectangle of round {round}");
    }
}

#[test]
fn the_pictures_that_an_old_base_brought_are_taken_out_when_it_is_replaced() {
    let mut s = Session::with_sequential_ids();
    rect(&mut s, 0);
    create(&mut s, "plain");
    let mut bytes = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    let mut seed = 3u32;
    for _ in 0..100_000 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        bytes.push((seed >> 24) as u8);
    }
    let png = kadakas_core::underlay::base64_encode(&bytes);
    let under = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "underlay",
            "min": [0, 0], "max": [640, 640], "width": 64, "height": 64,
            "png": png, "opacity": 1.0}}}),
    )["created"][0]
        .clone();
    create(&mut s, "with picture");
    let mut text = Vec::new();
    s.document_for_save()
        .unwrap()
        .write_pretty(&mut text)
        .unwrap();
    let file = Document::from_json(std::str::from_utf8(&text).unwrap()).unwrap();
    let mut t = Session::with_sequential_ids();
    let mut limits = t.limits().clone();
    limits.history_read_versions = 1;
    t.set_limits(limits);
    t.replace_document(file);
    assert_eq!(ok(&mut t, "version.list", json!({}))["read_only"], true);
    // The picture goes: the base needs it. Then it comes back: the base does not need it.
    ok(
        &mut t,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [under.clone()]}}),
    );
    let gone = t.document_for_save().unwrap().history.clone().unwrap();
    assert_eq!(gone.blob_list().len(), 1, "the base needs the picture");
    ok(&mut t, "undo", json!({}));
    let back = t.document_for_save().unwrap().history.clone().unwrap();
    assert!(
        back.blob_list().is_empty(),
        "the picture of the old base is gone"
    );
    assert!(back.top().get("base").is_none());
}

#[test]
fn the_source_data_of_a_dxf_import_goes_through_versions_and_the_file() {
    let mut s = Session::with_sequential_ids();
    let id = rect(&mut s, 0);
    create(&mut s, "plain");
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_data", "id": id, "set": {"import": {
            "drawing": "guid:{A}", "revision": "r1", "handle": "1F", "kind": "LINE",
            "hash": "00000000000000ab", "obj": "00000000000000cd"}}}}),
    );
    let recs: Vec<Value> = (0..300)
        .map(|n| json!({"kind": "LINE", "handle": format!("{n:X}"), "layer": "Walls", "text": "t".repeat(300)}))
        .collect();
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_resource", "name": "import base", "body": {
            "type": "import_base", "drawing": "guid:{A}", "revision": "r1",
            "part": 0, "parts": 1, "recs": recs}}}),
    );
    create(&mut s, "imported");
    let one = version_document(&mut s, 2);
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [id]}}),
    );
    create(&mut s, "deleted");
    // The base is one picture in the history, and the file reads back with both versions.
    let data = s.editor.doc().history.clone().unwrap();
    assert_eq!(data.blob_list().len(), 1);
    let mut text = Vec::new();
    s.document_for_save()
        .unwrap()
        .write_pretty(&mut text)
        .unwrap();
    let mut t =
        Session::with_document(Document::from_json(std::str::from_utf8(&text).unwrap()).unwrap());
    assert!(t.load_notes().is_empty(), "{:?}", t.load_notes());
    assert_eq!(version_document(&mut t, 2), one);
    ok(&mut t, "version.restore", json!({"id": 2, "now_ms": 5}));
    let d = ok(&mut t, "version.diff", json!({"from": 2, "to": "current"}));
    assert_eq!(
        d["totals"],
        json!({"added": 0, "removed": 0, "changed": 0}),
        "{d}"
    );
}
