//! Fillet and Chamfer in the modeler (MOD-21): the panel of the tray and the commands of the Edit
//! menu and the context menu. Headless: the window draws the panel from [`FilletPanel`] and runs
//! the actions it gives as one undo step.
//!
//! The commands work on the selected edges and on the edges of the selected faces. A selected
//! edge is taken as it is. Of a selected face only the sharp edges count: the edges at an angle of
//! more than 5 degrees that are not soft or smooth and that are convex (so a face of a curved
//! surface does not round its own facets). The size is in world mm: the panel turns it into the
//! units of a scaled model.

use std::collections::{BTreeMap, BTreeSet};

use kadakas_core::model::{Document, ObjectId};
use kadakas_core::surface::{EdgeChoice, EdgeId, Edit, FaceId};

use super::exec::Action;
use super::scenes::{frame_of, model_of};
use super::select::SubSelection;

/// Round or cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilletKind {
    /// A round of the radius of the panel.
    Fillet,
    /// A flat cut at the distance of the panel.
    Chamfer,
}

/// The values of the panel.
#[derive(Clone, Debug, PartialEq)]
pub struct FilletPanel {
    /// The radius of a fillet or the distance of a chamfer, in world mm.
    pub size: f64,
    /// The segments of a round; 0 lets the program choose from the radius and the angle.
    pub segments: u32,
    /// The edges inside a round are soft and smooth, so it shades round.
    pub smooth: bool,
}

/// The size of the panel when the modeler starts (mm).
pub const DEFAULT_FILLET_SIZE: f64 = 10.0;

impl Default for FilletPanel {
    fn default() -> Self {
        FilletPanel {
            size: DEFAULT_FILLET_SIZE,
            segments: 0,
            smooth: true,
        }
    }
}

/// The edges that a fillet or a chamfer takes, by model: the selected edges and the sharp convex
/// edges of the selected faces.
pub fn fillet_edges(doc: &Document, sub: &SubSelection) -> BTreeMap<ObjectId, Vec<EdgeId>> {
    let mut edges: BTreeMap<ObjectId, BTreeSet<EdgeId>> = BTreeMap::new();
    let mut faces: BTreeMap<ObjectId, Vec<FaceId>> = BTreeMap::new();
    for (m, e) in sub.edges() {
        edges.entry(m).or_default().insert(e);
    }
    for (m, f) in sub.faces() {
        faces.entry(m).or_default().push(f);
    }
    let models: BTreeSet<ObjectId> = edges.keys().chain(faces.keys()).copied().collect();
    let mut out = BTreeMap::new();
    for id in models {
        let Some(m) = model_of(doc, id) else {
            continue;
        };
        let choice = EdgeChoice {
            edges: edges.remove(&id).unwrap_or_default().into_iter().collect(),
            faces: faces.remove(&id).unwrap_or_default(),
            ..EdgeChoice::default()
        };
        if let Ok(es) = m.fillet_edges(&choice)
            && !es.is_empty()
        {
            out.insert(id, es);
        }
    }
    out
}

impl FilletPanel {
    /// One action for each model with edges to round or cut. Empty when nothing fits the choice.
    /// A model that its placement scales unequally along its axes is refused: a round in it would
    /// be an ellipse in the drawing (the text of the refusal is in the language of the window).
    pub fn actions(
        &self,
        doc: &Document,
        sub: &SubSelection,
        kind: FilletKind,
    ) -> Result<Vec<Action>, String> {
        fillet_edges(doc, sub)
            .into_iter()
            .map(|(model, edges)| {
                // The size in the units of the model: the placement scales each axis.
                let m = frame_of(doc, model).matrix3;
                let lengths = [m.x_axis.length(), m.y_axis.length(), m.z_axis.length()];
                let scale = lengths[0];
                if scale <= 0.0 || lengths.iter().any(|l| (l - scale).abs() > 1e-9 * scale) {
                    return Err(kadakas_l10n::tr!("uic-fillet-refused-scaled"));
                }
                let size = self.size / scale;
                let edit = match kind {
                    FilletKind::Fillet => Edit::Fillet {
                        edges,
                        radius: size,
                        segments: (self.segments > 0).then_some(self.segments),
                        smooth: self.smooth,
                    },
                    FilletKind::Chamfer => Edit::Chamfer {
                        edges,
                        distance: size,
                    },
                };
                Ok(Action::Edit { model, edit })
            })
            .collect()
    }
}
