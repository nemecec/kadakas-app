//! Parameters of the methods of the domain `view`. the view of the window or the picture (AGR-02, AGR-03).
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::*;

param_enum! {
    ViewCameraProjection {
        Ortho = "ortho" | "orthographic" | "parallel",
        Perspective = "perspective" | "persp",
    }
}

param_enum! {
    ViewHighlightColor {
        Agent = "agent",
        Red = "red",
        Blue = "blue",
        Green = "green",
        Orange = "orange",
    }
}

// Parameters of `view.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewGetParams {}

// Parameters of `view.zoom`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["ids"]}, {"required": ["box"]}, {"required": ["at"]}]}]))]
pub struct ViewZoomParams {
    /// objects to frame
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    /// [x0, y0, x1, y1] in drawing coordinates (mm)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#box: Option<Vec<Num>>,
    /// [x, y] or [x, y, z]: the new middle of the view
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Vec<Num>>,
    /// px per mm of the plan, 0.0001 to 100
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Num>,
    /// space around ids or box, as a fraction of its size on each side (0 to 10)
    #[schemars(extend("default" = 0.1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin: Option<Num>,
}

// Parameters of `view.pan`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["by"]}, {"required": ["to"]}]}]))]
pub struct ViewPanParams {
    /// [dx, dy] (3D also dz) in drawing mm, east and north
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<Vec<Num>>,
    /// [x, y] (3D also z): the new middle of the view
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<Vec<Num>>,
}

// Parameters of `view.camera`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewCameraParams {
    /// [x, y, z] in drawing mm
    pub eye: Vec<Num>,
    /// [x, y, z] in drawing mm
    pub target: Vec<Num>,
    /// must point upward (z above 0): the camera has no roll
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up: Option<Vec<Num>>,
    /// degrees; the camera has a fixed 45
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fov: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection: Option<ViewCameraProjection>,
}

// Parameters of `view.highlight`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["ids"]}, {"required": ["box"]}, {"required": ["at"]}]}]))]
pub struct ViewHighlightParams {
    /// objects to mark with an outline glow
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    /// [x0, y0, x1, y1] in drawing coordinates (mm)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#box: Option<Vec<Num>>,
    /// [x, y] or [x, y, z]: a marker at this place
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Vec<Num>>,
    /// a short text, at most 80 characters, one line
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[schemars(extend("default" = "agent"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ViewHighlightColor>,
    /// next_change, dismissed (default) or a time in ms (1 to 3600000)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<Either<String, Int>>,
}

// Parameters of `view.highlight_clear`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewHighlightClearParams {
    /// one pointer; all when left out
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Int>,
}

entries! {
    "view.get" => ViewGetParams,
    "view.zoom" => ViewZoomParams,
    "view.pan" => ViewPanParams,
    "view.camera" => ViewCameraParams,
    "view.highlight" => ViewHighlightParams,
    "view.highlight_clear" => ViewHighlightClearParams,
}
