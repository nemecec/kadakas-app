//! Agent API for massing models: building blocks with a footprint, a height or floors, and a simple roof.
//!
//! Methods: `massing.add`, `massing.set`, `massing.get`, `massing.from_shapes`, `massing.report`.

use serde::Deserialize;
use serde_json::{Map, Value, json};

use cadektor_core::command::Command;
use cadektor_core::curve::Vertex;
use cadektor_core::massing::{Massing, RoofStyle};
use cadektor_core::massing_batch::{self, FromShapesOptions};
use cadektor_core::model::{Attributes, ClassId, LayerId, ObjectId, Shape};
use cadektor_core::plants::catalog::write_csv;
use cadektor_site::SiteTerrain;

use crate::errors::bad;
use crate::{RpcError, Session};

pub const MASSING_ADD: (&str, &str) = (
    "massing.add",
    "Add a massing model (a building block) in one undo step. params: {footprint: [vertices] (a closed outline; curves are allowed like in polylines), height? (eave height in mm, default 3000) or floors? (number) with floor_height? (mm, default 3000), roof? (flat | gable | hip), pitch? (roof pitch in degrees, default 30), ridge_angle? (radians; default: along the long side), base? (height of the base above the layer in mm; default: on the site model of the layer, else 0), name?, layer?, class?, parent? (a group)}. Returns {id, quantities, warnings}. A gable or hip roof needs a convex footprint: on any other footprint the roof is built flat and warnings says so.",
);
pub const MASSING_SET: (&str, &str) = (
    "massing.set",
    "Change massing models. params: {id | ids, footprint?, height?, floors? (null for none), floor_height?, roof?, pitch?, ridge_angle? (null for the default), base? (null: on the site model), name?}. Returns {changed: [ids], warnings}.",
);
pub const MASSING_GET: (&str, &str) = (
    "massing.get",
    "One massing model with its quantities. params: {id}. Returns {id, name, footprint, height, floors, floor_height, roof, pitch, ridge_angle, base, base_elevation_mm (the base on the site model or at the layer), quantities}.",
);
pub const MASSING_FROM_SHAPES: (&str, &str) = (
    "massing.from_shapes",
    "Create massing from shapes: convert closed polygons (closed polylines, rectangles, circles, landscape areas, hardscapes, for example imported WFS building footprints) into massing models in one undo step. The height comes from a data field of the shape (the properties of an imported feature are kept in its data). params: {ids? (default: the selection), height_field? (default: height, building_height, building:height, ...), floors_field? (default: floors, levels, building:levels, ...), height_scale? (mm per unit of the height field, default 1000: the field is in meters), name_field?, default_height? (mm, default 3000, used when the shape has no height and no floors), floor_height?, roof?, pitch?, require_height? (leave out shapes without a height), delete_source? (default true), dry_run?}. A shape in a group gives a massing model in the same group. The data of the shape moves to the massing model. Returns {created: [{source, id, how}], skipped: [{id, reason}]}.",
);
pub const MASSING_REPORT: (&str, &str) = (
    "massing.report",
    "Quantities of massing models. params: {ids?, format?: csv}. Returns {rows: [{id, name, layer, floors, roof, footprint_m2, gross_floor_area_m2, eave_height_m, ridge_height_m, volume_m3, volume_to_eave_m3, roof_area_m2, wall_area_m2, base_elevation_m}], totals: {count, footprint_m2, gross_floor_area_m2, volume_m3}}. The gross floor area is the footprint times the number of floors; the volume is the volume of the closed solid with its roof.",
);

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "massing": "A massing model (shape kind 'massing') is a simple building: a closed footprint polygon (curves allowed), an eave height (or floors times a floor height), and a flat, gable or hip roof with a pitch. It is drawn in 2D with its roof lines, in 3D as a closed solid, and stands on the site model of its layer (the lowest ground height under the footprint) unless base is set. Quantities: gross floor area (footprint times floors), volume of the solid. IFC export writes an IfcBuildingElementProxy (option massing_as_building: IfcBuilding) with Qto quantities and a Cadektor_Massing property set.",
        "examples": [
            {"method": "massing.add", "params": {"footprint": [[0, 0], [12000, 0], [12000, 8000], [0, 8000]], "floors": 3, "floor_height": 2800, "roof": "gable", "pitch": 35, "name": "House"}},
            {"method": "massing.set", "params": {"id": 21, "roof": "hip", "height": 7500, "floors": null}},
            {"method": "massing.from_shapes", "params": {"ids": [30, 31, 32], "height_field": "height", "roof": "flat", "default_height": 6000}},
            {"method": "massing.report", "params": {"format": "csv"}}
        ]
    })
}

/// Handles a method of this module. `None` means that the method is not one of ours.
pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !method.starts_with("massing.") {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    let Some(m) = p.as_object() else {
        return Some(Err(bad("params must be an object")));
    };
    Some(match method {
        "massing.add" => add(s, m),
        "massing.set" => set(s, m),
        "massing.get" => get(s, m),
        "massing.from_shapes" => from_shapes(s, m),
        "massing.report" => report(s, m),
        other => Err(RpcError::method_not_found(other)),
    })
}

/// The fields that a massing model takes, with their aliases.
const FIELDS: [&str; 8] = [
    "footprint",
    "name",
    "height",
    "floors",
    "floor_height",
    "roof",
    "pitch_deg",
    "base",
];

/// Applies the given fields to a massing model (as JSON), and checks it.
fn patched(base: &Massing, m: &Map<String, Value>) -> Result<Massing, RpcError> {
    let mut v = serde_json::to_value(base).expect("json");
    let o = v.as_object_mut().expect("object");
    for (k, x) in m {
        let key = match k.as_str() {
            "pitch" => "pitch_deg",
            "ridge_angle" => "ridge_angle",
            k if FIELDS.contains(&k) => k,
            // Parameters of the call, not of the model.
            "id" | "ids" | "layer" | "class" | "parent" => continue,
            other => {
                return Err(bad(format!(
                    "unknown field '{other}'. Fields: footprint, name, height, floors, floor_height, roof, pitch, ridge_angle, base"
                )));
            }
        };
        if x.is_null() {
            o.remove(key);
        } else {
            o.insert(key.to_string(), x.clone());
        }
    }
    if let Some(r) = o.get("roof").and_then(Value::as_str) {
        let style = RoofStyle::from_name(r)
            .ok_or_else(|| bad(format!("roof must be flat, gable or hip, not {r}")))?;
        o.insert("roof".into(), json!(style));
    }
    let out: Massing = serde_json::from_value(v).map_err(bad)?;
    if let Some(why) = out.problem() {
        return Err(bad(why));
    }
    Ok(out)
}

fn quantities_json(m: &Massing) -> Value {
    let q = m.quantities();
    json!({
        "floors": q.floors,
        "footprint_m2": q.footprint_area / 1e6,
        "gross_floor_area_m2": q.gross_floor_area / 1e6,
        "eave_height_m": q.eave_height / 1e3,
        "ridge_height_m": q.ridge_height / 1e3,
        "volume_m3": q.volume / 1e9,
        "volume_to_eave_m3": q.volume_to_eave / 1e9,
        "roof_area_m2": q.roof_area / 1e6,
        "wall_area_m2": q.wall_area / 1e6,
    })
}

fn add(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    if !m.contains_key("footprint") {
        return Err(bad("missing 'footprint'"));
    }
    #[derive(Deserialize)]
    struct Place {
        layer: Option<LayerId>,
        class: Option<ClassId>,
        parent: Option<ObjectId>,
    }
    let place: Place = serde_json::from_value(Value::Object(m.clone())).map_err(bad)?;
    let _: Vec<Vertex> = serde_json::from_value(m["footprint"].clone()).map_err(bad)?;
    let base = Massing::new(Vec::new(), 3000.0);
    let model = patched(&base, m)?;
    let quantities = quantities_json(&model);
    let warnings: Vec<String> = model.roof_warning().into_iter().collect();
    let shape = Shape::Massing(Box::new(model));
    let command = match place.parent {
        Some(parent) => Command::AddObjectIn {
            parent,
            shape,
            class: place.class,
            attrs: Attributes::default(),
        },
        None => Command::AddObject {
            shape,
            layer: place.layer,
            class: place.class,
            attrs: Attributes::default(),
        },
    };
    let out = s.editor.execute(&command).map_err(RpcError::from)?;
    Ok(json!({"id": out.created[0], "quantities": quantities, "warnings": warnings}))
}

fn ids_of(m: &Map<String, Value>) -> Result<Vec<ObjectId>, RpcError> {
    if let Some(i) = m.get("id") {
        return Ok(vec![serde_json::from_value(i.clone()).map_err(bad)?]);
    }
    if let Some(l) = m.get("ids") {
        return serde_json::from_value(l.clone()).map_err(bad);
    }
    Err(bad("give id or ids"))
}

fn set(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let ids = ids_of(m)?;
    let mut commands = Vec::new();
    let mut warnings = Vec::new();
    for id in &ids {
        let o = s
            .editor
            .doc()
            .object(*id)
            .ok_or_else(|| bad(format!("object {id} does not exist")))?;
        let Shape::Massing(cur) = &o.shape else {
            return Err(bad(format!("object {id} is not a massing model")));
        };
        let new = patched(cur, m)?;
        if let Some(w) = new.roof_warning() {
            warnings.push(json!({"id": id, "warning": w}));
        }
        commands.push(Command::ModifyObject {
            id: *id,
            shape: Some(Shape::Massing(Box::new(new))),
            layer: None,
            class: None,
            attrs: None,
        });
    }
    s.editor
        .execute(&Command::Batch { commands })
        .map_err(RpcError::from)?;
    Ok(json!({"changed": ids, "warnings": warnings}))
}

/// The height of the base above the origin of the model: the layer elevation plus the base of the
/// model, which comes from the site model unless the model names one.
fn base_elevation(s: &Session, id: ObjectId) -> f64 {
    let doc = s.editor.doc();
    let Some(o) = doc.object(id) else { return 0.0 };
    let Shape::Massing(m) = &o.shape else {
        return 0.0;
    };
    let terrain = SiteTerrain::new(s.site_cache(), s.editor.revision(), doc);
    terrain.massing_base(o, m) + doc.layer(o.layer).map_or(0.0, |l| l.elevation)
}

fn get(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let id = ids_of(m)?[0];
    let o = s
        .editor
        .doc()
        .object(id)
        .ok_or_else(|| bad(format!("object {id} does not exist")))?;
    let Shape::Massing(mm) = &o.shape else {
        return Err(bad(format!("object {id} is not a massing model")));
    };
    let mut v = serde_json::to_value(mm.as_ref()).expect("json");
    v["id"] = json!(id);
    v["pitch"] = json!(mm.pitch_deg);
    v["base_elevation_mm"] = json!(base_elevation(s, id));
    v["quantities"] = quantities_json(mm);
    v["warnings"] = json!(mm.roof_warning().into_iter().collect::<Vec<_>>());
    Ok(v)
}

fn from_shapes(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let mut opts_json = m.clone();
    let ids: Vec<ObjectId> = match opts_json.remove("ids") {
        Some(v) => serde_json::from_value(v).map_err(bad)?,
        None => s.selection.clone(),
    };
    let dry_run = opts_json
        .remove("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if let Some(p) = opts_json.remove("pitch") {
        opts_json.insert("pitch_deg".into(), p);
    }
    if let Some(r) = opts_json.get("roof").and_then(Value::as_str) {
        let style =
            RoofStyle::from_name(r).ok_or_else(|| bad("roof must be flat, gable or hip"))?;
        opts_json.insert("roof".into(), json!(style));
    }
    let opts: FromShapesOptions = serde_json::from_value(Value::Object(opts_json)).map_err(bad)?;
    opts.check().map_err(bad)?;
    if ids.is_empty() {
        return Err(bad("select closed shapes or give ids"));
    }
    let made = massing_batch::from_shapes(s.editor.doc(), &ids, &opts);
    if let (false, Some(cmd)) = (dry_run, &made.command) {
        s.editor.execute(cmd).map_err(RpcError::from)?;
        s.prune_selection();
        // The new models become the selection.
        s.selection = made.created.iter().map(|c| c.1).collect();
    }
    Ok(json!({
        "created": made.created.iter().map(|c| json!({"source": c.0, "id": c.1, "how": c.2})).collect::<Vec<_>>(),
        "skipped": made.skipped.iter().map(|c| json!({"id": c.0, "reason": c.1})).collect::<Vec<_>>(),
        "dry_run": dry_run,
    }))
}

fn report(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let want: Option<Vec<ObjectId>> = match m.get("ids") {
        Some(v) => Some(serde_json::from_value(v.clone()).map_err(bad)?),
        None => None,
    };
    let csv = match m.get("format").and_then(Value::as_str) {
        None => false,
        Some("csv") => true,
        Some(other) => return Err(bad(format!("format must be csv, not {other}"))),
    };
    let doc = s.editor.doc();
    let mut rows = Vec::new();
    let (mut count, mut footprint, mut gfa, mut volume) = (0, 0.0, 0.0, 0.0);
    for o in doc.objects.values() {
        let Shape::Massing(mm) = &o.shape else {
            continue;
        };
        if want.as_ref().is_some_and(|w| !w.contains(&o.id)) {
            continue;
        }
        let q = mm.quantities();
        count += 1;
        footprint += q.footprint_area / 1e6;
        gfa += q.gross_floor_area / 1e6;
        volume += q.volume / 1e9;
        let mut r = quantities_json(mm);
        r["id"] = json!(o.id);
        r["name"] = json!(mm.name);
        r["layer"] = json!(doc.layer(o.layer).map(|l| l.name.clone()));
        r["roof"] = json!(mm.roof.name());
        r["base_elevation_m"] = json!(base_elevation(s, o.id) / 1e3);
        rows.push(r);
    }
    let totals = json!({"count": count, "footprint_m2": footprint, "gross_floor_area_m2": gfa, "volume_m3": volume});
    if csv {
        let cols = [
            "id",
            "name",
            "layer",
            "floors",
            "roof",
            "footprint_m2",
            "gross_floor_area_m2",
            "eave_height_m",
            "ridge_height_m",
            "volume_m3",
            "volume_to_eave_m3",
            "roof_area_m2",
            "wall_area_m2",
            "base_elevation_m",
        ];
        let mut table = vec![cols.iter().map(|c| c.to_string()).collect::<Vec<_>>()];
        for r in &rows {
            table.push(
                cols.iter()
                    .map(|c| match &r[*c] {
                        Value::Null => String::new(),
                        Value::String(t) => t.clone(),
                        Value::Number(n) if n.is_f64() => {
                            format!("{:.3}", n.as_f64().unwrap_or(0.0))
                        }
                        other => other.to_string(),
                    })
                    .collect(),
            );
        }
        return Ok(json!({"csv": write_csv(&table), "totals": totals}));
    }
    Ok(json!({"rows": rows, "totals": totals}))
}
