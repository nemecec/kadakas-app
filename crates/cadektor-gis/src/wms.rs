//! WMS 1.3.0 (and 1.1.1) map backgrounds.
//!
//! The view of the document is cut into tiles on a fixed grid in the CRS of the requests, each tile is
//! one GetMap request (and one file in the tile cache), and the tiles are warped into the model frame.
//! The requests use the CRS of the document when the server offers it. Otherwise they use Web Mercator
//! (EPSG:3857) or EPSG:4326, and the warp makes the picture fit the document.

use cadektor_core::DVec2;
use cadektor_core::georef::Georeference;
use cadektor_io::crs::Crs;
use serde::{Deserialize, Serialize};

use crate::cache::TileCache;
use crate::caps::WmsCaps;
use crate::http::{Fetch, shorten};
use crate::mosaic::{
    MapOutput, MapStats, TilePlan, ViewRequest, assemble, encode, fetch_tiles, finish, with_query,
};
use crate::tiles::{Bbox, TileGrid, wms_bbox};
use crate::warp::Frame;
use crate::{GisError, Result};

/// Pixels on a side of a request tile.
pub const TILE_PX: u32 = 512;

/// A WMS source: what the user chose in the dialog, or an agent in `gis.wms.add`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WmsSource {
    /// Address of the server (the part before the query).
    pub url: String,
    /// Layers to draw, bottom first.
    pub layers: Vec<String>,
    /// One style per layer. Empty strings (or a short list) mean the default style.
    #[serde(default)]
    pub styles: Vec<String>,
    /// `EPSG:...` to request. Empty: chosen from the document CRS and the server's list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crs: Option<String>,
    /// An image format such as `image/png`. Empty: PNG, or what the server offers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Ask for a clear background (default true; only PNG and GIF can be clear).
    #[serde(default = "yes")]
    pub transparent: bool,
    /// The WMS version. Default: the version of the capabilities document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

fn yes() -> bool {
    true
}

impl WmsSource {
    pub fn new(url: &str, layers: &[&str]) -> WmsSource {
        WmsSource {
            url: url.to_string(),
            layers: layers.iter().map(|s| s.to_string()).collect(),
            styles: Vec::new(),
            crs: None,
            format: None,
            transparent: true,
            version: None,
        }
    }

    /// A short name for lists: the layers on the host.
    pub fn label(&self) -> String {
        format!("{} ({})", self.layers.join(", "), shorten(&self.url))
    }
}

/// The address for GetCapabilities.
pub fn capabilities_url(base: &str) -> String {
    with_query(
        base,
        &[
            ("SERVICE", "WMS".into()),
            ("REQUEST", "GetCapabilities".into()),
        ],
    )
}

/// Fetches and reads the capabilities of a WMS.
pub fn fetch_capabilities(http: &dyn Fetch, base: &str) -> Result<WmsCaps> {
    let r = http.get(&capabilities_url(base))?;
    crate::caps::parse_wms(&String::from_utf8_lossy(&r.body))
}

/// The CRS to request: the one the source names, else the document CRS if every layer has it, else
/// Web Mercator, else EPSG:4326, else the first CRS the server offers that can be converted.
pub fn choose_crs(source: &WmsSource, caps: &WmsCaps, doc_crs: &Crs) -> Result<Crs> {
    let offered = caps.common_crs(&source.layers);
    let has = |c: &str| offered.iter().any(|o| o.eq_ignore_ascii_case(c));
    if let Some(want) = &source.crs {
        let c = Crs::parse(want).map_err(GisError::Crs)?;
        if !offered.is_empty() && !has(&c.spec) {
            return Err(GisError::Param(format!(
                "the server does not offer {} for these layers. It offers: {}",
                c.spec,
                offered.join(", ")
            )));
        }
        return Ok(c);
    }
    let candidates = std::iter::once(doc_crs.spec.clone())
        .chain([
            "EPSG:3857".to_string(),
            "EPSG:4326".to_string(),
            "CRS:84".to_string(),
        ])
        .chain(offered.iter().cloned());
    for c in candidates {
        if (has(&c) || (offered.is_empty() && c == doc_crs.spec))
            && let Ok(parsed) = Crs::parse(&c.replace("CRS:84", "EPSG:4326"))
        {
            return Ok(parsed);
        }
    }
    Err(GisError::Param(
        "the server offers no CRS that Cadektor can convert from. Name one with crs".into(),
    ))
}

fn version_of(source: &WmsSource, caps: &WmsCaps) -> String {
    source
        .version
        .clone()
        .unwrap_or_else(|| caps.version.clone())
}

/// The GetMap address for one tile.
pub fn getmap_url(
    base: &str,
    source: &WmsSource,
    version: &str,
    format: &str,
    crs: &Crs,
    bbox: &Bbox,
    size: (u32, u32),
) -> String {
    let v13 = version.starts_with("1.3");
    let layers: Vec<String> = source.layers.iter().map(|l| encode(l)).collect();
    let styles: Vec<String> = (0..source.layers.len())
        .map(|i| encode(source.styles.get(i).map_or("", String::as_str)))
        .collect();
    let mut params: Vec<(&str, String)> = vec![
        ("SERVICE", "WMS".into()),
        ("VERSION", version.to_string()),
        ("REQUEST", "GetMap".into()),
        ("LAYERS", layers.join(",")),
        ("STYLES", styles.join(",")),
        (if v13 { "CRS" } else { "SRS" }, encode(&crs.spec)),
        (
            "BBOX",
            wms_bbox(bbox, crs.epsg(), crs.is_geographic(), version),
        ),
        ("WIDTH", size.0.to_string()),
        ("HEIGHT", size.1.to_string()),
        ("FORMAT", encode(format)),
    ];
    if source.transparent {
        params.push(("TRANSPARENT", "TRUE".into()));
    }
    params.push((
        "EXCEPTIONS",
        encode(if v13 {
            "XML"
        } else {
            "application/vnd.ogc.se_xml"
        }),
    ));
    with_query(base, &params)
}

/// The tiles that a view needs, with their addresses.
pub fn plan_tiles(
    source: &WmsSource,
    caps: &WmsCaps,
    doc_crs: &Crs,
    georef: &Georeference,
    view: &ViewRequest,
) -> Result<(Crs, Vec<TilePlan>)> {
    view.check()?;
    if source.layers.is_empty() {
        return Err(GisError::Param("name at least one layer".into()));
    }
    for l in &source.layers {
        if caps.layer(l).is_none() {
            return Err(GisError::Param(format!(
                "the server has no layer named {l}"
            )));
        }
    }
    let frame = Frame::new(doc_crs, georef)?;
    let crs = choose_crs(source, caps, doc_crs)?;
    let format = source
        .format
        .clone()
        .or_else(|| caps.pick_format())
        .unwrap_or_else(|| "image/png".into());
    let version = version_of(source, caps);
    let base = caps
        .getmap_url
        .clone()
        .filter(|u| crate::http::parse_url(u).is_ok())
        .unwrap_or_else(|| source.url.clone());
    let mut bbox = frame.view_bbox(view.min, view.max, &crs)?;
    if crs.is_geographic() {
        bbox = Bbox::new(
            DVec2::new(bbox.min.x.max(-180.0), bbox.min.y.max(-90.0)),
            DVec2::new(bbox.max.x.min(180.0), bbox.max.y.min(90.0)),
        );
    }
    if !bbox.is_finite() || bbox.width() <= 0.0 || bbox.height() <= 0.0 {
        return Err(GisError::Param(
            "the view lies outside the area where the server CRS is valid".into(),
        ));
    }
    let mut grid = TileGrid::for_extent(bbox.width().max(bbox.height()), view.target_px, TILE_PX);
    // Too many tiles: take coarser tiles until it fits.
    let mut tries = 0;
    while grid.tile_count(&bbox) > view.max_tiles as u64 {
        grid.pixel *= 2.0;
        tries += 1;
        if tries > 8 {
            return Err(GisError::Limit("the view needs too many tiles".into()));
        }
    }
    let (c0, c1, r0, r1) = grid.range(&bbox);
    let mut plans = Vec::new();
    for row in (r0..=r1).rev() {
        for col in c0..=c1 {
            let b = grid.tile_bbox(col, row);
            plans.push(TilePlan {
                bbox: b,
                width: grid.tile_px,
                height: grid.tile_px,
                url: getmap_url(
                    &base,
                    source,
                    &version,
                    &format,
                    &crs,
                    &b,
                    (grid.tile_px, grid.tile_px),
                ),
            });
        }
    }
    Ok((crs, plans))
}

/// Fetches the map for a view and returns it as an underlay in the model frame.
pub fn fetch_map(
    http: &dyn Fetch,
    cache: Option<&TileCache>,
    source: &WmsSource,
    caps: &WmsCaps,
    doc_crs: &Crs,
    georef: &Georeference,
    view: &ViewRequest,
) -> Result<MapOutput> {
    let (crs, plans) = plan_tiles(source, caps, doc_crs, georef, view)?;
    let (images, from_cache, bytes) = fetch_tiles(http, cache, &plans)?;
    let tiles: Vec<(Bbox, _)> = plans.iter().map(|p| p.bbox).zip(images).collect();
    let mosaic = assemble(&tiles)?;
    let stats = MapStats {
        tiles: plans.len(),
        from_cache,
        bytes,
        crs: crs.spec.clone(),
        reprojected: false,
        service: "wms",
    };
    finish(
        &mosaic,
        &crs,
        doc_crs,
        georef,
        view,
        &source.layers.join(", "),
        &source.label(),
        stats,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caps::fixtures::{WMS_111, WMS_130};
    use crate::testing::{Fake, png};

    fn caps() -> WmsCaps {
        crate::caps::parse_wms(WMS_130).unwrap()
    }

    fn doc() -> (Crs, Georeference) {
        (
            Crs::parse("EPSG:3067").unwrap(),
            Georeference {
                crs: "EPSG:3067".into(),
                origin: DVec2::new(385_000.0, 6_672_000.0),
                rotation: 0.0,
                user_origin: DVec2::ZERO,
            },
        )
    }

    #[test]
    fn builds_requests_for_both_versions() {
        let c = caps();
        let s = WmsSource::new("http://example.test/wms", &["ortho", "roads"]);
        let crs = Crs::parse("EPSG:4326").unwrap();
        let b = Bbox::new(DVec2::new(24.0, 60.0), DVec2::new(25.0, 61.0));
        let u = getmap_url(
            &c.getmap_url.clone().unwrap(),
            &s,
            "1.3.0",
            "image/png",
            &crs,
            &b,
            (256, 128),
        );
        assert_eq!(
            u,
            "http://example.test/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=ortho,roads&STYLES=,&CRS=EPSG%3A4326&BBOX=60,24,61,25&WIDTH=256&HEIGHT=128&FORMAT=image%2Fpng&TRANSPARENT=TRUE&EXCEPTIONS=XML"
        );
        let mut s2 = s.clone();
        s2.transparent = false;
        s2.styles = vec!["default".into()];
        let u = getmap_url(
            "http://old.test/cgi?map=a",
            &s2,
            "1.1.1",
            "image/gif",
            &crs,
            &b,
            (10, 10),
        );
        assert!(u.starts_with("http://old.test/cgi?map=a&SERVICE=WMS&VERSION=1.1.1"));
        assert!(u.contains("&SRS=EPSG%3A4326&BBOX=24,60,25,61&"));
        assert!(u.contains("STYLES=default,&"));
        assert!(!u.contains("TRANSPARENT"));
        assert!(u.ends_with("EXCEPTIONS=application%2Fvnd.ogc.se_xml"));
        assert_eq!(
            capabilities_url("http://x/wms"),
            "http://x/wms?SERVICE=WMS&REQUEST=GetCapabilities"
        );
        assert!(s.label().contains("ortho, roads (http://example.test/wms)"));
    }

    #[test]
    fn chooses_the_crs() {
        let c = caps();
        let (doc_crs, _) = doc();
        let mut s = WmsSource::new("http://example.test/wms", &["ortho"]);
        // The layer offers EPSG:3067 itself.
        assert_eq!(choose_crs(&s, &c, &doc_crs).unwrap().spec, "EPSG:3067");
        // Both layers together: only the common CRSs, so Web Mercator.
        s.layers = vec!["ortho".into(), "roads".into()];
        assert_eq!(choose_crs(&s, &c, &doc_crs).unwrap().spec, "EPSG:3857");
        s.crs = Some("EPSG:4326".into());
        assert_eq!(choose_crs(&s, &c, &doc_crs).unwrap().spec, "EPSG:4326");
        s.crs = Some("EPSG:3067".into());
        let e = choose_crs(&s, &c, &doc_crs).unwrap_err().to_string();
        assert!(e.contains("does not offer EPSG:3067"), "{e}");
        s.crs = Some("nonsense".into());
        assert!(choose_crs(&s, &c, &doc_crs).is_err());
        // A server with 4326 only.
        let old =
            crate::caps::parse_wms(&WMS_111.replace("EPSG:4326 EPSG:3857", "EPSG:4326")).unwrap();
        let s3 = WmsSource::new("http://old.test/cgi", &["countries"]);
        assert_eq!(choose_crs(&s3, &old, &doc_crs).unwrap().spec, "EPSG:4326");
        // A server with a CRS that is not known: no choice.
        let odd =
            crate::caps::parse_wms(&WMS_111.replace("EPSG:4326 EPSG:3857", "EPSG:1")).unwrap();
        assert!(choose_crs(&s3, &odd, &doc_crs).is_err());
        // A layer list with no layers in the document: the document CRS is tried.
        let none = WmsSource::new("http://x", &[]);
        assert_eq!(choose_crs(&none, &c, &doc_crs).unwrap().spec, "EPSG:3067");
        let _ = version_of(&s3, &old);
    }

    #[test]
    fn plans_tiles_in_the_document_crs() {
        let c = caps();
        let (crs, g) = doc();
        let s = WmsSource::new("http://example.test/wms", &["ortho"]);
        let view = ViewRequest::new(DVec2::ZERO, DVec2::new(1_000_000.0, 600_000.0));
        let (used, plans) = plan_tiles(&s, &c, &crs, &g, &view).unwrap();
        assert_eq!(used.spec, "EPSG:3067");
        assert!(!plans.is_empty() && plans.len() <= 64);
        // The tiles cover the view.
        let all = plans
            .iter()
            .skip(1)
            .fold(plans[0].bbox, |b, p| b.union(&p.bbox));
        assert!(all.min.x <= 385_000.0 && all.max.x >= 386_000.0);
        assert!(all.min.y <= 6_672_000.0 && all.max.y >= 6_672_600.0);
        assert!(
            plans
                .iter()
                .all(|p| p.url.contains("CRS=EPSG%3A3067") && p.url.contains("WIDTH=512"))
        );
        // The same view gives the same addresses (so the cache works).
        let (_, again) = plan_tiles(&s, &c, &crs, &g, &view).unwrap();
        assert_eq!(
            plans.iter().map(|p| &p.url).collect::<Vec<_>>(),
            again.iter().map(|p| &p.url).collect::<Vec<_>>()
        );
        // A small pan inside the same tiles changes nothing.
        let mut panned = view;
        panned.min += DVec2::new(5.0, 5.0);
        panned.max += DVec2::new(5.0, 5.0);
        let (_, p2) = plan_tiles(&s, &c, &crs, &g, &panned).unwrap();
        assert!(
            p2.iter()
                .filter(|p| plans.iter().any(|q| q.url == p.url))
                .count()
                >= plans.len() - 3
        );
    }

    #[test]
    fn limits_and_errors() {
        let c = caps();
        let (crs, g) = doc();
        let s = WmsSource::new("http://example.test/wms", &["ortho"]);
        let mut view = ViewRequest::new(DVec2::ZERO, DVec2::new(1_000_000.0, 600_000.0));
        view.max_tiles = 1;
        let (_, plans) = plan_tiles(&s, &c, &crs, &g, &view).unwrap();
        assert_eq!(plans.len(), 1);
        let bad_layer = WmsSource::new("http://example.test/wms", &["nope"]);
        assert!(
            plan_tiles(&bad_layer, &c, &crs, &g, &view)
                .unwrap_err()
                .to_string()
                .contains("no layer named nope")
        );
        let none = WmsSource::new("http://example.test/wms", &[]);
        assert!(plan_tiles(&none, &c, &crs, &g, &view).is_err());
        let zero = ViewRequest::new(DVec2::ZERO, DVec2::ZERO);
        assert!(plan_tiles(&s, &c, &crs, &g, &zero).is_err());
        // A document with a geographic CRS is refused.
        let geo = Crs::parse("EPSG:4326").unwrap();
        assert!(plan_tiles(&s, &c, &geo, &g, &view).is_err());
        // A server that names a bad GetMap address: the address of the source is used.
        let mut c2 = c.clone();
        c2.getmap_url = Some("not a url".into());
        let (_, plans) = plan_tiles(&s, &c2, &crs, &g, &view).unwrap();
        assert!(plans[0].url.starts_with("http://example.test/wms?"));
    }

    #[test]
    fn fetches_and_warps_a_map_from_a_different_crs() {
        // The server only has Web Mercator for both layers; the document is in TM35FIN.
        let c = caps();
        let (crs, g) = doc();
        let s = WmsSource::new("http://example.test/wms", &["ortho", "roads"]);
        let view = ViewRequest {
            target_px: 256,
            ..ViewRequest::new(DVec2::ZERO, DVec2::new(400_000.0, 400_000.0))
        };
        let (used, plans) = plan_tiles(&s, &c, &crs, &g, &view).unwrap();
        assert_eq!(used.spec, "EPSG:3857");
        let mut fake = Fake::default();
        for p in &plans {
            fake.answers
                .insert(p.url.clone(), png(512, 512, [200, 100, 50, 255]));
        }
        let out = fetch_map(&fake, None, &s, &c, &crs, &g, &view).unwrap();
        assert_eq!(out.stats.service, "wms");
        assert!(out.stats.reprojected);
        assert_eq!(out.stats.tiles, plans.len());
        assert_eq!(out.stats.crs, "EPSG:3857");
        let u = &out.underlay;
        assert_eq!((u.width, u.height), (256, 256));
        assert_eq!((u.min, u.max), (view.min, view.max));
        assert!(u.problem().is_none());
        // The whole picture is covered by the solid tiles.
        let img = u.decode(4096).unwrap();
        assert!(img.data.chunks(4).all(|p| p == [200, 100, 50, 255]));
        assert!(u.source.contains("ortho, roads"));
    }

    #[test]
    fn a_server_error_is_reported() {
        let c = caps();
        let (crs, g) = doc();
        let s = WmsSource::new("http://example.test/wms", &["ortho"]);
        let view = ViewRequest::new(DVec2::ZERO, DVec2::new(1000.0, 1000.0));
        let fake = Fake::default();
        let e = fetch_map(&fake, None, &s, &c, &crs, &g, &view).unwrap_err();
        assert!(e.to_string().contains("404"), "{e}");
    }

    #[test]
    fn capabilities_are_fetched_and_read() {
        let mut fake = Fake::default();
        fake.answers.insert(
            capabilities_url("http://example.test/wms"),
            crate::testing::text(WMS_130, "text/xml"),
        );
        let c = fetch_capabilities(&fake, "http://example.test/wms").unwrap();
        assert_eq!(c.layers.len(), 4);
        assert!(fetch_capabilities(&fake, "http://example.test/other").is_err());
        let s = serde_json::to_string(&WmsSource::new("http://x", &["a"])).unwrap();
        assert!(!s.contains("crs"));
        assert!(
            serde_json::from_str::<WmsSource>(r#"{"url":"http://x","layers":["a"]}"#)
                .unwrap()
                .transparent
        );
    }
}
