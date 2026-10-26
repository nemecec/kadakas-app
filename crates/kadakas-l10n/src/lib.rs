//! Localization for Kadakas with Project Fluent (PLAT-05).
//!
//! All text that a person reads in the user interface lives in `.ftl` files under `locales/`.
//! The files are built into the program with `include_str!`, so the crate also works in the
//! browser build. Code asks for text by message id:
//!
//! ```
//! use kadakas_l10n::tr;
//! assert_eq!(tr!("common-ok"), "OK");
//! assert_eq!(tr!("common-delete-n", count = 2), "Delete 2 objects");
//! ```
//!
//! What is localized, and what is not:
//!
//! - Localized: menus, palettes, dialogs, tool names and hints, status messages, readouts.
//! - Not localized: the JSON-RPC API, the CLI output, and the error texts of the API. They are
//!   for agents and scripts, so they stay in English and keep their form. A UI that shows such
//!   an error shows it inside a localized sentence.
//!
//! The active language is process wide (`set_language`), so a worker thread that builds a
//! message uses the same language as the window. `with_language` overrides it for the current
//! thread only; tests use it. The language is not document data.

mod lang;
mod localizer;
mod overlay;
mod resources;

#[doc(hidden)]
pub mod argconv;

pub use lang::{FALLBACK, Lang, languages, negotiate, normalize_tag};
pub use localizer::{Arg, Localizer};
pub use overlay::{
    MAX_TEXT, OVERLAYABLE, OverlayProblem, check_overlay, is_overlayable, overlay_tags,
    set_overlay, with_overlay,
};
pub use resources::{message_ids, message_variables, problems, source_files};

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Index into `languages()` of the language of the whole process.
static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// The decimal symbol of numbers inside messages, as a code point (`.` or `,`).
static DECIMAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new('.' as u32);

thread_local! {
    /// A decimal symbol that overrides the process one on this thread (tests).
    static DECIMAL_OVERRIDE: std::cell::Cell<Option<char>> = const { std::cell::Cell::new(None) };
}

/// Sets how a number inside a message is written: `.` (the default) or `,`. The UI calls this
/// with the number format of the person (`kadakas_units::current().number.decimal`) when it
/// starts and when the person changes the language or the number format. Files, the API and
/// the command line do not use messages, so they are not affected.
pub fn set_decimal_symbol(symbol: char) {
    DECIMAL.store(symbol as u32, Ordering::Relaxed);
}

/// The decimal symbol of numbers inside messages.
pub fn decimal_symbol() -> char {
    DECIMAL_OVERRIDE
        .with(std::cell::Cell::get)
        .unwrap_or_else(|| char::from_u32(DECIMAL.load(Ordering::Relaxed)).unwrap_or('.'))
}

/// Runs `f` with another decimal symbol on this thread only. For tests.
pub fn with_decimal_symbol<R>(symbol: char, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<char>);
    impl Drop for Restore {
        fn drop(&mut self) {
            DECIMAL_OVERRIDE.with(|o| o.set(self.0));
        }
    }
    let _restore = Restore(DECIMAL_OVERRIDE.with(|o| o.replace(Some(symbol))));
    f()
}

thread_local! {
    /// The localizer of this thread, with the language index that it was made for.
    static CACHE: RefCell<Option<(usize, usize, Rc<Localizer>)>> = const { RefCell::new(None) };
    /// A language that overrides the process language on this thread.
    static OVERRIDE: RefCell<Option<usize>> = const { RefCell::new(None) };
}

fn current_index() -> usize {
    OVERRIDE
        .with(|o| *o.borrow())
        .unwrap_or_else(|| ACTIVE.load(Ordering::Relaxed))
}

fn localizer() -> Rc<Localizer> {
    let index = current_index();
    let generation = overlay::generation();
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        match &*c {
            Some((i, g, l)) if *i == index && *g == generation => l.clone(),
            _ => {
                let tag = languages()[index].tag;
                let l = Rc::new(Localizer::with_overlay(
                    tag,
                    overlay::text_for(tag).as_deref(),
                ));
                *c = Some((index, generation, l.clone()));
                l
            }
        }
    })
}

/// Sets the language of the process. The tag can be any BCP 47 tag or a POSIX locale name; the
/// closest supported language is used (see `negotiate`). Returns the tag that is now active.
pub fn set_language(tag: &str) -> &'static str {
    let tag = negotiate(&[tag]);
    let index = languages()
        .iter()
        .position(|l| l.tag == tag)
        .unwrap_or_default();
    ACTIVE.store(index, Ordering::Relaxed);
    languages()[index].tag
}

/// The tag of the active language, for example `en-US`.
pub fn language() -> &'static str {
    languages()[current_index()].tag
}

/// Runs `f` with another language on this thread only. For tests.
pub fn with_language<R>(tag: &str, f: impl FnOnce() -> R) -> R {
    let tag = negotiate(&[tag]);
    let index = languages()
        .iter()
        .position(|l| l.tag == tag)
        .unwrap_or_default();
    /// Puts the old override back, also when `f` panics.
    struct Restore(Option<usize>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDE.with(|o| *o.borrow_mut() = self.0);
        }
    }
    let _restore = Restore(OVERRIDE.with(|o| o.replace(Some(index))));
    f()
}

/// The text of a message in the active language. A message that the language lacks comes from
/// en-US. An unknown id gives the id itself, so a mistake is easy to see (a test finds it).
/// `id` can name an attribute: `message-id.attribute`.
pub fn translate(id: &str, args: &[(&str, Arg)]) -> String {
    localizer().tr(id, args)
}

/// The text of a message, with optional named arguments.
///
/// `tr!("file-new")` gives the text. `tr!("objects-selected", count = n, zoom = z)` passes the
/// arguments to Fluent, where `{ $count }` is the placeholder and `[one]` and `[other]` select a
/// plural form. A number argument is a real number for the plural rules. Format a number with
/// `kadakas-units` first when the text must show it, and pass the result as a string.
#[macro_export]
macro_rules! tr {
    ($id:expr $(,)?) => {
        $crate::translate($id, &[])
    };
    ($id:expr, $($name:ident = $value:expr),+ $(,)?) => {{
        #[allow(unused_imports)]
        use $crate::argconv::{ViaDisplay as _, ViaNum as _};
        $crate::translate(
            $id,
            &[$((stringify!($name), (&$crate::argconv::Wrap(&$value)).to_arg())),+],
        )
    }};
}

/// Marks a message id in a table or a `static`, where a call of `tr!` is not possible. It gives
/// the id as a `&'static str`; the code later passes it to `translate`. The CI check
/// (`tests/l10n_check.rs` of `kadakas-app`) finds the ids by this macro, like those of `tr!`.
#[macro_export]
macro_rules! msg {
    ($id:literal) => {
        $id
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_macro_works_with_and_without_arguments() {
        assert_eq!(tr!("common-ok"), "OK");
        assert_eq!(tr!("common-delete-n", count = 1), "Delete 1 object");
        assert_eq!(tr!("common-delete-n", count = 3,), "Delete 3 objects");
    }

    #[test]
    fn a_number_inside_a_message_follows_the_decimal_symbol() {
        assert_eq!(tr!("common-delete-n", count = 2.5), "Delete 2.5 objects");
        with_decimal_symbol(',', || {
            assert_eq!(tr!("common-delete-n", count = 2.5), "Delete 2,5 objects");
            assert_eq!(
                tr!("common-delete-n", count = -0.25f32),
                "Delete -0,25 objects"
            );
            // Whole numbers and counts are the same in every format, and plurals still work.
            assert_eq!(tr!("common-delete-n", count = 1), "Delete 1 object");
            assert_eq!(
                tr!("common-delete-n", count = 12345),
                "Delete 12345 objects"
            );
            // Text arguments are not touched.
            assert_eq!(tr!("common-test-hello", name = "1.5"), "Hello 1.5");
            assert_eq!(decimal_symbol(), ',');
        });
        assert_eq!(decimal_symbol(), '.');
        with_language("et", || {
            with_decimal_symbol(',', || {
                assert!(tr!("common-delete-n", count = 2.5).contains("2,5"));
            });
        });
    }

    #[test]
    fn the_language_switches_for_one_thread() {
        assert_eq!(language(), "en-US");
        with_language("et", || {
            assert_eq!(language(), "et");
            assert_eq!(tr!("common-ok"), "OK");
            assert_eq!(tr!("common-cancel"), "Katkesta");
            assert_eq!(tr!("common-delete-n", count = 1), "Kustuta 1 objekt");
            with_language("en", || assert_eq!(language(), "en-US"));
            assert_eq!(language(), "et");
        });
        assert_eq!(language(), "en-US");
    }

    #[test]
    fn the_override_is_restored_after_a_panic() {
        let result = std::panic::catch_unwind(|| {
            with_language("et", || {
                assert_eq!(language(), "et");
                panic!("on purpose");
            })
        });
        assert!(result.is_err());
        assert_eq!(language(), "en-US");
    }

    #[test]
    fn msg_gives_the_id() {
        const ID: &str = msg!("common-ok");
        assert_eq!(translate(ID, &[]), "OK");
    }

    #[test]
    fn an_unknown_id_gives_the_id() {
        assert_eq!(tr!("no-such-message"), "no-such-message");
    }
}
