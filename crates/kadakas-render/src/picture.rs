//! Pictures for resources: reads a PNG or JPEG file safely, makes the thumbnail and the
//! `ImageResource` that a document stores (see `kadakas_core::picture`).
//!
//! The work is bounded before it starts: the file size, the size in the header (pixels) and the
//! thumbnail size. A file that is not a picture, is too large or breaks the decoder gives an error.
//! Where the program unwinds a decoder that panics also gives an error (`catch_unwind`); the web
//! build uses `panic = abort`, where only the limits protect it. Pure CPU code, so it also runs in
//! the browser.
//!
//! The thumbnail that a document stores comes from the sender and is a hint. A program that shows
//! the picture to a person derives its own thumbnail from the file with [`derive_thumb`], once for
//! each hash, and keeps it.

use std::sync::Arc;

use kadakas_core::picture::{
    ImageResource, MAX_PICTURE_BYTES, MAX_PICTURE_PIXELS, MAX_THUMB_BASE64, MAX_THUMB_SIDE,
    PictureFormat, encode_base64, sha256_hex,
};

use crate::texture::{
    RawImage, TextureImage, decode_image_limited, decode_png_bytes, decode_raw_limited, encode_png,
};

/// What a host allows for one picture. The defaults are the limits of the document format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PictureLimits {
    /// Largest file in bytes.
    pub max_bytes: usize,
    /// Most pixels (width times height).
    pub max_pixels: u64,
}

impl Default for PictureLimits {
    fn default() -> Self {
        PictureLimits {
            max_bytes: MAX_PICTURE_BYTES,
            max_pixels: MAX_PICTURE_PIXELS,
        }
    }
}

impl PictureLimits {
    /// The limits, never above those of the document format.
    pub fn capped(self) -> PictureLimits {
        PictureLimits {
            max_bytes: self.max_bytes.min(MAX_PICTURE_BYTES),
            max_pixels: self.max_pixels.min(MAX_PICTURE_PIXELS),
        }
    }
}

/// The name of a file for the `source` field: the last part of a path, cut to a safe length.
pub fn source_name(path_or_name: &str) -> String {
    let last = path_or_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim();
    last.chars()
        .filter(|c| !c.is_control())
        .take(kadakas_core::picture::MAX_SOURCE_CHARS)
        .collect()
}

/// Makes the picture resource for the bytes of a PNG or JPEG file. `source` is a file name (a path
/// is cut to its last part).
pub fn import_picture(
    bytes: &[u8],
    source: &str,
    limits: PictureLimits,
) -> Result<ImageResource, String> {
    let limits = limits.capped();
    if bytes.len() > limits.max_bytes {
        return Err(format!(
            "the picture file is larger than {} bytes",
            limits.max_bytes
        ));
    }
    let format = PictureFormat::sniff(bytes).ok_or("the file is not a PNG or JPEG picture")?;
    let raw = decode_raw_limited(bytes, limits.max_pixels)?;
    let thumb = thumbnail_png(&raw)?;
    let res = ImageResource {
        sha256: sha256_hex(bytes),
        format,
        width: raw.width,
        height: raw.height,
        data: encode_base64(bytes).into(),
        thumb: encode_base64(&thumb).into(),
        source: source_name(source),
    };
    // The size of the decoded image must be the size in the header that others will read.
    res.verify_data()?;
    Ok(res)
}

/// The thumbnail PNG of an image: at most [`MAX_THUMB_SIDE`] pixels on a side and at most
/// [`MAX_THUMB_BASE64`] characters. A busy picture that does not fit is made smaller again.
fn thumbnail_png(raw: &RawImage) -> Result<Vec<u8>, String> {
    let mut side = MAX_THUMB_SIDE;
    loop {
        let small = raw.downscale(side);
        let png = encode_png(small.width, small.height, &small.rgba)?;
        if png.len().div_ceil(3) * 4 <= MAX_THUMB_BASE64 {
            return Ok(png);
        }
        if side <= 8 {
            return Err("cannot make a small thumbnail of this picture".into());
        }
        side /= 2;
    }
}

/// The thumbnail that the file itself gives: the file is decoded within `limits`, its size must be
/// the size in the resource, and the pixels are scaled down to at most [`MAX_THUMB_SIDE`]. This is
/// the thumbnail to show for a picture from a file or from another user. It costs a full decode,
/// so a caller keeps it by the hash of the file.
pub fn derive_thumb(res: &ImageResource, limits: PictureLimits) -> Option<TextureImage> {
    let limits = limits.capped();
    if res.byte_len() > limits.max_bytes {
        return None;
    }
    let raw = decode_raw_limited(&res.bytes()?, limits.max_pixels).ok()?;
    ((raw.width, raw.height) == (res.width, res.height)).then(|| raw.downscale(MAX_THUMB_SIDE))
}

/// The pixels of the thumbnail that the resource stores. `None` if it is not a readable PNG. The
/// sender made this thumbnail, so it can differ from the file: use [`derive_thumb`] to show the
/// picture to a person. A decoder that panics gives `None` where the program unwinds.
pub fn thumb_image(res: &ImageResource) -> Option<TextureImage> {
    let bytes = res.thumb_bytes()?;
    std::panic::catch_unwind(|| decode_png_bytes(&bytes))
        .ok()?
        .ok()
        .filter(|i| i.width <= MAX_THUMB_SIDE && i.height <= MAX_THUMB_SIDE)
}

/// The full picture pixels, limited by `limits`. `None` if the file cannot be read.
pub fn full_image(res: &ImageResource, limits: PictureLimits) -> Option<TextureImage> {
    let limits = limits.capped();
    decode_image_limited(&res.bytes()?, limits.max_pixels).ok()
}

/// The thumbnail on white paper as RGB, for sheet output (sheet images have no alpha channel).
/// The thumbnail is derived from the file (see [`derive_thumb`]). Returns the size and the pixels,
/// rows from the top.
pub fn thumb_rgb_on_white(res: &ImageResource) -> Option<(u32, u32, Arc<Vec<u8>>)> {
    let img = derive_thumb(res, PictureLimits::default())?;
    let rgb: Vec<u8> = img
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let a = f64::from(p[3]) / 255.0;
            [0, 1, 2].map(|c| (f64::from(p[c]) * a + 255.0 * (1.0 - a)).round() as u8)
        })
        .collect();
    Some((img.width, img.height, Arc::new(rgb)))
}

/// A picture made in code for tests of this and other crates: a gradient with a transparent
/// corner. There are no sample pictures in the repository except one tiny JPEG test file.
#[doc(hidden)]
pub fn test_png(width: u32, height: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let a = if x == 0 && y == 0 { 0 } else { 255 };
            px.extend_from_slice(&[
                (x * 255 / width.max(1)) as u8,
                (y * 255 / height.max(1)) as u8,
                90,
                a,
            ]);
        }
    }
    encode_png(width, height, &px).expect("encodes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_becomes_a_resource_with_a_small_thumbnail() {
        let png = test_png(400, 200);
        let r = import_picture(&png, "/home/me/pics/maple.png", PictureLimits::default()).unwrap();
        assert_eq!(
            (r.width, r.height, r.format),
            (400, 200, PictureFormat::Png)
        );
        assert_eq!(r.sha256, sha256_hex(&png));
        assert_eq!(r.source, "maple.png");
        assert_eq!(r.bytes().unwrap(), png);
        let t = thumb_image(&r).unwrap();
        assert!(t.width <= MAX_THUMB_SIDE && t.height <= MAX_THUMB_SIDE);
        assert_eq!((t.width, t.height), (133, 66));
        // The same file gives the same resource.
        assert_eq!(
            import_picture(&png, "x.png", PictureLimits::default())
                .unwrap()
                .sha256,
            r.sha256
        );
        let full = full_image(&r, PictureLimits::default()).unwrap();
        assert_eq!((full.width, full.height), (400, 200));
        assert!(
            full_image(
                &r,
                PictureLimits {
                    max_bytes: 1,
                    max_pixels: 10
                }
            )
            .is_none()
        );
    }

    #[test]
    fn a_jpeg_is_read_and_has_no_transparency() {
        let jpg = include_bytes!("../testdata/red_blue.jpg");
        let r = import_picture(jpg, "red_blue.jpg", PictureLimits::default()).unwrap();
        assert_eq!((r.width, r.height, r.format), (16, 16, PictureFormat::Jpeg));
        let (w, h, rgb) = thumb_rgb_on_white(&r).unwrap();
        assert_eq!((w, h, rgb.len()), (16, 16, 16 * 16 * 3));
    }

    #[test]
    fn transparent_pixels_show_white_paper() {
        let png = test_png(4, 4);
        let r = import_picture(&png, "a.png", PictureLimits::default()).unwrap();
        let (_, _, rgb) = thumb_rgb_on_white(&r).unwrap();
        assert_eq!(&rgb[..3], &[255, 255, 255]);
        assert_ne!(&rgb[3..6], &[255, 255, 255]);
    }

    #[test]
    fn hostile_and_oversize_files_are_refused() {
        let png = test_png(64, 64);
        let small = PictureLimits {
            max_bytes: 100,
            ..Default::default()
        };
        assert!(
            import_picture(&png, "a.png", small)
                .unwrap_err()
                .contains("larger")
        );
        let few = PictureLimits {
            max_pixels: 1000,
            ..Default::default()
        };
        assert!(
            import_picture(&png, "a.png", few)
                .unwrap_err()
                .contains("pixels")
        );
        // The limits of the host never go above the limits of the format.
        let huge = PictureLimits {
            max_bytes: usize::MAX,
            max_pixels: u64::MAX,
        };
        assert_eq!(huge.capped(), PictureLimits::default());
        for bad in [
            &b""[..],
            b"GIF89a",
            b"\x89PNG\r\n\x1a\n",
            b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR",
            b"\xFF\xD8\xFF",
            b"\xFF\xD8\xFF\xE0\0\x10JFIF\0",
            b"<svg/>",
        ] {
            assert!(import_picture(bad, "x", PictureLimits::default()).is_err());
        }
        // A PNG that claims a huge size in its header is refused before any pixel buffer exists.
        let mut lie = png.clone();
        lie[16..20].copy_from_slice(&8000u32.to_be_bytes());
        lie[20..24].copy_from_slice(&8000u32.to_be_bytes());
        assert!(import_picture(&lie, "x", PictureLimits::default()).is_err());
        // A file cut in the middle, and a file with a damaged JPEG body.
        assert!(import_picture(&png[..png.len() / 2], "x", PictureLimits::default()).is_err());
        let jpg = include_bytes!("../testdata/red_blue.jpg");
        let mut broken = jpg.to_vec();
        for b in broken.iter_mut().skip(200) {
            *b = 0x55;
        }
        let _ = import_picture(&broken, "x", PictureLimits::default());
    }

    #[test]
    fn a_noisy_picture_still_gets_a_thumbnail_within_the_size_limit() {
        // Pseudo-random pixels do not compress, so the thumbnail is made smaller until it fits.
        let (w, h) = (300u32, 300u32);
        let mut state = 12345u32;
        let px: Vec<u8> = (0..w * h * 4)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                (state >> 24) as u8
            })
            .collect();
        let png = encode_png(w, h, &px).unwrap();
        let r = import_picture(&png, "noise.png", PictureLimits::default()).unwrap();
        assert!(r.thumb.len() <= MAX_THUMB_BASE64);
        r.validate().unwrap();
    }

    #[test]
    fn source_names_lose_their_folders_and_control_characters() {
        assert_eq!(source_name("C:\\a\\b\\leaf.png"), "leaf.png");
        assert_eq!(source_name("a/b/le\naf.png "), "leaf.png");
        assert_eq!(source_name(&"x".repeat(400)).chars().count(), 255);
        assert_eq!(source_name(""), "");
    }

    #[test]
    fn the_thumbnail_is_derived_from_the_file_not_taken_from_the_sender() {
        let png = test_png(400, 200);
        let mut r = import_picture(&png, "a.png", PictureLimits::default()).unwrap();
        let own = derive_thumb(&r, PictureLimits::default()).unwrap();
        assert_eq!((own.width, own.height), (133, 66));
        // A sender that stores another picture as the thumbnail does not change what we derive.
        let other = import_picture(&test_png(8, 8), "b.png", PictureLimits::default()).unwrap();
        r.thumb = other.thumb.clone();
        assert_eq!(derive_thumb(&r, PictureLimits::default()).unwrap(), own);
        assert_ne!(thumb_image(&r).unwrap(), own, "the stored hint differs");
        let (w, h, rgb) = thumb_rgb_on_white(&r).unwrap();
        assert_eq!((w, h, rgb.len()), (133, 66, 133 * 66 * 3));
        // The size in the resource must be the size of the file; the limits apply.
        let mut wrong = r.clone();
        wrong.width = 5;
        assert!(derive_thumb(&wrong, PictureLimits::default()).is_none());
        let few = PictureLimits {
            max_pixels: 10,
            ..Default::default()
        };
        assert!(derive_thumb(&r, few).is_none());
        let small = PictureLimits {
            max_bytes: 10,
            ..Default::default()
        };
        assert!(derive_thumb(&r, small).is_none());
        let mut broken = r.clone();
        broken.data = "AAAA".into();
        assert!(derive_thumb(&broken, PictureLimits::default()).is_none());
    }

    #[test]
    fn the_import_checks_the_file_against_the_decoded_size() {
        // The data of every picture that the import makes passes the check of its own file.
        let r = import_picture(&test_png(33, 17), "a.png", PictureLimits::default()).unwrap();
        r.verify_data().unwrap();
        let jpg = include_bytes!("../testdata/red_blue.jpg");
        let j = import_picture(jpg, "j.jpg", PictureLimits::default()).unwrap();
        j.verify_data().unwrap();
        assert_eq!((j.width, j.height), (16, 16));
    }
}
