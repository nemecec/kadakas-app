//! The review of a report with attachments: what a human sees before anything is sent.
//!
//! The app dialog and `kadakas feedback review` use this model. It loads the stored
//! attachments, lets the person untick each one and set the privacy options of the excerpt,
//! shows the exact payload, and approves it. The approval path is the only way a report with
//! attachments leaves `needs_review`.

use kadakas_feedback::{
    AttachmentKind, FeedbackError, Hub, PayloadParts, Privacy, Report, Status, payload,
};

use super::excerpt::{self, ObjectBrief};

pub struct Review {
    pub report: Report,
    /// Which attachments go. They start ticked.
    include: Vec<(AttachmentKind, bool)>,
    pub privacy: Privacy,
    excerpt: Option<String>,
    screenshot: Option<Vec<u8>>,
    render: Option<Vec<u8>>,
    calls: Option<String>,
    log: Option<String>,
}

impl Review {
    /// Opens a report that waits for review.
    pub fn open(hub: &mut Hub, id: &str) -> Result<Review, FeedbackError> {
        let report = hub
            .get(id)
            .ok_or_else(|| FeedbackError::NotFound(format!("no report {id}")))?;
        if report.status != Status::NeedsReview {
            return Err(FeedbackError::State(
                "the report does not wait for review".into(),
            ));
        }
        let text = |k| {
            hub.attachment(id, k)
                .and_then(|b| String::from_utf8(b).ok())
        };
        Ok(Review {
            include: report.attachments.iter().map(|a| (a.kind, true)).collect(),
            privacy: report.privacy,
            excerpt: text(AttachmentKind::Excerpt),
            screenshot: hub.attachment(id, AttachmentKind::Screenshot),
            render: hub.attachment(id, AttachmentKind::Render),
            calls: text(AttachmentKind::Calls),
            log: text(AttachmentKind::Log),
            report,
        })
    }

    /// The attachments of the report with their tick.
    pub fn attachments(&self) -> &[(AttachmentKind, bool)] {
        &self.include
    }

    pub fn included(&self, kind: AttachmentKind) -> bool {
        self.include.iter().any(|(k, on)| *k == kind && *on)
    }

    pub fn set_included(&mut self, kind: AttachmentKind, on: bool) {
        for (k, v) in &mut self.include {
            if *k == kind {
                *v = on;
            }
        }
    }

    /// The PNG of an image attachment.
    pub fn image(&self, kind: AttachmentKind) -> Option<&[u8]> {
        match kind {
            AttachmentKind::Screenshot => self.screenshot.as_deref(),
            AttachmentKind::Render => self.render.as_deref(),
            _ => None,
        }
    }

    pub fn calls_text(&self) -> Option<&str> {
        self.calls.as_deref()
    }

    pub fn log_text(&self) -> Option<&str> {
        self.log.as_deref()
    }

    /// The excerpt as it will be sent (privacy options applied).
    pub fn excerpt_text(&self) -> Option<Result<String, String>> {
        self.excerpt
            .as_deref()
            .map(|t| excerpt::apply_privacy(t, self.privacy))
    }

    /// The objects of that excerpt, for the list and the small plan.
    pub fn excerpt_objects(&self) -> Vec<ObjectBrief> {
        match self.excerpt_text() {
            Some(Ok(t)) => excerpt::objects_of(&t),
            _ => Vec::new(),
        }
    }

    /// The exact message that will go to the server with the current ticks and options.
    pub fn payload(&self) -> Result<String, FeedbackError> {
        let on = |k| self.included(k);
        let excerpt = if on(AttachmentKind::Excerpt) {
            match self.excerpt_text() {
                Some(r) => Some(r.map_err(FeedbackError::Invalid)?),
                None => None,
            }
        } else {
            None
        };
        // Free texts in the parameters of the recent calls count as names and notes.
        let calls = self.calls.as_deref().filter(|_| on(AttachmentKind::Calls));
        let calls = calls.map(|c| {
            if self.privacy.remove_names {
                excerpt::blank_names_in(c)
            } else {
                c.to_string()
            }
        });
        let mut report = self.report.clone();
        report.privacy = self.privacy;
        payload::build(
            &report,
            &PayloadParts {
                excerpt: excerpt.as_deref(),
                screenshot: self
                    .screenshot
                    .as_deref()
                    .filter(|_| on(AttachmentKind::Screenshot)),
                render: self
                    .render
                    .as_deref()
                    .filter(|_| on(AttachmentKind::Render)),
                calls: calls.as_deref(),
                log: self.log.as_deref().filter(|_| on(AttachmentKind::Log)),
            },
        )
    }

    /// The person agrees: the payload that [`Review::payload`] gives is stored and goes out.
    pub fn approve(&self, hub: &mut Hub) -> Result<(), FeedbackError> {
        let p = self.payload()?;
        hub.approve(&self.report.id, &p, self.privacy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Session;
    use serde_json::{Value, json};

    fn session_with_report() -> (Session, String) {
        let mut s = Session::with_sequential_ids();
        s.handle(
            "execute",
            json!({"command": {"op": "add_object", "shape": {"kind": "text", "at": [500000, 600000], "text": "Smith garden", "height": 200}}}),
        )
        .unwrap();
        let r = s
            .handle("feedback.submit", json!({"kind": "bug", "title": "T"}))
            .unwrap();
        (s, r["id"].as_str().unwrap().to_string())
    }

    #[test]
    fn the_review_shows_what_is_sent_and_approves_it() {
        let (mut s, id) = session_with_report();
        let mut r = Review::open(&mut s.feedback.hub, &id).unwrap();
        assert!(r.attachments().iter().all(|a| a.1));
        assert!(r.image(AttachmentKind::Render).is_some());
        assert!(r.image(AttachmentKind::Excerpt).is_none());
        assert!(r.calls_text().is_some() || r.log_text().is_none());
        assert_eq!(r.excerpt_objects().len(), 1);
        // Privacy options are on by default: the place is gone.
        let p = r.payload().unwrap();
        assert!(!p.contains("500000") && p.contains("Smith garden"));
        // Untick the picture and the excerpt: they are not in the payload.
        r.set_included(AttachmentKind::Render, false);
        r.set_included(AttachmentKind::Excerpt, false);
        r.privacy.remove_names = true;
        let p: Value = serde_json::from_str(&r.payload().unwrap()).unwrap();
        assert!(p["attachments"].get("render").is_none());
        assert!(p["attachments"].get("excerpt").is_none());
        r.set_included(AttachmentKind::Excerpt, true);
        let text = r.payload().unwrap();
        assert!(!text.contains("Smith garden"));
        // Approval stores exactly that payload and the hub gives it to the sender.
        r.approve(&mut s.feedback.hub).unwrap();
        let out = s.feedback.hub.outgoing(false, 5);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].payload, text);
        assert!(Review::open(&mut s.feedback.hub, &id).is_err());
        assert!(Review::open(&mut s.feedback.hub, "fb-77").is_err());
    }

    #[test]
    fn an_excerpt_that_cannot_be_made_blocks_the_payload() {
        let (mut s, id) = session_with_report();
        let mut r = Review::open(&mut s.feedback.hub, &id).unwrap();
        r.excerpt = Some("{".into());
        assert!(r.payload().is_err());
        assert!(r.excerpt_objects().is_empty());
        r.set_included(AttachmentKind::Excerpt, false);
        assert!(r.payload().is_ok());
    }
}
