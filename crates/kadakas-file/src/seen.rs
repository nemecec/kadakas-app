//! The files that a document was opened from or saved to, and what they held at that moment
//! (DOC-21): the guard against replacing a file that another program wrote since.
//!
//! The guard of a lazily opened container compares its manifest (`FileSource::same_file`). That
//! covers only a container that was opened lazily. [`SeenFiles`] covers every path that the
//! document was opened from or saved to, in both forms: it keeps a content stamp for the path,
//! and a save compares it before the replace. For a container the stamp is the SHA-256 of its
//! manifest (the directory at the end of the file, so a big file costs a few kilobytes); for any
//! other file it is the SHA-256 of the whole file. A touched or copied file with the same content
//! has the same stamp.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

/// The content stamp of a file: a tag byte (`M` manifest, `C` whole content) and a SHA-256.
pub type Stamp = [u8; 33];

/// The stamp of the file at `path`, or `None` if it cannot be read.
pub fn stamp_of(path: &Path) -> Option<Stamp> {
    let mut f = std::fs::File::open(path).ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut head = [0u8; 4];
    let n = f.read(&mut head).ok()?;
    if n == 4
        && head == *b"PK\x03\x04"
        && let Some(sha) = manifest_of(path)
    {
        let mut out = [b'M'; 33];
        out[1..].copy_from_slice(&sha);
        return Some(out);
    }
    // Any other file, or a ZIP file that is not a container: its whole content.
    let mut f = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let mut out = [b'C'; 33];
    out[1..].copy_from_slice(&hasher.finalize());
    Some(out)
}

/// A SHA-256 that stands for the manifest of the container at `path` (the hash of its hex text).
fn manifest_of(path: &Path) -> Option<[u8; 32]> {
    let medium = kadakas_container::FileMedium::open_path(path).ok()?;
    let c =
        kadakas_container::Container::open(Arc::new(medium), kadakas_container::Limits::default())
            .ok()?;
    Some(Sha256::digest(c.manifest_sha256().as_bytes()).into())
}

/// The files that a session saw: the path (as the system names it) and the stamp. Cheap to clone,
/// and the clones share the table (the writer thread of the autosave notes its own save).
#[derive(Clone, Debug, Default)]
pub struct SeenFiles(Arc<Mutex<HashMap<PathBuf, Stamp>>>);

impl SeenFiles {
    fn key(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    fn table(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Stamp>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Looks at the file now and remembers its stamp: call it after the document was opened from
    /// the path and after a save to it. A file that cannot be read is forgotten.
    pub fn note(&self, path: &Path) {
        let stamp = stamp_of(path);
        let key = Self::key(path);
        let mut t = self.table();
        match stamp {
            Some(s) => {
                t.insert(key, s);
            }
            None => {
                t.remove(&key);
            }
        }
    }

    /// True if the document was opened from or saved to the path, the file is there, and its
    /// content is not what was seen: another program wrote it since. A path that was never seen,
    /// and a file that is gone, are not "changed".
    pub fn changed(&self, path: &Path) -> bool {
        let key = Self::key(path);
        let Some(seen) = self.table().get(&key).copied() else {
            return false;
        };
        if !path.exists() {
            return false;
        }
        stamp_of(path).is_none_or(|now| now != seen)
    }

    /// True if the path was seen.
    pub fn knows(&self, path: &Path) -> bool {
        self.table().contains_key(&Self::key(path))
    }
}
