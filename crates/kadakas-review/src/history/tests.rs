use std::collections::BTreeMap;

use kadakas_core::command::Editor;
use proptest::prelude::*;
use serde_json::{Value, json};

use super::*;
use crate::testutil::{circle, cmd, editor, new_version, run, steps};

fn limits(max_versions: usize, max_bytes: usize) -> Limits {
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

#[test]
fn the_first_version_is_the_base_and_later_ones_are_small_deltas() {
    let mut ed = editor();
    for i in 0..40 {
        circle(&mut ed, f64::from(i));
    }
    let mut h = History::new();
    assert!(h.is_empty() && h.tip_state().unwrap().is_none());
    let one = add(&mut h, &ed, "Start");
    assert_eq!(one, 1);
    assert_eq!(h.delta_bytes(one), Some(0));
    let base_bytes = h.bytes();
    let first = ed.doc().objects.keys().next().copied().unwrap();
    ed.execute(&cmd(
        json!({"op": "move_objects", "ids": [first], "delta": [1.0, 0.0]}),
    ))
    .unwrap();
    let two = add(&mut h, &ed, "Moved one");
    assert_eq!(h.len(), 2);
    let delta = h.delta_bytes(two).unwrap();
    assert!(
        delta > 0 && delta * 10 < base_bytes,
        "{delta} vs {base_bytes}"
    );
    assert!(h.bytes() < base_bytes + base_bytes / 5);
    // A third version with no change costs the metadata only.
    let three = add(&mut h, &ed, "Same");
    assert_eq!(h.delta_bytes(three), Some(0));
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
        want.insert(id, h.state_of(id).unwrap());
        assert_eq!(h.state_of(id).unwrap(), State::of(ed.doc(), None));
    }
    for (id, s) in &want {
        assert_eq!(&h.state_of(*id).unwrap(), s);
    }
    assert_eq!(h.state_of(99), Err(ReviewError::NotFound(99)));
    assert!(h.state_at(99).is_err());
    assert_eq!(
        h.state_of_document(ed.doc()).unwrap(),
        h.tip_state().unwrap().unwrap()
    );
    assert_eq!(h.meta(3).unwrap().name, "v2");
    assert_eq!(h.index_of(3), Some(2));
    assert_eq!(h.versions().count(), 6);
}

#[test]
fn the_caps_fold_the_oldest_versions_into_the_base() {
    let mut ed = editor();
    let mut h = History::new();
    let mut want = BTreeMap::new();
    for i in 0..7u8 {
        circle(&mut ed, f64::from(i));
        let added = h
            .add(
                ed.doc(),
                new_version(&format!("v{i}")),
                limits(3, usize::MAX),
            )
            .unwrap();
        want.insert(added.id, h.state_of(added.id).unwrap());
        assert!(h.len() <= 3);
        assert_eq!(added.dropped.len(), usize::from(i >= 3));
        if i >= 3 {
            assert_eq!(added.dropped[0], u64::from(i) - 2);
        }
    }
    let ids: Vec<u64> = h.versions().map(|m| m.id).collect();
    assert_eq!(ids, vec![5, 6, 7]);
    for id in ids {
        assert_eq!(h.state_of(id).unwrap(), want[&id]);
    }
    // The IDs go on counting; a dropped ID is not used again.
    assert_eq!(add(&mut h, &ed, "next"), 8);
}

#[test]
fn the_byte_cap_holds_and_a_single_version_that_does_not_fit_fails() {
    let mut ed = editor();
    let mut h = History::new();
    for i in 0..30 {
        circle(&mut ed, f64::from(i));
        let _ = h.add(ed.doc(), new_version("v"), limits(100, 6000));
        assert!(h.bytes() <= 6000, "{}", h.bytes());
    }
    assert!(!h.is_empty() && h.len() < 30);
    // One version alone above the cap: an error, the history stays.
    let before = h.clone();
    let err = h
        .add(ed.doc(), new_version("big"), limits(100, 500))
        .unwrap_err();
    assert!(matches!(err, ReviewError::TooLarge { max: 500, .. }));
    assert!(!err.to_string().is_empty());
    assert_eq!(h, before);
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

#[test]
fn a_version_can_be_deleted_from_any_place() {
    let build = || {
        let mut ed = editor();
        let mut h = History::new();
        let mut want = BTreeMap::new();
        for i in 0..5 {
            circle(&mut ed, f64::from(i));
            let id = add(&mut h, &ed, &format!("v{i}"));
            want.insert(id, h.state_of(id).unwrap());
        }
        (h, want)
    };
    for gone in 1..=5u64 {
        let (mut h, want) = build();
        assert_eq!(h.remove(gone), Ok(false));
        assert_eq!(h.len(), 4);
        assert!(h.meta(gone).is_none());
        for m in h.versions().map(|m| m.id).collect::<Vec<_>>() {
            assert_eq!(h.state_of(m).unwrap(), want[&m], "delete {gone}, check {m}");
        }
    }
    let (mut h, _) = build();
    assert_eq!(h.remove(77), Err(ReviewError::NotFound(77)));
    for id in 1..=4 {
        assert_eq!(h.remove(id), Ok(false));
    }
    assert_eq!(h.remove(5), Ok(true));
    assert!(h.is_empty() && h.bytes() == 0);
}

#[test]
fn the_json_form_round_trips() {
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
    let json = h.to_value();
    assert_eq!(json["format"], 1);
    assert_eq!(json["versions"].as_array().unwrap().len(), 4);
    assert!(json["versions"][0].get("delta").is_none());
    let text = serde_json::to_string(&json).unwrap();
    let loaded = History::load(&serde_json::from_str(&text).unwrap(), Limits::default());
    assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
    assert_eq!(loaded.history.unwrap(), h);
    // An empty history loads as none.
    let none = History::load(&History::new().to_value(), Limits::default());
    assert!(none.history.is_none() && none.notes.is_empty());
}

fn sample() -> Value {
    let mut ed = editor();
    let mut h = History::new();
    for i in 0..3 {
        circle(&mut ed, f64::from(i));
        add(&mut h, &ed, &format!("v{i}"));
    }
    h.to_value()
}

fn load(v: &Value) -> Loaded {
    History::load(v, Limits::default())
}

#[test]
fn a_history_that_cannot_be_read_is_dropped_with_a_note() {
    for bad in [
        json!(null),
        json!([1]),
        json!({}),
        json!({"format": 2}),
        json!({"format": 1, "base": 5}),
        json!({"format": 1, "base": [{"op": "nonsense"}]}),
        json!({"format": 1, "base": [{"op": "delete",
            "target": {"kind": "object", "id": "00000000-0000-0000-0000-000000000009"}}]}),
        json!({"format": 1, "versions": 7}),
    ] {
        let l = load(&bad);
        assert!(l.history.is_none(), "{bad}");
        assert!(!l.notes.is_empty(), "{bad}");
    }
    // No versions: nothing to keep, no complaint.
    let l = load(&json!({"format": 1, "versions": []}));
    assert!(l.history.is_none() && l.notes.is_empty());
}

#[test]
fn a_broken_delta_drops_that_version_and_the_later_ones() {
    let mut v = sample();
    v["versions"][1]["delta"] = json!([{"op": "delete",
        "target": {"kind": "object", "id": "00000000-0000-0000-0000-0000000000ff"}}]);
    let l = load(&v);
    let h = l.history.unwrap();
    assert_eq!(h.len(), 1);
    assert!(l.notes.iter().any(|n| n.contains("dropped")));
    // A delta that is not even an operation.
    let mut v = sample();
    v["versions"][2]["delta"] = json!(["x"]);
    assert_eq!(load(&v).history.unwrap().len(), 2);
    let mut v = sample();
    v["versions"][2]["delta"] = json!({"a": 1});
    assert_eq!(load(&v).history.unwrap().len(), 2);
    // A description that is not a version.
    let mut v = sample();
    v["versions"][1] = json!(5);
    assert_eq!(load(&v).history.unwrap().len(), 1);
    // A bad operation (a property named id).
    let mut v = sample();
    v["versions"][2]["delta"] = json!([{"op": "set", "prop": "id", "value": 1,
        "target": {"kind": "document", "id": "00000000-0000-0000-0000-000000000000"}}]);
    assert_eq!(load(&v).history.unwrap().len(), 2);
}

#[test]
fn texts_and_ids_are_repaired() {
    let mut v = sample();
    v["versions"][0]["name"] = json!("x".repeat(500));
    v["versions"][1]["name"] = json!("   ");
    v["versions"][1]["note"] = json!("n".repeat(NOTE_MAX + 50));
    v["versions"][2]["id"] = v["versions"][0]["id"].clone();
    v["versions"][2]["author"] = json!("a".repeat(400));
    v["versions"][0]["delta"] = json!([{"op": "bogus"}]);
    let l = load(&v);
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
    // A new version never takes an ID that exists.
    let mut h = h;
    let ed = editor();
    let id = h
        .add(ed.doc(), new_version("n"), Limits::default())
        .unwrap()
        .id;
    assert!(!ids.contains(&id));
}

#[test]
fn too_many_versions_are_cut_when_read_and_the_caps_apply() {
    let mut v = sample();
    let first = v["versions"][0].clone();
    let many: Vec<Value> = (0..HARD_MAX_VERSIONS + 20)
        .map(|i| {
            let mut x = first.clone();
            x["id"] = json!(i + 1);
            x
        })
        .collect();
    v["versions"] = json!(many);
    let l = History::load(&v, limits(usize::MAX, usize::MAX));
    assert_eq!(l.history.unwrap().len(), HARD_MAX_VERSIONS);
    assert!(l.notes.iter().any(|n| n.contains("only the first")));
    // The caps of the session fold the oldest versions.
    let l = History::load(&sample(), limits(2, usize::MAX));
    let h = l.history.unwrap();
    assert_eq!(h.versions().map(|m| m.id).collect::<Vec<_>>(), vec![2, 3]);
    assert!(l.notes.iter().any(|n| n.contains("folded")));
    // A history that cannot fit the byte cap at all is dropped.
    let l = History::load(&sample(), limits(10, 100));
    assert!(l.history.is_none() && !l.notes.is_empty());
}

#[test]
fn work_on_operations_is_bounded() {
    // A base whose operations are larger than the hard cap is refused before it is replayed.
    let big = "x".repeat(8 * 1024 * 1024);
    let ops: Vec<Value> = (0..40)
        .map(|i| {
            json!({"op": "create",
                "target": {"kind": "object", "id": format!("00000000-0000-0000-0000-{i:012}")},
                "props": {"blob": big}})
        })
        .collect();
    let l = History::load(&json!({"format": 1, "base": ops}), Limits::default());
    assert!(l.history.is_none());
    assert!(l.notes[0].contains("too large"), "{:?}", l.notes);
}

#[test]
fn limits_are_clamped() {
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

proptest! {
    #[test]
    fn hostile_history_data_never_panics(v in any_json()) {
        let l = History::load(&v, Limits::default());
        if let Some(h) = l.history {
            for m in h.versions().map(|m| m.id).collect::<Vec<_>>() {
                prop_assert!(h.state_of(m).is_ok());
            }
        }
    }

    #[test]
    fn a_damaged_history_keeps_only_versions_that_can_be_rebuilt(
        s in steps(),
        path in prop::collection::vec(0usize..40, 1..6),
        with in any_json(),
    ) {
        let mut ed = editor();
        let mut h = History::new();
        for chunk in s.chunks(3) {
            run(&mut ed, chunk);
            h.add(ed.doc(), new_version("v"), Limits::default()).unwrap();
        }
        let mut v = h.to_value();
        poke(&mut v, &path, with);
        let l = History::load(&v, Limits::default());
        if let Some(kept) = l.history {
            prop_assert!(kept.len() <= HARD_MAX_VERSIONS);
            for m in kept.versions().map(|m| m.id).collect::<Vec<_>>() {
                prop_assert!(kept.state_of(m).is_ok());
            }
            // What is kept writes and reads again to the same history.
            let again = History::load(&kept.to_value(), Limits::default());
            prop_assert_eq!(again.history.as_ref(), Some(&kept));
            prop_assert!(again.notes.is_empty());
        }
    }

    #[test]
    fn the_caps_hold_and_versions_that_stay_keep_their_state(
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
            match h.add(ed.doc(), new_version("v"), lim) {
                Ok(added) => {
                    want.insert(added.id, h.state_of(added.id).unwrap());
                    for d in added.dropped {
                        want.remove(&d);
                    }
                }
                Err(e) => {
                    let too_large = matches!(e, ReviewError::TooLarge { .. });
                    prop_assert!(too_large);
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
            prop_assert_eq!(&h.state_of(id).unwrap(), &want[&id]);
        }
    }
}
