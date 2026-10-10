//! WFS 2.0 import: GetFeature as GeoJSON or GML 3.2 (simple features).
//!
//! The features go through the importer that reads GeoJSON and Shapefile files (`cadektor_io::gis`).
//! So the CRS conversion, the origin shift of a document without georeference, the layers, classes
//! and the object data (the properties of a feature) are the same as for a file import.
//! Requests name the CRS in the short form `EPSG:n`, which means x, y order on the servers that
//! this was tried with. The long `urn:` form would follow the axis order of the EPSG database
//! (latitude first), which differs between servers.

use cadektor_core::georef::Georeference;
use cadektor_crs::Crs;
use cadektor_io::gis::GisData;
use cadektor_io::{Context, ImportOptions, ImportResult};
use serde::{Deserialize, Serialize};

use crate::caps::WfsCaps;
use crate::http::Fetch;
use crate::mosaic::{encode, with_query};
use crate::warp::Frame;
use crate::{GisError, Result};

/// Features per request when the source says nothing.
pub const DEFAULT_COUNT: u32 = 5000;
/// Features in one page. A larger request is made of several pages (STARTINDEX and COUNT), so a
/// server with a lower limit of its own still gives all the features up to `max_features`.
pub const PAGE: u32 = 1000;
/// Most features per request.
pub const MAX_COUNT: u32 = 50_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WfsSource {
    pub url: String,
    /// The feature type, for example `ns:buildings`.
    pub type_name: String,
    /// `EPSG:...` to request. Empty: the document CRS if the server offers it, else the default of the type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crs: Option<String>,
    /// Only features inside this box (model mm: [min x, min y, max x, max y]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<[f64; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_features: Option<u32>,
    /// `geojson` or `gml`. Empty: GeoJSON if the server offers it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

impl WfsSource {
    pub fn new(url: &str, type_name: &str) -> WfsSource {
        WfsSource {
            url: url.to_string(),
            type_name: type_name.to_string(),
            crs: None,
            bbox: None,
            max_features: None,
            format: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FeatureFormat {
    GeoJson,
    Gml,
}

pub fn capabilities_url(base: &str) -> String {
    with_query(
        base,
        &[
            ("SERVICE", "WFS".into()),
            ("REQUEST", "GetCapabilities".into()),
            ("VERSION", "2.0.0".into()),
        ],
    )
}

pub fn fetch_capabilities(http: &dyn Fetch, base: &str) -> Result<WfsCaps> {
    let r = http.get(&capabilities_url(base))?;
    crate::caps::parse_wfs(&String::from_utf8_lossy(&r.body))
}

/// The CRS to ask for.
pub fn choose_crs(
    source: &WfsSource,
    caps: Option<&WfsCaps>,
    doc_crs: Option<&Crs>,
) -> Result<Crs> {
    if let Some(c) = &source.crs {
        return Crs::parse(c).map_err(GisError::Crs);
    }
    let offered = |c: &Crs| {
        caps.and_then(|k| k.feature_type(&source.type_name))
            .is_some_and(|t| {
                std::iter::once(&t.default_crs)
                    .chain(&t.other_crs)
                    .filter_map(|x| Crs::parse(x).ok())
                    .any(|x| x.epsg().is_some() && x.epsg() == c.epsg())
            })
    };
    if let Some(d) = doc_crs
        && offered(d)
    {
        return Crs::parse(&d.spec).map_err(GisError::Crs);
    }
    if let Some(t) = caps.and_then(|k| k.feature_type(&source.type_name))
        && let Ok(c) = Crs::parse(&t.default_crs)
    {
        return Ok(c);
    }
    // Without capabilities: trust the document CRS, else ask for longitude and latitude.
    match doc_crs {
        Some(d) if caps.is_none() => Crs::parse(&d.spec).map_err(GisError::Crs),
        _ => Crs::from_epsg(4326).map_err(GisError::Crs),
    }
}

/// The most features that a request may bring.
pub fn cap_of(source: &WfsSource) -> u32 {
    source
        .max_features
        .unwrap_or(DEFAULT_COUNT)
        .clamp(1, MAX_COUNT)
}

/// The GetFeature address of the first page.
pub fn getfeature_url(
    base: &str,
    source: &WfsSource,
    crs: &Crs,
    json_format: Option<&str>,
    bbox: Option<&crate::tiles::Bbox>,
) -> String {
    page_url(
        base,
        source,
        crs,
        json_format,
        bbox,
        0,
        cap_of(source).min(PAGE),
    )
}

/// The GetFeature address of a page: `count` features from index `start`.
pub fn page_url(
    base: &str,
    source: &WfsSource,
    crs: &Crs,
    json_format: Option<&str>,
    bbox: Option<&crate::tiles::Bbox>,
    start: u32,
    count: u32,
) -> String {
    let mut p: Vec<(&str, String)> = vec![
        ("SERVICE", "WFS".into()),
        ("VERSION", "2.0.0".into()),
        ("REQUEST", "GetFeature".into()),
        ("TYPENAMES", encode(&source.type_name)),
        ("SRSNAME", encode(&crs.spec)),
        ("COUNT", count.to_string()),
    ];
    if let Some(b) = bbox {
        p.push((
            "BBOX",
            format!(
                "{},{},{},{},{}",
                crate::tiles::num(b.min.x),
                crate::tiles::num(b.min.y),
                crate::tiles::num(b.max.x),
                crate::tiles::num(b.max.y),
                encode(&crs.spec)
            ),
        ));
    }
    if start > 0 {
        p.push(("STARTINDEX", start.to_string()));
    }
    p.push((
        "OUTPUTFORMAT",
        encode(json_format.unwrap_or("application/gml+xml; version=3.2")),
    ));
    with_query(base, &p)
}

/// What a GetFeature request returned.
#[derive(Clone, Debug)]
pub struct Fetched {
    /// The answers of the pages, in order.
    pub pages: Vec<Vec<u8>>,
    /// True when the cap was reached with a full page: the server may have more features.
    pub truncated: bool,
    pub format: FeatureFormat,
    /// The CRS that was asked for.
    pub crs: String,
    pub type_name: String,
}

/// Asks the server for the features. `doc` is the CRS and georeference of the document, if it has them;
/// they are used to choose the CRS and to turn a model box into a box of the CRS.
pub fn fetch_features(
    http: &dyn Fetch,
    source: &WfsSource,
    caps: Option<&WfsCaps>,
    doc: Option<(&Crs, &Georeference)>,
) -> Result<Fetched> {
    if source.type_name.trim().is_empty() {
        return Err(GisError::Param("name the feature type (type_name)".into()));
    }
    if let Some(k) = caps
        && k.feature_type(&source.type_name).is_none()
    {
        return Err(GisError::Param(format!(
            "the server has no feature type {}. It has: {}",
            source.type_name,
            k.feature_types
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let crs = choose_crs(source, caps, doc.map(|d| d.0))?;
    let bbox = match (source.bbox, doc) {
        (Some(b), Some((dc, g))) => {
            let frame = Frame::new(dc, g)?;
            Some(frame.view_bbox(
                cadektor_core::DVec2::new(b[0], b[1]),
                cadektor_core::DVec2::new(b[2], b[3]),
                &crs,
            )?)
        }
        (Some(_), None) => {
            return Err(GisError::Param(
                "a box needs a document with a CRS to place it".into(),
            ));
        }
        (None, _) => None,
    };
    let want_json = match source
        .format
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("geojson" | "json") => true,
        Some("gml") => false,
        Some(other) => {
            return Err(GisError::Param(format!(
                "format must be geojson or gml, not {other}"
            )));
        }
        None => caps.is_none_or(|k| k.json_format().is_some()),
    };
    let json_format = want_json.then(|| {
        caps.and_then(|k| k.json_format())
            .unwrap_or("application/json")
    });
    let base = caps
        .and_then(|k| k.getfeature_url.clone())
        .filter(|u| crate::http::parse_url(u).is_ok())
        .unwrap_or_else(|| source.url.clone());
    let cap = cap_of(source) as usize;
    let mut pages: Vec<Vec<u8>> = Vec::new();
    let (mut got, mut truncated, mut format) = (0usize, false, FeatureFormat::Gml);
    while got < cap {
        let want = (cap - got).min(PAGE as usize);
        let url = page_url(
            &base,
            source,
            &crs,
            json_format,
            bbox.as_ref(),
            got as u32,
            want as u32,
        );
        let body = http.get(&url)?.body;
        let first = body.iter().find(|b| !b.is_ascii_whitespace()).copied();
        format = if first == Some(b'{') {
            FeatureFormat::GeoJson
        } else {
            FeatureFormat::Gml
        };
        // A server that ignores STARTINDEX sends the same page again: stop there.
        if pages.last() == Some(&body) {
            break;
        }
        let data = parse_page(&body, format)?;
        let n = data.features.len() + data.skipped.len();
        pages.push(body);
        got += n;
        if n < want {
            break;
        }
        truncated = got >= cap;
    }
    Ok(Fetched {
        pages,
        truncated,
        format,
        crs: crs.spec,
        type_name: source.type_name.clone(),
    })
}

fn parse_page(body: &[u8], format: FeatureFormat) -> Result<GisData> {
    match format {
        FeatureFormat::GeoJson => {
            let text = std::str::from_utf8(body)
                .map_err(|_| GisError::Format("the answer is not UTF-8 text".into()))?;
            let v: serde_json::Value = serde_json::from_str(text)
                .map_err(|e| GisError::Format(format!("the answer is not JSON: {e}")))?;
            cadektor_io::geojson::read(&v).map_err(|e| GisError::Format(e.to_string()))
        }
        FeatureFormat::Gml => crate::gml::read_gml(&String::from_utf8_lossy(body)),
    }
}

/// Reads the answers of all pages into features.
pub fn parse_features(f: &Fetched) -> Result<GisData> {
    let mut all = GisData::default();
    for page in &f.pages {
        let d = parse_page(page, f.format)?;
        all.features.extend(d.features);
        all.skipped.extend(d.skipped);
        all.notes.extend(d.notes);
        if all.file_crs.is_none() {
            all.file_crs = d.file_crs;
        }
    }
    Ok(all)
}

/// Turns the answer into an import (one batch of commands and a report).
pub fn import(fetched: &Fetched, ctx: &Context, opts: &ImportOptions) -> Result<ImportResult> {
    let data = parse_features(fetched)?;
    let name = fetched
        .type_name
        .rsplit(':')
        .next()
        .unwrap_or(&fetched.type_name)
        .to_string();
    let mut result = cadektor_io::gis::build("wfs", &name, data, ctx, opts, Some(&fetched.crs))
        .map_err(|e| GisError::Format(e.to_string()))?;
    if fetched.truncated {
        result.report.note(&format!(
            "the import stopped at the limit of {} features. The server may have more: raise max_features or use a smaller box",
            count_of(fetched)
        ));
    }
    Ok(result)
}

/// The number of features in the pages (for messages).
fn count_of(f: &Fetched) -> usize {
    f.pages
        .iter()
        .filter_map(|p| parse_page(p, f.format).ok())
        .map(|d| d.features.len() + d.skipped.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caps::fixtures::WFS;
    use crate::gml::fixtures::BUILDINGS_3067;
    use crate::testing::{Fake, text};
    use cadektor_core::DVec2;
    use cadektor_core::command::Editor;
    use cadektor_core::model::Document;

    fn caps() -> WfsCaps {
        crate::caps::parse_wfs(WFS).unwrap()
    }

    const GEOJSON: &str = r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{"name":"A","height":12.5,"floors":4},"geometry":{"type":"Polygon","coordinates":[[[385010,6672010],[385030,6672010],[385030,6672020],[385010,6672020],[385010,6672010]]]}}]}"#;

    #[test]
    fn builds_the_request() {
        let s = WfsSource {
            max_features: Some(10),
            ..WfsSource::new("http://features.test/wfs", "ns:buildings")
        };
        let crs = Crs::parse("EPSG:3067").unwrap();
        let b = crate::tiles::Bbox::new(DVec2::new(1.0, 2.0), DVec2::new(3.5, 4.0));
        let u = getfeature_url(
            "http://features.test/wfs?",
            &s,
            &crs,
            Some("application/json"),
            Some(&b),
        );
        assert_eq!(
            u,
            "http://features.test/wfs?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES=ns%3Abuildings&SRSNAME=EPSG%3A3067&COUNT=10&BBOX=1,2,3.5,4,EPSG%3A3067&OUTPUTFORMAT=application%2Fjson"
        );
        let u = getfeature_url("http://x/wfs", &WfsSource::new("x", "t"), &crs, None, None);
        assert!(
            u.contains("COUNT=1000")
                && u.ends_with("OUTPUTFORMAT=application%2Fgml%2Bxml%3B%20version%3D3.2")
        );
        let big = WfsSource {
            max_features: Some(10_000_000),
            ..WfsSource::new("x", "t")
        };
        assert!(getfeature_url("http://x", &big, &crs, None, None).contains("COUNT=1000"));
        assert_eq!(cap_of(&big), 50_000);
        let page = page_url("http://x", &big, &crs, None, None, 2000, 500);
        assert!(
            page.contains("COUNT=500&STARTINDEX=2000&OUTPUTFORMAT="),
            "{page}"
        );
        assert!(capabilities_url("http://x/wfs").contains("REQUEST=GetCapabilities&VERSION=2.0.0"));
    }

    #[test]
    fn chooses_the_crs() {
        let c = caps();
        let doc = Crs::parse("EPSG:3067").unwrap();
        let s = WfsSource::new("http://features.test/wfs", "ns:buildings");
        assert_eq!(
            choose_crs(&s, Some(&c), Some(&doc)).unwrap().epsg(),
            Some(3067)
        );
        let utm = Crs::parse("EPSG:32635").unwrap();
        // The server does not offer the document CRS: the default of the type.
        assert_eq!(
            choose_crs(&s, Some(&c), Some(&utm)).unwrap().epsg(),
            Some(3067)
        );
        // A type without CRS: longitude and latitude.
        let trees = WfsSource::new("http://x", "ns:trees");
        assert_eq!(
            choose_crs(&trees, Some(&c), Some(&utm)).unwrap().epsg(),
            Some(4326)
        );
        // No capabilities: trust the document.
        assert_eq!(
            choose_crs(&trees, None, Some(&utm)).unwrap().epsg(),
            Some(32635)
        );
        assert_eq!(choose_crs(&trees, None, None).unwrap().epsg(), Some(4326));
        let named = WfsSource {
            crs: Some("EPSG:25832".into()),
            ..s.clone()
        };
        assert_eq!(
            choose_crs(&named, Some(&c), None).unwrap().epsg(),
            Some(25832)
        );
        let bad = WfsSource {
            crs: Some("x".into()),
            ..s
        };
        assert!(choose_crs(&bad, None, None).is_err());
    }

    #[test]
    fn fetches_geojson_and_imports_it_with_properties() {
        let c = caps();
        let crs = Crs::parse("EPSG:3067").unwrap();
        let g = Georeference {
            crs: "EPSG:3067".into(),
            origin: DVec2::new(385_000.0, 6_672_000.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        };
        let s = WfsSource::new("http://features.test/wfs", "ns:buildings");
        let mut fake = Fake::default();
        let url = getfeature_url(
            "http://features.test/wfs?",
            &s,
            &crs,
            Some("application/json"),
            None,
        );
        fake.answers.insert(url, text(GEOJSON, "application/json"));
        let f = fetch_features(&fake, &s, Some(&c), Some((&crs, &g))).unwrap();
        assert_eq!(f.format, FeatureFormat::GeoJson);
        assert_eq!(f.crs, "EPSG:3067");
        // Import into a document with the same georeference.
        let mut doc = Document::default();
        doc.georef = Some(g);
        let ctx = Context::of(&doc);
        let r = import(&f, &ctx, &ImportOptions::default()).unwrap();
        let mut ed = Editor::new(doc);
        ed.execute(&r.command).unwrap();
        let o = ed
            .doc()
            .objects
            .values()
            .find(|o| o.data.contains_key("height"))
            .unwrap();
        assert_eq!(o.data["height"], serde_json::json!(12.5));
        assert_eq!(o.data["floors"], serde_json::json!(4));
        match &o.shape {
            cadektor_core::model::Shape::Polyline {
                closed: true,
                points,
            } => {
                assert!(points[0].p.distance(DVec2::new(10_000.0, 10_000.0)) < 1e-3);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn fetches_gml_and_imports_into_an_empty_document() {
        let s = WfsSource {
            format: Some("gml".into()),
            ..WfsSource::new("http://features.test/wfs", "ns:buildings")
        };
        let mut fake = Fake::default();
        let crs = Crs::parse("EPSG:3067").unwrap();
        fake.answers.insert(
            getfeature_url("http://features.test/wfs", &s, &crs, None, None),
            text(BUILDINGS_3067, "application/gml+xml"),
        );
        let f = fetch_features(&fake, &s, None, None);
        // Without capabilities and without a document the request asks for EPSG:4326, so this is not found.
        assert!(f.is_err());
        let s2 = WfsSource {
            crs: Some("EPSG:3067".into()),
            ..s
        };
        let f = fetch_features(&fake, &s2, None, None).unwrap();
        assert_eq!(f.format, FeatureFormat::Gml);
        let doc = Document::default();
        let r = import(&f, &Context::of(&doc), &ImportOptions::default()).unwrap();
        let mut ed = Editor::new(doc);
        ed.execute(&r.command).unwrap();
        assert!(ed.doc().georef.is_some());
        assert_eq!(ed.doc().georef.as_ref().unwrap().crs, "EPSG:3067");
        assert!(
            ed.doc()
                .objects
                .values()
                .any(|o| o.data.get("name") == Some(&serde_json::json!("House A")))
        );
    }

    #[test]
    fn rejects_bad_requests_and_answers() {
        let c = caps();
        let fake = Fake::default();
        let s = WfsSource::new("http://features.test/wfs", "ns:nothing");
        assert!(
            fetch_features(&fake, &s, Some(&c), None)
                .unwrap_err()
                .to_string()
                .contains("no feature type ns:nothing")
        );
        assert!(fetch_features(&fake, &WfsSource::new("x", " "), None, None).is_err());
        let bad_format = WfsSource {
            format: Some("csv".into()),
            ..WfsSource::new("http://x", "t")
        };
        assert!(
            fetch_features(&fake, &bad_format, None, None)
                .unwrap_err()
                .to_string()
                .contains("geojson or gml")
        );
        let with_box = WfsSource {
            bbox: Some([0.0, 0.0, 1.0, 1.0]),
            ..WfsSource::new("http://x", "t")
        };
        assert!(
            fetch_features(&fake, &with_box, None, None)
                .unwrap_err()
                .to_string()
                .contains("document")
        );
        // Junk answers.
        let junk = Fetched {
            pages: vec![b"{broken".to_vec()],
            truncated: false,
            format: FeatureFormat::GeoJson,
            crs: "EPSG:4326".into(),
            type_name: "t".into(),
        };
        assert!(parse_features(&junk).is_err());
        let bin = Fetched {
            pages: vec![vec![0xff, 0xfe]],
            truncated: false,
            format: FeatureFormat::GeoJson,
            crs: "EPSG:4326".into(),
            type_name: "t".into(),
        };
        assert!(parse_features(&bin).is_err());
        let json = serde_json::to_string(&WfsSource::new("http://x", "t")).unwrap();
        assert!(!json.contains("bbox"));
    }

    #[test]
    fn a_box_is_turned_into_the_request_crs() {
        let c = caps();
        let crs = Crs::parse("EPSG:3067").unwrap();
        let g = Georeference {
            crs: "EPSG:3067".into(),
            origin: DVec2::new(385_000.0, 6_672_000.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        };
        let s = WfsSource {
            bbox: Some([0.0, 0.0, 100_000.0, 50_000.0]),
            format: Some("geojson".into()),
            ..WfsSource::new("http://features.test/wfs", "ns:buildings")
        };
        let mut fake = Fake::default();
        let b = crate::tiles::Bbox::new(
            DVec2::new(385_000.0, 6_672_000.0),
            DVec2::new(385_100.0, 6_672_050.0),
        );
        fake.answers.insert(
            getfeature_url(
                "http://features.test/wfs?",
                &s,
                &crs,
                Some("application/json"),
                Some(&b),
            ),
            text(GEOJSON, "application/json"),
        );
        assert!(fetch_features(&fake, &s, Some(&c), Some((&crs, &g))).is_ok());
    }

    /// A page of `n` point features.
    fn points(n: usize, offset: usize) -> String {
        let features: Vec<String> = (offset..offset + n)
            .map(|i| format!(r#"{{"type":"Feature","properties":{{"i":{i}}},"geometry":{{"type":"Point","coordinates":[385000,6672000]}}}}"#))
            .collect();
        format!(
            r#"{{"type":"FeatureCollection","features":[{}]}}"#,
            features.join(",")
        )
    }

    #[test]
    fn big_requests_come_in_pages_up_to_the_cap() {
        let crs = Crs::parse("EPSG:3067").unwrap();
        let mut s = WfsSource::new("http://f.test/wfs", "ns:b");
        s.crs = Some("EPSG:3067".into());
        s.format = Some("geojson".into());
        let url = |s: &WfsSource, start, count| {
            page_url(
                "http://f.test/wfs",
                s,
                &crs,
                Some("application/json"),
                None,
                start,
                count,
            )
        };
        // 2500 asked, the server has plenty: three pages, and the cap is reached.
        s.max_features = Some(2500);
        let mut fake = Fake::default();
        for (start, count) in [(0, 1000), (1000, 1000), (2000, 500)] {
            fake.answers.insert(
                url(&s, start, count),
                text(&points(count as usize, start as usize), "application/json"),
            );
        }
        let f = fetch_features(&fake, &s, None, None).unwrap();
        assert_eq!(f.pages.len(), 3);
        assert!(f.truncated, "the cap was reached with a full page");
        assert_eq!(parse_features(&f).unwrap().features.len(), 2500);
        let doc = Document::default();
        let r = import(&f, &Context::of(&doc), &ImportOptions::default()).unwrap();
        assert!(
            r.report
                .notes
                .iter()
                .any(|n| n.contains("stopped at the limit of 2500"))
        );
        // The server has 1200: a short second page ends it without a warning.
        let mut fake = Fake::default();
        fake.answers
            .insert(url(&s, 0, 1000), text(&points(1000, 0), "application/json"));
        fake.answers.insert(
            url(&s, 1000, 1000),
            text(&points(200, 1000), "application/json"),
        );
        let f = fetch_features(&fake, &s, None, None).unwrap();
        assert_eq!((f.pages.len(), f.truncated), (2, false));
        assert_eq!(parse_features(&f).unwrap().features.len(), 1200);
        // A server that ignores STARTINDEX repeats the first page: the repeat is dropped.
        let mut fake = Fake::default();
        let first = text(&points(1000, 0), "application/json");
        fake.answers.insert(url(&s, 0, 1000), first.clone());
        fake.answers.insert(url(&s, 1000, 1000), first);
        let f = fetch_features(&fake, &s, None, None).unwrap();
        assert_eq!(f.pages.len(), 1);
        assert_eq!(parse_features(&f).unwrap().features.len(), 1000);
        // A small request is one page.
        s.max_features = Some(10);
        let mut fake = Fake::default();
        fake.answers
            .insert(url(&s, 0, 10), text(&points(10, 0), "application/json"));
        let f = fetch_features(&fake, &s, None, None).unwrap();
        assert_eq!(
            (f.pages.len(), f.truncated),
            (1, true),
            "exactly the cap: there may be more"
        );
    }
}
