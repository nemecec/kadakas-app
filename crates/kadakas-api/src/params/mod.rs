//! The parameters of every API method: one struct per method.
//!
//! A method takes what its struct says. The struct derives `Deserialize` (serde parses and checks
//! the types, the required fields and the allowed values), `Serialize` (the checked parameters go
//! back to JSON for the method) and `JsonSchema` (schemars makes the JSON Schema). Four things
//! read the structs, so they cannot drift:
//!
//! - [`prepare`] runs before every method. It refuses a parameter that the method does not take
//!   (the error names it and suggests the nearest known name), parses the rest into the struct,
//!   turns the names of layers and classes into IDs (the reference types of [`refs`]) and gives
//!   the method the checked parameters. A wrong type, a missing required parameter or a value
//!   that is not allowed is an error (-32602) that names the parameter and what it expects.
//! - [`schema`] gives the JSON Schema of a method. `describe {method}` and
//!   `describe {schemas: true}` return it, the MCP tools list it as their `inputSchema`, and the
//!   Python package builds its typed signatures from it.
//! - The tests check that every method of `METHODS` has a struct, and that every example of
//!   `describe` is accepted by the struct of its method.
//!
//! # Writing a struct
//!
//! ```ignore
//! /// Place plants.
//! #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
//! #[serde(deny_unknown_fields)]
//! pub struct PlantsPlaceParams {
//!     /// single, path or grid
//!     pub mode: PlantsPlaceMode,                    // required
//!     /// The layer of the plants
//!     #[serde(default, skip_serializing_if = "Option::is_none")]
//!     pub layer: Option<LayerRef>,                  // optional
//! }
//! ```
//!
//! Every optional field has `#[serde(default, skip_serializing_if = "Option::is_none")]`, so the
//! method sees only what the caller sent. A field that may be `null` is an
//! `Option<Option<T>>` with `deserialize_with = "nullable"` and `schemars(with = "Nullable<T>")`.
//! Doc comments become the descriptions of the schema. Add the struct to the `entries!` list of
//! its domain file; a test fails for a method without one.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use kadakas_core::model::Document;

use crate::RpcError;

mod macros;
pub mod refs;
pub mod types;

pub(crate) mod failure;

mod collab;
mod documents;
mod drafting;
mod feedback;
mod io;
mod irrigation;
mod layers;
pub(crate) mod mesh;
mod objects;
mod plants;
mod plugins;
mod reports;
mod resource;
mod review;
mod sheets;
mod site;
mod view;

pub(crate) use macros::param_enum;
pub use refs::{ClassRef, LayerRef, Names, SheetRef, nearest};
pub(crate) use review::AgentActivityParams;
pub use types::{
    Color, CommandJson, Either, Id, Int, Nullable, Num, Obj, ObjectRef, Point, Point3, ResourceRef,
    Vertex, nullable, required_nullable,
};

/// What a parameter struct must be.
pub trait Params: DeserializeOwned + Serialize + JsonSchema {}
impl<T: DeserializeOwned + Serialize + JsonSchema> Params for T {}

/// The struct of one method.
pub struct Entry {
    pub method: &'static str,
    pub(crate) schema: fn() -> Value,
    /// Parses and checks parameters; gives them back as JSON.
    pub(crate) parse: fn(&Value) -> Result<Value, ParseError>,
    /// The names of the fields, in the order of the struct, by a probe of serde.
    pub(crate) probe: fn() -> Fields,
}

/// The entries of a domain, from the list `method => Struct`.
macro_rules! entries {
    ($($method:literal => $ty:ty),* $(,)?) => {
        pub const ENTRIES: &[$crate::params::Entry] = &[
            $($crate::params::Entry {
                method: $method,
                schema: $crate::params::schema_of::<$ty>,
                parse: $crate::params::parse_as::<$ty>,
                probe: $crate::params::probe_fields::<$ty>,
            }),*
        ];
    };
}
pub(crate) use entries;

/// Why parameters were not accepted: the way to the place and what serde said.
pub(crate) type ParseError = serde_path_to_error::Error<serde_json::Error>;

pub(crate) fn parse_as<T: Params>(v: &Value) -> Result<Value, ParseError> {
    let parsed: T = serde_path_to_error::deserialize(v)?;
    let mut out = serde_json::to_value(&parsed).expect("a parameter struct is written as JSON");
    // An optional parameter that the caller sent as null stays null: some methods read null as
    // "remove it", and the others treat it as missing.
    if let (Value::Object(sent), Value::Object(kept)) = (v, &mut out) {
        for (key, value) in sent {
            if value.is_null() && !kept.contains_key(key) {
                kept.insert(key.clone(), Value::Null);
            }
        }
    }
    Ok(out)
}

/// The JSON Schema of a struct: draft 2020-12, one schema without `$ref`, no titles.
pub(crate) fn schema_of<T: JsonSchema>() -> Value {
    let generator = SchemaSettings::draft2020_12()
        .with(|s| s.inline_subschemas = true)
        .into_generator();
    let mut v = generator.into_root_schema_for::<T>().to_value();
    tidy(&mut v);
    // A union of parameter sets (a tagged enum) is still an object: MCP wants it at the root.
    if let Value::Object(root) = &mut v
        && !root.contains_key("type")
    {
        root.insert("type".into(), json!("object"));
    }
    v
}

/// Removes the titles that schemars makes from the names of the types, and the marker of
/// [`Nullable`]. An optional property keeps the `null` that schemars adds: a method takes `null`
/// for a parameter that is not given (a client in Python sends `None`), and some read it as "remove
/// it", so the schema says that `null` is allowed.
fn tidy(v: &mut Value) {
    let Value::Object(m) = v else { return };
    m.remove("title");
    m.remove("x-nullable");
    if let Some(Value::Object(props)) = m.get_mut("properties") {
        props.values_mut().for_each(tidy);
    }
    for key in ["items", "additionalProperties"] {
        if let Some(child) = m.get_mut(key) {
            tidy(child);
        }
    }
    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(Value::Array(list)) = m.get_mut(key) {
            list.iter_mut().for_each(tidy);
        }
    }
}

/// The names that a struct knows.
#[derive(Clone, Debug, Default)]
pub(crate) struct Fields {
    names: Vec<String>,
    /// The struct refuses other names itself (`deny_unknown_fields`), so the list is complete.
    strict: bool,
}

/// Asks serde for the field names: a struct with `deny_unknown_fields` answers an unknown field
/// with "unknown field `x`, expected one of `a`, `b`" (in the order of the struct).
pub(crate) fn probe_fields<T: Params>() -> Fields {
    let text = serde_json::from_value::<T>(json!({ "\u{1}": null }))
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    if text.contains("there are no fields") {
        return Fields {
            names: Vec::new(),
            strict: true,
        };
    }
    // Only a struct that refuses other names tells its fields (a union of parameter sets does not).
    let strict = text.starts_with("unknown field");
    Fields {
        names: if strict {
            text.split("expected")
                .nth(1)
                .map(|r| {
                    r.split('`')
                        .skip(1)
                        .step_by(2)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        },
        strict,
    }
}

/// What is worked out once per method.
struct Cached {
    schema: Value,
    fields: Fields,
    /// Groups of names of which at least one is needed.
    one_of: Vec<Vec<String>>,
    /// The struct has a layer, class or sheet reference somewhere.
    has_refs: bool,
}

fn cached(entry: &'static Entry) -> Arc<Cached> {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, Arc<Cached>>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache
        .entry(entry.method)
        .or_insert_with(|| {
            let schema = (entry.schema)();
            let mut fields = (entry.probe)();
            if !fields.strict {
                // A struct that serde cannot probe (a tagged union): the names of the schema.
                let mut names = Vec::new();
                collect_names(&schema, &mut names);
                fields.names = names;
            }
            let one_of = schema
                .get("allOf")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|g| g.get("anyOf").and_then(Value::as_array))
                .map(|any| {
                    any.iter()
                        .filter_map(|r| r["required"][0].as_str().map(str::to_string))
                        .collect()
                })
                .collect();
            let has_refs = schema.to_string().contains("\"x-kadakas-ref\"");
            Arc::new(Cached {
                schema,
                fields,
                one_of,
                has_refs,
            })
        })
        .clone()
}

/// The property names of a schema, also in the branches of `oneOf` and `anyOf`.
fn collect_names(schema: &Value, out: &mut Vec<String>) {
    if let Some(Value::Object(props)) = schema.get("properties") {
        for k in props.keys() {
            if !out.contains(k) {
                out.push(k.clone());
            }
        }
    }
    for key in ["oneOf", "anyOf", "allOf"] {
        if let Some(Value::Array(list)) = schema.get(key) {
            list.iter().for_each(|s| collect_names(s, out));
        }
    }
}

/// The entries of all domains, by method name.
fn registry() -> &'static HashMap<&'static str, &'static Entry> {
    static R: OnceLock<HashMap<&'static str, &'static Entry>> = OnceLock::new();
    R.get_or_init(|| {
        let mut map = HashMap::new();
        for e in all_entries() {
            let dup = map.insert(e.method, e);
            assert!(dup.is_none(), "{} has two parameter structs", e.method);
        }
        map
    })
}

fn all_entries() -> impl Iterator<Item = &'static Entry> {
    ENTRY_LISTS.iter().flat_map(|l| l.iter())
}

/// The lists of the domain files.
const ENTRY_LISTS: &[&[Entry]] = &[
    collab::ENTRIES,
    documents::ENTRIES,
    drafting::ENTRIES,
    feedback::ENTRIES,
    io::ENTRIES,
    irrigation::ENTRIES,
    layers::ENTRIES,
    mesh::ENTRIES,
    objects::ENTRIES,
    plants::ENTRIES,
    plugins::ENTRIES,
    reports::ENTRIES,
    resource::ENTRIES,
    review::ENTRIES,
    sheets::ENTRIES,
    site::ENTRIES,
    view::ENTRIES,
];

/// The methods that have a struct.
pub fn methods() -> impl Iterator<Item = &'static str> {
    all_entries().map(|e| e.method)
}

/// True when the method has a struct.
pub fn has(method: &str) -> bool {
    registry().contains_key(method)
}

/// The JSON Schema (draft 2020-12) of the parameters of a method.
pub fn schema(method: &str) -> Option<Value> {
    registry().get(method).map(|e| cached(e).schema.clone())
}

/// Checks the parameters of a method and gives them back as the method wants them: unknown
/// names refused, types and required parameters checked, names of layers and classes replaced by
/// IDs. A method without a struct gets its parameters as they are.
pub fn prepare(doc: &Document, method: &str, p: Value) -> Result<Value, RpcError> {
    prepare_with(Some(doc), method, p)
}

fn prepare_with(doc: Option<&Document>, method: &str, p: Value) -> Result<Value, RpcError> {
    let Some(&entry) = registry().get(method) else {
        // A method of the list without a struct would skip the check: that is a bug (a test
        // guards it at build time). A name that is no method goes on; the dispatcher says so.
        return if crate::METHODS.iter().any(|(name, _)| *name == method) {
            Err(RpcError::internal(format!(
                "{method} has no parameter struct: this is a bug of the program"
            )))
        } else {
            Ok(p)
        };
    };
    let Value::Object(mut map) = p else {
        return Ok(p);
    };
    let c = cached(entry);
    // A method that takes a list `ids` and has no `id` of its own accepts one `id` as a list of one.
    let has = |n: &str| c.fields.names.iter().any(|f| f == n);
    if has("ids")
        && !has("id")
        && !map.contains_key("ids")
        && let Some(one) = map.remove("id")
    {
        map.insert("ids".into(), json!([one]));
    }
    if c.fields.strict || !c.fields.names.is_empty() {
        let unknown: Vec<&String> = map.keys().filter(|k| !has(k)).collect();
        if !unknown.is_empty() {
            return Err(failure::unknown_error(method, &c.fields.names, &unknown));
        }
    }
    for group in &c.one_of {
        if !group.iter().any(|n| map.contains_key(n)) {
            return Err(failure::one_of_error(method, group));
        }
    }
    let input = Value::Object(map);
    refs::take_failure();
    let parsed = match doc {
        Some(doc) => {
            let names = if c.has_refs {
                Names::of(doc)
            } else {
                Names::default()
            };
            refs::with_names(names, || (entry.parse)(&input))
        }
        // The caller set the names (a test).
        None => (entry.parse)(&input),
    };
    let stale = refs::take_failure();
    match parsed {
        Ok(v) => Ok(v),
        Err(e) => Err(stale.unwrap_or_else(|| failure::type_error(method, &e, &input, &c.schema))),
    }
}

/// Checks parameters of a method against its struct without a document: names of layers and
/// classes stay as they are. For tests.
#[cfg(test)]
pub(crate) fn check_without_document(method: &str, p: Value) -> Result<Value, RpcError> {
    let doc = Document::default();
    refs::with_names(Names::lenient(), || {
        // `prepare` builds the names of the document, so build the call by hand.
        let _ = &doc;
        prepare_with(None, method, p)
    })
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
    let params = schema(name)
        .ok_or_else(|| RpcError::internal(format!("{name} has no parameter struct")))?;
    Ok(json!({"method": name, "doc": doc, "params": params}))
}

/// The schemas of all methods, by name.
pub fn all_schemas() -> Map<String, Value> {
    all_entries()
        .map(|e| (e.method.to_string(), cached(e).schema.clone()))
        .collect()
}

#[cfg(test)]
mod tests;
