//! Parameters of the methods of the domain `documents`.
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;

param_enum! {
    FileSaveFormat {
        Container = "container" | "zip" | "kad",
        Json = "json",
    }
}

// Parameters of `describe`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DescribeParams {
    /// a method name: returns the description and the JSON Schema of its parameters
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// true: add method_schemas, the JSON Schema of every method
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schemas: Option<bool>,
    /// true: add command_schema, the JSON Schema of the commands of execute
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<bool>,
}

// Parameters of `execute`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecuteParams {
    /// a command: {op, ...}; see describe.command_examples
    pub command: CommandJson,
}

// Parameters of `undo`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UndoParams {}

// Parameters of `redo`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RedoParams {}

// Parameters of `document.summary`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentSummaryParams {}

// Parameters of `document.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentGetParams {}

// Parameters of `document.load`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentLoadParams {
    /// a document as document.get returns it
    pub document: Obj,
    /// versions of the document (a document with a history field brings its own)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<Value>,
}

// Parameters of `file.save`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileSaveParams {
    /// the file; the extension decides the form unless format is given
    pub path: String,
    /// container (a ZIP file, the default) or json (one readable file)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<FileSaveFormat>,
    /// the consent token of a refusal because parts of the file cannot be found
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_missing: Option<String>,
    /// the consent token of a refusal because another program changed the file
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_overwrite: Option<String>,
}

// Parameters of `file.open`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileOpenParams {
    pub path: String,
}

// Parameters of `project.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectGetParams {}

// Parameters of `project.set`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectSetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawn_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

// Parameters of `project.new_guid`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectNewGuidParams {}

entries! {
    "describe" => DescribeParams,
    "execute" => ExecuteParams,
    "undo" => UndoParams,
    "redo" => RedoParams,
    "document.summary" => DocumentSummaryParams,
    "document.get" => DocumentGetParams,
    "document.load" => DocumentLoadParams,
    "file.save" => FileSaveParams,
    "file.open" => FileOpenParams,
    "project.get" => ProjectGetParams,
    "project.set" => ProjectSetParams,
    "project.new_guid" => ProjectNewGuidParams,
}
