//! The edit context: where a person works in a model that has groups and components.

use glam::DVec3;

use super::raw_model;
use crate::command::{Command, CommandError, Editor, Outcome};
use crate::model::{Document, LayerId, ObjectId, Shape};
use crate::surface::{Edit, SurfaceModel};

/// One step into a container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The group or the component instance that was entered.
    pub id: ObjectId,
    /// For an instance: the edit group that holds the parts of its definition while it is open.
    pub edit_group: Option<ObjectId>,
}

/// The path of ids of the containers that a person has entered (outermost first), and whether
/// the rest of the model is shown dimmed. It is plain data of the UI session, never saved. The
/// document is not changed by entering a group; entering a component opens it for edit in place
/// (`Command::EditComponent`), and leaving it writes the change to the definition
/// (`Command::CloseComponent`). Both are one undo step each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditContext {
    frames: Vec<Frame>,
    /// The rest of the model (everything outside the entered container) is dimmed and cannot be
    /// picked or edited. The UI reads this together with [`EditContext::dimmed`].
    pub dim_rest: bool,
}

impl Default for EditContext {
    fn default() -> Self {
        EditContext {
            frames: Vec::new(),
            dim_rest: true,
        }
    }
}

fn invalid(m: impl Into<String>) -> CommandError {
    CommandError::Invalid(m.into())
}

impl EditContext {
    /// The context of a person who has entered nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// True if nothing is entered.
    pub fn is_root(&self) -> bool {
        self.frames.is_empty()
    }

    /// The frames, outermost first.
    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    /// The ids that were entered (groups and instances), outermost first.
    pub fn path(&self) -> Vec<ObjectId> {
        self.frames.iter().map(|f| f.id).collect()
    }

    /// The object whose children are the level of the context: the group, or the edit group of an
    /// open component. `None` at the root.
    pub fn container(&self) -> Option<ObjectId> {
        self.frames.last().map(|f| f.edit_group.unwrap_or(f.id))
    }

    /// Enters a group or a component that is a member of the current container. A component
    /// is opened for edit in place.
    pub fn enter(&mut self, ed: &mut Editor, id: ObjectId) -> Result<(), CommandError> {
        self.validate(ed.doc());
        let o = ed.doc().object(id).ok_or(CommandError::NoObject(id))?;
        if o.parent != self.container() {
            return Err(invalid(format!(
                "object {id} is not in the group or component that is open"
            )));
        }
        let frame = match o.shape {
            Shape::Group { .. } => Frame {
                id,
                edit_group: None,
            },
            Shape::Component { .. } => {
                let out = ed.execute(&Command::EditComponent { id })?;
                Frame {
                    id,
                    edit_group: out.created.first().map(|u| ObjectId(*u)),
                }
            }
            _ => {
                return Err(invalid(
                    "only a group or a component can be opened for edit",
                ));
            }
        };
        self.frames.push(frame);
        Ok(())
    }

    /// Enters a chain of containers, outermost first (a double click on geometry deep inside).
    /// On an error the context is as it was before.
    pub fn enter_path(&mut self, ed: &mut Editor, ids: &[ObjectId]) -> Result<(), CommandError> {
        let keep = self.frames.len();
        for &id in ids {
            if let Err(e) = self.enter(ed, id) {
                while self.frames.len() > keep {
                    let _ = self.exit(ed);
                }
                return Err(e);
            }
        }
        Ok(())
    }

    /// Leaves the innermost container. For a component the change is written to the definition,
    /// and every instance shows it. On an error the container stays open.
    pub fn exit(&mut self, ed: &mut Editor) -> Result<Option<ObjectId>, CommandError> {
        self.validate(ed.doc());
        let Some(f) = self.frames.last().copied() else {
            return Ok(None);
        };
        if let Some(g) = f.edit_group {
            ed.execute(&Command::CloseComponent { group: g })?;
        }
        self.frames.pop();
        Ok(Some(f.id))
    }

    /// Leaves every container, innermost first. Stops at the first error.
    pub fn exit_all(&mut self, ed: &mut Editor) -> Result<(), CommandError> {
        while !self.is_root() {
            self.exit(ed)?;
        }
        Ok(())
    }

    /// Drops the frames whose group, instance or edit group is gone (after an undo, a delete, or a
    /// change by somebody else). Returns true if something was dropped.
    pub fn validate(&mut self, doc: &Document) -> bool {
        let ok = |f: &Frame| {
            doc.object(f.id).is_some() && f.edit_group.is_none_or(|g| doc.object(g).is_some())
        };
        let keep = self.frames.iter().take_while(|f| ok(f)).count();
        let changed = keep != self.frames.len();
        self.frames.truncate(keep);
        changed
    }

    /// True if the object is inside the container (or is the container). Only such objects can be
    /// picked and edited. At the root, every object is.
    pub fn contains(&self, doc: &Document, id: ObjectId) -> bool {
        match self.container() {
            None => true,
            Some(c) => id == c || doc.ancestors(id).contains(&c),
        }
    }

    /// True if the object is drawn dimmed: a container is open, `dim_rest` is on, and the object
    /// is outside it.
    pub fn dimmed(&self, doc: &Document, id: ObjectId) -> bool {
        self.dim_rest && !self.contains(doc, id)
    }

    /// What a click on `id` selects here: the object or the group that holds it that is a member
    /// of the container, or `None` for an object outside the container.
    pub fn pick(&self, doc: &Document, id: ObjectId) -> Option<ObjectId> {
        doc.selection_target(id, self.container())
    }

    fn layer(&self, doc: &Document) -> LayerId {
        self.container()
            .and_then(|c| doc.object(c))
            .map_or(doc.active_layer, |o| o.layer)
    }

    /// The raw model of the current level (see the module `component`): where drawing goes.
    pub fn raw_model(&self, doc: &Document) -> Option<ObjectId> {
        raw_model(doc, self.container(), self.layer(doc))
    }

    fn model<'a>(&self, doc: &'a Document) -> Option<&'a SurfaceModel> {
        match &doc.object(self.raw_model(doc)?)?.shape {
            Shape::SurfaceModel(m) => Some(m),
            _ => None,
        }
    }

    /// A point of the drawing in the local coordinates of the raw model.
    pub fn to_local(&self, doc: &Document, world: DVec3) -> Option<DVec3> {
        Some(self.model(doc)?.transform.inverse().transform_point3(world))
    }

    /// A point in the local coordinates of the raw model, in the drawing.
    pub fn to_world(&self, doc: &Document, local: DVec3) -> Option<DVec3> {
        Some(self.model(doc)?.transform.transform_point3(local))
    }

    /// Runs an edit of the raw model with points in the drawing: the points of `AddEdges` are
    /// converted into the local coordinates of the model; ids in the other edits are used as
    /// they are. If the level has no surface model yet, one is made in the same undo step.
    pub fn apply_world(&self, ed: &mut Editor, edit: &Edit) -> Result<Outcome, CommandError> {
        let container = self.container();
        let existing = self.raw_model(ed.doc());
        let layer = self.layer(ed.doc());
        ed.execute_stepwise(|run| {
            let id = match existing {
                Some(id) => id,
                None => {
                    let shape = Shape::SurfaceModel(Box::new(SurfaceModel::new()));
                    match container {
                        Some(parent) => run.run(&Command::AddObjectIn {
                            parent,
                            shape,
                            class: None,
                            attrs: Default::default(),
                            id: None,
                        })?,
                        None => run.run(&Command::AddObject {
                            shape,
                            layer: Some(layer),
                            class: None,
                            attrs: Default::default(),
                        })?,
                    }
                    raw_model(run.doc(), container, layer)
                        .ok_or_else(|| invalid("the surface model was not made"))?
                }
            };
            let local = match (edit, run.doc().object(id).map(|o| &o.shape)) {
                (Edit::AddEdges { edges }, Some(Shape::SurfaceModel(m))) => {
                    let inv = m.transform.inverse();
                    Edit::AddEdges {
                        edges: edges
                            .iter()
                            .map(|[a, b]| {
                                [
                                    inv.transform_point3(DVec3::from(*a)).to_array(),
                                    inv.transform_point3(DVec3::from(*b)).to_array(),
                                ]
                            })
                            .collect(),
                    }
                }
                _ => edit.clone(),
            };
            run.run(&Command::EditSurface { id, edit: local })
        })
    }
}
