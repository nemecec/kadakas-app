//! Parameters of the methods of the domain `reports`.
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;

param_enum! {
    WorksheetExportFormat {
        Csv = "csv",
        Xlsx = "xlsx",
    }
}

// Parameters of `report.areas`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportAreasParams {}

// Parameters of `worksheet.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorksheetListParams {}

// Parameters of `worksheet.templates`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorksheetTemplatesParams {}

// Parameters of `worksheet.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorksheetCreateParams {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cells: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub databases: Option<Value>,
}

// Parameters of `worksheet.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorksheetGetParams {
    pub id: ResourceRef,
}

// Parameters of `worksheet.rename`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorksheetRenameParams {
    pub id: ResourceRef,
    pub name: String,
}

// Parameters of `worksheet.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorksheetDeleteParams {
    pub id: ResourceRef,
}

// Parameters of `worksheet.set_cell`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorksheetSetCellParams {
    pub id: ResourceRef,
    pub address: String,
    /// text, a number or a formula such as =SUM(B3:B9); empty clears the cell; without it only the format changes
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decimals: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

// Parameters of `worksheet.set_cells`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorksheetSetCellsParams {
    pub id: ResourceRef,
    pub cells: Obj,
}

// Parameters of `worksheet.set_database`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorksheetSetDatabaseParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<Int>,
    pub database: Obj,
}

// Parameters of `worksheet.remove_database`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorksheetRemoveDatabaseParams {
    pub id: ResourceRef,
    pub index: Int,
}

// Parameters of `worksheet.evaluate`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorksheetEvaluateParams {
    pub id: ResourceRef,
}

// Parameters of `worksheet.export`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorksheetExportParams {
    pub id: ResourceRef,
    pub format: WorksheetExportFormat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overwrite: Option<bool>,
}

// Parameters of `worksheet.place`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorksheetPlaceParams {
    pub id: ResourceRef,
    /// top left corner
    pub at: Point,
    /// mm, default 3.5
    #[schemars(extend("default" = 3.5))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_height: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `worksheet.fit`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorksheetFitParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
}

entries! {
    "report.areas" => ReportAreasParams,
    "worksheet.list" => WorksheetListParams,
    "worksheet.templates" => WorksheetTemplatesParams,
    "worksheet.create" => WorksheetCreateParams,
    "worksheet.get" => WorksheetGetParams,
    "worksheet.rename" => WorksheetRenameParams,
    "worksheet.delete" => WorksheetDeleteParams,
    "worksheet.set_cell" => WorksheetSetCellParams,
    "worksheet.set_cells" => WorksheetSetCellsParams,
    "worksheet.set_database" => WorksheetSetDatabaseParams,
    "worksheet.remove_database" => WorksheetRemoveDatabaseParams,
    "worksheet.evaluate" => WorksheetEvaluateParams,
    "worksheet.export" => WorksheetExportParams,
    "worksheet.place" => WorksheetPlaceParams,
    "worksheet.fit" => WorksheetFitParams,
}
