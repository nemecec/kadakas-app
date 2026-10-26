//! Lengths: millimeters, centimeters, meters, inches, feet, and feet with inches and fractions.
//!
//! The model unit is the millimeter. Every function here takes or gives millimeters.

use serde::{Deserialize, Serialize};

use crate::number::{NumberFormat, is_group_space, round_half_away, trim_negative_zero};

/// The unit in which lengths show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    #[default]
    Mm,
    Cm,
    M,
    /// Inches.
    In,
    Ft,
    /// Feet and inches, for example 3'-4.5" or 3' 4 1/2".
    FtIn,
}

impl LengthUnit {
    /// All units, metric first.
    pub const ALL: [LengthUnit; 6] = [
        LengthUnit::Mm,
        LengthUnit::Cm,
        LengthUnit::M,
        LengthUnit::In,
        LengthUnit::Ft,
        LengthUnit::FtIn,
    ];

    /// The suffix of a drawing text, with the space where one belongs.
    pub fn suffix(self) -> &'static str {
        match self {
            LengthUnit::Mm => " mm",
            LengthUnit::Cm => " cm",
            LengthUnit::M => " m",
            LengthUnit::In => "\"",
            LengthUnit::Ft => "'",
            LengthUnit::FtIn => "",
        }
    }

    /// A short name for lists and for the unit label of a field.
    pub fn short_name(self) -> &'static str {
        match self {
            LengthUnit::Mm => "mm",
            LengthUnit::Cm => "cm",
            LengthUnit::M => "m",
            LengthUnit::In => "in",
            LengthUnit::Ft => "ft",
            LengthUnit::FtIn => "ft-in",
        }
    }

    /// Millimeters in one unit. Feet and inches count inches.
    pub fn mm_per_unit(self) -> f64 {
        match self {
            LengthUnit::Mm => 1.0,
            LengthUnit::Cm => 10.0,
            LengthUnit::M => 1000.0,
            LengthUnit::In | LengthUnit::FtIn => 25.4,
            LengthUnit::Ft => 304.8,
        }
    }

    /// True for inches, feet, and feet with inches.
    pub fn is_imperial(self) -> bool {
        matches!(self, LengthUnit::In | LengthUnit::Ft | LengthUnit::FtIn)
    }

    /// A good number of decimals for this unit when no fractions are in use.
    pub fn default_decimals(self) -> usize {
        match self {
            LengthUnit::Mm => 1,
            LengthUnit::Cm => 2,
            LengthUnit::M => 3,
            LengthUnit::In => 2,
            LengthUnit::Ft => 3,
            LengthUnit::FtIn => 2,
        }
    }
}

/// A length in feet and inches in the form of a drawing text: `3'-4.5"`, with a dot and
/// without fractions. `decimals` is the number of decimals of the inches.
pub fn format_ftin_drawing(mm: f64, decimals: usize) -> String {
    let total = mm / 25.4;
    let sign = if total < 0.0 { "-" } else { "" };
    let total = total.abs();
    let mut feet = (total / 12.0).floor();
    let mut inches = round_half_away(total - feet * 12.0, decimals);
    if inches >= 12.0 {
        feet += 1.0;
        inches -= 12.0;
    }
    if feet == 0.0 {
        format!("{sign}{inches:.decimals$}\"")
    } else {
        format!("{sign}{feet:.0}'-{inches:.decimals$}\"")
    }
}

/// A length as a number in the unit, as a drawing text does: fixed decimals with a point, the
/// suffix of the unit, and no sign for a negative zero. Not for feet and inches.
pub fn format_drawing(mm: f64, unit: LengthUnit, decimals: usize, show_unit: bool) -> String {
    if unit == LengthUnit::FtIn {
        return format_ftin_drawing(mm, decimals);
    }
    let n = trim_negative_zero(format!(
        "{:.decimals$}",
        round_half_away(mm / unit.mm_per_unit(), decimals)
    ));
    if show_unit {
        format!("{n}{}", unit.suffix())
    } else {
        n
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Inches as text: whole inches and a reduced fraction (`6 1/2`, `1/2`, `6`) when
/// `fraction` (the denominator, for example 16) is more than zero, or decimal inches.
/// Returns the text, and the whole number of inches in it (so the caller can carry to feet).
/// `inches` is not negative.
fn inches_text(inches: f64, fraction: u32, decimals: usize, nf: &NumberFormat) -> (f64, String) {
    if fraction == 0 {
        let r = round_half_away(inches, decimals);
        return (r, nf.plain().format_trim(r, decimals));
    }
    let den = u64::from(fraction);
    let n = (inches * den as f64 + 0.5).floor() as u64;
    let (whole, rem) = (n / den, n % den);
    let text = if rem == 0 {
        whole.to_string()
    } else {
        let g = gcd(rem, den);
        if whole == 0 {
            format!("{}/{}", rem / g, den / g)
        } else {
            format!("{whole} {}/{}", rem / g, den / g)
        }
    };
    (n as f64 / den as f64, text)
}

/// A length for people to read, in the unit, with the separators of `nf`.
///
/// - Metric units give the number with `decimals` decimals and the suffix: `1 234,5 mm`.
/// - Inches give `12 1/2"` with a fraction denominator (`fraction`, for example 16), or `12.5"`
///   when `fraction` is 0. Feet give `5.5'`.
/// - Feet and inches give `5' 6 1/2"`, `6 1/2"` below one foot, and `5' 0"` for whole feet.
pub fn format_length(
    mm: f64,
    unit: LengthUnit,
    decimals: usize,
    fraction: u32,
    nf: &NumberFormat,
) -> String {
    if !mm.is_finite() {
        return "\u{2013}".to_string();
    }
    match unit {
        LengthUnit::Mm | LengthUnit::Cm | LengthUnit::M => {
            format!(
                "{}{}",
                nf.format(mm / unit.mm_per_unit(), decimals),
                unit.suffix()
            )
        }
        LengthUnit::Ft => {
            format!(
                "{}'",
                nf.format_trim(mm / unit.mm_per_unit(), decimals.max(1))
            )
        }
        LengthUnit::In => {
            let total = (mm / 25.4).abs();
            let (shown, text) = inches_text(total, fraction, decimals, nf);
            let sign = if mm < 0.0 && shown > 0.0 { "-" } else { "" };
            format!("{sign}{text}\"")
        }
        LengthUnit::FtIn => {
            let total = (mm / 25.4).abs();
            // Work out the rounded inches first, then split them into feet and inches.
            let (shown, _) = inches_text(total, fraction, decimals, nf);
            let feet = (shown / 12.0).floor();
            let rest = (shown - feet * 12.0).max(0.0);
            let (_, text) = inches_text(rest, fraction, decimals, nf);
            let sign = if mm < 0.0 && shown > 0.0 { "-" } else { "" };
            if feet == 0.0 {
                format!("{sign}{text}\"")
            } else if text.contains('/') && !text.contains(' ') {
                // Feet and a bare fraction: show the zero inches, `4' 0 5/8"`.
                format!("{sign}{feet:.0}' 0 {text}\"")
            } else {
                format!("{sign}{feet:.0}' {text}\"")
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Parsing

/// A unit that a typed length can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    Mm,
    Cm,
    M,
    In,
    Ft,
}

impl Marker {
    fn mm(self) -> f64 {
        match self {
            Marker::Mm => 1.0,
            Marker::Cm => 10.0,
            Marker::M => 1000.0,
            Marker::In => 25.4,
            Marker::Ft => 304.8,
        }
    }
}

fn marker_of(unit: LengthUnit) -> Marker {
    match unit {
        LengthUnit::Mm => Marker::Mm,
        LengthUnit::Cm => Marker::Cm,
        LengthUnit::M => Marker::M,
        LengthUnit::In | LengthUnit::FtIn => Marker::In,
        LengthUnit::Ft => Marker::Ft,
    }
}

struct Scanner<'a> {
    c: Vec<char>,
    i: usize,
    nf: &'a NumberFormat,
}

impl Scanner<'_> {
    fn peek(&self, k: usize) -> Option<char> {
        self.c.get(self.i + k).copied()
    }

    /// True for a character that groups digits in this format. A plain space groups only in the
    /// space format. The Swiss apostrophe groups only in the apostrophe format, and never in
    /// `skip_spaces`: elsewhere `’` is the foot mark (`5’6”`).
    fn is_group(&self, ch: char) -> bool {
        use crate::number::Grouping;
        match ch {
            '\'' | '\u{2019}' => self.nf.grouping == Grouping::Apostrophe,
            ' ' => self.nf.grouping == Grouping::Space,
            c => is_group_space(c),
        }
    }

    fn skip_spaces(&mut self) {
        while self.peek(0).is_some_and(is_group_space) {
            self.i += 1;
        }
    }

    fn digits(&mut self) -> String {
        let mut s = String::new();
        while let Some(ch) = self.peek(0).filter(char::is_ascii_digit) {
            s.push(ch);
            self.i += 1;
        }
        s
    }

    /// A number, a fraction (`1/2`) or a mixed number (`6 1/2`). The value is in the unit that
    /// follows it.
    fn quantity(&mut self) -> Option<(f64, bool)> {
        let start = self.i;
        while self
            .peek(0)
            .is_some_and(|ch| ch.is_ascii_digit() || ch == '.' || ch == ',')
        {
            self.i += 1;
        }
        // An exponent: `1e3`.
        if matches!(self.peek(0), Some('e' | 'E'))
            && (self.peek(1).is_some_and(|c| c.is_ascii_digit())
                || (matches!(self.peek(1), Some('+' | '-'))
                    && self.peek(2).is_some_and(|c| c.is_ascii_digit())))
        {
            self.i += 2;
            while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        // A group of three digits after a space belongs to the number: `1 234,5`.
        loop {
            let sep = self.peek(0);
            let three = (1..=3).all(|k| self.peek(k).is_some_and(|ch| ch.is_ascii_digit()));
            let group = sep.is_some_and(|ch| self.is_group(ch));
            if group
                && three
                && !self
                    .peek(4)
                    .is_some_and(|ch| ch.is_ascii_digit() || ch == '/')
                && self.c[start..self.i]
                    .iter()
                    .all(|ch| ch.is_ascii_digit() || self.is_group(*ch))
            {
                self.i += 4;
                while self
                    .peek(0)
                    .is_some_and(|ch| ch.is_ascii_digit() || ch == '.' || ch == ',')
                {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
        if self.i == start {
            return None;
        }
        let text: String = self.c[start..self.i].iter().collect();
        let whole_text = text.clone();
        let int_only = whole_text.chars().all(|ch| ch.is_ascii_digit());
        // A fraction right after the number: `3/4`.
        if self.peek(0) == Some('/') {
            if !int_only {
                return None;
            }
            self.i += 1;
            let den: f64 = self.digits().parse().ok()?;
            let num: f64 = whole_text.parse().ok()?;
            return (den > 0.0).then_some((num / den, true));
        }
        // A fraction after spaces: `6 1/2`.
        if int_only {
            let save = self.i;
            self.skip_spaces();
            if self.i > save {
                let num = self.digits();
                if !num.is_empty() && self.peek(0) == Some('/') {
                    self.i += 1;
                    let den: f64 = self.digits().parse().ok()?;
                    let whole: f64 = whole_text.parse().ok()?;
                    let num: f64 = num.parse().ok()?;
                    return (den > 0.0).then_some((whole + num / den, true));
                }
            }
            self.i = save;
        }
        self.nf.parse(&text).map(|v| (v, false))
    }

    /// The unit after a quantity. `Ok(None)`: no unit there. `Err`: something that is no unit.
    fn marker(&mut self) -> Result<Option<Marker>, ()> {
        self.skip_spaces();
        match self.peek(0) {
            Some('"' | '\u{2033}' | '\u{201d}') => {
                self.i += 1;
                Ok(Some(Marker::In))
            }
            Some('\'' | '\u{2032}' | '\u{2019}') => {
                if matches!(self.peek(1), Some('\'' | '\u{2019}')) {
                    self.i += 2;
                    Ok(Some(Marker::In))
                } else {
                    self.i += 1;
                    Ok(Some(Marker::Ft))
                }
            }
            Some(ch) if ch.is_alphabetic() => {
                let mut word = String::new();
                while let Some(ch) = self.peek(0).filter(|ch| ch.is_alphabetic()) {
                    word.extend(ch.to_lowercase());
                    self.i += 1;
                }
                if self.peek(0) == Some('.') && !self.peek(1).is_some_and(|ch| ch.is_ascii_digit())
                {
                    self.i += 1;
                }
                match word.as_str() {
                    "mm" => Ok(Some(Marker::Mm)),
                    "cm" => Ok(Some(Marker::Cm)),
                    "m" => Ok(Some(Marker::M)),
                    "in" | "inch" | "inches" => Ok(Some(Marker::In)),
                    "ft" | "foot" | "feet" => Ok(Some(Marker::Ft)),
                    _ => Err(()),
                }
            }
            _ => Ok(None),
        }
    }
}

/// Reads a length that a person typed and gives millimeters.
///
/// A number without a unit counts in `default` (inches for feet and inches). A number can name
/// its unit: `5 m`, `50cm`, `12 mm`, `3'`, `6"`, `4 ft`, `2 in`. Feet and inches can be written
/// together, with or without a dash, and inches can have a fraction: `5' 6 1/2"`, `5'-6"`,
/// `5ft 6in`, `6.5"`, `1/2"`. A number after feet without a unit counts as inches (`5' 6`).
/// The number follows `nf`, so with a decimal comma `1,5 m` is one and a half meters. A sign in
/// front makes the whole length negative.
pub fn parse_length(text: &str, default: LengthUnit, nf: &NumberFormat) -> Option<f64> {
    let t = text.trim();
    let (neg, rest) = match t.chars().next()? {
        '-' | '\u{2212}' => (true, &t[t.chars().next()?.len_utf8()..]),
        '+' => (false, &t[1..]),
        _ => (false, t),
    };
    let mut s = Scanner {
        c: rest.chars().collect(),
        i: 0,
        nf,
    };
    let mut total = 0.0;
    let mut previous: Option<Marker> = None;
    let mut first = true;
    loop {
        // `5'-6"`: a dash right after the unit is the separator. A dash after a space is a
        // sign, and a sign belongs only at the start.
        if !first && s.peek(0) == Some('-') && s.peek(1).is_some_and(|ch| ch.is_ascii_digit()) {
            s.i += 1;
        } else {
            s.skip_spaces();
        }
        if s.peek(0).is_none() {
            break;
        }
        let (value, fractional) = s.quantity()?;
        let unit = match s.marker().ok()? {
            Some(m) => m,
            None if first => marker_of(default),
            None if previous == Some(Marker::Ft) => Marker::In,
            None => return None,
        };
        // Whole feet only (`5' 6 1/2"`, not `5 1/2'`), and the units go from large to small.
        if fractional && unit == Marker::Ft {
            return None;
        }
        if let Some(p) = previous
            && !matches!(
                (p, unit),
                (Marker::Ft, Marker::In)
                    | (Marker::M, Marker::Cm)
                    | (Marker::M, Marker::Mm)
                    | (Marker::Cm, Marker::Mm)
            )
        {
            return None;
        }
        total += value * unit.mm();
        previous = Some(unit);
        first = false;
    }
    if first || !total.is_finite() {
        return None;
    }
    Some(if neg { -total } else { total })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::number::{Decimal, Grouping};

    const POINT: NumberFormat = NumberFormat {
        decimal: Decimal::Point,
        grouping: Grouping::None,
    };
    const ET: NumberFormat = NumberFormat {
        decimal: Decimal::Comma,
        grouping: Grouping::Space,
    };

    fn f(mm: f64, unit: LengthUnit) -> String {
        format_length(mm, unit, unit.default_decimals(), 16, &POINT)
    }

    #[test]
    fn the_unit_table_is_complete() {
        for u in LengthUnit::ALL {
            assert!(u.mm_per_unit() > 0.0);
            assert!(!u.short_name().is_empty());
        }
        assert!(LengthUnit::FtIn.is_imperial());
        assert!(!LengthUnit::M.is_imperial());
        assert_eq!(LengthUnit::Ft.suffix(), "'");
        assert_eq!(LengthUnit::Mm.default_decimals(), 1);
        assert_eq!(LengthUnit::M.default_decimals(), 3);
        assert_eq!(LengthUnit::Cm.default_decimals(), 2);
        assert_eq!(LengthUnit::In.default_decimals(), 2);
        assert_eq!(LengthUnit::Ft.default_decimals(), 3);
        assert_eq!(LengthUnit::FtIn.default_decimals(), 2);
    }

    #[test]
    fn metric_lengths() {
        assert_eq!(f(1234.56, LengthUnit::Mm), "1234.6 mm");
        assert_eq!(f(1234.56, LengthUnit::Cm), "123.46 cm");
        assert_eq!(f(1234.56, LengthUnit::M), "1.235 m");
        assert_eq!(
            format_length(1234567.0, LengthUnit::Mm, 1, 16, &ET),
            "1\u{a0}234\u{a0}567,0 mm"
        );
        assert_eq!(f(f64::NAN, LengthUnit::Mm), "\u{2013}");
    }

    #[test]
    fn imperial_lengths() {
        assert_eq!(f(25.4 * 6.5, LengthUnit::In), "6 1/2\"");
        assert_eq!(f(25.4 * 6.0, LengthUnit::In), "6\"");
        assert_eq!(f(25.4 * 0.5, LengthUnit::In), "1/2\"");
        assert_eq!(f(-25.4 * 0.5, LengthUnit::In), "-1/2\"");
        assert_eq!(f(304.8 * 5.5, LengthUnit::Ft), "5.5'");
        assert_eq!(f(25.4 * 66.5, LengthUnit::FtIn), "5' 6 1/2\"");
        assert_eq!(f(25.4 * 60.0, LengthUnit::FtIn), "5' 0\"");
        assert_eq!(f(25.4 * 6.5, LengthUnit::FtIn), "6 1/2\"");
        assert_eq!(f(0.0, LengthUnit::FtIn), "0\"");
        assert_eq!(f(-25.4 * 66.5, LengthUnit::FtIn), "-5' 6 1/2\"");
        // Rounding carries to the next foot.
        assert_eq!(f(25.4 * 71.99, LengthUnit::FtIn), "6' 0\"");
        // A fraction is reduced.
        assert_eq!(f(25.4 * 6.25, LengthUnit::In), "6 1/4\"");
        assert_eq!(
            format_length(25.4 * 6.5, LengthUnit::In, 2, 64, &POINT),
            "6 1/2\""
        );
        // A tiny negative length shows no sign.
        assert_eq!(f(-0.001, LengthUnit::FtIn), "0\"");
        assert_eq!(f(-0.001, LengthUnit::In), "0\"");
    }

    #[test]
    fn imperial_lengths_with_decimal_inches() {
        let g = |mm: f64, u| format_length(mm, u, 2, 0, &ET);
        assert_eq!(g(25.4 * 6.5, LengthUnit::In), "6,5\"");
        assert_eq!(g(25.4 * 66.5, LengthUnit::FtIn), "5' 6,5\"");
        assert_eq!(g(25.4 * 71.999, LengthUnit::FtIn), "6' 0\"");
    }

    #[test]
    fn the_drawing_form_is_kept() {
        assert_eq!(format_ftin_drawing(25.4 * 40.5, 1), "3'-4.5\"");
        assert_eq!(format_ftin_drawing(25.4 * 6.0, 0), "6\"");
        assert_eq!(format_ftin_drawing(-25.4 * 40.5, 1), "-3'-4.5\"");
        assert_eq!(format_ftin_drawing(25.4 * 143.99, 1), "12'-0.0\"");
        assert_eq!(format_drawing(1500.0, LengthUnit::M, 2, true), "1.50 m");
        assert_eq!(format_drawing(-0.001, LengthUnit::Mm, 1, false), "0.0");
        assert_eq!(format_drawing(304.8, LengthUnit::Ft, 1, true), "1.0'");
        assert_eq!(
            format_drawing(25.4 * 40.5, LengthUnit::FtIn, 1, true),
            "3'-4.5\""
        );
    }

    fn p(s: &str) -> Option<f64> {
        parse_length(s, LengthUnit::Mm, &POINT)
    }

    #[test]
    fn parses_metric_text() {
        assert_eq!(p("12"), Some(12.0));
        assert_eq!(p("1.5 m"), Some(1500.0));
        assert_eq!(p("1,5 m"), Some(1500.0));
        assert_eq!(p("50cm"), Some(500.0));
        assert_eq!(p("12 MM"), Some(12.0));
        assert_eq!(p("-2 m"), Some(-2000.0));
        assert_eq!(p("\u{2212}2 m"), Some(-2000.0));
        assert_eq!(p("+2 m"), Some(2000.0));
        assert_eq!(parse_length("1,5", LengthUnit::M, &ET), Some(1500.0));
        assert_eq!(
            parse_length("1 234,5 mm", LengthUnit::Mm, &ET),
            Some(1234.5)
        );
        assert_eq!(
            parse_length("1\u{a0}234,5", LengthUnit::Mm, &ET),
            Some(1234.5)
        );
        assert_eq!(parse_length("2 m 30 cm", LengthUnit::Mm, &ET), Some(2300.0));
    }

    /// All the number formats that the program offers.
    fn all_formats() -> Vec<NumberFormat> {
        use crate::number::{Decimal, Grouping};
        [
            (Decimal::Point, Grouping::None),
            (Decimal::Point, Grouping::Comma),
            (Decimal::Comma, Grouping::Point),
            (Decimal::Comma, Grouping::Space),
            (Decimal::Point, Grouping::Apostrophe),
            (Decimal::Comma, Grouping::None),
        ]
        .into_iter()
        .map(|(decimal, grouping)| NumberFormat { decimal, grouping })
        .collect()
    }

    #[test]
    fn the_typographic_apostrophe_is_the_foot_mark_in_every_format() {
        // `5’` was 1524 mm before the Swiss grouping existed. It must stay so, in all formats.
        let inch = 25.4;
        for nf in all_formats() {
            let q = |s: &str, unit| parse_length(s, unit, &nf);
            let near = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-9);
            assert!(near(q("5\u{2019}", LengthUnit::Mm), 1524.0), "{nf:?} 5’");
            assert!(
                near(q("5\u{2019}6\u{201d}", LengthUnit::Mm), 1524.0 + 6.0 * inch),
                "{nf:?} 5’6”"
            );
            assert!(
                near(
                    q("5\u{2019}-6\u{201d}", LengthUnit::Mm),
                    1524.0 + 6.0 * inch
                ),
                "{nf:?} 5’-6”"
            );
            assert!(
                near(
                    q("5\u{2019} 6\u{201d}", LengthUnit::FtIn),
                    1524.0 + 6.0 * inch
                ),
                "{nf:?}"
            );
            assert!(
                near(
                    q("5\u{2019}6\u{2019}\u{2019}", LengthUnit::Mm),
                    1524.0 + 6.0 * inch
                ),
                "{nf:?} 5’6’’"
            );
            // The plain apostrophe too.
            assert!(near(q("5'", LengthUnit::Mm), 1524.0), "{nf:?} 5'");
        }
    }

    #[test]
    fn the_swiss_apostrophe_groups_digits_in_the_swiss_format_only() {
        use crate::number::{Decimal, Grouping};
        let swiss = NumberFormat {
            decimal: Decimal::Point,
            grouping: Grouping::Apostrophe,
        };
        assert_eq!(
            parse_length("1\u{2019}234 mm", LengthUnit::Mm, &swiss),
            Some(1234.0)
        );
        assert_eq!(
            parse_length("1'234'567", LengthUnit::Mm, &swiss),
            Some(1_234_567.0)
        );
        for nf in all_formats()
            .into_iter()
            .filter(|f| f.grouping != Grouping::Apostrophe)
        {
            // Not a number: no format but the Swiss one groups with an apostrophe.
            assert_eq!(nf.parse("1\u{2019}234"), None, "{nf:?}");
            assert_eq!(nf.parse("1'234"), None, "{nf:?}");
            // In a length it is one foot and 234 inches, not 1234.
            let v = parse_length("1\u{2019}234", LengthUnit::Mm, &nf);
            assert!(
                v.is_some_and(|v| (v - (304.8 + 234.0 * 25.4)).abs() < 1e-6),
                "{nf:?} {v:?}"
            );
        }
    }

    #[test]
    fn parses_imperial_text() {
        let ft = LengthUnit::FtIn;
        let q = |s: &str| parse_length(s, ft, &POINT);
        let inch = 25.4;
        let close = |a: Option<f64>, b: f64| (a.unwrap() - b).abs() < 1e-9;
        assert!(close(q("5' 6 1/2\""), 304.8 * 5.0 + inch * 6.5));
        assert!(close(q("5'-6\""), 304.8 * 5.0 + inch * 6.0));
        assert!(close(q("5ft 6in"), 304.8 * 5.0 + inch * 6.0));
        assert!(close(q("5' 6"), 304.8 * 5.0 + inch * 6.0));
        assert!(close(q("5'"), 304.8 * 5.0));
        assert!(close(q("5\u{2032} 6\u{2033}"), 304.8 * 5.0 + inch * 6.0));
        assert!(close(q("5 feet 6 inches"), 304.8 * 5.0 + inch * 6.0));
        assert!(close(q("6.5\""), inch * 6.5));
        assert!(close(q("6.5"), inch * 6.5));
        assert!(close(q("1/2\""), inch * 0.5));
        assert!(close(q("6 1/2"), inch * 6.5));
        assert!(close(q("6 1/2''"), inch * 6.5));
        assert!(close(q("-5' 6\""), -(304.8 * 5.0 + inch * 6.0)));
        assert!(close(
            parse_length("5' 6,5\"", ft, &ET),
            304.8 * 5.0 + inch * 6.5
        ));
        assert!(close(parse_length("2", LengthUnit::Ft, &POINT), 609.6));
        assert!(close(parse_length("2", LengthUnit::In, &POINT), 50.8));
        assert!(close(parse_length("2", LengthUnit::Cm, &POINT), 20.0));
        // Metric and imperial units mix, and any active unit accepts any marker.
        assert!(close(p("3'"), 914.4));
    }

    #[test]
    fn exponents_orders_and_dashes() {
        let ft = LengthUnit::FtIn;
        assert_eq!(p("1e3"), Some(1000.0));
        assert_eq!(p("2.5e-1 m"), Some(250.0));
        assert_eq!(
            parse_length("5'-6\"", ft, &POINT),
            Some(304.8 * 5.0 + 25.4 * 6.0)
        );
        assert_eq!(
            parse_length("2 m 30 cm 5 mm", LengthUnit::Mm, &POINT),
            Some(2305.0)
        );
        // A bare fraction of an inch below a foot shows the zero inches.
        assert_eq!(
            f(304.8 * 4.0 + 25.4 * 0.625, LengthUnit::FtIn),
            "4' 0 5/8\""
        );
        assert_eq!(
            parse_length("4' 0 5/8\"", ft, &POINT),
            Some(304.8 * 4.0 + 25.4 * 0.625)
        );
    }

    #[test]
    fn bad_text_is_refused() {
        for bad in [
            "",
            " ",
            "-",
            "abc",
            "5 parsecs",
            "5 6",
            "5' 6 1/0\"",
            "1/0",
            "1.5/2",
            "5 m 6",
            "m",
            "5' 6 1/\"",
            "5' -6\"",
            "6\" 5'",
            "5 1/2'",
            "1e3x",
            "nan",
            "5 6 7",
        ] {
            assert_eq!(p(bad), None, "{bad:?}");
            assert_eq!(parse_length(bad, LengthUnit::FtIn, &ET), None, "{bad:?}");
        }
    }
}
