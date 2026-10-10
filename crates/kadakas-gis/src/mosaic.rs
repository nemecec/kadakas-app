//! What WMS and WMTS share: fetching many tiles at once, putting them together, and turning the
//! result into an underlay in the model frame.

use kadakas_core::DVec2;
use kadakas_core::georef::Georeference;
use kadakas_core::underlay::{MAX_SIDE_PX, PngData, Underlay};
use kadakas_crs::Crs;
use serde::Serialize;

use crate::cache::TileCache;
use crate::http::Fetch;
use crate::raster::{Affine, GeoRaster, Rgba, blank, blit, decode_sized};
use crate::tiles::Bbox;
use crate::warp::{Frame, fit_size, warp};
use crate::{GisError, Result};

/// Tiles that are fetched at the same time.
const WORKERS: usize = 6;

/// The part of the model that a map should cover, and how sharp it should be.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewRequest {
    /// Corners of the view in model mm.
    pub min: DVec2,
    pub max: DVec2,
    /// Pixels along the long side of the result (at most `MAX_SIDE_PX`).
    pub target_px: u32,
    /// Most tiles in one request. More tiles mean the request is refused.
    pub max_tiles: usize,
}

impl ViewRequest {
    pub fn new(min: DVec2, max: DVec2) -> ViewRequest {
        ViewRequest {
            min: min.min(max),
            max: min.max(max),
            target_px: 2048,
            max_tiles: 64,
        }
    }

    pub fn check(&self) -> Result<()> {
        if !(self.min.is_finite()
            && self.max.is_finite()
            && self.max.x > self.min.x
            && self.max.y > self.min.y)
        {
            return Err(GisError::Param("the view needs a positive size".into()));
        }
        if self.target_px == 0 || self.target_px > MAX_SIDE_PX {
            return Err(GisError::Param(format!(
                "max_px must be between 1 and {MAX_SIDE_PX}"
            )));
        }
        Ok(())
    }

    /// The output size in pixels.
    pub fn size(&self) -> (u32, u32) {
        fit_size(
            self.max.x - self.min.x,
            self.max.y - self.min.y,
            self.target_px,
        )
    }
}

/// What a fetch did.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MapStats {
    pub tiles: usize,
    pub from_cache: usize,
    pub bytes: usize,
    /// The CRS of the requests.
    pub crs: String,
    /// True when the answer had to be warped from another CRS than the document CRS.
    pub reprojected: bool,
    /// The service type: `wms` or `wmts`.
    pub service: &'static str,
}

/// A fetched map in the model frame.
#[derive(Clone, Debug)]
pub struct MapOutput {
    pub underlay: Underlay,
    pub stats: MapStats,
}

/// One tile to fetch: where it lies in the server CRS, its pixel size, and its address.
#[derive(Clone, Debug)]
pub struct TilePlan {
    pub bbox: Bbox,
    pub width: u32,
    pub height: u32,
    pub url: String,
}

/// Fetches and decodes the tiles. Order is kept. Stops at the first error.
pub fn fetch_tiles(
    http: &dyn Fetch,
    cache: Option<&TileCache>,
    plans: &[TilePlan],
) -> Result<(Vec<Rgba>, usize, usize)> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new((0..plans.len()).map(|_| None).collect::<Vec<_>>());
    std::thread::scope(|s| {
        for _ in 0..WORKERS.min(plans.len()).max(1) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if i >= plans.len() {
                        break;
                    }
                    let r = fetch_one(http, cache, &plans[i]);
                    let failed = r.is_err();
                    results.lock().unwrap_or_else(|e| e.into_inner())[i] = Some(r);
                    if failed {
                        // Let the other workers stop early.
                        next.store(plans.len(), std::sync::atomic::Ordering::SeqCst);
                        break;
                    }
                }
            });
        }
    });
    let mut images = Vec::with_capacity(plans.len());
    let (mut cached, mut bytes) = (0, 0);
    let results = results.into_inner().unwrap_or_else(|e| e.into_inner());
    // The first error in tile order is the one to report.
    for r in results {
        match r {
            Some(Ok((img, hit, n))) => {
                cached += usize::from(hit);
                bytes += n;
                images.push(img);
            }
            Some(Err(e)) => return Err(e),
            None => return Err(GisError::Http("a tile was not fetched".into())),
        }
    }
    Ok((images, cached, bytes))
}

fn fetch_one(
    http: &dyn Fetch,
    cache: Option<&TileCache>,
    plan: &TilePlan,
) -> Result<(Rgba, bool, usize)> {
    let size = (plan.width, plan.height);
    // The address policy comes first: a cached tile must not get around it.
    http.check(&plan.url)?;
    // A tile from the disk is used only if it is a good picture of the right size.
    if let Some(c) = cache
        && let Some(bytes) = c.get(&plan.url)
    {
        match decode_sized(&bytes, size) {
            Ok(img) => return Ok((img, true, bytes.len())),
            Err(_) => c.remove(&plan.url),
        }
    }
    let resp = http.get(&plan.url)?;
    // A WMS sends an error as XML, often with status 200.
    if resp.content_type.contains("xml") || resp.body.first() == Some(&b'<') {
        let text = String::from_utf8_lossy(&resp.body);
        return Err(match crate::caps::detect(&text) {
            Err(e) => e,
            Ok(_) => GisError::Format("the server sent XML instead of a picture".into()),
        });
    }
    let img = decode_sized(&resp.body, size)?;
    // Only a good picture is kept.
    if let Some(c) = cache {
        c.put(&plan.url, &resp.body);
    }
    Ok((img, false, resp.body.len()))
}

/// Puts tiles of one pixel size together into one picture in their CRS.
pub fn assemble(tiles: &[(Bbox, Rgba)]) -> Result<GeoRaster> {
    let first = tiles
        .first()
        .ok_or_else(|| GisError::Format("no tiles".into()))?;
    let pixel = DVec2::new(
        first.0.width() / f64::from(first.1.width),
        first.0.height() / f64::from(first.1.height),
    );
    let all = tiles.iter().skip(1).fold(first.0, |b, t| b.union(&t.0));
    let w = (all.width() / pixel.x).round();
    let h = (all.height() / pixel.y).round();
    if !(w >= 1.0 && h >= 1.0) || w * h > crate::raster::MAX_PIXELS as f64 {
        return Err(GisError::Limit("the tiles make too large a picture".into()));
    }
    let mut out = blank(w as u32, h as u32);
    for (b, img) in tiles {
        let x = ((b.min.x - all.min.x) / pixel.x).round() as i64;
        let y = ((all.max.y - b.max.y) / pixel.y).round() as i64;
        blit(&mut out, img, x, y);
    }
    Ok(GeoRaster {
        image: out,
        affine: Affine::north_up(DVec2::new(all.min.x, all.max.y), pixel),
    })
}

/// Warps a picture that came from a server into the model frame and wraps it as an underlay.
#[allow(clippy::too_many_arguments)]
pub fn finish(
    mosaic: &GeoRaster,
    server_crs: &Crs,
    doc_crs: &Crs,
    georef: &Georeference,
    view: &ViewRequest,
    name: &str,
    source: &str,
    mut stats: MapStats,
) -> Result<MapOutput> {
    let frame = Frame::new(doc_crs, georef)?;
    let size = view.size();
    let out = warp(mosaic, Some(server_crs), &frame, view.min, view.max, size)?;
    stats.reprojected = server_crs.spec != doc_crs.spec && server_crs.epsg() != doc_crs.epsg();
    Ok(MapOutput {
        underlay: Underlay {
            name: name.to_string(),
            min: view.min,
            max: view.max,
            width: size.0,
            height: size.1,
            png: PngData(std::sync::Arc::new(out.to_png())),
            opacity: 1.0,
            source: source.to_string(),
        },
        stats,
    })
}

/// Percent-encodes a value for a query string. Letters, digits and `-_.~` stay.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Adds query parameters to an address that may have some already.
pub fn with_query(base: &str, params: &[(&str, String)]) -> String {
    let mut url = base.trim().to_string();
    // The address of a server often ends in `?` or has a map file in its query.
    if !url.contains('?') {
        url.push('?');
    } else if !url.ends_with('?') && !url.ends_with('&') {
        url.push('&');
    }
    let rest: Vec<String> = params.iter().map(|(k, v)| format!("{k}={v}")).collect();
    url.push_str(&rest.join("&"));
    url
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Response;

    use crate::testing::{Fake, png, solid};

    fn plan(i: i32, url: &str) -> TilePlan {
        TilePlan {
            bbox: Bbox::new(
                DVec2::new(f64::from(i) * 10.0, 0.0),
                DVec2::new(f64::from(i) * 10.0 + 10.0, 10.0),
            ),
            width: 4,
            height: 4,
            url: url.into(),
        }
    }

    #[test]
    fn the_address_policy_applies_before_the_cache() {
        struct Deny(Fake);
        impl Fetch for Deny {
            fn get(&self, url: &str) -> Result<Response> {
                self.0.get(url)
            }
            fn check(&self, url: &str) -> Result<()> {
                Err(GisError::Url(format!("{url} is not on the list")))
            }
        }
        let mut f = Fake::default();
        f.answers.insert("u0".into(), png(4, 4, [1, 2, 3, 255]));
        let dir = std::env::temp_dir().join(format!("kadakas-mosaic-deny-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = TileCache::new(&dir, 1 << 20);
        let plans = [plan(0, "u0")];
        // A tile is in the cache from an earlier, allowed fetch.
        fetch_tiles(&f, Some(&cache), &plans).unwrap();
        let denied = Deny(f);
        let e = fetch_tiles(&denied, Some(&cache), &plans).unwrap_err();
        assert!(matches!(e, GisError::Url(_)), "{e:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tiles_come_back_in_order_and_go_through_the_cache() {
        let mut f = Fake::default();
        for i in 0..8 {
            f.answers
                .insert(format!("u{i}"), png(4, 4, [i as u8 * 10, 0, 0, 255]));
        }
        let plans: Vec<TilePlan> = (0..8).map(|i| plan(i, &format!("u{i}"))).collect();
        let dir = std::env::temp_dir().join(format!("kadakas-mosaic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = TileCache::new(&dir, 1 << 20);
        let (imgs, cached, bytes) = fetch_tiles(&f, Some(&cache), &plans).unwrap();
        assert_eq!(imgs.len(), 8);
        assert_eq!(
            (0..8).map(|i| imgs[i].data[0]).collect::<Vec<_>>(),
            (0..8).map(|i| i as u8 * 10).collect::<Vec<_>>()
        );
        assert_eq!(cached, 0);
        assert!(bytes > 0);
        let (_, cached, _) = fetch_tiles(&f, Some(&cache), &plans).unwrap();
        assert_eq!(cached, 8);
        assert_eq!(f.log.lock().unwrap().len(), 8);
        let (imgs, _, _) = fetch_tiles(&f, None, &[]).unwrap();
        assert!(imgs.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn error_answers_are_never_cached_and_bad_cached_tiles_are_fetched_again() {
        let dir = std::env::temp_dir().join(format!("kadakas-mosaic-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = TileCache::new(&dir, 1 << 20);
        let mut f = Fake::default();
        f.answers.insert(
            "err".into(),
            Response {
                status: 200,
                content_type: "text/xml".into(),
                body: br#"<ServiceExceptionReport><ServiceException>Busy</ServiceException></ServiceExceptionReport>"#.to_vec(),
            },
        );
        f.answers.insert("small".into(), png(2, 2, [1, 2, 3, 255]));
        assert!(fetch_tiles(&f, Some(&cache), &[plan(0, "err")]).is_err());
        assert!(fetch_tiles(&f, Some(&cache), &[plan(0, "small")]).is_err());
        assert_eq!(cache.stats().0, 0, "nothing bad was kept");
        f.answers.insert("good".into(), png(4, 4, [9, 9, 9, 255]));
        cache.put("good", b"damaged");
        let (imgs, cached, _) = fetch_tiles(&f, Some(&cache), &[plan(0, "good")]).unwrap();
        assert_eq!((imgs.len(), cached), (1, 0));
        assert_eq!(cache.stats().0, 1);
        let (_, cached, _) = fetch_tiles(&f, Some(&cache), &[plan(0, "good")]).unwrap();
        assert_eq!(cached, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tile_errors_are_reported() {
        let mut f = Fake::default();
        f.answers.insert("ok".into(), png(4, 4, [1, 2, 3, 255]));
        f.answers.insert("small".into(), png(2, 2, [1, 2, 3, 255]));
        f.answers.insert(
            "xml".into(),
            Response {
                status: 200,
                content_type: "text/xml".into(),
                body: br#"<ServiceExceptionReport><ServiceException>Bad layer</ServiceException></ServiceExceptionReport>"#.to_vec(),
            },
        );
        f.answers.insert(
            "html".into(),
            Response {
                status: 200,
                content_type: "text/html".into(),
                body: b"<html></html>".to_vec(),
            },
        );
        f.answers.insert(
            "junk".into(),
            Response {
                status: 200,
                content_type: "image/png".into(),
                body: b"junk".to_vec(),
            },
        );
        let e = |name: &str| {
            fetch_tiles(&f, None, &[plan(0, "ok"), plan(1, name)])
                .unwrap_err()
                .to_string()
        };
        assert!(e("xml").contains("Bad layer"));
        assert!(e("small").contains("2 by 2"));
        assert!(e("missing").contains("404"));
        assert!(e("html").contains("not a capabilities") || e("html").contains("XML"));
        assert!(e("junk").contains("cannot read"));
    }

    #[test]
    fn assembles_tiles_by_position() {
        let a = (
            Bbox::new(DVec2::new(0.0, 10.0), DVec2::new(10.0, 20.0)),
            solid(5, 5, [255, 0, 0, 255]),
        );
        let b = (
            Bbox::new(DVec2::new(10.0, 0.0), DVec2::new(20.0, 10.0)),
            solid(5, 5, [0, 0, 255, 255]),
        );
        let g = assemble(&[a, b]).unwrap();
        assert_eq!((g.image.width, g.image.height), (10, 10));
        assert_eq!(g.image.data[0], 255);
        let bottom_right = (9 * 10 + 9) * 4;
        assert_eq!(g.image.data[bottom_right + 2], 255);
        let top_right = 9 * 4;
        assert_eq!(g.image.data[top_right + 3], 0);
        assert_eq!(g.affine.apply(0.0, 0.0), DVec2::new(0.0, 20.0));
        assert!(assemble(&[]).is_err());
    }

    #[test]
    fn view_request_checks() {
        let v = ViewRequest::new(DVec2::new(10.0, 10.0), DVec2::new(0.0, 5.0));
        assert!(v.check().is_ok());
        assert_eq!(v.min, DVec2::new(0.0, 5.0));
        assert_eq!(v.size(), (2048, 1024));
        assert!(ViewRequest::new(DVec2::ZERO, DVec2::ZERO).check().is_err());
        assert!(ViewRequest { target_px: 0, ..v }.check().is_err());
        assert!(
            ViewRequest {
                target_px: 100_000,
                ..v
            }
            .check()
            .is_err()
        );
        assert!(ViewRequest::new(DVec2::NAN, DVec2::ONE).check().is_err());
    }

    #[test]
    fn query_helpers() {
        assert_eq!(encode("a b/c"), "a%20b%2Fc");
        assert_eq!(encode("EPSG-1.x_~"), "EPSG-1.x_~");
        assert_eq!(
            with_query("http://h/wms", &[("A", "1".into()), ("B", "2".into())]),
            "http://h/wms?A=1&B=2"
        );
        assert_eq!(
            with_query("http://h/wms?", &[("A", "1".into())]),
            "http://h/wms?A=1"
        );
        assert_eq!(
            with_query("http://h/cgi?map=a", &[("A", "1".into())]),
            "http://h/cgi?map=a&A=1"
        );
        assert_eq!(
            with_query("http://h/cgi?map=a&", &[("A", "1".into())]),
            "http://h/cgi?map=a&A=1"
        );
    }
}
