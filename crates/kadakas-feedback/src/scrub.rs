//! Removes paths, tokens, secrets and e-mail addresses from text.
//!
//! Every text of a report goes through [`text`] before it is stored. The rules are simple and
//! work on any input. The cost is linear in the length, and [`text`] cuts the input first.
//!
//! - E-mail addresses become `<email>`.
//! - A value after a key such as `token`, `secret`, `password`, `api_key`, `authorization` or
//!   `cookie` becomes `<secret>`. So does the token after `Bearer`, and the password of an
//!   address (`https://user:pass@host`).
//! - Known token shapes and any long mixed word of letters and digits become `<secret>`.
//! - An absolute path (`/home/ann/x.kad`, `~/x`, `C:\Users\ann\x.kad`, `\\server\share\x`)
//!   becomes `<path>/x.kad`: the file name stays, the folders go.

use serde_json::Value;

/// Words of this length or longer that mix letters and digits are taken for secrets.
const LONG_WORD: usize = 24;

fn is_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')
}

/// True if a key name suggests that its value is a secret.
pub fn sensitive_key(key: &str) -> bool {
    let k: String = key
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase();
    if k.is_empty() {
        return false;
    }
    const PARTS: [&str; 9] = [
        "token",
        "secret",
        "passw",
        "pwd",
        "authoriz",
        "credential",
        "cookie",
        "bearer",
        "signature",
    ];
    PARTS.iter().any(|s| k.contains(s)) || k.ends_with("key") || k == "sid" || k == "auth"
}

/// Cuts a text to `max` characters. A cut text ends with `…`.
pub fn cut(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        None => s.to_string(),
        Some((i, _)) => {
            let mut out = s[..i].trim_end().to_string();
            out.push('…');
            out
        }
    }
}

/// Drops control characters except new lines and tabs.
fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}

/// Replaces the value after a sensitive key (`token=abc`, `"password": "abc"`).
fn keyed_secrets(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        out.push(c);
        i += 1;
        if !matches!(c, '=' | ':') {
            continue;
        }
        // The key is the word before the separator, without quotes.
        let mut e = out.len() - 1;
        while e > 0 && matches!(out[e - 1], '"' | '\'' | ' ') {
            e -= 1;
        }
        let mut b = e;
        while b > 0 && is_key_char(out[b - 1]) && e - b < 64 {
            b -= 1;
        }
        let key: String = out[b..e].iter().collect();
        if !sensitive_key(&key) {
            continue;
        }
        // The value: optional spaces and a quote, then a run up to a delimiter.
        let mut j = i;
        while j < chars.len() && chars[j] == ' ' {
            j += 1;
        }
        let quote = chars.get(j).copied().filter(|q| matches!(q, '"' | '\''));
        if quote.is_some() {
            j += 1;
        }
        let start = j;
        while j < chars.len() {
            let d = chars[j];
            let stop = match quote {
                Some(q) => d == q,
                None => d.is_whitespace() || matches!(d, ',' | ';' | '&' | '}' | ')' | '"' | '\''),
            };
            if stop {
                break;
            }
            j += 1;
        }
        let value: String = chars[start..j].iter().collect();
        // `Authorization: Bearer abc` has the word Bearer first: the Bearer pass does the rest.
        if value.is_empty()
            || value.eq_ignore_ascii_case("bearer")
            || value.eq_ignore_ascii_case("basic")
        {
            continue;
        }
        out.extend(chars[i..start].iter());
        out.extend("<secret>".chars());
        i = j;
    }
    out.into_iter().collect()
}

/// The token after `Bearer` or `Basic`.
fn bearer(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut words = s.split_inclusive(char::is_whitespace);
    while let Some(w) = words.next() {
        out.push_str(w);
        let name = w.trim_end();
        if (name.eq_ignore_ascii_case("bearer") || name.eq_ignore_ascii_case("basic"))
            && w.len() > name.len()
            && let Some(next) = words.next()
        {
            let value = next.trim_end();
            if value.is_empty() || value == "<secret>" {
                out.push_str(next);
            } else {
                out.push_str("<secret>");
                out.push_str(&next[value.len()..]);
            }
        }
    }
    out
}

/// E-mail addresses.
fn emails(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let local = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-');
    let domain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-');
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '@' {
            // The local part is already in `out`.
            let mut n = 0;
            while n < out.len() && local(out[out.len() - 1 - n]) {
                n += 1;
            }
            let mut e = i + 1;
            while e < chars.len() && domain(chars[e]) {
                e += 1;
            }
            // A trailing dot is the end of a sentence.
            while e > i + 1 && chars[e - 1] == '.' {
                e -= 1;
            }
            let dom: String = chars[i + 1..e].iter().collect();
            if n > 0 && dom.contains('.') && !dom.starts_with('.') {
                out.truncate(out.len() - n);
                out.extend("<email>".chars());
                i = e;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out.into_iter().collect()
}

/// `scheme://user:password@host`: the user information goes.
fn url_userinfo(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find("://") {
        let (head, tail) = rest.split_at(p + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '/' | '"' | '\'' | '<' | '>'))
            .unwrap_or(tail.len());
        match tail[..end].rfind('@') {
            Some(at) => {
                out.push_str("<secret>");
                rest = &tail[at..];
            }
            None => rest = tail,
        }
    }
    out.push_str(rest);
    out
}

fn is_uuid(w: &str) -> bool {
    let parts: Vec<&str> = w.split('-').collect();
    parts.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(&parts)
            .all(|(n, p)| p.len() == *n && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A word that looks like a secret: a known prefix, a JWT, or a long mix of letters and digits.
fn secret_word(w: &str) -> bool {
    let lw = w.to_ascii_lowercase();
    const PREFIXES: [&str; 8] = [
        "sk-",
        "ghp_",
        "gho_",
        "ghs_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "kadakas.token.",
    ];
    if PREFIXES.iter().any(|p| lw.starts_with(p)) && w.len() > 8 {
        return true;
    }
    if w.starts_with("AKIA") && w.len() >= 16 && w.chars().all(|c| c.is_ascii_alphanumeric()) {
        return true;
    }
    if w.starts_with("eyJ") && w.matches('.').count() == 2 {
        return true;
    }
    if w.len() < LONG_WORD || is_uuid(w) {
        return false;
    }
    w.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '=' | '/'))
        && w.chars().any(|c| c.is_ascii_digit())
        && w.chars().any(|c| c.is_ascii_alphabetic())
}

/// True if the word starts an absolute path.
fn path_start(w: &str) -> bool {
    let b = w.as_bytes();
    if w.starts_with("\\\\") {
        return w.len() > 2;
    }
    // A file address names a path too.
    if w.starts_with("file://") {
        return w.len() > 7;
    }
    (w.starts_with('/') && w.len() > 1 && b[1] != b'/')
        || w.starts_with("~/")
        || w.starts_with("~\\")
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'\\' | b'/'))
}

/// A word with a folder separator inside: the next part of a path that has spaces.
fn path_part(w: &str) -> bool {
    (w.contains('/') || w.contains('\\'))
        && !w.contains("://")
        && w.chars().any(char::is_alphanumeric)
}

/// The file name at the end of a path word, if it looks like one (`name.ext`).
fn file_name(w: &str) -> Option<String> {
    let last = w.rsplit(['/', '\\']).next().unwrap_or("");
    let last = last.trim_end_matches(['.', ',', ';', ')', ':', '!', '?']);
    let dot = last.rfind('.')?;
    let ext = &last[dot + 1..];
    let ok = dot > 0
        && (1..=8).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && last[..dot]
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ' '));
    ok.then(|| last.to_string())
}

fn is_word_break(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | '`' | '<' | '>' | '(' | '[' | '{' | '=' | ','
        )
}

/// Paths, token shapes and long mixed words, word by word.
fn words(s: &str) -> String {
    // (word, the run of break characters after it)
    let mut toks: Vec<(String, String)> = Vec::new();
    let mut word = String::new();
    let mut sep = String::new();
    for c in s.chars() {
        if is_word_break(c) {
            sep.push(c);
        } else {
            if !sep.is_empty() {
                toks.push((std::mem::take(&mut word), std::mem::take(&mut sep)));
            }
            word.push(c);
        }
    }
    toks.push((word, sep));

    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < toks.len() {
        let w = toks[i].0.as_str();
        if path_start(w) {
            // The path goes on while a single space joins words with folder separators.
            let mut name = file_name(w);
            while toks[i].1 == " " && i + 1 < toks.len() && path_part(&toks[i + 1].0) {
                i += 1;
                name = file_name(&toks[i].0);
            }
            out.push_str("<path>");
            if let Some(n) = name {
                out.push('/');
                out.push_str(&n);
            }
        } else {
            let core = w.trim_end_matches(['.', ',', ';', ':', '!', '?']);
            if secret_word(core) {
                out.push_str("<secret>");
                out.push_str(&w[core.len()..]);
            } else {
                out.push_str(w);
            }
        }
        out.push_str(&toks[i].1);
        i += 1;
    }
    out
}

/// Scrubs one text. It has no length limit: use [`text`] for untrusted input.
pub fn scrub(s: &str) -> String {
    let s = clean(s);
    let s = url_userinfo(&s);
    let s = keyed_secrets(&s);
    let s = bearer(&s);
    let s = emails(&s);
    words(&s)
}

/// File extensions that `harsh` takes for file names.
const FILE_EXTS: [&str; 20] = [
    "kad", "json", "dxf", "dwg", "ifc", "shp", "geojson", "png", "jpg", "jpeg", "svg", "pdf",
    "csv", "py", "wasm", "txt", "gltf", "glb", "obj", "xml",
];

/// True for a short lower case word like an API option or method name (`bogus`, `edit.fillet`).
fn api_word(w: &str) -> bool {
    w.len() <= 40
        && w.chars().next().is_some_and(|f| f.is_ascii_lowercase())
        && w.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.'))
}

/// A harder scrub for texts that Kadakas fills in from the session, such as the error of the
/// last failed call. It does what `scrub` does, and then:
///
/// - a file name loses its stem and keeps its extension (`<path>/garden.kad` becomes `<file>.kad`);
/// - a name in quotes (`'Smith house'`) becomes `<name>`, unless it looks like an API word.
///
/// Layer and object names in error messages are in quotes, so this removes most of them.
pub fn harsh(s: &str) -> String {
    let s = scrub(s);
    let chars: Vec<char> = s.chars().collect();
    let mut quoted = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '\'' | '"' | '`')
            && let Some(len) = chars[i + 1..].iter().take(81).position(|x| *x == c)
        {
            let inner: String = chars[i + 1..i + 1 + len].iter().collect();
            if len > 0 && !api_word(&inner) && !inner.starts_with('<') {
                quoted.push(c);
                quoted.push_str("<name>");
                quoted.push(c);
                i += len + 2;
                continue;
            }
        }
        quoted.push(c);
        i += 1;
    }
    // File names, word by word.
    let mut res = String::with_capacity(quoted.len());
    let mut word = String::new();
    let flush = |word: &mut String, res: &mut String| {
        let core = word.trim_end_matches(['.', ',', ';', ':', '!', '?', ')']);
        let tail = &word[core.len()..];
        let name = core.rsplit(['/', '\\']).next().unwrap_or("");
        match name.rsplit_once('.') {
            Some((stem, ext))
                if !stem.is_empty() && FILE_EXTS.contains(&ext.to_ascii_lowercase().as_str()) =>
            {
                res.push_str("<file>.");
                res.push_str(ext);
                res.push_str(tail);
            }
            _ => res.push_str(word),
        }
        word.clear();
    };
    for c in quoted.chars() {
        if c.is_whitespace() || matches!(c, '\'' | '"' | '`' | '(' | '[' | '{' | '=' | ',') {
            flush(&mut word, &mut res);
            res.push(c);
        } else {
            word.push(c);
        }
    }
    flush(&mut word, &mut res);
    res
}

/// Scrubs a text of any size: cuts to a few times `max` first, scrubs, then cuts to `max`.
pub fn text(s: &str, max: usize) -> String {
    cut(&scrub(&cut(s, max.saturating_mul(4).max(256))), max)
}

/// Scrubs every string in a JSON value. A string under a sensitive key becomes `<secret>`. The
/// value is cut: at most `depth` levels, `items` entries per array or object, and `chars` per
/// string.
pub fn json(v: &Value, depth: usize, items: usize, chars: usize) -> Value {
    match v {
        Value::String(s) => Value::String(text(s, chars)),
        Value::Array(a) if depth > 0 => Value::Array(
            a.iter()
                .take(items)
                .map(|x| json(x, depth - 1, items, chars))
                .collect(),
        ),
        Value::Object(o) if depth > 0 => Value::Object(
            o.iter()
                .take(items)
                .map(|(k, x)| {
                    let x = if sensitive_key(k) && !x.is_null() {
                        Value::String("<secret>".into())
                    } else {
                        json(x, depth - 1, items, chars)
                    };
                    (text(k, 64), x)
                })
                .collect(),
        ),
        Value::Array(_) | Value::Object(_) => Value::String("…".into()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn email_addresses_go() {
        assert_eq!(
            scrub("write to ann.lee+x@example.co.uk now"),
            "write to <email> now"
        );
        assert_eq!(scrub("mail: a@b.org."), "mail: <email>.");
        // Not an address: no dot in the domain, nothing before the at sign.
        assert_eq!(
            scrub("user@localhost and @home"),
            "user@localhost and @home"
        );
    }

    #[test]
    fn keyed_secrets_go() {
        assert_eq!(scrub("token=abc"), "token=<secret>");
        assert_eq!(scrub("a?token=abc123&b=2"), "a?token=<secret>&b=2");
        assert_eq!(
            scrub(r#"{"password": "hunter 2", "n": 1}"#),
            r#"{"password": "<secret>", "n": 1}"#
        );
        assert_eq!(scrub("api_key: s3cr3t"), "api_key: <secret>");
        assert_eq!(
            scrub("Authorization: Bearer abc.def"),
            "Authorization: Bearer <secret>"
        );
        assert_eq!(scrub("bearer xyz"), "bearer <secret>");
        assert_eq!(scrub("name=plant"), "name=plant");
        assert_eq!(scrub("token:"), "token:");
        assert_eq!(
            scrub("https://bob:pw@host.example/x"),
            "https://<secret>@host.example/x"
        );
    }

    #[test]
    fn token_shapes_go() {
        assert_eq!(scrub("key sk-abcdefghijkl1234."), "key <secret>.");
        assert_eq!(scrub("ghp_abcdefghijklmnop"), "<secret>");
        assert_eq!(scrub("eyJhbGciOi.eyJzdWIiOi.sig"), "<secret>");
        assert_eq!(scrub("AKIAIOSFODNN7EXAMPLE"), "<secret>");
        assert_eq!(scrub("0123456789abcdef0123456789abcdef"), "<secret>");
        // A UUID is an object id, and useful.
        let id = "0190a1b2-c3d4-7e5f-8a6b-0123456789ab";
        assert_eq!(scrub(id), id);
        assert_eq!(scrub("plants.place failed"), "plants.place failed");
    }

    #[test]
    fn paths_lose_their_folders() {
        assert_eq!(
            scrub("open /Users/ann/Plans/garden.kad failed"),
            "open <path>/garden.kad failed"
        );
        assert_eq!(scrub("open ~/garden.kad"), "open <path>/garden.kad");
        assert_eq!(scrub(r"C:\Users\ann\garden.kad"), "<path>/garden.kad");
        assert_eq!(scrub(r"\\srv\share\a.dxf"), "<path>/a.dxf");
        assert_eq!(
            scrub(r#"{"path":"/home/ann/x.dxf"}"#),
            r#"{"path":"<path>/x.dxf"}"#
        );
        // No file name: no trace of the folders.
        assert_eq!(scrub("in /home/ann/project"), "in <path>");
        // A folder name with a space.
        assert_eq!(scrub("/Users/ann lee/docs/a.kad ok"), "<path>/a.kad ok");
        // Not paths.
        assert_eq!(
            scrub("and/or 3/4 https://a.org/x"),
            "and/or 3/4 https://a.org/x"
        );
    }

    #[test]
    fn file_addresses_are_paths() {
        assert_eq!(
            scrub("open file:///Users/ann/g.kad now"),
            "open <path>/g.kad now"
        );
        assert!(!scrub("file:///home/secretname/x").contains("secretname"));
    }

    #[test]
    fn harsh_keeps_the_extension_and_drops_names() {
        assert_eq!(
            harsh("cannot open /Users/ann/Plans/garden.kad"),
            "cannot open <file>.kad"
        );
        assert_eq!(
            harsh("cannot read garden.dxf: bad"),
            "cannot read <file>.dxf: bad"
        );
        assert_eq!(
            harsh("layer 'Smith house' does not exist"),
            "layer '<name>' does not exist"
        );
        assert_eq!(harsh("unknown field `bogus`"), "unknown field `bogus`");
        assert_eq!(
            harsh("no method edit.fillet here"),
            "no method edit.fillet here"
        );
        assert_eq!(
            harsh(r#"class "Elm Road 4" is locked"#),
            r#"class "<name>" is locked"#
        );
        // An unmatched quote is left alone.
        assert_eq!(harsh("it's fine"), "it's fine");
        assert_eq!(harsh(""), "");
    }

    #[test]
    fn text_is_bounded_and_clean() {
        let long = "x ".repeat(10_000);
        let t = text(&long, 100);
        assert!(t.chars().count() <= 101, "{}", t.len());
        assert!(t.ends_with('…'));
        assert_eq!(scrub("a\u{0}b\u{7}c\nd"), "abc\nd");
        assert_eq!(cut("héllo", 2), "hé…");
        assert_eq!(cut("hi", 2), "hi");
    }

    #[test]
    fn json_is_scrubbed_and_cut() {
        let v = serde_json::json!({"token": "abc", "path": "/a/b/c.kad", "deep": {"a": {"b": {"c": 1}}}, "list": [1,2,3,4,5], "n": 3});
        let s = json(&v, 3, 5, 50);
        assert_eq!(s["token"], "<secret>");
        assert_eq!(s["path"], "<path>/c.kad");
        assert_eq!(s["deep"]["a"]["b"], "…");
        assert_eq!(s["list"].as_array().unwrap().len(), 5);
        assert_eq!(json(&v["list"], 3, 2, 5).as_array().unwrap().len(), 2);
        assert_eq!(s["n"], 3);
    }

    fn secret() -> impl Strategy<Value = String> {
        "[A-Za-z0-9]{12,40}".prop_filter("needs a letter and a digit", |s| {
            s.chars().any(|c| c.is_ascii_digit()) && s.chars().any(|c| c.is_ascii_alphabetic())
        })
    }

    fn noise() -> impl Strategy<Value = String> {
        "[a-z ,.!?\n]{0,30}"
    }

    proptest! {
        #[test]
        fn planted_secrets_do_not_survive(s in secret(), pre in noise(), post in noise(), form in 0usize..9) {
            let planted = match form {
                0 => format!("token={s}"),
                1 => format!("Authorization: Bearer {s}"),
                2 => format!("{{\"api_key\": \"{s}\"}}"),
                3 => format!("password: {s}"),
                4 => format!("https://user:{s}@host.example/x"),
                5 => format!("/home/{s}/proj/{s}/a.kad"),
                6 => format!("{s}@example.com"),
                7 => format!("C:\\Users\\{s}\\a.kad"),
                _ => format!("sk-{s}"),
            };
            let out = scrub(&format!("{pre} {planted} {post}"));
            prop_assert!(!out.contains(&s), "leaked {} in {}", s, out);
        }

        #[test]
        fn harsh_never_leaks_a_quoted_name(name in "[A-Z][a-z]{3,8} [A-Z][a-z]{3,8}") {
            let out = harsh(&format!("layer '{name}' and file /home/x/{name}.kad"));
            prop_assert!(!out.contains(&name), "{}", out);
        }

        #[test]
        fn scrub_never_panics_and_text_is_bounded(s in "\\PC{0,200}") {
            let _ = scrub(&s);
            prop_assert!(text(&s, 50).chars().count() <= 51);
        }
    }
}
