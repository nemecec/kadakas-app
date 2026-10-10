//! Signing, for CI and for tests. The launcher is built without this module (feature `sign`).
//!
//! The private key is a 32-byte seed. The tool `sign-manifest` reads it from a file or an
//! environment variable and passes it here.

use crate::error::Result;
use crate::hex;
use crate::manifest::{Manifest, SIGN_PREFIX, SignedManifest};
use ed25519_dalek::{Signer, SigningKey};

/// The key for a 32-byte seed.
pub fn key_from_seed(seed: [u8; 32]) -> SigningKey {
    SigningKey::from_bytes(&seed)
}

/// The key for a seed in hex (whitespace around it is fine).
pub fn key_from_hex(text: &str) -> Result<SigningKey> {
    Ok(key_from_seed(hex::decode::<32>(text)?))
}

/// The public half, as hex. This is what goes into the launcher.
pub fn public_hex(key: &SigningKey) -> String {
    hex::encode(&key.verifying_key().to_bytes())
}

fn signature_hex(payload: &str, key: &SigningKey) -> String {
    let mut message = Vec::with_capacity(SIGN_PREFIX.len() + payload.len());
    message.extend_from_slice(SIGN_PREFIX);
    message.extend_from_slice(payload.as_bytes());
    hex::encode(&key.sign(&message).to_bytes())
}

/// Signs a manifest.
pub fn sign_manifest(manifest: &Manifest, key: &SigningKey) -> SignedManifest {
    let payload = manifest.to_payload();
    let signature = signature_hex(&payload, key);
    SignedManifest { payload, signature }
}

/// Signs the payload text of an envelope again (it keeps the payload as it is).
pub fn resign(envelope: &mut SignedManifest, key: &SigningKey) {
    envelope.signature = signature_hex(&envelope.payload, key);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_from_text() {
        let k = key_from_hex(&format!(" {} ", "0a".repeat(32))).unwrap();
        assert_eq!(public_hex(&k), public_hex(&key_from_seed([10; 32])));
        assert!(key_from_hex("0a").is_err());
    }
}
