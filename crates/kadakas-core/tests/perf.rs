//! Speed checks for big drawings. They are slow in debug builds, so run:
//! `cargo test -p kadakas-core --release --test perf -- --ignored --nocapture`

use std::time::{Duration, Instant};

use kadakas_core::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::{Attributes, ObjectId, Shape};

const N: usize = 50_000;
const LIMIT: Duration = Duration::from_millis(200);

fn add_all() -> Command {
    Command::Batch {
        commands: (0..N)
            .map(|i| Command::AddObject {
                shape: Shape::Line {
                    a: DVec2::new(i as f64, 0.0),
                    b: DVec2::new(i as f64, 10.0),
                },
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
    assert_eq!(ed.doc().objects.len(), N);
    timed("undo add", || ed.undo());
    assert!(ed.doc().objects.is_empty());
    timed("redo add", || ed.redo());
    assert_eq!(ed.doc().objects.len(), N);
}

#[test]
#[ignore]
fn move_50k_objects_in_one_command() {
    let mut ed = Editor::default();
    ed.execute(&add_all()).unwrap();
    let ids: Vec<ObjectId> = ed.doc().objects.keys().copied().collect();
    let mv = Command::MoveObjects {
        ids: ids.clone(),
        delta: DVec2::new(5.0, 5.0),
    };
    timed("move 50k", || ed.execute(&mv).unwrap());
    timed("undo move", || ed.undo());
    timed("redo move", || ed.redo());
    let del = Command::DeleteObjects { ids };
    timed("delete 50k", || ed.execute(&del).unwrap());
    timed("undo delete", || ed.undo());
    assert_eq!(ed.doc().objects.len(), N);
}

/// 5000 plants and 1000 hardscapes, some of them concave with curved corners.
#[test]
#[ignore]
fn mesh_build_5k_plants_and_1k_hardscapes() {
    use kadakas_core::curve::{self, Vertex, VertexKind};
    use kadakas_core::mesh::{MeshCache, build};

    let mut cmds = Vec::new();
    for i in 0..5000 {
        let (x, y) = ((i % 100) as f64 * 3000.0, (i / 100) as f64 * 3000.0);
        cmds.push(Command::AddObject {
            shape: Shape::Plant {
                at: DVec2::new(x, y),
                species: format!("Species {}", i % 40),
                spread: 2500.0,
                height: 6000.0,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        });
    }
    for i in 0..1000 {
        let (x, y) = ((i % 50) as f64 * 6000.0, -8000.0 - (i / 50) as f64 * 6000.0);
        let o = DVec2::new(x, y);
        // An L shape with one rounded corner.
        let mut boundary = curve::corners(&[
            o,
            o + DVec2::new(4000.0, 0.0),
            o + DVec2::new(4000.0, 1500.0),
            o + DVec2::new(1500.0, 1500.0),
            o + DVec2::new(1500.0, 4000.0),
            o + DVec2::new(0.0, 4000.0),
        ]);
        boundary[2] = Vertex::new(
            o + DVec2::new(4000.0, 1500.0),
            VertexKind::Arc { radius: 300.0 },
        );
        cmds.push(Command::AddObject {
            shape: Shape::Hardscape {
                boundary,
                material: "Setts".into(),
                thickness: None,
                elevation: None,
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        });
    }
    let mut ed = Editor::default();
    ed.execute(&Command::Batch { commands: cmds }).unwrap();

    let t = Instant::now();
    let scene = build(ed.doc());
    let d = t.elapsed();
    eprintln!(
        "mesh build: {d:?} ({} triangles, {} vertices, {} line segments)",
        scene.mesh.triangle_count(),
        scene.mesh.positions.len(),
        scene.lines.segment_count()
    );
    assert!(d < Duration::from_millis(300), "mesh build took {d:?}");

    // The cache makes repeat calls free.
    let mut cache = MeshCache::default();
    cache.get(ed.revision(), ed.doc());
    let t = Instant::now();
    cache.get(ed.revision(), ed.doc());
    assert!(t.elapsed() < Duration::from_millis(1));
    assert_eq!(cache.builds, 1);
}
