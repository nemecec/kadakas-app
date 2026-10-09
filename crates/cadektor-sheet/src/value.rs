//! Cell values and errors.

use std::cmp::Ordering;
use std::fmt;

/// Why a cell has no value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// Division by zero.
    Div0,
    /// A reference that does not work here.
    Ref,
    /// An unknown function or name.
    Name,
    /// A value of the wrong type.
    Value,
    /// The cell depends on itself.
    Cycle,
    /// The formula cannot be read.
    Parse,
    /// A number that is not finite.
    Num,
}

impl ErrorKind {
    /// The text that a cell shows.
    pub fn code(self) -> &'static str {
        match self {
            ErrorKind::Div0 => "#DIV/0!",
            ErrorKind::Ref => "#REF!",
            ErrorKind::Name => "#NAME?",
            ErrorKind::Value => "#VALUE!",
            ErrorKind::Cycle => "#CYCLE!",
            ErrorKind::Parse => "#PARSE!",
            ErrorKind::Num => "#NUM!",
        }
    }
}

/// The value of a cell or of an expression.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Value {
    #[default]
    Empty,
    Number(f64),
    Text(String),
    Bool(bool),
    Error(ErrorKind),
}

/// A number as text, without float noise: up to 10 decimals, no trailing zeros.
pub fn format_number(n: f64) -> String {
    if !n.is_finite() {
        return ErrorKind::Num.code().to_string();
    }
    if n == n.trunc() && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    if n.abs() >= 1e15 || n.abs() < 1e-10 {
        return format!("{n}");
    }
    let s = format!("{n:.10}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_string() } else { s.to_string() }
}

impl Value {
    pub fn is_error(&self) -> bool {
        matches!(self, Value::Error(_))
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Value::Empty)
    }

    /// A number value. Not finite numbers become `#NUM!`.
    pub fn num(n: f64) -> Value {
        // Negative zero is zero: it must compare and group like it.
        if n == 0.0 {
            Value::Number(0.0)
        } else if n.is_finite() {
            Value::Number(n)
        } else {
            Value::Error(ErrorKind::Num)
        }
    }

    /// The value as a number, for arithmetic: empty is 0, a boolean is 0 or 1, text is read as a
    /// number if it looks like one.
    pub fn to_number(&self) -> Result<f64, ErrorKind> {
        match self {
            Value::Empty => Ok(0.0),
            Value::Number(n) => Ok(*n),
            Value::Bool(b) => Ok(f64::from(u8::from(*b))),
            Value::Text(s) => s.trim().parse::<f64>().ok().filter(|n| n.is_finite()).ok_or(ErrorKind::Value),
            Value::Error(e) => Err(*e),
        }
    }

    /// The value as a condition: numbers other than 0, and `TRUE`.
    pub fn to_bool(&self) -> Result<bool, ErrorKind> {
        match self {
            Value::Empty => Ok(false),
            Value::Bool(b) => Ok(*b),
            Value::Number(n) => Ok(*n != 0.0),
            Value::Text(s) if s.eq_ignore_ascii_case("true") => Ok(true),
            Value::Text(s) if s.eq_ignore_ascii_case("false") => Ok(false),
            Value::Text(_) => Err(ErrorKind::Value),
            Value::Error(e) => Err(*e),
        }
    }

    /// The text of the value, as `&` and CONCAT use it.
    pub fn to_text(&self) -> String {
        match self {
            Value::Empty => String::new(),
            Value::Number(n) => format_number(*n),
            Value::Text(s) => s.clone(),
            Value::Bool(true) => "TRUE".into(),
            Value::Bool(false) => "FALSE".into(),
            Value::Error(e) => e.code().into(),
        }
    }

    fn rank(&self) -> u8 {
        match self {
            Value::Empty => 0,
            Value::Number(_) => 1,
            Value::Text(_) => 2,
            Value::Bool(_) => 3,
            Value::Error(_) => 4,
        }
    }

    /// A total order for sorting and comparing: empty, numbers, text (without case), booleans,
    /// errors.
    pub fn compare(&self, other: &Value) -> Ordering {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Value::Text(a), Value::Text(b)) => a.to_lowercase().cmp(&b.to_lowercase()),
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (Value::Error(a), Value::Error(b)) => a.code().cmp(b.code()),
            // An empty cell equals 0, "" and FALSE when it meets that type.
            (Value::Empty, Value::Number(b)) => 0.0f64.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Value::Number(a), Value::Empty) => a.partial_cmp(&0.0).unwrap_or(Ordering::Equal),
            (Value::Empty, Value::Text(b)) => "".cmp(b.as_str()),
            (Value::Text(a), Value::Empty) => a.as_str().cmp(""),
            (Value::Empty, Value::Bool(b)) => false.cmp(b),
            (Value::Bool(a), Value::Empty) => a.cmp(&false),
            (a, b) => a.rank().cmp(&b.rank()),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_print_without_noise() {
        assert_eq!(format_number(3.0), "3");
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(0.1 + 0.2), "0.3");
        assert_eq!(format_number(-2.5), "-2.5");
        assert_eq!(format_number(1e20), "100000000000000000000");
        assert_eq!(format_number(1.5e-12), "0.0000000000015");
        assert_eq!(format_number(f64::NAN), "#NUM!");
        assert_eq!(format_number(-1e-11 + 1e-11), "0");
        assert_eq!(format_number(-0.00000000001), "-0.00000000001");
        assert_eq!(format_number(-0.000000000001 * 0.0001), "-0.0000000000000001");
        assert_eq!(format_number(0.00000000004), "0.00000000004");
    }

    #[test]
    fn conversions() {
        assert_eq!(Value::Empty.to_number(), Ok(0.0));
        assert_eq!(Value::Bool(true).to_number(), Ok(1.0));
        assert_eq!(Value::Text(" 2.5 ".into()).to_number(), Ok(2.5));
        assert_eq!(Value::Text("abc".into()).to_number(), Err(ErrorKind::Value));
        assert_eq!(Value::Text("inf".into()).to_number(), Err(ErrorKind::Value));
        assert_eq!(Value::Error(ErrorKind::Ref).to_number(), Err(ErrorKind::Ref));
        assert_eq!(Value::Empty.to_bool(), Ok(false));
        assert_eq!(Value::Number(2.0).to_bool(), Ok(true));
        assert_eq!(Value::Text("True".into()).to_bool(), Ok(true));
        assert_eq!(Value::Text("false".into()).to_bool(), Ok(false));
        assert_eq!(Value::Text("x".into()).to_bool(), Err(ErrorKind::Value));
        assert_eq!(Value::Bool(false).to_bool(), Ok(false));
        assert_eq!(Value::Error(ErrorKind::Div0).to_bool(), Err(ErrorKind::Div0));
        assert_eq!(Value::num(f64::INFINITY), Value::Error(ErrorKind::Num));
        assert_eq!(Value::num(1.0), Value::Number(1.0));
        assert!(Value::Error(ErrorKind::Name).is_error() && Value::Empty.is_empty());
    }

    #[test]
    fn text_and_display() {
        assert_eq!(Value::Empty.to_text(), "");
        assert_eq!(Value::Bool(true).to_text(), "TRUE");
        assert_eq!(Value::Bool(false).to_text(), "FALSE");
        assert_eq!(Value::Number(1.5).to_string(), "1.5");
        assert_eq!(Value::Error(ErrorKind::Cycle).to_text(), "#CYCLE!");
        for e in [ErrorKind::Div0, ErrorKind::Ref, ErrorKind::Name, ErrorKind::Value, ErrorKind::Cycle, ErrorKind::Parse, ErrorKind::Num] {
            assert!(e.code().starts_with('#'));
        }
    }

    #[test]
    fn ordering() {
        use Ordering::*;
        let t = |s: &str| Value::Text(s.into());
        assert_eq!(Value::Number(1.0).compare(&Value::Number(2.0)), Less);
        assert_eq!(t("a").compare(&t("B")), Less);
        assert_eq!(t("a").compare(&t("A")), Equal);
        assert_eq!(Value::Bool(false).compare(&Value::Bool(true)), Less);
        assert_eq!(Value::Error(ErrorKind::Div0).compare(&Value::Error(ErrorKind::Div0)), Equal);
        assert_eq!(Value::Empty.compare(&Value::Number(-1.0)), Greater);
        assert_eq!(Value::Number(0.0).compare(&Value::Empty), Equal);
        assert_eq!(Value::Empty.compare(&t("x")), Less);
        assert_eq!(t("x").compare(&Value::Empty), Greater);
        assert_eq!(Value::Empty.compare(&Value::Bool(true)), Less);
        assert_eq!(Value::Bool(true).compare(&Value::Empty), Greater);
        assert_eq!(Value::Number(5.0).compare(&t("a")), Less);
        assert_eq!(Value::Bool(false).compare(&t("a")), Greater);
        assert_eq!(Value::Error(ErrorKind::Num).compare(&Value::Bool(true)), Greater);
        assert_eq!(Value::Empty.compare(&Value::Empty), Equal);
        // Negative zero equals zero.
        assert_eq!(Value::Number(-0.0).compare(&Value::Number(0.0)), Equal);
        assert_eq!(Value::Number(-0.0).compare(&Value::Empty), Equal);
        assert_eq!(Value::Empty.compare(&Value::Number(-0.0)), Equal);
        assert_eq!(format!("{:?}", Value::num(-0.0)), "Number(0.0)");
    }
}
