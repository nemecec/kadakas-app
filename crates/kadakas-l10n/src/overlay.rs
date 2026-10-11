//! The terminology overlay of a workspace profile (PLAT-04).
//!
//! An overlay is one `.ftl` text per language. Its messages replace messages of the program
//! with the same id, for example `panels-class = Layer`. The overlay belongs to the process,
//! like the active language: it is a preference of the person, not document data. An overlay for
//! a language applies only while that language is active, so a profile needs one text per
//! language that it wants to change.

use std::sync::RwLock;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The active overlay as (language tag, `.ftl` text).
static OVERLAY: RwLock<Vec<(&'static str, String)>> = RwLock::new(Vec::new());

/// Counts the changes of the overlay, so a thread knows that its localizer is old.
static GENERATION: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn generation() -> usize {
    GENERATION.load(Ordering::Relaxed)
}

/// The overlay text of a language, if any.
pub(crate) fn text_for(tag: &str) -> Option<String> {
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
/// is not the tag of a supported language is ignored. An empty list removes the overlay.
pub fn set_overlay(texts: &[(&str, &str)]) {
    let mut list: Vec<(&'static str, String)> = Vec::new();
    for (tag, text) in texts {
        let exact = crate::languages()
            .iter()
            .find(|l| l.tag.eq_ignore_ascii_case(tag));
        if let Some(l) = exact {
            list.retain(|(t, _)| *t != l.tag);
            list.push((l.tag, (*text).to_string()));
        }
    }
    *OVERLAY.write().unwrap_or_else(|e| e.into_inner()) = list;
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// A problem of an overlay text. The CI checks and the profile loader show these.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlayProblem {
    /// The tag is not a supported language.
    UnknownLanguage(String),
    /// The text has a syntax error near this line.
    Syntax(usize),
    /// The overlay replaces a message that the language does not have.
    UnknownMessage(String),
    /// The overlay message uses a variable that the original does not have.
    UnknownVariable { id: String, variable: String },
}

/// Checks an overlay text for a language. An empty list means the overlay is good.
pub fn check_overlay(tag: &str, text: &str) -> Vec<OverlayProblem> {
    let Ok(original) = crate::message_variables(tag) else {
        return vec![OverlayProblem::UnknownLanguage(tag.to_string())];
    };
    let mine = match crate::resources::variables_in(text) {
        Ok(m) => m,
        Err(line) => return vec![OverlayProblem::Syntax(line)],
    };
    let mut out = Vec::new();
    for (id, vars) in &mine {
        let Some(have) = original.get(id) else {
            out.push(OverlayProblem::UnknownMessage(id.clone()));
            continue;
        };
        for v in vars.difference(have) {
            out.push(OverlayProblem::UnknownVariable {
                id: id.clone(),
                variable: v.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_overlay_text_is_checked() {
        assert!(check_overlay("en-US", "common-ok = Fine\n").is_empty());
        assert!(
            check_overlay("en-US", "common-delete-n = Remove { $count }\n").is_empty(),
            "a variable of the original is fine"
        );
        assert_eq!(
            check_overlay("en-US", "common-ok = Fine { $x }\n"),
            vec![OverlayProblem::UnknownVariable {
                id: "common-ok".into(),
                variable: "x".into()
            }]
        );
        assert_eq!(
            check_overlay("en-US", "no-such = x\n"),
            vec![OverlayProblem::UnknownMessage("no-such".into())]
        );
        assert_eq!(
            check_overlay("en-US", "ok = {\n"),
            vec![OverlayProblem::Syntax(2)]
        );
        assert_eq!(
            check_overlay("xx", ""),
            vec![OverlayProblem::UnknownLanguage("xx".into())]
        );
    }
}
