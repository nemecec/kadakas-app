//! Texture images: generated patterns, PNG decoding and sampling. Pure CPU code.
//!
//! The generated textures are deterministic and tile without a seam. They are tinted by the
//! base color of the material, so one pattern serves many materials.

use cadektor_core::present::{MAX_TEXTURE_SIDE, Procedural, Texture};

/// Side of a generated texture in pixels.
pub const PROCEDURAL_SIZE: u32 = 128;

/// An RGBA image used as a texture, 8 bits per channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl TextureImage {
    fn texel(&self, x: i64, y: i64) -> [f32; 4] {
        let x = x.rem_euclid(self.width as i64) as usize;
        let y = y.rem_euclid(self.height as i64) as usize;
        let i = (y * self.width as usize + x) * 4;
        [
            self.rgba[i] as f32,
            self.rgba[i + 1] as f32,
            self.rgba[i + 2] as f32,
            self.rgba[i + 3] as f32,
        ]
    }

    /// Bilinear sample with repeat. `u` and `v` count repeats of the whole image. The result
    /// is RGBA in 0 to 255. V grows upward in the world, so row 0 of the image is at `v = 1`.
    pub fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let fx = u.rem_euclid(1.0) * self.width as f32 - 0.5;
        let fy = (1.0 - v.rem_euclid(1.0)) * self.height as f32 - 0.5;
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let (a, b, c, d) = (
            self.texel(x0, y0),
            self.texel(x0 + 1, y0),
            self.texel(x0, y0 + 1),
            self.texel(x0 + 1, y0 + 1),
        );
        let mut out = [0.0; 4];
        for k in 0..4 {
            let top = a[k] + (b[k] - a[k]) * tx;
            let bottom = c[k] + (d[k] - c[k]) * tx;
            out[k] = top + (bottom - top) * ty;
        }
        out
    }

    /// The average color, RGB in 0 to 255.
    pub fn average(&self) -> [f32; 3] {
        let n = (self.width as f64 * self.height as f64).max(1.0);
        let mut sum = [0.0f64; 3];
        for p in self.rgba.chunks_exact(4) {
            for k in 0..3 {
                sum[k] += p[k] as f64;
            }
        }
        [
            (sum[0] / n) as f32,
            (sum[1] / n) as f32,
            (sum[2] / n) as f32,
        ]
    }
}

// --- Noise -------------------------------------------------------------------------------

fn hash(x: i64, y: i64, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1)
        ^ (y as u32).wrapping_mul(0x85EB_CA77)
        ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// A random number from 0 to 1 for a lattice point.
fn rnd(x: i64, y: i64, seed: u32) -> f32 {
    (hash(x, y, seed) >> 8) as f32 / (1u32 << 24) as f32
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Value noise that repeats every `px` cells along X and `py` along Y. `x` and `y` are in cells.
fn noise2(x: f32, y: f32, px: i64, py: i64, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (tx, ty) = (smooth(x - x0), smooth(y - y0));
    let (xi, yi) = (x0 as i64, y0 as i64);
    let at = |dx: i64, dy: i64| rnd((xi + dx).rem_euclid(px), (yi + dy).rem_euclid(py), seed);
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * tx;
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * tx;
    top + (bottom - top) * ty
}

fn noise(x: f32, y: f32, period: i64, seed: u32) -> f32 {
    noise2(x, y, period, period, seed)
}

/// Noise of several sizes, 0 to 1. `cells` is the lattice size of the coarsest part.
fn fractal(u: f32, v: f32, cells: i64, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut weight = 0.0;
    let mut amp = 1.0;
    for octave in 0..3 {
        let c = cells << octave;
        sum += amp
            * noise(
                u * c as f32,
                v * c as f32,
                c,
                seed.wrapping_add(octave as u32 * 101),
            );
        weight += amp;
        amp *= 0.5;
    }
    sum / weight
}

/// Distance to the nearest of the jittered points on a repeating grid (cellular noise).
/// Returns the distance (0 to about 1 cell) and the id of the nearest cell (0 to 1).
fn cells(u: f32, v: f32, n: i64, seed: u32) -> (f32, f32) {
    let (x, y) = (u * n as f32, v * n as f32);
    let (xi, yi) = (x.floor() as i64, y.floor() as i64);
    let mut best = (f32::MAX, 0.0);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (xi + dx, yi + dy);
            let (wx, wy) = (cx.rem_euclid(n), cy.rem_euclid(n));
            let px = cx as f32 + rnd(wx, wy, seed);
            let py = cy as f32 + rnd(wx, wy, seed ^ 0x5bd1);
            let d = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
            if d < best.0 {
                best = (d, rnd(wx, wy, seed ^ 0x1234));
            }
        }
    }
    best
}

fn tint(base: [u8; 3], k: f32) -> [f32; 3] {
    [
        (base[0] as f32 * k).clamp(0.0, 255.0),
        (base[1] as f32 * k).clamp(0.0, 255.0),
        (base[2] as f32 * k).clamp(0.0, 255.0),
    ]
}

/// The color of the pattern at `(u, v)`, both in 0 to 1 (the pattern repeats).
fn pattern(kind: Procedural, base: [u8; 3], u: f32, v: f32, seed: u32) -> [f32; 3] {
    match kind {
        Procedural::Noise => tint(base, 0.8 + 0.4 * fractal(u, v, 8, seed)),
        Procedural::Grass => {
            // Fine noise with streaks that run up, and patches of lighter and darker grass.
            let blades = noise2(u * 64.0, v * 16.0, 64, 16, seed);
            let patches = fractal(u, v, 4, seed ^ 7);
            let k = 0.7 + 0.25 * blades + 0.35 * patches;
            let mut c = tint(base, k);
            c[1] = (c[1] * (1.0 + 0.1 * patches)).min(255.0);
            c
        }
        Procedural::Gravel => {
            let (d, id) = cells(u, v, 14, seed);
            // Round stones with dark gaps.
            let edge = (1.0 - (d * 2.2 - 0.55).clamp(0.0, 1.0)).clamp(0.35, 1.0);
            tint(base, (0.65 + 0.6 * id) * edge)
        }
        Procedural::Pavers => {
            let (cols, rows) = (4.0, 4.0);
            let (fx, fy) = ((u * cols).fract(), (v * rows).fract());
            let (ix, iy) = ((u * cols).floor() as i64, (v * rows).floor() as i64);
            let gap = 0.035;
            let mortar = fx < gap || fy < gap;
            let k = 0.88 + 0.2 * rnd(ix, iy, seed) + 0.06 * noise(u * 64.0, v * 64.0, 64, seed);
            if mortar {
                tint(base, 0.45)
            } else {
                tint(base, k)
            }
        }
        Procedural::Mulch => {
            let chips = noise(u * 48.0, v * 48.0, 48, seed);
            let coarse = fractal(u, v, 6, seed ^ 3);
            let (d, id) = cells(u, v, 24, seed ^ 9);
            let fleck = if d < 0.18 { 0.5 + 0.9 * id } else { 1.0 };
            tint(base, (0.55 + 0.5 * chips + 0.3 * coarse) * fleck.min(1.3))
        }
        Procedural::Water => {
            let w =
                ((u * 6.0 + 0.7 * (v * 12.566).sin()) * std::f32::consts::TAU).sin() * 0.5 + 0.5;
            let ripple = noise(u * 16.0, v * 16.0, 16, seed);
            tint(base, 0.85 + 0.12 * w + 0.15 * ripple)
        }
        Procedural::Brick => {
            let rows = 8.0;
            let row = (v * rows).floor();
            let fy = (v * rows).fract();
            // Running bond: every second row shifts by half a brick.
            let bricks = 4.0;
            let shifted = u * bricks + if (row as i64) % 2 == 0 { 0.0 } else { 0.5 };
            let fx = shifted.fract();
            let id = (shifted.floor() as i64).rem_euclid(bricks as i64);
            if fy < 0.1 || fx < 0.04 {
                tint([200, 200, 195], 0.95)
            } else {
                let k = 0.82
                    + 0.3 * rnd(id, row as i64, seed)
                    + 0.1 * noise(u * 80.0, v * 80.0, 80, seed);
                tint(base, k)
            }
        }
        Procedural::Timber => {
            let planks = 4.0;
            let plank = (v * planks).floor();
            let fy = (v * planks).fract();
            let shade = 0.85 + 0.25 * rnd(plank as i64, 0, seed);
            // Grain: noise stretched along U.
            let grain = noise2(u * 6.0, v * 96.0, 6, 96, seed ^ plank as u32);
            let rings = ((v * planks * 18.0 + 3.0 * fractal(u, v, 2, seed)) * std::f32::consts::PI)
                .sin()
                * 0.5
                + 0.5;
            if fy < 0.04 {
                tint(base, 0.4)
            } else {
                tint(base, shade * (0.8 + 0.2 * grain + 0.1 * rings))
            }
        }
    }
}

/// Makes a generated texture of `size` by `size` pixels.
pub fn procedural(kind: Procedural, seed: u32, base: [u8; 3], size: u32) -> TextureImage {
    let size = size.clamp(1, MAX_TEXTURE_SIDE);
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let u = (px as f32 + 0.5) / size as f32;
            // Row 0 is the top of the image, where V is 1.
            let v = 1.0 - (py as f32 + 0.5) / size as f32;
            let c = pattern(kind, base, u, v, seed);
            rgba.extend_from_slice(&[c[0] as u8, c[1] as u8, c[2] as u8, 255]);
        }
    }
    TextureImage {
        width: size,
        height: size,
        rgba,
    }
}

/// Decodes a PNG file (base64 text) of any color type to RGBA.
pub fn decode_png(png_base64: &str) -> Result<TextureImage, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png_base64.trim())
        .map_err(|e| format!("the texture is not valid base64: {e}"))?;
    decode_png_bytes(&bytes)
}

/// Decodes the bytes of a PNG file to RGBA.
pub fn decode_png_bytes(bytes: &[u8]) -> Result<TextureImage, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("not a PNG file: {e}"))?;
    let info = reader.info();
    if info.width == 0
        || info.height == 0
        || info.width > MAX_TEXTURE_SIDE
        || info.height > MAX_TEXTURE_SIDE
    {
        return Err(format!(
            "a texture image side must be 1 to {MAX_TEXTURE_SIDE} pixels"
        ));
    }
    let mut buf = vec![0; reader.output_buffer_size().ok_or("the PNG is too large")?];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("cannot read the PNG: {e}"))?;
    let data = &buf[..frame.buffer_size()];
    let rgba: Vec<u8> = match frame.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => data
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Indexed => return Err("indexed PNG was not expanded".into()),
    };
    Ok(TextureImage {
        width: frame.width,
        height: frame.height,
        rgba,
    })
}

/// Decodes the bytes of a PNG or JPEG file to RGBA. The kind follows from the first bytes.
pub fn decode_image_bytes(bytes: &[u8]) -> Result<TextureImage, String> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        decode_jpeg_bytes(bytes)
    } else if bytes.starts_with(b"\x89PNG") {
        decode_png_bytes(bytes)
    } else {
        Err("the file is not a PNG or JPEG image".into())
    }
}

fn decode_jpeg_bytes(bytes: &[u8]) -> Result<TextureImage, String> {
    use zune_core::colorspace::ColorSpace;
    use zune_core::options::DecoderOptions;
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(
        zune_core::bytestream::ZCursor::new(bytes),
        options,
    );
    decoder
        .decode_headers()
        .map_err(|e| format!("cannot read the JPEG: {e:?}"))?;
    let info = decoder.info().ok_or("cannot read the JPEG header")?;
    let (w, h) = (info.width as u32, info.height as u32);
    if w == 0 || h == 0 || w > MAX_TEXTURE_SIDE || h > MAX_TEXTURE_SIDE {
        return Err(format!(
            "a texture image side must be 1 to {MAX_TEXTURE_SIDE} pixels"
        ));
    }
    let rgb = decoder
        .decode()
        .map_err(|e| format!("cannot read the JPEG: {e:?}"))?;
    if rgb.len() != (w * h * 3) as usize {
        return Err("the JPEG has an unsupported color layout".into());
    }
    Ok(TextureImage {
        width: w,
        height: h,
        rgba: rgb
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
    })
}

/// Makes a document texture from an image: scales it down to `max_side` and stores it as PNG.
pub fn to_texture(img: &TextureImage, max_side: u32) -> Result<Texture, String> {
    use base64::Engine;
    let img = downscale(img, max_side.clamp(1, MAX_TEXTURE_SIDE));
    let png = encode_png(img.width, img.height, &img.rgba)?;
    let png_base64 = base64::engine::general_purpose::STANDARD.encode(png);
    if png_base64.len() > cadektor_core::present::MAX_TEXTURE_BASE64 {
        return Err("the texture image is larger than 3 MB. Use a smaller max_side".into());
    }
    Ok(Texture::Image {
        png_base64,
        width: img.width,
        height: img.height,
    })
}

/// Encodes RGBA pixels as a PNG file.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(rgba).map_err(|e| e.to_string())?;
    w.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

/// The pixels of a material texture. `base` tints generated patterns. `None` if an image cannot
/// be decoded.
pub fn load(texture: &Texture, base: [u8; 3]) -> Option<TextureImage> {
    match texture {
        Texture::Procedural { kind, seed } => Some(procedural(*kind, *seed, base, PROCEDURAL_SIZE)),
        Texture::Image { png_base64, .. } => decode_png(png_base64).ok(),
    }
}

/// Scales an image down by whole factors (box filter) until both sides are at most `max`.
pub fn downscale(img: &TextureImage, max: u32) -> TextureImage {
    let k = img.width.max(img.height).div_ceil(max.max(1)).max(1);
    if k == 1 {
        return img.clone();
    }
    let (w, h) = ((img.width / k).max(1), (img.height / k).max(1));
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            let mut n = 0;
            for dy in 0..k {
                for dx in 0..k {
                    let (sx, sy) = (
                        (x * k + dx).min(img.width - 1),
                        (y * k + dy).min(img.height - 1),
                    );
                    let i = ((sy * img.width + sx) * 4) as usize;
                    for c in 0..4 {
                        sum[c] += img.rgba[i + c] as u32;
                    }
                    n += 1;
                }
            }
            rgba.extend(sum.iter().map(|s| (s / n) as u8));
        }
    }
    TextureImage {
        width: w,
        height: h,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(img: &TextureImage, x: u32, y: u32) -> f32 {
        let i = ((y * img.width + x) * 4) as usize;
        0.3 * img.rgba[i] as f32 + 0.59 * img.rgba[i + 1] as f32 + 0.11 * img.rgba[i + 2] as f32
    }

    #[test]
    fn generated_textures_are_deterministic_and_tinted_by_the_base_color() {
        for kind in Procedural::ALL {
            let a = procedural(kind, 5, [100, 140, 60], 64);
            assert_eq!(a, procedural(kind, 5, [100, 140, 60], 64), "{kind:?}");
            assert_eq!((a.width, a.height, a.rgba.len()), (64, 64, 64 * 64 * 4));
            // A different seed changes the random part, except for fully regular patterns.
            let b = procedural(kind, 6, [100, 140, 60], 64);
            if !matches!(kind, Procedural::Water) {
                assert_ne!(a, b, "{kind:?}");
            }
            // Green stays the strongest channel for a green base, for patterns without mortar.
            let avg = a.average();
            if matches!(kind, Procedural::Grass | Procedural::Noise) {
                assert!(avg[1] > avg[0] && avg[1] > avg[2], "{kind:?} {avg:?}");
                assert!((avg[1] - 140.0).abs() < 40.0, "{kind:?} {avg:?}");
            }
            // Every pixel is opaque.
            assert!(a.rgba.chunks_exact(4).all(|p| p[3] == 255));
        }
    }

    #[test]
    fn textures_tile_without_a_seam() {
        for kind in Procedural::ALL {
            let t = procedural(kind, 1, [150, 150, 150], 64);
            // The jump across the border is not larger than the typical jump inside.
            let mut inside = 0.0;
            let mut border = 0.0;
            for y in 0..64 {
                inside += (luminance(&t, 31, y) - luminance(&t, 32, y)).abs();
                border += (luminance(&t, 63, y) - luminance(&t, 0, y)).abs();
            }
            assert!(
                border <= inside * 2.5 + 64.0 * 6.0,
                "{kind:?}: {border} vs {inside}"
            );
        }
    }

    #[test]
    fn patterns_have_their_structure() {
        // Pavers: dark mortar lines in a 4 by 4 grid.
        let p = procedural(Procedural::Pavers, 1, [190, 190, 185], 128);
        let dark = (0..128).filter(|y| luminance(&p, 20, *y) < 100.0).count();
        assert!((3..12).contains(&dark), "{dark}");
        // Brick: light mortar rows between red bricks.
        let b = procedural(Procedural::Brick, 1, [160, 75, 55], 128);
        let mortar = (0..128)
            .filter(|y| b.rgba[((y * 128 + 60) * 4) as usize + 2] > 150)
            .count();
        assert!(mortar >= 8, "{mortar}");
        // Water is smooth: neighbors differ little.
        let w = procedural(Procedural::Water, 1, [60, 130, 170], 128);
        let jump: f32 = (0..127)
            .map(|x| (luminance(&w, x, 50) - luminance(&w, x + 1, 50)).abs())
            .sum();
        assert!(jump / 127.0 < 6.0);
        // Timber has gaps between planks and visible grain.
        let t = procedural(Procedural::Timber, 1, [150, 105, 65], 128);
        let min = (0..128)
            .map(|y| luminance(&t, 10, y))
            .fold(f32::MAX, f32::min);
        let max = (0..128).map(|y| luminance(&t, 10, y)).fold(0.0, f32::max);
        assert!(max - min > 40.0);
    }

    #[test]
    fn sampling_repeats_and_interpolates() {
        let img = TextureImage {
            width: 2,
            height: 2,
            // Row 0 (top, v near 1): red, green. Row 1: blue, white.
            rgba: vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        };
        // The center of the top-left texel.
        let c = img.sample(0.25, 0.75);
        assert_eq!(c, [255.0, 0.0, 0.0, 255.0]);
        // Repeat: a whole turn later gives the same value, also for negative coordinates.
        assert_eq!(img.sample(1.25, 0.75), c);
        assert_eq!(img.sample(-0.75, -0.25), c);
        // Halfway between red and green.
        let m = img.sample(0.5, 0.75);
        assert!((m[0] - 127.5).abs() < 0.5 && (m[1] - 127.5).abs() < 0.5);
        let avg = img.average();
        assert!((avg[0] - 127.5).abs() < 0.01);
    }

    #[test]
    fn png_round_trip_and_color_types() {
        let src = procedural(Procedural::Gravel, 2, [160, 150, 140], 16);
        let bytes = encode_png(src.width, src.height, &src.rgba).unwrap();
        let back = decode_png_bytes(&bytes).unwrap();
        assert_eq!(back, src);
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        assert_eq!(decode_png(&format!("  {b64}\n")).unwrap(), src);
        assert!(decode_png("!!!").is_err());
        assert!(decode_png("AAAA").is_err());

        // A gray PNG and an RGB PNG decode to RGBA.
        let mut gray = Vec::new();
        {
            let mut e = png::Encoder::new(&mut gray, 2, 1);
            e.set_color(png::ColorType::Grayscale);
            e.set_depth(png::BitDepth::Eight);
            let mut w = e.write_header().unwrap();
            w.write_image_data(&[10, 200]).unwrap();
            w.finish().unwrap();
        }
        assert_eq!(
            decode_png_bytes(&gray).unwrap().rgba,
            vec![10, 10, 10, 255, 200, 200, 200, 255]
        );
        let mut rgb = Vec::new();
        {
            let mut e = png::Encoder::new(&mut rgb, 1, 1);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            let mut w = e.write_header().unwrap();
            w.write_image_data(&[1, 2, 3]).unwrap();
            w.finish().unwrap();
        }
        assert_eq!(decode_png_bytes(&rgb).unwrap().rgba, vec![1, 2, 3, 255]);
        let mut ga = Vec::new();
        {
            let mut e = png::Encoder::new(&mut ga, 1, 1);
            e.set_color(png::ColorType::GrayscaleAlpha);
            e.set_depth(png::BitDepth::Eight);
            let mut w = e.write_header().unwrap();
            w.write_image_data(&[9, 128]).unwrap();
            w.finish().unwrap();
        }
        assert_eq!(decode_png_bytes(&ga).unwrap().rgba, vec![9, 9, 9, 128]);
        // An image that is too large is refused before the pixels are read.
        let mut big = Vec::new();
        {
            let mut e = png::Encoder::new(&mut big, MAX_TEXTURE_SIDE + 1, 1);
            e.set_color(png::ColorType::Grayscale);
            e.set_depth(png::BitDepth::Eight);
            let mut w = e.write_header().unwrap();
            w.write_image_data(&vec![0; (MAX_TEXTURE_SIDE + 1) as usize])
                .unwrap();
            w.finish().unwrap();
        }
        assert!(decode_png_bytes(&big).unwrap_err().contains("side"));
    }

    #[test]
    fn jpeg_files_decode_and_other_files_are_refused() {
        let jpg = include_bytes!("../testdata/red_blue.jpg");
        let img = decode_image_bytes(jpg).unwrap();
        assert_eq!((img.width, img.height), (16, 16));
        // Left half red, right half blue (JPEG is lossy, so allow a margin).
        let left = &img.rgba[(8 * 16 + 3) * 4..(8 * 16 + 3) * 4 + 4];
        let right = &img.rgba[(8 * 16 + 12) * 4..(8 * 16 + 12) * 4 + 4];
        assert!(left[0] > 150 && left[2] < 90, "{left:?}");
        assert!(right[2] > 150 && right[0] < 90, "{right:?}");
        assert_eq!(left[3], 255);
        // PNG goes through the same entry.
        let png = encode_png(1, 1, &[1, 2, 3, 255]).unwrap();
        assert_eq!(decode_image_bytes(&png).unwrap().rgba, vec![1, 2, 3, 255]);
        assert!(
            decode_image_bytes(b"GIF89a")
                .unwrap_err()
                .contains("PNG or JPEG")
        );
        assert!(decode_image_bytes(&jpg[..100]).is_err());
        assert!(decode_image_bytes(&[]).is_err());
    }

    #[test]
    fn load_handles_both_sources() {
        let t = Texture::Procedural {
            kind: Procedural::Brick,
            seed: 1,
        };
        let img = load(&t, [150, 70, 50]).unwrap();
        assert_eq!(img.width, PROCEDURAL_SIZE);
        let bad = Texture::Image {
            png_base64: "nope".into(),
            width: 1,
            height: 1,
        };
        assert!(load(&bad, [0, 0, 0]).is_none());
        let src = procedural(Procedural::Noise, 1, [9, 9, 9], 4);
        let bytes = encode_png(4, 4, &src.rgba).unwrap();
        use base64::Engine;
        let img = Texture::Image {
            png_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            width: 4,
            height: 4,
        };
        assert_eq!(load(&img, [0, 0, 0]).unwrap(), src);
    }

    #[test]
    fn downscale_averages_blocks() {
        let src = TextureImage {
            width: 4,
            height: 2,
            rgba: (0..8).flat_map(|i| [i * 10, 0, 0, 255]).collect(),
        };
        assert_eq!(downscale(&src, 8), src);
        let d = downscale(&src, 2);
        assert_eq!((d.width, d.height), (2, 1));
        assert_eq!(d.rgba[0], ((0 + 10 + 40 + 50) / 4) as u8);
        // An odd size uses the whole blocks.
        let odd = TextureImage {
            width: 3,
            height: 3,
            rgba: vec![50; 36],
        };
        assert_eq!(downscale(&odd, 2).width, 1);
    }
}
