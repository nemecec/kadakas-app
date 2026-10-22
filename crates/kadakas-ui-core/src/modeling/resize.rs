//! Resize the model (MOD-05, Tape Measure): what the scale will do, found before the question
//! is asked, and the command that does it.
//!
//! The scale is about the **first point of the measure**, so that point stays where it is. A scale
//! about the origin would move the whole model away from what the person is looking at. (The
//! point that other modelers use was not checked against a manual.) The person can move the model
//! afterwards. All objects of the open level scale, guides too. The model is one undo step.
//!
//! The plan objects (plants, hardscapes, site models and the like) are scaled in the plan only. Their
//! heights come from their layers and stay, and the question says how many there are. If any
//! object is on a locked layer or on a grey tag, nothing is scaled and the answer names them
//! before the question, because the command refuses the whole step in that case.

use kadakas_core::command::Command;
use kadakas_core::model::{Document, LayerKind, ObjectId, Shape};

use super::tape::Resize;

/// What a resize would do.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResizePlan {
    /// The objects of the open level (groups included) that the command scales.
    pub ids: Vec<ObjectId>,
    /// How many objects are scaled, groups not counted, members counted.
    pub scaled: usize,
    /// How many of them are plan objects that keep their height.
    pub plan_objects: usize,
    /// The names of the locked layers that hold objects of the level. Not empty: no resize.
    pub locked_layers: Vec<String>,
    /// How many objects are on a grey tag. More than zero: no resize.
    pub grey_objects: usize,
}

impl ResizePlan {
    /// True if the resize can run.
    pub fn allowed(&self) -> bool {
        !self.ids.is_empty() && self.locked_layers.is_empty() && self.grey_objects == 0
    }
}

/// The plan for the objects of the level `level` (`None`: the top level).
pub fn resize_plan(doc: &Document, level: Option<ObjectId>) -> ResizePlan {
    let ids: Vec<ObjectId> = doc
        .objects
        .values()
        .filter(|o| o.parent == level)
        .filter(|o| {
            doc.layer(o.layer)
                .is_some_and(|l| l.kind == LayerKind::Design)
        })
        .map(|o| o.id)
        .collect();
    let mut plan = ResizePlan {
        scaled: 0,
        ..ResizePlan::default()
    };
    for id in doc.with_members(&ids) {
        let Some(o) = doc.object(id) else { continue };
        if matches!(o.shape, Shape::Group { .. }) {
            continue;
        }
        plan.scaled += 1;
        if !matches!(
            o.shape,
            Shape::SurfaceModel(_) | Shape::Component { .. } | Shape::Guide(_)
        ) {
            plan.plan_objects += 1;
        }
        if let Some(l) = doc.layer(o.layer)
            && l.locked
            && !plan.locked_layers.contains(&l.name)
        {
            plan.locked_layers.push(l.name.clone());
        }
        if doc.drawing.grey.classes.contains(&o.class) {
            plan.grey_objects += 1;
        }
    }
    plan.locked_layers.sort();
    plan.ids = ids;
    plan
}

/// The command that scales the objects of the plan by the factor about the first point.
pub fn resize_command(plan: &ResizePlan, r: &Resize) -> Command {
    let about = glam::DAffine3::from_translation(r.about);
    let m = about * glam::DAffine3::from_scale(glam::DVec3::splat(r.factor)) * about.inverse();
    Command::TransformObjects3d {
        ids: plan.ids.clone(),
        matrix: m.to_cols_array(),
        copy: false,
        count: 1,
    }
}
