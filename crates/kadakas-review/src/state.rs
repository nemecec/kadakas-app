//! The state of a document as things with properties, and the operations between two states.
//!
//! A [`State`] is what a version holds: the alive things (objects, layers, classes, resources and
//! the document settings) by [`Target`], each with its flat properties. The cut into properties
//! is the one of collaboration (`kadakas_props::docsync`: `attrs.<field>`, `data.<key>`,
//! `drawing.<field>`, the points of a site model in chunks), so a diff here and a sync agree on
//! what one change is.
//!
//! A state has no writers and no tombstones. Collaboration needs them to merge, and a history does
//! not: a version is one fixed state, and an object that was deleted and came back with the same
//! ID (an undo) is just a thing that exists again. So the operations between two states use the
//! `Op` type of `kadakas-props`, but are applied with the plain rules of [`State::apply`].
//!
//! # Pictures
//!
//! A big picture (the PNG of an underlay, the file of an image resource) is not a value of a
//! property. Like in collaboration (`kadakas_props::blob`), the property holds a reference
//! `{"$blob": "<sha256>"}` and the text of the picture is in a table of the state, once for each
//! hash. A move of a 10 MiB underlay therefore changes a few bytes in a delta, not 10 MiB, and a
//! diff shows the reference. [`State::build`] puts the pictures back.
//!
//! The stacking order is the property `order` (a key that sorts, see `kadakas_props::order`).
//! [`State::of`] takes the key of a thing from the state before (`prev`) if it has one there, else
//! from the key that the document itself carries (the field `order` in `extra`, see
//! [`State::stamp_keys`]), else it makes one that fits between the neighbors. A thing that kept
//! its place keeps its key, and adding an object does not change the keys of the others.

use std::collections::BTreeMap;
use std::sync::Arc;

use kadakas_core::model::{Document, LayerId};
use kadakas_props::blob::{BLOB_PROPS, blob_ref, externalize};
use kadakas_props::docsync::replica_of;
use kadakas_props::materialize::{Materialized, materialize_with};
use kadakas_props::op::{Kind, Op, Target, op_size, validate_ops};
use kadakas_props::order::{rekey, valid_key};
use kadakas_props::replica::{Replica, same_json};
use serde_json::{Map, Value};

/// The flat properties of one thing.
pub type Props = Map<String, Value>;

/// Why an operation does not fit a state.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ApplyError {
    #[error("{0:?} exists already")]
    Exists(Target),
    #[error("{0:?} does not exist")]
    Missing(Target),
    #[error("the operation is not allowed in a history")]
    Unsupported,
    #[error("the operation is not well formed: {0}")]
    Invalid(String),
}

#[derive(Clone, Debug)]
pub struct State {
    things: BTreeMap<Target, Props>,
    /// The pictures that the properties refer to, by the SHA-256 of their text. The table can hold
    /// more than the things use (a history keeps the pictures of all its versions in one table).
    blobs: BTreeMap<String, Arc<str>>,
}

#[cfg(test)]
thread_local! {
    /// How many operations this thread applied to states, for tests of the work.
    pub(crate) static APPLIED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Default for State {
    fn default() -> Self {
        State::new()
    }
}

impl PartialEq for State {
    fn eq(&self, other: &State) -> bool {
        self.things.len() == other.things.len()
            && self
                .things
                .iter()
                .zip(&other.things)
                .all(|((ta, pa), (tb, pb))| ta == tb && same_props(pa, pb))
    }
}

/// True if two property maps hold the same values (`1` and `1.0` are the same).
pub fn same_props(a: &Props, b: &Props) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|(k, v)| b.get(k).is_some_and(|w| same_json(v, w)))
}

impl State {
    /// A state with only the settings of the document, without properties.
    pub fn new() -> State {
        State {
            things: BTreeMap::from([(Target::DOCUMENT, Props::new())]),
            blobs: BTreeMap::new(),
        }
    }

    /// Reads a document. `prev` is the state before, for the stacking keys (see the module text).
    pub fn of(doc: &Document, prev: Option<&State>) -> State {
        let replica = replica_of(doc);
        let mut things = BTreeMap::new();
        // The pictures of issue snapshots are review data too: a restore keeps them.
        let snapshots: std::collections::BTreeSet<&str> =
            doc.issues.snapshots().into_iter().collect();
        // A picture that a plant style uses is part of the drawing and of the version, also when
        // an issue uses it as a snapshot too.
        let styled: std::collections::BTreeSet<kadakas_core::uid::Uid> = replica
            .alive_things()
            .filter(|(t, _)| t.kind == Kind::Resource)
            .filter_map(|(_, e)| {
                e.get("body")
                    .and_then(|b| b.get("image"))
                    .and_then(serde_json::Value::as_str)
                    .and_then(kadakas_core::uid::Uid::parse)
            })
            .collect();
        for (t, e) in replica.alive_things() {
            if t.kind == Kind::Resource
                && !styled.contains(&t.id)
                && e.get("body")
                    .and_then(|b| b.get("sha256"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|h| snapshots.contains(h))
            {
                continue;
            }
            // Issues are review data: a version holds the drawing, and a restore leaves the
            // issues as they are.
            if matches!(t.kind, Kind::Issue | Kind::Comment) {
                continue;
            }
            let mut props = e.visible();
            props.remove("order");
            things.insert(*t, props);
        }
        let mut state = State {
            things,
            blobs: BTreeMap::new(),
        };
        state.take_pictures();
        state.stamp_order(doc, prev);
        state
    }

    /// Replaces the big pictures in the properties by references and keeps their texts.
    fn take_pictures(&mut self) {
        for props in self.things.values_mut() {
            for key in BLOB_PROPS {
                if let Some(v) = props.get_mut(key)
                    && let Some((reference, hash, text)) = externalize(v)
                {
                    *v = reference;
                    self.blobs.insert(hash, Arc::from(text));
                }
            }
        }
    }

    /// The pictures of the state, by hash.
    pub fn blobs(&self) -> &BTreeMap<String, Arc<str>> {
        &self.blobs
    }

    /// Gives the state a table of pictures (the one of a history).
    pub fn set_blobs(&mut self, blobs: BTreeMap<String, Arc<str>>) {
        self.blobs = blobs;
    }

    /// The hashes of the pictures that the things of the state use.
    pub fn used_blobs(&self) -> Vec<&str> {
        self.things
            .values()
            .flat_map(|p| BLOB_PROPS.iter().filter_map(|k| p.get(*k)))
            .filter_map(blob_ref)
            .collect()
    }

    /// Writes the stacking keys of this state into the objects and layers of a document (the
    /// field `order` in `extra`), so that `State::of(doc, None)` gives this state back. A file
    /// that keeps its versions as a change from the document needs the keys in the document.
    pub fn stamp_keys(&self, doc: &mut Document) {
        let key = |t: &Target| {
            self.things
                .get(t)
                .and_then(|p| p.get("order"))
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        for o in doc.objects.values_mut() {
            if let Some(k) = key(&Target::new(Kind::Object, o.id.0)) {
                o.extra.insert("order".into(), Value::String(k));
            }
        }
        for l in &mut doc.layers {
            if let Some(k) = key(&Target::new(Kind::Layer, l.id.0)) {
                l.extra.insert("order".into(), Value::String(k));
            }
        }
    }

    /// True if two states have the same things with the same properties and the same stacking
    /// order in each list, whatever the keys are.
    pub fn same_content(&self, other: &State) -> bool {
        let strip = |s: &State| {
            let mut s = s.clone();
            for p in s.things.values_mut() {
                p.remove("order");
            }
            s
        };
        strip(self) == strip(other) && crate::diff::moved_things(self, other).is_empty()
    }

    fn stamp_order(&mut self, doc: &Document, prev: Option<&State>) {
        let stored = |t: &Target, extra: &Props| {
            let _ = t;
            extra
                .get("order")
                .and_then(Value::as_str)
                .filter(|k| valid_key(k))
                .map(str::to_string)
        };
        let mut carried: std::collections::HashMap<Target, String> = Default::default();
        for o in doc.objects.values() {
            let t = Target::new(Kind::Object, o.id.0);
            if let Some(k) = stored(&t, &o.extra) {
                carried.insert(t, k);
            }
        }
        for l in &doc.layers {
            let t = Target::new(Kind::Layer, l.id.0);
            if let Some(k) = stored(&t, &l.extra) {
                carried.insert(t, k);
            }
        }
        let mut lists: Vec<Vec<Target>> = vec![
            doc.layers
                .iter()
                .map(|l| Target::new(Kind::Layer, l.id.0))
                .collect(),
        ];
        let mut per_layer: BTreeMap<LayerId, Vec<Target>> = BTreeMap::new();
        for o in doc.objects.values() {
            per_layer
                .entry(o.layer)
                .or_default()
                .push(Target::new(Kind::Object, o.id.0));
        }
        lists.extend(per_layer.into_values());
        for list in lists {
            let keys: Vec<Option<String>> = list
                .iter()
                .map(|t| {
                    prev.and_then(|p| p.things.get(t))
                        .and_then(|m| m.get("order"))
                        .and_then(Value::as_str)
                        .filter(|k| valid_key(k))
                        .map(str::to_string)
                        .or_else(|| carried.get(t).cloned())
                })
                .collect();
            let mut keys = keys;
            for (i, k) in rekey(&keys) {
                keys[i] = Some(k);
            }
            for (t, k) in list.iter().zip(keys) {
                if let (Some(props), Some(k)) = (self.things.get_mut(t), k) {
                    props.insert("order".into(), Value::String(k));
                }
            }
        }
    }

    pub fn get(&self, t: &Target) -> Option<&Props> {
        self.things.get(t)
    }

    pub fn things(&self) -> impl Iterator<Item = (&Target, &Props)> {
        self.things.iter()
    }

    /// How many things of a part of the document the state has.
    pub fn count(&self, group: crate::diff::Group) -> usize {
        self.things
            .keys()
            .filter(|t| crate::diff::Group::of(t.kind) == group)
            .count()
    }

    pub fn len(&self) -> usize {
        self.things.len()
    }

    pub fn is_empty(&self) -> bool {
        self.things.is_empty()
    }

    /// Applies one operation with the plain rules: `Create` needs a new thing, the others need an
    /// existing one. `Restore` does not exist in a history.
    pub fn apply(&mut self, op: &Op) -> Result<(), ApplyError> {
        #[cfg(test)]
        APPLIED.with(|c| c.set(c.get() + 1));
        match op {
            Op::Create { target, props, .. } => {
                if target.kind == Kind::Document || props.contains_key("id") {
                    return Err(ApplyError::Invalid("a created thing".into()));
                }
                if self.things.contains_key(target) {
                    return Err(ApplyError::Exists(*target));
                }
                self.things
                    .insert(*target, props.clone().into_iter().collect());
            }
            Op::Delete { target } => {
                if target.kind == Kind::Document {
                    return Err(ApplyError::Invalid("the settings cannot be deleted".into()));
                }
                self.things
                    .remove(target)
                    .ok_or(ApplyError::Missing(*target))?;
            }
            Op::Set {
                target,
                prop,
                value,
            } => {
                if prop == "id" {
                    return Err(ApplyError::Invalid("a property named id".into()));
                }
                self.things
                    .get_mut(target)
                    .ok_or(ApplyError::Missing(*target))?
                    .insert(prop.clone(), value.clone());
            }
            Op::Unset { target, prop } => {
                self.things
                    .get_mut(target)
                    .ok_or(ApplyError::Missing(*target))?
                    .remove(prop);
            }
            Op::Restore { .. } => return Err(ApplyError::Unsupported),
        }
        Ok(())
    }

    /// Applies a list of operations. Stops at the first one that does not fit.
    pub fn apply_all(&mut self, ops: &[Op]) -> Result<(), ApplyError> {
        ops.iter().try_for_each(|op| self.apply(op))
    }

    /// The operations that turn `self` into `new`: creates, then edits, then deletes, each group
    /// in the order of the targets. Empty if the states are equal.
    pub fn diff(&self, new: &State) -> Vec<Op> {
        let mut creates = Vec::new();
        let mut edits = Vec::new();
        let mut deletes = Vec::new();
        for (t, props) in &new.things {
            match self.things.get(t) {
                None => creates.push(create_op(*t, props)),
                Some(old) => {
                    for (k, v) in props {
                        if !old.get(k).is_some_and(|o| same_json(o, v)) {
                            edits.push(Op::Set {
                                target: *t,
                                prop: k.clone(),
                                value: v.clone(),
                            });
                        }
                    }
                    for k in old.keys().filter(|k| !props.contains_key(*k)) {
                        edits.push(Op::Unset {
                            target: *t,
                            prop: k.clone(),
                        });
                    }
                }
            }
        }
        for t in self.things.keys() {
            if !new.things.contains_key(t) {
                deletes.push(Op::Delete { target: *t });
            }
        }
        creates.extend(edits);
        creates.extend(deletes);
        creates
    }

    /// The operations that make this state from [`State::new`]: one `Create` for each thing but
    /// the settings, which get `Set` operations. It is the base snapshot of a history.
    pub fn snapshot(&self) -> Vec<Op> {
        State::new().diff(self)
    }

    /// The size in bytes of the JSON of the snapshot, estimated, with the pictures that the
    /// things use (each once).
    pub fn bytes(&self) -> usize {
        let mut used = self.used_blobs();
        used.sort_unstable();
        used.dedup();
        let pictures: usize = used
            .iter()
            .filter_map(|h| self.blobs.get(*h))
            .map(|t| t.len())
            .sum();
        self.snapshot().iter().map(op_size).sum::<usize>() + pictures
    }

    /// The replica for the build of a document. Everything has the baseline writer.
    pub fn to_replica(&self) -> Replica {
        let mut r = Replica::new();
        for (t, props) in &self.things {
            r.insert_baseline(*t, props.clone());
        }
        r
    }

    /// Builds the document of this state. `base` gives what a state does not hold: the active
    /// layer and class, the order of classes and resources, the generator of IDs and the history.
    /// The build is total: a thing that does not fit the file format is left out (see
    /// `Materialized::quarantined`).
    pub fn build(&self, base: &Document) -> Materialized {
        materialize_with(
            &self.to_replica(),
            base,
            &|hash| self.blobs.get(hash).cloned(),
            &mut |_, image| image.verify_data(),
        )
    }
}

fn create_op(target: Target, props: &Props) -> Op {
    Op::Create {
        target,
        props: props.clone().into_iter().collect(),
        merge: false,
    }
}

/// Checks the form of one operation that came from a file: names, depth and size of the values.
pub fn check_op(op: &Op) -> Result<(), String> {
    validate_ops(std::slice::from_ref(op)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::uid::Uid;
    use serde_json::json;

    fn cmd(v: Value) -> Command {
        serde_json::from_value(v).unwrap()
    }

    fn circle(ed: &mut Editor, x: f64) -> Uid {
        ed.execute(&cmd(json!({"op": "add_object", "shape":
            {"kind": "circle", "center": [x, 0.0], "radius": 1.0}})))
            .unwrap()
            .created[0]
    }

    fn obj(id: Uid) -> Target {
        Target::new(Kind::Object, id)
    }

    fn key(s: &State, id: Uid) -> String {
        s.get(&obj(id)).unwrap()["order"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn a_state_has_the_flat_properties_and_keys() {
        let mut ed = Editor::with_sequential_ids();
        let a = circle(&mut ed, 0.0);
        let s = State::of(ed.doc(), None);
        let p = s.get(&obj(a)).unwrap();
        assert!(p.contains_key("shape") && p.contains_key("layer") && p.contains_key("order"));
        assert!(s.get(&Target::DOCUMENT).is_some());
        assert!(!s.is_empty());
        // Layers and the first class are there too.
        assert!(s.things().any(|(t, _)| t.kind == Kind::Layer));
        assert!(s.things().any(|(t, _)| t.kind == Kind::Class));
        assert_eq!(s.len(), s.things().count());
    }

    #[test]
    fn keys_of_things_that_stayed_do_not_change() {
        let mut ed = Editor::with_sequential_ids();
        let a = circle(&mut ed, 0.0);
        let b = circle(&mut ed, 5.0);
        let s1 = State::of(ed.doc(), None);
        let c = circle(&mut ed, 9.0);
        let s2 = State::of(ed.doc(), Some(&s1));
        for id in [a, b] {
            assert_eq!(
                s1.get(&obj(id)).unwrap()["order"],
                s2.get(&obj(id)).unwrap()["order"]
            );
        }
        assert!(key(&s2, c) > key(&s2, b));
        // Without the state before, the keys come from the positions only.
        assert_eq!(State::of(ed.doc(), None), State::of(ed.doc(), None));
        // Moving an object to the back gives it a new key and leaves the others.
        ed.execute(&cmd(json!({"op": "arrange", "ids": [c], "how": "back"})))
            .unwrap();
        let s3 = State::of(ed.doc(), Some(&s2));
        assert_eq!(
            s3.get(&obj(a)).unwrap()["order"],
            s2.get(&obj(a)).unwrap()["order"]
        );
        assert!(key(&s3, c) < key(&s3, a));
        assert_eq!(s2.diff(&s3).len(), 1);
    }

    #[test]
    fn diff_applied_gives_the_new_state() {
        let mut ed = Editor::with_sequential_ids();
        let a = circle(&mut ed, 0.0);
        let s1 = State::of(ed.doc(), None);
        let b = circle(&mut ed, 4.0);
        ed.execute(&cmd(
            json!({"op": "move_objects", "ids": [a], "delta": [1.0, 2.0]}),
        ))
        .unwrap();
        ed.execute(&cmd(json!({"op": "add_layer", "name": "B"})))
            .unwrap();
        let s2 = State::of(ed.doc(), Some(&s1));
        let ops = s1.diff(&s2);
        assert!(
            ops.iter()
                .any(|o| matches!(o, Op::Create { target, .. } if *target == obj(b)))
        );
        let mut got = s1.clone();
        got.apply_all(&ops).unwrap();
        assert_eq!(got, s2);
        // And back, with a delete.
        let back = s2.diff(&s1);
        assert!(
            back.iter()
                .any(|o| matches!(o, Op::Delete { target } if *target == obj(b)))
        );
        let mut got = s2.clone();
        got.apply_all(&back).unwrap();
        assert_eq!(got, s1);
        assert!(s1.diff(&s1).is_empty());
    }

    #[test]
    fn a_snapshot_picture_is_left_out_of_a_version_unless_a_plant_style_uses_it() {
        use kadakas_core::issue::{Issue, Viewpoint};
        use kadakas_core::picture::header_only_sample;
        use kadakas_core::resource::{ResourceBody, ResourceId};
        let mut ed = Editor::with_sequential_ids();
        let (only_issue, shared) = (header_only_sample(1), header_only_sample(2));
        for pic in [&only_issue, &shared] {
            ed.execute(&Command::AddIssue {
                issue: Box::new(Issue {
                    title: "T".into(),
                    viewpoints: vec![Viewpoint {
                        snapshot: Some(pic.sha256.clone()),
                        ..Viewpoint::default()
                    }],
                    ..Issue::default()
                }),
                pictures: vec![pic.clone()],
            })
            .unwrap();
        }
        let style = ed
            .execute(&Command::AddResource {
                name: "Oak".into(),
                body: ResourceBody::PlantStyle(Box::default()),
            })
            .unwrap()
            .created[0];
        ed.execute(&Command::SetPlantPicture {
            style: ResourceId(style),
            picture: Some(Box::new(shared.clone())),
        })
        .unwrap();
        let state = State::of(ed.doc(), None);
        let hashes = |s: &State| -> Vec<String> {
            s.things
                .iter()
                .filter(|(t, _)| t.kind == Kind::Resource)
                .filter_map(|(_, p)| {
                    p.get("body")
                        .and_then(|b| b.get("sha256"))
                        .and_then(|h| h.as_str().map(String::from))
                })
                .collect()
        };
        let got = hashes(&state);
        assert!(!got.contains(&only_issue.sha256), "review data stays out");
        assert!(got.contains(&shared.sha256), "a style needs its picture");
    }

    #[test]
    fn the_snapshot_builds_the_state_from_nothing() {
        let mut ed = Editor::with_sequential_ids();
        circle(&mut ed, 0.0);
        let s = State::of(ed.doc(), None);
        let mut got = State::new();
        got.apply_all(&s.snapshot()).unwrap();
        assert_eq!(got, s);
        assert!(s.bytes() > 100);
    }

    #[test]
    fn a_state_builds_a_document_with_the_same_content() {
        let mut ed = Editor::with_sequential_ids();
        circle(&mut ed, 0.0);
        circle(&mut ed, 3.0);
        let s = State::of(ed.doc(), None);
        let built = s.build(ed.doc());
        assert!(built.quarantined.is_empty(), "{:?}", built.quarantined);
        assert_eq!(State::of(&built.doc, None), s);
    }

    #[test]
    fn apply_refuses_what_does_not_fit() {
        let mut s = State::new();
        let t = obj(Uid::from_num(5));
        let create = Op::Create {
            target: t,
            props: [("a".to_string(), json!(1))].into(),
            merge: false,
        };
        assert_eq!(s.apply(&create), Ok(()));
        assert_eq!(s.apply(&create), Err(ApplyError::Exists(t)));
        let set = |prop: &str| Op::Set {
            target: t,
            prop: prop.into(),
            value: json!(2),
        };
        assert_eq!(s.apply(&set("a")), Ok(()));
        assert_eq!(s.get(&t).unwrap()["a"], json!(2));
        assert!(matches!(s.apply(&set("id")), Err(ApplyError::Invalid(_))));
        assert_eq!(
            s.apply(&Op::Unset {
                target: t,
                prop: "a".into()
            }),
            Ok(())
        );
        assert!(s.get(&t).unwrap().is_empty());
        assert_eq!(s.apply(&Op::Delete { target: t }), Ok(()));
        assert_eq!(
            s.apply(&Op::Delete { target: t }),
            Err(ApplyError::Missing(t))
        );
        assert!(matches!(
            s.apply(&Op::Delete {
                target: Target::DOCUMENT
            }),
            Err(ApplyError::Invalid(_))
        ));
        assert!(matches!(
            s.apply(&Op::Create {
                target: Target::DOCUMENT,
                props: Default::default(),
                merge: false
            }),
            Err(ApplyError::Invalid(_))
        ));
        assert!(matches!(
            s.apply(&Op::Create {
                target: t,
                props: [("id".to_string(), json!(1))].into(),
                merge: false
            }),
            Err(ApplyError::Invalid(_))
        ));
        assert_eq!(
            s.apply(&Op::Restore {
                target: t,
                deleted_by: kadakas_props::op::Writer::BASELINE
            }),
            Err(ApplyError::Unsupported)
        );
        assert_eq!(s.apply(&set("x")), Err(ApplyError::Missing(t)));
        assert_eq!(
            s.apply(&Op::Unset {
                target: t,
                prop: "x".into()
            }),
            Err(ApplyError::Missing(t))
        );
        for e in [
            ApplyError::Exists(t),
            ApplyError::Missing(t),
            ApplyError::Unsupported,
            ApplyError::Invalid("x".into()),
        ] {
            assert!(!e.to_string().is_empty());
        }
    }

    #[test]
    fn numbers_compare_as_numbers() {
        let mut a = State::new();
        let mut b = State::new();
        let t = obj(Uid::from_num(5));
        let make = |v: Value| Op::Create {
            target: t,
            props: [("n".to_string(), v)].into(),
            merge: false,
        };
        a.apply(&make(json!(1))).unwrap();
        b.apply(&make(json!(1.0))).unwrap();
        assert_eq!(a, b);
        assert!(a.diff(&b).is_empty());
        let mut c = State::new();
        c.apply(&make(json!(2))).unwrap();
        assert_ne!(a, c);
        assert_ne!(a, State::new());
    }

    #[test]
    fn check_op_uses_the_rules_of_collaboration() {
        let ok = Op::Set {
            target: obj(Uid::from_num(1)),
            prop: "a".into(),
            value: json!(1),
        };
        assert!(check_op(&ok).is_ok());
        let bad = Op::Set {
            target: obj(Uid::from_num(1)),
            prop: "id".into(),
            value: json!(1),
        };
        assert!(check_op(&bad).is_err());
    }
}
