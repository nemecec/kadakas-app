//! Agent API for review (REV-01, REV-02, and the `version.*` part of REV-07): named versions of
//! the document and the diff between versions.
//!
//! Methods: `version.create`, `version.list`, `version.get`, `version.restore`, `version.delete`,
//! `version.diff`, `version.repair`.
//!
//! The logic is in `kadakas-review`. This module reads and writes the history in the document
//! (`Document::history`, opaque JSON for the core) through the editor, so every change is one undo
//! step and one change set (`document.history`, with the versions that it made or deleted in
//! `detail`). A parsed history is cached by the identity of its JSON, so a read does not parse the
//! file again.
//!
//! **No version is dropped without the consent of the person.** The limits (`max_versions`,
//! `max_version_bytes`) stop a save with an error that lists the versions. A history that could
//! not be read completely is kept as it is and is read only (`version.repair` is the explicit
//! choice to keep only the readable part). While a file is open, the history in the document is in
//! the memory form: the newest version as a change from the anchor (the drawing when the history
//! was opened or started, which the session keeps), in parts that the undo steps share. A save
//! writes the file form: the newest version as a change from the drawing (see
//! `Session::document_for_save`). A history that is kept as it is also gets the way back from the
//! drawing to the drawing it belongs to (the `base` entry), so that no version is lost when the
//! drawing is edited and saved.

use std::borrow::Cow;
use std::sync::Arc;

use serde_json::{Map, Value, json};

use kadakas_core::history::HistoryData;
use kadakas_core::model::Document;
use kadakas_review::diff::{DEFAULT_LIMIT, DiffOptions, Group, MAX_LIMIT};
use kadakas_review::history::{
    AUTHOR_MAX, HARD_MAX_VERSIONS, JSON_FACTOR, LimitInfo, NAME_MAX, NewVersion, ReadCaps,
    WRITE_MAX_BYTES, rebase_for_save, written_against,
};
use kadakas_review::restore::{self, without_stacking_keys};
use kadakas_review::{History, Limits, ReviewError, State, VersionInfo};

use crate::errors::{REASON_HISTORY_READONLY, REASON_PARTS_MISSING, REASON_VERSION_LIMIT, bad};
use crate::{RpcError, Session, SessionLimits};

/// The most versions the thresholds of a session can be set to (a file within them is never read
/// only: it is what a reader reads).
pub const VERSIONS_MAX: usize = HARD_MAX_VERSIONS;
/// A document that passes this many versions gets a one-time note in the `version.create` reply.
/// The note never blocks a save.
pub const VERSIONS_WARN: usize = 500;
/// The most bytes of history the thresholds of a session can be set to (see `VERSIONS_MAX`).
pub const VERSION_BYTES_MAX: u64 = WRITE_MAX_BYTES as u64;

/// The largest document that `version.get` returns, in bytes of the estimated JSON.
pub const GET_DOCUMENT_MAX_BYTES: usize = 8 * 1024 * 1024;

pub const VERSION_CREATE: (&str, &str) = (
    "version.create",
    "Save the document as a named version. One undo step (undo removes the version again). params: {name, note?, author?, now_ms?}. The version keeps the author (default: the collaboration name, else the origin: user, agent or script), the time, the origin and the editor revision. If the version would go over a limit (max_versions or max_version_bytes of the session), nothing is saved and nothing is deleted: the call fails with a limit error whose data has `limit` {kind: versions|bytes, max, needed}, `versions` [{id, name, bytes, time_ms, auto}] and `options`. Pass this to the person and ask what to do: which versions to delete (version.delete {ids}), or raising the limit in Preferences. Never delete versions by yourself to make room. Fails with a conflict error (reason history_read_only) if the history of the file could not be read completely. Returns {version, count, bytes, notes?, many_versions?}. The defaults are 1000 versions and 256 MiB, so a limit is rarely reached. When this save makes the count pass 500, the reply has `notes` (a text) and `many_versions` (500): tell the person once. The save is not blocked.",
);
pub const VERSION_LIST: (&str, &str) = (
    "version.list",
    "The versions, oldest first. params: {current? (false: leave out current_is_newest, which costs a pass over the drawing; default true)}. Returns {read_only (true: the history of the file could not be read completely; it is kept as it is until the person decides, see version.repair), notes? (what could not be read), unreadable_versions? (with read_only: how many versions version.repair would delete; null if not known), versions: [{id, name, note?, author, time_ms, origin, parents (the IDs of the versions it was made from, first parent first), branch, base_revision, auto?, delta_bytes}], branch (the current branch, default main), head (the version that the next one is made from), count, bytes, limits: {max_versions, max_bytes} (the thresholds at which a save is refused), current_is_newest (true: the document has the content of the newest version, null: there is no version)}.",
);
pub const VERSION_GET: (&str, &str) = (
    "version.get",
    "One version. params: {id, document? (true: also the document as it was, read only; at most 8 MiB, else use version.diff)}. Returns {version, things (how many objects, layers, classes, resources there were), document?, notes?}. The document is JSON of the file format without its history.",
);
pub const VERSION_RESTORE: (&str, &str) = (
    "version.restore",
    "Make the document equal to a version. The current state is saved first as an automatic version (not if it equals the newest version), then the content is replaced in one undo step. Undo takes back the restore and also removes the automatic version that this call made; the versions you had stay. If the automatic version would go over a limit, the call fails as version.create does (nothing changes): ask the person. params: {id, now_ms?}. The active layer and class stay if they exist. Returns {restored, saved_version: {id, new} or null, changed, notes (things of the version that did not fit and were left out)}.",
);
pub const VERSION_DELETE: (&str, &str) = (
    "version.delete",
    "Delete versions that the person chose. One undo step (undo brings them back). The other versions keep their content. params: {id} or {ids: [id]}. Nothing is deleted if one of the IDs does not exist. Returns {deleted: [{id, name, bytes, time_ms, auto}], count}; the change event names the deleted versions in `detail.deleted`.",
);
pub const VERSION_DIFF: (&str, &str) = (
    "version.diff",
    "Compare two versions, or a version and the current document. params: {from, to (a version ID or \"current\"), kinds? (objects, layers, classes, resources, document; default all), limit? (most things listed, default 100, at most 1000)}. Returns {from, to, counts: {<kind>: {added, removed, changed}}, totals, truncated (more things than listed), exact (false: the work limit stopped the diff and the counts are partial), objects|layers|classes|resources|document: {added: [{id, ...}], removed: [{id, ...}], changed: [{id, ..., properties: [{path, old?, new?}], properties_truncated?}]}}. Property paths are those of collaboration (shape, attrs.pen, data.<key>, drawing.project.name). order is the stacking position among the things that both sides have on the layer, and it is listed only for a thing that moved. A value longer than 160 characters is {cut, len, hash}.",
);
pub const VERSION_REPAIR: (&str, &str) = (
    "version.repair",
    "Keep only the part of the history that could be read. A file whose history could not be read completely (a damaged part, more than 1000 versions, a drawing that does not match the one the versions were saved with) is opened as it is, and the history is read only: no version is changed or lost until the person decides. This call is that decision and it loses the versions that could not be read: call it only when the person has said so. params: {confirm: true}. One undo step. Returns {kept, notes (what was left out)}.",
);

pub fn describe() -> Value {
    json!({
        "model": "A version is a named state of the whole document: objects, layers, classes, resources, drawing data and georeference. The file keeps the newest version as a change from the current drawing and every older version as a reverse delta from the next newer one (the property model of collaboration), so many small edits cost little and the drawing is the only full copy. Pictures are stored once, by hash.",
        "consent": "No version is deleted or merged without the person. The limits (max_versions, max_version_bytes) are thresholds: a save over one is refused with the list of versions (code 6, data.kind limit, reason version_limit), and you pass the list to the person. A history that could not be read completely is kept as it is and is read only (code 5, reason history_read_only) until the person chooses version.repair.",
        "local": "Versions belong to the file. Collaboration does not share them: joining a shared document keeps your versions, and the other users have their own. Shared versions are planned (REV-09). The compare in the render options reads the versions of the open file. The active layer and class, the order of classes and resources, and the history itself are not part of a version.",
        "events": "version.create, version.delete, version.restore and version.repair make an events.change with document.history = true and an op (version_create, version_delete, version_restore, version_repair). The change names what it did in `detail`: {created: {version}}, {deleted: [{id, name, ...}]}, {restored: {id, name}, saved_version}, {repaired: {notes}}. A restore also lists the objects, layers, classes and resources it changed.",
        "compare": {
            "what": "The visual compare (REV-03). render.png, render.svg and sheet.render take a `compare` option, so that an agent that reads images sees what changed. The picture shows the new side (`to`) with the changes since the old side (`from`): added objects are green, changed objects are orange, removed objects are red ghosts drawn from the old state (dashed and see-through), and unchanged objects are dimmed. Color is never the only sign: each status has its own line pattern (added solid and thick, changed dotted, removed dashed) and its own mark (+, ~, -), and the SVG has a legend with the counts.",
            "option": "compare: {from, to, mode?, hide_unchanged?, unchanged?, palette?, dark?, ghosts?, previous?}. from and to: a version ID or \"current\". mode: overlay (default) or side_by_side (two pictures with the same framing, no tints; sheets are always side by side). hide_unchanged: true leaves out the unchanged objects (short for unchanged hide). unchanged: show, dim (default) or hide. palette: standard (green, red, orange) or color_blind (blue, vermillion, dark yellow). dark: colors for a dark background. ghosts: false leaves out the ghosts of removed objects. previous: false leaves out the faint old shape of changed objects (plan only). render.png cannot be combined with render_style. An unknown option is an error.",
            "answer": "The answer of the render has a `compare` object: from and to (id and name, or current), mode, style, and for an overlay counts {added, removed, changed}, summary {total, ghosts_kept, ghosts_capped, ghosts_not_drawable, exact}, ghosts {drawn, hidden, not_drawable, capped} (what became of the removed objects in this picture: drawn, on a hidden layer or class, not drawable such as pictures and groups or shapes too long, or not kept because there are too many), notes (what the picture does not show completely) and legend [{status, color, pattern, symbol}]. The counts are exact whatever the size of the diff. At most 2000 ghosts are kept, and objects with a very long shape cannot be drawn as ghosts: `ghosts` and a notes entry say so. Ghosts are drawn against the old side, so removed symbols and styled plants have their own look. Use version.diff for the list of changed things with their properties.",
            "cost": "The states of the versions are kept in a small cache (at most 6 states, 192 MiB), so a series of renders that steps through the versions replays each version once. Rendering a version builds its document: it is bounded by the limits of the session.",
        },
        "examples": [
            {"method": "version.create", "params": {"name": "Concept approved", "note": "Before the client meeting"}},
            {"method": "version.list", "params": {}},
            {"method": "version.diff", "params": {"from": 1, "to": "current", "kinds": ["objects"], "limit": 20}},
            {"method": "render.png", "params": {"view": "top", "compare": {"from": 1, "to": "current"}}},
            {"method": "render.svg", "params": {"compare": {"from": 1, "to": 2, "hide_unchanged": true}}},
            {"method": "version.get", "params": {"id": 1, "document": false}},
            {"method": "version.restore", "params": {"id": 1}},
            {"method": "version.delete", "params": {"ids": [2]}}
        ],
    })
}

/// A history that still waits in the file the document was opened from (DOC-21).
pub(crate) struct Pending {
    loader: Arc<dyn kadakas_core::history::HistoryLoader>,
    /// The revision of the editor right after the open.
    rev: u64,
    /// True if the document in memory is the document in the file (the open changed nothing in
    /// it). Then, as long as the revision is `rev`, the document is the anchor of the history.
    exact: bool,
    /// Set when the last try to read the history failed: the revision and the state of the file
    /// then. A new try is made only when one of them is another (the answer would be the same,
    /// and the try reads the drawing again).
    failed: Option<String>,
}

/// What the cache holds for one history.
struct Cached {
    serial: u64,
    /// The reading of the JSON: the whole history, or what could be read.
    history: Arc<History>,
    /// What could not be read. While there are notes, the history is read only.
    notes: Vec<String>,
}

/// The parsed history of a session and the state of its newest version, by the identity of the
/// JSON (`HistoryData::serial`: a number, not a comparison of tens of megabytes).
#[derive(Default)]
pub struct ReviewCache {
    cached: Option<Cached>,
    /// The history waits in the file; it is read, checked and put into the document when a call
    /// needs it (`resolve`). While it is set, `Document::history` is `None`.
    pub(crate) pending: Option<Pending>,
    /// The state of the newest version, for the serial of the history it was made from.
    tip: Option<(u64, Option<Arc<State>>)>,
    /// The anchors of the histories that this session has had (see the module text of
    /// `kadakas_review::history`): the JSON of a history names its anchor, and the session gives
    /// the state back when it reads the JSON again (after an undo, for example). They are weak:
    /// every history value that is written against an anchor holds it (`HistoryData`
    /// keepalive), so an anchor lives as long as an undo or redo step names it, and no longer.
    anchors: Vec<(String, std::sync::Weak<State>)>,
    /// For a history that is kept as it is: the drawing that the file's history was written
    /// against (as loaded). A save writes the changes from the current drawing back to it (the
    /// `base` entry), so that the versions can be rebuilt after the drawing was edited.
    opaque_base: Option<Arc<State>>,
    /// Why a history that waits in the file could not be read: `not_found` (the file or the drive
    /// is away or another file is there; the versions come back when it is) or `damaged`.
    pub(crate) unreadable: Option<&'static str>,
    /// How often the JSON was parsed, and how often the newest state was rebuilt by replay. Tests
    /// count them.
    pub(crate) parses: u32,
    pub(crate) replays: u32,
}

impl ReviewCache {
    /// Tells the cache the revision of the editor right after the open, and whether the document
    /// is the one in the file (`Pending::exact`).
    pub(crate) fn arm(&mut self, rev: u64, exact: bool) {
        if let Some(p) = &mut self.pending {
            p.rev = rev;
            // A file with a drawing of its own for the versions (an edited document was saved with
            // them waiting): the open drawing is not that drawing.
            p.exact = exact && !p.loader.separate_anchor();
        }
    }

    /// True while the history waits in the file and the document in memory is the one in the
    /// file: a save may copy the entries of the history as they are.
    pub(crate) fn waits_unchanged(&self, revision: u64) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|p| p.exact && p.rev == revision)
    }

    /// Keeps a history that is the parsed form of this JSON.
    fn keep(&mut self, data: &HistoryData, history: Arc<History>, notes: Vec<String>) {
        self.cached = Some(Cached {
            serial: data.serial(),
            history,
            notes,
        });
        self.tip = None;
    }

    fn forget(&mut self) {
        self.cached = None;
        self.tip = None;
    }

    /// Remembers the anchor of a history (weakly, see `anchors`).
    fn remember(&mut self, pair: (String, Arc<State>)) {
        self.anchors.retain(|(_, w)| w.strong_count() > 0);
        if self.anchors.iter().all(|(d, _)| *d != pair.0) {
            self.anchors.push((pair.0, Arc::downgrade(&pair.1)));
        }
    }

    /// The anchors that are alive.
    fn live_anchors(&self) -> Vec<(String, Arc<State>)> {
        self.anchors
            .iter()
            .filter_map(|(d, w)| w.upgrade().map(|a| (d.clone(), a)))
            .collect()
    }
}

impl SessionLimits {
    /// What a file may hold for its history to be read.
    pub fn read_caps(&self) -> ReadCaps {
        ReadCaps {
            max_versions: self.history_read_versions,
            max_bytes: usize::try_from(self.history_read_bytes).unwrap_or(usize::MAX),
        }
    }

    /// The thresholds of the history.
    pub fn version_limits(&self) -> Limits {
        Limits {
            max_versions: self.max_versions,
            max_bytes: usize::try_from(self.max_version_bytes).unwrap_or(usize::MAX),
        }
    }
}

/// Checks the history of a document that comes from a file or a peer. It does not change the
/// history in the file: a history that could not be read completely stays as it is, and every
/// problem is a load note of the document (the history is then read only). A history in the file
/// form is read now, while the drawing is as it was saved, and the session keeps that drawing as
/// the anchor of the history in memory.
pub fn vet_document(doc: &mut Document, caps: ReadCaps) -> ReviewCache {
    let mut cache = ReviewCache::default();
    // A history that waits in the file is read when it is used (`resolve`).
    if doc.history.is_none()
        && let Some(loader) = doc.lazy_history.clone()
    {
        cache.pending = Some(Pending {
            loader,
            rev: 0,
            exact: false,
            failed: None,
        });
        return cache;
    }
    let Some(data) = doc.history.clone() else {
        return cache;
    };
    let vetted = vet(doc, data, caps);
    doc.history = Some(vetted.data);
    for n in vetted.notes {
        doc.add_load_note(n);
    }
    vetted.cache
}

/// The result of the check of a history (see [`vet`]).
struct Vetted {
    /// The history, with the anchor drawing that it keeps alive.
    data: HistoryData,
    cache: ReviewCache,
    /// What could not be read, and the sentence about the read only state.
    notes: Vec<String>,
}

/// Checks a history against the drawing it was written against. `doc` is that drawing.
fn vet(doc: &Document, data: HistoryData, caps: ReadCaps) -> Vetted {
    let mut cache = ReviewCache::default();
    let mut out = Vec::new();
    let (history, notes) = read_history(doc, &data, &[], caps);
    cache.parses += 1;
    let mut data = data;
    // The drawing is the anchor of the history now: the session keeps it, because the drawing
    // changes with the first edit.
    if let Some(h) = &history {
        cache.remember(h.anchor_pair());
        // The value of the file holds the anchor from now on, like every value that is made later
        // (so an undo back to it can read it, however the drawing was edited).
        let anchor = h.anchor_pair().1;
        let bytes = anchor.bytes();
        data = data.with_keepalive(anchor, bytes);
    }
    out.extend(notes.iter().cloned());
    if !notes.is_empty() {
        out.push(
            "The versions of this file are kept as they are and are read only until you choose to keep only the part that could be read."
                .into(),
        );
        // Keep the drawing that the versions belong to, so that a save can write the way back to
        // it (see `Session::document_for_save`).
        cache.opaque_base = written_against(data.top(), data.blob_list(), doc);
    }
    cache.keep(&data, Arc::new(history.unwrap_or_default()), notes);
    Vetted {
        data,
        cache,
        notes: out,
    }
}

/// Reads the history that waits in the file, checks it and puts it into the document (DOC-21). A
/// call that uses the history does this first (`read`). The history is checked against the drawing
/// as the file holds it: the document in memory if it is that drawing still, else the drawing read
/// again from the file. If the file cannot be read (it changed on disk, an entry is damaged) the
/// history stays unread, the session treats it as read only, and a save is refused until the
/// person decides (`version.repair {confirm: true}` drops it).
pub(crate) fn resolve(s: &mut Session) {
    let Some(p) = s.review.pending.take() else {
        return;
    };
    let key = format!("{}:{}", s.editor.revision(), p.loader.state_key());
    if p.failed.as_deref() == Some(key.as_str()) && s.review.cached.is_some() {
        // The same file and the same drawing as at the last try: the answer is the same.
        s.review.pending = Some(p);
        return;
    }
    let failed = |s: &mut Session, mut p: Pending, why: String, key: String| {
        // A file that is away or changed is not a file that is damaged: the first comes back.
        let away = why.contains("changed on disk")
            || why.contains("not in the file")
            || why.contains("cannot be read now")
            || why.contains("No such file")
            // The same error on Windows.
            || why.contains("cannot find the file specified")
            || why.contains("cannot find the path specified")
            || why.contains("the file is gone")
            || why.contains("not in the container");
        let note = if away {
            format!(
                "the versions of this file cannot be found now: {why}. They stay in the file, and come back when the file (or the drive) can be read again. Until then they are read only"
            )
        } else {
            format!(
                "the versions of this file could not be read: {why}. They stay in the file as they are, and are read only"
            )
        };
        s.review.unreadable = Some(if away { "not_found" } else { "damaged" });
        s.review.cached = Some(Cached {
            serial: 0,
            history: Arc::new(History::new()),
            notes: vec![note],
        });
        p.failed = Some(key);
        s.review.pending = Some(p);
    };
    // The size of the entries and of the pictures they list is known from the directory: a history
    // over the bound is not read at all (the bound is checked before the read, not after it).
    let caps = s.limits.read_caps();
    let size = p.loader.bytes();
    if size > caps.max_bytes.saturating_mul(JSON_FACTOR) {
        let why = format!(
            "the history of versions has {size} bytes, more than is read ({} bytes)",
            caps.max_bytes.saturating_mul(JSON_FACTOR)
        );
        return failed(s, p, why, key);
    }
    let data = match p.loader.load() {
        Ok(d) => d,
        Err(e) => return failed(s, p, e, key),
    };
    let anchor_doc;
    let drawing: &Document = if p.exact && s.editor.revision() == p.rev {
        s.editor.doc()
    } else {
        match p.loader.anchor() {
            Ok(d) => {
                anchor_doc = d;
                &anchor_doc
            }
            Err(e) => return failed(s, p, e, key),
        }
    };
    // The state of the drawing is made from its pictures too.
    if let Err(e) = drawing.check_blobs() {
        return failed(s, p, e.to_string(), key);
    }
    let vetted = vet(drawing, data, caps);
    s.load_notes.extend(vetted.notes.iter().cloned());
    s.editor.adopt_history(Some(vetted.data), vetted.notes);
    s.review = vetted.cache;
}

/// True while the history cannot be changed: it waits in a file that cannot be read, or it was
/// read with notes.
fn read_only(s: &Session) -> bool {
    s.review.pending.is_some() || (s.editor.doc().history.is_some() && !notes(s).is_empty())
}

/// Reads a history. Returns the history that could be read, and what could not.
fn read_history(
    doc: &Document,
    data: &HistoryData,
    anchors: &[(String, Arc<State>)],
    caps: ReadCaps,
) -> (Option<History>, Vec<String>) {
    if let Some(problem) = data.problem() {
        return (None, vec![problem]);
    }
    if data.bytes() > caps.max_bytes.saturating_mul(JSON_FACTOR) {
        return (
            None,
            vec![format!(
                "the history of versions has {} bytes, more than is read: it is kept as it is and read only",
                data.bytes()
            )],
        );
    }
    let loaded = History::load_with_caps(&data.to_value(), doc, anchors, caps);
    (loaded.history, loaded.notes)
}

pub(crate) fn limit_error(e: ReviewError) -> RpcError {
    match e {
        ReviewError::NotFound(_) => RpcError::not_found(e),
        ReviewError::Disabled => RpcError::limit(e),
        ReviewError::LimitReached(info) => limit_reached(&info),
        ReviewError::Invalid(_) => RpcError::validation(e),
        ReviewError::Damaged(_) => RpcError::app(e),
    }
}

fn version_json(v: &VersionInfo) -> Value {
    json!({"id": v.id, "name": v.name, "bytes": v.bytes, "time_ms": v.time_ms, "auto": v.auto})
}

/// The error of a save that would go over a limit. The data lists the versions and the choices.
fn limit_reached(info: &LimitInfo) -> RpcError {
    RpcError::limit(info)
        .with_reason(REASON_VERSION_LIMIT)
        .with_details(json!({
            "limit": {"kind": info.kind, "max": info.max, "needed": info.needed},
            "versions": info.versions.iter().map(version_json).collect::<Vec<_>>(),
            "options": [
                {"action": "delete", "how": "Ask the person which versions to delete, then call version.delete {ids}. Any versions can be chosen, not only the oldest."},
                {"action": "raise_limit", "how": "The person raises the limit in Preferences (most versions, most size of versions). Then save again."}
            ],
            "ask_the_person": true,
        }))
}

/// The history of the open document (an empty one if there is none) and what could not be read.
/// A history with notes is read only.
pub(crate) fn read(s: &mut Session) -> Arc<History> {
    resolve(s);
    if s.review.pending.is_some() {
        // The history waits in a file that cannot be read: nothing of it is shown.
        return s
            .review
            .cached
            .as_ref()
            .map_or_else(|| Arc::new(History::new()), |c| c.history.clone());
    }
    let Some(data) = s.editor.doc().history.clone() else {
        s.review.forget();
        return Arc::new(History::new());
    };
    if let Some(c) = &s.review.cached
        && c.serial == data.serial()
    {
        return c.history.clone();
    }
    let caps = s.limits.read_caps();
    let anchors = s.review.live_anchors();
    let (history, notes) = read_history(s.editor.doc(), &data, &anchors, caps);
    s.review.parses += 1;
    if let Some(h) = &history {
        s.review.remember(h.anchor_pair());
    }
    let h = Arc::new(history.unwrap_or_default());
    s.review.keep(&data, h.clone(), notes);
    h
}

/// What could not be read of the history of the open document. Empty: the reading is the whole
/// file and the history can be changed.
fn notes(s: &Session) -> &[String] {
    s.review.cached.as_ref().map_or(&[], |c| &c.notes)
}

/// Checks that the pictures of the document that stay in a file can be read: the file is the one
/// that was opened. The state of the document, for a version or a compare, is worked out from
/// the pictures too, and a picture that cannot be read would make its object look deleted. A
/// call that works out that state asks first (a conflict that says the file changed).
pub(crate) fn pictures_readable(s: &Session) -> Result<(), RpcError> {
    s.editor.doc().check_blobs().map_err(|e| {
        // The file changed: the pictures that only an undo step holds are read into memory now,
        // while some file still has them (the first look at the change).
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(src) = &s.file_source {
            src.rescue_if_changed(|| kadakas_file::drawing_hashes(s.editor.doc()));
        }
        let pictures = s.editor.doc().unreadable_pictures();
        if pictures.is_empty() {
            return RpcError::conflict(format!("{e}"));
        }
        missing_error(&kadakas_file::Missing {
            file: s
                .file_source
                .as_ref()
                .map_or_else(String::new, |f| f.label()),
            pictures,
            history: false,
            other: Vec::new(),
        })
    })
}

/// The error of a save that cannot find parts of the document in the file it was opened from.
/// Nothing was written. The person decides: put the file back, or save without them (the pictures
/// become small placeholders and the versions are left out): `file.save` with `allow_missing`.
/// The token (`details.consent_token`) is given by the session (`Session::issue_consent`).
pub fn missing_error(m: &kadakas_file::Missing) -> RpcError {
    let missing =
        json!({"file": m.file, "pictures": m.pictures, "history": m.history, "other": m.other});
    RpcError::conflict(m.message())
        .with_reason(REASON_PARTS_MISSING)
        .with_details(json!({
            "missing": missing,
            "options": [
                {"action": "restore", "how": "Put the file (or its .bak) back where it was, then save again."},
                {"action": "save_without", "how": "Ask the person. Then call file.save again with allow_missing set to the value of consent_token of this error (not true; the token is good for this one save of this document as it is): the missing pictures become small gray placeholders, the versions that cannot be found are left out, and the file is saved."}
            ],
            "ask_the_person": true,
        }))
}

/// The history, for a call that changes it: refused while the history is read only.
fn writable(s: &mut Session) -> Result<Arc<History>, RpcError> {
    pictures_readable(s)?;
    let h = read(s);
    if read_only(s) {
        return Err(read_only_error(notes(s)));
    }
    Ok(h)
}

fn read_only_error(notes: &[String]) -> RpcError {
    RpcError::conflict(
        "the history of versions of this file could not be read completely. It is kept as it is and is read only: no version is saved, restored or deleted until the person chooses to keep only the part that could be read (version.repair {confirm: true}), which loses the versions that could not be read",
    )
    .with_reason(REASON_HISTORY_READONLY)
    .with_details(json!({"notes": notes, "ask_the_person": true}))
}

/// The state of the newest version, from the cache if the history is the same. One replay of the
/// history for each new history, however often the window or an agent asks.
pub(crate) fn tip(s: &mut Session, h: &History) -> Result<Option<Arc<State>>, RpcError> {
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
fn store(s: &mut Session, h: &History, op: &str, detail: Value) {
    if h.is_empty() {
        s.editor.set_history_with(None, op, Some(detail));
        s.review.forget();
        return;
    }
    s.editor
        .set_history_with(Some(h.to_data()), op, Some(detail));
    if let Some(data) = s.editor.doc().history.clone() {
        s.review.remember(h.anchor_pair());
        s.review.keep(&data, Arc::new(h.clone()), Vec::new());
    }
}

/// The history of a session, ready to be written for any drawing.
enum FileHistory {
    /// A history that was read completely, and the state of its newest version.
    Readable(Arc<History>, Arc<State>),
    /// A history that is kept as it is (its JSON), and the drawing it was written against, if
    /// that is known.
    Opaque(Option<Arc<State>>, HistoryData),
}

impl FileHistory {
    /// The history as a file holds it for the drawing `doc`, and the state of `doc` (whose
    /// stacking keys the file must hold, see `State::stamp_keys`). A history that was read
    /// completely is written as the newest version against `doc`. A history that is kept as it is
    /// gets the way back from `doc` to the drawing it was written against (the `base` entry), so
    /// that no version is lost when the drawing is edited; the pictures that an older `base`
    /// brought and the new one does not need are taken out of the table. `None`: nothing to
    /// change in the history of the document.
    fn write_for(&self, doc: &Document) -> Option<(HistoryData, State)> {
        match self {
            FileHistory::Readable(h, tip) => {
                let (value, state) = h.to_disk(doc, tip);
                Some((HistoryData::new(value), state))
            }
            FileHistory::Opaque(base, data) => {
                let base = base.as_ref()?;
                let now = State::of(doc, Some(base));
                let (entry, blobs) = rebase_for_save(&now, base);
                if entry.is_none() && data.top().get("base").is_none() {
                    return None;
                }
                let old_added: Vec<String> = data
                    .top()
                    .get("base")
                    .and_then(|b| b.get("added"))
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|h| h.as_str().map(str::to_string))
                    .collect();
                let kept: std::collections::HashSet<&str> = data
                    .blob_list()
                    .iter()
                    .map(|(h, _)| h.as_str())
                    .filter(|h| !old_added.iter().any(|a| a == h))
                    .collect();
                let new_blobs: Vec<_> = blobs
                    .into_iter()
                    .filter(|(h, _)| !kept.contains(h.as_str()))
                    .collect();
                let added: Vec<&str> = new_blobs.iter().map(|(h, _)| h.as_str()).collect();
                let entry = entry.map(|mut e| {
                    e["added"] = json!(added);
                    e
                });
                Some((data.with_entry("base", entry, new_blobs, &old_added), now))
            }
        }
    }
}

impl Session {
    /// The document as a save writes it. Without versions it is the document. With versions it is
    /// a copy in which the history is in the file form: the newest version as a change from the
    /// drawing, the pictures that the drawing holds not repeated, and the stacking keys of the
    /// drawing written into its objects and layers, so that the file gives the versions back
    /// when it is read. A history that could not be read completely is written as it is.
    pub fn document_for_save(&mut self) -> Result<Cow<'_, Document>, RpcError> {
        self.document_for_save_in(false)
    }

    /// Like [`Session::document_for_save`], told whether the save is a container (DOC-20). A
    /// history that waits in the file, and a document that is still the one in the file, need no
    /// work for a container: the writer copies the entries of the history as they are, without
    /// reading them. Everything else reads the history first.
    pub fn document_for_save_in(&mut self, container: bool) -> Result<Cow<'_, Document>, RpcError> {
        if container && self.review.waits_unchanged(self.editor.revision()) {
            return Ok(Cow::Borrowed(self.editor.doc()));
        }
        pictures_readable(self)?;
        if container && self.review.pending.is_some() {
            // The versions wait in the file. A save never reads them just to write them again
            // (that would parse the drawing a second time) and never stops because they cannot be
            // read: the writer copies their entries as they are, and, if the drawing was edited,
            // the drawing of the file as their anchor (`needs_anchor`).
            return Ok(Cow::Borrowed(self.editor.doc()));
        }
        let Some(kind) = self.file_history()? else {
            return Ok(Cow::Borrowed(self.editor.doc()));
        };
        let Some((history, state)) = kind.write_for(self.editor.doc()) else {
            return Ok(Cow::Borrowed(self.editor.doc()));
        };
        let mut out = self.editor.doc().clone();
        state.stamp_keys(&mut out);
        out.history = Some(history);
        Ok(Cow::Owned(out))
    }

    /// True if a container save must write the drawing of the file as the anchor of the versions:
    /// the versions wait in the file, and the drawing was edited since the open.
    pub fn needs_anchor(&self) -> bool {
        self.review.pending.is_some() && !self.review.waits_unchanged(self.editor.revision())
    }

    /// What a save cannot find now: the pictures of the document that stay in a file that is gone
    /// or changed, and the versions that wait in a file that cannot be read.
    pub fn missing_for_save(&mut self) -> kadakas_file::Missing {
        resolve(self);
        let history = self.review.pending.is_some() && !notes(self).is_empty();
        kadakas_file::Missing {
            file: self
                .file_source
                .as_ref()
                .map_or_else(String::new, |f| f.label()),
            pictures: self.editor.doc().unreadable_pictures(),
            history,
            other: Vec::new(),
        }
    }

    /// The document for a save when the person chose to save without what cannot be found: a
    /// copy in which the missing pictures are small gray placeholders and the versions that wait in
    /// the file are left out.
    pub fn document_without_missing(&mut self, m: &kadakas_file::Missing) -> Document {
        let mut doc = self.editor.doc().clone();
        let set: std::collections::HashSet<String> = m.pictures.iter().cloned().collect();
        doc.replace_missing_pictures(&set);
        if m.history {
            doc.history = None;
            doc.lazy_history = None;
        }
        doc
    }

    /// After a save without what was missing: the session no longer refers to it. The pictures
    /// of the document become the placeholders (no undo step: the pictures were gone already),
    /// and the versions that could not be found are dropped.
    pub fn accept_missing(&mut self, m: &kadakas_file::Missing) {
        let set: std::collections::HashSet<String> = m.pictures.iter().cloned().collect();
        // One undo step. The undo brings back the handles of pictures that no file has; the next
        // save asks again.
        self.editor.replace_missing_pictures_with_undo(&set);
        if m.history {
            self.editor.adopt_history(None, Vec::new());
            self.review = ReviewCache::default();
        }
    }

    /// What a file needs of the history of this session to write it for a drawing (see
    /// `FileHistory::write_for`). `None` if the session has no history.
    fn file_history(&mut self) -> Result<Option<FileHistory>, RpcError> {
        resolve(self);
        if self.review.pending.is_some() {
            let why = notes(self).first().cloned().unwrap_or_default();
            // The plain JSON form holds the versions inside, so it cannot be written without them.
            let mut e = missing_error(&kadakas_file::Missing {
                file: self
                    .file_source
                    .as_ref()
                    .map_or_else(String::new, |f| f.label()),
                pictures: Vec::new(),
                history: true,
                other: Vec::new(),
            });
            e.message = format!(
                "{why}. The plain JSON form holds the versions and cannot be written without them. Save a container (.kad) instead, which keeps them as they are, or save without the versions if the person agrees"
            );
            return Err(e);
        }
        let Some(data) = self.editor.doc().history.clone() else {
            return Ok(None);
        };
        let h = read(self);
        if !notes(self).is_empty() || h.is_empty() {
            return Ok(Some(FileHistory::Opaque(
                self.review.opaque_base.clone(),
                data,
            )));
        }
        Ok(tip(self, &h)?.map(|tip| FileHistory::Readable(h, tip)))
    }

    /// For `document.load`: a document that has no `history` field keeps the versions of the
    /// session. They are written for the new drawing as a save would write them, and the new
    /// document reads them like a file.
    pub(crate) fn keep_versions_for(&mut self, doc: &mut Document) -> Result<(), RpcError> {
        if doc.history.is_some() {
            return Ok(());
        }
        let Some(kind) = self.file_history()? else {
            return Ok(());
        };
        match kind.write_for(doc) {
            Some((history, state)) => {
                state.stamp_keys(doc);
                doc.history = Some(history);
            }
            // A history that is kept as it is, and that has no way back to its drawing, stays
            // as it is.
            None => doc.history = self.editor.doc().history.clone(),
        }
        Ok(())
    }

    /// Puts a loaded document into the editor of this session, with its history checked (see
    /// `vet_document`), without the other changes that `replace_document` makes. For a host that
    /// starts a session with a file.
    pub fn adopt_document(&mut self, mut doc: Document) {
        let mut cache = vet_document(&mut doc, self.limits.read_caps());
        self.load_notes = doc.load_notes().to_vec();
        // An old file has no project GUID: it gets one here, and then the document is no longer
        // exactly the document of the file.
        let exact = !doc.ensure_project_guid();
        self.editor = kadakas_core::command::Editor::new(doc);
        cache.arm(self.editor.revision(), exact);
        self.review = cache;
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
pub(crate) fn default_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A browser build has no clock in `std`: the time is 0 (not known) unless the host gives it.
#[cfg(target_arch = "wasm32")]
pub(crate) fn default_now() -> u64 {
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

pub(crate) fn origin_name(o: kadakas_core::change::Origin) -> &'static str {
    use kadakas_core::change::Origin;
    match o {
        Origin::User => "user",
        Origin::Agent => "agent",
        Origin::Script => "script",
        Origin::Remote => "remote",
    }
}

pub(crate) fn author(s: &Session, m: &Map<String, Value>) -> Result<String, RpcError> {
    if let Some(a) = text(m, "author")? {
        return Ok(a.trim().to_string());
    }
    Ok(session_author(s))
}

/// Who this session is: the collaboration name in a shared session, else the name of the origin
/// (`user`, `agent`, ...). The caller cannot change it: the consent to delete the work of
/// somebody else is checked against this and not against a parameter.
pub(crate) fn session_author(s: &Session) -> String {
    let name = crate::collab_api::display_name(s).filter(|n| !n.trim().is_empty());
    let name = name
        .or_else(|| s.identity.clone())
        .unwrap_or_else(|| origin_name(s.editor.origin()).to_string());
    name.chars().take(AUTHOR_MAX).collect()
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
            | "version.repair"
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
        "version.repair" => repair(s, m),
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
    let mut h = (*writable(s)?).clone();
    let added = h
        .add(s.editor.doc(), new, s.limits.version_limits())
        .map_err(limit_error)?;
    let version = meta_json(&h, added.id);
    store(s, &h, "version_create", json!({"created": version}));
    let mut out = json!({
        "version": version,
        "count": h.len(),
        "bytes": h.bytes(),
    });
    if h.len() == VERSIONS_WARN + 1 {
        out["notes"] = json!([format!(
            "This document has more than {VERSIONS_WARN} versions. They stay; the person can delete old ones with version.delete (File > Versions)."
        )]);
        out["many_versions"] = json!(VERSIONS_WARN);
    }
    Ok(out)
}

/// What [`execute_between_versions`] did.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct Between {
    /// The version saved before the command.
    pub before: u64,
    /// The version saved after it. None: a limit refused it (the note says so).
    pub after: Option<u64>,
    /// Things to tell the person.
    pub notes: Vec<String>,
}

/// Runs a command with a named version before it and one after it, all in ONE undo step (an update
/// from DXF, IOP-23). The version "before" is the document as it is now. The version "after" is the
/// document the command makes, which is found on a copy first. If a limit refuses the version
/// "before", nothing changes and the error is that of `version.create`. If a limit refuses the
/// version "after", the command still runs, with the version "before" only, and a note says so.
/// A failing command changes nothing.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn execute_between_versions(
    s: &mut Session,
    cmd: &kadakas_core::command::Command,
    (before, after): (&str, &str),
    op: &str,
    detail: Value,
) -> Result<Between, RpcError> {
    let empty = Map::new();
    let limits = s.limits.version_limits();
    let new_version = |s: &Session, name: &str| -> Result<NewVersion, RpcError> {
        Ok(NewVersion {
            name: name.chars().take(NAME_MAX).collect(),
            note: None,
            author: author(s, &empty)?,
            time_ms: now_ms(&empty)?,
            origin: s.editor.origin(),
            base_revision: s.editor.revision(),
            auto: false,
        })
    };
    let mut h = (*writable(s)?).clone();
    let first = h
        .add(s.editor.doc(), new_version(s, before)?, limits)
        .map_err(limit_error)?;
    // The document after the command, on a copy: the same IDs as the real run.
    let mut probe = kadakas_core::command::Editor::new(s.editor.doc().clone());
    probe.execute(cmd).map_err(RpcError::from)?;
    let mut notes = Vec::new();
    let mut second = None;
    let mut with_after = h.clone();
    match with_after.add(probe.doc(), new_version(s, after)?, limits) {
        Ok(added) => {
            second = Some(added.id);
            h = with_after;
        }
        Err(e) => notes.push(format!(
            "the version \"{after}\" was not saved: {}",
            limit_error(e).message
        )),
    }
    let detail = json!({"between": detail, "versions": {"before": first.id, "after": second}});
    s.editor
        .execute_with_history(cmd, Some(h.to_data()), op, Some(detail))
        .map_err(RpcError::from)?;
    match s.editor.doc().history.clone() {
        Some(data) => {
            s.review.remember(h.anchor_pair());
            s.review.keep(&data, Arc::new(h.clone()), Vec::new());
        }
        None => s.review.forget(),
    }
    Ok(Between {
        before: first.id,
        after: second,
        notes,
    })
}

fn list(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let current = match m.get("current") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad("current must be true or false")),
    };
    let h = read(s);
    let versions: Vec<Value> = h.versions().map(|v| meta_json(&h, v.id)).collect();
    let l = s.limits.version_limits().clamped();
    let read_only = read_only(s);
    let mut out = json!({
        "read_only": read_only,
        "versions": versions,
        "count": h.len(),
        "bytes": h.bytes(),
        "limits": {"max_versions": l.max_versions, "max_bytes": l.max_bytes},
        "branch": h.branch(),
        "head": h.head(),
    });
    if read_only {
        // How many versions of the file `version.repair` would delete.
        let in_file = s
            .editor
            .doc()
            .history
            .as_ref()
            .and_then(HistoryData::version_count);
        out["unreadable_versions"] =
            in_file.map_or(Value::Null, |n| json!(n.saturating_sub(h.len())));
    }
    // The comparison of the whole document with the newest version costs a pass over the
    // document, so a caller that polls can leave it out.
    if current {
        pictures_readable(s)?;
        out["current_is_newest"] = match tip(s, &h)? {
            None => Value::Null,
            Some(t) => json!(State::of(s.editor.doc(), Some(&t)) == *t),
        };
    }
    if read_only {
        out["notes"] = json!(notes(s));
        // `not_found`: the file or the drive is away and the versions come back; `damaged`: they
        // do not (only `version.repair` helps).
        out["unreadable_kind"] = json!(s.review.unreadable.unwrap_or("damaged"));
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
    let h = read(s);
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
    let h = writable(s)?;
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
    let saved = r.saved.map(|(id, new)| json!({"id": id, "new": new}));
    // In a shared document an object that another user deleted stays deleted (delete wins): the
    // restore brings it back in this document only until the next sync.
    let mut notes = r.notes.clone();
    let current = s.editor.doc();
    let revived = r
        .doc
        .objects
        .keys()
        .filter(|id| !current.objects.contains_key(*id))
        .copied();
    let blocked = crate::collab_api::deleted_by_others(s, revived);
    if blocked > 0 {
        notes.push(format!(
            "{blocked} objects of the version were deleted by other users of the shared document, and they stay deleted there"
        ));
    }
    let detail = json!({"restored": {"id": id, "name": name}, "saved_version": saved});
    let changed = s
        .editor
        .restore_content_with(
            &r.doc,
            Some(r.history.to_data()),
            "version_restore",
            Some(detail),
        )
        .map_err(RpcError::from)?;
    match s.editor.doc().history.clone() {
        Some(d) => {
            s.review.remember(r.history.anchor_pair());
            s.review.keep(&d, Arc::new(r.history.clone()), Vec::new());
        }
        None => s.review.forget(),
    }
    s.prune_selection();
    Ok(json!({
        "restored": id,
        "saved_version": saved,
        "changed": changed,
        "notes": notes,
    }))
}

fn delete(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let ids: Vec<u64> = match (m.get("ids"), m.get("id")) {
        (Some(Value::Array(list)), None) => list
            .iter()
            .map(|v| v.as_u64().ok_or_else(|| bad("ids are numbers of versions")))
            .collect::<Result<_, _>>()?,
        (None, Some(_)) => vec![version_id(m, "id")?],
        _ => return Err(bad("give id (a version) or ids (a list of versions)")),
    };
    if ids.is_empty() {
        return Err(bad("ids must name at least one version"));
    }
    let mut h = (*writable(s)?).clone();
    let gone = h.remove_many(&ids).map_err(limit_error)?;
    let deleted: Vec<Value> = gone.iter().map(version_json).collect();
    store(s, &h, "version_delete", json!({"deleted": deleted}));
    Ok(json!({"deleted": deleted, "count": h.len()}))
}

/// Brings a history that waits in the file into the document for `version.repair`: what the file
/// holds is read as it is and checked against the best drawing there is (the anchor of the file,
/// else the open drawing), so that the repair keeps every version that is readable and is the same
/// step as for a history that was read with notes. Returns false if the entries cannot be read at
/// all (then nothing is readable and there is nothing to keep).
fn revive_for_repair(s: &mut Session) -> bool {
    let Some(p) = s.review.pending.take() else {
        return true;
    };
    let Ok(data) = p.loader.load() else {
        s.review.pending = Some(p);
        return false;
    };
    let caps = s.limits.read_caps();
    let anchor = if p.exact && s.editor.revision() == p.rev {
        None
    } else {
        p.loader.anchor().ok()
    };
    let vetted = match &anchor {
        Some(d) => vet(d, data, caps),
        None => vet(s.editor.doc(), data, caps),
    };
    s.load_notes.extend(vetted.notes.iter().cloned());
    s.editor.adopt_history(Some(vetted.data), vetted.notes);
    s.review = vetted.cache;
    true
}

fn repair(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    if m.get("confirm") != Some(&Value::Bool(true)) {
        return Err(bad(
            "version.repair loses the versions that could not be read. Ask the person, and pass confirm: true only if they agree",
        ));
    }
    let before = if s.review.pending.is_some() {
        notes(s).to_vec()
    } else {
        Vec::new()
    };
    if s.review.pending.is_some() && !revive_for_repair(s) {
        // The entries of the file cannot be read at all: nothing is readable, so nothing is kept.
        let reasons = notes(s).to_vec();
        s.editor.adopt_history(None, Vec::new());
        s.review = ReviewCache::default();
        // There is nothing in memory to bring back, so this one is not an undo step. The file is
        // not changed until the person saves.
        return Ok(json!({"kept": 0, "notes": reasons, "undoable": false}));
    }
    let h = read(s);
    let mut reasons = notes(s).to_vec();
    if reasons.is_empty() && !before.is_empty() && s.editor.doc().history.is_some() {
        // The versions were read now (the file is readable again): nothing is lost.
        return Ok(json!({"kept": h.len(), "notes": before}));
    }
    if s.editor.doc().history.is_none() || reasons.is_empty() {
        return Err(RpcError::conflict(
            "the history of this file was read completely: there is nothing to repair",
        ));
    }
    reasons.extend(before);
    let detail = json!({"repaired": {"notes": reasons, "kept": h.len()}});
    let kept = h.len();
    if h.is_empty() {
        s.editor
            .set_history_with(None, "version_repair", Some(detail));
        s.review.forget();
    } else {
        s.editor
            .set_history_with(Some(h.to_data()), "version_repair", Some(detail));
        if let Some(data) = s.editor.doc().history.clone() {
            s.review.remember(h.anchor_pair());
            s.review.keep(&data, h.clone(), Vec::new());
        }
    }
    Ok(json!({"kept": kept, "notes": reasons}))
}

/// A side of a diff or a compare: a version ID or "current".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Version(u64),
    Current,
}

pub(crate) fn side(m: &Map<String, Value>, key: &str) -> Result<Side, RpcError> {
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

pub(crate) fn side_ref(h: &History, side: &Side) -> Value {
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
    let h = read(s);
    let tip_state = if matches!(from, Side::Current) || matches!(to, Side::Current) {
        pictures_readable(s)?;
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
mod bound_tests;

#[cfg(test)]
mod cache_tests;

#[cfg(test)]
mod opaque_tests;

#[cfg(test)]
mod memory_tests;

#[cfg(test)]
mod keep_tests;

#[cfg(test)]
mod sheet_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod props;
