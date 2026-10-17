//! Reviewing what an agent changed (AGR-05). Logic only, without egui.
//!
//! The activity log (`kadakas_core::activity`) holds, for each step of the agent, what each
//! object looked like before and after. [`ReviewSet`] adds the steps that the person has not
//! marked as seen into one list of changes, by the rules of the compare (REV-03): added, changed
//! and removed. It is read against the document as it is now, so it never claims more than is
//! true:
//!
//! - an object the agent made counts as added while it exists;
//! - an object the agent changed counts as changed while it differs from how it was before the
//!   first step (a change that was put back is no change);
//! - an object the agent removed counts as removed while it is gone (the ghost shows how it was);
//! - an object that the person deleted after the agent changed it is not listed: that is a change
//!   of the person.
//!
//! The same set gives the overlay of the compare (`CompareOverlay`), so the plan draws agent
//! changes with the colors, patterns and marks of REV-03.

use glam::DVec2;
use kadakas_core::activity::Step;
use kadakas_core::command::Counts;
use kadakas_core::model::{Document, Object, ObjectId};
use kadakas_review::overlay::{Change, CompareOverlay, Status};
use serde_json::Value;

/// What the review shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Every step that is not marked as seen.
    Unseen,
    /// One step (chosen in the Activity panel), seen or not.
    Step(u64),
}

/// One object of the review.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: ObjectId,
    pub status: Status,
    /// The newest step of the review that touched it.
    pub step: u64,
    /// How it was before the first step of the review (`None` for an added object).
    pub before: Option<Object>,
    /// How it is now (`None` for a removed object).
    pub after: Option<Object>,
    /// The person (or another user) changed the object after the agent: what is drawn is not
    /// what the agent left.
    pub edited_later: bool,
}

/// The changes of a scope, read against a document.
#[derive(Clone, Debug, Default)]
pub struct ReviewSet {
    entries: Vec<Entry>,
    /// The steps that went in.
    steps: Vec<u64>,
    /// Steps of the scope that have no object states any more (the log's bound dropped them): the
    /// review cannot show them.
    pub missing: usize,
}

impl ReviewSet {
    /// Adds the steps (oldest first) and reads them against `doc`.
    pub fn build<'a>(steps: impl IntoIterator<Item = &'a Step>, doc: &Document) -> ReviewSet {
        // Per object: how it was before the first step, how the last step left it, and which
        // step touched it last.
        let mut order: Vec<ObjectId> = Vec::new();
        let mut net: std::collections::HashMap<ObjectId, (Option<Object>, Option<Object>, u64)> =
            std::collections::HashMap::new();
        let mut set = ReviewSet::default();
        for step in steps {
            let Some(data) = &step.data else {
                set.missing += 1;
                continue;
            };
            set.steps.push(step.id);
            for c in &data.objects {
                match net.get_mut(&c.id) {
                    Some(e) => {
                        e.1 = c.after.clone();
                        e.2 = step.id;
                    }
                    None => {
                        order.push(c.id);
                        net.insert(c.id, (c.before.clone(), c.after.clone(), step.id));
                    }
                }
            }
        }
        for id in order {
            let Some((first_before, last_after, step)) = net.remove(&id) else {
                continue;
            };
            let now = doc.object(id);
            // What the agent did is read from the agent's own states (before the first step,
            // after the last). What the person did to the object afterwards is told apart: it
            // never turns an unchanged object of the agent into a change of the agent.
            let entry = match (first_before, last_after, now) {
                // Made by the agent, and still there.
                (None, Some(agent), Some(now)) => {
                    Some((Status::Added, None, Some(now.clone()), *now != agent))
                }
                // Made and removed by the agent, or deleted by the person since.
                (None, _, _) => None,
                // Removed by the agent, and still gone.
                (Some(before), None, None) => Some((Status::Removed, Some(before), None, false)),
                // Removed by the agent and back again (a revert, or the person made it again).
                (Some(_), None, Some(_)) => None,
                // Changed by the agent, and deleted by the person since.
                (Some(_), Some(_), None) => None,
                (Some(before), Some(agent), Some(now)) => (before != agent).then(|| {
                    (
                        Status::Changed,
                        Some(before),
                        Some(now.clone()),
                        *now != agent,
                    )
                }),
            };
            if let Some((status, before, after, edited_later)) = entry {
                set.entries.push(Entry {
                    id,
                    status,
                    step,
                    before,
                    after,
                    edited_later,
                });
            }
        }
        set
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn steps(&self) -> &[u64] {
        &self.steps
    }

    pub fn get(&self, id: ObjectId) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// How many objects were changed again after the agent.
    pub fn edited_later(&self) -> usize {
        self.entries.iter().filter(|e| e.edited_later).count()
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for e in &self.entries {
            match e.status {
                Status::Added => c.added += 1,
                Status::Changed => c.changed += 1,
                Status::Removed => c.removed += 1,
            }
        }
        c
    }

    /// The ids of one status, in the order of the changes.
    pub fn ids(&self, status: Status) -> Vec<ObjectId> {
        self.entries
            .iter()
            .filter(|e| e.status == status)
            .map(|e| e.id)
            .collect()
    }

    /// The overlay of the compare for the plan: the colors, patterns and marks of REV-03.
    pub fn overlay(&self) -> CompareOverlay {
        CompareOverlay::from_changes(self.entries.iter().map(|e| Change {
            id: e.id,
            status: e.status,
            old: e.before.clone(),
            new: e.after.clone(),
        }))
    }

    /// The box around the objects (drawing mm), as they are now or, for removed ones, as they
    /// were. `None` if no object has a box.
    pub fn bounds(&self, doc: &Document, ids: &[ObjectId]) -> Option<(DVec2, DVec2)> {
        let mut out: Option<(DVec2, DVec2)> = None;
        for id in ids {
            let Some(e) = self.get(*id) else { continue };
            let Some(o) = e.after.as_ref().or(e.before.as_ref()) else {
                continue;
            };
            let (lo, hi) = doc.object_bounds(o);
            if !(lo.is_finite() && hi.is_finite()) {
                continue;
            }
            out = Some(match out {
                Some((a, b)) => (a.min(lo), b.max(hi)),
                None => (lo, hi),
            });
        }
        out
    }

    /// The object after `current` in the list (the first when `current` is `None` or last), for
    /// a key that steps through the changes.
    pub fn next_after(&self, current: Option<ObjectId>) -> Option<ObjectId> {
        let at = current.and_then(|c| self.entries.iter().position(|e| e.id == c));
        let next = at.map_or(0, |i| i + 1);
        self.entries
            .get(next)
            .or_else(|| self.entries.first())
            .map(|e| e.id)
    }

    /// The object before `current` in the list.
    pub fn prev_before(&self, current: Option<ObjectId>) -> Option<ObjectId> {
        let at = current.and_then(|c| self.entries.iter().position(|e| e.id == c));
        match at {
            Some(0) | None => self.entries.last(),
            Some(i) => self.entries.get(i - 1),
        }
        .map(|e| e.id)
    }
}

/// The box around everything a step touched (drawing mm), as it is now or, for removed objects,
/// as it was.
pub fn step_bounds(step: &Step, doc: &Document) -> Option<(DVec2, DVec2)> {
    let data = step.data.as_ref()?;
    let mut out: Option<(DVec2, DVec2)> = None;
    for c in &data.objects {
        let o = doc
            .object(c.id)
            .or(c.after.as_ref())
            .or(c.before.as_ref())?;
        let (lo, hi) = doc.object_bounds(o);
        if !(lo.is_finite() && hi.is_finite()) {
            continue;
        }
        out = Some(match out {
            Some((a, b)) => (a.min(lo), b.max(hi)),
            None => (lo, hi),
        });
    }
    out
}

// ---------------------------------------------------------------- the diff of one object

/// The longest value text of a [`PropChange`], in characters.
pub const MAX_VALUE_CHARS: usize = 60;
/// The most property changes that [`object_diff`] lists.
pub const MAX_PROPS: usize = 40;

/// One property of an object that differs between two states.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropChange {
    /// The property, for example `shape.a`, `attrs.pen` or `layer`.
    pub path: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

fn show(v: &Value) -> String {
    let t = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if t.chars().count() > MAX_VALUE_CHARS {
        let cut: String = t.chars().take(MAX_VALUE_CHARS).collect();
        format!("{cut}...")
    } else {
        t
    }
}

fn walk(path: &str, a: Option<&Value>, b: Option<&Value>, depth: usize, out: &mut Vec<PropChange>) {
    if a == b || out.len() >= MAX_PROPS {
        return;
    }
    if let (Some(Value::Object(x)), Some(Value::Object(y))) = (a, b)
        && depth < 3
    {
        let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
        keys.sort();
        keys.dedup();
        for k in keys {
            walk(&format!("{path}.{k}"), x.get(k), y.get(k), depth + 1, out);
        }
        return;
    }
    out.push(PropChange {
        path: path.trim_start_matches('.').to_string(),
        old: a.map(show),
        new: b.map(show),
    });
}

/// The properties that differ between the states of an object, with long values cut. The
/// paths are those of the JSON of the object (`shape.a`, `attrs.pen`, `layer`, `data.<key>`).
pub fn object_diff(before: &Object, after: &Object) -> Vec<PropChange> {
    let (Ok(a), Ok(b)) = (serde_json::to_value(before), serde_json::to_value(after)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk("", Some(&a), Some(&b), 0, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::activity::{ActivityLog, Recorded, StepKind};
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::model::Shape;

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

    fn step(log: &mut ActivityLog, ed: &mut Editor, f: impl FnOnce(&mut Editor)) -> u64 {
        log.touch(1, 0);
        let mark = ed.serial_mark();
        f(ed);
        let serials = ed.steps_since(mark);
        let data = ed.step_data(&serials).unwrap();
        log.record(Recorded {
            conn: 1,
            method: "batch".into(),
            time_ms: 0,
            revision: ed.revision(),
            serials,
            data,
            partial: false,
            kind: StepKind::Call,
        })
        .unwrap()
    }

    fn review(log: &ActivityLog, ed: &Editor, scope: Scope) -> ReviewSet {
        match scope {
            Scope::Unseen => ReviewSet::build(log.steps().iter().filter(|s| !s.seen), ed.doc()),
            Scope::Step(id) => {
                ReviewSet::build(log.steps().iter().filter(|s| s.id == id), ed.doc())
            }
        }
    }

    #[test]
    fn added_changed_and_removed_are_told_apart() {
        let mut ed = Editor::with_sequential_ids();
        let (a, b) = (add(&mut ed, 0.0), add(&mut ed, 5.0));
        let mut log = ActivityLog::default();
        let mut made = None;
        step(&mut log, &mut ed, |e| {
            made = Some(add(e, 9.0));
            mv(e, a, 2.0);
            e.execute(&Command::DeleteObjects { ids: vec![b] }).unwrap();
        });
        let r = review(&log, &ed, Scope::Unseen);
        let c = r.counts();
        assert_eq!((c.added, c.changed, c.removed), (1, 1, 1));
        assert_eq!(r.get(made.unwrap()).unwrap().status, Status::Added);
        assert_eq!(r.get(a).unwrap().status, Status::Changed);
        let removed = r.get(b).unwrap();
        assert_eq!(removed.status, Status::Removed);
        assert!(removed.before.is_some() && removed.after.is_none());
        let overlay = r.overlay();
        assert_eq!(overlay.status_of(a), Some(Status::Changed));
        assert_eq!(overlay.total(), 3);
    }

    #[test]
    fn two_steps_on_one_object_read_as_one_change_from_the_first_before() {
        let mut ed = Editor::with_sequential_ids();
        let a = add(&mut ed, 0.0);
        let mut log = ActivityLog::default();
        step(&mut log, &mut ed, |e| mv(e, a, 2.0));
        let second = step(&mut log, &mut ed, |e| mv(e, a, 3.0));
        let r = review(&log, &ed, Scope::Unseen);
        let e = r.get(a).unwrap();
        assert_eq!(e.step, second);
        assert_eq!(
            e.before.as_ref().unwrap().shape,
            Shape::Line {
                a: DVec2::new(0.0, 0.0),
                b: DVec2::new(1.0, 1.0)
            }
        );
        assert_eq!(r.counts().changed, 1);
    }

    #[test]
    fn a_change_that_was_put_back_is_no_change() {
        let mut ed = Editor::with_sequential_ids();
        let a = add(&mut ed, 0.0);
        let mut log = ActivityLog::default();
        step(&mut log, &mut ed, |e| mv(e, a, 2.0));
        step(&mut log, &mut ed, |e| mv(e, a, -2.0));
        assert!(review(&log, &ed, Scope::Unseen).is_empty());
    }

    #[test]
    fn what_the_person_deleted_afterwards_is_not_the_agents_removal() {
        let mut ed = Editor::with_sequential_ids();
        let a = add(&mut ed, 0.0);
        let mut log = ActivityLog::default();
        let mut made = None;
        step(&mut log, &mut ed, |e| {
            mv(e, a, 2.0);
            made = Some(add(e, 7.0));
        });
        ed.execute(&Command::DeleteObjects {
            ids: vec![a, made.unwrap()],
        })
        .unwrap();
        assert!(review(&log, &ed, Scope::Unseen).is_empty());
    }

    #[test]
    fn seen_steps_leave_the_review_and_a_step_scope_shows_one_step() {
        let mut ed = Editor::with_sequential_ids();
        let mut log = ActivityLog::default();
        let s1 = step(&mut log, &mut ed, |e| {
            add(e, 0.0);
        });
        let s2 = step(&mut log, &mut ed, |e| {
            add(e, 5.0);
            add(e, 6.0);
        });
        assert_eq!(review(&log, &ed, Scope::Unseen).counts().added, 3);
        log.set_seen(s1, true);
        assert_eq!(review(&log, &ed, Scope::Unseen).counts().added, 2);
        assert_eq!(review(&log, &ed, Scope::Step(s1)).counts().added, 1);
        assert_eq!(review(&log, &ed, Scope::Step(s2)).steps(), &[s2]);
        log.set_seen(s2, true);
        assert!(review(&log, &ed, Scope::Unseen).is_empty());
    }

    #[test]
    fn stepping_through_the_changes_wraps_around() {
        let mut ed = Editor::with_sequential_ids();
        let mut log = ActivityLog::default();
        step(&mut log, &mut ed, |e| {
            add(e, 0.0);
            add(e, 5.0);
        });
        let r = review(&log, &ed, Scope::Unseen);
        let ids: Vec<ObjectId> = r.entries().iter().map(|e| e.id).collect();
        assert_eq!(r.next_after(None), Some(ids[0]));
        assert_eq!(r.next_after(Some(ids[0])), Some(ids[1]));
        assert_eq!(r.next_after(Some(ids[1])), Some(ids[0]));
        assert_eq!(r.prev_before(Some(ids[0])), Some(ids[1]));
        assert_eq!(r.prev_before(None), Some(ids[1]));
    }

    #[test]
    fn the_box_of_a_removed_object_is_where_it_was() {
        let mut ed = Editor::with_sequential_ids();
        let a = add(&mut ed, 100.0);
        let mut log = ActivityLog::default();
        step(&mut log, &mut ed, |e| {
            e.execute(&Command::DeleteObjects { ids: vec![a] }).unwrap();
        });
        let r = review(&log, &ed, Scope::Unseen);
        let (lo, hi) = r.bounds(ed.doc(), &[a]).unwrap();
        assert!(lo.x <= 100.0 && hi.x >= 101.0);
    }

    #[test]
    fn the_box_of_a_step_covers_new_changed_and_removed_objects() {
        let mut ed = Editor::with_sequential_ids();
        let (a, b) = (add(&mut ed, 0.0), add(&mut ed, 1000.0));
        let mut log = ActivityLog::default();
        let id = step(&mut log, &mut ed, |e| {
            mv(e, a, 10.0);
            e.execute(&Command::DeleteObjects { ids: vec![b] }).unwrap();
            add(e, 5000.0);
        });
        let s = log.get(id).unwrap();
        let (lo, hi) = step_bounds(s, ed.doc()).unwrap();
        assert!(lo.x <= 10.0 && hi.x >= 5001.0, "{lo:?} {hi:?}");
        let mut empty = s.clone();
        empty.data = None;
        assert_eq!(step_bounds(&empty, ed.doc()), None);
    }

    #[test]
    fn the_diff_of_an_object_lists_the_properties_that_differ() {
        let mut ed = Editor::with_sequential_ids();
        let a = add(&mut ed, 0.0);
        let before = ed.doc().object(a).unwrap().clone();
        mv(&mut ed, a, 5.0);
        let after = ed.doc().object(a).unwrap().clone();
        let d = object_diff(&before, &after);
        assert!(d.iter().any(|p| p.path == "shape.a"), "{d:?}");
        assert!(d.iter().all(|p| p.old != p.new));
        assert!(object_diff(&before, &before).is_empty());
    }
}
