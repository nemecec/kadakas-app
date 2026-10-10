//! JSON Schema of commands (`schema_for_commands`), made with `schemars`.
//!
//! `Command` derives `JsonSchema`, so the schema always follows its variants and fields. Some
//! types inside commands are big (shapes, resources, patches) and other agents work on them. For
//! these types the schema is the shape of the object and the list of its kinds, not the full
//! description of each kind: use `describe` and the examples for the fields of each kind. The
//! list of kinds is read from the serde implementation of the type, so it cannot drift.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::command::Command;
use crate::curve::{Vertex, VertexKind};
use crate::model::{Attributes, ClassId, Color, LayerId, LayerKind, ObjectId, Shape};
use crate::resource::{ResourceBody, ResourceId};

/// The names of the variants of an enum, read from the error that serde gives for an unknown
/// variant: "unknown variant `x`, expected one of `a`, `b`" (or "expected `a`" for one variant).
/// `with` gives the other keys of the object, such as the tag.
pub fn variant_names<T: DeserializeOwned>(tag: Option<&str>) -> Vec<String> {
    let probe = match tag {
        Some(t) => json!({ t: "\u{1}unknown" }),
        None => json!("\u{1}unknown"),
    };
    let Err(e) = serde_json::from_value::<T>(probe) else {
        return Vec::new();
    };
    let text = e.to_string();
    let Some(rest) = text.split("expected").nth(1) else {
        return Vec::new();
    };
    rest.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

fn string_enum<T: DeserializeOwned>() -> Schema {
    let names = variant_names::<T>(None);
    json_schema!({ "type": "string", "enum": names })
}

fn tagged_object<T: DeserializeOwned>(tag: &str, doc: &str) -> Schema {
    let names = variant_names::<T>(Some(tag));
    json_schema!({
        "type": "object",
        "description": doc,
        "required": [tag],
        "properties": { tag: { "type": "string", "enum": names } },
        "additionalProperties": true
    })
}

fn opaque(doc: &str) -> Schema {
    json_schema!({ "type": "object", "description": doc })
}

macro_rules! schema_impl {
    ($ty:ty, $name:literal, |$g:ident| $body:expr) => {
        impl JsonSchema for $ty {
            fn schema_name() -> Cow<'static, str> {
                Cow::Borrowed($name)
            }
            fn json_schema($g: &mut SchemaGenerator) -> Schema {
                $body
            }
        }
    };
}

/// The text form of an ID: a UUID, 8-4-4-4-12 hex digits.
const ID_PATTERN: &str =
    "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$";

macro_rules! id_schema {
    ($ty:ty, $name:literal) => {
        schema_impl!($ty, $name, |_g| json_schema!({
            "type": "string", "pattern": ID_PATTERN, "minLength": 36, "maxLength": 36,
            "description": "An ID: a UUID text such as 0190f1c2-7a3b-7c1d-8e2f-123456789abc."
        }));
    };
}

id_schema!(ObjectId, "ObjectId");
id_schema!(LayerId, "LayerId");
id_schema!(ClassId, "ClassId");
id_schema!(ResourceId, "ResourceId");

schema_impl!(Color, "Color", |_g| json_schema!({
    "type": "array", "items": {"type": "integer", "minimum": 0, "maximum": 255},
    "minItems": 4, "maxItems": 4, "description": "[red, green, blue, alpha], 0 to 255."
}));

schema_impl!(Attributes, "Attributes", |g| {
    let color = g.subschema_for::<Color>();
    let id = g.subschema_for::<ResourceId>();
    json_schema!({
        "type": "object",
        "description": "Graphic attributes. A missing field uses the class attribute.",
        "properties": {
            "pen": color,
            "fill": color,
            "line_weight": {"type": "number", "description": "mm on paper"},
            "fill_pattern": id,
            "line_type": id
        },
        "additionalProperties": false
    })
});

schema_impl!(LayerKind, "LayerKind", |_g| string_enum::<LayerKind>());

fn pt() -> Value {
    json!({"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2})
}

fn pt3() -> Value {
    json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3})
}

fn vertex_value() -> Value {
    json!({
        "description": "A polyline point: [x, y] is a corner; {p: [x, y], kind: corner | point_on_arc | bezier | cubic} is a curved vertex; {p, kind: arc, radius} is a fillet.",
        "anyOf": [
            pt(),
            {"type": "object", "required": ["p"], "properties": {"p": pt()}, "additionalProperties": false},
            {"type": "object", "required": ["p", "kind"],
             "properties": {"p": pt(), "kind": {"enum": ["corner", "point_on_arc", "bezier", "cubic"]}}},
            {"type": "object", "required": ["p", "kind", "radius"],
             "properties": {"p": pt(), "kind": {"const": "arc"}, "radius": {"type": "number"}}}
        ]
    })
}

fn vertices(min: u64) -> Value {
    json!({"type": "array", "minItems": min, "items": vertex_value()})
}

/// The kinds of shape that the schema describes field by field. The schema accepts only what
/// serde accepts for these (a test checks it).
pub const EXACT_SHAPE_KINDS: [&str; 12] = [
    "line",
    "rect",
    "circle",
    "arc",
    "polyline",
    "text",
    "plant",
    "locus3d",
    "polyline3d",
    "group",
    "landscape_area",
    "hardscape",
];

/// The kinds of shape that the schema only names (`kind` and an object). Their fields are in
/// the examples of `describe`. The schema accepts objects that serde may still refuse.
pub fn structural_shape_kinds() -> Vec<String> {
    variant_names::<Shape>(Some("kind"))
        .into_iter()
        .filter(|k| !EXACT_SHAPE_KINDS.contains(&k.as_str()))
        .collect()
}

fn shape_value() -> Value {
    let kind = |k: &str, required: &[&str], props: Value| {
        let mut p = props.as_object().cloned().unwrap_or_default();
        p.insert("kind".into(), json!({"const": k}));
        let mut req = vec!["kind"];
        req.extend_from_slice(required);
        json!({"type": "object", "required": req, "properties": p})
    };
    let num = json!({"type": "number"});
    let mut any_of = vec![
        kind("line", &["a", "b"], json!({"a": pt(), "b": pt()})),
        kind("rect", &["min", "max"], json!({"min": pt(), "max": pt()})),
        kind(
            "circle",
            &["center", "radius"],
            json!({"center": pt(), "radius": num}),
        ),
        kind(
            "arc",
            &["center", "radius", "start", "end"],
            json!({"center": pt(), "radius": num, "start": num, "end": num}),
        ),
        kind(
            "polyline",
            &["points", "closed"],
            json!({"points": vertices(2), "closed": {"type": "boolean"}}),
        ),
        kind(
            "text",
            &["at", "text", "height"],
            json!({"at": pt(), "text": {"type": "string"}, "height": num}),
        ),
        kind(
            "plant",
            &["at", "species", "spread", "height"],
            json!({"at": pt(), "species": {"type": "string"}, "spread": num, "height": num}),
        ),
        kind("locus3d", &["p"], json!({"p": pt3()})),
        kind(
            "polyline3d",
            &["points", "closed"],
            json!({"points": {"type": "array", "minItems": 2, "items": pt3()}, "closed": {"type": "boolean"}}),
        ),
        kind("group", &[], json!({"name": {"type": "string"}})),
        kind(
            "landscape_area",
            &["boundary", "name"],
            json!({"boundary": vertices(3), "name": {"type": "string"}}),
        ),
        kind(
            "hardscape",
            &["boundary", "material"],
            json!({"boundary": vertices(3), "material": {"type": "string"}, "thickness": num, "elevation": num}),
        ),
    ];
    any_of.push(json!({
        "type": "object", "required": ["kind"],
        "properties": {"kind": {"enum": structural_shape_kinds()}},
        "description": "Structural only: the fields of these kinds are not checked."
    }));
    json!({
        "description": "A shape. `kind` names it. The kinds line, rect, circle, arc, polyline, text, plant, locus3d, polyline3d, group, landscape_area and hardscape are described field by field; the other kinds are checked for `kind` only (see describe: command_examples and the section of each feature).",
        "anyOf": any_of
    })
}

schema_impl!(Shape, "Shape", |_g| Schema::try_from(shape_value())
    .expect("the shape schema is an object"));

schema_impl!(Vertex, "Vertex", |_g| Schema::try_from(vertex_value())
    .expect("the vertex schema is an object"));

schema_impl!(VertexKind, "VertexKind", |_g| json_schema!({
    "description": "The kind of a vertex. `arc` has a radius.",
    "anyOf": [
        {"type": "object", "required": ["kind"],
         "properties": {"kind": {"enum": ["corner", "point_on_arc", "bezier", "cubic"]}}},
        {"type": "object", "required": ["kind", "radius"],
         "properties": {"kind": {"const": "arc"}, "radius": {"type": "number"}}}
    ]
}));

schema_impl!(ResourceBody, "ResourceBody", |_g| json_schema!({
    "type": "object",
    "description": "The content of a resource. `type` names it (plant_style, record_format, worksheet, hatch, tile, line_type, symbol, ... see describe: resources). The other keys depend on the type.",
    "required": ["type"],
    "properties": {"type": {"type": "string"}},
    "additionalProperties": true
}));

schema_impl!(crate::sheets::SavedView, "SavedView", |_g| opaque(
    "A saved view: name, layer visibility, active layer and camera."
));
schema_impl!(crate::sheets::PaperSize, "PaperSize", |_g| string_enum::<
    crate::sheets::PaperSize,
>());
schema_impl!(
    crate::sheets::Orientation,
    "Orientation",
    |_g| string_enum::<crate::sheets::Orientation>()
);
schema_impl!(crate::plants::variation::Variation, "Variation", |_g| {
    opaque("Random variation of placed plants: seed and ranges.")
});
schema_impl!(
    crate::present::SlotTarget,
    "SlotTarget",
    |_g| tagged_object::<crate::present::SlotTarget>(
        "kind",
        "A place that takes a material: a class, an object, or a hatch."
    )
);
schema_impl!(crate::present::SunSettings, "SunSettings", |_g| opaque(
    "The sun: latitude, longitude, date and time."
));
schema_impl!(crate::georef::Georeference, "Georeference", |_g| opaque(
    "Where the drawing sits on the earth: CRS and origin."
));
schema_impl!(crate::landscape::LandscapePatch, "LandscapePatch", |_g| {
    opaque("Landscape area settings to change. Missing fields stay.")
});
schema_impl!(crate::hardscape::HardscapePatch, "HardscapePatch", |_g| {
    opaque("Hardscape settings to change. Missing fields stay.")
});
schema_impl!(crate::visibility::VisMode, "VisMode", |_g| string_enum::<
    crate::visibility::VisMode,
>());

/// The JSON Schema (draft 2020-12) of a command, as JSON. Stable: the keys are sorted, so the same
/// code always gives the same text.
pub fn command_schema() -> Value {
    let schema = schemars::generate::SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<Command>();
    sorted(serde_json::to_value(schema).expect("a schema is JSON"))
}

/// The same JSON with the keys of every object in order, whatever the features of `serde_json`.
fn sorted(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<String> = m.keys().cloned().collect();
            keys.sort();
            let mut m = m;
            let mut out = serde_json::Map::new();
            for k in keys {
                let item = m.remove(&k).expect("key exists");
                out.insert(k, sorted(item));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(sorted).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_read_from_serde() {
        let kinds = variant_names::<Shape>(Some("kind"));
        for k in [
            "line",
            "rect",
            "circle",
            "arc",
            "polyline",
            "hardscape",
            "irrigation",
        ] {
            assert!(kinds.contains(&k.to_string()), "{k} in {kinds:?}");
        }
        assert_eq!(variant_names::<LayerKind>(None), vec!["design", "sheet"]);
        assert!(variant_names::<Color>(None).is_empty());
    }

    #[test]
    fn every_command_op_is_in_the_schema() {
        let schema = command_schema();
        let ops: Vec<String> = schema["oneOf"]
            .as_array()
            .expect("an enum of commands is a oneOf")
            .iter()
            .map(|v| v["properties"]["op"]["const"].as_str().unwrap().to_string())
            .collect();
        for needed in [
            "add_object",
            "modify_object",
            "delete_objects",
            "move_objects",
            "batch",
            "transform_objects",
            "array_objects",
            "set_visibility",
            "rotate_objects",
            "copy_objects",
        ] {
            assert!(ops.contains(&needed.to_string()), "{needed} in {ops:?}");
        }
        // Each op once.
        let mut sorted = ops.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ops.len());
        // The command schema accepts the op tag name of every variant: a command whose op the
        // serde implementation knows is in the list, and the other way round.
        let known = variant_names::<Command>(Some("op"));
        let mut a = known.clone();
        a.sort();
        assert_eq!(a, sorted);
    }

    #[test]
    fn the_schema_is_stable() {
        let a = serde_json::to_string(&command_schema()).unwrap();
        let b = serde_json::to_string(&command_schema()).unwrap();
        assert_eq!(a, b);
        assert!(a.contains("2020-12"));
    }
}

schema_impl!(crate::viewrules::ViewRules, "ViewRules", |_g| opaque(
    "Display rules: {ne_grid?: {spacing, labels?, label_height?, color?}, phases?: {enabled, existing, new, demolished}, visualization?: {name, field, rules, target?, enabled?, kinds?, include_missing?}}. See view_rules.get and the methods grid.set, phase.display and visualization.set."
));
