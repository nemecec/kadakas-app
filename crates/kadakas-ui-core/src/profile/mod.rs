//! Workspace profiles (PLAT-04).
//!
//! A profile bundles what makes a workspace feel like one CAD program or another: the keymap,
//! command aliases, the tool sets of the Tool bar, the menus, a terminology overlay for the
//! text of the UI, and optional default units. A profile is a JSON file with a version. The
//! format is described in `docs/profiles.md`.
//!
//! This module is pure: it parses and checks text, merges it with the defaults, finds key
//! conflicts and edits profiles. A UI program reads and writes the files and draws the result.
//!
//! - [`layout`]: menus and tool sets.
//! - [`command_line`]: the state of the command alias field.
//! - [`ProfileSet`]: the built-in and the user profiles, with duplicate, rename and import.

pub mod command_line;
pub mod layout;
mod problem;
mod set;

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::keymap::{Action, Chord, Keymap};
pub use command_line::{Aliases, CommandLine, Outcome};
pub use layout::{Cmd, Item, Menu, ToolSet};
pub use problem::{ParseError, Problem};
pub use set::{ProfileSet, RenameError, slug};

/// The version of the file format that this program writes and reads. A file with a higher
/// number is refused; a lower one would be converted (there is none yet).
pub const FORMAT: u32 = 1;

/// The longest name of a profile.
pub const MAX_NAME: usize = 60;
/// A profile file larger than this is refused before it is parsed.
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
/// The most menus, tool sets, entries per menu or set, and aliases that a profile can hold.
/// More are ignored and listed as a problem.
pub const MAX_MENUS: usize = 24;
pub const MAX_TOOL_SETS: usize = 24;
pub const MAX_ITEMS: usize = 200;
pub const MAX_ALIASES: usize = 500;
/// The longest description, in characters. A longer one is cut.
pub const MAX_DESCRIPTION: usize = 500;

/// The id of the built-in Vectorworks-like profile. It is the default.
pub const VECTORWORKS_ID: &str = "vectorworks";
/// The id of the built-in AutoCAD-like profile.
pub const AUTOCAD_ID: &str = "autocad";

/// One workspace profile.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    /// The id is the name of the file without `.json`: lower case letters, digits, `-`, `_`.
    pub id: String,
    pub name: String,
    pub description: String,
    /// The id of the profile that this one was copied from. "Reset" in the Shortcuts editor
    /// gives the keys of that profile.
    pub based_on: Option<String>,
    /// True for the profiles that the program carries. They are read-only.
    pub builtin: bool,
    pub keymap: Keymap,
    pub aliases: Aliases,
    pub tool_sets: Vec<ToolSet>,
    pub menus: Vec<Menu>,
    /// Terminology overlay: a `.ftl` text for each language tag.
    pub terms: BTreeMap<String, String>,
    /// Default units and formats. A UI program applies them when the person picks the profile.
    pub units: Option<kadakas_units::Settings>,
}

/// A parsed profile and the problems that the loader worked around.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub profile: Profile,
    pub problems: Vec<Problem>,
}

const VECTORWORKS_JSON: &str = include_str!("../../profiles/vectorworks.json");
const AUTOCAD_JSON: &str = include_str!("../../profiles/autocad.json");
const AUTOCAD_TERMS: [(&str, &str); 2] = [
    ("en-US", include_str!("../../profiles/autocad.en-US.ftl")),
    ("et", include_str!("../../profiles/autocad.et.ftl")),
];

impl Profile {
    /// The built-in profiles: Vectorworks-like first, then AutoCAD-like.
    pub fn builtins() -> Vec<Profile> {
        let mut out = Vec::new();
        for (id, text) in [
            (VECTORWORKS_ID, VECTORWORKS_JSON),
            (AUTOCAD_ID, AUTOCAD_JSON),
        ] {
            let mut p = Profile::parse(text, id)
                .unwrap_or_else(|e| panic!("built-in profile {id}: {e:?}"))
                .profile;
            p.builtin = true;
            if id == AUTOCAD_ID {
                for (tag, ftl) in AUTOCAD_TERMS {
                    p.terms.insert(tag.to_string(), ftl.to_string());
                }
            }
            out.push(p);
        }
        out
    }

    /// The Vectorworks-like profile: the keys, menus and tool sets that Kadakas had before
    /// profiles.
    pub fn vectorworks() -> Profile {
        Profile::builtins().remove(0)
    }

    /// The name that a person sees. The built-in profiles have a name in each language.
    pub fn display_name(&self) -> String {
        if !self.builtin {
            return self.name.clone();
        }
        match self.id.as_str() {
            VECTORWORKS_ID => kadakas_l10n::tr!("uic-profile-name-vectorworks"),
            AUTOCAD_ID => kadakas_l10n::tr!("uic-profile-name-autocad"),
            _ => self.name.clone(),
        }
    }

    /// The description that a person sees.
    pub fn display_description(&self) -> String {
        if !self.builtin {
            return self.description.clone();
        }
        match self.id.as_str() {
            VECTORWORKS_ID => kadakas_l10n::tr!("uic-profile-about-vectorworks"),
            AUTOCAD_ID => kadakas_l10n::tr!("uic-profile-about-autocad"),
            _ => self.description.clone(),
        }
    }

    /// Reads a profile file. Missing parts take the defaults of the Vectorworks-like profile
    /// (keys, tool sets, menus). A part that is wrong is replaced by its default and listed as a
    /// problem. Only a file that cannot be a profile at all gives an error. `fallback_id` is the
    /// id when the file has none, for example the file name.
    pub fn parse(text: &str, fallback_id: &str) -> Result<Parsed, ParseError> {
        if text.len() > MAX_FILE_BYTES {
            return Err(ParseError::TooLarge);
        }
        let value: Value =
            serde_json::from_str(text).map_err(|e| ParseError::NotJson(e.to_string()))?;
        let obj = value.as_object().ok_or(ParseError::NotAnObject)?;
        let format = obj
            .get("format")
            .and_then(Value::as_u64)
            .ok_or(ParseError::NoFormat)?;
        if format == 0 || format > u64::from(FORMAT) {
            return Err(ParseError::BadFormat(format));
        }
        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .ok_or(ParseError::NoName)?;
        if name.chars().count() > MAX_NAME {
            return Err(ParseError::NameTooLong);
        }
        let mut problems = Vec::new();
        let id = match obj.get("id").and_then(Value::as_str) {
            Some(id) if layout::valid_id(id) => id.to_string(),
            Some(id) => {
                problems.push(Problem::BadId(id.to_string()));
                fallback_id.to_string()
            }
            None => fallback_id.to_string(),
        };
        let mut description = obj
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if description.chars().count() > MAX_DESCRIPTION {
            description = description.chars().take(MAX_DESCRIPTION).collect();
            problems.push(Problem::TooLong("description".into()));
        }
        let based_on = obj
            .get("based_on")
            .and_then(Value::as_str)
            .filter(|b| layout::valid_id(b))
            .map(str::to_string);
        let mut p = Profile {
            id,
            name: name.to_string(),
            description,
            based_on,
            builtin: false,
            keymap: Keymap::defaults(),
            aliases: Aliases::new(),
            tool_sets: layout::default_tool_sets(),
            menus: layout::default_menus(),
            terms: BTreeMap::new(),
            units: None,
        };
        if let Some(v) = section(obj, "keymap") {
            p.read_keymap(v, &mut problems);
        }
        if let Some(v) = section(obj, "aliases") {
            p.read_aliases(v, &mut problems);
        }
        if let Some(v) = section(obj, "tool_sets")
            && let Some(list) = read_tool_sets(v, &mut problems)
        {
            p.tool_sets = list;
        }
        if let Some(v) = section(obj, "menus")
            && let Some(list) = read_menus(v, &mut problems)
        {
            p.menus = list;
        }
        if let Some(v) = section(obj, "terms") {
            p.read_terms(v, &mut problems);
        }
        if let Some(v) = section(obj, "units") {
            match serde_json::from_value::<kadakas_units::Settings>(v.clone()) {
                Ok(u) => p.units = Some(u),
                Err(e) => problems.push(Problem::BadUnits(e.to_string())),
            }
        }
        Ok(Parsed {
            profile: p,
            problems,
        })
    }

    /// The keymap section: an object of action ids and chords, as in `keymap.json`. Actions
    /// that the section names lose their default keys first, so a chord that the section gives
    /// is never taken by an old default of a named action. A chord that the section gives to
    /// two actions goes to the later one in the order of the ids.
    fn read_keymap(&mut self, v: &Value, problems: &mut Vec<Problem>) {
        let Some(obj) = v.as_object() else {
            problems.push(Problem::WrongType("keymap".into()));
            return;
        };
        let mut wanted: Vec<(Action, Vec<Chord>)> = Vec::new();
        for (id, val) in obj {
            let Some(action) = Action::from_id(id) else {
                problems.push(Problem::UnknownAction(id.clone()));
                continue;
            };
            let items: Vec<&Value> = match val {
                Value::Array(a) => a.iter().collect(),
                v => vec![v],
            };
            let mut chords = Vec::new();
            let mut bad = false;
            for item in items {
                match item.as_str().map(Chord::parse) {
                    Some(Ok(c)) => chords.push(c),
                    Some(Err(_)) => {
                        problems.push(Problem::BadChord {
                            action: id.clone(),
                            text: item.as_str().unwrap_or_default().to_string(),
                        });
                        bad = true;
                    }
                    None => {
                        problems.push(Problem::BadChord {
                            action: id.clone(),
                            text: item.to_string(),
                        });
                        bad = true;
                    }
                }
            }
            if !bad {
                wanted.push((action, chords));
            }
        }
        for (action, _) in &wanted {
            self.keymap.set_chords(*action, &[]);
        }
        for (action, chords) in wanted {
            for (chord, from) in self.keymap.set_chords(action, &chords) {
                problems.push(Problem::ChordMoved {
                    chord: chord.to_text(),
                    from: from.id().to_string(),
                    to: action.id().to_string(),
                });
            }
        }
    }

    fn read_aliases(&mut self, v: &Value, problems: &mut Vec<Problem>) {
        let Some(obj) = v.as_object() else {
            problems.push(Problem::WrongType("aliases".into()));
            return;
        };
        if obj.len() > MAX_ALIASES {
            problems.push(Problem::TooMany("aliases".into()));
        }
        for (alias, target) in obj.iter().take(MAX_ALIASES) {
            let upper = alias.to_ascii_uppercase();
            let action = target.as_str().and_then(Action::from_id);
            match action {
                Some(a) if command_line::valid_alias(&upper) => {
                    self.aliases.insert(upper, a);
                }
                Some(_) => problems.push(Problem::BadAlias(alias.clone())),
                None => problems.push(Problem::UnknownAction(
                    target
                        .as_str()
                        .map_or_else(|| target.to_string(), str::to_string),
                )),
            }
        }
    }

    fn read_terms(&mut self, v: &Value, problems: &mut Vec<Problem>) {
        let Some(obj) = v.as_object() else {
            problems.push(Problem::WrongType("terms".into()));
            return;
        };
        for (tag, text) in obj {
            let Some(text) = text.as_str() else {
                problems.push(Problem::WrongType(format!("terms.{tag}")));
                continue;
            };
            let known = kadakas_l10n::languages().iter().any(|l| l.tag == tag);
            if !known {
                problems.push(Problem::TermsLanguage(tag.clone()));
                continue;
            }
            let found = kadakas_l10n::check_overlay(tag, text);
            // The program filters the text when it sets the overlay: a message that may not
            // be replaced, or that has a wrong variable, never shows. Only a text that cannot
            // be read at all is dropped.
            let usable = !found.iter().any(|f| {
                matches!(
                    f,
                    kadakas_l10n::OverlayProblem::Syntax(_)
                        | kadakas_l10n::OverlayProblem::TooLong
                        | kadakas_l10n::OverlayProblem::UnknownLanguage(_)
                )
            });
            problems.extend(found.into_iter().map(|f| Problem::Terms {
                lang: tag.clone(),
                problem: f,
            }));
            if usable {
                self.terms.insert(tag.clone(), text.to_string());
            }
        }
    }

    /// The profile as a file: the whole keymap, so the file does not depend on the defaults of
    /// a later version.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.to_value()).expect("a value serializes")
    }

    pub fn to_value(&self) -> Value {
        let keymap: Map<String, Value> = Action::ALL
            .iter()
            .map(|a| {
                let chords: Vec<Value> = self
                    .keymap
                    .chords(*a)
                    .iter()
                    .map(|c| Value::String(c.to_text()))
                    .collect();
                (a.id().to_string(), Value::Array(chords))
            })
            .collect();
        let aliases: Map<String, Value> = self
            .aliases
            .iter()
            .map(|(k, a)| (k.clone(), json!(a.id())))
            .collect();
        let tool_sets: Vec<Value> = self
            .tool_sets
            .iter()
            .map(|s| {
                let tools: Vec<&str> = s.tools.iter().map(|t| Action::for_tool(*t).id()).collect();
                part(&s.id, &s.name, "tools", tools)
            })
            .collect();
        let menus: Vec<Value> = self
            .menus
            .iter()
            .map(|m| {
                let items: Vec<&str> = m.items.iter().map(|i| i.id()).collect();
                part(&m.id, &m.name, "items", items)
            })
            .collect();
        let mut obj = Map::new();
        obj.insert("format".into(), json!(FORMAT));
        obj.insert("id".into(), json!(self.id));
        obj.insert("name".into(), json!(self.name));
        obj.insert("description".into(), json!(self.description));
        if let Some(b) = &self.based_on {
            obj.insert("based_on".into(), json!(b));
        }
        obj.insert("keymap".into(), Value::Object(keymap));
        obj.insert("aliases".into(), Value::Object(aliases));
        obj.insert("tool_sets".into(), Value::Array(tool_sets));
        obj.insert("menus".into(), Value::Array(menus));
        obj.insert("terms".into(), json!(self.terms));
        if let Some(u) = &self.units {
            obj.insert(
                "units".into(),
                serde_json::to_value(u).expect("units serialize"),
            );
        }
        Value::Object(obj)
    }

    /// The terms as the pairs that `kadakas_l10n::set_overlay` takes.
    pub fn overlay(&self) -> Vec<(&str, &str)> {
        self.terms
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }

    // Editing. A built-in profile is never edited: `ProfileSet::editable` makes a copy first.

    /// Gives `action` these chords. Another action that holds one of them loses it; the
    /// result lists them.
    pub fn set_chords(&mut self, action: Action, chords: &[Chord]) -> Vec<(Chord, Action)> {
        self.keymap.set_chords(action, chords)
    }

    /// The other action that holds `chord`, before a Shortcuts editor assigns it.
    pub fn chord_conflict(&self, action: Action, chord: Chord) -> Option<Action> {
        self.keymap.conflict_for(action, chord)
    }

    /// Back to the keys that `base` gives the action. Other actions that hold those chords lose
    /// them. `ProfileSet::reset_chords` picks `base`: the keymap of the profile this one was
    /// copied from.
    pub fn reset_chords(&mut self, action: Action, base: &Keymap) -> Vec<(Chord, Action)> {
        self.set_chords(action, base.chords(action))
    }

    /// Adds or changes an alias. The text is normalized. Returns the action that the alias
    /// had before, and `None` for a text that is no alias.
    pub fn set_alias(&mut self, alias: &str, action: Action) -> Option<Option<Action>> {
        let key = command_line::normalize_alias(alias)?;
        Some(self.aliases.insert(key, action))
    }

    pub fn remove_alias(&mut self, alias: &str) -> bool {
        self.aliases.remove(alias).is_some()
    }

    /// Moves a tool inside a tool set. False for a wrong set or index.
    pub fn move_tool(&mut self, set: usize, from: usize, to: usize) -> bool {
        self.tool_sets
            .get_mut(set)
            .is_some_and(|s| s.move_tool(from, to))
    }
}

fn part(id: &str, name: &Option<String>, list: &str, items: Vec<&str>) -> Value {
    let mut o = Map::new();
    o.insert("id".into(), json!(id));
    if let Some(n) = name {
        o.insert("name".into(), json!(n));
    }
    o.insert(list.into(), json!(items));
    Value::Object(o)
}

/// A section of the file, or `None` when it is missing, `null`, or not the type the caller reads.
fn section<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    obj.get(key).filter(|v| !v.is_null())
}

fn read_name(o: &Map<String, Value>, id: &str, problems: &mut Vec<Problem>) -> Option<String> {
    match o.get("name").and_then(Value::as_str).map(str::trim) {
        Some(n) if !n.is_empty() && n.chars().count() <= MAX_NAME => Some(n.to_string()),
        Some(_) => {
            problems.push(Problem::BadName(id.to_string()));
            None
        }
        None => None,
    }
}

/// One part of the layout (a tool set or a menu) with its id and name; `list` is the array.
fn read_part<'a>(
    v: &'a Value,
    list: &str,
    seen: &mut Vec<String>,
    problems: &mut Vec<Problem>,
) -> Option<(String, Option<String>, Vec<&'a Value>)> {
    let Some(o) = v.as_object() else {
        problems.push(Problem::WrongType(list.to_string()));
        return None;
    };
    let id = o.get("id").and_then(Value::as_str).unwrap_or_default();
    if !layout::valid_id(id) {
        problems.push(Problem::BadId(id.to_string()));
        return None;
    }
    if seen.iter().any(|s| s == id) {
        problems.push(Problem::DuplicateId(id.to_string()));
        return None;
    }
    seen.push(id.to_string());
    let mut name = read_name(o, id, problems);
    if name.is_none() && !layout::has_known_title(id) {
        problems.push(Problem::NeedsName(id.to_string()));
        name = Some(id.to_string());
    }
    let items = match o.get(list) {
        Some(Value::Array(a)) => {
            if a.len() > MAX_ITEMS {
                problems.push(Problem::TooMany(format!("{id}.{list}")));
            }
            a.iter().take(MAX_ITEMS).collect()
        }
        None | Some(Value::Null) => Vec::new(),
        Some(_) => {
            problems.push(Problem::WrongType(format!("{id}.{list}")));
            Vec::new()
        }
    };
    Some((id.to_string(), name, items))
}

fn read_tool_sets(v: &Value, problems: &mut Vec<Problem>) -> Option<Vec<ToolSet>> {
    let Some(a) = v.as_array() else {
        problems.push(Problem::WrongType("tool_sets".into()));
        return None;
    };
    let mut seen = Vec::new();
    let mut out = Vec::new();
    if a.len() > MAX_TOOL_SETS {
        problems.push(Problem::TooMany("tool_sets".into()));
    }
    for entry in a.iter().take(MAX_TOOL_SETS) {
        let Some((id, name, items)) = read_part(entry, "tools", &mut seen, problems) else {
            continue;
        };
        let mut tools = Vec::new();
        for item in items {
            let text = item.as_str().unwrap_or_default();
            match Action::from_id(text).and_then(Action::tool) {
                Some(t) if tools.contains(&t) => {
                    problems.push(Problem::DuplicateTool {
                        set: id.clone(),
                        tool: text.to_string(),
                    });
                }
                Some(t) => tools.push(t),
                None => problems.push(Problem::UnknownTool {
                    set: id.clone(),
                    tool: text.to_string(),
                }),
            }
        }
        out.push(ToolSet { id, name, tools });
    }
    Some(out)
}

fn read_menus(v: &Value, problems: &mut Vec<Problem>) -> Option<Vec<Menu>> {
    let Some(a) = v.as_array() else {
        problems.push(Problem::WrongType("menus".into()));
        return None;
    };
    let mut seen = Vec::new();
    let mut out = Vec::new();
    if a.len() > MAX_MENUS {
        problems.push(Problem::TooMany("menus".into()));
    }
    for entry in a.iter().take(MAX_MENUS) {
        let Some((id, name, entries)) = read_part(entry, "items", &mut seen, problems) else {
            continue;
        };
        let mut items = Vec::new();
        for e in entries {
            let text = e.as_str().unwrap_or_default();
            match Item::parse(text) {
                Some(i) => items.push(i),
                None => problems.push(Problem::UnknownItem {
                    menu: id.clone(),
                    item: text.to_string(),
                }),
            }
        }
        out.push(Menu { id, name, items });
    }
    Some(out)
}

#[cfg(test)]
mod tests;
