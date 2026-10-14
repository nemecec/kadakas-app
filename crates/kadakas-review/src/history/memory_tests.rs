//! The memory form of a history is made of parts that a newer value shares with an older one.

use serde_json::json;

use super::*;
use crate::testutil::{circle, cmd, editor, new_version};

/// A new history made from an older one shares the versions that did not change and the
/// operations of the change since the anchor: what the editor adds for it is small.
#[test]
fn a_new_version_adds_its_own_part_to_the_memory_form() {
    let mut ed = editor();
    for i in 0..60 {
        circle(&mut ed, f64::from(i));
    }
    let mut h = History::new();
    let mut last: Option<HistoryData> = None;
    for i in 0..20 {
        let first = *ed.doc().objects.keys().nth(i % 60).unwrap();
        ed.execute(&cmd(
            json!({"op": "move_objects", "ids": [first], "delta": [1.0, 0.0]}),
        ))
        .unwrap();
        h.add(ed.doc(), new_version(&format!("v{i}")), Limits::default())
            .unwrap();
        let data = h.to_data();
        if let Some(prev) = last.replace(data.clone())
            && i > 10
        {
            let added = data.bytes_not_in(Some(&prev));
            assert!(
                added * 3 < data.bytes(),
                "version {i}: {added} new bytes of {}",
                data.bytes()
            );
        }
    }
    let data = last.unwrap();
    // The memory form reads back to the same history, and writes the same text as the value.
    let again = History::load_with(&data.to_value(), ed.doc(), &[h.anchor_pair()]);
    assert!(again.notes.is_empty(), "{:?}", again.notes);
    assert_eq!(again.history.unwrap(), h);
    assert_eq!(
        serde_json::to_string(&data).unwrap(),
        h.to_value().to_string()
    );
}

/// A version that was deleted or joined gets a new JSON, the others keep theirs.
#[test]
fn a_changed_version_makes_new_json_and_an_equal_one_keeps_it() {
    let mut ed = editor();
    let mut h = History::new();
    for i in 0..5 {
        circle(&mut ed, f64::from(i));
        h.add(ed.doc(), new_version(&format!("v{i}")), Limits::default())
            .unwrap();
    }
    let a = h.to_data();
    let mut fewer = h.clone();
    fewer.remove(3).unwrap();
    let b = fewer.to_data();
    // The versions 1, 2 and 5 are the same memory in both; 4 was joined with its neighbours.
    let added = b.bytes_not_in(Some(&a));
    assert!(added < b.bytes(), "{added} of {}", b.bytes());
    assert_eq!(b.to_value(), fewer.to_value());
}
