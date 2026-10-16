//! The consent tokens of `file.save` (DOC-21).
//!
//! A refusal that asks the person (`parts_missing`, `file_changed`) carries a token. The agent
//! passes it back (`allow_missing`, `allow_overwrite`) after the person agreed. The token is an
//! HMAC-SHA-256 with a random secret of the session over the kind, the facts of the refusal and a
//! nonce, so nobody can compute it without a refusal. It is good for one save: it is used up when
//! it is redeemed, it is dropped after a save, and it stops matching after any edit of the
//! document or when the open file is another one.

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::errors::{REASON_FILE_CHANGED, REASON_PARTS_MISSING};
use crate::{RpcError, Session};

/// The token that was given for one refusal, and what it is good for.
#[derive(Clone, Debug)]
struct Issued {
    kind: &'static str,
    nonce: [u8; 16],
    /// The revision of the document and the open file when the token was given.
    revision: u64,
    source: usize,
}

/// The secret of a session and the tokens that are alive.
#[derive(Debug)]
pub(crate) struct ConsentState {
    secret: [u8; 32],
    issued: Vec<Issued>,
}

impl Default for ConsentState {
    fn default() -> Self {
        ConsentState {
            secret: random_bytes(),
            issued: Vec::new(),
        }
    }
}

/// 32 random bytes: the operating system's source where there is one, else the random keys that
/// the standard library seeds its hash tables with (and the time).
fn random_bytes() -> [u8; 32] {
    #[cfg(unix)]
    {
        use std::io::Read;
        let mut out = [0u8; 32];
        if let Ok(mut f) = std::fs::File::open("/dev/urandom")
            && f.read_exact(&mut out).is_ok()
        {
            return out;
        }
    }
    use std::hash::{BuildHasher, Hasher};
    let mut out = [0u8; 32];
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    for (i, chunk) in out.chunks_mut(8).enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(now.wrapping_add(i as u64));
        chunk.copy_from_slice(&h.finish().to_le_bytes());
    }
    out
}

/// HMAC-SHA-256 (RFC 2104) as a hex string.
fn hmac_hex(key: &[u8], message: &[u8]) -> String {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Compares two strings without stopping at the first difference.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// The kind and the facts of a refusal that asks the person, or `None` for any other error.
fn kind_and_facts(e: &RpcError) -> Option<(&'static str, Value)> {
    let d = e.details.as_ref()?;
    match e.reason {
        Some(REASON_PARTS_MISSING) => Some(("parts_missing", d["missing"].clone())),
        Some(REASON_FILE_CHANGED) => Some(("file_changed", serde_json::json!({"file": d["file"]}))),
        _ => None,
    }
}

impl Session {
    fn consent_mark(&self) -> (u64, usize) {
        (
            self.editor.revision(),
            self.file_source
                .as_ref()
                .map_or(0, |f| std::sync::Arc::as_ptr(f) as usize),
        )
    }

    fn consent_token(&self, kind: &str, facts: &Value, nonce: &[u8; 16]) -> String {
        let mut m = format!("{kind}:{facts}:").into_bytes();
        m.extend_from_slice(nonce);
        hmac_hex(&self.consent.secret, &m)
    }

    /// Gives the refusal its token (`details.consent_token`). Any error that is not a refusal that
    /// asks the person is returned as it is. A session keeps the last few tokens alive (a client
    /// may ask about two paths before it decides); each is good once.
    pub(crate) fn issue_consent(&mut self, mut e: RpcError) -> RpcError {
        const ALIVE: usize = 8;
        let Some((kind, facts)) = kind_and_facts(&e) else {
            return e;
        };
        let nonce: [u8; 16] = random_bytes()[..16].try_into().unwrap_or_default();
        let token = self.consent_token(kind, &facts, &nonce);
        let (revision, source) = self.consent_mark();
        self.drop_stale_consent();
        if self.consent.issued.len() >= ALIVE {
            self.consent.issued.remove(0);
        }
        self.consent.issued.push(Issued {
            kind,
            nonce,
            revision,
            source,
        });
        if let Some(Value::Object(d)) = e.details.as_mut() {
            d.insert("consent_token".into(), Value::String(token));
        }
        e
    }

    /// Drops the tokens of another state of the document or of another open file.
    fn drop_stale_consent(&mut self) {
        let mark = self.consent_mark();
        self.consent
            .issued
            .retain(|i| (i.revision, i.source) == mark);
    }

    /// True if `token` is a token that this session gave for exactly this refusal, the document
    /// is as it was and the open file is the same. A token is good once: it is used up here.
    pub(crate) fn redeem_consent(&mut self, refusal: &RpcError, token: &str) -> bool {
        let Some((kind, facts)) = kind_and_facts(refusal) else {
            return false;
        };
        self.drop_stale_consent();
        let hit = self.consent.issued.iter().position(|i| {
            i.kind == kind && same(&self.consent_token(kind, &facts, &i.nonce), token)
        });
        match hit {
            Some(pos) => {
                self.consent.issued.remove(pos);
                true
            }
            None => false,
        }
    }

    /// Drops every token (after a save, and when the document is replaced).
    pub(crate) fn clear_consent(&mut self) {
        self.consent.issued.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_matches_the_rfc_4231_vector() {
        // Test case 2 of RFC 4231.
        assert_eq!(
            hmac_hex(b"Jefe", b"what do ya want for nothing?"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // A key longer than the block is hashed first (test case 6).
        assert_eq!(
            hmac_hex(
                &[0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First"
            ),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn the_secret_is_different_in_each_session() {
        assert_ne!(
            ConsentState::default().secret,
            ConsentState::default().secret
        );
    }
}
