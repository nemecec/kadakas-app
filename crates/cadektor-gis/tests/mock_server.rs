//! Tests against a local HTTP server. No test uses the real network.

mod common;

use std::time::{Duration, Instant};

use cadektor_core::DVec2;
use cadektor_core::georef::Georeference;
use cadektor_gis::cache::TileCache;
use cadektor_gis::http::{Fetch, HttpClient, UrlPolicy};
use cadektor_gis::job::Job;
use cadektor_gis::mosaic::ViewRequest;
use cadektor_gis::wfs::{self, WfsSource};
use cadektor_gis::wms::{self, WmsSource};
use cadektor_gis::wmts::{self, WmtsSource};
use cadektor_io::crs::Crs;
use common::{Mock, Reply, png};

fn client() -> HttpClient {
    HttpClient::new(UrlPolicy::unrestricted(), Duration::from_secs(10))
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

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("cadektor-gis-it-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn client_gets_answers_and_reports_errors() {
    let m = Mock::start(|p| match p.split('?').next().unwrap() {
        "/ok" => Reply::Ok("Text/Plain; charset=utf-8", b"hello".to_vec()),
        "/missing" => Reply::Status(404, "no such thing"),
        "/broken" => Reply::Status(500, "boom\nline"),
        _ => Reply::Status(400, "?"),
    });
    let c = client();
    let r = c.get(&format!("{}/ok", m.base)).unwrap();
    assert_eq!((r.status, r.content_type.as_str(), r.body.as_slice()), (200, "text/plain", &b"hello"[..]));
    let e = c.get(&format!("{}/missing?key=SECRET", m.base)).unwrap_err().to_string();
    assert!(e.contains("HTTP 404") && e.contains("no such thing"), "{e}");
    assert!(!e.contains("SECRET"), "a key in the query must not show: {e}");
    let e = c.get(&format!("{}/broken", m.base)).unwrap_err().to_string();
    assert!(e.contains("HTTP 500") && e.contains("boomline"), "{e}");
    assert_eq!(c.policy(), &UrlPolicy::unrestricted());
}

#[test]
fn redirects_are_followed_and_checked_again() {
    let other = Mock::start(|_| Reply::Ok("text/plain", b"secret place".to_vec()));
    let other_url = format!("{}/x", other.base);
    let m = Mock::start(move |p| match p {
        "/hop" => Reply::Redirect("/final".into()),
        "/final" => Reply::Ok("text/plain", b"done".to_vec()),
        "/away" => Reply::Redirect(other_url.clone()),
        "/loop" => Reply::Redirect("/loop".into()),
        "/nowhere" => Reply::Status(302, ""),
        _ => Reply::Status(404, "-"),
    });
    let c = client();
    assert_eq!(c.get(&format!("{}/hop", m.base)).unwrap().body, b"done");
    let e = c.get(&format!("{}/loop", m.base)).unwrap_err().to_string();
    assert!(e.contains("too many redirects"), "{e}");
    let e = c.get(&format!("{}/nowhere", m.base)).unwrap_err().to_string();
    assert!(e.contains("without a location"), "{e}");
    // Only the first server is on the list: the redirect to the second one is refused.
    let strict = HttpClient::new(UrlPolicy::allow_list([m.base.clone()]), Duration::from_secs(10));
    let e = strict.get(&format!("{}/away", m.base)).unwrap_err().to_string();
    assert!(e.contains("not on the list"), "{e}");
    assert_eq!(other.hit_count(), 0, "the second server was never asked");
    assert_eq!(strict.get(&format!("{}/hop", m.base)).unwrap().body, b"done");
}

#[test]
fn the_policy_blocks_before_any_request() {
    let m = Mock::start(|_| Reply::Ok("text/plain", b"x".to_vec()));
    let c = HttpClient::new(UrlPolicy::deny_all(), Duration::from_secs(2));
    assert!(c.get(&format!("{}/a", m.base)).is_err());
    assert_eq!(m.hit_count(), 0);
    let c = HttpClient::new(UrlPolicy::allow_list(["127.0.0.1".to_string()]), Duration::from_secs(2));
    assert!(c.get(&format!("{}/a", m.base)).is_ok());
    assert!(c.get("ftp://127.0.0.1/a").is_err());
    assert!(c.get("file:///etc/passwd").is_err());
}

#[test]
fn slow_servers_and_big_answers_are_stopped() {
    let m = Mock::start(|p| match p {
        "/slow" => Reply::Slow(Duration::from_secs(3)),
        "/hang" => Reply::Hang,
        _ => Reply::Ok("application/octet-stream", vec![7; 5000]),
    });
    let c = HttpClient::new(UrlPolicy::unrestricted(), Duration::from_millis(400));
    let t = Instant::now();
    assert!(c.get(&format!("{}/slow", m.base)).is_err());
    assert!(c.get(&format!("{}/hang", m.base)).is_err());
    assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    let small = HttpClient::new(UrlPolicy::unrestricted(), Duration::from_secs(5)).with_max_body(1000);
    let e = small.get(&format!("{}/big", m.base)).unwrap_err().to_string();
    assert!(e.contains("cannot read the answer"), "{e}");
    // A server that is not there.
    let dead = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    assert!(client().get(&format!("http://127.0.0.1:{dead}/")).is_err());
}

fn wms_caps_xml(base: &str) -> String {
    format!(
        r#"<?xml version="1.0"?>
<WMS_Capabilities version="1.3.0" xmlns="http://www.opengis.net/wms" xmlns:xlink="http://www.w3.org/1999/xlink">
 <Service><Name>WMS</Name><Title>Mock</Title></Service>
 <Capability><Request><GetMap><Format>image/png</Format>
  <DCPType><HTTP><Get><OnlineResource xlink:href="{base}/wms?"/></Get></HTTP></DCPType></GetMap></Request>
  <Layer><Title>root</Title><CRS>EPSG:3857</CRS><CRS>EPSG:3067</CRS><Layer><Name>ortho</Name><Title>Ortho</Title></Layer><Layer><Name>web</Name><Title>Web only</Title><CRS>EPSG:4326</CRS></Layer></Layer>
 </Capability>
</WMS_Capabilities>"#
    )
}

fn wms_server() -> Mock {
    // The capabilities need the address of the server, which is known after the start.
    let base = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let b2 = base.clone();
    let m = Mock::start(move |p| {
        if p.contains("REQUEST=GetCapabilities") {
            Reply::Ok("text/xml", wms_caps_xml(&b2.lock().unwrap()).into_bytes())
        } else if p.contains("REQUEST=GetMap") {
            if p.contains("LAYERS=broken") {
                Reply::Ok(
                    "text/xml",
                    br#"<ServiceExceptionReport><ServiceException code="LayerNotDefined">Layer is broken</ServiceException></ServiceExceptionReport>"#.to_vec(),
                )
            } else {
                // 512 pixels, one color per tile column, so a mix-up of tiles would show.
                Reply::Ok("image/png", png(512, 512, [30, 160, 90, 255]))
            }
        } else {
            Reply::Status(404, "?")
        }
    });
    *base.lock().unwrap() = m.base.clone();
    m
}

#[test]
fn wms_map_with_a_disk_cache() {
    let m = wms_server();
    let c = client();
    let (crs, g) = doc();
    let caps = wms::fetch_capabilities(&c, &format!("{}/wms", m.base)).unwrap();
    assert_eq!(caps.title, "Mock");
    assert_eq!(caps.layers.len(), 3);
    let dir = temp("wms-cache");
    let cache = TileCache::new(&dir, 64 << 20);
    let src = WmsSource::new(&format!("{}/wms", m.base), &["ortho"]);
    let view = ViewRequest {
        target_px: 400,
        ..ViewRequest::new(DVec2::ZERO, DVec2::new(300_000.0, 200_000.0))
    };
    let first = wms::fetch_map(&c, Some(&cache), &src, &caps, &crs, &g, &view).unwrap();
    assert_eq!(first.stats.from_cache, 0);
    assert!(first.stats.tiles >= 1);
    let u = &first.underlay;
    assert_eq!((u.width, u.height), (400, 267));
    assert!(u.decode(4096).unwrap().data.chunks(4).all(|p| p == [30, 160, 90, 255]));
    let hits_after_first = m.hit_count();
    // The same view again comes from the disk.
    let second = wms::fetch_map(&c, Some(&cache), &src, &caps, &crs, &g, &view).unwrap();
    assert_eq!(second.stats.from_cache, second.stats.tiles);
    assert_eq!(m.hit_count(), hits_after_first);
    assert_eq!(second.underlay.png, first.underlay.png);
    // A layer that exists only in Web Mercator is requested in that CRS and warped.
    let web = WmsSource { crs: Some("EPSG:3857".into()), ..WmsSource::new(&format!("{}/wms", m.base), &["ortho"]) };
    let r = wms::fetch_map(&c, None, &web, &caps, &crs, &g, &view).unwrap();
    assert!(r.stats.reprojected && r.stats.crs == "EPSG:3857");
    assert!(m.hits.lock().unwrap().iter().any(|h| h.contains("CRS=EPSG%3A3857")));
    // The server reports an error inside a 200 answer.
    let mut caps2 = caps.clone();
    caps2.layers.push(caps.layers[1].clone());
    caps2.layers.last_mut().unwrap().name = Some("broken".into());
    let bad = WmsSource::new(&format!("{}/wms", m.base), &["broken"]);
    let e = wms::fetch_map(&c, None, &bad, &caps2, &crs, &g, &view).unwrap_err().to_string();
    assert!(e.contains("Layer is broken"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wms_cache_respects_its_size_limit() {
    let m = wms_server();
    let c = client();
    let (crs, g) = doc();
    let caps = wms::fetch_capabilities(&c, &format!("{}/wms", m.base)).unwrap();
    let dir = temp("wms-limit");
    // About two tiles fit.
    let one_tile = png(512, 512, [30, 160, 90, 255]).len() as u64;
    let cache = TileCache::new(&dir, one_tile * 2 + 10);
    let src = WmsSource::new(&format!("{}/wms", m.base), &["ortho"]);
    let view = ViewRequest {
        target_px: 1500,
        ..ViewRequest::new(DVec2::ZERO, DVec2::new(1_000_000.0, 1_000_000.0))
    };
    let r = wms::fetch_map(&c, Some(&cache), &src, &caps, &crs, &g, &view).unwrap();
    assert!(r.stats.tiles > 2);
    let (files, bytes) = cache.stats();
    assert!(files <= 2 && bytes <= cache.limit(), "{files} {bytes}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wmts_map_from_a_template() {
    let m = Mock::start(|p| {
        if p.contains("REQUEST=GetCapabilities") {
            Reply::Status(404, "caps come from the test")
        } else if p.starts_with("/tiles/") {
            Reply::Ok("image/png", png(256, 256, [200, 50, 50, 255]))
        } else {
            Reply::Status(404, "-")
        }
    });
    let xml = format!(
        r#"<Capabilities xmlns="http://www.opengis.net/wmts/1.0" xmlns:ows="http://www.opengis.net/ows/1.1" version="1.0.0">
 <Contents><Layer><ows:Identifier>base</ows:Identifier><Format>image/png</Format>
  <TileMatrixSetLink><TileMatrixSet>WM</TileMatrixSet></TileMatrixSetLink>
  <ResourceURL format="image/png" resourceType="tile" template="{}/tiles/{{TileMatrix}}/{{TileCol}}/{{TileRow}}.png"/></Layer>
  <TileMatrixSet><ows:Identifier>WM</ows:Identifier><ows:SupportedCRS>urn:ogc:def:crs:EPSG::3857</ows:SupportedCRS>
   {}
  </TileMatrixSet></Contents></Capabilities>"#,
        m.base,
        (0..8)
            .map(|z| format!(
                "<TileMatrix><ows:Identifier>{z}</ows:Identifier><ScaleDenominator>{}</ScaleDenominator><TopLeftCorner>-20037508.342789244 20037508.342789244</TopLeftCorner><TileWidth>256</TileWidth><TileHeight>256</TileHeight><MatrixWidth>{}</MatrixWidth><MatrixHeight>{}</MatrixHeight></TileMatrix>",
                559082264.0287178 / f64::from(1u32 << z),
                1u32 << z,
                1u32 << z
            ))
            .collect::<String>()
    );
    let caps = cadektor_gis::caps::parse_wmts(&xml).unwrap();
    let (crs, g) = doc();
    let src = WmtsSource::new(&format!("{}/wmts", m.base), "base");
    let view = ViewRequest {
        target_px: 300,
        ..ViewRequest::new(DVec2::splat(-50_000.0), DVec2::splat(50_000.0))
    };
    let r = wmts::fetch_map(&client(), None, &src, &caps, &crs, &g, &view).unwrap();
    assert_eq!(r.stats.service, "wmts");
    assert!(r.stats.reprojected);
    let img = r.underlay.decode(4096).unwrap();
    assert!(img.data.chunks(4).all(|p| p == [200, 50, 50, 255]));
    assert!(m.hits.lock().unwrap().iter().all(|h| h.starts_with("/tiles/")));
}

#[test]
fn wfs_json_and_gml() {
    let m = Mock::start(|p| {
        if p.contains("OUTPUTFORMAT=application%2Fjson") {
            Reply::Ok(
                "application/json",
                br#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"name":"A","floors":2},"geometry":{"type":"Polygon","coordinates":[[[385010,6672010],[385030,6672010],[385030,6672020],[385010,6672020],[385010,6672010]]]}}]}"#.to_vec(),
            )
        } else if p.contains("OUTPUTFORMAT=") {
            Reply::Ok(
                "application/gml+xml",
                br#"<wfs:FeatureCollection xmlns:wfs="http://www.opengis.net/wfs/2.0" xmlns:gml="http://www.opengis.net/gml/3.2" xmlns:ns="x"><wfs:member><ns:b gml:id="b"><ns:h>7.5</ns:h><ns:g><gml:Polygon srsName="EPSG:3067"><gml:exterior><gml:LinearRing><gml:posList>385010 6672010 385030 6672010 385030 6672020 385010 6672010</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></ns:g></ns:b></wfs:member></wfs:FeatureCollection>"#.to_vec(),
            )
        } else {
            Reply::Status(404, "-")
        }
    });
    let (crs, g) = doc();
    let c = client();
    let mut s = WfsSource::new(&format!("{}/wfs", m.base), "ns:b");
    s.crs = Some("EPSG:3067".into());
    let f = wfs::fetch_features(&c, &s, None, Some((&crs, &g))).unwrap();
    assert_eq!(f.format, wfs::FeatureFormat::GeoJson);
    assert_eq!(wfs::parse_features(&f).unwrap().features.len(), 1);
    s.format = Some("gml".into());
    let f = wfs::fetch_features(&c, &s, None, Some((&crs, &g))).unwrap();
    assert_eq!(f.format, wfs::FeatureFormat::Gml);
    let d = wfs::parse_features(&f).unwrap();
    assert_eq!(d.features[0].props["h"], serde_json::json!(7.5));
    assert!(m.hits.lock().unwrap().iter().all(|h| h.contains("TYPENAMES=ns%3Ab") && h.contains("SRSNAME=EPSG%3A3067")));
}

#[test]
fn jobs_keep_the_caller_free() {
    let m = Mock::start(|_| Reply::Slow(Duration::from_millis(300)));
    let url = format!("{}/slow", m.base);
    let mut job = Job::spawn("fetch", move || {
        HttpClient::new(UrlPolicy::unrestricted(), Duration::from_secs(5)).get(&url).map(|r| r.body.len())
    });
    let t = Instant::now();
    let mut polls = 0;
    while !job.is_done() {
        polls += 1;
        std::thread::sleep(Duration::from_millis(10));
        assert!(t.elapsed() < Duration::from_secs(5));
    }
    assert!(polls > 3, "the caller was free while the server was slow");
    assert_eq!(job.take().unwrap().unwrap(), 4);
}
