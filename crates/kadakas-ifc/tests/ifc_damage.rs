//! Damaged files never panic the import (IOP-29). Every entity of every fixture (and of the small
//! files of the public corpus when they are cached) is damaged in three ways: its attributes are
//! emptied, cut to the first half, and all set to `$`. The import must return, with a result or an
//! error, never a panic.

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
        } else if c == b';' {
            if let Some(s) = start.take() {
                out.push((s, i + 1));
            }
        }
        i += 1;
    }
    out
}

/// The three damaged versions of one statement.
fn damaged(statement: &str) -> Vec<String> {
    let Some(open) = statement.find('(') else {
        return Vec::new();
    };
    let Some(close) = statement.rfind(')') else {
        return Vec::new();
    };
    let head = &statement[..open + 1];
    let body = &statement[open + 1..close];
    // Split the attributes at the commas outside of brackets and strings.
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
    let half = parts[..parts.len() / 2].join(",");
    let nulls = vec!["$"; parts.len()].join(",");
    vec![
        format!("{head});"),
        format!("{head}{half});"),
        format!("{head}{nulls});"),
    ]
}

fn run(text: &str) -> bool {
    let doc = Document::with_sequential_ids();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ =
            import_bytes_unguarded(text.as_bytes().to_vec(), &doc, &IfcImportOptions::default());
    }))
    .is_ok()
}

fn check(name: &str, text: &str, every: usize) -> usize {
    let mut runs = 0;
    for (n, (s, e)) in statements(text).into_iter().enumerate() {
        if n % every != 0 {
            continue;
        }
        for d in damaged(&text[s..e]) {
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

#[test]
fn damaged_fixtures_never_panic() {
    let mut runs = 0;
    for f in fixtures() {
        let text = std::fs::read_to_string(&f).unwrap();
        runs += check(&f.display().to_string(), &text, 1);
    }
    assert!(runs > 1000, "{runs}");
}

#[test]
fn a_trimmed_curve_without_attributes_in_an_advanced_brep_is_refused_not_a_panic() {
    // The case of the review: a plane angle unit in degrees and an empty IfcTrimmedCurve.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/ifc/advanced_brep_cylinder.ifc");
    let text = std::fs::read_to_string(path).unwrap().replace(
        "#21=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);",
        "#21=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);\n#22=IFCCONVERSIONBASEDUNIT($,.PLANEANGLEUNIT.,'DEGREE',$);",
    ).replace("#115=IFCLINE(#100,#114);", "#115=IFCTRIMMEDCURVE();");
    assert!(run(&text));
}

#[test]
fn damaged_public_files_never_panic() {
    let mut seen = 0;
    for (repo, path) in support::corpus::all_ifc() {
        let Some(file) = support::corpus::need(&repo, &path) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        // About 10 damaged statements per file (the fixtures test damages every entity).
        let every = (statements(&text).len() / 10).max(1);
        check(&path, &text, every);
        seen += 1;
    }
    if seen == 0 {
        eprintln!("SKIP: no file of the IFC corpus is available");
    }
}
