//! GIS and massing methods through the JSON-RPC interface, against a local mock server.

mod common;

use cadektor_core::uid::AsUid;
use std::sync::{Arc, Mutex};

use cadektor_api::Session;
use cadektor_gis::http::UrlPolicy;
use common::{Mock, Reply, png};
use serde_json::{Value, json};

fn call(s: &mut Session, method: &str, params: Value) -> Result<Value, (i64, String)> {
    s.handle(method, params).map_err(|e| (e.code, e.message))
}

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    call(s, method, params).unwrap_or_else(|e| panic!("{method}: {e:?}"))
}

fn err(s: &mut Session, method: &str, params: Value) -> String {
    call(s, method, params).unwrap_err().1
}

/// A folder of its own for each session, so tests that run at the same time never share a cache.
fn own_cache() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::env::temp_dir().join(format!("cadektor-api-gis-{}-{n}", std::process::id()))
}

fn session_for(m: &Mock) -> Session {
    let mut s = Session::with_sequential_ids();
    s.gis.urls = UrlPolicy::allow_list([m.base.clone()]);
    s.gis.cache_dir = Some(own_cache());
    s
}

fn services() -> Mock {
    let base = Arc::new(Mutex::new(String::new()));
    let b2 = base.clone();
    let m = Mock::start(move |p| {
        let b = b2.lock().unwrap().clone();
        if p.starts_with("/wms") && p.contains("REQUEST=GetCapabilities") {
            Reply::Ok(
                "text/xml",
                format!(
                    r#"<WMS_Capabilities version="1.3.0" xmlns="http://www.opengis.net/wms" xmlns:xlink="http://www.w3.org/1999/xlink"><Service><Title>Mock WMS</Title></Service>
<Capability><Request><GetMap><Format>image/png</Format><DCPType><HTTP><Get><OnlineResource xlink:href="{b}/wms?"/></Get></HTTP></DCPType></GetMap></Request>
<Layer><Title>r</Title><CRS>EPSG:3857</CRS><Layer><Name>ortho</Name><Title>Ortho</Title></Layer></Layer></Capability></WMS_Capabilities>"#
                )
                .into_bytes(),
            )
        } else if p.contains("REQUEST=GetMap") {
            Reply::Ok("image/png", png(512, 512, [40, 120, 200, 255]))
        } else if p.starts_with("/wfs") && p.contains("REQUEST=GetCapabilities") {
            Reply::Ok(
                "text/xml",
                format!(
                    r#"<wfs:WFS_Capabilities version="2.0.0" xmlns:wfs="http://www.opengis.net/wfs/2.0" xmlns:ows="http://www.opengis.net/ows/1.1" xmlns:xlink="http://www.w3.org/1999/xlink">
<ows:OperationsMetadata><ows:Operation name="GetFeature"><ows:DCP><ows:HTTP><ows:Get xlink:href="{b}/wfs?"/></ows:HTTP></ows:DCP>
<ows:Parameter name="outputFormat"><ows:AllowedValues><ows:Value>application/json</ows:Value></ows:AllowedValues></ows:Parameter></ows:Operation></ows:OperationsMetadata>
<wfs:FeatureTypeList><wfs:FeatureType><wfs:Name>ns:buildings</wfs:Name><wfs:DefaultCRS>urn:ogc:def:crs:EPSG::3067</wfs:DefaultCRS></wfs:FeatureType></wfs:FeatureTypeList></wfs:WFS_Capabilities>"#
                )
                .into_bytes(),
            )
        } else if p.contains("REQUEST=GetFeature") {
            let ring = |x: f64, y: f64, w: f64| {
                json!([[x, y], [x + w, y], [x + w, y + w], [x, y + w], [x, y]])
            };
            let fc = json!({"type": "FeatureCollection", "features": [
                {"type": "Feature", "properties": {"name": "Tall", "height": 12.5}, "geometry": {"type": "Polygon", "coordinates": [ring(385010.0, 6672010.0, 20.0)]}},
                {"type": "Feature", "properties": {"name": "Low", "levels": 2}, "geometry": {"type": "Polygon", "coordinates": [ring(385060.0, 6672010.0, 10.0)]}},
                {"type": "Feature", "properties": {"name": "Unknown"}, "geometry": {"type": "Polygon", "coordinates": [ring(385090.0, 6672010.0, 8.0)]}},
            ]});
            Reply::Ok("application/json", fc.to_string().into_bytes())
        } else {
            Reply::Status(404, "-")
        }
    });
    *base.lock().unwrap() = m.base.clone();
    m
}

fn set_crs(s: &mut Session) {
    let r = ok(
        s,
        "gis.georef.set",
        json!({"crs": "EPSG:3067", "origin": [385000, 6672000]}),
    );
    assert_eq!(r["georeference"]["crs"], "EPSG:3067");
}

#[test]
fn the_default_policy_allows_no_address() {
    let mut s = Session::with_sequential_ids();
    let m = services();
    let e = err(
        &mut s,
        "gis.wms.capabilities",
        json!({"url": format!("{}/wms", m.base)}),
    );
    assert!(
        e.contains("not on the list") && e.contains("--allow-url"),
        "{e}"
    );
    assert_eq!(m.hit_count(), 0, "no request left the session");
    let info = ok(&mut s, "gis.sources", json!({}));
    assert_eq!(info["restricted"], true);
    assert_eq!(info["allowed_urls"], json!([]));
    // A bad address is a parameter error.
    assert!(
        err(
            &mut s,
            "gis.wms.capabilities",
            json!({"url": "file:///etc/passwd"})
        )
        .contains("http")
    );
    assert!(
        err(
            &mut s,
            "gis.wfs.import",
            json!({"url": "ftp://x/", "type_name": "t"})
        )
        .contains("http")
    );
}

#[test]
fn capabilities_through_the_api() {
    let m = services();
    let mut s = session_for(&m);
    let c = ok(
        &mut s,
        "gis.wms.capabilities",
        json!({"url": format!("{}/wms", m.base)}),
    );
    assert_eq!(c["title"], "Mock WMS");
    assert!(
        c["layers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["name"] == "ortho")
    );
    let w = ok(
        &mut s,
        "gis.wfs.capabilities",
        json!({"url": format!("{}/wfs", m.base)}),
    );
    assert_eq!(w["feature_types"][0]["name"], "ns:buildings");
    // A server that answers with something else.
    let e = err(
        &mut s,
        "gis.wmts.capabilities",
        json!({"url": format!("{}/wms", m.base)}),
    );
    assert!(e.contains("not a WMTS"), "{e}");
    let e = err(
        &mut s,
        "gis.wms.capabilities",
        json!({"url": format!("{}/other", m.base)}),
    );
    assert!(e.contains("404"), "{e}");
    assert!(err(&mut s, "gis.nonsense", json!({})).contains("unknown method"));
}

#[test]
fn wms_underlay_in_the_document_and_refresh() {
    let m = services();
    let mut s = session_for(&m);
    // No CRS: a clear message.
    let e = err(
        &mut s,
        "gis.wms.add",
        json!({"url": format!("{}/wms", m.base), "layers": ["ortho"], "bbox": [0, 0, 1000, 1000]}),
    );
    assert!(e.contains("gis.georef.set"), "{e}");
    set_crs(&mut s);
    // An empty drawing needs a box.
    let e = err(
        &mut s,
        "gis.wms.add",
        json!({"url": format!("{}/wms", m.base), "layers": ["ortho"]}),
    );
    assert!(e.contains("bbox"), "{e}");
    let r = ok(
        &mut s,
        "gis.wms.add",
        json!({"url": format!("{}/wms", m.base), "layers": ["ortho"], "bbox": [0, 0, 200000, 100000], "max_px": 400, "name": "Aerial", "opacity": 0.8}),
    );
    let id = r["id"].as_uid().unwrap();
    assert_eq!(
        (r["width"].as_u64().unwrap(), r["height"].as_u64().unwrap()),
        (400, 200)
    );
    assert_eq!(r["stats"]["service"], "wms");
    assert_eq!(r["stats"]["crs"], "EPSG:3857");
    let list = ok(&mut s, "objects.list", json!({"kind": "underlay"}));
    assert_eq!(list.as_array().map(Vec::len), Some(1));
    // The picture itself is not in the list.
    assert!(
        list[0]["shape"]["png"]
            .as_str()
            .unwrap()
            .contains("not shown")
    );
    assert!(
        ok(&mut s, "objects.get", json!({"id": id}))["shape"]["png"]
            .as_str()
            .unwrap()
            .contains("not shown")
    );
    let doc = s.editor.doc();
    let o = doc.object(cadektor_core::model::ObjectId(id)).unwrap();
    let cadektor_core::model::Shape::Underlay(u) = &o.shape else {
        panic!()
    };
    assert_eq!((u.name.as_str(), u.opacity), ("Aerial", 0.8));
    assert_eq!(o.data["gis_source"]["kind"], "wms");
    // Undo removes it in one step.
    assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
    assert!(
        s.editor
            .doc()
            .object(cadektor_core::model::ObjectId(id))
            .is_none()
    );
    ok(&mut s, "redo", json!({}));
    // Refresh for a new box keeps the object and its id.
    let r2 = ok(
        &mut s,
        "gis.wms.refresh",
        json!({"id": id, "bbox": [0, 0, 100000, 100000], "max_px": 200}),
    );
    assert_eq!(r2["id"].as_uid().unwrap(), id);
    let o = s
        .editor
        .doc()
        .object(cadektor_core::model::ObjectId(id))
        .unwrap();
    let cadektor_core::model::Shape::Underlay(u) = &o.shape else {
        panic!()
    };
    assert_eq!((u.width, u.height, u.name.as_str()), (200, 200, "Aerial"));
    // The second request for the same tiles comes from the cache.
    let before = m.hit_count();
    let r3 = ok(&mut s, "gis.wms.refresh", json!({"id": id}));
    assert_eq!(r3["stats"]["from_cache"], r3["stats"]["tiles"]);
    let after = m.hit_count();
    assert!(
        after - before <= 1,
        "only the capabilities were asked again: {before} {after}"
    );
    // Errors.
    let c = ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}}}))["created"][0].as_uid().unwrap();
    assert!(err(&mut s, "gis.wms.refresh", json!({"id": c})).contains("not an underlay"));
    assert!(
        err(
            &mut s,
            "gis.wms.refresh",
            json!({"id": "00000000-0000-0000-0000-00000000270f"})
        )
        .contains("does not exist")
    );
    assert!(err(&mut s, "gis.wms.add", json!({"url": format!("{}/wms", m.base), "layers": ["ortho"], "bbox": [0, 0, 10, 10], "opacity": 3})).contains("opacity"));
    assert!(err(&mut s, "gis.wms.add", json!({"url": format!("{}/wms", m.base), "layers": ["nope"], "bbox": [0, 0, 1000, 1000]})).contains("no layer named nope"));
    assert!(err(&mut s, "gis.wms.add", json!({"url": format!("{}/wms", m.base), "layers": ["ortho"], "bbox": [0, 0, 1000, 1000], "layer": "Missing"})).contains("no layer named Missing"));
    // Clearing the cache.
    assert!(
        ok(&mut s, "gis.cache.clear", json!({}))["removed"]
            .as_u64()
            .unwrap()
            >= 1
    );
    let _ = std::fs::remove_dir_all(s.gis.cache_dir.clone().unwrap());
}

#[test]
fn jobs_do_not_wait_when_blocking_is_off() {
    let m = Mock::start(|_| Reply::Slow(std::time::Duration::from_millis(300)));
    let mut s = session_for(&m);
    s.gis.blocking = false;
    let t = std::time::Instant::now();
    let r = ok(
        &mut s,
        "gis.wfs.capabilities",
        json!({"url": format!("{}/wfs", m.base)}),
    );
    assert!(
        t.elapsed() < std::time::Duration::from_millis(250),
        "the call returned at once"
    );
    assert_eq!(r["status"], "running");
    let job = r["job"].as_u64().unwrap();
    assert_eq!(
        ok(&mut s, "gis.job.status", json!({"job": job}))["status"],
        "running"
    );
    // The server sends junk, so the job fails with a message.
    let done = ok(&mut s, "gis.job.status", json!({"job": job, "wait": 5}));
    assert_eq!(done["status"], "failed");
    assert!(
        done["error"].as_str().unwrap().contains("not an XML"),
        "{done}"
    );
    assert!(err(&mut s, "gis.job.status", json!({"job": 777})).contains("no job"));
    assert!(err(&mut s, "gis.job.status", json!({})).contains("missing"));
    // Cancel.
    let r = ok(
        &mut s,
        "gis.wfs.capabilities",
        json!({"url": format!("{}/wfs", m.base)}),
    );
    let job = r["job"].as_u64().unwrap();
    assert_eq!(
        ok(&mut s, "gis.job.cancel", json!({"job": job}))["cancelled"],
        true
    );
    assert_eq!(
        ok(&mut s, "gis.job.status", json!({"job": job}))["status"],
        "failed"
    );
    assert!(err(&mut s, "gis.job.cancel", json!({"job": 777})).contains("no job"));
}

#[test]
fn user_dialog_jobs_apply_through_the_poll() {
    let m = services();
    // The agent policy is empty, but the user's own dialog is trusted.
    let mut s = Session::with_sequential_ids();
    s.gis.cache_dir = Some(own_cache());
    set_crs(&mut s);
    let r = s
        .gis_call_trusted("gis.wms.add", json!({"url": format!("{}/wms", m.base), "layers": ["ortho"], "bbox": [0, 0, 50000, 50000], "max_px": 128}))
        .unwrap();
    assert_eq!(r["status"], "running");
    assert_eq!(s.gis_jobs().len(), 1);
    let mut done = Vec::new();
    for _ in 0..500 {
        done = s.gis_poll();
        if !done.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(done.len(), 1);
    assert!(done[0].2.is_ok(), "{:?}", done[0]);
    assert!(s.gis_jobs().is_empty());
    assert!(
        s.editor
            .doc()
            .objects
            .values()
            .any(|o| matches!(o.shape, cadektor_core::model::Shape::Underlay(_)))
    );
    // The same call as an agent is refused.
    assert!(err(&mut s, "gis.wms.add", json!({"url": format!("{}/wms", m.base), "layers": ["ortho"], "bbox": [0, 0, 5000, 5000]})).contains("not on the list"));
    let _ = std::fs::remove_dir_all(s.gis.cache_dir.clone().unwrap());
}

#[test]
fn wfs_import_then_massing_from_shapes() {
    let m = services();
    let mut s = session_for(&m);
    set_crs(&mut s);
    let r = ok(
        &mut s,
        "gis.wfs.import",
        json!({"url": format!("{}/wfs", m.base), "type_name": "ns:buildings", "options": {"layer": "Buildings"}}),
    );
    assert_eq!(r["imported"], 3);
    // The properties are in the object data.
    let tall = s
        .editor
        .doc()
        .objects
        .values()
        .find(|o| o.data.get("name") == Some(&json!("Tall")))
        .unwrap();
    assert_eq!(tall.data["height"], json!(12.5));
    let ids: Vec<cadektor_core::uid::Uid> = s.editor.doc().objects.keys().map(|k| k.0).collect();
    let dry = ok(
        &mut s,
        "massing.from_shapes",
        json!({"ids": ids, "dry_run": true, "roof": "gable"}),
    );
    assert_eq!(dry["created"].as_array().unwrap().len(), 3);
    assert_eq!(s.editor.doc().objects.len(), 3, "a dry run changes nothing");
    let r = ok(
        &mut s,
        "massing.from_shapes",
        json!({"ids": ids, "roof": "gable", "pitch": 35, "name_field": "name"}),
    );
    let created = r["created"].as_array().unwrap();
    assert_eq!(created.len(), 3);
    assert_eq!(created[0]["how"], "height from height");
    assert_eq!(created[1]["how"], "floors from levels");
    assert!(
        created[2]["how"]
            .as_str()
            .unwrap()
            .starts_with("default height")
    );
    assert_eq!(s.editor.doc().objects.len(), 3, "the sources were replaced");
    let rep = ok(&mut s, "massing.report", json!({}));
    assert_eq!(rep["totals"]["count"], 3);
    let tall = rep["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "Tall")
        .unwrap();
    assert_eq!(tall["roof"], "gable");
    assert!((tall["footprint_m2"].as_f64().unwrap() - 400.0).abs() < 0.01);
    assert!((tall["eave_height_m"].as_f64().unwrap() - 12.5).abs() < 1e-9);
    assert!(tall["volume_m3"].as_f64().unwrap() > 400.0 * 12.5);
    let low = rep["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "Low")
        .unwrap();
    assert_eq!(low["floors"], 2);
    assert!((low["gross_floor_area_m2"].as_f64().unwrap() - 200.0).abs() < 0.01);
    let csv = ok(&mut s, "massing.report", json!({"format": "csv"}));
    assert!(
        csv["csv"]
            .as_str()
            .unwrap()
            .starts_with("id,name,layer,floors,roof,")
    );
    assert!(err(&mut s, "massing.report", json!({"format": "xls"})).contains("csv"));
    // One undo brings the footprints back.
    ok(&mut s, "undo", json!({}));
    assert!(
        s.editor
            .doc()
            .objects
            .values()
            .all(|o| matches!(o.shape, cadektor_core::model::Shape::Polyline { .. }))
    );
    assert!(err(&mut s, "massing.from_shapes", json!({"ids": []})).contains("select"));
    assert!(
        err(
            &mut s,
            "massing.from_shapes",
            json!({"ids": ids, "default_height": -1})
        )
        .contains("positive")
    );
    // Another type name is refused by the capabilities check.
    let e = err(
        &mut s,
        "gis.wfs.import",
        json!({"url": format!("{}/wfs", m.base), "type_name": "ns:rivers"}),
    );
    assert!(e.contains("no feature type ns:rivers"), "{e}");
    // Without the capabilities call the same request works too.
    let r = ok(
        &mut s,
        "gis.wfs.import",
        json!({"url": format!("{}/wfs", m.base), "type_name": "ns:rivers", "caps": false, "format": "geojson"}),
    );
    assert_eq!(r["imported"], 3);
}

#[test]
fn massing_methods() {
    let mut s = Session::with_sequential_ids();
    let r = ok(
        &mut s,
        "massing.add",
        json!({"footprint": [[0, 0], [12000, 0], [12000, 8000], [0, 8000]], "floors": 3, "floor_height": 2800, "roof": "hip", "pitch": 30, "name": "House"}),
    );
    let id = r["id"].as_uid().unwrap();
    let q = &r["quantities"];
    assert_eq!(q["floors"], 3);
    assert!((q["gross_floor_area_m2"].as_f64().unwrap() - 288.0).abs() < 1e-6);
    let t = 30f64.to_radians().tan();
    let rise = 4.0 * t;
    let want = 96.0 * 8.4 + 8.0 * rise * (36.0 - 8.0) / 6.0;
    assert!(
        (q["volume_m3"].as_f64().unwrap() - want).abs() < 1e-6,
        "{q}"
    );
    let g = ok(&mut s, "massing.get", json!({"id": id}));
    assert_eq!(g["roof"], "hip");
    assert_eq!(g["pitch"], 30.0);
    assert_eq!(g["base_elevation_mm"], 0.0);
    ok(
        &mut s,
        "massing.set",
        json!({"id": id, "roof": "gable", "floors": null, "height": 7000, "pitch": 40, "base": 500, "ridge_angle": 0.0}),
    );
    let g = ok(&mut s, "massing.get", json!({"id": id}));
    assert_eq!(
        (g["roof"].clone(), g["height"].clone(), g["floors"].clone()),
        (json!("gable"), json!(7000.0), Value::Null)
    );
    assert_eq!(g["base"], 500.0);
    assert_eq!(g["base_elevation_mm"], 500.0);
    ok(
        &mut s,
        "massing.set",
        json!({"ids": [id], "base": null, "ridge_angle": null}),
    );
    // Errors.
    assert!(err(&mut s, "massing.add", json!({"height": 3000})).contains("footprint"));
    assert!(
        err(
            &mut s,
            "massing.add",
            json!({"footprint": [[0, 0], [1, 1]]})
        )
        .contains("3")
    );
    assert!(
        err(
            &mut s,
            "massing.add",
            json!({"footprint": [[0, 0], [1000, 0], [0, 1000]], "roof": "dome"})
        )
        .contains("flat, gable or hip")
    );
    assert!(
        err(
            &mut s,
            "massing.add",
            json!({"footprint": [[0, 0], [1000, 0], [0, 1000]], "color": 1})
        )
        .contains("unknown field")
    );
    assert!(
        err(
            &mut s,
            "massing.add",
            json!({"footprint": [[0, 0], [1000, 0], [0, 1000]], "pitch": 95})
        )
        .contains("pitch")
    );
    assert!(err(&mut s, "massing.set", json!({"id": id, "floors": 0})).contains("floors"));
    assert!(err(&mut s, "massing.set", json!({})).contains("id or ids"));
    assert!(
        err(
            &mut s,
            "massing.set",
            json!({"id": "00000000-0000-0000-0000-00000001869f", "roof": "flat"})
        )
        .contains("does not exist")
    );
    assert!(
        err(
            &mut s,
            "massing.get",
            json!({"id": "00000000-0000-0000-0000-00000001869f"})
        )
        .contains("does not exist")
    );
    let c = ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}}}))["created"][0].as_uid().unwrap();
    assert!(err(&mut s, "massing.get", json!({"id": c})).contains("not a massing"));
    assert!(err(&mut s, "massing.set", json!({"id": c, "roof": "flat"})).contains("not a massing"));
    assert!(err(&mut s, "massing.add", json!([1])).contains("object"));
    // The model is also a normal shape for the generic commands.
    let r = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "massing", "footprint": [[0, 0], [5000, 0], [5000, 5000]], "height": 4000}}}),
    );
    assert!(r["created"][0].as_uid().is_some());
}

#[test]
fn reproject_and_coords_through_the_api() {
    let mut s = Session::with_sequential_ids();
    assert!(err(&mut s, "gis.coords", json!({"x": 0, "y": 0})).contains("no CRS"));
    assert!(err(&mut s, "gis.reproject", json!({"crs": "EPSG:32635"})).contains("no CRS"));
    assert!(err(&mut s, "gis.georef.set", json!({"crs": "EPSG:4326"})).contains("projected"));
    assert!(err(&mut s, "gis.georef.set", json!({"crs": "nonsense"})).len() > 5);
    set_crs(&mut s);
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [20000, 10000]}}}),
    );
    let c = ok(&mut s, "gis.coords", json!({"x": 10500, "y": 250}));
    assert_eq!(c["easting"], 385010.5);
    assert_eq!(c["northing"], 6672000.25);
    assert!(c["lat"].as_f64().unwrap() > 60.0);
    let dry = ok(
        &mut s,
        "gis.reproject",
        json!({"crs": "EPSG:32635", "dry_run": true}),
    );
    assert_eq!(dry["applied"], false);
    assert_eq!(dry["report"]["objects_changed"], 1);
    assert_eq!(s.editor.doc().georef.as_ref().unwrap().crs, "EPSG:3067");
    let done = ok(&mut s, "gis.reproject", json!({"crs": "EPSG:32635"}));
    assert_eq!(done["applied"], true);
    assert_eq!(s.editor.doc().georef.as_ref().unwrap().crs, "EPSG:32635");
    // The same place on the earth.
    let c2 = ok(&mut s, "gis.coords", json!({"x": 10500, "y": 250}));
    assert!((c2["lat"].as_f64().unwrap() - c["lat"].as_f64().unwrap()).abs() < 1e-5);
    assert!((c2["lon"].as_f64().unwrap() - c["lon"].as_f64().unwrap()).abs() < 1e-5);
    ok(&mut s, "undo", json!({}));
    assert_eq!(s.editor.doc().georef.as_ref().unwrap().crs, "EPSG:3067");
    assert!(err(&mut s, "gis.reproject", json!({"crs": "EPSG:4326"})).contains("projected"));
    assert!(err(&mut s, "gis.reproject", json!({})).len() > 3);
}

#[test]
fn geoimage_import_follows_the_path_policy() {
    let dir = std::env::temp_dir().join(format!("cadektor-api-img-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("plan.png"), png(10, 10, [200, 30, 30, 255])).unwrap();
    std::fs::write(dir.join("plan.pgw"), "2\n0\n0\n-2\n385001\n6672009\n").unwrap();
    std::fs::write(dir.join("note.txt"), "x").unwrap();
    let mut s = Session::with_sequential_ids();
    // Into a document without a georeference: it gets one.
    let r = ok(
        &mut s,
        "gis.geoimage.import",
        json!({"path": dir.join("plan.png"), "crs": "EPSG:3067", "name": "Plan", "opacity": 0.5}),
    );
    assert_eq!(r["georeference"]["crs"], "EPSG:3067");
    assert_eq!(r["width"], 10);
    let id = r["id"].as_uid().unwrap();
    assert!(
        s.editor
            .doc()
            .object(cadektor_core::model::ObjectId(id))
            .is_some()
    );
    // Without a CRS anywhere there is a hint.
    let mut s2 = Session::with_sequential_ids();
    let e = err(
        &mut s2,
        "gis.geoimage.import",
        json!({"path": dir.join("plan.png")}),
    );
    assert!(e.contains("Name the CRS"), "{e}");
    // A restricted session only reads allowed folders and picture types.
    let mut s3 = Session::with_sequential_ids();
    s3.paths = cadektor_api::PathPolicy::restricted([dir.clone()]);
    ok(
        &mut s3,
        "gis.geoimage.import",
        json!({"path": dir.join("plan.png"), "crs": "EPSG:3067"}),
    );
    assert!(
        err(
            &mut s3,
            "gis.geoimage.import",
            json!({"path": dir.join("note.txt")})
        )
        .contains("extension")
    );
    let other = std::env::temp_dir().join(format!("cadektor-api-img-other-{}", std::process::id()));
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("p.png"), png(2, 2, [1, 2, 3, 255])).unwrap();
    assert!(
        err(
            &mut s3,
            "gis.geoimage.import",
            json!({"path": other.join("p.png")})
        )
        .contains("outside")
    );
    assert!(
        err(
            &mut s,
            "gis.geoimage.import",
            json!({"path": dir.join("plan.png"), "crs": "EPSG:3067", "max_px": 0})
        )
        .contains("max_px")
    );
    assert!(
        err(
            &mut s,
            "gis.geoimage.import",
            json!({"path": dir.join("plan.png"), "layer": "00000000-0000-0000-0000-00000000270f"})
        )
        .contains("does not exist")
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
fn describe_lists_the_new_methods_and_examples() {
    let mut s = Session::with_sequential_ids();
    let d = ok(&mut s, "describe", json!({}));
    let names: Vec<&str> = d["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    for n in [
        "gis.sources",
        "gis.cache.clear",
        "gis.georef.set",
        "gis.coords",
        "gis.reproject",
        "gis.wms.capabilities",
        "gis.wms.add",
        "gis.wms.refresh",
        "gis.wmts.capabilities",
        "gis.wmts.add",
        "gis.wfs.capabilities",
        "gis.wfs.import",
        "gis.geoimage.import",
        "gis.job.status",
        "gis.job.cancel",
        "massing.add",
        "massing.set",
        "massing.get",
        "massing.from_shapes",
        "massing.report",
    ] {
        assert!(names.contains(&n), "{n}");
    }
    assert!(
        d["gis"]["network_policy"]
            .as_str()
            .unwrap()
            .contains("--allow-url")
    );
    assert!(d["gis"]["examples"].as_array().unwrap().len() >= 6);
    assert!(d["massing"]["examples"].as_array().unwrap().len() >= 3);
    // Every example names a listed method.
    for e in d["gis"]["examples"]
        .as_array()
        .unwrap()
        .iter()
        .chain(d["massing"]["examples"].as_array().unwrap())
    {
        assert!(names.contains(&e["method"].as_str().unwrap()), "{e}");
    }
}

#[test]
fn replacing_the_document_keeps_the_network_policy() {
    let mut s = Session::with_sequential_ids();
    s.gis.urls = UrlPolicy::allow_list(["maps.example.org".to_string()]);
    s.replace_document(cadektor_core::model::Document::default());
    assert_eq!(s.gis.urls.entries(), ["maps.example.org".to_string()]);
}

fn wmts_server() -> Mock {
    let base = Arc::new(Mutex::new(String::new()));
    let b2 = base.clone();
    let m = Mock::start(move |p| {
        let b = b2.lock().unwrap().clone();
        if p.starts_with("/wmts") && p.contains("REQUEST=GetCapabilities") {
            let matrices: String = (0..6)
                .map(|z| {
                    format!(
                        "<TileMatrix><ows:Identifier>{z}</ows:Identifier><ScaleDenominator>{}</ScaleDenominator><TopLeftCorner>-20037508.342789244 20037508.342789244</TopLeftCorner><TileWidth>256</TileWidth><TileHeight>256</TileHeight><MatrixWidth>{}</MatrixWidth><MatrixHeight>{}</MatrixHeight></TileMatrix>",
                        559082264.0287178 / f64::from(1u32 << z),
                        1u32 << z,
                        1u32 << z
                    )
                })
                .collect();
            Reply::Ok(
                "text/xml",
                format!(
                    r#"<Capabilities xmlns="http://www.opengis.net/wmts/1.0" xmlns:ows="http://www.opengis.net/ows/1.1" version="1.0.0">
<ows:ServiceIdentification><ows:Title>Mock tiles</ows:Title></ows:ServiceIdentification>
<Contents><Layer><ows:Title>Base</ows:Title><ows:Identifier>base</ows:Identifier><Format>image/png</Format>
<TileMatrixSetLink><TileMatrixSet>WM</TileMatrixSet></TileMatrixSetLink>
<ResourceURL format="image/png" resourceType="tile" template="{b}/tiles/{{TileMatrix}}/{{TileCol}}/{{TileRow}}.png"/></Layer>
<TileMatrixSet><ows:Identifier>WM</ows:Identifier><ows:SupportedCRS>urn:ogc:def:crs:EPSG::3857</ows:SupportedCRS>{matrices}</TileMatrixSet></Contents></Capabilities>"#
                )
                .into_bytes(),
            )
        } else if p.starts_with("/tiles/") {
            Reply::Ok("image/png", png(256, 256, [220, 90, 40, 255]))
        } else {
            Reply::Status(404, "-")
        }
    });
    *base.lock().unwrap() = m.base.clone();
    m
}

#[test]
fn wmts_through_the_api_and_refresh() {
    let m = wmts_server();
    let mut s = session_for(&m);
    set_crs(&mut s);
    let c = ok(
        &mut s,
        "gis.wmts.capabilities",
        json!({"url": format!("{}/wmts", m.base)}),
    );
    assert_eq!(c["title"], "Mock tiles");
    assert_eq!(c["layers"][0]["id"], "base");
    assert_eq!(c["matrix_sets"][0]["levels"], 6);
    // A drawing gives the default view: the objects plus a margin.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [40000, 30000]}}}),
    );
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "text", "at": [100, 100], "text": "x", "height": 100}}}),
    );
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_layer", "name": "Maps"}}),
    );
    let r = ok(
        &mut s,
        "gis.wmts.add",
        json!({"url": format!("{}/wmts", m.base), "layer": "base", "max_px": 256, "layer_name": "Maps", "opacity": 0.6, "name": "Base map"}),
    );
    assert_eq!(r["stats"]["service"], "wmts");
    let id = r["id"].as_uid().unwrap();
    let o = s
        .editor
        .doc()
        .object(cadektor_core::model::ObjectId(id))
        .unwrap();
    assert_eq!(s.editor.doc().layer(o.layer).unwrap().name, "Maps");
    let cadektor_core::model::Shape::Underlay(u) = &o.shape else {
        panic!()
    };
    assert_eq!((u.name.as_str(), u.opacity), ("Base map", 0.6));
    // The view is larger than the drawing: it has a margin.
    assert!(u.min.x < 0.0 && u.max.x > 40000.0);
    assert_eq!(o.data["gis_source"]["kind"], "wmts");
    // Refresh with the stored source.
    let r2 = ok(
        &mut s,
        "gis.wms.refresh",
        json!({"id": id, "bbox": [0, 0, 20000, 20000]}),
    );
    assert_eq!(r2["stats"]["service"], "wmts");
    // A layer that does not exist.
    assert!(
        err(
            &mut s,
            "gis.wmts.add",
            json!({"url": format!("{}/wmts", m.base), "layer": "nope", "bbox": [0, 0, 1000, 1000]})
        )
        .contains("no layer named nope")
    );
    assert!(err(&mut s, "gis.wmts.add", json!({"url": format!("{}/wmts", m.base), "layer": "base", "bbox": [0, 0, 1000, 1000], "layer_name": "None"})).contains("no layer named None"));
    // An underlay from a file has no source to fetch again.
    let dir = std::env::temp_dir().join(format!("cadektor-api-gis-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("p.png"), png(4, 4, [1, 2, 3, 255])).unwrap();
    std::fs::write(dir.join("p.pgw"), "2\n0\n0\n-2\n385001\n6672009\n").unwrap();
    let f = ok(
        &mut s,
        "gis.geoimage.import",
        json!({"path": dir.join("p.png"), "crs": "EPSG:3067"}),
    );
    assert!(err(&mut s, "gis.wms.refresh", json!({"id": f["id"]})).contains("no source"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(s.gis.cache_dir.clone().unwrap());
}

#[test]
fn georef_set_checks_its_values() {
    let mut s = Session::with_sequential_ids();
    assert!(
        err(
            &mut s,
            "gis.georef.set",
            json!({"crs": "EPSG:3067", "rotation": "x"})
        )
        .len()
            > 3
    );
    let r = ok(
        &mut s,
        "gis.georef.set",
        json!({"crs": "EPSG:3067", "origin": [1, 2], "rotation": 0.5}),
    );
    assert_eq!(r["georeference"]["rotation"], 0.5);
    // Setting it again keeps the user origin.
    let r = ok(&mut s, "gis.georef.set", json!({"crs": "EPSG:32635"}));
    assert_eq!(r["georeference"]["origin"], json!([0.0, 0.0]));
    assert!(err(&mut s, "gis.job.cancel", json!({})).contains("missing"));
    assert!(err(&mut s, "gis.wms.add", json!("x")).contains("object"));
}

#[test]
fn massing_in_a_group_and_report_filters() {
    let mut s = Session::with_sequential_ids();
    let a = ok(
        &mut s,
        "massing.add",
        json!({"footprint": [[0, 0], [5000, 0], [5000, 5000]], "name": "A"}),
    )["id"]
        .as_uid()
        .unwrap();
    let b = ok(
        &mut s,
        "massing.add",
        json!({"footprint": [[9000, 0], [14000, 0], [14000, 5000]], "name": "B"}),
    )["id"]
        .as_uid()
        .unwrap();
    let g = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "group", "ids": [a]}}),
    )["created"][0]
        .as_uid()
        .unwrap();
    // A new model in the group joins it.
    let c = ok(
        &mut s,
        "massing.add",
        json!({"footprint": [[0, 9000], [5000, 9000], [5000, 14000]], "parent": g}),
    )["id"]
        .as_uid()
        .unwrap();
    assert_eq!(
        s.editor
            .doc()
            .object(cadektor_core::model::ObjectId(c))
            .unwrap()
            .parent,
        Some(cadektor_core::model::ObjectId(g))
    );
    let rep = ok(&mut s, "massing.report", json!({"ids": [b]}));
    assert_eq!(rep["rows"].as_array().unwrap().len(), 1);
    assert_eq!(rep["rows"][0]["name"], "B");
    assert!(err(&mut s, "massing.report", json!({"ids": "x"})).len() > 3);
    assert!(err(&mut s, "massing.nope", json!({})).contains("unknown method"));
    // Several at once with set, and a bad one rolls the whole batch back.
    ok(
        &mut s,
        "massing.set",
        json!({"ids": [b, c], "height": 9000, "name": "same"}),
    );
    let bad = err(
        &mut s,
        "massing.set",
        json!({"ids": [b, cadektor_core::uid::Uid::from_num(99999)], "height": 1}),
    );
    assert!(bad.contains("does not exist"));
    assert_eq!(
        ok(&mut s, "massing.get", json!({"id": b}))["height"],
        9000.0
    );
}

#[test]
fn a_result_is_applied_only_to_the_document_it_was_made_for() {
    let m = Mock::start(|p| {
        if p.contains("REQUEST=GetFeature") {
            std::thread::sleep(std::time::Duration::from_millis(150));
            Reply::Ok(
                "application/json",
                br#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[385000,6672000]}}]}"#.to_vec(),
            )
        } else {
            Reply::Status(404, "-")
        }
    });
    let import = |s: &mut Session| {
        ok(
            s,
            "gis.wfs.import",
            json!({"url": format!("{}/wfs", m.base), "type_name": "t", "caps": false, "format": "geojson"}),
        )
    };
    // 1. Another document is opened while the call runs.
    let mut s = session_for(&m);
    s.gis.blocking = false;
    set_crs(&mut s);
    let job = import(&mut s)["job"].as_u64().unwrap();
    s.replace_document(cadektor_core::model::Document::default());
    let r = ok(&mut s, "gis.job.status", json!({"job": job, "wait": 5}));
    assert_eq!(r["status"], "failed");
    assert!(
        r["error"].as_str().unwrap().contains("another document"),
        "{r}"
    );
    assert!(s.editor.doc().objects.is_empty());
    // 2. The georeference changes while the call runs.
    set_crs(&mut s);
    let job = import(&mut s)["job"].as_u64().unwrap();
    ok(
        &mut s,
        "gis.georef.set",
        json!({"crs": "EPSG:32635", "origin": [500000, 6670000]}),
    );
    let r = ok(&mut s, "gis.job.status", json!({"job": job, "wait": 5}));
    assert_eq!(r["status"], "failed");
    assert!(
        r["error"]
            .as_str()
            .unwrap()
            .contains("georeference changed"),
        "{r}"
    );
    assert!(s.editor.doc().objects.is_empty());
    // 3. Nothing changed: the result is applied.
    let job = import(&mut s)["job"].as_u64().unwrap();
    let r = ok(&mut s, "gis.job.status", json!({"job": job, "wait": 5}));
    assert_eq!(r["status"], "done", "{r}");
    assert_eq!(s.editor.doc().objects.len(), 1);
}

#[test]
fn cancel_stops_the_worker() {
    let m = Mock::start(|p| {
        if p.contains("REQUEST=GetFeature") {
            std::thread::sleep(std::time::Duration::from_millis(150));
            let start: usize = p
                .split("STARTINDEX=")
                .nth(1)
                .and_then(|t| t.split('&').next())
                .and_then(|t| t.parse().ok())
                .unwrap_or(0);
            let count: usize = p
                .split("COUNT=")
                .nth(1)
                .and_then(|t| t.split('&').next())
                .and_then(|t| t.parse().ok())
                .unwrap_or(1000);
            let features: Vec<String> = (start..start + count)
                .map(|i| format!(r#"{{"type":"Feature","properties":{{"i":{i}}},"geometry":{{"type":"Point","coordinates":[385000,6672000]}}}}"#))
                .collect();
            Reply::Ok(
                "application/json",
                format!(
                    r#"{{"type":"FeatureCollection","features":[{}]}}"#,
                    features.join(",")
                )
                .into_bytes(),
            )
        } else {
            Reply::Status(404, "-")
        }
    });
    let mut s = session_for(&m);
    s.gis.blocking = false;
    set_crs(&mut s);
    let r = ok(
        &mut s,
        "gis.wfs.import",
        json!({"url": format!("{}/wfs", m.base), "type_name": "t", "caps": false, "format": "geojson", "max_features": 20000}),
    );
    let job = r["job"].as_u64().unwrap();
    while m.hit_count() == 0 {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    ok(&mut s, "gis.job.cancel", json!({"job": job}));
    std::thread::sleep(std::time::Duration::from_millis(500));
    let seen = m.hit_count();
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(
        m.hit_count(),
        seen,
        "no request after the worker saw the cancel"
    );
    assert!(seen < 6, "it would have asked 20 times: {seen}");
    assert!(s.editor.doc().objects.is_empty());
}

#[test]
fn wfs_reports_when_the_limit_cut_the_features() {
    let m = Mock::start(|p| {
        if p.contains("REQUEST=GetFeature") {
            let start: usize = p
                .split("STARTINDEX=")
                .nth(1)
                .and_then(|t| t.split('&').next())
                .and_then(|t| t.parse().ok())
                .unwrap_or(0);
            let count: usize = p
                .split("COUNT=")
                .nth(1)
                .and_then(|t| t.split('&').next())
                .and_then(|t| t.parse().ok())
                .unwrap_or(1000);
            let features: Vec<String> = (start..start + count)
                .map(|i| format!(r#"{{"type":"Feature","properties":{{"i":{i}}},"geometry":{{"type":"Point","coordinates":[385000,6672000]}}}}"#))
                .collect();
            Reply::Ok(
                "application/json",
                format!(
                    r#"{{"type":"FeatureCollection","features":[{}]}}"#,
                    features.join(",")
                )
                .into_bytes(),
            )
        } else {
            Reply::Status(404, "-")
        }
    });
    let mut s = session_for(&m);
    set_crs(&mut s);
    let r = ok(
        &mut s,
        "gis.wfs.import",
        json!({"url": format!("{}/wfs", m.base), "type_name": "t", "caps": false, "format": "geojson", "max_features": 2500}),
    );
    assert_eq!(r["imported"], 2500);
    assert_eq!(r["truncated"], true);
    assert!(
        r["report"]["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n.as_str().unwrap().contains("limit"))
    );
}
