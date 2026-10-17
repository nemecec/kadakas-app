//! What the agent changed (AGR-05, AGR-06): the activity log of the session.
//!
//! Each call or script run of an agent that changed the document is one step of the log
//! (`kadakas_core::activity`). The session records it in [`Session::handle_as`] (origin `Agent`),
//! and saves a named version before the first change of an agent session. The method
//! `agent.activity` lists the steps and the objects of one step. It only reads: the revert of a
//! step is a decision of the person, made in the app ([`Session::revert_agent_step`]).

use kadakas_core::activity::{Recorded, Step};
use kadakas_core::change::Origin;
use kadakas_core::command::{Conflict, Counted, RevertReport, StepPlace};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::errors::RpcError;
use crate::{Session, params, review_api};

pub const AGENT_ACTIVITY: (&str, &str) = (
    "agent.activity",
    "The steps of the agent: each call or script run that changed the document (read only). params: {step?, since?, limit?}. Without step: {steps: [{id, session, time_ms, method, summary, counts {objects {added, changed, removed}, layers, classes, resources}, seen, state (active | undone | gone), revertible, session_version? (the version saved before the session; render.png {compare: {from: <it>, to: \"current\"}} draws the changes of the session), version_note?}], last_id, total}; since: only steps after this id; limit: at most this many, the newest (default 50, at most 500). With step: the same entry plus {objects {added, changed, removed} (IDs, at most 1000 each), truncated, blocked? (why the person cannot revert it now)}. The summary is written from the change set. The person sees the same list in the Activity panel and can undo or revert a step there; you cannot.",
);

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ActivityParams {
    step: Option<u64>,
    since: Option<u64>,
    limit: Option<usize>,
}

const MAX_LIMIT: usize = 500;
const MAX_IDS: usize = 1000;

/// What the app needs to know before it offers a revert.
#[derive(Clone, Debug, PartialEq)]
pub struct StepCheck {
    pub place: StepPlace,
    /// The step has its object states (the bound may have dropped them).
    pub details: bool,
    /// The steps above it in the undo history, and how many are not from an agent.
    pub above: usize,
    pub above_other: usize,
    /// Objects of the step that something changed later, deleted later, and so on.
    pub conflicts: Vec<Conflict>,
    /// Objects that the step made and that are deleted since.
    pub already_gone: usize,
    /// More than objects changed and later steps exist: only an undo back to the step works.
    pub snapshot_blocked: bool,
    /// Objects of the step that can go back now.
    pub free: usize,
}

impl StepCheck {
    /// True if the step is on top of the history: a revert is a plain undo.
    pub fn on_top(&self) -> bool {
        self.place == StepPlace::Undo(0)
    }

    /// Why a revert (of all the objects) is not possible now, in English, or `None`.
    pub fn blocked(&self) -> Option<String> {
        match self.place {
            StepPlace::Redo => return Some("the step is undone already".into()),
            StepPlace::Gone => {
                return Some(
                    "the step is not in the undo history any more (the history limit dropped it, or a newer change cleared it)".into(),
                );
            }
            StepPlace::Undo(0) => return None,
            StepPlace::Undo(_) => {}
        }
        if !self.details {
            return Some("the log no longer holds what the step changed (too many steps)".into());
        }
        if self.snapshot_blocked {
            return Some(
                "the step also changed layers, classes, resources, settings, versions or issues, and later steps exist: use undo back to the step".into(),
            );
        }
        if !self.conflicts.is_empty() {
            return Some(
                kadakas_core::command::Refusal::Conflicts(self.conflicts.clone()).to_string(),
            );
        }
        None
    }
}

/// What the app offers for a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advice {
    /// The step is on top: reverting it is an undo (redo brings it back). No question.
    Undo,
    /// Every object of the step can go back.
    Revert,
    /// `free` objects can go back; `blocked` cannot (changed later, deleted later, ...).
    Partial { free: usize, blocked: usize },
    /// Only an undo back to the step works (it changed more than objects).
    UndoBackOnly,
    /// Nothing can be done.
    Nothing,
}

impl StepCheck {
    pub fn advice(&self) -> Advice {
        match self.place {
            StepPlace::Undo(0) => Advice::Undo,
            StepPlace::Undo(_) if self.snapshot_blocked => Advice::UndoBackOnly,
            StepPlace::Undo(_) if !self.details => Advice::UndoBackOnly,
            StepPlace::Undo(_) if self.conflicts.is_empty() => Advice::Revert,
            StepPlace::Undo(_) if self.free > 0 => Advice::Partial {
                free: self.free,
                blocked: self.conflicts.len(),
            },
            _ => Advice::Nothing,
        }
    }
}

/// Why a step could not be reverted.
#[derive(Clone, Debug, PartialEq)]
pub enum StepError {
    /// No such step in the log.
    Unknown,
    /// The log no longer holds the object states of the step.
    DetailsDropped,
    Refused(kadakas_core::command::Refusal),
}

impl std::fmt::Display for StepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepError::Unknown => write!(f, "there is no such step"),
            StepError::DetailsDropped => write!(
                f,
                "the log no longer holds what the step changed (too many steps)"
            ),
            StepError::Refused(r) => write!(f, "{r}"),
        }
    }
}

/// Methods that cannot change the document, by the end or the start of their name. A call that is
/// not one of them may start an agent session: a version is saved first. A wrong guess costs one
/// extra version (a read call taken for a change) or none (a change taken for a read).
fn reads_only(method: &str) -> bool {
    let (domain, last) = method.split_once('.').unwrap_or(("", method));
    matches!(
        domain,
        "view" | "render" | "events" | "report" | "describe" | "agent"
    ) || matches!(
        last,
        "get"
            | "list"
            | "summary"
            | "describe"
            | "query"
            | "diff"
            | "poll"
            | "check"
            | "find"
            | "status"
            | "schema"
            | "measure"
            | "pointed"
            | "subscribe"
            | "unsubscribe"
    )
}

impl Session {
    /// Runs a request of an agent and records the step. See [`Session::handle_as`].
    pub(crate) fn handle_agent_call(
        &mut self,
        conn: u64,
        method: &str,
        p: Value,
    ) -> Result<Value, RpcError> {
        let now = review_api::default_now();
        let started = self.activity.touch(conn, now);
        if started.started && !reads_only(method) {
            self.save_session_version(started.session, now);
        }
        let mark = self.editor.serial_mark();
        let revision = self.editor.revision();
        let result = self.handle(method, p);
        if result.is_ok() && self.editor.revision() != revision {
            let serials = self.editor.steps_since(mark);
            if !serials.is_empty()
                && let Some(data) = self.editor.step_data(&serials)
            {
                self.activity.record(Recorded {
                    conn,
                    method: method.to_string(),
                    time_ms: now,
                    revision: self.editor.revision(),
                    serials,
                    data,
                });
            }
        }
        result
    }

    /// The named version at the start of an agent session. A refusal (the limit of versions, a
    /// history that cannot be written) does not stop the agent: the log keeps the reason, and
    /// the panel shows it.
    fn save_session_version(&mut self, session: u64, now: u64) {
        let r = self.handle(
            "version.create",
            json!({
                "name": "Before agent session",
                "note": "Saved automatically when an agent session started.",
                "now_ms": now,
            }),
        );
        match r {
            Ok(v) => {
                if let Some(id) = v.pointer("/version/id").and_then(Value::as_u64) {
                    self.activity.set_session_version(session, id);
                }
            }
            Err(e) => self.activity.note_version_failure(session, e.message),
        }
    }

    /// Can the step be reverted now, and what stands in the way?
    pub fn revert_check(&self, step_id: u64) -> Result<StepCheck, StepError> {
        let step = self.activity.get(step_id).ok_or(StepError::Unknown)?;
        let place = self.step_place(step);
        let origins = step
            .serials
            .iter()
            .last()
            .map(|s| self.editor.origins_above(*s))
            .unwrap_or_default();
        let above_other = origins.iter().filter(|o| **o != Origin::Agent).count();
        // The steps above the newest step of the call (they are not part of it).
        let (conflicts, gone) = match &step.data {
            Some(d) if matches!(place, StepPlace::Undo(n) if n > 0) => {
                self.editor.revert_conflicts(d)
            }
            _ => (Vec::new(), Vec::new()),
        };
        let snapshot_blocked = step
            .data
            .as_ref()
            .is_some_and(|d| d.has_snapshot_parts() && !matches!(place, StepPlace::Undo(0)));
        let total = step.data.as_ref().map_or(0, |d| d.objects.len());
        Ok(StepCheck {
            place,
            details: step.data.is_some(),
            above: origins.len(),
            above_other,
            free: total.saturating_sub(conflicts.len() + gone.len()),
            already_gone: gone.len(),
            conflicts,
            snapshot_blocked,
        })
    }

    /// Where the steps of a call are: gone if one part is, else undone if one part is, else in
    /// the history with the number of steps above the newest part.
    fn step_place(&self, step: &Step) -> StepPlace {
        let places: Vec<StepPlace> = step
            .serials
            .iter()
            .map(|s| self.editor.step_place(*s))
            .collect();
        if places.is_empty() || places.contains(&StepPlace::Gone) {
            StepPlace::Gone
        } else if places.contains(&StepPlace::Redo) {
            StepPlace::Redo
        } else {
            places.last().copied().unwrap_or(StepPlace::Gone)
        }
    }

    /// Takes one step of the agent back, although later steps exist (`Editor::revert_step`). The
    /// objects that something changed later stay as they are: with `skip_conflicts` the rest goes
    /// back, else nothing changes. One undo step of the person. The caller prunes the selection.
    pub fn revert_agent_step(
        &mut self,
        step_id: u64,
        skip_conflicts: bool,
    ) -> Result<RevertReport, StepError> {
        let step = self.activity.get(step_id).ok_or(StepError::Unknown)?;
        let data = step.data.clone().ok_or(StepError::DetailsDropped)?;
        let serials = step.serials.clone();
        let origin = self.editor.origin();
        self.editor.set_origin(Origin::User);
        let r = self.editor.revert_step(&serials, &data, skip_conflicts);
        self.editor.set_origin(origin);
        self.prune_selection();
        r.map_err(StepError::Refused)
    }

    /// Takes back every step down to this one of the agent (the steps of the person above it
    /// too: redo brings them back). Returns how many undo steps went.
    pub fn undo_back_to_agent_step(&mut self, step_id: u64) -> Result<usize, StepError> {
        let step = self.activity.get(step_id).ok_or(StepError::Unknown)?;
        let first = *step.serials.first().ok_or(StepError::Unknown)?;
        let origin = self.editor.origin();
        self.editor.set_origin(Origin::User);
        let r = self.editor.undo_through(first);
        self.editor.set_origin(origin);
        self.prune_selection();
        r.map_err(StepError::Refused)
    }
}

fn counts(c: kadakas_core::command::Counts) -> Value {
    json!({"added": c.added, "changed": c.changed, "removed": c.removed})
}

fn state(p: StepPlace) -> &'static str {
    match p {
        StepPlace::Undo(_) => "active",
        StepPlace::Redo => "undone",
        StepPlace::Gone => "gone",
    }
}

fn entry(s: &Session, step: &Step) -> Value {
    let check = s.revert_check(step.id).ok();
    let revertible = check.as_ref().is_some_and(|c| c.blocked().is_none());
    let mut v = json!({
        "id": step.id,
        "session": step.session,
        "time_ms": step.time_ms,
        "method": step.method,
        "summary": step.summary.text(),
        "counts": {
            "objects": counts(step.summary.objects),
            "layers": counts(step.summary.layers),
            "classes": counts(step.summary.classes),
            "resources": counts(step.summary.resources),
        },
        "seen": step.seen,
        "state": check.as_ref().map_or("gone", |c| state(c.place)),
        "revertible": revertible,
    });
    if let Some(id) = s.activity.session_version(step.session) {
        v["session_version"] = json!(id);
    }
    if let Some(note) = s.activity.version_note(step.session) {
        v["version_note"] = json!(format!("no version was saved at the start of the session: {note}"));
    }
    v
}

pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if method != AGENT_ACTIVITY.0 {
        return None;
    }
    Some(params::<ActivityParams>(p).and_then(|q| {
        if let Some(id) = q.step {
            let step = s.activity.get(id).ok_or_else(|| {
                RpcError::not_found(format!("there is no step {id} in the activity log"))
            })?;
            let mut v = entry(s, step);
            let mut truncated = false;
            let mut lists = serde_json::Map::new();
            for (name, kind) in [
                ("added", Counted::Added),
                ("changed", Counted::Changed),
                ("removed", Counted::Removed),
            ] {
                let ids = step.ids(kind);
                truncated |= ids.len() > MAX_IDS;
                lists.insert(
                    name.into(),
                    json!(ids.iter().take(MAX_IDS).map(|i| i.0.to_string()).collect::<Vec<_>>()),
                );
            }
            v["objects"] = Value::Object(lists);
            v["truncated"] = json!(truncated);
            if !step.data.is_some() {
                v["objects"] = Value::Null;
            }
            if let Some(why) = s.revert_check(id).ok().and_then(|c| c.blocked()) {
                v["blocked"] = json!(why);
            }
            return Ok(v);
        }
        let limit = q.limit.unwrap_or(50).min(MAX_LIMIT);
        let all: Vec<&Step> = s.activity.since(q.since.unwrap_or(0)).collect();
        let skip = all.len().saturating_sub(limit);
        Ok(json!({
            "steps": all.iter().skip(skip).map(|st| entry(s, st)).collect::<Vec<_>>(),
            "last_id": s.activity.last_id(),
            "total": s.activity.len(),
        }))
    }))
}

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "concept": "Every call or script run that changes the document is one step of the agent. The person reviews the steps in the Activity panel: what each step added, changed and removed (highlighted in the plan and in 3D), the time, the method and a summary, and can undo back to a step or revert one step. agent.activity reads the same list. A named version \"Before agent session\" is saved before the first change of a session (a new connection, or a call after 30 minutes of silence); if the limit of versions refuses it, the step says so in version_note.",
        "revert": "The person can revert one step although later steps exist. Objects of the step that something changed later (by the person or by you) are never overwritten: the revert is refused, or reverts the rest and leaves exactly those objects. A step that also changed layers, classes, resources, settings, versions or issues can only be undone together with the steps above it. Your next steps may therefore find objects gone or as they were: read before you rely on an ID.",
        "script_runs": "One script run (script.eval) is one step, whatever number of calls it made.",
        "examples": [
            {"method": "agent.activity", "params": {"limit": 10}},
            {"method": "agent.activity", "params": {"step": 3}},
        ],
    })
}
