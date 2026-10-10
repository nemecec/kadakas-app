//! Section lines and the section view of a viewport: the settings only. The cut through the 3D
//! model is made by the `cadektor-x3d` crate.
//!
//! A section line is a `Shape::Line` on a design layer with settings under `Object::data["section_line"]`.
//! The line gives the cut plane (it is vertical and runs along the line). The view looks to one
//! side of the line. A viewport with a `SectionView` shows the cut through the 3D objects, and
//! the objects beyond it as lines. A line in `elevation` mode cuts nothing: the viewport shows
//! the model seen from the line, up to the far clip.

use glam::DVec2;
use serde::{Deserialize, Serialize};

use crate::model::{Object, ObjectId, Shape};

/// The key in `Object::data` that holds the settings of a section line.
pub const KEY: &str = "section_line";

/// The largest far clip, in mm (1000 km).
pub const MAX_DEPTH: f64 = 1.0e9;

/// Which side of the line the view looks to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Look {
    /// To the left of the line, going from its first point to its second.
    #[default]
    Left,
    Right,
}

impl Look {
    pub fn id(self) -> &'static str {
        match self {
            Look::Left => "left",
            Look::Right => "right",
        }
    }
}

/// What a section line does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Cut through the objects. Cut faces and the objects beyond.
    #[default]
    Cut,
    /// No cut: the model seen from the line (an elevation).
    Elevation,
}

impl Mode {
    pub fn id(self) -> &'static str {
        match self {
            Mode::Cut => "cut",
            Mode::Elevation => "elevation",
        }
    }
}

/// The settings of a section line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SectionLine {
    pub name: String,
    pub look: Look,
    pub mode: Mode,
    /// Far clip: objects farther than this from the line, in the direction of the view, are not
    /// drawn. `None` draws everything beyond.
    pub depth: Option<f64>,
    /// Draw the objects beyond the cut (cut mode).
    pub beyond: bool,
}

impl Default for SectionLine {
    fn default() -> Self {
        SectionLine {
            name: "A".into(),
            look: Look::Left,
            mode: Mode::Cut,
            depth: None,
            beyond: true,
        }
    }
}

impl SectionLine {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(d) = self.depth
            && !(d.is_finite() && d > 0.0 && d <= MAX_DEPTH)
        {
            return Err("depth must be more than 0 and at most 1e9 mm".into());
        }
        if self.name.chars().count() > 200 {
            return Err("the name is too long".into());
        }
        Ok(())
    }
}

/// The settings of a section line object, or `None` if the object is not one.
pub fn data(obj: &Object) -> Option<SectionLine> {
    if !matches!(obj.shape, Shape::Line { .. }) {
        return None;
    }
    serde_json::from_value(obj.data.get(KEY)?.clone()).ok()
}

/// The two ends of a section line object.
pub fn ends(obj: &Object) -> Option<(DVec2, DVec2)> {
    match obj.shape {
        Shape::Line { a, b } if data(obj).is_some() => Some((a, b)),
        _ => None,
    }
}

/// True if the object is a section line.
pub fn is_section_line(obj: &Object) -> bool {
    obj.data.contains_key(KEY) && matches!(obj.shape, Shape::Line { .. })
}

/// What a viewport shows when it shows a section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionView {
    /// The section line object. The view follows it when it moves.
    pub line: ObjectId,
    /// Line weight of the cut (outline of the cut faces and the profile), in mm on paper.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut_weight: Option<f64>,
    /// Line weight of the objects beyond the cut, in mm on paper.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beyond_weight: Option<f64>,
}

/// Line weight of the cut when the view does not say, in mm on paper.
pub const DEFAULT_CUT_WEIGHT: f64 = 0.5;
/// Line weight of the objects beyond the cut when the view does not say, in mm on paper.
pub const DEFAULT_BEYOND_WEIGHT: f64 = 0.15;

impl SectionView {
    pub fn new(line: ObjectId) -> SectionView {
        SectionView {
            line,
            cut_weight: None,
            beyond_weight: None,
        }
    }

    pub fn is_finite(&self) -> bool {
        self.cut_weight.is_none_or(|w| w.is_finite() && w >= 0.0)
            && self.beyond_weight.is_none_or(|w| w.is_finite() && w >= 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClassId, LayerId};

    fn line_with(data: serde_json::Value) -> Object {
        let mut o = Object::new(
            ObjectId(1),
            LayerId(2),
            ClassId(3),
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::new(1000.0, 0.0),
            },
        );
        o.data.insert(KEY.into(), data);
        o
    }

    #[test]
    fn reads_the_settings() {
        let o = line_with(
            serde_json::json!({"name": "B", "look": "right", "mode": "elevation", "depth": 5000.0}),
        );
        let d = data(&o).unwrap();
        assert_eq!(d.name, "B");
        assert_eq!(d.look, Look::Right);
        assert_eq!(d.mode, Mode::Elevation);
        assert_eq!(d.depth, Some(5000.0));
        assert!(d.beyond);
        assert!(is_section_line(&o));
        assert_eq!(ends(&o), Some((DVec2::ZERO, DVec2::new(1000.0, 0.0))));
        assert_eq!(d.look.id(), "right");
        assert_eq!(d.mode.id(), "elevation");
    }

    #[test]
    fn plain_objects_are_not_section_lines() {
        let mut o = line_with(serde_json::json!({}));
        assert!(data(&o).is_some());
        o.data.clear();
        assert!(data(&o).is_none());
        assert!(!is_section_line(&o));
        assert!(ends(&o).is_none());
        let bad = line_with(serde_json::json!({"look": "up"}));
        assert!(data(&bad).is_none());
        let mut circle = line_with(serde_json::json!({}));
        circle.shape = Shape::Circle {
            center: DVec2::ZERO,
            radius: 1.0,
        };
        assert!(data(&circle).is_none());
        assert!(!is_section_line(&circle));
    }

    #[test]
    fn validates_the_depth() {
        assert!(SectionLine::default().validate().is_ok());
        for d in [0.0, -1.0, f64::NAN, f64::INFINITY, 2.0e9] {
            let s = SectionLine {
                depth: Some(d),
                ..SectionLine::default()
            };
            assert!(s.validate().is_err(), "{d}");
        }
        let long = SectionLine {
            name: "x".repeat(201),
            ..SectionLine::default()
        };
        assert!(long.validate().is_err());
    }
}
