//! Parameters of the methods of the domain `sheets`.
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;

param_enum! {
    RenderPngProjection {
        Ortho = "ortho" | "orthographic" | "parallel",
        Perspective = "perspective" | "persp",
    }
}

param_enum! {
    RenderPngMode {
        Shaded = "shaded" | "opengl" | "textured",
        Wireframe = "wireframe",
        HiddenLine = "hidden_line" | "hiddenline",
        DashedHiddenLine = "dashed_hidden_line" | "dashed_hidden",
    }
}

param_enum! {
    SectionViewportLineAddLook {
        Left = "left",
        Right = "right",
    }
}

param_enum! {
    SectionViewportLineAddMode {
        Cut = "cut",
        Elevation = "elevation",
    }
}

param_enum! {
    SectionViewportCreateScale {
        Fit = "fit",
    }
}

param_enum! {
    ViewScreenshotRegion {
        Drawing = "drawing",
        Window = "window",
    }
}

param_enum! {
    SheetCreateOrientation {
        Landscape = "landscape",
        Portrait = "portrait",
    }
}

param_enum! {
    SheetRenderFormat {
        Svg = "svg",
        Png = "png",
    }
}

// Parameters of `render.svg`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderSvgParams {
    /// pixels
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Int>,
    /// [x0, y0, x1, y1] in drawing coordinates (mm): draw only this part, and draw the units of the paved areas in it one by one when there are at most 60,000; above that the paved areas are a preview (marked in the picture)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Vec<Num>>,
    /// mark the findings of unit_pattern.check (overlaps, small pieces, joints, holes) on the paved areas
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_check: Option<bool>,
    /// {from, to, mode?, hide_unchanged?, ...}: draw the changes since a version
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compare: Option<Obj>,
}

// Parameters of `render.png`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderPngParams {
    /// a name from view.list; default the current view
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    /// pixels, 16 to 2048
    #[schemars(extend("default" = 800))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Int>,
    /// pixels, 16 to 2048
    #[schemars(extend("default" = 600))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection: Option<RenderPngProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<RenderPngMode>,
    /// a render style of the document (ID or name) or a built-in one such as Realistic
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_style: Option<ResourceRef>,
    /// 1 to 3 (anti-aliasing)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub samples: Option<Int>,
    /// {from, to, mode?, hide_unchanged?, ...}: draw the changes since a version
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compare: Option<Obj>,
}

// Parameters of `render.hidden_line`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderHiddenLineParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Num>,
    /// px, as render.png
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Num>,
    /// also return the covered edges, dashed and gray
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashed: Option<bool>,
    /// add silhouettes and creases of the surfaces; default true
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_edges: Option<bool>,
    /// also return the segments
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segments: Option<bool>,
}

// Parameters of `section_viewport.line_add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SectionViewportLineAddParams {
    pub a: Point,
    pub b: Point,
    #[schemars(extend("default" = "A"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[schemars(extend("default" = "left"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<SectionViewportLineAddLook>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<SectionViewportLineAddMode>,
    /// far clip in mm; default no limit
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<Num>,
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beyond: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `section_viewport.line_set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SectionViewportLineSetParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<SectionViewportLineAddLook>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<SectionViewportLineAddMode>,
    /// null removes the limit
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub depth: Option<Option<Num>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beyond: Option<bool>,
}

// Parameters of `section_viewport.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SectionViewportCreateParams {
    pub sheet: SheetRef,
    /// a section line
    pub line: ObjectRef,
    /// design layers that the viewport shows; default all
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<Vec<LayerRef>>,
    /// the frame on the sheet, mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Point>,
    /// a denominator or "fit"
    #[schemars(extend("default" = "fit"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Either<Num, SectionViewportCreateScale>>,
    /// [distance along the line, height] in mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_frame: Option<bool>,
    /// mm on paper
    #[schemars(extend("default" = 0.5))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut_weight: Option<Num>,
    #[schemars(extend("default" = 0.15))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beyond_weight: Option<Num>,
}

// Parameters of `section_viewport.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SectionViewportUpdateParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Point>,
    /// [distance along the line, height] in mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Either<Num, SectionViewportCreateScale>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<Vec<LayerRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_frame: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut_weight: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beyond_weight: Option<Num>,
    /// true recomputes center and scale from the drawing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<bool>,
}

// Parameters of `section_viewport.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SectionViewportGetParams {
    /// a section viewport or a section line
    pub id: ObjectRef,
    /// true adds all points
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<bool>,
}

// Parameters of `view.screenshot`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewScreenshotParams {
    #[schemars(extend("default" = "drawing"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<ViewScreenshotRegion>,
    /// px, 64 to 4096
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_width: Option<Int>,
}

// Parameters of `sheet.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SheetListParams {}

// Parameters of `sheet.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SheetCreateParams {
    pub name: String,
    /// a0 a1 a2 a3 a4 letter legal tabloid arch_c arch_d arch_e, or {width_mm, height_mm}
    #[schemars(extend("default" = "a3"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper: Option<Either<String, Obj>>,
    /// with height_mm: a paper size of your own
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<SheetCreateOrientation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

// Parameters of `sheet.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SheetSetParams {
    pub layer: SheetRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper: Option<Either<String, Obj>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<SheetCreateOrientation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// with width_mm: a paper size of your own
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_mm: Option<Num>,
}

// Parameters of `sheet.render`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SheetRenderParams {
    /// a sheet layer: ID or name (with compare it may be a sheet of a version)
    pub sheet: String,
    #[schemars(extend("default" = "svg"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<SheetRenderFormat>,
    /// png
    #[schemars(extend("default" = 100))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpi: Option<Num>,
    /// {from, to}: versions or "current"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compare: Option<Obj>,
}

// Parameters of `viewport.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewportCreateParams {
    pub sheet: SheetRef,
    /// design layers; default all
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<Vec<LayerRef>>,
    /// the frame on the sheet, mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Point>,
    /// a denominator such as 100, or "fit"
    #[schemars(extend("default" = "fit"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Either<Num, SectionViewportCreateScale>>,
    /// model point at the middle of the frame
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<Point>,
    /// plan or a 3D view from view.list
    #[schemars(extend("default" = "plan"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    /// 3D only
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<RenderPngMode>,
    /// a render style of the document; it replaces mode
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_style: Option<ResourceRef>,
    /// 3D only
    #[schemars(extend("default" = 150))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpi: Option<Num>,
    /// {type: rect, min, max} | {type: polygon, points} in paper mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_frame: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `viewport.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewportGetParams {
    pub id: ObjectRef,
}

// Parameters of `viewport.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewportListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<SheetRef>,
}

// Parameters of `viewport.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewportUpdateParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Either<Num, SectionViewportCreateScale>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<Vec<LayerRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<RenderPngMode>,
    /// null removes it
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<ResourceRef>")]
    pub render_style: Option<Option<ResourceRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpi: Option<Num>,
    /// null removes it
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Obj>")]
    pub crop: Option<Option<Obj>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_frame: Option<bool>,
    /// true recomputes center and scale
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<bool>,
}

// Parameters of `viewport.set_override`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewportSetOverrideParams {
    pub id: ObjectRef,
    /// {layer: true | false | null}; keys are layer IDs or names
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<BTreeMap<String, Option<bool>>>,
    /// {class: {visible?, pen?, fill?, line_weight?} | null}; keys are class IDs or names
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classes: Option<BTreeMap<String, Option<Obj>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<bool>,
}

// Parameters of `viewport.annotate`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewportAnnotateParams {
    pub id: ObjectRef,
    /// [{kind: line, a, b} | {kind: text, at, text, height} | {kind: dimension, a, b, offset, text?} | {kind: shape, shape}]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub add: Option<Vec<Obj>>,
    /// annotation ids
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove: Option<Vec<Int>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear: Option<bool>,
}

// Parameters of `title_block.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TitleBlockCreateParams {
    pub sheet: SheetRef,
    /// mm
    #[schemars(extend("default" = 10))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin: Option<Num>,
    /// 0 to 26
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_columns: Option<Int>,
    /// 0 to 99
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_rows: Option<Int>,
    /// {width, height, cells: [{x, y, w, h, label, text, size}]} in mm from the lower left corner of the block; omit for the standard layout
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Obj>,
    /// {name: text}, for {field.name}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<BTreeMap<String, String>>,
}

// Parameters of `title_block.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TitleBlockUpdateParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_columns: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid_rows: Option<Int>,
    /// null for the standard layout
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Obj>")]
    pub layout: Option<Option<Obj>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<BTreeMap<String, String>>,
}

// Parameters of `title_block.resolve`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TitleBlockResolveParams {
    pub id: ObjectRef,
}

entries! {
    "render.svg" => RenderSvgParams,
    "render.png" => RenderPngParams,
    "render.hidden_line" => RenderHiddenLineParams,
    "section_viewport.line_add" => SectionViewportLineAddParams,
    "section_viewport.line_set" => SectionViewportLineSetParams,
    "section_viewport.create" => SectionViewportCreateParams,
    "section_viewport.update" => SectionViewportUpdateParams,
    "section_viewport.get" => SectionViewportGetParams,
    "view.screenshot" => ViewScreenshotParams,
    "sheet.list" => SheetListParams,
    "sheet.create" => SheetCreateParams,
    "sheet.set" => SheetSetParams,
    "sheet.render" => SheetRenderParams,
    "viewport.create" => ViewportCreateParams,
    "viewport.get" => ViewportGetParams,
    "viewport.list" => ViewportListParams,
    "viewport.update" => ViewportUpdateParams,
    "viewport.set_override" => ViewportSetOverrideParams,
    "viewport.annotate" => ViewportAnnotateParams,
    "title_block.create" => TitleBlockCreateParams,
    "title_block.update" => TitleBlockUpdateParams,
    "title_block.resolve" => TitleBlockResolveParams,
}
