//! AGR-10: what a pointer move costs on a big drawing. Run by hand:
//! `cargo test -p kadakas-api --test pointer_cost -- --ignored --nocapture`
//! 200 surface models of about 2,000 triangles each (a grid of 32 x 32 faces), spread over a
//! plain. The pointer moves over the plain with a ray from above.

use std::time::Instant;

use glam::DVec3;
use kadakas_api::{Aim, PixelSize, PointerInput, Session};
use serde_json::{Value, json};

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

fn aim(x: f64, y: f64) -> Aim {
    Aim::Ray {
        origin: DVec3::new(x, y, 5000.0),
        direction: DVec3::new(0.0, 0.0, -1.0),
        pixel: PixelSize {
            constant: 2.0,
            per_distance: 0.0,
        },
    }
}

fn show(s: &mut Session, x: f64, y: f64) {
    s.pointer_new_frame();
    s.set_pointer(PointerInput {
        canvas: [x, y],
        now_ms: 0.0,
        navigating: false,
        focused: true,
        aim: aim(x, y),
    });
}

#[test]
#[ignore]
fn the_cost_of_a_pointer_move_on_200_models_of_2000_triangles() {
    let mut s = Session::with_sequential_ids();
    let edges = grid(32, 10.0);
    let t = Instant::now();
    let mut ids = Vec::new();
    for k in 0..200 {
        let id = ok(&mut s, "mesh.create", json!({"name": format!("M{k}")}))["id"].clone();
        ok(&mut s, "mesh.add_edges", json!({"id": id, "edges": edges}));
        // Place the models in a row of 20 by 10 on the plain.
        ok(
            &mut s,
            "execute",
            json!({"command": {"op": "move_objects", "ids": [id], "delta": [(k % 20) as f64 * 400.0, (k / 20) as f64 * 400.0]}}),
        );
        ids.push(id);
    }
    println!("built 200 models in {:?}", t.elapsed());
    // 1. Pointer moves over a model, nothing else changes.
    show(&mut s, 5.0, 5.0);
    let _ = s.pointer_hit_point();
    let n = 50;
    let t = Instant::now();
    for i in 0..n {
        show(&mut s, 20.0 + i as f64 * 3.0, 20.0 + (i % 7) as f64 * 3.0);
        assert!(s.pointer_hit_point().is_some());
    }
    let per_move = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    println!("pointer move, same drawing: {per_move:.2} ms per move");
    // 2. A move over empty plain (no hit): every triangle is tried.
    let t = Instant::now();
    for i in 0..n {
        show(&mut s, 20.0 + i as f64 * 3.0, -500.0);
        assert!(s.pointer_hit_point().is_none());
    }
    let per_miss = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    println!("pointer move, no hit: {per_miss:.2} ms per move");
    // 3. One model is edited: the next pick.
    let t = Instant::now();
    let m = 5;
    for i in 0..10 {
        ok(
            &mut s,
            "execute",
            json!({"command": {"op": "move_objects", "ids": [ids[m]], "delta": [1.0, 0.0]}}),
        );
        show(&mut s, 20.0 + i as f64, 20.0);
        let _ = s.pointer_hit_point();
    }
    let after_edit = t.elapsed().as_secs_f64() * 1000.0 / 10.0;
    println!("edit of one model and the next pick: {after_edit:.2} ms each (includes the edit)");
    // 4. The API answer.
    let t = Instant::now();
    for i in 0..n {
        show(&mut s, 20.0 + i as f64 * 3.0, 20.0);
        let _ = s.pointer_json(0.0);
    }
    println!(
        "pointer_json: {:.2} ms per answer",
        t.elapsed().as_secs_f64() * 1000.0 / n as f64
    );
}
