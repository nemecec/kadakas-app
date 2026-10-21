//! Size of the collaboration updates (operations) after the commands of the modeler, for
//! `docs/modeler-limits.md`. Same driver as `kadakas-core/tests/limits_measure.rs`
//! (`LM_TEST_NAME=scenario`, `limits_measure.py BINARY RUNS SCENARIO "ARGS"`).
#![allow(unused)]

#[path = "../../kadakas-core/tests/limits_common.rs"]
mod limits_common;

use limits_common::*;

use glam::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::*;
use kadakas_core::surface::Edit;
use kadakas_props::docsync::{ops_from_change, replica_of, split};
use kadakas_props::op::{self, Op};
use serde_json::Value;

#[global_allocator]
static ALLOC: Counting = Counting;

fn args() -> Vec<String> {
    std::env::var("LM_ARGS")
        .unwrap_or_default()
        .split_whitespace()
        .map(String::from)
        .collect()
}

fn num(a: &[String], i: usize) -> usize {
    a[i].parse().unwrap()
}

/// What the other users receive for one command: the operations of its change set.
fn report(label: &str, extra: &str, ed: &Editor, view: &kadakas_props::Replica, rev: u64) {
    let cs = ed
        .changes_since(rev)
        .changes
        .last()
        .cloned()
        .cloned()
        .expect("a change set");
    let (ops, t) = timed(|| ops_from_change(view, ed.doc(), &cs));
    let json = serde_json::to_vec(&ops).unwrap();
    let mut biggest: (usize, String) = (0, String::new());
    let mut by_kind = std::collections::BTreeMap::<String, (usize, usize)>::new();
    for o in &ops {
        let mut note = |prop: &str, v: &Value| {
            let n = serde_json::to_vec(v).unwrap().len();
            if n > biggest.0 {
                biggest = (n, format!("{:?}.{prop}", o.target().kind));
            }
            let e = by_kind.entry(format!("{:?}", o.target().kind)).or_default();
            e.1 += n;
        };
        match o {
            Op::Create { props, .. } => props.iter().for_each(|(k, v)| note(k, v)),
            Op::Set { prop, value, .. } => note(prop, value),
            _ => {}
        }
        by_kind
            .entry(format!("{:?}", o.target().kind))
            .or_default()
            .0 += 1;
    }
    let txns = split(ops.clone());
    let valid = op::validate_ops(&ops)
        .map(|_| "ok".to_string())
        .unwrap_or_else(|e| format!("REFUSED({e})").replace(' ', "_"));
    let kinds: Vec<String> = by_kind
        .iter()
        .map(|(k, (n, b))| format!("{k}:{n}ops/{:.2}MB", *b as f64 / 1048576.0))
        .collect();
    result(
        "collab",
        &format!(
            "{label} {extra} ops={} update_json_mb={} largest_value_mb={} largest_value={} transactions={} validate={valid} by_kind={} derive_{}",
            ops.len(),
            mb(json.len()),
            mb(biggest.0),
            biggest.1,
            txns.len(),
            kinds.join(","),
            fmt_took(&t)
        ),
    );
}

fn collab() {
    let a = args();
    match a[0].as_str() {
        // Move one instance (a definition of `parts` parts, `count` instances).
        "move_instance" => {
            let (kind, parts, count) = (a[1].as_str(), num(&a, 2), num(&a, 3));
            let (mut ed, _sym, ids) = world(kind, parts, count);
            let view = replica_of(ed.doc());
            let rev = ed.revision();
            ed.execute(&Command::MoveObjects {
                ids: vec![ids[0]],
                delta: DVec2::new(100.0, 50.0),
            })
            .unwrap();
            report(
                "move_instance",
                &format!("kind={kind} parts={parts} instances={count}"),
                &ed,
                &view,
                rev,
            );
        }
        // Edit the definition in place and close it again.
        "close_edit" => {
            let (kind, parts) = (a[1].as_str(), num(&a, 2));
            let (mut ed, _sym, ids) = world(kind, parts, 3);
            let g = ObjectId(
                ed.execute(&Command::EditComponent { id: ids[0] })
                    .unwrap()
                    .created[0],
            );
            // A real change inside: one part moves, so the definition differs when it closes.
            let first = ed.doc().children(g)[0];
            ed.execute(&Command::MoveObjects {
                ids: vec![first],
                delta: DVec2::new(10.0, 10.0),
            })
            .unwrap();
            let view_open = replica_of(ed.doc());
            let rev = ed.revision();
            let r = ed.execute(&Command::CloseComponent {
                group: g,
                resolve: None,
            });
            if let Err(e) = &r {
                result(
                    "collab",
                    &format!("close_edit kind={kind} parts={parts} REFUSED({e})"),
                );
                return;
            }
            report(
                "close_edit",
                &format!("kind={kind} parts={parts}"),
                &ed,
                &view_open,
                rev,
            );
            // The open too (it creates the objects of the edit group in the document).
            let (mut ed, _s, ids) = world(kind, parts, 3);
            let view = replica_of(ed.doc());
            let rev = ed.revision();
            ed.execute(&Command::EditComponent { id: ids[0] }).unwrap();
            report(
                "open_edit",
                &format!("kind={kind} parts={parts}"),
                &ed,
                &view,
                rev,
            );
        }
        // Make a group of 4 faces of a surface model of `shape` `p`.
        "make_group" => {
            let (shape, p) = (a[1].as_str(), num(&a, 2));
            let model = construct(&mesh_of(shape, p));
            let faces: Vec<_> = model.faces().map(|(f, _)| f).take(4).collect();
            let mut ed = Editor::with_sequential_ids();
            let id = ObjectId(
                ed.execute(&Command::AddObject {
                    shape: Shape::SurfaceModel(Box::new(model)),
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                })
                .unwrap()
                .created[0],
            );
            let view = replica_of(ed.doc());
            let rev = ed.revision();
            let r = ed.execute(&Command::MakeGroup {
                model: Some(id),
                faces,
                edges: vec![],
                objects: vec![],
                name: None,
            });
            if let Err(e) = &r {
                result("collab", &format!("make_group {shape} {p} REFUSED({e})"));
                return;
            }
            report(
                "make_group",
                &format!("shape={shape} param={p}"),
                &ed,
                &view,
                rev,
            );
        }
        // One small edit (a box) of a surface model of `shape` `p`: the whole shape is one value.
        "edit_surface" => {
            let (shape, p) = (a[1].as_str(), num(&a, 2));
            let mut model = construct(&mesh_of(shape, p));
            // A log like a model that has been worked on: 64 small edits.
            for k in 0..64 {
                model
                    .apply(&Edit::AddEdges {
                        edges: cube_segs(-5.0e6 - k as f64 * 3000.0, -5.0e6, 1000.0),
                    })
                    .unwrap();
            }
            let mut ed = Editor::with_sequential_ids();
            let id = ObjectId(
                ed.execute(&Command::AddObject {
                    shape: Shape::SurfaceModel(Box::new(model)),
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                })
                .unwrap()
                .created[0],
            );
            let view = replica_of(ed.doc());
            let rev = ed.revision();
            let r = ed.execute(&Command::EditSurface {
                id,
                edit: Edit::AddEdges {
                    edges: cube_segs(-9.0e6, -9.0e6, 1000.0),
                },
            });
            if let Err(e) = &r {
                result("collab", &format!("edit_surface {shape} {p} REFUSED({e})"));
                return;
            }
            report(
                "edit_surface",
                &format!("shape={shape} param={p}"),
                &ed,
                &view,
                rev,
            );
            // The first sync of a new client: the whole document as a replica.
            let (ops, t) = timed(|| {
                let r = replica_of(ed.doc());
                kadakas_props::docsync::diff_replicas(&kadakas_props::Replica::new(), &r)
            });
            let bytes = serde_json::to_vec(&ops).unwrap().len();
            result(
                "collab",
                &format!(
                    "snapshot_ops shape={shape} param={p} ops={} json_mb={} {}",
                    ops.len(),
                    mb(bytes),
                    fmt_took(&t)
                ),
            );
        }
        other => panic!("unknown {other}"),
    }
}

#[test]
#[ignore = "measurement: run in release mode"]
fn scenario() {
    // LM_SCALE multiplies the count limits of the surface model (feature `measure-limits`).
    #[cfg(feature = "measure-limits")]
    if let Some(n) = std::env::var("LM_SCALE").ok().and_then(|s| s.parse().ok()) {
        kadakas_core::surface::set_limit_scale(n);
    }
    match std::env::var("LM").expect("set LM").as_str() {
        "collab" => collab(),
        other => panic!("unknown scenario {other}"),
    }
    println!("MAXRSS_MB {}", mb(max_rss()));
}
