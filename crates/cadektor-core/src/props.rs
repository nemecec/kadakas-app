//! Object properties by name, for worksheets and the Data Manager.
//!
//! One function, [`property`], reads a property of an object: a built-in name such as `Area`
//! or `Layer`, a plant style field (`Plant.spread`), or a record field (`Soil.Depth`).
//! Worksheet database rows and Data Manager rules both use it, so they agree.
//!
//! `Area` of a hardscape is its surface (sloped or draped), and `Area` of a landscape area is the
//! planted area (holes taken out), as the reports count them. `PlanArea`, `SurfaceArea`,
//! `GrossArea`, `NetArea` and `HolesArea` give the parts. With a ground (`property_on`) draped
//! objects follow the site model.
//!
//! Units: `Area` is in m2, `Volume` in m3, `Length` and `Perimeter` in m. All other lengths
//! (spread, height, dbh, thickness, elevation, rpz) are in mm, as stored.

use serde_json::Value;

use crate::curve;
use crate::model::{Document, Object, Shape, arc_sweep};
use crate::plants::link;
use crate::plants::tree::rpz_radius;
use crate::record;
use crate::resource::ResourceBody;

/// Default thickness of a hardscape in mm.
pub const DEFAULT_THICKNESS: f64 = 150.0;

/// The result of reading a property.
#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    /// The value.
    Value(Value),
    /// The name is valid, but this object has no such property (for example `Area` of a line).
    NotApplicable,
    /// The name is not a property of any object.
    Unknown,
}

/// The built-in property names, as shown to users.
pub const NAMES: &[&str] = &[
    "Id",
    "Kind",
    "Layer",
    "Class",
    "Name",
    "Style",
    "Count",
    "Area",
    "PlanArea",
    "SurfaceArea",
    "GrossArea",
    "NetArea",
    "HolesArea",
    "Length",
    "Perimeter",
    "Volume",
    "Material",
    "Thickness",
    "Elevation",
    "Species",
    "Botanical",
    "CommonName",
    "IdCode",
    "Category",
    "Spread",
    "Height",
    "Price",
    "Size",
    "Container",
    "Root",
    "DBH",
    "Condition",
    "Action",
    "RPZ",
    "X",
    "Y",
];

fn key(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-'))
        .collect::<String>()
        .to_ascii_lowercase()
}

fn num(x: f64) -> Prop {
    if x.is_finite() {
        Prop::Value(Value::from(x))
    } else {
        Prop::NotApplicable
    }
}

fn text(s: impl Into<String>) -> Prop {
    Prop::Value(Value::String(s.into()))
}

/// Length of a curve through the vertices, in mm.
fn curve_length(v: &[curve::Vertex], closed: bool) -> f64 {
    let pts = curve::flatten(v, closed, 0.5);
    let mut total: f64 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
    if closed && pts.len() > 1 && pts[0] != pts[pts.len() - 1] {
        total += pts[0].distance(pts[pts.len() - 1]);
    }
    total
}

/// Perimeter of a closed shape in m, or the length of an open one. `None` for shapes
/// that have no outline.
pub fn length_m(shape: &Shape) -> Option<f64> {
    let mm = match shape {
        Shape::Line { a, b } => a.distance(*b),
        Shape::Rect { min, max } => 2.0 * ((max.x - min.x).abs() + (max.y - min.y).abs()),
        Shape::Circle { radius, .. } => std::f64::consts::TAU * radius,
        Shape::Arc {
            radius, start, end, ..
        } => radius * arc_sweep(*start, *end),
        Shape::Polyline { points, closed } => curve_length(points, *closed),
        Shape::LandscapeArea { boundary, .. } | Shape::Hardscape { boundary, .. } => {
            curve_length(boundary, true)
        }
        Shape::Massing(m) => curve_length(&m.footprint, true),
        Shape::Polyline3d { points, closed } => {
            let mut t: f64 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
            if *closed && points.len() > 1 {
                t += points[0].distance(points[points.len() - 1]);
            }
            t
        }
        _ => return None,
    };
    Some(mm / 1000.0)
}

/// Area in m2: closed shapes, and the canopy circle of a plant.
pub fn area_m2(shape: &Shape) -> Option<f64> {
    match shape {
        Shape::Plant { spread, .. } => Some(std::f64::consts::PI * (spread / 2.0).powi(2) / 1e6),
        s => s.area().map(|a| a / 1e6),
    }
}

fn is_closed(shape: &Shape) -> bool {
    matches!(
        shape,
        Shape::Rect { .. }
            | Shape::Circle { .. }
            | Shape::LandscapeArea { .. }
            | Shape::Hardscape { .. }
            | Shape::Massing(_)
            | Shape::Polyline { closed: true, .. }
            | Shape::Polyline3d { closed: true, .. }
    )
}

fn position(shape: &Shape) -> Option<glam::DVec2> {
    match shape {
        Shape::Plant { at, .. } | Shape::ExistingTree { at, .. } | Shape::Text { at, .. } => {
            Some(*at)
        }
        Shape::Circle { center, .. } | Shape::Arc { center, .. } => Some(*center),
        Shape::Locus3d { p } => Some(p.truncate()),
        Shape::Rect { min, max } => Some((*min + *max) / 2.0),
        _ => None,
    }
}

/// True if the object has a record of this name attached.
pub fn has_record(obj: &Object, name: &str) -> bool {
    obj.data.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case(name.trim())
            && v.as_object()
                .is_some_and(|m| m.contains_key(record::MARKER))
    })
}

/// Reads a property with no ground: a draped hardscape or area is measured flat.
pub fn property(doc: &Document, obj: &Object, path: &[&str]) -> Prop {
    property_on(doc, obj, path, &crate::hardscape::NoTerrain)
}

/// Reads a property. `path` has one name (`Area`), or two or more names: `Plant` and a style
/// field, or a record name and a field name (`Soil`, `Depth`).
/// `ground` gives the heights of the site model, so `Area` and `Volume` of draped hardscapes and
/// landscape areas agree with `report.hardscape` and `report.landscape_areas`.
pub fn property_on(
    doc: &Document,
    obj: &Object,
    path: &[&str],
    ground: &dyn crate::hardscape::TerrainSource,
) -> Prop {
    match path {
        [] => Prop::Unknown,
        [name] => builtin(doc, obj, name, ground),
        [first, rest @ ..] if first.eq_ignore_ascii_case("plant") => plant_field(doc, obj, rest),
        [record_name, field] => record_field(doc, obj, record_name, field),
        _ => Prop::Unknown,
    }
}

fn record_field(doc: &Document, obj: &Object, record_name: &str, field: &str) -> Prop {
    let Some(res) = doc.resources.iter().find(|r| {
        matches!(r.body, ResourceBody::RecordFormat(_))
            && r.name.eq_ignore_ascii_case(record_name.trim())
    }) else {
        return Prop::Unknown;
    };
    let ResourceBody::RecordFormat(f) = &res.body else {
        return Prop::Unknown;
    };
    let Some(def) = f.field(field) else {
        return Prop::Unknown;
    };
    match record::values(obj, &res.name).and_then(|m| m.get(&def.name)) {
        Some(v) => Prop::Value(v.clone()),
        None => Prop::NotApplicable,
    }
}

fn plant_field(doc: &Document, obj: &Object, rest: &[&str]) -> Prop {
    let Some(e) = link::effective(doc, obj) else {
        return if rest.is_empty() {
            Prop::Unknown
        } else {
            Prop::NotApplicable
        };
    };
    let mut v = serde_json::to_value(&e.style).unwrap_or(Value::Null);
    for (i, seg) in rest.iter().enumerate() {
        let Some(m) = v.as_object() else {
            return Prop::Unknown;
        };
        let Some(found) = m
            .iter()
            .find(|(k, _)| key(k) == key(seg))
            .map(|(_, x)| x.clone())
        else {
            // The last segment of `custom.<key>` may be missing on this plant.
            return if i > 0 && rest[0].eq_ignore_ascii_case("custom") {
                Prop::NotApplicable
            } else {
                Prop::Unknown
            };
        };
        v = found;
    }
    if rest.is_empty() || v.is_object() {
        return Prop::Unknown;
    }
    if v.is_null() {
        Prop::NotApplicable
    } else {
        Prop::Value(v)
    }
}

/// Gross area, planted (net) area and the ratio of draped surface to plan area of a landscape area.
fn landscape_area(
    doc: &Document,
    obj: &Object,
    ground: &dyn crate::hardscape::TerrainSource,
) -> Option<(f64, f64, f64)> {
    let q = crate::landscape::quantities(doc, obj)?;
    let ratio = match crate::landscape::surface_area_on(obj, ground) {
        Some(s) if q.gross_area_m2 > 0.0 => s / q.gross_area_m2,
        _ => 1.0,
    };
    Some((q.gross_area_m2, q.net_area_m2, ratio))
}

fn builtin(
    doc: &Document,
    obj: &Object,
    name: &str,
    ground: &dyn crate::hardscape::TerrainSource,
) -> Prop {
    let shape = &obj.shape;
    match key(name).as_str() {
        "id" => Prop::Value(Value::from(obj.id.0)),
        "kind" | "type" => text(shape.kind()),
        "layer" => doc
            .layer(obj.layer)
            .map_or(Prop::NotApplicable, |l| text(l.name.clone())),
        "class" => doc
            .class(obj.class)
            .map_or(Prop::NotApplicable, |c| text(c.name.clone())),
        "count" => Prop::Value(Value::from(1)),
        "area" | "surfacearea" => match shape {
            // The surface of the hardscape: sloped or draped, as the hardscape report counts it.
            Shape::Hardscape { .. } => crate::hardscape::report::quantities_on(doc, obj, ground)
                .map_or(Prop::NotApplicable, |q| num(q.surface_area_m2)),
            // The planted area: holes are taken out, and a draped area counts its surface.
            Shape::LandscapeArea { .. } => landscape_area(doc, obj, ground)
                .map_or(Prop::NotApplicable, |(_, net, ratio)| num(net * ratio)),
            _ if key(name) == "surfacearea" => Prop::NotApplicable,
            _ => area_m2(shape).map_or(Prop::NotApplicable, num),
        },
        "planarea" => match shape {
            Shape::Hardscape { .. } => crate::hardscape::report::quantities_on(doc, obj, ground)
                .map_or(Prop::NotApplicable, |q| num(q.plan_area_m2)),
            _ => area_m2(shape).map_or(Prop::NotApplicable, num),
        },
        "grossarea" | "netarea" | "holesarea" => match landscape_area(doc, obj, ground) {
            Some((gross, net, _)) => match key(name).as_str() {
                "grossarea" => num(gross),
                "netarea" => num(net),
                _ => num((gross - net).max(0.0)),
            },
            None => Prop::NotApplicable,
        },
        "length" => length_m(shape).map_or(Prop::NotApplicable, num),
        "perimeter" => {
            if is_closed(shape) {
                length_m(shape).map_or(Prop::NotApplicable, num)
            } else {
                Prop::NotApplicable
            }
        }
        // The volume of all layers, components included.
        "volume" => match shape {
            Shape::Massing(m) => num(m.quantities().volume / 1e9),
            Shape::Hardscape { .. } => crate::hardscape::report::quantities_on(doc, obj, ground)
                .map_or(Prop::NotApplicable, |q| num(q.volume_m3)),
            _ => Prop::NotApplicable,
        },
        "x" => position(shape).map_or(Prop::NotApplicable, |p| num(p.x)),
        "y" => position(shape).map_or(Prop::NotApplicable, |p| num(p.y)),
        "name" | "style" => match shape {
            Shape::Plant { .. } => {
                link::effective(doc, obj).map_or(Prop::NotApplicable, |e| text(e.style_name))
            }
            Shape::ExistingTree { species, .. } => text(species.clone()),
            Shape::LandscapeArea { name, .. } => text(name.clone()),
            Shape::Massing(m) => text(m.name.clone()),
            Shape::Hardscape { material, .. } => text(material.clone()),
            Shape::Text { text: t, .. } => text(t.clone()),
            _ => Prop::NotApplicable,
        },
        "material" => match shape {
            Shape::Hardscape { material, .. } => text(material.clone()),
            _ => Prop::NotApplicable,
        },
        "thickness" => match shape {
            Shape::Hardscape { thickness, .. } => num(thickness.unwrap_or(DEFAULT_THICKNESS)),
            _ => Prop::NotApplicable,
        },
        "elevation" => match shape {
            Shape::Hardscape { elevation, .. } => num(elevation.unwrap_or(0.0)),
            _ => Prop::NotApplicable,
        },
        "species" => match shape {
            Shape::Plant { species, .. } | Shape::ExistingTree { species, .. } => {
                text(species.clone())
            }
            _ => Prop::NotApplicable,
        },
        "spread" => match shape {
            Shape::Plant { spread, .. } | Shape::ExistingTree { spread, .. } => num(*spread),
            _ => Prop::NotApplicable,
        },
        // Massing models: the number of floors, the gross floor area (m2) and the roof style.
        "floors" => match shape {
            Shape::Massing(m) => num(f64::from(m.floor_count())),
            _ => Prop::NotApplicable,
        },
        "grossfloorarea" => match shape {
            Shape::Massing(m) => num(m.quantities().gross_floor_area / 1e6),
            _ => Prop::NotApplicable,
        },
        "roof" => match shape {
            Shape::Massing(m) => text(m.roof.name().to_string()),
            _ => Prop::NotApplicable,
        },
        "height" => match shape {
            Shape::Massing(m) => num(m.eave_height()),
            Shape::Plant { height, .. } | Shape::ExistingTree { height, .. } => num(*height),
            _ => Prop::NotApplicable,
        },
        "dbh" | "condition" | "action" | "rpz" => match shape {
            Shape::ExistingTree {
                dbh,
                condition,
                action,
                rpz_factor,
                rpz_cap,
                ..
            } => match key(name).as_str() {
                "dbh" => num(*dbh),
                "condition" => text(condition.id()),
                "action" => text(action.id()),
                _ => num(rpz_radius(*dbh, *rpz_factor, *rpz_cap)),
            },
            _ => Prop::NotApplicable,
        },
        k @ ("botanical" | "botanicalname" | "commonname" | "idcode" | "category" | "price"
        | "size" | "container" | "root") => match shape {
            Shape::Plant { .. } => {
                let field = match k {
                    "botanical" | "botanicalname" => "botanical_name",
                    "commonname" => "common_name",
                    "idcode" => "id_code",
                    other => other,
                };
                plant_field(doc, obj, &[field])
            }
            Shape::ExistingTree { species, .. } if matches!(k, "botanical" | "botanicalname") => {
                text(species.clone())
            }
            Shape::ExistingTree { .. } if k == "category" => text("tree"),
            _ => Prop::NotApplicable,
        },
        _ => Prop::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Editor};
    use crate::model::{Attributes, Shape};
    use crate::plants::style::PlantStyle;
    use crate::record::{FieldDef, FieldType, RecordFormat};
    use crate::resource::{ResourceBody, ResourceId};
    use glam::DVec2;
    use serde_json::json;

    fn add(ed: &mut Editor, shape: Shape) -> crate::model::ObjectId {
        let out = ed
            .execute(&Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        crate::model::ObjectId(out.created[0])
    }

    fn p(ed: &Editor, id: crate::model::ObjectId, path: &[&str]) -> Prop {
        property(ed.doc(), ed.doc().object(id).unwrap(), path)
    }

    fn v(x: Value) -> Prop {
        Prop::Value(x)
    }

    #[test]
    fn rect_and_line_and_circle() {
        let mut ed = Editor::default();
        let r = add(
            &mut ed,
            Shape::Rect {
                min: DVec2::ZERO,
                max: DVec2::new(4000.0, 2000.0),
            },
        );
        assert_eq!(p(&ed, r, &["Area"]), v(json!(8.0)));
        assert_eq!(p(&ed, r, &["perimeter"]), v(json!(12.0)));
        assert_eq!(p(&ed, r, &["Length"]), v(json!(12.0)));
        assert_eq!(p(&ed, r, &["Count"]), v(json!(1)));
        assert_eq!(p(&ed, r, &["kind"]), v(json!("rect")));
        let layer_name = ed.doc().layers[0].name.clone();
        assert_eq!(p(&ed, r, &["layer"]), v(json!(layer_name)));
        assert_eq!(
            p(&ed, r, &["class"]),
            v(json!(ed.doc().classes[0].name.clone()))
        );
        assert_eq!(p(&ed, r, &["x"]), v(json!(2000.0)));
        let l = add(
            &mut ed,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::new(3000.0, 4000.0),
            },
        );
        assert_eq!(p(&ed, l, &["Length"]), v(json!(5.0)));
        assert_eq!(p(&ed, l, &["Perimeter"]), Prop::NotApplicable);
        assert_eq!(p(&ed, l, &["Area"]), Prop::NotApplicable);
        assert_eq!(p(&ed, l, &["x"]), Prop::NotApplicable);
        let c = add(
            &mut ed,
            Shape::Circle {
                center: DVec2::new(1.0, 2.0),
                radius: 1000.0,
            },
        );
        let Prop::Value(a) = p(&ed, c, &["area"]) else {
            panic!()
        };
        assert!((a.as_f64().unwrap() - std::f64::consts::PI).abs() < 1e-9);
        assert_eq!(p(&ed, c, &["y"]), v(json!(2.0)));
        let arc = add(
            &mut ed,
            Shape::Arc {
                center: DVec2::ZERO,
                radius: 1000.0,
                start: 0.0,
                end: std::f64::consts::PI,
            },
        );
        let Prop::Value(a) = p(&ed, arc, &["length"]) else {
            panic!()
        };
        assert!((a.as_f64().unwrap() - std::f64::consts::PI).abs() < 1e-9);
        assert_eq!(p(&ed, l, &["nope"]), Prop::Unknown);
        assert_eq!(
            property(ed.doc(), ed.doc().object(l).unwrap(), &[]),
            Prop::Unknown
        );
        assert_eq!(
            property(ed.doc(), ed.doc().object(l).unwrap(), &["a", "b", "c"]),
            Prop::Unknown
        );
    }

    #[test]
    fn polylines_and_areas() {
        let mut ed = Editor::default();
        let sq = |closed| Shape::Polyline {
            points: curve::corners(&[
                DVec2::ZERO,
                DVec2::new(1000.0, 0.0),
                DVec2::new(1000.0, 1000.0),
                DVec2::new(0.0, 1000.0),
            ]),
            closed,
        };
        let open = add(&mut ed, sq(false));
        let closed = add(&mut ed, sq(true));
        assert_eq!(p(&ed, open, &["Length"]), v(json!(3.0)));
        assert_eq!(p(&ed, closed, &["Length"]), v(json!(4.0)));
        assert_eq!(p(&ed, closed, &["Perimeter"]), v(json!(4.0)));
        assert_eq!(p(&ed, closed, &["Area"]), v(json!(1.0)));
        assert_eq!(p(&ed, open, &["Area"]), Prop::NotApplicable);
        let p3 = add(
            &mut ed,
            Shape::Polyline3d {
                points: vec![glam::DVec3::ZERO, glam::DVec3::new(0.0, 0.0, 2000.0)],
                closed: false,
            },
        );
        assert_eq!(p(&ed, p3, &["Length"]), v(json!(2.0)));
        let p3c = add(
            &mut ed,
            Shape::Polyline3d {
                points: vec![
                    glam::DVec3::ZERO,
                    glam::DVec3::new(1000.0, 0.0, 0.0),
                    glam::DVec3::new(1000.0, 1000.0, 0.0),
                ],
                closed: true,
            },
        );
        let Prop::Value(l) = p(&ed, p3c, &["Length"]) else {
            panic!()
        };
        assert!((l.as_f64().unwrap() - (2.0 + 2f64.sqrt())).abs() < 1e-9);
    }

    #[test]
    fn hardscape_landscape_text_and_trees() {
        let mut ed = Editor::default();
        let b = curve::corners(&[
            DVec2::ZERO,
            DVec2::new(2000.0, 0.0),
            DVec2::new(2000.0, 3000.0),
            DVec2::new(0.0, 3000.0),
        ]);
        let h = add(
            &mut ed,
            Shape::Hardscape {
                boundary: b.clone(),
                material: "Granite".into(),
                thickness: None,
                elevation: None,
            },
        );
        assert_eq!(p(&ed, h, &["Material"]), v(json!("Granite")));
        assert_eq!(p(&ed, h, &["Style"]), v(json!("Granite")));
        assert_eq!(p(&ed, h, &["Thickness"]), v(json!(150.0)));
        assert_eq!(p(&ed, h, &["Elevation"]), v(json!(0.0)));
        let Prop::Value(vol) = p(&ed, h, &["Volume"]) else {
            panic!()
        };
        assert!((vol.as_f64().unwrap() - 0.9).abs() < 1e-9);
        let h2 = add(
            &mut ed,
            Shape::Hardscape {
                boundary: b.clone(),
                material: "Brick".into(),
                thickness: Some(100.0),
                elevation: Some(50.0),
            },
        );
        assert_eq!(p(&ed, h2, &["thickness"]), v(json!(100.0)));
        assert_eq!(p(&ed, h2, &["elevation"]), v(json!(50.0)));
        let la = add(
            &mut ed,
            Shape::LandscapeArea {
                boundary: b,
                name: "Lawn".into(),
            },
        );
        assert_eq!(p(&ed, la, &["Name"]), v(json!("Lawn")));
        assert_eq!(p(&ed, la, &["Material"]), Prop::NotApplicable);
        assert_eq!(p(&ed, la, &["Volume"]), Prop::NotApplicable);
        assert_eq!(p(&ed, la, &["Thickness"]), Prop::NotApplicable);
        assert_eq!(p(&ed, la, &["Elevation"]), Prop::NotApplicable);
        let t = add(
            &mut ed,
            Shape::Text {
                at: DVec2::new(5.0, 6.0),
                text: "Hello".into(),
                height: 100.0,
            },
        );
        assert_eq!(p(&ed, t, &["Name"]), v(json!("Hello")));
        let tree = add(
            &mut ed,
            Shape::ExistingTree {
                at: DVec2::ZERO,
                species: "Oak".into(),
                dbh: 500.0,
                spread: 8000.0,
                height: 15000.0,
                condition: Default::default(),
                action: Default::default(),
                rpz_factor: None,
                rpz_cap: None,
            },
        );
        assert_eq!(p(&ed, tree, &["Species"]), v(json!("Oak")));
        assert_eq!(p(&ed, tree, &["Style"]), v(json!("Oak")));
        assert_eq!(p(&ed, tree, &["DBH"]), v(json!(500.0)));
        assert_eq!(p(&ed, tree, &["Condition"]), v(json!("fair")));
        assert_eq!(p(&ed, tree, &["Action"]), v(json!("retain")));
        assert_eq!(p(&ed, tree, &["RPZ"]), v(json!(6000.0)));
        assert_eq!(p(&ed, tree, &["Spread"]), v(json!(8000.0)));
        assert_eq!(p(&ed, tree, &["Height"]), v(json!(15000.0)));
        assert_eq!(p(&ed, tree, &["Botanical"]), v(json!("Oak")));
        assert_eq!(p(&ed, tree, &["Category"]), v(json!("tree")));
        assert_eq!(p(&ed, tree, &["Price"]), Prop::NotApplicable);
        assert_eq!(p(&ed, tree, &["DBH"]), v(json!(500.0)));
        assert_eq!(p(&ed, h, &["DBH"]), Prop::NotApplicable);
        assert_eq!(p(&ed, h, &["Species"]), Prop::NotApplicable);
        assert_eq!(p(&ed, h, &["Spread"]), Prop::NotApplicable);
        assert_eq!(p(&ed, h, &["Height"]), Prop::NotApplicable);
        assert_eq!(p(&ed, tree, &["Plant", "price"]), Prop::NotApplicable);
        assert_eq!(p(&ed, tree, &["Plant"]), Prop::Unknown);
    }

    #[test]
    fn plants_with_styles_and_overrides() {
        let mut ed = Editor::default();
        let style = PlantStyle {
            botanical_name: "Acer campestre".into(),
            common_name: "Field maple".into(),
            id_code: "AC".into(),
            price: 100.0,
            size: "14-16".into(),
            ..PlantStyle::default()
        };
        let sid = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Maple".into(),
                body: ResourceBody::plant_style(style),
            })
            .unwrap()
            .created[0],
        );
        let ids = ed
            .execute(&Command::AddPlants {
                style: Some(sid),
                overrides: json!({"price": 150.0}).as_object().unwrap().clone(),
                positions: vec![DVec2::ZERO],
                layer: None,
                class: None,
                variation: None,
            })
            .unwrap();
        let id = crate::model::ObjectId(ids.created[0]);
        assert_eq!(p(&ed, id, &["Style"]), v(json!("Maple")));
        assert_eq!(p(&ed, id, &["Price"]), v(json!(150.0)));
        assert_eq!(p(&ed, id, &["Plant", "price"]), v(json!(150.0)));
        assert_eq!(p(&ed, id, &["plant", "Id Code"]), v(json!("AC")));
        assert_eq!(p(&ed, id, &["IdCode"]), v(json!("AC")));
        assert_eq!(p(&ed, id, &["Botanical"]), v(json!("Acer campestre")));
        assert_eq!(p(&ed, id, &["CommonName"]), v(json!("Field maple")));
        assert_eq!(p(&ed, id, &["Size"]), v(json!("14-16")));
        assert_eq!(p(&ed, id, &["Category"]), v(json!("shrub")));
        assert_eq!(p(&ed, id, &["Plant", "fill"]), Prop::NotApplicable);
        assert_eq!(p(&ed, id, &["Plant", "nope"]), Prop::Unknown);
        assert_eq!(p(&ed, id, &["Plant", "custom"]), Prop::Unknown);
        assert_eq!(
            p(&ed, id, &["Plant", "custom", "water"]),
            Prop::NotApplicable
        );
        assert_eq!(p(&ed, id, &["Plant", "price", "x"]), Prop::Unknown);
        let Prop::Value(a) = p(&ed, id, &["Area"]) else {
            panic!()
        };
        assert!((a.as_f64().unwrap() - std::f64::consts::PI * 2.25).abs() < 1e-9);
        // A plant by instance (no style) shows its species.
        let id2 = add(
            &mut ed,
            Shape::Plant {
                at: DVec2::ZERO,
                species: "Birch".into(),
                spread: 2000.0,
                height: 5000.0,
            },
        );
        assert_eq!(p(&ed, id2, &["Style"]), v(json!("Birch")));
        assert_eq!(p(&ed, id2, &["Species"]), v(json!("Birch")));
    }

    fn square_ring(side: f64, at: f64) -> Vec<crate::curve::Vertex> {
        curve::corners(&[
            DVec2::new(at, at),
            DVec2::new(at + side, at),
            DVec2::new(at + side, at + side),
            DVec2::new(at, at + side),
        ])
    }

    #[test]
    fn hardscape_and_landscape_values_agree_with_the_reports() {
        use crate::hardscape::style::{Component, HardscapeStyle};
        use crate::hardscape::{HardscapePatch, NoTerrain, report};
        use crate::landscape::LandscapePatch;
        use crate::landscape::style::{LandscapeAreaStyle, MixEntry};
        use crate::model::ObjectId;
        let mut ed = Editor::default();
        // A styled hardscape: surface layer and a component, so the volume is more than area x thickness.
        let style = HardscapeStyle {
            thickness: 60.0,
            components: vec![Component {
                name: "Base".into(),
                material: "Gravel".into(),
                thickness: 150.0,
                color: None,
            }],
            ..HardscapeStyle::default()
        };
        let hs = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Paving".into(),
                body: ResourceBody::HardscapeStyle(Box::new(style)),
            })
            .unwrap()
            .created[0],
        );
        let h = ObjectId(
            ed.execute(&Command::AddHardscape {
                boundary: Some(square_ring(2000.0, 0.0)),
                patch: Box::new(HardscapePatch {
                    style: Some(hs),
                    ..Default::default()
                }),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        let q = report::quantities_on(ed.doc(), ed.doc().object(h).unwrap(), &NoTerrain).unwrap();
        assert!((q.volume_m3 - 4.0 * 0.21).abs() < 1e-9, "{}", q.volume_m3);
        assert_eq!(p(&ed, h, &["Volume"]), v(json!(q.volume_m3)));
        assert_eq!(p(&ed, h, &["Area"]), v(json!(q.surface_area_m2)));
        assert_eq!(p(&ed, h, &["PlanArea"]), v(json!(q.plan_area_m2)));
        assert_eq!(p(&ed, h, &["SurfaceArea"]), v(json!(q.surface_area_m2)));
        assert_eq!(p(&ed, h, &["Perimeter"]), v(json!(q.perimeter_m)));
        assert_eq!(p(&ed, h, &["GrossArea"]), Prop::NotApplicable);
        // A slope makes the surface larger than the plan.
        ed.execute(&Command::SetHardscape {
            ids: vec![h],
            patch: HardscapePatch {
                slope: Some(crate::hardscape::Slope::Angle {
                    degrees: 30.0,
                    angle_deg: 0.0,
                }),
                ..Default::default()
            },
        })
        .unwrap();
        let q = report::quantities_on(ed.doc(), ed.doc().object(h).unwrap(), &NoTerrain).unwrap();
        assert!(q.surface_area_m2 > q.plan_area_m2 + 0.1);
        assert_eq!(p(&ed, h, &["Area"]), v(json!(q.surface_area_m2)));
        assert_eq!(p(&ed, h, &["PlanArea"]), v(json!(q.plan_area_m2)));
        // A ground makes a draped hardscape follow it.
        ed.execute(&Command::SetHardscape {
            ids: vec![h],
            patch: HardscapePatch {
                slope: Some(crate::hardscape::Slope::None),
                drape: Some(true),
                ..Default::default()
            },
        })
        .unwrap();
        let hill = |_: &Object, p: DVec2| Some(p.x * 0.5);
        struct Hill(fn(&Object, DVec2) -> Option<f64>);
        impl crate::hardscape::TerrainSource for Hill {
            fn elevation(&self, o: &Object, p: DVec2) -> Option<f64> {
                (self.0)(o, p)
            }
        }
        let ground = Hill(hill);
        let obj = ed.doc().object(h).unwrap();
        let on = report::quantities_on(ed.doc(), obj, &ground).unwrap();
        assert!(on.draped && on.surface_area_m2 > on.plan_area_m2 * 1.05);
        assert_eq!(
            property_on(ed.doc(), obj, &["Area"], &ground),
            v(json!(on.surface_area_m2))
        );
        assert_eq!(
            property_on(ed.doc(), obj, &["Volume"], &ground),
            v(json!(on.volume_m3))
        );
        assert_eq!(p(&ed, h, &["Area"]), v(json!(4.0)), "no ground, no drape");

        // A landscape area with a hole and a plant mix.
        let plant = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Sedge".into(),
                body: ResourceBody::plant_style(PlantStyle {
                    spread: 500.0,
                    ..PlantStyle::default()
                }),
            })
            .unwrap()
            .created[0],
        );
        let ls = ResourceId(
            ed.execute(&Command::AddResource {
                name: "Meadow".into(),
                body: ResourceBody::LandscapeAreaStyle(Box::new(LandscapeAreaStyle {
                    mix: vec![MixEntry {
                        plant,
                        percent: 100.0,
                        ..Default::default()
                    }],
                    ..Default::default()
                })),
            })
            .unwrap()
            .created[0],
        );
        let a = ObjectId(
            ed.execute(&Command::AddLandscapeArea {
                boundary: square_ring(10_000.0, 0.0),
                name: "Lawn".into(),
                patch: Box::new(LandscapePatch {
                    style: Some(ls),
                    holes: Some(vec![square_ring(2000.0, 4000.0)]),
                    ..Default::default()
                }),
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap()
            .created[0],
        );
        assert_eq!(p(&ed, a, &["GrossArea"]), v(json!(100.0)));
        assert_eq!(p(&ed, a, &["HolesArea"]), v(json!(4.0)));
        assert_eq!(p(&ed, a, &["NetArea"]), v(json!(96.0)));
        assert_eq!(p(&ed, a, &["Area"]), v(json!(96.0)), "holes are taken out");
        assert_eq!(p(&ed, a, &["Volume"]), Prop::NotApplicable);
        let rep = crate::landscape::report(ed.doc(), &NoTerrain);
        assert_eq!(rep.rows[0].quantities.net_area_m2, 96.0);
        // A draped area counts its surface for the planted part.
        ed.execute(&Command::SetLandscapeArea {
            ids: vec![a],
            patch: LandscapePatch {
                drape: Some(true),
                ..Default::default()
            },
        })
        .unwrap();
        let obj = ed.doc().object(a).unwrap();
        let rep = crate::landscape::report(ed.doc(), &ground);
        let surface = rep.rows[0].surface_area_m2.unwrap();
        assert!(surface > 100.0 * 1.05);
        let Prop::Value(area) = property_on(ed.doc(), obj, &["Area"], &ground) else {
            panic!()
        };
        assert!((area.as_f64().unwrap() - 96.0 * surface / 100.0).abs() < 1e-9);
        assert_eq!(p(&ed, a, &["Area"]), v(json!(96.0)));
        // Plants and other shapes keep their own rules.
        assert_eq!(p(&ed, a, &["PlanArea"]), v(json!(100.0)));
    }

    #[test]
    fn reserved_data_keys_are_not_records() {
        let mut ed = Editor::default();
        let a = add(
            &mut ed,
            Shape::Hardscape {
                boundary: square_ring(1000.0, 0.0),
                material: "x".into(),
                thickness: None,
                elevation: None,
            },
        );
        let o = ed.doc().object(a).unwrap();
        let mut o = o.clone();
        o.data.insert("hardscape".into(), json!({"mode": "slab"}));
        o.data.insert("landscape_area".into(), json!({"holes": []}));
        o.data.insert("plant".into(), json!({"style": 3}));
        o.data.insert(
            "worksheet_table".into(),
            json!({"worksheet": 1, "text_height": 3.0}),
        );
        for key in [
            "hardscape",
            "landscape_area",
            "plant",
            "worksheet_table",
            "Hardscape",
        ] {
            assert!(!has_record(&o, key), "{key} is not a record");
        }
        // A record format cannot take these names.
        for name in [
            "hardscape",
            "landscape_area",
            "Plant",
            "worksheet_table",
            "rotation",
        ] {
            let r = ed.execute(&Command::AddResource {
                name: name.into(),
                body: ResourceBody::RecordFormat(Box::default()),
            });
            assert!(r.is_err(), "{name}");
        }
    }

    #[test]
    fn record_fields() {
        let mut ed = Editor::default();
        let fmt = RecordFormat {
            fields: vec![FieldDef::new("Depth", FieldType::Dimension)],
        };
        ed.execute(&Command::AddResource {
            name: "Soil".into(),
            body: ResourceBody::RecordFormat(Box::new(fmt)),
        })
        .unwrap();
        let a = add(
            &mut ed,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 1.0,
            },
        );
        assert!(!has_record(ed.doc().object(a).unwrap(), "Soil"));
        assert_eq!(p(&ed, a, &["Soil", "Depth"]), Prop::NotApplicable);
        let soil = ed
            .doc()
            .resources
            .iter()
            .find(|r| r.name == "Soil")
            .unwrap()
            .id;
        ed.execute(&Command::AttachRecord {
            ids: vec![a],
            record: soil,
            values: json!({"Depth": 300.0}).as_object().unwrap().clone(),
        })
        .unwrap();
        assert!(has_record(ed.doc().object(a).unwrap(), " soil "));
        assert_eq!(p(&ed, a, &["soil", "depth"]), v(json!(300.0)));
        assert_eq!(p(&ed, a, &["Soil", "Nope"]), Prop::Unknown);
        // Plain data under the same name is not a record.
        let b = add(
            &mut ed,
            Shape::Circle {
                center: DVec2::ZERO,
                radius: 1.0,
            },
        );
        ed.execute(&Command::SetData {
            id: b,
            set: [("Other".to_string(), json!({"Depth": 5}))].into(),
            remove: vec![],
        })
        .unwrap();
        assert!(!has_record(ed.doc().object(b).unwrap(), "Other"));
        assert_eq!(p(&ed, a, &["Nope", "Depth"]), Prop::Unknown);
        assert!(NAMES.iter().all(|n| !matches!(
            builtin(
                ed.doc(),
                ed.doc().object(a).unwrap(),
                n,
                &crate::hardscape::NoTerrain
            ),
            Prop::Unknown
        )));
    }
}
