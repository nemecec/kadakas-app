//! Which modules may run. A module runs only if the user trusted its exact bytes.
//!
//! A module is sandboxed (no files, no network, limited time and memory), so a bad module cannot
//! harm the computer. It can still draw wrong shapes or waste time, so the user approves each new
//! module, as for Python scripts. The approval is for the SHA-256 hash of the module file. A new
//! build of the plug-in has a new hash and needs a new approval.
//!
//! The list is a small JSON file in the plug-in folder of the user data folder (or a blob in the
//! browser storage): hash -> name of the plug-in when it was trusted. The same rule as for scripts
//! holds: the user decides in a dialog of the app, and no API call adds trust. Agents can run plug-ins only when the host allows it (`--allow-agent-scripts`).

use std::collections::BTreeMap;
use std::sync::Arc;

use sha2::{Digest, Sha256};

#[cfg(not(target_arch = "wasm32"))]
use crate::store::FileBackend;
use crate::store::{MAX_TRUST_BYTES, MAX_TRUST_ENTRIES, StoreBackend, TRUST_FILE, parse_trust};

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
///
/// With a backend (see [`crate::store`]) the list is saved after every change. The list is read
/// with care: a broken, huge or foreign file is an empty list.
#[derive(Clone, Default)]
pub struct ModuleTrust {
    backend: Option<(Arc<dyn StoreBackend>, String)>,
    /// hash -> name of the plug-in when it was trusted
    entries: BTreeMap<String, String>,
}

impl std::fmt::Debug for ModuleTrust {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModuleTrust")
            .field("entries", &self.entries.len())
            .field("saved", &self.backend.is_some())
            .finish()
    }
}

/// The trust file of the user, in the plug-in folder of the user data folder.
#[cfg(not(target_arch = "wasm32"))]
pub fn default_path() -> Option<std::path::PathBuf> {
    Some(crate::store::default_home()?.join(TRUST_FILE))
}

impl ModuleTrust {
    /// A store in memory only. Nothing is trusted.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// A list that is kept in the blob `key` of a backend. A missing or broken blob is an empty
    /// list.
    pub fn with_backend(backend: Arc<dyn StoreBackend>, key: &str) -> Self {
        let entries = match backend.read(key, MAX_TRUST_BYTES) {
            Ok(Some(bytes)) => parse_trust(&bytes),
            _ => BTreeMap::new(),
        };
        ModuleTrust {
            backend: Some((backend, key.to_string())),
            entries,
        }
    }

    /// The list in a file. A missing or broken file is an empty list.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(path: std::path::PathBuf) -> Self {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(TRUST_FILE)
            .to_string();
        let dir = path.parent().map(std::path::Path::to_path_buf);
        Self::with_backend(Arc::new(FileBackend::new(dir.unwrap_or_default())), &name)
    }

    /// The list of the user in the plug-in folder of the user data folder.
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

    /// Trusts the module with this hash. Saves the list when the store has a backend. If the
    /// list cannot be saved, the module is still trusted for this session and the error says why
    /// the trust will not last.
    pub fn trust(&mut self, module_hash: &str, name: &str) -> std::io::Result<()> {
        if self.entries.len() >= MAX_TRUST_ENTRIES && !self.entries.contains_key(module_hash) {
            return Err(std::io::Error::other(format!(
                "the trust list has {MAX_TRUST_ENTRIES} entries. Remove one first"
            )));
        }
        self.entries
            .insert(module_hash.to_string(), name.to_string());
        self.save()
    }

    /// Removes the trust of a module. Saves the list when the store has a backend.
    pub fn forget(&mut self, module_hash: &str) -> std::io::Result<()> {
        self.entries.remove(module_hash);
        self.save()
    }

    /// True if the list is kept on a medium that outlives the session.
    pub fn persistent(&self) -> bool {
        self.backend.as_ref().is_some_and(|(b, _)| b.persistent())
    }

    fn save(&self) -> std::io::Result<()> {
        let Some((backend, key)) = &self.backend else {
            return Ok(());
        };
        let text = serde_json::to_string_pretty(&self.entries).expect("json");
        backend
            .write(key, text.as_bytes())
            .map_err(std::io::Error::other)
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
        // The path of the user list is built, but a test never reads or writes it.
        assert!(default_path().is_none_or(|p| p.ends_with("trusted_wasm_plugins.json")));
    }

    #[test]
    fn trust_can_live_in_any_backend_and_a_failed_save_is_reported() {
        let mem = Arc::new(crate::store::MemoryBackend::new());
        let mut t = ModuleTrust::with_backend(mem.clone(), "t.json");
        assert!(t.is_empty() && !t.persistent());
        let h = hash(b"one");
        t.trust(&h, "One").unwrap();
        assert!(format!("{t:?}").contains("saved"));
        // A second list on the same backend sees the entry.
        assert!(ModuleTrust::with_backend(mem.clone(), "t.json").is_trusted(&h));
        // A save that fails keeps the trust for the session and says so.
        *mem.fail_writes.lock().unwrap() = Some("quota".into());
        let two = hash(b"two");
        let e = t.trust(&two, "Two").unwrap_err();
        assert!(e.to_string().contains("quota"));
        assert!(t.is_trusted(&two));
        assert!(t.forget(&two).is_err());
        // A damaged blob is an empty list.
        mem.fail_writes.lock().unwrap().take();
        StoreBackend::write(mem.as_ref(), "bad.json", b"{").unwrap();
        assert!(ModuleTrust::with_backend(mem.clone(), "bad.json").is_empty());
        // A list over the cap takes no new entry.
        let big: BTreeMap<String, String> = (0..MAX_TRUST_ENTRIES)
            .map(|i| (hash(format!("m{i}").as_bytes()), "n".into()))
            .collect();
        StoreBackend::write(mem.as_ref(), "big.json", &serde_json::to_vec(&big).unwrap()).unwrap();
        let mut full = ModuleTrust::with_backend(mem, "big.json");
        assert_eq!(full.len(), MAX_TRUST_ENTRIES);
        assert!(full.trust(&hash(b"new"), "N").is_err() && !full.is_trusted(&hash(b"new")));
        // A hash that is in the list already can be trusted again.
        full.trust(&hash(b"m0"), "again").unwrap();
    }
}
