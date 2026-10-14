//! Measures the versions (REV-01, REV-02): the time of `version.create`, `version.restore`,
//! `version.diff` and the read of the history of a file, the size of the file, and the peak memory,
//! for drawings of 1 000, 10 000 and 100 000 objects and histories of 10, 100 and 1 000 versions.
//!
//! It is a table that a person reads to choose the default limits, not a test of speed. The test
//! that runs by default uses a tiny table and checks the work and the shape of the answers only.
//! The full table needs a release build and a long time (the 100 000 object rows take most of it):
//!
//! ```text
//! cargo test --release -p kadakas-api --test versions_bench -- --ignored --nocapture
//! ```
//!
//! `KADAKAS_BENCH_OBJECTS` and `KADAKAS_BENCH_VERSIONS` (comma separated) choose other sizes.
//! Memory is the resident size of the process (`ps`), sampled after each step, so it is the peak
//! of the samples; the process keeps what earlier rows freed, so read it as an upper bound.

use std::time::Instant;

use kadakas_api::Session;
use kadakas_core::model::{Document, Object, ObjectId, Shape};
use serde_json::{Value, json};

fn call(s: &mut Session, m: &str, p: Value) -> Value {
    match s.handle(m, p) {
        Ok(v) => v,
        Err(e) => panic!("{m} failed: {} {}", e.code, e.message),
    }
}

/// The resident memory of this process in bytes (0 if `ps` is not there).
fn rss() -> u64 {
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|t| t.trim().parse::<u64>().ok())
        .map_or(0, |kib| kib * 1024)
}

/// A drawing of `n` rectangles in a grid, on three layers.
fn drawing(n: usize) -> Document {
    let mut doc = Document::with_sequential_ids();
    for l in 0..2 {
        let id = doc.alloc_id();
        doc.layers.push(kadakas_core::model::Layer {
            id: kadakas_core::model::LayerId(id),
            name: format!("Layer {}", l + 2),
            ..doc.layers[0].clone()
        });
    }
    for i in 0..n {
        let id = ObjectId(doc.alloc_id());
        let layer = doc.layers[i % 3].id;
        let (x, y) = ((i % 300) as f64 * 1200.0, (i / 300) as f64 * 1200.0);
        let class = doc.active_class;
        doc.objects.insert(
            id,
            Object::new(
                id,
                layer,
                class,
                Shape::Rect {
                    min: kadakas_core::DVec2::new(x, y),
                    max: kadakas_core::DVec2::new(x + 1000.0, y + 800.0),
                },
            ),
        );
    }
    doc
}

/// One row of the table.
#[derive(Debug, Default)]
struct Row {
    objects: usize,
    create_avg_ms: f64,
    create_last_ms: f64,
    restore_ms: f64,
    diff_ms: f64,
    save_ms: f64,
    open_ms: f64,
    history_read_ms: f64,
    file_bytes: usize,
    file_bytes_without_history: usize,
    history_bytes: u64,
    peak_rss: u64,
    versions_after: usize,
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn measure(objects: usize, versions: usize) -> Row {
    let mut row = Row {
        objects,
        ..Row::default()
    };
    let mut s = Session::with_document(drawing(objects));
    let mut limits = s.limits().clone();
    limits.max_versions = 5_000;
    limits.max_version_bytes = 4 * 1024 * 1024 * 1024;
    s.set_limits(limits);
    let ids: Vec<String> = s
        .editor
        .doc()
        .objects
        .keys()
        .take(50)
        .map(|k| k.0.to_string())
        .collect();
    let mut peak = rss();
    let mut total = 0.0;
    for v in 0..versions {
        // A small edit between versions: three objects move.
        let pick: Vec<&String> = (0..3).map(|k| &ids[(v * 3 + k) % ids.len()]).collect();
        call(
            &mut s,
            "execute",
            json!({"command": {"op": "move_objects", "ids": pick, "delta": [1.0, 0.0]}}),
        );
        let t = Instant::now();
        call(
            &mut s,
            "version.create",
            json!({"name": format!("v{v}"), "now_ms": 1000 + v}),
        );
        let took = ms(t);
        total += took;
        row.create_last_ms = took;
        if v % 25 == 0 || v + 1 == versions {
            peak = peak.max(rss());
        }
    }
    row.create_avg_ms = total / versions as f64;
    let list = call(&mut s, "version.list", json!({}));
    row.history_bytes = list["bytes"].as_u64().unwrap_or(0);
    row.versions_after = list["count"].as_u64().unwrap_or(0) as usize;

    // Compare the oldest version with the drawing.
    let t = Instant::now();
    let d = call(&mut s, "version.diff", json!({"from": 1, "to": "current"}));
    row.diff_ms = ms(t);
    assert!(d["exact"].as_bool().unwrap_or(false));
    peak = peak.max(rss());

    // Save: the file form, and its size with and without the versions.
    let t = Instant::now();
    let text = kadakas_api::files::document_text(&s.document_for_save().unwrap()).unwrap();
    row.save_ms = ms(t);
    row.file_bytes = text.len();
    let mut plain = s.editor.doc().clone();
    plain.history = None;
    row.file_bytes_without_history = serde_json::to_string_pretty(&plain).unwrap().len();
    peak = peak.max(rss());

    // Open: the drawing first (it never needs the history), then the history.
    let t = Instant::now();
    let doc = Document::from_json(&text).unwrap();
    row.open_ms = ms(t);
    let t = Instant::now();
    let mut opened = Session::with_document(doc);
    row.history_read_ms = ms(t);
    assert!(opened.load_notes().is_empty(), "{:?}", opened.load_notes());
    let t = Instant::now();
    let l = call(&mut opened, "version.list", json!({"current": false}));
    row.history_read_ms += ms(t);
    assert_eq!(l["count"].as_u64().unwrap() as usize, row.versions_after);
    assert_eq!(row.versions_after, versions);
    peak = peak.max(rss());
    drop(opened);

    // Restore the oldest version (the work is saved first as a version).
    let t = Instant::now();
    call(&mut s, "version.restore", json!({"id": 1, "now_ms": 5}));
    row.restore_ms = ms(t);
    peak = peak.max(rss());
    row.peak_rss = peak;
    row
}

fn mib(b: u64) -> f64 {
    b as f64 / (1024.0 * 1024.0)
}

fn print_table(rows: &[Row]) {
    println!(
        "{:>8} {:>8} | {:>10} {:>10} {:>10} {:>10} | {:>9} {:>9} {:>9} | {:>10} {:>10} {:>10} | {:>9}",
        "objects", "versions", "create avg", "create end", "restore", "diff", "save", "open", "history",
        "file MiB", "no hist.", "history MiB", "peak MiB"
    );
    println!(
        "{:>8} {:>8} | {:>10} {:>10} {:>10} {:>10} | {:>9} {:>9} {:>9} | {:>10} {:>10} {:>10} | {:>9}",
        "", "", "ms", "ms", "ms", "ms", "ms", "ms (drawing)", "ms (read)", "", "file MiB", "(estimate)", ""
    );
    for r in rows {
        println!(
            "{:>8} {:>8} | {:>10.1} {:>10.1} {:>10.1} {:>10.1} | {:>9.1} {:>9.1} {:>9.1} | {:>10.2} {:>10.2} {:>10.2} | {:>9.0}",
            r.objects,
            r.versions_after,
            r.create_avg_ms,
            r.create_last_ms,
            r.restore_ms,
            r.diff_ms,
            r.save_ms,
            r.open_ms,
            r.history_read_ms,
            mib(r.file_bytes as u64),
            mib(r.file_bytes_without_history as u64),
            mib(r.history_bytes),
            mib(r.peak_rss),
        );
    }
}

fn sizes(var: &str, default: &[usize]) -> Vec<usize> {
    std::env::var(var)
        .ok()
        .map(|t| t.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_else(|| default.to_vec())
}

/// The table of the full sizes. Slow: run it in release mode.
#[test]
#[ignore = "timing test: run in release mode"]
fn the_table_of_versions() {
    let objects = sizes("KADAKAS_BENCH_OBJECTS", &[1_000, 10_000, 100_000]);
    let versions = sizes("KADAKAS_BENCH_VERSIONS", &[10, 100, 1_000]);
    let mut rows = Vec::new();
    for &o in &objects {
        for &v in &versions {
            eprintln!("measuring {o} objects, {v} versions ...");
            rows.push(measure(o, v));
            print_table(&rows[rows.len() - 1..]);
        }
    }
    println!();
    print_table(&rows);
}

/// The same code on a tiny table: it checks that the measuring works and that the answers have the
/// expected shape. No time is asserted.
#[test]
fn the_measuring_works_on_a_tiny_table() {
    let r = measure(200, 5);
    assert_eq!(r.versions_after, 5);
    assert!(r.file_bytes > r.file_bytes_without_history);
    assert!(r.history_bytes > 0);
    // Five small edits of a drawing of 200 objects are far smaller than five copies of it.
    assert!(
        (r.file_bytes - r.file_bytes_without_history) * 5 < r.file_bytes_without_history * 5,
        "the versions in the file: {} bytes for a drawing of {}",
        r.file_bytes - r.file_bytes_without_history,
        r.file_bytes_without_history
    );
    print_table(&[r]);
}
