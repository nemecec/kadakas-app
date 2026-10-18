//! Plane transforms of shapes: scale, mirror and any mix of rotate, scale, mirror and move.
//!
//! A transform is a 2x2 matrix and a move (`Xf`). `map_shape` applies it to a shape. What a shape
//! allows depends on the kind of transform:
//! - Moving and turning (a similarity without scale) work for every shape that `rotate_shape` turns.
//! - A uniform scale or a mirror keeps circles and arcs round.
//! - A non-uniform scale needs shapes that stay what they are: lines, rectangles, polylines, points.
//!   A circle or an arc becomes no ellipse, so it is refused. Polylines with fillet vertices or
//!   splines are flattened to corners at `FLATTEN_TOLERANCE`.

use std::f64::consts::TAU;

use glam::{DMat2, DVec2, DVec3};

use crate::curve::{self, Vertex, VertexKind};
use crate::model::{Shape, arc_sweep};

/// Chord tolerance in mm for curves that have to become corners.
pub const FLATTEN_TOLERANCE: f64 = 0.05;

/// A plane transform: `p -> m * p + t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub m: DMat2,
    pub t: DVec2,
}

impl Xf {
    pub const IDENTITY: Xf = Xf {
        m: DMat2::IDENTITY,
        t: DVec2::ZERO,
    };

    pub fn translate(d: DVec2) -> Xf {
        Xf {
            m: DMat2::IDENTITY,
            t: d,
        }
    }

    /// Turns about `c` by `angle` radians, counterclockwise.
    pub fn rotate(c: DVec2, angle: f64) -> Xf {
        let m = DMat2::from_angle(angle);
        Xf { m, t: c - m * c }
    }

    /// Scales about `c` by `sx` in x and `sy` in y. A negative factor mirrors that axis.
    pub fn scale(c: DVec2, sx: f64, sy: f64) -> Xf {
        let m = DMat2::from_diagonal(DVec2::new(sx, sy));
        Xf { m, t: c - m * c }
    }

    /// Mirrors on the line through `a` and `b`. `None` if the points are the same.
    pub fn mirror(a: DVec2, b: DVec2) -> Option<Xf> {
        let d = (b - a).try_normalize()?;
        // Reflection matrix about the direction d: 2 d d^T - I.
        let m = DMat2::from_cols(
            DVec2::new(2.0 * d.x * d.x - 1.0, 2.0 * d.x * d.y),
            DVec2::new(2.0 * d.x * d.y, 2.0 * d.y * d.y - 1.0),
        );
        Some(Xf { m, t: a - m * a })
    }

    /// First `self`, then `next`.
    pub fn then(self, next: Xf) -> Xf {
        Xf {
            m: next.m * self.m,
            t: next.m * self.t + next.t,
        }
    }

    pub fn apply(&self, p: DVec2) -> DVec2 {
        self.m * p + self.t
    }

    pub fn is_finite(&self) -> bool {
        self.m.is_finite() && self.t.is_finite()
    }

    pub fn det(&self) -> f64 {
        self.m.determinant()
    }

    pub fn is_reflection(&self) -> bool {
        self.det() < 0.0
    }

    /// The uniform scale factor if the transform keeps shapes round: a turn or a mirror, with
    /// a scale and a move. `None` for a non-uniform scale or a shear.
    pub fn similarity(&self) -> Option<f64> {
        let (x, y) = (self.m.x_axis, self.m.y_axis);
        let (lx, ly) = (x.length(), y.length());
        let tol = 1e-9 * lx.max(ly).max(1.0);
        ((lx - ly).abs() <= tol && x.dot(y).abs() <= tol * lx.max(1.0) && lx > 0.0).then_some(lx)
    }

    /// The turn angle of a similarity (for a mirror: the angle in `rotate(phi) * mirror_x`).
    pub fn angle(&self) -> f64 {
        self.m.x_axis.y.atan2(self.m.x_axis.x)
    }

    /// True if the transform only moves.
    pub fn is_translation(&self) -> bool {
        (self.m - DMat2::IDENTITY)
            .to_cols_array()
            .iter()
            .all(|v| v.abs() <= 1e-12)
    }

    /// True if the transform only turns (no scale, no mirror).
    pub fn is_rotation(&self) -> bool {
        self.similarity().is_some_and(|s| (s - 1.0).abs() <= 1e-12) && !self.is_reflection()
    }

    /// The center and angle if the transform is a pure turn (not a move, no scale, no mirror).
    pub fn as_rotation(&self) -> Option<(DVec2, f64)> {
        if !self.is_rotation() || self.is_translation() {
            return None;
        }
        let d = DMat2::IDENTITY - self.m;
        (d.determinant().abs() > 1e-12).then(|| (d.inverse() * self.t, self.angle()))
    }

    /// True if x axis stays x and y stays y (maybe swapped): rectangles stay rectangles.
    fn keeps_axes(&self) -> bool {
        let e = 1e-12;
        (self.m.x_axis.y.abs() <= e && self.m.y_axis.x.abs() <= e)
            || (self.m.x_axis.x.abs() <= e && self.m.y_axis.y.abs() <= e)
    }
}

fn map_vertices(v: &[Vertex], closed: bool, xf: &Xf) -> Result<Vec<Vertex>, String> {
    if let Some(s) = xf.similarity() {
        return Ok(v
            .iter()
            .map(|x| Vertex {
                p: xf.apply(x.p),
                kind: match x.kind {
                    VertexKind::Arc { radius } => VertexKind::Arc { radius: radius * s },
                    k => k,
                },
            })
            .collect());
    }
    if curve::all_straight(v) {
        return Ok(v.iter().map(|x| Vertex::corner(xf.apply(x.p))).collect());
    }
    // A fillet or spline does not survive a stretch. Use the corners of the flattened curve.
    let pts = curve::flatten(v, closed, FLATTEN_TOLERANCE);
    Ok(pts
        .into_iter()
        .map(|p| Vertex::corner(xf.apply(p)))
        .collect())
}

fn rect_corners(min: DVec2, max: DVec2) -> [DVec2; 4] {
    [min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)]
}

/// The kinds of shapes that `map_shape` can transform, for error messages.
pub const SUPPORTED: &str = "line, rect, circle, arc, polyline, text, plant, existing_tree, plant_tag, locus3d, polyline3d, extrude, sweep, symbol, group";

/// `shape` after the transform. Refuses shapes that the transform cannot keep (see the module notes).
pub fn map_shape(shape: &Shape, xf: &Xf) -> Result<Shape, String> {
    if !xf.is_finite() {
        return Err("numbers must be finite".into());
    }
    let sim = xf.similarity();
    let need_sim =
        |what: &str| sim.ok_or_else(|| format!("a {what} needs the same scale in x and y"));
    Ok(match shape {
        Shape::Line { a, b } => Shape::Line {
            a: xf.apply(*a),
            b: xf.apply(*b),
        },
        Shape::Rect { min, max } => {
            let c = rect_corners(min.min(*max), min.max(*max)).map(|p| xf.apply(p));
            if xf.keeps_axes() {
                let lo = c.iter().fold(DVec2::splat(f64::INFINITY), |a, p| a.min(*p));
                let hi = c
                    .iter()
                    .fold(DVec2::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
                Shape::Rect { min: lo, max: hi }
            } else {
                let mut pts: Vec<Vertex> = c.into_iter().map(Vertex::corner).collect();
                if xf.is_reflection() {
                    pts.reverse();
                }
                Shape::Polyline {
                    points: pts,
                    closed: true,
                }
            }
        }
        Shape::Circle { center, radius } => {
            let s = need_sim("circle")?;
            Shape::Circle {
                center: xf.apply(*center),
                radius: radius * s,
            }
        }
        Shape::Arc {
            center,
            radius,
            start,
            end,
        } => {
            let s = need_sim("arc")?;
            let phi = xf.angle();
            let sweep = arc_sweep(*start, *end);
            let full = sweep >= TAU;
            // The sweep stays, whatever the wrap of the old angles.
            let (ns, ne) = if xf.is_reflection() {
                let ns = phi - (start + sweep);
                (ns, ns + if full { TAU } else { sweep })
            } else {
                let ns = start + phi;
                (ns, ns + if full { TAU } else { sweep })
            };
            Shape::Arc {
                center: xf.apply(*center),
                radius: radius * s,
                start: ns,
                end: ne,
            }
        }
        Shape::Polyline { points, closed } => Shape::Polyline {
            points: map_vertices(points, *closed, xf)?,
            closed: *closed,
        },
        Shape::Text { at, text, height } => Shape::Text {
            at: xf.apply(*at),
            text: text.clone(),
            height: height * sim.unwrap_or_else(|| xf.det().abs().sqrt().max(1e-9)),
        },
        Shape::Plant {
            at,
            species,
            spread,
            height,
        } => Shape::Plant {
            at: xf.apply(*at),
            species: species.clone(),
            spread: spread * sim.unwrap_or_else(|| xf.det().abs().sqrt().max(1e-9)),
            height: *height,
        },
        Shape::ExistingTree {
            at,
            species,
            dbh,
            spread,
            height,
            condition,
            action,
            rpz_factor,
            rpz_cap,
        } => Shape::ExistingTree {
            at: xf.apply(*at),
            species: species.clone(),
            dbh: *dbh,
            spread: spread * sim.unwrap_or_else(|| xf.det().abs().sqrt().max(1e-9)),
            height: *height,
            condition: *condition,
            action: *action,
            rpz_factor: *rpz_factor,
            rpz_cap: *rpz_cap,
        },
        Shape::PlantTag {
            at,
            anchor,
            style,
            height,
        } => Shape::PlantTag {
            at: xf.apply(*at),
            anchor: xf.apply(*anchor),
            style: *style,
            height: *height,
        },
        Shape::Locus3d { p } => {
            let m = xf.apply(p.truncate());
            Shape::Locus3d {
                p: DVec3::new(m.x, m.y, p.z),
            }
        }
        Shape::Polyline3d { points, closed } => Shape::Polyline3d {
            points: points
                .iter()
                .map(|q| {
                    let m = xf.apply(q.truncate());
                    DVec3::new(m.x, m.y, q.z)
                })
                .collect(),
            closed: *closed,
        },
        Shape::Symbol {
            symbol,
            at,
            rotation,
            scale,
            scale_y,
        } => {
            let s = need_sim("symbol")?;
            let phi = xf.angle();
            let sy = scale_y.unwrap_or(*scale);
            if xf.is_reflection() {
                Shape::Symbol {
                    symbol: *symbol,
                    at: xf.apply(*at),
                    rotation: phi - rotation,
                    scale: scale * s,
                    scale_y: Some(-sy * s),
                }
            } else {
                Shape::Symbol {
                    symbol: *symbol,
                    at: xf.apply(*at),
                    rotation: rotation + phi,
                    scale: scale * s,
                    scale_y: scale_y.map(|v| v * s),
                }
            }
        }
        Shape::Extrude(e) => {
            let mut e = (**e).clone();
            e.profile = map_vertices(&e.profile, true, xf)?;
            e.holes = e
                .holes
                .iter()
                .map(|h| map_vertices(h, true, xf))
                .collect::<Result<_, _>>()?;
            // A uniform scale scales the height too (as the scale of a 3D object does). A
            // stretch in the plan leaves the height as it is.
            if let Some(k) = sim {
                e.height *= k;
                e.base *= k;
            }
            Shape::Extrude(Box::new(e))
        }
        Shape::Sweep(s) => {
            // A sweep is a 3D solid: a move, a turn, a uniform scale and a mirror keep it a
            // sweep. A stretch in the plan does not.
            let k = need_sim("sweep")?;
            let mut s = (**s).clone();
            let origin = xf.apply(s.origin.truncate());
            s.remap_plan(origin, xf.m, k, xf.is_reflection());
            Shape::Sweep(Box::new(s))
        }
        Shape::Group { name } => Shape::Group { name: name.clone() },
        other => {
            return Err(format!(
                "a {} cannot be scaled or mirrored. These shapes can: {SUPPORTED}",
                other.kind()
            ));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn mirror_about_the_x_axis_and_a_diagonal() {
        let m = Xf::mirror(v(0.0, 0.0), v(1.0, 0.0)).unwrap();
        assert!(m.apply(v(3.0, 2.0)).distance(v(3.0, -2.0)) < 1e-12);
        let d = Xf::mirror(v(1.0, 1.0), v(2.0, 2.0)).unwrap();
        assert!(d.apply(v(3.0, 0.0)).distance(v(0.0, 3.0)) < 1e-12);
        assert!(Xf::mirror(v(1.0, 1.0), v(1.0, 1.0)).is_none());
        assert!(m.is_reflection());
        assert_eq!(m.similarity().map(|s| (s * 1e9).round()), Some(1e9));
    }

    #[test]
    fn composition_and_kinds() {
        let r = Xf::rotate(v(1.0, 1.0), 0.7);
        assert!(r.is_rotation());
        assert!(r.apply(v(1.0, 1.0)).distance(v(1.0, 1.0)) < 1e-12);
        let back = r.then(Xf::rotate(v(1.0, 1.0), -0.7));
        assert!(back.apply(v(5.0, -2.0)).distance(v(5.0, -2.0)) < 1e-9);
        assert!(Xf::translate(v(1.0, 2.0)).is_translation());
        assert!(Xf::scale(v(0.0, 0.0), 2.0, 3.0).similarity().is_none());
        assert_eq!(Xf::scale(v(0.0, 0.0), 2.0, 2.0).similarity(), Some(2.0));
    }

    #[test]
    fn rect_stays_rect_when_axes_are_kept() {
        let r = Shape::Rect {
            min: v(0.0, 0.0),
            max: v(2.0, 1.0),
        };
        let s = map_shape(&r, &Xf::scale(v(0.0, 0.0), 2.0, 3.0)).unwrap();
        assert_eq!(
            s,
            Shape::Rect {
                min: v(0.0, 0.0),
                max: v(4.0, 3.0)
            }
        );
        let t = map_shape(&r, &Xf::rotate(v(0.0, 0.0), 0.3)).unwrap();
        assert!(matches!(t, Shape::Polyline { closed: true, .. }));
    }

    #[test]
    fn circles_and_arcs_need_a_uniform_scale() {
        let c = Shape::Circle {
            center: v(1.0, 0.0),
            radius: 2.0,
        };
        assert!(map_shape(&c, &Xf::scale(v(0.0, 0.0), 2.0, 3.0)).is_err());
        let d = map_shape(&c, &Xf::scale(v(0.0, 0.0), 2.0, 2.0)).unwrap();
        assert_eq!(
            d,
            Shape::Circle {
                center: v(2.0, 0.0),
                radius: 4.0
            }
        );
    }

    #[test]
    fn arc_mirror_swaps_the_ends() {
        let a = Shape::Arc {
            center: v(0.0, 0.0),
            radius: 1.0,
            start: 0.0,
            end: std::f64::consts::FRAC_PI_2,
        };
        // Mirror on the x axis: the arc goes from -90 degrees to 0.
        let m = map_shape(&a, &Xf::mirror(v(0.0, 0.0), v(1.0, 0.0)).unwrap()).unwrap();
        let Shape::Arc { start, end, .. } = m else {
            panic!()
        };
        assert!((arc_sweep(start, end) - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        let mid = DVec2::from_angle(start + arc_sweep(start, end) / 2.0);
        assert!(mid.distance(DVec2::from_angle(-std::f64::consts::FRAC_PI_4)) < 1e-12);
    }

    #[test]
    fn polyline_fillets_scale_and_stretch() {
        let pts = vec![
            Vertex::corner(v(0.0, 0.0)),
            Vertex::new(v(10.0, 0.0), VertexKind::Arc { radius: 2.0 }),
            Vertex::corner(v(10.0, 10.0)),
        ];
        let p = Shape::Polyline {
            points: pts,
            closed: false,
        };
        let s = map_shape(&p, &Xf::scale(v(0.0, 0.0), 3.0, 3.0)).unwrap();
        let Shape::Polyline { points, .. } = s else {
            panic!()
        };
        assert_eq!(points[1].kind, VertexKind::Arc { radius: 6.0 });
        // A stretch flattens the fillet to corners.
        let s = map_shape(&p, &Xf::scale(v(0.0, 0.0), 3.0, 1.0)).unwrap();
        let Shape::Polyline { points, .. } = s else {
            panic!()
        };
        assert!(points.len() > 3);
        assert!(points.iter().all(|v| v.is_straight()));
    }

    #[test]
    fn symbols_and_unsupported_shapes() {
        let sym = Shape::Symbol {
            symbol: crate::resource::ResourceId::from_num(1),
            at: v(1.0, 0.0),
            rotation: 0.5,
            scale: 2.0,
            scale_y: None,
        };
        let m = map_shape(&sym, &Xf::mirror(v(0.0, 0.0), v(0.0, 1.0)).unwrap()).unwrap();
        let Shape::Symbol {
            at, scale, scale_y, ..
        } = m
        else {
            panic!()
        };
        assert!(at.distance(v(-1.0, 0.0)) < 1e-12);
        assert_eq!((scale, scale_y), (2.0, Some(-2.0)));
        let h = Shape::Hardscape {
            boundary: curve::corners(&[v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0)]),
            material: "x".into(),
            thickness: None,
            elevation: None,
        };
        assert!(
            map_shape(&h, &Xf::scale(v(0.0, 0.0), 2.0, 2.0))
                .unwrap_err()
                .contains("cannot be scaled")
        );
        assert!(map_shape(&sym, &Xf::scale(v(0.0, 0.0), f64::NAN, 1.0)).is_err());
    }

    #[test]
    fn texts_plants_trees_tags_and_3d_points_move_and_scale() {
        use crate::plants::tree::{TreeAction, TreeCondition};
        use crate::resource::ResourceId;
        let up = Xf::scale(v(0.0, 0.0), 2.0, 2.0);
        // A stretch has no one scale: sizes use the square root of the area factor.
        let stretch = Xf::scale(v(0.0, 0.0), 4.0, 1.0);
        let text = Shape::Text {
            at: v(1.0, 1.0),
            text: "x".into(),
            height: 10.0,
        };
        assert!(matches!(
            map_shape(&text, &up).unwrap(),
            Shape::Text { at, height, .. } if at == v(2.0, 2.0) && height == 20.0
        ));
        assert!(matches!(
            map_shape(&text, &stretch).unwrap(),
            Shape::Text { height, .. } if (height - 20.0).abs() < 1e-9
        ));
        let plant = Shape::Plant {
            at: v(1.0, 0.0),
            species: "a".into(),
            spread: 100.0,
            height: 500.0,
        };
        assert!(matches!(
            map_shape(&plant, &up).unwrap(),
            Shape::Plant { at, spread, height, .. }
                if at == v(2.0, 0.0) && spread == 200.0 && height == 500.0
        ));
        assert!(matches!(
            map_shape(&plant, &stretch).unwrap(),
            Shape::Plant { spread, .. } if (spread - 200.0).abs() < 1e-9
        ));
        let tree = Shape::ExistingTree {
            at: v(0.0, 3.0),
            species: "oak".into(),
            dbh: 400.0,
            spread: 6000.0,
            height: 9000.0,
            condition: TreeCondition::default(),
            action: TreeAction::default(),
            rpz_factor: Some(10.0),
            rpz_cap: Some(8000.0),
        };
        assert!(matches!(
            map_shape(&tree, &up).unwrap(),
            Shape::ExistingTree { at, dbh, spread, rpz_factor, rpz_cap, .. }
                if at == v(0.0, 6.0) && dbh == 400.0 && spread == 12000.0
                    && rpz_factor == Some(10.0) && rpz_cap == Some(8000.0)
        ));
        let tag = Shape::PlantTag {
            at: v(1.0, 1.0),
            anchor: v(0.0, 1.0),
            style: ResourceId::from_num(3),
            height: 250.0,
        };
        assert!(matches!(
            map_shape(&tag, &up).unwrap(),
            Shape::PlantTag { at, anchor, height, .. }
                if at == v(2.0, 2.0) && anchor == v(0.0, 2.0) && height == 250.0
        ));
        // The height above the plan stays.
        let locus = Shape::Locus3d {
            p: glam::DVec3::new(1.0, 2.0, 7.0),
        };
        assert_eq!(
            map_shape(&locus, &up).unwrap(),
            Shape::Locus3d {
                p: glam::DVec3::new(2.0, 4.0, 7.0)
            }
        );
        let line3 = Shape::Polyline3d {
            points: vec![
                glam::DVec3::new(1.0, 0.0, 5.0),
                glam::DVec3::new(0.0, 1.0, 6.0),
            ],
            closed: false,
        };
        assert_eq!(
            map_shape(&line3, &up).unwrap(),
            Shape::Polyline3d {
                points: vec![
                    glam::DVec3::new(2.0, 0.0, 5.0),
                    glam::DVec3::new(0.0, 2.0, 6.0)
                ],
                closed: false,
            }
        );
        // A symbol turned (not mirrored) keeps its own y scale rule.
        let sym = Shape::Symbol {
            symbol: ResourceId::from_num(1),
            at: v(1.0, 0.0),
            rotation: 0.5,
            scale: 2.0,
            scale_y: Some(3.0),
        };
        let turned = map_shape(&sym, &Xf::rotate(v(0.0, 0.0), 0.25)).unwrap();
        assert!(matches!(
            turned,
            Shape::Symbol { rotation, scale, scale_y, .. }
                if (rotation - 0.75).abs() < 1e-9 && (scale - 2.0).abs() < 1e-9
                    && scale_y.is_some_and(|y| (y - 3.0).abs() < 1e-9)
        ));
        assert!(map_shape(&sym, &stretch).is_err());
        assert!(map_shape(&tree, &stretch).is_ok());
    }
}
