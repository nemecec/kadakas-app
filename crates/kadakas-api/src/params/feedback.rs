//! Parameters of the methods of the domain `feedback`.
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::*;

param_enum! {
    FeedbackSubmitKind {
        Bug = "bug",
        FeatureRequest = "feature_request",
        Question = "question",
    }
}

param_enum! {
    FeedbackSubmitSeverity {
        Low = "low",
        Medium = "medium",
        High = "high",
    }
}

// Parameters of `feedback.submit`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeedbackSubmitParams {
    pub kind: FeedbackSubmitKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wanted: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tried: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<FeedbackSubmitSeverity>,
    /// {excerpt?, screenshot?, calls?, log?, objects?, selection?, view?}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attach: Option<Either<bool, Obj>>,
    /// added by the host; the program name and its version
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<Obj>,
}

// Parameters of `feedback.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeedbackListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<Int>,
}

// Parameters of `feedback.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeedbackGetParams {
    pub id: String,
}

// Parameters of `feedback.withdraw`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeedbackWithdrawParams {
    pub id: String,
}

entries! {
    "feedback.submit" => FeedbackSubmitParams,
    "feedback.list" => FeedbackListParams,
    "feedback.get" => FeedbackGetParams,
    "feedback.withdraw" => FeedbackWithdrawParams,
}
