//! A shared stacking order: fractional keys.
//!
//! The z-order of objects and the order of layers are lists. Two users who insert into a list at
//! the same time must not get a conflict, and a list cannot be an LWW property as a whole. So each
//! object and each layer has a text key, the property `order`, and the list is sorted by the key
//! (ties by ID). Keys compare as text, and between two keys there is always a third one, so an
//! object that moves between two neighbours needs one new key, and no other object changes.
//!
//! This is *fractional indexing*, in the form that the `fractional-indexing` library of Rocicorp
//! uses (the idea comes from the editor of Figma and the write-up of Evan Wallace). The code is our
//! own.
//!
//! # The form of a key
//!
//! A key is an *integer part* and a *fraction*, both written with the digits `0-9A-Za-z` (62 digits,
//! in the order of their ASCII codes, so that text order is number order).
//!
//! - The integer part starts with a *head* letter that tells its length. `a` means 2 characters in
//!   all (the head and 1 digit), `b` means 3, up to `z` with 27. `Z` means 2 characters, `Y` 3, down
//!   to `A` with 27. Letters `a`-`z` are for the keys at and after the middle, `A`-`Z` for the keys
//!   before. The first key is `a0`.
//! - The rest is the fraction. It never ends with the digit `0`. (An integer part may end with it.)
//! - The smallest integer part, `A` and 26 zeros, is not a key by itself.
//!
//! **Why this form.** To put a key *after* the last one, the integer part is counted up by one. Only
//! when the digits are all used does the head letter go up, and the length grows by one digit. So
//! 62 appends fit in the keys `a0` to `az`, the next 3 844 in `b00` to `bzz`, and so on: the key
//! grows with the logarithm of the number of appends, not with the number. Prepending is the same
//! with the head letters `Z`, `Y`, ... Inserting in the middle again and again at one place grows
//! the fraction slowly, one digit per about 5 to 6 halvings.
//!
//! Where the keys are stored: in the replica as the property `order` of an object or layer, and in
//! the document in the field `order` of the object or layer (it is kept in `extra`, so that files
//! keep it). The document list is sorted by the keys whenever the document is built from the
//! replica. When the user changes the order locally (`arrange`, `move_layer`, or a new object),
//! `docsync` finds the objects that are out of order and gives them new keys between their
//! neighbours. A key that is not valid (see [`valid_key`]) counts as no key at all.

const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const BASE: usize = 62;
/// The longest key that is valid.
pub const MAX_KEY_LEN: usize = 1024;
/// The smallest integer part (head `A` and 26 zeros). No key is just this.
const SMALLEST: &str = "A00000000000000000000000000";

fn digit_value(c: u8) -> usize {
    DIGITS.iter().position(|d| *d == c).unwrap_or(0)
}

/// The number of characters in the integer part, from its head letter.
fn integer_len(head: u8) -> Option<usize> {
    match head {
        b'a'..=b'z' => Some((head - b'a') as usize + 2),
        b'A'..=b'Z' => Some((b'Z' - head) as usize + 2),
        _ => None,
    }
}

/// True if this is a key that the rules allow: letters and digits only, at most [`MAX_KEY_LEN`]
/// characters, a head letter and as many digits as it says, no `0` at the end of the fraction, and
/// not the smallest integer part alone.
pub fn valid_key(k: &str) -> bool {
    let b = k.as_bytes();
    if b.is_empty() || b.len() > MAX_KEY_LEN || !b.iter().all(u8::is_ascii_alphanumeric) {
        return false;
    }
    let Some(n) = integer_len(b[0]) else {
        return false;
    };
    if b.len() < n || k == SMALLEST {
        return false;
    }
    b.len() == n || b[b.len() - 1] != b'0'
}

/// The integer part of a valid key.
fn integer_part(k: &str) -> &str {
    &k[..integer_len(k.as_bytes()[0]).expect("a valid key")]
}

/// A fraction between `a` and `b` (digits only, `a` may be empty, `b` is `None` for infinity).
/// Needs `a < b`. Never ends with `0`.
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

/// The integer part one higher, or `None` at the top.
fn increment_integer(x: &str) -> Option<String> {
    let mut digs: Vec<u8> = x.as_bytes()[1..].to_vec();
    let mut head = x.as_bytes()[0];
    let mut carry = true;
    for d in digs.iter_mut().rev() {
        let v = digit_value(*d) + 1;
        if v == BASE {
            *d = DIGITS[0];
        } else {
            *d = DIGITS[v];
            carry = false;
            break;
        }
    }
    if carry {
        if head == b'Z' {
            return Some("a0".into());
        }
        if head == b'z' {
            return None;
        }
        head += 1;
        if head > b'a' {
            digs.push(DIGITS[0]);
        } else {
            digs.pop();
        }
    }
    Some(format!("{}{}", head as char, String::from_utf8(digs).ok()?))
}

/// The integer part one lower, or `None` at the bottom.
fn decrement_integer(x: &str) -> Option<String> {
    let mut digs: Vec<u8> = x.as_bytes()[1..].to_vec();
    let mut head = x.as_bytes()[0];
    let mut borrow = true;
    for d in digs.iter_mut().rev() {
        let v = digit_value(*d);
        if v == 0 {
            *d = DIGITS[BASE - 1];
        } else {
            *d = DIGITS[v - 1];
            borrow = false;
            break;
        }
    }
    if borrow {
        if head == b'a' {
            return Some(format!("Z{}", DIGITS[BASE - 1] as char));
        }
        if head == b'A' {
            return None;
        }
        head -= 1;
        if head < b'Z' {
            digs.push(DIGITS[BASE - 1]);
        } else {
            digs.pop();
        }
    }
    Some(format!("{}{}", head as char, String::from_utf8(digs).ok()?))
}

/// A key between `a` and `b`, or `None` if there is none (`a >= b`, or a key is not valid).
/// `a = None` is "before everything", `b = None` is "after everything".
pub fn between(a: Option<&str>, b: Option<&str>) -> Option<String> {
    if a.is_some_and(|a| !valid_key(a)) || b.is_some_and(|b| !valid_key(b)) {
        return None;
    }
    if let (Some(a), Some(b)) = (a, b)
        && a >= b
    {
        return None;
    }
    match (a, b) {
        (None, None) => Some("a0".into()),
        (None, Some(b)) => {
            let (ib, fb) = b.split_at(integer_part(b).len());
            if ib == SMALLEST {
                return Some(format!("{ib}{}", midpoint(b"", Some(fb.as_bytes()))));
            }
            if ib < b {
                return Some(ib.to_string());
            }
            decrement_integer(ib)
        }
        (Some(a), None) => {
            let (ia, fa) = a.split_at(integer_part(a).len());
            match increment_integer(ia) {
                Some(i) => Some(i),
                None => Some(format!("{ia}{}", midpoint(fa.as_bytes(), None))),
            }
        }
        (Some(a), Some(b)) => {
            let (ia, fa) = a.split_at(integer_part(a).len());
            let (ib, fb) = b.split_at(integer_part(b).len());
            if ia == ib {
                return Some(format!(
                    "{ia}{}",
                    midpoint(fa.as_bytes(), Some(fb.as_bytes()))
                ));
            }
            match increment_integer(ia) {
                Some(i) if i.as_str() < b => Some(i),
                _ => Some(format!("{ia}{}", midpoint(fa.as_bytes(), None))),
            }
        }
    }
}

/// `n` keys in increasing order, all between `a` and `b`.
pub fn keys_between(a: Option<&str>, b: Option<&str>, n: usize) -> Vec<String> {
    if n == 0 {
        return Vec::new();
    }
    let (a, b) = (a.filter(|k| valid_key(k)), b.filter(|k| valid_key(k)));
    if let (Some(x), Some(y)) = (a, b)
        && x >= y
    {
        // The bounds are wrong: give keys after `a` that grow, so the list is still sorted.
        return keys_after(a, n);
    }
    if b.is_none() {
        return keys_after(a, n);
    }
    if a.is_none() {
        let mut out = Vec::with_capacity(n);
        let mut c = between(None, b).expect("a key before a valid key");
        out.push(c.clone());
        for _ in 1..n {
            c = between(None, Some(&c)).expect("a key before a valid key");
            out.push(c.clone());
        }
        out.reverse();
        return out;
    }
    let mid = between(a, b).expect("a key between two valid keys");
    if n == 1 {
        return vec![mid];
    }
    let left = (n - 1) / 2;
    let mut out = keys_between(a, Some(&mid), left);
    out.push(mid.clone());
    out.extend(keys_between(Some(&mid), b, n - 1 - left));
    out
}

/// `n` keys after `a`, each after the one before.
fn keys_after(a: Option<&str>, n: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(n);
    let mut c = between(a, None).expect("a key after a valid key or none");
    out.push(c.clone());
    for _ in 1..n {
        c = between(Some(&c), None).expect("a key after a valid key");
        out.push(c.clone());
    }
    out
}

/// Which items of a list keep their key: the longest run of strictly increasing keys. Items
/// without a valid key never keep. Returns a flag for each item.
pub fn keep_flags(keys: &[Option<String>]) -> Vec<bool> {
    // Patience sorting with a back pointer.
    let mut tails: Vec<usize> = Vec::new(); // index of the item that ends a run of this length
    let mut prev: Vec<Option<usize>> = vec![None; keys.len()];
    for (i, k) in keys.iter().enumerate() {
        let Some(k) = k.as_ref().filter(|k| valid_key(k)) else {
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
    fn the_first_keys_and_the_form() {
        assert_eq!(between(None, None).unwrap(), "a0");
        assert_eq!(between(Some("a0"), None).unwrap(), "a1");
        assert_eq!(between(None, Some("a0")).unwrap(), "Zz");
        assert_eq!(between(Some("a0"), Some("a1")).unwrap(), "a0V");
        assert_eq!(between(Some("az"), None).unwrap(), "b00");
        assert_eq!(between(Some("Zz"), Some("a0")), Some("ZzV".to_string()));
        assert!(valid_key("a0") && valid_key("a0V") && valid_key("Zz") && valid_key("b00"));
        for bad in ["", "~", "a", "a0V0", "a00 ", "0", "a0\u{e9}", SMALLEST] {
            assert!(!valid_key(bad), "{bad:?}");
        }
        assert!(!valid_key(&format!("a0{}", "V".repeat(MAX_KEY_LEN))));
        assert!(valid_key(&format!("a0{}", "V".repeat(MAX_KEY_LEN - 2))));
    }

    #[test]
    fn rekey_resolves_equal_keys_around_a_new_item() {
        let k = |x: &str| Some(x.to_string());
        // The new item sits between two items with the same key: the run before the last tied
        // item gets keys below it, so the list is sorted again.
        let keys = [k("a0"), None, k("a0")];
        let new = rekey(&keys);
        let mut all = keys.to_vec();
        for (i, key) in new {
            all[i] = Some(key);
        }
        let all: Vec<String> = all.into_iter().map(Option::unwrap).collect();
        assert!(all.windows(2).all(|w| w[0] < w[1]), "{all:?}");
        assert_eq!(all[2], "a0");
    }

    #[test]
    fn keys_between_neighbours() {
        let first = between(None, None).unwrap();
        let after = between(Some(&first), None).unwrap();
        let before = between(None, Some(&first)).unwrap();
        assert!(before < first && first < after);
        let mid = between(Some(&first), Some(&after)).unwrap();
        assert!(first < mid && mid < after);
        assert_eq!(between(Some("a1"), Some("a0")), None);
        assert_eq!(between(Some("a0"), Some("a0")), None);
        // A key that is not valid has no place.
        assert_eq!(between(Some("~"), None), None);
        assert_eq!(between(None, Some("a00")), None);
        let k = between(Some("a0"), Some("a1")).unwrap();
        assert!("a0" < k.as_str() && k.as_str() < "a1");
        assert!(valid_key(&k));
    }

    #[test]
    fn appending_and_prepending_grow_with_the_logarithm() {
        let mut last: Option<String> = None;
        for i in 0..1000 {
            let k = between(last.as_deref(), None).unwrap();
            assert!(last.as_deref().is_none_or(|l| l < k.as_str()), "{i}");
            assert!(valid_key(&k));
            last = Some(k);
        }
        // 1000 appends: 3 characters (62 + 3844 keys fit in at most 3).
        assert!(last.as_ref().unwrap().len() <= 3, "{last:?}");
        let mut first: Option<String> = None;
        for _ in 0..1000 {
            let k = between(None, first.as_deref()).unwrap();
            assert!(first.as_deref().is_none_or(|f| k.as_str() < f));
            assert!(valid_key(&k));
            first = Some(k);
        }
        assert!(first.as_ref().unwrap().len() <= 4, "{first:?}");
        // A hundred thousand appends are still short.
        let mut last = "a0".to_string();
        for _ in 0..100_000 {
            last = between(Some(&last), None).unwrap();
        }
        assert!(last.len() <= 5, "{last}");
    }

    #[test]
    fn the_top_and_bottom_of_the_range() {
        // The largest integer part has no key after it, so the fraction grows.
        let top = format!("z{}", "z".repeat(26));
        assert!(valid_key(&top));
        let after = between(Some(&top), None).unwrap();
        assert!(after.as_str() > top.as_str() && valid_key(&after));
        // The smallest integer part gets a fraction before.
        let low = format!("{SMALLEST}V");
        assert!(valid_key(&low));
        let before = between(None, Some(&low)).unwrap();
        assert!(before.as_str() < low.as_str() && valid_key(&before));
        // Heads in the middle of the range count up and down.
        assert_eq!(increment_integer("Zz").unwrap(), "a0");
        assert_eq!(decrement_integer("a0").unwrap(), "Zz");
        assert_eq!(increment_integer(&top), None);
        assert_eq!(decrement_integer(SMALLEST), None);
        assert_eq!(increment_integer("bzz").unwrap(), "c000");
        assert_eq!(decrement_integer("b00").unwrap(), "az");
    }

    #[test]
    fn many_inserts_at_one_place_keep_working() {
        let hi = between(None, None).unwrap();
        let mut keys = vec![hi.clone()];
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
        let ks = keys_between(None, Some("a0"), 3);
        assert!(ks.windows(2).all(|w| w[0] < w[1]) && ks[2].as_str() < "a0");
        let ks = keys_between(Some("a0"), Some("a1"), 5);
        assert!(
            ks.windows(2).all(|w| w[0] < w[1]) && ks[0].as_str() > "a0" && ks[4].as_str() < "a1"
        );
        // Wrong bounds, or keys that are not valid, still give a sorted list.
        let ks = keys_between(Some("a5"), Some("a1"), 3);
        assert!(ks.windows(2).all(|w| w[0] < w[1]) && ks[0].as_str() > "a5");
        let ks = keys_between(Some("~"), Some("!"), 3);
        assert!(ks.windows(2).all(|w| w[0] < w[1]) && ks.iter().all(|k| valid_key(k)));
    }

    #[test]
    fn rekey_keeps_the_longest_run_and_ignores_hostile_keys() {
        let k = |s: &str| Some(s.to_string());
        // a2 is out of place.
        let keys = vec![k("a0"), k("a9"), k("a2"), k("a3"), None];
        let out = rekey(&keys);
        let mut fixed = keys.clone();
        for (i, key) in &out {
            fixed[*i] = Some(key.clone());
        }
        assert!(fixed.windows(2).all(|w| w[0] < w[1]));
        assert!(out.iter().any(|(i, _)| *i == 1) && out.iter().any(|(i, _)| *i == 4));
        assert!(!out.iter().any(|(i, _)| *i == 0 || *i == 2 || *i == 3));
        // Equal keys are not increasing: one of them is changed.
        let out = rekey(&[k("a0"), k("a0"), k("a1")]);
        assert_eq!(out.len(), 1);
        assert!(rekey(&[]).is_empty());
        assert_eq!(keep_flags(&[None, None]), vec![false, false]);
        // A key of an attacker counts as no key. It never becomes a bound.
        let out = rekey(&[k("a0"), k("~"), k("a5")]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, 1);
        assert!(valid_key(&out[0].1) && "a0" < out[0].1.as_str() && out[0].1.as_str() < "a5");
        let out = rekey(&[k("zzzz\u{7f}"), k("a0")]);
        assert_eq!(out.len(), 1);
        assert_eq!(keep_flags(&[k("~")]), vec![false]);
    }

    proptest! {
        #[test]
        fn rekey_always_sorts(keys in prop::collection::vec(prop::option::of("[0-9a-zA-Z~]{1,4}"), 0..40)) {
            let out = rekey(&keys);
            let mut fixed = keys.clone();
            for (i, key) in &out {
                fixed[*i] = Some(key.clone());
            }
            prop_assert!(fixed.iter().all(|k| k.as_deref().is_some_and(valid_key)));
            prop_assert!(fixed.windows(2).all(|w| w[0] < w[1]));
            let kept = keep_flags(&keys).iter().filter(|k| **k).count();
            prop_assert_eq!(out.len(), keys.len() - kept);
        }

        #[test]
        fn between_is_between(a in prop::option::of("[a-c][0-9a-z]{1,3}"), b in prop::option::of("[a-c][0-9a-z]{1,3}")) {
            if let Some(k) = between(a.as_deref(), b.as_deref()) {
                prop_assert!(valid_key(&k));
                if let Some(a) = &a { prop_assert!(a.as_str() < k.as_str()); }
                if let Some(b) = &b { prop_assert!(k.as_str() < b.as_str()); }
            }
        }
    }
}
