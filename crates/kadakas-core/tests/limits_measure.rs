//! Measurement of the limits of the modeler (surface models MOD-06, groups and components
//! MOD-07). Not a test of behavior: every scenario is one `#[ignore]` run of `scenario`, chosen by
//! the environment variable `LM` (arguments in `LM_ARGS`, separated by spaces), and prints
//! `RESULT ...` lines. `tests/limits_measure.py` runs the scenarios, each in its own process under
//! `/usr/bin/time -l`, several times, and takes the median. See `docs/modeler-limits.md`.
//!
//! Run (release build only; a debug build says nothing):
//! `LM=shape LM_ARGS="terrain 158" cargo test --release -p kadakas-core --test limits_measure -- --ignored --exact scenario --nocapture`

#![allow(unused)]

#[path = "limits_common.rs"]
mod limits_common;

use limits_common::*;

use std::collections::BTreeMap;

use glam::{DAffine3, DVec3};
use kadakas_core::command::{Command, Editor};
use kadakas_core::component::{self, Part};
use kadakas_core::graphics::{GraphicResource, SymbolDef};
use kadakas_core::mesh;
use kadakas_core::model::*;
use kadakas_core::resource::{Resource, ResourceBody, ResourceId};
use kadakas_core::surface::{
    Edit, MAX_EDGES, MAX_EDIT_EDGES, MAX_FACES, MAX_LOOP_VERTICES, MAX_VERTICES, SurfaceModel,
};

#[global_allocator]
static ALLOC: Counting = Counting;

fn tmp() -> String {
    std::env::var("LM_TMP").unwrap_or_else(|_| "/tmp".into())
}

fn args() -> Vec<String> {
    std::env::var("LM_ARGS")
        .unwrap_or_default()
        .split_whitespace()
        .map(String::from)
        .collect()
}

fn num(a: &[String], i: usize) -> usize {
    a.get(i)
        .unwrap_or_else(|| panic!("argument {i} missing"))
        .parse()
        .unwrap_or_else(|_| panic!("argument {i} is not a number"))
}

fn counts(m: &SurfaceModel) -> String {
    let loops: usize = m.faces().map(|(_, f)| f.loop_len()).sum();
    format!(
        "V={} E={} F={} L={}",
        m.vertex_count(),
        m.edge_count(),
        m.face_count(),
        loops
    )
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// Applies the edits one after the other. Stops at the first error.
struct Built {
    m: SurfaceModel,
    done: usize,
    total: usize,
    err: Option<String>,
    walls: Vec<f64>,
    cpus: Vec<f64>,
}

fn build_on(mut m: SurfaceModel, batches: Vec<Vec<Seg>>) -> Built {
    let total = batches.len();
    let mut b = Built {
        m: SurfaceModel::new(),
        done: 0,
        total,
        err: None,
        walls: Vec::new(),
        cpus: Vec::new(),
    };
    for edges in batches {
        let (r, t) = timed(|| m.apply(&Edit::AddEdges { edges }));
        b.walls.push(t.wall);
        b.cpus.push(t.cpu);
        match r {
            Ok(()) => b.done += 1,
            Err(e) => {
                b.err = Some(e.to_string());
                break;
            }
        }
    }
    b.m = m;
    b
}

fn build(batches: Vec<Vec<Seg>>) -> Built {
    build_on(SurfaceModel::new(), batches)
}

fn shape_batches(shape: &str, p: usize, edit_edges: usize) -> Vec<Vec<Seg>> {
    let mesh = mesh_of(shape, p);
    if shape == "boxes" {
        // One box per edit, as a person draws them.
        return mesh
            .f
            .chunks(6)
            .enumerate()
            .map(|(k, _)| cube_segs((k % 100) as f64 * 2000.0, (k / 100) as f64 * 2000.0, 1000.0))
            .collect();
    }
    chunks(mesh.segs(), edit_edges)
}

/// A1: the largest model of a shape that fits all limits.
fn shape() {
    let a = args();
    let (shape, p) = (a[0].as_str(), num(&a, 1));
    let per_edit = a.get(2).map_or(MAX_EDIT_EDGES, |s| s.parse().unwrap());
    let batches = shape_batches(shape, p, per_edit);
    let (b, t) = timed(|| build(batches));
    let mut last: Vec<f64> = b.walls.iter().rev().take(10).copied().collect();
    let mut lastc: Vec<f64> = b.cpus.iter().rev().take(10).copied().collect();
    let maxw = b.walls.iter().cloned().fold(0.0, f64::max);
    let verdict = match &b.err {
        None => "fits".to_string(),
        Some(e) => format!("STOPPED({e})"),
    };
    result(
        "shape",
        &format!(
            "shape={shape} param={p} edits={}/{} {} total_wall_s={:.2} total_cpu_s={:.2} median_last_edit_wall_s={:.3} median_last_edit_cpu_s={:.3} max_edit_wall_s={:.3} peak_heap_mb={:.0} verdict={}",
            b.done,
            b.total,
            counts(&b.m),
            t.wall,
            t.cpu,
            median(&mut last),
            median(&mut lastc),
            maxw,
            t.peak as f64 / 1048576.0,
            verdict.replace(' ', "_")
        ),
    );
    // Cache a model that fits, for the save and collaboration measurements.
    if b.err.is_none() && std::env::var("LM_CACHE").is_ok() {
        let path = format!("{}/lm-{shape}{p}.json", tmp());
        std::fs::write(path, serde_json::to_string(&b.m).unwrap()).unwrap();
    }
}

/// A1b: a model of the shape built from its parts (not drawn): do the limits accept it, and which
/// limit stops it? Also the time of the check of each command and of the full check.
fn limit() {
    let a = args();
    let (shape, p) = (a[0].as_str(), num(&a, 1));
    let (m, tbuild) = timed(|| construct(&mesh_of(shape, p)));
    let (problem, tq) = timed(|| m.problem());
    let c = counts(&m);
    let (rep, tfull) = timed(|| m.check());
    let (text, tsave) = timed(|| serde_json::to_string(&m).unwrap());
    let (back, tload) = timed(|| serde_json::from_str::<SurfaceModel>(&text));
    result(
        "limit",
        &format!(
            "shape={shape} param={p} {c} verdict={} quick_check_{} full_check_{} full_check_problems={} saved_mb={} save_wall_s={:.3} load_{} load_ok={}",
            problem
                .clone()
                .unwrap_or_else(|| "fits".into())
                .replace(' ', "_"),
            fmt_took(&tq),
            fmt_took(&tfull),
            rep.problems.len(),
            mb(text.len()),
            tsave.wall,
            fmt_took(&tload),
            back.is_ok()
        ),
    );
}

/// The base model of 80,000 edges (a 200 x 200 grid: 80,400 edges), from a cache file.
fn base80k() -> SurfaceModel {
    let path = format!("{}/lm-base80k.json", tmp());
    if let Ok(text) = std::fs::read_to_string(&path) {
        return serde_json::from_str(&text).unwrap();
    }
    // Long lines that cross (as the existing perf test does): 402 segments, 80,400 edges.
    let mut lines = Vec::new();
    for i in 0..=200 {
        let t = i as f64 * 1000.0;
        lines.push([[t, 0.0, 0.0], [t, 200_000.0, 0.0]]);
        lines.push([[0.0, t, 0.0], [200_000.0, t, 0.0]]);
    }
    let b = build(vec![lines]);
    assert!(b.err.is_none(), "{:?}", b.err);
    std::fs::write(&path, serde_json::to_string(&b.m).unwrap()).unwrap();
    b.m
}

/// The cells per side of a patch with about `k` edges.
fn side_for(k: usize, per_cell: f64) -> usize {
    // grid: 2m^2 + 2m edges; terrain: 3m^2 + 2m edges.
    // The largest patch with at most k edges.
    let mut m = 1;
    while ((per_cell * ((m + 1) * (m + 1)) as f64) + 2.0 * (m + 1) as f64) <= k as f64 {
        m += 1;
    }
    m
}

/// A2: one edit of about `k` edges on an empty model or on the 80k model.
fn sweep() {
    let a = args();
    let (base, kind, k) = (a[0].as_str(), a[1].as_str(), num(&a, 2));
    let mut m = if base == "80k" {
        base80k()
    } else {
        SurfaceModel::new()
    };
    let base_edges = m.edge_count();
    let mut ed = Editor::with_sequential_ids();
    let mut id = None;
    // The patch that the edit adds (for the add kinds).
    let patch: Vec<Seg> = match kind {
        // A grid patch that shares its left side with the big grid (the same plane, welded).
        "add_same" => grid_segs(
            side_for(k, 2.0),
            if base == "80k" { 200_000.0 } else { 0.0 },
            0.0,
            0.0,
        ),
        // The same patch in another plane (z = 5 m), not touching the big grid.
        "add_other" => grid_segs(side_for(k, 2.0), 0.0, 0.0, 5000.0),
        // A triangulated terrain patch, in many planes.
        "add_tri" => terrain_segs(
            side_for(k, 3.0),
            if base == "80k" { 200_000.0 } else { 0.0 },
            0.0,
            0.0,
        ),
        // Separate far-apart segments, no faces.
        "add_lines" => (0..k)
            .map(|i| {
                let x = (i % 300) as f64 * 5000.0;
                let y = (i / 300) as f64 * 5000.0 + 1.0e6;
                [[x, y, 0.0], [x + 1000.0, y + 700.0, 300.0]]
            })
            .collect(),
        "erase" | "noop" => Vec::new(),
        other => panic!("unknown kind {other}"),
    };
    let patch_edges = patch.len();
    // The model of an erase on an empty model is a patch of about k edges.
    if kind == "erase" && base != "80k" {
        // A model that is just the patch (built from its parts, as a loaded file would be).
        m = construct(&grid_mesh(side_for(k, 2.0), 0.0, 0.0, 0.0));
    }
    let before = counts(&m);
    // The edit on the bare model.
    let edit = if kind == "erase" {
        let ids: Vec<_> = m.edges().map(|(e, _)| e).take(k).collect();
        Edit::EraseEdges { edges: ids }
    } else {
        Edit::AddEdges {
            edges: patch.clone(),
        }
    };
    let setup_heap = heap();
    let rss0 = rss();
    // Raw: SurfaceModel::apply on a clone (so that the model for the command stays as it was).
    // (Only for small edits: a big one takes minutes and the editor run below measures it too.)
    let (raw_res, traw) = if k <= 2000 {
        let mut raw = m.clone();
        let (r, traw) = timed(|| raw.apply(&edit));
        (
            match &r {
                Ok(()) => format!("ok after: {}", counts(&raw)),
                Err(e) => format!("ERR({e})"),
            },
            traw,
        )
    } else {
        ("skipped".to_string(), timed(|| ()).1)
    };
    // Through the editor (the undo step, the check of each command).
    let m_edges = m.edge_count();
    let mut doc_m = Some(m);
    let (ed_res, tcmd) = if let Some(model) = doc_m.take() {
        let out = ed
            .execute(&Command::AddObject {
                shape: Shape::SurfaceModel(Box::new(model)),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        id = Some(ObjectId(out.created[0]));
        let rss1 = rss();
        let before_heap = heap();
        let (r, t) = timed(|| {
            ed.execute(&Command::EditSurface {
                id: id.unwrap(),
                edit: edit.clone(),
            })
        });
        let _ = (rss1, before_heap);
        (r.map(|_| ()).map_err(|e| e.to_string()), t)
    } else {
        unreachable!()
    };
    let (undo_ok, tundo) = timed(|| ed.undo());
    result(
        "sweep",
        &format!(
            "base={base} kind={kind} k={k} base_edges={base_edges} model_edges={m_edges} patch_edges={patch_edges} before=[{before}] raw=[{}] raw_{} cmd={} cmd_{} undo_ok={undo_ok} undo_{} rss_before_mb={} rss_after_mb={} heap_setup_mb={}",
            raw_res.replace(' ', "_"),
            fmt_took(&traw),
            match &ed_res {
                Ok(()) => "ok".to_string(),
                Err(e) => format!("ERR({})", e.replace(' ', "_")),
            },
            fmt_took(&tcmd),
            fmt_took(&tundo),
            mb(rss0),
            mb(rss()),
            mb(setup_heap)
        ),
    );
}

/// A2b: the same edits as several edits of at most 10,000 edges (what a caller must do above the
/// limit), through the editor. Total time.
fn sweep_chunked() {
    let a = args();
    let (base, kind, k) = (a[0].as_str(), a[1].as_str(), num(&a, 2));
    let m = if base == "80k" {
        base80k()
    } else {
        SurfaceModel::new()
    };
    let patch = match kind {
        "add_same" => grid_segs(
            side_for(k, 2.0),
            if base == "80k" { 200_000.0 } else { 0.0 },
            0.0,
            0.0,
        ),
        "add_other" => grid_segs(side_for(k, 2.0), 0.0, 0.0, 5000.0),
        other => panic!("unknown kind {other}"),
    };
    let mut ed = Editor::with_sequential_ids();
    let out = ed
        .execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::new(m)),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    let id = ObjectId(out.created[0]);
    let n = patch.len();
    let (res, t) = timed(|| {
        let mut worst = 0.0f64;
        let mut edits = 0;
        for c in patch.chunks(MAX_EDIT_EDGES) {
            let s = std::time::Instant::now();
            let r = ed.execute(&Command::EditSurface {
                id,
                edit: Edit::AddEdges { edges: c.to_vec() },
            });
            worst = worst.max(s.elapsed().as_secs_f64());
            edits += 1;
            if let Err(e) = r {
                return format!("ERR({})@edit{edits}", e.to_string().replace(' ', "_"));
            }
        }
        format!("ok edits={edits} worst_edit_wall_s={worst:.3}")
    });
    result(
        "sweep_chunked",
        &format!(
            "base={base} kind={kind} k={k} patch_edges={n} {} {}",
            res,
            fmt_took(&t)
        ),
    );
}

/// A3: the saved text of a document with one surface model, its load, and the edit log.
fn save() {
    let a = args();
    let (shape, p) = (a[0].as_str(), num(&a, 1));
    let mut m = construct(&mesh_of(shape, p));
    assert!(m.problem().is_none(), "does not fit: {:?}", m.problem());
    // A log like a model that has been worked on: 64 small edits far from the shape.
    for k in 0..64 {
        m.apply(&Edit::AddEdges {
            edges: cube_segs(-5.0e6 - k as f64 * 3000.0, -5.0e6, 1000.0),
        })
        .unwrap();
    }
    let c = counts(&m);
    let log_entries = m.log().len();
    let log_text = serde_json::to_string(m.log()).unwrap().len();
    let model_text = serde_json::to_string(&m).unwrap();
    let mut ed = Editor::with_sequential_ids();
    ed.execute(&Command::AddObject {
        shape: Shape::SurfaceModel(Box::new(m)),
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let (text, tsave) = timed(|| {
        let mut v = Vec::new();
        ed.doc().write_compact(&mut v).unwrap();
        v
    });
    let (pretty, tpretty) = timed(|| {
        let mut v = Vec::new();
        ed.doc().write_pretty(&mut v).unwrap();
        v.len()
    });
    let text = String::from_utf8(text).unwrap();
    let rss0 = rss();
    let (loaded, tload) = timed(|| Document::from_json(&text));
    let ok = loaded.is_ok();
    let rss1 = rss();
    // Without the log, to see what it adds.
    let (_, tclone) = timed(|| ed.doc().clone());
    result(
        "save",
        &format!(
            "shape={shape} param={p} {c} model_json_mb={} compact_mb={} pretty_mb={} log_entries={log_entries} log_json_kb={:.1} save_{} pretty_save_wall_s={:.3} load_ok={ok} load_{} doc_clone_wall_s={:.4} rss_before_load_mb={} rss_after_load_mb={}",
            mb(model_text.len()),
            mb(text.len()),
            mb(pretty),
            log_text as f64 / 1024.0,
            fmt_took(&tsave),
            tpretty.wall,
            fmt_took(&tload),
            tclone.wall,
            mb(rss0),
            mb(rss1)
        ),
    );
    if std::env::var("LM_CACHE").is_ok() {
        std::fs::write(format!("{}/lm-{shape}{p}.json", tmp()), model_text).unwrap();
    }
}

/// A3b: the edit log: 64 box edits, and 64 edits of 10,000 edges in an empty-ish model.
fn logsize() {
    let a = args();
    let kind = a[0].as_str();
    let mut m = SurfaceModel::new();
    match kind {
        "boxes64" => {
            for k in 0..64 {
                m.apply(&Edit::AddEdges {
                    edges: cube_segs(k as f64 * 2000.0, 0.0, 1000.0),
                })
                .unwrap();
            }
        }
        "lines64x1" => {
            for k in 0..64 {
                let x = k as f64 * 100.0;
                m.apply(&Edit::AddEdges {
                    edges: vec![[[x, 0.0, 0.0], [x + 50.0, 20.0, 5.0]]],
                })
                .unwrap();
            }
        }
        "big10k" => {
            // Two edits of 10,000 separate edges each (far apart, no faces).
            for e in 0..2 {
                let edges: Vec<Seg> = (0..10_000)
                    .map(|i| {
                        let x = (i % 100) as f64 * 5000.0;
                        let y = (i / 100) as f64 * 5000.0 + e as f64 * 1.0e6;
                        [[x, y, 0.0], [x + 1000.0, y + 700.0, 300.0]]
                    })
                    .collect();
                m.apply(&Edit::AddEdges { edges }).unwrap();
            }
        }
        other => panic!("unknown {other}"),
    }
    let log = serde_json::to_string(m.log()).unwrap();
    let all = serde_json::to_string(&m).unwrap();
    result(
        "logsize",
        &format!(
            "kind={kind} {} log_entries={} log_json_kb={:.1} model_json_kb={:.1}",
            counts(&m),
            m.log().len(),
            log.len() as f64 / 1024.0,
            all.len() as f64 / 1024.0
        ),
    );
}

// ---------------------------------------------------------------------------
// B: groups and components.

/// B4: memory per instance.
fn instmem() {
    let a = args();
    let (kind, parts, count) = (a[0].as_str(), num(&a, 1), num(&a, 2));
    let h0 = heap();
    let r0 = rss();
    let mut doc = Document::with_sequential_ids();
    let sym = push_def(&mut doc, "Def", def_parts(kind, parts));
    let h1 = heap();
    let r1 = rss();
    let (_ids, tmake) = timed(|| push_instances(&mut doc, sym, count));
    let h2 = heap();
    let r2 = rss();
    let one = serde_json::to_string(
        doc.objects
            .values()
            .next()
            .unwrap_or_else(|| panic!("no instance")),
    )
    .unwrap()
    .len();
    let (text, tsave) = timed(|| save_text(&doc));
    let def_only = {
        let mut d = Document::with_sequential_ids();
        push_def(&mut d, "Def", def_parts(kind, parts));
        save_text(&d).len()
    };
    let rl0 = rss();
    let hl0 = heap();
    let (loaded, tload) = timed(|| Document::from_json(&text));
    let loaded = loaded.expect("loads");
    let hl1 = heap();
    let rl1 = rss();
    // The mesh of all instances is built instance by instance (no instancing in the scene), so
    // it is only built when the total number of models is small enough to fit in memory.
    let total_models = parts * count;
    let (mesh_n, tmesh) = if total_models <= 500_000 {
        timed(|| mesh::build(&loaded).mesh.positions.len())
    } else {
        (0, timed(|| ()).1)
    };
    let ed = Editor::new(loaded);
    let (_, tclone) = timed(|| ed.doc().clone());
    result(
        "instmem",
        &format!(
            "kind={kind} parts={parts} instances={count} heap_def_mb={} heap_instances_mb={} heap_per_instance_bytes={:.0} rss_def_mb={} rss_instances_mb={} rss_per_instance_bytes={:.0} json_per_instance_bytes={one} saved_total_mb={} saved_def_only_mb={} saved_per_instance_bytes={:.1} save_{} load_{} load_heap_mb={} load_rss_delta_mb={} total_models={total_models} mesh_positions={mesh_n} mesh_{} doc_clone_wall_s={:.4}",
            mb(h1 - h0),
            mb(h2 - h1),
            (h2 - h1) as f64 / count.max(1) as f64,
            mb(r1.saturating_sub(r0)),
            mb(r2.saturating_sub(r1)),
            r2.saturating_sub(r1) as f64 / count.max(1) as f64,
            mb(text.len()),
            mb(def_only),
            (text.len() - def_only) as f64 / count.max(1) as f64,
            fmt_took(&tsave),
            fmt_took(&tload),
            mb(hl1.saturating_sub(hl0)),
            mb(rl1.saturating_sub(rl0)),
            fmt_took(&tmesh),
            tclone.wall
        ),
    );
}

fn phase<T: std::fmt::Debug>(name: &str, f: impl FnOnce() -> T) -> T {
    let (r, t) = timed(f);
    let mut dbg = format!("{r:?}");
    dbg.truncate(120);
    result(
        "phase",
        &format!(
            "name={name} {} rss_mb={} note={}",
            fmt_took(&t),
            mb(rss()),
            dbg.replace(' ', "_")
        ),
    );
    r
}

fn short(r: Result<kadakas_core::command::Outcome, kadakas_core::command::CommandError>) -> String {
    match r {
        Ok(o) => format!("ok created={}", o.created.len()),
        Err(e) => format!("ERR({e})"),
    }
}

/// B5: the operations on a definition of `parts` parts with 3 instances.
fn ops() {
    let a = args();
    let (kind, parts) = (a[0].as_str(), num(&a, 1));
    let (mut ed, sym, ids) = world(kind, parts, 3);
    result(
        "ops",
        &format!("kind={kind} parts={parts} rss_start_mb={}", mb(rss())),
    );
    phase("leaves", || {
        component::leaves(&ed.doc().resources, sym).map(|l| l.len())
    });
    phase("mesh_build_3_instances", || {
        mesh::build(ed.doc()).mesh.positions.len()
    });
    let text = phase("save", || save_text(ed.doc()));
    phase("load", || {
        Document::from_json(&text).map(|d| d.objects.len())
    });
    drop(text);
    // Edit in place and close, then undo both.
    let group = phase("edit_component_open", || {
        let r = ed.execute(&Command::EditComponent { id: ids[0] });
        match r {
            Ok(o) => Some(ObjectId(o.created[0])),
            Err(e) => {
                println!("open failed: {e}");
                None
            }
        }
    });
    if let Some(g) = group {
        phase("close_component_unchanged", || {
            short(ed.execute(&Command::CloseComponent {
                group: g,
                resolve: None,
            }))
        });
        phase("undo_close", || ed.undo());
        phase("undo_open", || ed.undo());
    }
    phase("make_unique_one_instance", || {
        short(ed.execute(&Command::MakeUnique { ids: vec![ids[1]] }))
    });
    phase("undo_make_unique", || ed.undo());
    // The explode merges every part into one surface model (a weld of all of them): slow for
    // many parts. `ops KIND PARTS 0` leaves it out.
    if a.get(2).is_none_or(|s| s != "0") {
        phase("explode_one_instance", || {
            short(ed.execute(&Command::Explode { ids: vec![ids[2]] }))
        });
        phase("undo_explode", || ed.undo());
        phase("redo_explode", || ed.redo());
    }
    result("ops", &format!("undo_bytes={}", ed.undo_bytes()));
}

/// B5b: the walk of 20,000 models of real definitions: 2 instances of a definition of 10,000.
fn walk20k() {
    let a = args();
    let kind = a[0].as_str();
    let mut doc = Document::with_sequential_ids();
    let big = push_def(&mut doc, "Big", def_parts(kind, component::MAX_PARTS));
    let inst = |k: usize| {
        Part::new(Shape::Component {
            symbol: big,
            transform: component::raw(&DAffine3::from_translation(DVec3::new(
                k as f64 * 3.0e6,
                0.0,
                0.0,
            ))),
        })
    };
    let two = push_def(&mut doc, "Two", vec![inst(0), inst(1)]);
    let ids = push_instances(&mut doc, two, 1);
    let ed = Editor::new(doc);
    result(
        "walk20k",
        &format!("kind={kind} rss_start_mb={}", mb(rss())),
    );
    phase("leaves_20000", || {
        component::leaves(&ed.doc().resources, two).map(|l| l.len())
    });
    phase("leaves_20000_again", || {
        component::leaves(&ed.doc().resources, two).map(|l| l.len())
    });
    phase("mesh_build_one_instance_of_20000_models", || {
        mesh::build(ed.doc()).mesh.positions.len()
    });
    let text = phase("save", || save_text(ed.doc()));
    result("walk20k", &format!("saved_mb={}", mb(text.len())));
    phase("load", || {
        Document::from_json(&text).map(|d| d.objects.len())
    });
    let _ = ids;
}

/// Makes a chain: definition i has `fan` instances of definition i-1; definition 0 has `base`
/// models. Through commands (the checks of the commands run). Returns the first error.
fn fan_chain(levels: usize, fan: usize, base: usize, kind: &str) -> (Editor, Vec<String>) {
    let mut ed = Editor::with_sequential_ids();
    let m = model_of(kind);
    let mut log = Vec::new();
    // Definition 0: `base` models, made into a component.
    let mut objs = Vec::new();
    for k in 0..base {
        let mut mm = m.clone();
        mm.transform = DAffine3::from_translation(DVec3::new(k as f64 * 2000.0, 0.0, 0.0));
        let out = ed
            .execute(&Command::AddObject {
                shape: Shape::SurfaceModel(Box::new(mm)),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        objs.push(ObjectId(out.created[0]));
    }
    let (r, t) = timed(|| {
        ed.execute(&Command::MakeComponent {
            name: "D0".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: objs,
            replace: true,
        })
    });
    let out = r.unwrap();
    let mut def = ResourceId(out.created[0]);
    log.push(format!("D0 ok {}", fmt_took(&t)));
    for lvl in 1..=levels {
        let mut objs = Vec::new();
        for k in 0..fan {
            let out = ed
                .execute(&Command::AddObject {
                    shape: instance_shape(def, k),
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                })
                .unwrap();
            objs.push(ObjectId(out.created[0]));
        }
        let (r, t) = timed(|| {
            ed.execute(&Command::MakeComponent {
                name: format!("D{lvl}"),
                model: None,
                faces: vec![],
                edges: vec![],
                objects: objs,
                replace: true,
            })
        });
        match r {
            Ok(o) => {
                def = ResourceId(o.created[0]);
                log.push(format!(
                    "D{lvl} ok leaves={} {}",
                    fan.pow(lvl as u32) * base,
                    fmt_took(&t)
                ));
            }
            Err(e) => {
                log.push(format!(
                    "D{lvl} REFUSED leaves_would_be={} error=\"{e}\" {}",
                    (fan as f64).powi(lvl as i32) * base as f64,
                    fmt_took(&t)
                ));
                break;
            }
        }
    }
    (ed, log)
}

/// B6: fan-out and depth through the commands.
fn nest() {
    let a = args();
    let (levels, fan, base, kind) = (num(&a, 0), num(&a, 1), num(&a, 2), a[3].as_str());
    let (_ed, log) = fan_chain(levels, fan, base, kind);
    for l in log {
        result(
            "nest",
            &format!(
                "levels={levels} fan={fan} base={base} {}",
                l.replace(' ', "_")
            ),
        );
    }
}

/// B6b: a definition over the limit that got into the document without the commands (an old or
/// hand-made file): what do the walk, the mesh, the save and the load do?
fn oversize() {
    let a = args();
    let (levels, fan, base) = (num(&a, 0), num(&a, 1), num(&a, 2));
    let mut doc = Document::with_sequential_ids();
    let mut def = push_def(&mut doc, "D0", def_parts("sq", base));
    for lvl in 1..=levels {
        let parts: Vec<Part> = (0..fan)
            .map(|k| Part::new(instance_shape(def, k)))
            .collect();
        def = push_def(&mut doc, &format!("D{lvl}"), parts);
    }
    push_instances(&mut doc, def, 1);
    let ed = Editor::new(doc);
    result(
        "oversize",
        &format!(
            "levels={levels} fan={fan} base={base} (leaves would be {})",
            (fan as f64).powi(levels as i32) * base as f64
        ),
    );
    phase("leaves_refusal", || {
        component::leaves(&ed.doc().resources, def)
            .map(|l| l.len())
            .map_err(|e| e.to_string())
    });
    phase("definition_depth", || {
        component::definition_depth(&ed.doc().resources, def)
    });
    phase("mesh_build", || mesh::build(ed.doc()).mesh.positions.len());
    let text = phase("save", || save_text(ed.doc()).len());
    let _ = text;
    let text = save_text(ed.doc());
    phase("load", || {
        Document::from_json(&text)
            .map(|d| d.objects.len())
            .map_err(|e| e.to_string())
    });
}

/// B6c: the cost of `definition_depth`, which the commands call before the limit of models: a
/// chain where every definition has `fan` instances of the one below (a tree of calls).
fn depth_cost() {
    let a = args();
    let (levels, fan) = (num(&a, 0), num(&a, 1));
    let base = a.get(2).map_or("sq", |s| s.as_str());
    let mut doc = Document::with_sequential_ids();
    let mut def = push_def(&mut doc, "D0", def_parts(base, 1));
    for lvl in 1..=levels {
        let parts: Vec<Part> = (0..fan)
            .map(|k| Part::new(instance_shape(def, k)))
            .collect();
        def = push_def(&mut doc, &format!("D{lvl}"), parts);
    }
    let ed = Editor::new(doc);
    phase("definition_depth", || {
        component::definition_depth(&ed.doc().resources, def)
    });
    phase("leaves", || {
        component::leaves(&ed.doc().resources, def)
            .map(|l| l.len())
            .map_err(|e| e.to_string())
    });
}

/// Builds the deepest nesting through the commands and runs the operations. Prints the name of
/// every step first, so that a crash shows where it was.
fn deepest(stack_kib: usize) {
    let run = move || {
        let step = |s: &str| {
            println!("STEP {s}");
            use std::io::Write;
            std::io::stdout().flush().unwrap();
        };
        // 32 groups.
        step("make 32 groups");
        let mut ed = Editor::with_sequential_ids();
        let out = ed
            .execute(&Command::AddObject {
                shape: Shape::SurfaceModel(Box::new(model_of("sq"))),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        let mut cur = ObjectId(out.created[0]);
        for _ in 0..component::MAX_GROUP_DEPTH {
            cur = ObjectId(
                ed.execute(&Command::MakeGroup {
                    model: None,
                    faces: vec![],
                    edges: vec![],
                    objects: vec![cur],
                    name: None,
                })
                .unwrap()
                .created[0],
            );
        }
        step("group height and ancestors");
        assert_eq!(
            component::group_height(ed.doc(), cur),
            component::MAX_GROUP_DEPTH
        );
        step("mesh of 32 groups");
        let _ = mesh::build(ed.doc());
        step("save and load 32 groups");
        let text = save_text(ed.doc());
        Document::from_json(&text).unwrap();
        step("move 32 groups");
        ed.execute(&Command::MoveObjects {
            ids: vec![cur],
            delta: glam::DVec2::new(5.0, 5.0),
        })
        .unwrap();
        step("33rd group is refused");
        let r = ed.execute(&Command::MakeGroup {
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![cur],
            name: None,
        });
        println!("33rd group: {}", short(r));
        step("explode 32 groups, one at a time");
        let mut top = cur;
        for _ in 0..component::MAX_GROUP_DEPTH {
            ed.execute(&Command::Explode { ids: vec![top] }).unwrap();
            match ed
                .doc()
                .objects
                .values()
                .find(|o| o.parent.is_none() && matches!(o.shape, Shape::Group { .. }))
            {
                Some(o) => top = o.id,
                None => break,
            }
        }
        // 16 components deep.
        step("make 16 components deep");
        let (mut ed2, log) = fan_chain(component::MAX_COMPONENT_DEPTH - 1, 1, 1, "sq");
        println!("{}", log.last().unwrap());
        let top_inst = ed2
            .doc()
            .objects
            .values()
            .find(|o| matches!(o.shape, Shape::Component { .. }))
            .map(|o| o.id)
            .unwrap();
        let sym = match ed2.doc().object(top_inst).unwrap().shape {
            Shape::Component { symbol, .. } => symbol,
            _ => unreachable!(),
        };
        step("leaves 16 deep");
        assert_eq!(
            component::leaves(&ed2.doc().resources, sym).unwrap().len(),
            1
        );
        step("mesh 16 deep");
        let _ = mesh::build(ed2.doc());
        step("save and load 16 deep");
        let text = save_text(ed2.doc());
        Document::from_json(&text).unwrap();
        step("17th level is refused");
        let o = ed2
            .execute(&Command::AddObject {
                shape: instance_shape(sym, 0),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        let r = ed2.execute(&Command::MakeComponent {
            name: "D16".into(),
            model: None,
            faces: vec![],
            edges: vec![],
            objects: vec![ObjectId(o.created[0])],
            replace: true,
        });
        println!("17th definition: {}", short(r));
        step("edit component 16 deep");
        let g = ed2.execute(&Command::EditComponent { id: top_inst });
        println!("edit: {}", short(g));
        step("make unique 16 deep");
        let _ = ed2.execute(&Command::CopyObjects {
            ids: vec![top_inst],
            delta: glam::DVec2::new(100.0, 0.0),
        });
        println!(
            "make unique: {}",
            short(ed2.execute(&Command::MakeUnique {
                ids: vec![top_inst]
            }))
        );
        step("explode 16 deep, level by level");
        for _ in 0..component::MAX_COMPONENT_DEPTH {
            let Some(i) = ed2
                .doc()
                .objects
                .values()
                .find(|o| matches!(o.shape, Shape::Component { .. }))
                .map(|o| o.id)
            else {
                break;
            };
            if ed2.execute(&Command::Explode { ids: vec![i] }).is_err() {
                break;
            }
        }
        step("done");
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

// ---------------------------------------------------------------------------------------------
// Scenarios of the second campaign (guards, raised limits).

/// Multiplies the count limits of the surface model for this process (`LM_SCALE`, default 1).
/// Needs `--features measure-limits` on `kadakas-core`.
fn scale_limits() -> usize {
    let n: usize = std::env::var("LM_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    #[cfg(feature = "measure-limits")]
    kadakas_core::surface::set_limit_scale(n);
    #[cfg(not(feature = "measure-limits"))]
    assert_eq!(
        n, 1,
        "LM_SCALE needs --features measure-limits on kadakas-core"
    );
    n
}

fn add_model(ed: &mut Editor, m: SurfaceModel) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::SurfaceModel(Box::new(m)),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .expect("the model fits the limits of this run")
        .created[0],
    )
}

/// A model above the current limits, made with `from_parts` and measured under the raised limits
/// (`LM_SCALE`): the per-command check, the full check, the saved text, load, one small edit, one
/// big edit, Push/Pull of a face, Intersect Faces. Args: shape, parameter.
fn big() {
    let a = args();
    let (shape, p) = (a[0].as_str(), num(&a, 1));
    let scale = scale_limits();
    let rss0 = rss();
    let (m, tbuild) = timed(|| construct(&mesh_of(shape, p)));
    let c = counts(&m);
    result(
        "big",
        &format!(
            "shape={shape} param={p} scale={scale} {c} build_wall_s={:.3} rss_model_mb={}",
            tbuild.wall,
            mb(rss().saturating_sub(rss0))
        ),
    );
    let (problem, tq) = timed(|| m.problem());
    let (rep, tfull) = timed(|| m.check());
    let (text, tsave) = timed(|| serde_json::to_string(&m).unwrap());
    let (back, tparse) = timed(|| serde_json::from_str::<SurfaceModel>(&text));
    result(
        "big",
        &format!(
            "verdict={} quick_check_{} full_check_{} full_check_problems={} model_json_mb={} save_{} parse_{} parse_ok={}",
            problem.unwrap_or_else(|| "fits".into()).replace(' ', "_"),
            fmt_took(&tq),
            fmt_took(&tfull),
            rep.problems.len(),
            mb(text.len()),
            fmt_took(&tsave),
            fmt_took(&tparse),
            back.is_ok()
        ),
    );
    drop(back);
    drop(text);
    // In a document: the command that adds it runs the check, then save and load.
    let first_face = m.faces().next().map(|(f, _)| f);
    let mut ed = Editor::with_sequential_ids();
    let (id, tadd) = timed(|| add_model(&mut ed, m));
    let (doc_text, tdoc) = timed(|| save_text(ed.doc()));
    let r0 = rss();
    let (loaded, tload) = timed(|| Document::from_json(&doc_text));
    let r1 = rss();
    result(
        "big",
        &format!(
            "add_object_{} doc_compact_mb={} doc_save_{} load_ok={} load_{} load_rss_delta_mb={}",
            fmt_took(&tadd),
            mb(doc_text.len()),
            fmt_took(&tdoc),
            loaded.is_ok(),
            fmt_took(&tload),
            mb(r1.saturating_sub(r0))
        ),
    );
    drop(loaded);
    drop(doc_text);
    // One edge, then 1,000 edges, far from the model (the cost is the model's, not the geometry).
    for (name, k) in [("add_1_edge", 1usize), ("add_1000_edges", 1000)] {
        let edges: Vec<Seg> = (0..k)
            .map(|i| {
                let x = -9.0e6 - i as f64 * 3000.0;
                [[x, -9.0e6, 0.0], [x + 1000.0, -9.0e6, 0.0]]
            })
            .collect();
        let (r, t) = timed(|| {
            ed.execute(&Command::EditSurface {
                id,
                edit: Edit::AddEdges { edges },
            })
        });
        result("big", &format!("{name} ok={} {}", r.is_ok(), fmt_took(&t)));
        if let Err(e) = r {
            result(
                "big",
                &format!("{name} error={}", e.to_string().replace(' ', "_")),
            );
        }
    }
    // Push/Pull of a face.
    if let Some(f) = first_face {
        let (r, t) = timed(|| {
            ed.execute(&Command::EditSurface {
                id,
                edit: Edit::PushPull {
                    face: f,
                    distance: 500.0,
                },
            })
        });
        result(
            "big",
            &format!("push_pull ok={} {}", r.is_ok(), fmt_took(&t)),
        );
        if let Err(e) = r {
            result(
                "big",
                &format!("push_pull error={}", e.to_string().replace(' ', "_")),
            );
        }
    }
    // Intersect Faces on the whole model.
    let (r, t) = timed(|| {
        ed.execute(&Command::EditSurface {
            id,
            edit: Edit::IntersectFaces {
                scope: kadakas_core::surface::IntersectScope::Model,
                faces: vec![],
                edges: vec![],
            },
        })
    });
    result(
        "big",
        &format!("intersect_faces ok={} {}", r.is_ok(), fmt_took(&t)),
    );
    if let Err(e) = r {
        result(
            "big",
            &format!("intersect_faces error={}", e.to_string().replace(' ', "_")),
        );
    }
}

/// The cost of every walk of a chain of definitions that hold no model: `levels` levels, `fan`
/// instances per level. Prints the time of each walk (and, on a build with the guards, the refusal).
fn chain() {
    let a = args();
    let (levels, fan) = (num(&a, 0), num(&a, 1));
    let mut doc = Document::with_sequential_ids();
    let mut def = push_def(&mut doc, "D0", def_parts("empty", 1));
    for lvl in 1..=levels {
        let parts: Vec<Part> = (0..fan)
            .map(|k| Part::new(instance_shape(def, k)))
            .collect();
        def = push_def(&mut doc, &format!("D{lvl}"), parts);
    }
    push_instances(&mut doc, def, 3);
    let ed = Editor::new(doc);
    phase("definition_depth", || {
        component::definition_depth(&ed.doc().resources, def)
    });
    phase("leaves", || {
        component::leaves(&ed.doc().resources, def)
            .map(|l| l.len())
            .map_err(|e| e.to_string())
    });
    phase("findings", || component::findings(ed.doc()).len());
    phase("mesh_build", || mesh::build(ed.doc()).lines.segment_count());
    let text = save_text(ed.doc());
    phase("load", || {
        Document::from_json(&text)
            .map(|d| d.load_notes().len())
            .map_err(|e| e.to_string())
    });
}

/// The time to refuse a chain over the budget when a command makes it (AddResource, one level at
/// a time): the level at which it is refused and the time of the refusal.
fn chain_commands() {
    let a = args();
    let (levels, fan) = (num(&a, 0), num(&a, 1));
    let mut ed = Editor::with_sequential_ids();
    let first = ed
        .execute(&Command::AddResource {
            name: "D0".into(),
            body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(SymbolDef {
                parts: def_parts("empty", 1),
                ..SymbolDef::default()
            }))),
        })
        .unwrap();
    let mut def = ResourceId(first.created[0]);
    for lvl in 1..=levels {
        let parts: Vec<Part> = (0..fan)
            .map(|k| Part::new(instance_shape(def, k)))
            .collect();
        let (r, t) = timed(|| {
            ed.execute(&Command::AddResource {
                name: format!("D{lvl}"),
                body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(SymbolDef {
                    parts,
                    ..SymbolDef::default()
                }))),
            })
        });
        match r {
            Ok(o) => {
                def = ResourceId(*o.created.last().unwrap());
                result("chain_cmd", &format!("level={lvl} ok {}", fmt_took(&t)));
            }
            Err(e) => {
                let mut why = e.to_string();
                why.truncate(100);
                result(
                    "chain_cmd",
                    &format!("level={lvl} REFUSED {} error=\"{why}\"", fmt_took(&t)),
                );
                break;
            }
        }
    }
}

/// A definition at the size limits. `kind`: `cubes` (parts of cubes with about 100,000 edges in
/// all) or `text` (one part padded to 16 MiB of saved text). Prints the text size, the time to add
/// it, to open it for edit and to close it.
fn defsize() {
    let a = args();
    let kind = a[0].as_str();
    let parts: Vec<Part> = match kind {
        "cubes" => {
            // 12 edges per cube: 8,333 cubes are 99,996 edges.
            let m = model_of("cube");
            (0..8_333).map(|k| part_at(&m, k)).collect()
        }
        "text" => {
            let mut m = SurfaceModel::new();
            m.name = "x".repeat(16 * 1024 * 1024 - 400);
            vec![Part::new(Shape::SurfaceModel(Box::new(m)))]
        }
        other => panic!("unknown kind {other}"),
    };
    let edges = component::definition_edges(&parts);
    let (bytes, tbytes) = timed(|| component::definition_bytes(&parts));
    result(
        "defsize",
        &format!(
            "kind={kind} parts={} edges={edges} saved_text_mb={} count_bytes_{}",
            parts.len(),
            mb(bytes),
            fmt_took(&tbytes)
        ),
    );
    let mut ed = Editor::with_sequential_ids();
    let (r, t) = timed(|| {
        ed.execute(&Command::AddResource {
            name: "Big".into(),
            body: ResourceBody::Graphic(Box::new(GraphicResource::Symbol(SymbolDef {
                parts,
                ..SymbolDef::default()
            }))),
        })
    });
    result(
        "defsize",
        &format!("add_resource ok={} {}", r.is_ok(), fmt_took(&t)),
    );
    let Ok(out) = r else { return };
    let sym = ResourceId(out.created[0]);
    let ids = push_instances_in(&mut ed, sym, 2);
    let (open, t) = timed(|| ed.execute(&Command::EditComponent { id: ids[0] }));
    result(
        "defsize",
        &format!("edit_component ok={} {}", open.is_ok(), fmt_took(&t)),
    );
    let Ok(open) = open else { return };
    let g = ObjectId(open.created[0]);
    let (r, t) = timed(|| {
        ed.execute(&Command::CloseComponent {
            group: g,
            resolve: Some(component::Resolve::Overwrite),
        })
    });
    result(
        "defsize",
        &format!("close_component ok={} {}", r.is_ok(), fmt_took(&t)),
    );
    let text = save_text(ed.doc());
    result(
        "defsize",
        &format!("document_compact_mb={}", mb(text.len())),
    );
}

fn push_instances_in(ed: &mut Editor, sym: ResourceId, n: usize) -> Vec<ObjectId> {
    (0..n)
        .map(|k| {
            ObjectId(
                ed.execute(&Command::AddObject {
                    shape: instance_shape(sym, k),
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                })
                .unwrap()
                .created[0],
            )
        })
        .collect()
}

/// The cost of the total of models: `parts` models in a definition, `count` instances; builds the
/// mesh (the 3D scene) and prints the time and the memory. Models are empty (`empty`) or cubes
/// (`cube`).
fn total_models() {
    let a = args();
    let (kind, parts, count) = (a[0].as_str(), num(&a, 1), num(&a, 2));
    let r0 = rss();
    let (mut doc, tbuild) = timed(|| {
        let mut doc = Document::with_sequential_ids();
        let sym = push_def(&mut doc, "Def", def_parts(kind, parts));
        push_instances(&mut doc, sym, count);
        doc
    });
    let models = parts * count;
    let findings = component::definition_findings(&doc);
    result(
        "total_models",
        &format!(
            "kind={kind} parts={parts} instances={count} models={models} build_{} rss_doc_mb={} notes={}",
            fmt_took(&tbuild),
            mb(rss().saturating_sub(r0)),
            findings.len()
        ),
    );
    let r1 = rss();
    let (scene, t) = timed(|| mesh::build(&doc));
    result(
        "total_models",
        &format!(
            "mesh_{} positions={} line_segments={} rss_after_mb={} rss_delta_mb={}",
            fmt_took(&t),
            scene.mesh.positions.len(),
            scene.lines.segment_count(),
            mb(rss()),
            mb(rss().saturating_sub(r1))
        ),
    );
    drop(scene);
    let _ = &mut doc;
}

#[test]
#[ignore = "measurement: run in release mode, see the header of this file"]
fn scenario() {
    let name = std::env::var("LM").expect("set LM to the scenario name");
    match name.as_str() {
        "noop" => {
            result("noop", &format!("rss_mb={}", mb(rss())));
        }
        "shape" => shape(),
        "limit" => limit(),
        "sweep" => sweep(),
        "sweep_chunked" => sweep_chunked(),
        "save" => save(),
        "logsize" => logsize(),
        "instmem" => instmem(),
        "ops" => ops(),
        "walk20k" => walk20k(),
        "nest" => nest(),
        "oversize" => oversize(),
        "depth_cost" => depth_cost(),
        "big" => big(),
        "chain" => chain(),
        "chain_cmd" => chain_commands(),
        "defsize" => defsize(),
        "total_models" => total_models(),
        "stack" => deepest(num(&args(), 0)),
        "limits" => {
            let _: BTreeMap<u8, u8> = BTreeMap::new();
            result(
                "limits",
                &format!(
                    "MAX_VERTICES={MAX_VERTICES} MAX_EDGES={MAX_EDGES} MAX_FACES={MAX_FACES} MAX_LOOP_VERTICES={MAX_LOOP_VERTICES} MAX_EDIT_EDGES={MAX_EDIT_EDGES}"
                ),
            );
        }
        other => panic!("unknown scenario {other}"),
    }
    println!("MAXRSS_MB {}", mb(max_rss()));
}
