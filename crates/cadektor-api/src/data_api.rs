//! Agent API for data and reports: record formats and records, the Data Manager, and worksheets.
//!
//! Methods: `record_format.list|get|create|update|delete`, `record.attach|detach|set|get`,
//! `data_manager.list|get|create|update|delete|check|evaluate|apply|properties`,
//! `worksheet.list|templates|create|get|rename|delete|set_cell|set_cells|set_database|remove_database|evaluate|export`.
//! Record formats, data maps and worksheets are document resources. All changes are commands, so
//! they can be undone.

use base64::Engine;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use cadektor_core::command::Command;
use cadektor_core::datamap::{self, DataMap, EntityMap, IfcType, MapRule};
use cadektor_core::model::{Document, Object, ObjectId};
use cadektor_core::props;
use cadektor_core::record::{self, FieldDef, FieldType, RecordFormat};
use cadektor_core::resource::{ResourceBody, ResourceId};
use cadektor_sheet::def::{CellFormat, DatabaseDef, WorksheetDef};
use cadektor_sheet::{export, templates};

use crate::errors::{app, bad};
use crate::{RpcError, Session, params};

pub const RECORD_FORMAT_LIST: (&str, &str) = (
    "record_format.list",
    "Record formats with their fields and the number of objects that use each. Returns {formats: [{id, name, fields, used_by}]}.",
);
pub const RECORD_FORMAT_GET: (&str, &str) = (
    "record_format.get",
    "One record format. params: {id} (the ID or the name).",
);
pub const RECORD_FORMAT_CREATE: (&str, &str) = (
    "record_format.create",
    "Create a record format. params: {name, fields: [{name, type, default?, unit?, choices?}]}. type is text, integer, number, boolean, dimension (mm), area (m2), choice (needs choices), date (YYYY-MM-DD), time (HH:MM or HH:MM:SS), volume (m3) or angle (degrees). Returns {id}.",
);
pub const RECORD_FORMAT_UPDATE: (&str, &str) = (
    "record_format.update",
    "Change a record format. params: {id, name?, fields?}. Attached records follow: they are renamed, fields that are gone are dropped, new fields get their defaults.",
);
pub const RECORD_FORMAT_DELETE: (&str, &str) = (
    "record_format.delete",
    "Delete a record format. params: {id}. Fails while objects have the record attached.",
);
pub const RECORD_ATTACH: (&str, &str) = (
    "record.attach",
    "Attach a record to objects. params: {ids, record (ID or name), values?: {field: value}}. Missing fields get the default. Values are checked against the field types. One undo step.",
);
pub const RECORD_DETACH: (&str, &str) = (
    "record.detach",
    "Remove a record from objects. params: {ids, record}.",
);
pub const RECORD_SET: (&str, &str) = (
    "record.set",
    "Change field values of attached records. params: {ids, record, values: {field: value}}.",
);
pub const RECORD_GET: (&str, &str) = (
    "record.get",
    "The records attached to one object. params: {id}. Returns {records: {name: {field: value}}}.",
);
pub const DATA_MANAGER_LIST: (&str, &str) = (
    "data_manager.list",
    "Data maps (mapping tables). Returns {maps: [{id, name, entities, rules}]}.",
);
pub const DATA_MANAGER_GET: (&str, &str) = (
    "data_manager.get",
    "One data map. params: {id} (ID or name).",
);
pub const DATA_MANAGER_CREATE: (&str, &str) = (
    "data_manager.create",
    "Create a data map. params: {name, template?: \"landscape\", entities?, rules?}. See describe.data_manager for the rule format. Returns {id}.",
);
pub const DATA_MANAGER_UPDATE: (&str, &str) = (
    "data_manager.update",
    "Change a data map. params: {id, name?, entities?, rules?}.",
);
pub const DATA_MANAGER_DELETE: (&str, &str) =
    ("data_manager.delete", "Delete a data map. params: {id}.");
pub const DATA_MANAGER_CHECK: (&str, &str) = (
    "data_manager.check",
    "Check a data map against the document: records and fields that do not exist. params: {id}. Returns {problems}.",
);
pub const DATA_MANAGER_EVALUATE: (&str, &str) = (
    "data_manager.evaluate",
    "Apply a data map to objects (read only). params: {id, ids?, limit?}. Returns {objects: [{id, kind, ifc_class, predefined_type, properties: {pset: {name: {ifc_type, value}}}, record_values, problems}]}. This is the data for IFC export and data tags.",
);
pub const DATA_MANAGER_APPLY: (&str, &str) = (
    "data_manager.apply",
    "Write the record targets of a data map into the records of the objects, as one undo step. Only objects that have the record attached change. params: {id}. Returns {changed, problems}.",
);
pub const DATA_MANAGER_PROPERTIES: (&str, &str) = (
    "data_manager.properties",
    "The names that rules and worksheet columns can read: built-in properties, plant style fields and record fields.",
);
pub const WORKSHEET_LIST: (&str, &str) = (
    "worksheet.list",
    "Worksheets. Returns {worksheets: [{id, name, cells, databases}]}.",
);
pub const WORKSHEET_TEMPLATES: (&str, &str) = (
    "worksheet.templates",
    "Built-in report templates: plant_list, hardscape_quantities, area_schedule, tree_survey, irrigation_heads, irrigation_pipes.",
);
pub const WORKSHEET_CREATE: (&str, &str) = (
    "worksheet.create",
    "Create a worksheet. params: {name, template?, cells?: {A1: \"text, number or =formula\"}, databases?: [database]}. See describe.worksheet. Returns {id}.",
);
pub const WORKSHEET_GET: (&str, &str) = (
    "worksheet.get",
    "The definition of a worksheet. params: {id} (ID or name). Returns {id, name, definition}.",
);
pub const WORKSHEET_RENAME: (&str, &str) = (
    "worksheet.rename",
    "Rename a worksheet. params: {id, name}.",
);
pub const WORKSHEET_DELETE: (&str, &str) =
    ("worksheet.delete", "Delete a worksheet. params: {id}.");
pub const WORKSHEET_SET_CELL: (&str, &str) = (
    "worksheet.set_cell",
    "Set one cell. params: {id, address: \"B2\", input: \"=SUM(B3:B9)\", decimals?, unit?}. An empty input clears the cell. A formula that cannot be read is refused.",
);
pub const WORKSHEET_SET_CELLS: (&str, &str) = (
    "worksheet.set_cells",
    "Set many cells as one undo step. params: {id, cells: {A1: input, ...}}.",
);
pub const WORKSHEET_SET_DATABASE: (&str, &str) = (
    "worksheet.set_database",
    "Add or replace a database block. params: {id, index?, database: {name?, anchor?, criteria?, columns: [{header, expr, summary?, decimals?, unit?}], sort?: [{column, descending?}], group_by?, group_mode?: detail|summary, header?, total?}}. Without index the block is added.",
);
pub const WORKSHEET_REMOVE_DATABASE: (&str, &str) = (
    "worksheet.remove_database",
    "Remove a database block. params: {id, index}.",
);
pub const WORKSHEET_EVALUATE: (&str, &str) = (
    "worksheet.evaluate",
    "Calculate a worksheet now. params: {id}. Returns {rows, cols, values: [[...]], text: [[...]], cells: {A1: value}, blocks, problems, errors}. An error cell is {error: \"#DIV/0!\"}. Cycles give #CYCLE!.",
);
pub const WORKSHEET_PLACE: (&str, &str) = (
    "worksheet.place",
    "Place a worksheet in the drawing as a table, on a design layer or a sheet layer (a sheet layer takes paper mm). It draws the calculated cells, prints in PDF and sheet.svg, and follows the document. params: {id, at: [x, y] (top left corner), text_height? (mm, default 3.5), layer?, class?}. Returns {id, size: [w, h]}. The frame is cut to the table size; use worksheet.fit after the worksheet changes.",
);
pub const WORKSHEET_FIT: (&str, &str) = (
    "worksheet.fit",
    "Fit the frames of worksheet tables to their calculated tables (the top left corner stays). params: {ids?: [object id]} (default: all tables). Returns {fitted}.",
);
pub const WORKSHEET_EXPORT: (&str, &str) = (
    "worksheet.export",
    "Export a calculated worksheet. params: {id, format: csv | xlsx, path?, overwrite?}. Without a path, csv returns {csv} and xlsx returns {xlsx_base64}. With a path the file is written. XLSX holds values, not formulas.",
);

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "records": "A record format is a document resource with typed fields (text, integer, number, boolean, dimension in mm, area in m2, choice, date YYYY-MM-DD, time HH:MM, volume in m3, angle in degrees). record.attach stores the values in object.data[<record name>] with a \"_record\" tag, and only tagged data counts as a record. Plant style data keeps working: the key 'plant' and 'rotation' are reserved.",
        "data_manager": {
            "summary": "A data map is a resource with two tables. entities: {kind, ifc_class, predefined_type?} gives the IFC class of each object kind. rules: {name?, kinds?: [kind], source, target}. source is {type: property, name: Area} | {type: plant_field, field: price} | {type: record_field, record, field} | {type: constant, value}. target is {type: ifc_property, pset, property, ifc_type: label|text|real|integer|boolean|length|area|volume} | {type: record_field, record, field}.",
            "properties": props::NAMES,
            "examples": [
                {"method": "data_manager.create", "params": {"name": "Landscape IFC", "template": "landscape"}},
                {"method": "data_manager.create", "params": {"name": "Takeoff", "entities": [{"kind": "hardscape", "ifc_class": "IfcPavement"}], "rules": [{"kinds": ["hardscape"], "source": {"type": "property", "name": "Area"}, "target": {"type": "ifc_property", "pset": "Cadektor_Quantities", "property": "Area", "ifc_type": "area"}}]}},
                {"method": "data_manager.evaluate", "params": {"id": "Landscape IFC", "limit": 10}}
            ]
        },
        "worksheet": {
            "cells": "Cells hold numbers, text, TRUE/FALSE or formulas that start with =. Formulas: + - * / ^ & comparison, AND OR NOT, references A1 and B2:C5, and the functions SUM AVERAGE MIN MAX COUNT COUNTA IF IFERROR ROUND ABS CONCAT AND OR NOT LEN UPPER LOWER FIELD HASRECORD.",
            "database": "A database block lists objects. criteria is an expression over object properties, for example Kind = \"plant\" AND Layer = \"Planting\" AND Soil.Depth > 300. Each column has an expression: Area (m2), Length and Perimeter (m), Volume (m3), Count, Layer, Class, Style, Material, Kind, Id, Species, Botanical, CommonName, IdCode, Size, Container, Price, Spread, Height (mm), Plant.<style field>, Record.Field or 'Record name'.'Field name'. summary (sum, average, min, max, count) adds a total row and group rows. group_by sorts by one column and groups equal values: detail mode shows every object and a subtotal, summary mode shows one row per group.",
            "units": "Area in m2, Volume in m3, Length and Perimeter in m. Other lengths in mm as stored.",
            "examples": [
                {"method": "worksheet.create", "params": {"name": "Plants", "template": "plant_list"}},
                {"method": "worksheet.create", "params": {"name": "Trees", "databases": [{"anchor": "A1", "criteria": "Kind = \"existing_tree\"", "columns": [{"header": "Species", "expr": "Species"}, {"header": "Count", "expr": "Count", "summary": "sum"}], "group_by": 0, "group_mode": "summary"}]}},
                {"method": "worksheet.set_cell", "params": {"id": "Plants", "address": "L1", "input": "=SUM(G2:G20)", "decimals": 0}},
                {"method": "worksheet.evaluate", "params": {"id": "Plants"}},
                {"method": "worksheet.export", "params": {"id": "Plants", "format": "csv"}},
                {"method": "worksheet.place", "params": {"id": "Plants", "at": [10, 280], "layer": "00000000-0000-0000-0000-000000000005"}}
            ],
            "templates": templates::TEMPLATES.iter().map(|t| json!({"id": t.id, "title": t.title, "description": t.description})).collect::<Vec<_>>(),
            "functions": cadektor_sheet::eval::FUNCTIONS.iter().map(|(n, d)| json!({"name": n, "doc": d})).collect::<Vec<_>>(),
        }
    })
}

/// Methods that this module handles.
const NAMES: &[&str] = &[
    "record_format.list",
    "record_format.get",
    "record_format.create",
    "record_format.update",
    "record_format.delete",
    "record.attach",
    "record.detach",
    "record.set",
    "record.get",
    "data_manager.list",
    "data_manager.get",
    "data_manager.create",
    "data_manager.update",
    "data_manager.delete",
    "data_manager.check",
    "data_manager.evaluate",
    "data_manager.apply",
    "data_manager.properties",
    "worksheet.list",
    "worksheet.templates",
    "worksheet.create",
    "worksheet.get",
    "worksheet.rename",
    "worksheet.delete",
    "worksheet.set_cell",
    "worksheet.set_cells",
    "worksheet.set_database",
    "worksheet.remove_database",
    "worksheet.evaluate",
    "worksheet.export",
    "worksheet.place",
    "worksheet.fit",
];

pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !NAMES.contains(&method) {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    Some(run(s, method, p))
}

/// A reference to a resource of one type: an ID, or a name.
fn resource_ref(doc: &Document, v: &Value, ty: &str) -> Result<ResourceId, RpcError> {
    let found = match v {
        Value::String(name) => cadektor_core::uid::Uid::parse(name.trim())
            .map(ResourceId)
            .filter(|id| {
                doc.resources
                    .get(*id)
                    .is_some_and(|r| r.body.type_name() == ty)
            })
            .or_else(|| {
                doc.resources
                    .iter()
                    .find(|r| r.body.type_name() == ty && r.name.eq_ignore_ascii_case(name.trim()))
                    .map(|r| r.id)
            }),
        _ => None,
    };
    found.ok_or_else(|| bad(format!("no {} {v}", ty.replace('_', " "))))
}

#[derive(Deserialize)]
struct IdParam {
    id: Value,
}

fn exec(s: &mut Session, c: Command) -> Result<cadektor_core::command::Outcome, RpcError> {
    let out = s.editor.execute(&c).map_err(RpcError::from)?;
    s.prune_selection();
    Ok(out)
}

// --- records ---

fn field_json(f: &FieldDef) -> Value {
    serde_json::to_value(f).unwrap_or_default()
}

fn format_json(doc: &Document, id: ResourceId) -> Value {
    let r = doc.resources.get(id).expect("exists");
    let ResourceBody::RecordFormat(f) = &r.body else {
        return Value::Null;
    };
    let used = doc
        .objects
        .values()
        .filter(|o| record::values(o, &r.name).is_some())
        .count();
    json!({"id": id, "name": r.name, "fields": f.fields.iter().map(field_json).collect::<Vec<_>>(), "used_by": used})
}

fn parse_fields(v: &Value) -> Result<RecordFormat, RpcError> {
    let fields: Vec<FieldDef> = serde_json::from_value(v.clone()).map_err(|e| bad(format!("fields: {e}. Each field is {{name, type (text|integer|number|boolean|dimension|area|choice|date|time|volume|angle), default?, unit?, choices?}}")))?;
    if let Some(f) = fields.iter().find(|f| f.ty == FieldType::Unknown) {
        return Err(bad(format!(
            "field '{}' has an unknown type. Types: text, integer, number, boolean, dimension, area, choice, date, time, volume, angle",
            f.name
        )));
    }
    Ok(RecordFormat { fields })
}

fn object_ids(doc: &Document, ids: &[ObjectId]) -> Result<(), RpcError> {
    match ids.iter().find(|id| doc.object(**id).is_none()) {
        Some(bad_id) => Err(RpcError::not_found(format!(
            "object {bad_id} does not exist"
        ))),
        None => Ok(()),
    }
}

#[derive(Deserialize)]
struct CreateFormat {
    name: String,
    #[serde(default)]
    fields: Value,
}

#[derive(Deserialize)]
struct UpdateFormat {
    id: Value,
    name: Option<String>,
    fields: Option<Value>,
}

#[derive(Deserialize)]
struct RecordOp {
    ids: Vec<ObjectId>,
    record: Value,
    #[serde(default)]
    values: Map<String, Value>,
}

// --- worksheets ---

fn worksheet_def(doc: &Document, id: ResourceId) -> Result<WorksheetDef, RpcError> {
    match doc.resources.get(id).map(|r| &r.body) {
        Some(ResourceBody::Worksheet(v)) => WorksheetDef::from_value(v).map_err(app),
        _ => Err(bad("no worksheet with this ID")),
    }
}

fn save_worksheet(s: &mut Session, id: ResourceId, def: &WorksheetDef) -> Result<(), RpcError> {
    def.validate().map_err(bad)?;
    exec(
        s,
        Command::ModifyResource {
            id,
            name: None,
            body: Some(ResourceBody::Worksheet(def.to_value())),
        },
    )?;
    Ok(())
}

fn value_json(v: &cadektor_sheet::Value) -> Value {
    use cadektor_sheet::Value as V;
    match v {
        V::Empty => Value::Null,
        V::Number(n) => json!(n),
        V::Text(t) => json!(t),
        V::Bool(b) => json!(b),
        V::Error(e) => json!({"error": e.code()}),
    }
}

#[derive(Deserialize)]
struct CreateSheet {
    name: String,
    template: Option<String>,
    #[serde(default)]
    cells: Map<String, Value>,
    #[serde(default)]
    databases: Vec<DatabaseDef>,
}

#[derive(Deserialize)]
struct SetCell {
    id: Value,
    address: String,
    #[serde(default)]
    input: Value,
    decimals: Option<u8>,
    unit: Option<String>,
}

#[derive(Deserialize)]
struct SetCells {
    id: Value,
    cells: Map<String, Value>,
}

#[derive(Deserialize)]
struct SetDatabase {
    id: Value,
    index: Option<usize>,
    database: DatabaseDef,
}

#[derive(Deserialize)]
struct RemoveDatabase {
    id: Value,
    index: usize,
}

#[derive(Deserialize)]
struct Rename {
    id: Value,
    name: String,
}

#[derive(Deserialize)]
struct ExportSheet {
    id: Value,
    format: String,
    path: Option<std::path::PathBuf>,
    #[serde(default)]
    overwrite: bool,
}

/// The text of a cell input: strings as they are, numbers and booleans as typed.
fn input_text(v: &Value) -> Result<String, RpcError> {
    match v {
        Value::Null => Ok(String::new()),
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        other => Err(bad(format!(
            "a cell input is text, a number or a boolean, not {other}"
        ))),
    }
}

// --- data maps ---

#[derive(Deserialize)]
struct CreateMap {
    name: String,
    template: Option<String>,
    entities: Option<Vec<EntityMap>>,
    rules: Option<Vec<MapRule>>,
}

#[derive(Deserialize)]
struct UpdateMap {
    id: Value,
    name: Option<String>,
    entities: Option<Vec<EntityMap>>,
    rules: Option<Vec<MapRule>>,
}

#[derive(Deserialize)]
struct EvalMap {
    id: Value,
    ids: Option<Vec<ObjectId>>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    1000
}

fn map_json(doc: &Document, id: ResourceId) -> Value {
    let r = doc.resources.get(id).expect("exists");
    let m = doc.resources.data_map(id).cloned().unwrap_or_default();
    json!({"id": id, "name": r.name, "entities": m.entities, "rules": m.rules})
}

fn mapped_json(doc: &Document, map: &DataMap, o: &Object) -> Value {
    let m = datamap::evaluate(doc, map, o);
    json!({"id": o.id, "kind": o.shape.kind(), "ifc_class": m.ifc_class, "predefined_type": m.predefined_type, "properties": m.properties, "record_values": m.record_values, "problems": m.problems})
}

fn run(s: &mut Session, method: &str, p: Value) -> Result<Value, RpcError> {
    match method {
        "record_format.list" => {
            let doc = s.editor.doc();
            let ids: Vec<ResourceId> = doc
                .resources
                .iter()
                .filter(|r| matches!(r.body, ResourceBody::RecordFormat(_)))
                .map(|r| r.id)
                .collect();
            Ok(
                json!({"formats": ids.into_iter().map(|id| format_json(doc, id)).collect::<Vec<_>>()}),
            )
        }
        "record_format.get" => {
            let IdParam { id } = params(p)?;
            let doc = s.editor.doc();
            let rid = resource_ref(doc, &id, "record_format")?;
            Ok(format_json(doc, rid))
        }
        "record_format.create" => {
            let CreateFormat { name, fields } = params(p)?;
            let format = parse_fields(&fields)?;
            let out = exec(
                s,
                Command::AddResource {
                    name,
                    body: ResourceBody::RecordFormat(Box::new(format)),
                },
            )?;
            Ok(json!({"id": out.created[0]}))
        }
        "record_format.update" => {
            let UpdateFormat { id, name, fields } = params(p)?;
            let rid = resource_ref(s.editor.doc(), &id, "record_format")?;
            let body = fields
                .map(|f| parse_fields(&f))
                .transpose()?
                .map(|f| ResourceBody::RecordFormat(Box::new(f)));
            exec(
                s,
                Command::ModifyResource {
                    id: rid,
                    name,
                    body,
                },
            )?;
            Ok(format_json(s.editor.doc(), rid))
        }
        "record_format.delete" => {
            let IdParam { id } = params(p)?;
            let rid = resource_ref(s.editor.doc(), &id, "record_format")?;
            exec(
                s,
                Command::DeleteResource {
                    id: rid,
                    reassign: None,
                },
            )?;
            Ok(json!({"ok": true}))
        }
        "record.attach" | "record.detach" | "record.set" => {
            let RecordOp {
                ids,
                record,
                values,
            } = params(p)?;
            let rid = resource_ref(s.editor.doc(), &record, "record_format")?;
            object_ids(s.editor.doc(), &ids)?;
            let count = ids.len();
            let cmd = match method {
                "record.attach" => Command::AttachRecord {
                    ids,
                    record: rid,
                    values,
                },
                "record.detach" => Command::DetachRecord { ids, record: rid },
                _ => Command::SetRecordFields {
                    ids,
                    record: rid,
                    values,
                },
            };
            exec(s, cmd)?;
            Ok(json!({"objects": count}))
        }
        "record.get" => {
            #[derive(Deserialize)]
            struct P {
                id: ObjectId,
            }
            let P { id } = params(p)?;
            let doc = s.editor.doc();
            let o = doc
                .object(id)
                .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
            let records: Map<String, Value> = doc
                .resources
                .iter()
                .filter(|r| matches!(r.body, ResourceBody::RecordFormat(_)))
                .filter_map(|r| {
                    record::values(o, &r.name)
                        .map(|v| (r.name.clone(), Value::Object(record::fields_only(v))))
                })
                .collect();
            Ok(json!({"records": records}))
        }
        "data_manager.list" => {
            let doc = s.editor.doc();
            let ids: Vec<ResourceId> = doc
                .resources
                .iter()
                .filter(|r| matches!(r.body, ResourceBody::DataMap(_)))
                .map(|r| r.id)
                .collect();
            Ok(json!({"maps": ids.into_iter().map(|id| map_json(doc, id)).collect::<Vec<_>>()}))
        }
        "data_manager.get" => {
            let IdParam { id } = params(p)?;
            let doc = s.editor.doc();
            let rid = resource_ref(doc, &id, "data_map")?;
            Ok(map_json(doc, rid))
        }
        "data_manager.create" => {
            let CreateMap {
                name,
                template,
                entities,
                rules,
            } = params(p)?;
            let mut map = match template.as_deref() {
                None => DataMap::default(),
                Some("landscape") => DataMap::landscape_template(),
                Some(other) => {
                    return Err(bad(format!(
                        "unknown template '{other}'. Templates: landscape"
                    )));
                }
            };
            if let Some(e) = entities {
                map.entities = e;
            }
            if let Some(r) = rules {
                map.rules = r;
            }
            let out = exec(
                s,
                Command::AddResource {
                    name,
                    body: ResourceBody::DataMap(Box::new(map)),
                },
            )?;
            Ok(json!({"id": out.created[0]}))
        }
        "data_manager.update" => {
            let UpdateMap {
                id,
                name,
                entities,
                rules,
            } = params(p)?;
            let rid = resource_ref(s.editor.doc(), &id, "data_map")?;
            let mut map = s
                .editor
                .doc()
                .resources
                .data_map(rid)
                .cloned()
                .unwrap_or_default();
            if let Some(e) = entities {
                map.entities = e;
            }
            if let Some(r) = rules {
                map.rules = r;
            }
            exec(
                s,
                Command::ModifyResource {
                    id: rid,
                    name,
                    body: Some(ResourceBody::DataMap(Box::new(map))),
                },
            )?;
            Ok(map_json(s.editor.doc(), rid))
        }
        "data_manager.delete" => {
            let IdParam { id } = params(p)?;
            let rid = resource_ref(s.editor.doc(), &id, "data_map")?;
            exec(
                s,
                Command::DeleteResource {
                    id: rid,
                    reassign: None,
                },
            )?;
            Ok(json!({"ok": true}))
        }
        "data_manager.check" => {
            let IdParam { id } = params(p)?;
            let doc = s.editor.doc();
            let rid = resource_ref(doc, &id, "data_map")?;
            Ok(
                json!({"problems": doc.resources.data_map(rid).map(|m| m.check_refs(doc)).unwrap_or_default()}),
            )
        }
        "data_manager.evaluate" => {
            let EvalMap { id, ids, limit } = params(p)?;
            let doc = s.editor.doc();
            let rid = resource_ref(doc, &id, "data_map")?;
            let map = doc.resources.data_map(rid).cloned().unwrap_or_default();
            let objects: Vec<&Object> = match &ids {
                Some(ids) => {
                    object_ids(doc, ids)?;
                    ids.iter().filter_map(|i| doc.object(*i)).collect()
                }
                None => doc.objects.values().collect(),
            };
            let total = objects.len();
            let list: Vec<Value> = objects
                .into_iter()
                .take(limit)
                .map(|o| mapped_json(doc, &map, o))
                .collect();
            Ok(json!({"objects": list, "total": total}))
        }
        "data_manager.apply" => {
            let IdParam { id } = params(p)?;
            let doc = s.editor.doc();
            let rid = resource_ref(doc, &id, "data_map")?;
            let map = doc.resources.data_map(rid).cloned().unwrap_or_default();
            let (commands, problems) = datamap::record_commands(doc, &map);
            let changed = commands.len();
            if changed > 0 {
                exec(s, Command::Batch { commands })?;
            }
            Ok(json!({"changed": changed, "problems": problems}))
        }
        "data_manager.properties" => {
            let doc = s.editor.doc();
            let records: Vec<Value> = doc
                .resources
                .iter()
                .filter_map(|r| match &r.body {
                    ResourceBody::RecordFormat(f) => Some(json!({"record": r.name, "fields": f.fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>()})),
                    _ => None,
                })
                .collect();
            let ifc_types: Vec<&str> = [
                IfcType::Label,
                IfcType::Text,
                IfcType::Real,
                IfcType::Integer,
                IfcType::Boolean,
                IfcType::Length,
                IfcType::Area,
                IfcType::Volume,
            ]
            .iter()
            .map(|t| t.id())
            .collect();
            Ok(json!({
                "builtin": props::NAMES,
                "plant_fields": cadektor_core::plants::style::PlantStyle::field_names(),
                "records": records,
                "field_types": FieldType::ALL.iter().map(|t| t.id()).collect::<Vec<_>>(),
                "ifc_types": ifc_types,
            }))
        }
        "worksheet.list" => {
            let doc = s.editor.doc();
            let list: Vec<Value> = doc
                .resources
                .iter()
                .filter_map(|r| match &r.body {
                    ResourceBody::Worksheet(v) => {
                        let def = WorksheetDef::from_value(v).unwrap_or_default();
                        Some(json!({"id": r.id, "name": r.name, "cells": def.cells.len(), "databases": def.databases.len()}))
                    }
                    _ => None,
                })
                .collect();
            Ok(json!({"worksheets": list}))
        }
        "worksheet.templates" => Ok(
            json!({"templates": templates::TEMPLATES.iter().map(|t| json!({"id": t.id, "title": t.title, "description": t.description})).collect::<Vec<_>>()}),
        ),
        "worksheet.create" => {
            let CreateSheet {
                name,
                template,
                cells,
                databases,
            } = params(p)?;
            let mut def = match template.as_deref() {
                None => WorksheetDef::default(),
                Some(t) => templates::template(t).ok_or_else(|| {
                    bad(format!(
                        "unknown template '{t}'. Templates: {}",
                        templates::TEMPLATES
                            .iter()
                            .map(|t| t.id)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                })?,
            };
            for (addr, input) in &cells {
                def.set_cell(addr, &input_text(input)?, None).map_err(bad)?;
            }
            def.databases.extend(databases);
            def.validate().map_err(bad)?;
            let out = exec(
                s,
                Command::AddResource {
                    name,
                    body: ResourceBody::Worksheet(def.to_value()),
                },
            )?;
            Ok(json!({"id": out.created[0]}))
        }
        "worksheet.get" => {
            let IdParam { id } = params(p)?;
            let doc = s.editor.doc();
            let rid = resource_ref(doc, &id, "worksheet")?;
            let def = worksheet_def(doc, rid)?;
            let mut v = def.to_value();
            v.as_object_mut().map(|m| m.remove("type"));
            Ok(
                json!({"id": rid, "name": doc.resources.get(rid).map(|r| r.name.clone()), "definition": v}),
            )
        }
        "worksheet.rename" => {
            let Rename { id, name } = params(p)?;
            let rid = resource_ref(s.editor.doc(), &id, "worksheet")?;
            exec(
                s,
                Command::ModifyResource {
                    id: rid,
                    name: Some(name),
                    body: None,
                },
            )?;
            Ok(json!({"ok": true}))
        }
        "worksheet.delete" => {
            let IdParam { id } = params(p)?;
            let rid = resource_ref(s.editor.doc(), &id, "worksheet")?;
            exec(
                s,
                Command::DeleteResource {
                    id: rid,
                    reassign: None,
                },
            )?;
            Ok(json!({"ok": true}))
        }
        "worksheet.set_cell" => {
            let SetCell {
                id,
                address,
                input,
                decimals,
                unit,
            } = params(p)?;
            let rid = resource_ref(s.editor.doc(), &id, "worksheet")?;
            let mut def = worksheet_def(s.editor.doc(), rid)?;
            let format = (decimals.is_some() || unit.is_some()).then(|| {
                let old = def
                    .cells
                    .get(&address.trim().to_ascii_uppercase())
                    .map(|c| c.format.clone())
                    .unwrap_or_default();
                CellFormat {
                    decimals: decimals.or(old.decimals),
                    unit: unit.unwrap_or(old.unit),
                }
            });
            def.set_cell(&address, &input_text(&input)?, format)
                .map_err(bad)?;
            save_worksheet(s, rid, &def)?;
            Ok(json!({"ok": true}))
        }
        "worksheet.set_cells" => {
            let SetCells { id, cells } = params(p)?;
            let rid = resource_ref(s.editor.doc(), &id, "worksheet")?;
            let mut def = worksheet_def(s.editor.doc(), rid)?;
            for (addr, input) in &cells {
                def.set_cell(addr, &input_text(input)?, None).map_err(bad)?;
            }
            save_worksheet(s, rid, &def)?;
            Ok(json!({"cells": cells.len()}))
        }
        "worksheet.set_database" => {
            let SetDatabase {
                id,
                index,
                database,
            } = params(p)?;
            let rid = resource_ref(s.editor.doc(), &id, "worksheet")?;
            let mut def = worksheet_def(s.editor.doc(), rid)?;
            let at = match index {
                Some(i) if i < def.databases.len() => {
                    def.databases[i] = database;
                    i
                }
                Some(i) => {
                    return Err(bad(format!(
                        "database {i} does not exist. The worksheet has {}",
                        def.databases.len()
                    )));
                }
                None => {
                    def.databases.push(database);
                    def.databases.len() - 1
                }
            };
            save_worksheet(s, rid, &def)?;
            Ok(json!({"index": at}))
        }
        "worksheet.remove_database" => {
            let RemoveDatabase { id, index } = params(p)?;
            let rid = resource_ref(s.editor.doc(), &id, "worksheet")?;
            let mut def = worksheet_def(s.editor.doc(), rid)?;
            if index >= def.databases.len() {
                return Err(bad(format!(
                    "database {index} does not exist. The worksheet has {}",
                    def.databases.len()
                )));
            }
            def.databases.remove(index);
            save_worksheet(s, rid, &def)?;
            Ok(json!({"ok": true}))
        }
        "worksheet.evaluate" => {
            let IdParam { id } = params(p)?;
            let c = computed(s, &id)?.0;
            let values: Vec<Vec<Value>> = (1..=c.rows)
                .map(|r| {
                    (1..=c.cols)
                        .map(|col| value_json(&c.value(r, col)))
                        .collect()
                })
                .collect();
            let cells: Map<String, Value> = c
                .cells()
                .map(|(r, col, v)| {
                    (
                        cadektor_sheet::formula::CellRef { row: r, col }.to_string(),
                        value_json(v),
                    )
                })
                .collect();
            Ok(json!({
                "rows": c.rows,
                "cols": c.cols,
                "values": values,
                "text": c.grid(),
                "cells": cells,
                "blocks": c.blocks.iter().map(|b| json!({"name": b.name, "first_row": b.first_row, "rows": b.rows, "matched": b.matched})).collect::<Vec<_>>(),
                "problems": c.problems,
                "errors": c.error_count(),
            }))
        }
        "worksheet.place" => {
            #[derive(Deserialize)]
            struct P {
                id: Value,
                at: cadektor_core::DVec2,
                text_height: Option<f64>,
                layer: Option<cadektor_core::model::LayerId>,
                class: Option<cadektor_core::model::ClassId>,
            }
            let P {
                id,
                at,
                text_height,
                layer,
                class,
            } = params(p)?;
            let (c, _) = computed(s, &id)?;
            let rid = resource_ref(s.editor.doc(), &id, "worksheet")?;
            let h = text_height.unwrap_or(cadektor_core::worksheet_table::DEFAULT_TEXT_HEIGHT);
            if !h.is_finite() || h <= 0.0 {
                return Err(bad("text_height must be a positive number"));
            }
            let l = cadektor_sheet::layout(&c, h);
            let size = cadektor_core::DVec2::new(l.width, l.height);
            let out = exec(
                s,
                Command::AddWorksheetTable {
                    worksheet: rid,
                    at,
                    size,
                    text_height: Some(h),
                    layer,
                    class,
                    attrs: Default::default(),
                },
            )?;
            Ok(json!({"id": out.created[0], "size": [l.width, l.height]}))
        }
        "worksheet.fit" => {
            #[derive(Deserialize)]
            struct P {
                ids: Option<Vec<ObjectId>>,
            }
            let P { ids } = params(p)?;
            let doc = s.editor.doc();
            let tables: Vec<(ObjectId, cadektor_core::worksheet_table::TableData)> = match ids {
                Some(ids) => ids
                    .iter()
                    .map(|i| {
                        doc.object(*i)
                            .and_then(cadektor_core::worksheet_table::data)
                            .map(|d| (*i, d))
                            .ok_or_else(|| bad(format!("object {i} is not a worksheet table")))
                    })
                    .collect::<Result<_, _>>()?,
                None => doc
                    .objects
                    .values()
                    .filter_map(|o| cadektor_core::worksheet_table::data(o).map(|d| (o.id, d)))
                    .collect(),
            };
            let mut commands = Vec::new();
            for (oid, d) in &tables {
                let (c, _) = computed(s, &json!(d.worksheet.0))?;
                let l = cadektor_sheet::layout(&c, d.text_height);
                commands.push(Command::SetWorksheetTable {
                    ids: vec![*oid],
                    worksheet: None,
                    text_height: None,
                    size: Some(cadektor_core::DVec2::new(l.width, l.height)),
                });
            }
            let fitted = commands.len();
            if fitted > 0 {
                exec(s, Command::Batch { commands })?;
            }
            Ok(json!({"fitted": fitted}))
        }
        "worksheet.export" => {
            let ExportSheet {
                id,
                format,
                path,
                overwrite,
            } = params(p)?;
            let (c, name) = computed(s, &id)?;
            let format = format.to_ascii_lowercase();
            let bytes: Vec<u8> = match format.as_str() {
                "csv" => export::csv(&c).into_bytes(),
                "xlsx" => export::xlsx(&c, &name),
                other => return Err(bad(format!("unknown format '{other}'. Use csv or xlsx"))),
            };
            let Some(path) = path else {
                return Ok(if format == "csv" {
                    json!({"csv": String::from_utf8_lossy(&bytes), "rows": c.rows, "cols": c.cols})
                } else {
                    json!({"xlsx_base64": base64::engine::general_purpose::STANDARD.encode(&bytes), "rows": c.rows, "cols": c.cols})
                });
            };
            write_export(s, &path, &format, overwrite, &bytes)
        }
        _ => unreachable!("handle() checks the names"),
    }
}

/// The calculated worksheet (from the cache) and its name.
fn computed(
    s: &Session,
    id: &Value,
) -> Result<(std::sync::Arc<cadektor_sheet::Computed>, String), RpcError> {
    let doc = s.editor.doc();
    let rid = resource_ref(doc, id, "worksheet")?;
    let def = worksheet_def(doc, rid)?;
    let name = doc
        .resources
        .get(rid)
        .map(|r| r.name.clone())
        .unwrap_or_default();
    let ground = cadektor_site::SiteTerrain::new(s.site_cache(), s.editor.revision(), doc);
    Ok((
        s.worksheet_cache
            .get_with(s.editor.revision(), rid.0, &def, doc, &ground),
        name,
    ))
}

#[cfg(not(target_arch = "wasm32"))]
fn write_export(
    s: &Session,
    path: &std::path::Path,
    ext: &str,
    overwrite: bool,
    bytes: &[u8],
) -> Result<Value, RpcError> {
    let path = if s.paths.is_restricted() {
        s.check_path(path, true, &[ext]).map_err(RpcError::policy)?
    } else {
        path.to_path_buf()
    };
    if s.paths.is_restricted() && path.exists() && !overwrite {
        return Err(RpcError::conflict(format!(
            "{} exists: pass \"overwrite\": true to replace it",
            path.display()
        )));
    }
    cadektor_sys::fsio::write_atomic(&path, bytes)
        .map_err(|e| app(format!("cannot write {}: {e}", path.display())))?;
    Ok(json!({"path": path, "bytes": bytes.len()}))
}

#[cfg(target_arch = "wasm32")]
fn write_export(
    _: &Session,
    _: &std::path::Path,
    _: &str,
    _: bool,
    _: &[u8],
) -> Result<Value, RpcError> {
    Err(app("this build cannot write files. Leave out the path"))
}

impl Session {
    /// A worksheet with its calculated result, for a host that shows it (the app). The result is
    /// cached by document revision, like the one of `worksheet.evaluate`.
    pub fn worksheet_result(
        &self,
        id: ResourceId,
    ) -> Option<(WorksheetDef, std::sync::Arc<cadektor_sheet::Computed>)> {
        let doc = self.editor.doc();
        let def = worksheet_def(doc, id).ok()?;
        let ground =
            cadektor_site::SiteTerrain::new(self.site_cache(), self.editor.revision(), doc);
        let result =
            self.worksheet_cache
                .get_with(self.editor.revision(), id.0, &def, doc, &ground);
        Some((def, result))
    }
}
