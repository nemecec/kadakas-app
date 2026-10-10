//! Tests of the care fields of plant styles: image, maintenance, irrigation, plant tag format
//! and the hydrozone report.

use cadektor_core::uid::AsUid;
use serde_json::{Value, json};

use crate::Session;

fn ok(s: &mut Session, method: &str, p: Value) -> Value {
    s.handle(method, p)
        .unwrap_or_else(|e| panic!("{method}: {}", e.message))
}

fn style(s: &mut Session, name: &str, need: &str, zone: &str) -> cadektor_core::uid::Uid {
    ok(
        s,
        "plant_style.create",
        json!({
            "name": name, "id_code": &name[..2].to_uppercase(), "common_name": name,
            "botanical_name": format!("{name} botanicus"), "size": "P9", "container": "C2",
            "spread": 2000, "height": 1000,
            "irrigation": {"water_need": need, "hydrozone": zone, "irrigated": true},
            "maintenance": {"interval": "monthly", "hours_per_year": 0.5, "pruning": "spring"},
        }),
    )["id"]
        .as_uid()
        .unwrap()
}

fn plant(s: &mut Session, style: cadektor_core::uid::Uid, x: i64) -> cadektor_core::uid::Uid {
    ok(
        s,
        "plants.place",
        json!({"mode": "single", "style": style, "at": [x, 0]}),
    )["created"][0]
        .as_uid()
        .unwrap()
}

#[test]
fn styles_keep_maintenance_irrigation_and_an_image() {
    let mut s = Session::with_sequential_ids();
    let id = style(&mut s, "Lavender", "low", "Z1");
    let got = ok(&mut s, "plant_style.get", json!({"id": id}));
    assert_eq!(got["irrigation"]["water_need"], "low");
    assert_eq!(got["maintenance"]["hours_per_year"], 0.5);
    assert!(got["image"].is_null());
    // A texture is the image of a style. Other resources are refused.
    let tex = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_resource", "name": "Lavender photo", "body": {"type": "texture", "color": [150, 140, 200, 255]}}}),
    )["created"][0]
        .as_uid()
        .unwrap();
    ok(
        &mut s,
        "plant_style.update",
        json!({"id": id, "image": tex}),
    );
    assert_eq!(
        ok(&mut s, "plant_style.get", json!({"id": id}))["image"],
        tex
    );
    assert!(
        s.handle("plant_style.update", json!({"id": id, "image": id}))
            .is_err()
    );
    assert!(
        s.handle("plant_style.update", json!({"id": id, "image": 9999}))
            .is_err()
    );
    // The texture cannot be deleted while a style uses it.
    assert!(
        s.handle(
            "execute",
            json!({"command": {"op": "delete_resource", "id": tex}})
        )
        .is_err()
    );
    for bad in [
        json!({"id": id, "irrigation": {"plant_factor": 9}}),
        json!({"id": id, "irrigation": {"water_need": "wet"}}),
        json!({"id": id, "maintenance": {"hours_per_year": -2}}),
    ] {
        assert!(s.handle("plant_style.update", bad).is_err());
    }
    // The CSV catalog has the columns.
    let csv = ok(&mut s, "plant_style.export_csv", json!({}))["csv"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(csv.contains("water_need") && csv.contains("hydrozone") && csv.contains("low,Z1"));
    let mut t = Session::with_sequential_ids();
    let r = ok(&mut t, "plant_style.import_csv", json!({"csv": csv}));
    assert!(r["warnings"].as_array().unwrap().is_empty(), "{r}");
    let list = ok(&mut t, "plant_style.list", json!({}));
    assert_eq!(list["styles"][0]["irrigation"]["hydrozone"], "Z1");
    assert_eq!(list["styles"][0]["maintenance"]["hours_per_year"], 0.5);
}

#[test]
fn plant_tags_have_three_levels_and_an_id_bubble() {
    let mut s = Session::with_sequential_ids();
    let id = style(&mut s, "Lavender", "low", "Z1");
    let r = ok(
        &mut s,
        "plants.place",
        json!({"mode": "single", "style": id, "at": [0, 0], "tag": true}),
    );
    let tag = r["tag"].as_uid().expect("a tag");
    let svg = |s: &mut Session| {
        let v = ok(s, "render.svg", json!({}));
        v["svg"]
            .as_str()
            .or_else(|| v.as_str())
            .unwrap()
            .to_string()
    };
    let one = svg(&mut s);
    assert!(
        one.contains("LA (1)") && !one.contains(">Lavender<"),
        "{one}"
    );
    let r = ok(&mut s, "plant_tag.set", json!({"ids": [tag], "levels": 2}));
    assert_eq!(
        (r["levels"].clone(), r["bubble"].clone()),
        (json!(2), json!(false))
    );
    let two = svg(&mut s);
    assert!(two.contains("LA (1)") && two.contains(">Lavender<"));
    ok(
        &mut s,
        "plant_tag.set",
        json!({"ids": [tag], "levels": 3, "bubble": true}),
    );
    let three = svg(&mut s);
    assert!(three.contains("plant_tag_bubble"), "{three}");
    assert!(three.contains(">LA<") && three.contains(">(1)<"));
    assert!(three.contains("P9, C2"), "level 3 shows size and container");
    // Back to one level and no bubble: the SVG is as before.
    ok(
        &mut s,
        "plant_tag.set",
        json!({"ids": [tag], "levels": 1, "bubble": false}),
    );
    assert_eq!(svg(&mut s), one);
    for bad in [
        json!({}),
        json!({"ids": []}),
        json!({"ids": [tag], "levels": 4}),
        json!({"ids": [tag], "bubble": "yes"}),
        json!({"ids": ["00000000-0000-0000-0000-00000000270f"]}),
    ] {
        assert!(s.handle("plant_tag.set", bad).is_err());
    }
    // A plant is not a tag.
    let p = plant(&mut s, id, 5000);
    assert!(s.handle("plant_tag.set", json!({"ids": [p]})).is_err());
}

#[test]
fn the_hydrozone_report_links_plants_to_irrigation_zones() {
    let mut s = Session::with_sequential_ids();
    let lav = style(&mut s, "Lavender", "low", "Z1");
    let hyd = style(&mut s, "Hydrangea", "high", "Z1");
    let box_ = style(&mut s, "Box", "moderate", "Z2");
    plant(&mut s, lav, 0);
    plant(&mut s, lav, 3000);
    plant(&mut s, hyd, 6000);
    plant(&mut s, box_, 9000);
    // Without a network: the plants are grouped, and no zone is linked.
    let r = ok(&mut s, "irrigation.report.hydrozones", json!({"eto": 1000}));
    assert_eq!(r["rows"].as_array().unwrap().len(), 2);
    assert_eq!(r["rows"][0]["hydrozone"], "Z1");
    assert_eq!(r["rows"][0]["plants"], 3);
    assert_eq!(r["rows"][0]["mixed"], true);
    assert!(r["rows"][0].get("zone").is_none());
    assert!(
        r["warnings"][0]
            .as_str()
            .unwrap()
            .contains("more than one class apart")
    );
    // With a network: zone 1 has an outlet.
    ok(
        &mut s,
        "irrigation.poc.add",
        json!({"at": [0, 5000], "static_pressure": 400, "available_flow": 100}),
    );
    ok(
        &mut s,
        "irrigation.line.add",
        json!({"points": [[0, 5000], [2000, 5000]], "line_type": "mainline", "size": "50", "material": "PVC"}),
    );
    ok(
        &mut s,
        "irrigation.valve.add",
        json!({"at": [2000, 5000], "zone": 1, "kv": 6}),
    );
    ok(
        &mut s,
        "irrigation.line.add",
        json!({"points": [[2000, 5000], [4000, 5000]], "line_type": "lateral", "size": "32", "material": "PE"}),
    );
    ok(
        &mut s,
        "irrigation.outlet.add",
        json!({"at": [4000, 5000], "model": "Spray-4.0", "arc": 180, "direction": 90}),
    );
    let r = ok(&mut s, "irrigation.report.hydrozones", json!({}));
    let z1 = &r["rows"][0];
    assert_eq!(z1["zone"]["zone"], 1);
    assert!(z1["zone"]["flow_lpm"].as_f64().unwrap() > 0.0);
    let text = r["warnings"].to_string();
    assert!(
        text.contains("hydrozone 'Z2' has no irrigation zone 2"),
        "{text}"
    );
    let csv = ok(
        &mut s,
        "irrigation.report.hydrozones",
        json!({"format": "csv"}),
    );
    assert!(csv["csv"].as_str().unwrap().starts_with("Hydrozone,Plants"));
    assert!(
        s.handle("irrigation.report.hydrozones", json!({"eto": 0}))
            .is_err()
    );
    assert!(
        s.handle("irrigation.report.hydrozones", json!({"eto": "x"}))
            .is_err()
    );
}

#[test]
fn plants_without_a_hydrozone_are_reported() {
    let mut s = Session::with_sequential_ids();
    let st = style(&mut s, "Aster", "moderate", "");
    plant(&mut s, st, 0);
    let r = ok(&mut s, "irrigation.report.hydrozones", json!({}));
    assert!(r["warnings"][0].as_str().unwrap().contains("no hydrozone"));
}

fn tilted_site(s: &mut Session) -> cadektor_core::uid::Uid {
    // A plane that rises 10 mm per m along X, from 100 mm.
    let mut ids = Vec::new();
    for x in 0..5 {
        for y in 0..5 {
            let r = ok(
                s,
                "execute",
                json!({"command": {"op": "add_object", "shape": {"kind": "locus3d", "p": [x * 5000, y * 5000, 100 + x * 50]}}}),
            );
            ids.push(r["created"][0].as_uid().unwrap());
        }
    }
    ok(s, "site.create", json!({"source_ids": ids}))["id"]
        .as_uid()
        .unwrap()
}

#[test]
fn plants_stand_on_the_terrain_when_placed_and_when_draped() {
    let mut s = Session::with_sequential_ids();
    tilted_site(&mut s);
    let st = style(&mut s, "Lavender", "low", "Z1");
    let r = ok(
        &mut s,
        "plants.place",
        json!({"mode": "path", "style": st, "path": [[0, 2000], [10000, 2000]], "spacing": 5000, "on_terrain": true}),
    );
    let ids: Vec<cadektor_core::uid::Uid> = r["created"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_uid().unwrap())
        .collect();
    assert_eq!(ids.len(), 3);
    let z = |s: &mut Session, id: cadektor_core::uid::Uid| {
        ok(s, "objects.get", json!({"id": id}))["data"]["ground_z"].as_f64()
    };
    assert!((z(&mut s, ids[0]).unwrap() - 100.0).abs() < 1e-6);
    assert!((z(&mut s, ids[2]).unwrap() - 200.0).abs() < 1e-6);
    // One undo step removes the plants and their heights.
    assert!(ok(&mut s, "undo", json!({}))["done"].as_bool().unwrap());
    assert_eq!(ok(&mut s, "document.summary", json!({}))["object_count"], 1);
    // A plant placed without it can be draped later, and released.
    let p = plant(&mut s, st, 5000);
    assert_eq!(z(&mut s, p), None);
    let r = ok(&mut s, "plants.drape", json!({"ids": [p]}));
    assert_eq!(r["updated"], 1);
    assert!((z(&mut s, p).unwrap() - 150.0).abs() < 1e-6);
    // A plant outside the model takes the height of the nearest edge and is counted.
    let far = plant(&mut s, st, 40000);
    let r = ok(&mut s, "plants.drape", json!({}));
    assert_eq!(
        (r["updated"].clone(), r["outside_model"].clone()),
        (json!(2), json!(1))
    );
    assert!(z(&mut s, far).unwrap() > 150.0);
    ok(
        &mut s,
        "plants.drape",
        json!({"ids": [p, far], "release": true}),
    );
    assert_eq!(z(&mut s, p), None);
    for bad in [
        json!({"ids": ["00000000-0000-0000-0000-00000000270f"]}),
        json!({"ids": ["00000000-0000-0000-0000-000000000001"]}),
        json!({"ids": "x"}),
    ] {
        assert!(s.handle("plants.drape", bad).is_err());
    }
    assert!(
        s.handle(
            "plants.place",
            json!({"mode": "single", "at": [0, 0], "on_terrain": "yes"})
        )
        .is_err()
    );
}

#[test]
fn a_hedgerow_is_a_placement_mode() {
    let mut s = Session::with_sequential_ids();
    let st = style(&mut s, "Beech", "moderate", "Z1");
    let r = ok(
        &mut s,
        "plants.place",
        json!({"mode": "hedgerow", "style": st, "path": [[0, 0], [5000, 0]], "spacing": 1000, "rows": 2, "row_spacing": 600}),
    );
    assert_eq!(r["count"], 11);
    let r = ok(
        &mut s,
        "plants.place",
        json!({"mode": "hedgerow", "style": st, "path": [[0, 3000], [5000, 3000]], "spacing": "spread", "rows": 3, "row_spacing": 500, "stagger": false}),
    );
    assert_eq!(r["count"], 9); // spread 2000: stations at 0, 2000, 4000 in three rows
    for bad in [
        json!({"mode": "hedgerow", "style": st, "path": [[0, 0], [5000, 0]], "spacing": 1000}),
        json!({"mode": "hedgerow", "style": st, "path": [[0, 0]], "spacing": 1000, "rows": 1}),
        json!({"mode": "hedgerow", "style": st, "path": [[0, 0], [5000, 0]], "rows": 1}),
    ] {
        assert!(s.handle("plants.place", bad).is_err());
    }
}
