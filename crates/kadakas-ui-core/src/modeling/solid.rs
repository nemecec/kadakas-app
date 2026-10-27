//! Solid tools in the modeler (MOD-09): the operands that a selection stands for, the preview and
//! the refusals for the status line, the action that runs an operation, and the check of the
//! closed solids of a model. Headless: the window draws the menu and shows the texts.
//!
//! The wording follows the public help of SketchUp (Solid Tools), not verified against the
//! product. The core does the work (`kadakas_core::surface::Edit::Solid`, see its module
//! `solid`); this module only turns the selection into the two lists of faces and the refusals
//! into the language of the window.
//!
//! **Operands.** The selected faces of one surface model name the solids: the solid of each
//! selected face is found by the walk of the core (`SurfaceModel::solid_of`), and the selection
//! must name exactly two. The *first* solid is the one with the lowest face id (the older one),
//! unless the order is swapped.

use std::collections::BTreeSet;

use kadakas_core::model::{Document, ObjectId};
use kadakas_core::surface::{FaceId, ShellState, SolidError, SolidOp, SurfaceError};
use kadakas_l10n::tr;

use super::exec::Action;
use super::messages::localize_error;
use super::scenes::model_of;
use super::select::{Item, SubSelection};
use super::tools::Out;

/// The two solids that a selection names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operands {
    pub model: ObjectId,
    pub first: Vec<FaceId>,
    pub second: Vec<FaceId>,
}

/// Why a selection cannot start a solid operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// No face is selected.
    NoSelection,
    /// The selected faces are in more than one model.
    Models,
    /// The selected faces name this many solids, not two.
    Count(usize),
    /// The core refused the operation; the text is in the language of the window.
    Refused(String),
    /// The core refused the operation because of a near-touch (MOD-19), and a snap would resolve
    /// it. The text is in the language of the window. The window offers "Snap and retry".
    NearTouch(String),
}

impl Problem {
    /// The text for the status line.
    pub fn text(&self) -> String {
        match self {
            Problem::NoSelection => tr!("uic-solid-need-selection"),
            Problem::Models => tr!("uic-solid-one-model"),
            Problem::Count(n) => tr!("uic-solid-need-two", found = *n),
            Problem::Refused(t) | Problem::NearTouch(t) => t.clone(),
        }
    }
}

/// The name of an operation in the language of the window.
pub fn op_name(op: SolidOp) -> String {
    match op {
        SolidOp::Union => tr!("uic-solid-op-union"),
        SolidOp::Subtract => tr!("uic-solid-op-subtract"),
        SolidOp::Intersect => tr!("uic-solid-op-intersect"),
        SolidOp::Trim => tr!("uic-solid-op-trim"),
        SolidOp::Split => tr!("uic-solid-op-split"),
    }
}

/// The two solids of the selected faces. With `swap` the order is the other way round.
pub fn operands(doc: &Document, sub: &SubSelection, swap: bool) -> Result<Operands, Problem> {
    let faces = sub.faces();
    let Some(&(model, _)) = faces.first() else {
        return Err(Problem::NoSelection);
    };
    if faces.iter().any(|(m, _)| *m != model) {
        return Err(Problem::Models);
    }
    let Some(m) = model_of(doc, model) else {
        return Err(Problem::NoSelection);
    };
    let mut solids: Vec<Vec<FaceId>> = Vec::new();
    let mut seen: BTreeSet<FaceId> = BTreeSet::new();
    for (_, f) in faces {
        if seen.contains(&f) {
            continue;
        }
        let Some(set) = m.solid_of(f) else { continue };
        seen.extend(set.iter().copied());
        solids.push(set);
    }
    let [a, b] = solids.as_slice() else {
        return Err(Problem::Count(solids.len()));
    };
    let (mut first, mut second) = (a.clone(), b.clone());
    // The older solid (the lower face id) is the first one.
    if second[0] < first[0] {
        std::mem::swap(&mut first, &mut second);
    }
    if swap {
        std::mem::swap(&mut first, &mut second);
    }
    Ok(Operands {
        model,
        first,
        second,
    })
}

/// A volume in m³ for the status line.
fn cubic_metres(mm3: f64) -> String {
    kadakas_units::num(mm3 / 1e9, 3)
}

/// What an operation would do, in the language of the window, or why it cannot. The volumes come
/// from the operation itself, run on a copy.
pub fn preview(doc: &Document, ops: &Operands, op: SolidOp) -> Result<String, Problem> {
    preview_with(doc, ops, op, false)
}

/// Like [`preview`]. With `snap_near` (MOD-19) a near-touch is snapped on the copy, and the text
/// says how many vertices were moved and how far.
pub fn preview_with(
    doc: &Document,
    ops: &Operands,
    op: SolidOp,
    snap_near: bool,
) -> Result<String, Problem> {
    let m = model_of(doc, ops.model).ok_or(Problem::NoSelection)?;
    let run = m
        .solid_run(op, &ops.first, &ops.second, snap_near)
        .map_err(|e| match &e {
            SurfaceError::Solid(SolidError::NearTouch { .. }) => {
                Problem::NearTouch(localize_error(&e))
            }
            _ => Problem::Refused(localize_error(&e)),
        })?;
    let v = run.volumes;
    let vol = |i: usize| cubic_metres(v[i]);
    let done = match op {
        SolidOp::Union => tr!("uic-solid-done-union", volume = vol(0)),
        SolidOp::Subtract => tr!("uic-solid-done-subtract", volume = vol(0)),
        SolidOp::Intersect => tr!("uic-solid-done-intersect", volume = vol(0)),
        SolidOp::Trim => tr!("uic-solid-done-trim", volume = vol(0)),
        SolidOp::Split => tr!(
            "uic-solid-done-split",
            first = vol(0),
            common = vol(1),
            second = vol(2)
        ),
    };
    if run.snapped.is_empty() {
        return Ok(done);
    }
    Ok(tr!(
        "uic-solid-snapped",
        count = run.snapped.len(),
        distance = kadakas_units::num(
            run.snapped.iter().map(|m| m.distance).fold(0.0, f64::max),
            4
        ),
        done = done
    ))
}

/// The preview of an operation on the selection: the text for the status line, or why it cannot
/// run. It runs the operation on a copy, so a caller that asks again for the same model and
/// selection keeps the answer and gives it to [`out_with`].
pub fn preview_result(
    doc: &Document,
    sub: &SubSelection,
    swap: bool,
    op: SolidOp,
) -> Result<String, Problem> {
    operands(doc, sub, swap).and_then(|o| preview(doc, &o, op))
}

/// The preview as one text for the status line (the text of the refusal when it cannot run).
pub fn preview_text(doc: &Document, sub: &SubSelection, swap: bool, op: SolidOp) -> String {
    match preview_result(doc, sub, swap, op) {
        Ok(t) | Err(Problem::Refused(t)) => t,
        Err(p) => p.text(),
    }
}

/// The step that runs an operation on the selection: one `edit_surface` edit, so one undo step.
/// The message is the preview, shown when the step is done.
pub fn out(doc: &Document, sub: &SubSelection, swap: bool, op: SolidOp) -> Result<Out, Problem> {
    out_with(doc, sub, swap, op, None)
}

/// Like [`out`], with a preview that the caller has (from [`preview_result`] for the same model,
/// selection, order and operation), so the operation is not run a second time to make the message.
pub fn out_with(
    doc: &Document,
    sub: &SubSelection,
    swap: bool,
    op: SolidOp,
    known: Option<Result<String, Problem>>,
) -> Result<Out, Problem> {
    let ops = operands(doc, sub, swap)?;
    let message = match known {
        Some(r) => r?,
        None => preview(doc, &ops, op)?,
    };
    Ok(step(ops, op, message, false))
}

/// The step of "Snap and retry" (MOD-19): the operation with the snap allowed, as one edit, so one
/// undo step together with the move of the vertices. The message says what was moved.
pub fn out_snapped(
    doc: &Document,
    sub: &SubSelection,
    swap: bool,
    op: SolidOp,
) -> Result<Out, Problem> {
    let ops = operands(doc, sub, swap)?;
    let message = preview_with(doc, &ops, op, true)?;
    Ok(step(ops, op, message, true))
}

fn step(ops: Operands, op: SolidOp, message: String, snap_near: bool) -> Out {
    Out {
        actions: vec![Action::Edit {
            model: ops.model,
            edit: kadakas_core::surface::Edit::Solid {
                op,
                first: ops.first,
                second: ops.second,
                snap_near,
            },
        }],
        message: Some(message),
        ..Out::default()
    }
}

/// The result of Check Solid.
#[derive(Clone, Debug, PartialEq)]
pub struct Check {
    /// The text for the status line.
    pub text: String,
    /// The edges that make the models open (and the loose edges), to be selected.
    pub open: Vec<Item>,
    /// True if every solid is closed and nothing is loose.
    pub closed: bool,
}

/// The models that Check Solid looks at: those of the selected faces and edges, else the selected
/// objects that are surface models.
fn checked_models(doc: &Document, sub: &SubSelection, objects: &[ObjectId]) -> Vec<ObjectId> {
    let mut ids: BTreeSet<ObjectId> = sub.items().map(Item::model).collect();
    if ids.is_empty() {
        ids.extend(objects.iter().copied());
    }
    ids.into_iter()
        .filter(|id| model_of(doc, *id).is_some())
        .collect()
}

/// Check Solid (MOD-09): is every part of the model a closed solid? The text says how many solids
/// and how big, or how many edges are open; those edges come back as items to select.
pub fn check(doc: &Document, sub: &SubSelection, objects: &[ObjectId]) -> Check {
    let models = checked_models(doc, sub, objects);
    let (mut solids, mut volume, mut open_edges) = (0usize, 0.0, 0usize);
    let mut open: Vec<Item> = Vec::new();
    let mut faces = 0usize;
    let mut other = 0usize;
    for id in &models {
        let Some(m) = model_of(doc, *id) else {
            continue;
        };
        faces += m.face_count();
        let r: kadakas_core::surface::SolidReport = m.solid_report();
        for s in &r.shells {
            match s.state {
                ShellState::Solid => {
                    solids += 1;
                    volume += s.volume;
                }
                ShellState::Inverted | ShellState::Flat | ShellState::SelfCrossing => other += 1,
                ShellState::Open => {}
            }
        }
        let edges = r.open_edges();
        open_edges += edges.len() + r.wire_edges.len();
        open.extend(edges.iter().map(|o| Item::Edge(*id, o.edge)));
        open.extend(r.wire_edges.iter().map(|e| Item::Edge(*id, *e)));
    }
    open.sort();
    open.dedup();
    if models.is_empty() || faces == 0 {
        return Check {
            text: tr!("uic-solid-check-nothing"),
            open,
            closed: false,
        };
    }
    if open_edges == 0 && other == 0 {
        return Check {
            text: tr!(
                "uic-solid-check-closed",
                solids = solids,
                volume = cubic_metres(volume)
            ),
            open,
            closed: true,
        };
    }
    let text = if open_edges > 0 {
        tr!("uic-solid-check-open", edges = open_edges, solids = solids)
    } else {
        tr!("uic-solid-check-odd", count = other, solids = solids)
    };
    Check {
        text,
        open,
        closed: false,
    }
}
