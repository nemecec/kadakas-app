//! Joint patterns: the lines between paving units, clipped to a polygon. The work is done by the
//! layout engine (`crate::layout`); this module keeps the names that the hardscape code uses.

use glam::DVec2;

use super::style::JointPattern;
use crate::layout::{Placement, Region};

pub use crate::layout::Segment;
pub use crate::layout::gaps::{MAX_DRAW_SEGMENTS as MAX_SEGMENTS, clip_segment};

/// The joint lines of a pattern inside a polygon. `truncated` is true when the pattern would have
/// too many lines: nothing is drawn then.
pub type Joints = crate::layout::Gaps;

/// The joint lines of `pattern` inside the polygon. The pattern starts at the middle of the box of
/// the polygon, so a rectangle gets a symmetric layout. A pattern that is too fine for the polygon
/// is drawn with larger units, and if that does not help, it is left out and `truncated` is true.
pub fn joint_lines(poly: &[DVec2], pattern: &JointPattern) -> Joints {
    joint_lines_in(
        poly,
        &[],
        pattern,
        &Placement {
            origin: None,
            angle: pattern.angle,
        },
    )
}

/// Like [`joint_lines`], with holes and a placement.
pub fn joint_lines_in(
    poly: &[DVec2],
    holes: &[Vec<DVec2>],
    pattern: &JointPattern,
    place: &Placement,
) -> Joints {
    match Region::from_rings(poly, holes) {
        Ok(region) => {
            crate::layout::gap_lines(&region, &pattern.layout(), place, MAX_SEGMENTS, true)
        }
        Err(_) => Joints::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardscape::style::JointKind;

    fn rect(w: f64, h: f64) -> Vec<DVec2> {
        vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(w, 0.0),
            DVec2::new(w, h),
            DVec2::new(0.0, h),
        ]
    }

    fn pat(kind: JointKind) -> JointPattern {
        JointPattern {
            kind,
            unit_length: 200.0,
            unit_width: 100.0,
            spacing: 0.0,
            angle: 0.0,
            color: None,
            units: Vec::new(),
            a: None,
            b: None,
            generator: None,
        }
    }

    fn total_length(j: &Joints) -> f64 {
        j.segments.iter().map(|(a, b)| a.distance(*b)).sum()
    }

    fn inside_all(j: &Joints, poly: &[DVec2]) -> bool {
        let n = poly.len();
        j.segments.iter().all(|(a, b)| {
            let m = (*a + *b) / 2.0;
            let on_edge =
                (0..n).any(|i| crate::geom::dist_to_segment(m, poly[i], poly[(i + 1) % n]) < 1e-6);
            on_edge || crate::geom::point_in_polygon(m, poly)
        })
    }

    #[test]
    fn stack_lines_cover_a_rectangle_exactly() {
        let j = joint_lines(&rect(1000.0, 500.0), &pat(JointKind::Stack));
        assert!(!j.truncated);
        // Vertical joints every 200 mm and horizontal every 100 mm, with the layout centered.
        // Edges of the polygon may add lines. Interior length: 4 verticals + 4 horizontals plus the
        // boundary lines that the pattern puts on the edge.
        assert!(inside_all(&j, &rect(1000.0, 500.0)));
        assert!(total_length(&j) >= 4.0 * 500.0 + 4.0 * 1000.0);
    }

    #[test]
    fn running_bond_shifts_alternate_rows() {
        let poly = rect(1000.0, 400.0);
        let j = joint_lines(&poly, &pat(JointKind::RunningBond));
        let verticals: Vec<&Segment> = j
            .segments
            .iter()
            .filter(|(a, b)| (a.x - b.x).abs() < 1e-9)
            .collect();
        assert!(!verticals.is_empty());
        let mut rows: std::collections::BTreeMap<i64, Vec<f64>> = Default::default();
        for (a, b) in &verticals {
            rows.entry(((a.y + b.y) / 2.0 / 100.0).floor() as i64)
                .or_default()
                .push(a.x);
        }
        let mut keys = rows.keys();
        let (r0, r1) = (&rows[keys.next().unwrap()], &rows[keys.next().unwrap()]);
        let frac = |x: f64| (x / 200.0).rem_euclid(1.0);
        assert!(
            (frac(r0[0]) - frac(r1[0]))
                .abs()
                .min(1.0 - (frac(r0[0]) - frac(r1[0])).abs())
                > 0.4,
            "rows are shifted by half a unit"
        );
        assert!(inside_all(&j, &poly));
    }

    #[test]
    fn every_kind_stays_inside_a_concave_polygon_and_turns() {
        let l = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(3000.0, 0.0),
            DVec2::new(3000.0, 1000.0),
            DVec2::new(1000.0, 1000.0),
            DVec2::new(1000.0, 3000.0),
            DVec2::new(0.0, 3000.0),
        ];
        for kind in JointKind::ALL {
            for angle in [0.0, 30.0, 90.0, -45.0] {
                let p = JointPattern {
                    angle,
                    spacing: 5.0,
                    ..pat(kind)
                };
                let j = joint_lines(&l, &p);
                assert!(!j.truncated && !j.segments.is_empty(), "{kind:?} {angle}");
                assert!(inside_all(&j, &l), "{kind:?} {angle}");
            }
        }
        // A turn of 90 degrees maps a stack pattern of a square unit onto itself.
        let sq = JointPattern {
            unit_length: 100.0,
            ..pat(JointKind::Stack)
        };
        let a = joint_lines(&rect(1000.0, 1000.0), &sq);
        let b = joint_lines(&rect(1000.0, 1000.0), &JointPattern { angle: 90.0, ..sq });
        assert!((total_length(&a) - total_length(&b)).abs() < 1e-6);
    }

    #[test]
    fn herringbone_bricks_tile_the_plane_without_overlap() {
        // Joint length per area of a tiling by 2x1 bricks: each brick has perimeter 6 and every edge is shared by two bricks.
        // Inside a large rectangle the length per cell area tends to 3 / 2 per brick area (2 cells), i.e. 1.5 per cell.
        let cell = 100.0;
        let poly = rect(8_000.0, 8_000.0);
        let j = joint_lines(&poly, &pat(JointKind::Herringbone));
        assert!(!j.coarsened);
        let per_cell = total_length(&j) / (8_000.0 * 8_000.0 / (cell * cell)) / cell;
        assert!((per_cell - 1.5).abs() < 0.05, "{per_cell}");
        assert!(inside_all(&j, &poly));
    }

    #[test]
    fn basket_weave_has_blocks_and_midlines() {
        let poly = rect(800.0, 800.0);
        let j = joint_lines(&poly, &pat(JointKind::BasketWeave));
        assert!(!j.segments.is_empty());
        assert!(inside_all(&j, &poly));
    }

    #[test]
    fn too_many_lines_are_left_out() {
        let tiny = JointPattern {
            unit_length: 10.0,
            unit_width: 10.0,
            spacing: 0.0,
            ..pat(JointKind::RunningBond)
        };
        for kind in JointKind::ALL {
            let j = joint_lines(
                &rect(1.0e7, 1.0e7),
                &JointPattern {
                    kind,
                    ..tiny.clone()
                },
            );
            assert!(j.truncated && j.segments.is_empty(), "{kind:?}");
        }
    }

    #[test]
    fn degenerate_input_gives_nothing() {
        assert_eq!(joint_lines(&[], &pat(JointKind::Stack)), Joints::default());
        let zero = JointPattern {
            unit_length: 0.0,
            ..pat(JointKind::Stack)
        };
        assert_eq!(joint_lines(&rect(10.0, 10.0), &zero), Joints::default());
    }

    #[test]
    fn clip_keeps_inside_parts_only() {
        let poly = rect(100.0, 100.0);
        let s = clip_segment(DVec2::new(-50.0, 50.0), DVec2::new(150.0, 50.0), &poly);
        assert_eq!(s.len(), 1);
        assert!((s[0].0.x - 0.0).abs() < 1e-9 && (s[0].1.x - 100.0).abs() < 1e-9);
        assert!(clip_segment(DVec2::new(-50.0, 200.0), DVec2::new(150.0, 200.0), &poly).is_empty());
        // A U-shape: a line through both arms gives two pieces.
        let u = vec![
            DVec2::new(0.0, 0.0),
            DVec2::new(30.0, 0.0),
            DVec2::new(30.0, 60.0),
            DVec2::new(60.0, 60.0),
            DVec2::new(60.0, 0.0),
            DVec2::new(90.0, 0.0),
            DVec2::new(90.0, 100.0),
            DVec2::new(0.0, 100.0),
        ];
        assert_eq!(
            clip_segment(DVec2::new(-10.0, 30.0), DVec2::new(100.0, 30.0), &u).len(),
            2
        );
    }
}
