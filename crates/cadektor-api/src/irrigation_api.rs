//! Agent API for irrigation design: objects, catalog, calculation, layout helpers and reports.
//!
//! Methods: `irrigation.outlet.add`, `irrigation.line.add`, `irrigation.poc.add`,
//! `irrigation.valve.add`, `irrigation.controller.add`, `irrigation.set`,
//! `irrigation.catalog.get|set|import_csv|export_csv`, `irrigation.calculate`,
//! `irrigation.layout.fill|edge|drip`, `irrigation.report.zones|pipes|pressure|materials`.
//!
//! Units: mm for lengths and coordinates, degrees for angles, kPa for pressure, l/min for flow.

use std::collections::HashMap;

use serde_json::{Map, Value, json};

use cadektor_core::DVec2;
use cadektor_core::command::Command;
use cadektor_core::irrigation::{IrrigationShape, LineType};
use cadektor_core::model::*;
use cadektor_core::plants::catalog::write_csv;
use cadektor_core::resource::{ResourceBody, ResourceId};
use cadektor_irrigation::catalog::{self, Catalog};
use cadektor_irrigation::extract::{DEFAULT_TOLERANCE, ExtractOptions, analyze_with};
use cadektor_irrigation::layout::{self, Pattern};
use cadektor_irrigation::report;
use cadektor_irrigation::{Analysis, zone_commands};

use crate::errors::{app, bad};
use crate::{RpcError, Session};

pub const OUTLET_ADD: (&str, &str) = (
    "irrigation.outlet.add",
    "Add an irrigation outlet (spray head, rotor, drip emitter, bubbler). params: {at: [x, y], model (a head of irrigation.catalog.get), arc? (spray angle in degrees, default 360), direction? (degrees, the middle of the arc, counterclockwise from +X, default 0), radius? (mm; default: the radius of the model), layer?, class?}. Returns {id}. The calculation sets its zone.",
);
pub const LINE_ADD: (&str, &str) = (
    "irrigation.line.add",
    "Add an irrigation line. params: {points: [[x, y], ...] (2 or more), line_type? (mainline | lateral | drip, default lateral), size (nominal pipe size, for example \"32\"), material (for example \"PE\"), emitter_spacing? (mm) and emitter_flow? (l/h per emitter) for a drip line, layer?, class?}. Size and material must be a pipe of the catalog. Lines connect where their points (or a valve or POC) lie within the tolerance of each other. Returns {id}.",
);
pub const POC_ADD: (&str, &str) = (
    "irrigation.poc.add",
    "Add a point of connection (the water supply). params: {at, static_pressure (kPa), available_flow (l/min), supply_loss? (kPa lost in the supply line at the available flow), meter_loss? (kPa lost in the meter and backflow preventer at the available flow), name?, layer?, class?}. While water runs the pressure at the POC is the static pressure minus the supply loss (grows with flow^1.852) and the meter loss (grows with flow^2). Returns {id}.",
);
pub const VALVE_ADD: (&str, &str) = (
    "irrigation.valve.add",
    "Add a control valve. params: {at, zone (number, 1 or more), kv? (flow factor in m3/h at 1 bar, default 6), name?, controller? (name of a controller), station?, layer?, class?}. Put it on the end of a mainline and the start of a lateral. Returns {id}.",
);
pub const CONTROLLER_ADD: (&str, &str) = (
    "irrigation.controller.add",
    "Add an irrigation controller. params: {at, name, stations? (default 6), layer?, class?}. Valves name it with controller and station. Returns {id}.",
);
pub const SET: (&str, &str) = (
    "irrigation.set",
    "Change fields of irrigation objects in one undo step. params: {id | ids, ...fields of the object (outlet: model, arc, direction, radius, zone; line: points, line_type, size, material, emitter_spacing, emitter_flow; poc: at, name, static_pressure, available_flow; valve: at, zone, name, kv, controller, station; controller: at, name, stations)}. null clears an optional field. Every object must have the fields you give.",
);
pub const CATALOG_GET: (&str, &str) = (
    "irrigation.catalog.get",
    "The irrigation catalog: the built-in parts with the catalogs of the document laid over them. params: {document_only?: bool}. Returns {heads: [{name, kind (spray|rotor|drip|bubbler), radius (mm), min_arc, max_arc, flow (l/min, full circle), pressure (kPa), min_pressure, price}], pipes: [{size, material, inner_diameter (mm), price (per m)}], materials: [{name, c (Hazen-Williams)}]}.",
);
pub const CATALOG_SET: (&str, &str) = (
    "irrigation.catalog.set",
    "Add or replace parts of the catalog of the document (an undoable resource). params: {heads?, pipes?, materials? (lists as in irrigation.catalog.get), replace? (drop the old document parts first)}. An entry with the name of an existing one (heads, materials) or the same size and material (pipes) replaces it. Returns {id}.",
);
pub const CATALOG_IMPORT_CSV: (&str, &str) = (
    "irrigation.catalog.import_csv",
    "Import catalog parts from CSV text. params: {kind: heads | pipes | materials, text, replace?}. The first row names the columns. Heads: name, radius, flow, pressure (required), kind, min_arc, max_arc, min_pressure, price. Pipes: size, material, inner_diameter (required), price. Materials: name, c. A file with semicolons and decimal commas is read too. Returns {id, imported, warnings} (warnings name rows that appear twice).",
);
pub const CATALOG_EXPORT_CSV: (&str, &str) = (
    "irrigation.catalog.export_csv",
    "Catalog parts as CSV text. params: {kind: heads | pipes | materials, document_only?}. Returns {csv}.",
);
pub const CALCULATE: (&str, &str) = (
    "irrigation.calculate",
    "Calculate the irrigation network: flow per zone, pressure at every outlet, the critical outlet and path, and warnings (low pressure, velocity above 1.5 m/s, flow above the supply, unconnected outlets, loops, catalog gaps). params: {tolerance? (mm, default 50), fittings? (percent added to the pipe losses, default 0), use_site_model? (default true: the height of the site model counts, 9.81 kPa per m), assign_zones? (default true: store the zone in the outlets, one undo step), areas? ({zone: m2}, real areas for the precipitation rate), target_depth? (mm, default 10)}. Returns {zones: [{zone, flow_lpm, available_lpm, static_kpa, max_velocity_ms, critical, critical_path, outlets, warnings}], zone_schedule, warnings: [{kind, object, zone, message}], assigned, ok}.",
);
pub const LAYOUT_FILL: (&str, &str) = (
    "irrigation.layout.fill",
    "Fill an area with heads. params: {boundary: [[x, y], ...] | object (ID of a rect, closed polyline, landscape area or hardscape), model, spacing? (head-to-head, mm; default the radius of the model), pattern? (square | triangular), angle? (degrees), inset? (mm from the edge), arc? (default 360), place? (default true: add the outlets in one undo step)}. Returns {count, points, ids}.",
);
pub const LAYOUT_EDGE: (&str, &str) = (
    "irrigation.layout.edge",
    "Place heads along an edge. params: {path: [[x, y], ...] | object (ID of a polyline or area), closed? (the path is a ring), model, spacing? (default the radius of the model; the heads are spread evenly at most this far apart), arc? (default 180), side? (left | right of the direction of the path: the way the heads face, default left), place?}. Returns {count, points, ids}.",
);
pub const LAYOUT_DRIP: (&str, &str) = (
    "irrigation.layout.drip",
    "Draw parallel drip lines in an area. params: {boundary | object, spacing (mm between lines), angle? (degrees), inset? (mm), size? (default \"16\"), material? (default \"PE\"), emitter_spacing?, emitter_flow?, place?}. Each line runs from the lower to the higher turned coordinate. Returns {count, lines: [[a, b]], ids, total_length_m}.",
);
pub const REPORT_ZONES: (&str, &str) = (
    "irrigation.report.zones",
    "Zone schedule. params: {target_depth? (mm, default 10), areas? ({zone: m2}), tolerance?, format?: csv}. Returns {rows: [{zone, valves, controller, station, heads: [{model, count, flow_lpm}], head_count, flow_lpm, area_m2, area_source, precipitation_mm_h, run_time_min}], total_flow_lpm, total_heads, outlets_without_zone}. Run time = target depth / precipitation rate.",
);
pub const REPORT_PIPES: (&str, &str) = (
    "irrigation.report.pipes",
    "Pipe schedule: lengths per line type, material and size. params: {tolerance?, format?: csv}. Returns {rows: [{line_type, material, size, inner_diameter_mm, count, length_m, price_per_m, cost}], total_length_m, total_cost}.",
);
pub const REPORT_PRESSURE: (&str, &str) = (
    "irrigation.report.pressure",
    "Pressure analysis per zone. params: {tolerance?, format?: csv}. Returns {zones: [{zone, flow_lpm, critical, critical_path, outlets: [{id, label, flow_lpm, pressure_kpa, min_kpa, margin_kpa, ok}], warnings}]}.",
);
pub const REPORT_HYDROZONES: (&str, &str) = (
    "irrigation.report.hydrozones",
    "Hydrozone report: the plants grouped by the hydrozone field of their plant style (water_need: very_low | low | moderate | high, hydrozone, plant_factor, irrigated), linked to the irrigation zones. A hydrozone named Z3, 3 or Zone 3 matches irrigation zone 3. params: {eto? (reference evapotranspiration in mm per year, default 1000), target_depth?, tolerance?, format?: csv}. Returns {rows: [{hydrozone, plants, by_need, canopy_area_m2, plant_factor, annual_water_l, mixed, not_irrigated, zone?: {zone, head_count, flow_lpm, area_m2, precipitation_mm_h, run_time_min}}], warnings, eto}. The plant factors are the middle of the public WUCOLS classes. The irrigation network is optional.",
);
pub const REPORT_MATERIALS: (&str, &str) = (
    "irrigation.report.materials",
    "Materials list: heads, pipes, valves, POCs and controllers with quantities and prices from the catalog. params: {tolerance?, format?: csv}. Returns {rows: [{category, item, quantity, unit, unit_price, total}], total_cost}.",
);

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "irrigation": "Irrigation objects are outlets (heads), lines (mainline, lateral, drip), points of connection (POC), valves (zone, flow factor kv) and controllers. The network comes from the geometry: points within the tolerance (50 mm) are one node. Water flows from a POC through the mainline, a valve, laterals to the outlets. Zones run one at a time; an outlet belongs to the zone of the nearest valve upstream. Pressure losses use Hazen-Williams (C by material), valve losses use kv, velocity above 1.5 m/s warns. Units: mm, degrees, kPa, l/min. The catalog (heads, pipes, materials) is a document resource over a built-in one. objects.list filters irrigation objects by kind: irrigation_outlet, irrigation_line, irrigation_poc, irrigation_valve, irrigation_controller.",
        "examples": [
            {"method": "irrigation.poc.add", "params": {"at": [0, 0], "static_pressure": 400, "available_flow": 100}},
            {"method": "irrigation.line.add", "params": {"points": [[0, 0], [20000, 0]], "line_type": "mainline", "size": "50", "material": "PVC"}},
            {"method": "irrigation.valve.add", "params": {"at": [20000, 0], "zone": 1, "kv": 6}},
            {"method": "irrigation.line.add", "params": {"points": [[20000, 0], [50000, 0]], "line_type": "lateral", "size": "32", "material": "PE"}},
            {"method": "irrigation.outlet.add", "params": {"at": [50000, 0], "model": "Spray-4.0", "arc": 180, "direction": 90}},
            {"method": "irrigation.layout.fill", "params": {"boundary": [[0, 0], [12000, 0], [12000, 8000], [0, 8000]], "model": "Spray-4.0", "pattern": "triangular"}},
            {"method": "irrigation.layout.edge", "params": {"path": [[0, 0], [12000, 0]], "model": "Spray-2.5", "spacing": 2500}},
            {"method": "irrigation.layout.drip", "params": {"boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]], "spacing": 500}},
            {"method": "irrigation.catalog.set", "params": {"heads": [{"name": "My rotor", "kind": "rotor", "radius": 9000, "flow": 14, "pressure": 300}]}},
            {"method": "irrigation.calculate", "params": {"target_depth": 8}},
            {"method": "irrigation.report.zones", "params": {"format": "csv"}},
            {"method": "irrigation.report.pressure", "params": {}}
        ],
    })
}

fn required<'a>(m: &'a Map<String, Value>, key: &str) -> Result<&'a Value, RpcError> {
    match m.get(key) {
        Some(v) if !v.is_null() => Ok(v),
        _ => Err(bad(format!("missing '{key}'"))),
    }
}

fn num(m: &Map<String, Value>, key: &str) -> Result<Option<f64>, RpcError> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite())
            .map(Some)
            .ok_or_else(|| bad(format!("'{key}' must be a number"))),
    }
}

fn text(m: &Map<String, Value>, key: &str) -> Result<Option<String>, RpcError> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        // A pipe size such as 32 is a number in JSON.
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(_) => Err(bad(format!("'{key}' must be text"))),
    }
}

fn point(v: &Value, what: &str) -> Result<DVec2, RpcError> {
    serde_json::from_value::<DVec2>(v.clone())
        .ok()
        .filter(|p| p.is_finite())
        .ok_or_else(|| bad(format!("'{what}' must be [x, y]")))
}

fn points(v: &Value, what: &str) -> Result<Vec<DVec2>, RpcError> {
    v.as_array()
        .ok_or_else(|| bad(format!("'{what}' must be a list of [x, y]")))?
        .iter()
        .map(|p| {
            // A vertex object {p: [x, y], ...} is accepted too.
            let p = p.get("p").unwrap_or(p);
            point(p, what)
        })
        .collect()
}

fn opt_id(m: &Map<String, Value>, key: &str) -> Result<Option<u64>, RpcError> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| bad(format!("'{key}' must be an ID"))),
    }
}

fn tolerance(m: &Map<String, Value>) -> Result<f64, RpcError> {
    match num(m, "tolerance")? {
        None => Ok(DEFAULT_TOLERANCE),
        Some(t) if t >= 0.0 => Ok(t),
        Some(_) => Err(bad("'tolerance' must not be negative")),
    }
}

/// Reads the network of the document with the options of the parameters: `tolerance` (mm),
/// `fittings` (percent of the pipe losses) and `use_site_model` (default true: water climbs
/// with the ground of the site model, when there is one).
fn analysis(s: &Session, m: &Map<String, Value>) -> Result<Analysis, RpcError> {
    let fittings_pct = match num(m, "fittings")? {
        None => 0.0,
        Some(f) if (0.0..=100.0).contains(&f) => f,
        Some(_) => return Err(bad("'fittings' is a percentage from 0 to 100")),
    };
    let doc = s.editor.doc();
    let has_site = doc
        .objects
        .values()
        .any(|o| matches!(o.shape, Shape::SiteModel { .. }));
    let use_site = has_site && m.get("use_site_model").and_then(Value::as_bool) != Some(false);
    let terrain = cadektor_site::SiteTerrain::new(s.site_cache(), s.editor.revision(), doc);
    Ok(analyze_with(
        doc,
        &ExtractOptions {
            tolerance: tolerance(m)?,
            fittings_pct,
            ground: use_site.then_some(&terrain as &dyn cadektor_core::ground::GroundSource),
        },
    ))
}

fn areas(m: &Map<String, Value>) -> Result<HashMap<u32, f64>, RpcError> {
    let mut out = HashMap::new();
    match m.get("areas") {
        None | Some(Value::Null) => {}
        Some(Value::Object(o)) => {
            for (k, v) in o {
                let zone: u32 = k
                    .parse()
                    .map_err(|_| bad("'areas' keys are zone numbers"))?;
                let a = v
                    .as_f64()
                    .filter(|a| a.is_finite() && *a > 0.0)
                    .ok_or_else(|| bad("'areas' values are areas in m2"))?;
                out.insert(zone, a);
            }
        }
        Some(_) => return Err(bad("'areas' must be {zone: area in m2}")),
    }
    Ok(out)
}

fn csv_requested(m: &Map<String, Value>) -> bool {
    m.get("format").and_then(Value::as_str) == Some("csv")
}

/// Adds one object with the layer and class of the parameters.
fn add_cmd(shape: IrrigationShape, m: &Map<String, Value>) -> Result<Command, RpcError> {
    Ok(Command::AddObject {
        shape: Shape::Irrigation(Box::new(shape)),
        layer: opt_id(m, "layer")?.map(LayerId),
        class: opt_id(m, "class")?.map(ClassId),
        attrs: Attributes::default(),
    })
}

fn add_one(
    s: &mut Session,
    shape: IrrigationShape,
    m: &Map<String, Value>,
) -> Result<Value, RpcError> {
    let cmd = s.in_context(add_cmd(shape, m)?);
    let out = s.editor.execute(&cmd).map_err(RpcError::from)?;
    Ok(json!({ "id": out.created[0] }))
}

/// Adds many objects in one undo step.
fn add_many(
    s: &mut Session,
    shapes: Vec<IrrigationShape>,
    m: &Map<String, Value>,
) -> Result<Vec<u64>, RpcError> {
    if shapes.is_empty() {
        return Ok(vec![]);
    }
    let cmds = shapes
        .into_iter()
        .map(|sh| add_cmd(sh, m).map(|c| s.in_context(c)))
        .collect::<Result<Vec<_>, _>>()?;
    let out = s
        .editor
        .execute(&Command::Batch { commands: cmds })
        .map_err(RpcError::from)?;
    Ok(out.created)
}

fn doc_catalog_resource(doc: &Document) -> Option<ResourceId> {
    doc.resources
        .iter()
        .find(|r| matches!(r.body, ResourceBody::IrrigationCatalog(_)))
        .map(|r| r.id)
}

/// The catalog parts that the document itself holds.
fn document_catalog(doc: &Document) -> Catalog {
    doc_catalog_resource(doc)
        .and_then(|id| doc.resources.get(id))
        .and_then(|r| match &r.body {
            ResourceBody::IrrigationCatalog(v) => Catalog::from_value(v).ok(),
            _ => None,
        })
        .unwrap_or_default()
}

/// Stores `cat` as the catalog resource of the document (adds it or changes it).
fn store_catalog(s: &mut Session, cat: &Catalog) -> Result<Value, RpcError> {
    cat.validate().map_err(bad)?;
    let body = ResourceBody::IrrigationCatalog(cat.to_value());
    let cmd = match doc_catalog_resource(s.editor.doc()) {
        Some(id) => Command::ModifyResource {
            id,
            name: None,
            body: Some(body),
        },
        None => Command::AddResource {
            name: "Irrigation catalog".into(),
            body,
        },
    };
    let out = s.editor.execute(&cmd).map_err(RpcError::from)?;
    let id = out
        .created
        .first()
        .copied()
        .or_else(|| doc_catalog_resource(s.editor.doc()).map(|r| r.0))
        .unwrap_or(0);
    Ok(json!({ "id": id }))
}

fn catalog_json(c: &Catalog) -> Value {
    json!({ "heads": c.heads, "pipes": c.pipes, "materials": c.materials })
}

fn parse_list<T: serde::de::DeserializeOwned>(
    m: &Map<String, Value>,
    key: &str,
) -> Result<Vec<T>, RpcError> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(vec![]),
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| bad(format!("'{key}': {e}"))),
    }
}

fn catalog_set(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let replace = m.get("replace").and_then(Value::as_bool).unwrap_or(false);
    let extra = Catalog {
        heads: parse_list(m, "heads")?,
        pipes: parse_list(m, "pipes")?,
        materials: parse_list(m, "materials")?,
    };
    let mut cat = if replace {
        Catalog::default()
    } else {
        document_catalog(s.editor.doc())
    };
    // Check the new parts alone first, so the error names them and not a mix.
    extra.validate().map_err(bad)?;
    cat.merge(&extra);
    store_catalog(s, &cat)
}

fn catalog_import(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let kind = text(m, "kind")?.ok_or_else(|| bad("missing 'kind'"))?;
    let csv = text(m, "text")?.ok_or_else(|| bad("missing 'text'"))?;
    let replace = m.get("replace").and_then(Value::as_bool).unwrap_or(false);
    let mut extra = Catalog::default();
    let mut duplicates: Vec<String> = vec![];
    let imported = match kind.as_str() {
        "heads" => {
            extra.heads = catalog::import_heads_csv(&csv).map_err(bad)?;
            duplicates = catalog::duplicate_heads(&extra.heads);
            extra.heads.len()
        }
        "pipes" => {
            extra.pipes = catalog::import_pipes_csv(&csv).map_err(bad)?;
            duplicates = catalog::duplicate_pipes(&extra.pipes);
            extra.pipes.len()
        }
        "materials" => {
            extra.materials = catalog::import_materials_csv(&csv).map_err(bad)?;
            duplicates = catalog::duplicate_materials(&extra.materials);
            extra.materials.len()
        }
        other => {
            return Err(bad(format!(
                "unknown kind '{other}'. Use heads, pipes or materials"
            )));
        }
    };
    let mut cat = if replace {
        Catalog::default()
    } else {
        document_catalog(s.editor.doc())
    };
    cat.merge(&extra);
    let mut out = store_catalog(s, &cat)?;
    out["imported"] = imported.into();
    out["warnings"] = duplicates
        .iter()
        .map(|d| format!("'{d}' appears more than once in the file; the last row is used"))
        .collect::<Vec<_>>()
        .into();
    Ok(out)
}

/// The corner points of a polygon object.
fn polygon_of(doc: &Document, id: u64) -> Result<(Vec<DVec2>, bool), RpcError> {
    let o = doc
        .object(ObjectId(id))
        .ok_or_else(|| bad(format!("object {id} does not exist")))?;
    match &o.shape {
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            Ok((
                vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)],
                true,
            ))
        }
        Shape::Polyline { points, closed } => {
            Ok((cadektor_core::curve::positions(points), *closed))
        }
        Shape::LandscapeArea { boundary, .. } | Shape::Hardscape { boundary, .. } => {
            Ok((cadektor_core::curve::positions(boundary), true))
        }
        _ => Err(bad(format!(
            "object {id} has no outline (use a rect, polyline, landscape area or hardscape)"
        ))),
    }
}

/// The area or path of the parameters: `key` (a point list) or `object`.
fn outline(
    doc: &Document,
    m: &Map<String, Value>,
    key: &str,
) -> Result<(Vec<DVec2>, bool), RpcError> {
    match (m.get(key).filter(|v| !v.is_null()), opt_id(m, "object")?) {
        (Some(v), _) => Ok((points(v, key)?, false)),
        (None, Some(id)) => polygon_of(doc, id),
        (None, None) => Err(bad(format!("give '{key}' or 'object'"))),
    }
}

fn head_for(
    m: &Map<String, Value>,
    doc: &Document,
) -> Result<cadektor_irrigation::HeadModel, RpcError> {
    let model = text(m, "model")?.ok_or_else(|| bad("missing 'model'"))?;
    Catalog::of_document(doc)
        .head(&model)
        .cloned()
        .ok_or_else(|| {
            bad(format!(
                "unknown head model '{model}'. See irrigation.catalog.get"
            ))
        })
}

fn layout_outlets(
    s: &mut Session,
    m: &Map<String, Value>,
    positions: &[(DVec2, f64)],
    model: &str,
    arc: f64,
) -> Result<Vec<u64>, RpcError> {
    let shapes = positions
        .iter()
        .map(|(at, direction)| IrrigationShape::Outlet {
            at: *at,
            model: model.to_string(),
            arc,
            direction: *direction,
            radius: None,
            zone: None,
        })
        .collect();
    add_many(s, shapes, m)
}

fn pts_json(p: &[DVec2]) -> Vec<Value> {
    p.iter().map(|p| json!([p.x, p.y])).collect()
}

fn layout(s: &mut Session, method: &str, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let place = m.get("place").and_then(Value::as_bool).unwrap_or(true);
    let doc = s.editor.doc();
    match method {
        "irrigation.layout.fill" => {
            let (poly, _) = outline(doc, m, "boundary")?;
            let head = head_for(m, doc)?;
            let spacing = num(m, "spacing")?.unwrap_or(head.radius);
            let pattern = match text(m, "pattern")? {
                None => Pattern::Square,
                Some(p) => Pattern::from_name(&p)
                    .ok_or_else(|| bad("'pattern' is square or triangular"))?,
            };
            let pts = layout::fill_area(
                &poly,
                spacing,
                pattern,
                num(m, "angle")?.unwrap_or(0.0),
                num(m, "inset")?.unwrap_or(0.0),
            )
            .map_err(bad)?;
            let arc = num(m, "arc")?.unwrap_or(360.0);
            let ids = if place {
                let at: Vec<(DVec2, f64)> = pts.iter().map(|p| (*p, 0.0)).collect();
                layout_outlets(s, m, &at, &head.name, arc)?
            } else {
                vec![]
            };
            Ok(json!({"count": pts.len(), "points": pts_json(&pts), "ids": ids}))
        }
        "irrigation.layout.edge" => {
            let (path, ring) = outline(doc, m, "path")?;
            let closed = m.get("closed").and_then(Value::as_bool).unwrap_or(ring);
            let head = head_for(m, doc)?;
            let spacing = num(m, "spacing")?.unwrap_or(head.radius);
            let turn = match text(m, "side")?.as_deref() {
                None | Some("left") => 90.0,
                Some("right") => -90.0,
                Some(_) => return Err(bad("'side' is left or right")),
            };
            let heads = layout::along_edge(&path, spacing, closed).map_err(bad)?;
            let arc = num(m, "arc")?.unwrap_or(180.0);
            let at: Vec<(DVec2, f64)> = heads
                .iter()
                .map(|h| (h.at, h.tangent.to_degrees() + turn))
                .collect();
            let pts: Vec<DVec2> = at.iter().map(|a| a.0).collect();
            let ids = if place {
                layout_outlets(s, m, &at, &head.name, arc)?
            } else {
                vec![]
            };
            Ok(json!({"count": pts.len(), "points": pts_json(&pts), "ids": ids}))
        }
        _ => {
            let (poly, _) = outline(doc, m, "boundary")?;
            let spacing = num(m, "spacing")?.ok_or_else(|| bad("missing 'spacing'"))?;
            let lines = layout::drip_lines(
                &poly,
                spacing,
                num(m, "angle")?.unwrap_or(0.0),
                num(m, "inset")?.unwrap_or(0.0),
            )
            .map_err(bad)?;
            let (size, material) = (
                text(m, "size")?.unwrap_or_else(|| "16".into()),
                text(m, "material")?.unwrap_or_else(|| "PE".into()),
            );
            if Catalog::of_document(doc).pipe(&size, &material).is_none() {
                return Err(bad(format!("pipe {material} {size} is not in the catalog")));
            }
            let shapes: Vec<IrrigationShape> = lines
                .iter()
                .map(|(a, b)| IrrigationShape::Line {
                    points: vec![*a, *b],
                    line_type: LineType::Drip,
                    size: size.clone(),
                    material: material.clone(),
                    emitter_spacing: num(m, "emitter_spacing").ok().flatten(),
                    emitter_flow: num(m, "emitter_flow").ok().flatten(),
                })
                .collect();
            // Report bad emitter numbers instead of dropping them.
            num(m, "emitter_spacing")?;
            num(m, "emitter_flow")?;
            let total: f64 = lines.iter().map(|(a, b)| a.distance(*b)).sum::<f64>() / 1000.0;
            let ids = if place {
                add_many(s, shapes, m)?
            } else {
                vec![]
            };
            let pairs: Vec<Value> = lines
                .iter()
                .map(|(a, b)| json!([[a.x, a.y], [b.x, b.y]]))
                .collect();
            Ok(json!({"count": lines.len(), "lines": pairs, "ids": ids, "total_length_m": total}))
        }
    }
}

const OUTLET_KEYS: &[&str] = &["at", "model", "arc", "direction", "radius", "zone"];
const LINE_KEYS: &[&str] = &[
    "points",
    "line_type",
    "size",
    "material",
    "emitter_spacing",
    "emitter_flow",
];
const POC_KEYS: &[&str] = &[
    "at",
    "name",
    "static_pressure",
    "available_flow",
    "supply_loss",
    "meter_loss",
];
const VALVE_KEYS: &[&str] = &["at", "zone", "name", "kv", "controller", "station"];
const CONTROLLER_KEYS: &[&str] = &["at", "name", "stations"];

fn keys_of(i: &IrrigationShape) -> &'static [&'static str] {
    match i {
        IrrigationShape::Outlet { .. } => OUTLET_KEYS,
        IrrigationShape::Line { .. } => LINE_KEYS,
        IrrigationShape::Poc { .. } => POC_KEYS,
        IrrigationShape::Valve { .. } => VALVE_KEYS,
        IrrigationShape::Controller { .. } => CONTROLLER_KEYS,
    }
}

fn ids_param(m: &Map<String, Value>) -> Result<Vec<ObjectId>, RpcError> {
    match (m.get("ids"), m.get("id")) {
        (Some(v), _) => serde_json::from_value(v.clone()).map_err(bad),
        (None, Some(v)) => Ok(vec![serde_json::from_value(v.clone()).map_err(bad)?]),
        _ => Err(bad("give 'id' or 'ids'")),
    }
}

fn set(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let ids = ids_param(m)?;
    let doc = s.editor.doc();
    let mut cmds = vec![];
    for id in &ids {
        let o = doc
            .object(*id)
            .ok_or_else(|| bad(format!("object {id} does not exist")))?;
        let Shape::Irrigation(i) = &o.shape else {
            return Err(bad(format!("object {id} is not an irrigation object")));
        };
        let mut v = serde_json::to_value(i.as_ref()).map_err(app)?;
        let obj = v.as_object_mut().expect("an irrigation shape is an object");
        let mut touched = 0;
        for (k, x) in m {
            if matches!(k.as_str(), "id" | "ids") {
                continue;
            }
            if !keys_of(i).contains(&k.as_str()) {
                return Err(bad(format!(
                    "'{k}' is not a field of {}. Fields: {}",
                    i.kind(),
                    keys_of(i).join(", ")
                )));
            }
            if x.is_null() {
                obj.remove(k);
            } else {
                obj.insert(k.clone(), x.clone());
            }
            touched += 1;
        }
        if touched == 0 {
            return Err(bad("give at least one field to change"));
        }
        let new: IrrigationShape = serde_json::from_value(v).map_err(bad)?;
        if let Some(p) = new.problem() {
            return Err(bad(p));
        }
        cmds.push(Command::ModifyObject {
            id: *id,
            shape: Some(Shape::Irrigation(Box::new(new))),
            layer: None,
            class: None,
            attrs: None,
        });
    }
    s.editor
        .execute(&Command::Batch { commands: cmds })
        .map_err(RpcError::from)?;
    Ok(json!({ "ids": ids }))
}

fn zones_json(a: &Analysis) -> Value {
    json!(report::pressure_analysis(a))
}

fn warnings_json(a: &Analysis) -> Vec<Value> {
    a.warnings()
        .into_iter()
        .map(|w| json!(report::WarningRow::from(w)))
        .collect()
}

fn calculate(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let depth = num(m, "target_depth")?.unwrap_or(10.0);
    let given = areas(m)?;
    let assign = m
        .get("assign_zones")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let a = analysis(s, m)?;
    let mut assigned = 0;
    if assign {
        let cmds = zone_commands(s.editor.doc(), &a);
        assigned = cmds.len();
        if !cmds.is_empty() {
            s.editor
                .execute(&Command::Batch { commands: cmds })
                .map_err(RpcError::from)?;
        }
    }
    let warnings = warnings_json(&a);
    Ok(json!({
        "zones": zones_json(&a),
        "zone_schedule": report::zone_schedule(&a, &given, depth),
        "warnings": warnings,
        "assigned": assigned,
        "ok": warnings.is_empty(),
    }))
}

/// Plants by hydrozone, linked to the zone schedule of the irrigation network (if there is one).
fn hydrozone_report(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    use cadektor_core::plants::care;
    let eto = num(m, "eto")?.unwrap_or(1000.0);
    if eto <= 0.0 {
        return Err(bad("'eto' must be a positive number of mm per year"));
    }
    let rows = care::hydrozones(s.editor.doc(), eto);
    // The network is optional: a document without irrigation objects has no zones.
    let schedule = analysis(s, m)
        .ok()
        .map(|a| {
            report::zone_schedule(
                &a,
                &areas(m).unwrap_or_default(),
                num(m, "target_depth").ok().flatten().unwrap_or(10.0),
            )
        })
        .filter(|sch| !sch.rows.is_empty());
    let mut warnings: Vec<String> = Vec::new();
    let mut used_zones: Vec<u32> = Vec::new();
    let mut out: Vec<Value> = Vec::new();
    for r in &rows {
        let mut v = json!(r);
        if let (Some(sch), Some(n)) = (&schedule, care::zone_number(&r.hydrozone)) {
            match sch.rows.iter().find(|z| z.zone == n) {
                Some(z) => {
                    used_zones.push(n);
                    v["zone"] = json!({
                        "zone": z.zone,
                        "head_count": z.head_count,
                        "flow_lpm": z.flow_lpm,
                        "area_m2": z.area_m2,
                        "precipitation_mm_h": z.precipitation_mm_h,
                        "run_time_min": z.run_time_min,
                    });
                }
                None => warnings.push(format!(
                    "hydrozone '{}' has no irrigation zone {n} in the network",
                    r.hydrozone
                )),
            }
        } else if r.hydrozone.is_empty() && r.plants > r.not_irrigated {
            warnings.push(format!(
                "{} irrigated plant(s) have no hydrozone",
                r.plants - r.not_irrigated
            ));
        }
        if r.mixed {
            warnings.push(format!(
                "hydrozone '{}' mixes plants whose water needs are more than one class apart",
                r.hydrozone
            ));
        }
        out.push(v);
    }
    if let Some(sch) = &schedule {
        for z in &sch.rows {
            if !used_zones.contains(&z.zone) && !rows.is_empty() {
                warnings.push(format!(
                    "irrigation zone {} has no hydrozone of plants",
                    z.zone
                ));
            }
        }
    }
    if csv_requested(m) {
        let mut table = vec![
            [
                "Hydrozone",
                "Plants",
                "Canopy area m2",
                "Plant factor",
                "Annual water l",
                "Mixed",
                "Zone flow l/min",
                "Zone run time min",
            ]
            .map(String::from)
            .to_vec(),
        ];
        for (r, v) in rows.iter().zip(&out) {
            table.push(vec![
                r.hydrozone.clone(),
                r.plants.to_string(),
                format!("{:.2}", r.canopy_area_m2),
                format!("{:.2}", r.plant_factor),
                format!("{:.0}", r.annual_water_l),
                if r.mixed { "yes" } else { "no" }.into(),
                v["zone"]["flow_lpm"]
                    .as_f64()
                    .map_or(String::new(), |f| format!("{f:.1}")),
                v["zone"]["run_time_min"]
                    .as_f64()
                    .map_or(String::new(), |f| format!("{f:.0}")),
            ]);
        }
        return Ok(json!({"csv": write_csv(&table)}));
    }
    Ok(json!({"rows": out, "warnings": warnings, "eto": eto}))
}

fn reports(s: &mut Session, method: &str, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let a = analysis(s, m)?;
    let csv = csv_requested(m);
    Ok(match method {
        "irrigation.report.zones" => {
            let r = report::zone_schedule(&a, &areas(m)?, num(m, "target_depth")?.unwrap_or(10.0));
            if csv {
                json!({"csv": r.to_csv()})
            } else {
                json!(r)
            }
        }
        "irrigation.report.pipes" => {
            let r = report::pipe_schedule(&a);
            if csv {
                json!({"csv": r.to_csv()})
            } else {
                json!(r)
            }
        }
        "irrigation.report.materials" => {
            let r = report::materials_list(&a);
            if csv {
                json!({"csv": r.to_csv()})
            } else {
                json!(r)
            }
        }
        _ => {
            let zones = report::pressure_analysis(&a);
            if csv {
                let mut rows = vec![
                    [
                        "Zone",
                        "Outlet",
                        "Flow l/min",
                        "Pressure kPa",
                        "Minimum kPa",
                        "Margin kPa",
                        "OK",
                    ]
                    .map(String::from)
                    .to_vec(),
                ];
                for z in &zones {
                    for o in &z.outlets {
                        rows.push(vec![
                            z.zone.to_string(),
                            o.label.clone(),
                            format!("{:.2}", o.flow_lpm),
                            format!("{:.1}", o.pressure_kpa),
                            format!("{:.1}", o.min_kpa),
                            format!("{:.1}", o.margin_kpa),
                            if o.ok { "yes" } else { "no" }.into(),
                        ]);
                    }
                }
                json!({"csv": write_csv(&rows)})
            } else {
                json!({ "zones": zones })
            }
        }
    })
}

fn add_methods(s: &mut Session, method: &str, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let at = || point(required(m, "at")?, "at");
    let shape = match method {
        "irrigation.outlet.add" => {
            let head = head_for(m, s.editor.doc())?;
            IrrigationShape::Outlet {
                at: at()?,
                model: head.name.clone(),
                arc: num(m, "arc")?.unwrap_or(360.0),
                direction: num(m, "direction")?.unwrap_or(0.0),
                radius: Some(num(m, "radius")?.unwrap_or(head.radius)),
                zone: None,
            }
        }
        "irrigation.line.add" => {
            let pts = points(required(m, "points")?, "points")?;
            let size = text(m, "size")?.ok_or_else(|| bad("missing 'size'"))?;
            let material = text(m, "material")?.ok_or_else(|| bad("missing 'material'"))?;
            let line_type = match text(m, "line_type")? {
                None => LineType::Lateral,
                Some(t) => LineType::from_name(&t)
                    .ok_or_else(|| bad("'line_type' is mainline, lateral or drip"))?,
            };
            if Catalog::of_document(s.editor.doc())
                .pipe(&size, &material)
                .is_none()
            {
                return Err(bad(format!(
                    "pipe {material} {size} is not in the catalog. See irrigation.catalog.get"
                )));
            }
            IrrigationShape::Line {
                points: pts,
                line_type,
                size,
                material,
                emitter_spacing: num(m, "emitter_spacing")?,
                emitter_flow: num(m, "emitter_flow")?,
            }
        }
        "irrigation.poc.add" => IrrigationShape::Poc {
            at: at()?,
            name: text(m, "name")?.unwrap_or_default(),
            static_pressure: num(m, "static_pressure")?
                .ok_or_else(|| bad("missing 'static_pressure'"))?,
            available_flow: num(m, "available_flow")?
                .ok_or_else(|| bad("missing 'available_flow'"))?,
            supply_loss: num(m, "supply_loss")?,
            meter_loss: num(m, "meter_loss")?,
        },
        "irrigation.valve.add" => IrrigationShape::Valve {
            at: at()?,
            zone: num(m, "zone")?
                .filter(|z| *z >= 1.0 && z.fract() == 0.0 && *z < 1e6)
                .ok_or_else(|| bad("'zone' must be a whole number from 1"))?
                as u32,
            name: text(m, "name")?.unwrap_or_default(),
            kv: num(m, "kv")?.unwrap_or(6.0),
            controller: text(m, "controller")?,
            station: num(m, "station")?.map(|x| x as u32),
        },
        _ => IrrigationShape::Controller {
            at: at()?,
            name: text(m, "name")?.ok_or_else(|| bad("missing 'name'"))?,
            stations: num(m, "stations")?.unwrap_or(6.0) as u32,
        },
    };
    if let Some(p) = shape.problem() {
        return Err(bad(p));
    }
    add_one(s, shape, m)
}

pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !method.starts_with("irrigation.") {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    let Some(m) = p.as_object() else {
        return Some(Err(bad("params must be an object")));
    };
    Some(match method {
        "irrigation.outlet.add"
        | "irrigation.line.add"
        | "irrigation.poc.add"
        | "irrigation.valve.add"
        | "irrigation.controller.add" => add_methods(s, method, m),
        "irrigation.set" => set(s, m),
        "irrigation.catalog.get" => {
            let c = if m.get("document_only").and_then(Value::as_bool) == Some(true) {
                document_catalog(s.editor.doc())
            } else {
                Catalog::of_document(s.editor.doc())
            };
            Ok(catalog_json(&c))
        }
        "irrigation.catalog.set" => catalog_set(s, m),
        "irrigation.catalog.import_csv" => catalog_import(s, m),
        "irrigation.catalog.export_csv" => {
            let c = if m.get("document_only").and_then(Value::as_bool) == Some(true) {
                document_catalog(s.editor.doc())
            } else {
                Catalog::of_document(s.editor.doc())
            };
            match text(m, "kind") {
                Ok(Some(k)) if k == "heads" => Ok(json!({"csv": c.heads_csv()})),
                Ok(Some(k)) if k == "pipes" => Ok(json!({"csv": c.pipes_csv()})),
                Ok(Some(k)) if k == "materials" => Ok(json!({"csv": c.materials_csv()})),
                Ok(_) => Err(bad("'kind' is heads, pipes or materials")),
                Err(e) => Err(e),
            }
        }
        "irrigation.calculate" => calculate(s, m),
        "irrigation.layout.fill" | "irrigation.layout.edge" | "irrigation.layout.drip" => {
            layout(s, method, m)
        }
        "irrigation.report.hydrozones" => hydrozone_report(s, m),
        "irrigation.report.zones"
        | "irrigation.report.pipes"
        | "irrigation.report.pressure"
        | "irrigation.report.materials" => reports(s, method, m),
        _ => Err(RpcError::method_not_found(method)),
    })
}

#[cfg(test)]
mod tests;
