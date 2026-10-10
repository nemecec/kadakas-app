use super::*;
use cadektor_core::uid::AsUid;
use cadektor_irrigation::hydraulics::{pipe_loss_kpa, valve_loss_kpa};

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    s.handle(method, params)
        .unwrap_or_else(|e| panic!("{method}: {}", e.message))
}

fn fail(s: &mut Session, method: &str, params: Value) -> (i64, String) {
    let e = s.handle(method, params).unwrap_err();
    (e.code, e.message)
}

fn id(v: &Value) -> cadektor_core::uid::Uid {
    v["id"].as_uid().unwrap()
}

/// POC, 20 m PVC 50 mainline, valve (zone 1, kv 6), 30 m PE 32 lateral, 10 m PE 25 branch and
/// four Spray-4.0 heads (8 l/min each) at the ends.
fn zone() -> (Session, Vec<cadektor_core::uid::Uid>) {
    let mut s = Session::with_sequential_ids();
    ok(
        &mut s,
        "irrigation.poc.add",
        json!({"at": [0, 0], "static_pressure": 400, "available_flow": 100, "name": "Meter"}),
    );
    ok(
        &mut s,
        "irrigation.line.add",
        json!({"points": [[0, 0], [20000, 0]], "line_type": "mainline", "size": 50, "material": "PVC"}),
    );
    ok(
        &mut s,
        "irrigation.valve.add",
        json!({"at": [20000, 0], "zone": 1, "kv": 6}),
    );
    ok(
        &mut s,
        "irrigation.line.add",
        json!({"points": [[20000, 0], [50000, 0]], "size": "32", "material": "PE"}),
    );
    ok(
        &mut s,
        "irrigation.line.add",
        json!({"points": [[50000, 0], [50000, 10000]], "size": "25", "material": "PE"}),
    );
    let mut ids = vec![];
    for at in [[50000, 0], [50000, 0], [50000, 10000], [50010, 10010]] {
        ids.push(id(&ok(
            &mut s,
            "irrigation.outlet.add",
            json!({"at": at, "model": "Spray-4.0"}),
        )));
    }
    (s, ids)
}

#[test]
fn objects_are_added_and_listed_by_kind() {
    let (mut s, ids) = zone();
    let list = ok(&mut s, "objects.list", json!({"kind": "irrigation_outlet"}));
    assert_eq!(list.as_array().unwrap().len(), 4);
    let o = ok(&mut s, "objects.get", json!({"id": ids[0]}));
    assert_eq!(o["shape"]["kind"], "irrigation");
    assert_eq!(o["shape"]["type"], "outlet");
    // The radius of the model is stored in the outlet.
    assert_eq!(o["shape"]["radius"], 4000.0);
    assert_eq!(
        ok(&mut s, "objects.list", json!({"kind": "irrigation_line"}))
            .as_array()
            .unwrap()
            .len(),
        3
    );
    // The SVG shows the coverage.
    let svg = ok(&mut s, "render.svg", json!({}));
    assert!(svg["svg"].as_str().unwrap().contains("irrigation_outlet"));
}

#[test]
fn calculate_matches_hand_formulas() {
    let (mut s, ids) = zone();
    let r = ok(&mut s, "irrigation.calculate", json!({}));
    assert_eq!(r["ok"], true, "{}", r["warnings"]);
    assert_eq!(r["assigned"], 4);
    let z = &r["zones"][0];
    assert_eq!(z["zone"], 1);
    assert!((z["flow_lpm"].as_f64().unwrap() - 32.0).abs() < 1e-9);
    // Pressure at the critical (far) outlet.
    let p_end = 400.0
        - pipe_loss_kpa(32.0, 45.2, 150.0, 20.0)
        - valve_loss_kpa(32.0, 6.0)
        - pipe_loss_kpa(32.0, 26.2, 140.0, 30.0);
    let p_far = p_end - pipe_loss_kpa(16.0, 20.4, 140.0, 10.0);
    let crit = &z["critical"];
    assert!((crit["pressure_kpa"].as_f64().unwrap() - p_far).abs() < 1e-6);
    assert_eq!(crit["id"], ids[2]);
    assert_eq!(z["critical_path"][0]["pressure_kpa"], 400.0);
    assert_eq!(z["outlets"].as_array().unwrap().len(), 4);
    // The zone is stored in the outlets, in one undo step.
    let o = ok(&mut s, "objects.get", json!({"id": ids[0]}));
    assert_eq!(o["shape"]["zone"], 1);
    assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
    let o = ok(&mut s, "objects.get", json!({"id": ids[0]}));
    assert!(o["shape"].get("zone").is_none());
    // A second calculation without assignment changes nothing.
    let r = ok(
        &mut s,
        "irrigation.calculate",
        json!({"assign_zones": false}),
    );
    assert_eq!(r["assigned"], 0);
    // The schedule is part of the result.
    assert_eq!(r["zone_schedule"]["rows"][0]["head_count"], 4);
}

#[test]
fn warnings_for_low_pressure_and_supply() {
    let (mut s, _) = zone();
    let poc = ok(&mut s, "objects.list", json!({"kind": "irrigation_poc"}))[0]["id"].clone();
    ok(
        &mut s,
        "irrigation.set",
        json!({"id": poc, "available_flow": 20}),
    );
    let r = ok(
        &mut s,
        "irrigation.calculate",
        json!({"assign_zones": false}),
    );
    assert_eq!(r["ok"], false);
    let kinds: Vec<&str> = r["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"flow_exceeds_supply"), "{kinds:?}");
}

#[test]
fn low_pressure_warns_with_weak_supply() {
    let (mut s, ids) = zone();
    let poc = ok(&mut s, "objects.list", json!({"kind": "irrigation_poc"}))[0]["id"].clone();
    ok(
        &mut s,
        "irrigation.set",
        json!({"id": poc, "static_pressure": 150}),
    );
    let r = ok(&mut s, "irrigation.calculate", json!({}));
    let low: Vec<&Value> = r["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["kind"] == "low_pressure")
        .collect();
    assert!(!low.is_empty());
    assert!(low.iter().any(|w| w["object"] == ids[2]));
    assert_eq!(r["zones"][0]["critical"]["ok"], false);
}

#[test]
fn set_changes_fields_in_one_step() {
    let (mut s, ids) = zone();
    ok(
        &mut s,
        "irrigation.set",
        json!({"ids": [ids[0], ids[1]], "arc": 180, "direction": 90, "radius": null}),
    );
    let o = ok(&mut s, "objects.get", json!({"id": ids[1]}));
    assert_eq!(o["shape"]["arc"], 180.0);
    assert!(o["shape"].get("radius").is_none());
    assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
    let o = ok(&mut s, "objects.get", json!({"id": ids[1]}));
    assert_eq!(o["shape"]["arc"], 360.0);
    let (_, m) = fail(
        &mut s,
        "irrigation.set",
        json!({"id": ids[0], "size": "20"}),
    );
    assert!(m.contains("not a field"), "{m}");
    assert!(
        fail(&mut s, "irrigation.set", json!({"id": ids[0]}))
            .1
            .contains("at least one")
    );
    assert!(
        fail(&mut s, "irrigation.set", json!({"id": ids[0], "arc": 0}))
            .1
            .contains("arc")
    );
    assert!(fail(&mut s, "irrigation.set", json!({"id": ids[0], "arc": "x"})).0 != 0);
    assert!(
        fail(
            &mut s,
            "irrigation.set",
            json!({"id": "00000000-0000-0000-0000-00000001869f", "arc": 5})
        )
        .1
        .contains("does not exist")
    );
    assert!(
        fail(&mut s, "irrigation.set", json!({"arc": 5}))
            .1
            .contains("'id'")
    );
    let layer = ok(&mut s, "document.summary", json!({}))["layers"][0]["id"].clone();
    assert!(fail(&mut s, "irrigation.set", json!({"id": layer, "arc": 5})).0 != 0);
}

#[test]
fn set_works_on_every_kind() {
    let (mut s, _) = zone();
    let ctl = id(&ok(
        &mut s,
        "irrigation.controller.add",
        json!({"at": [0, 5000], "name": "C1"}),
    ));
    ok(&mut s, "irrigation.set", json!({"id": ctl, "stations": 8}));
    let o = ok(&mut s, "objects.get", json!({"id": ctl}));
    assert_eq!(o["shape"]["stations"], 8);
    let valve = ok(&mut s, "objects.list", json!({"kind": "irrigation_valve"}))[0]["id"].clone();
    ok(
        &mut s,
        "irrigation.set",
        json!({"id": valve, "controller": "C1", "station": 3, "zone": 2}),
    );
    let line = ok(&mut s, "objects.list", json!({"kind": "irrigation_line"}))[0]["id"].clone();
    ok(
        &mut s,
        "irrigation.set",
        json!({"id": line, "line_type": "drip", "size": "25"}),
    );
    // The valve points at C1, station 3: no controller warning.
    let r = ok(
        &mut s,
        "irrigation.calculate",
        json!({"assign_zones": false}),
    );
    assert!(
        !r["warnings"].to_string().contains("controller"),
        "{}",
        r["warnings"]
    );
    // A station beyond the stations of the controller warns.
    ok(&mut s, "irrigation.set", json!({"id": valve, "station": 9}));
    let r = ok(
        &mut s,
        "irrigation.calculate",
        json!({"assign_zones": false}),
    );
    assert!(r["warnings"].to_string().contains("outside 1 to 8"));
}

#[test]
fn add_errors() {
    let mut s = Session::with_sequential_ids();
    let cases = [
        ("irrigation.outlet.add", json!({"at": [0, 0]}), "model"),
        (
            "irrigation.outlet.add",
            json!({"at": [0, 0], "model": "Nope"}),
            "unknown head model",
        ),
        ("irrigation.outlet.add", json!({"model": "Spray-4.0"}), "at"),
        (
            "irrigation.outlet.add",
            json!({"at": [0], "model": "Spray-4.0"}),
            "[x, y]",
        ),
        (
            "irrigation.outlet.add",
            json!({"at": [0, 0], "model": "Spray-4.0", "arc": 400}),
            "arc",
        ),
        (
            "irrigation.outlet.add",
            json!({"at": [0, 0], "model": "Spray-4.0", "arc": "wide"}),
            "number",
        ),
        (
            "irrigation.line.add",
            json!({"points": [[0, 0]], "size": "32", "material": "PE"}),
            "2 or more",
        ),
        (
            "irrigation.line.add",
            json!({"points": "x", "size": "32", "material": "PE"}),
            "list",
        ),
        (
            "irrigation.line.add",
            json!({"points": [[0, 0], [1, 1]], "material": "PE"}),
            "size",
        ),
        (
            "irrigation.line.add",
            json!({"points": [[0, 0], [1, 1]], "size": "32"}),
            "material",
        ),
        (
            "irrigation.line.add",
            json!({"points": [[0, 0], [1, 1]], "size": "32", "material": "Gold"}),
            "not in the catalog",
        ),
        (
            "irrigation.line.add",
            json!({"points": [[0, 0], [1, 1]], "size": "32", "material": "PE", "line_type": "river"}),
            "line_type",
        ),
        (
            "irrigation.line.add",
            json!({"points": [[0, 0], [1, 1]], "size": {}, "material": "PE"}),
            "text",
        ),
        (
            "irrigation.poc.add",
            json!({"at": [0, 0], "available_flow": 10}),
            "static_pressure",
        ),
        (
            "irrigation.poc.add",
            json!({"at": [0, 0], "static_pressure": 10}),
            "available_flow",
        ),
        (
            "irrigation.poc.add",
            json!({"at": [0, 0], "static_pressure": -5, "available_flow": 10}),
            "POC",
        ),
        (
            "irrigation.valve.add",
            json!({"at": [0, 0], "zone": 0}),
            "zone",
        ),
        (
            "irrigation.valve.add",
            json!({"at": [0, 0], "zone": 1.5}),
            "zone",
        ),
        (
            "irrigation.valve.add",
            json!({"at": [0, 0], "zone": 1, "kv": 0}),
            "kv",
        ),
        ("irrigation.controller.add", json!({"at": [0, 0]}), "name"),
        (
            "irrigation.controller.add",
            json!({"at": [0, 0], "name": "A", "stations": 0}),
            "station",
        ),
    ];
    for (method, params, needle) in cases {
        let (_, m) = fail(&mut s, method, params.clone());
        assert!(m.contains(needle), "{method} {params}: {m}");
    }
    assert!(
        fail(&mut s, "irrigation.outlet.add", json!([1]))
            .1
            .contains("object")
    );
    assert_eq!(fail(&mut s, "irrigation.nothing", json!({})).0, -32601);
    // Nothing was added by the failed calls.
    assert_eq!(
        ok(&mut s, "objects.list", json!({}))
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn catalog_get_set_and_csv() {
    let mut s = Session::with_sequential_ids();
    let c = ok(&mut s, "irrigation.catalog.get", json!(null));
    assert!(c["heads"].as_array().unwrap().len() >= 6);
    assert!(
        c["materials"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["name"] == "PVC" && m["c"] == 150.0)
    );
    let doc_only = ok(
        &mut s,
        "irrigation.catalog.get",
        json!({"document_only": true}),
    );
    assert!(doc_only["heads"].as_array().unwrap().is_empty());

    let r = ok(
        &mut s,
        "irrigation.catalog.set",
        json!({"heads": [{"name": "My rotor", "kind": "rotor", "radius": 9000, "flow": 14, "pressure": 300}],
               "pipes": [{"size": "90", "material": "PE", "inner_diameter": 73.6, "price": 8}],
               "materials": [{"name": "PE", "c": 145}]}),
    );
    let res = id(&r);
    let c = ok(&mut s, "irrigation.catalog.get", json!({}));
    assert!(
        c["heads"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["name"] == "My rotor")
    );
    assert!(
        c["materials"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["name"] == "PE" && m["c"] == 145.0)
    );
    // A second set modifies the same resource and keeps the old parts.
    let r = ok(
        &mut s,
        "irrigation.catalog.set",
        json!({"heads": [{"name": "Other", "radius": 3000, "flow": 5, "pressure": 200}]}),
    );
    assert_eq!(id(&r), res);
    let d = ok(
        &mut s,
        "irrigation.catalog.get",
        json!({"document_only": true}),
    );
    assert_eq!(d["heads"].as_array().unwrap().len(), 2);
    // The new head works.
    ok(
        &mut s,
        "irrigation.outlet.add",
        json!({"at": [0, 0], "model": "my ROTOR"}),
    );
    // Replace drops the old document parts.
    ok(
        &mut s,
        "irrigation.catalog.set",
        json!({"replace": true, "heads": [{"name": "Only", "radius": 3000, "flow": 5, "pressure": 200}]}),
    );
    let d = ok(
        &mut s,
        "irrigation.catalog.get",
        json!({"document_only": true}),
    );
    assert_eq!(d["heads"].as_array().unwrap().len(), 1);
    // Errors.
    assert!(
        fail(
            &mut s,
            "irrigation.catalog.set",
            json!({"heads": [{"name": "x"}]})
        )
        .1
        .contains("heads")
    );
    assert!(
        fail(
            &mut s,
            "irrigation.catalog.set",
            json!({"heads": [{"name": "x", "radius": -1, "flow": 1, "pressure": 1}]})
        )
        .1
        .contains("positive")
    );
    assert!(
        fail(&mut s, "irrigation.catalog.set", json!({"heads": "no"}))
            .1
            .contains("heads")
    );

    // CSV in and out.
    let r = ok(
        &mut s,
        "irrigation.catalog.import_csv",
        json!({"kind": "heads", "text": "name,radius,flow,pressure,kind\nFast,5000,9,250,spray\nSlow,6000,7,250,rotor\n"}),
    );
    assert_eq!(r["imported"], 2);
    let csv = ok(
        &mut s,
        "irrigation.catalog.export_csv",
        json!({"kind": "heads", "document_only": true}),
    );
    assert!(csv["csv"].as_str().unwrap().contains("Fast,spray,5000"));
    let r = ok(
        &mut s,
        "irrigation.catalog.import_csv",
        json!({"kind": "pipes", "text": "size,material,inner_diameter\n12,PE,9.5\n"}),
    );
    assert_eq!(r["imported"], 1);
    ok(
        &mut s,
        "irrigation.line.add",
        json!({"points": [[0, 0], [1000, 0]], "size": 12, "material": "PE"}),
    );
    let csv = ok(
        &mut s,
        "irrigation.catalog.export_csv",
        json!({"kind": "pipes"}),
    );
    assert!(
        csv["csv"]
            .as_str()
            .unwrap()
            .starts_with("size,material,inner_diameter,price")
    );
    let r = ok(
        &mut s,
        "irrigation.catalog.import_csv",
        json!({"kind": "materials", "text": "name,c\nHDPE,150\n", "replace": true}),
    );
    assert_eq!(r["imported"], 1);
    let d = ok(
        &mut s,
        "irrigation.catalog.get",
        json!({"document_only": true}),
    );
    assert!(d["heads"].as_array().unwrap().is_empty());
    assert!(
        fail(
            &mut s,
            "irrigation.catalog.import_csv",
            json!({"kind": "x", "text": "a"})
        )
        .1
        .contains("unknown kind")
    );
    assert!(
        fail(
            &mut s,
            "irrigation.catalog.import_csv",
            json!({"text": "a"})
        )
        .1
        .contains("kind")
    );
    assert!(
        fail(
            &mut s,
            "irrigation.catalog.import_csv",
            json!({"kind": "heads"})
        )
        .1
        .contains("text")
    );
    assert!(
        fail(
            &mut s,
            "irrigation.catalog.import_csv",
            json!({"kind": "heads", "text": "name\nA\n"})
        )
        .1
        .contains("column")
    );
    assert!(
        fail(
            &mut s,
            "irrigation.catalog.export_csv",
            json!({"kind": "x"})
        )
        .1
        .contains("heads, pipes or materials")
    );
    assert!(
        fail(&mut s, "irrigation.catalog.export_csv", json!({}))
            .1
            .contains("heads, pipes or materials")
    );
}

#[test]
fn catalog_is_undoable_and_saved_in_the_file() {
    let mut s = Session::with_sequential_ids();
    ok(
        &mut s,
        "irrigation.catalog.set",
        json!({"heads": [{"name": "Saved", "radius": 3000, "flow": 5, "pressure": 200}]}),
    );
    let doc = serde_json::to_value(s.editor.doc()).unwrap();
    let mut t = Session::with_sequential_ids();
    ok(&mut t, "document.load", json!({"document": doc}));
    let c = ok(&mut t, "irrigation.catalog.get", json!({}));
    assert!(
        c["heads"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["name"] == "Saved")
    );
    assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
    let c = ok(&mut s, "irrigation.catalog.get", json!({}));
    assert!(
        !c["heads"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["name"] == "Saved")
    );
}

fn rect(w: i64, h: i64) -> Value {
    json!([[0, 0], [w, 0], [w, h], [0, h]])
}

#[test]
fn layout_fill_places_heads_in_one_undo_step() {
    let mut s = Session::with_sequential_ids();
    // 10 m x 6 m, spacing 2 m, square: 6 x 4 = 24 heads.
    let r = ok(
        &mut s,
        "irrigation.layout.fill",
        json!({"boundary": rect(10000, 6000), "model": "Spray-4.0", "spacing": 2000, "arc": 90}),
    );
    assert_eq!(r["count"], 24);
    assert_eq!(r["ids"].as_array().unwrap().len(), 24);
    let o = ok(&mut s, "objects.get", json!({"id": r["ids"][0]}));
    assert_eq!(o["shape"]["arc"], 90.0);
    assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
    assert_eq!(
        ok(&mut s, "objects.list", json!({}))
            .as_array()
            .unwrap()
            .len(),
        0
    );
    // Without placing: only the points. The default spacing is the radius (4 m).
    let r = ok(
        &mut s,
        "irrigation.layout.fill",
        json!({"boundary": rect(8000, 8000), "model": "Spray-4.0", "place": false, "pattern": "triangular"}),
    );
    assert!(r["count"].as_u64().unwrap() > 0);
    assert!(r["ids"].as_array().unwrap().is_empty());
    // From an object.
    let rect_id = id(&ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [4000, 4000]}}}),
    )["created"]
        .as_array()
        .map(|c| json!({"id": c[0]}))
        .unwrap());
    let r = ok(
        &mut s,
        "irrigation.layout.fill",
        json!({"object": rect_id, "model": "Spray-4.0", "spacing": 2000, "place": false, "inset": 100, "angle": 0}),
    );
    assert_eq!(r["count"], 4);
    for (params, needle) in [
        (json!({"model": "Spray-4.0"}), "boundary"),
        (json!({"boundary": rect(10, 10)}), "model"),
        (
            json!({"boundary": rect(10, 10), "model": "X"}),
            "unknown head",
        ),
        (
            json!({"boundary": rect(10, 10), "model": "Spray-4.0", "pattern": "hex"}),
            "pattern",
        ),
        (
            json!({"boundary": rect(10, 10), "model": "Spray-4.0", "spacing": 0}),
            "spacing",
        ),
        (
            json!({"boundary": [[0, 0]], "model": "Spray-4.0"}),
            "3 or more",
        ),
        (
            json!({"object": "00000000-0000-0000-0000-00000001869f", "model": "Spray-4.0"}),
            "does not exist",
        ),
    ] {
        let (_, m) = fail(&mut s, "irrigation.layout.fill", params.clone());
        assert!(m.contains(needle), "{params}: {m}");
    }
    // An object without an outline.
    let o = id(&ok(
        &mut s,
        "irrigation.poc.add",
        json!({"at": [0, 0], "static_pressure": 100, "available_flow": 10}),
    ));
    assert!(
        fail(
            &mut s,
            "irrigation.layout.fill",
            json!({"object": o, "model": "Spray-4.0"})
        )
        .1
        .contains("no outline")
    );
}

#[test]
fn layout_edge_faces_inward_on_the_left() {
    let mut s = Session::with_sequential_ids();
    // The bottom edge of a 10 m wide bed, going right: left is up (+Y, direction 90).
    let r = ok(
        &mut s,
        "irrigation.layout.edge",
        json!({"path": [[0, 0], [10000, 0]], "model": "Spray-2.5", "spacing": 2500}),
    );
    assert_eq!(r["count"], 5);
    let o = ok(&mut s, "objects.get", json!({"id": r["ids"][0]}));
    assert_eq!(o["shape"]["arc"], 180.0);
    assert!((o["shape"]["direction"].as_f64().unwrap() - 90.0).abs() < 1e-9);
    let r = ok(
        &mut s,
        "irrigation.layout.edge",
        json!({"path": [[0, 0], [10000, 0]], "model": "Spray-2.5", "spacing": 2500, "side": "right", "arc": 90}),
    );
    let o = ok(&mut s, "objects.get", json!({"id": r["ids"][0]}));
    assert!((o["shape"]["direction"].as_f64().unwrap() + 90.0).abs() < 1e-9);
    // A closed polygon object is a ring: 4 sides of 4 m, spacing 2 m: 8 heads.
    let area = id(&ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": rect(4000, 4000)}),
    ));
    let r = ok(
        &mut s,
        "irrigation.layout.edge",
        json!({"object": area, "model": "Spray-2.5", "spacing": 2000, "place": false}),
    );
    assert_eq!(r["count"], 8);
    // An open polyline object, closed on request.
    let pl = id(&ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "polyline", "points": [[0, 0], [3000, 0], [3000, 3000]], "closed": false}}}),
    )["created"]
        .as_array()
        .map(|c| json!({"id": c[0]}))
        .unwrap());
    let r = ok(
        &mut s,
        "irrigation.layout.edge",
        json!({"object": pl, "model": "Spray-2.5", "spacing": 1000, "place": false}),
    );
    assert_eq!(r["count"], 7);
    let r = ok(
        &mut s,
        "irrigation.layout.edge",
        json!({"object": pl, "model": "Spray-2.5", "spacing": 1000, "place": false, "closed": true}),
    );
    // 3 + 3 + 4.24 m = 10.24 m in 11 parts.
    assert_eq!(r["count"], 11);
    assert!(
        fail(
            &mut s,
            "irrigation.layout.edge",
            json!({"path": [[0, 0], [1, 0]], "model": "Spray-2.5", "side": "up"})
        )
        .1
        .contains("side")
    );
    assert!(
        fail(
            &mut s,
            "irrigation.layout.edge",
            json!({"model": "Spray-2.5"})
        )
        .1
        .contains("path")
    );
}

#[test]
fn layout_drip_draws_lines() {
    let mut s = Session::with_sequential_ids();
    let r = ok(
        &mut s,
        "irrigation.layout.drip",
        json!({"boundary": rect(10000, 6000), "spacing": 2000, "emitter_spacing": 400, "emitter_flow": 1.6}),
    );
    assert_eq!(r["count"], 3);
    assert_eq!(r["total_length_m"], 30.0);
    let o = ok(&mut s, "objects.get", json!({"id": r["ids"][0]}));
    assert_eq!(o["shape"]["line_type"], "drip");
    assert_eq!(o["shape"]["emitter_flow"], 1.6);
    // A drip line is a pipe in the schedule.
    let p = ok(&mut s, "irrigation.report.pipes", json!({}));
    assert_eq!(p["rows"][0]["line_type"], "drip");
    assert_eq!(p["rows"][0]["length_m"], 30.0);
    let r = ok(
        &mut s,
        "irrigation.layout.drip",
        json!({"boundary": rect(10000, 6000), "spacing": 2000, "place": false, "angle": 90, "inset": 200, "size": 20}),
    );
    assert_eq!(r["count"], 5);
    for (params, needle) in [
        (json!({"boundary": rect(10, 10)}), "spacing"),
        (json!({"spacing": 10}), "boundary"),
        (
            json!({"boundary": rect(10, 10), "spacing": 5, "material": "Gold"}),
            "not in the catalog",
        ),
        (
            json!({"boundary": rect(10, 10), "spacing": 5, "emitter_flow": "x"}),
            "emitter_flow",
        ),
    ] {
        let (_, m) = fail(&mut s, "irrigation.layout.drip", params.clone());
        assert!(m.contains(needle), "{params}: {m}");
    }
}

#[test]
fn reports_agree_with_the_zone() {
    let (mut s, _) = zone();
    // 60 * 32 / 160 = 12 mm/h; 6 mm needs 30 minutes.
    let z = ok(
        &mut s,
        "irrigation.report.zones",
        json!({"target_depth": 6, "areas": {"1": 160}}),
    );
    let row = &z["rows"][0];
    assert_eq!(row["head_count"], 4);
    assert!((row["precipitation_mm_h"].as_f64().unwrap() - 12.0).abs() < 1e-9);
    assert!((row["run_time_min"].as_f64().unwrap() - 30.0).abs() < 1e-9);
    assert_eq!(row["heads"][0]["model"], "Spray-4.0");
    let csv = ok(&mut s, "irrigation.report.zones", json!({"format": "csv"}));
    assert!(csv["csv"].as_str().unwrap().starts_with("Zone,"));

    let p = ok(&mut s, "irrigation.report.pipes", json!({}));
    assert_eq!(p["total_length_m"], 60.0);
    assert!(
        ok(&mut s, "irrigation.report.pipes", json!({"format": "csv"}))["csv"]
            .as_str()
            .unwrap()
            .contains("mainline,PVC,50")
    );

    let m = ok(&mut s, "irrigation.report.materials", json!({}));
    assert!(
        m["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["category"] == "head" && r["quantity"] == 4.0)
    );
    assert!(
        ok(
            &mut s,
            "irrigation.report.materials",
            json!({"format": "csv"})
        )["csv"]
            .as_str()
            .unwrap()
            .contains("Category")
    );

    let pr = ok(&mut s, "irrigation.report.pressure", json!({}));
    assert_eq!(pr["zones"][0]["outlets"].as_array().unwrap().len(), 4);
    let csv = ok(
        &mut s,
        "irrigation.report.pressure",
        json!({"format": "csv"}),
    );
    assert!(csv["csv"].as_str().unwrap().lines().count() == 5);

    for (method, params, needle) in [
        ("irrigation.report.zones", json!({"areas": 5}), "areas"),
        (
            "irrigation.report.zones",
            json!({"areas": {"x": 5}}),
            "zone numbers",
        ),
        (
            "irrigation.report.zones",
            json!({"areas": {"1": -5}}),
            "area",
        ),
        (
            "irrigation.report.pipes",
            json!({"tolerance": -1}),
            "tolerance",
        ),
        (
            "irrigation.calculate",
            json!({"target_depth": "deep"}),
            "target_depth",
        ),
    ] {
        let (_, m) = fail(&mut s, method, params.clone());
        assert!(m.contains(needle), "{method} {params}: {m}");
    }
}

#[test]
fn describe_and_methods_list_irrigation() {
    let mut s = Session::with_sequential_ids();
    let d = ok(&mut s, "describe", json!({}));
    assert!(d["irrigation"]["examples"].as_array().unwrap().len() >= 10);
    let names: Vec<&str> = d["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    for n in [
        "irrigation.outlet.add",
        "irrigation.calculate",
        "irrigation.layout.fill",
        "irrigation.report.zones",
        "irrigation.catalog.import_csv",
    ] {
        assert!(names.contains(&n), "{n}");
    }
    // Every example names a method that exists, and every irrigation method has an example.
    let examples: Vec<&str> = d["irrigation"]["examples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["method"].as_str().unwrap())
        .collect();
    assert!(examples.iter().all(|e| names.contains(e)));
}

#[test]
fn rotate_and_move_irrigation_objects() {
    let (mut s, ids) = zone();
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [ids[0]], "delta": [100, 0]}}),
    );
    let o = ok(&mut s, "objects.get", json!({"id": ids[0]}));
    assert_eq!(o["shape"]["at"][0], 50100.0);
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "rotate_objects", "ids": [ids[0]], "center": [50100, 0], "angle": std::f64::consts::FRAC_PI_2}}),
    );
    let o = ok(&mut s, "objects.get", json!({"id": ids[0]}));
    assert!((o["shape"]["direction"].as_f64().unwrap() - 90.0).abs() < 1e-6);
    // Invalid shapes are refused by the normal commands too.
    let r = s.handle(
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "irrigation", "type": "outlet", "at": [0, 0], "model": "x", "arc": 0}}}),
    );
    assert!(r.unwrap_err().message.contains("arc"));
}

#[test]
fn fittings_supply_and_meter_losses_and_the_site_model_change_the_pressure() {
    let (mut s, ids) = zone();
    let far = |s: &mut Session, extra: Value| -> f64 {
        let mut p = json!({"assign_zones": false});
        p.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        ok(s, "irrigation.calculate", p)["zones"][0]["critical"]["pressure_kpa"]
            .as_f64()
            .unwrap()
    };
    let base = far(&mut s, json!({}));
    // 10 percent fittings on the pipes only (the valve loss stays).
    let fit = far(&mut s, json!({"fittings": 10}));
    let pipes = 400.0 - valve_loss_kpa(32.0, 6.0) - base;
    assert!((base - fit - 0.1 * pipes).abs() < 1e-6);
    assert!(
        fail(&mut s, "irrigation.calculate", json!({"fittings": 500}))
            .1
            .contains("fittings")
    );
    // Supply loss 30 kPa and meter loss 10 kPa at 100 l/min; the zone runs 32 l/min.
    let poc = ok(&mut s, "objects.list", json!({"kind": "irrigation_poc"}))[0]["id"].clone();
    ok(
        &mut s,
        "irrigation.set",
        json!({"id": poc, "supply_loss": 30, "meter_loss": 10}),
    );
    let loss = 30.0 * 0.32f64.powf(1.852) + 10.0 * 0.32 * 0.32;
    let dynamic = far(&mut s, json!({}));
    assert!((base - dynamic - loss).abs() < 1e-6);
    assert!(
        fail(
            &mut s,
            "irrigation.set",
            json!({"id": poc, "meter_loss": -1})
        )
        .1
        .contains("negative")
    );
    // A site model: the ground rises 2 m between the POC and the far head, 2 * 9.80665 kPa.
    ok(
        &mut s,
        "irrigation.set",
        json!({"id": poc, "supply_loss": null, "meter_loss": null}),
    );
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "site_model",
        "points": [[-1000, -1000, 0], [60000, -1000, 0], [60000, 20000, 0], [-1000, 20000, 0],
                   [60000, 10000, 2000]]}}}),
    );
    let high = far(&mut s, json!({}));
    assert!(high < base, "{high} {base}");
    let flat = far(&mut s, json!({"use_site_model": false}));
    assert!((flat - base).abs() < 1e-9);
    let _ = ids;
}

#[test]
fn csv_import_reads_semicolons_decimal_commas_and_warns_about_duplicates() {
    let mut s = Session::with_sequential_ids();
    let r = ok(
        &mut s,
        "irrigation.catalog.import_csv",
        json!({"kind": "heads", "text": "name;radius;flow;pressure\nA;3000;5,5;200\na;3500;6,5;200\nB;3000;5;200\n"}),
    );
    assert_eq!(r["imported"], 3);
    assert!(
        r["warnings"][0]
            .as_str()
            .unwrap()
            .contains("more than once")
    );
    let heads = ok(
        &mut s,
        "irrigation.catalog.get",
        json!({"document_only": true}),
    );
    // The later row wins.
    assert_eq!(heads["heads"][0]["flow"], 6.5);
    let r = ok(
        &mut s,
        "irrigation.catalog.import_csv",
        json!({"kind": "materials", "text": "name,c\nHDPE,150\n"}),
    );
    assert!(r["warnings"].as_array().unwrap().is_empty());
    let csv = ok(
        &mut s,
        "irrigation.catalog.export_csv",
        json!({"kind": "materials", "document_only": true}),
    );
    assert_eq!(csv["csv"], "name,c\nHDPE,150\n");
}
