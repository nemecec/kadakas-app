//! Parameters of the methods of the domain `layers`.
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::*;

param_enum! {
    ViewSetProjection {
        Ortho = "ortho" | "orthographic" | "parallel",
        Perspective = "perspective" | "persp",
    }
}

param_enum! {
    ViewSetMode {
        Shaded = "shaded" | "opengl" | "textured",
        Wireframe = "wireframe",
        HiddenLine = "hidden_line" | "hiddenline",
        DashedHiddenLine = "dashed_hidden_line" | "dashed_hidden",
    }
}

param_enum! {
    PhaseSetPhase {
        Existing = "existing" | "exist" | "retained",
        New = "new" | "proposed",
        Demolished = "demolished" | "demolish" | "demo" | "removed",
    }
}

param_enum! {
    VisualizationSetTarget {
        Fill = "fill",
        Pen = "pen",
        Both = "both",
    }
}

param_enum! {
    VisibilitySetMode {
        Visible = "visible",
        Grey = "grey" | "gray",
        Invisible = "invisible" | "hidden",
    }
}

// Parameters of `view.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewListParams {}

// Parameters of `view.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewSetParams {
    /// a name from view.list, or the name of a saved view (its camera only)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection: Option<ViewSetProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ViewSetMode>,
}

// Parameters of `view_rules.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewRulesGetParams {}

// Parameters of `grid.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GridSetParams {
    /// m, positive; null removes the grid
    #[serde(deserialize_with = "required_nullable")]
    #[schemars(with = "Nullable<Num>")]
    pub spacing: Option<Num>,
    /// default true
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<bool>,
    /// model mm, 0 = automatic
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_height: Option<Num>,
    /// [r,g,b,a]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

// Parameters of `phase.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PhaseSetParams {
    pub ids: Vec<ObjectRef>,
    pub phase: PhaseSetPhase,
}

// Parameters of `phase.display`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PhaseDisplayParams {
    /// false turns the rules off; null removes them
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<bool>")]
    pub enabled: Option<Option<bool>>,
    /// {visible?, pen?, fill?, weight_factor?}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demolished: Option<Obj>,
}

// Parameters of `phase.report`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PhaseReportParams {}

// Parameters of `visualization.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VisualizationSetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// a property such as Condition, Plant.category, Phase, Area or a record field Soil.Depth
    pub field: String,
    /// [{criterion: {type: equals, value} | {type: range, min?, max?} | {type: otherwise}, color: [r,g,b,a], label?}]; the first rule that fits wins
    pub rules: Vec<Obj>,
    #[schemars(extend("default" = "fill"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<VisualizationSetTarget>,
    /// shape kinds; empty = all
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_missing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

// Parameters of `visualization.auto`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VisualizationAutoParams {
    pub field: String,
    /// 2 to 64: ranges of a number, light to dark blue
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bins: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<VisualizationSetTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `visualization.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VisualizationGetParams {}

// Parameters of `visualization.clear`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VisualizationClearParams {}

// Parameters of `visibility.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VisibilityGetParams {}

// Parameters of `visibility.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VisibilitySetParams {
    pub mode: VisibilitySetMode,
    /// [ids]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<Vec<LayerRef>>,
    /// [ids]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classes: Option<Vec<ClassRef>>,
}

// Parameters of `saved_view.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SavedViewListParams {}

// Parameters of `saved_view.save`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SavedViewSaveParams {
    pub name: String,
    /// px per mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoom: Option<Num>,
    /// model point at the middle
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pan: Option<Point>,
}

// Parameters of `saved_view.restore`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SavedViewRestoreParams {
    pub name: String,
}

// Parameters of `saved_view.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SavedViewDeleteParams {
    pub name: String,
}

entries! {
    "view.list" => ViewListParams,
    "view.set" => ViewSetParams,
    "view_rules.get" => ViewRulesGetParams,
    "grid.set" => GridSetParams,
    "phase.set" => PhaseSetParams,
    "phase.display" => PhaseDisplayParams,
    "phase.report" => PhaseReportParams,
    "visualization.set" => VisualizationSetParams,
    "visualization.auto" => VisualizationAutoParams,
    "visualization.get" => VisualizationGetParams,
    "visualization.clear" => VisualizationClearParams,
    "visibility.get" => VisibilityGetParams,
    "visibility.set" => VisibilitySetParams,
    "saved_view.list" => SavedViewListParams,
    "saved_view.save" => SavedViewSaveParams,
    "saved_view.restore" => SavedViewRestoreParams,
    "saved_view.delete" => SavedViewDeleteParams,
}
