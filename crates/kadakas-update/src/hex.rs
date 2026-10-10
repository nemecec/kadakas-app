//! Lower-case hex text, for keys, signatures and hashes.

use crate::error::{Result, UpdateError};

pub fn encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 15) as usize] as char);
    }
    s
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Decodes hex of exactly `N` bytes. Upper and lower case are both fine.
pub fn decode<const N: usize>(text: &str) -> Result<[u8; N]> {
    let t = text.trim().as_bytes();
    if t.len() != N * 2 {
        return Err(UpdateError::BadKey(format!(
            "expected {} hex characters, got {}",
            N * 2,
            t.len()
        )));
    }
    let mut out = [0u8; N];
    for (i, pair) in t.chunks(2).enumerate() {
        match (nibble(pair[0]), nibble(pair[1])) {
            (Some(h), Some(l)) => out[i] = (h << 4) | l,
            _ => return Err(UpdateError::BadKey("not a hex character".into())),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let b = [0u8, 1, 0xab, 0xff];
        assert_eq!(encode(&b), "0001abff");
        assert_eq!(decode::<4>("0001ABFF").unwrap(), b);
        assert_eq!(decode::<4>(" 0001abff\n").unwrap(), b);
    }

    #[test]
    fn rejects_bad_text() {
        assert!(decode::<4>("00").is_err());
        assert!(decode::<2>("zzzz").is_err());
        assert!(decode::<2>("00zz").is_err());
    }
}
