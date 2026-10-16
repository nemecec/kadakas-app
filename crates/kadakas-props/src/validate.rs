//! Validation of one thing, as the core checks a command or a loaded file.
//!
//! A thing has properties (JSON). [`validate_entity`] tells if they make a valid object, layer,
//! class, resource or document setting: they must read as the right type, numbers must be finite
//! and in range, and the shape must pass the checks of the file format. The same function runs in
//! three places:
//!
//! - a **server** can run it on the things that a transaction touches, after a tentative apply,
//!   and refuse the transaction with `invalid_txn`. [`check_transaction`] does this, with the
//!   check for duplicate IDs, and the test sequencer uses it;
//! - [`crate::materialize::materialize`] runs it on every received thing and quarantines the ones
//!   that fail, so one bad thing never blocks a client;
//! - a client can run it on its own edits.
//!
//! The checks do not look at other things (references between things are repaired when the
//! document is built).

use kadakas_core::model::{
    Class, Document, Layer, MAX_COORD, Object, Shape, VertexKind, arc_sweep,
};
use kadakas_core::provenance;
use kadakas_core::resource::Resource;
use kadakas_core::uid::Uid;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

use crate::op::{Kind, Op, Target};
use crate::replica::Replica;

/// Reads a thing from its properties and its ID.
pub fn with_id(id: Uid, props: &Map<String, Value>) -> Value {
    let mut m = props.clone();
    m.insert("id".into(), id.into());
    Value::Object(m)
}

/// Why an object is not valid, or `None`. These are the checks of `Document::validate` that look
/// at one object alone.
pub fn object_problem(o: &Object) -> Option<String> {
    let bad_arc = matches!(o.shape, Shape::Arc { start, end, .. } if arc_sweep(start, end) == 0.0);
    let bad_vertex = o.shape.vertices().is_some_and(|vs| {
        vs.iter()
            .any(|v| matches!(v.kind, VertexKind::Arc { radius } if radius < 0.0))
    });
    if !o.is_finite()
        || !o.shape.is_in_range()
        || !o.shape.has_valid_thickness()
        || o.shape.problem().is_some()
        || bad_arc
        || bad_vertex
    {
        return Some(
            o.shape
                .problem()
                .unwrap_or_else(|| "the object has an invalid shape".into()),
        );
    }
    // The source tag of an imported object (IOP-22) comes from other users, so it is checked.
    if let Some(Err(why)) = o.data.get(provenance::KEY).map(provenance::check) {
        return Some(why);
    }
    None
}

/// Why a layer is not valid, or `None`.
pub fn layer_problem(l: &Layer) -> Option<String> {
    if !l.elevation.is_finite() || l.elevation.abs() > MAX_COORD {
        return Some("the elevation is out of range".into());
    }
    if !l.scale.is_finite() {
        return Some("the scale must be a finite number".into());
    }
    if let Some(v) = l.extra.get(provenance::LAYER_KEY)
        && provenance::LayerSource::of(v).is_none()
    {
        return Some("the import source of the layer is not valid".into());
    }
    None
}

/// The slow checks of a thing, for a thing whose data arrives now: a picture is read and compared
/// with its fields. Other things pass. A server runs it on the things that a transaction touches.
pub fn verify_entity(kind: Kind, id: Uid, props: &Map<String, Value>) -> Result<(), String> {
    if kind != Kind::Resource {
        return Ok(());
    }
    let full = with_id(id, &crate::docsync::unflatten(kind, props));
    let r: Resource =
        serde_json::from_value(full).map_err(|e| format!("not a valid Resource: {e}"))?;
    r.body.verify()
}

/// Checks the properties of one thing. `Ok` if they make a valid thing of this kind.
pub fn validate_entity(kind: Kind, id: Uid, props: &Map<String, Value>) -> Result<(), String> {
    if matches!(kind, Kind::Object | Kind::Layer)
        && let Some(k) = props.get("order")
        && !k.as_str().is_some_and(crate::order::valid_key)
    {
        return Err(format!(
            "order must be a key of the digits 0-9A-Za-z (no 0 at the end, at most {} characters)",
            crate::order::MAX_KEY_LEN
        ));
    }
    let props = &crate::docsync::unflatten(kind, props);
    let full = with_id(id, props);
    let read = |e: serde_json::Error| format!("not a valid {kind:?}: {e}");
    match kind {
        Kind::Object => {
            let o: Object = serde_json::from_value(full).map_err(read)?;
            match object_problem(&o) {
                Some(p) => Err(p),
                None => Ok(()),
            }
        }
        Kind::Layer => {
            let l: Layer = serde_json::from_value(full).map_err(read)?;
            layer_problem(&l).map_or(Ok(()), Err)
        }
        Kind::Class => {
            let c: Class = serde_json::from_value(full).map_err(read)?;
            if c.attrs.is_finite() {
                Ok(())
            } else {
                Err("class attributes must be finite".into())
            }
        }
        Kind::Resource => {
            let r: Resource = serde_json::from_value(full).map_err(read)?;
            // An empty or repeated name is not an error here: it is repaired when the document
            // is built.
            r.body.validate()
        }
        Kind::Issue => {
            let i: kadakas_core::issue::Issue = serde_json::from_value(full).map_err(read)?;
            i.validate()
        }
        Kind::Comment => {
            let c: kadakas_core::issue::Comment = serde_json::from_value(full).map_err(read)?;
            c.validate()
        }
        Kind::Document => {
            let mut doc = Document::default();
            for (k, v) in props {
                match k.as_str() {
                    "georef" => {
                        doc.georef = Some(serde_json::from_value(v.clone()).map_err(read)?);
                    }
                    "drawing" => {
                        doc.drawing = serde_json::from_value(v.clone()).map_err(read)?;
                    }
                    _ => {}
                }
            }
            Ok(())
        }
    }
}

/// Why a server refuses a transaction before it orders it.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CheckError {
    /// A `create` without `merge` uses an ID that exists, existed, or is made twice in the
    /// transaction. The server answers `duplicate_uid`.
    #[error("the ID {0} is used")]
    DuplicateUid(Uid),
    /// A thing refers to a blob that the server does not have. The text names the hash. The
    /// server answers `invalid_txn` (or a retry error with `hash`, if it wants the client to send
    /// the blob again).
    #[error("the blob {0} was not sent before the transaction")]
    MissingBlob(String),
    /// A thing would not be valid. The server answers `invalid_txn`.
    #[error("{0}")]
    Invalid(String),
}

/// Checks that no `create` without `merge` uses an ID that exists or ever existed (deleted things
/// count), or that the transaction makes twice. A `create` with `merge` is never a duplicate: the
/// merge rules write it as `set` operations, or drop it when the thing is deleted.
pub fn check_duplicates(r: &Replica, ops: &[Op]) -> Result<(), CheckError> {
    let mut made: Vec<Target> = Vec::new();
    for op in ops {
        if let Op::Create { target, merge, .. } = op {
            if (r.contains(*target) || made.contains(target)) && !*merge {
                return Err(CheckError::DuplicateUid(target.id));
            }
            made.push(*target);
        }
    }
    Ok(())
}

/// The properties of the things that the operations touch, as they will be after the operations
/// (a tentative apply on copies), and the things in the order they were first touched. An edit of
/// a thing that is not there or is deleted is left out: the merge drops it.
///
/// A `create` with `merge` of a living thing writes its properties over the current ones, so the
/// result is the visible properties overlaid with the properties of the `create`.
fn touched(r: &Replica, ops: &[Op]) -> (Vec<Target>, BTreeMap<Target, Map<String, Value>>) {
    let mut props: BTreeMap<Target, Map<String, Value>> = BTreeMap::new();
    let mut order: Vec<Target> = Vec::new();
    for op in ops {
        let t = op.target();
        let current = || r.alive(t).map(|e| e.visible());
        match op {
            Op::Create {
                props: p, merge, ..
            } => {
                let base = if *merge {
                    props.remove(&t).or_else(current)
                } else {
                    None
                };
                let mut m = base.unwrap_or_default();
                m.extend(p.iter().map(|(k, v)| (k.clone(), v.clone())));
                props.insert(t, m);
            }
            Op::Set { prop, value, .. } => {
                let Some(m) = working_copy(&mut props, t, current()) else {
                    continue;
                };
                m.insert(prop.clone(), value.clone());
            }
            Op::Unset { prop, .. } => {
                let Some(m) = working_copy(&mut props, t, current()) else {
                    continue;
                };
                m.remove(prop);
            }
            _ => continue,
        }
        if !order.contains(&t) {
            order.push(t);
        }
    }
    (order, props)
}

/// The working copy of a thing: the one made before in this transaction, or the current one.
fn working_copy(
    props: &mut BTreeMap<Target, Map<String, Value>>,
    t: Target,
    current: Option<Map<String, Value>>,
) -> Option<&mut Map<String, Value>> {
    match props.entry(t) {
        std::collections::btree_map::Entry::Occupied(e) => Some(e.into_mut()),
        std::collections::btree_map::Entry::Vacant(v) => Some(v.insert(current?)),
    }
}

/// The blobs that a server must have before it can check the transaction: the pictures of every
/// thing the operations touch, as they will be after the operations (an edit of the opacity of an
/// underlay needs the picture too). The list has no repeats.
pub fn blobs_needed(r: &Replica, ops: &[Op]) -> Vec<String> {
    let (order, props) = touched(r, ops);
    let mut out: Vec<String> = Vec::new();
    for t in order {
        for key in crate::blob::BLOB_PROPS {
            if let Some(h) = props[&t].get(key).and_then(crate::blob::blob_ref)
                && !out.iter().any(|o| o == h)
            {
                out.push(h.to_string());
            }
        }
    }
    out
}

/// All blobs that the replica refers to by any property of any thing, deleted things and removed
/// properties included (an undo can bring them back). A server may drop a blob that is not in
/// this set.
pub fn referenced_blobs(r: &Replica) -> BTreeSet<String> {
    r.things()
        .flat_map(|(_, e)| e.props.values())
        .filter_map(|slot| crate::blob::blob_ref(&slot.value).map(str::to_string))
        .collect()
}

/// The check that a server runs on a transaction before it orders it, after `op::validate_ops`:
///
/// 1. no duplicate ID ([`check_duplicates`]);
/// 2. each thing that the operations touch stays a valid thing: the operations are applied to
///    copies of the properties, the pictures are put back (`blobs` gives the text of a blob by its
///    hash, or `None` if the server does not have it), and [`validate_entity`] runs on the result.
///
/// An edit of a thing that is not there or is deleted is not an error: the merge drops it.
pub fn check_transaction(
    r: &Replica,
    ops: &[Op],
    blobs: &dyn Fn(&str) -> Option<String>,
) -> Result<(), CheckError> {
    check_duplicates(r, ops)?;
    check_entities(r, ops, blobs)
}

/// The second part of [`check_transaction`]: the value checks of the touched things.
pub fn check_entities(
    r: &Replica,
    ops: &[Op],
    blobs: &dyn Fn(&str) -> Option<String>,
) -> Result<(), CheckError> {
    let (order, props) = touched(r, ops);
    for t in order {
        let full = crate::blob::inline_props(&props[&t], blobs).map_err(CheckError::MissingBlob)?;
        validate_entity(t.kind, t.id, &full)
            .and_then(|()| verify_entity(t.kind, t.id, &full))
            .map_err(|e| CheckError::Invalid(format!("{:?} {}: {e}", t.kind, t.id)))?;
    }
    Ok(())
}

/// Finds the first ID (UUID text) in a message.
pub fn find_uid(msg: &str) -> Option<Uid> {
    let b = msg.as_bytes();
    (0..b.len().saturating_sub(35)).find_map(|i| {
        if b[i + 8] == b'-' && msg.is_char_boundary(i) && msg.is_char_boundary(i + 36) {
            Uid::parse(&msg[i..i + 36])
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn props(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn rect() -> Map<String, Value> {
        props(json!({
            "layer": Uid::from_num(1), "class": Uid::from_num(0),
            "shape": {"kind": "rect", "min": [0, 0], "max": [5, 5]}
        }))
    }

    #[test]
    fn objects() {
        let id = Uid::from_num(5);
        assert_eq!(validate_entity(Kind::Object, id, &rect()), Ok(()));
        let mut bad = rect();
        bad.insert("shape".into(), json!({"kind": "rect", "min": "x"}));
        assert!(validate_entity(Kind::Object, id, &bad).is_err());
        let mut far = rect();
        far.insert(
            "shape".into(),
            json!({"kind": "rect", "min": [0, 0], "max": [5e12, 5]}),
        );
        assert!(validate_entity(Kind::Object, id, &far).is_err());
        let mut arc = rect();
        arc.insert(
            "shape".into(),
            json!({"kind": "arc", "center": [0, 0], "radius": 5, "start": 1.0, "end": 1.0}),
        );
        assert!(validate_entity(Kind::Object, id, &arc).is_err());
        let mut neg = rect();
        neg.insert(
            "shape".into(),
            json!({"kind": "polyline", "closed": false,
                   "points": [[0, 0], {"p": [5, 0], "kind": "arc", "radius": -3}, [9, 9]]}),
        );
        assert!(validate_entity(Kind::Object, id, &neg).is_err());
        assert!(validate_entity(Kind::Object, id, &Map::new()).is_err());
    }

    #[test]
    fn layers_classes_resources_settings() {
        let id = Uid::from_num(7);
        let layer = props(json!({"name": "L", "visible": true, "locked": false,
                                 "elevation": 0.0, "scale": 100.0}));
        assert_eq!(validate_entity(Kind::Layer, id, &layer), Ok(()));
        let mut far = layer.clone();
        far.insert("elevation".into(), json!(1e12));
        assert!(validate_entity(Kind::Layer, id, &far).is_err());
        assert!(validate_entity(Kind::Layer, id, &Map::new()).is_err());
        let class = props(json!({"name": "C", "visible": true, "attrs": {}}));
        assert_eq!(validate_entity(Kind::Class, id, &class), Ok(()));
        let res = props(json!({"name": "W", "body": {"type": "worksheet"}}));
        assert_eq!(validate_entity(Kind::Resource, id, &res), Ok(()));
        // An empty name is repaired when the document is built, not refused.
        let res = props(json!({"name": " ", "body": {"type": "worksheet"}}));
        assert_eq!(validate_entity(Kind::Resource, id, &res), Ok(()));
        let res = props(json!({"name": "D", "body": {"type": "plant_style", "common_name": 5}}));
        assert!(validate_entity(Kind::Resource, id, &res).is_err());
        assert_eq!(
            validate_entity(Kind::Document, Uid::NIL, &Map::new()),
            Ok(())
        );
        let bad = props(json!({"drawing": 5}));
        assert!(validate_entity(Kind::Document, Uid::NIL, &bad).is_err());
        let bad = props(json!({"georef": 5}));
        assert!(validate_entity(Kind::Document, Uid::NIL, &bad).is_err());
        // The key of the stacking order: a good key passes, a hostile one does not.
        let mut o = rect();
        o.insert("order".into(), json!("a0V"));
        assert_eq!(validate_entity(Kind::Object, id, &o), Ok(()));
        for hostile in [
            json!("~"),
            json!(""),
            json!("a0V0"),
            json!(5),
            json!("a".repeat(2000)),
        ] {
            o.insert("order".into(), hostile);
            assert!(validate_entity(Kind::Object, id, &o).is_err());
        }
        let mut l = layer.clone();
        l.insert("order".into(), json!("~"));
        assert!(validate_entity(Kind::Layer, id, &l).is_err());
    }

    fn create(id: u64, p: Map<String, Value>, merge: bool) -> Op {
        Op::Create {
            target: Target::new(Kind::Object, Uid::from_num(id)),
            props: p.into_iter().collect(),
            merge,
        }
    }

    fn replica_with(id: u64) -> Replica {
        let mut r = Replica::new();
        r.apply(
            crate::op::Writer {
                client: Uid::from_num(9),
                op: 1,
            },
            &create(id, rect(), false),
        );
        r
    }

    fn no_blobs(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn transactions_with_a_used_id_or_a_bad_thing_are_refused() {
        let r = replica_with(5);
        let ok = [create(6, rect(), false)];
        assert_eq!(check_transaction(&r, &ok, &no_blobs), Ok(()));
        // An ID that exists, or that the transaction makes twice.
        let again = [create(5, rect(), false)];
        assert_eq!(
            check_transaction(&r, &again, &no_blobs),
            Err(CheckError::DuplicateUid(Uid::from_num(5)))
        );
        let twice = [create(6, rect(), false), create(6, rect(), false)];
        assert_eq!(
            check_transaction(&r, &twice, &no_blobs),
            Err(CheckError::DuplicateUid(Uid::from_num(6)))
        );
        // A deleted thing keeps its ID.
        let mut gone = replica_with(5);
        gone.apply(
            crate::op::Writer {
                client: Uid::from_num(9),
                op: 2,
            },
            &Op::Delete {
                target: Target::new(Kind::Object, Uid::from_num(5)),
            },
        );
        assert!(check_transaction(&gone, &again, &no_blobs).is_err());
        // With `merge` a repeat is fine, also for a deleted thing.
        let merge = [create(5, rect(), true)];
        assert_eq!(check_transaction(&r, &merge, &no_blobs), Ok(()));
        assert_eq!(check_transaction(&gone, &merge, &no_blobs), Ok(()));
        assert_eq!(check_duplicates(&r, &ok), Ok(()));
        // A bad value, in a new thing and in an edit of an old one.
        let mut far = rect();
        far.insert(
            "shape".into(),
            json!({"kind": "rect", "min": [0, 0], "max": [5e12, 5]}),
        );
        assert!(matches!(
            check_transaction(&r, &[create(7, far.clone(), false)], &no_blobs),
            Err(CheckError::Invalid(_))
        ));
        let edit = [Op::Set {
            target: Target::new(Kind::Object, Uid::from_num(5)),
            prop: "shape".into(),
            value: far["shape"].clone(),
        }];
        assert!(check_transaction(&r, &edit, &no_blobs).is_err());
        // An edit of a thing that is not there is dropped by the merge, not refused.
        let unknown = [Op::Set {
            target: Target::new(Kind::Object, Uid::from_num(77)),
            prop: "shape".into(),
            value: far["shape"].clone(),
        }];
        assert_eq!(check_transaction(&r, &unknown, &no_blobs), Ok(()));
        // A later edit in the same transaction sees the thing that was made before it.
        let both = [
            create(8, rect(), false),
            Op::Set {
                target: Target::new(Kind::Object, Uid::from_num(8)),
                prop: "shape".into(),
                value: far["shape"].clone(),
            },
        ];
        assert!(check_transaction(&r, &both, &no_blobs).is_err());
    }

    #[test]
    fn a_merge_create_of_a_living_thing_is_checked_on_top_of_its_properties() {
        let r = replica_with(5);
        // Alone, `{"order": "a0"}` is not an object. On top of the living thing it is fine.
        let part: Map<String, Value> = props(json!({"order": "a0"}));
        assert!(validate_entity(Kind::Object, Uid::from_num(5), &part).is_err());
        assert_eq!(
            check_transaction(&r, &[create(5, part.clone(), true)], &no_blobs),
            Ok(())
        );
        // A bad value on top of it is still refused.
        let bad = props(json!({"order": "~"}));
        assert!(check_transaction(&r, &[create(5, bad, true)], &no_blobs).is_err());
        // For a thing that does not exist yet, a merge create must be complete on its own.
        assert!(check_transaction(&r, &[create(6, part, true)], &no_blobs).is_err());
    }

    fn underlay(hash: &str) -> Value {
        json!({"kind": "underlay", "png": {"$blob": hash}})
    }

    #[test]
    fn blobs_are_listed_and_checked() {
        let mut p = rect();
        p.insert("shape".into(), underlay("aa"));
        let r = Replica::new();
        let ops = [create(5, p.clone(), false), create(6, p, false)];
        assert_eq!(blobs_needed(&r, &ops), vec!["aa".to_string()]);
        assert_eq!(
            check_transaction(&r, &ops, &no_blobs),
            Err(CheckError::MissingBlob("aa".into()))
        );
        assert!(blobs_needed(&r, &[create(7, rect(), false)]).is_empty());

        // An edit of another property of a thing with a picture needs the picture too.
        let mut r = Replica::new();
        let mut with = rect();
        with.insert("shape".into(), underlay("bb"));
        r.apply(
            crate::op::Writer {
                client: Uid::from_num(9),
                op: 1,
            },
            &create(5, with, false),
        );
        let edit = [Op::Set {
            target: Target::new(Kind::Object, Uid::from_num(5)),
            prop: "order".into(),
            value: json!("a0"),
        }];
        assert_eq!(blobs_needed(&r, &edit), vec!["bb".to_string()]);
        assert_eq!(
            check_transaction(&r, &edit, &no_blobs),
            Err(CheckError::MissingBlob("bb".into()))
        );
        // A thing that is not there needs nothing.
        let unknown = [Op::Set {
            target: Target::new(Kind::Object, Uid::from_num(77)),
            prop: "order".into(),
            value: json!("a0"),
        }];
        assert!(blobs_needed(&r, &unknown).is_empty());

        // Every blob that the replica holds, deleted things included.
        assert_eq!(referenced_blobs(&r), BTreeSet::from(["bb".to_string()]));
        r.apply(
            crate::op::Writer {
                client: Uid::from_num(9),
                op: 2,
            },
            &Op::Delete {
                target: Target::new(Kind::Object, Uid::from_num(5)),
            },
        );
        assert_eq!(referenced_blobs(&r).len(), 1, "an undo can bring it back");
        assert!(referenced_blobs(&Replica::new()).is_empty());
    }

    #[test]
    fn finds_ids_in_messages() {
        let u = Uid::from_num(0x1234);
        assert_eq!(find_uid(&format!("object {u} has a problem")), Some(u));
        assert_eq!(find_uid("no id here"), None);
        assert_eq!(find_uid("é"), None);
    }
}
