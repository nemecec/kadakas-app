//! Parameters of the methods of the domain `collab`.
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::*;

param_enum! {
    EventsSubscribeKinds {
        Objects = "objects",
        Layers = "layers",
        Classes = "classes",
        Resources = "resources",
        Document = "document",
        Issues = "issues",
    }
}

// Parameters of `collab.status`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollabStatusParams {}

// Parameters of `collab.connect`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollabConnectParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discard: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ms: Option<Int>,
}

// Parameters of `collab.disconnect`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollabDisconnectParams {}

// Parameters of `collab.sync`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollabSyncParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ms: Option<Int>,
}

// Parameters of `collab.presence`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollabPresenceParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Vec<ObjectRef>>,
    /// {center, scale, size}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clear: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ms: Option<Int>,
}

// Parameters of `collab.undo`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollabUndoParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ms: Option<Int>,
}

// Parameters of `collab.redo`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollabRedoParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_ms: Option<Int>,
}

// Parameters of `events.subscribe`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventsSubscribeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<EventsSubscribeKinds>>,
}

// Parameters of `events.unsubscribe`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventsUnsubscribeParams {}

// Parameters of `events.poll`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventsPollParams {
    pub since_revision: Int,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<EventsSubscribeKinds>>,
}

entries! {
    "collab.status" => CollabStatusParams,
    "collab.connect" => CollabConnectParams,
    "collab.disconnect" => CollabDisconnectParams,
    "collab.sync" => CollabSyncParams,
    "collab.presence" => CollabPresenceParams,
    "collab.undo" => CollabUndoParams,
    "collab.redo" => CollabRedoParams,
    "events.subscribe" => EventsSubscribeParams,
    "events.unsubscribe" => EventsUnsubscribeParams,
    "events.poll" => EventsPollParams,
}
