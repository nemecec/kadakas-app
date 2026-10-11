//! What can go wrong when a profile file is read. The loader keeps going when it can, and lists
//! the problems so the UI can show them.

use kadakas_l10n::{OverlayProblem, tr};

use crate::keymap::Action;

/// A problem that the loader worked around.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// A part of the file has the wrong JSON type. The default of that part is used.
    WrongType(String),
    /// An action id that no action has.
    UnknownAction(String),
    BadChord {
        action: String,
        text: String,
    },
    /// A chord that two actions asked for. The later action got it.
    ChordMoved {
        chord: String,
        from: String,
        to: String,
    },
    BadAlias(String),
    UnknownTool {
        set: String,
        tool: String,
    },
    DuplicateTool {
        set: String,
        tool: String,
    },
    UnknownItem {
        menu: String,
        item: String,
    },
    DuplicateId(String),
    BadId(String),
    BadName(String),
    NeedsName(String),
    TermsLanguage(String),
    Terms {
        lang: String,
        problem: OverlayProblem,
    },
    BadUnits(String),
    /// A list has more entries than a profile can hold.
    TooMany(String),
    /// A text is longer than allowed and was cut.
    TooLong(String),
}

fn action_name(id: &str) -> String {
    Action::from_id(id).map_or_else(|| id.to_string(), Action::label)
}

impl Problem {
    /// The problem in the language of the UI.
    pub fn text(&self) -> String {
        match self {
            Problem::WrongType(part) => tr!("uic-profile-problem-wrong-type", part = part),
            Problem::UnknownAction(id) => tr!("uic-profile-problem-unknown-action", id = id),
            Problem::BadChord { action, text } => {
                tr!(
                    "uic-profile-problem-bad-chord",
                    action = action,
                    text = text
                )
            }
            Problem::ChordMoved { chord, from, to } => tr!(
                "uic-profile-problem-chord-moved",
                chord = chord,
                from = action_name(from),
                to = action_name(to)
            ),
            Problem::BadAlias(a) => tr!("uic-profile-problem-bad-alias", alias = a),
            Problem::UnknownTool { set, tool } => {
                tr!("uic-profile-problem-unknown-tool", set = set, tool = tool)
            }
            Problem::DuplicateTool { set, tool } => {
                tr!("uic-profile-problem-duplicate-tool", set = set, tool = tool)
            }
            Problem::UnknownItem { menu, item } => {
                tr!("uic-profile-problem-unknown-item", menu = menu, item = item)
            }
            Problem::DuplicateId(id) => tr!("uic-profile-problem-duplicate-id", id = id),
            Problem::BadId(id) => tr!("uic-profile-problem-bad-id", id = id),
            Problem::BadName(id) => tr!("uic-profile-problem-bad-name", id = id),
            Problem::NeedsName(id) => tr!("uic-profile-problem-needs-name", id = id),
            Problem::TermsLanguage(tag) => tr!("uic-profile-problem-terms-language", lang = tag),
            Problem::Terms { lang, problem } => match problem {
                OverlayProblem::UnknownLanguage(_) => {
                    tr!("uic-profile-problem-terms-language", lang = lang)
                }
                OverlayProblem::Syntax(line) => {
                    tr!(
                        "uic-profile-problem-terms-syntax",
                        lang = lang,
                        line = *line
                    )
                }
                OverlayProblem::TooLong => tr!("uic-profile-problem-terms-too-long", lang = lang),
                OverlayProblem::NotAllowed(id) => {
                    tr!(
                        "uic-profile-problem-terms-not-allowed",
                        lang = lang,
                        id = id
                    )
                }
                OverlayProblem::Reference(id) => {
                    tr!("uic-profile-problem-terms-reference", lang = lang, id = id)
                }
                OverlayProblem::UnknownMessage(id) => {
                    tr!("uic-profile-problem-terms-message", lang = lang, id = id)
                }
                OverlayProblem::UnknownVariable { id, variable } => tr!(
                    "uic-profile-problem-terms-variable",
                    lang = lang,
                    id = id,
                    variable = variable
                ),
            },
            Problem::BadUnits(detail) => tr!("uic-profile-problem-bad-units", detail = detail),
            Problem::TooMany(part) => tr!("uic-profile-problem-too-many", part = part),
            Problem::TooLong(part) => tr!("uic-profile-problem-too-long", part = part),
        }
    }
}

/// Why a file is no profile at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    NotJson(String),
    NotAnObject,
    /// The file has no `format` number.
    NoFormat,
    /// The `format` is 0 or newer than this program knows.
    BadFormat(u64),
    NoName,
    NameTooLong,
    /// The file is larger than [`super::MAX_FILE_BYTES`].
    TooLarge,
}

impl ParseError {
    /// The error in the language of the UI.
    pub fn text(&self) -> String {
        match self {
            ParseError::NotJson(d) => tr!("uic-profile-error-not-json", detail = d),
            ParseError::NotAnObject => tr!("uic-profile-error-not-an-object"),
            ParseError::NoFormat => tr!("uic-profile-error-no-format"),
            ParseError::BadFormat(n) => tr!("uic-profile-error-bad-format", format = n.to_string()),
            ParseError::NoName => tr!("uic-profile-error-no-name"),
            ParseError::NameTooLong => tr!("uic-profile-error-name-too-long"),
            ParseError::TooLarge => tr!("uic-profile-error-too-large"),
        }
    }
}
