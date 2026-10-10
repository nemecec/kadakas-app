//! A tile cache on disk with a size limit.
//!
//! Each tile is one file. The name comes from a hash of the request address. When the files are
//! bigger than the limit, the least recently used files go first. A read marks the file as used.
//! The cache never fails a request: a problem with the disk only means that the tile is fetched again.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::Result;
use crate::http::{Fetch, Response};

/// Default limit: 256 MB.
pub const DEFAULT_LIMIT: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct TileCache {
    dir: PathBuf,
    limit: u64,
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
        }
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

    /// The bytes of a tile, if the cache has it. The tile counts as used now.
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let path = self.path(key);
        let bytes = std::fs::read(&path).ok()?;
        if let Ok(f) = std::fs::File::options().write(true).open(&path) {
            let _ = f.set_modified(SystemTime::now());
        }
        Some(bytes)
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

/// Fetches an address through the cache. The flag tells if the answer came from the cache.
/// Only good answers (status 2xx, which `Fetch` guarantees) are kept.
pub fn fetch_cached(
    cache: Option<&TileCache>,
    http: &dyn Fetch,
    url: &str,
) -> Result<(Response, bool)> {
    if let Some(bytes) = cache.and_then(|c| c.get(url)) {
        return Ok((
            Response {
                status: 200,
                content_type: String::new(),
                body: bytes,
            },
            true,
        ));
    }
    let r = http.get(url)?;
    if let Some(c) = cache {
        c.put(url, &r.body);
    }
    Ok((r, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("cadektor-gis-cache-{}-{name}", std::process::id()));
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
    fn keeps_to_the_size_limit_and_drops_the_least_used() {
        let d = dir("limit");
        let c = TileCache::new(&d, 25);
        c.put("one", &[1; 10]);
        std::thread::sleep(std::time::Duration::from_millis(30));
        c.put("two", &[2; 10]);
        std::thread::sleep(std::time::Duration::from_millis(30));
        // A read makes "one" the most recently used.
        assert!(c.get("one").is_some());
        std::thread::sleep(std::time::Duration::from_millis(30));
        c.put("three", &[3; 10]);
        assert!(c.get("two").is_none(), "the least recently used tile goes");
        assert!(c.get("one").is_some() && c.get("three").is_some());
        let (_, bytes) = c.stats();
        assert!(bytes <= 25);
        // A tile bigger than the whole cache is not kept.
        c.put("big", &[0; 100]);
        assert!(c.get("big").is_none());
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

    struct Counting(AtomicUsize);

    impl Fetch for Counting {
        fn get(&self, url: &str) -> Result<Response> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Response {
                status: 200,
                content_type: "image/png".into(),
                body: url.as_bytes().to_vec(),
            })
        }
    }

    #[test]
    fn fetch_cached_asks_the_server_once() {
        let d = dir("fetch");
        let c = TileCache::new(&d, 1000);
        let f = Counting(AtomicUsize::new(0));
        let (r, hit) = fetch_cached(Some(&c), &f, "http://x/t1").unwrap();
        assert!(!hit);
        assert_eq!(r.body, b"http://x/t1");
        let (r, hit) = fetch_cached(Some(&c), &f, "http://x/t1").unwrap();
        assert!(hit);
        assert_eq!(r.body, b"http://x/t1");
        assert_eq!(f.0.load(Ordering::SeqCst), 1);
        // Without a cache every call goes to the server.
        let (_, hit) = fetch_cached(None, &f, "http://x/t1").unwrap();
        assert!(!hit);
        assert_eq!(f.0.load(Ordering::SeqCst), 2);
        let _ = std::fs::remove_dir_all(&d);
    }
}
