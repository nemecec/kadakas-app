//! Feedback from agents (API-47): `feedback.submit`, `feedback.list`, `feedback.get`,
//! `feedback.withdraw`, the hints in error replies, and the context of a report.
//!
//! The pure logic (scrubbing, dedupe, caps, the outbox, consent) is in `kadakas-feedback`. This
//! module adds what needs the session: the recent calls, the last failure, and the attachments
//! (a model excerpt and a render of the view).
//!
//! No method here approves an attachment. A report with attachments waits as `needs_review`
//! until a human uses the review of the app or `kadakas feedback review` (see [`review`]).

pub mod excerpt;
pub mod review;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Once;

use kadakas_core::model::ObjectId;
use kadakas_core::uid::Uid;
use kadakas_feedback::{
    AttachmentData, AttachmentKind, Context, Draft, FeedbackError, Hub, Kind, MAX_CALLS,
    MAX_CALLS_BYTES, MAX_ERROR, MAX_IMAGE_BYTES, MAX_LOG_BYTES, MAX_LOG_LINES, Origin, Report,
    Severity, scrub,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::errors::RpcError;
use crate::{Session, params};

pub const FEEDBACK_SUBMIT: (&str, &str) = (
    "feedback.submit",
    "File a feature request or a bug report. Do it when a task needs a feature that Kadakas lacks (kind feature_request), and when a method fails in a way it must not (kind bug). params: {kind: bug|feature_request|question, title, body?, wanted?, tried?, method?, error?, severity?: low|medium|high, attach?}. A bug report attaches the failing objects, a picture of the view and the recent calls by default; attach {excerpt?, screenshot?, calls?, log?: false} removes one, attach {objects: [id]} or {selection: true} names objects, attach {view: {render.png params}} sets the picture. Attachments never leave the computer until a human approves them (status needs_review). Returns {id, merged, count, status, attachments}.",
);
pub const FEEDBACK_LIST: (&str, &str) = (
    "feedback.list",
    "The reports in the outbox. params: {status?, limit?}. Returns {reports: [{id, kind, title, status, count, ...}], needs_review, sending}.",
);
pub const FEEDBACK_GET: (&str, &str) = (
    "feedback.get",
    "One report with its attachments (kinds and sizes). params: {id}.",
);
pub const FEEDBACK_WITHDRAW: (&str, &str) = (
    "feedback.withdraw",
    "Take back a report that has not been sent. params: {id}. Returns the report.",
);

const METHODS: [&str; 4] = [
    "feedback.submit",
    "feedback.list",
    "feedback.get",
    "feedback.withdraw",
];

/// Calls kept for the call log, and how much of each.
const RING: usize = 60;
const LOG_RING: usize = 80;
/// The calls of a report: the newest ones.
const REPORT_CALLS: usize = 20;
/// Objects that an excerpt takes when nothing else says which.
const NEAR_OBJECTS: usize = 40;

/// What the session remembers for reports. All of it is bounded.
pub struct FeedbackState {
    /// The outbox, the settings and the consent. A host sets the storage (`Hub::set_storage`).
    pub hub: Hub,
    calls: VecDeque<Value>,
    log: VecDeque<String>,
    failure: Option<Failure>,
    client: Option<(String, String)>,
}

struct Failure {
    method: String,
    error: String,
    ids: Vec<Uid>,
}

impl Default for FeedbackState {
    fn default() -> Self {
        FeedbackState {
            hub: Hub::in_memory(),
            calls: VecDeque::new(),
            log: VecDeque::new(),
            failure: None,
            client: None,
        }
    }
}

impl FeedbackState {
    /// Adds a line to the log that bug reports can carry. It is scrubbed and cut.
    pub fn log(&mut self, line: &str) {
        if self.log.len() >= LOG_RING {
            self.log.pop_front();
        }
        self.log.push_back(scrub::text(line, 200));
    }

    /// The name of the MCP client (from `initialize`), added to reports.
    pub fn set_client(&mut self, name: &str, version: &str) {
        self.client = Some((scrub::text(name, 60), scrub::text(version, 60)));
    }

    /// Forgets the calls and the last failure. A new document starts a new story. The log lines,
    /// the outbox, the consent, the rate limit and the client stay.
    pub fn clear_context(&mut self) {
        self.calls.clear();
        self.failure = None;
    }

    /// The recent calls, newest last (what the call log of a report is made from).
    pub fn recent_calls(&self) -> Vec<Value> {
        self.calls.iter().cloned().collect()
    }
}

pub fn knows(method: &str) -> bool {
    METHODS.contains(&method)
}

fn is_feedback(method: &str) -> bool {
    method.starts_with("feedback.") || method == "describe"
}

/// The first part of a call: a short, scrubbed picture of the params and the ids in them.
pub(crate) struct Begun {
    method: String,
    params: Value,
    ids: Vec<Uid>,
}

pub(crate) fn begin(method: &str, p: &Value) -> Option<Begun> {
    if is_feedback(method) {
        return None;
    }
    Some(Begun {
        method: scrub::text(method, 80),
        params: scrub::json(p, 3, 6, 80),
        ids: excerpt::uids_in(p, 50).into_iter().collect(),
    })
}

/// The last part: remembers the call, and the error if there was one.
pub(crate) fn finish(s: &mut Session, b: Option<Begun>, result: &Result<Value, RpcError>) {
    let Some(b) = b else { return };
    let f = &mut s.feedback;
    if f.calls.len() >= RING {
        f.calls.pop_front();
    }
    let mut entry = json!({"method": b.method});
    if !b.params.is_null() && b.params != json!({}) {
        entry["params"] = b.params;
    }
    if let Err(e) = result {
        // The error text is filled in from the session, so the harder scrub applies: layer and
        // object names in quotes and the stems of file names go.
        let error = scrub::harsh(&format!("{} (code {}, {})", e.message, e.code, e.kind));
        entry["error"] = json!(scrub::cut(&error, 200));
        f.log(&format!("{}: {error}", b.method));
        f.failure = Some(Failure {
            method: b.method,
            error: scrub::cut(&error, MAX_ERROR),
            ids: b.ids,
        });
    }
    f.calls.push_back(entry);
}

thread_local! {
    /// The message and place of the last panic on this thread, left by the hook.
    static LAST_PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}

static HOOK: Once = Once::new();

/// Installs (once) a panic hook that keeps the message and the place of a panic for the report
/// and then calls the hook that was there before (so the message still goes to stderr).
fn install_panic_hook() {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let payload = info.payload();
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".into());
            let place = info
                .location()
                .map(|l| format!(" at {}:{}", l.file(), l.line()))
                .unwrap_or_default();
            LAST_PANIC.with(|p| *p.borrow_mut() = Some(format!("{msg}{place}")));
            previous(info);
        }));
    });
}

impl Session {
    /// Runs `f` so that a panic becomes an internal error (code -32603) and the session goes on.
    /// The command that panicked took its own changes back (`Editor::execute`). The session is
    /// marked `tainted`, because other state may be half way through a change. The panic is a bug
    /// of Kadakas, so the message and the place go to the log lines and the failure of the
    /// session, where a bug report finds them. In a program built with `panic = "abort"`, such as
    /// the web build, a panic ends the program and this function never sees it.
    pub fn is_tainted(&self) -> bool {
        self.tainted
    }

    pub(crate) fn guarded<T>(
        &mut self,
        method: &str,
        f: impl FnOnce(&mut Session) -> Result<T, RpcError>,
    ) -> Result<T, RpcError> {
        install_panic_hook();
        let depth = self.editor.undo_depth();
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self))) {
            Ok(r) => r,
            Err(_) => {
                // The command that panicked took its own changes back. The steps that the method
                // finished before it are taken back here, as `enforce_limits` does, so the
                // method is all or nothing.
                let steps = self.editor.undo_depth().saturating_sub(depth);
                self.editor.rollback(steps);
                self.prune_selection();
                let detail = LAST_PANIC.with(|p| p.borrow_mut().take());
                let detail = scrub::harsh(&detail.unwrap_or_else(|| "panic".into()));
                self.tainted = true;
                let method = scrub::text(method, 80);
                let error = scrub::cut(&format!("internal error: {detail}"), MAX_ERROR);
                self.feedback.log(&format!("{method}: {error}"));
                self.feedback.failure = Some(Failure {
                    method: method.clone(),
                    error,
                    ids: Vec::new(),
                });
                Err(RpcError::internal(format!("internal error in {method}")))
            }
        }
    }
}

fn now() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

fn rpc(e: FeedbackError) -> RpcError {
    match e {
        FeedbackError::Invalid(m) => RpcError::invalid_params(m),
        FeedbackError::Limit(m) => RpcError::limit(m),
        FeedbackError::NotFound(m) => RpcError::not_found(m),
        FeedbackError::State(m) => RpcError::conflict(m),
        FeedbackError::Store(m) => RpcError::app(m),
    }
}

/// The context that Kadakas adds to every report.
pub fn context(client: Option<&(String, String)>) -> Context {
    Context {
        version: env!("CARGO_PKG_VERSION").into(),
        os: std::env::consts::OS.into(),
        client_name: client.map(|c| c.0.clone()).unwrap_or_default(),
        client_version: client.map(|c| c.1.clone()).unwrap_or_default(),
    }
}

/// What the caller asked to attach. Missing fields take the default of the kind of report.
#[derive(Default)]
struct Attach {
    objects: Option<Vec<ObjectId>>,
    selection: bool,
    excerpt: Option<bool>,
    picture: Option<bool>,
    view: Option<Value>,
    calls: Option<bool>,
    log: Option<bool>,
}

fn parse_attach(v: Option<&Value>) -> Result<Attach, RpcError> {
    let mut a = Attach::default();
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(a);
    };
    if let Some(all) = v.as_bool() {
        // `attach: false` removes all, `true` is the default.
        if !all {
            a.excerpt = Some(false);
            a.picture = Some(false);
            a.calls = Some(false);
            a.log = Some(false);
        }
        return Ok(a);
    }
    let o = v
        .as_object()
        .ok_or_else(|| RpcError::invalid_params("attach must be an object or a bool"))?;
    let flag = |k: &str| o.get(k).and_then(Value::as_bool);
    a.excerpt = flag("excerpt");
    a.picture = flag("screenshot").or(flag("render"));
    a.calls = flag("calls");
    a.log = flag("log");
    a.selection = flag("selection") == Some(true);
    a.view = o.get("view").filter(|v| v.is_object()).cloned();
    if let Some(list) = o.get("objects") {
        let list = list
            .as_array()
            .ok_or_else(|| RpcError::invalid_params("attach.objects must be a list of ids"))?;
        let ids: Option<Vec<ObjectId>> = list
            .iter()
            .take(kadakas_feedback::MAX_EXCERPT_OBJECTS)
            .map(|x| x.as_str().and_then(Uid::parse).map(ObjectId))
            .collect();
        a.objects =
            Some(ids.ok_or_else(|| RpcError::invalid_params("attach.objects holds a bad id"))?);
    }
    Ok(a)
}

#[derive(Deserialize)]
struct SubmitParams {
    kind: Kind,
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    wanted: String,
    #[serde(default)]
    tried: String,
    #[serde(default)]
    method: String,
    #[serde(default)]
    error: String,
    #[serde(default)]
    severity: Option<Severity>,
    #[serde(default)]
    client: Option<ClientParam>,
    #[serde(default)]
    attach: Option<Value>,
}

#[derive(Deserialize)]
struct ClientParam {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
}

impl Session {
    /// The objects an excerpt takes: those named, else (for a bug) those of the failing call, else
    /// the selection, else (for a bug) the ones in the middle of the drawing.
    fn excerpt_ids(&self, kind: Kind, a: &Attach) -> Vec<ObjectId> {
        let doc = self.editor.doc();
        if let Some(ids) = &a.objects {
            return ids.clone();
        }
        let selected = || self.selection.clone();
        if a.selection {
            return selected();
        }
        if kind == Kind::FeatureRequest {
            return selected();
        }
        if kind != Kind::Bug {
            return Vec::new();
        }
        if let Some(f) = &self.feedback.failure {
            let ids: Vec<ObjectId> = f
                .ids
                .iter()
                .copied()
                .map(ObjectId)
                .filter(|i| doc.object(*i).is_some())
                .collect();
            if !ids.is_empty() {
                return ids;
            }
        }
        if !self.selection.is_empty() {
            return selected();
        }
        near_middle(doc, NEAR_OBJECTS)
    }

    fn picture(&mut self, view: Option<&Value>) -> Option<Vec<u8>> {
        use base64::Engine;
        if self.editor.doc().objects.is_empty() {
            return None;
        }
        // One render. A picture over the limit is left out, not made again.
        let mut p = view.cloned().unwrap_or_else(|| json!({}));
        if p.get("width").is_none() {
            p["width"] = json!(640);
            p["height"] = json!(480);
        }
        let r = self.dispatch("render.png", p).ok()?;
        let png = base64::engine::general_purpose::STANDARD
            .decode(r["png_base64"].as_str()?)
            .ok()?;
        (png.len() <= MAX_IMAGE_BYTES).then_some(png)
    }

    fn call_log(&self) -> Option<Vec<u8>> {
        let mut calls: Vec<Value> = self
            .feedback
            .calls
            .iter()
            .rev()
            .take(REPORT_CALLS.min(MAX_CALLS))
            .cloned()
            .collect();
        calls.reverse();
        while !calls.is_empty() {
            let t = serde_json::to_vec(&calls).ok()?;
            if t.len() <= MAX_CALLS_BYTES {
                return Some(t);
            }
            calls.remove(0);
        }
        None
    }

    fn log_text(&self) -> Option<Vec<u8>> {
        let mut text = self
            .feedback
            .log
            .iter()
            .rev()
            .take(MAX_LOG_LINES)
            .cloned()
            .collect::<Vec<_>>();
        text.reverse();
        let mut t = text.join("\n");
        while t.len() > MAX_LOG_BYTES {
            t = t.split_once('\n')?.1.to_string();
        }
        (!t.is_empty()).then(|| t.into_bytes())
    }

    fn feedback_submit(&mut self, p: Value) -> Result<Value, RpcError> {
        let p: SubmitParams = params(p)?;
        let a = parse_attach(p.attach.as_ref())?;
        let mut d = Draft::new(p.kind, &p.title, &p.body, Origin::Agent);
        d.wanted = p.wanted;
        d.tried = p.tried;
        d.method = p.method;
        d.error = p.error;
        d.severity = p.severity;
        // A bug report without a method or an error takes them from the last failed call.
        if p.kind == Kind::Bug
            && let Some(f) = &self.feedback.failure
        {
            if d.method.trim().is_empty() {
                d.method = f.method.clone();
            }
            if d.error.trim().is_empty() {
                d.error = f.error.clone();
            }
        }
        let client = p
            .client
            .map(|c| (scrub::text(&c.name, 60), scrub::text(&c.version, 60)))
            .filter(|c| !c.0.is_empty())
            .or_else(|| self.feedback.client.clone());
        d.context = context(client.as_ref());

        let mut att: Vec<AttachmentData> = Vec::new();
        // An equal report only raises a counter and drops new attachments, so make none.
        let merges = self
            .feedback
            .hub
            .would_merge(p.kind, &p.title, &d.method, Origin::Agent);
        let wants = |flag: Option<bool>, default: bool| !merges && flag.unwrap_or(default);
        let bug = p.kind == Kind::Bug;
        let feature = p.kind == Kind::FeatureRequest;
        if wants(
            a.excerpt,
            bug || feature || a.objects.is_some() || a.selection,
        ) {
            let ids = self.excerpt_ids(p.kind, &a);
            if let Some(ex) = excerpt::build(self.editor.doc(), &ids) {
                att.push(AttachmentData {
                    kind: AttachmentKind::Excerpt,
                    bytes: ex.json.into_bytes(),
                    objects: Some(ex.objects),
                });
            }
        }
        if wants(a.picture, bug || feature || a.view.is_some())
            && let Some(png) = self.picture(a.view.as_ref())
        {
            att.push(AttachmentData {
                kind: AttachmentKind::Render,
                bytes: png,
                objects: None,
            });
        }
        if wants(a.calls, bug || feature)
            && let Some(b) = self.call_log()
        {
            att.push(AttachmentData {
                kind: AttachmentKind::Calls,
                bytes: b,
                objects: None,
            });
        }
        if wants(a.log, bug)
            && let Some(b) = self.log_text()
        {
            att.push(AttachmentData {
                kind: AttachmentKind::Log,
                bytes: b,
                objects: None,
            });
        }
        let kinds: Vec<&str> = att.iter().map(|x| x.kind.id()).collect();
        let kinds: Vec<String> = kinds.into_iter().map(String::from).collect();
        let out = self.feedback.hub.submit(d, att, now()).map_err(rpc)?;
        let mut r = json!({
            "id": out.id,
            "merged": out.merged,
            "count": out.count,
            "status": out.status,
            "attachments": if out.merged { json!([]) } else { json!(kinds) },
        });
        r["note"] = json!(status_note(&self.feedback.hub, out.status));
        Ok(r)
    }
}

/// A short sentence about what happens next to a report with this status.
fn status_note(hub: &Hub, status: kadakas_feedback::Status) -> &'static str {
    use kadakas_feedback::Status::*;
    match status {
        NeedsReview => {
            "The attachments stay on this computer until a human approves them in Help > Feedback or with `kadakas feedback review`."
        }
        Pending if hub.send_enabled() => "Queued. It goes to the server in this run.",
        Pending => {
            "Saved in the outbox. It is sent only after the person agrees (Help > Feedback, or --feedback-send)."
        }
        Approved => "Approved by a human. It goes to the server.",
        Sent => "Sent.",
        Withdrawn => "Withdrawn.",
    }
}

/// The objects nearest to the middle of the drawing, at most `n`.
fn near_middle(doc: &kadakas_core::model::Document, n: usize) -> Vec<ObjectId> {
    let boxes: Vec<_> = doc
        .objects
        .values()
        .map(|o| (o.id, doc.object_bounds(o)))
        .filter(|(_, (a, b))| a.is_finite() && b.is_finite())
        .collect();
    let Some((lo, hi)) = boxes
        .iter()
        .map(|b| b.1)
        .reduce(|(a0, a1), (b0, b1)| (a0.min(b0), a1.max(b1)))
    else {
        return Vec::new();
    };
    let mid = (lo + hi) / 2.0;
    let mut v: Vec<_> = boxes
        .into_iter()
        .map(|(id, (a, b))| (id, ((a + b) / 2.0).distance_squared(mid)))
        .collect();
    v.sort_by(|x, y| x.1.total_cmp(&y.1));
    v.into_iter().take(n).map(|x| x.0).collect()
}

/// A report of an agent. The reports of people are not visible to agents: they are "not found".
fn agent_report(hub: &mut Hub, id: &str) -> Result<Report, RpcError> {
    hub.get(id)
        .filter(|r| r.origin == Origin::Agent)
        .ok_or_else(|| RpcError::not_found(format!("no report {}", scrub::text(id, 40))))
}

fn summary(r: &Report) -> Value {
    let mut v = json!({
        "id": r.id,
        "kind": r.kind,
        "title": r.title,
        "status": r.status,
        "count": r.count,
        "origin": r.origin,
        "attachments": r.attachments.iter().map(|a| a.kind.id()).collect::<Vec<_>>(),
    });
    if !r.method.is_empty() {
        v["method"] = json!(r.method);
    }
    if let Some(s) = &r.server_id {
        v["server_id"] = json!(s);
    }
    v
}

pub(crate) fn handle(s: &mut Session, method: &str, p: Value) -> Option<Result<Value, RpcError>> {
    Some(match method {
        "feedback.submit" => s.feedback_submit(p),
        "feedback.list" => (|| {
            #[derive(Deserialize)]
            struct P {
                status: Option<String>,
                limit: Option<usize>,
            }
            let P { status, limit } = params(p)?;
            let hub = &mut s.feedback.hub;
            let all = hub.list();
            // The reports of people are not for agents: they are a count.
            let people = all.iter().filter(|r| r.origin != Origin::Agent).count();
            let mut list: Vec<Report> = all
                .into_iter()
                .filter(|r| r.origin == Origin::Agent)
                .collect();
            list.reverse();
            let reports: Vec<Value> = list
                .iter()
                .filter(|r| {
                    status
                        .as_deref()
                        .is_none_or(|w| serde_json::to_value(r.status).ok() == Some(json!(w)))
                })
                .take(limit.unwrap_or(50).min(200))
                .map(summary)
                .collect();
            let waiting = list
                .iter()
                .filter(|r| r.status == kadakas_feedback::Status::NeedsReview)
                .count();
            Ok(json!({
                "reports": reports,
                "needs_review": waiting,
                "reports_of_people": people,
                "sending": {"enabled": hub.send_enabled(), "asked": hub.consent_asked()},
                "note": hub.note(),
            }))
        })(),
        "feedback.get" => (|| {
            #[derive(Deserialize)]
            struct P {
                id: String,
            }
            let P { id } = params(p)?;
            let r = agent_report(&mut s.feedback.hub, &id)?;
            let mut v = serde_json::to_value(&r).map_err(RpcError::app)?;
            v["note"] = json!(status_note(&s.feedback.hub, r.status));
            Ok(v)
        })(),
        "feedback.withdraw" => (|| {
            #[derive(Deserialize)]
            struct P {
                id: String,
            }
            let P { id } = params(p)?;
            agent_report(&mut s.feedback.hub, &id)?;
            let r = s.feedback.hub.withdraw(&id).map_err(rpc)?;
            serde_json::to_value(&r).map_err(RpcError::app)
        })(),
        _ => return None,
    })
}

/// The option or variant that a message names: the first word in quotes after `field`, `variant`
/// or `option`.
fn named_option(message: &str) -> Option<String> {
    let at = ["field", "variant", "option"]
        .iter()
        .filter_map(|w| message.find(w))
        .min()?;
    let rest = &message[at..];
    let open = rest.find(['`', '\'', '"'])?;
    let q = rest[open..].chars().next()?;
    let inner = &rest[open + 1..];
    let name = &inner[..inner.find(q)?];
    (!name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-')))
    .then(|| name.to_string())
}

/// The hint in an error reply: a short sentence and a draft for `feedback.submit`. Only the
/// errors that point to a missing feature or a bug have one. The draft has the kind, a title and
/// the method; the error is in the context of the session, so it is not repeated.
pub fn hint(method: &str, kind: &str, message: &str) -> Option<Value> {
    if method.starts_with("feedback.") {
        return None;
    }
    let lower = message.to_ascii_lowercase();
    const MISSING: &str = "If the name is right according to `describe` and the feature is missing, file a feature request with `feedback`.";
    let (draft_kind, title, text) = match kind {
        "method_not_found" => (
            "feature_request",
            format!("Method {method} is missing"),
            MISSING,
        ),
        "invalid_params"
            if [
                "unknown field",
                "unknown variant",
                "unknown option",
                "unsupported",
                "not supported",
            ]
            .iter()
            .any(|w| lower.contains(w)) =>
        {
            let title = match named_option(message) {
                Some(o) => format!("Option {o} not supported by {method}"),
                None => format!("Option not supported by {method}"),
            };
            ("feature_request", title, MISSING)
        }
        "internal" => (
            "bug",
            format!("Internal error in {method}"),
            "This is a bug in Kadakas. File it with `feedback`.",
        ),
        _ => return None,
    };
    Some(json!({
        "hint": text,
        "draft": {
            "kind": draft_kind,
            "title": scrub::text(&title, 100),
            "method": scrub::text(method, 80),
        },
    }))
}

pub fn describe() -> Value {
    json!({
        "purpose": "Tell the Kadakas team what is missing or broken. File a feature request (kind feature_request) when a task needs a feature that Kadakas lacks. File a bug report (kind bug) when a method fails in a way it must not. Do not work around it silently.",
        "methods": METHODS,
        "mcp_tool": "feedback: the same as feedback.submit, with action: submit (default) | list | get | withdraw.",
        "fields": "kind (bug | feature_request | question), title (short, one line), body (what happened or what you need), wanted (what you tried to do), tried (what you did), method, error, severity (low | medium | high). method and error of a bug report come from the last failed call if you leave them out.",
        "attachments": "A bug report attaches by default: the objects of the failing call (else the selection, else objects in the middle of the drawing), a picture of the current view, the recent calls and the recent log lines. A feature request attaches the selection, the picture and the recent calls. attach: {excerpt?, screenshot?, calls?, log?: false} removes one. attach: {objects: [id]} or {selection: true} names objects. attach: {view: {render.png params}} sets the picture. attach: false removes all.",
        "consent": "You cannot send attachments. A report with attachments gets status needs_review and waits in the outbox until a human approves it in Help > Feedback or with `kadakas feedback review`. A text-only report goes to the server only after the person agreed once.",
        "privacy": "Texts and the call log are scrubbed of paths (file names stay), tokens, keys and e-mail addresses. Write what you want to share. Do not put secrets in reports.",
        "limits": "Titles 120 characters, body 4000. 30 reports per session; equal reports (same kind, title and method) merge into a counter. The outbox holds 200 reports.",
        "examples": [
            {"feedback.submit": {"kind": "feature_request", "title": "Offset a polyline to both sides", "wanted": "Offset a path 500 mm to both sides in one call", "tried": "edit.offset takes one side"}},
            {"feedback.submit": {"kind": "bug", "title": "edit.fillet fails on a closed polyline", "body": "Radius 200 on a closed rectangle gives an error", "severity": "medium", "attach": {"log": false}}},
            {"feedback.submit": {"kind": "bug", "title": "Hatch lost after undo", "attach": {"objects": ["<id>"], "view": {"width": 800}}}},
            {"feedback.list": {}},
            {"feedback.get": {"id": "fb-1"}},
            {"feedback.withdraw": {"id": "fb-1"}},
        ],
        "errors": "An error of an unknown method, an unsupported option or an internal error has data.feedback: {hint, draft: {kind, title, method}}. Pass the draft to feedback.submit as it is, or edit it. The error of the last failed call is added to a bug report by itself.",
        "visibility": "You see and change the reports of agents only. The reports of people show as a count (reports_of_people).",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_feedback::{MemStorage, Status};

    fn s_with_plants() -> Session {
        let mut s = Session::with_sequential_ids();
        for i in 0..3 {
            s.handle(
                "execute",
                json!({"command": {"op": "add_object", "shape": {"kind": "plant", "at": [i * 1000, 0], "species": "Oak", "spread": 3000, "height": 9000}}}),
            )
            .unwrap();
        }
        s
    }

    fn submit(s: &mut Session, p: Value) -> Value {
        s.handle("feedback.submit", p).unwrap()
    }

    #[test]
    fn a_feature_request_attaches_selection_picture_and_calls() {
        let mut s = s_with_plants();
        s.selection = vec![ObjectId::from_num(2)];
        let r = submit(
            &mut s,
            json!({"kind": "feature_request", "title": "Need X", "wanted": "x"}),
        );
        assert_eq!(r["status"], "needs_review");
        assert_eq!(r["attachments"], json!(["excerpt", "render", "calls"]));
        let g = s.handle("feedback.get", json!({"id": r["id"]})).unwrap();
        assert_eq!(g["attachments"][0]["objects"], 1);
        assert!(g["note"].as_str().unwrap().contains("human"));
    }

    #[test]
    fn a_feature_request_without_a_selection_has_no_excerpt() {
        let mut s = s_with_plants();
        let r = submit(
            &mut s,
            json!({"kind": "feature_request", "title": "Need X"}),
        );
        assert_eq!(r["attachments"], json!(["render", "calls"]));
    }

    #[test]
    fn a_bug_takes_the_failure_its_objects_and_the_log() {
        let mut s = s_with_plants();
        let id = ObjectId::from_num(3);
        let bad = json!({"id": id, "radius": -5});
        assert!(s.handle("edit.fillet", bad).is_err());
        let r = submit(&mut s, json!({"kind": "bug", "title": "Fillet fails"}));
        assert_eq!(
            r["attachments"],
            json!(["excerpt", "render", "calls", "log"])
        );
        let g = s.handle("feedback.get", json!({"id": r["id"]})).unwrap();
        assert_eq!(g["method"], "edit.fillet");
        assert!(g["error"].as_str().unwrap().contains("code"));
        // The excerpt holds the object of the failing call, not all of them.
        assert_eq!(g["attachments"][0]["objects"], 1);
        let calls = s
            .feedback
            .hub
            .attachment("fb-1", AttachmentKind::Calls)
            .unwrap();
        let calls: Value = serde_json::from_slice(&calls).unwrap();
        assert_eq!(
            calls.as_array().unwrap().last().unwrap()["method"],
            "edit.fillet"
        );
    }

    #[test]
    fn a_bug_without_failure_or_selection_takes_objects_near_the_middle() {
        let mut s = s_with_plants();
        let r = submit(&mut s, json!({"kind": "bug", "title": "Odd"}));
        assert_eq!(r["attachments"][0], "excerpt");
        let g = s.handle("feedback.get", json!({"id": r["id"]})).unwrap();
        assert_eq!(g["attachments"][0]["objects"], 3);
    }

    #[test]
    fn attach_overrides_the_defaults() {
        let mut s = s_with_plants();
        let r = submit(
            &mut s,
            json!({"kind": "bug", "title": "a", "attach": {"excerpt": false, "screenshot": false, "calls": false, "log": false}}),
        );
        assert_eq!(
            (r["status"].as_str(), r["attachments"].clone()),
            (Some("pending"), json!([]))
        );
        let r = submit(
            &mut s,
            json!({"kind": "bug", "title": "b", "attach": false}),
        );
        assert_eq!(r["attachments"], json!([]));
        let id = ObjectId::from_num(2).to_string();
        let r = submit(
            &mut s,
            json!({"kind": "question", "title": "c", "attach": {"objects": [id], "view": {"width": 64, "height": 48}}}),
        );
        assert_eq!(r["attachments"], json!(["excerpt", "render"]));
        let r = submit(&mut s, json!({"kind": "question", "title": "d"}));
        assert_eq!(r["attachments"], json!([]));
        s.selection = vec![ObjectId::from_num(3)];
        let r = submit(
            &mut s,
            json!({"kind": "question", "title": "e", "attach": {"selection": true}}),
        );
        assert_eq!(r["attachments"], json!(["excerpt"]));
        for bad in [
            json!("x"),
            json!({"objects": "x"}),
            json!({"objects": ["nope"]}),
        ] {
            let e = s
                .handle(
                    "feedback.submit",
                    json!({"kind": "bug", "title": "f", "attach": bad}),
                )
                .unwrap_err();
            assert_eq!(e.code, -32602);
        }
    }

    #[test]
    fn an_empty_drawing_has_no_picture_or_excerpt() {
        let mut s = Session::with_sequential_ids();
        let r = submit(&mut s, json!({"kind": "bug", "title": "empty"}));
        assert_eq!(r["attachments"], json!([]));
        assert_eq!(r["status"], "pending");
    }

    #[test]
    fn the_client_and_context_are_added() {
        let mut s = Session::with_sequential_ids();
        s.feedback.set_client("claude-code", "1.2");
        submit(&mut s, json!({"kind": "question", "title": "who"}));
        let g = s.handle("feedback.get", json!({"id": "fb-1"})).unwrap();
        assert_eq!(g["context"]["client_name"], "claude-code");
        assert_eq!(g["context"]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(g["context"]["os"], std::env::consts::OS);
        assert_eq!(g["origin"], "agent");
        // A client in the params wins.
        submit(
            &mut s,
            json!({"kind": "question", "title": "who2", "client": {"name": "ann@example.com", "version": "9"}}),
        );
        let g = s.handle("feedback.get", json!({"id": "fb-2"})).unwrap();
        assert_eq!(g["context"]["client_name"], "<email>");
    }

    #[test]
    fn list_get_withdraw_and_errors() {
        let mut s = Session::with_sequential_ids();
        submit(&mut s, json!({"kind": "question", "title": "one"}));
        submit(&mut s, json!({"kind": "question", "title": "ONE"}));
        submit(&mut s, json!({"kind": "question", "title": "two"}));
        let l = s.handle("feedback.list", json!({})).unwrap();
        assert_eq!(l["reports"].as_array().unwrap().len(), 2);
        assert_eq!(l["reports"][1]["count"], 2);
        assert_eq!(l["sending"], json!({"enabled": false, "asked": false}));
        assert_eq!(l["needs_review"], 0);
        let l = s
            .handle("feedback.list", json!({"status": "sent"}))
            .unwrap();
        assert!(l["reports"].as_array().unwrap().is_empty());
        let l = s.handle("feedback.list", json!({"limit": 1})).unwrap();
        assert_eq!(l["reports"].as_array().unwrap().len(), 1);
        let w = s
            .handle("feedback.withdraw", json!({"id": "fb-1"}))
            .unwrap();
        assert_eq!(w["status"], "withdrawn");
        assert_eq!(
            s.handle("feedback.get", json!({"id": "nope"}))
                .unwrap_err()
                .code,
            2
        );
        assert_eq!(
            s.handle("feedback.withdraw", json!({"id": "nope"}))
                .unwrap_err()
                .code,
            2
        );
        assert_eq!(
            s.handle("feedback.submit", json!({"kind": "bug"}))
                .unwrap_err()
                .code,
            -32602
        );
        assert_eq!(
            s.handle("feedback.submit", json!({"kind": "bug", "title": " "}))
                .unwrap_err()
                .code,
            -32602
        );
        assert_eq!(
            s.handle("feedback.submit", json!({"kind": "idea", "title": "x"}))
                .unwrap_err()
                .code,
            -32602
        );
    }

    #[test]
    fn the_session_limit_is_a_limit_error() {
        let mut s = Session::with_sequential_ids();
        s.feedback.hub.set_max_submits(1);
        submit(&mut s, json!({"kind": "question", "title": "one"}));
        let e = s
            .handle(
                "feedback.submit",
                json!({"kind": "question", "title": "two"}),
            )
            .unwrap_err();
        assert_eq!(e.code, crate::errors::CODE_LIMIT);
    }

    #[test]
    fn secrets_in_a_report_and_in_the_call_log_are_scrubbed() {
        let mut s = s_with_plants();
        let secret = "Zq81xKd0Pa77";
        let _ = s.handle(
            "file.open",
            json!({"path": format!("/home/{secret}/plans/g.kad"), "token": secret}),
        );
        let _ = s.handle(
            "edit.nope",
            json!({"mail": format!("{secret}@example.com")}),
        );
        submit(
            &mut s,
            json!({"kind": "bug", "title": format!("fails for {secret}@example.com"), "body": format!("token={secret} in /Users/{secret}/a.kad")}),
        );
        let g = s.handle("feedback.get", json!({"id": "fb-1"})).unwrap();
        assert!(!g.to_string().contains(secret), "{g}");
        for kind in AttachmentKind::ALL {
            if let Some(b) = s.feedback.hub.attachment("fb-1", kind)
                && !matches!(kind, AttachmentKind::Render | AttachmentKind::Screenshot)
            {
                assert!(!String::from_utf8_lossy(&b).contains(secret), "{kind:?}");
            }
        }
    }

    #[test]
    fn nothing_can_approve_an_attachment_through_the_api() {
        let mut s = s_with_plants();
        s.feedback.hub.set_send_override(Some(true));
        let r = submit(&mut s, json!({"kind": "bug", "title": "wait"}));
        let id = r["id"].as_str().unwrap().to_string();
        // Every way an agent could try: other params, other methods, odd statuses.
        for (m, p) in [
            (
                "feedback.submit",
                json!({"kind": "bug", "title": "wait", "status": "approved", "approve": true, "approved": true}),
            ),
            ("feedback.get", json!({"id": id, "approve": true})),
            ("feedback.list", json!({"status": "approved"})),
            (
                "feedback.withdraw",
                json!({"id": id.clone(), "status": "approved"}),
            ),
            ("feedback.approve", json!({"id": id})),
            ("feedback.send", json!({"id": id})),
            ("feedback.review", json!({"id": id})),
        ] {
            let _ = s.handle(m, p);
        }
        // The report is gone (withdrawn) or waits; nothing is ready to send.
        assert!(s.feedback.hub.outgoing(true, 10, 1_000).is_empty());
        let st = s.feedback.hub.get(&id).unwrap().status;
        assert!(
            matches!(st, Status::Withdrawn | Status::NeedsReview),
            "{st:?}"
        );
        // A second report: with sending on, still nothing goes out until the approval path runs.
        let r = submit(&mut s, json!({"kind": "bug", "title": "wait too"}));
        assert_eq!(r["status"], "needs_review");
        assert!(s.feedback.hub.outgoing(true, 10, 1_000).is_empty());
    }

    #[test]
    fn hints_have_a_draft() {
        let h = hint(
            "plants.nope",
            "method_not_found",
            "unknown method: plants.nope",
        )
        .unwrap();
        assert_eq!(h["draft"]["kind"], "feature_request");
        assert_eq!(h["draft"]["method"], "plants.nope");
        assert!(h["hint"].as_str().unwrap().len() < 160);
        let h = hint("edit.offset", "invalid_params", "unknown field `side`").unwrap();
        assert_eq!(
            h["draft"]["title"],
            "Option side not supported by edit.offset"
        );
        assert!(h["draft"].get("error").is_none());
        assert!(h["hint"].as_str().unwrap().contains("`describe`"));
        let h = hint(
            "edit.offset",
            "invalid_params",
            "operation is not supported here",
        )
        .unwrap();
        assert_eq!(h["draft"]["title"], "Option not supported by edit.offset");
        let h = hint("a.b", "invalid_params", "unknown field 'x y'").unwrap();
        assert_eq!(h["draft"]["title"], "Option not supported by a.b");
        let h = hint("x.y", "internal", "internal error in x.y").unwrap();
        assert_eq!(h["draft"]["kind"], "bug");
        assert!(hint("x.y", "not_found", "object missing").is_none());
        assert!(hint("x.y", "invalid_params", "missing field `ids`").is_none());
        assert!(hint("feedback.get", "method_not_found", "x").is_none());
    }

    #[test]
    fn a_method_that_panics_after_some_steps_is_taken_back_whole() {
        let mut s = Session::with_sequential_ids();
        s.handle(
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "rect", "min": [0, 0], "max": [1, 1]}}}),
        )
        .unwrap();
        let (depth, count) = (s.editor.undo_depth(), s.editor.doc().objects.len());
        let add = |x: f64| kadakas_core::command::Command::AddObject {
            shape: kadakas_core::model::Shape::Circle {
                center: glam::DVec2::new(x, 0.0),
                radius: 1.0,
            },
            layer: None,
            class: None,
            attrs: Default::default(),
        };
        // A method with two steps; it panics after the second.
        let r: Result<(), RpcError> = s.guarded("many.steps", |s| {
            s.editor.execute(&add(1.0)).unwrap();
            s.editor.execute(&add(2.0)).unwrap();
            assert_eq!(s.editor.doc().objects.len(), count + 2);
            panic!("after two steps");
        });
        assert_eq!(r.unwrap_err().code, -32603);
        assert_eq!(s.editor.undo_depth(), depth);
        assert_eq!(s.editor.doc().objects.len(), count);
        assert!(!s.editor.can_redo());
        assert!(s.is_tainted());
        let failure = s.feedback.failure.as_ref().unwrap();
        assert!(
            failure.error.contains("after two steps"),
            "{}",
            failure.error
        );
        // A method that does not panic keeps its steps.
        let r: Result<(), RpcError> = s.guarded("fine", |s| {
            s.editor.execute(&add(3.0)).unwrap();
            Ok(())
        });
        assert!(r.is_ok());
        assert_eq!(s.editor.undo_depth(), depth + 1);
    }

    #[test]
    fn the_ring_is_bounded() {
        let mut s = Session::with_sequential_ids();
        for _ in 0..(RING + 30) {
            let _ = s.handle("document.summary", json!({}));
            s.feedback.log("line");
        }
        assert_eq!(s.feedback.recent_calls().len(), RING);
        assert_eq!(s.feedback.log.len(), LOG_RING);
        // Feedback and describe calls are not recorded.
        let n = s.feedback.recent_calls().len();
        let _ = s.handle("describe", json!({}));
        let _ = s.handle("feedback.list", json!({}));
        assert_eq!(s.feedback.recent_calls().len(), n);
    }

    #[test]
    fn describe_has_a_feedback_section() {
        let d = describe();
        assert!(d["purpose"].as_str().unwrap().contains("feature request"));
        assert_eq!(d["methods"].as_array().unwrap().len(), 4);
        assert!(d["examples"].as_array().unwrap().len() >= 4);
        let mut s = Session::with_sequential_ids();
        let all = s.handle("describe", json!({})).unwrap();
        assert!(
            all["feedback"]["consent"]
                .as_str()
                .unwrap()
                .contains("human")
        );
    }

    #[test]
    fn the_store_can_be_replaced() {
        let mut s = Session::with_sequential_ids();
        s.feedback.hub.set_storage(Box::new(MemStorage::new()));
        submit(&mut s, json!({"kind": "question", "title": "x"}));
        assert_eq!(s.feedback.hub.list().len(), 1);
    }
}
