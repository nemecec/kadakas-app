//! Reads binary DXF (IOP-01).
//!
//! A binary DXF file starts with the 22 bytes `AutoCAD Binary DXF\r\n\x1a\0`. Then come the same
//! pairs as in a text file, each with a group code and a value:
//!
//! - Release 12 and older write the group code in one byte. The byte 255 means that the code
//!   follows in two bytes. Release 13 and newer write every code in two bytes (little endian).
//! - The group code says how the value is stored (see [`Kind`]). A string ends with a zero byte.
//!   Numbers are little endian. A binary chunk (codes 310 to 319 and 1004) has a length byte and
//!   then the bytes.
//!
//! The reader turns the pairs into the text form that the rest of the importer reads, so a binary
//! file and the same drawing as text give the same result. It never panics and it refuses a damaged
//! file with the position in the file. A file that stops after a complete pair is read as a text
//! file that ends early.

use crate::IoError;

use super::parse::{MAX_PAIRS, MAX_STRING};

/// The first bytes of every binary DXF file (without the line end and the zero byte).
pub const SENTINEL: &[u8] = b"AutoCAD Binary DXF";
/// The whole marker: the text, a line end, the end-of-file character and a zero byte.
const MARKER_LEN: usize = 22;

/// How the value of a group code is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Str,
    Double,
    Int16,
    Int32,
    Int64,
    /// One byte, 0 or 1.
    Bool,
    /// A length byte, then the bytes.
    Chunk,
}

/// How the value of this group code is stored in a binary file.
pub fn kind_of(code: i32) -> Kind {
    match code {
        10..=59 | 110..=149 | 210..=239 | 460..=469 | 1010..=1059 => Kind::Double,
        60..=79 | 170..=179 | 270..=289 | 370..=389 | 400..=409 | 1060..=1070 => Kind::Int16,
        90..=99 | 420..=429 | 440..=459 | 1071 => Kind::Int32,
        160..=169 => Kind::Int64,
        290..=299 => Kind::Bool,
        310..=319 | 1004 => Kind::Chunk,
        _ => Kind::Str,
    }
}

/// True if the data is a binary DXF file.
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.starts_with(SENTINEL)
}

fn damaged(at: usize, why: &str) -> IoError {
    IoError::Format(format!(
        "the binary DXF file is damaged at byte {at}: {why}. Save the drawing as text DXF and import that file"
    ))
}

/// Reads one group code at `pos`. Returns the code and the position after it.
fn read_code(data: &[u8], pos: usize, wide: bool) -> Result<(i32, usize), IoError> {
    if wide {
        let b = data
            .get(pos..pos + 2)
            .ok_or_else(|| damaged(pos, "the file ends inside a group code"))?;
        return Ok((i32::from(u16::from_le_bytes([b[0], b[1]])), pos + 2));
    }
    let b = *data
        .get(pos)
        .ok_or_else(|| damaged(pos, "the file ends inside a group code"))?;
    if b == 255 {
        let w = data
            .get(pos + 1..pos + 3)
            .ok_or_else(|| damaged(pos, "the file ends inside a group code"))?;
        Ok((i32::from(u16::from_le_bytes([w[0], w[1]])), pos + 3))
    } else {
        Ok((i32::from(b), pos + 1))
    }
}

/// The raw value of one pair.
enum Raw<'a> {
    Text(&'a [u8]),
    Done(String),
}

fn take(data: &[u8], pos: usize, n: usize, code: i32) -> Result<&[u8], IoError> {
    data.get(pos..pos + n).ok_or_else(|| {
        damaged(
            pos,
            &format!("the file ends inside the value of group code {code}"),
        )
    })
}

/// Reads the pairs one by one and hands each to `f`, which says whether to go on. Returns the
/// number of pairs read and whether the last one is `0 EOF`. Nothing is kept, so the memory is
/// that of the caller.
fn scan<'a>(
    data: &'a [u8],
    mut f: impl FnMut(i32, Raw<'a>) -> bool,
) -> Result<(usize, bool), IoError> {
    if !data.starts_with(SENTINEL) {
        return Err(damaged(0, "the marker at the start is missing"));
    }
    let marker = data.get(SENTINEL.len()..MARKER_LEN);
    if marker != Some(b"\r\n\x1a\0".as_slice()) {
        return Err(damaged(
            SENTINEL.len(),
            "the marker \"AutoCAD Binary DXF\" is not followed by a line end, the character 26 and a zero byte",
        ));
    }
    let mut pos = MARKER_LEN;
    // Release 12 files have a one-byte code, newer files two. The first pair is usually
    // "0 SECTION": a zero byte and a letter (narrow), or two zero bytes (wide). A comment (999)
    // can come first: in the narrow form it starts with 255, in the wide form with 231 and 3.
    let wide = match (data.get(pos), data.get(pos + 1)) {
        (Some(0), Some(b)) => *b == 0,
        (Some(255), _) => false,
        (Some(_), Some(b)) => *b < 32,
        _ => false,
    };
    let (mut count, mut at_eof) = (0usize, false);
    while pos < data.len() {
        let start = pos;
        let Ok((code, after)) = read_code(data, pos, wide) else {
            // The file ends inside a group code: damaged, but earlier pairs are good.
            return Err(damaged(start, "the file ends inside a group code"));
        };
        pos = after;
        if count >= MAX_PAIRS {
            return Err(damaged(
                start,
                &format!("more than {MAX_PAIRS} pairs (a file of this kind is refused)"),
            ));
        }
        let raw = match kind_of(code) {
            Kind::Str => {
                let rest = data.get(pos..).unwrap_or_default();
                let len = rest
                    .iter()
                    .take(MAX_STRING + 1)
                    .position(|&b| b == 0)
                    .ok_or_else(|| {
                        if rest.len() > MAX_STRING {
                            damaged(
                                pos,
                                &format!("a text value is longer than {MAX_STRING} bytes"),
                            )
                        } else {
                            damaged(
                                pos,
                                &format!(
                                    "the text value of group code {code} has no end (the file ends)"
                                ),
                            )
                        }
                    })?;
                pos += len + 1;
                Raw::Text(&rest[..len])
            }
            Kind::Double => {
                let b = take(data, pos, 8, code)?;
                pos += 8;
                let v = f64::from_le_bytes(b.try_into().expect("8 bytes"));
                Raw::Done(if v.is_finite() {
                    format!("{v}")
                } else {
                    "0".into()
                })
            }
            Kind::Int16 => {
                let b = take(data, pos, 2, code)?;
                pos += 2;
                Raw::Done(i16::from_le_bytes([b[0], b[1]]).to_string())
            }
            Kind::Int32 => {
                let b = take(data, pos, 4, code)?;
                pos += 4;
                Raw::Done(i32::from_le_bytes(b.try_into().expect("4 bytes")).to_string())
            }
            Kind::Int64 => {
                let b = take(data, pos, 8, code)?;
                pos += 8;
                Raw::Done(i64::from_le_bytes(b.try_into().expect("8 bytes")).to_string())
            }
            Kind::Bool => {
                let b = take(data, pos, 1, code)?;
                pos += 1;
                Raw::Done(u8::from(b[0] != 0).to_string())
            }
            Kind::Chunk => {
                let n = usize::from(take(data, pos, 1, code)?[0]);
                let b = take(data, pos + 1, n, code)?;
                pos += 1 + n;
                Raw::Done(b.iter().map(|x| format!("{x:02X}")).collect())
            }
        };
        count += 1;
        at_eof = code == 0 && matches!(&raw, Raw::Text(t) if t.trim_ascii() == b"EOF");
        if !f(code, raw) {
            break;
        }
    }
    Ok((count, at_eof))
}

/// The code page that the header names (`$DWGCODEPAGE`). The scan stops at the end of the header.
fn code_page(data: &[u8]) -> Option<crate::encoding::Codec> {
    let mut found: Option<crate::encoding::Codec> = None;
    let mut after_key = false;
    let _ = scan(data, |code, v| {
        let Raw::Text(t) = v else {
            after_key = false;
            return true;
        };
        if after_key && code == 3 {
            found = crate::encoding::codec_from_dwg_code_page(&String::from_utf8_lossy(t));
            return false;
        }
        after_key = code == 9 && t.trim_ascii() == b"$DWGCODEPAGE";
        // The first ENDSEC is the end of the header.
        !(code == 0 && t.trim_ascii() == b"ENDSEC")
    });
    found
}

/// Reads the pairs of a binary DXF file as text pairs. The second value is true when the file has
/// no `EOF` at the end (it ends early).
pub fn read_pairs(data: &[u8]) -> Result<(Vec<(i32, String)>, bool), IoError> {
    let page = code_page(data);
    let text = |bytes: &[u8]| -> String {
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => match page {
                Some(enc) => enc.decode(bytes),
                None => bytes.iter().map(|&b| b as char).collect(),
            },
        }
    };
    // One pass, and the pairs are made once: the memory is that of the result.
    let mut pairs: Vec<(i32, String)> = Vec::new();
    let (_, at_eof) = scan(data, |c, v| {
        pairs.push(match v {
            Raw::Text(t) => (c, text(t)),
            Raw::Done(s) => (c, s),
        });
        true
    })?;
    Ok((pairs, !at_eof))
}

/// Writes pairs as a binary DXF file (`wide`: two-byte group codes of release 13 and newer).
/// A value that does not fit its type is written as 0. The writer exists for tests and for tools
/// that make test files.
pub fn write_pairs(pairs: &[(i32, String)], wide: bool) -> Vec<u8> {
    let mut out = SENTINEL.to_vec();
    out.extend_from_slice(b"\r\n\x1a\0");
    for (code, value) in pairs {
        if wide {
            out.extend_from_slice(&(*code as u16).to_le_bytes());
        } else if *code < 255 {
            out.push(*code as u8);
        } else {
            out.push(255);
            out.extend_from_slice(&(*code as u16).to_le_bytes());
        }
        let t = value.trim();
        match kind_of(*code) {
            Kind::Str => {
                out.extend_from_slice(value.as_bytes());
                out.push(0);
            }
            Kind::Double => out.extend_from_slice(&t.parse::<f64>().unwrap_or(0.0).to_le_bytes()),
            Kind::Int16 => out.extend_from_slice(&t.parse::<i16>().unwrap_or(0).to_le_bytes()),
            Kind::Int32 => out.extend_from_slice(&t.parse::<i32>().unwrap_or(0).to_le_bytes()),
            Kind::Int64 => out.extend_from_slice(&t.parse::<i64>().unwrap_or(0).to_le_bytes()),
            Kind::Bool => out.push(u8::from(t != "0" && !t.is_empty())),
            Kind::Chunk => {
                let bytes: Vec<u8> = (0..t.len() / 2)
                    .take(255)
                    .map(|i| u8::from_str_radix(&t[2 * i..2 * i + 2], 16).unwrap_or(0))
                    .collect();
                out.push(bytes.len() as u8);
                out.extend_from_slice(&bytes);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(items: &[(i32, &str)]) -> Vec<(i32, String)> {
        items.iter().map(|(c, v)| (*c, v.to_string())).collect()
    }

    fn sample() -> Vec<(i32, String)> {
        p(&[
            (0, "SECTION"),
            (2, "HEADER"),
            (9, "$ACADVER"),
            (1, "AC1015"),
            (9, "$INSUNITS"),
            (70, "4"),
            (0, "ENDSEC"),
            (0, "SECTION"),
            (2, "ENTITIES"),
            (0, "LINE"),
            (5, "2F"),
            (8, "Walls"),
            (10, "1.5"),
            (20, "-2.25"),
            (30, "0"),
            (90, "70000"),
            (160, "5000000000"),
            (290, "1"),
            (310, "0AFF10"),
            (1071, "123456"),
            (0, "ENDSEC"),
            (0, "EOF"),
        ])
    }

    fn norm(v: &[(i32, String)]) -> Vec<(i32, String)> {
        v.iter()
            .map(|(c, s)| match kind_of(*c) {
                Kind::Double => (*c, format!("{}", s.parse::<f64>().unwrap())),
                _ => (*c, s.clone()),
            })
            .collect()
    }

    #[test]
    fn kinds_follow_the_group_code_ranges() {
        assert_eq!(kind_of(0), Kind::Str);
        assert_eq!(kind_of(5), Kind::Str);
        assert_eq!(kind_of(10), Kind::Double);
        assert_eq!(kind_of(59), Kind::Double);
        assert_eq!(kind_of(70), Kind::Int16);
        assert_eq!(kind_of(90), Kind::Int32);
        assert_eq!(kind_of(100), Kind::Str);
        assert_eq!(kind_of(160), Kind::Int64);
        assert_eq!(kind_of(290), Kind::Bool);
        assert_eq!(kind_of(310), Kind::Chunk);
        assert_eq!(kind_of(330), Kind::Str);
        assert_eq!(kind_of(370), Kind::Int16);
        assert_eq!(kind_of(420), Kind::Int32);
        assert_eq!(kind_of(1000), Kind::Str);
        assert_eq!(kind_of(1004), Kind::Chunk);
        assert_eq!(kind_of(1010), Kind::Double);
        assert_eq!(kind_of(1070), Kind::Int16);
        assert_eq!(kind_of(1071), Kind::Int32);
    }

    #[test]
    fn round_trips_with_one_and_two_byte_codes() {
        for wide in [false, true] {
            let bytes = write_pairs(&sample(), wide);
            assert!(is_binary(&bytes));
            let (pairs, early) = read_pairs(&bytes).unwrap();
            assert!(!early);
            assert_eq!(pairs, norm(&sample()), "wide {wide}");
        }
    }

    #[test]
    fn a_comment_before_the_first_section_does_not_confuse_the_width() {
        for wide in [false, true] {
            let mut pairs = vec![(999, "made by a test".to_string())];
            pairs.extend(sample());
            let (read, _) = read_pairs(&write_pairs(&pairs, wide)).unwrap();
            assert_eq!(read[0], (999, "made by a test".to_string()), "wide {wide}");
            assert_eq!(read.len(), pairs.len());
        }
    }

    #[test]
    fn a_code_over_254_is_escaped_in_the_short_form() {
        let pairs = p(&[(0, "SECTION"), (1071, "7"), (0, "EOF")]);
        let bytes = write_pairs(&pairs, false);
        assert!(bytes.windows(3).any(|w| w == [255, 0x2f, 0x04]));
        assert_eq!(read_pairs(&bytes).unwrap().0, pairs);
    }

    #[test]
    fn a_file_without_eof_ends_early() {
        let mut s = sample();
        s.pop();
        let (pairs, early) = read_pairs(&write_pairs(&s, true)).unwrap();
        assert!(early);
        assert_eq!(pairs.len(), s.len());
    }

    #[test]
    fn a_damaged_file_is_refused_with_the_position() {
        let bytes = write_pairs(&sample(), true);
        // Cut inside the double of group code 10.
        let at = bytes
            .windows(2)
            .position(|w| w == [10, 0])
            .expect("code 10")
            + 5;
        let err = read_pairs(&bytes[..at]).unwrap_err().to_string();
        assert!(err.contains("byte"), "{err}");
        assert!(err.contains("group code 10"), "{err}");
        assert!(err.contains("text DXF"), "{err}");
        // A text value without end.
        let mut cut = write_pairs(&p(&[(0, "SECTION")]), true);
        cut.pop();
        let err = read_pairs(&cut).unwrap_err().to_string();
        assert!(err.contains("byte 24"), "{err}");
        // A bad marker.
        let mut bad = bytes.clone();
        bad[19] = b'x';
        assert!(
            read_pairs(&bad)
                .unwrap_err()
                .to_string()
                .contains("byte 18")
        );
        assert!(read_pairs(b"AutoCAD Binary DXF").is_err());
        assert!(read_pairs(b"hello").is_err());
        // Cut inside a code.
        let mut one = write_pairs(&p(&[(0, "SECTION")]), true);
        one.push(70);
        assert!(
            read_pairs(&one)
                .unwrap_err()
                .to_string()
                .contains("group code")
        );
        let mut short = write_pairs(&p(&[(0, "SECTION")]), false);
        short.push(255);
        short.push(0x2f);
        assert!(read_pairs(&short).is_err());
    }

    #[test]
    fn no_cut_of_a_file_panics() {
        let bytes = write_pairs(&sample(), true);
        for n in 0..bytes.len() {
            let _ = read_pairs(&bytes[..n]);
        }
        let bytes = write_pairs(&sample(), false);
        for n in 0..bytes.len() {
            let _ = read_pairs(&bytes[..n]);
        }
    }

    #[test]
    fn a_text_longer_than_the_limit_is_refused() {
        let mut data = write_pairs(&p(&[(0, "SECTION")]), true);
        data.extend_from_slice(&[1, 0]);
        data.extend(std::iter::repeat_n(b'a', MAX_STRING + 5));
        let err = read_pairs(&data).unwrap_err().to_string();
        assert!(err.contains("longer than"), "{err}");
    }

    #[test]
    fn text_uses_utf8_or_the_code_page() {
        let mut pairs = p(&[
            (0, "SECTION"),
            (2, "HEADER"),
            (9, "$DWGCODEPAGE"),
            (3, "ANSI_1257"),
        ]);
        let mut bytes = write_pairs(&pairs, true);
        // A text value in code page 1257: S caron, o tilde.
        bytes.extend_from_slice(&[1, 0, 0xD0, 0xF5, 0]);
        let (read, _) = read_pairs(&bytes).unwrap();
        assert_eq!(read.last().unwrap().1, "\u{160}\u{f5}");
        pairs.push((1, "K\u{e4}".into()));
        let (read, _) = read_pairs(&write_pairs(&pairs, true)).unwrap();
        assert_eq!(read.last().unwrap().1, "K\u{e4}");
        // No code page: Latin-1.
        let mut plain = write_pairs(&p(&[(0, "SECTION")]), true);
        plain.extend_from_slice(&[1, 0, 0xE4, 0]);
        assert_eq!(read_pairs(&plain).unwrap().0.last().unwrap().1, "\u{e4}");
    }

    #[test]
    fn non_finite_doubles_become_zero() {
        let mut data = write_pairs(&p(&[(0, "SECTION")]), true);
        data.extend_from_slice(&[10, 0]);
        data.extend_from_slice(&f64::NAN.to_le_bytes());
        assert_eq!(read_pairs(&data).unwrap().0.last().unwrap().1, "0");
    }
}
