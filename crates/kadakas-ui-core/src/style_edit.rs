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

use kadakas_core::annot::dim;
use kadakas_core::annot::style::{
    AnnotResource, DimensionStyle, Justify, LengthUnit, MAX_NOTES, MAX_TAG_LINES, Note, NotesDb,
    TagFrame, TagStyle, Terminator, TextStyle, Tolerance,
};
use kadakas_core::command::Command;
use kadakas_core::model::Document;
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_units::AreaUnit;

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

/// The area units of a dimension standard, with their short names.
pub fn area_units() -> [(AreaUnit, &'static str); 4] {
    AreaUnit::ALL.map(|u| (u, u.short_name()))
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

/// Sample texts of a dimension standard: a length, an area and an angle, as the style writes them.
/// The window shows them so the person sees the effect of a change at once.
pub fn dimension_samples(s: &DimensionStyle) -> [String; 3] {
    [
        dim::format_length(12_345.6, s),
        dim::format_area(87.65e6, s),
        dim::format_angle(37.25, s),
    ]
}

/// What a call of [`StyleEditor::sync`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Follow {
    /// Nothing to do.
    Same,
    /// The resource changed in the document and the draft was clean, so the draft was reloaded.
    Reloaded,
    /// The resource changed in the document but the draft has edits. The draft stays; saving
    /// writes it over the new content.
    Conflict,
    /// The resource is gone (deleted, or its creation was undone).
    Gone,
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

    /// True when the draft differs from the last saved or loaded content.
    pub fn is_dirty(&self) -> bool {
        self.name != self.saved_name || self.draft != self.saved
    }

    /// Throws the edits away.
    pub fn revert(&mut self) {
        self.name = self.saved_name.clone();
        self.draft = self.saved.clone();
        self.clamp_selection();
    }

    /// Why the draft cannot be saved, or `None`. The checks are those of the command: the name,
    /// and the values of the style.
    pub fn problem(&self, doc: &Document) -> Option<String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Some("a style needs a name".into());
        }
        let type_name = self.kind().type_name();
        if doc.resources.iter().any(|r| {
            r.id != self.id && r.body.type_name() == type_name && r.name.eq_ignore_ascii_case(name)
        }) {
            return Some(format!("a {type_name} named '{name}' exists"));
        }
        self.draft.validate().err()
    }

    /// The command that saves the draft: one `ModifyResource`, one undo step. `Ok(None)` when
    /// nothing changed; `Err` with the problem when the draft is not valid.
    pub fn save_command(&self, doc: &Document) -> Result<Option<Command>, String> {
        if let Some(p) = self.problem(doc) {
            return Err(p);
        }
        if !self.is_dirty() {
            return Ok(None);
        }
        Ok(Some(Command::ModifyResource {
            id: self.id,
            name: (self.name != self.saved_name).then(|| self.name.trim().to_string()),
            body: (self.draft != self.saved).then(|| ResourceBody::annot(self.draft.clone())),
        }))
    }

    /// Call after the save command worked: the draft is now the saved content.
    pub fn mark_saved(&mut self) {
        self.name = self.name.trim().to_string();
        self.saved_name = self.name.clone();
        self.saved = self.draft.clone();
    }

    /// Follows the document. See [`Follow`].
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
        let dirty = self.is_dirty();
        self.saved_name = r.name.clone();
        self.saved = (**a).clone();
        if dirty {
            Follow::Conflict
        } else {
            self.name = self.saved_name.clone();
            self.draft = self.saved.clone();
            self.clamp_selection();
            Follow::Reloaded
        }
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
