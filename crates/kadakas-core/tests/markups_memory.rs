//! REV-11: loading markups needs memory in the order of the size of the file. A counting
//! allocator measures the peak while a file with a million points loads.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: it forwards every call to the system allocator and only counts the bytes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let now = NOW.fetch_add(l.size(), Ordering::Relaxed) + l.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        NOW.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            if new >= l.size() {
                let now = NOW.fetch_add(new - l.size(), Ordering::Relaxed) + new - l.size();
                PEAK.fetch_max(now, Ordering::Relaxed);
            } else {
                NOW.fetch_sub(l.size() - new, Ordering::Relaxed);
            }
        }
        q
    }
}

#[global_allocator]
static A: Counting = Counting;

#[test]
fn a_file_with_a_million_points_loads_with_a_few_times_its_size() {
    // 100 markups of 10 marks of 1000 points: 1,000,000 points. The file text is the baseline.
    let mut text = serde_json::to_string(&kadakas_core::model::Document::default()).unwrap();
    text.pop();
    text.push_str(r#","markups":["#);
    for m in 0..100u32 {
        if m > 0 {
            text.push(',');
        }
        text.push_str(&format!(
            r#"{{"id":"00000000-0000-4000-8000-{:012}","viewpoint":{{"view":{{"kind":"plan","center":[0.0,0.0],"zoom":0.1}},"canvas":[800.0,600.0]}},"marks":["#,
            m + 1
        ));
        for k in 0..10u32 {
            if k > 0 {
                text.push(',');
            }
            text.push_str(&format!(
                r##"{{"id":{},"kind":"pen","color":"#d62828","width":3.0,"points":["##,
                k + 1
            ));
            for i in 0..1000 {
                if i > 0 {
                    text.push(',');
                }
                text.push_str(&format!(
                    "[{:.5},{:.5}]",
                    i as f64 / 1000.0,
                    (i % 97) as f64 / 97.0
                ));
            }
            text.push_str("]}");
        }
        text.push_str("]}");
    }
    text.push_str("]}");
    let size = text.len();
    let bytes = text.into_bytes();
    NOW.store(0, Ordering::Relaxed);
    PEAK.store(0, Ordering::Relaxed);
    let before = NOW.load(Ordering::Relaxed);
    let doc = kadakas_core::model::Document::from_slice(&bytes).expect("the file loads");
    let peak = PEAK.load(Ordering::Relaxed) - before.min(PEAK.load(Ordering::Relaxed));
    assert_eq!(doc.markups.point_count(), 1_000_000);
    println!(
        "file {size} bytes, peak of the load {peak} bytes ({:.1} times)",
        peak as f64 / size as f64
    );
    assert!(
        peak < 4 * size,
        "the load held {peak} bytes for a file of {size}"
    );
}
