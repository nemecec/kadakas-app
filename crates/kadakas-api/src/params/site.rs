//! Parameters of the methods of the domain `site`.
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;

param_enum! {
    MassingAddRoof {
        Flat = "flat",
        Gable = "gable" | "gabled",
        Hip = "hip" | "hipped",
    }
}

param_enum! {
    WallAddBottom {
        Existing = "existing",
        Proposed = "proposed",
    }
}

param_enum! {
    WallAddRetains {
        Left = "left",
        Right = "right",
    }
}

param_enum! {
    SiteAnalysisMode {
        Off = "off",
        Slope = "slope",
        Elevation = "elevation",
    }
}

param_enum! {
    SpotAddShow {
        Existing = "existing",
        Proposed = "proposed",
        Both = "both",
    }
}

param_enum! {
    SpotAddKind {
        Spot = "spot",
        Stake = "stake",
    }
}

param_enum! {
    SiteTextureLook {
        Color = "color",
        Texture = "texture",
        Elevation = "elevation",
    }
}

param_enum! {
    SurveyImportPointsDelimiter {
        Auto = "auto" | "",
        Comma = "comma" | ",",
        Semicolon = "semicolon" | ";",
        Tab = "tab" | "\t" | "\\t",
        Pipe = "pipe" | "|",
        Space = "space" | " " | "whitespace",
    }
}

param_enum! {
    SurveyImportPointsUnit {
        M = "m" | "meter" | "meters" | "metre",
        Mm = "mm",
        Cm = "cm",
        Km = "km",
        Ft = "ft" | "foot" | "feet",
        Usft = "usft" | "us_ft" | "survey_ft",
        Inch = "inch" | "in" | "inches",
        Yd = "yd" | "yard",
    }
}

// Parameters of `massing.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MassingAddParams {
    /// a closed outline; curves are allowed like in polylines
    pub footprint: Vec<Vertex>,
    /// eave height in mm
    #[schemars(extend("default" = 3000))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floors: Option<Int>,
    /// mm
    #[schemars(extend("default" = 3000))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_height: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roof: Option<MassingAddRoof>,
    /// roof pitch in degrees
    #[schemars(extend("default" = 30))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<Num>,
    /// radians; default: along the long side
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ridge_angle: Option<Num>,
    /// height of the base above the layer in mm; default: on the site model of the layer, else 0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    /// a group
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ObjectRef>,
}

// Parameters of `massing.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["ids"]}]}]))]
pub struct MassingSetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footprint: Option<Vec<Vertex>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Num>,
    /// null for none
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Int>")]
    pub floors: Option<Option<Int>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_height: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roof: Option<MassingAddRoof>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<Num>,
    /// null for the default
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub ridge_angle: Option<Option<Num>>,
    /// null: on the site model
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub base: Option<Option<Num>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `massing.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MassingGetParams {
    pub id: ObjectRef,
}

// Parameters of `massing.from_shapes`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MassingFromShapesParams {
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    /// default: height, building_height, building:height, ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_field: Option<String>,
    /// default: floors, levels, building:levels, ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floors_field: Option<String>,
    /// mm per unit of the height field
    #[schemars(extend("default" = 1000))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_scale: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_field: Option<String>,
    /// mm, used when the shape has no height and no floors
    #[schemars(extend("default" = 3000))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_height: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor_height: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roof: Option<MassingAddRoof>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<Num>,
    /// leave out shapes without a height
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_height: Option<bool>,
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_source: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
}

// Parameters of `massing.follow_sources`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MassingFollowSourcesParams {
    /// default: all models with a link to their shape
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
}

// Parameters of `massing.report`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MassingReportParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<Id>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

// Parameters of `extrude.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtrudeAddParams {
    /// a closed outline, curves allowed like in polylines
    pub profile: Vec<Vertex>,
    /// closed outlines inside the profile
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holes: Option<Value>,
    /// mm, not zero; negative extrudes downward
    pub height: Num,
    /// height of the profile above the layer in mm, default 0
    #[schemars(extend("default" = 0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    /// a group
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Value>,
}

// Parameters of `extrude.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["ids"]}]}]))]
pub struct ExtrudeSetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<Id>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<Vec<Vertex>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holes: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `extrude.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtrudeGetParams {
    pub id: ObjectRef,
}

// Parameters of `extrude.from_shapes`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtrudeFromShapesParams {
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<Id>>,
    /// mm, default 1000
    #[schemars(extend("default" = 1000))]
    pub height: Num,
    /// mm, default 0
    #[schemars(extend("default" = 0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Num>,
    /// default true: the 2D shape becomes the solid; false keeps it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_source: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
}

// Parameters of `sweep.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SweepAddParams {
    /// x is the distance from the axis, y the position along it; a closed profile makes a solid with end caps, an open one a surface
    pub profile: Vec<Vertex>,
    /// default true
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    /// [x, y, z] of a point of the axis, mm; z is measured from the layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Point3>,
    /// [x, y, z] direction, default [0, 0, 1]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<Value>,
    /// degrees, the angle of the profile plane around the axis at the start
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Num>,
    /// degrees, default 360, at most 3600; more than 360 needs a pitch at least as large as the profile along the axis, or the turns cross each other
    #[schemars(extend("default" = 360))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep_angle: Option<Num>,
    /// degrees, default 10, from 0.1 to 180: the smaller, the rounder
    #[schemars(extend("default" = 10))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment_angle: Option<Num>,
    /// rise along the axis for each full turn in mm, default 0; a spiral
    #[schemars(extend("default" = 0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Value>,
}

// Parameters of `sweep.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["ids"]}]}]))]
pub struct SweepSetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<Id>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<Vec<Vertex>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Point3>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep_angle: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment_angle: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `sweep.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SweepGetParams {
    pub id: ObjectRef,
}

// Parameters of `sweep.from_shapes`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SweepFromShapesParams {
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<Id>>,
    /// a line object, or a polyline with two vertices: the profile turns around it, in the plane of the plan; the axis is horizontal and the line stays
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis_id: Option<Id>,
    /// [x, y]: a vertical axis at this point instead
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis_at: Option<Value>,
    /// default 360
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep_angle: Option<Value>,
    /// default 10
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment_angle: Option<Value>,
    /// mm per turn, default 0
    #[schemars(extend("default" = 0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch: Option<Num>,
    /// default true
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_source: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
}

// Parameters of `polygon3d.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Polygon3dAddParams {
    /// [[x, y, z]]: z is measured from the layer, in mm
    pub points: Vec<Point3>,
    /// a closed polygon needs 3 points, an open one 2
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    /// a filled closed polygon is a face in 3D views and renders
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    /// in the object data
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    /// a group
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ObjectRef>,
}

// Parameters of `polygon3d.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["ids"]}]}]))]
pub struct Polygon3dSetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<Id>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    /// mm added to the z of every vertex
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raise: Option<Num>,
}

// Parameters of `polygon3d.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Polygon3dGetParams {
    pub id: ObjectRef,
}

// Parameters of `wall_style.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WallStyleCreateParams {
    pub name: String,
    /// mm; with components it is their total
    #[schemars(extend("default" = 300))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// [{name, material, thickness}], across the thickness
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<Obj>>,
    /// {height, overhang?, material?}
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Obj>")]
    pub coping: Option<Option<Obj>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

// Parameters of `wall_style.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WallStyleListParams {}

// Parameters of `wall_style.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WallStyleUpdateParams {
    pub id: ResourceRef,
    /// renames
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// mm; with components it is their total
    #[schemars(extend("default" = 300))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// [{name, material, thickness}], across the thickness
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<Obj>>,
    /// {height, overhang?, material?}
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Obj>")]
    pub coping: Option<Option<Obj>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

// Parameters of `wall_style.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WallStyleDeleteParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reassign: Option<ResourceRef>,
}

// Parameters of `wall.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WallAddParams {
    pub path: Vec<Vertex>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    /// mm
    #[schemars(extend("default" = 300))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<Num>,
    /// {mode: fixed, elevation} | {mode: terrain, offset} | {mode: slope, start, end}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<Obj>,
    /// the ground that the bottom and a terrain top follow
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<WallAddBottom>,
    /// mm below the ground
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embed: Option<Num>,
    /// its thickness and coping win
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    /// a site model; default the one that covers the wall
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<ObjectRef>,
    /// the wall is a retaining edge
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_modifier: Option<bool>,
    #[schemars(extend("default" = "left"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retains: Option<WallAddRetains>,
    /// mm the retained ground reaches behind the wall
    #[schemars(extend("default" = 1500))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backfill: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `wall.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["ids"]}]}]))]
pub struct WallSetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<Vertex>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    /// mm
    #[schemars(extend("default" = 300))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<Num>,
    /// {mode: fixed, elevation} | {mode: terrain, offset} | {mode: slope, start, end}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<Obj>,
    /// the ground that the bottom and a terrain top follow
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<WallAddBottom>,
    /// mm below the ground
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embed: Option<Num>,
    /// its thickness and coping win
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    /// a site model; default the one that covers the wall
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<ObjectRef>,
    /// the wall is a retaining edge
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_modifier: Option<bool>,
    #[schemars(extend("default" = "left"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retains: Option<WallAddRetains>,
    /// mm the retained ground reaches behind the wall
    #[schemars(extend("default" = 1500))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backfill: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `wall.report`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WallReportParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
}

// Parameters of `grade.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GradeAddParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<Point>,
    /// a curve with arcs; instead of a and b
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<Vertex>>,
    /// the grade changes the proposed surface
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_modifier: Option<bool>,
    /// mm
    #[schemars(extend("default" = 1500))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevation_a: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevation_b: Option<Num>,
    /// {percent: 2} | {ratio: 50} | {angle: 1.2}; positive rises from a to b
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slope: Option<Obj>,
    /// both elevations are read from the site model, live
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snap: Option<WallAddBottom>,
    /// a site model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_tags: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `grade.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GradeSetParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<Vertex>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_modifier: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Num>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub elevation_a: Option<Option<Num>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub elevation_b: Option<Option<Num>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Obj>")]
    pub slope: Option<Option<Obj>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<WallAddBottom>")]
    pub snap: Option<Option<WallAddBottom>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<ObjectRef>")]
    pub site: Option<Option<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_tags: Option<Value>,
}

// Parameters of `grade.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GradeGetParams {
    pub id: ObjectRef,
}

// Parameters of `site.analysis`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteAnalysisParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<SiteAnalysisMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<WallAddBottom>,
    /// class limits, ascending: slope in percent, elevation in mm; [] for the defaults
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breaks: Option<Vec<Num>>,
    /// one per band
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<Vec<Color>>,
    /// 0 to 1, the 2D tint
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<Num>,
    /// {rect: [[x0, y0], [x1, y1]], title?, show_areas?}: adds a legend object on the layer of the site model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legend: Option<Obj>,
}

// Parameters of `spot.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpotAddParams {
    pub at: Point,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show: Option<SpotAddShow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<SpotAddKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// stake name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 0 to 4
    #[schemars(extend("default" = 2))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decimals: Option<Int>,
    /// mm on paper
    #[schemars(extend("default" = 2.5))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_height: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `spot.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpotSetParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<ObjectRef>")]
    pub site: Option<Option<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show: Option<SpotAddShow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<SpotAddKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decimals: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_height: Option<Num>,
}

// Parameters of `spot.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["at"]}]}]))]
pub struct SpotGetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<ObjectRef>,
}

// Parameters of `section.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SectionAddParams {
    pub a: Point,
    pub b: Point,
    #[schemars(extend("default" = "A"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// vertical
    #[schemars(extend("default" = 2))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exaggeration: Option<Num>,
    /// mm between samples when the line leaves the model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<Num>,
    /// mm: plants this near the line show
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walls: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardscapes: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plants: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `section.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SectionSetParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exaggeration: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walls: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardscapes: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plants: Option<bool>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<ObjectRef>")]
    pub site: Option<Option<ObjectRef>>,
}

// Parameters of `section.profile`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SectionProfileParams {
    pub id: ObjectRef,
}

// Parameters of `section.place`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SectionPlaceParams {
    pub id: ObjectRef,
    /// where the start of the section goes
    pub at: Point,
    /// default the layer of the section; may be a sheet layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    /// drawing scale denominator, for example 100
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Num>,
}

// Parameters of `pad.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PadAddParams {
    /// arcs allowed
    pub boundary: Vec<Vertex>,
    /// mm
    pub elevation: Num,
    /// {direction (radians the ground falls), percent}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slope: Option<Obj>,
    /// true or the settings of pad.retaining_edge
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retaining_edge: Option<Either<bool, Obj>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `grade_limit.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GradeLimitAddParams {
    /// 3 or more, arcs allowed
    pub boundary: Vec<Vertex>,
    /// the modifier acts on the site models of its layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `pad.retaining_edge`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PadRetainingEdgeParams {
    /// a pad
    pub id: ObjectRef,
    /// false removes the wall
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// mm
    #[schemars(extend("default" = 300))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<Num>,
    /// mm
    #[schemars(extend("default" = 0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_offset: Option<Num>,
    /// mm below the ground
    #[schemars(extend("default" = 0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embed: Option<Num>,
    /// wall style
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `site.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteCreateParams {
    pub source_ids: Vec<Id>,
    /// design layer of the site model; default: the layer of the first source
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Obj>,
}

// Parameters of `site.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteUpdateParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<Obj>,
    /// 3D loci and 3D polylines that move into the site model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub add_source_ids: Option<Vec<ObjectRef>>,
    /// indexes in the source points
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_points: Option<Vec<Int>>,
    /// indexes in the source lines
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_lines: Option<Vec<Int>>,
    /// [{point: index | line: index, with: id}]: the locus3d or polyline3d takes the place of that source
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace: Option<Vec<Obj>>,
}

// Parameters of `site.texture`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteTextureParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<SiteTextureLook>,
    /// a material whose texture is used; null: the material of the class or of the object slot
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<ResourceRef>")]
    pub material: Option<Option<ResourceRef>>,
    /// mm, 10 to 1e8
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile_size: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Point>,
    /// degrees, counterclockwise
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Num>,
    /// 1 to 64 elevation bands
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bands: Option<Int>,
    /// 1 to 16 colors from low to high ground
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp: Option<Vec<Color>>,
    /// [low, high] mm elevation of the ramp
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Vec<Num>>")]
    pub range: Option<Option<Vec<Num>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bands_at_contours: Option<bool>,
}

// Parameters of `bed.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["boundary"]}, {"required": ["from"]}]}]))]
pub struct BedAddParams {
    /// arcs allowed
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<Vec<Vertex>>,
    /// closed shapes: a closed polyline, closed 3D polyline, rectangle, circle, landscape area without holes or hardscape
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Either<ObjectRef, Vec<ObjectRef>>>,
    /// a material; omit or null: the material of the slot or of the class
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<ResourceRef>")]
    pub texture: Option<Option<ResourceRef>>,
    /// mm, 10 to 1e8
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile_size: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Point>,
    /// degrees
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Num>,
    /// true gives the bed its own mapping; false uses the mapping of the site model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own_mapping: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_source: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `bed.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BedSetParams {
    pub id: ObjectRef,
    /// null for by class
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<ResourceRef>")]
    pub texture: Option<Option<ResourceRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile_size: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Num>,
    /// false: use the mapping of the site model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own_mapping: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<Vec<Vertex>>,
}

// Parameters of `bed.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BedGetParams {
    pub id: ObjectRef,
}

// Parameters of `bed.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BedListParams {
    /// only the beds that act on this site model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<ObjectRef>,
}

// Parameters of `site.elevation`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteElevationParams {
    pub id: ObjectRef,
    pub x: Num,
    pub y: Num,
    #[schemars(extend("default" = "proposed"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<WallAddBottom>,
}

// Parameters of `site.cut_fill`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteCutFillParams {
    pub id: ObjectRef,
}

// Parameters of `site.drape`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteDrapeParams {
    pub id: ObjectRef,
    pub points: Vec<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
}

// Parameters of `site.refresh`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteRefreshParams {
    pub id: ObjectRef,
}

// Parameters of `site.contours`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteContoursParams {
    pub id: ObjectRef,
    #[schemars(extend("default" = "proposed"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<WallAddBottom>,
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<bool>,
}

// Parameters of `survey.import_points`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["text"]}, {"required": ["path"]}]}]))]
pub struct SurveyImportPointsParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// the column order, default ["P","N","E","Z","Code"]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<Either<Vec<String>, String>>,
    #[schemars(extend("default" = "auto"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<SurveyImportPointsDelimiter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_lines: Option<Int>,
    /// unit of N and E
    #[schemars(extend("default" = "m"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<Either<Num, SurveyImportPointsUnit>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_unit: Option<Either<Num, SurveyImportPointsUnit>>,
    /// EPSG code of a new georeference
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crs: Option<String>,
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shift: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `tree.import_survey`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["text"]}, {"required": ["path"]}]}]))]
pub struct TreeImportSurveyParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delimiter: Option<SurveyImportPointsDelimiter>,
    /// N and E
    #[schemars(extend("default" = "m"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<Either<Num, SurveyImportPointsUnit>>,
    #[schemars(extend("default" = "cm"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dbh_unit: Option<Either<Num, SurveyImportPointsUnit>>,
    /// height and spread
    #[schemars(extend("default" = "m"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_unit: Option<Either<Num, SurveyImportPointsUnit>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crs: Option<String>,
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shift: Option<bool>,
    /// each tree stands on the ground of the site model that covers it
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_terrain: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `site.check`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteCheckParams {
    pub id: ObjectRef,
    /// mm
    #[schemars(extend("default" = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xy_tolerance: Option<Num>,
    /// mm
    #[schemars(extend("default" = 10))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_tolerance: Option<Num>,
    /// mm
    #[schemars(extend("default" = 1000))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spike_threshold: Option<Num>,
    #[schemars(extend("default" = 1000))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_issues: Option<Num>,
}

// Parameters of `site.simplify`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteSimplifyParams {
    pub id: ObjectRef,
    /// mm, positive
    pub tolerance: Num,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
}

// Parameters of `georef.survey_point`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["model"]}, {"required": ["object"]}]}]))]
pub struct GeorefSurveyPointParams {
    /// mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Point>,
    /// an object whose position is used
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectRef>,
    /// [easting, northing] in m
    pub map: Point,
    /// default: the CRS of the document
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Num>,
    /// {model | object, map}: a second known point sets the rotation
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second: Option<Obj>,
}

entries! {
    "massing.add" => MassingAddParams,
    "massing.set" => MassingSetParams,
    "massing.get" => MassingGetParams,
    "massing.from_shapes" => MassingFromShapesParams,
    "massing.follow_sources" => MassingFollowSourcesParams,
    "massing.report" => MassingReportParams,
    "extrude.add" => ExtrudeAddParams,
    "extrude.set" => ExtrudeSetParams,
    "extrude.get" => ExtrudeGetParams,
    "extrude.from_shapes" => ExtrudeFromShapesParams,
    "sweep.add" => SweepAddParams,
    "sweep.set" => SweepSetParams,
    "sweep.get" => SweepGetParams,
    "sweep.from_shapes" => SweepFromShapesParams,
    "polygon3d.add" => Polygon3dAddParams,
    "polygon3d.set" => Polygon3dSetParams,
    "polygon3d.get" => Polygon3dGetParams,
    "wall_style.create" => WallStyleCreateParams,
    "wall_style.list" => WallStyleListParams,
    "wall_style.update" => WallStyleUpdateParams,
    "wall_style.delete" => WallStyleDeleteParams,
    "wall.add" => WallAddParams,
    "wall.set" => WallSetParams,
    "wall.report" => WallReportParams,
    "grade.add" => GradeAddParams,
    "grade.set" => GradeSetParams,
    "grade.get" => GradeGetParams,
    "site.analysis" => SiteAnalysisParams,
    "spot.add" => SpotAddParams,
    "spot.set" => SpotSetParams,
    "spot.get" => SpotGetParams,
    "section.add" => SectionAddParams,
    "section.set" => SectionSetParams,
    "section.profile" => SectionProfileParams,
    "section.place" => SectionPlaceParams,
    "pad.add" => PadAddParams,
    "grade_limit.add" => GradeLimitAddParams,
    "pad.retaining_edge" => PadRetainingEdgeParams,
    "site.create" => SiteCreateParams,
    "site.update" => SiteUpdateParams,
    "site.texture" => SiteTextureParams,
    "bed.add" => BedAddParams,
    "bed.set" => BedSetParams,
    "bed.get" => BedGetParams,
    "bed.list" => BedListParams,
    "site.elevation" => SiteElevationParams,
    "site.cut_fill" => SiteCutFillParams,
    "site.drape" => SiteDrapeParams,
    "site.refresh" => SiteRefreshParams,
    "site.contours" => SiteContoursParams,
    "survey.import_points" => SurveyImportPointsParams,
    "tree.import_survey" => TreeImportSurveyParams,
    "site.check" => SiteCheckParams,
    "site.simplify" => SiteSimplifyParams,
    "georef.survey_point" => GeorefSurveyPointParams,
}
