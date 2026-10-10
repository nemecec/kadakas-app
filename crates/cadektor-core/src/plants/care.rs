//! Care fields of a plant style: maintenance and irrigation, and the hydrozone report.
//!
//! The irrigation fields follow the public water use classes of WUCOLS (very low, low, moderate,
//! high) and the landscape water budget idea: the water a planting needs in a year is the
//! reference evapotranspiration times a plant factor times the area, in liters per m2 and mm.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::link;
use crate::model::{Document, Shape};

/// How much water a plant needs. The classes follow the public WUCOLS categories.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum WaterNeed {
    VeryLow,
    Low,
    #[default]
    Moderate,
    High,
}

impl WaterNeed {
    pub const ALL: [WaterNeed; 4] = [
        WaterNeed::VeryLow,
        WaterNeed::Low,
        WaterNeed::Moderate,
        WaterNeed::High,
    ];

    pub fn id(self) -> &'static str {
        match self {
            WaterNeed::VeryLow => "very_low",
            WaterNeed::Low => "low",
            WaterNeed::Moderate => "moderate",
            WaterNeed::High => "high",
        }
    }

    /// The plant factor of the class: the middle of the public WUCOLS range.
    pub fn plant_factor(self) -> f64 {
        match self {
            WaterNeed::VeryLow => 0.05,
            WaterNeed::Low => 0.2,
            WaterNeed::Moderate => 0.5,
            WaterNeed::High => 0.8,
        }
    }

    pub fn from_name(s: &str) -> Option<WaterNeed> {
        let key = s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        match key.as_str() {
            "medium" | "med" | "mod" | "average" => return Some(WaterNeed::Moderate),
            "verylow" | "vlow" => return Some(WaterNeed::VeryLow),
            _ => {}
        }
        Self::ALL.into_iter().find(|w| w.id() == key)
    }
}

/// Irrigation fields of a plant style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IrrigationInfo {
    pub water_need: WaterNeed,
    /// The name of the hydrozone: plants with the same need that one irrigation zone waters.
    /// A name such as `Z3`, `3` or `Zone 3` is matched with irrigation zone 3.
    pub hydrozone: String,
    /// Overrides the plant factor of the water need class.
    pub plant_factor: Option<f64>,
    /// False for plants that the project does not irrigate (established natives, rain fed).
    pub irrigated: bool,
}

impl Default for IrrigationInfo {
    /// Moderate water need, no hydrozone, irrigated.
    fn default() -> Self {
        IrrigationInfo {
            water_need: WaterNeed::default(),
            hydrozone: String::new(),
            plant_factor: None,
            irrigated: true,
        }
    }
}

impl IrrigationInfo {
    /// The plant factor in use: the override, else the class value.
    pub fn factor(&self) -> f64 {
        self.plant_factor
            .unwrap_or_else(|| self.water_need.plant_factor())
    }

    pub fn is_valid(&self) -> Result<(), String> {
        if self
            .plant_factor
            .is_some_and(|f| !f.is_finite() || !(0.0..=3.0).contains(&f))
        {
            return Err("plant_factor must be a number from 0 to 3".into());
        }
        Ok(())
    }
}

/// Maintenance fields of a plant style.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaintenanceInfo {
    /// How often the plant is looked after, for example `monthly`.
    pub interval: String,
    pub pruning: String,
    pub fertilizer: String,
    /// Hours of work per plant and year.
    pub hours_per_year: f64,
    pub notes: String,
}

impl MaintenanceInfo {
    pub fn is_valid(&self) -> Result<(), String> {
        if !self.hours_per_year.is_finite() || !(0.0..=100_000.0).contains(&self.hours_per_year) {
            return Err("hours_per_year must be a number from 0 to 100000".into());
        }
        Ok(())
    }
}

/// The number part of a hydrozone name, to match an irrigation zone: `Z3` and `Zone 3` give 3.
pub fn zone_number(hydrozone: &str) -> Option<u32> {
    let digits: String = hydrozone
        .trim()
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    // Only a name that is a zone: letters first, then the number, nothing after it.
    let rest = hydrozone
        .trim()
        .trim_start_matches(|c: char| c.is_ascii_alphabetic() || c == ' ' || c == '-' || c == '_')
        .trim_start_matches(|c: char| c.is_ascii_digit());
    if digits.is_empty() || !rest.trim().is_empty() {
        return None;
    }
    digits.parse().ok().filter(|n| *n > 0)
}

/// One hydrozone of the plantings.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HydrozoneRow {
    /// The hydrozone name. Plants with none are listed under an empty name.
    pub hydrozone: String,
    pub plants: usize,
    /// Plants of each water need class.
    pub by_need: BTreeMap<String, usize>,
    /// Canopy area of the irrigated plants in m2 (circles; overlaps count twice).
    pub canopy_area_m2: f64,
    /// Average plant factor, weighted by canopy area.
    pub plant_factor: f64,
    /// Water the plants need in a year, in liters, for the reference evapotranspiration given.
    pub annual_water_l: f64,
    /// More than one step between the thirstiest and the driest plant: the zone is mixed.
    pub mixed: bool,
    /// Plants that are not irrigated.
    pub not_irrigated: usize,
}

/// Groups the plant objects by hydrozone. `eto_mm_year` is the reference evapotranspiration of
/// the site per year (1 mm over 1 m2 is 1 liter).
pub fn hydrozones(doc: &Document, eto_mm_year: f64) -> Vec<HydrozoneRow> {
    struct Acc {
        plants: usize,
        by_need: BTreeMap<WaterNeed, usize>,
        area: f64,
        weighted: f64,
        not_irrigated: usize,
    }
    let mut groups: BTreeMap<String, Acc> = BTreeMap::new();
    for o in doc.objects.values() {
        let Shape::Plant { spread, .. } = &o.shape else {
            continue;
        };
        let Some(e) = link::effective(doc, o) else {
            continue;
        };
        let irr = &e.style.irrigation;
        let acc = groups
            .entry(irr.hydrozone.trim().to_string())
            .or_insert(Acc {
                plants: 0,
                by_need: BTreeMap::new(),
                area: 0.0,
                weighted: 0.0,
                not_irrigated: 0,
            });
        acc.plants += 1;
        *acc.by_need.entry(irr.water_need).or_default() += 1;
        if !irr.irrigated {
            acc.not_irrigated += 1;
            continue;
        }
        let r = spread / 2000.0;
        let area = std::f64::consts::PI * r * r;
        acc.area += area;
        acc.weighted += area * irr.factor();
    }
    groups
        .into_iter()
        .map(|(name, a)| {
            let factor = if a.area > 0.0 {
                a.weighted / a.area
            } else {
                0.0
            };
            let lo = a.by_need.keys().next().copied().unwrap_or_default();
            let hi = a.by_need.keys().next_back().copied().unwrap_or_default();
            HydrozoneRow {
                hydrozone: name,
                plants: a.plants,
                by_need: a
                    .by_need
                    .iter()
                    .map(|(k, n)| (k.id().to_string(), *n))
                    .collect(),
                canopy_area_m2: a.area,
                plant_factor: factor,
                annual_water_l: if eto_mm_year.is_finite() && eto_mm_year > 0.0 {
                    a.weighted * eto_mm_year
                } else {
                    0.0
                },
                mixed: (hi as i32 - lo as i32) > 1,
                not_irrigated: a.not_irrigated,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Editor};
    use crate::plants::style::PlantStyle;
    use crate::resource::ResourceBody;
    use glam::DVec2;

    #[test]
    fn classes_have_ordered_factors_and_names() {
        let f: Vec<f64> = WaterNeed::ALL.iter().map(|w| w.plant_factor()).collect();
        assert!(f.windows(2).all(|w| w[0] < w[1]));
        for w in WaterNeed::ALL {
            assert_eq!(WaterNeed::from_name(w.id()), Some(w));
        }
        assert_eq!(WaterNeed::from_name("Very Low"), Some(WaterNeed::VeryLow));
        assert_eq!(WaterNeed::from_name("Medium"), Some(WaterNeed::Moderate));
        assert_eq!(WaterNeed::from_name("x"), None);
        assert_eq!(WaterNeed::default(), WaterNeed::Moderate);
    }

    #[test]
    fn the_factor_override_and_checks() {
        let mut i = IrrigationInfo::default();
        assert!(i.irrigated);
        assert_eq!(i.factor(), 0.5);
        i.plant_factor = Some(0.3);
        assert_eq!(i.factor(), 0.3);
        assert!(i.is_valid().is_ok());
        i.plant_factor = Some(-1.0);
        assert!(i.is_valid().is_err());
        i.plant_factor = Some(f64::NAN);
        assert!(i.is_valid().is_err());
        let mut m = MaintenanceInfo::default();
        assert!(m.is_valid().is_ok());
        m.hours_per_year = f64::INFINITY;
        assert!(m.is_valid().is_err());
        m.hours_per_year = -1.0;
        assert!(m.is_valid().is_err());
    }

    #[test]
    fn zone_numbers_come_from_names() {
        assert_eq!(zone_number("Z3"), Some(3));
        assert_eq!(zone_number("3"), Some(3));
        assert_eq!(zone_number("Zone 12"), Some(12));
        assert_eq!(zone_number("zone-4"), Some(4));
        assert_eq!(zone_number("North beds"), None);
        assert_eq!(zone_number("Z3 north"), None);
        assert_eq!(zone_number("0"), None);
        assert_eq!(zone_number(""), None);
    }

    fn style(
        ed: &mut Editor,
        name: &str,
        need: WaterNeed,
        zone: &str,
        irrigated: bool,
    ) -> crate::resource::ResourceId {
        let s = PlantStyle {
            spread: 2000.0,
            irrigation: IrrigationInfo {
                water_need: need,
                hydrozone: zone.into(),
                plant_factor: None,
                irrigated,
            },
            ..PlantStyle::default()
        };
        let out = ed
            .execute(&Command::AddResource {
                name: name.into(),
                body: ResourceBody::plant_style(s),
            })
            .unwrap();
        crate::resource::ResourceId(out.created[0])
    }

    #[test]
    fn plants_group_by_hydrozone_with_a_water_budget() {
        let mut ed = Editor::new(Document::default());
        let a = style(&mut ed, "Lavender", WaterNeed::Low, "Z1", true);
        let b = style(&mut ed, "Hydrangea", WaterNeed::High, "Z1", true);
        let c = style(&mut ed, "Oak", WaterNeed::VeryLow, "", false);
        for (i, s) in [a, a, b, c].into_iter().enumerate() {
            ed.execute(&Command::AddPlants {
                style: Some(s),
                overrides: serde_json::Map::new(),
                positions: vec![DVec2::new(i as f64 * 3000.0, 0.0)],
                layer: None,
                class: None,
                variation: None,
            })
            .unwrap();
        }
        let rows = hydrozones(ed.doc(), 1000.0);
        assert_eq!(rows.len(), 2);
        let none = &rows[0];
        assert_eq!(
            (none.hydrozone.as_str(), none.plants, none.not_irrigated),
            ("", 1, 1)
        );
        assert_eq!(none.annual_water_l, 0.0);
        let z1 = &rows[1];
        assert_eq!((z1.hydrozone.as_str(), z1.plants), ("Z1", 3));
        assert!(z1.mixed, "low and high are two steps apart");
        let area = std::f64::consts::PI;
        assert!((z1.canopy_area_m2 - 3.0 * area).abs() < 1e-9);
        let expect = (2.0 * 0.2 + 0.8) / 3.0;
        assert!((z1.plant_factor - expect).abs() < 1e-9);
        assert!((z1.annual_water_l - 3.0 * area * expect * 1000.0).abs() < 1e-6);
        assert_eq!(z1.by_need["low"], 2);
        assert_eq!(hydrozones(ed.doc(), f64::NAN)[1].annual_water_l, 0.0);
    }
}
