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
//! # Caps
//!
//! [`Limits`] caps the number of versions and the bytes of the history. When a new version breaks
//! a cap, the oldest version goes first and is folded into the base: the base becomes the state of
//! the next version, so every version that stays keeps its state exactly. Deleting a version from
//! the middle joins its delta to the next one by comparing the states on both sides.
//!
//! # The file
//!
//! The history is JSON in the field `history` of the file (the core holds it as opaque JSON):
//!
//! ```json
//! {"format": 1, "next_id": 4,
//!  "base": [<Create and Set operations that make the first version from nothing>],
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

use kadakas_core::change::Origin;
use kadakas_core::model::Document;
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
    /// The revision of the editor when the version was saved (`Editor::revision`). It is unique in
    /// one process. Use it to find out if the open document changed since the version; it means
    /// nothing after the file is opened again.
    #[serde(default)]
    pub base_revision: u64,
    /// True for a version that Kadakas saved by itself (before a restore).
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto: bool,
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
        }
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

    /// The size of the history in bytes, estimated from its operations and texts.
    pub fn bytes(&self) -> usize {
        self.base_bytes + self.versions.iter().map(|v| v.bytes).sum::<usize>()
    }

    /// The size of the delta of a version in bytes, estimated.
    pub fn delta_bytes(&self, id: u64) -> Option<usize> {
        self.index_of(id)
            .map(|i| ops_bytes(&self.versions[i].delta))
    }

    /// The state of the version at `index` (0 is the oldest): the base with the deltas applied.
    pub fn state_at(&self, index: usize) -> Result<State, ReviewError> {
        if index >= self.versions.len() {
            return Err(ReviewError::Damaged(format!("no version at {index}")));
        }
        let mut state = State::new();
        state.apply_all(&self.base)?;
        for v in &self.versions[1..=index] {
            state.apply_all(&v.delta)?;
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
        match self.versions.len() {
            0 => Ok(None),
            n => self.state_at(n - 1).map(Some),
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
            base_revision: new.base_revision,
            auto: new.auto,
        };
        let id = meta.id;
        next.next_id = next.next_id.saturating_add(1);
        let delta = match &tip {
            None => {
                next.base = state.snapshot();
                next.base_bytes = ops_bytes(&next.base);
                Vec::new()
            }
            Some(tip) => tip.diff(&state),
        };
        let bytes = ops_bytes(&delta) + meta_bytes(&meta);
        next.versions.push(Version { meta, delta, bytes });
        let dropped = next.enforce(limits)?;
        *self = next;
        Ok(Added { id, dropped })
    }

    /// Folds the oldest versions into the base until the caps hold. Returns the IDs that went. It
    /// keeps at least one version; if that one alone is above the byte cap, the call fails.
    pub fn enforce(&mut self, limits: Limits) -> Result<Vec<u64>, ReviewError> {
        let limits = limits.clamped();
        let mut dropped = Vec::new();
        while self.versions.len() > 1
            && (self.versions.len() > limits.max_versions || self.bytes() > limits.max_bytes)
        {
            dropped.push(self.versions[0].meta.id);
            self.drop_first()?;
        }
        if self.bytes() > limits.max_bytes {
            return Err(ReviewError::TooLarge {
                needed: self.bytes(),
                max: limits.max_bytes,
            });
        }
        Ok(dropped)
    }

    /// Removes the oldest version: the base becomes the state of the next one.
    fn drop_first(&mut self) -> Result<(), ReviewError> {
        if self.versions.len() < 2 {
            self.versions.clear();
            self.base.clear();
            self.base_bytes = 0;
            return Ok(());
        }
        let next = self.state_at(1)?;
        self.base = next.snapshot();
        self.base_bytes = ops_bytes(&self.base);
        self.versions.remove(0);
        let first = &mut self.versions[0];
        first.delta.clear();
        first.bytes = meta_bytes(&first.meta);
        Ok(())
    }

    /// Deletes a version. The versions before and after stay as they are. Returns true if no
    /// version is left.
    pub fn remove(&mut self, id: u64) -> Result<bool, ReviewError> {
        let i = self.index_of(id).ok_or(ReviewError::NotFound(id))?;
        let mut next = self.clone();
        if i == 0 {
            next.drop_first()?;
        } else if i + 1 == next.versions.len() {
            next.versions.pop();
        } else {
            let before = next.state_at(i - 1)?;
            let after = next.state_at(i + 1)?;
            let delta = before.diff(&after);
            next.versions.remove(i);
            let v = &mut next.versions[i];
            v.bytes = ops_bytes(&delta) + meta_bytes(&v.meta);
            v.delta = delta;
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
        json!({
            "format": FORMAT,
            "next_id": self.next_id,
            "base": self.base,
            "versions": versions,
        })
    }

    /// Reads the JSON form of a file or a peer. Never fails and never panics: it keeps what fits
    /// and says what it repaired or dropped (see the module text).
    pub fn load(value: &Value, limits: Limits) -> Loaded {
        let mut notes = Vec::new();
        let history = read(value, &mut notes).and_then(|mut h| {
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

fn read(value: &Value, notes: &mut Vec<String>) -> Option<History> {
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
    let base = match read_ops(obj.get("base"), &mut total, "the base") {
        Ok(b) => b,
        Err(e) => return drop_all(notes, e),
    };
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
    };
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
        let bytes = ops_bytes(&delta) + meta_bytes(&meta);
        history.versions.push(Version { meta, delta, bytes });
    }
    if repaired_texts {
        notes.push("some names, notes or numbers of versions were repaired".into());
    }
    Some(history)
}

#[cfg(test)]
mod tests;
