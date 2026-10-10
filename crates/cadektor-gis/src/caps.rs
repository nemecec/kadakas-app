//! GetCapabilities documents of WMS (1.3.0 and 1.1.1), WMTS 1.0.0 and WFS 2.0.
//!
//! The readers look at the local names of the elements and ignore XML namespaces, because servers
//! use different prefixes. They collect what the app needs for a layer picker: layer names and titles,
//! CRSs, image formats, tile matrix sets and feature types.

use cadektor_core::DVec2;
use roxmltree::{Document, Node};
use serde::Serialize;

use crate::tiles::{Bbox, TileMatrix, northing_first, xy_from_text};
use crate::{GisError, Result};

fn children<'a, 'i>(n: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children()
        .filter(move |c| c.is_element() && c.tag_name().name() == name)
}

fn child<'a, 'i>(n: Node<'a, 'i>, name: &'a str) -> Option<Node<'a, 'i>> {
    children(n, name).next()
}

fn text(n: Node<'_, '_>) -> String {
    n.text().unwrap_or("").trim().to_string()
}

fn child_text(n: Node<'_, '_>, name: &str) -> Option<String> {
    child(n, name).map(text).filter(|t| !t.is_empty())
}

fn attr<'a>(n: Node<'a, '_>, name: &str) -> Option<&'a str> {
    n.attributes().find(|a| a.name() == name).map(|a| a.value())
}

fn numbers(s: &str) -> Vec<f64> {
    s.split_whitespace()
        .filter_map(|t| t.parse::<f64>().ok())
        .collect()
}

/// Removes the DOCTYPE (with an internal subset in brackets). Real WMS 1.1.1 documents carry one
/// that names a DTD on the server. The parser never reads a DTD and never expands entities, so
/// dropping the declaration is safe: a document that uses an entity then fails to parse.
pub fn strip_doctype(xml: &str) -> std::borrow::Cow<'_, str> {
    let Some(start) = xml.find("<!DOCTYPE") else {
        return xml.into();
    };
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    for (i, c) in xml[start..].char_indices() {
        match (c, quote) {
            ('"' | '\'', None) => quote = Some(c),
            (q, Some(open)) if q == open => quote = None,
            ('[', None) => depth += 1,
            (']', None) => depth = depth.saturating_sub(1),
            ('>', None) if depth == 0 => {
                let end = start + i + 1;
                return format!("{}{}", &xml[..start], &xml[end..]).into();
            }
            _ => {}
        }
    }
    xml.into()
}

fn parse_doc(xml: &str) -> Result<Document<'_>> {
    let opts = roxmltree::ParsingOptions {
        allow_dtd: false,
        ..Default::default()
    };
    let doc = Document::parse_with_options(xml.trim_start_matches('\u{feff}'), opts)
        .map_err(|e| GisError::Format(format!("not an XML document: {e}")))?;
    let root = doc.root_element();
    let name = root.tag_name().name();
    if name == "ServiceExceptionReport" || name == "ExceptionReport" {
        let msg = root
            .descendants()
            .find(|n| {
                n.is_element()
                    && matches!(n.tag_name().name(), "ServiceException" | "ExceptionText")
            })
            .map(text)
            .unwrap_or_default();
        return Err(GisError::Format(format!(
            "the server reports an error: {msg}"
        )));
    }
    Ok(doc)
}

/// What a capabilities document is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceKind {
    Wms,
    Wmts,
    Wfs,
}

/// Finds out which service type a capabilities document is for.
pub fn detect(xml: &str) -> Result<ServiceKind> {
    let xml = strip_doctype(xml);
    let doc = parse_doc(&xml)?;
    match doc.root_element().tag_name().name() {
        "WMS_Capabilities" | "WMT_MS_Capabilities" => Ok(ServiceKind::Wms),
        "WFS_Capabilities" => Ok(ServiceKind::Wfs),
        "Capabilities" => Ok(ServiceKind::Wmts),
        other => Err(GisError::Format(format!(
            "this is not a capabilities document (root element {other})"
        ))),
    }
}

// ---------------------------------------------------------------- WMS

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WmsLayer {
    /// `None` for a group that cannot be requested.
    pub name: Option<String>,
    pub title: String,
    pub abstract_text: String,
    /// CRSs of the layer, including those of its parents.
    pub crs: Vec<String>,
    /// West, south, east, north in degrees.
    pub geographic_bbox: Option<[f64; 4]>,
    /// Boxes per CRS: (crs, [minx, miny, maxx, maxy]) as written in the document.
    pub bboxes: Vec<(String, [f64; 4])>,
    /// Style names.
    pub styles: Vec<String>,
    pub depth: usize,
    pub opaque: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WmsCaps {
    pub version: String,
    pub title: String,
    /// Address for GetMap requests, from the document.
    pub getmap_url: Option<String>,
    /// Image formats of GetMap.
    pub formats: Vec<String>,
    /// All layers in document order, parents before children.
    pub layers: Vec<WmsLayer>,
}

impl WmsCaps {
    /// A layer by name.
    pub fn layer(&self, name: &str) -> Option<&WmsLayer> {
        self.layers.iter().find(|l| l.name.as_deref() == Some(name))
    }

    /// The CRSs that all the named layers support.
    pub fn common_crs(&self, names: &[String]) -> Vec<String> {
        let mut out: Option<Vec<String>> = None;
        for n in names {
            let Some(l) = self.layer(n) else { continue };
            out = Some(match out {
                None => l.crs.clone(),
                Some(o) => o.into_iter().filter(|c| l.crs.contains(c)).collect(),
            });
        }
        out.unwrap_or_default()
    }

    /// The best image format: PNG first (it can be clear), then JPEG.
    pub fn pick_format(&self) -> Option<String> {
        ["image/png", "image/jpeg", "image/png8", "image/gif"]
            .iter()
            .find(|f| self.formats.iter().any(|x| x.eq_ignore_ascii_case(f)))
            .map(|f| f.to_string())
            .or_else(|| {
                self.formats
                    .iter()
                    .find(|f| f.to_ascii_lowercase().starts_with("image/"))
                    .cloned()
            })
    }
}

fn read_wms_layers(n: Node<'_, '_>, inherited: &[String], depth: usize, out: &mut Vec<WmsLayer>) {
    let mut crs = inherited.to_vec();
    for c in n.children().filter(|c| c.is_element()) {
        // 1.3.0 has CRS, 1.1.1 has SRS (maybe a list).
        if matches!(c.tag_name().name(), "CRS" | "SRS") {
            for t in text(c).split_whitespace() {
                if !crs.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                    crs.push(t.to_ascii_uppercase());
                }
            }
        }
    }
    let geographic_bbox = child(n, "EX_GeographicBoundingBox")
        .and_then(|g| {
            let v = |k| child_text(g, k)?.parse::<f64>().ok();
            Some([
                v("westBoundLongitude")?,
                v("southBoundLatitude")?,
                v("eastBoundLongitude")?,
                v("northBoundLatitude")?,
            ])
        })
        .or_else(|| {
            let b = child(n, "LatLonBoundingBox")?;
            let v = |k| attr(b, k)?.parse::<f64>().ok();
            Some([v("minx")?, v("miny")?, v("maxx")?, v("maxy")?])
        });
    let bboxes = children(n, "BoundingBox")
        .filter_map(|b| {
            let crs = attr(b, "CRS")
                .or_else(|| attr(b, "SRS"))?
                .to_ascii_uppercase();
            let v = |k| attr(b, k)?.parse::<f64>().ok();
            Some((crs, [v("minx")?, v("miny")?, v("maxx")?, v("maxy")?]))
        })
        .collect();
    out.push(WmsLayer {
        name: child_text(n, "Name"),
        title: child_text(n, "Title").unwrap_or_default(),
        abstract_text: child_text(n, "Abstract").unwrap_or_default(),
        crs: crs.clone(),
        geographic_bbox,
        bboxes,
        styles: children(n, "Style")
            .filter_map(|s| child_text(s, "Name"))
            .collect(),
        depth,
        opaque: attr(n, "opaque").is_some_and(|v| v == "1" || v == "true"),
    });
    for l in children(n, "Layer") {
        read_wms_layers(l, &crs, depth + 1, out);
    }
}

/// Reads a WMS capabilities document.
pub fn parse_wms(xml: &str) -> Result<WmsCaps> {
    let xml = strip_doctype(xml);
    let doc = parse_doc(&xml)?;
    let root = doc.root_element();
    if !matches!(
        root.tag_name().name(),
        "WMS_Capabilities" | "WMT_MS_Capabilities"
    ) {
        return Err(GisError::Format(format!(
            "this is not a WMS capabilities document (root element {})",
            root.tag_name().name()
        )));
    }
    let cap = child(root, "Capability")
        .ok_or_else(|| GisError::Format("the WMS document has no Capability element".into()))?;
    let getmap = child(cap, "Request").and_then(|r| child(r, "GetMap"));
    let formats = getmap
        .map(|g| children(g, "Format").map(text).collect())
        .unwrap_or_default();
    let getmap_url = getmap.and_then(|g| {
        g.descendants()
            .find(|d| d.is_element() && d.tag_name().name() == "OnlineResource")
            .and_then(|o| attr(o, "href"))
            .map(str::to_string)
    });
    let mut layers = Vec::new();
    if let Some(top) = child(cap, "Layer") {
        read_wms_layers(top, &[], 0, &mut layers);
    }
    Ok(WmsCaps {
        version: attr(root, "version").unwrap_or("1.3.0").to_string(),
        title: child(root, "Service")
            .and_then(|s| child_text(s, "Title"))
            .unwrap_or_default(),
        getmap_url,
        formats,
        layers,
    })
}

// ---------------------------------------------------------------- WMTS

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResourceUrl {
    pub format: String,
    pub template: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WmtsLayer {
    pub id: String,
    pub title: String,
    pub formats: Vec<String>,
    pub styles: Vec<String>,
    pub default_style: Option<String>,
    pub matrix_sets: Vec<String>,
    pub resource_urls: Vec<ResourceUrl>,
    /// West, south, east, north in degrees.
    pub wgs84_bbox: Option<[f64; 4]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatrixSet {
    pub id: String,
    pub crs: String,
    pub matrices: Vec<TileMatrix>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WmtsCaps {
    pub title: String,
    /// Address of the KVP GetTile requests, from the document.
    pub kvp_url: Option<String>,
    pub layers: Vec<WmtsLayer>,
    pub matrix_sets: Vec<MatrixSet>,
}

impl WmtsCaps {
    pub fn layer(&self, id: &str) -> Option<&WmtsLayer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn matrix_set(&self, id: &str) -> Option<&MatrixSet> {
        self.matrix_sets.iter().find(|m| m.id == id)
    }
}

fn pair(s: &str) -> Option<(f64, f64)> {
    let v = numbers(s);
    (v.len() >= 2).then(|| (v[0], v[1]))
}

/// Reads a WMTS 1.0.0 capabilities document.
pub fn parse_wmts(xml: &str) -> Result<WmtsCaps> {
    let xml = strip_doctype(xml);
    let doc = parse_doc(&xml)?;
    let root = doc.root_element();
    if root.tag_name().name() != "Capabilities" {
        return Err(GisError::Format(format!(
            "this is not a WMTS capabilities document (root element {})",
            root.tag_name().name()
        )));
    }
    let contents = child(root, "Contents")
        .ok_or_else(|| GisError::Format("the WMTS document has no Contents element".into()))?;
    let layers = children(contents, "Layer")
        .map(|l| {
            let styles: Vec<(String, bool)> = children(l, "Style")
                .filter_map(|s| {
                    Some((
                        child_text(s, "Identifier")?,
                        attr(s, "isDefault").is_some_and(|v| v == "true" || v == "1"),
                    ))
                })
                .collect();
            WmtsLayer {
                id: child_text(l, "Identifier").unwrap_or_default(),
                title: child_text(l, "Title").unwrap_or_default(),
                formats: children(l, "Format").map(text).collect(),
                default_style: styles
                    .iter()
                    .find(|s| s.1)
                    .or(styles.first())
                    .map(|s| s.0.clone()),
                styles: styles.into_iter().map(|s| s.0).collect(),
                matrix_sets: children(l, "TileMatrixSetLink")
                    .filter_map(|t| child_text(t, "TileMatrixSet"))
                    .collect(),
                resource_urls: children(l, "ResourceURL")
                    .filter(|r| attr(*r, "resourceType").is_none_or(|t| t == "tile"))
                    .filter_map(|r| {
                        Some(ResourceUrl {
                            format: attr(r, "format")?.to_string(),
                            template: attr(r, "template")?.to_string(),
                        })
                    })
                    .collect(),
                wgs84_bbox: child(l, "WGS84BoundingBox").and_then(|b| {
                    let lo = pair(&child_text(b, "LowerCorner")?)?;
                    let hi = pair(&child_text(b, "UpperCorner")?)?;
                    Some([lo.0, lo.1, hi.0, hi.1])
                }),
            }
        })
        .filter(|l| !l.id.is_empty())
        .collect();
    let matrix_sets = children(contents, "TileMatrixSet")
        .map(|s| {
            let crs = child_text(s, "SupportedCRS").unwrap_or_default();
            let code = cadektor_io::crs::Crs::parse(&crs).ok();
            let y_first = code
                .as_ref()
                .is_some_and(|c| northing_first(c.epsg(), c.is_geographic()));
            MatrixSet {
                id: child_text(s, "Identifier").unwrap_or_default(),
                matrices: children(s, "TileMatrix")
                    .filter_map(|m| {
                        let corner = pair(&child_text(m, "TopLeftCorner")?)?;
                        Some(TileMatrix {
                            id: child_text(m, "Identifier")?,
                            scale_denominator: child_text(m, "ScaleDenominator")?.parse().ok()?,
                            top_left: xy_from_text(corner.0, corner.1, y_first),
                            tile_width: child_text(m, "TileWidth")?.parse().ok()?,
                            tile_height: child_text(m, "TileHeight")?.parse().ok()?,
                            matrix_width: child_text(m, "MatrixWidth")?.parse().ok()?,
                            matrix_height: child_text(m, "MatrixHeight")?.parse().ok()?,
                        })
                    })
                    .collect(),
                crs,
            }
        })
        .filter(|s| !s.id.is_empty() && !s.matrices.is_empty())
        .collect();
    let kvp_url = child(root, "OperationsMetadata").and_then(|om| {
        children(om, "Operation")
            .find(|o| attr(*o, "name") == Some("GetTile"))?
            .descendants()
            .find(|d| d.is_element() && d.tag_name().name() == "Get")
            .and_then(|g| attr(g, "href"))
            .map(str::to_string)
    });
    Ok(WmtsCaps {
        title: child(root, "ServiceIdentification")
            .and_then(|s| child_text(s, "Title"))
            .unwrap_or_default(),
        kvp_url,
        layers,
        matrix_sets,
    })
}

// ---------------------------------------------------------------- WFS

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FeatureType {
    pub name: String,
    pub title: String,
    pub default_crs: String,
    pub other_crs: Vec<String>,
    /// West, south, east, north in degrees.
    pub wgs84_bbox: Option<[f64; 4]>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WfsCaps {
    pub version: String,
    pub title: String,
    pub getfeature_url: Option<String>,
    /// Allowed values of `outputFormat` for GetFeature (maybe empty).
    pub output_formats: Vec<String>,
    pub feature_types: Vec<FeatureType>,
}

impl WfsCaps {
    pub fn feature_type(&self, name: &str) -> Option<&FeatureType> {
        self.feature_types.iter().find(|f| f.name == name)
    }

    /// The output format to ask for: GeoJSON if the server lists it, else GML 3.2.
    pub fn pick_format(&self) -> &'static str {
        let has = |p: &str| {
            self.output_formats
                .iter()
                .any(|f| f.to_ascii_lowercase().contains(p))
        };
        if has("json") {
            "application/json"
        } else {
            "application/gml+xml; version=3.2"
        }
    }

    /// The JSON output format string the server itself lists, if any.
    pub fn json_format(&self) -> Option<&str> {
        self.output_formats
            .iter()
            .find(|f| f.to_ascii_lowercase().contains("json"))
            .map(String::as_str)
    }
}

/// Reads a WFS 2.0 (or 1.1) capabilities document.
pub fn parse_wfs(xml: &str) -> Result<WfsCaps> {
    let xml = strip_doctype(xml);
    let doc = parse_doc(&xml)?;
    let root = doc.root_element();
    if root.tag_name().name() != "WFS_Capabilities" {
        return Err(GisError::Format(format!(
            "this is not a WFS capabilities document (root element {})",
            root.tag_name().name()
        )));
    }
    let op = child(root, "OperationsMetadata")
        .and_then(|om| children(om, "Operation").find(|o| attr(*o, "name") == Some("GetFeature")));
    let output_formats = op
        .and_then(|o| {
            children(o, "Parameter")
                .find(|p| attr(*p, "name") == Some("outputFormat"))
                .map(|p| {
                    p.descendants()
                        .filter(|d| d.is_element() && d.tag_name().name() == "Value")
                        .map(text)
                        .collect::<Vec<_>>()
                })
        })
        .unwrap_or_default();
    let getfeature_url = op.and_then(|o| {
        o.descendants()
            .find(|d| d.is_element() && d.tag_name().name() == "Get")
            .and_then(|g| attr(g, "href"))
            .map(str::to_string)
    });
    let feature_types = child(root, "FeatureTypeList")
        .map(|l| {
            children(l, "FeatureType")
                .filter_map(|f| {
                    Some(FeatureType {
                        name: child_text(f, "Name")?,
                        title: child_text(f, "Title").unwrap_or_default(),
                        default_crs: child_text(f, "DefaultCRS")
                            .or_else(|| child_text(f, "DefaultSRS"))
                            .unwrap_or_default(),
                        other_crs: children(f, "OtherCRS")
                            .chain(children(f, "OtherSRS"))
                            .map(text)
                            .collect(),
                        wgs84_bbox: child(f, "WGS84BoundingBox").and_then(|b| {
                            let lo = pair(&child_text(b, "LowerCorner")?)?;
                            let hi = pair(&child_text(b, "UpperCorner")?)?;
                            Some([lo.0, lo.1, hi.0, hi.1])
                        }),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(WfsCaps {
        version: attr(root, "version").unwrap_or("2.0.0").to_string(),
        title: child(root, "ServiceIdentification")
            .and_then(|s| child_text(s, "Title"))
            .unwrap_or_default(),
        getfeature_url,
        output_formats,
        feature_types,
    })
}

/// The box of a layer in the CRS `epsg`: from its own box in that CRS, else `None`.
pub fn layer_bbox_in(l: &WmsLayer, crs: &str) -> Option<Bbox> {
    l.bboxes
        .iter()
        .find(|(c, _)| c.eq_ignore_ascii_case(crs))
        .map(|(_, b)| Bbox::new(DVec2::new(b[0], b[1]), DVec2::new(b[2], b[3])))
}

#[cfg(test)]
pub(crate) mod fixtures {
    pub const WMS_130: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<WMS_Capabilities version="1.3.0" xmlns="http://www.opengis.net/wms" xmlns:xlink="http://www.w3.org/1999/xlink">
 <Service><Name>WMS</Name><Title>Test map server</Title></Service>
 <Capability>
  <Request>
   <GetCapabilities><Format>text/xml</Format></GetCapabilities>
   <GetMap>
    <Format>image/jpeg</Format><Format>image/png</Format>
    <DCPType><HTTP><Get><OnlineResource xlink:type="simple" xlink:href="http://example.test/wms?"/></Get></HTTP></DCPType>
   </GetMap>
  </Request>
  <Layer>
   <Title>Root</Title>
   <CRS>EPSG:4326</CRS><CRS>EPSG:3857</CRS>
   <EX_GeographicBoundingBox><westBoundLongitude>19</westBoundLongitude><eastBoundLongitude>32</eastBoundLongitude><southBoundLatitude>59</southBoundLatitude><northBoundLatitude>70</northBoundLatitude></EX_GeographicBoundingBox>
   <BoundingBox CRS="EPSG:3857" minx="2100000" miny="8300000" maxx="3500000" maxy="11000000"/>
   <Layer queryable="1"><Name>ortho</Name><Title>Orthophoto</Title><Abstract>Aerial</Abstract><CRS>EPSG:3067</CRS>
     <Style><Name>default</Name><Title>Default</Title></Style></Layer>
   <Layer><Name>roads</Name><Title>Roads</Title><Layer opaque="1"><Title>Group child</Title></Layer></Layer>
  </Layer>
 </Capability>
</WMS_Capabilities>"#;

    pub const WMS_111: &str = r#"<?xml version="1.0"?>
<WMT_MS_Capabilities version="1.1.1">
 <Service><Name>OGC:WMS</Name><Title>Old server</Title></Service>
 <Capability>
  <Request><GetMap><Format>image/gif</Format><DCPType><HTTP><Get><OnlineResource xmlns:xlink="http://www.w3.org/1999/xlink" xlink:href="http://old.test/cgi?map=a"/></Get></HTTP></DCPType></GetMap></Request>
  <Layer>
   <SRS>EPSG:4326 EPSG:3857</SRS>
   <LatLonBoundingBox minx="-10" miny="35" maxx="30" maxy="60"/>
   <Layer><Name>countries</Name><Title>Countries</Title><BoundingBox SRS="EPSG:4326" minx="-10" miny="35" maxx="30" maxy="60"/></Layer>
  </Layer>
 </Capability>
</WMT_MS_Capabilities>"#;

    pub const WMTS: &str = r#"<?xml version="1.0"?>
<Capabilities xmlns="http://www.opengis.net/wmts/1.0" xmlns:ows="http://www.opengis.net/ows/1.1" xmlns:xlink="http://www.w3.org/1999/xlink" version="1.0.0">
 <ows:ServiceIdentification><ows:Title>Tile server</ows:Title></ows:ServiceIdentification>
 <ows:OperationsMetadata>
  <ows:Operation name="GetTile"><ows:DCP><ows:HTTP><ows:Get xlink:href="http://tiles.test/kvp?"/></ows:HTTP></ows:DCP></ows:Operation>
 </ows:OperationsMetadata>
 <Contents>
  <Layer>
   <ows:Title>Base map</ows:Title><ows:Identifier>base</ows:Identifier>
   <ows:WGS84BoundingBox><ows:LowerCorner>-180 -85</ows:LowerCorner><ows:UpperCorner>180 85</ows:UpperCorner></ows:WGS84BoundingBox>
   <Style isDefault="true"><ows:Identifier>normal</ows:Identifier></Style>
   <Format>image/png</Format>
   <TileMatrixSetLink><TileMatrixSet>WebMercator</TileMatrixSet></TileMatrixSetLink>
   <ResourceURL format="image/png" resourceType="tile" template="http://tiles.test/{TileMatrixSet}/{TileMatrix}/{TileCol}/{TileRow}.png"/>
  </Layer>
  <TileMatrixSet>
   <ows:Identifier>WebMercator</ows:Identifier><ows:SupportedCRS>urn:ogc:def:crs:EPSG::3857</ows:SupportedCRS>
   <TileMatrix><ows:Identifier>0</ows:Identifier><ScaleDenominator>559082264.0287178</ScaleDenominator><TopLeftCorner>-20037508.342789244 20037508.342789244</TopLeftCorner><TileWidth>256</TileWidth><TileHeight>256</TileHeight><MatrixWidth>1</MatrixWidth><MatrixHeight>1</MatrixHeight></TileMatrix>
   <TileMatrix><ows:Identifier>1</ows:Identifier><ScaleDenominator>279541132.0143589</ScaleDenominator><TopLeftCorner>-20037508.342789244 20037508.342789244</TopLeftCorner><TileWidth>256</TileWidth><TileHeight>256</TileHeight><MatrixWidth>2</MatrixWidth><MatrixHeight>2</MatrixHeight></TileMatrix>
  </TileMatrixSet>
  <TileMatrixSet>
   <ows:Identifier>Geo</ows:Identifier><ows:SupportedCRS>urn:ogc:def:crs:EPSG::4326</ows:SupportedCRS>
   <TileMatrix><ows:Identifier>0</ows:Identifier><ScaleDenominator>279541132.0143589</ScaleDenominator><TopLeftCorner>90 -180</TopLeftCorner><TileWidth>256</TileWidth><TileHeight>256</TileHeight><MatrixWidth>2</MatrixWidth><MatrixHeight>1</MatrixHeight></TileMatrix>
  </TileMatrixSet>
 </Contents>
</Capabilities>"#;

    pub const WFS: &str = r#"<?xml version="1.0"?>
<wfs:WFS_Capabilities version="2.0.0" xmlns:wfs="http://www.opengis.net/wfs/2.0" xmlns:ows="http://www.opengis.net/ows/1.1" xmlns:xlink="http://www.w3.org/1999/xlink">
 <ows:ServiceIdentification><ows:Title>Feature server</ows:Title></ows:ServiceIdentification>
 <ows:OperationsMetadata>
  <ows:Operation name="GetFeature">
   <ows:DCP><ows:HTTP><ows:Get xlink:href="http://features.test/wfs?"/></ows:HTTP></ows:DCP>
   <ows:Parameter name="outputFormat"><ows:AllowedValues><ows:Value>application/gml+xml; version=3.2</ows:Value><ows:Value>application/json</ows:Value></ows:AllowedValues></ows:Parameter>
  </ows:Operation>
 </ows:OperationsMetadata>
 <wfs:FeatureTypeList>
  <wfs:FeatureType><wfs:Name>ns:buildings</wfs:Name><wfs:Title>Buildings</wfs:Title><wfs:DefaultCRS>urn:ogc:def:crs:EPSG::3067</wfs:DefaultCRS><wfs:OtherCRS>urn:ogc:def:crs:EPSG::4326</wfs:OtherCRS>
   <ows:WGS84BoundingBox><ows:LowerCorner>19.0 59.0</ows:LowerCorner><ows:UpperCorner>32.0 70.0</ows:UpperCorner></ows:WGS84BoundingBox></wfs:FeatureType>
  <wfs:FeatureType><wfs:Name>ns:trees</wfs:Name></wfs:FeatureType>
  <wfs:FeatureType><wfs:Title>no name</wfs:Title></wfs:FeatureType>
 </wfs:FeatureTypeList>
</wfs:WFS_Capabilities>"#;
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn reads_wms_130() {
        let c = parse_wms(WMS_130).unwrap();
        assert_eq!(c.version, "1.3.0");
        assert_eq!(c.title, "Test map server");
        assert_eq!(c.getmap_url.as_deref(), Some("http://example.test/wms?"));
        assert_eq!(c.formats, vec!["image/jpeg", "image/png"]);
        assert_eq!(c.pick_format().as_deref(), Some("image/png"));
        assert_eq!(c.layers.len(), 4);
        let ortho = c.layer("ortho").unwrap();
        assert_eq!(ortho.title, "Orthophoto");
        assert_eq!(ortho.abstract_text, "Aerial");
        assert_eq!(ortho.crs, vec!["EPSG:4326", "EPSG:3857", "EPSG:3067"]);
        assert_eq!(ortho.geographic_bbox, None);
        assert_eq!(ortho.styles, vec!["default"]);
        assert_eq!(ortho.depth, 1);
        assert_eq!(c.layers[0].geographic_bbox, Some([19.0, 59.0, 32.0, 70.0]));
        assert_eq!(
            layer_bbox_in(&c.layers[0], "epsg:3857").unwrap().min.x,
            2_100_000.0
        );
        assert!(layer_bbox_in(&c.layers[0], "EPSG:1").is_none());
        assert!(c.layers[0].name.is_none());
        assert!(c.layers[3].opaque);
        assert_eq!(
            c.common_crs(&["ortho".into(), "roads".into(), "missing".into()]),
            vec!["EPSG:4326", "EPSG:3857"]
        );
        assert!(c.common_crs(&[]).is_empty());
        assert_eq!(detect(WMS_130).unwrap(), ServiceKind::Wms);
    }

    #[test]
    fn reads_wms_111() {
        let c = parse_wms(WMS_111).unwrap();
        assert_eq!(c.version, "1.1.1");
        assert_eq!(c.getmap_url.as_deref(), Some("http://old.test/cgi?map=a"));
        let l = c.layer("countries").unwrap();
        assert_eq!(l.crs, vec!["EPSG:4326", "EPSG:3857"]);
        assert_eq!(c.layers[0].geographic_bbox, Some([-10.0, 35.0, 30.0, 60.0]));
        assert_eq!(l.bboxes[0].0, "EPSG:4326");
        // Only GIF: the format is still offered.
        assert_eq!(c.pick_format().as_deref(), Some("image/gif"));
        let none = WmsCaps {
            formats: vec!["application/x".into(), "image/tiff".into()],
            ..c
        };
        assert_eq!(none.pick_format().as_deref(), Some("image/tiff"));
    }

    #[test]
    fn reads_wmts() {
        let c = parse_wmts(WMTS).unwrap();
        assert_eq!(c.title, "Tile server");
        assert_eq!(c.kvp_url.as_deref(), Some("http://tiles.test/kvp?"));
        let l = c.layer("base").unwrap();
        assert_eq!(l.default_style.as_deref(), Some("normal"));
        assert_eq!(l.formats, vec!["image/png"]);
        assert_eq!(l.matrix_sets, vec!["WebMercator"]);
        assert_eq!(l.resource_urls.len(), 1);
        assert_eq!(l.wgs84_bbox, Some([-180.0, -85.0, 180.0, 85.0]));
        let s = c.matrix_set("WebMercator").unwrap();
        assert_eq!(s.matrices.len(), 2);
        assert_eq!(s.matrices[1].matrix_width, 2);
        assert!((s.matrices[0].top_left.y - 20_037_508.342_789_244).abs() < 1e-6);
        // The geographic set lists the corner as latitude, longitude. It is turned to x, y.
        let g = c.matrix_set("Geo").unwrap();
        assert_eq!(g.matrices[0].top_left, DVec2::new(-180.0, 90.0));
        assert!(c.layer("x").is_none() && c.matrix_set("x").is_none());
        assert_eq!(detect(WMTS).unwrap(), ServiceKind::Wmts);
    }

    #[test]
    fn reads_wfs() {
        let c = parse_wfs(WFS).unwrap();
        assert_eq!(c.version, "2.0.0");
        assert_eq!(
            c.getfeature_url.as_deref(),
            Some("http://features.test/wfs?")
        );
        assert_eq!(c.feature_types.len(), 2);
        let b = c.feature_type("ns:buildings").unwrap();
        assert_eq!(b.default_crs, "urn:ogc:def:crs:EPSG::3067");
        assert_eq!(b.other_crs.len(), 1);
        assert_eq!(b.wgs84_bbox, Some([19.0, 59.0, 32.0, 70.0]));
        assert_eq!(c.pick_format(), "application/json");
        assert_eq!(c.json_format(), Some("application/json"));
        let gml_only = WfsCaps {
            output_formats: vec!["text/xml; subtype=gml/3.2".into()],
            ..c.clone()
        };
        assert_eq!(gml_only.pick_format(), "application/gml+xml; version=3.2");
        assert_eq!(gml_only.json_format(), None);
        assert!(c.feature_type("none").is_none());
        assert_eq!(detect(WFS).unwrap(), ServiceKind::Wfs);
    }

    #[test]
    fn real_wms_111_documents_with_a_doctype_are_read() {
        let doc = format!(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE WMT_MS_Capabilities SYSTEM \"http://schemas.opengis.net/wms/1.1.1/WMS_MS_Capabilities.dtd\" [\n <!ELEMENT VendorSpecificCapabilities EMPTY>\n]>\n{}",
            WMS_111.split_once("?>").unwrap().1
        );
        let c = parse_wms(&doc).unwrap();
        assert!(c.layer("countries").is_some());
        assert_eq!(detect(&doc).unwrap(), ServiceKind::Wms);
        assert_eq!(strip_doctype("<a/>"), "<a/>");
        // A broken declaration is left alone, and the parser then refuses the document.
        assert!(parse_wms("<!DOCTYPE a [ <WMS_Capabilities/>").is_err());
        // Quotes with a bracket or a greater-than sign inside do not end the declaration.
        assert_eq!(
            strip_doctype("<!DOCTYPE a [<!ENTITY b \"]>\">]><r/>"),
            "<r/>"
        );
    }

    #[test]
    fn rejects_other_documents_and_reports_server_errors() {
        assert!(parse_wms("not xml").is_err());
        assert!(
            parse_wms("<html/>")
                .unwrap_err()
                .to_string()
                .contains("not a WMS")
        );
        assert!(
            parse_wmts("<html/>")
                .unwrap_err()
                .to_string()
                .contains("not a WMTS")
        );
        assert!(
            parse_wfs("<html/>")
                .unwrap_err()
                .to_string()
                .contains("not a WFS")
        );
        assert!(detect("<html/>").is_err());
        let err = parse_wms(r#"<ServiceExceptionReport><ServiceException code="X">Layer missing</ServiceException></ServiceExceptionReport>"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Layer missing"), "{err}");
        assert!(detect(r#"<ows:ExceptionReport xmlns:ows="x"><ows:Exception><ows:ExceptionText>Denied</ows:ExceptionText></ows:Exception></ows:ExceptionReport>"#)
            .unwrap_err()
            .to_string()
            .contains("Denied"));
        assert!(
            parse_wms("<WMS_Capabilities/>")
                .unwrap_err()
                .to_string()
                .contains("Capability")
        );
        assert!(
            parse_wmts("<Capabilities/>")
                .unwrap_err()
                .to_string()
                .contains("Contents")
        );
        // A DOCTYPE is dropped, never read; an entity that it defines is not expanded.
        let with_entity = "<!DOCTYPE a [<!ENTITY b \"c\">]><WMS_Capabilities><Capability><Layer><Title>&b;</Title></Layer></Capability></WMS_Capabilities>";
        assert!(parse_wms(with_entity).is_err());
    }
}
