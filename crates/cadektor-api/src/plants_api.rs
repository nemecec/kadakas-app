//! Agent API for plants: plant styles, placement, plant schedule and existing trees.
//!
//! Methods: `plant_style.list|get|create|update|duplicate|delete|import_csv|export_csv|sample_csv`,
//! `plants.place`, `plants.set_style`, `report.plants`, `report.plant_schedule`, `report.existing_trees`.
//! Existing trees and plant tags are shapes (`existing_tree`, `plant_tag`) that `execute` adds.

use serde_json::{Map, Value, json};

use cadektor_core::DVec2;
use cadektor_core::command::Command;
use cadektor_core::curve::{self, Vertex};
use cadektor_core::model::*;
use cadektor_core::plants::catalog::{self, ImportOptions, Unit};
use cadektor_core::plants::link;
use cadektor_core::plants::placement::{self, PlaceSpec, Variation};
use cadektor_core::plants::schedule;
use cadektor_core::plants::style::PlantStyle;
use cadektor_core::resource::{ResourceBody, ResourceId};

use crate::{RpcError, Session};

/// Method descriptions. `METHODS` in the crate root lists them, so the CLI and MCP offer them.
pub const PLANT_STYLE_LIST: (&str, &str) = (
    "plant_style.list",
    "Plant styles with the number of plants that use each. Returns {styles: [{id, name, used_by, ...fields}]}.",
);
pub const PLANT_STYLE_GET: (&str, &str) = (
    "plant_style.get",
    "One plant style. params: {id} (the ID or the style name).",
);
pub const PLANT_STYLE_CREATE: (&str, &str) = (
    "plant_style.create",
    "Create a plant style. params: {name, botanical_name?, common_name?, id_code?, category? (tree|shrub|perennial|grass|groundcover), spread?, height? (mm), graphic? (circle|cloud|star|spokes|rosette|clover), form? (tree|columnar|conical|spreading|shrub_mound), fill?, pen? ([r,g,b,a]), price?, size?, container?, root?, custom? ({key: text})}. Returns {id}.",
);
pub const PLANT_STYLE_UPDATE: (&str, &str) = (
    "plant_style.update",
    "Change fields of a plant style. params: {id, name? (renames), ...fields as in create}. Plants that use the style follow, except fields they override.",
);
pub const PLANT_STYLE_DUPLICATE: (&str, &str) = (
    "plant_style.duplicate",
    "Copy a plant style. params: {id, name?}. Returns {id}.",
);
pub const PLANT_STYLE_DELETE: (&str, &str) = (
    "plant_style.delete",
    "Delete a plant style. params: {id, reassign?}. Fails if plants or tags use it, unless reassign names another style.",
);
pub const PLANT_STYLE_IMPORT_CSV: (&str, &str) = (
    "plant_style.import_csv",
    "Create plant styles from CSV text (columns are mapped by header). params: {csv, unit? (mm|cm|m, for spread and height columns without a unit), mapping? ({field: header}), on_duplicate? (skip|update, default skip)}. Returns {created, updated, skipped, warnings}.",
);
pub const PLANT_STYLE_EXPORT_CSV: (&str, &str) = (
    "plant_style.export_csv",
    "Plant styles as CSV text. params: {ids?}. Returns {csv}.",
);
pub const PLANT_STYLE_SAMPLE_CSV: (&str, &str) = (
    "plant_style.sample_csv",
    "A small sample plant catalog in CSV, to use with plant_style.import_csv.",
);
pub const PLANTS_PLACE: (&str, &str) = (
    "plants.place",
    "Place plants in one undo step. params: {mode, style? (ID or name), overrides? ({field: value}), variation? ({seed?, size? (0 to 0.9: size changes by up to this fraction), rotation? (bool)}; the same seed gives the same plants), layer?, class?, tag? (true adds a plant tag), source? (ID of a polyline, rect, circle, landscape area or hardscape to use as the path, boundary or rectangle)} and by mode: single {at}; path {path, closed?, spacing?, count?}; grid {min, max, spacing | spacing_x+spacing_y, margin?}; triangular {min, max, spacing, margin?}; fill {boundary, spacing | count | density (plants per m2), pattern? (triangular|rectangular), margin?, holes? ([[points], ...], areas that stay empty)}; poly {points}. Any spacing may be the text \"spread\": one canopy diameter of the plant. Returns {created: [id], count, tag?}.",
);
pub const PLANTS_SET_STYLE: (&str, &str) = (
    "plants.set_style",
    "Link plants to a style, change their overrides, or unlink them. params: {ids, style? (ID or name), overrides?, unlink?}.",
);
pub const REPORT_PLANT_SCHEDULE: (&str, &str) = (
    "report.plant_schedule",
    "Live plant schedule: a row per style with fields, count and prices, and totals. params: {format?: \"csv\"} adds {csv}.",
);
pub const REPORT_EXISTING_TREES: (&str, &str) = (
    "report.existing_trees",
    "Existing trees with trunk diameter, condition, action and root protection radius.",
);

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "styles": "A plant style is a document resource. A plant refers to a style (by style) and can override single fields (by instance). Plants without a style keep their own species, spread and height.",
        "place_modes": ["single", "path", "grid", "triangular", "fill", "poly"],
        "existing_tree": "add_object with shape {kind: existing_tree, at, species?, dbh, spread, height, condition? (good|fair|poor|dead), action? (retain|remove|relocate), rpz_factor? (default 12)}. The root protection radius is rpz_factor x dbh.",
        "plant_tag": "add_object with shape {kind: plant_tag, at, anchor, style, height?}, or plants.place with tag: true. Shows the ID code and the number of plants of the style on the layer.",
        "examples": [
            {"method": "plant_style.create", "params": {"name": "Field maple", "botanical_name": "Acer campestre", "id_code": "AC", "category": "tree", "spread": 7000, "height": 12000, "graphic": "cloud", "price": 185}},
            {"method": "plants.place", "params": {"mode": "fill", "style": "Field maple", "boundary": [[0,0],[20000,0],[20000,10000],[0,10000]], "spacing": 5000, "tag": true}},
            {"method": "plants.place", "params": {"mode": "path", "style": 2, "path": [[0,0],[10000,0]], "spacing": 1500}}
        ],
    })
}

fn bad(e: impl std::fmt::Display) -> RpcError {
    RpcError {
        code: -32602,
        message: e.to_string(),
    }
}

fn app(e: impl std::fmt::Display) -> RpcError {
    RpcError {
        code: 1,
        message: e.to_string(),
    }
}

fn object(p: &Value) -> Result<&Map<String, Value>, RpcError> {
    p.as_object().ok_or_else(|| bad("params must be an object"))
}

/// A style reference: an ID, or a style name.
fn style_ref(doc: &Document, v: &Value) -> Result<ResourceId, RpcError> {
    let id = match v {
        Value::Number(n) => n.as_u64().map(ResourceId),
        Value::String(s) => doc.resources.find_plant_style(s),
        _ => None,
    };
    id.filter(|id| doc.resources.plant_style(*id).is_some())
        .ok_or_else(|| bad(format!("no plant style {v}")))
}

fn required<'a>(m: &'a Map<String, Value>, key: &str) -> Result<&'a Value, RpcError> {
    m.get(key).ok_or_else(|| bad(format!("missing '{key}'")))
}

fn style_keys() -> Vec<String> {
    serde_json::to_value(PlantStyle::default())
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

/// The style after applying the fields of `patch`. A `null` clears an optional field.
fn patched(base: &PlantStyle, patch: &Map<String, Value>) -> Result<PlantStyle, RpcError> {
    let keys = style_keys();
    let mut v = serde_json::to_value(base).unwrap();
    for (k, x) in patch {
        if !keys.contains(k) {
            return Err(bad(format!(
                "unknown plant style field '{k}'. Fields: {}",
                keys.join(", ")
            )));
        }
        v[k] = x.clone();
    }
    let s: PlantStyle = serde_json::from_value(v).map_err(bad)?;
    s.validate().map_err(bad)?;
    Ok(s)
}

fn style_json(doc: &Document, id: ResourceId) -> Value {
    let r = doc.resources.get(id).expect("exists");
    let s = doc.resources.plant_style(id).expect("a plant style");
    let used = doc
        .objects
        .values()
        .filter(|o| link::link(o).is_some_and(|l| l.style == id))
        .count()
        + doc.resources.mixes_using(id).len();
    let mut v = serde_json::to_value(s).unwrap();
    v["id"] = json!(id);
    v["name"] = json!(r.name);
    v["used_by"] = json!(used);
    v
}

fn unique_copy_name(doc: &Document, base: &str) -> String {
    let first = format!("{base} copy");
    if doc.resources.find_plant_style(&first).is_none() {
        return first;
    }
    (2..)
        .map(|n| format!("{base} copy {n}"))
        .find(|n| doc.resources.find_plant_style(n).is_none())
        .unwrap()
}

/// Handles a plant method. `None` means that the method is not one of ours.
pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    let known = matches!(
        method,
        "plant_style.list"
            | "plant_style.get"
            | "plant_style.create"
            | "plant_style.update"
            | "plant_style.duplicate"
            | "plant_style.delete"
            | "plant_style.import_csv"
            | "plant_style.export_csv"
            | "plant_style.sample_csv"
            | "plants.place"
            | "plants.set_style"
            | "report.plant_schedule"
            | "report.existing_trees"
    );
    if !known {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    Some(run(s, method, &p))
}

fn run(s: &mut Session, method: &str, p: &Value) -> Result<Value, RpcError> {
    let m = object(p)?;
    match method {
        "plant_style.list" => {
            let doc = s.editor.doc();
            let ids: Vec<ResourceId> = doc.resources.plant_styles().map(|(id, _, _)| id).collect();
            Ok(
                json!({ "styles": ids.into_iter().map(|id| style_json(doc, id)).collect::<Vec<_>>() }),
            )
        }
        "plant_style.get" => {
            let doc = s.editor.doc();
            let id = style_ref(doc, required(m, "id")?)?;
            Ok(style_json(doc, id))
        }
        "plant_style.create" => {
            let name = required(m, "name")?
                .as_str()
                .ok_or_else(|| bad("'name' must be text"))?
                .to_string();
            let mut patch = m.clone();
            patch.remove("name");
            let style = patched(&PlantStyle::default(), &patch)?;
            let out = s
                .editor
                .execute(&Command::AddResource {
                    name,
                    body: ResourceBody::plant_style(style),
                })
                .map_err(app)?;
            Ok(json!({ "id": out.created[0] }))
        }
        "plant_style.update" => {
            let doc = s.editor.doc();
            let id = style_ref(doc, required(m, "id")?)?;
            let mut patch = m.clone();
            patch.remove("id");
            let name = match patch.remove("name") {
                None => None,
                Some(Value::String(n)) => Some(n),
                Some(_) => return Err(bad("'name' must be text")),
            };
            let style = patched(doc.resources.plant_style(id).expect("checked"), &patch)?;
            s.editor
                .execute(&Command::ModifyResource {
                    id,
                    name,
                    body: Some(ResourceBody::plant_style(style)),
                })
                .map_err(app)?;
            Ok(style_json(s.editor.doc(), id))
        }
        "plant_style.duplicate" => {
            let doc = s.editor.doc();
            let id = style_ref(doc, required(m, "id")?)?;
            let name = match m.get("name") {
                None => unique_copy_name(doc, &doc.resources.get(id).expect("checked").name),
                Some(Value::String(n)) => n.clone(),
                Some(_) => return Err(bad("'name' must be text")),
            };
            let style = doc.resources.plant_style(id).expect("checked").clone();
            let out = s
                .editor
                .execute(&Command::AddResource {
                    name,
                    body: ResourceBody::plant_style(style),
                })
                .map_err(app)?;
            Ok(json!({ "id": out.created[0] }))
        }
        "plant_style.delete" => {
            let doc = s.editor.doc();
            let id = style_ref(doc, required(m, "id")?)?;
            let reassign = m
                .get("reassign")
                .filter(|v| !v.is_null())
                .map(|v| style_ref(doc, v))
                .transpose()?;
            s.editor
                .execute(&Command::DeleteResource { id, reassign })
                .map_err(app)?;
            s.prune_selection();
            Ok(json!({ "ok": true }))
        }
        "plant_style.import_csv" => import_csv(s, m),
        "plant_style.export_csv" => {
            let doc = s.editor.doc();
            let ids: Option<Vec<ResourceId>> = match m.get("ids") {
                None | Some(Value::Null) => None,
                Some(Value::Array(a)) => Some(
                    a.iter()
                        .map(|v| style_ref(doc, v))
                        .collect::<Result<_, _>>()?,
                ),
                Some(_) => return Err(bad("'ids' must be a list")),
            };
            let rows = doc
                .resources
                .plant_styles()
                .filter(|(id, _, _)| ids.as_ref().is_none_or(|l| l.contains(id)));
            Ok(json!({ "csv": catalog::export(rows.map(|(_, n, st)| (n, st))) }))
        }
        "plant_style.sample_csv" => Ok(json!({ "csv": catalog::SAMPLE_CSV })),
        "plants.place" => place(s, m),
        "plants.set_style" => {
            let doc = s.editor.doc();
            let ids: Vec<ObjectId> =
                serde_json::from_value(required(m, "ids")?.clone()).map_err(bad)?;
            let style = m
                .get("style")
                .filter(|v| !v.is_null())
                .map(|v| style_ref(doc, v))
                .transpose()?;
            let overrides = match m.get("overrides") {
                None | Some(Value::Null) => None,
                Some(Value::Object(o)) => Some(o.clone()),
                Some(_) => return Err(bad("'overrides' must be an object")),
            };
            let unlink = m.get("unlink").and_then(Value::as_bool).unwrap_or(false);
            s.editor
                .execute(&Command::SetPlantStyle {
                    ids,
                    style,
                    overrides,
                    unlink,
                })
                .map_err(app)?;
            Ok(json!({ "ok": true }))
        }
        "report.plant_schedule" => {
            let sch = schedule::schedule(s.editor.doc());
            let mut v = serde_json::to_value(&sch).unwrap();
            if m.get("format").and_then(Value::as_str) == Some("csv") {
                v["csv"] = json!(schedule::schedule_csv(&sch));
            }
            Ok(v)
        }
        "report.existing_trees" => Ok(json!({ "rows": schedule::existing_trees(s.editor.doc()) })),
        _ => unreachable!("handle checks the method"),
    }
}

/// `report.plants`: counts by style or species. Each row has `species` and `count`. Rows of plants
/// with a style also have `style`, `style_name`, `id_code` and the prices.
pub fn report_plants(doc: &Document) -> Value {
    let sch = schedule::schedule(doc);
    let rows: Vec<Value> = sch
        .rows
        .iter()
        .map(|r| {
            let species = if r.botanical_name.trim().is_empty() {
                r.name.clone()
            } else {
                r.botanical_name.clone()
            };
            let mut row = json!({ "species": species, "count": r.count });
            if r.style.is_some() {
                row["style"] = json!(r.style);
                row["style_name"] = json!(r.name);
                row["id_code"] = json!(r.id_code);
                row["unit_price"] = json!(r.unit_price);
                row["total_price"] = json!(r.total_price);
            }
            row
        })
        .collect();
    json!({ "rows": rows })
}

fn import_csv(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let csv = required(m, "csv")?
        .as_str()
        .ok_or_else(|| bad("'csv' must be text"))?;
    let mut opts = ImportOptions::default();
    if let Some(u) = m.get("unit").filter(|v| !v.is_null()) {
        opts.unit = u
            .as_str()
            .and_then(Unit::from_name)
            .ok_or_else(|| bad("'unit' must be mm, cm or m"))?;
    }
    if let Some(map) = m.get("mapping").filter(|v| !v.is_null()) {
        opts.mapping = serde_json::from_value(map.clone())
            .map_err(|_| bad("'mapping' must be {field: header}"))?;
    }
    let update = match m.get("on_duplicate").and_then(Value::as_str) {
        None | Some("skip") => false,
        Some("update") => true,
        Some(_) => return Err(bad("'on_duplicate' must be skip or update")),
    };
    let result = catalog::import(csv, &opts).map_err(bad)?;
    let doc = s.editor.doc();
    let (mut commands, mut skipped) = (Vec::new(), Vec::new());
    let (mut created, mut updated) = (0usize, Vec::new());
    for imp in result.styles {
        match doc.resources.find_plant_style(&imp.name) {
            None => {
                commands.push(Command::AddResource {
                    name: imp.name,
                    body: ResourceBody::plant_style(imp.style),
                });
                created += 1;
            }
            Some(id) if update => {
                let merged = imp.apply_to(doc.resources.plant_style(id).expect("exists"));
                commands.push(Command::ModifyResource {
                    id,
                    name: None,
                    body: Some(ResourceBody::plant_style(merged)),
                });
                updated.push(id);
            }
            Some(_) => skipped.push(imp.name),
        }
    }
    let out = s
        .editor
        .execute(&Command::Batch { commands })
        .map_err(app)?;
    Ok(
        json!({ "created": out.created, "created_count": created, "updated": updated, "skipped": skipped, "warnings": result.warnings }),
    )
}

/// Takes the path, boundary or rectangle for a placement mode from an existing object.
fn use_source(doc: &Document, id: ObjectId, m: &mut Map<String, Value>) -> Result<(), RpcError> {
    let o = doc
        .object(id)
        .ok_or_else(|| bad(format!("object {id} does not exist")))?;
    let (verts, closed): (Vec<Vertex>, bool) = match &o.shape {
        Shape::Polyline { points, closed } => (points.clone(), *closed),
        Shape::LandscapeArea { boundary, .. } | Shape::Hardscape { boundary, .. } => {
            (boundary.clone(), true)
        }
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            (
                curve::corners(&[lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)]),
                true,
            )
        }
        Shape::Circle { center, radius } => {
            let pts: Vec<DVec2> = (0..64)
                .map(|k| {
                    *center + DVec2::from_angle(std::f64::consts::TAU * k as f64 / 64.0) * *radius
                })
                .collect();
            (curve::corners(&pts), true)
        }
        other => {
            return Err(bad(format!(
                "a {} cannot be a placement source",
                other.kind()
            )));
        }
    };
    let (lo, hi) = o.shape.bounds();
    let set = |m: &mut Map<String, Value>, k: &str, v: Value| {
        m.entry(k.to_string()).or_insert(v);
    };
    set(m, "path", serde_json::to_value(&verts).unwrap());
    set(m, "closed", json!(closed));
    set(m, "boundary", serde_json::to_value(&verts).unwrap());
    set(m, "points", serde_json::to_value(&verts).unwrap());
    set(m, "min", json!(lo));
    set(m, "max", json!(hi));
    Ok(())
}

/// The spread of the plants that a `plants.place` request makes.
fn spread_of(doc: &Document, m: &Map<String, Value>) -> Result<f64, RpcError> {
    let style = m
        .get("style")
        .filter(|v| !v.is_null())
        .map(|v| style_ref(doc, v))
        .transpose()?;
    let overrides = match m.get("overrides") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(o)) => o.clone(),
        Some(_) => return Err(bad("'overrides' must be an object")),
    };
    let base = style
        .and_then(|id| doc.resources.plant_style(id))
        .cloned()
        .unwrap_or_default();
    Ok(link::merge(&base, &overrides).map_err(bad)?.spread)
}

fn place(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let doc = s.editor.doc();
    let mut spec = m.clone();
    if let Some(src) = spec.remove("source").filter(|v| !v.is_null()) {
        use_source(doc, serde_json::from_value(src).map_err(bad)?, &mut spec)?;
    }
    for k in ["style", "overrides", "layer", "class", "tag", "variation"] {
        spec.remove(k);
    }
    // `"spacing": "spread"` means one canopy diameter, from the style and its overrides.
    if ["spacing", "spacing_x", "spacing_y"]
        .iter()
        .any(|k| spec.get(*k).and_then(Value::as_str) == Some("spread"))
    {
        let spread = spread_of(doc, m)?;
        for k in ["spacing", "spacing_x", "spacing_y"] {
            if spec.get(k).and_then(Value::as_str) == Some("spread") {
                spec.insert(k.into(), json!(spread));
            }
        }
    }
    if !spec.contains_key("mode") {
        return Err(bad(
            "missing 'mode'. Modes: single, path, grid, triangular, fill, poly",
        ));
    }
    let spec: PlaceSpec = serde_json::from_value(Value::Object(spec)).map_err(bad)?;
    let positions = placement::place(&spec).map_err(bad)?;

    let style = m
        .get("style")
        .filter(|v| !v.is_null())
        .map(|v| style_ref(doc, v))
        .transpose()?;
    let overrides = match m.get("overrides") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(o)) => o.clone(),
        Some(_) => return Err(bad("'overrides' must be an object")),
    };
    let id_of = |k: &str| -> Result<Option<u64>, RpcError> {
        m.get(k)
            .filter(|v| !v.is_null())
            .map(|v| {
                v.as_u64()
                    .ok_or_else(|| bad(format!("'{k}' must be an ID")))
            })
            .transpose()
    };
    let layer = id_of("layer")?.map(LayerId);
    let class = id_of("class")?.map(ClassId);
    let variation: Option<Variation> = match m.get("variation") {
        None | Some(Value::Null) => None,
        Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| {
            bad(format!(
                "'variation' must be {{seed?, size?, rotation?}}: {e}"
            ))
        })?),
    };
    let count = positions.len();
    let mut commands = vec![Command::AddPlants {
        style,
        overrides: overrides.clone(),
        positions: positions.clone(),
        layer,
        class,
        variation,
    }];
    let want_tag = match m.get("tag") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Bool(true)) => true,
        Some(_) => return Err(bad("'tag' must be true or false")),
    };
    if want_tag {
        let sid = style.ok_or_else(|| bad("a plant tag needs a style"))?;
        let spread = link::merge(doc.resources.plant_style(sid).expect("checked"), &overrides)
            .map_err(bad)?
            .spread;
        commands.push(Command::AddObject {
            shape: schedule::group_tag(sid, &positions, spread),
            layer,
            class,
            attrs: Attributes::default(),
        });
    }
    let out = s
        .editor
        .execute(&Command::Batch { commands })
        .map_err(app)?;
    let mut created = out.created;
    let tag = if want_tag { created.pop() } else { None };
    Ok(json!({ "created": created, "count": count, "tag": tag }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &mut Session, method: &str, params: Value) -> Value {
        match s.handle(method, params) {
            Ok(v) => v,
            Err(e) => panic!("{method}: {}", e.message),
        }
    }

    fn fail(s: &mut Session, method: &str, params: Value) -> RpcError {
        s.handle(method, params).expect_err(method)
    }

    fn maple(s: &mut Session) -> u64 {
        ok(s, "plant_style.create", json!({"name": "Field maple", "botanical_name": "Acer campestre", "id_code": "AC", "category": "tree", "spread": 7000, "height": 12000, "graphic": "cloud", "price": 100}))["id"]
            .as_u64()
            .unwrap()
    }

    #[test]
    fn style_crud() {
        let mut s = Session::default();
        let id = maple(&mut s);
        let list = ok(&mut s, "plant_style.list", json!({}));
        assert_eq!(list["styles"][0]["name"], "Field maple");
        assert_eq!(list["styles"][0]["used_by"], 0);
        assert_eq!(
            ok(&mut s, "plant_style.get", json!({"id": "field MAPLE"}))["id"],
            id
        );
        let u = ok(
            &mut s,
            "plant_style.update",
            json!({"id": id, "name": "Maple", "spread": 5000, "fill": [1, 2, 3, 255]}),
        );
        assert_eq!(
            (u["name"].clone(), u["spread"].clone(), u["fill"].clone()),
            (json!("Maple"), json!(5000.0), json!([1, 2, 3, 255]))
        );
        let u = ok(
            &mut s,
            "plant_style.update",
            json!({"id": "Maple", "fill": null}),
        );
        assert_eq!(u["fill"], Value::Null);
        let d = ok(&mut s, "plant_style.duplicate", json!({"id": id}));
        assert_eq!(
            ok(&mut s, "plant_style.get", json!({"id": d["id"]}))["name"],
            "Maple copy"
        );
        let d2 = ok(&mut s, "plant_style.duplicate", json!({"id": id}));
        assert_eq!(
            ok(&mut s, "plant_style.get", json!({"id": d2["id"]}))["name"],
            "Maple copy 2"
        );
        ok(
            &mut s,
            "plant_style.duplicate",
            json!({"id": id, "name": "Own name"}),
        );
        ok(&mut s, "plant_style.delete", json!({"id": d["id"]}));
        assert_eq!(
            ok(&mut s, "plant_style.list", json!({}))["styles"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        // Undo goes step by step.
        assert_eq!(ok(&mut s, "undo", json!({}))["done"], true);
        assert_eq!(
            ok(&mut s, "plant_style.list", json!({}))["styles"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }

    #[test]
    fn style_errors() {
        let mut s = Session::default();
        let id = maple(&mut s);
        assert_eq!(
            fail(&mut s, "plant_style.create", json!({"name": "Field maple"})).code,
            1
        );
        assert_eq!(
            fail(&mut s, "plant_style.create", json!({"spread": 10})).code,
            -32602
        );
        assert_eq!(
            fail(&mut s, "plant_style.create", json!({"name": 5})).code,
            -32602
        );
        assert_eq!(
            fail(
                &mut s,
                "plant_style.create",
                json!({"name": "X", "colour": "red"})
            )
            .code,
            -32602
        );
        assert_eq!(
            fail(
                &mut s,
                "plant_style.create",
                json!({"name": "X", "spread": -1})
            )
            .code,
            -32602
        );
        assert_eq!(
            fail(
                &mut s,
                "plant_style.create",
                json!({"name": "X", "category": "cactus"})
            )
            .code,
            -32602
        );
        assert_eq!(
            fail(&mut s, "plant_style.get", json!({"id": 999})).code,
            -32602
        );
        assert_eq!(
            fail(&mut s, "plant_style.get", json!({"id": true})).code,
            -32602
        );
        assert_eq!(fail(&mut s, "plant_style.get", json!([1])).code, -32602);
        assert_eq!(
            fail(&mut s, "plant_style.update", json!({"id": id, "name": 7})).code,
            -32602
        );
        assert_eq!(
            fail(&mut s, "plant_style.update", json!({"id": id, "name": ""})).code,
            1
        );
        assert_eq!(
            fail(
                &mut s,
                "plant_style.duplicate",
                json!({"id": id, "name": 7})
            )
            .code,
            -32602
        );
        assert_eq!(
            fail(
                &mut s,
                "plant_style.duplicate",
                json!({"id": id, "name": "Field maple"})
            )
            .code,
            1
        );
        assert_eq!(
            fail(
                &mut s,
                "plant_style.delete",
                json!({"id": id, "reassign": 999})
            )
            .code,
            -32602
        );
    }

    #[test]
    fn delete_in_use_is_refused_and_reassign_works() {
        let mut s = Session::default();
        let a = maple(&mut s);
        let b = ok(
            &mut s,
            "plant_style.create",
            json!({"name": "Birch", "spread": 3000}),
        )["id"]
            .as_u64()
            .unwrap();
        ok(
            &mut s,
            "plants.place",
            json!({"mode": "single", "style": a, "at": [0, 0]}),
        );
        let e = fail(&mut s, "plant_style.delete", json!({"id": a}));
        assert!(e.message.contains("used by 1"), "{}", e.message);
        assert_eq!(
            ok(&mut s, "plant_style.list", json!({}))["styles"][0]["used_by"],
            1
        );
        ok(
            &mut s,
            "plant_style.delete",
            json!({"id": "Field maple", "reassign": "Birch"}),
        );
        let r = ok(&mut s, "report.plant_schedule", json!({}));
        assert_eq!(r["rows"][0]["name"], "Birch");
        assert_eq!(r["rows"][0]["style"], b);
    }

    fn square(side: f64) -> Value {
        json!([[0, 0], [side, 0], [side, side], [0, side]])
    }

    #[test]
    fn place_in_every_mode() {
        let mut s = Session::default();
        let id = maple(&mut s);
        let count = |s: &mut Session, params: Value| {
            ok(s, "plants.place", params)["count"].as_u64().unwrap()
        };
        assert_eq!(
            count(
                &mut s,
                json!({"mode": "single", "style": id, "at": [10, 20]})
            ),
            1
        );
        assert_eq!(
            count(
                &mut s,
                json!({"mode": "path", "style": id, "path": [[0, 0], [1000, 0]], "spacing": 250})
            ),
            5
        );
        assert_eq!(
            count(
                &mut s,
                json!({"mode": "path", "style": id, "path": [[0, 0], [1000, 0]], "count": 3})
            ),
            3
        );
        assert_eq!(
            count(
                &mut s,
                json!({"mode": "path", "style": id, "path": [[0, 0], [1000, 0]], "count": 3, "spacing": 100})
            ),
            3
        );
        assert_eq!(
            count(
                &mut s,
                json!({"mode": "grid", "style": id, "min": [0, 0], "max": [1000, 500], "spacing_x": 500, "spacing_y": 250})
            ),
            9
        );
        assert_eq!(
            count(
                &mut s,
                json!({"mode": "grid", "style": id, "min": [0, 0], "max": [1000, 1000], "spacing": 500})
            ),
            9
        );
        assert_eq!(
            count(
                &mut s,
                json!({"mode": "triangular", "style": id, "min": [0, 0], "max": [1000, 1000], "spacing": 500})
            ),
            8
        );
        assert_eq!(
            count(
                &mut s,
                json!({"mode": "fill", "style": id, "boundary": square(2000.0), "spacing": 500, "pattern": "rectangular"})
            ),
            25
        );
        assert_eq!(
            count(
                &mut s,
                json!({"mode": "fill", "style": id, "boundary": square(2000.0), "count": 13})
            ),
            13
        );
        assert_eq!(
            count(
                &mut s,
                json!({"mode": "fill", "style": id, "boundary": square(2000.0), "density": 4, "pattern": "rectangular"})
            ),
            25
        );
        assert_eq!(
            count(
                &mut s,
                json!({"mode": "poly", "style": id, "points": [[0, 0], [5, 5], [9, 9]]})
            ),
            3
        );
        let r = ok(&mut s, "report.plants", json!({}));
        assert_eq!(
            r["rows"][0]["count"],
            1 + 5 + 3 + 3 + 9 + 9 + 8 + 25 + 13 + 25 + 3
        );
        assert_eq!(r["rows"][0]["species"], "Acer campestre");
        assert_eq!(
            r["rows"][0]["total_price"],
            100.0 * r["rows"][0]["count"].as_f64().unwrap()
        );
    }

    #[test]
    fn placement_is_one_undo_step_with_overrides_and_layers() {
        let mut s = Session::default();
        let id = maple(&mut s);
        let layer = ok(
            &mut s,
            "execute",
            json!({"command": {"op": "add_layer", "name": "Planting"}}),
        )["created"][0]
            .as_u64()
            .unwrap();
        let r = ok(
            &mut s,
            "plants.place",
            json!({"mode": "fill", "style": id, "boundary": square(3000.0), "spacing": 1000, "overrides": {"price": 10, "height": 999}, "layer": layer, "tag": true}),
        );
        let n = r["count"].as_u64().unwrap();
        assert!(n > 4 && r["tag"].is_u64());
        let objs = ok(&mut s, "objects.list", json!({"kind": "plant"}));
        assert_eq!(objs.as_array().unwrap().len() as u64, n);
        assert!(
            objs.as_array()
                .unwrap()
                .iter()
                .all(|o| o["layer"] == layer && o["shape"]["height"] == 999.0)
        );
        let tags = ok(&mut s, "objects.list", json!({"kind": "plant_tag"}));
        assert_eq!(tags.as_array().unwrap().len(), 1);
        let sch = ok(&mut s, "report.plant_schedule", json!({}));
        assert_eq!(
            (sch["total_count"].clone(), sch["total_price"].clone()),
            (json!(n), json!(10.0 * n as f64))
        );
        // One undo removes plants and tag.
        assert_eq!(ok(&mut s, "undo", json!({}))["done"], true);
        assert_eq!(
            ok(&mut s, "objects.list", json!({}))
                .as_array()
                .unwrap()
                .len(),
            0
        );
        ok(&mut s, "redo", json!({}));
        assert_eq!(
            ok(&mut s, "objects.list", json!({}))
                .as_array()
                .unwrap()
                .len() as u64,
            n + 1
        );
    }

    #[test]
    fn spacing_can_be_the_spread_and_fill_can_have_holes() {
        let mut s = Session::default();
        let id = ok(
            &mut s,
            "plant_style.create",
            json!({"name": "Box", "spread": 500}),
        )["id"]
            .as_u64()
            .unwrap();
        let n = |s: &mut Session, p: Value| ok(s, "plants.place", p)["count"].as_u64().unwrap();
        assert_eq!(
            n(
                &mut s,
                json!({"mode": "path", "style": id, "path": [[0, 0], [2000, 0]], "spacing": "spread"})
            ),
            5
        );
        // Overrides change the spread.
        assert_eq!(
            n(
                &mut s,
                json!({"mode": "path", "style": id, "overrides": {"spread": 1000}, "path": [[0, 0], [2000, 0]], "spacing": "spread"})
            ),
            3
        );
        assert_eq!(
            n(
                &mut s,
                json!({"mode": "grid", "style": "Box", "min": [0, 0], "max": [1000, 1000], "spacing_x": "spread", "spacing_y": 1000})
            ),
            6
        );
        assert_eq!(
            n(
                &mut s,
                json!({"mode": "single", "at": [0, 0], "overrides": {"spread": 700}})
            ),
            1
        );
        let hole = json!([[1000, 1000], [3000, 1000], [3000, 3000], [1000, 3000]]);
        let sq = json!([[0, 0], [4000, 0], [4000, 4000], [0, 4000]]);
        let full = n(
            &mut s,
            json!({"mode": "fill", "style": id, "boundary": sq, "spacing": 500, "pattern": "rectangular"}),
        );
        let holed = n(
            &mut s,
            json!({"mode": "fill", "style": id, "boundary": sq, "holes": [hole], "spacing": 500, "pattern": "rectangular"}),
        );
        assert_eq!(full - holed, 9);
        assert_eq!(fail(&mut s, "plants.place", json!({"mode": "path", "path": [[0, 0], [9, 0]], "spacing": "spread", "overrides": {"spread": -1}})).code, -32602);
        assert_eq!(
            fail(
                &mut s,
                "plants.place",
                json!({"mode": "path", "style": 999, "path": [[0, 0], [9, 0]], "spacing": "spread"})
            )
            .code,
            -32602
        );
        assert_eq!(fail(&mut s, "plants.place", json!({"mode": "path", "path": [[0, 0], [9, 0]], "spacing": "spread", "overrides": 4})).code, -32602);
        // A path with the default style spread of 3000 mm.
        assert_eq!(
            n(
                &mut s,
                json!({"mode": "path", "path": [[0, 0], [6000, 0]], "spacing": "spread"})
            ),
            3
        );
    }

    #[test]
    fn variation_locked_fields_area_and_zone_through_the_api() {
        let mut s = Session::default();
        let id = ok(&mut s, "plant_style.create", json!({"name": "Oak", "category": "tree", "spread": 6000, "height": 9000, "trunk_diameter": 400, "protection_zone": true, "locked": ["price"], "price": 50}))["id"].as_u64().unwrap();
        let r = ok(
            &mut s,
            "plants.place",
            json!({"mode": "poly", "style": id, "points": [[0, 0], [9000, 0], [18000, 0], [27000, 0]], "variation": {"seed": 4, "size": 0.2, "rotation": true}}),
        );
        let first = ok(&mut s, "objects.get", json!({"id": r["created"][0]}));
        assert!(first["data"]["rotation"].is_number());
        let spreads: Vec<f64> = r["created"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                ok(&mut s, "objects.get", json!({"id": i}))["shape"]["spread"]
                    .as_f64()
                    .unwrap()
            })
            .collect();
        assert!(
            spreads.iter().all(|x| (4800.0..=7200.0).contains(x))
                && spreads.windows(2).any(|w| w[0] != w[1])
        );
        // Same seed, same plants.
        let r2 = ok(
            &mut s,
            "plants.place",
            json!({"mode": "poly", "style": id, "points": [[0, 5000], [9000, 5000], [18000, 5000], [27000, 5000]], "variation": {"seed": 4, "size": 0.2, "rotation": true}}),
        );
        let spreads2: Vec<f64> = r2["created"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                ok(&mut s, "objects.get", json!({"id": i}))["shape"]["spread"]
                    .as_f64()
                    .unwrap()
            })
            .collect();
        assert_eq!(spreads, spreads2);
        assert_eq!(
            fail(
                &mut s,
                "plants.place",
                json!({"mode": "single", "at": [0, 0], "variation": {"size": 5}})
            )
            .code,
            1
        );
        assert_eq!(
            fail(
                &mut s,
                "plants.place",
                json!({"mode": "single", "at": [0, 0], "variation": 3})
            )
            .code,
            -32602
        );
        // Locked price.
        assert_eq!(
            fail(
                &mut s,
                "plants.place",
                json!({"mode": "single", "style": id, "at": [0, 0], "overrides": {"price": 1}})
            )
            .code,
            1
        );
        assert_eq!(
            fail(
                &mut s,
                "plant_style.update",
                json!({"id": id, "locked": ["nope"]})
            )
            .code,
            -32602
        );
        // The schedule keeps varied plants in one row.
        let sch = ok(&mut s, "report.plant_schedule", json!({}));
        assert_eq!(
            (
                sch["rows"].as_array().unwrap().len(),
                sch["total_count"].clone()
            ),
            (1, json!(8))
        );
        // Quantity by area.
        let gc = ok(&mut s, "plant_style.create", json!({"name": "Thyme", "category": "groundcover", "spread": 1000, "price": 10, "quantity": "area"}))["id"].as_u64().unwrap();
        ok(
            &mut s,
            "plants.place",
            json!({"mode": "single", "style": gc, "at": [0, 0]}),
        );
        let sch = ok(&mut s, "report.plant_schedule", json!({"format": "csv"}));
        let row = sch["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == "Thyme")
            .unwrap()
            .clone();
        assert!((row["area_m2"].as_f64().unwrap() - std::f64::consts::PI / 4.0).abs() < 1e-9);
        assert!(sch["total_area_m2"].as_f64().unwrap() > 0.7);
        assert!(sch["csv"].as_str().unwrap().contains("area"));
        // The protection zone is drawn at 12 x the trunk diameter.
        assert!(
            ok(&mut s, "render.svg", json!({}))["svg"]
                .as_str()
                .unwrap()
                .contains(r#"data-kind="tree_protection_zone""#)
        );
    }

    #[test]
    fn place_from_a_source_object() {
        let mut s = Session::default();
        let id = maple(&mut s);
        let add = |s: &mut Session, shape: Value| {
            ok(
                s,
                "execute",
                json!({"command": {"op": "add_object", "shape": shape}}),
            )["created"][0]
                .clone()
        };
        let pl = add(
            &mut s,
            json!({"kind": "polyline", "closed": false, "points": [[0, 0], [2000, 0]]}),
        );
        let rect = add(
            &mut s,
            json!({"kind": "rect", "min": [0, 0], "max": [2000, 1000]}),
        );
        let circle = add(
            &mut s,
            json!({"kind": "circle", "center": [0, 0], "radius": 1000}),
        );
        let area = add(
            &mut s,
            json!({"kind": "landscape_area", "name": "Bed", "boundary": [[0, 0], [2000, 0], [2000, 2000], [0, 2000]]}),
        );
        let text = add(
            &mut s,
            json!({"kind": "text", "at": [0, 0], "text": "x", "height": 10}),
        );
        assert_eq!(
            ok(
                &mut s,
                "plants.place",
                json!({"mode": "path", "style": id, "source": pl, "spacing": 500})
            )["count"],
            5
        );
        assert_eq!(
            ok(
                &mut s,
                "plants.place",
                json!({"mode": "grid", "style": id, "source": rect, "spacing": 1000})
            )["count"],
            6
        );
        assert_eq!(
            ok(
                &mut s,
                "plants.place",
                json!({"mode": "fill", "style": id, "source": area, "spacing": 1000, "pattern": "rectangular"})
            )["count"],
            9
        );
        assert_eq!(
            ok(
                &mut s,
                "plants.place",
                json!({"mode": "fill", "style": id, "source": circle, "count": 5})
            )["count"],
            5
        );
        assert_eq!(
            ok(
                &mut s,
                "plants.place",
                json!({"mode": "poly", "style": id, "source": pl})
            )["count"],
            2
        );
        assert_eq!(
            fail(
                &mut s,
                "plants.place",
                json!({"mode": "poly", "style": id, "source": text})
            )
            .code,
            -32602
        );
        assert_eq!(
            fail(
                &mut s,
                "plants.place",
                json!({"mode": "poly", "style": id, "source": 9999})
            )
            .code,
            -32602
        );
    }

    #[test]
    fn place_errors() {
        let mut s = Session::default();
        let id = maple(&mut s);
        let code = |s: &mut Session, p: Value| fail(s, "plants.place", p).code;
        assert_eq!(code(&mut s, json!({"style": id})), -32602);
        assert_eq!(code(&mut s, json!({"mode": "spiral"})), -32602);
        assert_eq!(code(&mut s, json!({"mode": "single"})), -32602);
        assert_eq!(
            code(&mut s, json!({"mode": "path", "path": [[0, 0], [10, 0]]})),
            -32602
        );
        assert_eq!(
            code(
                &mut s,
                json!({"mode": "single", "at": [0, 0], "style": 999})
            ),
            -32602
        );
        assert_eq!(
            code(
                &mut s,
                json!({"mode": "single", "at": [0, 0], "overrides": 5})
            ),
            -32602
        );
        assert_eq!(
            code(&mut s, json!({"mode": "single", "at": [0, 0], "tag": true})),
            -32602
        );
        assert_eq!(
            code(
                &mut s,
                json!({"mode": "single", "at": [0, 0], "style": id, "tag": "yes"})
            ),
            -32602
        );
        assert_eq!(
            code(
                &mut s,
                json!({"mode": "single", "at": [0, 0], "layer": "x"})
            ),
            -32602
        );
        assert_eq!(
            code(
                &mut s,
                json!({"mode": "single", "at": [0, 0], "layer": 999})
            ),
            1
        );
        assert_eq!(
            code(
                &mut s,
                json!({"mode": "single", "at": [0, 0], "style": id, "overrides": {"bogus": 1}})
            ),
            1
        );
        assert_eq!(code(&mut s, json!("x")), -32602);
        assert_eq!(
            ok(&mut s, "objects.list", json!({}))
                .as_array()
                .unwrap()
                .len(),
            0
        );
        // No tag without a style, but an unstyled plant is fine.
        let r = ok(
            &mut s,
            "plants.place",
            json!({"mode": "single", "at": [0, 0], "overrides": {"botanical_name": "Oak", "spread": 900}, "tag": false}),
        );
        assert_eq!(r["count"], 1);
        assert_eq!(
            ok(&mut s, "report.plants", json!({}))["rows"][0]["species"],
            "Oak"
        );
    }

    #[test]
    fn set_style_links_and_unlinks() {
        let mut s = Session::default();
        let id = maple(&mut s);
        let r = ok(
            &mut s,
            "plants.place",
            json!({"mode": "poly", "points": [[0, 0], [5, 5]]}),
        );
        let ids = r["created"].clone();
        ok(
            &mut s,
            "plants.set_style",
            json!({"ids": ids, "style": "Field maple", "overrides": {"price": 1}}),
        );
        assert_eq!(
            ok(&mut s, "report.plants", json!({}))["rows"][0]["total_price"],
            2.0
        );
        let o = ok(&mut s, "objects.get", json!({"id": ids[0]}));
        assert_eq!(o["data"]["plant"]["style"], id);
        assert_eq!(o["shape"]["spread"], 7000.0);
        ok(
            &mut s,
            "plants.set_style",
            json!({"ids": ids, "unlink": true}),
        );
        assert!(
            ok(&mut s, "objects.get", json!({"id": ids[0]}))["data"]
                .get("plant")
                .is_none()
        );
        assert_eq!(
            fail(&mut s, "plants.set_style", json!({"ids": ids})).code,
            1
        );
        assert_eq!(
            fail(&mut s, "plants.set_style", json!({"ids": "x"})).code,
            -32602
        );
        assert_eq!(
            fail(
                &mut s,
                "plants.set_style",
                json!({"ids": ids, "overrides": 3})
            )
            .code,
            -32602
        );
        assert_eq!(fail(&mut s, "plants.set_style", json!({})).code, -32602);
    }

    #[test]
    fn csv_import_export_round_trip() {
        let mut s = Session::default();
        let sample = ok(&mut s, "plant_style.sample_csv", json!({}))["csv"]
            .as_str()
            .unwrap()
            .to_string();
        let r = ok(&mut s, "plant_style.import_csv", json!({"csv": sample}));
        assert_eq!(r["created_count"], 8);
        assert!(r["warnings"].as_array().unwrap().is_empty());
        let exported = ok(&mut s, "plant_style.export_csv", json!({}))["csv"]
            .as_str()
            .unwrap()
            .to_string();
        // Importing again into a new session gives equal styles.
        let mut t = Session::default();
        ok(&mut t, "plant_style.import_csv", json!({"csv": exported}));
        let a = ok(&mut s, "plant_style.list", json!({}));
        let b = ok(&mut t, "plant_style.list", json!({}));
        let strip = |v: &Value| -> Vec<Value> {
            v["styles"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| {
                    let mut x = x.clone();
                    x.as_object_mut().unwrap().remove("id");
                    x
                })
                .collect()
        };
        assert_eq!(strip(&a), strip(&b));
        // Duplicates are skipped, or updated on request. One undo step each.
        let r = ok(
            &mut s,
            "plant_style.import_csv",
            json!({"csv": "name,spread (m)\nField maple,9\nNew one,2\n"}),
        );
        assert_eq!(
            (r["created_count"].clone(), r["skipped"].clone()),
            (json!(1), json!(["Field maple"]))
        );
        let r = ok(
            &mut s,
            "plant_style.import_csv",
            json!({"csv": "name,spread (m)\nField maple,9\n", "on_duplicate": "update"}),
        );
        assert_eq!(r["updated"].as_array().unwrap().len(), 1);
        assert_eq!(
            ok(&mut s, "plant_style.get", json!({"id": "Field maple"}))["spread"],
            9000.0
        );
        ok(&mut s, "undo", json!({}));
        assert_eq!(
            ok(&mut s, "plant_style.get", json!({"id": "Field maple"}))["spread"],
            7000.0
        );
        // Selected export, units and mapping.
        let one = ok(
            &mut s,
            "plant_style.export_csv",
            json!({"ids": ["Field maple"]}),
        )["csv"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(one.lines().count(), 2);
        let mut u = Session::default();
        ok(
            &mut u,
            "plant_style.import_csv",
            json!({"csv": "Plant;W;H\nA;2;3\n", "unit": "m", "mapping": {"name": "Plant", "spread": "W", "height": "H"}}),
        );
        assert_eq!(
            ok(&mut u, "plant_style.get", json!({"id": "A"}))["height"],
            3000.0
        );
    }

    #[test]
    fn csv_errors() {
        let mut s = Session::default();
        let code = |s: &mut Session, m: &str, p: Value| fail(s, m, p).code;
        assert_eq!(code(&mut s, "plant_style.import_csv", json!({})), -32602);
        assert_eq!(
            code(&mut s, "plant_style.import_csv", json!({"csv": 5})),
            -32602
        );
        assert_eq!(
            code(&mut s, "plant_style.import_csv", json!({"csv": ""})),
            -32602
        );
        assert_eq!(
            code(
                &mut s,
                "plant_style.import_csv",
                json!({"csv": "name\nA", "unit": "ft"})
            ),
            -32602
        );
        assert_eq!(
            code(
                &mut s,
                "plant_style.import_csv",
                json!({"csv": "name\nA", "unit": 5})
            ),
            -32602
        );
        assert_eq!(
            code(
                &mut s,
                "plant_style.import_csv",
                json!({"csv": "name\nA", "mapping": 5})
            ),
            -32602
        );
        assert_eq!(
            code(
                &mut s,
                "plant_style.import_csv",
                json!({"csv": "name\nA", "on_duplicate": "x"})
            ),
            -32602
        );
        assert_eq!(
            code(&mut s, "plant_style.export_csv", json!({"ids": 5})),
            -32602
        );
        assert_eq!(
            code(&mut s, "plant_style.export_csv", json!({"ids": [999]})),
            -32602
        );
        assert_eq!(
            ok(&mut s, "plant_style.export_csv", json!({"ids": null}))["csv"]
                .as_str()
                .unwrap()
                .lines()
                .count(),
            1
        );
    }

    #[test]
    fn schedule_csv_and_existing_trees() {
        let mut s = Session::default();
        maple(&mut s);
        ok(
            &mut s,
            "plants.place",
            json!({"mode": "poly", "style": "Field maple", "points": [[0, 0], [5, 5]]}),
        );
        let r = ok(&mut s, "report.plant_schedule", json!({"format": "csv"}));
        assert_eq!(r["rows"][0]["count"], 2);
        assert_eq!(r["rows"][0]["total_price"], 200.0);
        assert_eq!(r["rows"][0]["id_code"], "AC");
        assert!(
            r["csv"]
                .as_str()
                .unwrap()
                .contains("Field maple,AC,Acer campestre")
        );
        assert!(
            ok(&mut s, "report.plant_schedule", json!({}))
                .get("csv")
                .is_none()
        );
        ok(
            &mut s,
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "existing_tree", "at": [0, 0], "species": "Oak", "dbh": 500, "spread": 9000, "height": 15000, "condition": "good", "action": "remove"}}}),
        );
        let t = ok(&mut s, "report.existing_trees", json!({}));
        assert_eq!(t["rows"][0]["rpz_radius"], 6000.0);
        assert_eq!(t["rows"][0]["action"], "remove");
        assert!(ok(&mut s, "describe", json!({}))["plants"]["place_modes"].is_array());
    }

    #[test]
    fn methods_are_listed() {
        for (m, _) in [
            PLANT_STYLE_LIST,
            PLANT_STYLE_GET,
            PLANT_STYLE_CREATE,
            PLANT_STYLE_UPDATE,
            PLANT_STYLE_DUPLICATE,
            PLANT_STYLE_DELETE,
            PLANT_STYLE_IMPORT_CSV,
            PLANT_STYLE_EXPORT_CSV,
            PLANT_STYLE_SAMPLE_CSV,
            PLANTS_PLACE,
            PLANTS_SET_STYLE,
            REPORT_PLANT_SCHEDULE,
            REPORT_EXISTING_TREES,
        ] {
            assert!(crate::METHODS.iter().any(|(n, _)| *n == m), "{m}");
        }
        let mut s = Session::default();
        assert!(handle(&mut s, "nothing", Value::Null).is_none());
        assert!(handle(&mut s, "plant_style.list", Value::Null).is_some());
    }
}
