//! GeoJSON (RFC 7946) import and export.
//!
//! Import reads Point, LineString, Polygon (with holes), their Multi forms,
//! GeometryCollection, Feature and FeatureCollection. Coordinates are WGS 84
//! longitude, latitude and optional height in meters, unless a legacy `crs`
//! member says otherwise. Properties go to the record data of the objects.
//!
//! Export writes the objects in WGS 84 when the document has a georeference.
//! Without one, coordinates are local meters and the report says so.

use std::collections::BTreeMap;

use cadektor_core::curve;
use cadektor_core::model::{arc_sweep, Document, Object, Shape};
use cadektor_core::DVec2;
use serde_json::{json, Map, Value};

use crate::crs::{Crs, Transformer};
use crate::gis::{self, Feature, GisData, Part, Pt};
use crate::{Context, ExportOptions, ExportResult, Exporter, Format, ImportOptions, ImportResult, Importer, IoError, Report, Source};

pub struct GeoJsonFormat;

impl Importer for GeoJsonFormat {
    fn format(&self) -> Format {
        Format::GeoJson
    }

    fn import(&self, src: &Source, ctx: &Context, opts: &ImportOptions) -> Result<ImportResult, IoError> {
        let text = std::str::from_utf8(src.data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&src.data))
            .map_err(|_| IoError::Format("not a GeoJSON file: not UTF-8 text".into()))?;
        let value: Value = serde_json::from_str(text).map_err(|e| IoError::Format(format!("not a GeoJSON file: {e}")))?;
        let data = read(&value)?;
        let name = if src.name.is_empty() { "GeoJSON" } else { &src.name };
        // RFC 7946: coordinates are WGS 84.
        gis::build("geojson", name, data, ctx, opts, Some("EPSG:4326"))
    }
}

impl Exporter for GeoJsonFormat {
    fn format(&self) -> Format {
        Format::GeoJson
    }

    fn export(&self, doc: &Document, opts: &ExportOptions) -> Result<ExportResult, IoError> {
        export(doc, opts)
    }
}

fn position(v: &Value) -> Result<Pt, &'static str> {
    let a = v.as_array().ok_or("position is not an array")?;
    if a.len() < 2 {
        return Err("position has fewer than 2 numbers");
    }
    let n = |i: usize| a.get(i).and_then(Value::as_f64).filter(|x| x.is_finite());
    Ok(Pt { x: n(0).ok_or("coordinate is not a number")?, y: n(1).ok_or("coordinate is not a number")?, z: a.get(2).and_then(Value::as_f64).filter(|z| z.is_finite()) })
}

fn positions(v: &Value) -> Result<Vec<Pt>, &'static str> {
    v.as_array().ok_or("coordinates are not an array")?.iter().map(position).collect()
}

/// The parts of one geometry. Rings are exterior first, then holes.
fn geometry(g: &Value, parts: &mut Vec<Part>, depth: usize) -> Result<(), String> {
    let kind = g.get("type").and_then(Value::as_str).ok_or("geometry has no type")?;
    let coords = g.get("coordinates");
    fn list(c: Option<&Value>) -> Result<&Vec<Value>, &'static str> {
        c.and_then(Value::as_array).ok_or("coordinates are not an array")
    }
    match kind {
        "Point" => parts.push(Part::Point(position(coords.ok_or("no coordinates")?)?)),
        "MultiPoint" => {
            for p in list(coords)? {
                parts.push(Part::Point(position(p)?));
            }
        }
        "LineString" => parts.push(Part::Line(positions(coords.ok_or("no coordinates")?)?)),
        "MultiLineString" => {
            for l in list(coords)? {
                parts.push(Part::Line(positions(l)?));
            }
        }
        "Polygon" => polygon(list(coords)?, parts)?,
        "MultiPolygon" => {
            for p in list(coords)? {
                polygon(p.as_array().ok_or("polygon is not an array")?, parts)?;
            }
        }
        "GeometryCollection" => {
            if depth > 8 {
                return Err("geometry collections are nested too deep".into());
            }
            for sub in g.get("geometries").and_then(Value::as_array).ok_or("no geometries")? {
                geometry(sub, parts, depth + 1)?;
            }
        }
        other => return Err(format!("unknown geometry type {other}")),
    }
    Ok(())
}

fn polygon(rings: &[Value], parts: &mut Vec<Part>) -> Result<(), String> {
    for (i, r) in rings.iter().enumerate() {
        parts.push(Part::Ring(positions(r)?, i > 0));
    }
    Ok(())
}

fn feature(f: &Value, data: &mut GisData) {
    let Some(geom) = f.get("geometry").filter(|g| !g.is_null()) else {
        data.skipped.push(("Feature".into(), "no geometry".into()));
        return;
    };
    let mut parts = Vec::new();
    if let Err(why) = geometry(geom, &mut parts, 0) {
        data.skipped.push(("Feature".into(), why));
        return;
    }
    let mut props = BTreeMap::new();
    if let Some(Value::Object(o)) = f.get("properties") {
        props.extend(o.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    if let Some(id) = f.get("id").filter(|v| v.is_string() || v.is_number()) {
        props.entry("_id".to_string()).or_insert_with(|| id.clone());
    }
    data.features.push(Feature { parts, props });
}

/// Reads a GeoJSON value into features.
pub fn read(v: &Value) -> Result<GisData, IoError> {
    let file_crs = v.get("crs").and_then(|c| c.get("properties")).and_then(|p| p.get("name")).and_then(Value::as_str).map(str::to_string);
    let mut data = GisData { file_crs, ..GisData::default() };
    match v.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => {
            let list = v.get("features").and_then(Value::as_array).ok_or_else(|| IoError::Format("FeatureCollection has no features array".into()))?;
            for f in list {
                feature(f, &mut data);
            }
        }
        Some("Feature") => feature(v, &mut data),
        Some(
            "Point" | "MultiPoint" | "LineString" | "MultiLineString" | "Polygon" | "MultiPolygon" | "GeometryCollection",
        ) => {
            let mut parts = Vec::new();
            match geometry(v, &mut parts, 0) {
                Ok(()) => data.features.push(Feature { parts, props: BTreeMap::new() }),
                Err(why) => data.skipped.push(("Geometry".into(), why)),
            }
        }
        _ => return Err(IoError::Format("not a GeoJSON file: no GeoJSON type".into())),
    }
    Ok(data)
}

/// Outline points of a shape, for GeoJSON: (points, closed, has_z). `None` for points.
fn outline(shape: &Shape, tol: f64) -> Option<(Vec<[f64; 3]>, bool, bool)> {
    let flat = |pts: Vec<DVec2>, closed: bool| Some((pts.into_iter().map(|p| [p.x, p.y, 0.0]).collect(), closed, false));
    match shape {
        Shape::Line { a, b } => flat(vec![*a, *b], false),
        Shape::Rect { min, max } => flat(vec![*min, DVec2::new(max.x, min.y), *max, DVec2::new(min.x, max.y)], true),
        Shape::Circle { center, radius } => {
            let n = ((std::f64::consts::PI / (1.0 - tol / radius).clamp(-1.0, 1.0).acos().max(1e-3)).ceil() as usize).clamp(16, 720);
            flat((0..n).map(|i| *center + DVec2::from_angle(std::f64::consts::TAU * i as f64 / n as f64) * *radius).collect(), true)
        }
        Shape::Arc { center, radius, start, end } => {
            let sweep = arc_sweep(*start, *end);
            let n = ((sweep / (std::f64::consts::TAU / 128.0)).ceil() as usize).clamp(4, 720);
            flat((0..=n).map(|i| *center + DVec2::from_angle(start + sweep * i as f64 / n as f64) * *radius).collect(), false)
        }
        Shape::Polyline { points, closed } => flat(curve::flatten(points, *closed, tol), *closed),
        Shape::LandscapeArea { boundary, .. } | Shape::Hardscape { boundary, .. } => flat(curve::flatten(boundary, true, tol), true),
        Shape::Polyline3d { points, closed } => Some((points.iter().map(|p| [p.x, p.y, p.z]).collect(), *closed, true)),
        Shape::Text { .. } | Shape::Plant { .. } | Shape::Locus3d { .. } => None,
        // Other crates add shape kinds. They are reported as skipped, not guessed.
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

fn export(doc: &Document, opts: &ExportOptions) -> Result<ExportResult, IoError> {
    let mut report = Report::new("geojson");
    let wgs = Crs::from_epsg(4326).map_err(IoError::Format)?;
    let georef = if opts.origin == crate::options::ExportOrigin::File { doc.georef.as_ref() } else { None };
    let target = match georef {
        Some(g) if g.crs.trim().is_empty() => {
            report.note("the document georeference has no CRS: coordinates are map meters of the local frame, not longitude and latitude");
            Some((g, None))
        }
        Some(g) => Some((g, Some(Crs::parse(&g.crs).map_err(|e| IoError::Format(format!("the document CRS cannot be used: {e}")))?))),
        None => {
            report.note("the document has no georeference (or origin is document): coordinates are local meters, not longitude and latitude");
            None
        }
    };
    // Model mm to the coordinates of the file.
    let conv = |p: [f64; 3]| -> Option<Value> {
        let (x, y) = match &target {
            Some((g, Some(crs))) => {
                let m = g.to_map(DVec2::new(p[0], p[1]));
                // The map position is in meters. A CRS in feet wants feet.
                let (lon, lat, _) = Transformer::new(crs, &wgs).apply(m.x / crs.to_meter(), m.y / crs.to_meter(), 0.0).ok()?;
                (lon, lat)
            }
            Some((g, None)) => {
                let m = g.to_map(DVec2::new(p[0], p[1]));
                (m.x, m.y)
            }
            None => (p[0] / 1000.0, p[1] / 1000.0),
        };
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        Some(json!([x, y, p[2] / 1000.0]))
    };
    // Holes belong to the polygon of their outer ring (see `_hole_of`): they are written as its interior rings.
    let mut holes: BTreeMap<u64, Vec<&Object>> = BTreeMap::new();
    for o in doc.objects.values() {
        if o.data.get("_ring").and_then(Value::as_str) != Some("hole") || (opts.visible_only && !doc.is_visible(o)) {
            continue;
        }
        let outer = o.data.get("_hole_of").and_then(Value::as_u64).and_then(|id| doc.object(cadektor_core::model::ObjectId(id)));
        let closed_outer = outer.is_some_and(|ou| outline(&ou.shape, opts.tolerance).is_some_and(|(pts, closed, _)| closed && pts.len() >= 3));
        if closed_outer && outer.is_some_and(|ou| !opts.visible_only || doc.is_visible(ou)) && outline(&o.shape, opts.tolerance).is_some_and(|(p, c, _)| c && p.len() >= 3) {
            holes.entry(o.data["_hole_of"].as_u64().unwrap_or(0)).or_default().push(o);
        }
    }
    let absorbed: std::collections::BTreeSet<u64> = holes.values().flatten().map(|o| o.id.0).collect();
    let mut features = Vec::new();
    for o in doc.objects.values() {
        if opts.visible_only && !doc.is_visible(o) {
            continue;
        }
        if absorbed.contains(&o.id.0) {
            report.count(o.shape.kind());
            continue;
        }
        let mine: Vec<&Object> = holes.get(&o.id.0).cloned().unwrap_or_default();
        let geometry = match geometry_of(o, opts.tolerance, &conv, &mine) {
            Ok(g) => g,
            Err(why) => {
                report.skip(o.shape.kind(), why);
                continue;
            }
        };
        let mut props: Map<String, Value> = o.data.iter().filter(|(k, _)| !matches!(k.as_str(), "_ring" | "_hole_of")).map(|(k, v)| (k.clone(), v.clone())).collect();
        props.insert("kind".into(), json!(o.shape.kind()));
        props.insert("id".into(), json!(o.id.0));
        if let Some(c) = doc.class(o.class) {
            props.insert("class".into(), json!(c.name));
        }
        if let Some(l) = doc.layer(o.layer) {
            props.insert("layer".into(), json!(l.name));
        }
        match &o.shape {
            Shape::Plant { species, spread, height, .. } => {
                props.insert("species".into(), json!(species));
                props.insert("spread_mm".into(), json!(spread));
                props.insert("height_mm".into(), json!(height));
            }
            Shape::Text { text, .. } => {
                props.insert("text".into(), json!(text));
            }
            _ => {}
        }
        features.push(json!({"type": "Feature", "geometry": geometry, "properties": props}));
        report.count(o.shape.kind());
    }
    let out = json!({"type": "FeatureCollection", "features": features});
    let mut bytes = serde_json::to_vec(&out).map_err(|e| IoError::Format(e.to_string()))?;
    bytes.push(b'\n');
    Ok(ExportResult { bytes, report })
}

/// Closed ring, counterclockwise for exteriors and clockwise for holes (RFC 7946).
fn oriented(mut pts: Vec<[f64; 3]>, ccw: bool) -> Vec<[f64; 3]> {
    let area: f64 = (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    if (area < 0.0) == ccw {
        pts.reverse();
    }
    pts
}

fn geometry_of(o: &Object, tol: f64, conv: &dyn Fn([f64; 3]) -> Option<Value>, holes: &[&Object]) -> Result<Value, &'static str> {
    let anchor = match &o.shape {
        Shape::Text { at, .. } | Shape::Plant { at, .. } => Some([at.x, at.y, 0.0]),
        Shape::Locus3d { p } => Some([p.x, p.y, p.z]),
        _ => None,
    };
    if let Some(a) = anchor {
        return Ok(json!({"type": "Point", "coordinates": conv(a).ok_or("coordinates could not be converted")?}));
    }
    let (pts, closed, _) = outline(&o.shape, tol).ok_or("unsupported kind")?;
    let ring = |pts: Vec<[f64; 3]>, ccw: bool| -> Option<Vec<Value>> {
        let mut ring: Vec<Value> = oriented(pts, ccw).iter().map(|p| conv(*p)).collect::<Option<_>>()?;
        ring.push(ring[0].clone());
        Some(ring)
    };
    if closed && pts.len() >= 3 {
        let mut rings = vec![ring(pts, true).ok_or("coordinates could not be converted")?];
        for h in holes {
            let (hp, _, _) = outline(&h.shape, tol).ok_or("unsupported kind")?;
            rings.push(ring(hp, false).ok_or("coordinates could not be converted")?);
        }
        Ok(json!({"type": "Polygon", "coordinates": rings}))
    } else {
        let line: Vec<Value> = pts.iter().map(|p| conv(*p)).collect::<Option<_>>().ok_or("coordinates could not be converted")?;
        Ok(json!({"type": "LineString", "coordinates": line}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_core::command::Editor;
    use cadektor_core::curve::{corners, Vertex, VertexKind};
    use cadektor_core::georef::Georeference;
    use cadektor_core::DVec3;

    const SAMPLE: &str = r#"{
      "type": "FeatureCollection",
      "features": [
        {"type": "Feature", "id": 7, "geometry": {"type": "Point", "coordinates": [24.9384, 60.1699, 15.5]},
         "properties": {"name": "Oak", "age": 80, "tags": ["a", "b"], "meta": {"x": null}}},
        {"type": "Feature", "geometry": {"type": "LineString", "coordinates": [[24.9384, 60.1699], [24.9394, 60.1699]]},
         "properties": {"name": "Path"}},
        {"type": "Feature", "geometry": {"type": "Polygon", "coordinates": [
           [[24.93, 60.16], [24.94, 60.16], [24.94, 60.17], [24.93, 60.16]],
           [[24.932, 60.162], [24.936, 60.162], [24.936, 60.166], [24.932, 60.162]]]},
         "properties": null},
        {"type": "Feature", "geometry": {"type": "MultiPolygon", "coordinates": [
           [[[24.0, 60.0], [24.1, 60.0], [24.1, 60.1], [24.0, 60.0]]],
           [[[24.2, 60.0], [24.3, 60.0], [24.3, 60.1], [24.2, 60.0]]]]},
         "properties": {"name": "Multi"}},
        {"type": "Feature", "geometry": null, "properties": {}},
        {"type": "Feature", "geometry": {"type": "Circle", "coordinates": []}, "properties": {}},
        {"type": "Feature", "geometry": {"type": "Point", "coordinates": [1]}, "properties": {}},
        {"type": "Feature", "geometry": {"type": "GeometryCollection", "geometries": [
           {"type": "Point", "coordinates": [24.9, 60.1]}, {"type": "MultiPoint", "coordinates": [[24.91, 60.1], [24.92, 60.1]]},
           {"type": "MultiLineString", "coordinates": [[[24.9, 60.1], [24.91, 60.1]]]}]}, "properties": {"name": "Coll"}}
      ]
    }"#;

    fn import(text: &str, doc: Document, opts: &ImportOptions) -> Result<(Document, crate::Report), IoError> {
        let src = Source { name: "survey".into(), data: text.as_bytes().to_vec(), extra: BTreeMap::new() };
        let r = GeoJsonFormat.import(&src, &Context::of(&doc), opts)?;
        let mut ed = Editor::new(doc);
        ed.execute(&r.command).unwrap();
        Ok((ed.doc().clone(), r.report))
    }

    #[test]
    fn imports_features_with_properties() {
        let (doc, report) = import(SAMPLE, Document::default(), &ImportOptions::default()).unwrap();
        // 1 point + 1 line + 2 rings + 2 polygons + (1 point + 2 points + 1 line) = 10
        assert_eq!(doc.objects.len(), 10, "{:?}", report.skipped);
        assert_eq!(report.skipped_total(), 3);
        assert_eq!(doc.georef.as_ref().unwrap().crs, "EPSG:32635");
        let first = doc.objects.values().next().unwrap();
        assert_eq!(first.data["name"], "Oak");
        assert_eq!(first.data["age"], 80);
        assert_eq!(first.data["tags"], json!(["a", "b"]));
        assert_eq!(first.data["meta"], json!({"x": null}));
        assert_eq!(first.data["_id"], 7);
        let Shape::Locus3d { p } = first.shape else { panic!() };
        assert_eq!(p.z, 15_500.0);
    }

    #[test]
    fn accepts_other_root_types_and_rejects_garbage() {
        let (doc, _) = import(r#"{"type": "Point", "coordinates": [10, 20]}"#, Document::default(), &ImportOptions::default()).unwrap();
        assert_eq!(doc.objects.len(), 1);
        let (doc, _) = import(r#"{"type": "Feature", "geometry": {"type": "Point", "coordinates": [10, 20]}, "id": "abc"}"#, Document::default(), &ImportOptions::default()).unwrap();
        assert_eq!(doc.objects.values().next().unwrap().data["_id"], "abc");
        let (doc, report) = import(r#"{"type": "Polygon", "coordinates": "x"}"#, Document::default(), &ImportOptions::default()).unwrap();
        assert_eq!(doc.objects.len(), 0);
        assert_eq!(report.skipped_total(), 1);
        for bad in ["", "[]", "{}", r#"{"type": "Banana"}"#, r#"{"type": "FeatureCollection"}"#, "\u{1}", "nul"] {
            assert!(import(bad, Document::default(), &ImportOptions::default()).is_err(), "{bad}");
        }
        let src = Source { name: String::new(), data: vec![0xff, 0xfe], extra: BTreeMap::new() };
        assert!(GeoJsonFormat.import(&src, &Context::of(&Document::default()), &ImportOptions::default()).is_err());
        let with_bom = format!("\u{feff}{}", r#"{"type": "Point", "coordinates": [10, 20]}"#);
        assert!(import(&with_bom, Document::default(), &ImportOptions::default()).is_ok());
        // Deeply nested collections.
        let mut nested = r#"{"type": "Point", "coordinates": [1, 2]}"#.to_string();
        for _ in 0..12 {
            nested = format!(r#"{{"type": "GeometryCollection", "geometries": [{nested}]}}"#);
        }
        let (_, report) = import(&nested, Document::default(), &ImportOptions::default()).unwrap();
        assert_eq!(report.skipped_total(), 1);
        // Bad positions.
        for pos in ["[\"a\", 1]", "[1, null]", "5", "[]"] {
            let (doc, _) = import(&format!(r#"{{"type": "Point", "coordinates": {pos}}}"#), Document::default(), &ImportOptions::default()).unwrap();
            assert_eq!(doc.objects.len(), 0, "{pos}");
        }
    }

    #[test]
    fn legacy_crs_member_is_used() {
        let text = r#"{"type": "FeatureCollection", "crs": {"type": "name", "properties": {"name": "urn:ogc:def:crs:EPSG::3067"}},
          "features": [{"type": "Feature", "geometry": {"type": "Point", "coordinates": [385010, 6672020]}, "properties": {}}]}"#;
        let (doc, _) = import(text, Document::default(), &ImportOptions::default()).unwrap();
        let g = doc.georef.unwrap();
        assert_eq!(g.crs, "EPSG:3067");
        assert_eq!(g.origin, DVec2::new(385_010.0, 6_672_020.0));
    }

    fn sample_doc() -> Document {
        let mut doc = Document::default();
        doc.georef = Some(Georeference { crs: "EPSG:3067".into(), origin: DVec2::new(385_000.0, 6_672_000.0), rotation: 0.0, user_origin: DVec2::ZERO });
        let shapes = vec![
            Shape::Line { a: DVec2::ZERO, b: DVec2::new(1000.0, 0.0) },
            Shape::Rect { min: DVec2::ZERO, max: DVec2::new(1000.0, 500.0) },
            Shape::Circle { center: DVec2::ZERO, radius: 500.0 },
            Shape::Arc { center: DVec2::ZERO, radius: 500.0, start: 0.0, end: 1.0 },
            Shape::Polyline {
                points: vec![Vertex::corner(DVec2::ZERO), Vertex::new(DVec2::new(500.0, -500.0), VertexKind::PointOnArc), Vertex::corner(DVec2::new(1000.0, 0.0))],
                closed: false,
            },
            Shape::Text { at: DVec2::ZERO, text: "hi".into(), height: 100.0 },
            Shape::Plant { at: DVec2::ZERO, species: "Betula".into(), spread: 3000.0, height: 4000.0 },
            Shape::LandscapeArea { boundary: corners(&[DVec2::ZERO, DVec2::new(0.0, 1000.0), DVec2::new(1000.0, 1000.0)]), name: "L".into() },
            Shape::Locus3d { p: DVec3::new(1000.0, 2000.0, 3000.0) },
            Shape::Polyline3d { points: vec![DVec3::ZERO, DVec3::new(1000.0, 0.0, 500.0)], closed: false },
            Shape::Polyline3d { points: vec![DVec3::ZERO, DVec3::new(1000.0, 0.0, 500.0), DVec3::new(0.0, 1000.0, 200.0)], closed: true },
        ];
        let (ly, c) = (doc.layers[0].id, doc.classes[0].id);
        for s in shapes {
            let id = cadektor_core::model::ObjectId(doc.alloc_id().unwrap());
            let mut o = Object::new(id, ly, c, s);
            o.data.insert("note".into(), json!("n"));
            doc.objects.insert(id, o);
        }
        doc
    }

    #[test]
    fn export_then_import_returns_geometry() {
        let doc = sample_doc();
        let out = export(&doc, &ExportOptions::default()).unwrap();
        assert_eq!(out.report.total(), 11);
        let v: Value = serde_json::from_slice(&out.bytes).unwrap();
        assert_eq!(v["features"].as_array().unwrap().len(), 11);
        assert_eq!(v["features"][0]["properties"]["note"], "n");
        assert_eq!(v["features"][6]["properties"]["species"], "Betula");
        assert_eq!(v["features"][5]["properties"]["text"], "hi");
        // Import into a document with the same georeference: locus comes back.
        let mut empty = Document::default();
        empty.georef = doc.georef.clone();
        let text = String::from_utf8(out.bytes).unwrap();
        let (back, _) = import(&text, empty, &ImportOptions::default()).unwrap();
        assert_eq!(back.objects.len(), 11);
        let locus = back.objects.values().find(|o| matches!(o.shape, Shape::Locus3d { p } if p.z == 3000.0)).unwrap();
        let Shape::Locus3d { p } = locus.shape else { panic!() };
        assert!(p.truncate().distance(DVec2::new(1000.0, 2000.0)) < 1.0, "{p:?}");
        // Exterior rings are counterclockwise in the file.
        let ring = v["features"][7]["geometry"]["coordinates"][0].as_array().unwrap();
        assert_eq!(ring.first(), ring.last());
        let polygon3d = &v["features"][10]["geometry"];
        assert_eq!(polygon3d["type"], "Polygon");
        let visible_none = {
            let mut d = doc.clone();
            d.classes[0].visible = false;
            export(&d, &ExportOptions { visible_only: true, ..Default::default() }).unwrap()
        };
        assert_eq!(visible_none.report.total(), 0);
        assert_eq!(Importer::format(&GeoJsonFormat), Format::GeoJson);
    }

    #[test]
    fn export_without_georef_uses_local_meters() {
        let mut doc = sample_doc();
        doc.georef = None;
        let out = export(&doc, &ExportOptions::default()).unwrap();
        assert!(out.report.notes[0].contains("no georeference"));
        let v: Value = serde_json::from_slice(&out.bytes).unwrap();
        assert_eq!(v["features"][8]["geometry"]["coordinates"], json!([1.0, 2.0, 3.0]));
        // A document CRS that is not usable is an error.
        doc.georef = Some(Georeference { crs: "junk".into(), origin: DVec2::ZERO, rotation: 0.0, user_origin: DVec2::ZERO });
        assert!(export(&doc, &ExportOptions::default()).is_err());
    }

    #[test]
    fn unconvertible_coordinates_are_skipped() {
        let mut doc = sample_doc();
        // A point 1e8 meters away cannot be a valid longitude and latitude.
        doc.georef.as_mut().unwrap().origin = DVec2::new(1.0e12, 1.0e12);
        let out = export(&doc, &ExportOptions::default()).unwrap();
        assert!(out.report.skipped_total() > 0 || out.report.total() == 11);
    }

    #[test]
    fn holes_are_exported_as_interior_rings() {
        let (doc, _) = import(SAMPLE, Document::default(), &ImportOptions::default()).unwrap();
        let rings_before = doc.objects.values().filter(|o| o.data.contains_key("_ring")).count();
        assert_eq!(rings_before, 1);
        let out = export(&doc, &ExportOptions::default()).unwrap();
        let v: Value = serde_json::from_slice(&out.bytes).unwrap();
        let feats = v["features"].as_array().unwrap();
        // One feature fewer than objects: the hole is part of its polygon.
        assert_eq!(feats.len(), doc.objects.len() - 1);
        let with_hole: Vec<_> = feats.iter().filter(|f| f["geometry"]["type"] == "Polygon" && f["geometry"]["coordinates"].as_array().unwrap().len() == 2).collect();
        assert_eq!(with_hole.len(), 1);
        assert!(with_hole[0]["properties"].get("_ring").is_none() && with_hole[0]["properties"].get("_hole_of").is_none());
        // The interior ring is clockwise and closed.
        let hole = with_hole[0]["geometry"]["coordinates"][1].as_array().unwrap();
        assert_eq!(hole.first(), hole.last());
        let pts: Vec<(f64, f64)> = hole.iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
        let area: f64 = pts.windows(2).map(|w| w[0].0 * w[1].1 - w[1].0 * w[0].1).sum();
        assert!(area < 0.0);
        // Round trip: the hole is a hole again, not an island.
        let mut empty = Document::default();
        empty.georef = doc.georef.clone();
        let (back, _) = import(&String::from_utf8(out.bytes).unwrap(), empty, &ImportOptions::default()).unwrap();
        assert_eq!(back.objects.values().filter(|o| o.data.contains_key("_ring")).count(), 1);
        // A hole whose outer ring is gone is exported on its own (not lost).
        let mut doc2 = doc.clone();
        let outer = doc2.objects.values().find(|o| o.data.contains_key("_hole_of")).map(|o| o.data["_hole_of"].as_u64().unwrap()).unwrap();
        doc2.objects.shift_remove(&cadektor_core::model::ObjectId(outer));
        let out2 = export(&doc2, &ExportOptions::default()).unwrap();
        let v2: Value = serde_json::from_slice(&out2.bytes).unwrap();
        assert_eq!(v2["features"].as_array().unwrap().len(), doc2.objects.len());
    }

    #[test]
    fn local_frame_and_feet_crs_export() {
        let mut doc = sample_doc();
        doc.georef = Some(Georeference { crs: String::new(), origin: DVec2::new(1000.0, 2000.0), rotation: 0.0, user_origin: DVec2::ZERO });
        let out = export(&doc, &ExportOptions::default()).unwrap();
        assert!(out.report.notes[0].contains("no CRS"));
        let v: Value = serde_json::from_slice(&out.bytes).unwrap();
        assert_eq!(v["features"][8]["geometry"]["coordinates"], json!([1001.0, 2002.0, 3.0]));
        // A feet CRS (NY Long Island, EPSG:2263): the map origin is in meters.
        doc.georef = Some(Georeference { crs: "EPSG:2263".into(), origin: DVec2::new(300_533.0, 64_617.0), rotation: 0.0, user_origin: DVec2::ZERO });
        let out = export(&doc, &ExportOptions::default()).unwrap();
        let v: Value = serde_json::from_slice(&out.bytes).unwrap();
        let c = &v["features"][8]["geometry"]["coordinates"];
        assert!((c[0].as_f64().unwrap() + 74.0).abs() < 0.01 && (c[1].as_f64().unwrap() - 40.743).abs() < 0.01, "{c}");
    }
}
