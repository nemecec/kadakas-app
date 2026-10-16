use super::*;
use kadakas_core::uid::AsUid;

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    s.handle(method, params)
        .unwrap_or_else(|e| panic!("{method}: {}", e.message))
}

fn fail(s: &mut Session, method: &str, params: Value) -> String {
    s.handle(method, params).unwrap_err().message
}

fn add(s: &mut Session, shape: Value) -> Value {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape": shape}}),
    )["created"][0]
        .clone()
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Value {
    add(s, json!({"kind": "line", "a": a, "b": b}))
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

/// A hardscape of w by h with the style and a pattern that starts at the origin.
fn slab(s: &mut Session, w: f64, h: f64) -> Value {
    let st = style(s, "running_bond");
    ok(
        s,
        "hardscape.add",
        json!({"boundary": [[0, 0], [w, 0], [w, h], [0, h]], "style": st, "origin": [0, 0], "angle": 0}),
    )["id"]
        .clone()
}

#[test]
fn loose_lines_with_a_gap_and_an_island_become_a_hardscape_with_a_hole() {
    let mut s = Session::with_sequential_ids();
    // A lot of 30 m by 20 m as four lines with 0.4 mm gaps, a round island, and a stray line.
    let g = 0.4;
    let mut ids = vec![
        line(&mut s, [g, 0.0], [30_000.0, 0.0]),
        line(&mut s, [30_000.0, g], [30_000.0, 20_000.0]),
        line(&mut s, [30_000.0 - g, 20_000.0], [0.0, 20_000.0]),
        line(&mut s, [0.0, 20_000.0 - g], [0.0, 0.0]),
    ];
    ids.push(add(
        &mut s,
        json!({"kind": "circle", "center": [15_000, 10_000], "radius": 2000}),
    ));
    let st = style(&mut s, "running_bond");
    let r = ok(
        &mut s,
        "region.from_curves",
        json!({"ids": ids, "tolerance": 1, "create": "hardscape", "style": st}),
    );
    assert_eq!(r["complete"], true);
    assert_eq!(r["closed_gaps"]["count"], 4);
    assert_eq!(r["regions"].as_array().unwrap().len(), 2);
    let lot = r["regions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["depth"] == 0)
        .unwrap();
    assert_eq!(lot["holes"].as_array().unwrap().len(), 1);
    assert_eq!(lot["role"], "area");
    // Only the area becomes a hardscape: the island is its hole.
    assert_eq!(r["created"].as_array().unwrap().len(), 1);
    let id = r["created"][0]["id"].clone();
    let rep = ok(&mut s, "report.hardscape", json!({"ids": [id]}));
    assert_eq!(rep["rows"][0]["holes"], 1);
    let area = rep["rows"][0]["plan_area_m2"].as_f64().unwrap();
    assert!(
        (area - (600.0 - std::f64::consts::PI * 4.0)).abs() < 0.05,
        "{area}"
    );
    // One undo takes the hardscape away again.
    ok(&mut s, "undo", json!({}));
    assert_eq!(
        ok(&mut s, "report.hardscape", json!({}))["rows"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn what_does_not_close_is_reported_not_guessed() {
    let mut s = Session::with_sequential_ids();
    let ids = vec![
        line(&mut s, [0.0, 0.0], [1000.0, 0.0]),
        line(&mut s, [1000.0, 0.0], [1000.0, 1000.0]),
        line(&mut s, [1000.0, 1000.0], [0.0, 1000.0]),
        line(&mut s, [0.0, 970.0], [0.0, 0.0]),
    ];
    let r = ok(
        &mut s,
        "region.from_curves",
        json!({"ids": ids, "tolerance": 5, "create": "polyline"}),
    );
    assert_eq!(r["complete"], false);
    assert_eq!(r["regions"].as_array().unwrap().len(), 0);
    assert_eq!(r["created"].as_array().unwrap().len(), 0);
    let d = &r["dead_ends"];
    assert_eq!(d["count"], 2);
    assert_eq!(
        (d["open"].as_u64(), d["stubs"].as_u64()),
        (Some(2), Some(0))
    );
    let first = &d["listed"][0];
    assert_eq!(first["kind"], "open");
    assert!((first["nearest"]["distance_mm"].as_f64().unwrap() - 30.0).abs() < 1e-9);
    // A bigger tolerance closes it, and says so.
    let r = ok(
        &mut s,
        "region.from_curves",
        json!({"ids": ids, "tolerance": 40, "create": "polyline"}),
    );
    assert_eq!(r["complete"], true);
    assert_eq!(r["closed_gaps"]["listed"][0]["size_mm"], 30.0);
    assert_eq!(r["created"].as_array().unwrap().len(), 1);
    // Other kinds and bad options are told.
    let t = add(
        &mut s,
        json!({"kind": "text", "at": [0, 0], "text": "x", "height": 100}),
    );
    let r = ok(
        &mut s,
        "region.from_curves",
        json!({"ids": [t, ids[0]], "dry_run": true}),
    );
    assert_eq!(r["skipped"][0]["id"], t);
    assert!(
        fail(
            &mut s,
            "region.from_curves",
            json!({"ids": [ids[0]], "tolerance": 0})
        )
        .contains("tolerance")
    );
    assert!(
        fail(
            &mut s,
            "region.from_curves",
            json!({"ids": [ids[0]], "create": "cake"})
        )
        .contains("create")
    );
    assert!(fail(&mut s, "region.from_curves", json!({"ids": []})).contains("select"));
    assert!(
        fail(&mut s, "region.from_curves", json!({"ids": [t]})).contains("none of the objects")
    );
    assert!(
        fail(
            &mut s,
            "region.from_curves",
            json!({"ids": [ids[0]], "depths": "x"})
        )
        .contains("depths")
    );
    assert!(
        fail(
            &mut s,
            "region.from_curves",
            json!({"ids": [ids[0]], "nope": 1})
        )
        .contains("unknown")
    );
}

#[test]
fn zones_share_edges_and_count_per_zone() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 8000.0, 4000.0);
    let l = line(&mut s, [3000.0, -50.0], [3000.0, 4050.0]);
    let r = ok(&mut s, "hardscape.split", json!({"id": id, "lines": [l]}));
    let zones = r["zones"].as_array().unwrap();
    assert_eq!(zones.len(), 2);
    let sum: f64 = zones.iter().map(|z| z["area_m2"].as_f64().unwrap()).sum();
    assert_eq!(sum, 32.0);
    assert_eq!(r["zones_area_m2"], 32.0);
    // Settings by zone number: the left zone is turned by 90 degrees.
    let r = ok(
        &mut s,
        "hardscape.split",
        json!({"id": id, "zones": [{"index": 0, "name": "Stalls", "angle": 90}]}),
    );
    assert_eq!(r["zones"][0]["name"], "Stalls");
    assert_eq!(r["zones"][0]["has_settings"], true);
    let rep = ok(&mut s, "report.unit_pattern", json!({"ids": [id]}));
    let row = &rep["rows"][0];
    let groups = row["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    let (a, b) = (&groups[0], &groups[1]);
    assert_eq!(a["name"], "Stalls");
    // The counts add up.
    assert_eq!(
        row["whole_units"].as_u64().unwrap(),
        a["whole_units"].as_u64().unwrap() + b["whole_units"].as_u64().unwrap()
    );
    assert_eq!(
        row["cut_units"].as_u64().unwrap(),
        a["cut_units"].as_u64().unwrap() + b["cut_units"].as_u64().unwrap()
    );
    let chk = ok(&mut s, "unit_pattern.check", json!({"ids": [id]}));
    assert_eq!(chk["totals"]["counts"]["overlap"], 0, "{chk}");
    assert_eq!(chk["totals"]["counts"]["outside_area"], 0);
    // Moving the split line moves the zones.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [l], "delta": [1000, 0]}}),
    );
    let p = ok(&mut s, "hardscape.plan", json!({"id": id}));
    let areas: Vec<f64> = p["zones"]
        .as_array()
        .unwrap()
        .iter()
        .map(|z| z["area_m2"].as_f64().unwrap())
        .collect();
    assert_eq!(areas, vec![16.0, 16.0]);
    // The setting follows its seed: the zone that held (1000, ...) is still the left one.
    assert_eq!(p["zones"][0]["has_settings"], true);
    // Clearing.
    ok(
        &mut s,
        "hardscape.plan_clear",
        json!({"id": id, "what": "zones"}),
    );
    let p = ok(&mut s, "hardscape.plan", json!({"id": id}));
    assert_eq!(p["zones"][0]["has_settings"], false);
    ok(&mut s, "hardscape.plan_clear", json!({"id": id}));
    let p = ok(&mut s, "hardscape.plan", json!({"id": id}));
    assert_eq!(p["zones"].as_array().unwrap().len(), 0);
    assert_eq!(p["groups"][0]["name"], "field");
}

#[test]
fn a_line_that_stops_short_is_a_note_and_bad_input_is_refused() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 8000.0, 4000.0);
    let r = ok(
        &mut s,
        "hardscape.split",
        json!({"id": id, "lines": [[[3000, 0], [3000, 3950]]]}),
    );
    assert_eq!(r["zones"].as_array().unwrap().len(), 1);
    assert!(
        r["notes"][0].as_str().unwrap().contains("stops inside"),
        "{r}"
    );
    assert!(
        fail(
            &mut s,
            "hardscape.split",
            json!({"id": id, "zones": [{"index": 5}]})
        )
        .contains("no zone 5")
    );
    assert!(
        fail(
            &mut s,
            "hardscape.split",
            json!({"id": id, "lines": [[[0, 0]]]})
        )
        .contains("2 or more")
    );
    assert!(
        fail(&mut s, "hardscape.split", json!({"id": id, "lines": [7]})).contains("object ID or")
    );
    assert!(
        fail(
            &mut s,
            "hardscape.split",
            json!({"id": id, "zones": [{"name": "x"}]})
        )
        .contains("'index' or 'seed'")
    );
    assert!(
        fail(
            &mut s,
            "hardscape.split",
            json!({"id": id, "zones": [{"index": 0, "seed": [1, 1]}]})
        )
        .contains("not both")
    );
    assert!(fail(&mut s, "hardscape.split", json!({"id": id, "bogus": 1})).contains("unknown"));
    let rect = add(
        &mut s,
        json!({"kind": "rect", "min": [0, 0], "max": [10, 10]}),
    );
    assert!(fail(&mut s, "hardscape.split", json!({"id": rect})).contains("not a hardscape"));
    let t = add(
        &mut s,
        json!({"kind": "text", "at": [0, 0], "text": "x", "height": 100}),
    );
    assert!(
        fail(&mut s, "hardscape.split", json!({"id": id, "lines": [t]})).contains("not a line")
    );
}

#[test]
fn a_soldier_course_along_the_kerb_and_the_field_do_not_overlap() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 6000.0, 3000.0);
    let r = ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "courses": [{"name": "Kerb", "along": "outline", "corner": "miter", "align_field": true}]}),
    );
    let c = &r["courses"][0];
    assert_eq!(c["name"], "Kerb");
    assert_eq!(c["corners"], 4);
    assert!(c["units"].as_u64().unwrap() > 150);
    let rep = ok(&mut s, "report.unit_pattern", json!({"ids": [id]}));
    let row = &rep["rows"][0];
    // The stone is one: the soldier course and the field are added up.
    assert_eq!(row["by_unit"].as_array().unwrap().len(), 1);
    assert_eq!(row["by_unit"][0]["name"], "200x100");
    let groups = row["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[1]["role"], "course");
    assert_eq!(groups[1]["course"]["corners"], 4);
    let chk = ok(&mut s, "unit_pattern.check", json!({"ids": [id]}));
    let wide: Vec<&Value> = chk["objects"][0]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "joint_too_wide")
        .collect();
    assert_eq!(chk["totals"]["counts"]["overlap"], 0, "{chk}");
    assert_eq!(chk["totals"]["counts"]["joint_too_narrow"], 0, "{chk}");
    assert_eq!(chk["totals"]["counts"]["joint_too_wide"], 0, "{wide:?}");
    // Replace by name, then remove.
    let r = ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "Kerb", "along": "outline", "corner": "butt", "orientation": "stretcher"}),
    );
    assert_eq!(r["courses"].as_array().unwrap().len(), 1);
    let chk = ok(&mut s, "unit_pattern.check", json!({"ids": [id]}));
    assert_eq!(chk["totals"]["counts"]["overlap"], 0, "{chk}");
    assert!(
        fail(
            &mut s,
            "hardscape.course_remove",
            json!({"id": id, "names": ["Nope"]})
        )
        .contains("no course")
    );
    let r = ok(
        &mut s,
        "hardscape.course_remove",
        json!({"id": id, "names": ["kerb"]}),
    );
    assert_eq!(r["removed"], 1);
    let rep = ok(&mut s, "report.unit_pattern", json!({"ids": [id]}));
    assert!(rep["rows"][0].get("groups").is_none());
}

#[test]
fn course_input_is_checked() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 6000.0, 3000.0);
    assert!(fail(&mut s, "hardscape.course", json!({"id": id, "name": "a"})).contains("one of"));
    assert!(
        fail(
            &mut s,
            "hardscape.course",
            json!({"id": id, "along": "outline"})
        )
        .contains("name")
    );
    assert!(
        fail(
            &mut s,
            "hardscape.course",
            json!({"id": id, "name": "a", "along": "hole:3"})
        )
        .contains("no hole 3")
    );
    assert!(
        fail(
            &mut s,
            "hardscape.course",
            json!({"id": id, "name": "a", "along": "nonsense"})
        )
        .contains("outline or hole")
    );
    assert!(
        fail(
            &mut s,
            "hardscape.course",
            json!({"id": id, "name": "a", "along": "outline", "corner": "x"})
        )
        .contains("corner")
    );
    assert!(
        fail(
            &mut s,
            "hardscape.course",
            json!({"id": id, "name": "a", "along": "outline", "orientation": "x"})
        )
        .contains("orientation")
    );
    assert!(
        fail(&mut s, "hardscape.course", json!({"id": id, "courses": []})).contains("at least one")
    );
    assert!(
        fail(
            &mut s,
            "hardscape.course",
            json!({"id": id, "name": "a", "path": [[0, 0], [100, 0]], "source": id})
        )
        .contains("one of")
    );
    // A hairpin is refused with the point.
    let e = fail(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "a", "path": [[0, 0], [3000, 0], [0, 50]]}),
    );
    assert!(e.contains("turns by"), "{e}");
    // A course along a drawn line is followed.
    let l = line(&mut s, [0.0, 0.0], [5000.0, 0.0]);
    ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "k", "source": l}),
    );
    let before = ok(&mut s, "hardscape.plan", json!({"id": id}))["courses"][0]["length_m"]
        .as_f64()
        .unwrap();
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [l]}}),
    );
    let after = ok(&mut s, "hardscape.plan", json!({"id": id}));
    assert_eq!(after["courses"][0]["length_m"].as_f64().unwrap(), before);
    assert!(after["notes"][0].as_str().unwrap().contains("gone"));
}

#[test]
fn the_check_names_its_options() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 4010.0, 2503.0);
    assert!(fail(&mut s, "unit_pattern.check", json!({"min_share": 2})).contains("min_share"));
    let r = ok(
        &mut s,
        "unit_pattern.check",
        json!({"max_findings": 1, "min_share": 0.9}),
    );
    assert_eq!(r["objects"][0]["id"], id);
    assert!(r["objects"][0]["counts"]["small_piece"].as_u64().unwrap() > 1);
    assert_eq!(r["objects"][0]["findings"].as_array().unwrap().len(), 1);
    assert_eq!(r["totals"]["ok"], false);
    let r = ok(
        &mut s,
        "unit_pattern.check",
        json!({"ids": [id, "00000000-0000-0000-0000-0000000000ff"]}),
    );
    assert_eq!(r["totals"]["complete"], false);
    assert!(
        r["objects"][1]["error"]
            .as_str()
            .unwrap()
            .contains("no such")
    );
    assert!(id.as_uid().is_some());
}

#[test]
fn the_plan_moves_with_the_hardscape_and_undoes() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 8000.0, 4000.0);
    ok(
        &mut s,
        "hardscape.split",
        json!({"id": id, "lines": [[[3000, -50], [3000, 4050]]],
               "zones": [{"index": 1, "name": "Aisle", "angle": 90}]}),
    );
    ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "Kerb", "path": [[0, 0], [8000, 0]], "align_field": true}),
    );
    let before = ok(&mut s, "report.unit_pattern", json!({"ids": [id]}));
    let plan = ok(
        &mut s,
        "hardscape.plan",
        json!({"id": id, "outlines": true}),
    );
    assert_eq!(plan["zones"][1]["name"], "Aisle");
    assert_eq!(
        plan["zones"][0]["outline"][0][0].as_array().unwrap().len(),
        4
    );
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [id], "delta": [5000, 3000]}}),
    );
    let after = ok(&mut s, "report.unit_pattern", json!({"ids": [id]}));
    for k in ["whole_units", "cut_units", "order_units"] {
        assert_eq!(before["rows"][0][k], after["rows"][0][k], "{k}");
    }
    let plan = ok(&mut s, "hardscape.plan", json!({"id": id}));
    assert_eq!(plan["zones"][1]["name"], "Aisle");
    assert_eq!(
        plan["plan"]["splits"][0]["points"][0],
        json!([8000.0, 2950.0])
    );
    // One undo takes the move back; another takes the course away.
    ok(&mut s, "undo", json!({}));
    ok(&mut s, "undo", json!({}));
    let plan = ok(&mut s, "hardscape.plan", json!({"id": id}));
    assert_eq!(plan["courses"].as_array().unwrap().len(), 0);
    assert_eq!(
        plan["plan"]["splits"][0]["points"][0],
        json!([3000.0, -50.0])
    );
    // A hardscape without a joint pattern cannot be split.
    let bare = ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": [[0, 0], [1000, 0], [1000, 1000]]}),
    )["id"]
        .clone();
    assert!(
        fail(&mut s, "hardscape.split", json!({"id": bare, "lines": []}))
            .contains("no joint pattern")
    );
    assert!(
        fail(
            &mut s,
            "hardscape.plan_clear",
            json!({"id": id, "what": "x"})
        )
        .contains("what")
    );
    assert!(fail(&mut s, "hardscape.plan_clear", json!({})).contains("id"));
}
