//! The version store, with reverse deltas.
//!
//! ```text
//!   anchor --newest_delta--> version n (the newest)
//!   version 1 <--delta-- version 2 <--delta-- ... <--delta-- version n
//! ```
//!
//! The current document is the only full copy of the drawing in a file. The newest version is
//! stored as a *change from a drawing*, the **anchor**, and every older version is stored as a
//! *reverse delta* from the next newer version on its chain of first parents: the list of property
//! operations that turns the newer version into this one. To open version k, the code starts at the
//! newest version and applies the deltas backwards (see [`History::state_at`]). A version of a
//! drawing that changed a little is a few small operations, and there is no separate base
//! snapshot of the oldest version.
//!
//! # The anchor
//!
//! The JSON names the anchor by a digest (`newest.anchor`) and does not hold it. Which drawing it
//! is depends on where the JSON is:
//!
//! - In a **file** the anchor is the drawing that the file holds. [`History::to_disk`] makes the
//!   newest version again against the drawing at each save (one comparison of the drawing with
//!   the newest version), and the save also writes the stacking keys of the drawing
//!   (`State::stamp_keys`), so that the state of the drawing is the same when it is read again.
//!   The drawing never needs the history to load: it is read first, and the history is read
//!   afterwards, from the drawing and the JSON, and only when it is used.
//! - In an **open document** (`Document::history`, held by the undo steps too) the anchor is the
//!   drawing as it was when the file was opened, or when the first version was saved. The session
//!   keeps that one state (a shared copy, see [`History::anchor_pair`]) and gives it to
//!   [`History::load_with`]. Every state of the history in the undo steps is then a few
//!   operations, not a copy of the drawing: the cost of a step is the changes since the anchor.
//!
//! # Pictures
//!
//! A big picture (the PNG of an underlay, the file of an image resource) is not stored in the
//! operations. As in collaboration, the property holds a reference `{"$blob": "<sha256>"}`, and
//! the history keeps a table `blobs` with the text of each picture once, by hash. A move of a
//! 10 MiB underlay in three versions costs 10 MiB once, not three times. A picture stays while the
//! newest delta or a delta refers to it (a count of references that is made again after a delete),
//! and the byte count counts each picture once. A picture that the anchor holds is not repeated in
//! the JSON, and the byte count does not count it.
//!
//! # Memory: parts that undo steps share
//!
//! [`History::to_data`] makes the memory form as a `HistoryData` of parts: the JSON of each
//! version, of each operation of the newest change, and of each picture is its own shared value.
//! A new version changes the JSON of the old newest version and adds one; the others, the
//! pictures and the operations of the change that did not change are the same memory in the
//! value before and after. An undo step that holds both values costs what changed, and the memory
//! of many versions grows with the drawing and the deltas, not with versions times drawing.
//!
//! # A history that a reader keeps as it is
//!
//! A history that [`History::load_with_caps`] could not read completely (a cap, a damaged part)
//! is not changed or cut by the host: it stays in the document as it is, read only. If the
//! drawing is edited and saved, the versions would no longer fit it. So the host keeps the drawing
//! that the history was written against ([`written_against`]) and a save adds the entry `base`
//! to the history: the changes that turn the saved drawing into that drawing ([`rebase_for_save`]),
//! with the pictures they need. The loader uses it when the drawing is not the anchor itself.
//! After the host raises the caps and reads the file again, every version is exact.
//!
//! # Parents and branches
//!
//! The versions are a graph, not a list, so branches can come later (REV-10). Each version has
//! `parents`, the IDs of the versions it was made from, the first parent first (a merge has two;
//! the root has none) and a `branch` name (default `main`). The history has a current `branch` and
//! its `head`, the version that the next one is made from. Each version but the newest has `via`,
//! the version whose state its delta starts from: the next newer version on its chain of first
//! parents. Today every version has the previous one as its only parent, and there are no calls
//! that make a branch or a merge yet. [`History::load`] makes the chain linear: it sets the first
//! parent of every version to the version before it in the file (and says so in a note), because
//! it replays the file in order; it keeps the other parents that point at earlier versions.
//! Deleting a version joins its delta to the deltas of its neighbors, so the graph stays whole.
//!
//! # Limits: a version is never dropped without consent
//!
//! [`Limits`] sets a threshold for the number of versions and the bytes of the history. It is a
//! warning, not an eviction: when a new version would go over it, [`History::add`] refuses and
//! leaves the history unchanged. The error ([`ReviewError::LimitReached`]) lists the versions with
//! their sizes, so that a person can choose which to delete, or raise the limit. Nothing is merged
//! or folded away by itself, and there is no code that does it. [`History::remove_many`] is the
//! call for the versions that a person chose.
//!
//! # The file
//!
//! ```json
//! {"format": 2, "next_id": 4, "branch": "main", "head": 3,
//!  "newest": {"id": 3, "delta": [<operations: the anchor -> the newest version>], "anchor": "<sha256>"},
//!  "blobs": {"<sha256>": "<text of a picture>"},
//!  "versions": [{"id": 1, "name": "...", "note": "...", "author": "...", "time_ms": 0,
//!                "origin": "user", "base_revision": 7, "parents": [], "branch": "main",
//!                "via": 2, "delta": [<operations: version 2 -> version 1>]}, ...,
//!               {"id": 3, ..., "parents": [2]}]}
//! ```
//!
//! # Untrusted data
//!
//! A file or a collaboration peer can hand over any JSON. [`History::load`] never panics and does
//! bounded work: it reads at most [`HARD_MAX_VERSIONS`] versions (the newest ones) and
//! [`HARD_MAX_BYTES`] bytes of operations, checks every operation with the rules of collaboration,
//! replays the whole chain from the newest version, cuts long texts, gives a repeated ID a new
//! one, and leaves out a version (and the older ones, which depend on it) whose delta does not
//! fit. Each repair is a note. A JSON whose `anchor` is the digest of no known drawing (the drawing
//! was changed by a program that did not update the versions) cannot be read at all.
//!
//! The result is a *reading* of the file, not a replacement for it. The caller keeps the JSON of
//! the file as it is, shows the notes, and treats the history as read only while there are notes:
//! the person decides whether to write the readable part (`Loaded::notes` empty means that the
//! reading is the whole file). A file that is over a bound is therefore never trimmed by a load.
//!
//! # Collaboration
//!
//! The history is local to a file. It is not part of the shared properties, so joining a shared
//! document keeps the versions of the open file (`Editor::apply_remote` leaves them alone), and
//! the other users have their own. A server that shares versions is REV-09.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use kadakas_core::change::Origin;
use kadakas_core::history::HistoryData;
use kadakas_core::model::Document;
use kadakas_props::blob::{BLOB_PROPS, MAX_BLOB_BYTES, blob_ref, sha256_hex};
use kadakas_props::op::{Op, op_size, ops_digest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::state::{ApplyError, State, check_op};

/// The version of the JSON form.
pub const FORMAT: u64 = 2;
/// The most versions that [`History::load`] reads.
pub const HARD_MAX_VERSIONS: usize = 1000;
/// The most bytes of operations that [`History::load`] reads.
pub const HARD_MAX_BYTES: usize = 256 * 1024 * 1024;

/// How much a file may hold for [`History::load_with_caps`] to read it. A history over a cap is
/// not cut: the document opens, the history stays as the file has it, and the load notes say why
/// (the host may read again with higher caps).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadCaps {
    /// The most versions.
    pub max_versions: usize,
    /// The most bytes of operations and pictures.
    pub max_bytes: usize,
}

impl Default for ReadCaps {
    fn default() -> Self {
        ReadCaps {
            max_versions: HARD_MAX_VERSIONS,
            max_bytes: HARD_MAX_BYTES,
        }
    }
}

/// The longest name of a version, in characters.
pub const NAME_MAX: usize = 120;
/// The longest note of a version, in characters.
pub const NOTE_MAX: usize = 2000;
/// The longest author, in characters.
pub const AUTHOR_MAX: usize = 120;
/// What the metadata of a version counts in the size of the history, besides its texts.
const META_BYTES: usize = 160;

/// The thresholds of a history. A new version that would go over one is refused (see the module
/// text); no version is dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most versions. 0 turns versions off.
    pub max_versions: usize,
    /// The most bytes of the history: the changes, the texts and the pictures (an estimate of the
    /// JSON). The newest version counts as its metadata only, because the file stores it as a
    /// change from the drawing.
    pub max_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_versions: 100,
            max_bytes: 64 * 1024 * 1024,
        }
    }
}

impl Limits {
    /// The limits, cut to the security bounds (what a file may hold at all).
    pub fn clamped(self) -> Limits {
        Limits {
            max_versions: self.max_versions.min(HARD_MAX_VERSIONS),
            max_bytes: self.max_bytes.min(HARD_MAX_BYTES),
        }
    }
}

/// Which limit a new version would break.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitKind {
    Versions,
    Bytes,
}

/// A version as a person sees it when choosing what to delete.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VersionInfo {
    pub id: u64,
    pub name: String,
    /// About the bytes that deleting the version frees: its changes, its texts and the pictures
    /// that only it uses.
    pub bytes: usize,
    pub time_ms: u64,
    pub auto: bool,
}

/// What a refused save tells.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LimitInfo {
    pub kind: LimitKind,
    /// The limit.
    pub max: usize,
    /// What the history would be with the new version: a number of versions or bytes.
    pub needed: usize,
    /// The versions that exist, oldest first.
    pub versions: Vec<VersionInfo>,
}

impl std::fmt::Display for LimitInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (what, unit) = match self.kind {
            LimitKind::Versions => ("versions", ""),
            LimitKind::Bytes => ("size", " bytes"),
        };
        write!(
            f,
            "the new version would make the history {} {what}{unit} and the limit is {}{unit}: the version was not saved and nothing was deleted. Ask the person which versions to delete (version.delete {{ids}}), or to raise the limit",
            self.needed, self.max
        )
    }
}

/// Why a call on the history failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReviewError {
    #[error("version {0} does not exist")]
    NotFound(u64),
    #[error("{0}")]
    Invalid(String),
    #[error("versions are turned off (the limit is 0 versions)")]
    Disabled,
    /// Saving would go over a limit. The history is unchanged.
    #[error("{0}")]
    LimitReached(Box<LimitInfo>),
    /// A version cannot be rebuilt. Cannot happen for a history that [`History::load`] read
    /// without notes.
    #[error("the history is damaged: {0}")]
    Damaged(String),
}

impl From<ApplyError> for ReviewError {
    fn from(e: ApplyError) -> Self {
        ReviewError::Damaged(e.to_string())
    }
}

/// What a person or an agent gives about a version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionMeta {
    /// A number, unique in this file, never used again after the version is deleted.
    pub id: u64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub author: String,
    /// Milliseconds since 1970 (UTC). 0: not known (a host without a clock).
    #[serde(default)]
    pub time_ms: u64,
    /// Who was working when the version was saved (the origin of the editor).
    #[serde(default)]
    pub origin: Origin,
    /// The versions this one was made from, the first parent first. Empty for the first version.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<u64>,
    /// The branch that the version was made on.
    #[serde(default = "main_branch")]
    pub branch: String,
    /// The revision of the editor when the version was saved (`Editor::revision`). It is unique in
    /// one process. Use it to find out if the open document changed since the version; it means
    /// nothing after the file is opened again.
    #[serde(default)]
    pub base_revision: u64,
    /// True for a version that Kadakas saved by itself (before a restore).
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto: bool,
}

/// The name of the branch that every history starts with.
pub const MAIN_BRANCH: &str = "main";
/// The longest branch name, in characters.
pub const BRANCH_MAX: usize = 60;
/// The most parents of one version.
pub const PARENTS_MAX: usize = 8;

fn main_branch() -> String {
    MAIN_BRANCH.to_string()
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// The input for a new version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewVersion {
    pub name: String,
    pub note: Option<String>,
    pub author: String,
    pub time_ms: u64,
    pub origin: Origin,
    pub base_revision: u64,
    pub auto: bool,
}

/// The JSON of a version, made once and shared by the copies of the history (and by the values
/// that the editor holds in its undo steps). Whoever changes a version makes a new one with
/// `Memo::default()`. It plays no part in equality.
#[derive(Clone, Debug, Default)]
struct Memo(Arc<std::sync::OnceLock<Arc<Value>>>);

impl PartialEq for Memo {
    fn eq(&self, _: &Memo) -> bool {
        true
    }
}

/// The JSON of the operations of a list, each on its own, shared by the copies of the history. It
/// plays no part in equality.
#[derive(Clone, Debug, Default)]
struct OpsJson(Arc<Vec<Arc<Value>>>);

impl PartialEq for OpsJson {
    fn eq(&self, _: &OpsJson) -> bool {
        true
    }
}

/// A key for the operations of a diff: the same thing and property gives the same key.
fn op_key(op: &Op) -> (u8, String) {
    match op {
        Op::Create { target, .. } => (0, format!("{target:?}")),
        Op::Delete { target } => (1, format!("{target:?}")),
        Op::Restore { target, .. } => (2, format!("{target:?}")),
        Op::Set { target, prop, .. } => (3, format!("{target:?}/{prop}")),
        Op::Unset { target, prop } => (4, format!("{target:?}/{prop}")),
    }
}

/// The JSON of `ops`, with the JSON of the operations of `prev` (and `prev_json`) that are equal
/// to one of them: a change that grows by one edit at a time costs that edit, not all of it.
fn ops_json(ops: &[Op], prev: &[Op], prev_json: &[Arc<Value>]) -> Vec<Arc<Value>> {
    let known: BTreeMap<(u8, String), usize> = if prev.len() == prev_json.len() {
        prev.iter()
            .enumerate()
            .map(|(i, op)| (op_key(op), i))
            .collect()
    } else {
        BTreeMap::new()
    };
    ops.iter()
        .map(|op| {
            known
                .get(&op_key(op))
                .filter(|&&i| prev[i] == *op)
                .map_or_else(
                    || Arc::new(serde_json::to_value(op).unwrap_or(Value::Null)),
                    |&i| prev_json[i].clone(),
                )
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
struct Version {
    meta: VersionMeta,
    /// The version whose state the delta starts from (the next newer on the chain of first
    /// parents). `None` for the newest version.
    via: Option<u64>,
    /// The operations that turn the state of `via` into the state of this version. Empty for the
    /// newest version.
    delta: Vec<Op>,
    bytes: usize,
    json: Memo,
}

fn ops_bytes(ops: &[Op]) -> usize {
    ops.iter().map(op_size).sum()
}

fn meta_bytes(m: &VersionMeta) -> usize {
    META_BYTES + m.name.len() + m.note.as_ref().map_or(0, String::len) + m.author.len()
}

/// The versions of a document.
#[derive(Clone, Debug, PartialEq)]
pub struct History {
    next_id: u64,
    /// The drawing that the newest version is stored against (see the module text). One shared
    /// copy; it is not in the JSON.
    anchor: Arc<State>,
    /// The digest of the anchor.
    anchor_digest: String,
    /// The operations that turn the anchor into the newest version.
    newest_delta: Vec<Op>,
    /// The JSON of `newest_delta`, one operation each.
    newest_json: OpsJson,
    /// The ID of the newest version.
    newest: Option<u64>,
    /// Oldest first.
    versions: Vec<Version>,
    /// The branch that new versions go on.
    branch: String,
    /// The version that the next one is made from: the newest on the current branch.
    head: Option<u64>,
    /// The pictures that the deltas refer to, once each, by the SHA-256 of the text (the anchor
    /// has its own).
    blobs: BTreeMap<String, Arc<str>>,
}

/// The result of [`History::load`]: the history that could be read, and what was repaired.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub history: Option<History>,
    pub notes: Vec<String>,
}

/// The result of [`History::add`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Added {
    pub id: u64,
}

fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// The digest of a state, as text. The JSON of a history names its anchor by it.
pub fn anchor_digest(state: &State) -> String {
    ops_digest(&state.snapshot())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl History {
    /// A history with no versions. Use [`History::add`] to make the first one.
    pub fn new() -> History {
        History {
            next_id: 1,
            anchor: Arc::new(State::new()),
            anchor_digest: anchor_digest(&State::new()),
            newest_delta: Vec::new(),
            newest_json: OpsJson::default(),
            newest: None,
            versions: Vec::new(),
            branch: main_branch(),
            head: None,
            blobs: BTreeMap::new(),
        }
    }

    /// The branch that new versions go on.
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// The version that the next one is made from.
    pub fn head(&self) -> Option<u64> {
        self.head
    }

    pub fn len(&self) -> usize {
        self.versions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.versions.is_empty()
    }

    /// The versions, oldest first.
    pub fn versions(&self) -> impl Iterator<Item = &VersionMeta> {
        self.versions.iter().map(|v| &v.meta)
    }

    pub fn meta(&self, id: u64) -> Option<&VersionMeta> {
        self.versions.iter().map(|v| &v.meta).find(|m| m.id == id)
    }

    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.versions.iter().position(|v| v.meta.id == id)
    }

    /// The size of the history in bytes, estimated from its changes, texts and pictures (each
    /// picture once). The change of the newest version is not counted: a file stores that version
    /// as a change from the drawing. A picture that the anchor holds is not counted either.
    pub fn bytes(&self) -> usize {
        self.versions.iter().map(|v| v.bytes).sum::<usize>()
            + self
                .blobs
                .iter()
                .filter(|(h, _)| !self.anchor.blobs().contains_key(*h))
                .map(|(_, t)| t.len())
                .sum::<usize>()
    }

    /// The digest and the state of the anchor. A session keeps the pair, so that it can read the
    /// JSON of this history again (see the module text).
    pub fn anchor_pair(&self) -> (String, Arc<State>) {
        (self.anchor_digest.clone(), self.anchor.clone())
    }

    /// The pictures of the history, by hash.
    pub fn blobs(&self) -> &BTreeMap<String, Arc<str>> {
        &self.blobs
    }

    /// The size of the delta of a version in bytes, estimated.
    pub fn delta_bytes(&self, id: u64) -> Option<usize> {
        self.index_of(id)
            .map(|i| ops_bytes(&self.versions[i].delta))
    }

    /// Keeps only the pictures that a delta still refers to.
    fn collect_blobs(&mut self) {
        let mut used = BTreeSet::new();
        for op in self
            .newest_delta
            .iter()
            .chain(self.versions.iter().flat_map(|v| &v.delta))
        {
            op_refs(op, &mut |h| {
                used.insert(h.to_string());
            });
        }
        self.blobs.retain(|h, _| used.contains(h));
    }

    /// Sets the change from the anchor to the newest version, and its JSON.
    fn set_newest_delta(&mut self, delta: Vec<Op>) {
        let json = ops_json(&delta, &self.newest_delta, &self.newest_json.0);
        self.newest_delta = delta;
        self.newest_json = OpsJson(Arc::new(json));
    }

    /// Takes the pictures that these operations refer to from the state that made them.
    fn take_blobs(&mut self, ops: &[Op], from: &State) {
        for op in ops {
            op_refs(op, &mut |h| {
                if let Some(t) = from.blobs().get(h) {
                    self.blobs.insert(h.to_string(), t.clone());
                }
            });
        }
    }

    /// The state of the newest version: the anchor with the change applied.
    fn newest_state(&self) -> Result<State, ReviewError> {
        let mut state = (*self.anchor).clone();
        let mut table = self.anchor.blobs().clone();
        table.extend(self.blobs.iter().map(|(h, t)| (h.clone(), t.clone())));
        state.set_blobs(table);
        state.apply_all(&self.newest_delta)?;
        Ok(state)
    }

    /// The state of the version at `index` (0 is the oldest): the newest version, with the deltas
    /// along the chain from this version applied backwards.
    pub fn state_at(&self, index: usize) -> Result<State, ReviewError> {
        if index >= self.versions.len() {
            return Err(ReviewError::Damaged(format!("no version at {index}")));
        }
        let mut chain = vec![index];
        let mut at = index;
        while let Some(via) = self.versions[at].via {
            at = self
                .index_of(via)
                .ok_or_else(|| ReviewError::Damaged(format!("the version {via} is gone")))?;
            if chain.len() > self.versions.len() {
                return Err(ReviewError::Damaged("the versions make a cycle".into()));
            }
            chain.push(at);
        }
        if Some(self.versions[at].meta.id) != self.newest {
            return Err(ReviewError::Damaged(
                "a chain does not reach the newest version".into(),
            ));
        }
        let mut state = self.newest_state()?;
        for &i in chain[..chain.len() - 1].iter().rev() {
            state.apply_all(&self.versions[i].delta)?;
        }
        Ok(state)
    }

    /// The state of the version with this ID.
    pub fn state_of(&self, id: u64) -> Result<State, ReviewError> {
        let i = self.index_of(id).ok_or(ReviewError::NotFound(id))?;
        self.state_at(i)
    }

    /// The state of the newest version, if there is one.
    pub fn tip_state(&self) -> Result<Option<State>, ReviewError> {
        if self.versions.is_empty() {
            return Ok(None);
        }
        self.newest_state().map(Some)
    }

    /// True if the document has the content of the newest version.
    pub fn document_is_tip(&self, doc: &Document) -> Result<bool, ReviewError> {
        Ok(match self.tip_state()? {
            None => false,
            Some(tip) => State::of(doc, Some(&tip)) == tip,
        })
    }

    /// Saves the document as the newest version. The old newest version becomes a reverse delta
    /// from the new one. If a limit would be broken, nothing is saved and the history is
    /// unchanged: the error lists the versions (see [`ReviewError::LimitReached`]).
    pub fn add(
        &mut self,
        doc: &Document,
        new: NewVersion,
        limits: Limits,
    ) -> Result<Added, ReviewError> {
        let limits = limits.clamped();
        if limits.max_versions == 0 {
            return Err(ReviewError::Disabled);
        }
        let name = new.name.trim();
        if name.is_empty() {
            return Err(ReviewError::Invalid("a version needs a name".into()));
        }
        let too_long = |what: &str, text: &str, max: usize| {
            (text.chars().count() > max)
                .then(|| ReviewError::Invalid(format!("the {what} may have {max} characters")))
        };
        if let Some(e) = too_long("name", name, NAME_MAX) {
            return Err(e);
        }
        if let Some(e) = new
            .note
            .as_deref()
            .and_then(|n| too_long("note", n, NOTE_MAX))
        {
            return Err(e);
        }
        if let Some(e) = too_long("author", &new.author, AUTHOR_MAX) {
            return Err(e);
        }
        let tip = self.tip_state()?;
        // The first version makes the drawing the anchor. The others are stored against it.
        let first_anchor = tip.is_none().then(|| Arc::new(State::of(doc, None)));
        let state = match &first_anchor {
            Some(a) => (**a).clone(),
            None => State::of(doc, tip.as_ref()),
        };
        let id = self.next_id;
        let meta = VersionMeta {
            id,
            name: name.to_string(),
            note: new.note.filter(|n| !n.trim().is_empty()),
            author: new.author,
            time_ms: new.time_ms,
            origin: new.origin,
            parents: self
                .head
                .filter(|h| self.index_of(*h).is_some())
                .into_iter()
                .collect(),
            branch: self.branch.clone(),
            base_revision: new.base_revision,
            auto: new.auto,
        };
        let mut next = self.clone();
        next.next_id = id.saturating_add(1);
        // The old newest version becomes a reverse delta from the new one.
        if let (Some(tip), Some(old)) = (&tip, next.newest.and_then(|n| next.index_of(n))) {
            let delta = state.diff(tip);
            next.take_blobs(&delta, tip);
            let v = &mut next.versions[old];
            v.via = Some(id);
            v.bytes = ops_bytes(&delta) + meta_bytes(&v.meta);
            v.delta = delta;
            v.json = Memo::default();
        }
        if let Some(a) = first_anchor {
            next.anchor_digest = anchor_digest(&a);
            next.anchor = a;
        }
        next.set_newest_delta(next.anchor.diff(&state));
        let delta = next.newest_delta.clone();
        next.take_blobs(&delta, &state);
        let bytes = meta_bytes(&meta);
        next.versions.push(Version {
            meta,
            via: None,
            delta: Vec::new(),
            bytes,
            json: Memo::default(),
        });
        next.newest = Some(id);
        next.head = Some(id);
        next.collect_blobs();
        next.check_limits(limits, &self.infos())?;
        *self = next;
        Ok(Added { id })
    }

    /// Checks the thresholds for this history, which has the new version in it. `existing` lists
    /// the versions that a person can choose from.
    fn check_limits(&self, limits: Limits, existing: &[VersionInfo]) -> Result<(), ReviewError> {
        let refuse = |kind, max, needed| {
            Err(ReviewError::LimitReached(Box::new(LimitInfo {
                kind,
                max,
                needed,
                versions: existing.to_vec(),
            })))
        };
        if self.versions.len() > limits.max_versions {
            return refuse(
                LimitKind::Versions,
                limits.max_versions,
                self.versions.len(),
            );
        }
        if self.bytes() > limits.max_bytes {
            return refuse(LimitKind::Bytes, limits.max_bytes, self.bytes());
        }
        Ok(())
    }

    /// The versions with the bytes that deleting each one would free, oldest first.
    pub fn infos(&self) -> Vec<VersionInfo> {
        // How many operations refer to each picture, and how many of them are in each delta.
        let count = |ops: &[Op], into: &mut BTreeMap<String, usize>| {
            for op in ops {
                op_refs(op, &mut |h| {
                    *into.entry(h.to_string()).or_default() += 1;
                });
            }
        };
        let mut all: BTreeMap<String, usize> = BTreeMap::new();
        count(&self.newest_delta, &mut all);
        for v in &self.versions {
            count(&v.delta, &mut all);
        }
        self.versions
            .iter()
            .map(|v| {
                let mut own: BTreeMap<String, usize> = BTreeMap::new();
                count(&v.delta, &mut own);
                if self.newest == Some(v.meta.id) {
                    count(&self.newest_delta, &mut own);
                }
                let pictures: usize = own
                    .iter()
                    .filter(|(h, n)| all.get(h.as_str()) == Some(*n))
                    .filter_map(|(h, _)| self.blobs.get(h))
                    .map(|t| t.len())
                    .sum();
                VersionInfo {
                    id: v.meta.id,
                    name: v.meta.name.clone(),
                    bytes: v.bytes + pictures,
                    time_ms: v.meta.time_ms,
                    auto: v.meta.auto,
                }
            })
            .collect()
    }

    /// Deletes a version. The other versions keep their content: a version whose delta started
    /// from it starts from the version after it instead, so its delta is joined with the delta of
    /// the deleted one. Returns true if no version is left.
    pub fn remove(&mut self, id: u64) -> Result<bool, ReviewError> {
        let i = self.index_of(id).ok_or(ReviewError::NotFound(id))?;
        if self.versions.len() == 1 {
            *self = History {
                next_id: self.next_id,
                branch: self.branch.clone(),
                ..History::new()
            };
            return Ok(true);
        }
        let mut next = self.clone();
        let children: Vec<u64> = next
            .versions
            .iter()
            .filter(|v| v.via == Some(id))
            .map(|v| v.meta.id)
            .collect();
        let parent_of_removed = next.versions[i].meta.parents.first().copied();
        if next.newest == Some(id) {
            // The version made from it becomes the newest.
            let Some(&first) = children.last() else {
                return Err(ReviewError::Damaged(format!(
                    "version {id} is the newest but nothing leads to it"
                )));
            };
            let newest_state = next.newest_state()?;
            let mut states: Vec<(u64, State)> = Vec::new();
            for c in &children {
                let mut s = newest_state.clone();
                s.apply_all(&next.versions[next.index_of(*c).expect("a child")].delta)?;
                states.push((*c, s));
            }
            let head_state = states
                .iter()
                .find(|(c, _)| *c == first)
                .map(|(_, s)| s.clone())
                .expect("the first child");
            next.versions.remove(i);
            next.set_newest_delta(next.anchor.diff(&head_state));
            let delta = next.newest_delta.clone();
            next.take_blobs(&delta, &head_state);
            next.newest = Some(first);
            for (c, s) in &states {
                let k = next.index_of(*c).expect("a child");
                let v = &mut next.versions[k];
                if *c == first {
                    v.via = None;
                    v.delta.clear();
                } else {
                    v.via = Some(first);
                    v.delta = head_state.diff(s);
                }
                v.bytes = ops_bytes(&v.delta) + meta_bytes(&v.meta);
                v.json = Memo::default();
            }
        } else {
            let via = next.versions[i].via.ok_or_else(|| {
                ReviewError::Damaged(format!("version {id} has no way to the newest version"))
            })?;
            let via_state = next.state_of(via)?;
            let mut moved: Vec<(u64, State)> = Vec::new();
            for c in &children {
                moved.push((*c, next.state_of(*c)?));
            }
            next.versions.remove(i);
            for (c, s) in moved {
                let delta = via_state.diff(&s);
                // The pictures of the joined delta are in the table already.
                let k = next.index_of(c).expect("a child");
                let v = &mut next.versions[k];
                v.via = Some(via);
                v.bytes = ops_bytes(&delta) + meta_bytes(&v.meta);
                v.delta = delta;
                v.json = Memo::default();
            }
        }
        // The parents that named the deleted version name its first parent instead.
        for v in &mut next.versions {
            if v.meta.parents.contains(&id) {
                let mut seen = Vec::new();
                for p in v.meta.parents.iter().filter_map(|p| {
                    if *p == id {
                        parent_of_removed
                    } else {
                        Some(*p)
                    }
                }) {
                    if !seen.contains(&p) {
                        seen.push(p);
                    }
                }
                v.meta.parents = seen;
                v.json = Memo::default();
            }
        }
        if next.head == Some(id) {
            next.head = parent_of_removed
                .filter(|p| next.index_of(*p).is_some())
                .or(next.newest);
        }
        next.collect_blobs();
        *self = next;
        Ok(false)
    }

    /// Deletes these versions (an action of the person). Nothing changes if one of the IDs does
    /// not exist. Returns the deleted versions, oldest first.
    pub fn remove_many(&mut self, ids: &[u64]) -> Result<Vec<VersionInfo>, ReviewError> {
        if let Some(missing) = ids.iter().find(|id| self.index_of(**id).is_none()) {
            return Err(ReviewError::NotFound(*missing));
        }
        let gone: Vec<VersionInfo> = self
            .infos()
            .into_iter()
            .filter(|v| ids.contains(&v.id))
            .collect();
        let mut next = self.clone();
        for v in &gone {
            if next.index_of(v.id).is_some() {
                next.remove(v.id)?;
            }
        }
        *self = next;
        Ok(gone)
    }

    /// The JSON of each version. A version that did not change since it was last asked for gives
    /// the same memory.
    fn versions_json(&self) -> Vec<Arc<Value>> {
        self.versions
            .iter()
            .map(|v| {
                v.json
                    .0
                    .get_or_init(|| {
                        let mut m = serde_json::to_value(&v.meta).unwrap_or(Value::Null);
                        if let Some(via) = v.via {
                            m["via"] = json!(via);
                        }
                        if !v.delta.is_empty() {
                            m["delta"] = json!(v.delta);
                        }
                        Arc::new(m)
                    })
                    .clone()
            })
            .collect()
    }

    /// The parts of the JSON that are not versions or pictures.
    fn top_json(&self, anchor: &str, newest_delta: Option<&[Op]>) -> Value {
        let mut newest = json!({"id": self.newest, "anchor": anchor});
        if let Some(d) = newest_delta {
            newest["delta"] = json!(d);
        }
        json!({
            "format": FORMAT,
            "next_id": self.next_id,
            "branch": self.branch,
            "head": self.head,
            "newest": newest,
        })
    }

    fn json_with(
        &self,
        anchor: &str,
        newest_delta: &[Op],
        blobs: &BTreeMap<String, Arc<str>>,
    ) -> Value {
        let blobs: serde_json::Map<String, Value> = blobs
            .iter()
            .map(|(h, t)| (h.clone(), Value::String(t.to_string())))
            .collect();
        let mut top = self.top_json(anchor, Some(newest_delta));
        top["blobs"] = Value::Object(blobs);
        top["versions"] =
            Value::Array(self.versions_json().iter().map(|v| (**v).clone()).collect());
        top
    }

    /// The history as the editor holds it: the same JSON as `to_value`, in parts. The versions
    /// that did not change and the pictures are the same memory as in the value that this history
    /// made before, so that many undo steps do not hold many copies.
    pub fn to_data(&self) -> HistoryData {
        let blobs: Vec<(String, Arc<str>)> = self
            .blobs
            .iter()
            .filter(|(h, _)| !self.anchor.blobs().contains_key(*h))
            .map(|(h, t)| (h.clone(), t.clone()))
            .collect();
        HistoryData::from_parts(
            self.top_json(&self.anchor_digest, None),
            self.versions_json(),
            self.newest_json.0.to_vec(),
            blobs,
        )
    }

    /// The JSON against the anchor of this history (what the open document holds). The pictures
    /// that the anchor holds are not in it.
    pub fn to_value(&self) -> Value {
        let blobs: BTreeMap<String, Arc<str>> = self
            .blobs
            .iter()
            .filter(|(h, _)| !self.anchor.blobs().contains_key(*h))
            .map(|(h, t)| (h.clone(), t.clone()))
            .collect();
        self.json_with(&self.anchor_digest, &self.newest_delta, &blobs)
    }

    /// The JSON for a file that holds this document: the newest version as a change from the
    /// state of the document, with the digest of that state, and without the pictures that the
    /// document holds. `tip` is the state of the newest version. Returns the JSON and the state of
    /// the document (`State::stamp_keys` writes its keys into the document that is saved).
    pub fn to_disk(&self, doc: &Document, tip: &State) -> (Value, State) {
        let a = State::of(doc, Some(tip));
        let delta = a.diff(tip);
        // Every picture that a delta or the newest version needs, but the drawing.
        let mut table: BTreeMap<String, Arc<str>> = self.blobs.clone();
        for h in tip.used_blobs() {
            if let Some(t) = tip.blobs().get(h) {
                table.insert(h.to_string(), t.clone());
            }
        }
        table.retain(|h, _| !a.blobs().contains_key(h));
        (self.json_with(&anchor_digest(&a), &delta, &table), a)
    }

    /// Reads the JSON form of a file or a peer. `doc` is the drawing that the file holds, which
    /// the file form is relative to. Never fails and never panics: it keeps what fits and says
    /// what it repaired or left out (see the module text). The caller keeps the JSON itself, and
    /// does not replace it with this reading without the consent of the person.
    pub fn load(value: &Value, doc: &Document) -> Loaded {
        History::load_with(value, doc, &[])
    }

    /// Like [`History::load`], with anchors that the caller knows (see the module text): the
    /// JSON is read against the one whose digest it names, else against the drawing.
    pub fn load_with(value: &Value, doc: &Document, known: &[(String, Arc<State>)]) -> Loaded {
        History::load_with_caps(value, doc, known, ReadCaps::default())
    }

    /// Like [`History::load_with`], with the caps of the host (see [`ReadCaps`]).
    pub fn load_with_caps(
        value: &Value,
        doc: &Document,
        known: &[(String, Arc<State>)],
        caps: ReadCaps,
    ) -> Loaded {
        let mut notes = Vec::new();
        let history = read(value, doc, known, caps, &mut notes);
        Loaded {
            history: history.filter(|h| !h.is_empty()),
            notes,
        }
    }
}

impl Default for History {
    fn default() -> Self {
        History::new()
    }
}

/// The hashes of the pictures that an operation refers to.
fn op_refs(op: &Op, found: &mut dyn FnMut(&str)) {
    match op {
        Op::Create { props, .. } => {
            for key in BLOB_PROPS {
                if let Some(h) = props.get(key).and_then(blob_ref) {
                    found(h);
                }
            }
        }
        Op::Set { prop, value, .. } if BLOB_PROPS.contains(&prop.as_str()) => {
            if let Some(h) = blob_ref(value) {
                found(h);
            }
        }
        _ => {}
    }
}

/// Names the first picture that the operations need and the table lacks.
fn missing_blob(ops: &[Op], blobs: &BTreeMap<String, Arc<str>>) -> Option<String> {
    let mut missing = None;
    for op in ops {
        op_refs(op, &mut |h| {
            if missing.is_none() && !blobs.contains_key(h) {
                missing = Some(h.to_string());
            }
        });
    }
    missing
}

/// Reads the table of pictures. A picture whose text does not fit its hash, or is too large, is
/// left out with a note.
fn read_blobs(
    value: Option<&Value>,
    total: &mut usize,
    cap: usize,
    notes: &mut Vec<String>,
) -> BTreeMap<String, Arc<str>> {
    let mut out = BTreeMap::new();
    let Some(map) = value.and_then(Value::as_object) else {
        if value.is_some() {
            notes.push(
                "the table of pictures of the history is not a table and was left out".into(),
            );
        }
        return out;
    };
    for (hash, text) in map {
        let Some(text) = text.as_str() else {
            notes.push("a picture of the history is not text and was left out".into());
            continue;
        };
        *total = total.saturating_add(text.len());
        if text.len() > MAX_BLOB_BYTES || *total > cap {
            notes.push("a picture of the history is too large and was left out".into());
            *total = total.saturating_sub(text.len());
            continue;
        }
        if sha256_hex(text.as_bytes()) != *hash {
            notes.push("a picture of the history did not match its hash and was left out".into());
            continue;
        }
        out.insert(hash.clone(), Arc::from(text));
    }
    out
}

/// Reads the ops of a JSON array: each one is checked, and the running size is bounded.
fn read_ops(
    value: Option<&Value>,
    total: &mut usize,
    cap: usize,
    what: &str,
) -> Result<Vec<Op>, String> {
    let Some(list) = value else {
        return Ok(Vec::new());
    };
    let list = list
        .as_array()
        .ok_or_else(|| format!("{what} is not a list"))?;
    let mut out = Vec::with_capacity(list.len().min(4096));
    for v in list {
        let op: Op = serde_json::from_value(v.clone())
            .map_err(|e| format!("{what} has an operation that cannot be read: {e}"))?;
        check_op(&op).map_err(|e| format!("{what} has a bad operation: {e}"))?;
        *total = total.saturating_add(op_size(&op));
        if *total > cap {
            return Err(format!("{what} is too large"));
        }
        out.push(op);
    }
    Ok(out)
}

/// The state that the JSON of a history is written against (its anchor), made from the drawing:
/// the drawing itself, or, when the file has a `base` (the changes that turn the drawing into the
/// drawing that the versions were written against), that drawing. An error says why not.
fn against_drawing(
    base: Option<&Value>,
    digest: &str,
    doc: &Document,
    table: &BTreeMap<String, Arc<str>>,
    cap: usize,
) -> Result<State, String> {
    let drawing = State::of(doc, None);
    if anchor_digest(&drawing) == digest {
        return Ok(drawing);
    }
    let changed = "the drawing was changed after the versions were saved (by a program that did not update them), so the newest version cannot be rebuilt";
    let Some(base) = base.and_then(Value::as_object) else {
        return Err(changed.into());
    };
    if base.get("anchor").and_then(Value::as_str) != Some(&anchor_digest(&drawing)) {
        return Err(changed.into());
    }
    let ops = read_ops(base.get("delta"), &mut 0, cap, "the base of the history")?;
    let mut state = drawing;
    let mut all = state.blobs().clone();
    all.extend(table.iter().map(|(h, t)| (h.clone(), t.clone())));
    if let Some(h) = missing_blob(&ops, &all) {
        return Err(format!(
            "the base of the history needs the picture {h}, which is missing"
        ));
    }
    state.set_blobs(all);
    state
        .apply_all(&ops)
        .map_err(|e| format!("the base of the history does not fit: {e}"))?;
    if anchor_digest(&state) != digest {
        return Err(changed.into());
    }
    let used: std::collections::HashSet<String> =
        state.used_blobs().iter().map(|h| h.to_string()).collect();
    let mut pictures = state.blobs().clone();
    pictures.retain(|h, _| used.contains(h));
    state.set_blobs(pictures);
    Ok(state)
}

/// The drawing that a history in JSON form was written against, if the document is that drawing
/// or has the `base` that leads back to it. `top` is the history without its versions and
/// pictures, `blobs` its pictures (see `HistoryData`). A host that keeps a history as it is
/// (because it could not be read completely) keeps this state with it, and writes it with
/// [`rebase_for_save`] when the drawing changes.
pub fn written_against(
    top: &Value,
    blobs: &[(String, Arc<str>)],
    doc: &Document,
) -> Option<Arc<State>> {
    let digest = top.get("newest")?.get("anchor")?.as_str()?;
    let table: BTreeMap<String, Arc<str>> = blobs.iter().cloned().collect();
    against_drawing(top.get("base"), digest, doc, &table, HARD_MAX_BYTES)
        .ok()
        .map(Arc::new)
}

/// What a save adds to a history that is kept as it is, so that the versions can still be read
/// after the drawing changed: the `base` entry (the changes that turn the drawing `now` into the
/// drawing `written` that the history was written against, with the digest of `now`) or none when
/// they are equal, and the pictures that the changes need.
pub fn rebase_for_save(now: &State, written: &State) -> (Option<Value>, Vec<(String, Arc<str>)>) {
    let delta = now.diff(written);
    if delta.is_empty() {
        return (None, Vec::new());
    }
    let mut needed: BTreeMap<String, Arc<str>> = BTreeMap::new();
    for op in &delta {
        op_refs(op, &mut |h| {
            if let Some(t) = written.blobs().get(h) {
                needed.insert(h.to_string(), t.clone());
            }
        });
    }
    (
        Some(json!({"anchor": anchor_digest(now), "delta": delta})),
        needed.into_iter().collect(),
    )
}

fn read(
    value: &Value,
    doc: &Document,
    known: &[(String, Arc<State>)],
    caps: ReadCaps,
    notes: &mut Vec<String>,
) -> Option<History> {
    let drop_all = |notes: &mut Vec<String>, why: String| {
        notes.push(format!("the history of versions cannot be read: {why}"));
        None
    };
    let Some(obj) = value.as_object() else {
        return drop_all(notes, "it is not an object".into());
    };
    match obj.get("format").and_then(Value::as_u64) {
        Some(FORMAT) => {}
        other => return drop_all(notes, format!("its format {other:?} is not known")),
    }
    let mut total = 0usize;
    let mut table = read_blobs(obj.get("blobs"), &mut total, caps.max_bytes, notes);
    let Some(newest) = obj.get("newest").and_then(Value::as_object) else {
        return drop_all(notes, "it has no newest version".into());
    };
    let list = match obj.get("versions") {
        Some(Value::Array(a)) => a.as_slice(),
        Some(_) => return drop_all(notes, "the versions are not a list".into()),
        None => &[],
    };
    if list.is_empty() {
        return drop_all(notes, "it has no versions".into());
    }

    // The anchor: a known one, or the drawing.
    let digest = newest.get("anchor").and_then(Value::as_str).unwrap_or("");
    let anchor: Arc<State> = match known.iter().find(|(d, _)| d == digest) {
        Some((_, a)) => a.clone(),
        None => match against_drawing(obj.get("base"), digest, doc, &table, caps.max_bytes) {
            Ok(a) => Arc::new(a),
            Err(why) => return drop_all(notes, why),
        },
    };
    let newest_delta = match read_ops(
        newest.get("delta"),
        &mut total,
        caps.max_bytes,
        "the newest version",
    ) {
        Ok(o) => o,
        Err(e) => return drop_all(notes, e),
    };
    for (h, t) in anchor.blobs() {
        table.entry(h.clone()).or_insert_with(|| t.clone());
    }
    if let Some(h) = missing_blob(&newest_delta, &table) {
        return drop_all(
            notes,
            format!("the newest version needs the picture {h}, which is missing"),
        );
    }
    let mut state = (*anchor).clone();
    state.set_blobs(table.clone());
    if let Err(e) = state.apply_all(&newest_delta) {
        return drop_all(notes, format!("the newest version does not fit: {e}"));
    }

    // The versions, newest first: each delta turns the state of the newer version into this one.
    if list.len() > caps.max_versions {
        notes.push(format!(
            "the history had {} versions: only the newest {} were read",
            list.len(),
            caps.max_versions
        ));
    }
    let take = list.len().min(caps.max_versions);
    let mut kept: Vec<(VersionMeta, Vec<Op>, Option<Value>)> = Vec::new();
    for (n, item) in list.iter().rev().take(take).enumerate() {
        let parsed = serde_json::from_value::<VersionMeta>(item.clone())
            .map_err(|e| format!("its description cannot be read: {e}"))
            .and_then(|meta| {
                let delta = if n == 0 {
                    Vec::new()
                } else {
                    read_ops(item.get("delta"), &mut total, caps.max_bytes, "a delta")?
                };
                if let Some(h) = missing_blob(&delta, &table) {
                    return Err(format!("it needs the picture {h}, which is missing"));
                }
                state
                    .apply_all(&delta)
                    .map_err(|e| format!("its changes do not fit: {e}"))?;
                Ok((meta, delta))
            });
        match parsed {
            Ok((meta, delta)) => kept.push((meta, delta, item.get("via").cloned())),
            Err(why) => {
                notes.push(format!(
                    "version {} of the history and the {} older ones were left out: {why}",
                    list.len() - n,
                    list.len() - n - 1
                ));
                break;
            }
        }
    }
    kept.reverse();
    if kept.is_empty() {
        return None;
    }

    // Repairs, oldest first.
    let mut history = History {
        next_id: obj
            .get("next_id")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1),
        anchor,
        anchor_digest: digest.to_string(),
        newest_json: OpsJson(Arc::new(ops_json(&newest_delta, &[], &[]))),
        newest_delta,
        newest: None,
        versions: Vec::new(),
        branch: main_branch(),
        head: obj.get("head").and_then(Value::as_u64),
        blobs: table,
    };
    let mut repaired = false;
    // A value of the wrong type is replaced by the default, and that is a repair.
    for (key, ok) in [
        ("branch", obj.get("branch").is_none_or(|v| v.is_string())),
        ("next_id", obj.get("next_id").is_none_or(|v| v.is_u64())),
        (
            "head",
            obj.get("head").is_none_or(|v| v.is_null() || v.is_u64()),
        ),
    ] {
        if !ok {
            let _ = key;
            repaired = true;
        }
    }
    if let Some(b) = obj.get("branch").and_then(Value::as_str) {
        let c = cut(b.trim(), BRANCH_MAX);
        if !c.is_empty() {
            repaired |= c != b;
            history.branch = c;
        } else {
            repaired = true;
        }
    }
    let mut seen: HashSet<u64> = HashSet::new();
    let mut given_via: Vec<Option<Value>> = Vec::new();
    for (i, (mut meta, delta, via)) in kept.into_iter().enumerate() {
        given_via.push(via);
        let (name, note, author) = (meta.name.clone(), meta.note.clone(), meta.author.clone());
        meta.name = cut(name.trim(), NAME_MAX);
        if meta.name.is_empty() {
            meta.name = format!("Version {}", i + 1);
            repaired = true;
        }
        meta.note = note
            .clone()
            .map(|n| cut(&n, NOTE_MAX))
            .filter(|n| !n.trim().is_empty());
        meta.author = cut(&author, AUTHOR_MAX);
        repaired |= meta.name != name || meta.note != note || meta.author != author;
        if !seen.insert(meta.id) {
            meta.id = history.next_id;
            seen.insert(meta.id);
            repaired = true;
        }
        history.next_id = history.next_id.max(meta.id.saturating_add(1));
        // Parents: the first is the version before (the loader replays the file in order), the
        // others must be earlier versions.
        let before = history.versions.last().map(|v| v.meta.id);
        let old_parents = std::mem::take(&mut meta.parents);
        meta.parents = before.into_iter().collect();
        for p in old_parents.iter().skip(1) {
            if meta.parents.len() < PARENTS_MAX
                && !meta.parents.contains(p)
                && history.versions.iter().any(|v| v.meta.id == *p)
            {
                meta.parents.push(*p);
            }
        }
        repaired |=
            old_parents.first().copied() != before || old_parents.len() != meta.parents.len();
        let branch = cut(meta.branch.trim(), BRANCH_MAX);
        meta.branch = if branch.is_empty() {
            main_branch()
        } else {
            branch
        };
        let bytes = ops_bytes(&delta) + meta_bytes(&meta);
        history.versions.push(Version {
            meta,
            via: None,
            delta,
            bytes,
            json: Memo::default(),
        });
    }
    // Each version but the newest starts from the next one.
    let ids: Vec<u64> = history.versions.iter().map(|v| v.meta.id).collect();
    for (k, v) in history.versions.iter_mut().enumerate() {
        v.via = ids.get(k + 1).copied();
        v.json = Memo::default();
        // The link is derived; a file that names another version is repaired.
        repaired |= given_via[k] != v.via.map(|x| json!(x));
    }
    if newest.get("id") != ids.last().map(|x| json!(x)).as_ref() {
        repaired = true;
    }
    history.newest = ids.last().copied();
    if history
        .head
        .is_none_or(|h| history.versions.iter().all(|v| v.meta.id != h))
    {
        history.head = history.newest;
        repaired = true;
    }
    history.collect_blobs();
    if repaired {
        notes.push("some names, notes, numbers or parents of versions were repaired".into());
    }
    Some(history)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod work_tests;

#[cfg(test)]
mod memory_tests;

#[cfg(test)]
mod opaque_tests;
