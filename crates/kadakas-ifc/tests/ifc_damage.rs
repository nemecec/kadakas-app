//! Damaged files never panic the import (IOP-29). Statements of the fixtures (`all_kinds.ifc` has
//! every entity type that the importer reads) and of the public corpus (when cached) are damaged in
//! two ways: the list of attributes is emptied, cut to its first half or set to `$`; and one
//! attribute at a time is replaced by a bad value (a missing reference, a wrong kind, a huge
//! number, a wrong list). The import must return, with a result or an error, never a panic.
//!
//! The default run damages every entity type at least once: the first statement of each type, in
//! the smallest file that has it, with one cut of the list and one bad value. Measured: 48 s in a
//! debug build on a busy computer. The full run damages every statement in all ways; it takes
//! about 20 minutes for the fixtures in a release build:
//! `cargo test --release -p kadakas-ifc --test ifc_damage -- --ignored`.
//! No CI job runs the full run yet. It is run by hand, and a nightly step is planned for the merge
//! (see docs/dev-guide.md).

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
    let whole = [
        format!("{head});"),
        format!("{head}{half});"),
        format!("{head}{nulls});"),
    ];
    let one = |i: usize, v: &str| {
        let mut p = parts.clone();
        p[i] = v;
        format!("{head}{});", p.join(","))
    };
    if !full {
        // One cut of the list and one bad value in one attribute; the salt (the position of the
        // statement) rotates the choice, so the sample as a whole uses all of them.
        return vec![
            whole[salt % whole.len()].clone(),
            one(salt % parts.len(), BAD[salt % BAD.len()]),
        ];
    }
    let mut out = whole.to_vec();
    for i in 0..parts.len() {
        out.extend(BAD.iter().map(|v| one(i, v)));
    }
    out
}

/// The name of the entity of a statement (`#1=IFCWALL(...)` gives `IFCWALL`), upper case; a complex
/// entity `#1=(A()B())` gives `(complex)`.
fn entity_name(statement: &str) -> String {
    let Some((_, rest)) = statement.split_once('=') else {
        return String::new();
    };
    let rest = rest.trim_start();
    if rest.starts_with('(') {
        return "(complex)".into();
    }
    rest.split('(').next().unwrap_or("").trim().to_uppercase()
}

/// For each file, the statements to damage so that every entity type is damaged once: the first
/// statement of each type in the smallest file that has the type. Files are given as (name, text).
fn one_of_each_type(files: &[(String, String)]) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by_key(|i| files[*i].1.len());
    let mut seen = std::collections::HashSet::new();
    let mut plan = vec![Vec::new(); files.len()];
    for i in order {
        let text = &files[i].1;
        for (n, (s, e)) in statements(text).into_iter().enumerate() {
            if seen.insert(entity_name(&text[s..e])) {
                plan[i].push(n);
            }
        }
    }
    plan
}

type Imported = Result<kadakas_io::ImportResult, kadakas_ifc::IfcImportError>;

/// True when an import went well enough: it returned, and no product was skipped after an internal
/// error (the entry has no catch of its own, so this is a second guard in case one is added).
fn fine(result: &Imported) -> bool {
    match result {
        Ok(r) => {
            !r.report
                .skipped
                .iter()
                .any(|s| s.reason.contains("internal error"))
                && !r.report.notes.iter().any(|n| n.contains("internal error"))
                && r.report.handled_panics.is_empty()
        }
        Err(e) => !matches!(e, kadakas_ifc::IfcImportError::Internal(_)),
    }
}

/// Runs `f` and says if it returned `true`; a panic is `false`.
fn guard(f: impl FnOnce() -> bool) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(false)
}

/// True when the import of this text is fine and does not panic.
fn run(text: &str) -> bool {
    let doc = Document::with_sequential_ids();
    guard(|| {
        fine(&import_bytes_unguarded(
            text.as_bytes().to_vec(),
            &doc,
            &IfcImportOptions::default(),
        ))
    })
}

fn check(name: &str, text: &str, pick: &dyn Fn(usize) -> bool, full: bool) -> usize {
    let mut runs = 0;
    for (n, (s, e)) in statements(text).into_iter().enumerate() {
        if !pick(n) {
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

fn fixture_texts() -> Vec<(String, String)> {
    fixtures()
        .into_iter()
        .map(|f| {
            let text = std::fs::read_to_string(&f).unwrap();
            (f.display().to_string(), text)
        })
        .collect()
}

#[test]
fn every_entity_type_of_the_fixtures_is_damaged_and_the_import_returns() {
    let files = fixture_texts();
    let plan = one_of_each_type(&files);
    let types: usize = plan.iter().map(Vec::len).sum();
    // The fixtures hold every type that the importer reads (all_kinds.ifc has them all).
    assert!(types > 150, "{types} entity types");
    let mut runs = 0;
    for ((name, text), picked) in files.iter().zip(&plan) {
        runs += check(name, text, &|n| picked.contains(&n), false);
    }
    assert!(runs >= 2 * types, "{runs}");
}

#[test]
#[ignore = "the full run takes about 20 minutes in a release build: cargo test --release -p kadakas-ifc --test ifc_damage -- --ignored"]
fn damaged_fixtures_never_panic_full() {
    let mut runs = 0;
    for (name, text) in fixture_texts() {
        runs += check(&name, &text, &|_| true, true);
    }
    assert!(runs > 10_000, "{runs}");
}

#[test]
fn the_damage_check_fails_on_a_panic_and_on_an_internal_error() {
    let doc = Document::with_sequential_ids();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/ifc/extruded_rect.ifc");
    let good = import_bytes_unguarded(
        std::fs::read(path).unwrap(),
        &doc,
        &IfcImportOptions::default(),
    );
    assert!(fine(&good));
    // A panic anywhere in the call is a failure.
    assert!(guard(|| true));
    assert!(!guard(|| panic!("injected")));
    // A product skipped after an internal error, a caught panic and an `Internal` error are failures.
    let ok = good.as_ref().unwrap();
    let mut skipped = ok.clone();
    skipped
        .report
        .skip("IfcWall", "internal error in this product");
    assert!(!fine(&Ok(skipped)));
    let mut handled = ok.clone();
    handled.report.handled_panics.push("x".into());
    assert!(!fine(&Ok(handled)));
    let mut noted = ok.clone();
    noted
        .report
        .note("1 products were skipped after an internal error");
    assert!(!fine(&Ok(noted)));
    assert!(!fine(&Err(kadakas_ifc::IfcImportError::Internal(
        "x".into()
    ))));
    assert!(fine(&Err(kadakas_ifc::IfcImportError::Format("x".into()))));
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
    public(false);
}

#[test]
#[ignore = "the full run takes hours: cargo test --release -p kadakas-ifc --test ifc_damage -- --ignored"]
fn damaged_public_files_never_panic_full() {
    public(true);
}

/// Damages the cached public files: one statement of each entity type (the first, in the smallest
/// file that has it); the full run damages about 40 statements of each file in all ways.
fn public(full: bool) {
    let mut files = Vec::new();
    for (repo, path) in support::corpus::all_ifc() {
        let Some(file) = support::corpus::need(&repo, &path) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        // The default run leaves out the big files (the full run has them).
        if !full && text.len() > 200_000 {
            continue;
        }
        files.push((path, text));
    }
    if files.is_empty() {
        eprintln!("SKIP: no file of the IFC corpus is available");
        return;
    }
    if full {
        for (name, text) in &files {
            let every = (statements(text).len() / 40).max(1);
            check(name, text, &|n| n % every == 0, true);
        }
        return;
    }
    let plan = one_of_each_type(&files);
    for ((name, text), picked) in files.iter().zip(&plan) {
        check(name, text, &|n| picked.contains(&n), false);
    }
}
