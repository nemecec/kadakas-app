//! Damaged files never panic the import (IOP-29). Entities of every fixture (`all_kinds.ifc` has every
//! entity type that the importer reads) and of the public corpus (when cached) are damaged in two
//! ways: the attribute list is emptied, cut to its first half or set to `$`; and one attribute at a
//! time is replaced by a bad value (a missing reference, a wrong kind, a huge number, a wrong list).
//! The import must return, with a result or an error, never a panic.
//!
//! The default run damages a sample (measured: 44 to 49 s in a build without optimisation, on a busy computer). The full run damages everything:
//! `cargo test --release -p kadakas-ifc --test ifc_damage -- --ignored`.

mod support;

use std::path::PathBuf;

use kadakas_core::model::Document;
use kadakas_ifc::{IfcImportOptions, import_bytes_unguarded};

/// The statements of the data section as (start, end) byte ranges of `#id=NAME(...)`.
fn statements(text: &str) -> Vec<(usize, usize)> {
    let b = text.as_bytes();
    let Some(data) = text.find("DATA;") else {
        return Vec::new();
    };
    let (mut out, mut i, mut start, mut quote, mut comment) =
        (Vec::new(), data + 5, None, false, false);
    while i < b.len() {
        let c = b[i];
        if comment {
            if c == b'*' && b.get(i + 1) == Some(&b'/') {
                comment = false;
                i += 1;
            }
        } else if quote {
            if c == b'\'' {
                quote = false;
            }
        } else if c == b'\'' {
            quote = true;
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            comment = true;
        } else if c == b'#' && start.is_none() {
            start = Some(i);
        } else if c == b';'
            && let Some(s) = start.take()
        {
            out.push((s, i + 1));
        }
        i += 1;
    }
    out
}

/// Bad values for one attribute.
const BAD: [&str; 14] = [
    "#999999",
    "#1",
    "-1",
    "0",
    "1.E308",
    "-1.E308",
    "()",
    "(1,2,3)",
    "((1.,2.),(3.,4.))",
    "'x'",
    ".T.",
    "IFCLABEL('x')",
    "$",
    "*",
];

/// The attributes of a statement as slices of its body, with the head (`#1=NAME(`).
fn attributes(statement: &str) -> Option<(&str, Vec<&str>)> {
    let open = statement.find('(')?;
    let close = statement.rfind(')')?;
    if close < open {
        return None;
    }
    let head = &statement[..open + 1];
    let body = &statement[open + 1..close];
    let (mut parts, mut depth, mut quote, mut from) = (Vec::new(), 0i32, false, 0);
    for (i, c) in body.char_indices() {
        match c {
            '\'' => quote = !quote,
            '(' if !quote => depth += 1,
            ')' if !quote => depth -= 1,
            ',' if !quote && depth == 0 => {
                parts.push(&body[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[from..]);
    Some((head, parts))
}

/// The damaged versions of one statement. `full`: every bad value for every attribute; otherwise
/// one bad value per attribute, chosen by `salt` so that the sample as a whole uses all of them.
fn damaged(statement: &str, full: bool, salt: usize) -> Vec<String> {
    let Some((head, parts)) = attributes(statement) else {
        return Vec::new();
    };
    let half = parts[..parts.len() / 2].join(",");
    let nulls = vec!["$"; parts.len()].join(",");
    let mut out = vec![
        format!("{head});"),
        format!("{head}{half});"),
        format!("{head}{nulls});"),
    ];
    for i in 0..parts.len() {
        let values: Vec<&str> = if full {
            BAD.to_vec()
        } else {
            vec![BAD[(salt + i) % BAD.len()]]
        };
        for v in values {
            let mut p = parts.clone();
            p[i] = v;
            out.push(format!("{head}{});", p.join(",")));
        }
    }
    out
}

fn run(text: &str) -> bool {
    let doc = Document::with_sequential_ids();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ =
            import_bytes_unguarded(text.as_bytes().to_vec(), &doc, &IfcImportOptions::default());
    }))
    .is_ok()
}

fn check(name: &str, text: &str, every: usize, full: bool) -> usize {
    let mut runs = 0;
    for (n, (s, e)) in statements(text).into_iter().enumerate() {
        if n % every != 0 {
            continue;
        }
        for d in damaged(&text[s..e], full, n) {
            let mutant = format!("{}{}{}", &text[..s], d, &text[e..]);
            runs += 1;
            if std::env::var_os("DAMAGE_TRACE").is_some() {
                eprintln!("{name} #{n}: {}", d.chars().take(100).collect::<String>());
            }
            let started = std::time::Instant::now();
            let ok = run(&mutant);
            // A hang guard only (the work is bounded by counts, not by the clock).
            assert!(
                started.elapsed().as_secs() < 600,
                "{name}: {:?} for this damage: {}",
                started.elapsed(),
                d.chars().take(120).collect::<String>()
            );
            assert!(
                ok,
                "{name}: the import panicked after this damage: {}",
                d.chars().take(120).collect::<String>()
            );
        }
    }
    runs
}

fn fixtures() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/ifc");
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ifc"))
        .collect();
    v.sort();
    v
}

fn fixture_runs(stride: usize, full: bool) -> usize {
    let mut runs = 0;
    for f in fixtures() {
        let text = std::fs::read_to_string(&f).unwrap();
        runs += check(&f.display().to_string(), &text, stride, full);
    }
    runs
}

#[test]
fn damaged_fixtures_never_panic() {
    // A build without optimisation is 20 times slower: it damages a smaller sample (the unit
    // statements, with their 1000 times bigger models, take seconds each).
    let stride = if cfg!(debug_assertions) { 60 } else { 6 };
    let runs = fixture_runs(stride, false);
    assert!(runs > 100, "{runs}");
}

#[test]
#[ignore = "the full run takes about 20 minutes in a release build: cargo test --release -p kadakas-ifc --test ifc_damage -- --ignored"]
fn damaged_fixtures_never_panic_full() {
    let runs = fixture_runs(1, true);
    assert!(runs > 10_000, "{runs}");
}

#[test]
fn the_fixture_with_every_kind_imports_without_error() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/ifc/all_kinds.ifc");
    let doc = Document::with_sequential_ids();
    let r = import_bytes_unguarded(
        std::fs::read(path).unwrap(),
        &doc,
        &IfcImportOptions::default(),
    )
    .expect("imports");
    assert!(
        r.report.objects.values().sum::<usize>() > 20,
        "{:?}",
        r.report.skipped
    );
}

#[test]
fn a_trimmed_curve_without_attributes_in_an_advanced_brep_is_refused_not_a_panic() {
    // The case of the review: a plane angle unit in degrees (a conversion unit that the unit
    // assignment lists) and an empty IfcTrimmedCurve.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/ifc/advanced_brep_cylinder.ifc");
    let base = std::fs::read_to_string(path).unwrap();
    let text = base
        .replace(
            "#20=IFCUNITASSIGNMENT((#21));",
            "#20=IFCUNITASSIGNMENT((#21,#9001));\n#9002=IFCDIMENSIONALEXPONENTS(0,0,0,0,0,0,0);\n#9003=IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.);\n#9004=IFCMEASUREWITHUNIT(IFCPLANEANGLEMEASURE(0.01745329252),#9003);\n#9001=IFCCONVERSIONBASEDUNIT(#9002,.PLANEANGLEUNIT.,'DEGREE',#9004);",
        )
        .replace("#115=IFCLINE(#100,#114);", "#115=IFCTRIMMEDCURVE();");
    assert!(text.contains("#9001=") && text.contains("IFCTRIMMEDCURVE();"));
    assert!(run(&text));
}

#[test]
fn damaged_public_files_never_panic() {
    public(1, false);
}

#[test]
#[ignore = "the full run takes about 20 minutes in a release build: cargo test --release -p kadakas-ifc --test ifc_damage -- --ignored"]
fn damaged_public_files_never_panic_full() {
    public(40, true);
}

/// Damages about `per_file` statements of each cached public file.
fn public(per_file: usize, full: bool) {
    let mut seen = 0;
    for (repo, path) in support::corpus::all_ifc() {
        let Some(file) = support::corpus::need(&repo, &path) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        // The default run leaves out the big files (the full run has them).
        let small = if cfg!(debug_assertions) {
            30_000
        } else {
            200_000
        };
        if !full && text.len() > small {
            continue;
        }
        // A build without optimisation takes every fourth small file (the full run takes all).
        seen += 1;
        if !full && cfg!(debug_assertions) && seen % 4 != 1 {
            continue;
        }
        let every = (statements(&text).len() / per_file).max(1);
        check(&path, &text, every, full);
    }
    if seen == 0 {
        eprintln!("SKIP: no file of the IFC corpus is available");
    }
}
