//! The update manifest and its signed envelope.
//!
//! A manifest says which version is current on a channel and, for each system, where the archive
//! is, how large it is and what its SHA-256 is. CI signs the manifest with an ed25519 key. The
//! launcher checks the signature before it trusts any field.
//!
//! The envelope is a small JSON file:
//!
//! ```json
//! { "payload": "{\"version\":\"1.2.0\", ...}", "signature": "<128 hex characters>" }
//! ```
//!
//! The signature covers the exact bytes of `payload` (with a fixed prefix, see [`SIGN_PREFIX`]).
//! So no JSON re-writing can change what was signed.

use crate::error::{Result, UpdateError};
use crate::hex;
use crate::keys::TrustedKeys;
use crate::version::Version;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Put before the payload when it is signed. A signature for a manifest is then useless for any
/// other kind of message.
pub const SIGN_PREFIX: &[u8] = b"kadakas-update-manifest-v1\n";

/// The largest envelope that the launcher reads.
pub const MAX_ENVELOPE_BYTES: u64 = 1024 * 1024;

/// The largest archive that a manifest may offer.
pub const MAX_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Beta,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
        }
    }

    pub fn parse(text: &str) -> Option<Channel> {
        match text.trim() {
            "stable" => Some(Channel::Stable),
            "beta" => Some(Channel::Beta),
            _ => None,
        }
    }
}

/// One downloadable archive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformFile {
    pub url: String,
    pub size: u64,
    /// Lower-case hex SHA-256 of the archive.
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: Version,
    pub channel: Channel,
    /// Key: the system, as `os-arch`, for example `macos-aarch64`, `windows-x86_64`, `linux-x86_64`.
    pub platforms: BTreeMap<String, PlatformFile>,
}

impl Manifest {
    /// Checks every field. A manifest from [`SignedManifest::verify`] has passed this already.
    pub fn validate(&self) -> Result<()> {
        if self.platforms.is_empty() {
            return Err(UpdateError::Manifest("no platforms".into()));
        }
        for (name, f) in &self.platforms {
            let bad = |why: &str| UpdateError::Manifest(format!("{name}: {why}"));
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Err(UpdateError::Manifest(format!("bad platform name {name:?}")));
            }
            if f.size == 0 || f.size > MAX_ARCHIVE_BYTES {
                return Err(bad("size out of range"));
            }
            let h = f.sha256.as_bytes();
            if h.len() != 64 || !h.iter().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
                return Err(bad("sha256 must be 64 lower-case hex characters"));
            }
            crate::http::check_url(&f.url).map_err(|_| bad("url must be https"))?;
        }
        Ok(())
    }

    /// The file for one system.
    pub fn file_for(&self, platform: &str) -> Result<&PlatformFile> {
        self.platforms
            .get(platform)
            .ok_or_else(|| UpdateError::NoPlatform(platform.to_string()))
    }

    /// The JSON text that gets signed.
    pub fn to_payload(&self) -> String {
        // A struct of strings and numbers always serializes.
        serde_json::to_string(self).expect("manifest serializes")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedManifest {
    pub payload: String,
    /// Hex of the 64-byte ed25519 signature.
    pub signature: String,
}

impl SignedManifest {
    /// Reads the envelope. This does not check the signature.
    pub fn parse(bytes: &[u8]) -> Result<SignedManifest> {
        if bytes.len() as u64 > MAX_ENVELOPE_BYTES {
            return Err(UpdateError::Manifest("the manifest is too large".into()));
        }
        serde_json::from_slice(bytes).map_err(|e| UpdateError::Manifest(e.to_string()))
    }

    /// Checks the signature against the trusted keys, then reads and checks the manifest.
    /// The payload is not parsed before the signature is good.
    pub fn verify(&self, keys: &TrustedKeys) -> Result<Manifest> {
        let sig: [u8; 64] = hex::decode(&self.signature)?;
        let mut message = Vec::with_capacity(SIGN_PREFIX.len() + self.payload.len());
        message.extend_from_slice(SIGN_PREFIX);
        message.extend_from_slice(self.payload.as_bytes());
        keys.verify(&message, &sig)?;
        let manifest: Manifest = serde_json::from_str(&self.payload)
            .map_err(|e| UpdateError::Manifest(e.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("envelope serializes")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::sign;

    pub fn sample(version: &str, channel: Channel) -> Manifest {
        let mut platforms = BTreeMap::new();
        platforms.insert(
            "linux-x86_64".to_string(),
            PlatformFile {
                url: "https://example.org/kadakas-linux.tar.gz".into(),
                size: 10,
                sha256: "ab".repeat(32),
            },
        );
        Manifest {
            version: Version::parse(version).unwrap(),
            channel,
            platforms,
        }
    }

    #[test]
    fn channel_text() {
        assert_eq!(Channel::Beta.as_str(), "beta");
        assert_eq!(Channel::Stable.as_str(), "stable");
        assert_eq!(Channel::parse(" beta "), Some(Channel::Beta));
        assert_eq!(Channel::parse("stable"), Some(Channel::Stable));
        assert_eq!(Channel::parse("nightly"), None);
        assert_eq!(Channel::default(), Channel::Stable);
    }

    #[test]
    fn signed_manifest_round_trip() {
        let key = sign::key_from_seed([7; 32]);
        let m = sample("1.2.0", Channel::Stable);
        let signed = sign::sign_manifest(&m, &key);
        let again = SignedManifest::parse(signed.to_json().as_bytes()).unwrap();
        let keys = TrustedKeys::new(&[&sign::public_hex(&key)]).unwrap();
        assert_eq!(again.verify(&keys).unwrap(), m);
        assert_eq!(m.file_for("linux-x86_64").unwrap().size, 10);
        assert!(matches!(
            m.file_for("macos-aarch64"),
            Err(UpdateError::NoPlatform(_))
        ));
    }

    #[test]
    fn tampered_payload_fails() {
        let key = sign::key_from_seed([7; 32]);
        let keys = TrustedKeys::new(&[&sign::public_hex(&key)]).unwrap();
        let signed = sign::sign_manifest(&sample("1.2.0", Channel::Stable), &key);
        // The attacker changes the URL, the hash, the size, the version or the channel.
        for (from, to) in [
            ("example.org", "evil.example"),
            (&"ab".repeat(32), &"cd".repeat(32)),
            ("\"size\":10", "\"size\":11"),
            ("1.2.0", "9.9.9"),
            ("stable", "beta"),
        ] {
            let mut t = signed.clone();
            assert!(t.payload.contains(from), "{from}");
            t.payload = t.payload.replace(from, to);
            assert!(
                matches!(t.verify(&keys), Err(UpdateError::BadSignature)),
                "{from}"
            );
        }
    }

    #[test]
    fn tampered_signature_fails() {
        let key = sign::key_from_seed([7; 32]);
        let keys = TrustedKeys::new(&[&sign::public_hex(&key)]).unwrap();
        let mut signed = sign::sign_manifest(&sample("1.2.0", Channel::Stable), &key);
        let first = signed.signature.remove(0);
        signed
            .signature
            .insert(0, if first == '0' { '1' } else { '0' });
        assert!(matches!(
            signed.verify(&keys),
            Err(UpdateError::BadSignature)
        ));
        signed.signature = "zz".into();
        assert!(matches!(signed.verify(&keys), Err(UpdateError::BadKey(_))));
    }

    #[test]
    fn a_signature_for_other_bytes_fails() {
        // The prefix keeps a signature of the bare payload from working.
        use ed25519_dalek::Signer;
        let key = sign::key_from_seed([7; 32]);
        let keys = TrustedKeys::new(&[&sign::public_hex(&key)]).unwrap();
        let m = sample("1.2.0", Channel::Stable);
        let payload = m.to_payload();
        let bare = key.sign(payload.as_bytes());
        let forged = SignedManifest {
            payload,
            signature: hex::encode(&bare.to_bytes()),
        };
        assert!(matches!(
            forged.verify(&keys),
            Err(UpdateError::BadSignature)
        ));
    }

    #[test]
    fn signed_but_invalid_content_is_refused() {
        let key = sign::key_from_seed([7; 32]);
        let keys = TrustedKeys::new(&[&sign::public_hex(&key)]).unwrap();
        let sign_payload = |payload: String| {
            let mut m = sign::sign_manifest(&sample("1.0.0", Channel::Stable), &key);
            m.payload = payload;
            sign::resign(&mut m, &key);
            m
        };
        // Not JSON.
        assert!(matches!(
            sign_payload("nope".into()).verify(&keys),
            Err(UpdateError::Manifest(_))
        ));
        let mut bad = sample("1.0.0", Channel::Stable);
        bad.platforms.get_mut("linux-x86_64").unwrap().size = 0;
        assert!(matches!(
            sign_payload(bad.to_payload()).verify(&keys),
            Err(UpdateError::Manifest(_))
        ));
    }

    #[test]
    fn validate_checks_each_field() {
        let ok = sample("1.0.0", Channel::Beta);
        assert!(ok.validate().is_ok());
        let edit = |f: &dyn Fn(&mut Manifest)| {
            let mut m = ok.clone();
            f(&mut m);
            m.validate().unwrap_err()
        };
        edit(&|m| m.platforms.clear());
        edit(&|m| {
            let f = m.platforms.remove("linux-x86_64").unwrap();
            m.platforms.insert("../x".into(), f);
        });
        edit(&|m| {
            let f = m.platforms.remove("linux-x86_64").unwrap();
            m.platforms.insert(String::new(), f);
        });
        edit(&|m| m.platforms.get_mut("linux-x86_64").unwrap().size = MAX_ARCHIVE_BYTES + 1);
        edit(&|m| m.platforms.get_mut("linux-x86_64").unwrap().sha256 = "AB".repeat(32));
        edit(&|m| m.platforms.get_mut("linux-x86_64").unwrap().sha256 = "ab".into());
        edit(&|m| m.platforms.get_mut("linux-x86_64").unwrap().url = "http://example.org/a".into());
        edit(&|m| m.platforms.get_mut("linux-x86_64").unwrap().url = "file:///etc/passwd".into());
    }

    #[test]
    fn big_envelope_is_refused() {
        let big = vec![b' '; MAX_ENVELOPE_BYTES as usize + 1];
        assert!(matches!(
            SignedManifest::parse(&big),
            Err(UpdateError::Manifest(_))
        ));
        assert!(SignedManifest::parse(b"{}").is_err());
    }
}
