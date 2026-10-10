//! The ground under objects, as a trait.
//!
//! Core knows hardscapes, landscape areas and grading objects, but not terrain. They ask for
//! heights through [`GroundSource`]. The site crate answers with the site models of a document.

use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::model::Object;

/// Which surface of a site model to ask about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    #[default]
    Existing,
    Proposed,
}

impl Surface {
    pub fn name(self) -> &'static str {
        match self {
            Surface::Existing => "existing",
            Surface::Proposed => "proposed",
        }
    }
}

/// The ground that objects stand on.
///
/// `exact` is the height of the surface at a point, `None` outside the site model. `clamped` also
/// answers outside the model: there the ground has the height of the nearest edge, as hardscapes
/// do. An object that follows the ground uses `clamped` and says that it is partly off the model.
/// Snapping picks up only `exact` heights.
///
/// `obj` is the object that asks. Its layer selects the site models. Queries that belong to no
/// object pass `None`; a source that serves one site model ignores it.
pub trait GroundSource {
    fn exact(&self, obj: Option<&Object>, p: DVec2, which: Surface) -> Option<f64>;

    fn clamped(&self, obj: Option<&Object>, p: DVec2, which: Surface) -> Option<f64> {
        self.exact(obj, p, which)
    }

    /// Height of the ground (the proposed surface, with the edge height outside the model) at
    /// `p` for this object, or `None` if there is no ground at all.
    fn elevation(&self, obj: &Object, p: DVec2) -> Option<f64> {
        self.clamped(Some(obj), p, Surface::Proposed)
    }

    /// True if the point lies on the ground itself. Outside, `elevation` may still give the height
    /// of the nearest edge.
    fn covered(&self, obj: &Object, p: DVec2) -> bool {
        self.exact(Some(obj), p, Surface::Proposed).is_some()
    }
}

/// No ground: nothing drapes.
pub struct NoTerrain;

impl GroundSource for NoTerrain {
    fn exact(&self, _obj: Option<&Object>, _p: DVec2, _which: Surface) -> Option<f64> {
        None
    }
}

/// A closure is a ground without an edge (`clamped` is `exact`).
impl<F: Fn(DVec2, Surface) -> Option<f64>> GroundSource for F {
    fn exact(&self, _obj: Option<&Object>, p: DVec2, which: Surface) -> Option<f64> {
        self(p, which)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClassId, LayerId, ObjectId, Shape};

    fn object() -> Object {
        Object::new(
            ObjectId::from_num(1),
            LayerId::from_num(1),
            ClassId::from_num(0),
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 1.0,
            },
        )
    }

    #[test]
    fn defaults_follow_exact_and_clamped() {
        let ramp = |p: DVec2, which: Surface| {
            (p.x >= 0.0).then(|| p.x + if which == Surface::Proposed { 1.0 } else { 0.0 })
        };
        let o = object();
        assert_eq!(
            ramp.exact(None, DVec2::new(2.0, 0.0), Surface::Existing),
            Some(2.0)
        );
        assert_eq!(ramp.elevation(&o, DVec2::new(2.0, 0.0)), Some(3.0));
        assert!(ramp.covered(&o, DVec2::ZERO));
        assert!(!ramp.covered(&o, DVec2::new(-1.0, 0.0)));
        assert_eq!(ramp.elevation(&o, DVec2::new(-1.0, 0.0)), None);
        assert_eq!(NoTerrain.elevation(&o, DVec2::ZERO), None);
        assert!(!NoTerrain.covered(&o, DVec2::ZERO));
        assert_eq!(Surface::Existing.name(), "existing");
        assert_eq!(Surface::Proposed.name(), "proposed");
    }
}
