//! Pictures in a document: a PNG or JPEG file kept as a resource of type `image`.
//!
//! The resource is content addressed. It holds the SHA-256 of the file bytes, and a small PNG
//! thumbnail that the UI and the sheet output draw, so they never decode the full picture. Two
//! styles that use the same file share one resource. `kadakas-render` makes the resource from a
//! file (it decodes and scales); this module only holds the data and checks it.
//!
//! The limits keep a hostile file or peer from filling the memory: [`MAX_PICTURE_BYTES`] for the
//! file, [`MAX_PICTURE_PIXELS`] for the decoded size, and a thumbnail of at most
//! [`MAX_THUMB_SIDE`] pixels.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Largest picture file in bytes.
pub const MAX_PICTURE_BYTES: usize = 8 * 1024 * 1024;
/// Most pixels (width times height) of a picture.
pub const MAX_PICTURE_PIXELS: u64 = 16_000_000;
/// Longest side of a thumbnail in pixels.
pub const MAX_THUMB_SIDE: u32 = 160;
/// Largest thumbnail text (base64 of the PNG).
pub const MAX_THUMB_BASE64: usize = 96 * 1024;
/// Longest source name.
pub const MAX_SOURCE_CHARS: usize = 255;

/// The file format of a picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PictureFormat {
    Png,
    Jpeg,
}

impl PictureFormat {
    pub fn id(self) -> &'static str {
        match self {
            PictureFormat::Png => "png",
            PictureFormat::Jpeg => "jpeg",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            PictureFormat::Png => "image/png",
            PictureFormat::Jpeg => "image/jpeg",
        }
    }

    /// The format that the first bytes of a file show.
    pub fn sniff(bytes: &[u8]) -> Option<PictureFormat> {
        if bytes.starts_with(&PNG_MAGIC) {
            Some(PictureFormat::Png)
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(PictureFormat::Jpeg)
        } else {
            None
        }
    }
}

const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// A picture file with its thumbnail.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageResource {
    /// SHA-256 of the file bytes, as 64 lowercase hex digits.
    pub sha256: String,
    pub format: PictureFormat,
    pub width: u32,
    pub height: u32,
    /// The file, as base64 text.
    pub data: String,
    /// A PNG of at most [`MAX_THUMB_SIDE`] pixels on a side, as base64 text.
    pub thumb: String,
    /// The name of the file it came from. Never a path.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
}

/// SHA-256 as lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Strict base64 (standard alphabet, padding only at the end). Fast: one table lookup per
/// character. `max_bytes` bounds the work before any decoding is done.
pub fn decode_base64(text: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    let t = text.as_bytes();
    if !t.len().is_multiple_of(4) {
        return Err("the base64 text has a wrong length".into());
    }
    if t.len() / 4 * 3 > max_bytes + 2 {
        return Err("the base64 text is too long".into());
    }
    let mut table = [255u8; 256];
    for (i, c) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        .iter()
        .enumerate()
    {
        table[*c as usize] = i as u8;
    }
    let mut out = Vec::with_capacity(t.len() / 4 * 3);
    let quads = t.len() / 4;
    for (n, quad) in t.as_chunks::<4>().0.iter().enumerate() {
        let pad = if n + 1 == quads {
            quad.iter().rev().take_while(|c| **c == b'=').count()
        } else {
            0
        };
        if pad > 2 {
            return Err("the base64 text has wrong padding".into());
        }
        let mut acc = 0u32;
        for c in &quad[..4 - pad] {
            let v = table[*c as usize];
            if v == 255 {
                return Err("the base64 text has a character that is not allowed".into());
            }
            acc = (acc << 6) | u32::from(v);
        }
        acc <<= 6 * pad as u32;
        let bytes = acc.to_be_bytes();
        out.extend_from_slice(&bytes[1..4 - pad]);
    }
    if out.len() > max_bytes {
        return Err("the base64 text is too long".into());
    }
    Ok(out)
}

/// Base64 text of bytes.
pub fn encode_base64(bytes: &[u8]) -> String {
    crate::underlay::base64_encode(bytes)
}

/// The width and height in the header of a PNG file (the IHDR chunk).
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || !bytes.starts_with(&PNG_MAGIC) || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let be = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    Some((be(16), be(20)))
}

impl ImageResource {
    /// Checks the picture without decoding it: limits, the hash, the format marks and the
    /// thumbnail header.
    pub fn validate(&self) -> Result<(), String> {
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("the picture hash must be 64 lowercase hex digits".into());
        }
        if self.width == 0
            || self.height == 0
            || u64::from(self.width) * u64::from(self.height) > MAX_PICTURE_PIXELS
        {
            return Err(format!(
                "a picture needs 1 to {MAX_PICTURE_PIXELS} pixels (width times height)"
            ));
        }
        if self.source.chars().count() > MAX_SOURCE_CHARS {
            return Err("the picture source name is too long".into());
        }
        let bytes = decode_base64(&self.data, MAX_PICTURE_BYTES)
            .map_err(|e| format!("the picture data is not valid: {e}"))?;
        if PictureFormat::sniff(&bytes) != Some(self.format) {
            return Err(format!(
                "the picture data is not a {} file",
                self.format.id()
            ));
        }
        if sha256_hex(&bytes) != self.sha256 {
            return Err("the picture hash does not match its data".into());
        }
        if self.thumb.len() > MAX_THUMB_BASE64 {
            return Err("the picture thumbnail is too large".into());
        }
        let thumb = decode_base64(&self.thumb, MAX_THUMB_BASE64)
            .map_err(|e| format!("the picture thumbnail is not valid: {e}"))?;
        match png_size(&thumb) {
            Some((w, h))
                if (1..=MAX_THUMB_SIDE).contains(&w) && (1..=MAX_THUMB_SIDE).contains(&h) =>
            {
                Ok(())
            }
            _ => Err(format!(
                "the picture thumbnail must be a PNG of 1 to {MAX_THUMB_SIDE} pixels on a side"
            )),
        }
    }

    /// The file bytes. `None` if the data is not valid.
    pub fn bytes(&self) -> Option<Vec<u8>> {
        decode_base64(&self.data, MAX_PICTURE_BYTES).ok()
    }

    /// The thumbnail PNG bytes. `None` if the data is not valid.
    pub fn thumb_bytes(&self) -> Option<Vec<u8>> {
        decode_base64(&self.thumb, MAX_THUMB_BASE64).ok()
    }

    /// The file size in bytes, computed from the text without decoding it.
    pub fn byte_len(&self) -> usize {
        let pad = self.data.bytes().rev().take_while(|b| *b == b'=').count();
        (self.data.len() / 4 * 3).saturating_sub(pad)
    }

    /// The resource name for a picture: `Picture` and the first eight digits of the hash.
    pub fn default_name(&self) -> String {
        format!("Picture {}", &self.sha256[..8.min(self.sha256.len())])
    }
}

/// A tiny resource for tests of this and other crates. The file is a PNG header, not a
/// decodable image: the core only checks marks and hashes. `seed` makes different hashes.
#[doc(hidden)]
pub fn header_only_sample(seed: u8) -> ImageResource {
    let mut file = PNG_MAGIC.to_vec();
    file.extend_from_slice(&[0, 0, 0, 13]);
    file.extend_from_slice(b"IHDR");
    file.extend_from_slice(&[0, 0, 0, 2, 0, 0, 0, 2, 8, 6, 0, 0, 0]);
    file.push(seed);
    let thumb = file.clone();
    ImageResource {
        sha256: sha256_hex(&file),
        format: PictureFormat::Png,
        width: 2,
        height: 2,
        data: encode_base64(&file),
        thumb: encode_base64(&thumb),
        source: "leaf.png".into(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub use super::header_only_sample as sample;

    #[test]
    fn a_valid_picture_passes_and_gives_its_bytes() {
        let p = sample(1);
        p.validate().unwrap();
        assert_eq!(p.bytes().unwrap().len(), p.byte_len());
        assert_eq!(p.thumb_bytes().unwrap(), p.bytes().unwrap());
        assert!(p.default_name().starts_with("Picture "));
        assert_eq!(p.format.mime(), "image/png");
        assert_eq!(PictureFormat::Jpeg.id(), "jpeg");
        assert_eq!(PictureFormat::Jpeg.mime(), "image/jpeg");
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(serde_json::from_value::<ImageResource>(json).unwrap(), p);
    }

    #[test]
    fn a_bad_picture_is_refused() {
        let good = sample(2);
        let bad = |f: &dyn Fn(&mut ImageResource)| {
            let mut p = good.clone();
            f(&mut p);
            p.validate().unwrap_err()
        };
        assert!(bad(&|p| p.sha256 = "ABC".into()).contains("hash"));
        assert!(bad(&|p| p.sha256 = "g".repeat(64)).contains("hash"));
        assert!(bad(&|p| p.sha256 = "0".repeat(64)).contains("does not match"));
        assert!(bad(&|p| p.width = 0).contains("pixels"));
        assert!(bad(&|p| (p.width, p.height) = (5000, 5000)).contains("pixels"));
        assert!(bad(&|p| p.source = "x".repeat(300)).contains("source"));
        assert!(bad(&|p| p.data = "!!!!".into()).contains("data"));
        assert!(bad(&|p| p.format = PictureFormat::Jpeg).contains("jpeg"));
        assert!(bad(&|p| p.thumb = "A".repeat(MAX_THUMB_BASE64 + 4)).contains("large"));
        assert!(bad(&|p| p.thumb = "AAAA".into()).contains("thumbnail"));
        let mut wide = sample(3);
        let mut t = wide.thumb_bytes().unwrap();
        t[16..20].copy_from_slice(&1000u32.to_be_bytes());
        wide.thumb = encode_base64(&t);
        assert!(wide.validate().unwrap_err().contains("thumbnail"));
    }

    #[test]
    fn base64_is_strict_and_bounded() {
        for len in 0..9usize {
            let data: Vec<u8> = (0..len as u8).collect();
            assert_eq!(decode_base64(&encode_base64(&data), 100).unwrap(), data);
        }
        assert!(decode_base64("AAA", 10).is_err());
        assert!(decode_base64("AA=A", 10).is_err());
        assert!(decode_base64("A===", 10).is_err());
        assert!(decode_base64("AAAAAAAA", 3).is_err());
        assert!(decode_base64("AA\nA", 10).is_err());
        assert_eq!(decode_base64("", 0).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn sniff_and_png_size_read_headers() {
        assert_eq!(
            PictureFormat::sniff(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(PictureFormat::Jpeg)
        );
        assert_eq!(PictureFormat::sniff(b"GIF89a"), None);
        assert_eq!(png_size(&sample(0).bytes().unwrap()), Some((2, 2)));
        assert_eq!(png_size(b"short"), None);
        assert_eq!(sha256_hex(b"abc").len(), 64);
    }
}
