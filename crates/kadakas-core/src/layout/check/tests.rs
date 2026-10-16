use super::*;
use crate::layout::LayoutError;
use crate::layout::course;
use crate::layout::plan::{Body, CourseBody, Role};
use crate::layout::{Bond, Pattern, Placement, zones};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region {
    Region::from_rings(&[v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)], &[]).unwrap()
}

fn group(name: &str, region: Region, bond: Bond, angle: f64) -> Group {
    Group {
        name: name.into(),
        slug: name.to_uppercase(),
        role: Role::Field,
        region,
        owner: None,
        body: Body::Pattern {
            pattern: Pattern::new(bond, 200.0, 100.0, 5.0),
            place: Placement {
                origin: Some(v(0.0, 0.0)),
                angle,
            },
        },
    }
}

#[test]
fn clean_layouts_have_no_findings() {
    for bond in [Bond::RunningBond, Bond::Stack, Bond::Herringbone] {
        for angle in [0.0, 30.0, 45.0] {
            let g = group("field", rect(0.0, 0.0, 4010.0, 2503.0), bond, angle);
            let r = check(
                &[g],
                &rect(0.0, 0.0, 4010.0, 2503.0),
                &CheckOptions::default(),
            )
            .unwrap();
            assert!(r.units > 100);
            // Small pieces are real at an odd size; everything else must be clean.
            for (k, n) in &r.counts {
                if k != "small_piece" {
                    assert_eq!(
                        *n,
                        0,
                        "{bond:?} at {angle}: {k}: {:?}",
                        r.findings
                            .iter()
                            .filter(|f| f.kind.id() == k)
                            .take(3)
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }
}

#[test]
fn a_basket_weave_with_wide_joints_is_not_a_fault() {
    let mut g = group(
        "field",
        rect(0.0, 0.0, 3000.0, 2000.0),
        Bond::BasketWeave,
        0.0,
    );
    if let Body::Pattern { pattern, .. } = &mut g.body {
        pattern.unit_length = 205.0;
        pattern.unit_width = 100.0;
    }
    let r = check(
        &[g],
        &rect(0.0, 0.0, 3000.0, 2000.0),
        &CheckOptions::default(),
    )
    .unwrap();
    assert_eq!(r.counts["joint_too_wide"], 0, "{:?}", r.findings);
    assert_eq!(r.counts["overlap"], 0);
}

#[test]
fn zones_that_share_an_edge_are_clean() {
    let region = rect(0.0, 0.0, 10_000.0, 6_000.0);
    let split = zones::split(
        &region,
        &[
            vec![v(3333.0, -10.0), v(3333.0, 6010.0)],
            vec![v(0.0, 2222.0), v(10_000.0, 2222.0)],
        ],
        1.0,
    )
    .unwrap();
    assert_eq!(split.zones.len(), 4);
    let groups: Vec<Group> = split
        .zones
        .iter()
        .enumerate()
        .map(|(i, z)| {
            let mut g = group(
                &format!("zone {}", i + 1),
                Region::from_shapes(vec![z.shape.clone()]).unwrap(),
                Bond::RunningBond,
                0.0,
            );
            g.role = Role::Zone;
            g
        })
        .collect();
    let r = check(&groups, &region, &CheckOptions::default()).unwrap();
    for (k, n) in &r.counts {
        if k != "small_piece" {
            assert_eq!(
                *n,
                0,
                "{k}: {:?}",
                r.findings
                    .iter()
                    .filter(|f| f.kind.id() == k)
                    .take(3)
                    .collect::<Vec<_>>()
            );
        }
    }
    let covered: f64 = r.coverage.iter().map(|c| c.units_area_m2).sum();
    assert!(covered > 0.0 && covered < 60.0);
}

#[test]
fn overlapping_groups_are_found_with_positions() {
    let a = group("a", rect(0.0, 0.0, 1000.0, 500.0), Bond::Stack, 0.0);
    let b = group("b", rect(500.0, 0.0, 1500.0, 500.0), Bond::Stack, 0.0);
    // The lattice of both starts at the origin: the units line up and overlap exactly.
    let region = rect(0.0, 0.0, 1500.0, 500.0);
    let r = check(&[a, b], &region, &CheckOptions::default()).unwrap();
    assert!(r.counts["overlap"] > 5, "{:?}", r.counts);
    let f = r.findings.iter().find(|f| f.kind == Kind::Overlap).unwrap();
    assert_eq!(f.group, "a");
    assert_eq!(f.other_group.as_deref(), Some("b"));
    assert!(
        f.at[0] >= 495.0 && f.at[0] <= 1005.0 && f.size > 1000.0,
        "{f:?}"
    );
    assert!(!r.ok);
}

#[test]
fn narrow_and_wide_joints_between_groups() {
    // Two areas 40 mm apart in an area that holds both: the stretch between is a wide joint.
    let area = rect(0.0, 0.0, 2040.0, 500.0);
    let a = group("a", rect(0.0, 0.0, 1000.0, 500.0), Bond::Stack, 0.0);
    let mut b = group("b", rect(1040.0, 0.0, 2040.0, 500.0), Bond::Stack, 0.0);
    if let Body::Pattern { place, .. } = &mut b.body {
        place.origin = Some(v(1040.0, 0.0));
    }
    let r = check(&[a, b], &area, &CheckOptions::default()).unwrap();
    assert!(r.counts["joint_too_wide"] >= 4, "{:?}", r.counts);
    let w = r
        .findings
        .iter()
        .find(|f| f.kind == Kind::JointTooWide)
        .unwrap();
    assert!(
        (w.size - 40.0).abs() < 1e-6 || (w.size - 45.0).abs() < 2.6,
        "{w:?}"
    );
    // Across a hole of the area it is no joint.
    let holey = Region::from_rings(
        &[v(0.0, 0.0), v(2040.0, 0.0), v(2040.0, 500.0), v(0.0, 500.0)],
        &[vec![
            v(1005.0, -10.0),
            v(1035.0, -10.0),
            v(1035.0, 510.0),
            v(1005.0, 510.0),
        ]],
    )
    .unwrap();
    let a = group("a", rect(0.0, 0.0, 1000.0, 500.0), Bond::Stack, 0.0);
    let mut b = group("b", rect(1040.0, 0.0, 2040.0, 500.0), Bond::Stack, 0.0);
    if let Body::Pattern { place, .. } = &mut b.body {
        place.origin = Some(v(1040.0, 0.0));
    }
    let r = check(&[a, b], &holey, &CheckOptions::default()).unwrap();
    assert_eq!(r.counts["joint_too_wide"], 0);
    // Two areas 1 mm apart: the joint is narrower than the gap of 5 mm.
    let area = rect(0.0, 0.0, 2001.0, 500.0);
    let a = group("a", rect(0.0, 0.0, 1000.0, 500.0), Bond::Stack, 0.0);
    let mut b = group("b", rect(1001.0, 0.0, 2001.0, 500.0), Bond::Stack, 0.0);
    if let Body::Pattern { place, .. } = &mut b.body {
        place.origin = Some(v(1001.0, 0.0));
    }
    let r = check(&[a, b], &area, &CheckOptions::default()).unwrap();
    assert!(r.counts["joint_too_narrow"] >= 1, "{:?}", r.counts);
}

#[test]
fn small_pieces_and_the_listing_limit() {
    let g = group("field", rect(0.0, 0.0, 1910.0, 1000.0), Bond::Stack, 0.0);
    let opts = CheckOptions {
        min_share: 0.4,
        max_findings: 2,
        ..Default::default()
    };
    let r = check(&[g], &rect(0.0, 0.0, 1910.0, 1000.0), &opts).unwrap();
    // Units of 200 with joints of 5 start at 2.5: the tenth column starts at 1847.5 and is 62.5
    // wide at the edge (31 percent of a unit).
    assert_eq!(r.counts["small_piece"], 10);
    let g = group("field", rect(0.0, 0.0, 2020.0, 1000.0), Bond::Stack, 0.0);
    let opts = CheckOptions {
        min_share: 0.99,
        max_findings: 2,
        ..Default::default()
    };
    let r = check(&[g], &rect(0.0, 0.0, 2020.0, 1000.0), &opts).unwrap();
    assert!(r.counts["small_piece"] > 2);
    assert_eq!(
        r.findings
            .iter()
            .filter(|f| f.kind == Kind::SmallPiece)
            .count(),
        2
    );
    assert_eq!(r.truncated, vec!["small_piece".to_string()]);
}

#[test]
fn a_whole_unit_of_the_wrong_size_is_found() {
    // A course whose unit type says 25,000 mm2 while the pieces are 20,000 mm2.
    let spec = course::Spec {
        path: vec![
            crate::curve::Vertex::corner(v(0.0, 0.0)),
            crate::curve::Vertex::corner(v(1000.0, 0.0)),
        ],
        closed: false,
        side: course::Side::Left,
        offset: 0.0,
        along: 100.0,
        depth: 200.0,
        gap: 5.0,
        corner: course::Corner::Miter,
    };
    let laid = course::lay(&spec, 1000).unwrap();
    let n = laid.pieces.len();
    let g = Group {
        name: "kerb".into(),
        slug: "COURSE-KERB".into(),
        role: Role::Course,
        region: rect(-100.0, -100.0, 1100.0, 400.0),
        owner: None,
        body: Body::Course(Box::new(CourseBody {
            pieces: laid.pieces,
            piece_kind: vec![0; n],
            kinds: vec![("100x200".into(), 25_000.0)],
            joints: laid.joints,
            corners: 0,
            length: 1000.0,
            along: 100.0,
            depth: 200.0,
            gap: 5.0,
            band: Vec::new(),
        })),
    };
    let r = check(
        &[g],
        &rect(-100.0, -100.0, 1100.0, 400.0),
        &CheckOptions::default(),
    )
    .unwrap();
    assert!(r.counts["wrong_size"] >= 9, "{:?}", r.counts);
    assert_eq!(r.counts["overlap"], 0);
    assert_eq!(r.counts["joint_too_narrow"], 0, "{:?}", r.findings);
}

#[test]
fn the_area_outside_a_region() {
    let region = rect(0.0, 0.0, 100.0, 100.0);
    let ring = [
        v(50.0, 50.0),
        v(150.0, 50.0),
        v(150.0, 150.0),
        v(50.0, 150.0),
    ];
    assert!((outside_area(&ring, &region) - 7500.0).abs() < 1e-6);
    let inside = [v(10.0, 10.0), v(20.0, 10.0), v(20.0, 20.0), v(10.0, 20.0)];
    assert!(outside_area(&inside, &region).abs() < 1e-9);
}

#[test]
fn convexity_and_depth() {
    let a = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)];
    let b = [v(9.0, 0.0), v(19.0, 0.0), v(19.0, 10.0), v(9.0, 10.0)];
    let c = [v(10.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(10.0, 10.0)];
    assert!((sat_depth(&a, &b) - 1.0).abs() < 1e-9);
    assert_eq!(sat_depth(&a, &c), 0.0);
    assert!(is_convex(&a));
    assert!(!is_convex(&[
        v(0.0, 0.0),
        v(10.0, 0.0),
        v(5.0, 2.0),
        v(10.0, 10.0),
        v(0.0, 10.0)
    ]));
    assert_eq!(Kind::Overlap.id(), "overlap");
}

#[test]
fn the_area_of_a_group_is_checked_against_its_units() {
    let area = rect(0.0, 0.0, 2000.0, 1000.0);
    // The group claims a smaller area than its units cover: an error.
    let mut small = group("a", area.clone(), Bond::Stack, 0.0);
    small.region.area *= 0.5;
    let r = check(&[small], &area, &CheckOptions::default()).unwrap();
    assert_eq!(r.counts["coverage"], 1, "{:?}", r.findings);
    assert!(r.findings[0].message.contains("more than its area"));
    // The group claims a bigger area than the joints explain: units are missing.
    let mut big = group("a", area.clone(), Bond::Stack, 0.0);
    big.region.area *= 1.5;
    let r = check(&[big], &area, &CheckOptions::default()).unwrap();
    assert_eq!(r.counts["coverage"], 1, "{:?}", r.findings);
    assert!(r.findings[0].message.contains("units may be missing"));
    let c = &r.coverage[0];
    assert!(c.open_share > 0.3 && c.expected_open_share.unwrap() < 0.1);
    assert!(!r.ok);
}

#[test]
fn units_that_touch_in_one_group_are_a_joint_of_zero() {
    // A stack without a gap: the units touch. Only a limit above zero sees it.
    let mut g = group("a", rect(0.0, 0.0, 1000.0, 500.0), Bond::Stack, 0.0);
    if let Body::Pattern { pattern, .. } = &mut g.body {
        pattern.gap = 0.0;
    }
    let area = rect(0.0, 0.0, 1000.0, 500.0);
    let r = check(std::slice::from_ref(&g), &area, &CheckOptions::default()).unwrap();
    assert_eq!(r.counts["joint_too_narrow"], 0);
    let opts = CheckOptions {
        min_joint_mm: Some(1.0),
        ..Default::default()
    };
    let r = check(&[g], &area, &opts).unwrap();
    assert!(r.counts["joint_too_narrow"] > 10, "{:?}", r.counts);
    assert!(r.findings.iter().all(|f| f.size < 0.2));
}

#[test]
fn narrow_pieces_and_the_unit_limit() {
    let area = rect(0.0, 0.0, 1910.0, 1000.0);
    let g = group("a", area.clone(), Bond::Stack, 0.0);
    let opts = CheckOptions {
        min_width_mm: 90.0,
        min_share: 0.0,
        ..Default::default()
    };
    let r = check(std::slice::from_ref(&g), &area, &opts).unwrap();
    // The cut column is 62.5 mm wide and the cut row 52.5 mm tall.
    assert!(r.counts["small_piece"] >= 10, "{:?}", r.counts);
    let few = CheckOptions {
        max_units: 10,
        ..Default::default()
    };
    assert!(matches!(
        check(&[g], &area, &few),
        Err(LayoutError::TooMany(_))
    ));
    assert_eq!(
        Kind::ALL.map(Kind::id),
        [
            "overlap",
            "joint_too_narrow",
            "joint_too_wide",
            "small_piece",
            "wrong_size",
            "outside_area",
            "coverage"
        ]
    );
    assert_eq!(MAX_UNITS, 500_000);
}

#[test]
fn a_hole_in_a_course_is_found() {
    use crate::curve::Vertex;
    let spec = course::Spec {
        path: vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::corner(v(2000.0, 0.0)),
            Vertex::corner(v(2000.0, 1500.0)),
        ],
        closed: false,
        side: course::Side::Left,
        offset: 0.0,
        along: 100.0,
        depth: 200.0,
        gap: 5.0,
        corner: course::Corner::Miter,
    };
    let laid = course::lay(&spec, 10_000).unwrap();
    let make = |pieces: Vec<course::Piece>| {
        let n = pieces.len();
        Group {
            name: "kerb".into(),
            slug: "COURSE-KERB".into(),
            role: Role::Course,
            region: rect(-300.0, -300.0, 2300.0, 1800.0),
            owner: None,
            body: Body::Course(Box::new(CourseBody {
                pieces,
                piece_kind: vec![0; n],
                kinds: vec![("100x200".into(), 20_000.0)],
                joints: laid.joints,
                corners: 1,
                length: 3500.0,
                along: 100.0,
                depth: 200.0,
                gap: 5.0,
                band: laid.band.clone(),
            })),
        }
    };
    let area = rect(-300.0, -300.0, 2300.0, 1800.0);
    let full = check(
        &[make(laid.pieces.clone())],
        &area,
        &CheckOptions::default(),
    )
    .unwrap();
    assert_eq!(full.counts["coverage"], 0, "{:?}", full.findings);
    // Take out 6 units in a row: the band has a hole there.
    let mut pieces = laid.pieces.clone();
    pieces.drain(10..16);
    let r = check(&[make(pieces)], &area, &CheckOptions::default()).unwrap();
    assert_eq!(r.counts["coverage"], 1, "{:?}", r.findings);
    let f = r
        .findings
        .iter()
        .find(|f| f.kind == Kind::Coverage)
        .unwrap();
    assert!(
        f.message.contains("leaves") && f.at[1] > 0.0 && f.at[1] < 205.0,
        "{f:?}"
    );
}
