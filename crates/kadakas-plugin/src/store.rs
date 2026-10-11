//! Keeping installed modules and the trust list between sessions.
//!
//! The host does not name a folder or a browser API here. A [`StoreBackend`] holds named blobs.
//! The caller injects one: a [`FileBackend`] on a folder of the user (native), a browser backend
//! (`crate::web`, wasm32), or a [`MemoryBackend`] (tests, or a browser without storage). Tests
//! never use the real folders of the user.
//!
//! A module is stored under the name `<sha256 hex>.wasm`. The store treats the disk as untrusted:
//!
//! - **Verify on load.** The bytes must have the SHA-256 that the name says. A file that was
//!   changed is refused and reported. The trust of the user is for the hash, so a changed file
//!   could never run anyway, but it is not even compiled.
//! - **Caps.** One module has at most `max_module` bytes (2 MiB). All modules together have at
//!   most `max_total` bytes. A module that does not fit is not saved (the session still has it)
//!   and the caller gets the reason.
//! - **Names.** Only names of the form `<64 hex digits>.wasm` are modules. The store ignores other
//!   files.
//! - **No panic, no exit.** Every call returns errors as text. A broken disk or a full quota
//!   never stops the app.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::runtime::MaybeSend;
use crate::trust;

/// The file name of the trust list.
pub const TRUST_FILE: &str = "trusted_wasm_plugins.json";

/// Most bytes of all stored modules together on a computer.
pub const NATIVE_TOTAL_CAP: usize = 64 << 20;

/// Most bytes of all stored modules together in a browser. The browser quota is small, and it is
/// shared with the autosave of the drawing.
pub const WEB_TOTAL_CAP: usize = 3 << 20;

/// Largest trust list that the host reads, in bytes.
pub const MAX_TRUST_BYTES: usize = 1 << 20;

/// Most entries in the trust list.
pub const MAX_TRUST_ENTRIES: usize = 10_000;

/// Named blobs on some medium.
pub trait StoreBackend: MaybeSend {
    /// A short name for the medium, for messages: "folder", "browser storage", "memory".
    fn medium(&self) -> &'static str;
    /// False when the data does not outlive the session.
    fn persistent(&self) -> bool {
        true
    }
    /// The names and sizes of all blobs.
    fn list(&self) -> Result<Vec<(String, u64)>, String>;
    /// The blob, or `None` if there is none. A blob over `max` bytes is an error.
    fn read(&self, name: &str, max: usize) -> Result<Option<Vec<u8>>, String>;
    /// Writes (or replaces) a blob.
    fn write(&self, name: &str, bytes: &[u8]) -> Result<(), String>;
    /// Removes a blob. A missing blob is fine.
    fn remove(&self, name: &str) -> Result<(), String>;
}

/// True for a name that a backend may touch: letters, digits, `.`, `-` and `_`, no path parts.
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

// ------------------------------------------------------------------- memory

/// Blobs in memory. They end with the session. `fail_writes` makes a full quota for tests.
#[derive(Default)]
pub struct MemoryBackend {
    blobs: Mutex<BTreeMap<String, Vec<u8>>>,
    /// When set, every write fails with this text.
    pub fail_writes: Mutex<Option<String>>,
    /// When set, every list, read and remove fails with this text.
    pub fail_all: Mutex<Option<String>>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }

    fn check(&self) -> Result<(), String> {
        match &*self.fail_all.lock().expect("lock") {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
}

impl StoreBackend for MemoryBackend {
    fn medium(&self) -> &'static str {
        "memory"
    }

    fn persistent(&self) -> bool {
        false
    }

    fn list(&self) -> Result<Vec<(String, u64)>, String> {
        self.check()?;
        Ok(self
            .blobs
            .lock()
            .expect("lock")
            .iter()
            .map(|(k, v)| (k.clone(), v.len() as u64))
            .collect())
    }

    fn read(&self, name: &str, max: usize) -> Result<Option<Vec<u8>>, String> {
        self.check()?;
        match self.blobs.lock().expect("lock").get(name) {
            Some(b) if b.len() > max => Err(format!("{name} has more than {max} bytes")),
            Some(b) => Ok(Some(b.clone())),
            None => Ok(None),
        }
    }

    fn write(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        if let Some(e) = &*self.fail_writes.lock().expect("lock") {
            return Err(e.clone());
        }
        self.blobs
            .lock()
            .expect("lock")
            .insert(name.to_string(), bytes.to_vec());
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<(), String> {
        self.check()?;
        self.blobs.lock().expect("lock").remove(name);
        Ok(())
    }
}

// -------------------------------------------------------------------- files

/// Blobs as files in one folder. The folder is a parameter: the app passes the folder of the
/// user, a test passes a temporary folder. Writes are atomic, and the files are private to the
/// user (Unix mode 0600).
#[cfg(not(target_arch = "wasm32"))]
pub struct FileBackend {
    dir: std::path::PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileBackend {
    pub fn new(dir: std::path::PathBuf) -> Self {
        FileBackend { dir }
    }

    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    fn path(&self, name: &str) -> Result<std::path::PathBuf, String> {
        if safe_name(name) {
            Ok(self.dir.join(name))
        } else {
            Err(format!("'{name}' is not a name that the store uses"))
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl StoreBackend for FileBackend {
    fn medium(&self) -> &'static str {
        "folder"
    }

    fn list(&self) -> Result<Vec<(String, u64)>, String> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(format!("cannot read {}: {e}", self.dir.display())),
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            // A link or a folder is not one of our files.
            let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            if let Some(name) = entry.file_name().to_str().filter(|n| safe_name(n)) {
                out.push((name.to_string(), meta.len()));
            }
        }
        out.sort();
        Ok(out)
    }

    fn read(&self, name: &str, max: usize) -> Result<Option<Vec<u8>>, String> {
        use std::io::Read;
        let path = self.path(name)?;
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("cannot read {name}: {e}")),
        };
        if !meta.is_file() {
            return Err(format!("{name} is not a plain file"));
        }
        let file = std::fs::File::open(&path).map_err(|e| format!("cannot read {name}: {e}"))?;
        let mut bytes = Vec::new();
        file.take(max as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("cannot read {name}: {e}"))?;
        if bytes.len() > max {
            return Err(format!("{name} has more than {max} bytes"));
        }
        Ok(Some(bytes))
    }

    fn write(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.path(name)?;
        kadakas_sys::fsio::create_private_dir(&self.dir)
            .map_err(|e| format!("cannot make {}: {e}", self.dir.display()))?;
        kadakas_sys::fsio::write_private(&path, bytes)
            .map_err(|e| format!("cannot save {name}: {e}"))
    }

    fn remove(&self, name: &str) -> Result<(), String> {
        match std::fs::remove_file(self.path(name)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("cannot remove {name}: {e}")),
        }
    }
}

/// The folder of the user for plug-ins: modules and the trust list. It is a folder inside the
/// user data folder of `kadakas-sys`.
#[cfg(not(target_arch = "wasm32"))]
pub fn default_home() -> Option<std::path::PathBuf> {
    Some(kadakas_sys::dirs::data_dir()?.join("plugins"))
}

// ------------------------------------------------------------------ modules

/// A stored blob that was not loaded, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejected {
    pub name: String,
    pub reason: String,
}

/// What `load_all` found.
#[derive(Debug, Default)]
pub struct Loaded {
    /// The bytes of the modules that passed the checks, in the order of their names.
    pub modules: Vec<Vec<u8>>,
    pub rejected: Vec<Rejected>,
}

/// Why a module was not saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// The module is over the size of one module.
    TooLarge { size: usize, max: usize },
    /// All modules together would be over the total.
    Full { max_total: usize },
    /// The medium failed.
    Medium(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::TooLarge { size, max } => {
                write!(f, "the module has {size} bytes. The limit is {max}")
            }
            StoreError::Full { max_total } => write!(
                f,
                "the stored modules would be over {max_total} bytes together. Remove a module first"
            ),
            StoreError::Medium(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for StoreError {}

/// The modules that the user installed, kept on a medium.
#[derive(Clone)]
pub struct ModuleStore {
    backend: Arc<dyn StoreBackend>,
    max_module: usize,
    max_total: usize,
}

impl ModuleStore {
    pub fn new(backend: Arc<dyn StoreBackend>, max_module: usize, max_total: usize) -> Self {
        ModuleStore {
            backend,
            max_module,
            max_total,
        }
    }

    /// False when the modules do not outlive the session.
    pub fn persistent(&self) -> bool {
        self.backend.persistent()
    }

    pub fn medium(&self) -> &'static str {
        self.backend.medium()
    }

    pub fn max_total(&self) -> usize {
        self.max_total
    }

    /// The name of the blob that holds the module with this hash.
    pub fn name_of(hash: &str) -> String {
        format!("{hash}.wasm")
    }

    /// The hash that a blob name says, if the name is a module name.
    fn hash_of(name: &str) -> Option<&str> {
        let hash = name.strip_suffix(".wasm")?;
        (hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .then_some(hash)
    }

    /// Reads all stored modules. A module that is too big, changed (its hash is not the one in its
    /// name), or over the total is reported and left out. The medium failing is reported as one
    /// entry with the name of the medium.
    pub fn load_all(&self) -> Loaded {
        let mut out = Loaded::default();
        let list = match self.backend.list() {
            Ok(l) => l,
            Err(reason) => {
                out.rejected.push(Rejected {
                    name: self.backend.medium().to_string(),
                    reason,
                });
                return out;
            }
        };
        let mut total = 0usize;
        for (name, size) in list {
            let Some(hash) = Self::hash_of(&name) else {
                continue;
            };
            let reject = |out: &mut Loaded, reason: String| {
                out.rejected.push(Rejected {
                    name: name.clone(),
                    reason,
                })
            };
            if size > self.max_module as u64 {
                reject(
                    &mut out,
                    format!(
                        "the file has {size} bytes. The limit for a module is {}",
                        self.max_module
                    ),
                );
                continue;
            }
            let bytes = match self.backend.read(&name, self.max_module) {
                Ok(Some(b)) => b,
                // It vanished between the list and the read.
                Ok(None) => continue,
                Err(e) => {
                    reject(&mut out, e);
                    continue;
                }
            };
            if trust::hash(&bytes) != hash {
                reject(
                    &mut out,
                    "the file was changed: its SHA-256 is not the one in its name".into(),
                );
                continue;
            }
            if total + bytes.len() > self.max_total {
                reject(
                    &mut out,
                    format!(
                        "the stored modules are over {} bytes together",
                        self.max_total
                    ),
                );
                continue;
            }
            total += bytes.len();
            out.modules.push(bytes);
        }
        out
    }

    /// Saves a module under its hash. Saving the same module again changes nothing.
    pub fn save(&self, wasm: &[u8]) -> Result<(), StoreError> {
        if wasm.len() > self.max_module {
            return Err(StoreError::TooLarge {
                size: wasm.len(),
                max: self.max_module,
            });
        }
        let name = Self::name_of(&trust::hash(wasm));
        let list = self.backend.list().map_err(StoreError::Medium)?;
        if list
            .iter()
            .any(|(n, s)| *n == name && *s == wasm.len() as u64)
        {
            return Ok(());
        }
        let others: usize = list
            .iter()
            .filter(|(n, _)| *n != name && Self::hash_of(n).is_some())
            .map(|(_, s)| usize::try_from(*s).unwrap_or(usize::MAX))
            .fold(0, usize::saturating_add);
        if others.saturating_add(wasm.len()) > self.max_total {
            return Err(StoreError::Full {
                max_total: self.max_total,
            });
        }
        self.backend.write(&name, wasm).map_err(StoreError::Medium)
    }

    /// Removes the module with this hash.
    pub fn remove(&self, hash: &str) -> Result<(), String> {
        match Self::hash_of(&Self::name_of(hash)) {
            Some(_) => self.backend.remove(&Self::name_of(hash)),
            None => Err(format!("'{hash}' is not a module hash")),
        }
    }
}

// -------------------------------------------------------------------- trust

/// Reads the trust list from bytes: a JSON object, hash -> name. Entries with a name that is not
/// a hash are dropped. A broken list is an empty list.
pub fn parse_trust(bytes: &[u8]) -> BTreeMap<String, String> {
    let Ok(map) = serde_json::from_slice::<BTreeMap<String, String>>(bytes) else {
        return BTreeMap::new();
    };
    map.into_iter()
        .filter(|(h, n)| {
            ModuleStore::hash_of(&format!("{h}.wasm")).is_some() && n.chars().count() <= 200
        })
        .take(MAX_TRUST_ENTRIES)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(wasm: &[u8]) -> String {
        trust::hash(wasm)
    }

    fn store(max_module: usize, max_total: usize) -> (ModuleStore, Arc<MemoryBackend>) {
        let mem = Arc::new(MemoryBackend::new());
        (ModuleStore::new(mem.clone(), max_module, max_total), mem)
    }

    #[test]
    fn names_are_checked() {
        for ok in ["a.wasm", "trusted_wasm_plugins.json", "A-b_1.x"] {
            assert!(safe_name(ok), "{ok}");
        }
        for bad in ["", ".hidden", "a/b", "a\\b", "..", "a b", &"x".repeat(129)] {
            assert!(!safe_name(bad), "{bad}");
        }
    }

    #[test]
    fn modules_are_saved_and_loaded_by_hash() {
        let (s, mem) = store(100, 1000);
        assert!(!s.persistent() && s.medium() == "memory" && s.max_total() == 1000);
        s.save(b"module one").unwrap();
        s.save(b"module two").unwrap();
        // The same module again changes nothing.
        s.save(b"module one").unwrap();
        assert_eq!(mem.list().unwrap().len(), 2);
        let loaded = s.load_all();
        assert!(loaded.rejected.is_empty());
        assert_eq!(loaded.modules.len(), 2);
        assert!(loaded.modules.contains(&b"module one".to_vec()));
        s.remove(&hex(b"module one")).unwrap();
        assert_eq!(s.load_all().modules, vec![b"module two".to_vec()]);
        // A name that is not a hash is refused, so no path can be built from it.
        assert!(s.remove("../x").is_err());
        assert!(s.remove("ABC").is_err());
    }

    #[test]
    fn a_changed_file_is_refused_and_reported() {
        let (s, mem) = store(100, 1000);
        s.save(b"good").unwrap();
        // A file whose bytes do not match its name.
        let forged = ModuleStore::name_of(&hex(b"original"));
        mem.write(&forged, b"tampered").unwrap();
        let loaded = s.load_all();
        assert_eq!(loaded.modules, vec![b"good".to_vec()]);
        assert_eq!(loaded.rejected.len(), 1);
        assert_eq!(loaded.rejected[0].name, forged);
        assert!(loaded.rejected[0].reason.contains("changed"));
    }

    #[test]
    fn size_caps_apply_on_save_and_on_load() {
        let (s, mem) = store(10, 25);
        assert_eq!(
            s.save(&[0; 11]).unwrap_err(),
            StoreError::TooLarge { size: 11, max: 10 }
        );
        s.save(&[1; 10]).unwrap();
        s.save(&[2; 10]).unwrap();
        let e = s.save(&[3; 10]).unwrap_err();
        assert_eq!(e, StoreError::Full { max_total: 25 });
        assert!(e.to_string().contains("25") && e.to_string().contains("Remove"));
        assert!(
            StoreError::TooLarge { size: 1, max: 2 }
                .to_string()
                .contains("limit")
        );
        assert_eq!(StoreError::Medium("x".into()).to_string(), "x");
        // A bigger store wrote more files than this one allows: the extra ones are refused.
        let big = ModuleStore::new(mem.clone(), 100, 1000);
        big.save(&[4; 50]).unwrap();
        big.save(&[5; 8]).unwrap();
        let loaded = s.load_all();
        assert!(loaded.rejected.iter().any(|r| r.reason.contains("limit")));
        // The total caps what is loaded: 10 + 10 fit, the 8 byte one (a hash sorts anywhere) may not.
        let total: usize = loaded.modules.iter().map(Vec::len).sum();
        assert!(total <= 25, "{total}");
        assert!(
            loaded
                .rejected
                .iter()
                .any(|r| r.reason.contains("together") || r.reason.contains("limit"))
        );
    }

    #[test]
    fn a_backend_that_fails_is_reported_not_fatal() {
        let (s, mem) = store(100, 1000);
        s.save(b"x").unwrap();
        *mem.fail_all.lock().unwrap() = Some("disk gone".into());
        let loaded = s.load_all();
        assert!(loaded.modules.is_empty());
        assert_eq!(loaded.rejected[0].name, "memory");
        assert_eq!(loaded.rejected[0].reason, "disk gone");
        assert_eq!(
            s.save(b"y").unwrap_err(),
            StoreError::Medium("disk gone".into())
        );
        assert!(s.remove(&hex(b"x")).is_err());
        *mem.fail_all.lock().unwrap() = None;
        *mem.fail_writes.lock().unwrap() = Some("quota".into());
        assert_eq!(
            s.save(b"y").unwrap_err(),
            StoreError::Medium("quota".into())
        );
    }

    #[test]
    fn other_blobs_are_ignored_and_a_big_read_is_an_error() {
        let (s, mem) = store(100, 1000);
        mem.write("notes.txt", b"hi").unwrap();
        mem.write("short.wasm", b"hi").unwrap();
        mem.write(&format!("{}.wasm", "G".repeat(64)), b"hi")
            .unwrap();
        assert!(s.load_all().modules.is_empty() && s.load_all().rejected.is_empty());
        assert!(mem.read("notes.txt", 1).is_err());
        assert_eq!(mem.read("nothing", 1).unwrap(), None);
        // A listing that lies about a size is caught by the read.
        let name = ModuleStore::name_of(&hex(&[7; 200]));
        mem.write(&name, &[7; 200]).unwrap();
        let loaded = s.load_all();
        assert!(loaded.rejected.iter().any(|r| r.name == name));
    }

    #[test]
    fn the_trust_list_is_read_with_care() {
        let h = hex(b"a");
        let ok = format!(
            r#"{{"{h}":"Name","nothex":"X","{}":"{}"}}"#,
            hex(b"b"),
            "n".repeat(300)
        );
        let m = parse_trust(ok.as_bytes());
        assert_eq!(m.len(), 1);
        assert_eq!(m[&h], "Name");
        assert!(parse_trust(b"not json").is_empty());
        assert!(parse_trust(b"[1]").is_empty());
        let many: BTreeMap<String, String> = (0..MAX_TRUST_ENTRIES + 5)
            .map(|i| (hex(format!("m{i}").as_bytes()), "n".into()))
            .collect();
        let text = serde_json::to_vec(&many).unwrap();
        assert_eq!(parse_trust(&text).len(), MAX_TRUST_ENTRIES);
    }

    #[cfg(not(target_arch = "wasm32"))]
    mod files {
        use super::*;

        fn temp(tag: &str) -> std::path::PathBuf {
            let dir = std::env::temp_dir().join(format!(
                "kadakas-store-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            dir
        }

        #[test]
        fn modules_live_in_the_folder_that_the_caller_gives() {
            let dir = temp("a");
            let backend = Arc::new(FileBackend::new(dir.join("modules")));
            assert_eq!(backend.dir(), dir.join("modules"));
            assert!(backend.persistent() && backend.medium() == "folder");
            // A folder that does not exist is an empty store.
            let s = ModuleStore::new(backend.clone(), 1000, 10_000);
            assert!(s.load_all().modules.is_empty() && s.load_all().rejected.is_empty());
            s.save(b"module one").unwrap();
            assert!(s.persistent());
            let name = ModuleStore::name_of(&hex(b"module one"));
            assert!(dir.join("modules").join(&name).is_file());
            assert_eq!(s.load_all().modules, vec![b"module one".to_vec()]);
            // Another file in the folder is ignored. A folder with a module name is skipped too.
            std::fs::write(dir.join("modules").join("notes.txt"), "x").unwrap();
            std::fs::create_dir(dir.join("modules").join(ModuleStore::name_of(&hex(b"dir"))))
                .unwrap();
            assert_eq!(backend.list().unwrap().len(), 2);
            // Changing the file on disk is caught.
            std::fs::write(dir.join("modules").join(&name), "changed").unwrap();
            let loaded = s.load_all();
            assert!(loaded.modules.is_empty() && loaded.rejected.len() == 1);
            s.remove(&hex(b"module one")).unwrap();
            s.remove(&hex(b"module one")).unwrap();
            assert!(!dir.join("modules").join(&name).exists());
            std::fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn the_file_backend_refuses_bad_names_big_files_and_plain_errors() {
            let dir = temp("b");
            let backend = FileBackend::new(dir.clone());
            for bad in ["../x", "a/b", ".h", ""] {
                assert!(backend.write(bad, b"x").is_err(), "{bad}");
                assert!(backend.read(bad, 10).is_err());
                assert!(backend.remove(bad).is_err());
            }
            backend.write("big.bin", &[0; 20]).unwrap();
            assert!(
                backend
                    .read("big.bin", 10)
                    .unwrap_err()
                    .contains("more than")
            );
            assert_eq!(backend.read("big.bin", 20).unwrap().unwrap().len(), 20);
            assert_eq!(backend.read("missing", 10).unwrap(), None);
            // A folder is not a file.
            std::fs::create_dir(dir.join("sub")).unwrap();
            assert!(backend.read("sub", 10).unwrap_err().contains("plain file"));
            assert!(backend.remove("sub").is_err());
            // A file where the folder should be: nothing can be listed or saved.
            let blocked = FileBackend::new(dir.join("big.bin"));
            assert!(blocked.list().is_err());
            assert!(
                blocked
                    .write("a", b"x")
                    .unwrap_err()
                    .contains("cannot make")
            );
            std::fs::remove_dir_all(dir).unwrap();
        }

        #[cfg(unix)]
        #[test]
        fn a_link_is_not_followed() {
            let dir = temp("c");
            std::fs::create_dir_all(&dir).unwrap();
            let target = dir.join("secret.txt");
            std::fs::write(&target, "secret").unwrap();
            let name = ModuleStore::name_of(&hex(b"secret"));
            std::os::unix::fs::symlink(&target, dir.join(&name)).unwrap();
            let backend = FileBackend::new(dir.clone());
            assert!(backend.list().unwrap().iter().all(|(n, _)| *n != name));
            assert!(backend.read(&name, 100).is_err());
            std::fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn the_default_home_is_inside_the_user_data_folder() {
            if let Some(home) = default_home() {
                assert!(home.ends_with("plugins"));
            }
        }
    }
}
