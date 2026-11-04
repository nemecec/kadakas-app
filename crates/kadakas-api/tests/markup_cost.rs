//! REV-11: what picking the marks of a markup costs on a big drawing. Run by hand:
//! `cargo test --release -p kadakas-api --test markup_cost -- --ignored --nocapture`
//! 200 surface models of about 2,000 triangles each (a grid of 32 x 32 faces) on a plain, a top
//! view of the plain, and markups with big rectangles.

use std::time::Instant;

use serde_json::{Value, json};

use kadakas_api::Session;

fn ok(s: &mut Session, method: &str, params: Value) -> Value {
    s.handle(method, params)
        .unwrap_or_else(|e| panic!("{method}: {}", e.message))
}

fn grid(n: usize, step: f64) -> Vec<Value> {
    let mut edges = Vec::new();
    for i in 0..=n {
        for j in 0..=n {
            let (x, y) = (i as f64 * step, j as f64 * step);
            if i < n {
                edges.push(json!([[x, y, 0.0], [x + step, y, 0.0]]));
            }
            if j < n {
                edges.push(json!([[x, y, 0.0], [x, y + step, 0.0]]));
            }
        }
    }
    edges
}

#[test]
#[ignore = "measurement"]
fn the_cost_of_markup_get_on_200_models_of_2000_triangles() {
    let mut s = Session::with_sequential_ids();
    let edges = grid(32, 10.0);
    for k in 0..200 {
        let id = ok(&mut s, "mesh.create", json!({"name": format!("M{k}")}))["id"].clone();
        ok(&mut s, "mesh.add_edges", json!({"id": id, "edges": edges}));
        ok(
            &mut s,
            "execute",
            json!({"command": {"op": "move_objects", "ids": [id], "delta": [(k % 20) as f64 * 400.0, (k / 20) as f64 * 400.0]}}),
        );
    }
    let view = json!({
        "view": {"kind": "camera", "target": [4000, 2000, 0], "yaw": -1.5707963, "pitch": 1.5707963,
                 "distance": 12000, "perspective": false},
        "canvas": [800, 600],
    });
    for (marks, samples) in [(1usize, 25usize), (10, 25), (50, 100)] {
        let list: Vec<Value> = (0..marks)
            .map(|k| {
                let o = k as f64 * 0.01;
                json!({"kind": "rectangle", "points": [[0.1 + o, 0.1 + o], [0.9 - o, 0.9 - o]]})
            })
            .collect();
        let id = ok(
            &mut s,
            "markup.create",
            json!({"viewpoint": view, "marks": list}),
        )["id"]
            .clone();
        // The first call builds the triangles of every model, the second one finds them cut.
        for round in ["first call", "second call"] {
            let t = Instant::now();
            let got = ok(
                &mut s,
                "markup.get",
                json!({"id": id, "render": false, "samples": samples}),
            );
            let used: u64 = got["marks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["under"]["samples"].as_u64().unwrap_or(0))
                .sum();
            println!(
                "{marks} rectangles, {samples} samples each, {round}: {:?} for {used} samples (truncated: {})",
                t.elapsed(),
                got["truncated"] == true
            );
        }
    }
    let id = ok(&mut s, "markup.create", json!({"viewpoint": view, "marks": [{"kind": "arrow", "points": [[0.9, 0.9], [0.5, 0.5]]}]}))["id"].clone();
    let t = Instant::now();
    ok(&mut s, "markup.get", json!({"id": id, "width": 800}));
    println!("one arrow with both pictures (800 px): {:?}", t.elapsed());
}
