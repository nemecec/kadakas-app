//! The Activity panel (AGR-06) and the review of what the agent changed (AGR-05): drawing only.
//!
//! The logic is in `kadakas_ui_core::agent_activity` and `agent_review`, the log in
//! `kadakas_core::activity`, the checks of a revert in `kadakas_api` (`Session::revert_check`).
//! This file draws the window with the list of steps, the dialogs of a revert, the small notice
//! in the corner of the drawing, and the highlight of the changes in 3D (the plan draws them with
//! the overlay of the compare, REV-03).

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Ui, pos2, vec2};
use glam::{DVec3, Mat4};
use kadakas_api::{Advice, Session, StepCheck};
use kadakas_core::activity::{Step, Summary};
use kadakas_core::command::{Counted, Counts, StepPlace};
use kadakas_core::model::{Document, ObjectId};
use kadakas_l10n::tr;
use kadakas_review::overlay::Status;
use kadakas_ui_core::agent_activity::{ActivityModel, Review, SeenTarget, time_hms};
use kadakas_ui_core::agent_review::object_diff;

use crate::agent_view_ui::project_3d;

/// The most objects that a step lists in the panel.
const MAX_ROWS: usize = 100;
/// The most boxes that the 3D view draws for the changes in one frame.
pub const MAX_3D_BOXES: usize = 1500;

/// What the person asked for. The app does it (it owns the view, the session and the status).
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Zoom to what a step touched.
    ZoomStep(u64),
    /// Zoom to these objects (also removed ones: their old place) and select the ones that exist.
    ZoomObjects(Vec<ObjectId>),
    /// Zoom to all changes of the review.
    ZoomChanges,
    Next,
    Previous,
    Seen(SeenTarget),
    /// Revert one step; `skip` leaves the objects that cannot go back and reverts the rest.
    Revert {
        step: u64,
        skip: bool,
    },
    /// Undo this step and every step after it.
    UndoBack(u64),
    /// Open the panel (from the notice).
    Open,
}

enum Pending {
    Revert(u64, StepCheck),
    UndoBack(u64, StepCheck),
}

/// The window, its dialogs and the notice.
#[derive(Default)]
pub struct ActivityUi {
    pub model: ActivityModel,
    pending: Option<Pending>,
    /// The notice is hidden up to this step (the person closed it).
    notice_hidden_upto: u64,
    /// What a revert of each step needs, made again only when the log or the document changed
    /// (the check looks at every object of a step).
    checks: std::collections::HashMap<u64, StepCheck>,
    checks_key: Option<(u64, u64)>,
}

// ---------------------------------------------------------------- texts

fn plural_objects(n: usize) -> String {
    tr!("agent-activity-n-objects", count = n)
}

/// The summary of a step in the language of the person. It tells what `Summary::text` (the
/// English text of the API) tells.
pub fn summary_text(s: &Summary) -> String {
    let mut parts: Vec<String> = Vec::new();
    let o = s.objects;
    let single = |c: usize, others: usize| c > 0 && others == 0;
    let with_kinds = |what: String, show: bool| -> String {
        if !show || s.kinds.is_empty() {
            return what;
        }
        let mut k: Vec<String> = s
            .kinds
            .iter()
            .map(|(k, n)| format!("{n} {}", crate::enum_names::name("shape", k)))
            .collect();
        if s.others > 0 {
            k.push(tr!("agent-activity-sum-other", count = s.others));
        }
        tr!(
            "agent-activity-sum-kinds",
            what = what,
            kinds = k.join(", ")
        )
    };
    if o.added > 0 {
        let what = with_kinds(
            plural_objects(o.added),
            single(o.added, o.changed + o.removed),
        );
        parts.push(tr!("agent-activity-sum-added", what = what));
    }
    if o.changed > 0 {
        let what = with_kinds(
            plural_objects(o.changed),
            single(o.changed, o.added + o.removed),
        );
        parts.push(tr!("agent-activity-sum-changed", what = what));
    }
    if o.removed > 0 {
        let what = with_kinds(
            plural_objects(o.removed),
            single(o.removed, o.added + o.changed),
        );
        parts.push(tr!("agent-activity-sum-removed", what = what));
    }
    let named = !s.layers_added.is_empty() && s.layers.added == s.layers_added.len();
    for (c, noun) in [(s.layers, 0), (s.classes, 1), (s.resources, 2)] {
        let n = |k: usize| match noun {
            0 => tr!("agent-activity-n-layers", count = k),
            1 => tr!("agent-activity-n-classes", count = k),
            _ => tr!("agent-activity-n-resources", count = k),
        };
        if noun == 0 && named {
            parts.push(tr!(
                "agent-activity-sum-layer-named",
                names = s.layers_added.join(", ")
            ));
        } else if c.added > 0 {
            parts.push(tr!("agent-activity-sum-added", what = n(c.added)));
        }
        if c.changed > 0 {
            parts.push(tr!("agent-activity-sum-changed", what = n(c.changed)));
        }
        if c.removed > 0 {
            parts.push(tr!("agent-activity-sum-removed", what = n(c.removed)));
        }
    }
    if s.document {
        parts.push(tr!("agent-activity-sum-document"));
    }
    if s.versions {
        parts.push(tr!("agent-activity-sum-versions"));
    }
    if s.issues {
        parts.push(tr!("agent-activity-sum-issues"));
    }
    if parts.is_empty() {
        return capitalize(tr!("agent-activity-sum-none"));
    }
    capitalize(parts.join("; "))
}

fn capitalize(t: String) -> String {
    let mut chars = t.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// Why a revert is not possible, in the language of the person.
pub fn why_text(check: &StepCheck) -> Option<String> {
    Some(match check.place {
        StepPlace::Redo => tr!("agent-activity-why-undone"),
        StepPlace::Gone => tr!("agent-activity-why-gone"),
        StepPlace::Undo(0) if !check.shared => return None,
        StepPlace::Undo(_) if check.shared && check.snapshot_blocked => {
            tr!("agent-activity-why-shared")
        }
        StepPlace::Undo(_) if !check.details => tr!("agent-activity-why-dropped"),
        StepPlace::Undo(_) if check.snapshot_blocked => tr!("agent-activity-why-snapshot"),
        StepPlace::Undo(_) if !check.conflicts.is_empty() => {
            tr!(
                "agent-activity-why-conflicts",
                count = check.conflicts.len()
            )
        }
        StepPlace::Undo(_) => return None,
    })
}

/// The refusal of the session in the language of the person.
pub fn refusal_text(e: &kadakas_api::StepError) -> String {
    use kadakas_core::command::{ConflictKind, Refusal};
    match e {
        kadakas_api::StepError::Unknown => tr!("agent-activity-why-gone"),
        kadakas_api::StepError::DetailsDropped => tr!("agent-activity-why-dropped"),
        kadakas_api::StepError::Refused(r) => match r {
            Refusal::Gone => tr!("agent-activity-why-gone"),
            Refusal::AlreadyUndone => tr!("agent-activity-why-undone"),
            Refusal::Snapshot { .. } => tr!("agent-activity-why-snapshot"),
            Refusal::Conflicts(list) => {
                if list.iter().all(|c| c.kind == ConflictKind::LayerLocked) {
                    tr!("agent-activity-why-locked")
                } else {
                    tr!("agent-activity-why-conflicts", count = list.len())
                }
            }
            Refusal::Shared => tr!("agent-activity-why-shared"),
            Refusal::InUse(_) => tr!("agent-activity-why-group"),
            Refusal::Rejected(why) => tr!("agent-activity-why-other", why = why.clone()),
        },
    }
}

fn status_color(review: &Review, status: Status) -> Color32 {
    crate::compare_draw::status_color(&review.style, status)
}

fn status_name(status: Status) -> String {
    match status {
        Status::Added => tr!("agent-activity-status-added"),
        Status::Changed => tr!("agent-activity-status-changed"),
        Status::Removed => tr!("agent-activity-status-removed"),
    }
}

fn time_text(step: &Step) -> String {
    time_hms(step.time_ms).map_or_else(String::new, |(h, m, s)| format!("{h:02}:{m:02}:{s:02}"))
}

/// The counts of a review as three short lines with the swatches of the compare.
fn counts_row(ui: &mut Ui, review: &Review, c: Counts) {
    ui.horizontal_wrapped(|ui| {
        for (status, n) in [
            (Status::Added, c.added),
            (Status::Changed, c.changed),
            (Status::Removed, c.removed),
        ] {
            if n == 0 {
                continue;
            }
            crate::compare_ui::swatch(ui, &review.style, status);
            ui.label(match status {
                Status::Added => tr!("compare-ui-legend-added", count = n),
                Status::Changed => tr!("compare-ui-legend-changed", count = n),
                Status::Removed => tr!("compare-ui-legend-removed", count = n),
            });
        }
    });
}

// ---------------------------------------------------------------- the window

impl ActivityUi {
    /// The window and its dialogs. Gives what the person asked for.
    pub fn windows(
        &mut self,
        ctx: &egui::Context,
        session: &Session,
        review: &Review,
        hints: &Hints,
    ) -> Vec<Act> {
        let mut acts = Vec::new();
        if self.model.open {
            let key = (session.activity.generation(), session.editor.revision());
            if self.checks_key != Some(key) {
                self.checks = session
                    .activity
                    .steps()
                    .iter()
                    .filter_map(|s| Some((s.id, session.revert_check(s.id).ok()?)))
                    .collect();
                self.checks_key = Some(key);
            }
            let mut open = true;
            egui::Window::new(tr!("agent-activity-title"))
                .id(egui::Id::new("win-agent-activity"))
                .open(&mut open)
                .default_width(440.0)
                .default_height(460.0)
                .show(ctx, |ui| self.body(ui, session, review, hints, &mut acts));
            self.model.open = open;
        }
        self.dialog(ctx, &mut acts);
        acts
    }

    fn body(
        &mut self,
        ui: &mut Ui,
        session: &Session,
        review: &Review,
        hints: &Hints,
        acts: &mut Vec<Act>,
    ) {
        ui.small(tr!("agent-activity-intro"));
        let log = &session.activity;
        if log.is_empty() {
            ui.separator();
            ui.label(tr!("agent-activity-empty"));
            return;
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.model.show, tr!("agent-activity-show"))
                .on_hover_text(tr!("agent-activity-show-hint", key = hints.toggle.clone()));
        });
        // The review: what is highlighted now.
        let counts = review.set.counts();
        if review.set.is_empty() {
            ui.weak(tr!("agent-activity-none"));
        } else {
            counts_row(ui, review, counts);
        }
        ui.horizontal_wrapped(|ui| {
            match self.model.selected {
                Some(id) => {
                    ui.weak(tr!("agent-activity-scope-step", step = id));
                    if ui.small_button(tr!("agent-activity-scope-all")).clicked() {
                        self.model.selected = None;
                    }
                }
                None => {
                    ui.weak(tr!(
                        "agent-activity-scope-unseen",
                        count = ActivityModel::unseen(log)
                    ));
                }
            };
        });
        ui.horizontal_wrapped(|ui| {
            let has = !review.set.is_empty();
            if ui
                .add_enabled(has, egui::Button::new(tr!("agent-activity-prev-change")))
                .on_hover_text(tr!(
                    "agent-activity-prev-change-hint",
                    key = hints.previous.clone()
                ))
                .clicked()
            {
                acts.push(Act::Previous);
            }
            if ui
                .add_enabled(has, egui::Button::new(tr!("agent-activity-next-change")))
                .on_hover_text(tr!(
                    "agent-activity-next-change-hint",
                    key = hints.next.clone()
                ))
                .clicked()
            {
                acts.push(Act::Next);
            }
            if ui
                .add_enabled(has, egui::Button::new(tr!("agent-activity-zoom-all")))
                .clicked()
            {
                acts.push(Act::ZoomChanges);
            }
            if ui
                .add_enabled(
                    ActivityModel::unseen(log) > 0,
                    egui::Button::new(tr!("agent-activity-mark-all")),
                )
                .on_hover_text(tr!(
                    "agent-activity-mark-all-hint",
                    key = hints.seen.clone()
                ))
                .clicked()
            {
                acts.push(Act::Seen(SeenTarget::All));
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .max_height(ui.available_height().max(180.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let mut last_session = None;
                for step in log.steps().iter().rev() {
                    if last_session != Some(step.session) {
                        last_session = Some(step.session);
                        self.session_row(ui, session, step);
                    }
                    self.step_row(ui, session, review, step, acts);
                }
            });
    }

    fn session_row(&self, ui: &mut Ui, session: &Session, step: &Step) {
        let mut text = tr!("agent-activity-session", session = step.session);
        if let Some(v) = session.activity.session_version(step.session) {
            text = format!(
                "{text}. {}",
                tr!("agent-activity-session-version", version = v)
            );
        } else if let Some(why) = session.activity.version_note(step.session) {
            text = format!(
                "{text}. {}",
                tr!("agent-activity-session-no-version", why = why.to_string())
            );
        }
        ui.add_space(4.0);
        ui.weak(text);
    }

    fn step_row(
        &mut self,
        ui: &mut Ui,
        session: &Session,
        review: &Review,
        step: &Step,
        acts: &mut Vec<Act>,
    ) {
        let check = self.checks.get(&step.id).cloned();
        let place = check.as_ref().map_or(StepPlace::Gone, |c| c.place);
        let chosen = self.model.selected == Some(step.id);
        egui::Frame::group(ui.style())
            .fill(if chosen {
                ui.visuals().selection.bg_fill.gamma_multiply(0.35)
            } else {
                Color32::TRANSPARENT
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    let head = format!(
                        "{}  {}  {}",
                        tr!("agent-activity-step", id = step.id),
                        time_text(step),
                        step.method
                    );
                    let r = ui
                        .selectable_label(chosen, egui::RichText::new(head).strong())
                        .on_hover_text(tr!("agent-activity-scope-step", step = step.id));
                    if r.clicked() {
                        self.model.toggle_step(step.id);
                    }
                    if step.seen {
                        ui.weak(tr!("agent-activity-state-seen"));
                    }
                    match place {
                        StepPlace::Redo => {
                            ui.weak(tr!("agent-activity-state-undone"));
                        }
                        StepPlace::Gone => {
                            ui.weak(tr!("agent-activity-state-gone"));
                        }
                        StepPlace::Undo(_) => {}
                    }
                    if step.data.is_none() {
                        ui.weak(tr!("agent-activity-state-dropped"));
                    }
                });
                ui.label(summary_text(&step.summary));
                ui.horizontal_wrapped(|ui| {
                    let live = matches!(place, StepPlace::Undo(_));
                    if ui
                        .add_enabled(
                            step.data.is_some(),
                            egui::Button::new(tr!("agent-activity-zoom")),
                        )
                        .on_hover_text(tr!("agent-activity-zoom-hint"))
                        .clicked()
                    {
                        acts.push(Act::ZoomStep(step.id));
                    }
                    if ui
                        .add_enabled(
                            !step.seen,
                            egui::Button::new(tr!("agent-activity-mark-one")),
                        )
                        .on_hover_text(tr!("agent-activity-mark-one-hint"))
                        .clicked()
                    {
                        acts.push(Act::Seen(SeenTarget::Step(step.id)));
                    }
                    if let Some(check) = &check {
                        let advice = check.advice();
                        let reason = why_text(check);
                        let can_revert =
                            advice != Advice::Nothing && advice != Advice::UndoBackOnly;
                        let mut b = ui.add_enabled(
                            live && (can_revert || advice == Advice::UndoBackOnly),
                            egui::Button::new(tr!("agent-activity-revert")),
                        );
                        b = b.on_hover_text(tr!("agent-activity-revert-hint"));
                        if let Some(r) = &reason {
                            b = b.on_disabled_hover_text(r.clone()).on_hover_text(r.clone());
                        }
                        if b.clicked() {
                            match advice {
                                Advice::Undo | Advice::Revert => {
                                    acts.push(Act::Revert {
                                        step: step.id,
                                        skip: false,
                                    });
                                }
                                Advice::Partial { .. } => {
                                    self.pending = Some(Pending::Revert(step.id, check.clone()));
                                }
                                Advice::UndoBackOnly => {
                                    self.pending = Some(Pending::UndoBack(step.id, check.clone()));
                                }
                                Advice::Nothing => {}
                            }
                        }
                        let mut b = ui
                            .add_enabled(
                                live && !check.shared,
                                egui::Button::new(tr!("agent-activity-undo-to")),
                            )
                            .on_hover_text(tr!("agent-activity-undo-to-hint"));
                        if check.shared {
                            b = b.on_disabled_hover_text(tr!("agent-activity-why-shared"));
                        }
                        if b.clicked() {
                            if check.above_other > 0 {
                                self.pending = Some(Pending::UndoBack(step.id, check.clone()));
                            } else {
                                acts.push(Act::UndoBack(step.id));
                            }
                        }
                    }
                });
                if chosen {
                    self.objects(ui, session.editor.doc(), review, step, acts);
                }
            });
    }

    /// The objects of the chosen step, with the before and after of the one the person clicked.
    fn objects(
        &mut self,
        ui: &mut Ui,
        doc: &Document,
        review: &Review,
        step: &Step,
        acts: &mut Vec<Act>,
    ) {
        let Some(data) = &step.data else { return };
        ui.separator();
        ui.small(tr!("agent-activity-objects"));
        for c in data.objects.iter().take(MAX_ROWS) {
            let (status, mark) = match c.kind() {
                Counted::Added => (Status::Added, '+'),
                Counted::Changed => (Status::Changed, '~'),
                Counted::Removed => (Status::Removed, '-'),
            };
            let kind = c
                .after
                .as_ref()
                .or(c.before.as_ref())
                .map_or("", |o| o.shape.kind());
            let short: String = c.id.0.to_string().chars().take(8).collect();
            let text = tr!(
                "agent-activity-object-row",
                mark = mark.to_string(),
                kind = crate::enum_names::name("shape", kind),
                id = short
            );
            let here = doc.object(c.id).is_some();
            ui.horizontal(|ui| {
                let r = ui
                    .selectable_label(
                        self.model.detail == Some(c.id),
                        egui::RichText::new(text).color(status_color(review, status)),
                    )
                    .on_hover_text(status_name(status));
                if r.clicked() {
                    self.model.detail = Some(c.id);
                    acts.push(Act::ZoomObjects(vec![c.id]));
                }
                if !here {
                    ui.weak(tr!("agent-activity-object-gone"));
                }
            });
            if self.model.detail == Some(c.id) {
                self.diff(ui, doc, c);
            }
        }
        if data.objects.len() > MAX_ROWS {
            ui.weak(tr!(
                "agent-activity-objects-more",
                count = data.objects.len() - MAX_ROWS
            ));
        }
    }

    /// Before and after of one object, as a list of the properties that differ.
    fn diff(&self, ui: &mut Ui, doc: &Document, c: &kadakas_core::command::ObjChange) {
        let layer_name = |o: &kadakas_core::model::Object| {
            doc.layer(o.layer)
                .map_or_else(String::new, |l| l.name.clone())
        };
        ui.indent("agent-activity-diff", |ui| match (&c.before, &c.after) {
            (None, Some(a)) => {
                ui.small(tr!(
                    "agent-activity-new-object",
                    kind = crate::enum_names::name("shape", a.shape.kind()),
                    layer = layer_name(a)
                ));
            }
            (Some(b), None) => {
                ui.small(tr!(
                    "agent-activity-removed-object",
                    kind = crate::enum_names::name("shape", b.shape.kind()),
                    layer = layer_name(b)
                ));
            }
            (Some(b), Some(a)) => {
                ui.small(format!(
                    "{} → {}",
                    tr!("agent-activity-before"),
                    tr!("agent-activity-after")
                ));
                let diff = object_diff(b, a);
                for p in diff.iter() {
                    ui.small(tr!(
                        "agent-activity-prop",
                        path = p.path.clone(),
                        old = p
                            .old
                            .clone()
                            .unwrap_or_else(|| tr!("agent-activity-prop-none")),
                        new = p
                            .new
                            .clone()
                            .unwrap_or_else(|| tr!("agent-activity-prop-none"))
                    ));
                }
                if diff.len() >= kadakas_ui_core::agent_review::MAX_PROPS {
                    ui.weak(tr!("agent-activity-prop-more"));
                }
            }
            (None, None) => {}
        });
    }

    /// The question before a revert that would not be a plain undo.
    fn dialog(&mut self, ctx: &egui::Context, acts: &mut Vec<Act>) {
        let Some(p) = &self.pending else { return };
        let mut done = false;
        let mut open = true;
        match p {
            Pending::Revert(step, check) => {
                let (free, blocked) = (check.free, check.conflicts.len());
                egui::Window::new(tr!("agent-activity-revert-title", id = *step))
                    .id(egui::Id::new("win-agent-activity-revert"))
                    .open(&mut open)
                    .collapsible(false)
                    .show(ctx, |ui| {
                        ui.label(tr!(
                            "agent-activity-revert-partial",
                            blocked = blocked,
                            total = free + blocked + check.already_gone
                        ));
                        ui.horizontal(|ui| {
                            if ui
                                .button(tr!("agent-activity-revert-rest", free = free))
                                .clicked()
                            {
                                acts.push(Act::Revert {
                                    step: *step,
                                    skip: true,
                                });
                                done = true;
                            }
                            if ui.button(tr!("agent-activity-cancel")).clicked() {
                                done = true;
                            }
                        });
                    });
            }
            Pending::UndoBack(step, check) => {
                egui::Window::new(tr!("agent-activity-undo-title", id = *step))
                    .id(egui::Id::new("win-agent-activity-undo"))
                    .open(&mut open)
                    .collapsible(false)
                    .show(ctx, |ui| {
                        if check.snapshot_blocked || !check.details {
                            ui.label(tr!("agent-activity-undo-snapshot"));
                        }
                        ui.label(tr!("agent-activity-undo-body", count = check.above + 1));
                        if check.above_other > 0 {
                            ui.label(tr!("agent-activity-undo-others", other = check.above_other));
                        }
                        ui.horizontal(|ui| {
                            if ui.button(tr!("agent-activity-undo-go")).clicked() {
                                acts.push(Act::UndoBack(*step));
                                done = true;
                            }
                            if ui.button(tr!("agent-activity-cancel")).clicked() {
                                done = true;
                            }
                        });
                    });
            }
        }
        if done || !open {
            self.pending = None;
        }
    }

    /// The notice in the corner of the drawing while the panel is closed and there are changes
    /// that the person has not seen.
    pub fn notice(
        &mut self,
        ctx: &egui::Context,
        canvas: Rect,
        session: &Session,
        review: &Review,
        hints: &Hints,
    ) -> Vec<Act> {
        let mut acts = Vec::new();
        let log = &session.activity;
        let unseen = ActivityModel::unseen(log);
        if self.model.open
            || unseen == 0
            || self.model.selected.is_some()
            || log.last_id() <= self.notice_hidden_upto
        {
            return acts;
        }
        let mut hide = false;
        egui::Area::new(egui::Id::new("agent-activity-notice"))
            .order(egui::Order::Middle)
            .fixed_pos(pos2(canvas.left() + 8.0, canvas.top() + 8.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(300.0);
                    ui.label(tr!("agent-activity-banner", count = unseen));
                    counts_row(ui, review, review.set.counts());
                    ui.horizontal(|ui| {
                        if ui
                            .button(tr!("agent-activity-review"))
                            .on_hover_text(tr!(
                                "agent-activity-review-hint",
                                key = hints.open.clone()
                            ))
                            .clicked()
                        {
                            acts.push(Act::Open);
                        }
                        if ui
                            .button(tr!("agent-activity-mark-all"))
                            .on_hover_text(tr!(
                                "agent-activity-mark-all-hint",
                                key = hints.seen.clone()
                            ))
                            .clicked()
                        {
                            acts.push(Act::Seen(SeenTarget::All));
                        }
                        if ui
                            .small_button(tr!("agent-activity-dismiss-banner"))
                            .clicked()
                        {
                            hide = true;
                        }
                    });
                });
            });
        if hide {
            self.notice_hidden_upto = log.last_id();
        }
        acts
    }
}

/// The keys that the texts name.
#[derive(Clone, Debug, Default)]
pub struct Hints {
    pub open: String,
    pub next: String,
    pub previous: String,
    pub seen: String,
    pub toggle: String,
}

// ---------------------------------------------------------------- 3D

/// A box on the screen for an object, from its bounds in drawing mm.
fn screen_box(project: &dyn Fn(DVec3) -> Option<Pos2>, lo: DVec3, hi: DVec3) -> Option<Rect> {
    let mut out: Option<Rect> = None;
    for i in 0..8 {
        let p = project(DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        ))?;
        let r = Rect::from_min_max(p, p);
        out = Some(out.map_or(r, |o| o.union(r)));
    }
    out
}

/// The changes of the review in the 3D view: a box with the pattern and the mark of its status
/// around each object (removed objects: where they were), and a label with the counts. Never part
/// of the drawing.
pub fn paint_3d(
    painter: &Painter,
    rect: Rect,
    vp: Mat4,
    scene: &kadakas_core::mesh::Scene,
    doc: &Document,
    review: &Review,
) {
    if review.set.is_empty() {
        return;
    }
    let painter = painter.with_clip_rect(rect);
    let origin = scene.origin;
    let project = move |p: DVec3| project_3d(vp, rect, (p - origin).as_vec3());
    let mut drawn = 0;
    for e in review.set.entries() {
        if drawn >= MAX_3D_BOXES {
            break;
        }
        let bounds = scene
            .object_bounds(e.id)
            .filter(|(lo, hi)| lo.is_finite() && hi.is_finite())
            .map(|(lo, hi)| (scene.to_world(lo), scene.to_world(hi)))
            .or_else(|| {
                let o = e.after.as_ref().or(e.before.as_ref())?;
                let (lo, hi) = doc.object_bounds(o);
                (lo.is_finite() && hi.is_finite()).then(|| (lo.extend(0.0), hi.extend(0.0)))
            });
        let Some((lo, hi)) = bounds else { continue };
        let Some(r) = screen_box(&project, lo, hi) else {
            continue;
        };
        let r = r.expand(2.0);
        if !r.intersects(rect) {
            continue;
        }
        drawn += 1;
        let color = status_color(review, e.status);
        let stroke = Stroke::new(2.0, color);
        let pts = [
            r.left_top(),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
            r.left_top(),
        ];
        match e.status {
            Status::Added => {
                painter.add(egui::Shape::line(pts.to_vec(), stroke));
            }
            Status::Removed => {
                painter.extend(egui::Shape::dashed_line(&pts, stroke, 9.0, 5.0));
            }
            Status::Changed => {
                painter.extend(egui::Shape::dashed_line(&pts, stroke, 2.0, 4.0));
            }
        }
        let symbol = match e.status {
            Status::Added => "+",
            Status::Changed => "~",
            Status::Removed => "-",
        };
        painter.text(
            r.left_top() + vec2(3.0, -1.0),
            Align2::LEFT_BOTTOM,
            symbol,
            FontId::proportional(13.0),
            color,
        );
    }
    let c = review.set.counts();
    let mut lines = Vec::new();
    if c.added > 0 {
        lines.push((
            Status::Added,
            tr!("agent-activity-3d-added", count = c.added),
        ));
    }
    if c.changed > 0 {
        lines.push((
            Status::Changed,
            tr!("agent-activity-3d-changed", count = c.changed),
        ));
    }
    if c.removed > 0 {
        lines.push((
            Status::Removed,
            tr!("agent-activity-3d-removed", count = c.removed),
        ));
    }
    let mut y = rect.right_top().y + 8.0;
    for (status, text) in lines {
        painter.text(
            pos2(rect.right() - 10.0, y),
            Align2::RIGHT_TOP,
            text,
            FontId::proportional(12.0),
            status_color(review, status),
        );
        y += 15.0;
    }
}
