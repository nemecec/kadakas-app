//! Blocks and text in a tilted plane (IOP-27).
//!
//! A block reference or a text can lie in a plane that the extrusion direction (codes 210, 220 and
//! 230) tilts. The shapes of a plan are flat, and a text has no work plane, so the import places
//! what a plan can hold in 3D and projects the rest:
//!
//! - Lines, polylines, circles and arcs become 3D polylines (curves are cut within the tolerance).
//! - 3D polylines, points and meshes move in 3D.
//! - A fill cannot lie in a tilted plane. The outline stays and the fill is counted.
//! - A text is placed at its projected point on the drawing plane. The turn comes from the
//!   projected baseline, and the height from the projected height, so a text in a plane that is
//!   tilted by 60 degrees is drawn at half its height. In a plane that stands nearly upright (the
//!   height would be nothing) the text keeps its full height. A plane seen from the back mirrors
//!   the text. A text cannot be mirrored, so it is counted.
//!
//! The entities are first converted as if the plane were flat, in the drawing. The tilt then moves
//! the result: if `P` is the transform of the container (the drawing, or the block around) and `T`
//! the axes of the plane, the point `p` goes to `P T P^-1 p`. That map is a rotation about the origin
//! of the container, so the origin of the block reference or the text stays the pivot.

use std::rc::Rc;

use kadakas_core::curve;
use kadakas_core::model::{Shape, arc_sweep};
use kadakas_core::{DAffine3, DVec2, DVec3};

use super::tilted::Ocs;
use super::{Item, Tally, Xf, arc_steps};

/// A plane that stands up straighter than this (the part of its normal that points up is below it)
/// is nearly upright: the projected height of a text would be less than one tenth.
const UPRIGHT: f64 = 0.1;

/// A tilt chain: the maps of the tilted references around, innermost first applied.
pub(super) type Chain = Option<Rc<DAffine3>>;

/// `x -> xf(x)` as a 3D map: the plan transform and the height transform.
fn xf_map(xf: &Xf) -> DAffine3 {
    DAffine3::from_cols(
        xf.cols[0].extend(0.0),
        xf.cols[1].extend(0.0),
        DVec3::new(0.0, 0.0, xf.zs),
        xf.t.extend(xf.zt),
    )
}

/// The map `P T P^-1` of a tilted plane inside the transform `xf` of its container. `None` when
/// `xf` cannot be inverted.
pub(super) fn map_of(xf: &Xf, ocs: &Ocs) -> Option<DAffine3> {
    let p = xf_map(xf);
    let t = DAffine3::from_cols(ocs.x, ocs.y, ocs.z, DVec3::ZERO);
    let c = p * t * p.inverse();
    (c.matrix3.to_cols_array().iter().all(|v| v.is_finite()) && c.translation.is_finite())
        .then_some(c)
}

/// The chain of a reference inside the chain of the references around it. The inner map is applied first.
pub(super) fn chain(outer: &Chain, inner: DAffine3) -> Chain {
    Some(Rc::new(match outer {
        Some(o) => **o * inner,
        None => inner,
    }))
}

/// The tilt of an item: the map of its own plane first, then the chain of the references around.
pub(super) fn combined(own: Option<DAffine3>, outer: &Chain) -> Option<DAffine3> {
    match (own, outer) {
        (Some(a), Some(b)) => Some(**b * a),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(**b),
        (None, None) => None,
    }
}

/// Moves the items of one entity by a tilt map. `z0` is the height of a flat shape.
pub(super) fn apply(
    items: Vec<Item>,
    map: &DAffine3,
    z0: f64,
    tol: f64,
    t: &mut Tally,
) -> Vec<Item> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let z = item.z.unwrap_or(z0);
        let flat = |p: DVec2| map.transform_point3(p.extend(z));
        let filled = item.fill.is_some() || item.fill_pen || item.pattern.is_some();
        // The outline of a shape that was filled: the fill is lost.
        let outline = |mut item: Item, shape: Shape, t: &mut Tally| {
            if filled {
                t.tilted_fills += 1;
            }
            item.fill = None;
            item.fill_pen = false;
            item.pattern = None;
            item.shape = shape;
            t.tilted_objects += 1;
            item
        };
        match item.shape.clone() {
            // The white island of a hatch means nothing without a fill.
            _ if item.data.contains_key("hatch_island") => t.tilted_islands += 1,
            Shape::Line { a, b } => {
                let shape = Shape::Polyline3d {
                    points: vec![flat(a), flat(b)],
                    closed: false,
                };
                out.push(outline(item, shape, t));
            }
            Shape::Polyline { points, closed } => {
                let ring = curve::flatten(&points, closed, tol);
                let shape = Shape::Polyline3d {
                    points: ring.iter().map(|p| flat(*p)).collect(),
                    closed,
                };
                out.push(outline(item, shape, t));
            }
            Shape::Circle { center, radius } => {
                let n = arc_steps(radius, std::f64::consts::TAU, tol).max(8);
                let points = (0..n)
                    .map(|i| {
                        flat(
                            center
                                + DVec2::from_angle(std::f64::consts::TAU * i as f64 / n as f64)
                                    * radius,
                        )
                    })
                    .collect();
                let shape = Shape::Polyline3d {
                    points,
                    closed: true,
                };
                out.push(outline(item, shape, t));
            }
            Shape::Arc {
                center,
                radius,
                start,
                end,
            } => {
                let sweep = arc_sweep(start, end);
                let n = arc_steps(radius, sweep, tol).max(2);
                let points = (0..=n)
                    .map(|i| {
                        flat(
                            center
                                + DVec2::from_angle(start + sweep * i as f64 / n as f64) * radius,
                        )
                    })
                    .collect();
                let shape = Shape::Polyline3d {
                    points,
                    closed: false,
                };
                out.push(outline(item, shape, t));
            }
            Shape::Polyline3d { points, closed } => {
                let shape = Shape::Polyline3d {
                    points: points.iter().map(|p| map.transform_point3(*p)).collect(),
                    closed,
                };
                out.push(outline(item, shape, t));
            }
            Shape::Locus3d { p } => {
                let shape = Shape::Locus3d {
                    p: map.transform_point3(p),
                };
                out.push(outline(item, shape, t));
            }
            Shape::Mesh3d(mut m) => {
                for v in &mut m.vertices {
                    *v = map.transform_point3(*v);
                }
                out.push(outline(item, Shape::Mesh3d(m), t));
            }
            Shape::Text { at, text, height } => {
                out.push(place_text(item, at, text, height, z, map, t))
            }
            _ => out.push(item),
        }
    }
    out
}

/// A text on the drawing plane from a text in a tilted plane.
fn place_text(
    mut item: Item,
    at: DVec2,
    text: String,
    height: f64,
    z: f64,
    map: &DAffine3,
    t: &mut Tally,
) -> Item {
    let turn = item
        .data
        .get("rotation_deg")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0)
        .to_radians();
    let (sin, cos) = turn.sin_cos();
    let base = map.matrix3 * DVec3::new(cos, sin, 0.0);
    let up = map.matrix3 * DVec3::new(-sin, cos, 0.0);
    let normal = map.matrix3 * DVec3::Z;
    t.tilted_texts += 1;
    if normal.z < 0.0 {
        t.tilted_texts_back += 1;
    }
    let upright = up.truncate().length() < UPRIGHT;
    if upright {
        t.tilted_texts_upright += 1;
    }
    // The baseline: its projection, or (when the text runs straight up) the projection of its height turned back by 90 degrees.
    let dir = if base.truncate().length() >= UPRIGHT {
        base.truncate()
    } else {
        DVec2::new(up.y, -up.x)
    };
    let new_height = if upright {
        height
    } else {
        height * up.truncate().length()
    };
    item.shape = Shape::Text {
        at: map.transform_point3(at.extend(z)).truncate(),
        text,
        height: new_height,
    };
    let deg = dir.y.atan2(dir.x).to_degrees();
    item.data.remove("rotation_deg");
    if deg.is_finite() && deg.abs() > 1e-9 {
        let deg = (deg * 1e6).round() / 1e6;
        item.data.insert(
            "rotation_deg".into(),
            serde_json::Value::from(deg.rem_euclid(360.0)),
        );
    }
    item
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::curve::{VertexKind, corners};

    fn xf() -> Xf {
        Xf::scale(1.0)
    }

    fn vertical_x() -> Ocs {
        // Normal along +X: the plane is the YZ plane.
        Ocs::of([1.0, 0.0, 0.0]).unwrap()
    }

    fn near(a: DVec3, b: DVec3) -> bool {
        a.distance(b) < 1e-9
    }

    #[test]
    fn the_map_turns_about_the_origin_of_the_container() {
        let c = map_of(&xf(), &vertical_x()).unwrap();
        // OCS x is world Y, OCS y is world Z, OCS z is world X. The map takes the flat OCS
        // coordinates (x, y, z) of the drawing to their place.
        assert!(near(
            c.transform_point3(DVec3::new(1.0, 0.0, 0.0)),
            DVec3::new(0.0, 1.0, 0.0)
        ));
        assert!(near(
            c.transform_point3(DVec3::new(0.0, 1.0, 0.0)),
            DVec3::new(0.0, 0.0, 1.0)
        ));
        assert!(near(
            c.transform_point3(DVec3::new(0.0, 0.0, 1.0)),
            DVec3::new(1.0, 0.0, 0.0)
        ));
        // A container that moves: the pivot is the origin of the container, not of the drawing.
        let moved = Xf {
            t: DVec2::new(100.0, 50.0),
            ..Xf::scale(2.0)
        };
        let c = map_of(&moved, &vertical_x()).unwrap();
        let origin = moved.apply(DVec2::ZERO).extend(moved.z(0.0));
        assert!(near(c.transform_point3(origin), origin));
        // A point one unit along OCS x in the container (2 mm in the drawing) goes along world Y.
        let p = moved.apply(DVec2::new(1.0, 0.0)).extend(moved.z(0.0));
        assert!(near(
            c.transform_point3(p),
            origin + DVec3::new(0.0, 2.0, 0.0)
        ));
        // A container that cannot be inverted gives no map.
        assert!(map_of(&Xf::scale(0.0), &vertical_x()).is_none());
    }

    #[test]
    fn maps_chain_and_combine() {
        let a = map_of(&xf(), &vertical_x()).unwrap();
        let b = DAffine3::from_translation(DVec3::new(0.0, 0.0, 5.0));
        let chained = chain(&None, a);
        let both = chain(&chained, b).unwrap();
        // The inner map first, then the outer.
        assert!(near(
            both.transform_point3(DVec3::ZERO),
            a.transform_point3(b.transform_point3(DVec3::ZERO))
        ));
        assert!(combined(None, &None).is_none());
        assert!(combined(Some(a), &None).is_some());
        assert!(combined(None, &chained).is_some());
        let c = combined(Some(b), &chained).unwrap();
        assert!(near(
            c.transform_point3(DVec3::ZERO),
            a.transform_point3(b.transform_point3(DVec3::ZERO))
        ));
    }

    fn run(shape: Shape, z: Option<f64>) -> (Vec<Item>, Tally) {
        let mut item = Item::new(shape);
        item.z = z;
        let mut t = Tally::default();
        let c = map_of(&xf(), &vertical_x()).unwrap();
        (apply(vec![item], &c, 0.0, 0.01, &mut t), t)
    }

    #[test]
    fn flat_shapes_become_3d_polylines_in_the_plane() {
        let (out, t) = run(
            Shape::Line {
                a: DVec2::new(0.0, 0.0),
                b: DVec2::new(10.0, 5.0),
            },
            Some(2.0),
        );
        let Shape::Polyline3d { points, .. } = &out[0].shape else {
            panic!()
        };
        // OCS (10, 5, 2) -> world (2, 10, 5).
        assert!(near(points[1], DVec3::new(2.0, 10.0, 5.0)), "{points:?}");
        assert_eq!(t.tilted_objects, 1);
        let (out, _) = run(
            Shape::Circle {
                center: DVec2::new(1.0, 1.0),
                radius: 3.0,
            },
            None,
        );
        let Shape::Polyline3d { points, closed } = &out[0].shape else {
            panic!()
        };
        assert!(*closed && points.len() >= 8);
        assert!(points.iter().all(|p| p.x.abs() < 1e-9));
        let (out, _) = run(
            Shape::Arc {
                center: DVec2::ZERO,
                radius: 4.0,
                start: 0.0,
                end: std::f64::consts::FRAC_PI_2,
            },
            None,
        );
        let Shape::Polyline3d { points, closed } = &out[0].shape else {
            panic!()
        };
        assert!(!closed && near(points[0], DVec3::new(0.0, 4.0, 0.0)));
        assert!(near(*points.last().unwrap(), DVec3::new(0.0, 0.0, 4.0)));
        let (out, _) = run(
            Shape::Polyline {
                points: corners(&[
                    DVec2::new(0.0, 0.0),
                    DVec2::new(1.0, 0.0),
                    DVec2::new(1.0, 1.0),
                ]),
                closed: true,
            },
            None,
        );
        assert!(matches!(
            &out[0].shape,
            Shape::Polyline3d { closed: true, .. }
        ));
    }

    #[test]
    fn a_polyline_with_arcs_is_cut_into_points() {
        let pts = vec![
            kadakas_core::curve::Vertex::new(DVec2::new(0.0, 0.0), VertexKind::Corner),
            kadakas_core::curve::Vertex::new(DVec2::new(5.0, 3.0), VertexKind::PointOnArc),
            kadakas_core::curve::Vertex::new(DVec2::new(10.0, 0.0), VertexKind::Corner),
        ];
        let (out, _) = run(
            Shape::Polyline {
                points: pts,
                closed: false,
            },
            None,
        );
        let Shape::Polyline3d { points, .. } = &out[0].shape else {
            panic!()
        };
        assert!(points.len() > 4);
    }

    #[test]
    fn a_fill_is_lost_and_an_island_is_dropped() {
        let mut item = Item::new(Shape::Polyline {
            points: corners(&[DVec2::ZERO, DVec2::X, DVec2::ONE]),
            closed: true,
        });
        item.fill_pen = true;
        let mut island = item.clone();
        island.fill = Some(kadakas_core::model::Color::BLACK);
        island.data.insert("hatch_island".into(), true.into());
        let mut t = Tally::default();
        let c = map_of(&xf(), &vertical_x()).unwrap();
        let out = apply(vec![item, island], &c, 0.0, 0.01, &mut t);
        assert_eq!(out.len(), 1);
        assert!(out[0].fill.is_none() && !out[0].fill_pen);
        assert_eq!(t.tilted_fills, 1);
    }

    #[test]
    fn points_meshes_and_3d_polylines_move() {
        let (out, _) = run(
            Shape::Locus3d {
                p: DVec3::new(1.0, 2.0, 3.0),
            },
            None,
        );
        assert!(matches!(out[0].shape, Shape::Locus3d { p } if near(p, DVec3::new(3.0, 1.0, 2.0))));
        let mesh = kadakas_core::mesh3d::Mesh3d {
            name: String::new(),
            vertices: vec![
                DVec3::new(1.0, 0.0, 0.0),
                DVec3::new(0.0, 1.0, 0.0),
                DVec3::new(0.0, 0.0, 1.0),
            ],
            triangles: vec![[0, 1, 2]],
            color: None,
            exact: None,
            exact_unusable: None,
        };
        let (out, _) = run(Shape::Mesh3d(Box::new(mesh)), None);
        let Shape::Mesh3d(m) = &out[0].shape else {
            panic!()
        };
        assert!(near(m.vertices[0], DVec3::new(0.0, 1.0, 0.0)));
        let (out, _) = run(
            Shape::Polyline3d {
                points: vec![DVec3::new(1.0, 0.0, 0.0), DVec3::new(0.0, 1.0, 0.0)],
                closed: false,
            },
            None,
        );
        assert!(
            matches!(&out[0].shape, Shape::Polyline3d { points, .. } if near(points[0], DVec3::new(0.0, 1.0, 0.0)))
        );
        // A shape that a tilt does not touch passes.
        let (out, t) = run(Shape::Group { name: "g".into() }, None);
        assert!(matches!(out[0].shape, Shape::Group { .. }) && t.tilted_objects == 0);
    }

    fn text_in(ocs: Ocs, turn_deg: f64) -> (Item, Tally) {
        let mut item = Item::new(Shape::Text {
            at: DVec2::new(10.0, 20.0),
            text: "T".into(),
            height: 100.0,
        });
        if turn_deg != 0.0 {
            item.data.insert("rotation_deg".into(), turn_deg.into());
        }
        let mut t = Tally::default();
        let c = map_of(&xf(), &ocs).unwrap();
        let out = apply(vec![item], &c, 0.0, 0.01, &mut t);
        (out.into_iter().next().unwrap(), t)
    }

    fn text_parts(item: &Item) -> (DVec2, f64, f64) {
        let Shape::Text { at, height, .. } = &item.shape else {
            panic!()
        };
        let rot = item
            .data
            .get("rotation_deg")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        (*at, *height, rot)
    }

    #[test]
    fn a_text_in_a_tilted_plane_is_placed_by_its_projection() {
        // A plane tilted by 60 degrees about the X axis: the normal is (0, -sin60, cos60). The text
        // runs along X, and its height runs up the plane: the projected height is cos60.
        let a = 60f64.to_radians();
        let ocs = Ocs::of([0.0, -a.sin(), a.cos()]).unwrap();
        let (item, t) = text_in(ocs, 0.0);
        let (_, h, rot) = text_parts(&item);
        assert!((h - 50.0).abs() < 1e-6, "{h}");
        assert!(rot.abs() < 1e-6 || (rot - 360.0).abs() < 1e-6);
        assert_eq!(
            (t.tilted_texts, t.tilted_texts_upright, t.tilted_texts_back),
            (1, 0, 0)
        );
    }

    #[test]
    fn a_text_on_a_wall_keeps_its_height_and_follows_the_wall() {
        // A wall along X (the normal is -Y): the text runs along X, and its height is vertical.
        let ocs = Ocs::of([0.0, -1.0, 0.0]).unwrap();
        let (item, t) = text_in(ocs, 0.0);
        let (at, h, rot) = text_parts(&item);
        assert!((h - 100.0).abs() < 1e-9);
        assert!((at - DVec2::new(10.0, 0.0)).length() < 1e-9, "{at:?}");
        assert!(rot.abs() < 1e-6);
        assert_eq!(t.tilted_texts_upright, 1);
        // A wall along Y (the normal is +X): the text runs along Y.
        let (item, _) = text_in(vertical_x(), 0.0);
        let (_, h, rot) = text_parts(&item);
        assert!(
            (h - 100.0).abs() < 1e-9 && (rot - 90.0).abs() < 1e-6,
            "{rot}"
        );
        // A text on a wall that runs straight up the wall: its baseline is vertical, so it follows the height.
        let (item, _) = text_in(Ocs::of([0.0, -1.0, 0.0]).unwrap(), 90.0);
        let (_, _, rot) = text_parts(&item);
        assert!(rot.is_finite());
    }

    #[test]
    fn a_plane_seen_from_the_back_is_counted() {
        let a = 120f64.to_radians();
        let ocs = Ocs::of([0.0, -a.sin(), a.cos()]).unwrap();
        let (_, t) = text_in(ocs, 0.0);
        assert_eq!(t.tilted_texts_back, 1);
    }
}
