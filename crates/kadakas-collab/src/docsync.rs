//! The bridge between a [`Document`] and a [`Replica`].
//!
//! - [`replica_of`] reads a document into a replica (the first content of a shared document).
//! - [`ops_from_change`] turns a PLAT-02 `ChangeSet` (what one command, undo or redo changed)
//!   into operations. It compares only the things that the change set names.
//! - [`ops_from_diff`] does the same for the whole document, when no change set is at hand (the
//!   log dropped it, or the document was replaced).
//! - [`diff_replicas`] compares two replicas, for a client whose server went back.
//! - [`split`] cuts a long list of operations into transactions of a safe size.
//! - `flatten` and `unflatten` turn the JSON of a thing into properties and back.
//!
//! Building a document from a replica is in `materialize`.
//!
//! # Properties
//!
//! A property is the smallest unit that wins or loses a conflict. The JSON of a thing is cut
//! deeper than its top level, so users who change different parts both keep their change:
//!
//! - an object or class: `attrs.<field>` for each field of the attributes (`attrs.pen`,
//!   `attrs.fill`), and `data.<key>` for each key of the record data; other top-level fields
//!   (`shape`, `layer`, `class`, `parent`) are one property each;
//! - the document settings: `georef`, and `drawing.<field>` for each field of the drawing data,
//!   cut once more when the field is an object (`drawing.project.name`), and
//!   `drawing.sheets.<layer id>` for the page settings of each sheet layer;
//! - a layer or resource: its top-level fields (`name`, `visible`, `body`).
//!
//! The **shape** is one property. A big shape value (the points of a site model, the PNG of an
//! underlay) is copied whole by every edit of it, in the transaction, in the log and in the
//! snapshot. This costs size, not correctness. A later version can cut such values into parts (a
//! point list into chunks, a picture into a resource that is made once and never edited).
//!
//! # Numbers
//!
//! `1` and `1.0` are the same number. Clients compare values with [`same_json`], which ignores the
//! difference, so a round trip through a program that writes `1.0` does not make an edit.
//!
//! Operations are derived from the result of a command, not from the command itself. A command
//! can do many things (move 500 objects, run a script). Its result is exactly the properties that
//! changed, and this is what the other users need. It also means every command works with
//! collaboration without extra code.
//!
//! Limits: the z-order of objects and the order of layers are not shared (see `replica`). The
//! active layer and the active class are personal settings: they are not shared.

use std::collections::BTreeSet;

use kadakas_core::change::ChangeSet;
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::uid::Uid;
use serde_json::{Map, Value};

use crate::op::{Kind, Op, Target};
use crate::replica::{Replica, same_json};

/// The properties of a thing as JSON, without its `id`. `None` if the document has no such
/// thing.
fn props_of(doc: &Document, target: Target) -> Option<Map<String, Value>> {
    props_of_whole(doc, target).map(|m| flatten(target.kind, m))
}

/// The top-level fields of a thing.
fn props_of_whole(doc: &Document, target: Target) -> Option<Map<String, Value>> {
    fn strip<T: serde::Serialize>(t: &T) -> Option<Map<String, Value>> {
        match serde_json::to_value(t).ok()? {
            Value::Object(mut m) => {
                m.remove("id");
                Some(m)
            }
            _ => None,
        }
    }
    let id = target.id;
    match target.kind {
        Kind::Object => strip(doc.objects.get(&ObjectId(id))?),
        Kind::Layer => strip(doc.layers.iter().find(|l| l.id.0 == id)?),
        Kind::Class => strip(doc.classes.iter().find(|c| c.id.0 == id)?),
        Kind::Resource => strip(doc.resources.iter().find(|r| r.id.0 == id)?),
        Kind::Document => {
            let mut m = Map::new();
            if let Some(g) = &doc.georef {
                m.insert("georef".into(), serde_json::to_value(g).ok()?);
            }
            if !doc.drawing.is_empty() {
                m.insert("drawing".into(), serde_json::to_value(&doc.drawing).ok()?);
            }
            Some(m)
        }
    }
}

/// All things of a document.
fn all_targets(doc: &Document) -> Vec<Target> {
    let mut out = vec![Target::DOCUMENT];
    out.extend(doc.layers.iter().map(|l| Target::new(Kind::Layer, l.id.0)));
    out.extend(doc.classes.iter().map(|c| Target::new(Kind::Class, c.id.0)));
    out.extend(
        doc.resources
            .iter()
            .map(|r| Target::new(Kind::Resource, r.id.0)),
    );
    out.extend(doc.objects.keys().map(|o| Target::new(Kind::Object, o.0)));
    out
}

/// Cuts the JSON of a thing into properties (see the module text).
pub(crate) fn flatten(kind: Kind, whole: Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for (k, v) in whole {
        match (kind, k.as_str(), v) {
            (Kind::Object | Kind::Class, "attrs", Value::Object(m)) => {
                for (f, x) in m {
                    out.insert(format!("attrs.{f}"), x);
                }
            }
            (Kind::Object, "data", Value::Object(m)) => {
                for (f, x) in m {
                    out.insert(format!("data.{f}"), x);
                }
            }
            (Kind::Document, "drawing", Value::Object(m)) => {
                for (f, x) in m {
                    match (f.as_str(), x) {
                        ("sheets", Value::Array(pairs)) => {
                            for p in pairs {
                                match p.as_array().map(Vec::as_slice) {
                                    Some([Value::String(id), sheet]) => {
                                        out.insert(format!("drawing.sheets.{id}"), sheet.clone());
                                    }
                                    _ => {
                                        out.insert("drawing.sheets".into(), p);
                                    }
                                }
                            }
                        }
                        (_, Value::Object(sub)) => {
                            for (g, y) in sub {
                                out.insert(format!("drawing.{f}.{g}"), y);
                            }
                        }
                        (_, x) => {
                            out.insert(format!("drawing.{f}"), x);
                        }
                    }
                }
            }
            (_, _, v) => {
                out.insert(k, v);
            }
        }
    }
    out
}

/// The opposite of `flatten`: puts the properties together to the JSON of a thing.
pub(crate) fn unflatten(kind: Kind, flat: &Map<String, Value>) -> Map<String, Value> {
    fn sub<'a>(m: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
        let e = m
            .entry(key.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !e.is_object() {
            *e = Value::Object(Map::new());
        }
        e.as_object_mut().expect("made an object above")
    }
    let mut out = Map::new();
    let mut sheets: std::collections::BTreeMap<String, Value> = Default::default();
    for (k, v) in flat {
        if matches!(kind, Kind::Object | Kind::Class)
            && let Some(f) = k.strip_prefix("attrs.")
        {
            sub(&mut out, "attrs").insert(f.to_string(), v.clone());
        } else if kind == Kind::Object
            && let Some(f) = k.strip_prefix("data.")
        {
            sub(&mut out, "data").insert(f.to_string(), v.clone());
        } else if kind == Kind::Document
            && let Some(rest) = k.strip_prefix("drawing.")
        {
            if let Some(id) = rest.strip_prefix("sheets.") {
                sheets.insert(id.to_string(), v.clone());
            } else if let Some((f, g)) = rest.split_once('.') {
                let d = sub(&mut out, "drawing");
                sub(d, f).insert(g.to_string(), v.clone());
            } else {
                sub(&mut out, "drawing").insert(rest.to_string(), v.clone());
            }
        } else {
            out.insert(k.clone(), v.clone());
        }
    }
    if !sheets.is_empty() {
        let pairs = sheets
            .into_iter()
            .map(|(id, s)| Value::Array(vec![Value::String(id), s]))
            .collect();
        sub(&mut out, "drawing").insert("sheets".into(), Value::Array(pairs));
    }
    if matches!(kind, Kind::Object | Kind::Class) {
        sub(&mut out, "attrs");
    }
    out
}

/// The most operations in a transaction that [`split`] makes.
pub const CHUNK_OPS: usize = 5_000;

/// Cuts operations into transactions that stay below [`CHUNK_OPS`] operations and half of
/// `MAX_TXN_BYTES`. The order stays, so the creates of layers, classes and resources (which come
/// first in a list from `ops_from_change`) arrive before the objects that use them. A single
/// operation that is larger than the limit is alone in its transaction (the server refuses it, and
/// the client reports that).
pub fn split(ops: Vec<Op>) -> Vec<Vec<Op>> {
    let limit = crate::op::MAX_TXN_BYTES / 2;
    let mut out: Vec<Vec<Op>> = Vec::new();
    let mut cur: Vec<Op> = Vec::new();
    let mut bytes = 0;
    for op in ops {
        let size = crate::op::op_size(&op);
        if !cur.is_empty() && (cur.len() >= CHUNK_OPS || bytes + size > limit) {
            out.push(std::mem::take(&mut cur));
            bytes = 0;
        }
        bytes += size;
        cur.push(op);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The operations that bring what `target` knows into `base`: creates what `base` lacks, sets
/// the properties that differ, and deletes what `target` has deleted and `base` still has. It
/// leaves alone what only `base` has (the target never saw it). A client uses it when the server
/// went back: `base` is the snapshot, `target` is what the client had.
pub fn diff_replicas(base: &Replica, target: &Replica) -> Vec<Op> {
    let mut ops = Vec::new();
    for (t, e) in target.things() {
        if t.kind == Kind::Document && e.is_deleted() {
            continue;
        }
        if e.is_deleted() {
            if base.alive(*t).is_some() {
                ops.push(Op::Delete { target: *t });
            }
        } else if base.contains(*t) {
            ops.extend(diff_thing(base, *t, Some(e.visible())));
        } else {
            ops.push(Op::Create {
                target: *t,
                props: e.visible().into_iter().collect(),
            });
        }
    }
    order(ops)
}

/// Reads a whole document into a replica. All content has the baseline writer.
pub fn replica_of(doc: &Document) -> Replica {
    let mut r = Replica::new();
    for t in all_targets(doc) {
        if let Some(props) = props_of(doc, t) {
            r.insert_baseline(t, props);
        }
    }
    r
}

/// The operations that turn the thing in the replica into the thing in the document.
/// `new` is the properties in the document, or `None` if the document does not have the thing.
fn diff_thing(view: &Replica, target: Target, new: Option<Map<String, Value>>) -> Vec<Op> {
    let old = view.alive(target);
    match (old, new) {
        (None, Some(props)) => {
            // A deleted thing stays deleted: its ID is never made again.
            if view.contains(target) {
                Vec::new()
            } else {
                vec![Op::Create {
                    target,
                    props: props.into_iter().collect(),
                }]
            }
        }
        (Some(_), None) => vec![Op::Delete { target }],
        (None, None) => Vec::new(),
        (Some(e), Some(props)) => {
            let mut ops = Vec::new();
            for (k, v) in &props {
                if !e.get(k).is_some_and(|old| same_json(old, v)) {
                    ops.push(Op::Set {
                        target,
                        prop: k.clone(),
                        value: v.clone(),
                    });
                }
            }
            for k in e.visible().keys() {
                if !props.contains_key(k) {
                    ops.push(Op::Unset {
                        target,
                        prop: k.clone(),
                    });
                }
            }
            ops
        }
    }
}

/// Puts creates first (layers, classes and resources before objects), then edits, then deletes
/// (objects before the things they refer to).
fn order(mut ops: Vec<Op>) -> Vec<Op> {
    let rank = |op: &Op| match op {
        Op::Create { target, .. } => (0, target.kind),
        Op::Delete { target } => (
            2,
            match target.kind {
                Kind::Object => Kind::Layer,
                Kind::Layer => Kind::Class,
                Kind::Class => Kind::Resource,
                _ => Kind::Object,
            },
        ),
        _ => (1, Kind::Layer),
    };
    ops.sort_by_key(rank);
    ops
}

/// The operations for everything the change set names, comparing the document with the replica
/// `view` (the replica must hold the document as it was before the change).
pub fn ops_from_change(view: &Replica, doc: &Document, cs: &ChangeSet) -> Vec<Op> {
    let mut targets = BTreeSet::new();
    let mut add = |kind: Kind, ids: &[Uid]| {
        targets.extend(ids.iter().map(|id| Target::new(kind, *id)));
    };
    for ids in [&cs.added, &cs.changed, &cs.removed] {
        add(Kind::Object, &uids(&ids.objects));
        add(Kind::Layer, &uids(&ids.layers));
        add(Kind::Class, &uids(&ids.classes));
        add(Kind::Resource, &uids(&ids.resources));
    }
    if cs.document.georef || cs.document.drawing {
        targets.insert(Target::DOCUMENT);
    }
    order(
        targets
            .into_iter()
            .flat_map(|t| diff_thing(view, t, props_of(doc, t)))
            .collect(),
    )
}

fn uids<T: Copy + Into<Uid>>(ids: &[T]) -> Vec<Uid> {
    ids.iter().map(|i| (*i).into()).collect()
}

/// The operations that turn the replica `view` into the document. Compares everything.
pub fn ops_from_diff(view: &Replica, doc: &Document) -> Vec<Op> {
    let mut seen = BTreeSet::new();
    let mut ops = Vec::new();
    for t in all_targets(doc) {
        seen.insert(t);
        ops.extend(diff_thing(view, t, props_of(doc, t)));
    }
    for (t, _) in view.alive_things() {
        if !seen.contains(t) {
            ops.extend(diff_thing(view, *t, None));
        }
    }
    order(ops)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::model::{FIRST_LAYER, NONE_CLASS};
    use serde_json::json;

    fn editor() -> Editor {
        Editor::with_sequential_ids()
    }

    fn add_line(ed: &mut Editor, x: f64) -> ObjectId {
        let cmd: Command = serde_json::from_value(json!({
            "op": "add_object",
            "shape": {"kind": "rect", "min": [x, 0.0], "max": [x + 10.0, 5.0]}
        }))
        .unwrap();
        let out = ed.execute(&cmd).unwrap();
        ObjectId(out.created[0])
    }

    fn last_change(ed: &Editor, since: u64) -> ChangeSet {
        ed.changes_since(since)
            .changes
            .last()
            .cloned()
            .unwrap()
            .clone()
    }

    #[test]
    fn replica_of_new_document_has_layer_class_and_settings() {
        let ed = editor();
        let r = replica_of(ed.doc());
        assert!(r.alive(Target::DOCUMENT).is_some());
        assert!(r.alive(Target::new(Kind::Layer, FIRST_LAYER.0)).is_some());
        assert!(r.alive(Target::new(Kind::Class, NONE_CLASS.0)).is_some());
        assert_eq!(r.alive_of(Kind::Object).count(), 0);
    }

    #[test]
    fn change_set_becomes_create_set_and_delete() {
        let mut ed = editor();
        let mut view = replica_of(ed.doc());
        let rev = ed.revision();
        let id = add_line(&mut ed, 0.0);
        let cs = last_change(&ed, rev);
        let ops = ops_from_change(&view, ed.doc(), &cs);
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], Op::Create { target, .. } if target.id == id.0));
        view.apply_all(crate::op::Writer::BASELINE, &ops);

        // A move changes the shape property only.
        let rev = ed.revision();
        let cmd: Command = serde_json::from_value(json!({
            "op": "move_objects", "ids": [id], "delta": [5.0, 0.0]
        }))
        .unwrap();
        ed.execute(&cmd).unwrap();
        let ops = ops_from_change(&view, ed.doc(), &last_change(&ed, rev));
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], Op::Set { prop, .. } if prop == "shape"));
        view.apply_all(crate::op::Writer::BASELINE, &ops);
        assert_eq!(view, replica_of(ed.doc()));

        // Delete.
        let rev = ed.revision();
        ed.execute(&serde_json::from_value(json!({"op": "delete_objects", "ids": [id]})).unwrap())
            .unwrap();
        let ops = ops_from_change(&view, ed.doc(), &last_change(&ed, rev));
        assert_eq!(
            ops,
            vec![Op::Delete {
                target: Target::new(Kind::Object, id.0)
            }]
        );
        view.apply_all(crate::op::Writer::BASELINE, &ops);
        assert!(view.alive(Target::new(Kind::Object, id.0)).is_none());
        // A deleted ID is not created again, also if the document has it (an undo).
        assert!(ed.undo());
        let ops = ops_from_diff(&view, ed.doc());
        assert!(ops.is_empty());
    }

    #[test]
    fn full_diff_finds_everything_and_orders_ops() {
        let mut ed = editor();
        let mut view = replica_of(ed.doc());
        let a = add_line(&mut ed, 0.0);
        let b = add_line(&mut ed, 20.0);
        let ops = ops_from_diff(&view, ed.doc());
        assert_eq!(ops.len(), 2);
        view.apply_all(crate::op::Writer::BASELINE, &ops);
        assert_eq!(view, replica_of(ed.doc()));
        // Remove a prop (a record entry cleared) and delete one object.
        ed.execute(&serde_json::from_value(json!({"op": "delete_objects", "ids": [a]})).unwrap())
            .unwrap();
        let ops = ops_from_diff(&view, ed.doc());
        assert_eq!(
            ops,
            vec![Op::Delete {
                target: Target::new(Kind::Object, a.0)
            }]
        );
        view.apply_all(crate::op::Writer::BASELINE, &ops);
        assert!(view.alive(Target::new(Kind::Object, b.0)).is_some());
        // No difference, no operations.
        assert!(ops_from_diff(&view, ed.doc()).is_empty());
    }

    #[test]
    fn unset_when_a_property_disappears() {
        let ed = editor();
        let mut view = replica_of(ed.doc());
        let target = Target::DOCUMENT;
        view.apply(
            crate::op::Writer::BASELINE,
            &Op::Set {
                target,
                prop: "georef".into(),
                value: json!({}),
            },
        );
        let ops = ops_from_diff(&view, ed.doc());
        assert_eq!(
            ops,
            vec![Op::Unset {
                target,
                prop: "georef".into()
            }]
        );
    }

    #[test]
    fn layer_and_class_changes_are_found() {
        let mut ed = editor();
        let view = replica_of(ed.doc());
        let rev = ed.revision();
        ed.execute(&serde_json::from_value(json!({"op": "add_layer", "name": "Plan B"})).unwrap())
            .unwrap();
        let ops = ops_from_change(&view, ed.doc(), &last_change(&ed, rev));
        assert!(
            ops.iter()
                .any(|o| matches!(o, Op::Create { target, .. } if target.kind == Kind::Layer))
        );
    }

    #[test]
    fn settings_travel_as_a_thing() {
        let mut base = Document::default();
        base.drawing.project.name = "Garden".into();
        let view = replica_of(&base);
        assert!(
            view.alive(Target::DOCUMENT)
                .unwrap()
                .get("drawing.project.name")
                .is_some()
        );
        let m = crate::materialize::materialize(&view, &Document::default());
        assert_eq!(m.doc.drawing.project.name, "Garden");
        assert!(m.quarantined.is_empty());
    }
}
