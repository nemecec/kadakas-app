//! The change-event stream (PLAT-02). A client subscribes with `events.subscribe`. After each
//! change, the host sends a notification `{"jsonrpc": "2.0", "method": "events.change", "params":
//! ChangeSet}` (a message without an id). `events.poll` gives the same change sets on request,
//! for clients that cannot receive notifications.
//!
//! The session keeps the subscriptions. A host calls [`Session::take_notifications`] after each
//! reply and after each change that it made itself (the app does this after each frame), and
//! writes the messages to the connection.

use std::collections::BTreeMap;

use kadakas_core::change::{ChangeSet, KINDS, Origin};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::errors::RpcError;
use crate::{Session, params};

/// The connection that stdio and tests use.
pub const LOCAL: u64 = 0;

#[derive(Clone, Debug)]
struct Sub {
    kinds: Vec<String>,
    /// The latest revision that this connection has seen.
    cursor: u64,
    /// The document was replaced: the client must read it again.
    reset: bool,
}

/// The subscriptions of a session, by connection.
#[derive(Clone, Debug, Default)]
pub struct EventHub {
    subs: BTreeMap<u64, Sub>,
}

impl EventHub {
    /// Tells every subscriber that the document was replaced.
    pub(crate) fn document_replaced(&mut self) {
        for s in self.subs.values_mut() {
            s.reset = true;
        }
    }
}

#[derive(Deserialize, Default)]
struct SubscribeParams {
    kinds: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct PollParams {
    since_revision: u64,
    kinds: Option<Vec<String>>,
}

fn check_kinds(kinds: Option<Vec<String>>) -> Result<Vec<String>, RpcError> {
    let kinds = kinds.unwrap_or_else(|| KINDS.iter().map(|k| k.to_string()).collect());
    if let Some(bad) = kinds.iter().find(|k| !KINDS.contains(&k.as_str())) {
        return Err(RpcError::invalid_params(format!(
            "unknown kind '{bad}'; use {}",
            KINDS.join(", ")
        )));
    }
    Ok(kinds)
}

fn filter(changes: &[&ChangeSet], kinds: &[String]) -> Vec<ChangeSet> {
    let kinds: Vec<&str> = kinds.iter().map(String::as_str).collect();
    changes.iter().filter_map(|c| c.only(&kinds)).collect()
}

pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    Some(match method {
        "events.subscribe" => params::<SubscribeParams>(p)
            .and_then(|q| check_kinds(q.kinds))
            .map(|kinds| {
                let revision = s.editor.revision();
                let conn = s.conn;
                s.events.subs.insert(
                    conn,
                    Sub {
                        kinds: kinds.clone(),
                        cursor: revision,
                        reset: false,
                    },
                );
                json!({"revision": revision, "kinds": kinds})
            }),
        "events.unsubscribe" => {
            let conn = s.conn;
            Ok(json!({"done": s.events.subs.remove(&conn).is_some()}))
        }
        "events.poll" => params::<PollParams>(p).and_then(|q| {
            let kinds = check_kinds(q.kinds)?;
            let since = s.editor.changes_since(q.since_revision);
            Ok(json!({
                "revision": s.editor.revision(),
                "truncated": since.truncated,
                "changes": filter(&since.changes, &kinds),
            }))
        }),
        _ => return None,
    })
}

impl Session {
    /// Runs a request for a connection. `events.subscribe` and `events.unsubscribe` apply to this
    /// connection. The changes that the request makes have the given origin.
    pub fn handle_as(
        &mut self,
        conn: u64,
        origin: Origin,
        method: &str,
        p: Value,
    ) -> Result<Value, RpcError> {
        let (old_conn, old_origin) = (self.conn, self.editor.origin());
        self.conn = conn;
        self.editor.set_origin(origin);
        let result = self.handle(method, p);
        self.conn = old_conn;
        self.editor.set_origin(old_origin);
        result
    }

    /// True if this connection has subscribed.
    pub fn is_subscribed(&self, conn: u64) -> bool {
        self.events.subs.contains_key(&conn)
    }

    /// The connections with a subscription.
    pub fn subscribers(&self) -> Vec<u64> {
        self.events.subs.keys().copied().collect()
    }

    /// Forgets the subscription of a connection that closed.
    pub fn drop_connection(&mut self, conn: u64) {
        self.events.subs.remove(&conn);
    }

    /// The notifications that this connection has not received yet, oldest first, as JSON-RPC
    /// messages without an id. Each change set arrives once. A replaced document gives one
    /// `events.reset` instead of the missed changes. A client that missed changes that the
    /// bounded log dropped gets `events.reset` too.
    pub fn take_notifications(&mut self, conn: u64) -> Vec<Value> {
        // While a script runs, its steps are not announced (they may be taken back). The cursor
        // stays: at the end the run is one change, or none.
        if self.script_js.open() {
            return vec![];
        }
        let revision = self.editor.revision();
        let Some(sub) = self.events.subs.get_mut(&conn) else {
            return vec![];
        };
        let note = |method: &str, params: Value| json!({"jsonrpc": "2.0", "method": method, "params": params});
        if sub.reset {
            sub.reset = false;
            sub.cursor = revision;
            return vec![note("events.reset", json!({"revision": revision}))];
        }
        if sub.cursor == revision {
            return vec![];
        }
        let since = self.editor.changes_since(sub.cursor);
        sub.cursor = revision;
        if since.truncated {
            return vec![note("events.reset", json!({"revision": revision}))];
        }
        filter(&since.changes, &sub.kinds)
            .into_iter()
            .map(|c| note("events.change", json!(c)))
            .collect()
    }
}
