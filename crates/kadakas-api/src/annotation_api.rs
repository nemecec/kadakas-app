//! Agent API for annotation: dimensions, callouts, data tags, legends, north arrows, scale bars,
//! revision clouds, styled text, and their resources (text styles, dimension styles, tag styles and
//! notes databases).
//!
//! All changes are commands, so they can be undone. An annotation is an object of kind
//! `annotation` on a design layer or a sheet layer, or an item in the annotation space of a
//! viewport (`viewport` param, positions in paper mm from the lower left corner of the frame).
//! The sizes of annotations are paper mm. On a design layer they grow with the scale of the layer.

use kadakas_core::uid::{AsUid, Uid};
use serde_json::{Map, Value, json};

use glam::DVec2;
use kadakas_annot as annot;
use kadakas_core::annot::draw::{self, Ctx};
use kadakas_core::annot::style::AnnotResource;
use kadakas_core::annot::{Annot, DimMode, TYPE_NAMES, anchor_point, dim};
use kadakas_core::command::Command;
use kadakas_core::model::{
    Attributes, ClassId, Document, LayerId, LayerKind, NONE_CLASS, Object, ObjectId, Shape,
};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_core::sheets::{Annotation, AnnotationItem, Viewport};

use crate::errors::{app, bad};
use crate::{RpcError, Session};

macro_rules! style_methods {
    ($ty:literal, $what:literal, $list:ident, $get:ident, $create:ident, $update:ident, $delete:ident) => {
        pub const $list: (&str, &str) = (concat!($ty, ".list"), concat!("The ", $what, "s of the document. Returns {items: [{id, name, ...fields, used_by}]}."));
        pub const $get: (&str, &str) = (concat!($ty, ".get"), concat!("One ", $what, ". params: {id} (the ID or the name)."));
        pub const $create: (&str, &str) = (concat!($ty, ".create"), concat!("Create a ", $what, ". params: {name, ...fields}; see describe.annotation.resources for the fields. Returns {id}."));
        pub const $update: (&str, &str) = (concat!($ty, ".update"), concat!("Change a ", $what, ". params: {id, name?, ...fields}. Missing fields stay. Annotations that use it follow the change."));
        pub const $delete: (&str, &str) = (concat!($ty, ".delete"), concat!("Delete a ", $what, ". params: {id, reassign?}. Fails while annotations use it, unless reassign names another one."));
    };
}

style_methods!(
    "text_style",
    "text style",
    TEXT_STYLE_LIST,
    TEXT_STYLE_GET,
    TEXT_STYLE_CREATE,
    TEXT_STYLE_UPDATE,
    TEXT_STYLE_DELETE
);
style_methods!(
    "dimension_style",
    "dimension style (dimension standard)",
    DIMENSION_STYLE_LIST,
    DIMENSION_STYLE_GET,
    DIMENSION_STYLE_CREATE,
    DIMENSION_STYLE_UPDATE,
    DIMENSION_STYLE_DELETE
);
style_methods!(
    "tag_style",
    "data tag style",
    TAG_STYLE_LIST,
    TAG_STYLE_GET,
    TAG_STYLE_CREATE,
    TAG_STYLE_UPDATE,
    TAG_STYLE_DELETE
);
style_methods!(
    "notes_db",
    "notes database",
    NOTES_DB_LIST,
    NOTES_DB_GET,
    NOTES_DB_CREATE,
    NOTES_DB_UPDATE,
    NOTES_DB_DELETE
);

pub const DIMENSION_ADD: (&str, &str) = (
    "dimension.add",
    "Add a dimension. params: {mode, points?, anchors?, object?, angle?, offset?, style?, text?, layer?, class?, viewport?}. mode: horizontal, vertical, aligned, chain_horizontal|vertical|aligned, baseline_horizontal|vertical|aligned, ordinate_horizontal|vertical, angular, radial, diameter, arc_length, area. points: [[x, y]] (2 for linear, radial and diameter; 3 for angular and arc_length, in that order: vertex, first ray, second ray; center, start, end; 3 or more for chain and baseline; datum first for ordinate). anchors: one entry per point, null or {object, vertex: i} | {object, edge: i, t?} | {object, center: true}. With anchors the dimension is associative: it follows the objects, and when an object is deleted the dimension keeps its last position. With anchors and no points, the points come from the anchors. For radial, diameter and arc_length give object (a circle or an arc) and angle (radians, default 0.78) instead of points. An area dimension (mode area) needs object, a closed shape (rectangle, circle, closed polyline, landscape area, hardscape, site modifier). It shows the area in the unit of the dimension style (area_unit: m2|ha|ft2|ac, area_precision, area_show_unit, area_round_to), sits at the middle of the shape, and follows the shape: when the shape changes, the area updates (a viewport is not supported). offset is then the paper mm that the label moves up (default 0). offset is the distance of the dimension line from the first point in paper mm (default 8; for angular and arc_length the radius or the distance from the arc). Returns {id, mode, values, texts, points}; values are real mm (real mm2 for area, degrees for angular).",
);
pub const CALLOUT_ADD: (&str, &str) = (
    "callout.add",
    "Add a callout: text with leader lines. params: {at, leaders?: [{points: [[x, y]] (tip first), object?, vertex?|edge?|center?}], tip?: [x, y] (one leader), object?, text?, note?, notes_db?, style?, shoulder_mm?, terminator?: arrow|open_arrow|tick|dot|none, layer?, class?, viewport?}. at is where the leaders end and the shoulder starts. With note and notes_db the text comes from the notes database. A leader with object follows it. Returns {id, text}.",
);
pub const DATA_TAG_ADD: (&str, &str) = (
    "data_tag.add",
    "Tag one object. params: {style, object, offset?: [dx, dy] (paper mm from the object, default [8, 8]), layer?, viewport?}. The tag shows the lines of the tag style, calculated live from the object. Returns {id, lines}.",
);
pub const DATA_TAG_TAG_ALL: (&str, &str) = (
    "data_tag.tag_all",
    "Tag every object that matches the criteria, as one undo step. params: {style, criteria?, layer?, offset?}. criteria is a worksheet expression such as Kind = \"plant\". Objects that have a tag of this style are left out. Returns {tagged, ids, skipped}.",
);
pub const LEGEND_ADD: (&str, &str) = (
    "legend.add",
    "Add a graphic legend that updates live. params: {at (top left), title?, criteria?, group_by? (expression, default Style), show_count?, swatch_mm?, style?, layer?, class?, viewport?}. Returns {id, rows: [{label, count}]}.",
);
pub const NORTH_ARROW_ADD: (&str, &str) = (
    "north_arrow.add",
    "Add a north arrow. params: {at, size_mm?, rotation_deg?, use_georef?, arrow_style?: arrow|compass|simple, layer?, class?, viewport?}. Returns {id, rotation_deg}.",
);
pub const SCALE_BAR_ADD: (&str, &str) = (
    "scale_bar.add",
    "Add a scale bar that follows the scale of the layer or viewport. params: {at, unit?: m|mm|ft, segment? (length of one segment in the unit, default 5), segments?, height_mm?, scale? (denominator, instead of the one of the layer), style?, layer?, class?, viewport?}. Returns {id, length_mm, length_paper_mm}.",
);
pub const REVISION_CLOUD_ADD: (&str, &str) = (
    "revision_cloud.add",
    "Add a revision cloud around changes. params: {points: [[x, y]], bump_mm?, revision, date?, description?, author?, mark?, layer?, class?, viewport?}. Returns {id}.",
);
pub const ANNOTATION_TEXT: (&str, &str) = (
    "annotation.text",
    "Add text with a text style. The size is paper mm, so it follows the scale. params: {at, text, style?, layer?, class?, viewport?}. Lines are separated by a newline. Returns {id}.",
);
pub const ANNOTATION_UPDATE: (&str, &str) = (
    "annotation.update",
    "Change an annotation object. params: {id, ...fields of the annotation} (style names are accepted). Missing fields stay. Returns the annotation with its computed values.",
);
pub const ANNOTATION_LIST: (&str, &str) = (
    "annotation.list",
    "List annotation objects with their computed texts. params: {layer?, type?: dimension|text|callout|data_tag|legend|north_arrow|scale_bar|revision_cloud}. Returns {items: [{id, layer, type, annotation, computed}]}.",
);
pub const ANNOTATION_EVALUATE: (&str, &str) = (
    "annotation.evaluate",
    "The computed values of one annotation: dimension texts and values, tag lines, legend rows, scale bar length. params: {id}. Tag lines and legend rows come from worksheet expressions over the live document.",
);
pub const ANNOTATION_APPLY_TEXT_STYLE: (&str, &str) = (
    "annotation.apply_text_style",
    "Apply a text style to text, callouts, legends and scale bars. Plain text objects get the paper size of the style at the scale of their layer. params: {ids, style}.",
);
pub const REVISION_LIST: (&str, &str) = (
    "revision.list",
    "The revisions of the revision clouds, sorted by mark. Returns {revisions: [{revision, date, description, author, clouds}]}.",
);

/// All methods of this module, for `describe` and the MCP tool list.
pub const METHODS: &[(&str, &str)] = &[
    TEXT_STYLE_LIST,
    TEXT_STYLE_GET,
    TEXT_STYLE_CREATE,
    TEXT_STYLE_UPDATE,
    TEXT_STYLE_DELETE,
    DIMENSION_STYLE_LIST,
    DIMENSION_STYLE_GET,
    DIMENSION_STYLE_CREATE,
    DIMENSION_STYLE_UPDATE,
    DIMENSION_STYLE_DELETE,
    TAG_STYLE_LIST,
    TAG_STYLE_GET,
    TAG_STYLE_CREATE,
    TAG_STYLE_UPDATE,
    TAG_STYLE_DELETE,
    NOTES_DB_LIST,
    NOTES_DB_GET,
    NOTES_DB_CREATE,
    NOTES_DB_UPDATE,
    NOTES_DB_DELETE,
    DIMENSION_ADD,
    CALLOUT_ADD,
    DATA_TAG_ADD,
    DATA_TAG_TAG_ALL,
    LEGEND_ADD,
    NORTH_ARROW_ADD,
    SCALE_BAR_ADD,
    REVISION_CLOUD_ADD,
    ANNOTATION_TEXT,
    ANNOTATION_UPDATE,
    ANNOTATION_LIST,
    ANNOTATION_EVALUATE,
    ANNOTATION_APPLY_TEXT_STYLE,
    REVISION_LIST,
];

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "concepts": "Sizes of annotations (text, arrows, gaps, symbols) are paper mm. On a design layer they grow with the scale of the layer, so they print at the same size at any scale. In the annotation space of a viewport, positions are paper mm from the lower left corner of the frame, and real lengths (dimension values, scale bars) use the scale of the viewport. Dimensions, callouts and data tags are associative: they follow the objects they are tied to.",
        "resources": {
            "text_style": "{name, font?, size_mm? (paper), bold?, italic?, color?, justify?: left|center|right, line_spacing?}",
            "dimension_style": "{name, font?, text_size_mm?, unit?: mm|cm|m|in|ft|ft_in, precision? (decimals), show_unit?, angle_precision?, terminator?: arrow|open_arrow|tick|dot|none, terminator_size_mm?, ext_gap_mm?, ext_overshoot_mm?, baseline_spacing_mm?, text_gap_mm?, prefix?, suffix?, tolerance?: {plus, minus}, color?}",
            "tag_style": "{name, lines: [text with fields in braces], font?, size_mm?, frame?: none|box|underline|circle, leader?, terminator?, pad_mm?}. A field is a worksheet expression over the properties of the tagged object (the names of worksheet.databases): {IdCode}, {Botanical}, {ROUND(Area, 1)} m2, {Plant.spread}, {Soil.Depth}, {IF(Count > 1, Count, \"\")}.",
            "notes_db": "{name, notes: [{key, text}]} or {name, notes: {key: text}}. Callouts with note and notes_db show the text of the note."
        },
        "examples": [
            {"method": "dimension_style.create", "params": {"name": "Metric 1:100", "unit": "m", "precision": 2, "show_unit": true, "terminator": "tick"}},
            {"method": "dimension.add", "params": {"mode": "horizontal", "anchors": [{"object": "00000000-0000-0000-0000-000000000005", "vertex": 0}, {"object": "00000000-0000-0000-0000-000000000005", "vertex": 1}], "offset": 10, "style": "Metric 1:100"}},
            {"method": "dimension.add", "params": {"mode": "radial", "object": "00000000-0000-0000-0000-000000000007"}},
            {"method": "tag_style.create", "params": {"name": "Plant tag", "lines": ["{IdCode}", "{Botanical}"], "frame": "box"}},
            {"method": "data_tag.tag_all", "params": {"style": "Plant tag", "criteria": "Kind = \"plant\""}},
            {"method": "legend.add", "params": {"at": [20, 280], "title": "Plants", "criteria": "Kind = \"plant\"", "group_by": "Style"}},
            {"method": "north_arrow.add", "params": {"at": [380, 260], "size_mm": 15}},
            {"method": "scale_bar.add", "params": {"at": [20, 20], "unit": "m", "segment": 5, "segments": 4}},
            {"method": "revision_cloud.add", "params": {"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "revision": "A", "description": "Path moved"}},
            {"method": "notes_db.create", "params": {"name": "General notes", "notes": {"N1": "Verify on site"}}},
            {"method": "callout.add", "params": {"at": [8000, 6000], "tip": [5000, 3000], "note": "N1", "notes_db": "General notes"}}
        ],
    })
}

type Res = Result<Value, RpcError>;

const STYLE_TYPES: [(&str, &str); 4] = [
    ("text_style", "text_style"),
    ("dimension_style", "dimension_style"),
    ("tag_style", "tag_style"),
    ("notes_db", "notes_database"),
];

/// Methods that this module handles.
pub fn handles(method: &str) -> bool {
    METHODS.iter().any(|(m, _)| *m == method)
}

pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Res> {
    if !handles(method) {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    Some(run(s, method, &p))
}

fn run(s: &mut Session, method: &str, p: &Value) -> Res {
    let (head, tail) = method.split_once('.').expect("methods have a dot");
    if let Some((_, ty)) = STYLE_TYPES.iter().find(|(h, _)| *h == head) {
        return match tail {
            "list" => style_list(s.editor.doc(), ty),
            "get" => style_get(s.editor.doc(), ty, p),
            "create" => style_create(s, ty, p),
            "update" => style_update(s, ty, p),
            _ => style_delete(s, ty, p),
        };
    }
    match method {
        "dimension.add" => add(s, "dimension", p),
        "callout.add" => add(s, "callout", p),
        "data_tag.add" => add(s, "data_tag", p),
        "data_tag.tag_all" => tag_all(s, p),
        "legend.add" => add(s, "legend", p),
        "north_arrow.add" => add(s, "north_arrow", p),
        "scale_bar.add" => add(s, "scale_bar", p),
        "revision_cloud.add" => add(s, "revision_cloud", p),
        "annotation.text" => add(s, "text", p),
        "annotation.update" => update(s, p),
        "annotation.list" => list(s.editor.doc(), p),
        "annotation.evaluate" => evaluate(s.editor.doc(), p),
        "annotation.apply_text_style" => apply_text_style(s, p),
        _ => revision_list(s.editor.doc()),
    }
}

// ---------------------------------------------------------------- lookups

/// A resource of one type from an ID or a name.
fn resource_ref(doc: &Document, v: &Value, ty: &str) -> Result<ResourceId, RpcError> {
    let found = match v {
        Value::String(name) => kadakas_core::uid::Uid::parse(name.trim())
            .map(ResourceId)
            .filter(|id| {
                doc.resources
                    .get(*id)
                    .is_some_and(|r| r.body.type_name() == ty)
            })
            .or_else(|| doc.resources.find_annot(ty, name)),
        _ => None,
    };
    found.ok_or_else(|| bad(format!("no {} {v}", ty.replace('_', " "))))
}

fn layer_ref(doc: &Document, v: Option<&Value>) -> Result<LayerId, RpcError> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(doc.active_layer);
    };
    let found = match v {
        Value::String(t) => kadakas_core::uid::Uid::parse(t.trim())
            .and_then(|i| doc.layer(LayerId(i)))
            .or_else(|| {
                doc.layers
                    .iter()
                    .find(|l| l.name.eq_ignore_ascii_case(t.trim()))
            })
            .map(|l| l.id),
        _ => None,
    };
    found.ok_or_else(|| bad(format!("no layer {v}")))
}

fn class_ref(doc: &Document, v: Option<&Value>) -> Result<Option<ClassId>, RpcError> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let found = match v {
        Value::String(t) => kadakas_core::uid::Uid::parse(t.trim())
            .and_then(|i| doc.class(ClassId(i)))
            .or_else(|| {
                doc.classes
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(t.trim()))
            })
            .map(|c| c.id),
        _ => None,
    };
    found.map(Some).ok_or_else(|| bad(format!("no class {v}")))
}

fn object_param(p: &Value, key: &str) -> Result<ObjectId, RpcError> {
    p.get(key)
        .and_then(AsUid::as_uid)
        .map(ObjectId)
        .ok_or_else(|| bad(format!("params.{key} is required (an object ID)")))
}

fn annot_of(doc: &Document, id: ObjectId) -> Result<(&Object, &Annot), RpcError> {
    let o = doc
        .object(id)
        .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
    match &o.shape {
        Shape::Annotation(a) => Ok((o, a)),
        _ => Err(bad(format!("object {id} is not an annotation"))),
    }
}

// ---------------------------------------------------------------- resources

fn style_json(doc: &Document, id: ResourceId) -> Value {
    let r = doc.resources.get(id).expect("exists");
    let mut v = serde_json::to_value(&r.body).unwrap_or_default();
    if let Some(m) = v.as_object_mut() {
        m.remove("type");
        m.insert("id".into(), json!(id));
        m.insert("name".into(), json!(r.name));
        let used = doc
            .objects
            .values()
            .filter(|o| matches!(&o.shape, Shape::Annotation(a) if a.uses_resource(id)))
            .count();
        m.insert("used_by".into(), json!(used));
    }
    v
}

fn style_list(doc: &Document, ty: &str) -> Res {
    let items: Vec<Value> = doc
        .resources
        .iter()
        .filter(|r| r.body.type_name() == ty)
        .map(|r| style_json(doc, r.id))
        .collect();
    Ok(json!({"items": items}))
}

fn style_get(doc: &Document, ty: &str, p: &Value) -> Res {
    let id = resource_ref(doc, p.get("id").unwrap_or(&Value::Null), ty)?;
    Ok(style_json(doc, id))
}

/// The body of a resource from the params: all keys but `id` and `name`.
fn body_fields(ty: &str, p: &Value) -> Map<String, Value> {
    let mut m: Map<String, Value> = p
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(k, _)| !["id", "name", "reassign"].contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    // A notes database may be given as {key: text}.
    if ty == "notes_database"
        && let Some(Value::Object(map)) = m.get("notes")
    {
        let list: Vec<Value> = map
            .iter()
            .map(|(k, v)| json!({"key": k, "text": v}))
            .collect();
        m.insert("notes".into(), Value::Array(list));
    }
    m
}

fn style_body(ty: &str, fields: Map<String, Value>) -> Result<ResourceBody, RpcError> {
    let mut m = fields;
    m.insert("type".into(), json!(ty));
    let r: AnnotResource = serde_json::from_value(Value::Object(m)).map_err(bad)?;
    Ok(ResourceBody::annot(r))
}

fn style_create(s: &mut Session, ty: &str, p: &Value) -> Res {
    let name = p
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("params.name is required (text)"))?
        .to_string();
    let body = style_body(ty, body_fields(ty, p))?;
    let out = s
        .editor
        .execute(&Command::AddResource { name, body })
        .map_err(RpcError::from)?;
    Ok(json!({"id": out.created[0]}))
}

fn style_update(s: &mut Session, ty: &str, p: &Value) -> Res {
    let doc = s.editor.doc();
    let id = resource_ref(doc, p.get("id").unwrap_or(&Value::Null), ty)?;
    let current =
        serde_json::to_value(&doc.resources.get(id).expect("exists").body).map_err(app)?;
    let mut m = current.as_object().cloned().unwrap_or_default();
    m.remove("type");
    for (k, v) in body_fields(ty, p) {
        m.insert(k, v);
    }
    let body = style_body(ty, m)?;
    let name = p.get("name").and_then(Value::as_str).map(str::to_string);
    s.editor
        .execute(&Command::ModifyResource {
            id,
            name,
            body: Some(body),
        })
        .map_err(RpcError::from)?;
    Ok(style_json(s.editor.doc(), id))
}

fn style_delete(s: &mut Session, ty: &str, p: &Value) -> Res {
    let doc = s.editor.doc();
    let id = resource_ref(doc, p.get("id").unwrap_or(&Value::Null), ty)?;
    let reassign = match p.get("reassign") {
        None | Some(Value::Null) => None,
        Some(v) => Some(resource_ref(doc, v, ty)?),
    };
    s.editor
        .execute(&Command::DeleteResource { id, reassign })
        .map_err(RpcError::from)?;
    Ok(json!({"ok": true}))
}

// ---------------------------------------------------------------- adding

/// Which resource type a `style` key of an annotation type refers to.
fn style_type_of(ty: &str) -> &'static str {
    match ty {
        "dimension" => "dimension_style",
        "data_tag" => "tag_style",
        _ => "text_style",
    }
}

/// An anchor from `{object, vertex}`, `{object, edge, t?}` or `{object, center: true}`.
fn anchor_json(v: &Value) -> Result<Value, RpcError> {
    if v.is_null() {
        return Ok(Value::Null);
    }
    if v.get("target").is_some() {
        return Ok(v.clone());
    }
    let object = v
        .get("object")
        .and_then(AsUid::as_uid)
        .ok_or_else(|| bad("an anchor needs object (an object ID)"))?;
    let target = if let Some(i) = v.get("vertex").and_then(Value::as_u64) {
        json!({"at": "vertex", "index": i})
    } else if let Some(i) = v.get("edge").and_then(Value::as_u64) {
        json!({"at": "edge", "index": i, "t": v.get("t").and_then(Value::as_f64).unwrap_or(0.5)})
    } else if v.get("center").is_some() {
        json!({"at": "center"})
    } else {
        return Err(bad("an anchor needs vertex, edge or center"));
    };
    Ok(json!({"object": object, "target": target}))
}

fn points_of(doc: &Document, anchors: &[Value]) -> Result<Vec<Value>, RpcError> {
    anchors
        .iter()
        .map(|a| {
            let anchor: kadakas_core::annot::Anchor = serde_json::from_value(a.clone())
                .map_err(|_| bad("the anchor needs a point or a valid object part"))?;
            anchor_point(doc, &anchor)
                .map(|p| json!([p.x, p.y]))
                .ok_or_else(|| bad(format!("object {} has no such part", anchor.object)))
        })
        .collect()
}

fn prep_dimension(
    doc: &Document,
    j: &mut Map<String, Value>,
    in_viewport: bool,
) -> Result<(), RpcError> {
    let mode_name = j
        .get("mode")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            bad(format!(
                "params.mode is required: {}",
                DimMode::NAMES.join(", ")
            ))
        })?
        .to_string();
    let mode = DimMode::from_name(&mode_name).ok_or_else(|| {
        bad(format!(
            "unknown mode '{mode_name}'. Modes: {}",
            DimMode::NAMES.join(", ")
        ))
    })?;
    j.insert("mode".into(), serde_json::to_value(mode).unwrap());
    if !j.contains_key("offset") {
        let default = if mode == DimMode::Area { 0.0 } else { 8.0 };
        j.insert("offset".into(), json!(default));
    }
    if mode == DimMode::Area && in_viewport {
        return Err(bad("an area dimension cannot go in a viewport"));
    }
    if mode == DimMode::Area && !j.contains_key("object") {
        return Err(bad(
            "an area dimension needs object (a closed shape): the area follows it",
        ));
    }
    let mut anchors: Vec<Value> = match j.get("anchors") {
        Some(Value::Array(a)) => a.iter().map(anchor_json).collect::<Result<_, _>>()?,
        Some(Value::Null) | None => Vec::new(),
        Some(_) => return Err(bad("anchors is a list")),
    };
    if in_viewport && anchors.iter().any(|a| !a.is_null()) {
        return Err(bad(
            "a dimension in a viewport takes points in paper mm, not anchors",
        ));
    }
    // A circle or an arc gives the center and the radius for radial, diameter and arc length.
    if let Some(object) = j.remove("object").and_then(|v| v.as_uid()).map(ObjectId) {
        let shape = &doc
            .object(object)
            .ok_or_else(|| RpcError::not_found(format!("object {object} does not exist")))?
            .shape;
        let angle = j.remove("angle").and_then(|v| v.as_f64()).unwrap_or(0.78);
        let pts = match (mode, shape) {
            (DimMode::Area, shape) => {
                let (Some(area), Some(at)) = (shape.area(), kadakas_core::annot::ref_point(shape))
                else {
                    return Err(bad("object must be a closed shape (it has no area)"));
                };
                j.insert("area".into(), json!(area));
                vec![at]
            }
            (
                DimMode::Radial | DimMode::Diameter,
                Shape::Circle { center, radius } | Shape::Arc { center, radius, .. },
            ) => vec![*center, *center + DVec2::from_angle(angle) * *radius],
            (
                DimMode::ArcLength,
                Shape::Arc {
                    center,
                    radius,
                    start,
                    end,
                },
            ) => {
                let sweep = kadakas_core::model::arc_sweep(*start, *end);
                vec![
                    *center,
                    *center + DVec2::from_angle(*start) * *radius,
                    *center + DVec2::from_angle(*start + sweep) * *radius,
                ]
            }
            _ => {
                return Err(bad(
                    "object must be a circle or an arc (an arc for arc_length)",
                ));
            }
        };
        j.insert("points".into(), json!(pts));
        anchors = vec![json!({"object": object, "target": {"at": "center"}})];
    }
    if !j.contains_key("points") {
        if anchors.is_empty() {
            return Err(bad("give points, or anchors, or an object"));
        }
        let pts = points_of(doc, &anchors)?;
        j.insert("points".into(), Value::Array(pts));
    }
    if !anchors.is_empty() {
        j.insert("anchors".into(), Value::Array(anchors));
    }
    Ok(())
}

fn prep_callout(doc: &Document, j: &mut Map<String, Value>) -> Result<(), RpcError> {
    if !j.contains_key("leaders") {
        let tip = j
            .remove("tip")
            .ok_or_else(|| bad("give leaders, or tip for one leader"))?;
        let mut leader = Map::new();
        leader.insert("points".into(), json!([tip]));
        for k in ["object", "vertex", "edge", "center", "t"] {
            if let Some(v) = j.remove(k) {
                leader.insert(k.into(), v);
            }
        }
        j.insert("leaders".into(), json!([Value::Object(leader)]));
    }
    let Some(Value::Array(leaders)) = j.get_mut("leaders") else {
        return Err(bad("leaders is a list"));
    };
    for l in leaders {
        let Some(m) = l.as_object_mut() else {
            return Err(bad("a leader is {points, object?, ...}"));
        };
        let Some(object) = m.remove("object") else {
            continue;
        };
        let mut spec = Map::new();
        spec.insert("object".into(), object);
        for k in ["vertex", "edge", "t", "center"] {
            if let Some(v) = m.remove(k) {
                spec.insert(k.into(), v);
            }
        }
        if !["vertex", "edge", "center"]
            .iter()
            .any(|k| spec.contains_key(*k))
        {
            spec.insert("center".into(), json!(true));
        }
        let anchor = anchor_json(&Value::Object(spec))?;
        let tip = points_of(doc, std::slice::from_ref(&anchor))?.remove(0);
        let mut points: Vec<Value> = m
            .get("points")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if points.is_empty() {
            points.push(tip);
        } else {
            points[0] = tip;
        }
        m.insert("points".into(), Value::Array(points));
        m.insert("anchor".into(), anchor);
    }
    Ok(())
}

/// Resolves the resource names of the params into IDs.
fn resolve_refs(doc: &Document, ty: &str, j: &mut Map<String, Value>) -> Result<(), RpcError> {
    // The style of a north arrow is its look (arrow, compass, simple), not a resource.
    // The API calls it `arrow_style`.
    if ty == "north_arrow" {
        if let Some(a) = j.remove("arrow_style") {
            j.insert("style".into(), a);
        } else if j.get("style").is_some_and(Value::is_number) {
            return Err(bad(
                "a north arrow has no resource style. Use arrow_style: arrow, compass or simple",
            ));
        }
        return Ok(());
    }
    if let Some(v) = j.get("style").filter(|v| !v.is_null()).cloned() {
        j.insert(
            "style".into(),
            json!(resource_ref(doc, &v, style_type_of(ty))?),
        );
    }
    if let Some(v) = j.get("notes_db").filter(|v| !v.is_null()).cloned() {
        j.insert(
            "notes_db".into(),
            json!(resource_ref(doc, &v, "notes_database")?),
        );
    }
    Ok(())
}

/// The annotation from the params of an `*.add` method.
fn build(doc: &Document, ty: &str, p: &Value, in_viewport: bool) -> Result<Annot, RpcError> {
    let mut j: Map<String, Value> = p
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(k, _)| !["layer", "class", "viewport"].contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    j.insert("type".into(), json!(ty));
    resolve_refs(doc, ty, &mut j)?;
    match ty {
        "dimension" => prep_dimension(doc, &mut j, in_viewport)?,
        "callout" => prep_callout(doc, &mut j)?,
        "data_tag" => {
            if !j.contains_key("offset") {
                j.insert("offset".into(), json!([8.0, 8.0]));
            }
            let object = object_param(p, "object")?;
            if !j.contains_key("style") {
                return Err(bad("params.style is required (a tag style)"));
            }
            let anchor = doc
                .object(object)
                .and_then(|o| kadakas_core::annot::ref_point(&o.shape));
            let anchor = anchor.ok_or_else(|| bad(format!("object {object} cannot be tagged")))?;
            let paper = doc
                .layer(layer_ref(doc, p.get("layer"))?)
                .map_or(1.0, |l| l.scale);
            let off = p
                .get("offset")
                .map_or(Some(DVec2::new(8.0, 8.0)), |v| {
                    serde_json::from_value::<DVec2>(v.clone()).ok()
                })
                .ok_or_else(|| bad("offset is [dx, dy]"))?;
            let at = anchor + off * if in_viewport { 1.0 } else { paper };
            j.insert("at".into(), json!([at.x, at.y]));
            j.insert("anchor".into(), json!([anchor.x, anchor.y]));
        }
        "scale_bar" | "legend" | "north_arrow" | "revision_cloud" | "text" => {}
        _ => unreachable!("checked by the caller"),
    }
    serde_json::from_value(Value::Object(j)).map_err(|e| bad(format!("{ty}: {e}")))
}

fn viewport_of(doc: &Document, id: ObjectId) -> Result<&Viewport, RpcError> {
    match doc.object(id).map(|o| &o.shape) {
        Some(Shape::Viewport(v)) => Ok(v),
        Some(_) => Err(bad(format!("object {id} is not a viewport"))),
        None => Err(RpcError::not_found(format!("object {id} does not exist"))),
    }
}

/// The context of an annotation in a viewport or on a layer.
fn ctx_for(doc: &Document, layer: LayerId, viewport: Option<ObjectId>) -> Result<Ctx, RpcError> {
    match viewport {
        Some(v) => Ok(Ctx::viewport(viewport_of(doc, v)?)),
        None => Ok(Ctx::layer(doc, layer)),
    }
}

fn add(s: &mut Session, ty: &str, p: &Value) -> Res {
    let doc = s.editor.doc();
    let viewport = match p.get("viewport") {
        None | Some(Value::Null) => None,
        Some(v) => Some(ObjectId(
            v.as_uid().ok_or_else(|| bad("viewport is an object ID"))?,
        )),
    };
    let layer = layer_ref(doc, p.get("layer"))?;
    let class = class_ref(doc, p.get("class"))?;
    let a = build(doc, ty, p, viewport.is_some())?;
    let ctx = ctx_for(doc, layer, viewport)?;
    let info = computed(doc, &a, &ctx);
    let id = match viewport {
        Some(vid) => {
            let mut vp = viewport_of(doc, vid)?.clone();
            let aid = vp.next_annotation_id();
            vp.annotations.push(Annotation {
                id: aid,
                class: class.unwrap_or(NONE_CLASS),
                attrs: Attributes::default(),
                item: AnnotationItem::Shape {
                    shape: Box::new(Shape::Annotation(Box::new(a.clone()))),
                },
            });
            s.editor
                .execute(&Command::ModifyObject {
                    id: vid,
                    shape: Some(Shape::Viewport(Box::new(vp))),
                    layer: None,
                    class: None,
                    attrs: None,
                })
                .map_err(RpcError::from)?;
            json!({"viewport": vid, "annotation": aid})
        }
        None => {
            // In a group you have entered, the annotation becomes a member of the group.
            let cmd = s.in_context(Command::AddObject {
                shape: Shape::Annotation(Box::new(a.clone())),
                layer: Some(layer),
                class,
                attrs: Attributes::default(),
            });
            let out = s.editor.execute(&cmd).map_err(RpcError::from)?;
            json!(out.created[0])
        }
    };
    s.prune_selection();
    // The command may have changed the annotation (anchors apply), so read the stored one.
    let stored = match (&id, viewport) {
        (Value::String(n), None) => Uid::parse(n).and_then(|i| {
            annot_of(s.editor.doc(), ObjectId(i))
                .ok()
                .map(|(_, a)| a.clone())
        }),
        _ => None,
    }
    .unwrap_or(a);
    let info = if matches!(id, Value::String(_)) {
        computed(s.editor.doc(), &stored, &ctx)
    } else {
        info
    };
    let mut out = info.as_object().cloned().unwrap_or_default();
    out.insert("id".into(), id);
    Ok(Value::Object(out))
}

// ---------------------------------------------------------------- computed values

/// The values that the annotation shows now.
fn computed(doc: &Document, a: &Annot, ctx: &Ctx) -> Value {
    match a {
        Annot::Dimension(d) => {
            let style = draw::dimension_style(doc, d);
            let vals: Vec<f64> = dim::values(d)
                .iter()
                .map(|v| {
                    if matches!(d.mode, DimMode::Angular) {
                        *v
                    } else if d.mode == DimMode::Area {
                        v / (ctx.real * ctx.real)
                    } else {
                        v / ctx.real
                    }
                })
                .collect();
            json!({"mode": d.mode.name(), "detached": d.detached, "values": vals, "texts": dim::texts(d, &style, ctx), "points": d.points})
        }
        Annot::Callout(c) => json!({"text": draw::callout_text(doc, c), "detached": c.detached}),
        Annot::DataTag(t) => {
            let lines = doc
                .resources
                .tag_style(t.style)
                .map(|st| annot::tag_lines(doc, t, st))
                .unwrap_or_default();
            json!({"lines": lines, "object_exists": doc.object(t.object).is_some()})
        }
        Annot::Legend(l) => {
            let rows: Vec<Value> = annot::legend_rows(doc, l)
                .iter()
                .map(|r| json!({"label": r.label, "count": r.count}))
                .collect();
            json!({"rows": rows})
        }
        Annot::ScaleBar(b) => {
            let len = draw::scale_bar_length(b, ctx);
            json!({"length_mm": b.real_length_mm(), "length_drawing": len, "length_paper_mm": len / ctx.paper})
        }
        Annot::NorthArrow(n) => {
            json!({"rotation_deg": draw::north_rotation_deg(doc, n)})
        }
        Annot::RevisionCloud(r) => json!({"revision": r.revision, "points": r.points.len()}),
        Annot::Text(t) => json!({"text": t.text}),
    }
}

fn item_json(doc: &Document, o: &Object, a: &Annot) -> Value {
    json!({"id": o.id, "layer": o.layer, "type": a.type_name(), "annotation": a, "computed": computed(doc, a, &Ctx::layer(doc, o.layer))})
}

fn list(doc: &Document, p: &Value) -> Res {
    let layer = match p.get("layer") {
        None | Some(Value::Null) => None,
        v => Some(layer_ref(doc, v)?),
    };
    let ty = p.get("type").and_then(Value::as_str);
    if let Some(t) = ty
        && !TYPE_NAMES.contains(&t)
    {
        return Err(bad(format!(
            "unknown type '{t}'. Types: {}",
            TYPE_NAMES.join(", ")
        )));
    }
    let items: Vec<Value> = doc
        .objects
        .values()
        .filter(|o| layer.is_none_or(|l| o.layer == l))
        .filter_map(|o| match &o.shape {
            Shape::Annotation(a) if ty.is_none_or(|t| a.type_name() == t) => {
                Some(item_json(doc, o, a))
            }
            _ => None,
        })
        .collect();
    Ok(json!({"items": items}))
}

fn evaluate(doc: &Document, p: &Value) -> Res {
    let id = object_param(p, "id")?;
    let (o, a) = annot_of(doc, id)?;
    Ok(item_json(doc, o, a))
}

fn merge(base: &mut Map<String, Value>, p: &Value) {
    for (k, v) in p.as_object().into_iter().flatten() {
        if k != "id" {
            base.insert(k.clone(), v.clone());
        }
    }
}

fn update(s: &mut Session, p: &Value) -> Res {
    let id = object_param(p, "id")?;
    let doc = s.editor.doc();
    let (_, a) = annot_of(doc, id)?;
    let ty = a.type_name();
    let mut j = serde_json::to_value(a)
        .map_err(app)?
        .as_object()
        .cloned()
        .unwrap_or_default();
    let mut fields = p.clone();
    if let Some(m) = fields.as_object_mut() {
        if m.get("mode").is_some_and(Value::is_string) {
            let name = m["mode"].as_str().unwrap_or_default().to_string();
            let mode = DimMode::from_name(&name).ok_or_else(|| {
                bad(format!(
                    "unknown mode '{name}'. Modes: {}",
                    DimMode::NAMES.join(", ")
                ))
            })?;
            m.insert("mode".into(), serde_json::to_value(mode).unwrap());
        }
        if let Some(Value::Array(anchors)) = m.get("anchors").cloned() {
            m.insert(
                "anchors".into(),
                Value::Array(anchors.iter().map(anchor_json).collect::<Result<_, _>>()?),
            );
        }
    }
    merge(&mut j, &fields);
    resolve_refs(doc, ty, &mut j)?;
    let new: Annot =
        serde_json::from_value(Value::Object(j)).map_err(|e| bad(format!("{ty}: {e}")))?;
    s.editor
        .execute(&Command::ModifyObject {
            id,
            shape: Some(Shape::Annotation(Box::new(new))),
            layer: None,
            class: None,
            attrs: None,
        })
        .map_err(RpcError::from)?;
    let doc = s.editor.doc();
    let (o, a) = annot_of(doc, id)?;
    Ok(item_json(doc, o, a))
}

fn tag_all(s: &mut Session, p: &Value) -> Res {
    let doc = s.editor.doc();
    let style = resource_ref(doc, p.get("style").unwrap_or(&Value::Null), "tag_style")?;
    let criteria = p.get("criteria").and_then(Value::as_str).unwrap_or("");
    let layer = match p.get("layer") {
        None | Some(Value::Null) => None,
        v => Some(layer_ref(doc, v)?),
    };
    let offset = p
        .get("offset")
        .map_or(Some(DVec2::new(8.0, 8.0)), |v| {
            serde_json::from_value::<DVec2>(v.clone()).ok()
        })
        .ok_or_else(|| bad("offset is [dx, dy]"))?;
    let cmds = annot::tag_all(doc, style, criteria, layer, offset).map_err(bad)?;
    let matched = annot::select(doc, criteria).map_err(bad)?.len();
    let n = cmds.len();
    if n == 0 {
        return Ok(json!({"tagged": 0, "ids": [], "skipped": matched}));
    }
    let out = s
        .editor
        .execute(&Command::Batch { commands: cmds })
        .map_err(RpcError::from)?;
    Ok(json!({"tagged": n, "ids": out.created, "skipped": matched - n}))
}

fn apply_text_style(s: &mut Session, p: &Value) -> Res {
    let doc = s.editor.doc();
    let style = resource_ref(doc, p.get("style").unwrap_or(&Value::Null), "text_style")?;
    let size = doc.resources.text_style(style).map_or(3.0, |t| t.size_mm);
    let ids: Vec<ObjectId> = serde_json::from_value(p.get("ids").cloned().unwrap_or(Value::Null))
        .map_err(|_| bad("params.ids is a list of object IDs"))?;
    let mut cmds = Vec::new();
    for id in &ids {
        let o = doc
            .object(*id)
            .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
        let shape = match &o.shape {
            Shape::Annotation(a) => {
                let mut a = (**a).clone();
                match &mut a {
                    Annot::Text(t) => t.style = Some(style),
                    Annot::Callout(c) => c.style = Some(style),
                    Annot::Legend(l) => l.style = Some(style),
                    Annot::ScaleBar(b) => b.style = Some(style),
                    _ => return Err(bad(format!("object {id} has no text style"))),
                }
                Shape::Annotation(Box::new(a))
            }
            Shape::Text { at, text, .. } => Shape::Text {
                at: *at,
                text: text.clone(),
                height: size * doc.layer(o.layer).map_or(1.0, |l| l.scale),
            },
            _ => return Err(bad(format!("object {id} is not text"))),
        };
        cmds.push(Command::ModifyObject {
            id: *id,
            shape: Some(shape),
            layer: None,
            class: None,
            attrs: None,
        });
    }
    s.editor
        .execute(&Command::Batch { commands: cmds })
        .map_err(RpcError::from)?;
    Ok(json!({"changed": ids.len()}))
}

fn revision_list(doc: &Document) -> Res {
    let revisions: Vec<Value> = annot::revisions(doc)
        .iter()
        .map(|r| json!({"revision": r.revision, "date": r.date, "description": r.description, "author": r.author, "clouds": r.clouds}))
        .collect();
    Ok(json!({"revisions": revisions}))
}

/// True if the layer can hold annotation objects.
#[allow(dead_code)]
fn layer_ok(doc: &Document, id: LayerId) -> bool {
    doc.layer(id)
        .is_some_and(|l| matches!(l.kind, LayerKind::Design | LayerKind::Sheet))
}
