//! The standard views and the view direction of a DXF viewport (IOP-16).
//!
//! The view direction of a viewport (group codes 16, 26, 36) points from the target to the eye.
//! A sheet viewport shows a plan or one of the standard 3D views of Kadakas.

use kadakas_core::DVec3;

/// The standard views and the directions from the target to the eye.
const VIEWS: [(&str, [f64; 3]); 10] = [
    ("plan", [0.0, 0.0, 1.0]),
    ("bottom", [0.0, 0.0, -1.0]),
    ("front", [0.0, -1.0, 0.0]),
    ("back", [0.0, 1.0, 0.0]),
    ("left", [-1.0, 0.0, 0.0]),
    ("right", [1.0, 0.0, 0.0]),
    ("left_isometric", [-1.0, -1.0, 1.0]),
    ("right_isometric", [1.0, -1.0, 1.0]),
    ("left_rear_isometric", [-1.0, 1.0, 1.0]),
    ("right_rear_isometric", [1.0, 1.0, 1.0]),
];

/// The standard view that looks along this direction, and whether it is the exact direction (within
/// one degree).
pub fn view_for(dir: DVec3) -> (&'static str, bool) {
    let d = dir.normalize_or(DVec3::Z);
    let (mut best, mut dot) = ("plan", -2.0);
    for (name, v) in VIEWS {
        let c = d.dot(DVec3::from(v).normalize());
        if c > dot {
            (best, dot) = (name, c);
        }
    }
    (best, dot >= 1f64.to_radians().cos())
}

/// The direction (from the target to the eye, not normalized) of a standard view id. A view that
/// Kadakas does not know gives the plan.
pub fn direction_of(view: &str) -> [f64; 3] {
    VIEWS
        .iter()
        .find(|(n, _)| *n == view)
        .map_or([0.0, 0.0, 1.0], |(_, d)| *d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_are_found_by_direction() {
        assert_eq!(view_for(DVec3::new(0.0, 0.0, 1.0)), ("plan", true));
        assert_eq!(view_for(DVec3::new(0.0, 0.0, 5.0)), ("plan", true));
        assert_eq!(view_for(DVec3::new(0.0, -1.0, 0.0)), ("front", true));
        assert_eq!(
            view_for(DVec3::new(-1.0, -1.0, 1.0)),
            ("left_isometric", true)
        );
        assert_eq!(
            view_for(DVec3::new(1.0, 1.0, 1.0)),
            ("right_rear_isometric", true)
        );
        assert_eq!(view_for(DVec3::new(0.0, 0.0, -1.0)), ("bottom", true));
        let (name, exact) = view_for(DVec3::new(0.3, -1.0, 0.5));
        assert!(!exact && !name.is_empty());
        assert_eq!(view_for(DVec3::ZERO), ("plan", true));
    }

    #[test]
    fn every_view_has_its_direction() {
        for (name, d) in VIEWS {
            assert_eq!(direction_of(name), d);
            assert_eq!(view_for(DVec3::from(d)), (name, true));
        }
        assert_eq!(direction_of("nonsense"), [0.0, 0.0, 1.0]);
    }
}
