//! Plug-in objects: the definition, the parameters and the reset protocol.
//!
//! A definition has a name, a shape type, a list of parameters and a Python script. The script
//! defines `reset(params, geometry)` and returns a list of shapes. The app calls it again each time
//! a parameter or the path changes. The app keeps the shapes of the last good run.
//!
//! The reset protocol is JSON over the standard input and output of one Python process:
//! the request is `{event: "reset", name, shape_type, params, geometry, units, script}` and the
//! answer is one line, `{ok: true, shapes: [...]}` or `{ok: false, error, traceback}`.
//! A time limit and size limits apply (see [`ResetLimits`]).

use std::time::Duration;

use cadektor_core::model::{Attributes, MAX_COORD, Shape};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::exec::{self, ExecError, Limits};
use crate::python::Interpreter;

/// The runner script. It is also part of the Python package.
pub const RUNNER: &str = include_str!("../../../python/cadektor/plugin_runner.py");

/// Most parameters in one definition.
pub const MAX_PARAMS: usize = 64;
/// Longest script, in bytes.
pub const MAX_SCRIPT: usize = 256 * 1024;
/// Longest text value, in characters.
pub const MAX_TEXT: usize = 10_000;
/// Most points in a linear or path object.
pub const MAX_POINTS: usize = 100_000;

/// What kind of geometry places the object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeType {
    /// One point, with a rotation.
    Point,
    /// A straight line from a start to an end point.
    Linear,
    /// A rectangle (two corners, parallel to the axes).
    Rectangle,
    /// A 2D path of points, open or closed.
    Path,
}

impl ShapeType {
    pub fn name(self) -> &'static str {
        match self {
            ShapeType::Point => "point",
            ShapeType::Linear => "linear",
            ShapeType::Rectangle => "rectangle",
            ShapeType::Path => "path",
        }
    }
}

/// The type of a parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    /// A plain number.
    Number,
    /// A length in mm.
    Length,
    Text,
    Bool,
    /// One of a list of texts (`options`).
    Choice,
    /// A color as `#rrggbb` or `#rrggbbaa`.
    Color,
    /// A group of fields (`fields`), like a record. The value is an object with one entry per field.
    Record,
}

/// One parameter of a definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(rename = "type")]
    pub ty: ParamType,
    /// The default value. Missing: 0, empty text, false, the first option, black, or the defaults of the fields.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub default: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<ParamDef>,
}

fn valid_name(n: &str) -> bool {
    let mut c = n.chars();
    c.next()
        .is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
        && n.len() <= 64
}

fn normalize_color(s: &str) -> Option<String> {
    let h = s.strip_prefix('#')?;
    ((h.len() == 6 || h.len() == 8) && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("#{}", h.to_ascii_lowercase()))
}

impl ParamDef {
    /// The default value of the parameter (checked by [`ParamDef::validate`]).
    pub fn default_value(&self) -> Value {
        if !self.default.is_null() {
            return self.default.clone();
        }
        match self.ty {
            ParamType::Number | ParamType::Length => {
                let lo = self.min.unwrap_or(f64::MIN);
                let hi = self.max.unwrap_or(f64::MAX);
                json!(0.0_f64.clamp(lo, hi))
            }
            ParamType::Text => json!(""),
            ParamType::Bool => json!(false),
            ParamType::Choice => json!(self.options.first().cloned().unwrap_or_default()),
            ParamType::Color => json!("#000000"),
            ParamType::Record => Value::Object(
                self.fields
                    .iter()
                    .map(|f| (f.name.clone(), f.default_value()))
                    .collect(),
            ),
        }
    }

    /// Checks the definition of the parameter.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_in(false)
    }

    fn validate_in(&self, nested: bool) -> Result<(), String> {
        if !valid_name(&self.name) {
            return Err(format!(
                "parameter name '{}' must start with a letter or _ and use only letters, digits and _",
                self.name
            ));
        }
        let n = &self.name;
        match self.ty {
            ParamType::Choice if self.options.is_empty() => {
                return Err(format!("parameter {n}: a choice needs options"));
            }
            ParamType::Record if nested => {
                return Err(format!("parameter {n}: records cannot hold records"));
            }
            ParamType::Record if self.fields.is_empty() => {
                return Err(format!("parameter {n}: a record needs fields"));
            }
            _ => {}
        }
        if self.ty != ParamType::Record && !self.fields.is_empty() {
            return Err(format!("parameter {n}: only a record has fields"));
        }
        if self.ty != ParamType::Choice && !self.options.is_empty() {
            return Err(format!("parameter {n}: only a choice has options"));
        }
        for v in [self.min, self.max].into_iter().flatten() {
            if !v.is_finite() {
                return Err(format!("parameter {n}: min and max must be finite"));
            }
        }
        if self.min.zip(self.max).is_some_and(|(a, b)| a > b) {
            return Err(format!("parameter {n}: min is greater than max"));
        }
        let mut seen = std::collections::HashSet::new();
        for f in &self.fields {
            f.validate_in(true)?;
            if !seen.insert(&f.name) {
                return Err(format!("parameter {n}: field '{}' appears twice", f.name));
            }
        }
        self.coerce(&self.default_value())
            .map(|_| ())
            .map_err(|e| format!("parameter {n}: default: {e}"))
    }

    /// Checks a value against the type, and returns it in its normal form.
    pub fn coerce(&self, v: &Value) -> Result<Value, String> {
        match self.ty {
            ParamType::Number | ParamType::Length => {
                let x = v.as_f64().ok_or("must be a number")?;
                if !x.is_finite() || x.abs() > MAX_COORD {
                    return Err("must be a finite number of reasonable size".into());
                }
                if self.min.is_some_and(|m| x < m) || self.max.is_some_and(|m| x > m) {
                    return Err(format!(
                        "{x} is outside the range {} to {}",
                        self.min.map_or("-inf".into(), |m| m.to_string()),
                        self.max.map_or("inf".into(), |m| m.to_string())
                    ));
                }
                Ok(json!(x))
            }
            ParamType::Text => {
                let s = v.as_str().ok_or("must be text")?;
                if s.chars().count() > MAX_TEXT {
                    return Err(format!("text is longer than {MAX_TEXT} characters"));
                }
                Ok(json!(s))
            }
            ParamType::Bool => v
                .as_bool()
                .map(Value::Bool)
                .ok_or("must be true or false".into()),
            ParamType::Choice => {
                let s = v.as_str().ok_or("must be text")?;
                if self.options.iter().any(|o| o == s) {
                    Ok(json!(s))
                } else {
                    Err(format!("'{s}' is not one of: {}", self.options.join(", ")))
                }
            }
            ParamType::Color => {
                let s = v.as_str().ok_or("must be a color like #rrggbb")?;
                normalize_color(s)
                    .map(Value::String)
                    .ok_or_else(|| format!("'{s}' is not a color like #rrggbb"))
            }
            ParamType::Record => {
                let m = v.as_object().ok_or("must be an object")?;
                resolve(&self.fields, m).map(Value::Object)
            }
        }
    }
}

/// Fills in defaults and checks every value. Unknown names are an error.
pub fn resolve(
    defs: &[ParamDef],
    given: &Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    if let Some(k) = given.keys().find(|k| !defs.iter().any(|d| &d.name == *k)) {
        return Err(format!(
            "unknown parameter '{k}'. Parameters: {}",
            defs.iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut out = Map::new();
    for d in defs {
        let v = match given.get(&d.name) {
            Some(v) => d
                .coerce(v)
                .map_err(|e| format!("parameter {}: {e}", d.name))?,
            None => d
                .coerce(&d.default_value())
                .map_err(|e| format!("parameter {}: {e}", d.name))?,
        };
        out.insert(d.name.clone(), v);
    }
    Ok(out)
}

/// A plug-in object definition. In a document it is a resource with the JSON `type` `plugin_object`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginDef {
    pub shape_type: ShapeType,
    #[serde(default)]
    pub params: Vec<ParamDef>,
    pub script: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Time limit of one reset in ms (default 5000, at most 60000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// The `type` of the resource.
pub const RESOURCE_TYPE: &str = "plugin_object";

impl PluginDef {
    /// Reads a definition from the JSON of a resource body (the `type` key is ignored).
    pub fn from_json(v: &Value) -> Result<PluginDef, String> {
        let d: PluginDef = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
        d.validate()?;
        Ok(d)
    }

    /// The JSON of the resource body.
    pub fn to_json(&self) -> Value {
        let mut v = serde_json::to_value(self).expect("json");
        v["type"] = RESOURCE_TYPE.into();
        v
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.script.len() > MAX_SCRIPT {
            return Err(format!("the script is longer than {MAX_SCRIPT} bytes"));
        }
        if self.params.len() > MAX_PARAMS {
            return Err(format!("a definition has at most {MAX_PARAMS} parameters"));
        }
        if self.timeout_ms.is_some_and(|t| !(10..=60_000).contains(&t)) {
            return Err("timeout_ms must be from 10 to 60000".into());
        }
        let mut seen = std::collections::HashSet::new();
        for p in &self.params {
            p.validate()?;
            if !seen.insert(&p.name) {
                return Err(format!("parameter '{}' appears twice", p.name));
            }
        }
        Ok(())
    }

    /// The parameter values, with defaults, checked against the schema.
    pub fn resolve_params(&self, given: &Map<String, Value>) -> Result<Map<String, Value>, String> {
        resolve(&self.params, given)
    }
}

/// Where the object sits. Coordinates are document coordinates in mm.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    Point { at: [f64; 2], rotation: f64 },
    Linear { a: [f64; 2], b: [f64; 2] },
    Rectangle { min: [f64; 2], max: [f64; 2] },
    Path { points: Vec<[f64; 2]>, closed: bool },
}

fn pt(v: &Value, what: &str) -> Result<[f64; 2], String> {
    let a = v.as_array().filter(|a| a.len() == 2);
    let x = a.and_then(|a| Some([a[0].as_f64()?, a[1].as_f64()?]));
    x.filter(|p| p.iter().all(|c| c.is_finite() && c.abs() <= MAX_COORD))
        .ok_or_else(|| format!("{what} must be [x, y] with finite numbers"))
}

impl Geometry {
    /// Reads the geometry for a shape type. `point`: `{at, rotation?}`. `linear`: `{a, b}` (or
    /// `points` with two entries). `rectangle`: `{min, max}`. `path`: `{points, closed?}`.
    pub fn from_json(ty: ShapeType, v: &Value) -> Result<Geometry, String> {
        let m = v.as_object().ok_or("geometry must be an object")?;
        let get = |k: &str| m.get(k).ok_or(format!("geometry needs '{k}'"));
        match ty {
            ShapeType::Point => {
                let rotation = m.get("rotation").map_or(Ok(0.0), |r| {
                    r.as_f64()
                        .filter(|r| r.is_finite())
                        .ok_or("rotation must be a finite number")
                })?;
                Ok(Geometry::Point {
                    at: pt(get("at")?, "at")?,
                    rotation,
                })
            }
            ShapeType::Linear => {
                if let Some(Value::Array(p)) = m.get("points") {
                    if p.len() != 2 {
                        return Err("a linear object has exactly 2 points".into());
                    }
                    return Ok(Geometry::Linear {
                        a: pt(&p[0], "points[0]")?,
                        b: pt(&p[1], "points[1]")?,
                    });
                }
                Ok(Geometry::Linear {
                    a: pt(get("a")?, "a")?,
                    b: pt(get("b")?, "b")?,
                })
            }
            ShapeType::Rectangle => {
                let (a, b) = (pt(get("min")?, "min")?, pt(get("max")?, "max")?);
                Ok(Geometry::Rectangle {
                    min: [a[0].min(b[0]), a[1].min(b[1])],
                    max: [a[0].max(b[0]), a[1].max(b[1])],
                })
            }
            ShapeType::Path => {
                let arr = get("points")?.as_array().ok_or("points must be a list")?;
                if arr.len() < 2 {
                    return Err("a path needs at least 2 points".into());
                }
                if arr.len() > MAX_POINTS {
                    return Err(format!("a path has at most {MAX_POINTS} points"));
                }
                let points = arr
                    .iter()
                    .enumerate()
                    .map(|(i, p)| pt(p, &format!("points[{i}]")))
                    .collect::<Result<Vec<_>, _>>()?;
                let closed = m.get("closed").map_or(Ok(false), |c| {
                    c.as_bool().ok_or("closed must be true or false")
                })?;
                Ok(Geometry::Path { points, closed })
            }
        }
    }

    pub fn to_json(&self) -> Value {
        match self {
            Geometry::Point { at, rotation } => json!({"at": at, "rotation": rotation}),
            Geometry::Linear { a, b } => json!({"a": a, "b": b}),
            Geometry::Rectangle { min, max } => json!({"min": min, "max": max}),
            Geometry::Path { points, closed } => json!({"points": points, "closed": closed}),
        }
    }

    /// Moves the geometry.
    pub fn translate(&mut self, dx: f64, dy: f64) {
        let mv = |p: &mut [f64; 2]| {
            p[0] += dx;
            p[1] += dy;
        };
        match self {
            Geometry::Point { at, .. } => mv(at),
            Geometry::Linear { a, b } => {
                mv(a);
                mv(b);
            }
            Geometry::Rectangle { min, max } => {
                mv(min);
                mv(max);
            }
            Geometry::Path { points, .. } => points.iter_mut().for_each(mv),
        }
    }
}

/// Limits of one reset.
#[derive(Clone, Copy, Debug)]
pub struct ResetLimits {
    pub timeout: Duration,
    /// Most bytes in the answer.
    pub max_output: usize,
    /// Most shapes in the answer.
    pub max_shapes: usize,
    /// Most vertices in all polylines of the answer together.
    pub max_vertices: usize,
}

impl Default for ResetLimits {
    fn default() -> Self {
        ResetLimits {
            timeout: Duration::from_secs(5),
            max_output: 2 << 20,
            max_shapes: 2_000,
            max_vertices: 100_000,
        }
    }
}

/// One shape that a script returned.
#[derive(Clone, Debug, PartialEq)]
pub struct Generated {
    pub shape: Shape,
    pub attrs: Attributes,
}

/// The shape kinds that a script may return.
pub const ALLOWED_KINDS: &[&str] = &["line", "rect", "circle", "arc", "polyline", "text"];

/// Turns the `shapes` of an answer into shapes, and checks them against the limits.
pub fn parse_shapes(shapes: &Value, limits: &ResetLimits) -> Result<Vec<Generated>, String> {
    let list = shapes
        .as_array()
        .ok_or("the script must return a list of shapes")?;
    if list.is_empty() {
        return Err("the script returned no shapes".into());
    }
    if list.len() > limits.max_shapes {
        return Err(format!(
            "the script returned {} shapes. The limit is {}",
            list.len(),
            limits.max_shapes
        ));
    }
    let mut vertices = 0usize;
    let mut out = Vec::with_capacity(list.len());
    for (i, item) in list.iter().enumerate() {
        let at = |e: String| format!("shape {i}: {e}");
        let mut obj = item
            .as_object()
            .cloned()
            .ok_or_else(|| at("must be an object like {\"kind\": \"line\", ...}".into()))?;
        let attrs = match obj.remove("attrs") {
            Some(a) => {
                serde_json::from_value::<Attributes>(a).map_err(|e| at(format!("attrs: {e}")))?
            }
            None => Attributes::default(),
        };
        let kind = obj
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if !ALLOWED_KINDS.contains(&kind.as_str()) {
            return Err(at(format!(
                "kind '{kind}' is not allowed. Use one of: {}",
                ALLOWED_KINDS.join(", ")
            )));
        }
        let shape: Shape =
            serde_json::from_value(Value::Object(obj)).map_err(|e| at(e.to_string()))?;
        if let Shape::Polyline { points, .. } = &shape {
            vertices += points.len();
            if vertices > limits.max_vertices {
                return Err(format!(
                    "the polylines have more than {} vertices",
                    limits.max_vertices
                ));
            }
        }
        if !shape.is_finite() || !attrs.is_finite() || !shape.is_in_range() {
            return Err(at("a number is not finite or too large".into()));
        }
        out.push(Generated { shape, attrs });
    }
    Ok(out)
}

/// What can go wrong in a reset.
#[derive(Debug, PartialEq)]
pub enum ResetError {
    /// The process could not run, timed out or wrote too much.
    Exec(ExecError),
    /// The script raised an error. `trace` is the Python traceback.
    Script { error: String, trace: String },
    /// The process ended without a valid answer.
    Protocol(String),
    /// The answer holds shapes that break the rules.
    Shapes(String),
}

impl std::fmt::Display for ResetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResetError::Exec(e) => e.fmt(f),
            ResetError::Script { error, .. } => write!(f, "the script failed: {error}"),
            ResetError::Protocol(e) | ResetError::Shapes(e) => f.write_str(e),
        }
    }
}

/// The request of the reset protocol.
pub fn reset_request(
    name: &str,
    def: &PluginDef,
    params: &Map<String, Value>,
    geometry: &Geometry,
) -> Value {
    json!({
        "event": "reset",
        "name": name,
        "shape_type": def.shape_type.name(),
        "params": params,
        "geometry": geometry.to_json(),
        "units": "mm",
        "script": def.script,
    })
}

/// Parses the standard output of the runner.
pub fn parse_answer(
    stdout: &[u8],
    stderr: &str,
    code: Option<i32>,
    limits: &ResetLimits,
) -> Result<Vec<Generated>, ResetError> {
    let text = String::from_utf8_lossy(stdout);
    let line = text.lines().rev().find(|l| !l.trim().is_empty());
    let Some(answer) = line.and_then(|l| serde_json::from_str::<Value>(l).ok()) else {
        let tail: String = stderr
            .chars()
            .rev()
            .take(500)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        return Err(ResetError::Protocol(format!(
            "the script gave no answer (exit code {}). {}",
            code.map_or("none".to_string(), |c| c.to_string()),
            tail.trim()
        )));
    };
    if answer["ok"] != json!(true) {
        return Err(ResetError::Script {
            error: answer["error"]
                .as_str()
                .unwrap_or("unknown error")
                .to_string(),
            trace: answer["traceback"].as_str().unwrap_or("").to_string(),
        });
    }
    parse_shapes(&answer["shapes"], limits).map_err(ResetError::Shapes)
}

/// Runs the reset protocol once with the given interpreter.
pub fn reset(
    interpreter: &Interpreter,
    name: &str,
    def: &PluginDef,
    params: &Map<String, Value>,
    geometry: &Geometry,
    limits: &ResetLimits,
) -> Result<Vec<Generated>, ResetError> {
    let request = reset_request(name, def, params, geometry).to_string();
    let mut cmd = interpreter.command();
    cmd.args(["-I", "-c", RUNNER]);
    let out = exec::run(
        cmd,
        request.as_bytes(),
        Limits {
            timeout: limits.timeout,
            max_stdout: limits.max_output,
            max_stderr: 16 << 10,
        },
    )
    .map_err(ResetError::Exec)?;
    parse_answer(&out.stdout, &out.stderr, out.code, limits)
}

#[cfg(test)]
mod tests;
