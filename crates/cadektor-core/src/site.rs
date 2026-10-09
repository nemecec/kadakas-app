//! Data of site models and site modifiers. Only the data lives here. The terrain
//! algorithms (TIN, contours, cut and fill) are in the `cadektor-site` crate, and derived results
//! are never saved in files.

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

use crate::curve::{self, Vertex};
use crate::model::Color;

/// A 3D polygon or contour that is source data of a site model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SiteLine {
    pub points: Vec<DVec3>,
    #[serde(default)]
    pub closed: bool,
}

/// Which surface a view shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceDisplay {
    Existing,
    #[default]
    Proposed,
    Both,
}

/// How the 3D view shows the terrain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Display3d {
    #[default]
    Mesh,
    Contours,
    /// Mesh and contours together.
    Both,
}

/// Settings of a site model. Lengths are in mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteSettings {
    /// Height between two contour lines.
    pub minor_interval: f64,
    /// Height between two major (thick) contour lines. A multiple of the minor interval.
    pub major_interval: f64,
    /// Round the corners of contour lines.
    pub smooth_contours: bool,
    /// Draw the elevation next to major contours in 2D.
    pub label_contours: bool,
    pub display_2d: SurfaceDisplay,
    pub display_3d: Display3d,
    /// Which surface the 3D view shows.
    pub surface_3d: SurfaceDisplay,
    /// The elevation that contour levels count from.
    pub datum: f64,
    /// Color of the existing surface (mesh and contours). `None` picks a default.
    pub existing_color: Option<Color>,
    /// Color of the proposed surface. `None` picks a default.
    pub proposed_color: Option<Color>,
    /// Only the part of the surface inside this ring stays. Empty: no crop. The ring may be concave.
    pub crop: Vec<DVec2>,
    /// When false, the proposed surface changes only on an explicit "Update Site Model" and shows
    /// "out of date" until then.
    pub auto_update: bool,
}

/// The most contour levels a site model may ask for.
pub const MAX_CONTOUR_LEVELS: f64 = 10_000.0;

impl Default for SiteSettings {
    fn default() -> Self {
        SiteSettings {
            minor_interval: 500.0,
            major_interval: 2500.0,
            smooth_contours: false,
            label_contours: false,
            display_2d: SurfaceDisplay::default(),
            display_3d: Display3d::default(),
            surface_3d: SurfaceDisplay::default(),
            datum: 0.0,
            existing_color: None,
            proposed_color: None,
            crop: Vec::new(),
            auto_update: true,
        }
    }
}

impl SiteSettings {
    /// True if every number is finite.
    pub fn is_finite(&self) -> bool {
        self.minor_interval.is_finite() && self.major_interval.is_finite() && self.datum.is_finite() && self.crop.iter().all(|p| p.is_finite())
    }

    /// Why the settings are not usable, if they are not.
    pub fn problem(&self) -> Option<&'static str> {
        if !self.is_finite() {
            Some("site model numbers must be finite")
        } else if self.minor_interval <= 0.0 {
            Some("minor contour interval must be positive")
        } else if self.major_interval < self.minor_interval {
            Some("major contour interval must not be smaller than the minor interval")
        } else if self.major_interval / self.minor_interval > MAX_CONTOUR_LEVELS {
            Some("major contour interval is too large for the minor interval (at most 10000 times)")
        } else if self.minor_interval < 1e-3 {
            Some("minor contour interval must be at least 0.001 mm")
        } else if !self.crop.is_empty() && self.crop.len() < 3 {
            Some("crop boundary needs at least 3 points")
        } else {
            None
        }
    }
}

/// A slope of a pad: it falls in `direction` (radians, counterclockwise from +X) by `percent`
/// of the horizontal distance.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PadSlope {
    pub direction: f64,
    pub percent: f64,
}

/// What a site modifier does to the proposed surface.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModifierKind {
    /// A level or sloped area. `elevation` is the height at the middle of the boundary points.
    Pad {
        elevation: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        slope: Option<PadSlope>,
    },
    /// The edge of the modified area: outside it, the proposed surface stays the existing one.
    GradeLimit,
}

impl ModifierKind {
    pub fn is_finite(&self) -> bool {
        match self {
            ModifierKind::Pad { elevation, slope } => {
                elevation.is_finite() && slope.is_none_or(|s| s.direction.is_finite() && s.percent.is_finite())
            }
            ModifierKind::GradeLimit => true,
        }
    }

    /// The numbers of the kind, for range checks.
    pub fn numbers(&self) -> Vec<f64> {
        match self {
            ModifierKind::Pad { elevation, slope } => {
                let mut v = vec![*elevation];
                v.extend(slope.iter().flat_map(|s| [s.percent]));
                v
            }
            ModifierKind::GradeLimit => Vec::new(),
        }
    }
}

/// The corners of a modifier boundary with curves flattened.
pub fn boundary_points(boundary: &[Vertex]) -> Vec<DVec2> {
    curve::flatten(boundary, true, 2.0)
}

/// The farthest distance from the middle of the boundary points along `direction` (radians).
fn reach(boundary: &[DVec2], direction: f64) -> f64 {
    let mid = boundary.iter().copied().sum::<DVec2>() / boundary.len().max(1) as f64;
    let d = DVec2::from_angle(direction);
    boundary.iter().map(|p| d.dot(*p - mid)).fold(0.0, f64::max)
}

/// A pad slope from the elevation at its end: the height where the boundary ends in the
/// falling `direction`. `None` if the boundary has no extent in that direction.
pub fn slope_from_end_elevation(boundary: &[DVec2], elevation: f64, direction: f64, end_elevation: f64) -> Option<PadSlope> {
    let r = reach(boundary, direction);
    (r > 0.0 && end_elevation.is_finite()).then(|| PadSlope { direction, percent: (elevation - end_elevation) / r * 100.0 })
}

/// The elevation at the end of a sloped pad (see `slope_from_end_elevation`).
pub fn end_elevation(boundary: &[DVec2], elevation: f64, slope: PadSlope) -> f64 {
    elevation - slope.percent / 100.0 * reach(boundary, slope.direction)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_valid_and_checked() {
        let mut s = SiteSettings::default();
        assert_eq!(s.problem(), None);
        s.minor_interval = 0.0;
        assert!(s.problem().unwrap().contains("positive"));
        s.minor_interval = 500.0;
        s.major_interval = 100.0;
        assert!(s.problem().unwrap().contains("major"));
        s.major_interval = f64::NAN;
        assert!(s.problem().unwrap().contains("finite"));
        s.major_interval = 500.0;
        s.minor_interval = 1e-300;
        assert!(s.problem().is_some());
        s.minor_interval = 1.0;
        s.major_interval = 1e6;
        assert!(s.problem().unwrap().contains("too large"));
        s.major_interval = 10.0;
        s.crop = vec![DVec2::ZERO, DVec2::X];
        assert!(s.problem().unwrap().contains("crop"));
    }

    #[test]
    fn settings_read_with_missing_fields() {
        let s: SiteSettings = serde_json::from_str(r#"{"minor_interval": 250.0, "display_2d": "both"}"#).unwrap();
        assert_eq!(s.minor_interval, 250.0);
        assert_eq!(s.display_2d, SurfaceDisplay::Both);
        assert_eq!(s.major_interval, 2500.0);
    }

    #[test]
    fn modifier_kind_numbers_and_json() {
        let k = ModifierKind::Pad { elevation: 5.0, slope: Some(PadSlope { direction: 0.0, percent: 2.0 }) };
        assert!(k.is_finite());
        assert_eq!(k.numbers(), vec![5.0, 2.0]);
        let json = serde_json::to_string(&k).unwrap();
        assert_eq!(serde_json::from_str::<ModifierKind>(&json).unwrap(), k);
        assert!(!ModifierKind::Pad { elevation: f64::NAN, slope: None }.is_finite());
        assert!(!ModifierKind::Pad { elevation: 0.0, slope: Some(PadSlope { direction: f64::NAN, percent: 1.0 }) }.is_finite());
        assert!(ModifierKind::GradeLimit.is_finite() && ModifierKind::GradeLimit.numbers().is_empty());
    }

    #[test]
    fn slope_from_the_elevation_at_the_end() {
        let sq = [DVec2::new(0.0, 0.0), DVec2::new(10.0, 0.0), DVec2::new(10.0, 10.0), DVec2::new(0.0, 10.0)];
        // Falls toward +X; the end is 5 from the middle. 10 at the middle and 9 at the end is 20 percent.
        let s = slope_from_end_elevation(&sq, 10.0, 0.0, 9.0).unwrap();
        assert!((s.percent - 20.0).abs() < 1e-9 && s.direction == 0.0);
        assert!((end_elevation(&sq, 10.0, s) - 9.0).abs() < 1e-9);
        assert!(slope_from_end_elevation(&[], 1.0, 0.0, 2.0).is_none());
        assert!(slope_from_end_elevation(&sq, 1.0, 0.0, f64::NAN).is_none());
    }

    #[test]
    fn boundary_flattens_to_points() {
        let b = curve::corners(&[DVec2::ZERO, DVec2::X, DVec2::ONE]);
        assert_eq!(boundary_points(&b).len(), 3);
    }
}
