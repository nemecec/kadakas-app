//! Writing a BCF archive.

use std::collections::BTreeSet;
use std::io::{Cursor, Write};

use kadakas_core::issue::{Comment, Issue, IssueId, IssueStatus, Issues, ViewSpec, Viewpoint};
use kadakas_core::picture::{PictureFormat, sha256_hex};
use kadakas_core::uid::Uid;

use super::xml::Xml;
use super::{BcfError, ExportReport, Version, camera};
use crate::issues::{ifc_guid, time};

/// What an export needs.
pub struct ExportInput<'a> {
    pub issues: &'a Issues,
    /// Only these issues, in this order. `None`: all issues, oldest first.
    pub only: Option<&'a [IssueId]>,
    /// The picture file of a snapshot, by its SHA-256.
    pub snapshot: &'a dyn Fn(&str) -> Option<Vec<u8>>,
}

/// The author that BCF needs when an issue has none.
const UNKNOWN: &str = "unknown";

/// The ID of the viewpoint that an export makes for an issue that has objects and no viewpoint.
/// The import takes such a viewpoint back out: its selection goes to the objects of the issue.
pub fn generated_viewpoint_id(issue: IssueId) -> Uid {
    Uid::derive(issue.0, 1, 0)
}

/// The text of a status in BCF.
pub fn status_text(s: IssueStatus) -> &'static str {
    match s {
        IssueStatus::Open => "Open",
        IssueStatus::InProgress => "In Progress",
        IssueStatus::Resolved => "Resolved",
        IssueStatus::Closed => "Closed",
    }
}

/// One viewpoint as it goes into the file.
struct OutViewpoint<'a> {
    vp: std::borrow::Cow<'a, Viewpoint>,
    /// The picture file name in the topic folder, if the snapshot is there.
    snapshot: Option<String>,
    /// The index in the topic (sort order).
    index: usize,
}

fn author(a: &str) -> &str {
    if a.trim().is_empty() { UNKNOWN } else { a }
}

fn guid(u: impl std::fmt::Display) -> String {
    u.to_string()
}

fn picture_ext(bytes: &[u8]) -> Option<&'static str> {
    match kadakas_core::picture::PictureFormat::sniff(bytes)? {
        PictureFormat::Png => Some("png"),
        PictureFormat::Jpeg => Some("jpg"),
    }
}

/// Writes the issues as a BCF archive of the given version.
pub fn export(
    version: Version,
    input: &ExportInput<'_>,
) -> Result<(Vec<u8>, ExportReport), BcfError> {
    let list: Vec<&Issue> = match input.only {
        Some(ids) => ids
            .iter()
            .filter_map(|id| input.issues.get(*id))
            .collect(),
        None => input.issues.iter().collect(),
    };
    let mut report = ExportReport::default();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    let put = |zip: &mut zip::ZipWriter<Cursor<Vec<u8>>>,
                   name: &str,
                   bytes: &[u8]|
     -> Result<(), BcfError> {
        zip.start_file(name, opts)
            .map_err(|e| BcfError::Archive(e.to_string()))?;
        zip.write_all(bytes)
            .map_err(|e| BcfError::Archive(e.to_string()))
    };

    let mut replaced = 0usize;
    put(&mut zip, "bcf.version", version_file(version).as_bytes())?;

    let mut ext = Extensions::default();
    for issue in &list {
        let folder = guid(issue.id);
        let comments = input.issues.comments_of(issue.id);
        let (vps, mut notes) = out_viewpoints(issue, input, &folder);
        report.notes.append(&mut notes);
        for v in &vps {
            if let Some(name) = &v.snapshot {
                let bytes = (input.snapshot)(v.vp.snapshot.as_deref().unwrap_or(""))
                    .expect("checked when the viewpoint list was made");
                put(&mut zip, &format!("{folder}/{name}"), &bytes)?;
                report.snapshots += 1;
            }
            let (text, n) = viewpoint_file(version, issue, &v.vp);
            replaced += n;
            put(
                &mut zip,
                &format!("{folder}/{}.bcfv", guid(v.vp.id)),
                text.as_bytes(),
            )?;
            report.viewpoints += 1;
        }
        let (text, n) = markup(version, issue, &comments, &vps);
        replaced += n;
        put(&mut zip, &format!("{folder}/markup.bcf"), text.as_bytes())?;
        report.topics += 1;
        report.comments += comments.len();
        ext.add(issue, &comments);
    }
    if version == Version::V30 {
        let (text, n) = ext.file();
        replaced += n;
        put(&mut zip, "bcf.extensions", text.as_bytes())?;
    }
    if replaced > 0 {
        report.notes.push(format!(
            "{replaced} character(s) that XML cannot hold were replaced by U+FFFD"
        ));
    }
    if list.iter().any(|i| {
        i.viewpoints
            .iter()
            .any(|v| !v.layers.is_empty() || !v.classes.is_empty())
    }) {
        report
            .notes
            .push("the layer and class visibility of viewpoints is not part of BCF".into());
    }
    let out = zip
        .finish()
        .map_err(|e| BcfError::Archive(e.to_string()))?
        .into_inner();
    Ok((out, report))
}

fn version_file(v: Version) -> String {
    let mut x = Xml::new();
    match v {
        Version::V21 => {
            x.start("Version", &[("VersionId", "2.1")]);
            x.text("DetailedVersion", "2.1");
            x.end();
        }
        Version::V30 => x.empty("Version", &[("VersionId", "3.0")]),
    }
    x.finish()
}

/// The viewpoints of an issue as they go into the file. The objects of the issue go into the
/// selection of each one. An issue with objects and no viewpoint gets a viewpoint of its own.
fn out_viewpoints<'a>(
    issue: &'a Issue,
    input: &ExportInput<'_>,
    folder: &str,
) -> (Vec<OutViewpoint<'a>>, Vec<String>) {
    let mut notes = Vec::new();
    let mut vps: Vec<std::borrow::Cow<'a, Viewpoint>> = issue
        .viewpoints
        .iter()
        .map(std::borrow::Cow::Borrowed)
        .collect();
    if vps.is_empty() && !issue.objects.is_empty() {
        let view = match issue.point {
            Some(p) => ViewSpec::Plan {
                center: [p[0], p[1]],
                zoom: 0.1,
            },
            None => ViewSpec::Plan {
                center: [0.0, 0.0],
                zoom: 0.01,
            },
        };
        vps.push(std::borrow::Cow::Owned(Viewpoint {
            id: kadakas_core::issue::ViewpointId(generated_viewpoint_id(issue.id)),
            time: issue.created,
            view: Some(view),
            ..Viewpoint::default()
        }));
        notes.push(format!(
            "issue {folder} has objects and no viewpoint: a viewpoint was made for the file"
        ));
    }
    let mut out = Vec::new();
    for (index, mut vp) in vps.into_iter().enumerate() {
        if !issue.objects.is_empty() {
            let mut sel: BTreeSet<_> = vp.selection.iter().copied().collect();
            sel.extend(issue.objects.iter().copied());
            vp.to_mut().selection = sel.into_iter().collect();
        }
        let mut snapshot = None;
        if let Some(hash) = vp.snapshot.clone() {
            match (input.snapshot)(&hash) {
                Some(bytes) if sha256_hex(&bytes) == hash => match picture_ext(&bytes) {
                    Some(ext) => snapshot = Some(format!("{}.{ext}", guid(vp.id))),
                    None => notes.push(format!("the snapshot {hash} is not a PNG or JPEG file")),
                },
                _ => notes.push(format!(
                    "the snapshot {hash} of issue {folder} is not in the document: the viewpoint has no picture"
                )),
            }
        }
        out.push(OutViewpoint {
            vp,
            snapshot,
            index,
        });
    }
    (out, notes)
}

/// The `.bcfv` file of a viewpoint. Returns the text and the number of replaced characters.
fn viewpoint_file(version: Version, issue: &Issue, vp: &Viewpoint) -> (String, usize) {
    let mut x = Xml::new();
    x.start("VisualizationInfo", &[("Guid", &guid(vp.id))]);
    // Components: the selection, and the default visibility (BCF 2.1 needs it).
    x.start("Components", &[]);
    if !vp.selection.is_empty() {
        x.start("Selection", &[]);
        for o in &vp.selection {
            x.empty("Component", &[("IfcGuid", &ifc_guid(*o))]);
        }
        x.end();
    }
    x.empty("Visibility", &[("DefaultVisibility", "true")]);
    x.end();
    let view = vp.view.clone().unwrap_or(ViewSpec::Plan {
        center: [issue.point.map_or(0.0, |p| p[0]), issue.point.map_or(0.0, |p| p[1])],
        zoom: 0.1,
    });
    let cam = camera::to_bcf(&view, issue.point.map_or(0.0, |p| p[2]));
    x.start(
        if cam.orthogonal {
            "OrthogonalCamera"
        } else {
            "PerspectiveCamera"
        },
        &[],
    );
    x.point("CameraViewPoint", cam.eye);
    x.point("CameraDirection", cam.direction);
    x.point("CameraUpVector", cam.up);
    x.number(
        if cam.orthogonal {
            "ViewToWorldScale"
        } else {
            "FieldOfView"
        },
        cam.size,
    );
    if version == Version::V30 {
        x.number("AspectRatio", cam.aspect);
    }
    x.end();
    x.end();
    let n = x.replaced;
    (x.finish(), n)
}

fn markup(
    version: Version,
    issue: &Issue,
    comments: &[&Comment],
    vps: &[OutViewpoint<'_>],
) -> (String, usize) {
    let mut x = Xml::new();
    x.start("Markup", &[]);
    let id = guid(issue.id);
    x.start(
        "Topic",
        &[
            ("Guid", &id),
            ("TopicType", &issue.kind),
            ("TopicStatus", status_text(issue.status)),
        ],
    );
    x.text("Title", &issue.title);
    x.text("Priority", &issue.priority);
    if !issue.labels.is_empty() {
        match version {
            Version::V30 => {
                x.start("Labels", &[]);
                for l in &issue.labels {
                    x.text("Label", l);
                }
                x.end();
            }
            Version::V21 => {
                for l in &issue.labels {
                    x.text("Labels", l);
                }
            }
        }
    }
    x.text("CreationDate", &time::iso(issue.created));
    x.text("CreationAuthor", author(&issue.author));
    if issue.modified > issue.created {
        x.text("ModifiedDate", &time::iso(issue.modified));
    }
    if let Some(due) = issue.due.as_deref().and_then(time::from_date) {
        x.text("DueDate", &time::iso(due));
    }
    if let Some(a) = &issue.assignee {
        x.text("AssignedTo", a);
    }
    if !issue.description.trim().is_empty() {
        x.text("Description", &issue.description);
    }
    if version == Version::V30 {
        comment_list(&mut x, version, comments, vps);
        viewpoint_list(&mut x, version, vps);
    }
    x.end();
    if version == Version::V21 {
        comment_list(&mut x, version, comments, vps);
        viewpoint_list(&mut x, version, vps);
    }
    x.end();
    let n = x.replaced;
    (x.finish(), n)
}

fn comment_list(x: &mut Xml, version: Version, comments: &[&Comment], vps: &[OutViewpoint<'_>]) {
    if comments.is_empty() {
        return;
    }
    if version == Version::V30 {
        x.start("Comments", &[]);
    }
    for c in comments {
        x.start("Comment", &[("Guid", &guid(c.id))]);
        x.text("Date", &time::iso(c.time));
        x.text("Author", author(&c.author));
        x.text("Comment", &c.text);
        if let Some(v) = c.viewpoint
            && vps.iter().any(|o| o.vp.id == v)
        {
            x.empty("Viewpoint", &[("Guid", &guid(v))]);
        }
        if let Some(e) = c.edited {
            x.text("ModifiedDate", &time::iso(e));
        }
        x.end();
    }
    if version == Version::V30 {
        x.end();
    }
}

fn viewpoint_list(x: &mut Xml, version: Version, vps: &[OutViewpoint<'_>]) {
    if vps.is_empty() {
        return;
    }
    if version == Version::V30 {
        x.start("Viewpoints", &[]);
    }
    for v in vps {
        // BCF 2.1 repeats an element `Viewpoints` for each viewpoint; 3.0 has a list of `ViewPoint`.
        let name = if version == Version::V30 {
            "ViewPoint"
        } else {
            "Viewpoints"
        };
        x.start(name, &[("Guid", &guid(v.vp.id))]);
        x.text("Viewpoint", &format!("{}.bcfv", guid(v.vp.id)));
        if let Some(s) = &v.snapshot {
            x.text("Snapshot", s);
        }
        x.text("Index", &v.index.to_string());
        x.end();
    }
    if version == Version::V30 {
        x.end();
    }
}

/// The values that the issues use, for `bcf.extensions` of BCF 3.0.
#[derive(Default)]
struct Extensions {
    types: BTreeSet<String>,
    statuses: BTreeSet<String>,
    priorities: BTreeSet<String>,
    labels: BTreeSet<String>,
    users: BTreeSet<String>,
}

impl Extensions {
    fn add(&mut self, i: &Issue, comments: &[&Comment]) {
        self.types.insert(i.kind.clone());
        self.statuses.insert(status_text(i.status).into());
        self.priorities.insert(i.priority.clone());
        self.labels.extend(i.labels.iter().cloned());
        self.users.insert(author(&i.author).into());
        self.users.extend(i.assignee.iter().cloned());
        self.users
            .extend(comments.iter().map(|c| author(&c.author).to_string()));
    }

    fn file(&self) -> (String, usize) {
        let mut x = Xml::new();
        x.start("Extensions", &[]);
        for (outer, inner, set) in [
            ("TopicTypes", "TopicType", &self.types),
            ("TopicStatuses", "TopicStatus", &self.statuses),
            ("Priorities", "Priority", &self.priorities),
            ("TopicLabels", "TopicLabel", &self.labels),
            ("Users", "User", &self.users),
        ] {
            if set.is_empty() {
                continue;
            }
            x.start(outer, &[]);
            for v in set {
                x.text(inner, v);
            }
            x.end();
        }
        x.end();
        let n = x.replaced;
        (x.finish(), n)
    }
}
