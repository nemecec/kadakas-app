//! Record formats: named sets of typed fields that users attach to objects.
//!
//! A record format is a resource (`ResourceBody::RecordFormat`). Its resource name is the record
//! name. An object with an attached record keeps the values in `Object::data[<record name>]`
//! as a JSON object with one key per field. Commands check the values against the format.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::model::Object;

/// Largest absolute value of a number field.
pub const MAX_FIELD: f64 = 1.0e12;
/// Longest field or record name, in characters.
pub const MAX_NAME: usize = 64;

/// The type of a record field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    #[default]
    Text,
    Integer,
    Number,
    Boolean,
    /// A length in mm.
    Dimension,
    /// An area in m2.
    Area,
    /// One of the listed choices.
    Choice,
}

impl FieldType {
    pub const ALL: [FieldType; 7] =
        [FieldType::Text, FieldType::Integer, FieldType::Number, FieldType::Boolean, FieldType::Dimension, FieldType::Area, FieldType::Choice];

    pub fn id(self) -> &'static str {
        match self {
            FieldType::Text => "text",
            FieldType::Integer => "integer",
            FieldType::Number => "number",
            FieldType::Boolean => "boolean",
            FieldType::Dimension => "dimension",
            FieldType::Area => "area",
            FieldType::Choice => "choice",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id().eq_ignore_ascii_case(s.trim()))
    }

    /// True for the types that hold a real number.
    pub fn is_real(self) -> bool {
        matches!(self, FieldType::Number | FieldType::Dimension | FieldType::Area)
    }
}

/// One field of a record format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldDef {
    pub name: String,
    #[serde(rename = "type", default)]
    pub ty: FieldType,
    /// The value that a new record gets. Without it, the type decides: empty text, zero, false,
    /// or the first choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    /// Display unit, for example `mm` or `m2`. It is a label. The stored number does not change.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unit: String,
    /// The allowed values of a choice field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
}

impl FieldDef {
    pub fn new(name: &str, ty: FieldType) -> Self {
        FieldDef { name: name.into(), ty, default: None, unit: String::new(), choices: Vec::new() }
    }

    /// Checks one value against the type. Returns the value in its stored form: an integer field
    /// stores an integer, a choice stores the spelling of the list. Numbers and booleans are also
    /// read from strings, so a form can send what the user typed.
    pub fn coerce(&self, v: &Value) -> Result<Value, String> {
        let fail = |what: &str| Err(format!("field '{}' needs {what}, got {v}", self.name));
        match self.ty {
            FieldType::Text => match v {
                Value::String(s) if s.chars().count() <= 10_000 => Ok(v.clone()),
                Value::String(_) => fail("text of at most 10000 characters"),
                Value::Number(n) => Ok(Value::String(n.to_string())),
                Value::Bool(b) => Ok(Value::String(b.to_string())),
                _ => fail("text"),
            },
            FieldType::Integer => match as_number(v) {
                Some(n) if n.fract() == 0.0 => Ok(Value::from(n as i64)),
                _ => fail("a whole number"),
            },
            FieldType::Number | FieldType::Dimension | FieldType::Area => match as_number(v) {
                Some(n) => Ok(Value::from(n)),
                None => fail("a finite number"),
            },
            FieldType::Boolean => match v {
                Value::Bool(_) => Ok(v.clone()),
                Value::String(s) if s.eq_ignore_ascii_case("true") => Ok(Value::Bool(true)),
                Value::String(s) if s.eq_ignore_ascii_case("false") => Ok(Value::Bool(false)),
                _ => fail("true or false"),
            },
            FieldType::Choice => match v.as_str().and_then(|s| self.choices.iter().find(|c| c.eq_ignore_ascii_case(s.trim()))) {
                Some(c) => Ok(Value::String(c.clone())),
                None => Err(format!("field '{}' must be one of: {}", self.name, self.choices.join(", "))),
            },
        }
    }

    /// The value of this field in a new record.
    pub fn default_value(&self) -> Value {
        if let Some(d) = &self.default
            && let Ok(v) = self.coerce(d)
        {
            return v;
        }
        match self.ty {
            FieldType::Text => Value::String(String::new()),
            FieldType::Integer => Value::from(0),
            FieldType::Number | FieldType::Dimension | FieldType::Area => Value::from(0.0),
            FieldType::Boolean => Value::Bool(false),
            FieldType::Choice => Value::String(self.choices.first().cloned().unwrap_or_default()),
        }
    }
}

/// A finite number within range, from a JSON number or a numeric string.
fn as_number(v: &Value) -> Option<f64> {
    let n = match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    n.filter(|n| n.is_finite() && n.abs() <= MAX_FIELD)
}

/// A record format: the fields of a record. The record name is the resource name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordFormat {
    pub fields: Vec<FieldDef>,
}

/// Checks a record or field name: not empty, not too long, no control characters.
pub fn check_name(kind: &str, name: &str) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(format!("a {kind} needs a name"));
    }
    if n.chars().count() > MAX_NAME || n.chars().any(char::is_control) {
        return Err(format!("the {kind} name '{n}' is too long or has control characters"));
    }
    Ok(())
}

/// Names that an object's `data` already uses for other purposes.
pub fn is_reserved_record_name(name: &str) -> bool {
    let n = name.trim();
    crate::plants::link::RESERVED_DATA_KEYS.iter().any(|k| k.eq_ignore_ascii_case(n)) || n.eq_ignore_ascii_case(crate::plants::link::ROTATION_KEY)
}

impl RecordFormat {
    pub fn field(&self, name: &str) -> Option<&FieldDef> {
        self.fields.iter().find(|f| f.name.eq_ignore_ascii_case(name.trim()))
    }

    /// Checks names, choices and defaults.
    pub fn validate(&self) -> Result<(), String> {
        for (i, f) in self.fields.iter().enumerate() {
            check_name("field", &f.name)?;
            if self.fields[..i].iter().any(|o| o.name.trim().eq_ignore_ascii_case(f.name.trim())) {
                return Err(format!("two fields are named '{}'", f.name.trim()));
            }
            if f.ty == FieldType::Choice {
                if f.choices.is_empty() {
                    return Err(format!("choice field '{}' needs at least one choice", f.name));
                }
                if f.choices.iter().any(|c| c.trim().is_empty()) {
                    return Err(format!("choice field '{}' has an empty choice", f.name));
                }
                if let Some((_, c)) = f.choices.iter().enumerate().find(|(i, c)| f.choices[..*i].iter().any(|o| o.eq_ignore_ascii_case(c))) {
                    return Err(format!("choice field '{}' lists '{}' twice", f.name, c));
                }
            } else if !f.choices.is_empty() {
                return Err(format!("only choice fields have choices ('{}')", f.name));
            }
            if let Some(d) = &f.default {
                f.coerce(d).map_err(|e| format!("default: {e}"))?;
            }
        }
        Ok(())
    }

    /// A full record: every field has a value. Given values are checked, the others get the
    /// default. A name that is not a field is an error.
    pub fn build(&self, given: &Map<String, Value>) -> Result<Map<String, Value>, String> {
        self.merge(&self.defaults(), given)
    }

    /// The record of default values.
    pub fn defaults(&self) -> Map<String, Value> {
        self.fields.iter().map(|f| (f.name.clone(), f.default_value())).collect()
    }

    /// `existing` with the given values changed. Field names are matched without case.
    pub fn merge(&self, existing: &Map<String, Value>, given: &Map<String, Value>) -> Result<Map<String, Value>, String> {
        let mut out = existing.clone();
        for (k, v) in given {
            let f = self.field(k).ok_or_else(|| format!("no field '{k}'. Fields: {}", self.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ")))?;
            out.insert(f.name.clone(), f.coerce(v)?);
        }
        Ok(out)
    }

    /// Brings stored values in line with this format after the format changed: fields that no
    /// longer exist are dropped, new fields get their defaults, and values of the wrong type are
    /// replaced by the default.
    pub fn conform(&self, stored: &Map<String, Value>) -> Map<String, Value> {
        self.fields.iter().map(|f| (f.name.clone(), stored.get(&f.name).and_then(|v| f.coerce(v).ok()).unwrap_or_else(|| f.default_value()))).collect()
    }
}

/// The stored values of a record on an object.
pub fn values<'a>(obj: &'a Object, record: &str) -> Option<&'a Map<String, Value>> {
    obj.data.get(record)?.as_object()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn format() -> RecordFormat {
        let mut choice = FieldDef::new("Grade", FieldType::Choice);
        choice.choices = vec!["A".into(), "B".into()];
        let mut depth = FieldDef::new("Depth", FieldType::Dimension);
        depth.unit = "mm".into();
        depth.default = Some(json!(300));
        RecordFormat {
            fields: vec![
                FieldDef::new("Note", FieldType::Text),
                FieldDef::new("Qty", FieldType::Integer),
                depth,
                FieldDef::new("Wet", FieldType::Boolean),
                FieldDef::new("Cover", FieldType::Area),
                FieldDef::new("Ratio", FieldType::Number),
                choice,
            ],
        }
    }

    #[test]
    fn type_names() {
        for t in FieldType::ALL {
            assert_eq!(FieldType::from_name(t.id()), Some(t));
        }
        assert_eq!(FieldType::from_name(" TEXT "), Some(FieldType::Text));
        assert_eq!(FieldType::from_name("x"), None);
        assert!(FieldType::Area.is_real() && !FieldType::Integer.is_real());
    }

    #[test]
    fn defaults_follow_type_or_default_value() {
        let f = format();
        let d = f.defaults();
        assert_eq!(d["Note"], json!(""));
        assert_eq!(d["Qty"], json!(0));
        assert_eq!(d["Depth"], json!(300.0));
        assert_eq!(d["Wet"], json!(false));
        assert_eq!(d["Cover"], json!(0.0));
        assert_eq!(d["Grade"], json!("A"));
        let mut bad = FieldDef::new("N", FieldType::Integer);
        bad.default = Some(json!("x"));
        assert_eq!(bad.default_value(), json!(0));
        let empty_choice = FieldDef::new("C", FieldType::Choice);
        assert_eq!(empty_choice.default_value(), json!(""));
    }

    #[test]
    fn coerce_accepts_typed_and_typed_text() {
        let f = format();
        let c = |n: &str, v: Value| f.field(n).unwrap().coerce(&v);
        assert_eq!(c("Note", json!(5)).unwrap(), json!("5"));
        assert_eq!(c("Note", json!(true)).unwrap(), json!("true"));
        assert!(c("Note", json!(null)).is_err());
        assert!(c("Note", json!("x".repeat(10_001))).is_err());
        assert_eq!(c("Qty", json!(3.0)).unwrap(), json!(3));
        assert_eq!(c("Qty", json!(" 4 ")).unwrap(), json!(4));
        assert!(c("Qty", json!(3.5)).is_err());
        assert!(c("Qty", json!("a")).is_err());
        assert!(c("Qty", json!(true)).is_err());
        assert_eq!(c("Depth", json!("250.5")).unwrap(), json!(250.5));
        assert!(c("Depth", json!(1e13)).is_err());
        assert!(c("Depth", json!("nan")).is_err());
        assert_eq!(c("Wet", json!("TRUE")).unwrap(), json!(true));
        assert_eq!(c("Wet", json!("false")).unwrap(), json!(false));
        assert_eq!(c("Wet", json!(false)).unwrap(), json!(false));
        assert!(c("Wet", json!("yes")).is_err());
        assert_eq!(c("Grade", json!(" b ")).unwrap(), json!("B"));
        assert!(c("Grade", json!("C")).unwrap_err().contains("A, B"));
        assert!(c("Grade", json!(1)).is_err());
    }

    #[test]
    fn build_merge_and_conform() {
        let f = format();
        let given = json!({"qty": "7", "NOTE": "hi"});
        let r = f.build(given.as_object().unwrap()).unwrap();
        assert_eq!(r["Qty"], json!(7));
        assert_eq!(r["Note"], json!("hi"));
        assert_eq!(r["Depth"], json!(300.0));
        assert_eq!(r.len(), 7);
        let r2 = f.merge(&r, json!({"Wet": true}).as_object().unwrap()).unwrap();
        assert_eq!(r2["Wet"], json!(true));
        assert_eq!(r2["Qty"], json!(7));
        let e = f.build(json!({"zzz": 1}).as_object().unwrap()).unwrap_err();
        assert!(e.contains("no field 'zzz'") && e.contains("Note"), "{e}");
        let stored = json!({"Qty": "bad", "Old": 1, "Note": "keep"});
        let c = f.conform(stored.as_object().unwrap());
        assert_eq!(c["Qty"], json!(0));
        assert_eq!(c["Note"], json!("keep"));
        assert!(!c.contains_key("Old"));
        assert_eq!(c.len(), 7);
    }

    #[test]
    fn validate_catches_bad_formats() {
        assert!(format().validate().is_ok());
        assert!(RecordFormat::default().validate().is_ok());
        let mut f = format();
        f.fields.push(FieldDef::new("note", FieldType::Text));
        assert!(f.validate().unwrap_err().contains("two fields"));
        let f = RecordFormat { fields: vec![FieldDef::new(" ", FieldType::Text)] };
        assert!(f.validate().is_err());
        let f = RecordFormat { fields: vec![FieldDef::new("C", FieldType::Choice)] };
        assert!(f.validate().unwrap_err().contains("at least one"));
        let mut c = FieldDef::new("C", FieldType::Choice);
        c.choices = vec!["a".into(), " ".into()];
        assert!(RecordFormat { fields: vec![c.clone()] }.validate().unwrap_err().contains("empty choice"));
        c.choices = vec!["a".into(), "A".into()];
        assert!(RecordFormat { fields: vec![c.clone()] }.validate().unwrap_err().contains("twice"));
        let mut t = FieldDef::new("T", FieldType::Text);
        t.choices = vec!["a".into()];
        assert!(RecordFormat { fields: vec![t] }.validate().unwrap_err().contains("only choice"));
        let mut n = FieldDef::new("N", FieldType::Integer);
        n.default = Some(json!(1.5));
        assert!(RecordFormat { fields: vec![n] }.validate().unwrap_err().contains("default"));
    }

    #[test]
    fn names_and_reserved_names() {
        assert!(check_name("record", "Soil").is_ok());
        assert!(check_name("record", "  ").is_err());
        assert!(check_name("record", &"x".repeat(65)).is_err());
        assert!(check_name("record", "a\nb").is_err());
        assert!(is_reserved_record_name("Plant") && is_reserved_record_name(" rotation "));
        assert!(!is_reserved_record_name("Soil"));
    }

    #[test]
    fn json_shape_and_values_helper() {
        let f = format();
        let j = serde_json::to_value(&f).unwrap();
        assert_eq!(j["fields"][2]["type"], "dimension");
        assert_eq!(j["fields"][2]["unit"], "mm");
        assert!(j["fields"][0].get("unit").is_none());
        let back: RecordFormat = serde_json::from_value(j).unwrap();
        assert_eq!(back, f);
        let mut o = Object::new(
            crate::model::ObjectId(1),
            crate::model::FIRST_LAYER,
            crate::model::NONE_CLASS,
            crate::model::Shape::Circle { center: glam::DVec2::ZERO, radius: 1.0 },
        );
        assert!(values(&o, "Soil").is_none());
        o.data.insert("Soil".into(), json!({"a": 1}));
        assert_eq!(values(&o, "Soil").unwrap()["a"], json!(1));
        o.data.insert("Bad".into(), json!(5));
        assert!(values(&o, "Bad").is_none());
    }
}
