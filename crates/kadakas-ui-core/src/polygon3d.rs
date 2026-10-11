//! Settings of the 3D Polygon tool (3D-03).

use glam::DVec2;
use kadakas_core::curve::Vertex;
use kadakas_core::model::Shape;

/// Settings of the 3D Polygon tool: the height of the new vertices and whether the polygon closes.
#[derive(Clone, Debug, PartialEq)]
pub struct Polygon3dSettings {
    /// Height of every new vertex above the layer, in mm. Edit single heights in the
    /// vertex list of Object Info or with `polygon3d.set`.
    pub z: f64,
    /// True: a closed polygon (it can be filled). False: an open path.
    pub closed: bool,
}

impl Default for Polygon3dSettings {
    fn default() -> Self {
        Polygon3dSettings {
            z: 0.0,
            closed: true,
        }
    }
}

impl Polygon3dSettings {
    /// The 3D polygon for the clicked points, or `None` when there are too few of them.
    pub fn shape(&self, pts: &[Vertex]) -> Option<Shape> {
        let need = if self.closed { 3 } else { 2 };
        let z = if self.z.is_finite() { self.z } else { 0.0 };
        let points: Vec<_> = pts.iter().map(|v| DVec2::extend(v.p, z)).collect();
        (points.len() >= need).then_some(Shape::Polyline3d {
            points,
            closed: self.closed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::curve::corners;

    #[test]
    fn the_points_get_the_height_and_few_points_make_nothing() {
        let pts = corners(&[DVec2::ZERO, DVec2::X, DVec2::Y]);
        let mut s = Polygon3dSettings::default();
        s.z = 250.0;
        let Some(Shape::Polyline3d { points, closed }) = s.shape(&pts) else {
            panic!()
        };
        assert!(closed && points.iter().all(|p| p.z == 250.0) && points.len() == 3);
        assert!(s.shape(&pts[..2]).is_none());
        s.closed = false;
        assert!(s.shape(&pts[..2]).is_some() && s.shape(&pts[..1]).is_none());
        s.z = f64::NAN;
        let Some(Shape::Polyline3d { points, .. }) = s.shape(&pts) else {
            panic!()
        };
        assert!(points.iter().all(|p| p.z == 0.0));
    }
}
