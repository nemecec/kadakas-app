//! The state of the Activity panel (AGR-06) and of the highlight of agent changes (AGR-05).
//! Logic only, without egui.
//!
//! The panel shows the steps of the activity log and the changes of the steps that the person has
//! not marked as seen. This module keeps what the person chose (which step, which change, whether
//! the changes are highlighted), makes the [`ReviewSet`] only when the log, the document or the
//! choice changed, and moves through the changes with a key.

use std::sync::Arc;

use kadakas_core::activity::ActivityLog;
use kadakas_core::model::{Document, ObjectId};
use kadakas_review::overlay::{CompareOverlay, CompareStyle, GhostReport, UnchangedMode};

use crate::agent_review::{ReviewSet, Scope};

/// What the review was made for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    generation: u64,
    revision: u64,
    scope: Scope,
    dark: bool,
}

/// The review with what the plan draws of it.
pub struct Review {
    pub set: ReviewSet,
    pub overlay: CompareOverlay,
    pub style: CompareStyle,
    /// The ghosts of removed objects and the old shapes of changed ones.
    pub ghosts: Vec<(kadakas_core::model::Object, &'static str)>,
    pub report: GhostReport,
}

/// What the person chose in the panel.
pub struct ActivityModel {
    /// The window is open.
    pub open: bool,
    /// The changes are highlighted in the plan and in 3D.
    pub show: bool,
    /// The step the person chose in the list: the review then shows this step only.
    pub selected: Option<u64>,
    /// The change that the keys stepped to last.
    pub cursor: Option<ObjectId>,
    /// The object whose before and after the panel shows.
    pub detail: Option<ObjectId>,
    made: Option<(Key, Arc<Review>)>,
}

impl Default for ActivityModel {
    fn default() -> Self {
        ActivityModel {
            open: false,
            show: true,
            selected: None,
            cursor: None,
            detail: None,
            made: None,
        }
    }
}

/// What to mark as seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeenTarget {
    /// Every step.
    All,
    /// One step.
    Step(u64),
}

impl ActivityModel {
    pub fn scope(&self) -> Scope {
        match self.selected {
            Some(id) => Scope::Step(id),
            None => Scope::Unseen,
        }
    }

    /// A click on a step: it becomes the scope of the review; a second click goes back to all
    /// unseen steps.
    pub fn toggle_step(&mut self, id: u64) {
        self.selected = if self.selected == Some(id) {
            None
        } else {
            Some(id)
        };
        self.cursor = None;
        self.detail = None;
    }

    /// The steps that are not seen.
    pub fn unseen(log: &ActivityLog) -> usize {
        log.steps().iter().filter(|s| !s.seen).count()
    }

    /// The review of the scope against the document, made again only when the log, the document,
    /// the scope or the theme changed.
    pub fn review(
        &mut self,
        log: &ActivityLog,
        doc: &Document,
        revision: u64,
        dark: bool,
    ) -> Arc<Review> {
        // A chosen step that left the log is no choice any more.
        if self.selected.is_some_and(|id| log.get(id).is_none()) {
            self.selected = None;
        }
        let key = Key {
            generation: log.generation(),
            revision,
            scope: self.scope(),
            dark,
        };
        if let Some((k, r)) = &self.made
            && *k == key
        {
            return r.clone();
        }
        let set = match key.scope {
            Scope::Unseen => ReviewSet::build(log.steps().iter().filter(|s| !s.seen), doc),
            Scope::Step(id) => ReviewSet::build(log.steps().iter().filter(|s| s.id == id), doc),
        };
        // Highlight, not a comparison: the objects that did not change keep their look.
        let style = CompareStyle {
            unchanged: UnchangedMode::Show,
            dark,
            ..CompareStyle::default()
        };
        let overlay = set.overlay();
        let found = kadakas_review::overlay::ghosts_in(doc, doc, &overlay, &style);
        let made = Arc::new(Review {
            set,
            overlay,
            style,
            ghosts: found.items,
            report: found.report,
        });
        self.made = Some((key, made.clone()));
        made
    }

    /// Marks steps as seen: their changes are no longer highlighted. Returns how many steps
    /// changed.
    pub fn mark_seen(&mut self, log: &mut ActivityLog, target: SeenTarget) -> usize {
        self.cursor = None;
        self.detail = None;
        match target {
            SeenTarget::All => {
                let last = log.last_id();
                log.set_seen(last, true)
            }
            SeenTarget::Step(id) => {
                let n = usize::from(log.set_seen_one(id, true));
                if self.selected == Some(id) {
                    self.selected = None;
                }
                n
            }
        }
    }

    /// Moves the cursor to the next (or previous) change of the review and returns it.
    pub fn step_cursor(&mut self, review: &Review, forward: bool) -> Option<ObjectId> {
        let next = if forward {
            review.set.next_after(self.cursor)
        } else {
            review.set.prev_before(self.cursor)
        };
        self.cursor = next;
        next
    }
}

/// The time of day (UTC) of a time in ms since 1970: hours, minutes, seconds. `None` for 0 (not
/// known).
pub fn time_hms(time_ms: u64) -> Option<(u32, u32, u32)> {
    if time_ms == 0 {
        return None;
    }
    let day = (time_ms / 1000) % 86_400;
    Some((
        (day / 3600) as u32,
        (day % 3600 / 60) as u32,
        (day % 60) as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;
    use kadakas_core::activity::Recorded;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::model::Shape;

    fn add(ed: &mut Editor, x: f64) {
        ed.execute(&Command::AddObject {
            shape: Shape::Line {
                a: DVec2::new(x, 0.0),
                b: DVec2::new(x + 1.0, 1.0),
            },
            layer: None,
            class: None,
            attrs: Default::default(),
        })
        .unwrap();
    }

    fn step(log: &mut ActivityLog, ed: &mut Editor, x: f64) -> u64 {
        log.touch(1, 0);
        let mark = ed.serial_mark();
        add(ed, x);
        let serials = ed.steps_since(mark);
        let data = ed.step_data(&serials).unwrap();
        log.record(Recorded {
            conn: 1,
            method: "execute".into(),
            time_ms: 0,
            revision: ed.revision(),
            serials,
            data,
        })
        .unwrap()
    }

    #[test]
    fn the_review_is_made_again_only_when_something_changed() {
        let mut ed = Editor::with_sequential_ids();
        let mut log = ActivityLog::default();
        let mut m = ActivityModel::default();
        step(&mut log, &mut ed, 0.0);
        let a = m.review(&log, ed.doc(), ed.revision(), false);
        let b = m.review(&log, ed.doc(), ed.revision(), false);
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.set.counts().added, 1);
        let c = m.review(&log, ed.doc(), ed.revision(), true);
        assert!(!Arc::ptr_eq(&a, &c), "the theme changes the colors");
        step(&mut log, &mut ed, 5.0);
        let d = m.review(&log, ed.doc(), ed.revision(), true);
        assert_eq!(d.set.counts().added, 2);
        // The highlight does not dim the rest of the drawing.
        assert_eq!(d.style.unchanged, UnchangedMode::Show);
    }

    #[test]
    fn choosing_a_step_shows_that_step_and_a_second_click_goes_back() {
        let mut ed = Editor::with_sequential_ids();
        let mut log = ActivityLog::default();
        let mut m = ActivityModel::default();
        let s1 = step(&mut log, &mut ed, 0.0);
        step(&mut log, &mut ed, 5.0);
        m.toggle_step(s1);
        assert_eq!(m.scope(), Scope::Step(s1));
        assert_eq!(
            m.review(&log, ed.doc(), ed.revision(), false)
                .set
                .counts()
                .added,
            1
        );
        m.toggle_step(s1);
        assert_eq!(m.scope(), Scope::Unseen);
        assert_eq!(
            m.review(&log, ed.doc(), ed.revision(), false)
                .set
                .counts()
                .added,
            2
        );
        // A step that left the log is not a choice any more.
        m.selected = Some(999);
        m.review(&log, ed.doc(), ed.revision(), false);
        assert_eq!(m.selected, None);
    }

    #[test]
    fn marking_seen_clears_the_highlight_for_all_or_for_one_step() {
        let mut ed = Editor::with_sequential_ids();
        let mut log = ActivityLog::default();
        let mut m = ActivityModel::default();
        let s1 = step(&mut log, &mut ed, 0.0);
        let s2 = step(&mut log, &mut ed, 5.0);
        let s3 = step(&mut log, &mut ed, 9.0);
        assert_eq!(ActivityModel::unseen(&log), 3);
        // Only the newest: the older steps stay unseen.
        assert_eq!(m.mark_seen(&mut log, SeenTarget::Step(s3)), 1);
        assert!(!log.get(s1).unwrap().seen && !log.get(s2).unwrap().seen);
        assert!(log.get(s3).unwrap().seen);
        assert_eq!(
            m.review(&log, ed.doc(), ed.revision(), false)
                .set
                .counts()
                .added,
            2
        );
        assert_eq!(m.mark_seen(&mut log, SeenTarget::All), 2);
        assert!(
            m.review(&log, ed.doc(), ed.revision(), false)
                .set
                .is_empty()
        );
        assert_eq!(ActivityModel::unseen(&log), 0);
        assert_eq!(m.mark_seen(&mut log, SeenTarget::All), 0);
    }

    #[test]
    fn the_cursor_walks_through_the_changes_and_wraps() {
        let mut ed = Editor::with_sequential_ids();
        let mut log = ActivityLog::default();
        let mut m = ActivityModel::default();
        step(&mut log, &mut ed, 0.0);
        step(&mut log, &mut ed, 5.0);
        let r = m.review(&log, ed.doc(), ed.revision(), false);
        let first = m.step_cursor(&r, true).unwrap();
        let second = m.step_cursor(&r, true).unwrap();
        assert_ne!(first, second);
        assert_eq!(m.step_cursor(&r, true), Some(first));
        assert_eq!(m.step_cursor(&r, false), Some(second));
    }

    #[test]
    fn the_time_of_day_is_utc() {
        assert_eq!(time_hms(0), None);
        assert_eq!(time_hms(1_700_000_000_000), Some((22, 13, 20)));
    }
}
