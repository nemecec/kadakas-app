//! The lasting sign in the status bar that an agent server runs: listening, connected (with the
//! count), or working while a call runs. It uses the agent color (AGR-02, AGR-03), has a tooltip
//! with the address, the clients, the last call and the lock of the view, and opens the Agent
//! Activity panel on a click. A connect and a disconnect give a short notice (not every call).
//!
//! The data is `agent::Status` (read-only); this file only decides what to say and draws it.

use std::net::SocketAddr;

use egui::{Color32, Ui};
use kadakas_l10n::tr;
use kadakas_ui_core::collab::AGENT_RGB;

use crate::agent::Status;

/// What the indicator says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The server runs, no client is connected.
    Listening,
    /// This many clients are connected.
    Connected(usize),
    /// A call runs.
    Working,
}

pub fn kind(s: &Status) -> Kind {
    if s.clients == 0 {
        Kind::Listening
    } else if s.calls > 0 {
        Kind::Working
    } else {
        Kind::Connected(s.clients)
    }
}

pub fn label(k: Kind) -> String {
    match k {
        Kind::Listening => tr!("agent-status-listening"),
        Kind::Connected(n) => tr!("agent-status-connected", count = n),
        Kind::Working => tr!("agent-status-working"),
    }
}

/// The tooltip: address, clients, time of the last call, and the lock of the view. `locked` is
/// true when the person turned off "Agent may move my view".
pub fn tooltip(addr: SocketAddr, s: &Status, locked: bool, now_ms: u64) -> String {
    let last = match s.last_call_ms {
        Some(t) => tr!(
            "agent-status-last-ago",
            secs = now_ms.saturating_sub(t) / 1000
        ),
        None => tr!("agent-status-last-none"),
    };
    let lock = if locked {
        tr!("agent-status-no")
    } else {
        tr!("agent-status-yes")
    };
    tr!(
        "agent-status-tip",
        addr = addr.to_string(),
        clients = s.clients,
        last = last,
        lock = lock
    )
}

/// A change in the number of clients that the person is told about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// An agent connected; this many are connected now.
    Connected(usize),
    /// An agent disconnected; this many are still connected.
    Disconnected(usize),
}

impl Notice {
    pub fn text(self) -> String {
        match self {
            Notice::Connected(n) => tr!("agent-status-notice-connected", count = n),
            Notice::Disconnected(n) => tr!("agent-status-notice-disconnected", count = n),
        }
    }
}

/// Remembers the number of clients, to tell a connect from a disconnect.
#[derive(Default)]
pub struct Watch {
    clients: usize,
}

impl Watch {
    pub fn update(&mut self, clients: usize) -> Option<Notice> {
        let before = std::mem::replace(&mut self.clients, clients);
        match clients.cmp(&before) {
            std::cmp::Ordering::Greater => Some(Notice::Connected(clients)),
            std::cmp::Ordering::Less => Some(Notice::Disconnected(clients)),
            std::cmp::Ordering::Equal => None,
        }
    }
}

pub fn color() -> Color32 {
    Color32::from_rgb(AGENT_RGB[0], AGENT_RGB[1], AGENT_RGB[2])
}

/// Draws the indicator in the status bar. True when it was clicked (open the Activity panel).
pub fn item(ui: &mut Ui, addr: SocketAddr, s: &Status, locked: bool, now_ms: u64) -> bool {
    let text = egui::RichText::new(label(kind(s))).color(color());
    ui.add(egui::Button::new(text).frame(false))
        .on_hover_text(tooltip(addr, s, locked, now_ms))
        .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(clients: usize, calls: usize) -> Status {
        Status {
            clients,
            calls,
            last_call_ms: None,
        }
    }

    #[test]
    fn the_status_says_listening_connected_and_working() {
        kadakas_l10n::with_language("en-US", || {
            assert_eq!(label(kind(&st(0, 0))), "Agent: listening");
            assert_eq!(label(kind(&st(1, 0))), "Agent connected");
            assert_eq!(label(kind(&st(2, 0))), "Agent connected (2)");
            assert_eq!(label(kind(&st(1, 1))), "Agent working…");
            // A call without a client (it ended) is not work.
            assert_eq!(kind(&st(0, 1)), Kind::Listening);
        });
        kadakas_l10n::with_language("et", || {
            assert!(label(kind(&st(2, 0))).contains('2'));
            assert!(!label(kind(&st(0, 0))).is_empty());
        });
    }

    #[test]
    fn the_tooltip_has_the_address_the_clients_the_last_call_and_the_lock() {
        let addr: SocketAddr = "127.0.0.1:4711".parse().unwrap();
        let s = Status {
            clients: 2,
            calls: 0,
            last_call_ms: Some(10_000),
        };
        kadakas_l10n::with_language("en-US", || {
            let tip = tooltip(addr, &s, false, 15_000);
            assert!(tip.contains("127.0.0.1:4711"), "{tip}");
            assert!(tip.contains("2"), "{tip}");
            assert!(tip.contains("5 s ago"), "{tip}");
            assert!(tip.contains("Agent may move my view: yes"), "{tip}");
            let tip = tooltip(addr, &st(1, 0), true, 15_000);
            assert!(tip.contains("no call yet"), "{tip}");
            assert!(tip.contains("Agent may move my view: no"), "{tip}");
        });
    }

    #[test]
    fn a_connect_and_a_disconnect_give_a_notice_and_nothing_else_does() {
        let mut w = Watch::default();
        assert_eq!(w.update(0), None);
        assert_eq!(w.update(1), Some(Notice::Connected(1)));
        assert_eq!(w.update(1), None);
        assert_eq!(w.update(2), Some(Notice::Connected(2)));
        assert_eq!(w.update(1), Some(Notice::Disconnected(1)));
        assert_eq!(w.update(0), Some(Notice::Disconnected(0)));
        kadakas_l10n::with_language("en-US", || {
            assert!(Notice::Connected(1).text().contains("connected"));
            assert!(Notice::Disconnected(0).text().contains("disconnected"));
        });
    }
}
