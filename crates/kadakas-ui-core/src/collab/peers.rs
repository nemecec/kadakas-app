//! The other users (presence) and what this user shows to them.
//!
//! Presence is never part of the document. The client library already limits the rate (one
//! update per 50 ms), repeats an unchanged state every 5 s and forgets a user after 15 s without
//! news. This module adds what a UI needs on top: a color for each user, a flag for a user who
//! is quiet, and a sender that does not call the API for a state that did not change.

use kadakas_l10n::{Arg, msg, translate};
use serde_json::{Value, json};

/// The colors of the users, as RGB. They differ in hue and keep their contrast on both the
/// light and the dark canvas. A user gets one by a hash of the user ID; two users on one
/// document never share a color while the palette has a free one.
pub const PALETTE: [[u8; 3]; 8] = [
    [230, 25, 75],
    [0, 130, 200],
    [60, 160, 70],
    [245, 130, 48],
    [145, 30, 180],
    [0, 150, 150],
    [220, 50, 200],
    [150, 100, 40],
];

/// The client ID of the agent in the People palette. A real client has a UID, so it never equals
/// this.
pub const AGENT_CLIENT: &str = "agent";
/// The color of the agent: slate, which is not in [`PALETTE`].
pub const AGENT_RGB: [u8; 3] = [71, 85, 105];

/// A user who changed nothing for this long is shown as quiet (dimmed). The client forgets the
/// user after 15 s without any news, so this applies to a user whose program still sends its
/// heartbeat but who does not move.
pub const IDLE_MS: u64 = 30_000;

/// The part of the drawing that a user looks at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PeerViewport {
    pub center: [f64; 2],
    /// Drawing units (mm) per pixel.
    pub scale: f64,
    pub size: [f64; 2],
}

/// Another client of the document.
#[derive(Clone, Debug, PartialEq)]
pub struct Peer {
    pub client: String,
    pub user: String,
    pub name: String,
    /// The pointer in mm; `None` if it is outside the drawing.
    pub cursor: Option<[f64; 2]>,
    /// Object IDs as text.
    pub selection: Vec<String>,
    pub viewport: Option<PeerViewport>,
    /// Index into [`PALETTE`].
    pub color: usize,
    /// When the state last changed (the time of the caller).
    pub changed_ms: u64,
}

impl Peer {
    /// The color as RGB. The agent (AGR-03) has a color of its own, which no person gets.
    pub fn rgb(&self) -> [u8; 3] {
        if self.client == AGENT_CLIENT {
            return AGENT_RGB;
        }
        PALETTE[self.color % PALETTE.len()]
    }

    /// The name to show: the name, or "Guest" with a short mark of the user ID.
    pub fn label(&self) -> String {
        let name = self.name.trim();
        if !name.is_empty() {
            return name.to_string();
        }
        let tail: String = {
            let chars: Vec<char> = self.user.chars().filter(|c| *c != '-').collect();
            chars[chars.len().saturating_sub(4)..].iter().collect()
        };
        translate(msg!("uic-collab-guest"), &[("id", Arg::Text(tail))])
    }

    /// The label cut to [`MAX_LABEL_CHARS`] characters, with an ellipsis, for the canvas (a name
    /// can be 64 characters long, and a label must not cover the drawing).
    pub fn short_label(&self) -> String {
        let label = self.label();
        if label.chars().count() <= MAX_LABEL_CHARS {
            return label;
        }
        let mut cut: String = label.chars().take(MAX_LABEL_CHARS - 1).collect();
        cut.push('\u{2026}');
        cut
    }
}

/// The longest name that is drawn on the canvas, in characters.
pub const MAX_LABEL_CHARS: usize = 32;

/// FNV-1a, a small stable hash. The color of a user must not change between runs.
fn hash(text: &str) -> usize {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h as usize
}

fn number_pair(v: &Value) -> Option<[f64; 2]> {
    let a = v.as_array()?;
    let (x, y) = (a.first()?.as_f64()?, a.get(1)?.as_f64()?);
    (x.is_finite() && y.is_finite()).then_some([x, y])
}

fn viewport_of(v: &Value) -> Option<PeerViewport> {
    let scale = v.get("scale")?.as_f64()?;
    (scale.is_finite() && scale > 0.0).then_some(())?;
    Some(PeerViewport {
        center: number_pair(v.get("center")?)?,
        scale,
        size: number_pair(v.get("size")?)?,
    })
}

/// The other users, from the answers of `collab.presence`.
#[derive(Clone, Debug, Default)]
pub struct PeerBook {
    peers: Vec<Peer>,
}

impl PeerBook {
    pub fn peers(&self) -> &[Peer] {
        &self.peers
    }

    pub fn clear(&mut self) {
        self.peers.clear();
    }

    /// Takes the `peers` list of an answer of `collab.presence`. A user that is not in the list
    /// is gone (the client expired it). A color stays with its user for as long as the user is
    /// there.
    pub fn update(&mut self, list: &Value, now_ms: u64) {
        let mut next: Vec<Peer> = Vec::new();
        for item in list.as_array().map(Vec::as_slice).unwrap_or_default() {
            let text = |k: &str| item[k].as_str().unwrap_or("").to_string();
            let client = text("client");
            if client.is_empty() || next.iter().any(|p| p.client == client) {
                continue;
            }
            let mut peer = Peer {
                user: text("user"),
                name: text("name"),
                cursor: number_pair(&item["cursor"]),
                selection: item["selection"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|id| id.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                viewport: viewport_of(&item["viewport"]),
                color: 0,
                changed_ms: now_ms,
                client,
            };
            let old = self.peers.iter().find(|p| p.client == peer.client);
            peer.color = match old {
                Some(o) => o.color,
                None => self.color_for_new(&peer.user, &next),
            };
            // The time of the last change stays while the state stays.
            if let Some(o) = old
                && (o.cursor, &o.selection, o.viewport, &o.name)
                    == (peer.cursor, &peer.selection, peer.viewport, &peer.name)
            {
                peer.changed_ms = o.changed_ms;
            }
            next.push(peer);
        }
        self.peers = next;
    }

    /// The color of a user who just came: the one of the same user in another window, else the
    /// first free color from the place that the hash of the user ID gives.
    fn color_for_new(&self, user: &str, next: &[Peer]) -> usize {
        let known = |p: &&Peer| p.user == user;
        if let Some(p) = self.peers.iter().chain(next.iter()).find(known) {
            return p.color;
        }
        let start = hash(user) % PALETTE.len();
        (0..PALETTE.len())
            .map(|i| (start + i) % PALETTE.len())
            .find(|c| !self.peers.iter().chain(next.iter()).any(|p| p.color == *c))
            .unwrap_or(start)
    }

    /// True if the user changed nothing for [`IDLE_MS`].
    pub fn is_idle(peer: &Peer, now_ms: u64) -> bool {
        now_ms.saturating_sub(peer.changed_ms) >= IDLE_MS
    }
}

/// How much a number may change before it counts as a change: 0.1 mm.
fn round_mm(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// A relative step for the scale of the view: 0.1 percent.
fn round_scale(v: f64) -> f64 {
    if v <= 0.0 || !v.is_finite() {
        return 0.0;
    }
    let digits = 3 - v.log10().floor() as i32;
    let f = 10f64.powi(digits);
    (v * f).round() / f
}

#[derive(Clone, Debug, PartialEq)]
struct Shown {
    cursor: Option<[f64; 2]>,
    selection: Vec<String>,
    viewport: Option<([f64; 2], f64, [f64; 2])>,
}

/// What this user shows to the others, and when to tell the API.
#[derive(Clone, Debug, Default)]
pub struct OwnPresence {
    sent: Option<Shown>,
    sent_ms: Option<u64>,
}

/// The most objects in a shared selection (the protocol limit).
const MAX_SELECTION: usize = 1000;
/// The shortest time between two sets (the same as the client).
const MIN_INTERVAL_MS: u64 = 50;

impl OwnPresence {
    /// Forget what was sent (a new connection).
    pub fn reset(&mut self) {
        *self = OwnPresence::default();
    }

    /// The parameters for `collab.presence`. When the state changed since the last set, and the
    /// shortest interval has passed, they hold the new state (or `clear`); else they only read.
    /// The client library merges and repeats updates by itself, so this does not need a timer
    /// of its own.
    pub fn params(
        &mut self,
        now_ms: u64,
        cursor: Option<[f64; 2]>,
        selection: &[String],
        viewport: Option<([f64; 2], f64, [f64; 2])>,
    ) -> Value {
        let shown = Shown {
            cursor: cursor
                .filter(|c| c.iter().all(|v| v.is_finite()))
                .map(|c| [round_mm(c[0]), round_mm(c[1])]),
            selection: selection.iter().take(MAX_SELECTION).cloned().collect(),
            viewport: viewport
                .filter(|(c, s, z)| c.iter().chain(z).all(|v| v.is_finite()) && *s > 0.0)
                .map(|(c, s, z)| ([round_mm(c[0]), round_mm(c[1])], round_scale(s), z)),
        };
        let read = json!({"now_ms": now_ms});
        let due = self
            .sent_ms
            .is_none_or(|t| now_ms.saturating_sub(t) >= MIN_INTERVAL_MS);
        let unchanged = match &self.sent {
            Some(s) => *s == shown,
            // Nothing was sent: an empty state needs no call.
            None => {
                shown.cursor.is_none() && shown.selection.is_empty() && shown.viewport.is_none()
            }
        };
        if unchanged || !due {
            return read;
        }
        let empty =
            shown.cursor.is_none() && shown.selection.is_empty() && shown.viewport.is_none();
        let mut p = if empty {
            json!({"now_ms": now_ms, "clear": true})
        } else {
            let mut p = json!({"now_ms": now_ms, "selection": shown.selection});
            if let Some(c) = shown.cursor {
                p["cursor"] = json!(c);
            }
            if let Some((center, scale, size)) = shown.viewport {
                p["viewport"] = json!({"center": center, "scale": scale, "size": size});
            }
            p
        };
        p["now_ms"] = json!(now_ms);
        self.sent = Some(shown);
        self.sent_ms = Some(now_ms);
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer_json(client: &str, user: &str, name: &str, cursor: Option<[f64; 2]>) -> Value {
        json!({"client": client, "user": user, "name": name, "cursor": cursor,
               "selection": [], "viewport": null})
    }

    #[test]
    fn a_book_reads_the_answer() {
        let mut book = PeerBook::default();
        let list = json!([{
            "client": "c1", "user": "u1", "name": "Bo", "cursor": [10.5, 20.0],
            "selection": ["a", "b", 7],
            "viewport": {"center": [1.0, 2.0], "scale": 0.5, "size": [800.0, 600.0]},
        }]);
        book.update(&list, 100);
        let p = &book.peers()[0];
        assert_eq!((p.client.as_str(), p.name.as_str()), ("c1", "Bo"));
        assert_eq!(p.cursor, Some([10.5, 20.0]));
        assert_eq!(p.selection, ["a", "b"]);
        assert_eq!(
            p.viewport,
            Some(PeerViewport {
                center: [1.0, 2.0],
                scale: 0.5,
                size: [800.0, 600.0]
            })
        );
        assert_eq!(p.label(), "Bo");
    }

    #[test]
    fn bad_entries_are_skipped_or_cleaned() {
        let mut book = PeerBook::default();
        let list = json!([
            {"user": "no client"},
            peer_json("c1", "u1", "", None),
            peer_json("c1", "u1", "again", None),
            {"client": "c2", "user": "u2", "cursor": ["x", 1], "viewport": {"scale": -1}},
            {"client": "c3", "user": "u3", "viewport": {"center": [0, 0], "scale": 1.0, "size": [1, 1]},
             "cursor": [1.0]},
        ]);
        book.update(&list, 0);
        let ids: Vec<&str> = book.peers().iter().map(|p| p.client.as_str()).collect();
        assert_eq!(ids, ["c1", "c2", "c3"]);
        assert_eq!(book.peers()[0].name, "", "the first entry of a client wins");
        assert_eq!(book.peers()[1].cursor, None);
        assert_eq!(book.peers()[1].viewport, None);
        assert!(book.peers()[2].viewport.is_some());
        book.update(&json!("not a list"), 0);
        assert!(book.peers().is_empty());
        book.update(&list, 0);
        book.clear();
        assert!(book.peers().is_empty());
    }

    #[test]
    fn a_user_without_a_name_is_a_guest() {
        let mut book = PeerBook::default();
        book.update(
            &json!([peer_json(
                "c1",
                "01939a2e-7c1d-7a2b-9d3e-0123456789ab",
                "  ",
                None
            )]),
            0,
        );
        assert_eq!(book.peers()[0].label(), "Guest 89ab");
        kadakas_l10n::with_language("et", || {
            assert!(book.peers()[0].label().ends_with("89ab"));
            assert!(!book.peers()[0].label().starts_with("Guest"));
        });
        let short = Peer {
            user: "ab".into(),
            ..book.peers()[0].clone()
        };
        assert_eq!(short.label(), "Guest ab");
    }

    #[test]
    fn a_long_name_is_cut_for_the_canvas() {
        let mut book = PeerBook::default();
        let long = "\u{c4}".repeat(60);
        book.update(&json!([peer_json("c1", "u1", &long, None)]), 0);
        let p = &book.peers()[0];
        assert_eq!(
            p.label().chars().count(),
            60,
            "the list shows the whole name"
        );
        let short = p.short_label();
        assert_eq!(short.chars().count(), MAX_LABEL_CHARS);
        assert!(short.ends_with('\u{2026}'));
        book.update(&json!([peer_json("c1", "u1", "Ann", None)]), 0);
        assert_eq!(book.peers()[0].short_label(), "Ann");
        let exact = "x".repeat(MAX_LABEL_CHARS);
        book.update(&json!([peer_json("c1", "u1", &exact, None)]), 0);
        assert_eq!(book.peers()[0].short_label(), exact);
    }

    #[test]
    fn colors_are_stable_and_different() {
        let users: Vec<String> = (0..PALETTE.len()).map(|i| format!("user-{i}")).collect();
        let list: Vec<Value> = users
            .iter()
            .enumerate()
            .map(|(i, u)| peer_json(&format!("c{i}"), u, "", None))
            .collect();
        let mut book = PeerBook::default();
        book.update(&Value::Array(list.clone()), 0);
        let mut colors: Vec<usize> = book.peers().iter().map(|p| p.color).collect();
        colors.sort_unstable();
        colors.dedup();
        assert_eq!(colors.len(), PALETTE.len(), "no color is used twice");
        // The same list again gives the same colors.
        let before: Vec<usize> = book.peers().iter().map(|p| p.color).collect();
        book.update(&Value::Array(list.clone()), 50);
        let after: Vec<usize> = book.peers().iter().map(|p| p.color).collect();
        assert_eq!(before, after);
        // A user who stays keeps the color when others leave.
        book.update(&Value::Array(list[3..4].to_vec()), 60);
        assert_eq!(book.peers()[0].color, before[3]);
        // A user that comes gets the same color on another run (the hash is stable).
        let mut other = PeerBook::default();
        other.update(&Value::Array(list[3..4].to_vec()), 0);
        assert_eq!(other.peers()[0].color, before[3]);
        assert_eq!(other.peers()[0].rgb(), PALETTE[before[3]]);
    }

    #[test]
    fn two_windows_of_one_user_share_a_color_and_a_full_palette_still_works() {
        let mut book = PeerBook::default();
        book.update(
            &json!([
                peer_json("c1", "u", "", None),
                peer_json("c2", "u", "", None)
            ]),
            0,
        );
        assert_eq!(book.peers()[0].color, book.peers()[1].color);
        let many: Vec<Value> = (0..PALETTE.len() + 3)
            .map(|i| peer_json(&format!("c{i}"), &format!("u{i}"), "", None))
            .collect();
        book.update(&Value::Array(many), 0);
        assert_eq!(book.peers().len(), PALETTE.len() + 3);
        assert!(book.peers().iter().all(|p| p.color < PALETTE.len()));
    }

    #[test]
    fn a_quiet_user_becomes_idle_and_a_move_wakes_the_user() {
        let mut book = PeerBook::default();
        let at = |x: f64| json!([peer_json("c1", "u1", "Bo", Some([x, 0.0]))]);
        book.update(&at(1.0), 1_000);
        assert!(!PeerBook::is_idle(&book.peers()[0], 1_000 + IDLE_MS - 1));
        // The same state again keeps the old time of change.
        book.update(&at(1.0), 20_000);
        assert!(PeerBook::is_idle(&book.peers()[0], 1_000 + IDLE_MS));
        // A move is a change.
        book.update(&at(2.0), 40_000);
        assert!(!PeerBook::is_idle(&book.peers()[0], 40_000 + IDLE_MS - 1));
        assert!(PeerBook::is_idle(&book.peers()[0], 40_000 + IDLE_MS));
        // A clock that went back does not make an idle user.
        assert!(!PeerBook::is_idle(&book.peers()[0], 0));
    }

    #[test]
    fn nothing_to_show_makes_no_call() {
        let mut own = OwnPresence::default();
        assert_eq!(own.params(1_000, None, &[], None), json!({"now_ms": 1_000}));
    }

    #[test]
    fn a_change_is_sent_once_and_after_the_interval() {
        let mut own = OwnPresence::default();
        let vp = Some(([100.0, 200.0], 0.5, [800.0, 600.0]));
        let sel = vec!["a".to_string()];
        let p = own.params(1_000, Some([1.04, 2.0]), &sel, vp);
        assert_eq!(p["cursor"], json!([1.0, 2.0]));
        assert_eq!(p["selection"], json!(["a"]));
        assert_eq!(p["viewport"]["scale"], 0.5);
        assert!(p.get("clear").is_none());
        // The same state: a read.
        assert_eq!(
            own.params(2_000, Some([1.04, 2.0]), &sel, vp),
            json!({"now_ms": 2_000})
        );
        // A change inside the shortest interval waits.
        assert_eq!(
            own.params(1_020, Some([5.0, 2.0]), &sel, vp),
            json!({"now_ms": 1_020})
        );
        let p = own.params(1_050, Some([5.0, 2.0]), &sel, vp);
        assert_eq!(p["cursor"], json!([5.0, 2.0]));
        // A change smaller than 0.1 mm is not a change.
        assert_eq!(
            own.params(5_000, Some([5.01, 2.0]), &sel, vp),
            json!({"now_ms": 5_000})
        );
    }

    #[test]
    fn leaving_the_drawing_clears_the_presence() {
        let mut own = OwnPresence::default();
        own.params(0, Some([1.0, 1.0]), &[], None);
        let p = own.params(100, None, &[], None);
        assert_eq!(p, json!({"now_ms": 100, "clear": true}));
        assert_eq!(own.params(200, None, &[], None), json!({"now_ms": 200}));
        own.params(300, Some([1.0, 1.0]), &[], None);
        own.reset();
        // After a reset the same state is sent again.
        assert!(
            own.params(400, Some([1.0, 1.0]), &[], None)
                .get("cursor")
                .is_some()
        );
    }

    #[test]
    fn numbers_that_are_not_finite_and_long_selections_are_cut() {
        let mut own = OwnPresence::default();
        let p = own.params(
            0,
            Some([f64::NAN, 1.0]),
            &[],
            Some(([0.0, 0.0], 0.0, [1.0, 1.0])),
        );
        assert_eq!(p, json!({"now_ms": 0}), "nothing valid to show");
        let many: Vec<String> = (0..1500).map(|i| i.to_string()).collect();
        let p = own.params(100, None, &many, None);
        assert_eq!(p["selection"].as_array().unwrap().len(), 1000);
        assert!(p.get("cursor").is_none() && p.get("viewport").is_none());
    }

    #[test]
    fn the_scale_is_rounded_to_a_relative_step() {
        assert_eq!(round_scale(0.123456), 0.1235);
        assert_eq!(round_scale(12.3456), 12.35);
        assert_eq!(round_scale(0.0), 0.0);
        assert_eq!(round_scale(f64::NAN), 0.0);
        let mut own = OwnPresence::default();
        own.params(
            0,
            Some([0.0, 0.0]),
            &[],
            Some(([0.0, 0.0], 1.0, [10.0, 10.0])),
        );
        // A zoom change of 0.01 percent is not a change.
        assert_eq!(
            own.params(
                1_000,
                Some([0.0, 0.0]),
                &[],
                Some(([0.0, 0.0], 1.0001, [10.0, 10.0]))
            ),
            json!({"now_ms": 1_000})
        );
    }
}
