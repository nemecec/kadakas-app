//! Georeferencing: where the drawing sits on the earth.
//!
//! The document keeps one optional [`Georeference`]. It names a coordinate
//! reference system (CRS) and says which map position the model origin has.
//! Geometry stays near (0, 0), which keeps numbers small. The core does not
//! convert between CRSs. File importers do that (see `cadektor-io`).

use glam::DVec2;
use serde::{Deserialize, Serialize};

/// Links the model space (millimeters) to a map CRS (normally in meters).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Georeference {
    /// CRS of the map coordinates: `EPSG:<code>` or a WKT string. Empty: no CRS is known, and
    /// `origin` only records the shift of a local frame (an import that centered the drawing).
    pub crs: String,
    /// Map position (CRS units, normally meters) of the model origin.
    pub origin: DVec2,
    /// Counterclockwise angle of the model X axis against the map X axis, in radians.
    #[serde(default)]
    pub rotation: f64,
    /// Model position (mm) that the user sees as (0, 0), like the user origin of a drawing.
    #[serde(default)]
    pub user_origin: DVec2,
}

impl Georeference {
    pub fn is_finite(&self) -> bool {
        self.origin.is_finite() && self.rotation.is_finite() && self.user_origin.is_finite()
    }

    /// Model position (mm) of a map position (meters).
    pub fn to_model(&self, map: DVec2) -> DVec2 {
        DVec2::from_angle(-self.rotation).rotate(map - self.origin) * 1000.0
    }

    /// Map position (meters) of a model position (mm).
    pub fn to_map(&self, model: DVec2) -> DVec2 {
        DVec2::from_angle(self.rotation).rotate(model / 1000.0) + self.origin
    }
}

impl Georeference {
    /// The georeference in which the model position `model` (mm) is the known map position
    /// `map` (meters). The model does not move: only the map origin changes. `rotation` is
    /// the angle of the model X axis against the map X axis.
    pub fn from_known_point(
        crs: &str,
        model: DVec2,
        map: DVec2,
        rotation: f64,
        user_origin: DVec2,
    ) -> Georeference {
        Georeference {
            crs: crs.to_string(),
            origin: map - DVec2::from_angle(rotation).rotate(model / 1000.0),
            rotation,
            user_origin,
        }
    }

    /// The georeference from two known points. The rotation follows from the direction of the
    /// line between them. Also returns the scale ratio (map distance over model distance in
    /// meters). It is 1 when the points agree. `None` if a point repeats.
    pub fn from_two_points(
        crs: &str,
        model: [DVec2; 2],
        map: [DVec2; 2],
        user_origin: DVec2,
    ) -> Option<(Georeference, f64)> {
        let (dm, dp) = (model[1] - model[0], map[1] - map[0]);
        if dm.length() < 1e-9 || dp.length() < 1e-9 || !(dm.is_finite() && dp.is_finite()) {
            return None;
        }
        let rotation = dp.to_angle() - dm.to_angle();
        let scale = dp.length() / (dm.length() / 1000.0);
        Some((
            Georeference::from_known_point(crs, model[0], map[0], rotation, user_origin),
            scale,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_and_map_round_trip() {
        let g = Georeference {
            crs: "EPSG:3067".into(),
            origin: DVec2::new(385_000.0, 6_672_000.0),
            rotation: 0.3,
            user_origin: DVec2::ZERO,
        };
        let m = DVec2::new(1234.5, -987.0);
        assert!(g.to_model(g.to_map(m)).distance(m) < 1e-6);
        assert!(g.is_finite());
        let plain = Georeference {
            rotation: 0.0,
            ..g.clone()
        };
        assert_eq!(
            plain.to_model(DVec2::new(385_001.0, 6_672_002.0)),
            DVec2::new(1000.0, 2000.0)
        );
        assert!(
            !Georeference {
                rotation: f64::NAN,
                ..g
            }
            .is_finite()
        );
    }

    #[test]
    fn a_known_point_sets_the_origin_and_two_points_set_the_rotation() {
        let g = Georeference::from_known_point(
            "EPSG:3067",
            DVec2::new(5000.0, 2000.0),
            DVec2::new(385_005.0, 6_672_002.0),
            0.0,
            DVec2::ZERO,
        );
        assert_eq!(g.origin, DVec2::new(385_000.0, 6_672_000.0));
        assert_eq!(
            g.to_map(DVec2::new(5000.0, 2000.0)),
            DVec2::new(385_005.0, 6_672_002.0)
        );
        let rot = 0.4;
        let (m1, m2) = (DVec2::new(1000.0, 1000.0), DVec2::new(9000.0, 4000.0));
        let truth = Georeference {
            crs: "EPSG:3067".into(),
            origin: DVec2::new(100.0, 200.0),
            rotation: rot,
            user_origin: DVec2::ZERO,
        };
        let (g, scale) = Georeference::from_two_points(
            "EPSG:3067",
            [m1, m2],
            [truth.to_map(m1), truth.to_map(m2)],
            DVec2::ZERO,
        )
        .unwrap();
        assert!((g.rotation - rot).abs() < 1e-9 && (scale - 1.0).abs() < 1e-9);
        assert!(g.origin.distance(truth.origin) < 1e-6);
        assert!(Georeference::from_two_points("", [m1, m1], [m1, m2], DVec2::ZERO).is_none());
        assert!(Georeference::from_two_points("", [m1, m2], [m1, m1], DVec2::ZERO).is_none());
    }
}
