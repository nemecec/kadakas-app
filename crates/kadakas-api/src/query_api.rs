//! Agent API for queries and view-like settings: `objects.query` (find objects by criteria),
//! `snap.get|set|query|smart` (the Snapping set and the snap math) and `visibility.get|set`
//! (visible, grey and invisible layers and classes).

use serde::Deserialize;
use serde_json::{Map, Value, json};

use kadakas_core::DVec2;
use kadakas_core::command::Command;
use kadakas_core::model::{ClassId, LayerId, ObjectId};
use kadakas_core::snapping::{self, Geometry, Reach, Settings};
use kadakas_core::visibility::VisMode;

use crate::{RpcError, Session};

pub const OBJECTS_QUERY: (&str, &str) = (
    "objects.query",
    "Find objects by criteria. params: {criteria? (the criteria language of worksheet database blocks: Kind = \"plant\" AND Area > 20, Class = \"Trees\", Layer = \"Site\", 'Soil'.'Depth' > 300; empty matches all), columns? ([expressions, e.g. \"Area\", \"Style\"]; their values are returned per object), layer?, class?, kind? (as objects.list), limit? (default 1000), select? (true selects the matches, except objects of grey layers and classes, which cannot be selected)}. Returns {count, ids, rows? ([{id, values}] when columns are given), truncated, skipped (objects for which the criteria gave an error), grey_not_selected}. Names are properties: see describe.data.",
);
pub const SNAP_GET: (&str, &str) = (
    "snap.get",
    "The snapping settings of the session (the app uses the same ones): {set: {grid, object_points, midpoints, intersections, edges, perpendicular, tangent, angle, distance, smart_points, smart_edges}, angle_step_deg, relative_angles, distance_step, other_layers, grey}, and the acquired smart points and edges.",
);
pub const SNAP_SET: (&str, &str) = (
    "snap.set",
    "Change the snapping settings. params: {set? ({category: bool}), angle_step_deg? (above 0, at most 180), relative_angles?, distance_step? (mm; 0 uses the grid step), other_layers? (snap to objects of other layers), grey? (snap to objects of grey layers and classes)}. Returns what snap.get returns.",
);
pub const SNAP_QUERY: (&str, &str) = (
    "snap.query",
    "Where a point snaps to. params: {point: [x, y], tolerance? (mm, default 10), grid_step? (mm, default 100), anchor? ([x, y]: the last clicked point, for angle, distance, perpendicular and tangent), prior_direction? (radians), exclude? ([ids]), set? ({category: bool} for this call only)}. Returns {snapped, point, kind, cue (the label at the cursor), guides ([{a, b, kind}])}.",
);
pub const SNAP_SMART: (&str, &str) = (
    "snap.smart",
    "Smart points and smart edges (alignment lines). params: {action: acquire_point | acquire_edge | acquire_at | clear, point? ([x, y]), a? and b? ([x, y], the edge), tolerance? (acquire_at takes the object point or, with edges: true, the edge nearest to the point)}. Up to 3 points and 2 edges are kept; the oldest goes. Returns what snap.get returns.",
);
pub const VISIBILITY_GET: (&str, &str) = (
    "visibility.get",
    "The visibility mode (visible, grey or invisible) of layers and classes. Returns {layers: [{id, name, mode}], classes: [{id, name, mode}]}.",
);
pub const VISIBILITY_SET: (&str, &str) = (
    "visibility.set",
    "Set the visibility mode of layers and classes in one undo step. params: {mode: visible | grey | invisible, layers? ([ids]), classes? ([ids])}. A grey layer or class is drawn in grey and its objects cannot be selected, edited (commands refuse them) or (by default) snapped to. The active layer cannot be grey. Trim and extend may still use grey objects as cutters and boundaries. Returns what visibility.get returns.",
);

const METHOD_NAMES: &[&str] = &[
    "objects.query",
    "snap.get",
    "snap.set",
    "snap.query",
    "snap.smart",
    "visibility.get",
    "visibility.set",
];

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "criteria": "objects.query uses the criteria language of worksheet database blocks: comparisons (= <> < <= > >=), AND, OR, NOT, text in double quotes, and names of object properties (Kind, Class, Layer, Area, Style, record fields like 'Soil'.'Depth').",
        "snapping": "The snap math is the same as in the app. The categories: grid, object_points (endpoints, corners, centers, quadrants), midpoints, intersections, edges, perpendicular, tangent, angle, distance, smart_points, smart_edges. The cue is the text of the label at the cursor.",
        "examples": [
            {"method": "objects.query", "params": {"criteria": "Kind = \"plant\" AND Layer = \"Planting\"", "columns": ["Style", "Area"]}},
            {"method": "snap.set", "params": {"set": {"angle": true, "distance": true}, "angle_step_deg": 45}},
            {"method": "snap.query", "params": {"point": [1003, 998], "anchor": [0, 0], "tolerance": 10}},
            {"method": "visibility.set", "params": {"mode": "grey", "layers": ["00000000-0000-0000-0000-000000000002"]}},
        ],
    })
}

fn bad(e: impl std::fmt::Display) -> RpcError {
    RpcError::invalid_params(e)
}

fn read<T: for<'de> Deserialize<'de>>(m: &Map<String, Value>) -> Result<T, RpcError> {
    serde_json::from_value(Value::Object(m.clone())).map_err(bad)
}

fn finite(v: DVec2, key: &str) -> Result<DVec2, RpcError> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(bad(format!("'{key}' must be finite")))
    }
}

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

fn value_json(v: &kadakas_sheet::Value) -> Value {
    use kadakas_sheet::Value as V;
    match v {
        V::Empty => Value::Null,
        V::Number(n) => json!(n),
        V::Text(t) => json!(t),
        V::Bool(b) => json!(b),
        V::Error(e) => json!({"error": e.code()}),
    }
}

pub fn snap_json(s: &Session) -> Value {
    let st = &s.snap;
    json!({
        "set": st.set,
        "angle_step_deg": st.angle_step.to_degrees(),
        "relative_angles": st.relative_angles,
        "distance_step": st.distance_step,
        "other_layers": st.other_layers,
        "grey": st.grey,
        "smart": {"points": s.smart.points, "edges": s.smart.edges},
    })
}

fn visibility_json(s: &Session) -> Value {
    let doc = s.editor.doc();
    json!({
        "layers": doc.layers.iter().map(|l| json!({"id": l.id, "name": l.name, "mode": doc.layer_mode(l.id).map(VisMode::id)})).collect::<Vec<_>>(),
        "classes": doc.classes.iter().map(|c| json!({"id": c.id, "name": c.name, "mode": doc.class_mode(c.id).map(VisMode::id)})).collect::<Vec<_>>(),
    })
}

#[derive(Deserialize)]
struct QueryParams {
    #[serde(default)]
    criteria: String,
    #[serde(default)]
    columns: Vec<String>,
    layer: Option<LayerId>,
    class: Option<ClassId>,
    kind: Option<String>,
    limit: Option<usize>,
    #[serde(default)]
    select: bool,
}

#[derive(Deserialize)]
struct SnapSetParams {
    set: Option<Map<String, Value>>,
    angle_step_deg: Option<f64>,
    relative_angles: Option<bool>,
    distance_step: Option<f64>,
    other_layers: Option<bool>,
    grey: Option<bool>,
}

#[derive(Deserialize)]
struct SnapQueryParams {
    point: DVec2,
    tolerance: Option<f64>,
    grid_step: Option<f64>,
    anchor: Option<DVec2>,
    prior_direction: Option<f64>,
    #[serde(default)]
    exclude: Vec<ObjectId>,
    set: Option<Map<String, Value>>,
}

fn apply_set(set: &mut snapping::SnapSet, m: &Map<String, Value>) -> Result<(), RpcError> {
    for (k, v) in m {
        let on = v
            .as_bool()
            .ok_or_else(|| bad(format!("set.{k} must be true or false")))?;
        if !set.set(k, on) {
            return Err(bad(format!(
                "unknown snap category '{k}'. Use {}",
                snapping::SnapSet::NAMES.join(", ")
            )));
        }
    }
    Ok(())
}

fn run(s: &mut Session, method: &str, m: &Map<String, Value>) -> Result<Value, RpcError> {
    match method {
        "objects.query" => {
            let p: QueryParams = read(m)?;
            let doc = s.editor.doc();
            let ground = kadakas_site::SiteTerrain::new(s.site_cache(), s.editor.revision(), doc);
            let q = kadakas_sheet::db::query_objects(doc, &p.criteria, &p.columns, &ground)
                .map_err(bad)?;
            let limit = p.limit.unwrap_or(1000);
            let mut ids = Vec::new();
            let mut rows = Vec::new();
            let mut count = 0;
            for (id, row) in q.ids.iter().zip(&q.rows) {
                let o = doc.object(*id).expect("exists");
                if p.layer.is_some_and(|l| o.layer != l)
                    || p.class.is_some_and(|c| o.class != c)
                    || p.kind.as_deref().is_some_and(|k| o.shape.kind() != k)
                {
                    continue;
                }
                count += 1;
                if ids.len() < limit {
                    ids.push(*id);
                    if !p.columns.is_empty() {
                        rows.push(json!({"id": id, "values": row.iter().map(value_json).collect::<Vec<_>>()}));
                    }
                }
            }
            let truncated = count > ids.len();
            // Grey objects cannot be selected.
            let skipped_grey = if p.select {
                let doc = s.editor.doc();
                let (keep, grey): (Vec<ObjectId>, Vec<ObjectId>) = ids
                    .iter()
                    .partition(|id| doc.object(**id).is_some_and(|o| !doc.is_grey(o)));
                s.selection = keep;
                grey.len()
            } else {
                0
            };
            let mut out = json!({"count": count, "ids": ids, "truncated": truncated, "skipped": q.errors, "grey_not_selected": skipped_grey});
            if !p.columns.is_empty() {
                out["rows"] = json!(rows);
            }
            Ok(out)
        }
        "snap.get" => Ok(snap_json(s)),
        "snap.set" => {
            let p: SnapSetParams = read(m)?;
            let mut next = s.snap;
            if let Some(set) = &p.set {
                apply_set(&mut next.set, set)?;
            }
            if let Some(d) = p.angle_step_deg {
                next.angle_step = d.to_radians();
            }
            if let Some(v) = p.relative_angles {
                next.relative_angles = v;
            }
            if let Some(v) = p.distance_step {
                next.distance_step = v;
            }
            if let Some(v) = p.other_layers {
                next.other_layers = v;
            }
            if let Some(v) = p.grey {
                next.grey = v;
            }
            if let Some(msg) = next.problem() {
                return Err(bad(msg));
            }
            s.snap = next;
            Ok(snap_json(s))
        }
        "snap.query" => {
            let p: SnapQueryParams = read(m)?;
            let point = finite(p.point, "point")?;
            let tol = p.tolerance.unwrap_or(10.0);
            let grid = p.grid_step.unwrap_or(100.0);
            if !(tol.is_finite() && tol >= 0.0 && grid.is_finite() && grid >= 0.0) {
                return Err(bad("tolerance and grid_step must be 0 or more"));
            }
            let mut settings: Settings = s.snap;
            if let Some(set) = &p.set {
                apply_set(&mut settings.set, set)?;
            }
            let anchor = p.anchor.map(|a| finite(a, "anchor")).transpose()?;
            let geom = Geometry::from_doc(
                s.editor.doc(),
                &p.exclude,
                Reach::from(&settings),
                Some((point, tol * 4.0)),
            );
            let ctx = snapping::Context {
                tol,
                grid_step: grid,
                anchor,
                prior_direction: p.prior_direction,
            };
            Ok(
                match snapping::snap(point, &ctx, &settings, &geom, &s.smart) {
                    Some(r) => json!({
                        "snapped": true, "point": r.point, "kind": r.kind, "cue": r.cue,
                        "guides": r.guides.iter().map(|g| json!({"a": g.a, "b": g.b, "kind": g.kind})).collect::<Vec<_>>(),
                    }),
                    None => {
                        json!({"snapped": false, "point": point, "kind": null, "cue": null, "guides": []})
                    }
                },
            )
        }
        "snap.smart" => {
            #[derive(Deserialize)]
            struct P {
                action: String,
                point: Option<DVec2>,
                a: Option<DVec2>,
                b: Option<DVec2>,
                tolerance: Option<f64>,
                #[serde(default)]
                edges: bool,
            }
            let p: P = read(m)?;
            match p.action.as_str() {
                "acquire_point" => {
                    let pt = p.point.ok_or_else(|| bad("missing 'point'"))?;
                    s.smart.acquire_point(finite(pt, "point")?);
                }
                "acquire_edge" => {
                    let (a, b) = (
                        p.a.ok_or_else(|| bad("missing 'a'"))?,
                        p.b.ok_or_else(|| bad("missing 'b'"))?,
                    );
                    if a.distance(finite(b, "b")?) <= 1e-12 || !a.is_finite() {
                        return Err(bad("the edge needs two different finite points"));
                    }
                    s.smart.acquire_edge(a, b);
                }
                "acquire_at" => {
                    let pt = finite(p.point.ok_or_else(|| bad("missing 'point'"))?, "point")?;
                    let tol = p.tolerance.unwrap_or(10.0);
                    let geom = Geometry::from_doc(
                        s.editor.doc(),
                        &[],
                        Reach::from(&s.snap),
                        Some((pt, tol * 4.0)),
                    );
                    if s.smart.acquire_at(pt, &geom, tol, p.edges).is_none() {
                        return Err(RpcError::not_found(
                            "no object point or edge is near the point",
                        ));
                    }
                }
                "clear" => s.smart.clear(),
                other => {
                    return Err(bad(format!(
                        "unknown action '{other}'. Use acquire_point, acquire_edge, acquire_at or clear"
                    )));
                }
            }
            Ok(snap_json(s))
        }
        "visibility.get" => Ok(visibility_json(s)),
        "visibility.set" => {
            #[derive(Deserialize)]
            struct P {
                mode: String,
                #[serde(default)]
                layers: Vec<LayerId>,
                #[serde(default)]
                classes: Vec<ClassId>,
            }
            let p: P = read(m)?;
            let mode = VisMode::from_name(&p.mode)
                .ok_or_else(|| bad("mode must be visible, grey or invisible"))?;
            s.editor
                .execute(&Command::SetVisibility {
                    layers: p.layers,
                    classes: p.classes,
                    mode,
                })
                .map_err(RpcError::from)?;
            s.prune_selection();
            Ok(visibility_json(s))
        }
        _ => unreachable!("METHOD_NAMES lists only our methods"),
    }
}
