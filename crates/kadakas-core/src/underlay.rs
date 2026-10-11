//! Image underlays: a georeferenced raster on a layer, such as a map background from a WMS or a
//! scanned site plan.
//!
//! The raster is a PNG (RGBA) in the model frame: it covers the box `min..max` (mm) with the top
//! row at `max.y`. The GIS code warps maps into that frame when it makes the underlay, so every
//! renderer draws the same simple axis-aligned picture. Underlays are drawn below the vector
//! objects of the drawing (see `svg`, the app canvas and the print code).

use std::sync::Arc;

use glam::DVec2;
use serde::{Deserialize, Serialize};

/// Largest side of the embedded raster in pixels. Larger rasters are scaled down when they are made.
pub const MAX_SIDE_PX: u32 = 4096;

/// PNG bytes, kept as base64 text in files. The bytes are shared, so cloning an object is cheap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PngData(pub Arc<Vec<u8>>);

impl Serialize for PngData {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64_encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for PngData {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        base64_decode(&text)
            .map(|b| PngData(Arc::new(b)))
            .ok_or_else(|| serde::de::Error::custom("the image is not valid base64"))
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

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
        let v = ALPHABET.iter().position(|a| *a == ch)? as u32;
        acc = (acc << 6) | v;
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
        if self.png.0.len() < 8 || self.png.0[..4] != [0x89, b'P', b'N', b'G'] {
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
        let dec = png::Decoder::new(std::io::Cursor::new(self.png.0.as_slice()));
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
        format!(
            "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" opacity=\"{}\" preserveAspectRatio=\"none\" href=\"data:image/png;base64,{}\"/>",
            self.min.x,
            -self.max.y,
            w,
            h,
            self.opacity,
            base64_encode(&self.png.0),
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

    fn sample() -> Underlay {
        Underlay {
            name: "map".into(),
            min: DVec2::new(0.0, 0.0),
            max: DVec2::new(1000.0, 500.0),
            width: 2,
            height: 1,
            png: PngData(Arc::new(vec![0x89, b'P', b'N', b'G', 1, 2, 3, 4])),
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
            png: PngData(Arc::new(img.to_png())),
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
            png: PngData(Arc::new(out)),
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
        cut.png = PngData(Arc::new(cut.png.0[..30].to_vec()));
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
        b.png = PngData(Arc::new(vec![1, 2, 3]));
        assert!(b.problem().unwrap().contains("PNG"));
        b = u.clone();
        b.min.x = f64::NAN;
        assert!(b.problem().is_some() && !b.is_finite());
        assert!(u.svg_element().starts_with("<image "));
    }
}
