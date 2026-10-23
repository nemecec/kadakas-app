//! The unit of a DXF drawing (IOP-01, IOP-15).
//!
//! `$INSUNITS` (release 2000 and newer) names the unit of the coordinates. Older files and files
//! with the value 0 have none. Then the importer guesses. A guess is always said in the report.

use crate::options::Unit;

use super::parse::{Dxf, parse_int};
use super::version::Release;

/// The size of a unit in mm, and its name, for a `$INSUNITS` code. `None` for 0 (unitless) and for
/// the astronomical units (17 to 20), which no drawing of a site uses.
pub fn insunits_mm(code: i64) -> Option<(f64, &'static str)> {
    Some(match code {
        1 => (25.4, "inches"),
        2 => (304.8, "feet"),
        3 => (1_609_344.0, "miles"),
        4 => (1.0, "millimeters"),
        5 => (10.0, "centimeters"),
        6 => (1000.0, "meters"),
        7 => (1.0e6, "kilometers"),
        8 => (2.54e-5, "microinches"),
        9 => (0.0254, "mils"),
        10 => (914.4, "yards"),
        11 => (1.0e-7, "angstroms"),
        12 => (1.0e-6, "nanometers"),
        13 => (1.0e-3, "microns"),
        14 => (100.0, "decimeters"),
        15 => (1.0e4, "decameters"),
        16 => (1.0e5, "hectometers"),
        21 => (1200.0 / 3937.0 * 1000.0, "US survey feet"),
        22 => (1200.0 / 3937.0 * 1000.0 / 12.0, "US survey inches"),
        23 => (1200.0 / 3937.0 * 3000.0, "US survey yards"),
        24 => (1200.0 / 3937.0 * 5_280_000.0, "US survey miles"),
        _ => return None,
    })
}

/// The unit that the import uses.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    /// Millimeters per unit of the file.
    pub mm_per_unit: f64,
    /// A remark for the report when the unit was a guess or the option changed the unit of the
    /// file.
    pub note: Option<String>,
}

/// The name of a unit of the options, for the notes.
pub fn unit_name(u: Unit) -> &'static str {
    match u {
        Unit::Mm => "millimeters",
        Unit::Cm => "centimeters",
        Unit::M => "meters",
        Unit::Km => "kilometers",
        Unit::Inch => "inches",
        Unit::Foot => "feet",
        Unit::Yard => "yards",
        Unit::Mile => "miles",
    }
}

/// Finds the unit. The order is: the option `unit`, then `$INSUNITS`, then a guess. The guess is
/// inches when `$MEASUREMENT` says the drawing is English (0), else millimeters.
pub fn resolve(option: Option<Unit>, dxf: &Dxf, release: Option<&Release>) -> Resolved {
    let code = dxf.header_value("$INSUNITS").and_then(parse_int);
    let from_file = code.and_then(insunits_mm);
    if let Some(u) = option {
        let note = from_file
            .filter(|(mm, _)| (mm - u.mm_per_unit()).abs() > 1e-9 * mm)
            .map(|(_, name)| {
                format!(
                    "the option unit ({}) replaces the unit of the file ($INSUNITS says {name})",
                    unit_name(u)
                )
            });
        return Resolved {
            mm_per_unit: u.mm_per_unit(),
            note,
        };
    }
    if let Some((mm, _)) = from_file {
        return Resolved {
            mm_per_unit: mm,
            note: None,
        };
    }
    let why = match (code, release) {
        (Some(0), _) => "$INSUNITS is 0".to_string(),
        (Some(c), _) => format!("$INSUNITS {c} is not a unit of length that Kadakas knows"),
        (None, Some(r)) if !r.has_units() => format!("DXF release {} stores none", r.name),
        (None, _) => "$INSUNITS is missing".to_string(),
    };
    let english = dxf.header_value("$MEASUREMENT").and_then(parse_int) == Some(0);
    let (mm, name, reason) = if english {
        (
            25.4,
            "inches",
            " because $MEASUREMENT says the drawing is English",
        )
    } else {
        (1.0, "millimeters", "")
    };
    Resolved {
        mm_per_unit: mm,
        note: Some(format!(
            "the drawing has no unit ({why}): assuming {name}{reason}. Set the import option unit if this is wrong"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dxf(vars: &[(&str, &str)]) -> Dxf {
        let mut d = Dxf::default();
        for (k, v) in vars {
            d.header.insert((*k).into(), vec![(70, (*v).into())]);
        }
        d
    }

    #[test]
    fn every_unit_has_a_name_that_matches_its_size() {
        for u in [
            Unit::Mm,
            Unit::Cm,
            Unit::M,
            Unit::Km,
            Unit::Inch,
            Unit::Foot,
            Unit::Yard,
            Unit::Mile,
        ] {
            let (mm, name) = insunits_mm(u.insunits()).unwrap();
            assert_eq!(mm, u.mm_per_unit());
            assert_eq!(unit_name(u), name);
        }
    }

    #[test]
    fn every_code_has_a_size() {
        for code in (1..=16).chain(21..=24) {
            let (mm, name) = insunits_mm(code).unwrap_or_else(|| panic!("{code}"));
            assert!(mm > 0.0 && !name.is_empty());
        }
        for code in [0, 17, 18, 19, 20, 25, -1] {
            assert!(insunits_mm(code).is_none());
        }
        assert_eq!(insunits_mm(4).unwrap().0, 1.0);
        assert!((insunits_mm(21).unwrap().0 - 304.8006).abs() < 1e-3);
        for u in [
            Unit::Mm,
            Unit::Cm,
            Unit::M,
            Unit::Km,
            Unit::Inch,
            Unit::Foot,
            Unit::Yard,
            Unit::Mile,
        ] {
            assert_eq!(insunits_mm(u.insunits()).unwrap().0, u.mm_per_unit());
        }
    }

    #[test]
    fn the_file_unit_wins_over_a_guess_and_the_option_wins_over_the_file() {
        let r = resolve(None, &dxf(&[("$INSUNITS", "6")]), None);
        assert_eq!(r.mm_per_unit, 1000.0);
        assert!(r.note.is_none());
        let r = resolve(Some(Unit::Foot), &dxf(&[("$INSUNITS", "6")]), None);
        assert_eq!(r.mm_per_unit, 304.8);
        assert!(r.note.unwrap().contains("replaces"));
        let r = resolve(Some(Unit::M), &dxf(&[("$INSUNITS", "6")]), None);
        assert!(r.note.is_none());
        let r = resolve(Some(Unit::M), &dxf(&[]), None);
        assert!(r.note.is_none());
    }

    #[test]
    fn a_unitless_file_is_guessed_and_the_note_says_so() {
        let r12 = Release::parse("AC1009");
        let r = resolve(None, &dxf(&[]), r12.as_ref());
        assert_eq!(r.mm_per_unit, 1.0);
        let note = r.note.unwrap();
        assert!(note.starts_with("the drawing has no unit"));
        assert!(note.contains("assuming millimeters"));
        assert!(note.contains("release 12"));
        let r = resolve(
            None,
            &dxf(&[("$INSUNITS", "0")]),
            Release::parse("AC1018").as_ref(),
        );
        assert!(r.note.unwrap().contains("$INSUNITS is 0"));
        let r = resolve(None, &dxf(&[("$INSUNITS", "19")]), None);
        assert!(r.note.unwrap().contains("not a unit of length"));
        let r = resolve(None, &dxf(&[("$MEASUREMENT", "0")]), r12.as_ref());
        assert_eq!(r.mm_per_unit, 25.4);
        assert!(
            r.note
                .unwrap()
                .contains("assuming inches because $MEASUREMENT")
        );
        let r = resolve(None, &dxf(&[("$MEASUREMENT", "1")]), r12.as_ref());
        assert_eq!(r.mm_per_unit, 1.0);
    }
}
