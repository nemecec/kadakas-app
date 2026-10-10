//! The supported languages and the choice of a language from what the system asks for.

use crate::resources::FILES_EN_US;
use crate::resources::FILES_ET;

/// A language that the program has text for.
#[derive(Clone, Copy, Debug)]
pub struct Lang {
    /// The BCP 47 tag, for example `en-US`.
    pub tag: &'static str,
    /// The name of the language in the language itself, for the language list.
    pub native_name: &'static str,
    /// The `.ftl` files: file name and content.
    pub files: &'static [(&'static str, &'static str)],
}

/// The language that fills every gap, and the one that code is written against.
pub const FALLBACK: &str = "en-US";

const LANGUAGES: &[Lang] = &[
    Lang {
        tag: FALLBACK,
        native_name: "English",
        files: FILES_EN_US,
    },
    Lang {
        tag: "et",
        native_name: "Eesti",
        files: FILES_ET,
    },
];

/// All supported languages. The first one is the fallback.
pub fn languages() -> &'static [Lang] {
    LANGUAGES
}

/// Writes a locale name from the system as a BCP 47 tag with `-` and without encoding or
/// modifier: `et_EE.UTF-8` gives `et-EE`, `de_DE@euro` gives `de-DE`. `C` and `POSIX` give an
/// empty string.
pub fn normalize_tag(raw: &str) -> String {
    let cut = raw
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .trim()
        .replace('_', "-");
    if cut.eq_ignore_ascii_case("c") || cut.eq_ignore_ascii_case("posix") {
        String::new()
    } else {
        cut
    }
}

fn language_part(tag: &str) -> &str {
    tag.split('-').next().unwrap_or_default()
}

/// The supported language that fits the requests best. The requests come in order of
/// preference. For each one, an exact match wins (ignoring case), then a language with the same
/// base language (`et-EE` fits `et`, `en-GB` fits `en-US`). Nothing fits: en-US.
pub fn negotiate<S: AsRef<str>>(requested: &[S]) -> &'static str {
    for r in requested {
        let tag = normalize_tag(r.as_ref());
        if tag.is_empty() {
            continue;
        }
        if let Some(l) = LANGUAGES.iter().find(|l| l.tag.eq_ignore_ascii_case(&tag)) {
            return l.tag;
        }
        let base = language_part(&tag);
        if let Some(l) = LANGUAGES
            .iter()
            .find(|l| language_part(l.tag).eq_ignore_ascii_case(base))
        {
            return l.tag;
        }
    }
    FALLBACK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_from_the_system_are_cleaned() {
        assert_eq!(normalize_tag("et_EE.UTF-8"), "et-EE");
        assert_eq!(normalize_tag("de_DE@euro"), "de-DE");
        assert_eq!(normalize_tag(" en-US "), "en-US");
        assert_eq!(normalize_tag("C"), "");
        assert_eq!(normalize_tag("POSIX"), "");
        assert_eq!(normalize_tag(""), "");
    }

    #[test]
    fn negotiation_prefers_exact_then_base_language() {
        assert_eq!(negotiate(&["et"]), "et");
        assert_eq!(negotiate(&["ET-ee"]), "et");
        assert_eq!(negotiate(&["et_EE.UTF-8"]), "et");
        assert_eq!(negotiate(&["en-GB"]), "en-US");
        assert_eq!(negotiate(&["en-US"]), "en-US");
        // The first request that fits wins.
        assert_eq!(negotiate(&["fi", "et", "en"]), "et");
        assert_eq!(negotiate(&["fi", "en"]), "en-US");
        // Nothing fits, or nothing is asked.
        assert_eq!(negotiate(&["xx", "C", ""]), "en-US");
        assert_eq!(negotiate::<&str>(&[]), "en-US");
    }

    #[test]
    fn the_fallback_is_first() {
        assert_eq!(languages()[0].tag, FALLBACK);
        assert!(languages().iter().all(|l| !l.files.is_empty()));
        assert!(languages().iter().all(|l| !l.native_name.is_empty()));
    }
}
