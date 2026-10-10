//! Settings of the Massing Model tool.

use kadakas_core::massing::{Massing, RoofStyle};

/// Settings of the Massing Model tool.
#[derive(Clone, Debug, PartialEq)]
pub struct MassingToolSettings {
    /// True: the height is floors times the floor height. False: an eave height.
    pub by_floors: bool,
    pub height: f64,
    pub floors: u32,
    pub floor_height: f64,
    pub roof: RoofStyle,
    pub pitch_deg: f64,
}

impl Default for MassingToolSettings {
    fn default() -> Self {
        MassingToolSettings {
            by_floors: true,
            height: kadakas_core::massing::DEFAULT_HEIGHT,
            floors: 2,
            floor_height: kadakas_core::massing::DEFAULT_FLOOR_HEIGHT,
            roof: RoofStyle::Flat,
            pitch_deg: kadakas_core::massing::DEFAULT_PITCH_DEG,
        }
    }
}

impl MassingToolSettings {
    /// The massing model for a footprint drawn with the tool.
    pub fn massing(&self, footprint: Vec<kadakas_core::curve::Vertex>) -> Massing {
        let mut m = Massing::new(footprint, self.height.max(1.0));
        m.floors = self.by_floors.then_some(self.floors.max(1));
        m.floor_height = self.floor_height.max(1.0);
        m.roof = self.roof;
        m.pitch_deg = self
            .pitch_deg
            .clamp(0.0, kadakas_core::massing::MAX_PITCH_DEG);
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;
    use kadakas_core::curve::corners;

    #[test]
    fn tool_settings_make_a_valid_model() {
        let ring = corners(&[
            DVec2::ZERO,
            DVec2::new(5000.0, 0.0),
            DVec2::new(5000.0, 4000.0),
        ]);
        let mut s = MassingToolSettings::default();
        let m = s.massing(ring.clone());
        assert_eq!((m.floors, m.roof), (Some(2), RoofStyle::Flat));
        assert!(m.problem().is_none());
        s.by_floors = false;
        s.roof = RoofStyle::Hip;
        s.pitch_deg = 200.0;
        s.height = 0.0;
        let m = s.massing(ring);
        assert_eq!(
            (m.floors, m.pitch_deg),
            (None, kadakas_core::massing::MAX_PITCH_DEG)
        );
        assert!(m.height >= 1.0);
    }
}
