//! Surface operations on closed shapes: add, subtract (clip), intersect, compose and cut by a line.
//!
//! The work is done with polygons (the `i_overlay` crate). A rectangle is a polygon of four
//! corners. A circle, an arc segment and a polyline with curved vertices are flattened first:
//! every chord is at most `BOOL_TOLERANCE` mm from the curve. So the result of an operation that
//! touches a curved edge is a polygon with many short edges, and its area differs from the exact
//! area by less than the tolerance times the length of the curved edge. Edges that are straight
//! in the input stay exact. Open polylines count as closed, as they do in Vectorworks.
//!
//! A result may have holes. A polyline cannot, so a hole is joined to the outer ring by a bridge of
//! no width (the same way the hardscape outlines do it). The area of the ring is then the area of
//! the outer ring minus the holes.

use glam::DVec2;
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

use crate::curve;
use crate::hardscape::offset::bridge;
use crate::model::Shape;

use super::piece::{Chain, arc_steps};

/// Largest distance between a curve and its flattened chords, in mm.
pub const BOOL_TOLERANCE: f64 = 0.05;

/// A closed ring of points without a repeated first point.
pub type Ring = Vec<DVec2>;
/// A polygon: the outer ring first, then the holes.
pub type Poly = Vec<Ring>;

type Contour = Vec<[f64; 2]>;

fn to_contour(r: &Ring) -> Contour {
    r.iter().map(|p| [p.x, p.y]).collect()
}

fn from_contour(c: &Contour) -> Ring {
    c.iter().map(|p| DVec2::new(p[0], p[1])).collect()
}

/// Twice the signed area of a ring (counterclockwise is positive).
pub fn ring_area2(r: &[DVec2]) -> f64 {
    let n = r.len();
    if n < 3 {
        return 0.0;
    }
    let o = r[0];
    (0..n).map(|i| (r[i] - o).perp_dot(r[(i + 1) % n] - o)).sum()
}

/// The area of a polygon: the outer ring minus the holes.
pub fn poly_area(p: &Poly) -> f64 {
    p.iter()
        .enumerate()
        .map(|(i, r)| {
            let a = ring_area2(r).abs() / 2.0;
            if i == 0 { a } else { -a }
        })
        .sum()
}

fn ccw(mut r: Ring) -> Ring {
    if ring_area2(&r) < 0.0 {
        r.reverse();
    }
    r
}

/// The ring of a shape that can be used in a surface operation: a rectangle, a circle or a
/// polyline with three or more corners. `None` for other shapes and for rings without area.
pub fn ring_of(shape: &Shape) -> Option<Ring> {
    let ring = match shape {
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)]
        }
        Shape::Circle { center, radius } => {
            let k = arc_steps(*radius, std::f64::consts::TAU, BOOL_TOLERANCE).max(8);
            // The corners lie a little outside the circle, so that the polygon has the exact
            // area of the circle. The corners and the middles of the chords are both within the
            // tolerance of the circle.
            let kf = k as f64;
            let r = radius * (std::f64::consts::TAU / (kf * (std::f64::consts::TAU / kf).sin())).sqrt();
            (0..k)
                .map(|i| *center + DVec2::from_angle(std::f64::consts::TAU * i as f64 / kf) * r)
                .collect()
        }
        Shape::Polyline { points, .. } if points.len() >= 3 => {
            curve::flatten(points, true, BOOL_TOLERANCE)
        }
        _ => return None,
    };
    (ring.len() >= 3 && ring_area2(&ring).abs() > 0.0).then(|| ccw(ring))
}

fn contours(rings: &[Ring]) -> Vec<Contour> {
    rings.iter().map(|r| to_contour(&ccw(r.clone()))).collect()
}

fn polys(shapes: Vec<Vec<Vec<[f64; 2]>>>) -> Vec<Poly> {
    shapes
        .into_iter()
        .map(|s| s.iter().map(from_contour).collect::<Poly>())
        .filter(|p: &Poly| !p.is_empty() && poly_area(p) > 0.0)
        .collect()
}

fn flat(p: &[Poly]) -> Vec<Contour> {
    p.iter().flatten().map(to_contour).collect()
}

/// The union of the rings.
pub fn union(rings: &[Ring]) -> Vec<Poly> {
    if rings.is_empty() {
        return Vec::new();
    }
    polys(contours(rings).overlay(
        &Vec::<Contour>::new(),
        OverlayRule::Union,
        FillRule::NonZero,
    ))
}

/// `base` without the parts that `cutters` cover.
pub fn subtract(base: &Ring, cutters: &[Ring]) -> Vec<Poly> {
    polys(contours(std::slice::from_ref(base)).overlay(
        &contours(cutters),
        OverlayRule::Difference,
        FillRule::NonZero,
    ))
}

/// The part that all rings share.
pub fn intersect(rings: &[Ring]) -> Vec<Poly> {
    let Some((first, rest)) = rings.split_first() else {
        return Vec::new();
    };
    let mut acc: Vec<Poly> = vec![vec![ccw(first.clone())]];
    for r in rest {
        let subj = flat(&acc);
        acc = polys(subj.overlay(&contours(std::slice::from_ref(r)), OverlayRule::Intersect, FillRule::NonZero));
        if acc.is_empty() {
            break;
        }
    }
    acc
}

/// A piece of a composed set.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub polys: Vec<Poly>,
    /// Which input rings cover this region (indices, ascending).
    pub members: Vec<usize>,
}

/// Splits overlapping rings into regions that do not overlap: the part of each ring that no
/// other ring covers, and every overlap. The regions cover the union of the rings exactly once.
pub fn compose(rings: &[Ring]) -> Vec<Region> {
    let mut regions: Vec<Region> = Vec::new();
    for (i, ring) in rings.iter().enumerate() {
        let s = contours(std::slice::from_ref(ring));
        let mut rest: Vec<Contour> = s.clone();
        let mut next = Vec::new();
        for reg in regions {
            let r = flat(&reg.polys);
            let both = polys(r.overlay(&s, OverlayRule::Intersect, FillRule::NonZero));
            let only = polys(r.overlay(&s, OverlayRule::Difference, FillRule::NonZero));
            if !both.is_empty() {
                let mut m = reg.members.clone();
                m.push(i);
                next.push(Region {
                    polys: both,
                    members: m,
                });
                rest = rest_after(rest, &r);
            }
            if !only.is_empty() {
                next.push(Region {
                    polys: only,
                    members: reg.members,
                });
            }
        }
        let tail = polys(rest.overlay(
            &Vec::<Contour>::new(),
            OverlayRule::Union,
            FillRule::NonZero,
        ));
        if !tail.is_empty() {
            next.push(Region {
                polys: tail,
                members: vec![i],
            });
        }
        regions = next;
    }
    regions
}

fn rest_after(rest: Vec<Contour>, cover: &[Contour]) -> Vec<Contour> {
    let out = rest.overlay(&cover.to_vec(), OverlayRule::Difference, FillRule::NonZero);
    out.into_iter().flatten().collect()
}

/// Cuts a polygon on the infinite line through `a` and `b`. Returns the polygons on the left of
/// the line (seen from `a` to `b`) and then those on the right.
pub fn cut_by_line(poly: &Poly, a: DVec2, b: DVec2) -> Result<(Vec<Poly>, Vec<Poly>), String> {
    let u = (b - a)
        .try_normalize()
        .ok_or_else(|| "the cutting line needs two different points".to_string())?;
    let n = DVec2::new(-u.y, u.x);
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in poly.iter().flatten() {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let reach = 2.0 * (hi - lo).length() + 2.0 * ((lo + hi) / 2.0 - a).length() + 1.0;
    let side = |s: f64| -> Contour {
        let (p, q) = (a - u * reach, a + u * reach);
        let ring = vec![p, q, q + n * (s * reach), p + n * (s * reach)];
        to_contour(&ccw(ring))
    };
    let subj: Vec<Contour> = poly.iter().map(to_contour).collect();
    let run = |s: f64| polys(subj.overlay(&vec![side(s)], OverlayRule::Intersect, FillRule::NonZero));
    // The holes of a polygon have the opposite direction: the overlay handles that with the
    // non-zero rule only if the outer ring is counterclockwise.
    Ok((run(1.0), run(-1.0)))
}

/// The ring of a polygon: the outer ring with each hole joined by a bridge of no width.
pub fn poly_ring(p: &Poly) -> Ring {
    let mut ring = p[0].clone();
    for hole in &p[1..] {
        ring = bridge(ring, hole.clone());
    }
    ring
}

/// A closed polyline for a polygon (see `poly_ring`).
pub fn poly_shape(p: &Poly) -> Shape {
    Shape::Polyline {
        points: curve::corners(&poly_ring(p)),
        closed: true,
    }
}

/// The closed chain of a ring, for tools that work with chains.
pub fn ring_chain(r: &Ring) -> Option<Chain> {
    Chain::from_vertices(&curve::corners(r), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn sq(x: f64, y: f64, s: f64) -> Ring {
        vec![v(x, y), v(x + s, y), v(x + s, y + s), v(x, y + s)]
    }

    fn area(p: &[Poly]) -> f64 {
        p.iter().map(poly_area).sum()
    }

    #[test]
    fn union_subtract_intersect_areas() {
        let (a, b) = (sq(0.0, 0.0, 10.0), sq(5.0, 5.0, 10.0));
        assert!((area(&union(&[a.clone(), b.clone()])) - 175.0).abs() < 1e-9);
        assert!((area(&subtract(&a, &[b.clone()])) - 75.0).abs() < 1e-9);
        assert!((area(&intersect(&[a.clone(), b.clone()])) - 25.0).abs() < 1e-9);
        // Apart: two polygons.
        let far = sq(50.0, 0.0, 1.0);
        assert_eq!(union(&[a.clone(), far.clone()]).len(), 2);
        assert!(intersect(&[a.clone(), far]).is_empty());
        assert!(union(&[]).is_empty());
        assert!(intersect(&[]).is_empty());
    }

    #[test]
    fn a_hole_keeps_the_area() {
        let hole = sq(3.0, 3.0, 4.0);
        let r = subtract(&sq(0.0, 0.0, 10.0), &[hole]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].len(), 2);
        assert!((area(&r) - 84.0).abs() < 1e-9);
        // The bridged ring has the same area.
        let ring = poly_ring(&r[0]);
        assert!((ring_area2(&ring).abs() / 2.0 - 84.0).abs() < 1e-6);
        assert!(matches!(poly_shape(&r[0]), Shape::Polyline { closed: true, .. }));
    }

    #[test]
    fn compose_splits_into_regions() {
        let (a, b) = (sq(0.0, 0.0, 10.0), sq(5.0, 5.0, 10.0));
        let regions = compose(&[a.clone(), b.clone()]);
        assert_eq!(regions.len(), 3);
        let total: f64 = regions.iter().map(|r| area(&r.polys)).sum();
        assert!((total - 175.0).abs() < 1e-9);
        let both = regions.iter().find(|r| r.members == vec![0, 1]).unwrap();
        assert!((area(&both.polys) - 25.0).abs() < 1e-9);
        let c = sq(8.0, 0.0, 4.0);
        let regions = compose(&[a, b, c]);
        let total: f64 = regions.iter().map(|r| area(&r.polys)).sum();
        // The union of the three squares: 175 plus the part of c outside the others (4*4 - 2*4... computed).
        let all = union(&[sq(0.0, 0.0, 10.0), sq(5.0, 5.0, 10.0), sq(8.0, 0.0, 4.0)]);
        assert!((total - area(&all)).abs() < 1e-9);
        assert!(regions.iter().any(|r| r.members.len() == 3 || r.members.len() == 2));
    }

    #[test]
    fn cut_by_a_line_keeps_the_area() {
        let p: Poly = vec![ccw(sq(0.0, 0.0, 10.0))];
        let (l, r) = cut_by_line(&p, v(4.0, -5.0), v(4.0, 15.0)).unwrap();
        // The line goes up: the left side is x < 4.
        assert!((area(&l) - 40.0).abs() < 1e-9);
        assert!((area(&r) - 60.0).abs() < 1e-9);
        // A line that misses the polygon gives one side only.
        let (l, r) = cut_by_line(&p, v(-4.0, -5.0), v(-4.0, 15.0)).unwrap();
        assert!(l.is_empty());
        assert!((area(&r) - 100.0).abs() < 1e-9);
        assert!(cut_by_line(&p, v(1.0, 1.0), v(1.0, 1.0)).is_err());
    }

    #[test]
    fn cut_a_polygon_with_a_hole() {
        let r = subtract(&sq(0.0, 0.0, 10.0), &[sq(3.0, 3.0, 4.0)]);
        let (l, rr) = cut_by_line(&r[0], v(5.0, -1.0), v(5.0, 11.0)).unwrap();
        assert!((area(&l) + area(&rr) - 84.0).abs() < 1e-9);
    }

    #[test]
    fn rings_of_shapes() {
        let c = ring_of(&Shape::Circle {
            center: v(0.0, 0.0),
            radius: 100.0,
        })
        .unwrap();
        let a = ring_area2(&c) / 2.0;
        let exact = std::f64::consts::PI * 100.0 * 100.0;
        // The polygon has the area of the circle, and its corners are within the tolerance.
        assert!((a - exact).abs() < 1e-6 * exact);
        assert!(c.iter().all(|p| (p.length() - 100.0).abs() <= BOOL_TOLERANCE));
        assert!(ring_of(&Shape::Line { a: v(0.0, 0.0), b: v(1.0, 1.0) }).is_none());
        assert!(ring_of(&Shape::Rect { min: v(0.0, 0.0), max: v(0.0, 5.0) }).is_none());
        // An open polyline counts as closed.
        let p = Shape::Polyline {
            points: curve::corners(&[v(0.0, 0.0), v(4.0, 0.0), v(4.0, 4.0)]),
            closed: false,
        };
        assert_eq!(ring_of(&p).unwrap().len(), 3);
        assert!(ring_chain(&sq(0.0, 0.0, 1.0)).unwrap().closed);
    }
}
