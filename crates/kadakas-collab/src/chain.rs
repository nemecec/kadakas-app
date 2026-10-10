//! The hash chain of a log.
//!
//! Each entry of the log has a *chain hash* that depends on the entry and on the chain hash before
//! it, like the blocks of a block chain. Two logs that agree on the chain hash at a sequence
//! number have the same entries up to there. A client that keeps the chain hash at its last
//! sequence number can tell the server in `hello` (`last_hash`), and the server can see that its
//! log is not the log of the client, even when the log has the same name and a head that looks
//! right (a restore from a backup). Then the server gives the log a new name (`log_id`, section
//! 4.9 of `docs/collaboration.md`).
//!
//! ```text
//! chain_0 = 32 zero bytes
//! chain_n = SHA256(chain_{n-1} || seq || author || op_id || ops_text)
//! ```
//!
//! The byte layout, with no separators:
//!
//! - `chain_{n-1}`: the 32 bytes of the hash before;
//! - `seq`: 8 bytes, big endian;
//! - `author`: the client ID as text (lowercase, with hyphens, 36 bytes);
//! - `op_id`: 8 bytes, big endian;
//! - `ops_text`: the compact JSON text of the operations (UTF-8), as [`ops_text`] writes it.
//!
//! The chain is optional in the protocol: old servers and old clients leave it out.

use kadakas_core::uid::Uid;

use crate::op::Op;

/// The chain hash before the first entry.
pub const CHAIN_START: [u8; 32] = [0; 32];

/// The text of the operations that goes into the chain: the compact JSON of the list, with the
/// fields of each operation in the order of the type and the keys of the values sorted. A server
/// that parses and writes the operations again with `kadakas-collab` gets the same text. (The
/// crate does not use the `preserve_order` feature of `serde_json`, which would sort nothing.)
pub fn ops_text(ops: &[Op]) -> String {
    serde_json::to_string(ops).unwrap_or_default()
}

/// The chain hash of an entry, from the hash before it.
pub fn chain_next(prev: &[u8; 32], seq: u64, author: Uid, op_id: u64, ops: &[Op]) -> [u8; 32] {
    chain_next_text(prev, seq, author, op_id, &ops_text(ops))
}

/// Like [`chain_next`], for a server that has the text of the operations already.
pub fn chain_next_text(
    prev: &[u8; 32],
    seq: u64,
    author: Uid,
    op_id: u64,
    ops_text: &str,
) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(prev);
    h.update(seq.to_be_bytes());
    h.update(author.to_string().as_bytes());
    h.update(op_id.to_be_bytes());
    h.update(ops_text.as_bytes());
    h.finalize().into()
}

/// A chain hash as lowercase hex (the form in the messages).
pub fn to_hex(chain: &[u8; 32]) -> String {
    chain.iter().map(|b| format!("{b:02x}")).collect()
}

/// A chain hash from its hex form, or `None` if the text is not 64 hex digits.
pub fn from_hex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::op::{Kind, Target};
    use serde_json::json;

    #[test]
    fn the_layout_is_pinned() {
        let author = Uid::from_num(9);
        let ops = vec![Op::Delete {
            target: Target::new(Kind::Object, Uid::from_num(5)),
        }];
        let text = r#"[{"op":"delete","target":{"kind":"object","id":"00000000-0000-0000-0000-000000000005"}}]"#;
        assert_eq!(ops_text(&ops), text);
        // Built by hand from the layout in the module documentation.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&CHAIN_START);
        bytes.extend_from_slice(&1u64.to_be_bytes());
        bytes.extend_from_slice(b"00000000-0000-0000-0000-000000000009");
        bytes.extend_from_slice(&7u64.to_be_bytes());
        bytes.extend_from_slice(text.as_bytes());
        let by_hand = crate::blob::sha256_hex(&bytes);
        let got = chain_next(&CHAIN_START, 1, author, 7, &ops);
        assert_eq!(to_hex(&got), by_hand);
        assert_eq!(
            to_hex(&chain_next_text(&CHAIN_START, 1, author, 7, text)),
            by_hand
        );
        // The value is part of the protocol: it must not change.
        assert_eq!(
            by_hand,
            "766a6e3164baa2148ab25bd0c07718bec07cced53df0fd8c8dc61edad237bb6b"
        );
        // Another entry, another chain; the chain depends on the one before.
        let second = chain_next(&got, 2, author, 8, &ops);
        assert_ne!(second, got);
        assert_ne!(second, chain_next(&CHAIN_START, 2, author, 8, &ops));
    }

    #[test]
    fn the_text_does_not_depend_on_the_order_of_keys() {
        // `serde_json` must keep sorted keys here (no `preserve_order` anywhere in the build).
        assert_eq!(
            serde_json::to_string(&json!({"b": 1, "a": 2})).unwrap(),
            r#"{"a":2,"b":1}"#
        );
    }

    #[test]
    fn hex_round_trips() {
        let c = chain_next(&CHAIN_START, 1, Uid::from_num(1), 1, &[]);
        assert_eq!(from_hex(&to_hex(&c)), Some(c));
        assert_eq!(from_hex("abc"), None);
        assert_eq!(from_hex(&"zz".repeat(32)), None);
        assert_eq!(from_hex(&"é".repeat(32)), None);
    }
}
