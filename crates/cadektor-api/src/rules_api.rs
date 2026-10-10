//! Agent API for display rules: the northing/easting grid, project phases and data
//! visualizations (see `cadektor_core::viewrules`).
//!
//! Methods: `view_rules.get`, `grid.set`, `phase.set`, `phase.display`, `phase.report`,
//! `visualization.set`, `visualization.get`, `visualization.clear`.

use cadektor_core::uid::AsUid;
use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use cadektor_core::command::Command;
use cadektor_core::model::{Color, ObjectId};
use cadektor_core::viewrules::{self, NeGrid, Phase, PhaseStyle, ViewRules, Visualization};

use crate::errors::bad;
use crate::{RpcError, Session};

pub const VIEW_RULES_GET: (&str, &str) = (
    "view_rules.get",
    "The display rules of the document: {ne_grid?, phases?, visualization?}. They change how objects look, not the objects.",
);
pub const GRID_SET: (&str, &str) = (
    "grid.set",
    "Show the northing/easting grid of the georeference (2D plan and SVG). params: {spacing (m, positive; null removes the grid), labels? (default true), label_height? (model mm, 0 = automatic), color? ([r,g,b,a])}. Needs a georeference to draw (set one with georef.survey_point or gis.georef.set). Returns {ne_grid, georeferenced}.",
);
pub const PHASE_SET: (&str, &str) = (
    "phase.set",
    "Set the phase of objects: existing, new (the default) or demolished. params: {ids, phase}. It is a data field (key phase), so reports and data tags can read it (property Phase). One undo step.",
);
pub const PHASE_DISPLAY: (&str, &str) = (
    "phase.display",
    "Display rules for phases. params: {enabled? (false turns the rules off; null removes them), existing?, new?, demolished? ({visible?, pen?, fill?, weight_factor?}: pen and fill are [r,g,b,a] or null; fill [0,0,0,0] is no fill)}. Defaults: existing grey, new as drawn, demolished red without fill. Returns {phases}.",
);
pub const PHASE_REPORT: (&str, &str) = (
    "phase.report",
    "Object counts by phase and kind. Returns {rows: [{phase, kind, count}], totals: {existing, new, demolished}}.",
);
pub const VISUALIZATION_SET: (&str, &str) = (
    "visualization.set",
    "Color objects by criteria (data visualization). An override layer: no object or data changes. params: {name?, field (a property such as Condition, Plant.category, Phase, Area or a record field Soil.Depth), rules: [{criterion: {type: equals, value} | {type: range, min?, max?} | {type: otherwise}, color: [r,g,b,a], label?}] (the first rule that fits wins), target? (fill | pen | both, default fill), kinds? ([shape kinds]; empty = all), include_missing? (objects without the property go to the otherwise rule), enabled?}. Returns {visualization, legend}.",
);
pub const VISUALIZATION_AUTO: (&str, &str) = (
    "visualization.auto",
    "Color by a property with rules made from the values in the document: one color per distinct value (at most 40), or bins equal ranges of a number. params: {field, bins? (2 to 64: ranges of a number, light to dark blue), kinds?, target? (fill | pen | both), name?}. Same result as visualization.set.",
);
pub const VISUALIZATION_GET: (&str, &str) = (
    "visualization.get",
    "The active visualization with its legend: {visualization, legend: [{label, color, count}], unmatched}.",
);
pub const VISUALIZATION_CLEAR: (&str, &str) = (
    "visualization.clear",
    "Remove the visualization. Objects look as before.",
);

pub fn describe() -> Value {
    json!({
        "phases": "Objects have a phase: existing, new (default) or demolished (data key phase). phase.display chooses what each phase looks like or hides it.",
        "ne_grid": "A grid of northing and easting lines from the georeference, in the plan, in SVG output and in sheet viewports.",
        "visualization": "Color by criteria is a display rule. Sheet viewports and exports of other formats ignore it where noted in the feature spec.",
        "examples": [
            {"method": "grid.set", "params": {"spacing": 10}},
            {"method": "phase.set", "params": {"ids": ["00000000-0000-0000-0000-000000000003", "00000000-0000-0000-0000-000000000004"], "phase": "demolished"}},
            {"method": "visualization.set", "params": {"field": "Condition", "rules": [
                {"criterion": {"type": "equals", "value": "good"}, "color": [0, 160, 0, 255]},
                {"criterion": {"type": "otherwise"}, "color": [200, 0, 0, 255], "label": "Needs a look"}]}}
        ],
    })
}

/// Handles a method of this module. `None`: it is not one of ours.
pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    let known = matches!(
        method,
        "view_rules.get"
            | "grid.set"
            | "phase.set"
            | "phase.display"
            | "phase.report"
            | "visualization.set"
            | "visualization.auto"
            | "visualization.get"
            | "visualization.clear"
    );
    if !known {
        return None;
    }
    let p = if p.is_null() { json!({}) } else { p };
    let Some(m) = p.as_object() else {
        return Some(Err(bad("params must be an object")));
    };
    Some(match method {
        "view_rules.get" => Ok(json!(s.editor.doc().drawing.view_rules)),
        "grid.set" => grid_set(s, m),
        "phase.set" => phase_set(s, m),
        "phase.display" => phase_display(s, m),
        "phase.report" => phase_report(s),
        "visualization.set" => visualization_set(s, m),
        "visualization.auto" => visualization_auto(s, m),
        "visualization.get" => Ok(visualization_json(s)),
        _ => {
            let mut rules = s.editor.doc().drawing.view_rules.clone();
            rules.visualization = None;
            set_rules(s, rules).map(|_| json!({"cleared": true}))
        }
    })
}

fn set_rules(s: &mut Session, rules: ViewRules) -> Result<(), RpcError> {
    if rules == s.editor.doc().drawing.view_rules {
        return Ok(());
    }
    s.editor
        .execute(&Command::SetViewRules {
            rules: Box::new(rules),
        })
        .map(|_| ())
        .map_err(RpcError::from)
}

fn color_of(v: &Value, what: &str) -> Result<Color, RpcError> {
    let a = v
        .as_array()
        .filter(|a| a.len() == 4 || a.len() == 3)
        .ok_or_else(|| bad(format!("{what} must be [r, g, b, a]")))?;
    let mut c = [0u8, 0, 0, 255];
    for (i, x) in a.iter().enumerate() {
        c[i] = x.as_u64().filter(|n| *n <= 255).ok_or_else(|| {
            bad(format!(
                "{what}: each value is a whole number from 0 to 255"
            ))
        })? as u8;
    }
    Ok(Color(c))
}

fn grid_set(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let mut rules = s.editor.doc().drawing.view_rules.clone();
    match m.get("spacing") {
        None => return Err(bad("give spacing in meters, or null to remove the grid")),
        Some(Value::Null) => rules.ne_grid = None,
        Some(v) => {
            let spacing = v
                .as_f64()
                .filter(|x| x.is_finite() && *x > 0.0)
                .ok_or_else(|| bad("spacing must be a positive number (meters)"))?;
            let mut g = rules.ne_grid.unwrap_or(NeGrid::new(spacing));
            g.spacing = spacing;
            if let Some(l) = m.get("labels") {
                g.labels = l
                    .as_bool()
                    .ok_or_else(|| bad("labels must be true or false"))?;
            }
            if let Some(h) = m.get("label_height") {
                g.label_height = h
                    .as_f64()
                    .filter(|x| x.is_finite() && *x >= 0.0)
                    .ok_or_else(|| bad("label_height must be a number of 0 or more"))?;
            }
            if let Some(c) = m.get("color").filter(|c| !c.is_null()) {
                g.color = color_of(c, "color")?;
            }
            rules.ne_grid = Some(g);
        }
    }
    set_rules(s, rules)?;
    let doc = s.editor.doc();
    Ok(json!({
        "ne_grid": doc.drawing.view_rules.ne_grid,
        "georeferenced": doc.georef.is_some(),
    }))
}

fn phase_set(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let name = m
        .get("phase")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("give phase: existing, new or demolished"))?;
    let phase = Phase::from_name(name).ok_or_else(|| {
        bad(format!(
            "unknown phase '{name}'. Use existing, new or demolished"
        ))
    })?;
    let ids: Vec<ObjectId> = m
        .get("ids")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("give ids: a list of object IDs"))?
        .iter()
        .map(|v| {
            v.as_uid()
                .map(ObjectId)
                .ok_or_else(|| bad("ids are whole numbers"))
        })
        .collect::<Result<_, _>>()?;
    if ids.is_empty() {
        return Err(bad("give at least one ID"));
    }
    let cmds: Vec<Command> = ids
        .iter()
        .map(|id| Command::SetData {
            id: *id,
            set: BTreeMap::from([(viewrules::PHASE_KEY.to_string(), json!(phase.id()))]),
            remove: Vec::new(),
        })
        .collect();
    s.editor
        .execute(&Command::Batch { commands: cmds })
        .map_err(RpcError::from)?;
    Ok(json!({"phase": phase.id(), "count": ids.len()}))
}

fn style_patch(base: PhaseStyle, v: &Value, what: &str) -> Result<PhaseStyle, RpcError> {
    let o = v
        .as_object()
        .ok_or_else(|| bad(format!("{what} must be an object")))?;
    let mut st = base;
    if let Some(x) = o.get("visible") {
        st.visible = x
            .as_bool()
            .ok_or_else(|| bad("visible must be true or false"))?;
    }
    if let Some(x) = o.get("pen") {
        st.pen = if x.is_null() {
            None
        } else {
            Some(color_of(x, "pen")?)
        };
    }
    if let Some(x) = o.get("fill") {
        st.fill = if x.is_null() {
            None
        } else {
            Some(color_of(x, "fill")?)
        };
    }
    if let Some(x) = o.get("weight_factor") {
        st.weight_factor = if x.is_null() {
            None
        } else {
            Some(
                x.as_f64()
                    .filter(|f| f.is_finite() && (0.0..=100.0).contains(f))
                    .ok_or_else(|| bad("weight_factor must be a number from 0 to 100"))?,
            )
        };
    }
    Ok(st)
}

fn phase_display(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let mut rules = s.editor.doc().drawing.view_rules.clone();
    if m.get("enabled").is_some_and(Value::is_null) {
        rules.phases = None;
    } else {
        let mut d = rules.phases.unwrap_or_default();
        if let Some(e) = m.get("enabled") {
            d.enabled = e
                .as_bool()
                .ok_or_else(|| bad("enabled must be true or false"))?;
        }
        for (key, slot) in [
            ("existing", &mut d.existing),
            ("new", &mut d.new),
            ("demolished", &mut d.demolished),
        ] {
            if let Some(v) = m.get(key) {
                *slot = style_patch(*slot, v, key)?;
            }
        }
        rules.phases = Some(d);
    }
    set_rules(s, rules)?;
    Ok(json!({"phases": s.editor.doc().drawing.view_rules.phases}))
}

fn phase_report(s: &Session) -> Result<Value, RpcError> {
    let doc = s.editor.doc();
    let mut rows: BTreeMap<(&'static str, &'static str), usize> = BTreeMap::new();
    let mut totals = [0usize; 3];
    for o in doc.objects.values() {
        let p = viewrules::phase_of(o);
        *rows.entry((p.id(), o.shape.kind())).or_default() += 1;
        totals[Phase::ALL.iter().position(|x| *x == p).unwrap_or(1)] += 1;
    }
    Ok(json!({
        "rows": rows.iter().map(|((p, k), n)| json!({"phase": p, "kind": k, "count": n})).collect::<Vec<_>>(),
        "totals": {"existing": totals[0], "new": totals[1], "demolished": totals[2]},
    }))
}

fn visualization_set(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let mut v = serde_json::Map::new();
    v.insert(
        "name".into(),
        m.get("name").cloned().unwrap_or_else(|| {
            m.get("field")
                .cloned()
                .unwrap_or_else(|| json!("Visualization"))
        }),
    );
    for key in [
        "field",
        "rules",
        "target",
        "enabled",
        "kinds",
        "include_missing",
    ] {
        if let Some(x) = m.get(key).filter(|x| !x.is_null()) {
            v.insert(key.into(), x.clone());
        }
    }
    let viz: Visualization =
        serde_json::from_value(Value::Object(v)).map_err(|e| bad(e.to_string()))?;
    let mut rules = s.editor.doc().drawing.view_rules.clone();
    rules.visualization = Some(viz);
    set_rules(s, rules)?;
    Ok(visualization_json(s))
}

fn visualization_auto(s: &mut Session, m: &Map<String, Value>) -> Result<Value, RpcError> {
    let field = m
        .get("field")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("give field"))?;
    let kinds: Vec<String> = match m.get("kinds").filter(|v| !v.is_null()) {
        None => Vec::new(),
        Some(v) => {
            serde_json::from_value(v.clone()).map_err(|_| bad("kinds must be a list of text"))?
        }
    };
    let bins = match m.get("bins").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => Some(
            v.as_u64()
                .ok_or_else(|| bad("bins must be a whole number"))? as usize,
        ),
    };
    let rules = viewrules::auto_rules(s.editor.doc(), field, &kinds, bins).map_err(bad)?;
    let mut set = Map::new();
    set.insert("field".into(), json!(field));
    set.insert("rules".into(), serde_json::to_value(&rules).expect("JSON"));
    set.insert("kinds".into(), json!(kinds));
    for key in ["target", "name"] {
        if let Some(v) = m.get(key).filter(|v| !v.is_null()) {
            set.insert(key.into(), v.clone());
        }
    }
    visualization_set(s, &set)
}

fn visualization_json(s: &Session) -> Value {
    let doc = s.editor.doc();
    match &doc.drawing.view_rules.visualization {
        None => json!({"visualization": null, "legend": [], "unmatched": 0}),
        Some(v) => {
            let (rows, none) = viewrules::legend(doc, v);
            json!({
                "visualization": v,
                "legend": rows.iter().map(|r| json!({"label": r.label, "color": r.color, "count": r.count})).collect::<Vec<_>>(),
                "unmatched": none,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &mut Session, method: &str, p: Value) -> Value {
        s.handle(method, p)
            .unwrap_or_else(|e| panic!("{method}: {}", e.message))
    }

    fn tree(s: &mut Session, x: i64, condition: &str) -> cadektor_core::uid::Uid {
        ok(
            s,
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "existing_tree", "at": [x, 0], "dbh": 300, "spread": 4000, "height": 8000, "condition": condition}}}),
        )["created"][0]
            .as_uid()
            .unwrap()
    }

    #[test]
    fn the_grid_needs_a_georeference_and_shows_in_the_svg() {
        let mut s = Session::with_sequential_ids();
        let r = ok(&mut s, "grid.set", json!({"spacing": 10}));
        assert_eq!(r["georeferenced"], false);
        assert!(
            !ok(&mut s, "render.svg", json!({}))["svg"]
                .as_str()
                .unwrap_or("")
                .contains("ne_grid")
        );
        ok(
            &mut s,
            "georef.survey_point",
            json!({"model": [0, 0], "map": [385000, 6672000], "crs": "EPSG:3067"}),
        );
        tree(&mut s, 0, "good");
        tree(&mut s, 25_000, "good");
        let svg = ok(&mut s, "render.svg", json!({}));
        let svg = svg["svg"]
            .as_str()
            .or_else(|| svg.as_str())
            .unwrap()
            .to_string();
        assert!(svg.contains(r#"data-kind="ne_grid""#), "{svg}");
        assert!(svg.contains("E 385010"));
        let r = ok(
            &mut s,
            "grid.set",
            json!({"spacing": 5, "labels": false, "color": [1, 2, 3, 255]}),
        );
        assert_eq!(r["ne_grid"]["spacing"], 5.0);
        assert_eq!(r["ne_grid"]["labels"], false);
        ok(&mut s, "grid.set", json!({"spacing": null}));
        assert!(
            ok(&mut s, "view_rules.get", json!({}))
                .get("ne_grid")
                .is_none()
        );
        for bad_params in [
            json!({}),
            json!({"spacing": 0}),
            json!({"spacing": "x"}),
            json!({"spacing": 5, "labels": 3}),
            json!({"spacing": 5, "label_height": -1}),
            json!({"spacing": 5, "color": [1, 2]}),
            json!({"spacing": 5, "color": [1, 2, 3, 999]}),
        ] {
            assert!(s.handle("grid.set", bad_params).is_err());
        }
    }

    #[test]
    fn phases_are_a_data_field_with_display_rules_and_a_report() {
        let mut s = Session::with_sequential_ids();
        let a = tree(&mut s, 0, "good");
        let b = tree(&mut s, 5000, "good");
        ok(
            &mut s,
            "phase.set",
            json!({"ids": [a], "phase": "Existing"}),
        );
        ok(
            &mut s,
            "phase.set",
            json!({"ids": [b], "phase": "demolished"}),
        );
        let rep = ok(&mut s, "phase.report", json!({}));
        assert_eq!(
            rep["totals"],
            json!({"existing": 1, "new": 0, "demolished": 1})
        );
        assert_eq!(
            ok(&mut s, "objects.get", json!({"id": b}))["data"]["phase"],
            "demolished"
        );
        let d = ok(
            &mut s,
            "phase.display",
            json!({"demolished": {"visible": false}}),
        );
        assert_eq!(d["phases"]["demolished"]["visible"], false);
        let svg = ok(&mut s, "render.svg", json!({}));
        let svg = svg["svg"]
            .as_str()
            .or_else(|| svg.as_str())
            .unwrap()
            .to_string();
        assert!(svg.contains(&format!(r#"data-id="{a}""#)));
        assert!(
            !svg.contains(&format!(r#"data-id="{b}""#)),
            "demolished is hidden"
        );
        ok(
            &mut s,
            "phase.display",
            json!({"demolished": {"visible": true, "weight_factor": 2, "pen": null}}),
        );
        ok(&mut s, "phase.display", json!({"enabled": false}));
        ok(&mut s, "phase.display", json!({"enabled": null}));
        assert!(
            ok(&mut s, "view_rules.get", json!({}))
                .get("phases")
                .is_none()
        );
        // The phase is a property.
        let vz = ok(
            &mut s,
            "visualization.set",
            json!({"field": "Phase", "rules": [{"criterion": {"type": "equals", "value": "existing"}, "color": [90, 90, 90, 255]}]}),
        );
        assert_eq!(vz["legend"][0]["count"], 1);
        for bad_params in [
            json!({"ids": [a]}),
            json!({"ids": [a], "phase": "later"}),
            json!({"ids": [], "phase": "new"}),
            json!({"phase": "new"}),
            json!({"ids": ["x"], "phase": "new"}),
            json!({"ids": ["00000000-0000-0000-0000-00000000270f"], "phase": "new"}),
        ] {
            assert!(s.handle("phase.set", bad_params).is_err());
        }
        for bad_params in [
            json!({"enabled": "x"}),
            json!({"new": 5}),
            json!({"new": {"visible": 5}}),
            json!({"new": {"weight_factor": 1000}}),
            json!({"new": {"fill": [1]}}),
        ] {
            assert!(s.handle("phase.display", bad_params).is_err());
        }
    }

    #[test]
    fn a_visualization_colors_the_svg_without_changing_objects() {
        let mut s = Session::with_sequential_ids();
        let good = tree(&mut s, 0, "good");
        let poor = tree(&mut s, 5000, "poor");
        let before = ok(&mut s, "objects.get", json!({"id": poor}));
        let r = ok(
            &mut s,
            "visualization.set",
            json!({"field": "Condition", "target": "both", "rules": [
                {"criterion": {"type": "equals", "value": "good"}, "color": [0, 160, 0, 255]},
                {"criterion": {"type": "otherwise"}, "color": [200, 0, 0, 255], "label": "Needs a look"}]}),
        );
        assert_eq!(r["legend"][1]["label"], "Needs a look");
        assert_eq!(r["legend"][0]["count"], 1);
        assert_eq!(r["unmatched"], 0);
        let svg = ok(&mut s, "render.svg", json!({}));
        let svg = svg["svg"]
            .as_str()
            .or_else(|| svg.as_str())
            .unwrap()
            .to_string();
        assert!(
            svg.contains("rgb(0,160,0)") || svg.contains("#00a000") || svg.contains("0,160,0"),
            "{svg}"
        );
        assert_eq!(ok(&mut s, "objects.get", json!({"id": poor})), before);
        assert_eq!(
            ok(&mut s, "objects.get", json!({"id": good}))["shape"]["condition"],
            "good"
        );
        assert_eq!(
            ok(&mut s, "visualization.get", json!({}))["legend"][1]["count"],
            1
        );
        // One undo step undoes the rules.
        assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
        assert!(ok(&mut s, "visualization.get", json!({}))["visualization"].is_null());
        ok(&mut s, "redo", json!({}));
        ok(&mut s, "visualization.clear", json!({}));
        assert!(ok(&mut s, "visualization.get", json!({}))["visualization"].is_null());
        for bad_params in [
            json!({}),
            json!({"field": "Condition"}),
            json!({"field": "Condition", "rules": []}),
            json!({"field": "Condition", "rules": [{"criterion": {"type": "range", "min": 5, "max": 1}, "color": [1, 2, 3, 255]}]}),
            json!({"field": "Condition", "rules": [{"criterion": {"type": "nope"}, "color": [1, 2, 3, 255]}]}),
            json!({"field": " ", "rules": [{"criterion": {"type": "otherwise"}, "color": [1, 2, 3, 255]}]}),
        ] {
            assert!(s.handle("visualization.set", bad_params).is_err());
        }
    }

    #[test]
    fn visualization_auto_colors_by_value_or_range() {
        let mut s = Session::with_sequential_ids();
        tree(&mut s, 0, "good");
        tree(&mut s, 5000, "poor");
        tree(&mut s, 9000, "poor");
        let r = ok(
            &mut s,
            "visualization.auto",
            json!({"field": "Condition", "target": "both"}),
        );
        assert_eq!(r["legend"].as_array().unwrap().len(), 2);
        assert_eq!(r["legend"][1]["count"], 2);
        assert_eq!(r["visualization"]["target"], "both");
        let r = ok(
            &mut s,
            "visualization.auto",
            json!({"field": "X", "bins": 3, "kinds": ["existing_tree"]}),
        );
        let counts: Vec<u64> = r["legend"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["count"].as_u64().unwrap())
            .collect();
        assert_eq!(counts.iter().sum::<u64>(), 3);
        for bad in [
            json!({}),
            json!({"field": "Nope"}),
            json!({"field": "X", "bins": 1}),
            json!({"field": "X", "bins": "x"}),
            json!({"field": "X", "kinds": 5}),
        ] {
            assert!(s.handle("visualization.auto", bad).is_err());
        }
    }

    #[test]
    fn hiding_a_phase_never_changes_the_terrain_results() {
        let mut s = Session::with_sequential_ids();
        let mut ids = Vec::new();
        for x in 0..5 {
            for y in 0..5 {
                ids.push(
                    ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "locus3d", "p": [x * 5000, y * 5000, 0]}}}))["created"][0]
                        .as_uid()
                        .unwrap(),
                );
            }
        }
        let site = ok(&mut s, "site.create", json!({"source_ids": ids}))["id"]
            .as_uid()
            .unwrap();
        let pad = ok(&mut s, "pad.add", json!({"boundary": [[5000, 5000], [15000, 5000], [15000, 15000], [5000, 15000]], "elevation": 2000}))["id"]
            .as_uid()
            .unwrap();
        let before = ok(&mut s, "site.cut_fill", json!({"id": site}));
        assert!(before["fill_m3"].as_f64().unwrap() > 100.0);
        ok(
            &mut s,
            "phase.set",
            json!({"ids": [pad], "phase": "demolished"}),
        );
        ok(
            &mut s,
            "phase.display",
            json!({"demolished": {"visible": false}}),
        );
        assert_eq!(ok(&mut s, "site.cut_fill", json!({"id": site})), before);
        let svg = ok(&mut s, "render.svg", json!({}));
        let svg = svg["svg"]
            .as_str()
            .or_else(|| svg.as_str())
            .unwrap()
            .to_string();
        assert!(!svg.contains(&format!(r#"data-id="{pad}""#)));
    }
}
