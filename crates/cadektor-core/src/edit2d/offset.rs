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

use super::piece::{Chain, Piece, curve_intersections};

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
    (0..n).map(|i| pts[i].perp_dot(pts[(i + 1) % n])).sum::<f64>() / 2.0
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
    if !d.is_finite() {
        return Err("the distance must be a finite number".into());
    }
    if d == 0.0 {
        return Ok(chain.clone());
    }
    let mut items: Vec<(Piece, Piece)> = Vec::new();
    for p in &chain.pieces {
        let o = p
            .offset(d)
            .ok_or_else(|| "the distance is larger than the radius of an arc".to_string())?;
        items.push((*p, o));
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
                Some(x) if x.distance(corner) <= MITER_LIMIT * d.abs() + 1e-9 => {
                    offs[j] = set_end(&a, x);
                    offs[k] = set_start(&b, x);
                }
                _ => bevels[j] = Some(Piece::line(a.to(), b.from())),
            }
        }
        let bad: Vec<usize> = (0..m).filter(|&i| !alive(&items[i].0, &offs[i])).collect();
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

/// The shape moved by `distance` (above zero) to `side`. Circles and rectangles stay what they are.
pub fn offset_shape(shape: &Shape, distance: f64, side: Side) -> Result<Shape, String> {
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
                return Err("the offset is larger than the radius".into());
            }
            Ok(Shape::Circle {
                center: *center,
                radius: r,
            })
        }
        Shape::Rect { min, max } => {
            let g = grow(side, "rect")?;
            let (lo, hi) = (min.min(*max) - DVec2::splat(g), min.max(*max) + DVec2::splat(g));
            if lo.x >= hi.x || lo.y >= hi.y {
                return Err("the offset is larger than the rectangle".into());
            }
            Ok(Shape::Rect { min: lo, max: hi })
        }
        _ => {
            let chain = Chain::from_shape(shape)
                .ok_or_else(|| format!("a {} cannot be offset", shape.kind()))?;
            let left = match side {
                Side::Left => true,
                Side::Right => false,
                Side::Outside | Side::Inside => {
                    if !chain.closed {
                        return Err("outside and inside need a closed shape. Use left or right".into());
                    }
                    // The left of a counterclockwise shape is its inside.
                    let ccw = signed_area(&chain) > 0.0;
                    (side == Side::Inside) == ccw
                }
            };
            let off = offset_chain(&chain, if left { distance } else { -distance })?;
            Ok(off.to_shape())
        }
    }
}

/// The distance and side that put a copy of `shape` through the point `through`.
pub fn offset_through(shape: &Shape, through: DVec2) -> Result<(f64, Side), String> {
    match shape {
        Shape::Circle { center, radius } => {
            let d = through.distance(*center) - radius;
            Ok((d.abs(), if d >= 0.0 { Side::Outside } else { Side::Inside }))
        }
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            let c = (lo + hi) / 2.0;
            let q = (through - c).abs() - (hi - lo) / 2.0;
            // Rectangle offsets keep the shape of the rectangle: the distance is the largest
            // step outwards, or inwards.
            let outside = q.x > 0.0 || q.y > 0.0;
            let d = if outside { q.max_element() } else { -q.max_element() };
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

/// The corners of a polygon after an offset, for tests: the polygon area function used by callers.
pub fn polygon_area(points: &[DVec2]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| points[i].perp_dot(points[(i + 1) % n]))
        .sum::<f64>()
        .abs()
        / 2.0
}

/// Flattened corners of a closed shape (used to measure the area of offsets).
pub fn outline_points(shape: &Shape) -> Option<Vec<DVec2>> {
    match shape {
        Shape::Polyline { points, closed: true } => Some(curve::flatten(points, true, 0.001)),
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
        let sq = poly(&[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)], true);
        let out = offset_shape(&sq, 1.0, Side::Outside).unwrap();
        let a = polygon_area(&outline_points(&out).unwrap());
        assert!((a - 144.0).abs() < 1e-9, "{a}");
        let inn = offset_shape(&sq, 1.0, Side::Inside).unwrap();
        assert!((polygon_area(&outline_points(&inn).unwrap()) - 64.0).abs() < 1e-9);
        // The same square in the other direction gives the same outside.
        let cw = poly(&[v(0.0, 10.0), v(10.0, 10.0), v(10.0, 0.0), v(0.0, 0.0)], true);
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
        assert_eq!(offset_through(&c, v(8.0, 0.0)).unwrap(), (3.0, Side::Outside));
        let l = Shape::Line {
            a: v(0.0, 0.0),
            b: v(10.0, 0.0),
        };
        assert_eq!(offset_through(&l, v(4.0, -3.0)).unwrap(), (3.0, Side::Right));
        assert_eq!(offset_through(&l, v(4.0, 3.0)).unwrap(), (3.0, Side::Left));
        let r = Shape::Rect {
            min: v(0.0, 0.0),
            max: v(10.0, 10.0),
        };
        assert_eq!(offset_through(&r, v(13.0, 5.0)).unwrap(), (3.0, Side::Outside));
        assert_eq!(offset_through(&r, v(2.0, 5.0)).unwrap(), (2.0, Side::Inside));
        assert!(offset_through(&Shape::Text { at: v(0.0, 0.0), text: "x".into(), height: 1.0 }, v(1.0, 1.0)).is_err());
    }

    #[test]
    fn side_names() {
        assert_eq!(Side::from_name("inside"), Some(Side::Inside));
        assert_eq!(Side::from_name("up"), None);
        assert_eq!(outline_points(&Shape::Circle { center: v(0.0, 0.0), radius: 1.0 }), None);
    }
}
