//! WMTS 1.0.0 map backgrounds.
//!
//! The tiles come from the tile matrix set that fits the document: the CRS of the document if the
//! layer has a set for it, else Web Mercator. The level is the one whose pixels are the size that the
//! view needs. The tiles are put together and warped into the model frame like WMS tiles.

use cadektor_core::georef::Georeference;
use cadektor_io::crs::Crs;
use serde::{Deserialize, Serialize};

use crate::cache::TileCache;
use crate::caps::{MatrixSet, WmtsCaps, WmtsLayer};
use crate::http::{Fetch, shorten};
use crate::mosaic::{
    MapOutput, MapStats, TilePlan, ViewRequest, assemble, encode, fetch_tiles, finish, with_query,
};
use crate::tiles::{Bbox, METERS_PER_DEGREE, TileMatrix, pick_matrix};
use crate::warp::Frame;
use crate::{GisError, Result};

/// A WMTS source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WmtsSource {
    /// Address of the server (for GetCapabilities and, without a tile template, for GetTile).
    pub url: String,
    pub layer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix_set: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

impl WmtsSource {
    pub fn new(url: &str, layer: &str) -> WmtsSource {
        WmtsSource {
            url: url.to_string(),
            layer: layer.to_string(),
            matrix_set: None,
            style: None,
            format: None,
        }
    }

    pub fn label(&self) -> String {
        format!("{} ({})", self.layer, shorten(&self.url))
    }
}

pub fn capabilities_url(base: &str) -> String {
    with_query(
        base,
        &[
            ("SERVICE", "WMTS".into()),
            ("REQUEST", "GetCapabilities".into()),
            ("VERSION", "1.0.0".into()),
        ],
    )
}

pub fn fetch_capabilities(http: &dyn Fetch, base: &str) -> Result<WmtsCaps> {
    let r = http.get(&capabilities_url(base))?;
    crate::caps::parse_wmts(&String::from_utf8_lossy(&r.body))
}

fn units_per_meter(crs: &Crs) -> f64 {
    if crs.is_geographic() {
        METERS_PER_DEGREE
    } else {
        crs.to_meter()
    }
}

/// The tile matrix set to use: the one named, else one in the CRS of the document, else Web Mercator,
/// else EPSG:4326, else the first.
pub fn choose_set<'a>(
    source: &WmtsSource,
    layer: &WmtsLayer,
    caps: &'a WmtsCaps,
    doc_crs: &Crs,
) -> Result<(&'a MatrixSet, Crs)> {
    let sets: Vec<&MatrixSet> = layer
        .matrix_sets
        .iter()
        .filter_map(|id| caps.matrix_set(id))
        .collect();
    let parsed = |s: &MatrixSet| Crs::parse(&s.crs).ok();
    if let Some(want) = &source.matrix_set {
        let s = sets.iter().find(|s| &s.id == want).ok_or_else(|| {
            GisError::Param(format!(
                "the layer has no tile matrix set {want}. It has: {}",
                layer.matrix_sets.join(", ")
            ))
        })?;
        let c = parsed(s).ok_or_else(|| GisError::Crs(format!("unknown CRS {}", s.crs)))?;
        return Ok((s, c));
    }
    let same = |c: &Crs, epsg: u32| c.epsg() == Some(epsg);
    let doc_epsg = doc_crs.epsg();
    let pick = |f: &dyn Fn(&Crs) -> bool| {
        sets.iter()
            .find_map(|s| parsed(s).filter(|c| f(c)).map(|c| (*s, c)))
    };
    pick(&|c| c.epsg().is_some() && c.epsg() == doc_epsg)
        .or_else(|| pick(&|c| same(c, 3857)))
        .or_else(|| pick(&|c| same(c, 4326)))
        .or_else(|| pick(&|_| true))
        .ok_or_else(|| GisError::Param("the layer has no usable tile matrix set".into()))
}

/// Puts the values into a tile address template. Names are not case sensitive.
pub fn fill_template(template: &str, values: &[(&str, String)]) -> Result<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        let j = tail
            .find('}')
            .ok_or_else(|| GisError::Format("the tile address template is not closed".into()))?;
        let name = &tail[..j];
        let v = values
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .ok_or_else(|| {
                GisError::Param(format!(
                    "the tile address needs a value for {{{name}}}, which Cadektor does not set"
                ))
            })?;
        out.push_str(&v.1);
        rest = &tail[j + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn tile_url(
    source: &WmtsSource,
    layer: &WmtsLayer,
    caps: &WmtsCaps,
    set: &MatrixSet,
    m: &TileMatrix,
    format: &str,
    style: &str,
    col: u64,
    row: u64,
) -> Result<String> {
    if let Some(r) = layer
        .resource_urls
        .iter()
        .find(|r| r.format.eq_ignore_ascii_case(format))
    {
        return fill_template(
            &r.template,
            &[
                ("TileMatrixSet", set.id.clone()),
                ("TileMatrix", m.id.clone()),
                ("TileRow", row.to_string()),
                ("TileCol", col.to_string()),
                ("Style", style.to_string()),
                ("Layer", layer.id.clone()),
            ],
        );
    }
    let base = caps
        .kvp_url
        .clone()
        .filter(|u| crate::http::parse_url(u).is_ok())
        .unwrap_or_else(|| source.url.clone());
    Ok(with_query(
        &base,
        &[
            ("SERVICE", "WMTS".into()),
            ("REQUEST", "GetTile".into()),
            ("VERSION", "1.0.0".into()),
            ("LAYER", encode(&layer.id)),
            ("STYLE", encode(style)),
            ("TILEMATRIXSET", encode(&set.id)),
            ("TILEMATRIX", encode(&m.id)),
            ("TILEROW", row.to_string()),
            ("TILECOL", col.to_string()),
            ("FORMAT", encode(format)),
        ],
    ))
}

/// The tiles that a view needs. Returns the CRS of the tiles too.
pub fn plan_tiles(
    source: &WmtsSource,
    caps: &WmtsCaps,
    doc_crs: &Crs,
    georef: &Georeference,
    view: &ViewRequest,
) -> Result<(Crs, Vec<TilePlan>)> {
    view.check()?;
    let layer = caps.layer(&source.layer).ok_or_else(|| {
        GisError::Param(format!("the server has no layer named {}", source.layer))
    })?;
    let frame = Frame::new(doc_crs, georef)?;
    let (set, crs) = choose_set(source, layer, caps, doc_crs)?;
    let mpu = units_per_meter(&crs);
    let format = source
        .format
        .clone()
        .or_else(|| {
            layer
                .formats
                .iter()
                .find(|f| f.eq_ignore_ascii_case("image/png"))
                .or(layer.formats.first())
                .cloned()
        })
        .unwrap_or_else(|| "image/png".into());
    let style = source
        .style
        .clone()
        .or_else(|| layer.default_style.clone())
        .unwrap_or_else(|| "default".into());
    let bbox = frame.view_bbox(view.min, view.max, &crs)?;
    let wanted = bbox.width().max(bbox.height()) / f64::from(view.target_px);
    // Levels from the best fit to coarser ones, until the tile count is small enough.
    let mut order: Vec<usize> = (0..set.matrices.len()).collect();
    order.sort_by(|a, b| {
        set.matrices[*b]
            .pixel_size(mpu)
            .total_cmp(&set.matrices[*a].pixel_size(mpu))
    });
    let start = pick_matrix(&set.matrices, wanted, mpu)
        .ok_or_else(|| GisError::Format("the tile matrix set has no levels".into()))?;
    let from = order.iter().position(|i| *i == start).unwrap_or(0);
    for &i in &order[..=from].iter().rev().copied().collect::<Vec<_>>() {
        let m = &set.matrices[i];
        let Some((c0, c1, r0, r1)) = m.range(&bbox, mpu) else {
            return Err(GisError::Param(
                "the view lies outside the area that the tile matrix set covers".into(),
            ));
        };
        let count = (c1 - c0 + 1) * (r1 - r0 + 1);
        if count > view.max_tiles as u64 {
            continue;
        }
        let mut plans = Vec::new();
        for row in r0..=r1 {
            for col in c0..=c1 {
                plans.push(TilePlan {
                    bbox: m.tile_bbox(col, row, mpu),
                    width: m.tile_width,
                    height: m.tile_height,
                    url: tile_url(source, layer, caps, set, m, &format, &style, col, row)?,
                });
            }
        }
        return Ok((crs, plans));
    }
    Err(GisError::Limit("the view needs too many tiles".into()))
}

/// Fetches the map for a view and returns it as an underlay in the model frame.
pub fn fetch_map(
    http: &dyn Fetch,
    cache: Option<&TileCache>,
    source: &WmtsSource,
    caps: &WmtsCaps,
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
        service: "wmts",
    };
    finish(
        &mosaic,
        &crs,
        doc_crs,
        georef,
        view,
        &source.layer,
        &source.label(),
        stats,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caps::fixtures::WMTS;
    use crate::testing::{Fake, png, text};
    use cadektor_core::DVec2;

    fn caps() -> WmtsCaps {
        crate::caps::parse_wmts(WMTS).unwrap()
    }

    fn doc() -> (Crs, Georeference) {
        (
            Crs::parse("EPSG:32635").unwrap(),
            Georeference {
                crs: "EPSG:32635".into(),
                origin: DVec2::new(385_000.0, 6_672_000.0),
                rotation: 0.0,
                user_origin: DVec2::ZERO,
            },
        )
    }

    #[test]
    fn fills_templates() {
        let v = [("TileCol", "3".to_string()), ("TileRow", "4".to_string())];
        assert_eq!(fill_template("http://t/{tilecol}/{TileRow}.png", &v).unwrap(), "http://t/3/4.png");
        assert_eq!(fill_template("plain", &v).unwrap(), "plain");
        assert!(fill_template("{Time}", &v).unwrap_err().to_string().contains("{Time}"));
        assert!(fill_template("{open", &v).is_err());
    }

    #[test]
    fn chooses_a_matrix_set() {
        let c = caps();
        let l = c.layer("base").unwrap();
        let (utm, _) = doc();
        let s = WmtsSource::new("http://tiles.test/wmts", "base");
        let (set, crs) = choose_set(&s, l, &c, &utm).unwrap();
        assert_eq!((set.id.as_str(), crs.epsg()), ("WebMercator", Some(3857)));
        let mut s2 = s.clone();
        s2.matrix_set = Some("WebMercator".into());
        assert!(choose_set(&s2, l, &c, &utm).is_ok());
        s2.matrix_set = Some("Nope".into());
        assert!(choose_set(&s2, l, &c, &utm).unwrap_err().to_string().contains("no tile matrix set Nope"));
        // The set that matches the document CRS wins.
        let mut c3 = c.clone();
        c3.matrix_sets[0].crs = "urn:ogc:def:crs:EPSG::32635".into();
        let (set, crs) = choose_set(&s, c3.layer("base").unwrap(), &c3, &utm).unwrap();
        assert_eq!((set.id.as_str(), crs.epsg()), ("WebMercator", Some(32635)));
        // A layer without sets.
        let mut c4 = c.clone();
        c4.layers[0].matrix_sets.clear();
        assert!(choose_set(&s, &c4.layers[0], &c4, &utm).is_err());
        // Only an odd CRS: the first set is used when it can be parsed.
        let mut c5 = c.clone();
        c5.layers[0].matrix_sets = vec!["Geo".into()];
        let (set, _) = choose_set(&s, &c5.layers[0], &c5, &utm).unwrap();
        assert_eq!(set.id, "Geo");
        let mut c6 = c.clone();
        c6.matrix_sets[0].crs = "banana".into();
        let mut s6 = s.clone();
        s6.matrix_set = Some("WebMercator".into());
        assert!(choose_set(&s6, &c6.layers[0], &c6, &utm).is_err());
    }

    #[test]
    fn plans_tiles_from_the_template() {
        let c = caps();
        let (crs, g) = doc();
        let s = WmtsSource::new("http://tiles.test/wmts", "base");
        let view = ViewRequest {
            target_px: 512,
            ..ViewRequest::new(DVec2::splat(-5000.0), DVec2::splat(5000.0))
        };
        let (used, plans) = plan_tiles(&s, &c, &crs, &g, &view).unwrap();
        assert_eq!(used.epsg(), Some(3857));
        // Only two levels exist (0 and 1): the finest is used.
        assert!(plans.iter().all(|p| p.url.starts_with("http://tiles.test/WebMercator/1/")));
        assert!(plans.len() <= 4);
        assert!(plans.iter().all(|p| p.width == 256));
        // A layer without a template for this format uses KVP.
        let mut s2 = s.clone();
        s2.format = Some("image/jpeg".into());
        s2.style = Some("normal".into());
        let (_, plans) = plan_tiles(&s2, &c, &crs, &g, &view).unwrap();
        assert!(plans[0].url.starts_with("http://tiles.test/kvp?SERVICE=WMTS&REQUEST=GetTile"));
        assert!(plans[0].url.contains("LAYER=base") && plans[0].url.contains("FORMAT=image%2Fjpeg"));
        assert!(plans[0].url.contains("TILEMATRIXSET=WebMercator") && plans[0].url.contains("STYLE=normal"));
        let mut c2 = c.clone();
        c2.kvp_url = None;
        let (_, plans) = plan_tiles(&s2, &c2, &crs, &g, &view).unwrap();
        assert!(plans[0].url.starts_with("http://tiles.test/wmts?SERVICE=WMTS"));
    }

    #[test]
    fn errors_and_limits() {
        let c = caps();
        let (crs, g) = doc();
        let s = WmtsSource::new("http://tiles.test/wmts", "base");
        let view = ViewRequest {
            target_px: 512,
            ..ViewRequest::new(DVec2::splat(-5000.0), DVec2::splat(5000.0))
        };
        let bad = WmtsSource::new("http://tiles.test/wmts", "nope");
        assert!(plan_tiles(&bad, &c, &crs, &g, &view).unwrap_err().to_string().contains("no layer named nope"));
        assert!(plan_tiles(&s, &c, &crs, &g, &ViewRequest::new(DVec2::ZERO, DVec2::ZERO)).is_err());
        // Too many tiles at every level: the limit error.
        let mut tight = view;
        tight.max_tiles = 0;
        assert!(plan_tiles(&s, &c, &crs, &g, &tight).unwrap_err().to_string().contains("too many"));
        // One tile at most: the coarse level 0 serves.
        let mut one = view;
        one.max_tiles = 1;
        let (_, plans) = plan_tiles(&s, &c, &crs, &g, &one).unwrap();
        assert_eq!(plans.len(), 1);
        // A view that lies off the map: a geographic set with a small area.
        let mut c2 = c.clone();
        c2.layers[0].matrix_sets = vec!["Geo".into()];
        let far = ViewRequest { target_px: 512, ..ViewRequest::new(DVec2::splat(-5000.0), DVec2::splat(5000.0)) };
        // Geo: x -180..180 at level 0 (2 by 1 tiles of 180 degrees). The view is near 24E, 60N and
        // the set only reaches latitudes 90 to -90 in one row: it is inside, so tiles are found.
        let (crs4326, plans) = plan_tiles(&s, &c2, &crs, &g, &far).unwrap();
        assert_eq!(crs4326.epsg(), Some(4326));
        assert_eq!(plans.len(), 1);
        let _ = (c2, far);
    }

    #[test]
    fn fetches_a_map() {
        let c = caps();
        let (crs, g) = doc();
        let s = WmtsSource::new("http://tiles.test/wmts", "base");
        let view = ViewRequest {
            target_px: 128,
            ..ViewRequest::new(DVec2::splat(-2000.0), DVec2::splat(2000.0))
        };
        let (_, plans) = plan_tiles(&s, &c, &crs, &g, &view).unwrap();
        let mut fake = Fake::default();
        for p in &plans {
            fake.answers.insert(p.url.clone(), png(256, 256, [10, 200, 30, 255]));
        }
        let out = fetch_map(&fake, None, &s, &c, &crs, &g, &view).unwrap();
        assert_eq!(out.stats.service, "wmts");
        assert!(out.stats.reprojected);
        let img = out.underlay.decode(4096).unwrap();
        assert!(img.data.chunks(4).all(|p| p == [10, 200, 30, 255]));
        assert!(out.underlay.source.contains("base"));
        // Capabilities.
        fake.answers.insert(capabilities_url("http://tiles.test/wmts"), text(WMTS, "text/xml"));
        assert_eq!(fetch_capabilities(&fake, "http://tiles.test/wmts").unwrap().layers.len(), 1);
        assert!(fetch_capabilities(&fake, "http://tiles.test/x").is_err());
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<WmtsSource>(&j).unwrap(), s);
    }
}
