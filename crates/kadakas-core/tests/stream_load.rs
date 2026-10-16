//! DOC-19: a document loads from a buffered reader and saves through a buffered writer, and the
//! memory that this needs is far below the size of the file.
//!
//! The test binary counts every allocation (`Counting`). The figures are bytes, not time. All
//! tests take `LOCK`, because the counters are global.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::{BufReader, BufWriter, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use glam::DVec2;
use kadakas_core::model::{Document, FIRST_LAYER, LoadError, NONE_CLASS, Object, ObjectId, Shape};
use kadakas_core::underlay::{PngData, Underlay};

struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grow(n: usize) {
    let now = CURRENT.fetch_add(n, Relaxed) + n;
    PEAK.fetch_max(now, Relaxed);
}

// SAFETY: every call goes to the system allocator. The counters only watch.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        grow(l.size());
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        CURRENT.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        if new >= l.size() {
            grow(new - l.size());
        } else {
            CURRENT.fetch_sub(l.size() - new, Relaxed);
        }
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

static LOCK: Mutex<()> = Mutex::new(());

/// What `run` allocated at most, and what it still held at the end, both above the start.
struct Use<T> {
    peak: usize,
    kept: usize,
    value: T,
}

fn measure<T>(run: impl FnOnce() -> T) -> Use<T> {
    let start = CURRENT.load(Relaxed);
    PEAK.store(start, Relaxed);
    let value = run();
    Use {
        peak: PEAK.load(Relaxed).saturating_sub(start),
        kept: CURRENT.load(Relaxed).saturating_sub(start),
        value,
    }
}

/// `objects` circles and one underlay picture (PNG signature, then `picture_bytes` bytes).
fn big_document(objects: u64, picture_bytes: usize) -> Document {
    let mut doc = Document::with_sequential_ids();
    for n in 0..objects {
        let shape = Shape::Circle {
            center: DVec2::new(n as f64, (n % 977) as f64),
            radius: 5.0 + (n % 13) as f64,
        };
        let o = Object::new(ObjectId::from_num(10 + n), FIRST_LAYER, NONE_CLASS, shape);
        doc.objects.insert(o.id, o);
    }
    let mut png = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    png.extend((0..picture_bytes).map(|i| (i.wrapping_mul(2_654_435_761) >> 7) as u8));
    let picture = Underlay {
        name: "survey".into(),
        min: DVec2::ZERO,
        max: DVec2::new(1000.0, 1000.0),
        width: 100,
        height: 100,
        png: PngData::new(png),
        opacity: 1.0,
        source: String::new(),
    };
    let id = ObjectId::from_num(10 + objects);
    let o = Object::new(
        id,
        FIRST_LAYER,
        NONE_CLASS,
        Shape::Underlay(Box::new(picture)),
    );
    doc.objects.insert(id, o);
    doc
}

/// A temporary file that is deleted when the test ends, also when it fails.
struct TempFile(std::path::PathBuf);

impl TempFile {
    fn new(name: &str) -> TempFile {
        TempFile(std::env::temp_dir().join(format!("kad-stream-{name}-{}.kad", std::process::id())))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Saves and loads a document of this size and checks the memory. With `old_way`, it also runs
/// the old code (the whole text, then a `Value`) and checks that the new load needs a third of its
/// transient memory.
fn check_memory(name: &str, objects: u64, picture_bytes: usize, old_way: bool) {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let file = TempFile::new(name);
    let path = &file.0;
    let doc = big_document(objects, picture_bytes);

    // Save: the bytes go from the serializer to the file. Nothing near the file size is held.
    let save = measure(|| {
        let mut w = BufWriter::new(std::fs::File::create(path).unwrap());
        doc.write_pretty(&mut w).unwrap();
        w.flush().unwrap();
    });
    let file_len = std::fs::metadata(path).unwrap().len() as usize;
    assert!(file_len > picture_bytes, "{file_len}");
    println!(
        "{name} save: file {file_len} bytes, peak extra {} bytes",
        save.peak
    );
    assert!(save.peak < 1024 * 1024, "the save held {} bytes", save.peak);

    // Load: the reader feeds the typed parser. What is not kept in the document is the extra memory.
    let load = measure(|| {
        let file = std::fs::File::open(path).unwrap();
        Document::from_reader(BufReader::new(file)).unwrap()
    });
    let transient = load.peak - load.kept;
    println!(
        "{name} load: file {file_len} bytes, peak extra {} bytes, kept by the document {} bytes, transient {transient} bytes",
        load.peak, load.kept
    );
    assert_eq!(load.value, doc);
    assert_eq!(load.value.objects.len(), objects as usize + 1);
    // The transient memory is below half of the file. It is the buffer of the parser for the one
    // text of the picture.
    assert!(
        transient < file_len / 2,
        "the load held {transient} bytes besides the document; the file has {file_len}"
    );

    // The same bytes in memory.
    let bytes = std::fs::read(path).unwrap();
    let slice = measure(|| Document::from_slice(&bytes).unwrap());
    assert_eq!(slice.value, doc);
    println!(
        "{name} from_slice: transient {} bytes",
        slice.peak - slice.kept
    );

    if old_way {
        let old_save = measure(|| serde_json::to_string_pretty(&doc).unwrap().len());
        println!("{name} save, before: peak extra {} bytes", old_save.peak);
        assert_eq!(old_save.value, file_len);
        assert!(old_save.peak > file_len);
        let old = measure(|| {
            let text = std::fs::read_to_string(path).unwrap();
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            drop(text);
            Document::from_value(value).unwrap()
        });
        let old_transient = old.peak - old.kept;
        println!(
            "{name} load, before (read_to_string + Value): peak extra {} bytes, transient {old_transient} bytes",
            old.peak
        );
        assert_eq!(old.value, doc);
        assert!(
            transient * 3 < old_transient,
            "{transient} vs {old_transient}"
        );
    }
}

/// The same checks as the big test, with a small document (20k objects and a 2 MB picture).
#[test]
fn a_small_document_streams_through_a_small_buffer() {
    check_memory("small", 20_000, 2_000_000, false);
}

/// 200k objects and a 20 MB picture, and the comparison with the old code. It takes a minute
/// in a debug build: the perf job of CI runs it (`--ignored`).
#[test]
#[ignore = "large: takes about a minute in a debug build"]
fn a_large_document_streams_through_a_small_buffer() {
    check_memory("large", 200_000, 20_000_000, true);
}

#[test]
fn saved_bytes_equal_the_pretty_text_of_serde_json() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut doc = Document::with_sequential_ids();
    for n in 0..50 {
        let shape = Shape::Circle {
            center: DVec2::new(n as f64, 0.5),
            radius: 1.0,
        };
        let o = Object::new(ObjectId::from_num(10 + n), FIRST_LAYER, NONE_CLASS, shape);
        doc.objects.insert(o.id, o);
    }
    // A picture with a length that is not a multiple of 3 (padding) and one with a multiple of it.
    for (n, len) in [(100u64, 3 * 1024 * 2 + 1), (101, 3 * 1024 * 2), (102, 5)] {
        let mut png = vec![0x89, b'P', b'N', b'G'];
        png.extend((0..len).map(|i| i as u8));
        let u = Underlay {
            name: String::new(),
            min: DVec2::ZERO,
            max: DVec2::ONE,
            width: 1,
            height: 1,
            png: PngData::new(png),
            opacity: 1.0,
            source: String::new(),
        };
        let o = Object::new(
            ObjectId::from_num(n),
            FIRST_LAYER,
            NONE_CLASS,
            Shape::Underlay(Box::new(u)),
        );
        doc.objects.insert(o.id, o);
    }
    let mut bytes = Vec::new();
    doc.write_pretty(&mut bytes).unwrap();
    assert_eq!(
        bytes,
        serde_json::to_string_pretty(&doc).unwrap().into_bytes()
    );
    let mut compact = Vec::new();
    doc.write_compact(&mut compact).unwrap();
    assert_eq!(compact, serde_json::to_vec(&doc).unwrap());
    // The old text of the picture: one base64 string.
    let value = serde_json::to_value(&doc).unwrap();
    let text = value["objects"][50]["shape"]["png"].as_str().unwrap();
    assert_eq!(
        text,
        kadakas_core::underlay::base64_encode(&{
            let mut png = vec![0x89, b'P', b'N', b'G'];
            png.extend((0..3 * 1024 * 2 + 1).map(|i| i as u8));
            png
        })
    );
    // The bytes load again.
    assert_eq!(
        Document::from_json(std::str::from_utf8(&bytes).unwrap()).unwrap(),
        doc
    );
}

#[test]
fn a_failing_writer_is_an_error_not_a_panic() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    struct Full(usize);
    impl Write for Full {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            if self.0 < b.len() {
                return Err(std::io::Error::other("disk full"));
            }
            self.0 -= b.len();
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let doc = Document::with_sequential_ids();
    let e = doc.write_pretty(Full(100)).unwrap_err();
    assert!(e.to_string().contains("disk full"), "{e}");
    assert!(doc.write_compact(Full(10)).is_err());
}

#[test]
fn hostile_input_gives_a_clear_error() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut good = Vec::new();
    Document::with_sequential_ids()
        .write_pretty(&mut good)
        .unwrap();
    // Every cut of a good file is an error that says what is wrong, and no cut panics.
    for n in 0..good.len() {
        let e = Document::from_reader(std::io::Cursor::new(&good[..n])).unwrap_err();
        assert!(matches!(e, LoadError::Parse(_)), "cut at {n}: {e:?}");
        assert!(!e.to_string().is_empty());
    }
    assert!(Document::from_reader(std::io::Cursor::new(&good)).is_ok());

    // A file of 100 000 nested arrays stops at the nesting limit of the parser, with no stack overflow.
    let deep = "[".repeat(100_000);
    let e = Document::from_json(&deep).unwrap_err();
    assert!(matches!(e, LoadError::Parse(_)), "{e:?}");
    let deep_field = format!(r#"{{"format_version":3,"x":{}"#, "[".repeat(100_000));
    assert!(matches!(
        Document::from_json(&deep_field),
        Err(LoadError::Parse(_))
    ));

    let cases: [&[u8]; 8] = [
        b"",
        b"   ",
        b"\0\0\0\0",
        b"\xff\xfe{\"format_version\":3}",
        b"{\"format_version\":3}{\"format_version\":3}",
        b"{\"format_version\":-1,\"layers\":[]}",
        b"{\"format_version\":3,\"layers\":1e999999}",
        b"{\"format_version\":3,\"objects\":[null]}",
    ];
    for c in cases {
        let e = Document::from_reader(std::io::Cursor::new(c)).unwrap_err();
        assert!(!e.to_string().is_empty(), "{c:?}");
    }
    // A version of the wrong kind is a clear error, not a complaint about a later field.
    let e = Document::from_json(r#"{"format_version":"3"}"#).unwrap_err();
    assert_eq!(e, LoadError::Parse("missing format_version".into()));
    let e = Document::from_json(r#"{"format_version":99,"layers":{"a":[1,2,3]},"junk":"x"}"#)
        .unwrap_err();
    assert_eq!(e, LoadError::Newer(99));
}
