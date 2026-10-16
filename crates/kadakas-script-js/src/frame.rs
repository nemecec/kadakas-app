//! The frames that the host and the script module exchange: a `u32` count, and for each part a
//! `u32` length and the bytes (little endian). See `c/shim.c`.

/// Builds a frame from parts.
pub fn encode(parts: &[&[u8]]) -> Vec<u8> {
    let size: usize = 4 + parts.iter().map(|p| 4 + p.len()).sum::<usize>();
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(&(parts.len() as u32).to_le_bytes());
    for p in parts {
        out.extend_from_slice(&(p.len() as u32).to_le_bytes());
        out.extend_from_slice(p);
    }
    out
}

/// Splits a frame into its parts, or `None` if it is broken.
pub fn decode(bytes: &[u8]) -> Option<Vec<&[u8]>> {
    let count = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    // A part needs 4 bytes at least, so the count cannot be larger than this.
    if count > bytes.len() / 4 {
        return None;
    }
    let mut at = 4usize;
    let mut parts = Vec::with_capacity(count);
    for _ in 0..count {
        let len = u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize;
        at += 4;
        let end = at.checked_add(len)?;
        parts.push(bytes.get(at..end)?);
        at = end;
    }
    (at == bytes.len()).then_some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_round_trips() {
        let f = encode(&[b"ab", b"", b"xyz"]);
        assert_eq!(decode(&f).unwrap(), vec![&b"ab"[..], b"", b"xyz"]);
        assert_eq!(decode(&encode(&[])).unwrap().len(), 0);
    }

    #[test]
    fn a_broken_frame_is_refused() {
        let f = encode(&[b"abc"]);
        assert!(decode(&f[..f.len() - 1]).is_none(), "short");
        let mut long = f.clone();
        long.push(0);
        assert!(decode(&long).is_none(), "trailing bytes");
        assert!(decode(&[1, 0, 0]).is_none());
        // A count that cannot fit does not allocate.
        assert!(decode(&u32::MAX.to_le_bytes()).is_none());
    }
}
