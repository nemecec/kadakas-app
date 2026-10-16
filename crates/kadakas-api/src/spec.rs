//! The parameters of every API method, in one table (`params.spec`).
//!
//! The table is the single definition of what a method takes: the names, the types, what is
//! required, the allowed values and the defaults. Four things read it, so they cannot drift:
//!
//! - [`prepare`] runs before every method. It refuses a parameter that the method does not take
//!   (the error names it and suggests the nearest known name), and it turns the name of a layer or
//!   a class into its ID, for every parameter of the type `layer` or `class`.
//! - [`json_schema`] makes the JSON Schema of a method. `describe {schemas: true}` and
//!   `describe {method}` return it, the MCP tools list it as their `inputSchema`, and the Python
//!   package builds its typed signatures from it.
//! - The tests check that every method of `METHODS` has an entry, that every example of
//!   `describe` fits its entry, and that the entries name only methods that exist.
//!
//! # The table
//!
//! Lines that start in column 0 name a method (`plants.place`), with the flag `open` after it when
//! the method also takes parameters that the table cannot name (fields of a resource that the
//! caller defines). Lines that start with two spaces belong to the method above:
//!
//! ```text
//! plants.place
//!   mode! enum(single,path,grid)   # required (the `!`), one of three values
//!   layer layer                    # a layer: an ID or the unique name of a layer
//!   spacing num|str = 1000         # a number or text, default 1000
//!   @include plant_style.create    # all parameters of another method (not required)
//!   @one_of id,ids                 # at least one of these is needed
//! ```
//!
//! A parameter line is `name[!] type [= default] [# description]`. The types are `str num int bool
//! any obj null` (`obj` is a free JSON object), `id` (an ID), `object` (the ID of an object), `layer`, `class`,
//! `sheet` (a layer or a class by ID or name), `ref` (the ID or the name of a resource), `point`
//! (`[x, y]`), `vertex` (a corner or a curve point of a polyline), `color` (`[r, g, b, a]`),
//! `command` (a command of `execute`), `enum(a,b,c)`, `T[]` (a list), `map(T)` (an object whose
//! values are `T`), and `A|B` (either type).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::{Map, Value, json};

use kadakas_core::model::{Document, LayerKind};
use kadakas_core::uid::Uid;

use crate::RpcError;

/// The table, as text.
const TABLE: &str = include_str!("params.spec");

/// The type of a parameter.
#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Null,
    Str,
    Num,
    Int,
    Bool,
    Any,
    Obj,
    Id,
    Object,
    Layer,
    Class,
    Sheet,
    Ref,
    Point,
    Vertex,
    Color,
    Command,
    Enum(Vec<String>),
    List(Box<Ty>),
    Map(Box<Ty>),
    Union(Vec<Ty>),
}

/// One parameter of a method.
#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Ty,
    pub required: bool,
    pub default: Option<Value>,
    pub doc: String,
}

/// The parameters of a method.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spec {
    pub name: String,
    pub params: Vec<Param>,
    /// The method also takes parameters that the table cannot name.
    pub open: bool,
    /// Groups of names of which at least one is needed.
    pub one_of: Vec<Vec<String>>,
}

impl Spec {
    pub fn param(&self, name: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.name == name)
    }
}

/// All the entries, by method name.
#[derive(Debug)]
pub struct Table {
    specs: BTreeMap<String, Spec>,
}

impl Table {
    pub fn get(&self, method: &str) -> Option<&Spec> {
        self.specs.get(method)
    }

    pub fn methods(&self) -> impl Iterator<Item = &str> {
        self.specs.keys().map(String::as_str)
    }
}

/// The table of the program. A mistake in the file is a bug of the program: it panics with the
/// line number (a test reads the table, so the build finds it).
pub fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| parse(TABLE).unwrap_or_else(|e| panic!("params.spec: {e}")))
}

fn parse_type(text: &str) -> Result<Ty, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("a parameter needs a type".into());
    }
    // A union: split at `|` outside of brackets.
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    if parts.len() > 1 {
        return parts
            .into_iter()
            .map(parse_type)
            .collect::<Result<Vec<_>, _>>()
            .map(Ty::Union);
    }
    if text.starts_with('(') && text.ends_with(')') {
        return parse_type(&text[1..text.len() - 1]);
    }
    if let Some(inner) = text.strip_suffix("[]") {
        return Ok(Ty::List(Box::new(parse_type(inner)?)));
    }
    if let Some(inner) = text.strip_prefix("enum(").and_then(|t| t.strip_suffix(')')) {
        let values: Vec<String> = inner.split(',').map(|v| v.trim().to_string()).collect();
        if values.iter().any(String::is_empty) {
            return Err(format!("an empty value in {text}"));
        }
        return Ok(Ty::Enum(values));
    }
    if let Some(inner) = text.strip_prefix("map(").and_then(|t| t.strip_suffix(')')) {
        return Ok(Ty::Map(Box::new(parse_type(inner)?)));
    }
    Ok(match text {
        "null" => Ty::Null,
        "str" => Ty::Str,
        "num" => Ty::Num,
        "int" => Ty::Int,
        "bool" => Ty::Bool,
        "any" => Ty::Any,
        "obj" => Ty::Obj,
        "id" => Ty::Id,
        "object" => Ty::Object,
        "layer" => Ty::Layer,
        "class" => Ty::Class,
        "sheet" => Ty::Sheet,
        "ref" => Ty::Ref,
        "point" => Ty::Point,
        "vertex" => Ty::Vertex,
        "color" => Ty::Color,
        "command" => Ty::Command,
        other => return Err(format!("unknown type '{other}'")),
    })
}

fn parse_default(text: &str) -> Value {
    let t = text.trim();
    serde_json::from_str(t).unwrap_or_else(|_| Value::String(t.to_string()))
}

/// Reads a table.
pub fn parse(text: &str) -> Result<Table, String> {
    let mut specs: BTreeMap<String, Spec> = BTreeMap::new();
    let mut includes: Vec<(String, String)> = Vec::new();
    let mut current: Option<Spec> = None;
    let finish = |cur: &mut Option<Spec>, specs: &mut BTreeMap<String, Spec>| {
        if let Some(s) = cur.take() {
            specs.insert(s.name.clone(), s);
        }
    };
    for (n, raw) in text.lines().enumerate() {
        let at = |m: String| format!("line {}: {m}", n + 1);
        let line = raw.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') {
            finish(&mut current, &mut specs);
            let mut words = line.split_whitespace();
            let name = words.next().expect("a word").to_string();
            let mut spec = Spec {
                name: name.clone(),
                ..Spec::default()
            };
            for flag in words {
                match flag {
                    "open" => spec.open = true,
                    other => return Err(at(format!("unknown flag '{other}' after {name}"))),
                }
            }
            if specs.contains_key(&name) {
                return Err(at(format!("{name} has two entries")));
            }
            current = Some(spec);
            continue;
        }
        let spec = current
            .as_mut()
            .ok_or_else(|| at("a parameter line before any method".into()))?;
        let body = line.trim_start();
        let (body, doc) = match body.split_once(" # ") {
            Some((b, d)) => (b.trim(), d.trim().to_string()),
            None => (body.trim_end_matches(" #").trim(), String::new()),
        };
        if let Some(rest) = body.strip_prefix("@include ") {
            includes.push((spec.name.clone(), rest.trim().to_string()));
            continue;
        }
        if let Some(rest) = body.strip_prefix("@one_of ") {
            spec.one_of
                .push(rest.split(',').map(|s| s.trim().to_string()).collect());
            continue;
        }
        let (head, default) = match body.split_once(" = ") {
            Some((h, d)) => (h.trim(), Some(parse_default(d))),
            None => (body, None),
        };
        let (name, ty) = head
            .split_once(char::is_whitespace)
            .ok_or_else(|| at(format!("'{head}' needs a type")))?;
        let (name, required) = match name.strip_suffix('!') {
            Some(n) => (n, true),
            None => (name, false),
        };
        if spec.param(name).is_some() {
            return Err(at(format!("{} has the parameter {name} twice", spec.name)));
        }
        spec.params.push(Param {
            name: name.to_string(),
            ty: parse_type(ty).map_err(&at)?,
            required,
            default,
            doc,
        });
    }
    finish(&mut current, &mut specs);
    // An include copies the parameters that the method does not have yet, as optional ones.
    for (target, source) in includes {
        let from = specs
            .get(&source)
            .ok_or_else(|| format!("{target} includes {source}, which has no entry"))?
            .clone();
        let to = specs.get_mut(&target).expect("the including method");
        for mut p in from.params {
            if to.param(&p.name).is_none() {
                p.required = false;
                to.params.push(p);
            }
        }
        to.open |= from.open;
    }
    for s in specs.values() {
        for group in &s.one_of {
            for name in group {
                if s.param(name).is_none() {
                    return Err(format!(
                        "{}: one_of names {name}, which is no parameter",
                        s.name
                    ));
                }
            }
        }
    }
    Ok(Table { specs })
}

// ---------------------------------------------------------------------------------------------
// The check before a method runs.

/// The distance between two names, counting inserted, removed and changed letters and swapped
/// neighbours.
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// The known name that is nearest to `name`, if one is near enough to be a typing mistake.
pub fn nearest<'a>(name: &str, known: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let lower = name.to_ascii_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for k in known {
        let kl = k.to_ascii_lowercase();
        let mut d = distance(&lower, &kl);
        // `layers` for `layer`, `ids` for `id`, `site_model` for `site`: a name that contains the
        // other counts as a near miss.
        if kl.len() >= 3 && lower.len() >= 3 && (lower.contains(&kl) || kl.contains(&lower)) {
            d = d.min(1);
        }
        let limit = (kl.chars().count().max(lower.chars().count()) / 3).clamp(1, 3);
        if d <= limit && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, k));
        }
    }
    best.map(|(_, k)| k)
}

fn candidates<'a>(items: impl Iterator<Item = &'a str>) -> String {
    let mut v: Vec<String> = items.map(|n| format!("\"{n}\"")).take(13).collect();
    if v.len() == 13 {
        v.truncate(12);
        v.push("...".into());
    }
    v.join(", ")
}

/// A layer or class from an ID or a name. `kind` is `"layer"`, `"class"` or `"sheet"`.
fn resolve_ref(doc: &Document, kind: &str, text: &str) -> Result<Option<Uid>, RpcError> {
    let t = text.trim();
    // An ID stays as it is (the method says if it does not exist).
    if Uid::parse(t).is_some() {
        return Ok(None);
    }
    let items: Vec<(Uid, &str)> = match kind {
        "class" => doc
            .classes
            .iter()
            .map(|c| (c.id.0, c.name.as_str()))
            .collect(),
        "sheet" => doc
            .layers
            .iter()
            .filter(|l| l.kind == LayerKind::Sheet)
            .map(|l| (l.id.0, l.name.as_str()))
            .collect(),
        _ => doc
            .layers
            .iter()
            .map(|l| (l.id.0, l.name.as_str()))
            .collect(),
    };
    let what = if kind == "class" { "class" } else { "layer" };
    let label = if kind == "sheet" { "sheet layer" } else { what };
    // The exact name wins; else the name without regard to the case. Several matches are an error.
    for same in [
        (|a: &str, b: &str| a == b) as fn(&str, &str) -> bool,
        |a: &str, b: &str| a.eq_ignore_ascii_case(b),
    ] {
        let hits: Vec<&(Uid, &str)> = items.iter().filter(|(_, n)| same(n, t)).collect();
        match hits.as_slice() {
            [] => {}
            [(id, _)] => return Ok(Some(*id)),
            many => {
                let listed = many
                    .iter()
                    .map(|(id, n)| format!("\"{n}\" ({id})"))
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(RpcError::invalid_params(format!(
                    "the name \"{t}\" matches {} {label}s: {listed}. Give the ID of the one you mean",
                    many.len()
                ))
                .with_details(json!({
                    "name": t,
                    "candidates": many.iter().map(|(id, n)| json!({"id": id, "name": n})).collect::<Vec<_>>(),
                })));
            }
        }
    }
    let near = nearest(t, items.iter().map(|(_, n)| *n));
    let hint = near
        .map(|n| format!(" Did you mean \"{n}\"?"))
        .unwrap_or_default();
    Err(RpcError::invalid_params(format!(
        "no {label} is named \"{t}\".{hint} The {label}s are: {}",
        if items.is_empty() {
            "none".to_string()
        } else {
            candidates(items.iter().map(|(_, n)| *n))
        }
    ))
    .with_details(json!({
        "name": t,
        "candidates": items.iter().map(|(id, n)| json!({"id": id, "name": n})).collect::<Vec<_>>(),
    })))
}

/// Replaces names of layers and classes by IDs in a value of type `ty`.
fn resolve_in(doc: &Document, ty: &Ty, v: &mut Value) -> Result<(), RpcError> {
    match (ty, &mut *v) {
        (Ty::Layer | Ty::Class | Ty::Sheet, Value::String(s)) => {
            let kind = match ty {
                Ty::Class => "class",
                Ty::Sheet => "sheet",
                _ => "layer",
            };
            if let Some(id) = resolve_ref(doc, kind, s)? {
                *v = json!(id);
            }
        }
        (Ty::List(inner), Value::Array(items)) => {
            for item in items {
                resolve_in(doc, inner, item)?;
            }
        }
        (Ty::Union(options), Value::String(_)) => {
            if let Some(t) = options
                .iter()
                .find(|t| matches!(t, Ty::Layer | Ty::Class | Ty::Sheet))
            {
                resolve_in(doc, t, v)?;
            }
        }
        (Ty::Union(options), Value::Array(_)) => {
            if let Some(t) = options.iter().find(|t| matches!(t, Ty::List(_))) {
                resolve_in(doc, t, v)?;
            }
        }
        (Ty::Command, Value::Object(c)) => resolve_command(doc, c)?,
        _ => {}
    }
    Ok(())
}

/// The `layer` and `class` of a command of `execute`, and of the commands inside a batch.
fn resolve_command(doc: &Document, c: &mut Map<String, Value>) -> Result<(), RpcError> {
    for (key, ty) in [("layer", Ty::Layer), ("class", Ty::Class)] {
        if let Some(v) = c.get_mut(key) {
            resolve_in(doc, &ty, v)?;
        }
    }
    if let Some(Value::Array(inner)) = c.get_mut("commands") {
        for cmd in inner {
            if let Value::Object(m) = cmd {
                resolve_command(doc, m)?;
            }
        }
    }
    Ok(())
}

/// What a method gets: the parameters checked against the table, with the names of layers and
/// classes replaced by IDs. A method without an entry gets its parameters as they are.
pub fn prepare(doc: &Document, method: &str, p: Value) -> Result<Value, RpcError> {
    let Some(spec) = table().get(method) else {
        return Ok(p);
    };
    let Value::Object(mut map) = p else {
        return Ok(p);
    };
    if !spec.open {
        let unknown: Vec<&String> = map.keys().filter(|k| spec.param(k).is_none()).collect();
        if !unknown.is_empty() {
            return Err(unknown_error(spec, &unknown));
        }
    }
    for param in &spec.params {
        if let Some(v) = map.get_mut(&param.name) {
            resolve_in(doc, &param.ty, v)?;
        }
    }
    Ok(Value::Object(map))
}

fn unknown_error(spec: &Spec, unknown: &[&String]) -> RpcError {
    let known = || spec.params.iter().map(|p| p.name.as_str());
    let mut parts = Vec::new();
    let mut suggestions = Map::new();
    for name in unknown {
        match nearest(name, known()) {
            Some(near) => {
                parts.push(format!("'{name}' (did you mean '{near}'?)"));
                suggestions.insert((*name).clone(), json!(near));
            }
            None => parts.push(format!("'{name}'")),
        }
    }
    let list = known().collect::<Vec<_>>().join(", ");
    let noun = if unknown.len() == 1 {
        "parameter"
    } else {
        "parameters"
    };
    let takes = if spec.params.is_empty() {
        "it takes no parameters".to_string()
    } else {
        format!("it takes: {list}")
    };
    RpcError::invalid_params(format!(
        "unknown {noun} {} for {}; {takes}. describe {{method: \"{}\"}} has the schema",
        parts.join(", "),
        spec.name,
        spec.name
    ))
    .with_details(json!({
        "unknown": unknown,
        "suggestions": suggestions,
        "known": known().collect::<Vec<_>>(),
    }))
}

// ---------------------------------------------------------------------------------------------
// JSON Schema.

fn id_schema(what: &str) -> Value {
    json!({"type": "string", "description": what})
}

/// The JSON Schema of a type.
pub fn type_schema(ty: &Ty) -> Value {
    match ty {
        Ty::Null => json!({"type": "null"}),
        Ty::Str => json!({"type": "string"}),
        Ty::Num => json!({"type": "number"}),
        Ty::Int => json!({"type": "integer"}),
        Ty::Bool => json!({"type": "boolean"}),
        Ty::Any => json!({}),
        Ty::Obj => json!({"type": "object"}),
        Ty::Id => id_schema("An ID: UUID text"),
        Ty::Object => id_schema("The ID of an object: UUID text"),
        Ty::Layer => {
            json!({"type": "string", "x-kadakas-ref": "layer", "description": "A design layer: its ID (UUID text) or its unique name"})
        }
        Ty::Class => {
            json!({"type": "string", "x-kadakas-ref": "class", "description": "A class: its ID (UUID text) or its unique name"})
        }
        Ty::Sheet => {
            json!({"type": "string", "x-kadakas-ref": "sheet", "description": "A sheet layer: its ID (UUID text) or its unique name"})
        }
        Ty::Ref => id_schema("The ID or the name of a resource"),
        Ty::Point => {
            json!({"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2, "description": "[x, y] in mm"})
        }
        Ty::Vertex => {
            json!({"description": "A corner [x, y], or {p: [x, y], kind: corner | arc | point_on_arc | bezier | cubic, ...}"})
        }
        Ty::Color => {
            json!({"type": "array", "items": {"type": "integer", "minimum": 0, "maximum": 255}, "minItems": 3, "maxItems": 4, "description": "[r, g, b, a]"})
        }
        Ty::Command => {
            json!({"type": "object", "required": ["op"], "description": "A command: {op, ...}; see describe.command_examples and describe {schema: true}"})
        }
        Ty::Enum(values) => json!({"enum": values}),
        Ty::List(inner) => json!({"type": "array", "items": type_schema(inner)}),
        Ty::Map(inner) => json!({"type": "object", "additionalProperties": type_schema(inner)}),
        Ty::Union(options) => json!({"anyOf": options.iter().map(type_schema).collect::<Vec<_>>()}),
    }
}

/// The JSON Schema (draft 2020-12) of the parameters of a method.
pub fn json_schema(spec: &Spec) -> Value {
    let mut props = Map::new();
    let mut required = Vec::new();
    for p in &spec.params {
        let mut s = type_schema(&p.ty);
        if let Some(o) = s.as_object_mut() {
            if !p.doc.is_empty() {
                // The description of the table is more exact than the one of the type.
                let old = o
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                o.insert(
                    "description".into(),
                    json!(match old {
                        Some(old) if matches!(p.ty, Ty::Layer | Ty::Class | Ty::Sheet) => {
                            format!("{} ({})", p.doc, old)
                        }
                        _ => p.doc.clone(),
                    }),
                );
            }
            if let Some(d) = &p.default {
                o.insert("default".into(), d.clone());
            }
        }
        props.insert(p.name.clone(), s);
        if p.required {
            required.push(json!(p.name));
        }
    }
    let mut schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": props,
        "additionalProperties": spec.open,
    });
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    if !spec.one_of.is_empty() {
        let groups: Vec<Value> = spec
            .one_of
            .iter()
            .map(|g| json!({"anyOf": g.iter().map(|n| json!({"required": [n]})).collect::<Vec<_>>()}))
            .collect();
        schema["allOf"] = json!(groups);
    }
    schema
}

/// `describe {method}`: the description and the JSON Schema of one method.
pub fn describe_method(name: &str) -> Result<Value, RpcError> {
    let Some((_, doc)) = crate::METHODS.iter().find(|(n, _)| *n == name) else {
        let hint = nearest(name, crate::METHODS.iter().map(|(n, _)| *n))
            .map(|n| format!("; did you mean '{n}'?"))
            .unwrap_or_default();
        return Err(RpcError::not_found(format!(
            "unknown method '{name}'{hint}"
        )));
    };
    let spec = table()
        .get(name)
        .ok_or_else(|| RpcError::internal(format!("{name} has no entry in params.spec")))?;
    Ok(json!({"method": name, "doc": doc, "params": json_schema(spec)}))
}

/// The schemas of all methods, by name.
pub fn all_schemas() -> Map<String, Value> {
    table()
        .specs
        .values()
        .map(|s| (s.name.clone(), json_schema(s)))
        .collect()
}

/// The names in `params` that the method does not take. Empty for a method that is open or
/// has no entry.
#[cfg(test)]
pub(crate) fn unknown_names(method: &str, params: &Value) -> Vec<String> {
    match (table().get(method), params) {
        (Some(spec), Value::Object(m)) if !spec.open => m
            .keys()
            .filter(|k| spec.param(k).is_none())
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
