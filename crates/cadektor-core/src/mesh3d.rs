//! A generic 3D object: an indexed triangle mesh in model coordinates (mm, Z up).
//!
//! It holds geometry that has no native Cadektor shape, for example a slab or a covering from an
//! IFC file. The vertices are absolute: Z is the height in the model, not above the layer.
//! The source class (for example `IfcSlab`) is kept in the `data` of the object.

use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::model::{Color, MAX_COORD};

/// Most vertices and triangles that one mesh object may hold.
pub const MAX_VERTICES: usize = 4_000_000;
pub const MAX_TRIANGLES: usize = 8_000_000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mesh3d {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub vertices: Vec<DVec3>,
    /// Three vertex indices per triangle, counterclockwise seen from outside.
    pub triangles: Vec<[u32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

impl Mesh3d {
    pub fn is_finite(&self) -> bool {
        self.vertices.iter().all(|v| v.is_finite())
    }

    /// The first problem of the mesh, if any.
    pub fn problem(&self) -> Option<String> {
        if self.vertices.is_empty() || self.triangles.is_empty() {
            return Some("a mesh needs vertices and triangles".into());
        }
        if self.vertices.len() > MAX_VERTICES || self.triangles.len() > MAX_TRIANGLES {
            return Some("the mesh is too large".into());
        }
        if !self.is_finite()
            || self
                .vertices
                .iter()
                .any(|v| v.abs().max_element() > MAX_COORD)
        {
            return Some("a mesh vertex is not finite or out of range".into());
        }
        let n = self.vertices.len() as u32;
        if self.triangles.iter().any(|t| t.iter().any(|&i| i >= n)) {
            return Some("a triangle refers to a vertex that does not exist".into());
        }
        None
    }

    /// Box around the vertices as (min, max).
    pub fn bounds3(&self) -> (DVec3, DVec3) {
        self.vertices.iter().fold(
            (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        )
    }

    /// Volume in mm3 when the mesh is closed (the absolute value of the signed volume).
    pub fn volume(&self) -> f64 {
        let v: f64 = self
            .triangles
            .iter()
            .filter_map(|t| {
                let p = |i: u32| self.vertices.get(i as usize).copied();
                Some(p(t[0])?.dot(p(t[1])?.cross(p(t[2])?)))
            })
            .sum();
        (v / 6.0).abs()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tetra() -> Mesh3d {
        Mesh3d {
            name: "t".into(),
            vertices: vec![
                DVec3::ZERO,
                DVec3::new(6.0, 0.0, 0.0),
                DVec3::new(0.0, 6.0, 0.0),
                DVec3::new(0.0, 0.0, 6.0),
            ],
            triangles: vec![[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]],
            color: None,
        }
    }

    #[test]
    fn checks_and_measures() {
        let m = tetra();
        assert!(m.problem().is_none());
        assert!((m.volume() - 36.0).abs() < 1e-9);
        let (lo, hi) = m.bounds3();
        assert_eq!((lo, hi), (DVec3::ZERO, DVec3::splat(6.0)));
        let mut bad = m.clone();
        bad.triangles.push([0, 1, 9]);
        assert!(bad.problem().is_some());
        let mut bad = m.clone();
        bad.vertices[0].x = f64::NAN;
        assert!(bad.problem().is_some());
        assert!(Mesh3d::default().problem().is_some());
    }

    #[test]
    fn json_round_trip() {
        let m = tetra();
        let s = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Mesh3d>(&s).unwrap(), m);
    }
}
