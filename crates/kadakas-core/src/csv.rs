//! The one CSV writer of Kadakas. Every CSV export goes through it: plant lists, irrigation
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

/// Splits CSV text into rows of fields. Blank lines are skipped. The delimiter (comma, semicolon
/// or tab) is found from the first line. Quoted fields can hold delimiters, doubled quotes and
/// line breaks.
pub fn parse(text: &str) -> Result<Vec<Vec<String>>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let delim = detect_delimiter(text);
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut was_quoted = false;
    let mut chars = text.chars().peekable();
    let end_row = |row: &mut Vec<String>,
                   field: &mut String,
                   rows: &mut Vec<Vec<String>>,
                   was_quoted: &mut bool| {
        row.push(std::mem::take(field));
        let blank = row.len() == 1 && row[0].trim().is_empty() && !*was_quoted;
        if !blank {
            rows.push(std::mem::take(row));
        } else {
            row.clear();
        }
        *was_quoted = false;
    };
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' && field.trim().is_empty() {
            field.clear();
            quoted = true;
            was_quoted = true;
        } else if c == delim {
            row.push(std::mem::take(&mut field));
        } else if c == '\n' {
            end_row(&mut row, &mut field, &mut rows, &mut was_quoted);
        } else if c == '\r' {
            // Part of CRLF, or a lone CR as a line end.
            if chars.peek() == Some(&'\n') {
                continue;
            }
            end_row(&mut row, &mut field, &mut rows, &mut was_quoted);
        } else {
            field.push(c);
        }
    }
    if quoted {
        return Err("a quoted field is not closed".into());
    }
    if !field.is_empty() || !row.is_empty() || was_quoted {
        end_row(&mut row, &mut field, &mut rows, &mut was_quoted);
    }
    Ok(rows)
}

/// The delimiter that the first line uses most, outside quotes.
fn detect_delimiter(text: &str) -> char {
    let mut counts = [0usize; 3];
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            '\n' | '\r' if !quoted => break,
            ',' if !quoted => counts[0] += 1,
            ';' if !quoted => counts[1] += 1,
            '\t' if !quoted => counts[2] += 1,
            _ => {}
        }
    }
    let best = (0..3)
        .max_by_key(|i| (counts[*i], std::cmp::Reverse(*i)))
        .unwrap();
    if counts[best] == 0 {
        ','
    } else {
        [',', ';', '\t'][best]
    }
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

    #[test]
    fn parse_finds_the_delimiter_and_reads_quotes() {
        let rows = parse("a;b\n\"x;1\";\"say \"\"hi\"\"\"\n\n").unwrap();
        assert_eq!(rows, vec![vec!["a", "b"], vec!["x;1", "say \"hi\""]]);
        assert_eq!(parse("a\tb\r\n1\t2").unwrap().len(), 2);
        assert_eq!(parse("\u{feff}a,b").unwrap(), vec![vec!["a", "b"]]);
        assert!(parse("\"open").is_err());
        assert!(parse("").unwrap().is_empty());
    }
}
