//! Committed files with versions, made by an earlier build, must read with no notes and give the
//! same content for each version. This is the guard against drift of the format: a change of the
//! document model (a new property, a new repair at load, another default) that makes a test fail
//! here would make the versions of every file that people have open read only. Fix the reading
//! (or make the change keep the old reading), do not just make the files again.
//!
//! Three files: `versions_golden.kad` (shapes of three kinds, a layer, three versions and an edit
//! after the last one), `versions_golden_picture.kad` (an underlay with a picture that the
//! history keeps as a blob) and `versions_golden_base.kad` (a history that a reader kept as it was
//! while the drawing was edited, so it has a `base` entry). To make them again on purpose (a new
//! format of the history):
//!
//! ```text
//! cargo test -p kadakas-api --test versions_golden make_the_files -- --ignored
//! ```

use std::path::PathBuf;

use kadakas_api::Session;
use kadakas_core::model::Document;
use serde_json::{Value, json};

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn call(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

fn ex(s: &mut Session, c: Value) -> Value {
    call(s, "execute", json!({"command": c}))
}

fn save(s: &mut Session, name: &str) {
    let doc = s.document_for_save().unwrap();
    std::fs::create_dir_all(path(name).parent().unwrap()).unwrap();
    doc.write_pretty(std::fs::File::create(path(name)).unwrap())
        .unwrap();
}

/// Objects of three kinds on two layers, and three versions with edits between them. A last edit
/// comes after the last version, so the newest change is not empty.
fn make() -> Session {
    let mut s = Session::with_sequential_ids();
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

/// A plain drawing, a version, then an underlay with a picture (about 6 KB, so it is a blob), a
/// second version, and the underlay moved and a third version.
fn make_picture() -> Session {
    let mut s = Session::with_sequential_ids();
    ex(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [100, 100]}}),
    );
    call(
        &mut s,
        "version.create",
        json!({"name": "Plain", "now_ms": 1_700_000_000_000u64}),
    );
    let mut bytes = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    let mut seed = 11u32;
    for _ in 0..6000 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        bytes.push((seed >> 24) as u8);
    }
    let png = kadakas_core::underlay::base64_encode(&bytes);
    let id = ex(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "underlay", "min": [0, 0], "max": [640, 640],
            "width": 64, "height": 64, "png": png, "opacity": 1.0}}),
    )["created"][0]
        .clone();
    call(
        &mut s,
        "version.create",
        json!({"name": "With picture", "now_ms": 1_700_000_100_000u64}),
    );
    ex(
        &mut s,
        json!({"op": "move_objects", "ids": [id.clone()], "delta": [10, 10]}),
    );
    call(
        &mut s,
        "version.create",
        json!({"name": "Picture moved", "now_ms": 1_700_000_200_000u64}),
    );
    // The drawing no longer has the picture, so the history must hold it.
    ex(&mut s, json!({"op": "delete_objects", "ids": [id]}));
    s
}

/// The file of `make`, opened by a reader that reads one version at most (so the history is kept
/// as it is), with the drawing edited and saved.
fn make_base() -> Session {
    let text = std::fs::read_to_string(path("versions_golden.kad")).unwrap();
    let mut t = Session::with_sequential_ids();
    let mut limits = t.limits().clone();
    limits.history_read_versions = 1;
    t.set_limits(limits);
    t.replace_document(Document::from_json(&text).unwrap());
    ex(&mut t, json!({"op": "add_layer", "name": "Edited later"}));
    let first = call(&mut t, "objects.list", json!({}))[0]["id"].clone();
    ex(&mut t, json!({"op": "delete_objects", "ids": [first]}));
    ex(
        &mut t,
        json!({"op": "add_object", "shape": {"kind": "circle", "center": [5, 5], "radius": 3}}),
    );
    t
}

fn open(name: &str) -> Session {
    let text = std::fs::read_to_string(path(name)).expect("the committed file");
    Session::with_document(Document::from_json(&text).unwrap())
}

/// The shape of the first object of a kind in a version.
fn shape_of(s: &mut Session, id: u64, kind: &str) -> Value {
    let v = call(s, "version.get", json!({"id": id, "document": true}));
    v["document"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["shape"]["kind"] == kind)
        .map(|o| o["shape"].clone())
        .unwrap_or(Value::Null)
}

/// Makes the files again (see the top of this file). Not part of a normal run.
#[test]
#[ignore = "makes the committed files again"]
fn make_the_files() {
    save(&mut make(), "versions_golden.kad");
    save(&mut make_picture(), "versions_golden_picture.kad");
    save(&mut make_base(), "versions_golden_base.kad");
}

#[test]
fn a_file_with_versions_from_an_earlier_build_reads_with_no_notes() {
    let mut s = open("versions_golden.kad");
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
    // One shape value for each version: the rectangle moved in the third, the circle is gone, and
    // the line came with the second.
    let rect = |min: [f64; 2], max: [f64; 2]| json!({"kind": "rect", "min": min, "max": max});
    assert_eq!(
        shape_of(&mut s, 1, "rect"),
        rect([0.0, 0.0], [400.0, 300.0])
    );
    assert_eq!(
        shape_of(&mut s, 2, "rect"),
        rect([0.0, 0.0], [400.0, 300.0])
    );
    assert_eq!(
        shape_of(&mut s, 3, "rect"),
        rect([50.0, 20.0], [450.0, 320.0])
    );
    assert_eq!(shape_of(&mut s, 1, "circle")["radius"], 150.0);
    assert_eq!(shape_of(&mut s, 3, "circle"), Value::Null);
    assert_eq!(shape_of(&mut s, 1, "line"), Value::Null);
    assert_eq!(shape_of(&mut s, 2, "line")["b"], json!([1200.0, 500.0]));
    // The drawing has one object more than the newest version.
    let d = call(&mut s, "version.diff", json!({"from": 3, "to": "current"}));
    assert_eq!(d["totals"]["added"], 1, "{d}");
    assert_eq!(d["totals"]["removed"], 0);
    assert_eq!(d["exact"], true);
    // And the file still saves, and the save reads again with no notes.
    let mut bytes = Vec::new();
    s.document_for_save()
        .unwrap()
        .write_pretty(&mut bytes)
        .unwrap();
    let back =
        Session::with_document(Document::from_json(std::str::from_utf8(&bytes).unwrap()).unwrap());
    assert!(back.load_notes().is_empty(), "{:?}", back.load_notes());
}

#[test]
fn a_file_with_a_picture_in_its_versions_reads_with_no_notes() {
    let mut s = open("versions_golden_picture.kad");
    assert!(s.load_notes().is_empty(), "{:?}", s.load_notes());
    let text = std::fs::read_to_string(path("versions_golden_picture.kad")).unwrap();
    let file: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        file["history"]["blobs"]
            .as_object()
            .map(serde_json::Map::len),
        Some(1),
        "the picture is one blob of the history"
    );
    let list = call(&mut s, "version.list", json!({}));
    assert_eq!(
        (list["read_only"].clone(), list["count"].clone()),
        (json!(false), json!(3))
    );
    assert_eq!(shape_of(&mut s, 1, "underlay"), Value::Null);
    // The picture is whole in the versions that have it, and the underlay moved in the third.
    let with = shape_of(&mut s, 2, "underlay");
    let moved = shape_of(&mut s, 3, "underlay");
    assert_eq!(with["min"], json!([0.0, 0.0]));
    assert_eq!(moved["min"], json!([10.0, 10.0]));
    assert_eq!(with["png"], moved["png"]);
    assert!(with["png"].as_str().unwrap().len() > 6000);
    let d = call(&mut s, "version.diff", json!({"from": 3, "to": "current"}));
    assert_eq!(
        d["totals"],
        json!({"added": 0, "removed": 1, "changed": 0}),
        "{d}"
    );
}

#[test]
fn a_file_with_the_way_back_of_a_kept_history_gives_every_version() {
    let text = std::fs::read_to_string(path("versions_golden_base.kad")).unwrap();
    assert!(text.contains("\"base\""), "the file has a base entry");
    let mut s = open("versions_golden_base.kad");
    assert!(s.load_notes().is_empty(), "{:?}", s.load_notes());
    let list = call(&mut s, "version.list", json!({}));
    assert_eq!(
        (list["read_only"].clone(), list["count"].clone()),
        (json!(false), json!(3))
    );
    // The versions are the ones of the first file, although the drawing was edited since.
    let mut first = open("versions_golden.kad");
    for id in 1..=3 {
        let a = call(&mut s, "version.get", json!({"id": id, "document": true}));
        let b = call(
            &mut first,
            "version.get",
            json!({"id": id, "document": true}),
        );
        assert_eq!(a["document"], b["document"], "version {id}");
    }
}
