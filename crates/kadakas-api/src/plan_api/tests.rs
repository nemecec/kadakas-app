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
    assert!(fail(&mut s, "hardscape.split", json!({"id": id, "lines": [7]})).contains("lines[0]"));
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
    // A hairpin has no corner rule: the legs end flush, and the reply says so.
    let r = ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "a", "path": [[0, 0], [3000, 0], [0, 50]]}),
    );
    assert!(r["notes"][0].as_str().unwrap().contains("turns by"), "{r}");
    assert!(
        fail(
            &mut s,
            "hardscape.course",
            json!({"id": id, "name": "b", "along": 5, "path": [[0, 0], [9, 9]]})
        )
        .contains("along")
    );
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
    assert!(
        after["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n.as_str().unwrap().contains("gone"))
    );
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

#[test]
fn an_island_that_touches_the_outline_is_not_paved_unless_asked() {
    let mut s = Session::with_sequential_ids();
    // A lot of 10 m by 6 m, an island of 2 m by 1 m inside it, and a stall line that runs from the
    // top of the lot down to the island: the lines are one drawing.
    let rect = |s: &mut Session, x0: f64, y0: f64, x1: f64, y1: f64| -> Vec<Value> {
        vec![
            line(s, [x0, y0], [x1, y0]),
            line(s, [x1, y0], [x1, y1]),
            line(s, [x1, y1], [x0, y1]),
            line(s, [x0, y1], [x0, y0]),
        ]
    };
    let mut ids = rect(&mut s, 0.0, 0.0, 10_000.0, 6_000.0);
    ids.extend(rect(&mut s, 4000.0, 2000.0, 6000.0, 3000.0));
    ids.push(line(&mut s, [5000.0, 3000.0], [5000.0, 6000.0]));
    let st = style(&mut s, "running_bond");
    let r = ok(
        &mut s,
        "region.from_curves",
        json!({"ids": ids, "create": "hardscape", "style": st, "dry_run": true}),
    );
    let roles: Vec<&str> = r["regions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["role"].as_str().unwrap())
        .collect();
    assert!(roles.contains(&"island?"), "{roles:?}");
    assert!(
        r["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("may be an island"))
    );
    // The island is not made by default: only the two halves of the lot.
    let made: Vec<_> = r["created"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["region"].as_u64().unwrap())
        .collect();
    for m in &made {
        assert_ne!(r["regions"][*m as usize]["role"], "island?");
    }
    // Asking for a region by its number or by a point makes exactly that one.
    let island = r["regions"]
        .as_array()
        .unwrap()
        .iter()
        .position(|x| x["role"] == "island?")
        .unwrap();
    let r2 = ok(
        &mut s,
        "region.from_curves",
        json!({"ids": ids, "create": "polyline", "regions": [island, [5500, 2500]]}),
    );
    assert_eq!(r2["created"].as_array().unwrap().len(), 1);
    assert!(
        fail(
            &mut s,
            "region.from_curves",
            json!({"ids": ids, "create": "polyline", "regions": [99]})
        )
        .contains("no region 99")
    );
    assert!(
        fail(
            &mut s,
            "region.from_curves",
            json!({"ids": ids, "create": "polyline", "regions": [[-5, -5]]})
        )
        .contains("no region holds")
    );
    assert!(
        fail(
            &mut s,
            "region.from_curves",
            json!({"ids": ids, "create": "polyline", "regions": 3})
        )
        .contains("regions")
    );
    assert!(
        fail(
            &mut s,
            "region.from_curves",
            json!({"ids": ids, "depths": 4})
        )
        .contains("depths")
    );
    // The sources are deleted only when every piece is in a region that was made.
    let r3 = ok(
        &mut s,
        "region.from_curves",
        json!({"ids": ids, "create": "polyline", "regions": [0], "delete_source": true}),
    );
    assert!(!r3["kept_sources"].as_array().unwrap().is_empty());
    // A tolerance over 10 mm is a warning.
    let r4 = ok(
        &mut s,
        "region.from_curves",
        json!({"ids": ids, "tolerance": 20, "dry_run": true}),
    );
    assert!(
        r4["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("more than 10 mm"))
    );
}

#[test]
fn zones_keep_their_names_and_can_be_given_by_center() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 8000.0, 4000.0);
    let l = line(&mut s, [3010.0, -50.0], [3010.0, 4050.0]);
    let r = ok(&mut s, "hardscape.split", json!({"id": id, "lines": [l]}));
    assert_eq!(r["zones"][1]["name"], "zone 1");
    assert!(r["zone_edge_cuts"]["whole_units_lost"].as_u64().unwrap() > 5);
    assert!(
        r["zone_edge_note"]
            .as_str()
            .unwrap()
            .contains("not buildable")
    );
    assert_eq!(r["zones_area_m2"], r["area_m2"]);
    assert_eq!(r["courses_area_m2"], 0.0);
    // The line moves past the middle: the names stay with the places where they were made.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [l], "delta": [3000, 0]}}),
    );
    let p = ok(&mut s, "hardscape.plan", json!({"id": id}));
    assert_eq!(p["zones"][0]["name"], "zone 0");
    let r = ok(
        &mut s,
        "hardscape.split",
        json!({"id": id, "zone_units": "centroid"}),
    );
    assert_eq!(r["zone_units"], "centroid");
    assert!(r["zone_edge_cuts"].is_null());
    let rep = ok(&mut s, "report.unit_pattern", json!({"ids": [id]}));
    let groups = rep["rows"][0]["groups"].as_array().unwrap();
    let area: f64 = groups
        .iter()
        .map(|g| g["region_area_m2"].as_f64().unwrap())
        .sum();
    assert!((area - 32.0).abs() < 1e-6, "{area}");
    assert!(
        fail(
            &mut s,
            "hardscape.split",
            json!({"id": id, "zone_units": "x"})
        )
        .contains("zone_units")
    );
    assert!(
        fail(
            &mut s,
            "hardscape.split",
            json!({"id": id, "zones": [{"index": 0, "align_to_edge": 1.5}]})
        )
        .contains("whole number")
    );
    ok(
        &mut s,
        "hardscape.split",
        json!({"id": id, "zone_units": "cut"}),
    );
    let chk = ok(&mut s, "unit_pattern.check", json!({"ids": [id]}));
    assert_eq!(chk["totals"]["counts"]["overlap"], 0);
}

#[test]
fn a_course_along_the_outline_follows_the_edge_and_crossing_courses_are_clean() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 8000.0, 4000.0);
    ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "courses": [
            {"name": "Kerb", "along": "outline"},
            {"name": "Aisle", "path": [[4000, 0], [4000, 4000]], "side": "left"}
        ]}),
    );
    let plan = ok(&mut s, "hardscape.plan", json!({"id": id}));
    assert_eq!(plan["plan"]["courses"][0]["edge"], "outline");
    assert!(
        plan["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n.as_str().unwrap().contains("overlaps an earlier course")),
        "{plan}"
    );
    let chk = ok(&mut s, "unit_pattern.check", json!({"ids": [id]}));
    let c = &chk["totals"]["counts"];
    assert_eq!(
        (
            c["overlap"].as_u64(),
            c["coverage"].as_u64(),
            c["joint_too_narrow"].as_u64()
        ),
        (Some(0), Some(0), Some(0)),
        "{chk}"
    );
}

#[test]
fn a_circle_and_a_chain_of_arcs_that_stall_lines_run_up_to_are_not_paved_by_default() {
    let mut s = Session::with_sequential_ids();
    let mut ids = vec![
        line(&mut s, [0.0, 0.0], [20_000.0, 0.0]),
        line(&mut s, [20_000.0, 0.0], [20_000.0, 10_000.0]),
        line(&mut s, [20_000.0, 10_000.0], [0.0, 10_000.0]),
        line(&mut s, [0.0, 10_000.0], [0.0, 0.0]),
    ];
    // A circle island and a tree island of two arcs, with rows of stalls on both sides of each.
    ids.push(add(
        &mut s,
        json!({"kind": "circle", "center": [10_000, 5_000], "radius": 1500}),
    ));
    let pi = std::f64::consts::PI;
    for (a, b) in [(0.0, pi), (pi, 2.0 * pi)] {
        ids.push(add(
            &mut s,
            json!({"kind": "arc", "center": [16_000, 5_000], "radius": 1000, "start": a, "end": b}),
        ));
    }
    ids.push(line(&mut s, [0.0, 5_000.0], [8_500.0, 5_000.0]));
    ids.push(line(&mut s, [11_500.0, 5_000.0], [15_000.0, 5_000.0]));
    ids.push(line(&mut s, [17_000.0, 5_000.0], [20_000.0, 5_000.0]));
    let st = style(&mut s, "running_bond");
    let r = ok(
        &mut s,
        "region.from_curves",
        json!({"ids": ids, "create": "hardscape", "style": st, "dry_run": true}),
    );
    let regions = r["regions"].as_array().unwrap();
    let doubtful: Vec<f64> = regions
        .iter()
        .filter(|x| x["role"] == "island?")
        .map(|x| x["area_m2"].as_f64().unwrap())
        .collect();
    assert_eq!(doubtful.len(), 2, "{regions:#?}");
    assert!(
        doubtful.iter().any(|a| (a - 7.0686).abs() < 0.01),
        "{doubtful:?}"
    );
    assert!(
        doubtful
            .iter()
            .any(|a| (a - std::f64::consts::PI).abs() < 0.01),
        "{doubtful:?}"
    );
    // The two halves of the lot are made, the islands are not.
    let made: Vec<u64> = r["created"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["region"].as_u64().unwrap())
        .collect();
    assert_eq!(made.len(), 2, "{r:#?}");
    for m in made {
        assert_ne!(regions[m as usize]["role"], "island?");
    }
}

/// A lot of 30 m by 20 m as four lines that meet, with a round island of 2 m radius.
fn lot_with_an_island(s: &mut Session) -> Vec<Value> {
    let mut ids = vec![
        line(s, [0.0, 0.0], [30_000.0, 0.0]),
        line(s, [30_000.0, 0.0], [30_000.0, 20_000.0]),
        line(s, [30_000.0, 20_000.0], [0.0, 20_000.0]),
        line(s, [0.0, 20_000.0], [0.0, 0.0]),
    ];
    ids.push(add(
        s,
        json!({"kind": "circle", "center": [15_000, 10_000], "radius": 2000}),
    ));
    ids
}

#[test]
fn the_depths_and_the_regions_choose_what_becomes_an_object() {
    // Each case has a drawing of its own: the answer of one call must not depend on the calls
    // before it.
    let created = |p: Value| -> Result<usize, String> {
        let mut s = Session::with_sequential_ids();
        let ids = lot_with_an_island(&mut s);
        let mut p = p;
        p["ids"] = json!(ids);
        p["create"] = json!("polyline");
        s.handle("region.from_curves", p)
            .map(|r| r["created"].as_array().unwrap().len())
            .map_err(|e| e.message)
    };
    // Even: the lot. Odd: the islands that are sure. The circle is one: the lot is loose lines, no
    // closed shape, so that the circle is held by one shape only (itself) is no evidence against
    // the lines (it once was, and the circle was a doubtful "island?"). All: both.
    assert_eq!(created(json!({"depths": "even"})), Ok(1));
    assert_eq!(created(json!({"depths": "odd"})), Ok(1));
    assert_eq!(created(json!({})), Ok(2));
    // By number, by a point (in the lot, in the island), and the same region twice is one.
    assert_eq!(created(json!({"regions": [0]})), Ok(1));
    assert_eq!(created(json!({"regions": [[5000, 5000]]})), Ok(1));
    assert_eq!(
        created(json!({"regions": [[15000, 10000], [15100, 10000]]})),
        Ok(1)
    );
    assert_eq!(created(json!({"regions": [0, 1, 0]})), Ok(2));
    // A region that does not exist, or a point that no region holds, is told.
    let m = created(json!({"regions": [9]})).unwrap_err();
    assert!(m.contains("there is no region 9"), "{m}");
    let m = created(json!({"regions": [[900000, 900000]]})).unwrap_err();
    assert!(m.contains("no region holds"), "{m}");
    // Small regions are left out.
    assert_eq!(created(json!({"min_area_m2": 100})), Ok(1));
    // A tolerance over 10 mm closes gaps, and the reply warns about it.
    let mut s = Session::with_sequential_ids();
    let ids = lot_with_an_island(&mut s);
    let r = ok(
        &mut s,
        "region.from_curves",
        json!({"ids": ids, "tolerance": 20, "dry_run": true}),
    );
    let said = r.to_string();
    assert!(said.contains("more than 10 mm"), "{said}");
}

#[test]
fn a_dry_run_and_a_deleted_source_change_what_stays() {
    let mut s = Session::with_sequential_ids();
    let ids = lot_with_an_island(&mut s);
    let before = ok(&mut s, "document.summary", json!({}))["object_count"].clone();
    let r = ok(
        &mut s,
        "region.from_curves",
        json!({"ids": ids, "create": "polyline", "dry_run": true}),
    );
    assert_eq!(r["dry_run"], true);
    assert_eq!(
        ok(&mut s, "document.summary", json!({}))["object_count"],
        before
    );
    let r = ok(
        &mut s,
        "region.from_curves",
        json!({"ids": ids, "create": "polyline", "depths": "all", "delete_source": true}),
    );
    assert_eq!(r["created"].as_array().unwrap().len(), 2);
    // The five curves are gone and two polylines are there.
    assert_eq!(ok(&mut s, "document.summary", json!({}))["object_count"], 2);
}

#[test]
fn a_plan_that_cannot_be_read_is_refused_and_clearing_it_all_repairs_the_hardscape() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 4000.0, 2000.0);
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_data", "id": id, "set": {"pattern_plan": {"splits": "garbage"}}}}),
    );
    let m = fail(&mut s, "hardscape.plan", json!({"id": id}));
    assert!(
        m.contains("cannot be read") && m.contains("plan_clear"),
        "{m}"
    );
    // Only a clear of all of it can be done: a part of a plan that is not read cannot be kept.
    let m = fail(
        &mut s,
        "hardscape.plan_clear",
        json!({"id": id, "what": "courses"}),
    );
    assert!(m.contains("what") && m.contains("all"), "{m}");
    // The hint of the error works: the plan goes, and the hardscape is a plain field again.
    ok(&mut s, "hardscape.plan_clear", json!({"id": id}));
    let p = ok(&mut s, "hardscape.plan", json!({"id": id}));
    assert_eq!(p["groups"][0]["name"], "field");
    assert!(
        ok(&mut s, "objects.get", json!({"id": id}))["data"]
            .get("pattern_plan")
            .is_none()
    );
}

#[test]
fn the_parts_of_a_plan_clear_on_their_own_for_many_hardscapes_at_once() {
    let mut s = Session::with_sequential_ids();
    let a = slab(&mut s, 8000.0, 4000.0);
    let b = ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": [[0, 5000], [8000, 5000], [8000, 9000], [0, 9000]], "style": "Pavers", "origin": [0, 5000], "angle": 0}),
    )["id"]
        .clone();
    for (id, y) in [(&a, 0), (&b, 5000)] {
        ok(
            &mut s,
            "hardscape.split",
            json!({"id": id, "lines": [[[3000, y - 50], [3000, y + 4050]]]}),
        );
        ok(
            &mut s,
            "hardscape.course",
            json!({"id": id, "name": "Kerb", "path": [[0, y], [8000, y]]}),
        );
    }
    let count = |s: &mut Session, id: &Value| {
        let p = ok(s, "hardscape.plan", json!({"id": id}));
        (
            p["plan"]["splits"].as_array().map_or(0, Vec::len),
            p["courses"].as_array().unwrap().len(),
        )
    };
    assert_eq!(count(&mut s, &a), (1, 1));
    // The split lines go with their zones; the courses stay. One call, one undo step.
    ok(
        &mut s,
        "hardscape.plan_clear",
        json!({"ids": [a, b], "what": "splits"}),
    );
    assert_eq!(count(&mut s, &a), (0, 1));
    assert_eq!(count(&mut s, &b), (0, 1));
    ok(&mut s, "undo", json!({}));
    assert_eq!(count(&mut s, &a), (1, 1));
    assert_eq!(count(&mut s, &b), (1, 1));
    ok(
        &mut s,
        "hardscape.plan_clear",
        json!({"ids": [a, b], "what": "courses"}),
    );
    assert_eq!(count(&mut s, &a), (1, 0));
    // All courses of one hardscape at once.
    ok(&mut s, "undo", json!({}));
    let r = ok(
        &mut s,
        "hardscape.course_remove",
        json!({"id": b, "all": true}),
    );
    assert_eq!(
        (r["removed"].as_u64(), r["courses"].as_u64()),
        (Some(1), Some(0))
    );
    assert_eq!(count(&mut s, &a), (1, 1));
}

#[test]
fn a_zone_with_its_own_pattern_is_laid_reported_and_listed_by_group() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 8000.0, 4000.0);
    let r = ok(
        &mut s,
        "hardscape.split",
        json!({"id": id, "lines": [[[3000, -50], [3000, 4050]]], "tolerance": 0.5,
               "zones": [{"seed": [6000, 2000], "name": "Aisle", "origin": [3000, 0],
                          "pattern": {"bond": "stack", "unit_length": 300, "unit_width": 150, "gap": 3}}]}),
    );
    let aisle = r["zones"]
        .as_array()
        .unwrap()
        .iter()
        .find(|z| z["name"] == "Aisle")
        .expect("the zone of the seed has the name");
    assert_eq!(aisle["has_settings"], true);
    assert_eq!(aisle["area_m2"], 20.0);
    assert_eq!(
        ok(&mut s, "hardscape.plan", json!({"id": id}))["plan"]["tolerance"],
        0.5
    );
    // The zone lays its own stone: the report has two products, and the layout tags each unit.
    let rep = ok(&mut s, "report.unit_pattern", json!({"ids": [id]}));
    let names: Vec<&str> = rep["rows"][0]["by_unit"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"200x100") && names.contains(&"300x150"),
        "{names:?}"
    );
    let lay = ok(
        &mut s,
        "unit_pattern.layout",
        json!({"id": id, "include": ["units"], "limit": 20000}),
    );
    let groups = lay["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    let per_group: u64 = groups
        .iter()
        .map(|g| g["whole_units"].as_u64().unwrap() + g["cut_units"].as_u64().unwrap())
        .sum();
    assert_eq!(per_group, lay["units_total"].as_u64().unwrap());
    let units = lay["units"].as_array().unwrap();
    assert!(units.iter().any(|u| u["group"] == "Aisle"));
    assert!(units.iter().all(|u| u["group"].is_string()));
    // The table has a row for each stone of the hardscape and their sum, then one for each zone.
    let rep = ok(
        &mut s,
        "report.unit_pattern",
        json!({"ids": [id], "format": "csv"}),
    );
    let csv = rep["csv"].as_str().unwrap();
    assert_eq!(csv.lines().count(), 6, "{csv}");
    assert!(csv.contains(" / Aisle"), "{csv}");
    // A course whose drawn line is gone keeps its path, and the layout says why.
    let l = line(&mut s, [0.0, 0.0], [8000.0, 0.0]);
    ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "Kerb", "source": l}),
    );
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [l]}}),
    );
    let lay = ok(
        &mut s,
        "unit_pattern.layout",
        json!({"id": id, "include": [], "limit": 0}),
    );
    assert!(lay["notes"].to_string().contains("gone"), "{lay}");
}

#[test]
fn a_course_takes_its_side_and_its_stone_from_the_call() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 6000.0, 3000.0);
    // Along a drawn line, on its right side, with a stone of its own.
    let l = line(&mut s, [0.0, 3000.0], [6000.0, 3000.0]);
    let r = ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "Edge", "source": l, "side": "right",
               "unit_length": 300, "unit_width": 150, "gap": 4, "unit": "Kerb 300"}),
    );
    assert_eq!(r["courses"][0]["name"], "Edge");
    let rep = ok(&mut s, "report.unit_pattern", json!({"ids": [id]}));
    let names: Vec<&str> = rep["rows"][0]["by_unit"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Kerb 300"), "{names:?}");
    // The right side of a line that runs east along the top edge is inside the slab.
    let chk = ok(&mut s, "unit_pattern.check", json!({"ids": [id]}));
    assert_eq!(chk["totals"]["counts"]["outside_area"], 0, "{chk}");
    // Inside and outside of the outline: outside puts the course out of the area, where no
    // unit of it is laid.
    for (side, inside) in [("inside", true), ("outside", false)] {
        ok(
            &mut s,
            "hardscape.course",
            json!({"id": id, "name": "Edge", "along": "outline", "side": side}),
        );
        let chk = ok(&mut s, "unit_pattern.check", json!({"ids": [id]}));
        let course = chk["objects"][0]["coverage"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["role"] == "course")
            .unwrap()
            .clone();
        let n = course["units"].as_u64().unwrap();
        assert_eq!(n > 100, inside, "{side}: {n}");
    }
    // A text is not a path.
    let t = add(
        &mut s,
        json!({"kind": "text", "at": [0, 0], "text": "x", "height": 100}),
    );
    let m = fail(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "T", "source": t}),
    );
    assert!(m.contains("not a line, arc or polyline"), "{m}");
}

#[test]
fn a_course_on_a_module_pattern_needs_a_stone_size_of_its_own() {
    let mut s = Session::with_sequential_ids();
    let hex = ok(
        &mut s,
        "unit_pattern.module_preset",
        json!({"preset": "hexagon", "across_flats": 200}),
    )["pattern"]
        .clone();
    ok(
        &mut s,
        "hardscape_style.create",
        json!({"name": "Hex", "joints": {"kind": "module", "units": hex["units"], "a": hex["a"], "b": hex["b"], "spacing": 4}}),
    );
    let id = ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "style": "Hex"}),
    )["id"]
        .clone();
    let m = fail(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "Kerb", "along": "outline"}),
    );
    assert!(m.contains("no bond to take the stone from"), "{m}");
    let r = ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "Kerb", "along": "outline", "unit_length": 200, "unit_width": 100}),
    );
    assert!(r["courses"][0]["units"].as_u64().unwrap() > 50, "{r}");
}

#[test]
fn curves_become_landscape_areas_on_the_layer_and_class_of_the_call() {
    let mut s = Session::with_sequential_ids();
    let ids = lot_with_an_island(&mut s);
    let layer = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_layer", "name": "Beds"}}),
    );
    let class = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_class", "name": "Soft"}}),
    );
    let (layer, class) = (layer["created"][0].clone(), class["created"][0].clone());
    let r = ok(
        &mut s,
        "region.from_curves",
        json!({"ids": ids, "create": "landscape_area", "depths": "even", "name": "Lawn",
               "layer": layer, "class": class}),
    );
    let made = r["created"].as_array().unwrap();
    assert_eq!(made.len(), 1);
    let o = ok(&mut s, "objects.get", json!({"id": made[0]["id"]}));
    assert_eq!(o["shape"]["kind"], "landscape_area", "{o}");
    assert_eq!(o["shape"]["name"], "Lawn", "{o}");
    assert_eq!(o["layer"], layer, "{o}");
    assert_eq!(o["class"], class, "{o}");
    // The island is a hole of the area.
    let holes = o["data"]["landscape_area"]["holes"].as_array().unwrap();
    assert_eq!(holes.len(), 1, "{o}");
}

#[test]
fn closed_3d_polylines_are_read_in_plan_and_the_reply_says_so() {
    let mut s = Session::with_sequential_ids();
    let lot = add(
        &mut s,
        json!({"kind": "polyline3d", "closed": true,
               "points": [[0, 0, 100], [6000, 0, 150], [6000, 4000, 200], [0, 4000, 120]]}),
    );
    let r = ok(
        &mut s,
        "region.from_curves",
        json!({"ids": [lot], "create": "polyline"}),
    );
    assert_eq!(r["regions"].as_array().unwrap().len(), 1, "{r}");
    assert!(
        r["warnings"].to_string().contains("heights are dropped"),
        "{r}"
    );
    // Flat ones are not worth a note.
    let flat = add(
        &mut s,
        json!({"kind": "polyline3d", "closed": true,
               "points": [[0, 0, 100], [6000, 0, 100], [6000, 4000, 100], [0, 4000, 100]]}),
    );
    let r = ok(
        &mut s,
        "region.from_curves",
        json!({"ids": [flat], "dry_run": true}),
    );
    assert!(!r["warnings"].to_string().contains("heights"), "{r}");
}

#[test]
fn the_check_takes_its_thresholds_from_the_call_and_lists_what_it_cannot_check() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 4010.0, 2503.0);
    let base = ok(&mut s, "unit_pattern.check", json!({"ids": [id]}));
    let small = |r: &Value| r["totals"]["counts"]["small_piece"].as_u64().unwrap_or(0);
    // The cut row along the top is a few mm wide: a piece narrower than 50 mm is small now.
    let narrow = ok(
        &mut s,
        "unit_pattern.check",
        json!({"ids": [id], "min_width_mm": 50, "min_share": 0}),
    );
    assert!(small(&narrow) > small(&base), "{narrow}");
    // Wide limits find nothing.
    let lax = ok(
        &mut s,
        "unit_pattern.check",
        json!({"ids": [id], "overlap_mm2": 1e6, "overlap_depth_mm": 100, "min_share": 0,
               "size_tol_mm": 100, "coverage_tol": 1, "trim": 0.1}),
    );
    assert_eq!(lax["totals"]["ok"], true, "{lax}");
    // An object with no pattern is a row with an error, and the totals say the check is not
    // complete.
    let bare = ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": [[0, 0], [1000, 0], [1000, 1000]]}),
    )["id"]
        .clone();
    let r = ok(&mut s, "unit_pattern.check", json!({"ids": [id, bare]}));
    assert!(r["objects"][1]["error"].is_string(), "{r}");
    assert_eq!(r["totals"]["complete"], false);
}

#[test]
fn zone_set_changes_one_zone_and_says_when_nothing_changed() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 8000.0, 4000.0);
    let l = line(&mut s, [3000.0, -50.0], [3000.0, 4050.0]);
    ok(&mut s, "hardscape.split", json!({"id": id, "lines": [l]}));
    // Set a name, an angle and an origin on the second zone; the first stays as it is.
    let r = ok(
        &mut s,
        "hardscape.zone_set",
        json!({"id": id, "zone": 1, "name": " Yard ", "angle": 45, "origin": [10, 20]}),
    );
    assert_eq!(r["changed"], true);
    assert_eq!(r["name"], "Yard");
    assert_eq!(r["has_settings"], true);
    assert_eq!(r["zones"].as_array().unwrap().len(), 2);
    assert_eq!(r["zones"][0]["has_settings"], false);
    // The same call again changes nothing.
    let r = ok(
        &mut s,
        "hardscape.zone_set",
        json!({"id": id, "zone": 1, "name": "Yard", "angle": 45, "origin": [10, 20]}),
    );
    assert_eq!(r["changed"], false);
    // The two alignments replace each other.
    let r = ok(
        &mut s,
        "hardscape.zone_set",
        json!({"id": id, "zone": 1, "align_to_edge": 0}),
    );
    assert_eq!(r["changed"], true);
    ok(
        &mut s,
        "hardscape.zone_set",
        json!({"id": id, "zone": 1, "align_to_split": 0}),
    );
    // Clear every setting; the name stays.
    let r = ok(
        &mut s,
        "hardscape.zone_set",
        json!({"id": id, "zone": 1,
            "clear": ["pattern", "origin", "angle", "align_to_edge", "align_to_split"]}),
    );
    assert_eq!(r["changed"], true);
    // Bad input is refused.
    for (p, word) in [
        (json!({"id": id}), "zone"),
        (json!({"id": id, "zone": 7}), "no zone 7"),
        (
            json!({"id": id, "zone": 0, "clear": ["color"]}),
            "cannot clear",
        ),
        (json!({"id": id, "zone": 0, "clear": 5}), "array"),
        (
            json!({"id": id, "zone": 0, "angle": 5, "clear": ["angle"]}),
            "set and cleared",
        ),
        (
            json!({"id": id, "zone": 0, "align_to_edge": 0, "align_to_split": 0}),
            "not both",
        ),
        (json!({"id": id, "zone": 0, "name": 5}), "string"),
        (json!({"id": id, "zone": 0, "bogus": 1}), "unknown"),
    ] {
        let m = fail(&mut s, "hardscape.zone_set", p.clone());
        assert!(m.contains(word), "{p}: {m}");
    }
}

#[test]
fn a_course_with_holes_is_a_warning_of_the_report_and_of_the_export() {
    // Long stones on the outside of a tight bend leave wedges without a unit that are wider than
    // a joint: the check finds them, and the report and the DXF export must say so too (with the
    // count, the area and where), not only the check.
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 6000.0, 3000.0);
    let arc = add(
        &mut s,
        json!({"kind": "arc", "center": [3000, 1200], "radius": 900, "start": 0.2, "end": 2.9}),
    );
    ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "Ring", "source": arc, "side": "right",
               "unit_length": 400, "unit_width": 200, "gap": 3}),
    );
    let chk = ok(&mut s, "unit_pattern.check", json!({"ids": [id]}));
    let n = chk["totals"]["counts"]["coverage"].as_u64().unwrap();
    assert!(n > 0, "{chk}");
    let rep = ok(&mut s, "report.unit_pattern", json!({"ids": [id]}));
    let row = &rep["rows"][0];
    assert_eq!(row["coverage_gaps"]["count"].as_u64(), Some(n), "{row}");
    assert!(row["coverage_gaps"]["area_m2"].as_f64().unwrap() > 0.0);
    let first = &row["coverage_gaps"]["listed"][0];
    assert_eq!(first["group"], "Ring");
    assert!(first["at"][0].as_f64().is_some(), "{first}");
    let w = row["warnings"][0].as_str().unwrap();
    let places = format!("in {n} place");
    assert!(
        w.contains(&places) && w.contains("m2 in all") && w.contains(" at ["),
        "{w}"
    );
    assert_eq!(rep["totals"]["coverage_gaps"].as_u64(), Some(n));
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("ring.dxf");
    let ex = ok(
        &mut s,
        "export.file",
        json!({"path": out, "format": "dxf", "options": {"patterns": "units"}}),
    );
    let warnings = ex["warnings"].as_array().expect("export warnings");
    assert!(
        warnings
            .iter()
            .any(|x| x.as_str().unwrap().contains(&places)),
        "{ex}"
    );
    assert_eq!(ex["report"]["warnings"], ex["warnings"]);
    // A hardscape without holes has no warning.
    let mut s2 = Session::with_sequential_ids();
    let clean = slab(&mut s2, 2000.0, 1000.0);
    let rep = ok(&mut s2, "report.unit_pattern", json!({"ids": [clean]}));
    assert!(rep["rows"][0].get("warnings").is_none(), "{rep}");
    assert_eq!(rep["totals"]["coverage_gaps"].as_u64(), Some(0));
}

#[test]
fn wide_joints_are_a_warning_of_the_report_and_of_the_export() {
    // A miter course that does not continue the field leaves a wide gap between the kerb and the
    // field: the check finds it, and the report and the export must say so too.
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 6000.0, 3000.0);
    ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "courses": [{"name": "Kerb", "along": "outline", "corner": "miter"}]}),
    );
    let chk = ok(&mut s, "unit_pattern.check", json!({"ids": [id]}));
    let n = chk["totals"]["counts"]["joint_too_wide"].as_u64().unwrap();
    assert!(n > 0, "{chk}");
    let rep = ok(&mut s, "report.unit_pattern", json!({"ids": [id]}));
    let row = &rep["rows"][0];
    assert_eq!(row["wide_joints"]["count"].as_u64(), Some(n), "{row}");
    assert!(row["wide_joints"]["widest_mm"].as_f64().unwrap() > 5.0);
    let places = format!("{n} joint");
    let w = row["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .find(|x| x.contains("wider"))
        .unwrap_or_else(|| panic!("{row}"));
    assert!(
        w.contains(&places) && w.contains("widest") && w.contains(" at ["),
        "{w}"
    );
    assert_eq!(rep["totals"]["wide_joints"].as_u64(), Some(n));
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("kerb.dxf");
    let ex = ok(
        &mut s,
        "export.file",
        json!({"path": out, "format": "dxf", "options": {"patterns": "units"}}),
    );
    assert!(
        ex["warnings"]
            .as_array()
            .expect("export warnings")
            .iter()
            .any(|x| x.as_str().unwrap().contains(&places)),
        "{ex}"
    );
    let mut s2 = Session::with_sequential_ids();
    let clean = slab(&mut s2, 2000.0, 1000.0);
    let rep = ok(&mut s2, "report.unit_pattern", json!({"ids": [clean]}));
    assert!(rep["rows"][0].get("warnings").is_none(), "{rep}");
}

#[test]
fn the_warnings_of_the_report_add_up_over_objects_and_cap_the_lists() {
    // One object with wide joints in several places, one with holes (and wide joints) in a course, one clean: each
    // row has only its own warnings, the totals are the sums, and a long list is cut but counted.
    let mut s = Session::with_sequential_ids();
    let st = style(&mut s, "running_bond");
    let at = |s: &mut Session, w: f64, h: f64, x: f64| -> Value {
        ok(
            s,
            "hardscape.add",
            json!({"boundary": [[x, 0], [x + w, 0], [x + w, h], [x, h]], "style": st, "origin": [x, 0], "angle": 0}),
        )["id"]
            .clone()
    };
    let wide = at(&mut s, 12000.0, 6000.0, 0.0);
    ok(
        &mut s,
        "hardscape.course",
        json!({"id": wide, "courses": [{"name": "Kerb", "along": "outline", "corner": "miter"}]}),
    );
    let holes = at(&mut s, 6000.0, 3000.0, 20000.0);
    let arc = add(
        &mut s,
        json!({"kind": "arc", "center": [23000, 1200], "radius": 900, "start": 0.2, "end": 2.9}),
    );
    ok(
        &mut s,
        "hardscape.course",
        json!({"id": holes, "name": "Ring", "source": arc, "side": "right",
               "unit_length": 400, "unit_width": 200, "gap": 3}),
    );
    let clean = at(&mut s, 2000.0, 1000.0, 40000.0);
    let chk = ok(
        &mut s,
        "unit_pattern.check",
        json!({"ids": [wide, holes, clean]}),
    );
    let counts = |o: &Value, k: &str| o["counts"][k].as_u64().unwrap();
    let objects = chk["objects"].as_array().unwrap();
    let n_wide = counts(&objects[0], "joint_too_wide");
    let n_holes = counts(&objects[1], "coverage");
    assert!(n_wide > 3 && n_holes > 0, "{chk}");
    let rep = ok(
        &mut s,
        "report.unit_pattern",
        json!({"ids": [wide, holes, clean]}),
    );
    let rows = rep["rows"].as_array().unwrap();
    let warnings = |r: &Value| -> Vec<String> {
        r.get("warnings")
            .map(|w| {
                w.as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_str().unwrap().to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    // The wide joints: counted in full, listed up to a cap, the warning names three and the rest.
    let w = &rows[0]["wide_joints"];
    assert_eq!(w["count"].as_u64(), Some(n_wide), "{w}");
    assert_eq!(
        w["listed"].as_array().unwrap().len() as u64,
        n_wide.min(20),
        "{w}"
    );
    assert!(w["listed"][0]["width_mm"].as_f64().unwrap() > 5.0);
    assert!(w["listed"][0]["group"].as_str().is_some());
    let text = warnings(&rows[0]);
    assert_eq!(text.len(), 1, "{text:?}");
    assert!(
        text[0].contains(&format!("{n_wide} joints are wider"))
            && text[0].contains(&format!("and {} more", n_wide - 3)),
        "{}",
        text[0]
    );
    assert!(rows[0].get("coverage_gaps").is_none(), "{}", rows[0]);
    // The ring has holes and wide joints: both warnings, each counted as the check counts it.
    let n_ring = counts(&objects[1], "joint_too_wide");
    assert!(n_ring > 0, "{chk}");
    assert_eq!(rows[1]["wide_joints"]["count"].as_u64(), Some(n_ring));
    assert_eq!(warnings(&rows[1]).len(), 2);
    assert!(warnings(&rows[1])[0].contains("do not cover"));
    assert_eq!(rows[1]["coverage_gaps"]["count"].as_u64(), Some(n_holes));
    // The clean one: nothing.
    assert!(warnings(&rows[2]).is_empty(), "{}", rows[2]);
    assert!(rows[2].get("wide_joints").is_none());
    assert_eq!(rep["totals"]["wide_joints"].as_u64(), Some(n_wide + n_ring));
    assert_eq!(rep["totals"]["coverage_gaps"].as_u64(), Some(n_holes));
    // The export warns for the same two objects, once each, and not for the clean one.
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("all.dxf");
    let ex = ok(
        &mut s,
        "export.file",
        json!({"path": out, "format": "dxf", "options": {"patterns": "units"}}),
    );
    let all: Vec<&str> = ex["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect();
    assert_eq!(
        all.iter().filter(|x| x.contains("wider than")).count(),
        2,
        "{all:?}"
    );
    assert_eq!(
        all.iter().filter(|x| x.contains("do not cover")).count(),
        1,
        "{all:?}"
    );
}

#[test]
fn a_joint_change_of_the_style_reaches_courses_and_zones_without_their_own_joint() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 8000.0, 4000.0);
    let style_id = ok(&mut s, "hardscape_style.list", json!({}))["styles"][0]["id"].clone();
    ok(
        &mut s,
        "hardscape.split",
        json!({"id": id, "lines": [[[3000, -50], [3000, 4050]]], "tolerance": 0.5,
               "zones": [{"seed": [6000, 2000], "name": "Aisle", "origin": [3000, 0],
                          "pattern": {"bond": "stack", "unit_length": 300, "unit_width": 150}},
                         {"seed": [1000, 2000], "name": "Own", "origin": [0, 0],
                          "pattern": {"bond": "stack", "unit_length": 300, "unit_width": 150, "gap": 3}}]}),
    );
    ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "Kerb", "along": "outline"}),
    );
    let l = line(&mut s, [0.0, 0.0], [8000.0, 0.0]);
    ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "Fixed", "source": l, "gap": 3}),
    );
    let gaps = |s: &mut Session| -> std::collections::BTreeMap<String, f64> {
        let lay = ok(
            s,
            "unit_pattern.layout",
            json!({"id": id, "include": [], "limit": 0}),
        );
        lay["groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| {
                (
                    g["name"].as_str().unwrap().to_string(),
                    g["gap_mm"].as_f64().unwrap(),
                )
            })
            .collect()
    };
    let before = gaps(&mut s);
    assert_eq!(before["Kerb"], 5.0, "{before:?}");
    assert_eq!(before["Aisle"], 5.0, "{before:?}");
    assert_eq!(before["Own"], 3.0, "{before:?}");
    assert_eq!(before["Fixed"], 3.0, "{before:?}");
    ok(
        &mut s,
        "hardscape_style.update",
        json!({"id": style_id, "joints": {"kind": "running_bond", "unit_length": 200, "unit_width": 100, "spacing": 8}}),
    );
    let after = gaps(&mut s);
    assert_eq!(
        after["Kerb"], 8.0,
        "a course without a joint follows the style: {after:?}"
    );
    assert_eq!(
        after["Aisle"], 8.0,
        "a zone pattern without a joint follows the style: {after:?}"
    );
    assert_eq!(after["Own"], 3.0, "an explicit joint stays: {after:?}");
    assert_eq!(after["Fixed"], 3.0, "an explicit joint stays: {after:?}");
    // The stored plan holds no joint for the inheriting course.
    let plan = ok(&mut s, "hardscape.plan", json!({"id": id}));
    let kerb = plan["plan"]["courses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Kerb")
        .unwrap()
        .clone();
    assert!(kerb.get("gap").is_none(), "{kerb}");
}

#[test]
fn align_field_names_the_edge_it_used_and_lets_the_agent_choose_one() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 6000.0, 3000.0);
    // The outline of the slab runs east along the long south edge first.
    let r = ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "Kerb", "along": "outline", "align_field": true}),
    );
    let first = &r["aligned"]["edge"];
    assert_eq!(first["index"], 0, "{r}");
    assert_eq!(first["chosen"], false);
    assert!(first["length_mm"].as_f64().unwrap() > 1000.0, "{r}");
    let notes = r["notes"].to_string();
    assert!(
        notes.contains("align_field") && notes.contains("edge 0"),
        "{notes}"
    );
    let edges = r["aligned"]["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 4, "{r}");
    let first_angle = r["aligned"]["angle_deg"].as_f64().unwrap();
    // Another edge: the short one on the east side, running north.
    let short = edges
        .iter()
        .min_by(|a, b| {
            a["length_mm"]
                .as_f64()
                .unwrap()
                .total_cmp(&b["length_mm"].as_f64().unwrap())
        })
        .unwrap()["index"]
        .as_u64()
        .unwrap();
    let r2 = ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "Kerb", "along": "outline", "align_field": {"edge": short}}),
    );
    assert_eq!(r2["aligned"]["edge"]["index"], short);
    assert_eq!(r2["aligned"]["edge"]["chosen"], true);
    let second_angle = r2["aligned"]["angle_deg"].as_f64().unwrap();
    assert!(
        (second_angle - first_angle).abs() > 45.0,
        "{first_angle} {second_angle}"
    );
    assert!(
        r2["notes"].to_string().contains("replaced the origin"),
        "{r2}"
    );
    // An edge that does not exist is refused with the list of the real ones.
    let m = fail(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "Kerb", "along": "outline", "align_field": {"edge": 99}}),
    );
    assert!(
        m.contains("not a straight edge") && m.contains("mm at"),
        "{m}"
    );
}

#[test]
fn align_field_defaults_to_the_longest_straight_edge() {
    let mut s = Session::with_sequential_ids();
    let st = style(&mut s, "running_bond");
    // The outline starts with a short edge (270 mm), then a long one of 6000 mm.
    let id = ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": [[0, 0], [270, 0], [6000, 0], [6000, 3000], [0, 3000]], "style": st, "origin": [0, 0], "angle": 0}),
    )["id"]
        .clone();
    let r = ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "Kerb", "along": "outline", "align_field": true}),
    );
    let edge = &r["aligned"]["edge"];
    assert_eq!(edge["chosen"], false, "{r}");
    assert!(edge["length_mm"].as_f64().unwrap() > 3000.0, "{r}");
    assert_ne!(edge["index"], 0, "the 270 mm piece is not used: {r}");
    let notes = r["notes"].to_string();
    assert!(notes.contains("longest"), "{notes}");
}

#[test]
fn hardscape_plan_shows_an_inherited_joint_as_null_with_the_effective_one() {
    let mut s = Session::with_sequential_ids();
    let id = slab(&mut s, 8000.0, 4000.0);
    let style_id = ok(&mut s, "hardscape_style.list", json!({}))["styles"][0]["id"].clone();
    ok(
        &mut s,
        "hardscape.split",
        json!({"id": id, "lines": [[[3000, -50], [3000, 4050]]], "tolerance": 0.5,
               "zones": [{"seed": [6000, 2000], "name": "Aisle", "origin": [3000, 0],
                          "pattern": {"bond": "stack", "unit_length": 300, "unit_width": 150}},
                         {"seed": [1000, 2000], "name": "Own", "origin": [0, 0],
                          "pattern": {"bond": "stack", "unit_length": 300, "unit_width": 150, "gap": 3}}]}),
    );
    ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "Kerb", "along": "outline"}),
    );
    let l = line(&mut s, [0.0, 0.0], [8000.0, 0.0]);
    ok(
        &mut s,
        "hardscape.course",
        json!({"id": id, "name": "Fixed", "source": l, "gap": 3}),
    );
    let check = |s: &mut Session, field: f64| {
        let p = ok(s, "hardscape.plan", json!({"id": id}));
        let zone = |n: &str| {
            p["plan"]["zones"]
                .as_array()
                .unwrap()
                .iter()
                .find(|z| z["name"] == n)
                .unwrap()
                .clone()
        };
        let course = |n: &str| {
            p["plan"]["courses"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["name"] == n)
                .unwrap()
                .clone()
        };
        let aisle = zone("Aisle");
        assert!(aisle["pattern"]["gap"].is_null(), "{aisle}");
        assert_eq!(aisle["inherit_gap"], true);
        assert_eq!(aisle["effective_gap"], field, "{aisle}");
        let own = zone("Own");
        assert_eq!(own["pattern"]["gap"], 3.0, "{own}");
        assert_eq!(own["effective_gap"], 3.0);
        let kerb = course("Kerb");
        assert!(kerb["gap"].is_null(), "{kerb}");
        assert_eq!(kerb["effective_gap"], field, "{kerb}");
        let fixed = course("Fixed");
        assert_eq!(fixed["gap"], 3.0);
        assert_eq!(fixed["effective_gap"], 3.0);
    };
    check(&mut s, 5.0);
    ok(
        &mut s,
        "hardscape_style.update",
        json!({"id": style_id, "joints": {"kind": "running_bond", "unit_length": 200, "unit_width": 100, "spacing": 8}}),
    );
    check(&mut s, 8.0);
}
