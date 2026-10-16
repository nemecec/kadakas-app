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
        "unit_pattern.layout",
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
        "unit_pattern.layout",
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
    let (c, m) = fail(&mut s, "unit_pattern.layout", json!({}));
    assert_eq!(c, -32602);
    assert!(m.contains("'id' or 'region'"));
    let (_, m) = fail(
        &mut s,
        "unit_pattern.layout",
        json!({"region": {"outline": rect(1000, 1000)}}),
    );
    assert!(m.contains("unit_length"));
    let (_, m) = fail(
        &mut s,
        "unit_pattern.layout",
        json!({"region": {"outline": rect(1000, 1000)}, "pattern": {"unit_length": 200, "unit_width": 100, "bond": "zigzag"}}),
    );
    assert!(m.contains("bond"));
    let (_, m) = fail(
        &mut s,
        "unit_pattern.layout",
        json!({"region": {"outline": rect(1000, 1000)}, "pattern": {"unit_length": 200, "unit_width": 100}, "align_to_edge": 9}),
    );
    assert!(m.contains("edge 9"));
    // More than a million units.
    let (c, m) = fail(
        &mut s,
        "unit_pattern.layout",
        json!({"region": {"outline": rect(200_000, 150_000)}, "pattern": {"unit_length": 200, "unit_width": 100, "gap": 5}}),
    );
    assert_eq!(c, kadakas_core_limit_code());
    assert!(m.contains("units") && m.contains("limit"), "{m}");
    let (_, m) = fail(
        &mut s,
        "unit_pattern.layout",
        json!({"region": {"outline": rect(1000, 1000)}, "pattern": {"unit_length": 200, "unit_width": 100}, "include": ["tiles"]}),
    );
    assert!(m.contains("include"));
    let (_, m) = fail(
        &mut s,
        "unit_pattern.layout",
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
    let rep = ok(&mut s, "report.unit_pattern", json!({"waste_percent": 5}));
    let row = &rep["rows"][0];
    assert_eq!(row["id"], h);
    assert_eq!(row["whole_units"], 88);
    assert_eq!(row["cut_units"], 8);
    assert_eq!(row["order_units"], 96);
    assert_eq!(row["order_units_with_waste"], 101); // ceil(96 * 1.05)
    assert_eq!(rep["totals"]["order_units_with_waste"], 101);
    // Without waste_percent there is no waste.
    let rep = ok(&mut s, "report.unit_pattern", json!({}));
    assert!(rep["rows"][0]["order_units_with_waste"].is_null());
    // A shifted origin cuts units at the edges. Area laid + gaps = region minus holes.
    ok(&mut s, "hardscape.set", json!({"id": h, "origin": [50, 0]}));
    let row = ok(&mut s, "report.unit_pattern", json!({}))["rows"][0].clone();
    assert!(row["cut_units"].as_u64().unwrap() > 0);
    let laid = row["area_laid_m2"].as_f64().unwrap() + row["gap_area_m2"].as_f64().unwrap();
    assert!((laid - row["region_area_m2"].as_f64().unwrap()).abs() < 1e-9);
    // Slivers: a share of 1 makes every cut unit a sliver.
    let row = ok(&mut s, "report.unit_pattern", json!({"sliver_share": 1.0}))["rows"][0].clone();
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
        "unit_pattern.to_objects",
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
        "unit_pattern.to_objects",
        json!({"id": h, "group": false}),
    );
    assert!(r["group"].is_null() && r["gaps"] == 0);
    let (_, m) = fail(
        &mut s,
        "unit_pattern.to_objects",
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
        json!({"ids": ids, "style": st, "delete_source": true}),
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
    // Without holes, shapes inside others would make hardscapes that overlap: they are left out.
    let r = ok(
        &mut s,
        "hardscape.from_shapes",
        json!({"ids": ids, "holes_from_inner": false}),
    );
    assert_eq!(r["created"].as_array().unwrap().len(), 2);
    assert_eq!(r["skipped"].as_array().unwrap().len(), 2);
    assert!(
        r["skipped"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("overlaps")
    );
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
        "unit_pattern.layout",
        json!({"id": c, "pattern": {"unit_length": 200, "unit_width": 100, "gap": 4, "bond": "herringbone"},
            "include": ["units"], "limit": 1}),
    );
    let area = r["summary"]["region_area_m2"].as_f64().unwrap();
    assert!((area - std::f64::consts::PI).abs() < 0.01);
    assert!(r["summary"]["cut_units"].as_u64().unwrap() > 10);
    let line = add(&mut s, json!({"kind": "line", "a": [0, 0], "b": [100, 0]}));
    let (_, m) = fail(
        &mut s,
        "unit_pattern.layout",
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
        "report.unit_pattern",
        json!({"format": "csv", "waste_percent": 10}),
    );
    let csv = rep["csv"].as_str().unwrap();
    assert!(csv.lines().next().unwrap().contains("Whole units"));
    assert_eq!(csv.lines().count(), 2);
    // An explicit id that is not a pattern object gives an error row.
    let rep = ok(&mut s, "report.unit_pattern", json!({"ids": [line]}));
    assert!(rep["rows"][0]["error"].is_string());
    let (_, m) = fail(&mut s, "report.unit_pattern", json!({"waste_percent": 500}));
    assert!(m.contains("waste_percent"));
    let (_, m) = fail(&mut s, "report.unit_pattern", json!({"sliver_share": 2}));
    assert!(m.contains("sliver_share"));
}

#[test]
fn presets_and_modules_work_as_patterns_and_in_styles() {
    let mut s = Session::with_sequential_ids();
    let hex = ok(
        &mut s,
        "unit_pattern.module_preset",
        json!({"preset": "hexagon", "across_flats": 200}),
    );
    let pattern = hex["pattern"].clone();
    assert_eq!(pattern["kind"], "module");
    let region = json!({"outline": rect(3000, 2000)});
    let r = ok(
        &mut s,
        "unit_pattern.layout",
        json!({"region": region, "pattern": {"kind": "module", "units": pattern["units"], "a": pattern["a"], "b": pattern["b"], "gap": 4}, "include": ["units"], "limit": 2}),
    );
    assert_eq!(r["summary"]["bond"], "module");
    assert!(
        r["summary"]["unit_length_mm"].is_null()
            && r["summary"]["unit_area_m2"].as_f64().unwrap() > 0.03
    );
    assert_eq!(r["summary"]["by_unit"][0]["name"], "hexagon");
    assert_eq!(r["units"][0]["kind"], "hexagon");
    // A preset inside the pattern, with its numbers.
    let r = ok(
        &mut s,
        "unit_pattern.layout",
        json!({"region": region, "pattern": {"preset": "i_shape", "length": 200, "width": 165, "notch": 25, "gap": 4}, "include": ["gaps"], "limit": 1}),
    );
    assert!(r["gaps_total"].as_u64().unwrap() > 50);
    // Presets: errors.
    for (params, word) in [
        (json!({}), "preset"),
        (json!({"preset": "pentagon"}), "no preset"),
        (json!({"preset": "hexagon"}), "needs"),
        (
            json!({"preset": "hexagon", "side": 100, "across_flats": 100}),
            "not both",
        ),
        (
            json!({"preset": "hexagon", "side": 100, "bogus": 1}),
            "bogus",
        ),
        (json!({"preset": "i_shape", "notch": 500}), "notch"),
    ] {
        let (_, m) = fail(&mut s, "unit_pattern.module_preset", params);
        assert!(m.contains(word), "{m}");
    }
    ok(
        &mut s,
        "unit_pattern.module_preset",
        json!({"preset": "octagon_square", "side": 100}),
    );
    // Module errors in a pattern.
    for (pat, word) in [
        (json!({"kind": "module"}), "units"),
        (
            json!({"kind": "module", "units": [], "a": [1, 0], "b": [0, 1]}),
            "at least one",
        ),
        (
            json!({"kind": "module", "units": pattern["units"], "a": [100]}),
            "[x, y]",
        ),
        (
            json!({"kind": "module", "units": pattern["units"], "b": [0, 100]}),
            "'a'",
        ),
        (json!({"preset": "nope"}), "no preset"),
    ] {
        let (_, m) = fail(
            &mut s,
            "unit_pattern.layout",
            json!({"region": region, "pattern": pat}),
        );
        assert!(m.contains(word), "{pat}: {m}");
    }
    // A module as the joints of a style.
    ok(
        &mut s,
        "hardscape_style.create",
        json!({"name": "Hex", "joints": {"kind": "module", "units": pattern["units"], "a": pattern["a"], "b": pattern["b"], "spacing": 4}}),
    );
    let h = ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": rect(3000, 2000), "style": "Hex"}),
    )["id"]
        .clone();
    let rep = ok(&mut s, "report.unit_pattern", json!({"ids": [h]}));
    assert_eq!(rep["rows"][0]["bond"], "module");
    assert!(rep["totals"]["by_unit"][0]["whole_units"].as_u64().unwrap() > 50);
    // A module that overlaps itself is refused when the style is made.
    let (_, m) = fail(
        &mut s,
        "hardscape_style.create",
        json!({"name": "Bad", "joints": {"kind": "module", "units": pattern["units"], "a": [10, 0], "b": [0, 10], "spacing": 0}}),
    );
    assert!(
        m.contains("larger than the lattice cell") || m.contains("overlap"),
        "{m}"
    );
}

#[test]
fn modules_from_shapes_and_from_a_block() {
    let mut s = Session::with_sequential_ids();
    let a = add(
        &mut s,
        json!({"kind": "polyline", "closed": true, "points": rect(100, 100)}),
    );
    let b = add(
        &mut s,
        json!({"kind": "rect", "min": [100, 0], "max": [200, 100]}),
    );
    let m = ok(
        &mut s,
        "unit_pattern.module_from_shapes",
        json!({"ids": [a, b], "names": ["x", "y"]}),
    );
    assert_eq!(m["pattern"]["units"][1]["name"], "y");
    assert!(m["note"].as_str().unwrap().contains("stack"));
    // The selection is the default.
    s.selection = vec![a.as_uid().map(ObjectId).unwrap()];
    let m = ok(
        &mut s,
        "unit_pattern.module_from_shapes",
        json!({"a": [150, 0], "b": [0, 150]}),
    );
    assert!(m["note"].is_null());
    for (params, word) in [
        (json!({"ids": []}), "select"),
        (json!({"ids": [a, b], "names": ["one"]}), "one name"),
        (json!({"ids": [a], "a": [1, 0]}), "both"),
        (
            json!({"ids": ["00000000-0000-0000-0000-0000000000ff"]}),
            "does not exist",
        ),
    ] {
        let (_, msg) = fail(&mut s, "unit_pattern.module_from_shapes", params);
        assert!(msg.contains(word), "{msg}");
    }
    // A block of a file.
    let dir = std::env::temp_dir().join(format!("kadakas-block-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("m.dxf");
    let dxf = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n8\n0\n2\nM\n70\n0\n10\n0\n20\n0\n0\nLWPOLYLINE\n8\nONE\n90\n4\n70\n1\n10\n0\n20\n0\n10\n100\n20\n0\n10\n100\n20\n100\n10\n0\n20\n100\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n";
    std::fs::write(&path, dxf).unwrap();
    let m = ok(
        &mut s,
        "unit_pattern.module_from_block",
        json!({"path": path, "block": "M"}),
    );
    assert_eq!(m["pattern"]["units"][0]["name"], "ONE");
    for (params, word) in [
        (json!({"block": "M"}), "path"),
        (json!({"path": path}), "block"),
        (json!({"path": path, "block": "Z"}), "no block"),
        (json!({"path": path, "block": "M", "extra": 1}), "extra"),
        (
            json!({"path": dir.join("missing.dxf"), "block": "M"}),
            "missing",
        ),
    ] {
        let (_, msg) = fail(&mut s, "unit_pattern.module_from_block", params);
        assert!(msg.contains(word), "{msg}");
    }
    // A limit of the session on the size of the file.
    s.set_limits(crate::SessionLimits {
        import_text_bytes: 10,
        ..Default::default()
    });
    let (code, msg) = fail(
        &mut s,
        "unit_pattern.module_from_block",
        json!({"path": path, "block": "M"}),
    );
    assert_eq!(code, kadakas_core_limit_code());
    assert!(msg.contains("limit"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_limits_bound_the_pattern_work_and_errors_are_rows() {
    let mut s = Session::with_sequential_ids();
    ok(
        &mut s,
        "hardscape_style.create",
        json!({"name": "Big", "joints": {"kind": "stack", "unit_length": 200, "unit_width": 100, "spacing": 5}}),
    );
    let h = ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": rect(20000, 10000), "style": "Big"}),
    )["id"]
        .clone();
    s.set_limits(crate::SessionLimits {
        pattern_units: 1000,
        ..Default::default()
    });
    let rep = ok(&mut s, "report.unit_pattern", json!({}));
    assert!(rep["rows"][0]["error"].as_str().unwrap().contains("limit"));
    assert_eq!(rep["totals"]["complete"], false);
    let hs = ok(&mut s, "report.hardscape", json!({}));
    assert!(hs["rows"][0]["pattern"]["error"].is_string());
    assert_eq!(hs["totals"]["pattern"]["errors"], 1);
    let (c, _) = fail(&mut s, "unit_pattern.layout", json!({"id": h}));
    assert_eq!(c, kadakas_core_limit_code());
    // The strict limits are lower than the default ones.
    assert!(
        crate::SessionLimits::strict().pattern_units
            < crate::SessionLimits::default().pattern_units
    );
    // report.hardscape: options are checked.
    let (_, m) = fail(&mut s, "report.hardscape", json!({"sliver_share": 5}));
    assert!(m.contains("sliver_share"));
    // visible_only, the options of the count, csv.
    s.set_limits(Default::default());
    let rep = ok(
        &mut s,
        "report.unit_pattern",
        json!({"visible_only": true, "trim": 0, "min_width": 50, "unfillable_share": 0.1, "exclude_unfillable": true, "format": "csv"}),
    );
    assert_eq!(rep["totals"]["complete"], true);
    for (params, word) in [
        (json!({"trim": 500}), "trim"),
        (json!({"min_width": -1}), "min_width"),
        (json!({"unfillable_share": 2}), "unfillable_share"),
        (json!({"gap_length": "yes"}), "gap_length"),
        (json!({"visible_only": 3}), "visible_only"),
    ] {
        let (_, m) = fail(&mut s, "report.unit_pattern", params);
        assert!(m.contains(word), "{m}");
    }
    let oct = ok(
        &mut s,
        "unit_pattern.module_preset",
        json!({"preset": "octagon_square", "side": 100}),
    )["pattern"]
        .clone();
    let h = ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": rect(3000, 2000)}),
    )["id"]
        .clone();
    let rep = ok(
        &mut s,
        "report.unit_pattern",
        json!({"ids": [h], "pattern": {"kind": "module", "units": oct["units"], "a": oct["a"], "b": oct["b"], "gap": 3}, "format": "csv"}),
    );
    let csv = rep["csv"].as_str().unwrap();
    assert!(
        csv.contains("octagon") && csv.contains("square") && csv.contains("all"),
        "{csv}"
    );
}

#[test]
fn totals_never_add_different_products_together() {
    let mut s = Session::with_sequential_ids();
    for (name, l, w) in [("Small", 200, 100), ("Big", 300, 150)] {
        ok(
            &mut s,
            "hardscape_style.create",
            json!({"name": name, "joints": {"kind": "stack", "unit_length": l, "unit_width": w, "spacing": 5}}),
        );
    }
    for (style, n) in [("Small", 2), ("Big", 1)] {
        for k in 0..n {
            ok(
                &mut s,
                "hardscape.add",
                json!({"boundary": [[k * 10000, 0], [k * 10000 + 3050, 0], [k * 10000 + 3050, 2050], [k * 10000, 2050]], "style": style, "origin": [k * 10000, 0]}),
            );
        }
    }
    let rep = ok(&mut s, "report.unit_pattern", json!({}));
    let by: Vec<(String, String, u64)> = rep["totals"]["by_unit"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| {
            (
                u["pattern"].as_str().unwrap().to_string(),
                u["name"].as_str().unwrap().to_string(),
                u["whole_units"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(by.len(), 2, "{by:?}");
    assert!(
        by.iter()
            .any(|(p, n, w)| p == "Small" && n == "200x100" && *w > 0)
    );
    assert!(by.iter().any(|(p, n, _)| p == "Big" && n == "300x150"));
    // The same style twice is one line: the units of two lots of the same stone are added.
    let small = by.iter().find(|(p, _, _)| p == "Small").unwrap().2;
    let one = rep["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "Small")
        .unwrap()["whole_units"]
        .as_u64()
        .unwrap();
    assert_eq!(small, 2 * one);
    assert_eq!(rep["totals"]["mixed_products"], true);
    // The lower bound is kept per unit type and rounds up per type.
    for u in rep["totals"]["by_unit"].as_array().unwrap() {
        assert!(u["order_units_min"].as_u64().unwrap() >= u["whole_units"].as_u64().unwrap());
    }
    // A pattern given in the call is one product.
    let rep = ok(
        &mut s,
        "report.unit_pattern",
        json!({"pattern": {"unit_length": 200, "unit_width": 100, "gap": 5}}),
    );
    assert_eq!(rep["totals"]["by_unit"].as_array().unwrap().len(), 1);
    assert_eq!(rep["totals"]["by_unit"][0]["pattern"], "custom");
    assert_eq!(rep["totals"]["mixed_products"], false);
}

#[test]
fn waste_per_product_and_products_by_style_id() {
    let mut s = Session::with_sequential_ids();
    let a = ok(
        &mut s,
        "hardscape_style.create",
        json!({"name": "Grey", "joints": {"kind": "stack", "unit_length": 200, "unit_width": 100, "spacing": 5}}),
    )["id"]
        .clone();
    let b = ok(
        &mut s,
        "hardscape_style.create",
        json!({"name": "Red", "joints": {"kind": "stack", "unit_length": 200, "unit_width": 100, "spacing": 5}}),
    )["id"]
        .clone();
    for style in [&a, &b] {
        ok(
            &mut s,
            "hardscape.add",
            json!({"boundary": rect(2050, 1050), "style": style, "origin": [0, 0]}),
        );
    }
    let rep = ok(&mut s, "report.unit_pattern", json!({"waste_percent": 10}));
    let by = rep["totals"]["by_unit"].as_array().unwrap();
    // The same size, two styles: two products, each with its own waste.
    assert_eq!(by.len(), 2, "{by:?}");
    for u in by {
        assert_eq!(u["name"], "200x100");
        assert_eq!(u["order_units"], 100);
        assert_eq!(u["order_units_with_waste"], 110);
        assert!(u["pattern_id"].is_string());
    }
    assert_ne!(by[0]["pattern_id"], by[1]["pattern_id"]);
    // The rows have it too, and the same call without waste has none.
    assert_eq!(rep["rows"][0]["by_unit"][0]["order_units_with_waste"], 110);
    let rep = ok(&mut s, "report.unit_pattern", json!({}));
    assert!(rep["totals"]["by_unit"][0]["order_units_with_waste"].is_null());
    // The joint warning is in the summary, and the option is checked.
    let r = ok(
        &mut s,
        "unit_pattern.layout",
        json!({"region": {"outline": rect(3000, 2000)}, "pattern": {"bond": "basket_weave", "unit_length": 200, "unit_width": 100, "gap": 5}, "include": [], "limit": 0}),
    );
    assert!(
        r["summary"]["joint_warning"]
            .as_str()
            .unwrap()
            .contains("7.5")
    );
    let r = ok(
        &mut s,
        "unit_pattern.layout",
        json!({"region": {"outline": rect(3000, 2000)}, "pattern": {"bond": "basket_weave", "unit_length": 200, "unit_width": 100, "gap": 5}, "include": [], "max_extra_joint_mm": 5}),
    );
    assert!(r["summary"]["joint_warning"].is_null());
    let (_, m) = fail(
        &mut s,
        "unit_pattern.layout",
        json!({"region": {"outline": rect(3000, 2000)}, "pattern": {"unit_length": 200, "unit_width": 100}, "max_extra_joint_mm": -1}),
    );
    assert!(m.contains("max_extra_joint_mm"));
}
