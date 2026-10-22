//! Guides (MOD-05): construction geometry of the 3D modeler. A guide is an object that makes no
//! geometry. It is a line (a point, a direction and, for a finite guide, a length) or a point.
//! The Tape Measure tool makes guides. The modeler draws them as thin dashed lines and small
//! crosses, and the inference snaps to them. The plan of the Vectorworks-like app does not draw
//! them, and exports leave them out with a counted note.
//!
//! A guide lives on a layer and in a class like any object, so the visibility of the layer and of
//! the class (the tags) applies to it. The numbers are in the coordinates of the layer: `z` is
//! measured from the elevation of the layer, as for surface models.

use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::model::{Document, MAX_COORD, Object, ObjectId, Shape};

/// The most guides that a document may hold. The inference looks at all guides on every pointer
/// move, and a file with more is a mistake (a script in a loop).
pub const MAX_GUIDES: usize = 10_000;

/// A guide line or a guide point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    /// A point of the guide, in the coordinates of its layer (mm).
    pub at: DVec3,
    /// The direction of a guide line. `None` makes a guide point. The length of the vector does not
    /// matter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<DVec3>,
    /// The length of a finite guide line, from `at` along `dir`. `None` is a line without ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
}

impl Guide {
    /// A guide point.
    pub fn point(at: DVec3) -> Guide {
        Guide {
            at,
            dir: None,
            length: None,
        }
    }

    /// A guide line without ends through `at`.
    pub fn line(at: DVec3, dir: DVec3) -> Guide {
        Guide {
            at,
            dir: Some(dir),
            length: None,
        }
    }

    /// A finite guide line from `at` along `dir`.
    pub fn segment(at: DVec3, dir: DVec3, length: f64) -> Guide {
        Guide {
            at,
            dir: Some(dir),
            length: Some(length),
        }
    }

    pub fn is_point(&self) -> bool {
        self.dir.is_none()
    }

    /// The unit direction of a line, `None` for a point or a direction without length.
    pub fn unit_dir(&self) -> Option<DVec3> {
        self.dir.and_then(DVec3::try_normalize)
    }

    /// The ends of a finite line, `None` for a point and for a line without ends.
    pub fn ends(&self) -> Option<[DVec3; 2]> {
        let (d, l) = (self.unit_dir()?, self.length?);
        Some([self.at, self.at + d * l])
    }

    /// The same guide moved by `d`.
    pub fn moved(&self, d: DVec3) -> Guide {
        Guide {
            at: self.at + d,
            ..self.clone()
        }
    }

    /// True when every number is finite.
    pub fn is_finite(&self) -> bool {
        self.at.is_finite()
            && self.dir.is_none_or(|d| d.is_finite())
            && self.length.is_none_or(f64::is_finite)
    }

    /// The reason a guide is refused, or `None`.
    pub fn problem(&self) -> Option<String> {
        if !self.is_finite() {
            return Some("a guide needs finite numbers".into());
        }
        if self.at.abs().max_element() > MAX_COORD {
            return Some("a guide is too far from the origin".into());
        }
        if let Some(d) = self.dir
            && d.try_normalize().is_none()
        {
            return Some("the direction of a guide needs a length".into());
        }
        match (self.dir, self.length) {
            (None, Some(_)) => Some("a guide point has no length".into()),
            (_, Some(l)) if l <= 0.0 || l > 2.0 * MAX_COORD => {
                Some("the length of a guide must be above zero".into())
            }
            _ => None,
        }
    }

    /// Every number of the guide, for the range check of shapes.
    pub(crate) fn numbers(&self, f: &mut dyn FnMut(f64)) {
        self.at.to_array().into_iter().for_each(&mut *f);
        if let Some(d) = self.dir {
            d.to_array().into_iter().for_each(&mut *f);
        }
        if let Some(l) = self.length {
            f(l);
        }
    }
}

/// True if the object is a guide.
pub fn is_guide(obj: &Object) -> bool {
    matches!(obj.shape, Shape::Guide(_))
}

impl Document {
    /// The guides of the document, in the order of their ids.
    pub fn guides(&self) -> impl Iterator<Item = (&Object, &Guide)> {
        self.objects.values().filter_map(|o| match &o.shape {
            Shape::Guide(g) => Some((o, &**g)),
            _ => None,
        })
    }

    /// The ids of all guides.
    pub fn guide_ids(&self) -> Vec<ObjectId> {
        self.guides().map(|(o, _)| o.id).collect()
    }

    /// How many guides the document has.
    pub fn guide_count(&self) -> usize {
        self.guides().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_guide_point_and_a_line_know_what_they_are() {
        let p = Guide::point(DVec3::new(1.0, 2.0, 3.0));
        assert!(p.is_point());
        assert_eq!(p.unit_dir(), None);
        assert_eq!(p.ends(), None);
        let l = Guide::line(DVec3::ZERO, DVec3::new(0.0, 2.0, 0.0));
        assert!(!l.is_point());
        assert_eq!(l.unit_dir(), Some(DVec3::Y));
        assert_eq!(l.ends(), None);
        let s = Guide::segment(DVec3::X, DVec3::Y, 5.0);
        assert_eq!(s.ends(), Some([DVec3::X, DVec3::new(1.0, 5.0, 0.0)]));
        assert_eq!(s.moved(DVec3::Z).at, DVec3::new(1.0, 0.0, 1.0));
    }

    #[test]
    fn bad_guides_have_a_reason() {
        assert!(Guide::point(DVec3::ZERO).problem().is_none());
        assert!(Guide::line(DVec3::ZERO, DVec3::X).problem().is_none());
        assert!(
            Guide::segment(DVec3::ZERO, DVec3::X, 1.0)
                .problem()
                .is_none()
        );
        for bad in [
            Guide::point(DVec3::NAN),
            Guide::point(DVec3::splat(2.0 * MAX_COORD)),
            Guide::line(DVec3::ZERO, DVec3::ZERO),
            Guide::line(DVec3::ZERO, DVec3::NAN),
            Guide::segment(DVec3::ZERO, DVec3::X, 0.0),
            Guide::segment(DVec3::ZERO, DVec3::X, -1.0),
            Guide::segment(DVec3::ZERO, DVec3::X, f64::NAN),
            Guide {
                at: DVec3::ZERO,
                dir: None,
                length: Some(1.0),
            },
        ] {
            assert!(bad.problem().is_some(), "{bad:?}");
        }
    }

    #[test]
    fn numbers_lists_every_number() {
        let mut n = Vec::new();
        Guide::segment(DVec3::new(1.0, 2.0, 3.0), DVec3::X, 4.0).numbers(&mut |x| n.push(x));
        assert_eq!(n, vec![1.0, 2.0, 3.0, 1.0, 0.0, 0.0, 4.0]);
    }
}
