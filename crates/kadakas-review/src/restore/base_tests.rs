//! The source data of an import (IOP-22) in versions: the import base is a resource whose
//! summaries are a blob of JSON, kept once by hash like a picture.

use serde_json::{Value, json};

use super::*;
use crate::testutil::{circle, cmd, editor, new_version};

/// The body of one part of an import base, with `n` summaries of about 1 KB each.
fn base_body(revision: &str, n: usize) -> Value {
    let recs: Vec<Value> = (0..n)
        .map(|i| {
            json!({"kind": "LINE", "handle": format!("{i:X}{revision}"), "layer": "Walls",
                   "text": "t".repeat(1000), "points": [[0.0, 0.0], [1.0, 1.0]]})
        })
        .collect();
    json!({"type": "import_base", "drawing": "guid:{A}", "revision": revision,
           "part": 0, "parts": 1, "recs": recs})
}

#[test]
fn a_big_import_base_is_one_blob_and_every_version_gives_it_back() {
    let mut ed = editor();
    circle(&mut ed, 0.0);
    let mut h = History::new();
    let v1 = h
        .add(ed.doc(), new_version("before"), Limits::default())
        .unwrap()
        .id;
    let at_v1 = ed.doc().clone();
    // The importer adds the base (about 600 KB of summaries) and a resource that is small.
    ed.execute(&cmd(
        json!({"op": "add_resource", "name": "import base r1", "body": base_body("r1", 600)}),
    ))
    .unwrap();
    let v2 = h
        .add(ed.doc(), new_version("imported"), Limits::default())
        .unwrap()
        .id;
    let at_v2 = ed.doc().clone();
    // Two more versions that change the drawing and not the base: the text is kept once.
    circle(&mut ed, 5.0);
    let v3 = h
        .add(ed.doc(), new_version("edited"), Limits::default())
        .unwrap()
        .id;
    circle(&mut ed, 9.0);
    h.add(ed.doc(), new_version("edited again"), Limits::default())
        .unwrap();
    assert_eq!(h.blobs().len(), 1, "one blob for the base");
    assert!(h.bytes() > 500_000 && h.bytes() < 800_000, "{}", h.bytes());
    for (id, doc) in [(v1, &at_v1), (v2, &at_v2)] {
        let r = prepare(&h, ed.doc(), id, new_version("auto"), Limits::default()).unwrap();
        assert!(r.notes.is_empty(), "{:?}", r.notes);
        assert!(same_content(&r.doc, doc), "version {id}");
    }
    // The base is a list again in the restored document, as the importer made it.
    let built = h.state_of(v3).unwrap().build(ed.doc());
    assert!(built.quarantined.is_empty(), "{:?}", built.quarantined);
    let body = built
        .doc
        .resources
        .iter()
        .find_map(|r| match &r.body {
            kadakas_core::resource::ResourceBody::ImportBase(v) => Some((**v).clone()),
            _ => None,
        })
        .expect("the base");
    assert_eq!(body, base_body("r1", 600));
    // The JSON form of the history has the base as a picture of the table, not in a delta.
    let json = h.to_value();
    assert_eq!(json["blobs"].as_object().unwrap().len(), 1);
    let again = History::load_with(&json, ed.doc(), &[h.anchor_pair()]);
    assert!(again.notes.is_empty(), "{:?}", again.notes);
    assert_eq!(again.history.unwrap(), h);
}

#[test]
fn a_changed_base_is_a_new_blob_and_a_deleted_base_frees_the_old_one() {
    let mut ed = editor();
    circle(&mut ed, 0.0);
    let mut h = History::new();
    h.add(ed.doc(), new_version("start"), Limits::default())
        .unwrap();
    ed.execute(&cmd(
        json!({"op": "add_resource", "name": "import base r1", "body": base_body("r1", 300)}),
    ))
    .unwrap();
    h.add(ed.doc(), new_version("r1"), Limits::default())
        .unwrap();
    let id = ed.doc().resources.iter().last().unwrap().id;
    ed.execute(&cmd(
        json!({"op": "modify_resource", "id": id, "body": base_body("r2", 300)}),
    ))
    .unwrap();
    h.add(ed.doc(), new_version("r2"), Limits::default())
        .unwrap();
    assert_eq!(
        h.blobs().len(),
        2,
        "each revision of the base is its own text"
    );
    h.remove(2).unwrap();
    assert_eq!(h.blobs().len(), 1, "the base that no version uses is gone");
}
