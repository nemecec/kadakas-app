//! The replica: the document as a set of things with properties, and the merge rules.
//!
//! Every client and the server keep a [`Replica`]. All of them apply the same transactions in the
//! same order (the order of the server sequence numbers), with the same rules. The rules are
//! deterministic, so all replicas end in the same state. That is the whole merge: there is no
//! other conflict step.
//!
//! # The rules
//!
//! - **Last writer wins per property.** `Set` and `Unset` replace the value of one property.
//!   Because replicas apply operations in server order, the operation with the higher sequence
//!   number wins. Two users who change different properties of one object both keep their
//!   changes. Two users who change the same property keep the later one.
//! - **Delete wins over edit.** `Delete` marks the thing as deleted. The thing keeps its ID (a
//!   tombstone), so the ID is never used again. A `Set`, `Unset` or `Delete` that comes after the
//!   deletion is dropped, also when its author wrote it before they saw the deletion.
//! - **Duplicate IDs are refused.** `Create` fails when the ID exists or ever existed. UUIDv7 IDs
//!   make this impossible by accident. The rule still stands, so a faulty or hostile client cannot
//!   overwrite a thing.
//! - **Restore is for undo only.** `Restore` brings a deleted thing back, but only if the
//!   deletion was written by the writer that the restore names. If another user deleted it too, or
//!   deleted it instead, the restore is dropped: the other user's deletion stands.
//! - **Protected things** (the document settings, the first layer, the "None" class) cannot be
//!   deleted.
//! - **Unknown targets** make the operation drop. This happens only when a client breaks the
//!   causal order, because a client sees a thing before it edits it.
//!
//! A dropped operation is not an error: it is part of the merge, and every replica drops the same
//! operations. [`Outcome::Dropped`] says why.
//!
//! # Layers, classes and resources
//!
//! They are things like objects. They have properties (`name`, `visible`, `body`, ...) that
//! follow the same rules. Objects point to layers and classes by ID, and shapes can point to
//! resources. A reference can break when one user deletes a layer, class or resource while
//! another uses it. The replica does not stop this: it keeps the reference as it is. When the
//! replica is turned into a document (see `docsync::materialize`), an object whose layer or
//! class is gone moves to the first layer or the "None" class, and a missing group parent is
//! cleared. The objects are never lost. Two layers with the same name are fine: the ID is the
//! identity, not the name.
//!
//! The order of objects (z-order) and of layers is not part of the replica. A document keeps the
//! order it has, and new things go to the end, ordered by ID (which is by creation time).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::op::{Kind, Op, Target, Writer};

/// True if two JSON values are the same, with numbers compared as numbers: `1` and `1.0` are
/// the same. Clients use it to decide if a property changed, so a program that writes `1.0` for
/// `1` makes no edit. (Servers do not compare: they relay the JSON text unchanged.)
pub fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(p), Some(q)) => p == q,
            _ => x == y,
        },
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_json(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, p)| y.get(k).is_some_and(|q| same_json(p, q)))
        }
        _ => a == b,
    }
}

/// The value of one property, and who wrote it last.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    pub value: Value,
    pub writer: Writer,
    /// True after `Unset`: the property is gone, but its writer is still known.
    #[serde(default, skip_serializing_if = "is_false")]
    pub removed: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One thing: its properties, who made it and, if it is deleted, who deleted it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub created_by: Writer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_by: Option<Writer>,
    pub props: BTreeMap<String, Slot>,
}

impl Entity {
    fn new(created_by: Writer) -> Entity {
        Entity {
            created_by,
            deleted_by: None,
            props: BTreeMap::new(),
        }
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted_by.is_some()
    }

    /// The value of a property, if it is set.
    pub fn get(&self, prop: &str) -> Option<&Value> {
        self.props
            .get(prop)
            .filter(|s| !s.removed)
            .map(|s| &s.value)
    }

    /// The visible properties as a JSON object.
    pub fn visible(&self) -> Map<String, Value> {
        self.props
            .iter()
            .filter(|(_, s)| !s.removed)
            .map(|(k, s)| (k.clone(), s.value.clone()))
            .collect()
    }
}

/// Why an operation changed nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drop {
    /// The thing does not exist.
    UnknownTarget,
    /// The thing is deleted.
    TargetDeleted,
    /// `Create` with an ID that exists or existed.
    DuplicateUid,
    /// The thing cannot be deleted.
    Protected,
    /// `Restore` of a thing that is not deleted.
    NotDeleted,
    /// `Restore` that names another writer than the one who deleted the thing.
    StaleRestore,
    /// The operation is malformed (a property named `id`, or a created document).
    BadOp,
}

/// What an operation did to the replica.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Applied,
    Dropped(Drop),
}

impl Outcome {
    pub fn is_applied(self) -> bool {
        self == Outcome::Applied
    }
}

/// The state of a document as things with properties. Serialized, it is the snapshot of the
/// protocol: a list of things in the order of their keys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Wire", into = "Wire")]
pub struct Replica {
    things: BTreeMap<Target, Entity>,
}

/// The wire form of a replica: a sorted list, because JSON keys are text.
#[derive(Serialize, Deserialize)]
struct Wire {
    things: Vec<WireThing>,
}

#[derive(Serialize, Deserialize)]
struct WireThing {
    #[serde(flatten)]
    target: Target,
    #[serde(flatten)]
    entity: Entity,
}

impl From<Replica> for Wire {
    fn from(r: Replica) -> Wire {
        Wire {
            things: r
                .things
                .into_iter()
                .map(|(target, entity)| WireThing { target, entity })
                .collect(),
        }
    }
}

impl TryFrom<Wire> for Replica {
    type Error = String;

    fn try_from(w: Wire) -> Result<Replica, String> {
        let mut things = BTreeMap::new();
        for t in w.things {
            if things.insert(t.target, t.entity).is_some() {
                return Err(format!("the snapshot has {:?} twice", t.target));
            }
        }
        if !things.contains_key(&Target::DOCUMENT) {
            return Err("the snapshot has no document settings".into());
        }
        Ok(Replica { things })
    }
}

impl Default for Replica {
    fn default() -> Self {
        Replica::new()
    }
}

impl Replica {
    /// An empty replica: only the document settings thing, without properties.
    pub fn new() -> Replica {
        let mut things = BTreeMap::new();
        things.insert(Target::DOCUMENT, Entity::new(Writer::BASELINE));
        Replica { things }
    }

    /// Makes a thing with the baseline writer. For the first content of a document.
    pub fn insert_baseline(&mut self, target: Target, props: Map<String, Value>) {
        let mut e = Entity::new(Writer::BASELINE);
        for (k, v) in props {
            e.props.insert(
                k,
                Slot {
                    value: v,
                    writer: Writer::BASELINE,
                    removed: false,
                },
            );
        }
        self.things.insert(target, e);
    }

    pub fn get(&self, target: Target) -> Option<&Entity> {
        self.things.get(&target)
    }

    /// The thing, if it exists and is not deleted.
    pub fn alive(&self, target: Target) -> Option<&Entity> {
        self.things.get(&target).filter(|e| !e.is_deleted())
    }

    /// True if the ID is used: the thing exists or was deleted.
    pub fn contains(&self, target: Target) -> bool {
        self.things.contains_key(&target)
    }

    /// All things, also the deleted ones, in key order.
    pub fn things(&self) -> impl Iterator<Item = (&Target, &Entity)> {
        self.things.iter()
    }

    /// The things that are not deleted, with their kind, in key order.
    pub fn alive_things(&self) -> impl Iterator<Item = (&Target, &Entity)> {
        self.things.iter().filter(|(_, e)| !e.is_deleted())
    }

    /// The things of one kind that are not deleted, in the order of their IDs.
    pub fn alive_of(&self, kind: Kind) -> impl Iterator<Item = (&Target, &Entity)> {
        self.alive_things().filter(move |(t, _)| t.kind == kind)
    }

    /// How many things there are, with the deleted ones.
    pub fn len(&self) -> usize {
        self.things.len()
    }

    pub fn is_empty(&self) -> bool {
        self.things.is_empty()
    }

    /// Applies one operation of a writer. Returns what happened.
    pub fn apply(&mut self, writer: Writer, op: &Op) -> Outcome {
        self.apply_inverse(writer, op).0
    }

    /// Applies a list of operations of one writer.
    pub fn apply_all(&mut self, writer: Writer, ops: &[Op]) -> Vec<Outcome> {
        ops.iter().map(|op| self.apply(writer, op)).collect()
    }

    /// Like [`Replica::apply`], and also returns the operations that take the change back (empty
    /// if the operation was dropped or changed nothing). The client uses them for undo.
    pub fn apply_inverse(&mut self, writer: Writer, op: &Op) -> (Outcome, Vec<Op>) {
        let drop = |d| (Outcome::Dropped(d), Vec::new());
        match op {
            Op::Create { target, props } => {
                if target.kind == Kind::Document || props.contains_key("id") {
                    return drop(Drop::BadOp);
                }
                if self.things.contains_key(target) {
                    return drop(Drop::DuplicateUid);
                }
                let mut e = Entity::new(writer);
                for (k, v) in props {
                    e.props.insert(
                        k.clone(),
                        Slot {
                            value: v.clone(),
                            writer,
                            removed: false,
                        },
                    );
                }
                self.things.insert(*target, e);
                (Outcome::Applied, vec![Op::Delete { target: *target }])
            }
            Op::Delete { target } => {
                if target.is_protected() {
                    return drop(Drop::Protected);
                }
                match self.things.get_mut(target) {
                    None => drop(Drop::UnknownTarget),
                    Some(e) if e.is_deleted() => drop(Drop::TargetDeleted),
                    Some(e) => {
                        e.deleted_by = Some(writer);
                        (
                            Outcome::Applied,
                            vec![Op::Restore {
                                target: *target,
                                deleted_by: writer,
                            }],
                        )
                    }
                }
            }
            Op::Restore { target, deleted_by } => match self.things.get_mut(target) {
                None => drop(Drop::UnknownTarget),
                Some(e) => match e.deleted_by {
                    None => drop(Drop::NotDeleted),
                    Some(w) if w != *deleted_by => drop(Drop::StaleRestore),
                    Some(_) => {
                        e.deleted_by = None;
                        (Outcome::Applied, vec![Op::Delete { target: *target }])
                    }
                },
            },
            Op::Set {
                target,
                prop,
                value,
            } => self.write_prop(writer, *target, prop, Some(value)),
            Op::Unset { target, prop } => self.write_prop(writer, *target, prop, None),
        }
    }

    fn write_prop(
        &mut self,
        writer: Writer,
        target: Target,
        prop: &str,
        value: Option<&Value>,
    ) -> (Outcome, Vec<Op>) {
        let drop = |d| (Outcome::Dropped(d), Vec::new());
        if prop == "id" {
            return drop(Drop::BadOp);
        }
        let Some(e) = self.things.get_mut(&target) else {
            return drop(Drop::UnknownTarget);
        };
        if e.is_deleted() {
            return drop(Drop::TargetDeleted);
        }
        let before = e.get(prop).cloned();
        let inverse = match before {
            Some(v) => Op::Set {
                target,
                prop: prop.to_string(),
                value: v,
            },
            None => Op::Unset {
                target,
                prop: prop.to_string(),
            },
        };
        e.props.insert(
            prop.to_string(),
            Slot {
                value: value.cloned().unwrap_or(Value::Null),
                writer,
                removed: value.is_none(),
            },
        );
        (Outcome::Applied, vec![inverse])
    }

    /// The state as canonical JSON: things sorted by kind and ID, properties sorted by name. Two
    /// replicas are equal when this JSON text is equal. Tests compare it.
    pub fn canonical_json(&self) -> Value {
        json!(self.clone())
    }

    /// [`Replica::canonical_json`] as text.
    pub fn canonical_text(&self) -> String {
        self.canonical_json().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::model::{FIRST_LAYER, NONE_CLASS};
    use kadakas_core::uid::Uid;

    fn obj(n: u64) -> Target {
        Target::new(Kind::Object, Uid::from_num(n))
    }

    fn w(client: u64, op: u64) -> Writer {
        Writer {
            client: Uid::from_num(client),
            op,
        }
    }

    fn create(n: u64, shape: i64) -> Op {
        Op::Create {
            target: obj(n),
            props: [("shape".to_string(), json!(shape))].into(),
        }
    }

    fn set(n: u64, prop: &str, v: i64) -> Op {
        Op::Set {
            target: obj(n),
            prop: prop.into(),
            value: json!(v),
        }
    }

    #[test]
    fn numbers_compare_as_numbers() {
        assert!(same_json(&json!(1), &json!(1.0)));
        assert!(same_json(&json!([1, {"a": 2.0}]), &json!([1.0, {"a": 2}])));
        assert!(!same_json(&json!(1), &json!(1.5)));
        assert!(!same_json(&json!([1]), &json!([1, 2])));
        assert!(!same_json(&json!({"a": 1}), &json!({"b": 1})));
        assert!(!same_json(&json!({"a": 1}), &json!({"a": 1, "b": 1})));
        assert!(same_json(&json!("x"), &json!("x")));
        assert!(!same_json(&json!(1), &json!("1")));
        assert!(same_json(&json!(u64::MAX), &json!(u64::MAX)));
    }

    #[test]
    fn create_set_unset() {
        let mut r = Replica::new();
        assert_eq!(r.apply(w(1, 1), &create(1, 5)), Outcome::Applied);
        assert!(r.apply(w(1, 2), &set(1, "shape", 6)).is_applied());
        assert!(r.apply(w(1, 3), &set(1, "color", 1)).is_applied());
        let e = r.alive(obj(1)).unwrap();
        assert_eq!(e.get("shape"), Some(&json!(6)));
        assert_eq!(e.props["shape"].writer, w(1, 2));
        let un = Op::Unset {
            target: obj(1),
            prop: "color".into(),
        };
        assert!(r.apply(w(2, 1), &un).is_applied());
        let e = r.alive(obj(1)).unwrap();
        assert_eq!(e.get("color"), None);
        assert!(e.props["color"].removed);
        assert_eq!(e.visible().len(), 1);
        assert_eq!(r.len(), 2);
        assert!(!r.is_empty());
    }

    #[test]
    fn last_writer_wins_per_property() {
        // Two users change one property and two different properties, in server order.
        let mut r = Replica::new();
        r.apply(w(1, 1), &create(1, 0));
        r.apply(w(1, 2), &set(1, "a", 1));
        r.apply(w(2, 1), &set(1, "a", 2));
        r.apply(w(1, 3), &set(1, "b", 3));
        r.apply(w(2, 2), &set(1, "c", 4));
        let e = r.alive(obj(1)).unwrap();
        assert_eq!(e.get("a"), Some(&json!(2)));
        assert_eq!(e.get("b"), Some(&json!(3)));
        assert_eq!(e.get("c"), Some(&json!(4)));
    }

    #[test]
    fn delete_wins_over_edit() {
        let mut r = Replica::new();
        r.apply(w(1, 1), &create(1, 0));
        assert!(
            r.apply(w(2, 1), &Op::Delete { target: obj(1) })
                .is_applied()
        );
        assert_eq!(
            r.apply(w(1, 2), &set(1, "shape", 9)),
            Outcome::Dropped(Drop::TargetDeleted)
        );
        assert_eq!(
            r.apply(
                w(1, 3),
                &Op::Unset {
                    target: obj(1),
                    prop: "shape".into()
                }
            ),
            Outcome::Dropped(Drop::TargetDeleted)
        );
        assert_eq!(
            r.apply(w(3, 1), &Op::Delete { target: obj(1) }),
            Outcome::Dropped(Drop::TargetDeleted)
        );
        assert!(r.alive(obj(1)).is_none());
        assert!(r.get(obj(1)).unwrap().is_deleted());
        assert_eq!(r.get(obj(1)).unwrap().get("shape"), Some(&json!(0)));
        assert_eq!(r.alive_things().count(), 1);
    }

    #[test]
    fn duplicate_create_is_refused_also_after_delete() {
        let mut r = Replica::new();
        assert!(r.apply(w(1, 1), &create(1, 0)).is_applied());
        assert_eq!(
            r.apply(w(2, 1), &create(1, 7)),
            Outcome::Dropped(Drop::DuplicateUid)
        );
        assert_eq!(r.alive(obj(1)).unwrap().get("shape"), Some(&json!(0)));
        r.apply(w(1, 2), &Op::Delete { target: obj(1) });
        assert_eq!(
            r.apply(w(2, 2), &create(1, 7)),
            Outcome::Dropped(Drop::DuplicateUid)
        );
        assert!(r.contains(obj(1)));
    }

    #[test]
    fn malformed_operations_drop() {
        let mut r = Replica::new();
        let doc = Op::Create {
            target: Target::DOCUMENT,
            props: BTreeMap::new(),
        };
        assert_eq!(r.apply(w(1, 1), &doc), Outcome::Dropped(Drop::BadOp));
        let with_id = Op::Create {
            target: obj(1),
            props: [("id".to_string(), json!(1))].into(),
        };
        assert_eq!(r.apply(w(1, 1), &with_id), Outcome::Dropped(Drop::BadOp));
        r.apply(w(1, 1), &create(1, 0));
        assert_eq!(
            r.apply(w(1, 2), &set(1, "id", 3)),
            Outcome::Dropped(Drop::BadOp)
        );
    }

    #[test]
    fn unknown_targets_drop() {
        let mut r = Replica::new();
        assert_eq!(
            r.apply(w(1, 1), &set(9, "a", 1)),
            Outcome::Dropped(Drop::UnknownTarget)
        );
        assert_eq!(
            r.apply(w(1, 1), &Op::Delete { target: obj(9) }),
            Outcome::Dropped(Drop::UnknownTarget)
        );
        let restore = Op::Restore {
            target: obj(9),
            deleted_by: w(1, 1),
        };
        assert_eq!(
            r.apply(w(1, 1), &restore),
            Outcome::Dropped(Drop::UnknownTarget)
        );
    }

    #[test]
    fn protected_things_stay() {
        let mut r = Replica::new();
        r.insert_baseline(Target::new(Kind::Layer, FIRST_LAYER.0), Map::new());
        r.insert_baseline(Target::new(Kind::Class, NONE_CLASS.0), Map::new());
        for t in [
            Target::DOCUMENT,
            Target::new(Kind::Layer, FIRST_LAYER.0),
            Target::new(Kind::Class, NONE_CLASS.0),
        ] {
            assert_eq!(
                r.apply(w(1, 1), &Op::Delete { target: t }),
                Outcome::Dropped(Drop::Protected)
            );
        }
        // Other layers can go.
        let layer = Target::new(Kind::Layer, Uid::from_num(5));
        r.apply(
            w(1, 1),
            &Op::Create {
                target: layer,
                props: BTreeMap::new(),
            },
        );
        assert!(r.apply(w(1, 2), &Op::Delete { target: layer }).is_applied());
    }

    #[test]
    fn restore_needs_the_same_deleter() {
        let mut r = Replica::new();
        r.apply(w(1, 1), &create(1, 0));
        let (out, inv) = r.apply_inverse(w(1, 2), &Op::Delete { target: obj(1) });
        assert!(out.is_applied());
        assert_eq!(
            inv,
            vec![Op::Restore {
                target: obj(1),
                deleted_by: w(1, 2)
            }]
        );
        // Another user's restore does not match.
        let bad = Op::Restore {
            target: obj(1),
            deleted_by: w(2, 1),
        };
        assert_eq!(r.apply(w(2, 2), &bad), Outcome::Dropped(Drop::StaleRestore));
        assert!(r.apply(w(1, 3), &inv[0]).is_applied());
        assert!(r.alive(obj(1)).is_some());
        assert_eq!(
            r.apply(w(1, 4), &inv[0]),
            Outcome::Dropped(Drop::NotDeleted)
        );
        // The inverse of a restore deletes again.
        let (_, inv2) = r.apply_inverse(w(1, 5), &Op::Delete { target: obj(1) });
        let (_, inv3) = r.apply_inverse(w(1, 6), &inv2[0]);
        assert_eq!(inv3, vec![Op::Delete { target: obj(1) }]);
    }

    #[test]
    fn inverses_of_properties() {
        let mut r = Replica::new();
        let (_, inv) = r.apply_inverse(w(1, 1), &create(1, 0));
        assert_eq!(inv, vec![Op::Delete { target: obj(1) }]);
        let (_, inv) = r.apply_inverse(w(1, 2), &set(1, "shape", 4));
        assert_eq!(inv, vec![set(1, "shape", 0)]);
        let (_, inv) = r.apply_inverse(w(1, 3), &set(1, "new", 4));
        assert_eq!(
            inv,
            vec![Op::Unset {
                target: obj(1),
                prop: "new".into()
            }]
        );
        let (out, inv) = r.apply_inverse(w(1, 4), &set(7, "x", 1));
        assert!(!out.is_applied());
        assert!(inv.is_empty());
    }

    #[test]
    fn canonical_json_round_trips_and_is_sorted() {
        let mut r = Replica::new();
        r.apply(w(1, 1), &create(2, 0));
        r.apply(w(1, 2), &create(1, 0));
        r.apply(w(1, 3), &Op::Delete { target: obj(2) });
        r.apply(
            w(1, 4),
            &Op::Unset {
                target: obj(1),
                prop: "shape".into(),
            },
        );
        let text = r.canonical_text();
        let back: Replica = serde_json::from_str(&text).unwrap();
        assert_eq!(back, r);
        assert_eq!(back.canonical_text(), text);
        let list = r.canonical_json()["things"].as_array().unwrap().clone();
        assert_eq!(list.len(), 3);
        // The document settings (kind `document`) sort after objects: kinds sort by declaration.
        assert_eq!(list[0]["kind"], "object");
        assert_eq!(list[2]["kind"], "document");
        assert_eq!(Replica::default(), Replica::new());
    }

    #[test]
    fn bad_snapshots_are_refused() {
        let twice = json!({"things": [
            {"kind": "document", "id": Uid::NIL, "created_by": Writer::BASELINE, "props": {}},
            {"kind": "document", "id": Uid::NIL, "created_by": Writer::BASELINE, "props": {}},
        ]});
        assert!(serde_json::from_value::<Replica>(twice).is_err());
        let none = json!({"things": []});
        assert!(serde_json::from_value::<Replica>(none).is_err());
    }

    #[test]
    fn alive_of_filters_kind() {
        let mut r = Replica::new();
        r.apply(w(1, 1), &create(1, 0));
        let layer = Target::new(Kind::Layer, Uid::from_num(3));
        r.apply(
            w(1, 2),
            &Op::Create {
                target: layer,
                props: BTreeMap::new(),
            },
        );
        assert_eq!(r.alive_of(Kind::Object).count(), 1);
        assert_eq!(r.alive_of(Kind::Layer).count(), 1);
        assert_eq!(r.alive_of(Kind::Class).count(), 0);
        let outs = r.apply_all(w(1, 3), &[set(1, "a", 1), set(8, "a", 1)]);
        assert_eq!(outs.iter().filter(|o| o.is_applied()).count(), 1);
    }
}
