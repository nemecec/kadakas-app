//! A fuzz test of the cutting of polygons with holes (the joining of the holes and the ear
//! clipping), with a strict check of every result. A refusal is allowed and counted. A result
//! that is wrong (triangles that overlap, that leave a hole, that look the wrong way, that
//! cross an edge of a loop) is never allowed.
//!
//! The run of the CI has 300 cases of each kind and a fixed seed. The full run has 10,000:
//! `cargo test --release -p kadakas-step --test fuzz_cut -- --ignored --nocapture`.

#[path = "fuzz_cut/check.rs"]
mod check;
#[path = "fuzz_cut/shapes.rs"]
mod shapes;

use std::time::Instant;

use check::Verdict;
use kadakas_step::cutting::cut;

/// The most steps of the cutting that a case may take.
const FUEL: u64 = 2_000_000_000;

struct Row {
    kind: &'static str,
    valid: usize,
    wrong: usize,
    refused: usize,
    first_wrong: Option<(u64, String)>,
    first_refused: Option<u64>,
}

fn run(kind: &'static str, cases: u64) -> Row {
    let started = Instant::now();
    let mut row = Row {
        kind,
        valid: 0,
        wrong: 0,
        refused: 0,
        first_wrong: None,
        first_refused: None,
    };
    for seed in 0..cases {
        let case = shapes::make(kind, seed);
        match check::check(&case, cut(&case.uv, &case.loops, FUEL)) {
            Verdict::Valid => row.valid += 1,
            Verdict::Refused => {
                row.refused += 1;
                row.first_refused.get_or_insert(seed);
            }
            Verdict::Wrong(why) => {
                row.wrong += 1;
                row.first_wrong.get_or_insert((seed, why));
            }
        }
    }
    println!("  {kind}: {:?}", started.elapsed());
    row
}

fn table(cases: u64) -> Vec<Row> {
    let start = Instant::now();
    let rows: Vec<Row> = shapes::KINDS.iter().map(|k| run(k, cases)).collect();
    println!(
        "{:<24} {:>8} {:>8} {:>8} {:>8}",
        "kind", "cases", "valid", "wrong", "refused"
    );
    for r in &rows {
        println!(
            "{:<24} {:>8} {:>8} {:>8} {:>8}   {:.2}% valid{}",
            r.kind,
            cases,
            r.valid,
            r.wrong,
            r.refused,
            100.0 * r.valid as f64 / cases as f64,
            match (&r.first_wrong, r.first_refused) {
                (Some((seed, _)), _) => format!("   first wrong: seed {seed}"),
                (None, Some(seed)) => format!("   first refused: seed {seed}"),
                _ => String::new(),
            }
        );
    }
    println!("{:?}", start.elapsed());
    rows
}

/// The lowest share of valid results that each kind must keep: what the cutting does now,
/// less a little. A drop is a regression. A wrong result is a failure whatever the share.
fn floor(kind: &str) -> f64 {
    match kind {
        // Faces that are not one piece, or that touch themselves, may be refused.
        "touch_two_places" | "degenerate_selftouch" | "degenerate_spike" => 0.0,
        "touch_outline_edge" => 0.99,
        _ => 0.95,
    }
}

fn assert_clean(rows: &[Row], cases: u64) {
    for r in rows {
        assert!(
            r.wrong == 0,
            "{}: {} wrong, the first at seed {}: {}",
            r.kind,
            r.wrong,
            r.first_wrong.as_ref().unwrap().0,
            r.first_wrong.as_ref().unwrap().1
        );
        let share = r.valid as f64 / cases as f64;
        assert!(
            share >= floor(r.kind),
            "{}: only {:.1}% valid, the first refusal at seed {:?}",
            r.kind,
            100.0 * share,
            r.first_refused
        );
    }
}

#[test]
fn three_hundred_cases_of_every_kind_are_cut_right_or_refused() {
    assert_clean(&table(300), 300);
}

#[test]
#[ignore = "the full run: 10,000 cases of every kind"]
fn ten_thousand_cases_of_every_kind_are_cut_right_or_refused() {
    assert_clean(&table(10_000), 10_000);
}

#[test]
fn the_cases_of_the_reviews_are_cut_right() {
    use shapes::Case;
    let square = |w: f64| vec![[0.0, 0.0], [w, 0.0], [w, w], [0.0, w]];
    let mk = |outline: Vec<[f64; 2]>, holes: Vec<Vec<[f64; 2]>>| {
        let mut uv = outline.clone();
        let mut loops = vec![(0..outline.len()).collect::<Vec<usize>>()];
        for h in holes {
            let base = uv.len();
            loops.push((base..base + h.len()).collect());
            uv.extend(h);
        }
        Case {
            name: "review",
            uv,
            loops,
            tol: 1e-9,
        }
    };
    let cases = vec![
        // Two holes touching the same edge of the outline.
        mk(
            square(8.0),
            vec![
                vec![[6.0, 8.0], [6.5, 7.0], [5.0, 7.0]],
                vec![[2.0, 8.0], [2.5, 7.0], [1.0, 7.0]],
            ],
        ),
        // A hole bridged normally, then one that touches the edge.
        mk(
            square(8.0),
            vec![
                vec![[6.0, 6.0], [6.0, 7.5], [7.0, 7.5], [7.0, 6.0]],
                vec![[2.0, 8.0], [2.5, 7.0], [1.0, 7.0]],
            ],
        ),
        mk(
            square(8.0),
            vec![
                vec![[6.0, 6.0], [6.0, 7.5], [7.0, 7.5], [7.0, 6.0]],
                vec![[6.5, 8.0], [6.9, 7.8], [6.1, 7.8]],
            ],
        ),
        // The corner of the hole on a straight corner of the outline.
        mk(
            vec![
                [0.0, 0.0],
                [30.25, 0.0],
                [100.0, 0.0],
                [100.0, 40.75],
                [100.0, 100.0],
                [0.0, 100.0],
            ],
            vec![vec![[90.96, 36.23], [90.96, 45.27], [100.0, 40.75]]],
        ),
    ];
    for c in cases {
        match check::check(&c, cut(&c.uv, &c.loops, FUEL)) {
            Verdict::Valid => {}
            Verdict::Refused => panic!("a case of the reviews is refused"),
            Verdict::Wrong(why) => panic!("wrong: {why}"),
        }
    }
}

#[test]
#[ignore = "prints the first wrong result of every kind"]
fn show_wrong() {
    for kind in shapes::KINDS {
        let row = run(kind, 300);
        if let Some((seed, why)) = &row.first_wrong {
            println!("{kind}: seed {seed}: {why}");
        }
    }
}

/// Cuts the holes of a case away one at a time while the result is still wrong, and prints
/// what is left: `FUZZ_KIND=concave_nested FUZZ_SEED=0 cargo test ... minimise -- --ignored --nocapture`.
#[test]
#[ignore = "a tool for finding the smallest case that is wrong"]
fn minimise() {
    let kind = std::env::var("FUZZ_KIND").unwrap_or_else(|_| "concave_nested".into());
    let seed: u64 = std::env::var("FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let kind: &'static str = shapes::KINDS
        .iter()
        .copied()
        .find(|k| *k == kind)
        .expect("a kind");
    let mut case = shapes::make(kind, seed);
    let bad = |c: &shapes::Case| {
        matches!(
            check::check(c, cut(&c.uv, &c.loops, FUEL)),
            Verdict::Wrong(_)
        )
    };
    let refused = |c: &shapes::Case| {
        matches!(
            check::check(c, cut(&c.uv, &c.loops, FUEL)),
            Verdict::Refused
        )
    };
    let want_bad = bad(&case);
    let same = |c: &shapes::Case| if want_bad { bad(c) } else { refused(c) };
    assert!(same(&case), "the case is neither wrong nor refused");
    let mut i = 1;
    while i < case.loops.len() {
        let mut c2 = shapes::Case {
            name: case.name,
            uv: case.uv.clone(),
            loops: case.loops.clone(),
            tol: case.tol,
        };
        c2.loops.remove(i);
        if same(&c2) {
            case = c2;
        } else {
            i += 1;
        }
    }
    println!(
        "{} with {} holes left",
        if want_bad { "wrong" } else { "refused" },
        case.loops.len() - 1
    );
    for l in &case.loops {
        println!("  {:?}", l.iter().map(|&i| case.uv[i]).collect::<Vec<_>>());
    }
    println!("--- the last run");
    if let Verdict::Wrong(why) = check::check(&case, cut(&case.uv, &case.loops, FUEL)) {
        println!("{why}");
    }
}
