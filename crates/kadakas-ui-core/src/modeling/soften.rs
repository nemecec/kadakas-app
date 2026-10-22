//! Soft and smooth edges in the modeler (MOD-08): the Soften Edges panel and the commands of the
//! context menu (Soften, Smooth, Unsoften). Headless: the window draws the panel from
//! [`SoftenPanel`] and runs the actions it gives.
//!
//! The panel works on the selected edges and on the edges of the selected faces. Each change of
//! the slider or of a check box applies at once (a live preview); the changes in a row are one
//! undo step, because each one takes back the one before when nothing else happened since.

use std::collections::{BTreeMap, BTreeSet};

use kadakas_core::model::{Document, ObjectId};
use kadakas_core::surface::{EdgeId, Edit};

use super::exec::Action;
use super::select::{SubSelection, face_edges};
use super::tools::Out;

fn selection_key(sub: &SubSelection) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    sub.hash(&mut h);
    h.finish()
}

/// The commands of the context menu for selected edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeStyle {
    /// Soft and smooth: the line goes and the faces shade as one surface.
    Soften,
    /// Smooth only: the faces shade as one surface, the line stays.
    Smooth,
    /// Neither: a hard edge.
    Unsoften,
}

/// The selected edges and the edges of the selected faces, by model.
pub fn selected_edges(doc: &Document, sub: &SubSelection) -> BTreeMap<ObjectId, Vec<EdgeId>> {
    let mut by: BTreeMap<ObjectId, BTreeSet<EdgeId>> = BTreeMap::new();
    for (m, e) in sub.edges() {
        by.entry(m).or_default().insert(e);
    }
    for (m, f) in sub.faces() {
        by.entry(m).or_default().extend(face_edges(doc, m, f));
    }
    by.into_iter()
        .filter(|(_, es)| !es.is_empty())
        .map(|(m, es)| (m, es.into_iter().collect()))
        .collect()
}

/// One action for each model with selected edges: the flags of `style`.
pub fn edge_style_actions(doc: &Document, sub: &SubSelection, style: EdgeStyle) -> Vec<Action> {
    let (soft, smooth) = match style {
        EdgeStyle::Soften => (Some(true), Some(true)),
        EdgeStyle::Smooth => (None, Some(true)),
        EdgeStyle::Unsoften => (Some(false), Some(false)),
    };
    selected_edges(doc, sub)
        .into_iter()
        .map(|(model, edges)| Action::Edit {
            model,
            edit: Edit::SetEdgeStyle {
                edges,
                soft,
                smooth,
                hidden: None,
            },
        })
        .collect()
}

/// The Soften Edges panel: the angle, "Smooth normals" and "Soften coplanar".
#[derive(Clone, Debug, PartialEq)]
pub struct SoftenPanel {
    /// Edges between faces at an angle below this (degrees, 0 to 180) are softened.
    pub angle: f64,
    /// The softened edges are also smooth (round shading).
    pub smooth: bool,
    /// Edges between faces in one plane are softened too.
    pub coplanar: bool,
    /// The revision after the last change of the panel and the selection it changed: a change at
    /// that revision on the same selection takes it back first, so a drag of the slider is one
    /// undo step.
    applied: Option<(u64, u64)>,
    /// The selection of the last change.
    asked: u64,
}

/// The angle of the panel when the modeler starts (degrees).
pub const DEFAULT_SOFTEN_ANGLE: f64 = 20.0;

impl Default for SoftenPanel {
    fn default() -> Self {
        SoftenPanel {
            angle: DEFAULT_SOFTEN_ANGLE,
            smooth: true,
            coplanar: false,
            applied: None,
            asked: 0,
        }
    }
}

impl SoftenPanel {
    /// The actions for the values of the panel on the selection. `revision` is the revision of
    /// the document now. Empty when nothing is selected.
    pub fn change(&mut self, doc: &Document, sub: &SubSelection, revision: u64) -> Out {
        let actions: Vec<Action> = selected_edges(doc, sub)
            .into_iter()
            .map(|(model, edges)| Action::Edit {
                model,
                edit: Edit::SoftenEdges {
                    edges,
                    angle: self.angle.clamp(0.0, 180.0),
                    smooth: self.smooth,
                    coplanar: self.coplanar,
                },
            })
            .collect();
        self.asked = selection_key(sub);
        Out {
            redo: !actions.is_empty() && self.applied == Some((revision, self.asked)),
            actions,
            message: None,
        }
    }

    /// The actions of [`SoftenPanel::change`] ran: the next change takes them back first.
    pub fn committed(&mut self, revision: u64) {
        self.applied = Some((revision, self.asked));
    }

    /// After the actions of [`SoftenPanel::change`] ran (`redo` as it said, the undo depth before
    /// and after, the revision after): when they left a step of their own on top, the next change
    /// takes it back first; when they changed nothing (the values gave the model it had), there
    /// is no step to take back.
    pub fn ran(&mut self, redo: bool, depth_before: usize, depth_after: usize, revision: u64) {
        let step = if redo {
            depth_after == depth_before
        } else {
            depth_after == depth_before + 1
        };
        if step {
            self.committed(revision);
        } else {
            self.forget();
        }
    }

    /// The selection changed (or another edit ran): the next change is a new undo step.
    pub fn forget(&mut self) {
        self.applied = None;
    }
}
