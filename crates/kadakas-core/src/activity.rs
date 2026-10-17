//! The activity log of an agent (AGR-06): one entry for each call or script run that changed the
//! document, with the time, the method, a summary of the change set and what each object looked
//! like before and after.
//!
//! The log belongs to a session (`kadakas-api`), not to the document: it is not saved and is
//! cleared when the document is replaced. It is bounded in the number of steps and in the number
//! of object states it keeps; an old step that lost its states stays in the list with its counts
//! and cannot be reverted (the entry says so). The revert itself is `Editor::revert_step`, which
//! checks the states against the document.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::command::{Counted, Counts, StepData};
use crate::model::ObjectId;

pub mod detail;
pub use detail::Detail;

/// The most steps the log keeps.
pub const MAX_STEPS: usize = 500;
/// The most object states (before and after of one object count once) that the steps keep. The
/// oldest steps lose their states first.
pub const MAX_OBJECT_STATES: usize = 100_000;
/// The most bytes (an estimate) that the object states of all steps keep together. The oldest
/// steps lose their states first, and a single step above the budget loses its own.
pub const MAX_STATE_BYTES: u64 = 256 * 1024 * 1024;
/// An agent that is silent this long (ms) starts a new session at its next call.
pub const SESSION_GAP_MS: u64 = 30 * 60 * 1000;

/// What a step did, in numbers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub objects: Counts,
    /// The kinds of shape among the objects, most first (at most 4, `others` counts the rest).
    pub kinds: Vec<(String, usize)>,
    pub others: usize,
    pub layers: Counts,
    pub classes: Counts,
    pub resources: Counts,
    pub layers_added: Vec<String>,
    pub document: bool,
    pub versions: bool,
    pub issues: bool,
    /// What changed, when the step changed one object and nothing else.
    pub detail: Option<Detail>,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl Summary {
    pub fn of(data: &StepData) -> Summary {
        let mut kinds: HashMap<&'static str, usize> = HashMap::new();
        for c in &data.objects {
            if let Some(o) = c.after.as_ref().or(c.before.as_ref()) {
                *kinds.entry(o.shape.kind()).or_default() += 1;
            }
        }
        let mut list: Vec<(String, usize)> =
            kinds.into_iter().map(|(k, n)| (k.to_string(), n)).collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let others = list.iter().skip(4).map(|(_, n)| n).sum();
        list.truncate(4);
        let objects = data.object_counts();
        let only_one = objects.changed == 1
            && objects.added == 0
            && objects.removed == 0
            && data.layers.is_empty()
            && data.classes.is_empty()
            && data.resources.is_empty()
            && !data.document
            && !data.versions
            && !data.issues;
        let detail = if only_one {
            data.objects
                .iter()
                .find_map(|c| match (&c.before, &c.after) {
                    (Some(b), Some(a)) => detail::of(b, a),
                    _ => None,
                })
        } else {
            None
        };
        Summary {
            detail,
            objects: data.object_counts(),
            kinds: list,
            others,
            layers: data.layers,
            classes: data.classes,
            resources: data.resources,
            layers_added: data.layers_added.clone(),
            document: data.document,
            versions: data.versions,
            issues: data.issues,
        }
    }

    /// The summary as a short English text, for the API. The app writes the same facts in the
    /// language of the person from the numbers.
    pub fn text(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        let kinds = |c: usize| -> String {
            if self.kinds.is_empty() || c == 0 {
                return String::new();
            }
            let mut k: Vec<String> = self.kinds.iter().map(|(k, n)| format!("{n} {k}")).collect();
            if self.others > 0 {
                k.push(format!("{} other", self.others));
            }
            format!(" ({})", k.join(", "))
        };
        let o = self.objects;
        if o.added > 0 {
            parts.push(format!(
                "added {}{}",
                plural(o.added, "object", "objects"),
                if o.changed == 0 && o.removed == 0 {
                    kinds(o.added)
                } else {
                    String::new()
                }
            ));
        }
        if let Some(d) = &self.detail {
            let kind = self.kinds.first().map_or("object", |(k, _)| k.as_str());
            let label = d.label.as_ref().map_or(String::new(), |l| format!(" '{l}'"));
            parts.push(format!(
                "changed {}{label}: {}",
                kind.replace('_', " "),
                d.text()
            ));
        } else if o.changed > 0 {
            parts.push(format!(
                "changed {}{}",
                plural(o.changed, "object", "objects"),
                if o.added == 0 && o.removed == 0 {
                    kinds(o.changed)
                } else {
                    String::new()
                }
            ));
        }
        if o.removed > 0 {
            parts.push(format!(
                "removed {}{}",
                plural(o.removed, "object", "objects"),
                if o.added == 0 && o.changed == 0 {
                    kinds(o.removed)
                } else {
                    String::new()
                }
            ));
        }
        for (what, c) in [
            ("layer", self.layers),
            ("class", self.classes),
            ("resource", self.resources),
        ] {
            let (one, many) = if what == "class" {
                ("class", "classes")
            } else {
                (what, &*format!("{what}s"))
            };
            if what == "layer"
                && !self.layers_added.is_empty()
                && c.added == self.layers_added.len()
            {
                parts.push(format!("added layer {}", self.layers_added.join(", ")));
            } else if c.added > 0 {
                parts.push(format!("added {}", plural(c.added, one, many)));
            }
            if c.changed > 0 {
                parts.push(format!("changed {}", plural(c.changed, one, many)));
            }
            if c.removed > 0 {
                parts.push(format!("removed {}", plural(c.removed, one, many)));
            }
        }
        if self.document {
            parts.push("changed document settings".into());
        }
        if self.versions {
            parts.push("changed the versions".into());
        }
        if self.issues {
            parts.push("changed issues".into());
        }
        if parts.is_empty() {
            return "no visible change".into();
        }
        let text = parts.join("; ");
        let mut chars = text.chars();
        chars
            .next()
            .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
            .unwrap_or_default()
    }
}

/// One call or one script run of an agent that changed the document.
#[derive(Clone, Debug)]
pub struct Step {
    /// The number of the step in this log (counts from 1; never reused).
    pub id: u64,
    /// The agent session (a connection, and a gap of silence starts a new one).
    pub session: u64,
    /// The undo steps the call made, oldest first (`Editor::step_place`).
    pub serials: Vec<u64>,
    /// Wall clock, ms since 1970.
    pub time_ms: u64,
    /// The method (`batch`, `script.eval`, ...).
    pub method: String,
    pub conn: u64,
    /// The revision of the document after the step.
    pub revision: u64,
    pub summary: Summary,
    /// What each object looked like before and after. `None` after the bound dropped it.
    pub data: Option<Arc<StepData>>,
    /// The person marked the step as seen (AGR-05).
    pub seen: bool,
    /// The step was reverted (all its objects went back).
    pub reverted: bool,
    /// The record is not complete: the history limit dropped part of what the call did while it
    /// ran, so the objects of the step are only the ones that were kept. Such a step cannot be
    /// reverted.
    pub partial: bool,
    /// What the step was.
    pub kind: StepKind,
}

/// What a step of the log was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind {
    /// A call or script run that changed the document.
    Call,
    /// The agent took back the last undo step (which may be a step of the person).
    Undo,
    /// The agent redid a step.
    Redo,
}

impl Step {
    /// Ids of the objects by how they changed.
    pub fn ids(&self, which: Counted) -> Vec<ObjectId> {
        self.data
            .as_ref()
            .map(|d| {
                d.objects
                    .iter()
                    .filter(|o| o.kind() == which)
                    .map(|o| o.id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Every object of the step.
    pub fn object_ids(&self) -> Vec<ObjectId> {
        self.data
            .as_ref()
            .map(|d| d.objects.iter().map(|o| o.id).collect())
            .unwrap_or_default()
    }
}

/// What the log needs to record a step.
pub struct Recorded {
    pub conn: u64,
    pub method: String,
    pub time_ms: u64,
    pub revision: u64,
    pub serials: Vec<u64>,
    pub data: StepData,
    pub partial: bool,
    pub kind: StepKind,
}

/// The session that a call belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionRef {
    pub session: u64,
    /// True if this call starts the session (a new connection, or after a gap of silence).
    pub started: bool,
    /// True until a version was tried for this session (`ActivityLog::version_attempted`). It is
    /// not the first call that needs the version but the first call that may change something:
    /// an agent that starts with `describe` still gets its version before its first edit.
    pub needs_version: bool,
}

/// The steps of the agents of one session of the API.
#[derive(Clone, Debug, Default)]
pub struct ActivityLog {
    steps: VecDeque<Step>,
    next_id: u64,
    next_session: u64,
    /// Per connection: the session and the time of its last call.
    sessions: HashMap<u64, (u64, u64)>,
    /// Sessions for which a version was tried.
    versioned: Vec<u64>,
    states: usize,
    state_bytes: u64,
    /// Changes whenever the log changes, for a view that caches.
    generation: u64,
    /// Sessions whose version could not be saved: (session, why).
    version_notes: Vec<(u64, String)>,
    /// The version saved at the start of a session: (session, version id).
    session_versions: Vec<(u64, u64)>,
}

fn states(data: &StepData) -> usize {
    data.objects.len()
}

impl ActivityLog {
    pub fn steps(&self) -> &VecDeque<Step> {
        &self.steps
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn get(&self, id: u64) -> Option<&Step> {
        self.steps.iter().find(|s| s.id == id)
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The steps after this id (exclusive), oldest first.
    pub fn since(&self, id: u64) -> impl Iterator<Item = &Step> {
        self.steps.iter().filter(move |s| s.id > id)
    }

    /// The id of the newest step (0: none).
    pub fn last_id(&self) -> u64 {
        self.steps.back().map_or(0, |s| s.id)
    }

    /// Notes the call of a connection and tells whether it starts a session.
    pub fn touch(&mut self, conn: u64, now_ms: u64) -> SessionRef {
        let entry = self.sessions.get(&conn).copied();
        match entry {
            Some((session, last)) if now_ms.saturating_sub(last) <= SESSION_GAP_MS => {
                self.sessions.insert(conn, (session, now_ms));
                SessionRef {
                    session,
                    started: false,
                    needs_version: !self.versioned.contains(&session),
                }
            }
            _ => {
                self.next_session += 1;
                let session = self.next_session;
                self.sessions.insert(conn, (session, now_ms));
                SessionRef {
                    session,
                    started: true,
                    needs_version: true,
                }
            }
        }
    }

    /// Says that the version at the start of a session was tried (saved or refused).
    pub fn version_attempted(&mut self, session: u64) {
        if !self.versioned.contains(&session) {
            self.versioned.push(session);
            if self.versioned.len() > 256 {
                self.versioned.remove(0);
            }
        }
    }

    /// The session of a connection, if it called.
    pub fn session_of(&self, conn: u64) -> Option<u64> {
        self.sessions.get(&conn).map(|(s, _)| *s)
    }

    /// A connection closed: its next call starts a session.
    pub fn forget_connection(&mut self, conn: u64) {
        self.sessions.remove(&conn);
    }

    /// Says that the version at the start of a session could not be saved.
    pub fn note_version_failure(&mut self, session: u64, why: String) {
        self.version_notes.push((session, why));
        if self.version_notes.len() > 64 {
            self.version_notes.remove(0);
        }
        self.generation += 1;
    }

    /// Says which version was saved at the start of a session.
    pub fn set_session_version(&mut self, session: u64, version: u64) {
        self.session_versions.push((session, version));
        if self.session_versions.len() > 256 {
            self.session_versions.remove(0);
        }
        self.generation += 1;
    }

    /// The version saved at the start of the session, if one was.
    pub fn session_version(&self, session: u64) -> Option<u64> {
        self.session_versions
            .iter()
            .find(|(s, _)| *s == session)
            .map(|(_, v)| *v)
    }

    pub fn version_note(&self, session: u64) -> Option<&str> {
        self.version_notes
            .iter()
            .find(|(s, _)| *s == session)
            .map(|(_, w)| w.as_str())
    }

    /// Records a step. A call that changed nothing that a person sees is not a step: `None`.
    pub fn record(&mut self, r: Recorded) -> Option<u64> {
        if r.data.is_empty() {
            return None;
        }
        self.next_id += 1;
        let id = self.next_id;
        self.states += states(&r.data);
        self.state_bytes += r.data.approx_bytes();
        let session = self.session_of(r.conn).unwrap_or(0);
        self.steps.push_back(Step {
            id,
            session,
            serials: r.serials,
            time_ms: r.time_ms,
            method: r.method,
            conn: r.conn,
            revision: r.revision,
            summary: Summary::of(&r.data),
            data: Some(Arc::new(r.data)),
            seen: false,
            reverted: false,
            partial: r.partial,
            kind: r.kind,
        });
        self.bound();
        self.generation += 1;
        Some(id)
    }

    fn bound(&mut self) {
        while self.steps.len() > MAX_STEPS {
            if let Some(old) = self.steps.pop_front()
                && let Some(d) = old.data
            {
                self.states = self.states.saturating_sub(states(&d));
                self.state_bytes = self.state_bytes.saturating_sub(d.approx_bytes());
            }
        }
        // The oldest steps lose their states first. The newest step is bound too: a single step
        // above the budget keeps its counts and its summary and loses its states (the panel says
        // so, and the step cannot be reverted).
        let mut i = 0;
        while (self.states > MAX_OBJECT_STATES || self.state_bytes > MAX_STATE_BYTES)
            && i < self.steps.len()
        {
            if let Some(d) = self.steps[i].data.take() {
                self.states = self.states.saturating_sub(states(&d));
                self.state_bytes = self.state_bytes.saturating_sub(d.approx_bytes());
            }
            i += 1;
        }
    }

    /// Marks a step as reverted.
    pub fn set_reverted(&mut self, id: u64) {
        if let Some(s) = self.steps.iter_mut().find(|s| s.id == id)
            && !s.reverted
        {
            s.reverted = true;
            self.generation += 1;
        }
    }

    /// Marks steps as seen (or not). Returns how many changed.
    pub fn set_seen(&mut self, upto_id: u64, seen: bool) -> usize {
        let mut n = 0;
        for s in self.steps.iter_mut().filter(|s| s.id <= upto_id) {
            if s.seen != seen {
                s.seen = seen;
                n += 1;
            }
        }
        if n > 0 {
            self.generation += 1;
        }
        n
    }

    /// Marks one step as seen (or not). True if it changed.
    pub fn set_seen_one(&mut self, id: u64, seen: bool) -> bool {
        match self.steps.iter_mut().find(|s| s.id == id) {
            Some(s) if s.seen != seen => {
                s.seen = seen;
                self.generation += 1;
                true
            }
            _ => false,
        }
    }

    /// Everything goes (the document was replaced: the steps belong to the old one).
    pub fn clear(&mut self) {
        if self.steps.is_empty() && self.sessions.is_empty() {
            return;
        }
        self.steps.clear();
        self.sessions.clear();
        self.states = 0;
        self.state_bytes = 0;
        self.version_notes.clear();
        self.session_versions.clear();
        self.versioned.clear();
        self.generation += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::Origin;
    use crate::command::{Command, Editor};
    use crate::model::Shape;
    use glam::DVec2;

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

    fn record(
        log: &mut ActivityLog,
        ed: &mut Editor,
        conn: u64,
        f: impl FnOnce(&mut Editor),
    ) -> Option<u64> {
        log.touch(conn, 1_000);
        let mark = ed.serial_mark();
        f(ed);
        let serials = ed.steps_since(mark);
        let data = ed.step_data(&serials).unwrap();
        log.record(Recorded {
            conn,
            method: "batch".into(),
            time_ms: 1_000,
            revision: ed.revision(),
            serials,
            data,
            partial: false,
            kind: StepKind::Call,
        })
    }

    #[test]
    fn a_step_has_counts_a_text_and_the_object_states() {
        let mut ed = Editor::with_sequential_ids();
        let mut log = ActivityLog::default();
        let id = record(&mut log, &mut ed, 1, |e| {
            add(e, 0.0);
            add(e, 5.0);
        })
        .unwrap();
        let step = log.get(id).unwrap();
        assert_eq!(step.summary.objects.added, 2);
        assert_eq!(step.summary.text(), "Added 2 objects (2 line)");
        assert_eq!(step.ids(Counted::Added).len(), 2);
        assert!(step.data.is_some());
    }

    #[test]
    fn a_call_without_a_change_is_no_step() {
        let mut ed = Editor::with_sequential_ids();
        let mut log = ActivityLog::default();
        assert_eq!(record(&mut log, &mut ed, 1, |_| {}), None);
        assert!(log.is_empty());
    }

    #[test]
    fn mixed_changes_read_as_a_list() {
        let mut ed = Editor::with_sequential_ids();
        let a = add(&mut ed, 0.0);
        let b = add(&mut ed, 5.0);
        let mut log = ActivityLog::default();
        let id = record(&mut log, &mut ed, 1, |e| {
            add(e, 9.0);
            e.execute(&Command::MoveObjects {
                ids: vec![a],
                delta: DVec2::new(1.0, 0.0),
            })
            .unwrap();
            e.execute(&Command::DeleteObjects { ids: vec![b] }).unwrap();
            e.execute(&Command::AddLayer {
                name: "Planting".into(),
                elevation: 0.0,
                scale: None,
                kind: None,
            })
            .unwrap();
        })
        .unwrap();
        assert_eq!(
            log.get(id).unwrap().summary.text(),
            "Added 1 object; changed 1 object; removed 1 object; added layer Planting"
        );
    }

    #[test]
    fn a_script_run_is_one_step_and_one_revert() {
        let mut ed = Editor::with_sequential_ids();
        ed.set_origin(Origin::Agent);
        let mut log = ActivityLog::default();
        let mut mark_out = None;
        let id = record(&mut log, &mut ed, 1, |e| {
            let mark = e.begin_run();
            add(e, 0.0);
            add(e, 5.0);
            add(e, 9.0);
            assert_eq!(e.squash_run(mark, "script.eval"), 3);
            mark_out = Some(());
        })
        .unwrap();
        let step = log.get(id).unwrap();
        assert_eq!(step.serials.len(), 1, "one run is one undo step");
        assert_eq!(step.summary.objects.added, 3);
        // A later step of the person elsewhere; the run still reverts as one.
        ed.set_origin(Origin::User);
        add(&mut ed, 50.0);
        let r = ed
            .revert_step(&step.serials, step.data.as_ref().unwrap(), false, true)
            .unwrap();
        assert_eq!(r.reverted.len(), 3);
        assert_eq!(ed.doc().objects.len(), 1);
        assert!(ed.undo());
        assert_eq!(ed.doc().objects.len(), 4, "the revert is one undo step");
    }

    #[test]
    fn a_rolled_back_run_leaves_no_step() {
        let mut ed = Editor::with_sequential_ids();
        let mut log = ActivityLog::default();
        let r = record(&mut log, &mut ed, 1, |e| {
            let mark = e.begin_run();
            add(e, 0.0);
            e.rollback_run(mark);
        });
        assert_eq!(r, None);
    }

    #[test]
    fn sessions_start_for_a_new_connection_and_after_a_gap() {
        let mut log = ActivityLog::default();
        let a = log.touch(1, 0);
        assert!(a.started && a.needs_version);
        assert!(!log.touch(1, 1000).started);
        assert!(
            log.touch(1, 1000).needs_version,
            "until a version was tried"
        );
        log.version_attempted(a.session);
        assert!(!log.touch(1, 1000).needs_version);
        let b = log.touch(2, 1000);
        assert!(b.started);
        assert_ne!(a.session, b.session);
        assert!(log.touch(1, SESSION_GAP_MS + 2000).started);
        log.forget_connection(2);
        assert!(log.touch(2, 5000).started);
    }

    #[test]
    fn the_log_is_bounded_and_old_steps_lose_their_states_first() {
        let mut ed = Editor::with_sequential_ids();
        let mut log = ActivityLog::default();
        for i in 0..(MAX_STEPS + 5) {
            record(&mut log, &mut ed, 1, |e| {
                add(e, i as f64);
            });
        }
        assert_eq!(log.len(), MAX_STEPS);
        assert_eq!(log.steps().front().unwrap().id, 6);
        // States: fake a big step.
        let mut big = StepData::default();
        for i in 0..(MAX_OBJECT_STATES + 1) {
            big.objects.push(crate::command::ObjChange {
                id: ObjectId(crate::uid::Uid::from_u128(i as u128 + 1000)),
                before: None,
                after: Some(crate::model::Object::new(
                    ObjectId(crate::uid::Uid::from_u128(i as u128 + 1000)),
                    crate::model::LayerId::from_num(1),
                    crate::model::ClassId::from_num(1),
                    Shape::Line {
                        a: DVec2::ZERO,
                        b: DVec2::ONE,
                    },
                )),
                index: None,
            });
        }
        log.touch(1, 0);
        log.record(Recorded {
            conn: 1,
            method: "x".into(),
            time_ms: 0,
            revision: 0,
            serials: vec![],
            data: big,
            partial: false,
            kind: StepKind::Call,
        });
        assert!(log.steps().front().unwrap().data.is_none());
        assert!(
            log.steps().back().unwrap().data.is_none(),
            "a single step above the budget loses its own states"
        );
        assert_eq!(
            log.steps().back().unwrap().summary.objects.added,
            MAX_OBJECT_STATES + 1
        );
    }

    #[test]
    fn the_text_of_a_summary_names_every_kind_of_change() {
        let c = |a, ch, r| Counts {
            added: a,
            changed: ch,
            removed: r,
        };
        let s = Summary {
            objects: c(2, 1, 1),
            kinds: vec![("line".into(), 3), ("rect".into(), 1)],
            others: 2,
            layers: c(1, 1, 1),
            classes: c(2, 1, 1),
            resources: c(1, 2, 1),
            layers_added: vec!["Trees".into()],
            document: true,
            versions: true,
            issues: true,
            detail: None,
        };
        assert_eq!(
            s.text(),
            "Added 2 objects; changed 1 object; removed 1 object; added layer Trees; changed 1 layer; \
             removed 1 layer; added 2 classes; changed 1 class; removed 1 class; added 1 resource; \
             changed 2 resources; removed 1 resource; changed document settings; changed the versions; \
             changed issues"
        );
        // One kind of change shows the kinds of shape, with the rest counted.
        let only = Summary {
            objects: c(0, 0, 4),
            kinds: vec![("line".into(), 3), ("rect".into(), 1)],
            others: 2,
            ..Summary::default()
        };
        assert_eq!(only.text(), "Removed 4 objects (3 line, 1 rect, 2 other)");
        let changed = Summary {
            objects: c(0, 1, 0),
            kinds: vec![("circle".into(), 1)],
            ..Summary::default()
        };
        assert_eq!(changed.text(), "Changed 1 object (1 circle)");
        assert_eq!(Summary::default().text(), "no visible change");
    }

    #[test]
    fn a_step_tells_its_objects_reverted_and_the_notes_of_its_session() {
        let mut ed = Editor::with_sequential_ids();
        let mut log = ActivityLog::default();
        let a = record(&mut log, &mut ed, 1, |e| {
            add(e, 0.0);
            add(e, 3.0);
        })
        .unwrap();
        let step = log.get(a).unwrap();
        assert_eq!(step.object_ids().len(), 2);
        assert_eq!(step.ids(Counted::Changed).len(), 0);
        assert!(!step.reverted && !step.partial && step.kind == StepKind::Call);
        let g = log.generation();
        log.set_reverted(a);
        log.set_reverted(a);
        log.set_reverted(999);
        assert!(log.get(a).unwrap().reverted);
        assert_eq!(log.generation(), g + 1, "the second mark changes nothing");
        assert_eq!(log.since(0).count(), 1);
        assert_eq!(log.since(a).count(), 0);
        // Notes and versions of a session, bounded.
        log.note_version_failure(1, "limit".into());
        assert_eq!(log.version_note(1), Some("limit"));
        for i in 0..70 {
            log.note_version_failure(100 + i, "x".into());
        }
        assert_eq!(log.version_note(1), None, "the oldest notes go first");
        assert!(log.version_note(169).is_some());
        for i in 0..300 {
            log.set_session_version(i, i + 1000);
        }
        assert_eq!(log.session_version(0), None);
        assert_eq!(log.session_version(299), Some(1299));
        assert!(!log.set_seen_one(999, true));
        assert!(log.set_seen_one(a, true));
        assert!(!log.set_seen_one(a, true));
        assert_eq!(log.session_of(1), Some(1));
        log.clear();
        log.clear();
        assert!(log.is_empty() && log.session_version(299).is_none());
    }

    #[test]
    fn seen_marks_and_clear() {
        let mut ed = Editor::with_sequential_ids();
        let mut log = ActivityLog::default();
        let a = record(&mut log, &mut ed, 1, |e| {
            add(e, 0.0);
        })
        .unwrap();
        let b = record(&mut log, &mut ed, 1, |e| {
            add(e, 3.0);
        })
        .unwrap();
        assert_eq!(log.set_seen(a, true), 1);
        assert!(log.get(a).unwrap().seen && !log.get(b).unwrap().seen);
        let g = log.generation();
        log.clear();
        assert!(log.is_empty() && log.generation() > g);
    }
}
