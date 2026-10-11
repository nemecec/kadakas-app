//! The form logic of the style editors (ANN-12): dimension standards, text styles, tag styles and
//! notes databases. A UI program opens a [`StyleEditor`] on a resource, lets the person change the
//! draft, and saves it with one `ModifyResource` command, which is one undo step. The crate has no
//! egui: the program only draws the fields and calls the methods here.
//!
//! - The draft is a copy. Nothing in the document changes until [`StyleEditor::save_command`] runs.
//! - [`StyleEditor::sync`] follows the document: an undo or another user can change the resource
//!   while the window is open.
//! - The lists (the lines of a tag style, the notes of a notes database) have add, remove and move
//!   methods that keep the selection inside the list.

use kadakas_core::annot::style::{
    AnnotResource, DimensionStyle, Justify, LengthUnit, MAX_NOTES, MAX_TAG_LINES, Note, NotesDb,
    TagFrame, TagStyle, Terminator, TextStyle, Tolerance,
};
use kadakas_core::annot::{Annot, dim};
use kadakas_core::command::Command;
use kadakas_core::model::{Document, Shape};
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_l10n::tr;

/// The kinds of style that have an editor. The `type_name` is the type name of the resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleKind {
    Dimension,
    Text,
    Tag,
    Notes,
}

impl StyleKind {
    pub const ALL: [StyleKind; 4] = [
        StyleKind::Dimension,
        StyleKind::Text,
        StyleKind::Tag,
        StyleKind::Notes,
    ];

    /// The resource type name (`dimension_style`, ...).
    pub fn type_name(self) -> &'static str {
        match self {
            StyleKind::Dimension => "dimension_style",
            StyleKind::Text => "text_style",
            StyleKind::Tag => "tag_style",
            StyleKind::Notes => "notes_database",
        }
    }

    pub fn from_type_name(name: &str) -> Option<StyleKind> {
        StyleKind::ALL.into_iter().find(|k| k.type_name() == name)
    }

    /// The name of the kind in the language of the UI ("Dimension standard").
    pub fn label(self) -> String {
        match self {
            StyleKind::Dimension => tr!("uic-style-edit-kind-dimension"),
            StyleKind::Text => tr!("uic-style-edit-kind-text"),
            StyleKind::Tag => tr!("uic-style-edit-kind-tag"),
            StyleKind::Notes => tr!("uic-style-edit-kind-notes"),
        }
    }

    /// The name that a new style of this kind gets, English because it is document content.
    pub fn default_name(self) -> &'static str {
        match self {
            StyleKind::Dimension => "Dimension standard",
            StyleKind::Text => "Text style",
            StyleKind::Tag => "Tag style",
            StyleKind::Notes => "Notes",
        }
    }

    /// The content of a new style of this kind.
    pub fn default_body(self) -> AnnotResource {
        match self {
            StyleKind::Dimension => AnnotResource::DimensionStyle(DimensionStyle::default()),
            StyleKind::Text => AnnotResource::TextStyle(TextStyle::default()),
            StyleKind::Tag => AnnotResource::TagStyle(TagStyle::default()),
            StyleKind::Notes => AnnotResource::NotesDatabase(NotesDb::default()),
        }
    }
}

/// The kind of an annotation resource.
pub fn kind_of(r: &AnnotResource) -> StyleKind {
    match r {
        AnnotResource::TextStyle(_) => StyleKind::Text,
        AnnotResource::DimensionStyle(_) => StyleKind::Dimension,
        AnnotResource::TagStyle(_) => StyleKind::Tag,
        AnnotResource::NotesDatabase(_) => StyleKind::Notes,
    }
}

/// A choice for a list in an editor: the value, its id (the API name) and the domain of the
/// message that names it (`enum-<domain>-<id>`).
pub struct Choice<T: 'static> {
    pub value: T,
    pub id: &'static str,
}

pub const TERMINATOR_DOMAIN: &str = "terminator";
pub const JUSTIFY_DOMAIN: &str = "justify";
pub const FRAME_DOMAIN: &str = "frame";

pub static TERMINATORS: [Choice<Terminator>; 5] = [
    Choice {
        value: Terminator::Arrow,
        id: "arrow",
    },
    Choice {
        value: Terminator::OpenArrow,
        id: "open_arrow",
    },
    Choice {
        value: Terminator::Tick,
        id: "tick",
    },
    Choice {
        value: Terminator::Dot,
        id: "dot",
    },
    Choice {
        value: Terminator::None,
        id: "none",
    },
];

pub static JUSTIFICATIONS: [Choice<Justify>; 3] = [
    Choice {
        value: Justify::Left,
        id: "left",
    },
    Choice {
        value: Justify::Center,
        id: "center",
    },
    Choice {
        value: Justify::Right,
        id: "right",
    },
];

pub static FRAMES: [Choice<TagFrame>; 4] = [
    Choice {
        value: TagFrame::None,
        id: "none",
    },
    Choice {
        value: TagFrame::Box,
        id: "box",
    },
    Choice {
        value: TagFrame::Underline,
        id: "underline",
    },
    Choice {
        value: TagFrame::Circle,
        id: "circle",
    },
];

/// The id of a value in a choice list (the first one if the list lacks the value).
pub fn id_of<T: PartialEq + 'static>(list: &'static [Choice<T>], value: &T) -> &'static str {
    list.iter()
        .find(|c| c.value == *value)
        .map_or(list[0].id, |c| c.id)
}

/// The length units of a dimension standard, with their short names.
pub fn length_units() -> [(LengthUnit, &'static str); 6] {
    LengthUnit::ALL.map(|u| (u, u.short_name()))
}

/// Turns an optional number on (with `default`) or off. Used for the tolerance, the rounding and
/// the color of a style: a checkbox in the window.
pub fn set_enabled<T>(value: &mut Option<T>, on: bool, default: T) {
    match (on, value.is_some()) {
        (true, false) => *value = Some(default),
        (false, true) => *value = None,
        _ => {}
    }
}

/// The default tolerance when the person turns it on.
pub fn default_tolerance() -> Tolerance {
    Tolerance {
        plus: 1.0,
        minus: 1.0,
    }
}

/// Sample texts of a dimension standard: a length and an angle, as the style writes them.
/// The window shows them so the person sees the effect of a change at once.
pub fn dimension_samples(s: &DimensionStyle) -> [String; 2] {
    [dim::format_length(12_345.6, s), dim::format_angle(37.25, s)]
}

/// What a call of [`StyleEditor::sync`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Follow {
    /// Nothing to do.
    Same,
    /// The resource changed in the document and the draft was clean, so the draft was reloaded.
    Reloaded,
    /// The resource changed in the document and the draft has edits. The changes were merged
    /// field by field: the edits of the person stay, the other changes came in. No field changed
    /// on both sides.
    Merged,
    /// As `Merged`, but one field or the name changed on both sides. The edit of the person won.
    /// The window says so.
    Conflict,
    /// The resource is gone (deleted, or its creation was undone).
    Gone,
}

/// Why a draft cannot be saved. The window shows `text()` in the language of the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The name is empty.
    NameMissing,
    /// Another style of the same kind has the name.
    NameTaken { kind: StyleKind, name: String },
    /// A value is not valid. The text is the English text of the check in the core.
    Invalid(String),
}

impl Problem {
    /// The problem as a sentence in the language of the UI.
    pub fn text(&self) -> String {
        match self {
            Problem::NameMissing => tr!("uic-style-edit-name-missing"),
            Problem::NameTaken { kind, name } => {
                tr!(
                    "uic-style-edit-name-taken",
                    kind = kind.label(),
                    name = name.clone()
                )
            }
            Problem::Invalid(m) => tr!("uic-style-edit-invalid", message = m.clone()),
        }
    }
}

/// A note key that the person changed or removed while callouts use it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteWarning {
    pub key: String,
    pub callouts: usize,
}

/// An editor window's model for one style.
#[derive(Clone, Debug)]
pub struct StyleEditor {
    id: ResourceId,
    /// The name of the style. Edit it freely; `save_command` checks it.
    pub name: String,
    /// The content. Edit it freely; `save_command` checks it.
    pub draft: AnnotResource,
    saved_name: String,
    saved: AnnotResource,
    /// The row that is selected in a list (tag lines, notes).
    pub selected: usize,
}

impl StyleEditor {
    /// An editor on the resource, or `None` if it does not exist or is not an annotation style.
    pub fn open(doc: &Document, id: ResourceId) -> Option<StyleEditor> {
        let r = doc.resources.get(id)?;
        let ResourceBody::Annot(a) = &r.body else {
            return None;
        };
        Some(StyleEditor {
            id,
            name: r.name.clone(),
            draft: (**a).clone(),
            saved_name: r.name.clone(),
            saved: (**a).clone(),
            selected: 0,
        })
    }

    pub fn id(&self) -> ResourceId {
        self.id
    }

    pub fn kind(&self) -> StyleKind {
        kind_of(&self.draft)
    }

    /// True when the draft differs from the last saved or loaded content. The name counts
    /// without the spaces at its ends, as the command keeps it.
    pub fn is_dirty(&self) -> bool {
        self.name.trim() != self.saved_name || self.draft != self.saved
    }

    /// Throws the edits away.
    pub fn revert(&mut self) {
        self.name = self.saved_name.clone();
        self.draft = self.saved.clone();
        self.clamp_selection();
    }

    /// Why the draft cannot be saved, or `None`. The checks are those of the command: the name,
    /// and the values of the style.
    pub fn problem(&self, doc: &Document) -> Option<Problem> {
        let name = self.name.trim();
        if name.is_empty() {
            return Some(Problem::NameMissing);
        }
        let kind = self.kind();
        if doc.resources.iter().any(|r| {
            r.id != self.id
                && r.body.type_name() == kind.type_name()
                && r.name.eq_ignore_ascii_case(name)
        }) {
            return Some(Problem::NameTaken {
                kind,
                name: name.to_string(),
            });
        }
        self.draft.validate().err().map(Problem::Invalid)
    }

    /// The command that saves the draft: one `ModifyResource`, one undo step. `Ok(None)` when
    /// nothing changed; `Err` with the problem when the draft is not valid.
    pub fn save_command(&self, doc: &Document) -> Result<Option<Command>, Problem> {
        if let Some(p) = self.problem(doc) {
            return Err(p);
        }
        if !self.is_dirty() {
            return Ok(None);
        }
        Ok(Some(Command::ModifyResource {
            id: self.id,
            name: (self.name.trim() != self.saved_name).then(|| self.name.trim().to_string()),
            body: (self.draft != self.saved).then(|| ResourceBody::annot(self.draft.clone())),
        }))
    }

    /// Call after the save command worked: the draft is now the saved content.
    pub fn mark_saved(&mut self) {
        self.name = self.name.trim().to_string();
        self.saved_name = self.name.clone();
        self.saved = self.draft.clone();
    }

    /// Follows the document. See [`Follow`]. When the person has edits and the resource changed
    /// elsewhere, this is a three-way merge with the last loaded content as the base: a field the
    /// person did not change takes the new value, a field the person changed stays.
    pub fn sync(&mut self, doc: &Document) -> Follow {
        let Some(r) = doc.resources.get(self.id) else {
            return Follow::Gone;
        };
        let ResourceBody::Annot(a) = &r.body else {
            return Follow::Gone;
        };
        if r.name == self.saved_name && **a == self.saved {
            return Follow::Same;
        }
        let theirs_name = r.name.clone();
        let theirs = (**a).clone();
        if !self.is_dirty() {
            self.saved_name = theirs_name;
            self.saved = theirs;
            self.name = self.saved_name.clone();
            self.draft = self.saved.clone();
            self.clamp_selection();
            return Follow::Reloaded;
        }
        let (name, name_conflict) =
            merge3(&self.saved_name, &theirs_name, self.name.trim().to_string());
        let (body, body_conflict) = merge_body(&self.saved, &theirs, &self.draft);
        self.saved_name = theirs_name;
        self.saved = theirs;
        self.name = name;
        self.draft = body;
        self.clamp_selection();
        if name_conflict || body_conflict {
            Follow::Conflict
        } else {
            Follow::Merged
        }
    }

    /// The note keys that the draft of a notes database changed or removed while callouts of the
    /// document use them. A callout shows no text for a key that is gone.
    pub fn note_warnings(&self, doc: &Document) -> Vec<NoteWarning> {
        let (AnnotResource::NotesDatabase(draft), AnnotResource::NotesDatabase(saved)) =
            (&self.draft, &self.saved)
        else {
            return Vec::new();
        };
        let norm = |k: &str| k.trim().to_lowercase();
        let keep: std::collections::HashSet<String> =
            draft.notes.iter().map(|n| norm(&n.key)).collect();
        saved
            .notes
            .iter()
            .filter(|n| !keep.contains(&norm(&n.key)))
            .filter_map(|n| {
                let callouts = doc
                    .objects
                    .values()
                    .filter(|o| match &o.shape {
                        Shape::Annotation(a) => matches!(
                            &**a,
                            Annot::Callout(c) if c.notes_db == Some(self.id)
                                && c.note.as_deref().is_some_and(|k| norm(k) == norm(&n.key))
                        ),
                        _ => false,
                    })
                    .count();
                (callouts > 0).then(|| NoteWarning {
                    key: n.key.clone(),
                    callouts,
                })
            })
            .collect()
    }

    fn clamp_selection(&mut self) {
        let n = self.rows();
        self.selected = self.selected.min(n.saturating_sub(1));
    }

    // ---------------------------------------------------------------- lists

    /// The number of rows in the list of the style: tag lines or notes. Other kinds have none.
    pub fn rows(&self) -> usize {
        match &self.draft {
            AnnotResource::TagStyle(s) => s.lines.len(),
            AnnotResource::NotesDatabase(n) => n.notes.len(),
            _ => 0,
        }
    }

    /// Adds a row after the selected one and selects it. A tag style gets an empty line; a notes
    /// database gets a note with a free key (`note-1`, `note-2`, ...). False at the limit.
    pub fn add_row(&mut self) -> bool {
        let at = if self.rows() == 0 {
            0
        } else {
            self.selected + 1
        };
        match &mut self.draft {
            AnnotResource::TagStyle(s) => {
                if s.lines.len() >= MAX_TAG_LINES {
                    return false;
                }
                s.lines.insert(at, String::new());
            }
            AnnotResource::NotesDatabase(n) => {
                if n.notes.len() >= MAX_NOTES {
                    return false;
                }
                let key = (1..)
                    .map(|i| format!("note-{i}"))
                    .find(|k| n.get(k).is_none())
                    .expect("a free key exists");
                n.notes.insert(
                    at,
                    Note {
                        key,
                        text: String::new(),
                    },
                );
            }
            _ => return false,
        }
        self.selected = at;
        true
    }

    /// Removes row `i`. A tag style keeps at least one line (an empty one replaces the last).
    pub fn remove_row(&mut self, i: usize) -> bool {
        match &mut self.draft {
            AnnotResource::TagStyle(s) if i < s.lines.len() => {
                s.lines.remove(i);
                if s.lines.is_empty() {
                    s.lines.push(String::new());
                }
            }
            AnnotResource::NotesDatabase(n) if i < n.notes.len() => {
                n.notes.remove(i);
            }
            _ => return false,
        }
        self.clamp_selection();
        true
    }

    /// Moves row `i` up (`-1`) or down (`1`) and keeps it selected. False at the ends.
    pub fn move_row(&mut self, i: usize, delta: i32) -> bool {
        let n = self.rows();
        let to = i as i64 + i64::from(delta);
        if i >= n || to < 0 || to >= n as i64 || delta == 0 {
            return false;
        }
        let to = to as usize;
        match &mut self.draft {
            AnnotResource::TagStyle(s) => s.lines.swap(i, to),
            AnnotResource::NotesDatabase(d) => d.notes.swap(i, to),
            _ => return false,
        }
        self.selected = to;
        true
    }

    /// Typed access for the window.
    pub fn dimension(&mut self) -> Option<&mut DimensionStyle> {
        match &mut self.draft {
            AnnotResource::DimensionStyle(s) => Some(s),
            _ => None,
        }
    }

    pub fn text(&mut self) -> Option<&mut TextStyle> {
        match &mut self.draft {
            AnnotResource::TextStyle(s) => Some(s),
            _ => None,
        }
    }

    pub fn tag(&mut self) -> Option<&mut TagStyle> {
        match &mut self.draft {
            AnnotResource::TagStyle(s) => Some(s),
            _ => None,
        }
    }

    pub fn notes(&mut self) -> Option<&mut NotesDb> {
        match &mut self.draft {
            AnnotResource::NotesDatabase(s) => Some(s),
            _ => None,
        }
    }
}

/// Merges one value of three: `base` is what both sides started from. The result and a flag that
/// says both sides changed it to different values (then `mine` wins).
fn merge3<T: PartialEq + Clone>(base: &T, theirs: &T, mine: T) -> (T, bool) {
    if mine == *base {
        (theirs.clone(), false)
    } else if theirs == base || mine == *theirs {
        (mine, false)
    } else {
        (mine, true)
    }
}

/// A three-way merge of the bodies, field by field. A field that the person did not change takes
/// the other side's value. The flag says one field changed on both sides to different values.
fn merge_body(
    base: &AnnotResource,
    theirs: &AnnotResource,
    mine: &AnnotResource,
) -> (AnnotResource, bool) {
    use serde_json::Value;
    let json = |r: &AnnotResource| serde_json::to_value(r).ok();
    let (Some(Value::Object(b)), Some(Value::Object(t)), Some(Value::Object(m))) =
        (json(base), json(theirs), json(mine))
    else {
        return (mine.clone(), true);
    };
    let mut keys: Vec<&String> = b.keys().chain(t.keys()).chain(m.keys()).collect();
    keys.sort();
    keys.dedup();
    let mut out = serde_json::Map::new();
    let mut conflict = false;
    for k in keys {
        let none = Value::Null;
        let (bv, tv, mv) = (
            b.get(k).unwrap_or(&none),
            t.get(k).unwrap_or(&none),
            m.get(k).unwrap_or(&none),
        );
        let (v, c) = merge3(bv, tv, mv.clone());
        conflict |= c;
        out.insert(k.clone(), v);
    }
    match serde_json::from_value::<AnnotResource>(Value::Object(out)) {
        Ok(r) => (r, conflict),
        Err(_) => (mine.clone(), true),
    }
}

/// The first style of a kind in the document (the dimension standard that the document uses).
pub fn first_of(doc: &Document, kind: StyleKind) -> Option<ResourceId> {
    doc.resources
        .iter()
        .find(|r| r.body.type_name() == kind.type_name())
        .map(|r| r.id)
}

/// The commands of the "Add default styles" button: a text style, a dimension standard and a data
/// tag style when the document has none of that type, and the built-in "Area" tag style when no
/// tag style has that name. Empty when nothing is missing.
pub fn missing_default_styles(doc: &Document) -> Vec<Command> {
    use kadakas_core::annot::style::AREA_TAG_NAME;
    let has = |ty: &str| doc.resources.iter().any(|r| r.body.type_name() == ty);
    let add = |name: &str, r: AnnotResource| Command::AddResource {
        name: name.into(),
        body: ResourceBody::annot(r),
    };
    let mut cmds = Vec::new();
    if !has("text_style") {
        cmds.push(add(
            "Text 3 mm",
            AnnotResource::TextStyle(TextStyle::default()),
        ));
    }
    if !has("dimension_style") {
        cmds.push(add(
            "Dimension 2.5 mm",
            AnnotResource::DimensionStyle(DimensionStyle::default()),
        ));
    }
    if !has("tag_style") {
        cmds.push(add(
            "Plant tag",
            AnnotResource::TagStyle(TagStyle {
                lines: vec!["{IdCode}".into(), "{Botanical}".into()],
                ..TagStyle::default()
            }),
        ));
    }
    if doc
        .resources
        .find_annot("tag_style", AREA_TAG_NAME)
        .is_none()
    {
        cmds.push(add(
            AREA_TAG_NAME,
            AnnotResource::TagStyle(TagStyle::area()),
        ));
    }
    cmds
}

/// The command that creates a style of this kind. `name` is made unique among the styles of the
/// type (`Text style`, `Text style 2`, ...).
pub fn create_command(doc: &Document, kind: StyleKind) -> Command {
    let base = kind.default_name();
    let taken = |n: &str| {
        doc.resources
            .iter()
            .any(|r| r.body.type_name() == kind.type_name() && r.name.eq_ignore_ascii_case(n))
    };
    let name = if taken(base) {
        (2..)
            .map(|i| format!("{base} {i}"))
            .find(|n| !taken(n))
            .expect("a free name exists")
    } else {
        base.to_string()
    };
    Command::AddResource {
        name,
        body: ResourceBody::annot(kind.default_body()),
    }
}

#[cfg(test)]
mod tests;
