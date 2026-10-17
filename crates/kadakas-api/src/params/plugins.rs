//! Parameters of the methods of the domain `plugins`.
//!
//! The fields are read by serde and schemars only, so the compiler sees them as unused.
#![allow(dead_code)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;

param_enum! {
    PluginDefineShapeType {
        Point = "point",
        Linear = "linear",
        Rectangle = "rectangle",
        Path = "path",
    }
}

// Parameters of `plugin.define`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginDefineParams {
    pub name: String,
    pub shape_type: PluginDefineShapeType,
    /// [{name, label?, type: number | length | text | bool | choice | color | record, default?, min?, max?, options?, fields?}]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Vec<Obj>>,
    /// defines reset(params, geometry) and returns a list of shapes
    pub script: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 10 to 60000
    #[schemars(extend("default" = 5000))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<Int>,
}

// Parameters of `plugin.update`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["name"]}]}]))]
pub struct PluginUpdateParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape_type: Option<PluginDefineShapeType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Vec<Obj>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<Int>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<bool>,
}

// Parameters of `plugin.delete`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["name"]}]}]))]
pub struct PluginDeleteParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `plugin.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginListParams {}

// Parameters of `plugin.get`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["name"]}]}]))]
pub struct PluginGetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Parameters of `plugin.examples`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginExamplesParams {}

// Parameters of `plugin.install_example`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginInstallExampleParams {
    pub name: String,
}

// Parameters of `plugin.place`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginPlaceParams {
    /// an ID or a name
    pub definition: ResourceRef,
    /// {name: value}; missing values use the defaults
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// point: where
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    /// point: radians
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Num>,
    /// linear
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<Point>,
    /// linear
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<Point>,
    /// rectangle
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Point>,
    /// rectangle
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Point>,
    /// path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<Point>>,
    /// path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
}

// Parameters of `plugin.set_params`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginSetParamsParams {
    pub id: ObjectRef,
    pub params: Obj,
}

// Parameters of `plugin.reset`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["ids"]}, {"required": ["definition"]}]}]))]
pub struct PluginResetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<ResourceRef>,
}

// Parameters of `plugin.object`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginObjectParams {
    pub id: ObjectRef,
}

// Parameters of `plugin.preview`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginPreviewParams {
    pub definition: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<Point>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
}

// Parameters of `script.info`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptInfoParams {}

// Parameters of `script.run`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["path"]}, {"required": ["source"]}]}]))]
pub struct ScriptRunParams {
    /// must end in .py and be inside an allowed folder
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
}

// Parameters of `script.status`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptStatusParams {
    pub run: Int,
    /// line number
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<Int>,
}

// Parameters of `script.stop`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptStopParams {
    pub run: Int,
}

// Parameters of `script.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptListParams {}

// Parameters of `plugins.info`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginsInfoParams {}

// Parameters of `plugins.list`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginsListParams {}

// Parameters of `plugins.install`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginsInstallParams {
    pub path: String,
}

// Parameters of `plugins.remove`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginsRemoveParams {
    pub plugin: ResourceRef,
}

// Parameters of `plugins.place`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginsPlaceParams {
    pub plugin: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<Point>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
}

// Parameters of `plugins.set_params`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginsSetParamsParams {
    pub id: ObjectRef,
    pub params: Obj,
}

// Parameters of `plugins.reset`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [{"anyOf": [{"required": ["id"]}, {"required": ["ids"]}, {"required": ["plugin"]}]}]))]
pub struct PluginsResetParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<ObjectRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<ResourceRef>,
}

// Parameters of `plugins.object`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginsObjectParams {
    pub id: ObjectRef,
}

// Parameters of `plugins.preview`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginsPreviewParams {
    pub plugin: ResourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Obj>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<Point>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
}

// Parameters of `plugins.tool_step`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginsToolStepParams {
    pub plugin: ResourceRef,
    /// {type: start | move | click | finish, at?: [x, y]}
    pub event: Obj,
    /// {shift?, ctrl?, alt?}
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modifiers: Option<Obj>,
    /// what the last step returned; null at the start
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<Value>,
    /// tool options
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Obj>,
    #[schemars(extend("default" = true))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<LayerRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<ClassRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

param_enum! {
    /// How a JavaScript program may change the document.
    ScriptEvalMode {
        Edit = "edit",
        Read = "read",
    }
}

/// Parameters of `script.eval`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptEvalParams {
    /// the text of the program: the body of a function; `args` is the parameter args, `return` gives the result
    pub code: String,
    /// read: only read-only methods may be called, any other is refused before it runs
    #[schemars(extend("default" = "edit"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ScriptEvalMode>,
    /// any JSON; the program sees it as `args`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    /// seeds Math.random (0 to 4294967295); the same seed gives the same numbers
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<Int>,
}

// Parameters of `script.js_info`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptJsInfoParams {}

entries! {
    "script.eval" => ScriptEvalParams,
    "script.js_info" => ScriptJsInfoParams,
    "plugin.define" => PluginDefineParams,
    "plugin.update" => PluginUpdateParams,
    "plugin.delete" => PluginDeleteParams,
    "plugin.list" => PluginListParams,
    "plugin.get" => PluginGetParams,
    "plugin.examples" => PluginExamplesParams,
    "plugin.install_example" => PluginInstallExampleParams,
    "plugin.place" => PluginPlaceParams,
    "plugin.set_params" => PluginSetParamsParams,
    "plugin.reset" => PluginResetParams,
    "plugin.object" => PluginObjectParams,
    "plugin.preview" => PluginPreviewParams,
    "script.info" => ScriptInfoParams,
    "script.run" => ScriptRunParams,
    "script.status" => ScriptStatusParams,
    "script.stop" => ScriptStopParams,
    "script.list" => ScriptListParams,
    "plugins.info" => PluginsInfoParams,
    "plugins.list" => PluginsListParams,
    "plugins.install" => PluginsInstallParams,
    "plugins.remove" => PluginsRemoveParams,
    "plugins.place" => PluginsPlaceParams,
    "plugins.set_params" => PluginsSetParamsParams,
    "plugins.reset" => PluginsResetParams,
    "plugins.object" => PluginsObjectParams,
    "plugins.preview" => PluginsPreviewParams,
    "plugins.tool_step" => PluginsToolStepParams,
}
