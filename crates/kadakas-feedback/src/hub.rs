//! The outbox: reports, attachments and settings in a [`Storage`].
//!
//! Every call reads the outbox from the store and writes it back, so two programs that share a
//! folder (the app and a headless run) see each other's reports. The file is small: the
//! attachments are files of their own.
//!
//! Programs that share the folder lock it around each load-change-save (`Storage::lock`), and a
//! program claims a report on disk before it sends it, so a report is not sent twice.
//!
//! Consent has two parts, and the code keeps them apart:
//!
//! - Text-only reports go to the server when sending is on (`Settings::send`). The override that
//!   `--feedback-send` sets covers the reports of the run only, and never beats a stored "no".
//! - A report with attachments has the status `NeedsReview`. Only [`Hub::approve`] moves it on.
//!   The app review dialog and `kadakas feedback review` call it after a person has seen the
//!   exact payload. No API method calls it, and [`Hub::outgoing`] never returns such a report.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::error::FeedbackError;
use crate::model::*;
use crate::payload::{self, PayloadParts};
use crate::scrub;
use crate::send::{DEFAULT_URL, Delivery};
use crate::store::Storage;

const OUTBOX: &str = "outbox.json";
const SETTINGS: &str = "settings.json";
const VERSION: u32 = 1;
/// The bytes of attachments that wait in the outbox (not yet sent), for agents and for people each.
pub const MAX_WAITING_BYTES: usize = 32 << 20;
/// Reports one session may make, merged ones included.
pub const MAX_SUBMITS_PER_SESSION: u32 = 30;

#[derive(Serialize, Deserialize)]
struct Outbox {
    version: u32,
    next_seq: u64,
    reports: Vec<Report>,
}

impl Default for Outbox {
    fn default() -> Self {
        Outbox {
            version: VERSION,
            next_seq: 1,
            reports: Vec::new(),
        }
    }
}

/// The preferences of the person.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// `None`: the person was not asked yet.
    #[serde(default)]
    pub send: Option<bool>,
    /// Another feedback server.
    #[serde(default)]
    pub url: Option<String>,
    /// A random id of this installation, made at the first need. It is not the id of a person.
    #[serde(default)]
    pub installation: Option<String>,
    /// The person does not want the installation id in reports (PLAT-20). Stored as a "no" so
    /// that the default (a file without the key) is "include".
    #[serde(default)]
    pub omit_installation: bool,
    /// What happens to a crash report (PLAT-20).
    #[serde(default)]
    pub crash_policy: CrashPolicy,
}

/// Preferences > Privacy > Crash reports (PLAT-20).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashPolicy {
    /// Show the report and ask (the default).
    #[default]
    Ask,
    /// Send it at once, without a question.
    Always,
    /// Delete it, send nothing.
    Never,
}

impl CrashPolicy {
    pub const ALL: [CrashPolicy; 3] = [CrashPolicy::Ask, CrashPolicy::Always, CrashPolicy::Never];

    pub fn id(self) -> &'static str {
        match self {
            CrashPolicy::Ask => "ask",
            CrashPolicy::Always => "always",
            CrashPolicy::Never => "never",
        }
    }
}

/// A report that is ready to go out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    pub id: String,
    pub payload: String,
}

/// The answer of [`Hub::submit`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submitted {
    pub id: String,
    /// True if an equal report existed: only its counter went up.
    pub merged: bool,
    pub count: u32,
    pub status: Status,
}

pub struct Hub {
    storage: Box<dyn Storage>,
    send_override: Option<bool>,
    url_override: Option<String>,
    /// Reports made by agents in this run, merged ones included (the rate limit).
    submitted: u32,
    max_submits: u32,
    /// The ids of reports that this run made. `--feedback-send` covers these only. A report that
    /// this run only merged into is not in the list: its text is older.
    run_ids: HashSet<String>,
    /// A note for the person, for example that the outbox is in memory only.
    note: Option<String>,
}

impl Hub {
    pub fn new(storage: Box<dyn Storage>) -> Hub {
        Hub {
            storage,
            send_override: None,
            url_override: None,
            submitted: 0,
            max_submits: MAX_SUBMITS_PER_SESSION,
            run_ids: HashSet::new(),
            note: None,
        }
    }

    /// A hub that keeps everything in memory.
    pub fn in_memory() -> Hub {
        Hub::new(Box::new(crate::store::MemStorage::new()))
    }

    pub fn set_storage(&mut self, storage: Box<dyn Storage>) {
        self.storage = storage;
    }

    pub fn persistent(&self) -> bool {
        self.storage.persistent()
    }

    /// A note for the person, if the outbox needs one (see `keep_aside`).
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// The random id of this installation. It is made once and stored in `settings.json`. It
    /// tells the server that two reports come from one installation, so that it can drop a
    /// duplicate. It does not name a person.
    pub fn installation_id(&mut self) -> String {
        if let Some(i) = self.settings().installation {
            return i;
        }
        let Ok(_guard) = self.lock() else {
            return new_installation_id();
        };
        let mut s = self.settings();
        // Another program can have made it while we waited for the lock.
        if let Some(i) = s.installation.clone() {
            return i;
        }
        let id = new_installation_id();
        s.installation = Some(id.clone());
        let _ = self.save_settings(&s);
        id
    }

    /// True if reports carry the installation id (the default).
    pub fn include_installation(&self) -> bool {
        !self.settings().omit_installation
    }

    /// The installation id for a report: `None` when the person turned the id off.
    pub fn report_installation_id(&mut self) -> Option<String> {
        if self.include_installation() {
            Some(self.installation_id())
        } else {
            None
        }
    }

    /// The switch "Include the installation ID in reports".
    pub fn set_include_installation(&mut self, on: bool) -> Result<(), FeedbackError> {
        let _guard = self.lock()?;
        let mut s = self.settings();
        s.omit_installation = !on;
        self.save_settings(&s)
    }

    /// A new random id. Old reports can no longer be linked to the new ones.
    pub fn reset_installation_id(&mut self) -> Result<String, FeedbackError> {
        let _guard = self.lock()?;
        let mut s = self.settings();
        let id = new_installation_id();
        s.installation = Some(id.clone());
        self.save_settings(&s)?;
        Ok(id)
    }

    pub fn crash_policy(&self) -> CrashPolicy {
        self.settings().crash_policy
    }

    pub fn set_crash_policy(&mut self, p: CrashPolicy) -> Result<(), FeedbackError> {
        let _guard = self.lock()?;
        let mut s = self.settings();
        s.crash_policy = p;
        self.save_settings(&s)
    }

    pub fn set_max_submits(&mut self, n: u32) {
        self.max_submits = n;
    }

    // ---- settings

    pub fn settings(&self) -> Settings {
        self.storage
            .read(SETTINGS)
            .filter(|b| b.len() < 64 << 10)
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save_settings(&mut self, s: &Settings) -> Result<(), FeedbackError> {
        let b = serde_json::to_vec(s).map_err(|e| FeedbackError::Store(e.to_string()))?;
        self.storage
            .write(SETTINGS, &b)
            .map_err(FeedbackError::Store)
    }

    fn lock(&self) -> Result<Box<dyn Send>, FeedbackError> {
        self.storage.lock().map_err(FeedbackError::Store)
    }

    /// Stores the answer of the person to "may Kadakas send my reports?".
    pub fn set_send(&mut self, on: Option<bool>) -> Result<(), FeedbackError> {
        let mut s = self.settings();
        s.send = on;
        self.save_settings(&s)
    }

    /// Stores another server address (`None`: the default). The address is checked when it is
    /// used. A new address gives every unsent report a fresh start: the tries and the back-off
    /// belonged to the old address.
    pub fn set_url(&mut self, url: Option<String>) -> Result<(), FeedbackError> {
        let mut s = self.settings();
        let new = url
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty() && u.len() <= 300);
        let changed = s.url != new;
        s.url = new;
        self.save_settings(&s)?;
        if changed {
            self.reset_tries(None)?;
        }
        Ok(())
    }

    /// A choice for this run only (`--feedback-send`). It is not stored. It never overrides a
    /// stored "no", and it covers the reports of this run only.
    pub fn set_send_override(&mut self, on: Option<bool>) {
        self.send_override = on;
    }

    pub fn set_url_override(&mut self, url: Option<String>) {
        self.url_override = url.filter(|u| !u.trim().is_empty());
    }

    /// True if the person said no. Then nothing goes out, whatever a flag says.
    pub fn sending_off(&self) -> bool {
        self.settings().send == Some(false)
    }

    /// True if text-only reports may go to the server (for the reports of this run, if only a
    /// flag says so).
    pub fn send_enabled(&self) -> bool {
        match self.settings().send {
            Some(on) => on,
            None => self.send_override == Some(true),
        }
    }

    /// True if the person was asked (or a flag answered for them).
    pub fn consent_asked(&self) -> bool {
        self.send_override.is_some() || self.settings().send.is_some()
    }

    /// The server address: this run's, else the stored one, else the default.
    pub fn url(&self) -> String {
        self.url_override
            .clone()
            .or(self.settings().url)
            .unwrap_or_else(|| DEFAULT_URL.to_string())
    }

    // ---- the outbox file

    /// The outbox file is from a newer Kadakas. It is not read and never written: the reports of
    /// this run go to memory, and the person is told.
    fn keep_aside(&mut self) {
        self.storage = Box::new(crate::store::MemStorage::new());
        self.note = Some(
            "The outbox file is from a newer Kadakas. It was left as it is, and reports are kept in memory until the program ends."
                .into(),
        );
    }

    fn load(&mut self) -> Outbox {
        let Some(bytes) = self.storage.read(OUTBOX) else {
            return Outbox::default();
        };
        let newer = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|v| v["version"].as_u64())
            .is_some_and(|n| n > u64::from(VERSION));
        if newer {
            self.keep_aside();
            return Outbox::default();
        }
        let parsed = (bytes.len() <= MAX_OUTBOX_BYTES)
            .then(|| serde_json::from_slice::<Outbox>(&bytes).ok())
            .flatten()
            .filter(|o| o.version == VERSION);
        match parsed {
            Some(o) => o,
            None => {
                // Keep the broken file for a person to look at, and start again.
                let _ = self.storage.write("outbox.bad.json", &bytes);
                Outbox::default()
            }
        }
    }

    fn save(&mut self, o: &Outbox) -> Result<(), FeedbackError> {
        let b = serde_json::to_vec(o).map_err(|e| FeedbackError::Store(e.to_string()))?;
        if b.len() > MAX_OUTBOX_BYTES {
            return Err(FeedbackError::Limit("the outbox file is full".into()));
        }
        self.storage.write(OUTBOX, &b).map_err(FeedbackError::Store)
    }

    fn drop_files(&mut self, r: &Report) {
        for a in &r.attachments {
            self.storage.remove(&a.kind.file(&r.id));
        }
        self.storage.remove(&payload_file(&r.id));
    }

    // ---- reads

    /// All reports, oldest first.
    pub fn list(&mut self) -> Vec<Report> {
        self.load().reports
    }

    pub fn get(&mut self, id: &str) -> Option<Report> {
        self.load().reports.into_iter().find(|r| r.id == id)
    }

    /// Reports that wait for a human.
    pub fn needs_review(&mut self) -> usize {
        self.load()
            .reports
            .iter()
            .filter(|r| r.status == Status::NeedsReview)
            .count()
    }

    /// The bytes of a stored attachment.
    pub fn attachment(&self, id: &str, kind: AttachmentKind) -> Option<Vec<u8>> {
        self.storage.read(&kind.file(id))
    }

    /// The stored message of an approved report.
    pub fn approved_payload(&self, id: &str) -> Option<String> {
        self.storage
            .read(&payload_file(id))
            .and_then(|b| String::from_utf8(b).ok())
    }

    /// True if a report with these fields would merge into one that exists (`submit` would only
    /// raise its counter). A caller checks this before it makes attachments that a merge drops.
    pub fn would_merge(&mut self, kind: Kind, title: &str, method: &str, origin: Origin) -> bool {
        let title = scrub::text(title.trim(), MAX_TITLE);
        let method = scrub::text(method.trim(), MAX_METHOD);
        let key = dedupe_key(kind, &title, &method);
        !title.is_empty() && self.load().reports.iter().any(|r| merges(r, &key, origin))
    }

    // ---- writes

    /// Adds a report, or raises the counter of an equal one. The texts are scrubbed and cut
    /// here, whatever the caller did. Attachments are checked against their limits; a merged
    /// report drops the new attachments. A report that was sent is not merged into: the new one
    /// is a report of its own. Reports of agents merge with reports of agents only.
    pub fn submit(
        &mut self,
        draft: Draft,
        attachments: Vec<AttachmentData>,
        now: u64,
    ) -> Result<Submitted, FeedbackError> {
        // The limit is for agents. A person who writes many reports is not a flood.
        let agent = draft.origin == Origin::Agent;
        if agent && self.submitted >= self.max_submits {
            return Err(FeedbackError::Limit(format!(
                "this session already made {} reports; wait for the next session or ask the person to use Help > Feedback",
                self.max_submits
            )));
        }
        let title = scrub::text(draft.title.trim(), MAX_TITLE);
        if title.is_empty() {
            return Err(FeedbackError::Invalid("the title is empty".into()));
        }
        let att = check_attachments(attachments)?;
        let method = scrub::text(draft.method.trim(), MAX_METHOD);
        let key = dedupe_key(draft.kind, &title, &method);
        let _guard = self.lock()?;
        let mut ob = self.load();
        if let Some(r) = ob
            .reports
            .iter_mut()
            .find(|r| merges(r, &key, draft.origin))
        {
            r.count = r.count.saturating_add(1);
            let out = Submitted {
                id: r.id.clone(),
                merged: true,
                count: r.count,
                status: r.status,
            };
            self.save(&ob)?;
            self.submitted += u32::from(agent);
            return Ok(out);
        }
        // Agents and people have their own room, so an agent cannot fill the space of a person.
        let side = |r: &Report| (r.origin == Origin::Agent) == agent;
        if ob.reports.iter().filter(|r| side(r)).count() >= MAX_REPORTS_PER_SIDE {
            let old = ob
                .reports
                .iter()
                .position(|r| side(r) && matches!(r.status, Status::Sent | Status::Withdrawn));
            match old {
                Some(i) => {
                    let r = ob.reports.remove(i);
                    self.drop_files(&r);
                }
                None => {
                    return Err(FeedbackError::Limit(format!(
                        "the outbox holds {MAX_REPORTS_PER_SIDE} reports of this kind that wait; a person must send or withdraw some"
                    )));
                }
            }
        }
        let waiting = ob
            .reports
            .iter()
            .filter(|r| side(r) && !matches!(r.status, Status::Sent | Status::Withdrawn))
            .flat_map(|r| &r.attachments)
            .fold(0usize, |n, a| n.saturating_add(a.bytes));
        let new_bytes = att
            .iter()
            .fold(0usize, |n, a| n.saturating_add(a.bytes.len()));
        if waiting.saturating_add(new_bytes) > MAX_WAITING_BYTES {
            return Err(FeedbackError::Limit(
                "too many attachments wait in the outbox; a person must review or withdraw some"
                    .into(),
            ));
        }
        let id = format!("fb-{}", ob.next_seq);
        ob.next_seq += 1;
        let ctx = draft.context;
        let report = Report {
            id: id.clone(),
            nonce: new_nonce(),
            kind: draft.kind,
            title,
            body: scrub::text(&draft.body, MAX_BODY),
            wanted: scrub::text(&draft.wanted, MAX_WANTED),
            tried: scrub::text(&draft.tried, MAX_TRIED),
            method,
            error: scrub::text(&draft.error, MAX_ERROR),
            severity: draft.severity,
            origin: draft.origin,
            context: Context {
                version: scrub::text(&ctx.version, MAX_CLIENT),
                os: scrub::text(&ctx.os, MAX_CLIENT),
                client_name: scrub::text(&ctx.client_name, MAX_CLIENT),
                client_version: scrub::text(&ctx.client_version, MAX_CLIENT),
            },
            count: 1,
            status: if att.is_empty() {
                Status::Pending
            } else {
                Status::NeedsReview
            },
            key,
            created: now,
            attachments: att
                .iter()
                .map(|a| AttachmentMeta {
                    kind: a.kind,
                    bytes: a.bytes.len(),
                    objects: a.objects,
                })
                .collect(),
            privacy: Privacy::default(),
            server_id: None,
            server_status: None,
            attempts: 0,
            retries: 0,
            next_try: 0,
            claimed_until: 0,
            last_error: None,
        };
        for a in &att {
            if let Err(e) = self.storage.write(&a.kind.file(&id), &a.bytes) {
                self.drop_files(&report);
                return Err(FeedbackError::Store(e));
            }
        }
        let out = Submitted {
            id: id.clone(),
            merged: false,
            count: 1,
            status: report.status,
        };
        ob.reports.push(report.clone());
        if let Err(e) = self.save(&ob) {
            self.drop_files(&report);
            return Err(e);
        }
        self.submitted += u32::from(agent);
        self.run_ids.insert(id);
        Ok(out)
    }

    /// Takes a report back. A report that went to the server cannot be taken back here.
    pub fn withdraw(&mut self, id: &str) -> Result<Report, FeedbackError> {
        let _guard = self.lock()?;
        let mut ob = self.load();
        let r =
            ob.reports.iter_mut().find(|r| r.id == id).ok_or_else(|| {
                FeedbackError::NotFound(format!("no report {}", scrub::text(id, 40)))
            })?;
        match r.status {
            Status::Sent => {
                return Err(FeedbackError::State(
                    "the report was sent already; it cannot be withdrawn here".into(),
                ));
            }
            Status::Withdrawn => return Ok(r.clone()),
            _ => {}
        }
        let old = r.clone();
        r.status = Status::Withdrawn;
        r.claimed_until = 0;
        let out = r.clone();
        self.save(&ob)?;
        self.drop_files(&old);
        Ok(out)
    }

    /// The approval path. A person has seen `payload` (built from the attachments they kept, with
    /// the privacy options applied) and agreed. The payload is stored as it is, and it is exactly
    /// what [`Hub::outgoing`] gives to the sender.
    ///
    /// Nothing in the API calls this. It fails unless the report waits for review.
    pub fn approve(
        &mut self,
        id: &str,
        payload: &str,
        privacy: Privacy,
    ) -> Result<(), FeedbackError> {
        let _guard = self.lock()?;
        let mut ob = self.load();
        let r =
            ob.reports.iter_mut().find(|r| r.id == id).ok_or_else(|| {
                FeedbackError::NotFound(format!("no report {}", scrub::text(id, 40)))
            })?;
        payload::check(payload, &r.client_id())?;
        if r.status != Status::NeedsReview {
            return Err(FeedbackError::State(
                "the report does not wait for review".into(),
            ));
        }
        self.storage
            .write(&payload_file(id), payload.as_bytes())
            .map_err(FeedbackError::Store)?;
        r.status = Status::Approved;
        r.privacy = privacy;
        r.attempts = 0;
        r.retries = 0;
        r.next_try = 0;
        r.last_error = None;
        self.save(&ob)
    }

    /// Gives one report (or every unsent report, with `None`) a fresh start: no tries counted,
    /// no waiting. This is the "Try again" of a person.
    pub fn reset_tries(&mut self, id: Option<&str>) -> Result<(), FeedbackError> {
        let _guard = self.lock()?;
        let mut ob = self.load();
        let mut found = id.is_none();
        for r in ob.reports.iter_mut() {
            if id.is_some_and(|i| i != r.id) || matches!(r.status, Status::Sent | Status::Withdrawn)
            {
                continue;
            }
            found = true;
            r.attempts = 0;
            r.retries = 0;
            r.next_try = 0;
            r.claimed_until = 0;
            r.last_error = None;
        }
        if !found {
            return Err(FeedbackError::NotFound(format!(
                "no report {}",
                scrub::text(id.unwrap_or(""), 40)
            )));
        }
        self.save(&ob)
    }

    /// Makes a report that failed too often ready for new tries.
    pub fn retry(&mut self, id: &str) -> Result<(), FeedbackError> {
        self.reset_tries(Some(id))
    }

    /// The reports that may go out at time `now` (seconds since 1970), claimed on disk so that no
    /// other program sends them too. Approved reports go; text-only reports go if `include_text`
    /// and the consent allows (see below). A report waits while its back-off runs, and after
    /// `MAX_ATTEMPTS` counted failures. At most `max`.
    ///
    /// Consent: if the person said no, nothing goes out, whatever a flag says. If the person said
    /// yes, text-only reports go. If the person was not asked, `--feedback-send` covers the text
    /// reports of this run only.
    pub fn outgoing(&mut self, include_text: bool, max: usize, now: u64) -> Vec<Outgoing> {
        let stored = self.settings().send;
        if stored == Some(false) {
            return Vec::new();
        }
        // The id is made before the lock: the lock is not re-entrant.
        let installation = self.report_installation_id();
        let Ok(_guard) = self.lock() else {
            return Vec::new();
        };
        let mut ob = self.load();
        let mut out = Vec::new();
        let mut dirty = false;
        for i in 0..ob.reports.len() {
            if out.len() >= max {
                break;
            }
            let r = &ob.reports[i];
            if r.attempts >= MAX_ATTEMPTS || r.next_try > now || r.claimed_until > now {
                continue;
            }
            let text_ok = include_text
                && (stored == Some(true)
                    || (self.send_override == Some(true) && self.run_ids.contains(&r.id)));
            let payload = match r.status {
                Status::Approved => match self.approved_payload(&r.id) {
                    Some(p) => Some(p),
                    None => {
                        // The message is gone. The person must look at the report again.
                        let r = &mut ob.reports[i];
                        r.status = Status::NeedsReview;
                        r.last_error = Some(
                            "The approved message is missing. Review the report again.".into(),
                        );
                        dirty = true;
                        None
                    }
                },
                Status::Pending if text_ok && r.attachments.is_empty() => payload::build(
                    r,
                    &PayloadParts {
                        installation: installation.as_deref(),
                        ..Default::default()
                    },
                )
                .ok(),
                _ => None,
            };
            if let Some(payload) = payload {
                let r = &mut ob.reports[i];
                r.claimed_until = now.saturating_add(CLAIM_SECS);
                dirty = true;
                out.push(Outgoing {
                    id: r.id.clone(),
                    payload,
                });
            }
        }
        // If the claim cannot be written, nothing is sent: another program could send it too.
        if dirty && self.save(&ob).is_err() {
            return Vec::new();
        }
        out
    }

    /// Claims one approved report for sending now (for a person who sends it from a review). It
    /// fails if the person said no to sending, if another program has the report, or if the
    /// report is not approved.
    pub fn claim(&mut self, id: &str, now: u64) -> Option<Outgoing> {
        if self.sending_off() {
            return None;
        }
        let _guard = self.lock().ok()?;
        let mut ob = self.load();
        let r = ob.reports.iter_mut().find(|r| r.id == id)?;
        if r.status != Status::Approved || r.claimed_until > now {
            return None;
        }
        let payload = self.approved_payload(id)?;
        let r = ob.reports.iter_mut().find(|r| r.id == id)?;
        r.claimed_until = now.saturating_add(CLAIM_SECS);
        self.save(&ob).ok()?;
        Some(Outgoing {
            id: id.to_string(),
            payload,
        })
    }

    /// Writes down what happened to a report that was sent, at time `now`.
    pub fn record(&mut self, id: &str, delivery: &Delivery, now: u64) -> Result<(), FeedbackError> {
        let _guard = self.lock()?;
        let mut ob = self.load();
        let Some(r) = ob.reports.iter_mut().find(|r| r.id == id) else {
            return Ok(());
        };
        if matches!(r.status, Status::Sent | Status::Withdrawn) {
            return Ok(());
        }
        let old = r.clone();
        r.claimed_until = 0;
        match delivery {
            Delivery::Sent(reply) => {
                r.status = Status::Sent;
                r.server_id = Some(scrub::text(&reply.id, 100));
                r.server_status = Some(scrub::text(&reply.status, 40));
                r.last_error = None;
                r.next_try = 0;
            }
            Delivery::Failed {
                message,
                permanent,
                unreachable,
                retry_after,
                ..
            } => {
                r.retries = r.retries.saturating_add(1);
                if *permanent {
                    r.attempts = MAX_ATTEMPTS;
                } else if !*unreachable {
                    r.attempts = r.attempts.saturating_add(1);
                }
                let wait = backoff_secs(r.retries, &r.id).max(retry_after.unwrap_or(0));
                r.next_try = now.saturating_add(wait);
                r.last_error = Some(scrub::text(message, 200));
            }
        }
        let sent = r.status == Status::Sent;
        self.save(&ob)?;
        if sent {
            // The server has them now. Keep the room.
            self.drop_files(&old);
        }
        Ok(())
    }
}

/// A random id for an installation, in the form of a version 4 UUID. It needs to be unlikely to
/// repeat, not secret: it is a key against duplicates. The two hashers of `RandomState` start
/// with random keys from the operating system.
fn new_installation_id() -> String {
    use std::hash::BuildHasher;
    let a = std::collections::hash_map::RandomState::new().hash_one(0x004b_6164_616b_6173_u64);
    let b = std::collections::hash_map::RandomState::new().hash_one(0x6665_6564_6261_636bu64);
    let hi = (a & !0xf000) | 0x4000;
    let lo = (b & 0x3fff_ffff_ffff_ffff) | 0x8000_0000_0000_0000;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        hi >> 32,
        (hi >> 16) & 0xffff,
        hi & 0xffff,
        lo >> 48,
        lo & 0xffff_ffff_ffff
    )
}

/// 64 random bits as 16 hex characters, for the `client_id` of a report. It is made the same way
/// as the installation id.
fn new_nonce() -> String {
    use std::hash::BuildHasher;
    let n = std::collections::hash_map::RandomState::new().hash_one(0x6e6f_6e63_6531_u64);
    format!("{n:016x}")
}

/// True if a new report with this key and origin would merge into `r`.
fn merges(r: &Report, key: &str, origin: Origin) -> bool {
    r.key == key
        && !matches!(r.status, Status::Withdrawn | Status::Sent)
        && (r.origin == Origin::Agent) == (origin == Origin::Agent)
}

/// The wait after the `retries`-th failed start: one minute, doubling, at most six hours, with a
/// jitter of plus or minus 20 percent that depends on the report and the count (so that many
/// installations do not come back at the same second, and the value is the same in a test).
pub fn backoff_secs(retries: u32, id: &str) -> u64 {
    let n = retries.saturating_sub(1).min(20);
    let base = BACKOFF_BASE_SECS
        .saturating_mul(1u64 << n)
        .min(BACKOFF_MAX_SECS);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.bytes().chain(retries.to_le_bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    base * (80 + h % 41) / 100
}

fn payload_file(id: &str) -> String {
    format!("{id}.payload.json")
}

/// Checks the attachments: one of each kind, within the limits, of the right format.
fn check_attachments(list: Vec<AttachmentData>) -> Result<Vec<AttachmentData>, FeedbackError> {
    let mut seen = Vec::new();
    for a in &list {
        if seen.contains(&a.kind) {
            return Err(FeedbackError::Invalid(format!(
                "two attachments of kind {}",
                a.kind.id()
            )));
        }
        seen.push(a.kind);
        if a.bytes.len() > a.kind.limit() {
            return Err(FeedbackError::Invalid(format!(
                "the {} is larger than {} bytes",
                a.kind.id(),
                a.kind.limit()
            )));
        }
        let ok = match a.kind {
            AttachmentKind::Screenshot | AttachmentKind::Render => {
                a.bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            }
            AttachmentKind::Excerpt | AttachmentKind::Calls => {
                serde_json::from_slice::<serde_json::Value>(&a.bytes).is_ok()
            }
            AttachmentKind::Log => std::str::from_utf8(&a.bytes).is_ok(),
        };
        if !ok {
            return Err(FeedbackError::Invalid(format!(
                "the {} has the wrong format",
                a.kind.id()
            )));
        }
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::send::Reply;
    use proptest::prelude::*;

    /// A fixed time for the tests (seconds since 1970).
    const T0: u64 = 1_000_000;

    fn draft(title: &str) -> Draft {
        let mut d = Draft::new(Kind::Bug, title, "body", Origin::Agent);
        d.method = "edit.fillet".into();
        d
    }

    fn png() -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
        v.extend([0u8; 16]);
        v
    }

    fn att(kind: AttachmentKind, bytes: Vec<u8>) -> AttachmentData {
        AttachmentData {
            kind,
            bytes,
            objects: None,
        }
    }

    fn sent(id: &str) -> Delivery {
        Delivery::Sent(Reply {
            id: id.into(),
            status: "received".into(),
        })
    }

    fn failed(message: &str) -> Delivery {
        Delivery::Failed {
            message: message.into(),
            permanent: false,
            unreachable: false,
            retry_after: None,
            attempts: 4,
        }
    }

    fn offline() -> Delivery {
        Delivery::Failed {
            message: "dns".into(),
            permanent: false,
            unreachable: true,
            retry_after: None,
            attempts: 1,
        }
    }

    /// Two hubs on one folder, like the app and a headless run.
    fn two_hubs() -> (kadakas_sys::tmp::PrivateTempDir, Hub, Hub) {
        let tmp = kadakas_sys::tmp::PrivateTempDir::new("kadakas-fb-hubs").unwrap();
        let dir = tmp.path().join("fb");
        let a = Hub::new(Box::new(crate::store::DirStorage::new(dir.clone())));
        let b = Hub::new(Box::new(crate::store::DirStorage::new(dir)));
        (tmp, a, b)
    }

    #[test]
    fn a_text_report_is_pending_and_scrubbed() {
        let mut h = Hub::in_memory();
        let mut d = draft("Fails for ann@example.com");
        d.body = "see /Users/ann/x/garden.kad token=abc123".into();
        d.severity = Some(Severity::Low);
        let s = h.submit(d, vec![], 7).unwrap();
        assert_eq!(
            s,
            Submitted {
                id: "fb-1".into(),
                merged: false,
                count: 1,
                status: Status::Pending
            }
        );
        let r = h.get("fb-1").unwrap();
        assert_eq!(r.title, "Fails for <email>");
        assert_eq!(r.body, "see <path>/garden.kad token=<secret>");
        assert_eq!(r.created, 7);
        assert_eq!(h.list().len(), 1);
        assert!(h.get("fb-9").is_none());
    }

    #[test]
    fn fields_are_cut() {
        let mut h = Hub::in_memory();
        let mut d = draft(&"t".repeat(500));
        d.body = "b ".repeat(5000);
        d.error = "e ".repeat(5000);
        d.context.client_name = "c".repeat(500);
        h.submit(d, vec![], 0).unwrap();
        let r = h.get("fb-1").unwrap();
        assert!(r.title.chars().count() <= MAX_TITLE + 1);
        assert!(r.body.chars().count() <= MAX_BODY + 1);
        assert!(r.error.chars().count() <= MAX_ERROR + 1);
        assert!(r.context.client_name.chars().count() <= MAX_CLIENT + 1);
    }

    #[test]
    fn an_empty_title_is_refused() {
        let mut h = Hub::in_memory();
        assert!(matches!(
            h.submit(draft("  "), vec![], 0),
            Err(FeedbackError::Invalid(_))
        ));
        assert!(h.list().is_empty());
        assert!(!h.would_merge(Kind::Bug, " ", "", Origin::Agent));
    }

    #[test]
    fn equal_reports_merge_into_a_counter() {
        let mut h = Hub::in_memory();
        h.submit(draft("Fillet fails"), vec![], 0).unwrap();
        assert!(h.would_merge(Kind::Bug, "fillet FAILS", " edit.fillet", Origin::Agent));
        let s = h
            .submit(
                draft("fillet  FAILS!"),
                vec![att(AttachmentKind::Log, b"x".to_vec())],
                0,
            )
            .unwrap();
        assert_eq!((s.merged, s.count, s.id.as_str()), (true, 2, "fb-1"));
        let r = h.get("fb-1").unwrap();
        assert_eq!(r.count, 2);
        // The new attachment is dropped; the status is the old one.
        assert!(r.attachments.is_empty() && r.status == Status::Pending);
        // Another method is another report.
        let mut d = draft("Fillet fails");
        d.method = "edit.chamfer".into();
        assert!(!h.submit(d, vec![], 0).unwrap().merged);
        // A withdrawn report does not absorb a new one.
        h.withdraw("fb-1").unwrap();
        assert!(!h.would_merge(Kind::Bug, "Fillet fails", "edit.fillet", Origin::Agent));
        assert!(!h.submit(draft("Fillet fails"), vec![], 0).unwrap().merged);
        assert_eq!(h.list().len(), 3);
    }

    #[test]
    fn a_report_that_was_sent_is_not_merged_into() {
        let mut h = Hub::in_memory();
        h.submit(draft("Sent one"), vec![], 0).unwrap();
        h.record("fb-1", &sent("s1"), T0).unwrap();
        assert!(!h.would_merge(Kind::Bug, "Sent one", "edit.fillet", Origin::Agent));
        let s = h.submit(draft("Sent one"), vec![], 0).unwrap();
        assert_eq!((s.merged, s.id.as_str()), (false, "fb-2"));
    }

    #[test]
    fn agents_and_people_do_not_merge_with_each_other() {
        let mut h = Hub::in_memory();
        h.submit(draft("Same"), vec![], 0).unwrap();
        let mut d = draft("Same");
        d.origin = Origin::User;
        assert!(!h.would_merge(Kind::Bug, "Same", "edit.fillet", Origin::User));
        let s = h.submit(d, vec![], 0).unwrap();
        assert_eq!((s.merged, s.id.as_str()), (false, "fb-2"));
        // A second report of the person merges with the first of the person.
        let mut d = draft("Same");
        d.origin = Origin::Crash;
        assert!(h.submit(d, vec![], 0).unwrap().merged);
        assert_eq!(h.get("fb-1").unwrap().count, 1);
    }

    #[test]
    fn the_session_rate_limit_is_for_agents_only() {
        let mut h = Hub::in_memory();
        h.set_max_submits(2);
        h.submit(draft("a"), vec![], 0).unwrap();
        h.submit(draft("a"), vec![], 0).unwrap(); // merged, but it counts
        assert!(matches!(
            h.submit(draft("b"), vec![], 0),
            Err(FeedbackError::Limit(_))
        ));
        // A person is not limited.
        for i in 0..5 {
            let mut d = draft(&format!("person {i}"));
            d.origin = Origin::User;
            h.submit(d, vec![], 0).unwrap();
        }
        assert_eq!(h.list().len(), 6);
    }

    #[test]
    fn the_outbox_is_capped() {
        let mut h = Hub::in_memory();
        h.set_max_submits(1000);
        for i in 0..MAX_REPORTS_PER_SIDE {
            h.submit(draft(&format!("report {i}")), vec![], 0).unwrap();
        }
        let e = h.submit(draft("one more"), vec![], 0).unwrap_err();
        assert!(matches!(e, FeedbackError::Limit(_)), "{e}");
        // A sent report makes room.
        h.record("fb-1", &sent("s1"), T0).unwrap();
        h.submit(draft("one more"), vec![], 0).unwrap();
        assert_eq!(h.list().len(), MAX_REPORTS_PER_SIDE);
        assert!(h.get("fb-1").is_none());
    }

    #[test]
    fn attachments_wait_for_review() {
        let mut h = Hub::in_memory();
        let list = vec![
            att(AttachmentKind::Render, png()),
            AttachmentData {
                kind: AttachmentKind::Excerpt,
                bytes: b"{\"layers\":[]}".to_vec(),
                objects: Some(3),
            },
            att(AttachmentKind::Calls, b"[]".to_vec()),
            att(AttachmentKind::Log, b"line".to_vec()),
        ];
        let s = h.submit(draft("with pictures"), list, 0).unwrap();
        assert_eq!(s.status, Status::NeedsReview);
        assert_eq!(h.needs_review(), 1);
        let r = h.get(&s.id).unwrap();
        assert_eq!(r.attachments.len(), 4);
        assert_eq!(
            r.attachment(AttachmentKind::Excerpt).unwrap().objects,
            Some(3)
        );
        assert_eq!(h.attachment(&s.id, AttachmentKind::Log).unwrap(), b"line");
        // Nothing goes out: not with sending on, not as text.
        h.set_send(Some(true)).unwrap();
        h.set_send_override(Some(true));
        assert!(h.outgoing(true, 10, T0).is_empty());
        // The approval path makes it ready, and the payload is the stored one.
        let p = payload::build(&r, &PayloadParts::default()).unwrap();
        h.approve(&s.id, &p, Privacy::default()).unwrap();
        let out = h.outgoing(false, 10, T0);
        assert_eq!(
            out,
            vec![Outgoing {
                id: s.id.clone(),
                payload: p.clone()
            }]
        );
        assert_eq!(h.get(&s.id).unwrap().status, Status::Approved);
        assert_eq!(h.approved_payload(&s.id).as_deref(), Some(p.as_str()));
        // Approval works once.
        assert!(matches!(
            h.approve(&s.id, &out[0].payload, Privacy::default()),
            Err(FeedbackError::State(_))
        ));
    }

    #[test]
    fn approve_checks_the_report_and_the_payload() {
        let mut h = Hub::in_memory();
        let s = h
            .submit(draft("a"), vec![att(AttachmentKind::Log, b"x".to_vec())], 0)
            .unwrap();
        let t = h.submit(draft("b"), vec![], 0).unwrap();
        let r = h.get(&s.id).unwrap();
        let p = payload::build(&r, &PayloadParts::default()).unwrap();
        assert!(matches!(
            h.approve("fb-99", &p, Privacy::default()),
            Err(FeedbackError::NotFound(_))
        ));
        assert!(matches!(
            h.approve(&s.id, "{", Privacy::default()),
            Err(FeedbackError::Invalid(_))
        ));
        // A text report needs no review: it cannot be approved either.
        let pt = payload::build(&h.get(&t.id).unwrap(), &PayloadParts::default()).unwrap();
        assert!(matches!(
            h.approve(&t.id, &pt, Privacy::default()),
            Err(FeedbackError::State(_))
        ));
        let p99 = p.replace("\"fb-1\"", "\"fb-99\"");
        assert!(matches!(
            h.approve("fb-99", &p99, Privacy::default()),
            Err(FeedbackError::NotFound(_))
        ));
    }

    #[test]
    fn client_id_differs_between_fresh_outboxes() {
        let mut a = Hub::in_memory();
        let mut b = Hub::in_memory();
        let ia = a.submit(draft("x"), vec![], 0).unwrap().id;
        let ib = b.submit(draft("x"), vec![], 0).unwrap().id;
        let ra = a.get(&ia).unwrap();
        let rb = b.get(&ib).unwrap();
        assert_eq!(ra.id, "fb-1");
        assert_eq!(rb.id, "fb-1");
        assert_eq!(ra.nonce.len(), 16);
        assert!(ra.nonce.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(ra.client_id(), rb.client_id());
        assert!(ra.client_id().starts_with("fb-1-"));
    }

    #[test]
    fn old_report_without_nonce_builds_and_approves() {
        let mut h = Hub::in_memory();
        let s = h
            .submit(draft("a"), vec![att(AttachmentKind::Log, b"x".to_vec())], 0)
            .unwrap();
        // Make it look like a report from an older outbox.
        let mut ob = h.load();
        ob.reports[0].nonce.clear();
        h.save(&ob).unwrap();
        let r = h.get(&s.id).unwrap();
        assert_eq!(r.client_id(), "fb-1");
        let p = payload::build(&r, &PayloadParts::default()).unwrap();
        assert!(p.contains("\"client_id\":\"fb-1\""));
        h.approve(&s.id, &p, Privacy::default()).unwrap();
    }

    #[test]
    fn bad_attachments_are_refused() {
        let mut h = Hub::in_memory();
        let big = vec![0u8; MAX_IMAGE_BYTES + 1];
        for bad in [
            att(AttachmentKind::Render, b"not a png".to_vec()),
            att(AttachmentKind::Screenshot, big),
            att(AttachmentKind::Excerpt, b"{".to_vec()),
            att(AttachmentKind::Calls, b"nope".to_vec()),
            att(AttachmentKind::Log, vec![0xff, 0xfe]),
        ] {
            assert!(matches!(
                h.submit(draft("x"), vec![bad], 0),
                Err(FeedbackError::Invalid(_))
            ));
        }
        let twice = vec![
            att(AttachmentKind::Log, b"a".to_vec()),
            att(AttachmentKind::Log, b"b".to_vec()),
        ];
        assert!(h.submit(draft("x"), twice, 0).is_err());
        assert!(h.list().is_empty());
    }

    #[test]
    fn waiting_attachments_are_capped() {
        let mut h = Hub::in_memory();
        h.set_max_submits(1000);
        let mut ok = 0;
        for i in 0..80 {
            let mut p = png();
            p.resize(MAX_IMAGE_BYTES, 0);
            match h.submit(
                draft(&format!("big {i}")),
                vec![att(AttachmentKind::Render, p)],
                0,
            ) {
                Ok(_) => ok += 1,
                Err(FeedbackError::Limit(_)) => break,
                Err(e) => panic!("{e}"),
            }
        }
        assert_eq!(ok, MAX_WAITING_BYTES / MAX_IMAGE_BYTES);
    }

    #[test]
    fn withdraw_removes_files() {
        let mut h = Hub::in_memory();
        let s = h
            .submit(draft("a"), vec![att(AttachmentKind::Log, b"x".to_vec())], 0)
            .unwrap();
        let r = h.withdraw(&s.id).unwrap();
        assert_eq!(r.status, Status::Withdrawn);
        assert!(h.attachment(&s.id, AttachmentKind::Log).is_none());
        // Twice is fine; unknown ids and sent reports are errors.
        assert_eq!(h.withdraw(&s.id).unwrap().status, Status::Withdrawn);
        assert!(matches!(
            h.withdraw("fb-77"),
            Err(FeedbackError::NotFound(_))
        ));
        let t = h.submit(draft("b"), vec![], 0).unwrap();
        h.record(&t.id, &sent("s"), T0).unwrap();
        assert!(matches!(h.withdraw(&t.id), Err(FeedbackError::State(_))));
        assert_eq!(h.get(&t.id).unwrap().server_id.as_deref(), Some("s"));
    }

    #[test]
    fn sending_follows_the_consent() {
        let mut h = Hub::in_memory();
        h.submit(draft("a"), vec![], 0).unwrap();
        assert!(!h.consent_asked() && !h.send_enabled());
        assert!(h.outgoing(true, 5, T0).is_empty());
        h.set_send(Some(true)).unwrap();
        assert!(h.consent_asked() && h.send_enabled());
        // The host decides if text reports go out in this program.
        assert!(h.outgoing(false, 5, T0).is_empty());
        assert_eq!(h.outgoing(true, 5, T0).len(), 1);
        h.set_send(None).unwrap();
        assert!(!h.consent_asked());
    }

    #[test]
    fn a_stored_no_beats_every_flag_and_every_approval() {
        let mut h = Hub::in_memory();
        h.submit(draft("text"), vec![], 0).unwrap();
        let s = h
            .submit(
                draft("pictures"),
                vec![att(AttachmentKind::Log, b"x".to_vec())],
                0,
            )
            .unwrap();
        let p = payload::build(&h.get(&s.id).unwrap(), &PayloadParts::default()).unwrap();
        h.approve(&s.id, &p, Privacy::default()).unwrap();
        h.set_send(Some(false)).unwrap();
        h.set_send_override(Some(true));
        assert!(h.sending_off());
        assert!(!h.send_enabled());
        assert!(h.outgoing(true, 10, T0).is_empty());
        assert!(h.outgoing(false, 10, T0).is_empty());
        // The person changes their mind: both go.
        h.set_send(Some(true)).unwrap();
        assert!(!h.sending_off());
        assert_eq!(h.outgoing(true, 10, T0).len(), 2);
    }

    #[test]
    fn a_flag_covers_the_reports_of_its_run_only() {
        let (_tmp, mut old_run, mut this_run) = two_hubs();
        old_run.submit(draft("from before"), vec![], 0).unwrap();
        this_run.set_send_override(Some(true));
        assert!(this_run.send_enabled() && this_run.consent_asked());
        // The old report was not made in this run: it stays.
        assert!(this_run.outgoing(true, 10, T0).is_empty());
        let mut d = draft("from now");
        d.method = "other".into();
        let s = this_run.submit(d, vec![], 0).unwrap();
        let out = this_run.outgoing(true, 10, T0);
        assert_eq!(
            out.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
            vec![s.id.as_str()]
        );
        // The flag is not stored.
        assert_eq!(this_run.settings().send, None);
        // A report that this run only merged into is not a report of this run: its text is older.
        let m = this_run.submit(draft("from before"), vec![], 0).unwrap();
        assert!(m.merged);
        assert!(this_run.outgoing(true, 10, T0 + 1).is_empty());
        assert_eq!(this_run.get(&m.id).unwrap().count, 2);
    }

    #[test]
    fn urls_and_a_new_address_gives_a_fresh_start() {
        let mut h = Hub::in_memory();
        assert_eq!(h.url(), DEFAULT_URL);
        h.set_url(Some(" https://fb.example ".into())).unwrap();
        assert_eq!(h.url(), "https://fb.example");
        h.set_url_override(Some("https://other.example".into()));
        assert_eq!(h.url(), "https://other.example");
        h.set_url_override(Some(" ".into()));
        assert_eq!(h.url(), "https://fb.example");
        // Failures belong to the address that failed.
        h.set_send(Some(true)).unwrap();
        h.submit(draft("a"), vec![], 0).unwrap();
        h.record("fb-1", &failed("down"), T0).unwrap();
        let r = h.get("fb-1").unwrap();
        assert_eq!(
            (r.attempts, r.retries > 0, r.next_try > T0),
            (1, true, true)
        );
        h.set_url(Some("https://fb.example".into())).unwrap(); // same address: no reset
        assert_eq!(h.get("fb-1").unwrap().attempts, 1);
        h.set_url(Some("https://new.example".into())).unwrap();
        let r = h.get("fb-1").unwrap();
        assert_eq!(
            (r.attempts, r.retries, r.next_try, r.last_error),
            (0, 0, 0, None)
        );
        h.set_url(Some("".into())).unwrap();
        assert_eq!(h.url(), DEFAULT_URL);
    }

    #[test]
    fn a_failure_waits_with_a_back_off_and_an_offline_one_is_not_counted() {
        let mut h = Hub::in_memory();
        h.set_send(Some(true)).unwrap();
        h.submit(draft("a"), vec![], 0).unwrap();
        assert_eq!(h.outgoing(true, 5, T0).len(), 1);
        h.record("fb-1", &failed("down at /srv/x/y.log"), T0)
            .unwrap();
        let r = h.get("fb-1").unwrap();
        assert_eq!((r.attempts, r.retries, r.status), (1, 1, Status::Pending));
        assert_eq!(r.last_error.as_deref(), Some("down at <path>/y.log"));
        assert_eq!(r.next_try, T0 + backoff_secs(1, "fb-1"));
        // It waits: not before the time, yes after it.
        assert!(h.outgoing(true, 5, T0 + 10).is_empty());
        assert_eq!(h.outgoing(true, 5, r.next_try).len(), 1);
        // Offline: the wait grows, the lifetime count does not.
        for n in 0..20u64 {
            let at = T0 + 100_000 * (n + 1);
            h.record("fb-1", &offline(), at).unwrap();
        }
        let r = h.get("fb-1").unwrap();
        assert_eq!((r.attempts, r.retries), (1, 21));
        assert!(h.outgoing(true, 5, T0 + 100_000_000).len() == 1);
        // A busy server that says when to come back is obeyed.
        h.reset_tries(None).unwrap();
        let busy = Delivery::Failed {
            message: "429".into(),
            permanent: false,
            unreachable: false,
            retry_after: Some(5000),
            attempts: 1,
        };
        h.record("fb-1", &busy, T0).unwrap();
        assert_eq!(h.get("fb-1").unwrap().next_try, T0 + 5000);
        // Eight counted failures: it waits for a person.
        for _ in 0..8 {
            h.record("fb-1", &failed("no"), T0).unwrap();
        }
        assert!(h.outgoing(true, 5, T0 + 100_000_000).is_empty());
        h.retry("fb-1").unwrap();
        assert_eq!(h.outgoing(true, 5, T0).len(), 1);
        // A refusal is final until a person says so.
        let refused = Delivery::Failed {
            message: "no".into(),
            permanent: true,
            unreachable: false,
            retry_after: None,
            attempts: 1,
        };
        h.record("fb-1", &refused, T0).unwrap();
        assert!(h.outgoing(true, 5, T0 + 100_000_000).is_empty());
        assert!(matches!(h.retry("nope"), Err(FeedbackError::NotFound(_))));
        // Unknown ids, and reports that are done, are ignored.
        h.record("fb-9", &sent("s"), T0).unwrap();
        h.reset_tries(None).unwrap();
        h.record("fb-1", &sent("s"), T0).unwrap();
        h.record("fb-1", &failed("late"), T0).unwrap();
        assert_eq!(h.get("fb-1").unwrap().status, Status::Sent);
    }

    #[test]
    fn the_back_off_grows_to_hours_with_a_jitter() {
        let mut last = 0;
        for n in 1..=10 {
            let w = backoff_secs(n, "fb-1");
            let base = (BACKOFF_BASE_SECS << (n - 1)).min(BACKOFF_MAX_SECS);
            assert!(
                (base * 80 / 100..=base * 120 / 100).contains(&w),
                "{n}: {w}"
            );
            if n <= 5 {
                assert!(w > last, "{n}");
            }
            last = w;
        }
        assert!(backoff_secs(u32::MAX, "x") / 3600 >= 4);
        // Another report gets another wait.
        assert_ne!(backoff_secs(3, "fb-1"), backoff_secs(3, "fb-2"));
    }

    #[test]
    fn an_approved_report_without_its_message_goes_back_to_review() {
        let (tmp, mut h, _other) = two_hubs();
        let s = h
            .submit(draft("a"), vec![att(AttachmentKind::Log, b"x".to_vec())], 0)
            .unwrap();
        let p = payload::build(&h.get(&s.id).unwrap(), &PayloadParts::default()).unwrap();
        h.approve(&s.id, &p, Privacy::default()).unwrap();
        // The message file is lost.
        std::fs::remove_file(tmp.path().join("fb").join(payload_file(&s.id))).unwrap();
        assert!(h.outgoing(true, 5, T0).is_empty());
        let r = h.get(&s.id).unwrap();
        assert_eq!(r.status, Status::NeedsReview);
        assert!(r.last_error.unwrap().contains("missing"));
    }

    #[test]
    fn a_person_claims_one_approved_report() {
        let (_tmp, mut a, mut b) = two_hubs();
        let s = a
            .submit(draft("a"), vec![att(AttachmentKind::Log, b"x".to_vec())], 0)
            .unwrap();
        // Not approved yet.
        assert!(a.claim(&s.id, T0).is_none());
        let p = payload::build(&a.get(&s.id).unwrap(), &PayloadParts::default()).unwrap();
        a.approve(&s.id, &p, Privacy::default()).unwrap();
        let got = a.claim(&s.id, T0).unwrap();
        assert_eq!(got.payload, p);
        // Another program cannot take it, until the claim ends.
        assert!(b.claim(&s.id, T0 + 1).is_none());
        assert!(b.outgoing(false, 5, T0 + 1).is_empty());
        assert!(b.claim(&s.id, T0 + CLAIM_SECS + 1).is_some());
        // A stored no stops it.
        b.set_send(Some(false)).unwrap();
        assert!(a.claim(&s.id, T0 + 10 * CLAIM_SECS).is_none());
        assert!(a.claim("fb-99", T0).is_none());
    }

    #[test]
    fn two_hubs_on_one_folder_do_not_lose_or_double_reports() {
        let (_tmp, mut a, mut b) = two_hubs();
        a.set_send(Some(true)).unwrap();
        let x = a.submit(draft("from a"), vec![], 0).unwrap();
        let mut d = draft("from b");
        d.method = "m2".into();
        let y = b.submit(d, vec![], 0).unwrap();
        assert_eq!((x.id.as_str(), y.id.as_str()), ("fb-1", "fb-2"));
        assert_eq!(a.list().len(), 2);
        // Both ask for work at once: each report goes to one program.
        let ga = a.outgoing(true, 10, T0);
        let gb = b.outgoing(true, 10, T0);
        assert_eq!(ga.len() + gb.len(), 2, "{ga:?} {gb:?}");
        // A claim ends after a while: a crashed program does not hold a report for ever.
        assert_eq!(b.outgoing(true, 10, T0 + CLAIM_SECS + 1).len(), 2);
        // The program that finished records; the report is not sent again.
        a.record("fb-1", &sent("s1"), T0).unwrap();
        assert_eq!(b.get("fb-1").unwrap().status, Status::Sent);
    }

    #[test]
    fn many_threads_make_unique_ids() {
        let tmp = kadakas_sys::tmp::PrivateTempDir::new("kadakas-fb-threads").unwrap();
        let dir = tmp.path().join("fb");
        let handles: Vec<_> = (0..4)
            .map(|t| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    let mut h = Hub::new(Box::new(crate::store::DirStorage::new(dir)));
                    for i in 0..10 {
                        let mut d = draft(&format!("thread {t} report {i}"));
                        d.origin = Origin::User;
                        h.submit(d, vec![], 0).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let mut h = Hub::new(Box::new(crate::store::DirStorage::new(dir)));
        let mut ids: Vec<String> = h.list().into_iter().map(|r| r.id).collect();
        assert_eq!(ids.len(), 40);
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 40);
    }

    #[test]
    fn agents_cannot_fill_the_room_of_people() {
        let mut h = Hub::in_memory();
        h.set_max_submits(10_000);
        for i in 0..MAX_REPORTS_PER_SIDE {
            h.submit(draft(&format!("agent {i}")), vec![], 0).unwrap();
        }
        assert!(matches!(
            h.submit(draft("one more agent"), vec![], 0),
            Err(FeedbackError::Limit(_))
        ));
        // A person still has the whole room of people.
        for i in 0..MAX_REPORTS_PER_SIDE {
            let mut d = draft(&format!("person {i}"));
            d.origin = Origin::User;
            h.submit(d, vec![], 0).unwrap();
        }
        let mut d = draft("one more person");
        d.origin = Origin::User;
        assert!(matches!(
            h.submit(d, vec![], 0),
            Err(FeedbackError::Limit(_))
        ));
        assert_eq!(h.list().len(), MAX_REPORTS);
        // The attachment room is separate as well.
        let mut h = Hub::in_memory();
        h.set_max_submits(10_000);
        let mut p = png();
        p.resize(MAX_IMAGE_BYTES, 0);
        for i in 0..MAX_WAITING_BYTES / MAX_IMAGE_BYTES {
            h.submit(
                draft(&format!("big {i}")),
                vec![att(AttachmentKind::Render, p.clone())],
                0,
            )
            .unwrap();
        }
        assert!(
            h.submit(
                draft("big more"),
                vec![att(AttachmentKind::Render, p.clone())],
                0
            )
            .is_err()
        );
        let mut d = draft("a person");
        d.origin = Origin::User;
        h.submit(d, vec![att(AttachmentKind::Render, p)], 0)
            .unwrap();
    }

    #[test]
    fn the_installation_id_is_made_once_kept_and_sent_with_the_report() {
        let (_tmp, mut a, mut b) = two_hubs();
        let id = a.installation_id();
        assert_eq!(id.len(), 36);
        assert_eq!(id.as_bytes()[14], b'4');
        assert_eq!(a.installation_id(), id);
        // Another program on the same folder has the same id.
        assert_eq!(b.installation_id(), id);
        assert_eq!(a.settings().installation.as_deref(), Some(id.as_str()));
        a.set_send(Some(true)).unwrap();
        a.submit(draft("x"), vec![], 0).unwrap();
        let out = a.outgoing(true, 5, T0);
        let v: serde_json::Value = serde_json::from_str(&out[0].payload).unwrap();
        assert_eq!(v["installation"], id.as_str());
        // A memory store makes its own, and two of them differ.
        let (mut m1, mut m2) = (Hub::in_memory(), Hub::in_memory());
        assert_ne!(m1.installation_id(), m2.installation_id());
    }

    #[test]
    fn the_installation_id_can_be_reset_and_left_out() {
        let (_tmp, mut a, mut b) = two_hubs();
        assert!(a.include_installation());
        let first = a.installation_id();
        let second = a.reset_installation_id().unwrap();
        assert_ne!(first, second);
        assert_eq!(second.len(), 36);
        assert_eq!(second.as_bytes()[14], b'4');
        // The other program sees the new id.
        assert_eq!(b.installation_id(), second);
        // The switch is stored, and the id is kept when it is off.
        a.set_include_installation(false).unwrap();
        assert!(!b.include_installation());
        assert_eq!(b.report_installation_id(), None);
        assert_eq!(b.installation_id(), second);
        a.set_send(Some(true)).unwrap();
        a.submit(draft("x"), vec![], 0).unwrap();
        let out = a.outgoing(true, 5, T0);
        let v: serde_json::Value = serde_json::from_str(&out[0].payload).unwrap();
        assert!(v.get("installation").is_none(), "{v}");
        a.set_include_installation(true).unwrap();
        assert_eq!(a.report_installation_id().as_deref(), Some(second.as_str()));
    }

    #[test]
    fn the_crash_policy_defaults_to_ask_and_is_stored() {
        let (_tmp, mut a, b) = two_hubs();
        assert_eq!(a.crash_policy(), CrashPolicy::Ask);
        for p in CrashPolicy::ALL {
            a.set_crash_policy(p).unwrap();
            assert_eq!(b.crash_policy(), p);
        }
        // An old settings file without the keys reads as the defaults.
        let s: Settings = serde_json::from_str(r#"{"send":true}"#).unwrap();
        assert_eq!(s.crash_policy, CrashPolicy::Ask);
        assert!(!s.omit_installation);
    }

    #[test]
    fn an_outbox_from_a_newer_version_is_kept_aside_and_never_written() {
        let tmp = kadakas_sys::tmp::PrivateTempDir::new("kadakas-fb-newer").unwrap();
        let dir = tmp.path().join("fb");
        std::fs::create_dir_all(&dir).unwrap();
        let newer = br#"{"version":9,"next_seq":7,"reports":[{"secret":"future"}]}"#;
        std::fs::write(dir.join("outbox.json"), newer).unwrap();
        let mut h = Hub::new(Box::new(crate::store::DirStorage::new(dir.clone())));
        assert!(h.note().is_none());
        assert!(h.list().is_empty());
        assert!(h.note().unwrap().contains("newer"));
        // Reports go to memory and work; the file is not touched, and is not copied over either.
        let s = h.submit(draft("while newer"), vec![], 0).unwrap();
        assert_eq!(h.get(&s.id).unwrap().title, "while newer");
        assert!(!h.persistent());
        assert_eq!(std::fs::read(dir.join("outbox.json")).unwrap(), newer);
        assert!(!dir.join("outbox.bad.json").exists());
        assert!(h.withdraw(&s.id).is_ok());
    }

    #[test]
    fn a_broken_outbox_is_kept_and_replaced() {
        let mut m = crate::store::MemStorage::new();
        m.write("outbox.json", b"{ not json").unwrap();
        let mut h = Hub::new(Box::new(m));
        assert!(h.list().is_empty());
        h.submit(draft("a"), vec![], 0).unwrap();
        assert_eq!(h.list().len(), 1);
        // A newer version is not read either.
        let mut m = crate::store::MemStorage::new();
        m.write("outbox.json", br#"{"version":9,"next_seq":1,"reports":[]}"#)
            .unwrap();
        let mut h = Hub::new(Box::new(m));
        assert!(h.list().is_empty());
        assert!(!h.persistent());
    }

    #[test]
    fn a_failing_store_fails_the_call() {
        let mut h = Hub::new(Box::new(crate::store::MemStorage::with_cap(10)));
        assert!(matches!(
            h.submit(draft("a"), vec![], 0),
            Err(FeedbackError::Store(_))
        ));
        assert!(h.set_send(Some(true)).is_err());
        // The failed call does not use up the rate limit.
        let mut h2 = Hub::new(Box::new(crate::store::MemStorage::with_cap(1500)));
        h2.set_max_submits(1);
        assert!(
            h2.submit(
                draft("a"),
                vec![att(AttachmentKind::Log, vec![b'x'; 2000])],
                0
            )
            .is_err()
        );
        h2.submit(draft("a"), vec![], 0).unwrap();
    }

    proptest! {
        #[test]
        fn stored_texts_never_hold_planted_data(
            s in "[A-Za-z]{6,10}[0-9]{4,8}",
            title in "[a-z ]{1,30}",
        ) {
            let mut h = Hub::in_memory();
            let mut d = draft(&format!("{title} {s}@example.com"));
            d.body = format!("/home/{s}/proj/a.kad and token={s} file:///home/{s}/b.kad");
            d.wanted = format!("Authorization: Bearer {s}");
            d.tried = format!("C:\\Users\\{s}\\a.kad");
            d.error = format!("https://u:{s}@host.example/x");
            d.method = format!("m.{title}");
            d.context.client_name = format!("{s}@example.com");
            h.submit(d, vec![], 0).unwrap();
            let json = serde_json::to_string(&h.list()).unwrap();
            prop_assert!(!json.contains(&s), "{}", json);
            // And the message to the server is clean as well.
            let p = payload::build(&h.list()[0], &PayloadParts::default()).unwrap();
            prop_assert!(!p.contains(&s));
        }

        #[test]
        fn the_count_adds_up_and_the_outbox_stays_bounded(titles in proptest::collection::vec("[ab]{1,2}", 1..40)) {
            let mut h = Hub::in_memory();
            h.set_max_submits(1000);
            let mut n = 0u32;
            for t in &titles {
                h.submit(draft(t), vec![], 0).unwrap();
                n += 1;
            }
            let reports = h.list();
            prop_assert!(reports.len() <= 6);
            prop_assert_eq!(reports.iter().map(|r| r.count).sum::<u32>(), n);
        }
    }
}
