//! Agent API for review (REV-01, REV-02, and the `version.*` part of REV-07): named versions of
//! the document and the diff between versions.
//!
//! Methods: `version.create`, `version.list`, `version.get`, `version.restore`, `version.delete`,
//! `version.diff`.
//!
//! The logic is in `kadakas-review`. This module reads and writes the history in the document
//! (`Document::history`, opaque JSON for the core) through the editor, so every change is one undo
//! step and one change set (`document.history`). A parsed history is cached by the identity of
//! its JSON, so a read does not parse the file again.

use std::sync::Arc;

use serde_json::{Map, Value, json};

use kadakas_core::history::HistoryData;
use kadakas_core::model::Document;
use kadakas_review::diff::{DEFAULT_LIMIT, DiffOptions, Group, MAX_LIMIT};
use kadakas_review::history::{AUTHOR_MAX, NAME_MAX, NewVersion};
use kadakas_review::restore::{self, without_stacking_keys};
use kadakas_review::{History, Limits, ReviewError, State};

use crate::errors::bad;
use crate::{RpcError, Session, SessionLimits};

/// The largest document that `version.get` returns, in bytes of the estimated JSON.
pub const GET_DOCUMENT_MAX_BYTES: usize = 8 * 1024 * 1024;

pub const VERSION_CREATE: (&str, &str) = (
    "version.create",
    "Save the document as a named version. One undo step (undo removes the version again). params: {name, note?, author?, now_ms?}. The version keeps the author (default: the collaboration name, else the origin: user, agent or script), the time, the origin and the editor revision. The oldest versions are folded into the base when the count or the size passes the session limits (max_versions, max_version_bytes). Returns {version, dropped: [ids of versions the limits pushed out], count, bytes}.",
);
pub const VERSION_LIST: (&str, &str) = (
    "version.list",
    "The versions, oldest first. params: {current? (false: leave out current_is_newest, which costs a pass over the drawing; default true)}. Returns {notes? (what the check of a history that came from a file repaired), versions: [{id, name, note?, author, time_ms, origin, parents (the IDs of the versions it was made from, first parent first), branch, base_revision, auto?, delta_bytes}], branch (the current branch, default main), head (the version that the next one is made from), count, bytes, limits: {max_versions, max_bytes}, current_is_newest (true: the document has the content of the newest version, null: there is no version)}.",
);
pub const VERSION_GET: (&str, &str) = (
    "version.get",
    "One version. params: {id, document? (true: also the document as it was, read only; at most 8 MiB, else use version.diff)}. Returns {version, things (how many objects, layers, classes, resources there were), document?, notes?}. The document is JSON of the file format without its history.",
);
pub const VERSION_RESTORE: (&str, &str) = (
    "version.restore",
    "Make the document equal to a version. The current state is saved first as an automatic version (not if it equals the newest version), then the content is replaced in one undo step. Undo takes back the restore and also removes the automatic version that this call made; the versions you had stay. params: {id, now_ms?}. The active layer and class stay if they exist. Returns {restored, saved_version: {id, new} or null, dropped, changed, notes (things of the version that did not fit and were left out)}.",
);
pub const VERSION_DELETE: (&str, &str) = (
    "version.delete",
    "Delete a version. One undo step. The other versions keep their content. params: {id}. Returns {deleted, count}.",
);
pub const VERSION_DIFF: (&str, &str) = (
    "version.diff",
    "Compare two versions, or a version and the current document. params: {from, to (a version ID or \"current\"), kinds? (objects, layers, classes, resources, document; default all), limit? (most things listed, default 100, at most 1000)}. Returns {from, to, counts: {<kind>: {added, removed, changed}}, totals, truncated (more things than listed), exact (false: the work limit stopped the diff and the counts are partial), objects|layers|classes|resources|document: {added: [{id, ...}], removed: [{id, ...}], changed: [{id, ..., properties: [{path, old?, new?}], properties_truncated?}]}}. Property paths are those of collaboration (shape, attrs.pen, data.<key>, drawing.project.name, order = stacking). A value longer than 160 characters is {cut, len, hash}.",
);

pub fn describe() -> Value {
    json!({
        "model": "A version is a named state of the whole document: objects, layers, classes, resources, drawing data and georeference. The file keeps the first version as a full snapshot and each later one as a list of property changes (the property model of collaboration), so many small edits cost little. The caps on the count and the size drop the oldest versions first and fold them into the snapshot.",
        "local": "Versions belong to the file. Collaboration does not share them: joining a shared document keeps your versions, and the other users have their own. Shared versions are planned (REV-09). The active layer and class, the order of classes and resources, and the history itself are not part of a version.",
        "events": "version.create, version.delete and version.restore make an events.change with document.history = true (restore also lists the objects, layers, classes and resources it changed, with op \"restore\").",
        "examples": [
            {"method": "version.create", "params": {"name": "Concept approved", "note": "Before the client meeting"}},
            {"method": "version.list", "params": {}},
            {"method": "version.diff", "params": {"from": 1, "to": "current", "kinds": ["objects"], "limit": 20}},
            {"method": "version.get", "params": {"id": 1, "document": false}},
            {"method": "version.restore", "params": {"id": 1}},
            {"method": "version.delete", "params": {"id": 2}}
        ],
    })
}

/// The parsed history of a session and the state of its head version, by the identity of the
/// JSON (`HistoryData::serial`: a number, not a comparison of tens of megabytes).
#[derive(Default)]
pub struct ReviewCache {
    cached: Option<(u64, Arc<History>)>,
    /// The state of the head version, for the serial of the history it was made from.
    tip: Option<(u64, Option<Arc<State>>)>,
    /// What the last parse of the JSON repaired (a history from a file that was not checked yet).
    notes: Vec<String>,
    /// How often the JSON was parsed, and how often the head state was rebuilt by replay. Tests
    /// count them.
    pub(crate) parses: u32,
    pub(crate) replays: u32,
}

impl ReviewCache {
    fn get(&self, data: &HistoryData) -> Option<Arc<History>> {
        self.cached
            .as_ref()
            .filter(|(serial, _)| *serial == data.serial())
            .map(|(_, h)| h.clone())
    }

    /// Keeps a history that is the parsed form of this JSON.
    fn keep(&mut self, data: &HistoryData, h: Arc<History>) {
        self.cached = Some((data.serial(), h));
        self.tip = None;
    }
}

impl SessionLimits {
    /// The caps of the history.
    pub fn version_limits(&self) -> Limits {
        Limits {
            max_versions: self.max_versions,
            max_bytes: usize::try_from(self.max_version_bytes).unwrap_or(usize::MAX),
        }
    }
}

/// Checks the history of a document that comes from a file or a peer, and repairs or drops what
/// does not fit. Each repair is a load note of the document.
pub fn vet_document(doc: &mut Document, limits: &SessionLimits) {
    let Some(data) = doc.history.clone() else {
        return;
    };
    let loaded = History::load(data.value(), limits.version_limits());
    if loaded.notes.is_empty() {
        return;
    }
    doc.history = loaded.history.map(|h| HistoryData::new(h.to_value()));
    for n in loaded.notes {
        doc.add_load_note(n);
    }
}

fn limit_error(e: ReviewError) -> RpcError {
    match e {
        ReviewError::NotFound(_) => RpcError::not_found(e),
        ReviewError::TooLarge { .. } | ReviewError::Disabled => RpcError::limit(e),
        ReviewError::Invalid(_) => RpcError::validation(e),
        ReviewError::Damaged(_) => RpcError::app(e),
    }
}

/// The history of the open document (an empty one if there is none).
fn history(s: &mut Session) -> Result<Arc<History>, RpcError> {
    let Some(data) = s.editor.doc().history.clone() else {
        return Ok(Arc::new(History::new()));
    };
    if let Some(h) = s.review.get(&data) {
        return Ok(h);
    }
    let loaded = History::load(data.value(), s.limits.version_limits());
    s.review.parses += 1;
    // A history that was not checked at load (the lazy path of a host that put the document in
    // the editor itself) keeps what the check repaired: `version.list` reports it.
    s.review.notes = loaded.notes;
    let h = Arc::new(loaded.history.unwrap_or_default());
    s.review.keep(&data, h.clone());
    Ok(h)
}

/// The state of the head version, from the cache if the history is the same. One replay of the
/// history for each new history, however often the window or an agent asks.
fn tip(s: &mut Session, h: &History) -> Result<Option<Arc<State>>, RpcError> {
    let serial = s
        .editor
        .doc()
        .history
        .as_ref()
        .map_or(0, HistoryData::serial);
    if let Some((cached, state)) = &s.review.tip
        && *cached == serial
    {
        return Ok(state.clone());
    }
    let state = h.tip_state().map_err(limit_error)?.map(Arc::new);
    s.review.replays += 1;
    s.review.tip = Some((serial, state.clone()));
    Ok(state)
}

/// Writes a history into the document as one undo step, and keeps its parsed form.
fn store(s: &mut Session, h: &History) {
    s.review.notes.clear();
    if h.is_empty() {
        s.editor.set_history(None);
        s.review.cached = None;
        s.review.tip = None;
        return;
    }
    s.editor.set_history(Some(HistoryData::new(h.to_value())));
    if let Some(data) = s.editor.doc().history.clone() {
        s.review.keep(&data, Arc::new(h.clone()));
    }
}

fn now_ms(m: &Map<String, Value>) -> Result<u64, RpcError> {
    match m.get("now_ms") {
        Some(v) => v
            .as_u64()
            .ok_or_else(|| bad("now_ms must be a whole number of milliseconds")),
        None => Ok(default_now()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn default_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A browser build has no clock in `std`: the time is 0 (not known) unless the host gives it.
#[cfg(target_arch = "wasm32")]
fn default_now() -> u64 {
    0
}

fn text(m: &Map<String, Value>, key: &str) -> Result<Option<String>, RpcError> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) => Ok(Some(t.clone())),
        Some(_) => Err(bad(format!("{key} must be text"))),
    }
}

fn version_id(m: &Map<String, Value>, key: &str) -> Result<u64, RpcError> {
    m.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| bad(format!("give {key}: the number of a version")))
}

fn origin_name(o: kadakas_core::change::Origin) -> &'static str {
    use kadakas_core::change::Origin;
    match o {
        Origin::User => "user",
        Origin::Agent => "agent",
        Origin::Script => "script",
        Origin::Remote => "remote",
    }
}

fn author(s: &Session, m: &Map<String, Value>) -> Result<String, RpcError> {
    if let Some(a) = text(m, "author")? {
        return Ok(a.trim().to_string());
    }
    let name = crate::collab_api::display_name(s).filter(|n| !n.trim().is_empty());
    let name = name.unwrap_or_else(|| origin_name(s.editor.origin()).to_string());
    Ok(name.chars().take(AUTHOR_MAX).collect())
}

fn meta_json(h: &History, id: u64) -> Value {
    let mut v = h
        .meta(id)
        .and_then(|m| serde_json::to_value(m).ok())
        .unwrap_or(Value::Null);
    if let Some(b) = h.delta_bytes(id) {
        v["delta_bytes"] = json!(b);
    }
    v
}

pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !matches!(
        method,
        "version.create"
            | "version.list"
            | "version.get"
            | "version.restore"
            | "version.delete"
            | "version.diff"
    ) {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    let Some(m) = p.as_object() else {
        return Some(Err(bad("params must be an object")));
    };
    Some(match method {
        "version.create" => create(s, m),
        "version.list" => list(s, m),
        "version.get" => get(s, m),
        "version.restore" => restore_version(s, m),
        "version.delete" => delete(s, m),
        _ => diff(s, m),
    })
}

fn create(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let name = text(m, "name")?.ok_or_else(|| bad("give name: the name of the version"))?;
    let new = NewVersion {
        name,
        note: text(m, "note")?,
        author: author(s, m)?,
        time_ms: now_ms(m)?,
        origin: s.editor.origin(),
        base_revision: s.editor.revision(),
        auto: false,
    };
    let mut h = (*history(s)?).clone();
    let added = h
        .add(s.editor.doc(), new, s.limits.version_limits())
        .map_err(limit_error)?;
    store(s, &h);
    Ok(json!({
        "version": meta_json(&h, added.id),
        "dropped": added.dropped,
        "count": h.len(),
        "bytes": h.bytes(),
    }))
}

fn list(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let current = match m.get("current") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad("current must be true or false")),
    };
    let h = history(s)?;
    let versions: Vec<Value> = h.versions().map(|v| meta_json(&h, v.id)).collect();
    let l = s.limits.version_limits().clamped();
    let mut out = json!({
        "versions": versions,
        "count": h.len(),
        "bytes": h.bytes(),
        "limits": {"max_versions": l.max_versions, "max_bytes": l.max_bytes},
        "branch": h.branch(),
        "head": h.head(),
    });
    // The comparison of the whole document with the newest version costs a pass over the
    // document, so a caller that polls can leave it out.
    if current {
        out["current_is_newest"] = match tip(s, &h)? {
            None => Value::Null,
            Some(t) => json!(State::of(s.editor.doc(), Some(&t)) == *t),
        };
    }
    if !s.review.notes.is_empty() {
        out["notes"] = json!(s.review.notes);
    }
    Ok(out)
}

fn get(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let id = version_id(m, "id")?;
    let want_doc = match m.get("document") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad("document must be true or false")),
    };
    let h = history(s)?;
    let state = h.state_of(id).map_err(limit_error)?;
    let things = json!({
        "objects": state.count(Group::Objects),
        "layers": state.count(Group::Layers),
        "classes": state.count(Group::Classes),
        "resources": state.count(Group::Resources),
    });
    let mut out = json!({"version": meta_json(&h, id), "things": things});
    if want_doc {
        let size = state.bytes();
        if size > GET_DOCUMENT_MAX_BYTES {
            return Err(RpcError::limit(format!(
                "the version has about {size} bytes, the limit of version.get is {GET_DOCUMENT_MAX_BYTES}: use version.diff to see what differs"
            )));
        }
        let built = state.build(s.editor.doc());
        let mut doc = built.doc;
        doc.history = None;
        without_stacking_keys(&mut doc);
        out["document"] = serde_json::to_value(&doc).map_err(RpcError::app)?;
        let mut notes: Vec<String> = built
            .quarantined
            .iter()
            .map(|q| {
                format!(
                    "{:?} {} was left out: {}",
                    q.target.kind, q.target.id, q.reason
                )
            })
            .collect();
        notes.extend(built.repairs);
        if !notes.is_empty() {
            out["notes"] = json!(notes);
        }
    }
    Ok(out)
}

fn restore_version(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let id = version_id(m, "id")?;
    let h = history(s)?;
    let name = h
        .meta(id)
        .map(|v| v.name.clone())
        .ok_or_else(|| RpcError::not_found(ReviewError::NotFound(id)))?;
    let label: String = format!("Before restoring {name}")
        .chars()
        .take(NAME_MAX)
        .collect();
    let auto = NewVersion {
        name: label,
        note: None,
        author: author(s, m)?,
        time_ms: now_ms(m)?,
        origin: s.editor.origin(),
        base_revision: s.editor.revision(),
        auto: true,
    };
    let r = restore::prepare(&h, s.editor.doc(), id, auto, s.limits.version_limits())
        .map_err(limit_error)?;
    let changed = s
        .editor
        .restore_content(&r.doc, Some(HistoryData::new(r.history.to_value())))
        .map_err(RpcError::from)?;
    s.review.notes.clear();
    match s.editor.doc().history.clone() {
        Some(d) => s.review.keep(&d, Arc::new(r.history.clone())),
        None => {
            s.review.cached = None;
            s.review.tip = None;
        }
    }
    s.prune_selection();
    Ok(json!({
        "restored": id,
        "saved_version": r.saved.map(|(id, new)| json!({"id": id, "new": new})),
        "dropped": r.dropped,
        "changed": changed,
        "notes": r.notes,
    }))
}

fn delete(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let id = version_id(m, "id")?;
    let mut h = (*history(s)?).clone();
    h.remove(id).map_err(limit_error)?;
    store(s, &h);
    Ok(json!({"deleted": id, "count": h.len()}))
}

/// A side of a diff: a version ID or "current".
enum Side {
    Version(u64),
    Current,
}

fn side(m: &Map<String, Value>, key: &str) -> Result<Side, RpcError> {
    match m.get(key) {
        Some(Value::String(t)) if t.eq_ignore_ascii_case("current") => Ok(Side::Current),
        Some(Value::Number(n)) if n.as_u64().is_some() => {
            Ok(Side::Version(n.as_u64().unwrap_or(0)))
        }
        _ => Err(bad(format!(
            "{key} must be the number of a version or \"current\""
        ))),
    }
}

fn side_ref(h: &History, side: &Side) -> Value {
    match side {
        Side::Current => json!({"current": true}),
        Side::Version(id) => {
            json!({"id": id, "name": h.meta(*id).map(|m| m.name.clone())})
        }
    }
}

fn side_state(
    h: &History,
    doc: &Document,
    tip: Option<&State>,
    side: &Side,
) -> Result<State, RpcError> {
    match side {
        Side::Current => Ok(State::of(doc, tip)),
        Side::Version(id) => h.state_of(*id).map_err(limit_error),
    }
}

fn diff(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let (from, to) = (side(m, "from")?, side(m, "to")?);
    let mut kinds = Vec::new();
    match m.get("kinds") {
        None | Some(Value::Null) => {}
        Some(Value::Array(list)) => {
            for k in list {
                let g = k.as_str().and_then(Group::parse).ok_or_else(|| {
                    bad("kinds are objects, layers, classes, resources and document")
                })?;
                kinds.push(g);
            }
        }
        Some(_) => return Err(bad("kinds must be a list")),
    }
    let limit = match m.get("limit") {
        None | Some(Value::Null) => DEFAULT_LIMIT,
        Some(v) => v
            .as_u64()
            .map(|n| usize::try_from(n).unwrap_or(usize::MAX).min(MAX_LIMIT))
            .ok_or_else(|| bad("limit must be a whole number"))?,
    };
    let h = history(s)?;
    let tip_state = if matches!(from, Side::Current) || matches!(to, Side::Current) {
        tip(s, &h)?
    } else {
        None
    };
    let (a, b) = (
        side_state(&h, s.editor.doc(), tip_state.as_deref(), &from)?,
        side_state(&h, s.editor.doc(), tip_state.as_deref(), &to)?,
    );
    let d = kadakas_review::diff::diff(&a, &b, &DiffOptions { kinds, limit });
    let mut out = serde_json::to_value(&d).map_err(RpcError::app)?;
    out["from"] = side_ref(&h, &from);
    out["to"] = side_ref(&h, &to);
    Ok(out)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod cache_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod props;
