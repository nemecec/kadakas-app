//! Agent API for groups, symbols, patterns and the Resource Manager.
//!
//! Methods: `group.create|ungroup|enter|exit|context|members|move|rotate|copy`, `selection.click`,
//! `symbol.list|get|create|from_objects|place|set|convert_to_group|edit|edit_end`,
//! `resource.list|get|create|update|rename|duplicate|delete|purge_unused|thumbnail`,
//! `pattern.segments`, and (native only) `resource.list_file|import_from_file|export_to_file`,
//! `library.list|favorites|favorite_add|favorite_remove`.

use serde_json::{json, Map, Value};

use cadektor_core::command::Command;
use cadektor_core::graphics::{GraphicResource, TYPE_NAMES};
use cadektor_core::library;
use cadektor_core::model::*;
use cadektor_core::resource::{ResourceBody, ResourceId};
use cadektor_core::DVec2;

use crate::{RpcError, Session};

pub const GROUP_CREATE: (&str, &str) = ("group.create", "Group objects as one undo step. params: {ids? (default: the selection), name?}. The objects must be on one layer and in the same group. Selects the group. Returns {id}.");
pub const GROUP_UNGROUP: (&str, &str) = ("group.ungroup", "Dissolve groups by one level. params: {ids? (default: the selection)}. Selects the freed members. Returns {members}.");
pub const GROUP_ENTER: (&str, &str) = ("group.enter", "Enter a group (edit in place): clicks then select its members. params: {id}. The group must be a member of the group you are in now (or at the top). Returns {context}.");
pub const GROUP_EXIT: (&str, &str) = ("group.exit", "Leave the group you are in, one level. Selects that group. Returns {context}.");
pub const GROUP_CONTEXT: (&str, &str) = ("group.context", "The groups you have entered, outermost first. Returns {context: [id]}.");
pub const GROUP_MEMBERS: (&str, &str) = ("group.members", "Members of a group. params: {id, deep? (all levels, default false)}. Returns {members: [id]}.");
pub const GROUP_MOVE: (&str, &str) = ("group.move", "Move objects or groups as units. params: {ids? (default: the selection), delta: [dx, dy]}.");
pub const GROUP_ROTATE: (&str, &str) = ("group.rotate", "Rotate objects or groups as units. params: {ids? (default: the selection), center: [x, y], angle (radians, counterclockwise)}. Hardscapes, landscape areas and site models cannot be turned.");
pub const GROUP_COPY: (&str, &str) = ("group.copy", "Copy objects or groups (with their members) and move the copies. params: {ids? (default: the selection), delta?: [dx, dy]}. Selects the copies. Returns {ids: [new ids of the copies], members}.");
pub const SELECTION_CLICK: (&str, &str) = ("selection.click", "Select like a click does: a click on a member of a group selects the top group, unless you entered that group. params: {id, add? (toggle in the selection)}. Returns {ids}.");
pub const SYMBOL_LIST: (&str, &str) = ("symbol.list", "Symbol definitions with their kind (2d, 3d or hybrid) and the number of instances. Returns {symbols: [{id, name, kind, items, solids, insertion, instances}]}.");
pub const SYMBOL_GET: (&str, &str) = ("symbol.get", "One symbol definition. params: {id} (the ID or the name).");
pub const SYMBOL_CREATE: (&str, &str) = ("symbol.create", "Create a symbol definition. params: {name, items? ([{shape, attrs?, class?}], 2D components in definition coordinates: line, rect, circle, arc, polyline, text, symbol), solids? ([{kind: extrusion, outline, base?, height, color?} | {kind: cylinder, center, radius, base?, height, color?}], 3D components), insertion? ([x, y])}. Components without a class take the class of the instance. Returns {id}.");
pub const SYMBOL_FROM_OBJECTS: (&str, &str) = ("symbol.from_objects", "Make a symbol definition from objects (groups count as their members). params: {name, ids? (default: the selection), insertion? (default: the center), replace? (replace the objects by one instance)}. Returns {id, instance?}.");
pub const SYMBOL_PLACE: (&str, &str) = ("symbol.place", "Place an instance of a symbol. params: {symbol (ID or name), at: [x, y], rotation? (radians), scale? (default 1; negative mirrors), scale_y? (for an asymmetric scale), layer?, class?}. Returns {id}.");
pub const SYMBOL_SET: (&str, &str) = ("symbol.set", "Change instances. params: {id | ids, at?, rotation?, scale?, scale_y? (null makes the scale symmetric again), symbol? (another definition)}.");
pub const SYMBOL_CONVERT_TO_GROUP: (&str, &str) = ("symbol.convert_to_group", "Replace instances by groups of plain objects. params: {ids? (default: the selection)}. Returns {groups: [id]}.");
pub const SYMBOL_EDIT: (&str, &str) = ("symbol.edit", "Edit the 2D components of a symbol in place (Edit 2D Components). The components appear as a group at the instance, which is hidden, and you enter the group. Edit them with execute, then call symbol.edit_end. params: {id (instance)}. Returns {group}.");
pub const SYMBOL_EDIT_END: (&str, &str) = ("symbol.edit_end", "Finish editing a symbol: the objects of the edit group become the 2D components, and all instances change. params: {group?} (default: the edit group you are in).");
pub const RESOURCE_LIST: (&str, &str) = ("resource.list", "All resources of the document (the Resource Manager). params: {type? (plant_style, hardscape_style, landscape_area_style, record_format, data_map, worksheet, symbol, hatch, tile, line_type, texture), search? (part of the name or type), thumbnails? (adds an SVG of each)}. Returns {resources: [{id, name, type, used_by, depends_on, thumbnail?}]}.");
pub const RESOURCE_GET: (&str, &str) = ("resource.get", "One resource with its content and who uses it. params: {id} (the ID or the name; add type for a name).");
pub const RESOURCE_CREATE: (&str, &str) = ("resource.create", "Create a symbol, hatch, tile, line type or texture. params: {type, name, ...fields}. hatch: {units? (paper|world), lines: [{angle (degrees), spacing, offset?, origin?, dash? ([dash, gap, ...]), color?, weight?}]}. tile: {symbol, width, height, row_shift?, angle?, scale?, units?}. line_type: {units?, elements: [{kind: dash, length} | {kind: gap, length} | {kind: mark, symbol, scale?, rotate?}]}. texture: {color, size?, reflectivity?, transparency?, image?}. symbol: as symbol.create. Returns {id}.");
pub const RESOURCE_UPDATE: (&str, &str) = ("resource.update", "Change a graphic resource. params: {id, name? (renames), ...fields as in resource.create}. Fields you give replace the old ones. Every user follows at once.");
pub const RESOURCE_RENAME: (&str, &str) = ("resource.rename", "Rename any resource. params: {id, name}.");
pub const RESOURCE_DUPLICATE: (&str, &str) = ("resource.duplicate", "Copy any resource. params: {id, name?}. Returns {id}.");
pub const RESOURCE_DELETE: (&str, &str) = ("resource.delete", "Delete any resource. params: {id, reassign?}. Fails if it is used, unless reassign names another resource of the same type.");
pub const RESOURCE_PURGE_UNUSED: (&str, &str) = ("resource.purge_unused", "Delete the resources that nothing uses, as one undo step (a tile that is unused frees its symbol too). params: {types? (default: all but worksheets and data maps), dry_run?}. Returns {removed: [{id, name, type}]}.");
pub const RESOURCE_THUMBNAIL: (&str, &str) = ("resource.thumbnail", "A thumbnail of a resource. params: {id, size? (px, default 64)}. Returns {svg}.");
pub const PATTERN_SEGMENTS: (&str, &str) = ("pattern.segments", "The lines that a hatch or tile draws in a polygon, clipped to it (what the canvas, SVG, PDF and DXF draw). params: {pattern (ID or name), ring: [[x, y]], holes? ([[[x, y]]]), scale? (model mm per paper mm, default 100)}. Returns {count, total_length, segments? (the first 1000)}.");
#[cfg(not(target_arch = "wasm32"))]
pub const RESOURCE_LIST_FILE: (&str, &str) = ("resource.list_file", "What a library file holds. params: {path | library (a file in the library folder)}. Returns {resources: [{id, name, type, depends_on}], classes, layers}.");
#[cfg(not(target_arch = "wasm32"))]
pub const RESOURCE_IMPORT_FROM_FILE: (&str, &str) = ("resource.import_from_file", "Import resources from another .cdk.json file as one undo step, with the resources they need (a symbol brings its hatch). The path follows the path policy. params: {path | library, items? ([{type, name}] or [name]; default: everything when all is true), all?, on_conflict? (rename (default) | reuse | replace), classes? ([names]), layers? ([names])}. Returns {imported: [{name, type, id, status (added|renamed|reused|replaced), dependency}], classes_added, layers_added}.");
#[cfg(not(target_arch = "wasm32"))]
pub const RESOURCE_EXPORT_TO_FILE: (&str, &str) = ("resource.export_to_file", "Save resources (with the ones they need) as a library file. params: {path | library, ids? (default: all), overwrite?}. Returns {path, count}.");
#[cfg(not(target_arch = "wasm32"))]
pub const LIBRARY_LIST: (&str, &str) = ("library.list", "The library files in the per-user library folder (set CADEKTOR_LIBRARY_DIR to change it). Returns {dir, files: [{file, resources}]}.");
#[cfg(not(target_arch = "wasm32"))]
pub const LIBRARY_FAVORITES: (&str, &str) = ("library.favorites", "The favorite resources: {library, type, name}. Returns {favorites}.");
#[cfg(not(target_arch = "wasm32"))]
pub const LIBRARY_FAVORITE_ADD: (&str, &str) = ("library.favorite_add", "Mark a resource of a library file as a favorite. params: {library, type, name}.");
#[cfg(not(target_arch = "wasm32"))]
pub const LIBRARY_FAVORITE_REMOVE: (&str, &str) = ("library.favorite_remove", "Remove a favorite. params: {library, type, name}.");

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "groups": "A group is an object (kind group) whose members name it as parent. Moving, turning, copying and deleting a group acts on all members. A click selects the top group (selection.click). group.enter lets you select members. Groups undo like any command.",
        "symbols": "A symbol definition is a resource with 2D components and optional 3D components (hybrid). An instance (shape kind symbol) has a definition, a position (where the insertion point goes), a rotation and a scale. Editing the definition changes every instance at once.",
        "patterns": "Hatches, tiles and line types are resources. Objects use them with the attributes fill_pattern (a hatch or tile) and line_type, on the object or on its class: {op: modify_object, id, attrs: {fill_pattern: 5}}. Paper units follow the drawing scale. The pattern is clipped to the shape in the canvas, SVG, PDF and DXF.",
        "libraries": "resource.import_from_file reads another document and imports the resources you name with what they need. The library folder holds shared .cdk.json files.",
        "examples": [
            {"method": "group.create", "params": {"ids": [2, 3, 4], "name": "Bench"}},
            {"method": "group.rotate", "params": {"ids": [5], "center": [0, 0], "angle": 1.57}},
            {"method": "resource.create", "params": {"type": "hatch", "name": "Diagonal", "units": "paper", "lines": [{"angle": 45, "spacing": 2}]}},
            {"method": "symbol.create", "params": {"name": "Tree mark", "items": [{"shape": {"kind": "circle", "center": [0, 0], "radius": 500}}, {"shape": {"kind": "line", "a": [-500, 0], "b": [500, 0]}}]}},
            {"method": "symbol.place", "params": {"symbol": "Tree mark", "at": [1000, 2000], "rotation": 0.5, "scale": 2, "scale_y": 1}},
            {"method": "symbol.from_objects", "params": {"name": "Planter", "ids": [2, 3], "replace": true}},
            {"method": "resource.create", "params": {"type": "line_type", "name": "Dashed", "elements": [{"kind": "dash", "length": 3}, {"kind": "gap", "length": 1.5}]}},
            {"method": "resource.import_from_file", "params": {"path": "/data/library.cdk.json", "items": [{"type": "symbol", "name": "Tree mark"}], "on_conflict": "rename"}},
            {"method": "resource.purge_unused", "params": {"dry_run": true}}
        ],
    })
}

fn bad(e: impl std::fmt::Display) -> RpcError {
    RpcError { code: -32602, message: e.to_string() }
}

fn app(e: impl std::fmt::Display) -> RpcError {
    RpcError { code: 1, message: e.to_string() }
}

fn required<'a>(m: &'a Map<String, Value>, key: &str) -> Result<&'a Value, RpcError> {
    m.get(key).ok_or_else(|| bad(format!("missing '{key}'")))
}

fn text(m: &Map<String, Value>, key: &str) -> Result<String, RpcError> {
    required(m, key)?.as_str().map(str::to_string).ok_or_else(|| bad(format!("'{key}' must be text")))
}

fn num(v: &Value, key: &str) -> Result<f64, RpcError> {
    v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| bad(format!("'{key}' must be a number")))
}

fn point(v: &Value, key: &str) -> Result<DVec2, RpcError> {
    serde_json::from_value::<DVec2>(v.clone()).ok().filter(|p| p.is_finite()).ok_or_else(|| bad(format!("'{key}' must be [x, y]")))
}

fn ids_of(s: &Session, m: &Map<String, Value>) -> Result<Vec<ObjectId>, RpcError> {
    let ids: Vec<ObjectId> = if let Some(v) = m.get("ids") {
        serde_json::from_value(v.clone()).map_err(bad)?
    } else if let Some(v) = m.get("id") {
        vec![serde_json::from_value(v.clone()).map_err(bad)?]
    } else {
        s.selection.clone()
    };
    if ids.is_empty() {
        return Err(bad("give ids, or select objects first"));
    }
    Ok(ids)
}

/// A resource by ID or by name. `ty` limits a name lookup to one type (and checks the type of an ID).
fn resource_ref(doc: &Document, v: &Value, ty: Option<&str>) -> Result<ResourceId, RpcError> {
    let found = match v {
        Value::Number(n) => n.as_u64().map(ResourceId).filter(|id| doc.resources.get(*id).is_some_and(|r| ty.is_none_or(|t| r.body.type_name() == t))),
        Value::String(name) => {
            let mut hits = doc.resources.iter().filter(|r| r.name.eq_ignore_ascii_case(name.trim()) && ty.is_none_or(|t| r.body.type_name() == t));
            let first = hits.next().map(|r| r.id);
            if first.is_some() && hits.next().is_some() {
                return Err(bad(format!("several resources are named '{name}'. Give the ID or the type")));
            }
            first
        }
        _ => None,
    };
    found.ok_or_else(|| bad(format!("no {} {v}", ty.unwrap_or("resource"))))
}

fn info_json(doc: &Document, i: &library::ResourceInfo) -> Value {
    let mut v = json!({"id": i.id, "name": i.name, "type": i.type_name, "used_by": i.used_by, "depends_on": i.depends_on});
    if let Some(d) = doc.resources.symbol(i.id) {
        v["kind"] = json!(d.kind());
    }
    v
}

fn resource_json(doc: &Document, id: ResourceId) -> Value {
    let r = doc.resources.get(id).expect("exists");
    let mut v = serde_json::to_value(&r.body).unwrap_or_default();
    if !v.is_object() {
        v = json!({ "value": v });
    }
    let u = library::usage(doc, id);
    v["id"] = json!(id);
    v["name"] = json!(r.name);
    v["type"] = json!(r.body.type_name());
    v["used_by"] = json!({"objects": u.objects, "classes": u.classes, "resources": u.resources});
    v
}

/// The body of a graphic resource from the fields of a request.
fn graphic_from(ty: &str, m: &Map<String, Value>, base: Option<&GraphicResource>) -> Result<GraphicResource, RpcError> {
    if !TYPE_NAMES.contains(&ty) {
        return Err(bad(format!("unknown type '{ty}'. Types: {}", TYPE_NAMES.join(", "))));
    }
    let mut v = match base {
        Some(b) => serde_json::to_value(b).map_err(app)?,
        None => json!({}),
    };
    let obj = v.as_object_mut().expect("object");
    obj.insert("type".into(), ty.into());
    for (k, x) in m {
        if !["name", "type", "id", "reassign"].contains(&k.as_str()) {
            obj.insert(k.clone(), x.clone());
        }
    }
    let g: GraphicResource = serde_json::from_value(v).map_err(|e| bad(format!("not a valid {ty}: {e}")))?;
    g.validate().map_err(bad)?;
    Ok(g)
}

fn unique_copy_name(doc: &Document, ty: &str, base: &str) -> String {
    library::unique_name(&format!("{base} copy"), &|n| doc.resources.iter().any(|r| r.body.type_name() == ty && r.name.eq_ignore_ascii_case(n)))
}

const METHOD_NAMES: &[&str] = &[
    "group.create", "group.ungroup", "group.enter", "group.exit", "group.context", "group.members", "group.move", "group.rotate", "group.copy", "selection.click",
    "symbol.list", "symbol.get", "symbol.create", "symbol.from_objects", "symbol.place", "symbol.set", "symbol.convert_to_group", "symbol.edit", "symbol.edit_end",
    "resource.list", "resource.get", "resource.create", "resource.update", "resource.rename", "resource.duplicate", "resource.delete", "resource.purge_unused", "resource.thumbnail",
    "pattern.segments", "resource.list_file", "resource.import_from_file", "resource.export_to_file", "library.list", "library.favorites", "library.favorite_add", "library.favorite_remove",
];

/// Handles a method of this module. `None` means that the method is not one of ours.
pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !METHOD_NAMES.contains(&method) {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    Some(match p.as_object() {
        Some(m) => run(s, method, m),
        None => Err(bad("params must be an object")),
    })
}

fn exec(s: &mut Session, c: &Command) -> Result<cadektor_core::command::Outcome, RpcError> {
    let out = s.editor.execute(c).map_err(app)?;
    s.prune_selection();
    Ok(out)
}

fn run(s: &mut Session, method: &str, m: &Map<String, Value>) -> Result<Value, RpcError> {
    match method {
        "group.create" => {
            let ids = ids_of(s, m)?;
            let name = m.get("name").map(|_| text(m, "name")).transpose()?;
            let out = exec(s, &Command::Group { ids, name })?;
            s.selection = vec![ObjectId(out.created[0])];
            Ok(json!({ "id": out.created[0] }))
        }
        "group.ungroup" => {
            let ids = ids_of(s, m)?;
            let members: Vec<ObjectId> = ids.iter().flat_map(|g| s.editor.doc().children(*g)).collect();
            exec(s, &Command::Ungroup { ids })?;
            s.selection = members.clone();
            Ok(json!({ "members": members }))
        }
        "group.enter" => {
            let id: ObjectId = serde_json::from_value(required(m, "id")?.clone()).map_err(bad)?;
            let doc = s.editor.doc();
            if !doc.is_group(id) {
                return Err(app(format!("object {id} is not a group")));
            }
            if doc.selection_target(id, s.edit_context()) != Some(id) {
                return Err(app(format!("group {id} is not inside the group you are in")));
            }
            s.context.push(id);
            s.selection.clear();
            Ok(json!({ "context": s.context }))
        }
        "group.exit" => {
            let left = s.context.pop().ok_or_else(|| app("you are not in a group"))?;
            s.selection = vec![left];
            Ok(json!({ "context": s.context }))
        }
        "group.context" => Ok(json!({ "context": s.context })),
        "group.members" => {
            let id: ObjectId = serde_json::from_value(required(m, "id")?.clone()).map_err(bad)?;
            let doc = s.editor.doc();
            if !doc.is_group(id) {
                return Err(app(format!("object {id} is not a group")));
            }
            let deep = m.get("deep").and_then(Value::as_bool).unwrap_or(false);
            Ok(json!({ "members": if deep { doc.descendants(id) } else { doc.children(id) } }))
        }
        "group.move" => {
            let ids = ids_of(s, m)?;
            let delta = point(required(m, "delta")?, "delta")?;
            exec(s, &Command::MoveObjects { ids, delta })?;
            Ok(json!({ "ok": true }))
        }
        "group.rotate" => {
            let ids = ids_of(s, m)?;
            let center = point(required(m, "center")?, "center")?;
            let angle = num(required(m, "angle")?, "angle")?;
            exec(s, &Command::RotateObjects { ids, center, angle })?;
            Ok(json!({ "ok": true }))
        }
        "group.copy" => {
            let ids = ids_of(s, m)?;
            let delta = m.get("delta").map(|d| point(d, "delta")).transpose()?.unwrap_or(DVec2::ZERO);
            let n = ids.len();
            let out = exec(s, &Command::CopyObjects { ids, delta })?;
            let all: Vec<ObjectId> = out.created.iter().map(|i| ObjectId(*i)).collect();
            s.selection = all[..n].to_vec();
            Ok(json!({ "ids": &all[..n], "members": &all[n..] }))
        }
        "selection.click" => {
            let id: ObjectId = serde_json::from_value(required(m, "id")?.clone()).map_err(bad)?;
            let doc = s.editor.doc();
            doc.object(id).ok_or_else(|| app(format!("object {id} does not exist")))?;
            let target = doc.selection_target(id, s.edit_context()).ok_or_else(|| app(format!("object {id} is not inside the group you are in")))?;
            if m.get("add").and_then(Value::as_bool).unwrap_or(false) {
                match s.selection.iter().position(|x| *x == target) {
                    Some(i) => {
                        s.selection.remove(i);
                    }
                    None => s.selection.push(target),
                }
            } else {
                s.selection = vec![target];
            }
            Ok(json!({ "ids": s.selection }))
        }
        "symbol.list" => {
            let doc = s.editor.doc();
            let rows: Vec<Value> = doc
                .resources
                .iter()
                .filter_map(|r| doc.resources.symbol(r.id).map(|d| (r, d)))
                .map(|(r, d)| {
                    let instances = cadektor_core::symbols::instance_ids(doc, r.id).len();
                    json!({"id": r.id, "name": r.name, "kind": d.kind(), "items": d.items.len(), "solids": d.solids.len(), "insertion": d.insertion, "instances": instances})
                })
                .collect();
            Ok(json!({ "symbols": rows }))
        }
        "symbol.get" => {
            let doc = s.editor.doc();
            let id = resource_ref(doc, required(m, "id")?, Some("symbol"))?;
            let mut v = resource_json(doc, id);
            v["instances"] = json!(cadektor_core::symbols::instance_ids(doc, id));
            Ok(v)
        }
        "symbol.create" => {
            let name = text(m, "name")?;
            let mut fields = m.clone();
            fields.remove("name");
            if let Some(i) = fields.remove("insertion") {
                fields.insert("insertion".into(), json!(point(&i, "insertion")?));
            }
            let g = graphic_from("symbol", &fields, None)?;
            let out = exec(s, &Command::AddResource { name, body: ResourceBody::Graphic(Box::new(g)) })?;
            Ok(json!({ "id": out.created[0] }))
        }
        "symbol.from_objects" => {
            let ids = ids_of(s, m)?;
            let name = text(m, "name")?;
            let insertion = m.get("insertion").map(|i| point(i, "insertion")).transpose()?;
            let replace = m.get("replace").and_then(Value::as_bool).unwrap_or(false);
            let out = exec(s, &Command::CreateSymbol { name, ids, insertion, replace })?;
            if let Some(inst) = out.created.get(1) {
                s.selection = vec![ObjectId(*inst)];
            }
            Ok(json!({ "id": out.created[0], "instance": out.created.get(1) }))
        }
        "symbol.place" => {
            let doc = s.editor.doc();
            let symbol = resource_ref(doc, required(m, "symbol")?, Some("symbol"))?;
            let at = point(required(m, "at")?, "at")?;
            let rotation = m.get("rotation").map(|v| num(v, "rotation")).transpose()?.unwrap_or(0.0);
            let scale = m.get("scale").map(|v| num(v, "scale")).transpose()?.unwrap_or(1.0);
            let scale_y = m.get("scale_y").filter(|v| !v.is_null()).map(|v| num(v, "scale_y")).transpose()?;
            let layer = m.get("layer").map(|v| serde_json::from_value(v.clone())).transpose().map_err(bad)?;
            let class = m.get("class").map(|v| serde_json::from_value(v.clone())).transpose().map_err(bad)?;
            let shape = Shape::Symbol { symbol, at, rotation, scale, scale_y };
            let out = exec(s, &Command::AddObject { shape, layer, class, attrs: Attributes::default() })?;
            Ok(json!({ "id": out.created[0] }))
        }
        "symbol.set" => {
            let ids = ids_of(s, m)?;
            let mut cmds = Vec::new();
            for id in ids {
                let doc = s.editor.doc();
                let o = doc.object(id).ok_or_else(|| app(format!("object {id} does not exist")))?;
                let Shape::Symbol { symbol, at, rotation, scale, scale_y } = o.shape.clone() else { return Err(app(format!("object {id} is not a symbol instance"))) };
                let symbol = match m.get("symbol") {
                    Some(v) => resource_ref(doc, v, Some("symbol"))?,
                    None => symbol,
                };
                let shape = Shape::Symbol {
                    symbol,
                    at: m.get("at").map(|v| point(v, "at")).transpose()?.unwrap_or(at),
                    rotation: m.get("rotation").map(|v| num(v, "rotation")).transpose()?.unwrap_or(rotation),
                    scale: m.get("scale").map(|v| num(v, "scale")).transpose()?.unwrap_or(scale),
                    scale_y: match m.get("scale_y") {
                        Some(Value::Null) => None,
                        Some(v) => Some(num(v, "scale_y")?),
                        None => scale_y,
                    },
                };
                cmds.push(Command::ModifyObject { id, shape: Some(shape), layer: None, class: None, attrs: None });
            }
            exec(s, &Command::Batch { commands: cmds })?;
            Ok(json!({ "ok": true }))
        }
        "symbol.convert_to_group" => {
            let ids = ids_of(s, m)?;
            let out = exec(s, &Command::ConvertSymbolToGroup { ids })?;
            s.selection = out.created.iter().map(|i| ObjectId(*i)).collect();
            Ok(json!({ "groups": out.created }))
        }
        "symbol.edit" => {
            let id: ObjectId = serde_json::from_value(required(m, "id")?.clone()).map_err(bad)?;
            let out = exec(s, &Command::EditSymbol { id })?;
            let g = ObjectId(out.created[0]);
            s.context.push(g);
            s.selection.clear();
            Ok(json!({ "group": g }))
        }
        "symbol.edit_end" => {
            let group = match m.get("group") {
                Some(v) => serde_json::from_value(v.clone()).map_err(bad)?,
                None => s.context.iter().rev().copied().find(|g| s.editor.doc().object(*g).is_some_and(|o| o.data.contains_key(cadektor_core::symbols::EDIT_GROUP_KEY))).ok_or_else(|| app("you are not editing a symbol"))?,
            };
            exec(s, &Command::ExitSymbolEdit { group })?;
            s.context.retain(|g| *g != group);
            s.selection.clear();
            Ok(json!({ "ok": true }))
        }
        "resource.list" => {
            let doc = s.editor.doc();
            let ty = m.get("type").and_then(Value::as_str);
            let search = m.get("search").and_then(Value::as_str);
            let thumbs = m.get("thumbnails").and_then(Value::as_bool).unwrap_or(false);
            let rows: Vec<Value> = library::list(doc, ty, search)
                .iter()
                .map(|i| {
                    let mut v = info_json(doc, i);
                    if thumbs {
                        v["thumbnail"] = json!(cadektor_core::thumbnail::thumbnail(doc, i.id).to_svg(64));
                    }
                    v
                })
                .collect();
            Ok(json!({ "resources": rows }))
        }
        "resource.get" => {
            let doc = s.editor.doc();
            let id = resource_ref(doc, required(m, "id")?, m.get("type").and_then(Value::as_str))?;
            Ok(resource_json(doc, id))
        }
        "resource.create" => {
            let ty = text(m, "type")?;
            let name = text(m, "name")?;
            let mut fields = m.clone();
            // Names of resources in tiles, marks and nested components can be given as names.
            resolve_names(s.editor.doc(), &mut fields)?;
            if let Some(i) = fields.remove("insertion") {
                fields.insert("insertion".into(), json!(point(&i, "insertion")?));
            }
            let g = graphic_from(&ty, &fields, None)?;
            let out = exec(s, &Command::AddResource { name, body: ResourceBody::Graphic(Box::new(g)) })?;
            Ok(json!({ "id": out.created[0] }))
        }
        "resource.update" => {
            let doc = s.editor.doc();
            let id = resource_ref(doc, required(m, "id")?, None)?;
            let cur = doc.resources.get(id).expect("exists");
            let ResourceBody::Graphic(g) = &cur.body else { return Err(bad("resource.update changes symbols, hatches, tiles, line types and textures. Use the update method of the type, such as plant_style.update")) };
            let mut fields = m.clone();
            resolve_names(doc, &mut fields)?;
            let name = match fields.remove("name") {
                None => None,
                Some(Value::String(n)) => Some(n),
                Some(_) => return Err(bad("'name' must be text")),
            };
            let new = graphic_from(g.type_name(), &fields, Some(g))?;
            exec(s, &Command::ModifyResource { id, name, body: Some(ResourceBody::Graphic(Box::new(new))) })?;
            Ok(resource_json(s.editor.doc(), id))
        }
        "resource.rename" => {
            let id = resource_ref(s.editor.doc(), required(m, "id")?, None)?;
            let name = text(m, "name")?;
            exec(s, &Command::ModifyResource { id, name: Some(name), body: None })?;
            Ok(json!({ "ok": true }))
        }
        "resource.duplicate" => {
            let doc = s.editor.doc();
            let id = resource_ref(doc, required(m, "id")?, None)?;
            let r = doc.resources.get(id).expect("exists");
            let name = match m.get("name") {
                Some(Value::String(n)) => n.clone(),
                Some(_) => return Err(bad("'name' must be text")),
                None => unique_copy_name(doc, r.body.type_name(), &r.name),
            };
            let out = exec(s, &Command::AddResource { name, body: r.body.clone() })?;
            Ok(json!({ "id": out.created[0] }))
        }
        "resource.delete" => {
            let doc = s.editor.doc();
            let id = resource_ref(doc, required(m, "id")?, None)?;
            let reassign = m.get("reassign").filter(|v| !v.is_null()).map(|v| resource_ref(doc, v, None)).transpose()?;
            exec(s, &Command::DeleteResource { id, reassign })?;
            Ok(json!({ "ok": true }))
        }
        "resource.purge_unused" => {
            let doc = s.editor.doc();
            let types: Vec<String> = match m.get("types") {
                Some(v) => serde_json::from_value(v.clone()).map_err(bad)?,
                None => library::PURGE_TYPES.iter().map(|t| t.to_string()).collect(),
            };
            let refs: Vec<&str> = types.iter().map(String::as_str).collect();
            let plan = library::purge_plan(doc, &refs);
            let removed: Vec<Value> = plan.iter().filter_map(|id| doc.resources.get(*id)).map(|r| json!({"id": r.id, "name": r.name, "type": r.body.type_name()})).collect();
            if !m.get("dry_run").and_then(Value::as_bool).unwrap_or(false) && !plan.is_empty() {
                let commands = plan.iter().map(|id| Command::DeleteResource { id: *id, reassign: None }).collect();
                exec(s, &Command::Batch { commands })?;
            }
            Ok(json!({ "removed": removed }))
        }
        "resource.thumbnail" => {
            let doc = s.editor.doc();
            let id = resource_ref(doc, required(m, "id")?, None)?;
            let size = m.get("size").and_then(Value::as_u64).unwrap_or(64).clamp(8, 1024) as u32;
            Ok(json!({ "svg": cadektor_core::thumbnail::thumbnail(doc, id).to_svg(size) }))
        }
        "pattern.segments" => {
            let doc = s.editor.doc();
            let id = resource_ref(doc, required(m, "pattern")?, None)?;
            if doc.resources.hatch(id).is_none() && doc.resources.tile(id).is_none() {
                return Err(bad("the pattern must be a hatch or a tile"));
            }
            let ring: Vec<DVec2> = serde_json::from_value(required(m, "ring")?.clone()).map_err(bad)?;
            let mut rings = vec![ring];
            if let Some(h) = m.get("holes") {
                rings.extend(serde_json::from_value::<Vec<Vec<DVec2>>>(h.clone()).map_err(bad)?);
            }
            let scale = m.get("scale").map(|v| num(v, "scale")).transpose()?.unwrap_or(100.0);
            let segs = cadektor_core::pattern::fill_segments(&doc.resources, id, &rings, scale);
            let total: f64 = segs.iter().map(|g| g.a.distance(g.b)).sum();
            let first: Vec<Value> = segs.iter().take(1000).map(|g| json!([g.a, g.b])).collect();
            Ok(json!({ "count": segs.len(), "total_length": total, "segments": first }))
        }
        #[cfg(not(target_arch = "wasm32"))]
        _ => crate::libfiles::run(s, method, m),
        #[cfg(target_arch = "wasm32")]
        _ => Err(app("this method needs files, which a browser session does not have")),
    }
}

/// Replaces names by IDs in the fields that refer to resources: `symbol` of a tile, mark elements.
fn resolve_names(doc: &Document, fields: &mut Map<String, Value>) -> Result<(), RpcError> {
    if let Some(v) = fields.get("symbol").filter(|v| v.is_string()).cloned() {
        fields.insert("symbol".into(), json!(resource_ref(doc, &v, Some("symbol"))?));
    }
    if let Some(Value::Array(els)) = fields.get_mut("elements") {
        for e in els {
            if let Some(o) = e.as_object_mut()
                && let Some(v) = o.get("symbol").filter(|v| v.is_string()).cloned()
            {
                o.insert("symbol".into(), json!(resource_ref(doc, &v, Some("symbol"))?));
            }
        }
    }
    Ok(())
}
