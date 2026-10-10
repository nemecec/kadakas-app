//! "Create massing from shapes": turns closed shapes into massing models in one batch.
//!
//! The shapes are usually building footprints from a WFS or a GIS file. The height comes from a data
//! field of the shape (the properties of a feature are kept in the object data), or a number of floors
//! from another field. The result is one `Command::Batch`, so it is one undo step.

use std::collections::BTreeMap;

use glam::DVec2;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::command::Command;
use crate::curve::{self, Vertex};
use crate::massing::{DEFAULT_FLOOR_HEIGHT, Massing, RoofStyle};
use crate::model::{Attributes, Color, Document, Object, ObjectId, Shape};

/// Names that data fields of buildings often have, tried when no field is named.
const HEIGHT_FIELDS: [&str; 8] = [
    "height",
    "building_height",
    "building:height",
    "measured_height",
    "measuredheight",
    "roof_height",
    "hoehe",
    "korkeus",
];
const FLOORS_FIELDS: [&str; 9] = [
    "floors",
    "levels",
    "building:levels",
    "storeys",
    "stories",
    "number_of_floors",
    "numberoffloors",
    "kerrosluku",
    "floor_count",
];

/// Default fill of a massing model in plans.
pub const PLAN_FILL: Color = Color([226, 221, 212, 255]);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FromShapesOptions {
    /// Data field with the height. Empty: the common names are tried.
    pub height_field: Option<String>,
    /// Data field with the number of floors. Empty: the common names are tried.
    pub floors_field: Option<String>,
    /// mm per unit of the height field. Default 1000: the field is in meters.
    pub height_scale: f64,
    /// Data field with the name.
    pub name_field: Option<String>,
    /// Eave height in mm when the shape has no height and no floors.
    pub default_height: f64,
    /// Height of a floor in mm when floors are used.
    pub floor_height: f64,
    pub roof: RoofStyle,
    pub pitch_deg: f64,
    /// Leave out shapes that have no height and no floors, instead of using `default_height`.
    pub require_height: bool,
    /// Remove the source shapes after the conversion (default). Without it the shapes stay.
    pub delete_source: bool,
}

impl Default for FromShapesOptions {
    fn default() -> Self {
        FromShapesOptions {
            height_field: None,
            floors_field: None,
            height_scale: 1000.0,
            name_field: None,
            default_height: 3000.0,
            floor_height: DEFAULT_FLOOR_HEIGHT,
            roof: RoofStyle::Flat,
            pitch_deg: 30.0,
            require_height: false,
            delete_source: true,
        }
    }
}

impl FromShapesOptions {
    pub fn check(&self) -> Result<(), String> {
        let ok = |v: f64| v.is_finite() && v > 0.0;
        if !(ok(self.height_scale) && ok(self.default_height) && ok(self.floor_height)) {
            return Err(
                "height_scale, default_height and floor_height must be positive numbers".into(),
            );
        }
        if !(self.pitch_deg.is_finite()
            && (0.0..=crate::massing::MAX_PITCH_DEG).contains(&self.pitch_deg))
        {
            return Err("pitch must be between 0 and 80 degrees".into());
        }
        Ok(())
    }
}

/// What the batch does.
#[derive(Clone, Debug, PartialEq)]
pub struct Made {
    /// The batch to run. `None` if no shape could be converted.
    pub command: Option<Command>,
    /// (source object, new massing object, how the height was found).
    pub created: Vec<(ObjectId, ObjectId, String)>,
    /// (source object, reason).
    pub skipped: Vec<(ObjectId, String)>,
}

/// A number from a data value: a number, or text that starts with one ("12.5 m").
pub fn number_of(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            let t = s.trim().replace(',', ".");
            let end = t
                .char_indices()
                .find(|(i, c)| {
                    !(c.is_ascii_digit() || *c == '.' || (*i == 0 && (*c == '-' || *c == '+')))
                })
                .map_or(t.len(), |(i, _)| i);
            t[..end].parse::<f64>().ok()
        }
        _ => None,
    }
    .filter(|x| x.is_finite())
}

fn field<'a>(
    data: &'a BTreeMap<String, Value>,
    name: Option<&str>,
    candidates: &[&str],
) -> Option<(String, &'a Value)> {
    if let Some(n) = name {
        return data
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(n))
            .map(|(k, v)| (k.clone(), v));
    }
    candidates.iter().find_map(|c| {
        data.iter()
            .find(|(k, v)| k.eq_ignore_ascii_case(c) && number_of(v).is_some_and(|x| x > 0.0))
            .map(|(k, v)| (k.clone(), v))
    })
}

/// The footprint of a shape that can become a building, or why it cannot.
pub fn footprint_of(o: &Object) -> Result<Vec<Vertex>, String> {
    if o.data.get("_ring").and_then(Value::as_str) == Some("hole") {
        return Err("an inner ring (hole) of a polygon".into());
    }
    let v: Vec<Vertex> = match &o.shape {
        Shape::Polyline {
            points,
            closed: true,
        } => points.clone(),
        Shape::LandscapeArea { boundary, .. } | Shape::Hardscape { boundary, .. } => {
            boundary.clone()
        }
        Shape::Rect { min, max } => curve::corners(&[
            *min,
            DVec2::new(max.x, min.y),
            *max,
            DVec2::new(min.x, max.y),
        ]),
        Shape::Circle { center, radius } => {
            let n = 48;
            curve::corners(
                &(0..n)
                    .map(|i| {
                        *center
                            + DVec2::from_angle(std::f64::consts::TAU * f64::from(i) / f64::from(n))
                                * *radius
                    })
                    .collect::<Vec<_>>(),
            )
        }
        Shape::Polyline3d {
            points,
            closed: true,
        } => curve::corners(&points.iter().map(|p| p.truncate()).collect::<Vec<_>>()),
        Shape::Massing(_) => return Err("already a massing model".into()),
        other => return Err(format!("a {} is not a closed shape", other.kind())),
    };
    if v.len() < 3 || curve::area(&v).abs() <= 0.0 {
        return Err("the outline has no area".into());
    }
    Ok(v)
}

/// Plans the conversion of the given shapes. Objects that are not found are skipped with a reason.
pub fn from_shapes(doc: &Document, ids: &[ObjectId], opts: &FromShapesOptions) -> Made {
    let mut made = Made {
        command: None,
        created: Vec::new(),
        skipped: Vec::new(),
    };
    let mut commands = Vec::new();
    let mut next = doc.id_plan();
    let mut seen = std::collections::HashSet::new();
    let defaults = Attributes {
        fill: Some(PLAN_FILL),
        ..Default::default()
    };
    for &id in ids {
        if !seen.insert(id) {
            continue;
        }
        let Some(o) = doc.object(id) else {
            made.skipped.push((id, "no such object".into()));
            continue;
        };
        let footprint = match footprint_of(o) {
            Ok(f) => f,
            Err(why) => {
                made.skipped.push((id, why));
                continue;
            }
        };
        let mut m = Massing::new(footprint, opts.default_height);
        m.floor_height = opts.floor_height;
        m.roof = opts.roof;
        m.pitch_deg = opts.pitch_deg;
        let h = field(&o.data, opts.height_field.as_deref(), &HEIGHT_FIELDS)
            .and_then(|(k, v)| number_of(v).filter(|x| *x > 0.0).map(|x| (k, x)));
        let fl = field(&o.data, opts.floors_field.as_deref(), &FLOORS_FIELDS).and_then(|(k, v)| {
            number_of(v)
                .filter(|x| *x >= 1.0 && *x <= 300.0)
                .map(|x| (k, x))
        });
        let how = match (&h, &fl) {
            (Some((k, x)), _) => {
                m.height = x * opts.height_scale;
                format!("height from {k}")
            }
            (None, Some((k, n))) => {
                m.floors = Some(n.round() as u32);
                format!("floors from {k}")
            }
            (None, None) if opts.require_height => {
                made.skipped
                    .push((id, "no height and no floors in the data".into()));
                continue;
            }
            (None, None) => format!("default height {} mm", opts.default_height),
        };
        if let Some(n) = &opts.name_field
            && let Some((_, v)) = field(&o.data, Some(n), &[])
        {
            m.name = v.as_str().map_or_else(|| v.to_string(), str::to_string);
        }
        if let Some(why) = m.problem() {
            made.skipped.push((id, why));
            continue;
        }
        let shape = Shape::Massing(Box::new(m));
        let attrs = o.attrs.over(&defaults);
        // A shape in a group gives a massing model in that group.
        commands.push(match o.parent {
            Some(parent) => Command::AddObjectIn {
                parent,
                shape,
                class: Some(o.class),
                attrs,
            },
            None => Command::AddObject {
                shape,
                layer: Some(o.layer),
                class: Some(o.class),
                attrs,
            },
        });
        let new_id = ObjectId(next.take());
        // The data of the shape (the properties of the feature) goes along.
        if !o.data.is_empty() {
            commands.push(Command::SetData {
                id: new_id,
                set: o.data.clone(),
                remove: Vec::new(),
            });
        }
        made.created.push((id, new_id, how));
    }
    if opts.delete_source && !made.created.is_empty() {
        commands.push(Command::DeleteObjects {
            ids: made.created.iter().map(|c| c.0).collect(),
        });
    }
    if !commands.is_empty() {
        made.command = Some(Command::Batch { commands });
    }
    made
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Editor;
    use serde_json::json;

    fn square(at: DVec2, w: f64) -> Shape {
        Shape::Polyline {
            points: curve::corners(&[
                at,
                at + DVec2::new(w, 0.0),
                at + DVec2::splat(w),
                at + DVec2::new(0.0, w),
            ]),
            closed: true,
        }
    }

    fn add(ed: &mut Editor, shape: Shape, data: Value) -> ObjectId {
        let out = ed
            .execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        let id = ObjectId(out.created[0]);
        if let Value::Object(m) = data {
            ed.execute(&Command::SetData {
                id,
                set: m.into_iter().collect(),
                remove: Vec::new(),
            })
            .unwrap();
        }
        id
    }

    #[test]
    fn numbers_from_data() {
        assert_eq!(number_of(&json!(12.5)), Some(12.5));
        assert_eq!(number_of(&json!("12,5 m")), Some(12.5));
        assert_eq!(number_of(&json!("-3")), Some(-3.0));
        assert_eq!(number_of(&json!("m12")), None);
        assert_eq!(number_of(&json!(true)), None);
        assert_eq!(number_of(&json!("")), None);
    }

    #[test]
    fn converts_a_batch_with_heights_floors_and_defaults() {
        let mut ed = Editor::new(Document::default());
        let a = add(
            &mut ed,
            square(DVec2::ZERO, 10_000.0),
            json!({"height": 9.5, "name": "House"}),
        );
        let b = add(
            &mut ed,
            square(DVec2::new(20_000.0, 0.0), 8000.0),
            json!({"Building:Levels": "3"}),
        );
        let c = add(
            &mut ed,
            square(DVec2::new(40_000.0, 0.0), 6000.0),
            json!({}),
        );
        let d = add(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::X,
            },
            json!({}),
        );
        let opts = FromShapesOptions {
            name_field: Some("name".into()),
            roof: RoofStyle::Gable,
            ..Default::default()
        };
        let made = from_shapes(ed.doc(), &[a, b, c, d, a, ObjectId::from_num(999)], &opts);
        assert_eq!(made.created.len(), 3);
        assert_eq!(made.skipped.len(), 2);
        assert!(made.skipped[0].1.contains("not a closed shape"));
        assert_eq!(made.skipped[1].1, "no such object");
        ed.execute(&made.command.unwrap()).unwrap();
        let doc = ed.doc();
        // The sources are gone, the massing models carry the data.
        assert!(doc.object(a).is_none());
        let ma = doc.object(made.created[0].1).unwrap();
        let Shape::Massing(m) = &ma.shape else {
            panic!()
        };
        assert_eq!(
            (m.height, m.floors, m.name.as_str(), m.roof),
            (9500.0, None, "House", RoofStyle::Gable)
        );
        assert_eq!(ma.data["height"], json!(9.5));
        assert_eq!(ma.attrs.fill, Some(PLAN_FILL));
        let Shape::Massing(mb) = &doc.object(made.created[1].1).unwrap().shape else {
            panic!()
        };
        assert_eq!((mb.floors, mb.eave_height()), (Some(3), 9000.0));
        assert_eq!(made.created[1].2, "floors from Building:Levels");
        let Shape::Massing(mc) = &doc.object(made.created[2].1).unwrap().shape else {
            panic!()
        };
        assert_eq!(mc.height, 3000.0);
        assert!(made.created[2].2.starts_with("default height"));
        // One undo step.
        assert!(ed.undo());
        assert!(ed.doc().object(a).is_some() && ed.doc().object(made.created[0].1).is_none());
    }

    #[test]
    fn options_decide() {
        let mut ed = Editor::new(Document::default());
        let a = add(&mut ed, square(DVec2::ZERO, 10_000.0), json!({"h_m": "7"}));
        let strict = FromShapesOptions {
            require_height: true,
            ..Default::default()
        };
        let made = from_shapes(ed.doc(), &[a], &strict);
        assert!(made.command.is_none());
        assert!(made.skipped[0].1.contains("no height"));
        let named = FromShapesOptions {
            height_field: Some("H_M".into()),
            height_scale: 100.0,
            delete_source: false,
            ..Default::default()
        };
        let made = from_shapes(ed.doc(), &[a], &named);
        ed.execute(&made.command.unwrap()).unwrap();
        assert!(ed.doc().object(a).is_some(), "the source stays");
        let Shape::Massing(m) = &ed.doc().object(made.created[0].1).unwrap().shape else {
            panic!()
        };
        assert_eq!(m.height, 700.0);
        assert!(
            FromShapesOptions {
                default_height: 0.0,
                ..Default::default()
            }
            .check()
            .is_err()
        );
        assert!(
            FromShapesOptions {
                pitch_deg: 95.0,
                ..Default::default()
            }
            .check()
            .is_err()
        );
        assert!(FromShapesOptions::default().check().is_ok());
    }

    #[test]
    fn rect_circle_hole_and_massing_inputs() {
        let mut ed = Editor::new(Document::default());
        let r = add(
            &mut ed,
            Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::new(5000.0, 4000.0),
            },
            json!({}),
        );
        let c = add(
            &mut ed,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 3000.0,
            },
            json!({}),
        );
        let h = add(
            &mut ed,
            square(DVec2::ZERO, 1000.0),
            json!({"_ring": "hole"}),
        );
        let flat = add(
            &mut ed,
            Shape::Polyline {
                points: curve::corners(&[DVec2::ZERO, DVec2::X, DVec2::X * 2.0]),
                closed: true,
            },
            json!({}),
        );
        let open = add(
            &mut ed,
            Shape::Polyline {
                points: curve::corners(&[DVec2::ZERO, DVec2::X, DVec2::Y]),
                closed: false,
            },
            json!({}),
        );
        let made = from_shapes(
            ed.doc(),
            &[r, c, h, flat, open],
            &FromShapesOptions::default(),
        );
        assert_eq!(made.created.len(), 2);
        assert!(made.skipped.iter().any(|s| s.1.contains("hole")));
        assert!(made.skipped.iter().any(|s| s.1.contains("no area")));
        ed.execute(&made.command.unwrap()).unwrap();
        let again = from_shapes(
            ed.doc(),
            &[made.created[0].1],
            &FromShapesOptions::default(),
        );
        assert!(again.skipped[0].1.contains("already"));
        assert!(
            from_shapes(ed.doc(), &[], &FromShapesOptions::default())
                .command
                .is_none()
        );
    }

    #[test]
    fn a_shape_in_a_group_gives_a_massing_in_the_same_group() {
        let mut ed = Editor::new(Document::default());
        let a = add(&mut ed, square(DVec2::ZERO, 5000.0), json!({"height": 6}));
        let b = add(
            &mut ed,
            square(DVec2::new(9000.0, 0.0), 5000.0),
            json!({"height": 4}),
        );
        let out = ed
            .execute(&Command::Group {
                ids: vec![a, b],
                name: None,
            })
            .unwrap();
        let g = ObjectId(out.created[0]);
        let made = from_shapes(ed.doc(), &[a], &FromShapesOptions::default());
        ed.execute(&made.command.unwrap()).unwrap();
        assert_eq!(ed.doc().object(made.created[0].1).unwrap().parent, Some(g));
    }
}
