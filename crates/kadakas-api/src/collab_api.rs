//! Agent API for collaboration (PLAT-07, PLAT-08): drive a collaboration client from JSON-RPC.
//!
//! Methods: `collab.status`, `collab.connect`, `collab.disconnect`, `collab.sync`,
//! `collab.presence`, `collab.undo`, `collab.redo`.
//!
//! The session holds a [`kadakas_collab::transport::Link`]: a collaboration client and its
//! connection. The *host* decides where the session can connect: it gives the session an
//! [`Endpoint`] (`Session::set_collab_endpoint`). A native host can give a WebSocket endpoint
//! from its own crate; tests give an in-memory one. This crate has no network code for it.
//!
//! # The document
//!
//! The document of the session stays the normal document. Every call that moves messages does
//! the same three steps, in this order:
//!
//! 1. **Push.** The local changes since the last step (found with the change sets of PLAT-02) are
//!    compared with the *shadow*, the state that the document had when it last matched the
//!    client, and become transactions. A change set with many things becomes several
//!    transactions. This step comes first in every call, also in `collab.presence`, so a change
//!    that was made since the last call is never lost by the next step.
//! 2. **Pump.** Messages go to the server and come back.
//! 3. **Apply.** If other users changed something, the document is built from the client's view
//!    (see `kadakas_collab::materialize`, which never fails) and applied to the editor with
//!    `Editor::apply_remote`. The editor logs one change set with the origin `remote` that lists
//!    exactly the things that changed, so subscribers get `events.change` (not `events.reset`),
//!    and the selection, the group context and the caches of the session stay (selected objects
//!    that are gone leave the selection). The undo history of the normal `undo` is cleared,
//!    because its steps do not fit any more. While the session is connected, `undo` and `redo`
//!    refuse and point to `collab.undo`.

use kadakas_collab::client::{Client, ClientConfig, ClientEvent, ConnState};
use kadakas_collab::materialize::{Materialized, Quarantined};
use kadakas_collab::presence::{PresenceState, Viewport};
use kadakas_collab::transport::{Endpoint, Link};
use kadakas_core::model::Document;
use kadakas_core::uid::Uid;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::errors::bad;
use crate::{RpcError, Session};

pub const STATUS: (&str, &str) = (
    "collab.status",
    "State of the collaboration client. Returns {state (none | offline | connecting | syncing | online | failed), endpoint (text, or null if the host gave none), user, client (this program's writer ID), name, log_id, confirmed_seq, head_seq, pending (edits not yet confirmed by the server), peers (number of other clients), can_undo, can_redo, quarantined (things of the shared state that are not in the document, with the reason), repairs}.",
);
pub const CONNECT: (&str, &str) = (
    "collab.connect",
    "Connect to the endpoint that the host gave this session. Refused in a session with predictable IDs (sequential ids). params: {user? (UUID text; default: the user of an earlier connection, or of the saved journal, or a new one), name? (shown to others), token? (for the server), store? (a file path without extension: the offline log is saved in <store>.journal.json and <store>.checkpoint.json, and read from them; native only, the path policy applies), discard? (true: drop edits that wait for another user), now_ms?}. The shared document replaces the open one. Work that was made offline stays in the queue and goes to the server. Returns the status.",
);
pub const DISCONNECT: (&str, &str) = (
    "collab.disconnect",
    "Say goodbye and close the connection. The client keeps its state, so you can go on editing offline and connect again. Returns the status.",
);
pub const SYNC: (&str, &str) = (
    "collab.sync",
    "Send local changes, move messages, and apply changes of other users to the open document. Call it after your edits and now and then to hear of others. params: {now_ms?}. Returns {pushed (numbers of the transactions sent), problems (changes that could not be shared, as texts), received, sent, document_changed, events, quarantined, repairs, status}. events lists what happened: connected, disconnected, rejected {op_id, code, message}, server_error, failed, gap, store_failed, store_corrupt, regressed, renumbered, reconnect.",
);
pub const PRESENCE: (&str, &str) = (
    "collab.presence",
    "Set what others see of you and list the others. params: {cursor? [x, y] in mm, selection? [object ids], viewport? {center: [x, y], scale (mm per pixel), size: [w, h]}, clear? (true: show nothing), now_ms?}. Without cursor, selection, viewport and clear it only reads. Presence is not saved in the document, goes at most 20 times a second, and is forgotten after 15 seconds of silence. Returns {own, peers: [{client, user, name, cursor, selection, viewport}]}.",
);
pub const UNDO: (&str, &str) = (
    "collab.undo",
    "Undo your own last edit in the shared document. Other users' work stays: a property that another user changed after you is skipped, and so is an object that others use. Returns {done, op_id, applied, skipped: [{target, prop, reason (changed_by_other | gone | referenced)}], document_changed}.",
);
pub const REDO: (&str, &str) = (
    "collab.redo",
    "Redo what collab.undo took back. Edits of other users do not clear the redo steps; an edit of your own does. Same result as collab.undo.",
);

pub fn describe() -> Value {
    json!({
        "model": "The server orders the edits of all users. Edits are property-level: create, delete, set and unset a property of an object, layer, class or resource, by ID. The last edit in server order wins per property. A delete wins over later edits. Your own edits show at once and are replayed on the server order (rebase).",
        "endpoint": "The host gives the session an endpoint. Without one, collab.connect fails. The protocol is in docs/collaboration.md.",
        "undo": "collab.undo takes back only your own edits, as new edits. While the session is connected, the normal undo and redo refuse and point to collab.undo.",
        "presence": "Cursor, selection and viewport of each client. Not part of the document and not saved.",
        "remote_changes": "A change of another user reaches the document as one change set with the origin remote (events.change). The selection, the group context and the caches stay. The undo history of the normal undo is cleared.",
        "limits": "The z-order of objects and the order of layers are not shared. A session with predictable IDs cannot connect.",
        "examples": [
            {"method": "collab.connect", "params": {"name": "Ann"}},
            {"method": "collab.sync", "params": {}},
            {"method": "collab.presence", "params": {"cursor": [1200.0, 800.0], "selection": ["00000000-0000-0000-0000-000000000002"]}},
            {"method": "collab.undo", "params": {}}
        ],
    })
}

/// The collaboration state of a session.
#[derive(Default)]
pub struct CollabState {
    endpoint: Option<Box<dyn Endpoint>>,
    link: Option<Link>,
    /// The revision of the document that was last copied into the client.
    synced_rev: u64,
    /// What the last build of the document left out or repaired.
    quarantined: Vec<Quarantined>,
    repairs: Vec<String>,
    /// Local edits were turned into operations, so their stacking keys must be written into the
    /// document (see `stamp_keys`).
    stamp_pending: bool,
    /// Events that were read by a call that has no place for them (connect, presence, undo).
    kept_events: Vec<ClientEvent>,
}

impl Session {
    /// Gives the session the place to connect to (see `collab.connect`). The host decides where
    /// agents may connect.
    pub fn set_collab_endpoint(&mut self, endpoint: Box<dyn Endpoint>) {
        self.collab.endpoint = Some(endpoint);
    }

    /// The collaboration client, if `collab.connect` made one.
    pub fn collab_client(&self) -> Option<&Client> {
        self.collab.link.as_ref().map(|l| &l.client)
    }
}

/// True if the shared state has a thing with this ID, also a deleted one. A regeneration of a
/// plug-in object does not reuse the IDs of a generation that was undone: the ID of a deleted
/// thing stays taken for good.
pub(crate) fn knows_object(s: &Session, id: Uid) -> bool {
    use kadakas_collab::{Kind, Target};
    s.collab
        .link
        .as_ref()
        .is_some_and(|l| l.client.view().contains(Target::new(Kind::Object, id)))
}

/// True if the session is connected or connecting to a shared document. The normal `undo` and
/// `redo` refuse then.
pub(crate) fn is_connected(s: &Session) -> bool {
    s.collab.link.as_ref().is_some_and(|l| {
        matches!(
            l.client.state(),
            ConnState::Connecting | ConnState::Syncing | ConnState::Online
        )
    })
}

fn now_ms(m: &Map<String, Value>) -> Result<u64, RpcError> {
    match m.get("now_ms") {
        Some(v) => v
            .as_u64()
            .ok_or_else(|| bad("now_ms must be a whole number of milliseconds")),
        None => default_now(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn default_now() -> Result<u64, RpcError> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64))
}

#[cfg(target_arch = "wasm32")]
fn default_now() -> Result<u64, RpcError> {
    Err(bad("give now_ms: this build has no clock"))
}

pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !method.starts_with("collab.") {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    let Some(m) = p.as_object() else {
        return Some(Err(bad("params must be an object")));
    };
    Some(match method {
        "collab.status" => Ok(status(s)),
        "collab.connect" => connect(s, m),
        "collab.disconnect" => disconnect(s),
        "collab.sync" => sync(s, m),
        "collab.presence" => presence(s, m),
        "collab.undo" => undo(s, m, false),
        "collab.redo" => undo(s, m, true),
        _ => Err(RpcError::method_not_found(method)),
    })
}

fn state_name(st: ConnState) -> &'static str {
    match st {
        ConnState::Offline => "offline",
        ConnState::Connecting => "connecting",
        ConnState::Syncing => "syncing",
        ConnState::Online => "online",
        ConnState::Failed => "failed",
    }
}

fn quarantined_json(q: &[Quarantined]) -> Value {
    Value::Array(
        q.iter()
            .map(|q| json!({"kind": q.target.kind, "id": q.target.id, "reason": q.reason}))
            .collect(),
    )
}

fn status(s: &Session) -> Value {
    let endpoint = s.collab.endpoint.as_ref().map(|e| e.describe());
    match &s.collab.link {
        None => json!({"state": "none", "endpoint": endpoint}),
        Some(l) => {
            let c = &l.client;
            json!({
                "state": state_name(c.state()),
                "endpoint": endpoint,
                "user": c.user(),
                "client": c.client_id(),
                "name": c.name(),
                "log_id": c.log_id(),
                "confirmed_seq": c.confirmed_seq(),
                "head_seq": c.head_seq(),
                "pending": c.pending().len(),
                "peers": c.peers().len(),
                "can_undo": c.can_undo(),
                "can_redo": c.can_redo(),
                "quarantined": quarantined_json(&s.collab.quarantined),
                "repairs": s.collab.repairs,
            })
        }
    }
}

/// The offline log in two files.
#[cfg(not(target_arch = "wasm32"))]
struct FileStore {
    journal: std::path::PathBuf,
    checkpoint: std::path::PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileStore {
    fn path(&self, slot: kadakas_collab::store::Slot) -> &std::path::Path {
        match slot {
            kadakas_collab::store::Slot::Journal => &self.journal,
            kadakas_collab::store::Slot::Checkpoint => &self.checkpoint,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl kadakas_collab::store::OpLogStore for FileStore {
    fn save(&mut self, slot: kadakas_collab::store::Slot, text: &str) -> Result<(), String> {
        kadakas_sys::fsio::write_atomic(self.path(slot), text.as_bytes()).map_err(|e| e.to_string())
    }

    fn load(&self, slot: kadakas_collab::store::Slot) -> Option<String> {
        std::fs::read_to_string(self.path(slot)).ok()
    }
}

/// The store that `collab.connect {store}` names, if the path policy allows it.
#[cfg(not(target_arch = "wasm32"))]
fn open_store(s: &Session, base: &str) -> Result<FileStore, RpcError> {
    let one = |suffix: &str| {
        s.check_path(
            std::path::Path::new(&format!("{base}.{suffix}.json")),
            true,
            &["json"],
        )
        .map_err(RpcError::policy)
    };
    Ok(FileStore {
        journal: one("journal")?,
        checkpoint: one("checkpoint")?,
    })
}

fn connect(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let now = now_ms(m)?;
    if s.collab.endpoint.is_none() {
        return Err(RpcError::policy(
            "this session has no collaboration endpoint: the host has not set one",
        ));
    }
    if s.sequential_ids {
        return Err(RpcError::policy(
            "a session with predictable (sequential) IDs cannot join a shared document: its IDs would meet the IDs of other users",
        ));
    }
    if is_connected(s) {
        return Err(RpcError::conflict(
            "already connected: call collab.disconnect first",
        ));
    }
    let text = |key: &str| m.get(key).and_then(Value::as_str).map(str::to_string);
    #[cfg(not(target_arch = "wasm32"))]
    let store = match text("store") {
        Some(base) => Some(open_store(s, &base)?),
        None => None,
    };
    #[cfg(target_arch = "wasm32")]
    if text("store").is_some() {
        return Err(RpcError::policy("a store is not available in this build"));
    }
    let user = match text("user") {
        Some(t) => Uid::parse(&t).ok_or_else(|| bad("user must be a UUID text"))?,
        // Keep the user of an earlier connection, so the offline queue stays valid.
        None => s.collab.link.as_ref().map_or_else(
            || {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(u) = store.as_ref().and_then(journal_user) {
                    return u;
                }
                // A fresh UUIDv7 from the generator of a new document.
                Document::default().alloc_id()
            },
            |l| l.client.user(),
        ),
    };
    let reuse = s
        .collab
        .link
        .as_ref()
        .is_some_and(|l| l.client.user() == user);
    if !reuse {
        if let Some(l) = &s.collab.link
            && !l.client.pending().is_empty()
            && m.get("discard").and_then(Value::as_bool) != Some(true)
        {
            return Err(RpcError::conflict(format!(
                "{} edits wait for user {}: connect as that user, or pass discard: true to drop them",
                l.client.pending().len(),
                l.client.user()
            )));
        }
        let cfg = ClientConfig {
            user,
            client: Document::default().alloc_id(),
            name: text("name").unwrap_or_default(),
            token: text("token").unwrap_or_default(),
        };
        #[cfg(not(target_arch = "wasm32"))]
        let client = match store {
            Some(st) => Client::restore(cfg, Box::new(st)),
            None => Client::new(cfg),
        };
        #[cfg(target_arch = "wasm32")]
        let client = Client::new(cfg);
        s.collab.link = Some(Link::new(client));
    }
    let endpoint = s.collab.endpoint.as_mut().expect("checked above");
    let link = s.collab.link.as_mut().expect("made above");
    link.connect(endpoint.as_mut()).map_err(RpcError::app)?;
    // The document is not the state of the client yet: nothing of it goes out before the first
    // sync gave the client a state and a shadow.
    sync_cycle(s, now)?;
    Ok(status(s))
}

/// The user in a saved journal, if there is one.
#[cfg(not(target_arch = "wasm32"))]
fn journal_user(st: &FileStore) -> Option<Uid> {
    use kadakas_collab::store::{Journal, OpLogStore, Slot};
    let text = st.load(Slot::Journal)?;
    serde_json::from_str::<Journal>(&text).ok().map(|j| j.user)
}

fn disconnect(s: &mut Session) -> Result<Value, RpcError> {
    if let Some(l) = s.collab.link.as_mut() {
        l.disconnect();
    }
    Ok(status(s))
}

/// The edits in the document that the client does not have yet. Only a client that knows the
/// document (it has a shadow) takes them: a client that waits for its first snapshot must not turn
/// the open document into edits.
fn push_local(s: &mut Session) -> (Vec<u64>, Vec<String>) {
    let Some(link) = s.collab.link.as_mut() else {
        return (Vec::new(), Vec::new());
    };
    let rev = s.editor.revision();
    if !link.client.has_shadow() || rev == s.collab.synced_rev {
        return (Vec::new(), Vec::new());
    }
    let doc = s.editor.doc();
    let since = s.editor.changes_since(s.collab.synced_rev);
    let mut ids = Vec::new();
    let mut problems = Vec::new();
    if since.truncated {
        // The log dropped some change sets: compare everything. The comparison is with the
        // shadow, not with the view, so changes of other users are not undone.
        let made = link.client.edit_from_diff(doc);
        ids.extend(made.op_ids);
        problems.extend(made.problems);
    } else {
        for cs in &since.changes {
            let made = link.client.edit_from_change(doc, cs);
            ids.extend(made.op_ids);
            problems.extend(made.problems);
        }
    }
    // Always go on, also after a problem: the same change must not stop every later call.
    s.collab.synced_rev = rev;
    s.collab.stamp_pending |= !ids.is_empty();
    (ids, problems)
}

/// Builds the document from the client's view when it changed, and applies the difference to the
/// editor. True if the document changed.
fn apply_remote(s: &mut Session) -> bool {
    let Some(link) = s.collab.link.as_mut() else {
        return false;
    };
    let first = !link.client.has_shadow();
    if !link.client.take_changed() && !first {
        return false;
    }
    if !link.client.has_state() {
        return false;
    }
    let Materialized {
        doc,
        quarantined,
        repairs,
    } = link.client.materialize(s.editor.doc());
    s.collab.quarantined = quarantined;
    s.collab.repairs = repairs;
    // From now on the shadow is the document that the client built.
    link.client.set_shadow(&doc);
    let changed = s.editor.apply_remote(doc).is_some();
    if changed {
        s.prune_selection();
    }
    s.collab.synced_rev = s.editor.revision();
    changed
}

/// Copies the stacking keys that the client's view has into the document, silently (see
/// `Editor::stamp_order_keys`). A local edit gets its keys from the client, and the document must
/// carry them so that it matches what the others build.
fn stamp_keys(s: &mut Session) {
    use kadakas_collab::{Kind, Target};
    let Some(link) = s.collab.link.as_ref() else {
        return;
    };
    if !link.client.has_state() {
        return;
    }
    let view = link.client.view();
    let key = |kind, id: Uid| {
        view.alive(Target::new(kind, id))
            .and_then(|e| e.get("order"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let doc = s.editor.doc();
    let objects = doc
        .objects
        .keys()
        .filter_map(|id| key(Kind::Object, id.0).map(|k| (*id, k)))
        .collect();
    let layers = doc
        .layers
        .iter()
        .filter_map(|l| key(Kind::Layer, l.id.0).map(|k| (l.id, k)))
        .collect();
    s.editor.stamp_order_keys(&objects, &layers);
}

struct Moved {
    received: usize,
    sent: usize,
}

/// Moves messages until nothing moves (at most a few rounds: a real transport answers later).
fn pump(s: &mut Session, now: u64) -> Moved {
    let mut total = Moved {
        received: 0,
        sent: 0,
    };
    let Some(link) = s.collab.link.as_mut() else {
        return total;
    };
    for _ in 0..4 {
        let r = link.pump(now);
        total.received += r.received;
        total.sent += r.sent;
        if r.received == 0 && r.sent == 0 {
            break;
        }
    }
    total
}

struct Cycle {
    pushed: Vec<u64>,
    problems: Vec<String>,
    moved: Moved,
    changed: bool,
}

/// Push, pump, apply: the three steps of every call that moves messages.
fn sync_cycle(s: &mut Session, now: u64) -> Result<Cycle, RpcError> {
    let (pushed, problems) = push_local(s);
    let moved = pump(s, now);
    if std::mem::take(&mut s.collab.stamp_pending) {
        stamp_keys(s);
    }
    let changed = apply_remote(s);
    if let Some(l) = s.collab.link.as_mut() {
        let events = l.client.take_events();
        s.collab.kept_events.extend(events);
        let extra = s.collab.kept_events.len().saturating_sub(200);
        s.collab.kept_events.drain(..extra);
    }
    Ok(Cycle {
        pushed,
        problems,
        moved,
        changed,
    })
}

fn event_json(e: &ClientEvent) -> Value {
    match e {
        ClientEvent::Connected => json!({"event": "connected"}),
        ClientEvent::Disconnected => json!({"event": "disconnected"}),
        ClientEvent::Rejected {
            op_id,
            code,
            message,
        } => json!({"event": "rejected", "op_id": op_id, "code": code, "message": message}),
        ClientEvent::ServerError { code, message } => {
            json!({"event": "server_error", "code": code, "message": message})
        }
        ClientEvent::Failed { code, message } => {
            json!({"event": "failed", "code": code, "message": message})
        }
        ClientEvent::GapDetected { expected, got } => {
            json!({"event": "gap", "expected": expected, "got": got})
        }
        ClientEvent::StoreFailed(m) => json!({"event": "store_failed", "message": m}),
        ClientEvent::StoreCorrupt(m) => json!({"event": "store_corrupt", "message": m}),
        ClientEvent::ServerRegressed { resent } => json!({"event": "regressed", "resent": resent}),
        ClientEvent::Renumbered { count } => json!({"event": "renumbered", "count": count}),
        ClientEvent::Reconnect(m) => json!({"event": "reconnect", "message": m}),
    }
}

fn sync(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    if s.collab.link.is_none() {
        return Err(RpcError::conflict(
            "not connected: call collab.connect first",
        ));
    }
    let now = now_ms(m)?;
    let c = sync_cycle(s, now)?;
    let events: Vec<Value> = std::mem::take(&mut s.collab.kept_events)
        .iter()
        .map(event_json)
        .collect();
    Ok(json!({
        "pushed": c.pushed,
        "problems": c.problems,
        "received": c.moved.received,
        "sent": c.moved.sent,
        "document_changed": c.changed,
        "events": events,
        "quarantined": quarantined_json(&s.collab.quarantined),
        "repairs": s.collab.repairs,
        "status": status(s),
    }))
}

#[derive(Deserialize, Default)]
struct PresenceParams {
    cursor: Option<[f64; 2]>,
    selection: Option<Vec<Uid>>,
    viewport: Option<Viewport>,
    clear: Option<bool>,
}

fn presence_json(state: &PresenceState) -> Value {
    json!({"cursor": state.cursor, "selection": state.selection, "viewport": state.viewport})
}

fn presence(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let now = now_ms(m)?;
    let q: PresenceParams =
        serde_json::from_value(Value::Object(m.clone())).map_err(RpcError::invalid_params)?;
    let Some(link) = s.collab.link.as_mut() else {
        return Err(RpcError::conflict(
            "not connected: call collab.connect first",
        ));
    };
    let wants_set = q.clear == Some(true)
        || q.cursor.is_some()
        || q.selection.is_some()
        || q.viewport.is_some();
    if wants_set {
        let state = if q.clear == Some(true) {
            PresenceState::default()
        } else {
            PresenceState {
                cursor: q.cursor,
                selection: q.selection.unwrap_or_default(),
                viewport: q.viewport,
            }
        };
        link.client
            .set_presence(state)
            .map_err(RpcError::validation)?;
    }
    // The pump may bring changes of other users, so the three steps run here too.
    sync_cycle(s, now)?;
    let c = &s.collab.link.as_ref().expect("checked above").client;
    let peers: Vec<Value> = c
        .peers()
        .list()
        .iter()
        .map(|p| {
            let mut v = presence_json(&p.state);
            v["client"] = json!(p.client);
            v["user"] = json!(p.user);
            v["name"] = json!(p.name);
            v
        })
        .collect();
    Ok(json!({"own": presence_json(c.presence()), "peers": peers}))
}

fn undo(s: &mut Session, m: &Map<String, Value>, redo: bool) -> Result<Value, RpcError> {
    if s.collab.link.is_none() {
        return Err(RpcError::conflict(
            "not connected: call collab.connect first",
        ));
    }
    let now = now_ms(m)?;
    // Your latest edits first, so the undo step is the one you expect.
    push_local(s);
    let link = s.collab.link.as_mut().expect("checked above");
    let report = if redo {
        link.client.redo()
    } else {
        link.client.undo()
    };
    let c = sync_cycle(s, now)?;
    let mut v = serde_json::to_value(&report).map_err(RpcError::app)?;
    v["document_changed"] = json!(c.changed);
    Ok(v)
}

#[cfg(test)]
mod tests;
