//! Units and formats (PLAT-05): how Kadakas writes and reads lengths, areas, angles, numbers
//! and dates.
//!
//! The model keeps millimeters (and mm², mm³) and angles in degrees. This crate turns them into
//! text for people and reads what people type:
//!
//! - lengths: mm, cm, m, in, ft, and feet with inches and fractions (`5' 6 1/2"`);
//! - areas: m², ha, ft², ac; volumes: m³ and yd³;
//! - angles: decimal degrees, degrees-minutes-seconds, bearings (`N 45°30'00" E`);
//! - numbers: decimal point or comma, digit grouping;
//! - dates: ISO, day-month-year, month-day-year.
//!
//! `Units` holds one choice of all of these. `Settings` is what a person stores in the
//! preferences ("automatic" for what the language decides). The UI process has one current
//! `Units` (`current`, `set_current`), so code deep in the UI (tool readouts) needs no extra
//! argument. A value in a document never depends on `Units`: files and the API keep plain
//! numbers.
//!
//! Dimension texts in drawings are different: they follow the dimension style of the document
//! (`kadakas-core`), not the language of the person who looks at them. That code uses the
//! `format_drawing` and `format_ftin_drawing` functions of this crate, so the rounding rules
//! exist once.

mod angle;
mod area;
mod date;
mod length;
mod number;

pub use angle::{AngleFormat, format_angle, parse_angle};
pub use area::{
    AreaUnit, VolumeUnit, format_area, format_area_drawing, format_volume, parse_area, parse_volume,
};
pub use date::{Date, DateFormat, days_in_month, is_leap};
pub use length::{LengthUnit, format_drawing, format_ftin_drawing, format_length, parse_length};
pub use number::{Decimal, Grouping, NumberFormat, round_half_away, trim_negative_zero};

use std::cell::Cell;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

/// The default denominator of inch fractions (1/16).
pub const DEFAULT_FRACTION: u32 = 16;

/// One complete choice of units and formats.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Units {
    pub length: LengthUnit,
    pub area: AreaUnit,
    pub angle: AngleFormat,
    pub number: NumberFormat,
    pub date: DateFormat,
    /// The denominator of inch fractions (2, 4, 8, 16, 32 or 64), or 0 for decimal inches.
    pub inch_fraction: u32,
}

impl Units {
    /// Millimeters, square meters, decimal degrees, a decimal point, ISO dates.
    pub const METRIC: Units = Units {
        length: LengthUnit::Mm,
        area: AreaUnit::M2,
        angle: AngleFormat::Decimal,
        number: NumberFormat {
            decimal: Decimal::Point,
            grouping: Grouping::None,
        },
        date: DateFormat::Iso,
        inch_fraction: DEFAULT_FRACTION,
    };

    /// The number and date conventions of a language (`en-US`, `et`). An unknown language gets
    /// the conventions of en-US.
    pub fn conventions(language: &str) -> (NumberFormat, DateFormat) {
        let date = match language.split(['-', '_']).next().unwrap_or_default() {
            // Estonian: 11.10.2026. English (United States): 10/11/2026.
            "et" => DateFormat::Dmy,
            _ => DateFormat::Mdy,
        };
        (number_convention(language), date)
    }

    /// The decimals that lengths get when the caller has no better idea.
    pub fn length_decimals(&self) -> usize {
        self.length.default_decimals()
    }

    /// A length in mm for people to read, with its unit: `1 234,5 mm`, `5' 6 1/2"`.
    pub fn length_text(&self, mm: f64) -> String {
        self.length_text_d(mm, self.length_decimals())
    }

    /// Like `length_text`, with a number of decimals.
    pub fn length_text_d(&self, mm: f64, decimals: usize) -> String {
        format_length(mm, self.length, decimals, self.inch_fraction, &self.number)
    }

    /// A length for a widget that a person drags or edits: the number with the unit, without
    /// trailing zeros (`12.5 mm`, `1.2 m`), and without digit grouping. Feet and inches keep
    /// their fractions (`5' 6 1/2"`). Parsing the text with `parse_length` gives the length back.
    pub fn length_compact(&self, mm: f64) -> String {
        let plain = self.number.plain();
        let places = match self.length {
            LengthUnit::Mm => 3,
            LengthUnit::Cm => 4,
            LengthUnit::M => 6,
            LengthUnit::Ft => 5,
            LengthUnit::In | LengthUnit::FtIn => 4,
        };
        match self.length {
            LengthUnit::Mm | LengthUnit::Cm | LengthUnit::M | LengthUnit::Ft => {
                if !mm.is_finite() {
                    return plain.format(mm, 0);
                }
                format!(
                    "{}{}",
                    plain.format_trim(mm / self.length.mm_per_unit(), places),
                    self.length.suffix()
                )
            }
            LengthUnit::In | LengthUnit::FtIn if self.inch_fraction == 0 || !mm.is_finite() => {
                format_length(mm, self.length, places, 0, &plain)
            }
            // A length that is not on the grid of fractions shows as decimal inches, so that
            // the text is not a rounded copy of the value.
            _ => {
                let grid = f64::from(self.inch_fraction);
                let on_grid = ((mm / 25.4) * grid - ((mm / 25.4) * grid).round()).abs() < 1e-6;
                if on_grid {
                    format_length(mm, self.length, 2, self.inch_fraction, &plain)
                } else {
                    format_length(mm, self.length, places, 0, &plain)
                }
            }
        }
    }

    /// These units for an angle that is not a direction (a sweep, a step, a pitch, an arc of
    /// 360 degrees). A bearing only makes sense for a direction, so it becomes degrees,
    /// minutes and seconds here.
    pub fn plain_angle(&self) -> Units {
        Units {
            angle: if self.angle == AngleFormat::Bearing {
                AngleFormat::Dms
            } else {
                self.angle
            },
            ..*self
        }
    }

    /// If `text` starts with a number like `1,500` that the active format reads as a thousands
    /// group (1500) while another format would read it as a decimal fraction (1.5), this is the
    /// decimal separator of the active format. The Data bar refuses such a number and names the
    /// separator. `None` for an unambiguous number.
    pub fn ambiguous_number(&self, text: &str) -> Option<char> {
        let t = text.trim();
        let t = t.strip_prefix(['-', '+', '\u{2212}']).unwrap_or(t);
        let run: String = t
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
            .collect();
        self.number
            .is_ambiguous(&run)
            .then(|| self.number.decimal.symbol())
    }

    /// An angle for a widget that a person drags or edits (`45.5°`, `45°30'00"`).
    pub fn angle_compact(&self, deg: f64) -> String {
        match self.angle {
            AngleFormat::Decimal => {
                if deg.is_finite() {
                    format!("{}\u{b0}", self.number.plain().format_trim(deg, 3))
                } else {
                    self.number.plain().format(deg, 0)
                }
            }
            f => format_angle(deg, f, 1, &self.number.plain()),
        }
    }

    /// The text that goes into an edit field for a length: no digit grouping, and no unit for the
    /// metric units and feet (the field shows the unit next to it, see `length_field_unit`).
    /// Inches and feet with inches include their marks (`5' 6"`), because the marks are the unit.
    pub fn length_field(&self, mm: f64, decimals: usize) -> String {
        let plain = self.number.plain();
        match self.length {
            LengthUnit::In | LengthUnit::FtIn => {
                format_length(mm, self.length, decimals, self.inch_fraction, &plain)
            }
            u => plain.format(mm / u.mm_per_unit(), decimals),
        }
    }

    /// The unit that goes next to an edit field: `mm`, `cm`, `m`, `ft`, or empty when the text
    /// of the field carries its marks.
    pub fn length_field_unit(&self) -> &'static str {
        match self.length {
            LengthUnit::In | LengthUnit::FtIn => "",
            u => u.short_name(),
        }
    }

    /// Reads a length that a person typed into a field and gives mm. A number without a unit
    /// counts in the active unit.
    pub fn parse_length(&self, text: &str) -> Option<f64> {
        parse_length(text, self.length, &self.number)
    }

    /// An area in mm² with its unit.
    pub fn area_text(&self, mm2: f64) -> String {
        format_area(mm2, self.area, self.area.default_decimals(), &self.number)
    }

    /// An area in mm² with its unit and a number of decimals.
    pub fn area_text_d(&self, mm2: f64, decimals: usize) -> String {
        format_area(mm2, self.area, decimals, &self.number)
    }

    /// Reads an area and gives mm².
    pub fn parse_area(&self, text: &str) -> Option<f64> {
        parse_area(text, self.area, &self.number)
    }

    /// The unit of volumes: m³ for metric lengths, yd³ for imperial ones.
    pub fn volume_unit(&self) -> VolumeUnit {
        VolumeUnit::for_length(self.length)
    }

    /// A volume in mm³ with its unit.
    pub fn volume_text(&self, mm3: f64) -> String {
        format_volume(mm3, self.volume_unit(), 2, &self.number)
    }

    /// Reads a volume and gives mm³.
    pub fn parse_volume(&self, text: &str) -> Option<f64> {
        parse_volume(text, self.volume_unit(), &self.number)
    }

    /// An area for a widget that a person drags or edits: no trailing zeros, no grouping.
    pub fn area_compact(&self, mm2: f64) -> String {
        if !mm2.is_finite() {
            return self.number.plain().format(mm2, 0);
        }
        format!(
            "{}{}",
            self.number
                .plain()
                .format_trim(mm2 / self.area.mm2_per_unit(), 6),
            self.area.suffix()
        )
    }

    /// A volume for a widget that a person drags or edits: no trailing zeros, no grouping.
    pub fn volume_compact(&self, mm3: f64) -> String {
        let unit = self.volume_unit();
        if !mm3.is_finite() {
            return self.number.plain().format(mm3, 0);
        }
        format!(
            "{}{}",
            self.number
                .plain()
                .format_trim(mm3 / unit.mm3_per_unit(), 6),
            unit.suffix()
        )
    }

    /// An angle in degrees.
    pub fn angle_text(&self, deg: f64) -> String {
        self.angle_text_d(deg, 1)
    }

    /// An angle in degrees, with decimals of the degrees (decimal form) or of the seconds.
    pub fn angle_text_d(&self, deg: f64, decimals: usize) -> String {
        let decimals = if self.angle == AngleFormat::Decimal {
            decimals
        } else {
            decimals.saturating_sub(1)
        };
        format_angle(deg, self.angle, decimals, &self.number)
    }

    /// The text that goes into an edit field for an angle: no grouping and no degree sign for
    /// the decimal form.
    pub fn angle_field(&self, deg: f64, decimals: usize) -> String {
        match self.angle {
            AngleFormat::Decimal => self.number.plain().format(deg, decimals),
            f => format_angle(deg, f, decimals.saturating_sub(1), &self.number.plain()),
        }
    }

    /// Reads an angle that a person typed. All forms are accepted. A number without a mark is
    /// in degrees.
    pub fn parse_angle(&self, text: &str) -> Option<f64> {
        parse_angle(text, &self.number)
    }

    /// A plain number with `decimals` decimals (for example a scale or a count of a unit).
    pub fn number_text(&self, v: f64, decimals: usize) -> String {
        self.number.format(v, decimals)
    }

    /// A plain number without trailing zeros.
    pub fn number_text_trim(&self, v: f64, max_decimals: usize) -> String {
        self.number.format_trim(v, max_decimals)
    }

    /// Reads a plain number.
    pub fn parse_number(&self, text: &str) -> Option<f64> {
        self.number.parse(text)
    }

    /// A date as text.
    pub fn date_text(&self, d: Date) -> String {
        self.date.format(d)
    }

    /// Reads a date.
    pub fn parse_date(&self, text: &str) -> Option<Date> {
        self.date.parse(text)
    }
}

impl Default for Units {
    fn default() -> Units {
        Units::METRIC
    }
}

/// The way numbers are written where a locale is used: the one table of the program. `locale` is
/// a BCP 47 tag or a POSIX name (`de-CH`, `de_CH.UTF-8`); a region refines the language.
/// An unknown language gets `1,234.56`.
///
/// - comma groups, point decimal: English (all regions), Japanese, Chinese, Korean, Hebrew, Thai,
///   Hindi (the Indian lakh grouping `12,34,567.89` is not supported: it is written `1,234,567.89`),
///   Spanish in Mexico, the United States and most of Latin America;
/// - point groups, comma decimal: German, Dutch, Spanish, Italian, Portuguese (Brazil), Indonesian,
///   Turkish, Danish, Greek, Romanian, Croatian, Slovenian;
/// - space groups, comma decimal: Estonian, Finnish, French, Swedish, Norwegian, Polish, Czech,
///   Slovak, Russian, Ukrainian, Bulgarian, Hungarian, Lithuanian, Latvian, German in Austria,
///   Portuguese in Portugal, French in Switzerland, English in South Africa;
/// - apostrophe groups, point decimal: German and Italian in Switzerland.
pub fn number_convention(locale: &str) -> NumberFormat {
    use Decimal::{Comma, Point};
    use Grouping::{Apostrophe, Comma as GComma, Point as GPoint, Space};
    let tag = locale
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .replace('_', "-");
    let mut parts = tag.split('-');
    let lang = parts.next().unwrap_or_default().to_ascii_lowercase();
    let region = region_of(&tag).unwrap_or_default();
    let (decimal, grouping) = match (lang.as_str(), region.as_str()) {
        ("de" | "it", "CH") => (Point, Apostrophe),
        ("fr", "CH") => (Comma, Space),
        ("de", "AT") => (Comma, Space),
        ("pt", "PT") => (Comma, Space),
        ("en", "ZA") => (Comma, Space),
        ("es", "MX" | "US" | "419" | "GT" | "HN" | "NI" | "PA" | "PE" | "PR" | "SV" | "DO") => {
            (Point, GComma)
        }
        ("de" | "nl" | "es" | "it" | "pt" | "id" | "tr" | "da" | "el" | "ro" | "hr" | "sl", _) => {
            (Comma, GPoint)
        }
        (
            "et" | "fi" | "fr" | "sv" | "nb" | "no" | "nn" | "pl" | "cs" | "sk" | "ru" | "uk"
            | "bg" | "hu" | "lt" | "lv",
            _,
        ) => (Comma, Space),
        _ => (Point, GComma),
    };
    NumberFormat { decimal, grouping }
}

/// The region of a locale tag (`en-US` gives `US`, `zh-Hant-TW` gives `TW`, `es-419` gives `419`,
/// `en` gives nothing). A POSIX name (`en_US.UTF-8`) works too.
pub fn region_of(locale: &str) -> Option<String> {
    let tag = locale
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .replace('_', "-");
    tag.split('-')
        .skip(1)
        .find(|p| {
            (p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
                || (p.len() == 3 && p.chars().all(|c| c.is_ascii_digit()))
        })
        .map(str::to_ascii_uppercase)
}

/// True for the regions that measure in feet and inches in daily life: the United States,
/// Liberia and Myanmar. The United Kingdom is metric here (a decision of the project).
pub fn imperial_region(region: &str) -> bool {
    matches!(region.to_ascii_uppercase().as_str(), "US" | "LR" | "MM")
}

/// What a person chooses in the preferences. A choice of `None` means "from the language".
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub length: LengthUnit,
    pub area: AreaUnit,
    pub angle: AngleFormat,
    pub decimal: Option<Decimal>,
    pub grouping: Option<Grouping>,
    pub date: Option<DateFormat>,
    /// The denominator of inch fractions, or 0 for decimal inches. `None` gives 1/16.
    pub inch_fraction: Option<u32>,
}

impl Settings {
    /// The units that these settings give in a language.
    pub fn resolve(&self, language: &str) -> Units {
        let (number, date) = Units::conventions(language);
        let decimal = self.decimal.unwrap_or(number.decimal);
        let mut grouping = self.grouping.unwrap_or(number.grouping);
        // The same symbol for the decimal separator and for the groups cannot be read. Use a
        // space for the groups then.
        if grouping.symbol() == Some(decimal.symbol()) {
            grouping = Grouping::Space;
        }
        Units {
            length: self.length,
            area: self.area,
            angle: self.angle,
            number: NumberFormat { decimal, grouping },
            date: self.date.unwrap_or(date),
            inch_fraction: self.inch_fraction.unwrap_or(DEFAULT_FRACTION),
        }
    }
}

/// The units of the UI process.
static CURRENT: RwLock<Units> = RwLock::new(Units::METRIC);

thread_local! {
    /// Units that override the process units on this thread.
    static OVERRIDE: Cell<Option<Units>> = const { Cell::new(None) };
}

/// The current units: the override of this thread, else the units of the process.
pub fn current() -> Units {
    OVERRIDE
        .with(Cell::get)
        .unwrap_or_else(|| *CURRENT.read().unwrap_or_else(|e| e.into_inner()))
}

/// Sets the units of the process. The UI calls this at start and when the person changes the
/// preferences or the language.
pub fn set_current(units: Units) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = units;
}

/// Runs `f` with other units on this thread only. For tests.
pub fn with_units<R>(units: Units, f: impl FnOnce() -> R) -> R {
    /// Puts the old override back, also when `f` panics.
    struct Restore(Option<Units>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDE.with(|o| o.set(self.0));
        }
    }
    let _restore = Restore(OVERRIDE.with(|o| o.replace(Some(units))));
    f()
}

// ---- numbers in the UI ---------------------------------------------------------------------
//
// One set of helpers for every number that the UI shows or reads: they follow the number format
// of the person (`current().number`). Files, the JSON-RPC API, the command line and exports never
// call them: they use the plain format (a point, no grouping).

/// A number for people to read, with `decimals` decimals and digit grouping: `1 234,50`.
pub fn num(v: f64, decimals: usize) -> String {
    current().number_text(v, decimals)
}

/// Like [`num`], without trailing zeros.
pub fn num_trim(v: f64, max_decimals: usize) -> String {
    current().number_text_trim(v, max_decimals)
}

/// A number for a text field that a person edits: no digit grouping, so it reads back exactly.
pub fn num_plain(v: f64, decimals: usize) -> String {
    current().number.plain().format(v, decimals)
}

/// Like [`num_plain`], without trailing zeros.
pub fn num_plain_trim(v: f64, max_decimals: usize) -> String {
    current().number.plain().format_trim(v, max_decimals)
}

/// A whole number for people to read, with digit grouping: `1 234 567`.
pub fn int(n: i64) -> String {
    current().number.format(n as f64, 0)
}

/// Reads a number that a person typed. Both `.` and `,` work as the decimal separator where the
/// symbol cannot be a digit group (`1,5`); `1,234` follows the active format (see
/// [`NumberFormat::is_ambiguous`]).
pub fn parse_num(text: &str) -> Option<f64> {
    current().parse_number(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn et() -> Units {
        Settings::default().resolve("et")
    }

    #[test]
    fn the_ui_helpers_follow_the_current_format() {
        let comma = Settings {
            decimal: Some(Decimal::Comma),
            grouping: Some(Grouping::Space),
            ..Settings::default()
        }
        .resolve("en-US");
        with_units(comma, || {
            assert_eq!(num(1234.5, 2), "1\u{a0}234,50");
            assert_eq!(num(-1234567.891, 1), "-1\u{a0}234\u{a0}567,9");
            assert_eq!(num(0.5, 3), "0,500");
            assert_eq!(num(999.0, 0), "999");
            assert_eq!(num_trim(2.5, 3), "2,5");
            assert_eq!(num_plain(1234.5, 2), "1234,50");
            assert_eq!(num_plain_trim(-0.25, 4), "-0,25");
            assert_eq!(int(1234567), "1\u{a0}234\u{a0}567");
            assert_eq!(int(-12345), "-12\u{a0}345");
            assert_eq!(int(1000), "1\u{a0}000");
            assert_eq!(parse_num("1,5"), Some(1.5));
            assert_eq!(parse_num("1.5"), Some(1.5));
            assert_eq!(parse_num("1,234"), Some(1.234));
            assert_eq!(parse_num("1 234,5"), Some(1234.5));
            assert_eq!(parse_num("abc"), None);
            // Typed text reads back exactly.
            for v in [0.0, 7.25, -1234.56, 12345.67] {
                assert_eq!(parse_num(&num_plain(v, 2)), Some(v));
                assert_eq!(parse_num(&num(v, 2)), Some(v));
            }
        });
        with_units(Units::METRIC, || {
            assert_eq!(num(1234.5, 2), "1234.50");
            assert_eq!(parse_num("1,234"), Some(1.234));
            assert_eq!(int(12345), "12345");
        });
        let en = Settings::default().resolve("en-US");
        with_units(en, || {
            assert_eq!(num(1234.5, 2), "1,234.50");
            assert_eq!(parse_num("1,234"), Some(1234.0));
            assert_eq!(parse_num("1,5"), Some(1.5));
            assert_eq!(int(12345), "12,345");
        });
    }

    #[test]
    fn the_language_gives_conventions() {
        let e = et();
        assert_eq!(e.number.decimal, Decimal::Comma);
        assert_eq!(e.date, DateFormat::Dmy);
        let en = Settings::default().resolve("en-US");
        assert_eq!(en.number.decimal, Decimal::Point);
        assert_eq!(en.number.grouping, Grouping::Comma);
        assert_eq!(en.date, DateFormat::Mdy);
        assert_eq!(Units::conventions("xx_YY").0.decimal, Decimal::Point);
        assert_eq!(Units::conventions("et_EE").0.decimal, Decimal::Comma);
    }

    #[test]
    fn the_one_table_covers_languages_and_regions() {
        let n = |l: &str| {
            let f = number_convention(l);
            (f.decimal, f.grouping)
        };
        use Decimal::{Comma, Point};
        assert_eq!(n("en-US"), (Point, Grouping::Comma));
        assert_eq!(n("en-GB"), (Point, Grouping::Comma));
        assert_eq!(n("en-AU"), (Point, Grouping::Comma));
        assert_eq!(n("en-IN"), (Point, Grouping::Comma));
        assert_eq!(n("en_ZA.UTF-8"), (Comma, Grouping::Space));
        assert_eq!(n("de-DE"), (Comma, Grouping::Point));
        assert_eq!(n("de-AT"), (Comma, Grouping::Space));
        assert_eq!(n("de-CH"), (Point, Grouping::Apostrophe));
        assert_eq!(n("it-CH"), (Point, Grouping::Apostrophe));
        assert_eq!(n("fr-CH"), (Comma, Grouping::Space));
        assert_eq!(n("fr-FR"), (Comma, Grouping::Space));
        assert_eq!(n("pt-PT"), (Comma, Grouping::Space));
        assert_eq!(n("pt-BR"), (Comma, Grouping::Point));
        assert_eq!(n("es-ES"), (Comma, Grouping::Point));
        assert_eq!(n("es-MX"), (Point, Grouping::Comma));
        assert_eq!(n("es-419"), (Point, Grouping::Comma));
        assert_eq!(n("et"), (Comma, Grouping::Space));
        assert_eq!(n("ru_RU.UTF-8"), (Comma, Grouping::Space));
        assert_eq!(n("ja-JP"), (Point, Grouping::Comma));
        assert_eq!(n("xx"), (Point, Grouping::Comma));
        assert_eq!(n(""), (Point, Grouping::Comma));
        // Units::conventions uses the same table.
        assert_eq!(Units::conventions("de-CH").0, number_convention("de-CH"));
    }

    #[test]
    fn regions_and_the_imperial_regions() {
        for (tag, want) in [
            ("en-US", Some("US")),
            ("en_US.UTF-8", Some("US")),
            ("zh-Hant-TW", Some("TW")),
            ("es-419", Some("419")),
            ("en", None),
            ("C", None),
            ("", None),
        ] {
            assert_eq!(region_of(tag).as_deref(), want, "{tag}");
        }
        assert!(imperial_region("US") && imperial_region("lr") && imperial_region("MM"));
        assert!(!imperial_region("GB") && !imperial_region("CA") && !imperial_region(""));
    }

    #[test]
    fn settings_override_the_language() {
        let s = Settings {
            decimal: Some(Decimal::Point),
            grouping: Some(Grouping::None),
            date: Some(DateFormat::Iso),
            inch_fraction: Some(0),
            length: LengthUnit::FtIn,
            area: AreaUnit::Ac,
            angle: AngleFormat::Dms,
        };
        let u = s.resolve("et");
        assert_eq!(u.number, NumberFormat::default());
        assert_eq!(u.date, DateFormat::Iso);
        assert_eq!(u.inch_fraction, 0);
        assert_eq!(
            (u.length, u.area, u.angle),
            (LengthUnit::FtIn, AreaUnit::Ac, AngleFormat::Dms)
        );
        assert_eq!(Settings::default().resolve("en").inch_fraction, 16);
    }

    #[test]
    fn units_write_and_read_text() {
        let u = et();
        assert_eq!(u.length_text(1234567.0), "1\u{a0}234\u{a0}567,0 mm");
        assert_eq!(u.length_text_d(1500.0, 0), "1\u{a0}500 mm");
        assert_eq!(u.length_field(1234.56, 1), "1234,6");
        assert_eq!(u.length_field_unit(), "mm");
        assert_eq!(u.parse_length("1,5 m"), Some(1500.0));
        assert_eq!(u.parse_length("12,5"), Some(12.5));
        assert_eq!(u.area_text(12.5e6), "12,50 m\u{b2}");
        assert_eq!(u.area_text_d(12.5e6, 0), "13 m\u{b2}");
        assert_eq!(u.parse_area("12,5 m2"), Some(12.5e6));
        assert_eq!(u.volume_text(2.5e9), "2,50 m\u{b3}");
        assert_eq!(u.angle_text(45.55), "45,6\u{b0}");
        assert_eq!(u.angle_text_d(45.0, 0), "45\u{b0}");
        assert_eq!(u.angle_field(45.0, 1), "45,0");
        assert_eq!(u.parse_angle("45,5\u{b0}"), Some(45.5));
        assert_eq!(u.number_text(1234.5, 1), "1\u{a0}234,5");
        assert_eq!(u.number_text_trim(2.50, 3), "2,5");
        assert_eq!(u.parse_number("1 234,5"), Some(1234.5));
        let d = Date::new(2026, 10, 11).unwrap();
        assert_eq!(u.date_text(d), "11.10.2026");
        assert_eq!(u.parse_date("11.10.2026"), Some(d));
    }

    #[test]
    fn compact_text_for_widgets_round_trips() {
        let mut u = et();
        assert_eq!(u.length_compact(12.5), "12,5 mm");
        assert_eq!(u.length_compact(1234.0), "1234 mm");
        u.length = LengthUnit::M;
        assert_eq!(u.length_compact(1250.0), "1,25 m");
        u.length = LengthUnit::Cm;
        assert_eq!(u.length_compact(12.0), "1,2 cm");
        u.length = LengthUnit::Ft;
        assert_eq!(u.length_compact(609.6), "2'");
        u.length = LengthUnit::FtIn;
        assert_eq!(u.length_compact(25.4 * 66.5), "5' 6 1/2\"");
        u.length = LengthUnit::In;
        assert_eq!(u.length_compact(25.4 * 6.25), "6 1/4\"");
        u.inch_fraction = 0;
        assert_eq!(u.length_compact(25.4 * 6.25), "6,25\"");
        for l in LengthUnit::ALL {
            u.length = l;
            u.inch_fraction = 16;
            let t = u.length_compact(1234.0);
            assert!(u.parse_length(&t).is_some(), "{t}");
        }
        assert_eq!(u.length_compact(f64::NAN), "\u{2013}");
        let mut a = et();
        assert_eq!(a.angle_compact(45.5), "45,5\u{b0}");
        a.angle = AngleFormat::Dms;
        assert_eq!(a.angle_compact(45.5), "45\u{b0}30'00,0\"");
        assert_eq!(Units::METRIC.angle_compact(f64::INFINITY), "\u{2013}");
    }

    #[test]
    fn compact_areas_and_volumes_round_trip() {
        let mut u = et();
        assert_eq!(u.area_compact(12.5e6), "12,5 m\u{b2}");
        assert_eq!(u.volume_compact(2.5e9), "2,5 m\u{b3}");
        assert_eq!(u.parse_volume("2,5 m\u{b3}"), Some(2.5e9));
        assert_eq!(u.area_compact(f64::NAN), "\u{2013}");
        assert_eq!(u.volume_compact(f64::NAN), "\u{2013}");
        u.area = AreaUnit::Ac;
        u.length = LengthUnit::Ft;
        let t = u.area_compact(4_046_856_422.4);
        assert!(t.ends_with(" ac"), "{t}");
        assert!((u.parse_area(&t).unwrap() - 4_046_856_422.4).abs() < 1.0);
        let v = u.volume_compact(764_554_857_984.0);
        assert!(v.ends_with(" yd\u{b3}"), "{v}");
        assert!((u.parse_volume(&v).unwrap() - 764_554_857_984.0).abs() < 1e3);
    }

    #[test]
    fn a_length_off_the_grid_shows_as_decimal_inches() {
        let u = Units {
            length: LengthUnit::FtIn,
            ..Units::METRIC
        };
        // 1/16 inch is the grid. 1000 mm is not on it.
        assert_eq!(u.length_compact(25.4 * 66.5), "5' 6 1/2\"");
        assert_eq!(u.length_compact(1000.0), "3' 3.3701\"");
        let t = u.length_compact(1000.0);
        assert!((u.parse_length(&t).unwrap() - 1000.0).abs() < 0.01);
        let u = Units {
            length: LengthUnit::In,
            ..Units::METRIC
        };
        assert_eq!(u.length_compact(1000.0), "39.3701\"");
        assert_eq!(u.length_compact(25.4 * 6.25), "6 1/4\"");
    }

    #[test]
    fn a_bearing_is_for_directions_only() {
        let mut u = Units::METRIC;
        u.angle = AngleFormat::Bearing;
        assert_eq!(u.angle_compact(45.0), "N 45\u{b0}00'00.0\" E");
        let p = u.plain_angle();
        assert_eq!(p.angle, AngleFormat::Dms);
        assert_eq!(p.angle_compact(360.0), "360\u{b0}00'00.0\"");
        assert_eq!(p.angle_compact(-90.0), "-90\u{b0}00'00.0\"");
        let mut d = Units::METRIC;
        d.angle = AngleFormat::Decimal;
        assert_eq!(d.plain_angle().angle, AngleFormat::Decimal);
    }

    #[test]
    fn a_grouping_symbol_never_equals_the_decimal_symbol() {
        let s = Settings {
            decimal: Some(Decimal::Comma),
            grouping: Some(Grouping::Comma),
            ..Settings::default()
        };
        assert_eq!(s.resolve("en-US").number.grouping, Grouping::Space);
        let s = Settings {
            decimal: Some(Decimal::Point),
            grouping: Some(Grouping::Point),
            ..Settings::default()
        };
        assert_eq!(s.resolve("et").number.grouping, Grouping::Space);
        // The language alone never gives a conflict, and a free choice stays.
        for lang in ["en-US", "et", "xx"] {
            let n = Settings::default().resolve(lang).number;
            assert_ne!(n.grouping.symbol(), Some(n.decimal.symbol()));
        }
    }

    #[test]
    fn a_number_like_1_500_is_ambiguous_with_a_comma_for_groups() {
        let en = Settings::default().resolve("en-US");
        assert_eq!(en.ambiguous_number("1,500"), Some('.'));
        assert_eq!(en.ambiguous_number("-1,500 mm"), Some('.'));
        assert_eq!(en.ambiguous_number("1,5"), None);
        assert_eq!(en.ambiguous_number("1500"), None);
        assert_eq!(en.ambiguous_number("0,500"), None);
        assert_eq!(en.ambiguous_number("1,234,567"), None);
        let et = Settings::default().resolve("et");
        assert_eq!(et.ambiguous_number("1,500"), None);
        // A leading zero is no group: `0,500` is one half.
        assert_eq!(en.parse_number("0,500"), Some(0.5));
    }

    #[test]
    fn angle_forms_in_fields() {
        let mut u = Units::METRIC;
        u.angle = AngleFormat::Dms;
        assert_eq!(u.angle_text(45.5), "45\u{b0}30'00\"");
        assert_eq!(u.angle_field(45.5, 1), "45\u{b0}30'00\"");
        u.angle = AngleFormat::Bearing;
        assert_eq!(u.angle_text(45.0), "N 45\u{b0}00'00\" E");
    }

    #[test]
    fn imperial_fields() {
        let mut u = Units::METRIC;
        u.length = LengthUnit::FtIn;
        assert_eq!(u.length_text(25.4 * 66.5), "5' 6 1/2\"");
        assert_eq!(u.length_field(25.4 * 66.5, 2), "5' 6 1/2\"");
        assert_eq!(u.length_field_unit(), "");
        assert_eq!(u.parse_length("5' 6 1/2\""), Some(25.4 * 66.5));
        assert_eq!(u.volume_unit(), VolumeUnit::Yd3);
        u.length = LengthUnit::Ft;
        assert_eq!(u.length_field(304.8 * 2.5, 3), "2.500");
        assert_eq!(u.length_field_unit(), "ft");
    }

    #[test]
    fn the_current_units_can_be_set_and_overridden() {
        // `set_current` is process wide; the override keeps this test apart from other tests.
        with_units(et(), || {
            assert_eq!(current(), et());
            with_units(Units::METRIC, || assert_eq!(current(), Units::METRIC));
            assert_eq!(current(), et());
        });
        assert_eq!(Units::default(), Units::METRIC);
    }

    #[test]
    fn the_units_override_is_restored_after_a_panic() {
        let result = std::panic::catch_unwind(|| {
            with_units(et(), || {
                assert_eq!(current(), et());
                panic!("on purpose");
            })
        });
        assert!(result.is_err());
        assert_eq!(current(), Units::METRIC);
    }

    #[test]
    fn units_round_trip_through_json() {
        let u = et();
        let json = serde_json::to_string(&u).unwrap();
        assert_eq!(serde_json::from_str::<Units>(&json).unwrap(), u);
        assert_eq!(serde_json::from_str::<Units>("{}").unwrap(), Units::METRIC);
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
        assert_eq!(serde_json::from_str::<Settings>("{}").unwrap(), s);
    }
}
