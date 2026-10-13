use kadakas_core::uid::Uid;
use kadakas_props::docsync::{ops_from_diff, replica_of};
use kadakas_props::op::{Op, Target};
use proptest::prelude::*;
use serde_json::json;

use super::*;
use crate::testutil::{circle, cmd, editor, run, steps};

fn obj(n: u64) -> Target {
    Target::new(Kind::Object, Uid::from_num(n))
}

fn put(s: &mut State, t: Target, props: Value) {
    s.apply(&Op::Create {
        target: t,
        props: props.as_object().unwrap().clone().into_iter().collect(),
        merge: false,
    })
    .unwrap();
}

fn all() -> DiffOptions {
    DiffOptions {
        kinds: Vec::new(),
        limit: 1000,
    }
}

#[test]
fn objects_layers_and_settings_are_grouped_into_added_removed_and_changed() {
    let mut ed = editor();
    let a = circle(&mut ed, 0.0);
    let b = circle(&mut ed, 5.0);
    let s1 = State::of(ed.doc(), None);
    ed.execute(&cmd(
        json!({"op": "move_objects", "ids": [a], "delta": [1.0, 0.0]}),
    ))
    .unwrap();
    ed.execute(&cmd(json!({"op": "delete_objects", "ids": [b]})))
        .unwrap();
    let c = circle(&mut ed, 9.0);
    ed.execute(&cmd(json!({"op": "add_layer", "name": "Plan B"})))
        .unwrap();
    ed.execute(&cmd(json!({"op": "set_project", "name": "Garden"})))
        .unwrap();
    let s2 = State::of(ed.doc(), Some(&s1));
    let d = diff(&s1, &s2, &all());
    assert!(d.exact && !d.truncated);
    assert_eq!(
        d.counts["objects"],
        Counts {
            added: 1,
            removed: 1,
            changed: 1
        }
    );
    assert_eq!(
        d.counts["layers"],
        Counts {
            added: 1,
            removed: 0,
            changed: 0
        }
    );
    assert_eq!(d.counts["document"].changed, 1);
    assert_eq!(d.counts["classes"].total(), 0);
    assert_eq!(d.totals.total(), 5);
    let objects = &d.groups["objects"];
    assert_eq!(objects.added[0].id, c.0);
    assert_eq!(objects.added[0].info["shape"], "circle");
    assert_eq!(objects.removed[0].id, b.0);
    let moved = &objects.changed[0];
    assert_eq!(moved.id, a.0);
    assert_eq!(moved.properties.len(), 1);
    assert_eq!(moved.properties[0].path, "shape");
    assert_eq!(
        moved.properties[0].old,
        Some(Shown::Whole(
            json!({"kind": "circle", "center": [0.0, 0.0], "radius": 2.0})
        ))
    );
    assert!(d.groups["layers"].added[0].info["name"] == "Plan B");
    let doc = &d.groups["document"].changed[0];
    let name = doc
        .properties
        .iter()
        .find(|p| p.path == "drawing.project.name")
        .unwrap();
    assert_eq!(name.new, Some(Shown::Whole(json!("Garden"))));
    // The JSON form.
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(v["totals"]["added"], 2);
    assert_eq!(v["objects"]["added"][0]["id"], json!(c));
    assert_eq!(v["objects"]["changed"][0]["properties"][0]["path"], "shape");
    assert_eq!(v["truncated"], false);
    // The other way round swaps added and removed.
    let back = diff(&s2, &s1, &all());
    assert_eq!(
        back.counts["objects"],
        Counts {
            added: 1,
            removed: 1,
            changed: 1
        }
    );
    assert_eq!(back.groups["objects"].added[0].id, b.0);
    // Equal states have no change.
    assert_eq!(diff(&s2, &s2, &all()).totals.total(), 0);
}

#[test]
fn a_removed_property_has_no_new_value_and_a_new_one_no_old_value() {
    let mut a = State::new();
    let mut b = State::new();
    put(&mut a, obj(1), json!({"keep": 1, "gone": 2}));
    put(&mut b, obj(1), json!({"keep": 1.0, "fresh": 3}));
    let d = diff(&a, &b, &all());
    let props = &d.groups["objects"].changed[0].properties;
    assert_eq!(props.len(), 2, "1 and 1.0 are the same number");
    let fresh = props.iter().find(|p| p.path == "fresh").unwrap();
    assert!(fresh.old.is_none() && fresh.new.is_some());
    let gone = props.iter().find(|p| p.path == "gone").unwrap();
    assert!(gone.new.is_none() && gone.old.is_some());
}

#[test]
fn kinds_and_the_limit_choose_what_is_listed_but_counts_stay_complete() {
    let a = State::new();
    let mut b = State::new();
    for n in 1..=30 {
        put(&mut b, obj(n), json!({"shape": {"kind": "line"}}));
    }
    put(
        &mut b,
        Target::new(Kind::Layer, Uid::from_num(99)),
        json!({"name": "X"}),
    );
    let only_layers = DiffOptions {
        kinds: vec![Group::Layers],
        limit: 5,
    };
    let d = diff(&a, &b, &only_layers);
    assert_eq!(d.counts.len(), 1);
    assert_eq!(d.totals.added, 1);
    assert!(!d.truncated);
    assert!(!d.groups.contains_key("objects"));
    let limited = DiffOptions {
        kinds: Vec::new(),
        limit: 10,
    };
    let d = diff(&a, &b, &limited);
    assert_eq!(d.counts["objects"].added, 30, "the counts are complete");
    let listed: usize = d.groups.values().map(|l| l.added.len()).sum();
    assert_eq!(listed, 10);
    assert!(d.truncated && d.exact);
    // Removed and changed things count past the limit too.
    let d = diff(&b, &a, &limited);
    assert_eq!(d.counts["objects"].removed, 30);
    assert!(d.truncated);
    let mut c = b.clone();
    for n in 1..=30 {
        c.apply(&Op::Set {
            target: obj(n),
            prop: "x".into(),
            value: json!(1),
        })
        .unwrap();
    }
    let d = diff(
        &b,
        &c,
        &DiffOptions {
            kinds: vec![Group::Objects],
            limit: 3,
        },
    );
    assert_eq!(d.counts["objects"].changed, 30);
    assert_eq!(d.groups["objects"].changed.len(), 3);
    assert!(d.truncated);
    // The limit is cut to the maximum, and a limit of 0 lists nothing.
    let d = diff(
        &a,
        &b,
        &DiffOptions {
            kinds: Vec::new(),
            limit: usize::MAX,
        },
    );
    assert_eq!(d.groups["objects"].added.len(), 30);
    let d = diff(
        &a,
        &b,
        &DiffOptions {
            kinds: Vec::new(),
            limit: 0,
        },
    );
    assert!(d.truncated && d.groups["objects"].added.is_empty());
    assert_eq!(Group::parse("objects"), Some(Group::Objects));
    assert_eq!(Group::parse("nope"), None);
    assert_eq!(DiffOptions::default().limit, DEFAULT_LIMIT);
}

#[test]
fn long_values_are_cut_with_a_hash_and_long_lists_of_paths_are_cut() {
    let mut a = State::new();
    let mut b = State::new();
    let long = "x".repeat(1000);
    let long2 = format!("{}y", "x".repeat(999));
    put(&mut a, obj(1), json!({"text": long, "list": [1, 2]}));
    put(&mut b, obj(1), json!({"text": long2, "list": vec![7; 500]}));
    let d = diff(&a, &b, &all());
    let props = &d.groups["objects"].changed[0].properties;
    let text = props.iter().find(|p| p.path == "text").unwrap();
    let (Some(Shown::Cut { cut, len, hash }), Some(Shown::Cut { hash: hash2, .. })) =
        (&text.old, &text.new)
    else {
        panic!("{text:?}")
    };
    assert_eq!(
        (cut.chars().count(), *len, hash.len()),
        (MAX_VALUE_CHARS, 1000, 16)
    );
    assert_ne!(
        hash, hash2,
        "a change at the end of a long value shows in the hash"
    );
    let list = props.iter().find(|p| p.path == "list").unwrap();
    assert!(matches!(list.new, Some(Shown::Cut { len, .. }) if len > MAX_VALUE_CHARS));
    assert!(matches!(list.old, Some(Shown::Whole(_))));
    // Objects are cut by their JSON text too.
    assert!(matches!(
        Shown::of(&json!({"a": "z".repeat(300)})),
        Shown::Cut { .. }
    ));
    assert!(matches!(Shown::of(&json!("short")), Shown::Whole(_)));
    assert!(matches!(Shown::of(&json!(null)), Shown::Whole(_)));
    // Many changed properties of one thing.
    let mut a = State::new();
    let mut b = State::new();
    let many = |v: i32| {
        Value::Object(
            (0..80)
                .map(|i| (format!("p{i:03}"), json!(v)))
                .collect::<serde_json::Map<_, _>>(),
        )
    };
    put(&mut a, obj(1), many(1));
    put(&mut b, obj(1), many(2));
    let d = diff(&a, &b, &all());
    let c = &d.groups["objects"].changed[0];
    assert_eq!(c.properties.len(), MAX_PATHS);
    assert!(c.properties_truncated);
    assert_eq!(
        serde_json::to_value(&d).unwrap()["objects"]["changed"][0]["properties_truncated"],
        true
    );
}

#[test]
fn the_work_is_bounded() {
    let a = State::new();
    let mut b = State::new();
    for n in 1..=50 {
        put(&mut b, obj(n), json!({"a": 1, "b": 2}));
    }
    let d = diff_with_budget(&a, &b, &all(), 40);
    assert!(!d.exact && d.truncated);
    assert!(d.counts["objects"].added < 50);
    let d = diff_with_budget(&a, &b, &all(), 10_000);
    assert!(d.exact && d.counts["objects"].added == 50);
}

/// The paths of the changed things that a sync of the same two documents writes: the property
/// operations of collaboration, without the stacking key.
fn sync_paths(
    a: &kadakas_core::model::Document,
    b: &kadakas_core::model::Document,
) -> Vec<(Target, String)> {
    let mut out: Vec<(Target, String)> = ops_from_diff(&replica_of(a), b)
        .iter()
        .filter_map(|op| match op {
            Op::Set { target, prop, .. } | Op::Unset { target, prop } if prop != "order" => {
                Some((*target, prop.clone()))
            }
            _ => None,
        })
        .collect();
    out.sort();
    out
}

fn diff_paths(d: &Diff) -> Vec<(Target, String)> {
    let mut out = Vec::new();
    for (name, l) in &d.groups {
        let kind = match *name {
            "objects" => Kind::Object,
            "layers" => Kind::Layer,
            "classes" => Kind::Class,
            "resources" => Kind::Resource,
            _ => Kind::Document,
        };
        for c in &l.changed {
            for p in c.properties.iter().filter(|p| p.path != "order") {
                let id = if kind == Kind::Document {
                    Uid::NIL
                } else {
                    c.id
                };
                out.push((Target::new(kind, id), p.path.clone()));
            }
        }
    }
    out.sort();
    out
}

proptest! {
    /// A diff and a sync agree: the properties that the diff reports as changed are the ones that
    /// the property operations of collaboration set or unset. The same things are added and
    /// removed.
    #[test]
    fn a_diff_and_a_sync_agree(first in steps(), second in steps()) {
        let mut ed = editor();
        run(&mut ed, &first);
        let a = ed.doc().clone();
        run(&mut ed, &second);
        let b = ed.doc().clone();
        let (sa, sb) = (State::of(&a, None), State::of(&b, Some(&State::of(&a, None))));
        let d = diff(&sa, &sb, &all());
        prop_assert_eq!(diff_paths(&d), sync_paths(&a, &b));
        let ra = replica_of(&a);
        let rb = replica_of(&b);
        let added: Vec<Uid> = rb.alive_of(Kind::Object).map(|(t, _)| t.id)
            .filter(|id| ra.alive(Target::new(Kind::Object, *id)).is_none()).collect();
        let mut got: Vec<Uid> = d.groups["objects"].added.iter().map(|e| e.id).collect();
        got.sort();
        let mut want = added;
        want.sort();
        prop_assert_eq!(got, want);
    }

    /// The operations of a diff, applied to the first state, give the second.
    #[test]
    fn a_diff_applied_to_a_gives_b(first in steps(), second in steps()) {
        let mut ed = editor();
        run(&mut ed, &first);
        let a = State::of(ed.doc(), None);
        run(&mut ed, &second);
        let b = State::of(ed.doc(), Some(&a));
        let mut got = a.clone();
        prop_assert!(got.apply_all(&a.diff(&b)).is_ok());
        prop_assert_eq!(&got, &b);
        // And the report is empty exactly when the states are equal.
        prop_assert_eq!(diff(&a, &b, &all()).totals.total() == 0, a == b);
    }
}
