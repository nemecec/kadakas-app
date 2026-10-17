//! Parameters of the methods of the domain `drafting`.
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;

param_enum! {
    EditArrayShape {
        Linear = "linear",
        Rectangular = "rectangular",
        Circular = "circular",
    }
}

param_enum! {
    EditOffsetSide {
        Left = "left",
        Right = "right",
        Outside = "outside",
        Inside = "inside",
    }
}

param_enum! {
    EditFilletMode {
        Trim = "trim",
        Split = "split",
        Standard = "standard",
    }
}

param_enum! {
    SnapSmartAction {
        AcquirePoint = "acquire_point",
        AcquireEdge = "acquire_edge",
        AcquireAt = "acquire_at",
        Clear = "clear",
    }
}

param_enum! {
    TextStyleCreateJustify {
        Left = "left",
        Center = "center",
        Right = "right",
    }
}

param_enum! {
    DimensionStyleCreateUnit {
        Mm = "mm",
        Cm = "cm",
        M = "m",
        In = "in",
        Ft = "ft",
        FtIn = "ft_in",
    }
}

param_enum! {
    DimensionStyleCreateTerminator {
        Arrow = "arrow",
        OpenArrow = "open_arrow",
        Tick = "tick",
        Dot = "dot",
        None = "none",
    }
}

param_enum! {
    TagStyleCreateFrame {
        None = "none",
        Box = "box",
        Underline = "underline",
        Circle = "circle",
    }
}

param_enum! {
    TagStyleCreatePreset {
        Area = "area",
    }
}

param_enum! {
    NorthArrowAddArrowStyle {
        Arrow = "arrow",
        Compass = "compass",
        Simple = "simple",
    }
}

param_enum! {
    ScaleBarAddUnit {
        M = "m",
        Mm = "mm",
        Ft = "ft",
    }
}

param_enum! {
    AnnotationListType {
        Dimension = "dimension",
        Text = "text",
        Callout = "callout",
        DataTag = "data_tag",
        Legend = "legend",
        NorthArrow = "north_arrow",
        ScaleBar = "scale_bar",
        RevisionCloud = "revision_cloud",
    }
}

// Parameters of `edit.rotate`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditRotateParams {
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    pub center: Point,
    /// radians, counterclockwise
    pub angle: Num,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<bool>,
}

// Parameters of `edit.scale`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditScaleParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    pub center: Point,
    pub sx: Num,
    /// default sx; a negative factor mirrors
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sy: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<bool>,
}

// Parameters of `edit.mirror`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditMirrorParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    pub a: Point,
    pub b: Point,
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<bool>,
}

// Parameters of `edit.array`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditArrayParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    pub shape: EditArrayShape,
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retain: Option<bool>,
    /// each copy this many times larger than the one before
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resize: Option<Num>,
    /// radians; each copy turned this much more than the one before
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate: Option<Num>,
    /// linear, circular
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<Int>,
    /// linear: [dx, dy]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Point>,
    /// linear: with angle
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<Num>,
    /// linear: radians. rectangular: radians. circular: between copies, radians
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Num>,
    /// rectangular
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<Int>,
    /// rectangular
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<Int>,
    /// rectangular
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_spacing: Option<Num>,
    /// rectangular
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_spacing: Option<Num>,
    /// circular
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<Point>,
    /// circular
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate_objects: Option<bool>,
}

// Parameters of `edit.offset`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["distance"]}, {"required": ["through"]}]}]))]
pub struct EditOffsetParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<EditOffsetSide>,
    /// the copy passes through the point
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through: Option<Point>,
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<bool>,
}

// Parameters of `edit.fillet`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditFilletParams {
    pub radius: Num,
    #[schemars(extend("default" = "trim"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<EditFilletMode>,
    /// between two objects
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pick_a: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pick_b: Option<Point>,
    /// inside one shape
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corners: Option<Vec<Int>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
}

// Parameters of `edit.chamfer`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditChamferParams {
    #[schemars(extend("default" = "trim"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<EditFilletMode>,
    /// between two objects
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pick_a: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pick_b: Option<Point>,
    /// inside one shape
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corners: Option<Vec<Int>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
    /// from the corner along the first object
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<Num>,
    /// from the corner along the second object (default distance)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance2: Option<Num>,
    /// radians between the first object and the chamfer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Num>,
    /// the length of the chamfer line
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<Num>,
}

// Parameters of `edit.trim`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditTrimParams {
    pub id: ObjectRef,
    pub pick: Point,
    /// default: all other objects of the layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cutters: Option<Vec<ObjectRef>>,
}

// Parameters of `edit.extend`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditExtendParams {
    pub id: ObjectRef,
    pub pick: Point,
    /// default: all other objects of the layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundaries: Option<Vec<ObjectRef>>,
}

// Parameters of `edit.split`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["at"]}, {"required": ["line"]}]}]))]
pub struct EditSplitParams {
    pub id: ObjectRef,
    /// points on the object; it is cut there
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Vec<Point>>,
    /// {a, b}: cut a closed shape along the infinite line through a and b
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<Obj>,
}

// Parameters of `surface.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurfaceAddParams {
    /// 2 or more
    pub ids: Vec<ObjectRef>,
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep: Option<bool>,
}

// Parameters of `surface.subtract`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurfaceSubtractParams {
    pub base: ObjectRef,
    pub cutters: Vec<ObjectRef>,
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_cutters: Option<bool>,
}

// Parameters of `surface.clip`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurfaceClipParams {
    /// 2 or more
    pub ids: Vec<ObjectRef>,
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_cutter: Option<bool>,
}

// Parameters of `surface.intersect`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurfaceIntersectParams {
    /// 2 or more
    pub ids: Vec<ObjectRef>,
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep: Option<bool>,
}

// Parameters of `surface.compose`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SurfaceComposeParams {
    /// 2 or more
    pub ids: Vec<ObjectRef>,
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep: Option<bool>,
}

// Parameters of `snap.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapGetParams {}

// Parameters of `snap.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapSetParams {
    /// {category: bool}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<BTreeMap<String, bool>>,
    /// above 0, at most 180
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle_step_deg: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_angles: Option<bool>,
    /// mm; 0 uses the grid step
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance_step: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other_layers: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grey: Option<bool>,
}

// Parameters of `snap.query`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapQueryParams {
    pub point: Point,
    /// mm
    #[schemars(extend("default" = 10))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
    /// mm
    #[schemars(extend("default" = 100))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_step: Option<Num>,
    /// the last clicked point, for angle, distance, perpendicular and tangent
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Point>,
    /// radians
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_direction: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<ObjectRef>>,
    /// for this call only
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<BTreeMap<String, bool>>,
}

// Parameters of `snap.smart`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapSmartParams {
    pub action: SnapSmartAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
    /// acquire_at: take the edge nearest to the point
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<bool>,
}

// Parameters of `text_style.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextStyleListParams {}

// Parameters of `text_style.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextStyleGetParams {
    pub id: ResourceRef,
}

// Parameters of `text_style.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextStyleCreateParams {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// paper
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Color>")]
    pub color: Option<Option<Color>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub justify: Option<TextStyleCreateJustify>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_spacing: Option<Num>,
}

// Parameters of `text_style.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextStyleUpdateParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// paper
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Color>")]
    pub color: Option<Option<Color>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub justify: Option<TextStyleCreateJustify>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_spacing: Option<Num>,
}

// Parameters of `text_style.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextStyleDeleteParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reassign: Option<Value>,
}

// Parameters of `dimension_style.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DimensionStyleListParams {}

// Parameters of `dimension_style.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DimensionStyleGetParams {
    pub id: ResourceRef,
}

// Parameters of `dimension_style.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DimensionStyleCreateParams {
    pub name: String,
    #[schemars(extend("default" = "sans-serif"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// paper
    #[schemars(extend("default" = 2.5))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_size_mm: Option<Num>,
    #[schemars(extend("default" = "mm"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<DimensionStyleCreateUnit>,
    /// decimals
    #[schemars(extend("default" = 0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<Int>,
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_unit: Option<bool>,
    #[schemars(extend("default" = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle_precision: Option<Int>,
    #[schemars(extend("default" = "arrow"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminator: Option<DimensionStyleCreateTerminator>,
    #[schemars(extend("default" = 2.5))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminator_size_mm: Option<Num>,
    /// gap between the object and the extension line
    #[schemars(extend("default" = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext_gap_mm: Option<Num>,
    /// extension line past the dimension line
    #[schemars(extend("default" = 2))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext_overshoot_mm: Option<Num>,
    #[schemars(extend("default" = 7))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_spacing_mm: Option<Num>,
    #[schemars(extend("default" = 0.8))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_gap_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    /// {plus, minus} in the unit of the style
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// round values to a multiple of this many real mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_to: Option<Num>,
}

// Parameters of `dimension_style.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DimensionStyleUpdateParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[schemars(extend("default" = "sans-serif"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// paper
    #[schemars(extend("default" = 2.5))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_size_mm: Option<Num>,
    #[schemars(extend("default" = "mm"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<DimensionStyleCreateUnit>,
    /// decimals
    #[schemars(extend("default" = 0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<Int>,
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_unit: Option<bool>,
    #[schemars(extend("default" = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle_precision: Option<Int>,
    #[schemars(extend("default" = "arrow"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminator: Option<DimensionStyleCreateTerminator>,
    #[schemars(extend("default" = 2.5))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminator_size_mm: Option<Num>,
    /// gap between the object and the extension line
    #[schemars(extend("default" = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext_gap_mm: Option<Num>,
    /// extension line past the dimension line
    #[schemars(extend("default" = 2))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext_overshoot_mm: Option<Num>,
    #[schemars(extend("default" = 7))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_spacing_mm: Option<Num>,
    #[schemars(extend("default" = 0.8))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_gap_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    /// {plus, minus} in the unit of the style
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    /// round values to a multiple of this many real mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_to: Option<Num>,
}

// Parameters of `dimension_style.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DimensionStyleDeleteParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reassign: Option<Value>,
}

// Parameters of `tag_style.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TagStyleListParams {}

// Parameters of `tag_style.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TagStyleGetParams {
    pub id: ResourceRef,
}

// Parameters of `tag_style.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TagStyleCreateParams {
    pub name: String,
    /// text with fields in braces: worksheet expressions over the properties of the tagged object
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_mm: Option<Num>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Color>")]
    pub color: Option<Option<Color>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<TagStyleCreateFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminator: Option<DimensionStyleCreateTerminator>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pad_mm: Option<Num>,
    /// the Area tag style
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<TagStyleCreatePreset>,
}

// Parameters of `tag_style.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TagStyleUpdateParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// text with fields in braces: worksheet expressions over the properties of the tagged object
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_mm: Option<Num>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Color>")]
    pub color: Option<Option<Color>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<TagStyleCreateFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminator: Option<DimensionStyleCreateTerminator>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pad_mm: Option<Num>,
    /// the Area tag style
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<TagStyleCreatePreset>,
}

// Parameters of `tag_style.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TagStyleDeleteParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reassign: Option<Value>,
}

// Parameters of `notes_db.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotesDbListParams {}

// Parameters of `notes_db.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotesDbGetParams {
    pub id: ResourceRef,
}

// Parameters of `notes_db.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotesDbCreateParams {
    pub name: String,
    /// [{key, text}] or {key: text}
    pub notes: Either<Vec<Obj>, BTreeMap<String, String>>,
}

// Parameters of `notes_db.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotesDbUpdateParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// [{key, text}] or {key: text}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<Either<Vec<Obj>, BTreeMap<String, String>>>,
}

// Parameters of `notes_db.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotesDbDeleteParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reassign: Option<Value>,
}

// Parameters of `dimension.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DimensionAddParams {
    /// horizontal, vertical, aligned, chain_*, baseline_*, ordinate_*, angular, radial, diameter, arc_length
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<Point>>,
    /// one entry per point: null or {object, vertex: i} | {object, edge: i, t?} | {object, center: true}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchors: Option<Vec<Option<Obj>>>,
    /// radial, diameter, arc_length: a circle or an arc
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectRef>,
    /// radians
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Num>,
    /// paper mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detached: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<ObjectRef>,
}

// Parameters of `callout.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalloutAddParams {
    pub at: Point,
    /// [{points, object?, vertex?, edge?, center?, t?}]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaders: Option<Vec<Obj>>,
    /// one leader
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip: Option<Point>,
    /// one leader: follows the object
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectRef>,
    /// one leader
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertex: Option<Int>,
    /// one leader
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<Int>,
    /// one leader: the place along the edge
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t: Option<Num>,
    /// one leader
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_db: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shoulder_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminator: Option<DimensionStyleCreateTerminator>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detached: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<ObjectRef>,
}

// Parameters of `data_tag.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DataTagAddParams {
    /// a tag style
    pub style: ResourceRef,
    pub object: ObjectRef,
    /// paper mm from the object
    #[schemars(extend("default" = "[8, 8]"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<ObjectRef>,
}

// Parameters of `data_tag.tag_all`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DataTagTagAllParams {
    pub style: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Point>,
}

// Parameters of `legend.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LegendAddParams {
    /// top left
    pub at: Point,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<String>,
    /// an expression
    #[schemars(extend("default" = "Style"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_count: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swatch_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<ObjectRef>,
}

// Parameters of `north_arrow.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NorthArrowAddParams {
    pub at: Point,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_deg: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_georef: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrow_style: Option<NorthArrowAddArrowStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<ObjectRef>,
}

// Parameters of `scale_bar.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScaleBarAddParams {
    pub at: Point,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<ScaleBarAddUnit>,
    /// length of one segment in the unit
    #[schemars(extend("default" = 5))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segments: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_mm: Option<Num>,
    /// denominator, instead of the one of the layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<ObjectRef>,
}

// Parameters of `revision_cloud.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RevisionCloudAddParams {
    pub points: Vec<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bump_mm: Option<Num>,
    pub revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<ObjectRef>,
}

// Parameters of `annotation.text`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnnotationTextParams {
    pub at: Point,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<ObjectRef>,
}

/// The fields of the annotation, as annotation.list returns them in items[].annotation. Which
/// ones apply depends on the type of the annotation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnnotationUpdateParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Either<Num, Point>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<Point>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchors: Option<Vec<Obj>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detached: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaders: Option<Vec<Obj>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_db: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shoulder_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminator: Option<DimensionStyleCreateTerminator>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_count: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swatch_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_deg: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_georef: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrow_style: Option<NorthArrowAddArrowStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<ScaleBarAddUnit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segments: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_mm: Option<Num>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub scale: Option<Option<Num>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bump_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<bool>,
    /// data_tag: what the tag follows, as for dimension anchors
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Obj>")]
    pub anchor: Option<Option<Obj>>,
}

// Parameters of `annotation.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnnotationListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<AnnotationListType>,
}

// Parameters of `annotation.evaluate`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnnotationEvaluateParams {
    pub id: ObjectRef,
}

// Parameters of `annotation.apply_text_style`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnnotationApplyTextStyleParams {
    pub ids: Vec<ObjectRef>,
    pub style: ResourceRef,
}

entries! {
    "edit.rotate" => EditRotateParams,
    "edit.scale" => EditScaleParams,
    "edit.mirror" => EditMirrorParams,
    "edit.array" => EditArrayParams,
    "edit.offset" => EditOffsetParams,
    "edit.fillet" => EditFilletParams,
    "edit.chamfer" => EditChamferParams,
    "edit.trim" => EditTrimParams,
    "edit.extend" => EditExtendParams,
    "edit.split" => EditSplitParams,
    "surface.add" => SurfaceAddParams,
    "surface.subtract" => SurfaceSubtractParams,
    "surface.clip" => SurfaceClipParams,
    "surface.intersect" => SurfaceIntersectParams,
    "surface.compose" => SurfaceComposeParams,
    "snap.get" => SnapGetParams,
    "snap.set" => SnapSetParams,
    "snap.query" => SnapQueryParams,
    "snap.smart" => SnapSmartParams,
    "text_style.list" => TextStyleListParams,
    "text_style.get" => TextStyleGetParams,
    "text_style.create" => TextStyleCreateParams,
    "text_style.update" => TextStyleUpdateParams,
    "text_style.delete" => TextStyleDeleteParams,
    "dimension_style.list" => DimensionStyleListParams,
    "dimension_style.get" => DimensionStyleGetParams,
    "dimension_style.create" => DimensionStyleCreateParams,
    "dimension_style.update" => DimensionStyleUpdateParams,
    "dimension_style.delete" => DimensionStyleDeleteParams,
    "tag_style.list" => TagStyleListParams,
    "tag_style.get" => TagStyleGetParams,
    "tag_style.create" => TagStyleCreateParams,
    "tag_style.update" => TagStyleUpdateParams,
    "tag_style.delete" => TagStyleDeleteParams,
    "notes_db.list" => NotesDbListParams,
    "notes_db.get" => NotesDbGetParams,
    "notes_db.create" => NotesDbCreateParams,
    "notes_db.update" => NotesDbUpdateParams,
    "notes_db.delete" => NotesDbDeleteParams,
    "dimension.add" => DimensionAddParams,
    "callout.add" => CalloutAddParams,
    "data_tag.add" => DataTagAddParams,
    "data_tag.tag_all" => DataTagTagAllParams,
    "legend.add" => LegendAddParams,
    "north_arrow.add" => NorthArrowAddParams,
    "scale_bar.add" => ScaleBarAddParams,
    "revision_cloud.add" => RevisionCloudAddParams,
    "annotation.text" => AnnotationTextParams,
    "annotation.update" => AnnotationUpdateParams,
    "annotation.list" => AnnotationListParams,
    "annotation.evaluate" => AnnotationEvaluateParams,
    "annotation.apply_text_style" => AnnotationApplyTextStyleParams,
}
