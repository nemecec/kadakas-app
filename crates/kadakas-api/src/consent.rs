//! The consent tokens of `file.save` (DOC-21).
//!
//! A refusal that asks the person (`parts_missing`, `file_changed`) carries a token. The agent
//! passes it back (`allow_missing`, `allow_overwrite`) after the person agreed. The token is an
//! HMAC-SHA-256 with a random secret of the session over the kind, the facts of the refusal and a
//! nonce, so nobody can compute it without a refusal. The secret and the nonces come from the
//! operating system (`getrandom`); if the system gives none, the session gives no token and no
//! consent can be given. A token is good for one save: it is used up when it is redeemed, it is
//! dropped after a save, and it stops matching after any edit of the document or when the open
//! file is another one.

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
    source: u64,
}

/// The secret of a session and the tokens that are alive.
#[derive(Debug)]
pub(crate) struct ConsentState {
    /// `None`: the system gave no random bytes, so no token is given.
    secret: Option<[u8; 32]>,
    issued: Vec<Issued>,
}

impl Default for ConsentState {
    fn default() -> Self {
        ConsentState {
            secret: random_bytes::<32>(),
            issued: Vec::new(),
        }
    }
}

/// `N` random bytes from the operating system (`getrandom`), or `None` if it has none. Nothing
/// else is used: a secret that is not random would make the tokens guessable.
fn random_bytes<const N: usize>() -> Option<[u8; N]> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).ok()?;
    Some(out)
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
    /// The state of the document and of the open file: a token is good only while it is the same.
    /// The file is named by a number of its source (`FileSource::id`), not by an address.
    fn consent_mark(&self) -> (u64, u64) {
        (
            self.editor.revision(),
            self.file_source.as_ref().map_or(0, |f| f.id()),
        )
    }

    fn consent_token(&self, kind: &str, facts: &Value, nonce: &[u8; 16]) -> Option<String> {
        let secret = self.consent.secret.as_ref()?;
        let mut m = format!("{kind}:{facts}:").into_bytes();
        m.extend_from_slice(nonce);
        Some(hmac_hex(secret, &m))
    }

    /// Gives the refusal its token (`details.consent_token`). Any error that is not a refusal that
    /// asks the person is returned as it is. A session keeps the last few tokens alive (a client
    /// may ask about two paths before it decides); each is good once. If the system has no random
    /// bytes, the refusal has no token.
    pub(crate) fn issue_consent(&mut self, mut e: RpcError) -> RpcError {
        const ALIVE: usize = 8;
        let Some((kind, facts)) = kind_and_facts(&e) else {
            return e;
        };
        let Some(nonce) = random_bytes::<16>() else {
            return e;
        };
        let Some(token) = self.consent_token(kind, &facts, &nonce) else {
            return e;
        };
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
            i.kind == kind
                && self
                    .consent_token(kind, &facts, &i.nonce)
                    .is_some_and(|t| same(&t, token))
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
    fn the_secret_comes_from_the_system_and_differs_in_each_session() {
        let (a, b) = (ConsentState::default(), ConsentState::default());
        assert!(a.secret.is_some() && b.secret.is_some());
        assert_ne!(a.secret, b.secret);
    }

    #[test]
    fn a_session_without_a_secret_gives_no_token() {
        let mut s = Session::with_sequential_ids();
        s.consent.secret = None;
        let e = crate::review_api::missing_error(&kadakas_file::Missing {
            file: "a.kad".into(),
            pictures: vec!["ab".repeat(32)],
            ..kadakas_file::Missing::default()
        });
        let e = s.issue_consent(e);
        assert!(e.details.as_ref().unwrap().get("consent_token").is_none());
        assert!(!s.redeem_consent(&e, "anything"));
    }
}
