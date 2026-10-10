//! GML 3.2 simple features, as WFS 2.0 sends them.
//!
//! The reader finds the feature members, takes the geometry property of each (Point, LineString,
//! Curve, Polygon, Surface and the Multi forms) and keeps the other simple properties as data. It
//! gives the same `GisData` that the GeoJSON and Shapefile readers give, so the importer is the same.
//!
//! Axis order: a CRS written as `urn:ogc:def:crs:EPSG::4326` or `http://www.opengis.net/def/crs/EPSG/0/4326`
//! has the axis order of the EPSG database (latitude first for geographic CRSs, northing first for a few
//! projected ones). A short `EPSG:4326` means x, y. Positions are turned to x, y.

use std::collections::BTreeMap;

use cadektor_io::crs::Crs;
use cadektor_io::gis::{Feature, GisData, Part, Pt};
use roxmltree::Node;
use serde_json::Value;

use crate::tiles::northing_first;
use crate::{GisError, Result};

const GEOMETRY_NAMES: [&str; 12] = [
    "Point",
    "LineString",
    "Curve",
    "Polygon",
    "Surface",
    "MultiPoint",
    "MultiCurve",
    "MultiLineString",
    "MultiSurface",
    "MultiPolygon",
    "MultiGeometry",
    "CompositeCurve",
];

fn is_geometry(n: Node<'_, '_>) -> bool {
    n.is_element() && GEOMETRY_NAMES.contains(&n.tag_name().name())
}

/// True if the srsName is in the long form that follows the axis order of the EPSG database.
fn long_form(srs: &str) -> bool {
    let s = srs.to_ascii_lowercase();
    s.starts_with("urn:") || s.starts_with("http://www.opengis.net/def/crs") || s.starts_with("https://www.opengis.net/def/crs")
}

/// Whether positions in this srsName come as y, x.
pub fn y_first(srs: &str) -> bool {
    if !long_form(srs) {
        return false;
    }
    Crs::parse(srs)
        .map(|c| northing_first(c.epsg(), c.is_geographic()))
        .unwrap_or(false)
}

struct Ctx {
    srs: Option<String>,
    dim: usize,
    problems: Vec<String>,
}

fn attr<'a>(n: Node<'a, '_>, name: &str) -> Option<&'a str> {
    n.attributes().find(|a| a.name() == name).map(|a| a.value())
}

fn srs_of(n: Node<'_, '_>, inherited: &Option<String>) -> Option<String> {
    attr(n, "srsName").map(str::to_string).or_else(|| inherited.clone())
}

fn numbers(s: &str) -> Vec<f64> {
    s.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse::<f64>().ok())
        .collect()
}

/// The points of a `posList`, `pos` or `coordinates` below a node.
fn positions(n: Node<'_, '_>, ctx: &Ctx, srs: &Option<String>) -> Vec<Pt> {
    let dim = attr(n, "srsDimension")
        .and_then(|d| d.parse::<usize>().ok())
        .unwrap_or(ctx.dim)
        .clamp(2, 3);
    let swap = srs.as_deref().is_some_and(y_first);
    let mut out = Vec::new();
    let mut push = |v: &[f64]| {
        if v.len() >= 2 {
            let (x, y) = if swap { (v[1], v[0]) } else { (v[0], v[1]) };
            out.push(Pt { x, y, z: v.get(2).copied().filter(|z| z.is_finite()) });
        }
    };
    match n.tag_name().name() {
        "pos" => push(&numbers(n.text().unwrap_or(""))),
        "posList" => {
            let v = numbers(n.text().unwrap_or(""));
            for c in v.chunks_exact(dim) {
                push(c);
            }
        }
        "coordinates" => {
            // GML 2 style: "x,y x,y".
            for pair in n.text().unwrap_or("").split_whitespace() {
                push(&numbers(pair));
            }
        }
        _ => {}
    }
    out
}

/// All coordinates below a geometry in document order.
fn collect(n: Node<'_, '_>, ctx: &Ctx, srs: &Option<String>) -> Vec<Pt> {
    let mut out = Vec::new();
    for d in n.descendants().filter(|d| d.is_element()) {
        if matches!(d.tag_name().name(), "pos" | "posList" | "coordinates") {
            // A position list inherits the srsName of its nearest ancestor that has one.
            let own = d
                .ancestors()
                .find_map(|a| attr(a, "srsName"))
                .map(str::to_string)
                .or_else(|| srs.clone());
            out.extend(positions(d, ctx, &own));
        }
    }
    out
}

fn ring_of(n: Node<'_, '_>, ctx: &Ctx, srs: &Option<String>) -> Vec<Pt> {
    collect(n, ctx, srs)
}

fn polygon(n: Node<'_, '_>, ctx: &Ctx, srs: &Option<String>, parts: &mut Vec<Part>) {
    let mut first = true;
    for c in n.children().filter(|c| c.is_element()) {
        let hole = match c.tag_name().name() {
            "exterior" | "outerBoundaryIs" => false,
            "interior" | "innerBoundaryIs" => true,
            _ => continue,
        };
        let ring = ring_of(c, ctx, srs);
        if !ring.is_empty() {
            parts.push(Part::Ring(ring, hole || !first));
            first = false;
        }
    }
}

fn geometry(n: Node<'_, '_>, ctx: &mut Ctx, srs: &Option<String>, parts: &mut Vec<Part>, depth: usize) {
    if depth > 8 {
        ctx.problems.push("geometries are nested too deep".into());
        return;
    }
    let srs = srs_of(n, srs);
    match n.tag_name().name() {
        "Point" => {
            let p = collect(n, ctx, &srs);
            if let Some(p) = p.first() {
                parts.push(Part::Point(*p));
            }
        }
        "LineString" | "Curve" | "CompositeCurve" => {
            let pts = collect(n, ctx, &srs);
            if pts.len() >= 2 {
                parts.push(Part::Line(pts));
            }
        }
        "Polygon" => polygon(n, ctx, &srs, parts),
        "Surface" => {
            for p in n.descendants().filter(|d| d.is_element() && d.tag_name().name() == "PolygonPatch") {
                polygon(p, ctx, &srs, parts);
            }
        }
        "MultiPoint" | "MultiCurve" | "MultiLineString" | "MultiSurface" | "MultiPolygon" | "MultiGeometry" => {
            for member in n.children().filter(|c| c.is_element()) {
                for g in member.children().filter(|g| is_geometry(*g)) {
                    geometry(g, ctx, &srs, parts, depth + 1);
                }
            }
        }
        _ => {}
    }
}

/// A property value: a number when it reads as one (and has no leading zero, so codes stay text).
fn value_of(text: &str) -> Value {
    let t = text.trim();
    let plain = !(t.len() > 1 && t.starts_with('0') && !t.starts_with("0.")) && !t.starts_with('+');
    if plain && let Ok(i) = t.parse::<i64>() {
        return Value::from(i);
    }
    if plain && let Ok(f) = t.parse::<f64>() && f.is_finite() {
        return Value::from(f);
    }
    match t {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => Value::String(t.to_string()),
    }
}

/// Reads a WFS response (or a GML file) with feature members.
pub fn read_gml(xml: &str) -> Result<GisData> {
    let opts = roxmltree::ParsingOptions {
        allow_dtd: false,
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(xml.trim_start_matches('\u{feff}'), opts)
        .map_err(|e| GisError::Format(format!("the answer is not XML: {e}")))?;
    let root = doc.root_element();
    if matches!(root.tag_name().name(), "ExceptionReport" | "ServiceExceptionReport") {
        let msg = root
            .descendants()
            .find(|n| n.is_element() && matches!(n.tag_name().name(), "ExceptionText" | "ServiceException"))
            .and_then(|n| n.text())
            .unwrap_or("")
            .trim()
            .to_string();
        return Err(GisError::Format(format!("the server reports an error: {msg}")));
    }
    let mut data = GisData::default();
    let default_srs = root
        .descendants()
        .find_map(|n| attr(n, "srsName"))
        .map(str::to_string);
    let mut ctx = Ctx {
        srs: default_srs.clone(),
        dim: 2,
        problems: Vec::new(),
    };
    // Members: wfs:member (2.0) or gml:featureMember (older), or the children of featureMembers.
    let members: Vec<Node<'_, '_>> = root
        .descendants()
        .filter(|n| n.is_element())
        .filter(|n| matches!(n.tag_name().name(), "member" | "featureMember" | "featureMembers"))
        .flat_map(|m| {
            if m.tag_name().name() == "featureMembers" {
                m.children().filter(|c| c.is_element()).collect::<Vec<_>>()
            } else {
                m.children().filter(|c| c.is_element()).take(1).collect()
            }
        })
        .collect();
    if members.is_empty() && !matches!(root.tag_name().name(), "FeatureCollection") {
        return Err(GisError::Format(format!(
            "this is not a feature collection (root element {})",
            root.tag_name().name()
        )));
    }
    for m in members {
        let mut parts = Vec::new();
        let mut props: BTreeMap<String, Value> = BTreeMap::new();
        if let Some(id) = attr(m, "id") {
            props.insert("_id".into(), Value::String(id.to_string()));
        }
        for p in m.children().filter(|c| c.is_element()) {
            let name = p.tag_name().name();
            if name == "boundedBy" {
                continue;
            }
            if let Some(g) = p.children().find(|g| is_geometry(*g)) {
                let srs = ctx.srs.clone();
                geometry(g, &mut ctx, &srs, &mut parts, 0);
            } else if p.children().any(|c| c.is_element()) {
                // A complex property: not a simple feature.
                continue;
            } else if let Some(t) = p.text().filter(|t| !t.trim().is_empty()) {
                props.insert(name.to_string(), value_of(t));
            }
        }
        if parts.is_empty() {
            data.skipped.push((m.tag_name().name().into(), "no geometry".into()));
        } else {
            data.features.push(Feature { parts, props });
        }
    }
    data.skipped.extend(ctx.problems.into_iter().map(|p| ("geometry".to_string(), p)));
    // The CRS of the coordinates, as `EPSG:n`, so the importer converts them.
    data.file_crs = default_srs.and_then(|s| Crs::parse(&s).ok().map(|c| c.spec));
    Ok(data)
}

#[cfg(test)]
pub(crate) mod fixtures {
    pub const BUILDINGS_3067: &str = r#"<?xml version="1.0"?>
<wfs:FeatureCollection xmlns:wfs="http://www.opengis.net/wfs/2.0" xmlns:gml="http://www.opengis.net/gml/3.2" xmlns:ns="http://example.test/ns" numberMatched="3" numberReturned="3">
 <wfs:boundedBy><gml:Envelope srsName="urn:ogc:def:crs:EPSG::3067"><gml:lowerCorner>385000 6672000</gml:lowerCorner><gml:upperCorner>385100 6672100</gml:upperCorner></gml:Envelope></wfs:boundedBy>
 <wfs:member>
  <ns:buildings gml:id="b1">
   <ns:name>House A</ns:name><ns:floors>3</ns:floors><ns:height>9.5</ns:height><ns:code>007</ns:code>
   <ns:geom><gml:Polygon srsName="urn:ogc:def:crs:EPSG::3067" gml:id="g1"><gml:exterior><gml:LinearRing><gml:posList>385010 6672010 385030 6672010 385030 6672020 385010 6672020 385010 6672010</gml:posList></gml:LinearRing></gml:exterior>
    <gml:interior><gml:LinearRing><gml:posList>385015 6672013 385020 6672013 385020 6672016 385015 6672016 385015 6672013</gml:posList></gml:LinearRing></gml:interior></gml:Polygon></ns:geom>
  </ns:buildings>
 </wfs:member>
 <wfs:member>
  <ns:buildings gml:id="b2"><ns:name>Shed</ns:name><ns:height>3</ns:height>
   <ns:geom><gml:MultiSurface srsName="urn:ogc:def:crs:EPSG::3067"><gml:surfaceMember><gml:Polygon><gml:exterior><gml:LinearRing><gml:posList>385050 6672050 385060 6672050 385060 6672060 385050 6672050</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></ns:geom>
  </ns:buildings>
 </wfs:member>
 <wfs:member>
  <ns:buildings gml:id="b3"><ns:name>Mast</ns:name><ns:geom><gml:Point srsName="urn:ogc:def:crs:EPSG::3067"><gml:pos>385070 6672070</gml:pos></gml:Point></ns:geom></ns:buildings>
 </wfs:member>
 <wfs:member><ns:buildings gml:id="b4"><ns:name>No geometry</ns:name></ns:buildings></wfs:member>
</wfs:FeatureCollection>"#;

    /// Latitude first, as the long CRS name says.
    pub const POINT_4326_LONG: &str = r#"<wfs:FeatureCollection xmlns:wfs="http://www.opengis.net/wfs/2.0" xmlns:gml="http://www.opengis.net/gml/3.2" xmlns:ns="x">
 <wfs:member><ns:p gml:id="p"><ns:g><gml:Point srsName="urn:ogc:def:crs:EPSG::4326"><gml:pos>60.17 24.94</gml:pos></gml:Point></ns:g></ns:p></wfs:member>
</wfs:FeatureCollection>"#;
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn reads_polygons_with_holes_multi_geometries_and_points() {
        let d = read_gml(BUILDINGS_3067).unwrap();
        assert_eq!(d.file_crs.as_deref(), Some("EPSG:3067"));
        assert_eq!(d.features.len(), 3);
        assert_eq!(d.skipped.len(), 1);
        let f = &d.features[0];
        assert_eq!(f.parts.len(), 2);
        assert!(matches!(&f.parts[0], Part::Ring(r, false) if r.len() == 5));
        assert!(matches!(&f.parts[1], Part::Ring(r, true) if r.len() == 5));
        assert_eq!(f.props["name"], Value::from("House A"));
        assert_eq!(f.props["floors"], Value::from(3));
        assert_eq!(f.props["height"], Value::from(9.5));
        // A code with leading zeros stays text.
        assert_eq!(f.props["code"], Value::from("007"));
        assert_eq!(f.props["_id"], Value::from("b1"));
        assert!(matches!(&d.features[1].parts[0], Part::Ring(_, false)));
        assert!(matches!(&d.features[2].parts[0], Part::Point(p) if p.x == 385_070.0));
    }

    #[test]
    fn follows_the_axis_order_of_the_crs_name() {
        let d = read_gml(POINT_4326_LONG).unwrap();
        assert_eq!(d.file_crs.as_deref(), Some("EPSG:4326"));
        match &d.features[0].parts[0] {
            Part::Point(p) => assert_eq!((p.x, p.y), (24.94, 60.17)),
            other => panic!("{other:?}"),
        }
        let short = POINT_4326_LONG.replace("urn:ogc:def:crs:EPSG::4326", "EPSG:4326").replace("60.17 24.94", "24.94 60.17");
        match &read_gml(&short).unwrap().features[0].parts[0] {
            Part::Point(p) => assert_eq!((p.x, p.y), (24.94, 60.17)),
            other => panic!("{other:?}"),
        }
        assert!(y_first("urn:ogc:def:crs:EPSG::4326"));
        assert!(y_first("http://www.opengis.net/def/crs/EPSG/0/4326"));
        assert!(!y_first("EPSG:4326"));
        assert!(!y_first("urn:ogc:def:crs:EPSG::3067"));
        assert!(y_first("urn:ogc:def:crs:EPSG::3035"));
        assert!(!y_first("urn:ogc:def:crs:nonsense"));
    }

    #[test]
    fn reads_other_geometry_forms() {
        let xml = r#"<gml:FeatureCollection xmlns:gml="http://www.opengis.net/gml/3.2" xmlns:ns="x">
 <gml:featureMember><ns:r gml:id="r"><ns:g><gml:LineString srsName="EPSG:3067"><gml:posList srsDimension="3">1 2 3 4 5 6</gml:posList></gml:LineString></ns:g><ns:note>true</ns:note></ns:r></gml:featureMember>
 <gml:featureMember><ns:r><ns:g><gml:Curve srsName="EPSG:3067"><gml:segments><gml:LineStringSegment><gml:posList>0 0 1 1</gml:posList></gml:LineStringSegment></gml:segments></gml:Curve></ns:g></ns:r></gml:featureMember>
 <gml:featureMember><ns:r><ns:g><gml:Surface srsName="EPSG:3067"><gml:patches><gml:PolygonPatch><gml:exterior><gml:LinearRing><gml:posList>0 0 4 0 4 4 0 0</gml:posList></gml:LinearRing></gml:exterior></gml:PolygonPatch></gml:patches></gml:Surface></ns:g></ns:r></gml:featureMember>
 <gml:featureMember><ns:r><ns:g><gml:MultiPoint srsName="EPSG:3067"><gml:pointMember><gml:Point><gml:pos>1 1</gml:pos></gml:Point></gml:pointMember><gml:pointMember><gml:Point><gml:pos>2 2</gml:pos></gml:Point></gml:pointMember></gml:MultiPoint></ns:g></ns:r></gml:featureMember>
 <gml:featureMember><ns:r><ns:g><gml:Polygon srsName="EPSG:3067"><gml:outerBoundaryIs><gml:LinearRing><gml:coordinates>0,0 5,0 5,5 0,0</gml:coordinates></gml:LinearRing></gml:outerBoundaryIs></gml:Polygon></ns:g></ns:r></gml:featureMember>
</gml:FeatureCollection>"#;
        let d = read_gml(xml).unwrap();
        assert_eq!(d.features.len(), 5);
        assert!(matches!(&d.features[0].parts[0], Part::Line(l) if l.len() == 2 && l[1].z == Some(6.0)));
        assert_eq!(d.features[0].props["note"], Value::Bool(true));
        assert!(matches!(&d.features[1].parts[0], Part::Line(l) if l.len() == 2));
        assert!(matches!(&d.features[2].parts[0], Part::Ring(r, false) if r.len() == 4));
        assert_eq!(d.features[3].parts.len(), 2);
        assert!(matches!(&d.features[4].parts[0], Part::Ring(r, false) if r.len() == 4));
    }

    #[test]
    fn reports_problems() {
        assert!(read_gml("nonsense").is_err());
        assert!(read_gml("<html/>").unwrap_err().to_string().contains("not a feature collection"));
        let e = read_gml(r#"<ows:ExceptionReport xmlns:ows="x"><ows:Exception><ows:ExceptionText>Unknown type</ows:ExceptionText></ows:Exception></ows:ExceptionReport>"#)
            .unwrap_err()
            .to_string();
        assert!(e.contains("Unknown type"));
        // A collection with no members is fine.
        let empty = read_gml(r#"<wfs:FeatureCollection xmlns:wfs="http://www.opengis.net/wfs/2.0"/>"#).unwrap();
        assert!(empty.features.is_empty());
        // Geometry nested without end.
        let mut nested = String::from("<a xmlns:gml='g'>");
        let mut inner = String::from("<gml:Point><gml:pos>1 1</gml:pos></gml:Point>");
        for _ in 0..12 {
            inner = format!("<gml:MultiGeometry><gml:geometryMember>{inner}</gml:geometryMember></gml:MultiGeometry>");
        }
        nested.push_str(&format!("<gml:featureMember><r><g>{inner}</g></r></gml:featureMember></a>"));
        let d = read_gml(&nested).unwrap();
        assert!(d.features.is_empty());
        assert!(!d.skipped.is_empty());
        // A short list of numbers is ignored without a panic.
        let odd = r#"<c xmlns:gml="g"><gml:featureMember><r><g><gml:LineString><gml:posList>1 2 3</gml:posList></gml:LineString></g></r></gml:featureMember></c>"#;
        assert!(read_gml(odd).unwrap().features.is_empty());
        // Values.
        assert_eq!(value_of("12"), Value::from(12));
        assert_eq!(value_of("1.5"), Value::from(1.5));
        assert_eq!(value_of("0.5"), Value::from(0.5));
        assert_eq!(value_of("+5"), Value::from("+5"));
        assert_eq!(value_of("false"), Value::Bool(false));
        assert_eq!(value_of("abc"), Value::from("abc"));
        assert_eq!(value_of("NaN"), Value::from("NaN"));
    }
}
