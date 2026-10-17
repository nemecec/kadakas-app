//! The value types of the parameter structs.
//!
//! They keep the JSON of the caller as it is when a struct is written back (`5` stays `5`, a
//! number is not turned into `5.0`), so a method sees what the caller sent.

use std::borrow::Cow;
use std::marker::PhantomData;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Number, Value};

/// A JSON object whose fields the method reads itself.
pub type Obj = Map<String, Value>;

/// A number, as the caller wrote it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Num(pub Number);

impl JsonSchema for Num {
    fn schema_name() -> Cow<'static, str> {
        "Num".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "number"})
    }
    fn inline_schema() -> bool {
        true
    }
}

macro_rules! text_type {
    ($(#[$doc:meta])* $name:ident, $what:literal) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!({"type": "string", "description": $what})
            }
            fn inline_schema() -> bool {
                true
            }
        }
    };
}

/// A whole number. It reads `800` and `800.0` (a number with no fraction), refuses `2.5`, and is
/// written back as an integer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Int(pub i64);

impl<'de> Deserialize<'de> for Int {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let n = Number::deserialize(d)?;
        if let Some(i) = n.as_i64() {
            return Ok(Int(i));
        }
        if let Some(u) = n.as_u64() {
            return i64::try_from(u).map(Int).map_err(|_| {
                D::Error::invalid_value(serde::de::Unexpected::Unsigned(u), &"an integer")
            });
        }
        match n.as_f64() {
            // 2^53: every whole number below it is exact in a float.
            Some(f) if f.fract() == 0.0 && f.abs() <= 9_007_199_254_740_992.0 => Ok(Int(f as i64)),
            Some(f) => Err(D::Error::invalid_value(
                serde::de::Unexpected::Float(f),
                &"an integer",
            )),
            None => Err(D::Error::custom("not a number")),
        }
    }
}

impl JsonSchema for Int {
    fn schema_name() -> Cow<'static, str> {
        "Int".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({"type": "integer"})
    }
    fn inline_schema() -> bool {
        true
    }
}

text_type!(
    /// An ID: UUID text.
    Id,
    "An ID: UUID text"
);
text_type!(
    /// The ID of an object: UUID text.
    ObjectRef,
    "The ID of an object: UUID text"
);
text_type!(
    /// The ID or the name of a resource (a style, a symbol, a material). The method looks it up.
    ResourceRef,
    "The ID or the name of a resource"
);

/// `[x, y]` in mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Point(pub [Num; 2]);

impl JsonSchema for Point {
    fn schema_name() -> Cow<'static, str> {
        "Point".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "array",
            "items": {"type": "number"},
            "minItems": 2,
            "maxItems": 2,
            "description": "[x, y] in mm"
        })
    }
    fn inline_schema() -> bool {
        true
    }
}

/// `[x, y, z]` in mm (`[x, y]` is z = 0).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Point3(pub Vec<Num>);

impl<'de> Deserialize<'de> for Point3 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Vec::<Num>::deserialize(d)?;
        if !(2..=3).contains(&v.len()) {
            return Err(D::Error::invalid_length(
                v.len(),
                &"2 or 3 numbers [x, y, z]",
            ));
        }
        Ok(Point3(v))
    }
}

impl JsonSchema for Point3 {
    fn schema_name() -> Cow<'static, str> {
        "Point3".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "array",
            "items": {"type": "number"},
            "minItems": 2,
            "maxItems": 3,
            "description": "[x, y, z] in mm"
        })
    }
    fn inline_schema() -> bool {
        true
    }
}

/// `[r, g, b]` or `[r, g, b, a]`, each 0 to 255.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Color(pub Vec<u8>);

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Vec::<Int>::deserialize(d)?;
        if !(3..=4).contains(&v.len()) {
            return Err(D::Error::invalid_length(
                v.len(),
                &"3 or 4 numbers [r, g, b, a]",
            ));
        }
        v.into_iter()
            .map(|c| {
                u8::try_from(c.0)
                    .map_err(|_| D::Error::invalid_value(serde::de::Unexpected::Signed(c.0), &"u8"))
            })
            .collect::<Result<Vec<u8>, _>>()
            .map(Color)
    }
}

impl JsonSchema for Color {
    fn schema_name() -> Cow<'static, str> {
        "Color".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "array",
            "items": {"type": "integer", "minimum": 0, "maximum": 255},
            "minItems": 3,
            "maxItems": 4,
            "description": "[r, g, b, a]"
        })
    }
    fn inline_schema() -> bool {
        true
    }
}

/// A corner `[x, y]`, or `{p: [x, y], kind: ..., ...}`: the method reads it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Vertex(pub Value);

impl JsonSchema for Vertex {
    fn schema_name() -> Cow<'static, str> {
        "Vertex".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A corner [x, y], or {p: [x, y], kind: corner | arc | point_on_arc | bezier | cubic, ...}"
        })
    }
    fn inline_schema() -> bool {
        true
    }
}

/// A command of `execute`: an object with `op`. Execute reads it (and turns the names of layers
/// and classes into IDs, one command at a time).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommandJson(pub Obj);

impl JsonSchema for CommandJson {
    fn schema_name() -> Cow<'static, str> {
        "CommandJson".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "object",
            "required": ["op"],
            "description": "A command: {op, ...}; see describe.command_examples and describe {schema: true}"
        })
    }
    fn inline_schema() -> bool {
        true
    }
}

/// Either of two types: the first that fits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Either<A, B> {
    A(A),
    B(B),
}

/// The schema of `T` or null. Use it as `#[schemars(with = "Nullable<T>")]` on an
/// `Option<Option<T>>` field; [`nullable`] reads it.
pub struct Nullable<T>(PhantomData<T>);

impl<T: JsonSchema> JsonSchema for Nullable<T> {
    fn schema_name() -> Cow<'static, str> {
        format!("Nullable_{}", T::schema_name()).into()
    }
    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        json_schema!({"anyOf": [g.subschema_for::<T>(), {"type": "null"}], "x-nullable": true})
    }
    fn inline_schema() -> bool {
        true
    }
}

/// For an `Option<Option<T>>` field: no field is `None`, `null` is `Some(None)`.
pub fn nullable<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// For a required `Option<T>` field: the field must be there, and may be `null`.
pub fn required_nullable<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d)
}
