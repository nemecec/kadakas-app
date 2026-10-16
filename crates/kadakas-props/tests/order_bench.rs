//! The time that one edit takes to find its stacking keys in a large document (`docsync`).
//!
//! Not a check: run it by hand, in release, and read the numbers.
//!
//!     cargo test --release -p kadakas-props --test order_bench -- --ignored --nocapture

use std::time::{Duration, Instant};

use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::uid::Uid;
use kadakas_props::docsync::{ops_from_change, replica_of, shadow_of};
use kadakas_props::op::Writer;
use serde_json::json;

const OBJECTS: u64 = 200_000;
const LAYERS: usize = 4;
const EDITS: usize = 50;

/// A document with `OBJECTS` rectangles over `LAYERS` layers, in turn, with stacking keys (built
/// from a replica, as a client builds it).
fn large_document() -> Document {
    let mut ed = Editor::with_sequential_ids();
    for n in 1..LAYERS {
        let cmd: Command =
            serde_json::from_value(json!({"op": "add_layer", "name": format!("L{n}")})).unwrap();
        ed.execute(&cmd).unwrap();
    }
    let cmd: Command = serde_json::from_value(json!({"op": "add_object",
        "shape": {"kind": "rect", "min": [0, 0], "max": [10, 10]}}))
    .unwrap();
    let first = ObjectId(ed.execute(&cmd).unwrap().created[0]);
    let mut doc = ed.doc().clone();
    let template = doc.objects.shift_remove(&first).unwrap();
    let layers: Vec<_> = doc.layers.iter().map(|l| l.id).collect();
    for i in 0..OBJECTS {
        let mut o = template.clone();
        o.id = ObjectId(Uid::from_num(1_000_000 + i));
        o.layer = layers[i as usize % LAYERS];
        doc.objects.insert(o.id, o);
    }
    kadakas_props::materialize::materialize(&replica_of(&doc), &doc).doc
}

/// Runs `EDITS` edits made by `cmd` (given the edit number and the IDs in stacking order) and
/// returns the mean time of `ops_from_change` and the mean number of operations.
fn measure(
    name: &str,
    ed: &mut Editor,
    shadow: &mut kadakas_props::replica::Replica,
    cmd: impl Fn(usize, &[ObjectId]) -> serde_json::Value,
) {
    let mut total = Duration::ZERO;
    let mut ops_count = 0;
    for n in 0..EDITS {
        let ids: Vec<ObjectId> = ed.doc().objects.keys().copied().collect();
        let rev = ed.revision();
        let c: Command = serde_json::from_value(cmd(n, &ids)).unwrap();
        ed.execute(&c).unwrap();
        let cs = ed.changes_since(rev).changes.last().cloned().unwrap();
        let t = Instant::now();
        let ops = ops_from_change(shadow, ed.doc(), cs);
        total += t.elapsed();
        ops_count += ops.len();
        shadow.apply_all(Writer::BASELINE, &ops);
    }
    println!(
        "{name:<28} {:>10.3} ms per edit, {:.1} operations per edit",
        total.as_secs_f64() * 1000.0 / EDITS as f64,
        ops_count as f64 / EDITS as f64
    );
}

/// Like [`measure`], with two commands in one push: both change sets are compared with the final
/// document, as a client does after several edits between two syncs.
fn measure_push(
    name: &str,
    ed: &mut Editor,
    shadow: &mut kadakas_props::replica::Replica,
    cmds: impl Fn(usize, &[ObjectId]) -> [serde_json::Value; 2],
) {
    let mut total = Duration::ZERO;
    let mut ops_count = 0;
    for n in 0..EDITS {
        let ids: Vec<ObjectId> = ed.doc().objects.keys().copied().collect();
        let rev = ed.revision();
        for c in cmds(n, &ids) {
            ed.execute(&serde_json::from_value::<Command>(c).unwrap())
                .unwrap();
        }
        let changes: Vec<_> = ed.changes_since(rev).changes.into_iter().cloned().collect();
        let t = Instant::now();
        for cs in &changes {
            let ops = ops_from_change(shadow, ed.doc(), cs);
            ops_count += ops.len();
            shadow.apply_all(Writer::BASELINE, &ops);
        }
        total += t.elapsed();
    }
    println!(
        "{name:<28} {:>10.3} ms per push, {:.1} operations per push",
        total.as_secs_f64() * 1000.0 / EDITS as f64,
        ops_count as f64 / EDITS as f64
    );
}

#[test]
#[ignore = "a benchmark: run it by hand in release"]
fn time_per_edit_in_a_large_document() {
    let doc = large_document();
    let mut shadow = shadow_of(&doc);
    let mut ed = Editor::new(doc);
    ed.set_history_limit(0);
    println!("{OBJECTS} objects on {LAYERS} layers, {EDITS} edits each");
    measure(
        "arrange forward",
        &mut ed,
        &mut shadow,
        |n, ids| json!({"op": "arrange", "ids": [ids[1000 + n * 997].0], "how": "forward"}),
    );
    measure(
        "arrange to the front",
        &mut ed,
        &mut shadow,
        |n, ids| json!({"op": "arrange", "ids": [ids[2000 + n * 991].0], "how": "front"}),
    );
    measure(
        "move an object",
        &mut ed,
        &mut shadow,
        |n, ids| json!({"op": "move_objects", "ids": [ids[3000 + n * 983].0], "delta": [10, 5]}),
    );
    measure(
        "add an object",
        &mut ed,
        &mut shadow,
        |_, _| json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [5, 5]}}),
    );
    let rect = json!({"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [5, 5]}});
    measure_push("add two objects, one push", &mut ed, &mut shadow, |_, _| {
        [rect.clone(), rect.clone()]
    });
    measure_push(
        "arrange and add, one push",
        &mut ed,
        &mut shadow,
        |n, ids| {
            [
                json!({"op": "arrange", "ids": [ids[4000 + n * 977].0], "how": "forward"}),
                rect.clone(),
            ]
        },
    );
}
