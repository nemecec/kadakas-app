//! The messages of a tool plug-in (kind `tool`, ABI version 2).
//!
//! A tool lets a person draw with the mouse. The host sends what the person does, and the module
//! answers with a preview and, at the end, a result. Everything is JSON, as for `reset`.
//!
//! Every call runs in a new instance, so the module cannot keep a variable between two calls. It
//! keeps its progress in `state`: the answer holds a JSON value, and the host sends the same
//! value back with the next event. The host treats the state as data. It does not look at it,
//! it caps its size ([`MAX_STATE_BYTES`]), and it drops it when the person cancels the tool.
//!
//! Request: `{"event": {"type": "click", "at": [x, y]}, "modifiers": {"shift": false, "ctrl": false, "alt": false},
//! "state": null, "params": {...}, "units": "mm"}`.
//!
//! Events:
//!
//! | Event | When |
//! |---|---|
//! | `start` | The person chose the tool. `state` is `null`. |
//! | `move` | The pointer moved. `at` is in drawing coordinates (mm). |
//! | `click` | The person clicked. `at` is in drawing coordinates (mm). |
//! | `finish` | The person pressed Enter or double-clicked. |
//!
//! Escape never reaches the module: the host drops the state and the preview.
//!
//! Answer: `{"status": "ok", "state": ..., "preview": [shapes], "hint": "text", "result": null}`
//! or `{"status": "error", "message": "..."}`. A `result` ends the tool run. It is one of:
//!
//! - `{"type": "shapes", "shapes": [...]}`: plain shapes. The host adds them to the drawing.
//! - `{"type": "object", "params": {...}, "geometry": {...}}`: a plug-in object of this same
//!   module (the module must also provide the kind `object`). The host checks the parameters and
//!   the geometry, calls `reset` and adds the group. A module cannot place the object of another
//!   module.
//!
//! The preview is drawn on the canvas only. It never enters the drawing.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::message::{Geometry, OutShape};

/// Largest `state` that the host keeps between two calls, as JSON bytes.
pub const MAX_STATE_BYTES: usize = 64 << 10;

/// Longest hint, in characters.
pub const MAX_HINT_CHARS: usize = 200;

/// What the person did.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolEvent {
    /// The tool was chosen.
    Start,
    /// The pointer moved to a point.
    Move { at: [f64; 2] },
    /// The person clicked at a point.
    Click { at: [f64; 2] },
    /// The person ended the input (Enter or a double click).
    Finish,
}

impl ToolEvent {
    /// The name of the event, as in the JSON.
    pub fn name(&self) -> &'static str {
        match self {
            ToolEvent::Start => "start",
            ToolEvent::Move { .. } => "move",
            ToolEvent::Click { .. } => "click",
            ToolEvent::Finish => "finish",
        }
    }

    /// The point of the event, if it has one.
    pub fn point(&self) -> Option<[f64; 2]> {
        match self {
            ToolEvent::Move { at } | ToolEvent::Click { at } => Some(*at),
            _ => None,
        }
    }
}

/// The state of the modifier keys while the event happened.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifiers {
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
}

/// The request of a tool call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolRequest {
    pub event: ToolEvent,
    #[serde(default)]
    pub modifiers: Modifiers,
    /// What the module answered before in `state`, or `null` at the start.
    #[serde(default)]
    pub state: Value,
    /// The tool parameters (the schema of `tool.params` in the manifest), defaults filled in.
    #[serde(default)]
    pub params: Map<String, Value>,
    /// Always `mm`.
    #[serde(default = "mm")]
    pub units: String,
}

fn mm() -> String {
    "mm".into()
}

impl ToolRequest {
    pub fn new(event: ToolEvent, state: Value, params: Map<String, Value>) -> Self {
        ToolRequest {
            event,
            modifiers: Modifiers::default(),
            state,
            params,
            units: mm(),
        }
    }
}

/// How a tool run ends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResult {
    /// Plain shapes for the drawing.
    Shapes { shapes: Vec<OutShape> },
    /// A plug-in object of this module. `params` need not hold all parameters.
    Object {
        #[serde(default)]
        params: Map<String, Value>,
        geometry: Geometry,
    },
}

/// What the module answers when all is well.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolReply {
    /// What the module wants back with the next event.
    #[serde(default)]
    pub state: Value,
    /// Shapes to draw while the person works. Not part of the drawing.
    #[serde(default)]
    pub preview: Vec<OutShape>,
    /// A short text for the status bar, such as "Click the end point".
    #[serde(default)]
    pub hint: String,
    /// Set when the run ends.
    #[serde(default)]
    pub result: Option<ToolResult>,
}

/// The answer of a tool call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ToolResponse {
    Ok(ToolReply),
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::ShapeData;
    use serde_json::json;

    #[test]
    fn a_request_reads_and_writes_the_documented_json() {
        let text = r#"{"event":{"type":"click","at":[1,2]},"modifiers":{"shift":true},"state":{"a":1},"params":{"n":3}}"#;
        let r: ToolRequest = serde_json::from_str(text).unwrap();
        assert_eq!(r.event, ToolEvent::Click { at: [1.0, 2.0] });
        assert!(r.modifiers.shift && !r.modifiers.ctrl && !r.modifiers.alt);
        assert_eq!(r.state, json!({"a": 1}));
        assert_eq!(r.units, "mm");
        let again: ToolRequest = serde_json::from_slice(&serde_json::to_vec(&r).unwrap()).unwrap();
        assert_eq!(again, r);
        let bare: ToolRequest = serde_json::from_str(r#"{"event":{"type":"start"}}"#).unwrap();
        assert!(bare.state.is_null() && bare.params.is_empty());
        let made = ToolRequest::new(ToolEvent::Finish, Value::Null, Map::new());
        assert_eq!(made.units, "mm");
    }

    #[test]
    fn events_have_names_and_points() {
        assert_eq!(ToolEvent::Start.name(), "start");
        assert_eq!(ToolEvent::Finish.name(), "finish");
        assert_eq!(ToolEvent::Move { at: [1.0, 2.0] }.name(), "move");
        assert_eq!(ToolEvent::Click { at: [3.0, 4.0] }.name(), "click");
        assert_eq!(
            ToolEvent::Click { at: [3.0, 4.0] }.point(),
            Some([3.0, 4.0])
        );
        assert_eq!(ToolEvent::Move { at: [1.0, 2.0] }.point(), Some([1.0, 2.0]));
        assert_eq!(ToolEvent::Start.point(), None);
        assert_eq!(ToolEvent::Finish.point(), None);
    }

    #[test]
    fn an_answer_reads_and_writes_the_documented_json() {
        let shapes = ToolResponse::Ok(ToolReply {
            state: json!({"start": [0, 0]}),
            preview: vec![OutShape::new(ShapeData::Line {
                a: [0.0, 0.0],
                b: [1.0, 1.0],
            })],
            hint: "Click".into(),
            result: Some(ToolResult::Shapes { shapes: vec![] }),
        });
        let bytes = serde_json::to_vec(&shapes).unwrap();
        assert_eq!(
            serde_json::from_slice::<ToolResponse>(&bytes).unwrap(),
            shapes
        );
        let minimal: ToolResponse = serde_json::from_str(r#"{"status":"ok"}"#).unwrap();
        assert_eq!(minimal, ToolResponse::Ok(ToolReply::default()));
        let object: ToolResponse = serde_json::from_str(
            r#"{"status":"ok","result":{"type":"object","geometry":{"type":"point","at":[1,2]}}}"#,
        )
        .unwrap();
        assert!(matches!(
            object,
            ToolResponse::Ok(ToolReply { result: Some(ToolResult::Object { params, .. }), .. }) if params.is_empty()
        ));
        let err: ToolResponse =
            serde_json::from_str(r#"{"status":"error","message":"no"}"#).unwrap();
        assert_eq!(
            err,
            ToolResponse::Error {
                message: "no".into()
            }
        );
        assert!(
            serde_json::from_str::<ToolResponse>(r#"{"status":"ok","result":{"type":"x"}}"#)
                .is_err()
        );
    }
}
