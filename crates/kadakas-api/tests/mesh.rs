//! The agent API of surface models (MOD-11): reads, edits, undo steps, batches, refusals,
//! the render and the marking of agent changes.

use kadakas_api::{METHODS, RpcError, Session};
use kadakas_core::change::Origin;
use serde_json::{Value, json};

fn call(s: &mut Session, method: &str, params: Value) -> Result<Value, RpcError> {
    s.handle(method, params)
}

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    call(s, method, params).unwrap_or_else(|e| panic!("{method}: {} {:?}", e.message, e.details))
}

fn err(s: &mut Session, method: &str, params: Value) -> RpcError {
    match call(s, method, params) {
        Ok(v) => panic!("{method} should fail but gave {v}"),
        Err(e) => e,
    }
}

/// The stable id of a refusal.
fn error_id(e: &RpcError) -> String {
    e.details.as_ref().unwrap()["error_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn new_model(s: &mut Session) -> String {
    ok(s, "mesh.create", json!({"name": "Test"}))["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// The 12 edges of a box.
fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> Value {
    let c = |i: usize| -> [f64; 3] {
        std::array::from_fn(|k| if i >> k & 1 == 0 { lo[k] } else { hi[k] })
    };
    let mut edges = Vec::new();
    for i in 0..8usize {
        for bit in [1usize, 2, 4] {
            if i & bit == 0 {
                edges.push(json!([c(i), c(i | bit)]));
            }
        }
    }
    json!(edges)
}

/// A rectangle on the ground as four edges.
fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!([
        [[x0, y0, 0], [x1, y0, 0]],
        [[x1, y0, 0], [x1, y1, 0]],
        [[x1, y1, 0], [x0, y1, 0]],
        [[x0, y1, 0], [x0, y0, 0]]
    ])
}

/// Draws boxes and turns the ones that look inward over.
fn draw_boxes(s: &mut Session, id: &str, boxes: &[([f64; 3], [f64; 3])]) {
    for (lo, hi) in boxes {
        ok(
            s,
            "mesh.add_edges",
            json!({"id": id, "edges": cuboid(*lo, *hi)}),
        );
    }
    let report = ok(s, "mesh.solid_report", json!({"id": id}));
    let inward: Vec<Value> = report["shells"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|sh| sh["state"] == "inverted")
        .flat_map(|sh| sh["faces"].as_array().unwrap().clone())
        .collect();
    if !inward.is_empty() {
        ok(s, "mesh.reverse_faces", json!({"id": id, "faces": inward}));
    }
}

fn all(s: &mut Session, method: &str, id: &str) -> Vec<Value> {
    ok(s, method, json!({"id": id, "limit": 1000}))["items"]
        .as_array()
        .unwrap()
        .clone()
}

fn ids(items: &[Value]) -> Vec<u64> {
    items.iter().map(|i| i["id"].as_u64().unwrap()).collect()
}

/// The whole geometry of a model, to compare before and after.
fn snapshot(s: &mut Session, id: &str) -> Value {
    json!({
        "v": all(s, "mesh.vertices", id),
        "e": all(s, "mesh.edges", id),
        "f": all(s, "mesh.faces", id),
    })
}

fn vertex_at(s: &mut Session, id: &str, p: [f64; 3]) -> u64 {
    all(s, "mesh.vertices", id)
        .iter()
        .find(|v| {
            let q: Vec<f64> = v["p"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap())
                .collect();
            (0..3).all(|k| (q[k] - p[k]).abs() < 1e-6)
        })
        .unwrap_or_else(|| panic!("no vertex at {p:?}"))["id"]
        .as_u64()
        .unwrap()
}

fn edge_between(s: &mut Session, id: &str, a: [f64; 3], b: [f64; 3]) -> u64 {
    let (va, vb) = (vertex_at(s, id, a), vertex_at(s, id, b));
    all(s, "mesh.edges", id)
        .iter()
        .find(|e| {
            let v = e["v"].as_array().unwrap();
            let v: Vec<u64> = v.iter().map(|x| x.as_u64().unwrap()).collect();
            (v[0] == va && v[1] == vb) || (v[0] == vb && v[1] == va)
        })
        .unwrap()["id"]
        .as_u64()
        .unwrap()
}

/// The id of the face with this normal and the highest position along it.
fn face_facing(s: &mut Session, id: &str, normal: [f64; 3]) -> u64 {
    let items = ok(
        s,
        "mesh.faces",
        json!({"id": id, "limit": 1000, "with_points": true}),
    );
    let mut best: Option<(f64, u64)> = None;
    for f in items["items"].as_array().unwrap() {
        let n: Vec<f64> = f["normal"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .collect();
        if (0..3).all(|k| (n[k] - normal[k]).abs() < 1e-9) {
            let p: Vec<f64> = f["points"]["outer"][0]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap())
                .collect();
            let h = (0..3).map(|k| p[k] * normal[k]).sum::<f64>();
            if best.is_none_or(|(b, _)| h > b) {
                best = Some((h, f["id"].as_u64().unwrap()));
            }
        }
    }
    best.unwrap_or_else(|| panic!("no face facing {normal:?}"))
        .1
}

fn depth(s: &Session) -> usize {
    s.editor.undo_depth()
}

#[test]
fn a_model_is_made_drawn_pulled_and_checked() {
    let mut s = Session::with_sequential_ids();
    let id = new_model(&mut s);
    let r = ok(
        &mut s,
        "mesh.add_edges",
        json!({"id": id, "edges": rect(0.0, 0.0, 4000.0, 3000.0)}),
    );
    assert_eq!(r["op"], "add_edges");
    assert_eq!(r["counts"], json!({"vertices": 4, "edges": 4, "faces": 1}));
    let face = r["created"]["faces"][0].as_u64().unwrap();
    assert_eq!(r["created"]["vertices"].as_array().unwrap().len(), 4);
    assert_eq!(r["removed"]["faces"], json!([]));
    let r = ok(
        &mut s,
        "mesh.push_pull",
        json!({"id": id, "face": face, "distance": 2500}),
    );
    assert_eq!(r["counts"], json!({"vertices": 8, "edges": 12, "faces": 6}));
    let report = ok(&mut s, "mesh.solid_report", json!({"id": id}));
    assert_eq!(report["closed"], true);
    assert_eq!(report["shells"][0]["state"], "solid");
    assert!(
        (report["shells"][0]["volume"].as_f64().unwrap() - 4000.0 * 3000.0 * 2500.0).abs() < 1.0
    );
    let b = ok(&mut s, "mesh.bounds", json!({"id": id}));
    assert_eq!(b["empty"], false);
    assert_eq!(b["local"]["max"], json!([4000.0, 3000.0, 2500.0]));
    assert_eq!(b["world"]["size"], json!([4000.0, 3000.0, 2500.0]));
}

#[test]
fn create_takes_a_name_an_origin_a_layer_and_a_class() {
    let mut s = Session::with_sequential_ids();
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_layer", "name": "Model"}}),
    );
    ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_class", "name": "Walls"}}),
    );
    let id = ok(
        &mut s,
        "mesh.create",
        json!({"name": "Shed", "origin": [1000, 2000, 0], "layer": "Model", "class": "Walls"}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    ok(
        &mut s,
        "mesh.add_edges",
        json!({"id": id, "edges": rect(0.0, 0.0, 100.0, 100.0)}),
    );
    let sum = ok(&mut s, "mesh.summary", json!({"id": id}));
    let m = &sum["models"][0];
    assert_eq!(m["name"], "Shed");
    assert_eq!(m["bounds"]["world"]["min"], json!([1000.0, 2000.0, 0.0]));
    assert_eq!(m["bounds"]["local"]["min"], json!([0.0, 0.0, 0.0]));
    assert_eq!(m["faces"], 1);
    assert_eq!(m["problem"], Value::Null);
    let o = ok(&mut s, "objects.get", json!({"id": id}));
    assert_eq!(o["shape"]["kind"], "surface_model");
    let e = err(&mut s, "mesh.create", json!({"layer": "Nope"}));
    assert_eq!(e.code, -32602);
}

#[test]
fn the_summary_lists_all_models_and_names_the_one_asked_for() {
    let mut s = Session::with_sequential_ids();
    assert_eq!(ok(&mut s, "mesh.summary", json!({}))["models"], json!([]));
    let a = new_model(&mut s);
    let b = new_model(&mut s);
    draw_boxes(&mut s, &b, &[([0.0; 3], [100.0; 3])]);
    let all_models = ok(&mut s, "mesh.summary", json!({}));
    assert_eq!(all_models["models"].as_array().unwrap().len(), 2);
    let one = ok(&mut s, "mesh.summary", json!({"id": b}));
    assert_eq!(one["models"][0]["faces"], 6);
    assert_eq!(one["models"][0]["curves"], 0);
    let empty = ok(&mut s, "mesh.bounds", json!({"id": a}));
    assert_eq!(empty["empty"], true);
    assert_eq!(empty["local"], Value::Null);
    // A rectangle is not a surface model.
    let rect_id = ok(
        &mut s,
        "execute",
        json!({"command": {"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": 10}}}),
    )["created"][0]
        .as_str()
        .unwrap()
        .to_string();
    let e = err(&mut s, "mesh.summary", json!({"id": rect_id}));
    assert_eq!(error_id(&e), "not_a_surface_model");
    assert_eq!(e.details.as_ref().unwrap()["kind"], "circle");
    let e = err(
        &mut s,
        "mesh.summary",
        json!({"id": "00000000-0000-0000-0000-0000000000ff"}),
    );
    assert_eq!(e.code, 2);
    assert_eq!(error_id(&e), "no_such_object");
    let e = err(&mut s, "mesh.faces", json!({"id": "not an id"}));
    assert_eq!(e.code, -32602);
}

#[test]
fn vertices_edges_and_faces_come_in_pages_and_by_id() {
    let mut s = Session::with_sequential_ids();
    let id = new_model(&mut s);
    draw_boxes(&mut s, &id, &[([0.0; 3], [1000.0; 3])]);
    let page = ok(&mut s, "mesh.vertices", json!({"id": id, "limit": 3}));
    assert_eq!(page["total"], 8);
    assert_eq!(page["items"].as_array().unwrap().len(), 3);
    let next = ok(
        &mut s,
        "mesh.vertices",
        json!({"id": id, "offset": 3, "limit": 3}),
    );
    assert_eq!(next["offset"], 3);
    assert_ne!(page["items"][0]["id"], next["items"][0]["id"]);
    let last = ok(&mut s, "mesh.vertices", json!({"id": id, "offset": 6}));
    assert_eq!(last["items"].as_array().unwrap().len(), 2);
    let some = ok(
        &mut s,
        "mesh.vertices",
        json!({"id": id, "ids": [1, 2, 999]}),
    );
    assert_eq!(some["total"], 2);
    assert_eq!(some["missing"], json!([999]));
    let edges = all(&mut s, "mesh.edges", &id);
    assert_eq!(edges.len(), 12);
    for e in &edges {
        assert_eq!(e["faces"].as_array().unwrap().len(), 2);
        assert_eq!(e["soft"], false);
        assert_eq!(e["curve"], Value::Null);
        assert!((e["length_mm"].as_f64().unwrap() - 1000.0).abs() < 1e-6);
    }
    let f = ok(
        &mut s,
        "mesh.faces",
        json!({"id": id, "limit": 2, "with_points": true}),
    );
    assert_eq!(f["total"], 6);
    let first = &f["items"][0];
    assert_eq!(first["holes"], json!([]));
    assert!((first["area_mm2"].as_f64().unwrap() - 1.0e6).abs() < 1e-3);
    assert_eq!(first["points"]["outer"].as_array().unwrap().len(), 4);
    assert_eq!(first["hidden"], false);
    assert_eq!(first["material"], Value::Null);
    for bad_limit in [0, 1001] {
        let e = err(&mut s, "mesh.edges", json!({"id": id, "limit": bad_limit}));
        assert_eq!(e.code, -32602);
    }
}

#[test]
fn near_and_pick_find_faces_edges_and_vertices() {
    let mut s = Session::with_sequential_ids();
    let id = new_model(&mut s);
    draw_boxes(&mut s, &id, &[([0.0; 3], [1000.0; 3])]);
    let top = face_facing(&mut s, &id, [0.0, 0.0, 1.0]);
    let n = ok(
        &mut s,
        "mesh.near",
        json!({"point": [500, 500, 1100], "radius": 200, "kinds": ["faces", "edges", "vertices"]}),
    );
    assert_eq!(n["faces"][0]["id"], top);
    assert!((n["faces"][0]["distance"].as_f64().unwrap() - 100.0).abs() < 1e-6);
    assert_eq!(n["faces"][0]["closest"], json!([500.0, 500.0, 1000.0]));
    assert_eq!(n["edges"], json!([]));
    assert_eq!(n["vertices"], json!([]));
    let n = ok(
        &mut s,
        "mesh.near",
        json!({"id": id, "point": [-10, -10, -10], "radius": 100, "kinds": ["vertices", "edges", "faces"], "limit": 2}),
    );
    assert_eq!(n["vertices"].as_array().unwrap().len(), 1);
    assert_eq!(n["edges"].as_array().unwrap().len(), 2);
    assert_eq!(n["faces"].as_array().unwrap().len(), 2);
    assert!(
        n["edges"][0]["distance"].as_f64().unwrap() <= n["edges"][1]["distance"].as_f64().unwrap()
    );
    // Far away: nothing.
    let n = ok(
        &mut s,
        "mesh.near",
        json!({"point": [9000, 0, 0], "radius": 10}),
    );
    assert_eq!(n["faces"], json!([]));
    for bad in [
        json!({"point": [0, 0, 0], "radius": -1}),
        json!({"point": [0, 0, 0], "radius": 1, "limit": 0}),
    ] {
        assert_eq!(err(&mut s, "mesh.near", bad).code, -32602);
    }
    // A ray from above hits the top face from the front, then the bottom face from the back.
    let hit = ok(
        &mut s,
        "mesh.pick",
        json!({"origin": [500, 500, 5000], "direction": [0, 0, -1]}),
    );
    assert_eq!(hit["hit"]["face"], top);
    assert_eq!(hit["hit"]["side"], "front");
    assert!((hit["hit"]["distance"].as_f64().unwrap() - 4000.0).abs() < 1e-6);
    assert_eq!(hit["hit"]["point"], json!([500.0, 500.0, 1000.0]));
    assert_eq!(hit["hit"]["normal"], json!([0.0, 0.0, 1.0]));
    let both = ok(
        &mut s,
        "mesh.pick",
        json!({"origin": [500, 500, 5000], "direction": [0, 0, -2], "all": true}),
    );
    assert_eq!(both["hits"].as_array().unwrap().len(), 2);
    assert_eq!(both["hits"][1]["side"], "back");
    let miss = ok(
        &mut s,
        "mesh.pick",
        json!({"origin": [5000, 5000, 5000], "direction": [0, 0, -1]}),
    );
    assert_eq!(miss["hit"], Value::Null);
    let e = err(
        &mut s,
        "mesh.pick",
        json!({"origin": [0, 0, 0], "direction": [0, 0, 0]}),
    );
    assert_eq!(e.code, -32602);
    // A hidden face is not hit unless asked for.
    ok(&mut s, "mesh.hide", json!({"id": id, "faces": [top]}));
    let hit = ok(
        &mut s,
        "mesh.pick",
        json!({"origin": [500, 500, 5000], "direction": [0, 0, -1]}),
    );
    assert_ne!(hit["hit"]["face"], top);
    let hit = ok(
        &mut s,
        "mesh.pick",
        json!({"origin": [500, 500, 5000], "direction": [0, 0, -1], "include_hidden": true}),
    );
    assert_eq!(hit["hit"]["face"], top);
    let n = ok(
        &mut s,
        "mesh.near",
        json!({"point": [500, 500, 1100], "radius": 200}),
    );
    assert_ne!(n["faces"][0]["id"], top);
}

#[test]
fn a_transformed_model_is_picked_in_drawing_coordinates() {
    let mut s = Session::with_sequential_ids();
    let id = ok(&mut s, "mesh.create", json!({"origin": [10000, 0, 0]}))["id"]
        .as_str()
        .unwrap()
        .to_string();
    draw_boxes(&mut s, &id, &[([0.0; 3], [1000.0; 3])]);
    let hit = ok(
        &mut s,
        "mesh.pick",
        json!({"origin": [10500, 500, 5000], "direction": [0, 0, -1]}),
    );
    assert_eq!(hit["hit"]["point"], json!([10500.0, 500.0, 1000.0]));
    assert_eq!(hit["hit"]["local"], json!([500.0, 500.0, 1000.0]));
    let miss = ok(
        &mut s,
        "mesh.pick",
        json!({"origin": [500, 500, 5000], "direction": [0, 0, -1]}),
    );
    assert_eq!(miss["hit"], Value::Null);
}

type Case = Box<dyn Fn(&Scene) -> Value>;

/// Each edit method, with the scenario it needs: a box of 1000 mm, plus a few loose things.
struct Scene {
    id: String,
    top: u64,
    bottom: u64,
    edge: u64,
    material: String,
}

fn scene(s: &mut Session) -> Scene {
    scene_of(s, false)
}

fn scene_of(s: &mut Session, crossing: bool) -> Scene {
    let id = new_model(s);
    draw_boxes(s, &id, &[([0.0; 3], [1000.0; 3])]);
    if crossing {
        draw_boxes(s, &id, &[([500.0; 3], [1500.0; 3])]);
    }
    let top = face_facing(s, &id, [0.0, 0.0, 1.0]);
    let bottom = face_facing(s, &id, [0.0, 0.0, -1.0]);
    let edge = edge_between(s, &id, [0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]);
    let material = ok(
        s,
        "material.create",
        json!({"name": "Brick", "base_color": [180, 80, 60]}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    Scene {
        id,
        top,
        bottom,
        edge,
        material,
    }
}

#[test]
fn every_edit_method_is_one_undo_step_and_undo_brings_the_model_back() {
    let cases: Vec<(&str, Case)> = vec![
        (
            "mesh.add_edges",
            Box::new(|c| json!({"id": c.id, "edges": [[[2000, 0, 0], [3000, 0, 0]]]})),
        ),
        (
            "mesh.add_curve",
            Box::new(|c| {
                let pts: Vec<Value> = (0..12)
                    .map(|i| {
                        let a = std::f64::consts::TAU * f64::from(i) / 12.0;
                        json!([4000.0 + 200.0 * a.cos(), 200.0 * a.sin(), 0.0])
                    })
                    .collect();
                json!({"id": c.id, "points": pts, "closed": true})
            }),
        ),
        (
            "mesh.erase",
            Box::new(|c| json!({"id": c.id, "edges": [c.edge], "faces": [c.bottom]})),
        ),
        (
            "mesh.reverse_faces",
            Box::new(|c| json!({"id": c.id, "faces": [c.top]})),
        ),
        (
            "mesh.push_pull",
            Box::new(|c| json!({"id": c.id, "face": c.top, "distance": 300})),
        ),
        (
            "mesh.offset",
            Box::new(|c| json!({"id": c.id, "face": c.top, "distance": 100})),
        ),
        (
            "mesh.offset",
            Box::new(
                |c| json!({"id": c.id, "edges": [c.edge], "distance": 100, "toward": [500, -500, 0]}),
            ),
        ),
        ("mesh.intersect_faces", Box::new(|c| json!({"id": c.id}))),
        (
            "mesh.transform",
            Box::new(|c| json!({"id": c.id, "faces": [c.top], "translate": [0, 0, 200]})),
        ),
        (
            "mesh.transform",
            Box::new(
                |c| json!({"id": c.id, "faces": [c.top], "rotate": {"axis": [0, 0, 1], "degrees": 10, "about": [500, 500, 1000]}}),
            ),
        ),
        (
            "mesh.transform",
            Box::new(
                |c| json!({"id": c.id, "faces": [c.top], "scale": {"factor": 0.5, "about": [500, 500, 1000]}}),
            ),
        ),
        (
            "mesh.transform",
            Box::new(
                |c| json!({"id": c.id, "faces": [c.top], "matrix": [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 100]}),
            ),
        ),
        (
            "mesh.copy",
            Box::new(
                |c| json!({"id": c.id, "faces": [c.top], "translate": [3000, 0, 0], "count": 2}),
            ),
        ),
        (
            "mesh.paint",
            Box::new(
                |c| json!({"id": c.id, "faces": [c.top], "material": c.material, "side": "both"}),
            ),
        ),
        (
            "mesh.paint",
            Box::new(|c| json!({"id": c.id, "faces": [c.top], "material": "brick"})),
        ),
        (
            "mesh.hide",
            Box::new(|c| json!({"id": c.id, "faces": [c.top], "edges": [c.edge]})),
        ),
        (
            "mesh.soften",
            Box::new(|c| json!({"id": c.id, "edges": [c.edge], "soft": true, "smooth": true})),
        ),
        (
            "mesh.soften_edges",
            Box::new(|c| json!({"id": c.id, "below_angle": 100})),
        ),
        (
            "mesh.make_group",
            Box::new(|c| json!({"id": c.id, "faces": [c.top], "name": "Lid"})),
        ),
        (
            "mesh.make_component",
            Box::new(|c| json!({"id": c.id, "faces": [c.top], "name": "Lid"})),
        ),
    ];
    for (method, params) in cases {
        let mut s = Session::with_sequential_ids();
        let c = scene_of(&mut s, method == "mesh.intersect_faces");
        let before = snapshot(&mut s, &c.id);
        let objects_before = ok(&mut s, "document.summary", json!({}))["objects_total"].clone();
        let d = depth(&s);
        let p = params(&c);
        let r = ok(&mut s, method, p.clone());
        assert_eq!(depth(&s), d + 1, "{method} {p}: one undo step");
        assert_eq!(r["op"], method.trim_start_matches("mesh."), "{method}");
        ok(&mut s, "undo", json!({}));
        assert_eq!(depth(&s), d, "{method}: undo took one step back");
        assert_eq!(
            snapshot(&mut s, &c.id),
            before,
            "{method} {p}: undo restores the model"
        );
        assert_eq!(
            ok(&mut s, "document.summary", json!({}))["objects_total"],
            objects_before,
            "{method}"
        );
    }
}

#[test]
fn explode_and_the_solid_operations_are_one_undo_step() {
    // Explode a group.
    let mut s = Session::with_sequential_ids();
    let c = scene(&mut s);
    let g = ok(
        &mut s,
        "mesh.make_group",
        json!({"id": c.id, "faces": [c.top]}),
    );
    let group = g["group"].as_str().unwrap().to_string();
    assert_eq!(g["created_objects"].as_array().unwrap().len(), 2);
    let d = depth(&s);
    let r = ok(&mut s, "mesh.explode", json!({"ids": [group]}));
    assert_eq!(depth(&s), d + 1);
    assert_eq!(r["op"], "explode");
    assert!(!r["removed_objects"].as_array().unwrap().is_empty());
    ok(&mut s, "undo", json!({}));
    assert_eq!(depth(&s), d);
    // A single id is a list of one for explode.
    ok(&mut s, "mesh.explode", json!({"id": group}));
    // A component.
    let mut s = Session::with_sequential_ids();
    let c = scene(&mut s);
    let r = ok(
        &mut s,
        "mesh.make_component",
        json!({"id": c.id, "faces": [c.top], "name": "Lid", "replace": true}),
    );
    assert_eq!(r["created_resources"].as_array().unwrap().len(), 1);
    assert!(!r["created_objects"].as_array().unwrap().is_empty());

    for op in ["union", "subtract", "intersect", "trim", "split"] {
        let mut s = Session::with_sequential_ids();
        let id = new_model(&mut s);
        draw_boxes(
            &mut s,
            &id,
            &[([0.0; 3], [1000.0; 3]), ([500.0; 3], [1500.0; 3])],
        );
        let a = face_facing(&mut s, &id, [0.0, 0.0, -1.0]);
        let b = face_facing(&mut s, &id, [0.0, 0.0, 1.0]);
        let before = snapshot(&mut s, &id);
        let d = depth(&s);
        let r = ok(
            &mut s,
            "mesh.solid",
            json!({"id": id, "op": op, "first_face": a, "second_face": b}),
        );
        assert_eq!(depth(&s), d + 1, "{op}");
        assert_eq!(r["op"], "solid");
        let report = ok(&mut s, "mesh.solid_report", json!({"id": id}));
        assert_eq!(report["closed"], true, "{op}");
        assert_eq!(
            report["shells"].as_array().unwrap().len(),
            if op == "split" {
                3
            } else if op == "trim" {
                2
            } else {
                1
            },
            "{op}"
        );
        ok(&mut s, "undo", json!({}));
        assert_eq!(snapshot(&mut s, &id), before, "{op}");
    }
}

#[test]
fn solid_takes_lists_of_faces_too_and_refuses_a_mix() {
    let mut s = Session::with_sequential_ids();
    let id = new_model(&mut s);
    draw_boxes(
        &mut s,
        &id,
        &[([0.0; 3], [1000.0; 3]), ([500.0; 3], [1500.0; 3])],
    );
    let report = ok(&mut s, "mesh.solid_report", json!({"id": id}));
    let first = report["shells"][0]["faces"].clone();
    let second = report["shells"][1]["faces"].clone();
    ok(
        &mut s,
        "mesh.solid",
        json!({"id": id, "op": "UNION", "first": first, "second": second}),
    );
    let e = err(
        &mut s,
        "mesh.solid",
        json!({"id": id, "op": "union", "first": [1]}),
    );
    assert_eq!(e.code, -32602);
    let e = err(
        &mut s,
        "mesh.solid",
        json!({"id": id, "op": "union", "first": [1], "first_face": 1, "second_face": 2}),
    );
    assert_eq!(e.code, -32602);
    let e = err(
        &mut s,
        "mesh.solid",
        json!({"id": id, "op": "union", "first_face": 9999, "second_face": 1}),
    );
    assert_eq!(error_id(&e), "no_such_face");
    assert_eq!(e.details.as_ref().unwrap()["face"], 9999);
}

#[test]
fn a_solid_operation_on_an_open_surface_is_refused_with_the_edge_to_fix() {
    let mut s = Session::with_sequential_ids();
    let id = new_model(&mut s);
    let flat = ok(
        &mut s,
        "mesh.add_edges",
        json!({"id": id, "edges": rect(0.0, 0.0, 1000.0, 1000.0)}),
    )["created"]["faces"][0]
        .clone();
    draw_boxes(
        &mut s,
        &id,
        &[([3000.0, 0.0, 0.0], [4000.0, 1000.0, 1000.0])],
    );
    let boxed = face_facing(&mut s, &id, [0.0, 0.0, -1.0]);
    let d = depth(&s);
    let e = err(
        &mut s,
        "mesh.solid",
        json!({"id": id, "op": "union", "first_face": flat, "second_face": boxed}),
    );
    assert_eq!(e.code, 3);
    assert_eq!(e.kind, "validation");
    assert_eq!(error_id(&e), "solid_open");
    let data = e.details.as_ref().unwrap();
    assert_eq!(data["operand"], "first");
    assert_eq!(data["open_kind"], "border");
    assert!(data["edge"].is_u64());
    assert!(e.message.contains("not a closed solid"), "{}", e.message);
    assert_eq!(depth(&s), d, "a refusal makes no step");
}

#[test]
fn refusals_have_stable_ids_and_the_facts() {
    let mut s = Session::with_sequential_ids();
    let c = scene(&mut s);
    let id = &c.id;
    let before = snapshot(&mut s, id);
    let d = depth(&s);
    let e = err(
        &mut s,
        "mesh.push_pull",
        json!({"id": id, "face": 9999, "distance": 10}),
    );
    assert_eq!((e.code, error_id(&e).as_str()), (2, "no_such_face"));
    assert_eq!(e.details.as_ref().unwrap()["face"], 9999);
    assert_eq!(e.details.as_ref().unwrap()["object"], json!(id));
    assert_eq!(e.reason, Some("no_such_face"));
    let e = err(&mut s, "mesh.erase", json!({"id": id, "edges": [9999]}));
    assert_eq!(error_id(&e), "no_such_edge");
    let e = err(
        &mut s,
        "mesh.transform",
        json!({"id": id, "vertices": [9999], "translate": [1, 0, 0]}),
    );
    assert_eq!(error_id(&e), "no_such_vertex");
    let e = err(
        &mut s,
        "mesh.add_edges",
        json!({"id": id, "edges": [[[0, 0, 5000], [0, 0, 5000]]]}),
    );
    assert_eq!(error_id(&e), "edge_too_short");
    assert!(e.details.as_ref().unwrap()["tolerance_mm"].is_number());
    let e = err(
        &mut s,
        "mesh.add_edges",
        json!({"id": id, "edges": [[[0, 0, 5000], [1e300, 0, 5000]]]}),
    );
    assert_eq!(error_id(&e), "bad_point");
    let e = err(
        &mut s,
        "mesh.push_pull",
        json!({"id": id, "face": c.top, "distance": -5000}),
    );
    assert_eq!(e.code, 3);
    assert!(!e.message.is_empty());
    let e = err(
        &mut s,
        "mesh.offset",
        json!({"id": id, "face": c.top, "distance": 100000}),
    );
    assert_eq!(e.code, 3);
    assert_eq!(depth(&s), d);
    assert_eq!(snapshot(&mut s, id), before);
}

#[test]
fn a_model_over_the_limit_is_refused_as_a_limit() {
    let mut s = Session::with_sequential_ids();
    let id = new_model(&mut s);
    let edges: Vec<Value> = (0..10_001)
        .map(|i| {
            json!([
                [f64::from(i) * 10.0, 0, 0],
                [f64::from(i) * 10.0 + 5.0, 0, 0]
            ])
        })
        .collect();
    let e = err(&mut s, "mesh.add_edges", json!({"id": id, "edges": edges}));
    assert_eq!((e.code, error_id(&e).as_str()), (6, "model_limit"));
    assert_eq!(e.details.as_ref().unwrap()["max"], 10_000);
}

#[test]
fn the_parameters_are_checked() {
    let mut s = Session::with_sequential_ids();
    let c = scene(&mut s);
    let id = &c.id;
    let bad = [
        ("mesh.erase", json!({"id": id})),
        ("mesh.transform", json!({"id": id, "translate": [1, 0, 0]})),
        ("mesh.transform", json!({"id": id, "faces": [1]})),
        (
            "mesh.transform",
            json!({"id": id, "faces": [1], "matrix": [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0], "translate": [1, 0, 0]}),
        ),
        (
            "mesh.transform",
            json!({"id": id, "faces": [1], "scale": {}}),
        ),
        (
            "mesh.transform",
            json!({"id": id, "faces": [1], "scale": {"factor": 0}}),
        ),
        (
            "mesh.transform",
            json!({"id": id, "faces": [1], "rotate": {"axis": [0, 0, 0], "degrees": 5}}),
        ),
        ("mesh.copy", json!({"id": id, "translate": [1, 0, 0]})),
        (
            "mesh.copy",
            json!({"id": id, "faces": [1], "translate": [1, 0, 0], "count": 0}),
        ),
        ("mesh.hide", json!({"id": id})),
        ("mesh.soften", json!({"id": id, "edges": [1]})),
        ("mesh.soften_edges", json!({"id": id, "below_angle": 181})),
        ("mesh.soften_edges", json!({"id": id, "below_angle": -1})),
        ("mesh.offset", json!({"id": id, "distance": 5})),
        (
            "mesh.offset",
            json!({"id": id, "edges": [1], "distance": 5}),
        ),
        (
            "mesh.offset",
            json!({"id": id, "face": 1, "edges": [1], "distance": 5, "toward": [0, 0, 0]}),
        ),
        (
            "mesh.paint",
            json!({"id": id, "faces": [1], "material": "No such material"}),
        ),
        (
            "mesh.solid",
            json!({"id": id, "op": "weld", "first": [1], "second": [2]}),
        ),
        ("mesh.push_pull", json!({"id": id, "face": 1})),
        (
            "mesh.push_pull",
            json!({"id": id, "face": 1, "distance": 1, "extra": 2}),
        ),
    ];
    for (m, p) in bad {
        let e = err(&mut s, m, p.clone());
        assert_eq!(e.code, -32602, "{m} {p}: {}", e.message);
    }
}

#[test]
fn paint_hide_soften_and_reverse_show_in_the_reads() {
    let mut s = Session::with_sequential_ids();
    let c = scene(&mut s);
    let id = &c.id;
    ok(
        &mut s,
        "mesh.paint",
        json!({"id": id, "faces": [c.top], "material": "Brick"}),
    );
    let f = ok(&mut s, "mesh.faces", json!({"id": id, "ids": [c.top]}));
    assert_eq!(f["items"][0]["material"], json!(c.material));
    assert_eq!(f["items"][0]["back_material"], Value::Null);
    ok(
        &mut s,
        "mesh.paint",
        json!({"id": id, "faces": [c.top], "side": "back", "material": c.material}),
    );
    let f = ok(&mut s, "mesh.faces", json!({"id": id, "ids": [c.top]}));
    assert_eq!(f["items"][0]["back_material"], json!(c.material));
    ok(
        &mut s,
        "mesh.paint",
        json!({"id": id, "faces": [c.top], "side": "both"}),
    );
    let f = ok(&mut s, "mesh.faces", json!({"id": id, "ids": [c.top]}));
    assert_eq!(f["items"][0]["material"], Value::Null);
    assert_eq!(f["items"][0]["back_material"], Value::Null);

    ok(
        &mut s,
        "mesh.hide",
        json!({"id": id, "faces": [c.top], "edges": [c.edge]}),
    );
    let sum = ok(&mut s, "mesh.summary", json!({"id": id}));
    assert_eq!(sum["models"][0]["hidden_faces"], 1);
    assert_eq!(sum["models"][0]["hidden_edges"], 1);
    ok(
        &mut s,
        "mesh.hide",
        json!({"id": id, "faces": [c.top], "edges": [c.edge], "hidden": false}),
    );
    assert_eq!(
        ok(&mut s, "mesh.summary", json!({"id": id}))["models"][0]["hidden_faces"],
        0
    );

    ok(
        &mut s,
        "mesh.soften",
        json!({"id": id, "edges": [c.edge], "soft": true}),
    );
    let e = ok(&mut s, "mesh.edges", json!({"id": id, "ids": [c.edge]}));
    assert_eq!(e["items"][0]["soft"], true);
    assert_eq!(e["items"][0]["smooth"], false);
    ok(
        &mut s,
        "mesh.soften",
        json!({"id": id, "edges": [c.edge], "smooth": true}),
    );
    let e = ok(&mut s, "mesh.edges", json!({"id": id, "ids": [c.edge]}));
    assert_eq!(
        (
            e["items"][0]["soft"].clone(),
            e["items"][0]["smooth"].clone()
        ),
        (json!(true), json!(true))
    );
    assert_eq!(
        ok(&mut s, "mesh.summary", json!({"id": id}))["models"][0]["soft_edges"],
        1
    );

    let normal = |s: &mut Session| {
        ok(s, "mesh.faces", json!({"id": id, "ids": [c.top]}))["items"][0]["normal"].clone()
    };
    assert_eq!(normal(&mut s), json!([0.0, 0.0, 1.0]));
    ok(
        &mut s,
        "mesh.reverse_faces",
        json!({"id": id, "faces": [c.top]}),
    );
    assert_eq!(normal(&mut s), json!([0.0, 0.0, -1.0]));
}

/// A closed 24-sided prism whose sides meet at 15 degrees, with hard edges.
fn prism(s: &mut Session) -> (String, u64) {
    let id = new_model(s);
    let pts: Vec<Value> = (0..24)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 24.0;
            json!([1000.0 * a.cos(), 1000.0 * a.sin(), 0.0])
        })
        .collect();
    ok(
        s,
        "mesh.add_curve",
        json!({"id": id, "points": pts, "closed": true, "faceted": true}),
    );
    let face = face_facing(s, &id, [0.0, 0.0, 1.0]);
    ok(
        s,
        "mesh.push_pull",
        json!({"id": id, "face": face, "distance": 2000}),
    );
    (id, face)
}

#[test]
fn soften_edges_softens_the_edges_under_an_angle_and_counts_the_changes() {
    let mut s = Session::with_sequential_ids();
    let (id, _) = prism(&mut s);
    let sum = ok(&mut s, "mesh.summary", json!({"id": id}));
    assert_eq!(sum["models"][0]["soft_edges"], 0);
    // The edges between side faces meet at 15 degrees, the rim at 90.
    let none = ok(
        &mut s,
        "mesh.soften_edges",
        json!({"id": id, "below_angle": 10}),
    );
    assert_eq!(
        (none["changed"].clone(), none["softened"].clone()),
        (json!(0), json!(0))
    );
    assert_eq!(none["considered"], 72);
    let d = depth(&s);
    let r = ok(
        &mut s,
        "mesh.soften_edges",
        json!({"id": id, "below_angle": 20}),
    );
    assert_eq!(depth(&s), d + 1);
    assert_eq!(r["changed"], 24);
    assert_eq!(r["softened"], 24);
    assert_eq!(r["op"], "soften_edges");
    // Again: nothing changes.
    let again = ok(
        &mut s,
        "mesh.soften_edges",
        json!({"id": id, "below_angle": 20}),
    );
    assert_eq!(again["changed"], 0);
    assert_eq!(again["softened"], 24);
    // A larger angle takes the rim too (48 more edges on the two rims).
    let all_soft = ok(
        &mut s,
        "mesh.soften_edges",
        json!({"id": id, "below_angle": 100}),
    );
    assert_eq!(all_soft["changed"], 48);
    assert_eq!(all_soft["softened"], 72);
    // Back to hard.
    let hard = ok(
        &mut s,
        "mesh.soften_edges",
        json!({"id": id, "below_angle": 0}),
    );
    assert_eq!(hard["changed"], 72);
    assert_eq!(hard["softened"], 0);
    // Only some edges, and not smooth.
    let first24: Vec<u64> = ids(&all(&mut s, "mesh.edges", &id))
        .into_iter()
        .take(24)
        .collect();
    let some = ok(
        &mut s,
        "mesh.soften_edges",
        json!({"id": id, "below_angle": 20, "edges": first24, "smooth": false}),
    );
    assert!(some["changed"].as_u64().unwrap() <= 24);
    assert_eq!(some["softened"], some["changed"]);
    let e = ok(&mut s, "mesh.edges", json!({"id": id, "limit": 1000}));
    assert!(
        e["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["smooth"] == false)
    );
    ok(&mut s, "undo", json!({}));
}

#[test]
fn a_flat_model_has_nothing_to_soften_without_coplanar() {
    let mut s = Session::with_sequential_ids();
    let id = new_model(&mut s);
    ok(
        &mut s,
        "mesh.add_edges",
        json!({"id": id, "edges": rect(0.0, 0.0, 100.0, 100.0)}),
    );
    let r = ok(
        &mut s,
        "mesh.soften_edges",
        json!({"id": id, "below_angle": 90}),
    );
    assert_eq!(
        (r["changed"].clone(), r["considered"].clone()),
        (json!(0), json!(0))
    );
}

#[test]
fn follow_me_offset_copy_and_intersect_make_new_geometry() {
    let mut s = Session::with_sequential_ids();
    let id = new_model(&mut s);
    let sq = [
        [0.0, -100.0, -100.0],
        [0.0, 100.0, -100.0],
        [0.0, 100.0, 100.0],
        [0.0, -100.0, 100.0],
    ];
    let edges: Vec<Value> = (0..4).map(|i| json!([sq[i], sq[(i + 1) % 4]])).collect();
    ok(&mut s, "mesh.add_edges", json!({"id": id, "edges": edges}));
    let profile = ok(&mut s, "mesh.faces", json!({"id": id}))["items"][0]["id"]
        .as_u64()
        .unwrap();
    ok(
        &mut s,
        "mesh.add_edges",
        json!({"id": id, "edges": [[[0, 0, 0], [2000, 0, 0]], [[2000, 0, 0], [2000, 1000, 0]]]}),
    );
    let p1 = edge_between(&mut s, &id, [0.0; 3], [2000.0, 0.0, 0.0]);
    let p2 = edge_between(&mut s, &id, [2000.0, 0.0, 0.0], [2000.0, 1000.0, 0.0]);
    let d = depth(&s);
    let r = ok(
        &mut s,
        "mesh.follow_me",
        json!({"id": id, "face": profile, "path": [p1, p2]}),
    );
    assert_eq!(depth(&s), d + 1);
    assert!(r["created"]["faces"].as_array().unwrap().len() >= 6);
    assert_eq!(
        ok(&mut s, "mesh.solid_report", json!({"id": id}))["shells"][0]["state"],
        "solid"
    );
    let e = err(
        &mut s,
        "mesh.follow_me",
        json!({"id": id, "face": 9999, "path": [p1]}),
    );
    assert_eq!(error_id(&e), "no_such_face");

    // Copy faces and intersect two boxes.
    let mut s = Session::with_sequential_ids();
    let id = new_model(&mut s);
    draw_boxes(
        &mut s,
        &id,
        &[([0.0; 3], [1000.0; 3]), ([500.0; 3], [1500.0; 3])],
    );
    let before = ok(&mut s, "mesh.summary", json!({"id": id}))["models"][0]["faces"]
        .as_u64()
        .unwrap();
    let r = ok(
        &mut s,
        "mesh.intersect_faces",
        json!({"id": id, "scope": "model"}),
    );
    assert!(r["counts"]["faces"].as_u64().unwrap() > before);
    let top = face_facing(&mut s, &id, [0.0, 0.0, 1.0]);
    let r = ok(
        &mut s,
        "mesh.intersect_faces",
        json!({"id": id, "scope": "selection", "faces": [top]}),
    );
    assert_eq!(r["op"], "intersect_faces");
    let r = ok(
        &mut s,
        "mesh.copy",
        json!({"id": id, "faces": [top], "translate": [5000, 0, 0]}),
    );
    assert!(!r["created"]["faces"].as_array().unwrap().is_empty());
}

#[test]
fn erase_takes_faces_first_so_the_edges_are_still_good() {
    let mut s = Session::with_sequential_ids();
    let c = scene(&mut s);
    let r = ok(
        &mut s,
        "mesh.erase",
        json!({"id": c.id, "faces": [c.top, c.bottom], "edges": [c.edge]}),
    );
    assert!(r["counts"]["faces"].as_u64().unwrap() < 6);
}

#[test]
fn a_batch_is_one_undo_step_with_the_change_of_each_edit() {
    let mut s = Session::with_sequential_ids();
    let id = new_model(&mut s);
    let before = snapshot(&mut s, &id);
    let d = depth(&s);
    let r = ok(
        &mut s,
        "mesh.batch",
        json!({"id": id, "edits": [
            {"method": "add_edges", "edges": rect(0.0, 0.0, 1000.0, 1000.0)},
            {"method": "add_edges", "edges": rect(2000.0, 0.0, 3000.0, 1000.0)},
            {"method": "add_curve", "points": [[5000, 0, 0], [5100, 0, 0], [5100, 100, 0]], "closed": true},
        ]}),
    );
    assert_eq!(depth(&s), d + 1);
    assert_eq!(r["count"], 3);
    assert_eq!(
        r["edits"][0]["created"]["faces"].as_array().unwrap().len(),
        1
    );
    assert_eq!(r["edits"][1]["counts"]["faces"], 2);
    assert_eq!(r["edits"][2]["op"], "add_curve");
    // The second step can use ids that the first one made, by a second batch.
    let top = r["edits"][0]["created"]["faces"][0].as_u64().unwrap();
    let r2 = ok(
        &mut s,
        "mesh.batch",
        json!({"id": id, "edits": [
            {"method": "push_pull", "face": top, "distance": 500},
            {"method": "soften_edges", "below_angle": 45},
            {"method": "hide", "faces": [top]},
        ]}),
    );
    assert_eq!(depth(&s), d + 2);
    assert!(r2["edits"][1]["changed"].is_number());
    ok(&mut s, "undo", json!({}));
    ok(&mut s, "undo", json!({}));
    assert_eq!(snapshot(&mut s, &id), before);
}

#[test]
fn a_batch_can_work_on_several_models_and_group() {
    let mut s = Session::with_sequential_ids();
    let a = new_model(&mut s);
    let b = new_model(&mut s);
    let plate = ok(
        &mut s,
        "mesh.add_edges",
        json!({"id": b, "edges": rect(0.0, 0.0, 50.0, 50.0)}),
    )["created"]["faces"][0]
        .clone();
    let r = ok(
        &mut s,
        "mesh.batch",
        json!({"edits": [
            {"method": "add_edges", "id": a, "edges": cuboid([0.0; 3], [100.0; 3])},
            {"method": "mesh.hide", "id": b, "faces": [plate]},
            {"method": "make_group", "id": b, "faces": [plate], "name": "Plate"},
        ]}),
    );
    assert_eq!(r["edits"][2]["op"], "make_group");
    assert!(r["edits"][2]["group"].is_string());
    let e = err(
        &mut s,
        "mesh.batch",
        json!({"edits": [{"method": "add_edges", "edges": []}]}),
    );
    assert_eq!(e.code, -32602);
    assert!(e.message.contains("edits[0]"), "{}", e.message);
}

#[test]
fn a_batch_is_all_or_nothing() {
    let mut s = Session::with_sequential_ids();
    let c = scene(&mut s);
    let id = &c.id;
    let before = snapshot(&mut s, id);
    let d = depth(&s);
    let doc_before = ok(&mut s, "document.summary", json!({}));
    let e = err(
        &mut s,
        "mesh.batch",
        json!({"id": id, "edits": [
            {"method": "push_pull", "face": c.top, "distance": 200},
            {"method": "add_edges", "edges": [[[3000, 0, 0], [4000, 0, 0]]]},
            {"method": "push_pull", "face": 9999, "distance": 5},
            {"method": "hide", "faces": [c.top]},
        ]}),
    );
    assert_eq!(error_id(&e), "no_such_face");
    assert_eq!(e.details.as_ref().unwrap()["step"], 2);
    assert_eq!(depth(&s), d, "no undo step for a failed batch");
    assert_eq!(snapshot(&mut s, id), before);
    assert_eq!(ok(&mut s, "document.summary", json!({})), doc_before);
    // The same batch without the bad edit works.
    ok(
        &mut s,
        "mesh.batch",
        json!({"id": id, "edits": [
            {"method": "push_pull", "face": c.top, "distance": 200},
            {"method": "add_edges", "edges": [[[3000, 0, 0], [4000, 0, 0]]]},
        ]}),
    );
    assert_eq!(depth(&s), d + 1);
    // A solid refusal inside a batch keeps its id and names the step.
    let id2 = new_model(&mut s);
    let d = depth(&s);
    ok(
        &mut s,
        "mesh.add_edges",
        json!({"id": id2, "edges": rect(0.0, 0.0, 100.0, 100.0)}),
    );
    let e = err(
        &mut s,
        "mesh.batch",
        json!({"id": id2, "edits": [
            {"method": "hide", "faces": [1]},
            {"method": "solid", "first_face": 1, "second_face": 1},
        ]}),
    );
    assert_eq!(e.code, -32602);
    assert_eq!(depth(&s), d + 1);
}

#[test]
fn a_solid_refusal_in_a_batch_undoes_the_earlier_edits() {
    let mut s = Session::with_sequential_ids();
    let id = new_model(&mut s);
    ok(
        &mut s,
        "mesh.add_edges",
        json!({"id": id, "edges": rect(0.0, 0.0, 100.0, 100.0)}),
    );
    let before = snapshot(&mut s, &id);
    let d = depth(&s);
    let e = err(
        &mut s,
        "mesh.batch",
        json!({"id": id, "edits": [
            {"method": "add_edges", "edges": [[[500, 0, 0], [600, 0, 0]]]},
            {"method": "solid", "solid_op_is_not_a_name": 1},
        ]}),
    );
    assert_eq!(e.code, -32602);
    let e = err(
        &mut s,
        "mesh.batch",
        json!({"id": id, "edits": [
            {"method": "add_edges", "edges": [[[500, 0, 0], [600, 0, 0]]]},
            {"method": "hide", "faces": [77]},
        ]}),
    );
    assert_eq!(error_id(&e), "no_such_face");
    assert_eq!(e.details.as_ref().unwrap()["step"], 1);
    assert_eq!(snapshot(&mut s, &id), before);
    assert_eq!(depth(&s), d);
}

#[test]
fn a_batch_has_a_hard_cap() {
    let mut s = Session::with_sequential_ids();
    let id = new_model(&mut s);
    let one = json!({"method": "add_edges", "edges": [[[0, 0, 0], [1, 0, 0]]]});
    let many: Vec<Value> = (0..51).map(|_| one.clone()).collect();
    let e = err(&mut s, "mesh.batch", json!({"id": id, "edits": many}));
    assert_eq!((e.code, error_id(&e).as_str()), (6, "batch_too_large"));
    assert_eq!(e.details.as_ref().unwrap()["max"], 50);
    // Fifty is fine.
    let fifty: Vec<Value> = (0..50)
        .map(|i| json!({"method": "add_edges", "edges": [[[f64::from(i) * 10.0, 0, 0], [f64::from(i) * 10.0 + 5.0, 0, 0]]]}))
        .collect();
    let d = depth(&s);
    let r = ok(&mut s, "mesh.batch", json!({"id": id, "edits": fifty}));
    assert_eq!(r["count"], 50);
    assert_eq!(depth(&s), d + 1);
    // Too many edges in all.
    let big = |n: u32, at: f64| -> Value {
        let edges: Vec<Value> = (0..n)
            .map(|i| {
                json!([
                    [at + f64::from(i) * 10.0, 100, 0],
                    [at + f64::from(i) * 10.0 + 5.0, 100, 0]
                ])
            })
            .collect();
        json!({"method": "add_edges", "edges": edges})
    };
    let e = err(
        &mut s,
        "mesh.batch",
        json!({"id": id, "edits": [big(6000, 0.0), big(6000, 100000.0)]}),
    );
    assert_eq!((e.code, error_id(&e).as_str()), (6, "batch_edges_too_many"));
    for bad in [
        json!({"id": id, "edits": []}),
        json!({"id": id, "edits": [{"edges": []}]}),
        json!({"id": id, "edits": [{"method": "render"}]}),
        json!({"id": id, "edits": [{"method": "create"}]}),
        json!({"id": id, "edits": [{"method": "push_pull", "face": 1, "distance": 5, "oops": 1}]}),
        json!({"edits": [{"method": "push_pull", "face": 1, "distance": 5}]}),
    ] {
        assert_eq!(err(&mut s, "mesh.batch", bad.clone()).code, -32602, "{bad}");
    }
}

#[test]
fn a_notes_text_tells_what_a_merge_dropped() {
    let mut s = Session::with_sequential_ids();
    let id = new_model(&mut s);
    ok(
        &mut s,
        "mesh.add_edges",
        json!({"id": id, "edges": rect(0.0, 0.0, 100.0, 100.0)}),
    );
    ok(
        &mut s,
        "mesh.add_edges",
        json!({"id": id, "edges": [[[50, 0, 0], [50, 100, 0]]]}),
    );
    let mat = ok(
        &mut s,
        "material.create",
        json!({"name": "Red", "base_color": [200, 0, 0]}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    let faces = all(&mut s, "mesh.faces", &id);
    assert_eq!(faces.len(), 2);
    ok(
        &mut s,
        "mesh.paint",
        json!({"id": id, "faces": [ids(&faces)[1]], "material": mat}),
    );
    let middle = edge_between(&mut s, &id, [50.0, 0.0, 0.0], [50.0, 100.0, 0.0]);
    let r = ok(&mut s, "mesh.erase", json!({"id": id, "edges": [middle]}));
    assert_eq!(r["counts"]["faces"], 1);
    assert!(r.get("notes").is_none() || r["notes"].is_array());
}

#[test]
fn the_edits_of_an_agent_are_marked_as_agent_changes() {
    let mut s = Session::with_sequential_ids();
    let id = match s.handle_as(7, Origin::Agent, "mesh.create", json!({})) {
        Ok(v) => v["id"].as_str().unwrap().to_string(),
        Err(e) => panic!("{}", e.message),
    };
    let face = s
        .handle_as(
            7,
            Origin::Agent,
            "mesh.add_edges",
            json!({"id": id, "edges": rect(0.0, 0.0, 100.0, 100.0)}),
        )
        .unwrap()["created"]["faces"][0]
        .clone();
    s.handle_as(
        7,
        Origin::Agent,
        "mesh.batch",
        json!({"id": id, "edits": [{"method": "hide", "faces": [face]}]}),
    )
    .unwrap();
    // Reads are no steps.
    s.handle_as(7, Origin::Agent, "mesh.summary", json!({}))
        .unwrap();
    let log = s
        .handle_as(9, Origin::User, "agent.activity", json!({}))
        .unwrap();
    let steps = log["steps"].as_array().unwrap();
    let methods: Vec<&str> = steps
        .iter()
        .map(|x| x["method"].as_str().unwrap())
        .collect();
    assert_eq!(methods, ["mesh.create", "mesh.add_edges", "mesh.batch"]);
    assert_eq!(steps[1]["counts"]["objects"]["changed"], 1);
    let one = s
        .handle_as(
            9,
            Origin::User,
            "agent.activity",
            json!({"step": steps[2]["id"]}),
        )
        .unwrap();
    assert_eq!(one["objects"]["changed"], json!([id]));
    // A person's edit is not marked.
    s.handle_as(
        9,
        Origin::User,
        "mesh.hide",
        json!({"id": id, "faces": [face], "hidden": false}),
    )
    .unwrap();
    let log = s
        .handle_as(9, Origin::User, "agent.activity", json!({}))
        .unwrap();
    assert_eq!(log["steps"].as_array().unwrap().len(), 3);
}

#[test]
fn render_gives_a_png_of_the_model_with_the_camera() {
    use base64::Engine;
    let mut s = Session::with_sequential_ids();
    let c = scene(&mut s);
    let r = ok(
        &mut s,
        "mesh.render",
        json!({"id": c.id, "width": 160, "height": 120}),
    );
    let png = base64::engine::general_purpose::STANDARD
        .decode(r["png_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(&png[1..4], b"PNG");
    assert_eq!(
        (r["width"].clone(), r["height"].clone()),
        (json!(160), json!(120))
    );
    assert!(r["camera"]["distance"].as_f64().unwrap() > 0.0);
    // A free camera is used as given, and the picture differs from the standard view.
    let cam = json!({"target": [500, 500, 500], "yaw": 0.8, "pitch": 0.5, "distance": 5000, "fov": 45, "projection": "perspective"});
    let r2 = ok(
        &mut s,
        "mesh.render",
        json!({"camera": cam, "width": 160, "height": 120, "mode": "hidden_line"}),
    );
    assert_eq!(r2["projection"], "perspective");
    assert_eq!(r2["camera"]["projection"], "perspective");
    assert!((r2["camera"]["distance"].as_f64().unwrap() - 5000.0).abs() < 1e-3);
    assert_ne!(r["png_base64"], r2["png_base64"]);
    // The same through render.png.
    let r3 = ok(
        &mut s,
        "render.png",
        json!({"camera": cam, "width": 160, "height": 120, "mode": "hidden_line"}),
    );
    assert_eq!(r3["png_base64"], r2["png_base64"]);
    // Without a camera render.png keeps its answer as it was.
    let plain = ok(&mut s, "render.png", json!({"width": 32, "height": 32}));
    assert!(plain.get("camera").is_none());
    // A pushed face changes the picture.
    ok(
        &mut s,
        "mesh.push_pull",
        json!({"id": c.id, "face": c.top, "distance": 1500}),
    );
    let r4 = ok(
        &mut s,
        "mesh.render",
        json!({"camera": cam, "width": 160, "height": 120, "mode": "hidden_line"}),
    );
    assert_ne!(r4["png_base64"], r2["png_base64"]);
    // A render style by name, and the refusals.
    let r5 = ok(
        &mut s,
        "mesh.render",
        json!({"id": c.id, "render_style": "Realistic", "width": 96, "height": 96}),
    );
    assert!(r5["render_style"].is_object());
    let bad_cam = json!({"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 0, "fov": 45, "projection": "perspective"});
    assert_eq!(
        err(&mut s, "mesh.render", json!({"camera": bad_cam})).code,
        -32602
    );
    assert_eq!(
        err(
            &mut s,
            "mesh.render",
            json!({"id": "00000000-0000-0000-0000-0000000000ff"})
        )
        .code,
        2
    );
    // A render makes no undo step.
    let d = depth(&s);
    ok(&mut s, "mesh.render", json!({"width": 32, "height": 32}));
    assert_eq!(depth(&s), d);
}

#[test]
fn every_mesh_method_is_listed_documented_and_has_an_example() {
    let names: Vec<&str> = METHODS
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| n.starts_with("mesh."))
        .collect();
    assert_eq!(names.len(), 29, "{names:?}");
    for (n, d) in METHODS.iter().filter(|(n, _)| n.starts_with("mesh.")) {
        assert!(
            d.contains("params:") || d.contains("params "),
            "{n} documents its params"
        );
    }
    let mut s = Session::with_sequential_ids();
    let d = ok(&mut s, "describe", json!({}));
    assert!(d["mesh"]["examples"].as_array().unwrap().len() >= 10);
    assert_eq!(d["mesh"]["limits"]["batch_edits"], 50);
    let m = ok(&mut s, "describe", json!({"method": "mesh.soften_edges"}));
    assert!(m["params"]["properties"]["below_angle"].is_object());
    assert_eq!(m["params"]["required"], json!(["id", "below_angle"]));
    // Every edit method of the list is in the batch schema text.
    let batch = ok(&mut s, "describe", json!({"method": "mesh.batch"}));
    for n in &names {
        let op = n.trim_start_matches("mesh.");
        if [
            "summary",
            "vertices",
            "edges",
            "faces",
            "solid_report",
            "bounds",
            "near",
            "pick",
            "create",
            "batch",
            "render",
        ]
        .contains(&op)
        {
            continue;
        }
        assert!(
            batch["params"]["properties"]["edits"]["description"]
                .as_str()
                .unwrap()
                .contains(op),
            "{op}"
        );
    }
}

#[test]
fn the_edit_methods_are_edits_and_the_reads_are_reads_for_scripts() {
    let mut s = Session::with_sequential_ids();
    let c = scene(&mut s);
    // A call that only reads changes nothing.
    let rev = s.editor.revision();
    for (m, p) in [
        ("mesh.summary", json!({})),
        ("mesh.vertices", json!({"id": c.id})),
        ("mesh.edges", json!({"id": c.id})),
        ("mesh.faces", json!({"id": c.id})),
        ("mesh.solid_report", json!({"id": c.id})),
        ("mesh.bounds", json!({"id": c.id})),
        ("mesh.near", json!({"point": [0, 0, 0], "radius": 5})),
        (
            "mesh.pick",
            json!({"origin": [0, 0, 5000], "direction": [0, 0, -1]}),
        ),
        ("mesh.render", json!({"width": 32, "height": 32})),
    ] {
        ok(&mut s, m, p);
        assert_eq!(s.editor.revision(), rev, "{m} reads only");
    }
}
