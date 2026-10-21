//! Runs what a tool finished as one command of the document: one `edit_surface` command (or a few
//! in one batch) is one undo step. The tools work in world coordinates; this turns the points into
//! the local coordinates of the model.

use std::collections::BTreeSet;

use glam::DVec3;
use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::model::{Attributes, Document, ObjectId, Shape};
use kadakas_core::surface::{EdgeId, Edit, FaceId};

use super::scenes::{elevation, frame_of, model_of};

/// The model that an action works in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// An existing surface model.
    Model(ObjectId),
    /// A new model on the active layer, made by the same undo step.
    New,
}

/// A finished operation of a tool, in world coordinates (mm).
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Straight edges, each from one point to another.
    AddEdges {
        target: Target,
        segments: Vec<[DVec3; 2]>,
    },
    /// A circle or an arc: a chain of edges kept as one curve.
    AddCurve {
        target: Target,
        points: Vec<DVec3>,
        closed: bool,
    },
    /// Edges erased (the Eraser).
    EraseEdges { model: ObjectId, edges: Vec<EdgeId> },
    /// Push/Pull of one face.
    PushPull {
        model: ObjectId,
        face: FaceId,
        distance: f64,
    },
    /// Delete of the selected faces and edges of one model: the faces go first, then the edges.
    Delete {
        model: ObjectId,
        faces: Vec<FaceId>,
        edges: Vec<EdgeId>,
    },
}

/// The model that an action works in (a new model counts as `None`).
impl Action {
    pub fn model(&self) -> Option<ObjectId> {
        match self {
            Action::AddEdges { target, .. } | Action::AddCurve { target, .. } => match target {
                Target::Model(m) => Some(*m),
                Target::New => None,
            },
            Action::EraseEdges { model, .. }
            | Action::PushPull { model, .. }
            | Action::Delete { model, .. } => Some(*model),
        }
    }
}

fn local(doc: &Document, model: Option<ObjectId>, p: DVec3) -> [f64; 3] {
    let q = match model {
        Some(id) => {
            let fr = frame_of(doc, id);
            if fr.matrix3.determinant().abs() > 1e-12 {
                fr.inverse().transform_point3(p)
            } else {
                p
            }
        }
        None => p - DVec3::new(0.0, 0.0, doc_ground(doc)),
    };
    q.to_array()
}

fn doc_ground(doc: &Document) -> f64 {
    doc.layer(doc.active_layer).map_or(0.0, |l| l.elevation)
}

/// Runs the actions as one undo step. Returns the model that the last action worked in (a new
/// model is made when an action asks for one, once for the step). With `undo_first` the last
/// step of the history is taken back first (the person typed a new value for what was just done).
pub fn run(
    editor: &mut Editor,
    actions: &[Action],
    undo_first: bool,
) -> Result<Option<ObjectId>, String> {
    run_noted(editor, actions, undo_first).map(|(used, _)| used)
}

/// Runs the actions like [`run`] and also returns the notes of the commands (what the step changed
/// that was not asked for, such as the material of a face that merged away), for the status line.
pub fn run_noted(
    editor: &mut Editor,
    actions: &[Action],
    undo_first: bool,
) -> Result<(Option<ObjectId>, Vec<String>), String> {
    if actions.is_empty() {
        return Ok((None, Vec::new()));
    }
    if undo_first {
        editor.undo();
    }
    let before: BTreeSet<ObjectId> = editor.doc().objects.keys().copied().collect();
    let mut used: Option<ObjectId> = None;
    let out = editor.execute_stepwise(|step| {
        let mut made: Option<ObjectId> = None;
        for a in actions {
            // A model that the step taken back had made is not there any more: make it again.
            let mut model = a.model().filter(|id| {
                step.doc().object(*id).is_some()
                    || !matches!(a, Action::AddEdges { .. } | Action::AddCurve { .. })
            });
            if model.is_none() {
                // A new model, once for the whole step.
                if made.is_none() {
                    step.run(&Command::AddObject {
                        shape: Shape::SurfaceModel(Box::default()),
                        layer: None,
                        class: None,
                        attrs: Attributes::default(),
                    })?;
                    made = step
                        .doc()
                        .objects
                        .keys()
                        .find(|k| !before.contains(k))
                        .copied();
                }
                model = made;
            }
            let Some(id) = model else {
                return Err(CommandError::Invalid("no model to draw in".into()));
            };
            let cmds = commands(step.doc(), id, made == Some(id), a);
            for c in cmds {
                step.run(&c)?;
            }
            used = Some(id);
        }
        Ok(())
    });
    match out {
        Ok(o) => Ok((used, o.notes)),
        Err(e) => {
            // The step that was taken back for the redo comes back: a refused value loses nothing.
            if undo_first {
                editor.redo();
            }
            Err(e.to_string())
        }
    }
}

fn commands(doc: &Document, id: ObjectId, fresh: bool, a: &Action) -> Vec<Command> {
    // A model that this step made has the identity transform on the active layer, like the
    // coordinates `local` gives for `None`.
    let at = |p: DVec3| local(doc, if fresh { None } else { Some(id) }, p);
    let edit = |e: Edit| Command::EditSurface { id, edit: e };
    match a {
        Action::AddEdges { segments, .. } => vec![edit(Edit::AddEdges {
            edges: segments.iter().map(|s| [at(s[0]), at(s[1])]).collect(),
        })],
        Action::AddCurve { points, closed, .. } => vec![edit(Edit::AddCurve {
            points: points.iter().map(|p| at(*p)).collect(),
            closed: *closed,
        })],
        Action::EraseEdges { edges, .. } => vec![edit(Edit::EraseEdges {
            edges: edges.clone(),
        })],
        Action::PushPull { face, distance, .. } => {
            vec![edit(Edit::PushPull {
                face: *face,
                distance: local_distance(doc, id, *face, *distance),
            })]
        }
        Action::Delete { faces, edges, .. } => {
            let mut v = Vec::new();
            if !faces.is_empty() {
                v.push(edit(Edit::EraseFaces {
                    faces: faces.clone(),
                }));
            }
            if !edges.is_empty() {
                v.push(edit(Edit::EraseEdges {
                    edges: edges.clone(),
                }));
            }
            v
        }
    }
}

/// A distance along the normal of a face in the world (mm), as a distance along its normal in the
/// local coordinates of the model: a model that is scaled has other units inside.
pub fn local_distance(doc: &Document, id: ObjectId, face: FaceId, d: f64) -> f64 {
    let Some(f) = model_of(doc, id).and_then(|m| m.face(face)) else {
        return d;
    };
    let m3 = frame_of(doc, id).matrix3;
    if m3.determinant().abs() < 1e-12 {
        return d;
    }
    let n_local = f.plane.normal;
    let n_world = (m3.inverse().transpose() * n_local).normalize_or_zero();
    let v = m3.inverse() * (n_world * d);
    if v.dot(n_local) < 0.0 {
        -v.length()
    } else {
        v.length()
    }
}

/// The elevation that a model of the document sits at (mm).
pub fn model_elevation(doc: &Document, id: ObjectId) -> f64 {
    elevation(doc, id)
}
