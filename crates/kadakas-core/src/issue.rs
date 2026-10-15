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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::change::Origin;
use crate::model::{ClassId, LayerId, ObjectId};
use crate::uid::{Uid, id_type};

mod shared;
pub use shared::Shared;

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
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    JsonSchema,
)]
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
    #[serde(
        default,
        with = "crate::sheets::id_map",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    #[schemars(with = "Vec<(LayerId, bool)>")]
    pub layers: BTreeMap<LayerId, bool>,
    /// Visibility of classes when the viewpoint was made.
    #[serde(
        default,
        with = "crate::sheets::id_map",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
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
    /// Checks the bounds and the values of a viewpoint.
    pub fn validate(&self) -> Result<(), String> {
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
            return Err(format!("a viewpoint selects at most {MAX_OBJECTS} objects"));
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
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
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
        if self
            .assignee
            .as_deref()
            .is_some_and(|a| a.trim().is_empty())
        {
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
            v.validate()?;
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

/// The issues and comments of a document. The lists are in order (issues by creation time, then
/// ID; comments by time, then ID). They are [`Shared`] lists: a copy shares its parts with the
/// original, so an undo step keeps a few kilobytes and not the whole list.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IssueData {
    #[serde(default)]
    pub issues: Shared<Issue>,
    #[serde(default)]
    pub comments: Shared<Comment>,
}

impl IssueData {
    /// The data of two lists that are in order (issues by creation time, then ID; comments by
    /// time, then ID). A host that builds data (a loader, a benchmark) uses this.
    pub fn from_vecs(issues: Vec<Issue>, comments: Vec<Comment>) -> IssueData {
        IssueData {
            issues: Shared::from_vec(issues),
            comments: Shared::from_vec(comments),
        }
    }
}

/// What is looked up often and costs a walk over the lists. Each part is made on its first use
/// for one state of the data, and a change of the data drops them all.
#[derive(Debug, Default)]
struct Index {
    /// The place (see [`Shared::places`]) of each issue.
    issue_at: OnceLock<HashMap<IssueId, (u32, u32)>>,
    /// The place of each comment.
    comment_at: OnceLock<HashMap<CommentId, (u32, u32)>>,
    /// For each issue the places of its comments, oldest first.
    comments: OnceLock<HashMap<IssueId, Vec<(u32, u32)>>>,
}

/// The review data of a document. A clone costs two pointers: the undo steps and the change log
/// share the data, and a command that changes it copies only the parts that change.
#[derive(Debug, Default)]
pub struct Issues {
    data: Arc<IssueData>,
    index: Arc<Index>,
}

impl Clone for Issues {
    fn clone(&self) -> Issues {
        Issues {
            data: self.data.clone(),
            index: self.index.clone(),
        }
    }
}

impl PartialEq for Issues {
    fn eq(&self, other: &Issues) -> bool {
        Arc::ptr_eq(&self.data, &other.data) || self.data == other.data
    }
}

impl Serialize for Issues {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.data.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Issues {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Issues, D::Error> {
        IssueData::deserialize(d).map(Issues::from)
    }
}

impl From<IssueData> for Issues {
    fn from(d: IssueData) -> Issues {
        Issues {
            data: Arc::new(d),
            index: Arc::default(),
        }
    }
}

impl Issues {
    pub fn is_empty(&self) -> bool {
        self.data.issues.is_empty() && self.data.comments.is_empty()
    }

    pub fn data(&self) -> &IssueData {
        &self.data
    }

    /// The same data without the index. An undo step keeps this, so that a step holds no index
    /// (which can be large) of a state that nobody reads any more.
    pub fn lean(&self) -> Issues {
        Issues {
            data: self.data.clone(),
            index: Arc::default(),
        }
    }

    fn comments_by_issue(&self) -> &HashMap<IssueId, Vec<(u32, u32)>> {
        self.index.comments.get_or_init(|| {
            let mut comments: HashMap<IssueId, Vec<(u32, u32)>> = HashMap::new();
            for (place, c) in self.data.comments.places() {
                comments.entry(c.issue).or_default().push(place);
            }
            comments
        })
    }

    /// Puts an issue where it belongs: the list is sorted by creation time, then by ID, so two
    /// documents with the same issues have them in the same order.
    pub fn insert_issue(&mut self, mut issue: Issue) {
        issue.viewpoints.sort_by_key(|v| (v.time, v.id));
        let d = self.data_mut();
        let key = (issue.created, issue.id);
        let at = d.issues.partition_point(|i| (i.created, i.id) <= key);
        d.issues.insert(at, issue);
    }

    /// Puts a comment where it belongs: by time, then by ID.
    pub fn insert_comment(&mut self, c: Comment) {
        let d = self.data_mut();
        let key = (c.time, c.id);
        let at = d.comments.partition_point(|x| (x.time, x.id) <= key);
        d.comments.insert(at, c);
    }

    /// The data for a change. The parts that the undo steps share are copied when they change.
    pub fn data_mut(&mut self) -> &mut IssueData {
        // The index is of the state before the change.
        self.index = Arc::default();
        Arc::make_mut(&mut self.data)
    }

    pub fn len(&self) -> usize {
        self.data.issues.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Issue> {
        self.data.issues.iter()
    }

    pub fn get(&self, id: IssueId) -> Option<&Issue> {
        let at = self.index.issue_at.get_or_init(|| {
            self.data
                .issues
                .places()
                .map(|(place, i)| (i.id, place))
                .collect()
        });
        at.get(&id).map(|p| self.data.issues.at_place(*p))
    }

    pub fn comment(&self, id: CommentId) -> Option<&Comment> {
        let at = self.index.comment_at.get_or_init(|| {
            self.data
                .comments
                .places()
                .map(|(place, c)| (c.id, place))
                .collect()
        });
        at.get(&id).map(|p| self.data.comments.at_place(*p))
    }

    /// The comments of an issue, oldest first (by time, then ID).
    pub fn comments_of(&self, id: IssueId) -> Vec<&Comment> {
        let mut v: Vec<&Comment> = self
            .comments_by_issue()
            .get(&id)
            .map(|places| {
                places
                    .iter()
                    .map(|p| self.data.comments.at_place(*p))
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by_key(|c| (c.time, c.id));
        v
    }

    pub fn comment_count(&self, id: IssueId) -> usize {
        self.comments_by_issue().get(&id).map_or(0, Vec::len)
    }

    /// True if some viewpoint refers to the snapshot with this hash.
    pub fn uses_snapshot(&self, hash: &str) -> bool {
        self.data.issues.iter().any(|i| {
            i.viewpoints
                .iter()
                .any(|v| v.snapshot.as_deref() == Some(hash))
        })
    }

    /// The hashes of all snapshots that the issues refer to.
    pub fn snapshots(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self
            .data
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
        let issues: u64 = self.data.issues.iter().map(issue_bytes).sum();
        let comments: u64 = self.data.comments.iter().map(comment_bytes).sum();
        issues + comments
    }

    /// What an undo step keeps when it holds `before` and `after`: the pointers of the lists, and
    /// the chunks and items that the two states do not share. The rest is shared with the
    /// document (and with the other steps), and is counted once, there.
    pub fn step_bytes(before: &Issues, after: &Issues) -> u64 {
        const POINTER: u64 = 8;
        let (bi, ai) = after.data.issues.unshared(&before.data.issues);
        let (bc, ac) = after.data.comments.unshared(&before.data.comments);
        let items: u64 = bi
            .iter()
            .chain(&ai)
            .map(|i| issue_bytes(i))
            .chain(bc.iter().chain(&ac).map(|c| comment_bytes(c)))
            .sum();
        // The outer lists of both states, and the pointers of the chunks that are not shared.
        let outer = (before.data.issues.chunk_count()
            + after.data.issues.chunk_count()
            + before.data.comments.chunk_count()
            + after.data.comments.chunk_count()) as u64
            * POINTER;
        // A chunk that is not shared is a list of pointers (256 when it is full).
        let (oi, ni) = after.data.issues.unshared_chunks(&before.data.issues);
        let (oc, nc) = after.data.comments.unshared_chunks(&before.data.comments);
        let chunks = (oi + ni + oc + nc) as u64 * 256 * POINTER;
        items + outer + chunks
    }

    /// The issues and the comments that differ between two states: `(in old, in new)` for each of
    /// the two lists. Only the chunks that the states do not share are looked at.
    #[allow(clippy::type_complexity)]
    pub fn unshared<'a>(
        &'a self,
        old: &'a Issues,
    ) -> (
        (Vec<&'a Issue>, Vec<&'a Issue>),
        (Vec<&'a Comment>, Vec<&'a Comment>),
    ) {
        (
            self.data.issues.unshared(&old.data.issues),
            self.data.comments.unshared(&old.data.comments),
        )
    }

    /// Checks what a file holds and repairs what cannot stay as it is. Nothing of the person is
    /// left out: a second issue or comment with a used ID gets a new ID, and comments of an issue
    /// that is not there go to a new issue that says so. Returns a note for each repair, so that
    /// the person is told. Values that break a bound stay (a file must open): the next command
    /// that changes them is refused until they fit.
    pub fn repair(&mut self) -> Vec<String> {
        let mut notes = Vec::new();
        let data = &self.data;
        let mut seen = HashSet::new();
        let dup_issues = data.issues.iter().filter(|i| !seen.insert(i.id)).count();
        let mut cseen = HashSet::new();
        let dup_comments = data.comments.iter().filter(|c| !cseen.insert(c.id)).count();
        let ids: HashSet<IssueId> = data.issues.iter().map(|i| i.id).collect();
        let orphans = data
            .comments
            .iter()
            .filter(|c| !ids.contains(&c.issue))
            .count();
        if dup_issues > 0 || dup_comments > 0 || orphans > 0 {
            let mut issues = data.issues.to_vec();
            let mut comments = data.comments.to_vec();
            // A second issue with an ID gets an ID made from the old one.
            let mut taken: HashSet<IssueId> = issues.iter().map(|i| i.id).collect();
            let mut seen = HashSet::new();
            for i in &mut issues {
                if !seen.insert(i.id) {
                    let old = i.id;
                    let new = (1..)
                        .map(|n| IssueId(Uid::derive(old.0, 20, n)))
                        .find(|u| !taken.contains(u))
                        .expect("an unused ID");
                    taken.insert(new);
                    seen.insert(new);
                    i.id = new;
                }
            }
            let mut ctaken: HashSet<CommentId> = comments.iter().map(|c| c.id).collect();
            let mut cseen = HashSet::new();
            for c in &mut comments {
                if !cseen.insert(c.id) {
                    let old = c.id;
                    let new = (1..)
                        .map(|n| CommentId(Uid::derive(old.0, 21, n)))
                        .find(|u| !ctaken.contains(u))
                        .expect("an unused ID");
                    ctaken.insert(new);
                    cseen.insert(new);
                    c.id = new;
                }
            }
            // Comments of an issue that is not there are kept in an issue of that ID.
            let ids: HashSet<IssueId> = issues.iter().map(|i| i.id).collect();
            let mut lost: Vec<(IssueId, i64, i64)> = Vec::new();
            for c in comments.iter().filter(|c| !ids.contains(&c.issue)) {
                match lost.iter_mut().find(|(id, ..)| *id == c.issue) {
                    Some(l) => {
                        l.1 = l.1.min(c.time);
                        l.2 = l.2.max(c.time);
                    }
                    None => lost.push((c.issue, c.time, c.time)),
                }
            }
            for (id, first, last) in &lost {
                issues.push(Issue {
                    id: *id,
                    title: LOST_ISSUE_TITLE.into(),
                    created: *first,
                    modified: *last,
                    ..Issue::default()
                });
            }
            let d = self.data_mut();
            d.issues = Shared::from_vec(issues);
            d.comments = Shared::from_vec(comments);
            if dup_issues > 0 {
                notes.push(format!(
                    "{dup_issues} issue(s) with an ID that another issue had got new IDs"
                ));
            }
            if dup_comments > 0 {
                notes.push(format!(
                    "{dup_comments} comment(s) with an ID that another comment had got new IDs"
                ));
            }
            if orphans > 0 {
                notes.push(format!(
                    "{orphans} comment(s) belonged to {} issue(s) that are not in the file: the comments are kept in new issues titled \"{LOST_ISSUE_TITLE}\"",
                    lost.len()
                ));
            }
        }
        // The lists keep their order (see `insert_issue`): a file made by hand is put in order.
        let sorted = self
            .data
            .issues
            .windows_all(|a, b| (a.created, a.id) <= (b.created, b.id))
            && self
                .data
                .comments
                .windows_all(|a, b| (a.time, a.id) <= (b.time, b.id))
            && self.data.issues.iter().all(|i| {
                i.viewpoints
                    .windows(2)
                    .all(|w| (w[0].time, w[0].id) <= (w[1].time, w[1].id))
            });
        if !sorted {
            let d = self.data_mut();
            d.issues.sort_by_key(|i| (i.created, i.id));
            d.comments.sort_by_key(|c| (c.time, c.id));
            d.issues
                .for_each_mut(|i| i.viewpoints.sort_by_key(|v| (v.time, v.id)));
        }
        let mut too_big = 0;
        if self.data.issues.len() > MAX_ISSUES || self.data.comments.len() > MAX_COMMENTS_TOTAL {
            too_big += 1;
        }
        too_big += self
            .data
            .issues
            .iter()
            .filter(|i| i.validate().is_err())
            .count();
        if too_big > 0 {
            notes.push(format!(
                "{too_big} issue(s) do not fit the limits of issues; they are kept as they are"
            ));
        }
        notes
    }
}

/// A rough size of an issue in bytes.
fn issue_bytes(i: &Issue) -> u64 {
    (256 + i.title.len()
        + i.description.len()
        + i.labels.iter().map(String::len).sum::<usize>()
        + i.objects.len() * 40
        + i.viewpoints
            .iter()
            .map(|v| 200 + v.selection.len() * 40 + (v.layers.len() + v.classes.len()) * 48)
            .sum::<usize>()) as u64
}

/// A rough size of a comment in bytes.
fn comment_bytes(c: &Comment) -> u64 {
    (160 + c.text.len()) as u64
}

/// The title of an issue that holds the comments of an issue that a file lost.
pub const LOST_ISSUE_TITLE: &str = "Comments of an issue that is not in the file";

/// The ID that a command gets for a new issue, comment or viewpoint: the nil ID means "make one".
pub fn is_unset(id: Uid) -> bool {
    id == Uid::NIL
}

#[cfg(test)]
mod tests;
