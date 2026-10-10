//! Moving pictures between a map CRS and the model frame of the document.
//!
//! The picture of an underlay lies in the model frame, axis aligned (see `cadektor_core::underlay`).
//! A map from a server lies in some CRS. The warp finds, for each pixel of the new picture, the pixel
//! of the old one. The exact CRS conversion is done on a coarse grid of points and the grid is
//! interpolated, which is fast and accurate for map sizes.

use cadektor_core::DVec2;
use cadektor_core::georef::Georeference;
use cadektor_crs::{Crs, Transformer};

use crate::raster::{Affine, GeoRaster, Rgba, bilinear};
use crate::tiles::Bbox;
use crate::{GisError, Result};

/// The CRS and the georeference of a document.
pub struct Frame<'a> {
    pub crs: &'a Crs,
    pub georef: &'a Georeference,
}

fn same_crs(a: &Crs, b: &Crs) -> bool {
    a.spec == b.spec || (a.epsg().is_some() && a.epsg() == b.epsg())
}

impl<'a> Frame<'a> {
    /// A frame for a document georeference. The CRS must be projected.
    pub fn new(crs: &'a Crs, georef: &'a Georeference) -> Result<Frame<'a>> {
        if crs.is_geographic() {
            return Err(GisError::Crs(
                "the document CRS must be a projected CRS (meters), not longitude and latitude"
                    .into(),
            ));
        }
        if !georef.is_finite() {
            return Err(GisError::Param("the georeference is not valid".into()));
        }
        Ok(Frame { crs, georef })
    }

    /// A model position (mm) in the units of another CRS.
    pub fn model_to_crs(&self, model: DVec2, target: &Crs) -> Result<DVec2> {
        let m = self.georef.to_map(model) / self.crs.to_meter();
        if same_crs(self.crs, target) {
            return Ok(m);
        }
        let (x, y, _) = Transformer::new(self.crs, target)
            .apply(m.x, m.y, 0.0)
            .map_err(GisError::Crs)?;
        Ok(DVec2::new(x, y))
    }

    /// A position in the units of another CRS as a model position (mm).
    pub fn crs_to_model(&self, p: DVec2, from: &Crs) -> Result<DVec2> {
        let (x, y) = if same_crs(self.crs, from) {
            (p.x, p.y)
        } else {
            let (x, y, _) = Transformer::new(from, self.crs)
                .apply(p.x, p.y, 0.0)
                .map_err(GisError::Crs)?;
            (x, y)
        };
        Ok(self.georef.to_model(DVec2::new(x, y) * self.crs.to_meter()))
    }

    /// The box in another CRS that covers a box of the model. Points along the edges and inside are
    /// converted, so a curved edge is covered.
    pub fn view_bbox(&self, min: DVec2, max: DVec2, target: &Crs) -> Result<Bbox> {
        let n = 8;
        let mut pts = Vec::new();
        for i in 0..=n {
            for j in 0..=n {
                let t = DVec2::new(f64::from(i), f64::from(j)) / f64::from(n);
                pts.push(self.model_to_crs(min + (max - min) * t, target)?);
            }
        }
        Bbox::from_points(pts).ok_or_else(|| GisError::Param("empty view".into()))
    }
}

/// A size in pixels for a box of `w` by `h` that has no side longer than `max_side`.
pub fn fit_size(w: f64, h: f64, max_side: u32) -> (u32, u32) {
    let max_side = max_side.max(1);
    if !(w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()) {
        return (1, 1);
    }
    let k = f64::from(max_side) / w.max(h);
    (
        ((w * k).round() as u32).clamp(1, max_side),
        ((h * k).round() as u32).clamp(1, max_side),
    )
}

/// Makes the picture of the box `out_min..out_max` (model mm) of size `size` pixels from `src`,
/// which lies in `src_crs` (`None`: the CRS of the document).
pub fn warp(
    src: &GeoRaster,
    src_crs: Option<&Crs>,
    frame: &Frame<'_>,
    out_min: DVec2,
    out_max: DVec2,
    size: (u32, u32),
) -> Result<Rgba> {
    let (w, h) = size;
    if w == 0 || h == 0 || !(out_max.x > out_min.x && out_max.y > out_min.y) {
        return Err(GisError::Param("the target area is empty".into()));
    }
    let inv = src.affine.inverse().ok_or_else(|| {
        GisError::Format("the picture has a map transform that cannot be turned around".into())
    })?;
    let src_crs = src_crs.unwrap_or(frame.crs);
    let same = same_crs(frame.crs, src_crs);
    // Source pixel of an output pixel position (in pixels, top left origin).
    let node = |px: f64, py: f64| -> Option<DVec2> {
        let model = DVec2::new(
            out_min.x + (out_max.x - out_min.x) * px / f64::from(w),
            out_max.y - (out_max.y - out_min.y) * py / f64::from(h),
        );
        let p = frame.model_to_crs(model, src_crs).ok()?;
        let q = inv.apply(p.x, p.y);
        q.is_finite().then_some(q)
    };
    // Exact conversion only at the nodes of a grid.
    let g = (w.max(h) / 32).clamp(4, 64) as usize;
    let mut nodes: Vec<Option<DVec2>> = Vec::with_capacity((g + 1) * (g + 1));
    for j in 0..=g {
        for i in 0..=g {
            let (px, py) = (
                f64::from(w) * i as f64 / g as f64,
                f64::from(h) * j as f64 / g as f64,
            );
            nodes.push(node(px, py));
        }
    }
    let mut out = crate::raster::blank(w, h);
    for y in 0..h {
        let fy = (f64::from(y) + 0.5) / f64::from(h) * g as f64;
        let j = (fy.floor() as usize).min(g - 1);
        let ty = fy - j as f64;
        for x in 0..w {
            let fx = (f64::from(x) + 0.5) / f64::from(w) * g as f64;
            let i = (fx.floor() as usize).min(g - 1);
            let tx = fx - i as f64;
            let at = |di: usize, dj: usize| nodes[(j + dj) * (g + 1) + i + di];
            let (Some(a), Some(b), Some(c), Some(d)) = (at(0, 0), at(1, 0), at(0, 1), at(1, 1))
            else {
                continue;
            };
            let top = a + (b - a) * tx;
            let bottom = c + (d - c) * tx;
            let s = top + (bottom - top) * ty;
            let px = bilinear(&src.image, s.x, s.y);
            let o = (y as usize * w as usize + x as usize) * 4;
            out.data[o..o + 4].copy_from_slice(&px);
        }
    }
    let _ = same;
    Ok(out)
}

/// The affine map of a picture in the model frame, for tests and for callers that place it directly.
pub fn model_affine(min: DVec2, max: DVec2, size: (u32, u32)) -> Affine {
    Affine::north_up(
        DVec2::new(min.x, max.y),
        DVec2::new(
            (max.x - min.x) / f64::from(size.0),
            (max.y - min.y) / f64::from(size.1),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn georef(crs: &str, origin: DVec2, rotation: f64) -> Georeference {
        Georeference {
            crs: crs.into(),
            origin,
            rotation,
            user_origin: DVec2::ZERO,
        }
    }

    /// A picture whose red channel runs from 0 to 255 along the columns and the green one along the rows.
    fn ramp(w: u32, h: u32) -> Rgba {
        let mut img = crate::raster::blank(w, h);
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize * 4;
                let k = |v: u32, n: u32| (v * 255 / (n - 1).max(1)) as u8;
                img.data[i..i + 4].copy_from_slice(&[k(x, w), k(y, h), 0, 255]);
            }
        }
        img
    }

    #[test]
    fn same_crs_warp_places_pixels_where_they_belong() {
        let crs = Crs::parse("EPSG:3067").unwrap();
        let g = georef("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        let frame = Frame::new(&crs, &g).unwrap();
        // 10 by 10 pixels of 1 m, top left at (385000, 6672010).
        let src = GeoRaster {
            image: ramp(10, 10),
            affine: Affine::north_up(DVec2::new(385_000.0, 6_672_010.0), DVec2::ONE),
        };
        // The whole picture: model x 0..10000 mm, y 0..10000 mm.
        let out = warp(
            &src,
            Some(&crs),
            &frame,
            DVec2::ZERO,
            DVec2::splat(10_000.0),
            (10, 10),
        )
        .unwrap();
        assert_eq!(out.data[..4], [0, 0, 0, 255]);
        let last = (9 * 10 + 9) * 4;
        assert_eq!(&out.data[last..last + 3], &[255, 255, 0]);
        // Outside the picture is clear.
        let wide = warp(
            &src,
            None,
            &frame,
            DVec2::splat(-10_000.0),
            DVec2::splat(10_000.0),
            (20, 20),
        )
        .unwrap();
        assert_eq!(wide.data[3], 0);
        let mid = (9 * 20 + 10) * 4;
        assert_eq!(wide.data[mid + 3], 255);
    }

    #[test]
    fn warp_between_crs_keeps_places() {
        // A picture in Web Mercator, shown in a UTM document.
        let doc = Crs::parse("EPSG:32635").unwrap();
        let merc = Crs::parse("EPSG:3857").unwrap();
        // Origin near Helsinki in UTM 35N.
        let g = georef("EPSG:32635", DVec2::new(385_000.0, 6_672_000.0), 0.0);
        let frame = Frame::new(&doc, &g).unwrap();
        let view_min = DVec2::new(-500_000.0, -500_000.0);
        let view_max = DVec2::new(500_000.0, 500_000.0);
        let b = frame.view_bbox(view_min, view_max, &merc).unwrap();
        let w = 64;
        let h = 64;
        // A picture of that box, whose red channel follows the column.
        let src = GeoRaster {
            image: ramp(w, h),
            affine: Affine::north_up(
                DVec2::new(b.min.x, b.max.y),
                DVec2::new(b.width() / f64::from(w), b.height() / f64::from(h)),
            ),
        };
        let out = warp(&src, Some(&merc), &frame, view_min, view_max, (32, 32)).unwrap();
        // The middle of the view is the middle of the picture, and the two agree.
        let mid = (16 * 32 + 16) * 4;
        assert_eq!(out.data[mid + 3], 255);
        assert!(
            (i32::from(out.data[mid]) - 128).abs() < 40
                && (i32::from(out.data[mid + 1]) - 128).abs() < 40,
            "{:?}",
            &out.data[mid..mid + 4]
        );
        // A point on the ground, converted by hand, lands on the same pixel.
        let probe = DVec2::new(100_000.0, -80_000.0);
        let m = frame.model_to_crs(probe, &merc).unwrap();
        let back = frame.crs_to_model(m, &merc).unwrap();
        assert!(back.distance(probe) < 1e-3, "{back} {probe}");
    }

    #[test]
    fn rotated_georeference_turns_the_picture() {
        let crs = Crs::parse("EPSG:3067").unwrap();
        let g = georef(
            "EPSG:3067",
            DVec2::new(385_000.0, 6_672_000.0),
            std::f64::consts::FRAC_PI_2,
        );
        let frame = Frame::new(&crs, &g).unwrap();
        let p = frame.model_to_crs(DVec2::new(1000.0, 0.0), &crs).unwrap();
        assert!((p - DVec2::new(385_000.0, 6_672_001.0)).length() < 1e-6);
    }

    #[test]
    fn bad_input_is_reported() {
        let geo = Crs::parse("EPSG:4326").unwrap();
        let g = georef("EPSG:4326", DVec2::ZERO, 0.0);
        assert!(Frame::new(&geo, &g).is_err());
        let crs = Crs::parse("EPSG:3067").unwrap();
        let bad = georef("EPSG:3067", DVec2::NAN, 0.0);
        assert!(Frame::new(&crs, &bad).is_err());
        let g = georef("EPSG:3067", DVec2::ZERO, 0.0);
        let frame = Frame::new(&crs, &g).unwrap();
        let src = GeoRaster {
            image: ramp(2, 2),
            affine: Affine::north_up(DVec2::ZERO, DVec2::ONE),
        };
        assert!(warp(&src, None, &frame, DVec2::ZERO, DVec2::ZERO, (4, 4)).is_err());
        assert!(warp(&src, None, &frame, DVec2::ZERO, DVec2::ONE, (0, 4)).is_err());
        let flat = GeoRaster {
            image: ramp(2, 2),
            affine: Affine {
                a: 1.0,
                b: 1.0,
                c: 0.0,
                d: 1.0,
                e: 1.0,
                f: 0.0,
            },
        };
        assert!(warp(&flat, None, &frame, DVec2::ZERO, DVec2::ONE, (4, 4)).is_err());
        // A target far outside the valid area of the source CRS gives a clear picture, not an error.
        let utm = Crs::parse("EPSG:32635").unwrap();
        let out = warp(
            &src,
            Some(&utm),
            &frame,
            DVec2::splat(1.0e8),
            DVec2::splat(1.1e8),
            (4, 4),
        )
        .unwrap();
        assert!(out.data.iter().skip(3).step_by(4).all(|a| *a == 0));
    }

    #[test]
    fn fit_size_keeps_the_shape() {
        assert_eq!(fit_size(200.0, 100.0, 1000), (1000, 500));
        assert_eq!(fit_size(100.0, 200.0, 1000), (500, 1000));
        assert_eq!(fit_size(0.0, 100.0, 1000), (1, 1));
        assert_eq!(fit_size(f64::NAN, 1.0, 10), (1, 1));
        assert_eq!(fit_size(1.0, 1000.0, 100), (1, 100));
        let a = model_affine(DVec2::new(0.0, 0.0), DVec2::new(100.0, 50.0), (10, 5));
        assert_eq!(a.apply(0.0, 0.0), DVec2::new(0.0, 50.0));
        assert_eq!(a.apply(10.0, 5.0), DVec2::new(100.0, 0.0));
    }

    proptest! {
        #[test]
        fn model_and_crs_round_trip(
            x in -50_000.0..50_000.0f64, y in -50_000.0..50_000.0f64, rot in -3.0..3.0f64,
        ) {
            let doc = Crs::parse("EPSG:3067").unwrap();
            let target = Crs::parse("EPSG:4326").unwrap();
            let g = georef("EPSG:3067", DVec2::new(385_000.0, 6_672_000.0), rot);
            let frame = Frame::new(&doc, &g).unwrap();
            let p = DVec2::new(x, y);
            let there = frame.model_to_crs(p, &target).unwrap();
            let back = frame.crs_to_model(there, &target).unwrap();
            prop_assert!(back.distance(p) < 1.0, "{} {}", back, p);
        }
    }
}
