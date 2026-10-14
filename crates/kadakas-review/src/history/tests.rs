use std::collections::BTreeMap;

use kadakas_core::command::Editor;
use proptest::prelude::*;
use serde_json::{Value, json};

use super::*;
use crate::testutil::{circle, cmd, editor, new_version, run, steps};

pub(super) fn limits(max_versions: usize, max_bytes: usize) -> Limits {
    Limits {
        max_versions,
        max_bytes,
    }
}

fn add(h: &mut History, ed: &Editor, name: &str) -> u64 {
    h.add(ed.doc(), new_version(name), Limits::default())
        .unwrap()
        .id
}

/// What a save and an open do: the file form, the stamped keys, and the reading.
pub(super) fn through_file(h: &History, doc: &Document) -> (Value, Loaded) {
    let tip = h.tip_state().unwrap().unwrap();
    let (value, state) = h.to_disk(doc, &tip);
    let mut saved = doc.clone();
    state.stamp_keys(&mut saved);
    saved.history = None;
    let saved = Document::from_json(&serde_json::to_string(&saved).unwrap()).unwrap();
    let value: Value = serde_json::from_str(&value.to_string()).unwrap();
    let loaded = History::load(&value, &saved);
    (value, loaded)
}

fn infos_of(h: &History) -> Vec<(u64, String)> {
    h.versions().map(|m| (m.id, m.name.clone())).collect()
}

#[test]
fn the_newest_version_is_the_snapshot_and_older_ones_are_small_reverse_deltas() {
    let mut ed = editor();
    for i in 0..40 {
        circle(&mut ed, f64::from(i));
    }
    let mut h = History::new();
    assert!(h.is_empty() && h.tip_state().unwrap().is_none());
    let one = add(&mut h, &ed, "Start");
    assert_eq!(one, 1);
    assert_eq!(h.delta_bytes(one), Some(0), "the newest has no delta");
    let first = ed.doc().objects.keys().next().copied().unwrap();
    ed.execute(&cmd(
        json!({"op": "move_objects", "ids": [first], "delta": [1.0, 0.0]}),
    ))
    .unwrap();
    let two = add(&mut h, &ed, "Moved one");
    // Now version 1 is the reverse delta: a few operations, not a copy of the drawing.
    let delta = h.delta_bytes(one).unwrap();
    assert!(delta > 0 && delta < 1500, "{delta}");
    assert_eq!(h.delta_bytes(two), Some(0));
    assert!(h.bytes() < 3000, "{}", h.bytes());
    // The newest is the drawing, and the first is the drawing as it was.
    assert_eq!(h.tip_state().unwrap().unwrap(), State::of(ed.doc(), None));
    assert_ne!(h.state_of(one).unwrap(), h.state_of(two).unwrap());
    let metas: Vec<&VersionMeta> = h.versions().collect();
    assert_eq!(metas[0].parents, Vec::<u64>::new());
    assert_eq!(metas[1].parents, vec![1]);
    // A third version with no change costs the metadata only.
    let three = add(&mut h, &ed, "Same");
    assert_eq!(h.delta_bytes(two), Some(0));
    assert_eq!(h.state_of(two).unwrap(), h.state_of(three).unwrap());
    assert!(h.document_is_tip(ed.doc()).unwrap());
    ed.execute(&cmd(json!({"op": "delete_objects", "ids": [first]})))
        .unwrap();
    assert!(!h.document_is_tip(ed.doc()).unwrap());
    assert!(!History::new().document_is_tip(ed.doc()).unwrap());
}

#[test]
fn every_version_keeps_its_state() {
    let mut ed = editor();
    let mut h = History::new();
    let mut want = BTreeMap::new();
    for i in 0..6 {
        circle(&mut ed, f64::from(i) * 3.0);
        let id = add(&mut h, &ed, &format!("v{i}"));
        want.insert(id, State::of(ed.doc(), None));
        assert_eq!(h.state_of(id).unwrap().len(), want[&id].len());
    }
    for (id, s) in &want {
        assert!(h.state_of(*id).unwrap().same_content(s), "version {id}");
    }
    assert_eq!(h.state_of(99), Err(ReviewError::NotFound(99)));
    assert!(h.state_at(99).is_err());
    assert_eq!(h.meta(3).unwrap().name, "v2");
    assert_eq!(h.index_of(3), Some(2));
    assert_eq!(h.versions().count(), 6);
    assert_eq!((h.branch(), h.head()), ("main", Some(6)));
}

#[test]
fn a_limit_refuses_the_version_and_changes_nothing() {
    let mut ed = editor();
    let mut h = History::new();
    for i in 0..3 {
        circle(&mut ed, f64::from(i));
        h.add(
            ed.doc(),
            new_version(&format!("v{i}")),
            limits(3, usize::MAX),
        )
        .unwrap();
    }
    let before = h.clone();
    circle(&mut ed, 50.0);
    let err = h
        .add(ed.doc(), new_version("one too many"), limits(3, usize::MAX))
        .unwrap_err();
    let ReviewError::LimitReached(info) = &err else {
        panic!("{err:?}")
    };
    assert_eq!(
        (info.kind, info.max, info.needed),
        (LimitKind::Versions, 3, 4)
    );
    assert_eq!(
        info.versions.iter().map(|v| v.id).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(
        info.versions
            .iter()
            .all(|v| v.bytes > 0 && !v.name.is_empty())
    );
    assert!(err.to_string().contains("nothing was deleted"), "{err}");
    assert_eq!(h, before, "the history is unchanged");
    // The same for the bytes.
    let used = h.bytes();
    let err = h
        .add(ed.doc(), new_version("big"), limits(100, used + 5))
        .unwrap_err();
    assert!(matches!(&err, ReviewError::LimitReached(i) if i.kind == LimitKind::Bytes));
    assert_eq!(h, before);
    // Deleting a chosen version makes room, and the save goes on.
    let mut h2 = h.clone();
    h2.remove_many(&[2]).unwrap();
    h2.add(ed.doc(), new_version("now it fits"), limits(3, usize::MAX))
        .unwrap();
    assert_eq!(infos_of(&h2).len(), 3);
    assert_eq!(
        History::new().add(ed.doc(), new_version("x"), limits(0, 10)),
        Err(ReviewError::Disabled)
    );
}

#[test]
fn names_and_texts_are_checked() {
    let ed = editor();
    let mut h = History::new();
    let bad = |mut n: NewVersion, f: &dyn Fn(&mut NewVersion)| {
        f(&mut n);
        n
    };
    for v in [
        new_version("   "),
        bad(new_version("a"), &|n| n.name = "x".repeat(NAME_MAX + 1)),
        bad(new_version("a"), &|n| {
            n.note = Some("x".repeat(NOTE_MAX + 1))
        }),
        bad(new_version("a"), &|n| n.author = "x".repeat(AUTHOR_MAX + 1)),
    ] {
        assert!(matches!(
            h.add(ed.doc(), v, Limits::default()),
            Err(ReviewError::Invalid(_))
        ));
    }
    assert!(h.is_empty());
    let mut v = new_version("  padded  ");
    v.note = Some("  ".into());
    let id = h.add(ed.doc(), v, Limits::default()).unwrap().id;
    let m = h.meta(id).unwrap();
    assert_eq!(m.name, "padded");
    assert_eq!(m.note, None);
}

fn five() -> (History, BTreeMap<u64, State>) {
    let mut ed = editor();
    let mut h = History::new();
    let mut want = BTreeMap::new();
    for i in 0..5 {
        circle(&mut ed, f64::from(i));
        let id = add(&mut h, &ed, &format!("v{i}"));
        want.insert(id, h.state_of(id).unwrap());
    }
    (h, want)
}

#[test]
fn deleting_any_version_keeps_all_the_others_exact() {
    for gone in 1..=5u64 {
        let (mut h, want) = five();
        assert_eq!(h.remove(gone), Ok(false));
        assert_eq!(h.len(), 4);
        assert!(h.meta(gone).is_none());
        for m in h.versions().map(|m| m.id).collect::<Vec<_>>() {
            assert_eq!(h.state_of(m).unwrap(), want[&m], "delete {gone}, check {m}");
        }
        // The versions after the deleted one name its first parent.
        if gone > 1 && gone < 5 {
            assert_eq!(h.meta(gone + 1).unwrap().parents, vec![gone - 1]);
        }
        if gone == 5 {
            assert_eq!(h.head(), Some(4));
        }
    }
    let (mut h, _) = five();
    assert_eq!(h.remove(77), Err(ReviewError::NotFound(77)));
    for id in 1..=4 {
        assert_eq!(h.remove(id), Ok(false));
    }
    assert_eq!(h.remove(5), Ok(true));
    assert!(h.is_empty() && h.bytes() == 0);
    // Several at once, and nothing happens if one is missing.
    let (mut h, want) = five();
    let before = h.clone();
    assert_eq!(h.remove_many(&[2, 99]), Err(ReviewError::NotFound(99)));
    assert_eq!(h, before);
    let gone = h.remove_many(&[5, 1, 3]).unwrap();
    assert_eq!(gone.iter().map(|v| v.id).collect::<Vec<_>>(), vec![1, 3, 5]);
    for m in [2u64, 4] {
        assert_eq!(h.state_of(m).unwrap(), want[&m]);
    }
}

#[test]
fn memory_and_file_forms_round_trip() {
    let mut ed = editor();
    let mut h = History::new();
    for i in 0..4u8 {
        circle(&mut ed, f64::from(i));
        let mut v = new_version(&format!("v{i}"));
        v.note = Some(format!("note {i}"));
        v.origin = if i % 2 == 0 {
            Origin::Agent
        } else {
            Origin::Script
        };
        v.auto = i == 3;
        v.time_ms = 1_700_000_000_000 + u64::from(i);
        v.base_revision = u64::from(i) * 7;
        h.add(ed.doc(), v, Limits::default()).unwrap();
    }
    // Move one so that the drawing differs from the newest version, and reorder.
    let first = *ed.doc().objects.keys().next().unwrap();
    ed.execute(&cmd(
        json!({"op": "arrange", "ids": [first], "how": "front"}),
    ))
    .unwrap();
    ed.execute(&cmd(
        json!({"op": "move_objects", "ids": [first], "delta": [4.0, 4.0]}),
    ))
    .unwrap();
    // The JSON of the open document is against the anchor, which the session keeps.
    let json = h.to_value();
    assert_eq!(json["format"], 2);
    assert!(json["newest"]["delta"].is_array() && json["newest"].get("snapshot").is_none());
    assert_eq!(json["newest"]["anchor"].as_str().unwrap().len(), 64);
    let text = serde_json::to_string(&json).unwrap();
    let known = [h.anchor_pair()];
    let loaded = History::load_with(&serde_json::from_str(&text).unwrap(), ed.doc(), &known);
    assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
    assert_eq!(loaded.history.unwrap(), h);
    // Without the anchor, the drawing must be the anchor: it is not.
    assert!(History::load(&json, ed.doc()).history.is_none());
    // The file form is relative to the drawing.
    let (value, loaded) = through_file(&h, ed.doc());
    assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
    let back = loaded.history.as_ref().unwrap();
    assert_eq!(back.infos(), h.infos());
    for m in h.versions() {
        assert_eq!(back.state_of(m.id).unwrap(), h.state_of(m.id).unwrap());
    }
    assert!(value["newest"].get("snapshot").is_none());
    assert!(value["newest"]["anchor"].as_str().unwrap().len() == 64);
    assert_eq!(value["versions"].as_array().unwrap().len(), 4);
    assert!(value["versions"][3].get("delta").is_none());
    assert_eq!(value["versions"][0]["via"], 2);
    // A history without versions is not stored: the field is left out. If it is there, it says so.
    let none = History::load(&History::new().to_value(), ed.doc());
    assert!(none.history.is_none() && !none.notes.is_empty());
}

#[test]
fn a_file_whose_drawing_changed_cannot_rebuild_the_newest_version() {
    let mut ed = editor();
    circle(&mut ed, 0.0);
    let mut h = History::new();
    add(&mut h, &ed, "One");
    circle(&mut ed, 9.0);
    add(&mut h, &ed, "Two");
    let tip = h.tip_state().unwrap().unwrap();
    let (value, state) = h.to_disk(ed.doc(), &tip);
    let mut saved = ed.doc().clone();
    state.stamp_keys(&mut saved);
    // Another program moved an object and kept the history.
    let first = *saved.objects.keys().next().unwrap();
    saved
        .objects
        .get_mut(&first)
        .unwrap()
        .shape
        .translate(kadakas_core::DVec2::new(1.0, 0.0));
    let l = History::load(&value, &saved);
    assert!(l.history.is_none());
    assert!(
        l.notes[0].contains("changed after the versions"),
        "{:?}",
        l.notes
    );
    // A file form without an anchor fails the same way.
    let mut no_anchor = value.clone();
    no_anchor["newest"]
        .as_object_mut()
        .unwrap()
        .remove("anchor");
    assert!(History::load(&no_anchor, ed.doc()).history.is_none());
}

/// Three versions. The JSON is against the drawing of the first one, which is returned: a file
/// that holds that drawing reads it.
fn sample() -> (Value, Document) {
    let mut ed = editor();
    let mut h = History::new();
    let mut anchor = None;
    for i in 0..3 {
        circle(&mut ed, f64::from(i));
        add(&mut h, &ed, &format!("v{i}"));
        anchor.get_or_insert_with(|| ed.doc().clone());
    }
    (h.to_value(), anchor.unwrap())
}

fn load(v: &Value, doc: &Document) -> Loaded {
    History::load(v, doc)
}

#[test]
fn a_history_that_cannot_be_read_is_reported() {
    let (_, doc) = sample();
    for bad in [
        json!(null),
        json!([1]),
        json!({}),
        json!({"format": 1}),
        json!({"format": 2}),
        json!({"format": 2, "newest": {"snapshot": 5}, "versions": [{"id": 1, "name": "a", "author": "b"}]}),
        json!({"format": 2, "newest": {"snapshot": [{"op": "nonsense"}]}, "versions": [{"id": 1, "name": "a", "author": "b"}]}),
        json!({"format": 2, "newest": {"snapshot": [{"op": "delete",
            "target": {"kind": "object", "id": "00000000-0000-0000-0000-000000000009"}}]},
            "versions": [{"id": 1, "name": "a", "author": "b"}]}),
        json!({"format": 2, "newest": {"snapshot": []}, "versions": 7}),
    ] {
        let l = load(&bad, &doc);
        assert!(l.history.is_none(), "{bad}");
        assert!(!l.notes.is_empty(), "{bad}");
    }
    let l = load(
        &json!({"format": 2, "newest": {"snapshot": []}, "versions": []}),
        &doc,
    );
    assert!(l.history.is_none() && !l.notes.is_empty());
}

#[test]
fn a_broken_delta_leaves_out_that_version_and_the_older_ones() {
    let (v, doc) = sample();
    let bad_op = json!([{"op": "delete",
        "target": {"kind": "object", "id": "00000000-0000-0000-0000-0000000000ff"}}]);
    // The deltas belong to the older versions: version 2 starts from version 3.
    let mut x = v.clone();
    x["versions"][1]["delta"] = bad_op.clone();
    let l = load(&x, &doc);
    assert_eq!(l.history.unwrap().len(), 1, "only the newest is left");
    assert!(
        l.notes.iter().any(|n| n.contains("left out")),
        "{:?}",
        l.notes
    );
    let mut x = v.clone();
    x["versions"][0]["delta"] = bad_op;
    assert_eq!(load(&x, &doc).history.unwrap().len(), 2);
    for broken in [json!(["x"]), json!({"a": 1})] {
        let mut x = v.clone();
        x["versions"][0]["delta"] = broken;
        assert_eq!(load(&x, &doc).history.unwrap().len(), 2);
    }
    let mut x = v.clone();
    x["versions"][1] = json!(5);
    assert_eq!(load(&x, &doc).history.unwrap().len(), 1);
    let mut x = v.clone();
    x["versions"][0]["delta"] = json!([{"op": "set", "prop": "id", "value": 1,
        "target": {"kind": "document", "id": "00000000-0000-0000-0000-000000000000"}}]);
    assert_eq!(load(&x, &doc).history.unwrap().len(), 2);
}

#[test]
fn texts_ids_parents_and_branches_are_repaired() {
    let (v, doc) = sample();
    let mut x = v.clone();
    x["versions"][0]["name"] = json!("x".repeat(500));
    x["versions"][1]["name"] = json!("   ");
    x["versions"][1]["note"] = json!("n".repeat(NOTE_MAX + 50));
    x["versions"][2]["id"] = x["versions"][0]["id"].clone();
    x["versions"][2]["author"] = json!("a".repeat(400));
    let l = load(&x, &doc);
    let h = l.history.unwrap();
    let metas: Vec<&VersionMeta> = h.versions().collect();
    assert_eq!(metas.len(), 3);
    assert_eq!(metas[0].name.chars().count(), NAME_MAX);
    assert_eq!(metas[1].name, "Version 2");
    assert_eq!(metas[1].note.as_ref().unwrap().chars().count(), NOTE_MAX);
    assert_eq!(metas[2].author.chars().count(), AUTHOR_MAX);
    let mut ids: Vec<u64> = metas.iter().map(|m| m.id).collect();
    ids.dedup();
    assert_eq!(ids.len(), 3, "the repeated ID got a new one");
    assert!(l.notes.iter().any(|n| n.contains("repaired")));
    let mut h = h;
    let ed = editor();
    let id = h
        .add(ed.doc(), new_version("n"), Limits::default())
        .unwrap()
        .id;
    assert!(!ids.contains(&id));

    // A merge keeps its second parent; the first parent is the version before.
    let mut x = v.clone();
    x["versions"][2]["parents"] = json!([2, 1]);
    let l = load(&x, &doc);
    assert!(l.notes.is_empty(), "{:?}", l.notes);
    assert_eq!(l.history.unwrap().meta(3).unwrap().parents, vec![2, 1]);
    let mut x = v.clone();
    x["versions"][2]["parents"] = json!([1, 77, 1, 2]);
    x["versions"][1]["parents"] = json!([9]);
    x["versions"][0]["parents"] = json!([5]);
    x["versions"][0]["branch"] = json!("   ");
    x["versions"][1]["branch"] = json!("b".repeat(300));
    x["head"] = json!(999);
    let l = load(&x, &doc);
    let repaired = l.history.unwrap();
    assert!(repaired.meta(1).unwrap().parents.is_empty());
    assert_eq!(repaired.meta(2).unwrap().parents, vec![1]);
    assert_eq!(repaired.meta(3).unwrap().parents, vec![2, 1]);
    assert_eq!(repaired.meta(1).unwrap().branch, "main");
    assert_eq!(repaired.meta(2).unwrap().branch.chars().count(), BRANCH_MAX);
    assert_eq!(repaired.head(), Some(3));
    assert!(l.notes.iter().any(|n| n.contains("parents")));
}

#[test]
fn too_many_versions_are_not_all_read_and_say_so() {
    let (v, doc) = sample();
    let first = v["versions"][0].clone();
    let many: Vec<Value> = (0..HARD_MAX_VERSIONS + 20)
        .map(|i| {
            let mut x = first.clone();
            x["id"] = json!(i + 1);
            x["parents"] = if i == 0 { json!([]) } else { json!([i]) };
            x.as_object_mut().unwrap().remove("delta");
            x
        })
        .collect();
    let mut x = v;
    x["versions"] = json!(many);
    let l = load(&x, &doc);
    assert_eq!(l.history.unwrap().len(), HARD_MAX_VERSIONS);
    assert!(
        l.notes.iter().any(|n| n.contains("only the newest")),
        "{:?}",
        l.notes
    );
}

#[test]
fn limits_are_clamped_and_errors_have_texts() {
    let l = limits(usize::MAX, usize::MAX).clamped();
    assert_eq!(l.max_versions, HARD_MAX_VERSIONS);
    assert_eq!(l.max_bytes, HARD_MAX_BYTES);
    assert_eq!(Limits::default().clamped(), Limits::default());
    for e in [
        ReviewError::NotFound(1),
        ReviewError::Invalid("x".into()),
        ReviewError::Disabled,
        ReviewError::Damaged("x".into()),
    ] {
        assert!(!e.to_string().is_empty());
    }
    assert!(matches!(
        ReviewError::from(ApplyError::Unsupported),
        ReviewError::Damaged(_)
    ));
}

/// A version made from version 2 while version 3 was made from it too: a branch.
fn with_branch(h: &mut History, from: u64, state: &State) -> u64 {
    let id = h.next_id;
    h.next_id += 1;
    // The delta of the branch starts from the state of `from`.
    let delta = h.state_of(from).unwrap().diff(state);
    let mut m = h.versions[0].meta.clone();
    m.id = id;
    m.name = "alt".into();
    m.parents = vec![from];
    m.branch = "alt".into();
    let bytes = ops_bytes(&delta) + meta_bytes(&m);
    h.versions.push(Version {
        meta: m,
        via: Some(from),
        delta,
        bytes,
        json: Memo::default(),
    });
    id
}

#[test]
fn a_delta_starts_from_the_version_it_names() {
    let (mut h, _) = five();
    let mut ed = editor();
    circle(&mut ed, 100.0);
    let other = State::of(ed.doc(), None);
    let alt = with_branch(&mut h, 2, &other);
    assert_eq!(h.state_of(alt).unwrap(), other);
    for id in 1..=5 {
        assert!(h.state_of(id).is_ok());
    }
    // Deleting version 2 joins the deltas: the branch and version 1 keep their content.
    let (before_alt, before_1) = (h.state_of(alt).unwrap(), h.state_of(1).unwrap());
    let mut gone = h.clone();
    gone.remove(2).unwrap();
    assert_eq!(gone.state_of(alt).unwrap(), before_alt);
    assert_eq!(gone.state_of(1).unwrap(), before_1);
    assert_eq!(gone.versions[gone.index_of(alt).unwrap()].via, Some(3));
    // Deleting the newest version: the version made from it becomes the newest.
    let mut cut = h.clone();
    let (s4, s3) = (h.state_of(4).unwrap(), h.state_of(3).unwrap());
    cut.remove(5).unwrap();
    assert_eq!(cut.newest, Some(4));
    assert_eq!(cut.state_of(4).unwrap(), s4);
    assert_eq!(cut.state_of(3).unwrap(), s3);
    assert_eq!(cut.state_of(alt).unwrap(), before_alt);
    // The head moves to the first parent when its version is deleted.
    assert_eq!(cut.head(), Some(4));
    // A chain that does not reach the newest version, or a cycle, cannot be rebuilt.
    let mut broken = h.clone();
    broken.versions[0].via = Some(42);
    assert!(broken.state_at(0).is_err());
    broken.versions[0].via = Some(1);
    assert!(broken.state_at(0).is_err(), "a cycle");
    broken.versions[0].via = None;
    assert!(broken.state_at(0).is_err(), "a second end");
}

/// Any JSON, nested a few levels.
fn any_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        "[a-z0-9-]{0,12}".prop_map(Value::String),
    ];
    leaf.prop_recursive(4, 48, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::btree_map("[a-z_]{1,8}", inner, 0..6)
                .prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

/// Replaces a random place of a JSON value with other JSON.
fn poke(v: &mut Value, path: &[usize], with: Value) {
    match v {
        Value::Array(a) if !a.is_empty() => {
            let i = path.first().copied().unwrap_or(0) % a.len();
            if path.len() <= 1 {
                a[i] = with;
            } else {
                poke(&mut a[i], &path[1..], with);
            }
        }
        Value::Object(m) if !m.is_empty() => {
            let i = path.first().copied().unwrap_or(0) % m.len();
            let key = m.keys().nth(i).cloned().unwrap();
            if path.len() <= 1 {
                m.insert(key, with);
            } else if let Some(x) = m.get_mut(&key) {
                poke(x, &path[1..], with);
            }
        }
        _ => *v = with,
    }
}

/// The versions with their states: what a person sees of a history.
fn fingerprint(h: &History) -> Vec<(VersionMeta, State)> {
    h.versions()
        .map(|m| (m.clone(), h.state_of(m.id).unwrap()))
        .collect()
}

proptest! {
    #[test]
    fn hostile_history_data_never_panics(v in any_json(), s in steps()) {
        let mut ed = editor();
        run(&mut ed, &s);
        let l = History::load(&v, ed.doc());
        if let Some(h) = l.history {
            for m in h.versions().map(|m| m.id).collect::<Vec<_>>() {
                prop_assert!(h.state_of(m).is_ok());
            }
        }
    }

    /// A history that was changed in one place reads as it was given, or it says what it changed.
    #[test]
    fn a_damaged_history_reads_the_same_or_gives_notes(
        s in steps(),
        path in prop::collection::vec(0usize..40, 1..6),
        with in any_json(),
        file_form in any::<bool>(),
    ) {
        let mut ed = editor();
        let mut h = History::new();
        for chunk in s.chunks(3) {
            run(&mut ed, chunk);
            h.add(ed.doc(), new_version("v"), Limits::default()).unwrap();
        }
        prop_assume!(!h.is_empty());
        let known = [h.anchor_pair()];
        let (mut v, mut doc) = (h.to_value(), ed.doc().clone());
        if file_form {
            let tip = h.tip_state().unwrap().unwrap();
            let (value, state) = h.to_disk(ed.doc(), &tip);
            state.stamp_keys(&mut doc);
            v = value;
        }
        poke(&mut v, &path, with);
        let l = History::load_with(&v, &doc, &known);
        match &l.history {
            Some(kept) => {
                prop_assert!(kept.len() <= HARD_MAX_VERSIONS);
                for m in kept.versions().map(|m| m.id).collect::<Vec<_>>() {
                    prop_assert!(kept.state_of(m).is_ok());
                }
                // If the reading is not the JSON that was given, the loader changed something
                // (repaired it or left a part out), and it must say so. A change that keeps the
                // JSON well formed (another value in a property) is not a repair.
                let given = if file_form {
                    let tip = kept.tip_state().unwrap().unwrap();
                    kept.to_disk(&doc, &tip).0
                } else {
                    kept.to_value()
                };
                if given != v {
                    prop_assert!(!l.notes.is_empty(), "changed without a note: {v}");
                }
                // What is read writes and reads again to the same history.
                let again = History::load_with(&kept.to_value(), &doc, &[kept.anchor_pair()]);
                prop_assert_eq!(again.history.as_ref(), Some(kept));
                prop_assert!(again.notes.is_empty());
            }
            None => prop_assert!(!l.notes.is_empty(), "the history vanished without a note"),
        }
    }

    #[test]
    fn the_limits_refuse_and_the_versions_that_stay_keep_their_state(
        edits in prop::collection::vec(steps(), 1..10),
        max_versions in 1usize..6,
        max_bytes in 3000usize..20000,
        deletes in prop::collection::vec(any::<u8>(), 0..4),
    ) {
        let mut ed = editor();
        let mut h = History::new();
        let mut want: BTreeMap<u64, State> = BTreeMap::new();
        let lim = limits(max_versions, max_bytes);
        for s in &edits {
            run(&mut ed, s);
            let before = h.clone();
            match h.add(ed.doc(), new_version("v"), lim) {
                Ok(added) => {
                    want.insert(added.id, State::of(ed.doc(), None));
                }
                Err(e) => {
                    let refused = matches!(e, ReviewError::LimitReached(_));
                    prop_assert!(refused);
                    prop_assert_eq!(&h, &before, "a refused save changes nothing");
                }
            }
            prop_assert!(h.len() <= max_versions);
            prop_assert!(h.bytes() <= max_bytes);
        }
        for d in deletes {
            let ids: Vec<u64> = h.versions().map(|m| m.id).collect();
            if ids.is_empty() {
                break;
            }
            let id = ids[d as usize % ids.len()];
            h.remove(id).unwrap();
            want.remove(&id);
        }
        let ids: Vec<u64> = h.versions().map(|m| m.id).collect();
        prop_assert_eq!(ids.clone(), want.keys().copied().collect::<Vec<_>>());
        for id in ids {
            prop_assert!(h.state_of(id).unwrap().same_content(&want[&id]));
        }
    }

    /// The file form: a save and an open give the same versions back, whatever the drawing did
    /// since the newest version.
    #[test]
    fn the_file_form_round_trips(first in steps(), second in steps(), third in steps()) {
        let mut ed = editor();
        let mut h = History::new();
        run(&mut ed, &first);
        add(&mut h, &ed, "one");
        run(&mut ed, &second);
        add(&mut h, &ed, "two");
        run(&mut ed, &third);
        let (_, l) = through_file(&h, ed.doc());
        prop_assert!(l.notes.is_empty(), "{:?}", l.notes);
        let read = l.history.unwrap();
        prop_assert_eq!(fingerprint(&read), fingerprint(&h));
    }
}
