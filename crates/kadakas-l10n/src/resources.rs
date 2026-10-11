//! The `.ftl` files, built into the program.
//!
//! A new file in `locales/<tag>/` must be added to the list here. The test
//! `every_ftl_file_is_listed` fails when a file is missing from the list.

use std::collections::{BTreeMap, BTreeSet};

macro_rules! files {
    ($lang:literal: $($f:literal),* $(,)?) => {
        &[$(($f, include_str!(concat!("../locales/", $lang, "/", $f)))),*]
    };
}

pub(crate) const FILES_EN_US: &[(&str, &str)] = files!(
    "en-US":
    "about.ftl",
    "annot-ui.ftl",
    "app.ftl",
    "area-ui.ftl",
    "array-ui.ftl",
    "autosave-ui.ftl",
    "autosave.ftl",
    "canvas3d.ftl",
    "common.ftl",
    "data-ui.ftl",
    "enum-names.ftl",
    "file-io.ftl",
    "gis-dialogs.ftl",
    "gis-ui.ftl",
    "grading-ui.ftl",
    "icon-size.ftl",
    "io-dialog.ftl",
    "irrigation-ui.ftl",
    "keymap.ftl",
    "layout.ftl",
    "main.ftl",
    "panels.ftl",
    "plant-sheet-ui.ftl",
    "plant-ui.ftl",
    "prefs-ui.ftl",
    "prefs.ftl",
    "present-ui.ftl",
    "quick-search.ftl",
    "resource-ui.ftl",
    "rules-ui.ftl",
    "script-ui.ftl",
    "sheets-ui.ftl",
    "site-ui.ftl",
    "snap-ui.ftl",
    "theme.ftl",
    "uic-annot-tool.ftl",
    "uic-databar.ftl",
    "uic-effects.ftl",
    "uic-grading.ftl",
    "uic-irrigation-tool.ftl",
    "uic-keymap.ftl",
    "uic-modify-tool.ftl",
    "uic-plant-tool.ftl",
    "uic-profile.ftl",
    "uic-snap.ftl",
    "uic-tools.ftl"
);

pub(crate) const FILES_ET: &[(&str, &str)] = files!(
    "et":
    "about.ftl",
    "annot-ui.ftl",
    "app.ftl",
    "area-ui.ftl",
    "array-ui.ftl",
    "autosave-ui.ftl",
    "autosave.ftl",
    "canvas3d.ftl",
    "common.ftl",
    "data-ui.ftl",
    "enum-names.ftl",
    "file-io.ftl",
    "gis-dialogs.ftl",
    "gis-ui.ftl",
    "grading-ui.ftl",
    "icon-size.ftl",
    "io-dialog.ftl",
    "irrigation-ui.ftl",
    "keymap.ftl",
    "layout.ftl",
    "main.ftl",
    "panels.ftl",
    "plant-sheet-ui.ftl",
    "plant-ui.ftl",
    "prefs-ui.ftl",
    "prefs.ftl",
    "present-ui.ftl",
    "quick-search.ftl",
    "resource-ui.ftl",
    "rules-ui.ftl",
    "script-ui.ftl",
    "sheets-ui.ftl",
    "site-ui.ftl",
    "snap-ui.ftl",
    "theme.ftl",
    "uic-annot-tool.ftl",
    "uic-databar.ftl",
    "uic-effects.ftl",
    "uic-grading.ftl",
    "uic-irrigation-tool.ftl",
    "uic-keymap.ftl",
    "uic-modify-tool.ftl",
    "uic-plant-tool.ftl",
    "uic-profile.ftl",
    "uic-snap.ftl",
    "uic-tools.ftl"
);

/// The files of a language as (file name, content), or `None` for an unknown tag.
pub fn source_files(tag: &str) -> Option<&'static [(&'static str, &'static str)]> {
    crate::languages()
        .iter()
        .find(|l| l.tag == tag)
        .map(|l| l.files)
}

/// All message ids of a language, with attributes as `id.attribute`. An error names the file and
/// the line of a syntax error, so the CI check shows the problem.
pub fn message_ids(tag: &str) -> Result<BTreeSet<String>, String> {
    use fluent_syntax::ast::{Entry, Resource};
    let files = source_files(tag).ok_or_else(|| format!("unknown language {tag}"))?;
    let mut ids = BTreeSet::new();
    for (name, text) in files {
        let res: Resource<&str> = match fluent_syntax::parser::parse(*text) {
            Ok(r) => r,
            Err((_, errors)) => {
                let first = errors.first().map(|e| e.pos.start).unwrap_or(0);
                let line = text[..first.min(text.len())].matches('\n').count() + 1;
                return Err(format!("{tag}/{name}: syntax error near line {line}"));
            }
        };
        for entry in res.body {
            if let Entry::Message(m) = entry {
                for a in &m.attributes {
                    ids.insert(format!("{}.{}", m.id.name, a.id.name));
                }
                if m.value.is_some() {
                    ids.insert(m.id.name.to_string());
                }
            }
        }
    }
    Ok(ids)
}

/// The variables (`$name`) of each message in one `.ftl` text, with attributes as
/// `id.attribute`. A syntax error gives `Err(line)`.
pub(crate) fn variables_in(text: &str) -> Result<BTreeMap<String, BTreeSet<String>>, usize> {
    use fluent_syntax::ast::{Entry, Expression, InlineExpression, Pattern, PatternElement};

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

    let res = fluent_syntax::parser::parse(text).map_err(|(_, errors)| {
        let first = errors.first().map(|e| e.pos.start).unwrap_or(0);
        text[..first.min(text.len())].matches('\n').count() + 1
    })?;
    let mut all = BTreeMap::new();
    for entry in res.body {
        if let Entry::Message(m) = entry {
            if let Some(v) = &m.value {
                let mut set = BTreeSet::new();
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
    Ok(all)
}

/// The variables (`$name`) that each message uses, with attributes as `id.attribute`. A
/// translation must use the variables of the English message, so the check compares them.
pub fn message_variables(tag: &str) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let files = source_files(tag).ok_or_else(|| format!("unknown language {tag}"))?;
    let mut all = BTreeMap::new();
    for (name, text) in files {
        let vars = variables_in(text).map_err(|_| format!("{tag}/{name}: syntax error"))?;
        all.extend(vars);
    }
    Ok(all)
}

/// Problems of a language that a reader of the files would not see: a message id that two files
/// both define, and a message that does not format cleanly (a variable that is missing, a
/// plural selector that fails). The CI check shows them.
pub fn problems(tag: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Some(files) = source_files(tag) else {
        return vec![format!("unknown language {tag}")];
    };
    let mut seen: BTreeMap<String, &str> = BTreeMap::new();
    for (name, text) in files {
        let Ok(res) = fluent_syntax::parser::parse(*text) else {
            out.push(format!("{tag}/{name}: syntax error"));
            continue;
        };
        for entry in res.body {
            if let fluent_syntax::ast::Entry::Message(m) = entry
                && let Some(first) = seen.insert(m.id.name.to_string(), name)
            {
                out.push(format!(
                    "{tag}: message {} is in {first} and in {name}",
                    m.id.name
                ));
            }
        }
    }
    let Ok(vars) = message_variables(tag) else {
        return out;
    };
    let localizer = crate::Localizer::new(tag);
    for (id, names) in &vars {
        // Give every variable a number, then text: both must format without an error.
        for text in [false, true] {
            let args: Vec<(&str, crate::Arg)> = names
                .iter()
                .map(|n| {
                    (
                        n.as_str(),
                        if text {
                            crate::Arg::Text("x".into())
                        } else {
                            crate::Arg::Num(2.0)
                        },
                    )
                })
                .collect();
            for e in localizer.format_errors(id, &args) {
                out.push(format!("{tag}: {id}: {e}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_messages_have_no_problems() {
        for lang in crate::languages() {
            assert_eq!(problems(lang.tag), Vec::<String>::new());
        }
        assert_eq!(problems("xx"), vec!["unknown language xx"]);
    }

    #[test]
    fn variables_are_listed_per_message() {
        let v = message_variables("en-US").unwrap();
        assert_eq!(v["common-test-hello"], ["name".to_string()].into());
        assert!(v["common-ok"].is_empty());
        assert_eq!(v["common-delete-n"], ["count".to_string()].into());
        assert!(message_variables("xx").is_err());
    }

    #[test]
    fn every_ftl_file_is_listed() {
        for lang in crate::languages() {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("locales")
                .join(lang.tag);
            let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
                .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".ftl"))
                .collect();
            on_disk.sort();
            let mut listed: Vec<String> = lang.files.iter().map(|(n, _)| n.to_string()).collect();
            listed.sort();
            assert_eq!(on_disk, listed, "{}: update resources.rs", lang.tag);
        }
    }

    #[test]
    fn ids_come_from_messages_and_attributes() {
        let ids = message_ids("en-US").unwrap();
        assert!(ids.contains("common-ok"));
        assert!(ids.contains("common-test-hello"));
        assert!(ids.contains("common-test-hello.title"));
        assert!(message_ids("xx").unwrap_err().contains("unknown"));
    }

    #[test]
    fn every_file_parses_without_error() {
        for lang in crate::languages() {
            message_ids(lang.tag).unwrap();
            assert_eq!(source_files(lang.tag).unwrap().len(), lang.files.len());
        }
        assert!(source_files("xx").is_none());
    }
}
