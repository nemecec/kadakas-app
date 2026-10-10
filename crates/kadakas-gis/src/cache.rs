//! A tile cache on disk with a size limit and an expiry.
//!
//! Each tile is one file. The name comes from a hash of the request address. A tile older than the
//! maximum age is not used and is removed. When the files are bigger than the limit, the oldest
//! files go first. Callers store a tile only after they have checked that it is a good picture,
//! so an error answer is never kept. The cache never fails a request: a problem with the disk
//! only means that the tile is fetched again.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Default limit: 256 MB.
pub const DEFAULT_LIMIT: u64 = 256 * 1024 * 1024;
/// Default age after which a tile is fetched again: 7 days.
pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Clone, Debug)]
pub struct TileCache {
    dir: PathBuf,
    limit: u64,
    max_age: Duration,
}

/// FNV-1a, 64 bit. It does not change between Rust versions, so cached files stay valid.
fn hash(key: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl TileCache {
    pub fn new(dir: impl Into<PathBuf>, limit: u64) -> TileCache {
        TileCache {
            dir: dir.into(),
            limit,
            max_age: DEFAULT_MAX_AGE,
        }
    }

    /// Sets the age after which a tile is fetched again.
    pub fn with_max_age(mut self, max_age: Duration) -> Self {
        self.max_age = max_age;
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn limit(&self) -> u64 {
        self.limit
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir
            .join(format!("{:016x}{:04x}.tile", hash(key), key.len() & 0xffff))
    }

    /// The bytes of a tile, if the cache has it and it is not too old.
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let path = self.path(key);
        let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
        if SystemTime::now()
            .duration_since(modified)
            .is_ok_and(|age| age > self.max_age)
        {
            let _ = std::fs::remove_file(&path);
            return None;
        }
        std::fs::read(&path).ok()
    }

    /// Removes a tile (for example one that turned out to be damaged).
    pub fn remove(&self, key: &str) {
        let _ = std::fs::remove_file(self.path(key));
    }

    /// Stores a tile, then removes the oldest files until the cache fits the limit.
    pub fn put(&self, key: &str, bytes: &[u8]) {
        if bytes.len() as u64 > self.limit || std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let path = self.path(key);
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        if std::fs::write(&tmp, bytes).is_err() || std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return;
        }
        self.trim();
    }

    /// (number of files, bytes) of the cache.
    pub fn stats(&self) -> (usize, u64) {
        let files = self.files();
        (files.len(), files.iter().map(|f| f.1).sum())
    }

    fn files(&self) -> Vec<(PathBuf, u64, SystemTime)> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        rd.filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "tile"))
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                Some((
                    e.path(),
                    m.len(),
                    m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                ))
            })
            .collect()
    }

    fn trim(&self) {
        let mut files = self.files();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        if total <= self.limit {
            return;
        }
        files.sort_by_key(|f| f.2);
        for (path, size, _) in files {
            if total <= self.limit {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= size;
            }
        }
    }

    /// Removes all tiles. Returns how many.
    pub fn clear(&self) -> usize {
        self.files()
            .into_iter()
            .filter(|f| std::fs::remove_file(&f.0).is_ok())
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("kadakas-gis-cache-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn stores_and_returns_tiles() {
        let d = dir("basic");
        let c = TileCache::new(&d, 1000);
        assert_eq!(c.get("a"), None);
        c.put("a", b"hello");
        assert_eq!(c.get("a").unwrap(), b"hello");
        assert_eq!(c.stats(), (1, 5));
        c.put("a", b"hi");
        assert_eq!(c.stats(), (1, 2));
        assert_eq!(c.dir(), d.as_path());
        assert_eq!(c.limit(), 1000);
        assert_eq!(c.clear(), 1);
        assert_eq!(c.stats(), (0, 0));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn keeps_to_the_size_limit_and_drops_the_oldest() {
        let d = dir("limit");
        let c = TileCache::new(&d, 25);
        c.put("one", &[1; 10]);
        std::thread::sleep(std::time::Duration::from_millis(30));
        c.put("two", &[2; 10]);
        std::thread::sleep(std::time::Duration::from_millis(30));
        c.put("three", &[3; 10]);
        assert!(c.get("one").is_none(), "the oldest tile goes");
        assert!(c.get("two").is_some() && c.get("three").is_some());
        let (_, bytes) = c.stats();
        assert!(bytes <= 25);
        // A tile bigger than the whole cache is not kept.
        c.put("big", &[0; 100]);
        assert!(c.get("big").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn old_tiles_are_not_used_and_bad_tiles_can_be_removed() {
        let d = dir("age");
        let c = TileCache::new(&d, 1000);
        c.put("a", b"data");
        // A tile of 8 days is older than the default 7 days.
        let path = std::fs::read_dir(&d)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let old = SystemTime::now() - Duration::from_secs(8 * 24 * 3600);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert!(c.get("a").is_none());
        assert_eq!(c.stats().0, 0, "the old file is deleted");
        let short = TileCache::new(&d, 1000).with_max_age(Duration::from_millis(20));
        short.put("b", b"x");
        assert!(short.get("b").is_some());
        std::thread::sleep(Duration::from_millis(60));
        assert!(short.get("b").is_none());
        c.put("c", b"y");
        c.remove("c");
        assert!(c.get("c").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_broken_disk_is_not_an_error() {
        let d = dir("broken");
        std::fs::create_dir_all(&d).unwrap();
        // The cache folder is a file, so nothing can be written.
        let file = d.join("file");
        std::fs::write(&file, b"x").unwrap();
        let c = TileCache::new(file.join("sub"), 100);
        c.put("a", b"data");
        assert!(c.get("a").is_none());
        assert_eq!(c.stats(), (0, 0));
        assert_eq!(c.clear(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }
}
