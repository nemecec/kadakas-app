//! A shared stacking order: fractional keys.
//!
//! The z-order of objects and the order of layers are lists. Two users who insert into a list at
//! the same time must not get a conflict, and a list cannot be an LWW property as a whole. So each
//! object and each layer has a text key, the property `order`, and the list is sorted by the key
//! (ties by ID). A key is a string of digits from `0-9A-Za-z`. Keys compare as text, and between two
//! keys there is always a third one, so an object that moves between two neighbours needs one new
//! key, and no other object changes.
//!
//! This is *fractional indexing* (known from the editor of Figma and the write-up of Evan
//! Wallace). The code is our own. A key never ends with the lowest digit `0`, which keeps a key
//! possible before every key.
//!
//! Where the keys are stored: in the replica as the property `order` of an object or layer, and in
//! the document in the field `order` of the object or layer (it is kept in `extra`, so that files
//! keep it). The document list is sorted by the keys whenever the document is built from the
//! replica. When the user changes the order locally (`arrange`, `move_layer`, or a new object),
//! `docsync` finds the objects that are out of order and gives them new keys between their
//! neighbours.

const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const BASE: usize = 62;

fn digit_value(c: u8) -> usize {
    DIGITS.iter().position(|d| *d == c).unwrap_or(0)
}

/// A key between `a` and `b`, or `None` if `a >= b`. `a = None` is "before everything", `b = None`
/// is "after everything".
pub fn between(a: Option<&str>, b: Option<&str>) -> Option<String> {
    // A trailing lowest digit changes nothing: "1" and "10" are the same place.
    let a = a.unwrap_or("").trim_end_matches('0');
    let b = b.map(|b| b.trim_end_matches('0'));
    if b.is_some_and(|b| a >= b || b.is_empty()) {
        return None;
    }
    Some(midpoint(a.as_bytes(), b.map(str::as_bytes)))
}

/// A key that the rules allow: not empty and not ending with the lowest digit.
fn valid(k: &str) -> bool {
    !k.is_empty() && !k.ends_with('0')
}

/// `a < b` (`a` may be empty, `b` is `None` for infinity).
fn midpoint(a: &[u8], b: Option<&[u8]>) -> String {
    if let Some(b) = b {
        // The common prefix (a is padded with the lowest digit).
        let mut n = 0;
        while n < b.len() && a.get(n).copied().unwrap_or(b'0') == b[n] {
            n += 1;
        }
        if n > 0 {
            let rest_a = if a.len() > n { &a[n..] } else { &[] };
            let mut out = String::from_utf8_lossy(&b[..n]).to_string();
            out.push_str(&midpoint(rest_a, Some(&b[n..])));
            return out;
        }
    }
    let da = a.first().map_or(0, |c| digit_value(*c));
    let db = b.map_or(BASE, |b| digit_value(b[0]));
    if db - da > 1 {
        let mid = (da + db).div_ceil(2);
        return (DIGITS[mid] as char).to_string();
    }
    // The first digits are next to each other.
    match b {
        Some(b) if b.len() > 1 => (b[0] as char).to_string(),
        _ => {
            let mut out = (DIGITS[da] as char).to_string();
            out.push_str(&midpoint(if a.is_empty() { &[] } else { &a[1..] }, None));
            out
        }
    }
}

/// `n` keys in increasing order, all between `a` and `b`.
pub fn keys_between(a: Option<&str>, b: Option<&str>, n: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    let Some(mid) = between(a, b) else {
        // The bounds are wrong (a >= b): give keys after `a` that grow, so the list is still sorted.
        let mut out = Vec::new();
        let mut last = a.map(str::to_string);
        for _ in 0..n {
            let k = between(last.as_deref(), None).expect("after a key there is a key");
            last = Some(k.clone());
            out.push(k);
        }
        return out;
    };
    if n == 1 {
        return vec![mid];
    }
    let left = (n - 1) / 2;
    let mut out = keys_between(a, Some(&mid), left);
    out.push(mid.clone());
    out.extend(keys_between(Some(&mid), b, n - 1 - left));
    out
}

/// Which items of a list keep their key: the longest run of strictly increasing keys. Items
/// without a key (`None`) never keep. Returns a flag for each item.
pub fn keep_flags(keys: &[Option<String>]) -> Vec<bool> {
    // Patience sorting with a back pointer.
    let mut tails: Vec<usize> = Vec::new(); // index of the item that ends a run of this length
    let mut prev: Vec<Option<usize>> = vec![None; keys.len()];
    for (i, k) in keys.iter().enumerate() {
        let Some(k) = k.as_ref().filter(|k| valid(k)) else {
            continue;
        };
        let at = tails.partition_point(|t| keys[*t].as_deref().is_some_and(|x| x < k.as_str()));
        prev[i] = at.checked_sub(1).map(|p| tails[p]);
        if at == tails.len() {
            tails.push(i);
        } else {
            tails[at] = i;
        }
    }
    let mut keep = vec![false; keys.len()];
    let mut cur = tails.last().copied();
    while let Some(i) = cur {
        keep[i] = true;
        cur = prev[i];
    }
    keep
}

/// New keys for a list so that it is sorted by key. Items that keep a key (the longest increasing
/// run) are not changed. Returns the new key for each item that needs one, as (index, key).
pub fn rekey(keys: &[Option<String>]) -> Vec<(usize, String)> {
    let keep = keep_flags(keys);
    let mut out = Vec::new();
    let mut i = 0;
    while i < keys.len() {
        if keep[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < keys.len() && !keep[i] {
            i += 1;
        }
        let lo = start.checked_sub(1).and_then(|p| keys[p].as_deref());
        let hi = keys.get(i).and_then(|k| k.as_deref());
        for (n, k) in keys_between(lo, hi, i - start).into_iter().enumerate() {
            out.push((start + n, k));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn keys_between_neighbours() {
        let first = between(None, None).unwrap();
        let after = between(Some(&first), None).unwrap();
        let before = between(None, Some(&first)).unwrap();
        assert!(before < first && first < after);
        let mid = between(Some(&first), Some(&after)).unwrap();
        assert!(first < mid && mid < after);
        assert_eq!(between(Some("b"), Some("a")), None);
        assert_eq!(between(Some("a"), Some("a")), None);
        assert_eq!(between(None, Some("")), None);
        // Next to each other: the key gets longer.
        let k = between(Some("a"), Some("b")).unwrap();
        assert!("a" < k.as_str() && k.as_str() < "b");
        assert!(!k.ends_with('0'));
    }

    #[test]
    fn many_inserts_at_one_place_keep_working() {
        let mut keys = vec![between(None, None).unwrap()];
        let hi = keys[0].clone();
        // Always insert just before the last key: the keys get longer, but they stay ordered.
        let mut lo: Option<String> = None;
        for _ in 0..200 {
            let k = between(lo.as_deref(), Some(&hi)).unwrap();
            assert!(lo.as_deref().is_none_or(|l| l < k.as_str()) && k < hi);
            lo = Some(k.clone());
            keys.push(k);
        }
        let mut sorted = keys.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len());
    }

    #[test]
    fn evenly_spread_keys() {
        let ks = keys_between(None, None, 100);
        assert_eq!(ks.len(), 100);
        assert!(ks.windows(2).all(|w| w[0] < w[1]));
        assert!(ks.iter().all(|k| k.len() <= 3), "short keys: {}", ks[99]);
        assert!(keys_between(None, None, 0).is_empty());
        // Wrong bounds still give a sorted list.
        let ks = keys_between(Some("b"), Some("a"), 3);
        assert!(ks.windows(2).all(|w| w[0] < w[1]) && ks[0].as_str() > "b");
    }

    #[test]
    fn rekey_keeps_the_longest_run() {
        let k = |s: &str| Some(s.to_string());
        // b is out of place.
        let keys = vec![k("a"), k("z"), k("c"), k("d"), None];
        let out = rekey(&keys);
        let mut fixed = keys.clone();
        for (i, key) in &out {
            fixed[*i] = Some(key.clone());
        }
        assert!(fixed.windows(2).all(|w| w[0] < w[1]));
        assert!(out.iter().any(|(i, _)| *i == 1) && out.iter().any(|(i, _)| *i == 4));
        assert!(!out.iter().any(|(i, _)| *i == 0 || *i == 2 || *i == 3));
        // Equal keys are not increasing: one of them is changed.
        let keys = vec![k("a"), k("a"), k("b")];
        let out = rekey(&keys);
        assert_eq!(out.len(), 1);
        assert!(rekey(&[]).is_empty());
        assert_eq!(keep_flags(&[None, None]), vec![false, false]);
    }

    proptest! {
        #[test]
        fn rekey_always_sorts(keys in prop::collection::vec(prop::option::of("[0-9a-f]{1,3}"), 0..40)) {
            let out = rekey(&keys);
            let mut fixed = keys.clone();
            for (i, key) in &out {
                fixed[*i] = Some(key.clone());
            }
            prop_assert!(fixed.iter().all(Option::is_some));
            prop_assert!(fixed.windows(2).all(|w| w[0] < w[1]));
            // Few changes: never more than the items outside the longest run.
            let kept = keep_flags(&keys).iter().filter(|k| **k).count();
            prop_assert_eq!(out.len(), keys.len() - kept);
        }
    }
}
