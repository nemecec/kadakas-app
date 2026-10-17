//! What changed in one object: the fields that differ between its state before and after a step,
//! for the summary of a step that changed one object ("Hardscape 'Pavers': zone 'Aisle' angle
//! 0° → 45°").
//!
//! The comparison reads the objects as JSON, so it needs no list of the fields of each kind. The
//! path of a field is turned into words: the zones and courses of a hardscape plan are named by
//! their own name, a shape field by its name, other data by its key. The values are short: a
//! number, a text, or a short point. A long list (an outline) is only said to be changed.

use serde_json::Value;

use crate::model::{Object, Shape};

/// Most changes that a summary names; the rest is counted.
pub const MAX_CHANGES: usize = 2;

/// The part of an object that a change belongs to, when it is not the object itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    /// `zone`, `course` or `split`.
    pub kind: &'static str,
    /// Its name, if it has one.
    pub name: Option<String>,
}

/// One field that differs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldChange {
    pub scope: Option<Scope>,
    /// The path of the field below its scope, as keys joined by dots (`angle`, `pattern.gap`).
    pub field: String,
    /// The value before and after, as short text. `None` for a value that is too long to show or
    /// that did not exist.
    pub before: Option<String>,
    pub after: Option<String>,
}

/// What changed in one object.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Detail {
    /// The name that tells the object apart from others of its kind, if it has one (the
    /// material of a hardscape).
    pub label: Option<String>,
    /// The first changes (at most [`MAX_CHANGES`]).
    pub changes: Vec<FieldChange>,
    /// How many more fields changed.
    pub more: usize,
}

/// The name of an object, as far as the object holds one.
pub fn label_of(o: &Object) -> Option<String> {
    match &o.shape {
        Shape::Hardscape { material, .. } if !material.trim().is_empty() => {
            Some(material.trim().to_string())
        }
        _ => None,
    }
}

/// The text of a number: up to three decimals, no trailing zeros.
pub fn number_text(v: f64) -> String {
    let t = format!("{v:.3}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t.is_empty() || t == "-" || t == "-0" {
        "0".into()
    } else {
        t.to_string()
    }
}

#[derive(Clone, Debug)]
enum Seg {
    Key(String),
    Index(usize),
}

/// The fields that differ between two states of one object. `None` if nothing differs that can be
/// named.
pub fn of(before: &Object, after: &Object) -> Option<Detail> {
    let (b, a) = (
        serde_json::to_value(before).ok()?,
        serde_json::to_value(after).ok()?,
    );
    let mut found: Vec<(Vec<Seg>, Option<&Value>, Option<&Value>)> = Vec::new();
    walk(&mut Vec::new(), Some(&b), Some(&a), &mut found);
    let mut changes: Vec<FieldChange> = found
        .into_iter()
        .filter_map(|(path, vb, va)| field_change(&path, vb, va, &a))
        .collect();
    // The same field twice (a path of two keys that both changed) is one line.
    changes.dedup();
    if changes.is_empty() {
        return None;
    }
    let more = changes.len().saturating_sub(MAX_CHANGES);
    changes.truncate(MAX_CHANGES);
    Some(Detail {
        label: label_of(after),
        changes,
        more,
    })
}

fn walk<'a>(
    path: &mut Vec<Seg>,
    b: Option<&'a Value>,
    a: Option<&'a Value>,
    out: &mut Vec<(Vec<Seg>, Option<&'a Value>, Option<&'a Value>)>,
) {
    if b == a {
        return;
    }
    match (b, a) {
        (Some(Value::Object(mb)), Some(Value::Object(ma))) => {
            let mut keys: Vec<&String> = mb.keys().chain(ma.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                path.push(Seg::Key(k.clone()));
                walk(path, mb.get(k), ma.get(k), out);
                path.pop();
            }
        }
        (Some(Value::Array(lb)), Some(Value::Array(la)))
            if lb.len() == la.len() && !is_short_numbers(lb) =>
        {
            for i in 0..lb.len() {
                path.push(Seg::Index(i));
                walk(path, lb.get(i), la.get(i), out);
                path.pop();
            }
        }
        _ => out.push((path.clone(), b, a)),
    }
}

/// A point or a pair of numbers: shown as one value.
fn is_short_numbers(l: &[Value]) -> bool {
    l.len() <= 3 && l.iter().all(Value::is_number)
}

fn key_of(s: &Seg) -> Option<&str> {
    match s {
        Seg::Key(k) => Some(k),
        Seg::Index(_) => None,
    }
}

/// True for a field that holds an angle in degrees.
fn degrees(path: &[&str]) -> bool {
    let last = path.last().copied().unwrap_or("");
    last == "angle" || last.ends_with("_deg") || last == "degrees"
}

/// True for a field of a shape that holds an angle in radians.
fn radians(path: &[&str]) -> bool {
    matches!(path.last().copied(), Some("rotation"))
}

fn show(v: Option<&Value>, deg: bool, rad: bool) -> Option<String> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::Number(n)) => {
            let x = n.as_f64()?;
            Some(if deg {
                format!("{}°", number_text(x))
            } else if rad {
                format!("{}°", number_text(x.to_degrees()))
            } else {
                number_text(x)
            })
        }
        Some(Value::String(s)) if s.chars().count() <= 40 => Some(s.clone()),
        Some(Value::Bool(b)) => Some(b.to_string()),
        Some(Value::Array(l)) if is_short_numbers(l) => Some(format!(
            "({})",
            l.iter()
                .filter_map(|x| x.as_f64().map(number_text))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        _ => None,
    }
}

fn field_change(
    path: &[Seg],
    b: Option<&Value>,
    a: Option<&Value>,
    after: &Value,
) -> Option<FieldChange> {
    let first = key_of(path.first()?)?;
    // Skip what is not a field of the content.
    if matches!(first, "id" | "extra") {
        return None;
    }
    let mut scope = None;
    let mut keys: Vec<&str> = Vec::new();
    match first {
        "shape" => keys.extend(path[1..].iter().filter_map(key_of)),
        "data" => {
            let rest = &path[1..];
            let key = key_of(rest.first()?)?;
            if key == crate::layout::plan::KEY {
                // The plan of a hardscape: zones, courses and split lines by their own names.
                let section = rest.get(1).and_then(key_of);
                let index = match rest.get(2) {
                    Some(Seg::Index(i)) => Some(*i),
                    _ => None,
                };
                let kind = match section {
                    Some("zones") => Some("zone"),
                    Some("courses") => Some("course"),
                    Some("splits") => Some("split"),
                    _ => None,
                };
                if let (Some(kind), Some(i), Some(section)) = (kind, index, section) {
                    let name = after
                        .get("data")
                        .and_then(|d| d.get(key))
                        .and_then(|p| p.get(section))
                        .and_then(|l| l.get(i))
                        .and_then(|z| z.get("name"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    scope = Some(Scope {
                        kind,
                        name: name.or_else(|| Some(format!("{}", i + 1))),
                    });
                    keys.extend(rest[3..].iter().filter_map(key_of));
                } else {
                    keys.extend(rest[1..].iter().filter_map(key_of));
                    if keys.is_empty() {
                        keys.push("plan");
                    }
                }
            } else if key == "hardscape" {
                // The own data of a hardscape (origin, angle, holes) reads as its fields.
                keys.extend(rest[1..].iter().filter_map(key_of));
            } else {
                keys.extend(rest.iter().filter_map(key_of));
            }
        }
        _ => keys.extend(path.iter().filter_map(key_of)),
    }
    if keys.is_empty() {
        // A list of zones that changed in length, or a whole value: name the nearest key.
        keys.push(first);
    }
    let in_shape = first == "shape";
    let deg = degrees(&keys);
    let rad = in_shape && radians(&keys);
    Some(FieldChange {
        scope,
        field: keys.join("."),
        before: show(b, deg, rad),
        after: show(a, deg, rad),
    })
}

impl Detail {
    /// The English text of the changes: `zone 'Aisle' angle 0° → 45°; …`.
    pub fn text(&self) -> String {
        let mut parts: Vec<String> = self.changes.iter().map(FieldChange::text).collect();
        if self.more > 0 {
            parts.push(format!("{} more", self.more));
        }
        parts.join("; ")
    }
}

impl FieldChange {
    pub fn text(&self) -> String {
        let field = self.field.replace('_', " ");
        let scope = match &self.scope {
            None => String::new(),
            Some(Scope { kind, name: Some(n) }) => format!("{kind} '{n}' "),
            Some(Scope { kind, name: None }) => format!("{kind} "),
        };
        match (&self.before, &self.after) {
            (Some(b), Some(a)) => format!("{scope}{field} {b} → {a}"),
            (None, Some(a)) => format!("{scope}{field} set to {a}"),
            (Some(b), None) => format!("{scope}{field} cleared (was {b})"),
            (None, None) => format!("{scope}{field} changed"),
        }
    }
}
