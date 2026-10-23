//! Spatial questions about the faces of a surface model (MOD-11): which faces a ray meets, and
//! which faces are near a point.
//!
//! The functions work on the triangles of [`SurfaceModel::mesh`] with the positions the caller
//! gives (local ones, or the ones moved into the drawing), so that a caller can keep the mesh for
//! a revision of the model. The triangle tests are the ones of the solid tools (`solid/locate.rs`).

use std::collections::BTreeMap;

use glam::DVec3;

use super::solid::{closest_on_triangle, ray_hit_distance};
use super::{FaceId, SurfaceMesh, geom};

fn tolerance(positions: &[DVec3]) -> f64 {
    let size = geom::bounds(positions.iter().copied())
        .map_or(1.0, |(lo, hi)| (hi - lo).max_element().max(1.0));
    geom::plane_tolerance_at(size)
}

fn triangle(mesh: &SurfaceMesh, positions: &[DVec3], t: usize) -> [DVec3; 3] {
    mesh.triangles[t].map(|i| positions[i as usize])
}

/// The faces that the ray meets (from either side), nearest first, each with the distance to its
/// nearest hit. `direction` must be a unit vector. `skip` leaves faces out (hidden faces).
pub fn ray_faces(
    mesh: &SurfaceMesh,
    positions: &[DVec3],
    origin: DVec3,
    direction: DVec3,
    skip: impl Fn(FaceId) -> bool,
) -> Vec<(FaceId, f64)> {
    let tol = tolerance(positions);
    let mut best: BTreeMap<FaceId, f64> = BTreeMap::new();
    for (t, face) in mesh.faces.iter().enumerate() {
        if skip(*face) {
            continue;
        }
        if let Some(d) = ray_hit_distance(origin, direction, &triangle(mesh, positions, t), tol)
            && best.get(face).is_none_or(|b| d < *b)
        {
            best.insert(*face, d);
        }
    }
    let mut out: Vec<(FaceId, f64)> = best.into_iter().collect();
    out.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    out
}

/// The faces within `radius` of the point, nearest first, each with its distance and the nearest
/// point of the face.
pub fn faces_near(
    mesh: &SurfaceMesh,
    positions: &[DVec3],
    point: DVec3,
    radius: f64,
    skip: impl Fn(FaceId) -> bool,
) -> Vec<(FaceId, f64, DVec3)> {
    let mut best: BTreeMap<FaceId, (f64, DVec3)> = BTreeMap::new();
    for (t, face) in mesh.faces.iter().enumerate() {
        if skip(*face) {
            continue;
        }
        let c = closest_on_triangle(point, &triangle(mesh, positions, t));
        let d = c.distance(point);
        if d <= radius && best.get(face).is_none_or(|(b, _)| d < *b) {
            best.insert(*face, (d, c));
        }
    }
    let mut out: Vec<(FaceId, f64, DVec3)> =
        best.into_iter().map(|(f, (d, c))| (f, d, c)).collect();
    out.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    out
}

/// The point of the segment `ab` that is nearest to `p`.
pub fn closest_on_segment(p: DVec3, a: DVec3, b: DVec3) -> DVec3 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 == 0.0 {
        return a;
    }
    a + ab * (((p - a).dot(ab) / len2).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::{Edit, SurfaceModel};

    fn square() -> SurfaceModel {
        let mut m = SurfaceModel::new();
        let p = |x: f64, y: f64| [x, y, 0.0];
        let c = [p(0.0, 0.0), p(100.0, 0.0), p(100.0, 100.0), p(0.0, 100.0)];
        m.apply(&Edit::AddEdges {
            edges: (0..4).map(|i| [c[i], c[(i + 1) % 4]]).collect(),
        })
        .unwrap();
        m
    }

    #[test]
    fn a_ray_meets_the_face_from_both_sides_and_a_hidden_face_is_skipped() {
        let m = square();
        let mesh = m.mesh().unwrap();
        let pos = mesh.positions.clone();
        let face = mesh.faces[0];
        let down = DVec3::new(0.0, 0.0, -1.0);
        let hits = ray_faces(&mesh, &pos, DVec3::new(50.0, 50.0, 10.0), down, |_| false);
        assert_eq!(hits.len(), 1);
        assert!((hits[0].1 - 10.0).abs() < 1e-9);
        let up = ray_faces(&mesh, &pos, DVec3::new(50.0, 50.0, -10.0), -down, |_| false);
        assert_eq!(up[0].0, face);
        assert!(
            ray_faces(&mesh, &pos, DVec3::new(50.0, 50.0, 10.0), down, |f| f
                == face)
            .is_empty()
        );
        // Behind the origin and off the face: no hit.
        assert!(ray_faces(&mesh, &pos, DVec3::new(50.0, 50.0, 10.0), -down, |_| false).is_empty());
        assert!(ray_faces(&mesh, &pos, DVec3::new(500.0, 50.0, 10.0), down, |_| false).is_empty());
    }

    #[test]
    fn faces_near_a_point_come_nearest_first_with_the_closest_point() {
        let m = square();
        let mesh = m.mesh().unwrap();
        let pos = mesh.positions.clone();
        let near = faces_near(&mesh, &pos, DVec3::new(150.0, 50.0, 0.0), 60.0, |_| false);
        assert_eq!(near.len(), 1);
        assert!((near[0].1 - 50.0).abs() < 1e-9);
        assert_eq!(near[0].2, DVec3::new(100.0, 50.0, 0.0));
        assert!(faces_near(&mesh, &pos, DVec3::new(150.0, 50.0, 0.0), 10.0, |_| false).is_empty());
        assert!(faces_near(&mesh, &pos, DVec3::ZERO, 10.0, |_| true).is_empty());
    }

    #[test]
    fn the_closest_point_of_a_segment_is_clamped() {
        let (a, b) = (DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0));
        assert_eq!(
            closest_on_segment(DVec3::new(5.0, 3.0, 0.0), a, b),
            DVec3::new(5.0, 0.0, 0.0)
        );
        assert_eq!(closest_on_segment(DVec3::new(-5.0, 3.0, 0.0), a, b), a);
        assert_eq!(closest_on_segment(DVec3::new(15.0, 3.0, 0.0), a, b), b);
        assert_eq!(closest_on_segment(DVec3::ONE, a, a), a);
    }
}
