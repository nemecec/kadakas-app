//! The wire protocol between a client and a collaboration server.
//!
//! Messages are JSON objects with a `type` field, sent one per frame (a WebSocket text frame, or
//! one line). Both directions are listed here as two enums. The version number is
//! [`PROTOCOL_VERSION`]. `docs/collaboration.md` explains the order of the messages.
//!
//! In short:
//!
//! 1. The client sends `hello` with the protocol version, its user ID, its name, a token and the
//!    last sequence number it has (if any).
//! 2. The server answers `welcome` with the head of its log, the last transaction of this user
//!    that it has sequenced, and the other users with their presence. Then it sends `entries`
//!    (what the client missed) or `snapshot` (the whole state).
//! 3. The client sends `submit` with a transaction. The server orders it, gives it the next
//!    sequence number, sends `ack` to the author and `broadcast` to all others.
//! 4. Presence goes as `presence` messages. They have no sequence number.
//! 5. A message that the server refuses gets an `error`. If the error has an `op_id`, only
//!    that transaction was refused.
//!
//! The sequence numbers have no gaps. Each connected client gets every sequence number exactly
//! once, as `ack` (its own) or `broadcast` (the others) or in `entries`. A client that sees a gap
//! reconnects.

use kadakas_core::uid::Uid;
use serde::{Deserialize, Serialize};

use crate::op::{Op, Txn};
use crate::presence::{Peer, PresenceState};
use crate::replica::Replica;

/// The version of this protocol. A server refuses a `hello` with another version.
pub const PROTOCOL_VERSION: u32 = 1;

/// One transaction in the log of the server, with its place in the order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// The sequence number. The first entry has 1.
    pub seq: u64,
    pub author: Uid,
    /// The number of the transaction for its author (`Txn::op_id`).
    pub op_id: u64,
    pub ops: Vec<Op>,
}

impl Entry {
    pub fn txn(&self) -> Txn {
        Txn {
            op_id: self.op_id,
            ops: self.ops.clone(),
        }
    }
}

/// The reason in an `error` message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The server does not speak the protocol version of the client.
    UnsupportedProtocol,
    /// The token is wrong or missing. The connection ends.
    AuthFailed,
    /// The message is not valid, or comes at a wrong time (before `hello`).
    BadMessage,
    /// A `create` used an ID that exists or existed.
    DuplicateUid,
    /// The transaction is empty, too large or malformed.
    InvalidTxn,
    /// The user sent too much too fast.
    RateLimited,
    /// A fault in the server.
    Internal,
}

impl ErrorCode {
    /// True if the connection cannot go on after this error.
    pub fn is_fatal(self) -> bool {
        matches!(self, ErrorCode::UnsupportedProtocol | ErrorCode::AuthFailed)
    }
}

/// Messages from a client to the server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Hello {
        protocol: u32,
        user: Uid,
        name: String,
        token: String,
        /// The last sequence number the client has, or `None` if it has no state yet.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_seq: Option<u64>,
    },
    /// A transaction. The server answers with `ack`, or with `error` that has this `op_id`. The
    /// server must treat a second submit with the same user and `op_id` as the first (it answers
    /// with the old sequence number): a client sends again after a lost connection.
    Submit {
        op_id: u64,
        ops: Vec<Op>,
    },
    Presence {
        state: PresenceState,
    },
    /// The client leaves. The server may close the connection.
    Bye,
}

/// Messages from the server to a client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome {
        protocol: u32,
        /// The sequence number of the last entry in the log.
        head_seq: u64,
        /// The highest `op_id` of this user that the server has sequenced (0 if none). The
        /// client drops its own transactions up to this number from its queue.
        your_last_op: u64,
        /// The other users who are online now.
        peers: Vec<Peer>,
    },
    /// The whole state at a sequence number. It follows `welcome` when the client has no state
    /// or is too far behind.
    Snapshot { seq: u64, state: Replica },
    /// The entries after the `last_seq` of the client, in order. Maybe empty. It follows
    /// `welcome` when the client can catch up.
    Entries { entries: Vec<Entry> },
    /// The transaction `op_id` of this client has the sequence number `seq`.
    Ack { op_id: u64, seq: u64 },
    /// An entry of another user.
    Broadcast { entry: Entry },
    /// The presence of another user.
    Presence {
        user: Uid,
        name: String,
        state: PresenceState,
    },
    /// Another user went offline.
    PresenceLeave { user: Uid },
    Error {
        code: ErrorCode,
        message: String,
        /// The transaction that was refused, if the error is about one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op_id: Option<u64>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op::{Kind, Target};

    fn round<T: Serialize + for<'a> Deserialize<'a> + PartialEq + std::fmt::Debug>(
        v: &T,
    ) -> String {
        let text = serde_json::to_string(v).unwrap();
        assert_eq!(&serde_json::from_str::<T>(&text).unwrap(), v);
        text
    }

    #[test]
    fn every_message_round_trips() {
        let user = Uid::from_num(5);
        let op = Op::Delete {
            target: Target::new(Kind::Object, Uid::from_num(9)),
        };
        let entry = Entry {
            seq: 4,
            author: user,
            op_id: 2,
            ops: vec![op.clone()],
        };
        assert_eq!(entry.txn().op_id, 2);
        let t = round(&ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            user,
            name: "Ann".into(),
            token: "t".into(),
            last_seq: Some(3),
        });
        assert!(t.contains(r#""type":"hello""#));
        round(&ClientMsg::Hello {
            protocol: 1,
            user,
            name: "Ann".into(),
            token: String::new(),
            last_seq: None,
        });
        round(&ClientMsg::Submit {
            op_id: 1,
            ops: vec![op],
        });
        round(&ClientMsg::Presence {
            state: PresenceState::default(),
        });
        round(&ClientMsg::Bye);
        round(&ServerMsg::Welcome {
            protocol: 1,
            head_seq: 9,
            your_last_op: 3,
            peers: vec![Peer {
                user,
                name: "Bo".into(),
                state: PresenceState::default(),
            }],
        });
        round(&ServerMsg::Snapshot {
            seq: 9,
            state: Replica::new(),
        });
        round(&ServerMsg::Entries {
            entries: vec![entry.clone()],
        });
        round(&ServerMsg::Ack { op_id: 1, seq: 2 });
        round(&ServerMsg::Broadcast { entry });
        round(&ServerMsg::Presence {
            user,
            name: "Bo".into(),
            state: PresenceState::default(),
        });
        round(&ServerMsg::PresenceLeave { user });
        let t = round(&ServerMsg::Error {
            code: ErrorCode::DuplicateUid,
            message: "x".into(),
            op_id: Some(3),
        });
        assert!(t.contains("duplicate_uid"));
        round(&ServerMsg::Error {
            code: ErrorCode::Internal,
            message: "x".into(),
            op_id: None,
        });
    }

    #[test]
    fn fatal_codes() {
        assert!(ErrorCode::AuthFailed.is_fatal());
        assert!(ErrorCode::UnsupportedProtocol.is_fatal());
        assert!(!ErrorCode::RateLimited.is_fatal());
        assert!(!ErrorCode::InvalidTxn.is_fatal());
        assert!(!ErrorCode::BadMessage.is_fatal());
    }
}
