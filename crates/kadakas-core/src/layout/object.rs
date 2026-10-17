//! Adapters between document objects and the layout engine: the outline and holes of an object
//! as a region, and the pattern that an object brings with it.

use glam::DVec2;

use super::{Pattern, Placement, Region};
use crate::curve::{self, Vertex};
use crate::model::{Document, Object, Shape};

/// The outline and the holes of an object that can be filled with a pattern: a closed polyline, a
/// rectangle, a circle, a landscape area or a hardscape. `Err` names the kind of shape that cannot.
pub fn outline_of(obj: &Object) -> Result<(Vec<Vertex>, Vec<Vec<Vertex>>), String> {
    Ok(match &obj.shape {
        Shape::Polyline {
            points,
            closed: true,
        } => {
            // A closing point that repeats the first one is not a corner.
            let mut p = points.clone();
            if p.len() > 3 && p[0].p.distance(p[p.len() - 1].p) < 1e-9 {
                p.pop();
            }
            (p, Vec::new())
        }
        // A closed 3D polyline is read in plan: its heights are dropped.
        Shape::Polyline3d {
            points,
            closed: true,
        } => {
            let mut p: Vec<DVec2> = points.iter().map(|v| DVec2::new(v.x, v.y)).collect();
            if p.len() > 3 && p[0].distance(p[p.len() - 1]) < 1e-9 {
                p.pop();
            }
            (curve::corners(&p), Vec::new())
        }
        Shape::Polyline { closed: false, .. } => {
            return Err("an open polyline has no inside. Close it first".into());
        }
        Shape::Rect { min, max } => {
            let (lo, hi) = (min.min(*max), min.max(*max));
            (
                curve::corners(&[lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)]),
                Vec::new(),
            )
        }
        Shape::Circle { center, radius } => {
            let n = 256;
            (
                curve::corners(
                    &(0..n)
                        .map(|k| {
                            *center
                                + DVec2::from_angle(std::f64::consts::TAU * k as f64 / n as f64)
                                    * *radius
                        })
                        .collect::<Vec<_>>(),
                ),
                Vec::new(),
            )
        }
        Shape::LandscapeArea { boundary, .. } => (
            boundary.clone(),
            crate::landscape::data(obj)
                .map(|d| d.holes)
                .unwrap_or_default(),
        ),
        Shape::Hardscape { boundary, .. } => (
            boundary.clone(),
            crate::hardscape::data(obj)
                .map(|d| d.holes)
                .unwrap_or_default(),
        ),
        other => {
            return Err(format!(
                "a {} cannot be filled with a pattern",
                other.kind()
            ));
        }
    })
}

/// The region of an object, with its holes, as plain geometry (the border of a hardscape is not
/// taken off; see [`region_of`]).
pub fn plain_region_of(obj: &Object, tolerance: f64) -> Result<Region, String> {
    let (outer, holes) = outline_of(obj)?;
    Region::new(&outer, &holes, tolerance)
}

/// The width of the kerb border of a hardscape and the sides it is on.
fn border_of(doc: &Document, obj: &Object) -> Option<(f64, crate::hardscape::Sides)> {
    let (_, style) = crate::hardscape::data::style_of(doc, obj)?;
    let b = style.border?;
    (b.width > 0.0).then_some((b.width, b.sides))
}

/// The region that a pattern is laid in. For a hardscape with a kerb border, the border is not
/// paved with units: the region is the outline moved inward by the border width, and the holes
/// moved outward by it (a path loses the border only on the sides it has). Holes that overlap are
/// merged, and holes outside the outline are cut off.
pub fn region_of(doc: &Document, obj: &Object, tolerance: f64) -> Result<Region, String> {
    let Some((width, sides)) =
        border_of(doc, obj).filter(|_| matches!(obj.shape, Shape::Hardscape { .. }))
    else {
        return plain_region_of(obj, tolerance);
    };
    let Shape::Hardscape { boundary, .. } = &obj.shape else {
        unreachable!("checked above")
    };
    let d = crate::hardscape::data(obj).unwrap_or_default();
    let ring = crate::geom::open_ring(&curve::flatten(boundary, true, tolerance), 1e-9);
    let collapsed = || format!("the kerb border of {width} mm covers the whole area");
    let outers: Vec<Vec<DVec2>> = if d.mode == crate::hardscape::Mode::Path {
        let count = sides.count() as f64;
        let w2 = d.width - width * count;
        if w2 <= 0.0 {
            return Err(collapsed());
        }
        let extra = match sides {
            crate::hardscape::Sides::Both => 0.0,
            crate::hardscape::Sides::Left => -width / 2.0,
            crate::hardscape::Sides::Right => width / 2.0,
        };
        let axis = crate::hardscape::offset::shifted(
            &d.center_points(),
            d.offset
                + extra
                + match d.align {
                    crate::hardscape::offset::Align::Center => 0.0,
                    crate::hardscape::offset::Align::Left => -d.width / 2.0,
                    crate::hardscape::offset::Align::Right => d.width / 2.0,
                },
        );
        vec![crate::hardscape::offset::path_outline(
            &axis, w2, d.join, d.closed,
        )]
    } else {
        move_ring(&ring, width, crate::edit2d::offset::Side::Inside).ok_or_else(collapsed)?
    };
    let mut holes: Vec<Vec<DVec2>> = Vec::new();
    for h in &d.holes {
        let hr = crate::geom::open_ring(&curve::flatten(h, true, tolerance), 1e-9);
        match move_ring(&hr, width, crate::edit2d::offset::Side::Outside) {
            Some(rs) => holes.extend(rs),
            None => holes.push(hr),
        }
    }
    Region::from_paths(&outers, &holes)
}

/// A flat ring moved by `d` to a side. `None` if nothing is left.
fn move_ring(ring: &[DVec2], d: f64, side: crate::edit2d::offset::Side) -> Option<Vec<Vec<DVec2>>> {
    let shape = Shape::Polyline {
        points: curve::corners(ring),
        closed: true,
    };
    let shapes = crate::edit2d::offset::offset_shapes(&shape, d, side).ok()?;
    let rings: Vec<Vec<DVec2>> = shapes
        .iter()
        .filter_map(crate::edit2d::offset::outline_points)
        .collect();
    (!rings.is_empty()).then_some(rings)
}

/// A placement that starts at vertex `index` of the outline of the object and runs along the edge
/// to the next vertex. For a hardscape with a kerb border the start is moved to the inner border
/// line, where the pattern begins.
pub fn align_placement(doc: &Document, obj: &Object, index: usize) -> Result<Placement, String> {
    let (outline, _) = outline_of(obj)?;
    if let Shape::Hardscape { .. } = obj.shape {
        let mut d = crate::hardscape::data(obj).unwrap_or_default();
        // A path has the outline of its center line.
        let outline = if d.mode == crate::hardscape::Mode::Path {
            d.outline()
        } else {
            outline
        };
        d.holes.clear();
        return crate::hardscape::data::align_start(&outline, &d, border_of(doc, obj), index);
    }
    Placement::along_edge(&outline, index)
}

/// The pattern of an object, with where it starts: for a hardscape, the joint pattern of its style
/// with the angle and the origin of the object itself laid over it. `None` if the object has none.
pub fn pattern_of(doc: &Document, obj: &Object) -> Option<(Pattern, Placement)> {
    let (_, style) = crate::hardscape::data::style_of(doc, obj)?;
    let joints = style.joints?;
    let d = crate::hardscape::data(obj).unwrap_or_default();
    Some((
        joints.layout(),
        Placement {
            origin: d.origin,
            angle: d.angle.unwrap_or(joints.angle),
        },
    ))
}
