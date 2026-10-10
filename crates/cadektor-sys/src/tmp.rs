//! Private temporary folders.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A private temporary folder (mode 0700 on Unix) that is removed when it goes out of scope.
#[derive(Debug)]
pub struct PrivateTempDir(PathBuf);

impl PrivateTempDir {
    /// Makes `<temp>/<prefix>-<pid>-<n>-<nanos>`. The name is new: an existing folder is never reused.
    pub fn new(prefix: &str) -> std::io::Result<PrivateTempDir> {
        let base = std::env::temp_dir();
        for _ in 0..100 {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let dir = base.join(format!(
                "{prefix}-{}-{}-{nanos}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            #[allow(unused_mut)] // only unix sets the mode
            let mut b = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                b.mode(0o700);
            }
            match b.create(&dir) {
                Ok(()) => return Ok(PrivateTempDir(dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(std::io::Error::new(
                        e.kind(),
                        format!("cannot make a temporary folder in {}: {e}", base.display()),
                    ));
                }
            }
        }
        Err(std::io::Error::other("cannot make a temporary folder"))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for PrivateTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_new_private_and_removed_on_drop() {
        let a = PrivateTempDir::new("cadektor-sys-test").unwrap();
        let b = PrivateTempDir::new("cadektor-sys-test").unwrap();
        assert_ne!(a.path(), b.path());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(a.path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        let p = a.path().to_path_buf();
        std::fs::write(p.join("f"), "x").unwrap();
        drop(a);
        assert!(!p.exists());
    }
}
