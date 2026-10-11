//! The state of the link for the status bar, the error classes, and the undo route.

use kadakas_l10n::{Arg, msg, translate};
use serde_json::Value;

/// What the status bar shows about the link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// No connection, and none is wanted.
    Offline,
    /// The first connection is being made.
    Connecting,
    Connected,
    /// The connection was lost, and the program tries again by itself.
    Reconnecting,
    /// The server refused the token. The program does not try again.
    Refused,
    /// Another fatal error. The program does not try again.
    Failed,
}

impl Link {
    pub fn message_id(self) -> &'static str {
        match self {
            Link::Offline => msg!("uic-collab-link-offline"),
            Link::Connecting => msg!("uic-collab-link-connecting"),
            Link::Connected => msg!("uic-collab-link-connected"),
            Link::Reconnecting => msg!("uic-collab-link-reconnecting"),
            Link::Refused => msg!("uic-collab-link-refused"),
            Link::Failed => msg!("uic-collab-link-failed"),
        }
    }

    pub fn text(self) -> String {
        translate(self.message_id(), &[])
    }
}

/// The class of an error that the server or the client reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The token is wrong or ran out (`auth_failed`).
    AuthRefused,
    /// The server speaks another protocol version (`unsupported_protocol`).
    Protocol,
    /// The document is at its size limit (`document_full`).
    DocumentFull,
    /// The server refused an edit for good (`bad_message`, `invalid_txn`, `duplicate_uid`).
    Rejected,
    /// The server is busy; the edit is sent again (`rate_limited`).
    RateLimited,
    /// A fault in the server; the edit is sent again (`internal`).
    ServerFault,
    /// The numbers of the edits were used before (`op_id_reused`).
    Renumber,
    /// Any other code (a newer server).
    Unknown,
}

impl ErrorKind {
    /// The class of a code of the protocol (the text of `ErrorCode`).
    pub fn from_code(code: &str) -> ErrorKind {
        match code {
            "auth_failed" => ErrorKind::AuthRefused,
            "unsupported_protocol" => ErrorKind::Protocol,
            "document_full" => ErrorKind::DocumentFull,
            "bad_message" | "invalid_txn" | "duplicate_uid" => ErrorKind::Rejected,
            "rate_limited" => ErrorKind::RateLimited,
            "internal" => ErrorKind::ServerFault,
            "op_id_reused" => ErrorKind::Renumber,
            _ => ErrorKind::Unknown,
        }
    }

    pub fn message_id(self) -> &'static str {
        match self {
            ErrorKind::AuthRefused => msg!("uic-collab-err-auth"),
            ErrorKind::Protocol => msg!("uic-collab-err-protocol"),
            ErrorKind::DocumentFull => msg!("uic-collab-err-document-full"),
            ErrorKind::Rejected => msg!("uic-collab-err-rejected"),
            ErrorKind::RateLimited => msg!("uic-collab-err-rate-limited"),
            ErrorKind::ServerFault => msg!("uic-collab-err-server-fault"),
            ErrorKind::Renumber => msg!("uic-collab-err-renumber"),
            ErrorKind::Unknown => msg!("uic-collab-err-unknown"),
        }
    }

    fn severity(self) -> Severity {
        match self {
            ErrorKind::RateLimited | ErrorKind::ServerFault | ErrorKind::Renumber => {
                Severity::Warning
            }
            _ => Severity::Error,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A message for the person, made from an event of the client.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub id: &'static str,
    pub args: Vec<(&'static str, String)>,
    pub severity: Severity,
}

impl Notice {
    fn new(id: &'static str, severity: Severity) -> Notice {
        Notice {
            id,
            args: Vec::new(),
            severity,
        }
    }

    fn arg(mut self, name: &'static str, value: &str) -> Notice {
        self.args.push((name, value.to_string()));
        self
    }

    /// The text in the language of the UI. The code and the message of the server are English
    /// texts of the API; they sit inside the localized sentence.
    pub fn text(&self) -> String {
        let args: Vec<(&str, Arg)> = self
            .args
            .iter()
            .map(|(n, v)| (*n, Arg::Text(v.clone())))
            .collect();
        translate(self.id, &args)
    }
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The notice for an event of `collab.sync`, if the person needs to hear of it. Events that
/// the status bar shows already (`disconnected`, `reconnect`, `gap`) give none.
fn notice_for(event: &Value) -> Option<Notice> {
    let code = str_of(event, "code");
    let message = str_of(event, "message");
    Some(match str_of(event, "event") {
        "connected" => Notice::new(msg!("uic-collab-note-connected"), Severity::Info),
        "failed" | "rejected" | "server_error" => {
            let kind = ErrorKind::from_code(code);
            Notice::new(kind.message_id(), kind.severity())
                .arg("code", code)
                .arg("message", message)
        }
        "regressed" => Notice::new(msg!("uic-collab-note-regressed"), Severity::Warning),
        "renumbered" => Notice::new(msg!("uic-collab-note-renumbered"), Severity::Info),
        "store_failed" => Notice::new(msg!("uic-collab-note-store-failed"), Severity::Warning)
            .arg("error", message),
        "store_corrupt" => Notice::new(msg!("uic-collab-note-store-corrupt"), Severity::Warning)
            .arg("error", message),
        _ => return None,
    })
}

/// The state of the link, read from `collab.status`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusView {
    pub link: Link,
    /// Edits that the server has not confirmed.
    pub pending: usize,
    /// Other clients.
    pub peers: usize,
    /// The first snapshot has not arrived; it will replace the document.
    pub waiting_for_snapshot: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// The API treats the session as connected: the normal undo is refused, the per-user undo
    /// works.
    pub api_connected: bool,
    pub last_error: Option<String>,
}

impl Default for StatusView {
    fn default() -> Self {
        StatusView {
            link: Link::Offline,
            pending: 0,
            peers: 0,
            waiting_for_snapshot: false,
            can_undo: false,
            can_redo: false,
            api_connected: false,
            last_error: None,
        }
    }
}

impl StatusView {
    /// The short text of the status bar item.
    pub fn label(&self) -> String {
        if self.pending > 0 && self.link != Link::Offline {
            translate(
                msg!("uic-collab-status-line-pending"),
                &[
                    ("link", Arg::Text(self.link.text())),
                    ("count", Arg::Num(self.pending as f64)),
                ],
            )
        } else {
            self.link.text()
        }
    }

    /// The longer text for a tooltip.
    pub fn tooltip(&self) -> String {
        let mut text = translate(
            msg!("uic-collab-status-people"),
            &[("count", Arg::Num(self.peers as f64))],
        );
        if let Some(e) = &self.last_error {
            text.push('\n');
            text.push_str(&translate(
                msg!("uic-collab-status-last-error"),
                &[("error", Arg::Text(e.clone()))],
            ));
        }
        text
    }

    /// True while the link is up or being made (the program tries to be connected).
    pub fn is_active(&self) -> bool {
        !matches!(self.link, Link::Offline)
    }
}

/// Reads `collab.status` and the events of `collab.sync`, and remembers what the status alone
/// cannot say: whether the link was online (a lost link is `Reconnecting`, a first try is
/// `Connecting`) and why a failed link failed.
#[derive(Clone, Debug, Default)]
pub struct Tracker {
    was_online: bool,
    failure: Option<ErrorKind>,
}

impl Tracker {
    /// Forget everything (a new connection).
    pub fn reset(&mut self) {
        *self = Tracker::default();
    }

    /// Takes a status and the events that came with it. Returns the view and the notices of the
    /// events.
    pub fn observe(&mut self, status: &Value, events: &[Value]) -> (StatusView, Vec<Notice>) {
        let mut notices = Vec::new();
        for e in events {
            if str_of(e, "event") == "failed" {
                self.failure = Some(ErrorKind::from_code(str_of(e, "code")));
            }
            notices.extend(notice_for(e));
        }
        let state = str_of(status, "state");
        let reconnecting = status["reconnecting"].as_bool() == Some(true);
        let link = match state {
            "online" => {
                self.was_online = true;
                self.failure = None;
                Link::Connected
            }
            "connecting" | "syncing" if self.was_online => Link::Reconnecting,
            "connecting" | "syncing" => Link::Connecting,
            "offline" if reconnecting && self.was_online => Link::Reconnecting,
            "offline" if reconnecting => Link::Connecting,
            "failed" if self.failure == Some(ErrorKind::AuthRefused) => Link::Refused,
            "failed" => Link::Failed,
            _ => {
                self.was_online = false;
                self.failure = None;
                Link::Offline
            }
        };
        let count = |key: &str| status[key].as_u64().unwrap_or(0) as usize;
        let flag = |key: &str| status[key].as_bool() == Some(true);
        let view = StatusView {
            link,
            pending: count("pending"),
            peers: count("peers"),
            waiting_for_snapshot: flag("waiting_for_snapshot"),
            can_undo: flag("can_undo"),
            can_redo: flag("can_redo"),
            api_connected: matches!(state, "connecting" | "syncing" | "online"),
            last_error: status["last_error"].as_str().map(str::to_string),
        };
        (view, notices)
    }
}

/// Which undo the Edit menu and the keys use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UndoRoute {
    /// The normal undo of the document (offline).
    Local,
    /// The per-user undo (`collab.undo`): it takes back your own edits only.
    Collab,
}

/// While the session is connected, the normal undo is refused by the API and could take back an
/// edit of another user, so the per-user undo must be used.
pub fn undo_route(status: &StatusView) -> UndoRoute {
    if status.api_connected {
        UndoRoute::Collab
    } else {
        UndoRoute::Local
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status(state: &str) -> Value {
        json!({"state": state, "reconnecting": false, "last_error": null, "pending": 0,
               "peers": 0, "waiting_for_snapshot": false, "can_undo": false, "can_redo": false})
    }

    fn view(t: &mut Tracker, state: &str) -> StatusView {
        t.observe(&status(state), &[]).0
    }

    #[test]
    fn the_link_follows_the_state() {
        let mut t = Tracker::default();
        assert_eq!(view(&mut t, "none").link, Link::Offline);
        assert_eq!(view(&mut t, "connecting").link, Link::Connecting);
        assert_eq!(view(&mut t, "syncing").link, Link::Connecting);
        assert_eq!(view(&mut t, "online").link, Link::Connected);
        // After it was online, a new try is a reconnect.
        assert_eq!(view(&mut t, "syncing").link, Link::Reconnecting);
        assert_eq!(view(&mut t, "connecting").link, Link::Reconnecting);
        let mut lost = status("offline");
        lost["reconnecting"] = json!(true);
        assert_eq!(t.observe(&lost, &[]).0.link, Link::Reconnecting);
        // Offline without a wish to connect again is offline, and forgets the past.
        assert_eq!(view(&mut t, "offline").link, Link::Offline);
        assert_eq!(view(&mut t, "connecting").link, Link::Connecting);
        // A first try that failed and waits for the next try is still connecting.
        assert_eq!(t.observe(&lost, &[]).0.link, Link::Connecting);
        t.reset();
        assert_eq!(view(&mut t, "weird").link, Link::Offline);
    }

    #[test]
    fn a_refused_token_is_told_apart_from_other_failures() {
        let mut t = Tracker::default();
        assert_eq!(view(&mut t, "failed").link, Link::Failed);
        let event = json!({"event": "failed", "code": "auth_failed", "message": "bad token"});
        let (v, notes) = t.observe(&status("failed"), &[event]);
        assert_eq!(v.link, Link::Refused);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, "uic-collab-err-auth");
        assert_eq!(notes[0].severity, Severity::Error);
        // The reason stays until the link is online again.
        assert_eq!(view(&mut t, "failed").link, Link::Refused);
        view(&mut t, "online");
        assert_eq!(view(&mut t, "failed").link, Link::Failed);
        let event = json!({"event": "failed", "code": "unsupported_protocol", "message": "v2"});
        assert_eq!(t.observe(&status("failed"), &[event]).0.link, Link::Failed);
    }

    #[test]
    fn the_counts_and_flags_are_read() {
        let mut t = Tracker::default();
        let s = json!({"state": "online", "pending": 3, "peers": 2, "waiting_for_snapshot": true,
                       "can_undo": true, "can_redo": true, "last_error": "boom"});
        let v = t.observe(&s, &[]).0;
        assert_eq!((v.pending, v.peers), (3, 2));
        assert!(v.waiting_for_snapshot && v.can_undo && v.can_redo && v.api_connected);
        assert_eq!(v.last_error.as_deref(), Some("boom"));
        assert!(v.is_active());
        // A status with missing fields reads as zero and false.
        let v = t.observe(&json!({}), &[]).0;
        assert_eq!(v, StatusView::default());
        assert!(!v.is_active());
    }

    #[test]
    fn each_error_code_has_a_class_and_a_message() {
        use ErrorKind::*;
        let table = [
            ("auth_failed", AuthRefused),
            ("unsupported_protocol", Protocol),
            ("document_full", DocumentFull),
            ("bad_message", Rejected),
            ("invalid_txn", Rejected),
            ("duplicate_uid", Rejected),
            ("rate_limited", RateLimited),
            ("internal", ServerFault),
            ("op_id_reused", Renumber),
            ("something_new", Unknown),
            ("", Unknown),
        ];
        for (code, kind) in table {
            assert_eq!(ErrorKind::from_code(code), kind, "{code}");
            let text = translate(kind.message_id(), &[]);
            assert!(!text.is_empty() && text != kind.message_id());
        }
    }

    #[test]
    fn events_become_notices() {
        let mut t = Tracker::default();
        let events = [
            json!({"event": "connected"}),
            json!({"event": "disconnected"}),
            json!({"event": "rejected", "op_id": 4, "code": "document_full", "message": "too big"}),
            json!({"event": "server_error", "code": "rate_limited", "message": "slow down"}),
            json!({"event": "server_error", "code": "zzz", "message": "odd"}),
            json!({"event": "gap", "expected": 2, "got": 5}),
            json!({"event": "reconnect", "message": "again"}),
            json!({"event": "regressed", "resent": 1}),
            json!({"event": "renumbered", "count": 2}),
            json!({"event": "store_failed", "message": "disk full"}),
            json!({"event": "store_corrupt", "message": "bad json"}),
            json!({"event": "failed", "code": "internal", "message": "oops"}),
            json!({}),
        ];
        let (_, notes) = t.observe(&status("online"), &events);
        let ids: Vec<&str> = notes.iter().map(|n| n.id).collect();
        assert_eq!(
            ids,
            [
                "uic-collab-note-connected",
                "uic-collab-err-document-full",
                "uic-collab-err-rate-limited",
                "uic-collab-err-unknown",
                "uic-collab-note-regressed",
                "uic-collab-note-renumbered",
                "uic-collab-note-store-failed",
                "uic-collab-note-store-corrupt",
                "uic-collab-err-server-fault",
            ]
        );
        assert_eq!(notes[0].severity, Severity::Info);
        assert_eq!(notes[1].severity, Severity::Error);
        assert_eq!(notes[2].severity, Severity::Warning);
        assert!(notes[3].text().contains("odd") && notes[3].text().contains("zzz"));
        assert!(notes[6].text().contains("disk full"));
        for n in &notes {
            assert!(!n.text().is_empty() && n.text() != n.id, "{}", n.id);
        }
    }

    #[test]
    fn the_label_counts_waiting_edits() {
        let mut v = StatusView {
            link: Link::Connected,
            ..Default::default()
        };
        assert_eq!(v.label(), "Connected");
        v.pending = 1;
        assert_eq!(v.label(), "Connected, 1 edit waiting");
        v.pending = 5;
        v.link = Link::Reconnecting;
        assert_eq!(v.label(), "Reconnecting, 5 edits waiting");
        v.link = Link::Offline;
        assert_eq!(v.label(), "Offline");
        for l in [Link::Connecting, Link::Refused, Link::Failed] {
            assert_ne!(l.text(), l.message_id());
        }
        kadakas_l10n::with_language("et", || {
            v.link = Link::Connected;
            v.pending = 5;
            assert!(v.label().contains('5'));
            assert_ne!(v.label(), "Connected, 5 edits waiting");
        });
    }

    #[test]
    fn the_tooltip_tells_the_people_and_the_last_error() {
        let mut v = StatusView {
            peers: 2,
            ..Default::default()
        };
        assert_eq!(v.tooltip(), "2 other people here");
        v.peers = 1;
        v.last_error = Some("connection refused".into());
        let t = v.tooltip();
        assert!(t.starts_with("1 other person here"), "{t}");
        assert!(t.contains("connection refused"));
    }

    #[test]
    fn undo_goes_to_the_per_user_undo_while_connected() {
        let mut t = Tracker::default();
        for (state, route) in [
            ("none", UndoRoute::Local),
            ("offline", UndoRoute::Local),
            ("failed", UndoRoute::Local),
            ("connecting", UndoRoute::Collab),
            ("syncing", UndoRoute::Collab),
            ("online", UndoRoute::Collab),
        ] {
            assert_eq!(undo_route(&view(&mut t, state)), route, "{state}");
        }
    }
}
