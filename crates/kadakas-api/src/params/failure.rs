//! The errors of the parameter check (-32602). They name the parameter, and what the method
//! expects in its place.

use serde_json::{Value, json};

use crate::RpcError;

use super::refs::nearest;

/// A parameter that the method does not take: the error names it and the nearest known name.
/// `details`: `unknown`, `suggestions` (name to nearest known name), `known`.
pub(super) fn unknown_error(method: &str, known: &[String], unknown: &[&String]) -> RpcError {
    let mut parts = Vec::new();
    let mut suggestions = serde_json::Map::new();
    for name in unknown {
        match nearest(name, known.iter().map(String::as_str)) {
            Some(near) => {
                parts.push(format!("'{name}' (did you mean '{near}'?)"));
                suggestions.insert((*name).clone(), json!(near));
            }
            None => parts.push(format!("'{name}'")),
        }
    }
    let noun = if unknown.len() == 1 {
        "parameter"
    } else {
        "parameters"
    };
    let takes = if known.is_empty() {
        "it takes no parameters".to_string()
    } else {
        format!("it takes: {}", known.join(", "))
    };
    RpcError::invalid_params(format!(
        "unknown {noun} {} for {method}; {takes}. describe {{method: \"{method}\"}} has the schema",
        parts.join(", ")
    ))
    .with_details(json!({
        "unknown": unknown,
        "suggestions": suggestions,
        "known": known,
    }))
}

/// None of a group of parameters was given, and one of them is needed.
pub(super) fn one_of_error(method: &str, group: &[String]) -> RpcError {
    RpcError::invalid_params(format!(
        "{method} needs at least one of these parameters: {}",
        group.join(", ")
    ))
    .with_details(json!({"missing_one_of": group}))
}

/// The words for a JSON type in a schema.
fn type_words(t: &str) -> &'static str {
    match t {
        "string" => "a string",
        "integer" => "an integer",
        "number" => "a number",
        "boolean" => "a boolean",
        "array" => "an array",
        "object" => "an object",
        "null" => "null",
        _ => "another type",
    }
}

/// The schema of a parameter that may be left out: `null` ("not given") is not a value to ask for.
fn without_null(schema: &Value) -> Value {
    let mut plain = schema.clone();
    if let Some(Value::Array(types)) = schema.get("type") {
        let others: Vec<&Value> = types.iter().filter(|t| *t != "null").collect();
        if others.len() == 1 && types.len() == 2 {
            plain["type"] = others[0].clone();
        }
    }
    plain
}

/// What a schema accepts, in words.
fn expected_words(schema: &Value) -> String {
    if let Some(Value::Array(values)) = schema.get("enum") {
        let list: Vec<String> = values
            .iter()
            .filter(|v| !v.is_null())
            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string))
            .collect();
        return format!("one of {}", list.join(", "));
    }
    if let Some(Value::String(c)) = schema.get("const") {
        return c.clone();
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(Value::Array(options)) = schema.get(key) {
            let mut words: Vec<String> = options.iter().map(expected_words).collect();
            words.dedup();
            return words.join(" or ");
        }
    }
    match schema.get("type") {
        Some(Value::String(t)) => {
            let mut text = type_words(t).to_string();
            if t == "array" {
                let fixed = match (schema.get("minItems"), schema.get("maxItems")) {
                    (Some(a), Some(b)) if a == b => Some(a.to_string()),
                    _ => None,
                };
                let item = schema.get("items").map(expected_words);
                let many = |i: &str| {
                    if i.starts_with("an array of 2 numbers [x, y]") {
                        "points [x, y]".to_string()
                    } else if i.starts_with("an array of 2 or 3 numbers") {
                        "points [x, y, z]".to_string()
                    } else {
                        plural(i)
                    }
                };
                let spread = (schema.get("minItems"), schema.get("maxItems"));
                text = match (fixed, item) {
                    (Some(n), Some(i)) if !i.starts_with("any") => {
                        if n == "2" && i == "a number" {
                            "an array of 2 numbers [x, y]".to_string()
                        } else {
                            format!("an array of {n} {}", many(&i))
                        }
                    }
                    (None, Some(i))
                        if spread == (Some(&json!(2)), Some(&json!(3))) && i == "a number" =>
                    {
                        "an array of 2 or 3 numbers [x, y, z]".to_string()
                    }
                    (None, Some(i)) if !i.starts_with("any") => {
                        format!("an array of {}", many(&i))
                    }
                    (Some(n), _) => format!("an array of {n} items"),
                    _ => text,
                };
                if let Some(d) = schema.get("description").and_then(Value::as_str)
                    && d.starts_with('[')
                    && !text.contains(&d[..d.find(']').map_or(d.len(), |i| i + 1)])
                {
                    text = format!("{text} ({d})");
                }
            }
            text
        }
        Some(Value::Array(types)) => types
            .iter()
            .filter_map(Value::as_str)
            .map(type_words)
            .collect::<Vec<_>>()
            .join(" or "),
        _ => "any value".to_string(),
    }
}

/// "a number" is "numbers" in a list.
fn plural(words: &str) -> String {
    let bare = words
        .strip_prefix("an ")
        .or_else(|| words.strip_prefix("a "))
        .unwrap_or(words);
    if bare.contains(' ') || bare == "null" {
        format!("({words})")
    } else {
        format!("{bare}s")
    }
}

/// How a value is called in a message.
fn got_words(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(_) => "a boolean".into(),
        Value::Number(n) => {
            if n.is_f64() {
                "a number with a fraction".into()
            } else {
                "an integer".into()
            }
        }
        Value::String(s) => {
            let mut short: String = s.chars().take(40).collect();
            if short.len() < s.len() {
                short.push_str("...");
            }
            format!("the text \"{short}\"")
        }
        Value::Array(_) => "an array".into(),
        Value::Object(_) => "an object".into(),
    }
}

/// One step on the way to a parameter: a name or a position in a list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Seg {
    Key(String),
    Index(usize),
}

/// The way to a parameter, as serde_path_to_error saw it.
pub(crate) fn segments(path: &serde_path_to_error::Path) -> Vec<Seg> {
    use serde_path_to_error::Segment;
    path.iter()
        .filter_map(|s| match s {
            Segment::Map { key } => Some(Seg::Key(key.clone())),
            Segment::Seq { index } => Some(Seg::Index(*index)),
            _ => None,
        })
        .collect()
}

/// `a.b[2]` for the way to a parameter; empty for the object itself.
fn show(segs: &[Seg]) -> String {
    let mut out = String::new();
    for s in segs {
        match s {
            Seg::Key(k) if out.is_empty() => out.push_str(k),
            Seg::Key(k) => {
                out.push('.');
                out.push_str(k);
            }
            Seg::Index(i) => out.push_str(&format!("[{i}]")),
        }
    }
    out
}

/// A union of parameter sets reads the set that its tag names with its own call of serde, which
/// loses the way to the parameter. It puts the way in the text of the error, and the check reads
/// it back.
pub(crate) fn nested_error(e: &serde_path_to_error::Error<serde_json::Error>) -> String {
    let mut text = String::from("\u{1}");
    for s in segments(e.path()) {
        match s {
            Seg::Key(k) => text.push_str(&format!("k{k}\u{2}")),
            Seg::Index(i) => text.push_str(&format!("i{i}\u{2}")),
        }
    }
    text.push('\u{1}');
    text.push_str(&e.inner().to_string());
    text
}

/// Splits the text of [`nested_error`]: the way and the message.
fn unnest(text: &str) -> Option<(Vec<Seg>, String)> {
    let rest = text.strip_prefix('\u{1}')?;
    let (way, message) = rest.split_once('\u{1}')?;
    let segs = way
        .split('\u{2}')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_at(1) {
            ("i", n) => Seg::Index(n.parse().unwrap_or(0)),
            (_, k) => Seg::Key(k.to_string()),
        })
        .collect();
    Some((segs, message.to_string()))
}

/// The schema of the value at a place of the parameters, if the way leads through properties,
/// items and map values.
fn schema_at<'a>(schema: &'a Value, segs: &[Seg]) -> Option<&'a Value> {
    let mut s = schema;
    for seg in segs {
        s = match seg {
            Seg::Key(key) => property(s, key.as_str())
                .or_else(|| s.get("additionalProperties").filter(|a| a.is_object()))?,
            Seg::Index(_) => s.get("items")?,
        };
    }
    Some(s)
}

/// The schema of a property, also in the branches of a union of parameter sets.
fn property<'a>(schema: &'a Value, key: &str) -> Option<&'a Value> {
    if let Some(p) = schema.get("properties").and_then(|p| p.get(key)) {
        return Some(p);
    }
    ["oneOf", "anyOf", "allOf"]
        .iter()
        .filter_map(|k| schema.get(k).and_then(Value::as_array))
        .flatten()
        .find_map(|branch| property(branch, key))
}

/// The value at a place of the input.
fn value_at<'a>(input: &'a Value, segs: &[Seg]) -> Option<&'a Value> {
    let mut v = input;
    for seg in segs {
        v = match seg {
            Seg::Key(key) => v.get(key.as_str())?,
            Seg::Index(i) => v.get(*i)?,
        };
    }
    Some(v)
}

/// The text between the first pair of backticks after `after`.
fn quoted<'a>(text: &'a str, after: &str) -> Option<&'a str> {
    let rest = text.split(after).nth(1)?;
    rest.split('`').nth(1)
}

/// The names between backticks after "expected" (`expected one of `a`, `b``).
fn expected_names(text: &str) -> Vec<String> {
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
}

/// serde's words for a type, in ours.
fn serde_type_words(t: &str) -> String {
    match t.trim() {
        "u8" => "an integer from 0 to 255".into(),
        "u16" => "an integer from 0 to 65535".into(),
        "u32" | "u64" | "usize" => "a whole number that is not negative".into(),
        "i64" | "i32" | "isize" => "an integer".into(),
        "f64" | "f32" => "a number".into(),
        "a string" | "a borrowed string" => "a string".into(),
        "a boolean" => "a boolean".into(),
        "a sequence" => "an array".into(),
        "a map" => "an object".into(),
        s if s.starts_with("struct ") || s.starts_with("a struct") => "an object".into(),
        s if s.starts_with("an array of length") => {
            s.replace("an array of length", "an array with") + " items"
        }
        other => other.to_string(),
    }
}

/// A parameter that is missing, has the wrong type or a value that is not allowed.
pub(super) fn type_error(
    method: &str,
    e: &serde_path_to_error::Error<serde_json::Error>,
    input: &Value,
    schema: &Value,
) -> RpcError {
    let mut segs = segments(e.path());
    let mut text = e.inner().to_string();
    if let Some((way, message)) = unnest(&text) {
        segs.extend(way);
        text = message;
    }
    describe(method, segs, &text, input, schema)
}

fn describe(
    method: &str,
    mut segs: Vec<Seg>,
    text: &str,
    input: &Value,
    schema: &Value,
) -> RpcError {
    // The tag of a union: serde reports the variant at the root; the parameter is the one whose
    // value it is.
    if let (Some(variant), Value::Object(map)) = (quoted(text, "unknown variant"), input)
        && segs.is_empty()
        && let Some((key, _)) = map.iter().find(|(_, v)| v.as_str() == Some(variant))
    {
        segs.push(Seg::Key(key.clone()));
    }
    let name = show(&segs);
    // A required parameter that is not there. The way ends at the object that lacks it.
    if let Some(field) = quoted(text, "missing field") {
        let full = if name.is_empty() {
            field.to_string()
        } else {
            format!("{name}.{field}")
        };
        let mut inner = segs.clone();
        inner.push(Seg::Key(field.to_string()));
        let expected = schema_at(schema, &inner).map(|s| expected_words(&without_null(s)));
        let mut msg = format!("missing '{full}': {method} needs this parameter");
        if let Some(x) = &expected {
            msg.push_str(&format!(" ({x})"));
        }
        return RpcError::invalid_params(msg).with_details(json!({
            "parameter": full,
            "problem": "missing",
            "expected": expected,
        }));
    }
    // A name that a nested object does not take.
    if let Some(field) = quoted(text, "unknown field") {
        let known = expected_names(text);
        // serde_path_to_error puts the unknown name at the end of the way already.
        let full = match segs.last() {
            Some(Seg::Key(k)) if k == field => name.clone(),
            _ if name.is_empty() => field.to_string(),
            _ => format!("{name}.{field}"),
        };
        let hint = nearest(field, known.iter().map(String::as_str))
            .map(|n| format!(" (did you mean '{n}'?)"))
            .unwrap_or_default();
        let takes = if known.is_empty() {
            String::new()
        } else {
            format!("; it takes: {}", known.join(", "))
        };
        return RpcError::invalid_params(format!(
            "unknown parameter '{full}'{hint} for {method}{takes}"
        ))
        .with_details(json!({"unknown": [full], "known": known}));
    }
    // A parameter (one step) may be null as well; a value inside a list or a map is as it is.
    let at = schema_at(schema, &segs).map(|s| {
        if segs.len() <= 1 {
            without_null(s)
        } else {
            s.clone()
        }
    });
    let at = at.as_ref();
    let value = value_at(input, &segs);
    // A value of the right type that is not allowed (a number out of range) is shown as it is.
    let out_of_range = text.starts_with("invalid value");
    let got = if out_of_range {
        value.map(|v| format!("{v}"))
    } else {
        value.map(got_words)
    };
    // What the parameter expects: the schema says it best (it lists the values of an enum and
    // the shape of a point); serde's words when the schema does not reach the place.
    let from_schema = at
        .filter(|_| !out_of_range && !text.starts_with("unknown variant"))
        .map(expected_words)
        .filter(|w| w != "any value");
    let expected = if let Some(w) = from_schema {
        w
    } else if let Some(rest) = text
        .strip_prefix("invalid type: ")
        .or_else(|| text.strip_prefix("invalid value: "))
        && let Some((_, want)) = rest.rsplit_once(", expected ")
    {
        serde_type_words(want)
    } else if text.starts_with("unknown variant") {
        format!("one of {}", expected_names(text).join(", "))
    } else {
        at.map(expected_words).unwrap_or_else(|| text.to_string())
    };
    let msg = match (&got, name.is_empty()) {
        (Some(g), false) => format!("parameter '{name}': expected {expected}, got {g}"),
        (None, false) => format!("parameter '{name}': expected {expected} ({text})"),
        // The object itself is always there.
        (_, true) => format!(
            "{method} takes an object: expected {expected}, got {}",
            got.unwrap_or_default()
        ),
    };
    RpcError::invalid_params(msg).with_details(json!({
        "parameter": name,
        "problem": "wrong_value",
        "expected": expected,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_words_for_types_are_ours() {
        for (serde, ours) in [
            ("u8", "an integer from 0 to 255"),
            ("u16", "an integer from 0 to 65535"),
            ("u32", "a whole number that is not negative"),
            ("i64", "an integer"),
            ("f64", "a number"),
            ("a string", "a string"),
            ("a boolean", "a boolean"),
            ("a sequence", "an array"),
            ("a map", "an object"),
            ("struct Foo", "an object"),
            ("an array of length 2", "an array with 2 items"),
            ("something else", "something else"),
        ] {
            assert_eq!(serde_type_words(serde), ours);
        }
    }

    #[test]
    fn values_are_named_by_their_type() {
        assert_eq!(got_words(&json!(null)), "null");
        assert_eq!(got_words(&json!(true)), "a boolean");
        assert_eq!(got_words(&json!(2)), "an integer");
        assert_eq!(got_words(&json!(2.5)), "a number with a fraction");
        assert_eq!(got_words(&json!([1])), "an array");
        assert_eq!(got_words(&json!({})), "an object");
        let long = "x".repeat(60);
        assert!(got_words(&json!(long)).ends_with("...\""));
    }

    #[test]
    fn the_schema_says_what_is_expected() {
        let s = json!({"type": "array", "items": {"type": "integer"}});
        assert_eq!(expected_words(&s), "an array of integers");
        let s = json!({"type": ["string", "null"]});
        assert_eq!(expected_words(&s), "a string or null");
        assert_eq!(expected_words(&without_null(&s)), "a string");
        let s = json!({"type": ["string", "integer"]});
        assert_eq!(expected_words(&s), "a string or an integer");
        let s = json!({"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 3});
        assert_eq!(expected_words(&s), "an array of 2 or 3 numbers [x, y, z]");
        let s = json!({"type": "array", "minItems": 4, "maxItems": 4});
        assert_eq!(expected_words(&s), "an array of 4 items");
        let s = json!({"const": "a"});
        assert_eq!(expected_words(&s), "a");
        assert_eq!(expected_words(&json!(true)), "any value");
        assert_eq!(plural("null"), "(null)");
    }

    #[test]
    fn errors_say_the_way_to_the_parameter() {
        let schema = json!({"type": "object", "properties": {"a": {"type": "array", "items": {"type": "object", "properties": {"b": {"type": "integer"}}}}}});
        let input = json!({"a": [{"b": "x"}]});
        let way = vec![Seg::Key("a".into()), Seg::Index(0), Seg::Key("b".into())];
        let e = describe(
            "m",
            way.clone(),
            "invalid type: string \"x\", expected i64",
            &input,
            &schema,
        );
        assert!(e.message.contains("parameter 'a[0].b'"), "{}", e.message);
        // serde's words, when the schema does not reach the place.
        let e = describe(
            "m",
            vec![Seg::Key("z".into())],
            "invalid type: string \"x\", expected u16",
            &input,
            &schema,
        );
        assert!(
            e.message.contains("an integer from 0 to 65535"),
            "{}",
            e.message
        );
        // No value there: the message carries the text of serde.
        let e = describe(
            "m",
            vec![Seg::Key("z".into())],
            "something odd",
            &input,
            &schema,
        );
        assert!(e.message.contains("something odd"), "{}", e.message);
        // The object itself.
        let e = describe("m", vec![], "something odd", &json!(null), &schema);
        assert_eq!(e.message, "m takes an object: expected an object, got null");
        let e = describe(
            "m",
            vec![],
            "invalid type: sequence, expected a map",
            &json!([1]),
            &json!({}),
        );
        assert!(e.message.contains("m takes an object"), "{}", e.message);
        // A name that a nested object does not take, and one that is missing there.
        let e = describe(
            "m",
            vec![Seg::Key("a".into())],
            "unknown field `c`, expected `b`",
            &input,
            &schema,
        );
        assert!(
            e.message.contains("unknown parameter 'a.c'"),
            "{}",
            e.message
        );
        // The way of serde_path_to_error ends with the unknown name already.
        let way = vec![Seg::Key("a".into()), Seg::Key("c".into())];
        let e = describe("m", way, "unknown field `c`, expected `b`", &input, &schema);
        assert!(
            e.message
                .contains("unknown parameter 'a.c' (did you mean 'b'?)"),
            "{}",
            e.message
        );
        let e = describe(
            "m",
            vec![],
            "unknown field `q`, expected `b`",
            &input,
            &schema,
        );
        assert!(e.message.contains("unknown parameter 'q'"), "{}", e.message);
        let e = describe(
            "m",
            vec![Seg::Key("a".into())],
            "unknown field `q`, there are no fields",
            &input,
            &schema,
        );
        assert!(!e.message.contains("it takes"), "{}", e.message);
        let e = describe(
            "m",
            vec![Seg::Key("a".into())],
            "missing field `b`",
            &input,
            &schema,
        );
        assert!(e.message.contains("missing 'a.b'"), "{}", e.message);
        // The way through an error that a union of parameter sets passes on.
        let text = "\u{1}ka\u{2}i0\u{2}\u{1}invalid type: string \"x\", expected i64";
        let (way, message) = unnest(text).unwrap();
        assert_eq!(way, vec![Seg::Key("a".into()), Seg::Index(0)]);
        assert!(message.starts_with("invalid type"));
        assert!(unnest("plain").is_none());
    }
}
