//! The version store: one full snapshot as the base, and every later version as a list of property
//! operations from the version before it.
//!
//! ```text
//! base (the state of the first version) --delta 2--> version 2 --delta 3--> version 3 ...
//! ```
//!
//! The state of a version is rebuilt by applying the deltas in order (see [`History::state_at`]).
//! No version holds a full copy, so a history of many small edits is small.
//!
//! # Pictures
//!
//! A big picture (the PNG of an underlay, the file of an image resource) is not stored in the
//! operations. As in collaboration, the property holds a reference `{"$blob": "<sha256>"}`, and
//! the history keeps a table `blobs` with the text of each picture once, by hash. A move of a
//! 10 MiB underlay in three versions costs 10 MiB once, not three times. A picture stays while a
//! base or a delta refers to it (a count of references that is made again after a fold or a
//! delete), and the byte cap counts each picture once.
//!
//! # Parents and branches
//!
//! The versions are a graph, not a list, so branches can come later (REV-10). Each version has
//! `parents`, the IDs of the versions it was made from, the first parent first (a merge has two;
//! the root has none) and a `branch` name (default `main`). A delta is always relative to the
//! **first parent**, and the state of a version is the base with the deltas along the chain of
//! first parents applied. The history has a current `branch` and its `head`, the version that the
//! next one is made from. Today every version has the previous one as its only parent, and there
//! are no calls that make a branch or a merge yet. [`History::load`] makes the first-parent chain
//! linear: it sets the first parent of every version to the version before it in the file (and
//! says so in a note), because it replays the file in order; it keeps the other parents that
//! point at earlier versions. The rest of the code (state, fold, delete) already works on the
//! graph. Folding and deleting keep the graph whole: a version
//! that is the first parent of a kept version is never lost, the kept version is cut loose from it
//! with a delta against the new first parent instead.
//!
//! # Caps
//!
//! [`Limits`] caps the number of versions and the bytes of the history. When a new version breaks
//! a cap, the oldest versions go first and are folded into the base in one pass (the file is
//! replayed once and the snapshot is made once, however many versions go): the base becomes the
//! state of the first version that stays, so every version that stays keeps its state exactly. Deleting a version from
//! the middle joins its delta to the next one by comparing the states on both sides.
//!
//! # The file
//!
//! The history is JSON in the field `history` of the file (the core holds it as opaque JSON):
//!
//! ```json
//! {"format": 1, "next_id": 4,
//!  "base": [<Create and Set operations that make the first version from nothing>],
//!  "branch": "main", "head": 3,
//!  "blobs": {"<sha256>": "<text of a picture>"},
//!  "versions": [{"id": 1, "name": "...", "note": "...", "author": "...", "time_ms": 0,
//!                "origin": "user", "base_revision": 7, "auto": false, "delta": [<ops>]}, ...]}
//! ```
//!
//! # Untrusted data
//!
//! A file or a collaboration peer can hand over any JSON. [`History::load`] never panics and does
//! bounded work: it reads at most [`HARD_MAX_VERSIONS`] versions and [`HARD_MAX_BYTES`] bytes of
//! operations, checks every operation with the rules of collaboration, replays the whole chain,
//! cuts long texts, gives a repeated ID a new one, and drops a version (and the later ones, which
//! depend on it) whose delta does not fit. Each repair is a note.
//!
//! # Collaboration
//!
//! The history is local to a file. It is not part of the shared properties, so joining a shared
//! document keeps the versions of the open file (`Editor::apply_remote` leaves them alone), and
//! the other users have their own. A server that shares versions is REV-09.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use kadakas_core::change::Origin;
use kadakas_core::model::Document;
use kadakas_props::blob::{BLOB_PROPS, MAX_BLOB_BYTES, blob_ref, sha256_hex};
use kadakas_props::op::{Op, op_size};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::state::{ApplyError, State, check_op};

/// The version of the JSON form.
pub const FORMAT: u64 = 1;
/// The most versions that [`History::load`] reads, whatever the limits say.
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

/// The caps of a history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most versions. 0 turns versions off.
    pub max_versions: usize,
    /// The most bytes of the history (an estimate of its JSON).
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
    /// The limits, cut to what a file may hold at all.
    pub fn clamped(self) -> Limits {
        Limits {
            max_versions: self.max_versions.min(HARD_MAX_VERSIONS),
            max_bytes: self.max_bytes.min(HARD_MAX_BYTES),
        }
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
    #[error(
        "the history would need {needed} bytes, the limit is {max}: delete versions or raise the limit"
    )]
    TooLarge { needed: usize, max: usize },
    /// A version cannot be rebuilt. Cannot happen for a history that [`History::load`] checked.
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
    /// The versions this one was made from, the first parent first. The delta of this version is
    /// relative to the first parent. Empty for the first version.
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
    /// The operations from the version before. Empty for the first version.
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
    /// The operations that make the state of the first version from [`State::new`].
    base: Vec<Op>,
    base_bytes: usize,
    versions: Vec<Version>,
    /// The branch that new versions go on.
    branch: String,
    /// The version that the next one is made from: the newest on the current branch.
    head: Option<u64>,
    /// The pictures that the base and the deltas refer to, once each, by the SHA-256 of the text.
    blobs: BTreeMap<String, Arc<str>>,
}

/// The result of [`History::load`]: the history that could be kept, and what was repaired.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub history: Option<History>,
    pub notes: Vec<String>,
}

/// The result of [`History::add`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Added {
    pub id: u64,
    /// The IDs of the oldest versions that the caps pushed out, oldest first.
    pub dropped: Vec<u64>,
}

fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

impl History {
    fn empty() -> History {
        History {
            next_id: 1,
            base: Vec::new(),
            base_bytes: 0,
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

    /// A history with no versions. Use [`History::add`] to make the first one.
    pub fn new() -> History {
        History::empty()
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

    /// The size of the history in bytes, estimated from its operations, texts and pictures (each
    /// picture once).
    pub fn bytes(&self) -> usize {
        self.base_bytes
            + self.versions.iter().map(|v| v.bytes).sum::<usize>()
            + self.blobs.values().map(|t| t.len()).sum::<usize>()
    }

    /// The pictures of the history, by hash.
    pub fn blobs(&self) -> &BTreeMap<String, Arc<str>> {
        &self.blobs
    }

    /// Keeps only the pictures that the base or a delta still refers to.
    fn collect_blobs(&mut self) {
        let mut used = BTreeSet::new();
        for op in self
            .base
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

    /// The size of the delta of a version in bytes, estimated.
    pub fn delta_bytes(&self, id: u64) -> Option<usize> {
        self.index_of(id)
            .map(|i| ops_bytes(&self.versions[i].delta))
    }

    /// The state of the version at `index` (0 is the oldest, the root): the base with the deltas
    /// along the chain of first parents applied.
    pub fn state_at(&self, index: usize) -> Result<State, ReviewError> {
        if index >= self.versions.len() {
            return Err(ReviewError::Damaged(format!("no version at {index}")));
        }
        let mut chain = vec![index];
        let mut at = index;
        while let Some(&parent) = self.versions[at].meta.parents.first() {
            at = self
                .index_of(parent)
                .ok_or_else(|| ReviewError::Damaged(format!("the parent {parent} is gone")))?;
            if chain.len() > self.versions.len() {
                return Err(ReviewError::Damaged("the parents make a cycle".into()));
            }
            chain.push(at);
        }
        if at != 0 {
            return Err(ReviewError::Damaged(
                "a chain does not reach the root".into(),
            ));
        }
        let mut state = State::new();
        state.set_blobs(self.blobs.clone());
        state.apply_all(&self.base)?;
        for &i in chain.iter().rev().skip(1) {
            state.apply_all(&self.versions[i].delta)?;
        }
        Ok(state)
    }

    /// The state of the version with this ID.
    pub fn state_of(&self, id: u64) -> Result<State, ReviewError> {
        let i = self.index_of(id).ok_or(ReviewError::NotFound(id))?;
        self.state_at(i)
    }

    /// The index of the head, or of the newest version if the head is not set.
    fn head_index(&self) -> Option<usize> {
        self.head
            .and_then(|h| self.index_of(h))
            .or(self.versions.len().checked_sub(1))
    }

    /// The state of the head version, if there is one.
    pub fn tip_state(&self) -> Result<Option<State>, ReviewError> {
        match self.head_index() {
            None => Ok(None),
            Some(i) => self.state_at(i).map(Some),
        }
    }

    /// The state of a document, with stacking keys that fit the newest version (see
    /// [`State::of`]).
    pub fn state_of_document(&self, doc: &Document) -> Result<State, ReviewError> {
        Ok(State::of(doc, self.tip_state()?.as_ref()))
    }

    /// True if the document has the content of the newest version.
    pub fn document_is_tip(&self, doc: &Document) -> Result<bool, ReviewError> {
        Ok(match self.tip_state()? {
            None => false,
            Some(tip) => State::of(doc, Some(&tip)) == tip,
        })
    }

    /// Saves the document as the newest version. If a cap is broken, the oldest versions are
    /// folded into the base. On an error the history is unchanged.
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
        let mut next = self.clone();
        let tip = next.tip_state()?;
        let state = State::of(doc, tip.as_ref());
        let meta = VersionMeta {
            id: next.next_id,
            name: name.to_string(),
            note: new.note.filter(|n| !n.trim().is_empty()),
            author: new.author,
            time_ms: new.time_ms,
            origin: new.origin,
            parents: Vec::new(),
            branch: main_branch(),
            base_revision: new.base_revision,
            auto: new.auto,
        };
        let id = meta.id;
        let mut meta = meta;
        meta.parents = next
            .head_index()
            .map(|i| next.versions[i].meta.id)
            .into_iter()
            .collect();
        meta.branch = next.branch.clone();
        next.head = Some(id);
        next.next_id = next.next_id.saturating_add(1);
        let delta = match &tip {
            None => {
                next.base = state.snapshot();
                next.base_bytes = ops_bytes(&next.base);
                let base = next.base.clone();
                next.take_blobs(&base, &state);
                Vec::new()
            }
            Some(tip) => {
                let delta = tip.diff(&state);
                next.take_blobs(&delta, &state);
                delta
            }
        };
        let bytes = ops_bytes(&delta) + meta_bytes(&meta);
        next.versions.push(Version { meta, delta, bytes });
        let dropped = next.enforce(limits)?;
        *self = next;
        Ok(Added { id, dropped })
    }

    /// Folds the oldest versions into the base until the caps hold. Returns the IDs that went. It
    /// keeps at least one version; if that one alone is above the byte cap, the call fails.
    ///
    /// The versions that go are folded in one pass: the count is chosen first (from the cap on the
    /// count and from the bytes that the deltas of the oldest versions hold), then the file is
    /// replayed once up to the first version that stays, and the snapshot is made once. A loop
    /// is needed only when the estimate was too low, because a delta that adds things does not
    /// free its bytes when it joins the base.
    pub fn enforce(&mut self, limits: Limits) -> Result<Vec<u64>, ReviewError> {
        let limits = limits.clamped();
        let mut dropped = Vec::new();
        loop {
            let n = self.versions.len();
            if n <= 1 {
                break;
            }
            let bytes = self.bytes();
            let mut k = n.saturating_sub(limits.max_versions.max(1));
            let mut freed = 0usize;
            let mut j = 0;
            while j + 1 < n && bytes.saturating_sub(freed) > limits.max_bytes {
                freed += self.versions[j].bytes;
                j += 1;
            }
            k = k.max(j).min(n - 1);
            if k == 0 {
                break;
            }
            dropped.extend(self.versions[..k].iter().map(|v| v.meta.id));
            self.drop_oldest(k)?;
        }
        if self.bytes() > limits.max_bytes {
            return Err(ReviewError::TooLarge {
                needed: self.bytes(),
                max: limits.max_bytes,
            });
        }
        Ok(dropped)
    }

    /// Removes the `k` oldest versions: the base becomes the state of the version after them.
    /// A version that was made from a removed one, other than that version, is cut loose with a
    /// delta against it.
    fn drop_oldest(&mut self, k: usize) -> Result<(), ReviewError> {
        if k >= self.versions.len() {
            self.versions.clear();
            self.base.clear();
            self.base_bytes = 0;
            self.head = None;
            self.blobs.clear();
            return Ok(());
        }
        let gone: Vec<u64> = self.versions[..k].iter().map(|v| v.meta.id).collect();
        let new_root = self.versions[k].meta.id;
        let root_state = self.state_at(k)?;
        let cut_loose: Vec<(u64, State)> = (k + 1..self.versions.len())
            .filter(|&i| {
                self.versions[i]
                    .meta
                    .parents
                    .first()
                    .is_some_and(|p| gone.contains(p))
            })
            .map(|i| Ok((self.versions[i].meta.id, self.state_at(i)?)))
            .collect::<Result<_, ReviewError>>()?;
        self.base = root_state.snapshot();
        self.base_bytes = ops_bytes(&self.base);
        self.versions.drain(..k);
        for (id, state) in cut_loose {
            if let Some(i) = self.index_of(id) {
                let delta = root_state.diff(&state);
                self.take_blobs(&delta, &state);
                let v = &mut self.versions[i];
                v.meta.parents[0] = new_root;
                v.bytes = ops_bytes(&delta) + meta_bytes(&v.meta);
                v.delta = delta;
            }
        }
        for v in &mut self.versions {
            v.meta.parents.retain(|p| !gone.contains(p));
        }
        let first = &mut self.versions[0];
        first.meta.parents.clear();
        first.delta.clear();
        first.bytes = meta_bytes(&first.meta);
        if self.head.is_some_and(|h| gone.contains(&h)) {
            self.head = self.versions.last().map(|v| v.meta.id);
        }
        self.collect_blobs();
        Ok(())
    }

    /// Deletes a version. The other versions keep their content: a version made from it is
    /// made from its first parent instead, with a delta against that. Returns true if no version
    /// is left.
    pub fn remove(&mut self, id: u64) -> Result<bool, ReviewError> {
        let i = self.index_of(id).ok_or(ReviewError::NotFound(id))?;
        let mut next = self.clone();
        if i == 0 {
            next.drop_oldest(1)?;
        } else {
            let parent = next.versions[i]
                .meta
                .parents
                .first()
                .copied()
                .ok_or_else(|| {
                    ReviewError::Damaged(format!("version {id} has no parent but is not the root"))
                })?;
            let parent_state = next.state_of(parent)?;
            let children: Vec<(u64, State)> = (0..next.versions.len())
                .filter(|&c| next.versions[c].meta.parents.first() == Some(&id))
                .map(|c| Ok((next.versions[c].meta.id, next.state_at(c)?)))
                .collect::<Result<_, ReviewError>>()?;
            next.versions.remove(i);
            for (cid, state) in children {
                if let Some(c) = next.index_of(cid) {
                    let delta = parent_state.diff(&state);
                    next.take_blobs(&delta, &state);
                    let v = &mut next.versions[c];
                    v.meta.parents[0] = parent;
                    v.bytes = ops_bytes(&delta) + meta_bytes(&v.meta);
                    v.delta = delta;
                }
            }
            for v in &mut next.versions {
                if v.meta.parents.contains(&id) {
                    let mut seen = Vec::new();
                    for p in v
                        .meta
                        .parents
                        .iter()
                        .map(|p| if *p == id { parent } else { *p })
                    {
                        if !seen.contains(&p) {
                            seen.push(p);
                        }
                    }
                    v.meta.parents = seen;
                }
            }
            if next.head == Some(id) {
                next.head = Some(parent);
            }
            next.collect_blobs();
        }
        *self = next;
        Ok(self.versions.is_empty())
    }

    /// The JSON form (see the module text).
    pub fn to_value(&self) -> Value {
        let versions: Vec<Value> = self
            .versions
            .iter()
            .map(|v| {
                let mut m = serde_json::to_value(&v.meta).unwrap_or(Value::Null);
                if !v.delta.is_empty() {
                    m["delta"] = json!(v.delta);
                }
                m
            })
            .collect();
        let blobs: serde_json::Map<String, Value> = self
            .blobs
            .iter()
            .map(|(h, t)| (h.clone(), Value::String(t.to_string())))
            .collect();
        json!({
            "format": FORMAT,
            "next_id": self.next_id,
            "branch": self.branch,
            "head": self.head,
            "base": self.base,
            "blobs": blobs,
            "versions": versions,
        })
    }

    /// Reads the JSON form of a file or a peer. Never fails and never panics: it keeps what fits
    /// and says what it repaired or dropped (see the module text).
    pub fn load(value: &Value, limits: Limits) -> Loaded {
        let mut notes = Vec::new();
        let history = read(value, &mut notes, limits.clamped()).and_then(|mut h| {
            match h.enforce(limits) {
                Ok(dropped) if !dropped.is_empty() => notes.push(format!(
                    "the history had more than the limits allow: the {} oldest versions were folded into the base",
                    dropped.len()
                )),
                Ok(_) => {}
                Err(e) => {
                    notes.push(format!("the history was dropped: {e}"));
                    return None;
                }
            }
            Some(h)
        });
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

fn read(value: &Value, notes: &mut Vec<String>, limits: Limits) -> Option<History> {
    let drop_all = |notes: &mut Vec<String>, why: String| {
        notes.push(format!("the history of versions was dropped: {why}"));
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
    let blobs = read_blobs(obj.get("blobs"), &mut total, notes);
    let base = match read_ops(obj.get("base"), &mut total, "the base") {
        Ok(b) => b,
        Err(e) => return drop_all(notes, e),
    };
    // The base can never be made smaller. If it is above the cap, nothing can be kept, and the
    // versions are not replayed at all.
    let mut base_pictures = BTreeSet::new();
    for op in &base {
        op_refs(op, &mut |h| {
            base_pictures.insert(h.to_string());
        });
    }
    let base_size = ops_bytes(&base)
        + base_pictures
            .iter()
            .filter_map(|h| blobs.get(h))
            .map(|t| t.len())
            .sum::<usize>();
    if base_size > limits.max_bytes {
        return drop_all(
            notes,
            format!(
                "the base needs {base_size} bytes and the limit is {}",
                limits.max_bytes
            ),
        );
    }
    if let Some(h) = missing_blob(&base, &blobs) {
        return drop_all(
            notes,
            format!("the base needs the picture {h}, which is missing"),
        );
    }
    let mut state = State::new();
    if let Err(e) = state.apply_all(&base) {
        return drop_all(notes, format!("the base does not fit: {e}"));
    }
    let list = match obj.get("versions") {
        Some(Value::Array(a)) => a.as_slice(),
        Some(_) => return drop_all(notes, "the versions are not a list".into()),
        None => &[],
    };
    let mut history = History {
        next_id: obj
            .get("next_id")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1),
        base_bytes: ops_bytes(&base),
        base,
        versions: Vec::new(),
        branch: main_branch(),
        head: obj.get("head").and_then(Value::as_u64),
        blobs,
    };
    if let Some(b) = obj.get("branch").and_then(Value::as_str) {
        let b = cut(b.trim(), BRANCH_MAX);
        if !b.is_empty() {
            history.branch = b;
        }
    }
    if list.len() > HARD_MAX_VERSIONS {
        notes.push(format!(
            "the history had {} versions: only the first {HARD_MAX_VERSIONS} were read",
            list.len()
        ));
    }
    let mut repaired_texts = false;
    for (i, item) in list.iter().take(HARD_MAX_VERSIONS).enumerate() {
        let parsed = serde_json::from_value::<VersionMeta>(item.clone())
            .map_err(|e| format!("its description cannot be read: {e}"))
            .and_then(|meta| {
                let delta = if i == 0 {
                    Vec::new()
                } else {
                    read_ops(item.get("delta"), &mut total, "a delta")?
                };
                Ok((meta, delta))
            })
            .and_then(|(meta, delta)| {
                if let Some(h) = missing_blob(&delta, &history.blobs) {
                    return Err(format!("it needs the picture {h}, which is missing"));
                }
                state
                    .apply_all(&delta)
                    .map_err(|e| format!("its changes do not fit: {e}"))?;
                Ok((meta, delta))
            });
        let (mut meta, delta) = match parsed {
            Ok(p) => p,
            Err(why) => {
                notes.push(format!(
                    "version {} of the history and the {} after it were dropped: {why}",
                    i + 1,
                    list.len().min(HARD_MAX_VERSIONS) - i - 1
                ));
                break;
            }
        };
        let (name, note, author) = (meta.name.clone(), meta.note.clone(), meta.author.clone());
        meta.name = cut(name.trim(), NAME_MAX);
        if meta.name.is_empty() {
            meta.name = format!("Version {}", i + 1);
            repaired_texts = true;
        }
        meta.note = note
            .clone()
            .map(|n| cut(&n, NOTE_MAX))
            .filter(|n| !n.trim().is_empty());
        meta.author = cut(&author, AUTHOR_MAX);
        repaired_texts |= meta.name != name || meta.note != note || meta.author != author;
        if history.versions.iter().any(|v| v.meta.id == meta.id) {
            meta.id = history.next_id;
            repaired_texts = true;
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
        repaired_texts |=
            old_parents.first().copied() != before || old_parents.len() != meta.parents.len();
        let branch = cut(meta.branch.trim(), BRANCH_MAX);
        meta.branch = if branch.is_empty() {
            main_branch()
        } else {
            branch
        };
        let bytes = ops_bytes(&delta) + meta_bytes(&meta);
        history.versions.push(Version { meta, delta, bytes });
    }
    if history
        .head
        .is_none_or(|h| history.versions.iter().all(|v| v.meta.id != h))
    {
        history.head = history.versions.last().map(|v| v.meta.id);
    }
    history.collect_blobs();
    if repaired_texts {
        notes.push("some names, notes, numbers or parents of versions were repaired".into());
    }
    Some(history)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod work_tests;
