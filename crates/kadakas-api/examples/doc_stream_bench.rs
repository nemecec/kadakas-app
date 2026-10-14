//! Benchmark for DOC-19: peak memory and time to make, load and save a `.kad` file of a given size.
//! It asserts nothing about time. Run it in a release build, one process per measurement, so that
//! the peak resident memory of the process (`/usr/bin/time -l` on macOS, `-v` on Linux) is that
//! of one step:
//!
//! ```text
//! doc_stream_bench gen  <megabytes> <file>      make a file of about that size
//! doc_stream_bench load <file> reader|slice     load with `from_reader` (the app) or `from_slice`
//! doc_stream_bench save <file> <out>            load, then save with the streaming atomic save
//! ```
//!
//! The file is a mix: half of its bytes in plain objects (circles, rectangles, polylines), a
//! quarter in site models (3D points) and a quarter in underlay pictures (base64 PNG data).
//! It prints one line with the file size, the time and the peak of the heap (a counting allocator).
//! The folder of the file is the caller's choice: use a temporary folder.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use glam::{DVec2, DVec3};
use kadakas_core::model::SiteSettings;
use kadakas_core::model::{Document, FIRST_LAYER, NONE_CLASS, Object, ObjectId, Shape, Vertex};
use kadakas_core::underlay::{PngData, Underlay};

struct Counting;
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call goes to the system allocator. The counters only watch.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let now = CURRENT.fetch_add(l.size(), Relaxed) + l.size();
        PEAK.fetch_max(now, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        CURRENT.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        if new >= l.size() {
            let now = CURRENT.fetch_add(new - l.size(), Relaxed) + new - l.size();
            PEAK.fetch_max(now, Relaxed);
        } else {
            CURRENT.fetch_sub(l.size() - new, Relaxed);
        }
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

const MB: usize = 1_000_000;
/// About 1.6 MB of pretty JSON for one site model of 20 000 points.
const SITE_POINTS: usize = 20_000;
const SITE_MB: f64 = 1.6;
/// A 2 MB picture is 2.7 MB of base64 text.
const PICTURE_BYTES: usize = 2 * MB;
const PICTURE_MB: f64 = 2.7;
/// About this many bytes of pretty JSON for one plain object.
const OBJECT_BYTES: f64 = 450.0;

fn make(megabytes: usize) -> Document {
    let mb = megabytes as f64;
    let objects = (mb * 0.5 * MB as f64 / OBJECT_BYTES) as u64;
    let sites = ((mb * 0.25 / SITE_MB) as u64).max(1);
    let pictures = ((mb * 0.25 / PICTURE_MB) as u64).max(1);
    let mut doc = Document::with_sequential_ids();
    let mut n = 10u64;
    let mut add = |doc: &mut Document, shape: Shape| {
        let o = Object::new(ObjectId::from_num(n), FIRST_LAYER, NONE_CLASS, shape);
        n += 1;
        doc.objects.insert(o.id, o);
    };
    for i in 0..objects {
        let x = (i % 1000) as f64 * 10.0;
        let y = (i / 1000) as f64 * 10.0;
        let shape = match i % 3 {
            0 => Shape::Circle {
                center: DVec2::new(x, y),
                radius: 5.0 + (i % 7) as f64,
            },
            1 => Shape::Rect {
                min: DVec2::new(x, y),
                max: DVec2::new(x + 8.0, y + 6.0),
            },
            _ => Shape::Polyline {
                points: (0..4)
                    .map(|k| Vertex::corner(DVec2::new(x + k as f64, y + (k * k) as f64)))
                    .collect(),
                closed: false,
            },
        };
        add(&mut doc, shape);
    }
    for s in 0..sites {
        let points = (0..SITE_POINTS)
            .map(|i| {
                let t = (s as usize * SITE_POINTS + i) as f64;
                DVec3::new(t.sin() * 5000.0, t.cos() * 5000.0, (t * 0.01).sin() * 800.0)
            })
            .collect();
        let shape = Shape::SiteModel {
            points,
            lines: Vec::new(),
            settings: SiteSettings::default(),
        };
        add(&mut doc, shape);
    }
    for p in 0..pictures {
        let mut png = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
        png.extend(
            (0..PICTURE_BYTES).map(|i| ((i as u64 + p).wrapping_mul(2_654_435_761) >> 7) as u8),
        );
        let u = Underlay {
            name: format!("picture {p}"),
            min: DVec2::ZERO,
            max: DVec2::new(1000.0, 1000.0),
            width: 100,
            height: 100,
            png: PngData(std::sync::Arc::new(png)),
            opacity: 1.0,
            source: String::new(),
        };
        add(&mut doc, Shape::Underlay(Box::new(u)));
    }
    doc
}

fn report(what: &str, file: &Path, started: Instant) {
    let len = std::fs::metadata(file).map_or(0, |m| m.len());
    println!(
        "{what}: file {:.1} MB, time {:.2} s, heap peak {:.1} MB",
        len as f64 / MB as f64,
        started.elapsed().as_secs_f64(),
        PEAK.load(Relaxed) as f64 / MB as f64
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or_default();
    let started = Instant::now();
    match arg(0) {
        "gen" => {
            let file = Path::new(arg(2));
            let doc = make(arg(1).parse().expect("megabytes"));
            let mut w = BufWriter::new(std::fs::File::create(file).expect("create"));
            doc.write_pretty(&mut w).expect("write");
            w.flush().expect("flush");
            report("gen", file, started);
        }
        "load" => {
            let file = Path::new(arg(1));
            let doc = match arg(2) {
                "slice" => {
                    let bytes = std::fs::read(file).expect("read");
                    Document::from_slice(&bytes).expect("load")
                }
                _ => kadakas_api::files::read_document(file).expect("load"),
            };
            report(
                &format!("load {} ({} objects)", arg(2), doc.objects.len()),
                file,
                started,
            );
        }
        "save" => {
            let file = Path::new(arg(1));
            let doc = kadakas_api::files::read_document(file).expect("load");
            let loaded = Instant::now();
            PEAK.store(CURRENT.load(Relaxed), Relaxed);
            kadakas_api::files::write_document_file(Path::new(arg(2)), &doc).expect("save");
            report("save", Path::new(arg(2)), loaded);
        }
        _ => eprintln!(
            "usage: doc_stream_bench gen <megabytes> <file> | load <file> reader|slice | save <file> <out>"
        ),
    }
}
