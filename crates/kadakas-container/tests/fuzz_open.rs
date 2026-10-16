//! A fuzzer for `Container::open` and the read of every entry, on stable Rust (no `cargo fuzz`, no
//! nightly). It takes seed files, changes them in random ways, and opens each result. Nothing may
//! panic, no read may take long, and a read that works must give exactly the bytes that the
//! manifest says (the size and the SHA-256 are checked by the reader; the test checks the size).
//!
//! The seeds are the committed containers of `kadakas-api` (the goldens: a drawing, a picture, an
//! import base, versions), containers made here (stored, deflated, streamed with the ZIP64 field,
//! many small entries) and hostile ones (a name that leaves the container, two entries at one
//! place, an entry of zeros).
//!
//! The time is bounded: `KADAKAS_FUZZ_SECS` seconds (default 3, so an ordinary `cargo test` is
//! short). CI runs 60 s in release mode. `KADAKAS_FUZZ_SEED` repeats a run. A failing input is
//! written to the temporary folder and its path is in the message.

use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use kadakas_container::{Compression, Container, Limits, Writer};

/// xorshift64*: small, and the same on every platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

fn made(entries: &[(&str, Compression, Vec<u8>)]) -> Vec<u8> {
    let mut w = Writer::new(Vec::new()).generator("fuzz");
    for (name, how, bytes) in entries {
        w.add_bytes(name, *how, bytes).unwrap();
    }
    w.finish().unwrap()
}

fn seeds() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    // The committed containers.
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../kadakas-api/tests/data");
    if let Ok(dir) = std::fs::read_dir(data) {
        for e in dir.flatten() {
            if e.path().extension().is_some_and(|x| x == "kad")
                && let Ok(bytes) = std::fs::read(e.path())
                && bytes.starts_with(b"PK")
            {
                out.push(bytes);
            }
        }
    }
    let text = br#"{"format_version": 3, "objects": []}"#.to_vec();
    out.push(made(&[(
        "document.json",
        Compression::Deflate(1),
        text.clone(),
    )]));
    out.push(made(&[
        ("document.json", Compression::Stored, text.clone()),
        ("blobs/aa", Compression::Stored, vec![7; 5000]),
        (
            "history/top.json",
            Compression::Deflate(6),
            vec![b'x'; 3000],
        ),
    ]));
    // A streamed entry with the ZIP64 field and a 64 bit descriptor.
    let mut w = Writer::new(Vec::new());
    w.add_with_hint("document.json", Compression::Deflate(1), true, |f| {
        f.write_all(&text)
    })
    .unwrap();
    out.push(w.finish().unwrap());
    // Many small entries.
    let many: Vec<(String, Compression, Vec<u8>)> = (0..300)
        .map(|i| {
            (
                format!("blobs/{i:064}"),
                Compression::Stored,
                vec![i as u8; 3],
            )
        })
        .collect();
    let refs: Vec<(&str, Compression, Vec<u8>)> = many
        .iter()
        .map(|(n, c, b)| (n.as_str(), *c, b.clone()))
        .collect();
    out.push(made(&refs));
    // An entry of zeros that deflates very well (a bomb for a small limit).
    out.push(made(&[(
        "document.json",
        Compression::Deflate(9),
        vec![0; 2 << 20],
    )]));
    // Not containers at all, and the start of one.
    out.push(b"PK\x03\x04".to_vec());
    out.push(vec![0; 64]);
    out
}

fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut b = seed.to_vec();
    for _ in 0..1 + rng.below(6) {
        if b.is_empty() {
            b.push(rng.next() as u8);
            continue;
        }
        // Changes are likelier near the end (the end records and the directory) and at the
        // signatures of the headers.
        let at = if rng.below(3) == 0 {
            b.len() - 1 - rng.below(b.len().min(300))
        } else {
            rng.below(b.len())
        };
        match rng.below(9) {
            0 => b[at] ^= 1 << rng.below(8),
            1 => b[at] = rng.next() as u8,
            2 => b.truncate(at),
            3 => b.extend((0..rng.below(64)).map(|_| rng.next() as u8)),
            4 => {
                // A 32 bit field becomes a value that means something to a ZIP reader.
                let v = [0u32, 1, 0xFFFF, 0xFFFF_FFFF, 0x7FFF_FFFF, 0x8000_0000][rng.below(6)];
                for (i, x) in v.to_le_bytes().iter().enumerate() {
                    if at + i < b.len() {
                        b[at + i] = *x;
                    }
                }
            }
            5 => {
                let len = rng.below(b.len() - at).min(64);
                let copy = b[at..at + len].to_vec();
                let to = rng.below(b.len());
                b.splice(to..to, copy);
            }
            6 => {
                let len = rng.below(b.len() - at).min(32);
                b[at..at + len].fill(0);
            }
            7 => {
                // Another signature in a random place.
                let sig: [u8; 4] = [
                    [0x50, 0x4b, 0x03, 0x04],
                    [0x50, 0x4b, 0x01, 0x02],
                    [0x50, 0x4b, 0x05, 0x06],
                    [0x50, 0x4b, 0x06, 0x06],
                ][rng.below(4)];
                for (i, x) in sig.iter().enumerate() {
                    if at + i < b.len() {
                        b[at + i] = *x;
                    }
                }
            }
            _ => {
                let len = rng.below(b.len() - at).min(16);
                b.drain(at..at + len);
            }
        }
    }
    b
}

/// Opens the bytes and reads every entry. Returns the number of entries that were read.
fn exercise(bytes: Vec<u8>) -> usize {
    // A small limit: a bomb is refused or stops early, and an iteration stays short.
    let limits = Limits::default().with_total(8 << 20);
    let Ok(c) = Container::from_bytes(bytes, limits) else {
        return 0;
    };
    let mut read = 0;
    for e in c.entries() {
        let Ok(mut r) = c.reader(&e.name) else {
            continue;
        };
        let mut out = Vec::new();
        if r.read_to_end(&mut out).is_ok() {
            assert_eq!(out.len() as u64, e.size, "{}", e.name);
            read += 1;
        }
        let _ = c.read_prefix(&e.name, 16);
        let _ = c.raw(&e.name);
    }
    read
}

#[test]
fn changed_containers_are_errors_never_panics() {
    let secs: f64 = std::env::var("KADAKAS_FUZZ_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3.0);
    let seed: u64 = std::env::var("KADAKAS_FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64)
        })
        | 1;
    let mut rng = Rng(seed);
    let seeds = seeds();
    assert!(seeds.len() >= 6, "the seeds are there");
    let started = Instant::now();
    let (mut runs, mut opened) = (0u64, 0u64);
    while started.elapsed() < Duration::from_secs_f64(secs) {
        let base = &seeds[rng.below(seeds.len())];
        let input = mutate(&mut rng, base);
        let t = Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| exercise(input.clone())));
        let slow = t.elapsed() > Duration::from_secs(5);
        if result.is_err() || slow {
            let file = std::env::temp_dir().join(format!("kadakas-fuzz-{seed}-{runs}.bin"));
            let _ = std::fs::write(&file, &input);
            panic!(
                "{} on run {runs} (seed {seed}); the input is in {}",
                if slow {
                    "a read took too long"
                } else {
                    "a panic"
                },
                file.display()
            );
        }
        opened += u64::from(result.is_ok_and(|n| n > 0));
        runs += 1;
    }
    eprintln!("fuzz: {runs} inputs in {secs} s (seed {seed}), {opened} with an entry that read");
    assert!(runs > 0);
    // The seeds themselves read (all but the bomb and the two that are no containers).
    let good = seeds.iter().filter(|s| exercise((*s).clone()) > 0).count();
    assert!(good >= 5, "{good} seeds read");
}
