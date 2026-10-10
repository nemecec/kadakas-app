//! Pictures: decoding PNG and JPEG, sampling, and the affine map from pixels to the world.

use cadektor_core::DVec2;
pub use cadektor_core::underlay::Rgba;

use crate::tiles::Bbox;
use crate::{GisError, Result};

/// Most pixels of a picture that is decoded (64 megapixels).
pub const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// Decodes a PNG or JPEG (the format comes from the bytes, not from the name).
pub fn decode_image(bytes: &[u8]) -> Result<Rgba> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| GisError::Format(format!("cannot read the picture: {e}")))?;
    let (w, h) = reader
        .into_dimensions()
        .map_err(|e| GisError::Format(format!("cannot read the picture: {e}")))?;
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(GisError::Limit(format!(
            "the picture is {w} by {h} pixels. The limit is {MAX_PIXELS} pixels"
        )));
    }
    let img = image::load_from_memory(bytes)
        .map_err(|e| GisError::Format(format!("cannot read the picture: {e}")))?
        .to_rgba8();
    Ok(Rgba {
        width: img.width(),
        height: img.height(),
        data: img.into_raw(),
    })
}

/// A clear picture.
pub fn blank(width: u32, height: u32) -> Rgba {
    Rgba {
        width,
        height,
        data: vec![0; width as usize * height as usize * 4],
    }
}

/// The pixel at integer position, clamped to the picture.
fn px(img: &Rgba, x: i64, y: i64) -> [f64; 4] {
    let x = x.clamp(0, i64::from(img.width) - 1) as usize;
    let y = y.clamp(0, i64::from(img.height) - 1) as usize;
    let i = (y * img.width as usize + x) * 4;
    let p = &img.data[i..i + 4];
    let a = f64::from(p[3]) / 255.0;
    // Premultiplied, so clear pixels do not tint their neighbors.
    [
        f64::from(p[0]) * a,
        f64::from(p[1]) * a,
        f64::from(p[2]) * a,
        f64::from(p[3]),
    ]
}

/// Bilinear sample at pixel coordinates (the center of the first pixel is 0.5, 0.5). Outside the
/// picture the result is clear.
pub fn bilinear(img: &Rgba, x: f64, y: f64) -> [u8; 4] {
    if !(x >= 0.0 && y >= 0.0 && x <= f64::from(img.width) && y <= f64::from(img.height)) {
        return [0; 4];
    }
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let mut acc = [0.0; 4];
    for (dx, dy, w) in [
        (0, 0, (1.0 - tx) * (1.0 - ty)),
        (1, 0, tx * (1.0 - ty)),
        (0, 1, (1.0 - tx) * ty),
        (1, 1, tx * ty),
    ] {
        let p = px(img, x0 + dx, y0 + dy);
        for c in 0..4 {
            acc[c] += p[c] * w;
        }
    }
    let a = acc[3];
    if a <= 0.0 {
        return [0; 4];
    }
    let k = 255.0 / a;
    [
        (acc[0] * k).round().clamp(0.0, 255.0) as u8,
        (acc[1] * k).round().clamp(0.0, 255.0) as u8,
        (acc[2] * k).round().clamp(0.0, 255.0) as u8,
        a.round().clamp(0.0, 255.0) as u8,
    ]
}

/// Draws `src` over `dst` with its top left corner at (x, y).
pub fn blit(dst: &mut Rgba, src: &Rgba, x: i64, y: i64) {
    for sy in 0..i64::from(src.height) {
        let dy = y + sy;
        if dy < 0 || dy >= i64::from(dst.height) {
            continue;
        }
        for sx in 0..i64::from(src.width) {
            let dx = x + sx;
            if dx < 0 || dx >= i64::from(dst.width) {
                continue;
            }
            let s = ((sy * i64::from(src.width) + sx) * 4) as usize;
            let d = ((dy * i64::from(dst.width) + dx) * 4) as usize;
            let sp = &src.data[s..s + 4];
            if sp[3] == 255 {
                dst.data[d..d + 4].copy_from_slice(sp);
            } else if sp[3] > 0 {
                let (sa, da) = (f64::from(sp[3]) / 255.0, f64::from(dst.data[d + 3]) / 255.0);
                let oa = sa + da * (1.0 - sa);
                for (c, s) in sp.iter().enumerate().take(3) {
                    let v =
                        (f64::from(*s) * sa + f64::from(dst.data[d + c]) * da * (1.0 - sa)) / oa;
                    dst.data[d + c] = v.round().clamp(0.0, 255.0) as u8;
                }
                dst.data[d + 3] = (oa * 255.0).round() as u8;
            }
        }
    }
}

/// Maps pixel positions (column, row) to world positions:
/// `x = a*col + b*row + c`, `y = d*col + e*row + f`. A world file holds exactly these six numbers
/// (in the order a, d, b, e, c, f), with c and f at the center of the first pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    /// A picture without rotation. `top_left` is the corner of the first pixel, `pixel` the size of
    /// a pixel (both positive; rows go down, so y decreases).
    pub fn north_up(top_left: DVec2, pixel: DVec2) -> Affine {
        Affine {
            a: pixel.x,
            b: 0.0,
            c: top_left.x,
            d: 0.0,
            e: -pixel.y,
            f: top_left.y,
        }
    }

    pub fn apply(&self, col: f64, row: f64) -> DVec2 {
        DVec2::new(
            self.a * col + self.b * row + self.c,
            self.d * col + self.e * row + self.f,
        )
    }

    pub fn inverse(&self) -> Option<Affine> {
        let det = self.a * self.e - self.b * self.d;
        if !det.is_finite() || det.abs() < 1e-300 {
            return None;
        }
        let (a, b, d, e) = (self.e / det, -self.b / det, -self.d / det, self.a / det);
        Some(Affine {
            a,
            b,
            c: -(a * self.c + b * self.f),
            d,
            e,
            f: -(d * self.c + e * self.f),
        })
    }

    /// Box of the whole picture of `w` by `h` pixels.
    pub fn bounds(&self, w: u32, h: u32) -> Bbox {
        let (w, h) = (f64::from(w), f64::from(h));
        Bbox::from_points([
            self.apply(0.0, 0.0),
            self.apply(w, 0.0),
            self.apply(0.0, h),
            self.apply(w, h),
        ])
        .expect("four points")
    }

    /// Size of a pixel: the length of the column and row steps.
    pub fn pixel_size(&self) -> DVec2 {
        DVec2::new(self.a.hypot(self.d), self.b.hypot(self.e))
    }
}

/// A picture with its place in the world.
#[derive(Clone, Debug, PartialEq)]
pub struct GeoRaster {
    pub image: Rgba,
    pub affine: Affine,
}

impl GeoRaster {
    pub fn bounds(&self) -> Bbox {
        self.affine.bounds(self.image.width, self.image.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn solid(w: u32, h: u32, c: [u8; 4]) -> Rgba {
        Rgba {
            width: w,
            height: h,
            data: c
                .iter()
                .copied()
                .cycle()
                .take(w as usize * h as usize * 4)
                .collect(),
        }
    }

    #[test]
    fn decodes_png_and_rejects_junk() {
        let img = solid(3, 2, [10, 20, 30, 255]);
        let back = decode_image(&img.to_png()).unwrap();
        assert_eq!(back, img);
        assert!(decode_image(b"nonsense").is_err());
        assert!(decode_image(&[]).is_err());
    }

    #[test]
    fn decodes_jpeg() {
        let mut out = Vec::new();
        let rgb = image::RgbImage::from_pixel(8, 8, image::Rgb([200, 40, 40]));
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95)
            .encode_image(&rgb)
            .unwrap();
        let img = decode_image(&out).unwrap();
        assert_eq!((img.width, img.height), (8, 8));
        assert!(img.data[0] > 180 && img.data[1] < 70 && img.data[3] == 255);
    }

    #[test]
    fn sampling_blends_and_clips() {
        let mut img = blank(2, 1);
        img.data = vec![255, 0, 0, 255, 0, 0, 255, 255];
        assert_eq!(bilinear(&img, 0.5, 0.5), [255, 0, 0, 255]);
        assert_eq!(bilinear(&img, 1.5, 0.5), [0, 0, 255, 255]);
        assert_eq!(bilinear(&img, 1.0, 0.5), [128, 0, 128, 255]);
        assert_eq!(bilinear(&img, -0.1, 0.5), [0; 4]);
        assert_eq!(bilinear(&img, 0.5, 1.5), [0; 4]);
        assert_eq!(bilinear(&img, f64::NAN, 0.5), [0; 4]);
        // A clear neighbor does not darken the result.
        let mut half = solid(2, 1, [100, 100, 100, 255]);
        half.data[7] = 0;
        let v = bilinear(&half, 1.0, 0.5);
        assert_eq!(&v[..3], &[100, 100, 100]);
        assert_eq!(v[3], 128);
        assert_eq!(bilinear(&blank(2, 2), 1.0, 1.0), [0; 4]);
    }

    #[test]
    fn blit_blends_and_clips() {
        let mut dst = solid(4, 4, [0, 0, 0, 255]);
        blit(&mut dst, &solid(2, 2, [255, 255, 255, 255]), 3, 3);
        assert_eq!(&dst.data[(3 * 4 + 3) * 4..(3 * 4 + 3) * 4 + 4], &[255; 4]);
        blit(&mut dst, &solid(1, 1, [255, 255, 255, 128]), 0, 0);
        assert!((i32::from(dst.data[0]) - 128).abs() <= 1);
        blit(&mut dst, &solid(1, 1, [9, 9, 9, 0]), 1, 1);
        assert_eq!(dst.data[(4 + 1) * 4], 0);
        blit(&mut dst, &solid(3, 3, [1, 2, 3, 255]), -5, -5);
        blit(&mut dst, &solid(3, 3, [1, 2, 3, 255]), 2, -1);
        // Over a clear background the source stays as it is.
        let mut clear = blank(1, 1);
        blit(&mut clear, &solid(1, 1, [50, 60, 70, 100]), 0, 0);
        assert_eq!(&clear.data[..3], &[50, 60, 70]);
        assert_eq!(clear.data[3], 100);
    }

    #[test]
    fn affine_maps_both_ways() {
        let a = Affine::north_up(DVec2::new(100.0, 200.0), DVec2::new(2.0, 3.0));
        assert_eq!(a.apply(0.0, 0.0), DVec2::new(100.0, 200.0));
        assert_eq!(a.apply(10.0, 10.0), DVec2::new(120.0, 170.0));
        let b = a.bounds(10, 10);
        assert_eq!(
            (b.min, b.max),
            (DVec2::new(100.0, 170.0), DVec2::new(120.0, 200.0))
        );
        assert_eq!(a.pixel_size(), DVec2::new(2.0, 3.0));
        let inv = a.inverse().unwrap();
        assert!(inv.apply(120.0, 170.0).distance(DVec2::new(10.0, 10.0)) < 1e-9);
        let flat = Affine {
            a: 1.0,
            b: 2.0,
            c: 0.0,
            d: 2.0,
            e: 4.0,
            f: 0.0,
        };
        assert!(flat.inverse().is_none());
        let g = GeoRaster {
            image: blank(10, 10),
            affine: a,
        };
        assert_eq!(g.bounds(), b);
    }

    proptest! {
        #[test]
        fn affine_inverse_round_trips(
            a in 0.1..10.0f64, b in -2.0..2.0f64, d in -2.0..2.0f64, e in -10.0..-0.1f64,
            c in -1.0e6..1.0e6f64, f in -1.0e6..1.0e6f64,
            col in 0.0..5000.0f64, row in 0.0..5000.0f64,
        ) {
            let t = Affine { a, b, c, d, e, f };
            prop_assume!((a * e - b * d).abs() > 1e-3);
            let w = t.apply(col, row);
            let back = t.inverse().unwrap().apply(w.x, w.y);
            prop_assert!((back.x - col).abs() < 1e-6 * (1.0 + col.abs()));
            prop_assert!((back.y - row).abs() < 1e-6 * (1.0 + row.abs()));
        }
    }
}
