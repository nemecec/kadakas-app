use super::*;
use crate::curve::VertexKind;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn line(a: DVec2, b: DVec2) -> Shape {
    Shape::Line { a, b }
}

fn curves(shapes: &[Shape]) -> Vec<Curve> {
    shapes.iter().flat_map(|s| curves_of(s).unwrap()).collect()
}

#[test]
fn a_lot_of_loose_lines_with_gaps_and_an_island() {
    // 30 m by 20 m, four lines with a 0.4 mm gap at each corner, and a round island.
    let g = 0.4;
    let mut shapes = vec![
        line(v(g, 0.0), v(30_000.0, 0.0)),
        line(v(30_000.0, g), v(30_000.0, 20_000.0)),
        line(v(30_000.0 - g, 20_000.0), v(0.0, 20_000.0)),
        line(v(0.0, 20_000.0 - g), v(0.0, 0.0)),
    ];
    shapes.push(Shape::Circle {
        center: v(15_000.0, 10_000.0),
        radius: 2000.0,
    });
    let found = from_curves(&curves(&shapes), 1.0).unwrap();
    assert_eq!(found.regions.len(), 2);
    assert_eq!(found.closed_gap_count, 4);
    assert_eq!(found.dead_end_count, 0);
    let lot = found.regions.iter().find(|r| r.depth == 0).unwrap();
    let island = found.regions.iter().find(|r| r.depth == 1).unwrap();
    assert_eq!(lot.holes.len(), 1);
    let disc = PI * 2000.0 * 2000.0;
    assert!((island.area - disc).abs() < 1e-3);
    // Closing a gap of 0.4 mm moves a corner by 0.2 mm: a few thousand mm2 of 600 m2.
    assert!((lot.area - (600_000_000.0 - disc)).abs() < 20_000.0);
    // The island keeps its arcs: two arc points, not a polygon of many corners.
    assert_eq!(island.outer.len(), 4);
    assert_eq!(
        island
            .outer
            .iter()
            .filter(|v| v.kind == VertexKind::PointOnArc)
            .count(),
        2
    );
}

#[test]
fn a_rounded_corner_polyline_and_a_line_close() {
    // An open polyline with an arc, closed by one line.
    let poly = Shape::Polyline {
        points: vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::corner(v(1000.0, 0.0)),
            Vertex::new(v(1000.0, 1000.0), VertexKind::Arc { radius: 300.0 }),
            Vertex::corner(v(0.0, 1000.0)),
        ],
        closed: false,
    };
    let shapes = vec![poly, line(v(0.0, 1000.0), v(0.0, 0.0))];
    let found = from_curves(&curves(&shapes), 1.0).unwrap();
    assert_eq!(found.regions.len(), 1);
    // The square minus the corner that the fillet cuts off.
    let corner = 300.0 * 300.0 * (1.0 - PI / 4.0);
    assert!((found.regions[0].area - (1_000_000.0 - corner)).abs() < 1e-3);
}

#[test]
fn what_does_not_close_is_reported_with_the_gap() {
    let shapes = vec![
        line(v(0.0, 0.0), v(1000.0, 0.0)),
        line(v(1000.0, 0.0), v(1000.0, 1000.0)),
        line(v(1000.0, 1000.0), v(0.0, 1000.0)),
        line(v(0.0, 970.0), v(0.0, 0.0)),
    ];
    let found = from_curves(&curves(&shapes), 5.0).unwrap();
    assert!(found.regions.is_empty());
    assert_eq!(found.dead_end_count, 2);
    let d = found
        .dead_ends
        .iter()
        .find(|d| d.at == v(0.0, 970.0))
        .unwrap();
    let (at, size) = d.nearest.unwrap();
    assert_eq!(at, v(0.0, 1000.0));
    assert!((size - 30.0).abs() < 1e-9);
    // With a tolerance of 30 mm it closes, and says so.
    let found = from_curves(&curves(&shapes), 31.0).unwrap();
    assert_eq!(found.regions.len(), 1);
    assert_eq!(found.closed_gap_count, 1);
    assert!((found.closed_gaps[0].size - 30.0).abs() < 1e-9);
}

#[test]
fn curves_of_the_shapes() {
    // A 3D polyline is read in plan: the heights do not matter.
    let p3 = |closed: bool, n: usize| Shape::Polyline3d {
        points: [(0.0, 0.0, 5.0), (4.0, 0.0, 9.0), (4.0, 3.0, 1.0)][..n]
            .iter()
            .map(|&(x, y, z)| glam::DVec3::new(x, y, z))
            .collect(),
        closed,
    };
    assert_eq!(curves_of(&p3(true, 3)).unwrap().len(), 3);
    assert_eq!(curves_of(&p3(false, 3)).unwrap().len(), 2);
    assert!(curves_of(&p3(false, 1)).is_err());
    assert!(
        curves_of(&Shape::Polyline3d {
            points: vec![glam::DVec3::NAN, glam::DVec3::ZERO],
            closed: false
        })
        .is_err()
    );
    assert_eq!(
        curves_of(&Shape::Rect {
            min: v(0.0, 0.0),
            max: v(2.0, 1.0)
        })
        .unwrap()
        .len(),
        4
    );
    let full = curves_of(&Shape::Arc {
        center: v(0.0, 0.0),
        radius: 5.0,
        start: 0.0,
        end: TAU,
    })
    .unwrap();
    assert_eq!(full.len(), 2);
    let quarter = curves_of(&Shape::Arc {
        center: v(0.0, 0.0),
        radius: 5.0,
        start: 0.0,
        end: PI / 2.0,
    })
    .unwrap();
    assert!((quarter[0].length() - 5.0 * PI / 2.0).abs() < 1e-9);
    assert!(
        curves_of(&Shape::Arc {
            center: v(0.0, 0.0),
            radius: 5.0,
            start: 1.0,
            end: 1.0
        })
        .is_err()
    );
    assert!(
        curves_of(&Shape::Text {
            at: v(0.0, 0.0),
            text: "x".into(),
            height: 1.0
        })
        .is_err()
    );
    assert!(
        curves_of(&Shape::Polyline {
            points: vec![Vertex::corner(v(0.0, 0.0))],
            closed: false
        })
        .is_err()
    );
    // A circle through a polyline with a Bezier piece.
    let bez = curves_of_vertices(
        &[
            Vertex::corner(v(0.0, 0.0)),
            Vertex::new(v(50.0, 100.0), VertexKind::Bezier),
            Vertex::corner(v(100.0, 0.0)),
        ],
        false,
    );
    assert!(bez.len() > 2);
    // A closed polyline with a full-circle arc is two arcs.
    let c = curves_of(&Shape::Circle {
        center: v(0.0, 0.0),
        radius: 1.0,
    })
    .unwrap();
    assert_eq!(c.len(), 2);
}

#[test]
fn the_source_curves_of_a_region_are_listed() {
    let shapes = vec![
        line(v(0.0, 0.0), v(100.0, 0.0)),
        line(v(100.0, 0.0), v(100.0, 100.0)),
        line(v(100.0, 100.0), v(0.0, 100.0)),
        line(v(0.0, 100.0), v(0.0, 0.0)),
        line(v(500.0, 500.0), v(600.0, 500.0)),
    ];
    let found = from_curves(&curves(&shapes), 1.0).unwrap();
    assert_eq!(found.regions.len(), 1);
    assert_eq!(found.regions[0].curves, vec![0, 1, 2, 3]);
    assert_eq!(found.dead_end_count, 2);
    // A lone line is an open piece.
    assert_eq!(found.open_dead_ends, 2);
}
