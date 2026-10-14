//! The logic of the Compare window and the history slider (REV-03), without egui.
//!
//! The window shows the colored overlay of two sides of a drawing (versions, or a version and the
//! open drawing) and lists the changes. This module holds what a window needs and an egui-free
//! program can reuse:
//!
//! - [`HistorySlider`]: steps through the versions of one branch, newest to oldest, in two modes:
//!   each step compares neighbouring versions, or every step compares with one fixed base. It
//!   gives the pair of sides for a position, and the pairs next to it, so that a window can warm
//!   its cache.
//! - [`CompareForm`]: the two sides that the person picked.
//! - [`RowFilter`] and [`click_action`]: which rows of the list are shown, and what a click on a
//!   row does (select and zoom, or frame the ghost of a removed object).
//! - [`frame_box`]: the part of the model to show for an object.
//!
//! The texts are in the app (`compare-ui.ftl`).

use glam::DVec2;

use crate::versions::{Change, ChangeRow, Side, VersionList};

/// How the slider picks the two sides of a step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SliderMode {
    /// Each step compares a version with the one before it: what that version changed.
    #[default]
    Neighbours,
    /// Each step compares a version with one fixed base: what changed since the base.
    FixedBase,
}

/// Steps through the versions of a branch. Position 0 is the newest step.
///
/// The sides, newest first, are the open drawing (when it differs from the newest version) and
/// the versions of the branch from the newest to the oldest. In `Neighbours` mode a position `k`
/// compares side `k + 1` (older) with side `k` (newer). In `FixedBase` mode the base is one of
/// the sides, and a position `k` compares the base with side `k`, for the sides that are newer
/// than the base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistorySlider {
    steps: Vec<Side>,
    mode: SliderMode,
    /// The index of the base in `steps` (`FixedBase` mode).
    base: usize,
    pos: usize,
}

impl HistorySlider {
    /// A slider over these sides, newest first. The base is the oldest side.
    pub fn new(steps: Vec<Side>) -> HistorySlider {
        let base = steps.len().saturating_sub(1);
        HistorySlider {
            steps,
            mode: SliderMode::Neighbours,
            base,
            pos: 0,
        }
    }

    /// The slider for a branch of a version list (`None`: the current branch of the file). The
    /// open drawing is the first side unless it equals the newest version.
    pub fn for_branch(list: &VersionList, branch: Option<&str>) -> HistorySlider {
        let current = if list.branch.is_empty() {
            "main"
        } else {
            &list.branch
        };
        let branch = branch.unwrap_or(current);
        let mut steps = Vec::new();
        if list.current_is_newest != Some(true) {
            steps.push(Side::Current);
        }
        steps.extend(
            list.rows
                .iter()
                .rev()
                .filter(|r| r.branch == branch)
                .map(|r| Side::Version(r.id)),
        );
        HistorySlider::new(steps)
    }

    /// The sides, newest first.
    pub fn steps(&self) -> &[Side] {
        &self.steps
    }

    pub fn mode(&self) -> SliderMode {
        self.mode
    }

    /// The side that every step is compared with, in `FixedBase` mode.
    pub fn base(&self) -> Option<Side> {
        self.steps.get(self.base).copied()
    }

    /// How many positions the slider has.
    pub fn len(&self) -> usize {
        match self.mode {
            SliderMode::Neighbours => self.steps.len().saturating_sub(1),
            SliderMode::FixedBase => self.base.min(self.steps.len()),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    /// Moves to a position. A position past the end goes to the last one.
    pub fn set_position(&mut self, pos: usize) {
        self.pos = pos.min(self.len().saturating_sub(1));
    }

    /// Moves by `delta` positions (positive: back in time). Returns true if the position changed.
    pub fn step(&mut self, delta: i32) -> bool {
        let before = self.pos;
        let target = (self.pos as i64 + i64::from(delta)).clamp(0, i64::MAX) as usize;
        self.set_position(target);
        self.pos != before
    }

    pub fn set_mode(&mut self, mode: SliderMode) {
        self.mode = mode;
        self.set_position(self.pos);
    }

    /// Chooses the base (a side of the slider). Returns false if it is not one of the sides.
    pub fn set_base(&mut self, side: Side) -> bool {
        let Some(i) = self.steps.iter().position(|s| *s == side) else {
            return false;
        };
        self.base = i;
        self.set_position(self.pos);
        true
    }

    /// The pair of sides to compare at a position: the old side and the new side.
    pub fn pair_at(&self, pos: usize) -> Option<(Side, Side)> {
        if pos >= self.len() {
            return None;
        }
        match self.mode {
            SliderMode::Neighbours => Some((self.steps[pos + 1], self.steps[pos])),
            SliderMode::FixedBase => Some((self.steps[self.base], self.steps[pos])),
        }
    }

    /// The pair of sides at the position of the slider.
    pub fn pair(&self) -> Option<(Side, Side)> {
        self.pair_at(self.pos)
    }

    /// The pairs at the positions next to this one, nearest first (one step back in time, one
    /// forward). A window computes them while it is idle, so that the next step is quick.
    pub fn neighbours(&self) -> Vec<(Side, Side)> {
        [self.pos.checked_add(1), self.pos.checked_sub(1)]
            .into_iter()
            .flatten()
            .filter_map(|p| self.pair_at(p))
            .collect()
    }

    /// Takes a new list of versions (after a save or a delete) and keeps the person's place:
    /// the same pair if it still exists, else the nearest position. The mode stays, and so does
    /// the base if it is still a side.
    pub fn update(&mut self, list: &VersionList, branch: Option<&str>) {
        let was = self.pair();
        let base = self.base();
        let mode = self.mode;
        *self = HistorySlider::for_branch(list, branch);
        self.mode = mode;
        if let Some(b) = base {
            self.set_base(b);
        }
        if let Some(pair) = was
            && let Some(p) = (0..self.len()).find(|p| self.pair_at(*p) == Some(pair))
        {
            self.pos = p;
        } else {
            self.set_position(self.pos);
        }
    }
}

/// The two sides that the person picked in the Compare window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompareForm {
    pub from: Option<Side>,
    pub to: Side,
}

impl Default for CompareForm {
    fn default() -> Self {
        CompareForm {
            from: None,
            to: Side::Current,
        }
    }
}

impl CompareForm {
    /// The sides to compare, if the form is complete and the sides differ.
    pub fn pair(&self) -> Option<(Side, Side)> {
        self.from.filter(|f| *f != self.to).map(|f| (f, self.to))
    }

    /// Swaps the old and the new side. Returns false if the form has no old side.
    pub fn swap(&mut self) -> bool {
        let Some(from) = self.from else { return false };
        self.from = Some(self.to);
        self.to = from;
        true
    }

    /// Forgets a side that is not in the list any more: an old side becomes empty, a new side
    /// becomes the open drawing.
    pub fn retain(&mut self, list: &VersionList) {
        let gone = |s: Side| matches!(s, Side::Version(id) if list.find(id).is_none());
        if self.from.is_some_and(gone) {
            self.from = None;
        }
        if gone(self.to) {
            self.to = Side::Current;
        }
    }
}

/// Which statuses the list shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowFilter {
    pub added: bool,
    pub removed: bool,
    pub changed: bool,
}

impl Default for RowFilter {
    fn default() -> Self {
        RowFilter {
            added: true,
            removed: true,
            changed: true,
        }
    }
}

impl RowFilter {
    pub fn shows(&self, change: Change) -> bool {
        match change {
            Change::Added => self.added,
            Change::Removed => self.removed,
            Change::Changed => self.changed,
        }
    }
}

/// The rows of the objects that a filter shows. Layers, classes and resources have no place in
/// the plan, so the Compare window lists objects only.
pub fn visible_rows<'a>(rows: &'a [ChangeRow], filter: &RowFilter) -> Vec<&'a ChangeRow> {
    rows.iter()
        .filter(|r| r.group == "objects" && filter.shows(r.change))
        .collect()
}

/// A mark for a status that is not a color: `+`, `-` and `~`.
pub fn symbol(change: Change) -> char {
    match change {
        Change::Added => '+',
        Change::Removed => '-',
        Change::Changed => '~',
    }
}

/// What a click on a row does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClickAction {
    /// Select the object in the drawing that is shown, and zoom to it.
    SelectAndZoom(String),
    /// The object is not in the drawing that is shown: frame the ghost that the overlay draws.
    FrameGhost(String),
    /// Nothing to show in the plan (a layer, a class, a resource, or a setting).
    Nothing,
}

pub fn click_action(row: &ChangeRow) -> ClickAction {
    if row.group != "objects" {
        return ClickAction::Nothing;
    }
    match row.change {
        Change::Removed => ClickAction::FrameGhost(row.id.clone()),
        Change::Added | Change::Changed => ClickAction::SelectAndZoom(row.id.clone()),
    }
}

/// The part of the model to show for an object with this box: the box with a margin, and at least
/// half a meter around a point (a plant or a text of no size). `None` if the box is not finite.
pub fn frame_box(lo: DVec2, hi: DVec2) -> Option<(DVec2, DVec2)> {
    if !lo.is_finite() || !hi.is_finite() {
        return None;
    }
    let (lo, hi) = (lo.min(hi), lo.max(hi));
    let margin = ((hi - lo).max_element() * 0.4).max(500.0);
    Some((lo - margin, hi + margin))
}

#[cfg(test)]
mod tests;
