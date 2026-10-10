//! One loaded plug-in: the checked manifest, the hash of the module and the reset call.
//!
//! The module is untrusted code, so everything it says is checked here, the same way for every
//! engine: the manifest, the size of the answer, the JSON of the answer, and every shape.

use kadakas_core::DVec2;
use kadakas_core::curve::Vertex;
use kadakas_core::model::{Attributes, Color, Shape};
use kadakas_plugin_abi::{
    Geometry, Kind, Manifest, OutShape, ResetRequest, ResetResponse, ShapeData,
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

/// A plug-in module that is loaded and checked.
pub struct Plugin {
    manifest: Manifest,
    hash: String,
    size: usize,
    module: Box<dyn LoadedModule>,
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
            module,
            limits: *limits,
        })
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
        let bytes = self.module.reset(&request)?;
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

/// Longest message of a plug-in that the host keeps, in characters.
pub const MAX_MESSAGE_CHARS: usize = 1000;

/// A message from a plug-in is shown to users and stored in the document. Control characters
/// (also line ends) become spaces, and the text is cut.
pub fn clean_message(message: &str) -> String {
    let text: String = message
        .chars()
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
                let size = body.range().len();
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
    Ok(())
}

fn v2(p: [f64; 2]) -> DVec2 {
    DVec2::new(p[0], p[1])
}

/// Turns the shapes of an answer into core shapes, and checks them against the limits.
pub fn convert(shapes: &[OutShape], limits: &Limits) -> Result<Vec<Generated>, PluginError> {
    let bad = |i: usize, e: &str| PluginError::BadOutput(format!("shape {i}: {e}"));
    if shapes.is_empty() {
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
