//! Presence (PLAT-08): where the other users are and what they look at.
//!
//! Presence is ephemeral. It is not an operation, it has no sequence number, the server does not
//! store it, and it is never part of the document, a snapshot or an offline log. A new
//! connection learns the presence of the others from the `Welcome` message. A user who
//! disconnects or goes quiet disappears from the others after a time.
//!
//! Three things keep presence cheap:
//!
//! - A sender sends at most one update per [`MIN_INTERVAL_MS`]. Updates in between merge: only the
//!   latest state is sent ([`Outgoing`]).
//! - A sender repeats its state every [`HEARTBEAT_MS`] when nothing changes, so the others know it
//!   is still there.
//! - A receiver forgets a user after [`TTL_MS`] without news ([`Peers::expire`]).
//!
//! A server enforces the same minimum interval for each client ([`Throttle`]). Updates that come
//! faster are not dropped for good: the server keeps the latest one and forwards it when the
//! interval has passed (the *trailing edge*), so the last position of a cursor always arrives. The
//! server also checks the size of the state ([`PresenceState::check`]).
//!
//! Time is an argument (`now_ms`, milliseconds on any steady clock), so the code is pure.

use std::collections::BTreeMap;

use kadakas_core::uid::Uid;
use serde::{Deserialize, Serialize};

/// The shortest time between two updates of one user.
pub const MIN_INTERVAL_MS: u64 = 50;
/// A sender repeats an unchanged state after this time.
pub const HEARTBEAT_MS: u64 = 5_000;
/// A receiver forgets a user after this time without news.
pub const TTL_MS: u64 = 15_000;
/// The most objects in a shared selection.
pub const MAX_SELECTION: usize = 1_000;
/// The longest user name, in characters.
pub const MAX_NAME: usize = 64;

/// The part of the drawing that a user looks at.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    /// The center, in drawing units (mm).
    pub center: [f64; 2],
    /// Drawing units (mm) per screen pixel.
    pub scale: f64,
    /// The size of the window in pixels.
    pub size: [f64; 2],
}

/// What one user shows to the others.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PresenceState {
    /// The pointer, in drawing units (mm). `None`: the pointer is outside the drawing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<[f64; 2]>,
    /// The selected objects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selection: Vec<Uid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<Viewport>,
}

/// Why a presence state is refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PresenceError {
    #[error("presence numbers must be finite")]
    NotFinite,
    #[error("a selection can hold at most {MAX_SELECTION} objects")]
    SelectionTooLarge,
    #[error("the viewport scale must be above zero")]
    BadScale,
}

impl PresenceState {
    /// Checks the numbers and the size.
    pub fn check(&self) -> Result<(), PresenceError> {
        let finite = |v: &[f64]| v.iter().all(|x| x.is_finite());
        if self.cursor.is_some_and(|c| !finite(&c)) {
            return Err(PresenceError::NotFinite);
        }
        if let Some(v) = &self.viewport {
            if !finite(&v.center) || !finite(&v.size) || !v.scale.is_finite() {
                return Err(PresenceError::NotFinite);
            }
            if v.scale <= 0.0 {
                return Err(PresenceError::BadScale);
            }
        }
        if self.selection.len() > MAX_SELECTION {
            return Err(PresenceError::SelectionTooLarge);
        }
        Ok(())
    }
}

/// Cuts a user name to the allowed length.
pub fn clean_name(name: &str) -> String {
    name.chars().take(MAX_NAME).collect()
}

/// One user as the others see them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Peer {
    /// The client instance. One user on two devices shows two cursors.
    pub client: Uid,
    pub user: Uid,
    pub name: String,
    #[serde(default)]
    pub state: PresenceState,
}

/// A minimum interval between events. A server uses it to drop updates that come too fast.
#[derive(Clone, Copy, Debug)]
pub struct Throttle {
    min_interval_ms: u64,
    last: Option<u64>,
}

impl Throttle {
    pub fn new(min_interval_ms: u64) -> Throttle {
        Throttle {
            min_interval_ms,
            last: None,
        }
    }

    /// True if an event at this time is allowed. It records the event when it returns true.
    pub fn allow(&mut self, now_ms: u64) -> bool {
        if self.ready(now_ms) {
            self.last = Some(now_ms);
            true
        } else {
            false
        }
    }

    /// True if an event at this time would be allowed.
    pub fn ready(&self, now_ms: u64) -> bool {
        self.last
            .is_none_or(|l| now_ms.saturating_sub(l) >= self.min_interval_ms)
    }

    /// The time of the last allowed event.
    pub fn last(&self) -> Option<u64> {
        self.last
    }
}

/// The outgoing side: merges updates and decides when to send.
#[derive(Clone, Debug)]
pub struct Outgoing {
    state: PresenceState,
    dirty: bool,
    throttle: Throttle,
}

impl Default for Outgoing {
    fn default() -> Self {
        Outgoing {
            state: PresenceState::default(),
            dirty: false,
            throttle: Throttle::new(MIN_INTERVAL_MS),
        }
    }
}

impl Outgoing {
    /// Sets the state to show. Nothing is sent until [`Outgoing::poll`].
    pub fn set(&mut self, state: PresenceState) -> Result<(), PresenceError> {
        state.check()?;
        if state != self.state {
            self.state = state;
            self.dirty = true;
        }
        Ok(())
    }

    pub fn state(&self) -> &PresenceState {
        &self.state
    }

    /// Marks the state as not sent: the next poll sends it. For a new connection.
    pub fn resend(&mut self) {
        self.dirty = true;
    }

    /// The state to send now, if there is one: it changed and the interval has passed, or it
    /// has not been sent for [`HEARTBEAT_MS`].
    pub fn poll(&mut self, now_ms: u64) -> Option<PresenceState> {
        let due = match self.throttle.last() {
            Some(last) => self.dirty || now_ms.saturating_sub(last) >= HEARTBEAT_MS,
            None => self.dirty,
        };
        if due && self.throttle.allow(now_ms) {
            self.dirty = false;
            Some(self.state.clone())
        } else {
            None
        }
    }
}

/// The other clients, with the time of their last news.
#[derive(Clone, Debug, Default)]
pub struct Peers {
    peers: BTreeMap<Uid, (Peer, u64)>,
}

impl Peers {
    /// Adds or updates a user.
    pub fn update(&mut self, peer: Peer, now_ms: u64) {
        self.peers.insert(peer.client, (peer, now_ms));
    }

    pub fn remove(&mut self, client: Uid) -> bool {
        self.peers.remove(&client).is_some()
    }

    pub fn clear(&mut self) {
        self.peers.clear();
    }

    /// Forgets the users who sent nothing for [`TTL_MS`]. Returns their IDs.
    pub fn expire(&mut self, now_ms: u64) -> Vec<Uid> {
        let gone: Vec<Uid> = self
            .peers
            .iter()
            .filter(|(_, (_, seen))| now_ms.saturating_sub(*seen) >= TTL_MS)
            .map(|(u, _)| *u)
            .collect();
        for u in &gone {
            self.peers.remove(u);
        }
        gone
    }

    pub fn get(&self, client: Uid) -> Option<&Peer> {
        self.peers.get(&client).map(|(p, _)| p)
    }

    /// All users, ordered by ID.
    pub fn list(&self) -> Vec<Peer> {
        self.peers.values().map(|(p, _)| p.clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64) -> PresenceState {
        PresenceState {
            cursor: Some([x, 0.0]),
            ..Default::default()
        }
    }

    #[test]
    fn check_refuses_bad_values() {
        assert_eq!(at(1.0).check(), Ok(()));
        assert_eq!(at(f64::NAN).check(), Err(PresenceError::NotFinite));
        let big = PresenceState {
            selection: vec![Uid::from_num(1); MAX_SELECTION + 1],
            ..Default::default()
        };
        assert_eq!(big.check(), Err(PresenceError::SelectionTooLarge));
        let mut v = PresenceState {
            viewport: Some(Viewport {
                center: [0.0, 0.0],
                scale: 0.0,
                size: [800.0, 600.0],
            }),
            ..Default::default()
        };
        assert_eq!(v.check(), Err(PresenceError::BadScale));
        v.viewport.as_mut().unwrap().scale = 2.0;
        assert_eq!(v.check(), Ok(()));
        v.viewport.as_mut().unwrap().center[0] = f64::INFINITY;
        assert_eq!(v.check(), Err(PresenceError::NotFinite));
        v.viewport.as_mut().unwrap().center[0] = 0.0;
        v.viewport.as_mut().unwrap().scale = f64::NAN;
        assert_eq!(v.check(), Err(PresenceError::NotFinite));
        assert!(!PresenceError::BadScale.to_string().is_empty());
    }

    #[test]
    fn names_are_cut() {
        assert_eq!(clean_name("abc"), "abc");
        assert_eq!(clean_name(&"é".repeat(100)).chars().count(), MAX_NAME);
    }

    #[test]
    fn throttle_spaces_events() {
        let mut t = Throttle::new(100);
        assert!(t.ready(0));
        assert!(t.allow(0));
        assert!(!t.allow(50));
        assert!(!t.ready(99));
        assert!(t.allow(100));
        assert_eq!(t.last(), Some(100));
    }

    #[test]
    fn outgoing_merges_and_rate_limits() {
        let mut o = Outgoing::default();
        // Nothing to send before the first set.
        assert_eq!(o.poll(0), None);
        o.set(at(1.0)).unwrap();
        assert_eq!(o.poll(1_000), Some(at(1.0)));
        // Updates inside the interval merge: only the latest goes out.
        o.set(at(2.0)).unwrap();
        assert_eq!(o.poll(1_010), None);
        o.set(at(3.0)).unwrap();
        assert_eq!(o.poll(1_020), None);
        assert_eq!(o.poll(1_000 + MIN_INTERVAL_MS), Some(at(3.0)));
        assert_eq!(o.state(), &at(3.0));
        // The same state again is not new.
        o.set(at(3.0)).unwrap();
        assert_eq!(o.poll(2_000), None);
        // A heartbeat repeats it.
        assert_eq!(
            o.poll(1_000 + MIN_INTERVAL_MS + HEARTBEAT_MS),
            Some(at(3.0))
        );
        // Resend marks it as new.
        o.resend();
        assert!(o.poll(100_000).is_some());
        // A bad state is refused and does not change anything.
        assert!(o.set(at(f64::NAN)).is_err());
        assert_eq!(o.state(), &at(3.0));
    }

    #[test]
    fn peers_expire() {
        let mut p = Peers::default();
        assert!(p.is_empty());
        let peer = |n: u64| Peer {
            client: Uid::from_num(n),
            user: Uid::from_num(n + 100),
            name: format!("u{n}"),
            state: at(n as f64),
        };
        p.update(peer(2), 0);
        p.update(peer(1), 10_000);
        assert_eq!(p.len(), 2);
        assert_eq!(p.list()[0].client, Uid::from_num(1));
        assert_eq!(p.get(Uid::from_num(2)).unwrap().name, "u2");
        assert!(p.expire(14_999).is_empty());
        assert_eq!(p.expire(15_000), vec![Uid::from_num(2)]);
        assert!(p.get(Uid::from_num(2)).is_none());
        assert!(p.remove(Uid::from_num(1)));
        assert!(!p.remove(Uid::from_num(1)));
        p.update(peer(3), 0);
        p.clear();
        assert!(p.is_empty());
    }

    #[test]
    fn json_form_skips_empty_parts() {
        let text = serde_json::to_string(&PresenceState::default()).unwrap();
        assert_eq!(text, "{}");
        let back: PresenceState = serde_json::from_str("{}").unwrap();
        assert_eq!(back, PresenceState::default());
    }
}
