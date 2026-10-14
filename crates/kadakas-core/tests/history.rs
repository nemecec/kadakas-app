//! The history field of a document (REV-01): it loads and saves, a hostile value is dropped with
//! a note, a change of it is one undo step, and `restore_content` makes one undo step that puts
//! the content of another document in place.

use kadakas_core::change::Origin;
use kadakas_core::command::{Command, Editor};
use kadakas_core::history::HistoryData;
use kadakas_core::model::{Document, ObjectId};
use proptest::prelude::*;
use serde_json::{Value, json};

fn cmd(v: Value) -> Command {
    serde_json::from_value(v).unwrap()
}

fn add(ed: &mut Editor, x: f64) -> ObjectId {
    let out = ed
        .execute(&cmd(json!({
            "op": "add_object",
            "shape": {"kind": "circle", "center": [x, 0.0], "radius": 2.0}
        })))
        .unwrap();
    ObjectId(out.created[0])
}

#[test]
fn a_file_without_history_loads_and_one_with_history_keeps_it() {
    let doc = Document::with_sequential_ids();
    let mut v = serde_json::to_value(&doc).unwrap();
    assert!(v.get("history").is_none());
    assert!(Document::from_value(v.clone()).unwrap().history.is_none());
    v["history"] = json!({"format": 1, "versions": []});
    let loaded = Document::from_value(v.clone()).unwrap();
    assert_eq!(&loaded.history.as_ref().unwrap().to_value(), &v["history"]);
    assert!(loaded.load_notes().is_empty());
    assert_eq!(
        serde_json::to_value(&loaded).unwrap()["history"],
        v["history"]
    );
}

#[test]
fn a_history_that_is_not_an_object_is_kept_unchanged_with_a_note() {
    let mut v = serde_json::to_value(Document::with_sequential_ids()).unwrap();
    for bad in [json!([1, 2]), json!("text"), json!(5)] {
        v["history"] = bad.clone();
        let loaded = Document::from_value(v.clone()).unwrap();
        // Nothing is thrown away: the person decides about the versions.
        assert_eq!(&loaded.history.as_ref().unwrap().to_value(), &bad);
        assert_eq!(loaded.load_notes().len(), 1);
        assert!(loaded.load_notes()[0].contains("read only"));
        assert_eq!(serde_json::to_value(&loaded).unwrap()["history"], bad);
    }
    v["history"] = Value::Null;
    assert!(Document::from_value(v).unwrap().history.is_none());
}

#[test]
fn a_change_of_the_history_is_one_undo_step() {
    let mut ed = Editor::with_sequential_ids();
    let rev = ed.revision();
    let h = HistoryData::new(json!({"format": 1}));
    assert!(ed.set_history(Some(h.clone())));
    assert!(
        !ed.set_history(Some(h.clone())),
        "the same value is no change"
    );
    assert_eq!(ed.undo_depth(), 1);
    let cs = ed
        .changes_since(rev)
        .changes
        .last()
        .cloned()
        .unwrap()
        .clone();
    assert!(cs.document.history && cs.touches("document"));
    assert_eq!(cs.op, "set_history");
    assert!(ed.undo());
    assert!(ed.doc().history.is_none());
    assert!(ed.redo());
    assert_eq!(ed.doc().history.as_ref(), Some(&h));
    assert!(ed.set_history(None));
    assert!(ed.doc().history.is_none());
}

#[test]
fn history_counts_in_the_size_of_the_undo_steps() {
    let mut ed = Editor::with_sequential_ids();
    let before = ed.history_bytes();
    let h = HistoryData::new(json!({"blob": "x".repeat(10_000)}));
    ed.set_history(Some(h));
    assert!(ed.history_bytes() >= before + 10_000);
}

#[test]
fn the_history_stays_when_a_remote_document_arrives() {
    let mut ed = Editor::with_sequential_ids();
    ed.set_history(Some(HistoryData::new(json!({"format": 1}))));
    let mut other = ed.doc().clone();
    other.history = None;
    assert!(
        ed.apply_remote(other).is_none(),
        "only the history differed"
    );
    assert!(ed.doc().history.is_some());
}

#[test]
fn restore_content_is_one_undo_step_and_goes_back() {
    let mut ed = Editor::with_sequential_ids();
    let a = add(&mut ed, 0.0);
    let b = add(&mut ed, 10.0);
    let target = ed.doc().clone();
    // Change a lot: move, delete, add, reorder, a new layer.
    ed.execute(&cmd(
        json!({"op": "move_objects", "ids": [a], "delta": [5.0, 0.0]}),
    ))
    .unwrap();
    ed.execute(&cmd(json!({"op": "delete_objects", "ids": [b]})))
        .unwrap();
    let c = add(&mut ed, 30.0);
    ed.execute(&cmd(json!({"op": "add_layer", "name": "Plan B"})))
        .unwrap();
    ed.execute(&cmd(json!({"op": "arrange", "ids": [c], "how": "back"})))
        .unwrap();
    let changed = ed.doc().clone();
    assert_ne!(changed, target);
    let depth = ed.undo_depth();
    let rev = ed.revision();
    ed.set_origin(Origin::Agent);
    let history = HistoryData::new(json!({"format": 1, "n": 1}));
    assert!(ed.restore_content(&target, Some(history.clone())).unwrap());
    assert_eq!(ed.undo_depth(), depth + 1);
    let mut expected = target.clone();
    expected.history = Some(history);
    assert_eq!(ed.doc(), &expected);
    let cs = ed
        .changes_since(rev)
        .changes
        .last()
        .cloned()
        .unwrap()
        .clone();
    assert_eq!(cs.op, "restore");
    assert_eq!(cs.origin, Origin::Agent);
    assert!(cs.removed.objects.contains(&c));
    assert!(cs.added.objects.contains(&b));
    assert!(cs.added.layers.is_empty() && cs.removed.layers.len() == 1);
    // Nothing to do the second time.
    assert!(
        !ed.restore_content(&target, expected.history.clone())
            .unwrap()
    );
    // Undo gives the exact document before, redo the restored one.
    assert!(ed.undo());
    assert_eq!(ed.doc(), &changed);
    assert!(ed.redo());
    assert_eq!(ed.doc(), &expected);
}

/// A few steps of random editing.
#[derive(Clone, Debug)]
enum Step {
    Add(i8),
    Delete(u8),
    Move(u8, i8),
    Back(u8),
    Front(u8),
    Layer,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        3 => any::<i8>().prop_map(Step::Add),
        2 => any::<u8>().prop_map(Step::Delete),
        2 => (any::<u8>(), any::<i8>()).prop_map(|(i, d)| Step::Move(i, d)),
        1 => any::<u8>().prop_map(Step::Back),
        1 => any::<u8>().prop_map(Step::Front),
        1 => Just(Step::Layer),
    ]
}

fn run(ed: &mut Editor, steps: &[Step]) {
    for s in steps {
        let ids: Vec<ObjectId> = ed.doc().objects.keys().copied().collect();
        let pick = |i: &u8| (!ids.is_empty()).then(|| ids[*i as usize % ids.len()]);
        let c = match s {
            Step::Add(x) => Some(json!({"op": "add_object", "shape": {"kind": "circle",
                "center": [f64::from(*x), 1.0], "radius": 2.0}})),
            Step::Delete(i) => pick(i).map(|id| json!({"op": "delete_objects", "ids": [id]})),
            Step::Move(i, d) => pick(i).map(
                |id| json!({"op": "move_objects", "ids": [id], "delta": [f64::from(*d), 0.0]}),
            ),
            Step::Back(i) => pick(i).map(|id| json!({"op": "arrange", "ids": [id], "how": "back"})),
            Step::Front(i) => {
                pick(i).map(|id| json!({"op": "arrange", "ids": [id], "how": "front"}))
            }
            Step::Layer => Some(json!({"op": "add_layer", "name": "L"})),
        };
        if let Some(c) = c {
            let _ = ed.execute(&cmd(c));
        }
    }
}

proptest! {
    #[test]
    fn restore_content_gives_the_target_and_undo_gives_the_start(
        first in prop::collection::vec(step(), 0..12),
        second in prop::collection::vec(step(), 0..12),
    ) {
        let mut ed = Editor::with_sequential_ids();
        run(&mut ed, &first);
        let target = ed.doc().clone();
        run(&mut ed, &second);
        let start = ed.doc().clone();
        let depth = ed.undo_depth();
        let changed = ed.restore_content(&target, None).unwrap();
        prop_assert_eq!(changed, start != target);
        prop_assert_eq!(ed.doc(), &target);
        prop_assert_eq!(ed.undo_depth(), depth + usize::from(changed));
        if changed {
            prop_assert!(ed.undo());
            prop_assert_eq!(ed.doc(), &start);
            prop_assert!(ed.redo());
            prop_assert_eq!(ed.doc(), &target);
        }
    }
}

#[test]
fn a_big_history_does_not_shrink_the_undo_stack_of_other_edits() {
    // Strict limits: 64 MiB of undo steps. A 16 MiB history sits in the document.
    let mut ed = Editor::with_sequential_ids();
    ed.set_history_limit(64 * 1024 * 1024);
    let big = HistoryData::new(json!({"blob": "x".repeat(16 * 1024 * 1024)}));
    ed.set_history(Some(big));
    let after_history = ed.history_bytes();
    assert!(
        (16 * 1024 * 1024..20 * 1024 * 1024).contains(&after_history),
        "the new value counts once: {after_history}"
    );
    for i in 0..100 {
        ed.execute(&cmd(json!({"op": "add_layer", "name": format!("L{i}")})))
            .unwrap();
        ed.execute(&cmd(json!({"op": "add_class", "name": format!("C{i}")})))
            .unwrap();
    }
    assert!(
        ed.undo_depth() >= 200,
        "layer and class edits stay in the undo stack: {}",
        ed.undo_depth()
    );
    // Changing the history again counts the new value only.
    let again = ed.history_bytes();
    ed.set_history(Some(HistoryData::new(
        json!({"blob": "y".repeat(1024 * 1024)}),
    )));
    assert!(ed.history_bytes() < again + 2 * 1024 * 1024);
}

#[test]
fn a_step_of_the_history_can_say_what_it_did() {
    let mut ed = Editor::with_sequential_ids();
    let rev = ed.revision();
    let detail = json!({"deleted": [{"id": 3, "name": "Old"}]});
    assert!(ed.set_history_with(
        Some(HistoryData::new(json!({"format": 2}))),
        "version_delete",
        Some(detail.clone())
    ));
    let cs = ed
        .changes_since(rev)
        .changes
        .last()
        .cloned()
        .unwrap()
        .clone();
    assert_eq!(cs.op, "version_delete");
    assert_eq!(cs.detail, Some(detail));
    assert!(cs.document.history);
    // The undo of the step has no detail.
    assert!(ed.undo());
    let cs = ed
        .changes_since(rev)
        .changes
        .last()
        .cloned()
        .unwrap()
        .clone();
    assert_eq!(cs.op, "undo");
    assert_eq!(cs.detail, None);
    // A restore too, and the detail is part of the JSON of the event.
    let target = ed.doc().clone();
    ed.execute(&cmd(json!({"op": "add_layer", "name": "X"})))
        .unwrap();
    let rev = ed.revision();
    ed.restore_content_with(
        &target,
        None,
        "version_restore",
        Some(json!({"restored": 1})),
    )
    .unwrap();
    let cs = ed
        .changes_since(rev - 1)
        .changes
        .last()
        .cloned()
        .unwrap()
        .clone();
    assert_eq!(cs.op, "version_restore");
    let text = serde_json::to_value(&cs).unwrap();
    assert_eq!(text["detail"], json!({"restored": 1}));
    // A change set without a detail has no field.
    ed.execute(&cmd(json!({"op": "add_layer", "name": "Y"})))
        .unwrap();
    let cs = ed.changes_since(0).changes.last().cloned().unwrap().clone();
    assert!(serde_json::to_value(&cs).unwrap().get("detail").is_none());
}
