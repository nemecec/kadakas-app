//! Agent API for survey data: point files, tree surveys, source data checks and the survey
//! point of the georeference.
//!
//! Methods: `survey.import_points`, `tree.import_survey`, `site.check`, `site.simplify`,
//! `georef.survey_point`.

use glam::{DVec2, DVec3};
use serde_json::{Map, Value, json};

use cadektor_core::command::Command;
use cadektor_core::georef::Georeference;
use cadektor_core::model::{Attributes, ClassId, LayerId, ObjectId, Shape};
use cadektor_core::survey::{self, Column, Delimiter, PointOptions, TreeOptions};
use cadektor_site::check::{self, CheckOptions, Issue, Source};

use crate::errors::bad;
use crate::{RpcError, Session};

pub const SURVEY_IMPORT_POINTS: (&str, &str) = (
    "survey.import_points",
    "Import survey points from delimited text (CSV, TXT) as 3D loci with their code. params: {text | path, columns? (the column order, default [\"P\",\"N\",\"E\",\"Z\",\"Code\"]; letters or words: P/id, N/northing, E/easting, Z/elevation, C/code, D/description, - to skip), delimiter? (auto | comma | semicolon | tab | pipe | space), skip_lines? (default: the first line is skipped if it is not numbers), unit? (m default, mm, cm, ft, inch: unit of N and E), z_unit? (default: unit), crs? (EPSG code of a new georeference), shift? (default true: a survey more than 1 km from the origin gets a georeference at its center), layer?, class?}. The points go in model millimeters. Northing is Y, easting is X. Each locus has the data keys point_id, code and description. Returns {imported, skipped: [{line, reason}], georeference?, ids_from, ids_to}.",
);
pub const TREE_IMPORT_SURVEY: (&str, &str) = (
    "tree.import_survey",
    "Import a tree survey from delimited text as existing trees. The first line is a header: ID, E (or X), N (or Y), Species, DBH, Height, Spread, Condition, Action, Notes. A unit in the header ((cm), [mm], _m) wins. params: {text | path, delimiter?, unit? (N and E, default m), dbh_unit? (default cm), size_unit? (height and spread, default m), crs?, shift?, layer?, class?}. Returns {imported, skipped, georeference?, ids_from, ids_to}. The report is report.existing_trees.",
);
pub const SITE_CHECK: (&str, &str) = (
    "site.check",
    "Check the source data of a site model: duplicate XY with different Z, spikes (a point far from the plane of its neighbors) and crossing breaklines at different heights. params: {id, xy_tolerance? (mm, default 1), z_tolerance? (mm, default 10), spike_threshold? (mm, default 1000), max_issues? (default 1000)}. Returns {clean, counts: {duplicate_xy, spikes, crossing_breaklines, exact_duplicates}, vertices_checked, truncated, issues: [{kind, ...}]}.",
);
pub const SITE_SIMPLIFY: (&str, &str) = (
    "site.simplify",
    "Simplify the source data of a site model: remove copies, loose points that their neighbors predict within the tolerance, and line vertices within the tolerance of the line between their neighbors. One undo step. params: {id, tolerance (mm, positive), dry_run?}. Returns {removed_duplicates, removed_points, removed_vertices, points, lines}.",
);
pub const GEOREF_SURVEY_POINT: (&str, &str) = (
    "georef.survey_point",
    "Set the georeference from a known point (the Survey Point tool). The drawing does not move. params: {model: [x, y] (mm) | object (ID of an object whose position is used), map: [easting, northing] (m), crs? (default: the CRS of the document), rotation? (default: the current one), second? ({model | object, map}: a second known point sets the rotation)}. Returns {georeference, scale?} where scale is the map distance over the model distance of the two points (1 when they agree).",
);

pub fn describe() -> Value {
    json!({
        "point_file": "Columns by letter: P (point number), N, E, Z, C (code). Default order P,N,E,Z,Code. Delimiters: comma, semicolon, tab, pipe, space (auto detects it).",
        "tree_survey": "Header names: ID, E/X, N/Y, Species, DBH, Height, Spread, Condition (good|fair|poor|dead), Action (retain|remove|relocate).",
        "examples": [
            {"method": "survey.import_points", "params": {"text": "1,6672000.5,385000.25,12.5,TREE\n2,6672001,385001,12.75,KERB", "crs": "EPSG:3067"}},
            {"method": "survey.import_points", "params": {"path": "/data/points.txt", "columns": ["P", "E", "N", "Z", "C"], "delimiter": "space"}},
            {"method": "tree.import_survey", "params": {"text": "ID,E,N,Species,DBH (cm),Height (m),Spread (m),Condition,Action\nT1,10,20,Quercus robur,55,16,9,good,retain"}},
            {"method": "site.check", "params": {"id": 5}},
            {"method": "georef.survey_point", "params": {"model": [0, 0], "map": [385000, 6672000], "crs": "EPSG:3067"}}
        ],
    })
}

/// Handles a method of this module. `None`: it is not one of ours.
pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    let known = matches!(
        method,
        "survey.import_points"
            | "tree.import_survey"
            | "site.check"
            | "site.simplify"
            | "georef.survey_point"
    );
    if !known {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    let Some(m) = p.as_object() else {
        return Some(Err(bad("params must be an object")));
    };
    Some(match method {
        "survey.import_points" => import_points(s, m),
        "tree.import_survey" => import_trees(s, m),
        "site.check" => site_check(s, m),
        "site.simplify" => site_simplify(s, m),
        _ => survey_point(s, m),
    })
}

/// The most text that an import reads: 256 MB.
const MAX_TEXT: u64 = 256 * 1024 * 1024;

fn text_of(s: &Session, m: &Map<String, Value>) -> Result<String, RpcError> {
    if let Some(t) = m.get("text").and_then(Value::as_str) {
        if t.len() as u64 > MAX_TEXT {
            return Err(RpcError::limit("the text is too long"));
        }
        return Ok(t.to_string());
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = m.get("path").and_then(Value::as_str) {
        let path = s
            .check_path(
                std::path::Path::new(path),
                false,
                &["csv", "txt", "xyz", "pts", "dat", "asc", "tsv"],
            )
            .map_err(RpcError::policy)?;
        let meta = std::fs::metadata(&path)
            .map_err(|e| RpcError::not_found(format!("{}: {e}", path.display())))?;
        if meta.len() > MAX_TEXT {
            return Err(RpcError::limit("the file is larger than 256 MB"));
        }
        let bytes = std::fs::read(&path)
            .map_err(|e| RpcError::not_found(format!("{}: {e}", path.display())))?;
        return Ok(cadektor_io::decode_text(&bytes));
    }
    let _ = s;
    Err(bad("give text (the file content) or path"))
}

fn delimiter_of(m: &Map<String, Value>) -> Result<Delimiter, RpcError> {
    match m.get("delimiter").and_then(Value::as_str) {
        None => Ok(Delimiter::Auto),
        Some(n) => Delimiter::from_name(n).ok_or_else(|| {
            bad(format!(
                "unknown delimiter '{n}'. Use auto, comma, semicolon, tab, pipe or space"
            ))
        }),
    }
}

/// Meters per unit for a unit name.
fn unit_m(m: &Map<String, Value>, key: &str, default: f64) -> Result<f64, RpcError> {
    let Some(v) = m.get(key).filter(|v| !v.is_null()) else {
        return Ok(default);
    };
    if let Some(n) = v.as_f64() {
        return if n.is_finite() && n > 0.0 {
            Ok(n)
        } else {
            Err(bad(format!("{key} must be a positive number")))
        };
    }
    let name = v.as_str().unwrap_or("").trim().to_ascii_lowercase();
    Ok(match name.as_str() {
        "m" | "meter" | "meters" | "metre" => 1.0,
        "mm" => 0.001,
        "cm" => 0.01,
        "km" => 1000.0,
        "ft" | "foot" | "feet" => 0.3048,
        "usft" | "us_ft" | "survey_ft" => 1200.0 / 3937.0,
        "in" | "inch" | "inches" => 0.0254,
        "yd" | "yard" => 0.9144,
        _ => {
            return Err(bad(format!(
                "unknown unit '{name}'. Use m, mm, cm, km, ft, usft, inch or yd"
            )));
        }
    })
}

fn layer_class(
    s: &Session,
    m: &Map<String, Value>,
) -> Result<(Option<LayerId>, Option<ClassId>), RpcError> {
    let doc = s.editor.doc();
    let layer = match m.get("layer").filter(|v| !v.is_null()) {
        Some(v) => Some(crate::sheets_api::layer_ref(doc, v)?),
        None => None,
    };
    let class = match m.get("class").filter(|v| !v.is_null()) {
        Some(v) => Some(crate::sheets_api::class_ref(doc, v)?),
        None => None,
    };
    Ok((layer, class))
}

fn georef_args(m: &Map<String, Value>) -> (String, bool) {
    (
        m.get("crs")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        m.get("shift").and_then(Value::as_bool).unwrap_or(true),
    )
}

fn skipped_json(v: &[(usize, String)]) -> Value {
    Value::Array(
        v.iter()
            .take(200)
            .map(|(line, reason)| json!({"line": line, "reason": reason}))
            .collect(),
    )
}

fn import_points(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let text = text_of(s, m)?;
    let mut opts = PointOptions {
        delimiter: delimiter_of(m)?,
        ..PointOptions::default()
    };
    if let Some(cols) = m.get("columns").filter(|v| !v.is_null()) {
        let list: Vec<String> = match cols {
            Value::String(t) => t
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect(),
            Value::Array(a) => a
                .iter()
                .map(|x| x.as_str().unwrap_or("").to_string())
                .collect(),
            _ => return Err(bad("columns must be a list of names")),
        };
        opts.columns = list
            .iter()
            .map(|n| Column::from_name(n).ok_or_else(|| bad(format!("unknown column '{n}'"))))
            .collect::<Result<_, _>>()?;
        opts.columns_given = true;
    }
    opts.skip_lines = m
        .get("skip_lines")
        .filter(|v| !v.is_null())
        .map(|v| {
            v.as_u64()
                .map(|n| n as usize)
                .ok_or_else(|| bad("skip_lines must be a whole number"))
        })
        .transpose()?;
    opts.xy_unit_m = unit_m(m, "unit", 1.0)?;
    opts.z_unit_m = unit_m(m, "z_unit", opts.xy_unit_m)?;
    let parsed = survey::parse_points(&text, &opts).map_err(bad)?;
    if parsed.rows.is_empty() {
        return Err(RpcError::validation(format!(
            "no survey points found ({} lines skipped). Check the column order and the delimiter",
            parsed.skipped.len()
        )));
    }
    let (layer, class) = layer_class(s, m)?;
    let (crs, shift) = georef_args(m);
    let doc = s.editor.doc();
    let (positions, new_georef) =
        survey::place_points(&parsed.rows, doc.georef.as_ref(), &crs, shift);
    let mut cmds = Vec::new();
    if let Some(g) = &new_georef {
        cmds.push(Command::SetGeoreference {
            georef: Some(g.clone()),
        });
    }
    let first = doc.next_id();
    let mut id = first;
    let mut n = 0usize;
    for (row, p) in parsed.rows.iter().zip(&positions) {
        let shape = Shape::Locus3d { p: *p };
        if !shape.is_finite() || !shape.is_in_range() {
            continue;
        }
        cmds.push(Command::AddObject {
            shape,
            layer,
            class,
            attrs: Attributes::default(),
        });
        let mut set = std::collections::BTreeMap::new();
        set.insert("point_id".to_string(), json!(row.id));
        set.insert("code".to_string(), json!(row.code));
        if !row.description.is_empty() {
            set.insert("description".to_string(), json!(row.description));
        }
        cmds.push(Command::SetData {
            id: ObjectId(id),
            set,
            remove: Vec::new(),
        });
        id += 1;
        n += 1;
    }
    if n == 0 {
        return Err(RpcError::validation("all points are out of range"));
    }
    s.editor
        .execute(&Command::Batch { commands: cmds })
        .map_err(RpcError::from)?;
    s.prune_selection();
    let mut skipped = parsed.skipped;
    skipped.extend(std::iter::repeat_n(
        (0, "out of range".to_string()),
        parsed.rows.len() - n,
    ));
    Ok(json!({
        "imported": n,
        "skipped": skipped_json(&skipped),
        "skipped_count": skipped.len(),
        "georeference": new_georef,
        "ids_from": first,
        "ids_to": id - 1,
    }))
}

fn import_trees(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let text = text_of(s, m)?;
    let xy = unit_m(m, "unit", 1.0)?;
    let opts = TreeOptions {
        delimiter: delimiter_of(m)?,
        xy_unit_m: xy,
        dbh_unit_m: unit_m(m, "dbh_unit", 0.01)?,
        size_unit_m: unit_m(m, "size_unit", 1.0)?,
        ..TreeOptions::default()
    };
    let parsed = survey::parse_trees(&text, &opts).map_err(bad)?;
    if parsed.rows.is_empty() {
        return Err(RpcError::validation(format!(
            "no trees found ({} lines skipped)",
            parsed.skipped.len()
        )));
    }
    let (layer, class) = layer_class(s, m)?;
    let (crs, shift) = georef_args(m);
    let doc = s.editor.doc();
    let map: Vec<DVec2> = parsed
        .rows
        .iter()
        .map(|t| DVec2::new(t.easting, t.northing))
        .collect();
    let (positions, new_georef) = survey::place(&map, doc.georef.as_ref(), &crs, shift);
    let mut cmds = Vec::new();
    if let Some(g) = &new_georef {
        cmds.push(Command::SetGeoreference {
            georef: Some(g.clone()),
        });
    }
    let first = doc.next_id();
    let mut id = first;
    let mut n = 0usize;
    let mut bad_rows: Vec<(usize, String)> = Vec::new();
    for (t, p) in parsed.rows.iter().zip(&positions) {
        let shape = Shape::ExistingTree {
            at: *p,
            species: t.species.clone(),
            dbh: t.dbh,
            spread: t.spread,
            height: t.height,
            condition: t.condition,
            action: t.action,
            rpz_factor: None,
            rpz_cap: None,
        };
        if !shape.is_finite() || !shape.is_in_range() || shape.problem().is_some() {
            bad_rows.push((
                0,
                format!("tree '{}': position or size is out of range", t.id),
            ));
            continue;
        }
        cmds.push(Command::AddObject {
            shape,
            layer,
            class,
            attrs: Attributes::default(),
        });
        let mut set = std::collections::BTreeMap::new();
        set.insert("tree_id".to_string(), json!(t.id));
        if !t.note.is_empty() {
            set.insert("note".to_string(), json!(t.note));
        }
        cmds.push(Command::SetData {
            id: ObjectId(id),
            set,
            remove: Vec::new(),
        });
        id += 1;
        n += 1;
    }
    if n == 0 {
        return Err(RpcError::validation("all trees are out of range"));
    }
    s.editor
        .execute(&Command::Batch { commands: cmds })
        .map_err(RpcError::from)?;
    s.prune_selection();
    let mut skipped = parsed.skipped;
    skipped.extend(bad_rows);
    Ok(json!({
        "imported": n,
        "skipped": skipped_json(&skipped),
        "skipped_count": skipped.len(),
        "georeference": new_georef,
        "ids_from": first,
        "ids_to": id - 1,
    }))
}

fn site_sources(
    s: &Session,
    m: &Map<String, Value>,
) -> Result<(ObjectId, Vec<DVec3>, Vec<cadektor_core::site::SiteLine>), RpcError> {
    let id = m
        .get("id")
        .and_then(Value::as_u64)
        .map(ObjectId)
        .ok_or_else(|| bad("give id: the ID of a site model"))?;
    let obj = s
        .editor
        .doc()
        .object(id)
        .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
    match &obj.shape {
        Shape::SiteModel { points, lines, .. } => Ok((id, points.clone(), lines.clone())),
        other => Err(RpcError::validation(format!(
            "object {id} is a {}, not a site model",
            other.kind()
        ))),
    }
}

fn number(m: &Map<String, Value>, key: &str, default: f64) -> Result<f64, RpcError> {
    match m.get(key).filter(|v| !v.is_null()) {
        None => Ok(default),
        Some(v) => v
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(|| bad(format!("{key} must be a number"))),
    }
}

fn source_json(s: Source) -> Value {
    match s {
        Source::Point(i) => json!({"point": i}),
        Source::Vertex { line, vertex } => json!({"line": line, "vertex": vertex}),
    }
}

fn issue_json(i: &Issue) -> Value {
    match i {
        Issue::DuplicateXy {
            xy,
            sources,
            z_min,
            z_max,
        } => json!({
            "kind": i.kind(), "at": [xy.x, xy.y], "z_min": z_min, "z_max": z_max,
            "sources": sources.iter().map(|s| source_json(*s)).collect::<Vec<_>>(),
        }),
        Issue::Spike {
            source,
            xy,
            z,
            expected,
            deviation,
        } => json!({
            "kind": i.kind(), "at": [xy.x, xy.y], "z": z, "expected": expected,
            "deviation": deviation, "source": source_json(*source),
        }),
        Issue::CrossingBreaklines { a, b, xy, z_a, z_b } => json!({
            "kind": i.kind(), "at": [xy.x, xy.y], "z_a": z_a, "z_b": z_b,
            "a": {"line": a.0, "segment": a.1}, "b": {"line": b.0, "segment": b.1},
        }),
    }
}

fn site_check(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let (_, points, lines) = site_sources(s, m)?;
    let d = CheckOptions::default();
    let opts = CheckOptions {
        xy_tolerance: number(m, "xy_tolerance", d.xy_tolerance)?,
        z_tolerance: number(m, "z_tolerance", d.z_tolerance)?,
        spike_threshold: number(m, "spike_threshold", d.spike_threshold)?,
        max_issues: number(m, "max_issues", d.max_issues as f64)?.clamp(0.0, 100_000.0) as usize,
    };
    if !opts.is_valid() {
        return Err(bad(
            "xy_tolerance, z_tolerance and spike_threshold must be positive numbers",
        ));
    }
    let r = check::check(&points, &lines, &opts);
    Ok(json!({
        "clean": r.is_clean(),
        "counts": {
            "duplicate_xy": r.duplicate_xy,
            "spikes": r.spikes,
            "crossing_breaklines": r.crossings,
            "exact_duplicates": r.exact_duplicates,
        },
        "vertices_checked": r.vertices_checked,
        "truncated": r.truncated,
        "issues": r.issues.iter().map(issue_json).collect::<Vec<_>>(),
    }))
}

fn site_simplify(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let (id, points, lines) = site_sources(s, m)?;
    let tol = number(m, "tolerance", f64::NAN)?;
    if !(tol.is_finite() && tol > 0.0) {
        return Err(bad("give tolerance: a positive number in mm"));
    }
    let dry = m.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
    let r = check::simplify(&points, &lines, tol);
    if !dry && (r.removed_duplicates + r.removed_points + r.removed_vertices) > 0 {
        let mut shape = s.editor.doc().object(id).expect("checked").shape.clone();
        if let Shape::SiteModel { points, lines, .. } = &mut shape {
            *points = r.points.clone();
            *lines = r.lines.clone();
        }
        s.editor
            .execute(&Command::ModifyObject {
                id,
                shape: Some(shape),
                layer: None,
                class: None,
                attrs: None,
            })
            .map_err(RpcError::from)?;
    }
    Ok(json!({
        "removed_duplicates": r.removed_duplicates,
        "removed_points": r.removed_points,
        "removed_vertices": r.removed_vertices,
        "points": r.points.len(),
        "lines": r.lines.len(),
        "dry_run": dry,
    }))
}

/// A known point: a model position (from `model` or from an object) and a map position.
fn known_point(s: &Session, v: &Value) -> Result<(DVec2, DVec2), RpcError> {
    let pair = |x: &Value, what: &str| -> Result<DVec2, RpcError> {
        let a = x.as_array().filter(|a| a.len() == 2);
        match a.map(|a| (a[0].as_f64(), a[1].as_f64())) {
            Some((Some(x), Some(y))) if x.is_finite() && y.is_finite() => Ok(DVec2::new(x, y)),
            _ => Err(bad(format!("{what} must be [x, y] with finite numbers"))),
        }
    };
    let model = if let Some(id) = v.get("object").and_then(Value::as_u64) {
        let o = s
            .editor
            .doc()
            .object(ObjectId(id))
            .ok_or_else(|| RpcError::not_found(format!("object {id} does not exist")))?;
        match &o.shape {
            Shape::Locus3d { p } => p.truncate(),
            Shape::Plant { at, .. } | Shape::ExistingTree { at, .. } => *at,
            Shape::Circle { center, .. } => *center,
            other => {
                return Err(bad(format!(
                    "object {id} is a {}: use a point-like object or give model",
                    other.kind()
                )));
            }
        }
    } else {
        pair(v.get("model").unwrap_or(&Value::Null), "model")?
    };
    let map = pair(v.get("map").unwrap_or(&Value::Null), "map")?;
    Ok((model, map))
}

fn survey_point(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let doc = s.editor.doc();
    let current = doc.georef.clone();
    let crs = match m.get("crs").and_then(Value::as_str) {
        Some(c) => c.to_string(),
        None => current.as_ref().map(|g| g.crs.clone()).unwrap_or_default(),
    };
    let user_origin = current.as_ref().map_or(DVec2::ZERO, |g| g.user_origin);
    let first = known_point(s, &Value::Object(m.clone()))?;
    let (g, scale) = if let Some(second) = m.get("second").filter(|v| !v.is_null()) {
        let second = known_point(s, second)?;
        let (g, scale) = Georeference::from_two_points(
            &crs,
            [first.0, second.0],
            [first.1, second.1],
            user_origin,
        )
        .ok_or_else(|| bad("the two known points must be at different places"))?;
        (g, Some(scale))
    } else {
        let rotation = number(m, "rotation", current.as_ref().map_or(0.0, |g| g.rotation))?;
        (
            Georeference::from_known_point(&crs, first.0, first.1, rotation, user_origin),
            None,
        )
    };
    if !g.is_finite() {
        return Err(bad("the numbers must be finite"));
    }
    s.editor
        .execute(&Command::SetGeoreference {
            georef: Some(g.clone()),
        })
        .map_err(RpcError::from)?;
    Ok(json!({"georeference": g, "scale": scale}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &mut Session, method: &str, p: Value) -> Value {
        s.handle(method, p)
            .unwrap_or_else(|e| panic!("{method}: {}", e.message))
    }

    #[test]
    fn point_files_become_loci_with_codes_and_a_georeference() {
        let mut s = Session::default();
        let r = ok(
            &mut s,
            "survey.import_points",
            json!({"text": "P,N,E,Z,Code\n1,6672000,385000,10.5,TREE\n2,6672010,385010,11,KERB\nbad,x\n", "crs": "EPSG:3067"}),
        );
        assert_eq!(r["imported"], 2);
        assert_eq!(r["skipped_count"], 1);
        assert_eq!(r["georeference"]["crs"], "EPSG:3067");
        let o = ok(&mut s, "objects.get", json!({"id": r["ids_from"]}));
        assert_eq!(o["shape"]["kind"], "locus3d");
        assert_eq!(o["data"]["code"], "TREE");
        assert_eq!(o["data"]["point_id"], "1");
        // The geometry stays near the origin: the point is 5 m west of the center.
        assert_eq!(o["shape"]["p"], json!([-5000.0, -5000.0, 10500.0]));
        // One undo step.
        assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
        assert_eq!(ok(&mut s, "document.summary", json!({}))["object_count"], 0);
    }

    #[test]
    fn column_order_delimiter_and_units_are_options() {
        let mut s = Session::default();
        let r = ok(
            &mut s,
            "survey.import_points",
            json!({"text": "10 20 30 oak\n", "columns": "E N Z C", "delimiter": "space", "unit": "ft"}),
        );
        assert_eq!(r["imported"], 1);
        let o = ok(&mut s, "objects.get", json!({"id": r["ids_from"]}));
        let p = o["shape"]["p"].as_array().unwrap();
        assert!((p[0].as_f64().unwrap() - 3048.0).abs() < 1e-6);
        assert!((p[1].as_f64().unwrap() - 6096.0).abs() < 1e-6);
        assert_eq!(o["data"]["code"], "oak");
        for bad_params in [
            json!({}),
            json!({"text": "1,2,3,4,5", "columns": ["Q"]}),
            json!({"text": "1,2,3,4,5", "delimiter": "x"}),
            json!({"text": "1,2,3,4,5", "unit": "parsec"}),
            json!({"text": "a,b,c"}),
            json!({"text": "1,2,3,4,5", "columns": 5}),
            json!({"text": "1,2,3,4,5", "skip_lines": -1}),
        ] {
            assert!(s.handle("survey.import_points", bad_params).is_err());
        }
    }

    #[test]
    fn a_tree_survey_makes_existing_trees() {
        let mut s = Session::default();
        let r = ok(
            &mut s,
            "tree.import_survey",
            json!({"text": "ID;E;N;Species;DBH (cm);Height (m);Spread (m);Condition;Action\nT1;5;7;Oak;50;15;9;good;remove\n"}),
        );
        assert_eq!(r["imported"], 1);
        let rows = ok(&mut s, "report.existing_trees", json!({}));
        assert_eq!(rows["rows"][0]["rpz_radius"], 6000.0);
        let o = ok(&mut s, "objects.get", json!({"id": r["ids_from"]}));
        assert_eq!(o["data"]["tree_id"], "T1");
        assert_eq!(o["shape"]["action"], "remove");
        assert!(s.handle("tree.import_survey", json!({"text": ""})).is_err());
        assert!(
            s.handle("tree.import_survey", json!({"text": "a,b\n1,2"}))
                .is_err()
        );
    }

    fn site_with_problems(s: &mut Session) -> u64 {
        let mut ids = Vec::new();
        for x in 0..6 {
            for y in 0..6 {
                let z = if x == 3 && y == 3 { 6000 } else { 100 };
                let r = ok(
                    s,
                    "execute",
                    json!({"command": {"op": "add_object", "shape": {"kind": "locus3d", "p": [x * 1000, y * 1000, z]}}}),
                );
                ids.push(r["created"][0].as_u64().unwrap());
            }
        }
        let r = ok(
            s,
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "locus3d", "p": [1000, 1000, 900]}}}),
        );
        ids.push(r["created"][0].as_u64().unwrap());
        ok(s, "site.create", json!({"source_ids": ids}))["id"]
            .as_u64()
            .unwrap()
    }

    #[test]
    fn site_check_reports_and_simplify_changes_the_source_in_one_step() {
        let mut s = Session::default();
        let id = site_with_problems(&mut s);
        let c = ok(&mut s, "site.check", json!({"id": id}));
        assert_eq!(c["clean"], false);
        assert_eq!(c["counts"]["duplicate_xy"], 1);
        assert_eq!(c["counts"]["spikes"], 1);
        assert!(
            c["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["kind"] == "spike")
        );
        let dry = ok(
            &mut s,
            "site.simplify",
            json!({"id": id, "tolerance": 50, "dry_run": true}),
        );
        assert!(dry["removed_points"].as_u64().unwrap() > 0);
        let before = ok(&mut s, "site.update", json!({"id": id}))["points"]
            .as_u64()
            .unwrap();
        assert_eq!(before, 36 + 1);
        let r = ok(&mut s, "site.simplify", json!({"id": id, "tolerance": 50}));
        let after = ok(&mut s, "site.update", json!({"id": id}))["points"]
            .as_u64()
            .unwrap();
        assert_eq!(after, r["points"].as_u64().unwrap());
        assert!(after < before);
        assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
        assert_eq!(
            ok(&mut s, "site.update", json!({"id": id}))["points"]
                .as_u64()
                .unwrap(),
            before
        );
        for bad_params in [
            json!({}),
            json!({"id": 9999}),
            json!({"id": id, "tolerance": 0}),
            json!({"id": id, "tolerance": "x"}),
        ] {
            assert!(s.handle("site.simplify", bad_params).is_err());
        }
        assert!(
            s.handle("site.check", json!({"id": id, "z_tolerance": -1}))
                .is_err()
        );
        assert!(s.handle("site.check", json!({"id": 1})).is_err());
    }

    #[test]
    fn the_survey_point_sets_the_origin_and_two_points_set_the_rotation() {
        let mut s = Session::default();
        let r = ok(
            &mut s,
            "georef.survey_point",
            json!({"model": [5000, 2000], "map": [385005, 6672002], "crs": "EPSG:3067"}),
        );
        assert_eq!(r["georeference"]["origin"], json!([385000.0, 6672000.0]));
        let r = ok(
            &mut s,
            "georef.survey_point",
            json!({"model": [0, 0], "map": [100, 200], "second": {"model": [10000, 0], "map": [100, 210]}}),
        );
        assert!(
            (r["georeference"]["rotation"].as_f64().unwrap() - std::f64::consts::FRAC_PI_2).abs()
                < 1e-9
        );
        assert!((r["scale"].as_f64().unwrap() - 1.0).abs() < 1e-9);
        assert_eq!(r["georeference"]["crs"], "EPSG:3067");
        // An object gives the model position.
        let l = ok(
            &mut s,
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "locus3d", "p": [1000, 0, 0]}}}),
        );
        let id = l["created"][0].clone();
        ok(
            &mut s,
            "georef.survey_point",
            json!({"object": id, "map": [1, 1]}),
        );
        for bad_params in [
            json!({}),
            json!({"model": [0, 0]}),
            json!({"model": [0, "x"], "map": [0, 0]}),
            json!({"object": 999, "map": [0, 0]}),
            json!({"model": [0, 0], "map": [0, 0], "second": {"model": [0, 0], "map": [1, 1]}}),
        ] {
            assert!(s.handle("georef.survey_point", bad_params).is_err());
        }
    }

    #[test]
    fn bad_tree_rows_are_skipped_and_headers_decide_the_columns() {
        let mut s = Session::default();
        let r = ok(
            &mut s,
            "tree.import_survey",
            json!({"text": "ID,E,N,DBH (cm),Height (m)\nA,1,2,40,9\nB,1e12,2,40,9\nC,x,2,40,9\nD,3,4,,\n"}),
        );
        assert_eq!(r["imported"], 2);
        assert_eq!(r["skipped_count"], 2);
        let rows = ok(&mut s, "report.existing_trees", json!({}));
        assert_eq!(
            rows["rows"][1]["dbh"], 300.0,
            "a missing size gets the default"
        );
        // A header with Easting before Northing is read by name.
        let r = ok(
            &mut s,
            "survey.import_points",
            json!({"text": "Point,Easting,Northing,Z\n1,100,200,5\n"}),
        );
        let o = ok(&mut s, "objects.get", json!({"id": r["ids_from"]}));
        assert_eq!(o["shape"]["p"], json!([100000.0, 200000.0, 5000.0]));
        // An order that contradicts the header is refused.
        assert!(
            s.handle(
                "survey.import_points",
                json!({"text": "Point,Easting,Northing,Z\n1,100,200,5\n", "columns": ["P", "N", "E", "Z"]})
            )
            .is_err()
        );
    }
}
