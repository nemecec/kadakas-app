//! Areas and volumes. The model keeps millimeters, so areas are in mm² and volumes in mm³.

use serde::{Deserialize, Serialize};

use crate::length::LengthUnit;
use crate::number::{NumberFormat, is_group_space};

/// The unit in which areas show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AreaUnit {
    /// Square meters.
    #[default]
    M2,
    /// Hectares (10 000 m²).
    Ha,
    /// Square feet.
    Ft2,
    /// Acres (43 560 ft²).
    Ac,
}

impl AreaUnit {
    pub const ALL: [AreaUnit; 4] = [AreaUnit::M2, AreaUnit::Ha, AreaUnit::Ft2, AreaUnit::Ac];

    /// Square millimeters in one unit.
    pub fn mm2_per_unit(self) -> f64 {
        match self {
            AreaUnit::M2 => 1e6,
            AreaUnit::Ha => 1e10,
            AreaUnit::Ft2 => 304.8 * 304.8,
            AreaUnit::Ac => 43_560.0 * 304.8 * 304.8,
        }
    }

    /// The suffix with a leading space, for example ` m²`.
    pub fn suffix(self) -> &'static str {
        match self {
            AreaUnit::M2 => " m\u{b2}",
            AreaUnit::Ha => " ha",
            AreaUnit::Ft2 => " ft\u{b2}",
            AreaUnit::Ac => " ac",
        }
    }

    /// The name without a space, for lists.
    pub fn short_name(self) -> &'static str {
        self.suffix().trim_start()
    }

    /// A good number of decimals for this unit.
    pub fn default_decimals(self) -> usize {
        match self {
            AreaUnit::M2 => 2,
            AreaUnit::Ha => 4,
            AreaUnit::Ft2 => 1,
            AreaUnit::Ac => 4,
        }
    }
}

/// An area in mm² as text with its unit: `1 234,50 m²`.
pub fn format_area(mm2: f64, unit: AreaUnit, decimals: usize, nf: &NumberFormat) -> String {
    format!(
        "{}{}",
        nf.format(mm2 / unit.mm2_per_unit(), decimals),
        unit.suffix()
    )
}

/// The unit for volumes: cubic meters for metric lengths, cubic yards for imperial ones (the
/// unit of soil and mulch orders).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeUnit {
    M3,
    Yd3,
}

impl VolumeUnit {
    /// The volume unit that goes with a length unit.
    pub fn for_length(unit: LengthUnit) -> VolumeUnit {
        if unit.is_imperial() {
            VolumeUnit::Yd3
        } else {
            VolumeUnit::M3
        }
    }

    pub fn mm3_per_unit(self) -> f64 {
        match self {
            VolumeUnit::M3 => 1e9,
            VolumeUnit::Yd3 => 914.4f64.powi(3),
        }
    }

    pub fn suffix(self) -> &'static str {
        match self {
            VolumeUnit::M3 => " m\u{b3}",
            VolumeUnit::Yd3 => " yd\u{b3}",
        }
    }
}

/// A volume in mm³ as text with its unit: `12,500 m³`.
pub fn format_volume(mm3: f64, unit: VolumeUnit, decimals: usize, nf: &NumberFormat) -> String {
    format!(
        "{}{}",
        nf.format(mm3 / unit.mm3_per_unit(), decimals),
        unit.suffix()
    )
}

/// Reads an area that a person typed and gives mm². A number without a unit counts in
/// `default`. Units: `m²`, `m2`, `m^2`, `sqm`, `sq m`; `ha`, `hectare(s)`; `ft²`, `ft2`, `sqft`,
/// `sq ft`, `sf`; `ac`, `acre(s)`.
pub fn parse_area(text: &str, default: AreaUnit, nf: &NumberFormat) -> Option<f64> {
    let t = text.trim();
    let mut end = 0;
    let chars: Vec<(usize, char)> = t.char_indices().collect();
    for (k, (at, ch)) in chars.iter().enumerate() {
        let number_char = ch.is_ascii_digit()
            || matches!(ch, '.' | ',' | '-' | '+' | '\u{2212}')
            // A space belongs to the number only before a digit: `1 234`.
            || (is_group_space(*ch) && chars.get(k + 1).is_some_and(|(_, n)| n.is_ascii_digit()));
        if !number_char {
            break;
        }
        end = at + ch.len_utf8();
    }
    let (number, rest) = t.split_at(end);
    let value = nf.parse(number)?;
    let rest: String = rest
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| !is_group_space(*c) && *c != '.')
        .collect();
    let unit = match rest.as_str() {
        "" => default,
        "m\u{b2}" | "m2" | "m^2" | "sqm" | "sqmeter" | "sqmeters" => AreaUnit::M2,
        "ha" | "hectare" | "hectares" => AreaUnit::Ha,
        "ft\u{b2}" | "ft2" | "ft^2" | "sqft" | "sf" | "sqfeet" | "sqfoot" => AreaUnit::Ft2,
        "ac" | "acre" | "acres" => AreaUnit::Ac,
        _ => return None,
    };
    let mm2 = value * unit.mm2_per_unit();
    mm2.is_finite().then_some(mm2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::number::{Decimal, Grouping};

    const EN: NumberFormat = NumberFormat {
        decimal: Decimal::Point,
        grouping: Grouping::Comma,
    };
    const ET: NumberFormat = NumberFormat {
        decimal: Decimal::Comma,
        grouping: Grouping::Space,
    };

    #[test]
    fn areas_show_in_every_unit() {
        let one_ha = 1e10;
        assert_eq!(
            format_area(one_ha, AreaUnit::M2, 2, &EN),
            "10,000.00 m\u{b2}"
        );
        assert_eq!(format_area(one_ha, AreaUnit::Ha, 2, &ET), "1,00 ha");
        assert_eq!(
            format_area(one_ha, AreaUnit::Ft2, 0, &EN),
            "107,639 ft\u{b2}"
        );
        assert_eq!(format_area(one_ha, AreaUnit::Ac, 3, &EN), "2.471 ac");
        for u in AreaUnit::ALL {
            assert!(u.mm2_per_unit() > 0.0);
            assert!(u.default_decimals() > 0);
            assert_eq!(u.suffix().trim_start(), u.short_name());
        }
    }

    #[test]
    fn volumes_follow_the_length_system() {
        assert_eq!(VolumeUnit::for_length(LengthUnit::M), VolumeUnit::M3);
        assert_eq!(VolumeUnit::for_length(LengthUnit::FtIn), VolumeUnit::Yd3);
        assert_eq!(format_volume(2.5e9, VolumeUnit::M3, 2, &EN), "2.50 m\u{b3}");
        assert_eq!(
            format_volume(0.764_554_857_984e9, VolumeUnit::Yd3, 2, &EN),
            "1.00 yd\u{b3}"
        );
        assert_eq!(VolumeUnit::M3.mm3_per_unit(), 1e9);
    }

    #[test]
    fn parses_areas() {
        let p = |s: &str| parse_area(s, AreaUnit::M2, &ET);
        assert_eq!(p("12,5"), Some(12.5e6));
        assert_eq!(p("12,5 m\u{b2}"), Some(12.5e6));
        assert_eq!(p("12 m2"), Some(12e6));
        assert_eq!(p("12 sq m"), Some(12e6));
        assert_eq!(p("1 234 m\u{b2}"), Some(1234e6));
        assert_eq!(p("2 ha"), Some(2e10));
        assert_eq!(p("100 ft2"), Some(100.0 * 304.8 * 304.8));
        assert_eq!(p("100 sq. ft."), Some(100.0 * 304.8 * 304.8));
        assert_eq!(p("1 ac"), Some(43_560.0 * 304.8 * 304.8));
        assert_eq!(p("2 acres"), Some(2.0 * 43_560.0 * 304.8 * 304.8));
        assert_eq!(p("-3 m2"), Some(-3e6));
        assert_eq!(parse_area("5", AreaUnit::Ha, &ET), Some(5e10));
        for bad in ["", "abc", "5 parsecs", "m2", "1 2 m2", "5 km2"] {
            assert_eq!(p(bad), None, "{bad:?}");
        }
    }
}
