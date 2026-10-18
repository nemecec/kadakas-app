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
    /// What changed in the resource, when the step changed exactly one resource and added or
    /// removed none.
    pub resource_detail: Option<crate::activity::detail::ResourceDetail>,
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

    /// The change the other way round: what an undo of the step did.
    pub fn inverted(&self) -> StepData {
        let flip = |c: Counts| Counts {
            added: c.removed,
            changed: c.changed,
            removed: c.added,
        };
        StepData {
            objects: self
                .objects
                .iter()
                .map(|c| ObjChange {
                    id: c.id,
                    before: c.after.clone(),
                    after: c.before.clone(),
                    index: None,
                })
                .collect(),
            layers: flip(self.layers),
            classes: flip(self.classes),
            resources: flip(self.resources),
            layers_added: Vec::new(),
            document: self.document,
            versions: self.versions,
            issues: self.issues,
            resource_detail: None,
        }
    }

    /// A rough size in bytes, for the budget of the activity log.
    pub fn approx_bytes(&self) -> u64 {
        self.objects
            .iter()
            .map(|c| {
                c.before.as_ref().map_or(0, object_bytes) + c.after.as_ref().map_or(0, object_bytes)
            })
            .sum::<u64>()
            + 256
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
    /// Its layer or class is grey (the editor refuses to edit it).
    Grey,
    /// Something it refers to is gone: a resource, its group, or an object it is anchored to.
    MissingRef,
    /// Something made after the step hangs on it (a data tag, the wall of a pad, a dimension, a
    /// member of a group, a site model); removing it would delete or change that work.
    UsedLater,
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
    /// Every object of the step is as it was before the step, or gone: there is nothing to do.
    NothingToDo,
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
            Refusal::NothingToDo => write!(
                f,
                "nothing to revert: the objects of the step are back as they were, or gone"
            ),
            Refusal::Conflicts(list) => {
                let mut counts = [0usize; 9];
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
                    "on a grey layer or class",
                    "missing what they refer to",
                    "used by work made later",
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
    // The resources that changed: the state before the first change and after the last.
    let mut res_net: IndexMap<ResourceId, (Resource, Resource)> = IndexMap::new();
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
        if let Some((old, new)) = &p.meta {
            for id in &cs.changed.resources {
                let (Some(b), Some(a)) = (old.resources.get(*id), new.resources.get(*id)) else {
                    continue;
                };
                // A picture or an imported drawing is large and has nothing to name.
                if matches!(a.body, ResourceBody::Image(_) | ResourceBody::ImportBase(_)) {
                    continue;
                }
                res_net
                    .entry(*id)
                    .and_modify(|e| e.1 = a.clone())
                    .or_insert_with(|| (b.clone(), a.clone()));
            }
        }
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
    if res_net.len() == 1 && data.resources.added == 0 && data.resources.removed == 0 {
        data.resource_detail = res_net
            .values()
            .next()
            .and_then(|(b, a)| crate::activity::detail::of_resource(b, a));
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

    /// The step that the last undo or redo moved, and whether it was a redo.
    pub fn last_moved(&self) -> Option<(u64, bool)> {
        self.last_moved
    }

    /// True if the history limit dropped a step made since the mark (the record of a call is then
    /// not complete).
    pub fn trimmed_since(&self, mark: u64) -> bool {
        self.max_trimmed_serial >= mark
    }

    /// The objects of the step that cannot go back now, and the objects that need nothing (the
    /// step made them and they are deleted since, or they are as they were before the step).
    ///
    /// Every object must be exactly as the step left it. Besides that, the ones that go back must
    /// be allowed to: the layer is there and not locked or grey, the class is not grey, what the
    /// object refers to exists, and nothing that was made later depends on an object that the
    /// revert removes (a data tag, a wall of a pad, a dimension, a member of a group, a site
    /// model that names it).
    pub fn revert_conflicts(&mut self, data: &StepData) -> (Vec<Conflict>, Vec<ObjectId>) {
        let step_ids: HashSet<ObjectId> = data.objects.iter().map(|c| c.id).collect();
        let removal: HashSet<ObjectId> = data
            .objects
            .iter()
            .filter(|c| {
                c.before.is_none()
                    && self
                        .doc
                        .object(c.id)
                        .is_some_and(|cur| Some(cur) == c.after.as_ref())
            })
            .map(|c| c.id)
            .collect();
        let used = dependants(&self.doc, &removal, &step_ids);
        // Objects that exist after the revert and were not there before it.
        let back: HashSet<ObjectId> = data
            .objects
            .iter()
            .filter(|c| c.before.is_some() && self.doc.object(c.id).is_none())
            .map(|c| c.id)
            .collect();
        let mut conflicts = Vec::new();
        let mut nothing = Vec::new();
        let tx = Tx::new(&mut self.doc);
        let refs = |o: &Object| -> Option<ConflictKind> {
            None::<ConflictKind>
                .or_else(|| tx.check_layer(o.layer).err().map(|e| kind_of(&e)))
                .or_else(|| tx.check_class(o.class).err().map(|e| kind_of(&e)))
                .or_else(|| {
                    tx.doc
                        .drawing
                        .grey
                        .classes
                        .contains(&o.class)
                        .then_some(ConflictKind::Grey)
                })
                .or_else(|| {
                    tx.check_shape_refs(&o.shape)
                        .err()
                        .map(|_| ConflictKind::MissingRef)
                })
                .or_else(|| {
                    tx.check_attr_refs(&o.attrs)
                        .err()
                        .map(|_| ConflictKind::MissingRef)
                })
                .or_else(|| {
                    let gone = |id: ObjectId| {
                        removal.contains(&id)
                            || (!back.contains(&id) && tx.doc.object(id).is_none())
                    };
                    let parent_gone = o.parent.is_some_and(gone);
                    let anchor_gone = match &o.shape {
                        Shape::Annotation(a) => a.depends_on().into_iter().any(gone),
                        _ => false,
                    };
                    (parent_gone || anchor_gone).then_some(ConflictKind::MissingRef)
                })
        };
        for ch in &data.objects {
            let current = tx.doc.object(ch.id);
            let kind = match (&ch.before, &ch.after) {
                (Some(before), None) => match current {
                    Some(c) if c == before => {
                        nothing.push(ch.id);
                        None
                    }
                    Some(_) => Some(ConflictKind::ExistsAgain),
                    None => refs(before),
                },
                (None, Some(after)) => match current {
                    None => {
                        nothing.push(ch.id);
                        None
                    }
                    Some(c) if c == after => tx
                        .editable(ch.id)
                        .err()
                        .map(|e| kind_of(&e))
                        .or_else(|| used.contains(&ch.id).then_some(ConflictKind::UsedLater)),
                    Some(_) => Some(ConflictKind::ChangedLater),
                },
                (Some(before), Some(after)) => match current {
                    None => Some(ConflictKind::DeletedLater),
                    Some(c) if c == before && c != after => {
                        nothing.push(ch.id);
                        None
                    }
                    Some(c) if c == after => tx
                        .editable(ch.id)
                        .err()
                        .map(|e| kind_of(&e))
                        .or_else(|| refs(before)),
                    Some(_) => Some(ConflictKind::ChangedLater),
                },
                (None, None) => None,
            };
            if let Some(kind) = kind {
                conflicts.push(Conflict { id: ch.id, kind });
            }
        }
        (conflicts, nothing)
    }

    /// Takes one step (or the steps of one call) back although later steps exist. See the module
    /// text for the rules. `serials` are the steps, oldest first, and `data` is what they did
    /// (kept by the activity log, so it does not depend on the history: a step that the history
    /// forgot can still go back object by object).
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
        let places: Vec<StepPlace> = serials.iter().map(|s| self.step_place(*s)).collect();
        if places.contains(&StepPlace::Redo) {
            return Err(Refusal::AlreadyUndone);
        }
        let above = places
            .iter()
            .filter_map(|p| match p {
                StepPlace::Undo(n) => Some(*n),
                _ => None,
            })
            .min()
            .unwrap_or(0);
        // On top of the history: a plain undo is exact and can be redone.
        let top = !serials.is_empty()
            && serials.len() <= self.undo.len()
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
            return Err(if conflicts.is_empty() {
                Refusal::NothingToDo
            } else {
                Refusal::Conflicts(conflicts)
            });
        }
        let step_ids: HashSet<ObjectId> = data.objects.iter().map(|c| c.id).collect();
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
                            "{USED_MARK}: object {extra} was made later and goes with the objects of the step"
                        )));
                    }
                    tx.run(
                        &Command::DeleteObjects { ids: remove.clone() },
                        &mut Outcome::default(),
                    )?;
                    // The delete takes along what hangs on the objects (data tags, walls of a
                    // pad) and changes what is anchored to them. Nothing outside the step may be
                    // touched this way.
                    let touched: Vec<ObjectId> = tx
                        .before
                        .keys()
                        .copied()
                        .chain(tx.removed.iter().map(|(_, id)| *id))
                        .collect();
                    if let Some(extra) = touched
                        .iter()
                        .find(|i| !step_ids.contains(i) && !remove.contains(i))
                    {
                        return Err(CommandError::Invalid(format!(
                            "{USED_MARK}: object {extra} was made later and depends on the objects of the step"
                        )));
                    }
                }
                let mut restored: HashSet<ObjectId> = HashSet::new();
                for c in todo
                    .iter()
                    .filter(|c| c.before.is_some() && c.after.is_some())
                {
                    tx.editable(c.id)?;
                    tx.touch(c.id);
                    if let (Some(slot), Some(before)) = (tx.doc.object_mut(c.id), &c.before) {
                        *slot = before.clone();
                    }
                    restored.insert(c.id);
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
                    restored.insert(o.id);
                }
                // What follows the objects (dimensions, callouts, tags, the wall of a pad) is
                // made again from the objects as they are now, as after any other command.
                if !restored.is_empty() {
                    tx.annot_sync(&restored, false, false);
                    for id in &restored {
                        tx.retaining_sync(*id);
                    }
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
            Err(CommandError::Invalid(why)) if why.starts_with(USED_MARK) => {
                Err(Refusal::InUse(why))
            }
            Err(e) => Err(Refusal::Rejected(e.to_string())),
        }
    }
}

/// The start of the text of an error that means "something made later hangs on these objects".
const USED_MARK: &str = "used later";

fn kind_of(e: &CommandError) -> ConflictKind {
    match e {
        CommandError::LayerLocked(_) => ConflictKind::LayerLocked,
        CommandError::NoLayer(_) => ConflictKind::LayerGone,
        CommandError::NoClass(_) => ConflictKind::ClassGone,
        CommandError::Invalid(m) if m.contains("grey") => ConflictKind::Grey,
        _ => ConflictKind::MissingRef,
    }
}

/// The objects among `removal` that something outside the step hangs on: a member of a group, a
/// data tag, a dimension or callout anchored to it, the wall of a pad, a site model that names
/// it. A revert that removed them would delete or change that work.
fn dependants(
    doc: &Document,
    removal: &HashSet<ObjectId>,
    step_ids: &HashSet<ObjectId>,
) -> HashSet<ObjectId> {
    let mut used = HashSet::new();
    if removal.is_empty() {
        return used;
    }
    for o in doc.objects.values() {
        if removal.contains(&o.id) || step_ids.contains(&o.id) {
            continue;
        }
        let mut hit = |id: ObjectId| {
            if removal.contains(&id) {
                used.insert(id);
            }
        };
        if let Some(p) = o.parent {
            hit(p);
        }
        if let Shape::Annotation(a) = &o.shape {
            for d in a.depends_on() {
                hit(d);
            }
        }
        for key in [
            super::retaining_ops::WALL_KEY,
            crate::site::SITE_KEY,
            super::retaining_ops::PAD_KEY,
        ] {
            if let Some(v) = o.data.get(key) {
                let uid = v.as_str().and_then(crate::uid::Uid::parse).or_else(|| {
                    v.get("wall")
                        .and_then(|w| w.as_str())
                        .and_then(crate::uid::Uid::parse)
                });
                if let Some(u) = uid {
                    hit(ObjectId(u));
                }
            }
        }
    }
    used
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
    fn work_that_hangs_on_an_object_of_the_step_blocks_its_removal() {
        let mut ed = Editor::with_sequential_ids();
        let (s, d) = step(&mut ed, |e| {
            let a = add(e, 0.0);
            let b = add(e, 10.0);
            e.execute(&Command::Group {
                ids: vec![a, b],
                name: None,
            })
            .unwrap();
        });
        let group = d
            .objects
            .iter()
            .find(|o| o.after.as_ref().is_some_and(|a| a.shape.kind() == "group"))
            .unwrap()
            .id;
        // The person draws inside the group of the agent: the group itself is unchanged.
        ed.execute(&Command::AddObjectIn {
            parent: group,
            shape: Shape::Line {
                a: DVec2::new(50.0, 0.0),
                b: DVec2::new(51.0, 1.0),
            },
            class: None,
            attrs: Default::default(),
            id: None,
        })
        .unwrap();
        let before = ed.doc().clone();
        let err = ed.revert_step(&s, &d, false, true).unwrap_err();
        match err {
            Refusal::Conflicts(list) => {
                assert!(
                    list.iter()
                        .any(|c| c.id == group && c.kind == ConflictKind::UsedLater),
                    "{list:?}"
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(ed.doc(), &before, "nothing changed, nothing deleted");
    }

    #[test]
    fn a_step_that_the_history_forgot_goes_back_object_by_object() {
        let mut ed = Editor::with_sequential_ids();
        ed.set_history_limit(1);
        let (s, d) = step(&mut ed, |e| {
            add(e, 0.0);
        });
        add(&mut ed, 50.0);
        add(&mut ed, 60.0);
        assert_eq!(ed.step_place(s[0]), StepPlace::Gone);
        let r = ed.revert_step(&s, &d, false, true).unwrap();
        assert_eq!(r.reverted.len(), 1);
        assert_eq!(ed.doc().objects.len(), 2);
        // Again: every object is back as it was.
        assert_eq!(
            ed.revert_step(&s, &d, false, true),
            Err(Refusal::NothingToDo)
        );
    }

    #[test]
    fn what_a_step_would_not_need_to_do_is_not_a_success() {
        let mut ed = Editor::with_sequential_ids();
        let (s, d) = step(&mut ed, |e| {
            add(e, 0.0);
        });
        let id = d.objects[0].id;
        add(&mut ed, 5.0);
        ed.execute(&Command::DeleteObjects { ids: vec![id] })
            .unwrap();
        assert_eq!(
            ed.revert_step(&s, &d, false, true),
            Err(Refusal::NothingToDo)
        );
    }

    #[test]
    fn every_refusal_and_conflict_reads_as_a_sentence() {
        let id = ObjectId::from_num(7);
        let kinds = [
            ConflictKind::ChangedLater,
            ConflictKind::DeletedLater,
            ConflictKind::ExistsAgain,
            ConflictKind::LayerGone,
            ConflictKind::LayerLocked,
            ConflictKind::ClassGone,
            ConflictKind::Grey,
            ConflictKind::MissingRef,
            ConflictKind::UsedLater,
        ];
        let list: Vec<Conflict> = kinds.iter().map(|k| Conflict { id, kind: *k }).collect();
        let text = Refusal::Conflicts(list).to_string();
        for word in [
            "changed later",
            "deleted later",
            "exist again",
            "layer that is gone",
            "locked layer",
            "class that is gone",
            "grey",
            "missing what they refer to",
            "used by work made later",
        ] {
            assert!(text.contains(word), "{word}: {text}");
        }
        for r in [
            Refusal::Gone,
            Refusal::AlreadyUndone,
            Refusal::NothingToDo,
            Refusal::Snapshot { later: 2 },
            Refusal::Shared,
            Refusal::InUse("used later: object x".into()),
            Refusal::Rejected("no".into()),
        ] {
            assert!(!r.to_string().is_empty());
        }
        assert_eq!(
            kind_of(&CommandError::LayerLocked(LayerId::from_num(1))),
            ConflictKind::LayerLocked
        );
        assert_eq!(
            kind_of(&CommandError::NoLayer(LayerId::from_num(1))),
            ConflictKind::LayerGone
        );
        assert_eq!(
            kind_of(&CommandError::NoClass(ClassId::from_num(1))),
            ConflictKind::ClassGone
        );
        assert_eq!(
            kind_of(&CommandError::Invalid("layer x is grey".into())),
            ConflictKind::Grey
        );
        assert_eq!(
            kind_of(&CommandError::Invalid("other".into())),
            ConflictKind::MissingRef
        );
    }

    #[test]
    fn a_restored_object_needs_its_layer_group_and_class() {
        let mut ed = Editor::with_sequential_ids();
        let a = add(&mut ed, 0.0);
        let (s, d) = step(&mut ed, |e| {
            e.execute(&Command::DeleteObjects { ids: vec![a] }).unwrap();
        });
        // The layer of the object is locked since: it cannot come back.
        let layer = d.objects[0].before.as_ref().unwrap().layer;
        add(&mut ed, 9.0);
        ed.execute(&Command::ModifyLayer {
            id: layer,
            name: None,
            visible: None,
            locked: Some(true),
            elevation: None,
            scale: None,
        })
        .unwrap();
        match ed.revert_step(&s, &d, false, true) {
            Err(Refusal::Conflicts(l)) => assert_eq!(l[0].kind, ConflictKind::LayerLocked),
            other => panic!("{other:?}"),
        }
        ed.execute(&Command::ModifyLayer {
            id: layer,
            name: None,
            visible: None,
            locked: Some(false),
            elevation: None,
            scale: None,
        })
        .unwrap();
        let r = ed.revert_step(&s, &d, false, true).unwrap();
        assert_eq!(r.reverted, vec![a]);
        assert!(ed.doc().object(a).is_some());
        // An object with this ID exists again: no second one is made.
        let (s2, d2) = step(&mut ed, |e| {
            e.execute(&Command::DeleteObjects { ids: vec![a] }).unwrap();
        });
        let again = d2.objects[0].before.clone().unwrap();
        ed.execute(&Command::AddObject {
            shape: again.shape.clone(),
            layer: None,
            class: None,
            attrs: Default::default(),
        })
        .unwrap();
        // (a new ID, so the old one stays free and comes back)
        assert!(ed.revert_step(&s2, &d2, false, true).is_ok());
    }

    /// The agent makes `p` and a free object `f` in one step; `setup` adds work of the person
    /// that hangs on `p` and gives its id. The revert must refuse, name `p`, and change nothing;
    /// "the others" must revert `f` and leave `p` and the work of the person as they are.
    fn the_work_of_the_person_hangs_on(setup: impl FnOnce(&mut Editor, ObjectId) -> ObjectId) {
        let mut ed = Editor::with_sequential_ids();
        let mut ids = None;
        let (s, d) = step(&mut ed, |e| {
            ids = Some((add(e, 0.0), add(e, 500.0)));
        });
        let (p, f) = ids.unwrap();
        let dependant = setup(&mut ed, p);
        let before = ed.doc().clone();
        match ed.revert_step(&s, &d, false, true) {
            Err(Refusal::Conflicts(list)) => assert!(
                list.iter()
                    .any(|c| c.id == p && c.kind == ConflictKind::UsedLater),
                "{list:?}"
            ),
            other => panic!("{other:?}"),
        }
        assert_eq!(ed.doc(), &before, "nothing was deleted or changed");
        let r = ed.revert_step(&s, &d, true, true).unwrap();
        assert_eq!(r.reverted, vec![f]);
        assert!(r.skipped.iter().any(|c| c.id == p));
        assert!(ed.doc().object(p).is_some(), "the object stays");
        assert!(ed.doc().object(f).is_none(), "the free object went");
        assert_eq!(
            ed.doc().object(dependant),
            before.object(dependant),
            "the work of the person is untouched"
        );
    }

    #[test]
    fn a_data_tag_of_the_person_on_an_object_of_the_step_blocks_its_removal() {
        use crate::annot::style::{AnnotResource, TagStyle};
        use crate::annot::{Annot, DataTag};
        the_work_of_the_person_hangs_on(|ed, p| {
            ed.execute(&Command::AddResource {
                name: "Tags".into(),
                body: crate::resource::ResourceBody::Annot(Box::new(AnnotResource::TagStyle(
                    TagStyle::default(),
                ))),
            })
            .unwrap();
            let style = ed.doc().resources.iter().next().unwrap().id;
            let tag = DataTag {
                style,
                object: p,
                offset: DVec2::new(1.0, 1.0),
                at: DVec2::ZERO,
                anchor: DVec2::ZERO,
            };
            let out = ed
                .execute(&Command::AddObject {
                    shape: Shape::Annotation(Box::new(Annot::DataTag(tag))),
                    layer: None,
                    class: None,
                    attrs: Default::default(),
                })
                .unwrap();
            ObjectId(out.created[0])
        });
    }

    #[test]
    fn the_wall_of_a_pad_of_the_person_blocks_the_removal_of_the_pad() {
        the_work_of_the_person_hangs_on(|ed, pad| {
            let wall = add(ed, 900.0);
            ed.execute(&Command::SetData {
                id: wall,
                set: [(
                    super::super::retaining_ops::WALL_KEY.to_string(),
                    serde_json::json!(pad.0.to_string()),
                )]
                .into(),
                remove: vec![],
            })
            .unwrap();
            wall
        });
    }

    #[test]
    fn a_pad_of_the_person_that_names_the_wall_of_the_step_blocks_its_removal() {
        the_work_of_the_person_hangs_on(|ed, wall| {
            let pad = add(ed, 900.0);
            ed.execute(&Command::SetData {
                id: pad,
                set: [(
                    super::super::retaining_ops::PAD_KEY.to_string(),
                    serde_json::json!({"wall": wall.0.to_string()}),
                )]
                .into(),
                remove: vec![],
            })
            .unwrap();
            pad
        });
    }

    fn dimension_on(ed: &mut Editor, p: ObjectId) -> ObjectId {
        use crate::annot::{Anchor, AnchorTarget, Annot, Axis, DimMode, Dimension};
        let out = ed
            .execute(&Command::AddObject {
                shape: Shape::Annotation(Box::new(Annot::Dimension(Dimension {
                    mode: DimMode::Linear {
                        axis: Axis::Horizontal,
                    },
                    points: vec![DVec2::new(0.0, 0.0), DVec2::new(1000.0, 0.0)],
                    anchors: vec![
                        Some(Anchor::new(p, AnchorTarget::Vertex { index: 0 })),
                        None,
                    ],
                    offset: 5.0,
                    style: None,
                    text: None,
                    detached: false,
                }))),
                layer: None,
                class: None,
                attrs: Default::default(),
            })
            .unwrap();
        ObjectId(out.created[0])
    }

    #[test]
    fn a_dimension_of_the_person_anchored_to_an_object_of_the_step_blocks_its_removal() {
        the_work_of_the_person_hangs_on(dimension_on);
    }

    #[test]
    fn reverting_a_moved_object_makes_a_dimension_of_the_person_follow() {
        use crate::annot::Annot;
        let mut ed = Editor::with_sequential_ids();
        let p = add(&mut ed, 0.0);
        let (s, d) = step(&mut ed, |e| mv(e, p, 300.0));
        // The dimension is drawn after the step, on the moved object.
        let dim = dimension_on(&mut ed, p);
        add(&mut ed, 9000.0);
        let first_point = |ed: &Editor| match &ed.doc().object(dim).unwrap().shape {
            Shape::Annotation(a) => match &**a {
                Annot::Dimension(x) => x.points[0],
                _ => panic!("not a dimension"),
            },
            _ => panic!("not an annotation"),
        };
        let vertex = |ed: &Editor| match &ed.doc().object(p).unwrap().shape {
            Shape::Line { a, .. } => *a,
            _ => panic!("not a line"),
        };
        assert_eq!(
            first_point(&ed),
            vertex(&ed),
            "anchored to the moved vertex"
        );
        let r = ed.revert_step(&s, &d, false, true).unwrap();
        assert_eq!(r.reverted, vec![p]);
        assert_eq!(vertex(&ed), DVec2::new(0.0, 0.0));
        assert_eq!(
            first_point(&ed),
            vertex(&ed),
            "the dimension follows the object that went back"
        );
    }

    #[test]
    fn the_inverted_change_swaps_before_and_after() {
        let mut ed = Editor::with_sequential_ids();
        let (_, d) = step(&mut ed, |e| {
            add(e, 0.0);
        });
        let inv = d.inverted();
        assert_eq!(inv.object_counts().removed, 1);
        assert_eq!(inv.object_counts().added, 0);
        assert!(d.approx_bytes() > 0);
    }

    #[test]
    fn the_editor_tells_which_step_the_last_undo_moved_and_what_the_limit_dropped() {
        let mut ed = Editor::with_sequential_ids();
        let (s, _) = step(&mut ed, |e| {
            add(e, 0.0);
        });
        assert!(ed.undo());
        assert_eq!(ed.last_moved(), Some((s[0], false)));
        assert!(ed.redo());
        assert_eq!(ed.last_moved(), Some((s[0], true)));
        let mark = ed.serial_mark();
        assert!(!ed.trimmed_since(mark));
        ed.set_history_limit(1);
        add(&mut ed, 5.0);
        add(&mut ed, 6.0);
        assert!(ed.trimmed_since(mark));
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
