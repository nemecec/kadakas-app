//! The editing, query, snapping and visibility methods: `edit.*`, `surface.*`, `objects.query`,
//! `snap.*`, `visibility.*`, and the command schema in `describe`.

use cadektor_api::{METHODS, Session};
use serde_json::{Value, json};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    match s.handle(m, p) {
        Ok(v) => v,
        Err(e) => panic!("{m} failed: {}", e.message),
    }
}

/// The error code and message.
fn fail(s: &mut Session, m: &str, p: Value) -> (i64, String) {
    match s.handle(m, p) {
        Ok(v) => panic!("{m} should fail, got {v}"),
        Err(e) => (e.code, e.message),
    }
}

fn add(s: &mut Session, shape: Value) -> u64 {
    call(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape": shape}}),
    )["created"][0]
        .as_u64()
        .unwrap()
}

fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> u64 {
    add(s, json!({"kind": "line", "a": a, "b": b}))
}

fn rect(s: &mut Session, x0: f64, y0: f64, x1: f64, y1: f64) -> u64 {
    add(s, json!({"kind": "rect", "min": [x0, y0], "max": [x1, y1]}))
}

fn circle(s: &mut Session, x: f64, y: f64, r: f64) -> u64 {
    add(s, json!({"kind": "circle", "center": [x, y], "radius": r}))
}

fn shape(s: &mut Session, id: u64) -> Value {
    call(s, "objects.get", json!({"id": id}))["shape"].clone()
}

fn count(s: &mut Session) -> usize {
    call(s, "objects.list", json!({})).as_array().unwrap().len()
}

fn near(a: &Value, b: [f64; 2]) -> bool {
    (a[0].as_f64().unwrap() - b[0]).abs() < 1e-6 && (a[1].as_f64().unwrap() - b[1]).abs() < 1e-6
}

#[test]
fn every_method_of_these_modules_is_listed_and_routed() {
    let names = [
        "edit.rotate",
        "edit.scale",
        "edit.mirror",
        "edit.array",
        "edit.offset",
        "edit.fillet",
        "edit.chamfer",
        "edit.trim",
        "edit.extend",
        "edit.split",
        "surface.add",
        "surface.subtract",
        "surface.clip",
        "surface.intersect",
        "surface.compose",
        "objects.query",
        "snap.get",
        "snap.set",
        "snap.query",
        "snap.smart",
        "visibility.get",
        "visibility.set",
        "view.screenshot",
    ];
    for n in names {
        assert!(METHODS.iter().any(|(m, d)| *m == n && !d.is_empty()), "{n}");
    }
    let mut s = Session::default();
    for n in names {
        // Empty params are an error of the parameters, never an unknown method.
        if let Err(e) = s.handle(n, json!({})) {
            assert_ne!(e.code, -32601, "{n}");
        }
    }
    // Params that are not an object.
    for n in ["edit.rotate", "objects.query", "snap.set", "visibility.set"] {
        let (code, msg) = fail(&mut s, n, json!([1]));
        assert_eq!(
            (code, msg.as_str()),
            (-32602, "params must be an object"),
            "{n}"
        );
    }
    let d = call(&mut s, "describe", Value::Null);
    assert!(d["editing"]["examples"].as_array().unwrap().len() >= 5);
    assert!(d["queries"]["examples"].as_array().unwrap().len() >= 3);
}

#[test]
fn transforms_copy_and_select() {
    let mut s = Session::default();
    let c = circle(&mut s, 1000.0, 0.0, 100.0);
    let out = call(
        &mut s,
        "edit.scale",
        json!({"ids": [c], "center": [0, 0], "sx": 2, "copy": true}),
    );
    let copy = out["created"][0].as_u64().unwrap();
    assert_eq!(
        call(&mut s, "selection.get", json!({}))["ids"],
        json!([copy])
    );
    assert_eq!(shape(&mut s, copy)["radius"], 200.0);
    // The selection is the default.
    call(&mut s, "selection.set", json!({"ids": [c]}));
    call(
        &mut s,
        "edit.rotate",
        json!({"center": [0, 0], "angle": std::f64::consts::PI}),
    );
    assert!(near(&shape(&mut s, c)["center"], [-1000.0, 0.0]));
    call(
        &mut s,
        "edit.mirror",
        json!({"ids": [c], "a": [0, 0], "b": [1, 0]}),
    );
    call(&mut s, "undo", json!({}));
    call(&mut s, "undo", json!({}));
    assert!(near(&shape(&mut s, c)["center"], [1000.0, 0.0]));
    // Errors: nothing selected, a bad scale, a bad mirror line.
    call(&mut s, "selection.set", json!({"ids": []}));
    assert_eq!(
        fail(&mut s, "edit.rotate", json!({"center": [0, 0], "angle": 1})).0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "edit.scale",
            json!({"ids": [c], "center": [0, 0], "sx": 0})
        )
        .0,
        3
    );
    assert_eq!(
        fail(
            &mut s,
            "edit.mirror",
            json!({"ids": [c], "a": [1, 1], "b": [1, 1]})
        )
        .0,
        3
    );
    assert_eq!(
        fail(&mut s, "edit.scale", json!({"ids": [c], "sx": 2})).0,
        -32602
    );
}

#[test]
fn arrays_in_three_shapes() {
    let mut s = Session::default();
    let c = circle(&mut s, 500.0, 0.0, 10.0);
    let lin = call(
        &mut s,
        "edit.array",
        json!({"ids": [c], "shape": "linear", "count": 2, "distance": 100, "angle": 0}),
    );
    assert_eq!(lin["created"].as_array().unwrap().len(), 2);
    let rect_a = call(
        &mut s,
        "edit.array",
        json!({"ids": [c], "shape": "rectangular", "columns": 2, "rows": 2, "column_spacing": 50, "row_spacing": 50, "retain": false}),
    );
    assert_eq!(rect_a["created"].as_array().unwrap().len(), 4);
    assert_eq!(
        fail(&mut s, "objects.get", json!({"id": c})).0,
        2,
        "the original is gone"
    );
}

#[test]
fn offset_and_its_errors() {
    let mut s = Session::default();
    let l = line(&mut s, [0.0, 0.0], [100.0, 0.0]);
    let r = rect(&mut s, 0.0, 100.0, 50.0, 150.0);
    // A line offsets to its left by default, a rectangle outwards.
    let o = call(&mut s, "edit.offset", json!({"id": l, "distance": 10}));
    assert!(near(
        &shape(&mut s, o["created"][0].as_u64().unwrap())["a"],
        [0.0, 10.0]
    ));
    let o = call(&mut s, "edit.offset", json!({"id": r, "distance": 10}));
    assert_eq!(
        shape(&mut s, o["created"][0].as_u64().unwrap())["min"],
        json!([-10.0, 90.0])
    );
    call(
        &mut s,
        "edit.offset",
        json!({"id": l, "through": [50, -30], "copy": false}),
    );
    assert!(near(&shape(&mut s, l)["a"], [0.0, -30.0]));
    for (p, code) in [
        (json!({"id": l}), -32602),
        (json!({"id": l, "distance": 1, "through": [0, 0]}), -32602),
        (json!({"id": l, "distance": 1, "side": "up"}), -32602),
        (json!({"id": l, "through": [null, 0]}), -32602),
        (json!({"id": 999, "distance": 1}), 2),
        (json!({"id": l, "distance": -1}), 3),
    ] {
        assert_eq!(fail(&mut s, "edit.offset", p.clone()).0, code, "{p}");
    }
}

#[test]
fn fillet_and_chamfer_forms() {
    let mut s = Session::default();
    let a = line(&mut s, [0.0, 0.0], [1000.0, 0.0]);
    let b = line(&mut s, [0.0, 0.0], [0.0, 1000.0]);
    let p = rect(&mut s, 5000.0, 0.0, 6000.0, 1000.0);
    let poly = add(
        &mut s,
        json!({"kind": "polyline", "closed": true, "points": [[0, 3000], [1000, 3000], [1000, 4000], [0, 4000]]}),
    );
    let args = json!({"a": a, "b": b, "pick_a": [800, 0], "pick_b": [0, 800]});
    let with = |extra: Value| {
        let mut m = args.as_object().unwrap().clone();
        m.extend(extra.as_object().unwrap().clone());
        Value::Object(m)
    };
    let out = call(&mut s, "edit.fillet", with(json!({"radius": 100})));
    assert_eq!(out["created"].as_array().unwrap().len(), 1);
    call(&mut s, "undo", json!({}));
    for mode in ["trim", "split", "standard"] {
        call(
            &mut s,
            "edit.chamfer",
            with(json!({"distance": 100, "mode": mode})),
        );
        call(&mut s, "undo", json!({}));
    }
    call(
        &mut s,
        "edit.chamfer",
        with(json!({"distance": 100, "distance2": 50})),
    );
    call(&mut s, "undo", json!({}));
    call(
        &mut s,
        "edit.chamfer",
        with(json!({"distance": 100, "angle": 0.7})),
    );
    call(&mut s, "undo", json!({}));
    call(&mut s, "edit.chamfer", with(json!({"length": 100})));
    call(&mut s, "undo", json!({}));
    // One shape: a corner, corners or all.
    call(
        &mut s,
        "edit.fillet",
        json!({"id": poly, "corner": 0, "radius": 100}),
    );
    call(
        &mut s,
        "edit.fillet",
        json!({"id": poly, "corners": [1, 2], "radius": 100}),
    );
    call(
        &mut s,
        "edit.chamfer",
        json!({"id": p, "all": true, "distance": 100}),
    );
    // Errors.
    for (m, params, code) in [
        ("edit.fillet", json!({"radius": 10}), -32602),
        ("edit.fillet", with(json!({})), -32602),
        ("edit.fillet", json!({"a": a, "b": b, "radius": 10}), -32602),
        (
            "edit.fillet",
            json!({"a": a, "b": b, "pick_a": [800, 0], "radius": 10}),
            -32602,
        ),
        (
            "edit.fillet",
            with(json!({"radius": 10, "mode": "round"})),
            -32602,
        ),
        ("edit.fillet", json!({"id": poly, "radius": 10}), -32602),
        (
            "edit.fillet",
            json!({"id": poly, "corner": 0, "all": true, "radius": 10}),
            -32602,
        ),
        (
            "edit.fillet",
            json!({"id": poly, "all": true, "radius": 10, "mode": "standard"}),
            -32602,
        ),
        (
            "edit.fillet",
            json!({"id": poly, "all": true, "radius": 10000}),
            3,
        ),
        ("edit.fillet", with(json!({"radius": "big"})), -32602),
        ("edit.chamfer", with(json!({})), -32602),
        ("edit.chamfer", with(json!({"distance": 5000})), 3),
        (
            "edit.fillet",
            json!({"a": a, "b": b, "pick_a": [null, 0], "pick_b": [0, 8], "radius": 10}),
            -32602,
        ),
    ] {
        assert_eq!(fail(&mut s, m, params.clone()).0, code, "{m} {params}");
    }
}

#[test]
fn trim_extend_split() {
    let mut s = Session::default();
    let t = line(&mut s, [0.0, 0.0], [1000.0, 0.0]);
    let c1 = line(&mut s, [300.0, -50.0], [300.0, 50.0]);
    let c2 = line(&mut s, [700.0, -50.0], [700.0, 50.0]);
    let out = call(&mut s, "edit.trim", json!({"id": t, "pick": [500, 0]}));
    assert_eq!(out["created"].as_array().unwrap().len(), 1);
    call(&mut s, "undo", json!({}));
    call(
        &mut s,
        "edit.trim",
        json!({"id": t, "pick": [500, 0], "cutters": [c1, c2]}),
    );
    call(&mut s, "undo", json!({}));
    assert_eq!(
        fail(
            &mut s,
            "edit.trim",
            json!({"id": t, "pick": [500, 0], "cutters": []})
        )
        .0,
        3
    );
    assert_eq!(
        fail(&mut s, "edit.trim", json!({"id": t, "pick": [null, 0]})).0,
        -32602
    );
    // Extend.
    let e = line(&mut s, [0.0, 500.0], [200.0, 500.0]);
    let wall = line(&mut s, [900.0, 400.0], [900.0, 600.0]);
    call(
        &mut s,
        "edit.extend",
        json!({"id": e, "pick": [190, 500], "boundaries": [wall]}),
    );
    assert!(near(&shape(&mut s, e)["b"], [900.0, 500.0]));
    assert_eq!(
        fail(
            &mut s,
            "edit.extend",
            json!({"id": e, "pick": [190, 500], "boundaries": []})
        )
        .0,
        3
    );
    // Split a line, and a rectangle along a line.
    let out = call(&mut s, "edit.split", json!({"id": t, "at": [[500, 5]]}));
    assert_eq!(out["created"].as_array().unwrap().len(), 1);
    let r = rect(&mut s, 0.0, 2000.0, 1000.0, 3000.0);
    let out = call(
        &mut s,
        "edit.split",
        json!({"id": r, "line": {"a": [400, 0], "b": [400, 9000]}}),
    );
    assert_eq!(out["created"].as_array().unwrap().len(), 1);
    for p in [
        json!({"id": r}),
        json!({"id": r, "at": [[1, 1]], "line": {"a": [0, 0], "b": [1, 1]}}),
        json!({"id": r, "at": [[null, 1]]}),
        json!({"id": r, "line": {"a": [0, 0], "b": [0, 0]}}),
    ] {
        let (code, _) = fail(&mut s, "edit.split", p.clone());
        assert!(code == -32602 || code == 3, "{p}");
    }
}

#[test]
fn surfaces() {
    let mut s = Session::default();
    let a = rect(&mut s, 0.0, 0.0, 1000.0, 1000.0);
    let b = rect(&mut s, 500.0, 500.0, 1500.0, 1500.0);
    let area = |s: &mut Session| -> f64 {
        call(s, "objects.list", json!({}))
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                let sh = &o["shape"];
                let pts = sh["points"].as_array().cloned().unwrap_or_default();
                if sh["kind"] == "rect" {
                    (sh["max"][0].as_f64().unwrap() - sh["min"][0].as_f64().unwrap())
                        * (sh["max"][1].as_f64().unwrap() - sh["min"][1].as_f64().unwrap())
                } else {
                    let p: Vec<(f64, f64)> = pts
                        .iter()
                        .map(|v| {
                            let v = if v.is_array() {
                                v.clone()
                            } else {
                                v["p"].clone()
                            };
                            (v[0].as_f64().unwrap(), v[1].as_f64().unwrap())
                        })
                        .collect();
                    let n = p.len();
                    (0..n)
                        .map(|i| p[i].0 * p[(i + 1) % n].1 - p[(i + 1) % n].0 * p[i].1)
                        .sum::<f64>()
                        .abs()
                        / 2.0
                }
            })
            .sum()
    };
    call(&mut s, "surface.add", json!({"ids": [a, b]}));
    assert!((area(&mut s) - 1_750_000.0).abs() < 1e-3);
    call(&mut s, "undo", json!({}));
    call(&mut s, "surface.add", json!({"ids": [a, b], "keep": true}));
    assert_eq!(count(&mut s), 2);
    call(&mut s, "undo", json!({}));
    call(
        &mut s,
        "surface.subtract",
        json!({"base": a, "cutters": [b], "keep_cutters": true}),
    );
    assert_eq!(count(&mut s), 2);
    call(&mut s, "undo", json!({}));
    call(
        &mut s,
        "surface.clip",
        json!({"ids": [a, b], "keep_cutter": false}),
    );
    assert_eq!(count(&mut s), 1);
    call(&mut s, "undo", json!({}));
    call(&mut s, "surface.intersect", json!({"ids": [a, b]}));
    assert!((area(&mut s) - 250_000.0).abs() < 1e-3);
    call(&mut s, "undo", json!({}));
    let out = call(&mut s, "surface.compose", json!({"ids": [a, b]}));
    assert_eq!(out["created"].as_array().unwrap().len(), 3);
    for (m, p, code) in [
        ("surface.add", json!({}), -32602),
        ("surface.subtract", json!({"base": a}), -32602),
        ("surface.subtract", json!({"cutters": [b]}), -32602),
        ("surface.add", json!({"ids": [999, 998]}), 2),
    ] {
        assert_eq!(fail(&mut s, m, p.clone()).0, code, "{m} {p}");
    }
}

#[test]
fn query_by_criteria() {
    let mut s = Session::default();
    let r1 = rect(&mut s, 0.0, 0.0, 1000.0, 1000.0);
    let r2 = rect(&mut s, 0.0, 0.0, 5000.0, 5000.0);
    circle(&mut s, 0.0, 0.0, 500.0);
    let q = call(
        &mut s,
        "objects.query",
        json!({"criteria": "Kind = \"rect\" AND Area > 10"}),
    );
    assert_eq!(q["ids"], json!([r2]));
    assert_eq!(q["count"], 1);
    let q = call(
        &mut s,
        "objects.query",
        json!({"kind": "rect", "columns": ["Kind"], "limit": 1, "select": true}),
    );
    assert_eq!(q["ids"], json!([r1]));
    assert_eq!(q["count"], 2);
    assert_eq!(q["truncated"], true);
    assert_eq!(q["rows"][0]["values"], json!(["rect"]));
    assert_eq!(call(&mut s, "selection.get", json!({}))["ids"], json!([r1]));
    let q = call(&mut s, "objects.query", json!({"layer": 1, "class": 0}));
    assert_eq!(q["count"], 3);
    let q = call(
        &mut s,
        "objects.query",
        json!({"criteria": "NoSuchThing > 1"}),
    );
    assert_eq!(
        (q["count"].as_u64(), q["skipped"].as_u64()),
        (Some(0), Some(3))
    );
    assert_eq!(
        fail(&mut s, "objects.query", json!({"criteria": "Kind ="})).0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "objects.query", json!({"columns": ["((("]})).0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "objects.query", json!({"limit": "x"})).0,
        -32602
    );
}

#[test]
fn snapping_settings_and_queries() {
    let mut s = Session::default();
    line(&mut s, [0.0, 0.0], [1000.0, 0.0]);
    let g = call(&mut s, "snap.get", json!({}));
    assert_eq!(g["set"]["grid"], true);
    assert_eq!(g["angle_step_deg"].as_f64().unwrap().round(), 15.0);
    let g = call(
        &mut s,
        "snap.set",
        json!({"set": {"angle": true}, "angle_step_deg": 45, "relative_angles": true, "distance_step": 100, "other_layers": false, "grey": true}),
    );
    assert_eq!(
        (
            g["set"]["angle"].clone(),
            g["relative_angles"].clone(),
            g["other_layers"].clone()
        ),
        (json!(true), json!(true), json!(false))
    );
    call(
        &mut s,
        "snap.set",
        json!({"other_layers": true, "grey": false}),
    );
    for p in [
        json!({"set": {"nope": true}}),
        json!({"set": {"grid": 1}}),
        json!({"angle_step_deg": 0}),
        json!({"distance_step": -5}),
    ] {
        assert_eq!(fail(&mut s, "snap.set", p.clone()).0, -32602, "{p}");
    }
    let q = call(
        &mut s,
        "snap.query",
        json!({"point": [3, 2], "tolerance": 10}),
    );
    assert_eq!(
        (q["kind"].clone(), q["cue"].clone()),
        (json!("endpoint"), json!("Endpoint"))
    );
    let q = call(
        &mut s,
        "snap.query",
        json!({"point": [3000, 2000], "tolerance": 10, "set": {"grid": false, "angle": false}}),
    );
    assert_eq!(q["snapped"], false);
    let q = call(
        &mut s,
        "snap.query",
        json!({"point": [214, 212], "anchor": [0, 0], "tolerance": 10, "set": {"distance": true}}),
    );
    assert_eq!(q["kind"], "angle");
    assert_eq!(
        fail(
            &mut s,
            "snap.query",
            json!({"point": [0, 0], "tolerance": -1})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "snap.query",
            json!({"point": [0, 0], "set": {"zzz": true}})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "snap.query", json!({"point": [null, 0]})).0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "snap.query",
            json!({"point": [0, 0], "anchor": [null, 1]})
        )
        .0,
        -32602
    );
}

#[test]
fn smart_points_and_edges() {
    let mut s = Session::default();
    line(&mut s, [0.0, 0.0], [1000.0, 0.0]);
    call(
        &mut s,
        "snap.smart",
        json!({"action": "acquire_point", "point": [100, 200]}),
    );
    call(
        &mut s,
        "snap.smart",
        json!({"action": "acquire_edge", "a": [0, 0], "b": [10, 10]}),
    );
    let g = call(&mut s, "snap.get", json!({}));
    assert_eq!(g["smart"]["points"].as_array().unwrap().len(), 1);
    assert_eq!(g["smart"]["edges"].as_array().unwrap().len(), 1);
    let q = call(
        &mut s,
        "snap.query",
        json!({"point": [400, 203], "tolerance": 10, "set": {"grid": false}}),
    );
    assert_eq!(q["kind"], "smart_point");
    call(
        &mut s,
        "snap.smart",
        json!({"action": "acquire_at", "point": [3, 2]}),
    );
    call(
        &mut s,
        "snap.smart",
        json!({"action": "acquire_at", "point": [300, 3], "edges": true}),
    );
    call(&mut s, "snap.smart", json!({"action": "clear"}));
    assert_eq!(
        call(&mut s, "snap.get", json!({}))["smart"]["points"],
        json!([])
    );
    for (p, code) in [
        (json!({"action": "dance"}), -32602),
        (json!({"action": "acquire_point"}), -32602),
        (
            json!({"action": "acquire_point", "point": [null, 0]}),
            -32602,
        ),
        (json!({"action": "acquire_edge", "a": [0, 0]}), -32602),
        (json!({"action": "acquire_edge", "b": [0, 0]}), -32602),
        (
            json!({"action": "acquire_edge", "a": [0, 0], "b": [0, 0]}),
            -32602,
        ),
        (json!({"action": "acquire_at"}), -32602),
        (json!({"action": "acquire_at", "point": [9000, 9000]}), 2),
    ] {
        assert_eq!(fail(&mut s, "snap.smart", p.clone()).0, code, "{p}");
    }
    // A new document keeps the snapping settings but not the smart points.
    call(&mut s, "snap.set", json!({"set": {"edges": true}}));
    call(
        &mut s,
        "snap.smart",
        json!({"action": "acquire_point", "point": [1, 1]}),
    );
    let doc = call(&mut s, "document.get", json!({}));
    call(&mut s, "document.load", json!({"document": doc}));
    let g = call(&mut s, "snap.get", json!({}));
    assert_eq!(g["set"]["edges"], true);
    assert_eq!(g["smart"]["points"], json!([]));
}

#[test]
fn visibility_modes() {
    let mut s = Session::default();
    let layer = call(
        &mut s,
        "execute",
        json!({"command": {"op": "add_layer", "name": "Survey", "elevation": 0}}),
    )["created"][0]
        .as_u64()
        .unwrap();
    let v = call(
        &mut s,
        "visibility.set",
        json!({"mode": "grey", "layers": [layer], "classes": [0]}),
    );
    let mode = |v: &Value, key: &str, id: u64| -> Value {
        v[key]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == id)
            .unwrap()["mode"]
            .clone()
    };
    assert_eq!(mode(&v, "layers", layer), "grey");
    assert_eq!(mode(&v, "classes", 0), "grey");
    assert_eq!(
        call(&mut s, "document.summary", json!({}))["grey"]["layers"],
        json!([layer])
    );
    let v = call(
        &mut s,
        "visibility.set",
        json!({"mode": "invisible", "layers": [layer]}),
    );
    assert_eq!(mode(&v, "layers", layer), "invisible");
    let v = call(&mut s, "visibility.get", json!({}));
    assert_eq!(mode(&v, "layers", 1), "visible");
    for (p, code) in [
        (json!({"mode": "pink", "layers": [1]}), -32602),
        (json!({"layers": [1]}), -32602),
        (json!({"mode": "grey"}), 3),
        (json!({"mode": "grey", "layers": [999]}), 2),
    ] {
        assert_eq!(fail(&mut s, "visibility.set", p.clone()).0, code, "{p}");
    }
}

#[test]
fn the_command_schema_and_the_screenshot_error() {
    let mut s = Session::default();
    let plain = call(&mut s, "describe", json!({}));
    assert!(plain.get("command_schema").is_none());
    assert!(
        plain["schemas"]["command"]
            .as_str()
            .unwrap()
            .contains("schema")
    );
    let full = call(&mut s, "describe", json!({"schema": true}));
    assert_eq!(
        full["command_schema"]["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    let ops = full["command_schema"]["oneOf"].as_array().unwrap();
    assert!(ops.len() > 40);
    // The headless server has no window.
    let (code, msg) = fail(&mut s, "view.screenshot", json!({}));
    assert_eq!(code, 1);
    assert!(msg.contains("live window"), "{msg}");
}

#[test]
fn grey_objects_cannot_be_selected_or_edited_but_they_can_cut() {
    let mut s = Session::default();
    let survey = call(
        &mut s,
        "execute",
        json!({"command": {"op": "add_layer", "name": "Survey", "elevation": 0}}),
    )["created"][0]
        .as_u64()
        .unwrap();
    let t = call(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "layer": survey, "shape": {"kind": "line", "a": [0, 0], "b": [1000, 0]}}}),
    )["created"][0]
        .as_u64()
        .unwrap();
    let plain = line(&mut s, [0.0, 100.0], [1000.0, 100.0]);
    call(
        &mut s,
        "visibility.set",
        json!({"mode": "grey", "layers": [survey]}),
    );
    // The active layer cannot be grey.
    let (code, msg) = fail(
        &mut s,
        "visibility.set",
        json!({"mode": "grey", "layers": [1]}),
    );
    assert_eq!(code, 3);
    assert!(msg.contains("active layer"), "{msg}");
    // Not selectable, by query or by id; not editable.
    let q = call(&mut s, "objects.query", json!({"select": true}));
    assert_eq!(q["grey_not_selected"], 1);
    assert_eq!(
        call(&mut s, "selection.get", json!({}))["ids"],
        json!([plain])
    );
    assert_eq!(fail(&mut s, "selection.set", json!({"ids": [t]})).0, 3);
    assert_eq!(
        fail(
            &mut s,
            "edit.mirror",
            json!({"ids": [t], "a": [0, 0], "b": [0, 1]})
        )
        .0,
        3
    );
    assert_eq!(
        fail(&mut s, "edit.split", json!({"id": t, "at": [[400, 0]]})).0,
        3
    );
    // A grey class on the active layer: its line cuts, and it cannot be cut.
    let class = call(
        &mut s,
        "execute",
        json!({"command": {"op": "add_class", "name": "Existing"}}),
    )["created"][0]
        .as_u64()
        .unwrap();
    let wall = call(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "class": class, "shape": {"kind": "line", "a": [400, -500], "b": [400, 500]}}}),
    )["created"][0]
        .as_u64()
        .unwrap();
    call(
        &mut s,
        "visibility.set",
        json!({"mode": "grey", "classes": [class]}),
    );
    let out = call(
        &mut s,
        "edit.trim",
        json!({"id": plain, "pick": [800, 100]}),
    );
    assert!(out["created"].as_array().unwrap().is_empty());
    assert!(
        near(&shape(&mut s, plain)["b"], [400.0, 100.0]),
        "the grey wall was the cutter"
    );
    assert_eq!(
        fail(
            &mut s,
            "edit.extend",
            json!({"id": wall, "pick": [400, 400], "boundaries": [plain]})
        )
        .0,
        3
    );
    // Visible again: editable.
    call(
        &mut s,
        "visibility.set",
        json!({"mode": "visible", "classes": [class]}),
    );
    call(
        &mut s,
        "edit.mirror",
        json!({"ids": [wall], "a": [0, 0], "b": [0, 1]}),
    );
}
