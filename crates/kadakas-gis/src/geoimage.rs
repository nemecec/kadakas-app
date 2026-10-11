//! Georeferenced pictures: a PNG or JPEG with a world file, and GeoTIFF.
//!
//! A world file (`.pgw`, `.jgw`, `.tfw` or `.wld`) holds the six numbers of the map transform of a
//! picture. The CRS comes from a `.prj` file next to the picture, from the GeoTIFF keys, or from the
//! user. The picture is warped into the model frame of the document and becomes an underlay.

use std::path::{Path, PathBuf};

use kadakas_core::DVec2;
use kadakas_core::georef::Georeference;
use kadakas_core::underlay::{MAX_SIDE_PX, PngData, Underlay};
use kadakas_crs::{Crs, Transformer, utm_epsg};

use crate::raster::{Affine, GeoRaster, Rgba, decode_image};
use crate::tiles::Bbox;
use crate::warp::{Frame, fit_size, warp};
use crate::{GisError, Result};

/// A picture with a place on the earth.
#[derive(Clone, Debug)]
pub struct GeoImage {
    pub name: String,
    pub raster: GeoRaster,
    /// The CRS of the map transform, as text (`EPSG:3067`, a WKT or a PROJ string), if known.
    pub crs: Option<String>,
    /// Remarks for the report.
    pub notes: Vec<String>,
}

/// Reads the six numbers of a world file and returns the map transform of the picture. The numbers
/// are: A (pixel width), D, B, E (pixel height, negative for a north up picture), C and F (the
/// center of the top left pixel).
pub fn parse_world_file(text: &str) -> Result<Affine> {
    let v: Vec<f64> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(6)
        .map(|l| {
            l.replace(',', ".").parse::<f64>().map_err(|_| {
                GisError::Format(format!(
                    "the world file has a line that is not a number: {l}"
                ))
            })
        })
        .collect::<Result<_>>()?;
    if v.len() < 6 {
        return Err(GisError::Format(
            "a world file has six lines, this one has fewer".into(),
        ));
    }
    if v.iter().any(|x| !x.is_finite()) {
        return Err(GisError::Format(
            "the world file has a number that is not finite".into(),
        ));
    }
    let (a, d, b, e, c, f) = (v[0], v[1], v[2], v[3], v[4], v[5]);
    if (a * e - b * d).abs() < 1e-300 {
        return Err(GisError::Format(
            "the world file does not describe a picture (zero pixel size)".into(),
        ));
    }
    // C and F are the center of the first pixel: move to its corner.
    Ok(Affine {
        a,
        b,
        c: c - 0.5 * a - 0.5 * b,
        d,
        e,
        f: f - 0.5 * d - 0.5 * e,
    })
}

/// The names of the world files to try for a picture, best first.
pub fn world_file_candidates(path: &Path) -> Vec<PathBuf> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let first = match ext.as_str() {
        "png" => "pgw",
        "jpg" | "jpeg" => "jgw",
        "tif" | "tiff" => "tfw",
        "gif" => "gfw",
        "bmp" => "bpw",
        _ => "wld",
    };
    let mut out = Vec::new();
    // The extension in lower case, then in upper case, then the long form `.jpgw`, then `.wld`.
    for e in [
        first.to_string(),
        first.to_ascii_uppercase(),
        format!("{ext}w"),
        "wld".to_string(),
        "WLD".to_string(),
    ] {
        let p = path.with_extension(e);
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// A picture with the text of its world file.
pub fn from_picture(
    name: &str,
    bytes: &[u8],
    world: &str,
    crs: Option<String>,
) -> Result<GeoImage> {
    let image = decode_image(bytes)?;
    Ok(GeoImage {
        name: name.to_string(),
        raster: GeoRaster {
            image,
            affine: parse_world_file(world)?,
        },
        crs,
        notes: Vec::new(),
    })
}

// ---------------------------------------------------------------- GeoTIFF

const TAG_PIXEL_SCALE: u16 = 33550;
const TAG_TIEPOINT: u16 = 33922;
const TAG_TRANSFORMATION: u16 = 34264;
const TAG_GEO_KEYS: u16 = 34735;

/// EPSG code from the GeoKey directory: the projected CRS, else the geographic CRS.
/// The second value tells if the raster type is "pixel is point".
fn geo_keys(keys: &[u16]) -> (Option<u32>, bool) {
    let (mut projected, mut geographic, mut point) = (None, None, false);
    if keys.len() >= 4 {
        let n = usize::from(keys[3]);
        for k in keys[4..].as_chunks::<4>().0.iter().take(n) {
            let (id, location, value) = (k[0], k[1], k[3]);
            if location != 0 {
                continue;
            }
            match id {
                3072 if value != 32767 && value != 0 => projected = Some(u32::from(value)),
                2048 if value != 32767 && value != 0 => geographic = Some(u32::from(value)),
                1025 => point = value == 2,
                _ => {}
            }
        }
    }
    (projected.or(geographic), point)
}

/// Reads a GeoTIFF: 8 or 16 bit gray, gray with alpha, RGB or RGBA, with a tiepoint and pixel scale
/// (or a model transformation) and a CRS from the GeoKeys.
pub fn read_geotiff(name: &str, bytes: &[u8]) -> Result<GeoImage> {
    read_tiff(name, bytes, true)
}

/// Reads the pixels of a TIFF. Without `need_geo` a TIFF without georeference tags is accepted (a
/// world file gives the place).
fn read_tiff(name: &str, bytes: &[u8], need_geo: bool) -> Result<GeoImage> {
    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::tags::Tag;
    let bad = |e: tiff::TiffError| GisError::Format(format!("cannot read the GeoTIFF: {e}"));
    let mut d = Decoder::new(std::io::Cursor::new(bytes)).map_err(bad)?;
    let (w, h) = d.dimensions().map_err(bad)?;
    if u64::from(w) * u64::from(h) > crate::raster::MAX_PIXELS {
        return Err(GisError::Limit(format!(
            "the GeoTIFF is {w} by {h} pixels, which is too large"
        )));
    }
    let scale = d.get_tag_f64_vec(Tag::Unknown(TAG_PIXEL_SCALE)).ok();
    let tie = d.get_tag_f64_vec(Tag::Unknown(TAG_TIEPOINT)).ok();
    let matrix = d.get_tag_f64_vec(Tag::Unknown(TAG_TRANSFORMATION)).ok();
    let keys = d
        .get_tag_u16_vec(Tag::Unknown(TAG_GEO_KEYS))
        .ok()
        .unwrap_or_default();
    let (epsg, point) = geo_keys(&keys);
    let mut affine = match (&matrix, &scale, &tie) {
        (Some(m), _, _) if m.len() >= 16 => Affine {
            a: m[0],
            b: m[1],
            c: m[3],
            d: m[4],
            e: m[5],
            f: m[7],
        },
        (_, Some(s), Some(t)) if s.len() >= 2 && t.len() >= 6 => Affine {
            a: s[0],
            b: 0.0,
            c: t[3] - t[0] * s[0],
            d: 0.0,
            e: -s[1],
            f: t[4] + t[1] * s[1],
        },
        _ if !need_geo => Affine::north_up(DVec2::ZERO, DVec2::ONE),
        _ => {
            return Err(GisError::Format(
                "the TIFF has no georeference (no tiepoint and pixel scale). Use a world file"
                    .into(),
            ));
        }
    };
    if point {
        // The tiepoint is the center of a pixel: move to the corner.
        affine.c -= 0.5 * affine.a + 0.5 * affine.b;
        affine.f -= 0.5 * affine.d + 0.5 * affine.e;
    }
    let color = d.colortype().map_err(bad)?;
    let channels = match color {
        tiff::ColorType::Gray(_) => 1,
        tiff::ColorType::GrayA(_) => 2,
        tiff::ColorType::RGB(_) => 3,
        tiff::ColorType::RGBA(_) => 4,
        other => {
            return Err(GisError::Format(format!(
                "this GeoTIFF has the color type {other:?}, which is not supported (use 8 or 16 bit gray, RGB or RGBA)"
            )));
        }
    };
    let samples: Vec<u8> = match d.read_image().map_err(bad)? {
        DecodingResult::U8(v) => v,
        DecodingResult::U16(v) => v.into_iter().map(|x| (x >> 8) as u8).collect(),
        _ => {
            return Err(GisError::Format(
                "this GeoTIFF has samples that are not 8 or 16 bit integers".into(),
            ));
        }
    };
    if samples.len() < w as usize * h as usize * channels {
        return Err(GisError::Format(
            "the GeoTIFF has fewer samples than its size says".into(),
        ));
    }
    let mut data = Vec::with_capacity(w as usize * h as usize * 4);
    for px in samples.chunks_exact(channels).take(w as usize * h as usize) {
        match channels {
            1 => data.extend_from_slice(&[px[0], px[0], px[0], 255]),
            2 => data.extend_from_slice(&[px[0], px[0], px[0], px[1]]),
            3 => data.extend_from_slice(&[px[0], px[1], px[2], 255]),
            _ => data.extend_from_slice(px),
        }
    }
    Ok(GeoImage {
        name: name.to_string(),
        raster: GeoRaster {
            image: Rgba {
                width: w,
                height: h,
                data,
            },
            affine,
        },
        crs: epsg.map(|c| format!("EPSG:{c}")),
        notes: Vec::new(),
    })
}

/// Reads a picture file with its georeference: a GeoTIFF alone, or a PNG, JPEG (or a TIFF) with a
/// world file next to it. A `.prj` file next to the picture gives the CRS. `crs` overrides both.
pub fn load_file(path: &Path, crs: Option<&str>) -> Result<GeoImage> {
    let bytes = std::fs::read(path)
        .map_err(|e| GisError::Format(format!("cannot read {}: {e}", path.display())))?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let world = world_file_candidates(path)
        .into_iter()
        .find_map(|p| std::fs::read_to_string(p).ok());
    let is_tiff = bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*");
    let mut img = match (is_tiff, &world) {
        (true, None) => read_geotiff(&name, &bytes)?,
        (true, Some(w)) => {
            // A world file wins over the tags, like in most GIS programs. The picture is the TIFF.
            let mut g = read_tiff(&name, &bytes, false)?;
            g.raster.affine = parse_world_file(w)?;
            g
        }
        (false, Some(w)) => from_picture(&name, &bytes, w, None)?,
        (false, None) => {
            return Err(GisError::Format(format!(
                "no world file found next to {}. Looked for: {}",
                path.display(),
                world_file_candidates(path)
                    .iter()
                    .filter_map(|p| p.extension().map(|e| format!(".{}", e.to_string_lossy())))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
    };
    if img.crs.is_none() {
        for ext in ["prj", "PRJ"] {
            if let Ok(t) = std::fs::read_to_string(path.with_extension(ext)) {
                img.crs = Some(t.trim().to_string());
                break;
            }
        }
    }
    if let Some(c) = crs {
        img.crs = Some(c.to_string());
    }
    Ok(img)
}

// ---------------------------------------------------------------- to an underlay

/// What placing a geoimage in a document gave.
#[derive(Debug)]
pub struct Placed {
    pub underlay: Underlay,
    /// Set when the document had no georeference: the caller sets it with `SetGeoreference`.
    pub new_georef: Option<Georeference>,
    pub notes: Vec<String>,
}

/// Warps the picture into the model frame of a document.
///
/// `doc` is the georeference of the document, if any. A document without one gets a georeference
/// from the picture (a projected CRS of the picture, or the UTM zone of a geographic one), with the
/// origin at the middle of the picture, like the file importers do. The picture CRS is the one in
/// the picture, else the document CRS.
pub fn place(img: &GeoImage, doc: Option<&Georeference>, max_side: u32) -> Result<Placed> {
    let mut notes = img.notes.clone();
    let src_crs: Option<Crs> = match &img.crs {
        Some(t) => Some(Crs::parse(t).map_err(GisError::Crs)?),
        None => None,
    };
    let doc_has_crs = doc.is_some_and(|g| !g.crs.trim().is_empty());
    let (doc_crs, georef, new_georef) = match (doc, doc_has_crs, &src_crs) {
        (Some(g), true, _) => (Crs::parse(&g.crs).map_err(GisError::Crs)?, g.clone(), None),
        (_, _, Some(src)) => {
            // Make a georeference from the picture.
            let b = img.raster.bounds();
            let c = (b.min + b.max) / 2.0;
            let target = if src.is_geographic() {
                Crs::from_epsg(utm_epsg(c.x, c.y)).map_err(GisError::Crs)?
            } else {
                Crs::parse(&src.spec).map_err(GisError::Crs)?
            };
            let (x, y, _) = Transformer::new(src, &target)
                .apply(c.x, c.y, 0.0)
                .map_err(GisError::Crs)?;
            let origin = DVec2::new(x * target.to_meter(), y * target.to_meter()).round();
            let g = Georeference {
                crs: target.spec.clone(),
                origin,
                rotation: 0.0,
                user_origin: DVec2::ZERO,
            };
            (target, g.clone(), Some(g))
        }
        (_, _, None) => {
            return Err(GisError::Crs(
                "the picture has no CRS and the document has no CRS either. Name the CRS of the picture (crs)".into(),
            ));
        }
    };
    if src_crs.is_none() {
        notes.push(format!(
            "the picture has no CRS: the document CRS {} is assumed",
            doc_crs.spec
        ));
    }
    let frame = Frame::new(&doc_crs, &georef)?;
    let from = src_crs.as_ref().unwrap_or(&doc_crs);
    // The picture in the model frame: the box around its (converted) outline.
    let r = &img.raster;
    let (w, h) = (f64::from(r.image.width), f64::from(r.image.height));
    let mut pts = Vec::new();
    for i in 0..=8 {
        for j in 0..=8 {
            let p = r
                .affine
                .apply(w * f64::from(i) / 8.0, h * f64::from(j) / 8.0);
            pts.push(frame.crs_to_model(p, from)?);
        }
    }
    let bx = Bbox::from_points(pts).expect("points");
    if !(bx.width() > 0.0 && bx.height() > 0.0 && bx.is_finite()) {
        return Err(GisError::Format(
            "the picture has no size on the map".into(),
        ));
    }
    let max_side = max_side.clamp(1, MAX_SIDE_PX);
    let size = fit_size(
        bx.width(),
        bx.height(),
        max_side.min(r.image.width.max(r.image.height).max(1)),
    );
    let out = warp(r, Some(from), &frame, bx.min, bx.max, size)?;
    Ok(Placed {
        underlay: Underlay {
            name: img.name.clone(),
            min: bx.min,
            max: bx.max,
            width: size.0,
            height: size.1,
            png: PngData(std::sync::Arc::new(out.to_png())),
            opacity: 1.0,
            source: format!("{}.{}", img.name, "geoimage"),
        },
        new_georef,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::solid;

    const WORLD: &str = "2.0\n0.0\n0.0\n-2.0\n385001.0\n6672009.0\n";

    #[test]
    fn world_file_moves_to_the_pixel_corner() {
        let a = parse_world_file(WORLD).unwrap();
        assert_eq!(a.apply(0.0, 0.0), DVec2::new(385_000.0, 6_672_010.0));
        assert_eq!(a.apply(1.0, 1.0), DVec2::new(385_002.0, 6_672_008.0));
        // Comma decimals, blank lines and extra lines.
        let b = parse_world_file("\n1,5\n0\n0\n-1,5\n10\n20\nextra\n").unwrap();
        assert_eq!(b.a, 1.5);
        assert!(
            parse_world_file("1\n2\n3")
                .unwrap_err()
                .to_string()
                .contains("six lines")
        );
        assert!(parse_world_file("1\n0\n0\nx\n0\n0").is_err());
        assert!(
            parse_world_file("0\n0\n0\n0\n0\n0")
                .unwrap_err()
                .to_string()
                .contains("zero pixel")
        );
        assert!(parse_world_file("1\n0\n0\n-1\ninf\n0").is_err());
        // A rotated picture.
        let r = parse_world_file("1\n0.5\n-0.5\n-1\n0\n0\n").unwrap();
        assert_eq!(r.b, -0.5);
    }

    #[test]
    fn world_file_names() {
        let p = Path::new("/a/site.PNG");
        let c = world_file_candidates(p);
        assert_eq!(c[0], PathBuf::from("/a/site.pgw"));
        assert!(c.contains(&PathBuf::from("/a/site.wld")));
        let j = world_file_candidates(Path::new("x.jpeg"));
        assert_eq!(j[0], PathBuf::from("x.jgw"));
        assert!(j.contains(&PathBuf::from("x.jpegw")));
        assert_eq!(
            world_file_candidates(Path::new("x.tif"))[0],
            PathBuf::from("x.tfw")
        );
        assert_eq!(
            world_file_candidates(Path::new("x.gif"))[0],
            PathBuf::from("x.gfw")
        );
        assert_eq!(
            world_file_candidates(Path::new("x.bmp"))[0],
            PathBuf::from("x.bpw")
        );
        assert_eq!(
            world_file_candidates(Path::new("x.xyz"))[0],
            PathBuf::from("x.wld")
        );
        // No duplicates.
        let w = world_file_candidates(Path::new("x.wld"));
        assert_eq!(
            w.iter().filter(|p| p.extension().unwrap() == "wld").count(),
            1
        );
    }

    fn geo(crs: Option<&str>) -> GeoImage {
        from_picture(
            "plan",
            &solid(10, 10, [10, 20, 30, 255]).to_png(),
            WORLD,
            crs.map(String::from),
        )
        .unwrap()
    }

    #[test]
    fn places_a_picture_in_a_document_with_a_crs() {
        let g = Georeference {
            crs: "EPSG:3067".into(),
            origin: DVec2::new(385_000.0, 6_672_000.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        };
        let placed = place(&geo(Some("EPSG:3067")), Some(&g), 512).unwrap();
        assert!(placed.new_georef.is_none());
        let u = &placed.underlay;
        // 10 pixels of 2 m: 20 m square; x from 0 to 20000 mm, y from -10000 to 10000 mm (the origin is 6672000).
        assert!((u.min.x).abs() < 1e-6 && (u.max.x - 20_000.0).abs() < 1e-6);
        assert!((u.min.y + 10_000.0).abs() < 1e-6 && (u.max.y - 10_000.0).abs() < 1e-6);
        assert_eq!((u.width, u.height), (10, 10));
        assert!(u.problem().is_none());
        // Without a CRS in the picture the document CRS is assumed, with a note.
        let placed = place(&geo(None), Some(&g), 512).unwrap();
        assert!(placed.notes[0].contains("assumed"));
        // A picture in another CRS lands in the same place.
        let wgs = from_picture(
            "p",
            &solid(10, 10, [1, 2, 3, 255]).to_png(),
            "0.0001\n0\n0\n-0.0001\n24.9\n60.2\n",
            Some("EPSG:4326".into()),
        )
        .unwrap();
        let g35 = Georeference {
            crs: "EPSG:32635".into(),
            origin: DVec2::new(385_000.0, 6_672_000.0),
            ..g.clone()
        };
        let p = place(&wgs, Some(&g35), 512).unwrap();
        assert!(p.underlay.max.x > p.underlay.min.x);
    }

    #[test]
    fn a_document_without_georeference_gets_one() {
        let placed = place(&geo(Some("EPSG:3067")), None, 512).unwrap();
        let g = placed.new_georef.unwrap();
        assert_eq!(g.crs, "EPSG:3067");
        assert_eq!(g.origin, DVec2::new(385_010.0, 6_672_000.0));
        // The picture is centered near the origin.
        let u = &placed.underlay;
        assert!(u.min.x < 0.0 && u.max.x > 0.0);
        // A geographic picture gets a UTM zone.
        let wgs = from_picture(
            "p",
            &solid(4, 4, [1, 2, 3, 255]).to_png(),
            "0.001\n0\n0\n-0.001\n24.9\n60.2\n",
            Some("EPSG:4326".into()),
        )
        .unwrap();
        let g = place(&wgs, None, 512).unwrap().new_georef.unwrap();
        assert_eq!(g.crs, "EPSG:32635");
        // A document with a georeference but no CRS counts as none.
        let local = Georeference {
            crs: String::new(),
            origin: DVec2::new(5.0, 5.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        };
        assert!(
            place(&geo(Some("EPSG:3067")), Some(&local), 512)
                .unwrap()
                .new_georef
                .is_some()
        );
        // No CRS anywhere.
        let e = place(&geo(None), None, 512).unwrap_err().to_string();
        assert!(e.contains("no CRS"), "{e}");
        assert!(place(&geo(Some("banana")), None, 512).is_err());
    }

    #[test]
    fn geotiff_keys() {
        // Header: version 1, revision 1, minor 0, 3 keys.
        let keys = [
            1u16, 1, 0, 3, 1024, 0, 1, 1, 1025, 0, 1, 2, 3072, 0, 1, 3067,
        ];
        assert_eq!(geo_keys(&keys), (Some(3067), true));
        let user_defined = [1u16, 1, 0, 1, 3072, 0, 1, 32767];
        assert_eq!(geo_keys(&user_defined), (None, false));
        let geographic = [1u16, 1, 0, 1, 2048, 0, 1, 4326];
        assert_eq!(geo_keys(&geographic), (Some(4326), false));
        assert_eq!(geo_keys(&[]), (None, false));
        // A key with a value stored in another tag is skipped.
        assert_eq!(geo_keys(&[1u16, 1, 0, 1, 3072, 34736, 1, 0]), (None, false));
    }
}
