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

use std::sync::Arc;

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

/// Text that clones in constant time. A picture file is megabytes of base64 text; the document, the
/// undo steps and the change log all hold the resource, so a deep copy per command would cost
/// that much memory every time. The equality check stops at once for the same text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SharedText(Arc<str>);

impl SharedText {
    pub fn as_arc(&self) -> &Arc<str> {
        &self.0
    }
}

impl std::ops::Deref for SharedText {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl From<String> for SharedText {
    fn from(s: String) -> Self {
        SharedText(Arc::from(s))
    }
}

impl From<&str> for SharedText {
    fn from(s: &str) -> Self {
        SharedText(Arc::from(s))
    }
}

impl From<Arc<str>> for SharedText {
    fn from(s: Arc<str>) -> Self {
        SharedText(s)
    }
}

impl Serialize for SharedText {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SharedText {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(SharedText::from)
    }
}

/// A picture file with its thumbnail.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageResource {
    /// SHA-256 of the file bytes, as 64 lowercase hex digits.
    pub sha256: String,
    pub format: PictureFormat,
    pub width: u32,
    pub height: u32,
    /// The file, as base64 text.
    pub data: SharedText,
    /// A PNG of at most [`MAX_THUMB_SIDE`] pixels on a side, as base64 text. The thumbnail is a
    /// hint of the sender: programs that show the picture derive their own from the file.
    pub thumb: SharedText,
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
        // The unused bits of the last character must be zero, so a file has one text only.
        let extra = [0u32, 2, 4][pad];
        if extra > 0 && acc & ((1 << extra) - 1) != 0 {
            return Err("the base64 text is not in its canonical form".into());
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

/// The width and height in the first frame header (SOF) of a JPEG file.
pub fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    if !b.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    while i + 2 <= b.len() {
        if b[i] != 0xFF {
            return None;
        }
        // Fill bytes (0xFF) may come before the marker code.
        while i + 1 < b.len() && b[i + 1] == 0xFF {
            i += 1;
        }
        let code = *b.get(i + 1)?;
        i += 2;
        match code {
            0x01 | 0xD0..=0xD8 => continue,
            0xD9 | 0xDA => return None,
            _ => {}
        }
        let len = usize::from(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]));
        if len < 2 {
            return None;
        }
        if (0xC0..=0xCF).contains(&code) && !matches!(code, 0xC4 | 0xC8 | 0xCC) {
            let g = |k: usize| b.get(i + k).copied();
            let h = u32::from(u16::from_be_bytes([g(3)?, g(4)?]));
            let w = u32::from(u16::from_be_bytes([g(5)?, g(6)?]));
            return (w > 0 && h > 0).then_some((w, h));
        }
        i += len;
    }
    None
}

/// What the file bytes say about themselves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageFacts {
    pub sha256: String,
    pub format: PictureFormat,
    pub width: u32,
    pub height: u32,
}

/// The facts of a file given as base64 text: format, header size and SHA-256. Decoding the text
/// and hashing it costs time in proportion to the size, so a caller that sees the same text
/// again keeps the answer.
pub fn facts_of_text(text: &str) -> Result<ImageFacts, String> {
    let bytes = decode_base64(text, MAX_PICTURE_BYTES)
        .map_err(|e| format!("the picture data is not valid: {e}"))?;
    let format =
        PictureFormat::sniff(&bytes).ok_or("the picture data is not a PNG or JPEG file")?;
    let (width, height) = match format {
        PictureFormat::Png => png_size(&bytes),
        PictureFormat::Jpeg => jpeg_size(&bytes),
    }
    .ok_or("the picture header cannot be read")?;
    Ok(ImageFacts {
        sha256: sha256_hex(&bytes),
        format,
        width,
        height,
    })
}

impl ImageResource {
    /// Checks the picture without reading the file: the hash form, the sizes, the length of the
    /// text and the thumbnail header. It is cheap, so the document and the merge code run it
    /// often. [`ImageResource::verify_data`] reads the file and runs when the data first arrives.
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
        if self.data.len() > MAX_PICTURE_BYTES.div_ceil(3) * 4 || !self.data.len().is_multiple_of(4)
        {
            return Err("the picture data has a wrong length".into());
        }
        if self.thumb.len() > MAX_THUMB_BASE64 {
            return Err("the picture thumbnail is too large".into());
        }
        if !self.thumb.len().is_multiple_of(4) {
            return Err("the picture thumbnail is not valid: the text has a wrong length".into());
        }
        // The header of the PNG is in the first 24 bytes, which are the first 32 characters.
        let head = &self.thumb[..self.thumb.len().min(32)];
        let thumb = decode_base64(head, 24)
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

    /// Reads the file and checks it against the fields: format, header size and SHA-256. It
    /// costs time in proportion to the size. Run it once when the data first arrives (a command, a
    /// file load, a blob), not on every change.
    pub fn verify_data(&self) -> Result<(), String> {
        self.validate()?;
        self.check_facts(&facts_of_text(&self.data)?)
    }

    /// Compares the fields with the facts of the file.
    pub fn check_facts(&self, f: &ImageFacts) -> Result<(), String> {
        if f.format != self.format {
            return Err(format!(
                "the picture data is not a {} file",
                self.format.id()
            ));
        }
        if f.sha256 != self.sha256 {
            return Err("the picture hash does not match its data".into());
        }
        if (f.width, f.height) != (self.width, self.height) {
            return Err("the picture size does not match the size in the file".into());
        }
        Ok(())
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

    /// The resource name for a picture: `Image` and the first eight digits of the hash.
    pub fn default_name(&self) -> String {
        format!("Image {}", &self.sha256[..8.min(self.sha256.len())])
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
        data: encode_base64(&file).into(),
        thumb: encode_base64(&thumb).into(),
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
        assert!(p.default_name().starts_with("Image "));
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
        assert!(bad(&|p| p.width = 0).contains("pixels"));
        assert!(bad(&|p| (p.width, p.height) = (5000, 5000)).contains("pixels"));
        assert!(bad(&|p| p.source = "x".repeat(300)).contains("source"));
        assert!(bad(&|p| p.data = "AAA".into()).contains("length"));
        assert!(bad(&|p| p.data = "A".repeat(MAX_PICTURE_BYTES * 2).into()).contains("length"));
        assert!(bad(&|p| p.thumb = "A".repeat(MAX_THUMB_BASE64 + 4).into()).contains("large"));
        assert!(bad(&|p| p.thumb = "AAA".into()).contains("thumbnail"));
        assert!(bad(&|p| p.thumb = "AAAA".into()).contains("thumbnail"));
        assert!(bad(&|p| p.thumb = "!!!!".into()).contains("thumbnail"));
        let mut wide = sample(3);
        let mut t = wide.thumb_bytes().unwrap();
        t[16..20].copy_from_slice(&1000u32.to_be_bytes());
        wide.thumb = encode_base64(&t).into();
        assert!(wide.validate().unwrap_err().contains("thumbnail"));
    }

    #[test]
    fn verify_data_reads_the_file_and_compares_every_field() {
        let good = sample(4);
        good.verify_data().unwrap();
        let bad = |f: &dyn Fn(&mut ImageResource)| {
            let mut p = good.clone();
            f(&mut p);
            p.verify_data().unwrap_err()
        };
        assert!(bad(&|p| p.sha256 = "0".repeat(64)).contains("does not match"));
        assert!(bad(&|p| p.format = PictureFormat::Jpeg).contains("jpeg"));
        assert!(bad(&|p| p.data = "!!!!".into()).contains("data"));
        assert!(bad(&|p| p.data = encode_base64(b"GIF89a....").into()).contains("PNG or JPEG"));
        assert!(bad(&|p| p.data = encode_base64(&PNG_MAGIC).into()).contains("header"));
        // The size in the fields must be the size in the file.
        assert!(bad(&|p| (p.width, p.height) = (2, 3)).contains("size"));
        assert!(bad(&|p| (p.width, p.height) = (20, 2)).contains("size"));
        // A structural error comes first.
        assert!(bad(&|p| p.width = 0).contains("pixels"));
    }

    /// A JPEG header with the given segments, for the tests of the size scan.
    fn jpeg_with(segments: &[&[u8]]) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        for s in segments {
            v.extend_from_slice(s);
        }
        v
    }

    #[test]
    fn the_jpeg_size_comes_from_the_first_frame_header() {
        let app0 = [0xFF, 0xE0, 0, 4, 0, 0];
        let sof0 = [0xFF, 0xC0, 0, 11, 8, 0, 20, 0, 30, 3, 1, 0x22, 0];
        let sof2 = [0xFF, 0xC2, 0, 11, 8, 0, 7, 0, 9, 3, 1, 0x22, 0];
        assert_eq!(jpeg_size(&jpeg_with(&[&app0, &sof0])), Some((30, 20)));
        assert_eq!(jpeg_size(&jpeg_with(&[&sof2])), Some((9, 7)));
        // Fill bytes before a marker, and markers without a length, are skipped.
        assert_eq!(
            jpeg_size(&jpeg_with(&[
                &[0xFF, 0xFF, 0xE0, 0, 2],
                &[0xFF, 0x01],
                &sof0
            ])),
            Some((30, 20))
        );
        // The Huffman table (0xC4) is not a frame header.
        let dht = [0xFF, 0xC4, 0, 4, 0, 0];
        assert_eq!(jpeg_size(&jpeg_with(&[&dht, &sof0])), Some((30, 20)));
        // Not a JPEG, a scan or end before any frame, a bad length, a cut header, zero size.
        assert_eq!(jpeg_size(b"GIF"), None);
        assert_eq!(jpeg_size(&jpeg_with(&[&[0xFF, 0xDA, 0, 2]])), None);
        assert_eq!(jpeg_size(&jpeg_with(&[&[0xFF, 0xD9]])), None);
        assert_eq!(jpeg_size(&jpeg_with(&[&[0xFF, 0xE0, 0, 1]])), None);
        assert_eq!(jpeg_size(&jpeg_with(&[&sof0[..8]])), None);
        assert_eq!(jpeg_size(&jpeg_with(&[&[0x00, 0x01]])), None);
        assert_eq!(jpeg_size(&jpeg_with(&[&[0xFF]])), None);
        let zero = [0xFF, 0xC0, 0, 11, 8, 0, 0, 0, 30, 3, 1, 0x22, 0];
        assert_eq!(jpeg_size(&jpeg_with(&[&zero])), None);
        let facts = facts_of_text(&encode_base64(&jpeg_with(&[&sof0]))).unwrap();
        assert_eq!(
            (facts.format, facts.width, facts.height),
            (PictureFormat::Jpeg, 30, 20)
        );
    }

    #[test]
    fn shared_text_clones_without_copying() {
        let t = SharedText::from("abc".to_string());
        let c = t.clone();
        assert!(Arc::ptr_eq(t.as_arc(), c.as_arc()));
        assert_eq!(&*t, "abc");
        assert_eq!(
            SharedText::from("abc"),
            SharedText::from(Arc::<str>::from("abc"))
        );
        assert_eq!(serde_json::to_string(&t).unwrap(), "\"abc\"");
        assert_eq!(
            serde_json::from_str::<SharedText>("\"x\"").unwrap(),
            "x".into()
        );
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
        // One file has one text: the unused bits of the last character must be zero.
        assert_eq!(decode_base64("QQ==", 10).unwrap(), b"A");
        assert!(decode_base64("QR==", 10).is_err());
        assert_eq!(decode_base64("QUI=", 10).unwrap(), b"AB");
        assert!(decode_base64("QUJ=", 10).is_err());
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
