//! Units and axes of a mesh file, and the change to the model (millimeters, Z up).

use serde::{Deserialize, Serialize};

/// The length unit of the numbers in a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    #[serde(rename = "mm")]
    Millimetre,
    #[serde(rename = "cm")]
    Centimetre,
    #[serde(rename = "m")]
    Metre,
    #[serde(rename = "km")]
    Kilometre,
    Inch,
    Foot,
    Yard,
    Mile,
}

impl Unit {
    pub const ALL: [Unit; 8] = [
        Unit::Millimetre,
        Unit::Centimetre,
        Unit::Metre,
        Unit::Kilometre,
        Unit::Inch,
        Unit::Foot,
        Unit::Yard,
        Unit::Mile,
    ];

    /// Millimeters in one unit.
    pub fn mm(self) -> f64 {
        match self {
            Unit::Millimetre => 1.0,
            Unit::Centimetre => 10.0,
            Unit::Metre => 1000.0,
            Unit::Kilometre => 1.0e6,
            Unit::Inch => 25.4,
            Unit::Foot => 304.8,
            Unit::Yard => 914.4,
            Unit::Mile => 1_609_344.0,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Unit::Millimetre => "mm",
            Unit::Centimetre => "cm",
            Unit::Metre => "m",
            Unit::Kilometre => "km",
            Unit::Inch => "inch",
            Unit::Foot => "foot",
            Unit::Yard => "yard",
            Unit::Mile => "mile",
        }
    }

    pub fn from_name(s: &str) -> Option<Unit> {
        let s = s.trim().to_ascii_lowercase();
        Unit::ALL
            .into_iter()
            .find(|u| u.id() == s)
            .or(match s.as_str() {
                "millimeter" | "millimetre" => Some(Unit::Millimetre),
                "centimeter" | "centimetre" => Some(Unit::Centimetre),
                "meter" | "metre" | "metres" | "meters" => Some(Unit::Metre),
                "kilometer" | "kilometre" => Some(Unit::Kilometre),
                "in" | "inches" => Some(Unit::Inch),
                "ft" | "feet" => Some(Unit::Foot),
                "yd" => Some(Unit::Yard),
                "mi" => Some(Unit::Mile),
                _ => None,
            })
    }
}

/// The axis that points up in a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Up {
    #[serde(rename = "y")]
    Y,
    #[serde(rename = "z")]
    Z,
}

impl Up {
    pub fn id(self) -> &'static str {
        match self {
            Up::Y => "y",
            Up::Z => "z",
        }
    }

    pub fn from_name(s: &str) -> Option<Up> {
        match s.trim().to_ascii_lowercase().as_str() {
            "y" | "y_up" => Some(Up::Y),
            "z" | "z_up" => Some(Up::Z),
            _ => None,
        }
    }
}

/// The unit and the up axis of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Frame {
    pub unit: Unit,
    pub up: Up,
}

impl Frame {
    /// A point of the file as a point of the model: millimeters, Z up. A Y up file has its
    /// forward axis on negative Z, so (x, y, z) becomes (x, -z, y), the inverse of the glTF
    /// export of Kadakas.
    pub fn to_model(self, p: [f64; 3]) -> [f64; 3] {
        let k = self.unit.mm();
        match self.up {
            Up::Z => [p[0] * k, p[1] * k, p[2] * k],
            Up::Y => [p[0] * k, -p[2] * k, p[1] * k],
        }
    }

    /// A point of the model as a point of the file.
    pub fn from_model(self, p: [f64; 3]) -> [f64; 3] {
        let k = 1.0 / self.unit.mm();
        match self.up {
            Up::Z => [p[0] * k, p[1] * k, p[2] * k],
            Up::Y => [p[0] * k, p[2] * k, -p[1] * k],
        }
    }
}
