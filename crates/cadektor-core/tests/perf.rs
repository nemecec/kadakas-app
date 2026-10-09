//! Speed checks for big drawings. They are slow in debug builds, so run:
//! `cargo test -p cadektor-core --release --test perf -- --ignored --nocapture`

use std::time::{Duration, Instant};

use cadektor_core::command::{Command, Editor};
use cadektor_core::model::{Attributes, ObjectId, Shape};
use cadektor_core::DVec2;

const N: usize = 50_000;
const LIMIT: Duration = Duration::from_millis(200);

fn add_all() -> Command {
    Command::Batch {
        commands: (0..N)
            .map(|i| Command::AddObject {
                shape: Shape::Line { a: DVec2::new(i as f64, 0.0), b: DVec2::new(i as f64, 10.0) },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .collect(),
    }
}

fn timed<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let r = f();
    let d = t.elapsed();
    eprintln!("{what}: {d:?}");
    assert!(d < LIMIT, "{what} took {d:?}");
    r
}

#[test]
#[ignore]
fn add_50k_objects_in_one_batch() {
    let cmd = add_all();
    let mut ed = Editor::default();
    timed("add 50k", || ed.execute(&cmd).unwrap());
    assert_eq!(ed.doc.objects.len(), N);
    timed("undo add", || ed.undo());
    assert!(ed.doc.objects.is_empty());
    timed("redo add", || ed.redo());
    assert_eq!(ed.doc.objects.len(), N);
}

#[test]
#[ignore]
fn move_50k_objects_in_one_command() {
    let mut ed = Editor::default();
    ed.execute(&add_all()).unwrap();
    let ids: Vec<ObjectId> = ed.doc.objects.keys().copied().collect();
    let mv = Command::MoveObjects { ids: ids.clone(), delta: DVec2::new(5.0, 5.0) };
    timed("move 50k", || ed.execute(&mv).unwrap());
    timed("undo move", || ed.undo());
    timed("redo move", || ed.redo());
    let del = Command::DeleteObjects { ids };
    timed("delete 50k", || ed.execute(&del).unwrap());
    timed("undo delete", || ed.undo());
    assert_eq!(ed.doc.objects.len(), N);
}
