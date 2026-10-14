//! The logic of the feedback form and the outbox window (UI-14), without egui.
//!
//! - [`Form`]: kind, title, text and an optional screenshot. It fills from an app error, from a
//!   search without a result and from the crash recovery dialog.
//! - [`Flow`]: sending a form, the one-time consent of the person, and the reports that wait for
//!   that answer.
//!
//! The form is the review of a report that a person writes: they see the text and the screenshot
//! before they press Send. So a form report with a screenshot is approved by that press (with the
//! exact payload that was on screen). The reports of agents are not: they wait as `needs_review`
//! until a person opens them in the window (see `kadakas-api`, `feedback_api::review`).

use kadakas_feedback::{
    AttachmentData, AttachmentKind, Context, Draft, Hub, Kind, MAX_BODY, MAX_ERROR,
    MAX_IMAGE_BYTES, MAX_TITLE, Origin, PayloadParts, Report, Status, payload, scrub,
};
use kadakas_l10n::{Arg, msg, translate};

use crate::profile::Cmd;

/// What a Help menu command opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Open {
    /// The form, for a bug.
    ReportBug,
    /// The form, for a feature request.
    RequestFeature,
    /// The window with the outbox.
    Window,
}

impl Open {
    /// The Help command that opens it. Any other command opens the window.
    pub fn of(c: Cmd) -> Open {
        match c {
            Cmd::ReportBug => Open::ReportBug,
            Cmd::RequestFeature => Open::RequestFeature,
            _ => Open::Window,
        }
    }
}

/// The messages of the app that report an error, as ids. The "Report this" link shows next to a
/// status line that starts like one of them.
const ERROR_MESSAGES: &[&str] = &[
    msg!("panels-error"),
    msg!("app-error"),
    msg!("app-cannot-open"),
    msg!("app-cannot-start-the-session-for-scripts"),
    msg!("app-agent-server-failed-on-port"),
    msg!("collab-ui-error"),
    msg!("io-dialog-import-failed"),
    msg!("plugin-ui-install-failed"),
    msg!("plugin-ui-read-failed"),
    msg!("plugin-ui-object-error"),
    msg!("plugin-ui-tool-failed"),
    msg!("plant-ui-error"),
    msg!("resource-ui-error"),
    msg!("rules-ui-error"),
    msg!("site-ui-error"),
    msg!("style-ui-error"),
];

/// The text of a message before its first argument, in the active language.
fn fixed_prefix(id: &str) -> String {
    const MARK: &str = "\u{1}";
    let args: Vec<(&str, Arg)> = ["e", "message", "name", "error", "port"]
        .into_iter()
        .map(|n| (n, Arg::Text(MARK.into())))
        .collect();
    let text = translate(id, &args);
    text.split(MARK).next().unwrap_or("").to_string()
}

/// True if the status line shows an error message of the app.
pub fn is_error_status(status: &str) -> bool {
    !status.is_empty()
        && ERROR_MESSAGES.iter().any(|id| {
            let p = fixed_prefix(id);
            !p.trim().is_empty() && status.starts_with(&p)
        })
}

/// The report form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    pub kind: Kind,
    pub title: String,
    pub body: String,
    /// The command or method, when the form comes from an error.
    pub method: String,
    pub error: String,
    pub origin: Origin,
    /// The person wants a screenshot of the window in the report.
    pub want_screenshot: bool,
    /// The PNG of the screenshot, once the window has made it.
    pub screenshot: Option<Vec<u8>>,
}

impl Form {
    pub fn new(kind: Kind) -> Form {
        Form {
            kind,
            title: String::new(),
            body: String::new(),
            method: String::new(),
            error: String::new(),
            origin: Origin::User,
            want_screenshot: false,
            screenshot: None,
        }
    }

    /// An error message of the app: the text of the message and the last command.
    pub fn from_error(status: &str, last_command: &str) -> Form {
        let mut f = Form::new(Kind::Bug);
        f.title = scrub::cut(status.lines().next().unwrap_or(""), MAX_TITLE);
        f.method = last_command.to_string();
        f.error = scrub::cut(status, MAX_ERROR);
        f
    }

    /// A search that found no command.
    pub fn from_search(query: &str) -> Form {
        let mut f = Form::new(Kind::FeatureRequest);
        let q = scrub::cut(query.trim(), 80);
        f.title = format!("Command: {q}");
        f.body = format!("I searched for \"{q}\" and found no command.");
        f
    }

    /// The crash recovery dialog: Kadakas did not close cleanly.
    pub fn from_crash(versions: usize) -> Form {
        let mut f = Form::new(Kind::Bug);
        f.origin = Origin::Crash;
        f.title = "Kadakas did not close cleanly".into();
        f.body =
            format!("The last session ended without a clean exit. Recovered versions: {versions}.");
        f
    }

    /// The form can be sent: it has a title.
    pub fn can_send(&self) -> bool {
        !self.title.trim().is_empty()
    }

    /// The draft of the report, with the context of this program.
    pub fn draft(&self, context: Context) -> Draft {
        let mut d = Draft::new(
            self.kind,
            &self.title,
            &scrub::cut(&self.body, MAX_BODY * 4),
            self.origin,
        );
        d.method = self.method.clone();
        d.error = self.error.clone();
        d.context = context;
        d
    }

    /// The screenshot that goes with the report: only if the person ticked it and it fits.
    pub fn attachment(&self) -> Option<AttachmentData> {
        let png = self.screenshot.as_ref().filter(|_| self.want_screenshot)?;
        (png.len() <= MAX_IMAGE_BYTES).then(|| AttachmentData {
            kind: AttachmentKind::Screenshot,
            bytes: png.clone(),
            objects: None,
        })
    }
}

/// The context that the app adds: version and system. The client is the person's program.
pub fn app_context(version: &str, os: &str) -> Context {
    Context {
        version: version.into(),
        os: os.into(),
        client_name: "kadakas-app".into(),
        client_version: version.into(),
    }
}

/// A report whose approval waits for the answer of the consent dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Waiting {
    id: String,
    /// The exact payload that the person saw, if the report has a screenshot.
    payload: Option<String>,
}

/// What `send` did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sent {
    pub id: String,
    pub merged: bool,
    pub status: Status,
}

/// Sending forms and asking for consent.
#[derive(Default)]
pub struct Flow {
    waiting: Vec<Waiting>,
    /// "Ask me later": the dialog stays closed until the next start.
    snoozed: bool,
}

impl Flow {
    /// Files the report of a form. A form with a screenshot is approved at once if the person
    /// answered the consent question; else it waits for the answer.
    pub fn send(
        &mut self,
        hub: &mut Hub,
        form: &Form,
        context: Context,
        now: u64,
    ) -> Result<Sent, String> {
        if !form.can_send() {
            return Err("the title is empty".into());
        }
        let att: Vec<AttachmentData> = form.attachment().into_iter().collect();
        let shot = att.first().map(|a| a.bytes.clone());
        let out = hub
            .submit(form.draft(context), att, now)
            .map_err(|e| e.to_string())?;
        if !out.merged
            && let Some(png) = shot
        {
            let report = hub.get(&out.id).ok_or("the report is gone")?;
            let p = payload::build(
                &report,
                &PayloadParts {
                    screenshot: Some(&png),
                    ..Default::default()
                },
            )
            .map_err(|e| e.to_string())?;
            if !hub.consent_asked() {
                self.waiting.push(Waiting {
                    id: out.id.clone(),
                    payload: Some(p),
                });
            } else if hub.send_enabled() {
                hub.approve(&out.id, &p, report.privacy)
                    .map_err(|e| e.to_string())?;
            }
            // Else the person said no: the report waits in the outbox for a review.
        }
        let status = hub.get(&out.id).map_or(out.status, |r| r.status);
        Ok(Sent {
            id: out.id,
            merged: out.merged,
            status,
        })
    }

    /// True if the consent dialog should show: the person was not asked, and a report waits.
    /// `asked` is `Hub::consent_asked`, and `reports` is `Hub::list`; the caller keeps both for a
    /// moment, because reading them is not free.
    pub fn consent_due(&self, asked: bool, reports: &[Report]) -> bool {
        !self.snoozed
            && !asked
            && reports
                .iter()
                .any(|r| !matches!(r.status, Status::Sent | Status::Withdrawn))
    }

    /// The answer of the dialog. `Some(true)` allows sending (and approves the reports that
    /// wait), `Some(false)` keeps every report in the outbox, `None` asks again at the next start.
    pub fn answer_consent(&mut self, hub: &mut Hub, answer: Option<bool>) -> Result<(), String> {
        let Some(allow) = answer else {
            self.snoozed = true;
            return Ok(());
        };
        hub.set_send(Some(allow)).map_err(|e| e.to_string())?;
        let waiting = std::mem::take(&mut self.waiting);
        if allow {
            for w in waiting {
                if let Some(p) = w.payload {
                    let privacy = hub.get(&w.id).map(|r| r.privacy).unwrap_or_default();
                    hub.approve(&w.id, &p, privacy).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
        v.extend([0u8; 8]);
        v
    }

    fn ctx() -> Context {
        app_context("1.0", "linux")
    }

    #[test]
    fn the_help_commands_open_the_form_or_the_window() {
        assert_eq!(Open::of(Cmd::ReportBug), Open::ReportBug);
        assert_eq!(Open::of(Cmd::RequestFeature), Open::RequestFeature);
        assert_eq!(Open::of(Cmd::Feedback), Open::Window);
    }

    #[test]
    fn error_messages_of_the_app_are_found() {
        // The language is set for this thread only: other tests run in parallel.
        kadakas_l10n::with_language("en-US", || {
            assert!(is_error_status("Error: object does not exist"));
            assert!(is_error_status("Cannot open garden.kad: bad file"));
            assert!(is_error_status("Import failed: no layer"));
            assert!(!is_error_status("Ready"));
            assert!(!is_error_status(""));
        });
        kadakas_l10n::with_language("et", || {
            assert!(is_error_status(&translate(
                "app-error",
                &[("e", Arg::Text("x".into()))]
            )));
            assert!(!is_error_status("Error: x"));
        });
    }

    #[test]
    fn forms_fill_from_their_source() {
        let f = Form::from_error("Error: radius must be positive", "edit.fillet");
        assert_eq!((f.kind, f.origin), (Kind::Bug, Origin::User));
        assert_eq!(f.method, "edit.fillet");
        assert!(f.error.contains("radius") && f.title.contains("radius"));
        let f = Form::from_search("mirror along line");
        assert_eq!(f.kind, Kind::FeatureRequest);
        assert!(f.title.contains("mirror") && f.body.contains("no command"));
        let f = Form::from_crash(3);
        assert_eq!(f.origin, Origin::Crash);
        assert!(f.body.contains('3'));
        assert!(!Form::new(Kind::Bug).can_send());
    }

    #[test]
    fn the_screenshot_goes_only_if_ticked_and_small() {
        let mut f = Form::new(Kind::Bug);
        f.title = "t".into();
        f.screenshot = Some(png());
        assert!(f.attachment().is_none());
        f.want_screenshot = true;
        assert!(f.attachment().is_some());
        f.screenshot = Some(vec![0; MAX_IMAGE_BYTES + 1]);
        assert!(f.attachment().is_none());
        f.screenshot = None;
        assert!(f.attachment().is_none());
    }

    #[test]
    fn a_text_report_waits_for_the_consent_dialog() {
        let mut hub = Hub::in_memory();
        let mut flow = Flow::default();
        assert!(!flow.consent_due(hub.consent_asked(), &hub.list()));
        let mut f = Form::new(Kind::FeatureRequest);
        f.title = "Mirror tool".into();
        let s = flow.send(&mut hub, &f, ctx(), 5).unwrap();
        assert_eq!((s.id.as_str(), s.status), ("fb-1", Status::Pending));
        assert!(flow.consent_due(hub.consent_asked(), &hub.list()));
        // Nothing is ready to send before the answer.
        assert!(hub.outgoing(true, 5, 1_000).is_empty());
        flow.answer_consent(&mut hub, Some(true)).unwrap();
        assert!(!flow.consent_due(hub.consent_asked(), &hub.list()));
        assert_eq!(hub.outgoing(true, 5, 1_000).len(), 1);
        assert_eq!(hub.get("fb-1").unwrap().origin, Origin::User);
        assert_eq!(hub.get("fb-1").unwrap().context.client_name, "kadakas-app");
    }

    #[test]
    fn a_screenshot_is_approved_after_the_answer_and_not_after_a_refusal() {
        for allow in [true, false] {
            let mut hub = Hub::in_memory();
            let mut flow = Flow::default();
            let mut f = Form::new(Kind::Bug);
            f.title = "Odd picture".into();
            f.want_screenshot = true;
            f.screenshot = Some(png());
            let s = flow.send(&mut hub, &f, ctx(), 0).unwrap();
            assert_eq!(s.status, Status::NeedsReview);
            flow.answer_consent(&mut hub, Some(allow)).unwrap();
            let st = hub.get("fb-1").unwrap().status;
            assert_eq!(
                st,
                if allow {
                    Status::Approved
                } else {
                    Status::NeedsReview
                }
            );
            let out = hub.outgoing(false, 5, 1_000);
            assert_eq!(out.len(), usize::from(allow));
            if allow {
                // The payload is the one that was built from the form: text and screenshot.
                assert!(
                    out[0].payload.contains("Odd picture") && out[0].payload.contains("screenshot")
                );
            }
        }
    }

    #[test]
    fn after_the_consent_a_screenshot_report_is_approved_at_once() {
        let mut hub = Hub::in_memory();
        hub.set_send(Some(true)).unwrap();
        let mut flow = Flow::default();
        let mut f = Form::new(Kind::Bug);
        f.title = "Again".into();
        f.want_screenshot = true;
        f.screenshot = Some(png());
        let s = flow.send(&mut hub, &f, ctx(), 0).unwrap();
        assert_eq!(s.status, Status::Approved);
        // The same report again merges and keeps the status.
        let s = flow.send(&mut hub, &f, ctx(), 0).unwrap();
        assert!(s.merged);
        assert_eq!(hub.get("fb-1").unwrap().count, 2);
    }

    #[test]
    fn a_form_with_a_screenshot_is_not_approved_when_the_person_said_no() {
        let mut hub = Hub::in_memory();
        hub.set_send(Some(false)).unwrap();
        let mut flow = Flow::default();
        let mut f = Form::new(Kind::Bug);
        f.title = "Refused".into();
        f.want_screenshot = true;
        f.screenshot = Some(png());
        let s = flow.send(&mut hub, &f, ctx(), 0).unwrap();
        assert_eq!(s.status, Status::NeedsReview);
        assert_eq!(hub.get("fb-1").unwrap().status, Status::NeedsReview);
        assert!(hub.outgoing(true, 5, 1_000).is_empty());
        assert!(hub.outgoing(false, 5, 1_000).is_empty());
        // No dialog either: the answer was given.
        assert!(!flow.consent_due(hub.consent_asked(), &hub.list()));
    }

    #[test]
    fn later_and_refusal() {
        let mut hub = Hub::in_memory();
        let mut flow = Flow::default();
        let mut f = Form::new(Kind::Bug);
        f.title = "x".into();
        flow.send(&mut hub, &f, ctx(), 0).unwrap();
        flow.answer_consent(&mut hub, None).unwrap();
        // Snoozed: no dialog, nothing sent, the person is still unasked.
        assert!(!flow.consent_due(hub.consent_asked(), &hub.list()) && !hub.consent_asked());
        let mut flow = Flow::default();
        assert!(flow.consent_due(hub.consent_asked(), &hub.list()));
        flow.answer_consent(&mut hub, Some(false)).unwrap();
        assert!(hub.consent_asked() && !hub.send_enabled());
        assert!(hub.outgoing(true, 5, 1_000).is_empty());
        assert!(
            flow.send(&mut hub, &Form::new(Kind::Bug), ctx(), 0)
                .is_err()
        );
    }
}
