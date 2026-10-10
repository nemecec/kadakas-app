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
pub const CODES: [(i64, &str, &str); 10] = [
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
}

impl RpcError {
    pub fn new(code: i64, kind: &'static str, e: impl std::fmt::Display) -> Self {
        RpcError {
            code,
            message: e.to_string(),
            kind,
        }
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

impl From<cadektor_core::command::CommandError> for RpcError {
    fn from(e: cadektor_core::command::CommandError) -> Self {
        use cadektor_core::command::CommandError as E;
        match e {
            E::NoObject(_) | E::NoLayer(_) | E::NoClass(_) | E::NoResource(_) => Self::not_found(e),
            E::ResourceInUse(..) | E::LayerNotEmpty(_) | E::LayerLocked(_) => Self::conflict(e),
            E::LastLayer | E::Invalid(_) => Self::validation(e),
        }
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
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message, "data": {"kind": kind}}})
        .to_string()
}

/// The `describe` entry for the error codes.
pub fn describe() -> Value {
    json!({
        "shape": "error: {code, message, data: {kind}}. data.kind is a stable string. Match on it or on the code, never on the message.",
        "codes": CODES.iter().map(|(c, k, m)| json!({"code": c, "kind": k, "meaning": m})).collect::<Vec<_>>(),
    })
}
