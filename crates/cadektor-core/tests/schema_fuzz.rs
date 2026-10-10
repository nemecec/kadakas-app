//! The command schema must be sound where it claims to be exact: anything that the schema accepts
//! for `add_object`, `add_object_in`, `modify_object` and `set_vertex` with one of the exact shape
//! kinds, serde accepts too. The test starts from good commands, damages them in random ways and
//! checks every result with a small validator for the parts of JSON Schema that the schema uses.

use cadektor_core::command::Command;
use cadektor_core::schema::{EXACT_SHAPE_KINDS, command_schema, structural_shape_kinds};
use proptest::prelude::*;
use serde_json::{Value, json};

static SCHEMA: std::sync::LazyLock<Value> = std::sync::LazyLock::new(command_schema);

fn check(schema: &Value, root: &Value, v: &Value) -> bool {
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        let target = if r == "#" {
            root.clone()
        } else {
            root["$defs"][r.rsplit('/').next().unwrap()].clone()
        };
        return check(&target, root, v);
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(opts) = schema.get(key).and_then(Value::as_array) {
            let n = opts.iter().filter(|o| check(o, root, v)).count();
            return if key == "anyOf" { n >= 1 } else { n == 1 };
        }
    }
    if let Some(t) = schema.get("type") {
        let types: Vec<&str> = match t {
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            other => other.as_str().into_iter().collect(),
        };
        let ok = types.iter().any(|t| match *t {
            "object" => v.is_object(),
            "array" => v.is_array(),
            "string" => v.is_string(),
            "boolean" => v.is_boolean(),
            "null" => v.is_null(),
            "integer" => v.is_i64() || v.is_u64(),
            "number" => v.is_number(),
            _ => false,
        });
        if !ok {
            return false;
        }
    }
    if schema.get("const").is_some_and(|c| c != v) {
        return false;
    }
    if schema
        .get("enum")
        .and_then(Value::as_array)
        .is_some_and(|e| !e.contains(v))
    {
        return false;
    }
    if let (Some(n), Some(x)) = (v.as_f64(), schema.get("minimum").and_then(Value::as_f64))
        && n < x
    {
        return false;
    }
    if let (Some(n), Some(x)) = (v.as_f64(), schema.get("maximum").and_then(Value::as_f64))
        && n > x
    {
        return false;
    }
    if let Some(o) = v.as_object() {
        if let Some(req) = schema.get("required").and_then(Value::as_array)
            && req.iter().any(|k| !o.contains_key(k.as_str().unwrap()))
        {
            return false;
        }
        let props = schema.get("properties").and_then(Value::as_object);
        for (k, sub) in props.into_iter().flatten() {
            if let Some(x) = o.get(k)
                && !check(sub, root, x)
            {
                return false;
            }
        }
        if schema.get("additionalProperties") == Some(&json!(false))
            && o.keys().any(|k| !props.is_some_and(|p| p.contains_key(k)))
        {
            return false;
        }
    }
    if let Some(a) = v.as_array() {
        let len = a.len() as u64;
        if schema
            .get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|m| len < m)
            || schema
                .get("maxItems")
                .and_then(Value::as_u64)
                .is_some_and(|m| len > m)
        {
            return false;
        }
        if let Some(items) = schema.get("items")
            && !a.iter().all(|x| check(items, root, x))
        {
            return false;
        }
    }
    true
}

/// Good shapes, one or more per exact kind.
fn seeds() -> Vec<Value> {
    vec![
        json!({"kind": "line", "a": [0, 0], "b": [1, 2]}),
        json!({"kind": "rect", "min": [0, 0], "max": [5, 5]}),
        json!({"kind": "circle", "center": [1, 1], "radius": 3}),
        json!({"kind": "arc", "center": [0, 0], "radius": 3, "start": 0, "end": 1.5}),
        json!({"kind": "polyline", "closed": true, "points": [[0, 0], {"p": [5, 0], "kind": "arc", "radius": 1}, {"p": [5, 5]}, {"p": [0, 5], "kind": "bezier"}]}),
        json!({"kind": "text", "at": [0, 0], "text": "x", "height": 2}),
        json!({"kind": "plant", "at": [0, 0], "species": "Oak", "spread": 3, "height": 5}),
        json!({"kind": "locus3d", "p": [1, 2, 3]}),
        json!({"kind": "polyline3d", "closed": false, "points": [[0, 0, 1], [1, 1, 2]]}),
        json!({"kind": "group", "name": "g"}),
        json!({"kind": "landscape_area", "name": "Lawn", "boundary": [[0, 0], [4, 0], [4, 3]]}),
        json!({"kind": "hardscape", "material": "Slate", "thickness": 50, "boundary": [[0, 0], [4, 0], [4, 3]]}),
    ]
}

fn commands(shape: Value, vertex_kind: Value, which: usize) -> Value {
    match which % 4 {
        0 => json!({"op": "add_object", "shape": shape}),
        1 => json!({"op": "add_object_in", "parent": 3, "shape": shape}),
        2 => json!({"op": "modify_object", "id": 4, "shape": shape}),
        _ => json!({"op": "set_vertex", "id": 4, "index": 1, "kind": vertex_kind}),
    }
}

/// All places in a value: a path of keys and indices.
fn paths(v: &Value, here: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    out.push(here.clone());
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                here.push(k.clone());
                paths(x, here, out);
                here.pop();
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                here.push(i.to_string());
                paths(x, here, out);
                here.pop();
            }
        }
        _ => {}
    }
}

fn get_mut<'a>(v: &'a mut Value, path: &[String]) -> Option<&'a mut Value> {
    let mut cur = v;
    for k in path {
        cur = match cur {
            Value::Object(m) => m.get_mut(k)?,
            Value::Array(a) => a.get_mut(k.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn junk(n: u8) -> Value {
    match n % 10 {
        0 => Value::Null,
        1 => json!(true),
        2 => json!("text"),
        3 => json!(-1),
        4 => json!(1.5),
        5 => json!([]),
        6 => json!({}),
        7 => json!([1]),
        8 => json!([1, 2, 3]),
        _ => json!({"kind": "arc"}),
    }
}

/// Damages the value in up to three places.
fn damage(mut v: Value, picks: &[(usize, u8, u8)]) -> Value {
    for (at, how, what) in picks {
        let mut all = Vec::new();
        paths(&v, &mut Vec::new(), &mut all);
        let path = all[at % all.len()].clone();
        if path.is_empty() {
            continue;
        }
        let (last, parent) = path.split_last().unwrap();
        let Some(p) = get_mut(&mut v, parent) else {
            continue;
        };
        match (how % 3, p) {
            (0, Value::Object(m)) => {
                m.remove(last);
            }
            (0, Value::Array(a)) => {
                if let Ok(i) = last.parse::<usize>() {
                    a.remove(i);
                }
            }
            (1, Value::Object(m)) => {
                m.insert(last.clone(), junk(*what));
            }
            (1, Value::Array(a)) => {
                if let Ok(i) = last.parse::<usize>() {
                    a[i] = junk(*what);
                }
            }
            (_, Value::Object(m)) => {
                m.insert(format!("extra{what}"), junk(*what));
            }
            (_, Value::Array(a)) => a.push(junk(*what)),
            _ => {}
        }
    }
    v
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    /// What the schema accepts for an exact shape kind, serde accepts too.
    #[test]
    fn the_schema_is_sound_for_exact_shapes(
        seed in 0usize..12,
        which in 0usize..4,
        picks in prop::collection::vec((0usize..64, 0u8..3, 0u8..10), 0..4),
    ) {
        let schema = &*SCHEMA;
        let vertex_kind = damage(json!({"kind": "arc", "radius": 2}), &picks);
        let cmd = commands(damage(seeds()[seed].clone(), &picks), vertex_kind, which);
        let exact = cmd["shape"]["kind"].as_str().is_none_or(|k| EXACT_SHAPE_KINDS.contains(&k));
        if exact && check(schema, schema, &cmd) {
            prop_assert!(serde_json::from_value::<Command>(cmd.clone()).is_ok(), "{cmd}");
        }
    }

    /// Whole commands are checked too: damaged points and ids.
    #[test]
    fn ids_and_points_in_commands_are_bounded(id in -5i64..5, big in prop::bool::ANY, x in prop::option::of(-3i64..3)) {
        let schema = &*SCHEMA;
        let id_value = if big { json!(u64::MAX) } else { json!(id) };
        let delta = x.map_or(json!("a"), |x| json!([x, 1]));
        let cmd = json!({"op": "move_objects", "ids": [id_value], "delta": delta});
        if check(schema, schema, &cmd) {
            prop_assert!(serde_json::from_value::<Command>(cmd.clone()).is_ok(), "{cmd}");
            prop_assert!(id >= 0 && !big);
        }
    }
}

#[test]
fn good_seeds_pass_and_bad_shapes_fail() {
    let schema = command_schema();
    for s in seeds() {
        let cmd = json!({"op": "add_object", "shape": s});
        assert!(check(&schema, &schema, &cmd), "{cmd}");
        assert!(serde_json::from_value::<Command>(cmd).is_ok());
    }
    let bad = [
        json!({"op": "add_object", "shape": {"kind": "line", "a": [0, 0]}}),
        json!({"op": "add_object", "shape": {"kind": "circle", "center": [0, 0], "radius": "big"}}),
        json!({"op": "add_object", "shape": {"kind": "teapot"}}),
        json!({"op": "add_object", "shape": {"kind": "polyline", "closed": true, "points": [[0, 0], {"p": [1, 1], "kind": "arc"}]}}),
        json!({"op": "set_vertex", "id": 1, "index": 0, "kind": {"kind": "arc"}}),
        json!({"op": "delete_objects", "ids": [18446744073709551615u64]}),
    ];
    for cmd in bad {
        assert!(!check(&schema, &schema, &cmd), "{cmd}");
    }
}

#[test]
fn exact_and_structural_kinds_cover_every_shape_kind() {
    let structural = structural_shape_kinds();
    assert!(structural.contains(&"massing".to_string()));
    assert!(
        EXACT_SHAPE_KINDS
            .iter()
            .all(|k| !structural.contains(&k.to_string()))
    );
    for k in EXACT_SHAPE_KINDS {
        let r =
            serde_json::from_value::<Command>(json!({"op": "add_object", "shape": {"kind": k}}));
        if let Err(e) = r {
            assert!(
                !e.to_string().contains("unknown variant"),
                "{k} is not a shape kind: {e}"
            );
        }
    }
}
