//! The CI check for UI text (PLAT-05). It fails when:
//!
//! 1. a language lacks a message that another language has, or has an extra one;
//! 2. a message uses other variables in one language than in another;
//! 3. a message id that the code uses is missing from en-US (the code uses `tr!("id")`,
//!    `msg!("id")` and `translate("id", ...)`);
//! 4. an en-US message is not used by any code (a leftover);
//! 5. a hard-coded user-visible string is left in an egui call (`.label("..")`, `.button("..")`
//!    and the like) in the UI crates.
//!
//! The rules and the way to add a language are in `docs/localization.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use fluent_syntax::ast::{Entry, Expression, InlineExpression, Pattern, PatternElement};

/// The crates with user interface text. A crate with egui calls must be here.
const UI_CRATES: &[&str] = &["kadakas-ui-core"];

/// Crates that use messages (all the crates with `tr!`, `msg!` or `translate` calls).
const CODE_CRATES: &[&str] = &["kadakas-app", "kadakas-ui-core"];

/// Message ids that no code uses on purpose: examples and tests of the l10n crate itself.
const UNUSED_OK: &[&str] = &[
    "common-test-hello",
    "common-test-hello.title",
    "common-ok",
    "common-cancel",
    "common-delete-n",
];

/// String literals that may stay in an egui call, as (source file, literal). These are not
/// words that a translator could change: symbols, units and names of file formats.
const LITERAL_OK: &[(&str, &str)] = &[("icons.rs", "X")];

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

/// The source of a file without its test code: the text before the first `#[cfg(test)]`.
/// Files named `tests.rs` or in a `tests` folder are test code as a whole.
fn production_source(path: &Path) -> Option<String> {
    let name = path.file_name().unwrap().to_string_lossy();
    if name == "tests.rs" || path.components().any(|c| c.as_os_str() == "tests") {
        return None;
    }
    let text = std::fs::read_to_string(path).unwrap();
    Some(match text.find("#[cfg(test)]") {
        Some(i) => text[..i].to_string(),
        None => text,
    })
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

/// The variables (`$name`) that a message uses, per message id (attributes as `id.attr`).
fn variables(tag: &str) -> BTreeMap<String, BTreeSet<String>> {
    fn in_pattern(p: &Pattern<&str>, out: &mut BTreeSet<String>) {
        for el in &p.elements {
            if let PatternElement::Placeable { expression } = el {
                in_expression(expression, out);
            }
        }
    }
    fn in_inline(e: &InlineExpression<&str>, out: &mut BTreeSet<String>) {
        match e {
            InlineExpression::VariableReference { id } => {
                out.insert(id.name.to_string());
            }
            InlineExpression::Placeable { expression } => in_expression(expression, out),
            InlineExpression::FunctionReference { arguments, .. } => {
                for a in &arguments.positional {
                    in_inline(a, out);
                }
                for n in &arguments.named {
                    in_inline(&n.value, out);
                }
            }
            _ => {}
        }
    }
    fn in_expression(e: &Expression<&str>, out: &mut BTreeSet<String>) {
        match e {
            Expression::Inline(i) => in_inline(i, out),
            Expression::Select { selector, variants } => {
                in_inline(selector, out);
                for v in variants {
                    in_pattern(&v.value, out);
                }
            }
        }
    }
    let mut all = BTreeMap::new();
    for (_, text) in kadakas_l10n::source_files(tag).unwrap() {
        let res = fluent_syntax::parser::parse(*text).expect("syntax");
        for entry in res.body {
            if let Entry::Message(m) = entry {
                let mut set = BTreeSet::new();
                if let Some(v) = &m.value {
                    in_pattern(v, &mut set);
                    all.insert(m.id.name.to_string(), set);
                }
                for a in &m.attributes {
                    let mut set = BTreeSet::new();
                    in_pattern(&a.value, &mut set);
                    all.insert(format!("{}.{}", m.id.name, a.id.name), set);
                }
            }
        }
    }
    all
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
fn a_message_uses_the_same_variables_in_every_language() {
    let base = variables(kadakas_l10n::FALLBACK);
    let mut problems = Vec::new();
    for lang in kadakas_l10n::languages() {
        for (id, vars) in variables(lang.tag) {
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
];

/// Calls with the text as a later argument: the check looks for a literal after a comma.
const LATER_ARG: &[&str] = &[
    ".checkbox(",
    ".selectable_label(",
    ".selectable_value(",
    ".radio_value(",
    ".radio(",
    ".colored_label(",
    ".toggle_value(",
];

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
                    let mut rest = &code[at..];
                    // A call that breaks the line after the parenthesis.
                    let next;
                    if rest.trim().is_empty()
                        && let Some(l) = lines.get(n + 1)
                    {
                        next = l.to_string();
                        rest = &next;
                    }
                    if let Some(lit) = leading_literal(rest) {
                        found.push(lit.to_string());
                    }
                }
            }
            for needle in LATER_ARG {
                let mut from = 0;
                while let Some(i) = code[from..].find(needle) {
                    let at = from + i + needle.len();
                    from = at;
                    for part in code[at..].split(',').skip(1) {
                        if let Some(lit) = leading_literal(part) {
                            found.push(lit.to_string());
                            break;
                        }
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
}
