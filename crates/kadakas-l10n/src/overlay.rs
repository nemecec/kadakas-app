//! The terminology overlay of a workspace profile (PLAT-04).
//!
//! An overlay is one `.ftl` text per language. Its messages replace messages of the program
//! with the same id, for example `panels-class = Layer`. The overlay belongs to the process,
//! like the active language: it is a preference of the person, not document data. An overlay for
//! a language applies only while that language is active, so a profile needs one text per
//! language that it wants to change.
//!
//! An overlay can rename terms only. A message may be replaced only when it is in
//! [`OVERLAYABLE`] and not in a denied group (about, licenses, scripts, the profile windows and
//! the common buttons). `set_overlay` and `with_overlay` drop every other message, so no caller
//! can skip the rule. A replaced message may use the variables of the original only, and it may
//! not refer to other messages or terms.

use std::collections::BTreeSet;
use std::sync::RwLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use fluent_syntax::ast::{Entry, Expression, InlineExpression, Pattern, PatternElement, Resource};

/// The longest overlay text of one language, in bytes.
pub const MAX_TEXT: usize = 64 * 1024;

/// Message groups that no overlay may change.
const DENIED_PREFIXES: &[&str] = &[
    "script-ui-",
    "about-",
    "autosave-ui-",
    "profiles-ui-",
    "uic-profile-",
    "common-",
];

/// The messages that an overlay may replace: the terms of the work (class, layer, tool and
/// dialog names), not the texts that guard files, licenses or settings.
pub const OVERLAYABLE: &[&str] = &[
    "action-cmd-duplicate-array",
    "action-cmd-sweep",
    "action-databar-focus",
    "action-databar-prev",
    "action-tool-polygon3d",
    "action-tool-select",
    "app-add-a-sheet-layer-first-navigation-sheet",
    "area-ui-the-formation-level-under-the-build-up",
    "array-ui-duplicate-array",
    "data-ui-add-the-worksheet-as-a-table-on",
    "io-dialog-building-storeys-become-design-layers-off-all",
    "io-dialog-class-from-attribute",
    "io-dialog-classes-become-ifc-presentation-layers",
    "io-dialog-design-layer",
    "io-dialog-design-layers-become-building-storeys-default-everyt",
    "io-dialog-dxf-layers-become-design-layers-default-classes",
    "io-dialog-empty-a-new-layer-named-after-the-file",
    "io-dialog-one-dxf-layer-per-design-layer-default",
    "io-dialog-only-visible-layers-and-classes",
    "panels-add-class",
    "panels-add-design-layer",
    "panels-class",
    "panels-classes",
    "panels-design-layers",
    "panels-duplicate-array",
    "panels-layer",
    "panels-lock-layer",
    "panels-nothing-selected-active-layer-active-class",
    "panels-override-the-class-attribute",
    "panels-radius-of-new-arc-vertices-type-a",
    "panels-sweep",
    "plant-ui-back-to-the-elevation-of-the-layer",
    "plant-ui-no-site-model-on-the-layer-of",
    "plant-ui-none-use-the-data-bar",
    "plant-ui-off-use-the-class-color",
    "sheets-ui-add-sheet-layer",
    "sheets-ui-sheet-layers",
    "snap-ui-objects-of-grey-layers-and-classes-cannot",
    "snap-ui-off-only-objects-of-the-active-layer",
    "snap-ui-snap-to-grey-layers-and-classes",
    "snap-ui-snap-to-other-layers",
    "solid-ui-sweep",
    "uic-annot-tool-click-to-place-the-scale-bar-it",
    "uic-style-edit-kind-dimension",
    "uic-tools-click-to-place-set-values-in-the",
    "uic-tools-pick-a-mode-in-the-data-bar",
    "uic-tools-polygon3d",
];

/// True when an overlay may replace the message `id`.
pub fn is_overlayable(id: &str) -> bool {
    !DENIED_PREFIXES.iter().any(|p| id.starts_with(p)) && OVERLAYABLE.contains(&id)
}

/// The active overlay as (language tag, `.ftl` text).
static OVERLAY: RwLock<Vec<(&'static str, String)>> = RwLock::new(Vec::new());

/// Counts the changes of the overlay, so a thread knows that its localizer is old.
static GENERATION: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn generation() -> usize {
    GENERATION.load(Ordering::Relaxed)
}

thread_local! {
    /// An overlay that replaces the process overlay on this thread only. For tests.
    static LOCAL: std::cell::RefCell<Option<Vec<(&'static str, String)>>> =
        const { std::cell::RefCell::new(None) };
}

/// A problem of an overlay text. The CI checks and the profile loader show these.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlayProblem {
    /// The tag is not a supported language.
    UnknownLanguage(String),
    /// The text has a syntax error near this line.
    Syntax(usize),
    /// The text is longer than [`MAX_TEXT`].
    TooLong,
    /// The overlay replaces a message that the language does not have.
    UnknownMessage(String),
    /// The message exists, but an overlay may not replace it.
    NotAllowed(String),
    /// The overlay message uses a variable that the original does not have.
    UnknownVariable { id: String, variable: String },
    /// The overlay message refers to another message or a term, or has attributes.
    Reference(String),
}

/// What one overlay message uses.
struct Used {
    vars: BTreeSet<String>,
    /// A reference to a message or a term, or an attribute.
    reference: bool,
}

fn in_pattern(p: &Pattern<&str>, u: &mut Used) {
    for el in &p.elements {
        if let PatternElement::Placeable { expression } = el {
            in_expression(expression, u);
        }
    }
}

fn in_inline(e: &InlineExpression<&str>, u: &mut Used) {
    match e {
        InlineExpression::VariableReference { id } => {
            u.vars.insert(id.name.to_string());
        }
        InlineExpression::MessageReference { .. } | InlineExpression::TermReference { .. } => {
            u.reference = true;
        }
        InlineExpression::Placeable { expression } => in_expression(expression, u),
        InlineExpression::FunctionReference { arguments, .. } => {
            for a in &arguments.positional {
                in_inline(a, u);
            }
            for n in &arguments.named {
                in_inline(&n.value, u);
            }
        }
        _ => {}
    }
}

fn in_expression(e: &Expression<&str>, u: &mut Used) {
    match e {
        Expression::Inline(i) => in_inline(i, u),
        Expression::Select { selector, variants } => {
            in_inline(selector, u);
            for v in variants {
                in_pattern(&v.value, u);
            }
        }
    }
}

/// Checks an overlay text and keeps its good messages. Returns the text to use (it holds only
/// the messages that pass) and the problems. A text with a syntax error, or a text that is too
/// long, gives no message at all.
fn filter(tag: &str, text: &str) -> (String, Vec<OverlayProblem>) {
    let Ok(original) = crate::message_variables(tag) else {
        return (
            String::new(),
            vec![OverlayProblem::UnknownLanguage(tag.into())],
        );
    };
    if text.len() > MAX_TEXT {
        return (String::new(), vec![OverlayProblem::TooLong]);
    }
    let res: Resource<&str> = match fluent_syntax::parser::parse(text) {
        Ok(r) => r,
        Err((_, errors)) => {
            let first = errors.first().map(|e| e.pos.start).unwrap_or(0);
            let line = text[..first.min(text.len())].matches('\n').count() + 1;
            return (String::new(), vec![OverlayProblem::Syntax(line)]);
        }
    };
    let mut problems = Vec::new();
    let mut kept = Vec::new();
    for entry in res.body {
        let Entry::Message(m) = entry else {
            continue;
        };
        let id = m.id.name.to_string();
        let Some(have) = original.get(&id) else {
            problems.push(OverlayProblem::UnknownMessage(id));
            continue;
        };
        if !is_overlayable(&id) {
            problems.push(OverlayProblem::NotAllowed(id));
            continue;
        }
        let mut used = Used {
            vars: BTreeSet::new(),
            reference: !m.attributes.is_empty() || m.value.is_none(),
        };
        if let Some(v) = &m.value {
            in_pattern(v, &mut used);
        }
        if used.reference {
            problems.push(OverlayProblem::Reference(id));
            continue;
        }
        let unknown: Vec<String> = used.vars.difference(have).cloned().collect();
        if !unknown.is_empty() {
            problems.extend(
                unknown
                    .into_iter()
                    .map(|variable| OverlayProblem::UnknownVariable {
                        id: id.clone(),
                        variable,
                    }),
            );
            continue;
        }
        kept.push(Entry::Message(m));
    }
    let clean = fluent_syntax::serializer::serialize(&Resource { body: kept });
    (clean, problems)
}

/// Checks an overlay text for a language. An empty list means the overlay is good.
pub fn check_overlay(tag: &str, text: &str) -> Vec<OverlayProblem> {
    filter(tag, text).1
}

fn build(texts: &[(&str, &str)]) -> Vec<(&'static str, String)> {
    let mut list: Vec<(&'static str, String)> = Vec::new();
    for (tag, text) in texts {
        let exact = crate::languages()
            .iter()
            .find(|l| l.tag.eq_ignore_ascii_case(tag));
        if let Some(l) = exact {
            list.retain(|(t, _)| *t != l.tag);
            list.push((l.tag, filter(l.tag, text).0));
        }
    }
    list
}

/// Runs `f` with another overlay on this thread only. Tests use it, like `with_language`.
/// The same rules as `set_overlay` apply.
pub fn with_overlay<R>(texts: &[(&str, &str)], f: impl FnOnce() -> R) -> R {
    /// Puts the old overlay back, also when `f` panics.
    struct Restore(Option<Vec<(&'static str, String)>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            LOCAL.with(|l| *l.borrow_mut() = self.0.take());
            GENERATION.fetch_add(1, Ordering::Relaxed);
        }
    }
    let _restore = Restore(LOCAL.with(|l| l.replace(Some(build(texts)))));
    GENERATION.fetch_add(1, Ordering::Relaxed);
    f()
}

/// The overlay text of a language, if any.
pub(crate) fn text_for(tag: &str) -> Option<String> {
    if let Some(found) = LOCAL.with(|l| {
        l.borrow()
            .as_ref()
            .map(|list| list.iter().find(|(t, _)| *t == tag).map(|(_, s)| s.clone()))
    }) {
        return found;
    }
    let guard = OVERLAY.read().unwrap_or_else(|e| e.into_inner());
    guard
        .iter()
        .find(|(t, _)| *t == tag)
        .map(|(_, s)| s.clone())
}

/// The language tags that the active overlay has text for.
pub fn overlay_tags() -> Vec<&'static str> {
    let guard = OVERLAY.read().unwrap_or_else(|e| e.into_inner());
    guard.iter().map(|(t, _)| *t).collect()
}

/// Sets the terminology overlay of the process. `texts` are (tag, `.ftl` text). A tag that
/// is not the tag of a supported language is ignored. Messages that an overlay may not replace
/// are dropped (see [`OVERLAYABLE`]), and so are messages with a wrong variable or a reference.
/// An empty list removes the overlay.
pub fn set_overlay(texts: &[(&str, &str)]) {
    let list = build(texts);
    *OVERLAY.write().unwrap_or_else(|e| e.into_inner()) = list;
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_overlay_text_is_checked() {
        assert!(check_overlay("en-US", "panels-class = Fine\n").is_empty());
        assert!(
            check_overlay(
                "en-US",
                "panels-nothing-selected-active-layer-active-class = { $layer } { $class }\n"
            )
            .is_empty(),
            "variables of the original are fine"
        );
        assert_eq!(
            check_overlay("en-US", "panels-class = Fine { $x }\n"),
            vec![OverlayProblem::UnknownVariable {
                id: "panels-class".into(),
                variable: "x".into()
            }]
        );
        assert_eq!(
            check_overlay("en-US", "no-such = x\n"),
            vec![OverlayProblem::UnknownMessage("no-such".into())]
        );
        assert_eq!(
            check_overlay("en-US", "common-ok = Fine\n"),
            vec![OverlayProblem::NotAllowed("common-ok".into())]
        );
        assert_eq!(
            check_overlay("en-US", "panels-class = { panels-layer }\n"),
            vec![OverlayProblem::Reference("panels-class".into())]
        );
        assert_eq!(
            check_overlay(
                "en-US",
                "panels-class = { -term }\npanels-layer = x\n    .a = b\n"
            ),
            vec![
                OverlayProblem::Reference("panels-class".into()),
                OverlayProblem::Reference("panels-layer".into())
            ]
        );
        assert_eq!(
            check_overlay("en-US", "ok = {\n"),
            vec![OverlayProblem::Syntax(2)]
        );
        assert_eq!(
            check_overlay(
                "en-US",
                &format!("panels-class = {}\n", "x".repeat(MAX_TEXT))
            ),
            vec![OverlayProblem::TooLong]
        );
        assert_eq!(
            check_overlay("xx", ""),
            vec![OverlayProblem::UnknownLanguage("xx".into())]
        );
    }

    #[test]
    fn functions_selectors_and_nested_placeables_are_checked_for_variables() {
        const ID: &str = "panels-nothing-selected-active-layer-active-class";
        let check = |body: &str| check_overlay("en-US", &format!("{ID} = {body}\n"));
        let unknown = |v: &str| {
            vec![OverlayProblem::UnknownVariable {
                id: ID.into(),
                variable: v.into(),
            }]
        };
        // The variables of the original are fine in a function, a named argument, a selector
        // and a nested placeable.
        assert!(check("{ NUMBER($layer, minimumFractionDigits: 1) } { $class }").is_empty());
        assert!(check("{ $layer ->\n    [a] x { $class }\n   *[b] { $layer }\n}").is_empty());
        assert!(check("{ { $class } }").is_empty());
        // Others are not, wherever they stand.
        assert_eq!(check("{ NUMBER($zz) }"), unknown("zz"));
        assert_eq!(check("{ $zz ->\n   *[b] x\n}"), unknown("zz"));
        assert_eq!(
            check("{ $layer ->\n    [a] { $zz }\n   *[b] x\n}"),
            unknown("zz")
        );
        assert_eq!(check("{ { $zz } }"), unknown("zz"));
        // A function that refers to a message is a reference.
        assert_eq!(
            check("{ NUMBER(panels-class) }"),
            vec![OverlayProblem::Reference(ID.into())]
        );
    }

    #[test]
    fn bad_messages_are_dropped_and_good_ones_stay() {
        let (clean, problems) = filter(
            "en-US",
            "# a comment\npanels-class = Layer\ncommon-ok = Fine\n-term = x\npanels-layer = { panels-class }\nsheets-ui-sheet-layers = Layouts\n",
        );
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(clean.contains("panels-class = Layer"));
        assert!(clean.contains("sheets-ui-sheet-layers = Layouts"));
        assert!(!clean.contains("common-ok") && !clean.contains("-term"));
        assert!(!clean.contains("{ panels-class }"));
        // A syntax error drops the whole text.
        assert!(filter("en-US", "a = {\n").0.is_empty());
    }

    #[test]
    fn the_list_of_overlayable_messages_is_good() {
        let ids = crate::message_ids("en-US").unwrap();
        let mut seen = BTreeSet::new();
        for id in OVERLAYABLE {
            assert!(ids.contains(*id), "{id} is no message");
            assert!(seen.insert(*id), "{id} twice");
            assert!(is_overlayable(id), "{id} is in a denied group");
            for p in DENIED_PREFIXES {
                assert!(!id.starts_with(p));
            }
        }
        assert!(!is_overlayable("common-ok") && !is_overlayable("about-kadakas"));
        assert!(!is_overlayable("panels-save") && !is_overlayable("script-ui-run"));
        // The list is sorted, so a double entry is easy to see.
        let mut sorted = OVERLAYABLE.to_vec();
        sorted.sort();
        assert_eq!(sorted, OVERLAYABLE);
    }
}
