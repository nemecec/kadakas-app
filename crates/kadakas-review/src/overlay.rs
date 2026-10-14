//! The compare overlay (REV-03): which objects were added, removed or changed, as a plain model
//! that every renderer can use.
//!
//! The model has no egui and no renderer. A [`CompareOverlay`] holds:
//!
//! - the status of each object that is not unchanged (added, removed, changed), by [`ObjectId`];
//! - the *ghosts*: the objects of the old side that are not in the new drawing (removed
//!   objects), so that a renderer can draw them where they were;
//! - for a changed object, the shape it had before (and the new shape, if the caller gives it).
//!
//! Where the input comes from does not matter. The model is built from a generic list of
//! [`Change`] values (`id`, status, old object, new object), so REV-03 builds it from two
//! [`State`]s ([`CompareOverlay::from_states`]), and IOP-23 (Update from DXF) and IOP-24 (Compare
//! Drawings) build it from their own change lists ([`CompareOverlay::from_changes`]). The
//! renderers (`svg`, the plan canvas, the 3D scene) read the overlay and a [`CompareStyle`]: see
//! [`apply`].
//!
//! # Bounds
//!
//! The input can be a diff of 100 000 objects. The model keeps all counts, but:
//!
//! - at most [`MAX_GHOSTS`] ghosts, [`MAX_SHAPES`] old or new shapes of changed or added objects,
//!   and [`MAX_TRACKED`] statuses are kept. What is left out is counted (`omitted`) and a
//!   [`Note`] says so, so that a window or a render can tell the person;
//! - an object whose shape is longer than [`MAX_SHAPE_BYTES`] as JSON is not kept as a ghost;
//! - [`CompareOverlay::from_states`] stops after [`MAX_WORK`] compared properties (then `exact`
//!   is false).
//!
//! [`State`]: crate::State

use std::collections::BTreeMap;

use kadakas_core::model::{Object, ObjectId};
use kadakas_core::uid::Uid;
use kadakas_props::docsync::unflatten;
use kadakas_props::op::{Kind, Target};
use kadakas_props::validate::with_id;
use serde::Serialize;

use crate::diff::{Counts, MAX_WORK, moved_things, same_without_order};
use crate::state::{Props, State};

mod apply;
mod style;

pub use apply::{SvgCompare, ghost_document, tint_scene};
pub use style::{CompareStyle, Palette, Pattern, Tint, UnchangedMode};

/// The most ghosts (removed objects) that an overlay keeps.
pub const MAX_GHOSTS: usize = 2_000;
/// The most old or new shapes of changed or added objects that an overlay keeps.
pub const MAX_SHAPES: usize = 2_000;
/// The most statuses that an overlay keeps by ID. Counts stay exact beyond it.
pub const MAX_TRACKED: usize = 250_000;
/// The longest shape (as compact JSON, in bytes) of an object that is kept as a ghost.
pub const MAX_SHAPE_BYTES: usize = 256 * 1024;

/// What happened to an object between two sides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Added,
    Removed,
    Changed,
}

impl Status {
    pub const ALL: [Status; 3] = [Status::Added, Status::Removed, Status::Changed];

    pub fn name(self) -> &'static str {
        match self {
            Status::Added => "added",
            Status::Removed => "removed",
            Status::Changed => "changed",
        }
    }

    pub fn parse(name: &str) -> Option<Status> {
        Status::ALL.into_iter().find(|s| s.name() == name)
    }

    /// A mark that tells the status without color: `+`, `-` and `~`.
    pub fn symbol(self) -> char {
        match self {
            Status::Added => '+',
            Status::Removed => '-',
            Status::Changed => '~',
        }
    }
}

/// One change, as a caller of the overlay gives it. It does not depend on versions.
#[derive(Clone, Debug)]
pub struct Change {
    pub id: ObjectId,
    pub status: Status,
    /// The object on the old side (removed and changed objects).
    pub old: Option<Object>,
    /// The object on the new side (added and changed objects), when the new drawing does not
    /// hold it, for example a revision that is not accepted yet.
    pub new: Option<Object>,
}

/// The old and the new object of a change, as far as they are kept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shapes {
    pub old: Option<Object>,
    pub new: Option<Object>,
}

/// Something that the overlay does not show completely, and a window should say.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "note", rename_all = "snake_case")]
pub enum Note {
    /// Only `drawn` of the removed objects are drawn as ghosts; `omitted` are not.
    GhostsCapped { drawn: usize, omitted: usize },
    /// `omitted` old or new shapes of changed and added objects are not kept.
    ShapesCapped { omitted: usize },
    /// `untracked` changes have no status by ID, but are in the counts.
    StatusesCapped { untracked: usize },
    /// The work limit stopped the comparison: the counts are partial.
    Partial,
}

/// The limits of an overlay. The defaults are the constants of this module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub ghosts: usize,
    pub shapes: usize,
    pub tracked: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            ghosts: MAX_GHOSTS,
            shapes: MAX_SHAPES,
            tracked: MAX_TRACKED,
        }
    }
}

/// A short account of an overlay, for answers to agents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    pub total: usize,
    /// Removed objects that are drawn as ghosts.
    pub ghosts_drawn: usize,
    /// Removed objects that are not drawn because of the cap.
    pub ghosts_omitted: usize,
    /// False: the work limit stopped the comparison and the counts are partial.
    pub exact: bool,
}

/// The overlay. See the module text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompareOverlay {
    status: BTreeMap<ObjectId, Status>,
    counts: Counts,
    shapes: BTreeMap<ObjectId, Shapes>,
    kept_ghosts: usize,
    omitted_ghosts: usize,
    omitted_shapes: usize,
    untracked: usize,
    partial: bool,
}

/// Collects changes into an overlay within limits.
#[derive(Debug)]
pub struct Builder {
    limits: Limits,
    out: CompareOverlay,
    kept_shapes: usize,
}

impl Builder {
    pub fn new(limits: Limits) -> Builder {
        Builder {
            limits,
            out: CompareOverlay::default(),
            kept_shapes: 0,
        }
    }

    /// Notes the status of an object. Returns false if the object has a status already (the first
    /// one counts) and nothing changed.
    fn note(&mut self, id: ObjectId, status: Status) -> bool {
        if self.out.status.contains_key(&id) {
            return false;
        }
        match status {
            Status::Added => self.out.counts.added += 1,
            Status::Removed => self.out.counts.removed += 1,
            Status::Changed => self.out.counts.changed += 1,
        }
        if self.out.status.len() < self.limits.tracked {
            self.out.status.insert(id, status);
        } else {
            self.out.untracked += 1;
        }
        true
    }

    /// True if the object has a status entry (not beyond the cap of statuses).
    fn tracked(&self, id: ObjectId) -> bool {
        self.out.status.contains_key(&id)
    }

    /// True if a removed object could still be kept as a ghost.
    fn ghost_room(&self) -> bool {
        self.out.kept_ghosts < self.limits.ghosts
    }

    /// True if the old or new shape of a changed or added object could still be kept.
    fn shape_room(&self) -> bool {
        self.kept_shapes < self.limits.shapes
    }

    /// Keeps the shapes of an object whose status was noted. Objects that are too big, or that
    /// do not fit under the cap, are counted as omitted.
    fn keep(&mut self, id: ObjectId, status: Status, old: Option<Object>, new: Option<Object>) {
        let big = |o: &Option<Object>| o.as_ref().is_some_and(|o| !fits(&o.shape));
        if old.is_none() && new.is_none() {
            return;
        }
        if !self.out.status.contains_key(&id) {
            // No status entry (the status cap): the shapes are not kept either.
            return;
        }
        let room = if status == Status::Removed {
            self.ghost_room()
        } else {
            self.shape_room()
        };
        if !room || big(&old) || big(&new) {
            if status == Status::Removed {
                self.out.omitted_ghosts += 1;
            } else {
                self.out.omitted_shapes += 1;
            }
            return;
        }
        if status == Status::Removed {
            self.out.kept_ghosts += 1;
        } else {
            self.kept_shapes += 1;
        }
        self.out.shapes.insert(id, Shapes { old, new });
    }

    /// Adds a change. The first change of an object counts; a second one is ignored.
    pub fn push(&mut self, c: Change) {
        if self.note(c.id, c.status) {
            let (old, new) = match c.status {
                Status::Added => (None, c.new),
                Status::Removed => (c.old, None),
                Status::Changed => (c.old, c.new),
            };
            // A removed object without an old object has no ghost, which is not an omission.
            self.keep(c.id, c.status, old, new);
        }
    }

    pub fn finish(self) -> CompareOverlay {
        self.out
    }
}

/// True if the value is no longer than [`MAX_SHAPE_BYTES`] as compact JSON. The measure stops at
/// the limit, so a huge value is not written out.
fn fits<T: Serialize>(v: &T) -> bool {
    struct Cap(usize);
    impl std::io::Write for Cap {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(buf.len())
                .ok_or_else(|| std::io::Error::other("too long"))?;
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Cap(MAX_SHAPE_BYTES), v).is_ok()
}

/// The objects of a state with their properties, in ID order.
fn objects_of(s: &State) -> Vec<(Uid, &Props)> {
    s.things()
        .filter(|(t, _)| t.kind == Kind::Object)
        .map(|(t, p)| (t.id, p))
        .collect()
}

/// Reads an object from the properties of a state. `None` if it does not read (it needs a picture
/// that the state keeps as a reference, or the properties are not an object).
pub fn object_of(id: Uid, props: &Props) -> Option<Object> {
    serde_json::from_value(with_id(id, &unflatten(Kind::Object, props))).ok()
}

impl CompareOverlay {
    /// An overlay from a list of changes, with the default limits.
    pub fn from_changes(changes: impl IntoIterator<Item = Change>) -> CompareOverlay {
        CompareOverlay::from_changes_with(changes, Limits::default())
    }

    /// Like [`CompareOverlay::from_changes`], with limits. The input is read lazily: an iterator
    /// of 100 000 changes is not collected.
    pub fn from_changes_with(
        changes: impl IntoIterator<Item = Change>,
        limits: Limits,
    ) -> CompareOverlay {
        let mut b = Builder::new(limits);
        for c in changes {
            b.push(c);
        }
        b.finish()
    }

    /// The overlay between two states: only the objects (layers, classes and resources have
    /// their own place in the diff). An object counts as changed under the same rules as the
    /// diff (`kadakas_review::diff`): a property differs, or it moved in the stacking order.
    pub fn from_states(old: &State, new: &State) -> CompareOverlay {
        CompareOverlay::from_states_with(old, new, Limits::default(), MAX_WORK)
    }

    /// Like [`CompareOverlay::from_states`], with limits and the most properties to compare.
    pub fn from_states_with(
        old: &State,
        new: &State,
        limits: Limits,
        max_work: usize,
    ) -> CompareOverlay {
        let mut b = Builder::new(limits);
        let moved = moved_things(old, new);
        let (a, z) = (objects_of(old), objects_of(new));
        let (mut i, mut j) = (0, 0);
        let mut work = 0usize;
        let mut partial = false;
        while i < a.len() || j < z.len() {
            let order = match (a.get(i), z.get(j)) {
                (Some((x, _)), Some((y, _))) => x.cmp(y),
                (Some(_), None) => std::cmp::Ordering::Less,
                _ => std::cmp::Ordering::Greater,
            };
            let (before, after) = match order {
                std::cmp::Ordering::Less => {
                    i += 1;
                    (Some(a[i - 1]), None)
                }
                std::cmp::Ordering::Greater => {
                    j += 1;
                    (None, Some(z[j - 1]))
                }
                std::cmp::Ordering::Equal => {
                    i += 1;
                    j += 1;
                    (Some(a[i - 1]), Some(z[j - 1]))
                }
            };
            work += before.map_or(0, |(_, p)| p.len()) + after.map_or(0, |(_, p)| p.len()) + 1;
            if work > max_work {
                partial = true;
                break;
            }
            match (before, after) {
                (None, Some((id, _))) => {
                    b.note(ObjectId(id), Status::Added);
                }
                (Some((id, p)), None) => {
                    let oid = ObjectId(id);
                    if b.note(oid, Status::Removed) && b.tracked(oid) {
                        let old = (b.ghost_room() && p.get("shape").is_none_or(fits))
                            .then(|| object_of(id, p))
                            .flatten();
                        if old.is_some() {
                            b.keep(oid, Status::Removed, old, None);
                        } else {
                            b.out.omitted_ghosts += 1;
                        }
                    }
                }
                (Some((id, p)), Some((_, q))) => {
                    let t = Target::new(Kind::Object, id);
                    if moved.contains_key(&t) || !same_without_order(p, q) {
                        let oid = ObjectId(id);
                        if b.note(oid, Status::Changed) && b.tracked(oid) {
                            let old = (b.shape_room() && p.get("shape").is_none_or(fits))
                                .then(|| object_of(id, p))
                                .flatten();
                            if old.is_some() {
                                b.keep(oid, Status::Changed, old, None);
                            } else {
                                b.out.omitted_shapes += 1;
                            }
                        }
                    }
                }
                (None, None) => {}
            }
        }
        let mut out = b.finish();
        out.partial = partial;
        out
    }

    /// The status of an object, or `None` for an unchanged object (or one beyond the cap of
    /// statuses).
    pub fn status_of(&self, id: ObjectId) -> Option<Status> {
        self.status.get(&id).copied()
    }

    pub fn counts(&self) -> Counts {
        self.counts
    }

    /// How many objects changed in any way.
    pub fn total(&self) -> usize {
        self.counts.total()
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    /// False: the work limit stopped the comparison and the counts are partial.
    pub fn exact(&self) -> bool {
        !self.partial
    }

    /// The IDs with a status, in ID order, at most `limit`. Beyond the cap of statuses an object
    /// is in the counts but not in this list.
    pub fn ids(&self, status: Status, limit: usize) -> Vec<ObjectId> {
        self.status
            .iter()
            .filter(|(_, s)| **s == status)
            .map(|(id, _)| *id)
            .take(limit)
            .collect()
    }

    /// All the objects with a status, in ID order.
    pub fn statuses(&self) -> impl Iterator<Item = (ObjectId, Status)> + '_ {
        self.status.iter().map(|(id, s)| (*id, *s))
    }

    /// The shapes that are kept for an object.
    pub fn shapes_of(&self, id: ObjectId) -> Option<&Shapes> {
        self.shapes.get(&id)
    }

    /// The objects of the old side that are not in the new drawing, in ID order: the ghosts.
    pub fn ghosts(&self) -> impl Iterator<Item = &Object> + '_ {
        self.shapes
            .iter()
            .filter(|(id, _)| self.status.get(id) == Some(&Status::Removed))
            .filter_map(|(_, s)| s.old.as_ref())
    }

    /// The shapes that changed objects had before, in ID order.
    pub fn previous(&self) -> impl Iterator<Item = &Object> + '_ {
        self.shapes
            .iter()
            .filter(|(id, _)| self.status.get(id) == Some(&Status::Changed))
            .filter_map(|(_, s)| s.old.as_ref())
    }

    /// The number of removed objects that are kept as ghosts.
    pub fn ghost_count(&self) -> usize {
        self.kept_ghosts
    }

    /// The number of removed objects that are not drawn because of the cap or the size.
    pub fn omitted_ghosts(&self) -> usize {
        self.omitted_ghosts
    }

    /// Changes that have no status by ID because of the cap of statuses.
    pub fn untracked(&self) -> usize {
        self.untracked
    }

    /// What the overlay does not show completely. Empty: everything is shown.
    pub fn notes(&self) -> Vec<Note> {
        let mut notes = Vec::new();
        // A removed object that has no old object to draw (a caller gave none) is not a ghost
        // and not an omission.
        if self.omitted_ghosts > 0 {
            notes.push(Note::GhostsCapped {
                drawn: self.kept_ghosts,
                omitted: self.omitted_ghosts,
            });
        }
        if self.omitted_shapes > 0 {
            notes.push(Note::ShapesCapped {
                omitted: self.omitted_shapes,
            });
        }
        if self.untracked > 0 {
            notes.push(Note::StatusesCapped {
                untracked: self.untracked,
            });
        }
        if self.partial {
            notes.push(Note::Partial);
        }
        notes
    }

    pub fn summary(&self) -> Summary {
        Summary {
            added: self.counts.added,
            removed: self.counts.removed,
            changed: self.counts.changed,
            total: self.total(),
            ghosts_drawn: self.kept_ghosts,
            ghosts_omitted: self.omitted_ghosts,
            exact: self.exact(),
        }
    }

    /// An estimate of the memory that the overlay holds, in bytes. A cache uses it to stay within
    /// a budget.
    pub fn bytes(&self) -> usize {
        let per_status = std::mem::size_of::<(ObjectId, Status)>() + 16;
        let shape_bytes: usize = self
            .shapes
            .values()
            .map(|s| {
                [&s.old, &s.new]
                    .into_iter()
                    .flatten()
                    .map(|o| serde_json::to_vec(&o.shape).map_or(512, |v| v.len()) + 256)
                    .sum::<usize>()
            })
            .sum();
        self.status.len() * per_status + shape_bytes + 128
    }
}

#[cfg(test)]
mod tests;
