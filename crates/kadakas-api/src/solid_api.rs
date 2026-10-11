//! Agent API for extrudes, sweeps and 3D polygons (3D-03).
//!
//! Methods: `extrude.add|set|get|from_shapes`, `sweep.add|set|get|from_shapes`,
//! `polygon3d.add|set|get`. The batch methods (`from_shapes`) are Model > Extrude and Model > Sweep.

use serde::Deserialize;
use serde_json::{Map, Value, json};

use kadakas_core::DVec3;
use kadakas_core::command::Command;
use kadakas_core::model::{Attributes, ClassId, LayerId, ObjectId, Shape};
use kadakas_core::solid3d::{self, Extrude, Sweep};
use kadakas_core::solid_batch::{self, ExtrudeOptions, Made, SweepOptions};

use crate::errors::bad;
use crate::{RpcError, Session};

pub const EXTRUDE_ADD: (&str, &str) = (
    "extrude.add",
    "Add an extrude (shape kind 'extrude') in one undo step: a closed 2D profile pushed up by a height. The profile stays editable. params: {profile: [vertices] (a closed outline, curves allowed like in polylines), holes?: [[vertices]] (closed outlines inside the profile), height (mm, not zero; negative extrudes downward), base? (height of the profile above the layer in mm, default 0), name?, layer?, class?, parent? (a group)}. A profile that crosses itself, has no area, or has a hole that touches the outline is refused with the reason. Returns {id, quantities}.",
);
pub const EXTRUDE_SET: (&str, &str) = (
    "extrude.set",
    "Change extrudes. params: {id | ids, profile?, holes?, height?, base?, name?}. Returns {changed: [ids]}.",
);
pub const EXTRUDE_GET: (&str, &str) = (
    "extrude.get",
    "One extrude with its quantities. params: {id}. Returns {id, name, profile, holes, height, base, quantities: {profile_area_m2, volume_m3, surface_area_m2, triangles}, bounds_mm: {min, max}}.",
);
pub const EXTRUDE_FROM_SHAPES: (&str, &str) = (
    "extrude.from_shapes",
    "Model > Extrude: turn closed 2D shapes (closed polylines, rectangles, circles, landscape areas with their holes, hardscapes, closed 3D polygons) into extrudes in one undo step. params: {ids? (default: the selection), height (mm, default 1000), base? (mm, default 0), delete_source? (default true: the 2D shape becomes the solid; false keeps it), dry_run?}. The class, attributes and group of the shape and its free data go to the solid. The new solids become the selection. Returns {created: [{source, id}], skipped: [{id, reason}], dry_run}.",
);
pub const SWEEP_ADD: (&str, &str) = (
    "sweep.add",
    "Add a sweep (shape kind 'sweep') in one undo step: a profile turned around an axis. params: {profile: [vertices] (x is the distance from the axis, y the position along it; a closed profile makes a solid with end caps, an open one a surface), closed? (default true), origin? ([x, y, z] of a point of the axis, mm; z is measured from the layer), axis? ([x, y, z] direction, default [0, 0, 1]), rotation? (degrees, the angle of the profile plane around the axis at the start), sweep_angle? (degrees, default 360, at most 3600), segment_angle? (degrees, default 10, from 0.1 to 180: the smaller, the rounder), pitch? (rise along the axis for each full turn in mm, default 0; a spiral), name?, layer?, class?, parent?}. At most 720 segments: a smaller segment angle is lowered to that and the result says so in warnings. A profile that crosses the axis or itself is refused. Returns {id, quantities, warnings}.",
);
pub const SWEEP_SET: (&str, &str) = (
    "sweep.set",
    "Change sweeps. params: {id | ids, profile?, closed?, origin?, axis?, rotation?, sweep_angle?, segment_angle?, pitch?, name?}. Returns {changed: [ids], warnings}.",
);
pub const SWEEP_GET: (&str, &str) = (
    "sweep.get",
    "One sweep with its quantities. params: {id}. Returns {id, name, profile, closed, origin, axis, rotation, sweep_angle, segment_angle, pitch, segments, quantities: {volume_m3 (closed profiles), surface_area_m2, triangles}, bounds_mm, warnings}.",
);
pub const SWEEP_FROM_SHAPES: (&str, &str) = (
    "sweep.from_shapes",
    "Model > Sweep: turn 2D shapes into sweeps in one undo step. Closed shapes make solids, open polylines and lines make surfaces. params: {ids? (default: the selection), axis_id? (a line object, or a polyline with two vertices: the profile turns around it, in the plane of the plan; the axis is horizontal and the line stays), axis_at? ([x, y]: a vertical axis at this point instead), sweep_angle? (default 360), segment_angle? (default 10), pitch? (mm per turn, default 0), delete_source? (default true), dry_run?}. Without axis_id and axis_at, the axis is vertical at the left edge of the profile and the profile is read like an elevation: x is the distance from the axis, y the height above the bottom edge. A profile on both sides of the axis is skipped. Returns {created: [{source, id}], skipped: [{id, reason}], dry_run}.",
);
pub const POLYGON3D_ADD: (&str, &str) = (
    "polygon3d.add",
    "Add a 3D polygon (shape kind 'polyline3d'): vertices with a height. params: {points: [[x, y, z]], closed? (default true; a closed polygon needs 3 points, an open one 2), fill? (a color [r, g, b, a]: a filled closed polygon is a face in 3D views and renders), name? (in the object data), layer?, class?, parent?}. z is measured from the layer, in mm. Returns {id}.",
);
pub const POLYGON3D_SET: (&str, &str) = (
    "polygon3d.set",
    "Change 3D polygons. params: {id | ids, points?, closed?, raise? (mm added to the z of every vertex)}. Returns {changed: [ids]}.",
);
pub const POLYGON3D_GET: (&str, &str) = (
    "polygon3d.get",
    "One 3D polygon. params: {id}. Returns {id, points, closed, z_range_mm: [low, high], length_m, area_plan_m2 (closed), face (true when it has a fill and draws as a face)}.",
);

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "solids": "Extrude (shape kind 'extrude'), sweep ('sweep') and 3D polygon ('polyline3d') are the 3D modeling objects of Model. An extrude keeps its 2D profile (and holes) and a height. A sweep keeps a profile in the plane of an axis and turns it by a sweep angle in segments of a segment angle; a pitch lifts it for a spiral. Both are closed solids when the profile is closed: the 3D view, the render, glTF/GLB/OBJ and the IFC and DXF exports use the same mesh, and sections cut them. Units are millimeters; z is measured from the elevation of the layer. Extrude Along Path is not built: use a sweep for a turned profile. Limits: 8000 points in a profile after curves are cut, 720 segments, 400000 triangles.",
        "examples": [
            {"method": "extrude.add", "params": {"profile": [[0, 0], [3000, 0], [3000, 1200], [0, 1200]], "holes": [[[400, 400], [1400, 400], [1400, 800], [400, 800]]], "height": 450, "name": "Planter"}},
            {"method": "extrude.from_shapes", "params": {"ids": ["00000000-0000-0000-0000-000000000003"], "height": 2500}},
            {"method": "sweep.add", "params": {"profile": [[0, 0], [400, 0], [300, 800], [0, 800]], "origin": [5000, 5000, 0], "name": "Pot"}},
            {"method": "sweep.add", "params": {"profile": [[800, 0], [1200, 0], [1200, 150], [800, 150]], "sweep_angle": 720, "pitch": 1500, "segment_angle": 8}},
            {"method": "polygon3d.add", "params": {"points": [[0, 0, 0], [4000, 0, 300], [4000, 3000, 600], [0, 3000, 300]], "fill": "#a0b890"}}
        ]
    })
}

/// Handles a method of this module. `None` means that the method is not one of ours.
pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !(method.starts_with("extrude.")
        || method.starts_with("sweep.")
        || method.starts_with("polygon3d."))
    {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    let Some(m) = p.as_object() else {
        return Some(Err(bad("params must be an object")));
    };
    Some(match method {
        "extrude.add" => add_extrude(s, m),
        "extrude.set" => set_extrude(s, m),
        "extrude.get" => get_extrude(s, m),
        "extrude.from_shapes" => from_shapes(s, m, true),
        "sweep.add" => add_sweep(s, m),
        "sweep.set" => set_sweep(s, m),
        "sweep.get" => get_sweep(s, m),
        "sweep.from_shapes" => from_shapes(s, m, false),
        "polygon3d.add" => add_polygon(s, m),
        "polygon3d.set" => set_polygon(s, m),
        "polygon3d.get" => get_polygon(s, m),
        other => Err(RpcError::method_not_found(other)),
    })
}

/// Parameters of the call that place a new object, not fields of the model.
const CALL_KEYS: [&str; 5] = ["id", "ids", "layer", "class", "parent"];

#[derive(Deserialize)]
struct Place {
    layer: Option<LayerId>,
    class: Option<ClassId>,
    parent: Option<ObjectId>,
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

/// Applies the given fields to a model (as JSON). A `null` removes a field (back to its default).
fn patch_json(
    base: Value,
    m: &Map<String, Value>,
    fields: &[(&str, &str)],
) -> Result<Value, RpcError> {
    let mut v = base;
    let o = v.as_object_mut().expect("object");
    for (k, x) in m {
        if CALL_KEYS.contains(&k.as_str()) {
            continue;
        }
        let Some((_, key)) = fields.iter().find(|(name, _)| name == k) else {
            let names: Vec<&str> = fields.iter().map(|f| f.0).collect();
            return Err(bad(format!(
                "unknown field '{k}'. Fields: {}",
                names.join(", ")
            )));
        };
        if x.is_null() {
            o.remove(*key);
        } else {
            o.insert((*key).to_string(), x.clone());
        }
    }
    Ok(v)
}

fn add_command(place: Place, shape: Shape) -> Command {
    match place.parent {
        Some(parent) => Command::AddObjectIn {
            parent,
            shape,
            class: place.class,
            attrs: Attributes::default(),
            id: None,
        },
        None => Command::AddObject {
            shape,
            layer: place.layer,
            class: place.class,
            attrs: Attributes::default(),
        },
    }
}

fn arr3(v: DVec3) -> Value {
    json!([v.x, v.y, v.z])
}

fn mesh_numbers(shape: &Shape) -> (Value, Value) {
    let Some(mesh) = shape.solid_mesh() else {
        return (Value::Null, Value::Null);
    };
    let closed = mesh.is_closed();
    (
        json!({
            "volume_m3": closed.then(|| mesh.volume() / 1e9),
            "surface_area_m2": mesh.area() / 1e6,
            "triangles": mesh.triangles.len(),
        }),
        shape.solid_bounds3().map_or(Value::Null, |(lo, hi)| json!({"min": arr3(lo), "max": arr3(hi)})),
    )
}

// ------------------------------------------------------------------ extrude

const EXTRUDE_FIELDS: [(&str, &str); 5] = [
    ("profile", "profile"),
    ("holes", "holes"),
    ("height", "height"),
    ("base", "base"),
    ("name", "name"),
];

fn patched_extrude(base: &Extrude, m: &Map<String, Value>) -> Result<Extrude, RpcError> {
    let v = patch_json(serde_json::to_value(base).expect("json"), m, &EXTRUDE_FIELDS)?;
    let out: Extrude = serde_json::from_value(v).map_err(bad)?;
    match out.problem() {
        Some(why) => Err(bad(why)),
        None => Ok(out),
    }
}

fn add_extrude(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    for k in ["profile", "height"] {
        if !m.contains_key(k) {
            return Err(bad(format!("missing '{k}'")));
        }
    }
    let place: Place = serde_json::from_value(Value::Object(m.clone())).map_err(bad)?;
    let model = patched_extrude(&Extrude::new(Vec::new(), 1.0), m)?;
    let shape = Shape::Extrude(Box::new(model));
    let (q, _) = mesh_numbers(&shape);
    let out = s
        .editor
        .execute(&add_command(place, shape))
        .map_err(RpcError::from)?;
    Ok(json!({"id": out.created[0], "quantities": q}))
}

fn set_extrude(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let ids = ids_of(m)?;
    let mut commands = Vec::new();
    for id in &ids {
        let o = s
            .editor
            .doc()
            .object(*id)
            .ok_or_else(|| bad(format!("object {id} does not exist")))?;
        let Shape::Extrude(cur) = &o.shape else {
            return Err(bad(format!("object {id} is not an extrude")));
        };
        let new = patched_extrude(cur, m)?;
        commands.push(Command::ModifyObject {
            id: *id,
            shape: Some(Shape::Extrude(Box::new(new))),
            layer: None,
            class: None,
            attrs: None,
        });
    }
    s.editor
        .execute(&Command::Batch { commands })
        .map_err(RpcError::from)?;
    Ok(json!({"changed": ids}))
}

fn get_extrude(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let id = ids_of(m)?[0];
    let o = s
        .editor
        .doc()
        .object(id)
        .ok_or_else(|| bad(format!("object {id} does not exist")))?;
    let Shape::Extrude(e) = &o.shape else {
        return Err(bad(format!("object {id} is not an extrude")));
    };
    let mut v = serde_json::to_value(e.as_ref()).expect("json");
    let (mut q, bounds) = mesh_numbers(&o.shape);
    q["profile_area_m2"] = json!(e.profile_area() / 1e6);
    v["id"] = json!(id);
    v["holes"] = v.get("holes").cloned().unwrap_or_else(|| json!([]));
    v["base"] = json!(e.base);
    v["quantities"] = q;
    v["bounds_mm"] = bounds;
    Ok(v)
}

// ------------------------------------------------------------------ sweep

const SWEEP_FIELDS: [(&str, &str); 9] = [
    ("profile", "profile"),
    ("closed", "closed"),
    ("origin", "origin"),
    ("axis", "axis"),
    ("rotation", "rotation_deg"),
    ("sweep_angle", "sweep_angle_deg"),
    ("segment_angle", "segment_angle_deg"),
    ("pitch", "pitch"),
    ("name", "name"),
];

fn patched_sweep(base: &Sweep, m: &Map<String, Value>) -> Result<Sweep, RpcError> {
    let v = patch_json(serde_json::to_value(base).expect("json"), m, &SWEEP_FIELDS)?;
    let out: Sweep = serde_json::from_value(v).map_err(bad)?;
    match out.problem() {
        Some(why) => Err(bad(why)),
        None => Ok(out),
    }
}

fn sweep_warnings(sw: &Sweep) -> Vec<String> {
    let mut w = Vec::new();
    if sw.segments_capped() {
        w.push(format!(
            "the segment angle is too small: the sweep has the largest number of segments, {}",
            solid3d::MAX_SEGMENTS
        ));
    }
    w
}

fn add_sweep(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    if !m.contains_key("profile") {
        return Err(bad("missing 'profile'"));
    }
    let place: Place = serde_json::from_value(Value::Object(m.clone())).map_err(bad)?;
    let model = patched_sweep(&Sweep::new(Vec::new()), m)?;
    let warnings = sweep_warnings(&model);
    let shape = Shape::Sweep(Box::new(model));
    let (q, _) = mesh_numbers(&shape);
    let out = s
        .editor
        .execute(&add_command(place, shape))
        .map_err(RpcError::from)?;
    Ok(json!({"id": out.created[0], "quantities": q, "warnings": warnings}))
}

fn set_sweep(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let ids = ids_of(m)?;
    let mut commands = Vec::new();
    let mut warnings = Vec::new();
    for id in &ids {
        let o = s
            .editor
            .doc()
            .object(*id)
            .ok_or_else(|| bad(format!("object {id} does not exist")))?;
        let Shape::Sweep(cur) = &o.shape else {
            return Err(bad(format!("object {id} is not a sweep")));
        };
        let new = patched_sweep(cur, m)?;
        for w in sweep_warnings(&new) {
            warnings.push(json!({"id": id, "warning": w}));
        }
        commands.push(Command::ModifyObject {
            id: *id,
            shape: Some(Shape::Sweep(Box::new(new))),
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

fn get_sweep(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let id = ids_of(m)?[0];
    let o = s
        .editor
        .doc()
        .object(id)
        .ok_or_else(|| bad(format!("object {id} does not exist")))?;
    let Shape::Sweep(sw) = &o.shape else {
        return Err(bad(format!("object {id} is not a sweep")));
    };
    let (q, bounds) = mesh_numbers(&o.shape);
    Ok(json!({
        "id": id,
        "name": sw.name,
        "profile": sw.profile,
        "closed": sw.closed,
        "origin": arr3(sw.origin),
        "axis": arr3(sw.axis),
        "rotation": sw.rotation_deg,
        "sweep_angle": sw.sweep_angle_deg,
        "segment_angle": sw.segment_angle_deg,
        "pitch": sw.pitch,
        "segments": sw.segments(),
        "quantities": q,
        "bounds_mm": bounds,
        "warnings": sweep_warnings(sw),
    }))
}

// ------------------------------------------------------------------ Model > Extrude, Model > Sweep

fn from_shapes(s: &mut Session, m: &Map<String, Value>, extrude: bool) -> Result<Value, RpcError> {
    let mut opts = m.clone();
    let ids: Vec<ObjectId> = match opts.remove("ids") {
        Some(v) => serde_json::from_value(v).map_err(bad)?,
        None => s.selection.clone(),
    };
    let dry_run = opts
        .remove("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if ids.is_empty() {
        return Err(bad("select 2D shapes or give ids"));
    }
    let made: Made = if extrude {
        let o: ExtrudeOptions = serde_json::from_value(Value::Object(opts)).map_err(bad)?;
        o.check().map_err(bad)?;
        solid_batch::extrude(s.editor.doc(), &ids, &o)
    } else {
        for (from, to) in [
            ("sweep_angle", "sweep_angle_deg"),
            ("segment_angle", "segment_angle_deg"),
        ] {
            if let Some(v) = opts.remove(from) {
                opts.insert(to.into(), v);
            }
        }
        let o: SweepOptions = serde_json::from_value(Value::Object(opts)).map_err(bad)?;
        o.check().map_err(bad)?;
        solid_batch::sweep(s.editor.doc(), &ids, &o)
    };
    if let (false, Some(cmd)) = (dry_run, &made.command) {
        s.editor.execute(cmd).map_err(RpcError::from)?;
        s.prune_selection();
        s.selection = made.created.iter().map(|c| c.1).collect();
    }
    Ok(json!({
        "created": made.created.iter().map(|c| json!({"source": c.0, "id": c.1})).collect::<Vec<_>>(),
        "skipped": made.skipped.iter().map(|c| json!({"id": c.0, "reason": c.1})).collect::<Vec<_>>(),
        "dry_run": dry_run,
    }))
}

// ------------------------------------------------------------------ 3D polygon

fn points_of(v: &Value) -> Result<Vec<DVec3>, RpcError> {
    let pts: Vec<DVec3> = serde_json::from_value(v.clone()).map_err(bad)?;
    if pts.iter().any(|p| !p.is_finite()) {
        return Err(bad("the points must be finite numbers"));
    }
    if pts.len() > solid3d::MAX_RING_POINTS {
        return Err(bad("too many points"));
    }
    Ok(pts)
}

fn check_polygon(points: &[DVec3], closed: bool) -> Result<(), RpcError> {
    let need = if closed { 3 } else { 2 };
    if points.len() < need {
        return Err(bad(format!(
            "a {} 3D polygon needs at least {need} points",
            if closed { "closed" } else { "open" }
        )));
    }
    Ok(())
}

fn add_polygon(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let Some(p) = m.get("points") else {
        return Err(bad("missing 'points'"));
    };
    for k in m.keys() {
        if !["points", "closed", "fill", "name", "layer", "class", "parent"].contains(&k.as_str()) {
            return Err(bad(format!(
                "unknown field '{k}'. Fields: points, closed, fill, name, layer, class, parent"
            )));
        }
    }
    let points = points_of(p)?;
    let closed = m.get("closed").and_then(Value::as_bool).unwrap_or(true);
    check_polygon(&points, closed)?;
    let place: Place = serde_json::from_value(Value::Object(m.clone())).map_err(bad)?;
    let mut cmd = add_command(place, Shape::Polyline3d { points, closed });
    if let Some(f) = m.get("fill") {
        let color: kadakas_core::model::Color = serde_json::from_value(f.clone()).map_err(bad)?;
        match &mut cmd {
            Command::AddObject { attrs, .. } | Command::AddObjectIn { attrs, .. } => {
                attrs.fill = Some(color);
            }
            _ => {}
        }
    }
    let out = s.editor.execute(&cmd).map_err(RpcError::from)?;
    let id = ObjectId(out.created[0]);
    if let Some(name) = m.get("name").and_then(Value::as_str) {
        s.editor
            .execute(&Command::SetData {
                id,
                set: [("name".to_string(), json!(name))].into_iter().collect(),
                remove: Vec::new(),
            })
            .map_err(RpcError::from)?;
    }
    Ok(json!({"id": id}))
}

fn set_polygon(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    for k in m.keys() {
        if !["id", "ids", "points", "closed", "raise"].contains(&k.as_str()) {
            return Err(bad(format!(
                "unknown field '{k}'. Fields: points, closed, raise"
            )));
        }
    }
    let ids = ids_of(m)?;
    let raise = match m.get("raise") {
        None => 0.0,
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite())
            .ok_or_else(|| bad("raise must be a finite number"))?,
    };
    let mut commands = Vec::new();
    for id in &ids {
        let o = s
            .editor
            .doc()
            .object(*id)
            .ok_or_else(|| bad(format!("object {id} does not exist")))?;
        let Shape::Polyline3d { points, closed } = &o.shape else {
            return Err(bad(format!("object {id} is not a 3D polygon")));
        };
        let mut points = match m.get("points") {
            Some(v) => points_of(v)?,
            None => points.clone(),
        };
        let closed = m.get("closed").and_then(Value::as_bool).unwrap_or(*closed);
        for p in &mut points {
            p.z += raise;
        }
        check_polygon(&points, closed)?;
        commands.push(Command::ModifyObject {
            id: *id,
            shape: Some(Shape::Polyline3d { points, closed }),
            layer: None,
            class: None,
            attrs: None,
        });
    }
    s.editor
        .execute(&Command::Batch { commands })
        .map_err(RpcError::from)?;
    Ok(json!({"changed": ids}))
}

fn get_polygon(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let id = ids_of(m)?[0];
    let doc = s.editor.doc();
    let o = doc
        .object(id)
        .ok_or_else(|| bad(format!("object {id} does not exist")))?;
    let Shape::Polyline3d { points, closed } = &o.shape else {
        return Err(bad(format!("object {id} is not a 3D polygon")));
    };
    let (lo, hi) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| (a.min(p.z), b.max(p.z)));
    let face = *closed && doc.resolved_attrs(o).fill.is_some_and(|c| c.0[3] > 0);
    Ok(json!({
        "id": id,
        "points": points.iter().map(|p| arr3(*p)).collect::<Vec<_>>(),
        "closed": closed,
        "z_range_mm": [lo, hi],
        "length_m": kadakas_core::props::length_m(&o.shape),
        "area_plan_m2": closed.then(|| o.shape.area().map(|a| a / 1e6)).flatten(),
        "face": face,
    }))
}
