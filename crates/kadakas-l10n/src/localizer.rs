//! One language: a Fluent bundle for it and a bundle for en-US behind it.

use std::cell::RefCell;
use std::collections::HashMap;

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

use crate::{FALLBACK, languages};

/// A named argument of a message.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    /// A number. Plural rules (`[one]`, `[other]`) select on it.
    Num(f64),
    /// Text, for example a number that `kadakas-units` has formatted.
    Text(String),
}

macro_rules! arg_from_num {
    ($($t:ty),*) => {$(
        impl From<$t> for Arg {
            fn from(v: $t) -> Arg {
                Arg::Num(v as f64)
            }
        }
    )*};
}
arg_from_num!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64);

impl From<&str> for Arg {
    fn from(v: &str) -> Arg {
        Arg::Text(v.to_string())
    }
}
impl From<String> for Arg {
    fn from(v: String) -> Arg {
        Arg::Text(v)
    }
}
impl From<&String> for Arg {
    fn from(v: &String) -> Arg {
        Arg::Text(v.clone())
    }
}

/// The text of one language, with en-US as the fallback for messages that are missing.
pub struct Localizer {
    tag: &'static str,
    main: FluentBundle<FluentResource>,
    /// `None` when the language is en-US itself.
    fallback: Option<FluentBundle<FluentResource>>,
    /// The terminology overlay of the workspace profile for this language (PLAT-04). Its
    /// messages win over the messages of the language.
    overlay: Option<FluentBundle<FluentResource>>,
    /// The texts of messages that are asked without arguments. A UI asks for the same labels
    /// in each frame.
    cache: RefCell<HashMap<String, String>>,
}

fn bundle(tag: &str) -> FluentBundle<FluentResource> {
    let lang = languages()
        .iter()
        .find(|l| l.tag == tag)
        .expect("known tag");
    bundle_from(tag, lang.files.iter().map(|(_, text)| *text))
}

fn bundle_from<'a>(
    tag: &str,
    texts: impl Iterator<Item = &'a str>,
) -> FluentBundle<FluentResource> {
    let id: LanguageIdentifier = tag.parse().expect("the tags in lang.rs are valid");
    let mut b = FluentBundle::new(vec![id]);
    // Unicode isolation marks around arguments are noise in a plain UI.
    b.set_use_isolating(false);
    for text in texts {
        // A file with a syntax error keeps its good messages (`Err` holds both). The CI check
        // reports the error.
        let res = match FluentResource::try_new(text.to_string()) {
            Ok(r) => r,
            Err((r, _)) => r,
        };
        // A duplicate id keeps the first message; the CI check reports it too.
        let _ = b.add_resource(res);
    }
    b
}

impl Localizer {
    /// A localizer for a supported tag. An unknown tag gives en-US.
    pub fn new(tag: &str) -> Localizer {
        Localizer::with_overlay(tag, None)
    }

    /// A localizer whose messages the `.ftl` text `overlay` can replace. The overlay should have
    /// passed `check_overlay`; a message in it that does not format falls back to the
    /// normal text.
    pub fn with_overlay(tag: &str, overlay: Option<&str>) -> Localizer {
        let tag = languages()
            .iter()
            .find(|l| l.tag == tag)
            .map_or(FALLBACK, |l| l.tag);
        Localizer {
            tag,
            main: bundle(tag),
            fallback: (tag != FALLBACK).then(|| bundle(FALLBACK)),
            overlay: overlay.map(|text| bundle_from(tag, std::iter::once(text))),
            cache: RefCell::default(),
        }
    }

    /// The tag of the language.
    pub fn tag(&self) -> &'static str {
        self.tag
    }

    fn format(
        bundle: &FluentBundle<FluentResource>,
        id: &str,
        args: &FluentArgs,
    ) -> Option<String> {
        let (name, attr) = match id.split_once('.') {
            Some((n, a)) => (n, Some(a)),
            None => (id, None),
        };
        let msg = bundle.get_message(name)?;
        let pattern = match attr {
            Some(a) => msg.get_attribute(a)?.value(),
            None => msg.value()?,
        };
        let mut errors = Vec::new();
        Some(
            bundle
                .format_pattern(pattern, Some(args), &mut errors)
                .into_owned(),
        )
    }

    /// The errors that Fluent finds when it formats a message of this language (not the
    /// fallback) with these arguments. Empty for a message that is fine or missing.
    pub fn format_errors(&self, id: &str, args: &[(&str, Arg)]) -> Vec<String> {
        let mut fa = FluentArgs::new();
        for (name, value) in args {
            match value {
                Arg::Num(n) => fa.set(*name, FluentValue::from(*n)),
                Arg::Text(t) => fa.set(*name, FluentValue::from(t.clone())),
            }
        }
        let (name, attr) = match id.split_once('.') {
            Some((n, a)) => (n, Some(a)),
            None => (id, None),
        };
        let Some(msg) = self.main.get_message(name) else {
            return Vec::new();
        };
        let pattern = match attr {
            Some(a) => msg.get_attribute(a).map(|a| a.value()),
            None => msg.value(),
        };
        let Some(pattern) = pattern else {
            return Vec::new();
        };
        let mut errors = Vec::new();
        let _ = self.main.format_pattern(pattern, Some(&fa), &mut errors);
        errors.iter().map(|e| e.to_string()).collect()
    }

    /// True if this language itself (not the fallback) has the message.
    pub fn has(&self, id: &str) -> bool {
        Self::format(&self.main, id, &FluentArgs::new()).is_some()
    }

    /// The text of a message. See `crate::translate`.
    pub fn tr(&self, id: &str, args: &[(&str, Arg)]) -> String {
        if args.is_empty() {
            if let Some(text) = self.cache.borrow().get(id) {
                return text.clone();
            }
            let text = self.tr_uncached(id, args);
            self.cache.borrow_mut().insert(id.to_string(), text.clone());
            return text;
        }
        self.tr_uncached(id, args)
    }

    fn tr_uncached(&self, id: &str, args: &[(&str, Arg)]) -> String {
        let mut fa = FluentArgs::new();
        for (name, value) in args {
            match value {
                Arg::Num(n) => fa.set(*name, FluentValue::from(*n)),
                Arg::Text(t) => fa.set(*name, FluentValue::from(t.clone())),
            }
        }
        self.overlay
            .as_ref()
            .and_then(|b| Self::format(b, id, &fa))
            .or_else(|| Self::format(&self.main, id, &fa))
            .or_else(|| {
                self.fallback
                    .as_ref()
                    .and_then(|b| Self::format(b, id, &fa))
            })
            .unwrap_or_else(|| id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_convert() {
        assert_eq!(Arg::from(3usize), Arg::Num(3.0));
        assert_eq!(Arg::from(-2i32), Arg::Num(-2.0));
        assert_eq!(Arg::from(1.5f64), Arg::Num(1.5));
        assert_eq!(Arg::from("a"), Arg::Text("a".into()));
        assert_eq!(Arg::from(String::from("b")), Arg::Text("b".into()));
        assert_eq!(Arg::from(&String::from("c")), Arg::Text("c".into()));
    }

    #[test]
    fn plurals_follow_the_language() {
        let en = Localizer::new("en-US");
        assert_eq!(en.tag(), "en-US");
        assert_eq!(
            en.tr("common-delete-n", &[("count", 1.into())]),
            "Delete 1 object"
        );
        assert_eq!(
            en.tr("common-delete-n", &[("count", 0.into())]),
            "Delete 0 objects"
        );
        let et = Localizer::new("et");
        assert_eq!(
            et.tr("common-delete-n", &[("count", 2.into())]),
            "Kustuta 2 objekti"
        );
    }

    #[test]
    fn text_arguments_and_attributes_work() {
        let en = Localizer::new("en-US");
        assert_eq!(
            en.tr("common-test-hello", &[("name", "Mari".into())]),
            "Hello Mari"
        );
        assert_eq!(en.tr("common-test-hello.title", &[]), "Greeting");
        // A missing attribute and a missing message give the id.
        assert_eq!(
            en.tr("common-test-hello.nope", &[]),
            "common-test-hello.nope"
        );
        assert!(en.has("common-test-hello"));
        assert!(!en.has("nope"));
    }

    #[test]
    fn a_message_missing_in_a_language_comes_from_en_us() {
        let et = Localizer {
            tag: "et",
            main: bundle_from("et", ["a = A\n"].into_iter()),
            fallback: Some(bundle_from(
                "en-US",
                ["a = X\nb = B\n", "bad = {\n"].into_iter(),
            )),
            overlay: None,
            cache: RefCell::default(),
        };
        assert_eq!(et.tr("a", &[]), "A");
        assert!(!et.has("b"));
        assert_eq!(et.tr("b", &[]), "B");
        // A file with a syntax error keeps its good messages.
        assert_eq!(et.tr("bad", &[]), "bad");
    }

    #[test]
    fn format_errors_are_found_and_missing_messages_are_quiet() {
        let en = Localizer::new("en-US");
        // A message without its variable has an error; with it, none.
        assert!(!en.format_errors("common-test-hello", &[]).is_empty());
        assert!(
            en.format_errors("common-test-hello", &[("name", "x".into())])
                .is_empty()
        );
        // An attribute is checked too. A missing message, attribute or value is no error.
        assert!(en.format_errors("common-test-hello.title", &[]).is_empty());
        assert!(en.format_errors("common-test-hello.nope", &[]).is_empty());
        assert!(en.format_errors("no-such-message", &[]).is_empty());
    }

    #[test]
    fn an_overlay_wins_over_the_messages_of_the_language() {
        let plain = Localizer::new("en-US");
        let over = Localizer::with_overlay("en-US", Some("common-ok = Fine\n"));
        assert_eq!(plain.tr("common-ok", &[]), "OK");
        assert_eq!(over.tr("common-ok", &[]), "Fine");
        // Other messages stay.
        assert_eq!(
            over.tr("common-cancel", &[]),
            plain.tr("common-cancel", &[])
        );
        // The overlay of one language does not touch another one.
        let et = Localizer::with_overlay("et", Some("common-cancel = Peata\n"));
        assert_eq!(et.tr("common-cancel", &[]), "Peata");
        assert_eq!(et.tr("common-ok", &[]), "OK");
    }

    #[test]
    fn an_unknown_tag_gives_en_us() {
        assert_eq!(Localizer::new("xx").tag(), "en-US");
    }
}
