//! The report and its parts, and the limits that bound all of them.

use serde::{Deserialize, Serialize};

pub const MAX_TITLE: usize = 120;
pub const MAX_BODY: usize = 4000;
pub const MAX_WANTED: usize = 2000;
pub const MAX_TRIED: usize = 2000;
pub const MAX_METHOD: usize = 80;
pub const MAX_ERROR: usize = 2000;
pub const MAX_CLIENT: usize = 60;
/// Reports in the outbox. When it is full, the oldest sent or withdrawn report goes.
pub const MAX_REPORTS: usize = 200;
/// The same outbox has room for 100 reports of agents and 100 of people, so agents cannot fill
/// the space of a person.
pub const MAX_REPORTS_PER_SIDE: usize = MAX_REPORTS / 2;
/// A screenshot or a render, as PNG.
pub const MAX_IMAGE_BYTES: usize = 1 << 20;
/// A model excerpt, as a JSON document.
pub const MAX_EXCERPT_BYTES: usize = 512 << 10;
/// Objects in a model excerpt.
pub const MAX_EXCERPT_OBJECTS: usize = 200;
/// The call log: entries and bytes.
pub const MAX_CALLS: usize = 40;
pub const MAX_CALLS_BYTES: usize = 32 << 10;
/// The log lines: lines and bytes.
pub const MAX_LOG_LINES: usize = 50;
pub const MAX_LOG_BYTES: usize = 16 << 10;
/// The whole message to the server.
pub const MAX_PAYLOAD_BYTES: usize = 6 << 20;
/// The outbox file.
pub const MAX_OUTBOX_BYTES: usize = 2 << 20;
/// Tries over all runs; then the report waits for a human to try again.
pub const MAX_ATTEMPTS: u32 = 8;
/// A report that a program is sending is claimed on disk for this long (seconds).
pub const CLAIM_SECS: u64 = 300;
/// The back-off between starts: one minute, doubling, at most six hours (seconds).
pub const BACKOFF_BASE_SECS: u64 = 60;
pub const BACKOFF_MAX_SECS: u64 = 6 * 3600;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Bug,
    FeatureRequest,
    Question,
}

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Kind::Bug => "bug",
            Kind::FeatureRequest => "feature_request",
            Kind::Question => "question",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Low,
    Medium,
    High,
}

/// Who wrote the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Agent,
    User,
    Crash,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Text only. It goes to the server when sending is on.
    Pending,
    /// It has attachments. It waits for a human, who reviews it. Only [`crate::Hub::approve`]
    /// leaves this status, and no API method calls that.
    NeedsReview,
    /// A human approved it. The exact payload is stored. It goes to the server.
    Approved,
    Sent,
    Withdrawn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    /// A small `.kad` document: the objects with what they need.
    Excerpt,
    /// A picture of the app window.
    Screenshot,
    /// A rendered picture of the current view.
    Render,
    /// The recent API calls: method and scrubbed parameters.
    Calls,
    /// The recent log lines, scrubbed.
    Log,
}

impl AttachmentKind {
    pub const ALL: [AttachmentKind; 5] = [
        AttachmentKind::Excerpt,
        AttachmentKind::Screenshot,
        AttachmentKind::Render,
        AttachmentKind::Calls,
        AttachmentKind::Log,
    ];

    pub fn id(self) -> &'static str {
        match self {
            AttachmentKind::Excerpt => "excerpt",
            AttachmentKind::Screenshot => "screenshot",
            AttachmentKind::Render => "render",
            AttachmentKind::Calls => "calls",
            AttachmentKind::Log => "log",
        }
    }

    /// The largest size of this kind in bytes.
    pub fn limit(self) -> usize {
        match self {
            AttachmentKind::Excerpt => MAX_EXCERPT_BYTES,
            AttachmentKind::Screenshot | AttachmentKind::Render => MAX_IMAGE_BYTES,
            AttachmentKind::Calls => MAX_CALLS_BYTES,
            AttachmentKind::Log => MAX_LOG_BYTES,
        }
    }

    /// The name of the stored file of report `id`.
    pub fn file(self, id: &str) -> String {
        let ext = match self {
            AttachmentKind::Excerpt => "kad.json",
            AttachmentKind::Screenshot | AttachmentKind::Render => "png",
            AttachmentKind::Calls => "json",
            AttachmentKind::Log => "txt",
        };
        format!("{id}.{}.{ext}", self.id())
    }
}

/// What the outbox knows about a stored attachment. The bytes are in the store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentMeta {
    pub kind: AttachmentKind,
    pub bytes: usize,
    /// Objects in an excerpt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objects: Option<usize>,
}

/// An attachment on its way into the outbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentData {
    pub kind: AttachmentKind,
    pub bytes: Vec<u8>,
    pub objects: Option<usize>,
}

/// What Kadakas adds to a report by itself.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    pub version: String,
    pub os: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub client_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub client_version: String,
}

/// What to remove from a model excerpt before it is sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Privacy {
    /// Move the objects so that their center is at (0, 0).
    pub move_to_origin: bool,
    /// Remove the georeference.
    pub remove_georef: bool,
    /// Remove names, notes and record data.
    pub remove_names: bool,
}

impl Default for Privacy {
    /// A landscape model shows a real place, so the place goes by default.
    fn default() -> Self {
        Privacy {
            move_to_origin: true,
            remove_georef: true,
            remove_names: false,
        }
    }
}

/// The input of a new report. The texts need no scrubbing: the outbox does that.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub kind: Kind,
    pub title: String,
    pub body: String,
    pub wanted: String,
    pub tried: String,
    pub method: String,
    pub error: String,
    pub severity: Option<Severity>,
    pub origin: Origin,
    pub context: Context,
}

impl Draft {
    pub fn new(kind: Kind, title: &str, body: &str, origin: Origin) -> Draft {
        Draft {
            kind,
            title: title.into(),
            body: body.into(),
            wanted: String::new(),
            tried: String::new(),
            method: String::new(),
            error: String::new(),
            severity: None,
            origin,
            context: Context::default(),
        }
    }
}

/// A report in the outbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// The local id (`fb-12`). It is also the key against double delivery at the server.
    pub id: String,
    /// 16 random hex characters made with the report. They keep `client_id` unique when the
    /// outbox restarts at `fb-1`. Empty in reports from older outboxes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub nonce: String,
    pub kind: Kind,
    pub title: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub wanted: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tried: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub method: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    pub origin: Origin,
    pub context: Context,
    /// How often the same report came (see [`dedupe_key`]).
    pub count: u32,
    pub status: Status,
    pub key: String,
    /// Seconds since 1970, from the caller. Zero if it has no clock.
    #[serde(default)]
    pub created: u64,
    pub attachments: Vec<AttachmentMeta>,
    #[serde(default)]
    pub privacy: Privacy,
    /// The id that the server gave, and the status it reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_status: Option<String>,
    /// Failed starts that count toward `MAX_ATTEMPTS`. A failure to reach the server does not count.
    #[serde(default)]
    pub attempts: u32,
    /// All failed starts, for the back-off.
    #[serde(default)]
    pub retries: u32,
    /// Seconds since 1970: the earliest time of the next try.
    #[serde(default)]
    pub next_try: u64,
    /// Seconds since 1970: another program is sending this report until then.
    #[serde(default)]
    pub claimed_until: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl Report {
    /// The id on the wire: `fb-{seq}-{nonce}`, or just the local id for a report without a nonce.
    /// The server keys a report on (installation, `client_id`).
    pub fn client_id(&self) -> String {
        if self.nonce.is_empty() {
            self.id.clone()
        } else {
            format!("{}-{}", self.id, self.nonce)
        }
    }

    pub fn attachment(&self, kind: AttachmentKind) -> Option<&AttachmentMeta> {
        self.attachments.iter().find(|a| a.kind == kind)
    }
}

/// Words of a title in lower case, joined by one space, so small differences do not matter.
pub fn normalize_title(title: &str) -> String {
    let mut out = String::new();
    for w in title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(w.chars().flat_map(char::to_lowercase));
    }
    out
}

/// The key that merges equal reports: kind, normalized title and method.
pub fn dedupe_key(kind: Kind, title: &str, method: &str) -> String {
    format!(
        "{}|{}|{}",
        kind.id(),
        normalize_title(title),
        method.trim().to_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_ignore_case_and_punctuation() {
        let a = dedupe_key(Kind::Bug, "Hatch  fails: on ARCS!", "edit.fillet");
        let b = dedupe_key(Kind::Bug, "hatch fails on arcs", "Edit.Fillet ");
        assert_eq!(a, b);
        assert_ne!(
            a,
            dedupe_key(Kind::FeatureRequest, "hatch fails on arcs", "edit.fillet")
        );
        assert_ne!(a, dedupe_key(Kind::Bug, "hatch fails on arcs", ""));
    }

    #[test]
    fn privacy_default_hides_the_place() {
        let p = Privacy::default();
        assert!(p.move_to_origin && p.remove_georef && !p.remove_names);
    }

    #[test]
    fn attachment_files_are_safe_names() {
        for k in AttachmentKind::ALL {
            let f = k.file("fb-3");
            assert!(
                f.starts_with("fb-3.")
                    && f.chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
            );
            assert!(k.limit() > 0 && !k.id().is_empty());
        }
        assert_eq!(Kind::Question.id(), "question");
    }
}
