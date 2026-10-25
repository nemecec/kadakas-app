//! Runs what a tool finished as one command of the document: one `edit_surface` command (or a few
//! in one batch) is one undo step. The tools work in world coordinates; this turns the points into
//! the local coordinates of the model.

use std::collections::BTreeSet;

use glam::{DAffine3, DVec3};
use kadakas_core::command::{Command, CommandError, Editor};
use kadakas_core::model::{Attributes, Document, ObjectId, Shape};
use kadakas_core::resource::ResourceId;
use kadakas_core::surface::{Analytic, EdgeId, Edit, FaceId, IntersectScope};

use super::scenes::{elevation, frame_of, model_of};

/// The model that an action works in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// An existing surface model.
    Model(ObjectId),
    /// A new model on the active layer, made by the same undo step.
    New,
}

/// A guide that a tool made, in world coordinates (mm). The step puts it on the active layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuideSpec {
    pub at: DVec3,
    /// The direction of a guide line; `None` makes a guide point.
    pub dir: Option<DVec3>,
    /// The length of a finite guide line; `None` is a line without ends.
    pub length: Option<f64>,
}

impl GuideSpec {
    pub fn point(at: DVec3) -> GuideSpec {
        GuideSpec {
            at,
            dir: None,
            length: None,
        }
    }

    pub fn line(at: DVec3, dir: DVec3) -> GuideSpec {
        GuideSpec {
            at,
            dir: Some(dir),
            length: None,
        }
    }

    pub fn segment(at: DVec3, dir: DVec3, length: f64) -> GuideSpec {
        GuideSpec {
            at,
            dir: Some(dir),
            length: Some(length),
        }
    }
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
        /// A polygon: its corners stay corners (see `Edit::AddCurve`).
        faceted: bool,
        /// The exact circle of the curve in world coordinates (MOD-15). `run` turns it into the
        /// local coordinates of the model.
        analytic: Option<Analytic>,
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
    /// Intersect Faces in one model: the whole model, or the chosen faces and edges against it.
    Intersect {
        model: ObjectId,
        scope: IntersectScope,
        faces: Vec<FaceId>,
        edges: Vec<EdgeId>,
    },
    /// Guides made by the Tape Measure: one object each, on the active layer and class, in one
    /// undo step.
    AddGuides { guides: Vec<GuideSpec> },
    /// Guides erased (the Eraser): the objects go in one undo step.
    EraseGuides { ids: Vec<ObjectId> },
    /// Move, Rotate or Scale (and the copies of Move and Rotate) of whole objects and of faces and
    /// edges of surface models, by one affine map in world coordinates. `copies` is 0 to change the
    /// geometry in place, else the number of copies (the first placed by the map, the next by the
    /// map applied twice, and so on).
    Xform {
        objects: Vec<ObjectId>,
        parts: Vec<(ObjectId, Vec<FaceId>, Vec<EdgeId>)>,
        matrix: DAffine3,
        copies: u32,
    },
    /// The Paint Bucket on faces: for each model the faces whose front and the faces whose back
    /// get the material (`None` clears it).
    Paint {
        models: Vec<(ObjectId, Vec<FaceId>, Vec<FaceId>)>,
        material: Option<ResourceId>,
    },
    /// The Paint Bucket on whole objects (surface models, groups, component instances).
    PaintObjects {
        objects: Vec<ObjectId>,
        material: Option<ResourceId>,
    },
    /// One edit of a surface model, already in its local coordinates (Offset, Follow Me, the
    /// styles of edges).
    Edit { model: ObjectId, edit: Edit },
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
            | Action::Delete { model, .. }
            | Action::Edit { model, .. }
            | Action::Intersect { model, .. } => Some(*model),
            Action::Xform { parts, .. } => parts.first().map(|p| p.0),
            Action::AddGuides { .. } | Action::EraseGuides { .. } => None,
            Action::Paint { models, .. } => models.first().map(|p| p.0),
            Action::PaintObjects { .. } => None,
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

/// The description of a circle in the local coordinates of the model (`None`: a new model on the
/// active layer). A model that is scaled differently along the axes of the circle makes it an
/// ellipse, and the core drops the description then, with a note.
fn local_circle(doc: &Document, model: Option<ObjectId>, a: &Analytic) -> Analytic {
    let mut out = a.clone();
    out.center = local(doc, model, a.center()).into();
    if let Some(id) = model {
        let fr = frame_of(doc, id);
        if fr.matrix3.determinant().abs() > 1e-12 {
            let inv = fr.matrix3.inverse();
            let (u, n) = (a.axis(), a.normal());
            out.radius = (inv * (u * a.radius)).length();
            out.axis = (inv * u).normalize_or_zero().to_array();
            out.normal = (fr.matrix3.transpose() * n).normalize_or_zero().to_array();
        }
    }
    out
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
    run_full(editor, actions, undo_first).map(|r| (r.used, r.notes))
}

/// What a run of actions did, for the window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ran {
    /// The model that the last action worked in.
    pub used: Option<ObjectId>,
    /// What the step changed that was not asked for, in the language of the window.
    pub notes: Vec<String>,
    /// The copies of objects that a Move or Rotate with copy made (the last copy), which become
    /// the selection.
    pub objects: Vec<ObjectId>,
    /// The faces and edges of the last copy of geometry, which become the selection.
    pub parts: Vec<super::select::Item>,
}

/// Runs the actions like [`run_noted`] and also returns the copies that they made.
pub fn run_full(editor: &mut Editor, actions: &[Action], undo_first: bool) -> Result<Ran, String> {
    if actions.is_empty() {
        return Ok(Ran::default());
    }
    if undo_first {
        editor.undo();
    }
    let before: BTreeSet<ObjectId> = editor.doc().objects.keys().copied().collect();
    // The next id of each model that a copy of geometry goes into: what is above it is new.
    let next_ids: std::collections::BTreeMap<ObjectId, u32> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Xform { parts, copies, .. } if *copies > 0 => Some(parts),
            _ => None,
        })
        .flatten()
        .filter_map(|(m, _, _)| model_of(editor.doc(), *m).map(|s| (*m, s.next_id())))
        .collect();
    let mut used: Option<ObjectId> = None;
    let out = editor.execute_stepwise(|step| {
        let mut made: Option<ObjectId> = None;
        for a in actions {
            if let Action::Xform { .. } = a {
                for c in xform_commands(step.doc(), a) {
                    step.run(&c)?;
                }
                used = a.model().or(used);
                continue;
            }
            if let Action::AddGuides { guides } = a {
                let ground = doc_ground(step.doc());
                for g in guides {
                    step.run(&Command::AddObject {
                        shape: Shape::Guide(Box::new(kadakas_core::guide::Guide {
                            at: g.at - DVec3::new(0.0, 0.0, ground),
                            dir: g.dir,
                            length: g.length,
                        })),
                        layer: None,
                        class: None,
                        attrs: Attributes::default(),
                    })?;
                }
                continue;
            }
            if let Action::EraseGuides { ids } = a {
                step.run(&Command::DeleteObjects { ids: ids.clone() })?;
                continue;
            }
            if let Action::Paint { .. } | Action::PaintObjects { .. } = a {
                for c in paint_commands(a) {
                    step.run(&c)?;
                }
                used = a.model().or(used);
                continue;
            }
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
        Ok(o) => {
            let mut ran = Ran {
                used,
                notes: o
                    .notes
                    .into_iter()
                    .map(|n| match kadakas_core::command::height_note_kind(&n) {
                        Some(kind) => kadakas_l10n::tr!("uic-modeling-height-kept", kind = kind),
                        None => super::messages::localize(&n),
                    })
                    .collect(),
                ..Ran::default()
            };
            for a in actions {
                let Action::Xform {
                    objects,
                    parts,
                    matrix,
                    copies,
                } = a
                else {
                    continue;
                };
                if *copies == 0 {
                    continue;
                }
                // The copies of objects come in the order of the copies: the last ones.
                if !objects.is_empty() && o.created.len() >= objects.len() {
                    ran.objects = o.created[o.created.len() - objects.len()..]
                        .iter()
                        .map(|u| ObjectId(*u))
                        .collect();
                }
                for (m, faces, edges) in parts {
                    ran.parts.extend(copied_parts(
                        editor.doc(),
                        *m,
                        next_ids.get(m).copied().unwrap_or(u32::MAX),
                        faces,
                        edges,
                        matrix,
                        *copies,
                    ));
                }
            }
            Ok(ran)
        }
        Err(e) => {
            // The step that was taken back for the redo comes back: a refused value loses nothing.
            if undo_first {
                editor.redo();
            }
            Err(super::messages::localize(&e.to_string()))
        }
    }
}

/// The faces and edges of the last copy: those made by the step (ids from `next`) that lie where
/// the chosen faces and edges go with the map applied `copies` times.
fn copied_parts(
    doc: &Document,
    model: ObjectId,
    next: u32,
    faces: &[FaceId],
    edges: &[EdgeId],
    matrix: &DAffine3,
    copies: u32,
) -> Vec<super::select::Item> {
    use super::select::Item;
    let Some(m) = model_of(doc, model) else {
        return Vec::new();
    };
    let fr = frame_of(doc, model);
    if fr.matrix3.determinant().abs() < 1e-12 {
        return Vec::new();
    }
    let local = fr.inverse() * *matrix * fr;
    let mut at = DAffine3::IDENTITY;
    for _ in 0..copies {
        at = local * at;
    }
    let mid = |ps: Vec<DVec3>| ps.iter().copied().sum::<DVec3>() / ps.len().max(1) as f64;
    let face_mid = |f: &kadakas_core::surface::Face| {
        mid(f.outer.iter().filter_map(|v| m.vertex(*v)).collect())
    };
    let mut out = Vec::new();
    for f in faces {
        let Some(face) = m.face(*f) else { continue };
        let want = at.transform_point3(face_mid(face));
        if let Some((g, _)) = m
            .faces()
            .find(|(g, h)| g.0 >= next && face_mid(h).distance(want) < 0.05)
        {
            out.push(Item::Face(model, g));
        }
    }
    for e in edges {
        let Some(edge) = m.edge(*e) else { continue };
        let ends = |e: &kadakas_core::surface::Edge| {
            mid(e.v.iter().filter_map(|v| m.vertex(*v)).collect())
        };
        let want = at.transform_point3(ends(&edge));
        if let Some((g, _)) = m
            .edges()
            .find(|(g, h)| g.0 >= next && ends(h).distance(want) < 0.05)
        {
            out.push(Item::Edge(model, g));
        }
    }
    out
}

/// The commands of the Paint Bucket: one edit for each model and side, or one slot for each
/// object.
fn paint_commands(a: &Action) -> Vec<Command> {
    use kadakas_core::present::SlotTarget;
    use kadakas_core::surface::PaintSide;
    match a {
        Action::Paint { models, material } => {
            let mut v = Vec::new();
            for (id, front, back) in models {
                for (faces, side) in [(front, PaintSide::Front), (back, PaintSide::Back)] {
                    if faces.is_empty() {
                        continue;
                    }
                    v.push(Command::EditSurface {
                        id: *id,
                        edit: Edit::SetFaceMaterial {
                            faces: faces.clone(),
                            side,
                            material: material.map(|m| m.to_string()),
                        },
                    });
                }
            }
            v
        }
        Action::PaintObjects { objects, material } => objects
            .iter()
            .map(|o| Command::SetMaterialSlot {
                target: SlotTarget::Object {
                    object: *o,
                    slot: None,
                },
                material: *material,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn xform_commands(doc: &Document, a: &Action) -> Vec<Command> {
    let Action::Xform {
        objects,
        parts,
        matrix,
        copies,
    } = a
    else {
        return Vec::new();
    };
    let mut v = Vec::new();
    if !objects.is_empty() {
        v.push(Command::TransformObjects3d {
            ids: objects.clone(),
            matrix: matrix.to_cols_array(),
            copy: *copies > 0,
            count: (*copies).max(1),
        });
    }
    for (model, faces, edges) in parts {
        let fr = frame_of(doc, *model);
        if fr.matrix3.determinant().abs() < 1e-12 {
            continue;
        }
        let local = (fr.inverse() * *matrix * fr).to_cols_array();
        let edit = if *copies > 0 {
            Edit::CopyGeometry {
                faces: faces.clone(),
                edges: edges.clone(),
                matrix: local,
                count: *copies,
            }
        } else {
            Edit::TransformVertices {
                vertices: Vec::new(),
                faces: faces.clone(),
                edges: edges.clone(),
                matrix: local,
            }
        };
        v.push(Command::EditSurface { id: *model, edit });
    }
    v
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
        Action::AddCurve {
            points,
            closed,
            faceted,
            analytic,
            ..
        } => vec![edit(Edit::AddCurve {
            points: points.iter().map(|p| at(*p)).collect(),
            closed: *closed,
            faceted: *faceted,
            analytic: analytic
                .as_ref()
                .map(|a| local_circle(doc, if fresh { None } else { Some(id) }, a)),
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
        Action::Intersect {
            scope,
            faces,
            edges,
            ..
        } => vec![edit(Edit::IntersectFaces {
            scope: *scope,
            faces: faces.clone(),
            edges: edges.clone(),
        })],
        Action::Edit { edit: e, .. } => vec![edit(e.clone())],
        Action::Xform { .. }
        | Action::Paint { .. }
        | Action::PaintObjects { .. }
        | Action::AddGuides { .. }
        | Action::EraseGuides { .. } => Vec::new(),
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
