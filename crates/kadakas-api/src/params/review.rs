//! Parameters of the methods of the domain `review`.
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::*;

param_enum! {
    VersionDiffKinds {
        Objects = "objects",
        Layers = "layers",
        Classes = "classes",
        Resources = "resources",
        Document = "document",
    }
}

param_enum! {
    IssueListOrigin {
        User = "user",
        Agent = "agent",
        Script = "script",
        Remote = "remote",
    }
}

param_enum! {
    IssueListSort {
        Created = "created",
        Oldest = "oldest",
        Modified = "modified",
        Status = "status",
    }
}

param_enum! {
    IssueResolveStatus {
        Resolved = "resolved",
        Closed = "closed",
    }
}

param_enum! {
    IssueImportBcfOnConflict {
        Ask = "ask",
        Skip = "skip",
        Replace = "replace",
        KeepBoth = "keep_both",
    }
}

param_enum! {
    IssueExportBcfVersion {
        V30 = "3.0",
        V21 = "2.1",
    }
}

// Parameters of `revision.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RevisionListParams {}

// Parameters of `version.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VersionCreateParams {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ms: Option<Int>,
}

// Parameters of `version.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VersionListParams {
    /// false: leave out current_is_newest, which costs a pass over the drawing; default true
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<bool>,
}

// Parameters of `version.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VersionGetParams {
    pub id: Either<Int, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<bool>,
}

// Parameters of `version.restore`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VersionRestoreParams {
    pub id: Either<Int, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ms: Option<Int>,
}

// Parameters of `version.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["ids"]}]}]))]
pub struct VersionDeleteParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Either<Int, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<Either<Int, String>>>,
}

// Parameters of `version.diff`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VersionDiffParams {
    /// a version ID or "current"
    pub from: Either<Int, String>,
    /// a version ID or "current"
    pub to: Either<Int, String>,
    /// default all
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<VersionDiffKinds>>,
    /// at most 1000
    #[schemars(extend("default" = 100))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<Int>,
}

// Parameters of `version.repair`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VersionRepairParams {
    pub confirm: bool,
}

// Parameters of `issue.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueListParams {
    /// open, in_progress, resolved, closed
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Either<String, Vec<String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// the issue is about this object
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectRef>,
    /// in the title or the description
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<IssueListOrigin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<IssueListSort>,
    /// at most 1000
    #[schemars(extend("default" = 200))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<Int>,
    #[schemars(extend("default" = 0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<Int>,
}

// Parameters of `issue.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueGetParams {
    pub id: String,
    /// true: add snapshot_base64 to each viewpoint that has a picture
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshots: Option<bool>,
}

// Parameters of `issue.create`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueCreateParams {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[schemars(extend("default" = "issue"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[schemars(extend("default" = "open"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[schemars(extend("default" = "normal"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    /// YYYY-MM-DD
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    /// they must exist
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objects: Option<Vec<ObjectRef>>,
    /// [x, y, z] in mm
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<Vec<Num>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ms: Option<Int>,
    /// {name?, view?, selection?, capture_visibility?, snapshot?, snapshot_base64?}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewpoint: Option<Obj>,
    /// a viewpoint of the current view
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<bool>,
    /// true or render.png params
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Either<bool, Obj>>,
}

// Parameters of `issue.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueUpdateParams {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<String>")]
    pub assignee: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub add_labels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_labels: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<String>")]
    pub due: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objects: Option<Vec<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub add_objects: Option<Vec<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_objects: Option<Vec<ObjectRef>>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Nullable<Vec<Num>>")]
    pub point: Option<Option<Vec<Num>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_viewpoints: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ms: Option<Int>,
}

// Parameters of `issue.add_viewpoint`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueAddViewpointParams {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// {kind: plan, center, zoom} or {kind: camera, target, yaw, pitch, distance, perspective?}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<Obj>,
    /// default: the objects of the issue
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Vec<ObjectRef>>,
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_visibility: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Either<bool, Obj>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ms: Option<Int>,
}

// Parameters of `issue.comment`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueCommentParams {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ms: Option<Int>,
}

// Parameters of `issue.resolve`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueResolveParams {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<IssueResolveStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ms: Option<Int>,
}

// Parameters of `issue.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["ids"]}]}]))]
pub struct IssueDeleteParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm: Option<bool>,
    /// who deletes; default the collaboration name, else the origin
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// the pictures that no other issue uses go too (refused in a shared session)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purge_snapshots: Option<bool>,
}

// Parameters of `issue.import_bcf`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["path"]}, {"required": ["base64"]}]}]))]
pub struct IssueImportBcfParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base64: Option<String>,
    #[schemars(extend("default" = "ask"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_conflict: Option<IssueImportBcfOnConflict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_items: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ms: Option<Int>,
}

// Parameters of `issue.export_bcf`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueExportBcfParams {
    /// a file that ends in .bcfzip; omit to get the file as base64
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[schemars(extend("default" = 3.0))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<IssueExportBcfVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Either<String, Vec<String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overwrite: Option<bool>,
}

// Parameters of `dxf.compare`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DxfCompareParams {
    /// the old file
    pub base: String,
    /// the new file
    pub new: String,
    /// {tolerance? (units of the drawing), layers? (only entities on these DXF layers), limit? (default 200, at most 1000), ...import options such as hatches}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Obj>,
    /// {format: png|svg|pdf, revision_clouds?, width?, height?, palette?, unchanged?, dark?, ghosts?, previous?, view?}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render: Option<Obj>,
}

entries! {
    "revision.list" => RevisionListParams,
    "version.create" => VersionCreateParams,
    "version.list" => VersionListParams,
    "version.get" => VersionGetParams,
    "version.restore" => VersionRestoreParams,
    "version.delete" => VersionDeleteParams,
    "version.diff" => VersionDiffParams,
    "version.repair" => VersionRepairParams,
    "issue.list" => IssueListParams,
    "issue.get" => IssueGetParams,
    "issue.create" => IssueCreateParams,
    "issue.update" => IssueUpdateParams,
    "issue.add_viewpoint" => IssueAddViewpointParams,
    "issue.comment" => IssueCommentParams,
    "issue.resolve" => IssueResolveParams,
    "issue.delete" => IssueDeleteParams,
    "issue.import_bcf" => IssueImportBcfParams,
    "issue.export_bcf" => IssueExportBcfParams,
    "dxf.compare" => DxfCompareParams,
}
