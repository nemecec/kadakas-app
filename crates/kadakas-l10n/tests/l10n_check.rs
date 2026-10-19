//! The CI check for UI text (PLAT-05). It fails when:
//!
//! 1. a language lacks a message that another language has, or has an extra one;
//! 2. a message uses other variables in one language than in another;
//! 3. a message id that the code uses is missing from en-US (the code uses `tr!("id")`,
//!    `msg!("id")` and `translate("id", ...)`);
//! 4. an en-US message is not used by any code (a leftover);
//! 5. a hard-coded user-visible string is left in an egui call (`.label("..")`, `.button("..")`,
//!    `painter.text(.., "..", ..)` and the like) in the UI crates;
//! 6. an Estonian message is the same text as the English one (a copy that nobody translated),
//!    unless it is in `SAME_OK`: a name, a symbol or a word that both languages spell alike.
//!
//! The probe `text_probe` in `kadakas-app` (tests there) covers the rest: it draws parts of the UI
//! in Estonian and fails when egui paints an English message text that did not come from `tr!`.
//!
//! The rules and the way to add a language are in `docs/localization.md`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The crates with user interface text. A crate with egui calls must be here.
const UI_CRATES: &[&str] = &["kadakas-app", "kadakas-ui-core", "kadakas-launcher"];

/// Crates that use messages (all the crates with `tr!`, `msg!` or `translate` calls).
const CODE_CRATES: &[&str] = &["kadakas-app", "kadakas-ui-core", "kadakas-launcher"];

/// Message ids that no code uses on purpose: examples and tests of the l10n crate itself.
const UNUSED_OK: &[&str] = &[
    "common-test-hello",
    "common-test-hello.title",
    "common-ok",
    "common-cancel",
    "common-delete-n",
];

/// Messages whose Estonian text is the English text on purpose: names, symbols, loan words that
/// Estonian spells alike, and units. A new message must not be here only because it is not
/// translated yet.
const SAME_OK: &[&str] = &[
    "about-kadakas",
    "launcher-heading",
    "agent-view-name",
    "app-kadakas",
    "app-import",
    "enum-form-palm",
    "feedback-ui-consent-server",
    "gis-dialogs-server",
    "gis-dialogs-filter",
    "io-dialog-dxf-dwg-ifc-shapefile-geojson",
    "io-dialog-import",
    "io-dialog-ifc4-ifc4-add2-tc1",
    "issues-ui-bcf",
    "issues-ui-bcf-import-heading",
    "issues-ui-bcf-error",
    "panels-import-menu",
    "panels-bezier",
    "plant-sheet-ui-filter",
    "plugin-ui-hash",
    "profiles-ui-filter",
    "profiles-ui-alias-runs",
    "script-ui-python",
    "sheets-ui-number",
    "uic-tools-lasso",
    "uic-tools-bezier",
    "versions-ui-size-kib",
    "versions-ui-size-mib",
    // Only variables, units and symbols: nothing to translate.
    "gis-ui-x-px",
    "panels-cursor-position",
    "picture-ui-facts",
    "array-ui-ok",
    "common-ok",
    "solid-ui-ok",
    "data-ui-a-b-c",
    "enum-length-unit-mm",
    "enum-length-unit-cm",
    "enum-length-unit-m",
    "enum-length-unit-km",
];

/// String literals that may stay in an egui call, as (source file, literal). These are not
/// words that a translator could change: symbols, units and names of file formats.
const LITERAL_OK: &[(&str, &str)] = &[
    // A multiplication sign and a close mark on small buttons.
    ("panels.rs", "\\u{d7}"),
    // The profile windows: a key with a remove mark, the arrows that reorder tools, and the
    // name of the profile file format.
    ("profiles.rs", "{} \\u{d7}"),
    ("profiles.rs", "\\u{25b2}"),
    ("profiles.rs", "\\u{25bc}"),
    ("profiles.rs", "JSON"),
    ("data_ui.rs", "x"),
    ("gis_ui.rs", "x"),
    ("plant_ui.rs", "x"),
    ("array_ui.rs", "x "),
    // The operators of the criteria language and its example.
    ("data_ui.rs", "AND"),
    ("data_ui.rs", "OR"),
    ("data_ui.rs", "Kind = \\"),
    // A MIME type and an EPSG code are examples that need no translation.
    ("gis_dialogs.rs", "image/png"),
    ("gis_dialogs.rs", "EPSG:32635"),
    // Row numbers and sort arrows are only symbols and numbers.
    ("panels.rs", "{n}"),
    ("panels.rs", "{i}"),
    ("plant_sheet_ui.rs", "{}{arrow}"),
    ("site_ui.rs", "{vertices}"),
    // Names of file formats in file dialogs. They are the same in every language.
    ("app.rs", "Kadakas"),
    ("app.rs", "SVG"),
    ("app.rs", "PDF"),
    ("app.rs", "JSON"),
    ("io_dialog.rs", "DXF"),
    ("update_ui.rs", "DXF"),
    ("drawings_ui.rs", "DXF"),
    ("io_dialog.rs", "GeoJSON"),
    ("io_dialog.rs", "IFC"),
    ("io_dialog.rs", "IFC4"),
    ("io_dialog.rs", "Shapefile"),
    ("plant_ui.rs", "CSV"),
    // The title of the window is the name of the program.
    ("main.rs", "Kadakas"),
    // An empty text, a bare placeholder.
    ("app.rs", "{}"),
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// The text of a file without the items that are marked `#[cfg(test)]` (a test module, a helper
/// function for tests). A marked item ends at its closing brace, or at its semicolon.
fn strip_cfg_test(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..i]);
        let after = &rest[i + "#[cfg(test)]".len()..];
        let (brace, semi) = (after.find('{'), after.find(';'));
        let end = match (brace, semi) {
            (Some(b), Some(s)) if s < b => s + 1,
            (None, Some(s)) => s + 1,
            (Some(b), _) => b + matching_brace(&after[b..]),
            (None, None) => after.len(),
        };
        rest = &after[end.min(after.len())..];
    }
    out.push_str(rest);
    out
}

/// The length of a block that starts with `{`, up to and with its closing brace. Strings, characters
/// and comments are skipped.
fn matching_brace(text: &str) -> usize {
    let b = text.as_bytes();
    let (mut depth, mut i) = (0i32, 0usize);
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'\'' => {
                // A character like 'a' or '\n' or '{' (not a lifetime).
                if b.get(i + 2) == Some(&b'\'') {
                    i += 2;
                } else if b.get(i + 1) == Some(&b'\\') {
                    while i < b.len() && b[i] != b'\'' || i < 2 {
                        i += 1;
                        if b.get(i) == Some(&b'\'') && b[i - 1] != b'\\' {
                            break;
                        }
                    }
                }
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    b.len()
}

/// The source of a file without its test code. Files named `tests.rs` or in a `tests` folder
/// are test code as a whole.
fn production_source(path: &Path) -> Option<String> {
    let name = path.file_name().unwrap().to_string_lossy();
    if name == "tests.rs" || path.components().any(|c| c.as_os_str() == "tests") {
        return None;
    }
    Some(strip_cfg_test(&std::fs::read_to_string(path).unwrap()))
}

fn sources(crates: &[&str]) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for c in crates {
        let mut files = Vec::new();
        rust_files(&workspace().join("crates").join(c).join("src"), &mut files);
        for f in files {
            if let Some(s) = production_source(&f) {
                out.push((f, s));
            }
        }
    }
    out
}

#[test]
fn every_language_has_the_same_messages() {
    let base: BTreeSet<String> = kadakas_l10n::message_ids(kadakas_l10n::FALLBACK).unwrap();
    let mut problems = Vec::new();
    for lang in kadakas_l10n::languages() {
        let ids = kadakas_l10n::message_ids(lang.tag).unwrap_or_else(|e| panic!("{e}"));
        for id in base.difference(&ids) {
            problems.push(format!("{}: missing message {id}", lang.tag));
        }
        for id in ids.difference(&base) {
            problems.push(format!("{}: message {id} is not in en-US", lang.tag));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[test]
fn no_estonian_message_is_a_copy_of_the_english_one() {
    let ids = kadakas_l10n::message_ids(kadakas_l10n::FALLBACK).unwrap();
    let mut problems = Vec::new();
    // One switch of the language for all messages: a switch is slow.
    let all = |tag: &str| -> Vec<String> {
        kadakas_l10n::with_language(tag, || {
            ids.iter()
                .map(|id| kadakas_l10n::translate(id, &[]))
                .collect()
        })
    };
    let (english, estonian) = (all("en-US"), all("et"));
    for ((id, en), et) in ids.iter().zip(english).zip(estonian) {
        // Only the words count, not the `{$variable}` parts.
        let mut words = String::new();
        let mut depth = 0;
        for c in en.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                c if depth == 0 => words.push(c),
                _ => {}
            }
        }
        if en == et
            && words.chars().filter(|c| c.is_alphabetic()).count() >= 2
            && !SAME_OK.contains(&id.as_str())
        {
            problems.push(format!(
                "{id}: the Estonian text is the English text {en:?}"
            ));
        }
    }
    for id in SAME_OK {
        assert!(ids.contains(*id), "{id} in SAME_OK is no message");
    }
    assert!(
        problems.is_empty(),
        "translate these, or add them to SAME_OK with a reason:\n{}",
        problems.join("\n")
    );
}

#[test]
fn every_message_formats_without_an_error() {
    let mut problems = Vec::new();
    for lang in kadakas_l10n::languages() {
        problems.extend(kadakas_l10n::problems(lang.tag));
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[test]
fn a_message_uses_the_same_variables_in_every_language() {
    let base = kadakas_l10n::message_variables(kadakas_l10n::FALLBACK).unwrap();
    let mut problems = Vec::new();
    for lang in kadakas_l10n::languages() {
        for (id, vars) in kadakas_l10n::message_variables(lang.tag).unwrap() {
            if let Some(b) = base.get(&id)
                && *b != vars
            {
                problems.push(format!(
                    "{}: {id} uses {vars:?} but en-US uses {b:?}",
                    lang.tag
                ));
            }
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

/// The string literal that follows a marker like `tr!(`: its text, if the next token is a literal.
fn ids_after(src: &str, marker: &str, out: &mut Vec<(String, usize)>) {
    let mut from = 0;
    while let Some(i) = src[from..].find(marker) {
        let at = from + i;
        from = at + marker.len();
        // The marker must start a word: `tr!(` must not be part of `str!(`.
        let before = src[..at].chars().next_back();
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let rest = src[from..].trim_start();
        if let Some(r) = rest.strip_prefix('"')
            && let Some(end) = r.find('"')
        {
            let line = src[..at].matches('\n').count() + 1;
            out.push((r[..end].to_string(), line));
        }
    }
}

fn used_ids() -> Vec<(String, String, usize)> {
    let mut out = Vec::new();
    for (path, src) in sources(CODE_CRATES) {
        let mut ids = Vec::new();
        for marker in ["tr!(", "msg!(", "translate("] {
            ids_after(&src, marker, &mut ids);
        }
        let name = path
            .strip_prefix(workspace())
            .unwrap()
            .display()
            .to_string();
        out.extend(ids.into_iter().map(|(id, line)| (id, name.clone(), line)));
    }
    out
}

#[test]
fn every_id_used_in_the_code_exists_in_en_us() {
    let base = kadakas_l10n::message_ids(kadakas_l10n::FALLBACK).unwrap();
    let missing: Vec<String> = used_ids()
        .into_iter()
        .filter(|(id, _, _)| !base.contains(id))
        .map(|(id, file, line)| format!("{file}:{line}: message {id} is not in en-US"))
        .collect();
    assert!(missing.is_empty(), "\n{}", missing.join("\n"));
}

#[test]
fn every_en_us_message_is_used() {
    let used: BTreeSet<String> = used_ids().into_iter().map(|(id, _, _)| id).collect();
    let base = kadakas_l10n::message_ids(kadakas_l10n::FALLBACK).unwrap();
    // `id.attribute` counts as used when the code uses `id.attribute` or the message `id`.
    let unused: Vec<&String> = base
        .iter()
        .filter(|id| !used.contains(*id))
        .filter(|id| !used.contains(id.split('.').next().unwrap()))
        .filter(|id| !UNUSED_OK.contains(&id.as_str()))
        .collect();
    assert!(
        unused.is_empty(),
        "these messages are not used by any code; delete them or use them:\n{}",
        unused
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Calls whose first argument is user-visible text.
const FIRST_ARG: &[&str] = &[
    ".label(",
    ".button(",
    ".small_button(",
    ".heading(",
    ".strong(",
    ".weak(",
    ".small(",
    ".monospace(",
    ".on_hover_text(",
    ".on_disabled_hover_text(",
    ".menu_button(",
    ".collapsing(",
    ".hint_text(",
    ".selected_text(",
    ".prefix(",
    ".suffix(",
    ".hyperlink_to(",
    "RichText::new(",
    "Button::new(",
    "Label::new(",
    "Window::new(",
    "CollapsingHeader::new(",
    "ComboBox::from_label(",
    "Context::set_title(",
    "text_edit_singleline_hint(",
    // The names of file types and the title of a file dialog (rfd).
    ".add_filter(",
    ".set_title(",
    // Text that goes to the status line or to a message field.
    "status = ",
    "message = ",
    "error = Some(",
    // Other ways to put text on the screen.
    ".layout_no_wrap(",
    ".link(",
    ".hyperlink(",
    ".with_title(",
    ".on_hover_text_at_pointer(",
    "SelectableLabel::new(",
];

/// Calls with the text as a later argument, with the place of that argument (the first
/// argument is 0): `ui.checkbox(&mut on, "Text")`, `ui.selectable_value(&mut v, value, "Text")`.
const LATER_ARG: &[(&str, usize)] = &[
    (".checkbox(", 1),
    (".selectable_label(", 1),
    (".selectable_value(", 2),
    (".radio_value(", 2),
    (".radio(", 1),
    (".colored_label(", 1),
    (".toggle_value(", 1),
    ("Checkbox::new(", 1),
    ("RadioButton::new(", 1),
    ("ComboBox::new(", 1),
    // `painter.text(position, anchor, "text", font, color)` draws text on the canvas.
    ("painter.text(", 2),
    (".text(", 2),
];

/// Splits the text of a call at the commas that are not inside brackets or quotes. The text
/// ends at the bracket that closes the call.
fn split_args(call: &str) -> Vec<&str> {
    let (mut depth, mut start, mut quoted) = (0i32, 0usize, false);
    let mut out = Vec::new();
    let bytes = call.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if quoted => i += 1,
            b'"' => quoted = !quoted,
            b'(' | b'[' | b'{' if !quoted => depth += 1,
            b')' | b']' | b'}' if !quoted => {
                depth -= 1;
                if depth < 0 {
                    out.push(&call[start..i]);
                    return out;
                }
            }
            b',' if !quoted && depth == 0 => {
                out.push(&call[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&call[start..]);
    out
}

fn has_letters(s: &str) -> bool {
    s.chars().any(|c| c.is_alphabetic())
}

/// The literal at the start of `rest` (after optional `format!(` or `&`), if there is one.
fn leading_literal(rest: &str) -> Option<&str> {
    let r = rest.trim_start();
    let r = r.strip_prefix("format!(").map_or(r, str::trim_start);
    let r = r.strip_prefix('&').unwrap_or(r);
    let r = r.strip_prefix('"')?;
    let end = r.find('"')?;
    Some(&r[..end])
}

#[test]
fn no_user_visible_literal_is_left_in_an_egui_call() {
    let mut problems = Vec::new();
    for (path, src) in sources(UI_CRATES) {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let lines: Vec<&str> = src.lines().collect();
        for (n, line) in lines.iter().enumerate() {
            let code = line.split("//").next().unwrap_or(line);
            let mut found: Vec<String> = Vec::new();
            for needle in FIRST_ARG {
                let mut from = 0;
                while let Some(i) = code[from..].find(needle) {
                    let at = from + i + needle.len();
                    from = at;
                    // A call that breaks the line after the parenthesis, or after `format!(`.
                    let mut joined = code[at..].to_string();
                    for k in 1..=2 {
                        let t = joined.trim_end();
                        if (t.is_empty() || t.ends_with("format!("))
                            && let Some(l) = lines.get(n + k)
                        {
                            joined = format!("{t} {}", l.trim());
                        }
                    }
                    if let Some(lit) = leading_literal(&joined) {
                        found.push(lit.to_string());
                    }
                }
            }
            for (needle, place) in LATER_ARG {
                let mut from = 0;
                while let Some(i) = code[from..].find(needle) {
                    let at = from + i + needle.len();
                    from = at;
                    if let Some(arg) = split_args(&code[at..]).get(*place)
                        && let Some(lit) = leading_literal(arg)
                    {
                        found.push(lit.to_string());
                    }
                }
            }
            for lit in found {
                if has_letters(&lit) && !LITERAL_OK.contains(&(file.as_str(), lit.as_str())) {
                    problems.push(format!(
                        "{}:{}: hard-coded text {lit:?}; use tr!(..) and a message in locales/en-US",
                        path.strip_prefix(workspace()).unwrap().display(),
                        n + 1
                    ));
                }
            }
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

/// A call of `tr!` in the source: the id, the names of its arguments with their expressions, and
/// the line.
struct TrCall {
    id: String,
    args: Vec<(String, String)>,
    line: usize,
}

fn tr_calls(src: &str) -> Vec<TrCall> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = src[from..].find("tr!(") {
        let at = from + i;
        from = at + 4;
        if src[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            continue;
        }
        let parts = split_args(&src[from..]);
        let Some(first) = parts.first().map(|p| p.trim()) else {
            continue;
        };
        let Some(id) = first.strip_prefix('"').and_then(|r| r.strip_suffix('"')) else {
            continue;
        };
        let mut args = Vec::new();
        for p in &parts[1..] {
            let p = p.trim();
            if let Some((name, expr)) = p.split_once('=')
                && !expr.starts_with('=')
                && !name.trim().is_empty()
                && name.trim().chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                args.push((name.trim().to_string(), expr.trim().to_string()));
            }
        }
        out.push(TrCall {
            id: id.to_string(),
            args,
            line: src[..at].matches('\n').count() + 1,
        });
    }
    out
}

#[test]
fn the_arguments_of_each_tr_call_are_the_variables_of_its_message() {
    let vars = kadakas_l10n::message_variables(kadakas_l10n::FALLBACK).unwrap();
    let mut problems = Vec::new();
    for (path, src) in sources(CODE_CRATES) {
        let file = path
            .strip_prefix(workspace())
            .unwrap()
            .display()
            .to_string();
        for call in tr_calls(&src) {
            let Some(want) = vars.get(&call.id) else {
                continue; // the test for unknown ids reports it
            };
            let got: BTreeSet<String> = call.args.iter().map(|(n, _)| n.clone()).collect();
            if &got != want {
                problems.push(format!(
                    "{file}:{}: {} passes {:?} but the message uses {:?}",
                    call.line, call.id, got, want
                ));
            }
            // JSON values print with quotes and are no numbers for the plural rules. Convert
            // them with `.as_u64()`, `.as_str()` and so on first.
            for (name, expr) in &call.args {
                let json_index =
                    expr.contains("[\"") && !expr.contains(".as_") && !expr.contains(".to_");
                if json_index {
                    problems.push(format!(
                        "{file}:{}: {} argument {name} is a JSON value ({expr}); convert it first",
                        call.line, call.id
                    ));
                }
            }
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[test]
fn the_scan_of_tr_calls_reads_ids_and_argument_names() {
    let src = "x = tr!(\"a-b\", n = v[\"k\"], other = f(1, 2));\nstr!(\"no\");\ntr!(\"c\")";
    let calls = tr_calls(src);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id, "a-b");
    assert_eq!(calls[0].args[0], ("n".to_string(), "v[\"k\"]".to_string()));
    assert_eq!(calls[0].args[1].0, "other");
    assert_eq!(calls[1].line, 3);
    assert!(calls[1].args.is_empty());
}

#[test]
fn test_code_is_cut_out_of_a_file() {
    let text = "a\n#[cfg(test)]\nfn helper() { let s = \"}\"; let c = '{'; }\nb\n#[cfg(test)]\nuse x;\nc\n#[cfg(test)]\nmod tests { fn f() { { } } }\nd";
    assert_eq!(
        strip_cfg_test(text).split_whitespace().collect::<Vec<_>>(),
        ["a", "b", "c", "d"]
    );
}

#[test]
fn the_check_finds_what_it_should() {
    // The scanner itself: a literal in a label call is found, a call of tr! is not.
    assert_eq!(leading_literal("\"Hello\")"), Some("Hello"));
    assert_eq!(leading_literal("format!(\"Hi {}\", x)"), Some("Hi {}"));
    assert_eq!(leading_literal("tr!(\"x\")"), None);
    assert_eq!(leading_literal("&\"Hi\""), Some("Hi"));
    let mut ids = Vec::new();
    ids_after(
        "a tr!(\"one\") str!(\"two\") msg!(\n  \"three\")",
        "tr!(",
        &mut ids,
    );
    ids_after(
        "a tr!(\"one\") str!(\"two\") msg!(\n  \"three\")",
        "msg!(",
        &mut ids,
    );
    let names: Vec<&str> = ids.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(names, vec!["one", "three"]);
    assert!(has_letters("x") && !has_letters("12 %") && !has_letters("\u{b0}"));
    assert_eq!(
        split_args("&mut v, f(1, 2), \"a,b\") x"),
        vec!["&mut v", " f(1, 2)", " \"a,b\""]
    );
}
