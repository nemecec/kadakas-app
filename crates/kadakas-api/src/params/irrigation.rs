//! Parameters of the methods of the domain `irrigation`.
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::*;

param_enum! {
    IrrigationLineAddLineType {
        Mainline = "mainline",
        Lateral = "lateral",
        Drip = "drip",
    }
}

param_enum! {
    IrrigationCatalogImportCsvKind {
        Heads = "heads",
        Pipes = "pipes",
        Materials = "materials",
    }
}

param_enum! {
    IrrigationLayoutFillPattern {
        Square = "square",
        Triangular = "triangular" | "triangle",
    }
}

param_enum! {
    IrrigationLayoutEdgeSide {
        Left = "left",
        Right = "right",
    }
}

param_enum! {
    IrrigationReportZonesFormat {
        Csv = "csv",
    }
}

// Parameters of `irrigation.outlet.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrrigationOutletAddParams {
    pub at: Point,
    /// a head of irrigation.catalog.get
    pub model: String,
    /// spray angle in degrees
    #[schemars(extend("default" = 360))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc: Option<Num>,
    /// degrees, the middle of the arc, counterclockwise from +X
    #[schemars(extend("default" = 0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Num>,
    /// mm; default: the radius of the model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `irrigation.line.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrrigationLineAddParams {
    /// 2 or more
    pub points: Vec<Point>,
    #[schemars(extend("default" = "lateral"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_type: Option<IrrigationLineAddLineType>,
    /// nominal pipe size, for example "32"
    pub size: Either<String, Num>,
    /// for example "PE"
    pub material: String,
    /// mm, drip line
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emitter_spacing: Option<Num>,
    /// l/h per emitter, drip line
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emitter_flow: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `irrigation.poc.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrrigationPocAddParams {
    pub at: Point,
    /// kPa
    pub static_pressure: Num,
    /// l/min
    pub available_flow: Num,
    /// kPa lost in the supply line at the available flow
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supply_loss: Option<Num>,
    /// kPa lost in the meter and backflow preventer at the available flow
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meter_loss: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `irrigation.valve.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrrigationValveAddParams {
    pub at: Point,
    /// 1 or more
    pub zone: Int,
    /// flow factor in m3/h at 1 bar
    #[schemars(extend("default" = 6))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kv: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// name of a controller
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `irrigation.controller.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrrigationControllerAddParams {
    pub at: Point,
    pub name: String,
    /// default 6
    #[schemars(extend("default" = 6))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stations: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

/// The other parameters are the fields of the object. outlet: model, arc, direction, radius,
/// zone. line: points, line_type, size, material, emitter_spacing, emitter_flow. poc: at, name,
/// static_pressure, available_flow. valve: at, zone, name, kv, controller, station. controller:
/// at, name, stations. null clears an optional field.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["ids"]}]}]))]
pub struct IrrigationSetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Num>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub radius: Option<Option<Num>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Int>")]
    pub zone: Option<Option<Int>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<Point>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_type: Option<IrrigationLineAddLineType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Either<String, Num>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub emitter_spacing: Option<Option<Num>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub emitter_flow: Option<Option<Num>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<String>")]
    pub name: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_pressure: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_flow: Option<Num>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub supply_loss: Option<Option<Num>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub meter_loss: Option<Option<Num>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kv: Option<Num>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<String>")]
    pub controller: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Int>")]
    pub station: Option<Option<Int>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stations: Option<Int>,
}

// Parameters of `irrigation.catalog.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrrigationCatalogGetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_only: Option<bool>,
}

// Parameters of `irrigation.catalog.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrrigationCatalogSetParams {
    /// lists as in irrigation.catalog.get
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heads: Option<Vec<Obj>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipes: Option<Vec<Obj>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materials: Option<Vec<Obj>>,
    /// drop the old document parts first
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace: Option<bool>,
}

// Parameters of `irrigation.catalog.import_csv`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrrigationCatalogImportCsvParams {
    pub kind: IrrigationCatalogImportCsvKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace: Option<bool>,
}

// Parameters of `irrigation.catalog.export_csv`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrrigationCatalogExportCsvParams {
    pub kind: IrrigationCatalogImportCsvKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_only: Option<bool>,
}

// Parameters of `irrigation.calculate`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrrigationCalculateParams {
    /// mm
    #[schemars(extend("default" = 50))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
    /// percent added to the pipe losses
    #[schemars(extend("default" = 0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fittings: Option<Num>,
    /// the height of the site model counts, 9.81 kPa per m
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_site_model: Option<bool>,
    /// store the zone in the outlets, one undo step
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assign_zones: Option<bool>,
    /// {zone: m2}, real areas for the precipitation rate
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub areas: Option<BTreeMap<String, Num>>,
    /// mm
    #[schemars(extend("default" = 10))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_depth: Option<Num>,
}

// Parameters of `irrigation.layout.fill`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["boundary"]}, {"required": ["object"]}]}]))]
pub struct IrrigationLayoutFillParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<Vec<Vertex>>,
    /// a rect, closed polyline, landscape area or hardscape
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectRef>,
    pub model: String,
    /// head-to-head, mm; default the radius of the model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<IrrigationLayoutFillPattern>,
    /// degrees
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Num>,
    /// mm from the edge
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inset: Option<Num>,
    #[schemars(extend("default" = 360))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc: Option<Num>,
    /// add the outlets in one undo step
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<bool>,
    /// the layer of the new objects; default the active layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `irrigation.layout.edge`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["path"]}, {"required": ["object"]}]}]))]
pub struct IrrigationLayoutEdgeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<Vertex>>,
    /// a polyline or area
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectRef>,
    /// the path is a ring
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    pub model: String,
    /// default the radius of the model; the heads are spread evenly at most this far apart
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<Num>,
    #[schemars(extend("default" = 180))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arc: Option<Num>,
    /// the way the heads face
    #[schemars(extend("default" = "left"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<IrrigationLayoutEdgeSide>,
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `irrigation.layout.drip`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["boundary"]}, {"required": ["object"]}]}]))]
pub struct IrrigationLayoutDripParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<Vec<Vertex>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectRef>,
    /// mm between lines
    pub spacing: Num,
    /// degrees
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Num>,
    /// mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inset: Option<Num>,
    #[schemars(extend("default" = 16))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Either<String, Num>>,
    #[schemars(extend("default" = "PE"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emitter_spacing: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emitter_flow: Option<Num>,
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `irrigation.report.zones`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrrigationReportZonesParams {
    /// mm
    #[schemars(extend("default" = 10))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_depth: Option<Num>,
    /// {zone: m2}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub areas: Option<BTreeMap<String, Num>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<IrrigationReportZonesFormat>,
}

// Parameters of `irrigation.report.pipes`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrrigationReportPipesParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

// Parameters of `irrigation.report.pressure`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrrigationReportPressureParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

// Parameters of `irrigation.report.materials`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrrigationReportMaterialsParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

// Parameters of `irrigation.report.hydrozones`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrrigationReportHydrozonesParams {
    /// reference evapotranspiration in mm per year
    #[schemars(extend("default" = 1000))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eto: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_depth: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<IrrigationReportZonesFormat>,
}

entries! {
    "irrigation.outlet.add" => IrrigationOutletAddParams,
    "irrigation.line.add" => IrrigationLineAddParams,
    "irrigation.poc.add" => IrrigationPocAddParams,
    "irrigation.valve.add" => IrrigationValveAddParams,
    "irrigation.controller.add" => IrrigationControllerAddParams,
    "irrigation.set" => IrrigationSetParams,
    "irrigation.catalog.get" => IrrigationCatalogGetParams,
    "irrigation.catalog.set" => IrrigationCatalogSetParams,
    "irrigation.catalog.import_csv" => IrrigationCatalogImportCsvParams,
    "irrigation.catalog.export_csv" => IrrigationCatalogExportCsvParams,
    "irrigation.calculate" => IrrigationCalculateParams,
    "irrigation.layout.fill" => IrrigationLayoutFillParams,
    "irrigation.layout.edge" => IrrigationLayoutEdgeParams,
    "irrigation.layout.drip" => IrrigationLayoutDripParams,
    "irrigation.report.zones" => IrrigationReportZonesParams,
    "irrigation.report.pipes" => IrrigationReportPipesParams,
    "irrigation.report.pressure" => IrrigationReportPressureParams,
    "irrigation.report.materials" => IrrigationReportMaterialsParams,
    "irrigation.report.hydrozones" => IrrigationReportHydrozonesParams,
}
