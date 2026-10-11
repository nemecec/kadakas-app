//! One loaded plug-in: the checked manifest, the hash of the module and the reset call.
//!
//! The module is untrusted code, so everything it says is checked here, the same way for every
//! engine: the manifest, the size of the answer, the JSON of the answer, and every shape.

use std::sync::{Arc, OnceLock};

use kadakas_core::DVec2;
use kadakas_core::curve::Vertex;
use kadakas_core::model::{Attributes, Color, Shape};
use kadakas_plugin_abi::tool::{MAX_HINT_CHARS, MAX_STATE_BYTES};
use kadakas_plugin_abi::{
    Geometry, Kind, Manifest, Modifiers, OutShape, ResetRequest, ResetResponse, ShapeData,
    ToolEvent, ToolRequest, ToolResponse, ToolResult,
};
use serde_json::{Map, Value};

use crate::runtime::{Limits, LoadedModule, PluginError, PluginRuntime};
use crate::trust::{self, Permit};

/// Largest line weight of a shape, in mm on paper.
pub const MAX_LINE_WEIGHT: f64 = 100.0;

/// One shape of an answer, as a core shape with its attributes.
#[derive(Clone, Debug, PartialEq)]
pub struct Generated {
    pub shape: Shape,
    pub attrs: Attributes,
}

/// One call of a tool: what the person did, and the state that the module gave back last time.
#[derive(Clone, Copy, Debug)]
pub struct ToolCall<'a> {
    pub event: &'a ToolEvent,
    pub modifiers: Modifiers,
    /// `Value::Null` at the start.
    pub state: &'a Value,
}

/// How a tool run ends. All shapes are checked.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolDone {
    /// Plain shapes for the drawing.
    Shapes(Vec<Generated>),
    /// A plug-in object of the same module. The host already ran `reset` for it.
    Object {
        /// All parameters, with defaults filled in.
        params: Map<String, Value>,
        geometry: Geometry,
        shapes: Vec<Generated>,
    },
}

/// The checked answer of one tool call.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolStep {
    /// What to send back with the next event.
    pub state: Value,
    /// Shapes to draw while the person works. They are not part of the drawing.
    pub preview: Vec<Generated>,
    /// A short text for the status bar. Control characters are gone.
    pub hint: String,
    /// Set when the run ends.
    pub done: Option<ToolDone>,
}

/// The bytes of a plug-in that was read from the store and not compiled or run yet.
struct Pending {
    wasm: Vec<u8>,
    runtime: Arc<dyn PluginRuntime>,
}

/// A plug-in module that is loaded and checked.
///
/// A plug-in from `Plugin::load` is compiled and has run `describe`. A plug-in from
/// [`Plugin::stored`] has the manifest that was stored when the module was installed, and **no
/// code has run**: the module is compiled, and `describe` runs, when someone first needs it, and
/// the answer must match the stored manifest. A page that lists its plug-ins at load time
/// therefore never runs a module that nobody trusts.
pub struct Plugin {
    manifest: Manifest,
    hash: String,
    size: usize,
    module: OnceLock<Box<dyn LoadedModule>>,
    pending: Option<Pending>,
    limits: Limits,
}

impl std::fmt::Debug for Plugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plugin")
            .field("id", &self.manifest.id)
            .field("hash", &self.hash)
            .finish()
    }
}

impl Plugin {
    /// Loads a module: checks its size, compiles it, reads its manifest and checks the manifest.
    /// Running `describe` happens inside the sandbox, so loading is safe before anyone trusts the
    /// module. The module is not allowed to ask for permissions: this host grants none.
    pub fn load(
        runtime: &dyn PluginRuntime,
        wasm: &[u8],
        limits: &Limits,
    ) -> Result<Plugin, PluginError> {
        if wasm.len() > limits.max_module_bytes {
            return Err(PluginError::Limit(format!(
                "the module has {} bytes. The limit is {}",
                wasm.len(),
                limits.max_module_bytes
            )));
        }
        if !wasm.starts_with(b"\0asm") {
            return Err(PluginError::Invalid(
                "this is not a WebAssembly module (a .wasm file starts with the bytes 00 61 73 6d)"
                    .into(),
            ));
        }
        scan(wasm, limits)?;
        let module = runtime.load(wasm, limits)?;
        let manifest = Manifest::from_bytes(&module.describe()?).map_err(PluginError::Invalid)?;
        if let Some(p) = manifest.permissions.first() {
            return Err(PluginError::Refused(format!(
                "the plug-in '{}' asks for the permission '{}'. This host grants no permissions, so it will not load the plug-in",
                manifest.id,
                p.name()
            )));
        }
        Ok(Plugin {
            manifest,
            hash: trust::hash(wasm),
            size: wasm.len(),
            module: OnceLock::from(module),
            pending: None,
            limits: *limits,
        })
    }

    /// A plug-in from the store. Checks the size and the structure of the bytes (parsing and
    /// validation only) and takes the manifest that was stored with the module. Runs nothing and
    /// compiles nothing.
    pub fn stored(
        runtime: Arc<dyn PluginRuntime>,
        wasm: Vec<u8>,
        manifest: Manifest,
        limits: &Limits,
    ) -> Result<Plugin, PluginError> {
        if wasm.len() > limits.max_module_bytes {
            return Err(PluginError::Limit(format!(
                "the module has {} bytes. The limit is {}",
                wasm.len(),
                limits.max_module_bytes
            )));
        }
        if !wasm.starts_with(b"\0asm") {
            return Err(PluginError::Invalid(
                "this is not a WebAssembly module (a .wasm file starts with the bytes 00 61 73 6d)"
                    .into(),
            ));
        }
        scan(&wasm, limits)?;
        manifest.validate().map_err(PluginError::Invalid)?;
        if let Some(p) = manifest.permissions.first() {
            return Err(PluginError::Refused(format!(
                "the plug-in '{}' asks for the permission '{}'. This host grants no permissions",
                manifest.id,
                p.name()
            )));
        }
        Ok(Plugin {
            manifest,
            hash: trust::hash(&wasm),
            size: wasm.len(),
            module: OnceLock::new(),
            pending: Some(Pending { wasm, runtime }),
            limits: *limits,
        })
    }

    /// True once the module is compiled and has run `describe`.
    pub fn is_active(&self) -> bool {
        self.module.get().is_some()
    }

    /// Compiles the module and runs `describe` if that did not happen yet, and checks that the
    /// module still says what was stored. A module that does not match is refused for good.
    pub fn activate(&self) -> Result<(), PluginError> {
        self.module().map(|_| ())
    }

    fn module(&self) -> Result<&dyn LoadedModule, PluginError> {
        if let Some(m) = self.module.get() {
            return Ok(m.as_ref());
        }
        let pending = self.pending.as_ref().ok_or_else(|| {
            PluginError::Invalid("the plug-in has no module".into())
        })?;
        let module = pending.runtime.load(&pending.wasm, &self.limits)?;
        let described = Manifest::from_bytes(&module.describe()?).map_err(PluginError::Invalid)?;
        if described != self.manifest {
            return Err(PluginError::Invalid(format!(
                "the module '{}' does not say what its stored description says. It was changed or the store was edited",
                self.manifest.id
            )));
        }
        let _ = self.module.set(module);
        Ok(self.module.get().expect("just set").as_ref())
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The SHA-256 of the module file, as hex. The trust store uses it.
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// The size of the module file in bytes.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Runs `reset` and returns the shapes. The `permit` must be for this module. `given` holds
    /// parameter values; missing ones take the defaults of the manifest.
    pub fn reset(
        &self,
        permit: &Permit,
        given: &Map<String, Value>,
        geometry: &Geometry,
    ) -> Result<Vec<Generated>, PluginError> {
        if !permit.allows(&self.hash) {
            return Err(PluginError::Refused(
                "this plug-in module is not trusted, so it did not run".into(),
            ));
        }
        let spec = match (&self.manifest.object, self.manifest.provides(Kind::Object)) {
            (Some(spec), true) => spec,
            _ => {
                return Err(PluginError::Refused(format!(
                    "the plug-in '{}' provides no object",
                    self.manifest.id
                )));
            }
        };
        if geometry.shape_type() != spec.shape_type {
            return Err(PluginError::Refused(format!(
                "the plug-in '{}' is placed with a {} geometry, not a {} one",
                self.manifest.id,
                spec.shape_type.name(),
                geometry.shape_type().name()
            )));
        }
        let params = self
            .manifest
            .resolve_params(given)
            .map_err(PluginError::Refused)?;
        let request = serde_json::to_vec(&ResetRequest::new(params, geometry.clone()))
            .expect("a request is always valid JSON");
        let bytes = self.module()?.reset(&request)?;
        // The engine limits the size already. A second check keeps every engine to the same rule.
        if bytes.len() > self.limits.max_output_bytes {
            return Err(PluginError::Limit(format!(
                "the answer has {} bytes. The limit is {}",
                bytes.len(),
                self.limits.max_output_bytes
            )));
        }
        match serde_json::from_slice::<ResetResponse>(&bytes) {
            Err(e) => Err(PluginError::BadOutput(format!(
                "the answer is not valid: {e}"
            ))),
            Ok(ResetResponse::Error { message }) => {
                Err(PluginError::Guest(clean_message(&message)))
            }
            Ok(ResetResponse::Ok { shapes }) => convert(&shapes, &self.limits),
        }
    }
}

impl Plugin {
    /// Runs one step of the tool. The `permit` must be for this module. `given` holds tool
    /// parameters; missing ones take the defaults of the manifest. Everything that the module
    /// answers is checked: the size of the state, the preview, the hint and the result.
    pub fn tool_step(
        &self,
        permit: &Permit,
        call: &ToolCall<'_>,
        given: &Map<String, Value>,
    ) -> Result<ToolStep, PluginError> {
        if !permit.allows(&self.hash) {
            return Err(PluginError::Refused(
                "this plug-in module is not trusted, so it did not run".into(),
            ));
        }
        if !self.manifest.provides(Kind::Tool) {
            return Err(PluginError::Refused(format!(
                "the plug-in '{}' provides no tool",
                self.manifest.id
            )));
        }
        if call.event.point().is_some_and(|p| {
            !p.iter()
                .all(|c| c.is_finite() && c.abs() <= kadakas_plugin_abi::MAX_VALUE)
        }) {
            return Err(PluginError::Refused(
                "the point of the event is not a finite number of reasonable size".into(),
            ));
        }
        let state_len = serde_json::to_vec(call.state).map_or(usize::MAX, |b| b.len());
        if state_len > MAX_STATE_BYTES {
            return Err(PluginError::Limit(format!(
                "the state of the tool has more than {MAX_STATE_BYTES} bytes"
            )));
        }
        let params = self
            .manifest
            .resolve_tool_params(given)
            .map_err(PluginError::Refused)?;
        let mut request = ToolRequest::new(call.event.clone(), call.state.clone(), params);
        request.modifiers = call.modifiers;
        let request = serde_json::to_vec(&request).expect("a request is always valid JSON");
        let bytes = self
            .module()?
            .tool(&request, matches!(call.event, ToolEvent::Move { .. }))?;
        if bytes.len() > self.limits.max_output_bytes {
            return Err(PluginError::Limit(format!(
                "the answer has {} bytes. The limit is {}",
                bytes.len(),
                self.limits.max_output_bytes
            )));
        }
        let reply = match serde_json::from_slice::<ToolResponse>(&bytes) {
            Err(e) => {
                return Err(PluginError::BadOutput(format!(
                    "the answer is not valid: {e}"
                )));
            }
            Ok(ToolResponse::Error { message }) => {
                return Err(PluginError::Guest(clean_message(&message)));
            }
            Ok(ToolResponse::Ok(reply)) => reply,
        };
        let state_len = serde_json::to_vec(&reply.state).map_or(usize::MAX, |b| b.len());
        if state_len > MAX_STATE_BYTES {
            return Err(PluginError::Limit(format!(
                "the state of the tool has {state_len} bytes. The limit is {MAX_STATE_BYTES}"
            )));
        }
        let preview = convert_shapes(&reply.preview, &self.limits, true)?;
        let hint: String = clean_message(&reply.hint)
            .chars()
            .take(MAX_HINT_CHARS)
            .collect();
        let done = match reply.result {
            None => None,
            Some(ToolResult::Shapes { shapes }) => Some(ToolDone::Shapes(convert_shapes(
                &shapes,
                &self.limits,
                false,
            )?)),
            Some(ToolResult::Object { params, geometry }) => {
                let spec = match (&self.manifest.object, self.manifest.provides(Kind::Object)) {
                    (Some(spec), true) => spec,
                    _ => {
                        return Err(PluginError::BadOutput(
                            "the tool placed an object, but the plug-in provides no object".into(),
                        ));
                    }
                };
                if geometry.shape_type() != spec.shape_type {
                    return Err(PluginError::BadOutput(format!(
                        "the tool placed a {} geometry, but the object needs a {} one",
                        geometry.shape_type().name(),
                        spec.shape_type.name()
                    )));
                }
                // The geometry comes from the module: check numbers and ranges again.
                let geometry = Geometry::from_value(spec.shape_type, &geometry.to_value())
                    .map_err(|e| PluginError::BadOutput(format!("the object geometry: {e}")))?;
                let params = self
                    .manifest
                    .resolve_params(&params)
                    .map_err(|e| PluginError::BadOutput(format!("the object parameters: {e}")))?;
                let shapes = self.reset(permit, &params, &geometry)?;
                Some(ToolDone::Object {
                    params,
                    geometry,
                    shapes,
                })
            }
        };
        Ok(ToolStep {
            state: reply.state,
            preview,
            hint,
            done,
        })
    }
}

/// Longest message of a plug-in that the host keeps, in characters.
pub const MAX_MESSAGE_CHARS: usize = 1000;

/// A message from a plug-in is shown to users and stored in the document. Control characters
/// (also line ends) become spaces, and the text is cut.
pub fn clean_message(message: &str) -> String {
    let text: String = message
        .chars()
        .filter(|c| !kadakas_plugin_abi::param::is_format_char(*c))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let text = text.trim();
    if text.chars().count() > MAX_MESSAGE_CHARS {
        let cut: String = text.chars().take(MAX_MESSAGE_CHARS).collect();
        format!("{cut}...")
    } else {
        text.to_string()
    }
}

/// Reads the structure of the module before it is compiled. Compiling is the costly step, and it
/// happens before anybody trusts the module, so a module with a huge function, many functions
/// or many locals is refused here.
pub fn scan(wasm: &[u8], limits: &Limits) -> Result<(), PluginError> {
    use wasmparser::{Parser, Payload};
    let invalid = |e: wasmparser::BinaryReaderError| PluginError::Invalid(e.to_string());
    for payload in Parser::new(0).parse_all(wasm) {
        match payload.map_err(invalid)? {
            Payload::CodeSectionStart { count, .. } if count as usize > limits.max_functions => {
                return Err(PluginError::Limit(format!(
                    "the module has {count} functions. The limit is {}",
                    limits.max_functions
                )));
            }
            Payload::CodeSectionEntry(body) => {
                // The range is in `u64`. A size that does not fit in `usize` counts as the
                // largest size, so it is refused.
                let range = body.range();
                let size =
                    usize::try_from(range.end.saturating_sub(range.start)).unwrap_or(usize::MAX);
                if size > limits.max_function_bytes {
                    return Err(PluginError::Limit(format!(
                        "a function has {size} bytes. The limit is {}",
                        limits.max_function_bytes
                    )));
                }
                let mut locals = 0usize;
                for group in body.get_locals_reader().map_err(invalid)? {
                    locals = locals.saturating_add(group.map_err(invalid)?.0 as usize);
                }
                if locals > limits.max_locals {
                    return Err(PluginError::Limit(format!(
                        "a function has {locals} local variables. The limit is {}",
                        limits.max_locals
                    )));
                }
            }
            _ => {}
        }
    }
    // Parsing does not prove that a module is well formed or inside the allowed features.
    crate::harden::validate(wasm)
}

fn v2(p: [f64; 2]) -> DVec2 {
    DVec2::new(p[0], p[1])
}

/// Turns the shapes of an answer into core shapes, and checks them against the limits.
pub fn convert(shapes: &[OutShape], limits: &Limits) -> Result<Vec<Generated>, PluginError> {
    convert_shapes(shapes, limits, false)
}

/// Like [`convert`]. With `allow_empty`, a list without shapes is fine (the preview of a tool).
pub fn convert_shapes(
    shapes: &[OutShape],
    limits: &Limits,
    allow_empty: bool,
) -> Result<Vec<Generated>, PluginError> {
    let bad = |i: usize, e: &str| PluginError::BadOutput(format!("shape {i}: {e}"));
    if shapes.is_empty() && !allow_empty {
        return Err(PluginError::BadOutput(
            "the plug-in returned no shapes".into(),
        ));
    }
    if shapes.len() > limits.max_shapes {
        return Err(PluginError::Limit(format!(
            "the plug-in returned {} shapes. The limit is {}",
            shapes.len(),
            limits.max_shapes
        )));
    }
    let mut vertices = 0usize;
    let mut out = Vec::with_capacity(shapes.len());
    for (i, s) in shapes.iter().enumerate() {
        let shape = match &s.shape {
            ShapeData::Line { a, b } => Shape::Line {
                a: v2(*a),
                b: v2(*b),
            },
            ShapeData::Rect { min, max } => Shape::Rect {
                min: v2(*min),
                max: v2(*max),
            },
            ShapeData::Circle { center, radius } => {
                if *radius <= 0.0 {
                    return Err(bad(i, "the radius must be greater than zero"));
                }
                Shape::Circle {
                    center: v2(*center),
                    radius: *radius,
                }
            }
            ShapeData::Arc {
                center,
                radius,
                start,
                end,
            } => {
                if *radius <= 0.0 {
                    return Err(bad(i, "the radius must be greater than zero"));
                }
                Shape::Arc {
                    center: v2(*center),
                    radius: *radius,
                    start: *start,
                    end: *end,
                }
            }
            ShapeData::Polyline { points, closed } => {
                if points.len() < 2 {
                    return Err(bad(i, "a polyline needs at least 2 points"));
                }
                vertices += points.len();
                if vertices > limits.max_vertices {
                    return Err(PluginError::Limit(format!(
                        "the polylines have more than {} vertices",
                        limits.max_vertices
                    )));
                }
                Shape::Polyline {
                    points: points.iter().map(|p| Vertex::corner(v2(*p))).collect(),
                    closed: *closed,
                }
            }
            ShapeData::Text { at, text, height } => {
                if text.chars().count() > kadakas_core::annot::MAX_TEXT_CHARS {
                    return Err(bad(
                        i,
                        &format!(
                            "a text has at most {} characters",
                            kadakas_core::annot::MAX_TEXT_CHARS
                        ),
                    ));
                }
                if *height <= 0.0 {
                    return Err(bad(i, "the text height must be greater than zero"));
                }
                Shape::Text {
                    at: v2(*at),
                    text: text.clone(),
                    height: *height,
                }
            }
        };
        let attrs = Attributes {
            pen: s.attrs.pen.map(Color),
            fill: s.attrs.fill.map(Color),
            line_weight: s.attrs.line_weight,
            ..Attributes::default()
        };
        if attrs
            .line_weight
            .is_some_and(|w| !(0.0..=MAX_LINE_WEIGHT).contains(&w))
        {
            return Err(bad(
                i,
                &format!("the line weight must be from 0 to {MAX_LINE_WEIGHT} mm"),
            ));
        }
        if !shape.is_finite() || !attrs.is_finite() || !shape.is_in_range() {
            return Err(bad(i, "a number is not finite or too large"));
        }
        out.push(Generated { shape, attrs });
    }
    Ok(out)
}
