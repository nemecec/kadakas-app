//! A history that a reader keeps as it is: the read caps, and the way back from an edited
//! drawing to the drawing that the versions were written against (the `base` entry).

use kadakas_core::command::Editor;
use serde_json::{Value, json};

use super::*;
use crate::testutil::{circle, cmd, editor, new_version};

/// Three versions of a drawing; returns the history, the drawing of the first version (the
/// anchor), and the editor with the newest drawing.
fn three() -> (History, Document, Editor) {
    let mut ed = editor();
    let mut h = History::new();
    let mut first = None;
    for i in 0..3 {
        circle(&mut ed, f64::from(i));
        h.add(ed.doc(), new_version(&format!("v{i}")), Limits::default())
            .unwrap();
        first.get_or_insert_with(|| ed.doc().clone());
    }
    (h, first.unwrap(), ed)
}

#[test]
fn a_cap_on_the_versions_keeps_the_newest_and_says_so() {
    let (h, anchor, _) = three();
    let caps = ReadCaps {
        max_versions: 2,
        max_bytes: HARD_MAX_BYTES,
    };
    let l = History::load_with_caps(&h.to_value(), &anchor, &[], caps);
    assert!(
        l.notes.iter().any(|n| n.contains("only the newest 2")),
        "{:?}",
        l.notes
    );
    assert_eq!(l.history.unwrap().len(), 2);
    let tiny = ReadCaps {
        max_versions: 100,
        max_bytes: 10,
    };
    let l = History::load_with_caps(&h.to_value(), &anchor, &[], tiny);
    assert!(!l.notes.is_empty());
    let l = History::load_with_caps(&h.to_value(), &anchor, &[], ReadCaps::default());
    assert!(l.notes.is_empty());
    assert_eq!(ReadCaps::default().max_versions, HARD_MAX_VERSIONS);
}

/// The JSON of a history with a `base` for an edited drawing, as a save writes it.
fn with_base(h: &History, anchor: &Document, edited: &mut Document) -> Value {
    let data = h.to_data();
    let written = written_against(data.top(), data.blob_list(), anchor).expect("the anchor");
    let now = State::of(edited, Some(&written));
    let (entry, blobs) = rebase_for_save(&now, &written);
    now.stamp_keys(edited);
    data.with_entry("base", entry, blobs, &[]).to_value()
}

#[test]
fn an_edited_drawing_gets_the_way_back_to_the_drawing_of_the_versions() {
    let (h, anchor, ed) = three();
    let mut edited = ed.doc().clone();
    // Edit: one object more and the first one gone.
    let mut e = Editor::new(edited);
    circle(&mut e, 9.0);
    let first = *e.doc().objects.keys().next().unwrap();
    e.execute(&cmd(json!({"op": "delete_objects", "ids": [first]})))
        .unwrap();
    edited = e.doc().clone();
    let value = with_base(&h, &anchor, &mut edited);
    assert!(
        value["base"]["delta"]
            .as_array()
            .is_some_and(|d| !d.is_empty())
    );
    // Without the base, the edited drawing is not the anchor.
    let mut bare = value.clone();
    bare.as_object_mut().unwrap().remove("base");
    assert!(History::load(&bare, &edited).history.is_none());
    // With it, every version is back, exactly.
    let l = History::load(&value, &edited);
    assert!(l.notes.is_empty(), "{:?}", l.notes);
    let back = l.history.unwrap();
    for m in h.versions() {
        assert_eq!(back.state_of(m.id).unwrap(), h.state_of(m.id).unwrap());
    }
    // A second edit and save: the base is made again from the drawing that the history belongs to.
    let (value2, mut edited2) = {
        let mut e2 = Editor::new(edited.clone());
        circle(&mut e2, 11.0);
        let mut d = e2.doc().clone();
        let data = kadakas_core::history::HistoryData::new(value.clone());
        let written = written_against(data.top(), data.blob_list(), &edited).unwrap();
        let now = State::of(&d, Some(&written));
        let (entry, blobs) = rebase_for_save(&now, &written);
        now.stamp_keys(&mut d);
        (data.with_entry("base", entry, blobs, &[]).to_value(), d)
    };
    let l = History::load(&value2, &edited2);
    assert!(l.notes.is_empty(), "{:?}", l.notes);
    assert_eq!(l.history.unwrap().len(), 3);
    // No change: no base.
    let same = with_base(&h, &anchor, &mut anchor.clone());
    assert!(same.get("base").is_none());
    edited2.history = None;
}

#[test]
fn a_base_that_does_not_fit_is_a_note_and_not_a_guess() {
    let (h, anchor, ed) = three();
    let mut edited = ed.doc().clone();
    let mut e = Editor::new(edited);
    circle(&mut e, 9.0);
    edited = e.doc().clone();
    let value = with_base(&h, &anchor, &mut edited);
    let mutate = |f: &dyn Fn(&mut Value)| {
        let mut v = value.clone();
        f(&mut v);
        let l = History::load(&v, &edited);
        assert!(l.history.is_none(), "{v}");
        assert!(!l.notes.is_empty());
        l.notes
    };
    // The base names another drawing.
    mutate(&|v| v["base"]["anchor"] = json!("00"));
    // The base is not an object, or its changes are not a list, or do not apply.
    mutate(&|v| v["base"] = json!(5));
    mutate(&|v| v["base"]["delta"] = json!("x"));
    mutate(&|v| {
        v["base"]["delta"] = json!([{"op": "delete",
        "target": {"kind": "object", "id": "00000000-0000-0000-0000-0000000000aa"}}])
    });
    // The changes lead to another drawing than the one that the history names.
    mutate(&|v| v["base"]["delta"] = json!([]));
    // A picture that the changes need is not in the table.
    let notes = mutate(&|v| {
        v["base"]["delta"] = json!([{"op": "create",
            "target": {"kind": "object", "id": "00000000-0000-0000-0000-0000000000bb"},
            "props": {"shape": {"kind": "underlay", "png": {"$blob": "ab".repeat(32)}}}}]);
    });
    assert!(notes.iter().any(|n| n.contains("missing")), "{notes:?}");
}

#[test]
fn a_history_without_a_matching_drawing_has_no_written_against() {
    let (h, _, ed) = three();
    let data = h.to_data();
    // The drawing is not the anchor and there is no base: nothing to keep.
    assert!(written_against(data.top(), data.blob_list(), ed.doc()).is_none());
    // Nonsense in the history.
    assert!(written_against(&json!({}), &[], ed.doc()).is_none());
    assert!(written_against(&json!({"newest": {"anchor": 5}}), &[], ed.doc()).is_none());
}
