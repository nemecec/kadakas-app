//! Existing trees: survey trees that the design keeps, removes or moves.
//!
//! The root protection zone is a circle around the trunk. The default radius is
//! 12 times the trunk diameter, and at most 15 m. These are the public rules of the
//! British standard BS 5837:2012 for the root protection area of a single stem
//! (12 x stem diameter, with a radius of 15 m at most). Users can set another factor
//! and another cap per tree.

use std::f64::consts::TAU;

use glam::DVec2;
use serde::{Deserialize, Serialize};

/// Default root protection radius, as a multiple of the trunk diameter (DBH).
pub const DEFAULT_RPZ_FACTOR: f64 = 12.0;

/// Health of an existing tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeCondition {
    Good,
    #[default]
    Fair,
    Poor,
    Dead,
}

impl TreeCondition {
    pub const ALL: [TreeCondition; 4] = [TreeCondition::Good, TreeCondition::Fair, TreeCondition::Poor, TreeCondition::Dead];

    pub fn id(self) -> &'static str {
        match self {
            TreeCondition::Good => "good",
            TreeCondition::Fair => "fair",
            TreeCondition::Poor => "poor",
            TreeCondition::Dead => "dead",
        }
    }
}

/// What the design does with an existing tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeAction {
    #[default]
    Retain,
    Remove,
    Relocate,
}

impl TreeAction {
    pub const ALL: [TreeAction; 3] = [TreeAction::Retain, TreeAction::Remove, TreeAction::Relocate];

    pub fn id(self) -> &'static str {
        match self {
            TreeAction::Retain => "retain",
            TreeAction::Remove => "remove",
            TreeAction::Relocate => "relocate",
        }
    }
}

/// Default largest root protection radius, in mm (BS 5837).
pub const DEFAULT_RPZ_CAP: f64 = 15_000.0;

/// Radius of the root protection zone, in mm: `dbh` times the factor, at most the cap.
/// A missing or invalid factor or cap means the default.
pub fn rpz_radius(dbh: f64, factor: Option<f64>, cap: Option<f64>) -> f64 {
    let f = factor.filter(|f| f.is_finite() && *f >= 0.0).unwrap_or(DEFAULT_RPZ_FACTOR);
    let cap = cap.filter(|c| c.is_finite() && *c >= 0.0).unwrap_or(DEFAULT_RPZ_CAP);
    (dbh.max(0.0) * f).min(cap)
}

/// A circle as a closed point list, for drawing.
pub fn circle(center: DVec2, radius: f64, segments: usize) -> Vec<DVec2> {
    (0..segments).map(|k| center + DVec2::from_angle(TAU * k as f64 / segments as f64) * radius).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpz_uses_the_documented_rule() {
        assert_eq!(rpz_radius(500.0, None, None), 6000.0);
        assert_eq!(rpz_radius(500.0, Some(10.0), None), 5000.0);
        assert_eq!(rpz_radius(500.0, Some(0.0), None), 0.0);
        // A bad factor falls back to the default.
        assert_eq!(rpz_radius(500.0, Some(-1.0), None), 6000.0);
        assert_eq!(rpz_radius(500.0, Some(f64::NAN), None), 6000.0);
        assert_eq!(rpz_radius(-5.0, None, None), 0.0);
    }

    #[test]
    fn the_radius_is_capped_at_15_m_unless_the_cap_is_changed() {
        // 1.5 m trunk: 12 x 1500 = 18 m, capped.
        assert_eq!(rpz_radius(1500.0, None, None), 15_000.0);
        assert_eq!(rpz_radius(1500.0, None, Some(20_000.0)), 18_000.0);
        assert_eq!(rpz_radius(1500.0, None, Some(5_000.0)), 5_000.0);
        assert_eq!(rpz_radius(1500.0, None, Some(f64::NAN)), 15_000.0);
        assert_eq!(rpz_radius(1500.0, None, Some(-1.0)), 15_000.0);
        assert_eq!(rpz_radius(1250.0, None, None), 15_000.0);
    }

    #[test]
    fn enums_use_snake_case_names() {
        for c in TreeCondition::ALL {
            assert_eq!(serde_json::to_value(c).unwrap(), c.id());
        }
        for a in TreeAction::ALL {
            assert_eq!(serde_json::to_value(a).unwrap(), a.id());
        }
        assert_eq!(TreeAction::default(), TreeAction::Retain);
        assert_eq!(TreeCondition::default(), TreeCondition::Fair);
    }

    #[test]
    fn circle_points_lie_on_the_circle() {
        let c = DVec2::new(3.0, 4.0);
        let pts = circle(c, 5.0, 16);
        assert_eq!(pts.len(), 16);
        assert!(pts.iter().all(|p| (p.distance(c) - 5.0).abs() < 1e-9));
    }
}
