//! Parameters of the methods of the domain `io`.
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::*;

param_enum! {
    ImportFileFormat {
        Dxf = "dxf",
        Dwg = "dwg",
        Shp = "shp" | "shapefile",
        Geojson = "geojson" | "json",
        Ifc = "ifc",
    }
}

param_enum! {
    ExportFileFormat {
        Dxf = "dxf",
        Geojson = "geojson" | "json",
        Dwg = "dwg",
        Ifc = "ifc",
        Gltf = "gltf",
        Glb = "glb",
        Obj = "obj",
        Pdf = "pdf",
        SheetSvg = "sheet_svg",
        SheetPng = "sheet_png",
    }
}

param_enum! {
    ImportApplyConflicts {
        Skip = "skip",
        Overwrite = "overwrite",
    }
}

param_enum! {
    GisWfsImportFormat {
        Geojson = "geojson",
        Gml = "gml",
    }
}

// Parameters of `import.file`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportFileParams {
    pub path: String,
    /// default: from the extension
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ImportFileFormat>,
    /// see describe.io for the options of each format
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Obj>,
}

// Parameters of `export.file`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportFileParams {
    pub path: String,
    /// default: from the extension; the sheet formats need format
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ExportFileFormat>,
    /// see describe.io for the options of each format
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Obj>,
    /// replace an existing file; a server for agents needs it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overwrite: Option<bool>,
}

// Parameters of `import.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportUpdateParams {
    pub path: String,
    /// the lineage; needed when several drawings were imported and the file does not say which
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawing: Option<String>,
    /// the old revision, when the document has no update data and no usable link
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    /// only entities on these DXF layers; the other layers stay at the old revision
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<Vec<String>>,
    /// largest distance of a moved entity, in units of the drawing; default automatic
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
    /// true: no pending update is kept
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    /// most changes listed, at most 1000
    #[schemars(extend("default" = 200))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<Int>,
    /// import options of the new file such as hatches or base: auto|link|none
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Obj>,
}

// Parameters of `import.apply`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportApplyParams {
    pub update_id: String,
    /// "all" or a list of change ids
    pub accept: Either<String, Vec<String>>,
    /// change ids
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reject: Option<Vec<String>>,
    /// skip keeps the edited objects of a change that the person edited
    #[schemars(extend("default" = "skip"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflicts: Option<ImportApplyConflicts>,
}

// Parameters of `gis.sources`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GisSourcesParams {}

// Parameters of `gis.cache.clear`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GisCacheClearParams {}

// Parameters of `gis.georef.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GisGeorefSetParams {
    /// 'EPSG:3067' or WKT/PROJ text; a projected CRS in meters
    pub crs: String,
    /// [easting, northing] in meters, default [0, 0]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Point>,
    /// radians, default 0
    #[schemars(extend("default" = 0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Num>,
}

// Parameters of `gis.coords`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GisCoordsParams {
    pub x: Num,
    pub y: Num,
}

// Parameters of `gis.reproject`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GisReprojectParams {
    /// 'EPSG:32635'
    pub crs: String,
    /// true: only the report
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    /// [easting, northing] in the new CRS for the model origin; default: the old origin converted and rounded to a meter
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Point>,
}

// Parameters of `gis.wms.capabilities`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GisWmsCapabilitiesParams {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,
}

// Parameters of `gis.wms.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GisWmsAddParams {
    pub url: String,
    /// the WMS layers
    pub layers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub styles: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transparent: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// [min x, min y, max x, max y] in model mm; default: the drawing plus 5 percent
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<Vec<Num>>,
    /// at most 4096
    #[schemars(extend("default" = 2048))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_px: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 0 to 1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<Num>,
    /// the design layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,
}

// Parameters of `gis.wms.refresh`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GisWmsRefreshParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<Vec<Num>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_px: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,
}

// Parameters of `gis.wmts.capabilities`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GisWmtsCapabilitiesParams {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,
}

// Parameters of `gis.wmts.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GisWmtsAddParams {
    pub url: String,
    /// the WMTS layer
    pub layer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix_set: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// [min x, min y, max x, max y] in model mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<Vec<Num>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_px: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<Num>,
    /// the design layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_name: Option<LayerRef>,
    /// the design layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,
}

// Parameters of `gis.wfs.capabilities`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GisWfsCapabilitiesParams {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,
}

// Parameters of `gis.wfs.import`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GisWfsImportParams {
    pub url: String,
    pub type_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crs: Option<String>,
    /// [min x, min y, max x, max y] in model mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<Vec<Num>>,
    /// at most 50000
    #[schemars(extend("default" = 5000))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_features: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<GisWfsImportFormat>,
    /// as in import.file: layer, class_field, origin, ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Obj>,
    /// false: skip GetCapabilities
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caps: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<bool>,
}

// Parameters of `gis.geoimage.import`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GisGeoimageImportParams {
    pub path: String,
    /// when the picture has none
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crs: Option<String>,
    /// default 2048
    #[schemars(extend("default" = 2048))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_px: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `gis.job.status`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GisJobStatusParams {
    pub job: Int,
    /// seconds to wait
    #[schemars(extend("default" = 0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<Num>,
}

// Parameters of `gis.job.cancel`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GisJobCancelParams {
    pub job: Int,
}

entries! {
    "import.file" => ImportFileParams,
    "export.file" => ExportFileParams,
    "import.update" => ImportUpdateParams,
    "import.apply" => ImportApplyParams,
    "gis.sources" => GisSourcesParams,
    "gis.cache.clear" => GisCacheClearParams,
    "gis.georef.set" => GisGeorefSetParams,
    "gis.coords" => GisCoordsParams,
    "gis.reproject" => GisReprojectParams,
    "gis.wms.capabilities" => GisWmsCapabilitiesParams,
    "gis.wms.add" => GisWmsAddParams,
    "gis.wms.refresh" => GisWmsRefreshParams,
    "gis.wmts.capabilities" => GisWmtsCapabilitiesParams,
    "gis.wmts.add" => GisWmtsAddParams,
    "gis.wfs.capabilities" => GisWfsCapabilitiesParams,
    "gis.wfs.import" => GisWfsImportParams,
    "gis.geoimage.import" => GisGeoimageImportParams,
    "gis.job.status" => GisJobStatusParams,
    "gis.job.cancel" => GisJobCancelParams,
}
