//! The parameter schema of a plug-in object, and the checks of parameter values.
//!
//! The JSON is the same as for the parameters of Python plug-in objects, so one editor can show both.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::MAX_VALUE;

/// Longest text value, in characters.
pub const MAX_TEXT: usize = 10_000;

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
    /// A group of fields (`fields`). The value is an object with one entry per field.
    Record,
}

/// One parameter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamSchema {
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
    pub fields: Vec<ParamSchema>,
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

impl ParamSchema {
    /// A parameter with only a name and a type.
    pub fn new(name: &str, ty: ParamType) -> Self {
        ParamSchema {
            name: name.into(),
            label: String::new(),
            ty,
            default: Value::Null,
            min: None,
            max: None,
            options: vec![],
            fields: vec![],
        }
    }

    /// Sets the default value.
    pub fn with_default(mut self, v: Value) -> Self {
        self.default = v;
        self
    }

    /// Sets the label.
    pub fn with_label(mut self, label: &str) -> Self {
        self.label = label.into();
        self
    }

    /// Sets the range of a number or a length.
    pub fn with_range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// The default value of the parameter.
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
                if !x.is_finite() || x.abs() > MAX_VALUE {
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

/// Reads a color value (`#rrggbb` or `#rrggbbaa`) as `[r, g, b, a]`. Plug-ins use it for the
/// parameters of type `color`.
pub fn color_rgba(text: &str) -> Option<[u8; 4]> {
    let h = normalize_color(text)?;
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([
        byte(1)?,
        byte(3)?,
        byte(5)?,
        if h.len() == 9 { byte(7)? } else { 255 },
    ])
}

/// Fills in defaults and checks every value. Unknown names are an error.
pub fn resolve(
    schema: &[ParamSchema],
    given: &Map<String, Value>,
) -> Result<Map<String, Value>, String> {
    if let Some(k) = given.keys().find(|k| !schema.iter().any(|d| &d.name == *k)) {
        return Err(format!(
            "unknown parameter '{k}'. Parameters: {}",
            schema
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut out = Map::new();
    for d in schema {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn num() -> ParamSchema {
        ParamSchema::new("w", ParamType::Length)
            .with_default(json!(100))
            .with_range(10.0, 500.0)
    }

    #[test]
    fn numbers_are_checked_against_range_and_size() {
        let p = num();
        assert_eq!(p.coerce(&json!(50)).unwrap(), json!(50.0));
        assert!(p.coerce(&json!(5)).unwrap_err().contains("outside"));
        assert!(p.coerce(&json!("x")).is_err());
        let free = ParamSchema::new("n", ParamType::Number);
        assert!(free.coerce(&json!(1e12)).is_err());
        assert!(free.coerce(&json!(-3)).is_ok());
        let half = ParamSchema {
            max: Some(1.0),
            ..ParamSchema::new("h", ParamType::Number)
        };
        assert!(half.coerce(&json!(2)).unwrap_err().contains("-inf"));
    }

    #[test]
    fn every_type_has_a_default_and_a_check() {
        let mut choice = ParamSchema::new("c", ParamType::Choice);
        choice.options = vec!["a".into(), "b".into()];
        let mut rec = ParamSchema::new("r", ParamType::Record);
        rec.fields = vec![ParamSchema::new("t", ParamType::Text)];
        let all = [
            ParamSchema::new("n", ParamType::Number),
            ParamSchema::new("t", ParamType::Text),
            ParamSchema::new("b", ParamType::Bool),
            ParamSchema::new("k", ParamType::Color),
            choice.clone(),
            rec.clone(),
        ];
        for p in &all {
            p.validate().unwrap();
            assert!(p.coerce(&p.default_value()).is_ok(), "{}", p.name);
        }
        assert_eq!(choice.default_value(), json!("a"));
        assert_eq!(rec.default_value(), json!({"t": ""}));
        assert_eq!(
            ParamSchema::new("k", ParamType::Color)
                .coerce(&json!("#AABBCC"))
                .unwrap(),
            json!("#aabbcc")
        );
        assert!(choice.coerce(&json!("z")).unwrap_err().contains("a, b"));
        assert!(choice.coerce(&json!(1)).is_err());
        assert!(rec.coerce(&json!(1)).is_err());
        assert!(rec.coerce(&json!({"t": 1})).is_err());
        let b = ParamSchema::new("b", ParamType::Bool);
        assert!(b.coerce(&json!(1)).is_err());
        assert!(
            ParamSchema::new("k", ParamType::Color)
                .coerce(&json!("red"))
                .is_err()
        );
        assert!(
            ParamSchema::new("k", ParamType::Color)
                .coerce(&json!(1))
                .is_err()
        );
        let t = ParamSchema::new("t", ParamType::Text);
        assert!(t.coerce(&json!(1)).is_err());
        assert!(t.coerce(&json!("x".repeat(MAX_TEXT + 1))).is_err());
        // The default of a choice without options is empty text, and the check refuses it.
        assert_eq!(
            ParamSchema::new("c", ParamType::Choice).default_value(),
            json!("")
        );
    }

    #[test]
    fn bad_definitions_are_refused() {
        let bad = |p: ParamSchema, what: &str| {
            let e = p.validate().unwrap_err();
            assert!(e.contains(what), "{e}");
        };
        bad(ParamSchema::new("1x", ParamType::Text), "must start");
        bad(ParamSchema::new("c", ParamType::Choice), "needs options");
        bad(ParamSchema::new("r", ParamType::Record), "needs fields");
        let mut r = ParamSchema::new("r", ParamType::Record);
        let mut inner = ParamSchema::new("i", ParamType::Record);
        inner.fields = vec![ParamSchema::new("x", ParamType::Text)];
        r.fields = vec![inner];
        bad(r, "cannot hold records");
        let mut dup = ParamSchema::new("r", ParamType::Record);
        dup.fields = vec![
            ParamSchema::new("x", ParamType::Text),
            ParamSchema::new("x", ParamType::Text),
        ];
        bad(dup, "appears twice");
        let mut f = ParamSchema::new("t", ParamType::Text);
        f.fields = vec![ParamSchema::new("x", ParamType::Text)];
        bad(f, "only a record");
        let mut o = ParamSchema::new("t", ParamType::Text);
        o.options = vec!["a".into()];
        bad(o, "only a choice");
        let mut nan = ParamSchema::new("n", ParamType::Number);
        nan.min = Some(f64::NAN);
        bad(nan, "finite");
        bad(
            ParamSchema::new("n", ParamType::Number).with_range(5.0, 1.0),
            "greater than max",
        );
        bad(
            ParamSchema::new("n", ParamType::Number)
                .with_range(0.0, 1.0)
                .with_default(json!(5)),
            "default",
        );
    }

    #[test]
    fn resolve_fills_defaults_and_refuses_unknown_names() {
        let schema = vec![
            num().with_label("Width"),
            ParamSchema::new("on", ParamType::Bool),
        ];
        let out = resolve(&schema, &Map::new()).unwrap();
        assert_eq!(out["w"], json!(100.0));
        assert_eq!(out["on"], json!(false));
        let mut given = Map::new();
        given.insert("w".into(), json!(200));
        assert_eq!(resolve(&schema, &given).unwrap()["w"], json!(200.0));
        given.insert("zz".into(), json!(1));
        assert!(
            resolve(&schema, &given)
                .unwrap_err()
                .contains("unknown parameter 'zz'")
        );
        given.remove("zz");
        given.insert("w".into(), json!(1));
        assert!(
            resolve(&schema, &given)
                .unwrap_err()
                .starts_with("parameter w")
        );
        // A bad default shows up when the value is missing.
        let broken = vec![ParamSchema::new("c", ParamType::Choice)];
        assert!(resolve(&broken, &Map::new()).is_err());
    }

    #[test]
    fn colors_read_as_rgba() {
        assert_eq!(color_rgba("#ff8000"), Some([255, 128, 0, 255]));
        assert_eq!(color_rgba("#FF800040"), Some([255, 128, 0, 64]));
        assert_eq!(color_rgba("ff8000"), None);
        assert_eq!(color_rgba("#12"), None);
    }

    #[test]
    fn schema_json_round_trips() {
        let p = num().with_label("Width");
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["type"], "length");
        assert_eq!(serde_json::from_value::<ParamSchema>(v).unwrap(), p);
    }
}
