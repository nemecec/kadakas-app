//! Parameters of the methods of the domain `objects`.
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;

param_enum! {
    RenderStyleCreateMode {
        Shaded = "shaded" | "opengl" | "textured",
        Wireframe = "wireframe",
        HiddenLine = "hidden_line" | "hiddenline",
        DashedHiddenLine = "dashed_hidden_line" | "dashed_hidden",
    }
}

param_enum! {
    SymbolCreateUnits {
        World = "world",
        Paper = "paper",
    }
}

param_enum! {
    ResourceImportFromFileOnConflict {
        Rename = "rename",
        Reuse = "reuse" | "skip",
        Replace = "replace" | "overwrite",
    }
}

// Parameters of `objects.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObjectsListParams {
    /// only objects on this layer (ID or name)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    /// only objects of this class (ID or name)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    /// only objects of this kind, for example rect or plant
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

// Parameters of `objects.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObjectsGetParams {
    pub id: ObjectRef,
    /// show the import tags of the sources of a site model (data.import_sources)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<bool>,
}

// Parameters of `selection.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectionGetParams {}

// Parameters of `selection.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectionSetParams {
    pub ids: Vec<ObjectRef>,
}

// Parameters of `record_format.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordFormatListParams {}

// Parameters of `record_format.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordFormatGetParams {
    pub id: ResourceRef,
}

// Parameters of `record_format.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordFormatCreateParams {
    pub name: String,
    /// [{name, type, default?, unit?, choices?}]; type is text, integer, number, boolean, dimension, area, choice, date, time, volume or angle
    pub fields: Vec<Obj>,
}

// Parameters of `record_format.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordFormatUpdateParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// as in record_format.create
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<Obj>>,
}

// Parameters of `record_format.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordFormatDeleteParams {
    pub id: ResourceRef,
}

// Parameters of `record.attach`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordAttachParams {
    pub ids: Vec<Id>,
    /// ID or name
    pub record: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Obj>,
}

// Parameters of `record.detach`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordDetachParams {
    pub ids: Vec<Id>,
    pub record: String,
}

// Parameters of `record.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordSetParams {
    pub ids: Vec<Id>,
    pub record: String,
    pub values: Obj,
}

// Parameters of `record.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordGetParams {
    pub id: ObjectRef,
}

// Parameters of `data_manager.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DataManagerListParams {}

// Parameters of `data_manager.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DataManagerGetParams {
    pub id: ResourceRef,
}

// Parameters of `data_manager.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DataManagerCreateParams {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entities: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Value>,
}

// Parameters of `data_manager.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DataManagerUpdateParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entities: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Value>,
}

// Parameters of `data_manager.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DataManagerDeleteParams {
    pub id: ResourceRef,
}

// Parameters of `data_manager.check`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DataManagerCheckParams {
    pub id: ResourceRef,
}

// Parameters of `data_manager.evaluate`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DataManagerEvaluateParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<Id>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<Int>,
}

// Parameters of `data_manager.apply`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DataManagerApplyParams {
    pub id: ResourceRef,
}

// Parameters of `data_manager.properties`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DataManagerPropertiesParams {}

// Parameters of `material.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaterialListParams {}

// Parameters of `material.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaterialGetParams {
    pub id: ResourceRef,
}

// Parameters of `material.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaterialCreateParams {
    pub name: String,
    /// [r,g,b] or [r,g,b,a]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_color: Option<Color>,
    /// null | "grass" | {procedural: grass|gravel|pavers|mulch|water|brick|timber|noise, seed?} | {png_base64} | {jpeg_base64} | {path}
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Either<String, Obj>>")]
    pub texture: Option<Option<Either<String, Obj>>>,
    /// {kind: planar|box|cylindrical, scale: mm of one repeat as a number or [u, v], rotation: degrees, offset: [x, y] mm}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping: Option<Obj>,
    /// 0 to 1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness: Option<Num>,
    /// 0 to 1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic: Option<Num>,
    /// 0 to 1; below 1 is see-through, for water and glass
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<Num>,
}

// Parameters of `material.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaterialUpdateParams {
    pub id: ResourceRef,
    /// renames
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// [r,g,b] or [r,g,b,a]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_color: Option<Color>,
    /// null | "grass" | {procedural: grass|gravel|pavers|mulch|water|brick|timber|noise, seed?} | {png_base64} | {jpeg_base64} | {path}
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Either<String, Obj>>")]
    pub texture: Option<Option<Either<String, Obj>>>,
    /// {kind: planar|box|cylindrical, scale: mm of one repeat as a number or [u, v], rotation: degrees, offset: [x, y] mm}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mapping: Option<Obj>,
    /// 0 to 1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness: Option<Num>,
    /// 0 to 1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic: Option<Num>,
    /// 0 to 1; below 1 is see-through, for water and glass
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<Num>,
}

// Parameters of `material.duplicate`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaterialDuplicateParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `material.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaterialDeleteParams {
    pub id: ResourceRef,
}

// Parameters of `material.add_defaults`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaterialAddDefaultsParams {}

// Parameters of `material.import_texture`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["path"]}, {"required": ["png_base64"]}, {"required": ["jpeg_base64"]}]}]))]
pub struct MaterialImportTextureParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub png_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jpeg_base64: Option<String>,
    /// default 1024, at most 2048
    #[schemars(extend("default" = 1024))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_side: Option<Int>,
}

// Parameters of `material.assign`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaterialAssignParams {
    /// a material (ID or name), or null to clear the slot
    #[serde(deserialize_with = "required_nullable")]
    #[schemars(with = "Nullable<ResourceRef>")]
    pub material: Option<ResourceRef>,
    /// a target: one object
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectRef>,
    /// or several objects
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objects: Option<Vec<ObjectRef>>,
    /// or a plant style or hardscape style (ID or name)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ResourceRef>,
    /// or a class
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    /// a part: trunk, canopy, surface, component:N
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
}

// Parameters of `material.slots`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaterialSlotsParams {}

// Parameters of `render_style.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderStyleListParams {}

// Parameters of `render_style.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderStyleGetParams {
    pub id: ResourceRef,
}

// Parameters of `render_style.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderStyleCreateParams {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<RenderStyleCreateMode>,
    /// draw edges on shaded surfaces
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<bool>,
    /// needs sun
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadows: Option<bool>,
    /// light with the sun of the document, see sun.set
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sun: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub textures: Option<bool>,
    /// [r,g,b,a]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Color>,
    /// mm on paper, for hidden line
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_weight: Option<Num>,
}

// Parameters of `render_style.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderStyleUpdateParams {
    pub id: ResourceRef,
    /// renames
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<RenderStyleCreateMode>,
    /// draw edges on shaded surfaces
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<bool>,
    /// needs sun
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadows: Option<bool>,
    /// light with the sun of the document, see sun.set
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sun: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub textures: Option<bool>,
    /// [r,g,b,a]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<Color>,
    /// mm on paper, for hidden line
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_weight: Option<Num>,
}

// Parameters of `render_style.duplicate`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderStyleDuplicateParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `render_style.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderStyleDeleteParams {
    pub id: ResourceRef,
}

// Parameters of `render_style.set_active`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderStyleSetActiveParams {
    /// null clears the active style
    #[serde(deserialize_with = "required_nullable")]
    #[schemars(with = "Nullable<ResourceRef>")]
    pub id: Option<ResourceRef>,
}

// Parameters of `sun.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SunSetParams {
    /// degrees north
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latitude: Option<Num>,
    /// degrees east
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub longitude: Option<Num>,
    /// take latitude and longitude from the georeference
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_georef: Option<bool>,
    /// "YYYY-MM-DD"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<Value>,
    /// "HH:MM" local clock time
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<Value>,
    /// standard time, hours from UTC; default: the nominal zone of the longitude
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tz_offset: Option<Value>,
    /// true when summer time is in force: the clock is one hour ahead of tz_offset
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dst: Option<bool>,
    /// degrees clockwise from the model +Y axis to true north; from_georef uses the rotation of the georeference and the meridian convergence of its coordinate system
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub north_angle: Option<Num>,
    /// true removes the sun
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off: Option<bool>,
}

// Parameters of `sun.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SunGetParams {
    /// list of "HH:MM" for a sun path on the same date
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub times: Option<Value>,
}

// Parameters of `group.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupCreateParams {
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `group.ungroup`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupUngroupParams {
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
}

// Parameters of `group.enter`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupEnterParams {
    pub id: ObjectRef,
}

// Parameters of `group.exit`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupExitParams {}

// Parameters of `group.context`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupContextParams {}

// Parameters of `group.members`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupMembersParams {
    pub id: ObjectRef,
    /// all levels
    #[schemars(extend("default" = false))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deep: Option<bool>,
}

// Parameters of `group.move_into`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupMoveIntoParams {
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    pub group: ObjectRef,
}

// Parameters of `group.move_out`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupMoveOutParams {
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
}

// Parameters of `group.move`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupMoveParams {
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    pub delta: Point,
}

// Parameters of `group.rotate`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupRotateParams {
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    pub center: Point,
    /// radians, counterclockwise
    pub angle: Num,
}

// Parameters of `group.copy`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupCopyParams {
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<Point>,
}

// Parameters of `selection.click`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectionClickParams {
    pub id: ObjectRef,
    /// toggle in the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub add: Option<bool>,
}

// Parameters of `symbol.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolListParams {}

// Parameters of `symbol.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolGetParams {
    pub id: ResourceRef,
}

// Parameters of `symbol.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolCreateParams {
    pub name: String,
    #[schemars(extend("default" = "world"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units: Option<SymbolCreateUnits>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_class: Option<ClassRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate_on_insert: Option<bool>,
    /// [{shape, attrs?, class?}]: 2D components in definition coordinates
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<Obj>>,
    /// [{kind: extrusion, outline, base?, height, color?} | ...]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solids: Option<Vec<Obj>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insertion: Option<Point>,
}

// Parameters of `symbol.from_objects`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolFromObjectsParams {
    pub name: String,
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    /// default: the center
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insertion: Option<Point>,
    /// replace the objects by one instance
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace: Option<bool>,
}

// Parameters of `symbol.place`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolPlaceParams {
    pub symbol: ResourceRef,
    pub at: Point,
    /// radians
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate_to: Option<Point>,
    #[schemars(extend("default" = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_y: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
}

// Parameters of `symbol.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["ids"]}]}]))]
pub struct SymbolSetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Num>,
    /// null makes the scale symmetric again
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Num>")]
    pub scale_y: Option<Option<Num>>,
    /// another definition
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<ResourceRef>,
}

// Parameters of `symbol.convert_to_group`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolConvertToGroupParams {
    /// default: the selection
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
}

// Parameters of `symbol.edit`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolEditParams {
    /// an instance
    pub id: ObjectRef,
}

// Parameters of `symbol.edit_end`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SymbolEditEndParams {
    /// default: the edit group you are in
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<ObjectRef>,
}

// Parameters of `resource.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceListParams {
    /// plant_style, hardscape_style, landscape_area_style, record_format, data_map, worksheet, symbol, hatch, tile, line_type, texture, ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    /// part of the name or type
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    /// adds an SVG of each
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnails: Option<bool>,
}

// Parameters of `resource.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceGetParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
}

// Parameters of `resource.rename`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceRenameParams {
    pub id: ResourceRef,
    pub name: String,
}

// Parameters of `resource.duplicate`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceDuplicateParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `resource.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceDeleteParams {
    pub id: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reassign: Option<ResourceRef>,
}

// Parameters of `resource.purge_unused`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourcePurgeUnusedParams {
    /// default: all but worksheets and data maps
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
}

// Parameters of `resource.thumbnail`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceThumbnailParams {
    pub id: ResourceRef,
    /// px
    #[schemars(extend("default" = 64))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Int>,
}

// Parameters of `pattern.segments`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatternSegmentsParams {
    pub pattern: ResourceRef,
    pub ring: Vec<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holes: Option<Vec<Vec<Point>>>,
    /// model mm per paper mm
    #[schemars(extend("default" = 100))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Num>,
}

// Parameters of `resource.list_file`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["path"]}, {"required": ["library"]}]}]))]
pub struct ResourceListFileParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// a file in the library folder
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
}

// Parameters of `resource.import_from_file`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["path"]}, {"required": ["library"]}]}]))]
pub struct ResourceImportFromFileParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
    /// [{type, name}] or [name]; default: everything when all is true
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Either<Vec<Obj>, Vec<String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
    #[schemars(extend("default" = "rename"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_conflict: Option<ResourceImportFromFileOnConflict>,
    /// names
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classes: Option<Vec<String>>,
    /// names
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layers: Option<Vec<String>>,
}

// Parameters of `resource.export_to_file`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["path"]}, {"required": ["library"]}]}]))]
pub struct ResourceExportToFileParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
    /// default: all
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ResourceRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overwrite: Option<bool>,
}

// Parameters of `library.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryListParams {}

// Parameters of `library.favorites`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryFavoritesParams {}

// Parameters of `library.favorite_add`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryFavoriteAddParams {
    pub library: String,
    pub r#type: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

// Parameters of `library.favorite_remove`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LibraryFavoriteRemoveParams {
    pub library: String,
    pub r#type: String,
    pub name: String,
}

// Parameters of `objects.query`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObjectsQueryParams {
    /// the criteria language of worksheet database blocks: Kind = "plant" AND Area > 20
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<String>,
    /// expressions, e.g. "Area", "Style"; their values are returned per object
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[schemars(extend("default" = 1000))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<Int>,
    /// true selects the matches, except objects of grey layers and classes
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<bool>,
}

entries! {
    "objects.list" => ObjectsListParams,
    "objects.get" => ObjectsGetParams,
    "selection.get" => SelectionGetParams,
    "selection.set" => SelectionSetParams,
    "record_format.list" => RecordFormatListParams,
    "record_format.get" => RecordFormatGetParams,
    "record_format.create" => RecordFormatCreateParams,
    "record_format.update" => RecordFormatUpdateParams,
    "record_format.delete" => RecordFormatDeleteParams,
    "record.attach" => RecordAttachParams,
    "record.detach" => RecordDetachParams,
    "record.set" => RecordSetParams,
    "record.get" => RecordGetParams,
    "data_manager.list" => DataManagerListParams,
    "data_manager.get" => DataManagerGetParams,
    "data_manager.create" => DataManagerCreateParams,
    "data_manager.update" => DataManagerUpdateParams,
    "data_manager.delete" => DataManagerDeleteParams,
    "data_manager.check" => DataManagerCheckParams,
    "data_manager.evaluate" => DataManagerEvaluateParams,
    "data_manager.apply" => DataManagerApplyParams,
    "data_manager.properties" => DataManagerPropertiesParams,
    "material.list" => MaterialListParams,
    "material.get" => MaterialGetParams,
    "material.create" => MaterialCreateParams,
    "material.update" => MaterialUpdateParams,
    "material.duplicate" => MaterialDuplicateParams,
    "material.delete" => MaterialDeleteParams,
    "material.add_defaults" => MaterialAddDefaultsParams,
    "material.import_texture" => MaterialImportTextureParams,
    "material.assign" => MaterialAssignParams,
    "material.slots" => MaterialSlotsParams,
    "render_style.list" => RenderStyleListParams,
    "render_style.get" => RenderStyleGetParams,
    "render_style.create" => RenderStyleCreateParams,
    "render_style.update" => RenderStyleUpdateParams,
    "render_style.duplicate" => RenderStyleDuplicateParams,
    "render_style.delete" => RenderStyleDeleteParams,
    "render_style.set_active" => RenderStyleSetActiveParams,
    "sun.set" => SunSetParams,
    "sun.get" => SunGetParams,
    "group.create" => GroupCreateParams,
    "group.ungroup" => GroupUngroupParams,
    "group.enter" => GroupEnterParams,
    "group.exit" => GroupExitParams,
    "group.context" => GroupContextParams,
    "group.members" => GroupMembersParams,
    "group.move_into" => GroupMoveIntoParams,
    "group.move_out" => GroupMoveOutParams,
    "group.move" => GroupMoveParams,
    "group.rotate" => GroupRotateParams,
    "group.copy" => GroupCopyParams,
    "selection.click" => SelectionClickParams,
    "symbol.list" => SymbolListParams,
    "symbol.get" => SymbolGetParams,
    "symbol.create" => SymbolCreateParams,
    "symbol.from_objects" => SymbolFromObjectsParams,
    "symbol.place" => SymbolPlaceParams,
    "symbol.set" => SymbolSetParams,
    "symbol.convert_to_group" => SymbolConvertToGroupParams,
    "symbol.edit" => SymbolEditParams,
    "symbol.edit_end" => SymbolEditEndParams,
    "resource.list" => ResourceListParams,
    "resource.get" => ResourceGetParams,
    "resource.rename" => ResourceRenameParams,
    "resource.duplicate" => ResourceDuplicateParams,
    "resource.delete" => ResourceDeleteParams,
    "resource.purge_unused" => ResourcePurgeUnusedParams,
    "resource.thumbnail" => ResourceThumbnailParams,
    "pattern.segments" => PatternSegmentsParams,
    "resource.list_file" => ResourceListFileParams,
    "resource.import_from_file" => ResourceImportFromFileParams,
    "resource.export_to_file" => ResourceExportToFileParams,
    "library.list" => LibraryListParams,
    "library.favorites" => LibraryFavoritesParams,
    "library.favorite_add" => LibraryFavoriteAddParams,
    "library.favorite_remove" => LibraryFavoriteRemoveParams,
    "objects.query" => ObjectsQueryParams,
}
