//! Extrude, sweep and 3D polygon (3D-03) through the JSON-RPC interface.

use kadakas_api::{Session, SessionLimits};
use serde_json::{Value, json};

fn call(s: &mut Session, method: &str, params: Value) -> Result<Value, (i64, String)> {
    s.handle(method, params).map_err(|e| (e.code, e.message))
}

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    call(s, method, params).unwrap_or_else(|e| panic!("{method}: {e:?}"))
}

fn err(s: &mut Session, method: &str, params: Value) -> String {
    call(s, method, params).unwrap_err().1
}

/// Adds a shape with the `execute` method and gives the ID.
fn add(s: &mut Session, shape: Value) -> Value {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape": shape}}),
    )["created"][0]
        .clone()
}

fn rect(x: f64, y: f64, w: f64, d: f64) -> Value {
    json!([[x, y], [x + w, y], [x + w, y + d], [x, y + d]])
}

#[test]
fn an_extrude_is_added_read_changed_and_undone() {
    let mut s = Session::with_sequential_ids();
    let r = ok(
        &mut s,
        "extrude.add",
        json!({"profile": rect(0.0, 0.0, 2000.0, 1000.0), "holes": [rect(500.0, 250.0, 400.0, 400.0)], "height": 450, "name": "Planter"}),
    );
    let id = r["id"].clone();
    let want = (2.0e6 - 1.6e5) * 450.0 / 1e9;
    assert!((r["quantities"]["volume_m3"].as_f64().unwrap() - want).abs() < 1e-9);
    let g = ok(&mut s, "extrude.get", json!({"id": id}));
    assert_eq!(g["height"], 450.0);
    assert_eq!(g["name"], "Planter");
    assert_eq!(g["holes"].as_array().unwrap().len(), 1);
    assert_eq!(g["bounds_mm"]["max"], json!([2000.0, 1000.0, 450.0]));
    assert!((g["quantities"]["profile_area_m2"].as_f64().unwrap() - 1.84).abs() < 1e-9);
    ok(
        &mut s,
        "extrude.set",
        json!({"id": id, "height": 900, "base": 100, "holes": null}),
    );
    let g = ok(&mut s, "extrude.get", json!({"id": id}));
    assert_eq!(
        (g["height"].clone(), g["base"].clone()),
        (json!(900.0), json!(100.0))
    );
    assert_eq!(g["holes"], json!([]));
    assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
    assert_eq!(
        ok(&mut s, "extrude.get", json!({"id": id}))["height"],
        450.0
    );
    // The shape is saved with the document and read back.
    let doc = ok(&mut s, "document.get", json!({}));
    assert!(doc.to_string().contains("\"kind\":\"extrude\""));
}

#[test]
fn bad_extrudes_say_why() {
    let mut s = Session::with_sequential_ids();
    let sq = rect(0.0, 0.0, 100.0, 100.0);
    assert!(err(&mut s, "extrude.add", json!({"height": 5})).contains("profile"));
    assert!(err(&mut s, "extrude.add", json!({"profile": sq})).contains("height"));
    assert!(err(&mut s, "extrude.add", json!({"profile": sq, "height": 0})).contains("zero"));
    assert!(
        err(
            &mut s,
            "extrude.add",
            json!({"profile": sq, "height": 5, "colour": 1})
        )
        .contains("unknown field")
    );
    assert!(
        err(
            &mut s,
            "extrude.add",
            json!({"profile": [[0, 0], [10, 10], [10, 0], [0, 10]], "height": 5})
        )
        .contains("itself")
    );
    assert!(
        err(
            &mut s,
            "extrude.add",
            json!({"profile": [[0, 0], [5, 5]], "height": 5})
        )
        .contains("3 vertices")
    );
    assert!(
        err(
            &mut s,
            "extrude.add",
            json!({"profile": sq, "height": 5, "holes": [rect(80.0, 80.0, 50.0, 50.0)]})
        )
        .contains("hole")
    );
    assert!(
        err(
            &mut s,
            "extrude.add",
            json!({"profile": sq, "height": 1e30})
        )
        .contains("range")
    );
    assert!(err(&mut s, "extrude.add", json!(5)).contains("object"));
    assert!(err(&mut s, "extrude.get", json!({})).contains("id"));
    assert!(
        err(
            &mut s,
            "extrude.get",
            json!({"id": "00000000-0000-0000-0000-0000000000aa"})
        )
        .contains("does not exist")
    );
    let w = ok(
        &mut s,
        "polygon3d.add",
        json!({"points": [[0, 0, 0], [1, 0, 0], [1, 1, 0]]}),
    )["id"]
        .clone();
    assert!(err(&mut s, "extrude.get", json!({"id": w})).contains("not an extrude"));
    assert!(err(&mut s, "extrude.set", json!({"id": w, "height": 3})).contains("not an extrude"));
    assert!(err(&mut s, "sweep.get", json!({"id": w})).contains("not a sweep"));
    assert!(err(&mut s, "sweep.set", json!({"id": w})).contains("not a sweep"));
}

#[test]
fn a_sweep_has_segments_and_the_cap_is_reported() {
    let mut s = Session::with_sequential_ids();
    let r = ok(
        &mut s,
        "sweep.add",
        json!({"profile": rect(0.0, 0.0, 1000.0, 2000.0), "origin": [100, 200, 50], "sweep_angle": 180, "segment_angle": 10}),
    );
    let id = r["id"].clone();
    assert_eq!(r["warnings"], json!([]));
    let g = ok(&mut s, "sweep.get", json!({"id": id}));
    assert_eq!(g["segments"], 18);
    assert_eq!(g["origin"], json!([100.0, 200.0, 50.0]));
    let vol = g["quantities"]["volume_m3"].as_f64().unwrap();
    assert!(
        (vol - std::f64::consts::PI * 1.0 * 2.0 / 2.0).abs() < 0.05,
        "{vol}"
    );
    assert_eq!(g["bounds_mm"]["min"][2], 50.0);
    // A tiny segment angle is lowered, with a warning.
    let r = ok(
        &mut s,
        "sweep.set",
        json!({"id": id, "sweep_angle": 3600, "segment_angle": 0.1}),
    );
    assert_eq!(r["warnings"].as_array().unwrap().len(), 1);
    assert_eq!(ok(&mut s, "sweep.get", json!({"id": id}))["segments"], 720);
    // A spiral.
    let sp = ok(
        &mut s,
        "sweep.add",
        json!({"profile": rect(800.0, 0.0, 400.0, 150.0), "sweep_angle": 720, "pitch": 1500}),
    );
    let b = ok(&mut s, "sweep.get", json!({"id": sp["id"]}));
    assert!((b["bounds_mm"]["max"][2].as_f64().unwrap() - 3150.0).abs() < 1e-6);
    // An open profile is a surface without a volume.
    let open = ok(
        &mut s,
        "sweep.add",
        json!({"profile": [[100, 0], [500, 800]], "closed": false}),
    );
    assert!(open["quantities"]["volume_m3"].is_null());
    assert!(open["quantities"]["surface_area_m2"].as_f64().unwrap() > 0.0);
}

#[test]
fn bad_sweeps_say_why() {
    let mut s = Session::with_sequential_ids();
    let sq = rect(100.0, 0.0, 100.0, 100.0);
    assert!(err(&mut s, "sweep.add", json!({})).contains("profile"));
    assert!(
        err(
            &mut s,
            "sweep.add",
            json!({"profile": rect(-50.0, 0.0, 100.0, 100.0)})
        )
        .contains("axis")
    );
    assert!(
        err(
            &mut s,
            "sweep.add",
            json!({"profile": sq, "sweep_angle": 0})
        )
        .contains("sweep angle")
    );
    assert!(
        err(
            &mut s,
            "sweep.add",
            json!({"profile": sq, "sweep_angle": 5000})
        )
        .contains("sweep angle")
    );
    assert!(
        err(
            &mut s,
            "sweep.add",
            json!({"profile": sq, "segment_angle": 0})
        )
        .contains("segment angle")
    );
    assert!(
        err(
            &mut s,
            "sweep.add",
            json!({"profile": sq, "axis": [0, 0, 0]})
        )
        .contains("axis")
    );
    assert!(err(&mut s, "sweep.add", json!({"profile": sq, "pitch": 1e30})).contains("range"));
    assert!(err(&mut s, "sweep.add", json!({"profile": sq, "speed": 1})).contains("unknown field"));
    assert!(err(&mut s, "sweep.add", json!({"profile": "x"})).len() > 3);
}

#[test]
fn model_extrude_and_model_sweep_convert_the_selection() {
    let mut s = Session::with_sequential_ids();
    let a = add(
        &mut s,
        json!({"kind": "polyline", "closed": true, "points": rect(0.0, 0.0, 1000.0, 1000.0)}),
    );
    let t = add(
        &mut s,
        json!({"kind": "text", "at": [0, 0], "text": "t", "height": 10}),
    );
    let dry = ok(
        &mut s,
        "extrude.from_shapes",
        json!({"ids": [a, t], "height": 700, "dry_run": true}),
    );
    assert_eq!(dry["created"].as_array().unwrap().len(), 1);
    assert_eq!(dry["skipped"][0]["reason"], "a text is not a closed shape");
    assert_eq!(s.editor.doc().objects.len(), 2, "a dry run changes nothing");
    let r = ok(
        &mut s,
        "extrude.from_shapes",
        json!({"ids": [a], "height": 700}),
    );
    let new = r["created"][0]["id"].clone();
    assert_eq!(
        ok(&mut s, "extrude.get", json!({"id": new}))["height"],
        700.0
    );
    assert_eq!(s.selection.len(), 1);
    assert!(err(&mut s, "extrude.from_shapes", json!({"ids": []})).contains("select"));
    assert!(
        err(
            &mut s,
            "extrude.from_shapes",
            json!({"ids": [new], "height": 0})
        )
        .contains("height")
    );
    // Sweep: a pot drawn as an elevation, then a ring around an axis line.
    let pot = add(
        &mut s,
        json!({"kind": "polyline", "closed": true, "points": [[5000, 100], [5600, 100], [7000, 3100], [5000, 3100]]}),
    );
    let r = ok(
        &mut s,
        "sweep.from_shapes",
        json!({"ids": [pot], "segment_angle": 20}),
    );
    let swept = r["created"][0]["id"].clone();
    let g = ok(&mut s, "sweep.get", json!({"id": swept}));
    assert_eq!(g["origin"], json!([5000.0, 100.0, 0.0]));
    assert_eq!(g["segments"], 18);
    let axis = add(&mut s, json!({"kind": "line", "a": [0, 0], "b": [4000, 0]}));
    let ring = add(
        &mut s,
        json!({"kind": "rect", "min": [1000, 500], "max": [1600, 1100]}),
    );
    let r = ok(
        &mut s,
        "sweep.from_shapes",
        json!({"ids": [ring], "axis_id": axis, "sweep_angle": 90}),
    );
    assert_eq!(r["created"].as_array().unwrap().len(), 1);
    assert!(
        s.editor
            .doc()
            .object(serde_json::from_value(axis.clone()).unwrap())
            .is_some()
    );
    assert!(
        err(
            &mut s,
            "sweep.from_shapes",
            json!({"ids": [ring], "axis_id": axis, "axis_at": [0, 0]})
        )
        .contains("not both")
    );
    assert!(
        err(
            &mut s,
            "sweep.from_shapes",
            json!({"ids": [ring], "sweep_angle": -1})
        )
        .contains("sweep_angle")
    );
}

#[test]
fn a_3d_polygon_has_heights_and_can_be_a_face() {
    let mut s = Session::with_sequential_ids();
    let r = ok(
        &mut s,
        "polygon3d.add",
        json!({"points": [[0, 0, 0], [4000, 0, 300], [4000, 3000, 600], [0, 3000, 300]], "fill": [160, 184, 144, 255], "name": "Deck"}),
    );
    let id = r["id"].clone();
    let g = ok(&mut s, "polygon3d.get", json!({"id": id}));
    assert_eq!(g["z_range_mm"], json!([0.0, 600.0]));
    assert_eq!(g["face"], true);
    assert_eq!(g["area_plan_m2"], 12.0);
    ok(
        &mut s,
        "polygon3d.set",
        json!({"id": id, "raise": 1000, "closed": false}),
    );
    let g = ok(&mut s, "polygon3d.get", json!({"id": id}));
    assert_eq!(
        (
            g["z_range_mm"].clone(),
            g["closed"].clone(),
            g["face"].clone()
        ),
        (json!([1000.0, 1600.0]), json!(false), json!(false))
    );
    ok(
        &mut s,
        "polygon3d.set",
        json!({"id": id, "points": [[0, 0, 5], [1, 1, 6]]}),
    );
    assert_eq!(
        ok(&mut s, "polygon3d.get", json!({"id": id}))["points"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(err(&mut s, "polygon3d.set", json!({"id": id, "closed": true})).contains("closed"));
    assert!(err(&mut s, "polygon3d.add", json!({})).contains("points"));
    assert!(
        err(
            &mut s,
            "polygon3d.add",
            json!({"points": [[0, 0, 0], [1, 1, 1]]})
        )
        .contains("closed")
    );
    assert!(
        err(
            &mut s,
            "polygon3d.add",
            json!({"points": [[0, 0]], "closed": false})
        )
        .len()
            > 3
    );
    assert!(
        err(
            &mut s,
            "polygon3d.add",
            json!({"points": [[0, 0, 0], [1, 1, 1]], "closed": false, "x": 1})
        )
        .contains("unknown field")
    );
    assert!(err(&mut s, "polygon3d.set", json!({"id": id, "foo": 1})).contains("unknown field"));
    assert!(err(&mut s, "polygon3d.set", json!({"id": id, "raise": "high"})).contains("raise"));
    let ex = ok(
        &mut s,
        "extrude.add",
        json!({"profile": rect(0.0, 0.0, 10.0, 10.0), "height": 5}),
    )["id"]
        .clone();
    assert!(err(&mut s, "polygon3d.get", json!({"id": ex})).contains("not a 3D polygon"));
    assert!(err(&mut s, "polygon3d.set", json!({"id": ex})).contains("not a 3D polygon"));
}

#[test]
fn solids_render_export_and_respect_the_object_limit() {
    let dir = std::env::temp_dir().join(format!("kadakas-solids-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::with_sequential_ids();
    ok(
        &mut s,
        "extrude.add",
        json!({"profile": rect(0.0, 0.0, 2000.0, 1000.0), "height": 500, "name": "Box"}),
    );
    ok(
        &mut s,
        "sweep.add",
        json!({"profile": rect(0.0, 0.0, 400.0, 900.0), "origin": [5000, 0, 0], "name": "Column"}),
    );
    let glb = dir.join("s.glb");
    let r = ok(&mut s, "export.file", json!({"path": glb, "format": "glb"}));
    assert_eq!(r["exported"], 2);
    let bytes = std::fs::read(&glb).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("\"kind\":\"extrude\"") && text.contains("\"kind\":\"sweep\""));
    let ifc = dir.join("s.ifc");
    ok(&mut s, "export.file", json!({"path": ifc, "format": "ifc"}));
    let ifc_text = std::fs::read_to_string(&ifc).unwrap();
    assert!(
        ifc_text.contains("IFCEXTRUDEDAREASOLID") && ifc_text.contains("IFCTRIANGULATEDFACESET")
    );
    let dxf = dir.join("s.dxf");
    ok(&mut s, "export.file", json!({"path": dxf, "format": "dxf"}));
    assert!(
        std::fs::read_to_string(&dxf)
            .unwrap()
            .contains("AcDbPolyFaceMesh")
    );
    let png = ok(&mut s, "render.png", json!({"width": 64, "height": 48}));
    assert!(png.to_string().len() > 100);
    let _ = std::fs::remove_dir_all(&dir);
    // The limit of objects counts solids as it counts all objects.
    let mut limited = Session::with_sequential_ids();
    limited.set_limits(SessionLimits {
        max_objects: 1,
        ..SessionLimits::default()
    });
    ok(
        &mut limited,
        "extrude.add",
        json!({"profile": rect(0.0, 0.0, 10.0, 10.0), "height": 5}),
    );
    assert!(
        err(
            &mut limited,
            "sweep.add",
            json!({"profile": rect(1.0, 0.0, 10.0, 10.0)})
        )
        .contains("at most")
    );
    assert_eq!(limited.editor.doc().objects.len(), 1);
}

#[test]
fn describe_lists_the_methods_and_the_solids_text() {
    let mut s = Session::with_sequential_ids();
    let d = ok(&mut s, "describe", json!({}));
    let names: Vec<&str> = d["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    for n in [
        "extrude.add",
        "extrude.set",
        "extrude.get",
        "extrude.from_shapes",
        "sweep.add",
        "sweep.set",
        "sweep.get",
        "sweep.from_shapes",
        "polygon3d.add",
        "polygon3d.set",
        "polygon3d.get",
    ] {
        assert!(names.contains(&n), "{n}");
    }
    assert!(
        d["solids"]["solids"]
            .as_str()
            .unwrap()
            .contains("Extrude Along Path is not built")
    );
    assert_eq!(d["solids"]["examples"].as_array().unwrap().len(), 5);
}
