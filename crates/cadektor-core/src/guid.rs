//! IFC GUIDs: 128 bits written as 22 characters of a base64 alphabet (`IfcGloballyUniqueId`).
//!
//! The first character holds 2 bits, so it is `0` to `3`. Each of the other 21 characters holds 6 bits.
//! The GUID of an entity comes from the project GUID, a tag and a number, so the same document gives
//! the same GlobalIds in every export.

const ALPHABET: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";

/// Writes 128 bits as an IFC GUID.
pub fn encode(value: u128) -> String {
    let mut out = String::with_capacity(22);
    out.push(ALPHABET[(value >> 126) as usize & 0x3] as char);
    for i in (0..21).rev() {
        out.push(ALPHABET[(value >> (i * 6)) as usize & 0x3f] as char);
    }
    out
}

/// Reads an IFC GUID. `None` if it is not 22 characters of the alphabet, or if the first is above `3`.
pub fn decode(text: &str) -> Option<u128> {
    let bytes = text.as_bytes();
    if bytes.len() != 22 {
        return None;
    }
    let mut value: u128 = 0;
    for (i, b) in bytes.iter().enumerate() {
        let digit = ALPHABET.iter().position(|a| a == b)? as u128;
        if i == 0 && digit > 3 {
            return None;
        }
        value = (value << if i == 0 { 2 } else { 6 }) | digit;
    }
    Some(value)
}

/// True if the text is a valid IFC GUID.
pub fn is_valid(text: &str) -> bool {
    decode(text).is_some()
}

fn mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

fn fnv(seed: u64, parts: &[&[u8]]) -> u64 {
    let mut h = seed;
    for p in parts {
        for b in *p {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    mix(h)
}

/// A GUID that depends only on the inputs: `project` (the project GUID), a `tag` that names the kind of
/// entity, and a number.
pub fn derive(project: &str, tag: &str, id: u64) -> String {
    let id = id.to_le_bytes();
    let parts: [&[u8]; 3] = [project.as_bytes(), tag.as_bytes(), &id];
    let hi = fnv(0xcbf2_9ce4_8422_2325, &parts);
    let lo = fnv(0x9e37_79b9_7f4a_7c15, &parts);
    encode(((hi as u128) << 64) | lo as u128)
}

/// A new GUID that differs in every call and every process.
pub fn random() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut parts = [0u64; 2];
    for (i, p) in parts.iter_mut().enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(now);
        h.write_u64(n);
        h.write_u64(i as u64);
        *p = h.finish();
    }
    encode(((parts[0] as u128) << 64) | parts[1] as u128)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values() {
        assert_eq!(encode(0), "0000000000000000000000");
        assert_eq!(encode(u128::MAX), "3$$$$$$$$$$$$$$$$$$$$$");
        assert_eq!(encode(1), "0000000000000000000001");
        assert_eq!(encode(63), "000000000000000000000$");
        assert_eq!(encode(64), "0000000000000000000010");
        assert_eq!(encode(1u128 << 127), "2000000000000000000000");
    }

    #[test]
    fn round_trip_and_rejects_bad_text() {
        for v in [0u128, 1, 12345678901234567890, u128::MAX, 1 << 100] {
            assert_eq!(decode(&encode(v)), Some(v));
        }
        assert_eq!(decode("short"), None);
        assert_eq!(decode("4000000000000000000000"), None);
        assert_eq!(decode("000000000000000000000!"), None);
        assert!(is_valid(&random()));
        assert!(!is_valid(""));
    }

    #[test]
    fn derived_guids_are_stable_and_distinct() {
        let a = derive("p", "obj", 1);
        assert_eq!(a, derive("p", "obj", 1));
        assert!(is_valid(&a));
        assert_ne!(a, derive("p", "obj", 2));
        assert_ne!(a, derive("p", "layer", 1));
        assert_ne!(a, derive("q", "obj", 1));
        assert_ne!(random(), random());
    }
}
