//! Parameters of the methods of the domain `plants`.
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;

param_enum! {
    PlantStyleCreateCategory {
        Tree = "tree",
        Shrub = "shrub",
        Perennial = "perennial",
        Grass = "grass",
        Groundcover = "groundcover",
    }
}

param_enum! {
    PlantStyleCreateGraphic {
        Circle = "circle",
        Cloud = "cloud",
        Star = "star",
        Spokes = "spokes",
        Rosette = "rosette",
        Clover = "clover",
    }
}

param_enum! {
    PlantStyleCreateForm {
        Tree = "tree",
        Columnar = "columnar",
        Conical = "conical",
        Spreading = "spreading",
        ShrubMound = "shrub_mound",
        Weeping = "weeping",
        Palm = "palm",
        ColumnarConifer = "columnar_conifer",
        MultiStem = "multi_stem",
        Hedge = "hedge",
    }
}

param_enum! {
    PlantStyleImportCsvUnit {
        Mm = "mm",
        Cm = "cm",
        M = "m",
    }
}

param_enum! {
    PlantStyleImportCsvOnDuplicate {
        Skip = "skip",
        Update = "update",
    }
}

param_enum! {
    PlantsPlaceMode {
        Single = "single",
        Path = "path",
        Grid = "grid",
        Triangular = "triangular",
        Fill = "fill",
        Poly = "poly",
        Hedgerow = "hedgerow",
    }
}

param_enum! {
    PlantsPlacePattern {
        Triangular = "triangular",
        Rectangular = "rectangular",
    }
}

param_enum! {
    ReportPlantScheduleFormat {
        Csv = "csv",
    }
}

param_enum! {
    HardscapeAddMode {
        Slab = "slab",
        Path = "path",
    }
}

param_enum! {
    HardscapeAddJoin {
        Miter = "miter",
        Round = "round",
        Bevel = "bevel",
    }
}

param_enum! {
    HardscapeAddAlign {
        Center = "center",
        Left = "left",
        Right = "right",
    }
}

param_enum! {
    UnitPatternLayoutInclude {
        Units = "units",
        Gaps = "gaps",
    }
}

param_enum! {
    UnitPatternModulePresetPreset {
        Hexagon = "hexagon",
        OctagonSquare = "octagon_square",
        IShape = "i_shape",
    }
}

param_enum! {
    RegionFromCurvesCreate {
        Hardscape = "hardscape",
        LandscapeArea = "landscape_area",
        Polyline = "polyline",
    }
}

param_enum! {
    RegionFromCurvesDepths {
        Even = "even",
        Odd = "odd",
        All = "all",
    }
}

param_enum! {
    HardscapeSplitZoneUnits {
        Cut = "cut",
        Centroid = "centroid",
    }
}

param_enum! {
    HardscapeCourseSide {
        Left = "left",
        Right = "right",
        Center = "center",
        Inside = "inside",
        Outside = "outside",
    }
}

param_enum! {
    HardscapeCourseOrientation {
        Soldier = "soldier",
        Stretcher = "stretcher",
    }
}

param_enum! {
    HardscapeCourseCorner {
        Miter = "miter",
        Butt = "butt",
        CornerUnit = "corner_unit",
    }
}

param_enum! {
    HardscapePlanClearWhat {
        All = "all",
        Splits = "splits",
        Zones = "zones",
        Courses = "courses",
    }
}

// Parameters of `report.plants`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportPlantsParams {}

// Parameters of `plant_style.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantStyleListParams {}

// Parameters of `plant_style.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantStyleGetParams {
    /// the ID or the name of the style
    pub id: ResourceRef,
}

// Parameters of `plant_style.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantStyleCreateParams {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub botanical_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub common_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<PlantStyleCreateCategory>,
    /// mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread: Option<Num>,
    /// mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graphic: Option<PlantStyleCreateGraphic>,
    /// the 3D form
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<PlantStyleCreateForm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// {key: text}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<BTreeMap<String, String>>,
    /// fields that a plant may not override
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<Vec<String>>,
    /// how the plants are counted in the schedule
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantity: Option<String>,
    /// mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trunk_diameter: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protection_zone: Option<bool>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub rpz_factor: Option<Option<Num>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub rpz_cap: Option<Option<Num>>,
    /// a symbol
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<ResourceRef>")]
    pub plan_symbol: Option<Option<ResourceRef>>,
    /// the picture resource
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<ResourceRef>")]
    pub image: Option<Option<ResourceRef>>,
    /// {pruning, fertilizer, interval, hours_per_year, notes}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintenance: Option<Obj>,
    /// {irrigated, water_need, plant_factor, hydrozone}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub irrigation: Option<Obj>,
}

// Parameters of `plant_style.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantStyleUpdateParams {
    pub id: ResourceRef,
    /// renames
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub botanical_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub common_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<PlantStyleCreateCategory>,
    /// mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread: Option<Num>,
    /// mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graphic: Option<PlantStyleCreateGraphic>,
    /// the 3D form
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<PlantStyleCreateForm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// {key: text}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<BTreeMap<String, String>>,
    /// fields that a plant may not override
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<Vec<String>>,
    /// how the plants are counted in the schedule
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantity: Option<String>,
    /// mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trunk_diameter: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protection_zone: Option<bool>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub rpz_factor: Option<Option<Num>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub rpz_cap: Option<Option<Num>>,
    /// a symbol
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<ResourceRef>")]
    pub plan_symbol: Option<Option<ResourceRef>>,
    /// the picture resource
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<ResourceRef>")]
    pub image: Option<Option<ResourceRef>>,
    /// {pruning, fertilizer, interval, hours_per_year, notes}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintenance: Option<Obj>,
    /// {irrigated, water_need, plant_factor, hydrozone}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub irrigation: Option<Obj>,
}

// Parameters of `plant_style.duplicate`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantStyleDuplicateParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `plant_style.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantStyleDeleteParams {
    pub id: ResourceRef,
    /// another plant style that takes the plants
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reassign: Option<ResourceRef>,
}

// Parameters of `plant_style.import_csv`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantStyleImportCsvParams {
    pub csv: String,
    /// folder that the picture file names of an image column are relative to
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pictures_dir: Option<String>,
    /// for spread and height columns without a unit
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<PlantStyleImportCsvUnit>,
    /// {field: header}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping: Option<BTreeMap<String, String>>,
    #[schemars(extend("default" = "skip"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_duplicate: Option<PlantStyleImportCsvOnDuplicate>,
}

// Parameters of `plant_style.export_csv`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantStyleExportCsvParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ResourceRef>>,
}

// Parameters of `plant_style.sample_csv`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantStyleSampleCsvParams {}

// Parameters of `plants.place`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantsPlaceParams {
    pub mode: PlantsPlaceMode,
    /// ID or name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    /// {field: value}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<Obj>,
    /// {seed?, size? (0 to 0.9), rotation? (bool)}; the same seed gives the same plants
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    /// true adds a plant tag
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<bool>,
    /// true stands each plant on the ground of the site model that covers it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_terrain: Option<bool>,
    /// a polyline, rect, circle, landscape area or hardscape to use as the path, boundary or rectangle
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ObjectRef>,
    /// single
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    /// path, hedgerow
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<Vertex>>,
    /// path, hedgerow
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    /// mm, or "spread" (one canopy diameter); path, grid, triangular, fill, hedgerow
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<Either<Num, String>>,
    /// grid
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing_x: Option<Either<Num, String>>,
    /// grid
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing_y: Option<Either<Num, String>>,
    /// path, fill
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<Int>,
    /// grid, triangular
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Point>,
    /// grid, triangular
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Point>,
    /// grid, triangular, fill
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin: Option<Num>,
    /// fill
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<Vec<Vertex>>,
    /// fill: plants per m2
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub density: Option<Num>,
    /// fill
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<PlantsPlacePattern>,
    /// fill: areas that stay empty
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holes: Option<Vec<Vec<Point>>>,
    /// poly
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<Point>>,
    /// hedgerow, 1 to 20
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<Int>,
    /// hedgerow
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_spacing: Option<Num>,
    /// hedgerow
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stagger: Option<bool>,
}

// Parameters of `plants.set_style`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantsSetStyleParams {
    pub ids: Vec<ObjectRef>,
    /// ID or name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unlink: Option<bool>,
}

// Parameters of `report.plant_schedule`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportPlantScheduleParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ReportPlantScheduleFormat>,
}

// Parameters of `report.existing_trees`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportExistingTreesParams {}

// Parameters of `plants.drape`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantsDrapeParams {
    /// plants and existing trees; default all
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    /// true removes the stored height
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<bool>,
}

// Parameters of `plant_tag.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantTagSetParams {
    /// plant tags
    pub ids: Vec<ObjectRef>,
    /// 1 to 3
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bubble: Option<bool>,
}

// Parameters of `plant_style.set_picture`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["path"]}, {"required": ["base64"]}]}]))]
pub struct PlantStyleSetPictureParams {
    pub id: ResourceRef,
    /// a PNG or JPEG file
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// the file bytes as base64 text
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base64: Option<String>,
    /// the file name to remember, for base64
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `plant_style.clear_picture`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantStyleClearPictureParams {
    pub id: ResourceRef,
}

// Parameters of `plant_style.get_picture`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlantStyleGetPictureParams {
    pub id: ResourceRef,
    /// true adds thumbnail_base64
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<bool>,
    /// true adds base64, the original file
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<bool>,
}

// Parameters of `hardscape_style.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HardscapeStyleListParams {}

// Parameters of `hardscape_style.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HardscapeStyleGetParams {
    pub id: ResourceRef,
}

// Parameters of `hardscape_style.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HardscapeStyleCreateParams {
    pub name: String,
    /// surface material
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen: Option<Color>,
    /// surface layer, mm
    #[schemars(extend("default" = 60))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<Num>,
    /// [{name, material, thickness, color?}], layers below the surface from top to bottom
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<Obj>>,
    /// {width, material, color?, sides? (both|left|right; a path only)}
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Obj>")]
    pub border: Option<Option<Obj>>,
    /// {kind (running_bond|stack|herringbone|basket_weave), unit_length, unit_width, spacing, angle, color?}
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Obj>")]
    pub joints: Option<Option<Obj>>,
    /// {key: text}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<BTreeMap<String, String>>,
}

// Parameters of `hardscape_style.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HardscapeStyleUpdateParams {
    pub id: ResourceRef,
    /// renames
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// surface material
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen: Option<Color>,
    /// surface layer, mm
    #[schemars(extend("default" = 60))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<Num>,
    /// [{name, material, thickness, color?}], layers below the surface from top to bottom
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<Obj>>,
    /// {width, material, color?, sides? (both|left|right; a path only)}
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Obj>")]
    pub border: Option<Option<Obj>>,
    /// {kind (running_bond|stack|herringbone|basket_weave), unit_length, unit_width, spacing, angle, color?}
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Obj>")]
    pub joints: Option<Option<Obj>>,
    /// {key: text}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<BTreeMap<String, String>>,
}

// Parameters of `hardscape_style.duplicate`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HardscapeStyleDuplicateParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `hardscape_style.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HardscapeStyleDeleteParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reassign: Option<Value>,
}

// Parameters of `landscape_area_style.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LandscapeAreaStyleListParams {}

// Parameters of `landscape_area_style.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LandscapeAreaStyleGetParams {
    pub id: ResourceRef,
}

// Parameters of `landscape_area_style.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LandscapeAreaStyleCreateParams {
    pub name: String,
    /// [{plant (plant style ID or name), percent? (default 100, all together at most 100), spacing? (mm) | density? (plants per m2), min_count?}]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mix: Option<Vec<Obj>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen: Option<Color>,
    /// mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_weight: Option<Num>,
    /// draw the plants in 2D
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_plants: Option<bool>,
    /// data fields such as hydrozone
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<BTreeMap<String, String>>,
}

// Parameters of `landscape_area_style.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LandscapeAreaStyleUpdateParams {
    pub id: ResourceRef,
    /// renames
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// [{plant (plant style ID or name), percent? (default 100, all together at most 100), spacing? (mm) | density? (plants per m2), min_count?}]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mix: Option<Vec<Obj>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pen: Option<Color>,
    /// mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_weight: Option<Num>,
    /// draw the plants in 2D
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_plants: Option<bool>,
    /// data fields such as hydrozone
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<BTreeMap<String, String>>,
}

// Parameters of `landscape_area_style.duplicate`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LandscapeAreaStyleDuplicateParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `landscape_area_style.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LandscapeAreaStyleDeleteParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reassign: Option<Value>,
}

// Parameters of `hardscape.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["boundary"]}, {"required": ["center"]}]}]))]
pub struct HardscapeAddParams {
    /// a slab: the outline
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<Vec<Vertex>>,
    /// a path: the center line (curves are allowed)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<Vec<Vertex>>,
    /// slab keeps the outline; path makes a slab into a path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<HardscapeAddMode>,
    /// path, mm
    #[schemars(extend("default" = 1200))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Num>,
    /// path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<HardscapeAddJoin>,
    /// path: a ring without ends
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    /// path: which edge of the path the line is
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<HardscapeAddAlign>,
    /// path: mm, moves the path to the left of the line; negative to the right
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Num>,
    /// ID or name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    /// {type: none} | {type: edges, elevations} | {type: cross, percent, angle_deg?} | {type: angle, degrees, angle_deg?} | {type: rise_run, rise, run, angle_deg?}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slope: Option<Obj>,
    /// top surface above the layer; above the ground when draped
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevation: Option<Num>,
    /// follow the site model that covers it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drape: Option<bool>,
    /// act as a pad on the site model; the formation level is the top minus the build-up
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_modifier: Option<bool>,
    /// when there is no style
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// when there is no style
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<Num>,
    /// open parts inside the boundary such as planting islands
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holes: Option<Vec<Vec<Vertex>>>,
    /// objects whose outline becomes a hole
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<ObjectRef>>,
    /// where the joint pattern starts
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Point>,
    /// degrees: the angle of the joint pattern
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Num>,
    /// start the pattern at vertex n of the outline and run it along the edge to vertex n+1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_to_edge: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `hardscape.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["ids"]}]}]))]
pub struct HardscapeSetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    /// slab or path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// null removes it
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<ResourceRef>")]
    pub style: Option<Option<ResourceRef>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Obj>")]
    pub slope: Option<Option<Obj>>,
    /// null clears it
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Point>")]
    pub origin: Option<Option<Point>>,
    /// null clears it
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub angle: Option<Option<Num>>,
    /// replaces the old holes; [] or null clears them
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Vec<Vec<Vertex>>>")]
    pub holes: Option<Option<Vec<Vec<Vertex>>>>,
    /// a slab: the outline
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<Vec<Vertex>>,
    /// a path: the center line (curves are allowed)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<Vec<Vertex>>,
    /// path, mm
    #[schemars(extend("default" = 1200))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Num>,
    /// path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<HardscapeAddJoin>,
    /// path: a ring without ends
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    /// path: which edge of the path the line is
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<HardscapeAddAlign>,
    /// path: mm, moves the path to the left of the line; negative to the right
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Num>,
    /// top surface above the layer; above the ground when draped
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevation: Option<Num>,
    /// follow the site model that covers it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drape: Option<bool>,
    /// act as a pad on the site model; the formation level is the top minus the build-up
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_modifier: Option<bool>,
    /// when there is no style
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// when there is no style
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<Num>,
    /// objects whose outline becomes a hole
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<ObjectRef>>,
    /// start the pattern at vertex n of the outline and run it along the edge to vertex n+1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_to_edge: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `hardscape.from_shapes`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HardscapeFromShapesParams {
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<Id>>,
    /// ID or name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    /// default true; false makes every shape a hardscape of its own
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holes_from_inner: Option<bool>,
    /// default false: the outlines stay
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_source: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
}

// Parameters of `unit_pattern.layout`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["region"]}]}]))]
pub struct UnitPatternLayoutParams {
    /// a closed polyline, rectangle, circle, landscape area or hardscape
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    /// {outline: [vertices], holes?: [[vertices]]}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Obj>,
    /// {bond, unit_length, unit_width, gap?} or a module; see unit_pattern.module_preset
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<Obj>,
    /// where the pattern starts
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Point>,
    /// degrees, counterclockwise
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Num>,
    /// start at vertex n of the outline and run along the edge to vertex n+1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_to_edge: Option<Int>,
    /// default both
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<UnitPatternLayoutInclude>>,
    /// a page of the lists
    #[schemars(extend("default" = 0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Int>,
    /// at most 20000; 0 gives the counts only
    #[schemars(extend("default" = 1000))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<Int>,
    /// curve flattening, mm
    #[schemars(extend("default" = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
    #[schemars(extend("default" = 0.25))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sliver_share: Option<Num>,
    #[schemars(extend("default" = 0.05))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unfillable_share: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_width: Option<Num>,
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_unfillable: Option<bool>,
    #[schemars(extend("default" = 0.1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waste_percent: Option<Num>,
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap_length: Option<bool>,
    #[schemars(extend("default" = 2))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_extra_joint_mm: Option<Num>,
}

// Parameters of `unit_pattern.to_objects`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["region"]}]}]))]
pub struct UnitPatternToObjectsParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_to_edge: Option<Int>,
    #[schemars(extend("default" = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
    /// default ["units"]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<UnitPatternLayoutInclude>>,
    /// default: the layer of the object, else the active layer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<bool>,
    /// the name of the group
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `unit_pattern.module_preset`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnitPatternModulePresetParams {
    pub preset: UnitPatternModulePresetPreset,
    /// hexagon, octagon_square: mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Num>,
    /// hexagon: mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub across_flats: Option<Num>,
    /// i_shape: mm
    #[schemars(extend("default" = 200))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<Num>,
    /// i_shape: mm
    #[schemars(extend("default" = 165))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<Num>,
    /// i_shape: mm
    #[schemars(extend("default" = 25))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notch: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<Num>,
}

// Parameters of `unit_pattern.module_from_shapes`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnitPatternModuleFromShapesParams {
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    /// one per shape
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub names: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<Point>,
}

// Parameters of `unit_pattern.module_from_block`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnitPatternModuleFromBlockParams {
    pub path: String,
    pub block: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<Point>,
}

// Parameters of `report.unit_pattern`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportUnitPatternParams {
    /// default: every hardscape with a joint pattern
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_to_edge: Option<Int>,
    #[schemars(extend("default" = 0.25))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sliver_share: Option<Num>,
    #[schemars(extend("default" = 0.05))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unfillable_share: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_width: Option<Num>,
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_unfillable: Option<bool>,
    #[schemars(extend("default" = 0.1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waste_percent: Option<Num>,
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap_length: Option<bool>,
    #[schemars(extend("default" = 2))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_extra_joint_mm: Option<Num>,
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_only: Option<bool>,
    #[schemars(extend("default" = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ReportPlantScheduleFormat>,
}

// Parameters of `region.from_curves`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegionFromCurvesParams {
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    /// mm, the widest gap that is closed
    #[schemars(extend("default" = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
    /// hardscape, landscape_area or polyline: make objects from the regions in one undo step; without it nothing is created
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create: Option<RegionFromCurvesCreate>,
    /// even (the regions with role area; the default for hardscape and landscape_area), odd (the islands) or all (every region, also the doubtful ones; the default for polyline)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depths: Option<RegionFromCurvesDepths>,
    /// region indexes or points: make exactly these regions
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regions: Option<Vec<Either<Int, Point>>>,
    /// hardscape style, for create hardscape
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    /// landscape area name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// leave out regions smaller than this (m2)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_area_m2: Option<Num>,
    /// the layer of the new objects; default: the layer of the first source object
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    /// the class of the new objects; default: the class of the first source object
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    /// delete a source only when every piece of it lies in regions that are made
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_source: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
}

// Parameters of `hardscape.split`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HardscapeSplitParams {
    pub id: ObjectRef,
    /// replaces the split lines; [] removes them
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<Either<ObjectRef, Vec<Vertex>>>>,
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow: Option<bool>,
    /// [{index?, seed?, name?, pattern?, origin?, angle?, align_to_edge?, align_to_split?}]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zones: Option<Vec<Obj>>,
    /// mm
    #[schemars(extend("default" = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
    #[schemars(extend("default" = "cut"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone_units: Option<HardscapeSplitZoneUnits>,
}

// Parameters of `hardscape.zone_set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HardscapeZoneSetParams {
    pub id: ObjectRef,
    /// the index of the zone in the reply of hardscape.split or hardscape.plan (from 0)
    pub zone: Int,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// as the pattern of unit_pattern.layout
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Point>,
    /// degrees
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Num>,
    /// start at vertex n of the outline of the zone
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_to_edge: Option<Int>,
    /// start at the first point of split line n
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_to_split: Option<Int>,
    /// settings to remove: pattern, origin, angle, align_to_edge, align_to_split
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear: Option<Vec<String>>,
}

// Parameters of `hardscape.course`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HardscapeCourseParams {
    /// the hardscape that gets the course
    pub id: ObjectRef,
    /// [{name, path | source | along, side?, offset?, closed?, orientation?, unit_length?, unit_width?, gap?, corner?, unit?, align_field?}]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub courses: Option<Vec<Obj>>,
    /// the fields of one course directly in the params
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// the vertices of the path (curves allowed)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<Vertex>>,
    /// a line, arc or polyline to follow; it is read again each time
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ObjectRef>,
    /// outline, or hole:N (the edge of the Nth hole): the path is made from the edge of the hardscape each time
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub along: Option<String>,
    /// left, right, center, inside or outside; default for outline: inside, for a hole: outside
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<HardscapeCourseSide>,
    /// mm to the left of the path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Num>,
    /// the path closes on itself
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    /// soldier (default): the long side of the stone across the path; stretcher: along it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation: Option<HardscapeCourseOrientation>,
    /// mm along the path; default: from the stone of the style
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_length: Option<Num>,
    /// mm across the path; default: from the stone of the style
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_width: Option<Num>,
    /// the joint in mm; default: the joint of the style
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<Num>,
    /// miter (default), butt or corner_unit
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner: Option<HardscapeCourseCorner>,
    /// the name of the unit type; default: the size of the stone of the style
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// true: the pattern of the hardscape starts at the inner edge of the course (sets its origin and angle)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_field: Option<bool>,
}

// Parameters of `hardscape.course_remove`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HardscapeCourseRemoveParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub names: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
}

// Parameters of `hardscape.plan`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HardscapePlanParams {
    pub id: ObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outlines: Option<bool>,
}

// Parameters of `hardscape.plan_clear`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["ids"]}]}]))]
pub struct HardscapePlanClearParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    #[schemars(extend("default" = "all"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub what: Option<HardscapePlanClearWhat>,
}

// Parameters of `unit_pattern.check`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnitPatternCheckParams {
    /// default: every hardscape with a joint pattern
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_only: Option<bool>,
    #[schemars(extend("default" = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlap_mm2: Option<Num>,
    #[schemars(extend("default" = 0.15))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlap_depth_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_joint_mm: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_joint_mm: Option<Num>,
    #[schemars(extend("default" = 0.05))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_share: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_width_mm: Option<Num>,
    #[schemars(extend("default" = 0.05))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_tol_mm: Option<Num>,
    #[schemars(extend("default" = 0.02))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_tol: Option<Num>,
    #[schemars(extend("default" = 0.1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim: Option<Num>,
    /// per kind, at most 10000
    #[schemars(extend("default" = 200))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_findings: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align_to_edge: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
}

// Parameters of `landscape_area.add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LandscapeAreaAddParams {
    pub boundary: Vec<Vertex>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// ID or name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    /// [[vertices]], parts that stay empty and lie inside the boundary
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holes: Option<Value>,
    /// [object IDs] whose outline becomes a hole: rect, circle, closed polyline, hardscape, landscape area
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_plants: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drape: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `landscape_area.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["ids"]}]}]))]
pub struct LandscapeAreaSetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<Id>>,
    /// null removes it
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<ResourceRef>")]
    pub style: Option<Option<ResourceRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holes: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_plants: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drape: Option<bool>,
}

// Parameters of `report.hardscape`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportHardscapeParams {
    /// default: every hardscape
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ReportPlantScheduleFormat>,
    /// false leaves out the unit pattern block
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<bool>,
    #[schemars(extend("default" = 0.25))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sliver_share: Option<Num>,
    #[schemars(extend("default" = 0.05))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unfillable_share: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_width: Option<Num>,
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_unfillable: Option<bool>,
    #[schemars(extend("default" = 0.1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trim: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waste_percent: Option<Num>,
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap_length: Option<bool>,
    #[schemars(extend("default" = 2))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_extra_joint_mm: Option<Num>,
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_only: Option<bool>,
    #[schemars(extend("default" = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Num>,
}

// Parameters of `report.landscape_areas`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportLandscapeAreasParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

// Parameters of `report.plant_list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportPlantListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

entries! {
    "report.plants" => ReportPlantsParams,
    "plant_style.list" => PlantStyleListParams,
    "plant_style.get" => PlantStyleGetParams,
    "plant_style.create" => PlantStyleCreateParams,
    "plant_style.update" => PlantStyleUpdateParams,
    "plant_style.duplicate" => PlantStyleDuplicateParams,
    "plant_style.delete" => PlantStyleDeleteParams,
    "plant_style.import_csv" => PlantStyleImportCsvParams,
    "plant_style.export_csv" => PlantStyleExportCsvParams,
    "plant_style.sample_csv" => PlantStyleSampleCsvParams,
    "plants.place" => PlantsPlaceParams,
    "plants.set_style" => PlantsSetStyleParams,
    "report.plant_schedule" => ReportPlantScheduleParams,
    "report.existing_trees" => ReportExistingTreesParams,
    "plants.drape" => PlantsDrapeParams,
    "plant_tag.set" => PlantTagSetParams,
    "plant_style.set_picture" => PlantStyleSetPictureParams,
    "plant_style.clear_picture" => PlantStyleClearPictureParams,
    "plant_style.get_picture" => PlantStyleGetPictureParams,
    "hardscape_style.list" => HardscapeStyleListParams,
    "hardscape_style.get" => HardscapeStyleGetParams,
    "hardscape_style.create" => HardscapeStyleCreateParams,
    "hardscape_style.update" => HardscapeStyleUpdateParams,
    "hardscape_style.duplicate" => HardscapeStyleDuplicateParams,
    "hardscape_style.delete" => HardscapeStyleDeleteParams,
    "landscape_area_style.list" => LandscapeAreaStyleListParams,
    "landscape_area_style.get" => LandscapeAreaStyleGetParams,
    "landscape_area_style.create" => LandscapeAreaStyleCreateParams,
    "landscape_area_style.update" => LandscapeAreaStyleUpdateParams,
    "landscape_area_style.duplicate" => LandscapeAreaStyleDuplicateParams,
    "landscape_area_style.delete" => LandscapeAreaStyleDeleteParams,
    "hardscape.add" => HardscapeAddParams,
    "hardscape.set" => HardscapeSetParams,
    "hardscape.from_shapes" => HardscapeFromShapesParams,
    "unit_pattern.layout" => UnitPatternLayoutParams,
    "unit_pattern.to_objects" => UnitPatternToObjectsParams,
    "unit_pattern.module_preset" => UnitPatternModulePresetParams,
    "unit_pattern.module_from_shapes" => UnitPatternModuleFromShapesParams,
    "unit_pattern.module_from_block" => UnitPatternModuleFromBlockParams,
    "report.unit_pattern" => ReportUnitPatternParams,
    "region.from_curves" => RegionFromCurvesParams,
    "hardscape.split" => HardscapeSplitParams,
    "hardscape.zone_set" => HardscapeZoneSetParams,
    "hardscape.course" => HardscapeCourseParams,
    "hardscape.course_remove" => HardscapeCourseRemoveParams,
    "hardscape.plan" => HardscapePlanParams,
    "hardscape.plan_clear" => HardscapePlanClearParams,
    "unit_pattern.check" => UnitPatternCheckParams,
    "landscape_area.add" => LandscapeAreaAddParams,
    "landscape_area.set" => LandscapeAreaSetParams,
    "report.hardscape" => ReportHardscapeParams,
    "report.landscape_areas" => ReportLandscapeAreasParams,
    "report.plant_list" => ReportPlantListParams,
}
