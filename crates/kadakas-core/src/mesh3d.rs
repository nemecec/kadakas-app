//! A generic 3D object: an indexed triangle mesh in model coordinates (mm, Z up).
//!
//! It holds geometry that has no native Kadakas shape, for example a slab or a covering from an
//! IFC file. The vertices are absolute: Z is the height in the model, not above the layer.
//! The source class (for example `IfcSlab`) is kept in the `data` of the object.

use glam::{DAffine3, DVec2, DVec3};
use serde::{Deserialize, Serialize};

use crate::exact::{self, ExactSolid};
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
    /// The exact surfaces that the triangles were cut from (a STEP import). The triangles stay
    /// the source for drawing and for every tool; see [`crate::exact`]. A tool that edits the
    /// triangles does not need to know: the payload is stale then (`exact_state`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact: Option<Box<ExactSolid>>,
}

impl Mesh3d {
    /// Is the exact payload in step with the triangles? `None` when there is no payload.
    pub fn exact_state(&self) -> Option<exact::State> {
        let e = self.exact.as_ref()?;
        Some(
            if e.mesh_hash == exact::fingerprint(&self.vertices, &self.triangles) {
                exact::State::Fresh
            } else {
                exact::State::Stale
            },
        )
    }

    /// Puts a payload on the mesh and ties it to the triangles as they are now.
    pub fn set_exact(&mut self, mut e: ExactSolid) {
        e.mesh_hash = exact::fingerprint(&self.vertices, &self.triangles);
        self.exact = Some(Box::new(e));
    }

    /// Moves the mesh in the plan; the payload moves with it and stays in step.
    pub fn translate_xy(&mut self, d: DVec2) {
        let map = DAffine3::from_translation(DVec3::new(d.x, d.y, 0.0));
        self.apply_map(&map, |p| DVec3::new(p.x + d.x, p.y + d.y, p.z));
    }

    /// Turns the mesh about a vertical axis; the payload turns with it.
    pub fn rotate_z(&mut self, center: DVec2, angle: f64) {
        let (s, c) = angle.sin_cos();
        let turn = move |p: DVec3| {
            let r = DVec2::new(p.x - center.x, p.y - center.y);
            DVec3::new(
                center.x + r.x * c - r.y * s,
                center.y + r.x * s + r.y * c,
                p.z,
            )
        };
        let map = DAffine3::from_translation(DVec3::new(center.x, center.y, 0.0))
            * DAffine3::from_rotation_z(angle)
            * DAffine3::from_translation(DVec3::new(-center.x, -center.y, 0.0));
        self.apply_map(&map, turn);
    }

    /// Moves every vertex with `f` and the payload with `map` (the same move). A payload that
    /// was stale stays stale. A map the payload cannot follow drops it; the result says so.
    fn apply_map(&mut self, map: &DAffine3, f: impl Fn(DVec3) -> DVec3) -> Option<&'static str> {
        let fresh = self.exact_state() == Some(exact::State::Fresh);
        self.vertices.iter_mut().for_each(|p| *p = f(*p));
        let mut note = None;
        if let Some(e) = self.exact.take() {
            match e.transformed(map) {
                Ok(mut moved) => {
                    moved.mesh_hash = if fresh {
                        exact::fingerprint(&self.vertices, &self.triangles)
                    } else {
                        e.mesh_hash
                    };
                    self.exact = Some(Box::new(moved));
                }
                Err(_) => {
                    note = Some(
                        "the exact data was dropped: the map is not a move, turn, mirror or uniform scale",
                    )
                }
            }
        }
        note
    }

    /// Maps the mesh by an affine map (any map moves the triangles). The payload follows a
    /// move, a turn, a mirror and a uniform scale. For a stretch or a shear it is dropped, and
    /// the answer is a note for the report.
    pub fn transform(&mut self, map: &DAffine3) -> Option<&'static str> {
        let m = *map;
        self.apply_map(map, move |p| m.transform_point3(p))
    }

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
        if let Some(e) = &self.exact
            && let Err(why) = e.validate()
        {
            return Some(format!("the exact data is not valid: {why}"));
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
            exact: None,
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
