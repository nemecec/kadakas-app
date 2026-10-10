//! The public keys that the launcher trusts.
//!
//! The launcher holds public keys only. Two keys are allowed at the same time. This is for key
//! rotation: ship a launcher that trusts the old and the new key, switch CI to the new key, and
//! later ship a launcher that trusts the new key only. The private keys live in CI secrets.

use crate::error::{Result, UpdateError};
use crate::hex;
use ed25519_dalek::{Signature, VerifyingKey};

/// The most keys that a launcher may trust.
pub const MAX_KEYS: usize = 2;

#[derive(Clone, Debug)]
pub struct TrustedKeys {
    keys: Vec<VerifyingKey>,
}

impl TrustedKeys {
    /// From hex texts of 32-byte public keys: one or two keys.
    pub fn new(hex_keys: &[&str]) -> Result<TrustedKeys> {
        if hex_keys.is_empty() || hex_keys.len() > MAX_KEYS {
            return Err(UpdateError::BadKey(format!(
                "a launcher trusts 1 to {MAX_KEYS} keys, not {}",
                hex_keys.len()
            )));
        }
        let mut keys = Vec::new();
        for h in hex_keys {
            let bytes: [u8; 32] = hex::decode(h)?;
            let key =
                VerifyingKey::from_bytes(&bytes).map_err(|e| UpdateError::BadKey(e.to_string()))?;
            keys.push(key);
        }
        Ok(TrustedKeys { keys })
    }

    /// From the text of a key list: one hex key per line. Empty lines and `#` comments are ignored.
    pub fn parse_list(text: &str) -> Result<TrustedKeys> {
        let lines: Vec<&str> = text
            .lines()
            .map(|l| l.split('#').next().unwrap_or("").trim())
            .filter(|l| !l.is_empty())
            .collect();
        TrustedKeys::new(&lines)
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Passes when any trusted key made the signature.
    pub fn verify(&self, message: &[u8], signature: &[u8; 64]) -> Result<()> {
        let sig = Signature::from_bytes(signature);
        if self
            .keys
            .iter()
            .any(|k| k.verify_strict(message, &sig).is_ok())
        {
            Ok(())
        } else {
            Err(UpdateError::BadSignature)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Channel, SignedManifest, tests::sample};
    use crate::sign;

    fn pub_hex(seed: u8) -> String {
        sign::public_hex(&sign::key_from_seed([seed; 32]))
    }

    #[test]
    fn counts_and_limits() {
        assert!(TrustedKeys::new(&[]).is_err());
        let (a, b, c) = (pub_hex(1), pub_hex(2), pub_hex(3));
        assert!(TrustedKeys::new(&[&a, &b, &c]).is_err());
        let k = TrustedKeys::new(&[&a, &b]).unwrap();
        assert_eq!(k.len(), 2);
        assert!(!k.is_empty());
        assert!(TrustedKeys::new(&["00"]).is_err());
        // 32 bytes that are not a point on the curve.
        assert!(TrustedKeys::new(&[&"02".repeat(32)]).is_err());
    }

    #[test]
    fn list_text() {
        let text = format!("# keys\n{}  # old\n\n{}\n", pub_hex(1), pub_hex(2));
        assert_eq!(TrustedKeys::parse_list(&text).unwrap().len(), 2);
        assert!(TrustedKeys::parse_list("# nothing\n").is_err());
    }

    #[test]
    fn key_rotation() {
        let old = sign::key_from_seed([1; 32]);
        let new = sign::key_from_seed([2; 32]);
        let other = sign::key_from_seed([3; 32]);
        let m = sample("2.0.0", Channel::Stable);
        let by = |k| -> SignedManifest { sign::sign_manifest(&m, k) };

        // Before the rotation: the launcher trusts the old key only.
        let before = TrustedKeys::new(&[&sign::public_hex(&old)]).unwrap();
        assert!(by(&old).verify(&before).is_ok());
        assert!(by(&new).verify(&before).is_err());

        // During the rotation: the launcher trusts both. CI may sign with either.
        let both = TrustedKeys::new(&[&sign::public_hex(&old), &sign::public_hex(&new)]).unwrap();
        assert!(by(&old).verify(&both).is_ok());
        assert!(by(&new).verify(&both).is_ok());
        assert!(by(&other).verify(&both).is_err());

        // After the rotation: the old key is dropped, and its manifests no longer pass.
        let after = TrustedKeys::new(&[&sign::public_hex(&new)]).unwrap();
        assert!(by(&new).verify(&after).is_ok());
        assert!(by(&old).verify(&after).is_err());
    }
}
