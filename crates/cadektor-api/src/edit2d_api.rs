//! Agent API for editing 2D geometry: rotate, scale, mirror, duplicate array, offset, fillet,
//! chamfer, trim, extend, split and the surface operations.
//!
//! Methods: `edit.rotate|scale|mirror|array|offset|fillet|chamfer|trim|extend|split`,
//! `surface.add|subtract|clip|intersect|compose`. Every call is one undo step. The logic is in
//! `cadektor_core::edit2d`.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use cadektor_core::DVec2;
use cadektor_core::command::{Command, Outcome};
use cadektor_core::edit2d::modify::{JoinKind, JoinMode};
use cadektor_core::edit2d::offset::Side;
use cadektor_core::edit2d::plan::{self, Corner, OffsetTo, SurfaceOp};
use cadektor_core::edit2d::{ArrayParams, Transform};
use cadektor_core::model::ObjectId;

use crate::{RpcError, Session};

pub const EDIT_ROTATE: (&str, &str) = (
    "edit.rotate",
    "Turn objects (a group as one unit). params: {ids? (default: the selection), center: [x, y], angle (radians, counterclockwise), copy? (turn copies, keep the originals)}. Returns {created: [ids of the copies]}. Hardscapes, landscape areas and site models cannot be turned.",
);
pub const EDIT_SCALE: (&str, &str) = (
    "edit.scale",
    "Scale objects about a point. params: {ids?, center: [x, y], sx, sy? (default sx; a negative factor mirrors), copy?}. Circles, arcs and symbols need sx = sy. Fillet vertices and curves are flattened for a stretch. Returns {created}.",
);
pub const EDIT_MIRROR: (&str, &str) = (
    "edit.mirror",
    "Mirror objects on the line through a and b. params: {ids?, a: [x, y], b: [x, y], copy? (default false: the objects move to the other side)}. Returns {created}.",
);
pub const EDIT_ARRAY: (&str, &str) = (
    "edit.array",
    "Duplicate array. params: {ids?, shape: linear | rectangular | circular, retain? (keep the original, default true), resize? (each copy this many times larger than the one before), rotate? (radians; each copy turned this much more than the one before)}. linear: {count, offset: [dx, dy] | distance + angle}; rectangular: {columns, rows, column_spacing, row_spacing, angle?}, the place at column 0 and row 0 is the original; circular: {count, center: [x, y], angle? (between copies, radians; default spreads them over a full turn), rotate_objects? (default true)}. Returns {created: [ids of the copies]}.",
);
pub const EDIT_OFFSET: (&str, &str) = (
    "edit.offset",
    "Offset: a parallel copy of a line, arc, circle, rectangle or polyline. params: {id, distance + side (left | right | outside | inside; default outside for closed shapes, left otherwise) | through: [x, y] (the copy passes through the point), copy? (default true; false moves the object itself)}. Arcs stay arcs; corners get a miter, or a bevel when the miter is longer than 4 times the distance. Returns {created}.",
);
pub const EDIT_FILLET: (&str, &str) = (
    "edit.fillet",
    "Fillet (round a corner). params: {radius, mode? (trim (default) | split | standard)}. Between two objects: {a, b, pick_a: [x, y], pick_b: [x, y]} (the picks are points on the parts to keep). trim cuts both and the fillet arc is a new object; split keeps the cut-off parts as objects; standard leaves the objects alone. Inside one shape: {id, corner? (index of the corner: between segment k and the next) | corners? ([indices]) | all? (every corner)}, the fillet becomes part of the shape (exact arc vertices). Lines that are too short are extended. Returns {created}.",
);
pub const EDIT_CHAMFER: (&str, &str) = (
    "edit.chamfer",
    "Chamfer (cut a corner with a line). Same objects and modes as edit.fillet. The size is {distance, distance2? (default distance)} measured from the corner along the first and second object, or {distance, angle} (radians between the first object and the chamfer), or {length} of the chamfer line. Returns {created}.",
);
pub const EDIT_TRIM: (&str, &str) = (
    "edit.trim",
    "Trim: remove the part of an object at a point, between the nearest crossings with other objects of the layer. params: {id, pick: [x, y], cutters? (ids; default: all other objects of the layer)}. A closed shape becomes an open one. Returns {created} (a second piece, if the click was between two crossings).",
);
pub const EDIT_EXTEND: (&str, &str) = (
    "edit.extend",
    "Extend the end of an open line, arc or polyline (the end nearer to the point) to the nearest boundary. params: {id, pick: [x, y], boundaries? (ids; default: all other objects of the layer)}.",
);
pub const EDIT_SPLIT: (&str, &str) = (
    "edit.split",
    "Split an object. params: {id, at: [[x, y]] (points on the object; it is cut there) | line: {a: [x, y], b: [x, y]} (cut a closed shape along the infinite line through a and b into closed polylines; the pieces together keep the area)}. Returns {created} (the first piece keeps the ID).",
);
pub const SURFACE_ADD: (&str, &str) = (
    "surface.add",
    "Add surface: one shape from the union of rectangles, circles and polylines on one layer. params: {ids (2 or more), keep? (keep the other shapes, default false)}. The lowest shape keeps its ID and attributes. Curves are flattened to 0.05 mm. Returns {created}.",
);
pub const SURFACE_SUBTRACT: (&str, &str) = (
    "surface.subtract",
    "Subtract surface: the base shape without the area of the cutters. params: {base, cutters: [ids], keep_cutters? (default false)}. The result can have holes (a polyline with a bridge of no width). Returns {created}; the base is deleted if nothing is left.",
);
pub const SURFACE_CLIP: (&str, &str) = (
    "surface.clip",
    "Clip surface: the topmost shape cuts out its area of the shapes under it. params: {ids (2 or more), keep_cutter? (default true)}. Returns {created}.",
);
pub const SURFACE_INTERSECT: (&str, &str) = (
    "surface.intersect",
    "Intersect surface: the area that all shapes share. params: {ids (2 or more), keep? (default false)}. The lowest shape keeps its ID. Returns {created}.",
);
pub const SURFACE_COMPOSE: (&str, &str) = (
    "surface.compose",
    "Compose: split overlapping shapes into regions that do not overlap (every overlap and every rest is a new shape, in the attributes of the topmost shape that covers it). params: {ids (2 or more), keep? (keep the originals, default false)}. Returns {created}.",
);

const METHOD_NAMES: &[&str] = &[
    "edit.rotate",
    "edit.scale",
    "edit.mirror",
    "edit.array",
    "edit.offset",
    "edit.fillet",
    "edit.chamfer",
    "edit.trim",
    "edit.extend",
    "edit.split",
    "surface.add",
    "surface.subtract",
    "surface.clip",
    "surface.intersect",
    "surface.compose",
];

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "notes": "Every edit is one undo step and works through commands: edit.rotate, edit.scale, edit.mirror and edit.array run the commands transform_objects and array_objects; the others run a batch of modify_object, add_object and delete_objects. Lines, arcs, circles and rectangles are exact. Polylines are read as lines and exact arcs (fillet vertices and arcs through three points); Bezier and spline pieces are cut into short lines (0.05 mm). Edited polylines write arcs back as point_on_arc vertices. Surface operations flatten curves to 0.05 mm: a circle becomes a polygon with the exact area of the circle.",
        "examples": [
            {"method": "edit.mirror", "params": {"ids": ["00000000-0000-0000-0000-000000000002"], "a": [0, 0], "b": [0, 1000], "copy": true}},
            {"method": "edit.array", "params": {"ids": ["00000000-0000-0000-0000-000000000002"], "shape": "rectangular", "columns": 4, "rows": 3, "column_spacing": 2000, "row_spacing": 1500}},
            {"method": "edit.array", "params": {"ids": ["00000000-0000-0000-0000-000000000002"], "shape": "circular", "count": 5, "center": [0, 0]}},
            {"method": "edit.offset", "params": {"id": "00000000-0000-0000-0000-000000000002", "distance": 500, "side": "outside"}},
            {"method": "edit.fillet", "params": {"a": 2, "b": 3, "pick_a": [8000, 0], "pick_b": [0, 8000], "radius": 500}},
            {"method": "edit.chamfer", "params": {"id": "00000000-0000-0000-0000-000000000004", "all": true, "distance": 300}},
            {"method": "edit.trim", "params": {"id": "00000000-0000-0000-0000-000000000002", "pick": [5000, 0]}},
            {"method": "edit.split", "params": {"id": "00000000-0000-0000-0000-000000000005", "line": {"a": [4000, -1000], "b": [4000, 9000]}}},
            {"method": "surface.subtract", "params": {"base": "00000000-0000-0000-0000-000000000002", "cutters": ["00000000-0000-0000-0000-000000000003"]}},
        ],
    })
}

fn bad(e: impl std::fmt::Display) -> RpcError {
    RpcError::invalid_params(e)
}

/// The error for a plan that cannot be made: a missing object is "not found", the rest is
/// "validation" (the request is well formed, the shapes do not allow it).
fn plan_error(msg: String) -> RpcError {
    if msg.starts_with("no object ") {
        RpcError::not_found(msg)
    } else {
        RpcError::validation(msg)
    }
}

fn read<T: DeserializeOwned>(m: &Map<String, Value>) -> Result<T, RpcError> {
    serde_json::from_value(Value::Object(m.clone())).map_err(bad)
}

fn ids_or_selection(s: &Session, ids: Option<Vec<ObjectId>>) -> Result<Vec<ObjectId>, RpcError> {
    let ids = ids.unwrap_or_else(|| s.selection.clone());
    if ids.is_empty() {
        return Err(bad("give ids, or select objects first"));
    }
    Ok(ids)
}

fn run(s: &mut Session, cmd: &Command) -> Result<Outcome, RpcError> {
    let out = s.editor.execute(cmd).map_err(RpcError::from)?;
    s.prune_selection();
    Ok(out)
}

fn created(out: &Outcome) -> Value {
    json!({ "created": out.created })
}

/// Runs a plan (a batch of commands) and returns the IDs of the new objects.
fn run_plan(s: &mut Session, plan: Result<Command, String>) -> Result<Value, RpcError> {
    let cmd = plan.map_err(plan_error)?;
    let out = run(s, &cmd)?;
    Ok(created(&out))
}

#[derive(Deserialize)]
struct Ids {
    ids: Option<Vec<ObjectId>>,
    #[serde(default)]
    copy: bool,
}

fn transform(
    s: &mut Session,
    m: &Map<String, Value>,
    make: impl Fn(&Map<String, Value>) -> Result<Transform, RpcError>,
) -> Result<Value, RpcError> {
    let Ids { ids, copy } = read(m)?;
    let ids = ids_or_selection(s, ids)?;
    let transform = make(m)?;
    let out = run(
        s,
        &Command::TransformObjects {
            ids,
            transform,
            copy,
        },
    )?;
    if copy {
        s.selection = out.created.iter().map(|c| ObjectId(*c)).collect();
    }
    Ok(created(&out))
}

fn mode_of(m: &Map<String, Value>) -> Result<JoinMode, RpcError> {
    match m.get("mode").and_then(Value::as_str) {
        None | Some("trim") => Ok(JoinMode::Trim),
        Some("split") => Ok(JoinMode::Split),
        Some("standard") => Ok(JoinMode::Standard),
        Some(o) => Err(bad(format!(
            "unknown mode '{o}'. Use trim, split or standard"
        ))),
    }
}

#[derive(Deserialize)]
struct JoinParams {
    id: Option<ObjectId>,
    a: Option<ObjectId>,
    b: Option<ObjectId>,
    pick_a: Option<DVec2>,
    pick_b: Option<DVec2>,
    corner: Option<usize>,
    corners: Option<Vec<usize>>,
    #[serde(default)]
    all: bool,
}

/// Fillet and chamfer: the corner inside one shape, or the corner between two objects.
fn join(s: &mut Session, m: &Map<String, Value>, kind: JoinKind) -> Result<Value, RpcError> {
    let p: JoinParams = read(m)?;
    let mode = mode_of(m)?;
    if let Some(id) = p.id {
        let list = match (p.corner, &p.corners, p.all) {
            (Some(c), None, false) => Some(vec![c]),
            (None, Some(c), false) => Some(c.clone()),
            (None, None, true) => None,
            _ => return Err(bad("give corner, corners or all")),
        };
        if mode == JoinMode::Standard {
            return Err(bad("mode standard needs two objects (a and b)"));
        }
        return run_plan(
            s,
            plan::round_all(s.editor.doc(), id, list.as_deref(), kind),
        );
    }
    let (Some(a), Some(b)) = (p.a, p.b) else {
        return Err(bad("give a and b (two objects), or id (one shape)"));
    };
    let (Some(pa), Some(pb)) = (p.pick_a, p.pick_b) else {
        return Err(bad("give pick_a and pick_b: points on the parts to keep"));
    };
    if !pa.is_finite() || !pb.is_finite() {
        return Err(bad("numbers must be finite"));
    }
    run_plan(
        s,
        plan::join(s.editor.doc(), a, pa, b, pb, Corner { kind, mode }),
    )
}

#[derive(Deserialize)]
struct OffsetParams {
    id: ObjectId,
    distance: Option<f64>,
    side: Option<String>,
    through: Option<DVec2>,
    copy: Option<bool>,
}

#[derive(Deserialize)]
struct TrimParams {
    id: ObjectId,
    pick: DVec2,
    cutters: Option<Vec<ObjectId>>,
    boundaries: Option<Vec<ObjectId>>,
}

#[derive(Deserialize)]
struct SplitParams {
    id: ObjectId,
    at: Option<Vec<DVec2>>,
    line: Option<LineParams>,
}

#[derive(Deserialize)]
struct LineParams {
    a: DVec2,
    b: DVec2,
}

#[derive(Deserialize)]
struct SurfaceParams {
    ids: Option<Vec<ObjectId>>,
    base: Option<ObjectId>,
    cutters: Option<Vec<ObjectId>>,
    keep: Option<bool>,
    keep_cutters: Option<bool>,
    keep_cutter: Option<bool>,
}

fn finite(v: DVec2, key: &str) -> Result<DVec2, RpcError> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(bad(format!("'{key}' must be finite")))
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

/// Handles a method of this module. `None` means that the method is not one of ours.
pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if !METHOD_NAMES.contains(&method) {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    Some(match p.as_object() {
        Some(m) => call(s, method, m),
        None => Err(bad("params must be an object")),
    })
}

fn call(s: &mut Session, method: &str, m: &Map<String, Value>) -> Result<Value, RpcError> {
    match method {
        "edit.rotate" => transform(s, m, |m| {
            #[derive(Deserialize)]
            struct P {
                center: DVec2,
                angle: f64,
            }
            let P { center, angle } = read(m)?;
            Ok(Transform::Rotate { center, angle })
        }),
        "edit.scale" => transform(s, m, |m| {
            #[derive(Deserialize)]
            struct P {
                center: DVec2,
                sx: f64,
                sy: Option<f64>,
            }
            let P { center, sx, sy } = read(m)?;
            Ok(Transform::Scale { center, sx, sy })
        }),
        "edit.mirror" => transform(s, m, |m| {
            #[derive(Deserialize)]
            struct P {
                a: DVec2,
                b: DVec2,
            }
            let P { a, b } = read(m)?;
            Ok(Transform::Mirror { a, b })
        }),
        "edit.array" => {
            #[derive(Deserialize)]
            struct P {
                ids: Option<Vec<ObjectId>>,
            }
            let P { ids } = read(m)?;
            let ids = ids_or_selection(s, ids)?;
            let mut rest = m.clone();
            rest.remove("ids");
            let array: ArrayParams = read(&rest)?;
            let out = run(s, &Command::ArrayObjects { ids, array })?;
            s.selection = out.created.iter().map(|c| ObjectId(*c)).collect();
            Ok(created(&out))
        }
        "edit.offset" => {
            let p: OffsetParams = read(m)?;
            let to = match (p.distance, p.through) {
                (Some(_), Some(_)) => return Err(bad("give distance or through, not both")),
                (None, None) => return Err(bad("give distance, or through: [x, y]")),
                (None, Some(t)) => OffsetTo::Through(finite(t, "through")?),
                (Some(d), None) => {
                    let side = match p.side.as_deref() {
                        Some(n) => Side::from_name(n).ok_or_else(|| {
                            bad(format!(
                                "unknown side '{n}'. Use left, right, outside or inside"
                            ))
                        })?,
                        None => {
                            let closed = s.editor.doc().object(p.id).is_some_and(|o| {
                                cadektor_core::edit2d::Chain::from_shape(&o.shape)
                                    .is_some_and(|c| c.closed)
                            });
                            if closed { Side::Outside } else { Side::Left }
                        }
                    };
                    OffsetTo::Distance(d, side)
                }
            };
            let out = run_plan(
                s,
                plan::offset(s.editor.doc(), p.id, to, p.copy.unwrap_or(true)),
            )?;
            Ok(out)
        }
        "edit.fillet" => {
            let r = num(m, "radius")?.ok_or_else(|| bad("missing 'radius'"))?;
            join(s, m, JoinKind::Fillet(r))
        }
        "edit.chamfer" => {
            let size = plan::chamfer_size(
                num(m, "distance")?,
                num(m, "distance2")?,
                num(m, "angle")?,
                num(m, "length")?,
            )
            .map_err(bad)?;
            join(s, m, JoinKind::Chamfer(size))
        }
        "edit.trim" => {
            let p: TrimParams = read(m)?;
            run_plan(
                s,
                plan::trim(
                    s.editor.doc(),
                    p.id,
                    finite(p.pick, "pick")?,
                    p.cutters.as_deref(),
                ),
            )
        }
        "edit.extend" => {
            let p: TrimParams = read(m)?;
            run_plan(
                s,
                plan::extend(
                    s.editor.doc(),
                    p.id,
                    finite(p.pick, "pick")?,
                    p.boundaries.as_deref(),
                ),
            )
        }
        "edit.split" => {
            let p: SplitParams = read(m)?;
            match (p.at, p.line) {
                (Some(at), None) => {
                    if at.iter().any(|v| !v.is_finite()) {
                        return Err(bad("numbers must be finite"));
                    }
                    run_plan(s, plan::split(s.editor.doc(), p.id, &at))
                }
                (None, Some(l)) => run_plan(
                    s,
                    plan::split_by_line(
                        s.editor.doc(),
                        p.id,
                        finite(l.a, "line.a")?,
                        finite(l.b, "line.b")?,
                    ),
                ),
                _ => Err(bad(
                    "give at (points on the object) or line {a, b}, not both",
                )),
            }
        }
        "surface.add" | "surface.intersect" | "surface.compose" | "surface.clip" => {
            let p: SurfaceParams = read(m)?;
            let ids = p.ids.ok_or_else(|| bad("missing 'ids'"))?;
            let (op, keep) = match method {
                "surface.add" => (SurfaceOp::Add, p.keep.unwrap_or(false)),
                "surface.intersect" => (SurfaceOp::Intersect, p.keep.unwrap_or(false)),
                "surface.compose" => (SurfaceOp::Compose, p.keep.unwrap_or(false)),
                _ => (SurfaceOp::Clip, p.keep_cutter.unwrap_or(true)),
            };
            run_plan(s, plan::surface(s.editor.doc(), op, &ids, None, keep))
        }
        "surface.subtract" => {
            let p: SurfaceParams = read(m)?;
            let base = p.base.ok_or_else(|| bad("missing 'base'"))?;
            let cutters = p.cutters.ok_or_else(|| bad("missing 'cutters'"))?;
            run_plan(
                s,
                plan::surface(
                    s.editor.doc(),
                    SurfaceOp::Subtract,
                    &cutters,
                    Some(base),
                    p.keep_cutters.unwrap_or(false),
                ),
            )
        }
        _ => unreachable!("METHOD_NAMES lists only our methods"),
    }
}
