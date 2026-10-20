//! View history: the views of the plan that you looked at, with Previous View and Next View.
//!
//! A view is the pan and zoom of the plan. A change counts once the view has stopped moving for
//! `SETTLE` seconds, so a mouse wheel turn or a drag is one entry and not hundreds. Going back and
//! forth does not add entries. A new view after going back removes the views that were ahead (like
//! the history of a web browser).

use glam::DVec2;

/// Seconds that the view must rest before it counts as a new view.
pub const SETTLE: f64 = 0.4;
/// The most views that are kept.
pub const MAX_ENTRIES: usize = 100;

/// Pan and zoom of the plan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot {
    pub origin: DVec2,
    pub zoom: f64,
}

/// The history of views. `S` is what a view is: the pan and zoom of the plan by default; the 3D
/// modeler keeps the camera (`kadakas_render::Camera`, see `nav3d::Nav3d`).
#[derive(Debug)]
pub struct ViewHistory<S = Snapshot> {
    entries: Vec<S>,
    /// The entry that is shown now.
    pos: usize,
    /// The view in the last frame, and since when it has not changed.
    seen: Option<(S, f64)>,
}

impl<S> Default for ViewHistory<S> {
    fn default() -> Self {
        ViewHistory {
            entries: Vec::new(),
            pos: 0,
            seen: None,
        }
    }
}

impl<S: Copy + PartialEq> ViewHistory<S> {
    /// Call once per frame with the time in seconds and the current view. Returns true while a
    /// change waits to settle, so the caller can ask for a repaint after `SETTLE`.
    pub fn observe(&mut self, now: f64, view: S) -> bool {
        match self.seen {
            Some((s, _)) if s == view => {}
            _ => self.seen = Some((view, now)),
        }
        let (_, since) = self.seen.expect("set above");
        if self.entries.get(self.pos) == Some(&view) {
            return false;
        }
        if now - since >= SETTLE || self.entries.is_empty() {
            self.push(view);
            return false;
        }
        true
    }

    fn push(&mut self, view: S) {
        self.entries
            .truncate(self.pos + usize::from(!self.entries.is_empty()));
        self.entries.push(view);
        if self.entries.len() > MAX_ENTRIES {
            self.entries.remove(0);
        }
        self.pos = self.entries.len() - 1;
    }

    /// Counts the current view at once, even if it has not rested yet. Call it before going
    /// back, so that the view you leave can be reached again with Next View.
    pub fn flush(&mut self, view: S) {
        if self.entries.get(self.pos) != Some(&view) {
            self.push(view);
        }
    }

    /// The view before the current one, which then becomes the current view.
    pub fn previous(&mut self) -> Option<S> {
        (self.pos > 0).then(|| {
            self.pos -= 1;
            self.entries[self.pos]
        })
    }

    /// The view after the current one.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<S> {
        (self.pos + 1 < self.entries.len()).then(|| {
            self.pos += 1;
            self.entries[self.pos]
        })
    }

    pub fn can_go_back(&self) -> bool {
        self.pos > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.pos + 1 < self.entries.len()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Changes every view of the history (and the one that is waiting to settle) with `f`, for a
    /// change of the coordinates of the scene. The history keeps its entries and its place, and
    /// the change is not an entry of its own.
    pub fn map_all(&mut self, f: impl Fn(S) -> S) {
        for e in &mut self.entries {
            *e = f(*e);
        }
        if let Some((s, _)) = &mut self.seen {
            *s = f(*s);
        }
    }

    /// Forget everything (a new document).
    pub fn clear(&mut self) {
        *self = ViewHistory::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(zoom: f64) -> Snapshot {
        Snapshot {
            origin: DVec2::new(10.0, 20.0),
            zoom,
        }
    }

    /// A view that rests for a while.
    fn rest(h: &mut ViewHistory, t: &mut f64, view: Snapshot) {
        h.observe(*t, view);
        *t += SETTLE + 0.1;
        h.observe(*t, view);
    }

    #[test]
    fn views_count_when_they_rest() {
        let mut h = ViewHistory::default();
        let mut t = 0.0;
        rest(&mut h, &mut t, v(1.0));
        rest(&mut h, &mut t, v(2.0));
        rest(&mut h, &mut t, v(4.0));
        assert_eq!(h.len(), 3);
        assert_eq!(h.previous(), Some(v(2.0)));
        assert_eq!(h.previous(), Some(v(1.0)));
        assert_eq!(h.previous(), None);
        assert_eq!(h.next(), Some(v(2.0)));
        assert_eq!(h.next(), Some(v(4.0)));
        assert_eq!(h.next(), None);
        assert!(h.can_go_back() && !h.can_go_forward());
    }

    #[test]
    fn a_moving_view_is_one_entry() {
        let mut h = ViewHistory::default();
        let mut t = 0.0;
        rest(&mut h, &mut t, v(1.0));
        // A wheel turn: the zoom changes every frame.
        for i in 1..31 {
            t += 0.016;
            let waiting = h.observe(t, v(1.0 + i as f64 * 0.1));
            assert!(waiting, "the change is still settling");
        }
        assert_eq!(h.len(), 1);
        t += SETTLE + 0.01;
        assert!(!h.observe(t, v(1.0 + 30.0 * 0.1)));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn going_back_adds_nothing_and_a_new_view_cuts_the_future() {
        let mut h = ViewHistory::default();
        let mut t = 0.0;
        for z in [1.0, 2.0, 3.0] {
            rest(&mut h, &mut t, v(z));
        }
        let back = h.previous().unwrap();
        // The app shows the view; the next frames see it and do not add an entry.
        rest(&mut h, &mut t, back);
        assert_eq!(h.len(), 3);
        assert!(h.can_go_forward());
        // A new view from here removes the view that was ahead.
        rest(&mut h, &mut t, v(9.0));
        assert_eq!(h.len(), 3);
        assert!(!h.can_go_forward());
        assert_eq!(h.previous(), Some(v(2.0)));
    }

    #[test]
    fn the_history_has_a_limit_and_can_be_cleared() {
        let mut h = ViewHistory::default();
        let mut t = 0.0;
        for i in 0..(MAX_ENTRIES + 20) {
            rest(&mut h, &mut t, v(1.0 + i as f64));
        }
        assert_eq!(h.len(), MAX_ENTRIES);
        assert!(h.can_go_back());
        h.clear();
        assert!(h.is_empty() && !h.can_go_back());
        assert_eq!(h.previous(), None);
        assert_eq!(h.next(), None);
    }

    #[test]
    fn flush_keeps_the_view_you_leave() {
        let mut h = ViewHistory::default();
        let mut t = 0.0;
        rest(&mut h, &mut t, v(1.0));
        // The user zooms and goes back before the new view has rested.
        h.observe(t + 0.01, v(2.0));
        h.flush(v(2.0));
        assert_eq!(h.previous(), Some(v(1.0)));
        assert_eq!(h.next(), Some(v(2.0)));
        h.flush(v(2.0));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn the_first_view_counts_at_once() {
        let mut h = ViewHistory::default();
        assert!(!h.observe(0.0, v(1.0)));
        assert_eq!(h.len(), 1);
    }
}
