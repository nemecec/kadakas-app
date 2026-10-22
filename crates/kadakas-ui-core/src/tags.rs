//! The logic of the Tags panel of the 3D modeler (MOD-05): tags are classes. The panel lists the
//! classes with an eye (visible or hidden), a color swatch, and the active tag (new geometry
//! goes to it). "Untagged" is the default class. Below the tags it lists the layers with an eye.
//! This module has no egui: it reads the document and gives back rows and commands. The window
//! draws the rows and runs the commands.
//!
//! Hidden tags hide their objects in the 3D view, in picking, in the inference and in exports,
//! because the scene builders already leave out objects of a class that is not visible
//! (`Document::is_visible`). Nothing here repeats that rule.

use kadakas_core::command::Command;
use kadakas_core::model::{Attributes, ClassId, Document, LayerId, LayerKind, NONE_CLASS, ObjectId};

/// One row of the list of tags.
#[derive(Clone, Debug, PartialEq)]
pub struct TagRow {
    pub id: ClassId,
    /// The name; for Untagged the window shows its own word (see `untagged`).
    pub name: String,
    /// Untagged: the default class. It cannot be renamed or deleted.
    pub untagged: bool,
    pub visible: bool,
    /// New geometry goes to the active tag.
    pub active: bool,
    /// The color of the tag, if it has one: the pen color, else the fill color.
    pub color: Option<[u8; 3]>,
    /// How many objects have the tag.
    pub objects: usize,
}

/// One row of the list of layers.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerRow {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub active: bool,
    pub locked: bool,
}

/// The rows of the panel: Untagged first, then the other tags in the order of the document.
#[derive(Clone, Debug, PartialEq)]
pub struct Rows {
    pub tags: Vec<TagRow>,
    pub layers: Vec<LayerRow>,
}

/// The color of a set of attributes for a swatch: the pen if it shows, else the fill.
pub fn swatch(attrs: &Attributes) -> Option<[u8; 3]> {
    [attrs.pen, attrs.fill]
        .into_iter()
        .flatten()
        .find(|c| c.0[3] > 0)
        .map(|c| [c.0[0], c.0[1], c.0[2]])
}

/// The rows for the document. The default class is Untagged and has no swatch.
pub fn rows(doc: &Document) -> Rows {
    let mut counts = std::collections::BTreeMap::<ClassId, usize>::new();
    for o in doc.objects.values() {
        *counts.entry(o.class).or_default() += 1;
    }
    let mut tags: Vec<TagRow> = doc
        .classes
        .iter()
        .map(|c| TagRow {
            id: c.id,
            name: c.name.clone(),
            untagged: c.id == NONE_CLASS,
            visible: c.visible,
            active: doc.active_class == c.id,
            color: (c.id != NONE_CLASS).then(|| swatch(&c.attrs)).flatten(),
            objects: counts.get(&c.id).copied().unwrap_or(0),
        })
        .collect();
    // Untagged first; the rest keep the order of the document.
    tags.sort_by_key(|t| !t.untagged);
    let layers = doc
        .layers
        .iter()
        .filter(|l| l.kind == LayerKind::Design)
        .map(|l| LayerRow {
            id: l.id,
            name: l.name.clone(),
            visible: l.visible,
            active: doc.active_layer == l.id,
            locked: l.locked,
        })
        .collect();
    Rows { tags, layers }
}

/// The eye of a tag: hides it when it shows, shows it when it is hidden.
pub fn toggle_tag(doc: &Document, id: ClassId) -> Option<Command> {
    let c = doc.class(id)?;
    Some(Command::ModifyClass {
        id,
        name: None,
        visible: Some(!c.visible),
        attrs: None,
    })
}

/// The eye of a layer.
pub fn toggle_layer(doc: &Document, id: LayerId) -> Option<Command> {
    let l = doc.layer(id)?;
    Some(Command::ModifyLayer {
        id,
        name: None,
        visible: Some(!l.visible),
        locked: None,
        elevation: None,
        scale: None,
    })
}

/// Shows every hidden tag and layer, in one undo step. `None` when all show.
pub fn show_all(doc: &Document) -> Option<Command> {
    let mut commands: Vec<Command> = doc
        .classes
        .iter()
        .filter(|c| !c.visible)
        .filter_map(|c| toggle_tag(doc, c.id))
        .collect();
    commands.extend(
        doc.layers
            .iter()
            .filter(|l| !l.visible && l.kind == LayerKind::Design)
            .filter_map(|l| toggle_layer(doc, l.id)),
    );
    (!commands.is_empty()).then_some(Command::Batch { commands })
}

/// The tag that new geometry goes to.
pub fn set_active_tag(id: ClassId) -> Command {
    Command::SetActive {
        layer: None,
        class: Some(id),
    }
}

/// The layer that new geometry goes to.
pub fn set_active_layer(id: LayerId) -> Command {
    Command::SetActive {
        layer: Some(id),
        class: None,
    }
}

/// Why a name was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameProblem {
    Empty,
    /// Another tag has the name (the comparison ignores case).
    Taken,
    /// Untagged is the default class: it keeps its name.
    Untagged,
    NoSuchTag,
}

fn check_name(doc: &Document, name: &str, except: Option<ClassId>) -> Result<String, NameProblem> {
    let name = name.trim();
    if name.is_empty() {
        return Err(NameProblem::Empty);
    }
    let taken = doc
        .classes
        .iter()
        .any(|c| Some(c.id) != except && c.name.trim().eq_ignore_ascii_case(name));
    if taken {
        return Err(NameProblem::Taken);
    }
    Ok(name.to_string())
}

/// A name for a new tag that no tag has: `Tag 1`, `Tag 2` and so on. The name goes into the file,
/// so it is the same in every language (like `Layer 3`).
pub fn new_tag_name(doc: &Document) -> String {
    (1..)
        .map(|n| format!("Tag {n}"))
        .find(|name| check_name(doc, name, None).is_ok())
        .expect("an endless range")
}

/// The command that makes a new tag with the name.
pub fn new_tag(doc: &Document, name: &str) -> Result<Command, NameProblem> {
    Ok(Command::AddClass {
        name: check_name(doc, name, None)?,
        attrs: Attributes::default(),
    })
}

/// The command that renames a tag.
pub fn rename_tag(doc: &Document, id: ClassId, name: &str) -> Result<Command, NameProblem> {
    if id == NONE_CLASS {
        return Err(NameProblem::Untagged);
    }
    doc.class(id).ok_or(NameProblem::NoSuchTag)?;
    Ok(Command::ModifyClass {
        id,
        name: Some(check_name(doc, name, Some(id))?),
        visible: None,
        attrs: None,
    })
}

/// What deleting a tag does.
#[derive(Clone, Debug, PartialEq)]
pub struct DeletePlan {
    /// How many objects have the tag. They go to Untagged. When this is more than zero the window
    /// asks first: nothing is moved without the answer.
    pub objects: usize,
    pub command: Command,
}

/// The plan to delete a tag. Untagged cannot be deleted.
pub fn delete_tag(doc: &Document, id: ClassId) -> Result<DeletePlan, NameProblem> {
    if id == NONE_CLASS {
        return Err(NameProblem::Untagged);
    }
    doc.class(id).ok_or(NameProblem::NoSuchTag)?;
    Ok(DeletePlan {
        objects: doc.objects.values().filter(|o| o.class == id).count(),
        command: Command::DeleteClass {
            id,
            move_to: Some(NONE_CLASS),
        },
    })
}

/// The tag of a set of objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionTag {
    /// Nothing is selected.
    None,
    One(ClassId),
    /// The objects have different tags.
    Mixed,
}

/// The tag that the objects share.
pub fn tag_of(doc: &Document, ids: &[ObjectId]) -> SelectionTag {
    let mut it = ids.iter().filter_map(|id| doc.object(*id)).map(|o| o.class);
    let Some(first) = it.next() else {
        return SelectionTag::None;
    };
    if it.all(|c| c == first) {
        SelectionTag::One(first)
    } else {
        SelectionTag::Mixed
    }
}

/// The command that gives the objects (and the members of the groups among them) the tag, in one
/// undo step. `None` when they have it already. The tag of an object is the tag of its geometry:
/// a surface model has one class for all its faces and edges.
pub fn set_tag(doc: &Document, ids: &[ObjectId], class: ClassId) -> Option<Command> {
    doc.class(class)?;
    let commands: Vec<Command> = doc
        .with_members(ids)
        .into_iter()
        .filter(|id| doc.object(*id).is_some_and(|o| o.class != class))
        .map(|id| Command::ModifyObject {
            id,
            shape: None,
            layer: None,
            class: Some(class),
            attrs: None,
        })
        .collect();
    (!commands.is_empty()).then_some(Command::Batch { commands })
}

#[cfg(test)]
mod tests;
