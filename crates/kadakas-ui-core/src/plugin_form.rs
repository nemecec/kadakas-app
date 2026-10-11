//! The form of a plug-in object in Object Info: the logic without egui (PLAT-10).
//!
//! The form reads the parameter schema of a plug-in (the JSON of `plugins.object`, field
//! `schema`) and the saved values. It keeps the text that a person is typing, checks it, and
//! gives back a [`Change`] only for a value that is valid and different from the saved one. The
//! window code draws the fields and sends each [`Change`] to `plugins.set_params`, which is one
//! undoable step with the change origin `script`.
//!
//! Field types: `number` (with a range and an optional unit text), `length` (in mm; the form shows
//! and reads the length unit of the person), `text`, `bool`, `choice`, `color` and `record` (a
//! group of fields; a change of one field gives the whole record).

use std::collections::HashMap;

use kadakas_l10n::tr;
use serde_json::{Map, Value, json};

/// Largest absolute value of a number parameter. The same as in the plug-in ABI.
pub const MAX_VALUE: f64 = 1.0e9;

/// Longest text value, in characters. The same as in the plug-in ABI.
pub const MAX_TEXT: usize = 10_000;

/// What kind of editor a field needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Number,
    Length,
    Text,
    Bool,
    Choice,
    Color,
    Record,
}

/// One field of the form.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    /// The label to show: the label of the schema, or the name.
    pub label: String,
    pub kind: Kind,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// The unit text of a number, from the plug-in. Empty if none.
    pub unit: String,
    pub options: Vec<String>,
    pub fields: Vec<Field>,
    default: Value,
}

/// Why an input is refused.
#[derive(Clone, Debug, PartialEq)]
pub enum Problem {
    NotANumber,
    TooSmall(String),
    TooLarge(String),
    OutOfRange(String, String),
    TooLong,
    BadColor,
    NotAnOption,
}

impl Problem {
    /// The message for the person, in the language of the UI.
    pub fn text(&self) -> String {
        match self {
            Problem::NotANumber => tr!("uic-plugin-form-not-a-number"),
            Problem::TooSmall(limit) => tr!("uic-plugin-form-below-min", limit = limit.as_str()),
            Problem::TooLarge(limit) => tr!("uic-plugin-form-above-max", limit = limit.as_str()),
            Problem::OutOfRange(min, max) => {
                tr!(
                    "uic-plugin-form-out-of-range",
                    min = min.as_str(),
                    max = max.as_str()
                )
            }
            Problem::TooLong => tr!("uic-plugin-form-too-long", max = MAX_TEXT),
            Problem::BadColor => tr!("uic-plugin-form-bad-color"),
            Problem::NotAnOption => tr!("uic-plugin-form-not-an-option"),
        }
    }
}

/// A valid new value for one parameter of the object.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// The name of the top level parameter.
    pub name: String,
    /// The new value. For a field of a record, the whole record. `null` puts the default back.
    pub value: Value,
}

impl Change {
    /// The `params` object for `plugins.set_params`.
    pub fn params(&self) -> Value {
        json!({ self.name.clone(): self.value.clone() })
    }
}

fn parse_field(v: &Value) -> Option<Field> {
    let name = v["name"].as_str()?.to_string();
    let kind = match v["type"].as_str()? {
        "number" => Kind::Number,
        "length" => Kind::Length,
        "text" => Kind::Text,
        "bool" => Kind::Bool,
        "choice" => Kind::Choice,
        "color" => Kind::Color,
        "record" => Kind::Record,
        _ => return None,
    };
    let label = v["label"]
        .as_str()
        .filter(|l| !l.is_empty())
        .unwrap_or(&name)
        .to_string();
    let strings = |key: &str| -> Vec<String> {
        v[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|o| o.as_str().map(str::to_string))
            .collect()
    };
    Some(Field {
        label,
        kind,
        min: v["min"].as_f64(),
        max: v["max"].as_f64(),
        unit: v["unit"].as_str().unwrap_or("").to_string(),
        options: strings("options"),
        fields: v["fields"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(parse_field)
            .collect(),
        default: v["default"].clone(),
        name,
    })
}

impl Field {
    /// The value for a field without a saved value.
    fn fallback(&self) -> Value {
        if !self.default.is_null() {
            return self.default.clone();
        }
        match self.kind {
            Kind::Number | Kind::Length => {
                json!(0.0_f64.clamp(self.min.unwrap_or(f64::MIN), self.max.unwrap_or(f64::MAX)))
            }
            Kind::Text => json!(""),
            Kind::Bool => json!(false),
            Kind::Choice => json!(self.options.first().cloned().unwrap_or_default()),
            Kind::Color => json!("#000000"),
            Kind::Record => Value::Object(
                self.fields
                    .iter()
                    .map(|f| (f.name.clone(), f.fallback()))
                    .collect(),
            ),
        }
    }

    fn limit_text(&self, x: f64) -> String {
        let u = kadakas_units::current();
        match self.kind {
            Kind::Length => u.length_text(x),
            _ => with_unit(u.number_text_trim(x, 6), &self.unit),
        }
    }

    /// The range as text for a hint, such as "50 mm to 500 mm". `None` without a limit.
    pub fn range_text(&self) -> Option<String> {
        match (self.min, self.max) {
            (Some(a), Some(b)) => Some(tr!(
                "uic-plugin-form-range",
                min = self.limit_text(a),
                max = self.limit_text(b)
            )),
            (Some(a), None) => Some(tr!("uic-plugin-form-below-min", limit = self.limit_text(a))),
            (None, Some(b)) => Some(tr!("uic-plugin-form-above-max", limit = self.limit_text(b))),
            (None, None) => None,
        }
    }

    /// The unit to show next to the field: the unit of the person for a length, the unit text of
    /// the plug-in for a number.
    pub fn unit_text(&self) -> String {
        match self.kind {
            Kind::Length => kadakas_units::current().length_field_unit().to_string(),
            Kind::Number => self.unit.clone(),
            _ => String::new(),
        }
    }

    /// Checks input text for a number or length field. Returns the value in the unit of the
    /// parameter (mm for a length).
    fn parse_number(&self, text: &str) -> Result<f64, Problem> {
        let u = kadakas_units::current();
        let text = text.trim();
        let x = if self.kind == Kind::Length {
            u.parse_length(text)
        } else {
            u.parse_number(text)
        };
        let x = x
            .filter(|x| x.is_finite() && x.abs() <= MAX_VALUE)
            .ok_or(Problem::NotANumber)?;
        match (self.min, self.max) {
            (Some(a), Some(b)) if x < a || x > b => {
                Err(Problem::OutOfRange(self.limit_text(a), self.limit_text(b)))
            }
            (Some(a), _) if x < a => Err(Problem::TooSmall(self.limit_text(a))),
            (_, Some(b)) if x > b => Err(Problem::TooLarge(self.limit_text(b))),
            _ => Ok(x),
        }
    }
}

fn with_unit(number: String, unit: &str) -> String {
    if unit.is_empty() {
        number
    } else {
        format!("{number} {unit}")
    }
}

fn normalize_color(s: &str) -> Option<String> {
    let h = s.trim().strip_prefix('#')?;
    ((h.len() == 6 || h.len() == 8) && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("#{}", h.to_ascii_lowercase()))
}

fn same_number(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0),
        _ => a == b,
    }
}

/// The state of the form of one plug-in object.
#[derive(Clone, Debug)]
pub struct Form {
    schema: Value,
    saved: Map<String, Value>,
    fields: Vec<Field>,
    drafts: HashMap<String, String>,
    problems: HashMap<String, Problem>,
}

impl Form {
    /// A form for a schema and the saved values. A field with a type that this version does not
    /// know is left out.
    pub fn new(schema: &Value, saved: &Value) -> Form {
        Form {
            schema: schema.clone(),
            saved: saved.as_object().cloned().unwrap_or_default(),
            fields: schema
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(parse_field)
                .collect(),
            drafts: HashMap::new(),
            problems: HashMap::new(),
        }
    }

    /// True if this form is for exactly this schema and these saved values. When they change (a
    /// change was applied, an undo, another object), the caller makes a new form.
    pub fn matches(&self, schema: &Value, saved: &Value) -> bool {
        self.schema == *schema && Some(&self.saved) == saved.as_object()
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    fn field_at(&self, path: &str) -> Option<&Field> {
        let mut parts = path.split('.');
        let first = parts.next()?;
        let mut field = self.fields.iter().find(|f| f.name == first)?;
        for part in parts {
            field = field.fields.iter().find(|f| f.name == part)?;
        }
        Some(field)
    }

    /// The saved value at a path such as `wall` or `box.width`, or the default.
    pub fn value_at(&self, path: &str) -> Value {
        let Some(field) = self.field_at(path) else {
            return Value::Null;
        };
        let mut parts = path.split('.');
        let mut v = parts.next().and_then(|n| self.saved.get(n));
        for part in parts {
            v = v.and_then(|r| r.get(part));
        }
        v.cloned().unwrap_or_else(|| field.fallback())
    }

    /// The text to show in a number, length, text or color field: what the person is typing, or
    /// the saved value.
    pub fn text_at(&self, path: &str) -> String {
        if let Some(d) = self.drafts.get(path) {
            return d.clone();
        }
        let Some(field) = self.field_at(path) else {
            return String::new();
        };
        let v = self.value_at(path);
        let u = kadakas_units::current();
        match field.kind {
            Kind::Length => u.length_field(v.as_f64().unwrap_or(0.0), u.length_decimals()),
            Kind::Number => u.number_text_trim(v.as_f64().unwrap_or(0.0), 6),
            _ => v.as_str().unwrap_or("").to_string(),
        }
    }

    /// The person types. Keeps the text; checks nothing yet.
    pub fn edit(&mut self, path: &str, text: &str) {
        self.drafts.insert(path.to_string(), text.to_string());
        self.problems.remove(path);
    }

    /// True while the field has text that is not applied.
    pub fn is_editing(&self, path: &str) -> bool {
        self.drafts.contains_key(path)
    }

    /// The person leaves the field or presses Enter: checks the text. A valid and new value is
    /// returned as a change. A refused text stays in the field with its problem, so the person
    /// can fix it.
    pub fn commit(&mut self, path: &str) -> Option<Change> {
        let text = self.drafts.get(path)?.clone();
        let field = self.field_at(path)?.clone();
        let value = match field.kind {
            Kind::Number | Kind::Length => field.parse_number(&text).map(|x| json!(x)),
            Kind::Text => {
                if text.chars().count() > MAX_TEXT {
                    Err(Problem::TooLong)
                } else {
                    Ok(json!(text))
                }
            }
            Kind::Color => normalize_color(&text)
                .map(Value::String)
                .ok_or(Problem::BadColor),
            Kind::Choice => Ok(json!(text)),
            Kind::Bool | Kind::Record => return None,
        };
        match value {
            Err(p) => {
                self.problems.insert(path.to_string(), p);
                None
            }
            Ok(v) => {
                self.drafts.remove(path);
                self.problems.remove(path);
                self.change_to(path, v)
            }
        }
    }

    /// The person gives up the text: back to the saved value.
    pub fn revert(&mut self, path: &str) {
        self.drafts.remove(path);
        self.problems.remove(path);
    }

    /// The problem of the last refused input of a field.
    pub fn problem(&self, path: &str) -> Option<&Problem> {
        self.problems.get(path)
    }

    pub fn set_bool(&mut self, path: &str, on: bool) -> Option<Change> {
        if self.field_at(path)?.kind != Kind::Bool {
            return None;
        }
        self.change_to(path, json!(on))
    }

    pub fn set_choice(&mut self, path: &str, option: &str) -> Option<Change> {
        let field = self.field_at(path)?;
        if field.kind != Kind::Choice {
            return None;
        }
        if !field.options.iter().any(|o| o == option) {
            self.problems.insert(path.to_string(), Problem::NotAnOption);
            return None;
        }
        self.problems.remove(path);
        self.change_to(path, json!(option))
    }

    /// Sets a color (`#rrggbb` or `#rrggbbaa`) from a color picker.
    pub fn set_color(&mut self, path: &str, hex: &str) -> Option<Change> {
        if self.field_at(path)?.kind != Kind::Color {
            return None;
        }
        match normalize_color(hex) {
            Some(c) => self.change_to(path, json!(c)),
            None => {
                self.problems.insert(path.to_string(), Problem::BadColor);
                None
            }
        }
    }

    /// Puts the default of a top level parameter back (`null` for `plugins.set_params`).
    pub fn reset(&mut self, name: &str) -> Option<Change> {
        self.field_at(name)?;
        self.drafts.retain(|k, _| !k.starts_with(name));
        self.problems.retain(|k, _| !k.starts_with(name));
        Some(Change {
            name: name.to_string(),
            value: Value::Null,
        })
    }

    /// The change that sets the value at `path`, or `None` if it is the saved value already.
    fn change_to(&self, path: &str, value: Value) -> Option<Change> {
        if same_number(&self.value_at(path), &value) {
            return None;
        }
        let mut parts = path.split('.');
        let name = parts.next()?.to_string();
        let rest: Vec<&str> = parts.collect();
        if rest.is_empty() {
            return Some(Change { name, value });
        }
        // A field of a record: the whole record goes back, with one field changed.
        let mut record = self
            .value_at(&name)
            .as_object()
            .cloned()
            .unwrap_or_default();
        record.insert(rest[0].to_string(), value);
        Some(Change {
            name,
            value: Value::Object(record),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_units::{Units, with_units};

    fn schema() -> Value {
        json!([
            {"name": "wall", "label": "Wall thickness", "type": "length", "default": 150, "min": 50, "max": 500},
            {"name": "count", "type": "number", "default": 3, "min": 1, "max": 20, "unit": "pcs"},
            {"name": "free", "type": "number"},
            {"name": "above", "type": "number", "max": 10},
            {"name": "below", "type": "number", "min": -1},
            {"name": "label", "type": "bool", "default": true},
            {"name": "corners", "type": "choice", "options": ["Square", "Cut"]},
            {"name": "note", "type": "text"},
            {"name": "color", "type": "color", "default": "#6b4f3a"},
            {"name": "box", "type": "record", "fields": [
                {"name": "w", "type": "length", "default": 100, "min": 10},
                {"name": "on", "type": "bool"}
            ]},
            {"name": "weird", "type": "hologram"}
        ])
    }

    fn form(saved: Value) -> Form {
        Form::new(&schema(), &saved)
    }

    #[test]
    fn fields_come_from_the_schema_and_unknown_types_are_left_out() {
        let f = form(json!({}));
        let names: Vec<_> = f.fields().iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names.len(), 10);
        assert!(!names.contains(&"weird"));
        assert_eq!(f.fields()[0].label, "Wall thickness");
        assert_eq!(f.fields()[1].label, "count");
        assert_eq!(f.fields()[1].unit, "pcs");
        assert_eq!(f.fields()[9].fields.len(), 2);
        assert!(Form::new(&json!("x"), &json!(null)).fields().is_empty());
    }

    #[test]
    fn values_are_the_saved_ones_or_the_defaults() {
        let f = form(json!({"wall": 200, "box": {"w": 40}}));
        assert_eq!(f.value_at("wall"), json!(200));
        assert_eq!(f.value_at("count"), json!(3));
        assert_eq!(f.value_at("free"), json!(0.0));
        assert_eq!(f.value_at("above"), json!(0.0));
        assert_eq!(f.value_at("below"), json!(0.0));
        assert_eq!(f.value_at("label"), json!(true));
        assert_eq!(f.value_at("corners"), json!("Square"));
        assert_eq!(f.value_at("note"), json!(""));
        assert_eq!(f.value_at("box.w"), json!(40));
        assert_eq!(f.value_at("box.on"), json!(false));
        assert_eq!(f.value_at("nothing"), Value::Null);
        let none = form(json!(null));
        assert_eq!(none.value_at("box"), json!({"w": 100, "on": false}));
        assert_eq!(none.value_at("color"), json!("#6b4f3a"));
    }

    #[test]
    fn a_valid_number_is_one_change_and_the_same_value_is_none() {
        with_units(Units::METRIC, || {
            let mut f = form(json!({"count": 3}));
            assert_eq!(f.text_at("count"), "3");
            f.edit("count", "7");
            assert!(f.is_editing("count"));
            assert_eq!(f.text_at("count"), "7");
            let c = f.commit("count").unwrap();
            assert_eq!(c.name, "count");
            assert_eq!(c.value, json!(7.0));
            assert_eq!(c.params(), json!({"count": 7.0}));
            assert!(!f.is_editing("count"));
            // The same value again is no change.
            f.edit("count", "3,0");
            let again = f.commit("count");
            assert!(again.is_none() && f.problem("count").is_none());
            // Nothing to commit.
            assert!(f.commit("count").is_none());
        });
    }

    #[test]
    fn bad_numbers_are_refused_with_a_problem_and_the_text_stays() {
        with_units(Units::METRIC, || {
            let mut f = form(json!({}));
            let mut try_text = |path: &str, text: &str| {
                f.edit(path, text);
                assert!(f.commit(path).is_none(), "{text}");
                assert!(f.is_editing(path));
                f.problem(path).cloned().unwrap()
            };
            assert_eq!(try_text("count", "abc"), Problem::NotANumber);
            assert_eq!(try_text("count", ""), Problem::NotANumber);
            assert_eq!(try_text("count", "1e12"), Problem::NotANumber);
            assert_eq!(try_text("count", "NaN"), Problem::NotANumber);
            assert!(
                matches!(try_text("count", "0"), Problem::OutOfRange(a, b) if a.contains("pcs") && b.contains("20"))
            );
            assert!(matches!(try_text("count", "21"), Problem::OutOfRange(..)));
            assert!(matches!(try_text("above", "11"), Problem::TooLarge(_)));
            assert!(matches!(try_text("below", "-2"), Problem::TooSmall(_)));
            assert!(matches!(try_text("wall", "10"), Problem::OutOfRange(..)));
            for p in [
                Problem::NotANumber,
                Problem::TooSmall("1".into()),
                Problem::TooLarge("2".into()),
                Problem::OutOfRange("1".into(), "2".into()),
                Problem::TooLong,
                Problem::BadColor,
                Problem::NotAnOption,
            ] {
                assert!(!p.text().is_empty());
            }
            // Typing again clears the problem. Reverting drops the text.
            f.edit("count", "5");
            assert!(f.problem("count").is_none());
            f.revert("count");
            assert!(!f.is_editing("count"));
        });
    }

    #[test]
    fn lengths_use_the_unit_of_the_person() {
        use kadakas_units::LengthUnit;
        let m = Units {
            length: LengthUnit::M,
            ..Units::METRIC
        };
        with_units(m, || {
            let mut f = form(json!({"wall": 150}));
            assert_eq!(f.fields()[0].unit_text(), "m");
            assert_eq!(f.text_at("wall"), "0.150");
            f.edit("wall", "0.3");
            assert_eq!(f.commit("wall").unwrap().value, json!(300.0));
            f.edit("wall", "0.01");
            assert!(f.commit("wall").is_none());
            assert!(f.problem("wall").is_some());
            let range = f.fields()[0].range_text().unwrap();
            assert!(range.contains("0.05") && range.contains("0.5"), "{range}");
        });
        with_units(Units::METRIC, || {
            let f = form(json!({}));
            assert_eq!(f.fields()[0].unit_text(), "mm");
            assert_eq!(f.fields()[1].unit_text(), "pcs");
            assert_eq!(f.fields()[5].unit_text(), "");
            assert!(f.fields()[1].range_text().unwrap().contains("pcs"));
            assert!(f.fields()[2].range_text().is_none());
            assert!(f.fields()[3].range_text().unwrap().contains("10"));
            assert!(f.fields()[4].range_text().unwrap().contains("-1"));
        });
    }

    #[test]
    fn bool_choice_and_color_give_changes() {
        let mut f = form(json!({"label": true, "corners": "Square", "color": "#112233"}));
        assert_eq!(f.set_bool("label", false).unwrap().value, json!(false));
        assert!(f.set_bool("label", true).is_none());
        assert!(f.set_bool("count", true).is_none());
        assert!(f.set_bool("nothing", true).is_none());
        assert_eq!(f.set_choice("corners", "Cut").unwrap().value, json!("Cut"));
        assert!(f.set_choice("corners", "Square").is_none());
        assert!(f.set_choice("corners", "Round").is_none());
        assert_eq!(f.problem("corners"), Some(&Problem::NotAnOption));
        assert!(f.set_choice("label", "x").is_none());
        assert_eq!(
            f.set_color("color", "#AABBCC").unwrap().value,
            json!("#aabbcc")
        );
        assert!(f.set_color("color", "#112233").is_none());
        assert!(f.set_color("color", "red").is_none());
        assert_eq!(f.problem("color"), Some(&Problem::BadColor));
        assert!(f.set_color("label", "#000000").is_none());
        // Typed colors and texts.
        f.edit("color", "#FFF");
        assert!(f.commit("color").is_none());
        assert_eq!(f.problem("color"), Some(&Problem::BadColor));
        f.edit("color", "#ABCDEF80");
        assert_eq!(f.commit("color").unwrap().value, json!("#abcdef80"));
        f.edit("note", "hello");
        assert_eq!(f.commit("note").unwrap().value, json!("hello"));
        f.edit("note", &"x".repeat(MAX_TEXT + 1));
        assert!(f.commit("note").is_none());
        assert_eq!(f.problem("note"), Some(&Problem::TooLong));
        assert_eq!(f.text_at("note"), "x".repeat(MAX_TEXT + 1));
        // A bool has no text to commit.
        f.edit("label", "x");
        assert!(f.commit("label").is_none());
        assert!(f.commit("nothing").is_none());
    }

    #[test]
    fn a_choice_typed_as_text_is_taken_as_it_is() {
        let mut f = form(json!({}));
        f.edit("corners", "Cut");
        assert_eq!(f.commit("corners").unwrap().value, json!("Cut"));
    }

    #[test]
    fn a_field_of_a_record_changes_the_whole_record() {
        with_units(Units::METRIC, || {
            let mut f = form(json!({"box": {"w": 40, "on": false}}));
            f.edit("box.w", "60");
            let c = f.commit("box.w").unwrap();
            assert_eq!(c.name, "box");
            assert_eq!(c.value, json!({"w": 60.0, "on": false}));
            let c = f.set_bool("box.on", true).unwrap();
            assert_eq!(c.value, json!({"w": 40, "on": true}));
            f.edit("box.w", "5");
            assert!(f.commit("box.w").is_none());
            assert!(f.problem("box.w").is_some());
            // The record without saved values uses the defaults.
            let mut g = form(json!({}));
            let c = g.set_bool("box.on", true).unwrap();
            assert_eq!(c.value, json!({"w": 100, "on": true}));
        });
    }

    #[test]
    fn reset_puts_the_default_back_and_drops_the_text() {
        let mut f = form(json!({"wall": 200}));
        f.edit("wall", "abc");
        f.commit("wall");
        let c = f.reset("wall").unwrap();
        assert_eq!(c.value, Value::Null);
        assert_eq!(c.params(), json!({"wall": null}));
        assert!(!f.is_editing("wall") && f.problem("wall").is_none());
        assert!(f.reset("nothing").is_none());
    }

    #[test]
    fn the_form_knows_when_it_is_out_of_date() {
        let f = form(json!({"wall": 200}));
        assert!(f.matches(&schema(), &json!({"wall": 200})));
        assert!(!f.matches(&schema(), &json!({"wall": 201})));
        assert!(!f.matches(&json!([]), &json!({"wall": 200})));
        assert!(!f.matches(&schema(), &json!(null)));
    }

    #[test]
    fn a_saved_value_outside_the_range_is_shown_and_is_not_an_edit() {
        with_units(Units::METRIC, || {
            let mut f = form(json!({"count": 99}));
            assert_eq!(f.text_at("count"), "99");
            f.edit("count", "99");
            assert!(f.commit("count").is_none());
            assert!(f.problem("count").is_some());
        });
    }
}
