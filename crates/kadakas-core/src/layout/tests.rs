use super::*;
use crate::curve;
use crate::curve::Vertex;
use proptest::prelude::*;

fn rect(w: f64, h: f64) -> Vec<DVec2> {
    vec![
        DVec2::new(0.0, 0.0),
        DVec2::new(w, 0.0),
        DVec2::new(w, h),
        DVec2::new(0.0, h),
    ]
}

fn pat(bond: Bond, gap: f64) -> Pattern {
    Pattern {
        bond,
        unit_length: 200.0,
        unit_width: 100.0,
        gap,
    }
}

fn region(outer: &[DVec2], holes: &[Vec<DVec2>]) -> Region {
    Region::from_rings(outer, holes).unwrap()
}

fn l_shape() -> Vec<DVec2> {
    vec![
        DVec2::new(0.0, 0.0),
        DVec2::new(3000.0, 0.0),
        DVec2::new(3000.0, 1000.0),
        DVec2::new(1000.0, 1000.0),
        DVec2::new(1000.0, 3000.0),
        DVec2::new(0.0, 3000.0),
    ]
}

fn island() -> Vec<DVec2> {
    vec![
        DVec2::new(1300.0, 300.0),
        DVec2::new(1800.0, 380.0),
        DVec2::new(1700.0, 700.0),
        DVec2::new(1350.0, 640.0),
    ]
}

fn run(r: &Region, p: &Pattern, place: &Placement) -> (Vec<Unit>, Stats) {
    lay_units(r, p, place, MAX_UNITS).unwrap()
}

fn in_unit(u: &Unit, p: DVec2) -> bool {
    let mut inside = false;
    for ring in &u.rings {
        let n = ring.points.len();
        for i in 0..n {
            let (a, b) = (ring.points[i], ring.points[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) && a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y) < p.x {
                inside = !inside;
            }
        }
    }
    inside
}

fn in_region(r: &Region, p: DVec2) -> bool {
    let mut inside = false;
    for ring in r.rings() {
        let n = ring.len();
        for i in 0..n {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) && a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y) < p.x {
                inside = !inside;
            }
        }
    }
    inside
}

#[test]
fn without_gaps_the_units_fill_the_region_exactly() {
    let r = region(&l_shape(), &[island()]);
    for bond in Bond::ALL {
        for angle in [0.0, 30.0, -77.0] {
            let place = Placement {
                origin: None,
                angle,
            };
            let (units, stats) = run(&r, &pat(bond, 0.0), &place);
            let sum: f64 = units.iter().map(|u| u.area).sum();
            assert!(
                (sum - r.area).abs() < 1e-7 * r.area + 1e-3,
                "{bond:?} {angle}: {sum} vs {}",
                r.area
            );
            assert_eq!(stats.units, units.len());
            assert!(units.iter().any(|u| !u.whole) && units.iter().any(|u| u.whole));
        }
    }
}

#[test]
fn units_are_inside_the_region_and_never_overlap() {
    let r = region(&l_shape(), &[island()]);
    for bond in Bond::ALL {
        let (units, _) = run(
            &r,
            &pat(bond, 5.0),
            &Placement {
                origin: None,
                angle: 20.0,
            },
        );
        let mut hit_units = 0;
        for ix in 0..60 {
            for iy in 0..60 {
                let p = DVec2::new(ix as f64 * 50.3 + 7.1, iy as f64 * 50.3 + 3.3);
                let n = units.iter().filter(|u| in_unit(u, p)).count();
                assert!(n <= 1, "{bond:?}: point in {n} units");
                if n == 1 {
                    hit_units += 1;
                    assert!(in_region(&r, p), "{bond:?}: unit outside the region");
                }
            }
        }
        assert!(hit_units > 100);
    }
}

#[test]
fn gap_area_is_the_region_minus_the_units() {
    // Aligned rectangle, stack: whole units and an exact gap area.
    let r = region(&rect(2050.0, 1050.0), &[]);
    let place = Placement {
        origin: Some(DVec2::ZERO),
        angle: 0.0,
    };
    let (units, stats) = run(&r, &pat(Bond::Stack, 5.0), &place);
    // Cells are 205 x 105: 10 x 10 cells, units 200 x 100.
    assert_eq!(units.len(), 100);
    assert!(units.iter().all(|u| u.whole));
    let sum: f64 = units.iter().map(|u| u.area).sum();
    assert!((sum - 100.0 * 200.0 * 100.0).abs() < 1e-6);
    let gap_area = stats.region_area - sum;
    assert!((gap_area - (2050.0 * 1050.0 - 2_000_000.0)).abs() < 1e-6);
    // Gap center lines: 9 inner vertical lines of 1050 and 9 inner horizontal of 2050, plus the
    // lines on the four edges of the region.
    let g = gap_lines(&r, &pat(Bond::Stack, 5.0), &place, MAX_UNITS, false);
    assert!(!g.truncated);
    let len = gaps::length(&g.segments);
    assert!(
        len >= 9.0 * 1050.0 + 9.0 * 2050.0 - 1.0 && len <= 11.0 * 1050.0 + 11.0 * 2050.0 + 1.0,
        "{len}"
    );
}

#[test]
fn counts_stay_the_same_when_the_drawing_moves() {
    let base = region(&l_shape(), &[island()]);
    let shift = DVec2::new(1_234_567.8, -98_765.4);
    let moved = region(
        &l_shape().iter().map(|p| *p + shift).collect::<Vec<_>>(),
        &[island().iter().map(|p| *p + shift).collect()],
    );
    for bond in Bond::ALL {
        for angle in [0.0, 17.0] {
            let p = pat(bond, 5.0);
            let (a, _) = run(
                &base,
                &p,
                &Placement {
                    origin: None,
                    angle,
                },
            );
            let (b, _) = run(
                &moved,
                &p,
                &Placement {
                    origin: None,
                    angle,
                },
            );
            let count = |u: &[Unit]| (u.iter().filter(|x| x.whole).count(), u.len());
            assert_eq!(count(&a), count(&b), "{bond:?} {angle}");
            let (sa, sb): (f64, f64) = (
                a.iter().map(|u| u.area).sum(),
                b.iter().map(|u| u.area).sum(),
            );
            assert!((sa - sb).abs() < 1e-3, "{bond:?}");
        }
    }
}

#[test]
fn origin_moves_the_pattern_and_the_edge_aligns_it() {
    let r = region(&rect(2000.0, 1000.0), &[]);
    let p = pat(Bond::Stack, 0.0);
    // Origin at a corner: the first unit starts there.
    let (units, _) = run(
        &r,
        &p,
        &Placement {
            origin: Some(DVec2::ZERO),
            angle: 0.0,
        },
    );
    assert_eq!(units.len(), 10 * 10);
    assert!(units.iter().all(|u| u.whole));
    // A shifted origin cuts units at the edges.
    let (units, _) = run(
        &r,
        &p,
        &Placement {
            origin: Some(DVec2::new(50.0, 0.0)),
            angle: 0.0,
        },
    );
    assert_eq!(units.iter().filter(|u| !u.whole).count(), 20);
    // Edge 1 of a rotated rectangle: the pattern follows it.
    let a = 33.0f64.to_radians();
    let turn = DVec2::from_angle(a);
    let pts: Vec<Vertex> = curve::corners(
        &rect(2000.0, 1000.0)
            .iter()
            .map(|q| turn.rotate(*q) + DVec2::new(500.0, 500.0))
            .collect::<Vec<_>>(),
    );
    let place = Placement::along_edge(&pts, 0).unwrap();
    assert!((place.angle - 33.0).abs() < 1e-9);
    assert_eq!(place.origin, Some(pts[0].p));
    let reg = Region::new(&pts, &[], 1.0).unwrap();
    let (units, _) = run(&reg, &p, &place);
    assert_eq!(units.len(), 100);
    assert!(
        units.iter().all(|u| u.whole),
        "all whole when aligned to the edge"
    );
    assert!(Placement::along_edge(&pts, 9).is_err());
}

#[test]
fn too_many_units_is_an_error_with_the_estimate() {
    let r = region(&rect(100_000.0, 100_000.0), &[]);
    let e = lay_units(
        &r,
        &pat(Bond::RunningBond, 5.0),
        &Placement::default(),
        1000,
    )
    .unwrap_err();
    assert!(e.estimated > 400_000 && e.max == 1000);
    assert!(e.to_string().contains("limit"));
    // A thin diagonal shape with a big box is refused too, not slow.
    let thin = region(
        &[
            DVec2::new(0.0, 0.0),
            DVec2::new(2_000_000.0, 2_000_000.0),
            DVec2::new(2_000_000.0, 2_000_100.0),
            DVec2::new(0.0, 100.0),
        ],
        &[],
    );
    assert!(lay_units(&thin, &pat(Bond::Stack, 5.0), &Placement::default(), 10_000).is_err());
}

#[test]
fn holes_remove_and_split_units() {
    let big = region(
        &rect(2000.0, 1000.0),
        &[vec![
            DVec2::new(500.0, 200.0),
            DVec2::new(900.0, 200.0),
            DVec2::new(900.0, 800.0),
            DVec2::new(500.0, 800.0),
        ]],
    );
    let (units, stats) = run(
        &big,
        &pat(Bond::Stack, 0.0),
        &Placement {
            origin: Some(DVec2::ZERO),
            angle: 0.0,
        },
    );
    let sum: f64 = units.iter().map(|u| u.area).sum();
    assert!((sum - (2_000_000.0 - 400.0 * 600.0)).abs() < 1e-6);
    assert_eq!(stats.region_area, 2_000_000.0 - 240_000.0);
    // A hole inside one unit makes a cut unit with a hole ring.
    let tiny = region(
        &rect(400.0, 200.0),
        &[vec![
            DVec2::new(50.0, 20.0),
            DVec2::new(100.0, 20.0),
            DVec2::new(100.0, 70.0),
            DVec2::new(50.0, 70.0),
        ]],
    );
    let (units, _) = run(
        &tiny,
        &pat(Bond::Stack, 0.0),
        &Placement {
            origin: Some(DVec2::ZERO),
            angle: 0.0,
        },
    );
    assert_eq!(units.len(), 4);
    let cut: Vec<&Unit> = units.iter().filter(|u| !u.whole).collect();
    assert_eq!(cut.len(), 1);
    assert_eq!(cut[0].rings.len(), 2);
    assert!(cut[0].rings[1].hole);
    assert!((cut[0].area - (20_000.0 - 2500.0)).abs() < 1e-6);
    assert!(Region::from_rings(&rect(10.0, 10.0), &[rect(20.0, 20.0)]).is_err());
}

proptest! {
    #[test]
    fn units_never_exceed_the_region(
        angle in -180.0f64..180.0, ox in -500.0f64..500.0, oy in -500.0f64..500.0,
        gap in 0.0f64..20.0, bond in 0usize..4,
    ) {
        let r = region(&l_shape(), &[island()]);
        let p = pat(Bond::ALL[bond], gap);
        let (units, stats) = run(&r, &p, &Placement { origin: Some(DVec2::new(ox, oy)), angle });
        let sum: f64 = units.iter().map(|u| u.area).sum();
        prop_assert!(sum <= r.area + 1e-3);
        prop_assert!(units.iter().all(|u| u.share <= 1.0 + 1e-9 && u.share > 0.0));
        // The gaps take the rest: with no gap the units fill the region, else a share of it.
        let fill = sum / r.area;
        if gap == 0.0 { prop_assert!((fill - 1.0).abs() < 1e-6, "{fill}"); }
        prop_assert!(fill > 0.5, "{fill}");
        prop_assert_eq!(stats.units, units.len());
    }
}

/// Measures time and memory of the biggest jobs. Run with
/// `cargo test -p kadakas-core --release layout::tests::measure_bounds -- --ignored --nocapture`.
#[test]
#[ignore]
fn measure_bounds() {
    use std::time::Instant;
    let (w, h) = match std::env::var("MEASURE_BIG") {
        Ok(_) => (200_000.0, 100_000.0),
        Err(_) => (100_000.0, 50_000.0),
    };
    let r = region(
        &rect(w, h),
        &[island()
            .iter()
            .map(|p| *p + DVec2::new(40_000.0, 20_000.0))
            .collect()],
    );
    for bond in Bond::ALL {
        let p = pat(bond, 5.0);
        let t = Instant::now();
        let mut n = 0usize;
        let mut bytes = 0usize;
        let stats = for_each_unit(
            &r,
            &p,
            &Placement {
                origin: None,
                angle: 11.0,
            },
            MAX_UNITS,
            &mut |u| {
                n += 1;
                bytes += u
                    .rings
                    .iter()
                    .map(|r| r.points.len() * 16 + 32)
                    .sum::<usize>();
            },
        )
        .unwrap();
        let ms_stream = t.elapsed().as_millis();
        let t = Instant::now();
        let (units, _) = run(
            &r,
            &p,
            &Placement {
                origin: None,
                angle: 11.0,
            },
        );
        let ms_list = t.elapsed().as_millis();
        let t = Instant::now();
        let g = gap_lines(
            &r,
            &p,
            &Placement {
                origin: None,
                angle: 11.0,
            },
            MAX_UNITS,
            false,
        );
        println!(
            "{bond:?}: {n} units, stream {ms_stream} ms, list {ms_list} ms ({} MB of points, {} bytes per unit), gaps {} segments in {} ms, truncated {}, dust {}",
            bytes / 1_000_000,
            bytes / n.max(1),
            g.segments.len(),
            t.elapsed().as_millis(),
            g.truncated,
            stats.dust_pieces
        );
        assert_eq!(units.len(), n);
    }
}
