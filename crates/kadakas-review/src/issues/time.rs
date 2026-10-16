//! Times of issues: milliseconds since 1970 (UTC) and their text forms (ISO 8601, as BCF writes
//! them). The crate has no clock: a host passes the time.

/// Days from 1970-01-01 to a civil date (proleptic Gregorian calendar).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date (year, month, day) of a day number counted from 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `2026-10-15T12:30:00.250Z` for a time in milliseconds.
pub fn iso(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let rest = ms.rem_euclid(86_400_000);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3_600_000,
        rest / 60_000 % 60,
        rest / 1000 % 60,
        rest % 1000
    )
}

/// `2026-10-15` for a time in milliseconds (the UTC date).
pub fn date(ms: i64) -> String {
    let (y, m, d) = civil_from_days(ms.div_euclid(86_400_000));
    format!("{y:04}-{m:02}-{d:02}")
}

/// The time at the start (UTC) of a date `YYYY-MM-DD`.
pub fn from_date(text: &str) -> Option<i64> {
    if !kadakas_core::issue::is_date(text) {
        return None;
    }
    let n = |r: std::ops::Range<usize>| text[r].parse::<i64>().ok();
    Some(days_from_civil(n(0..4)?, n(5..7)?, n(8..10)?) * 86_400_000)
}

/// Reads an ISO 8601 date and time (`xs:dateTime`): `YYYY-MM-DDThh:mm:ss`, an optional fraction,
/// and `Z` or an offset `+hh:mm`. A text without an offset counts as UTC. A text with a date only
/// is accepted too. Returns milliseconds since 1970, or `None` for text that is not a date.
pub fn parse_iso(text: &str) -> Option<i64> {
    let t = text.trim();
    let (date_part, rest) = match t.split_once(['T', 't', ' ']) {
        Some((d, r)) => (d, Some(r)),
        None => (t, None),
    };
    let day = from_date(date_part)?;
    let Some(rest) = rest else { return Some(day) };
    // The offset.
    let (clock, offset_ms) = if let Some(c) = rest.strip_suffix(['Z', 'z']) {
        (c, 0)
    } else if let Some(at) = rest.rfind(['+', '-']) {
        let (c, o) = rest.split_at(at);
        let sign = if o.starts_with('-') { -1 } else { 1 };
        let o = &o[1..];
        let (h, m) = match o.split_once(':') {
            Some((h, m)) => (h, m),
            None if o.len() == 4 => o.split_at(2),
            None if o.len() == 2 => (o, "00"),
            _ => return None,
        };
        let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
        if h > 23 || m > 59 {
            return None;
        }
        (c, sign * (h * 3_600_000 + m * 60_000))
    } else {
        (rest, 0)
    };
    let mut parts = clock.split(':');
    let h: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next().unwrap_or("0").parse().ok()?;
    let sec_text = parts.next().unwrap_or("0");
    if parts.next().is_some() {
        return None;
    }
    let (s_text, frac) = match sec_text.split_once(['.', ',']) {
        Some((s, f)) => (s, f),
        None => (sec_text, ""),
    };
    let s: i64 = s_text.parse().ok()?;
    if h > 23 || m > 59 || s > 60 || !frac.bytes().all(|b| b.is_ascii_digit()) || frac.len() > 12 {
        return None;
    }
    let mut ms = 0;
    for (i, b) in frac.bytes().take(3).enumerate() {
        ms += i64::from(b - b'0') * [100, 10, 1][i];
    }
    Some(day + h * 3_600_000 + m * 60_000 + s * 1000 + ms - offset_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_round_trips_and_knows_leap_days() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_000), "1970-01-01T00:00:01.000Z");
        let t = parse_iso("2024-02-29T23:59:58.123Z").unwrap();
        assert_eq!(iso(t), "2024-02-29T23:59:58.123Z");
        assert_eq!(date(t), "2024-02-29");
        for ms in [
            0,
            86_399_999,
            951_782_400_000,
            4_102_444_800_000,
            253_402_300_799_000,
        ] {
            assert_eq!(parse_iso(&iso(ms)), Some(ms));
        }
    }

    #[test]
    fn offsets_and_forms() {
        let utc = parse_iso("2026-10-15T12:00:00Z").unwrap();
        assert_eq!(parse_iso("2026-10-15T14:00:00+02:00"), Some(utc));
        assert_eq!(parse_iso("2026-10-15T09:30:00-02:30"), Some(utc));
        assert_eq!(parse_iso("2026-10-15T14:00:00+0200"), Some(utc));
        assert_eq!(parse_iso("2026-10-15T14:00+02"), Some(utc));
        assert_eq!(parse_iso("2026-10-15T12:00:00"), Some(utc));
        assert_eq!(parse_iso("2026-10-15 12:00:00.5"), Some(utc + 500));
        assert_eq!(parse_iso("2026-10-15T12:00:00,25Z"), Some(utc + 250));
        assert_eq!(parse_iso("2026-10-15"), from_date("2026-10-15"));
        assert_eq!(parse_iso(" 2026-10-15T12:00:00Z "), Some(utc));
    }

    #[test]
    fn bad_texts_are_refused() {
        for bad in [
            "",
            "yesterday",
            "2026-13-01T00:00:00Z",
            "2026-10-15T25:00:00Z",
            "2026-10-15T12:61:00Z",
            "2026-10-15T12:00:00+25:00",
            "2026-10-15T12:00:00+xx",
            "2026-10-15T12:00:00.abcZ",
            "2026-10-15T12:00:00:00Z",
            "2026-10-15T12:00:00+1:2:3",
        ] {
            assert_eq!(parse_iso(bad), None, "{bad:?}");
        }
        assert_eq!(from_date("2026-02-30"), None);
    }

    #[test]
    fn civil_days_are_inverse() {
        for z in (-800_000..800_000).step_by(997) {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
    }
}
