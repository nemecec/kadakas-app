//! Timing tests. Run with `cargo test -p kadakas-site --release -- --ignored --nocapture`.

use std::time::Instant;

use glam::DVec3;
use kadakas_site::*;

/// A small deterministic random generator (no extra crate).
fn rng(seed: &mut u64) -> f64 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*seed >> 11) as f64) / ((1u64 << 53) as f64)
}

fn random_terrain(n: usize) -> Vec<DVec3> {
    let mut seed = 12345;
    (0..n)
        .map(|_| {
            let (x, y) = (rng(&mut seed) * 500_000.0, rng(&mut seed) * 500_000.0);
            DVec3::new(
                x,
                y,
                (x * 0.00002).sin() * 3000.0 + (y * 0.00003).cos() * 2000.0,
            )
        })
        .collect()
}

#[test]
#[ignore = "timing test: run in release mode"]
fn tin_of_50k_points_in_under_a_second() {
    let pts = random_terrain(50_000);
    let t = Instant::now();
    let tin = Tin::build(&SourceData {
        points: pts,
        lines: vec![],
    })
    .unwrap();
    let build = t.elapsed();
    let t = Instant::now();
    let cs = contours(
        &tin,
        &ContourSpec {
            minor_interval: 250.0,
            major_interval: 1250.0,
            datum: 0.0,
            smooth: false,
        },
    )
    .unwrap();
    let contour = t.elapsed();
    let t = Instant::now();
    let r = cut_fill(&tin, &tin);
    let volume = t.elapsed();
    println!(
        "TIN of 50000 points: {build:?}, {} triangles; contours: {contour:?} ({} lines); cut/fill: {volume:?} (area {:.0})",
        tin.triangles.len(),
        cs.len(),
        r.area
    );
    assert!(build.as_secs_f64() < 1.0, "TIN took {build:?}");
}

#[test]
fn tin_of_5k_points_builds_in_any_mode() {
    let tin = Tin::build(&SourceData {
        points: random_terrain(5_000),
        lines: vec![],
    })
    .unwrap();
    assert!(tin.triangles.len() > 5_000);
}
