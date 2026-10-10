//! The one CSV writer of Cadektor. Every CSV export goes through it: plant lists, irrigation
//! reports, hardscape reports, worksheets and schedules.
//!
//! It quotes like RFC 4180 and guards against formula injection: a spreadsheet program runs a
//! text cell that starts with `=`, `+`, `-` or `@`. Such a cell, and a cell that starts with a tab
//! or a carriage return, gets a leading `'`. A cell that is a plain number (for example `-4.5`)
//! stays as it is.

/// True if the text is only a number, such as `12`, `-4.5` or `1e-3`. Such a cell is no formula.
fn is_number(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
        && s.parse::<f64>().is_ok_and(f64::is_finite)
}

/// Adds the guard `'` to a text cell that a spreadsheet program could run as a formula.
pub fn defuse(s: &str) -> String {
    if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_string()
    }
}

/// Quotes a cell when it has a delimiter, a quote, a line break or edge spaces.
pub fn quote(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r', ';', '\t']) || s.starts_with(' ') || s.ends_with(' ') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// A text cell: guarded, then quoted.
pub fn text_cell(s: &str) -> String {
    quote(&defuse(s))
}

/// A cell of unknown type: a plain number stays as it is, anything else is a text cell.
pub fn cell(s: &str) -> String {
    if is_number(s) {
        s.to_string()
    } else {
        text_cell(s)
    }
}

/// Writes rows as CSV text with comma delimiters and `\n` line ends.
pub fn write_rows(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for r in rows {
        out.push_str(&r.iter().map(|f| cell(f)).collect::<Vec<_>>().join(","));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_starts_get_a_guard() {
        for s in [
            "=1+1",
            "+cmd",
            "-cmd",
            "@SUM(A1)",
            "\tx",
            "\rx",
            "=HYPERLINK(\"u\")",
        ] {
            assert!(defuse(s).starts_with('\''), "{s:?}");
        }
        assert_eq!(defuse("Acer"), "Acer");
        assert_eq!(defuse(""), "");
        assert_eq!(defuse("a=b"), "a=b");
    }

    #[test]
    fn numbers_stay_numbers() {
        for s in ["12", "-4.5", "+3", "1e-3", "0.00"] {
            assert_eq!(cell(s), s);
        }
        // Text that only looks like a number start is still guarded.
        assert_eq!(cell("-1+2x"), "'-1+2x");
        assert_eq!(cell("-"), "'-");
        assert_eq!(cell("-inf"), "'-inf");
        assert_eq!(cell("-NaN"), "'-NaN");
    }

    #[test]
    fn quoting_follows_rfc_4180() {
        assert_eq!(quote("a,b"), "\"a,b\"");
        assert_eq!(quote("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(quote("two\nlines"), "\"two\nlines\"");
        assert_eq!(quote(" lead"), "\" lead\"");
        assert_eq!(quote("plain"), "plain");
    }

    #[test]
    fn guard_and_quote_combine() {
        assert_eq!(text_cell("=a,b"), "\"'=a,b\"");
        assert_eq!(text_cell("\tTab"), "\"'\tTab\"");
    }

    #[test]
    fn rows_end_with_newlines() {
        let rows = vec![
            vec!["Name".to_string(), "Price".into()],
            vec!["=evil()".into(), "-4.50".into()],
        ];
        assert_eq!(write_rows(&rows), "Name,Price\n'=evil(),-4.50\n");
        assert_eq!(write_rows(&[]), "");
    }
}
