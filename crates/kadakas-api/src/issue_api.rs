//! Agent API for review issues (REV-04 and the `issue.*` part of REV-07), and BCF files (REV-06).
//!
//! Methods: `issue.list`, `issue.get`, `issue.create`, `issue.update`, `issue.comment`,
//! `issue.resolve`, `issue.delete`, `issue.import_bcf`, `issue.export_bcf`.
//!
//! The model and the commands are in `kadakas_core::issue`, the filters and the BCF code are in
//! `kadakas-review`. This module turns calls into commands, so every change is one undo step and
//! one change set. An issue that an agent makes has the origin `agent` (the origin of the
//! session). Deleting the work of somebody else needs the consent of the person (`confirm`), and
//! a BCF import that meets an issue with the same GUID but other content asks and changes
//! nothing (error `conflict`, reason `bcf_conflict`).

use std::collections::{BTreeSet, HashSet};

use base64::Engine;
use kadakas_core::change::Origin;
use kadakas_core::command::Command;
use kadakas_core::issue::{
    Comment, CommentId, Issue, IssueId, IssuePatch, IssueStatus, MAX_COMMENTS, MAX_SNAPSHOT_BYTES,
    ViewSpec, Viewpoint, ViewpointId, is_unset,
};
use kadakas_core::model::{Document, ObjectId};
use kadakas_core::picture::ImageResource;
use kadakas_core::uid::Uid;
use kadakas_review::bcf::read::{ReadLimits, same_content};
use kadakas_review::bcf::write::{ExportInput, export};
use kadakas_review::bcf::{BcfError, ImportedTopic, Version};
use kadakas_review::issues::{
    Filter, capture_visibility, consent_reason, facets, pin_position, select, time,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::errors::{REASON_BCF_CONFLICT, REASON_BCF_UNFIT, REASON_CONFIRM_NEEDED, bad};
use crate::review_api::{author, default_now, session_author};
use crate::{RpcError, Session};

pub const ISSUE_LIST: (&str, &str) = (
    "issue.list",
    "List review issues (REV-04). params: {status? (a status or a list: open, in_progress, resolved, closed), open_only? (true: not resolved or closed), assignee?, label?, object? (an object ID: the issue is about it), text? (in the title or the description), origin? (user, agent, script, remote), sort? (created, default, oldest first; modified, newest first; status), limit? (default 200, at most 1000), offset? (how many matching issues to skip, default 0: the next page starts at next_offset)}. Returns {issues: [{id, title, type, status, priority, assignee, labels, due, author, origin, created, modified (ISO 8601), created_ms, modified_ms, objects (count), viewpoints (count), comments (count), point, pin (where the pin is: the point, else the first object, else the plan view)}], count (listed), total (matching), offset, next_offset (null at the end), facets: {assignees, labels}}.",
);
pub const ISSUE_GET: (&str, &str) = (
    "issue.get",
    "One issue with its viewpoints and its comment thread. params: {id, snapshots? (true: add snapshot_base64 to each viewpoint that has a picture)}. Returns the issue: {id, title, description, type, status, priority, assignee, labels, due, author, origin, created, modified (ISO 8601), created_ms, modified_ms, point, objects, objects_missing (objects that are not in the document), objects_ifc (the IFC GlobalIds, as the IFC export and BCF write them), pin, viewpoints: [{id, name, time, view, layers, classes, selection, snapshot (SHA-256 of the picture file), snapshot_base64?}], comments: [{id, author, origin, time, time_ms, text, viewpoint?, edited?}]}.",
);
pub const ISSUE_CREATE: (&str, &str) = (
    "issue.create",
    "Make an issue. One undo step. The origin is the origin of the session (agent for an agent). params: {title, description?, type? (issue, error, warning, request, clash, remark or any text; default issue), status? (default open), priority? (low, normal, high, critical or any text; default normal), assignee?, labels?, due? (YYYY-MM-DD), objects? (IDs; they must exist), point? ([x, y, z] in mm), author? (default: the collaboration name, else the origin), now_ms?, viewpoint? {name?, view? ({kind: plan, center: [x, y], zoom} or {kind: camera, target: [x, y, z], yaw, pitch, distance, perspective?}), selection? (default: the objects), capture_visibility? (default true: the visible layers and classes now), snapshot? (true or the params of render.png: a picture of the model; or snapshot_base64: a PNG or JPEG file you made)}, capture? (true: a viewpoint of the current view: the view of the session, the selection when objects is not given, the visible layers and classes, and a snapshot), snapshot? (true or render.png params: with a viewpoint or capture)}. The snapshot is stored once in the document as a picture (SHA-256), at most 2 MiB. Returns {id, viewpoints: [ids], issue: summary, note?}; `note` is there when the document has more than 10000 issues (a warning only; the cap is 100000).",
);
pub const ISSUE_UPDATE: (&str, &str) = (
    "issue.update",
    "Change an issue. One undo step; a call that changes nothing makes no step. params: {id, title?, description?, type?, status?, priority?, assignee? (null or empty: clear), labels? (replace), add_labels?, remove_labels?, due? (YYYY-MM-DD; null or empty: clear), objects? (replace), add_objects?, remove_objects?, point? ([x, y, z] or null: clear), remove_viewpoints? (viewpoint IDs; the picture stays in the document until resource.purge; viewpoints of an issue of somebody else need confirm: true, as issue.delete), confirm?, now_ms?}. Returns {id, changed, issue: summary}.",
);
pub const ISSUE_ADD_VIEWPOINT: (&str, &str) = (
    "issue.add_viewpoint",
    "Add a viewpoint to an issue. One undo step. params: {id, name?, view? ({kind: plan, center: [x, y], zoom} or {kind: camera, target: [x, y, z], yaw, pitch, distance, perspective?}; default: the current view of the session), selection? (object IDs; default: the objects of the issue), capture_visibility? (default true: the visible layers and classes now), snapshot? (true or the params of render.png), snapshot_base64? (a PNG or JPEG file you made), now_ms?}. At most 64 viewpoints for an issue. Returns {id, viewpoint (the new ID), issue: summary}. Remove viewpoints with issue.update {remove_viewpoints: [IDs]}.",
);
pub const ISSUE_COMMENT: (&str, &str) = (
    "issue.comment",
    "Add a comment to the thread of an issue. A comment is a record of its own, so two people who comment at the same time both keep their comment. One undo step. params: {id, text, viewpoint? (the ID of a viewpoint of the issue), author?, now_ms?}. The origin is the origin of the session. At most 5000 comments for an issue. Returns {comment: ID, issue: summary}.",
);
pub const ISSUE_RESOLVE: (&str, &str) = (
    "issue.resolve",
    "Set the status of an issue to resolved (default) or closed, with an optional comment, in one undo step. params: {id, status? (resolved or closed), comment? (a text), author?, now_ms?}. Returns {id, changed, issue: summary}.",
);
pub const ISSUE_DELETE: (&str, &str) = (
    "issue.delete",
    "Delete issues with their comments. One undo step. Work of somebody else is never deleted silently: an issue that another origin made (a person, an agent, a script, another user), or another author, or one that has comments of another origin or author, needs {confirm: true}, which you may send only when the person has said so. Without it the call fails with a conflict error (reason confirm_needed) whose data has `issues` (what would be lost and why) and ask_the_person. Nothing is deleted if one issue needs the consent. params: {id} or {ids}, confirm?, author? (who deletes; default the collaboration name, else the origin), purge_snapshots? (true: the pictures that no other issue uses go too, in the same undo step; refused with a policy error while the session is connected to a shared document). The consent is checked against who this session is (the collaboration name in a shared session, else the identity the host gave, else the origin); the caller's `author` is ignored, and an issue or comment without an author always needs the consent. Somebody else is another origin or another author with the same origin. A delete leaves the snapshot pictures in the document until a purge. Returns {deleted: [ID], count}.",
);
pub const ISSUE_IMPORT_BCF: (&str, &str) = (
    "issue.import_bcf",
    "Read a BCF file (.bcfzip, BCF 2.1 or 3.0) and add its issues. One undo step. params: {path | base64 (the file), on_conflict? (ask, default; skip; replace; keep_both), skip_items? (true: import without the items that do not fit, see below), dry_run? (true: a plan of what would happen, no change), author? (the author of the new issues that the file gives none; default the collaboration name, else the origin), now_ms?}. Issues are matched by the GUID of the topic. A GUID that the document does not have is added. A GUID that the document has with the same content (what BCF holds) is left alone. A GUID with other content is a conflict: with `ask` the call fails with a conflict error (reason bcf_conflict, ask_the_person) that lists the conflicts, and nothing is imported; pass skip (keep ours), replace (take the values of the file for the fields, the viewpoints and the comments of the same ID, and add the new ones; viewpoints and comments that only we have stay) or keep_both (add the issue of the file with new IDs) after the person chose. Components with an IfcGuid become the objects of the issue (the GlobalId of an IFC export of this model gives the same object). Items that do not fit the limits of Kadakas (a comment that is not valid, a snapshot over the size limits, a camera that cannot be used) are never dropped silently: the call fails with a conflict error (reason bcf_unfit, ask_the_person, options skip_items and cancel) that lists them, and nothing is imported; ask the person, then call again with skip_items: true, or give up. A dry run lists them in `unfit` and in the plan. With replace, a picture or a camera of ours stays when the file's was refused. A comment GUID that another issue of the document already uses gets a new ID. Reading is bounded: a violation is an error. A topic that cannot be read or breaks a bound (no title, a title over 200 characters, over 64 viewpoints, over 5000 comments, a repeated GUID) is an item of the same question (item `topic`): nothing is imported until the caller passes skip_items, and then it is listed in `topics_skipped`. A topic that is the same already, or a conflict that on_conflict skip keeps ours for, is not asked about. The other things that were not read are in `warnings`. Returns {version, imported, replaced, unchanged, skipped_conflicts, kept_both, topics_skipped: [{topic, reason}], items_skipped: [{topic, item, reason, viewpoint}], warnings, created: [IDs], pictures, dry_run?, plan?}.",
);
pub const ISSUE_EXPORT_BCF: (&str, &str) = (
    "issue.export_bcf",
    "Write issues as a BCF file (.bcfzip). params: {path (a file that ends in .bcfzip; omit to get the file as base64), version? (3.0 default, or 2.1), ids?, status?, open_only?, assignee?, label?, object? (which issues; default all), overwrite? (a restricted session needs it to replace a file)}. Objects become Components with the IfcGuid of the IFC export. Not in BCF: the layers and classes of a viewpoint, the origin, the pin. Returns {version, path?, base64?, bytes, topics, viewpoints, snapshots, comments, notes}.",
);

/// Extra text for `describe`.
pub fn describe() -> Value {
    json!({
        "model": "An issue (BCF terms) has a title, description, type, status (open, in_progress, resolved, closed), priority, assignee, labels, a due date, an author, an origin (user, agent, script, remote), times, a pin point, the objects it is about (by ID), viewpoints and a comment thread. A viewpoint is a plan view or a 3D camera, the visible layers and classes, a selection and a snapshot picture. Issues live in the document as review data, not as objects: they have no layer and no shape, and they are not versions (a restore leaves them as they are). Every change is one undo step and works in collaboration: an issue is a set of properties by ID, comments are records of their own, so two people who edit or comment at the same time keep both.",
        "bounds": {
            "issues": kadakas_core::issue::MAX_ISSUES,
            "issues_warning_above": kadakas_core::issue::WARN_ISSUES,
            "title_chars": kadakas_core::issue::MAX_TITLE_CHARS,
            "text_chars": kadakas_core::issue::MAX_TEXT_CHARS,
            "label_chars": kadakas_core::issue::MAX_LABEL_CHARS,
            "labels": kadakas_core::issue::MAX_LABELS,
            "name_chars": kadakas_core::issue::MAX_NAME_CHARS,
            "comments_per_issue": MAX_COMMENTS,
            "comments": kadakas_core::issue::MAX_COMMENTS_TOTAL,
            "viewpoints_per_issue": kadakas_core::issue::MAX_VIEWPOINTS,
            "objects_per_issue": kadakas_core::issue::MAX_OBJECTS,
            "snapshot_bytes": MAX_SNAPSHOT_BYTES,
            "snapshot_pixels": kadakas_core::issue::MAX_SNAPSHOT_PIXELS,
            "note": "A value over a bound is refused with a message. Nothing is cut. The limits were chosen with the person from measurements (REV-04 in the feature list): more than issues_warning_above issues only gives a note (issue.create answers with `note`, issue.import_bcf adds it to `warnings`, the app shows it), issues is the hard cap. The snapshot limits are still provisional.",
        },
        "consent": "issue.delete never deletes the work of somebody else silently: an issue of another origin, or with comments of another origin, needs confirm: true after the person said yes. issue.import_bcf never overwrites: an issue with the same GUID and other content is a conflict that the person decides (skip, replace or keep_both).",
        "origin": "An issue and a comment made through the API have the origin agent. The app makes user, a change of another user in a shared document arrives as remote.",
        "events": "Every issue change is an events.change with the kind `issues`: added.issues, changed.issues and removed.issues list the issue IDs, added.comments, changed.comments and removed.comments list the comment IDs. A comment, a viewpoint or a snapshot also lists its issue in changed.issues. The op is the command (add_issue, modify_issue, add_issue_comment, ...), or issue_import_bcf for an import. events.subscribe takes the kind `issues`.",
        "snapshots": "A snapshot is a picture file (PNG or JPEG, at most 2 MiB and 4 million pixels), stored once in the document as an `image` resource named by its SHA-256, and the viewpoint holds the hash. The resource goes when the last viewpoint that uses it goes (not if a plant style uses it too).",
        "bcf": {
            "versions": ["3.0", "2.1"],
            "files": ".bcfzip. One folder for each issue with markup.bcf, a .bcfv for each viewpoint and the snapshot pictures.",
            "ids": "The GUIDs of the topic, the viewpoints and the comments are the IDs of the issue, the viewpoints and the comments. A component is an IfcGuid: the 128 bits of the object ID in the IFC alphabet, the same GlobalId that export.file writes for ifc.",
            "objects": "BCF has no component list for a topic, only for viewpoints. The objects of an issue go into the selection of each of its viewpoints, and an issue with objects and no viewpoint gets a viewpoint for the file. On import the objects of an issue are the union of the selections.",
            "limits": "Reading is bounded: at most 50000 entries, 32 MiB for an entry, 4 MiB for an XML file, 512 MiB in all, a compression ratio of 200 to 1, names of at most 256 bytes without .., absolute paths and drive letters, no encrypted entries, at most 10000 topics, 5000 comments and 64 viewpoints for a topic, XML with at most 40 levels and 200000 nodes and no DTD, snapshots of at most 2 MiB and 4 million pixels. A violation of the archive is an error and nothing is imported; a topic that breaks a limit is listed in topics_skipped.",
            "status": "Statuses are matched without case: open, active, new, assigned and reopened are open; in progress and started are in_progress; resolved, fixed and done are resolved; closed is closed. Any other word is open, and the label bcf-status:<word> keeps it.",
            "not_read": "Clipping planes, lines, bitmaps, coloring, hidden components, document references, related topics, BIM snippets and reference links are not read. warnings counts them.",
            "schema": "The files that export_bcf writes are checked against the buildingSMART XSD files of BCF 2.1 and 3.0 in the behavior tests.",
        },
        "examples": [
            {"method": "issue.create", "params": {"title": "Dead tree at the gate", "type": "error", "priority": "high", "objects": ["0190f1c2-7a3b-7c1d-8e2f-123456789abc"], "capture": true}},
            {"method": "issue.list", "params": {"status": ["open", "in_progress"], "assignee": "Mari"}},
            {"method": "issue.get", "params": {"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abd", "snapshots": true}},
            {"method": "issue.update", "params": {"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abd", "assignee": "Mari", "add_labels": ["planting"]}},
            {"method": "issue.add_viewpoint", "params": {"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abd", "snapshot": true}},
            {"method": "issue.comment", "params": {"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abd", "text": "Replaced with a field maple."}},
            {"method": "issue.resolve", "params": {"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abd", "comment": "Done."}},
            {"method": "issue.delete", "params": {"id": "0190f1c2-7a3b-7c1d-8e2f-123456789abd"}},
            {"method": "issue.export_bcf", "params": {"path": "/data/issues.bcfzip", "version": "3.0", "open_only": true}},
            {"method": "issue.import_bcf", "params": {"path": "/data/from-engineer.bcfzip", "on_conflict": "ask"}}
        ],
    })
}

pub fn knows(method: &str) -> bool {
    method.starts_with("issue.")
}

fn parse<T: for<'de> Deserialize<'de>>(p: Value) -> Result<T, RpcError> {
    let p = if p.is_null() { json!({}) } else { p };
    serde_json::from_value(p).map_err(RpcError::invalid_params)
}

fn now_of(ms: Option<i64>) -> i64 {
    ms.unwrap_or_else(|| default_now() as i64)
}

fn iso(ms: i64) -> String {
    time::iso(ms)
}

fn comment_count(doc: &Document, id: IssueId) -> usize {
    doc.issues.comment_count(id)
}

fn pin_of(doc: &Document, i: &Issue) -> Value {
    let pos = |o: ObjectId| {
        doc.object(o).map(|ob| {
            let (lo, hi) = ob.shape.bounds();
            let c = (lo + hi) / 2.0;
            [c.x, c.y, 0.0]
        })
    };
    pin_position(i, &pos).map_or(Value::Null, |p| json!(p))
}

/// The short form of an issue, for lists and answers of changes.
pub fn summary(doc: &Document, i: &Issue) -> Value {
    json!({
        "id": i.id,
        "title": i.title,
        "type": i.kind,
        "status": i.status.name(),
        "priority": i.priority,
        "assignee": i.assignee,
        "labels": i.labels,
        "due": i.due,
        "author": i.author,
        "origin": i.origin.name(),
        "created": iso(i.created),
        "modified": iso(i.modified),
        "created_ms": i.created,
        "modified_ms": i.modified,
        "objects": i.objects.len(),
        "viewpoints": i.viewpoints.len(),
        "comments": comment_count(doc, i.id),
        "point": i.point,
        "pin": pin_of(doc, i),
    })
}

/// The whole issue with its comments.
fn full(doc: &Document, i: &Issue, snapshots: bool) -> Value {
    let mut v = serde_json::to_value(i).unwrap_or(Value::Null);
    let o = v.as_object_mut().expect("an issue is an object");
    o.insert("created_ms".into(), json!(i.created));
    o.insert("modified_ms".into(), json!(i.modified));
    o.insert("created".into(), json!(iso(i.created)));
    o.insert("modified".into(), json!(iso(i.modified)));
    o.insert("status".into(), json!(i.status.name()));
    o.insert("origin".into(), json!(i.origin.name()));
    o.insert("labels".into(), json!(i.labels));
    o.insert("objects".into(), json!(i.objects));
    let missing: Vec<ObjectId> = i
        .objects
        .iter()
        .copied()
        .filter(|o| doc.object(*o).is_none())
        .collect();
    o.insert("objects_missing".into(), json!(missing));
    o.insert(
        "objects_ifc".into(),
        json!(
            i.objects
                .iter()
                .map(|o| kadakas_review::issues::ifc_guid(*o))
                .collect::<Vec<_>>()
        ),
    );
    o.insert("pin".into(), pin_of(doc, i));
    let mut vps = Vec::new();
    for vp in &i.viewpoints {
        let mut x = serde_json::to_value(vp).unwrap_or(Value::Null);
        let m = x.as_object_mut().expect("a viewpoint is an object");
        m.insert("time_ms".into(), json!(vp.time));
        m.insert("time".into(), json!(iso(vp.time)));
        if snapshots
            && let Some(img) = vp
                .snapshot
                .as_deref()
                .and_then(|h| doc.resources.find_image(h))
                .and_then(|r| doc.resources.image(r))
        {
            m.insert("snapshot_base64".into(), json!(img.data));
        }
        vps.push(x);
    }
    o.insert("viewpoints".into(), Value::Array(vps));
    let comments: Vec<Value> = doc
        .issues
        .comments_of(i.id)
        .into_iter()
        .map(|c| {
            let mut x = serde_json::to_value(c).unwrap_or(Value::Null);
            let m = x.as_object_mut().expect("a comment is an object");
            m.insert("time_ms".into(), json!(c.time));
            m.insert("time".into(), json!(iso(c.time)));
            m.insert("origin".into(), json!(c.origin.name()));
            m.remove("issue");
            x
        })
        .collect();
    o.insert("comments".into(), Value::Array(comments));
    v
}

fn issue_id(m: &Map<String, Value>, key: &str) -> Result<IssueId, RpcError> {
    let v = m
        .get(key)
        .ok_or_else(|| bad(format!("give {key}: the ID of an issue")))?;
    serde_json::from_value(v.clone()).map_err(|e| bad(format!("{key}: {e}")))
}

fn find(doc: &Document, id: IssueId) -> Result<&Issue, RpcError> {
    doc.issues
        .get(id)
        .ok_or_else(|| RpcError::not_found(format!("issue {id} does not exist")))
}

fn object_ids(doc: &Document, ids: &[ObjectId]) -> Result<(), RpcError> {
    for id in ids {
        if doc.object(*id).is_none() {
            return Err(RpcError::not_found(format!("object {id} does not exist")));
        }
    }
    Ok(())
}

fn flag(m: &Map<String, Value>, key: &str) -> Result<bool, RpcError> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(bad(format!("{key} must be true or false"))),
    }
}

fn as_object(p: &Value) -> Result<Map<String, Value>, RpcError> {
    match p {
        Value::Null => Ok(Map::new()),
        Value::Object(m) => Ok(m.clone()),
        _ => Err(bad("params must be an object")),
    }
}

fn status_list(m: &Map<String, Value>) -> Result<Vec<IssueStatus>, RpcError> {
    let one = |v: &Value| {
        v.as_str()
            .and_then(IssueStatus::parse)
            .ok_or_else(|| bad("status is open, in_progress, resolved or closed"))
    };
    match m.get("status") {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::Array(a)) => a.iter().map(one).collect(),
        Some(v) => Ok(vec![one(v)?]),
    }
}

fn text_param(m: &Map<String, Value>, key: &str) -> Result<Option<String>, RpcError> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(t)) => Ok(Some(t.clone())),
        Some(_) => Err(bad(format!("{key} must be text"))),
    }
}

fn filter_of(m: &Map<String, Value>) -> Result<Filter, RpcError> {
    let object = match m.get("object") {
        None | Some(Value::Null) => None,
        Some(v) => {
            Some(serde_json::from_value(v.clone()).map_err(|e| bad(format!("object: {e}")))?)
        }
    };
    let origin = match text_param(m, "origin")? {
        None => None,
        Some(t) => {
            Some(Origin::parse(&t).ok_or_else(|| bad("origin is user, agent, script or remote"))?)
        }
    };
    Ok(Filter {
        status: status_list(m)?,
        assignee: text_param(m, "assignee")?,
        label: text_param(m, "label")?,
        object,
        text: text_param(m, "text")?,
        origin,
        open_only: flag(m, "open_only")?,
    })
}

/// The viewpoint of a call: `issue.create` (`viewpoint`) and `issue.add_viewpoint`.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Vp {
    name: Option<String>,
    view: Option<ViewSpec>,
    selection: Option<Vec<ObjectId>>,
    capture_visibility: Option<bool>,
    snapshot: Option<Value>,
    snapshot_base64: Option<String>,
}

/// The options of a snapshot: `true` is a render with the defaults, an object is the params of
/// `render.png`.
fn snapshot_options(v: &Option<Value>) -> Option<Value> {
    match v {
        Some(Value::Bool(true)) => Some(json!({})),
        Some(Value::Object(m)) => Some(Value::Object(m.clone())),
        _ => None,
    }
}

const LIST_LIMIT: usize = 200;
const LIST_LIMIT_MAX: usize = 1000;

impl Session {
    /// The method of an `issue.*` call. `None`: not one of ours.
    pub(crate) fn handle_issue(
        &mut self,
        method: &str,
        p: Value,
    ) -> Option<Result<Value, RpcError>> {
        if !knows(method) {
            return None;
        }
        Some(match method {
            "issue.list" => self.issue_list(&p),
            "issue.get" => self.issue_get(&p),
            "issue.create" => self.issue_create(&p),
            "issue.update" => self.issue_update(&p),
            "issue.add_viewpoint" => self.issue_add_viewpoint(&p),
            "issue.comment" => self.issue_comment(&p),
            "issue.resolve" => self.issue_resolve(&p),
            "issue.delete" => self.issue_delete(&p),
            "issue.import_bcf" => self.issue_import_bcf(&p),
            "issue.export_bcf" => self.issue_export_bcf(&p),
            _ => Err(RpcError::method_not_found(method)),
        })
    }

    fn issue_list(&mut self, p: &Value) -> Result<Value, RpcError> {
        let m = as_object(p)?;
        let filter = filter_of(&m)?;
        let limit = match m.get("limit") {
            None | Some(Value::Null) => LIST_LIMIT,
            Some(v) => v
                .as_u64()
                .filter(|n| *n >= 1)
                .ok_or_else(|| bad("limit is a whole number of 1 or more"))?
                .min(LIST_LIMIT_MAX as u64) as usize,
        };
        let offset = match m.get("offset") {
            None | Some(Value::Null) => 0,
            Some(v) => v
                .as_u64()
                .ok_or_else(|| bad("offset is a whole number of 0 or more"))?
                as usize,
        };
        let doc = self.editor.doc();
        let mut list = select(&doc.issues, &filter);
        match text_param(&m, "sort")?.as_deref() {
            None | Some("created") => {}
            Some("modified") => list.sort_by_key(|i| std::cmp::Reverse((i.modified, i.id))),
            Some("status") => list.sort_by_key(|i| (i.status, i.created, i.id)),
            Some(_) => return Err(bad("sort is created, modified or status")),
        }
        let total = list.len();
        let shown: Vec<Value> = list
            .iter()
            .skip(offset)
            .take(limit)
            .map(|i| summary(doc, i))
            .collect();
        let next = offset.saturating_add(shown.len());
        let (assignees, labels) = facets(&doc.issues);
        Ok(json!({
            "issues": shown,
            "count": shown.len(),
            "total": total,
            "offset": offset,
            "next_offset": (next < total).then_some(next),
            "facets": {"assignees": assignees, "labels": labels},
        }))
    }

    fn issue_get(&mut self, p: &Value) -> Result<Value, RpcError> {
        let m = as_object(p)?;
        let id = issue_id(&m, "id")?;
        let snapshots = flag(&m, "snapshots")?;
        let doc = self.editor.doc();
        Ok(full(doc, find(doc, id)?, snapshots))
    }

    /// Renders the model as a snapshot picture. `opts` are the params of `render.png`.
    fn issue_snapshot(&mut self, opts: &Value) -> Result<ImageResource, RpcError> {
        let mut p = match opts {
            Value::Object(m) => Value::Object(m.clone()),
            _ => json!({}),
        };
        if p.get("width").is_none() && p.get("height").is_none() {
            p["width"] = json!(640);
            p["height"] = json!(480);
        }
        let r = self.dispatch("render.png", p)?;
        let png = base64::engine::general_purpose::STANDARD
            .decode(r["png_base64"].as_str().unwrap_or_default())
            .map_err(|e| RpcError::app(format!("the render gave no picture: {e}")))?;
        self.snapshot_from_bytes(&png)
    }

    fn snapshot_from_bytes(&self, bytes: &[u8]) -> Result<ImageResource, RpcError> {
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(RpcError::limit(format!(
                "the snapshot is {} bytes; at most {MAX_SNAPSHOT_BYTES}: use a smaller width and height",
                bytes.len()
            )));
        }
        // The smaller of the limits of the session and of a snapshot.
        let mut limits = self.limits().picture_limits();
        limits.max_pixels = limits
            .max_pixels
            .min(kadakas_core::issue::MAX_SNAPSHOT_PIXELS);
        kadakas_render::picture::import_picture(bytes, kadakas_core::issue::SNAPSHOT_SOURCE, limits)
            .map_err(RpcError::validation)
    }

    /// The view of the session as a viewpoint view: a plan view that frames the objects, or the
    /// orbit camera of the standard view.
    fn current_view(&mut self, objects: &[ObjectId]) -> ViewSpec {
        use kadakas_render::camera::Camera;
        use kadakas_render::views::StandardView;
        let settings = self.view;
        if settings.view == StandardView::Plan || settings.view == StandardView::Top {
            let doc = self.editor.doc();
            let mut lo = glam::DVec2::splat(f64::INFINITY);
            let mut hi = glam::DVec2::splat(f64::NEG_INFINITY);
            let mut take = |o: &kadakas_core::model::Object| {
                let (a, b) = o.shape.bounds();
                if a.is_finite() && b.is_finite() {
                    lo = lo.min(a);
                    hi = hi.max(b);
                }
            };
            let named: Vec<_> = objects.iter().filter_map(|o| doc.object(*o)).collect();
            if named.is_empty() {
                doc.objects.values().for_each(&mut take);
            } else {
                named.into_iter().for_each(&mut take);
            }
            if lo.x > hi.x {
                return ViewSpec::Plan {
                    center: [0.0, 0.0],
                    zoom: 0.05,
                };
            }
            let size = (hi - lo).max(glam::DVec2::splat(100.0));
            let zoom = (0.9 * (1200.0 / size.x).min(800.0 / size.y)).clamp(1e-4, 100.0);
            let c = (lo + hi) / 2.0;
            return ViewSpec::Plan {
                center: [c.x, c.y],
                zoom,
            };
        }
        let scene = self.scene();
        let mut camera = Camera::standard(settings.view, settings.projection);
        camera.fit(scene.bounds, 1.5);
        ViewSpec::Camera {
            target: [
                f64::from(camera.target.x),
                f64::from(camera.target.y),
                f64::from(camera.target.z),
            ],
            yaw: f64::from(camera.yaw),
            pitch: f64::from(camera.pitch),
            distance: f64::from(camera.distance),
            perspective: settings.projection == kadakas_render::views::Projection::Perspective,
        }
    }

    /// Makes a viewpoint of the session: the view (given, else the current view of the session),
    /// the selection, the visible layers and classes, and a snapshot picture if one is wanted.
    fn build_viewpoint(
        &mut self,
        vp: Vp,
        default_selection: Vec<ObjectId>,
        capture: bool,
        top_snapshot: &Option<Value>,
        now: i64,
    ) -> Result<(Viewpoint, Option<ImageResource>), RpcError> {
        let selection = match vp.selection {
            Some(s) => {
                object_ids(self.editor.doc(), &s)?;
                s
            }
            None => default_selection,
        };
        let view = match vp.view {
            Some(v) => v,
            None => self.current_view(&selection),
        };
        let mut v = Viewpoint {
            name: vp.name.unwrap_or_default(),
            time: now,
            view: Some(view),
            selection,
            ..Viewpoint::default()
        };
        if vp.capture_visibility.unwrap_or(true) {
            let (layers, classes) = capture_visibility(self.editor.doc());
            v.layers = layers;
            v.classes = classes;
        }
        let opts = snapshot_options(&vp.snapshot)
            .or_else(|| snapshot_options(top_snapshot))
            .or(if capture && vp.snapshot_base64.is_none() {
                Some(json!({}))
            } else {
                None
            });
        let mut picture = None;
        if let Some(b64) = vp.snapshot_base64 {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .map_err(|e| bad(format!("snapshot_base64: {e}")))?;
            picture = Some(self.snapshot_from_bytes(&bytes)?);
        } else if let Some(opts) = opts {
            picture = Some(self.issue_snapshot(&opts)?);
        }
        v.snapshot = picture.as_ref().map(|p| p.sha256.clone());
        Ok((v, picture))
    }

    fn issue_add_viewpoint(&mut self, p: &Value) -> Result<Value, RpcError> {
        let mut m = as_object(p)?;
        let id = issue_id(&m, "id")?;
        let now = now_of(m.get("now_ms").and_then(Value::as_i64));
        m.remove("id");
        m.remove("now_ms");
        let vp: Vp = parse(Value::Object(m))?;
        let issue = find(self.editor.doc(), id)?.clone();
        let (v, picture) = self.build_viewpoint(vp, issue.objects.clone(), false, &None, now)?;
        let out = self
            .editor
            .execute(&Command::AddIssueViewpoint {
                issue: id,
                viewpoint: Box::new(v),
                picture: picture.map(Box::new),
                now,
            })
            .map_err(RpcError::from)?;
        let doc = self.editor.doc();
        Ok(json!({
            "id": id,
            "viewpoint": out.created[0],
            "issue": summary(doc, find(doc, id)?),
        }))
    }

    fn issue_create(&mut self, p: &Value) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct P {
            title: String,
            description: Option<String>,
            #[serde(rename = "type")]
            kind: Option<String>,
            status: Option<IssueStatus>,
            priority: Option<String>,
            assignee: Option<String>,
            labels: Option<Vec<String>>,
            due: Option<String>,
            objects: Option<Vec<ObjectId>>,
            point: Option<[f64; 3]>,
            #[serde(rename = "author")]
            _author: Option<String>,
            now_ms: Option<i64>,
            viewpoint: Option<Vp>,
            capture: Option<bool>,
            snapshot: Option<Value>,
        }
        let m = as_object(p)?;
        let author_name = author(self, &m)?;
        let P {
            title,
            description,
            kind,
            status,
            priority,
            assignee,
            labels,
            due,
            objects,
            point,
            now_ms,
            viewpoint,
            capture,
            snapshot,
            ..
        } = parse(p.clone())?;
        let now = now_of(now_ms);
        let objects = objects.unwrap_or_default();
        object_ids(self.editor.doc(), &objects)?;
        let mut issue = Issue {
            title,
            description: description.unwrap_or_default(),
            kind: kind.unwrap_or_else(|| kadakas_core::issue::DEFAULT_TYPE.into()),
            status: status.unwrap_or_default(),
            priority: priority.unwrap_or_else(|| kadakas_core::issue::DEFAULT_PRIORITY.into()),
            assignee: assignee.filter(|a| !a.is_empty()),
            labels: labels.unwrap_or_default(),
            due: due.filter(|d| !d.is_empty()),
            author: author_name,
            origin: self.editor.origin(),
            created: now,
            modified: now,
            point,
            objects: objects.clone(),
            ..Issue::default()
        };
        let capture = capture.unwrap_or(false);
        let mut pictures: Vec<ImageResource> = Vec::new();
        if viewpoint.is_some() || capture || snapshot_options(&snapshot).is_some() {
            let default_selection = if objects.is_empty() && capture {
                self.selection.clone()
            } else {
                objects.clone()
            };
            let (v, picture) = self.build_viewpoint(
                viewpoint.unwrap_or_default(),
                default_selection,
                capture,
                &snapshot,
                now,
            )?;
            pictures.extend(picture);
            issue.viewpoints.push(v);
        }
        let out = self
            .editor
            .execute(&Command::AddIssue {
                issue: Box::new(issue),
                pictures,
            })
            .map_err(RpcError::from)?;
        let id = IssueId(out.created[0]);
        let doc = self.editor.doc();
        let mut answer = json!({
            "id": id,
            "viewpoints": out.created[1..],
            "issue": summary(doc, find(doc, id)?),
        });
        if let Some(note) = many_issues_note(doc) {
            answer["note"] = json!(note);
        }
        Ok(answer)
    }

    fn issue_update(&mut self, p: &Value) -> Result<Value, RpcError> {
        let m = as_object(p)?;
        const KEYS: [&str; 19] = [
            "id",
            "title",
            "description",
            "type",
            "status",
            "priority",
            "assignee",
            "labels",
            "add_labels",
            "remove_labels",
            "due",
            "objects",
            "add_objects",
            "remove_objects",
            "point",
            "now_ms",
            "author",
            "remove_viewpoints",
            "confirm",
        ];
        if let Some(k) = m.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(bad(format!("unknown parameter {k}")));
        }
        let id = issue_id(&m, "id")?;
        let doc = self.editor.doc();
        let old = find(doc, id)?;
        let mut patch = IssuePatch {
            title: text_param(&m, "title")?,
            description: text_param(&m, "description")?,
            kind: text_param(&m, "type")?,
            priority: text_param(&m, "priority")?,
            ..IssuePatch::default()
        };
        if let Some(v) = m.get("status").filter(|v| !v.is_null()) {
            patch.status = Some(
                v.as_str()
                    .and_then(IssueStatus::parse)
                    .ok_or_else(|| bad("status is open, in_progress, resolved or closed"))?,
            );
        }
        // null and empty text clear.
        for (key, slot) in [("assignee", &mut patch.assignee), ("due", &mut patch.due)] {
            match m.get(key) {
                None => {}
                Some(Value::Null) => *slot = Some(String::new()),
                Some(Value::String(t)) => *slot = Some(t.clone()),
                Some(_) => return Err(bad(format!("{key} must be text or null"))),
            }
        }
        let list = |key: &str| -> Result<Option<Vec<String>>, RpcError> {
            match m.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => serde_json::from_value(v.clone())
                    .map(Some)
                    .map_err(|_| bad(format!("{key} must be a list of texts"))),
            }
        };
        let mut labels = list("labels")?.unwrap_or_else(|| old.labels.clone());
        let touched = m.contains_key("labels")
            || m.contains_key("add_labels")
            || m.contains_key("remove_labels");
        for l in list("add_labels")?.unwrap_or_default() {
            if !labels.iter().any(|x| x.to_lowercase() == l.to_lowercase()) {
                labels.push(l);
            }
        }
        if let Some(r) = list("remove_labels")? {
            let r: BTreeSet<String> = r.iter().map(|x| x.to_lowercase()).collect();
            labels.retain(|x| !r.contains(&x.to_lowercase()));
        }
        if touched {
            patch.labels = Some(labels);
        }
        let ids = |key: &str| -> Result<Option<Vec<ObjectId>>, RpcError> {
            match m.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(v) => serde_json::from_value(v.clone())
                    .map(Some)
                    .map_err(|e| bad(format!("{key}: {e}"))),
            }
        };
        let mut objects = ids("objects")?.unwrap_or_else(|| old.objects.clone());
        let add = ids("add_objects")?.unwrap_or_default();
        object_ids(doc, &add)?;
        if let Some(o) = ids("objects")? {
            object_ids(doc, &o)?;
        }
        for o in add {
            if !objects.contains(&o) {
                objects.push(o);
            }
        }
        if let Some(r) = ids("remove_objects")? {
            objects.retain(|o| !r.contains(o));
        }
        if m.contains_key("objects")
            || m.contains_key("add_objects")
            || m.contains_key("remove_objects")
        {
            patch.objects = Some(objects);
        }
        match m.get("point") {
            None => {}
            Some(Value::Null) => patch.clear_point = true,
            Some(v) => {
                patch.point = Some(
                    serde_json::from_value(v.clone())
                        .map_err(|_| bad("point is [x, y, z] or null"))?,
                );
            }
        }
        let now = now_of(m.get("now_ms").and_then(Value::as_i64));
        let mut commands = vec![Command::ModifyIssue {
            id,
            patch: Box::new(patch),
            now,
        }];
        if let Some(v) = m.get("remove_viewpoints").filter(|v| !v.is_null()) {
            let gone: Vec<ViewpointId> = serde_json::from_value(v.clone())
                .map_err(|e| bad(format!("remove_viewpoints: {e}")))?;
            if !gone.is_empty() {
                let my_author = session_author(self);
                self.check_consent(&[id], &my_author, flag(&m, "confirm")?)?;
            }
            commands.extend(
                gone.into_iter()
                    .map(|viewpoint| Command::DeleteIssueViewpoint {
                        issue: id,
                        viewpoint,
                        now,
                    }),
            );
        }
        let before = self.editor.revision();
        // One command keeps its own name in the change events.
        let command = if commands.len() == 1 {
            commands.remove(0)
        } else {
            Command::Batch { commands }
        };
        self.editor.execute(&command).map_err(RpcError::from)?;
        let changed = self.editor.revision() != before;
        let doc = self.editor.doc();
        Ok(json!({"id": id, "changed": changed, "issue": summary(doc, find(doc, id)?)}))
    }

    fn issue_comment(&mut self, p: &Value) -> Result<Value, RpcError> {
        let m = as_object(p)?;
        if let Some(k) = m
            .keys()
            .find(|k| !["id", "text", "viewpoint", "author", "now_ms"].contains(&k.as_str()))
        {
            return Err(bad(format!("unknown parameter {k}")));
        }
        let id = issue_id(&m, "id")?;
        let text = text_param(&m, "text")?.ok_or_else(|| bad("give text"))?;
        let viewpoint = match m.get("viewpoint").filter(|v| !v.is_null()) {
            None => None,
            Some(v) => Some(
                serde_json::from_value::<ViewpointId>(v.clone())
                    .map_err(|e| bad(format!("viewpoint: {e}")))?,
            ),
        };
        let now = now_of(m.get("now_ms").and_then(Value::as_i64));
        let comment = Comment {
            id: CommentId::default(),
            issue: id,
            author: author(self, &m)?,
            origin: self.editor.origin(),
            time: now,
            text,
            viewpoint,
            edited: None,
        };
        let out = self
            .editor
            .execute(&Command::AddIssueComment {
                comment: Box::new(comment),
            })
            .map_err(RpcError::from)?;
        let doc = self.editor.doc();
        Ok(json!({"comment": out.created[0], "issue": summary(doc, find(doc, id)?)}))
    }

    fn issue_resolve(&mut self, p: &Value) -> Result<Value, RpcError> {
        let m = as_object(p)?;
        if let Some(k) = m
            .keys()
            .find(|k| !["id", "status", "comment", "author", "now_ms"].contains(&k.as_str()))
        {
            return Err(bad(format!("unknown parameter {k}")));
        }
        let id = issue_id(&m, "id")?;
        let status = match text_param(&m, "status")?.as_deref() {
            None | Some("resolved") => IssueStatus::Resolved,
            Some("closed") => IssueStatus::Closed,
            Some(_) => return Err(bad("status is resolved or closed")),
        };
        let now = now_of(m.get("now_ms").and_then(Value::as_i64));
        find(self.editor.doc(), id)?;
        let mut commands = vec![Command::ModifyIssue {
            id,
            patch: Box::new(IssuePatch {
                status: Some(status),
                ..IssuePatch::default()
            }),
            now,
        }];
        if let Some(text) = text_param(&m, "comment")? {
            commands.push(Command::AddIssueComment {
                comment: Box::new(Comment {
                    id: CommentId::default(),
                    issue: id,
                    author: author(self, &m)?,
                    origin: self.editor.origin(),
                    time: now,
                    text,
                    viewpoint: None,
                    edited: None,
                }),
            });
        }
        let before = self.editor.revision();
        self.editor
            .execute(&Command::Batch { commands })
            .map_err(RpcError::from)?;
        let changed = self.editor.revision() != before;
        let doc = self.editor.doc();
        Ok(json!({"id": id, "changed": changed, "issue": summary(doc, find(doc, id)?)}))
    }

    /// The rule of consent: the work of somebody else is not removed unless `confirm` is true.
    /// Somebody else is another origin or another author. Nothing is removed if one issue asks.
    fn check_consent(
        &self,
        ids: &[IssueId],
        my_author: &str,
        confirm: bool,
    ) -> Result<(), RpcError> {
        let me = self.editor.origin();
        let doc = self.editor.doc();
        let mut ask = Vec::new();
        for id in ids {
            let i = find(doc, *id)?;
            let comments = doc.issues.comments_of(*id);
            if let Some(why) = consent_reason(i, &comments, me, my_author) {
                ask.push(json!({
                    "id": id, "title": i.title, "author": i.author,
                    "origin": i.origin.name(), "comments": comments.len(), "why": why,
                }));
            }
        }
        if ask.is_empty() || confirm {
            return Ok(());
        }
        Err(RpcError::conflict(
            "these issues were made by somebody else or have comments of somebody else: nothing was changed. Ask the person, then call again with confirm: true",
        )
        .with_reason(REASON_CONFIRM_NEEDED)
        .with_details(json!({"issues": ask, "ask_the_person": true})))
    }

    fn issue_delete(&mut self, p: &Value) -> Result<Value, RpcError> {
        let m = as_object(p)?;
        if let Some(k) = m
            .keys()
            .find(|k| !["id", "ids", "confirm", "author", "purge_snapshots"].contains(&k.as_str()))
        {
            return Err(bad(format!("unknown parameter {k}")));
        }
        let ids: Vec<IssueId> = match (m.get("id"), m.get("ids")) {
            (Some(_), None) => vec![issue_id(&m, "id")?],
            (None, Some(v)) => {
                serde_json::from_value(v.clone()).map_err(|e| bad(format!("ids: {e}")))?
            }
            _ => return Err(bad("give id or ids")),
        };
        let confirm = flag(&m, "confirm")?;
        // The consent is checked against who this session is, not against a parameter.
        let my_author = session_author(self);
        self.check_consent(&ids, &my_author, confirm)?;
        let mut commands: Vec<Command> = ids
            .iter()
            .map(|id| Command::DeleteIssue { id: *id })
            .collect();
        // On request the pictures that no other issue uses go too (one undo step). A shared
        // document should not do this: another user may point to the picture at the same time.
        if flag(&m, "purge_snapshots")? {
            if crate::collab_api::is_connected(self) {
                return Err(RpcError::policy(
                    "purge_snapshots is not allowed in a shared session: another user may point to a picture at the same time. Disconnect first, or delete the pictures later with resource.purge",
                ));
            }
            let doc = self.editor.doc();
            let kept: std::collections::BTreeSet<&str> = doc
                .issues
                .iter()
                .filter(|i| !ids.contains(&i.id))
                .flat_map(|i| i.viewpoints.iter().filter_map(|v| v.snapshot.as_deref()))
                .collect();
            let mut gone = BTreeSet::new();
            for i in ids.iter().filter_map(|id| doc.issues.get(*id)) {
                for h in i.viewpoints.iter().filter_map(|v| v.snapshot.as_deref()) {
                    if !kept.contains(h)
                        && let Some(rid) = doc.resources.find_image(h)
                        && kadakas_core::library::usage(doc, rid).resources.is_empty()
                    {
                        gone.insert(rid);
                    }
                }
            }
            commands.extend(
                gone.into_iter()
                    .map(|id| Command::DeleteResource { id, reassign: None }),
            );
        }
        self.editor
            .execute(&Command::Batch { commands })
            .map_err(RpcError::from)?;
        Ok(json!({"deleted": ids, "count": ids.len()}))
    }

    // ----- BCF -----

    fn issue_export_bcf(&mut self, p: &Value) -> Result<Value, RpcError> {
        let m = as_object(p)?;
        const KEYS: [&str; 11] = [
            "path",
            "version",
            "ids",
            "status",
            "open_only",
            "assignee",
            "label",
            "object",
            "overwrite",
            "text",
            "origin",
        ];
        if let Some(k) = m.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(bad(format!("unknown parameter {k}")));
        }
        let version = match text_param(&m, "version")?.as_deref() {
            None => Version::V30,
            Some(t) => Version::parse(t)
                .filter(|_| t != "2.0")
                .ok_or_else(|| bad("version is 3.0 or 2.1"))?,
        };
        let doc = self.editor.doc();
        let only: Option<Vec<IssueId>> = match m.get("ids").filter(|v| !v.is_null()) {
            Some(v) => {
                let ids: Vec<IssueId> =
                    serde_json::from_value(v.clone()).map_err(|e| bad(format!("ids: {e}")))?;
                for id in &ids {
                    find(doc, *id)?;
                }
                Some(ids)
            }
            None => {
                let filter = filter_of(&m)?;
                (filter != Filter::default())
                    .then(|| select(&doc.issues, &filter).iter().map(|i| i.id).collect())
            }
        };
        let lookup = |hash: &str| {
            doc.resources
                .find_image(hash)
                .and_then(|r| doc.resources.image(r))
                .and_then(ImageResource::bytes)
        };
        let (bytes, report) = export(
            version,
            &ExportInput {
                issues: &doc.issues,
                only: only.as_deref(),
                snapshot: &lookup,
                to_png: &|bytes: &[u8]| {
                    let img = kadakas_render::texture::decode_image_limited(
                        bytes,
                        kadakas_core::issue::MAX_SNAPSHOT_PIXELS,
                    )
                    .ok()?;
                    kadakas_render::texture::encode_png(img.width, img.height, &img.rgba).ok()
                },
            },
        )
        .map_err(bcf_error)?;
        let mut out = json!({
            "version": version.id(),
            "bytes": bytes.len(),
            "topics": report.topics,
            "viewpoints": report.viewpoints,
            "snapshots": report.snapshots,
            "comments": report.comments,
            "notes": report.notes,
        });
        match text_param(&m, "path")? {
            Some(path) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let overwrite = flag(&m, "overwrite")?;
                    let path = self.write_bcf_file(&path, &bytes, overwrite)?;
                    out["path"] = json!(path);
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = path;
                    return Err(RpcError::policy(
                        "a browser has no files: leave out path to get the file as base64",
                    ));
                }
            }
            None => {
                out["base64"] = json!(base64::engine::general_purpose::STANDARD.encode(&bytes));
            }
        }
        Ok(out)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn write_bcf_file(
        &self,
        path: &str,
        bytes: &[u8],
        overwrite: bool,
    ) -> Result<String, RpcError> {
        let path = std::path::PathBuf::from(path);
        let path = self
            .check_path(&path, true, &["bcfzip"])
            .map_err(RpcError::policy)?;
        if self.paths.is_restricted() && path.exists() && !overwrite {
            return Err(RpcError::conflict(format!(
                "{} exists: pass \"overwrite\": true to replace it",
                path.display()
            )));
        }
        kadakas_sys::fsio::write_atomic_with(&path, |w| w.write_all(bytes))
            .map_err(|e| RpcError::app(format!("cannot write {}: {e}", path.display())))?;
        Ok(path.display().to_string())
    }

    fn bcf_bytes(&self, m: &Map<String, Value>) -> Result<Vec<u8>, RpcError> {
        let limit = ReadLimits::default();
        match (text_param(m, "path")?, text_param(m, "base64")?) {
            (Some(_), Some(_)) => Err(bad("give path or base64, not both")),
            (None, Some(b64)) => {
                // The text is checked before it is decoded.
                if b64.len() as u64 > limit.max_total_bytes.div_ceil(3) * 4 {
                    return Err(RpcError::limit("the BCF file is too large"));
                }
                base64::engine::general_purpose::STANDARD
                    .decode(b64.trim())
                    .map_err(|e| bad(format!("base64: {e}")))
            }
            (Some(path), None) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    use std::io::Read;
                    let path = std::path::PathBuf::from(path);
                    let path = self
                        .check_path(&path, false, &["bcfzip", "bcf", "zip"])
                        .map_err(RpcError::policy)?;
                    let mut r = kadakas_sys::fsio::open_regular(&path, limit.max_total_bytes)
                        .map_err(|e| {
                            RpcError::app(format!("cannot read {}: {e}", path.display()))
                        })?;
                    let mut bytes = Vec::new();
                    r.read_to_end(&mut bytes).map_err(|e| {
                        RpcError::app(format!("cannot read {}: {e}", path.display()))
                    })?;
                    Ok(bytes)
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let _ = path;
                    Err(RpcError::policy(
                        "a browser has no files: give the file as base64",
                    ))
                }
            }
            (None, None) => Err(bad("give path or base64")),
        }
    }

    fn issue_import_bcf(&mut self, p: &Value) -> Result<Value, RpcError> {
        let m = as_object(p)?;
        if let Some(k) = m.keys().find(|k| {
            ![
                "path",
                "base64",
                "on_conflict",
                "skip_items",
                "dry_run",
                "now_ms",
                "author",
            ]
            .contains(&k.as_str())
        }) {
            return Err(bad(format!("unknown parameter {k}")));
        }
        #[derive(Clone, Copy, PartialEq)]
        enum Mode {
            Ask,
            Skip,
            Replace,
            KeepBoth,
        }
        let mode = match text_param(&m, "on_conflict")?.as_deref() {
            None | Some("ask") => Mode::Ask,
            Some("skip") => Mode::Skip,
            Some("replace") => Mode::Replace,
            Some("keep_both") => Mode::KeepBoth,
            Some(_) => return Err(bad("on_conflict is ask, skip, replace or keep_both")),
        };
        let dry_run = flag(&m, "dry_run")?;
        let skip_items = flag(&m, "skip_items")?;
        let now = now_of(m.get("now_ms").and_then(Value::as_i64));
        let bytes = self.bcf_bytes(&m)?;
        let mut imported = kadakas_review::bcf::read::read(&bytes, &ReadLimits::default(), now)
            .map_err(bcf_error)?;
        let me = self.editor.origin();
        let my_author = author(self, &m)?;

        // Sort the topics: new, the same, or a conflict.
        let doc = self.editor.doc();
        let (mut new, mut same, mut conflicts) = (Vec::new(), Vec::new(), Vec::new());
        for (n, t) in imported.topics.iter().enumerate() {
            match doc.issues.get(t.issue.id) {
                None => new.push(n),
                Some(e) => {
                    let comments = doc.issues.comments_of(e.id);
                    if same_content(e, &comments, t) {
                        same.push(n);
                    } else {
                        conflicts.push(n);
                    }
                }
            }
        }
        // What does not fit: found by the reader, the topics that could not be read at all, and
        // the pictures that cannot be made. It is listed, and the import goes on only when the
        // caller says to skip these items. A topic that is the same already, or that the caller
        // keeps ours for (`skip`), brings nothing in, so nothing is asked about it.
        let bringing: HashSet<String> = new
            .iter()
            .chain(if mode == Mode::Skip {
                &[][..]
            } else {
                &conflicts[..]
            })
            .map(|n| imported.topics[*n].issue.id.to_string())
            .collect();
        let reader_unfit: Vec<&kadakas_review::bcf::Unfit> = imported
            .unfit
            .iter()
            .filter(|u| bringing.contains(&u.topic))
            .collect();
        let mut unfit: Vec<Value> = reader_unfit.iter().map(|u| unfit_json(u)).collect();
        unfit.extend(imported.skipped.iter().map(
            |s| json!({"topic": s.topic, "item": "topic", "reason": s.reason, "viewpoint": null}),
        ));
        let keep_snapshot: HashSet<ViewpointId> = reader_unfit
            .iter()
            .filter(|u| u.item == "snapshot")
            .filter_map(|u| u.viewpoint)
            .collect();
        let keep_view: HashSet<ViewpointId> = reader_unfit
            .iter()
            .filter(|u| u.item == "camera")
            .filter_map(|u| u.viewpoint)
            .collect();
        let mut prepared: Vec<Option<Prepared>> = imported
            .topics
            .iter()
            .enumerate()
            .map(|(n, t)| {
                (new.contains(&n) || (mode != Mode::Skip && conflicts.contains(&n))).then(|| {
                    Prepared::new(
                        self,
                        t,
                        me,
                        &my_author,
                        &keep_snapshot,
                        &keep_view,
                        &mut unfit,
                    )
                })
            })
            .collect();
        let doc = self.editor.doc();
        let conflict_json = |n: usize| {
            let t = &imported.topics[n];
            let existing = doc.issues.get(t.issue.id).expect("a conflict has an issue");
            json!({
                "id": t.issue.id,
                "ours": summary(doc, existing),
                "file": {
                    "title": t.issue.title, "status": t.issue.status.name(),
                    "modified": iso(t.issue.modified), "comments": t.comments.len(),
                    "viewpoints": t.issue.viewpoints.len(),
                },
            })
        };
        let plan = json!({
            "new": new.iter().map(|n| imported.topics[*n].issue.id).collect::<Vec<_>>(),
            "unchanged": same.iter().map(|n| imported.topics[*n].issue.id).collect::<Vec<_>>(),
            "conflicts": conflicts.iter().map(|n| conflict_json(*n)).collect::<Vec<_>>(),
            "unfit": unfit,
        });
        if !unfit.is_empty() && !skip_items && !dry_run {
            return Err(RpcError::conflict(format!(
                "{} item(s) of the file do not fit the limits of Kadakas (a comment that is not valid, a picture that is too large, a camera that cannot be used): nothing was imported. Ask the person, then call again with skip_items: true to import without them, or give up",
                unfit.len()
            ))
            .with_reason(REASON_BCF_UNFIT)
            .with_details(json!({
                "ask_the_person": true,
                "unfit": unfit,
                "options": ["skip_items", "cancel"],
                "new": new.len(),
                "unchanged": same.len(),
                "conflicts": conflicts.len(),
            })));
        }
        if !conflicts.is_empty() && mode == Mode::Ask && !dry_run {
            return Err(RpcError::conflict(format!(
                "{} issue(s) of the file have the GUID of an issue in the document and other content: nothing was imported. Ask the person, then call again with on_conflict skip, replace or keep_both",
                conflicts.len()
            ))
            .with_reason(REASON_BCF_CONFLICT)
            .with_details(json!({
                "ask_the_person": true,
                "conflicts": conflicts.iter().map(|n| conflict_json(*n)).collect::<Vec<_>>(),
                "options": ["skip", "replace", "keep_both"],
                "new": new.len(),
                "unchanged": same.len(),
            })));
        }
        let skipped: Vec<Value> = imported
            .skipped
            .iter()
            .map(|s| json!({"topic": s.topic, "reason": s.reason}))
            .collect();
        let mut warnings = std::mem::take(&mut imported.warnings);
        if dry_run {
            return Ok(json!({
                "version": imported.version.id(),
                "dry_run": true,
                "plan": plan,
                "topics_skipped": skipped,
                "unfit": unfit,
                "warnings": warnings,
            }));
        }

        // Build the commands. The IDs of comments and issues that are taken, in the document or
        // by an earlier topic of the file, are checked here, so one topic cannot fail the rest.
        let mut commands: Vec<Command> = Vec::new();
        let (mut replaced, mut kept_both, mut skipped_conflicts) = (0usize, 0usize, 0usize);
        let mut created_ids: Vec<Uid> = Vec::new();
        let mut pictures = 0usize;
        let mut used_comments: HashSet<CommentId> =
            doc.issues.data().comments.iter().map(|c| c.id).collect();
        let mut taken_issues: HashSet<IssueId> = doc
            .issues
            .iter()
            .map(|i| i.id)
            .chain(imported.topics.iter().map(|t| t.issue.id))
            .collect();
        for n in &new {
            let mut prep = prepared[*n].take().expect("each topic once");
            prep.claim_comment_ids(&mut used_comments);
            pictures += prep.pictures.len();
            created_ids.push(prep.issue.id.0);
            prep.add_commands(&mut commands);
        }
        for n in &conflicts {
            if mode == Mode::Skip {
                skipped_conflicts += 1;
                continue;
            }
            let mut prep = prepared[*n].take().expect("each topic once");
            match mode {
                Mode::Skip => unreachable!("handled above"),
                Mode::KeepBoth => {
                    prep.renumber(doc, &mut taken_issues, &mut used_comments);
                    pictures += prep.pictures.len();
                    created_ids.push(prep.issue.id.0);
                    prep.add_commands(&mut commands);
                    kept_both += 1;
                }
                Mode::Replace => {
                    pictures += prep.pictures.len();
                    prep.replace_commands(doc, now, &mut used_comments, &mut commands);
                    replaced += 1;
                }
                Mode::Ask => unreachable!("asked above"),
            }
        }
        let imported_n = new.len();
        let detail = json!({
            "imported": imported_n, "replaced": replaced, "kept_both": kept_both,
            "unchanged": same.len(), "skipped_conflicts": skipped_conflicts,
            "items_skipped": unfit.len(),
        });
        if !commands.is_empty() {
            let history = self.editor.doc().history.clone();
            self.editor
                .execute_with_history(
                    &Command::Batch { commands },
                    history,
                    "issue_import_bcf",
                    Some(detail),
                )
                .map_err(RpcError::from)?;
        }
        if let Some(note) = many_issues_note(self.editor.doc()) {
            warnings.push(note);
        }
        Ok(json!({
            "version": imported.version.id(),
            "imported": imported_n,
            "replaced": replaced,
            "unchanged": same.len(),
            "skipped_conflicts": skipped_conflicts,
            "kept_both": kept_both,
            "topics_skipped": skipped,
            "items_skipped": unfit,
            "warnings": warnings,
            "created": created_ids,
            "pictures": pictures,
        }))
    }
}

/// The note for a document with more issues than [`kadakas_core::issue::WARN_ISSUES`].
fn many_issues_note(doc: &Document) -> Option<String> {
    (doc.issues.len() > kadakas_core::issue::WARN_ISSUES).then(|| {
        format!(
            "the document has {} issues; above {} the lists get slow to read and the file grows (the cap is {})",
            doc.issues.len(),
            kadakas_core::issue::WARN_ISSUES,
            kadakas_core::issue::MAX_ISSUES
        )
    })
}

fn unfit_json(u: &kadakas_review::bcf::Unfit) -> Value {
    json!({"topic": u.topic, "item": u.item, "reason": u.reason, "viewpoint": u.viewpoint})
}

/// An imported topic ready for commands: the picture files are checked and made into pictures,
/// the origin is set, and a snapshot whose picture failed is taken out. Whatever does not fit is
/// listed in `unfit`, so the caller can ask the person.
struct Prepared {
    issue: Issue,
    comments: Vec<Comment>,
    pictures: Vec<ImageResource>,
    /// Viewpoints whose picture the file has but we refused: a replace keeps our picture.
    keep_snapshot: HashSet<ViewpointId>,
    /// Viewpoints whose camera could not be used: a replace keeps our view.
    keep_view: HashSet<ViewpointId>,
}

impl Prepared {
    fn new(
        s: &Session,
        t: &ImportedTopic,
        me: Origin,
        author: &str,
        keep_snapshot: &HashSet<ViewpointId>,
        keep_view: &HashSet<ViewpointId>,
        unfit: &mut Vec<Value>,
    ) -> Prepared {
        let mut issue = t.issue.clone();
        issue.origin = me;
        if issue.author.trim().is_empty() {
            issue.author = author.to_string();
        }
        let mut keep_snapshot = keep_snapshot.clone();
        let mut pictures: Vec<ImageResource> = Vec::new();
        for pic in &t.pictures {
            let why = match s.snapshot_from_bytes(&pic.bytes) {
                Ok(img) if img.sha256 == pic.hash => {
                    pictures.push(img);
                    continue;
                }
                Ok(_) => "the picture changed on the way".to_string(),
                Err(e) => e.message,
            };
            for v in issue
                .viewpoints
                .iter()
                .filter(|v| v.snapshot.as_deref() == Some(pic.hash.as_str()))
            {
                keep_snapshot.insert(v.id);
                unfit.push(json!({
                    "topic": issue.id, "item": "snapshot", "reason": why, "viewpoint": v.id,
                }));
            }
        }
        for v in &mut issue.viewpoints {
            if let Some(h) = &v.snapshot
                && !pictures.iter().any(|p| &p.sha256 == h)
            {
                v.snapshot = None;
            }
        }
        let comments = t
            .comments
            .iter()
            .cloned()
            .map(|mut c| {
                c.origin = me;
                c
            })
            .collect();
        Prepared {
            issue,
            comments,
            pictures,
            keep_snapshot,
            keep_view: keep_view.clone(),
        }
    }

    fn picture_for(&self, hash: &Option<String>) -> Option<Box<ImageResource>> {
        let h = hash.as_deref()?;
        self.pictures
            .iter()
            .find(|p| p.sha256 == h)
            .map(|p| Box::new(p.clone()))
    }

    /// A comment ID that is taken (in the document, or by an earlier topic) becomes the nil ID:
    /// the command makes a new one. The others are marked as taken.
    fn claim_comment_ids(&mut self, used: &mut HashSet<CommentId>) {
        for c in &mut self.comments {
            if !used.insert(c.id) {
                c.id = CommentId::default();
            }
        }
    }

    /// New IDs for the issue, its viewpoints and its comments (`keep_both`), made from the old
    /// IDs, so the same file gives the same copy. `taken_issues` and `used_comments` hold the IDs
    /// of the document and of the other topics of the file, and get the new ones.
    fn renumber(
        &mut self,
        doc: &Document,
        taken_issues: &mut HashSet<IssueId>,
        used_comments: &mut HashSet<CommentId>,
    ) {
        let fresh = |old: Uid, taken: &dyn Fn(Uid) -> bool| {
            (1..)
                .map(|n| Uid::derive(old, 10, n))
                .find(|u| !taken(*u))
                .expect("an unused ID")
        };
        let new_issue = IssueId(fresh(self.issue.id.0, &|u| {
            taken_issues.contains(&IssueId(u)) || doc.issues.get(IssueId(u)).is_some()
        }));
        taken_issues.insert(new_issue);
        let mut vp_map: Vec<(ViewpointId, ViewpointId)> = Vec::new();
        for v in &mut self.issue.viewpoints {
            let new = ViewpointId(fresh(v.id.0, &|_| false));
            vp_map.push((v.id, new));
            v.id = new;
        }
        self.issue.id = new_issue;
        for c in &mut self.comments {
            c.id = CommentId(fresh(c.id.0, &|u| used_comments.contains(&CommentId(u))));
            used_comments.insert(c.id);
            c.issue = new_issue;
            if let Some(v) = c.viewpoint {
                c.viewpoint = vp_map.iter().find(|(o, _)| *o == v).map(|(_, n)| *n);
            }
        }
        debug_assert!(!is_unset(new_issue.0));
    }

    /// The commands of a new issue: the issue and its comments.
    fn add_commands(self, out: &mut Vec<Command>) {
        let comments = self.comments;
        out.push(Command::AddIssue {
            issue: Box::new(self.issue),
            pictures: self.pictures,
        });
        out.extend(comments.into_iter().map(|c| Command::AddIssueComment {
            comment: Box::new(c),
        }));
    }

    /// The commands that bring the values of the file into an issue that the document has: the
    /// fields, the viewpoints and the comments of the same ID, and the new ones. What only the
    /// document has stays, and so does a picture or a camera of ours that the file's was refused.
    fn replace_commands(
        self,
        doc: &Document,
        now: i64,
        used: &mut HashSet<CommentId>,
        out: &mut Vec<Command>,
    ) {
        let id = self.issue.id;
        let ours = doc.issues.get(id).expect("a conflict has an issue");
        let i = &self.issue;
        out.push(Command::ModifyIssue {
            id,
            patch: Box::new(IssuePatch {
                title: Some(i.title.clone()),
                description: Some(i.description.clone()),
                kind: Some(i.kind.clone()),
                status: Some(i.status),
                priority: Some(i.priority.clone()),
                assignee: Some(i.assignee.clone().unwrap_or_default()),
                labels: Some(i.labels.clone()),
                due: Some(i.due.clone().unwrap_or_default()),
                objects: Some(i.objects.clone()),
                ..IssuePatch::default()
            }),
            now,
        });
        for v in &i.viewpoints {
            match ours.viewpoints.iter().find(|o| o.id == v.id) {
                Some(o) => {
                    // BCF has no layers and classes: ours stay.
                    let mut merged = v.clone();
                    merged.layers = o.layers.clone();
                    merged.classes = o.classes.clone();
                    let mut picture = self.picture_for(&v.snapshot);
                    if self.keep_snapshot.contains(&v.id) && v.snapshot.is_none() {
                        merged.snapshot = o.snapshot.clone();
                        picture = None;
                    }
                    if self.keep_view.contains(&v.id) && v.view.is_none() {
                        merged.view = o.view.clone();
                    }
                    out.push(Command::ModifyIssueViewpoint {
                        issue: id,
                        viewpoint: Box::new(merged),
                        picture,
                        now,
                    });
                }
                None => out.push(Command::AddIssueViewpoint {
                    issue: id,
                    viewpoint: Box::new(v.clone()),
                    picture: self.picture_for(&v.snapshot),
                    now,
                }),
            }
        }
        for c in &self.comments {
            match doc.issues.comment(c.id) {
                Some(o) if o.issue == id => {
                    if o.text != c.text {
                        out.push(Command::EditIssueComment {
                            id: c.id,
                            text: c.text.clone(),
                            now,
                        });
                    }
                }
                // Another issue has a comment with this ID, or an earlier topic of the file
                // took it: this one gets a new ID.
                other => {
                    let mut c = c.clone();
                    if other.is_some() || !used.insert(c.id) {
                        c.id = CommentId::default();
                    }
                    out.push(Command::AddIssueComment {
                        comment: Box::new(c),
                    });
                }
            }
        }
    }
}

fn bcf_error(e: BcfError) -> RpcError {
    match e {
        BcfError::Limit(_) => RpcError::limit(e),
        other => RpcError::validation(other),
    }
}

#[cfg(test)]
mod tests;
