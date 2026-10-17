//! The Activity panel and the review of the changes of the agent in the window (AGR-05, AGR-06).
//! The drawing is in `agent_activity_ui`, the logic in `kadakas_ui_core::agent_activity` and
//! `kadakas_api` (`Session::revert_agent_step`). This file connects them to the window.

use glam::DVec2;
use kadakas_core::model::ObjectId;
use kadakas_l10n::tr;
use kadakas_ui_core::agent_activity::Review;
use kadakas_ui_core::agent_review::step_bounds;

use super::*;
use crate::agent_activity_ui::{Act, Hints, refusal_text, summary_text};

impl KadakasApp {
    /// What the plan draws of the review: the overlay of the compare, with the colors, patterns
    /// and marks of REV-03, without dimming the rest. `None` while the highlight is off.
    pub(super) fn activity_draw(&self) -> Option<crate::compare_draw::CompareDraw<'_>> {
        let r = self
            .activity_review
            .as_ref()
            .filter(|r| self.activity_ui.model.show && !r.set.is_empty())?;
        Some(crate::compare_draw::CompareDraw {
            overlay: &r.overlay,
            style: &r.style,
            ghosts: &r.ghosts,
            ghost_doc: None,
            report: r.report,
            focus: self.activity_ui.model.cursor,
        })
    }

    fn activity_hints(&self) -> Hints {
        Hints {
            open: self.keys.hint(Action::ViewAgentActivity),
            next: self.keys.hint(Action::ViewAgentChangeNext),
            previous: self.keys.hint(Action::ViewAgentChangePrevious),
            seen: self.keys.hint(Action::ViewAgentChangesSeen),
            toggle: self.keys.hint(Action::ViewAgentChangesToggle),
        }
    }

    /// One frame: makes the review (only when something changed), draws the panel and the
    /// notice, and does what the person asked.
    pub(super) fn activity_step(&mut self, ctx: &egui::Context) {
        let now_ms = ctx.input(|i| i.time) * 1000.0;
        let review = self.activity_ui.model.review(
            &self.session.activity,
            self.session.editor.doc(),
            self.session.editor.revision(),
            self.dark,
            now_ms,
        );
        if self.activity_ui.model.is_stale() {
            // The document is changing fast (a drag): the review is made again in a moment.
            ctx.request_repaint_after(std::time::Duration::from_millis(
                kadakas_ui_core::agent_activity::REBUILD_MS as u64 + 20,
            ));
        }
        self.activity_review = Some(review.clone());
        let hints = self.activity_hints();
        let mut acts = self
            .activity_ui
            .windows(ctx, &mut self.session, &review, &hints);
        if let Some(canvas) = self.canvas_rect {
            acts.extend(
                self.activity_ui
                    .notice(ctx, canvas, &self.session, &review, &hints),
            );
            acts.extend(self.activity_ui.hold_notice(ctx, canvas, &self.session));
        }
        for a in acts {
            self.activity_act(ctx, a);
        }
    }

    fn activity_zoom(&mut self, ctx: &egui::Context, bounds: Option<(DVec2, DVec2)>) {
        if let Some((lo, hi)) = bounds {
            crate::agent_view::zoom_to_box(&mut self.agent_env(ctx), lo, hi);
        }
    }

    /// Selects the objects that exist and zooms to them (removed ones: to where they were).
    fn activity_show(&mut self, ctx: &egui::Context, ids: &[ObjectId], review: &Review) {
        let doc = self.session.editor.doc();
        let bounds = review.set.bounds(doc, ids).or_else(|| {
            ids.iter().find_map(|id| {
                let (lo, hi) = doc.object_bounds(doc.object(*id)?);
                (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
            })
        });
        let here: Vec<ObjectId> = ids
            .iter()
            .copied()
            .filter(|id| doc.object(*id).is_some())
            .collect();
        if !here.is_empty() {
            self.session.selection = here;
        }
        self.activity_zoom(ctx, bounds);
    }

    /// Does what the person asked in the Activity panel or with a key.
    pub(super) fn activity_act(&mut self, ctx: &egui::Context, act: Act) {
        // The review of the last frame is what the person saw when they asked.
        let review = match self.activity_review.clone() {
            Some(r) => r,
            None => return,
        };
        match act {
            Act::Open => self.activity_ui.model.open = true,
            Act::ZoomStep(id) => {
                let bounds = self
                    .session
                    .activity
                    .get(id)
                    .and_then(|s| step_bounds(s, self.session.editor.doc()));
                self.activity_zoom(ctx, bounds);
            }
            Act::ZoomObjects(ids) => self.activity_show(ctx, &ids, &review),
            Act::ZoomChanges => {
                let ids: Vec<ObjectId> = review.set.entries().iter().map(|e| e.id).collect();
                let bounds = review.set.bounds(self.session.editor.doc(), &ids);
                self.activity_zoom(ctx, bounds);
            }
            Act::Next | Act::Previous => {
                let forward = act == Act::Next;
                match self.activity_ui.model.step_cursor(&review, forward) {
                    Some(id) => {
                        self.activity_show(ctx, &[id], &review);
                        if let Some(step) = review
                            .set
                            .get(id)
                            .and_then(|e| self.session.activity.get(e.step))
                        {
                            self.status = tr!(
                                "agent-activity-status-line",
                                id = step.id,
                                summary = summary_text(&step.summary)
                            );
                        }
                    }
                    None => self.status = tr!("agent-activity-none"),
                }
            }
            Act::Seen(target) => {
                self.activity_ui
                    .model
                    .mark_seen(&mut self.session.activity, target);
            }
            Act::Revert { step, skip } => self.activity_revert(step, skip),
            Act::UndoBack { step, expect } => self.activity_undo_back(step, expect),
            Act::RedoAll => {
                if self.read_only() {
                    self.status = tr!("compare-ui-read-only");
                } else {
                    while self.session.editor.redo() {}
                    self.session.prune_selection();
                }
            }
            Act::ReleaseHold => self.session.release_agent_hold(),
        }
    }

    fn activity_revert(&mut self, step: u64, skip: bool) {
        if self.read_only() {
            self.status = tr!("compare-ui-read-only");
            return;
        }
        let (summary, total) = self
            .session
            .activity
            .get(step)
            .map(|s| {
                (
                    summary_text(&s.summary),
                    s.data.as_ref().map_or(0, |d| d.objects.len()),
                )
            })
            .unwrap_or_default();
        match self.session.revert_agent_step(step, skip) {
            Ok(r) if r.skipped.is_empty() => {
                self.status = tr!("agent-activity-reverted", id = step, summary = summary);
            }
            Ok(r) => {
                self.status = tr!(
                    "agent-activity-reverted-some",
                    id = step,
                    done = r.reverted.len(),
                    total = total,
                    kept = r.skipped.len()
                );
            }
            Err(e) => {
                self.status = tr!("agent-activity-refused", id = step, why = refusal_text(&e));
            }
        }
    }

    fn activity_undo_back(&mut self, step: u64, expect: usize) {
        if self.read_only() {
            self.status = tr!("compare-ui-read-only");
            return;
        }
        // The question named a number of steps: if the history is another one now, ask again.
        let now = self.session.revert_check(step).map(|c| c.above + 1).ok();
        if now.is_some_and(|n| n != expect) {
            self.status = tr!("agent-activity-changed-meanwhile");
            return;
        }
        match self.session.undo_back_to_agent_step(step) {
            Ok(n) => self.status = tr!("agent-activity-undone-to", count = n, id = step),
            Err(e) => {
                self.status = tr!("agent-activity-refused", id = step, why = refusal_text(&e));
            }
        }
    }
}
