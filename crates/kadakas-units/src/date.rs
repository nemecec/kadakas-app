//! Calendar dates and their written forms. Pure: there is no clock and no time zone here.

use serde::{Deserialize, Serialize};

/// A day in the proleptic Gregorian calendar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

/// True for a leap year.
pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days in a month, or 0 for a month that does not exist.
pub fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

impl Date {
    /// A valid date, or `None`.
    pub fn new(year: i32, month: u8, day: u8) -> Option<Date> {
        (1..=9999)
            .contains(&year)
            .then_some(())
            .filter(|_| day >= 1 && day <= days_in_month(year, month))
            .map(|_| Date { year, month, day })
    }

    /// The day number: days since 1970-01-01 (negative before).
    pub fn to_days(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.month);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The date of a day number (days since 1970-01-01).
    pub fn from_days(days: i64) -> Date {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        Date { year, month, day }
    }

    /// The UTC date of a Unix time in seconds.
    pub fn from_unix_secs(secs: i64) -> Date {
        Date::from_days(secs.div_euclid(86_400))
    }

    /// The weekday, 0 for Monday up to 6 for Sunday.
    pub fn weekday(self) -> u8 {
        // 1970-01-01 was a Thursday.
        (self.to_days() + 3).rem_euclid(7) as u8
    }
}

/// The order and separators of a written date.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateFormat {
    /// 2026-10-11
    #[default]
    Iso,
    /// 11.10.2026
    Dmy,
    /// 10/11/2026
    Mdy,
}

impl DateFormat {
    pub const ALL: [DateFormat; 3] = [DateFormat::Iso, DateFormat::Dmy, DateFormat::Mdy];

    /// The pattern as text, for a hint: `yyyy-mm-dd`.
    pub fn pattern(self) -> &'static str {
        match self {
            DateFormat::Iso => "yyyy-mm-dd",
            DateFormat::Dmy => "dd.mm.yyyy",
            DateFormat::Mdy => "mm/dd/yyyy",
        }
    }

    /// A date as text.
    pub fn format(self, d: Date) -> String {
        match self {
            DateFormat::Iso => format!("{:04}-{:02}-{:02}", d.year, d.month, d.day),
            DateFormat::Dmy => format!("{:02}.{:02}.{:04}", d.day, d.month, d.year),
            DateFormat::Mdy => format!("{:02}/{:02}/{:04}", d.month, d.day, d.year),
        }
    }

    /// Reads a date. A date that starts with a four digit year (`2026-10-11`, `2026/10/11`) is
    /// always read year first. Otherwise the order of the format applies. The separators `-`,
    /// `.` and `/` are all accepted, but one date uses one kind. A missing leading zero is fine.
    pub fn parse(self, text: &str) -> Option<Date> {
        let t = text.trim();
        let sep = ['-', '.', '/']
            .into_iter()
            .find(|s| t.contains(*s))
            .filter(|s| t.matches(*s).count() == 2)?;
        let parts: Vec<&str> = t.split(sep).map(str::trim).collect();
        if parts.len() != 3
            || parts
                .iter()
                .any(|p| p.is_empty() || p.len() > 4 || !p.chars().all(|c| c.is_ascii_digit()))
        {
            return None;
        }
        let n: Vec<i32> = parts.iter().map(|p| p.parse().unwrap_or(-1)).collect();
        let (y, m, d) = if parts[0].len() == 4 {
            (n[0], n[1], n[2])
        } else if parts[2].len() == 4 {
            match self {
                DateFormat::Mdy => (n[2], n[0], n[1]),
                _ => (n[2], n[1], n[0]),
            }
        } else {
            return None;
        };
        Date::new(y, u8::try_from(m).ok()?, u8::try_from(d).ok()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_and_dates_agree() {
        assert_eq!(Date::new(1970, 1, 1).unwrap().to_days(), 0);
        assert_eq!(Date::from_days(0), Date::new(1970, 1, 1).unwrap());
        assert_eq!(
            Date::from_unix_secs(1_760_140_800),
            Date::new(2025, 10, 11).unwrap()
        );
        assert_eq!(Date::from_unix_secs(-1), Date::new(1969, 12, 31).unwrap());
        assert_eq!(Date::new(2000, 3, 1).unwrap().to_days(), 11_017);
        assert_eq!(Date::new(1900, 3, 1).unwrap().to_days(), -25_508);
        for days in (-800_000..800_000).step_by(997) {
            assert_eq!(Date::from_days(days).to_days(), days);
        }
    }

    #[test]
    fn weekdays() {
        assert_eq!(Date::new(1970, 1, 1).unwrap().weekday(), 3);
        assert_eq!(Date::new(2026, 10, 11).unwrap().weekday(), 6);
        assert_eq!(Date::new(2024, 2, 29).unwrap().weekday(), 3);
    }

    #[test]
    fn validity() {
        assert!(Date::new(2024, 2, 29).is_some());
        assert!(Date::new(2023, 2, 29).is_none());
        assert!(Date::new(1900, 2, 29).is_none());
        assert!(Date::new(2000, 2, 29).is_some());
        assert!(Date::new(2026, 13, 1).is_none());
        assert!(Date::new(2026, 0, 1).is_none());
        assert!(Date::new(2026, 4, 31).is_none());
        assert!(Date::new(0, 1, 1).is_none());
        assert!(Date::new(2026, 1, 0).is_none());
        assert_eq!(days_in_month(2026, 13), 0);
    }

    #[test]
    fn formats_and_parses() {
        let d = Date::new(2026, 10, 1).unwrap();
        assert_eq!(DateFormat::Iso.format(d), "2026-10-01");
        assert_eq!(DateFormat::Dmy.format(d), "01.10.2026");
        assert_eq!(DateFormat::Mdy.format(d), "10/01/2026");
        for f in DateFormat::ALL {
            assert_eq!(f.parse(&f.format(d)), Some(d));
            assert!(!f.pattern().is_empty());
            // The ISO form is always understood.
            assert_eq!(f.parse("2026-10-01"), Some(d));
        }
        assert_eq!(DateFormat::Dmy.parse("1.10.2026"), Some(d));
        assert_eq!(DateFormat::Dmy.parse("1/10/2026"), Some(d));
        assert_eq!(DateFormat::Mdy.parse("10-1-2026"), Some(d));
        assert_eq!(DateFormat::Iso.parse("01.10.2026"), Some(d));
        assert_eq!(DateFormat::Mdy.parse(" 10 / 1 / 2026 "), Some(d));
    }

    #[test]
    fn bad_dates_are_refused() {
        for bad in [
            "",
            "2026",
            "2026-10",
            "2026-13-01",
            "2026-02-30",
            "26-10-11",
            "1.2.3",
            "a.b.2026",
            "01.10.2026.5",
            "2026-10.01",
            "-1.10.2026",
            "01..2026",
            "11111-1-1",
        ] {
            assert_eq!(DateFormat::Dmy.parse(bad), None, "{bad:?}");
        }
    }
}
