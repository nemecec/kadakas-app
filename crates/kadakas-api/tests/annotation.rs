//! Annotation methods: limits, speed, and the north arrow style.

use std::time::Instant;

use kadakas_api::Session;
use serde_json::{Value, json};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    match s.handle(m, p) {
        Ok(v) => v,
        Err(e) => panic!("{m} failed: {}", e.message),
    }
}

fn exec(s: &mut Session, cmd: Value) -> Value {
    call(s, "execute", json!({"command": cmd}))
}

fn plants(s: &mut Session, n: usize) {
    let style = call(
        s,
        "plant_style.create",
        json!({"name": "Oak", "id_code": "QR", "spread": 800, "height": 3000}),
    )["id"]
        .clone();
    let cmds: Vec<Value> = (0..n)
        .map(|i| json!({"op": "add_plants", "style": style, "positions": [[(i % 100) as f64 * 1000.0, (i / 100) as f64 * 1000.0]]}))
        .collect();
    exec(s, json!({"op": "batch", "commands": cmds}));
}

#[test]
fn twenty_legends_over_4000_plants_add_little_to_an_svg_export() {
    let mut s = Session::with_sequential_ids();
    plants(&mut s, 4000);
    let t = Instant::now();
    let base = call(&mut s, "render.svg", json!({}))["svg"]
        .as_str()
        .unwrap()
        .len();
    let plain = t.elapsed();
    for i in 0..20 {
        call(
            &mut s,
            "legend.add",
            json!({"at": [i * 10, 0], "criteria": "Kind = \"plant\"", "group_by": "Style"}),
        );
    }
    let t = Instant::now();
    let with = call(&mut s, "render.svg", json!({}))["svg"]
        .as_str()
        .unwrap()
        .len();
    let extra = t.elapsed().saturating_sub(plain);
    assert!(with > base);
    assert!(extra.as_millis() < 100, "20 legends added {extra:?}");
    // A second export of the same revision is served from the cache.
    let t = Instant::now();
    call(&mut s, "render.svg", json!({}));
    assert!(t.elapsed().as_millis() < 100 + plain.as_millis());
}

#[test]
fn tag_all_is_not_quadratic() {
    let mut s = Session::with_sequential_ids();
    plants(&mut s, 3000);
    call(
        &mut s,
        "tag_style.create",
        json!({"name": "T", "lines": ["{IdCode}"]}),
    );
    let t = Instant::now();
    let r = call(
        &mut s,
        "data_tag.tag_all",
        json!({"style": "T", "criteria": "Kind = \"plant\""}),
    );
    assert_eq!(r["tagged"], 3000);
    assert!(t.elapsed().as_secs() < 5, "tag_all took {:?}", t.elapsed());
}

#[test]
fn a_legend_shows_at_most_500_rows() {
    let mut s = Session::with_sequential_ids();
    let cmds: Vec<Value> = (0..600)
        .map(|i| json!({"op": "add_object", "shape": {"kind": "plant", "at": [i, 0], "species": format!("Species {i:04}"), "spread": 100, "height": 100}}))
        .collect();
    exec(&mut s, json!({"op": "batch", "commands": cmds}));
    let r = call(
        &mut s,
        "legend.add",
        json!({"at": [0, 0], "criteria": "Kind = \"plant\"", "group_by": "Species"}),
    );
    let rows = r["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 501);
    assert_eq!(rows[500], json!({"label": "... 100 more", "count": 100}));
}

#[test]
fn north_arrow_style_is_the_look_and_the_georeference_turns_it() {
    let mut s = Session::with_sequential_ids();
    let r = call(
        &mut s,
        "north_arrow.add",
        json!({"at": [0, 0], "arrow_style": "compass"}),
    );
    let listed = call(&mut s, "annotation.evaluate", json!({"id": r["id"]}));
    assert_eq!(listed["annotation"]["style"], "compass");
    assert!(
        s.handle(
            "north_arrow.add",
            json!({"at": [0, 0], "style": "00000000-0000-0000-0000-000000000005"})
        )
        .is_err()
    );
    exec(
        &mut s,
        json!({"op": "set_georeference", "georef": {"crs": "", "origin": [0, 0], "rotation": 0.5235987755982988, "user_origin": [0, 0]}}),
    );
    let r = call(
        &mut s,
        "north_arrow.add",
        json!({"at": [0, 0], "use_georef": true}),
    );
    assert!((r["rotation_deg"].as_f64().unwrap() + 30.0).abs() < 1e-9);
}

#[test]
fn detached_dimensions_say_so() {
    let mut s = Session::with_sequential_ids();
    let pl = exec(&mut s, json!({"op": "add_object", "shape": {"kind": "polyline", "closed": false, "points": [[0, 0], [1000, 0], [2000, 0]]}}))["created"][0].clone();
    let d = call(
        &mut s,
        "dimension.add",
        json!({"mode": "horizontal", "anchors": [{"object": pl, "vertex": 0}, {"object": pl, "vertex": 2}]}),
    );
    assert_eq!(d["detached"], false);
    exec(
        &mut s,
        json!({"op": "modify_object", "id": pl, "shape": {"kind": "polyline", "closed": false, "points": [[0, 0], [1000, 0]]}}),
    );
    let e = call(&mut s, "annotation.evaluate", json!({"id": d["id"]}));
    assert_eq!(e["computed"]["detached"], true);
    assert_eq!(e["computed"]["texts"], json!(["2000"]));
}

#[test]
fn annotations_made_in_an_entered_group_become_members() {
    let mut s = Session::with_sequential_ids();
    let a = exec(
        &mut s,
        json!({"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 100}}),
    )["created"][0]
        .clone();
    let g = call(&mut s, "group.create", json!({"ids": [a]}));
    let gid = g["id"].clone();
    call(&mut s, "group.enter", json!({"id": gid}));
    let t = call(
        &mut s,
        "annotation.text",
        json!({"at": [0, 0], "text": "inside"}),
    );
    let members = call(&mut s, "group.members", json!({"id": gid}));
    assert!(
        members.to_string().contains(&t["id"].to_string()),
        "{members}"
    );
}
