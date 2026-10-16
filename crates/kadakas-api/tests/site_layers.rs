//! Objects that stand on the ground use the site model that covers their place, on any design
//! layer (SIT rows of the spec). The QA scenario: the site model is on "Site", the plants on
//! "Planting", the trees on "Existing Trees" and a building on the layer of an import.

use kadakas_api::Session;
use kadakas_core::uid::AsUid;
use serde_json::{Value, json};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    match s.handle(m, p) {
        Ok(v) => v,
        Err(e) => panic!("{m} failed: {} {}", e.code, e.message),
    }
}

fn exec(s: &mut Session, command: Value) -> Value {
    call(s, "execute", json!({"command": command}))["created"][0].clone()
}

/// A square site model on the layer `layer`, `size` mm wide, from `at`, rising 10 percent toward
/// +X from `base` mm (so that the ground is high above any layer at 0).
fn site(s: &mut Session, layer: &Value, at: [i64; 2], size: i64, base: i64) -> Value {
    let step = size / 4;
    let mut ids = Vec::new();
    for i in 0..5 {
        for j in 0..5 {
            let (x, y) = (at[0] + i * step, at[1] + j * step);
            ids.push(exec(
                s,
                json!({"op": "add_object", "shape": {"kind": "locus3d", "p": [x, y, base + i * step / 10]}}),
            ));
        }
    }
    call(s, "site.create", json!({"source_ids": ids, "layer": layer}))["id"].clone()
}

fn layer(s: &mut Session, name: &str, elevation: i64) -> Value {
    exec(
        s,
        json!({"op": "add_layer", "name": name, "elevation": elevation}),
    )
}

fn style(s: &mut Session) -> Value {
    call(
        s,
        "plant_style.create",
        json!({"name": "Oak", "spread": 4000, "height": 9000}),
    )["id"]
        .clone()
}

fn ground_z(s: &mut Session, id: &Value) -> Option<f64> {
    call(s, "objects.get", json!({"id": id}))["data"]["ground_z"].as_f64()
}

#[test]
fn plants_trees_and_massing_stand_on_a_site_model_of_another_layer() {
    let mut s = Session::with_sequential_ids();
    let site_layer = layer(&mut s, "Site", 0);
    let planting = layer(&mut s, "Planting", 0);
    let trees = layer(&mut s, "Existing Trees", 0);
    let import = layer(&mut s, "Import", 0);
    // The ground starts 33 m up and rises 10 percent toward +X: z = 33 000 + x / 10.
    let model = site(&mut s, &site_layer, [0, 0], 40_000, 33_000);
    let oak = style(&mut s);

    // Plants on another layer: placed on the terrain, no warning.
    let r = call(
        &mut s,
        "plants.place",
        json!({"mode": "single", "at": [20_000, 20_000], "style": oak, "layer": "Planting", "on_terrain": true}),
    );
    assert!(r.get("warnings").is_none(), "{r}");
    let z = ground_z(&mut s, &r["created"][0]).expect("a stored ground height");
    assert!((z - 35_000.0).abs() < 1e-6, "{z}");

    // plants.drape finds the ground for the plants and for the trees on "Existing Trees".
    let t = call(
        &mut s,
        "tree.import_survey",
        json!({"text": "ID,E,N,Species,DBH (cm),Height (m),Spread (m)\nT1,10,10,Quercus robur,55,16,9\n", "layer": "Existing Trees", "on_terrain": false}),
    );
    assert_eq!(t["imported"], 1);
    let tree = call(&mut s, "objects.list", json!({"layer": "Existing Trees"}))[0]["id"].clone();
    assert_eq!(ground_z(&mut s, &tree), None);
    let d = call(&mut s, "plants.drape", json!({}));
    assert_eq!(d["skipped"], 0, "{d}");
    assert_eq!(d["updated"], 2);
    let z = ground_z(&mut s, &tree).unwrap();
    assert!((z - 34_000.0).abs() < 1e-6, "{z}");

    // A building on the layer of an import stands on the ground too.
    let m = call(
        &mut s,
        "massing.add",
        json!({"footprint": [[10_000, 10_000], [14_000, 10_000], [14_000, 14_000], [10_000, 14_000]], "layer": "Import"}),
    );
    assert!(m["warnings"].as_array().unwrap().is_empty(), "{m}");
    let g = call(&mut s, "massing.get", json!({"id": m["id"]}));
    assert!(
        (g["base_elevation_mm"].as_f64().unwrap() - 34_000.0).abs() < 1e-6,
        "{g}"
    );

    // Moving the site model to another layer does not change the heights.
    call(
        &mut s,
        "execute",
        json!({"command": {"op": "modify_object", "id": model, "layer": import}}),
    );
    let g = call(&mut s, "massing.get", json!({"id": m["id"]}));
    assert!(
        (g["base_elevation_mm"].as_f64().unwrap() - 34_000.0).abs() < 1e-6,
        "{g}"
    );
    let _ = (planting, trees);
}

#[test]
fn a_call_says_so_when_no_site_model_covers_the_place() {
    let mut s = Session::with_sequential_ids();
    let site_layer = layer(&mut s, "Site", 0);
    let oak = style(&mut s);
    // No site model at all.
    let m = call(
        &mut s,
        "massing.add",
        json!({"footprint": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]}),
    );
    assert!(
        m["warnings"][0].as_str().unwrap().contains("no site model"),
        "{m}"
    );
    let r = call(
        &mut s,
        "plants.place",
        json!({"mode": "single", "at": [0, 0], "style": oak, "on_terrain": true}),
    );
    assert!(
        r["warnings"][0].as_str().unwrap().contains("no site model"),
        "{r}"
    );
    let d = call(&mut s, "plants.drape", json!({}));
    assert_eq!(d["skipped"], 1);
    assert!(
        d["warnings"][0].as_str().unwrap().contains("no site model"),
        "{d}"
    );
    // A model that is far away: the plant takes the edge height and the call says it is outside.
    site(&mut s, &site_layer, [100_000, 100_000], 40_000, 5_000);
    let r = call(
        &mut s,
        "plants.place",
        json!({"mode": "single", "at": [0, 0], "style": oak, "on_terrain": true}),
    );
    assert!(
        r["warnings"][0]
            .as_str()
            .unwrap()
            .contains("outside every site model"),
        "{r}"
    );
    let m = call(
        &mut s,
        "massing.add",
        json!({"footprint": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]}),
    );
    assert!(
        m["warnings"][0]
            .as_str()
            .unwrap()
            .contains("no site model covers"),
        "{m}"
    );
}

#[test]
fn the_layer_elevation_counts_like_a_height_above_the_origin() {
    let mut s = Session::with_sequential_ids();
    let low = layer(&mut s, "Site", 2_000);
    let _high = layer(&mut s, "Planting", 5_000);
    site(&mut s, &low, [0, 0], 40_000, 1_000);
    let oak = style(&mut s);
    let r = call(
        &mut s,
        "plants.place",
        json!({"mode": "single", "at": [0, 0], "style": oak, "layer": "Planting", "on_terrain": true}),
    );
    // The model is 1 m above its layer at 2 m: the plant stands 3 m above the origin, and the
    // stored height is the one above the origin.
    let z = ground_z(&mut s, &r["created"][0]).unwrap();
    assert!((z - 3_000.0).abs() < 1e-6, "{z}");
    // A massing model on the layer at 5 m has its base below its layer: -2 m.
    let m = call(
        &mut s,
        "massing.add",
        json!({"footprint": [[0, 0], [2000, 0], [2000, 2000], [0, 2000]], "layer": "Planting"}),
    );
    let g = call(&mut s, "massing.get", json!({"id": m["id"]}));
    assert!(
        (g["base_elevation_mm"].as_f64().unwrap() - 3_000.0).abs() < 1e-3,
        "{g}"
    );
}

#[test]
fn an_object_or_a_layer_can_name_its_site_model() {
    let mut s = Session::with_sequential_ids();
    let a = layer(&mut s, "Site A", 0);
    let b = layer(&mut s, "Site B", 0);
    let planting = layer(&mut s, "Planting", 0);
    // Two models over the same ground, 10 m and 50 m high.
    let _low = site(&mut s, &a, [0, 0], 40_000, 10_000);
    let high = site(&mut s, &b, [0, 0], 40_000, 50_000);
    let oak = style(&mut s);
    let place = |s: &mut Session| {
        let r = call(
            s,
            "plants.place",
            json!({"mode": "single", "at": [0, 0], "style": oak, "layer": "Planting", "on_terrain": true}),
        );
        r["created"][0].clone()
    };
    // Without a name: the first model that covers the place.
    let p1 = place(&mut s);
    assert!((ground_z(&mut s, &p1).unwrap() - 10_000.0).abs() < 1e-6);
    // The layer names the high model.
    call(
        &mut s,
        "execute",
        json!({"command": {"op": "set_layer_site", "id": planting, "site": high}}),
    );
    let p2 = place(&mut s);
    assert!((ground_z(&mut s, &p2).unwrap() - 50_000.0).abs() < 1e-6);
    // The object names the first model, and its name wins over the layer.
    let low_id = call(&mut s, "objects.list", json!({"layer": "Site A"}))[0]["id"].clone();
    call(
        &mut s,
        "execute",
        json!({"command": {"op": "set_data", "id": p2, "set": {"site_model": low_id}}}),
    );
    call(&mut s, "plants.drape", json!({"ids": [p2]}));
    assert!((ground_z(&mut s, &p2).unwrap() - 10_000.0).abs() < 1e-6);
    // Only a site model can be named.
    let e = s
        .handle(
            "execute",
            json!({"command": {"op": "set_layer_site", "id": planting, "site": p2}}),
        )
        .unwrap_err();
    assert!(e.message.contains("not a site model"), "{}", e.message);
    let _ = oak.as_uid();
}

#[test]
fn issue_pins_stand_on_the_ground() {
    let mut s = Session::with_sequential_ids();
    let site_layer = layer(&mut s, "Site", 0);
    site(&mut s, &site_layer, [0, 0], 40_000, 33_000);
    let w = call(
        &mut s,
        "wall.add",
        json!({"path": [[10_000, 10_000], [14_000, 10_000]], "top": {"mode": "terrain", "offset": 900}}),
    );
    let i = call(
        &mut s,
        "issue.create",
        json!({"title": "Crack", "objects": [w["id"]]}),
    );
    let pin = call(&mut s, "issue.get", json!({"id": i["id"]}))["pin"].clone();
    let z = pin[2].as_f64().unwrap();
    assert!((z - 34_200.0).abs() < 1.0, "{pin}");
}

#[test]
fn hiding_the_layer_of_the_site_model_does_not_drop_the_ground() {
    let mut s = Session::with_sequential_ids();
    let site_layer = layer(&mut s, "Site", 0);
    let _planting = layer(&mut s, "Planting", 0);
    site(&mut s, &site_layer, [0, 0], 40_000, 33_000);
    let oak = style(&mut s);
    call(
        &mut s,
        "execute",
        json!({"command": {"op": "set_visibility", "layers": [site_layer], "mode": "invisible"}}),
    );
    let r = call(
        &mut s,
        "plants.place",
        json!({"mode": "single", "at": [20_000, 20_000], "style": oak, "layer": "Planting", "on_terrain": true}),
    );
    assert!(r.get("warnings").is_none(), "{r}");
    let z = ground_z(&mut s, &r["created"][0]).unwrap();
    assert!((z - 35_000.0).abs() < 1e-6, "{z}");
    let m = call(
        &mut s,
        "massing.add",
        json!({"footprint": [[10_000, 10_000], [14_000, 10_000], [14_000, 14_000], [10_000, 14_000]], "layer": "Planting"}),
    );
    assert!(m["warnings"].as_array().unwrap().is_empty(), "{m}");
}

#[test]
fn deleting_a_site_model_clears_the_names_and_undo_brings_them_back() {
    let mut s = Session::with_sequential_ids();
    let a = layer(&mut s, "Site A", 0);
    let b = layer(&mut s, "Site B", 0);
    let planting = layer(&mut s, "Planting", 0);
    let _low = site(&mut s, &a, [0, 0], 40_000, 10_000);
    let high = site(&mut s, &b, [0, 0], 40_000, 50_000);
    let oak = style(&mut s);
    call(
        &mut s,
        "execute",
        json!({"command": {"op": "set_layer_site", "id": planting, "site": high}}),
    );
    let p = call(
        &mut s,
        "plants.place",
        json!({"mode": "single", "at": [0, 0], "style": oak, "layer": "Planting", "on_terrain": true}),
    )["created"][0]
        .clone();
    call(
        &mut s,
        "execute",
        json!({"command": {"op": "set_data", "id": p, "set": {"site_model": high}}}),
    );
    call(
        &mut s,
        "execute",
        json!({"command": {"op": "delete_objects", "ids": [high]}}),
    );
    let layers = call(&mut s, "document.get", json!({}))["layers"].clone();
    assert!(!layers.to_string().contains("site_model"), "{layers}");
    let o = call(&mut s, "objects.get", json!({"id": p}));
    assert!(o["data"].get("site_model").is_none(), "{o}");
    // The plant now stands on the model that covers it.
    let d = call(&mut s, "plants.drape", json!({}));
    assert!(d.get("warnings").is_none(), "{d}");
    assert!((ground_z(&mut s, &p).unwrap() - 10_000.0).abs() < 1e-6);
    // One undo for the drape, one for the delete: the names come back with the model.
    assert!(s.editor.undo() && s.editor.undo());
    let o = call(&mut s, "objects.get", json!({"id": p}));
    assert_eq!(o["data"]["site_model"], high);
    let layers = call(&mut s, "document.get", json!({}))["layers"].clone();
    assert!(layers.to_string().contains("site_model"));
}

#[test]
fn a_name_that_points_at_nothing_falls_back_with_a_warning() {
    let mut s = Session::with_sequential_ids();
    let a = layer(&mut s, "Site", 0);
    let _p = layer(&mut s, "Planting", 0);
    site(&mut s, &a, [0, 0], 40_000, 10_000);
    let oak = style(&mut s);
    let p = call(
        &mut s,
        "plants.place",
        json!({"mode": "single", "at": [0, 0], "style": oak, "layer": "Planting"}),
    )["created"][0]
        .clone();
    call(
        &mut s,
        "execute",
        json!({"command": {"op": "set_data", "id": p, "set": {"site_model": "00000000-0000-0000-0000-0000000003e7"}}}),
    );
    let d = call(&mut s, "plants.drape", json!({}));
    assert!((ground_z(&mut s, &p).unwrap() - 10_000.0).abs() < 1e-6);
    assert!(
        d["warnings"][0]
            .as_str()
            .unwrap()
            .contains("name a site model that does not exist"),
        "{d}"
    );
}

#[test]
fn the_choice_between_models_follows_the_layer_order_and_not_the_stacking_order() {
    let mut s = Session::with_sequential_ids();
    let first = layer(&mut s, "First", 0);
    let second = layer(&mut s, "Second", 0);
    let _third = layer(&mut s, "Third", 0);
    // The model of the second layer is made first, so it is first in the stacking order.
    let _b = site(&mut s, &second, [0, 0], 40_000, 20_000);
    let _a = site(&mut s, &first, [0, 0], 40_000, 10_000);
    let oak = style(&mut s);
    let r = call(
        &mut s,
        "plants.place",
        json!({"mode": "single", "at": [0, 0], "style": oak, "layer": "Third", "on_terrain": true}),
    );
    assert!((ground_z(&mut s, &r["created"][0]).unwrap() - 10_000.0).abs() < 1e-6);
}

#[test]
fn a_wall_report_equals_the_wall_in_3d_when_layer_elevations_differ() {
    let mut s = Session::with_sequential_ids();
    let site_layer = layer(&mut s, "Site", 2_000);
    let walls = layer(&mut s, "Walls", 5_000);
    site(&mut s, &site_layer, [0, 0], 40_000, 1_000);
    let w = call(
        &mut s,
        "wall.add",
        json!({"path": [[10_000, 10_000], [14_000, 10_000]], "top": {"mode": "terrain", "offset": 900}, "layer": walls}),
    );
    let row = call(&mut s, "wall.report", json!({"ids": [w["id"]]}))["rows"][0].clone();
    let id = kadakas_core::model::ObjectId(w["id"].as_uid().unwrap());
    let doc = s.editor.doc();
    let wall = kadakas_site::grading::hook::wall_of(
        doc,
        s.site_cache(),
        s.editor.revision(),
        doc.object(id).unwrap(),
    )
    .unwrap();
    let first = wall.stations.first().unwrap();
    assert_eq!(row["top_start_mm"].as_f64().unwrap(), first.top);
    assert_eq!(row["bottom_start_mm"].as_f64().unwrap(), first.bottom);
}

#[test]
fn a_pad_on_a_layer_without_a_site_model_acts_on_the_model_under_it() {
    let mut s = Session::with_sequential_ids();
    let site_layer = layer(&mut s, "Site", 0);
    let _other = layer(&mut s, "Grading", 0);
    let model = site(&mut s, &site_layer, [0, 0], 40_000, 1_000);
    let pad = call(
        &mut s,
        "pad.add",
        json!({"boundary": [[5000, 5000], [15_000, 5000], [15_000, 15_000], [5000, 15_000]], "elevation": 9000, "layer": "Grading"}),
    );
    let cf = call(&mut s, "site.cut_fill", json!({"id": model}));
    assert!(
        cf["modifiers"].as_array().unwrap().contains(&pad["id"]),
        "{cf}"
    );
    assert!(cf["fill_m3"].as_f64().unwrap() > 0.0, "{cf}");
    // A layer that has a model of its own keeps its pads to itself.
    let own = layer(&mut s, "Own site", 0);
    let _m2 = site(&mut s, &own, [200_000, 0], 40_000, 1_000);
    let pad2 = call(
        &mut s,
        "pad.add",
        json!({"boundary": [[5000, 5000], [15_000, 5000], [15_000, 15_000], [5000, 15_000]], "elevation": 5000, "layer": "Own site"}),
    );
    let cf1 = call(&mut s, "site.cut_fill", json!({"id": model}));
    assert!(!cf1["modifiers"].as_array().unwrap().contains(&pad2["id"]));
}
