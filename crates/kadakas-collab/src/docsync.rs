//! The bridge between a [`Document`] and a [`Replica`].
//!
//! - [`replica_of`] reads a document into a replica (the first content of a shared document).
//! - [`ops_from_change`] turns a PLAT-02 `ChangeSet` (what one command, undo or redo changed)
//!   into operations. It compares only the things that the change set names.
//! - [`ops_from_diff`] does the same for the whole document, when no change set is at hand (the
//!   log dropped it, or the document was replaced).
//! - [`materialize`] builds a document from a replica, with the repairs that keep it valid.
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
use kadakas_core::model::{Document, FIRST_LAYER, NONE_CLASS, ObjectId};
use kadakas_core::uid::Uid;
use serde_json::{Map, Value};

use crate::op::{Kind, Op, Target};
use crate::replica::Replica;

/// The properties of a thing as JSON, without its `id`. `None` if the document has no such
/// thing.
fn props_of(doc: &Document, target: Target) -> Option<Map<String, Value>> {
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
                if e.get(k) != Some(v) {
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

/// Why a replica cannot become a document.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the shared document is not valid: {0}")]
pub struct MaterializeError(pub String);

/// Builds the document that a replica describes. `base` gives what is not shared: the order of
/// things that exist in both, the IDs generator and the active layer and class.
///
/// Repairs: an object whose layer or class is gone goes to the first layer or the "None" class;
/// a group parent that is gone is cleared. If the result is still not a valid document (for
/// example a shape that fails the checks of the file format), the error says why and the replica
/// is not changed.
pub fn materialize(view: &Replica, base: &Document) -> Result<Document, MaterializeError> {
    fn ordered(
        view: &Replica,
        kind: Kind,
        first: impl Iterator<Item = Uid>,
    ) -> Vec<(Uid, Map<String, Value>)> {
        let mut out = Vec::new();
        let mut done = BTreeSet::new();
        for id in first {
            if let Some(e) = view.alive(Target::new(kind, id)) {
                done.insert(id);
                out.push((id, e.visible()));
            }
        }
        for (t, e) in view.alive_of(kind) {
            if !done.contains(&t.id) {
                out.push((t.id, e.visible()));
            }
        }
        out
    }
    fn with_id(id: Uid, mut m: Map<String, Value>) -> Value {
        m.insert("id".into(), id.into());
        Value::Object(m)
    }
    let layers = ordered(view, Kind::Layer, base.layers.iter().map(|l| l.id.0));
    let classes = ordered(view, Kind::Class, base.classes.iter().map(|c| c.id.0));
    let resources = ordered(view, Kind::Resource, base.resources.iter().map(|r| r.id.0));
    let objects = ordered(view, Kind::Object, base.objects.keys().map(|o| o.0));
    let layer_ids: BTreeSet<Uid> = layers.iter().map(|(i, _)| *i).collect();
    let class_ids: BTreeSet<Uid> = classes.iter().map(|(i, _)| *i).collect();
    let object_ids: BTreeSet<Uid> = objects.iter().map(|(i, _)| *i).collect();

    let objects: Vec<Value> = objects
        .into_iter()
        .map(|(id, mut m)| {
            let fix = |m: &mut Map<String, Value>, key: &str, ids: &BTreeSet<Uid>, to: Uid| {
                let ok = m
                    .get(key)
                    .and_then(Value::as_str)
                    .and_then(Uid::parse)
                    .is_some_and(|u| ids.contains(&u));
                if !ok {
                    m.insert(key.into(), to.into());
                }
            };
            fix(&mut m, "layer", &layer_ids, FIRST_LAYER.0);
            fix(&mut m, "class", &class_ids, NONE_CLASS.0);
            let parent_ok = m
                .get("parent")
                .and_then(Value::as_str)
                .and_then(Uid::parse)
                .is_some_and(|u| object_ids.contains(&u));
            if !parent_ok {
                m.remove("parent");
            }
            with_id(id, m)
        })
        .collect();
    let list = |v: Vec<(Uid, Map<String, Value>)>| -> Vec<Value> {
        v.into_iter().map(|(i, m)| with_id(i, m)).collect()
    };
    let settings = view
        .alive(Target::DOCUMENT)
        .map(|e| e.visible())
        .unwrap_or_default();
    let keep = |id: Uid, ids: &BTreeSet<Uid>, fallback: Uid| {
        if ids.contains(&id) { id } else { fallback }
    };
    let mut value = serde_json::json!({
        "format_version": base.format_version,
        "layers": list(layers),
        "classes": list(classes),
        "objects": objects,
        "resources": list(resources),
        "active_layer": keep(base.active_layer.0, &layer_ids, FIRST_LAYER.0),
        "active_class": keep(base.active_class.0, &class_ids, NONE_CLASS.0),
    });
    for key in ["georef", "drawing"] {
        if let Some(v) = settings.get(key) {
            value[key] = v.clone();
        }
    }
    let new = Document::from_value(value).map_err(|e| MaterializeError(e.to_string()))?;
    let mut out = base.clone();
    out.layers = new.layers;
    out.classes = new.classes;
    out.objects = new.objects;
    out.resources = new.resources;
    out.georef = new.georef;
    out.drawing = new.drawing;
    out.active_layer = new.active_layer;
    out.active_class = new.active_class;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::model::{ClassId, LayerId, ObjectId, Shape};
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
    fn materialize_round_trips_a_document() {
        let mut ed = editor();
        add_line(&mut ed, 0.0);
        add_line(&mut ed, 30.0);
        let view = replica_of(ed.doc());
        let doc = materialize(&view, ed.doc()).unwrap();
        assert_eq!(&doc, ed.doc());
        // A new document gets the content, with the shape of the replica.
        let empty = Document::default();
        let doc = materialize(&view, &empty).unwrap();
        assert_eq!(doc.objects.len(), 2);
    }

    #[test]
    fn materialize_repairs_orphans() {
        let mut ed = editor();
        let first = add_line(&mut ed, 0.0);
        let mut view = replica_of(ed.doc());
        let w = crate::op::Writer::BASELINE;
        // Another user adds a layer and a class, and puts an object on them. Then both go.
        let layer = Target::new(Kind::Layer, Uid::from_num(900));
        let class = Target::new(Kind::Class, Uid::from_num(901));
        let obj = Target::new(Kind::Object, Uid::from_num(902));
        let layer_json = serde_json::to_value(&ed.doc().layers[0]).unwrap();
        let class_json = serde_json::to_value(&ed.doc().classes[0]).unwrap();
        let mut obj_props = props_of(ed.doc(), Target::new(Kind::Object, first.0)).unwrap();
        obj_props.insert("layer".into(), layer.id.into());
        obj_props.insert("class".into(), class.id.into());
        obj_props.insert("parent".into(), Uid::from_num(777).into());
        let props_of_json = |v: &Value| -> std::collections::BTreeMap<String, Value> {
            let mut m = v.as_object().unwrap().clone();
            m.remove("id");
            m.into_iter().collect()
        };
        view.apply(
            w,
            &Op::Create {
                target: layer,
                props: props_of_json(&layer_json),
            },
        );
        view.apply(
            w,
            &Op::Create {
                target: class,
                props: props_of_json(&class_json),
            },
        );
        view.apply(
            w,
            &Op::Create {
                target: obj,
                props: obj_props.into_iter().collect(),
            },
        );
        view.apply(w, &Op::Delete { target: layer });
        view.apply(w, &Op::Delete { target: class });
        let doc = materialize(&view, ed.doc()).unwrap();
        let o = doc.object(ObjectId(obj.id)).unwrap();
        assert_eq!(o.layer, FIRST_LAYER);
        assert_eq!(o.class, NONE_CLASS);
        assert_eq!(o.parent, None);
    }

    #[test]
    fn materialize_resets_a_vanished_active_layer() {
        let mut ed = editor();
        ed.execute(&serde_json::from_value(json!({"op": "add_layer", "name": "B"})).unwrap())
            .unwrap();
        let mut base = ed.doc().clone();
        let extra = base.layers.last().unwrap().id;
        base.active_layer = extra;
        let view = replica_of(&Document::default());
        let doc = materialize(&view, &base).unwrap();
        assert_eq!(doc.active_layer, FIRST_LAYER);
        assert_eq!(doc.active_class, NONE_CLASS);
        let _ = (LayerId::from_num(1), ClassId::from_num(0));
    }

    #[test]
    fn materialize_reports_an_invalid_result() {
        let base = Document::default();
        let mut view = replica_of(&base);
        let bad = Target::new(Kind::Object, Uid::from_num(5));
        view.apply(
            crate::op::Writer::BASELINE,
            &Op::Create {
                target: bad,
                props: [("shape".to_string(), json!({"kind": "rect", "min": "x"}))].into(),
            },
        );
        let err = materialize(&view, &base).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("the shared document is not valid")
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
                .get("drawing")
                .is_some()
        );
        let doc = materialize(&view, &Document::default()).unwrap();
        assert_eq!(doc.drawing.project.name, "Garden");
        let _ = Shape::Unknown {
            kind: String::new(),
            raw: Value::Null,
        };
    }
}
