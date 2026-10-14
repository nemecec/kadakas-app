//! A history that is kept as it is (it could not be read completely) keeps the drawing that it
//! belongs to, so that no version is lost when the drawing is edited and saved.

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

/// What a save writes, read again as a file would be (through the text).
fn through_the_file(s: &mut Session) -> Document {
    let mut text = Vec::new();
    s.document_for_save()
        .unwrap()
        .write_pretty(&mut text)
        .unwrap();
    Document::from_json(std::str::from_utf8(&text).unwrap()).unwrap()
}

/// The document of a version, as `version.get` gives it.
fn version_document(s: &mut Session, id: u64) -> Value {
    ok(s, "version.get", json!({"id": id, "document": true}))["document"].clone()
}

fn count(s: &mut Session) -> u64 {
    ok(s, "version.list", json!({"current": false}))["count"]
        .as_u64()
        .unwrap()
}

/// A session that read `doc` with a cap on the versions that it reads.
fn open_with_cap(doc: Document, versions: usize) -> Session {
    let mut s = Session::with_sequential_ids();
    let mut limits = s.limits().clone();
    limits.history_read_versions = versions;
    s.set_limits(limits);
    s.replace_document(doc);
    s
}

#[test]
fn a_history_over_a_cap_is_written_back_with_the_drawing_it_belongs_to() {
    let mut s = Session::with_sequential_ids();
    let mut first = Vec::new();
    for i in 0..12 {
        first.push(rect(&mut s, i * 20));
        create(&mut s, &format!("v{i}"));
    }
    // The moving drawing: edit, then one more version.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [first[0].clone(), first[1].clone()], "delta": [5, 5]}}),
    );
    create(&mut s, "moved");
    let expected: Vec<(u64, Value)> = (1..=13)
        .map(|id| (id, version_document(&mut s, id)))
        .collect();
    let file = through_the_file(&mut s);
    let original = file.history.clone().unwrap().to_value();

    // A host that reads at most 5 versions opens the file: the history is kept and read only.
    let mut t = open_with_cap(file, 5);
    assert!(
        t.load_notes()
            .iter()
            .any(|n| n.contains("only the newest 5"))
    );
    let l = ok(&mut t, "version.list", json!({}));
    assert_eq!(l["read_only"], true);
    assert!(
        t.review.opaque_base.is_some(),
        "the drawing it belongs to is kept"
    );

    // The drawing is edited and saved, several times. The versions are not touched.
    rect(&mut t, 1000);
    ok(
        &mut t,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [first[2].clone()]}}),
    );
    let saved = through_the_file(&mut t);
    let kept = saved.history.clone().unwrap().to_value();
    assert_eq!(kept["versions"], original["versions"]);
    assert!(
        kept.get("base").is_some(),
        "the way back to the old drawing"
    );
    let mut t = open_with_cap(saved, 5);
    rect(&mut t, 2000);
    let saved = through_the_file(&mut t);
    assert_eq!(
        saved.history.clone().unwrap().to_value()["versions"],
        original["versions"]
    );

    // The cap is raised and the file is read again: every version is there, exactly.
    let mut u = open_with_cap(saved, 100);
    assert!(u.load_notes().is_empty(), "{:?}", u.load_notes());
    assert_eq!(count(&mut u), 13);
    for (id, doc) in &expected {
        assert_eq!(&version_document(&mut u, *id), doc, "version {id}");
    }
    // And a restore of the oldest version gives its content back.
    ok(&mut u, "version.restore", json!({"id": 1, "now_ms": 5}));
    let now = ok(&mut u, "version.diff", json!({"from": 1, "to": "current"}));
    assert_eq!(now["totals"]["changed"], 0, "{now}");
    assert_eq!(now["totals"]["added"], 0);
    assert_eq!(now["totals"]["removed"], 0);
}

#[test]
fn a_saved_file_without_edits_is_written_as_it_was() {
    let mut s = Session::with_sequential_ids();
    for i in 0..6 {
        rect(&mut s, i * 20);
        create(&mut s, &format!("v{i}"));
    }
    let file = through_the_file(&mut s);
    let original = file.history.clone().unwrap().to_value();
    let mut t = open_with_cap(file, 3);
    let saved = through_the_file(&mut t);
    assert_eq!(saved.history.unwrap().to_value(), original);
}

#[test]
fn a_picture_that_the_edit_removed_is_kept_for_the_versions() {
    let mut s = Session::with_sequential_ids();
    rect(&mut s, 0);
    create(&mut s, "plain");
    let mut bytes = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    let mut seed = 5u32;
    for _ in 0..200_000 {
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
    rect(&mut s, 40);
    create(&mut s, "more");
    let expected: Vec<(u64, Value)> = (1..=3)
        .map(|id| (id, version_document(&mut s, id)))
        .collect();
    let file = through_the_file(&mut s);

    // Open with a cap of 2, delete the picture, save, and read it all again.
    let mut t = open_with_cap(file, 2);
    assert_eq!(ok(&mut t, "version.list", json!({}))["read_only"], true);
    ok(
        &mut t,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [under.clone()]}}),
    );
    let saved = through_the_file(&mut t);
    let h = saved.history.clone().unwrap();
    assert!(
        !h.blob_list().is_empty(),
        "the picture that the drawing lost goes into the table of the history"
    );
    let mut u = open_with_cap(saved, 100);
    assert!(u.load_notes().is_empty(), "{:?}", u.load_notes());
    for (id, doc) in &expected {
        assert_eq!(&version_document(&mut u, *id), doc, "version {id}");
    }
}

#[test]
fn a_history_of_a_drawing_that_another_program_changed_has_no_way_back() {
    // The versions were written against a drawing that the file no longer holds: there is nothing
    // to keep, and the save writes the history as it is (the known limit).
    let mut s = Session::with_sequential_ids();
    rect(&mut s, 0);
    create(&mut s, "One");
    rect(&mut s, 20);
    create(&mut s, "Two");
    let mut file = through_the_file(&mut s);
    // Another program added an object and did not update the history.
    let mut other = Session::with_sequential_ids();
    rect(&mut other, 0);
    rect(&mut other, 20);
    rect(&mut other, 40);
    file.objects = other.editor.doc().objects.clone();
    let mut t = Session::with_sequential_ids();
    t.replace_document(file);
    assert_eq!(ok(&mut t, "version.list", json!({}))["read_only"], true);
    assert!(t.review.opaque_base.is_none());
    let saved = through_the_file(&mut t);
    assert!(saved.history.is_some());
}
