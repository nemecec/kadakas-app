//! Image underlays: a georeferenced raster on a layer, such as a map background from a WMS or a
//! scanned site plan.
//!
//! The raster is a PNG (RGBA) in the model frame: it covers the box `min..max` (mm) with the top
//! row at `max.y`. The GIS code warps maps into that frame when it makes the underlay, so every
//! renderer draws the same simple axis-aligned picture. Underlays are drawn below the vector
//! objects of the drawing (see `svg`, the app canvas and the print code).

use std::sync::{Arc, OnceLock};

use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::blob::{BlobError, LazyBlob, MIN_BLOB_TEXT};

/// Largest side of the embedded raster in pixels. Larger rasters are scaled down when they are made.
pub const MAX_SIDE_PX: u32 = 4096;

/// A PNG of one pixel (gray, opaque): the picture that stands for a picture that could not be
/// found, when the person chose to save without it.
pub fn placeholder_png() -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, 1, 1);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    if let Ok(mut w) = enc.write_header() {
        let _ = w.write_image_data(&[128, 128, 128, 255]);
    }
    out
}

/// PNG bytes, kept as base64 text in files. The bytes are shared, so cloning an object is cheap.
///
/// The bytes are in memory, or in the file the document was opened from (DOC-21): a lazy picture
/// is read, decoded and checked against its hash when a program asks for it ([`PngData::bytes`]),
/// and is not kept.
#[derive(Clone)]
pub struct PngData(Repr);

#[derive(Clone)]
enum Repr {
    Mem {
        bytes: Arc<Vec<u8>>,
        /// The hash of the base64 text (the name of the blob), made when it is first asked for.
        hash: Arc<OnceLock<String>>,
    },
    Lazy(Arc<LazyBlob>),
}

impl std::fmt::Debug for PngData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Repr::Mem { bytes, .. } => write!(f, "PngData({} bytes)", bytes.len()),
            Repr::Lazy(l) => write!(f, "PngData({l:?})"),
        }
    }
}

impl PartialEq for PngData {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Repr::Mem { bytes: a, .. }, Repr::Mem { bytes: b, .. }) => Arc::ptr_eq(a, b) || a == b,
            (Repr::Lazy(a), Repr::Lazy(b)) => Arc::ptr_eq(a, b) || a.hash() == b.hash(),
            _ => self.text_len() == other.text_len() && self.blob_hash() == other.blob_hash(),
        }
    }
}

impl Eq for PngData {}

impl From<Vec<u8>> for PngData {
    fn from(bytes: Vec<u8>) -> Self {
        PngData::from_arc(Arc::new(bytes))
    }
}

impl PngData {
    pub fn new(bytes: Vec<u8>) -> PngData {
        PngData::from(bytes)
    }

    pub fn from_arc(bytes: Arc<Vec<u8>>) -> PngData {
        PngData(Repr::Mem {
            bytes,
            hash: Arc::default(),
        })
    }

    /// A picture that stays in the file until it is used.
    pub fn from_lazy(blob: Arc<LazyBlob>) -> PngData {
        PngData(Repr::Lazy(blob))
    }

    /// The bytes, shared. A lazy picture is read from the file and checked; the error says why it
    /// could not be (the file changed, a part is damaged).
    pub fn bytes(&self) -> Result<Arc<Vec<u8>>, BlobError> {
        match &self.0 {
            Repr::Mem { bytes, .. } => Ok(bytes.clone()),
            Repr::Lazy(l) => l.bytes(),
        }
    }

    /// The bytes if they are in memory.
    pub fn mem_bytes(&self) -> Option<&Arc<Vec<u8>>> {
        match &self.0 {
            Repr::Mem { bytes, .. } => Some(bytes),
            Repr::Lazy(_) => None,
        }
    }

    /// The number of bytes. For a picture in the file it is worked out from the length of the
    /// text, so it can be 2 bytes too many (the padding is not known without reading).
    pub fn len(&self) -> usize {
        match &self.0 {
            Repr::Mem { bytes, .. } => bytes.len(),
            Repr::Lazy(l) => l.text_len() / 4 * 3,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many clones of this picture exist (they share the bytes, or the handle to the file).
    pub fn shared_count(&self) -> usize {
        match &self.0 {
            Repr::Mem { bytes, .. } => Arc::strong_count(bytes),
            Repr::Lazy(l) => Arc::strong_count(l),
        }
    }

    /// The bytes that this picture holds in memory: 0 for a picture that stays in the file.
    pub fn resident_len(&self) -> usize {
        self.mem_bytes().map_or(0, |b| b.len())
    }

    /// True if the picture is in the file and not in memory.
    pub fn is_lazy(&self) -> bool {
        matches!(self.0, Repr::Lazy(_))
    }

    /// The lazy handle, if the picture is in the file.
    pub fn lazy(&self) -> Option<&Arc<LazyBlob>> {
        match &self.0 {
            Repr::Lazy(l) => Some(l),
            Repr::Mem { .. } => None,
        }
    }

    /// The length of the base64 text.
    pub fn text_len(&self) -> usize {
        match &self.0 {
            Repr::Mem { bytes, .. } => bytes.len().div_ceil(3) * 4,
            Repr::Lazy(l) => l.text_len(),
        }
    }

    /// The SHA-256 of the base64 text: the name of the blob (the same as in collaboration).
    pub fn blob_hash(&self) -> String {
        match &self.0 {
            Repr::Mem { bytes, hash } => hash
                .get_or_init(|| crate::blob::text_hash(&base64_encode(bytes)))
                .clone(),
            Repr::Lazy(l) => l.hash().to_string(),
        }
    }

    /// True if a save in the container form writes this picture as a blob.
    pub fn is_blob(&self) -> bool {
        self.text_len() >= MIN_BLOB_TEXT
    }

    /// The base64 text of the picture.
    pub fn base64(&self) -> Result<Arc<str>, BlobError> {
        match &self.0 {
            Repr::Mem { bytes, .. } => Ok(Arc::from(base64_encode(bytes))),
            Repr::Lazy(l) => l.text(),
        }
    }

    /// A number that is the same for clones of this picture, for a cache of decoded pictures:
    /// the address of the shared bytes, or a number made of the hash.
    pub fn cache_key(&self) -> u64 {
        match &self.0 {
            Repr::Mem { bytes, .. } => Arc::as_ptr(bytes) as usize as u64,
            Repr::Lazy(l) => {
                let h = l.hash().as_bytes();
                let mut k = 0u64;
                for b in &h[..16.min(h.len())] {
                    k = k.wrapping_mul(31).wrapping_add(u64::from(*b));
                }
                k | 1 << 63
            }
        }
    }

    /// True if the first bytes are those of a PNG file. For a picture in the file it looks at the
    /// first characters of the text, which it kept, and does not read the file.
    pub fn looks_like_png(&self) -> bool {
        match &self.0 {
            Repr::Mem { bytes, .. } => bytes.len() >= 8 && bytes[..4] == [0x89, b'P', b'N', b'G'],
            // "iVBOR" is the base64 of 0x89 'P' 'N' 'G' (the first 30 of its 32 bits).
            Repr::Lazy(l) => l.text_len() >= 12 && l.head().starts_with("iVBOR"),
        }
    }
}

/// The base64 text of some bytes, written in pieces to a formatter. A serializer that streams
/// (`collect_str`) never holds the whole text.
struct Base64Text<'a>(&'a [u8]);

impl std::fmt::Display for Base64Text<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 3 KiB of data is 4 KiB of text.
        for piece in self.0.chunks(3 * 1024) {
            f.write_str(&base64_encode(piece))?;
        }
        Ok(())
    }
}

impl Serialize for PngData {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if crate::blob::writing_refs() && self.is_blob() {
            return crate::blob::serialize_ref(&self.blob_hash(), s);
        }
        // A picture in the file is already the text.
        if let Repr::Lazy(l) = &self.0 {
            let text = l.text().map_err(serde::ser::Error::custom)?;
            return s.serialize_str(&text);
        }
        let bytes = self.bytes().map_err(serde::ser::Error::custom)?;
        s.collect_str(&Base64Text(&bytes))
    }
}

impl<'de> Deserialize<'de> for PngData {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // The visitor decodes the text where the parser holds it. No second copy of the text is made.
        struct Base64Visitor;
        impl<'de> serde::de::Visitor<'de> for Base64Visitor {
            type Value = PngData;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("base64 text or a {\"$blob\": hash} reference")
            }
            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<PngData, E> {
                // The same text read before in this load is the same memory.
                crate::blob::intern_png(text, || base64_decode(text).map(PngData::new))
                    .ok_or_else(|| E::custom("the image is not valid base64"))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<PngData, A::Error> {
                crate::blob::RefVisitor(
                    |r| match r {
                        crate::blob::Resolved::Lazy(l) => Ok(PngData::from_lazy(l)),
                        crate::blob::Resolved::Text(hash, t) => {
                            crate::blob::intern_png_of_hash(&hash, || {
                                base64_decode(&t).map(PngData::new)
                            })
                            .ok_or_else(|| "the image is not valid base64".to_string())
                        }
                    },
                    |_: &str| -> PngData { unreachable!() },
                )
                .visit_map(map)
            }
        }
        d.deserialize_any(Base64Visitor)
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// The value of each base64 letter, 255 for any other byte.
const DECODE: [u8; 256] = {
    let mut t = [255u8; 256];
    let mut i = 0;
    while i < 64 {
        t[ALPHABET[i] as usize] = i as u8;
        i += 1;
    }
    t
};

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for ch in text.bytes() {
        if ch == b'=' || ch.is_ascii_whitespace() {
            continue;
        }
        let v = DECODE[ch as usize];
        if v == 255 {
            return None;
        }
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// A georeferenced raster.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Underlay {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Lower left and upper right corner in model mm.
    pub min: DVec2,
    pub max: DVec2,
    /// Size of the raster in pixels.
    pub width: u32,
    pub height: u32,
    pub png: PngData,
    /// 0 (clear) to 1 (opaque).
    #[serde(default = "one")]
    pub opacity: f64,
    /// Where the picture came from: a source name or a file name. Never a URL with a secret.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
}

fn one() -> f64 {
    1.0
}

impl Underlay {
    pub fn problem(&self) -> Option<String> {
        if !(self.min.is_finite() && self.max.is_finite() && self.opacity.is_finite()) {
            return Some("an underlay needs finite numbers".into());
        }
        if self.max.x <= self.min.x || self.max.y <= self.min.y {
            return Some("the underlay needs a positive size".into());
        }
        if self.width == 0
            || self.height == 0
            || self.width > MAX_SIDE_PX
            || self.height > MAX_SIDE_PX
        {
            return Some(format!(
                "the raster must be 1 to {MAX_SIDE_PX} pixels per side"
            ));
        }
        if !(0.0..=1.0).contains(&self.opacity) {
            return Some("the opacity must be between 0 and 1".into());
        }
        if !self.png.looks_like_png() {
            return Some("the raster is not a PNG".into());
        }
        None
    }

    pub fn is_finite(&self) -> bool {
        self.min.is_finite() && self.max.is_finite() && self.opacity.is_finite()
    }

    /// The raster as 8-bit RGBA, rows from the top. Reduced by whole-pixel averaging until no side
    /// is longer than `max_side`. `None` if the PNG cannot be read.
    pub fn decode(&self, max_side: u32) -> Option<Rgba> {
        let bytes = self.png.bytes().ok()?;
        let dec = png::Decoder::new(std::io::Cursor::new(bytes.as_slice()));
        let mut reader = dec.read_info().ok()?;
        let mut buf = vec![0; reader.output_buffer_size()?];
        let info = reader.next_frame(&mut buf).ok()?;
        if info.bit_depth != png::BitDepth::Eight {
            return None;
        }
        let (w, h) = (info.width as usize, info.height as usize);
        let src = &buf[..info.buffer_size()];
        let mut data = Vec::with_capacity(w * h * 4);
        match info.color_type {
            png::ColorType::Rgba => data.extend_from_slice(src),
            png::ColorType::Rgb => src
                .as_chunks::<3>()
                .0
                .iter()
                .for_each(|p| data.extend_from_slice(&[p[0], p[1], p[2], 255])),
            png::ColorType::Grayscale => src
                .iter()
                .for_each(|g| data.extend_from_slice(&[*g, *g, *g, 255])),
            png::ColorType::GrayscaleAlpha => src
                .as_chunks::<2>()
                .0
                .iter()
                .for_each(|p| data.extend_from_slice(&[p[0], p[0], p[0], p[1]])),
            png::ColorType::Indexed => return None,
        }
        let mut img = Rgba {
            width: w as u32,
            height: h as u32,
            data,
        };
        while img.width.max(img.height) > max_side.max(1) && img.width.min(img.height) > 1 {
            img = img.half();
        }
        Some(img)
    }

    /// The picture as an SVG image element. The drawing uses Y = -model Y, so the top row sits at -max.y.
    pub fn svg_element(&self) -> String {
        let w = self.max.x - self.min.x;
        let h = self.max.y - self.min.y;
        let text = match self.png.base64() {
            Ok(t) => t,
            // The picture is in the file and the file cannot be read: say so in the drawing.
            Err(e) => return format!("<!-- the underlay picture is not available: {e} -->"),
        };
        format!(
            "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" opacity=\"{}\" preserveAspectRatio=\"none\" href=\"data:image/png;base64,{}\"/>",
            self.min.x, -self.max.y, w, h, self.opacity, text,
        )
    }
}

/// A decoded raster: 8-bit RGBA, rows from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Rgba {
    /// Half the size (rounded up), averaging 2 by 2 blocks.
    fn half(&self) -> Rgba {
        let (w, h) = (self.width as usize, self.height as usize);
        let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
        let mut data = Vec::with_capacity(nw * nh * 4);
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0u32; 4];
                let mut n = 0;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = (x * 2 + dx, y * 2 + dy);
                    if sx < w && sy < h {
                        let i = (sy * w + sx) * 4;
                        for (c, a) in acc.iter_mut().enumerate() {
                            *a += u32::from(self.data[i + c]);
                        }
                        n += 1;
                    }
                }
                data.extend(acc.map(|v| (v / n) as u8));
            }
        }
        Rgba {
            width: nw as u32,
            height: nh as u32,
            data,
        }
    }

    /// Encodes as PNG.
    pub fn to_png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, self.width, self.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        if let Ok(mut w) = enc.write_header() {
            let _ = w.write_image_data(&self.data);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The largest picture an underlay can have: 4096 x 4096 pixels of noise that does not
    /// compress. The result sets `MAX_BLOB_TEXT` (see `crate::blob`). Run with `--ignored
    /// --nocapture` to see the numbers.
    #[test]
    #[ignore = "measures the worst case picture: 64 MiB of noise"]
    fn the_worst_case_picture_is_inside_the_bound_of_a_blob() {
        let side = MAX_SIDE_PX as usize;
        let mut seed = 0x1234_5678_9abc_def0u64;
        let mut data = vec![0u8; side * side * 4];
        for chunk in data.chunks_mut(8) {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            chunk.copy_from_slice(&seed.to_le_bytes()[..chunk.len()]);
        }
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, side as u32, side as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&data).unwrap();
        w.finish().unwrap();
        let text = crate::underlay::base64_encode(&out);
        println!(
            "4096 x 4096 RGBA noise: PNG {} bytes ({:.1} MiB), base64 text {} bytes ({:.1} MiB), bound {} bytes ({} MiB)",
            out.len(),
            out.len() as f64 / 1048576.0,
            text.len(),
            text.len() as f64 / 1048576.0,
            crate::blob::MAX_BLOB_TEXT,
            crate::blob::MAX_BLOB_TEXT >> 20
        );
        assert!(text.len() < crate::blob::MAX_BLOB_TEXT);
        // 16 bit RGBA is twice that and is not a picture the app makes (the GIS code and the image
        // import make 8 bit RGBA); the bound is for what Kadakas writes.
    }

    fn sample() -> Underlay {
        Underlay {
            name: "map".into(),
            min: DVec2::new(0.0, 0.0),
            max: DVec2::new(1000.0, 500.0),
            width: 2,
            height: 1,
            png: PngData::new(vec![0x89, b'P', b'N', b'G', 1, 2, 3, 4]),
            opacity: 1.0,
            source: String::new(),
        }
    }

    #[test]
    fn decode_reduces_and_round_trips() {
        let mut data = Vec::new();
        for y in 0..5u8 {
            for x in 0..7u8 {
                data.extend_from_slice(&[x * 30, y * 40, 7, 255]);
            }
        }
        let img = Rgba {
            width: 7,
            height: 5,
            data,
        };
        let u = Underlay {
            width: 7,
            height: 5,
            png: PngData::new(img.to_png()),
            ..sample()
        };
        assert_eq!(u.decode(100).unwrap(), img);
        let small = u.decode(4).unwrap();
        assert_eq!((small.width, small.height), (4, 3));
        assert_eq!(small.data.len(), 4 * 3 * 4);
        // The first pixel is the mean of a 2 by 2 block.
        assert_eq!(small.data[0], (30 + 30) / 4);
        assert!(sample().decode(10).is_none());
    }

    fn encode(color: png::ColorType, depth: png::BitDepth, data: &[u8]) -> Underlay {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, 2, 1);
        enc.set_color(color);
        enc.set_depth(depth);
        if color == png::ColorType::Indexed {
            enc.set_palette(vec![10, 20, 30, 40, 50, 60]);
        }
        enc.write_header().unwrap().write_image_data(data).unwrap();
        Underlay {
            png: PngData::new(out),
            ..sample()
        }
    }

    #[test]
    fn decode_reads_every_8_bit_color_type_and_refuses_the_rest() {
        let rgb = encode(
            png::ColorType::Rgb,
            png::BitDepth::Eight,
            &[1, 2, 3, 4, 5, 6],
        );
        assert_eq!(
            rgb.decode(10).unwrap().data,
            vec![1, 2, 3, 255, 4, 5, 6, 255]
        );
        let gray = encode(png::ColorType::Grayscale, png::BitDepth::Eight, &[7, 8]);
        assert_eq!(
            gray.decode(10).unwrap().data,
            vec![7, 7, 7, 255, 8, 8, 8, 255]
        );
        let ga = encode(
            png::ColorType::GrayscaleAlpha,
            png::BitDepth::Eight,
            &[7, 100, 8, 200],
        );
        assert_eq!(
            ga.decode(10).unwrap().data,
            vec![7, 7, 7, 100, 8, 8, 8, 200]
        );
        let indexed = encode(png::ColorType::Indexed, png::BitDepth::Eight, &[0, 1]);
        assert!(
            indexed.decode(10).is_none(),
            "palette pictures are not read"
        );
        let deep = encode(
            png::ColorType::Grayscale,
            png::BitDepth::Sixteen,
            &[0, 1, 0, 2],
        );
        assert!(deep.decode(10).is_none(), "16 bit pictures are not read");
        // A cut file is not read either.
        let mut cut = rgb.clone();
        cut.png = PngData::new(cut.png.bytes().unwrap()[..30].to_vec());
        assert!(cut.decode(10).is_none());
    }

    #[test]
    fn base64_round_trip_and_errors() {
        for n in 0..20u8 {
            let data: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(base64_decode(&base64_encode(&data)).unwrap(), data);
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert!(base64_decode("@@@").is_none());
        assert_eq!(base64_decode("TW Fu\n").unwrap(), b"Man");
    }

    #[test]
    fn json_round_trip_and_checks() {
        let u = sample();
        let text = serde_json::to_string(&u).unwrap();
        assert_eq!(serde_json::from_str::<Underlay>(&text).unwrap(), u);
        assert!(
            serde_json::from_str::<Underlay>(&text.replace("\"png\":\"", "\"png\":\"@")).is_err()
        );
        assert!(u.problem().is_none() && u.is_finite());
        let mut b = u.clone();
        b.max = b.min;
        assert!(b.problem().unwrap().contains("size"));
        b = u.clone();
        b.width = 0;
        assert!(b.problem().unwrap().contains("pixels"));
        b = u.clone();
        b.opacity = 2.0;
        assert!(b.problem().unwrap().contains("opacity"));
        b = u.clone();
        b.png = PngData::new(vec![1, 2, 3]);
        assert!(b.problem().unwrap().contains("PNG"));
        b = u.clone();
        b.min.x = f64::NAN;
        assert!(b.problem().is_some() && !b.is_finite());
        assert!(u.svg_element().starts_with("<image "));
    }
}
