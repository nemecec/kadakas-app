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
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use glam::{DVec2, DVec3};
use kadakas_api::files::SaveSpec;
use kadakas_core::model::SiteSettings;
use kadakas_core::model::{Document, FIRST_LAYER, NONE_CLASS, Object, ObjectId, Shape, Vertex};
use kadakas_core::underlay::{PngData, Underlay};
use kadakas_file::ReadMode;

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
            png: PngData::new(png),
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
        // The form follows the extension: `.json` is the plain JSON form (the file of DOC-19), anything
        // else the container (DOC-20).
        "gen" => {
            let file = Path::new(arg(2));
            let doc = make(arg(1).parse().expect("megabytes"));
            let spec = SaveSpec::from_extension(file, Vec::new());
            kadakas_api::files::write_document(file, &doc, &spec).expect("write");
            report("gen", file, started);
        }
        // `lazy` keeps the pictures of a container in the file, `eager` reads them (and is what a
        // plain JSON file always does), `slice` reads the whole file into memory first.
        "load" => {
            let file = Path::new(arg(1));
            let (doc, source) = match arg(2) {
                "slice" => {
                    let bytes = std::fs::read(file).expect("read");
                    (Document::from_slice(&bytes).expect("load"), None)
                }
                "eager" => {
                    let o = kadakas_api::files::open_document(file, u64::MAX, ReadMode::Eager)
                        .expect("load");
                    (o.doc, o.source)
                }
                _ => {
                    let o = kadakas_api::files::open_document(file, u64::MAX, ReadMode::Lazy)
                        .expect("load");
                    (o.doc, o.source)
                }
            };
            let mut lazy = 0;
            doc.for_each_blob(&mut |b| {
                if let kadakas_core::model::DocBlob::Picture(p) = b {
                    lazy += usize::from(p.is_lazy());
                }
            });
            report(
                &format!(
                    "load {} ({} objects, {lazy} pictures in the file, source {})",
                    arg(2),
                    doc.objects.len(),
                    source.is_some()
                ),
                file,
                started,
            );
        }
        // Load lazily, then save to another path (copies the pictures from the first file).
        "save" => {
            let file = Path::new(arg(1));
            let o =
                kadakas_api::files::open_document(file, u64::MAX, ReadMode::Lazy).expect("load");
            let loaded = Instant::now();
            PEAK.store(CURRENT.load(Relaxed), Relaxed);
            let out = Path::new(arg(2));
            let spec = SaveSpec::from_extension(out, o.source.iter().cloned().collect());
            kadakas_api::files::write_document(out, &o.doc, &spec).expect("save");
            report("save", out, loaded);
        }
        // Load lazily, change one object, save over the same file (the pictures are copied from the
        // file that is replaced).
        "saveover" => {
            let file = Path::new(arg(1));
            let mut o =
                kadakas_api::files::open_document(file, u64::MAX, ReadMode::Lazy).expect("load");
            let loaded = Instant::now();
            PEAK.store(CURRENT.load(Relaxed), Relaxed);
            if let Some(obj) = o.doc.objects.values_mut().next() {
                obj.shape = Shape::Circle {
                    center: DVec2::new(1.0, 2.0),
                    radius: 3.0,
                };
            }
            let spec = SaveSpec::from_extension(file, o.source.iter().cloned().collect());
            kadakas_api::files::write_document(file, &o.doc, &spec).expect("save over");
            report("saveover", file, loaded);
        }
        _ => eprintln!(
            "usage: doc_stream_bench gen <megabytes> <file.kad|file.json> | load <file> lazy|eager|slice | save <file> <out> | saveover <file>"
        ),
    }
}
