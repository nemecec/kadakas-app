//! Settings of the Hardscape tool and the Landscape Area tool, and the commands they make.

use kadakas_core::command::Command;
use kadakas_core::hardscape::{HardscapePatch, Join};
use kadakas_core::landscape::LandscapePatch;
use kadakas_core::resource::ResourceId;

/// Settings of the Hardscape tool and the Landscape Area tool.
#[derive(Clone, Debug, PartialEq)]
pub struct AreaToolSettings {
    /// Hardscape style of new hardscapes. `None`: a plain slab with the material of the Data bar.
    pub hardscape_style: Option<ResourceId>,
    /// Draw a path (a center line with a width) instead of a slab.
    pub path: bool,
    pub width: f64,
    pub join: Join,
    /// Landscape area style of new areas.
    pub landscape_style: Option<ResourceId>,
}

impl Default for AreaToolSettings {
    fn default() -> Self {
        AreaToolSettings {
            hardscape_style: None,
            path: false,
            width: 1200.0,
            join: Join::Miter,
            landscape_style: None,
        }
    }
}

impl AreaToolSettings {
    /// The command that adds a hardscape drawn with these settings. `None` means: add the plain
    /// shape, as before hardscape styles.
    pub fn hardscape_command(&self, points: &[kadakas_core::curve::Vertex]) -> Option<Command> {
        if !self.path && self.hardscape_style.is_none() {
            return None;
        }
        let mut patch = HardscapePatch {
            style: self.hardscape_style,
            ..Default::default()
        };
        let boundary = if self.path {
            patch.center = Some(points.to_vec());
            patch.width = Some(self.width);
            patch.join = Some(self.join);
            None
        } else {
            Some(points.to_vec())
        };
        Some(Command::AddHardscape {
            boundary,
            patch: Box::new(patch),
            layer: None,
            class: None,
            attrs: Default::default(),
        })
    }

    /// The command that adds a landscape area drawn with these settings, or `None` for a plain area.
    pub fn landscape_command(
        &self,
        points: &[kadakas_core::curve::Vertex],
        name: &str,
    ) -> Option<Command> {
        let style = self.landscape_style?;
        let patch = LandscapePatch {
            style: Some(style),
            ..Default::default()
        };
        Some(Command::AddLandscapeArea {
            boundary: points.to_vec(),
            name: name.into(),
            patch: Box::new(patch),
            layer: None,
            class: None,
            attrs: Default::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;
    use kadakas_core::curve::corners;

    fn square() -> Vec<kadakas_core::curve::Vertex> {
        corners(&[
            DVec2::ZERO,
            DVec2::new(1000.0, 0.0),
            DVec2::new(1000.0, 1000.0),
        ])
    }

    #[test]
    fn a_plain_slab_makes_no_styled_command() {
        assert!(
            AreaToolSettings::default()
                .hardscape_command(&square())
                .is_none()
        );
        assert!(
            AreaToolSettings::default()
                .landscape_command(&square(), "A")
                .is_none()
        );
    }

    #[test]
    fn a_style_or_a_path_makes_a_command() {
        let styled = AreaToolSettings {
            hardscape_style: Some(ResourceId::from_num(7)),
            ..Default::default()
        };
        match styled.hardscape_command(&square()) {
            Some(Command::AddHardscape {
                boundary, patch, ..
            }) => {
                assert_eq!(boundary.map(|b| b.len()), Some(3));
                assert_eq!(patch.style, Some(ResourceId::from_num(7)));
                assert!(patch.center.is_none());
            }
            other => panic!("{other:?}"),
        }
        let path = AreaToolSettings {
            path: true,
            width: 900.0,
            ..Default::default()
        };
        match path.hardscape_command(&square()) {
            Some(Command::AddHardscape {
                boundary, patch, ..
            }) => {
                assert!(boundary.is_none());
                assert_eq!(patch.width, Some(900.0));
                assert_eq!(patch.center.map(|c| c.len()), Some(3));
                assert_eq!(patch.join, Some(Join::Miter));
            }
            other => panic!("{other:?}"),
        }
        let area = AreaToolSettings {
            landscape_style: Some(ResourceId::from_num(9)),
            ..Default::default()
        };
        match area.landscape_command(&square(), "Lawn") {
            Some(Command::AddLandscapeArea {
                boundary,
                name,
                patch,
                ..
            }) => {
                assert_eq!((boundary.len(), name.as_str()), (3, "Lawn"));
                assert_eq!(patch.style, Some(ResourceId::from_num(9)));
            }
            other => panic!("{other:?}"),
        }
    }
}
