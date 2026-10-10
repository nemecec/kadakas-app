//! Which plug-in scripts may run. A script runs only if the user trusted its exact text.
//!
//! The trust store keeps the SHA-256 hash of each trusted script, with the name of the plug-in object
//! for display. It is a small JSON file in the config folder of the user. The scripts that ship with
//! the app are trusted by hash. Any change to a script gives a new hash, so the script is untrusted
//! again. Nothing in the API adds trust: only the dialog of the app does.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::examples;

/// The SHA-256 of a script, as lower case hex.
pub fn hash(script: &str) -> String {
    Sha256::digest(script.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Where a script comes from, for the "Trust this plug-in?" dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Part of the app.
    Shipped,
    /// The user trusted exactly this text before.
    UserLibrary,
    /// Only this document has it. Nobody trusted it yet.
    Document,
}

impl Origin {
    pub fn name(self) -> &'static str {
        match self {
            Origin::Shipped => "shipped with the app",
            Origin::UserLibrary => "trusted by you earlier",
            Origin::Document => "this document",
        }
    }
}

/// The trusted script hashes of the user.
#[derive(Clone, Debug, Default)]
pub struct TrustStore {
    path: Option<PathBuf>,
    /// hash -> name of the plug-in object when it was trusted
    entries: BTreeMap<String, String>,
}

/// The config folder of the user, for the trust file.
pub fn default_path() -> Option<PathBuf> {
    let var = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        var("APPDATA")?
    } else if cfg!(target_os = "macos") {
        var("HOME")?.join("Library").join("Application Support")
    } else {
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config")))?
    };
    Some(base.join("cadektor").join("trusted_plugins.json"))
}

impl TrustStore {
    /// A store that lives in memory only. Shipped scripts are trusted, nothing else.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// The store in a file. A missing or broken file is an empty store.
    pub fn load(path: PathBuf) -> Self {
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<BTreeMap<String, String>>(&t).ok())
            .unwrap_or_default();
        TrustStore {
            path: Some(path),
            entries,
        }
    }

    /// The store of the user in the config folder.
    pub fn load_default() -> Self {
        default_path().map_or_else(Self::in_memory, Self::load)
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Where the script comes from. A shipped script is always `Shipped`.
    pub fn origin(&self, script: &str) -> Origin {
        if examples::is_shipped_script(script) {
            Origin::Shipped
        } else if self.entries.contains_key(&hash(script)) {
            Origin::UserLibrary
        } else {
            Origin::Document
        }
    }

    /// True if this exact script may run.
    pub fn is_trusted(&self, script: &str) -> bool {
        self.origin(script) != Origin::Document
    }

    /// Trusts this exact script text. Only the dialog of the app calls this. Saves the file.
    pub fn trust(&mut self, script: &str, name: &str) -> std::io::Result<()> {
        self.entries.insert(hash(script), name.to_string());
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(&self.entries).expect("json");
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_sha256_hex() {
        assert_eq!(
            hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn shipped_scripts_are_trusted_and_others_are_not() {
        let t = TrustStore::in_memory();
        for (_, d) in examples::all() {
            assert_eq!(t.origin(&d.script), Origin::Shipped);
            assert!(t.is_trusted(&d.script));
            // One changed character, and the script is a stranger.
            assert_eq!(t.origin(&format!("{}\n", d.script)), Origin::Document);
        }
        assert!(!t.is_trusted("def reset(p, g): return []"));
        assert!(t.is_empty() && t.len() == 0);
    }

    #[test]
    fn trust_is_saved_and_tied_to_the_exact_text() {
        let dir = std::env::temp_dir().join(format!("cadektor-trust-{}", std::process::id()));
        let path = dir.join("sub").join("trusted.json");
        let mut t = TrustStore::load(path.clone());
        assert!(t.is_empty() && t.path() == Some(path.as_path()));
        t.trust("print(1)", "Mine").unwrap();
        assert_eq!(t.origin("print(1)"), Origin::UserLibrary);
        assert!(!t.is_trusted("print(2)"));
        let again = TrustStore::load(path.clone());
        assert!(again.is_trusted("print(1)") && again.len() == 1);
        // A broken file is an empty store, not a crash.
        std::fs::write(&path, "not json").unwrap();
        assert!(TrustStore::load(path).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
        assert!(!Origin::Document.name().is_empty());
        assert!(!Origin::Shipped.name().is_empty() && !Origin::UserLibrary.name().is_empty());
        let _ = default_path();
        let _ = TrustStore::in_memory().trust("x", "n");
    }
}
