//! The plant style: a reusable plant definition. Plants refer to a style and can override its fields.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::{Color, MAX_COORD};

/// The plant group, for filters and schedules.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PlantCategory {
    Tree,
    #[default]
    Shrub,
    Perennial,
    Grass,
    Groundcover,
}

impl PlantCategory {
    pub const ALL: [PlantCategory; 5] = [
        PlantCategory::Tree,
        PlantCategory::Shrub,
        PlantCategory::Perennial,
        PlantCategory::Grass,
        PlantCategory::Groundcover,
    ];

    pub fn id(self) -> &'static str {
        match self {
            PlantCategory::Tree => "tree",
            PlantCategory::Shrub => "shrub",
            PlantCategory::Perennial => "perennial",
            PlantCategory::Grass => "grass",
            PlantCategory::Groundcover => "groundcover",
        }
    }

    /// Reads an ID such as `tree`, in any case. Also accepts `ground cover` and plural forms.
    pub fn from_name(s: &str) -> Option<Self> {
        let key: String = s
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_ascii_lowercase();
        Self::ALL.into_iter().find(|c| {
            let id = c.id();
            key == id || key.strip_suffix('s') == Some(id) || key.strip_suffix("es") == Some(id)
        })
    }
}

/// The 2D outline that a plant draws in plan, generated from the spread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlantGraphic {
    /// A plain circle.
    #[default]
    Circle,
    /// A scalloped outline, like a cloud.
    Cloud,
    /// A star with sharp points.
    Star,
    /// A circle with spokes from the center.
    Spokes,
    /// A rosette of petals.
    Rosette,
    /// Three lobes around the center.
    Clover,
}

impl PlantGraphic {
    pub const ALL: [PlantGraphic; 6] = [
        PlantGraphic::Circle,
        PlantGraphic::Cloud,
        PlantGraphic::Star,
        PlantGraphic::Spokes,
        PlantGraphic::Rosette,
        PlantGraphic::Clover,
    ];

    pub fn id(self) -> &'static str {
        match self {
            PlantGraphic::Circle => "circle",
            PlantGraphic::Cloud => "cloud",
            PlantGraphic::Star => "star",
            PlantGraphic::Spokes => "spokes",
            PlantGraphic::Rosette => "rosette",
            PlantGraphic::Clover => "clover",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|g| g.id().eq_ignore_ascii_case(s.trim()))
    }
}

/// The 3D shape of a plant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlantForm {
    /// A trunk with a rounded canopy.
    #[default]
    Tree,
    /// A tall, narrow canopy that reaches near the ground.
    Columnar,
    /// A cone, wide at the base.
    Conical,
    /// A wide, low canopy on a short trunk.
    Spreading,
    /// A low dome that sits on the ground.
    ShrubMound,
    /// A dome with a skirt of branches that hang down.
    Weeping,
    /// A slender trunk with a crown of arching fronds.
    Palm,
    /// A narrow conifer with tiers.
    ColumnarConifer,
    /// Several stems that lean outward, each with a lobe of canopy.
    MultiStem,
    /// A clipped block. A plant makes a short block. A hedge path (a polyline) makes a long one.
    Hedge,
}

impl PlantForm {
    pub const ALL: [PlantForm; 10] = [
        PlantForm::Tree,
        PlantForm::Columnar,
        PlantForm::Conical,
        PlantForm::Spreading,
        PlantForm::ShrubMound,
        PlantForm::Weeping,
        PlantForm::Palm,
        PlantForm::ColumnarConifer,
        PlantForm::MultiStem,
        PlantForm::Hedge,
    ];

    pub fn id(self) -> &'static str {
        match self {
            PlantForm::Tree => "tree",
            PlantForm::Columnar => "columnar",
            PlantForm::Conical => "conical",
            PlantForm::Spreading => "spreading",
            PlantForm::ShrubMound => "shrub_mound",
            PlantForm::Weeping => "weeping",
            PlantForm::Palm => "palm",
            PlantForm::ColumnarConifer => "columnar_conifer",
            PlantForm::MultiStem => "multi_stem",
            PlantForm::Hedge => "hedge",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        let key = s.trim().replace([' ', '-'], "_");
        Self::ALL
            .into_iter()
            .find(|f| f.id().eq_ignore_ascii_case(&key))
    }
}

/// How the schedule counts a plant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuantityType {
    /// By number of plants. The price is per plant.
    #[default]
    Count,
    /// By covered area, for groundcovers. Each plant covers its canopy circle, and the price is per m2.
    Area,
}

impl QuantityType {
    pub fn id(self) -> &'static str {
        match self {
            QuantityType::Count => "count",
            QuantityType::Area => "area",
        }
    }
}

/// A plant style. The style name is the name of the resource that holds it.
/// Lengths are in mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlantStyle {
    pub botanical_name: String,
    pub common_name: String,
    /// The short code that plant tags and schedules show, for example `AC`.
    pub id_code: String,
    pub category: PlantCategory,
    /// Canopy diameter.
    pub spread: f64,
    pub height: f64,
    pub graphic: PlantGraphic,
    pub form: PlantForm,
    /// Fill color of the plan symbol and the 3D canopy. `None` uses the class color.
    pub fill: Option<Color>,
    /// Outline color of the plan symbol. `None` uses the class color.
    pub pen: Option<Color>,
    /// Price of one plant. The currency is up to the user.
    pub price: f64,
    /// Nursery size, for example `14-16 cm girth`.
    pub size: String,
    /// Container, for example `C10`.
    pub container: String,
    /// Root type, for example `bare root` or `balled`.
    pub root: String,
    /// Free fields, for example `water need`.
    pub custom: BTreeMap<String, String>,
    /// Fields that plants cannot override (by style only). The other fields can be set per plant (by instance).
    pub locked: Vec<String>,
    /// Count plants, or add up their area (groundcovers).
    pub quantity: QuantityType,
    /// Trunk diameter in mm. Needed for the tree protection zone.
    pub trunk_diameter: f64,
    /// Draw a tree protection zone (category tree only). The radius follows the rule of existing trees.
    pub protection_zone: bool,
    /// Zone radius as a multiple of the trunk diameter (default 12).
    pub rpz_factor: Option<f64>,
    /// Largest zone radius in mm (default 15 000).
    pub rpz_cap: Option<f64>,
    /// A symbol resource that draws the plant in plan, scaled to the canopy. `None` uses `graphic`.
    pub plan_symbol: Option<crate::resource::ResourceId>,
}

impl Default for PlantStyle {
    fn default() -> Self {
        PlantStyle {
            botanical_name: String::new(),
            common_name: String::new(),
            id_code: String::new(),
            category: PlantCategory::default(),
            spread: 3000.0,
            height: 4000.0,
            graphic: PlantGraphic::default(),
            form: PlantForm::default(),
            fill: None,
            pen: None,
            price: 0.0,
            size: String::new(),
            container: String::new(),
            root: String::new(),
            custom: BTreeMap::new(),
            locked: Vec::new(),
            quantity: QuantityType::default(),
            trunk_diameter: 0.0,
            protection_zone: false,
            rpz_factor: None,
            rpz_cap: None,
            plan_symbol: None,
        }
    }
}

impl PlantStyle {
    /// Checks the numbers: finite, in range, and a positive spread and height.
    pub fn validate(&self) -> Result<(), String> {
        for (name, v) in [("spread", self.spread), ("height", self.height)] {
            if !v.is_finite() || v <= 0.0 {
                return Err(format!("{name} must be a positive, finite number"));
            }
            if v > MAX_COORD {
                return Err(format!("{name} must not be larger than 1e9 mm"));
            }
        }
        if !self.price.is_finite() || self.price < 0.0 || self.price > MAX_COORD {
            return Err("price must be a finite number of zero or more".into());
        }
        for (name, v) in [
            ("trunk_diameter", Some(self.trunk_diameter)),
            ("rpz_factor", self.rpz_factor),
            ("rpz_cap", self.rpz_cap),
        ] {
            if v.is_some_and(|v| !v.is_finite() || !(0.0..=MAX_COORD).contains(&v)) {
                return Err(format!("{name} must be a finite number of zero or more"));
            }
        }
        let names = Self::field_names();
        if let Some(bad) = self
            .locked
            .iter()
            .find(|f| !names.contains(f) || *f == "locked")
        {
            return Err(format!(
                "'{bad}' cannot be locked. Fields: {}",
                names.join(", ")
            ));
        }
        Ok(())
    }

    /// The names of all fields, as used in JSON, overrides and `locked`.
    pub fn field_names() -> Vec<String> {
        serde_json::to_value(PlantStyle::default())
            .expect("a style is JSON")
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect()
    }

    /// True if plants cannot override this field.
    pub fn is_locked(&self, field: &str) -> bool {
        self.locked.iter().any(|f| f == field)
    }

    /// Radius of the tree protection zone in mm, for a tree with a trunk diameter and the zone switched on.
    pub fn protection_radius(&self) -> Option<f64> {
        (self.category == PlantCategory::Tree && self.protection_zone && self.trunk_diameter > 0.0)
            .then(|| super::tree::rpz_radius(self.trunk_diameter, self.rpz_factor, self.rpz_cap))
    }

    /// The name that a plant object shows as its species: the botanical name, else the common name.
    pub fn species(&self, fallback: &str) -> String {
        [&self.botanical_name, &self.common_name]
            .into_iter()
            .find(|s| !s.trim().is_empty())
            .map_or_else(|| fallback.to_string(), |s| s.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_round_trip_by_name() {
        for c in PlantCategory::ALL {
            assert_eq!(PlantCategory::from_name(c.id()), Some(c));
            assert_eq!(serde_json::to_value(c).unwrap(), c.id());
        }
        assert_eq!(
            PlantCategory::from_name("Ground Cover"),
            Some(PlantCategory::Groundcover)
        );
        assert_eq!(PlantCategory::from_name("Trees"), Some(PlantCategory::Tree));
        assert_eq!(
            PlantCategory::from_name("Grasses"),
            Some(PlantCategory::Grass)
        );
        assert_eq!(PlantCategory::from_name("cactus"), None);
        for g in PlantGraphic::ALL {
            assert_eq!(PlantGraphic::from_name(g.id()), Some(g));
            assert_eq!(serde_json::to_value(g).unwrap(), g.id());
        }
        assert_eq!(PlantGraphic::from_name(" STAR "), Some(PlantGraphic::Star));
        assert_eq!(PlantGraphic::from_name("x"), None);
        for f in PlantForm::ALL {
            assert_eq!(PlantForm::from_name(f.id()), Some(f));
            assert_eq!(serde_json::to_value(f).unwrap(), f.id());
        }
        assert_eq!(
            PlantForm::from_name("Shrub mound"),
            Some(PlantForm::ShrubMound)
        );
        assert_eq!(PlantForm::from_name("x"), None);
    }

    #[test]
    fn validate_numbers() {
        assert!(PlantStyle::default().validate().is_ok());
        let bad = |f: &dyn Fn(&mut PlantStyle)| {
            let mut s = PlantStyle::default();
            f(&mut s);
            s.validate().is_err()
        };
        assert!(bad(&|s| s.spread = 0.0));
        assert!(bad(&|s| s.spread = f64::INFINITY));
        assert!(bad(&|s| s.height = -1.0));
        assert!(bad(&|s| s.height = 2.0 * MAX_COORD));
        assert!(bad(&|s| s.price = -0.5));
        assert!(bad(&|s| s.price = f64::NAN));
    }

    #[test]
    fn partial_json_uses_defaults() {
        let s: PlantStyle =
            serde_json::from_str(r#"{"botanical_name": "Acer", "spread": 5000}"#).unwrap();
        assert_eq!(s.spread, 5000.0);
        assert_eq!(s.height, 4000.0);
        assert_eq!(s.species("x"), "Acer");
    }

    #[test]
    fn locked_fields_and_zone_settings_are_checked() {
        let mut s = PlantStyle {
            locked: vec!["price".into(), "spread".into()],
            ..PlantStyle::default()
        };
        assert!(s.validate().is_ok());
        assert!(s.is_locked("price") && !s.is_locked("height"));
        s.locked.push("nope".into());
        assert!(s.validate().unwrap_err().contains("cannot be locked"));
        s.locked = vec!["locked".into()];
        assert!(s.validate().is_err());
        let mut t = PlantStyle {
            trunk_diameter: -1.0,
            ..PlantStyle::default()
        };
        assert!(t.validate().is_err());
        t = PlantStyle {
            rpz_factor: Some(f64::NAN),
            ..PlantStyle::default()
        };
        assert!(t.validate().is_err());
        t = PlantStyle {
            rpz_cap: Some(-5.0),
            ..PlantStyle::default()
        };
        assert!(t.validate().is_err());
        assert!(PlantStyle::field_names().contains(&"locked".to_string()));
    }

    #[test]
    fn protection_zone_needs_a_tree_a_switch_and_a_trunk() {
        let mut s = PlantStyle {
            category: PlantCategory::Tree,
            trunk_diameter: 400.0,
            ..PlantStyle::default()
        };
        assert_eq!(s.protection_radius(), None, "off by default");
        s.protection_zone = true;
        assert_eq!(s.protection_radius(), Some(4800.0));
        s.trunk_diameter = 1500.0;
        assert_eq!(
            s.protection_radius(),
            Some(15_000.0),
            "capped like existing trees"
        );
        s.rpz_cap = Some(20_000.0);
        s.rpz_factor = Some(10.0);
        assert_eq!(s.protection_radius(), Some(15_000.0));
        s.category = PlantCategory::Shrub;
        assert_eq!(s.protection_radius(), None);
        s.category = PlantCategory::Tree;
        s.trunk_diameter = 0.0;
        assert_eq!(s.protection_radius(), None);
        assert_eq!(QuantityType::Area.id(), "area");
        assert_eq!(QuantityType::Count.id(), "count");
    }

    #[test]
    fn species_falls_back() {
        let mut s = PlantStyle::default();
        assert_eq!(s.species("Style A"), "Style A");
        s.common_name = "Maple".into();
        assert_eq!(s.species("Style A"), "Maple");
        s.botanical_name = "Acer".into();
        assert_eq!(s.species("Style A"), "Acer");
    }
}
