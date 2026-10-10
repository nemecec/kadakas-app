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
//! The document of the session stays the normal document. `collab.sync` copies the local
//! changes (found with the change sets of PLAT-02) into the collaboration client, moves the
//! messages, and then rebuilds the document when other users changed something. A rebuilt
//! document replaces the open one (`events.reset` for subscribers, and the undo history of the
//! normal `undo` is gone: with collaboration, use `collab.undo`, which takes back only your own
//! work).

use kadakas_collab::client::{Client, ClientConfig, ClientEvent, ConnState};
use kadakas_collab::docsync;
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
    "State of the collaboration client. Returns {state (none | offline | connecting | syncing | online | failed), endpoint (text, or null if the host gave none), user, name, confirmed_seq, head_seq, pending (edits not yet confirmed by the server), peers (number of other users), can_undo, can_redo}.",
);
pub const CONNECT: (&str, &str) = (
    "collab.connect",
    "Connect to the endpoint that the host gave this session. params: {user? (UUID text; default: a new one, kept for later calls), name? (shown to others), token? (for the server), now_ms?}. The shared document replaces the open one. Work that was made offline stays in the queue and goes to the server. Returns the status.",
);
pub const DISCONNECT: (&str, &str) = (
    "collab.disconnect",
    "Say goodbye and close the connection. The client keeps its state, so you can go on editing offline and connect again. Returns the status.",
);
pub const SYNC: (&str, &str) = (
    "collab.sync",
    "Send local changes, move messages, and apply changes of other users to the open document. Call it after your edits and now and then to hear of others. params: {now_ms?}. Returns {pushed (number of the transaction sent, or null), received, sent, document_changed, events, status}. events lists what happened: connected, disconnected, rejected {op_id, code, message}, server_error, failed, gap, store_failed.",
);
pub const PRESENCE: (&str, &str) = (
    "collab.presence",
    "Set what others see of you and list the others. params: {cursor? [x, y] in mm, selection? [object ids], viewport? {center: [x, y], scale (mm per pixel), size: [w, h]}, clear? (true: show nothing), now_ms?}. Without cursor, selection, viewport and clear it only reads. Presence is not saved in the document, goes at most 20 times a second, and is forgotten after 15 seconds of silence. Returns {own, peers: [{user, name, cursor, selection, viewport}]}.",
);
pub const UNDO: (&str, &str) = (
    "collab.undo",
    "Undo your own last edit in the shared document. Other users' work stays: a property that another user changed after you is skipped. Returns {done, op_id, applied, skipped: [{target, prop, reason (changed_by_other | gone)}], document_changed}.",
);
pub const REDO: (&str, &str) = (
    "collab.redo",
    "Redo what collab.undo took back. Same result as collab.undo.",
);

pub fn describe() -> Value {
    json!({
        "model": "The server orders the edits of all users. Edits are property-level: create, delete, set and unset a property of an object, layer, class or resource, by ID. The last edit in server order wins per property. A delete wins over later edits. Your own edits show at once and are replayed on the server order (rebase).",
        "endpoint": "The host gives the session an endpoint. Without one, collab.connect fails. The protocol is in docs/collaboration.md.",
        "undo": "collab.undo takes back only your own edits, as new edits. The normal undo is not used with collaboration.",
        "presence": "Cursor, selection and viewport of each user. Not part of the document and not saved.",
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
                "name": c.name(),
                "confirmed_seq": c.confirmed_seq(),
                "head_seq": c.head_seq(),
                "pending": c.pending().len(),
                "peers": c.peers().len(),
                "can_undo": c.can_undo(),
                "can_redo": c.can_redo(),
            })
        }
    }
}

fn connect(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let now = now_ms(m)?;
    if s.collab.endpoint.is_none() {
        return Err(RpcError::policy(
            "this session has no collaboration endpoint: the host has not set one",
        ));
    }
    let busy = s.collab.link.as_ref().is_some_and(|l| {
        matches!(
            l.client.state(),
            ConnState::Connecting | ConnState::Syncing | ConnState::Online
        )
    });
    if busy {
        return Err(RpcError::conflict(
            "already connected: call collab.disconnect first",
        ));
    }
    let text = |key: &str| m.get(key).and_then(Value::as_str).map(str::to_string);
    let user = match text("user") {
        Some(t) => Uid::parse(&t).ok_or_else(|| bad("user must be a UUID text"))?,
        // Keep the user of an earlier connection, so the offline queue stays valid.
        None => s.collab.link.as_ref().map_or_else(
            // A fresh UUIDv7 from the generator of a new document.
            || Document::default().alloc_id(),
            |l| l.client.user(),
        ),
    };
    let reuse = s
        .collab
        .link
        .as_ref()
        .is_some_and(|l| l.client.user() == user);
    if !reuse {
        let cfg = ClientConfig {
            user,
            name: text("name").unwrap_or_default(),
            token: text("token").unwrap_or_default(),
        };
        s.collab.link = Some(Link::new(Client::new(cfg)));
    }
    let endpoint = s.collab.endpoint.as_mut().expect("checked above");
    let link = s.collab.link.as_mut().expect("made above");
    link.connect(endpoint.as_mut()).map_err(RpcError::app)?;
    run_sync(s, now)?;
    Ok(status(s))
}

fn disconnect(s: &mut Session) -> Result<Value, RpcError> {
    if let Some(l) = s.collab.link.as_mut() {
        l.disconnect();
    }
    Ok(status(s))
}

/// The edits in the document that the client does not have yet. Only a client with a state
/// takes them: a client that waits for its first snapshot must not turn the open document into
/// edits.
fn push_local(s: &mut Session) -> Result<Option<u64>, RpcError> {
    let Some(link) = s.collab.link.as_mut() else {
        return Ok(None);
    };
    let rev = s.editor.revision();
    if !link.client.has_state() || rev == s.collab.synced_rev {
        return Ok(None);
    }
    let doc = s.editor.doc();
    let since = s.editor.changes_since(s.collab.synced_rev);
    let mut last = None;
    if since.truncated {
        last = link
            .client
            .edit_from_diff(doc)
            .map_err(RpcError::validation)?;
    } else {
        for cs in &since.changes {
            if let Some(id) = link
                .client
                .edit_from_change(doc, cs)
                .map_err(RpcError::validation)?
            {
                last = Some(id);
            }
        }
    }
    s.collab.synced_rev = rev;
    Ok(last)
}

/// Rebuilds the open document when the client's view changed. True if it changed.
fn apply_remote(s: &mut Session) -> Result<bool, RpcError> {
    let Some(link) = s.collab.link.as_mut() else {
        return Ok(false);
    };
    if !link.client.take_changed() {
        return Ok(false);
    }
    let doc =
        docsync::materialize(link.client.view(), s.editor.doc()).map_err(RpcError::validation)?;
    if doc == *s.editor.doc() {
        return Ok(false);
    }
    s.replace_document(doc);
    s.collab.synced_rev = s.editor.revision();
    Ok(true)
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

/// Pushes local changes, moves messages and applies remote changes.
fn run_sync(s: &mut Session, now: u64) -> Result<(Option<u64>, Moved, bool), RpcError> {
    let pushed = push_local(s)?;
    let moved = pump(s, now);
    let changed = apply_remote(s)?;
    Ok((pushed, moved, changed))
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
    }
}

fn sync(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    if s.collab.link.is_none() {
        return Err(RpcError::conflict(
            "not connected: call collab.connect first",
        ));
    }
    let now = now_ms(m)?;
    let (pushed, moved, changed) = run_sync(s, now)?;
    let events: Vec<Value> = s
        .collab
        .link
        .as_mut()
        .map(|l| l.client.take_events())
        .unwrap_or_default()
        .iter()
        .map(event_json)
        .collect();
    Ok(json!({
        "pushed": pushed,
        "received": moved.received,
        "sent": moved.sent,
        "document_changed": changed,
        "events": events,
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
    pump(s, now);
    let c = &s.collab.link.as_ref().expect("checked above").client;
    let peers: Vec<Value> = c
        .peers()
        .list()
        .iter()
        .map(|p| {
            let mut v = presence_json(&p.state);
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
    push_local(s)?;
    let link = s.collab.link.as_mut().expect("checked above");
    let report = if redo {
        link.client.redo()
    } else {
        link.client.undo()
    };
    pump(s, now);
    let changed = apply_remote(s)?;
    let mut v = serde_json::to_value(&report).map_err(RpcError::app)?;
    v["document_changed"] = json!(changed);
    Ok(v)
}

#[cfg(test)]
mod tests;
