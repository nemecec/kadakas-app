use kadakas_core::command::Editor;
use kadakas_core::history::HistoryData;
use proptest::prelude::*;
use serde_json::json;

use super::*;
use crate::testutil::{circle, cmd, editor, new_version, run, steps};

/// Applies a restore to the editor as the API does.
fn apply(ed: &mut Editor, r: &Restore) {
    ed.restore_content(&r.doc, Some(HistoryData::new(r.history.to_value())))
        .unwrap();
}

#[test]
fn restore_saves_the_current_state_first_and_goes_back() {
    let mut ed = editor();
    circle(&mut ed, 0.0);
    let mut h = History::new();
    let v1 = h
        .add(ed.doc(), new_version("one"), Limits::default())
        .unwrap()
        .id;
    let at_v1 = ed.doc().clone();
    let b = circle(&mut ed, 5.0);
    ed.execute(&cmd(json!({"op": "delete_objects", "ids": [b]})))
        .unwrap();
    circle(&mut ed, 9.0);
    let before = ed.doc().clone();
    let r = prepare(
        &h,
        ed.doc(),
        v1,
        new_version("Before restore"),
        Limits::default(),
    )
    .unwrap();
    let (saved, is_new) = r.saved.unwrap();
    assert!(is_new && r.notes.is_empty(), "{:?}", r.notes);
    assert_eq!(r.history.len(), 2);
    assert!(same_content(&r.doc, &at_v1));
    apply(&mut ed, &r);
    assert!(same_content(ed.doc(), &at_v1));
    // The history of the document is the new one, in the same undo step.
    let kept = History::load(ed.doc().history.as_ref().unwrap().value(), ed.doc());
    assert_eq!(kept.history.unwrap().len(), 2);
    // Restoring the automatic version brings the work back.
    let h2 = History::load(ed.doc().history.as_ref().unwrap().value(), ed.doc())
        .history
        .unwrap();
    let r2 = prepare(&h2, ed.doc(), saved, new_version("auto"), Limits::default()).unwrap();
    assert!(same_content(&r2.doc, &before));
    // Undo takes the whole restore back, the version too.
    assert!(ed.undo());
    assert_eq!(ed.doc(), &before);
    assert!(ed.doc().history.is_none());
}

#[test]
fn a_document_that_equals_the_newest_version_needs_no_new_one() {
    let mut ed = editor();
    circle(&mut ed, 0.0);
    let mut h = History::new();
    let v1 = h
        .add(ed.doc(), new_version("one"), Limits::default())
        .unwrap()
        .id;
    circle(&mut ed, 3.0);
    let v2 = h
        .add(ed.doc(), new_version("two"), Limits::default())
        .unwrap()
        .id;
    let r = prepare(&h, ed.doc(), v1, new_version("auto"), Limits::default()).unwrap();
    assert_eq!(r.saved, Some((v2, false)));
    assert_eq!(r.history.len(), 2);
    assert!(matches!(
        prepare(&h, ed.doc(), 99, new_version("auto"), Limits::default()),
        Err(ReviewError::NotFound(99))
    ));
}

#[test]
fn a_restore_whose_automatic_version_would_break_a_limit_is_refused() {
    let mut ed = editor();
    circle(&mut ed, 0.0);
    let mut h = History::new();
    let lim = Limits {
        max_versions: 1,
        max_bytes: usize::MAX,
    };
    let v1 = h.add(ed.doc(), new_version("one"), lim).unwrap().id;
    circle(&mut ed, 3.0);
    let before = h.clone();
    let err = prepare(&h, ed.doc(), v1, new_version("auto"), lim).unwrap_err();
    let ReviewError::LimitReached(info) = err else {
        panic!("a limit error")
    };
    assert_eq!(info.versions.len(), 1);
    assert_eq!(h, before, "nothing was deleted");
    // A person deletes nothing but raises the limit: the restore goes on.
    let lim2 = Limits {
        max_versions: 2,
        max_bytes: usize::MAX,
    };
    let r = prepare(&h, ed.doc(), v1, new_version("auto"), lim2).unwrap();
    assert_eq!(r.history.len(), 2);
}

#[test]
fn what_does_not_fit_is_left_out_and_reported() {
    let mut ed = editor();
    circle(&mut ed, 0.0);
    let mut h = History::new();
    h.add(ed.doc(), new_version("one"), Limits::default())
        .unwrap();
    let mut json = h.to_value();
    let mut base = json["newest"]["delta"].as_array().unwrap().clone();
    base.push(json!({"op": "create",
        "target": {"kind": "object", "id": "00000000-0000-0000-0000-0000000000aa"},
        "props": {"shape": 5, "layer": "00000000-0000-0000-0000-000000000001",
                  "class": "00000000-0000-0000-0000-000000000000"}}));
    json["newest"]["delta"] = json!(base);
    let h = History::load_with(&json, ed.doc(), &[h.anchor_pair()])
        .history
        .unwrap();
    let r = prepare(&h, ed.doc(), 1, new_version("auto"), Limits::default()).unwrap();
    assert!(
        r.notes.iter().any(|n| n.contains("left out")),
        "{:?}",
        r.notes
    );
    assert_eq!(r.doc.objects.len(), 1);
}

#[test]
fn the_order_of_objects_in_a_layer_is_restored() {
    let mut ed = editor();
    let ids: Vec<_> = (0..5).map(|i| circle(&mut ed, f64::from(i))).collect();
    let mut h = History::new();
    let v = h
        .add(ed.doc(), new_version("v"), Limits::default())
        .unwrap()
        .id;
    let order = |ed: &Editor| ed.doc().objects.keys().copied().collect::<Vec<_>>();
    let start = order(&ed);
    ed.execute(&cmd(
        json!({"op": "arrange", "ids": [ids[0]], "how": "front"}),
    ))
    .unwrap();
    ed.execute(&cmd(
        json!({"op": "arrange", "ids": [ids[4]], "how": "back"}),
    ))
    .unwrap();
    assert_ne!(order(&ed), start);
    let r = prepare(&h, ed.doc(), v, new_version("auto"), Limits::default()).unwrap();
    apply(&mut ed, &r);
    assert_eq!(order(&ed), start);
}

proptest! {
    /// Restore round-trips: after a restore the content is the version's, and a restore of the
    /// automatic version brings back the content from before. Undo gives the exact old document.
    #[test]
    fn restore_round_trips(first in steps(), second in steps(), third in steps()) {
        let mut ed = editor();
        run(&mut ed, &first);
        let mut h = History::new();
        let v1 = h.add(ed.doc(), new_version("one"), Limits::default()).unwrap().id;
        let at_v1 = ed.doc().clone();
        run(&mut ed, &second);
        ed.set_history(Some(HistoryData::new(h.to_value())));
        run(&mut ed, &third);
        let before = ed.doc().clone();
        let r = prepare(&h, ed.doc(), v1, new_version("auto"), Limits::default()).unwrap();
        prop_assert!(r.notes.is_empty(), "{:?}", r.notes);
        apply(&mut ed, &r);
        prop_assert!(same_content(ed.doc(), &at_v1));
        prop_assert!(r.history.len() <= 2);
        if let Some((auto, true)) = r.saved {
            let h2 = r.history.clone();
            let back = prepare(&h2, ed.doc(), auto, new_version("auto"), Limits::default()).unwrap();
            prop_assert!(same_content(&back.doc, &before));
        }
    }

    /// One undo step undoes the restore exactly.
    #[test]
    fn one_undo_undoes_the_restore(first in steps(), second in steps()) {
        let mut ed = editor();
        run(&mut ed, &first);
        let mut h = History::new();
        let v1 = h.add(ed.doc(), new_version("one"), Limits::default()).unwrap().id;
        run(&mut ed, &second);
        let before = ed.doc().clone();
        let depth = ed.undo_depth();
        let r = prepare(&h, ed.doc(), v1, new_version("auto"), Limits::default()).unwrap();
        apply(&mut ed, &r);
        prop_assert!(ed.undo_depth() <= depth + 1);
        if ed.undo_depth() == depth + 1 {
            prop_assert!(ed.undo());
            prop_assert_eq!(ed.doc(), &before);
        }
    }
}

#[test]
fn restoring_equal_content_changes_nothing_even_with_layers_interleaved() {
    let mut ed = editor();
    ed.execute(&cmd(json!({"op": "add_layer", "name": "B"})))
        .unwrap();
    let layers: Vec<_> = ed.doc().layers.iter().map(|l| l.id).collect();
    for (i, layer) in [0usize, 1, 1, 0, 1, 0, 0, 1].iter().enumerate() {
        ed.execute(&cmd(json!({"op": "add_object", "layer": layers[*layer],
            "shape": {"kind": "circle", "center": [i as f64 * 5.0, 0.0], "radius": 1.0}})))
            .unwrap();
    }
    let mut h = History::new();
    h.add(ed.doc(), new_version("one"), Limits::default())
        .unwrap();
    ed.set_history(Some(HistoryData::new(h.to_value())));
    let before = ed.doc().clone();
    let (rev, depth) = (ed.revision(), ed.undo_depth());
    let r = prepare(&h, ed.doc(), 1, new_version("auto"), Limits::default()).unwrap();
    assert_eq!(r.saved, Some((1, false)));
    assert_eq!(
        r.doc.objects.keys().collect::<Vec<_>>(),
        before.objects.keys().collect::<Vec<_>>(),
        "the interleave of the layers is kept"
    );
    let changed = ed
        .restore_content(&r.doc, Some(HistoryData::new(r.history.to_value())))
        .unwrap();
    assert!(!changed);
    assert_eq!((ed.revision(), ed.undo_depth()), (rev, depth));
    assert!(ed.changes_since(rev).changes.is_empty());
    assert_eq!(ed.doc(), &before);

    // A change inside one layer: the objects of the other layer stay where they are.
    let first_b = ed
        .doc()
        .objects
        .values()
        .find(|o| o.layer == layers[1])
        .unwrap()
        .id;
    ed.execute(&cmd(
        json!({"op": "arrange", "ids": [first_b], "how": "front"}),
    ))
    .unwrap();
    let moved = ed.doc().clone();
    let r = prepare(&h, ed.doc(), 1, new_version("auto"), Limits::default()).unwrap();
    apply(&mut ed, &r);
    assert!(same_content(ed.doc(), &before));
    let of_layer = |d: &Document, l| {
        d.objects
            .values()
            .filter(|o| o.layer == l)
            .map(|o| o.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(of_layer(ed.doc(), layers[0]), of_layer(&moved, layers[0]));
    assert!(ed.undo());
    assert_eq!(ed.doc(), &moved);
}
