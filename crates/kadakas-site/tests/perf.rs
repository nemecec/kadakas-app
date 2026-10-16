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

/// 20 000 plants over three site models on different layers: the lookup of the model must not
/// grow with the number of models or objects. Run in release mode.
#[test]
#[ignore = "timing test: run in release mode"]
fn perf_ground_under_20000_objects_with_three_site_models() {
    use glam::DVec2;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::ground::GroundSource;
    use kadakas_core::model::{Attributes, LayerId, Object, ObjectId, Shape, SiteSettings};
    let mut ed = Editor::default();
    let mut layers = Vec::new();
    for i in 0..3 {
        let id = ed
            .execute(&Command::AddLayer {
                name: format!("Site {i}"),
                elevation: i as f64 * 1000.0,
                scale: None,
                kind: None,
            })
            .unwrap()
            .created[0];
        layers.push(LayerId(id));
        let mut points = Vec::new();
        for x in 0..40 {
            for y in 0..40 {
                points.push(DVec3::new(
                    i as f64 * 400_000.0 + x as f64 * 10_000.0,
                    y as f64 * 10_000.0,
                    (x + y) as f64 * 50.0,
                ));
            }
        }
        ed.execute(&Command::AddObject {
            shape: Shape::SiteModel {
                points,
                lines: vec![],
                settings: SiteSettings::default(),
            },
            layer: Some(LayerId(id)),
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    }
    let mut seed = 99;
    let plants: Vec<DVec2> = (0..20_000)
        .map(|_| DVec2::new(rng(&mut seed) * 1_200_000.0, rng(&mut seed) * 390_000.0))
        .collect();
    let cache = SiteCache::default();
    let start = Instant::now();
    let terrain = SiteTerrain::new(&cache, ed.revision(), ed.doc());
    let o = Object::new(
        ObjectId::from_num(0),
        layers[1],
        kadakas_core::model::NONE_CLASS,
        Shape::Locus3d { p: DVec3::ZERO },
    );
    let found = plants
        .iter()
        .filter(|p| terrain.elevation(&o, **p).is_some())
        .count();
    let elapsed = start.elapsed();
    println!("20 000 lookups over 3 site models: {elapsed:?} ({found} found)");
    assert!(found > 0);
}
