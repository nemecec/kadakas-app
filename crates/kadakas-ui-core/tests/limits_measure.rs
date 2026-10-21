//! Measurement of the 3D inference scene of component instances (MOD-07) for
//! `docs/modeler-limits.md`. Same driver as `kadakas-core/tests/limits_measure.rs`
//! (`LM_TEST_NAME=scenario`, `limits_measure.py BINARY RUNS SCENARIO "ARGS"`).
#![allow(unused)]

#[path = "../../kadakas-core/tests/limits_common.rs"]
mod limits_common;

use limits_common::*;

use glam::{DAffine3, DVec2, DVec3};
use kadakas_core::command::{Command, Editor};
use kadakas_core::component::{self, Part};
use kadakas_core::model::*;
use kadakas_ui_core::infer3d::{
    Camera, DefinitionCache, Inferencer, Projection, scene_of_instances, scene_of_instances_cached,
};

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

fn cam() -> Camera {
    Camera::look_at(
        DVec3::new(0.0, -10000.0, 6000.0),
        DVec3::new(1000.0, 0.0, 0.0),
        DVec3::Z,
        Projection::Perspective { fov_y: 0.8 },
        DVec2::new(1000.0, 800.0),
    )
    .unwrap()
}

/// The scene of `instances` instances of a definition of `parts` parts, and one inference.
fn infer() {
    let a = args();
    let (kind, parts, count) = (a[0].as_str(), num(&a, 1), num(&a, 2));
    if parts * count > 500_000 {
        result(
            "infer",
            &format!(
                "kind={kind} parts={parts} instances={count} SKIPPED total_models={} (the scene holds a copy per instance)",
                parts * count
            ),
        );
        return;
    }
    let (ed, sym, ids) = world(&kind, parts, count);
    let rss0 = rss();
    let (r, t) = timed(|| scene_of_instances(ed.doc(), &ids, 0));
    let r = r.expect("scene");
    let rss1 = rss();
    let (edges, faces) = (r.scene.edge_count(), r.scene.face_count());
    let mut cache = DefinitionCache::default();
    let (r2, tcached) = timed(|| scene_of_instances_cached(ed.doc(), &ids, 0, &mut cache));
    let r2 = r2.unwrap();
    let (_, tcached2) =
        timed(|| scene_of_instances_cached(ed.doc(), &ids, 0, &mut cache).map(|s| s.placed.len()));
    let c = cam();
    let mut inf = Inferencer::default();
    let px = c.project(DVec3::new(0.0, 0.0, 0.0)).unwrap();
    let (first, tinf1) = timed(|| inf.infer(&r.scene, &c, px, None));
    let mut times = Vec::new();
    for k in 0..20 {
        let p = px + DVec2::new(k as f64, 0.0);
        let (_, t) = timed(|| inf.infer(&r.scene, &c, p, None));
        times.push(t.wall);
    }
    times.sort_by(|a, b| a.total_cmp(b));
    result(
        "infer",
        &format!(
            "kind={kind} parts={parts} instances={count} scene_edges={edges} scene_faces={faces} build_{} cached_build_wall_s={:.4} cached_rebuild_wall_s={:.4} rss_before_mb={} rss_after_build_mb={} first_infer_wall_s={:.5} median_infer_wall_s={:.5} kind_of_first={:?}",
            fmt_took(&t),
            tcached.wall,
            tcached2.wall,
            mb(rss0),
            mb(rss1),
            tinf1.wall,
            times[times.len() / 2],
            first.kind
        ),
    );
}

/// The scene of a chain of 16 definitions (the deepest allowed), on a thread with a given stack.
fn deep(stack_kib: usize) {
    let run = move || {
        println!("STEP build 16-deep chain");
        let mut doc = Document::with_sequential_ids();
        let mut def = push_def(&mut doc, "D0", def_parts("sq", 1));
        for lvl in 1..component::MAX_COMPONENT_DEPTH {
            def = push_def(
                &mut doc,
                &format!("D{lvl}"),
                vec![Part::new(instance_shape(def, 0))],
            );
        }
        let ids = push_instances(&mut doc, def, 1);
        println!("STEP scene");
        let r = scene_of_instances(&doc, &ids, 0).unwrap();
        println!("STEP infer");
        let c = cam();
        let mut inf = Inferencer::default();
        let px = c.project(DVec3::new(0.0, 0.0, 0.0)).unwrap();
        inf.infer(&r.scene, &c, px, None);
        println!("STEP done");
    };
    if stack_kib == 0 {
        run();
    } else {
        std::thread::Builder::new()
            .stack_size(stack_kib * 1024)
            .spawn(run)
            .unwrap()
            .join()
            .unwrap();
    }
    result("stack", &format!("stack_kib={stack_kib} survived=true"));
}

/// The scene of the widest allowed expansion: two instances of a 10,000-part definition.
fn wide() {
    let a = args();
    let kind = a[0].as_str();
    let mut doc = Document::with_sequential_ids();
    let big = push_def(&mut doc, "Big", def_parts(kind, component::MAX_PARTS));
    let inst = |k: usize| Part::new(instance_shape(big, k));
    let two = push_def(&mut doc, "Two", vec![inst(0), inst(1)]);
    let ids = push_instances(&mut doc, two, 1);
    let rss0 = rss();
    let (r, t) = timed(|| scene_of_instances(&doc, &ids, 0));
    let r = r.expect("scene");
    let c = cam();
    let mut inf = Inferencer::default();
    let px = c.project(DVec3::new(0.0, 0.0, 0.0)).unwrap();
    let (_, ti) = timed(|| inf.infer(&r.scene, &c, px, None));
    result(
        "wide",
        &format!(
            "kind={kind} models=20000 scene_edges={} scene_faces={} build_{} infer_wall_s={:.5} rss_before_mb={} rss_after_mb={}",
            r.scene.edge_count(),
            r.scene.face_count(),
            fmt_took(&t),
            ti.wall,
            mb(rss0),
            mb(rss())
        ),
    );
}

#[test]
#[ignore = "measurement: run in release mode"]
fn scenario() {
    match std::env::var("LM").expect("set LM").as_str() {
        "infer" => infer(),
        "deep" => deep(num(&args(), 0)),
        "wide" => wide(),
        other => panic!("unknown scenario {other}"),
    }
    println!("MAXRSS_MB {}", mb(max_rss()));
}
