use super::*;
use crate::tin::SourceData;
use crate::volume::{cut_fill, cut_fill_in};
use glam::DVec3;

fn grid(n: i32, step: f64, from: f64, f: impl Fn(f64, f64) -> f64) -> Tin {
    let mut points = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (from + i as f64 * step, from + j as f64 * step);
            points.push(DVec3::new(x, y, f(x, y)));
        }
    }
    Tin::build(&SourceData {
        points,
        lines: vec![],
    })
    .unwrap()
}

fn flat(n: i32, step: f64, from: f64, z: f64) -> Tin {
    grid(n, step, from, |_, _| z)
}

fn square(lo: f64, hi: f64) -> Vec<DVec2> {
    vec![
        DVec2::new(lo, lo),
        DVec2::new(hi, lo),
        DVec2::new(hi, hi),
        DVec2::new(lo, hi),
    ]
}

fn pad(boundary: Vec<DVec2>, elevation: f64) -> Modifier {
    Modifier::Pad {
        boundary,
        elevation,
        slope: None,
    }
}

fn at(t: &Tin, x: f64, y: f64) -> f64 {
    t.elevation_at(DVec2::new(x, y)).unwrap()
}

#[test]
fn pad_elevation_flat_and_sloped() {
    let sq = square(0.0, 10.0);
    assert_eq!(pad_elevation(&sq, 5.0, None, DVec2::new(3.0, 3.0)), 5.0);
    let s = Some(PadSlope {
        direction: 0.0,
        percent: 10.0,
    });
    assert!((pad_elevation(&sq, 5.0, s, DVec2::new(5.0, 5.0)) - 5.0).abs() < 1e-12);
    assert!((pad_elevation(&sq, 5.0, s, DVec2::new(6.0, 0.0)) - 4.9).abs() < 1e-12);
    assert!((pad_elevation(&sq, 5.0, s, DVec2::new(5.0, 9.0)) - 5.0).abs() < 1e-12);
}

#[test]
fn inside_or_on_counts_the_edge() {
    let sq = square(0.0, 10.0);
    assert!(inside_or_on(&sq, DVec2::new(5.0, 5.0)));
    assert!(inside_or_on(&sq, DVec2::new(10.0, 4.0)));
    assert!(inside_or_on(&sq, DVec2::new(0.0, 0.0)));
    assert!(!inside_or_on(&sq, DVec2::new(10.1, 4.0)));
    assert!(!inside_or_on(&[], DVec2::ZERO));
    assert!(
        strictly_inside(&sq, DVec2::new(5.0, 5.0)) && !strictly_inside(&sq, DVec2::new(10.0, 4.0))
    );
}

#[test]
fn no_pads_leaves_the_surface_unchanged() {
    let existing = flat(5, 10.0, 0.0, 3.0);
    let limit_only = Modifier::GradeLimit {
        boundary: square(5.0, 25.0),
    };
    for mods in [vec![], vec![limit_only]] {
        let p = proposed_surface(&existing, &mods).unwrap();
        let r = cut_fill(&existing, &p);
        assert!(r.cut.abs() < 1e-9 && r.fill.abs() < 1e-9);
        assert_eq!(p.vertices.len(), existing.vertices.len());
    }
}

#[test]
fn box_pad_on_flat_ground_is_area_times_height() {
    let existing = flat(11, 5.0, -20.0, 0.0);
    let mods = vec![
        Modifier::GradeLimit {
            boundary: square(-1.0, 11.0),
        },
        pad(square(0.0, 10.0), 1.0),
    ];
    let proposed = proposed_surface(&existing, &mods).unwrap();
    let in_pad = cut_fill_in(&existing, &proposed, Some(&square(0.0, 10.0)));
    assert!((in_pad.fill - 100.0).abs() < 1e-9, "{in_pad:?}");
    assert!(in_pad.cut.abs() < 1e-12);
    let all = cut_fill(&existing, &proposed);
    assert!(all.cut.abs() < 1e-9);
    assert!((all.fill - 121.0 - 1.0 / 3.0).abs() < 1e-6, "{all:?}");
    assert!(at(&proposed, 20.0, 20.0).abs() < 1e-12);
    assert!((at(&proposed, 5.0, 5.0) - 1.0).abs() < 1e-12);
    let mid = at(&proposed, -0.5, 5.0);
    assert!(mid > 0.0 && mid < 1.0, "{mid}");
    assert!((proposed.area_xy() - existing.area_xy()).abs() < 1e-6);
}

#[test]
fn pad_below_ground_is_cut_and_sloped_pad_averages() {
    let existing = flat(7, 10.0, 0.0, 0.0);
    let one = |elevation, slope| {
        vec![Modifier::Pad {
            boundary: square(20.0, 40.0),
            elevation,
            slope,
        }]
    };
    let p = proposed_surface(&existing, &one(-2.0, None)).unwrap();
    let r = cut_fill_in(&existing, &p, Some(&square(20.0, 40.0)));
    assert!(
        (r.cut - 800.0).abs() < 1e-9 && r.fill.abs() < 1e-12,
        "{r:?}"
    );
    let slope = Some(PadSlope {
        direction: 0.0,
        percent: 5.0,
    });
    let p = proposed_surface(&existing, &one(0.0, slope)).unwrap();
    let r = cut_fill_in(&existing, &p, Some(&square(20.0, 40.0)));
    assert!(
        (r.cut - 50.0).abs() < 1e-9 && (r.fill - 50.0).abs() < 1e-9,
        "{r:?}"
    );
    assert!((at(&p, 20.000001, 25.0) - 0.5).abs() < 1e-5);
}

#[test]
fn the_hull_survives_a_pad_and_a_wide_limit() {
    let existing = flat(5, 10.0, 0.0, 0.0);
    let mods = vec![
        Modifier::GradeLimit {
            boundary: square(-5.0, 100.0),
        },
        pad(square(12.0, 28.0), 0.5),
    ];
    let p = proposed_surface(&existing, &mods).unwrap();
    assert!((p.area_xy() - existing.area_xy()).abs() < 1e-6);
    assert!(existing.boundary_vertices().iter().all(|i| {
        p.elevation_at(existing.vertices[*i as usize].truncate())
            .is_some()
    }));
    assert!(at(&p, 0.0, 0.0).abs() < 1e-9 && at(&p, 40.0, 40.0).abs() < 1e-9);
}

#[test]
fn pad_without_grade_limits_has_a_vertical_edge() {
    let existing = flat(11, 10.0, 0.0, 0.0);
    let p = proposed_surface(&existing, &[pad(square(23.0, 61.0), 7.0)]).unwrap();
    assert!(at(&p, 22.999999, 40.0).abs() < 1e-9);
    assert!((at(&p, 23.000001, 40.0) - 7.0).abs() < 1e-9);
    let r = cut_fill(&existing, &p);
    assert!((r.fill - 38.0 * 38.0 * 7.0).abs() < 1e-9, "{r:?}");
    assert!(r.cut.abs() < 1e-9);
    // The wall is four sides of 38 and 7 high. Quads are a1, b1, b2, a2.
    let area: f64 = p
        .walls()
        .iter()
        .map(|q| (q[1] - q[0]).length() * (q[3] - q[0]).length())
        .sum();
    assert!((area - 4.0 * 38.0 * 7.0).abs() < 1e-6, "{area}");
    let p = proposed_surface(&existing, &[pad(square(0.0, 30.0), 2.0)]).unwrap();
    assert!((cut_fill(&existing, &p).fill - 900.0 * 2.0).abs() < 1e-9);
}

#[test]
fn overlapping_pads_nest_and_the_last_one_is_on_top() {
    let existing = flat(11, 10.0, 0.0, 0.0);
    let a = pad(square(20.0, 60.0), 10.0);
    let b = pad(square(40.0, 80.0), 30.0);
    let p = proposed_surface(&existing, &[a.clone(), b.clone()]).unwrap();
    assert!((at(&p, 30.0, 30.0) - 10.0).abs() < 1e-9);
    assert!((at(&p, 70.0, 70.0) - 30.0).abs() < 1e-9);
    assert!((at(&p, 50.0, 50.0) - 30.0).abs() < 1e-9);
    assert!((at(&p, 30.0, 50.0) - 10.0).abs() < 1e-9);
    assert!(at(&p, 10.0, 10.0).abs() < 1e-9);
    let expect = (1600.0 - 400.0) * 10.0 + 1600.0 * 30.0;
    let r = cut_fill(&existing, &p);
    assert!((r.fill - expect).abs() < 1e-6, "{r:?} {expect}");
    let p = proposed_surface(&existing, &[b, a]).unwrap();
    assert!((at(&p, 50.0, 50.0) - 10.0).abs() < 1e-9);
}

#[test]
fn overlapping_pads_inside_a_grade_limit() {
    let existing = flat(13, 10.0, 0.0, 0.0);
    let mods = vec![
        Modifier::GradeLimit {
            boundary: square(5.0, 105.0),
        },
        pad(square(20.0, 60.0), 10.0),
        pad(square(40.0, 80.0), 30.0),
    ];
    let p = proposed_surface(&existing, &mods).unwrap();
    assert!((at(&p, 30.0, 30.0) - 10.0).abs() < 1e-9);
    assert!((at(&p, 70.0, 70.0) - 30.0).abs() < 1e-9);
    assert!(at(&p, 0.0, 0.0).abs() < 1e-12 && at(&p, 110.0, 110.0).abs() < 1e-12);
    let z = at(&p, 12.0, 30.0);
    assert!(z > 0.0 && z < 10.0, "{z}");
    assert!((p.area_xy() - existing.area_xy()).abs() < 1e-6);
}

#[test]
fn pad_partly_outside_the_site_is_clipped_to_it() {
    let existing = flat(3, 10.0, 0.0, 0.0);
    let outside = vec![
        DVec2::new(10.0, 0.0),
        DVec2::new(30.0, 0.0),
        DVec2::new(30.0, 10.0),
        DVec2::new(10.0, 10.0),
    ];
    let p = proposed_surface(&existing, &[pad(outside, 5.0)]).unwrap();
    let r = cut_fill(&existing, &p);
    assert!((r.fill - 10.0 * 10.0 * 5.0).abs() < 1e-9, "{r:?}");
    assert!((p.area_xy() - 400.0).abs() < 1e-9);
    assert!(p.elevation_at(DVec2::new(25.0, 5.0)).is_none());
    let mods = vec![
        Modifier::GradeLimit {
            boundary: square(-10.0, 40.0),
        },
        pad(square(5.0, 15.0), 1.0),
    ];
    let p = proposed_surface(&existing, &mods).unwrap();
    assert!((p.area_xy() - 400.0).abs() < 1e-6);
}

#[test]
fn ground_outside_the_work_does_not_change_on_curved_terrain() {
    let f = |x: f64, y: f64| 3.0 * (x * 0.07).sin() + 2.0 * (y * 0.11).cos() + 0.02 * x;
    let existing = grid(15, 10.0, 0.0, f);
    let samples: Vec<DVec2> = (0..40)
        .flat_map(|i| (0..40).map(move |j| DVec2::new(1.3 + i as f64 * 3.4, 0.7 + j as f64 * 3.4)))
        .collect();
    let limit = square(27.0, 93.0);
    let mods = vec![
        Modifier::GradeLimit {
            boundary: limit.clone(),
        },
        pad(square(45.0, 70.0), 12.0),
    ];
    let p = proposed_surface(&existing, &mods).unwrap();
    for s in &samples {
        if !inside_or_on(&limit, *s) {
            assert!(
                (p.elevation_at(*s).unwrap() - existing.elevation_at(*s).unwrap()).abs() < 1e-9,
                "{s}"
            );
        }
    }
    for q in [
        DVec2::new(27.000001, 50.0),
        DVec2::new(26.999999, 50.0),
        DVec2::new(60.0, 93.000001),
    ] {
        assert!((p.elevation_at(q).unwrap() - existing.elevation_at(q).unwrap()).abs() < 1e-4);
    }
    let wall = square(33.0, 71.0);
    let p = proposed_surface(&existing, &[pad(wall.clone(), 9.0)]).unwrap();
    for s in &samples {
        if !inside_or_on(&wall, *s) {
            assert!(
                (p.elevation_at(*s).unwrap() - existing.elevation_at(*s).unwrap()).abs() < 1e-9,
                "{s}"
            );
        }
    }
    let r = cut_fill(&existing, &p);
    let inside = cut_fill_in(&existing, &p, Some(&wall));
    assert!(
        (r.fill - inside.fill).abs() < 1e-6 && (r.cut - inside.cut).abs() < 1e-6,
        "{r:?} {inside:?}"
    );
}

#[test]
fn crop_cuts_a_concave_ring_out_of_the_hull() {
    let t = flat(5, 10.0, 0.0, 2.0);
    let l = vec![
        DVec2::new(0.0, 0.0),
        DVec2::new(40.0, 0.0),
        DVec2::new(40.0, 15.0),
        DVec2::new(15.0, 15.0),
        DVec2::new(15.0, 40.0),
        DVec2::new(0.0, 40.0),
    ];
    let c = crate::overlay::crop(&t, &l).unwrap();
    assert!((c.area_xy() - (40.0 * 15.0 + 15.0 * 25.0)).abs() < 1e-6);
    assert!(c.elevation_at(DVec2::new(30.0, 30.0)).is_none());
    assert!((at(&c, 30.0, 5.0) - 2.0).abs() < 1e-12);
    assert_eq!(c.z_range(), (2.0, 2.0));
    assert!((crate::overlay::crop(&t, &[]).unwrap().area_xy() - 1600.0).abs() < 1e-9);
}
