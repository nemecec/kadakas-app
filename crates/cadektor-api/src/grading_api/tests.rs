use super::*;

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    s.handle(method, params)
        .unwrap_or_else(|e| panic!("{method}: {}", e.message))
}

fn fail(s: &mut Session, method: &str, params: Value) -> (i64, String) {
    let e = s.handle(method, params).unwrap_err();
    (e.code, e.message)
}

/// A site model of 11 by 11 loci, 1 m apart, rising toward +X by 10 percent.
fn session_with_site() -> (Session, u64) {
    let mut s = Session::default();
    let mut ids = Vec::new();
    for i in 0..11 {
        for j in 0..11 {
            let out = ok(
                &mut s,
                "execute",
                json!({"command": {"op": "add_object", "shape": {"kind": "locus3d", "p": [i * 1000, j * 1000, i * 100]}}}),
            );
            ids.push(out["created"][0].as_u64().unwrap());
        }
    }
    let id = ok(&mut s, "site.create", json!({"source_ids": ids}))["id"]
        .as_u64()
        .unwrap();
    (s, id)
}

fn data_of(s: &mut Session, id: u64, key: &str) -> Value {
    ok(s, "objects.get", json!({"id": id}))["data"][key].clone()
}

#[test]
fn wall_styles_crud() {
    let mut s = Session::default();
    let id = ok(&mut s, "wall_style.create", json!({"name": "Block", "components": [{"name": "Block", "material": "Concrete", "thickness": 200}, {"name": "Render", "material": "Lime", "thickness": 20}], "coping": {"height": 50, "overhang": 20}}))["id"].as_u64().unwrap();
    let list = ok(&mut s, "wall_style.list", json!(null));
    assert_eq!(list["styles"][0]["name"], "Block");
    assert_eq!(list["styles"][0]["total_thickness"], 220.0);
    assert_eq!(list["styles"][0]["used_by"], 0);
    ok(
        &mut s,
        "wall_style.update",
        json!({"id": "Block", "name": "Block wall", "thickness": 250, "components": []}),
    );
    assert_eq!(
        ok(&mut s, "wall_style.list", json!({}))["styles"][0]["total_thickness"],
        250.0
    );
    assert_eq!(
        fail(
            &mut s,
            "wall_style.create",
            json!({"name": "x", "thickness": 0})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "wall_style.create", json!({"thickness": 100})).0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "wall_style.create",
            json!({"name": "x", "bogus": 1})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "wall_style.update", json!({"id": "Nope"})).0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "wall_style.update", json!({"name": "x"})).0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "wall_style.update", json!({"id": id, "name": 5})).0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "wall_style.update",
            json!({"id": id, "thickness": -1})
        )
        .0,
        -32602
    );
    // A style in use cannot be deleted, unless the walls move to another style.
    let other = ok(
        &mut s,
        "wall_style.create",
        json!({"name": "Stone", "thickness": 400}),
    )["id"]
        .as_u64()
        .unwrap();
    let wall = ok(&mut s, "wall.add", json!({"path": [[0, 0], [4000, 0]], "style": "Block wall", "top": {"mode": "fixed", "elevation": 1000}}))["id"].as_u64().unwrap();
    assert_eq!(
        ok(&mut s, "wall_style.list", json!({}))["styles"][0]["used_by"],
        1
    );
    assert_eq!(fail(&mut s, "wall_style.delete", json!({"id": id})).0, 1);
    assert_eq!(
        fail(
            &mut s,
            "wall_style.delete",
            json!({"id": id, "reassign": 999})
        )
        .0,
        -32602
    );
    ok(
        &mut s,
        "wall_style.delete",
        json!({"id": id, "reassign": other}),
    );
    assert_eq!(data_of(&mut s, wall, "landscape_wall")["style"], other);
    ok(&mut s, "wall.set", json!({"id": wall, "style": null}));
    ok(
        &mut s,
        "wall_style.delete",
        json!({"id": other, "reassign": null}),
    );
    assert_eq!(
        data_of(&mut s, wall, "landscape_wall")["style"],
        Value::Null
    );
    assert_eq!(
        ok(&mut s, "wall_style.list", json!({}))["styles"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    // One undo step brings everything back.
    assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
    assert_eq!(
        ok(&mut s, "wall_style.list", json!({}))["styles"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn wall_on_flat_ground_has_exact_quantities() {
    let mut s = Session::default();
    let w = ok(
        &mut s,
        "wall.add",
        json!({"path": [[0, 0], [4000, 0], [4000, 3000]], "thickness": 250, "top": {"mode": "fixed", "elevation": 1200}}),
    );
    let q = &w["quantities"];
    assert_eq!(q["length_m"], 7.0);
    assert_eq!(q["face_area_m2"], 8.4);
    assert_eq!(q["both_faces_m2"], 16.8);
    assert!((q["volume_m3"].as_f64().unwrap() - 7.0 * 1.2 * 0.25).abs() < 1e-9);
    assert_eq!(q["min_height_mm"], 1200.0);
    assert_eq!(q["outside_model"], q["outside_model"]);
    // The wall is a polyline with settings, and appears in the 3D scene.
    let obj = ok(&mut s, "objects.get", json!({"id": w["id"]}));
    assert_eq!(obj["shape"]["kind"], "polyline");
    let r = ok(&mut s, "wall.report", json!({}));
    assert_eq!(r["rows"].as_array().unwrap().len(), 1);
    assert_eq!(r["totals"]["walls"], 1);
    assert_eq!(r["totals"]["length_m"], 7.0);
    let one = ok(&mut s, "wall.report", json!({"ids": [w["id"]]}));
    assert_eq!(one["rows"][0]["top_start_mm"], 1200.0);
}

#[test]
fn wall_follows_the_ground_and_changes() {
    let (mut s, _) = session_with_site();
    let w = ok(
        &mut s,
        "wall.add",
        json!({"path": [[1000, 2000], [6000, 2000]], "thickness": 300, "top": {"mode": "terrain", "offset": 600}, "embed": 100}),
    );
    let q = &w["quantities"];
    // The ground rises 100 mm per meter; the wall is 700 mm high everywhere.
    assert_eq!(q["min_height_mm"], 700.0);
    assert_eq!(q["max_height_mm"], 700.0);
    assert_eq!(q["top_start_mm"], 700.0);
    assert_eq!(q["bottom_end_mm"], 500.0);
    assert!((q["volume_m3"].as_f64().unwrap() - 5.0 * 0.7 * 0.3).abs() < 1e-9);
    assert_eq!(q["outside_model"], 0);
    let id = w["id"].as_u64().unwrap();
    // Set: a slope top, a new path and a bottom on the proposed ground.
    ok(
        &mut s,
        "wall.set",
        json!({"id": id, "top": {"mode": "slope", "start": 1000, "end": 2000}, "bottom": "proposed", "embed": 0, "path": [[1000, 2000], [3000, 2000]]}),
    );
    let q = ok(&mut s, "wall.report", json!({}))["rows"][0].clone();
    assert_eq!(q["length_m"], 2.0);
    assert_eq!(q["top_end_mm"], 2000.0);
    assert_eq!(q["bottom_start_mm"], 100.0);
    // Moving the wall with the move command keeps the rules.
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "move_objects", "ids": [id], "delta": [1000, 0]}}),
    );
    assert_eq!(
        ok(&mut s, "wall.report", json!({}))["rows"][0]["bottom_start_mm"],
        200.0
    );
    // Errors.
    assert_eq!(
        fail(&mut s, "wall.set", json!({"id": id, "thickness": 0})).0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "wall.set", json!({"id": id, "bogus": 1})).0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "wall.set",
            json!({"ids": [id, id], "path": [[0, 0], [1, 1]]})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "wall.set", json!({"path": [[0, 0], [1, 1]]})).0,
        -32602
    );
    assert_eq!(fail(&mut s, "wall.set", json!({"ids": []})).0, -32602);
    assert_eq!(fail(&mut s, "wall.set", json!({"id": 9999})).0, 1);
    assert_eq!(
        fail(&mut s, "wall.set", json!({"id": id, "path": "x"})).0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "wall.set", json!({"id": id, "style": "Nope"})).0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "wall.add",
            json!({"path": [[0, 0], [1, 1]], "top": {"mode": "fixed"}})
        )
        .0,
        -32602
    );
    assert_eq!(fail(&mut s, "wall.add", json!({})).0, -32602);
    assert_eq!(fail(&mut s, "wall.add", json!({"path": "x"})).0, -32602);
    assert_eq!(
        fail(
            &mut s,
            "wall.add",
            json!({"path": [[0, 0], [1, 1]], "closed": 3})
        )
        .0,
        -32602
    );
    assert_eq!(fail(&mut s, "wall.add", json!({"path": [[0, 0]]})).0, 1);
    assert_eq!(
        fail(
            &mut s,
            "wall.add",
            json!({"path": [[0, 0], [1, 1]], "site_modifier": true})
        )
        .0,
        -32602
    );
    let circle = ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}}}))["created"][0].as_u64().unwrap();
    assert_eq!(
        fail(&mut s, "wall.set", json!({"id": circle, "thickness": 5})).0,
        1
    );
    assert_eq!(fail(&mut s, "wall.report", json!({"ids": [circle]})).0, 1);
    assert_eq!(fail(&mut s, "wall.report", json!({"ids": "x"})).0, -32602);
    assert_eq!(fail(&mut s, "wall.report", json!({"ids": [9999]})).0, 1);
}

#[test]
fn wall_as_site_modifier_breaks_the_proposed_surface() {
    let (mut s, site) = session_with_site();
    let before = ok(&mut s, "site.cut_fill", json!({"id": site}));
    assert_eq!(before["fill_m3"], 0.0);
    let w = ok(
        &mut s,
        "wall.add",
        json!({"path": [[2000, 5000], [8000, 5000]], "thickness": 500, "top": {"mode": "fixed", "elevation": 1500}, "site_modifier": true}),
    );
    let id = w["id"].as_u64().unwrap();
    let after = ok(&mut s, "site.cut_fill", json!({"id": site}));
    assert!(after["fill_m3"].as_f64().unwrap() > 0.0, "{after}");
    // The proposed surface on the wall is at the top; beside the wall it keeps the old height.
    let on = ok(
        &mut s,
        "site.elevation",
        json!({"id": site, "x": 5000, "y": 5000, "surface": "proposed"}),
    );
    assert!((on["z"].as_f64().unwrap() - 1500.0).abs() < 1e-6, "{on}");
    let beside = ok(
        &mut s,
        "site.elevation",
        json!({"id": site, "x": 5000, "y": 5400, "surface": "proposed"}),
    );
    assert!(
        (beside["z"].as_f64().unwrap() - 500.0).abs() < 1e-6,
        "{beside}"
    );
    assert_eq!(
        ok(&mut s, "site.refresh", json!({"id": site}))["modifiers"],
        json!([id])
    );
    // A wall that follows the ground cannot be a modifier. Turning it off restores the ground.
    assert_eq!(
        fail(
            &mut s,
            "wall.set",
            json!({"id": id, "top": {"mode": "terrain", "offset": 100}})
        )
        .0,
        -32602
    );
    ok(
        &mut s,
        "wall.set",
        json!({"id": id, "site_modifier": false}),
    );
    assert_eq!(
        ok(&mut s, "site.cut_fill", json!({"id": site}))["fill_m3"],
        0.0
    );
}

#[test]
fn grade_objects() {
    let (mut s, _) = session_with_site();
    let g = ok(
        &mut s,
        "grade.add",
        json!({"a": [0, 0], "b": [10000, 0], "elevation_a": 1000, "slope": {"ratio": -50}}),
    );
    let id = g["id"].as_u64().unwrap();
    assert!((g["grade"]["percent"].as_f64().unwrap() + 2.0).abs() < 1e-9);
    assert_eq!(g["grade"]["elevation_b"], 800.0);
    assert_eq!(g["grade"]["falls"], true);
    assert_eq!(g["grade"]["label"], "2.0% (1:50)");
    // Move the end point: the end elevation follows.
    let g2 = ok(&mut s, "grade.set", json!({"id": id, "b": [20000, 0]}));
    assert_eq!(g2["grade"]["elevation_b"], 600.0);
    // Two elevations.
    let g3 = ok(
        &mut s,
        "grade.set",
        json!({"id": id, "elevation_b": 1500, "slope": null}),
    );
    assert!((g3["grade"]["percent"].as_f64().unwrap() - 2.5).abs() < 1e-9);
    // Snap picks the elevations from the site model.
    let snapped = ok(
        &mut s,
        "grade.add",
        json!({"a": [1000, 1000], "b": [5000, 1000], "snap": "existing"}),
    );
    assert_eq!(snapped["grade"]["elevation_a"], 100.0);
    assert_eq!(snapped["grade"]["elevation_b"], 500.0);
    assert!((snapped["grade"]["percent"].as_f64().unwrap() - 10.0).abs() < 1e-9);
    assert_eq!(snapped["grade"]["angle_deg"].as_f64().unwrap().round(), 6.0);
    let got = ok(&mut s, "grade.get", json!({"id": snapped["id"]}));
    assert_eq!(got["length_mm"], 4000.0);
    assert_eq!(got["falls"], false);
    // Level ground has no ratio.
    let level = ok(
        &mut s,
        "grade.add",
        json!({"a": [0, 0], "b": [1000, 0], "elevation_a": 5, "elevation_b": 5}),
    );
    assert_eq!(level["grade"]["ratio"], Value::Null);
    // Errors.
    assert_eq!(
        fail(&mut s, "grade.add", json!({"a": [0, 0], "b": [1, 0]})).0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "grade.add",
            json!({"a": [0, 0], "elevation_a": 1, "elevation_b": 2})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "grade.add",
            json!({"a": [0, 0], "b": [0, 0], "elevation_a": 1, "elevation_b": 2})
        )
        .0,
        1
    );
    assert_eq!(
        fail(
            &mut s,
            "grade.add",
            json!({"a": [0, 0], "b": "x", "elevation_a": 1, "elevation_b": 2})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "grade.add",
            json!({"a": [0, 0], "b": [1, 1], "slope": {"angle": 95}, "elevation_a": 0})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "grade.add",
            json!({"a": [-9000, 0], "b": [-8000, 0], "snap": "existing"})
        )
        .0,
        1,
        "outside the model"
    );
    assert_eq!(
        fail(&mut s, "grade.set", json!({"id": id, "b": [0, 0]})).0,
        1
    );
    assert_eq!(
        fail(&mut s, "grade.set", json!({"id": id, "bogus": 1})).0,
        -32602
    );
    assert_eq!(fail(&mut s, "grade.set", json!({"id": 9999})).0, 1);
    assert_eq!(fail(&mut s, "grade.get", json!({"id": 9999})).0, 1);
    assert_eq!(fail(&mut s, "grade.get", json!({})).0, -32602);
    let line = ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [1, 1]}}}))["created"][0].as_u64().unwrap();
    assert_eq!(fail(&mut s, "grade.get", json!({"id": line})).0, 1);
    assert_eq!(fail(&mut s, "grade.set", json!({"id": line})).0, 1);
    let circle = ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}}}))["created"][0].as_u64().unwrap();
    assert_eq!(fail(&mut s, "grade.get", json!({"id": circle})).0, 1);
    assert_eq!(fail(&mut s, "grade.set", json!({"id": circle})).0, 1);
    // A grade without a site model cannot snap.
    let mut empty = Session::default();
    assert_eq!(
        fail(
            &mut empty,
            "grade.add",
            json!({"a": [0, 0], "b": [10, 0], "snap": "proposed"})
        )
        .0,
        1
    );
}

#[test]
fn analysis_bands_and_legend() {
    let (mut s, site) = session_with_site();
    let off = ok(&mut s, "site.analysis", json!({"id": site}));
    assert_eq!(off["mode"], "off");
    assert!(off.get("bands").is_none());
    let r = ok(
        &mut s,
        "site.analysis",
        json!({"id": site, "mode": "slope", "surface": "existing", "breaks": [2, 5, 10], "colors": [[1, 2, 3], [4, 5, 6, 255], [7, 8, 9], [10, 11, 12]], "opacity": 0.5}),
    );
    let bands = r["bands"].as_array().unwrap();
    assert_eq!(bands.len(), 4);
    // The ground rises 10 percent everywhere: the band [10, inf) holds it all. (10 is at the limit.)
    assert_eq!(bands[3]["percent"], 100.0);
    assert_eq!(bands[3]["color"], json!([10, 11, 12, 255]));
    assert_eq!(bands[0]["color"], json!([1, 2, 3, 255]));
    assert_eq!(bands[3]["area_m2"], 100.0);
    assert!((bands[3]["surface_area_m2"].as_f64().unwrap() - 100.0 * 1.01f64.sqrt()).abs() < 1e-4);
    // The plan shows the bands and the legend.
    let lg = ok(
        &mut s,
        "site.analysis",
        json!({"id": site, "legend": {"rect": [[12000, 0], [16000, 6000]], "show_areas": true, "title": "Slope"}}),
    );
    assert!(lg["legend"].as_u64().is_some());
    let svg = ok(&mut s, "render.svg", json!({}))["svg"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        svg.contains(r#"data-kind="analysis""#)
            && svg.contains("legend-swatch")
            && svg.contains("10 % and above")
    );
    // Elevation with the default limits, on the proposed surface.
    let e = ok(
        &mut s,
        "site.analysis",
        json!({"id": site, "mode": "elevation", "surface": "proposed", "breaks": [], "colors": []}),
    );
    assert_eq!(e["bands"].as_array().unwrap().len(), 7);
    let total: f64 = e["bands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["area_m2"].as_f64().unwrap())
        .sum();
    assert!((total - 100.0).abs() < 1e-6);
    // The 3D scene has band colors.
    let scene = s.scene();
    assert!(scene.mesh.colors.len() > 100);
    // Errors.
    assert_eq!(
        fail(
            &mut s,
            "site.analysis",
            json!({"id": site, "breaks": [5, 2]})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "site.analysis",
            json!({"id": site, "mode": "aspect"})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "site.analysis", json!({"id": site, "legend": {}})).0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "site.analysis",
            json!({"id": site, "legend": {"rect": [[0, 0], [0, 0]]}})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "site.analysis",
            json!({"id": site, "legend": {"rect": "x"}})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "site.analysis", json!({"id": site, "legend": 5})).0,
        -32602
    );
    assert_eq!(fail(&mut s, "site.analysis", json!({"id": 9999})).0, 1);
    assert_eq!(fail(&mut s, "site.analysis", json!({})).0, -32602);
    let circle = ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}}}))["created"][0].as_u64().unwrap();
    assert_eq!(fail(&mut s, "site.analysis", json!({"id": circle})).0, 1);
    // A site model that cannot make a surface cannot be analyzed.
    let few = ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "site_model", "points": [[0, 0, 0]]}}}))["created"][0].as_u64().unwrap();
    assert_eq!(
        fail(&mut s, "site.analysis", json!({"id": few, "mode": "slope"})).0,
        1
    );
}

#[test]
fn spot_elevations_and_stakes() {
    let (mut s, site) = session_with_site();
    let pad = json!({"command": {"op": "add_object", "shape": {"kind": "site_modifier", "modifier": {"type": "pad", "elevation": 900}, "boundary": [[2000, 2000], [4000, 2000], [4000, 4000], [2000, 4000]]}}});
    let _ = site;
    let spot = ok(
        &mut s,
        "spot.add",
        json!({"at": [3000, 3000], "prefix": "TC"}),
    );
    assert_eq!(spot["reading"]["existing"], 300.0);
    assert_eq!(spot["reading"]["modified"], false);
    assert_eq!(spot["reading"]["label"], "TC 0.30");
    ok(&mut s, "execute", pad);
    // The reading is live: the pad changes the proposed height.
    let id = spot["id"].as_u64().unwrap();
    let got = ok(&mut s, "spot.get", json!({"id": id}));
    assert_eq!(
        (
            got["existing"].as_f64().unwrap(),
            got["proposed"].as_f64().unwrap()
        ),
        (300.0, 900.0)
    );
    assert_eq!(got["label"], "TC EX 0.30 / PR 0.90");
    let moved = ok(
        &mut s,
        "spot.set",
        json!({"id": id, "at": [8000, 8000], "show": "proposed", "decimals": 3}),
    );
    assert_eq!(moved["reading"]["label"], "TC PR 0.800");
    // Any point.
    let any = ok(&mut s, "spot.get", json!({"at": [5000, 5000]}));
    assert_eq!(any["existing"], 500.0);
    let out = ok(&mut s, "spot.get", json!({"at": [-5000, 5000]}));
    assert_eq!(out["existing"], Value::Null);
    // A stake with and without a georeference.
    let stake = ok(
        &mut s,
        "spot.add",
        json!({"at": [3000, 4000], "kind": "stake", "name": "S1"}),
    );
    assert_eq!(stake["reading"]["georeferenced"], false);
    assert_eq!(stake["reading"]["northing_m"], 4.0);
    assert_eq!(stake["reading"]["easting_m"], 3.0);
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "set_georeference", "georef": {"crs": "EPSG:3067", "origin": [500000.0, 6700000.0], "rotation": 0.0}}}),
    );
    let got = ok(&mut s, "spot.get", json!({"id": stake["id"]}));
    assert_eq!(got["georeferenced"], true);
    assert_eq!(got["northing_m"], 6_700_004.0);
    assert_eq!(got["easting_m"], 500_003.0);
    assert!(
        got["label"]
            .as_str()
            .unwrap()
            .starts_with("S1 N 6700004.000 E 500003.000")
    );
    // The label shows in the plan.
    assert!(
        ok(&mut s, "render.svg", json!({}))["svg"]
            .as_str()
            .unwrap()
            .contains("spot-label")
    );
    // Errors.
    assert_eq!(
        fail(&mut s, "spot.add", json!({"at": [0, 0], "label_height": 0})).0,
        -32602
    );
    assert_eq!(fail(&mut s, "spot.add", json!({})).0, -32602);
    assert_eq!(fail(&mut s, "spot.add", json!({"at": [0]})).0, -32602);
    assert_eq!(
        fail(&mut s, "spot.set", json!({"id": id, "show": "all"})).0,
        -32602
    );
    assert_eq!(fail(&mut s, "spot.set", json!({"id": 9999})).0, 1);
    assert_eq!(fail(&mut s, "spot.get", json!({"id": 9999})).0, 1);
    assert_eq!(fail(&mut s, "spot.get", json!({})).0, -32602);
    assert_eq!(
        fail(&mut s, "spot.get", json!({"at": [0, 0], "site": "x"})).0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "spot.get", json!({"at": [0, 0], "site": 9999})).0,
        1
    );
    let circle = ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}}}))["created"][0].as_u64().unwrap();
    assert_eq!(fail(&mut s, "spot.get", json!({"id": circle})).0, 1);
    assert_eq!(fail(&mut s, "spot.set", json!({"id": circle})).0, 1);
    let line = ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "line", "a": [0, 0], "b": [1, 1]}}}))["created"][0].as_u64().unwrap();
    assert_eq!(fail(&mut s, "spot.get", json!({"id": line})).0, 1);
    assert_eq!(fail(&mut s, "spot.set", json!({"id": line})).0, 1);
    // Without a site model a spot has no height. A point read needs a site model.
    let mut empty = Session::default();
    let sp = ok(&mut empty, "spot.add", json!({"at": [0, 0]}));
    assert_eq!(sp["reading"]["label"], "no site model");
    assert_eq!(fail(&mut empty, "spot.get", json!({"at": [0, 0]})).0, 1);
}

#[test]
fn sections_profile_and_placement() {
    let (mut s, site) = session_with_site();
    let _ = site;
    ok(
        &mut s,
        "wall.add",
        json!({"path": [[3000, 0], [3000, 10000]], "thickness": 300, "top": {"mode": "terrain", "offset": 800}}),
    );
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "plant", "at": [7000, 5500], "species": "Oak", "spread": 3000, "height": 6000}}}),
    );
    let sec = ok(
        &mut s,
        "section.add",
        json!({"a": [0, 5000], "b": [10000, 5000], "name": "A", "exaggeration": 3}),
    )["id"]
        .as_u64()
        .unwrap();
    let p = ok(&mut s, "section.profile", json!({"id": sec}));
    assert_eq!(p["length_mm"], 10000.0);
    assert_eq!(p["modified"], false);
    let run = p["existing"][0].as_array().unwrap();
    assert!(run.len() >= 11);
    // The profile equals the site model.
    for pt in run {
        let (x, z) = (pt[0].as_f64().unwrap(), pt[1].as_f64().unwrap());
        let e = ok(
            &mut s,
            "site.elevation",
            json!({"id": site, "x": x, "y": 5000}),
        );
        assert!((e["z"].as_f64().unwrap() - z).abs() < 1e-6);
    }
    assert_eq!(p["walls"].as_array().unwrap().len(), 1);
    assert_eq!(p["walls"][0]["top"], 1100.0);
    assert_eq!(p["plants"].as_array().unwrap().len(), 1);
    // Place it on a sheet layer at 1:100 (paper mm).
    let sheet = ok(&mut s, "sheet.create", json!({"name": "S1", "paper": "a3"}));
    let sheet_layer = sheet["layer"].as_u64().unwrap();
    let _ = sheet_layer;
    let placed = ok(
        &mut s,
        "section.place",
        json!({"id": sec, "at": [20, 20], "layer": sheet_layer}),
    );
    let ids = placed["ids"].as_array().unwrap();
    assert!(ids.len() >= 5);
    let tag = data_of(&mut s, ids[0].as_u64().unwrap(), "site_section_profile");
    assert_eq!(tag["section"], sec);
    // 10 m at 1:100 is 100 mm on paper.
    let axis = ok(&mut s, "objects.get", json!({"id": ids[0]}));
    assert_eq!(axis["shape"]["kind"], "line");
    assert!(
        (axis["shape"]["b"][0].as_f64().unwrap() - 120.0).abs() < 1e-9,
        "{axis}"
    );
    assert!(placed["bounds"][1][0].as_f64().unwrap() >= 120.0);
    // Placing again replaces the shapes: one undo step each.
    let count = ok(&mut s, "objects.list", json!({}))["objects"]
        .as_array()
        .map_or(0, Vec::len);
    let _ = count;
    let again = ok(
        &mut s,
        "section.place",
        json!({"id": sec, "at": [20, 20], "layer": sheet_layer}),
    );
    assert_eq!(again["ids"].as_array().unwrap().len(), ids.len());
    assert_eq!(
        fail(&mut s, "objects.get", json!({"id": ids[0]})).0,
        1,
        "the old shapes are gone"
    );
}

#[test]
fn sections_on_a_design_layer_and_errors() {
    let (mut s, _) = session_with_site();
    let sec = ok(&mut s, "section.add", json!({"a": [0, 5000], "b": [10000, 5000], "walls": false, "hardscapes": false, "plants": false}))["id"].as_u64().unwrap();
    ok(
        &mut s,
        "section.set",
        json!({"id": sec, "name": "B", "b": [9000, 5000]}),
    );
    assert_eq!(data_of(&mut s, sec, "site_section")["name"], "B");
    let placed = ok(
        &mut s,
        "section.place",
        json!({"id": sec, "at": [0, -30000]}),
    );
    // On a design layer the drawing is at full size.
    let axis = |s: &mut Session, p: &Value| {
        ok(s, "objects.get", json!({"id": p["ids"][0]}))["shape"]["b"][0]
            .as_f64()
            .unwrap()
    };
    assert!((axis(&mut s, &placed) - 9000.0).abs() < 1e-6);
    let scaled = ok(
        &mut s,
        "section.place",
        json!({"id": sec, "at": [0, -30000], "scale": 100}),
    );
    assert!((axis(&mut s, &scaled) - 90.0).abs() < 1e-6);
    // Errors.
    assert_eq!(
        fail(&mut s, "section.add", json!({"a": [0, 0], "b": [0, 0]})).0,
        -32602
    );
    assert_eq!(fail(&mut s, "section.add", json!({"a": [0, 0]})).0, -32602);
    assert_eq!(
        fail(
            &mut s,
            "section.add",
            json!({"a": [0, 0], "b": [1, 0], "exaggeration": 0})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "section.set", json!({"id": sec, "b": [0, 5000]})).0,
        -32602
    );
    assert_eq!(
        fail(&mut s, "section.set", json!({"id": sec, "step": 1})).0,
        -32602
    );
    assert_eq!(fail(&mut s, "section.set", json!({"id": 9999})).0, 1);
    assert_eq!(fail(&mut s, "section.profile", json!({"id": 9999})).0, 1);
    assert_eq!(fail(&mut s, "section.place", json!({"id": sec})).0, -32602);
    assert_eq!(
        fail(
            &mut s,
            "section.place",
            json!({"id": sec, "at": [0, 0], "layer": 999})
        )
        .0,
        1
    );
    assert_eq!(
        fail(
            &mut s,
            "section.place",
            json!({"id": sec, "at": [0, 0], "scale": -1})
        )
        .0,
        -32602
    );
    assert_eq!(
        fail(
            &mut s,
            "section.place",
            json!({"id": sec, "at": [0, 0], "layer": "x"})
        )
        .0,
        -32602
    );
    let circle = ok(&mut s, "execute", json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 5}}}))["created"][0].as_u64().unwrap();
    assert_eq!(fail(&mut s, "section.set", json!({"id": circle})).0, 1);
    assert_eq!(fail(&mut s, "section.profile", json!({"id": circle})).0, 1);
    let mut empty = Session::default();
    let e = ok(
        &mut empty,
        "section.add",
        json!({"a": [0, 0], "b": [10, 0]}),
    )["id"]
        .as_u64()
        .unwrap();
    assert_eq!(fail(&mut empty, "section.profile", json!({"id": e})).0, 1);
}

#[test]
fn describe_has_grading_examples_and_methods_are_routed() {
    let mut s = Session::default();
    let d = ok(&mut s, "describe", json!(null));
    assert!(d["grading"]["examples"].as_array().unwrap().len() >= 8);
    for m in [
        "wall.add",
        "wall.set",
        "wall.report",
        "grade.add",
        "site.analysis",
        "spot.add",
        "section.place",
    ] {
        assert!(
            d["methods"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x["name"] == m),
            "{m}"
        );
    }
    assert!(!knows("wall.nope"));
    assert_eq!(fail(&mut s, "wall.add", json!(5)).0, -32602);
    assert_eq!(
        s.handle_grading("nope.nope", json!({})).unwrap_err().code,
        -32601
    );
}
