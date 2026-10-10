//! Small bounded caches, owned by a session.
//!
//! [`LruCache`] holds at most `capacity` results; when it is full, the least recently used one
//! goes first. [`RevCache`] is an `LruCache` for results that are valid for one document
//! revision: it forgets everything when another revision comes. The revision changes with every
//! command, undo and redo. Every session owns its caches, so no result crosses from one
//! document to another.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

/// Results by key. Bounded, least recently used first out.
#[derive(Debug)]
pub struct LruCache<K, V> {
    capacity: usize,
    map: HashMap<K, V>,
    /// Keys in the order of use, the oldest first.
    used: VecDeque<K>,
}

impl<K: Hash + Eq + Clone, V: Clone> LruCache<K, V> {
    /// A cache for at most `capacity` results (at least one).
    pub fn new(capacity: usize) -> Self {
        LruCache {
            capacity: capacity.max(1),
            map: HashMap::new(),
            used: VecDeque::new(),
        }
    }

    /// Number of results held.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.map.clear();
        self.used.clear();
    }

    fn touch(&mut self, key: &K) {
        self.used.retain(|k| k != key);
        self.used.push_back(key.clone());
    }

    /// The result for `key`, which becomes the most recently used one.
    pub fn get(&mut self, key: &K) -> Option<V> {
        let v = self.map.get(key)?.clone();
        self.touch(key);
        Some(v)
    }

    /// Stores a result. When the cache is full, the least recently used result goes.
    pub fn insert(&mut self, key: K, value: V) {
        while self.map.len() >= self.capacity && !self.map.contains_key(&key) {
            match self.used.pop_front() {
                Some(old) => {
                    self.map.remove(&old);
                }
                None => break,
            }
        }
        self.touch(&key);
        self.map.insert(key, value);
    }
}

/// Results by key for one revision. Bounded, least recently used first out.
#[derive(Debug)]
pub struct RevCache<K, V> {
    revision: Option<u64>,
    inner: LruCache<K, V>,
}

impl<K: Hash + Eq + Clone, V: Clone> RevCache<K, V> {
    /// A cache for at most `capacity` results (at least one).
    pub fn new(capacity: usize) -> Self {
        RevCache {
            revision: None,
            inner: LruCache::new(capacity),
        }
    }

    /// The revision of the results in the cache.
    pub fn revision(&self) -> Option<u64> {
        self.revision
    }

    /// Number of results held.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.revision = None;
        self.inner.clear();
    }

    /// The result for `key` at `revision`. Another revision empties the cache first.
    pub fn get(&mut self, revision: u64, key: &K) -> Option<V> {
        if self.revision != Some(revision) {
            self.inner.clear();
            self.revision = Some(revision);
        }
        self.inner.get(key)
    }

    /// Stores a result. A result for a revision that is not the current one of the cache is
    /// dropped: it is old (another caller moved on while this one calculated).
    pub fn insert(&mut self, revision: u64, key: K, value: V) {
        if self.revision == Some(revision) {
            self.inner.insert(key, value);
        }
    }

    /// The result for `key` at `revision`; `make` runs when there is none.
    pub fn get_or_insert_with(&mut self, revision: u64, key: K, make: impl FnOnce() -> V) -> V {
        if let Some(v) = self.get(revision, &key) {
            return v;
        }
        let v = make();
        self.insert(revision, key, v.clone());
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_live_for_one_revision() {
        let mut c: RevCache<u32, String> = RevCache::new(4);
        assert!(c.is_empty() && c.revision().is_none());
        let mut runs = 0;
        for _ in 0..3 {
            let v = c.get_or_insert_with(1, 7, || {
                runs += 1;
                "a".into()
            });
            assert_eq!(v, "a");
        }
        assert_eq!(runs, 1);
        assert_eq!(c.len(), 1);
        // A new revision forgets the old result.
        c.get_or_insert_with(2, 7, || {
            runs += 1;
            "b".into()
        });
        assert_eq!(runs, 2);
        assert_eq!(c.revision(), Some(2));
        assert_eq!(c.get(2, &7).as_deref(), Some("b"));
        // A late insert for an old revision is dropped.
        c.insert(1, 8, "old".into());
        assert_eq!(c.get(2, &8), None);
        c.clear();
        assert!(c.is_empty() && c.revision().is_none());
        c.insert(2, 1, "x".into());
        assert!(c.is_empty(), "no revision is set before the first get");
    }

    #[test]
    fn an_lru_cache_is_bounded() {
        let mut c: LruCache<u32, u32> = LruCache::new(2);
        assert!(c.is_empty());
        c.insert(1, 1);
        c.insert(2, 2);
        assert_eq!(c.get(&1), Some(1));
        c.insert(3, 3);
        assert_eq!((c.len(), c.get(&2)), (2, None));
        assert_eq!((c.get(&1), c.get(&3)), (Some(1), Some(3)));
        c.clear();
        assert!(c.is_empty());
    }

    #[test]
    fn the_least_recently_used_result_goes_first() {
        let mut c: RevCache<u32, u32> = RevCache::new(2);
        c.get(1, &0);
        c.insert(1, 1, 10);
        c.insert(1, 2, 20);
        // Use 1 again, so 2 is the oldest.
        assert_eq!(c.get(1, &1), Some(10));
        c.insert(1, 3, 30);
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(1, &2), None);
        assert_eq!(c.get(1, &1), Some(10));
        assert_eq!(c.get(1, &3), Some(30));
        // Replacing a key does not evict another one.
        c.insert(1, 3, 31);
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(1, &3), Some(31));
        assert_eq!(RevCache::<u32, u32>::new(0).capacity(), 1);
    }
}
