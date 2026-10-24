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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh3d {
    pub name: String,
    pub vertices: Vec<DVec3>,
    /// Three vertex indices per triangle, counterclockwise seen from outside.
    pub triangles: Vec<[u32; 3]>,
    pub color: Option<Color>,
    /// The exact surfaces that the triangles were cut from (a STEP import). The triangles stay
    /// the source for drawing and for every tool; see [`crate::exact`]. A tool that edits the
    /// triangles does not need to know: the payload is stale then (`exact_state`).
    pub exact: Option<Box<ExactSolid>>,
    /// A payload that this program cannot use (a newer version, an unknown kind of surface,
    /// damaged data). It is kept as it is, written back unchanged and never touched by an edit.
    /// At most one of `exact` and `exact_unusable` is set.
    pub exact_unusable: Option<Box<UnusableExact>>,
}

/// A payload that the program does not understand, as it came from the file.
#[derive(Clone, Debug, PartialEq)]
pub struct UnusableExact {
    pub raw: serde_json::Value,
    pub reason: String,
}

/// The form of a mesh in the file. The payload is read as plain JSON first, so a payload that
/// the program does not understand never makes the document unreadable.
#[derive(Deserialize)]
struct Wire {
    #[serde(default)]
    name: String,
    vertices: Vec<DVec3>,
    triangles: Vec<[u32; 3]>,
    #[serde(default)]
    color: Option<Color>,
    #[serde(default)]
    exact: Option<serde_json::Value>,
}

impl<'de> Deserialize<'de> for Mesh3d {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let w = Wire::deserialize(d)?;
        let (exact, exact_unusable) = match w.exact {
            None | Some(serde_json::Value::Null) => (None, None),
            Some(raw) => match ExactSolid::from_json(&raw) {
                Ok(e) => (Some(Box::new(e)), None),
                Err(reason) => (None, Some(Box::new(UnusableExact { raw, reason }))),
            },
        };
        Ok(Mesh3d {
            name: w.name,
            vertices: w.vertices,
            triangles: w.triangles,
            color: w.color,
            exact,
            exact_unusable,
        })
    }
}

impl Serialize for Mesh3d {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let payload = self.exact.is_some() || self.exact_unusable.is_some();
        let n = 2
            + usize::from(!self.name.is_empty())
            + usize::from(self.color.is_some())
            + usize::from(payload);
        let mut st = s.serialize_struct("Mesh3d", n)?;
        if !self.name.is_empty() {
            st.serialize_field("name", &self.name)?;
        }
        st.serialize_field("vertices", &self.vertices)?;
        st.serialize_field("triangles", &self.triangles)?;
        if let Some(c) = &self.color {
            st.serialize_field("color", c)?;
        }
        if let Some(e) = &self.exact {
            st.serialize_field("exact", e)?;
        } else if let Some(u) = &self.exact_unusable {
            st.serialize_field("exact", &u.raw)?;
        }
        st.end()
    }
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
    use crate::exact::Surface;

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
            exact_unusable: None,
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

    fn with_exact() -> Mesh3d {
        use crate::exact::{ExactEdge, ExactFace, ExactLoop, ExactShell, LoopItem, Surface};
        let mut m = tetra();
        let solid = ExactSolid {
            version: exact::VERSION,
            product: "T".into(),
            unit_mm: 1.0,
            tolerance_mm: 0.1,
            angle_deg: 15.0,
            vertices: m.vertices.clone(),
            edges: vec![ExactEdge {
                curve: None,
                start: 0,
                end: 1,
                same_sense: true,
            }],
            shells: vec![ExactShell {
                closed: true,
                outward: None,
            }],
            faces: vec![ExactFace {
                shell: 0,
                surface: Some(Surface::Cylinder {
                    f: DAffine3::IDENTITY,
                    r: 2.0,
                }),
                same_sense: true,
                flip: false,
                loops: vec![ExactLoop {
                    outer: true,
                    items: vec![LoopItem::Edge {
                        edge: 0,
                        forward: true,
                    }],
                }],
                color: None,
                source: 0,
            }],
            mesh_hash: 0,
            extra: Default::default(),
        };
        m.set_exact(solid);
        m
    }

    #[test]
    fn the_exact_data_follows_moves_and_turns_and_goes_stale_on_edits() {
        use crate::exact::State;
        let mut m = with_exact();
        assert_eq!(m.exact_state(), Some(State::Fresh));
        assert!(m.problem().is_none());
        m.translate_xy(DVec2::new(10.0, -5.0));
        assert_eq!(m.exact_state(), Some(State::Fresh));
        assert_eq!(m.exact.as_ref().unwrap().vertices[1], m.vertices[1]);
        m.rotate_z(DVec2::new(1.0, 1.0), 0.8);
        assert_eq!(m.exact_state(), Some(State::Fresh));
        assert!((m.exact.as_ref().unwrap().vertices[2] - m.vertices[2]).length() < 1e-9);
        // A uniform scale and a mirror are kept, the radius scales.
        let scale = DAffine3::from_scale(DVec3::splat(3.0));
        assert!(m.transform(&scale).is_none());
        assert_eq!(m.exact_state(), Some(State::Fresh));
        let Some(Surface::Cylinder { r, .. }) = &m.exact.as_ref().unwrap().faces[0].surface else {
            panic!()
        };
        assert!((r - 6.0).abs() < 1e-9);
        // A stretch cannot be followed: the payload is dropped, with a note.
        let mut t = m.clone();
        let stretch = DAffine3::from_scale(DVec3::new(2.0, 1.0, 1.0));
        assert!(t.transform(&stretch).is_some());
        assert!(t.exact.is_none() && t.exact_state().is_none());
        // An edit of the triangles makes the payload stale, and a move leaves it stale.
        m.vertices[0].z += 1.0;
        assert_eq!(m.exact_state(), Some(State::Stale));
        m.translate_xy(DVec2::new(1.0, 1.0));
        assert_eq!(m.exact_state(), Some(State::Stale));
        // A payload with a damaged index does not make the mesh unreadable: it is unusable.
        let mut v = serde_json::to_value(with_exact()).unwrap();
        v["exact"]["edges"][0]["end"] = serde_json::json!(99);
        let bad: Mesh3d = serde_json::from_value(v.clone()).unwrap();
        assert!(bad.exact.is_none() && bad.problem().is_none());
        assert!(
            bad.exact_unusable
                .as_ref()
                .unwrap()
                .reason
                .contains("not valid")
        );
        assert_eq!(serde_json::to_value(&bad).unwrap(), v);
    }

    #[test]
    fn a_payload_of_a_later_version_loads_stays_and_is_not_touched() {
        let mut v = serde_json::to_value(with_exact()).unwrap();
        v["exact"]["version"] = serde_json::json!(2);
        v["exact"]["faces"][0]["surface"] = serde_json::json!({"kind": "helicoid", "pitch": 3});
        let mut m: Mesh3d = serde_json::from_str(&v.to_string()).unwrap();
        assert!(m.exact.is_none() && m.exact_state().is_none() && m.problem().is_none());
        let u = m.exact_unusable.as_ref().unwrap();
        assert!(u.reason.contains("version 2"), "{}", u.reason);
        // Edits of the triangles leave it as it is, and the file writes it back unchanged.
        m.translate_xy(DVec2::new(5.0, 5.0));
        assert!(
            m.transform(&DAffine3::from_scale(DVec3::new(2.0, 1.0, 1.0)))
                .is_none()
        );
        let back = serde_json::to_value(&m).unwrap();
        assert_eq!(back["exact"], v["exact"]);
        // A payload of an unknown kind in the version that is known is unusable too.
        let mut w = v.clone();
        w["exact"]["version"] = serde_json::json!(1);
        let m: Mesh3d = serde_json::from_value(w.clone()).unwrap();
        assert!(
            m.exact_unusable
                .as_ref()
                .unwrap()
                .reason
                .contains("not understood")
        );
        assert_eq!(serde_json::to_value(&m).unwrap()["exact"], w["exact"]);
        // A payload that is not an object also loads.
        w["exact"] = serde_json::json!("junk");
        let m: Mesh3d = serde_json::from_value(w).unwrap();
        assert!(m.exact_unusable.is_some());
    }

    #[test]
    fn the_payload_is_optional_in_the_file_and_unknown_fields_survive() {
        // A mesh from an older program has no payload.
        let old = r#"{"name":"t","vertices":[[0,0,0],[1,0,0],[0,1,0]],"triangles":[[0,1,2]]}"#;
        let m: Mesh3d = serde_json::from_str(old).unwrap();
        assert!(m.exact.is_none() && m.exact_state().is_none());
        assert!(!serde_json::to_string(&m).unwrap().contains("exact"));
        // A mesh with a payload round-trips, and a field of a later version stays.
        let m = with_exact();
        let mut v = serde_json::to_value(&m).unwrap();
        v["exact"]["later"] = serde_json::json!([1, 2, 3]);
        let back: Mesh3d = serde_json::from_value(v).unwrap();
        assert_eq!(
            serde_json::to_value(&back).unwrap()["exact"]["later"],
            serde_json::json!([1, 2, 3])
        );
        assert_eq!(back.exact_state(), Some(exact::State::Fresh));
    }
}
