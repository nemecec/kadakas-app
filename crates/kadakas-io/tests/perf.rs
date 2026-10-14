//! Timing of the DXF import with source tracking, the stored base, the diff of two revisions and
//! the cost of a command after a big import (IOP-22 to IOP-24). These are perf tests: they only
//! print numbers, so the limits can be chosen from them.
//!
//! ```text
//! cargo test --release -p kadakas-io --test perf -- --ignored --nocapture
//! ```
//!
//! Set `PERF_SIZES=10000,100000` to run fewer sizes.

use std::time::{Duration, Instant};

use kadakas_core::command::{Command, Editor};
use kadakas_core::model::Document;
use kadakas_core::provenance;
use kadakas_io::dxf::DxfFormat;
use kadakas_io::dxf::revision::{self, Options, Rec};
use kadakas_io::{BaseMode, Context, ImportOptions, Importer, Source};

fn drawing(n: usize, version: &str, moved: usize) -> String {
    let mut s = String::with_capacity(n * 70);
    s.push_str(&format!(
        "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n9\n$FINGERPRINTGUID\n2\n{{AAAA-1}}\n9\n$VERSIONGUID\n2\n{{{version}}}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n"
    ));
    for k in 0..n {
        let (x, y) = ((k % 1000) as f64 * 100.0, (k / 1000) as f64 * 100.0);
        let d = if k < moved { 3.0 } else { 0.0 };
        s.push_str(&format!(
            "0\nLINE\n5\n{:X}\n8\nL{}\n10\n{}\n20\n{y}\n11\n{}\n21\n{}\n",
            k + 100,
            k % 20,
            x + d,
            x + d + 10.0,
            y + 5.0
        ));
    }
    s.push_str("0\nENDSEC\n0\nEOF\n");
    s
}

fn ms(d: Duration) -> String {
    format!("{:>9.1} ms", d.as_secs_f64() * 1000.0)
}

fn import(text: &str, track: bool) -> (Editor, Duration, Duration, kadakas_io::Report) {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let src = Source {
        name: "perf.dxf".into(),
        data: text.as_bytes().to_vec(),
        extra: Default::default(),
    };
    // A drawing over the cap needs a choice; the timing is of an import without update data.
    let base = if text.matches("\n0\nLINE\n").count() > provenance::MAX_BASE_RECS {
        BaseMode::Off
    } else {
        BaseMode::Auto
    };
    let opts = ImportOptions {
        track_source: track,
        max_objects: 10_000_000,
        base,
        ..ImportOptions::default()
    };
    let t = Instant::now();
    let r = DxfFormat
        .import(&src, &Context::of(ed.doc()), &opts)
        .unwrap();
    let plan = t.elapsed();
    let t = Instant::now();
    ed.execute(&r.command).unwrap();
    (ed, plan, t.elapsed(), r.report)
}

#[test]
#[ignore = "timing test: run in release mode"]
fn perf_import_base_diff_and_undo_cost() {
    let sizes: Vec<usize> = std::env::var("PERF_SIZES")
        .ok()
        .map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![10_000, 100_000, 500_000]);
    println!("\nentities | step                                   | time        | size / note");
    for n in sizes {
        let v1 = drawing(n, "B-1", 0);
        let v2 = drawing(n, "B-2", n / 10);
        println!(
            "{n:>8} | file                                   |             | {:.1} MiB",
            v1.len() as f64 / 1048576.0
        );

        let (_, plan, run, _) = import(&v1, false);
        println!(
            "{n:>8} | import without tracking: plan, execute | {} | {}",
            ms(plan),
            ms(run)
        );
        let (mut ed, plan, run, report) = import(&v1, true);
        println!(
            "{n:>8} | import with tracking: plan, execute    | {} | {}",
            ms(plan),
            ms(run)
        );
        let base_bytes: usize = ed
            .doc()
            .resources
            .iter()
            .map(|r| serde_json::to_string(r).unwrap().len())
            .sum();
        println!(
            "{n:>8} | stored base                            |             | {} part(s), {:.1} MiB JSON{}",
            ed.doc().resources.len(),
            base_bytes as f64 / 1048576.0,
            if ed.doc().resources.is_empty() {
                format!(" (not stored: {:?})", report.notes.last())
            } else {
                String::new()
            }
        );

        let t = Instant::now();
        let json = serde_json::to_string(ed.doc()).unwrap();
        let save = t.elapsed();
        let t = Instant::now();
        let loaded = Document::from_json(&json).unwrap();
        println!(
            "{n:>8} | save to JSON, load                     | {} | {} load, {:.1} MiB",
            ms(save),
            ms(t.elapsed()),
            json.len() as f64 / 1048576.0
        );
        drop(loaded);

        // The diff of the stored base against the next revision of the file.
        let t = Instant::now();
        let new_dxf = kadakas_io::dxf::parse::parse(v2.as_bytes()).unwrap();
        let (new, _) = revision::records(&new_dxf, 5_000_000);
        let prep = t.elapsed();
        let old: Vec<Rec> = match provenance::base_recs(ed.doc(), "guid:{AAAA-1}", "guid:{B-1}") {
            Some(v) => v.iter().filter_map(|v| Rec::from_value(v)).collect(),
            None => {
                revision::records(
                    &kadakas_io::dxf::parse::parse(v1.as_bytes()).unwrap(),
                    5_000_000,
                )
                .0
            }
        };
        let t = Instant::now();
        let d = revision::diff(&old, &new, &Options::for_unit(1.0));
        println!(
            "{n:>8} | records of the new file, diff          | {} | {} diff: {} changed, {} unchanged, truncated {}, work {}",
            ms(prep),
            ms(t.elapsed()),
            d.counts.changed,
            d.counts.unchanged,
            d.truncated,
            d.work
        );
        // The same without handles (everything is found by hash or position).
        let strip = |recs: &[Rec]| -> Vec<Rec> {
            recs.iter()
                .cloned()
                .map(|mut r| {
                    r.handle = None;
                    r
                })
                .collect()
        };
        let (a, b) = (strip(&old), strip(&new));
        let t = Instant::now();
        let d = revision::diff(&a, &b, &Options::for_unit(1.0));
        println!(
            "{n:>8} | diff without handles                   | {} | {} changed by position, truncated {}, work {}",
            ms(t.elapsed()),
            d.counts.by_proximity,
            d.truncated,
            d.work
        );

        // The cost of a command after the import: the steps of the undo history share the base.
        let t = Instant::now();
        for k in 0..20 {
            ed.execute(&Command::AddLayer {
                name: format!("Work {k}"),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap();
        }
        let per = t.elapsed() / 20;
        let t = Instant::now();
        ed.execute(&Command::MoveObjects {
            ids: ed.doc().objects.keys().take(10).copied().collect(),
            delta: kadakas_core::DVec2::new(1.0, 0.0),
        })
        .unwrap();
        println!(
            "{n:>8} | one AddLayer (avg of 20), one MoveObjects |  {} | {}",
            ms(per),
            ms(t.elapsed())
        );
    }
}
