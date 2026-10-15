//! Review issues (REV-04): the data model, in the terms of BCF (the BIM Collaboration Format).
//!
//! An issue is review data. It is not a drawing object: it has no layer, class or shape, and it is
//! not in the object list. The document keeps issues in `Document::issues`. The pieces:
//!
//! - [`Issue`]: title, description, type, [`IssueStatus`], priority, assignee, labels, due date,
//!   author, origin, times, a point, the objects it is about (by ID) and its [`Viewpoint`]s.
//! - [`Viewpoint`]: a view (plan or camera), the visible layers and classes, a selection and the
//!   hash of a PNG snapshot. The snapshot is a picture file. It is stored as an `image` resource of
//!   the document (content addressed by SHA-256, as all pictures are), so the file format keeps it
//!   once, and collaboration sends it once as a blob.
//! - [`Comment`]: one message of the thread of an issue. A comment is a record of its own with an
//!   ID, not a field of the issue, so two people who comment at the same time both keep their
//!   comment.
//!
//! All numbers are finite, and all texts and lists have a bound (the `MAX_*` constants). A command
//! that would break a bound is refused with a message. Nothing is cut silently.
//!
//! The commands that change issues are in `crate::command` (`AddIssue`, `ModifyIssue`, ...). The
//! BCF files and the filters are in `kadakas-review`.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::change::Origin;
use crate::model::{ClassId, LayerId, ObjectId};
use crate::uid::{Uid, id_type};

id_type!(
    /// Identifier of an issue. A BCF topic GUID is the same ID.
    IssueId
);
id_type!(
    /// Identifier of a comment.
    CommentId
);
id_type!(
    /// Identifier of a viewpoint of an issue.
    ViewpointId
);

/// The most issues in a document.
pub const MAX_ISSUES: usize = 10_000;
/// The most comments in a document, over all issues.
pub const MAX_COMMENTS_TOTAL: usize = 100_000;
/// The most comments of one issue.
pub const MAX_COMMENTS: usize = 1_000;
/// The most viewpoints of one issue.
pub const MAX_VIEWPOINTS: usize = 16;
/// The longest title, in characters.
pub const MAX_TITLE_CHARS: usize = 200;
/// The longest description or comment text, in characters.
pub const MAX_TEXT_CHARS: usize = 20_000;
/// The longest label, in characters.
pub const MAX_LABEL_CHARS: usize = 64;
/// The most labels of one issue.
pub const MAX_LABELS: usize = 32;
/// The longest type, priority, assignee, author or viewpoint name, in characters.
pub const MAX_NAME_CHARS: usize = 120;
/// The most objects that an issue or a viewpoint refers to.
pub const MAX_OBJECTS: usize = 5_000;
/// The most layers or classes in the visibility of a viewpoint.
pub const MAX_VISIBILITY: usize = 5_000;
/// The largest snapshot (a PNG file), in bytes.
pub const MAX_SNAPSHOT_BYTES: usize = 2 * 1024 * 1024;
/// The largest snapshot, in pixels (width times height).
pub const MAX_SNAPSHOT_PIXELS: u64 = 4_000_000;
/// The source name of the picture resource of a snapshot.
pub const SNAPSHOT_SOURCE: &str = "issue-snapshot.png";
/// The type of an issue when nobody gives one.
pub const DEFAULT_TYPE: &str = "issue";
/// The priority of an issue when nobody gives one.
pub const DEFAULT_PRIORITY: &str = "normal";
/// The priorities that the app offers. A priority can be another text (a BCF file has free text).
pub const PRIORITIES: [&str; 4] = ["low", "normal", "high", "critical"];
/// The types that the app offers. A type can be another text.
pub const TYPES: [&str; 6] = ["issue", "error", "warning", "request", "clash", "remark"];

/// The state of an issue.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[derive(JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IssueStatus {
    #[default]
    Open,
    InProgress,
    Resolved,
    Closed,
}

impl IssueStatus {
    pub const ALL: [IssueStatus; 4] = [
        IssueStatus::Open,
        IssueStatus::InProgress,
        IssueStatus::Resolved,
        IssueStatus::Closed,
    ];

    /// The name in the API and in files.
    pub fn name(self) -> &'static str {
        match self {
            IssueStatus::Open => "open",
            IssueStatus::InProgress => "in_progress",
            IssueStatus::Resolved => "resolved",
            IssueStatus::Closed => "closed",
        }
    }

    pub fn parse(text: &str) -> Option<IssueStatus> {
        IssueStatus::ALL.into_iter().find(|s| s.name() == text)
    }

    /// True for an issue that is done with: resolved or closed.
    pub fn is_done(self) -> bool {
        matches!(self, IssueStatus::Resolved | IssueStatus::Closed)
    }
}

/// What a viewpoint looks at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewSpec {
    /// The 2D plan: the model point at the middle of the view and the zoom in pixels per mm.
    Plan { center: [f64; 2], zoom: f64 },
    /// The 3D view: an orbit camera as in the 3D window. Angles are radians, lengths are mm.
    Camera {
        target: [f64; 3],
        yaw: f64,
        pitch: f64,
        distance: f64,
        #[serde(default)]
        perspective: bool,
    },
}

impl ViewSpec {
    fn check(&self) -> Result<(), String> {
        let finite = |v: &[f64]| v.iter().all(|x| x.is_finite());
        let lim = crate::model::MAX_COORD;
        match self {
            ViewSpec::Plan { center, zoom } => {
                if !finite(center) || !zoom.is_finite() || *zoom <= 0.0 {
                    return Err("a plan view needs a finite center and a zoom above 0".into());
                }
                if center.iter().any(|c| c.abs() > lim) {
                    return Err("the plan view is out of range".into());
                }
            }
            ViewSpec::Camera {
                target,
                yaw,
                pitch,
                distance,
                ..
            } => {
                if !finite(target) || !yaw.is_finite() || !pitch.is_finite() {
                    return Err("a camera needs finite numbers".into());
                }
                if !distance.is_finite() || *distance <= 0.0 || *distance > lim {
                    return Err("the camera distance must be above 0 and in range".into());
                }
                if target.iter().any(|c| c.abs() > lim) {
                    return Err("the camera target is out of range".into());
                }
            }
        }
        Ok(())
    }
}

/// A saved view of an issue.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Viewpoint {
    /// The ID. A nil ID in a command means "make a new one".
    #[serde(default)]
    pub id: ViewpointId,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// When it was made, in milliseconds since 1970 (UTC).
    #[serde(default)]
    pub time: i64,
    /// The camera or the plan view. Without one, the viewpoint holds a selection only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<ViewSpec>,
    /// Visibility of layers when the viewpoint was made (`true`: visible). Layers that are not
    /// listed stay as they are.
    #[serde(default, with = "crate::sheets::id_map", skip_serializing_if = "BTreeMap::is_empty")]
    #[schemars(with = "Vec<(LayerId, bool)>")]
    pub layers: BTreeMap<LayerId, bool>,
    /// Visibility of classes when the viewpoint was made.
    #[serde(default, with = "crate::sheets::id_map", skip_serializing_if = "BTreeMap::is_empty")]
    #[schemars(with = "Vec<(ClassId, bool)>")]
    pub classes: BTreeMap<ClassId, bool>,
    /// The selected objects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selection: Vec<ObjectId>,
    /// The SHA-256 of the PNG snapshot, as 64 lowercase hex digits. The file is the `image`
    /// resource of the document with this hash (`Resources::find_image`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
}

impl Viewpoint {
    fn check(&self) -> Result<(), String> {
        if chars(&self.name) > MAX_NAME_CHARS {
            return Err(format!(
                "a viewpoint name has at most {MAX_NAME_CHARS} characters"
            ));
        }
        if let Some(v) = &self.view {
            v.check()?;
        }
        if self.layers.len() > MAX_VISIBILITY || self.classes.len() > MAX_VISIBILITY {
            return Err(format!(
                "a viewpoint lists at most {MAX_VISIBILITY} layers and {MAX_VISIBILITY} classes"
            ));
        }
        if self.selection.len() > MAX_OBJECTS {
            return Err(format!(
                "a viewpoint selects at most {MAX_OBJECTS} objects"
            ));
        }
        if let Some(h) = &self.snapshot
            && !is_hash(h)
        {
            return Err("a snapshot is named by the SHA-256 of its file (64 hex digits)".into());
        }
        Ok(())
    }
}

/// A review issue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Issue {
    /// The ID. A nil ID in a command means "make a new one".
    #[serde(default)]
    pub id: IssueId,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The type, free text. The app offers `issue`, `error`, `warning`, `request`, `clash` and
    /// `remark`.
    #[serde(rename = "type", default = "default_type")]
    pub kind: String,
    #[serde(default)]
    pub status: IssueStatus,
    /// The priority, free text. The app offers `low`, `normal`, `high` and `critical`.
    #[serde(default = "default_priority")]
    pub priority: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// The due date, `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    #[serde(default)]
    pub author: String,
    /// Who made it: a person in the app, an agent, a script or another user.
    #[serde(default)]
    pub origin: Origin,
    /// Milliseconds since 1970 (UTC).
    #[serde(default)]
    pub created: i64,
    /// Milliseconds since 1970 (UTC). Every change sets it.
    #[serde(default)]
    pub modified: i64,
    /// The model point (x, y, z in mm) that the issue is about, for the pin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<[f64; 3]>,
    /// The objects that the issue is about.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<ObjectId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub viewpoints: Vec<Viewpoint>,
}

fn default_type() -> String {
    DEFAULT_TYPE.into()
}

fn default_priority() -> String {
    DEFAULT_PRIORITY.into()
}

impl Default for Issue {
    fn default() -> Self {
        Issue {
            id: IssueId::default(),
            title: String::new(),
            description: String::new(),
            kind: default_type(),
            status: IssueStatus::Open,
            priority: default_priority(),
            assignee: None,
            labels: Vec::new(),
            due: None,
            author: String::new(),
            origin: Origin::User,
            created: 0,
            modified: 0,
            point: None,
            objects: Vec::new(),
            viewpoints: Vec::new(),
        }
    }
}

/// One message in the thread of an issue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Comment {
    /// The ID. A nil ID in a command means "make a new one".
    #[serde(default)]
    pub id: CommentId,
    pub issue: IssueId,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub origin: Origin,
    /// Milliseconds since 1970 (UTC).
    #[serde(default)]
    pub time: i64,
    pub text: String,
    /// The viewpoint of the issue that the comment refers to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewpoint: Option<ViewpointId>,
    /// When the text was edited last (milliseconds since 1970).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited: Option<i64>,
}

/// The fields of an issue that `ModifyIssue` can change. A field that is left out stays. For
/// `assignee` and `due`, an empty text clears the field.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssuePatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub status: Option<IssueStatus>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub labels: Option<Vec<String>>,
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub point: Option<[f64; 3]>,
    /// True removes the point.
    #[serde(default)]
    pub clear_point: bool,
    #[serde(default)]
    pub objects: Option<Vec<ObjectId>>,
}

impl IssuePatch {
    /// The issue with the patch applied (not yet checked).
    pub fn apply(&self, issue: &Issue) -> Issue {
        let mut i = issue.clone();
        if let Some(t) = &self.title {
            i.title = t.clone();
        }
        if let Some(t) = &self.description {
            i.description = t.clone();
        }
        if let Some(t) = &self.kind {
            i.kind = t.clone();
        }
        if let Some(t) = self.status {
            i.status = t;
        }
        if let Some(t) = &self.priority {
            i.priority = t.clone();
        }
        if let Some(t) = &self.assignee {
            i.assignee = (!t.is_empty()).then(|| t.clone());
        }
        if let Some(t) = &self.labels {
            i.labels = t.clone();
        }
        if let Some(t) = &self.due {
            i.due = (!t.is_empty()).then(|| t.clone());
        }
        if self.clear_point {
            i.point = None;
        }
        if let Some(p) = self.point {
            i.point = Some(p);
        }
        if let Some(o) = &self.objects {
            i.objects = o.clone();
        }
        i
    }
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

/// True for 64 lowercase hex digits.
pub fn is_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// True for a date `YYYY-MM-DD` that exists in the calendar.
pub fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<u32>().ok();
    let (Some(y), Some(m), Some(d)) = (num(0..4), num(5..7), num(8..10)) else {
        return false;
    };
    if y < 1 || !(1..=12).contains(&m) || d < 1 {
        return false;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = match m {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    d <= days
}

/// A time is a number of milliseconds since 1970. The range keeps every calendar function exact.
const MAX_TIME_MS: i64 = 253_402_300_799_000; // 9999-12-31

fn check_time(name: &str, t: i64) -> Result<(), String> {
    if (0..=MAX_TIME_MS).contains(&t) {
        Ok(())
    } else {
        Err(format!("{name} must be a time between 1970 and 9999"))
    }
}

fn check_text(what: &str, s: &str, max: usize) -> Result<(), String> {
    if chars(s) > max {
        return Err(format!("{what} has at most {max} characters"));
    }
    if s.chars().any(|c| c == '\0') {
        return Err(format!("{what} cannot hold a NUL character"));
    }
    Ok(())
}

impl Issue {
    /// Checks the bounds and the values. A command runs it before it changes the document.
    pub fn validate(&self) -> Result<(), String> {
        if self.title.trim().is_empty() {
            return Err("an issue needs a title".into());
        }
        check_text("a title", &self.title, MAX_TITLE_CHARS)?;
        check_text("a description", &self.description, MAX_TEXT_CHARS)?;
        for (what, text) in [("a type", &self.kind), ("a priority", &self.priority)] {
            if text.trim().is_empty() {
                return Err(format!("{what} cannot be empty"));
            }
            check_text(what, text, MAX_NAME_CHARS)?;
        }
        for (what, text) in [
            ("an assignee", self.assignee.as_deref()),
            ("an author", Some(self.author.as_str())),
        ] {
            if let Some(t) = text {
                check_text(what, t, MAX_NAME_CHARS)?;
            }
        }
        if self.assignee.as_deref().is_some_and(|a| a.trim().is_empty()) {
            return Err("an assignee cannot be empty: leave it out".into());
        }
        if self.labels.len() > MAX_LABELS {
            return Err(format!("an issue has at most {MAX_LABELS} labels"));
        }
        let mut seen = HashSet::new();
        for l in &self.labels {
            if l.trim().is_empty() {
                return Err("a label cannot be empty".into());
            }
            check_text("a label", l, MAX_LABEL_CHARS)?;
            if !seen.insert(l.to_lowercase()) {
                return Err(format!("the label '{l}' is there twice"));
            }
        }
        if let Some(d) = &self.due
            && !is_date(d)
        {
            return Err(format!("the due date '{d}' is not a date YYYY-MM-DD"));
        }
        check_time("the creation time", self.created)?;
        check_time("the change time", self.modified)?;
        if let Some(p) = &self.point
            && !(p.iter().all(|x| x.is_finite())
                && p.iter().all(|x| x.abs() <= crate::model::MAX_COORD))
        {
            return Err("the point must be finite and in range".into());
        }
        if self.objects.len() > MAX_OBJECTS {
            return Err(format!("an issue refers to at most {MAX_OBJECTS} objects"));
        }
        let mut seen = HashSet::new();
        if !self.objects.iter().all(|o| seen.insert(*o)) {
            return Err("an object is listed twice".into());
        }
        if self.viewpoints.len() > MAX_VIEWPOINTS {
            return Err(format!("an issue has at most {MAX_VIEWPOINTS} viewpoints"));
        }
        let mut seen = HashSet::new();
        for v in &self.viewpoints {
            v.check()?;
            check_time("the viewpoint time", v.time)?;
            if !seen.insert(v.id) {
                return Err("two viewpoints have the same ID".into());
            }
        }
        Ok(())
    }
}

impl Comment {
    pub fn validate(&self) -> Result<(), String> {
        if self.text.trim().is_empty() {
            return Err("a comment needs a text".into());
        }
        check_text("a comment", &self.text, MAX_TEXT_CHARS)?;
        check_text("an author", &self.author, MAX_NAME_CHARS)?;
        check_time("the comment time", self.time)?;
        if let Some(e) = self.edited {
            check_time("the edit time", e)?;
        }
        Ok(())
    }
}

/// The issues and comments of a document.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IssueData {
    #[serde(default)]
    pub issues: Vec<Issue>,
    #[serde(default)]
    pub comments: Vec<Comment>,
}

/// The review data of a document. A clone costs a pointer: the undo steps and the change log
/// share the data, and a command that changes it copies it once.
#[derive(Clone, Debug, Default)]
pub struct Issues(Arc<IssueData>);

impl PartialEq for Issues {
    fn eq(&self, other: &Issues) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

impl Serialize for Issues {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Issues {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Issues, D::Error> {
        IssueData::deserialize(d).map(|data| Issues(Arc::new(data)))
    }
}

impl From<IssueData> for Issues {
    fn from(d: IssueData) -> Issues {
        Issues(Arc::new(d))
    }
}

impl Issues {
    pub fn is_empty(&self) -> bool {
        self.0.issues.is_empty() && self.0.comments.is_empty()
    }

    pub fn data(&self) -> &IssueData {
        &self.0
    }

    /// The data for a change. It is copied first if the undo steps share it.
    pub fn data_mut(&mut self) -> &mut IssueData {
        Arc::make_mut(&mut self.0)
    }

    pub fn len(&self) -> usize {
        self.0.issues.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Issue> {
        self.0.issues.iter()
    }

    pub fn get(&self, id: IssueId) -> Option<&Issue> {
        self.0.issues.iter().find(|i| i.id == id)
    }

    pub fn comment(&self, id: CommentId) -> Option<&Comment> {
        self.0.comments.iter().find(|c| c.id == id)
    }

    /// The comments of an issue, oldest first (by time, then ID).
    pub fn comments_of(&self, id: IssueId) -> Vec<&Comment> {
        let mut v: Vec<&Comment> = self.0.comments.iter().filter(|c| c.issue == id).collect();
        v.sort_by_key(|c| (c.time, c.id));
        v
    }

    pub fn comment_count(&self, id: IssueId) -> usize {
        self.0.comments.iter().filter(|c| c.issue == id).count()
    }

    /// True if some viewpoint refers to the snapshot with this hash.
    pub fn uses_snapshot(&self, hash: &str) -> bool {
        self.0
            .issues
            .iter()
            .any(|i| i.viewpoints.iter().any(|v| v.snapshot.as_deref() == Some(hash)))
    }

    /// The hashes of all snapshots that the issues refer to.
    pub fn snapshots(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self
            .0
            .issues
            .iter()
            .flat_map(|i| i.viewpoints.iter())
            .filter_map(|vp| vp.snapshot.as_deref())
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// A rough size in bytes, for the budget of the undo steps.
    pub fn approx_bytes(&self) -> u64 {
        let issues: usize = self
            .0
            .issues
            .iter()
            .map(|i| {
                256 + i.title.len()
                    + i.description.len()
                    + i.labels.iter().map(String::len).sum::<usize>()
                    + i.objects.len() * 40
                    + i.viewpoints
                        .iter()
                        .map(|v| 200 + v.selection.len() * 40 + (v.layers.len() + v.classes.len()) * 48)
                        .sum::<usize>()
            })
            .sum();
        let comments: usize = self.0.comments.iter().map(|c| 160 + c.text.len()).sum();
        (issues + comments) as u64
    }

    /// Checks what a file holds and repairs what cannot stay: a second issue or comment with the
    /// same ID and a comment of an issue that is not there are left out. Returns a note for each
    /// repair, so that the person is told. Values that break a bound stay (a file must open): the
    /// next command that changes them is refused until they fit.
    pub fn repair(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        let data = &self.0;
        let mut seen = HashSet::new();
        let dup_issues = data.issues.iter().filter(|i| !seen.insert(i.id)).count();
        let ids: HashSet<IssueId> = data.issues.iter().map(|i| i.id).collect();
        let mut cseen = HashSet::new();
        let bad_comments = data
            .comments
            .iter()
            .filter(|c| !cseen.insert(c.id) || !ids.contains(&c.issue))
            .count();
        if dup_issues > 0 || bad_comments > 0 {
            let d = self.data_mut();
            let mut seen = HashSet::new();
            d.issues.retain(|i| seen.insert(i.id));
            let ids: HashSet<IssueId> = d.issues.iter().map(|i| i.id).collect();
            let mut cseen = HashSet::new();
            d.comments
                .retain(|c| cseen.insert(c.id) && ids.contains(&c.issue));
            if dup_issues > 0 {
                notes.push(format!(
                    "{dup_issues} issue(s) with an ID that was used before were left out"
                ));
            }
            if bad_comments > 0 {
                notes.push(format!(
                    "{bad_comments} comment(s) of an issue that is not there, or with a repeated ID, were left out"
                ));
            }
        }
        let mut too_big = 0;
        if self.0.issues.len() > MAX_ISSUES || self.0.comments.len() > MAX_COMMENTS_TOTAL {
            too_big += 1;
        }
        too_big += self.0.issues.iter().filter(|i| i.validate().is_err()).count();
        if too_big > 0 {
            notes.push(format!(
                "{too_big} issue(s) do not fit the limits of issues; they are kept as they are"
            ));
        }
        notes
    }
}

/// The ID that a command gets for a new issue, comment or viewpoint: the nil ID means "make one".
pub fn is_unset(id: Uid) -> bool {
    id == Uid::NIL
}

#[cfg(test)]
mod tests;
