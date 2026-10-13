//! The outbox: reports, attachments and settings in a [`Storage`].
//!
//! Every call reads the outbox from the store and writes it back, so two programs that share a
//! folder (the app and a headless run) see each other's reports. The file is small: the
//! attachments are files of their own.
//!
//! Consent has two parts, and the code keeps them apart:
//!
//! - Text-only reports go to the server when sending is on (`Settings::send`, or the override
//!   that `--feedback-send` sets).
//! - A report with attachments has the status `NeedsReview`. Only [`Hub::approve`] moves it on.
//!   The app review dialog and `kadakas feedback review` call it after a person has seen the
//!   exact payload. No API method calls it, and [`Hub::outgoing`] never returns such a report.

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
/// The bytes of attachments that wait in the outbox (not yet sent).
pub const MAX_WAITING_BYTES: usize = 64 << 20;
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
    submitted: u32,
    max_submits: u32,
}

impl Hub {
    pub fn new(storage: Box<dyn Storage>) -> Hub {
        Hub {
            storage,
            send_override: None,
            url_override: None,
            submitted: 0,
            max_submits: MAX_SUBMITS_PER_SESSION,
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

    /// Stores the answer of the person to "may Kadakas send my reports?".
    pub fn set_send(&mut self, on: Option<bool>) -> Result<(), FeedbackError> {
        let mut s = self.settings();
        s.send = on;
        self.save_settings(&s)
    }

    /// Stores another server address (`None`: the default). The address is checked when it is used.
    pub fn set_url(&mut self, url: Option<String>) -> Result<(), FeedbackError> {
        let mut s = self.settings();
        s.url = url
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty() && u.len() <= 300);
        self.save_settings(&s)
    }

    /// A choice for this run only (`--feedback-send`). It is not stored.
    pub fn set_send_override(&mut self, on: Option<bool>) {
        self.send_override = on;
    }

    pub fn set_url_override(&mut self, url: Option<String>) {
        self.url_override = url.filter(|u| !u.trim().is_empty());
    }

    /// True if text-only reports may go to the server.
    pub fn send_enabled(&self) -> bool {
        self.send_override.or(self.settings().send).unwrap_or(false)
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

    fn load(&mut self) -> Outbox {
        let Some(bytes) = self.storage.read(OUTBOX) else {
            return Outbox::default();
        };
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

    // ---- writes

    /// Adds a report, or raises the counter of an equal one. The texts are scrubbed and cut
    /// here, whatever the caller did. Attachments are checked against their limits; a merged
    /// report drops the new attachments.
    pub fn submit(
        &mut self,
        draft: Draft,
        attachments: Vec<AttachmentData>,
        now: u64,
    ) -> Result<Submitted, FeedbackError> {
        if self.submitted >= self.max_submits {
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
        let mut ob = self.load();
        self.submitted += 1;
        if let Some(r) = ob
            .reports
            .iter_mut()
            .find(|r| r.key == key && r.status != Status::Withdrawn)
        {
            r.count = r.count.saturating_add(1);
            let out = Submitted {
                id: r.id.clone(),
                merged: true,
                count: r.count,
                status: r.status,
            };
            self.save(&ob)?;
            return Ok(out);
        }
        if ob.reports.len() >= MAX_REPORTS {
            let old = ob
                .reports
                .iter()
                .position(|r| matches!(r.status, Status::Sent | Status::Withdrawn));
            match old {
                Some(i) => {
                    let r = ob.reports.remove(i);
                    self.drop_files(&r);
                }
                None => {
                    self.submitted -= 1;
                    return Err(FeedbackError::Limit(format!(
                        "the outbox holds {MAX_REPORTS} reports that wait; a person must send or withdraw some"
                    )));
                }
            }
        }
        let waiting: usize = ob
            .reports
            .iter()
            .filter(|r| !matches!(r.status, Status::Sent | Status::Withdrawn))
            .flat_map(|r| &r.attachments)
            .map(|a| a.bytes)
            .sum();
        let new_bytes: usize = att.iter().map(|a| a.bytes.len()).sum();
        if waiting + new_bytes > MAX_WAITING_BYTES {
            self.submitted -= 1;
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
            last_error: None,
        };
        for a in &att {
            if let Err(e) = self.storage.write(&a.kind.file(&id), &a.bytes) {
                self.drop_files(&report);
                self.submitted -= 1;
                return Err(FeedbackError::Store(e));
            }
        }
        let out = Submitted {
            id,
            merged: false,
            count: 1,
            status: report.status,
        };
        ob.reports.push(report.clone());
        if let Err(e) = self.save(&ob) {
            self.drop_files(&report);
            self.submitted -= 1;
            return Err(e);
        }
        Ok(out)
    }

    /// Takes a report back. A report that went to the server cannot be taken back here.
    pub fn withdraw(&mut self, id: &str) -> Result<Report, FeedbackError> {
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
        payload::check(payload, id)?;
        let mut ob = self.load();
        let r =
            ob.reports.iter_mut().find(|r| r.id == id).ok_or_else(|| {
                FeedbackError::NotFound(format!("no report {}", scrub::text(id, 40)))
            })?;
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
        r.last_error = None;
        self.save(&ob)
    }

    /// Makes a report that failed too often ready for new tries.
    pub fn retry(&mut self, id: &str) -> Result<(), FeedbackError> {
        let mut ob = self.load();
        let r =
            ob.reports.iter_mut().find(|r| r.id == id).ok_or_else(|| {
                FeedbackError::NotFound(format!("no report {}", scrub::text(id, 40)))
            })?;
        r.attempts = 0;
        r.last_error = None;
        self.save(&ob)
    }

    /// The reports that may go out now: approved ones, and text-only ones if `include_text` and
    /// sending is on. Reports that failed `MAX_ATTEMPTS` times wait for `retry`. At most `max`.
    pub fn outgoing(&mut self, include_text: bool, max: usize) -> Vec<Outgoing> {
        let text_ok = include_text && self.send_enabled();
        let ob = self.load();
        let mut out = Vec::new();
        for r in ob.reports.iter().filter(|r| r.attempts < MAX_ATTEMPTS) {
            if out.len() >= max {
                break;
            }
            let payload = match r.status {
                Status::Approved => self
                    .storage
                    .read(&payload_file(&r.id))
                    .and_then(|b| String::from_utf8(b).ok()),
                Status::Pending if text_ok && r.attachments.is_empty() => {
                    payload::build(r, &PayloadParts::default()).ok()
                }
                _ => None,
            };
            if let Some(payload) = payload {
                out.push(Outgoing {
                    id: r.id.clone(),
                    payload,
                });
            }
        }
        out
    }

    /// Writes down what happened to a report that was sent.
    pub fn record(&mut self, id: &str, delivery: &Delivery) -> Result<(), FeedbackError> {
        let mut ob = self.load();
        let Some(r) = ob.reports.iter_mut().find(|r| r.id == id) else {
            return Ok(());
        };
        if matches!(r.status, Status::Sent | Status::Withdrawn) {
            return Ok(());
        }
        let old = r.clone();
        match delivery {
            Delivery::Sent(reply) => {
                r.status = Status::Sent;
                r.server_id = Some(scrub::text(&reply.id, 100));
                r.server_status = Some(scrub::text(&reply.status, 40));
                r.last_error = None;
            }
            Delivery::Failed {
                message,
                permanent,
                attempts,
            } => {
                r.attempts = if *permanent {
                    MAX_ATTEMPTS
                } else {
                    r.attempts.saturating_add(*attempts)
                };
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
    }

    #[test]
    fn equal_reports_merge_into_a_counter() {
        let mut h = Hub::in_memory();
        h.submit(draft("Fillet fails"), vec![], 0).unwrap();
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
        assert!(!h.submit(draft("Fillet fails"), vec![], 0).unwrap().merged);
        assert_eq!(h.list().len(), 3);
    }

    #[test]
    fn the_session_rate_limit() {
        let mut h = Hub::in_memory();
        h.set_max_submits(2);
        h.submit(draft("a"), vec![], 0).unwrap();
        h.submit(draft("a"), vec![], 0).unwrap(); // merged, but it counts
        assert!(matches!(
            h.submit(draft("b"), vec![], 0),
            Err(FeedbackError::Limit(_))
        ));
    }

    #[test]
    fn the_outbox_is_capped() {
        let mut h = Hub::in_memory();
        h.set_max_submits(1000);
        for i in 0..MAX_REPORTS {
            h.submit(draft(&format!("report {i}")), vec![], 0).unwrap();
        }
        let e = h.submit(draft("one more"), vec![], 0).unwrap_err();
        assert!(matches!(e, FeedbackError::Limit(_)), "{e}");
        // A sent report makes room.
        h.record(
            "fb-1",
            &Delivery::Sent(Reply {
                id: "s1".into(),
                status: "received".into(),
            }),
        )
        .unwrap();
        h.submit(draft("one more"), vec![], 0).unwrap();
        assert_eq!(h.list().len(), MAX_REPORTS);
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
        assert!(h.outgoing(true, 10).is_empty());
        // The approval path makes it ready, and the payload is the stored one.
        let p = payload::build(&r, &PayloadParts::default()).unwrap();
        h.approve(&s.id, &p, Privacy::default()).unwrap();
        let out = h.outgoing(false, 10);
        assert_eq!(
            out,
            vec![Outgoing {
                id: s.id.clone(),
                payload: p
            }]
        );
        assert_eq!(h.get(&s.id).unwrap().status, Status::Approved);
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
            Err(FeedbackError::Invalid(_))
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
        // 64 MiB of waiting images need 64 reports of 1 MiB. Use the real cap with few big ones.
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
        h.record(
            &t.id,
            &Delivery::Sent(Reply {
                id: "s".into(),
                status: "received".into(),
            }),
        )
        .unwrap();
        assert!(matches!(h.withdraw(&t.id), Err(FeedbackError::State(_))));
        assert_eq!(h.get(&t.id).unwrap().server_id.as_deref(), Some("s"));
    }

    #[test]
    fn sending_follows_the_consent() {
        let mut h = Hub::in_memory();
        h.submit(draft("a"), vec![], 0).unwrap();
        assert!(!h.consent_asked() && !h.send_enabled());
        assert!(h.outgoing(true, 5).is_empty());
        h.set_send(Some(true)).unwrap();
        assert!(h.consent_asked() && h.send_enabled());
        // The host decides if text reports go out in this program.
        assert!(h.outgoing(false, 5).is_empty());
        assert_eq!(h.outgoing(true, 5).len(), 1);
        // The override of a flag wins over the preference and is not stored.
        h.set_send_override(Some(false));
        assert!(!h.send_enabled() && h.settings().send == Some(true));
        h.set_send_override(None);
        h.set_send(None).unwrap();
        assert!(!h.consent_asked());
    }

    #[test]
    fn urls() {
        let mut h = Hub::in_memory();
        assert_eq!(h.url(), DEFAULT_URL);
        h.set_url(Some(" https://fb.example ".into())).unwrap();
        assert_eq!(h.url(), "https://fb.example");
        h.set_url_override(Some("https://other.example".into()));
        assert_eq!(h.url(), "https://other.example");
        h.set_url_override(Some(" ".into()));
        assert_eq!(h.url(), "https://fb.example");
        h.set_url(Some("".into())).unwrap();
        assert_eq!(h.url(), DEFAULT_URL);
    }

    #[test]
    fn delivery_results_are_recorded() {
        let mut h = Hub::in_memory();
        h.set_send(Some(true)).unwrap();
        h.submit(draft("a"), vec![], 0).unwrap();
        h.record(
            "fb-1",
            &Delivery::Failed {
                message: "down at /srv/x/y.log".into(),
                permanent: false,
                attempts: 4,
            },
        )
        .unwrap();
        let r = h.get("fb-1").unwrap();
        assert_eq!((r.attempts, r.status), (4, Status::Pending));
        assert_eq!(r.last_error.as_deref(), Some("down at <path>/y.log"));
        h.record(
            "fb-1",
            &Delivery::Failed {
                message: "again".into(),
                permanent: false,
                attempts: 4,
            },
        )
        .unwrap();
        // Eight tries: it waits for a person.
        assert!(h.outgoing(true, 5).is_empty());
        h.retry("fb-1").unwrap();
        assert_eq!(h.outgoing(true, 5).len(), 1);
        h.record(
            "fb-1",
            &Delivery::Failed {
                message: "no".into(),
                permanent: true,
                attempts: 1,
            },
        )
        .unwrap();
        assert!(h.outgoing(true, 5).is_empty());
        assert!(matches!(h.retry("nope"), Err(FeedbackError::NotFound(_))));
        // Unknown ids, and reports that are done, are ignored.
        h.record(
            "fb-9",
            &Delivery::Sent(Reply {
                id: "s".into(),
                status: "x".into(),
            }),
        )
        .unwrap();
        h.record(
            "fb-1",
            &Delivery::Sent(Reply {
                id: "s".into(),
                status: "received".into(),
            }),
        )
        .unwrap();
        h.record(
            "fb-1",
            &Delivery::Failed {
                message: "late".into(),
                permanent: false,
                attempts: 1,
            },
        )
        .unwrap();
        assert_eq!(h.get("fb-1").unwrap().status, Status::Sent);
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
            d.body = format!("/home/{s}/proj/a.kad and token={s}");
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
