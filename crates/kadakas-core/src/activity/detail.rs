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
use crate::resource::Resource;

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

/// What changed in one resource (a style, a material): its kind and name, and the first fields.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceDetail {
    /// The `type` of the resource (`hardscape_style`, `plant_style`, ...).
    pub kind: String,
    pub name: String,
    /// The first changes (at most [`MAX_CHANGES`]); a length has its unit in the value.
    pub changes: Vec<FieldChange>,
    pub more: usize,
}

/// A length in mm, by the name of the field.
fn is_length(key: &str) -> bool {
    matches!(
        key,
        "spacing"
            | "unit_length"
            | "unit_width"
            | "thickness"
            | "width"
            | "gap"
            | "offset"
            | "elevation"
            | "radius"
            | "height"
            | "depth"
            | "spread"
    )
}

/// The fields that differ between two states of one resource. `None` if nothing differs that can
/// be named.
pub fn of_resource(before: &Resource, after: &Resource) -> Option<ResourceDetail> {
    let (b, a) = (
        serde_json::to_value(before).ok()?,
        serde_json::to_value(after).ok()?,
    );
    let mut found: Vec<(Vec<Seg>, Option<&Value>, Option<&Value>)> = Vec::new();
    walk(&mut Vec::new(), Some(&b), Some(&a), &mut found);
    let mut changes: Vec<FieldChange> = Vec::new();
    for (path, vb, va) in found {
        let mut keys: Vec<&str> = path.iter().filter_map(key_of).collect();
        match keys.first().copied() {
            Some("id") => continue,
            Some("body") => {
                keys.remove(0);
                if keys.first() == Some(&"type") {
                    continue;
                }
            }
            _ => {}
        }
        // The joint pattern of a style: its fields read as the fields of the style, and the
        // width of the joint is "joint".
        if keys.first() == Some(&"joints") && keys.len() == 2 {
            keys.remove(0);
            if keys[0] == "spacing" {
                keys[0] = "joint";
            }
        }
        if keys.is_empty() {
            keys.push("resource");
        }
        let deg = degrees(&keys);
        let unit = keys.last().is_some_and(|k| is_length(k) || *k == "joint");
        let (sb, sa) = (show(vb, deg, false), show(va, deg, false));
        let sa = sa.map(|t| if unit { format!("{t} mm") } else { t });
        changes.push(FieldChange {
            scope: None,
            field: keys.join("."),
            before: sb,
            after: sa,
        });
    }
    changes.dedup();
    if changes.is_empty() {
        return None;
    }
    let more = changes.len().saturating_sub(MAX_CHANGES);
    changes.truncate(MAX_CHANGES);
    let kind = a
        .get("body")
        .and_then(|b| b.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("resource")
        .to_string();
    Some(ResourceDetail {
        kind,
        name: after.name.clone(),
        changes,
        more,
    })
}

impl ResourceDetail {
    /// The English text: `hardscape style 'Pavers': joint 3 → 5 mm`.
    pub fn text(&self) -> String {
        let mut parts: Vec<String> = self.changes.iter().map(FieldChange::text).collect();
        if self.more > 0 {
            parts.push(format!("{} more", self.more));
        }
        format!(
            "{} '{}': {}",
            self.kind.replace('_', " "),
            self.name,
            parts.join("; ")
        )
    }
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
            if rest.is_empty() {
                // The whole data map was emptied (or filled): the keys say what went.
                let names = |v: Option<&Value>| match v {
                    Some(Value::Object(m)) if !m.is_empty() => {
                        Some(m.keys().cloned().collect::<Vec<_>>().join(", "))
                    }
                    _ => None,
                };
                return Some(FieldChange {
                    scope: None,
                    field: "data".to_string(),
                    before: names(b),
                    after: names(a),
                });
            }
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
            Some(Scope {
                kind,
                name: Some(n),
            }) => format!("{kind} '{n}' "),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FIRST_LAYER, NONE_CLASS, ObjectId};
    use crate::resource::ResourceId;
    use glam::DVec2;
    use serde_json::json;

    fn obj(shape: Shape) -> Object {
        Object::new(ObjectId::from_num(1), FIRST_LAYER, NONE_CLASS, shape)
    }

    fn circle(r: f64) -> Object {
        obj(Shape::Circle {
            center: DVec2::ZERO,
            radius: r,
        })
    }

    fn pavers(material: &str) -> Object {
        obj(Shape::Hardscape {
            boundary: Vec::new(),
            material: material.into(),
            thickness: None,
            elevation: None,
        })
    }

    fn style(name: &str, spacing: f64, thickness: f64) -> Resource {
        use crate::hardscape::style::{HardscapeStyle, JointKind, JointPattern};
        use crate::resource::ResourceBody;
        Resource {
            id: ResourceId::from_num(1),
            name: name.into(),
            body: ResourceBody::HardscapeStyle(Box::new(HardscapeStyle {
                thickness,
                joints: Some(JointPattern {
                    kind: JointKind::RunningBond,
                    unit_length: 200.0,
                    unit_width: 100.0,
                    spacing,
                    angle: 0.0,
                    color: None,
                    units: Vec::new(),
                    a: None,
                    b: None,
                    generator: None,
                }),
                ..Default::default()
            })),
        }
    }

    #[test]
    fn a_changed_style_names_the_field_and_has_the_unit() {
        let d = of_resource(&style("Pavers", 3.0, 80.0), &style("Pavers", 5.0, 80.0)).unwrap();
        assert_eq!(d.kind, "hardscape_style");
        assert_eq!(d.name, "Pavers");
        assert_eq!(d.text(), "hardscape style 'Pavers': joint 3 → 5 mm");
        // Two fields, then the rest is counted (MAX_CHANGES is 2).
        let mut after = style("Pavers", 5.0, 100.0);
        if let crate::resource::ResourceBody::HardscapeStyle(s) = &mut after.body {
            s.joints.as_mut().unwrap().unit_length = 300.0;
        }
        let d = of_resource(&style("Pavers", 3.0, 80.0), &after).unwrap();
        assert_eq!(d.changes.len(), MAX_CHANGES);
        assert_eq!(d.more, 1);
        assert!(d.text().ends_with("; 1 more"), "{}", d.text());
        // A rename is a change of the name; nothing changed gives none.
        let d = of_resource(&style("A", 3.0, 80.0), &style("B", 3.0, 80.0)).unwrap();
        assert_eq!(d.changes[0].field, "name");
        assert!(of_resource(&style("A", 3.0, 80.0), &style("A", 3.0, 80.0)).is_none());
        // A fillet or a color that was not there before is "set", a removed one "cleared".
        let mut with_border = style("A", 3.0, 80.0);
        if let crate::resource::ResourceBody::HardscapeStyle(s) = &mut with_border.body {
            s.joints.as_mut().unwrap().angle = 45.0;
        }
        let d = of_resource(&style("A", 3.0, 80.0), &with_border).unwrap();
        assert_eq!(d.changes[0].text(), "angle 0° → 45°");
    }

    #[test]
    fn numbers_have_up_to_three_decimals_and_no_zeros() {
        assert_eq!(number_text(100.0), "100");
        assert_eq!(number_text(1.5), "1.5");
        assert_eq!(number_text(1.23456), "1.235");
        assert_eq!(number_text(-0.0004), "0");
        assert_eq!(number_text(0.0), "0");
        assert_eq!(number_text(-2.25), "-2.25");
    }

    #[test]
    fn a_changed_shape_field_is_named_with_both_values() {
        let d = of(&circle(100.0), &circle(150.5)).unwrap();
        assert_eq!(d.changes.len(), 1);
        assert_eq!(d.more, 0);
        assert_eq!(d.label, None);
        assert_eq!(d.text(), "radius 100 → 150.5");
    }

    #[test]
    fn nothing_that_can_be_named_gives_none() {
        assert!(of(&circle(1.0), &circle(1.0)).is_none());
        let mut other = circle(1.0);
        other.id = ObjectId::from_num(2);
        assert!(of(&circle(1.0), &other).is_none());
    }

    #[test]
    fn a_hardscape_is_labelled_by_its_material() {
        assert_eq!(label_of(&pavers("  Granite ")), Some("Granite".into()));
        assert_eq!(label_of(&pavers("  ")), None);
        assert_eq!(label_of(&circle(1.0)), None);
        let d = of(&pavers("Granite"), &{
            let mut o = pavers("Granite");
            if let Shape::Hardscape { thickness, .. } = &mut o.shape {
                *thickness = Some(80.0);
            }
            o
        })
        .unwrap();
        assert_eq!(d.label.as_deref(), Some("Granite"));
        assert_eq!(d.text(), "thickness set to 80");
    }

    #[test]
    fn a_rotation_of_a_shape_reads_in_degrees() {
        let sym = |rotation: f64| {
            obj(Shape::Symbol {
                symbol: ResourceId::from_num(1),
                at: DVec2::ZERO,
                rotation,
                scale: 1.0,
                scale_y: None,
            })
        };
        let d = of(&sym(0.0), &sym(std::f64::consts::FRAC_PI_2)).unwrap();
        assert_eq!(d.text(), "rotation 0° → 90°");
    }

    #[test]
    fn a_short_list_of_numbers_is_one_point_and_a_long_one_is_changed() {
        let moved = |x: f64| {
            obj(Shape::Symbol {
                symbol: ResourceId::from_num(1),
                at: DVec2::new(x, 2.0),
                rotation: 0.0,
                scale: 1.0,
                scale_y: None,
            })
        };
        let d = of(&moved(1.0), &moved(5.0)).unwrap();
        assert_eq!(d.text(), "at (1, 2) → (5, 2)");
        let mut a = pavers("x");
        let mut b = pavers("x");
        a.data.insert("list".into(), json!([1, 2, 3, 4, 5]));
        b.data.insert("list".into(), json!([1, 2, 3, 4, 5, 6]));
        assert_eq!(of(&a, &b).unwrap().text(), "list changed");
    }

    #[test]
    fn an_emptied_data_map_is_a_change_that_names_the_removed_keys() {
        let mut a = pavers("x");
        let b = pavers("x");
        a.data.insert("note".into(), json!("short"));
        a.data.insert("flag".into(), json!(true));
        assert_eq!(of(&a, &b).unwrap().text(), "data cleared (was flag, note)");
        assert_eq!(of(&b, &a).unwrap().text(), "data set to flag, note");
    }

    #[test]
    fn plain_data_is_named_by_its_key_and_values_by_type() {
        let mut a = pavers("x");
        let mut b = pavers("x");
        a.data.insert("note".into(), json!("short"));
        b.data.insert("note".into(), json!("longer"));
        assert_eq!(of(&a, &b).unwrap().text(), "note short → longer");
        a.data.insert("flag".into(), json!(false));
        b.data.insert("flag".into(), json!(true));
        a.data.remove("note");
        b.data.remove("note");
        assert_eq!(of(&a, &b).unwrap().text(), "flag false → true");
        // The whole `data` is skipped when it is empty, so keep one key that does not change.
        a.data.insert("keep".into(), json!(1));
        b.data.insert("keep".into(), json!(1));
        let long = "x".repeat(60);
        a.data.insert("flag".into(), json!(long));
        b.data.remove("flag");
        assert_eq!(of(&a, &b).unwrap().text(), "flag changed");
        a.data.insert("flag".into(), json!("was"));
        assert_eq!(of(&a, &b).unwrap().text(), "flag cleared (was was)");
    }

    #[test]
    fn the_own_data_of_a_hardscape_reads_as_its_fields() {
        let mut a = pavers("x");
        let mut b = pavers("x");
        a.data.insert("hardscape".into(), json!({"angle": 0}));
        b.data.insert("hardscape".into(), json!({"angle": 30}));
        assert_eq!(of(&a, &b).unwrap().text(), "angle 0° → 30°");
        let mut c = pavers("x");
        c.data
            .insert("hardscape".into(), json!({"spacing_deg": 1.5}));
        assert_eq!(of(&b, &c).unwrap().changes.len(), 2);
    }

    #[test]
    fn plan_parts_are_named_by_their_own_name() {
        let plan = |angle: f64, name: Option<&str>| {
            let mut o = pavers("x");
            let mut zone = json!({ "angle": angle });
            if let Some(n) = name {
                zone["name"] = json!(n);
            }
            o.data
                .insert(crate::layout::plan::KEY.into(), json!({ "zones": [zone] }));
            o
        };
        let d = of(&plan(0.0, Some("Aisle")), &plan(45.0, Some("Aisle"))).unwrap();
        assert_eq!(d.text(), "zone 'Aisle' angle 0° → 45°");
        let d = of(&plan(0.0, None), &plan(45.0, None)).unwrap();
        assert_eq!(d.text(), "zone '1' angle 0° → 45°");
        let nameless = FieldChange {
            scope: Some(Scope {
                kind: "split",
                name: None,
            }),
            field: "gap_size".into(),
            before: None,
            after: None,
        };
        assert_eq!(nameless.text(), "split gap size changed");
    }

    #[test]
    fn plan_courses_splits_and_the_whole_plan() {
        let with = |v: Value| {
            let mut o = pavers("x");
            o.data.insert(crate::layout::plan::KEY.into(), v);
            o
        };
        let d = of(
            &with(json!({"courses": [{"name": "Edge", "width": 100}]})),
            &with(json!({"courses": [{"name": "Edge", "width": 120}]})),
        )
        .unwrap();
        assert_eq!(d.text(), "course 'Edge' width 100 → 120");
        let d = of(
            &with(json!({"splits": [{"at": 1}]})),
            &with(json!({"splits": [{"at": 2}]})),
        )
        .unwrap();
        assert_eq!(d.text(), "split '1' at 1 → 2");
        // A zone added: the list changed in length.
        let d = of(
            &with(json!({"zones": [{"name": "A"}]})),
            &with(json!({"zones": [{"name": "A"}, {"name": "B"}]})),
        )
        .unwrap();
        assert_eq!(d.text(), "zones changed");
        // The plan itself appears.
        let mut base = pavers("x");
        base.data.insert("keep".into(), json!(1));
        let mut planned = with(json!({"zones": []}));
        planned.data.insert("keep".into(), json!(1));
        let d = of(&base, &planned).unwrap();
        assert_eq!(d.text(), "plan changed");
        let d = of(&with(json!(1)), &with(json!(2))).unwrap();
        assert_eq!(d.text(), "plan 1 → 2");
    }

    #[test]
    fn only_the_first_changes_are_named_and_the_rest_counted() {
        let mut a = pavers("x");
        let mut b = pavers("x");
        for (k, x, y) in [("a", 1, 2), ("b", 1, 2), ("c", 1, 2), ("d", 1, 2)] {
            a.data.insert(k.into(), json!(x));
            b.data.insert(k.into(), json!(y));
        }
        let d = of(&a, &b).unwrap();
        assert_eq!(d.changes.len(), MAX_CHANGES);
        assert_eq!(d.more, 2);
        assert_eq!(d.text(), "a 1 → 2; b 1 → 2; 2 more");
    }

    #[test]
    fn a_field_change_text_covers_all_value_states() {
        let c = |before: Option<&str>, after: Option<&str>| FieldChange {
            scope: Some(Scope {
                kind: "zone",
                name: Some("A".into()),
            }),
            field: "gap".into(),
            before: before.map(str::to_string),
            after: after.map(str::to_string),
        };
        assert_eq!(c(None, Some("3")).text(), "zone 'A' gap set to 3");
        assert_eq!(c(Some("3"), None).text(), "zone 'A' gap cleared (was 3)");
        assert_eq!(c(None, None).text(), "zone 'A' gap changed");
    }
}
