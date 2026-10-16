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
//!
//! The stamp comes with the size and the time of change of the file. A file is hashed again only
//! when one of them differs from what was seen, so a plain JSON file of hundreds of megabytes is
//! not read at every save. A save of this program records the stamp from the bytes it wrote
//! ([`record_written`]), so it does not read the file again.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

/// The content stamp of a file: a tag byte (`M` manifest, `C` whole content) and a SHA-256.
pub type Stamp = [u8; 33];

/// The stamp of a container from the SHA-256 of its manifest (`Container::manifest_sha256` as
/// bytes).
pub fn stamp_of_manifest(manifest_sha256: &[u8; 32]) -> Stamp {
    let hex: String = manifest_sha256.iter().map(|b| format!("{b:02x}")).collect();
    let mut out = [b'M'; 33];
    out[1..].copy_from_slice(&Sha256::digest(hex.as_bytes()));
    out
}

/// The stamp of a file from the SHA-256 of all its bytes.
pub fn stamp_of_content(sha256: [u8; 32]) -> Stamp {
    let mut out = [b'C'; 33];
    out[1..].copy_from_slice(&sha256);
    out
}

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
        return Some(stamp_of_manifest(&sha));
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
    Some(stamp_of_content(hasher.finalize().into()))
}

/// The SHA-256 of the manifest of the container at `path`.
fn manifest_of(path: &Path) -> Option<[u8; 32]> {
    let medium = kadakas_container::FileMedium::open_path(path).ok()?;
    let c =
        kadakas_container::Container::open(Arc::new(medium), kadakas_container::Limits::default())
            .ok()?;
    let hex = c.manifest_sha256();
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// The size and the time of change of a file.
type Meta = (u64, SystemTime);

fn meta_of(path: &Path) -> Option<Meta> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

fn key_of(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// What this program wrote last to a path: the stamp of the bytes, and the size and time of change
/// of the file right after the rename. [`SeenFiles::note`] takes it if the file still has that
/// size and time.
fn written() -> std::sync::MutexGuard<'static, HashMap<PathBuf, (Stamp, Meta)>> {
    static WRITTEN: OnceLock<Mutex<HashMap<PathBuf, (Stamp, Meta)>>> = OnceLock::new();
    WRITTEN
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Records the stamp of the bytes that a save wrote to `path` (call it after the rename).
pub fn record_written(path: &Path, stamp: Stamp) {
    let Some(meta) = meta_of(path) else {
        return;
    };
    let mut w = written();
    if w.len() >= 64 {
        w.clear();
    }
    w.insert(key_of(path), (stamp, meta));
}

/// What was seen of one file.
#[derive(Clone, Copy, Debug)]
struct Seen {
    stamp: Stamp,
    meta: Option<Meta>,
}

/// The files that a session saw: the path (as the system names it) and the stamp. Cheap to clone,
/// and the clones share the table (the writer thread of the autosave notes its own save).
#[derive(Clone, Debug, Default)]
pub struct SeenFiles(Arc<Mutex<HashMap<PathBuf, Seen>>>);

impl SeenFiles {
    fn table(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Seen>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Remembers what the file at `path` holds now: call it after the document was opened from the
    /// path and after a save to it. After a save of this program the stamp of the bytes that were
    /// written is taken (the file is not read again); else the file is read. A file that cannot
    /// be read is forgotten.
    pub fn note(&self, path: &Path) {
        let key = key_of(path);
        let meta = meta_of(path);
        let from_save = written()
            .get(&key)
            .filter(|(_, m)| Some(*m) == meta)
            .map(|(s, _)| *s);
        let stamp = from_save.or_else(|| stamp_of(path));
        let mut t = self.table();
        match stamp {
            Some(stamp) => {
                t.insert(key, Seen { stamp, meta });
            }
            None => {
                t.remove(&key);
            }
        }
    }

    /// True if the document was opened from or saved to the path, the file is there, and its
    /// content is not what was seen: another program wrote it since. A path that was never seen,
    /// and a file that is gone, are not "changed". The file is read again only when its size or
    /// its time of change differs from what was seen.
    pub fn changed(&self, path: &Path) -> bool {
        let key = key_of(path);
        let Some(seen) = self.table().get(&key).copied() else {
            return false;
        };
        let Some(now) = meta_of(path) else {
            // Gone, or not readable.
            return path.exists();
        };
        if seen.meta == Some(now) {
            return false;
        }
        match stamp_of(path) {
            Some(s) if s == seen.stamp => {
                // A touch: the same content with another time. Remember the new time.
                if let Some(entry) = self.table().get_mut(&key) {
                    entry.meta = Some(now);
                }
                false
            }
            _ => true,
        }
    }

    /// True if the path was seen.
    pub fn knows(&self, path: &Path) -> bool {
        self.table().contains_key(&key_of(path))
    }
}
