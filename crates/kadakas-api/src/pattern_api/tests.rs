use super::*;
use kadakas_core::uid::AsUid;

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    s.handle(method, params)
        .unwrap_or_else(|e| panic!("{method}: {}", e.message))
}

fn fail(s: &mut Session, method: &str, params: Value) -> (i64, String) {
    let e = s.handle(method, params).unwrap_err();
    (e.code, e.message)
}

fn add(s: &mut Session, shape: Value) -> Value {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape": shape}}),
    )["created"][0]
        .clone()
}

fn rect(w: i64, h: i64) -> Value {
    json!([[0, 0], [w, 0], [w, h], [0, h]])
}

fn style(s: &mut Session, bond: &str) -> Value {
    ok(
        s,
        "hardscape_style.create",
        json!({"name": "Pavers", "thickness": 80,
            "joints": {"kind": bond, "unit_length": 200, "unit_width": 100, "spacing": 5}}),
    )["id"]
        .clone()
}

#[test]
fn layout_of_a_region_with_a_hole_and_paging() {
    let mut s = Session::with_sequential_ids();
    let r = ok(
        &mut s,
        "pattern.layout",
        json!({"region": {"outline": rect(2050, 1050),
            "holes": [[[405, 210], [815, 210], [815, 630], [405, 630]]]},
            "pattern": {"bond": "stack", "unit_length": 200, "unit_width": 100, "gap": 5},
            "origin": [0, 0], "limit": 10}),
    );
    let sum = &r["summary"];
    assert_eq!(
        sum["order_units"],
        sum["whole_units"].as_u64().unwrap() + sum["cut_units"].as_u64().unwrap()
    );
    // The hole covers 3 x 4 cells. The middle column of 4 units is gone, and the columns on each
    // side lose a few mm and become 8 cut units. 100 - 12 = 88 whole units remain.
    assert_eq!(sum["whole_units"], 88);
    assert_eq!(sum["cut_units"], 8);
    assert_eq!(r["units_total"], 96);
    assert_eq!(r["units"].as_array().unwrap().len(), 10);
    assert_eq!(r["next_offset"], 10);
    assert!(r["gaps_total"].as_u64().unwrap() > 10);
    // The next page continues.
    let r2 = ok(
        &mut s,
        "pattern.layout",
        json!({"region": {"outline": rect(2050, 1050)},
            "pattern": {"bond": "stack", "unit_length": 200, "unit_width": 100, "gap": 5},
            "origin": [0, 0], "include": ["units"], "offset": 95, "limit": 10}),
    );
    assert_eq!(r2["units"].as_array().unwrap().len(), 5);
    assert!(r2["next_offset"].is_null());
    assert!(r2.get("gaps").is_none());
}

#[test]
fn layout_errors_are_clear() {
    let mut s = Session::with_sequential_ids();
    let (c, m) = fail(&mut s, "pattern.layout", json!({}));
    assert_eq!(c, -32602);
    assert!(m.contains("'id' or 'region'"));
    let (_, m) = fail(
        &mut s,
        "pattern.layout",
        json!({"region": {"outline": rect(1000, 1000)}}),
    );
    assert!(m.contains("unit_length"));
    let (_, m) = fail(
        &mut s,
        "pattern.layout",
        json!({"region": {"outline": rect(1000, 1000)}, "pattern": {"unit_length": 200, "unit_width": 100, "bond": "zigzag"}}),
    );
    assert!(m.contains("bond"));
    let (_, m) = fail(
        &mut s,
        "pattern.layout",
        json!({"region": {"outline": rect(1000, 1000)}, "pattern": {"unit_length": 200, "unit_width": 100}, "align_to_edge": 9}),
    );
    assert!(m.contains("edge 9"));
    // More than a million units.
    let (c, m) = fail(
        &mut s,
        "pattern.layout",
        json!({"region": {"outline": rect(200_000, 150_000)}, "pattern": {"unit_length": 200, "unit_width": 100, "gap": 5}}),
    );
    assert_eq!(c, kadakas_core_limit_code());
    assert!(m.contains("units") && m.contains("limit"), "{m}");
    let (_, m) = fail(
        &mut s,
        "pattern.layout",
        json!({"region": {"outline": rect(1000, 1000)}, "pattern": {"unit_length": 200, "unit_width": 100}, "include": ["tiles"]}),
    );
    assert!(m.contains("include"));
    let (_, m) = fail(
        &mut s,
        "pattern.layout",
        json!({"id": "00000000-0000-0000-0000-0000000000ff"}),
    );
    assert!(m.contains("does not exist"));
}

fn kadakas_core_limit_code() -> i64 {
    crate::errors::CODE_LIMIT
}

#[test]
fn hardscape_with_style_origin_edge_and_report() {
    let mut s = Session::with_sequential_ids();
    let st = style(&mut s, "stack");
    let h = ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": rect(2050, 1050), "style": st, "origin": [0, 0],
            "holes": [[[405, 210], [815, 210], [815, 630], [405, 630]]]}),
    )["id"]
        .clone();
    let rep = ok(&mut s, "report.pattern", json!({"waste_percent": 5}));
    let row = &rep["rows"][0];
    assert_eq!(row["id"], h);
    assert_eq!(row["whole_units"], 88);
    assert_eq!(row["cut_units"], 8);
    assert_eq!(row["order_units"], 96);
    assert_eq!(row["order_units_with_waste"], 101); // ceil(96 * 1.05)
    assert_eq!(rep["totals"]["order_units_with_waste"], 101);
    // Without waste_percent there is no waste.
    let rep = ok(&mut s, "report.pattern", json!({}));
    assert!(rep["rows"][0]["order_units_with_waste"].is_null());
    // A shifted origin cuts units at the edges. Area laid + gaps = region minus holes.
    ok(&mut s, "hardscape.set", json!({"id": h, "origin": [50, 0]}));
    let row = ok(&mut s, "report.pattern", json!({}))["rows"][0].clone();
    assert!(row["cut_units"].as_u64().unwrap() > 0);
    let laid = row["area_laid_m2"].as_f64().unwrap() + row["gap_area_m2"].as_f64().unwrap();
    assert!((laid - row["region_area_m2"].as_f64().unwrap()).abs() < 1e-9);
    // Slivers: a share of 1 makes every cut unit a sliver.
    let row = ok(&mut s, "report.pattern", json!({"sliver_share": 1.0}))["rows"][0].clone();
    assert_eq!(row["slivers"], row["cut_units"]);
    // Clearing the origin and the hole; align_to_edge turns the pattern.
    ok(
        &mut s,
        "hardscape.set",
        json!({"id": h, "origin": null, "holes": null}),
    );
    ok(
        &mut s,
        "hardscape.set",
        json!({"id": h, "align_to_edge": 1}),
    );
    let o = ok(&mut s, "objects.get", json!({"id": h}));
    assert_eq!(o["data"]["hardscape"]["angle"], 90.0);
    assert_eq!(o["data"]["hardscape"]["origin"], json!([2050.0, 0.0]));
    assert!(
        o["data"]["hardscape"]["holes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // report.hardscape carries the block, and the holes show in the plan area.
    let rep = ok(&mut s, "report.hardscape", json!({}));
    assert!(rep["rows"][0]["pattern"]["whole_units"].as_u64().unwrap() > 90);
    assert_eq!(rep["rows"][0]["holes"], 0);
    let rep = ok(&mut s, "report.hardscape", json!({"pattern": false}));
    assert!(rep["rows"][0].get("pattern").is_none());
    // A hole outside the outline is refused.
    let (_, m) = fail(
        &mut s,
        "hardscape.set",
        json!({"id": h, "holes": [[[5000, 5000], [5100, 5000], [5100, 5100]]]}),
    );
    assert!(m.contains("not inside"), "{m}");
}

#[test]
fn hardscape_holes_change_the_area_and_survive_moving() {
    let mut s = Session::with_sequential_ids();
    let h = ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": rect(4000, 4000),
            "holes": [[[1000, 1000], [2000, 1000], [2000, 2000], [1000, 2000]]]}),
    )["id"]
        .clone();
    let rep = ok(&mut s, "report.hardscape", json!({}));
    let q = &rep["rows"][0];
    assert!((q["plan_area_m2"].as_f64().unwrap() - 15.0).abs() < 1e-9);
    assert_eq!(q["holes"], 1);
    assert!((q["holes_area_m2"].as_f64().unwrap() - 1.0).abs() < 1e-9);
    // 16 m outline + 4 m round the hole.
    assert!((q["perimeter_m"].as_f64().unwrap() - 20.0).abs() < 1e-6);
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [h], "delta": [100, 200]}}),
    );
    let o = ok(&mut s, "objects.get", json!({"id": h}));
    assert_eq!(
        o["data"]["hardscape"]["holes"][0][0],
        json!([1100.0, 1200.0])
    );
}

#[test]
fn to_objects_makes_a_group_in_one_undo_step() {
    let mut s = Session::with_sequential_ids();
    let st = style(&mut s, "running_bond");
    let h = ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": rect(1025, 420), "style": st, "origin": [0, 0]}),
    )["id"]
        .clone();
    let before = ok(&mut s, "document.summary", json!({}));
    let r = ok(
        &mut s,
        "pattern.to_objects",
        json!({"id": h, "include": ["units", "gaps"], "name": "Paving"}),
    );
    let units = r["units"].as_u64().unwrap();
    assert!(units >= 20 && r["gaps"].as_u64().unwrap() > 0);
    assert_eq!(
        r["objects"].as_u64().unwrap(),
        units + r["gaps"].as_u64().unwrap()
    );
    assert!(r["group"].is_string());
    ok(&mut s, "undo", json!({}));
    let after = ok(&mut s, "document.summary", json!({}));
    assert_eq!(before["object_count"], after["object_count"]);
    assert_eq!(
        after["object_count"], 1,
        "one undo removes the group and all objects"
    );
    // Units only, no group.
    let r = ok(
        &mut s,
        "pattern.to_objects",
        json!({"id": h, "group": false}),
    );
    assert!(r["group"].is_null() && r["gaps"] == 0);
    let (_, m) = fail(
        &mut s,
        "pattern.to_objects",
        json!({"region": {"outline": rect(100_000, 100_000)}, "pattern": {"unit_length": 200, "unit_width": 100}}),
    );
    assert!(m.contains("units"), "{m}");
}

#[test]
fn from_shapes_turns_nested_outlines_into_a_hardscape_with_holes() {
    let mut s = Session::with_sequential_ids();
    let st = style(&mut s, "stack");
    let mut ids = Vec::new();
    for pts in [
        rect(20_000, 10_000),
        json!([[5000, 3000], [8000, 3000], [8000, 6000], [5000, 6000]]),
        // An island inside the island is a hardscape again.
        json!([[6000, 4000], [7000, 4000], [7000, 5000], [6000, 5000]]),
        // A second lot far away.
        json!([[30000, 0], [40000, 0], [40000, 5000], [30000, 5000]]),
    ] {
        ids.push(add(
            &mut s,
            json!({"kind": "polyline", "points": pts, "closed": true}),
        ));
    }
    let dry = ok(
        &mut s,
        "hardscape.from_shapes",
        json!({"ids": ids, "style": st, "dry_run": true}),
    );
    assert_eq!(dry["created"].as_array().unwrap().len(), 3);
    assert_eq!(dry["holes"].as_array().unwrap().len(), 1);
    let r = ok(
        &mut s,
        "hardscape.from_shapes",
        json!({"ids": ids, "style": st}),
    );
    assert_eq!(r["created"][0]["holes"], 1);
    assert_eq!(r["created"].as_array().unwrap().len(), 3);
    let rep = ok(&mut s, "report.hardscape", json!({}));
    assert_eq!(rep["rows"].as_array().unwrap().len(), 3);
    let areas: Vec<f64> = rep["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["plan_area_m2"].as_f64().unwrap())
        .collect();
    assert!(
        areas.iter().any(|a| (a - (200.0 - 9.0)).abs() < 1e-9),
        "{areas:?}"
    );
    // Sources are gone; one undo brings them back.
    let count = |s: &mut Session| {
        ok(s, "document.summary", json!({}))["object_count"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(count(&mut s), 3);
    ok(&mut s, "undo", json!({}));
    assert_eq!(count(&mut s), 4);
    // Everything as its own hardscape, sources kept.
    let r = ok(
        &mut s,
        "hardscape.from_shapes",
        json!({"ids": ids, "holes_from_inner": false, "delete_source": false}),
    );
    assert_eq!(r["created"].as_array().unwrap().len(), 4);
    // Wrong kinds are skipped with a reason.
    let h = r["created"][0]["id"].as_uid().unwrap();
    let r = ok(&mut s, "hardscape.from_shapes", json!({"ids": [h]}));
    assert_eq!(r["created"].as_array().unwrap().len(), 0);
    assert!(r["skipped"][0]["reason"].as_str().unwrap().contains("area"));
    let (_, m) = fail(&mut s, "hardscape.from_shapes", json!({"ids": []}));
    assert!(m.contains("select"));
    let (_, m) = fail(
        &mut s,
        "hardscape.from_shapes",
        json!({"ids": ids, "bogus": 1}),
    );
    assert!(m.contains("bogus"));
}

#[test]
fn any_closed_shape_works_and_csv_has_rows() {
    let mut s = Session::with_sequential_ids();
    let c = add(
        &mut s,
        json!({"kind": "circle", "center": [0, 0], "radius": 1000}),
    );
    let r = ok(
        &mut s,
        "pattern.layout",
        json!({"id": c, "pattern": {"unit_length": 200, "unit_width": 100, "gap": 4, "bond": "herringbone"},
            "include": ["units"], "limit": 1}),
    );
    let area = r["summary"]["region_area_m2"].as_f64().unwrap();
    assert!((area - std::f64::consts::PI).abs() < 0.01);
    assert!(r["summary"]["cut_units"].as_u64().unwrap() > 10);
    let line = add(&mut s, json!({"kind": "line", "a": [0, 0], "b": [100, 0]}));
    let (_, m) = fail(
        &mut s,
        "pattern.layout",
        json!({"id": line, "pattern": {"unit_length": 200, "unit_width": 100}}),
    );
    assert!(m.contains("cannot be filled"), "{m}");
    let st = style(&mut s, "basket_weave");
    ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": rect(1000, 1000), "style": st}),
    );
    let rep = ok(
        &mut s,
        "report.pattern",
        json!({"format": "csv", "waste_percent": 10}),
    );
    let csv = rep["csv"].as_str().unwrap();
    assert!(csv.lines().next().unwrap().contains("Whole units"));
    assert_eq!(csv.lines().count(), 2);
    // An explicit id that is not a pattern object gives an error row.
    let rep = ok(&mut s, "report.pattern", json!({"ids": [line]}));
    assert!(rep["rows"][0]["error"].is_string());
    let (_, m) = fail(&mut s, "report.pattern", json!({"waste_percent": 500}));
    assert!(m.contains("waste_percent"));
    let (_, m) = fail(&mut s, "report.pattern", json!({"sliver_share": 2}));
    assert!(m.contains("sliver_share"));
}
