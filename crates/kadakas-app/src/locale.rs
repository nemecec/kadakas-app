//! The language and the units of the person (PLAT-05): where the default comes from, how a
//! change of the preferences reaches the rest of the program.
//!
//! The default language comes from the system: the locale list of the operating system on the
//! desktop (`sys-locale`), `navigator.languages` in the browser. The person can pick another
//! language in File > Preferences. A change is live: every text is made again each frame from the
//! messages of the active language, so nothing needs a restart.

use kadakas_units::{Settings, Units};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The language setting: the language of the system, or a fixed language (one of
/// `kadakas_l10n::languages()`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LanguageChoice(Option<&'static str>);

impl LanguageChoice {
    /// Follow the system.
    pub const SYSTEM: LanguageChoice = LanguageChoice(None);

    /// A fixed language. A tag that is not supported gives `SYSTEM`.
    pub fn fixed(tag: &str) -> LanguageChoice {
        kadakas_l10n::languages()
            .iter()
            .find(|l| l.tag.eq_ignore_ascii_case(tag))
            .map_or(LanguageChoice::SYSTEM, |l| LanguageChoice(Some(l.tag)))
    }

    /// The tag of a fixed language, or `None` for the system.
    pub fn tag(self) -> Option<&'static str> {
        self.0
    }
}

impl Serialize for LanguageChoice {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.0.unwrap_or("system"))
    }
}

impl<'de> Deserialize<'de> for LanguageChoice {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // An unknown language (from a newer version) falls back to the system language.
        let v = serde_json::Value::deserialize(d)?;
        Ok(v.as_str()
            .map_or(LanguageChoice::SYSTEM, LanguageChoice::fixed))
    }
}

/// The languages that the system asks for, the first one is the most wanted.
pub fn system_languages() -> Vec<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        sys_locale::get_locales().collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let Some(window) = web_sys::window() else {
            return Vec::new();
        };
        let navigator = window.navigator();
        let mut out: Vec<String> = navigator
            .languages()
            .iter()
            .filter_map(|v| v.as_string())
            .collect();
        if out.is_empty() {
            out.extend(navigator.language());
        }
        out
    }
}

/// The language tag that a choice gives, when the system asks for `system`.
pub fn resolve_language(choice: LanguageChoice, system: &[String]) -> &'static str {
    match choice.tag() {
        Some(tag) => tag,
        None => kadakas_l10n::negotiate(system),
    }
}

/// The language and the units that preferences give, without changing anything.
pub fn resolve(
    language: LanguageChoice,
    units: &Settings,
    system: &[String],
) -> (&'static str, Units) {
    let tag = resolve_language(language, system);
    (tag, units.resolve(tag))
}

/// Makes the language and the units of the preferences current for the whole program.
pub fn apply(language: LanguageChoice, units: &Settings) -> &'static str {
    let (tag, resolved) = resolve(language, units, &system_languages());
    kadakas_l10n::set_language(tag);
    kadakas_units::set_current(resolved);
    tag
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_units::Decimal;

    fn sys(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_fixed_language_wins_over_the_system() {
        let et = LanguageChoice::fixed("et");
        assert_eq!(et.tag(), Some("et"));
        assert_eq!(resolve_language(et, &sys(&["en-US"])), "et");
        assert_eq!(LanguageChoice::fixed("xx"), LanguageChoice::SYSTEM);
        assert_eq!(LanguageChoice::fixed("ET").tag(), Some("et"));
    }

    #[test]
    fn the_system_language_is_negotiated() {
        let c = LanguageChoice::SYSTEM;
        assert_eq!(resolve_language(c, &sys(&["et_EE.UTF-8", "en"])), "et");
        assert_eq!(resolve_language(c, &sys(&["fi", "en-GB"])), "en-US");
        assert_eq!(resolve_language(c, &sys(&["xx"])), "en-US");
        assert_eq!(resolve_language(c, &[]), "en-US");
    }

    #[test]
    fn the_units_follow_the_language_until_the_person_chooses() {
        let mut s = Settings::default();
        let (tag, u) = resolve(LanguageChoice::fixed("et"), &s, &[]);
        assert_eq!((tag, u.number.decimal), ("et", Decimal::Comma));
        s.decimal = Some(Decimal::Point);
        let (_, u) = resolve(LanguageChoice::fixed("et"), &s, &[]);
        assert_eq!(u.number.decimal, Decimal::Point);
        let (tag, u) = resolve(LanguageChoice::SYSTEM, &Settings::default(), &sys(&["en"]));
        assert_eq!((tag, u.number.decimal), ("en-US", Decimal::Point));
    }

    #[test]
    fn the_choice_is_saved_as_text() {
        let json = serde_json::to_string(&LanguageChoice::fixed("et")).unwrap();
        assert_eq!(json, "\"et\"");
        assert_eq!(
            serde_json::to_string(&LanguageChoice::SYSTEM).unwrap(),
            "\"system\""
        );
        for (text, want) in [
            ("\"et\"", Some("et")),
            ("\"system\"", None),
            ("\"xx\"", None),
            ("3", None),
            ("null", None),
        ] {
            let c: LanguageChoice = serde_json::from_str(text).unwrap();
            assert_eq!(c.tag(), want, "{text}");
        }
    }

    #[test]
    fn the_system_list_does_not_fail() {
        // Whatever the machine says, the list can be negotiated.
        let tag = resolve_language(LanguageChoice::SYSTEM, &system_languages());
        assert!(kadakas_l10n::languages().iter().any(|l| l.tag == tag));
    }
}
