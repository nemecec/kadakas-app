//! What an undo step changed, and taking a step back out of order (AGR-05, AGR-06).
//!
//! Every undo step has a serial that stays through undo and redo. The activity log of an agent
//! (`crate::activity`) reads what a step did as a [`StepData`]: for each object the state before
//! and after, so a review can show the change as it was, and the revert of an old step can check
//! that nothing touched the objects since.
//!
//! [`Editor::revert_step`] takes one step back although later steps exist:
//!
//! - A step that is on top of the history is a plain undo (it can be redone).
//! - Else the objects of the step go back to the state before it, but only those that are still
//!   exactly as the step left them. An object that something changed later is a [`Conflict`]: the
//!   revert is refused as a whole, or (`skip_conflicts`) leaves exactly those objects as they are
//!   and reverts the rest. Nothing a later step made is ever overwritten.
//! - A step that also changed layers, classes, resources, document settings, the versions or the
//!   issues is only reverted as a plain undo: those parts are one snapshot, and putting an old
//!   snapshot back would drop later changes.

use super::*;

/// A count of added, changed and removed things.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub added: usize,
    pub changed: usize,
    pub removed: usize,
}

impl Counts {
    pub fn total(&self) -> usize {
        self.added + self.changed + self.removed
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }
}

/// One object of a step: its state before and after. `before` is `None` for an object the step
/// made, `after` is `None` for one it removed.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjChange {
    pub id: ObjectId,
    pub before: Option<Object>,
    pub after: Option<Object>,
    /// Where a removed object stood in the z-order (a hint for putting it back).
    pub index: Option<usize>,
}

impl ObjChange {
    /// `Added`, `Changed` or `Removed`.
    pub fn kind(&self) -> Counted {
        match (&self.before, &self.after) {
            (None, _) => Counted::Added,
            (_, None) => Counted::Removed,
            _ => Counted::Changed,
        }
    }
}

/// How an object changed in a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Counted {
    Added,
    Changed,
    Removed,
}

/// What one step (or one call that made several steps) changed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StepData {
    /// The objects with a net change, in the order they were first touched. An object that the
    /// step made and removed again, or changed and put back, is not listed.
    pub objects: Vec<ObjChange>,
    pub layers: Counts,
    pub classes: Counts,
    pub resources: Counts,
    /// The names of the layers the step added.
    pub layers_added: Vec<String>,
    /// Active layer or class, the georeference, project data, sheets or saved views changed.
    pub document: bool,
    /// The named versions of the file changed.
    pub versions: bool,
    /// Review issues or their comments changed.
    pub issues: bool,
}

impl StepData {
    /// The objects by how they changed.
    pub fn object_counts(&self) -> Counts {
        let mut c = Counts::default();
        for o in &self.objects {
            match o.kind() {
                Counted::Added => c.added += 1,
                Counted::Changed => c.changed += 1,
                Counted::Removed => c.removed += 1,
            }
        }
        c
    }

    /// True if the step changed more than objects (layers, classes, resources, settings, versions
    /// or issues). Such a step is reverted only as a plain undo.
    pub fn has_snapshot_parts(&self) -> bool {
        !(self.layers.is_empty() && self.classes.is_empty() && self.resources.is_empty())
            || self.document
            || self.versions
            || self.issues
    }

    /// True if the step changed nothing that a person sees.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && !self.has_snapshot_parts()
    }
}

/// Where a step is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepPlace {
    /// In the undo history, with this many steps above it.
    Undo(usize),
    /// Undone; redo can bring it back.
    Redo,
    /// Not in the history any more (the limit dropped it, a new command cleared the redo steps,
    /// or a shared document replaced the history).
    Gone,
}

/// Why an object of a step cannot go back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    /// Changed after the step.
    ChangedLater,
    /// Removed after the step, but the step changed it.
    DeletedLater,
    /// The step removed it, and an object with this ID exists again.
    ExistsAgain,
    /// Its layer is gone.
    LayerGone,
    /// Its layer is locked.
    LayerLocked,
    /// Its class is gone.
    ClassGone,
}

/// An object that the revert would have to overwrite or cannot put back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub id: ObjectId,
    pub kind: ConflictKind,
}

/// Why a revert did not happen. Nothing changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The step is not in the history (see [`StepPlace::Gone`]).
    Gone,
    /// The step is undone already.
    AlreadyUndone,
    /// Objects of the step were touched later; none was changed.
    Conflicts(Vec<Conflict>),
    /// The step changed layers, classes, resources, settings, versions or issues, and later steps
    /// exist: only a plain undo (back to the step) is possible.
    Snapshot { later: usize },
    /// Taking the objects back would also take away something that was made later (for example
    /// a group that holds the object now).
    InUse(String),
    /// The document is shared: a plain undo of the editor would not fit the per-user undo of the
    /// collaboration. Only the revert of the objects works there.
    Shared,
    /// The document refused the change.
    Rejected(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Gone => write!(
                f,
                "the step is not in the undo history any more (the history limit dropped it, or a newer change cleared it)"
            ),
            Refusal::AlreadyUndone => write!(f, "the step is undone already"),
            Refusal::Conflicts(list) => {
                let mut counts = [0usize; 6];
                for c in list {
                    counts[c.kind as usize] += 1;
                }
                let names = [
                    "changed later",
                    "deleted later",
                    "exist again",
                    "on a layer that is gone",
                    "on a locked layer",
                    "in a class that is gone",
                ];
                let parts: Vec<String> = counts
                    .iter()
                    .zip(names)
                    .filter(|(n, _)| **n > 0)
                    .map(|(n, w)| format!("{n} {w}"))
                    .collect();
                write!(
                    f,
                    "{} object(s) of the step cannot go back: {}",
                    list.len(),
                    parts.join(", ")
                )
            }
            Refusal::Snapshot { later } => write!(
                f,
                "the step also changed layers, classes, resources, settings, versions or issues, and {later} later step(s) exist; undo back to the step instead"
            ),
            Refusal::Shared => write!(
                f,
                "the document is shared: only the objects of the step can go back one by one, and a step that also changed layers, classes, resources, settings or versions cannot"
            ),
            Refusal::InUse(why) => write!(f, "{why}"),
            Refusal::Rejected(why) => write!(f, "the document refused the change: {why}"),
        }
    }
}

impl std::error::Error for Refusal {}

/// What a revert did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RevertReport {
    /// The objects that went back.
    pub reverted: Vec<ObjectId>,
    /// The objects that stayed as they are (only with `skip_conflicts`).
    pub skipped: Vec<Conflict>,
    /// The objects the step made that were deleted since: nothing to do.
    pub already_gone: Vec<ObjectId>,
    /// True if the step was on top of the history and went back as a plain undo (redo works).
    pub undone: bool,
}

/// Adds the net change of the patches (oldest first) into one.
fn compose(patches: &[&Patch]) -> StepData {
    type Net = (Option<Object>, Option<Object>, Option<usize>);
    let mut map: IndexMap<ObjectId, Net> = IndexMap::new();
    let mut data = StepData::default();
    for p in patches {
        for (index, o) in &p.removed {
            map.entry(o.id).and_modify(|e| e.1 = None).or_insert((
                Some(o.clone()),
                None,
                Some(*index),
            ));
        }
        for (b, a) in &p.modified {
            map.entry(b.id)
                .and_modify(|e| e.1 = Some(a.clone()))
                .or_insert((Some(b.clone()), Some(a.clone()), None));
        }
        for (_, o) in &p.added {
            map.entry(o.id)
                .and_modify(|e| e.1 = Some(o.clone()))
                .or_insert((None, Some(o.clone()), None));
        }
        let cs = p.change_set(true, 0, "", p.origin);
        let add = |c: &mut Counts, a: usize, ch: usize, r: usize| {
            c.added += a;
            c.changed += ch;
            c.removed += r;
        };
        add(
            &mut data.layers,
            cs.added.layers.len(),
            cs.changed.layers.len(),
            cs.removed.layers.len(),
        );
        add(
            &mut data.classes,
            cs.added.classes.len(),
            cs.changed.classes.len(),
            cs.removed.classes.len(),
        );
        add(
            &mut data.resources,
            cs.added.resources.len(),
            cs.changed.resources.len(),
            cs.removed.resources.len(),
        );
        if let Some((_, after)) = &p.meta {
            data.layers_added.extend(
                after
                    .layers
                    .iter()
                    .filter(|l| cs.added.layers.contains(&l.id))
                    .map(|l| l.name.clone()),
            );
        }
        data.document |= cs.document.active_layer
            || cs.document.active_class
            || cs.document.georef
            || cs.document.drawing;
        data.versions |= cs.document.history;
        data.issues |= !(cs.added.issues.is_empty()
            && cs.changed.issues.is_empty()
            && cs.removed.issues.is_empty()
            && cs.added.comments.is_empty()
            && cs.changed.comments.is_empty()
            && cs.removed.comments.is_empty());
    }
    data.objects = map
        .into_iter()
        .filter(|(_, (b, a, _))| b != a)
        .map(|(id, (before, after, index))| ObjChange {
            id,
            before,
            after,
            index,
        })
        .collect();
    data
}

impl Editor {
    /// A mark for [`Editor::steps_since`]: steps made after this call have a serial from the
    /// mark on.
    pub fn serial_mark(&self) -> u64 {
        SERIALS.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The serials of the steps in the undo history made since the mark, oldest first. A step that
    /// was made and taken back again is not there.
    pub fn steps_since(&self, mark: u64) -> Vec<u64> {
        self.undo
            .iter()
            .map(|p| p.serial)
            .filter(|s| *s >= mark)
            .collect()
    }

    /// Where a step is.
    pub fn step_place(&self, serial: u64) -> StepPlace {
        if let Some(i) = self.undo.iter().rposition(|p| p.serial == serial) {
            StepPlace::Undo(self.undo.len() - 1 - i)
        } else if self.redo.iter().any(|p| p.serial == serial) {
            StepPlace::Redo
        } else {
            StepPlace::Gone
        }
    }

    /// Who made the steps above this one in the undo history, newest first.
    pub fn origins_above(&self, serial: u64) -> Vec<Origin> {
        match self.undo.iter().rposition(|p| p.serial == serial) {
            Some(i) => self.undo[i + 1..].iter().rev().map(|p| p.origin).collect(),
            None => Vec::new(),
        }
    }

    /// The net change of the steps with these serials (in the undo or the redo history). `None`
    /// if one of them is gone.
    pub fn step_data(&self, serials: &[u64]) -> Option<StepData> {
        let mut found: Vec<&Patch> = Vec::with_capacity(serials.len());
        for s in serials {
            found.push(
                self.undo
                    .iter()
                    .rev()
                    .chain(self.redo.iter())
                    .find(|p| p.serial == *s)?,
            );
        }
        Some(compose(&found))
    }

    /// Takes back the steps down to this one, and this one (the newest first). The steps above it
    /// go to the redo history. Returns how many steps were undone. Nothing happens if the step
    /// is not in the undo history.
    pub fn undo_through(&mut self, serial: u64) -> Result<usize, Refusal> {
        match self.step_place(serial) {
            StepPlace::Undo(above) => {
                for _ in 0..=above {
                    self.undo();
                }
                Ok(above + 1)
            }
            StepPlace::Redo => Err(Refusal::AlreadyUndone),
            StepPlace::Gone => Err(Refusal::Gone),
        }
    }

    /// The objects of the step that cannot go back now, and the objects of it that were deleted
    /// since and need nothing.
    pub fn revert_conflicts(&self, data: &StepData) -> (Vec<Conflict>, Vec<ObjectId>) {
        let mut conflicts = Vec::new();
        let mut gone = Vec::new();
        let doc = &self.doc;
        let place_ok = |layer: LayerId, class: ClassId| -> Option<ConflictKind> {
            match doc.layer(layer) {
                None => return Some(ConflictKind::LayerGone),
                Some(l) if l.locked => return Some(ConflictKind::LayerLocked),
                Some(_) => {}
            }
            doc.class(class)
                .is_none()
                .then_some(ConflictKind::ClassGone)
        };
        for ch in &data.objects {
            let current = doc.object(ch.id);
            let kind = match (&ch.before, &ch.after) {
                (Some(before), None) => match current {
                    Some(_) => Some(ConflictKind::ExistsAgain),
                    None => place_ok(before.layer, before.class),
                },
                (None, Some(after)) => match current {
                    None => {
                        gone.push(ch.id);
                        None
                    }
                    Some(c) if c == after => {
                        place_ok(c.layer, c.class).filter(|k| *k == ConflictKind::LayerLocked)
                    }
                    Some(_) => Some(ConflictKind::ChangedLater),
                },
                (Some(before), Some(after)) => match current {
                    None => Some(ConflictKind::DeletedLater),
                    Some(c) if c == after => place_ok(c.layer, c.class)
                        .filter(|k| *k == ConflictKind::LayerLocked)
                        .or_else(|| place_ok(before.layer, before.class)),
                    Some(_) => Some(ConflictKind::ChangedLater),
                },
                (None, None) => None,
            };
            if let Some(kind) = kind {
                conflicts.push(Conflict { id: ch.id, kind });
            }
        }
        (conflicts, gone)
    }

    /// Takes one step (or the steps of one call) back although later steps exist. See the module
    /// text for the rules. `serials` are the steps, oldest first, and `data` is what they did
    /// (kept by the activity log, so it does not depend on the history).
    ///
    /// On success the revert is one undo step of its own, made by the current origin, so it can
    /// be undone. On an error nothing changed.
    pub fn revert_step(
        &mut self,
        serials: &[u64],
        data: &StepData,
        skip_conflicts: bool,
        allow_undo: bool,
    ) -> Result<RevertReport, Refusal> {
        let mut above = usize::MAX;
        for s in serials {
            match self.step_place(*s) {
                StepPlace::Undo(n) => above = above.min(n),
                StepPlace::Redo => return Err(Refusal::AlreadyUndone),
                StepPlace::Gone => return Err(Refusal::Gone),
            }
        }
        if serials.is_empty() {
            return Err(Refusal::Gone);
        }
        // On top of the history: a plain undo is exact and can be redone.
        let top = serials.len() <= self.undo.len()
            && self
                .undo
                .iter()
                .rev()
                .take(serials.len())
                .map(|p| p.serial)
                .eq(serials.iter().rev().copied());
        if top && allow_undo {
            let mut report = RevertReport {
                undone: true,
                ..Default::default()
            };
            report.reverted = data.objects.iter().map(|o| o.id).collect();
            for _ in serials {
                self.undo();
            }
            return Ok(report);
        }
        if data.has_snapshot_parts() {
            return Err(if allow_undo {
                Refusal::Snapshot { later: above }
            } else {
                Refusal::Shared
            });
        }
        let (conflicts, already_gone) = self.revert_conflicts(data);
        if !conflicts.is_empty() && !skip_conflicts {
            return Err(Refusal::Conflicts(conflicts));
        }
        let skip: HashSet<ObjectId> = conflicts.iter().map(|c| c.id).collect();
        let gone: HashSet<ObjectId> = already_gone.iter().copied().collect();
        let todo: Vec<&ObjChange> = data
            .objects
            .iter()
            .filter(|c| !skip.contains(&c.id) && !gone.contains(&c.id))
            .collect();
        if todo.is_empty() {
            return if conflicts.is_empty() {
                Ok(RevertReport {
                    already_gone,
                    ..Default::default()
                })
            } else {
                Err(Refusal::Conflicts(conflicts))
            };
        }
        let outcome = self.transact(
            || "revert".to_string(),
            None,
            |tx, _| {
                // The objects the step made go first, with whatever must go with them.
                let remove: Vec<ObjectId> = todo
                    .iter()
                    .filter(|c| c.before.is_none())
                    .map(|c| c.id)
                    .collect();
                if !remove.is_empty() {
                    let all = tx.with_group_members(&remove);
                    if let Some(extra) = all.iter().find(|i| !remove.contains(i)) {
                        return Err(CommandError::Invalid(format!(
                            "object {extra} was made later and goes with the objects of the step"
                        )));
                    }
                    tx.run(
                        &Command::DeleteObjects { ids: remove },
                        &mut Outcome::default(),
                    )?;
                }
                for c in todo
                    .iter()
                    .filter(|c| c.before.is_some() && c.after.is_some())
                {
                    tx.editable(c.id)?;
                    tx.touch(c.id);
                    if let (Some(slot), Some(before)) = (tx.doc.object_mut(c.id), &c.before) {
                        *slot = before.clone();
                    }
                }
                let mut back: Vec<&&ObjChange> =
                    todo.iter().filter(|c| c.after.is_none()).collect();
                back.sort_by_key(|c| c.index.unwrap_or(usize::MAX));
                for c in back {
                    let Some(o) = &c.before else { continue };
                    tx.check_layer(o.layer)?;
                    tx.check_class(o.class)?;
                    let at = c.index.unwrap_or(usize::MAX).min(tx.doc.objects.len());
                    tx.added.push(o.id);
                    tx.added_set.insert(o.id);
                    tx.doc.objects.shift_insert(at, o.id, o.clone());
                }
                Ok(())
            },
        );
        match outcome {
            Ok(_) => Ok(RevertReport {
                reverted: todo.iter().map(|c| c.id).collect(),
                skipped: conflicts,
                already_gone,
                undone: false,
            }),
            Err(CommandError::Invalid(why)) if why.contains("goes with the objects") => {
                Err(Refusal::InUse(why))
            }
            Err(e) => Err(Refusal::Rejected(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(ed: &mut Editor, x: f64) -> ObjectId {
        let o = ed
            .execute(&Command::AddObject {
                shape: Shape::Line {
                    a: DVec2::new(x, 0.0),
                    b: DVec2::new(x + 1.0, 1.0),
                },
                layer: None,
                class: None,
                attrs: Default::default(),
            })
            .unwrap();
        ObjectId(o.created[0])
    }

    fn mv(ed: &mut Editor, id: ObjectId, dx: f64) {
        ed.execute(&Command::MoveObjects {
            ids: vec![id],
            delta: DVec2::new(dx, 0.0),
        })
        .unwrap();
    }

    fn step(ed: &mut Editor, f: impl FnOnce(&mut Editor)) -> (Vec<u64>, StepData) {
        let mark = ed.serial_mark();
        f(ed);
        let serials = ed.steps_since(mark);
        let data = ed.step_data(&serials).unwrap();
        (serials, data)
    }

    #[test]
    fn serials_stay_through_undo_and_redo() {
        let mut ed = Editor::with_sequential_ids();
        let (s, _) = step(&mut ed, |e| {
            add(e, 0.0);
        });
        assert_eq!(ed.step_place(s[0]), StepPlace::Undo(0));
        ed.undo();
        assert_eq!(ed.step_place(s[0]), StepPlace::Redo);
        ed.redo();
        assert_eq!(ed.step_place(s[0]), StepPlace::Undo(0));
        ed.undo();
        add(&mut ed, 5.0);
        assert_eq!(
            ed.step_place(s[0]),
            StepPlace::Gone,
            "a new command clears redo"
        );
    }

    #[test]
    fn data_of_a_call_with_several_steps_is_the_net_change() {
        let mut ed = Editor::with_sequential_ids();
        let keep = add(&mut ed, 0.0);
        let (serials, data) = step(&mut ed, |e| {
            let a = add(e, 10.0);
            mv(e, a, 5.0);
            mv(e, keep, 2.0);
            let b = add(e, 20.0);
            e.execute(&Command::DeleteObjects { ids: vec![b] }).unwrap();
        });
        assert_eq!(serials.len(), 5);
        let c = data.object_counts();
        assert_eq!((c.added, c.changed, c.removed), (1, 1, 0));
        let added = data
            .objects
            .iter()
            .find(|o| o.kind() == Counted::Added)
            .unwrap();
        // The state after is the moved one.
        assert_eq!(
            added.after.as_ref().unwrap().shape,
            Shape::Line {
                a: DVec2::new(15.0, 0.0),
                b: DVec2::new(16.0, 1.0)
            }
        );
    }

    #[test]
    fn the_top_step_reverts_as_an_undo_that_redo_brings_back() {
        let mut ed = Editor::with_sequential_ids();
        let (s, d) = step(&mut ed, |e| {
            add(e, 0.0);
        });
        let r = ed.revert_step(&s, &d, false, true).unwrap();
        assert!(r.undone);
        assert!(ed.doc().objects.is_empty());
        assert!(ed.redo());
        assert_eq!(ed.doc().objects.len(), 1);
    }

    #[test]
    fn an_old_step_reverts_when_later_steps_left_its_objects_alone() {
        let mut ed = Editor::with_sequential_ids();
        let other = add(&mut ed, 100.0);
        let (s, d) = step(&mut ed, |e| {
            add(e, 0.0);
            mv(e, other, 1.0);
        });
        let later = add(&mut ed, 50.0);
        let before = ed.doc().object(other).cloned();
        assert!(before.is_some());
        let r = ed.revert_step(&s, &d, false, true).unwrap();
        assert!(!r.undone);
        assert_eq!(r.reverted.len(), 2);
        // The new object is gone, the moved one is back, and the later object stayed.
        assert_eq!(ed.doc().objects.len(), 2);
        assert!(ed.doc().object(later).is_some());
        assert_eq!(
            ed.doc().object(other).unwrap().shape,
            Shape::Line {
                a: DVec2::new(100.0, 0.0),
                b: DVec2::new(101.0, 1.0)
            }
        );
        // The revert is a step of its own: undo puts the agent's work back.
        assert!(ed.undo());
        assert_eq!(ed.doc().objects.len(), 3);
        assert_eq!(ed.doc().object(other), before.as_ref());
    }

    #[test]
    fn a_revert_is_refused_when_a_later_step_changed_an_object_of_the_step() {
        let mut ed = Editor::with_sequential_ids();
        let (s, d) = step(&mut ed, |e| {
            add(e, 0.0);
        });
        let id = d.objects[0].id;
        mv(&mut ed, id, 3.0);
        let doc_before = ed.doc().clone();
        let depth = ed.undo_depth();
        let err = ed.revert_step(&s, &d, false, true).unwrap_err();
        assert_eq!(
            err,
            Refusal::Conflicts(vec![Conflict {
                id,
                kind: ConflictKind::ChangedLater
            }])
        );
        assert!(err.to_string().contains("changed later"));
        assert_eq!(ed.doc(), &doc_before, "nothing changed");
        assert_eq!(ed.undo_depth(), depth);
    }

    #[test]
    fn skip_conflicts_reverts_the_rest_and_keeps_the_touched_objects() {
        let mut ed = Editor::with_sequential_ids();
        let (s, d) = step(&mut ed, |e| {
            add(e, 0.0);
            add(e, 10.0);
        });
        let (a, b) = (d.objects[0].id, d.objects[1].id);
        mv(&mut ed, a, 3.0);
        let r = ed.revert_step(&s, &d, true, true).unwrap();
        assert_eq!(r.reverted, vec![b]);
        assert_eq!(r.skipped.len(), 1);
        assert!(ed.doc().object(a).is_some());
        assert!(ed.doc().object(b).is_none());
    }

    #[test]
    fn deleted_later_is_a_conflict_for_a_change_and_nothing_to_do_for_an_addition() {
        let mut ed = Editor::with_sequential_ids();
        let old = add(&mut ed, 0.0);
        let (s, d) = step(&mut ed, |e| {
            mv(e, old, 1.0);
            add(e, 10.0);
        });
        let new = d.objects.iter().find(|o| o.id != old).unwrap().id;
        ed.execute(&Command::DeleteObjects {
            ids: vec![old, new],
        })
        .unwrap();
        let err = ed.revert_step(&s, &d, false, true).unwrap_err();
        assert_eq!(
            err,
            Refusal::Conflicts(vec![Conflict {
                id: old,
                kind: ConflictKind::DeletedLater
            }])
        );
        let (_, gone) = ed.revert_conflicts(&d);
        assert_eq!(gone, vec![new]);
    }

    #[test]
    fn removed_objects_come_back_in_place_and_not_over_a_new_object_with_the_same_id() {
        let mut ed = Editor::with_sequential_ids();
        let a = add(&mut ed, 0.0);
        let b = add(&mut ed, 10.0);
        let c = add(&mut ed, 20.0);
        let (s, d) = step(&mut ed, |e| {
            e.execute(&Command::DeleteObjects { ids: vec![b] }).unwrap();
        });
        add(&mut ed, 30.0);
        let r = ed.revert_step(&s, &d, false, true).unwrap();
        assert_eq!(r.reverted, vec![b]);
        let order: Vec<ObjectId> = ed.doc().objects.keys().copied().take(3).collect();
        assert_eq!(order, vec![a, b, c], "back at its place in the z-order");
    }

    #[test]
    fn a_locked_layer_blocks_the_revert() {
        let mut ed = Editor::with_sequential_ids();
        let (s, d) = step(&mut ed, |e| {
            add(e, 0.0);
        });
        add(&mut ed, 5.0);
        let layer = ed.doc().object(d.objects[0].id).unwrap().layer;
        ed.execute(&Command::ModifyLayer {
            id: layer,
            name: None,
            visible: None,
            locked: Some(true),
            elevation: None,
            scale: None,
        })
        .unwrap();
        let err = ed.revert_step(&s, &d, false, true).unwrap_err();
        assert!(matches!(
            err,
            Refusal::Conflicts(ref l) if l[0].kind == ConflictKind::LayerLocked
        ));
    }

    #[test]
    fn a_step_with_layers_reverts_only_as_an_undo() {
        let mut ed = Editor::with_sequential_ids();
        let (s, d) = step(&mut ed, |e| {
            e.execute(&Command::AddLayer {
                name: "Planting".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap();
        });
        assert_eq!(d.layers.added, 1);
        assert_eq!(d.layers_added, vec!["Planting".to_string()]);
        add(&mut ed, 0.0);
        assert_eq!(
            ed.revert_step(&s, &d, false, true).unwrap_err(),
            Refusal::Snapshot { later: 1 }
        );
        // Undo back to the step takes both.
        assert_eq!(ed.undo_through(s[0]).unwrap(), 2);
        assert_eq!(ed.step_place(s[0]), StepPlace::Redo);
        assert_eq!(ed.undo_through(s[0]), Err(Refusal::AlreadyUndone));
    }

    #[test]
    fn a_group_made_later_blocks_the_removal_of_a_member() {
        let mut ed = Editor::with_sequential_ids();
        let (s, d) = step(&mut ed, |e| {
            add(e, 0.0);
            add(e, 10.0);
        });
        let ids: Vec<ObjectId> = d.objects.iter().map(|o| o.id).collect();
        ed.execute(&Command::Group {
            ids: ids.clone(),
            name: None,
        })
        .unwrap();
        // The members changed (their parent): a conflict, not a deletion of the group.
        assert!(matches!(
            ed.revert_step(&s, &d, false, true),
            Err(Refusal::Conflicts(_))
        ));
        assert_eq!(ed.doc().objects.len(), 3);
    }

    #[test]
    fn without_a_plain_undo_the_top_step_reverts_as_a_new_edit_and_a_snapshot_step_is_refused() {
        let mut ed = Editor::with_sequential_ids();
        let (s, d) = step(&mut ed, |e| {
            add(e, 0.0);
        });
        let r = ed.revert_step(&s, &d, false, false).unwrap();
        assert!(!r.undone);
        assert!(ed.doc().objects.is_empty());
        assert!(!ed.can_redo(), "a new edit, not an undo");
        assert!(ed.undo(), "which can be undone");
        assert_eq!(ed.doc().objects.len(), 1);
        let (s, d) = step(&mut ed, |e| {
            e.execute(&Command::AddLayer {
                name: "L".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap();
        });
        assert_eq!(ed.revert_step(&s, &d, false, false), Err(Refusal::Shared));
    }

    #[test]
    fn steps_of_other_origins_are_listed_above() {
        let mut ed = Editor::with_sequential_ids();
        ed.set_origin(Origin::Agent);
        let (s, _) = step(&mut ed, |e| {
            add(e, 0.0);
        });
        ed.set_origin(Origin::User);
        add(&mut ed, 1.0);
        assert_eq!(ed.origins_above(s[0]), vec![Origin::User]);
    }
}
