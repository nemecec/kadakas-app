//! The cache of the parsed history, the lazy check and the load notes.

use serde_json::{Value, json};

use super::*;

fn ok(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method}: {} {}", e.code, e.message))
}

fn fail(s: &mut Session, method: &str, p: Value) -> RpcError {
    s.handle(method, p)
        .expect_err(&format!("{method} should fail"))
}

fn rect(s: &mut Session, x: i32) {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape":
            {"kind": "rect", "min": [x, 0], "max": [x + 10, 10]}}}),
    );
}

fn create(s: &mut Session, name: &str) {
    ok(s, "version.create", json!({"name": name, "now_ms": 1000}));
}

#[test]
fn a_refresh_does_not_parse_or_replay_the_history_again() {
    let mut s = Session::with_sequential_ids();
    for i in 0..6 {
        rect(&mut s, i * 20);
        create(&mut s, &format!("v{i}"));
    }
    let (parses, replays) = (s.review.parses, s.review.replays);
    // The window asks again and again while the drawing changes: one replay of the head, no parse.
    for i in 0..5 {
        rect(&mut s, 500 + i * 20);
        let l = ok(&mut s, "version.list", json!({}));
        assert_eq!(l["current_is_newest"], false);
    }
    assert_eq!(s.review.parses, parses, "the JSON is not parsed again");
    assert!(
        s.review.replays <= replays + 1,
        "{} replays",
        s.review.replays - replays
    );
    // A diff against the current drawing uses the cached head too.
    let before = s.review.replays;
    ok(&mut s, "version.diff", json!({"from": 1, "to": "current"}));
    ok(&mut s, "version.diff", json!({"from": 3, "to": "current"}));
    assert_eq!(s.review.replays, before);
    // `current: false` leaves out the comparison with the drawing.
    let l = ok(&mut s, "version.list", json!({"current": false}));
    assert!(l.get("current_is_newest").is_none());
    assert_eq!(l["count"], 6);
    assert_eq!(
        fail(&mut s, "version.list", json!({"current": 3})).code,
        -32602
    );
    // A new version makes a new history: one more replay, once.
    create(&mut s, "v6");
    ok(&mut s, "version.list", json!({}));
    ok(&mut s, "version.list", json!({}));
    assert_eq!(s.review.replays, before + 1);
}

#[test]
fn the_lazy_check_of_a_history_keeps_its_notes() {
    // A host that put a document into the editor itself (no check at load): the first read
    // checks the history and reports what it repaired.
    let mut donor = Session::with_sequential_ids();
    rect(&mut donor, 0);
    create(&mut donor, "One");
    let mut file = donor.editor.doc().history.as_ref().unwrap().value().clone();
    file["versions"][0]["name"] = json!("n".repeat(500));
    let mut s = Session::with_sequential_ids();
    s.editor.set_history(Some(HistoryData::new(file)));
    let l = ok(&mut s, "version.list", json!({}));
    assert!(l["notes"][0].as_str().unwrap().contains("repaired"), "{l}");
    assert_eq!(l["versions"][0]["name"].as_str().unwrap().len(), 120);
    assert_eq!(s.review.parses, 1);
    let l = ok(&mut s, "version.list", json!({}));
    assert!(l["notes"].is_array(), "the notes stay");
    assert_eq!(s.review.parses, 1);
    // A change of the versions writes the repaired history, and the notes are done.
    ok(&mut s, "version.create", json!({"name": "Two"}));
    assert!(ok(&mut s, "version.list", json!({})).get("notes").is_none());
}

#[test]
fn the_load_notes_of_a_session_come_from_the_file_check_and_the_history_check() {
    let mut donor = Session::with_sequential_ids();
    rect(&mut donor, 0);
    create(&mut donor, "One");
    let mut doc = donor.editor.doc().clone();
    doc.history = Some(HistoryData::new(json!({"format": 99})));
    let s = Session::with_document(doc);
    assert_eq!(s.load_notes().len(), 1);
    // The file check drops a history that is not an object; the notes of replace_document are the
    // notes of the session.
    let mut t = Session::with_sequential_ids();
    let mut file = serde_json::to_value(donor.editor.doc()).unwrap();
    file["history"] = json!("nonsense");
    let doc = Document::from_value(file).unwrap();
    let notes = t.replace_document(doc);
    assert_eq!(notes.len(), 1);
    assert_eq!(t.load_notes(), notes.as_slice());
}
