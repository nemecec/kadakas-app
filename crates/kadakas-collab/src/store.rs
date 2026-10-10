//! The persistent op log of a client (the offline log).
//!
//! A client must not lose work when the network goes away or the program ends. It saves two
//! things through an [`OpLogStore`]:
//!
//! - The **journal**: the transactions that the server has not acknowledged (the pending queue),
//!   the undo and redo stacks, and the counter of transaction numbers. It is small. The client
//!   saves it after every change.
//! - The **checkpoint**: the state that the server confirmed, with its sequence number. It is
//!   larger. The client saves it when it finishes a sync and when it disconnects.
//!
//! After a restart, the client builds itself from both (see `Client::restore`). The pending
//! transactions go to the server again when the connection is back. Their identity is the user
//! and the transaction number, and the objects in them have UUIDv7 IDs, so the server and the
//! other clients can tell what is new and what they have already. The server answers a
//! transaction that it has already sequenced with the old sequence number, never twice.
//!
//! The store is a trait because this crate has no file access. A native host writes the text to a
//! file (for example with the atomic write of `kadakas-sys`). A browser host uses IndexedDB. The
//! text is JSON.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use kadakas_core::uid::Uid;
use serde::{Deserialize, Serialize};

use crate::op::Txn;
use crate::replica::Replica;
use crate::undo::UndoEntry;

/// The two things a store keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Slot {
    Journal,
    Checkpoint,
}

/// A place to keep text. A failure is a message, and the client reports it as an event; it does
/// not stop the client.
pub trait OpLogStore: Send {
    fn save(&mut self, slot: Slot, text: &str) -> Result<(), String>;
    fn load(&self, slot: Slot) -> Option<String>;
}

/// The largest journal that a client saves, in bytes of JSON. Above it the oldest undo and redo
/// steps go first.
pub const MAX_JOURNAL_BYTES: usize = 64 * 1024 * 1024;

/// The pending work of a client.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Journal {
    pub user: Uid,
    /// The client instance: the writer of the transactions. It stays the same after a restart,
    /// so the numbers of the transactions go on.
    pub client: Uid,
    /// The number of the last transaction that this user made.
    pub last_op: u64,
    pub pending: Vec<Txn>,
    pub undo: Vec<UndoEntry>,
    pub redo: Vec<UndoEntry>,
    /// The blobs (pictures) that the waiting transactions refer to, by hash.
    #[serde(default)]
    pub blobs: BTreeMap<String, String>,
}

/// The confirmed state of a client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// The name of the log that `state` comes from (empty if the client did not know it).
    #[serde(default)]
    pub log_id: String,
    pub seq: u64,
    pub state: Replica,
    /// The blobs (pictures) that the client has, by hash.
    #[serde(default)]
    pub blobs: BTreeMap<String, String>,
    /// The chain hash (hex) at `seq`, if the server sent it. Old checkpoints have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain: Option<String>,
}

/// A store in memory. Clones share the data, so a test can "restart" a client with a clone.
#[derive(Clone, Debug, Default)]
pub struct MemStore {
    slots: Arc<Mutex<BTreeMap<Slot, String>>>,
    fail: Arc<Mutex<bool>>,
}

impl MemStore {
    pub fn new() -> MemStore {
        MemStore::default()
    }

    /// Makes every later save fail (or work again). For tests.
    pub fn set_failing(&self, fail: bool) {
        *self.fail.lock().unwrap() = fail;
    }
}

impl OpLogStore for MemStore {
    fn save(&mut self, slot: Slot, text: &str) -> Result<(), String> {
        if *self.fail.lock().unwrap() {
            return Err("the store is full".into());
        }
        self.slots.lock().unwrap().insert(slot, text.to_string());
        Ok(())
    }

    fn load(&self, slot: Slot) -> Option<String> {
        self.slots.lock().unwrap().get(&slot).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_store_shares_data_between_clones() {
        let mut a = MemStore::new();
        let b = a.clone();
        assert_eq!(b.load(Slot::Journal), None);
        a.save(Slot::Journal, "x").unwrap();
        assert_eq!(b.load(Slot::Journal).as_deref(), Some("x"));
        assert_eq!(b.load(Slot::Checkpoint), None);
        a.set_failing(true);
        assert!(a.save(Slot::Journal, "y").is_err());
        assert_eq!(b.load(Slot::Journal).as_deref(), Some("x"));
        a.set_failing(false);
        assert!(a.save(Slot::Journal, "y").is_ok());
    }

    #[test]
    fn journal_and_checkpoint_round_trip() {
        let j = Journal {
            user: Uid::from_num(1),
            last_op: 4,
            ..Default::default()
        };
        let back: Journal = serde_json::from_str(&serde_json::to_string(&j).unwrap()).unwrap();
        assert_eq!(back, j);
        let c = Checkpoint {
            log_id: "log".into(),
            seq: 3,
            state: Replica::new(),
            blobs: BTreeMap::new(),
            chain: None,
        };
        let back: Checkpoint = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }
}
