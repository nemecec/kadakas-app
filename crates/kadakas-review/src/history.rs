//! The version store, with reverse deltas.
//!
//! ```text
//!                                   snapshot of the newest version
//!  version 1 <--delta-- version 2 <--delta-- ... <--delta-- version n   (the newest)
//! ```
//!
//! The current document is the only full copy of the drawing in a file. The newest version is
//! stored as a *change from the current document* (it is made again at each save, which is a cheap
//! comparison), and every older version is stored as a *reverse delta* from the next newer
//! version on its chain of first parents: the list of property operations that turns the newer
//! version into this one. To open version k, the code starts at the newest version and applies the
//! deltas backwards (see [`History::state_at`]). A version of a drawing that changed a little is a
//! few small operations, and there is no separate base snapshot of the oldest version.
//!
//! # Two forms
//!
//! - The **memory form** (`History::to_value`) is what the document holds while it is open, in
//!   `Document::history`. It is self contained: the newest version is a snapshot (`newest.snapshot`,
//!   the operations that make it from nothing). The undo steps hold it, and it does not depend on
//!   the drawing, which changes with every edit.
//! - The **file form** (`History::to_disk`) is what a save writes. The newest version is the change
//!   from the drawing that the file holds (`newest.delta`) and a digest of that drawing
//!   (`newest.anchor`). The pictures that the drawing holds are not repeated. [`History::load`]
//!   reads both forms. The drawing never needs the history to load: it is read first, and the
//!   history is read afterwards, from the drawing and the file form, and only when it is used. A
//!   save also writes the stacking keys of the drawing (`State::stamp_keys`), so that the state of
//!   the drawing is the same when it is read again.
//!
//! # Pictures
//!
//! A big picture (the PNG of an underlay, the file of an image resource) is not stored in the
//! operations. As in collaboration, the property holds a reference `{"$blob": "<sha256>"}`, and
//! the history keeps a table `blobs` with the text of each picture once, by hash. A move of a
//! 10 MiB underlay in three versions costs 10 MiB once, not three times. A picture stays while the
//! snapshot or a delta refers to it (a count of references that is made again after a delete), and
//! the byte count counts each picture once. In the file form, a picture that the drawing holds is
//! not repeated.
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
//!  "newest": {"id": 3, "delta": [<operations: the drawing -> the newest version>], "anchor": "<sha256>"},
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
//! fit. Each repair is a note. A file form whose `anchor` is not the digest of the drawing (the
//! drawing was changed by a program that did not update the versions) cannot be read at all.
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
    /// The operations that make the state of the newest version from [`State::new`].
    snapshot: Vec<Op>,
    /// The ID of the newest version, which holds the snapshot.
    newest: Option<u64>,
    /// Oldest first.
    versions: Vec<Version>,
    /// The branch that new versions go on.
    branch: String,
    /// The version that the next one is made from: the newest on the current branch.
    head: Option<u64>,
    /// The pictures that the snapshot and the deltas refer to, once each, by the SHA-256 of the
    /// text.
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

/// The digest of a state, as text: the anchor of the file form.
fn anchor_of(state: &State) -> String {
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
            snapshot: Vec::new(),
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
    /// picture once). The snapshot of the newest version is not counted: a file stores that
    /// version as a change from the drawing.
    pub fn bytes(&self) -> usize {
        self.versions.iter().map(|v| v.bytes).sum::<usize>()
            + self.blobs.values().map(|t| t.len()).sum::<usize>()
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

    /// Keeps only the pictures that the snapshot or a delta still refers to.
    fn collect_blobs(&mut self) {
        let mut used = BTreeSet::new();
        for op in self
            .snapshot
            .iter()
            .chain(self.versions.iter().flat_map(|v| &v.delta))
        {
            op_refs(op, &mut |h| {
                used.insert(h.to_string());
            });
        }
        self.blobs.retain(|h, _| used.contains(h));
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

    /// The state of the newest version: the snapshot, applied.
    fn newest_state(&self) -> Result<State, ReviewError> {
        let mut state = State::new();
        state.set_blobs(self.blobs.clone());
        state.apply_all(&self.snapshot)?;
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
        let state = State::of(doc, tip.as_ref());
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
        }
        next.snapshot = state.snapshot();
        let snapshot = next.snapshot.clone();
        next.take_blobs(&snapshot, &state);
        let bytes = meta_bytes(&meta);
        next.versions.push(Version {
            meta,
            via: None,
            delta: Vec::new(),
            bytes,
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
            return refuse(LimitKind::Versions, limits.max_versions, self.versions.len());
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
        count(&self.snapshot, &mut all);
        for v in &self.versions {
            count(&v.delta, &mut all);
        }
        self.versions
            .iter()
            .map(|v| {
                let mut own: BTreeMap<String, usize> = BTreeMap::new();
                count(&v.delta, &mut own);
                if self.newest == Some(v.meta.id) {
                    count(&self.snapshot, &mut own);
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
            // The version made from it becomes the newest: its state is the snapshot.
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
            next.snapshot = head_state.snapshot();
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
            }
        }
        // The parents that named the deleted version name its first parent instead.
        for v in &mut next.versions {
            if v.meta.parents.contains(&id) {
                let mut seen = Vec::new();
                for p in v.meta.parents.iter().filter_map(|p| {
                    if *p == id { parent_of_removed } else { Some(*p) }
                }) {
                    if !seen.contains(&p) {
                        seen.push(p);
                    }
                }
                v.meta.parents = seen;
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

    fn versions_json(&self) -> Vec<Value> {
        self.versions
            .iter()
            .map(|v| {
                let mut m = serde_json::to_value(&v.meta).unwrap_or(Value::Null);
                if let Some(via) = v.via {
                    m["via"] = json!(via);
                }
                if !v.delta.is_empty() {
                    m["delta"] = json!(v.delta);
                }
                m
            })
            .collect()
    }

    fn json_with(&self, newest: Value, blobs: &BTreeMap<String, Arc<str>>) -> Value {
        let blobs: serde_json::Map<String, Value> = blobs
            .iter()
            .map(|(h, t)| (h.clone(), Value::String(t.to_string())))
            .collect();
        json!({
            "format": FORMAT,
            "next_id": self.next_id,
            "branch": self.branch,
            "head": self.head,
            "newest": newest,
            "blobs": blobs,
            "versions": self.versions_json(),
        })
    }

    /// The memory form: self contained, with the snapshot of the newest version (see the module
    /// text).
    pub fn to_value(&self) -> Value {
        self.json_with(
            json!({"id": self.newest, "snapshot": self.snapshot}),
            &self.blobs,
        )
    }

    /// The file form for a document: the newest version as a change from the state of the
    /// document, with the digest of that state, and without the pictures that the document holds.
    /// `tip` is the state of the newest version. Returns the JSON and the state of the document
    /// (`State::stamp_keys` writes its keys into the document that is saved).
    pub fn to_disk(&self, doc: &Document, tip: &State) -> (Value, State) {
        let a = State::of(doc, Some(tip));
        let delta = a.diff(tip);
        let blobs: BTreeMap<String, Arc<str>> = self
            .blobs
            .iter()
            .filter(|(h, _)| !a.blobs().contains_key(*h))
            .map(|(h, t)| (h.clone(), t.clone()))
            .collect();
        let newest = json!({"id": self.newest, "delta": delta, "anchor": anchor_of(&a)});
        (self.json_with(newest, &blobs), a)
    }

    /// Reads the JSON form of a file or a peer. `doc` is the drawing that the file holds, which
    /// the file form is relative to. Never fails and never panics: it keeps what fits and says
    /// what it repaired or left out (see the module text). The caller keeps the JSON itself, and
    /// does not replace it with this reading without the consent of the person.
    pub fn load(value: &Value, doc: &Document) -> Loaded {
        let mut notes = Vec::new();
        let history = read(value, doc, &mut notes);
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
    notes: &mut Vec<String>,
) -> BTreeMap<String, Arc<str>> {
    let mut out = BTreeMap::new();
    let Some(map) = value.and_then(Value::as_object) else {
        if value.is_some() {
            notes.push("the table of pictures of the history is not a table and was left out".into());
        }
        return out;
    };
    for (hash, text) in map {
        let Some(text) = text.as_str() else {
            notes.push("a picture of the history is not text and was left out".into());
            continue;
        };
        *total = total.saturating_add(text.len());
        if text.len() > MAX_BLOB_BYTES || *total > HARD_MAX_BYTES {
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
fn read_ops(value: Option<&Value>, total: &mut usize, what: &str) -> Result<Vec<Op>, String> {
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
        if *total > HARD_MAX_BYTES {
            return Err(format!("{what} is too large"));
        }
        out.push(op);
    }
    Ok(out)
}

fn read(value: &Value, doc: &Document, notes: &mut Vec<String>) -> Option<History> {
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
    let mut table = read_blobs(obj.get("blobs"), &mut total, notes);
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

    // The state of the newest version, from its snapshot or from the drawing.
    let mut state;
    if let Some(snapshot) = newest.get("snapshot") {
        let ops = match read_ops(Some(snapshot), &mut total, "the newest version") {
            Ok(o) => o,
            Err(e) => return drop_all(notes, e),
        };
        if let Some(h) = missing_blob(&ops, &table) {
            return drop_all(
                notes,
                format!("the newest version needs the picture {h}, which is missing"),
            );
        }
        state = State::new();
        state.set_blobs(table.clone());
        if let Err(e) = state.apply_all(&ops) {
            return drop_all(notes, format!("the newest version does not fit: {e}"));
        }
    } else {
        let drawing = State::of(doc, None);
        let anchor = newest.get("anchor").and_then(Value::as_str).unwrap_or("");
        if anchor_of(&drawing) != anchor {
            return drop_all(
                notes,
                "the drawing was changed after the versions were saved (by a program that did not update them), so the newest version cannot be rebuilt".into(),
            );
        }
        let ops = match read_ops(newest.get("delta"), &mut total, "the newest version") {
            Ok(o) => o,
            Err(e) => return drop_all(notes, e),
        };
        for (h, t) in drawing.blobs() {
            table.entry(h.clone()).or_insert_with(|| t.clone());
        }
        if let Some(h) = missing_blob(&ops, &table) {
            return drop_all(
                notes,
                format!("the newest version needs the picture {h}, which is missing"),
            );
        }
        state = drawing;
        state.set_blobs(table.clone());
        if let Err(e) = state.apply_all(&ops) {
            return drop_all(notes, format!("the newest version does not fit: {e}"));
        }
    }
    let snapshot = state.snapshot();

    // The versions, newest first: each delta turns the state of the newer version into this one.
    if list.len() > HARD_MAX_VERSIONS {
        notes.push(format!(
            "the history had {} versions: only the newest {HARD_MAX_VERSIONS} were read",
            list.len()
        ));
    }
    let take = list.len().min(HARD_MAX_VERSIONS);
    let mut kept: Vec<(VersionMeta, Vec<Op>, Option<Value>)> = Vec::new();
    for (n, item) in list.iter().rev().take(take).enumerate() {
        let parsed = serde_json::from_value::<VersionMeta>(item.clone())
            .map_err(|e| format!("its description cannot be read: {e}"))
            .and_then(|meta| {
                let delta = if n == 0 {
                    Vec::new()
                } else {
                    read_ops(item.get("delta"), &mut total, "a delta")?
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
        snapshot,
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
        ("head", obj.get("head").is_none_or(|v| v.is_null() || v.is_u64())),
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
        });
    }
    // Each version but the newest starts from the next one.
    let ids: Vec<u64> = history.versions.iter().map(|v| v.meta.id).collect();
    for (k, v) in history.versions.iter_mut().enumerate() {
        v.via = ids.get(k + 1).copied();
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
