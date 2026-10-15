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
        } => (points.clone(), Vec::new()),
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

/// The region of an object, with its holes.
pub fn region_of(obj: &Object, tolerance: f64) -> Result<Region, String> {
    let (outer, holes) = outline_of(obj)?;
    Region::new(&outer, &holes, tolerance)
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
