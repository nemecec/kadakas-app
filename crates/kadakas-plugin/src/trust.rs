//! Which modules may run. A module runs only if the user trusted its exact bytes.
//!
//! A module is sandboxed (no files, no network, limited time and memory), so a bad module cannot
//! harm the computer. It can still draw wrong shapes or waste time, so the user approves each new
//! module, as for Python scripts. The approval is for the SHA-256 hash of the module file. A new
//! build of the plug-in has a new hash and needs a new approval.
//!
//! The store is a small JSON file in the config folder: hash -> name of the plug-in when it was
//! trusted. The same rule as for scripts holds: the user decides in a dialog of the app, and no
//! API call adds trust. Agents can run plug-ins only when the host allows it (`--allow-agent-scripts`).

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

/// The SHA-256 of a module, as lower case hex.
pub fn hash(wasm: &[u8]) -> String {
    Sha256::digest(wasm)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The right to run one exact module. Only [`ModuleTrust::permit`] makes one, and
/// [`crate::plugin::Plugin::reset`] refuses to run without one that fits the module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Permit {
    hash: String,
}

impl Permit {
    /// True if this permit is for the module with this hash.
    pub fn allows(&self, module_hash: &str) -> bool {
        self.hash == module_hash
    }

    /// A permit for any module, for tests.
    #[cfg(test)]
    pub fn for_test(module_hash: &str) -> Permit {
        Permit {
            hash: module_hash.to_string(),
        }
    }
}

/// The trusted module hashes of the user.
#[derive(Clone, Debug, Default)]
pub struct ModuleTrust {
    #[cfg(not(target_arch = "wasm32"))]
    path: Option<std::path::PathBuf>,
    /// hash -> name of the plug-in when it was trusted
    entries: BTreeMap<String, String>,
}

/// The trust file of the user, in the config folder.
#[cfg(not(target_arch = "wasm32"))]
pub fn default_path() -> Option<std::path::PathBuf> {
    Some(kadakas_sys::dirs::config_dir()?.join("trusted_wasm_plugins.json"))
}

impl ModuleTrust {
    /// A store in memory only. Nothing is trusted.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// The store in a file. A missing or broken file is an empty store.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(path: std::path::PathBuf) -> Self {
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<BTreeMap<String, String>>(&t).ok())
            .unwrap_or_default();
        ModuleTrust {
            path: Some(path),
            entries,
        }
    }

    /// The store of the user in the config folder.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_default() -> Self {
        default_path().map_or_else(Self::in_memory, Self::load)
    }

    /// True if the user trusted the module with this hash.
    pub fn is_trusted(&self, module_hash: &str) -> bool {
        self.entries.contains_key(module_hash)
    }

    /// The right to run this module, if it is trusted or `allow_all` is set (the host lets agents
    /// run plug-ins). `None` means the module must not run.
    pub fn permit(&self, module_hash: &str, allow_all: bool) -> Option<Permit> {
        (allow_all || self.is_trusted(module_hash)).then(|| Permit {
            hash: module_hash.to_string(),
        })
    }

    /// Trusts the module with this hash. Saves the file when the store has one.
    pub fn trust(&mut self, module_hash: &str, name: &str) -> std::io::Result<()> {
        self.entries
            .insert(module_hash.to_string(), name.to_string());
        self.save()
    }

    /// Removes the trust of a module. Saves the file when the store has one.
    pub fn forget(&mut self, module_hash: &str) -> std::io::Result<()> {
        self.entries.remove(module_hash);
        self.save()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(&self.entries).expect("json");
        kadakas_sys::fsio::write_private(path, text.as_bytes())
    }

    #[cfg(target_arch = "wasm32")]
    fn save(&self) -> std::io::Result<()> {
        Ok(())
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
            hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn nothing_is_trusted_at_first_and_the_flag_opens_everything() {
        let mut t = ModuleTrust::in_memory();
        assert!(t.is_empty() && !t.is_trusted("aa"));
        assert!(t.permit("aa", false).is_none());
        let p = t.permit("aa", true).unwrap();
        assert!(p.allows("aa") && !p.allows("bb"));
        t.trust("aa", "Name").unwrap();
        assert!(t.is_trusted("aa") && t.len() == 1);
        assert!(t.permit("aa", false).is_some());
        t.forget("aa").unwrap();
        assert!(!t.is_trusted("aa"));
        let _ = Permit::for_test("x");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn trust_is_saved_and_tied_to_the_exact_bytes() {
        let dir = std::env::temp_dir().join(format!("kadakas-wasm-trust-{}", std::process::id()));
        let path = dir.join("sub").join("trusted.json");
        let mut t = ModuleTrust::load(path.clone());
        assert!(t.is_empty());
        let h = hash(b"module one");
        t.trust(&h, "One").unwrap();
        // One changed byte is another module.
        assert!(!t.is_trusted(&hash(b"module onf")));
        let mut again = ModuleTrust::load(path.clone());
        assert!(again.is_trusted(&h) && again.len() == 1);
        again.trust(&hash(b"two"), "Two").unwrap();
        let reread = ModuleTrust::load(path.clone());
        assert!(reread.len() == 2);
        let mut gone = ModuleTrust::load(path.clone());
        gone.forget(&h).unwrap();
        let left = ModuleTrust::load(path.clone());
        assert!(!left.is_trusted(&h) && left.len() == 1);
        // A broken file is an empty store, not a crash.
        std::fs::write(&path, "not json").unwrap();
        assert!(ModuleTrust::load(path).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
        let _ = default_path();
        let _ = ModuleTrust::load_default();
    }
}
