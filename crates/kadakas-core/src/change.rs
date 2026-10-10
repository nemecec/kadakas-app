//! The change-event stream (PLAT-02).
//!
//! Every command that changes the document, and every undo and redo, produces one [`ChangeSet`]:
//! the new revision, what was added, changed and removed (by kind), which document-level settings
//! changed, who made the change and which command it was. A batch is one command, so it is one
//! change set. The editor keeps the latest change sets in a bounded log ([`ChangeLog`]); the API
//! turns them into notifications for subscribers, and the app uses them to keep its caches.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;

use serde::{Deserialize, Serialize};

use crate::model::{ClassId, LayerId, ObjectId};
use crate::resource::ResourceId;

/// Who made a change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// A person, in the app.
    #[default]
    User,
    /// An agent, through the API.
    Agent,
    /// A plug-in object that its script generated again. A Python script that calls the API is
    /// an `Agent`.
    Script,
    /// Another user of a shared document (collaboration, PLAT-07). The editor applied their
    /// changes with `Editor::apply_remote`.
    Remote,
}

impl Origin {
    pub fn name(self) -> &'static str {
        match self {
            Origin::User => "user",
            Origin::Agent => "agent",
            Origin::Script => "script",
            Origin::Remote => "remote",
        }
    }

    pub fn parse(text: &str) -> Option<Origin> {
        match text {
            "user" => Some(Origin::User),
            "agent" => Some(Origin::Agent),
            "script" => Some(Origin::Script),
            "remote" => Some(Origin::Remote),
            _ => None,
        }
    }
}

/// The names of the parts of a change set, for `events.subscribe {kinds}`.
pub const KINDS: [&str; 5] = ["objects", "layers", "classes", "resources", "document"];

/// IDs by kind: objects, layers, classes and resources (styles, symbols, worksheets, ...).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdsByKind {
    #[serde(default)]
    pub objects: Vec<ObjectId>,
    #[serde(default)]
    pub layers: Vec<LayerId>,
    #[serde(default)]
    pub classes: Vec<ClassId>,
    #[serde(default)]
    pub resources: Vec<ResourceId>,
}

impl IdsByKind {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
            && self.layers.is_empty()
            && self.classes.is_empty()
            && self.resources.is_empty()
    }

    /// Number of IDs in all kinds.
    pub fn len(&self) -> usize {
        self.objects.len() + self.layers.len() + self.classes.len() + self.resources.len()
    }
}

/// Which settings of the document itself changed (not objects, layers, classes or resources).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentChanged {
    pub active_layer: bool,
    pub active_class: bool,
    pub georef: bool,
    /// Project data, sheets' settings, saved views, view rules, visibility modes.
    pub drawing: bool,
}

impl DocumentChanged {
    pub fn any(&self) -> bool {
        self.active_layer || self.active_class || self.georef || self.drawing
    }
}

/// What one command, undo or redo changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSet {
    /// The revision of the document after the change (`Editor::revision`).
    pub revision: u64,
    /// The command (`move_objects`, `batch`, ...), or `undo`, `redo` or `rollback`.
    pub op: String,
    pub origin: Origin,
    pub added: IdsByKind,
    pub changed: IdsByKind,
    pub removed: IdsByKind,
    pub document: DocumentChanged,
}

impl ChangeSet {
    /// True if the change touches this part: `objects`, `layers`, `classes`, `resources` or
    /// `document`.
    pub fn touches(&self, kind: &str) -> bool {
        match kind {
            "objects" => {
                !(self.added.objects.is_empty()
                    && self.changed.objects.is_empty()
                    && self.removed.objects.is_empty())
            }
            "layers" => {
                !(self.added.layers.is_empty()
                    && self.changed.layers.is_empty()
                    && self.removed.layers.is_empty())
            }
            "classes" => {
                !(self.added.classes.is_empty()
                    && self.changed.classes.is_empty()
                    && self.removed.classes.is_empty())
            }
            "resources" => {
                !(self.added.resources.is_empty()
                    && self.changed.resources.is_empty()
                    && self.removed.resources.is_empty())
            }
            "document" => self.document.any(),
            _ => false,
        }
    }

    /// The change set with only the given parts; the other parts are empty. The revision, the
    /// command and the origin stay. `None` if the change touches none of the parts.
    pub fn only(&self, kinds: &[&str]) -> Option<ChangeSet> {
        if !kinds.iter().any(|k| self.touches(k)) {
            return None;
        }
        let want = |k: &str| kinds.contains(&k);
        let pick = |ids: &IdsByKind| IdsByKind {
            objects: if want("objects") {
                ids.objects.clone()
            } else {
                vec![]
            },
            layers: if want("layers") {
                ids.layers.clone()
            } else {
                vec![]
            },
            classes: if want("classes") {
                ids.classes.clone()
            } else {
                vec![]
            },
            resources: if want("resources") {
                ids.resources.clone()
            } else {
                vec![]
            },
        };
        Some(ChangeSet {
            revision: self.revision,
            op: self.op.clone(),
            origin: self.origin,
            added: pick(&self.added),
            changed: pick(&self.changed),
            removed: pick(&self.removed),
            document: if want("document") {
                self.document
            } else {
                DocumentChanged::default()
            },
        })
    }
}

/// IDs that are in `after` and not in `before`, in the order of `after`.
fn only_in<K: Copy + Eq + Hash>(after: &[K], before: &HashSet<K>) -> Vec<K> {
    after
        .iter()
        .copied()
        .filter(|k| !before.contains(k))
        .collect()
}

/// Compares two lists of things with IDs: (added, changed, removed).
pub(crate) fn diff_by_id<K: Copy + Eq + Hash, V: PartialEq>(
    before: &[(K, &V)],
    after: &[(K, &V)],
) -> (Vec<K>, Vec<K>, Vec<K>) {
    let b: HashMap<K, &V> = before.iter().copied().collect();
    let a: HashMap<K, &V> = after.iter().copied().collect();
    let added = after
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !b.contains_key(k))
        .collect();
    let removed = before
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !a.contains_key(k))
        .collect();
    let changed = after
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| b.get(k).is_some_and(|old| *old != a[k]))
        .collect();
    (added, changed, removed)
}

/// The objects of a patch as (added, changed, removed). `forward` is false for an undo.
pub(crate) fn object_changes(
    added: &[ObjectId],
    removed: &[ObjectId],
    modified: &[ObjectId],
    forward: bool,
) -> (Vec<ObjectId>, Vec<ObjectId>, Vec<ObjectId>) {
    let a: HashSet<ObjectId> = added.iter().copied().collect();
    let r: HashSet<ObjectId> = removed.iter().copied().collect();
    // An object that leaves and comes back (it moved in the z-order, or was replaced) changed.
    let mut changed: Vec<ObjectId> = modified.to_vec();
    let seen: HashSet<ObjectId> = changed.iter().copied().collect();
    changed.extend(
        added
            .iter()
            .copied()
            .filter(|i| r.contains(i) && !seen.contains(i)),
    );
    let new = only_in(added, &r);
    let gone = only_in(removed, &a);
    if forward {
        (new, changed, gone)
    } else {
        (gone, changed, new)
    }
}

/// The latest change sets of an editor, newest last. The log is bounded in the number of change
/// sets and in the number of IDs in them, so a big import cannot fill the memory.
#[derive(Clone, Debug)]
pub struct ChangeLog {
    entries: VecDeque<ChangeSet>,
    /// The highest revision of a change set that was dropped from the log (0: none).
    dropped_upto: u64,
    ids: usize,
    max_entries: usize,
    max_ids: usize,
}

/// The most change sets that a log keeps.
pub const LOG_ENTRIES: usize = 4096;
/// The most IDs, in all change sets of a log together.
pub const LOG_IDS: usize = 1_000_000;

impl Default for ChangeLog {
    fn default() -> Self {
        ChangeLog::with_limits(LOG_ENTRIES, LOG_IDS)
    }
}

/// What a client gets when it asks for the changes since a revision.
#[derive(Clone, Debug, PartialEq)]
pub struct ChangesSince<'a> {
    pub changes: Vec<&'a ChangeSet>,
    /// True if change sets after the asked revision were dropped from the log. The client cannot
    /// follow the changes and must read the document again.
    pub truncated: bool,
}

impl ChangeLog {
    pub fn with_limits(max_entries: usize, max_ids: usize) -> Self {
        ChangeLog {
            entries: VecDeque::new(),
            dropped_upto: 0,
            ids: 0,
            max_entries: max_entries.max(1),
            max_ids: max_ids.max(1),
        }
    }

    /// An empty log for an editor made at `revision`: a client that asks for changes before it
    /// gets `truncated`, because it cannot follow them.
    pub fn starting_at(revision: u64) -> Self {
        ChangeLog {
            dropped_upto: revision,
            ..ChangeLog::default()
        }
    }

    /// A rough size in bytes.
    pub fn approx_bytes(&self) -> u64 {
        self.ids as u64 * 16 + self.entries.len() as u64 * 200
    }

    pub fn push(&mut self, change: ChangeSet) {
        self.ids += change.added.len() + change.changed.len() + change.removed.len();
        self.entries.push_back(change);
        while self.entries.len() > 1
            && (self.entries.len() > self.max_entries || self.ids > self.max_ids)
        {
            if let Some(old) = self.entries.pop_front() {
                self.ids -= old.added.len() + old.changed.len() + old.removed.len();
                self.dropped_upto = self.dropped_upto.max(old.revision);
            }
        }
    }

    pub fn last(&self) -> Option<&ChangeSet> {
        self.entries.back()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The change sets with a revision above `revision`, oldest first.
    pub fn since(&self, revision: u64) -> ChangesSince<'_> {
        ChangesSince {
            changes: self
                .entries
                .iter()
                .filter(|c| c.revision > revision)
                .collect(),
            truncated: revision < self.dropped_upto,
        }
    }
}

/// The name of a command (its `op`), or `None` if it cannot be found.
///
/// Commands serialize as a tagged object whose first field is `op`. This reads that field and
/// stops, so a big command is not copied to find its name.
pub fn op_name<T: Serialize>(command: &T) -> Option<String> {
    sniff::op_of(command)
}

mod sniff {
    use serde::Serialize;
    use serde::ser::{self, Impossible, Serializer};

    #[derive(Debug)]
    pub struct Stop(Option<String>);

    impl std::fmt::Display for Stop {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("stop")
        }
    }

    impl std::error::Error for Stop {}

    impl ser::Error for Stop {
        fn custom<T: std::fmt::Display>(_: T) -> Self {
            Stop(None)
        }
    }

    pub fn op_of<T: Serialize>(value: &T) -> Option<String> {
        match value.serialize(Sniffer) {
            Err(Stop(name)) => name,
            Ok(never) => match never {},
        }
    }

    struct Sniffer;

    /// Takes the first field of a struct. `op` is the tag, and serde writes the tag first.
    struct First;

    impl ser::SerializeStruct for First {
        type Ok = std::convert::Infallible;
        type Error = Stop;

        fn serialize_field<T: ?Sized + Serialize>(
            &mut self,
            key: &'static str,
            value: &T,
        ) -> Result<(), Stop> {
            let name = if key == "op" {
                value.serialize(Text).ok()
            } else {
                None
            };
            Err(Stop(name))
        }

        fn end(self) -> Result<Self::Ok, Stop> {
            Err(Stop(None))
        }
    }

    /// Takes a text.
    struct Text;

    macro_rules! no {
        ($($name:ident($($arg:ty),*) -> $ret:ty;)*) => {
            $(fn $name(self, $(_: $arg),*) -> Result<$ret, Stop> { Err(Stop(None)) })*
        };
    }

    impl Serializer for Text {
        type Ok = String;
        type Error = Stop;
        type SerializeSeq = Impossible<String, Stop>;
        type SerializeTuple = Impossible<String, Stop>;
        type SerializeTupleStruct = Impossible<String, Stop>;
        type SerializeTupleVariant = Impossible<String, Stop>;
        type SerializeMap = Impossible<String, Stop>;
        type SerializeStruct = Impossible<String, Stop>;
        type SerializeStructVariant = Impossible<String, Stop>;

        fn serialize_str(self, v: &str) -> Result<String, Stop> {
            Ok(v.to_string())
        }

        no! {
            serialize_bool(bool) -> String;
            serialize_i8(i8) -> String;
            serialize_i16(i16) -> String;
            serialize_i32(i32) -> String;
            serialize_i64(i64) -> String;
            serialize_u8(u8) -> String;
            serialize_u16(u16) -> String;
            serialize_u32(u32) -> String;
            serialize_u64(u64) -> String;
            serialize_f32(f32) -> String;
            serialize_f64(f64) -> String;
            serialize_char(char) -> String;
            serialize_bytes(&[u8]) -> String;
            serialize_none() -> String;
            serialize_unit() -> String;
            serialize_unit_struct(&'static str) -> String;
            serialize_unit_variant(&'static str, u32, &'static str) -> String;
            serialize_seq(Option<usize>) -> Self::SerializeSeq;
            serialize_tuple(usize) -> Self::SerializeTuple;
            serialize_tuple_struct(&'static str, usize) -> Self::SerializeTupleStruct;
            serialize_tuple_variant(&'static str, u32, &'static str, usize) -> Self::SerializeTupleVariant;
            serialize_map(Option<usize>) -> Self::SerializeMap;
            serialize_struct(&'static str, usize) -> Self::SerializeStruct;
            serialize_struct_variant(&'static str, u32, &'static str, usize) -> Self::SerializeStructVariant;
        }

        fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<String, Stop> {
            Err(Stop(None))
        }

        fn serialize_newtype_struct<T: ?Sized + Serialize>(
            self,
            _: &'static str,
            _: &T,
        ) -> Result<String, Stop> {
            Err(Stop(None))
        }

        fn serialize_newtype_variant<T: ?Sized + Serialize>(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
            _: &T,
        ) -> Result<String, Stop> {
            Err(Stop(None))
        }
    }

    impl Serializer for Sniffer {
        type Ok = std::convert::Infallible;
        type Error = Stop;
        type SerializeSeq = Impossible<Self::Ok, Stop>;
        type SerializeTuple = Impossible<Self::Ok, Stop>;
        type SerializeTupleStruct = Impossible<Self::Ok, Stop>;
        type SerializeTupleVariant = Impossible<Self::Ok, Stop>;
        type SerializeMap = Impossible<Self::Ok, Stop>;
        type SerializeStruct = First;
        type SerializeStructVariant = Impossible<Self::Ok, Stop>;

        fn serialize_struct(self, _: &'static str, _: usize) -> Result<First, Stop> {
            Ok(First)
        }

        no! {
            serialize_bool(bool) -> Self::Ok;
            serialize_i8(i8) -> Self::Ok;
            serialize_i16(i16) -> Self::Ok;
            serialize_i32(i32) -> Self::Ok;
            serialize_i64(i64) -> Self::Ok;
            serialize_u8(u8) -> Self::Ok;
            serialize_u16(u16) -> Self::Ok;
            serialize_u32(u32) -> Self::Ok;
            serialize_u64(u64) -> Self::Ok;
            serialize_f32(f32) -> Self::Ok;
            serialize_f64(f64) -> Self::Ok;
            serialize_char(char) -> Self::Ok;
            serialize_str(&str) -> Self::Ok;
            serialize_bytes(&[u8]) -> Self::Ok;
            serialize_none() -> Self::Ok;
            serialize_unit() -> Self::Ok;
            serialize_unit_struct(&'static str) -> Self::Ok;
            serialize_unit_variant(&'static str, u32, &'static str) -> Self::Ok;
            serialize_seq(Option<usize>) -> Self::SerializeSeq;
            serialize_tuple(usize) -> Self::SerializeTuple;
            serialize_tuple_struct(&'static str, usize) -> Self::SerializeTupleStruct;
            serialize_tuple_variant(&'static str, u32, &'static str, usize) -> Self::SerializeTupleVariant;
            serialize_map(Option<usize>) -> Self::SerializeMap;
            serialize_struct_variant(&'static str, u32, &'static str, usize) -> Self::SerializeStructVariant;
        }

        fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<Self::Ok, Stop> {
            Err(Stop(None))
        }

        fn serialize_newtype_struct<T: ?Sized + Serialize>(
            self,
            _: &'static str,
            value: &T,
        ) -> Result<Self::Ok, Stop> {
            value.serialize(Sniffer)
        }

        fn serialize_newtype_variant<T: ?Sized + Serialize>(
            self,
            _: &'static str,
            _: u32,
            _: &'static str,
            _: &T,
        ) -> Result<Self::Ok, Stop> {
            Err(Stop(None))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    #[serde(tag = "op", rename_all = "snake_case")]
    enum Cmd {
        MoveThings { ids: Vec<u32>, delta: [f64; 2] },
        Nothing,
        Wrap(Inner),
    }

    #[derive(Serialize)]
    struct Inner {
        a: u32,
    }

    fn cs(revision: u64, added: usize, op: &str) -> ChangeSet {
        ChangeSet {
            revision,
            op: op.into(),
            origin: Origin::User,
            added: IdsByKind {
                objects: (0..added as u64).map(ObjectId::from_num).collect(),
                ..Default::default()
            },
            changed: IdsByKind::default(),
            removed: IdsByKind::default(),
            document: DocumentChanged::default(),
        }
    }

    #[test]
    fn the_op_of_a_command_is_found_without_copying_it() {
        assert_eq!(
            op_name(&Cmd::MoveThings {
                ids: vec![1; 100_000],
                delta: [0.0, 1.0]
            })
            .as_deref(),
            Some("move_things")
        );
        assert_eq!(op_name(&Cmd::Nothing).as_deref(), Some("nothing"));
        assert_eq!(op_name(&Cmd::Wrap(Inner { a: 1 })).as_deref(), Some("wrap"));
        assert_eq!(op_name(&5u32), None);
        assert_eq!(op_name(&Inner { a: 1 }), None);
        assert_eq!(op_name(&"text"), None);
        assert_eq!(op_name(&vec![1, 2]), None);
    }

    #[test]
    fn origin_names_round_trip() {
        for o in [Origin::User, Origin::Agent, Origin::Script] {
            assert_eq!(Origin::parse(o.name()), Some(o));
            assert_eq!(
                serde_json::to_value(o).unwrap(),
                serde_json::json!(o.name())
            );
        }
        assert_eq!(Origin::parse("robot"), None);
    }

    #[test]
    fn a_change_set_can_be_cut_to_the_parts_that_a_subscriber_wants() {
        let mut c = cs(5, 2, "add_object");
        c.changed.layers = vec![LayerId::from_num(1)];
        assert!(c.touches("objects") && c.touches("layers"));
        assert!(!c.touches("classes") && !c.touches("document") && !c.touches("nonsense"));
        let only_layers = c.only(&["layers"]).unwrap();
        assert!(only_layers.added.objects.is_empty());
        assert_eq!(only_layers.changed.layers.len(), 1);
        assert_eq!(
            (only_layers.revision, only_layers.op.as_str()),
            (5, "add_object")
        );
        assert!(c.only(&["classes", "document"]).is_none());
        c.document.georef = true;
        assert!(c.only(&["document"]).unwrap().document.georef);
        assert_eq!(c.added.len(), 2);
        assert!(!c.added.is_empty() && IdsByKind::default().is_empty());
    }

    #[test]
    fn the_log_keeps_the_newest_and_says_when_it_dropped_what_a_client_needs() {
        let mut log = ChangeLog::with_limits(3, 100);
        assert!(log.is_empty() && log.last().is_none());
        for r in 1..=5 {
            log.push(cs(r, 1, "x"));
        }
        assert_eq!(log.len(), 3);
        assert_eq!(log.last().unwrap().revision, 5);
        let s = log.since(3);
        assert_eq!(
            s.changes.iter().map(|c| c.revision).collect::<Vec<_>>(),
            vec![4, 5]
        );
        assert!(!s.truncated);
        // Revisions 1 and 2 are gone: a client at revision 1 missed revision 2.
        assert!(log.since(1).truncated);
        assert!(!log.since(2).truncated);
        // A big change set pushes older ones out, but the newest always stays.
        let mut log = ChangeLog::with_limits(100, 10);
        log.push(cs(1, 4, "x"));
        log.push(cs(2, 4, "x"));
        log.push(cs(3, 50, "big"));
        assert_eq!(log.len(), 1);
        assert_eq!(log.last().unwrap().op, "big");
        assert!(log.since(0).truncated);
    }

    #[test]
    fn objects_that_leave_and_return_changed() {
        let (a, b, c) = (
            ObjectId::from_num(1),
            ObjectId::from_num(2),
            ObjectId::from_num(3),
        );
        let (added, changed, removed) = object_changes(&[a, b], &[b, c], &[], true);
        assert_eq!((added, changed, removed), (vec![a], vec![b], vec![c]));
        // An undo swaps added and removed.
        let (added, changed, removed) = object_changes(&[a, b], &[b, c], &[], false);
        assert_eq!((added, changed, removed), (vec![c], vec![b], vec![a]));
    }

    #[test]
    fn lists_with_ids_are_compared() {
        let (x, y, z) = (1u32, 2u32, 3u32);
        let before = [(x, &"a"), (y, &"b")];
        let after = [(y, &"B"), (z, &"c")];
        assert_eq!(diff_by_id(&before, &after), (vec![z], vec![y], vec![x]));
    }
}
