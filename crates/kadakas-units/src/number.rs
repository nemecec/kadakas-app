//! Plain numbers: rounding, decimal separator, digit grouping, and parsing of what a person types.

use serde::{Deserialize, Serialize};

/// Rounds to a number of decimals, halves away from zero (2.5 gives 3, -2.5 gives -3).
pub fn round_half_away(x: f64, decimals: usize) -> f64 {
    let k = 10f64.powi(decimals as i32);
    let r = (x * k).round() / k;
    if r.is_finite() { r } else { x }
}

/// Removes the sign of a text that is only a negative zero, for example `-0.0` becomes `0.0`.
pub fn trim_negative_zero(s: String) -> String {
    if s.starts_with('-') && s[1..].chars().all(|c| !c.is_ascii_digit() || c == '0') {
        s[1..].to_string()
    } else {
        s
    }
}

/// The character between the whole part and the fraction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decimal {
    /// 1234.5
    #[default]
    Point,
    /// 1234,5
    Comma,
}

impl Decimal {
    pub fn symbol(self) -> char {
        match self {
            Decimal::Point => '.',
            Decimal::Comma => ',',
        }
    }
}

/// The separator between groups of three digits in the whole part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    /// 1234
    #[default]
    None,
    /// 1,234
    Comma,
    /// 1.234
    Point,
    /// 1 234 (a no-break space)
    Space,
    /// 1’234 (Swiss German and Swiss Italian)
    Apostrophe,
}

impl Grouping {
    /// The separator, or `None` for no grouping.
    pub fn symbol(self) -> Option<char> {
        match self {
            Grouping::None => None,
            Grouping::Comma => Some(','),
            Grouping::Point => Some('.'),
            Grouping::Space => Some('\u{a0}'),
            Grouping::Apostrophe => Some('\u{2019}'),
        }
    }
}

/// True for the spaces that people and programs use between groups of digits.
pub(crate) fn is_group_space(c: char) -> bool {
    matches!(c, ' ' | '\u{a0}' | '\u{202f}' | '\u{2009}')
}

/// How numbers look: the decimal separator and the digit grouping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NumberFormat {
    pub decimal: Decimal,
    pub grouping: Grouping,
}

impl NumberFormat {
    /// The same format without digit grouping. Text fields that people edit use it.
    pub fn plain(self) -> NumberFormat {
        NumberFormat {
            grouping: Grouping::None,
            ..self
        }
    }

    /// A number with exactly `decimals` decimals, rounded half away from zero. A value that is
    /// not finite gives an en dash.
    pub fn format(&self, v: f64, decimals: usize) -> String {
        if !v.is_finite() {
            return "\u{2013}".to_string();
        }
        let r = round_half_away(v, decimals);
        let fixed = format!("{:.*}", decimals, r.abs());
        let (int, frac) = match fixed.split_once('.') {
            Some((i, f)) => (i, Some(f)),
            None => (fixed.as_str(), None),
        };
        let mut out = String::new();
        if r < 0.0 {
            out.push('-');
        }
        match self.grouping.symbol() {
            Some(sep) => {
                let n = int.len();
                for (k, ch) in int.chars().enumerate() {
                    if k > 0 && (n - k) % 3 == 0 {
                        out.push(sep);
                    }
                    out.push(ch);
                }
            }
            None => out.push_str(int),
        }
        if let Some(f) = frac {
            out.push(self.decimal.symbol());
            out.push_str(f);
        }
        out
    }

    /// Like `format`, but without trailing zeros: 2.50 gives `2.5` and 3.00 gives `3`.
    pub fn format_trim(&self, v: f64, max_decimals: usize) -> String {
        let s = self.format(v, max_decimals);
        let d = self.decimal.symbol();
        if s.contains(d) {
            s.trim_end_matches('0').trim_end_matches(d).to_string()
        } else {
            s
        }
    }

    /// Reads a number that a person typed.
    ///
    /// The active separators count first. The parser is also kind: a comma or a point that can
    /// only be the decimal separator is accepted in either format (`1,5` and `1.5` are both
    /// one and a half), groups of three digits may be separated by the active grouping or by a
    /// space, and when both symbols appear, the last one is the decimal separator.
    /// Text with letters, an empty text and a value that is not finite give `None`.
    pub fn parse(&self, text: &str) -> Option<f64> {
        let t = text.trim();
        let (neg, body) = match t.chars().next()? {
            '-' | '\u{2212}' => (true, &t[t.chars().next()?.len_utf8()..]),
            '+' => (false, &t[1..]),
            _ => (false, t),
        };
        let body = body.trim_start();
        let v = self.parse_unsigned(body)?;
        let v = if neg { -v } else { v };
        v.is_finite().then_some(v)
    }

    /// True for a number like `1,500` that this format reads as a number with groups (when the
    /// grouping symbol is the comma, `1,500` is 1500), while a format with a decimal comma would
    /// read it as 1.5. A person who types it may mean the other one.
    pub fn is_ambiguous(&self, text: &str) -> bool {
        let Some(sep) = self.grouping.symbol().filter(|s| *s == ',' || *s == '.') else {
            return false;
        };
        let t = text.trim();
        t.matches(sep).count() == 1
            && t.chars().all(|c| c.is_ascii_digit() || c == sep)
            && valid_groups(t, sep)
            && self.parse(t).is_some_and(|v| v >= 1000.0)
    }

    fn parse_unsigned(&self, body: &str) -> Option<f64> {
        if body.is_empty() {
            return None;
        }
        // An exponent: `1e3`, `2,5E-2`.
        if let Some(at) = body.find(['e', 'E']) {
            let exp: i32 = body[at + 1..].parse().ok()?;
            let mantissa = self.parse_unsigned(&body[..at])?;
            let v = mantissa * 10f64.powi(exp.clamp(-400, 400));
            return v.is_finite().then_some(v);
        }
        // The Swiss grouping (`1’234`, often typed with the plain apostrophe) counts as a group
        // symbol in this format only. In every other format `'` and `’` are the foot mark and
        // not part of a number.
        let swiss;
        let body = if self.grouping == Grouping::Apostrophe && body.contains(['\'', '\u{2019}']) {
            swiss = body.replace(['\'', '\u{2019}'], "\u{a0}");
            swiss.as_str()
        } else {
            body
        };
        // Spaces between groups of digits.
        let joined;
        let body = if body.chars().any(is_group_space) {
            let parts: Vec<&str> = body.split(is_group_space).collect();
            let digits = |p: &str, n: Option<usize>| {
                !p.is_empty()
                    && p.chars().all(|c| c.is_ascii_digit())
                    && n.is_none_or(|n| p.len() == n)
            };
            let last = parts.last()?;
            let last_int = last.split(['.', ',']).next().unwrap_or_default();
            let ok = parts.len() > 1
                && parts[0].len() <= 3
                && digits(parts[0], None)
                && parts[1..parts.len() - 1].iter().all(|p| digits(p, Some(3)))
                && digits(last_int, Some(3));
            if !ok {
                return None;
            }
            joined = parts.concat();
            joined.as_str()
        } else {
            body
        };
        if !body
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
        {
            return None;
        }
        let d = self.decimal.symbol();
        let o = if d == '.' { ',' } else { '.' };
        let count = |c: char| body.chars().filter(|x| *x == c).count();
        let (nd, no) = (count(d), count(o));
        let (int, frac): (String, &str) = match (nd, no) {
            (0, 0) => (body.to_string(), ""),
            (1, 0) => split_at_last(body, d),
            (_, 0) => return None,
            (0, 1) => {
                // One symbol of the other kind: a group separator only when it is the active
                // grouping and exactly three digits follow it.
                if self.grouping.symbol() == Some(o) && valid_groups(body, o) {
                    (body.replace(o, ""), "")
                } else {
                    split_at_last(body, o)
                }
            }
            (0, _) => {
                if !valid_groups(body, o) {
                    return None;
                }
                (body.replace(o, ""), "")
            }
            _ => {
                // Both symbols: the last one is the decimal separator, the other one groups.
                let last = body.rfind(['.', ','])?;
                let sep = body[last..].chars().next()?;
                let g = if sep == d { o } else { d };
                if body.matches(sep).count() != 1 {
                    return None;
                }
                let (i, f) = body.split_at(last);
                if !valid_groups(i, g) {
                    return None;
                }
                (i.replace(g, ""), &f[1..])
            }
        };
        if int.is_empty() && frac.is_empty() {
            return None;
        }
        let text = format!(
            "{}.{}",
            if int.is_empty() { "0" } else { &int },
            if frac.is_empty() { "0" } else { frac }
        );
        text.parse().ok()
    }
}

fn split_at_last(body: &str, sep: char) -> (String, &str) {
    let at = body.rfind(sep).unwrap_or(0);
    (body[..at].to_string(), &body[at + 1..])
}

/// True if `s` is groups of digits that `sep` separates: one to three digits, then groups of
/// exactly three.
fn valid_groups(s: &str, sep: char) -> bool {
    let mut parts = s.split(sep);
    let Some(first) = parts.next() else {
        return false;
    };
    // A group of digits does not start with a zero: `0,500` is not 500.
    (1..=3).contains(&first.len()) && !first.starts_with('0') && parts.all(|p| p.len() == 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    const POINT: NumberFormat = NumberFormat {
        decimal: Decimal::Point,
        grouping: Grouping::None,
    };
    const COMMA: NumberFormat = NumberFormat {
        decimal: Decimal::Comma,
        grouping: Grouping::None,
    };
    const EN: NumberFormat = NumberFormat {
        decimal: Decimal::Point,
        grouping: Grouping::Comma,
    };
    const ET: NumberFormat = NumberFormat {
        decimal: Decimal::Comma,
        grouping: Grouping::Space,
    };
    const DE: NumberFormat = NumberFormat {
        decimal: Decimal::Comma,
        grouping: Grouping::Point,
    };

    #[test]
    fn the_swiss_apostrophe_groups_and_reads_back() {
        let ch = NumberFormat {
            decimal: Decimal::Point,
            grouping: Grouping::Apostrophe,
        };
        assert_eq!(ch.format(1234567.891, 2), "1\u{2019}234\u{2019}567.89");
        assert_eq!(ch.format(-1234.5, 1), "-1\u{2019}234.5");
        assert_eq!(ch.parse("1\u{2019}234.56"), Some(1234.56));
        assert_eq!(ch.parse("1'234'567"), Some(1234567.0));
        assert_eq!(ch.parse("12'34"), None);
    }

    #[test]
    fn rounding_goes_away_from_zero() {
        assert_eq!(round_half_away(2.5, 0), 3.0);
        assert_eq!(round_half_away(-2.5, 0), -3.0);
        assert_eq!(round_half_away(0.125, 2), 0.13);
        assert_eq!(round_half_away(f64::INFINITY, 2), f64::INFINITY);
        assert_eq!(trim_negative_zero("-0.00".into()), "0.00");
        assert_eq!(trim_negative_zero("-0.01".into()), "-0.01");
        assert_eq!(trim_negative_zero("1.5".into()), "1.5");
    }

    #[test]
    fn formats_with_separators() {
        assert_eq!(POINT.format(1234.5, 1), "1234.5");
        assert_eq!(COMMA.format(1234.5, 1), "1234,5");
        assert_eq!(EN.format(1234567.891, 2), "1,234,567.89");
        assert_eq!(ET.format(1234567.891, 2), "1\u{a0}234\u{a0}567,89");
        assert_eq!(DE.format(-1234.5, 1), "-1.234,5");
        assert_eq!(EN.format(999.0, 0), "999");
        assert_eq!(EN.format(1000.0, 0), "1,000");
        assert_eq!(EN.format(-0.001, 2), "0.00");
        assert_eq!(EN.format(f64::NAN, 2), "\u{2013}");
        assert_eq!(ET.plain().format(12345.0, 0), "12345");
    }

    #[test]
    fn trims_trailing_zeros() {
        assert_eq!(POINT.format_trim(2.5, 3), "2.5");
        assert_eq!(COMMA.format_trim(3.0, 3), "3");
        assert_eq!(COMMA.format_trim(2.25, 3), "2,25");
        assert_eq!(POINT.format_trim(100.0, 0), "100");
    }

    #[test]
    fn parses_the_active_format() {
        assert_eq!(COMMA.parse("1,5"), Some(1.5));
        assert_eq!(POINT.parse("1.5"), Some(1.5));
        assert_eq!(ET.parse("1 234,5"), Some(1234.5));
        assert_eq!(ET.parse("1\u{a0}234\u{a0}567,89"), Some(1234567.89));
        assert_eq!(EN.parse("1,234.5"), Some(1234.5));
        assert_eq!(DE.parse("1.234,5"), Some(1234.5));
        assert_eq!(EN.parse("-1,234,567"), Some(-1234567.0));
        assert_eq!(POINT.parse("\u{2212}3"), Some(-3.0));
        assert_eq!(POINT.parse("+3"), Some(3.0));
        assert_eq!(POINT.parse(" .5 "), Some(0.5));
        assert_eq!(POINT.parse("5."), Some(5.0));
        assert_eq!(POINT.parse("- 5"), Some(-5.0));
    }

    #[test]
    fn parsing_is_kind_to_the_other_symbol() {
        // A symbol that can only be the decimal separator counts as one.
        assert_eq!(POINT.parse("1,5"), Some(1.5));
        assert_eq!(COMMA.parse("1.5"), Some(1.5));
        assert_eq!(ET.parse("1.5"), Some(1.5));
        // The grouping symbol with three digits groups.
        assert_eq!(EN.parse("1,500"), Some(1500.0));
        assert_eq!(DE.parse("1.500"), Some(1500.0));
        // Both symbols: the last one is the decimal separator.
        assert_eq!(POINT.parse("1.234,5"), Some(1234.5));
        assert_eq!(COMMA.parse("1,234.5"), Some(1234.5));
    }

    #[test]
    fn bad_text_is_refused() {
        for bad in [
            "",
            " ",
            "-",
            "abc",
            "1e",
            "inf",
            "NaN",
            ".",
            ",",
            "1.2.3",
            "1,2,3",
            "1..2",
            "1 23",
            "12 3456",
            "1 234 56",
            "1.234,5,6",
            "1,23,456.7",
            "1.2,3.4",
            "--1",
            "5 mm",
        ] {
            assert_eq!(POINT.parse(bad), None, "{bad:?}");
            assert_eq!(ET.parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_settings_round_trip_through_json() {
        let json = serde_json::to_string(&ET).unwrap();
        assert_eq!(json, r#"{"decimal":"comma","grouping":"space"}"#);
        let back: NumberFormat = serde_json::from_str("{}").unwrap();
        assert_eq!(back, NumberFormat::default());
    }
}
