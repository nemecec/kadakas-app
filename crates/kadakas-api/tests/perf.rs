//! Timing tests for the annotation methods. They need a release build, so CI runs them apart:
//! `cargo test -p kadakas-api --release --test perf -- --ignored --nocapture`.
//! The checks of the work done (cache use, counts) are in `annotation.rs`.

use std::time::Instant;

use kadakas_api::Session;
use serde_json::{Value, json};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    match s.handle(m, p) {
        Ok(v) => v,
        Err(e) => panic!("{m} failed: {}", e.message),
    }
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
    call(
        s,
        "execute",
        json!({"command": {"op": "batch", "commands": cmds}}),
    );
}

#[test]
#[ignore = "timing test: run in release mode"]
fn twenty_legends_over_4000_plants_add_little_time_to_an_svg_export() {
    let mut s = Session::with_sequential_ids();
    plants(&mut s, 4000);
    let t = Instant::now();
    call(&mut s, "render.svg", json!({}));
    let plain = t.elapsed();
    for i in 0..20 {
        call(
            &mut s,
            "legend.add",
            json!({"at": [i * 10, 0], "criteria": "Kind = \"plant\"", "group_by": "Style"}),
        );
    }
    let t = Instant::now();
    call(&mut s, "render.svg", json!({}));
    let extra = t.elapsed().saturating_sub(plain);
    assert!(extra.as_millis() < 200, "20 legends added {extra:?}");
    // The second export of the same revision comes from the cache: best of 5 runs.
    let cached = (0..5)
        .map(|_| {
            let t = Instant::now();
            call(&mut s, "render.svg", json!({}));
            t.elapsed()
        })
        .min()
        .unwrap();
    assert!(
        cached.as_millis() < 50 + plain.as_millis(),
        "cached export took {cached:?}"
    );
}

#[test]
#[ignore = "timing test: run in release mode"]
fn tag_all_is_not_quadratic() {
    let mut s = Session::with_sequential_ids();
    plants(&mut s, 3000);
    call(
        &mut s,
        "tag_style.create",
        json!({"name": "T", "lines": ["{IdCode}"]}),
    );
    let t = Instant::now();
    call(
        &mut s,
        "data_tag.tag_all",
        json!({"style": "T", "criteria": "Kind = \"plant\""}),
    );
    assert!(t.elapsed().as_secs() < 2, "tag_all took {:?}", t.elapsed());
}
