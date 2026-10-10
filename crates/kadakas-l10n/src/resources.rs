//! The `.ftl` files, built into the program.
//!
//! A new file in `locales/<tag>/` must be added to the list here. The test
//! `every_ftl_file_is_listed` fails when a file is missing from the list.

macro_rules! files {
    ($lang:literal: $($f:literal),* $(,)?) => {
        &[$(($f, include_str!(concat!("../locales/", $lang, "/", $f)))),*]
    };
}

pub(crate) const FILES_EN_US: &[(&str, &str)] = files!(
    "en-US":
    "common.ftl",
    "uic-annot-tool.ftl",
    "uic-databar.ftl",
    "uic-effects.ftl",
    "uic-grading.ftl",
    "uic-irrigation-tool.ftl",
    "uic-keymap.ftl",
    "uic-modify-tool.ftl",
    "uic-plant-tool.ftl",
    "uic-snap.ftl",
    "uic-tools.ftl"
);

pub(crate) const FILES_ET: &[(&str, &str)] = files!(
    "et":
    "common.ftl",
    "uic-annot-tool.ftl",
    "uic-databar.ftl",
    "uic-effects.ftl",
    "uic-grading.ftl",
    "uic-irrigation-tool.ftl",
    "uic-keymap.ftl",
    "uic-modify-tool.ftl",
    "uic-plant-tool.ftl",
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
pub fn message_ids(tag: &str) -> Result<std::collections::BTreeSet<String>, String> {
    use fluent_syntax::ast::{Entry, Resource};
    let files = source_files(tag).ok_or_else(|| format!("unknown language {tag}"))?;
    let mut ids = std::collections::BTreeSet::new();
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

#[cfg(test)]
mod tests {
    use super::*;

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
