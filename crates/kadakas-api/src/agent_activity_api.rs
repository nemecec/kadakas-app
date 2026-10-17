//! What the agent changed (AGR-05, AGR-06): the activity log of the session.
//!
//! Each call or script run of an agent that changed the document is one step of the log
//! (`kadakas_core::activity`). The session records it in [`Session::handle_as`] (origin `Agent`),
//! also when the call failed after it had made changes, and the host may ask for a named version
//! before the first change of an agent session. The method `agent.activity` lists the steps and
//! the objects of one step. It only reads: the revert of a step is a decision of the person, made
//! in the app ([`Session::revert_agent_step`]).

use kadakas_core::activity::{Recorded, Step, StepKind};
use kadakas_core::change::Origin;
use kadakas_core::command::{Conflict, Counted, Refusal, RevertReport, StepPlace};
use serde_json::{Value, json};

use crate::errors::RpcError;
use crate::params::AgentActivityParams;
use crate::script_js_api::policy::{Class, class_of};
use crate::{Session, params, review_api};

pub const AGENT_ACTIVITY: (&str, &str) = (
    "agent.activity",
    "The steps of the agent: each call or script run that changed the document (read only), also a call that failed after it had made changes, and an undo or redo of the agent. params: {step?, since?, limit?}. Without step: {steps: [{id, session, time_ms, method, kind (call | undo | redo), summary, counts {objects {added, changed, removed}, layers, classes, resources}, seen, reverted, partial (the history limit dropped part of the record), state (active | undone | gone), revertible (null for the older steps of a long list), session_version? (the version saved before the session; render.png {compare: {from: <it>, to: \"current\"}} draws the changes of the session), version_note?}], last_id, total}; since: only steps after this id; limit: at most this many, the newest (default 50, at most 500). With step: the same entry plus {objects {added, changed, removed} (IDs, at most 1000 each), truncated, blocked? (why the person cannot revert it now)}. The summary is written from the change set. A step that changed one object and nothing else names the fields that changed, with the values before and after (for example: Changed hardscape 'Pavers': zone 'Aisle' angle 0° → 45°; at most two fields, then a count); the person sees it in their language. The person sees the same list in the Activity panel and can undo or revert a step there; you cannot. While the person has undone steps of their own with Undo to here and can redo them, your calls that change the document fail with a conflict (reason redo_pending) until the person redoes them or releases the hold.",
);

const MAX_LIMIT: usize = 500;
const MAX_IDS: usize = 1000;
/// The list gives `revertible` for this many of the newest steps (each check looks at every
/// object of the step).
const CHECKED: usize = 50;

/// Why a revert of the whole step is not possible now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// The step is an undo or redo of the agent, not a change.
    NotACall,
    /// The history limit dropped part of the record.
    Partial,
    /// Reverted already.
    Reverted,
    /// Undone (redo brings it back).
    Undone,
    /// The log holds no states of the step any more.
    Dropped,
    /// The step also changed layers, classes, resources, settings, versions or issues, and later
    /// steps exist.
    Snapshot,
    /// As `Snapshot`, in a shared document.
    Shared,
    /// This many objects cannot go back.
    Conflicts(usize),
    /// Every object is back already, or gone.
    NothingToDo,
}

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
    /// Objects that need nothing: made by the step and deleted since, or back as they were.
    pub already_gone: usize,
    /// More than objects changed and later steps exist: only an undo back to the step works.
    pub snapshot_blocked: bool,
    /// Objects of the step that can go back now.
    pub free: usize,
    /// The session is connected to a shared document: the editor's own undo is not used (the
    /// collaboration has a per-user undo), so only the revert of single objects works.
    pub shared: bool,
    /// The step is a call (not an undo or redo of the agent).
    pub is_call: bool,
    /// The record of the step is not complete.
    pub partial: bool,
    /// The step was reverted.
    pub reverted: bool,
}

impl StepCheck {
    /// True if the step is on top of the history and the editor's own undo may take it back.
    pub fn on_top(&self) -> bool {
        self.place == StepPlace::Undo(0) && !self.shared
    }

    /// Why the whole step cannot go back now, or `None`.
    pub fn why(&self) -> Option<Why> {
        if !self.is_call {
            return Some(Why::NotACall);
        }
        if self.partial {
            return Some(Why::Partial);
        }
        if self.reverted {
            return Some(Why::Reverted);
        }
        if self.place == StepPlace::Redo {
            return Some(Why::Undone);
        }
        if self.on_top() {
            return None;
        }
        if !self.details {
            return Some(Why::Dropped);
        }
        if self.snapshot_blocked {
            return Some(if self.shared {
                Why::Shared
            } else {
                Why::Snapshot
            });
        }
        if !self.conflicts.is_empty() {
            return Some(Why::Conflicts(self.conflicts.len()));
        }
        if self.free == 0 {
            return Some(Why::NothingToDo);
        }
        None
    }

    /// Why a revert (of all the objects) is not possible now, in English, or `None`.
    pub fn blocked(&self) -> Option<String> {
        Some(match self.why()? {
            Why::NotACall => "the step is an undo or redo of the agent, not a change".into(),
            Why::Partial => "the history limit dropped part of the record of this step, so it cannot be reverted".into(),
            Why::Reverted => "the step was reverted already".into(),
            Why::Undone => "the step is undone already".into(),
            Why::Dropped => "the log no longer holds what the step changed (too many steps)".into(),
            Why::Snapshot => Refusal::Snapshot { later: self.above }.to_string(),
            Why::Shared => Refusal::Shared.to_string(),
            Why::Conflicts(_) => Refusal::Conflicts(self.conflicts.clone()).to_string(),
            Why::NothingToDo => Refusal::NothingToDo.to_string(),
        })
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
        if !self.is_call || self.partial || self.reverted || self.place == StepPlace::Redo {
            return Advice::Nothing;
        }
        if self.on_top() {
            return Advice::Undo;
        }
        let undo_back = matches!(self.place, StepPlace::Undo(_)) && !self.shared;
        if !self.details || self.snapshot_blocked {
            return if undo_back && !self.partial && self.is_call {
                Advice::UndoBackOnly
            } else {
                Advice::Nothing
            };
        }
        if self.conflicts.is_empty() {
            return if self.free > 0 {
                Advice::Revert
            } else {
                Advice::Nothing
            };
        }
        if self.free > 0 {
            Advice::Partial {
                free: self.free,
                blocked: self.conflicts.len(),
            }
        } else {
            Advice::Nothing
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
    /// The step is not a change (an undo or redo of the agent), or its record is partial.
    NotRevertible(Why),
    Refused(Refusal),
}

impl std::fmt::Display for StepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepError::Unknown => write!(f, "there is no such step"),
            StepError::DetailsDropped => write!(
                f,
                "the log no longer holds what the step changed (too many steps)"
            ),
            StepError::NotRevertible(Why::Partial) => write!(
                f,
                "the history limit dropped part of the record of this step, so it cannot be reverted"
            ),
            StepError::NotRevertible(_) => {
                write!(f, "the step is an undo or redo of the agent, not a change")
            }
            StepError::Refused(r) => write!(f, "{r}"),
        }
    }
}

/// True if the method may change the document, by the classes of the script policy
/// (`script_js_api/policy.rs`): `Read` and `Ui` methods cannot. A method that nobody classified
/// counts as one that may.
pub fn may_change_document(method: &str) -> bool {
    match class_of(method) {
        Some(Class::Read | Class::Ui) => false,
        // A script never calls these; most of them end a run or change the session. Those that
        // only talk are no change.
        Some(Class::Never) => {
            !(method.starts_with("events.")
                || method.starts_with("feedback.")
                || method == "collab.status"
                || method == "collab.presence")
        }
        // Files: a save or an export reads the document and writes a file.
        Some(Class::Files) => !(method.starts_with("export") || method.starts_with("file.save")),
        _ => true,
    }
}

/// True if a call starts the version "Before agent session" first: a call that may change the
/// drawing. The calls that make a version, answer a review issue, move through the history or
/// replace the document themselves do not.
fn starts_session_version(method: &str) -> bool {
    let domain = method.split('.').next().unwrap_or("");
    may_change_document(method)
        && !matches!(
            domain,
            "version"
                | "issue"
                | "undo"
                | "redo"
                | "document"
                | "file"
                | "collab"
                | "agent"
                | "selection"
        )
}

/// The length of a hold on the agent when the person has undone steps of their own.
const HOLD_REASON: &str = "the person undid steps of their own with Undo to here and can still redo them; an edit now would delete those steps. Wait until they redo them or release the hold (the call changes nothing), then try again";

impl Session {
    /// Turns on the version "Before agent session": the first call that may change the document
    /// in an agent session (a new connection, or a call after 30 minutes of silence) saves one
    /// first. It is a change of the history of the document (an undo step of its own, below the
    /// first step of the agent, and a read of the history of a file that waits unread), so a host
    /// turns it on where a person reviews the work of an agent (the app does). Off by default.
    pub fn set_session_version(&mut self, on: bool) {
        self.session_version = on;
    }

    pub fn session_version_enabled(&self) -> bool {
        self.session_version
    }

    /// True while agent calls that change the document are held: the person undid steps of their
    /// own with Undo to here, and redo can still bring them back (a new edit would delete them).
    pub fn agent_hold(&self) -> bool {
        self.agent_hold && self.editor.can_redo()
    }

    /// The person lets the agent go on: the undone steps stay undone and can no longer be redone
    /// after the next edit of the agent.
    pub fn release_agent_hold(&mut self) {
        self.agent_hold = false;
    }

    /// Runs a request of an agent and records the step. See [`Session::handle_as`].
    pub(crate) fn handle_agent_call(
        &mut self,
        conn: u64,
        method: &str,
        p: Value,
    ) -> Result<Value, RpcError> {
        self.agent_call_with(conn, method, |s| s.handle(method, p))
    }

    /// The call of an agent with the work given as a function (a seam for tests that need a call
    /// which edits and then fails).
    pub(crate) fn agent_call_with(
        &mut self,
        conn: u64,
        method: &str,
        run: impl FnOnce(&mut Session) -> Result<Value, RpcError>,
    ) -> Result<Value, RpcError> {
        let now = review_api::default_now();
        let started = self.activity.touch(conn, now);
        let changes = may_change_document(method);
        if !self.editor.can_redo() {
            self.agent_hold = false;
        }
        if changes && self.agent_hold && !matches!(method, "undo" | "redo") {
            return Err(RpcError::conflict(HOLD_REASON).with_reason("redo_pending"));
        }
        if self.session_version && started.needs_version && starts_session_version(method) {
            self.activity.version_attempted(started.session);
            self.save_session_version(started.session, now);
        }
        let mark = self.editor.serial_mark();
        let revision = self.editor.revision();
        // A call that fails or panics after some changes has made steps: they are logged too, so
        // the person sees what the agent did (the call itself does not roll back).
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(self)));
        if self.editor.revision() != revision {
            self.record_call(conn, method, now, mark);
        }
        match result {
            Ok(r) => r,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Records what a call changed. An undo or redo of the agent is a step too (it can take back
    /// a step of the person), but not one that can be reverted.
    fn record_call(&mut self, conn: u64, method: &str, now: u64, mark: u64) {
        let serials = self.editor.steps_since(mark);
        let revision = self.editor.revision();
        let recorded = if !serials.is_empty() {
            self.editor.step_data(&serials).map(|data| Recorded {
                conn,
                method: method.to_string(),
                time_ms: now,
                revision,
                serials,
                data,
                partial: self.editor.trimmed_since(mark),
                kind: StepKind::Call,
            })
        } else if matches!(method, "undo" | "redo") {
            self.editor.last_moved().and_then(|(serial, forward)| {
                let data = self.editor.step_data(&[serial])?;
                // An undo shows what it took away: the change of the step the other way round.
                let data = if forward { data } else { data.inverted() };
                Some(Recorded {
                    conn,
                    method: method.to_string(),
                    time_ms: now,
                    revision,
                    serials: Vec::new(),
                    data,
                    partial: false,
                    kind: if forward {
                        StepKind::Redo
                    } else {
                        StepKind::Undo
                    },
                })
            })
        } else {
            None
        };
        if let Some(r) = recorded {
            self.activity.record(r);
        }
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

    /// Can the step be reverted now, and what stands in the way? Looks at every object of the
    /// step, so a host asks when it needs the answer, not for every frame.
    pub fn revert_check(&mut self, step_id: u64) -> Result<StepCheck, StepError> {
        let step = self
            .activity
            .get(step_id)
            .ok_or(StepError::Unknown)?
            .clone();
        let place = self.step_place(&step);
        let origins = step
            .serials
            .iter()
            .last()
            .map(|s| self.editor.origins_above(*s))
            .unwrap_or_default();
        // Steps of the person (and of other users of a shared document) are the ones that the
        // person did not expect to lose; steps of agents and of their scripts are not.
        let above_other = origins
            .iter()
            .filter(|o| matches!(o, Origin::User | Origin::Remote))
            .count();
        let shared = crate::collab_api::is_connected(self);
        let is_call = step.kind == StepKind::Call && !step.serials.is_empty();
        let exact = matches!(place, StepPlace::Undo(0)) && !shared;
        let analyse =
            is_call && !step.partial && !step.reverted && place != StepPlace::Redo && !exact;
        let (conflicts, gone) = match &step.data {
            Some(d) if analyse && !d.has_snapshot_parts() => self.editor.revert_conflicts(d),
            _ => (Vec::new(), Vec::new()),
        };
        let snapshot_blocked = step
            .data
            .as_ref()
            .is_some_and(|d| d.has_snapshot_parts() && !exact);
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
            shared,
            is_call,
            partial: step.partial,
            reverted: step.reverted,
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
    /// A step that the history forgot goes back object by object too.
    pub fn revert_agent_step(
        &mut self,
        step_id: u64,
        skip_conflicts: bool,
    ) -> Result<RevertReport, StepError> {
        let step = self.activity.get(step_id).ok_or(StepError::Unknown)?;
        if step.kind != StepKind::Call || step.serials.is_empty() {
            return Err(StepError::NotRevertible(Why::NotACall));
        }
        if step.partial {
            return Err(StepError::NotRevertible(Why::Partial));
        }
        let data = step.data.clone().ok_or(StepError::DetailsDropped)?;
        let serials = step.serials.clone();
        let allow_undo = !crate::collab_api::is_connected(self);
        let origin = self.editor.origin();
        self.editor.set_origin(Origin::User);
        let r = self
            .editor
            .revert_step(&serials, &data, skip_conflicts, allow_undo);
        self.editor.set_origin(origin);
        self.prune_selection();
        match r {
            Ok(report) => {
                if report.skipped.is_empty() {
                    self.activity.set_reverted(step_id);
                }
                Ok(report)
            }
            Err(e) => Err(StepError::Refused(e)),
        }
    }

    /// Takes back every step down to this one of the agent (the steps of the person above it
    /// too: redo brings them back). Returns how many undo steps went. While redo holds steps of
    /// the person, calls of the agent that change the document wait (`agent_hold`).
    pub fn undo_back_to_agent_step(&mut self, step_id: u64) -> Result<usize, StepError> {
        let step = self.activity.get(step_id).ok_or(StepError::Unknown)?;
        let first = *step.serials.first().ok_or(StepError::Unknown)?;
        if crate::collab_api::is_connected(self) {
            return Err(StepError::Refused(Refusal::Shared));
        }
        let person_steps = self
            .editor
            .origins_above(first)
            .iter()
            .any(|o| matches!(o, Origin::User | Origin::Remote));
        let origin = self.editor.origin();
        self.editor.set_origin(Origin::User);
        let r = self.editor.undo_through(first);
        self.editor.set_origin(origin);
        self.prune_selection();
        if r.is_ok() && person_steps {
            self.agent_hold = true;
        }
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

fn kind_name(k: StepKind) -> &'static str {
    match k {
        StepKind::Call => "call",
        StepKind::Undo => "undo",
        StepKind::Redo => "redo",
    }
}

fn entry(s: &Session, step: &Step, check: Option<&StepCheck>) -> Value {
    let mut v = json!({
        "id": step.id,
        "session": step.session,
        "time_ms": step.time_ms,
        "method": step.method,
        "kind": kind_name(step.kind),
        "summary": step.summary.text(),
        "counts": {
            "objects": counts(step.summary.objects),
            "layers": counts(step.summary.layers),
            "classes": counts(step.summary.classes),
            "resources": counts(step.summary.resources),
        },
        "seen": step.seen,
        "reverted": step.reverted,
        "partial": step.partial,
        "state": check.map_or("gone", |c| state(c.place)),
        "revertible": check.map(|c| c.why().is_none()),
    });
    if let Some(id) = s.activity.session_version(step.session) {
        v["session_version"] = json!(id);
    }
    if let Some(note) = s.activity.version_note(step.session) {
        v["version_note"] = json!(format!(
            "no version was saved at the start of the session: {note}"
        ));
    }
    v
}

pub fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    if method != AGENT_ACTIVITY.0 {
        return None;
    }
    Some(params::<AgentActivityParams>(p).and_then(|q| {
        let (step, since, limit) = (
            q.step.map(|n| n.0.max(0) as u64),
            q.since.map(|n| n.0.max(0) as u64),
            q.limit.map(|n| n.0.max(0) as usize),
        );
        if let Some(id) = step {
            let step = s
                .activity
                .get(id)
                .ok_or_else(|| {
                    RpcError::not_found(format!("there is no step {id} in the activity log"))
                })?
                .clone();
            let check = s.revert_check(id).ok();
            let mut v = entry(s, &step, check.as_ref());
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
                    json!(
                        ids.iter()
                            .take(MAX_IDS)
                            .map(|i| i.0.to_string())
                            .collect::<Vec<_>>()
                    ),
                );
            }
            v["objects"] = Value::Object(lists);
            v["truncated"] = json!(truncated);
            if step.data.is_none() {
                v["objects"] = Value::Null;
            }
            if let Some(why) = check.and_then(|c| c.blocked()) {
                v["blocked"] = json!(why);
            }
            return Ok(v);
        }
        let limit = limit.unwrap_or(50).min(MAX_LIMIT);
        let all: Vec<Step> = s.activity.since(since.unwrap_or(0)).cloned().collect();
        let skip = all.len().saturating_sub(limit);
        let shown: Vec<&Step> = all.iter().skip(skip).collect();
        let first_checked = shown.len().saturating_sub(CHECKED);
        let mut steps = Vec::with_capacity(shown.len());
        for (i, st) in shown.iter().enumerate() {
            let check = if i >= first_checked {
                s.revert_check(st.id).ok()
            } else {
                None
            };
            let mut e = entry(s, st, check.as_ref());
            if check.is_none() {
                e["state"] = json!(null);
            }
            steps.push(e);
        }
        Ok(json!({
            "steps": steps,
            "last_id": s.activity.last_id(),
            "total": s.activity.len(),
        }))
    }))
}

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "concept": "Every call or script run that changes the document is one step of the agent, also a call that failed after it had made changes, and an undo or redo of the agent (kind undo or redo). The person reviews the steps in the Activity panel: what each step added, changed and removed (highlighted in the plan and in 3D), the time, the method and a summary, and can undo back to a step or revert one step. agent.activity reads the same list. In the app (not in the headless server unless it is started with --session-version) a named version \"Before agent session\" is saved before the first change of a session (a new connection, or a call after 30 minutes of silence); if the limit of versions refuses it, the step says so in version_note. The version is an undo step of its own below your first step.",
        "revert": "The person can revert one step although later steps exist. Objects of the step that something changed later (by the person or by you) are never overwritten: the revert is refused, or reverts the rest and leaves exactly those objects. An object that something made later hangs on (a data tag, a dimension, a member of a group, the wall of a pad) is not removed either. A step that also changed layers, classes, resources, settings, versions or issues can only be undone together with the steps above it. A step that the undo history forgot can still go back object by object. Your next steps may therefore find objects gone or as they were: read before you rely on an ID.",
        "hold": "When the person uses Undo to here and steps of their own are among the undone ones, your calls that change the document fail with a conflict (reason redo_pending) until the person redoes them or releases the hold. Reads and view calls still work.",
        "script_runs": "One script run (script.eval) is one step, whatever number of calls it made.",
        "limits": "The log keeps 500 steps and the states of at most 100000 objects and about 256 MiB; older steps lose their states first and cannot be reverted then. A step is partial when the undo history limit dropped part of what a call did while it ran.",
        "examples": [
            {"method": "agent.activity", "params": {"limit": 10}},
            {"method": "agent.activity", "params": {"step": 3}},
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_core::command::Command;
    use kadakas_core::model::Shape;

    fn add(s: &mut Session, x: f64) {
        s.editor
            .execute(&Command::AddObject {
                shape: Shape::Line {
                    a: glam::DVec2::new(x, 0.0),
                    b: glam::DVec2::new(x + 1.0, 1.0),
                },
                layer: None,
                class: None,
                attrs: Default::default(),
            })
            .unwrap();
    }

    #[test]
    fn a_call_that_edits_and_then_fails_is_still_logged() {
        let mut s = Session::with_sequential_ids();
        s.editor.set_origin(Origin::Agent);
        let r = s.agent_call_with(3, "plants.place", |s| {
            add(s, 0.0);
            add(s, 5.0);
            Err(RpcError::conflict("the second half failed"))
        });
        assert!(r.is_err(), "the error reaches the agent");
        assert_eq!(s.activity.len(), 1);
        let step = &s.activity.steps()[0];
        assert_eq!(step.serials.len(), 2);
        assert_eq!(step.summary.objects.added, 2);
        assert_eq!(step.method, "plants.place");
        // The person can revert what the failed call left behind.
        assert!(s.revert_check(step.id).unwrap().why().is_none());
        // A failed call that changed nothing is no step.
        let _ = s.agent_call_with(3, "plants.place", |_| Err(RpcError::conflict("no")));
        assert_eq!(s.activity.len(), 1);
    }

    #[test]
    fn a_call_that_edits_and_then_panics_is_logged_and_the_panic_goes_on() {
        let mut s = Session::with_sequential_ids();
        s.editor.set_origin(Origin::Agent);
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = s.agent_call_with(3, "plants.place", |s| {
                add(s, 0.0);
                panic!("a bug in a method");
            });
        }));
        assert!(caught.is_err());
        assert_eq!(s.activity.len(), 1);
        assert_eq!(s.activity.steps()[0].summary.objects.added, 1);
    }

    fn lot_with_an_aisle(s: &mut Session) -> Value {
        let st = s
            .handle(
                "hardscape_style.create",
                json!({"name": "Pavers", "thickness": 80,
                       "joints": {"kind": "running_bond", "unit_length": 200, "unit_width": 100, "spacing": 5}}),
            )
            .unwrap()["id"]
            .clone();
        let id = s
            .handle(
                "hardscape.add",
                json!({"boundary": [[0, 0], [8000, 0], [8000, 4000], [0, 4000]], "style": st}),
            )
            .unwrap()["id"]
            .clone();
        s.handle(
            "hardscape.split",
            json!({"id": id, "lines": [[[3000, -50], [3000, 4050]]],
                   "zones": [{"seed": [6000, 2000], "name": "Aisle", "angle": 0}]}),
        )
        .unwrap();
        id
    }

    #[test]
    fn a_step_that_changes_one_object_names_what_changed() {
        let mut s = Session::with_sequential_ids();
        let id = lot_with_an_aisle(&mut s);
        s.editor.set_origin(Origin::Agent);
        s.agent_call_with(3, "hardscape.zone_set", |s| {
            s.handle("hardscape.zone_set", json!({"id": id, "zone": 1, "angle": 45}))
        })
        .unwrap();
        let step = s.activity.steps().back().unwrap();
        let text = step.summary.text();
        assert!(
            text.contains("hardscape 'Pavers'")
                && text.contains("zone 'Aisle' angle 0° → 45°"),
            "{text}"
        );
        // Three changes: the first two and a count.
        s.agent_call_with(3, "hardscape.set", |s| {
            s.handle(
                "hardscape.set",
                json!({"ids": [id], "elevation": 30, "origin": [10, 20], "thickness": 90}),
            )
        })
        .unwrap();
        let text = s.activity.steps().back().unwrap().summary.text();
        assert!(
            text.contains("origin set to (10, 20)") && text.contains("elevation"),
            "{text}"
        );
        // A step that changes two objects stays a count.
        let other = s
            .handle("hardscape.add", json!({"boundary": [[0, 0], [10, 0], [10, 10], [0, 10]], "material": "Gravel", "thickness": 50}))
            .unwrap()["id"]
            .clone();
        s.agent_call_with(3, "hardscape.set", |s| {
            s.handle("hardscape.set", json!({"ids": [id, other], "elevation": 5}))
        })
        .unwrap();
        let text = s.activity.steps().back().unwrap().summary.text();
        assert!(text.starts_with("Changed 2 objects"), "{text}");
    }
}
