//! The command alias input (PLAT-04).
//!
//! Some profiles name commands by short aliases, for example `PL` for the polyline tool. The
//! app has no command line in the CAD sense. It has a small field: the person types letters and
//! presses Enter. This module holds the state of that field and finds the action of an alias.
//! The UI program owns the field and draws it.

use std::collections::BTreeMap;

use crate::keymap::Action;

/// Aliases of a profile: the alias in upper case and the action it runs.
pub type Aliases = BTreeMap<String, Action>;

/// The longest alias, and the longest text of the field.
pub const MAX_LEN: usize = 16;

/// How many commands the history keeps.
const HISTORY: usize = 50;

/// The alias that `text` makes: letters and digits in upper case, at most [`MAX_LEN`]. `None`
/// when nothing is left.
pub fn normalize_alias(text: &str) -> Option<String> {
    let s: String = text
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .take(MAX_LEN)
        .collect();
    (!s.is_empty()).then_some(s)
}

/// True when `text` is a good alias as it stands: one to [`MAX_LEN`] upper case letters or digits.
pub fn valid_alias(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_LEN
        && text
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// What Enter did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Run this action.
    Run(Action),
    /// The field was empty and the last command runs again.
    Repeat(Action),
    /// The field was empty and there is nothing to repeat.
    Nothing,
    /// The text is no alias. The text stays in the field.
    Unknown(String),
}

/// The state of the command alias field.
#[derive(Clone, Debug, Default)]
pub struct CommandLine {
    text: String,
    /// Aliases that ran, oldest first.
    history: Vec<String>,
    /// The place in the history while the person steps through it.
    cursor: Option<usize>,
    last: Option<Action>,
}

impl CommandLine {
    pub fn new() -> CommandLine {
        CommandLine::default()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Replaces the text, for a UI program whose text field edits its own copy. Characters that
    /// no alias has are dropped; letters become upper case.
    pub fn set_text(&mut self, text: &str) {
        self.text = normalize_alias(text).unwrap_or_default();
        self.cursor = None;
    }

    /// Adds typed characters at the end.
    pub fn type_text(&mut self, typed: &str) {
        let joined = format!("{}{}", self.text, typed);
        self.set_text(&joined);
    }

    pub fn backspace(&mut self) {
        self.text.pop();
        self.cursor = None;
    }

    /// Escape: empties the field.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = None;
    }

    /// Aliases that start with the text, shortest first, at most `limit`. An empty field gives
    /// none.
    pub fn suggestions(&self, aliases: &Aliases, limit: usize) -> Vec<(String, Action)> {
        if self.text.is_empty() {
            return Vec::new();
        }
        let mut found: Vec<(String, Action)> = aliases
            .iter()
            .filter(|(a, _)| a.starts_with(&self.text))
            .map(|(a, act)| (a.clone(), *act))
            .collect();
        found.sort_by(|x, y| x.0.len().cmp(&y.0.len()).then_with(|| x.0.cmp(&y.0)));
        found.truncate(limit);
        found
    }

    /// Enter. An alias that matches runs and goes to the history. An empty field repeats the
    /// last action. Another text stays in the field and gives `Unknown`.
    pub fn enter(&mut self, aliases: &Aliases) -> Outcome {
        if self.text.is_empty() {
            return match self.last {
                Some(a) => Outcome::Repeat(a),
                None => Outcome::Nothing,
            };
        }
        match aliases.get(&self.text) {
            Some(action) => {
                let action = *action;
                self.last = Some(action);
                if self.history.last() != Some(&self.text) {
                    self.history.push(self.text.clone());
                    if self.history.len() > HISTORY {
                        self.history.remove(0);
                    }
                }
                self.text.clear();
                self.cursor = None;
                Outcome::Run(action)
            }
            None => Outcome::Unknown(self.text.clone()),
        }
    }

    /// The Up key: the previous command of the history in the field.
    pub fn history_previous(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let at = match self.cursor {
            None => self.history.len() - 1,
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.cursor = Some(at);
        self.text = self.history[at].clone();
    }

    /// The Down key: the next command of the history, and at the end an empty field.
    pub fn history_next(&mut self) {
        let Some(i) = self.cursor else {
            return;
        };
        if i + 1 < self.history.len() {
            self.cursor = Some(i + 1);
            self.text = self.history[i + 1].clone();
        } else {
            self.cursor = None;
            self.text.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aliases() -> Aliases {
        [
            ("L", Action::ToolLine),
            ("PL", Action::ToolPolyline),
            ("PLANT", Action::ToolPlant),
            ("PLA", Action::ToolPlant),
            ("REC", Action::ToolRect),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_string(), b))
        .collect()
    }

    #[test]
    fn aliases_are_normalized_and_checked() {
        assert_eq!(normalize_alias(" pl "), Some("PL".into()));
        assert_eq!(normalize_alias("p-l_1"), Some("PL1".into()));
        assert_eq!(normalize_alias("---"), None);
        assert_eq!(normalize_alias(""), None);
        assert_eq!(normalize_alias(&"a".repeat(40)).unwrap().len(), MAX_LEN);
        assert!(valid_alias("PL") && valid_alias("A1"));
        assert!(!valid_alias("") && !valid_alias("pl") && !valid_alias("P L"));
        assert!(!valid_alias(&"A".repeat(MAX_LEN + 1)));
    }

    #[test]
    fn typing_builds_an_upper_case_text() {
        let mut c = CommandLine::new();
        assert!(c.is_empty());
        c.type_text("p");
        c.type_text("l!");
        assert_eq!(c.text(), "PL");
        c.backspace();
        assert_eq!(c.text(), "P");
        c.set_text("rec");
        assert_eq!(c.text(), "REC");
        c.clear();
        assert!(c.is_empty());
        c.backspace();
        assert!(c.is_empty());
    }

    #[test]
    fn suggestions_list_aliases_that_start_with_the_text() {
        let a = aliases();
        let mut c = CommandLine::new();
        assert!(c.suggestions(&a, 5).is_empty());
        c.type_text("p");
        let s = c.suggestions(&a, 5);
        let names: Vec<&str> = s.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(names, ["PL", "PLA", "PLANT"]);
        assert_eq!(c.suggestions(&a, 2).len(), 2);
        c.type_text("x");
        assert!(c.suggestions(&a, 5).is_empty());
    }

    #[test]
    fn enter_runs_an_alias_and_remembers_it() {
        let a = aliases();
        let mut c = CommandLine::new();
        assert_eq!(c.enter(&a), Outcome::Nothing);
        c.type_text("pl");
        assert_eq!(c.enter(&a), Outcome::Run(Action::ToolPolyline));
        assert!(c.is_empty());
        // An empty field repeats the last action.
        assert_eq!(c.enter(&a), Outcome::Repeat(Action::ToolPolyline));
        // An unknown text stays.
        c.type_text("zz");
        assert_eq!(c.enter(&a), Outcome::Unknown("ZZ".into()));
        assert_eq!(c.text(), "ZZ");
        assert_eq!(c.enter(&a), Outcome::Unknown("ZZ".into()));
    }

    #[test]
    fn the_history_steps_back_and_forth() {
        let a = aliases();
        let mut c = CommandLine::new();
        c.history_previous();
        c.history_next();
        assert!(c.is_empty());
        for t in ["l", "rec", "rec", "pl"] {
            c.type_text(t);
            c.enter(&a);
        }
        // The same alias twice in a row is one entry.
        c.history_previous();
        assert_eq!(c.text(), "PL");
        c.history_previous();
        assert_eq!(c.text(), "REC");
        c.history_previous();
        assert_eq!(c.text(), "L");
        c.history_previous();
        assert_eq!(c.text(), "L");
        c.history_next();
        assert_eq!(c.text(), "REC");
        c.history_next();
        c.history_next();
        assert!(c.is_empty());
        c.history_next();
        assert!(c.is_empty());
    }

    #[test]
    fn the_history_is_limited() {
        let a: Aliases = (0..HISTORY + 10)
            .map(|i| (format!("A{i}"), Action::ToolLine))
            .collect();
        let mut c = CommandLine::new();
        for i in 0..HISTORY + 10 {
            c.type_text(&format!("a{i}"));
            c.enter(&a);
        }
        for _ in 0..HISTORY + 5 {
            c.history_previous();
        }
        assert_eq!(c.text(), "A10");
    }
}
