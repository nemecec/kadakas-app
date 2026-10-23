//! A batch of many `AddObject` and `SetData` pairs (what an import makes) runs in near-linear
//! time. The timing test is ignored by default, because time depends on the machine:
//! `cargo test -p kadakas-core --test batch_speed -- --ignored --nocapture`.

use std::collections::BTreeMap;
use std::time::Instant;

use glam::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Attributes, Shape};

fn batch(n: usize) -> Command {
    let mut commands = Vec::with_capacity(2 * n);
    let mut ids = Editor::with_sequential_ids().doc().id_plan();
    for i in 0..n {
        commands.push(Command::AddObject {
            shape: Shape::Circle {
                center: DVec2::new(i as f64, 0.0),
                radius: 1.0,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        });
        let id = kadakas_core::model::ObjectId(ids.take());
        commands.push(Command::SetData {
            id,
            set: BTreeMap::from([("k".to_string(), serde_json::Value::from(i))]),
            remove: vec![],
        });
    }
    Command::Batch { commands }
}

fn run(n: usize) -> f64 {
    let cmd = batch(n);
    let mut ed = Editor::with_sequential_ids();
    let t = Instant::now();
    ed.execute(&cmd).unwrap();
    assert_eq!(ed.doc().objects.len(), n);
    t.elapsed().as_secs_f64()
}

#[test]
fn the_data_of_a_small_batch_is_set() {
    let cmd = batch(50);
    let mut ed = Editor::with_sequential_ids();
    ed.execute(&cmd).unwrap();
    let last = ed.doc().objects.values().last().unwrap();
    assert_eq!(last.data["k"], 49);
    assert!(ed.undo());
    assert!(ed.doc().objects.is_empty());
}

#[test]
#[ignore = "timing: run by hand"]
fn forty_thousand_pairs_run_in_near_linear_time() {
    let (small, big) = (run(10_000), run(40_000));
    eprintln!("10k: {small:.2}s, 40k: {big:.2}s");
    // Linear is 4 times, quadratic 16 times.
    assert!(big < small * 9.0, "{small} -> {big}");
}
