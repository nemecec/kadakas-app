//! The landscape area style: a plant mix with the 2D look of the area and data fields.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{Color, MAX_COORD};
use crate::resource::ResourceId;

/// Most plants in one mix.
pub const MAX_MIX: usize = 100;

fn hundred() -> f64 {
    100.0
}

/// One plant of the mix.
///
/// The plant covers `percent` of the area. Its count comes from the density: `density` plants
/// per m2, or one plant per `spacing` by `spacing` mm. Without either, the canopy diameter of
/// the plant style gives the spacing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MixEntry {
    /// The plant style (a resource).
    pub plant: ResourceId,
    #[serde(default = "hundred")]
    pub percent: f64,
    /// Distance between plants in mm.
    pub spacing: Option<f64>,
    /// Plants per m2.
    pub density: Option<f64>,
    /// The least number of plants, even in a small area.
    pub min_count: u32,
}

impl Default for MixEntry {
    fn default() -> Self {
        MixEntry {
            plant: ResourceId::from_num(0),
            percent: 100.0,
            spacing: None,
            density: None,
            min_count: 0,
        }
    }
}

impl MixEntry {
    /// Plants per m2, with `fallback_spacing` (the canopy diameter) when the entry gives neither.
    pub fn density_per_m2(&self, fallback_spacing: f64) -> f64 {
        match (self.density, self.spacing) {
            (Some(d), _) => d,
            (None, Some(s)) => 1.0e6 / (s * s),
            (None, None) => 1.0e6 / (fallback_spacing * fallback_spacing),
        }
    }
}

/// A landscape area style. The name is the name of the resource that holds it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LandscapeAreaStyle {
    pub mix: Vec<MixEntry>,
    /// Fill color of the area in 2D and 3D.
    pub fill: Option<Color>,
    /// Border color.
    pub pen: Option<Color>,
    /// Border line weight in mm on paper.
    pub line_weight: Option<f64>,
    /// Draw the plants of the mix in 2D, as scattered symbols.
    pub show_plants: bool,
    /// Data fields, for example `hydrozone` or `water need`.
    pub fields: BTreeMap<String, String>,
}

impl LandscapeAreaStyle {
    /// Checks the numbers and the rules of the mix. It does not check that the plant styles exist.
    pub fn validate(&self) -> Result<(), String> {
        if self.mix.len() > MAX_MIX {
            return Err(format!("a mix has at most {MAX_MIX} plants"));
        }
        let mut total = 0.0;
        for (i, e) in self.mix.iter().enumerate() {
            let n = i + 1;
            if !e.percent.is_finite() || e.percent <= 0.0 || e.percent > 100.0 {
                return Err(format!(
                    "percent of plant {n} must be more than 0 and at most 100"
                ));
            }
            total += e.percent;
            if e.spacing.is_some() && e.density.is_some() {
                return Err(format!("plant {n}: give a spacing or a density, not both"));
            }
            if e.spacing
                .is_some_and(|s| !s.is_finite() || s <= 0.0 || s > MAX_COORD)
            {
                return Err(format!(
                    "spacing of plant {n} must be a positive, finite number"
                ));
            }
            if e.density
                .is_some_and(|d| !d.is_finite() || d <= 0.0 || d > 1.0e6)
            {
                return Err(format!(
                    "density of plant {n} must be a positive, finite number (plants per m2)"
                ));
            }
            if self.mix[..i].iter().any(|o| o.plant == e.plant) {
                return Err(format!("plant {n} is in the mix twice"));
            }
        }
        if total > 100.0 + 1e-6 {
            return Err(format!(
                "the percentages add up to {total}. They must not be more than 100"
            ));
        }
        if self
            .line_weight
            .is_some_and(|w| !(0.0..=1000.0).contains(&w))
        {
            return Err("line weight must be a finite number of zero or more".into());
        }
        Ok(())
    }

    /// Sum of the percentages of the mix.
    pub fn total_percent(&self) -> f64 {
        self.mix.iter().map(|e| e.percent).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(plant: u64, percent: f64) -> MixEntry {
        MixEntry {
            plant: ResourceId(crate::uid::Uid::from_num(plant)),
            percent,
            ..Default::default()
        }
    }

    #[test]
    fn density_comes_from_density_spacing_or_fallback() {
        let mut e = entry(1, 50.0);
        assert!((e.density_per_m2(500.0) - 4.0).abs() < 1e-12);
        e.spacing = Some(250.0);
        assert!((e.density_per_m2(500.0) - 16.0).abs() < 1e-12);
        e.spacing = None;
        e.density = Some(7.5);
        assert_eq!(e.density_per_m2(500.0), 7.5);
    }

    #[test]
    fn validate_checks_the_mix() {
        let ok = LandscapeAreaStyle {
            mix: vec![entry(1, 60.0), entry(2, 40.0)],
            ..Default::default()
        };
        assert!(ok.validate().is_ok());
        assert_eq!(ok.total_percent(), 100.0);
        let bad = |f: &dyn Fn(&mut LandscapeAreaStyle)| {
            let mut s = ok.clone();
            f(&mut s);
            s.validate().unwrap_err()
        };
        assert!(bad(&|s| s.mix[0].percent = 0.0).contains("percent"));
        assert!(bad(&|s| s.mix[0].percent = f64::NAN).contains("percent"));
        assert!(bad(&|s| s.mix[0].percent = 120.0).contains("percent"));
        assert!(bad(&|s| s.mix[0].percent = 70.0).contains("add up"));
        assert!(bad(&|s| s.mix[1].plant = ResourceId::from_num(1)).contains("twice"));
        assert!(
            bad(&|s| {
                s.mix[0].spacing = Some(100.0);
                s.mix[0].density = Some(4.0);
            })
            .contains("not both")
        );
        assert!(bad(&|s| s.mix[0].spacing = Some(-1.0)).contains("spacing"));
        assert!(bad(&|s| s.mix[0].spacing = Some(f64::INFINITY)).contains("spacing"));
        assert!(bad(&|s| s.mix[0].density = Some(0.0)).contains("density"));
        assert!(bad(&|s| s.line_weight = Some(-1.0)).contains("line weight"));
        assert!(
            bad(&|s| s.mix = (0..=MAX_MIX as u64).map(|i| entry(i, 0.5)).collect())
                .contains("at most")
        );
    }

    #[test]
    fn partial_json_uses_defaults() {
        let s: LandscapeAreaStyle = serde_json::from_str(
            r#"{"mix": [{"plant": "00000000-0000-0000-0000-000000000007", "spacing": 300}], "fields": {"hydrozone": "low"}}"#,
        )
        .unwrap();
        assert_eq!(s.mix[0].percent, 100.0);
        assert_eq!(s.mix[0].min_count, 0);
        assert!(!s.show_plants);
        assert_eq!(s.fields["hydrozone"], "low");
    }
}
