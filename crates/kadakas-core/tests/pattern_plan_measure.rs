//! Measurements that set the bounds of regions from loose curves, zones, courses and the check
//! (PAT-06 to PAT-10). Run in release mode:
//!
//! `cargo test -p kadakas-core --release --test pattern_plan_measure -- --ignored --nocapture`
//!
//! Timing is not asserted (the tests must not depend on the speed of the machine); the counts are.

use std::time::Instant;

use kadakas_core::DVec2;
use kadakas_core::arrangement::Curve;
use kadakas_core::curve::{Vertex, VertexKind};
use kadakas_core::layout::check::{CheckOptions, check};
use kadakas_core::layout::course::{self, Corner, Side};
use kadakas_core::layout::plan::{Body, Group, Owner, Role};
use kadakas_core::layout::summary::{SummaryOptions, summarize};
use kadakas_core::layout::{Bond, Pattern, Placement, Region, zones};
use kadakas_core::regions;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// A small deterministic generator, so that the numbers are the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f64) / ((1u64 << 31) as f64)
    }
}

/// A street grid of n by n blocks of 5 m, every edge a separate line with a gap of up to 0.45 mm
/// at each end (so that the gap between two neighbours is under the tolerance of 1 mm).
fn grid_lines(n: usize) -> Vec<Curve> {
    let mut rng = Rng(7);
    let s = 5000.0;
    let mut out = Vec::new();
    for j in 0..=n {
        for i in 0..n {
            let (a, b) = (
                v(i as f64 * s, j as f64 * s),
                v((i + 1) as f64 * s, j as f64 * s),
            );
            let g = (rng.next() * 0.45, rng.next() * 0.45);
            out.push(Curve::line(a + v(g.0, 0.0), b - v(g.1, 0.0)));
        }
    }
    for i in 0..=n {
        for j in 0..n {
            let (a, b) = (
                v(i as f64 * s, j as f64 * s),
                v(i as f64 * s, (j + 1) as f64 * s),
            );
            let g = (rng.next() * 0.45, rng.next() * 0.45);
            out.push(Curve::line(a + v(0.0, g.0), b - v(0.0, g.1)));
        }
    }
    out
}

#[test]
#[ignore = "timing test: run in release mode"]
fn measure_from_curves() {
    for n in [10usize, 35, 70, 100, 140, 315] {
        let curves = grid_lines(n);
        let t = Instant::now();
        let found = regions::from_curves(&curves, 1.0).unwrap();
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "from_curves: {} segments -> {} regions, {} closed gaps, {} dead ends, {:.0} ms",
            curves.len(),
            found.regions.len(),
            found.closed_gap_count,
            found.dead_end_count,
            ms
        );
        assert_eq!(found.regions.len(), n * n);
        assert_eq!(found.dead_end_count, 0);
    }
    // One more row of lines is over the bound of 200,000 curves.
    let too_many = grid_lines(316);
    assert!(too_many.len() > kadakas_core::arrangement::MAX_CURVES);
    assert!(regions::from_curves(&too_many, 1.0).is_err());
}

fn lot(w: f64, h: f64) -> Region {
    Region::from_rings(&[v(0.0, 0.0), v(w, 0.0), v(w, h), v(0.0, h)], &[]).unwrap()
}

fn group(name: &str, region: Region, angle: f64) -> Group {
    Group {
        name: name.into(),
        slug: name.to_uppercase().replace(' ', "-"),
        role: Role::Zone,
        region,
        owner: None,
        body: Body::Pattern {
            pattern: Pattern::new(Bond::RunningBond, 200.0, 100.0, 5.0),
            place: Placement {
                origin: Some(v(0.0, 0.0)),
                angle,
            },
        },
    }
}

#[test]
#[ignore = "timing test: run in release mode"]
fn measure_zones() {
    // A lot of 100 m by 50 m (5,000 m2) in 20 zones: 4 lines one way, 3 the other.
    let r = lot(100_000.0, 50_000.0);
    let mut lines: Vec<Vec<DVec2>> = Vec::new();
    for k in 1..5 {
        lines.push(vec![
            v(k as f64 * 20_000.0, -100.0),
            v(k as f64 * 20_000.0, 50_100.0),
        ]);
    }
    for k in 1..4 {
        lines.push(vec![
            v(-100.0, k as f64 * 12_500.0),
            v(100_100.0, k as f64 * 12_500.0),
        ]);
    }
    let t = Instant::now();
    let s = zones::split(&r, &lines, 1.0).unwrap();
    println!(
        "zones: {} lines -> {} zones in {:.2} ms",
        lines.len(),
        s.zones.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    assert_eq!(s.zones.len(), 20);
    let sum: f64 = s.zones.iter().map(|z| z.area).sum();
    assert_eq!(sum, r.area);
    // Many lines: a fine grid of 100 by 100 lines.
    let mut many: Vec<Vec<DVec2>> = Vec::new();
    for k in 1..100 {
        many.push(vec![
            v(k as f64 * 1000.0, -100.0),
            v(k as f64 * 1000.0, 50_100.0),
        ]);
        many.push(vec![
            v(-100.0, k as f64 * 500.0),
            v(100_100.0, k as f64 * 500.0),
        ]);
    }
    let t = Instant::now();
    let s = zones::split(&r, &many, 1.0).unwrap();
    println!(
        "zones: {} lines -> {} zones in {:.0} ms",
        many.len(),
        s.zones.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    assert_eq!(s.zones.len(), 99 * 99 + 99 + 99 + 1);
    // The 10,000 small zones laid, one group each.
    let t = Instant::now();
    let mut units = 0;
    for z in &s.zones {
        let g = group(
            "z",
            Region::from_shapes(vec![z.shape.clone()]).unwrap(),
            0.0,
        );
        let sum = g.summarize(&SummaryOptions::default()).unwrap();
        units += sum.whole_units + sum.cut_units;
    }
    println!(
        "zones laid: {} zones, {} units in {:.0} ms",
        s.zones.len(),
        units,
        t.elapsed().as_secs_f64() * 1000.0
    );
    // The 20 zones laid: counts of the zones against the whole lot.
    let s = zones::split(&r, &lines, 1.0).unwrap();
    let opts = SummaryOptions::default();
    let t = Instant::now();
    let whole = summarize(
        &r,
        &Pattern::new(Bond::RunningBond, 200.0, 100.0, 5.0),
        &Placement {
            origin: Some(v(0.0, 0.0)),
            angle: 0.0,
        },
        &opts,
    )
    .unwrap();
    let t_whole = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let mut units = 0;
    let mut laid = 0.0;
    for (i, z) in s.zones.iter().enumerate() {
        let g = group(
            &format!("zone {i}"),
            Region::from_shapes(vec![z.shape.clone()]).unwrap(),
            0.0,
        );
        let sum = g.summarize(&opts).unwrap();
        units += sum.whole_units + sum.cut_units;
        laid += sum.area_laid_m2;
    }
    println!(
        "zones laid: {} units in 20 zones in {:.0} ms ({} units and {:.0} ms as one area; area laid {:.2} against {:.2} m2)",
        units,
        t.elapsed().as_secs_f64() * 1000.0,
        whole.whole_units + whole.cut_units,
        t_whole,
        laid,
        whole.area_laid_m2
    );
}

#[test]
#[ignore = "timing test: run in release mode"]
fn measure_courses() {
    let spec = |path: Vec<Vertex>| course::Spec {
        path,
        closed: false,
        side: Side::Left,
        offset: 0.0,
        along: 100.0,
        depth: 200.0,
        gap: 5.0,
        corner: Corner::Miter,
    };
    // 1 km straight.
    let t = Instant::now();
    let c = course::lay(
        &spec(vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::corner(v(1_000_000.0, 0.0)),
        ]),
        1_000_000,
    )
    .unwrap();
    println!(
        "course: 1 km straight -> {} units in {:.0} ms",
        c.pieces.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    // 1 km with 20 corners.
    let mut pts = vec![Vertex::corner(v(0.0, 0.0))];
    for k in 1..=20 {
        let x = k as f64 * 50_000.0;
        let y = if k % 2 == 0 { 0.0 } else { 20_000.0 };
        pts.push(Vertex::corner(v(x - 25_000.0, y)));
    }
    let t = Instant::now();
    let c = course::lay(&spec(pts), 1_000_000).unwrap();
    println!(
        "course: 20 corners -> {} units in {:.0} ms",
        c.pieces.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    // 1 km of arc: a half circle.
    let r = 1_000_000.0 / std::f64::consts::PI;
    let arc = |cx: f64, up: bool| {
        let s = if up { 1.0 } else { -1.0 };
        vec![
            Vertex::corner(v(cx - r, 0.0)),
            Vertex::new(v(cx, s * r), VertexKind::PointOnArc),
            Vertex::corner(v(cx + r, 0.0)),
        ]
    };
    let t = Instant::now();
    let c = course::lay(&spec(arc(0.0, true)), 1_000_000).unwrap();
    println!(
        "course: 1 km of arc (radius {:.0} m) -> {} units, joints {:.2} to {:.2} mm, in {:.0} ms",
        r / 1000.0,
        c.pieces.len(),
        c.joints.narrowest,
        c.joints.widest,
        t.elapsed().as_secs_f64() * 1000.0
    );
    // A tight bend: radius 3 m.
    let tight = vec![
        Vertex::corner(v(3000.0, 0.0)),
        Vertex::new(v(0.0, 3000.0), VertexKind::PointOnArc),
        Vertex::corner(v(-3000.0, 0.0)),
    ];
    let c = course::lay(&spec(tight), 1_000_000).unwrap();
    println!(
        "course: radius 3 m -> {} units, joints {:.2} to {:.2} mm",
        c.pieces.len(),
        c.joints.narrowest,
        c.joints.widest
    );
}

#[test]
#[ignore = "timing test: run in release mode"]
fn measure_check() {
    // The lot of 5,000 m2 with 200 x 100 mm pavers: about 233,000 units, as one area and as 20 zones.
    let r = lot(100_000.0, 50_000.0);
    let one = vec![group("field", r.clone(), 0.0)];
    let t = Instant::now();
    let rep = check(&one, &r, &CheckOptions::default()).unwrap();
    println!(
        "check: {} units in {:.2} s; findings {:?}",
        rep.units,
        t.elapsed().as_secs_f64(),
        rep.counts
    );
    let t = Instant::now();
    let rep = check(
        &one,
        &r,
        &CheckOptions {
            ..Default::default()
        },
    )
    .unwrap();
    let _ = rep;
    println!("check again: {:.2} s", t.elapsed().as_secs_f64());
    // At 45 degrees every unit has boxes that overlap its neighbours.
    let turned = vec![group("field", r.clone(), 45.0)];
    let t = Instant::now();
    let rep = check(&turned, &r, &CheckOptions::default()).unwrap();
    println!(
        "check at 45 degrees: {} units in {:.2} s; findings {:?}",
        rep.units,
        t.elapsed().as_secs_f64(),
        rep.counts
    );
}

#[test]
#[ignore = "timing test: run in release mode"]
fn measure_centroid_zones() {
    // A lot of 90 m by 45 m (about 200,000 units of 200 x 100 mm) in 20 x 15 = 300 zones that
    // give each unit to the zone of its center.
    let (w, h) = (90_000.0, 45_000.0);
    let r = lot(w, h);
    let mut lines: Vec<Vec<DVec2>> = Vec::new();
    for k in 1..20 {
        lines.push(vec![
            v(k as f64 * w / 20.0, -100.0),
            v(k as f64 * w / 20.0, h + 100.0),
        ]);
    }
    for k in 1..15 {
        lines.push(vec![
            v(-100.0, k as f64 * h / 15.0),
            v(w + 100.0, k as f64 * h / 15.0),
        ]);
    }
    let s = zones::split(&r, &lines, 1.0).unwrap();
    assert_eq!(s.zones.len(), 300);
    let regions: Vec<Region> = s
        .zones
        .iter()
        .map(|z| Region::from_shapes(vec![z.shape.clone()]).unwrap())
        .collect();
    let first = Owner::new(0, std::sync::Arc::new(regions));
    let groups: Vec<Group> = (0..300)
        .map(|i| {
            let mut g = group(&format!("zone {i}"), r.clone(), 0.0);
            g.owner = Some(first.sibling(i));
            g
        })
        .collect();
    let opts = SummaryOptions::default();
    let t = Instant::now();
    let mut units = 0;
    for g in &groups {
        let sum = g.summarize(&opts).unwrap();
        units += sum.whole_units + sum.cut_units;
    }
    println!(
        "centroid zones, report: 300 zones, {} units in {:.2} s",
        units,
        t.elapsed().as_secs_f64()
    );
    let t = Instant::now();
    let mut n = 0usize;
    for g in &groups {
        g.for_each_unit(&Default::default(), &mut |_| n += 1)
            .unwrap();
    }
    println!(
        "centroid zones, export walk: {} units in {:.2} s",
        n,
        t.elapsed().as_secs_f64()
    );
    let t = Instant::now();
    let rep = check(&groups, &r, &CheckOptions::default()).unwrap();
    println!(
        "centroid zones, check: {} units in {:.2} s; findings {:?}",
        rep.units,
        t.elapsed().as_secs_f64(),
        rep.counts
    );
}
