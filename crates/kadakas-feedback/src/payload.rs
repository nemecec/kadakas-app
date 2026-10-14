//! The message to the feedback server (protocol 1, see `docs/feedback.md`).
//!
//! One JSON object. Images are base64 text. The whole message is at most [`MAX_PAYLOAD_BYTES`].

use base64::Engine;
use serde_json::{Value, json};

use crate::error::FeedbackError;
use crate::model::{MAX_PAYLOAD_BYTES, Report};

/// The version of the protocol.
pub const PROTOCOL: u32 = 1;

/// The attachments that go with a report. A part that is `None` is not sent.
#[derive(Default)]
pub struct PayloadParts<'a> {
    /// The text of a `.kad` document (privacy options already applied).
    pub excerpt: Option<&'a str>,
    pub screenshot: Option<&'a [u8]>,
    pub render: Option<&'a [u8]>,
    /// The text of the JSON array of calls.
    pub calls: Option<&'a str>,
    pub log: Option<&'a str>,
    /// The random id of this installation (not of the person). The server uses it with
    /// `client_id` to drop a report that arrives twice.
    pub installation: Option<&'a str>,
}

fn image(png: &[u8]) -> Value {
    json!({"mime": "image/png", "base64": base64::engine::general_purpose::STANDARD.encode(png)})
}

/// Builds the message. Fails if an attachment is not JSON where it must be, or if the message
/// is too long.
pub fn build(report: &Report, parts: &PayloadParts) -> Result<String, FeedbackError> {
    let mut att = serde_json::Map::new();
    if let Some(t) = parts.excerpt {
        let doc: Value = serde_json::from_str(t)
            .map_err(|e| FeedbackError::Invalid(format!("the excerpt is not JSON: {e}")))?;
        att.insert("excerpt".into(), json!({"format": "kad", "document": doc}));
    }
    if let Some(p) = parts.screenshot {
        att.insert("screenshot".into(), image(p));
    }
    if let Some(p) = parts.render {
        att.insert("render".into(), image(p));
    }
    if let Some(t) = parts.calls {
        let calls: Value = serde_json::from_str(t)
            .map_err(|e| FeedbackError::Invalid(format!("the call log is not JSON: {e}")))?;
        att.insert("calls".into(), calls);
    }
    if let Some(t) = parts.log {
        att.insert("log".into(), json!(t));
    }
    let mut m = json!({
        "protocol": PROTOCOL,
        "client_id": report.id,
        "kind": report.kind,
        "title": report.title,
        "body": report.body,
        "origin": report.origin,
        "count": report.count,
        "context": report.context,
        "attachments": att,
    });
    if let Some(i) = parts.installation {
        m["installation"] = json!(i);
    }
    for (k, v) in [
        ("wanted", &report.wanted),
        ("tried", &report.tried),
        ("method", &report.method),
        ("error", &report.error),
    ] {
        if !v.is_empty() {
            m[k] = json!(v);
        }
    }
    if let Some(s) = report.severity {
        m["severity"] = json!(s);
    }
    let text = m.to_string();
    if text.len() > MAX_PAYLOAD_BYTES {
        return Err(FeedbackError::Invalid(format!(
            "the report is larger than {} bytes",
            MAX_PAYLOAD_BYTES
        )));
    }
    Ok(text)
}

/// Checks a stored payload before it is approved: JSON of protocol 1 for this report.
pub fn check(payload: &str, id: &str) -> Result<(), FeedbackError> {
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(FeedbackError::Invalid("the payload is too large".into()));
    }
    let v: Value = serde_json::from_str(payload)
        .map_err(|e| FeedbackError::Invalid(format!("the payload is not JSON: {e}")))?;
    if v["protocol"] != PROTOCOL || v["client_id"] != id {
        return Err(FeedbackError::Invalid(
            "the payload is not for this report".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn report() -> Report {
        Report {
            id: "fb-1".into(),
            kind: Kind::Bug,
            title: "t".into(),
            body: "b".into(),
            wanted: String::new(),
            tried: "tr".into(),
            method: "m.x".into(),
            error: "e".into(),
            severity: Some(Severity::High),
            origin: Origin::Agent,
            context: Context {
                version: "1".into(),
                os: "linux".into(),
                client_name: "c".into(),
                client_version: String::new(),
            },
            count: 2,
            status: Status::Pending,
            key: "k".into(),
            created: 0,
            attachments: vec![],
            privacy: Privacy::default(),
            server_id: None,
            server_status: None,
            attempts: 0,
            retries: 0,
            next_try: 0,
            claimed_until: 0,
            last_error: None,
        }
    }

    #[test]
    fn text_only_payload() {
        let p = build(&report(), &PayloadParts::default()).unwrap();
        let v: Value = serde_json::from_str(&p).unwrap();
        assert_eq!(v["protocol"], 1);
        assert_eq!(v["client_id"], "fb-1");
        assert_eq!(v["kind"], "bug");
        assert_eq!(v["severity"], "high");
        assert_eq!(v["origin"], "agent");
        assert_eq!(v["count"], 2);
        assert_eq!(v["tried"], "tr");
        assert!(v.get("wanted").is_none());
        assert_eq!(v["context"]["client_name"], "c");
        assert_eq!(v["attachments"], json!({}));
        check(&p, "fb-1").unwrap();
        assert!(check(&p, "fb-2").is_err());
        assert!(check("{", "fb-1").is_err());
        assert!(check(&"x".repeat(MAX_PAYLOAD_BYTES + 1), "fb-1").is_err());
    }

    #[test]
    fn payload_with_attachments() {
        let png = [0x89, b'P', b'N', b'G'];
        let parts = PayloadParts {
            excerpt: Some(r#"{"layers":[]}"#),
            screenshot: Some(&png),
            render: Some(&png),
            calls: Some(r#"[{"method":"a"}]"#),
            log: Some("line"),
            installation: Some("0190a1b2-c3d4-4e5f-8a6b-0123456789ab"),
        };
        let v: Value = serde_json::from_str(&build(&report(), &parts).unwrap()).unwrap();
        let a = &v["attachments"];
        assert_eq!(a["excerpt"]["format"], "kad");
        assert_eq!(a["excerpt"]["document"], json!({"layers": []}));
        assert_eq!(a["screenshot"]["mime"], "image/png");
        assert_eq!(a["screenshot"]["base64"], "iVBORw==");
        assert_eq!(a["render"]["base64"], "iVBORw==");
        assert_eq!(a["calls"][0]["method"], "a");
        assert_eq!(a["log"], "line");
        assert_eq!(v["installation"], "0190a1b2-c3d4-4e5f-8a6b-0123456789ab");
    }

    #[test]
    fn bad_parts_and_big_messages_fail() {
        let bad = PayloadParts {
            excerpt: Some("{"),
            ..Default::default()
        };
        assert!(build(&report(), &bad).is_err());
        let bad = PayloadParts {
            calls: Some("["),
            ..Default::default()
        };
        assert!(build(&report(), &bad).is_err());
        let big = vec![0u8; MAX_PAYLOAD_BYTES];
        let parts = PayloadParts {
            screenshot: Some(&big),
            ..Default::default()
        };
        assert!(build(&report(), &parts).is_err());
    }
}
