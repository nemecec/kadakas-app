//! Error codes of the API. One place for all of them.
//!
//! Every error has a numeric `code`, a human `message` and `data.kind`, a stable string.
//! The JSON-RPC codes -32700, -32600, -32601 and -32602 keep their usual meaning.

use serde_json::{Value, json};

/// Anything that does not fit another code.
pub const CODE_GENERIC: i64 = 1;
/// An object, layer, class, resource, file or job does not exist.
pub const CODE_NOT_FOUND: i64 = 2;
/// The request is well formed, but the document or a value does not allow it.
pub const CODE_VALIDATION: i64 = 3;
/// The host policy refuses it: a path, a URL, a script or a converter.
pub const CODE_POLICY: i64 = 4;
/// The target is in use or has changed.
pub const CODE_CONFLICT: i64 = 5;
/// A size, count or time limit was exceeded.
pub const CODE_LIMIT: i64 = 6;

/// `(code, kind, meaning)` of every code, for `describe`.
pub const CODES: [(i64, &str, &str); 11] = [
    (-32700, "parse_error", "The request is not valid JSON."),
    (
        -32600,
        "invalid_request",
        "The request has no method or has the wrong shape.",
    ),
    (-32601, "method_not_found", "The method does not exist."),
    (
        -32602,
        "invalid_params",
        "The parameters are missing or have the wrong type.",
    ),
    (
        -32603,
        "internal",
        "Kadakas failed in a way it must not. This is a bug: file it with the feedback tool.",
    ),
    (1, "error", "Any other failure."),
    (
        2,
        "not_found",
        "An object, layer, class, resource, file or job does not exist.",
    ),
    (
        3,
        "validation",
        "The values are well formed, but the document does not allow them.",
    ),
    (
        4,
        "policy",
        "The host refuses it: a path, URL, script or converter is not allowed.",
    ),
    (5, "conflict", "The target is in use or has changed."),
    (6, "limit", "A size, count or time limit was exceeded."),
];

#[derive(Debug)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub kind: &'static str,
    /// A stable reason for callers in the same process (a UI), for a case where `kind` is too
    /// coarse. It is not part of the JSON-RPC answer. See the `REASON_*` constants.
    pub reason: Option<&'static str>,
    /// Facts that go with the error, for example the numbers of a limit. A JSON object. It is sent
    /// in the `data` of the JSON-RPC error, next to `kind`, so that agents get the numbers too (for example the list of versions when a save is refused
    /// because of a limit).
    pub details: Option<Value>,
}

/// `collab.connect`: the open document has changes that nobody saved, and the shared document
/// would replace it. The caller may ask the person, then pass `replace: true`.
pub const REASON_UNSAVED_CHANGES: &str = "unsaved_changes";
/// `collab.connect`: edits still wait for another document or another user. The caller may ask
/// the person, then pass `discard: true`.
pub const REASON_EDITS_WAIT: &str = "edits_wait";
/// `collab.connect`: the session is connected already.
pub const REASON_ALREADY_CONNECTED: &str = "already_connected";
/// `version.create` and `version.restore`: the new version would go over a limit of versions or
/// bytes. Nothing was saved or deleted. The details list the versions; the person decides.
pub const REASON_VERSION_LIMIT: &str = "version_limit";
/// `version.create`, `version.restore`, `version.delete`: the history of the file could not be
/// read completely. It is kept as it is and read only, until the person calls `version.repair`.
pub const REASON_HISTORY_READONLY: &str = "history_read_only";
/// `issue.delete`: an issue of somebody else (or with comments of somebody else) would be deleted.
/// Nothing was deleted. The details list the issues. The caller may ask the person, then pass
/// `confirm: true`.
pub const REASON_CONFIRM_NEEDED: &str = "confirm_needed";
/// `issue.import_bcf`: an issue of the file has the GUID of an issue in the document and other
/// content. Nothing was imported. The details list the conflicts and the options (`on_conflict`).
pub const REASON_BCF_CONFLICT: &str = "bcf_conflict";
/// `issue.import_bcf`: items of the file do not fit the limits (a comment, a picture, a camera).
/// Nothing was imported. The details list them; the person decides (`skip_items` or give up).
pub const REASON_BCF_UNFIT: &str = "bcf_unfit";
/// `file.save`: parts of the document that stay in the file it was opened from (pictures, versions)
/// cannot be found any more, because the file changed on disk. Nothing was written. `details`
/// has `missing` {file, pictures, history, other} and `consent_token`. The caller asks the person,
/// then saves again with `allow_missing` set to that token (not `true`).
pub const REASON_PARTS_MISSING: &str = "parts_missing";
/// `file.save`: the file at the path was changed by another program since the document was opened
/// from it (two instances, a sync conflict copy). Nothing was written. `details` has `file`,
/// `consent_token`, `ask_the_person`. The caller asks the person, then saves again with
/// `allow_overwrite` set to the token. A token is good for one save of that document as it is.
pub const REASON_FILE_CHANGED: &str = "file_changed";
/// `import.file`: the drawing has more entities than an import keeps for updates and the option
/// `base` is `auto`. `details` has `{entities, cap}`. The caller may ask the person, then pass
/// `base: "link"` or `base: "none"`.
pub const REASON_BASE_TOO_MANY: &str = "base_too_many";
/// `import.file`: the summaries of the entities are too large for a base and the option `base` is
/// `auto`. `details` has `{entities, bytes, cap}`. The choices are the same.
pub const REASON_BASE_TOO_LARGE: &str = "base_too_large";
/// `view.zoom`, `view.pan`, `view.camera`, `view.set`: the person is dragging or zooming the view
/// at this moment. The agent request was dropped and nothing moved. The caller may try again after
/// a moment: the person always wins over the agent (AGR-02).
pub const REASON_PERSON_NAVIGATING: &str = "person_navigating";
/// The same calls: the person turned off "Agent may move my view". Nothing moved. Do not try
/// again until the person says so; pointing (`view.highlight`) and `view.get` still work.
pub const REASON_VIEW_LOCKED: &str = "view_locked";

impl RpcError {
    pub fn new(code: i64, kind: &'static str, e: impl std::fmt::Display) -> Self {
        RpcError {
            code,
            message: e.to_string(),
            kind,
            reason: None,
            details: None,
        }
    }
    /// Adds facts to the error (see `details`).
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
    /// Adds a stable reason (see the `REASON_*` constants).
    pub fn with_reason(mut self, reason: &'static str) -> Self {
        self.reason = Some(reason);
        self
    }
    pub fn invalid_params(e: impl std::fmt::Display) -> Self {
        Self::new(-32602, "invalid_params", e)
    }
    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            -32601,
            "method_not_found",
            format!("unknown method: {method}"),
        )
    }
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::new(-32603, "internal", e)
    }
    pub fn app(e: impl std::fmt::Display) -> Self {
        Self::new(CODE_GENERIC, "error", e)
    }
    pub fn not_found(e: impl std::fmt::Display) -> Self {
        Self::new(CODE_NOT_FOUND, "not_found", e)
    }
    pub fn validation(e: impl std::fmt::Display) -> Self {
        Self::new(CODE_VALIDATION, "validation", e)
    }
    pub fn policy(e: impl std::fmt::Display) -> Self {
        Self::new(CODE_POLICY, "policy", e)
    }
    pub fn conflict(e: impl std::fmt::Display) -> Self {
        Self::new(CODE_CONFLICT, "conflict", e)
    }
    pub fn limit(e: impl std::fmt::Display) -> Self {
        Self::new(CODE_LIMIT, "limit", e)
    }
}

impl From<kadakas_core::command::CommandError> for RpcError {
    fn from(e: kadakas_core::command::CommandError) -> Self {
        use kadakas_core::command::CommandError as E;
        match e {
            E::NoObject(_)
            | E::NoLayer(_)
            | E::NoClass(_)
            | E::NoResource(_)
            | E::NoIssue(_)
            | E::NoComment(_) => Self::not_found(e),
            E::ResourceInUse(..) | E::LayerNotEmpty(_) | E::LayerLocked(_) => Self::conflict(e),
            E::MeshToSurface(ref c) => convert_rpc(c),
            E::LastLayer | E::Invalid(_) | E::Surface(_) => Self::validation(e),
        }
    }
}

/// A refusal of the conversion into a surface model, as an API error with the counts.
pub fn convert_rpc(e: &kadakas_core::surface::ConvertError) -> RpcError {
    match e {
        kadakas_core::surface::ConvertError::OverLimit { over, counts } => {
            let d = json!({
                "error_id": "mesh_too_large_for_surface",
                "over": over,
                "counts": counts,
            });
            RpcError::limit(e)
                .with_reason("mesh_too_large_for_surface")
                .with_details(d)
        }
        kadakas_core::surface::ConvertError::TooManyTriangles { count, limit } => {
            RpcError::limit(e)
                .with_reason("mesh_too_large_for_surface")
                .with_details(json!({
                    "error_id": "mesh_too_large_for_surface",
                    "triangles": count, "limit": limit,
                }))
        }
        kadakas_core::surface::ConvertError::Options(_) => bad(e),
        other => RpcError::validation(other)
            .with_reason("mesh_not_convertible")
            .with_details(json!({"error_id": "mesh_not_convertible"})),
    }
}

/// Malformed parameters (-32602).
pub fn bad(e: impl std::fmt::Display) -> RpcError {
    RpcError::invalid_params(e)
}

/// A generic failure (code 1).
pub fn app(e: impl std::fmt::Display) -> RpcError {
    RpcError::app(e)
}

/// The JSON text of an error response.
pub fn error_response(id: Value, code: i64, kind: &str, message: &str) -> String {
    response(id, code, message, json!({"kind": kind}))
}

/// The JSON text of the response to a failed call: the `data` has the facts of
/// `RpcError::details` (for example the numbers of a limit, the versions of a refused save), and
/// then the `kind`, the stable `reason` (`version_limit`, `history_read_only`, ...), a `feedback`
/// hint (see `feedback_api::hint`) and the mark that a method panicked in this session. The
/// facts come first, so that they cannot replace the others.
pub fn error_response_with(
    id: Value,
    e: &RpcError,
    feedback: Option<Value>,
    tainted: bool,
) -> String {
    let mut data = json!({});
    if let Some(Value::Object(d)) = &e.details {
        for (k, v) in d {
            data[k] = v.clone();
        }
    }
    data["kind"] = json!(e.kind);
    if let Some(r) = e.reason {
        data["reason"] = json!(r);
    }
    if let Some(f) = feedback {
        data["feedback"] = f;
    }
    // A method panicked in this session. The document may not be what the agent expects.
    if tainted {
        data["tainted"] = json!(true);
    }
    response(id, e.code, &e.message, data)
}

fn response(id: Value, code: i64, message: &str, data: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message, "data": data}})
        .to_string()
}

/// The `describe` entry for the error codes.
pub fn describe() -> Value {
    json!({
        "shape": "error: {code, message, data: {kind, ...}}. data.kind is a stable string. Match on it or on the code, never on the message. Some errors have more facts in data (a refused save lists the versions).",
        "codes": CODES.iter().map(|(c, k, m)| json!({"code": c, "kind": k, "meaning": m})).collect::<Vec<_>>(),
    })
}

/// An error of a mesh read as an API error: the limits are `limit` errors, the rest are
/// validation errors, all with a stable reason and the counts.
pub(crate) fn mesh_rpc(e: &kadakas_meshio::MeshError) -> crate::RpcError {
    let err = if e.is_limit() {
        crate::RpcError::limit(e)
    } else {
        crate::RpcError::validation(e)
    };
    let mut d = match e.details() {
        Value::Object(m) => m,
        _ => serde_json::Map::new(),
    };
    d.insert("error_id".into(), json!(e.kind()));
    err.with_reason(e.kind()).with_details(Value::Object(d))
}
