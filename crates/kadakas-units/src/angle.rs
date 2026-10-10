//! Angles in degrees, in three forms: decimal degrees, degrees-minutes-seconds, and bearings.
//!
//! The model angle is the math angle in degrees: zero along +X, counterclockwise. A bearing
//! (surveying) counts from north, so `N 45° E` is a math angle of 45°, and `S 80° W` is
//! 190° in math terms. A parsed bearing gives a math angle from 0 up to (not including) 360.

use serde::{Deserialize, Serialize};

use crate::number::{NumberFormat, round_half_away};

/// How angles are written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AngleFormat {
    /// 45.5°
    #[default]
    Decimal,
    /// 45°30'00"
    Dms,
    /// Quadrant bearing from north or south: N 45°30'00" E
    Bearing,
}

impl AngleFormat {
    pub const ALL: [AngleFormat; 3] =
        [AngleFormat::Decimal, AngleFormat::Dms, AngleFormat::Bearing];
}

/// Degrees, minutes and seconds of a non-negative angle, with the seconds rounded to
/// `decimals` and carried into minutes and degrees.
fn split_dms(deg: f64, decimals: usize) -> (u64, u64, f64) {
    let total = round_half_away(deg * 3600.0, decimals);
    let d = (total / 3600.0).floor();
    let rest = total - d * 3600.0;
    let m = (rest / 60.0).floor();
    let s = round_half_away(rest - m * 60.0, decimals);
    // Float noise can leave 59.9999 or 60 after the split.
    if s >= 60.0 {
        return (d as u64, m as u64 + 1, 0.0);
    }
    (d as u64, m as u64, s)
}

fn dms_text(deg: f64, decimals: usize, nf: &NumberFormat) -> String {
    let (d, mut m, mut s) = split_dms(deg, decimals);
    let mut d = d;
    if m >= 60 {
        d += 1;
        m -= 60;
        s = 0.0;
    }
    let secs = nf.plain().format(s, decimals);
    // Seconds below ten get a leading zero, so the text lines up.
    let secs = if s < 10.0 { format!("0{secs}") } else { secs };
    format!("{d}\u{b0}{m:02}'{secs}\"")
}

/// The bearing parts of a math angle: north or south, degrees in 0..=90, east or west.
fn bearing_parts(math_deg: f64) -> (char, f64, char) {
    let az = (90.0 - math_deg).rem_euclid(360.0);
    if az <= 90.0 {
        ('N', az, 'E')
    } else if az <= 180.0 {
        ('S', 180.0 - az, 'E')
    } else if az <= 270.0 {
        ('S', az - 180.0, 'W')
    } else {
        ('N', 360.0 - az, 'W')
    }
}

/// An angle in degrees as text. `decimals` is the number of decimals of the degrees (decimal
/// form) or of the seconds (DMS and bearing).
pub fn format_angle(deg: f64, format: AngleFormat, decimals: usize, nf: &NumberFormat) -> String {
    if !deg.is_finite() {
        return "\u{2013}".to_string();
    }
    match format {
        AngleFormat::Decimal => format!("{}\u{b0}", nf.format(deg, decimals)),
        AngleFormat::Dms => {
            let sign = if round_half_away(deg * 3600.0, decimals) < 0.0 {
                "-"
            } else {
                ""
            };
            format!("{sign}{}", dms_text(deg.abs(), decimals, nf))
        }
        AngleFormat::Bearing => {
            let (ns, a, ew) = bearing_parts(deg);
            format!("{ns} {} {ew}", dms_text(a, decimals, nf))
        }
    }
}

/// Reads one angle in decimal or DMS form: `45`, `45.5`, `45°`, `45°30'`, `45°30'15.5"`,
/// `45 30 15` is not accepted. The sign is not part of it.
fn parse_degrees(text: &str, nf: &NumberFormat) -> Option<f64> {
    let t = text.trim();
    let mut parts: Vec<(f64, char)> = Vec::new();
    let mut num = String::new();
    let flush = |num: &mut String, mark: char, parts: &mut Vec<(f64, char)>| -> Option<()> {
        let v = nf.parse(num)?;
        if v < 0.0 {
            return None;
        }
        parts.push((v, mark));
        num.clear();
        Some(())
    };
    for ch in t.chars() {
        match ch {
            '\u{b0}' | '\u{ba}' => flush(&mut num, 'd', &mut parts)?,
            '\'' | '\u{2032}' | '\u{2019}' => flush(&mut num, 'm', &mut parts)?,
            '"' | '\u{2033}' | '\u{201d}' => flush(&mut num, 's', &mut parts)?,
            ' ' | '\u{a0}' if num.is_empty() => {}
            _ => num.push(ch),
        }
    }
    let rest = num.trim();
    if !rest.is_empty() {
        // A trailing number without a mark: it is the degrees, and only if nothing came before.
        if !parts.is_empty() {
            return None;
        }
        flush(&mut num, 'd', &mut parts)?;
    }
    // The marks must come in the order d, m, s, each once. Only the last part can have decimals.
    let mut total = 0.0;
    let mut last_rank = None;
    for (k, (v, mark)) in parts.iter().enumerate() {
        let (rank, div) = match mark {
            'd' => (0, 1.0),
            'm' => (1, 60.0),
            _ => (2, 3600.0),
        };
        if last_rank.is_some_and(|l| l >= rank) {
            return None;
        }
        last_rank = Some(rank);
        if k + 1 < parts.len() && v.fract() != 0.0 {
            return None;
        }
        if rank > 0 && *v >= 60.0 {
            return None;
        }
        total += v / div;
    }
    (!parts.is_empty()).then_some(total)
}

/// Reads an angle that a person typed and gives math degrees. All three forms are accepted,
/// whatever form is active: `45,5`, `45,5°`, `-45°30'`, `N 45°30'00" E`, `s80w`. A bearing
/// is `N` or `S`, an angle from 0 to 90, and `E` or `W`.
pub fn parse_angle(text: &str, nf: &NumberFormat) -> Option<f64> {
    let t = text.trim();
    let first = t.chars().next()?;
    let v = if matches!(first.to_ascii_uppercase(), 'N' | 'S') {
        let ns = first.to_ascii_uppercase();
        let last = t.chars().last()?.to_ascii_uppercase();
        let inner = &t[1..t.len() - last.len_utf8().min(t.len() - 1)];
        let inner = if matches!(last, 'E' | 'W') {
            inner
        } else {
            return None;
        };
        let a = parse_degrees(inner, nf)?;
        if a > 90.0 {
            return None;
        }
        let az = match (ns, last) {
            ('N', 'E') => a,
            ('S', 'E') => 180.0 - a,
            ('S', 'W') => 180.0 + a,
            _ => 360.0 - a,
        };
        (90.0 - az).rem_euclid(360.0)
    } else {
        let (neg, body) = match first {
            '-' | '\u{2212}' => (true, &t[first.len_utf8()..]),
            '+' => (false, &t[1..]),
            _ => (false, t),
        };
        let v = parse_degrees(body, nf)?;
        if neg { -v } else { v }
    };
    v.is_finite().then_some(v)
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

    #[test]
    fn decimal_degrees() {
        assert_eq!(
            format_angle(45.5, AngleFormat::Decimal, 1, &POINT),
            "45.5\u{b0}"
        );
        assert_eq!(
            format_angle(-45.56, AngleFormat::Decimal, 1, &ET),
            "-45,6\u{b0}"
        );
        assert_eq!(format_angle(f64::NAN, AngleFormat::Dms, 1, &ET), "\u{2013}");
    }

    #[test]
    fn degrees_minutes_seconds() {
        let f = |d: f64, dec: usize| format_angle(d, AngleFormat::Dms, dec, &POINT);
        assert_eq!(f(45.5, 0), "45\u{b0}30'00\"");
        assert_eq!(f(45.504_166_666_7, 0), "45\u{b0}30'15\"");
        assert_eq!(f(-10.25, 0), "-10\u{b0}15'00\"");
        assert_eq!(f(0.0, 0), "0\u{b0}00'00\"");
        assert_eq!(f(45.504_166_666_7, 1), "45\u{b0}30'15.0\"");
        assert_eq!(f(45.5, 2), "45\u{b0}30'00.00\"");
        // 59.99 seconds rounds up and carries.
        assert_eq!(f(10.0 + 59.0 / 60.0 + 59.9 / 3600.0, 0), "11\u{b0}00'00\"");
        assert_eq!(f(10.0 + 59.99 / 3600.0, 0), "10\u{b0}01'00\"");
        assert_eq!(f(-0.000_01, 0), "0\u{b0}00'00\"");
        assert_eq!(
            format_angle(10.5, AngleFormat::Dms, 1, &ET),
            "10\u{b0}30'00,0\""
        );
    }

    #[test]
    fn bearings() {
        let f = |d: f64| format_angle(d, AngleFormat::Bearing, 0, &POINT);
        assert_eq!(f(45.0), "N 45\u{b0}00'00\" E");
        assert_eq!(f(90.0), "N 0\u{b0}00'00\" E");
        assert_eq!(f(0.0), "N 90\u{b0}00'00\" E");
        assert_eq!(f(-45.0), "S 45\u{b0}00'00\" E");
        assert_eq!(f(-100.0), "S 10\u{b0}00'00\" W");
        assert_eq!(f(180.0), "S 90\u{b0}00'00\" W");
        assert_eq!(f(190.0), "S 80\u{b0}00'00\" W");
        assert_eq!(f(135.0), "N 45\u{b0}00'00\" W");
        assert_eq!(f(270.0), "S 0\u{b0}00'00\" E");
    }

    #[test]
    fn parses_every_form() {
        let p = |s: &str| parse_angle(s, &ET);
        let close = |a: Option<f64>, b: f64| (a.unwrap() - b).abs() < 1e-9;
        assert_eq!(p("45,5"), Some(45.5));
        assert_eq!(p("45.5"), Some(45.5));
        assert_eq!(p("45,5\u{b0}"), Some(45.5));
        assert_eq!(p("-45\u{b0}30'"), Some(-45.5));
        assert_eq!(p("\u{2212}45\u{b0}"), Some(-45.0));
        assert_eq!(p("+10"), Some(10.0));
        assert!(close(p("10\u{b0} 30' 36\""), 10.51));
        assert!(close(p("10\u{b0}30'36\""), 10.51));
        assert!(close(p("10\u{b0}30'36,0\""), 10.51));
        assert!(close(p("0\u{b0}0'1\""), 1.0 / 3600.0));
        assert_eq!(p("N 45\u{b0} E"), Some(45.0));
        assert_eq!(p("n45e"), Some(45.0));
        assert!(close(p("S 10\u{b0}30' E"), 280.5));
        assert!(close(p("S80W"), 190.0));
        assert_eq!(p("N 45 W"), Some(135.0));
    }

    #[test]
    fn bad_angles_are_refused() {
        for bad in [
            "",
            " ",
            "abc",
            "N",
            "N E",
            "N 91 E",
            "N 45",
            "45 E",
            "X 45 E",
            "10'20\u{b0}",
            "10\u{b0}20\u{b0}",
            "10.5\u{b0}30'",
            "10\u{b0}60'",
            "10\u{b0}30'60\"",
            "1 2",
            "--5",
            "N -5 E",
            "N 45\u{b0}30'45\"15\" E",
            "nan",
        ] {
            assert_eq!(parse_angle(bad, &ET), None, "{bad:?}");
        }
    }

    #[test]
    fn all_formats_are_listed() {
        assert_eq!(AngleFormat::ALL.len(), 3);
    }
}
