//! The codes inside DXF text (IOP-15).
//!
//! A TEXT string can hold `%%c` (diameter), `%%d` (degree), `%%p` (plus-minus), `%%nnn` (the
//! character with that code in the ANSI page), the toggles `%%u`, `%%o` and `%%k`, and the escape
//! `\U+XXXX`. A caret and a letter stand for a control character (`^J` is a line feed). MTEXT adds
//! backslash codes for fonts, heights, colors, stacked text and paragraphs. The importer keeps the
//! text and drops the formatting.

/// The character with this code in the Windows page 1252 (codes 1 to 255).
fn ansi_char(code: u32) -> Option<char> {
    if (1..0x80).contains(&code) {
        return char::from_u32(code);
    }
    let byte = u8::try_from(code).ok().filter(|b| *b >= 0x80)?;
    let bytes = [byte];
    let (s, _, bad) = encoding_rs::WINDOWS_1252.decode(&bytes);
    if bad { None } else { s.chars().next() }
}

/// Replaces `%%c`, `%%d`, `%%p`, `%%%`, `%%nnn` and `\U+XXXX`. The toggles `%%u`, `%%o` and `%%k`
/// (underline, overline, strike-through) are dropped. A code that is not known stays as it is.
pub fn decode_percent(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' && chars.get(i + 1) == Some(&'%') {
            let next = chars.get(i + 2).copied();
            match next.map(|c| c.to_ascii_lowercase()) {
                Some('d') => out.push('\u{b0}'),
                Some('p') => out.push('\u{b1}'),
                Some('c') => out.push('\u{d8}'),
                Some('%') => out.push('%'),
                Some('u' | 'o' | 'k') => {}
                Some(d) if d.is_ascii_digit() => {
                    let digits: String = chars[i + 2..]
                        .iter()
                        .take(3)
                        .take_while(|c| c.is_ascii_digit())
                        .collect();
                    let code = digits.parse::<u32>().ok().and_then(ansi_char);
                    match (digits.len() == 3, code) {
                        (true, Some(c)) => {
                            out.push(c);
                            i += 5;
                            continue;
                        }
                        _ => {
                            out.push_str("%%");
                            out.push(d);
                        }
                    }
                }
                Some(other) => {
                    out.push_str("%%");
                    out.push(other);
                }
                None => out.push_str("%%"),
            }
            i += 3.min(chars.len() - i);
        } else if chars[i] == '\\'
            && matches!(chars.get(i + 1), Some('U') | Some('u'))
            && chars.get(i + 2) == Some(&'+')
        {
            let hex: String = chars[i + 3..].iter().take(4).collect();
            match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                Some(c) if hex.len() == 4 => {
                    out.push(c);
                    i += 7;
                }
                _ => {
                    out.push('\\');
                    i += 1;
                }
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Replaces the caret codes: `^J` is a line feed, `^I` a tab, `^M` a carriage return (dropped),
/// and a caret and a space is a caret. Only a caret before `@`, a capital letter, `[`, `\`, `]`,
/// `^` or `_` is a code, so a text like `x^2` stays as it is.
pub fn decode_caret(s: &str) -> String {
    if !s.contains('^') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '^' {
            out.push(c);
            continue;
        }
        match it.peek().copied() {
            Some(' ') => {
                it.next();
                out.push('^');
            }
            Some(n @ ('@'..='_')) => {
                it.next();
                match (n as u8) ^ 0x40 {
                    10 => out.push('\n'),
                    9 => out.push(' '),
                    // The other control characters have no place in a drawing text.
                    _ => {}
                }
            }
            _ => out.push('^'),
        }
    }
    out
}

/// The text of a TEXT, ATTRIB or MTEXT string without the codes (caret, percent and `\U+`).
pub fn decode_text(s: &str) -> String {
    decode_percent(&decode_caret(s))
}

/// Plain text of MTEXT: line breaks kept, formatting codes removed.
pub fn clean_mtext(s: &str) -> String {
    let s = decode_text(s);
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '{' | '}' => {}
            '\\' => match it.next() {
                Some('P') | Some('X') | Some('N') => out.push('\n'),
                Some('~') => out.push(' '),
                Some(c @ ('\\' | '{' | '}')) => out.push(c),
                Some('L' | 'l' | 'O' | 'o' | 'K' | 'k') => {}
                Some('S') => {
                    // Stacked text: keep the numerator and denominator.
                    let mut body = String::new();
                    for c in it.by_ref() {
                        if c == ';' {
                            break;
                        }
                        body.push(c);
                    }
                    out.push_str(&body.replace(['^', '#'], "/"));
                }
                Some(_) => {
                    // A code with arguments, such as \fArial|b0; or \H2.5x; or \pxqc;
                    for c in it.by_ref() {
                        if c == ';' {
                            break;
                        }
                    }
                }
                None => {}
            },
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_codes() {
        assert_eq!(decode_percent("%%c10 %%p2 %%d"), "\u{d8}10 \u{b1}2 \u{b0}");
        assert_eq!(decode_percent("%%C %%D %%P"), "\u{d8} \u{b0} \u{b1}");
        assert_eq!(decode_percent("100%%%"), "100%");
        assert_eq!(decode_percent("%%x %%"), "%%x %%");
        assert_eq!(decode_percent("%"), "%");
        assert_eq!(decode_percent("\\U+12"), "\\U+12");
        assert_eq!(decode_percent("\\U+zzzz"), "\\U+zzzz");
        assert_eq!(decode_percent("\\U+00E4\\u+00f6"), "\u{e4}\u{f6}");
    }

    #[test]
    fn decimal_and_toggle_codes() {
        assert_eq!(decode_percent("90%%176"), "90\u{b0}");
        assert_eq!(decode_percent("%%228%%246"), "\u{e4}\u{f6}");
        assert_eq!(decode_percent("%%128"), "\u{20ac}");
        assert_eq!(decode_percent("%%065"), "A");
        // A code with fewer than three digits, or none that exists, stays.
        assert_eq!(decode_percent("%%17"), "%%17");
        assert_eq!(decode_percent("%%000"), "%%000");
        assert_eq!(decode_percent("%%999"), "%%999");
        // The toggles are removed.
        assert_eq!(
            decode_percent("%%uunder%%u and %%oover%%o %%kx"),
            "under and over x"
        );
    }

    #[test]
    fn caret_codes() {
        assert_eq!(decode_caret("a^Jb"), "a\nb");
        assert_eq!(decode_caret("a^Ib"), "a b");
        assert_eq!(decode_caret("a^Mb"), "ab");
        assert_eq!(decode_caret("a^ b"), "a^b");
        assert_eq!(decode_caret("x^2"), "x^2");
        assert_eq!(decode_caret("end^"), "end^");
        assert_eq!(decode_caret("^@^[^_"), "");
        assert_eq!(decode_text("1^J%%c5"), "1\n\u{d8}5");
    }

    #[test]
    fn mtext_codes() {
        assert_eq!(clean_mtext("a\\"), "a");
        assert_eq!(clean_mtext("\\PA\\Xb"), "\nA\nb");
        assert_eq!(
            clean_mtext("{\\fArial|b0;Hello}\\Psecond \\S1/2; line\\~x\\L\\\\"),
            "Hello\nsecond 1/2 line x\\"
        );
        // Paragraph, height, width, tracking, color and alignment codes carry arguments.
        assert_eq!(
            clean_mtext("\\pxqc,i1;A\\H2.5x;B\\W0.8;C\\T1.2;D\\C3;E\\c255;F\\Q15;G\\A1;H"),
            "ABCDEFGH"
        );
        assert_eq!(clean_mtext("\\S+0.1^-0.1;"), "+0.1/-0.1");
        assert_eq!(clean_mtext("\\S1#4;"), "1/4");
        assert_eq!(clean_mtext("a\\Nb"), "a\nb");
        assert_eq!(
            clean_mtext("%%c5\\U+00B0 and \\{x\\}"),
            "\u{d8}5\u{b0} and {x}"
        );
        assert_eq!(clean_mtext("\\Ko\\kn\\Ou\\o"), "onu");
    }
}
