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
//!   (`shape`, `layer`, `class`, `parent`, `order`) are one property each;
//! - the points of a site model: `shape.points.<n>` in chunks of [`POINT_CHUNK`], and
//!   `shape.lines.<n>` for each line (the rest of the shape stays in `shape`);
//! - the document settings: `georef`, and `drawing.<field>` for each field of the drawing data,
//!   cut once more when the field is an object (`drawing.project.name`), and
//!   `drawing.sheets.<layer id>` for the page settings of each sheet layer;
//! - a layer or resource: its top-level fields (`name`, `visible`, `body`, `order`).
//!
//! The **shape** is one property, except for a site model. The chunks of its points are cut by
//! *position*, so if one user inserts or removes a point while another moves a point of the same
//! model, the chunks no longer fit together and points can end up wrong. Edits that only move
//! points, or that only add at the end, are safe. The picture of an underlay is not in the
//! operation: `blob` replaces it by a reference.
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
//! The stacking order of objects and layers is shared with the property `order` (see `order`).
//! The active layer and the active class are personal settings: they are not shared.

use std::collections::{BTreeMap, BTreeSet};

use kadakas_core::change::ChangeSet;
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::uid::Uid;
use serde_json::{Map, Value};

use crate::op::{Kind, Op, Target};
use crate::replica::{Replica, same_json};

/// The properties of a thing as JSON, without its `id`. `None` if the document has no such
/// thing.
fn props_of(doc: &Document, target: Target) -> Option<Map<String, Value>> {
    props_of_whole(doc, target).map(|m| {
        let mut m = flatten(target.kind, m);
        // The key of the stacking order is not compared with the document: `order_ops` finds
        // out of the list order what keys are needed.
        if matches!(target.kind, Kind::Object | Kind::Layer) {
            m.remove("order");
        }
        m
    })
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
            (Kind::Object, "shape", Value::Object(mut m))
                if m.get("kind").and_then(Value::as_str) == Some("site_model") =>
            {
                // The points of a site model go in chunks and the lines one by one, so moving one
                // point sends one chunk, not all points.
                if let Some(Value::Array(points)) = m.remove("points") {
                    for (n, chunk) in points.chunks(POINT_CHUNK).enumerate() {
                        out.insert(format!("shape.points.{n:08}"), Value::Array(chunk.to_vec()));
                    }
                }
                if let Some(Value::Array(lines)) = m.remove("lines") {
                    for (n, line) in lines.into_iter().enumerate() {
                        out.insert(format!("shape.lines.{n:08}"), line);
                    }
                }
                out.insert("shape".into(), Value::Object(m));
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
    // The parts of a site model: chunks of points and single lines, in the order of their keys.
    let mut points: Vec<Value> = Vec::new();
    let mut lines: Vec<Value> = Vec::new();
    let mut has_points = false;
    let mut has_lines = false;
    for (k, v) in flat {
        if kind == Kind::Object {
            if let Some(n) = k.strip_prefix("shape.points.") {
                let _ = n;
                has_points = true;
                if let Value::Array(a) = v {
                    points.extend(a.iter().cloned());
                }
                continue;
            }
            if k.starts_with("shape.lines.") {
                has_lines = true;
                lines.push(v.clone());
                continue;
            }
        }
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
    if has_points || has_lines {
        let shape = sub(&mut out, "shape");
        shape
            .entry("kind")
            .or_insert_with(|| Value::String("site_model".into()));
        if has_points {
            shape.insert("points".into(), Value::Array(points));
        }
        if has_lines {
            shape.insert("lines".into(), Value::Array(lines));
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

/// The most points of a site model in one property.
pub const POINT_CHUNK: usize = 256;

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
                merge: false,
            });
        }
    }
    order(ops)
}

/// Reads a whole document into a replica. All content has the baseline writer.
pub fn replica_of(doc: &Document) -> Replica {
    let keys = document_keys(doc);
    let mut r = Replica::new();
    for t in all_targets(doc) {
        if let Some(mut props) = props_of(doc, t) {
            if let Some(k) = keys.get(&t) {
                props.insert("order".into(), Value::String(k.clone()));
            }
            r.insert_baseline(t, props);
        }
    }
    r
}

/// The key in the `order` field of an object or layer (kept in `extra`), if it has one.
fn stored_key(extra: &Map<String, Value>) -> Option<String> {
    extra
        .get("order")
        .and_then(Value::as_str)
        .filter(|k| crate::order::valid_key(k))
        .map(str::to_string)
}

/// The stacking keys of a document: the keys that its objects and layers carry, if every one of a
/// list has a key and the keys are increasing; else new keys by position.
fn document_keys(doc: &Document) -> BTreeMap<Target, String> {
    fn lists_keys(items: Vec<(Target, Option<String>)>, out: &mut BTreeMap<Target, String>) {
        let given: Vec<Option<String>> = items.iter().map(|(_, k)| k.clone()).collect();
        let sorted = given.iter().all(Option::is_some) && given.windows(2).all(|w| w[0] < w[1]);
        let keys: Vec<String> = if sorted {
            given.into_iter().flatten().collect()
        } else {
            crate::order::keys_between(None, None, items.len())
        };
        for ((t, _), k) in items.into_iter().zip(keys) {
            out.insert(t, k);
        }
    }
    let mut out = BTreeMap::new();
    lists_keys(
        doc.layers
            .iter()
            .map(|l| (Target::new(Kind::Layer, l.id.0), stored_key(&l.extra)))
            .collect(),
        &mut out,
    );
    let mut per_layer: BTreeMap<kadakas_core::model::LayerId, Vec<(Target, Option<String>)>> =
        BTreeMap::new();
    for o in doc.objects.values() {
        per_layer
            .entry(o.layer)
            .or_default()
            .push((Target::new(Kind::Object, o.id.0), stored_key(&o.extra)));
    }
    for items in per_layer.into_values() {
        lists_keys(items, &mut out);
    }
    out
}

/// The key of a thing in a replica, if it has one.
fn key_in(r: &Replica, t: Target) -> Option<String> {
    r.alive(t)
        .and_then(|e| e.get("order"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// The operations that give a new key to every object that is out of place in the list of its
/// layer (and to every layer out of place in the list of layers, if `layers_too`). `only` limits
/// the objects to these layers. The keys of the others stay.
fn order_ops(
    shadow: &Replica,
    doc: &Document,
    only: Option<&BTreeSet<kadakas_core::model::LayerId>>,
    layers_too: bool,
) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut emit = |items: Vec<Target>| {
        // A thing that the replica has deleted stays deleted: it takes no key.
        let items: Vec<Target> = items
            .into_iter()
            .filter(|t| shadow.alive(*t).is_some() || !shadow.contains(*t))
            .collect();
        let keys: Vec<Option<String>> = items.iter().map(|t| key_in(shadow, *t)).collect();
        for (i, k) in crate::order::rekey(&keys) {
            ops.push(Op::Set {
                target: items[i],
                prop: "order".into(),
                value: Value::String(k),
            });
        }
    };
    if layers_too {
        emit(
            doc.layers
                .iter()
                .map(|l| Target::new(Kind::Layer, l.id.0))
                .collect(),
        );
    }
    let mut per_layer: BTreeMap<kadakas_core::model::LayerId, Vec<Target>> = BTreeMap::new();
    for o in doc.objects.values() {
        if only.is_none_or(|s| s.contains(&o.layer)) {
            per_layer
                .entry(o.layer)
                .or_default()
                .push(Target::new(Kind::Object, o.id.0));
        }
    }
    for items in per_layer.into_values() {
        emit(items);
    }
    ops
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
                    // Shapes of a plug-in object have derived IDs, and two users may make them.
                    merge: target.kind == Kind::Object && target.id.is_derived(),
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
                if !(props.contains_key(k) || k == "order" && target.kind != Kind::Class) {
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
    let mut ops: Vec<Op> = targets
        .into_iter()
        .flat_map(|t| diff_thing(view, t, props_of(doc, t)))
        .collect();
    // Objects that were added or changed may have a new place in the stacking order.
    let layers: BTreeSet<kadakas_core::model::LayerId> = cs
        .added
        .objects
        .iter()
        .chain(&cs.changed.objects)
        .filter_map(|id| doc.object(*id).map(|o| o.layer))
        .collect();
    let layers_too = cs.op == "move_layer"
        || [&cs.added, &cs.changed, &cs.removed]
            .iter()
            .any(|ids| !ids.layers.is_empty());
    ops.extend(order_ops(view, doc, Some(&layers), layers_too));
    order(ops)
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
    ops.extend(order_ops(view, doc, None, true));
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
        // The object, and its key in the stacking order.
        assert_eq!(ops.len(), 2);
        assert!(matches!(&ops[0], Op::Create { target, .. } if target.id == id.0));
        assert!(matches!(&ops[1], Op::Set { prop, .. } if prop == "order"));
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
        assert_eq!(ops.len(), 4, "two objects and their keys");
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
    fn replicas_are_compared_for_a_server_that_went_back() {
        let ed = editor();
        let mut base = replica_of(ed.doc());
        let mut mine = base.clone();
        let w = crate::op::Writer::BASELINE;
        let t = Target::new(Kind::Object, Uid::from_num(900));
        let props: Map<String, Value> = serde_json::from_value(json!({"a": 1})).unwrap();
        let create = |target| Op::Create {
            target,
            props: props.clone().into_iter().collect(),
            merge: false,
        };
        mine.apply(w, &create(t));
        // A thing that only the snapshot has is left alone; a thing I deleted goes.
        let theirs = Target::new(Kind::Object, Uid::from_num(901));
        base.apply(w, &create(theirs));
        let gone = Target::new(Kind::Object, Uid::from_num(902));
        base.apply(w, &create(gone));
        mine.apply(w, &create(gone));
        mine.apply(w, &Op::Delete { target: gone });
        mine.apply(
            w,
            &Op::Set {
                target: Target::DOCUMENT,
                prop: "georef".into(),
                value: json!(1),
            },
        );
        let ops = diff_replicas(&base, &mine);
        assert!(
            ops.iter()
                .any(|o| matches!(o, Op::Create { target, .. } if *target == t))
        );
        assert!(ops.contains(&Op::Delete { target: gone }));
        assert!(!ops.iter().any(|o| o.target() == theirs));
        assert!(
            ops.iter()
                .any(|o| matches!(o, Op::Set { prop, .. } if prop == "georef"))
        );
    }

    #[test]
    fn splitting_keeps_order_and_limits() {
        let ops: Vec<Op> = (0..(CHUNK_OPS as u64 * 2 + 1))
            .map(|i| Op::Delete {
                target: Target::new(Kind::Object, Uid::from_num(i)),
            })
            .collect();
        let chunks = split(ops.clone());
        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|c| c.len() <= CHUNK_OPS));
        assert_eq!(chunks.concat(), ops);
        assert!(split(Vec::new()).is_empty());
        // Big values: a transaction is cut by size too.
        let big = |i: u64| Op::Set {
            target: Target::new(Kind::Object, Uid::from_num(i)),
            prop: "shape".into(),
            value: json!("x".repeat(crate::op::MAX_VALUE_BYTES / 2)),
        };
        let chunks = split(vec![big(1), big(2), big(3)]);
        assert!(chunks.len() >= 2);
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
