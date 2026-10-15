//! A list whose parts are shared between its copies (structural sharing).
//!
//! The issues and the comments of a document are two long lists, and every undo step keeps the
//! list from before and after. With a `Vec` each step copied everything: with 10000 issues and
//! 100000 comments a step kept tens of megabytes. Here the list is a short list of chunks, and a
//! chunk is a short list of `Arc` items. A copy clones the short outer list (a pointer for each
//! chunk), and a change copies one chunk (a pointer for each item) and the one item that changed.
//! The undo step then keeps a few kilobytes, whatever the length of the list.
//!
//! Two copies can also be compared fast: a chunk that both hold (the same pointer) did not change.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A chunk that grows over this many items is split in two.
const SPLIT: usize = 512;
/// A chunk that is made from a list, or by a split, holds this many items.
const FILL: usize = 256;

type Chunk<T> = Arc<Vec<Arc<T>>>;

/// An ordered list of `T` with shared parts. `Clone` costs one pointer for each 256 items.
pub struct Shared<T> {
    chunks: Vec<Chunk<T>>,
    len: usize,
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Shared {
            chunks: self.chunks.clone(),
            len: self.len,
        }
    }
}

impl<T> Default for Shared<T> {
    fn default() -> Self {
        Shared {
            chunks: Vec::new(),
            len: 0,
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: PartialEq> PartialEq for Shared<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.len != other.len {
            return false;
        }
        // Chunks that both hold are equal without a look at their items. The chunks may be cut at
        // other places in the two lists, so the walk goes over the items, but an item that both
        // hold (the same pointer) is not compared.
        let mut a = self.arcs();
        let mut b = other.arcs();
        loop {
            match (a.next(), b.next()) {
                (None, None) => return true,
                (Some(x), Some(y)) => {
                    if !Arc::ptr_eq(x, y) && **x != **y {
                        return false;
                    }
                }
                _ => return false,
            }
        }
    }
}

impl<T: Serialize> Serialize for Shared<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.iter())
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Shared<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Vec::<T>::deserialize(d).map(Shared::from_vec)
    }
}

impl<T> std::ops::Index<usize> for Shared<T> {
    type Output = T;

    fn index(&self, at: usize) -> &T {
        self.get(at).expect("an item at this place")
    }
}

/// A change through the index copies the chunk and the item if some other copy holds them.
impl<T: Clone> std::ops::IndexMut<usize> for Shared<T> {
    fn index_mut(&mut self, at: usize) -> &mut T {
        self.make_mut(at)
    }
}

impl<T> From<Vec<T>> for Shared<T> {
    fn from(v: Vec<T>) -> Self {
        Shared::from_vec(v)
    }
}

impl<T> FromIterator<T> for Shared<T> {
    fn from_iter<I: IntoIterator<Item = T>>(i: I) -> Self {
        Shared::from_vec(i.into_iter().collect())
    }
}

impl<T> Shared<T> {
    pub fn new() -> Self {
        Shared::default()
    }

    /// Builds the list from a vector, in chunks of 256.
    pub fn from_vec(v: Vec<T>) -> Self {
        let len = v.len();
        let mut chunks = Vec::with_capacity(len / FILL + 1);
        let mut it = v.into_iter().peekable();
        while it.peek().is_some() {
            let chunk: Vec<Arc<T>> = it.by_ref().take(FILL).map(Arc::new).collect();
            chunks.push(Arc::new(chunk));
        }
        Shared { chunks, len }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The number of chunks (the size of an undo step is about this many pointers).
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    fn arcs(&self) -> impl Iterator<Item = &Arc<T>> {
        self.chunks.iter().flat_map(|c| c.iter())
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.arcs().map(|a| &**a)
    }

    /// The chunk and the place in it of the item at `at`. `at == len` is the end of the last chunk.
    fn locate(&self, mut at: usize) -> Option<(usize, usize)> {
        for (c, chunk) in self.chunks.iter().enumerate() {
            if at < chunk.len() {
                return Some((c, at));
            }
            at -= chunk.len();
        }
        (at == 0 && !self.chunks.is_empty()).then(|| {
            let last = self.chunks.len() - 1;
            (last, self.chunks[last].len())
        })
    }

    /// The item at a place found with [`Shared::places`].
    pub fn at_place(&self, place: (u32, u32)) -> &T {
        &self.chunks[place.0 as usize][place.1 as usize]
    }

    /// The place of each item, in order: (chunk, item in the chunk). For an index that a host
    /// builds once for a state that does not change.
    pub fn places(&self) -> impl Iterator<Item = ((u32, u32), &T)> {
        self.chunks.iter().enumerate().flat_map(|(c, chunk)| {
            chunk
                .iter()
                .enumerate()
                .map(move |(i, a)| ((c as u32, i as u32), &**a))
        })
    }

    pub fn get(&self, at: usize) -> Option<&T> {
        let (c, i) = self.locate(at)?;
        self.chunks[c].get(i).map(|a| &**a)
    }

    pub fn first(&self) -> Option<&T> {
        self.chunks.first().and_then(|c| c.first()).map(|a| &**a)
    }

    pub fn position(&self, f: impl Fn(&T) -> bool) -> Option<usize> {
        self.iter().position(f)
    }

    /// The index of the first item for which `f` is false, in a list in which `f` is true for a
    /// first part and false for the rest (like the method of a slice).
    pub fn partition_point(&self, f: impl Fn(&T) -> bool) -> usize {
        // The first chunk whose last item fails `f`.
        let c = self
            .chunks
            .partition_point(|chunk| chunk.last().is_none_or(|last| f(last)));
        let before: usize = self.chunks[..c].iter().map(|k| k.len()).sum();
        match self.chunks.get(c) {
            Some(chunk) => before + chunk.partition_point(|a| f(a)),
            None => before,
        }
    }

    /// True if every pair of neighbors passes `ok` (to check that a list is in order).
    pub fn windows_all(&self, ok: impl Fn(&T, &T) -> bool) -> bool {
        let mut prev: Option<&T> = None;
        for x in self.iter() {
            if let Some(p) = prev
                && !ok(p, x)
            {
                return false;
            }
            prev = Some(x);
        }
        true
    }

    /// The items that only one of the two lists holds: (in `old`, in `self`). A chunk that both
    /// hold is skipped, and so is an item that both hold (the same pointer) in a chunk that was
    /// copied. An item that changed is in both lists, as two different values.
    pub fn unshared<'a>(&'a self, old: &'a Shared<T>) -> (Vec<&'a T>, Vec<&'a T>) {
        let mine: HashSet<*const Vec<Arc<T>>> = self.chunks.iter().map(Arc::as_ptr).collect();
        let theirs: HashSet<*const Vec<Arc<T>>> = old.chunks.iter().map(Arc::as_ptr).collect();
        let dirty_old: Vec<&Arc<T>> = old
            .chunks
            .iter()
            .filter(|c| !mine.contains(&Arc::as_ptr(c)))
            .flat_map(|c| c.iter())
            .collect();
        let dirty_new: Vec<&Arc<T>> = self
            .chunks
            .iter()
            .filter(|c| !theirs.contains(&Arc::as_ptr(c)))
            .flat_map(|c| c.iter())
            .collect();
        let old_items: HashSet<*const T> = dirty_old.iter().map(|a| Arc::as_ptr(a)).collect();
        let new_items: HashSet<*const T> = dirty_new.iter().map(|a| Arc::as_ptr(a)).collect();
        (
            dirty_old
                .into_iter()
                .filter(|a| !new_items.contains(&Arc::as_ptr(a)))
                .map(|a| &**a)
                .collect(),
            dirty_new
                .into_iter()
                .filter(|a| !old_items.contains(&Arc::as_ptr(a)))
                .map(|a| &**a)
                .collect(),
        )
    }

    /// The number of chunks of the two lists that are not shared: (in `old`, in `self`).
    pub fn unshared_chunks(&self, old: &Shared<T>) -> (usize, usize) {
        let mine: HashSet<*const Vec<Arc<T>>> = self.chunks.iter().map(Arc::as_ptr).collect();
        let theirs: HashSet<*const Vec<Arc<T>>> = old.chunks.iter().map(Arc::as_ptr).collect();
        (
            old.chunks
                .iter()
                .filter(|c| !mine.contains(&Arc::as_ptr(c)))
                .count(),
            self.chunks
                .iter()
                .filter(|c| !theirs.contains(&Arc::as_ptr(c)))
                .count(),
        )
    }
}

impl<T: Clone> Shared<T> {
    pub fn to_vec(&self) -> Vec<T> {
        self.iter().cloned().collect()
    }

    /// Puts an item at `at` (0 to `len`).
    pub fn insert(&mut self, at: usize, item: T) {
        assert!(at <= self.len, "insert past the end");
        let (c, i) = match self.locate(at) {
            Some(place) => place,
            None => {
                self.chunks.push(Arc::new(Vec::new()));
                (0, 0)
            }
        };
        let chunk = Arc::make_mut(&mut self.chunks[c]);
        chunk.insert(i, Arc::new(item));
        if chunk.len() > SPLIT {
            let tail = chunk.split_off(FILL);
            self.chunks.insert(c + 1, Arc::new(tail));
        }
        self.len += 1;
    }

    pub fn push(&mut self, item: T) {
        self.insert(self.len, item);
    }

    /// Takes the item at `at` out.
    pub fn remove(&mut self, at: usize) -> T {
        let (c, i) = self.locate(at).filter(|_| at < self.len).expect("an item");
        let chunk = Arc::make_mut(&mut self.chunks[c]);
        let a = chunk.remove(i);
        if chunk.is_empty() {
            self.chunks.remove(c);
        }
        self.len -= 1;
        Arc::unwrap_or_clone(a)
    }

    /// Replaces the item at `at`.
    pub fn set(&mut self, at: usize, item: T) {
        let (c, i) = self.locate(at).filter(|_| at < self.len).expect("an item");
        Arc::make_mut(&mut self.chunks[c])[i] = Arc::new(item);
    }

    /// The item at `at` for a change. It is copied first if some copy of the list holds it.
    pub fn make_mut(&mut self, at: usize) -> &mut T {
        let (c, i) = self.locate(at).filter(|_| at < self.len).expect("an item");
        let chunk = Arc::make_mut(&mut self.chunks[c]);
        Arc::make_mut(&mut chunk[i])
    }

    /// Keeps the items for which `f` is true. A chunk in which all items stay is not touched.
    pub fn retain(&mut self, f: impl Fn(&T) -> bool) {
        for chunk in &mut self.chunks {
            if chunk.iter().all(|a| f(a)) {
                continue;
            }
            Arc::make_mut(chunk).retain(|a| f(a));
        }
        self.chunks.retain(|c| !c.is_empty());
        self.len = self.chunks.iter().map(|c| c.len()).sum();
    }

    /// Changes the items for which `pred` is true. Only their chunks and they are copied.
    pub fn update_where(&mut self, pred: impl Fn(&T) -> bool, change: impl Fn(&mut T)) {
        for chunk in &mut self.chunks {
            if !chunk.iter().any(|a| pred(a)) {
                continue;
            }
            for a in Arc::make_mut(chunk).iter_mut() {
                if pred(a) {
                    change(Arc::make_mut(a));
                }
            }
        }
    }

    /// Puts the list in order by a key. Chunks are made again (a repair of a file does this).
    pub fn sort_by_key<K: Ord>(&mut self, key: impl Fn(&T) -> K) {
        let mut all: Vec<Arc<T>> = self.arcs().cloned().collect();
        all.sort_by_key(|a| key(a));
        let len = all.len();
        let mut chunks = Vec::with_capacity(len / FILL + 1);
        for part in all.chunks(FILL) {
            chunks.push(Arc::new(part.to_vec()));
        }
        self.chunks = chunks;
        self.len = len;
    }

    /// Changes every item (for a repair that touches many). Chunks and items are copied.
    pub fn for_each_mut(&mut self, f: impl Fn(&mut T)) {
        for chunk in &mut self.chunks {
            for a in Arc::make_mut(chunk).iter_mut() {
                f(Arc::make_mut(a));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(n: usize) -> Shared<u32> {
        Shared::from_vec((0..n as u32).collect())
    }

    #[test]
    fn it_behaves_like_a_vec() {
        let mut s = list(1000);
        let mut v: Vec<u32> = (0..1000).collect();
        assert_eq!(s.len(), 1000);
        assert_eq!(s.chunk_count(), 4);
        // Inserts in the middle, at the ends, many in one place (this splits chunks).
        for k in 0..2000u32 {
            let at = (k as usize * 7919) % (v.len() + 1);
            s.insert(at, 10_000 + k);
            v.insert(at, 10_000 + k);
        }
        for k in 0..1200u32 {
            s.insert(500, 50_000 + k);
            v.insert(500, 50_000 + k);
        }
        s.push(7);
        v.push(7);
        assert_eq!(s.to_vec(), v);
        assert_eq!(s.len(), v.len());
        assert!(s.chunk_count() > 4);
        // Removes, sets, changes.
        for k in 0..1500usize {
            let at = (k * 31) % v.len();
            assert_eq!(s.remove(at), v.remove(at));
        }
        for k in 0..100usize {
            let at = (k * 17) % v.len();
            s.set(at, 99);
            v[at] = 99;
            *s.make_mut(at) += 1;
            v[at] += 1;
        }
        assert_eq!(s.to_vec(), v);
        for at in [0, 5, v.len() - 1] {
            assert_eq!(s.get(at), v.get(at));
        }
        assert_eq!(s.get(v.len()), None);
        assert_eq!(s.first(), v.first());
        s.retain(|x| x % 3 != 0);
        v.retain(|x| x % 3 != 0);
        assert_eq!(s.to_vec(), v);
        assert_eq!(s.len(), v.len());
        s.update_where(|x| x % 2 == 0, |x| *x += 1);
        for x in &mut v {
            if *x % 2 == 0 {
                *x += 1;
            }
        }
        assert_eq!(s.to_vec(), v);
        s.sort_by_key(|x| std::cmp::Reverse(*x));
        v.sort_by_key(|x| std::cmp::Reverse(*x));
        assert_eq!(s.to_vec(), v);
        s.for_each_mut(|x| *x = x.saturating_sub(1));
        for x in &mut v {
            *x = x.saturating_sub(1);
        }
        assert_eq!(s.to_vec(), v);
        while !s.is_empty() {
            assert_eq!(s.remove(0), v.remove(0));
        }
        assert_eq!(s.chunk_count(), 0);
        assert!(s.first().is_none());
    }

    #[test]
    fn partition_point_and_order_checks_match_a_slice() {
        let mut s: Shared<u32> = Shared::new();
        let mut v: Vec<u32> = Vec::new();
        for k in 0..3000u32 {
            let x = k * 2;
            let at = v.partition_point(|y| *y <= x);
            assert_eq!(s.partition_point(|y| *y <= x), at);
            s.insert(at, x);
            v.insert(at, x);
        }
        for probe in [0, 1, 2, 3999, 5998, 5999, 7000] {
            assert_eq!(
                s.partition_point(|y| *y < probe),
                v.partition_point(|y| *y < probe)
            );
        }
        assert!(s.windows_all(|a, b| a <= b));
        s.set(100, 1_000_000);
        assert!(!s.windows_all(|a, b| a <= b));
        assert_eq!(Shared::<u32>::new().partition_point(|_| true), 0);
    }

    #[test]
    fn a_copy_shares_and_a_change_copies_one_chunk() {
        let a = list(10_000);
        let mut b = a.clone();
        assert_eq!(a, b);
        b.set(5000, 7);
        assert_ne!(a, b);
        // One chunk is not shared; the diff looks at its items only.
        assert_eq!(b.unshared_chunks(&a), (1, 1));
        let (old, new) = b.unshared(&a);
        assert_eq!((old.len(), new.len()), (1, 1), "only the item that changed");
        assert!(old.contains(&&5000) && new.contains(&&7));
        // Equal content in other chunks is equal.
        let c = list(10_000);
        assert_eq!(a, c);
        assert_eq!(c.unshared_chunks(&a), (a.chunk_count(), a.chunk_count()));
        // An insert splits at most one chunk into two.
        let mut d = a.clone();
        for k in 0..300 {
            d.insert(100, k);
        }
        let (o, n) = d.unshared_chunks(&a);
        assert!(o <= 2 && n <= 3, "{o} {n}");
        // The places of the items.
        let places: Vec<_> = a.places().take(300).collect();
        assert_eq!(places[0].0, (0, 0));
        assert_eq!(places[256].0, (1, 0));
        assert_eq!(*a.at_place((1, 3)), 259);
    }

    #[test]
    fn serde_is_a_plain_list() {
        let s = list(600);
        let json = serde_json::to_string(&s).unwrap();
        let v: Vec<u32> = serde_json::from_str(&json).unwrap();
        assert_eq!(v.len(), 600);
        let back: Shared<u32> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        assert_eq!(format!("{:?}", list(3)), "[0, 1, 2]");
        let c: Shared<u32> = (0..3).collect();
        assert_eq!(c, Shared::from(vec![0, 1, 2]));
    }
}
