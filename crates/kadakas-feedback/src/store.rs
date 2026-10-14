//! Where the outbox keeps its files: a trait, a memory store, and (native) a folder.
//!
//! The names are fixed words made by this crate (`outbox.json`, `fb-3.render.png`). A store
//! refuses any other name, so a name from outside can never reach a path.

use std::collections::BTreeMap;

pub trait Storage: Send {
    fn read(&self, name: &str) -> Option<Vec<u8>>;
    fn write(&mut self, name: &str, data: &[u8]) -> Result<(), String>;
    fn remove(&mut self, name: &str);
    /// True if the files survive the end of the program.
    fn persistent(&self) -> bool;
    /// An exclusive lock around a load-change-save of the outbox, for programs that share the
    /// folder. The lock ends when the guard is dropped. A store that only one program can reach
    /// needs none.
    fn lock(&self) -> Result<Box<dyn Send>, String> {
        Ok(Box::new(()))
    }
}

/// A name is safe if it has letters, digits, `-`, `_` and inner dots only, and is not long.
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 80
        && !name.starts_with('.')
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Files in memory. For tests, for a web page with no storage, and for a session that keeps
/// nothing. An optional cap on the total size makes `write` fail when it would be passed.
#[derive(Default)]
pub struct MemStorage {
    files: BTreeMap<String, Vec<u8>>,
    cap: Option<usize>,
}

impl MemStorage {
    pub fn new() -> MemStorage {
        MemStorage::default()
    }

    pub fn with_cap(cap: usize) -> MemStorage {
        MemStorage {
            files: BTreeMap::new(),
            cap: Some(cap),
        }
    }

    /// The names of the files.
    pub fn names(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }

    pub fn total_bytes(&self) -> usize {
        self.files.values().map(Vec::len).sum()
    }
}

impl Storage for MemStorage {
    fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.files.get(name).cloned()
    }

    fn write(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        if !safe_name(name) {
            return Err(format!("not a feedback file name: {name}"));
        }
        let old = self.files.get(name).map_or(0, Vec::len);
        if let Some(cap) = self.cap
            && self.total_bytes() - old + data.len() > cap
        {
            return Err("the feedback store is full".into());
        }
        self.files.insert(name.to_string(), data.to_vec());
        Ok(())
    }

    fn remove(&mut self, name: &str) {
        self.files.remove(name);
    }

    fn persistent(&self) -> bool {
        false
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{DirStorage, default_dir, default_dir_for};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{Storage, safe_name};
    use std::io::Read;
    use std::path::PathBuf;

    /// A file is never read beyond this size.
    const READ_LIMIT: u64 = 16 << 20;

    /// The folder of the outbox: `KADAKAS_FEEDBACK_DIR` if it is set (tests use it), else
    /// `feedback` in the data folder of the user.
    pub fn default_dir() -> Option<PathBuf> {
        default_dir_for(std::env::var_os("KADAKAS_FEEDBACK_DIR"))
    }

    /// Like [`default_dir`], with the value of `KADAKAS_FEEDBACK_DIR` given.
    pub fn default_dir_for(over: Option<std::ffi::OsString>) -> Option<PathBuf> {
        match over.filter(|v| !v.is_empty()) {
            Some(d) => Some(PathBuf::from(d)),
            None => kadakas_sys::dirs::data_dir().map(|d| d.join("feedback")),
        }
    }

    /// Files in a folder, written atomically with owner-only permissions.
    pub struct DirStorage {
        dir: PathBuf,
    }

    impl DirStorage {
        pub fn new(dir: PathBuf) -> DirStorage {
            DirStorage { dir }
        }

        pub fn dir(&self) -> &std::path::Path {
            &self.dir
        }
    }

    impl Storage for DirStorage {
        fn read(&self, name: &str) -> Option<Vec<u8>> {
            if !safe_name(name) {
                return None;
            }
            let f = std::fs::File::open(self.dir.join(name)).ok()?;
            let mut buf = Vec::new();
            f.take(READ_LIMIT).read_to_end(&mut buf).ok()?;
            Some(buf)
        }

        fn write(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
            if !safe_name(name) {
                return Err(format!("not a feedback file name: {name}"));
            }
            kadakas_sys::fsio::create_private_dir(&self.dir)
                .or_else(|e| if self.dir.is_dir() { Ok(()) } else { Err(e) })
                .map_err(|e| format!("cannot create the feedback folder: {e}"))?;
            kadakas_sys::fsio::write_private(&self.dir.join(name), data)
                .map_err(|e| format!("cannot write {name}: {e}"))
        }

        fn remove(&mut self, name: &str) {
            if safe_name(name) {
                let _ = std::fs::remove_file(self.dir.join(name));
            }
        }

        fn persistent(&self) -> bool {
            true
        }

        fn lock(&self) -> Result<Box<dyn Send>, String> {
            kadakas_sys::fsio::create_private_dir(&self.dir)
                .or_else(|e| if self.dir.is_dir() { Ok(()) } else { Err(e) })
                .map_err(|e| format!("cannot create the feedback folder: {e}"))?;
            // The file `lock` stays where it is. The operating system holds the lock for the open
            // file, and releases it when the file closes, also when the program dies. So no
            // program ever has to decide that a lock is stale, and none removes the file.
            let mut opts = std::fs::OpenOptions::new();
            opts.read(true).write(true).create(true).truncate(false);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let file = opts
                .open(self.dir.join("lock"))
                .map_err(|e| format!("cannot open the lock of the feedback folder: {e}"))?;
            for _ in 0..STEPS {
                match file.try_lock() {
                    Ok(()) => return Ok(Box::new(Guard(file))),
                    Err(std::fs::TryLockError::WouldBlock) => std::thread::sleep(WAIT),
                    Err(std::fs::TryLockError::Error(e)) => {
                        return Err(format!("cannot lock the feedback folder: {e}"));
                    }
                }
            }
            Err("the feedback folder is locked by another program".into())
        }
    }

    /// The open lock file; dropping it releases the lock.
    struct Guard(std::fs::File);

    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.unlock();
        }
    }

    const STEPS: u32 = 250;
    const WAIT: std::time::Duration = std::time::Duration::from_millis(20);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        assert!(safe_name("outbox.json") && safe_name("fb-3.render.png"));
        for bad in [
            "",
            ".x",
            "a/b",
            "a\\b",
            "..",
            "a..b",
            "a b",
            "ä",
            &"x".repeat(81),
        ] {
            assert!(!safe_name(bad), "{bad}");
        }
    }

    #[test]
    fn memory_store_has_a_cap() {
        let mut m = MemStorage::with_cap(10);
        m.write("a", b"12345").unwrap();
        m.write("b", b"12345").unwrap();
        assert!(m.write("c", b"1").is_err());
        // A write over an old file counts the difference.
        m.write("a", b"123").unwrap();
        m.write("c", b"12").unwrap();
        assert_eq!(m.total_bytes(), 10);
        m.remove("b");
        assert_eq!(m.read("b"), None);
        assert_eq!(m.names(), vec!["a", "c"]);
        assert!(m.write("../x", b"").is_err());
        assert!(!m.persistent());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn folder_store_roundtrip() {
        let tmp = kadakas_sys::tmp::PrivateTempDir::new("kadakas-fb-store").unwrap();
        let mut d = DirStorage::new(tmp.path().join("fb"));
        assert_eq!(d.read("a.json"), None);
        d.write("a.json", b"{}").unwrap();
        d.write("a.json", b"{\"x\":1}").unwrap();
        assert_eq!(d.read("a.json").unwrap(), b"{\"x\":1}");
        assert!(d.write("../evil", b"x").is_err());
        assert_eq!(d.read("../evil"), None);
        d.remove("a.json");
        d.remove("../evil");
        assert_eq!(d.read("a.json"), None);
        assert!(d.persistent() && d.dir().ends_with("fb"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_lock_is_an_os_lock_on_a_file_that_stays() {
        let tmp = kadakas_sys::tmp::PrivateTempDir::new("kadakas-fb-lock").unwrap();
        let dir = tmp.path().join("fb");
        let d = DirStorage::new(dir.clone());
        let guard = d.lock().unwrap();
        let other = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.join("lock"))
            .unwrap();
        assert!(matches!(
            other.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(guard);
        // Released, and the file is still there: nobody removes it.
        other.try_lock().unwrap();
        other.unlock().unwrap();
        assert!(dir.join("lock").is_file());
        d.lock().unwrap();
        assert!(dir.join("lock").is_file());
        // A memory store needs no lock.
        assert!(MemStorage::new().lock().is_ok());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn default_folder_follows_the_override() {
        assert_eq!(
            default_dir_for(Some("/x/fb".into())),
            Some(std::path::PathBuf::from("/x/fb"))
        );
        let d = default_dir_for(Some("".into()));
        assert!(d.is_none_or(|d| d.ends_with("feedback")));
        let _ = default_dir();
    }
}
