//! The manifest: `manifest.json`, the list of every entry with its size and SHA-256.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::ContainerError;

/// The value of `format` in the manifest.
pub const FORMAT: &str = "kadakas-container";
/// The version of the container (the layout of the manifest and the rules of the entries). The
/// drawing inside `document.json` has its own `format_version`.
pub const VERSION: u32 = 1;

/// One entry of the manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub name: String,
    /// The bytes of the entry, uncompressed.
    pub size: u64,
    /// SHA-256 of those bytes, 64 lowercase hex digits.
    pub sha256: String,
}

/// The manifest. It lists every entry but itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    /// The program that wrote the container, for people who look inside. Not used when reading.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub generator: String,
    pub entries: Vec<ManifestEntry>,
}

impl Manifest {
    /// The bytes of the manifest. The entries are in the order they were written.
    pub fn to_bytes(&self) -> Vec<u8> {
        // A manifest of plain texts and numbers always serializes.
        serde_json::to_vec_pretty(self).unwrap_or_default()
    }

    /// Reads the bytes of a manifest and checks its shape.
    pub fn parse(bytes: &[u8]) -> Result<Manifest, ContainerError> {
        let name = crate::limits::MANIFEST_NAME;
        let m: Manifest = serde_json::from_slice(bytes)
            .map_err(|e| ContainerError::entry(name, format!("it cannot be read: {e}")))?;
        if m.format != FORMAT {
            return Err(ContainerError::entry(
                name,
                format!("the format is \"{}\", not \"{FORMAT}\"", m.format),
            ));
        }
        if m.format_version == 0 || m.format_version > VERSION {
            return Err(ContainerError::entry(
                name,
                format!(
                    "the container version {} is not known to this program (it reads version {VERSION}); update Kadakas to open this file",
                    m.format_version
                ),
            ));
        }
        Ok(m)
    }
}

/// SHA-256 as 64 lowercase hex digits.
pub fn hex(digest: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// SHA-256 of some bytes as hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The bytes of a hex hash, or `None` if the text is not 64 lowercase hex digits.
pub fn parse_hash(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, pair) in text.as_bytes().chunks(2).enumerate() {
        let d = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        };
        out[i] = d(pair[0])? << 4 | d(pair[1])?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_text_round_trips() {
        let h = sha256_hex(b"abc");
        assert_eq!(
            h,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hex(&parse_hash(&h).unwrap()), h);
        assert!(parse_hash(&h.to_uppercase()).is_none());
        assert!(parse_hash("ab").is_none());
        assert!(parse_hash(&"g".repeat(64)).is_none());
    }

    #[test]
    fn a_manifest_is_checked() {
        let m = Manifest {
            format: FORMAT.into(),
            format_version: VERSION,
            generator: String::new(),
            entries: vec![ManifestEntry {
                name: "a".into(),
                size: 1,
                sha256: sha256_hex(b"x"),
            }],
        };
        assert_eq!(Manifest::parse(&m.to_bytes()).unwrap(), m);
        let mut other = m.clone();
        other.format = "zip".into();
        assert!(Manifest::parse(&other.to_bytes()).is_err());
        other = m.clone();
        other.format_version = VERSION + 1;
        let e = Manifest::parse(&other.to_bytes()).unwrap_err();
        assert!(e.to_string().contains("update Kadakas"));
        other.format_version = 0;
        assert!(Manifest::parse(&other.to_bytes()).is_err());
        assert!(Manifest::parse(b"{").is_err());
        assert!(Manifest::parse(b"[]").is_err());
    }
}
