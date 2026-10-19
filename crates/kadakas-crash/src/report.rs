//! From a crash file to a report in the feedback outbox, and the exact text that goes out.
//!
//! 1. [`assemble`] scrubs the crash file (`scrub`) and builds the parts: the title with the
//!    signature, the log lines, the names of the commands, and the crash object.
//! 2. [`Review::open`] puts the report into the outbox as a report that waits for review
//!    (`Status::NeedsReview`): nothing is sent. The person sees [`Review::payload`], the exact
//!    text of the message, and can add a note and a contact.
//! 3. [`Review::send`] stores that text as approved. The sending thread of the app sends it
//!    (`kadakas_feedback::dispatch`), with the retries and the waiting of the outbox when the
//!    computer is offline. [`Review::discard`] takes the report back.
//!
//! Crash reports use the channel of UI-14 as it is: same outbox, same transport, same
//! `installation` field (only if the person did not turn it off), same server protocol (the
//! crash details are the additional object `crash`, see `docs/feedback.md`).

use crate::file::{CrashFile, Frame};
use crate::scrub::{self, Context};
use crate::signature;
use kadakas_feedback::payload::{self, PayloadParts};
use kadakas_feedback::{
    AttachmentData, AttachmentKind, Draft, FeedbackError, Hub, Kind, Origin, Privacy, Report,
    Severity, Status, Submitted,
};
use serde_json::{Value, json};

/// The most crash reports that wait in the outbox. When one more comes, the oldest goes.
pub const MAX_WAITING: usize = 10;
/// The longest note and contact (characters).
pub const MAX_NOTE: usize = 1000;
pub const MAX_CONTACT: usize = 200;

/// A crash file, scrubbed and cut into the parts of a report.
#[derive(Clone, Debug, PartialEq)]
pub struct Assembled {
    pub signature: String,
    pub draft: Draft,
    /// The log lines, one per line (the attachment `log`).
    pub log: String,
    /// The names of the last commands, as the JSON array of the attachment `calls`.
    pub commands: String,
    /// The crash object without the note and the contact.
    pub crash: Value,
}

/// Scrubs `file` with `ctx` and builds the parts of the report.
pub fn assemble(file: &CrashFile, ctx: &Context) -> Assembled {
    let signature = signature::of(file);
    let message = scrub::text(&file.message, ctx);
    let frames: Vec<Frame> = file.frames.iter().map(scrub::frame).collect();
    let log: Vec<String> = file.log.iter().map(|l| scrub::text(l, ctx)).collect();
    let mut log_text = log.join("\n");
    if kadakas_feedback::model::MAX_LOG_BYTES < log_text.len() {
        let skip = log_text.len() - kadakas_feedback::model::MAX_LOG_BYTES;
        let mut at = skip;
        while !log_text.is_char_boundary(at) {
            at += 1;
        }
        log_text = log_text[at..].to_string();
    }
    // The names were checked when they came into the ring; they pass the scrubbing anyway, so a
    // file from another source cannot bring text in.
    let commands: Vec<Value> = file
        .commands
        .iter()
        .filter(|c| !c.is_empty() && c.len() <= 80)
        .filter(|c| {
            c.chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-' | ':'))
        })
        .map(|c| json!({"method": c}))
        .collect();
    let thread = scrub::text(&file.thread, ctx);
    let gpu = file.gpu.as_ref().map(|g| {
        json!({
            "adapter": scrub::text(&g.adapter, ctx),
            "driver": scrub::text(&g.driver, ctx),
            "backend": scrub::text(&g.backend, ctx),
        })
    });
    let version = scrub::text(&file.version, ctx);
    let channel = scrub::text(&file.channel, ctx);
    let os = scrub::text(&file.os, ctx);
    let crash = json!({
        "signature": signature,
        "kind": file.kind.id(),
        "version": version,
        "channel": channel,
        "thread": thread,
        "gpu": gpu,
        "message": message,
        "frames": frames,
        "memory_peak_mib": file.memory_peak_mib,
        "uptime_secs": file.uptime_secs,
    });
    let title = format!("Crash {signature} in {version}");
    let body = format!(
        "Kadakas {version} ({channel}) on {os} ended with a {} in thread {thread}.",
        file.kind.id()
    );
    let mut draft = Draft::new(Kind::Bug, &title, &body, Origin::Crash);
    draft.error = message;
    draft.severity = Some(Severity::High);
    draft.context.version = version;
    draft.context.os = os;
    draft.context.client_name = "kadakas".into();
    Assembled {
        signature,
        draft,
        log: log_text,
        commands: Value::Array(commands).to_string(),
        crash,
    }
}

impl Assembled {
    /// The attachments for the outbox: the log lines and the command names.
    pub fn attachments(&self) -> Vec<AttachmentData> {
        let mut v = Vec::new();
        if !self.log.is_empty() {
            v.push(AttachmentData {
                kind: AttachmentKind::Log,
                bytes: self.log.clone().into_bytes(),
                objects: None,
            });
        }
        v.push(AttachmentData {
            kind: AttachmentKind::Calls,
            bytes: self.commands.clone().into_bytes(),
            objects: None,
        });
        v
    }
}

/// What the person adds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Extras {
    pub note: String,
    pub contact: String,
}

fn cut(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

/// The exact message for `report`: the payload of the feedback channel, with the object `crash`
/// (and the note and the contact, if there are any) added, as indented JSON. The text that the
/// person reads is the text that is sent.
pub fn payload_text(
    report: &Report,
    a: &Assembled,
    extras: &Extras,
    installation: Option<&str>,
) -> Result<String, FeedbackError> {
    let base = payload::build(
        report,
        &PayloadParts {
            calls: Some(&a.commands),
            log: (!a.log.is_empty()).then_some(a.log.as_str()),
            installation,
            ..PayloadParts::default()
        },
    )?;
    let mut v: Value = serde_json::from_str(&base)
        .map_err(|e| FeedbackError::Invalid(format!("the message is not JSON: {e}")))?;
    let mut crash = a.crash.clone();
    let note = cut(&extras.note, MAX_NOTE);
    let contact = cut(&extras.contact, MAX_CONTACT);
    if !note.is_empty() {
        crash["note"] = json!(note);
    }
    if !contact.is_empty() {
        crash["contact"] = json!(contact);
    }
    v["crash"] = crash;
    serde_json::to_string_pretty(&v)
        .map_err(|e| FeedbackError::Invalid(format!("the message cannot be written: {e}")))
}

/// Withdraws the oldest crash reports that wait in the outbox until `max` are left.
pub fn trim_outbox(hub: &mut Hub, max: usize) {
    let waiting: Vec<String> = hub
        .list()
        .into_iter()
        .filter(|r| {
            r.origin == Origin::Crash && !matches!(r.status, Status::Sent | Status::Withdrawn)
        })
        .map(|r| r.id)
        .collect();
    if waiting.len() > max {
        for id in &waiting[..waiting.len() - max] {
            let _ = hub.withdraw(id);
        }
    }
}

/// A crash report that waits in the outbox for the decision of the person.
#[derive(Clone, Debug, PartialEq)]
pub struct Review {
    id: String,
    assembled: Assembled,
    pub extras: Extras,
}

impl Review {
    /// Puts the report into the outbox, waiting for review. Nothing is sent. `None` means that
    /// the same crash (same signature, same version) is in the outbox already and goes out or
    /// has gone out: only its counter went up.
    pub fn open(
        hub: &mut Hub,
        file: &CrashFile,
        ctx: &Context,
        now: u64,
    ) -> Result<Option<Review>, FeedbackError> {
        let assembled = assemble(file, ctx);
        let Submitted {
            id, merged, status, ..
        } = hub.submit(assembled.draft.clone(), assembled.attachments(), now)?;
        if merged && status != Status::NeedsReview {
            return Ok(None);
        }
        Ok(Some(Review {
            id,
            assembled,
            extras: Extras::default(),
        }))
    }

    /// The id of the report in the outbox.
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn signature(&self) -> &str {
        &self.assembled.signature
    }

    pub fn assembled(&self) -> &Assembled {
        &self.assembled
    }

    /// The exact text that goes out, with the note and the contact as they are now.
    pub fn payload(&self, hub: &mut Hub) -> Result<String, FeedbackError> {
        let report = hub
            .get(&self.id)
            .ok_or_else(|| FeedbackError::NotFound(format!("no report {}", self.id)))?;
        let installation = hub.report_installation_id();
        payload_text(
            &report,
            &self.assembled,
            &self.extras,
            installation.as_deref(),
        )
    }

    /// The person chose Send: the text is stored as approved. The sending thread sends it.
    pub fn send(self, hub: &mut Hub) -> Result<(), FeedbackError> {
        let text = self.payload(hub)?;
        hub.approve(&self.id, &text, Privacy::default())?;
        trim_outbox(hub, MAX_WAITING);
        Ok(())
    }

    /// The person chose Do not send, or closed the question: the report leaves the outbox.
    pub fn discard(self, hub: &mut Hub) {
        let _ = hub.withdraw(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::{CrashKind, Gpu};
    use kadakas_feedback::{CrashPolicy, Hub};

    fn ctx() -> Context {
        Context::new("annika", "/Users/annika")
    }

    pub(crate) fn fake_file() -> CrashFile {
        let mut f = CrashFile::new(CrashKind::Panic);
        f.run_id = "1a-2b".into();
        f.created = 1_700_000_000;
        f.version = "0.4.0".into();
        f.channel = "stable".into();
        f.os = "macos aarch64".into();
        f.gpu = Some(Gpu {
            adapter: "Apple M2".into(),
            driver: "Metal 3".into(),
            backend: "Metal".into(),
        });
        f.message = "called `Result::unwrap()` on an `Err` value: Io(\"cannot read /Users/annika/Documents/Oak Garden.kad\")".into();
        f.thread = "main".into();
        f.frames = vec![
            Frame {
                function: "core::panicking::panic_fmt::h0123456789abcdef".into(),
                file: Some(
                    "/rustc/0123456789abcdef0123456789abcdef01234567/library/core/src/panicking.rs"
                        .into(),
                ),
                line: Some(72),
            },
            Frame {
                function: "kadakas_io::dxf::read::h1111111111111111".into(),
                file: Some("/Users/annika/dev/kadakas/crates/kadakas-io/src/dxf.rs".into()),
                line: Some(210),
            },
            Frame {
                function: "kadakas_app::app::App::open".into(),
                file: Some("/Users/annika/Garden/helper.rs".into()),
                line: Some(5),
            },
        ];
        f.log = vec![
            "opened /Users/annika/Documents/Oak Garden.kad".into(),
            "mail annika@example.com about it".into(),
        ];
        f.commands = vec![
            "file.open".into(),
            "edit.move".into(),
            "open Oak Garden".into(),
        ];
        f.memory_peak_mib = Some(812);
        f.uptime_secs = 640;
        f
    }

    #[test]
    fn a_fake_crash_file_becomes_a_scrubbed_report() {
        let a = assemble(&fake_file(), &ctx());
        let all = serde_json::to_string(&json!({
            "crash": a.crash, "log": a.log, "cmds": a.commands,
            "title": a.draft.title, "body": a.draft.body, "error": a.draft.error,
        }))
        .unwrap();
        for leak in [
            "annika",
            "Annika",
            "Oak",
            "Garden",
            "Documents",
            "example.com",
            "helper.rs",
            "/Users",
        ] {
            assert!(!all.contains(leak), "{leak} in {all}");
        }
        assert_eq!(a.draft.origin, Origin::Crash);
        assert_eq!(a.draft.kind, Kind::Bug);
        assert!(a.draft.title.starts_with("Crash "));
        assert!(a.draft.title.contains(&a.signature));
        assert!(a.draft.title.ends_with("in 0.4.0"));
        // What is useful stays.
        assert_eq!(a.crash["frames"][1]["file"], "crates/kadakas-io/src/dxf.rs");
        assert_eq!(a.crash["frames"][1]["function"], "kadakas_io::dxf::read");
        assert_eq!(
            a.crash["frames"][0]["file"],
            "rustc/library/core/src/panicking.rs"
        );
        assert_eq!(a.crash["gpu"]["backend"], "Metal");
        assert_eq!(a.crash["memory_peak_mib"], 812);
        assert_eq!(a.crash["uptime_secs"], 640);
        assert_eq!(a.crash["channel"], "stable");
        // Only plain names are commands; the text with spaces is dropped.
        assert_eq!(
            a.commands,
            r#"[{"method":"file.open"},{"method":"edit.move"}]"#
        );
    }

    #[test]
    fn a_log_of_wide_characters_is_cut_to_the_limit_on_a_character() {
        let mut f = fake_file();
        f.log = (0..50).map(|_| "\u{e4}".repeat(290)).collect();
        let a = assemble(&f, &ctx());
        assert!(a.log.len() <= kadakas_feedback::model::MAX_LOG_BYTES);
        assert!(a.log.chars().all(|c| c == '\u{e4}' || c == '\n'));
        assert_eq!(a.attachments().len(), 2);
        // No log lines: only the command names are attached.
        f.log.clear();
        assert_eq!(assemble(&f, &ctx()).attachments().len(), 1);
    }

    #[test]
    fn the_signature_does_not_depend_on_who_crashed() {
        let a = assemble(&fake_file(), &ctx());
        let mut other = fake_file();
        other.message = "a different message".into();
        other.thread = "worker".into();
        other.uptime_secs = 1;
        for f in &mut other.frames {
            f.file = Some("/somewhere/else.rs".into());
        }
        let b = assemble(&other, &Context::new("bob", "/home/bob"));
        assert_eq!(a.signature, b.signature);
    }

    #[test]
    fn the_payload_is_the_channel_message_plus_the_crash_object() {
        let mut hub = Hub::in_memory();
        let review = Review::open(&mut hub, &fake_file(), &ctx(), 1)
            .unwrap()
            .unwrap();
        let text = review.payload(&mut hub).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["protocol"], 1);
        assert_eq!(v["origin"], "crash");
        assert_eq!(v["kind"], "bug");
        assert_eq!(v["crash"]["signature"], review.signature());
        assert_eq!(v["context"]["version"], "0.4.0");
        assert_eq!(v["context"]["os"], "macos aarch64");
        assert!(v["attachments"]["log"].as_str().unwrap().contains("<path>"));
        assert_eq!(v["attachments"]["calls"][0]["method"], "file.open");
        assert_eq!(v["installation"].as_str().unwrap().len(), 36);
        assert!(v["crash"].get("note").is_none());
        // The text is indented JSON: what the dialog shows is what is sent.
        assert!(text.contains("\n  \"crash\""));
    }

    #[test]
    fn note_and_contact_go_into_the_text_and_are_cut() {
        let mut hub = Hub::in_memory();
        let mut review = Review::open(&mut hub, &fake_file(), &ctx(), 1)
            .unwrap()
            .unwrap();
        review.extras.note = format!("I was drawing a bed {}", "x".repeat(3000));
        review.extras.contact = " ann@example.com ".into();
        let v: Value = serde_json::from_str(&review.payload(&mut hub).unwrap()).unwrap();
        assert!(
            v["crash"]["note"]
                .as_str()
                .unwrap()
                .starts_with("I was drawing")
        );
        assert_eq!(
            v["crash"]["note"].as_str().unwrap().chars().count(),
            MAX_NOTE
        );
        assert_eq!(v["crash"]["contact"], "ann@example.com");
    }

    #[test]
    fn the_installation_id_follows_the_switch() {
        let mut hub = Hub::in_memory();
        let review = Review::open(&mut hub, &fake_file(), &ctx(), 1)
            .unwrap()
            .unwrap();
        let id = hub.installation_id();
        let v: Value = serde_json::from_str(&review.payload(&mut hub).unwrap()).unwrap();
        assert_eq!(v["installation"], id.as_str());
        hub.set_include_installation(false).unwrap();
        let v: Value = serde_json::from_str(&review.payload(&mut hub).unwrap()).unwrap();
        assert!(v.get("installation").is_none());
        let fresh = hub.reset_installation_id().unwrap();
        hub.set_include_installation(true).unwrap();
        let v: Value = serde_json::from_str(&review.payload(&mut hub).unwrap()).unwrap();
        assert_eq!(v["installation"], fresh.as_str());
        assert_ne!(fresh, id);
    }

    #[test]
    fn nothing_waits_for_sending_before_the_person_says_send() {
        let mut hub = Hub::in_memory();
        hub.set_send(Some(true)).unwrap();
        let review = Review::open(&mut hub, &fake_file(), &ctx(), 1)
            .unwrap()
            .unwrap();
        assert_eq!(hub.get(review.id()).unwrap().status, Status::NeedsReview);
        assert!(
            hub.outgoing(true, 10, 100).is_empty(),
            "no sending before the choice"
        );
        let id = review.id().to_string();
        let text = review.payload(&mut hub).unwrap();
        review.send(&mut hub).unwrap();
        let out = hub.outgoing(true, 10, 100);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, id);
        assert_eq!(
            out[0].payload, text,
            "the text that was shown is the text that goes out"
        );
    }

    #[test]
    fn do_not_send_takes_the_report_back() {
        let mut hub = Hub::in_memory();
        hub.set_send(Some(true)).unwrap();
        let review = Review::open(&mut hub, &fake_file(), &ctx(), 1)
            .unwrap()
            .unwrap();
        let id = review.id().to_string();
        review.discard(&mut hub);
        assert_eq!(hub.get(&id).unwrap().status, Status::Withdrawn);
        assert!(hub.outgoing(true, 10, 100).is_empty());
    }

    #[test]
    fn the_same_crash_is_counted_not_sent_twice() {
        let mut hub = Hub::in_memory();
        hub.set_send(Some(true)).unwrap();
        let first = Review::open(&mut hub, &fake_file(), &ctx(), 1)
            .unwrap()
            .unwrap();
        let id = first.id().to_string();
        first.send(&mut hub).unwrap();
        // The same crash again while the first waits to go out.
        assert!(
            Review::open(&mut hub, &fake_file(), &ctx(), 2)
                .unwrap()
                .is_none()
        );
        assert_eq!(hub.get(&id).unwrap().count, 2);
        // A new version is a new group.
        let mut newer = fake_file();
        newer.version = "0.4.1".into();
        assert!(Review::open(&mut hub, &newer, &ctx(), 3).unwrap().is_some());
    }

    #[test]
    fn at_most_ten_crash_reports_wait_and_the_oldest_go_first() {
        let mut hub = Hub::in_memory();
        // Offline: nothing is ever delivered.
        let mut ids = Vec::new();
        for i in 0..14 {
            let mut f = fake_file();
            f.version = format!("0.4.{i}");
            let r = Review::open(&mut hub, &f, &ctx(), i as u64)
                .unwrap()
                .unwrap();
            ids.push(r.id().to_string());
            r.send(&mut hub).unwrap();
            let waiting = hub
                .list()
                .iter()
                .filter(|r| r.origin == Origin::Crash && r.status == Status::Approved)
                .count();
            assert!(waiting <= MAX_WAITING, "{waiting}");
        }
        let waiting: Vec<String> = hub
            .list()
            .into_iter()
            .filter(|r| r.status == Status::Approved)
            .map(|r| r.id)
            .collect();
        assert_eq!(waiting, ids[4..].to_vec());
        assert_eq!(hub.get(&ids[0]).unwrap().status, Status::Withdrawn);
    }

    #[test]
    fn user_reports_are_not_counted_in_the_crash_limit() {
        let mut hub = Hub::in_memory();
        for i in 0..12 {
            let mut d = Draft::new(Kind::Bug, &format!("user {i}"), "b", Origin::User);
            d.context.version = "1".into();
            hub.submit(d, vec![], 1).unwrap();
        }
        trim_outbox(&mut hub, MAX_WAITING);
        assert!(hub.list().iter().all(|r| r.status != Status::Withdrawn));
        let _ = CrashPolicy::Ask;
    }
}
