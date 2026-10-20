//! The parser of the Measurements box.

use glam::DVec3;
use kadakas_units::{Decimal, NumberFormat, Units, parse_angle, parse_length};

use super::{
    Bare, InputError, MAX_COPIES, MAX_PARTS, MAX_SIDES, MIN_SIDES, Phase, Tool, Value, accepts,
    value_separator,
};

/// Reads what a person typed for a tool at a phase. The value is one of the kinds the tool takes
/// (see [`accepts`]); anything else is an error.
pub fn parse(text: &str, units: &Units, tool: Tool, phase: Phase) -> Result<Value, InputError> {
    let acc = accepts(tool, phase);
    let bare = acc.bare.unwrap_or(Bare::Length);
    let mut v = parse_any(text, units, bare)?;
    // `2,5` for a tool that takes a length and no pair: a decimal comma that was typed by habit.
    if let Value::Pair(Some(_), Some(_)) = v
        && !acc.takes(super::Kind::Pair)
        && acc.takes(super::Kind::Length)
        && let Some(l) = comma_as_decimal(text, units)
    {
        // `1,000` could be a thousands group: do not guess.
        if looks_like_thousands(text) {
            return Err(InputError::AmbiguousComma {
                text: text.trim().to_string(),
            });
        }
        v = Value::Length(l);
    }
    // A rectangle takes one length as the first value of the pair.
    if let Value::Length(l) = v
        && acc.takes(super::Kind::Pair)
        && !acc.takes(super::Kind::Length)
    {
        v = Value::Pair(Some(l), None);
    }
    if !acc.takes(v.kind()) {
        return Err(InputError::NotAccepted {
            kind: v.kind(),
            accepted: acc.kinds,
        });
    }
    let negative = match v {
        Value::Length(l) | Value::Radius(l) => l < 0.0,
        Value::Pair(a, b) => a.is_some_and(|x| x < 0.0) || b.is_some_and(|x| x < 0.0),
        _ => false,
    };
    if negative && !acc.signed {
        return Err(InputError::Negative);
    }
    if matches!(v, Value::Radius(r) if r < 0.0) {
        return Err(InputError::Negative);
    }
    Ok(v)
}

/// Digits, one comma, exactly three digits (`1,000`, `-12,345`): a thousands group in some
/// settings and a decimal comma in others.
fn looks_like_thousands(text: &str) -> bool {
    let t = text.trim().trim_start_matches(['-', '+']);
    match t.split_once(',') {
        Some((a, b)) => all_digits(a) && b.len() == 3 && all_digits(b),
        None => false,
    }
}

/// `1,5` as one and a half, for a text with a point as decimal separator.
fn comma_as_decimal(text: &str, units: &Units) -> Option<f64> {
    let t = text.trim();
    if units.number.decimal != Decimal::Point || t.matches(',').count() != 1 {
        return None;
    }
    let comma = NumberFormat {
        decimal: Decimal::Comma,
        ..units.number.plain()
    };
    parse_length(t, units.length, &comma)
}

/// Reads a text without a tool: the grammar of the box. `bare` says what a number without a unit
/// or a mark means. The result is not checked against a tool.
pub fn parse_any(text: &str, units: &Units, bare: Bare) -> Result<Value, InputError> {
    let t = text.trim();
    if t.is_empty() {
        return Err(InputError::Empty);
    }
    let nf = units.number.plain();
    let not_a_value = || InputError::NotAValue {
        text: t.to_string(),
    };
    let v = match t.chars().next() {
        Some('[') => Value::PointAbs(point(t, ']', units)?),
        Some('<') => Value::PointRel(point(t, '>', units)?),
        _ => {
            if let Some(v) = count_or_divide(t)? {
                v
            } else if let Some(v) = suffixed(t, units, &nf)? {
                v
            } else if let Some(v) = slope(t, &nf) {
                v
            } else if t.contains('°') || lower_ends_with(t, "deg") {
                let a = t
                    .replace("deg", "°")
                    .replace("DEG", "°")
                    .replace("Deg", "°");
                Value::Angle(parse_angle(&a, &nf).ok_or_else(not_a_value)?)
            } else if has_separator(t, units) {
                pair(t, units, &nf)?
            } else if nf.parse(t).is_some() {
                // A bare number.
                match bare {
                    Bare::Length => Value::Length(length(t, units, &nf)?),
                    Bare::Angle => Value::Angle(nf.parse(t).ok_or_else(not_a_value)?),
                    Bare::PairFirst => Value::Pair(Some(length(t, units, &nf)?), None),
                    Bare::Sides => match bare_sides(t) {
                        Some(r) => r?,
                        None => Value::Length(length(t, units, &nf)?),
                    },
                }
            } else {
                Value::Length(length(t, units, &nf)?)
            }
        }
    };
    finite(v)
}

/// A bare whole number as the number of sides. `None` for a text that is not a whole number
/// (a length with a unit or a decimal goes on to the length reading).
fn bare_sides(t: &str) -> Option<Result<Value, InputError>> {
    if !all_digits(t) {
        return None;
    }
    let n = whole(t);
    Some(if (MIN_SIDES..=MAX_SIDES).contains(&n) {
        Ok(Value::Sides(n))
    } else {
        Err(InputError::SidesRange)
    })
}

pub(super) fn finite(v: Value) -> Result<Value, InputError> {
    let ok = match v {
        Value::Length(x) | Value::Angle(x) | Value::Radius(x) => x.is_finite(),
        Value::Pair(a, b) => a.is_none_or(f64::is_finite) && b.is_none_or(f64::is_finite),
        Value::PointAbs(p) | Value::PointRel(p) => p.is_finite(),
        _ => true,
    };
    if ok { Ok(v) } else { Err(InputError::TooLarge) }
}

fn lower_ends_with(t: &str, suffix: &str) -> bool {
    t.len() >= suffix.len()
        && t.is_char_boundary(t.len() - suffix.len())
        && t[t.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

fn length(t: &str, units: &Units, nf: &NumberFormat) -> Result<f64, InputError> {
    parse_length(t, units.length, nf).ok_or_else(|| InputError::NotAValue {
        text: t.to_string(),
    })
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// A whole number from digits, saturating at `u32::MAX`.
fn whole(s: &str) -> u32 {
    s.parse::<u64>()
        .map(|v| v.min(u64::from(u32::MAX)) as u32)
        .unwrap_or(u32::MAX)
}

fn is_times(c: char) -> bool {
    matches!(c, 'x' | 'X' | '*' | '×')
}

/// `5x`, `x5`, `5*`, `*5` and `/5`, `5/`.
fn count_or_divide(t: &str) -> Result<Option<Value>, InputError> {
    let first = t.chars().next();
    let last = t.chars().last();
    if let (Some(f), Some(l)) = (first, last) {
        if f == '/' || l == '/' {
            let digits = if f == '/' { &t[1..] } else { &t[..t.len() - 1] };
            let digits = digits.trim();
            if !all_digits(digits) {
                return Ok(None);
            }
            let n = whole(digits);
            return match n {
                0 | 1 => Err(InputError::DivideTooSmall),
                n if n > MAX_PARTS => Err(InputError::DivideTooMany),
                n => Ok(Some(Value::Divide(n))),
            };
        }
        if is_times(l) || is_times(f) {
            let digits = if is_times(l) {
                &t[..t.len() - l.len_utf8()]
            } else {
                &t[f.len_utf8()..]
            };
            let digits = digits.trim();
            if !all_digits(digits) {
                return Ok(None);
            }
            let n = whole(digits);
            return match n {
                0 => Err(InputError::CopiesZero),
                n if n > MAX_COPIES => Err(InputError::CopiesTooMany),
                n => Ok(Some(Value::Count(n))),
            };
        }
    }
    Ok(None)
}

/// `12s` (sides) and `24r` (radius).
fn suffixed(t: &str, units: &Units, nf: &NumberFormat) -> Result<Option<Value>, InputError> {
    let Some(last) = t.chars().last() else {
        return Ok(None);
    };
    let body = &t[..t.len() - last.len_utf8()];
    match last.to_ascii_lowercase() {
        's' if all_digits(body.trim()) => {
            let n = whole(body.trim());
            if (MIN_SIDES..=MAX_SIDES).contains(&n) {
                Ok(Some(Value::Sides(n)))
            } else {
                Err(InputError::SidesRange)
            }
        }
        // `24r`, `2.5mr`: the radius is a length. A text that is not one stays unread.
        'r' if !body.trim().is_empty() => {
            Ok(parse_length(body.trim(), units.length, nf).map(Value::Radius))
        }
        _ => Ok(None),
    }
}

/// `8:12`: rise to run, as an angle.
fn slope(t: &str, nf: &NumberFormat) -> Option<Value> {
    let (a, b) = t.split_once(':')?;
    let (rise, run) = (nf.parse(a)?, nf.parse(b)?);
    if run <= 0.0 {
        return None;
    }
    Some(Value::Angle(rise.atan2(run).to_degrees()))
}

fn separators(units: &Units) -> &'static [char] {
    match units.number.decimal {
        Decimal::Comma => &[';'],
        Decimal::Point => &[',', ';'],
    }
}

fn has_separator(t: &str, units: &Units) -> bool {
    t.contains(separators(units))
}

fn pair(t: &str, units: &Units, nf: &NumberFormat) -> Result<Value, InputError> {
    let parts: Vec<&str> = t.split(separators(units)).collect();
    if parts.len() > 2 {
        return Err(InputError::TooManyValues {
            separator: value_separator(units),
        });
    }
    let one = |s: &str| -> Result<Option<f64>, InputError> {
        let s = s.trim();
        if s.is_empty() {
            Ok(None)
        } else {
            length(s, units, nf).map(Some)
        }
    };
    let (a, b) = (one(parts[0])?, one(parts[1])?);
    if a.is_none() && b.is_none() {
        return Err(InputError::NotAValue {
            text: t.to_string(),
        });
    }
    Ok(Value::Pair(a, b))
}

fn point(t: &str, close: char, units: &Units) -> Result<DVec3, InputError> {
    if !t.ends_with(close) {
        return Err(InputError::BadBrackets);
    }
    let inner = &t[1..t.len() - close.len_utf8()];
    if inner.contains(['[', ']', '<', '>']) {
        return Err(InputError::BadBrackets);
    }
    let parts: Vec<&str> = inner.split(separators(units)).collect();
    if parts.len() != 3 {
        return Err(InputError::BadPoint {
            separator: value_separator(units),
        });
    }
    let nf = units.number.plain();
    let mut v = [0.0; 3];
    for (k, p) in parts.iter().enumerate() {
        v[k] = length(p.trim(), units, &nf)?;
    }
    Ok(DVec3::from(v))
}
