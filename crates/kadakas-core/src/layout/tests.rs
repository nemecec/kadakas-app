use super::module::{self, ModuleUnit};
use super::summary::{SummaryOptions, summarize, with_waste};
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
    Pattern::new(bond, 200.0, 100.0, gap)
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

fn limits() -> Limits {
    Limits::new(MAX_UNITS)
}

fn run(r: &Region, p: &Pattern, place: &Placement) -> (Vec<Unit>, Stats) {
    lay_units(r, p, place, &limits()).unwrap()
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

fn presets() -> Vec<(&'static str, Pattern)> {
    vec![
        (
            "hexagon",
            Pattern::of_module(module::hexagon(150.0).unwrap(), 5.0),
        ),
        (
            "octagon_square",
            Pattern::of_module(module::octagon_square(100.0).unwrap(), 5.0),
        ),
        (
            "i_shape",
            Pattern::of_module(module::i_shape(200.0, 165.0, 25.0).unwrap(), 5.0),
        ),
    ]
}

#[test]
fn without_gaps_the_units_fill_the_region_exactly() {
    let r = region(&l_shape(), &[island()]);
    let mut all: Vec<(String, Pattern)> = Bond::ALL
        .iter()
        .map(|b| (b.id().to_string(), pat(*b, 0.0)))
        .collect();
    for (n, p) in presets() {
        all.push((
            n.to_string(),
            Pattern::of_module(p.module.clone().unwrap(), 0.0),
        ));
    }
    for (name, p) in all {
        for angle in [0.0, 30.0, -77.0] {
            let place = Placement {
                origin: None,
                angle,
            };
            let (units, stats) = run(&r, &p, &place);
            let sum: f64 = units.iter().map(|u| u.area).sum();
            assert!(
                (sum - r.area).abs() < 1e-7 * r.area + 1e-3 + stats.dust_pieces as f64 * DUST_AREA,
                "{name} {angle}: {sum} vs {}",
                r.area
            );
            assert_eq!(stats.units, units.len());
            assert!(units.iter().any(|u| !u.whole) && units.iter().any(|u| u.whole));
        }
    }
}

fn edge_lengths(u: &Unit) -> (f64, f64) {
    let p = &u.rings[0].points;
    assert_eq!(p.len(), 4, "a whole unit of a bond is a rectangle");
    (p[0].distance(p[1]), p[1].distance(p[2]))
}

#[test]
fn every_whole_unit_of_every_bond_has_exactly_the_size_that_was_asked_for() {
    let r = region(&rect(6000.0, 4000.0), &[]);
    for bond in Bond::ALL {
        for (l, w) in [
            (200.0, 100.0),
            (300.0, 200.0),
            (300.0, 100.0),
            (100.0, 200.0),
        ] {
            for gap in [0.0, 4.0, 10.0] {
                for angle in [0.0, 33.0] {
                    let p = Pattern::new(bond, l, w, gap);
                    let place = Placement {
                        origin: None,
                        angle,
                    };
                    let result = lay_units(&r, &p, &place, &limits());
                    if bond == Bond::BasketWeave && (l == 300.0 && w == 200.0) {
                        // 3:2 does not fit a basket weave: refused, never resized.
                        assert!(
                            matches!(result, Err(LayoutError::Invalid(ref e)) if e.contains("does not fit")),
                            "{result:?}"
                        );
                        continue;
                    }
                    let (units, _) =
                        result.unwrap_or_else(|e| panic!("{bond:?} {l}x{w} {gap}: {e}"));
                    let whole: Vec<&Unit> = units.iter().filter(|u| u.whole).collect();
                    assert!(whole.len() > 50, "{bond:?} {l}x{w}");
                    for u in whole {
                        let (a, b) = edge_lengths(u);
                        let mut got = [a, b];
                        got.sort_by(f64::total_cmp);
                        let mut want = [l, w];
                        want.sort_by(f64::total_cmp);
                        assert!(
                            (got[0] - want[0]).abs() < 1e-6 && (got[1] - want[1]).abs() < 1e-6,
                            "{bond:?} {l}x{w} gap {gap} angle {angle}: {got:?}"
                        );
                        assert!((u.area - l * w).abs() < 1e-6);
                    }
                }
            }
        }
    }
}

#[test]
fn herringbone_with_a_gap_keeps_the_count_right() {
    // 200 x 100, gap 5: a 4,100 x 2,100 mm area aligned to the origin has 20 columns of 205 mm
    // and the herringbone has two units per 2 * 205 * 105 mm2 of the lattice.
    let r = region(&rect(4100.0, 2100.0), &[]);
    let place = Placement {
        origin: Some(DVec2::ZERO),
        angle: 0.0,
    };
    let (units, _) = run(&r, &pat(Bond::Herringbone, 5.0), &place);
    let expected = 4100.0 * 2100.0 / (205.0 * 105.0); // units per area, 400
    let whole = units.iter().filter(|u| u.whole).count() as f64;
    assert!(
        whole > expected * 0.9 && whole <= expected + 1.0,
        "{whole} vs {expected}"
    );
    let all = units.len() as f64;
    assert!(all > expected && all < expected * 1.25);
}

#[test]
fn units_are_inside_the_region_and_never_overlap() {
    let r = region(&l_shape(), &[island()]);
    let mut all: Vec<(String, Pattern)> = Bond::ALL
        .iter()
        .map(|b| (b.id().to_string(), pat(*b, 5.0)))
        .collect();
    for (n, p) in presets() {
        all.push((n.to_string(), p));
    }
    for (name, p) in all {
        let (units, _) = run(
            &r,
            &p,
            &Placement {
                origin: None,
                angle: 20.0,
            },
        );
        let mut hit_units = 0;
        for ix in 0..60 {
            for iy in 0..60 {
                let q = DVec2::new(ix as f64 * 50.3 + 7.1, iy as f64 * 50.3 + 3.3);
                let n = units.iter().filter(|u| in_unit(u, q)).count();
                assert!(n <= 1, "{name}: point in {n} units");
                if n == 1 {
                    hit_units += 1;
                    assert!(in_region(&r, q), "{name}: unit outside the region");
                }
            }
        }
        assert!(hit_units > 100, "{name}");
    }
}

#[test]
fn gap_area_is_the_region_minus_the_units() {
    let r = region(&rect(2050.0, 1050.0), &[]);
    let place = Placement {
        origin: Some(DVec2::ZERO),
        angle: 0.0,
    };
    let (units, stats) = run(&r, &pat(Bond::Stack, 5.0), &place);
    assert_eq!(units.len(), 100);
    assert!(units.iter().all(|u| u.whole));
    let sum: f64 = units.iter().map(|u| u.area).sum();
    assert!((sum - 100.0 * 200.0 * 100.0).abs() < 1e-6);
    let gap_area = stats.region_area - sum;
    assert!((gap_area - (2050.0 * 1050.0 - 2_000_000.0)).abs() < 1e-6);
    let g = gap_lines(&r, &pat(Bond::Stack, 5.0), &place, MAX_UNITS, false);
    assert!(!g.truncated);
    let len = gaps::length(&g.segments);
    assert!(
        (9.0 * 1050.0 + 9.0 * 2050.0 - 1.0..=11.0 * 1050.0 + 11.0 * 2050.0 + 1.0).contains(&len),
        "{len}"
    );
}

#[test]
fn gap_lines_of_cell_patterns_follow_the_cells() {
    let r = region(&rect(2050.0, 1050.0), &[]);
    let place = Placement {
        origin: Some(DVec2::ZERO),
        angle: 0.0,
    };
    for p in [pat(Bond::Herringbone, 5.0), pat(Bond::BasketWeave, 5.0)]
        .into_iter()
        .chain(presets().into_iter().map(|x| x.1))
    {
        let g = gap_lines(&r, &p, &place, MAX_GAP_SEGMENTS, false);
        assert!(
            !g.truncated && g.segments.len() > 10,
            "{:?} {} {}",
            p.bond,
            g.segments.len(),
            g.truncated
        );
        // Each segment lies in the box of the region.
        assert!(g.segments.iter().all(|(a, b)| {
            [a, b]
                .iter()
                .all(|q| q.x > -1e-6 && q.x < 2050.0 + 1e-6 && q.y > -1e-6 && q.y < 1050.0 + 1e-6)
        }));
    }
    // A limit that is too small is a clear refusal, and the on-screen drawing may use larger units.
    let g = gap_lines(&r, &pat(Bond::Herringbone, 5.0), &place, 10, false);
    assert!(g.truncated);
    let big = region(&rect(400_000.0, 400_000.0), &[]);
    let g = gap_lines(
        &big,
        &Pattern::new(Bond::Herringbone, 40.0, 20.0, 0.0),
        &place,
        30_000,
        true,
    );
    assert!(g.coarsened || g.truncated);
}

#[test]
fn the_gap_length_of_a_herringbone_is_three_halves_of_a_cell_per_cell() {
    // Bricks of 2 x 1 cells: every brick has a perimeter of 6 cells, and each edge is shared by two
    // bricks, so the lines are 1.5 cells long for each cell of area.
    let r = region(&rect(20_000.0, 20_000.0), &[]);
    let g = gap_lines(
        &r,
        &pat(Bond::Herringbone, 0.0),
        &Placement::default(),
        MAX_GAP_SEGMENTS,
        false,
    );
    assert!(!g.truncated);
    let per_cell = gaps::length(&g.segments) / (20_000.0 * 20_000.0 / 100.0 / 100.0) / 100.0;
    assert!((per_cell - 1.5).abs() < 0.05, "{per_cell}");
}

#[test]
fn counts_stay_the_same_when_the_drawing_moves() {
    let base = region(&l_shape(), &[island()]);
    let shift = DVec2::new(1_234_567.8, -98_765.4);
    let moved = region(
        &l_shape().iter().map(|p| *p + shift).collect::<Vec<_>>(),
        &[island().iter().map(|p| *p + shift).collect()],
    );
    let mut all: Vec<Pattern> = Bond::ALL.iter().map(|b| pat(*b, 5.0)).collect();
    all.extend(presets().into_iter().map(|x| x.1));
    for p in all {
        for angle in [0.0, 17.0] {
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
            assert_eq!(count(&a), count(&b), "{:?} {angle}", p.bond);
            let (sa, sb): (f64, f64) = (
                a.iter().map(|u| u.area).sum(),
                b.iter().map(|u| u.area).sum(),
            );
            assert!((sa - sb).abs() < 1e-3, "{:?}", p.bond);
        }
    }
}

#[test]
fn origin_moves_the_pattern_and_the_edge_aligns_it() {
    let r = region(&rect(2000.0, 1000.0), &[]);
    let p = pat(Bond::Stack, 0.0);
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
    let (units, _) = run(
        &r,
        &p,
        &Placement {
            origin: Some(DVec2::new(50.0, 0.0)),
            angle: 0.0,
        },
    );
    assert_eq!(units.iter().filter(|u| !u.whole).count(), 20);
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
        &Limits::new(1000),
    )
    .unwrap_err();
    let LayoutError::TooMany(e) = e else {
        panic!("{e:?}")
    };
    assert!(e.estimated > 400_000 && e.max == 1000);
    assert!(e.to_string().contains("limit"));
    let thin = region(
        &[
            DVec2::new(0.0, 0.0),
            DVec2::new(2_000_000.0, 2_000_000.0),
            DVec2::new(2_000_000.0, 2_000_100.0),
            DVec2::new(0.0, 100.0),
        ],
        &[],
    );
    assert!(
        lay_units(
            &thin,
            &pat(Bond::Stack, 5.0),
            &Placement::default(),
            &Limits::new(10_000)
        )
        .is_err()
    );
}

#[test]
fn an_invalid_pattern_is_an_error_not_an_empty_layout() {
    let r = region(&rect(1000.0, 1000.0), &[]);
    let bad = Pattern::new(Bond::Stack, 0.0, 100.0, 0.0);
    let e = lay_units(&r, &bad, &Placement::default(), &limits()).unwrap_err();
    assert!(matches!(e, LayoutError::Invalid(_)) && e.to_string().contains("unit length"));
    let nomod = Pattern {
        bond: Bond::Module,
        ..pat(Bond::Stack, 0.0)
    };
    assert!(lay_units(&r, &nomod, &Placement::default(), &limits()).is_err());
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

#[test]
fn a_unit_that_crosses_the_edge_by_noise_counts_as_whole() {
    // Gap 0, the region 0.05 mm short of three units: with the trim they are whole, without it
    // the last column is cut.
    let r = region(&rect(599.95, 100.0), &[]);
    let p = Pattern::new(Bond::Stack, 200.0, 100.0, 0.0);
    let place = Placement {
        origin: Some(DVec2::ZERO),
        angle: 0.0,
    };
    let (units, stats) = lay_units(&r, &p, &place, &Limits::new(1000)).unwrap();
    assert_eq!(units.len(), 3);
    assert!(units.iter().all(|u| u.whole), "trimmed: {units:?}");
    assert_eq!(stats.trimmed, 1);
    let (units, stats) = lay_units(
        &r,
        &p,
        &place,
        &Limits {
            max_units: 1000,
            trim: 0.0,
        },
    )
    .unwrap();
    assert_eq!(units.iter().filter(|u| !u.whole).count(), 1);
    assert_eq!(stats.trimmed, 0);
    // A real cut stays a cut.
    let r = region(&rect(550.0, 100.0), &[]);
    let (units, _) = lay_units(&r, &p, &place, &Limits::new(1000)).unwrap();
    assert_eq!(units.iter().filter(|u| !u.whole).count(), 1);
}

#[test]
fn the_basket_weave_never_resizes_a_unit_and_says_how_wide_the_joints_get() {
    // 200 x 100 with a gap of 5: the block is 210 mm, the length needs 205: the extra goes into the joint.
    let p = pat(Bond::BasketWeave, 5.0);
    assert!(
        (p.widest_joint() - 7.5).abs() < 1e-9,
        "{}",
        p.widest_joint()
    );
    assert_eq!(pat(Bond::Stack, 5.0).widest_joint(), 5.0);
    let bad = Pattern::new(Bond::BasketWeave, 300.0, 200.0, 5.0);
    assert!(bad.validate().unwrap_err().contains("does not fit"));
    assert!(
        Pattern::new(Bond::BasketWeave, 300.0, 100.0, 5.0)
            .validate()
            .is_ok()
    );
}

#[test]
fn modules_are_checked() {
    let sq = |x: f64, y: f64, s: f64| ModuleUnit {
        name: format!("sq{x}"),
        outline: curve::corners(&[
            DVec2::new(x, y),
            DVec2::new(x + s, y),
            DVec2::new(x + s, y + s),
            DVec2::new(x, y + s),
        ]),
    };
    let ok = Module {
        units: vec![sq(0.0, 0.0, 100.0)],
        a: [100.0, 0.0],
        b: [0.0, 100.0],
    };
    assert!(ok.compile(0.0).is_ok());
    // A lattice that is not a lattice.
    let flat = Module {
        b: [200.0, 0.0],
        ..ok.clone()
    };
    assert!(flat.compile(0.0).unwrap_err().contains("same way"));
    let tiny = Module {
        a: [0.1, 0.0],
        ..ok.clone()
    };
    assert!(tiny.compile(0.0).is_err());
    // The shape is larger than the cell: it overlaps its copies.
    let big = Module {
        units: vec![sq(0.0, 0.0, 150.0)],
        ..ok.clone()
    };
    assert!(
        big.compile(0.0)
            .unwrap_err()
            .contains("larger than the lattice cell")
    );
    // Two shapes that overlap each other.
    let two = Module {
        units: vec![sq(0.0, 0.0, 60.0), sq(30.0, 30.0, 60.0)],
        a: [200.0, 0.0],
        b: [0.0, 200.0],
    };
    assert!(two.compile(0.0).unwrap_err().contains("overlap"));
    // A shape that sticks out and meets its neighbour copy.
    let reach = Module {
        units: vec![ModuleUnit {
            name: "l".into(),
            outline: curve::corners(&[
                DVec2::new(0.0, 0.0),
                DVec2::new(150.0, 0.0),
                DVec2::new(150.0, 40.0),
                DVec2::new(0.0, 40.0),
            ]),
        }],
        a: [100.0, 0.0],
        b: [0.0, 100.0],
    };
    assert!(reach.compile(0.0).is_err());
    // Gaps between shapes are fine.
    let spaced = Module {
        units: vec![sq(10.0, 10.0, 50.0)],
        ..ok.clone()
    };
    assert!(spaced.compile(0.0).is_ok());
    // Other refusals.
    let none = Module {
        units: vec![],
        ..ok.clone()
    };
    assert!(none.compile(0.0).is_err());
    let line = Module {
        units: vec![ModuleUnit {
            name: "x".into(),
            outline: curve::corners(&[DVec2::ZERO, DVec2::X]),
        }],
        ..ok.clone()
    };
    assert!(line.compile(0.0).is_err());
    let bow = Module {
        units: vec![ModuleUnit {
            name: "bow".into(),
            outline: curve::corners(&[
                DVec2::new(0.0, 0.0),
                DVec2::new(60.0, 50.0),
                DVec2::new(50.0, 0.0),
                DVec2::new(0.0, 40.0),
            ]),
        }],
        ..ok.clone()
    };
    let e = bow.compile(0.0).unwrap_err();
    assert!(e.contains("crosses itself"), "{e}");
    // A gap that eats a shape.
    assert!(ok.compile(120.0).is_err());
    assert!(ok.compile(f64::NAN).is_err());
    assert!(Pattern::of_module(ok, 4.0).validate().is_ok());
    assert!(module::preset("pentagon", &|_| None).is_err());
    assert!(module::preset("hexagon", &|_| None).is_err());
    assert!(module::hexagon(1.0).is_err());
    assert!(module::i_shape(200.0, 100.0, 60.0).is_err());
}

#[test]
fn presets_tile_with_the_right_unit_counts_and_shapes() {
    let r = region(&rect(6000.0, 4000.0), &[]);
    let place = Placement::default();
    // Hexagon: every whole unit is a regular hexagon of the side, less half a gap all round.
    let hex = Pattern::of_module(module::hexagon_across_flats(200.0).unwrap(), 0.0);
    let (units, _) = run(&r, &hex, &place);
    let whole = units.iter().find(|u| u.whole).unwrap();
    assert_eq!(whole.rings[0].points.len(), 6);
    let area = 3.0 * 3.0f64.sqrt() / 2.0 * (200.0 / 3.0f64.sqrt()).powi(2);
    assert!((whole.area - area).abs() < 1e-3, "{} vs {area}", whole.area);
    // Octagon with square: two unit types, one square per octagon.
    let os = Pattern::of_module(module::octagon_square(100.0).unwrap(), 0.0);
    let sum = summarize(&r, &os, &place, &SummaryOptions::default()).unwrap();
    assert_eq!(sum.by_unit.len(), 2);
    let oct = sum.by_unit.iter().find(|u| u.name == "octagon").unwrap();
    let sq = sum.by_unit.iter().find(|u| u.name == "square").unwrap();
    assert!(
        (oct.whole_units as f64 / sq.whole_units as f64 - 1.0).abs() < 0.15,
        "{} {}",
        oct.whole_units,
        sq.whole_units
    );
    assert_eq!(sum.whole_units, oct.whole_units + sq.whole_units);
    assert_eq!(sum.unit_area_m2, None);
    assert!((sum.area_laid_m2 + sum.gap_area_m2 - sum.region_area_m2).abs() < 1e-9);
    let by_area: f64 = sum.by_unit.iter().map(|u| u.area_laid_m2).sum();
    assert!((by_area - sum.area_laid_m2).abs() < 1e-9);
    // I-shape: 12 corners and the exact lattice (no gap: the whole region is covered).
    let i = Pattern::of_module(module::i_shape(200.0, 165.0, 25.0).unwrap(), 0.0);
    let (units, _) = run(&r, &i, &place);
    let whole = units.iter().find(|u| u.whole).unwrap();
    assert_eq!(whole.rings[0].points.len(), 12);
    let covered: f64 = units.iter().map(|u| u.area).sum();
    assert!((covered - r.area).abs() < 1e-3, "{covered}");
}

#[test]
fn units_of_a_module_carry_their_type() {
    let r = region(&rect(3000.0, 3000.0), &[]);
    let p = Pattern::of_module(module::octagon_square(100.0).unwrap(), 4.0);
    let (units, stats) = run(&r, &p, &Placement::default());
    assert_eq!(stats.kinds.len(), 2);
    assert!(units.iter().any(|u| u.kind == 0) && units.iter().any(|u| u.kind == 1));
    let names: Vec<&str> = stats.kinds.iter().map(|k| k.0.as_str()).collect();
    assert_eq!(names, ["octagon", "square"]);
}

#[test]
fn waste_is_integer_math() {
    assert_eq!(with_waste(10, 10.0), 11);
    assert_eq!(with_waste(100, 5.0), 105);
    assert_eq!(with_waste(97, 5.0), 102);
    assert_eq!(with_waste(0, 50.0), 0);
    assert_eq!(with_waste(3, 0.0), 3);
}

#[test]
fn the_order_has_bounds_and_unfillable_pieces_are_counted_separately() {
    let r = region(&rect(2000.0, 1000.0), &[]);
    let p = Pattern::new(Bond::Stack, 200.0, 100.0, 0.0);
    let place = Placement {
        origin: Some(DVec2::new(95.0, 0.0)),
        angle: 0.0,
    };
    let s = summarize(&r, &p, &place, &SummaryOptions::default()).unwrap();
    // 9 columns of 10 are whole, the two edge columns are 105 and 95 mm wide: shares .525 and .475.
    assert_eq!((s.whole_units, s.cut_units), (90, 20));
    assert_eq!(s.order_units, 110);
    assert_eq!(s.order_units_max, 110);
    assert_eq!(s.order_units_min, 100); // 90 + ceil(10 * 0.525 + 10 * 0.475) = 100
    assert_eq!(s.unfillable_pieces, 0);
    assert!((s.cut_area_m2 - 0.2).abs() < 1e-9);
    // A column of 5 mm: unfillable by share, and left out of the order only on request.
    let place = Placement {
        origin: Some(DVec2::new(5.0, 0.0)),
        angle: 0.0,
    };
    let s = summarize(&r, &p, &place, &SummaryOptions::default()).unwrap();
    assert_eq!(s.unfillable_pieces, 10);
    assert_eq!(s.order_units, s.whole_units + s.cut_units);
    let s2 = summarize(
        &r,
        &p,
        &place,
        &SummaryOptions {
            exclude_unfillable: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(s2.order_units, s.order_units - 10);
    assert_eq!(s2.order_units_max, s.order_units);
    let s3 = summarize(
        &r,
        &p,
        &place,
        &SummaryOptions {
            min_width: 120.0,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(s3.unfillable_pieces >= 10);
    let w = summarize(
        &r,
        &p,
        &place,
        &SummaryOptions {
            waste_percent: Some(10.0),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        w.order_units_with_waste,
        Some(with_waste(w.order_units, 10.0))
    );
}

#[test]
fn bond_units_are_named_by_their_size() {
    let r = region(&rect(3000.0, 2000.0), &[]);
    for bond in Bond::ALL {
        let s = summarize(
            &r,
            &pat(bond, 5.0),
            &Placement::default(),
            &SummaryOptions::default(),
        )
        .unwrap();
        assert_eq!(s.by_unit.len(), 1, "{bond:?}");
        assert_eq!(s.by_unit[0].name, "200x100", "{bond:?}");
        assert_eq!(s.by_unit[0].whole_units, s.whole_units);
        assert_eq!(s.by_unit[0].order_units_min, s.order_units_min);
    }
    let s = summarize(
        &r,
        &Pattern::new(Bond::Stack, 300.0, 150.5, 4.0),
        &Placement::default(),
        &SummaryOptions::default(),
    )
    .unwrap();
    assert_eq!(s.by_unit[0].name, "300x150.5");
}

#[test]
fn the_widest_joint_of_a_basket_weave_is_the_real_one() {
    // 200 x 100 with a gap of 3: the block is 206 wide, the unit needs 203. The unit sits in the
    // middle of its slot: 3 mm on each side of the block, and the units of the next block start
    // 1.5 mm in, so the joint between two blocks is 3 + 1.5 mm.
    let p = pat(Bond::BasketWeave, 3.0);
    assert!(
        (p.widest_joint() - 4.5).abs() < 1e-9,
        "{}",
        p.widest_joint()
    );
    // The measured joint between a whole unit and its neighbour of the next block is the same.
    let r = region(&rect(2060.0, 2060.0), &[]);
    let place = Placement {
        origin: Some(DVec2::ZERO),
        angle: 0.0,
    };
    let (units, _) = run(&r, &p, &place);
    let whole: Vec<&Unit> = units.iter().filter(|u| u.whole).collect();
    // The first block (flat units) and the next block (standing units) along x.
    let box_of = |u: &Unit| {
        let pts = &u.rings[0].points;
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for q in pts {
            lo = lo.min(*q);
            hi = hi.max(*q);
        }
        (lo, hi)
    };
    let flat = whole
        .iter()
        .map(|u| box_of(u))
        .find(|(lo, hi)| (hi.x - lo.x - 200.0).abs() < 1e-6 && lo.x < 100.0 && lo.y < 100.0)
        .unwrap();
    let standing = whole
        .iter()
        .map(|u| box_of(u))
        .filter(|(lo, hi)| (hi.y - lo.y - 200.0).abs() < 1e-6 && lo.x > flat.1.x && lo.y < 300.0)
        .min_by(|a, b| a.0.x.total_cmp(&b.0.x))
        .unwrap();
    assert!(
        (standing.0.x - flat.1.x - 4.5).abs() < 1e-6,
        "{} {}",
        standing.0.x,
        flat.1.x
    );
}

#[test]
fn the_width_of_a_piece_is_the_width_of_its_narrowest_strip() {
    use super::units::min_width;
    let r = rect(200.0, 100.0);
    assert!((min_width(&r) - 100.0).abs() < 1e-9);
    // Turned by 30 degrees the width stays.
    let turn = DVec2::from_angle(30f64.to_radians());
    let t: Vec<DVec2> = r.iter().map(|p| turn.rotate(*p)).collect();
    assert!((min_width(&t) - 100.0).abs() < 1e-9);
    // A piece with a long thin tail has the width of the tail; the area over the perimeter would
    // give 2/3 of the short side of a rectangle (2 x 100 x 200 / 600 = 66.7).
    let tail = vec![
        DVec2::new(0.0, 0.0),
        DVec2::new(500.0, 0.0),
        DVec2::new(500.0, 10.0),
        DVec2::new(0.0, 10.0),
    ];
    assert!((min_width(&tail) - 10.0).abs() < 1e-9);
    // A triangle: its smallest height. An L shape: the hull is a pentagon.
    let tri = vec![DVec2::ZERO, DVec2::new(100.0, 0.0), DVec2::new(0.0, 100.0)];
    assert!((min_width(&tri) - 70.710678).abs() < 1e-3);
    assert_eq!(min_width(&[DVec2::ZERO, DVec2::X]), 0.0);
    // A near-whole cut unit is no sliver by width: a 200 x 100 unit cut to 195 x 100 is 100 wide.
    let r = region(&rect(795.0, 100.0), &[]);
    let p = Pattern::new(Bond::Stack, 200.0, 100.0, 0.0);
    let place = Placement {
        origin: Some(DVec2::ZERO),
        angle: 0.0,
    };
    let s = summarize(
        &r,
        &p,
        &place,
        &SummaryOptions {
            min_width: 90.0,
            trim: 0.0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(s.cut_units, 1);
    assert_eq!(s.unfillable_pieces, 0, "195 x 100 is 100 mm wide, not 66");
    let s = summarize(
        &r,
        &p,
        &place,
        &SummaryOptions {
            min_width: 110.0,
            trim: 0.0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(s.unfillable_pieces, 1);
}

#[test]
fn a_shape_that_meets_a_far_lattice_copy_is_found() {
    // a = (100, 0), b = (300, 10): the copy at b - 3a = (0, 10) overlaps the shape.
    let m = Module {
        units: vec![ModuleUnit {
            name: "bar".into(),
            outline: curve::corners(&[
                DVec2::new(0.0, 0.0),
                DVec2::new(60.0, 0.0),
                DVec2::new(60.0, 15.0),
                DVec2::new(0.0, 15.0),
            ]),
        }],
        a: [100.0, 0.0],
        b: [300.0, 10.0],
    };
    let e = m.compile(0.0).unwrap_err();
    assert!(e.contains("overlaps"), "{e}");
    // The same lattice with a shape of 8 mm height has room.
    let ok = Module {
        units: vec![ModuleUnit {
            name: "bar".into(),
            outline: curve::corners(&[
                DVec2::new(0.0, 0.0),
                DVec2::new(60.0, 0.0),
                DVec2::new(60.0, 8.0),
                DVec2::new(0.0, 8.0),
            ]),
        }],
        ..m.clone()
    };
    assert!(ok.compile(0.0).is_ok());
    // A shape drawn several lattice steps away from the others.
    let far = Module {
        units: vec![
            ModuleUnit {
                name: "near".into(),
                outline: curve::corners(&[
                    DVec2::ZERO,
                    DVec2::new(50.0, 0.0),
                    DVec2::new(50.0, 50.0),
                    DVec2::new(0.0, 50.0),
                ]),
            },
            ModuleUnit {
                name: "far".into(),
                // 3 steps of a away and 2 of b: the copy at -3a - 2b meets "near".
                outline: curve::corners(&[
                    DVec2::new(300.0, 200.0),
                    DVec2::new(350.0, 200.0),
                    DVec2::new(350.0, 250.0),
                    DVec2::new(300.0, 250.0),
                ]),
            },
        ],
        a: [100.0, 0.0],
        b: [0.0, 100.0],
    };
    assert!(far.compile(0.0).unwrap_err().contains("overlap"));
    // The reduced basis is shorter and has the same cell.
    let (ra, rb) = module::reduce_basis(DVec2::new(100.0, 0.0), DVec2::new(300.0, 10.0));
    assert!((ra.perp_dot(rb).abs() - 1000.0).abs() < 1e-9 && ra.length() <= 10.0 + 1e-9);
    // Too many cells to check: a shape across a huge number of cells is refused, not slow.
    let wide = Module {
        units: vec![ModuleUnit {
            name: "w".into(),
            outline: curve::corners(&[
                DVec2::ZERO,
                DVec2::new(100_000.0, 0.0),
                DVec2::new(100_000.0, 5.0),
                DVec2::new(0.0, 5.0),
            ]),
        }],
        a: [10.0, 0.0],
        b: [0.0, 10.0],
    };
    assert!(wide.compile(0.0).is_err());
}

#[test]
fn the_minimum_order_is_kept_per_unit_type() {
    let r = region(&rect(3000.0, 2000.0), &[]);
    let p = Pattern::of_module(module::octagon_square(100.0).unwrap(), 4.0);
    let s = summarize(&r, &p, &Placement::default(), &SummaryOptions::default()).unwrap();
    for u in &s.by_unit {
        assert!(
            u.order_units_min >= u.whole_units && u.order_units_min <= u.whole_units + u.cut_units
        );
    }
    let sum: usize = s.by_unit.iter().map(|u| u.order_units_min).sum();
    assert!(sum >= s.order_units_min, "{sum} {}", s.order_units_min);
}

#[test]
fn a_long_thin_region_in_the_frame_of_the_pattern_gets_a_clear_message() {
    let thin = region(
        &[
            DVec2::new(0.0, 0.0),
            DVec2::new(2_000_000.0, 2_000_000.0),
            DVec2::new(2_000_000.0, 2_000_100.0),
            DVec2::new(0.0, 100.0),
        ],
        &[],
    );
    let e = lay_units(
        &thin,
        &pat(Bond::Stack, 5.0),
        &Placement::default(),
        &Limits::new(10_000),
    )
    .unwrap_err();
    assert!(matches!(e, LayoutError::Scan(_)), "{e:?}");
    assert!(e.to_string().contains("Turn the pattern"), "{e}");
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
        let fill = sum / r.area;
        if gap == 0.0 { prop_assert!((fill - 1.0).abs() < 1e-6, "{fill}"); }
        prop_assert!(fill > 0.5, "{fill}");
        prop_assert_eq!(stats.units, units.len());
    }

    #[test]
    fn area_per_unit_type_adds_up_for_modules(
        angle in -180.0f64..180.0, ox in -500.0f64..500.0, oy in -500.0f64..500.0,
        which in 0usize..3,
    ) {
        let r = region(&l_shape(), &[island()]);
        let p = Pattern::of_module(presets()[which].1.module.clone().unwrap(), 0.0);
        let s = summarize(&r, &p, &Placement { origin: Some(DVec2::new(ox, oy)), angle }, &SummaryOptions::default()).unwrap();
        // No gap: the units fill the region, and the types add up to the whole.
        prop_assert!((s.area_laid_m2 - s.region_area_m2).abs() < 1e-6 * s.region_area_m2.max(1.0));
        let by: f64 = s.by_unit.iter().map(|u| u.area_laid_m2).sum();
        prop_assert!((by - s.area_laid_m2).abs() < 1e-9);
        let n: usize = s.by_unit.iter().map(|u| u.whole_units + u.cut_units).sum();
        prop_assert_eq!(n, s.whole_units + s.cut_units);
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
    let mut all: Vec<(String, Pattern)> = Bond::ALL
        .iter()
        .map(|b| (b.id().to_string(), pat(*b, 5.0)))
        .collect();
    for (n, p) in presets() {
        all.push((n.to_string(), p));
    }
    for (name, p) in all {
        let place = Placement {
            origin: None,
            angle: 11.0,
        };
        let t = Instant::now();
        let mut n = 0usize;
        let mut bytes = 0usize;
        let stats = for_each_unit(&r, &p, &place, &limits(), &mut |u| {
            n += 1;
            bytes += u
                .rings
                .iter()
                .map(|r| r.points.len() * 16 + 32)
                .sum::<usize>();
        })
        .unwrap();
        let ms_stream = t.elapsed().as_millis();
        let t = Instant::now();
        let (units, _) = run(&r, &p, &place);
        let ms_list = t.elapsed().as_millis();
        let t = Instant::now();
        let g = gap_lines(&r, &p, &place, MAX_GAP_SEGMENTS, false);
        println!(
            "{name}: {n} units, stream {ms_stream} ms, list {ms_list} ms ({} MB of points, {} bytes per unit), gaps {} segments in {} ms, truncated {}, dust {}",
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

#[test]
fn a_wide_joint_in_a_basket_weave_is_a_warning() {
    let r = region(&rect(3000.0, 2000.0), &[]);
    let place = Placement::default();
    // 200 x 100 with a gap of 5: the widest joint is 7.5 mm, 2.5 mm more than the gap.
    let s = summarize(
        &r,
        &pat(Bond::BasketWeave, 5.0),
        &place,
        &SummaryOptions::default(),
    )
    .unwrap();
    let w = s.joint_warning.expect("a warning");
    assert!(w.contains("7.5") && w.contains("2.5"), "{w}");
    // With a gap of 3 the extra is 1.5 mm: no warning. A looser option takes the first one away.
    let s = summarize(
        &r,
        &pat(Bond::BasketWeave, 3.0),
        &place,
        &SummaryOptions::default(),
    )
    .unwrap();
    assert!(s.joint_warning.is_none() && (s.widest_joint_mm - 4.5).abs() < 1e-9);
    let s = summarize(
        &r,
        &pat(Bond::BasketWeave, 5.0),
        &place,
        &SummaryOptions {
            max_extra_joint_mm: 3.0,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(s.joint_warning.is_none());
    // The other bonds have the gap as their joint.
    let s = summarize(
        &r,
        &pat(Bond::Herringbone, 5.0),
        &place,
        &SummaryOptions::default(),
    )
    .unwrap();
    assert!(s.joint_warning.is_none());
}

#[test]
fn the_waste_is_worked_out_for_each_unit_type() {
    let r = region(&rect(3000.0, 2000.0), &[]);
    let p = Pattern::of_module(module::octagon_square(100.0).unwrap(), 4.0);
    let s = summarize(
        &r,
        &p,
        &Placement::default(),
        &SummaryOptions {
            waste_percent: Some(10.0),
            ..Default::default()
        },
    )
    .unwrap();
    for u in &s.by_unit {
        assert_eq!(
            u.order_units_with_waste,
            Some(with_waste(u.order_units, 10.0)),
            "{}",
            u.name
        );
    }
    let s = summarize(&r, &p, &Placement::default(), &SummaryOptions::default()).unwrap();
    assert!(s.by_unit.iter().all(|u| u.order_units_with_waste.is_none()));
}
