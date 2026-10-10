//! Offset: a parallel copy of a line, arc, circle, rectangle or polyline at a distance.
//!
//! Lines move sideways. Arcs and circles keep their center and change their radius. A polyline
//! with arcs offsets each piece and joins the neighbors where their offset curves cross (a
//! miter). At a sharp outer corner the miter would be longer than `MITER_LIMIT` times the
//! distance, so a straight bevel joins the pieces there. Pieces that vanish on the inner side of
//! a tight corner are removed. The result of a large offset of a concave shape can cross itself:
//! this code does not remove such loops.

use std::f64::consts::{PI, TAU};

use glam::DVec2;

use crate::curve;
use crate::model::Shape;

use super::piece::{Chain, Piece, curve_intersections, piece_intersections};

/// A miter longer than this many times the distance becomes a bevel.
pub const MITER_LIMIT: f64 = 4.0;

/// Which side of the original the copy goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// To the left of the direction of travel. For a circle, towards the center.
    Left,
    Right,
    /// Away from the inside of a closed shape.
    Outside,
    Inside,
}

impl Side {
    pub fn from_name(s: &str) -> Option<Side> {
        match s {
            "left" => Some(Side::Left),
            "right" => Some(Side::Right),
            "outside" => Some(Side::Outside),
            "inside" => Some(Side::Inside),
            _ => None,
        }
    }
}

/// Twice the signed area of a closed chain (counterclockwise is positive).
pub fn signed_area(chain: &Chain) -> f64 {
    let mut pts = Vec::new();
    for p in &chain.pieces {
        let f = p.flatten(0.01);
        pts.extend_from_slice(&f[..f.len() - 1]);
    }
    let n = pts.len();
    (0..n)
        .map(|i| pts[i].perp_dot(pts[(i + 1) % n]))
        .sum::<f64>()
        / 2.0
}

fn set_end(p: &Piece, x: DVec2) -> Piece {
    match *p {
        Piece::Line { a, .. } => Piece::Line { a, b: x },
        Piece::Arc { c, r, start, sweep } => {
            let end = start + sweep;
            let delta = wrap_pi(((x - c).to_angle() - end) * sweep.signum());
            Piece::Arc {
                c,
                r,
                start,
                sweep: sweep + delta * sweep.signum(),
            }
        }
    }
}

fn set_start(p: &Piece, x: DVec2) -> Piece {
    match *p {
        Piece::Line { b, .. } => Piece::Line { a: x, b },
        Piece::Arc { c, r, start, sweep } => {
            let delta = wrap_pi(((x - c).to_angle() - start) * sweep.signum());
            Piece::Arc {
                c,
                r,
                start: start + delta * sweep.signum(),
                sweep: sweep - delta * sweep.signum(),
            }
        }
    }
}

fn wrap_pi(a: f64) -> f64 {
    let mut d = a.rem_euclid(TAU);
    if d > PI {
        d -= TAU;
    }
    d
}

/// True if `off` still runs the way `orig` runs and has a length.
fn alive(orig: &Piece, off: &Piece) -> bool {
    match (orig, off) {
        (Piece::Line { a, b }, Piece::Line { a: a2, b: b2 }) => {
            (b2 - a2).dot(b - a) > 1e-12 * a.distance(*b).max(1.0)
        }
        (Piece::Arc { sweep: s, .. }, Piece::Arc { sweep: s2, .. }) => {
            s.signum() == s2.signum() && s2.abs() > 1e-12 && s2.abs() <= TAU + 1e-9
        }
        _ => false,
    }
}

/// The chain moved sideways by `d`: to the left of its direction if `d` is above zero, to the
/// right if below. See the module notes for how the corners are joined.
pub fn offset_chain(chain: &Chain, d: f64) -> Result<Chain, String> {
    offset_chain_with(chain, d, false)
}

/// Like `offset_chain`, but keeps every piece, even the ones that turn round at a tight corner.
/// The result can cross itself. `clean_closed` takes the loops out.
fn offset_chain_raw(chain: &Chain, d: f64) -> Result<Chain, String> {
    offset_chain_with(chain, d, true)
}

fn offset_chain_with(chain: &Chain, d: f64, raw: bool) -> Result<Chain, String> {
    if !d.is_finite() {
        return Err("the distance must be a finite number".into());
    }
    if d == 0.0 {
        return Ok(chain.clone());
    }
    let mut items: Vec<(Piece, Piece)> = Vec::new();
    for p in &chain.pieces {
        match p.offset(d) {
            Some(o) => items.push((*p, o)),
            // An arc that shrinks to nothing: the raw offset leaves it out.
            None if raw => {}
            None => {
                return Err("the distance is larger than the radius of an arc".to_string());
            }
        }
    }
    for _ in 0..16 {
        let m = items.len();
        if m == 0 {
            return Err("the offset leaves nothing of the shape".into());
        }
        let mut offs: Vec<Piece> = items.iter().map(|i| i.1).collect();
        let mut bevels: Vec<Option<Piece>> = vec![None; m];
        let joints = if chain.closed { m } else { m - 1 };
        for j in 0..joints {
            let k = (j + 1) % m;
            if j == k {
                continue;
            }
            let corner = items[j].0.to();
            let (a, b) = (offs[j], offs[k]);
            let scale = d.abs().max(1.0);
            if a.to().distance(b.from()) <= 1e-9 * scale {
                continue;
            }
            let best = curve_intersections(&a, &b)
                .into_iter()
                .min_by(|p, q| p.distance(corner).total_cmp(&q.distance(corner)));
            match best {
                // An inner corner: the offset pieces overlap, and they are always cut at their
                // crossing. At an outer corner they are extended, up to the miter limit.
                Some(x)
                    if (a.param_of(x) <= 1.0 + 1e-9 && b.param_of(x) >= -1e-9)
                        || x.distance(corner) <= MITER_LIMIT * d.abs() + 1e-9 =>
                {
                    offs[j] = set_end(&a, x);
                    offs[k] = set_start(&b, x);
                }
                _ => bevels[j] = Some(Piece::line(a.to(), b.from())),
            }
        }
        let bad: Vec<usize> = if raw {
            Vec::new()
        } else {
            (0..m).filter(|&i| !alive(&items[i].0, &offs[i])).collect()
        };
        if bad.is_empty() {
            let mut pieces = Vec::new();
            for i in 0..m {
                pieces.push(offs[i]);
                if let Some(b) = bevels[i]
                    && b.length() > 1e-12
                {
                    pieces.push(b);
                }
            }
            return Ok(Chain {
                pieces,
                closed: chain.closed,
            });
        }
        let mut idx = 0;
        items.retain(|_| {
            idx += 1;
            !bad.contains(&(idx - 1))
        });
    }
    Err("the offset leaves nothing of the shape".into())
}

/// The largest piece of `offset_shapes`.
pub fn offset_shape(shape: &Shape, distance: f64, side: Side) -> Result<Shape, String> {
    offset_shapes(shape, distance, side).map(|mut v| v.remove(0))
}

/// Chord tolerance when a self-crossing offset is cleaned (curves become short lines).
const CLEAN_TOLERANCE: f64 = 0.05;

/// True if two pieces that are not neighbors cross, or a neighbor pair overlaps more than at
/// the shared end.
fn crosses_itself(chain: &Chain) -> bool {
    let n = chain.pieces.len();
    for i in 0..n {
        for j in i + 2..n {
            if chain.closed && i == 0 && j == n - 1 {
                continue;
            }
            if !piece_intersections(&chain.pieces[i], &chain.pieces[j]).is_empty() {
                return true;
            }
        }
    }
    false
}

/// Cleans the offset of a closed chain: the loops with the wrong winding and the crossing
/// pieces go (a union of the parts that wind the same way as the source). The pieces that are
/// left are returned, the largest first. Empty if the offset collapses the shape.
fn clean_closed(raw: &Chain, source_ccw: bool) -> Vec<Vec<DVec2>> {
    use i_overlay::core::fill_rule::FillRule;
    use i_overlay::core::overlay_rule::OverlayRule;
    use i_overlay::float::single::SingleFloatOverlay;
    let mut ring: Vec<DVec2> = Vec::new();
    for p in &raw.pieces {
        let f = p.flatten(CLEAN_TOLERANCE);
        ring.extend_from_slice(&f[..f.len() - 1]);
    }
    if !source_ccw {
        ring.reverse();
    }
    let contour: Vec<[f64; 2]> = ring.iter().map(|p| [p.x, p.y]).collect();
    let shapes = contour.overlay(
        &Vec::<Vec<[f64; 2]>>::new(),
        OverlayRule::Union,
        FillRule::Positive,
    );
    shapes_to_rings(shapes)
}

/// An inward offset that has to be cleaned: the points that are farther than `d` from the
/// outline, as the shape minus a band of width `2 d` along its outline (a rectangle along each
/// edge, a 32-sided polygon around each corner). Corners that point into the shape come out
/// rounded. The band is a little wider than `d` at the corners, never narrower.
fn erode(chain: &Chain, d: f64, source_ccw: bool) -> Vec<Vec<DVec2>> {
    use i_overlay::core::fill_rule::FillRule;
    use i_overlay::core::overlay_rule::OverlayRule;
    use i_overlay::float::single::SingleFloatOverlay;
    let mut ring: Vec<DVec2> = Vec::new();
    for p in &chain.pieces {
        let f = p.flatten(CLEAN_TOLERANCE);
        ring.extend_from_slice(&f[..f.len() - 1]);
    }
    if !source_ccw {
        ring.reverse();
    }
    let n = ring.len();
    let ccw = |mut r: Vec<DVec2>| -> Vec<[f64; 2]> {
        if super::boolean::ring_area2(&r) < 0.0 {
            r.reverse();
        }
        r.into_iter().map(|p| [p.x, p.y]).collect()
    };
    let mut band: Vec<Vec<[f64; 2]>> = Vec::new();
    const SIDES: usize = 32;
    let corner_r = d / (PI / SIDES as f64).cos();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if let Some(u) = (b - a).try_normalize() {
            let nrm = DVec2::new(-u.y, u.x) * d;
            band.push(ccw(vec![a - nrm, b - nrm, b + nrm, a + nrm]));
        }
        band.push(ccw((0..SIDES)
            .map(|k| a + DVec2::from_angle(TAU * k as f64 / SIDES as f64) * corner_r)
            .collect()));
    }
    let subject: Vec<[f64; 2]> = ring.iter().map(|p| [p.x, p.y]).collect();
    let shapes = subject.overlay(&band, OverlayRule::Difference, FillRule::NonZero);
    shapes_to_rings(shapes)
}

fn shapes_to_rings(shapes: Vec<Vec<Vec<[f64; 2]>>>) -> Vec<Vec<DVec2>> {
    let mut polys: Vec<Vec<Vec<DVec2>>> = shapes
        .into_iter()
        .map(|s| {
            s.into_iter()
                .map(|c| c.into_iter().map(|p| DVec2::new(p[0], p[1])).collect())
                .collect()
        })
        .collect();
    let area = |p: &Vec<Vec<DVec2>>| -> f64 {
        p.iter()
            .enumerate()
            .map(|(i, r)| {
                let a = super::boolean::ring_area2(r).abs() / 2.0;
                if i == 0 { a } else { -a }
            })
            .sum()
    };
    polys.retain(|p| area(p) > 1e-6);
    polys.sort_by(|a, b| area(b).total_cmp(&area(a)));
    polys
        .into_iter()
        .map(|p| super::boolean::poly_ring(&p))
        .collect()
}

/// The shapes moved by `distance` (above zero) to `side`. Circles and rectangles stay what they
/// are. A closed shape whose offset crosses itself (an inward offset of a narrow part, or of a
/// small shape) is cleaned: what is left is one or more closed polylines, the largest first. If
/// nothing is left, the error says that the offset collapses the shape.
pub fn offset_shapes(shape: &Shape, distance: f64, side: Side) -> Result<Vec<Shape>, String> {
    offset_shapes_inner(shape, distance, side)
}

fn offset_shapes_inner(shape: &Shape, distance: f64, side: Side) -> Result<Vec<Shape>, String> {
    if !distance.is_finite() || distance <= 0.0 {
        return Err("the offset distance must be above zero".into());
    }
    let grow = |side: Side, what: &str| -> Result<f64, String> {
        match side {
            Side::Outside | Side::Right => Ok(distance),
            Side::Inside | Side::Left => Ok(-distance),
        }
        .map_err(|e: String| format!("{what}: {e}"))
    };
    match shape {
        Shape::Circle { center, radius } => {
            let r = radius + grow(side, "circle")?;
            if r <= 0.0 {
                return Err("offset collapses the shape: it is larger than the radius".into());
            }
            Ok(vec![Shape::Circle {
                center: *center,
                radius: r,
            }])
        }
        Shape::Rect { min, max } => {
            let g = grow(side, "rect")?;
            let (lo, hi) = (
                min.min(*max) - DVec2::splat(g),
                min.max(*max) + DVec2::splat(g),
            );
            if lo.x >= hi.x || lo.y >= hi.y {
                return Err("offset collapses the shape: it is larger than the rectangle".into());
            }
            Ok(vec![Shape::Rect { min: lo, max: hi }])
        }
        _ => {
            let chain = Chain::from_shape(shape)
                .ok_or_else(|| format!("a {} cannot be offset", shape.kind()))?;
            let left = match side {
                Side::Left => true,
                Side::Right => false,
                Side::Outside | Side::Inside => {
                    if !chain.closed {
                        return Err(
                            "outside and inside need a closed shape. Use left or right".into()
                        );
                    }
                    // The left of a counterclockwise shape is its inside.
                    let ccw = signed_area(&chain) > 0.0;
                    (side == Side::Inside) == ccw
                }
            };
            let signed = if left { distance } else { -distance };
            let collapse = |e: String| format!("offset collapses the shape: {e}");
            let exact = offset_chain(&chain, signed);
            if !chain.closed {
                return Ok(vec![exact.map_err(collapse)?.to_shape()]);
            }
            let source_area = signed_area(&chain);
            // The exact offset is used when it is a proper closed curve. Else the raw offset is
            // cleaned.
            if let Ok(off) = &exact
                && off.closed
                && signed_area(off) * source_area > 0.0
                && !crosses_itself(off)
            {
                return Ok(vec![off.to_shape()]);
            }
            let inward = left == (source_area > 0.0);
            let rings = if inward {
                erode(&chain, distance, source_area > 0.0)
            } else {
                let off = offset_chain_raw(&chain, signed).map_err(collapse)?;
                clean_closed(&off, source_area > 0.0)
            };
            if rings.is_empty() {
                return Err("offset collapses the shape".into());
            }
            Ok(rings
                .into_iter()
                .map(|r| Shape::Polyline {
                    points: curve::corners(&r),
                    closed: true,
                })
                .collect())
        }
    }
}

/// The distance and side that put a copy of `shape` through the point `through`.
pub fn offset_through(shape: &Shape, through: DVec2) -> Result<(f64, Side), String> {
    match shape {
        Shape::Circle { center, radius } => {
            let d = through.distance(*center) - radius;
            Ok((
                d.abs(),
                if d >= 0.0 {
                    Side::Outside
                } else {
                    Side::Inside
                },
            ))
        }
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            let c = (lo + hi) / 2.0;
            let q = (through - c).abs() - (hi - lo) / 2.0;
            // Rectangle offsets keep the shape of the rectangle: the distance is the largest
            // step outwards, or inwards.
            let outside = q.x > 0.0 || q.y > 0.0;
            let d = if outside {
                q.max_element()
            } else {
                -q.max_element()
            };
            Ok((d, if outside { Side::Outside } else { Side::Inside }))
        }
        _ => {
            let chain = Chain::from_shape(shape)
                .ok_or_else(|| format!("a {} cannot be offset", shape.kind()))?;
            let (i, t, q, d) = chain.closest(through);
            let tan = chain.pieces[i].tangent(t);
            let left = tan.perp_dot(through - q) > 0.0;
            Ok((d, if left { Side::Left } else { Side::Right }))
        }
    }
}

/// Flattened corners of a closed shape (used to measure the area of offsets).
pub fn outline_points(shape: &Shape) -> Option<Vec<DVec2>> {
    match shape {
        Shape::Polyline {
            points,
            closed: true,
        } => Some(curve::flatten(points, true, 0.001)),
        Shape::Rect { min, max } => Some(vec![
            *min,
            DVec2::new(max.x, min.y),
            *max,
            DVec2::new(min.x, max.y),
        ]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::{Vertex, VertexKind, corners};
    use crate::geom::polygon_area;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn poly(pts: &[DVec2], closed: bool) -> Shape {
        Shape::Polyline {
            points: corners(pts),
            closed,
        }
    }

    #[test]
    fn a_line_moves_sideways() {
        let l = Shape::Line {
            a: v(0.0, 0.0),
            b: v(10.0, 0.0),
        };
        assert_eq!(
            offset_shape(&l, 2.0, Side::Left).unwrap(),
            Shape::Line {
                a: v(0.0, 2.0),
                b: v(10.0, 2.0)
            }
        );
        assert_eq!(
            offset_shape(&l, 2.0, Side::Right).unwrap(),
            Shape::Line {
                a: v(0.0, -2.0),
                b: v(10.0, -2.0)
            }
        );
        assert!(offset_shape(&l, 2.0, Side::Outside).is_err());
        assert!(offset_shape(&l, 0.0, Side::Left).is_err());
        assert!(offset_shape(&l, f64::NAN, Side::Left).is_err());
    }

    #[test]
    fn circles_and_rectangles_stay_exact() {
        let c = Shape::Circle {
            center: v(1.0, 2.0),
            radius: 5.0,
        };
        assert_eq!(
            offset_shape(&c, 2.0, Side::Outside).unwrap(),
            Shape::Circle {
                center: v(1.0, 2.0),
                radius: 7.0
            }
        );
        assert!(offset_shape(&c, 6.0, Side::Inside).is_err());
        let r = Shape::Rect {
            min: v(0.0, 0.0),
            max: v(10.0, 4.0),
        };
        assert_eq!(
            offset_shape(&r, 1.0, Side::Outside).unwrap(),
            Shape::Rect {
                min: v(-1.0, -1.0),
                max: v(11.0, 5.0)
            }
        );
        assert!(offset_shape(&r, 2.0, Side::Inside).is_err());
    }

    #[test]
    fn a_polygon_offsets_to_a_miter() {
        let sq = poly(
            &[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)],
            true,
        );
        let out = offset_shape(&sq, 1.0, Side::Outside).unwrap();
        let a = polygon_area(&outline_points(&out).unwrap());
        assert!((a - 144.0).abs() < 1e-9, "{a}");
        let inn = offset_shape(&sq, 1.0, Side::Inside).unwrap();
        assert!((polygon_area(&outline_points(&inn).unwrap()) - 64.0).abs() < 1e-9);
        // The same square in the other direction gives the same outside.
        let cw = poly(
            &[v(0.0, 10.0), v(10.0, 10.0), v(10.0, 0.0), v(0.0, 0.0)],
            true,
        );
        let out = offset_shape(&cw, 1.0, Side::Outside).unwrap();
        assert!((polygon_area(&outline_points(&out).unwrap()) - 144.0).abs() < 1e-9);
        // Too much inside: the shape vanishes.
        assert!(offset_shape(&sq, 6.0, Side::Inside).is_err());
    }

    #[test]
    fn sharp_corners_get_a_bevel() {
        // A thin spike: the miter would be very long.
        let spike = poly(&[v(0.0, 0.0), v(10.0, 0.5), v(0.0, 1.0)], false);
        let out = offset_shape(&spike, 1.0, Side::Right).unwrap();
        let Shape::Polyline { points, .. } = out else {
            panic!()
        };
        assert!(points.len() >= 4, "{points:?}");
        assert!(points.iter().all(|p| p.p.x < 12.0));
    }

    #[test]
    fn an_open_polyline_with_an_arc() {
        let p = Shape::Polyline {
            points: vec![
                Vertex::corner(v(0.0, 0.0)),
                Vertex::new(v(10.0, 0.0), VertexKind::Arc { radius: 3.0 }),
                Vertex::corner(v(10.0, 10.0)),
            ],
            closed: false,
        };
        let chain = Chain::from_shape(&p).unwrap();
        let l = offset_chain(&chain, 1.0).unwrap();
        // Left of a left turn is the inside: the arc radius shrinks by 1.
        let arcs: Vec<f64> = l
            .pieces
            .iter()
            .filter_map(|p| p.circle().map(|c| c.1))
            .collect();
        assert_eq!(arcs.len(), 1);
        assert!((arcs[0] - 2.0).abs() < 1e-9);
        // The offset is continuous.
        for w in l.pieces.windows(2) {
            assert!(w[0].to().distance(w[1].from()) < 1e-9);
        }
        let rr = offset_chain(&chain, -1.0).unwrap();
        let arcs: Vec<f64> = rr
            .pieces
            .iter()
            .filter_map(|p| p.circle().map(|c| c.1))
            .collect();
        assert!((arcs[0] - 4.0).abs() < 1e-9);
        // An arc that would shrink to nothing.
        assert!(offset_chain(&chain, 3.5).is_err());
    }

    #[test]
    fn through_a_point() {
        let c = Shape::Circle {
            center: v(0.0, 0.0),
            radius: 5.0,
        };
        assert_eq!(
            offset_through(&c, v(8.0, 0.0)).unwrap(),
            (3.0, Side::Outside)
        );
        let l = Shape::Line {
            a: v(0.0, 0.0),
            b: v(10.0, 0.0),
        };
        assert_eq!(
            offset_through(&l, v(4.0, -3.0)).unwrap(),
            (3.0, Side::Right)
        );
        assert_eq!(offset_through(&l, v(4.0, 3.0)).unwrap(), (3.0, Side::Left));
        let r = Shape::Rect {
            min: v(0.0, 0.0),
            max: v(10.0, 10.0),
        };
        assert_eq!(
            offset_through(&r, v(13.0, 5.0)).unwrap(),
            (3.0, Side::Outside)
        );
        assert_eq!(
            offset_through(&r, v(2.0, 5.0)).unwrap(),
            (2.0, Side::Inside)
        );
        assert!(
            offset_through(
                &Shape::Text {
                    at: v(0.0, 0.0),
                    text: "x".into(),
                    height: 1.0
                },
                v(1.0, 1.0)
            )
            .is_err()
        );
    }

    #[test]
    fn side_names() {
        assert_eq!(Side::from_name("inside"), Some(Side::Inside));
        assert_eq!(Side::from_name("up"), None);
        assert_eq!(
            outline_points(&Shape::Circle {
                center: v(0.0, 0.0),
                radius: 1.0
            }),
            None
        );
    }

    fn u_shape() -> Shape {
        poly(
            &[
                v(0.0, 0.0),
                v(3000.0, 0.0),
                v(3000.0, 2000.0),
                v(2200.0, 2000.0),
                v(2200.0, 600.0),
                v(800.0, 600.0),
                v(800.0, 2000.0),
                v(0.0, 2000.0),
            ],
            true,
        )
    }

    fn shoelace(s: &Shape) -> f64 {
        s.area().unwrap()
    }

    #[test]
    fn an_inward_offset_of_a_narrow_part_is_cleaned() {
        let u = u_shape();
        let src = shoelace(&u);
        // 250 in: the arms are 800 wide and stay 300 wide; the base keeps 100.
        let r = offset_shapes(&u, 250.0, Side::Inside).unwrap();
        assert_eq!(r.len(), 1);
        let a: f64 = r.iter().map(shoelace).sum();
        assert!(a < src && a > 0.0, "{a} {src}");
        let Shape::Polyline { points, .. } = &r[0] else {
            panic!()
        };
        assert!(points.len() >= 8);
        // 600 in: the arms are gone and so is the base. A clear error, not a loop.
        let e = offset_shapes(&u, 600.0, Side::Inside).unwrap_err();
        assert!(e.contains("collapses"), "{e}");
        // 5000 in: the offset would be a larger reversed polygon.
        let e = offset_shapes(&u, 5000.0, Side::Inside).unwrap_err();
        assert!(e.contains("collapses"), "{e}");
        let sq = poly(
            &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(0.0, 100.0)],
            true,
        );
        assert!(
            offset_shapes(&sq, 70.0, Side::Inside)
                .unwrap_err()
                .contains("collapses")
        );
        // The same square, wound the other way.
        let cw = poly(
            &[v(0.0, 100.0), v(100.0, 100.0), v(100.0, 0.0), v(0.0, 0.0)],
            true,
        );
        assert!(
            offset_shapes(&cw, 70.0, Side::Inside)
                .unwrap_err()
                .contains("collapses")
        );
        assert!((shoelace(&offset_shape(&cw, 10.0, Side::Inside).unwrap()) - 6400.0).abs() < 1e-6);
    }

    #[test]
    fn an_outward_offset_closes_a_narrow_notch() {
        let u = u_shape();
        // The slot between the arms is 1400 wide; 800 out closes it, and leaves a hole at most.
        let r = offset_shapes(&u, 800.0, Side::Outside).unwrap();
        assert!(!r.is_empty());
        let a: f64 = r.iter().map(shoelace).sum();
        assert!(a > shoelace(&u));
    }
}
