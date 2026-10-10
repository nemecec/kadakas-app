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
    /// The client instance that wrote it (the `client` of its `hello`). This is the writer.
    pub author: Uid,
    /// The user of that client, as the server knows it from the sign-in.
    pub user: Uid,
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
    /// The token is wrong or missing, or the user does not belong to the token. Ends the
    /// connection.
    AuthFailed,
    /// The message is not valid, or comes at a wrong time (before `hello`).
    BadMessage,
    /// A `create` used an ID that exists or existed.
    DuplicateUid,
    /// The transaction is empty, malformed, too deep, too large or has a value that the server
    /// refuses (see `validate`).
    InvalidTxn,
    /// The user sent too much too fast. Temporary: the client sends the transaction again.
    RateLimited,
    /// A fault in the server. Temporary: the client sends the transaction again.
    Internal,
    /// The pair (client, `op_id`) was used before with other content, or is below the highest
    /// number that the server has for this client. The client numbers the transaction again.
    OpIdReused,
    /// The document would become too large (the limit of the snapshot). Refused for good.
    DocumentFull,
    /// A code that this version does not know (a newer server). The client treats it as a retry
    /// error when it has an `op_id`, and otherwise reports it and goes on.
    #[serde(other)]
    Unknown,
}

/// What a client does when it gets an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorClass {
    /// The client cannot go on. It must not connect again by itself.
    Fatal,
    /// The server refused the transaction for good. The client drops it.
    Reject,
    /// The server could not take the transaction now. The client keeps it and sends it again
    /// after a time that grows.
    Retry,
    /// The client goes offline. At the next sign-in it gives its waiting transactions new
    /// numbers above everything the server knows, and sends them.
    Renumber,
}

impl ErrorCode {
    /// True if the connection cannot go on after this error.
    pub fn is_fatal(self) -> bool {
        self.class() == ErrorClass::Fatal
    }

    /// What a client does with a transaction that gets this error.
    pub fn class(self) -> ErrorClass {
        match self {
            ErrorCode::UnsupportedProtocol | ErrorCode::AuthFailed => ErrorClass::Fatal,
            ErrorCode::BadMessage
            | ErrorCode::DuplicateUid
            | ErrorCode::InvalidTxn
            | ErrorCode::DocumentFull => ErrorClass::Reject,
            ErrorCode::RateLimited | ErrorCode::Internal | ErrorCode::Unknown => ErrorClass::Retry,
            ErrorCode::OpIdReused => ErrorClass::Renumber,
        }
    }
}

/// Messages from a client to the server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Hello {
        protocol: u32,
        /// The person. The server must check that the token belongs to this user.
        user: Uid,
        /// The client instance: a UUID that this program made for itself. It is the writer of
        /// the transactions. The client keeps it in its journal.
        client: Uid,
        name: String,
        token: String,
        /// The last sequence number the client has, or `None` if it has no state yet.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_seq: Option<u64>,
        /// The `log_id` that the client saw last, with its state.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        log_id: Option<String>,
    },
    /// A transaction. The server answers with `ack`, or with `error` that has this `op_id`. The
    /// server must treat a second submit with the same client, `op_id` and content as the first
    /// (it answers with the old sequence number): a client sends again after a lost connection.
    Submit {
        op_id: u64,
        ops: Vec<Op>,
    },
    Presence {
        state: PresenceState,
    },
    /// A blob (see `blob`): the text of a picture. `hash` is its SHA-256 as hex. The server checks it
    /// and keeps the blob. The client sends it before the transaction that refers to it.
    BlobPut {
        hash: String,
        data: String,
    },
    /// Asks for a blob that a reference in the state points to.
    BlobGet {
        hash: String,
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
        /// The name of this log. It changes when the log starts again (a restore from a backup
        /// that lost entries, a copy of the document). A client that sees another name than
        /// the one in its state starts again from the snapshot and sends its own work again.
        log_id: String,
        /// The sequence number of the last entry in the log.
        head_seq: u64,
        /// The highest `op_id` of this client that the server has sequenced (0 if none).
        your_last_op: u64,
        /// The other clients who are online now.
        peers: Vec<Peer>,
    },
    /// The whole state at a sequence number. It follows `welcome` when the client has no state
    /// or is too far behind.
    Snapshot {
        log_id: String,
        seq: u64,
        state: Replica,
    },
    /// The entries after the `last_seq` of the client, in order. Maybe empty. It follows
    /// `welcome` when the client can catch up. A client without state must not get it.
    Entries { entries: Vec<Entry> },
    /// The transaction `op_id` of this client has the sequence number `seq`.
    Ack { op_id: u64, seq: u64 },
    /// An entry of another client.
    Broadcast { entry: Entry },
    /// The presence of another client.
    Presence {
        client: Uid,
        user: Uid,
        name: String,
        state: PresenceState,
    },
    /// Another client went offline.
    PresenceLeave { client: Uid },
    /// The answer to `blob_get`.
    Blob { hash: String, data: String },
    /// The server does not have this blob.
    BlobMissing { hash: String },
    Error {
        code: ErrorCode,
        message: String,
        /// The transaction that was refused, if the error is about one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op_id: Option<u64>,
        /// The blob that the error is about, for a refused `blob_put` (it has no `op_id`). With a
        /// retry class, the client puts this blob again before it sends the transaction again.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hash: Option<String>,
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
        let client = Uid::from_num(6);
        let op = Op::Delete {
            target: Target::new(Kind::Object, Uid::from_num(9)),
        };
        let entry = Entry {
            seq: 4,
            author: client,
            user,
            op_id: 2,
            ops: vec![op.clone()],
        };
        assert_eq!(entry.txn().op_id, 2);
        let t = round(&ClientMsg::Hello {
            protocol: PROTOCOL_VERSION,
            user,
            client,
            name: "Ann".into(),
            token: "t".into(),
            last_seq: Some(3),
            log_id: Some("log".into()),
        });
        assert!(t.contains(r#""type":"hello""#));
        round(&ClientMsg::Hello {
            protocol: 1,
            user,
            client,
            name: "Ann".into(),
            token: String::new(),
            last_seq: None,
            log_id: None,
        });
        round(&ClientMsg::Submit {
            op_id: 1,
            ops: vec![op],
        });
        round(&ClientMsg::Presence {
            state: PresenceState::default(),
        });
        round(&ClientMsg::BlobPut {
            hash: "h".into(),
            data: "d".into(),
        });
        round(&ClientMsg::BlobGet { hash: "h".into() });
        round(&ServerMsg::Blob {
            hash: "h".into(),
            data: "d".into(),
        });
        round(&ServerMsg::BlobMissing { hash: "h".into() });
        round(&ClientMsg::Bye);
        round(&ServerMsg::Welcome {
            protocol: 1,
            log_id: "log".into(),
            head_seq: 9,
            your_last_op: 3,
            peers: vec![Peer {
                client,
                user,
                name: "Bo".into(),
                state: PresenceState::default(),
            }],
        });
        round(&ServerMsg::Snapshot {
            log_id: "log".into(),
            seq: 9,
            state: Replica::new(),
        });
        round(&ServerMsg::Entries {
            entries: vec![entry.clone()],
        });
        round(&ServerMsg::Ack { op_id: 1, seq: 2 });
        round(&ServerMsg::Broadcast { entry });
        round(&ServerMsg::Presence {
            client,
            user,
            name: "Bo".into(),
            state: PresenceState::default(),
        });
        round(&ServerMsg::PresenceLeave { client });
        let t = round(&ServerMsg::Error {
            code: ErrorCode::DuplicateUid,
            message: "x".into(),
            op_id: Some(3),
            hash: Some("h".into()),
        });
        assert!(t.contains("duplicate_uid"));
        round(&ServerMsg::Error {
            code: ErrorCode::Internal,
            message: "x".into(),
            op_id: None,
            hash: None,
        });
    }

    #[test]
    fn every_error_code_has_a_class() {
        use ErrorClass::*;
        let all = [
            (ErrorCode::UnsupportedProtocol, Fatal),
            (ErrorCode::AuthFailed, Fatal),
            (ErrorCode::BadMessage, Reject),
            (ErrorCode::DuplicateUid, Reject),
            (ErrorCode::InvalidTxn, Reject),
            (ErrorCode::DocumentFull, Reject),
            (ErrorCode::RateLimited, Retry),
            (ErrorCode::Internal, Retry),
            (ErrorCode::OpIdReused, Renumber),
            (ErrorCode::Unknown, Retry),
        ];
        for (code, class) in all {
            assert_eq!(code.class(), class, "{code:?}");
            assert_eq!(code.is_fatal(), class == Fatal);
        }
    }

    #[test]
    fn a_code_from_a_newer_server_reads_as_unknown() {
        let m: ServerMsg =
            serde_json::from_str(r#"{"type":"error","code":"from_the_future","message":"x"}"#)
                .unwrap();
        assert!(matches!(
            m,
            ServerMsg::Error {
                code: ErrorCode::Unknown,
                op_id: None,
                ..
            }
        ));
        assert!(!ErrorCode::Unknown.is_fatal());
    }

    #[test]
    fn numbers_keep_every_digit_when_read_and_written_again() {
        // The crate parses with `float_roundtrip`: a server that re-serializes must not lose digits.
        for text in [
            "0.1",
            "0.30000000000000004",
            "123456.789012345",
            "1e-7",
            "5e22",
        ] {
            let v: serde_json::Value = serde_json::from_str(text).unwrap();
            let again: serde_json::Value =
                serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
            assert_eq!(v, again, "{text}");
            assert_eq!(v.as_f64(), text.parse::<f64>().ok(), "{text}");
        }
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
