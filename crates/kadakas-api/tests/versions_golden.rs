//! A committed file with versions, made by an earlier build, must read with no notes. If a change
//! of the document model (a new property, a new repair at load, another default) makes this test
//! fail, the versions of every file that people have would open read only. Fix the reading (or
//! make the change tolerant), do not just make the file again.
//!
//! To make the file again on purpose (a new format of the history):
//!
//! ```text
//! KADAKAS_UPDATE_GOLDEN=1 cargo test -p kadakas-api --test versions_golden
//! ```

use std::path::PathBuf;

use kadakas_api::Session;
use kadakas_core::model::Document;
use serde_json::{Value, json};

fn path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/versions_golden.kad")
}

fn call(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

/// Four objects of different kinds on two layers, and three versions with edits between them. A
/// last edit comes after the last version, so the newest change is not empty.
fn make() -> Session {
    let mut s = Session::with_sequential_ids();
    let ex = |s: &mut Session, c: Value| call(s, "execute", json!({"command": c}));
    ex(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [400, 300]}}),
    );
    ex(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "circle", "center": [900, 400], "radius": 150}}),
    );
    call(
        &mut s,
        "version.create",
        json!({"name": "Start", "note": "first", "now_ms": 1_700_000_000_000u64}),
    );
    ex(&mut s, json!({"op": "add_layer", "name": "Planting"}));
    ex(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "line", "a": [0, 500], "b": [1200, 500]}}),
    );
    call(
        &mut s,
        "version.create",
        json!({"name": "Planting layer", "now_ms": 1_700_000_100_000u64}),
    );
    let ids: Vec<Value> = call(&mut s, "objects.list", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["id"].clone())
        .collect();
    ex(
        &mut s,
        json!({"op": "move_objects", "ids": [ids[0].clone()], "delta": [50, 20]}),
    );
    ex(
        &mut s,
        json!({"op": "delete_objects", "ids": [ids[1].clone()]}),
    );
    call(
        &mut s,
        "version.create",
        json!({"name": "Moved and deleted", "now_ms": 1_700_000_200_000u64}),
    );
    ex(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "rect", "min": [2000, 0], "max": [2100, 100]}}),
    );
    s
}

#[test]
fn a_file_with_versions_from_an_earlier_build_reads_with_no_notes() {
    if std::env::var("KADAKAS_UPDATE_GOLDEN").is_ok() {
        let mut s = make();
        let doc = s.document_for_save().unwrap();
        std::fs::create_dir_all(path().parent().unwrap()).unwrap();
        let file = std::fs::File::create(path()).unwrap();
        doc.write_pretty(file).unwrap();
    }
    let text = std::fs::read_to_string(path()).expect("the committed file");
    let doc = Document::from_json(&text).unwrap();
    let mut s = Session::with_document(doc);
    assert!(
        s.load_notes().is_empty(),
        "the versions of the committed file do not read any more: {:?}",
        s.load_notes()
    );
    let list = call(&mut s, "version.list", json!({}));
    assert_eq!(list["read_only"], false, "{list}");
    assert_eq!(list["count"], 3);
    let names: Vec<&str> = list["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Start", "Planting layer", "Moved and deleted"]);
    // What each version holds: the count of objects (2, 3, 2) and of layers (1, 2, 2).
    for (id, objects, layers) in [(1, 2, 1), (2, 3, 2), (3, 2, 2)] {
        let v = call(&mut s, "version.get", json!({"id": id}));
        assert_eq!(v["things"]["objects"], objects, "version {id}: {v}");
        assert_eq!(v["things"]["layers"], layers, "version {id}: {v}");
    }
    // The drawing has one object more than the newest version.
    let d = call(&mut s, "version.diff", json!({"from": 3, "to": "current"}));
    assert_eq!(d["totals"]["added"], 1, "{d}");
    assert_eq!(d["totals"]["removed"], 0);
    assert_eq!(d["exact"], true);
    // And the file still saves, and the save reads again with no notes.
    let again = s.document_for_save().unwrap().into_owned();
    let mut bytes = Vec::new();
    again.write_pretty(&mut bytes).unwrap();
    let back =
        Session::with_document(Document::from_json(std::str::from_utf8(&bytes).unwrap()).unwrap());
    assert!(back.load_notes().is_empty(), "{:?}", back.load_notes());
}
