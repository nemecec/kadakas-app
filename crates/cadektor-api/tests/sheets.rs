//! The drawing documentation methods: sheets, viewports, overrides, annotations, title blocks,
//! saved views and output.

use cadektor_api::{METHODS, Session};
use serde_json::{Value, json};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    match s.handle(m, p) {
        Ok(v) => v,
        Err(e) => panic!("{m} failed: {}", e.message),
    }
}

fn fail(s: &mut Session, m: &str, p: Value) -> String {
    match s.handle(m, p) {
        Ok(v) => panic!("{m} should fail, got {v}"),
        Err(e) => e.message,
    }
}

fn exec(s: &mut Session, cmd: Value) -> Value {
    call(s, "execute", json!({"command": cmd}))
}

/// A session with a drawing on the first layer and a class "Trees".
fn model() -> Session {
    let mut s = Session::default();
    exec(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [20000, 10000]}, "attrs": {"pen": [200, 0, 0, 255]}}),
    );
    exec(
        &mut s,
        json!({"op": "add_class", "name": "Trees", "attrs": {"pen": [0, 0, 255, 255]}}),
    );
    exec(
        &mut s,
        json!({"op": "add_object", "class": 3, "shape": {"kind": "circle", "center": [5000, 5000], "radius": 1500}}),
    );
    s
}

/// The class override of one class in a viewport JSON. The viewport stores the visibility of every class.
fn class_ov(vp: &Value, class: u64) -> Value {
    vp["class_overrides"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e[0] == class)
        .map_or(Value::Null, |e| e[1].clone())
}

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cadektor-api-sheets-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn every_method_is_listed_and_described() {
    let names: Vec<&str> = METHODS.iter().map(|(n, _)| *n).collect();
    for m in [
        "sheet.list",
        "sheet.create",
        "sheet.set",
        "sheet.render",
        "project.get",
        "project.set",
        "viewport.create",
        "viewport.get",
        "viewport.list",
        "viewport.update",
        "viewport.set_override",
        "viewport.annotate",
        "title_block.create",
        "title_block.update",
        "title_block.resolve",
        "saved_view.list",
        "saved_view.save",
        "saved_view.restore",
        "saved_view.delete",
        "export.file",
    ] {
        assert!(names.contains(&m), "{m} is not in METHODS");
    }
    let mut s = Session::default();
    let d = call(&mut s, "describe", json!({}));
    assert!(
        d["sheets"]["override_precedence"]
            .as_str()
            .unwrap()
            .contains("viewport class override")
    );
    for ex in d["sheets"]["examples"].as_array().unwrap() {
        let m = ex["method"].as_str().unwrap();
        assert!(names.contains(&m), "example for unknown method {m}");
    }
    for ex in d["command_examples"].as_array().unwrap() {
        serde_json::from_value::<cadektor_core::command::Command>(ex.clone()).unwrap();
    }
}

#[test]
fn sheets_are_created_listed_and_changed() {
    let mut s = model();
    let r = call(
        &mut s,
        "sheet.create",
        json!({"name": "Planting plan", "paper": "A1", "number": "L-101", "title": "Planting"}),
    );
    let layer = r["layer"].as_u64().unwrap();
    let list = call(&mut s, "sheet.list", json!({}));
    let sheet = &list["sheets"][0];
    assert_eq!(
        (
            sheet["number"].as_str(),
            sheet["title"].as_str(),
            sheet["paper"].as_str()
        ),
        (Some("L-101"), Some("Planting"), Some("a1"))
    );
    assert_eq!(
        (sheet["width_mm"].as_f64(), sheet["height_mm"].as_f64()),
        (Some(841.0), Some(594.0))
    );
    // By name and by ID.
    let r = call(
        &mut s,
        "sheet.set",
        json!({"layer": "planting plan", "orientation": "portrait", "paper": {"width_mm": 300, "height_mm": 500}}),
    );
    assert_eq!(
        (r["width_mm"].as_f64(), r["height_mm"].as_f64()),
        (Some(300.0), Some(500.0))
    );
    let r = call(
        &mut s,
        "sheet.set",
        json!({"layer": layer, "width_mm": 400, "height_mm": 200, "number": "L-102"}),
    );
    assert_eq!(
        (
            r["number"].as_str(),
            r["width_mm"].as_f64(),
            r["height_mm"].as_f64()
        ),
        (Some("L-102"), Some(200.0), Some(400.0)),
        "the portrait orientation stays"
    );
    assert_eq!(
        call(&mut s, "document.summary", json!({}))["layers"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // One undo step per call.
    assert_eq!(call(&mut s, "undo", json!({}))["done"], true);
    assert_eq!(
        call(&mut s, "sheet.list", json!({}))["sheets"][0]["number"],
        "L-101"
    );

    // Errors.
    assert!(fail(&mut s, "sheet.create", json!({})).contains("name"));
    assert!(
        fail(&mut s, "sheet.create", json!({"name": "x", "paper": "b7"})).contains("unknown paper")
    );
    assert!(fail(&mut s, "sheet.create", json!({"name": "x", "paper": 5})).contains("paper"));
    assert!(
        fail(
            &mut s,
            "sheet.create",
            json!({"name": "x", "paper": {"width_mm": "a"}})
        )
        .contains("custom")
    );
    assert!(
        fail(
            &mut s,
            "sheet.create",
            json!({"name": "x", "width_mm": 100})
        )
        .contains("both")
    );
    assert!(
        fail(
            &mut s,
            "sheet.create",
            json!({"name": "x", "orientation": "diagonal"})
        )
        .contains("orientation")
    );
    assert!(
        fail(
            &mut s,
            "sheet.create",
            json!({"name": "x", "orientation": 3})
        )
        .contains("orientation")
    );
    assert!(
        fail(
            &mut s,
            "sheet.create",
            json!({"name": "x", "paper": {"custom": {"width_mm": 1, "height_mm": 1}}})
        )
        .contains("paper side")
    );
    assert!(fail(&mut s, "sheet.set", json!({"layer": 1})).contains("not a sheet"));
    assert!(fail(&mut s, "sheet.set", json!({"layer": "nowhere"})).contains("no layer"));
    assert!(fail(&mut s, "sheet.set", json!({"layer": true})).contains("no layer"));
    assert!(fail(&mut s, "sheet.set", json!({})).contains("no layer"));
}

#[test]
fn project_data() {
    let mut s = Session::default();
    assert_eq!(call(&mut s, "project.get", json!({}))["name"], "");
    let r = call(
        &mut s,
        "project.set",
        json!({"name": "Garden", "drawn_by": "NM", "client": "C", "address": "A", "date": "d", "revision": "r"}),
    );
    assert_eq!(
        (r["name"].as_str(), r["revision"].as_str()),
        (Some("Garden"), Some("r"))
    );
    call(&mut s, "project.set", json!({"name": "Park"}));
    let r = call(&mut s, "project.get", json!(null));
    assert_eq!(
        (r["name"].as_str(), r["drawn_by"].as_str()),
        (Some("Park"), Some("NM"))
    );
}

#[test]
fn viewports_fit_the_model_and_update_live() {
    let mut s = model();
    let sheet = call(&mut s, "sheet.create", json!({"name": "S", "paper": "a3"}))["layer"]
        .as_u64()
        .unwrap();
    let vp = call(
        &mut s,
        "viewport.create",
        json!({"sheet": sheet, "name": "Main"}),
    );
    let id = vp["id"].as_u64().unwrap();
    // 20 m by 10 m in a 390 mm by 227 mm frame: 1:75 fits (20000/75 = 267).
    assert_eq!(vp["scale"], 75.0);
    assert_eq!(vp["center"], json!([10000.0, 5000.0]));
    assert_eq!(
        (
            vp["view"].as_str(),
            vp["name"].as_str(),
            vp["sheet"].as_u64()
        ),
        (Some("plan"), Some("Main"), Some(sheet))
    );
    assert_eq!(vp["layers"], json!([1]));
    assert_eq!(
        call(&mut s, "viewport.get", json!({"id": id}))["scale"],
        75.0
    );

    // The sheet renders the model. A change to the model shows without any viewport call.
    let svg_before = call(&mut s, "sheet.render", json!({"sheet": "S"}))["svg"]
        .as_str()
        .unwrap()
        .to_string();
    exec(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "text", "at": [1000, 1000], "text": "LIVE-MARK", "height": 500}}),
    );
    let svg_after = call(&mut s, "sheet.render", json!({"sheet": sheet}))["svg"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!svg_before.contains("LIVE-MARK") && svg_after.contains("LIVE-MARK"));
    assert!(svg_after.contains(r#"width="420mm""#));

    // Update: fixed scale, a crop, other layers.
    let r = call(
        &mut s,
        "viewport.update",
        json!({"id": id, "scale": 100, "center": [0, 0], "crop": {"type": "rect", "min": [50, 50], "max": [200, 200]}, "show_frame": true}),
    );
    assert_eq!(
        (r["scale"].as_f64(), r["show_frame"].as_bool()),
        (Some(100.0), Some(true))
    );
    assert_eq!(r["crop"]["type"], "rect");
    let r = call(
        &mut s,
        "viewport.update",
        json!({"id": id, "crop": null, "scale": "fit", "view": "Right Isometric", "mode": "wireframe", "dpi": 60}),
    );
    assert_eq!(
        (r["view"].as_str(), r["scale"].as_f64(), r["dpi"].as_f64()),
        (Some("right_isometric"), Some(75.0), Some(60.0))
    );
    assert!(r.get("crop").is_none());
    let r = call(
        &mut s,
        "viewport.update",
        json!({"id": id, "view": "plan", "fit": true, "layers": ["Layer 1", 1]}),
    );
    assert_eq!(r["layers"], json!([1]));
    let l2 = exec(&mut s, json!({"op": "add_layer", "name": "Planting"}))["created"][0]
        .as_u64()
        .unwrap();
    let r = call(
        &mut s,
        "viewport.update",
        json!({"id": id, "layers": ["planting"]}),
    );
    assert_eq!(r["layers"], json!([l2]));
    // Listing.
    let list = call(&mut s, "viewport.list", json!({"sheet": "S"}));
    assert_eq!(list["viewports"].as_array().unwrap().len(), 1);
    assert_eq!(
        call(&mut s, "viewport.list", json!({}))["viewports"][0]["id"],
        id
    );
    assert_eq!(
        call(&mut s, "sheet.list", json!({}))["sheets"][0]["viewports"],
        json!([id])
    );
    // One undo for one update.
    assert_eq!(call(&mut s, "undo", json!({}))["done"], true);
    assert_eq!(
        call(&mut s, "viewport.get", json!({"id": id}))["layers"],
        json!([1])
    );

    // Explicit frames.
    let r = call(
        &mut s,
        "viewport.create",
        json!({"sheet": "S", "at": [10, 10], "size": [100, 80], "scale": 200, "center": [1, 2], "layers": [1], "view": "top", "mode": "wireframe", "dpi": 100, "show_frame": true, "crop": {"type": "polygon", "points": [[10, 10], [110, 10], [60, 90]]}}),
    );
    assert_eq!(
        (
            r["min"].clone(),
            r["max"].clone(),
            r["scale"].as_f64(),
            r["view"].as_str()
        ),
        (
            json!([10.0, 10.0]),
            json!([110.0, 90.0]),
            Some(200.0),
            Some("top")
        )
    );
    let r = call(
        &mut s,
        "viewport.create",
        json!({"sheet": "S", "min": [0, 0], "max": [50, 50]}),
    );
    assert!(r["scale"].as_f64().unwrap() >= 100.0);

    // Errors.
    assert!(fail(&mut s, "viewport.create", json!({"sheet": 1})).contains("not a sheet"));
    assert!(
        fail(
            &mut s,
            "viewport.create",
            json!({"sheet": "S", "min": [0, 0]})
        )
        .contains("min and max")
    );
    assert!(
        fail(
            &mut s,
            "viewport.create",
            json!({"sheet": "S", "scale": "big"})
        )
        .contains("scale")
    );
    assert!(fail(&mut s, "viewport.create", json!({"sheet": "S", "scale": 0})).contains("scale"));
    assert!(
        fail(
            &mut s,
            "viewport.create",
            json!({"sheet": "S", "view": "sideways"})
        )
        .contains("unknown view")
    );
    assert!(
        fail(
            &mut s,
            "viewport.create",
            json!({"sheet": "S", "layers": [sheet]})
        )
        .contains("sheet layer")
    );
    assert!(
        fail(
            &mut s,
            "viewport.create",
            json!({"sheet": "S", "layers": "all"})
        )
        .contains("list")
    );
    assert!(
        fail(
            &mut s,
            "viewport.create",
            json!({"sheet": "S", "layers": ["nope"]})
        )
        .contains("no layer")
    );
    assert!(fail(&mut s, "viewport.create", json!({"sheet": "S", "dpi": 1})).contains("dpi"));
    assert!(fail(&mut s, "viewport.update", json!({"id": id, "view": 3})).contains("view"));
    assert!(
        fail(
            &mut s,
            "viewport.update",
            json!({"id": id, "min": [500, 500]})
        )
        .contains("frame")
    );
    assert!(fail(&mut s, "viewport.update", json!({"id": id, "scale": true})).contains("scale"));
    assert!(fail(&mut s, "viewport.get", json!({})).contains("id"));
    assert!(fail(&mut s, "viewport.get", json!({"id": 2})).contains("not a viewport"));
    assert!(fail(&mut s, "viewport.get", json!({"id": 9999})).contains("does not exist"));
    assert!(fail(&mut s, "viewport.list", json!({"sheet": 1})).contains("not a sheet"));
}

#[test]
fn overrides_and_annotations() {
    let mut s = model();
    call(&mut s, "sheet.create", json!({"name": "S"}));
    let id = call(&mut s, "viewport.create", json!({"sheet": "S"}))["id"]
        .as_u64()
        .unwrap();
    let base = call(&mut s, "sheet.render", json!({"sheet": "S"}))["svg"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(base.contains("rgb(0,0,255)"), "the class pen shows");

    let r = call(
        &mut s,
        "viewport.set_override",
        json!({"id": id, "classes": {"Trees": {"pen": [10, 20, 30, 255], "line_weight": 0.7}}}),
    );
    assert_eq!(class_ov(&r, 3)["line_weight"], 0.7);
    let svg = call(&mut s, "sheet.render", json!({"sheet": "S"}))["svg"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(svg.contains("rgb(10,20,30)") && !svg.contains("rgb(0,0,255)"));
    // Merge: a second call changes one field and keeps the others. null removes a field.
    call(
        &mut s,
        "viewport.set_override",
        json!({"id": id, "classes": {"3": {"visible": false, "pen": null}}}),
    );
    let r = call(&mut s, "viewport.get", json!({"id": id}));
    let ov = &class_ov(&r, 3);
    assert_eq!(
        (ov["visible"].as_bool(), ov["line_weight"].as_f64()),
        (Some(false), Some(0.7))
    );
    assert!(ov.get("pen").is_none());
    assert!(
        !call(&mut s, "sheet.render", json!({"sheet": "S"}))["svg"]
            .as_str()
            .unwrap()
            .contains("rgb(10,20,30)")
    );
    assert_eq!(
        call(
            &mut s,
            "execute",
            json!({"command": {"op": "batch", "commands": []}})
        )["created"],
        json!([])
    );
    // The document keeps the class visible.
    let doc = call(&mut s, "document.get", json!({}));
    assert_eq!(doc["classes"][1]["visible"], true);
    // Layer overrides, removal and reset.
    call(
        &mut s,
        "viewport.set_override",
        json!({"id": id, "layers": {"Layer 1": false}}),
    );
    assert_eq!(
        call(&mut s, "viewport.get", json!({"id": id}))["layer_overrides"],
        json!([[1, false]])
    );
    call(
        &mut s,
        "viewport.set_override",
        json!({"id": id, "layers": {"1": null}}),
    );
    assert_eq!(
        call(&mut s, "viewport.get", json!({"id": id}))["layer_overrides"],
        json!([])
    );
    call(
        &mut s,
        "viewport.set_override",
        json!({"id": id, "classes": {"Trees": null}}),
    );
    assert!(class_ov(&call(&mut s, "viewport.get", json!({"id": id})), 3).is_null());
    call(
        &mut s,
        "viewport.set_override",
        json!({"id": id, "layers": {"1": true}, "classes": {"Trees": {}}}),
    );
    call(
        &mut s,
        "viewport.set_override",
        json!({"id": id, "reset": true}),
    );
    let r = call(&mut s, "viewport.get", json!({"id": id}));
    assert_eq!(
        (r["layer_overrides"].clone(), r["class_overrides"].clone()),
        (json!([]), json!([]))
    );
    assert!(
        fail(
            &mut s,
            "viewport.set_override",
            json!({"id": id, "layers": [1]})
        )
        .contains("object")
    );
    assert!(
        fail(
            &mut s,
            "viewport.set_override",
            json!({"id": id, "layers": {"1": "yes"}})
        )
        .contains("true, false or null")
    );
    assert!(
        fail(
            &mut s,
            "viewport.set_override",
            json!({"id": id, "layers": {"zz": true}})
        )
        .contains("no layer")
    );
    assert!(
        fail(
            &mut s,
            "viewport.set_override",
            json!({"id": id, "classes": []})
        )
        .contains("object")
    );
    assert!(
        fail(
            &mut s,
            "viewport.set_override",
            json!({"id": id, "classes": {"zz": {}}})
        )
        .contains("no class")
    );
    assert!(
        fail(
            &mut s,
            "viewport.set_override",
            json!({"id": id, "classes": {"Trees": 5}})
        )
        .contains("object or null")
    );
    assert!(
        fail(
            &mut s,
            "viewport.set_override",
            json!({"id": id, "classes": {"Trees": {"colour": 1}}})
        )
        .contains("unknown override field")
    );
    assert!(
        fail(
            &mut s,
            "viewport.set_override",
            json!({"id": id, "classes": {"Trees": {"line_weight": -1}}})
        )
        .contains("line weights")
    );

    // Annotations.
    let r = call(
        &mut s,
        "viewport.annotate",
        json!({"id": id, "add": [
            {"kind": "text", "at": [20, 20], "text": "ANN-TEXT", "height": 4},
            {"kind": "line", "a": [0, 0], "b": [30, 0], "class": "Trees", "attrs": {"line_weight": 1.0}},
            {"kind": "dimension", "a": [0, 10], "b": [50, 10], "offset": 5},
            {"kind": "shape", "shape": {"kind": "circle", "center": [20, 30], "radius": 3}}
        ]}),
    );
    assert_eq!(
        (r["added"].clone(), r["count"].as_u64()),
        (json!([1, 2, 3, 4]), Some(4))
    );
    let svg = call(&mut s, "sheet.render", json!({"sheet": "S"}))["svg"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(svg.contains("ANN-TEXT"));
    let r = call(
        &mut s,
        "viewport.annotate",
        json!({"id": id, "remove": [2, 3], "add": [{"kind": "text", "at": [1, 1], "text": "x", "height": 2}]}),
    );
    assert_eq!(
        (r["added"].clone(), r["count"].as_u64()),
        (json!([5]), Some(3))
    );
    assert_eq!(
        call(&mut s, "viewport.list", json!({}))["viewports"][0]["annotations"],
        3
    );
    let r = call(
        &mut s,
        "viewport.annotate",
        json!({"id": id, "clear": true}),
    );
    assert_eq!(r["count"], 0);
    assert!(
        fail(
            &mut s,
            "viewport.annotate",
            json!({"id": id, "remove": [77]})
        )
        .contains("no annotation 77")
    );
    assert!(
        !fail(
            &mut s,
            "viewport.annotate",
            json!({"id": id, "add": [{"kind": "blob"}]})
        )
        .is_empty()
    );
    assert!(
        fail(
            &mut s,
            "viewport.annotate",
            json!({"id": id, "add": [{"kind": "text", "at": [0, 0], "text": "x", "height": 0}]})
        )
        .contains("finite")
    );
    assert!(fail(&mut s, "viewport.annotate", json!({"id": id, "add": [{"kind": "text", "at": [0, 0], "text": "x", "height": 1, "class": "zz"}]})).contains("no class"));
    assert!(
        fail(
            &mut s,
            "viewport.annotate",
            json!({"id": id, "remove": "all"})
        )
        .contains("invalid type")
    );
}

#[test]
fn title_blocks_resolve_their_fields() {
    let mut s = model();
    call(
        &mut s,
        "sheet.create",
        json!({"name": "S", "number": "L-101", "title": "Planting plan"}),
    );
    call(
        &mut s,
        "project.set",
        json!({"name": "Riverside", "drawn_by": "NM", "date": "2026-10-10"}),
    );
    call(
        &mut s,
        "viewport.create",
        json!({"sheet": "S", "scale": 100}),
    );
    let id = call(
        &mut s,
        "title_block.create",
        json!({"sheet": "S", "grid_columns": 4, "grid_rows": 3, "fields": {"zone": "7b"}}),
    )["id"]
        .as_u64()
        .unwrap();
    let r = call(&mut s, "title_block.resolve", json!({"id": id}));
    let cells = r["cells"].as_array().unwrap();
    let text = |label: &str| {
        cells.iter().find(|c| c["label"] == label).unwrap()["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(text("Project"), "Riverside");
    assert_eq!(text("Sheet no."), "L-101");
    assert_eq!(text("Sheet title"), "Planting plan");
    assert_eq!(text("Scale"), "1:100");
    assert_eq!(text("Date"), "2026-10-10");
    assert_eq!(text("Drawn by"), "NM");
    assert_eq!(r["grid_labels"].as_array().unwrap().len(), 14);
    assert_eq!(r["border"], json!([[10.0, 10.0], [410.0, 287.0]]));
    // JSON layout and extra fields.
    let layout = json!({"width": 100, "height": 20, "cells": [{"x": 0, "y": 0, "w": 100, "h": 20, "label": "Zone", "text": "{field.zone} - {sheet.index}/{sheet.count} - {sheet.paper}"}]});
    let r = call(
        &mut s,
        "title_block.update",
        json!({"id": id, "layout": layout, "margin": 5}),
    );
    assert_eq!(r["margin"], 5.0);
    let r = call(&mut s, "title_block.resolve", json!({"id": id}));
    assert_eq!(r["cells"][0]["text"], "7b - 1/1 - A3 landscape");
    assert_eq!(r["border"][0], json!([5.0, 5.0]));
    call(
        &mut s,
        "title_block.update",
        json!({"id": id, "layout": null, "fields": {"zone": "8"}}),
    );
    assert_eq!(
        call(&mut s, "title_block.resolve", json!({"id": id}))["cells"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    // The page follows the sheet.
    call(&mut s, "sheet.set", json!({"layer": "S", "paper": "a1"}));
    let obj = call(&mut s, "objects.get", json!({"id": id}));
    assert_eq!(obj["shape"]["page"], json!([841.0, 594.0]));
    assert_eq!(
        call(&mut s, "sheet.list", json!({}))["sheets"][0]["title_blocks"],
        json!([id])
    );
    // Errors.
    assert!(
        fail(
            &mut s,
            "title_block.create",
            json!({"sheet": "S", "grid_columns": 40})
        )
        .contains("26 columns")
    );
    assert!(fail(&mut s, "title_block.create", json!({"sheet": 1})).contains("not a sheet"));
    assert!(
        !fail(
            &mut s,
            "title_block.create",
            json!({"sheet": "S", "margin": "wide"})
        )
        .is_empty()
    );
    assert!(fail(&mut s, "title_block.update", json!({"id": 2})).contains("not a title block"));
    assert!(fail(&mut s, "title_block.update", json!({"id": 9999})).contains("does not exist"));
    assert!(fail(&mut s, "title_block.resolve", json!({"id": 2})).contains("not a title block"));
    assert!(
        fail(
            &mut s,
            "title_block.update",
            json!({"id": id, "layout": {"width": 0, "height": 5, "cells": []}})
        )
        .contains("title block")
    );
}

#[test]
fn viewports_and_title_blocks_work_through_execute_too() {
    let mut s = model();
    let sheet = call(&mut s, "sheet.create", json!({"name": "S"}))["layer"]
        .as_u64()
        .unwrap();
    let r = exec(
        &mut s,
        json!({"op": "add_object", "layer": sheet, "shape": {"kind": "viewport", "min": [10, 10], "max": [200, 150], "center": [0, 0], "scale": 100, "layers": [1]}}),
    );
    let id = r["created"][0].as_u64().unwrap();
    exec(
        &mut s,
        json!({"op": "move_objects", "ids": [id], "delta": [5, 5]}),
    );
    assert_eq!(
        call(&mut s, "viewport.get", json!({"id": id}))["min"],
        json!([15.0, 15.0])
    );
    let f = call(&mut s, "objects.list", json!({"kind": "viewport"}));
    assert_eq!(f.as_array().unwrap().len(), 1);
    // The model views do not see sheet objects.
    let summary = call(&mut s, "document.summary", json!({}));
    assert_eq!(summary["bounds"]["max"], json!([20000.0, 10000.0]));
    // An old document with no sheets loads and has no sheet list.
    let doc = serde_json::to_value(cadektor_core::model::Document::default()).unwrap();
    call(&mut s, "document.load", json!({"document": doc}));
    assert_eq!(call(&mut s, "sheet.list", json!({}))["sheets"], json!([]));
}

#[test]
fn saved_views_store_and_restore_the_visibility_and_the_view() {
    let mut s = model();
    let l2 = exec(&mut s, json!({"op": "add_layer", "name": "Planting"}))["created"][0]
        .as_u64()
        .unwrap();
    let r = call(
        &mut s,
        "saved_view.save",
        json!({"name": "Everything", "zoom": 0.25, "pan": [100, 200]}),
    );
    assert_eq!(
        (
            r["zoom"].as_f64(),
            r["view"].as_str(),
            r["active_layer"].as_u64()
        ),
        (Some(0.25), Some("plan"), Some(1))
    );
    // Change visibility, the active layer and the 3D view, then restore.
    exec(
        &mut s,
        json!({"op": "modify_layer", "id": l2, "visible": false}),
    );
    exec(
        &mut s,
        json!({"op": "modify_class", "id": 3, "visible": false}),
    );
    exec(&mut s, json!({"op": "set_active", "layer": l2}));
    call(
        &mut s,
        "view.set",
        json!({"view": "front", "projection": "perspective", "mode": "wireframe"}),
    );
    s.take_view_changed();
    let r = call(&mut s, "saved_view.restore", json!({"name": "everything"}));
    assert_eq!(r["name"], "Everything");
    let sum = call(&mut s, "document.summary", json!({}));
    assert_eq!(sum["active_layer"], 1);
    assert!(sum["layers"][1]["visible"] == true && sum["classes"][1]["visible"] == true);
    let v = call(&mut s, "view.list", json!({}));
    assert_eq!(
        (
            v["current"]["view"].as_str(),
            v["current"]["projection"].as_str(),
            v["current"]["mode"].as_str()
        ),
        (Some("plan"), Some("ortho"), Some("shaded"))
    );
    assert!(s.take_view_changed(), "the host follows the new 3D view");
    let applied = s.take_applied_view().expect("the host gets the view");
    assert_eq!(
        (applied.zoom, applied.pan),
        (Some(0.25), Some(cadektor_core::DVec2::new(100.0, 200.0)))
    );
    assert!(s.take_applied_view().is_none());
    // One undo step.
    assert_eq!(call(&mut s, "undo", json!({}))["done"], true);
    assert_eq!(
        call(&mut s, "document.summary", json!({}))["active_layer"],
        l2
    );
    // Listing and deleting.
    assert_eq!(
        call(&mut s, "saved_view.list", json!({}))["views"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    call(&mut s, "saved_view.delete", json!({"name": "EVERYTHING"}));
    assert_eq!(
        call(&mut s, "saved_view.list", json!({}))["views"],
        json!([])
    );
    assert!(
        fail(&mut s, "saved_view.restore", json!({"name": "Everything"})).contains("no saved view")
    );
    assert!(
        fail(&mut s, "saved_view.delete", json!({"name": "Everything"})).contains("no saved view")
    );
    assert!(fail(&mut s, "saved_view.save", json!({})).contains("name"));
    assert!(fail(&mut s, "saved_view.save", json!({"name": "x", "zoom": -3})).contains("finite"));
    assert!(
        fail(
            &mut s,
            "saved_view.save",
            json!({"name": "x", "pan": "left"})
        )
        .contains("invalid type")
    );
}

fn export(s: &mut Session, format: &str, path: &std::path::Path, options: Value) -> Value {
    call(
        s,
        "export.file",
        json!({"format": format, "path": path.display().to_string(), "options": options}),
    )
}

fn export_err(s: &mut Session, format: &str, path: &std::path::Path, options: Value) -> String {
    fail(
        s,
        "export.file",
        json!({"format": format, "path": path.display().to_string(), "options": options}),
    )
}

#[test]
fn export_file_writes_pdf_svg_and_png_files() {
    let mut s = model();
    assert!(export_err(&mut s, "pdf", &tmp("none.pdf"), json!({})).contains("no sheet layers"));
    call(
        &mut s,
        "sheet.create",
        json!({"name": "One", "number": "L-1", "title": "First sheet"}),
    );
    call(
        &mut s,
        "sheet.create",
        json!({"name": "Two", "paper": "a4", "orientation": "portrait", "number": "L-2", "title": "Second sheet"}),
    );
    call(&mut s, "viewport.create", json!({"sheet": "One"}));
    call(&mut s, "title_block.create", json!({"sheet": "One"}));
    call(
        &mut s,
        "viewport.create",
        json!({"sheet": "Two", "view": "right_isometric", "dpi": 40}),
    );

    let pdf = tmp("all.pdf");
    let r = export(&mut s, "pdf", &pdf, json!({}));
    assert_eq!(
        (r["pages"].as_u64(), r["path"].as_str()),
        (Some(2), Some(pdf.display().to_string().as_str()))
    );
    let bytes = std::fs::read(&pdf).unwrap();
    assert_eq!(r["bytes"].as_u64(), Some(bytes.len() as u64));
    assert!(bytes.starts_with(b"%PDF-") && bytes.windows(9).any(|w| w == b"/MediaBox"));
    let one = tmp("one.PDF");
    assert_eq!(
        export(&mut s, "pdf", &one, json!({"sheets": ["Two"]}))["pages"],
        1
    );

    let svg = tmp("one.svg");
    export(&mut s, "sheet_svg", &svg, json!({"sheet": "One"}));
    let text = std::fs::read_to_string(&svg).unwrap();
    assert!(text.contains("First sheet") && text.starts_with("<svg"));
    let png = tmp("one.png");
    let r = export(
        &mut s,
        "sheet_png",
        &png,
        json!({"sheet": "One", "dpi": 40}),
    );
    assert_eq!(
        (r["width"].as_u64(), r["height"].as_u64()),
        (Some(661), Some(468))
    );
    assert_eq!(&std::fs::read(&png).unwrap()[1..4], b"PNG");
    let r = call(
        &mut s,
        "sheet.render",
        json!({"sheet": "One", "format": "png", "dpi": 30}),
    );
    assert_eq!(r["width"], 496);
    assert!(r["png_base64"].as_str().unwrap().starts_with("iVBOR"));

    // Errors.
    assert!(fail(&mut s, "export.file", json!({"format": "pdf"})).contains("path"));
    assert!(export_err(&mut s, "pdf", &tmp("x.txt"), json!({})).contains(".pdf"));
    assert!(
        export_err(&mut s, "sheet_svg", &tmp("x.pdf"), json!({"sheet": "One"})).contains(".svg")
    );
    let folder = tmp("folder.pdf");
    std::fs::create_dir_all(&folder).unwrap();
    assert!(export_err(&mut s, "pdf", &folder, json!({})).contains("folder"));
    assert!(!export_err(&mut s, "pdf", &tmp("a/b/c.pdf"), json!({})).is_empty());
    assert!(
        export_err(&mut s, "pdf", &pdf, json!({"sheets": ["Layer 1"]})).contains("not a sheet")
    );
    assert!(export_err(&mut s, "pdf", &pdf, json!({"sheets": "all"})).contains("list"));
    assert!(
        export_err(&mut s, "sheet_png", &png, json!({"sheet": "One", "dpi": 0})).contains("dpi")
    );
    assert!(
        !export_err(
            &mut s,
            "sheet_png",
            &png,
            json!({"sheet": "One", "dpi": 5000})
        )
        .is_empty()
    );
    assert!(export_err(&mut s, "sheet_svg", &svg, json!({})).contains("no layer"));
    assert!(
        fail(
            &mut s,
            "sheet.render",
            json!({"sheet": "One", "format": "gif"})
        )
        .contains("svg or png")
    );
    assert!(
        fail(
            &mut s,
            "sheet.render",
            json!({"sheet": "One", "format": "png", "dpi": 0})
        )
        .contains("dpi")
    );
    let _ = std::fs::remove_dir_all(tmp("x").parent().unwrap());
}

#[test]
fn sheet_exports_follow_the_path_policy() {
    let dir = std::env::temp_dir().join(format!("cadektor-api-policy-{}", std::process::id()));
    let allowed = dir.join("allowed");
    let other = dir.join("other");
    std::fs::create_dir_all(&allowed).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let mut s = model();
    s.paths = cadektor_api::PathPolicy::restricted([allowed.clone()]);
    call(&mut s, "sheet.create", json!({"name": "One"}));
    // Outside the allowed folder: refused for every sheet format.
    for (fmt, name) in [
        ("pdf", "a.pdf"),
        ("sheet_svg", "a.svg"),
        ("sheet_png", "a.png"),
    ] {
        let msg = export_err(&mut s, fmt, &other.join(name), json!({"sheet": "One"}));
        assert!(
            msg.contains("allowed") || msg.contains("outside") || msg.contains("folder"),
            "{fmt}: {msg}"
        );
        assert!(!other.join(name).exists());
    }
    // Inside: written, and replacing needs overwrite.
    let ok = allowed.join("a.pdf");
    export(&mut s, "pdf", &ok, json!({}));
    assert!(ok.exists());
    let msg = export_err(&mut s, "pdf", &ok, json!({}));
    assert!(msg.contains("overwrite"), "{msg}");
    call(
        &mut s,
        "export.file",
        json!({"format": "pdf", "path": ok.display().to_string(), "overwrite": true}),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_sheet_methods_are_unknown() {
    let mut s = Session::default();
    assert!(fail(&mut s, "sheet.explode", json!({})).contains("unknown method"));
    assert!(fail(&mut s, "viewport.get", json!(null)).contains("id"));
}

#[test]
fn a_viewport_keeps_its_own_visibility() {
    let mut s = model();
    call(&mut s, "sheet.create", json!({"name": "S"}));
    // The document hides the class before the viewport exists: the viewport stores that.
    exec(
        &mut s,
        json!({"op": "modify_class", "id": 3, "visible": false}),
    );
    let id = call(
        &mut s,
        "viewport.create",
        json!({"sheet": "S", "scale": 100, "center": [10000, 5000]}),
    )["id"]
        .as_u64()
        .unwrap();
    let svg = |s: &mut Session| {
        call(s, "sheet.render", json!({"sheet": "S"}))["svg"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert!(!svg(&mut s).contains("rgb(0,0,255)"));
    assert_eq!(
        class_ov(&call(&mut s, "viewport.get", json!({"id": id})), 3)["visible"],
        false
    );
    // Showing the class in the document later does not show it in the viewport.
    exec(
        &mut s,
        json!({"op": "modify_class", "id": 3, "visible": true}),
    );
    assert!(!svg(&mut s).contains("rgb(0,0,255)"));
    // Hiding a layer in the document does not hide it in the viewport either.
    exec(
        &mut s,
        json!({"op": "modify_layer", "id": 1, "visible": false}),
    );
    assert!(svg(&mut s).contains("rgb(200,0,0)") || svg(&mut s).contains("stroke"));
    call(
        &mut s,
        "viewport.set_override",
        json!({"id": id, "classes": {"Trees": {"visible": true}}}),
    );
    assert!(svg(&mut s).contains("rgb(0,0,255)"));
    // A class made later shows in the viewport.
    let c = exec(&mut s, json!({"op": "add_class", "name": "Later"}))["created"][0]
        .as_u64()
        .unwrap();
    exec(
        &mut s,
        json!({"op": "add_object", "class": c, "shape": {"kind": "text", "at": [100, 100], "text": "LATER", "height": 400}}),
    );
    assert!(svg(&mut s).contains("LATER"));
}
