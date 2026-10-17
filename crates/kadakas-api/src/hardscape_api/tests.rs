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

fn square(w: i64) -> Value {
    json!([[0, 0], [w, 0], [w, w], [0, w]])
}

fn setup() -> Session {
    let mut s = Session::with_sequential_ids();
    ok(
        &mut s,
        "plant_style.create",
        json!({"name": "Lavender", "id_code": "LA", "spread": 500, "price": 4}),
    );
    ok(
        &mut s,
        "plant_style.create",
        json!({"name": "Sedge", "id_code": "SE", "spread": 400, "category": "grass"}),
    );
    s
}

#[test]
fn hardscape_styles_crud() {
    let mut s = Session::with_sequential_ids();
    let id = ok(
        &mut s,
        "hardscape_style.create",
        json!({"name": "Setts", "material": "Granite", "thickness": 60,
            "components": [{"name": "Bed", "material": "Sand", "thickness": 40}, {"name": "Base", "material": "Stone", "thickness": 150}],
            "border": {"width": 150, "material": "Kerb", "sides": "left"},
            "joints": {"kind": "herringbone", "unit_length": 200, "unit_width": 100, "spacing": 4, "angle": 45}}),
    )["id"]
        .as_uid()
        .unwrap();
    let got = ok(&mut s, "hardscape_style.get", json!({"id": "setts"}));
    assert_eq!(got["total_thickness"], 250.0);
    assert_eq!(got["joints"]["kind"], "herringbone");
    assert_eq!(got["border"]["sides"], "left");
    assert_eq!(got["used_by"], 0);
    let h = ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": square(3000), "style": id}),
    )["id"]
        .as_uid()
        .unwrap();
    assert_eq!(
        ok(&mut s, "hardscape_style.list", json!(null))["styles"][0]["used_by"],
        1
    );
    // Update: the object follows, null clears.
    let r = ok(
        &mut s,
        "hardscape_style.update",
        json!({"id": id, "name": "Setts 2", "thickness": 80, "border": null}),
    );
    assert_eq!(r["name"], "Setts 2");
    assert!(r["border"].is_null());
    let o = ok(&mut s, "objects.get", json!({"id": h}));
    assert_eq!(o["shape"]["thickness"], 270.0);
    assert_eq!(o["shape"]["material"], "Granite");
    // Duplicate, delete.
    let copy = ok(&mut s, "hardscape_style.duplicate", json!({"id": id}))["id"]
        .as_uid()
        .unwrap();
    assert_eq!(
        ok(&mut s, "hardscape_style.get", json!({"id": copy}))["name"],
        "Setts 2 copy"
    );
    let copy2 = ok(&mut s, "hardscape_style.duplicate", json!({"id": id}))["id"]
        .as_uid()
        .unwrap();
    assert_eq!(
        ok(&mut s, "hardscape_style.get", json!({"id": copy2}))["name"],
        "Setts 2 copy 2"
    );
    let named = ok(
        &mut s,
        "hardscape_style.duplicate",
        json!({"id": id, "name": "Mine"}),
    )["id"]
        .as_uid()
        .unwrap();
    assert_eq!(
        ok(&mut s, "hardscape_style.get", json!({"id": named}))["name"],
        "Mine"
    );
    assert_eq!(
        fail(&mut s, "hardscape_style.delete", json!({"id": id})).0,
        5,
        "in use"
    );
    ok(
        &mut s,
        "hardscape_style.delete",
        json!({"id": id, "reassign": copy}),
    );
    assert_eq!(
        ok(&mut s, "objects.get", json!({"id": h}))["data"]["hardscape"]["style"],
        copy
    );
    ok(&mut s, "hardscape_style.delete", json!({"id": named}));
    // Errors.
    assert_eq!(
        fail(
            &mut s,
            "hardscape_style.create",
            json!({"name": "X", "colour": 1})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "hardscape_style.create",
            json!({"name": "X", "thickness": -1})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "hardscape_style.create",
            json!({"name": "X", "thickness": "thick"})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "hardscape_style.create", json!({"thickness": 5})).0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "hardscape_style.create", json!({"name": 5})).0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "hardscape_style.create",
            json!({"name": "Setts 2 copy"})
        )
        .0,
        3,
        "name exists"
    );
    assert_eq!(
        fail(
            &mut s,
            "hardscape_style.get",
            json!({"id": "00000000-0000-0000-0000-0000000003e7"})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "hardscape_style.get", json!({"id": true})).0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "hardscape_style.update",
            json!({"id": copy, "name": 5})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "hardscape_style.duplicate",
            json!({"id": copy, "name": 5})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "hardscape_style.delete",
            json!({"id": copy, "reassign": 999})
        )
        .0,
        -32602
    );
    // A plant style is not a hardscape style.
    let plant = ok(&mut s, "plant_style.create", json!({"name": "Oak"}))["id"]
        .as_uid()
        .unwrap();
    assert_eq!(
        fail(&mut s, "hardscape_style.get", json!({"id": plant})).0,
        -32602
    );
}

#[test]
fn landscape_area_styles_crud_and_mix_names() {
    let mut s = setup();
    let sedge = ok(&mut s, "plant_style.get", json!({"id": "Sedge"}))["id"].clone();
    let id = ok(
        &mut s,
        "landscape_area_style.create",
        json!({"name": "Meadow", "show_plants": true, "fields": {"hydrozone": "low"},
            "mix": [{"plant": "Lavender", "percent": 60, "density": 5}, {"plant": sedge, "percent": 40, "spacing": 400, "min_count": 10}]}),
    )["id"]
        .as_uid()
        .unwrap();
    let got = ok(&mut s, "landscape_area_style.get", json!({"id": "Meadow"}));
    assert_eq!(got["mix_names"], json!(["Lavender", "Sedge"]));
    assert_eq!(got["total_percent"], 100.0);
    assert_eq!(got["fields"]["hydrozone"], "low");
    let list = ok(&mut s, "landscape_area_style.list", json!({}));
    assert_eq!(list["styles"].as_array().unwrap().len(), 1);
    // Plant styles show how many mixes use them.
    assert_eq!(
        ok(&mut s, "plant_style.get", json!({"id": "Lavender"}))["used_by"],
        1
    );
    let r = ok(
        &mut s,
        "landscape_area_style.update",
        json!({"id": id, "mix": [{"plant": "Sedge"}], "name": "Sedge lawn"}),
    );
    assert_eq!(r["mix_names"], json!(["Sedge"]));
    assert_eq!(r["name"], "Sedge lawn");
    let copy = ok(&mut s, "landscape_area_style.duplicate", json!({"id": id}))["id"]
        .as_uid()
        .unwrap();
    assert_eq!(
        ok(&mut s, "landscape_area_style.get", json!({"id": copy}))["name"],
        "Sedge lawn copy"
    );
    ok(&mut s, "landscape_area_style.delete", json!({"id": copy}));
    // Errors.
    let e = |s: &mut Session, p: Value| fail(s, "landscape_area_style.create", p).0;
    assert_eq!(
        e(&mut s, json!({"name": "A", "mix": [{"plant": "Nope"}]})),
        -32602
    );
    assert_eq!(
        e(&mut s, json!({"name": "A", "mix": [{"percent": 5}]})),
        -32602
    );
    assert_eq!(e(&mut s, json!({"name": "A", "mix": 5})), -32602);
    assert_eq!(e(&mut s, json!({"name": "A", "mix": [5]})), -32602);
    assert_eq!(
        e(
            &mut s,
            json!({"name": "A", "mix": [{"plant": "Sedge", "percent": 150}]})
        ),
        -32602
    );
    assert_eq!(
        e(
            &mut s,
            json!({"name": "A", "mix": [{"plant": "Sedge", "spacing": 5, "density": 5}]})
        ),
        -32602
    );
    assert_eq!(e(&mut s, json!({"name": "A", "wild": 1})), -32602);
    assert_eq!(
        fail(&mut s, "landscape_area_style.get", json!({"id": "Nope"})).0,
        -32602
    );
}

#[test]
fn hardscape_add_and_set() {
    let mut s = Session::with_sequential_ids();
    let st = ok(
        &mut s,
        "hardscape_style.create",
        json!({"name": "Setts", "thickness": 60}),
    )["id"]
        .as_uid()
        .unwrap();
    // A path with a curve.
    let p = ok(
        &mut s,
        "hardscape.add",
        json!({"center": [[0, 0], {"p": [6000, 0], "kind": "arc", "radius": 1500}, [6000, 5000]], "width": 1500, "join": "round", "style": "Setts", "elevation": 20}),
    )["id"]
        .as_uid()
        .unwrap();
    let o = ok(&mut s, "objects.get", json!({"id": p}));
    assert_eq!(o["data"]["hardscape"]["mode"], "path");
    assert_eq!(o["data"]["hardscape"]["style"], st);
    assert!(o["shape"]["boundary"].as_array().unwrap().len() > 8);
    // Set: width, slope, drape, site modifier, then back to a slab.
    ok(
        &mut s,
        "hardscape.set",
        json!({"ids": [p], "width": 2000, "slope": {"type": "cross", "percent": 2}, "drape": true, "site_modifier": false}),
    );
    let d = ok(&mut s, "objects.get", json!({"id": p}))["data"]["hardscape"].clone();
    assert_eq!(
        (
            d["width"].clone(),
            d["drape"].clone(),
            d["slope"]["percent"].clone()
        ),
        (json!(2000.0), json!(true), json!(2.0))
    );
    ok(
        &mut s,
        "hardscape.set",
        json!({"id": p, "slope": null, "mode": "slab", "style": null, "material": "Gravel", "thickness": 100}),
    );
    let o = ok(&mut s, "objects.get", json!({"id": p}));
    assert_eq!(o["data"]["hardscape"]["mode"], "slab");
    assert!(o["data"]["hardscape"]["style"].is_null());
    assert_eq!(o["shape"]["material"], "Gravel");
    // A slab with every option, in a layer and a class.
    let layer = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_layer", "name": "Hard"}}),
    )["created"][0]
        .as_uid()
        .unwrap();
    let slab = ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": square(3000), "mode": "slab", "slope": {"type": "edges", "elevations": [0, 10, 20, 10]}, "layer": layer, "class": "00000000-0000-0000-0000-000000000000", "material": "Slate", "thickness": 50, "site_modifier": true, "drape": false}),
    )["id"]
        .as_uid()
        .unwrap();
    assert_eq!(
        ok(&mut s, "objects.get", json!({"id": slab}))["layer"],
        layer
    );
    // Undo is one step.
    assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
    assert_eq!(fail(&mut s, "objects.get", json!({"id": slab})).0, 2);
    // Errors.
    let f = |s: &mut Session, m: &str, p: Value| fail(s, m, p).0;
    assert_eq!(
        f(&mut s, "hardscape.add", json!({})),
        -32602,
        "needs a boundary or a center line"
    );
    assert_eq!(
        f(
            &mut s,
            "hardscape.add",
            json!({"mode": "path", "boundary": square(10)})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "hardscape.add",
            json!({"mode": "slab", "center": [[0, 0], [5, 0]]})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "hardscape.add",
            json!({"boundary": square(10), "center": [[0, 0], [5, 0]]})
        ),
        3
    );
    assert_eq!(
        f(
            &mut s,
            "hardscape.add",
            json!({"boundary": square(10), "style": "Nope"})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "hardscape.add",
            json!({"boundary": square(10), "mode": "tower"})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "hardscape.add",
            json!({"boundary": square(10), "join": "x"})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "hardscape.add",
            json!({"boundary": square(10), "wibble": 1})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "hardscape.add",
            json!({"boundary": square(10), "width": "wide"})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "hardscape.add",
            json!({"boundary": square(10), "drape": 1})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "hardscape.add",
            json!({"boundary": square(10), "site_modifier": 1})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "hardscape.add",
            json!({"boundary": square(10), "material": 1})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "hardscape.add",
            json!({"boundary": square(10), "thickness": "x"})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "hardscape.add",
            json!({"boundary": square(10), "elevation": "x"})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "hardscape.add",
            json!({"boundary": square(10), "slope": {"type": "tilt"}})
        ),
        -32602
    );
    assert_eq!(f(&mut s, "hardscape.add", json!({"boundary": 5})), -32602);
    assert_eq!(f(&mut s, "hardscape.add", json!({"center": 5})), -32602);
    assert_eq!(
        f(
            &mut s,
            "hardscape.add",
            json!({"boundary": square(10), "layer": "x"})
        ),
        -32602
    );
    assert_eq!(
        f(&mut s, "hardscape.set", json!({"width": 5})),
        -32602,
        "needs an id"
    );
    assert_eq!(
        f(&mut s, "hardscape.set", json!({"id": p})),
        3,
        "nothing to change"
    );
    assert_eq!(
        f(&mut s, "hardscape.set", json!({"id": "x", "width": 5})),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "hardscape.set",
            json!({"id": "00000000-0000-0000-0000-0000000003e7", "width": 5})
        ),
        2
    );
    assert_eq!(
        f(&mut s, "hardscape.set", json!({"id": p, "id2": 1})),
        -32602
    );
    assert_eq!(f(&mut s, "report.hardscape", json!(5)), -32602);
}

#[test]
fn landscape_areas_holes_and_reports() {
    let mut s = setup();
    ok(
        &mut s,
        "landscape_area_style.create",
        json!({"name": "Meadow", "mix": [{"plant": "Lavender", "percent": 60, "density": 4}, {"plant": "Sedge", "percent": 40, "density": 4}]}),
    );
    let pond = ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [5000, 5000], "radius": 1000}}}))["created"][0].as_uid().unwrap();
    let rect = ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "rect", "min": [7000, 1000], "max": [8000, 2000]}}}))["created"][0].as_uid().unwrap();
    let a = ok(
        &mut s,
        "landscape_area.add",
        json!({"boundary": square(10_000), "name": "Bed A", "style": "Meadow", "exclude": [pond, rect], "holes": [[[1000, 1000], [2000, 1000], [2000, 2000]]], "show_plants": true}),
    )["id"]
        .as_uid()
        .unwrap();
    let r = ok(&mut s, "report.landscape_areas", json!({}));
    let row = &r["rows"][0];
    assert_eq!(row["id"], a);
    assert_eq!(row["holes"], 3);
    assert_eq!(row["gross_area_m2"], 100.0);
    let holes = row["holes_area_m2"].as_f64().unwrap();
    // The circle is approximated by 64 points.
    assert!((holes - (0.5 + 1.0 + 3.1365)).abs() < 0.01, "{holes}");
    assert!((row["net_area_m2"].as_f64().unwrap() - (100.0 - holes)).abs() < 1e-9);
    assert_eq!(row["rows"][0]["name"], "Lavender");
    assert_eq!(r["totals"]["gross_area_m2"], 100.0);
    // Set: replace holes, rename, unstyle.
    ok(
        &mut s,
        "landscape_area.set",
        json!({"id": a, "holes": [], "name": "Bed B", "drape": true}),
    );
    let r = ok(&mut s, "report.landscape_areas", json!({"format": "csv"}));
    assert_eq!(r["rows"][0]["name"], "Bed B");
    assert_eq!(r["rows"][0]["holes_area_m2"], 0.0);
    assert!(r["csv"].as_str().unwrap().contains("Lavender"));
    ok(
        &mut s,
        "landscape_area.set",
        json!({"ids": [a], "style": null}),
    );
    assert!(
        ok(&mut s, "report.landscape_areas", json!({}))["rows"][0]["rows"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // A hole outside the area is an error; so are unknown fields.
    let f = |s: &mut Session, m: &str, p: Value| fail(s, m, p).0;
    assert_eq!(
        f(
            &mut s,
            "landscape_area.set",
            json!({"id": a, "holes": [[[-50, 0], [10, 0], [10, 10]]]})
        ),
        3
    );
    assert_eq!(
        f(&mut s, "landscape_area.set", json!({"id": a, "wibble": 1})),
        -32602
    );
    assert_eq!(f(&mut s, "landscape_area.set", json!({"id": a})), 3);
    assert_eq!(
        f(&mut s, "landscape_area.add", json!({"name": "x"})),
        -32602,
        "needs a boundary"
    );
    assert_eq!(
        f(
            &mut s,
            "landscape_area.add",
            json!({"boundary": square(10), "style": "Nope"})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "landscape_area.add",
            json!({"boundary": square(10), "name": 5})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "landscape_area.add",
            json!({"boundary": square(10), "show_plants": 5})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "landscape_area.add",
            json!({"boundary": square(10), "drape": 5})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "landscape_area.add",
            json!({"boundary": square(10), "exclude": ["00000000-0000-0000-0000-0000000003e7"]})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "landscape_area.add",
            json!({"boundary": square(10), "exclude": "x"})
        ),
        -32602
    );
    assert_eq!(
        f(
            &mut s,
            "landscape_area.add",
            json!({"boundary": square(10), "holes": 5})
        ),
        -32602
    );
    let line = ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [1, 1]}}}))["created"][0].as_uid().unwrap();
    assert_eq!(
        f(
            &mut s,
            "landscape_area.add",
            json!({"boundary": square(10), "exclude": [line]})
        ),
        -32602
    );
    // A polyline and another area can be holes.
    let poly = ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "polyline", "closed": true, "points": [[100, 100], [900, 100], [900, 900]]}}}))["created"][0].as_uid().unwrap();
    ok(
        &mut s,
        "landscape_area.set",
        json!({"id": a, "exclude": [poly, a]}),
    );
}

#[test]
fn hardscape_report_and_plant_list() {
    let mut s = setup();
    let st = ok(
        &mut s,
        "hardscape_style.create",
        json!({"name": "Setts", "material": "Granite", "thickness": 60, "components": [{"name": "Base", "material": "Stone", "thickness": 150}], "border": {"width": 200, "material": "Kerb"}}),
    )["id"]
        .as_uid()
        .unwrap();
    ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": square(4000), "style": st}),
    );
    ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": square(1000), "material": "Gravel", "thickness": 100}),
    );
    let r = ok(&mut s, "report.hardscape", json!({"format": "csv"}));
    assert_eq!(r["rows"].as_array().unwrap().len(), 2);
    assert_eq!(r["rows"][0]["plan_area_m2"], 16.0);
    assert_eq!(r["rows"][0]["layers"].as_array().unwrap().len(), 3);
    assert!(
        r["materials"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["material"] == "Stone")
    );
    assert_eq!(r["totals"]["plan_area_m2"], 17.0);
    assert!(r["csv"].as_str().unwrap().starts_with("Id,Name,Style"));
    // Plant list: single plants plus the areas.
    ok(
        &mut s,
        "plants.place",
        json!({"mode": "single", "style": "Lavender", "at": [0, 0]}),
    );
    ok(
        &mut s,
        "landscape_area_style.create",
        json!({"name": "Meadow", "mix": [{"plant": "Lavender", "density": 4}]}),
    );
    ok(
        &mut s,
        "landscape_area.add",
        json!({"boundary": square(2000), "name": "Bed", "style": "Meadow"}),
    );
    let list = ok(&mut s, "report.plant_list", json!({"format": "csv"}));
    assert_eq!(list["rows"][0]["name"], "Lavender");
    assert_eq!(list["rows"][0]["plants"], 1);
    assert_eq!(list["rows"][0]["in_areas"], 16);
    assert_eq!(list["total_count"], 17);
    assert!(list["csv"].as_str().unwrap().contains("Lavender"));
    // The old plant report is unchanged: single plants only.
    assert_eq!(
        ok(&mut s, "report.plants", json!({}))["rows"][0]["count"],
        1
    );
    // Describe has the examples.
    let d = ok(&mut s, "describe", json!(null));
    assert!(d["hardscapes"]["examples"].as_array().unwrap().len() >= 6);
    assert!(describe()["hardscape"].as_str().unwrap().contains("slab"));
}

#[test]
fn drape_uses_the_site_model_in_reports_and_in_3d() {
    let mut s = Session::with_sequential_ids();
    // A ramp: z = x / 10, on 7 by 7 points of 1 m.
    let pts: Vec<Value> =
        (0..7).flat_map(|i| (0..7).map(move |j| json!({"kind": "locus3d", "p": [i as f64 * 1000.0, j as f64 * 1000.0, i as f64 * 100.0]}))).collect();
    let ids: Vec<kadakas_core::uid::Uid> = pts
        .iter()
        .map(|p| {
            ok(
                &mut s,
                "execute",
                json!({"command": {"op": "add_object", "shape": p}}),
            )["created"][0]
                .as_uid()
                .unwrap()
        })
        .collect();
    ok(&mut s, "site.create", json!({"source_ids": ids}));
    let h = ok(&mut s, "hardscape.add", json!({"boundary": [[1000, 1000], [3000, 1000], [3000, 3000], [1000, 3000]], "drape": true, "thickness": 100}))["id"]
        .as_uid()
        .unwrap();
    let r = ok(&mut s, "report.hardscape", json!({}));
    let q = &r["rows"][0];
    assert_eq!(q["draped"], true);
    let want = 4.0 * (1.0f64 + 0.01).sqrt();
    assert!((q["surface_area_m2"].as_f64().unwrap() - want).abs() < 1e-6);
    ok(&mut s, "hardscape.set", json!({"id": h, "drape": false}));
    assert_eq!(
        ok(&mut s, "report.hardscape", json!({}))["rows"][0]["draped"],
        false
    );
    // The 3D scene follows the ground when it drapes.
    ok(&mut s, "hardscape.set", json!({"id": h, "drape": true}));
    let scene = s.scene();
    let (lo, hi) = scene.object_bounds(ObjectId(h)).unwrap();
    assert!(
        hi.z - lo.z > 150.0,
        "the slab follows the 200 mm rise: {lo:?} {hi:?}"
    );
}

fn add_shape(s: &mut Session, shape: Value) -> Value {
    ok(
        s,
        "execute",
        json!({"command": {"op": "add_object", "shape": shape}}),
    )["created"][0]
        .clone()
}

#[test]
fn closed_3d_polylines_are_outlines_and_holes_in_plan_with_a_warning() {
    let mut s = Session::with_sequential_ids();
    // A pond drawn as a closed 3D polyline with sloping heights, and a rectangle: both are taken
    // out of the slab.
    let pond = add_shape(
        &mut s,
        json!({"kind": "polyline3d", "closed": true,
               "points": [[1000, 1000, 50], [3000, 1000, 80], [3000, 3000, 90], [1000, 3000, 60]]}),
    );
    let bin = add_shape(
        &mut s,
        json!({"kind": "rect", "min": [6000, 6000], "max": [7000, 7000]}),
    );
    let id = ok(
        &mut s,
        "hardscape.add",
        json!({"boundary": square(10_000), "exclude": [pond, bin]}),
    )["id"]
        .clone();
    let rep = ok(&mut s, "report.hardscape", json!({"ids": [id]}));
    assert_eq!(rep["rows"][0]["holes"], 2, "{rep}");
    let area = rep["rows"][0]["plan_area_m2"].as_f64().unwrap();
    assert!((area - (100.0 - 4.0 - 1.0)).abs() < 1e-6, "{area}");
    // As a source of its own the 3D polyline becomes a slab, and the reply says that its
    // heights are dropped.
    let r = ok(&mut s, "hardscape.from_shapes", json!({"ids": [pond]}));
    assert_eq!(r["created"].as_array().unwrap().len(), 1, "{r}");
    assert!(
        r["warnings"][0]
            .as_str()
            .unwrap()
            .contains("heights are dropped"),
        "{r}"
    );
    // A flat one has no warning.
    let flat = add_shape(
        &mut s,
        json!({"kind": "polyline3d", "closed": true,
               "points": [[20000, 0, 50], [21000, 0, 50], [21000, 1000, 50]]}),
    );
    let r = ok(
        &mut s,
        "hardscape.from_shapes",
        json!({"ids": [flat], "dry_run": true}),
    );
    assert_eq!(r["warnings"].as_array().unwrap().len(), 0, "{r}");
}
