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
        let plain = Georeference { rotation: 0.0, ..g.clone() };
        assert_eq!(plain.to_model(DVec2::new(385_001.0, 6_672_002.0)), DVec2::new(1000.0, 2000.0));
        assert!(!Georeference { rotation: f64::NAN, ..g }.is_finite());
    }
}
