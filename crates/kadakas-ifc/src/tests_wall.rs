//! Landscape walls in the IFC export.

use std::collections::BTreeMap;

use glam::{DVec2, DVec3};
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve;
use kadakas_core::model::*;
use kadakas_core::resource::ResourceBody;
use serde_json::json;

use crate::export::Schema;
use crate::step::{self, StepFile, Value};
use crate::{IfcOptions, export_bytes};

fn export(doc: &Document, opts: &IfcOptions) -> (StepFile, kadakas_io::Report) {
    let (bytes, report) = export_bytes(doc, opts).unwrap();
    (
        step::parse(&String::from_utf8(bytes).unwrap()).unwrap(),
        report,
    )
}

fn named<'a>(f: &'a StepFile, name: &str) -> Vec<&'a step::Entity> {
    f.entities.iter().filter(|e| e.name == name).collect()
}

fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddObject {
            shape,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

fn wall(ed: &mut Editor, pts: &[(f64, f64)], data: serde_json::Value) -> ObjectId {
    let id = add(
        ed,
        Shape::Polyline {
            points: curve::corners(&pts.iter().map(|p| DVec2::new(p.0, p.1)).collect::<Vec<_>>()),
            closed: false,
        },
    );
    ed.execute(&Command::SetData {
        id,
        set: BTreeMap::from([(kadakas_site::grading::wall::KEY.to_string(), data)]),
        remove: vec![],
    })
    .unwrap();
    id
}

fn doc() -> Document {
    let mut ed = Editor::with_sequential_ids();
    let mut points = Vec::new();
    for i in 0..9 {
        for j in 0..9 {
            let (x, y) = (f64::from(i) * 1000.0, f64::from(j) * 1000.0);
            points.push(DVec3::new(x, y, x * 0.1));
        }
    }
    add(
        &mut ed,
        Shape::SiteModel {
            points,
            lines: vec![],
            settings: Default::default(),
        },
    );
    let style = kadakas_site::grading::wall::WallStyle {
        material: "Concrete".into(),
        components: vec![kadakas_site::grading::wall::Component {
            name: "Block".into(),
            material: "Concrete".into(),
            thickness: 200.0,
        }],
        coping: Some(kadakas_site::grading::wall::Coping {
            height: 50.0,
            overhang: 20.0,
            material: "Stone".into(),
        }),
        ..Default::default()
    };
    let rid = ed
        .execute(&Command::AddResource {
            name: "Block wall".into(),
            body: style.to_body(),
        })
        .unwrap()
        .created[0];
    assert!(matches!(
        ed.doc()
            .resources
            .get(kadakas_core::resource::ResourceId(rid))
            .unwrap()
            .body,
        ResourceBody::Unknown(_)
    ));
    wall(
        &mut ed,
        &[(1000.0, 2000.0), (6000.0, 2000.0)],
        json!({"top": {"mode": "terrain", "offset": 600.0}, "style": rid}),
    );
    wall(
        &mut ed,
        &[(1000.0, 4000.0), (4000.0, 4000.0)],
        json!({"top": {"mode": "fixed", "elevation": 1500.0}}),
    );
    wall(
        &mut ed,
        &[(7000.0, 500.0), (7000.0, 3500.0)],
        json!({"top": {"mode": "fixed", "elevation": 1200.0}, "site_modifier": true}),
    );
    // A wall that is a point (a path with no length) has no solid.
    wall(&mut ed, &[(2000.0, 6000.0), (2000.0, 6000.0)], json!({}));
    ed.doc().clone()
}

fn text_attr(e: &step::Entity, i: usize) -> Option<&str> {
    match &e.attrs[i] {
        Value::Str(s) | Value::Enum(s) => Some(s.as_str()),
        _ => None,
    }
}

#[test]
fn walls_are_ifc_walls_with_type_and_materials() {
    let d = doc();
    let (f, report) = export(&d, &IfcOptions::default());
    assert!(f.dangling().is_empty());
    let walls = named(&f, "IFCWALL");
    assert_eq!(walls.len(), 3);
    assert_eq!(report.objects["wall"], 3);
    assert_eq!(report.skipped.iter().map(|s| s.count).sum::<usize>(), 1);
    let types: Vec<(Option<&str>, Option<&str>)> = walls
        .iter()
        .map(|e| (text_attr(e, 4), text_attr(e, 8)))
        .collect();
    assert!(types.contains(&(Some("LANDSCAPEWALL"), Some("SOLIDWALL"))));
    assert!(types.contains(&(Some("RETAININGWALL"), Some("USERDEFINED"))));
    assert!(named(&f, "IFCELEMENTQUANTITY").len() >= 3);
    assert!(
        named(&f, "IFCMATERIAL")
            .iter()
            .any(|m| text_attr(m, 0) == Some("Concrete"))
    );
    assert!(!named(&f, "IFCTRIANGULATEDFACESET").is_empty());

    // IFC4x3 knows the retaining wall. IFC2x3 has no predefined type on a wall.
    let o3 = IfcOptions {
        version: "IFC4X3".into(),
        ..Default::default()
    };
    assert_eq!(o3.schema(), Schema::Ifc4x3);
    let (f3, _) = export(&d, &o3);
    assert!(
        named(&f3, "IFCWALL")
            .iter()
            .any(|e| text_attr(e, 8) == Some("RETAININGWALL"))
    );
    let o2 = IfcOptions {
        version: "IFC2X3".into(),
        ..Default::default()
    };
    let (f2, _) = export(&d, &o2);
    assert!(named(&f2, "IFCWALL").iter().all(|e| e.attrs.len() == 8));
}

#[test]
fn irrigation_is_skipped_with_a_reason_that_says_so() {
    let mut ed = Editor::with_sequential_ids();
    add(
        &mut ed,
        Shape::Irrigation(Box::new(
            kadakas_core::irrigation::IrrigationShape::Controller {
                at: DVec2::ZERO,
                name: "C1".into(),
                stations: 4,
            },
        )),
    );
    let (_, report) = export(ed.doc(), &IfcOptions::default());
    let s = &report.skipped[0];
    assert_eq!(s.entity, "irrigation_controller");
    assert!(s.reason.contains("irrigation"), "{}", s.reason);
}
