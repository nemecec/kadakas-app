//! Massing models in the IFC export.

use cadektor_core::command::{Command, Editor};
use cadektor_core::curve;
use cadektor_core::georef::Georeference;
use cadektor_core::massing::{Massing, RoofStyle};
use cadektor_core::model::*;
use glam::{DVec2, DVec3};

use crate::step::{self, StepFile, Value};
use crate::{IfcOptions, export_bytes};

fn export(doc: &Document, opts: &IfcOptions) -> (String, cadektor_io::Report) {
    let (bytes, report) = export_bytes(doc, opts).unwrap();
    (String::from_utf8(bytes).unwrap(), report)
}

fn parsed(doc: &Document, opts: &IfcOptions) -> StepFile {
    step::parse(&export(doc, opts).0).unwrap()
}

fn named<'a>(f: &'a StepFile, name: &str) -> Vec<&'a step::Entity> {
    f.entities.iter().filter(|e| e.name == name).collect()
}

fn rect(x: f64, y: f64, w: f64, d: f64) -> Vec<Vertex> {
    curve::corners(&[
        DVec2::new(x, y),
        DVec2::new(x + w, y),
        DVec2::new(x + w, y + d),
        DVec2::new(x, y + d),
    ])
}

fn doc() -> (Document, ObjectId, ObjectId) {
    let mut ed = Editor::default();
    ed.execute(&Command::SetGeoreference {
        georef: Some(Georeference {
            crs: "EPSG:3067".into(),
            origin: DVec2::new(385_000.0, 6_672_000.0),
            rotation: 0.0,
            user_origin: DVec2::ZERO,
        }),
    })
    .unwrap();
    // Ground that rises 50 mm per meter toward +X, from 200 mm.
    let mut points = Vec::new();
    for i in 0..9 {
        for j in 0..9 {
            let (x, y) = (f64::from(i) * 1000.0, f64::from(j) * 1000.0);
            points.push(DVec3::new(x, y, 200.0 + x * 0.05));
        }
    }
    let add = |ed: &mut Editor, shape: Shape| {
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
    };
    add(
        &mut ed,
        Shape::SiteModel {
            points,
            lines: vec![],
            settings: Default::default(),
        },
    );
    let mut flat = Massing::new(rect(2000.0, 1000.0, 4000.0, 3000.0), 6000.0);
    flat.name = "Block".into();
    flat.floors = Some(2);
    flat.floor_height = 3000.0;
    let a = add(&mut ed, Shape::Massing(Box::new(flat)));
    let mut gable = Massing::new(rect(1000.0, 5000.0, 6000.0, 2000.0), 3000.0);
    gable.roof = RoofStyle::Gable;
    let b = add(&mut ed, Shape::Massing(Box::new(gable)));
    (ed.doc().clone(), a, b)
}

fn z_values(f: &StepFile) -> Vec<f64> {
    named(f, "IFCCARTESIANPOINT")
        .iter()
        .filter_map(|e| match &e.attrs[0] {
            Value::List(v) if v.len() == 3 => match v[2] {
                Value::Real(z) => Some(z),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

#[test]
fn massing_models_are_building_element_proxies_on_the_ground() {
    let (d, ..) = doc();
    let (text, report) = export(&d, &IfcOptions::default());
    assert!(text.is_ascii());
    let f = step::parse(&text).unwrap();
    assert!(f.dangling().is_empty());
    assert_eq!(named(&f, "IFCBUILDINGELEMENTPROXY").len(), 2);
    assert!(named(&f, "IFCBUILDING").is_empty());
    assert_eq!(report.objects["massing"], 2);
    // The flat roof is an extrusion, the gable roof a closed face set.
    assert!(!named(&f, "IFCEXTRUDEDAREASOLID").is_empty());
    assert_eq!(
        named(&f, "IFCTRIANGULATEDFACESET").len(),
        2,
        "the terrain and the gable block"
    );
    // Standard predefined type and the property sets.
    let proxies = named(&f, "IFCBUILDINGELEMENTPROXY");
    assert!(
        proxies
            .iter()
            .all(|e| matches!(&e.attrs[8], Value::Enum(s) if s == "NOTDEFINED"))
    );
    assert!(
        proxies
            .iter()
            .any(|e| matches!(&e.attrs[2], Value::Str(s) if s == "Block"))
    );
    let sets: Vec<String> = named(&f, "IFCPROPERTYSET")
        .iter()
        .filter_map(|e| match &e.attrs[2] {
            Value::Str(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    assert!(sets.contains(&"Pset_BuildingElementProxyCommon".to_string()));
    assert!(sets.contains(&"Cadektor_Massing".to_string()));
    let quantity_sets: Vec<String> = named(&f, "IFCELEMENTQUANTITY")
        .iter()
        .filter_map(|e| match &e.attrs[2] {
            Value::Str(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        quantity_sets
            .iter()
            .filter(|s| *s == "Cadektor_MassingQuantities")
            .count(),
        2
    );
    let names: Vec<String> = named(&f, "IFCQUANTITYVOLUME")
        .iter()
        .filter_map(|e| match &e.attrs[0] {
            Value::Str(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(names.iter().filter(|n| *n == "GrossVolume").count(), 2);
    // The flat block stands on the lowest ground under its footprint: x from 2000 mm, 200 + 100 mm.
    assert!(
        z_values(&f).iter().any(|z| (*z - 300.0).abs() < 1e-3),
        "{:?}",
        z_values(&f)
    );
    // The gable block starts at 1000 mm: 250 mm.
    assert!(z_values(&f).iter().any(|z| (*z - 250.0).abs() < 1e-3));
    // The same input gives the same bytes.
    assert_eq!(text, export(&d, &IfcOptions::default()).0);
}

#[test]
fn massing_as_building_and_storeys() {
    let (d, ..) = doc();
    let opts = IfcOptions {
        massing_as_building: true,
        ..Default::default()
    };
    let f = parsed(&d, &opts);
    assert!(f.dangling().is_empty());
    let buildings = named(&f, "IFCBUILDING");
    assert_eq!(buildings.len(), 2);
    assert!(named(&f, "IFCBUILDINGELEMENTPROXY").is_empty());
    assert!(
        buildings
            .iter()
            .all(|b| b.attrs.len() == 12 && matches!(&b.attrs[4], Value::Str(s) if s == "MASSING"))
    );
    // The site aggregates the buildings.
    let rel = named(&f, "IFCRELAGGREGATES");
    assert_eq!(rel.len(), 2, "project to site, site to the two buildings");
    assert!(
        rel.iter()
            .any(|r| matches!(&r.attrs[5], Value::List(v) if v.len() == 2))
    );
    let sets: Vec<String> = named(&f, "IFCELEMENTQUANTITY")
        .iter()
        .filter_map(|e| match &e.attrs[2] {
            Value::Str(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        sets.iter()
            .filter(|s| *s == "Qto_BuildingBaseQuantities")
            .count(),
        2
    );
    // With storeys for the layers, the proxies belong to the storey; the buildings stay aggregated in the site.
    let f = parsed(
        &d,
        &IfcOptions {
            layers_as_storeys: true,
            ..Default::default()
        },
    );
    assert!(f.dangling().is_empty());
    assert_eq!(named(&f, "IFCBUILDINGSTOREY").len(), 1);
    assert_eq!(named(&f, "IFCBUILDINGELEMENTPROXY").len(), 2);
    let f = parsed(
        &d,
        &IfcOptions {
            layers_as_storeys: true,
            massing_as_building: true,
            ..Default::default()
        },
    );
    assert!(f.dangling().is_empty());
    assert_eq!(
        named(&f, "IFCBUILDING").len(),
        3,
        "the storey building and two massing buildings"
    );
}

#[test]
fn bad_models_and_underlays_are_skipped_with_a_reason() {
    let (mut d, a, _) = doc();
    // A model with a broken value cannot be written; it is reported.
    if let Shape::Massing(m) = &mut d.objects.get_mut(&a).unwrap().shape {
        m.floors = Some(0);
    }
    let (_, report) = export(&d, &IfcOptions::default());
    assert!(
        report
            .skipped
            .iter()
            .any(|s| s.entity == "massing" && s.reason.contains("floors"))
    );
    assert_eq!(report.objects["massing"], 1);
    // An underlay is a picture, not a building element.
    let mut ed = Editor::new(d);
    let img = cadektor_core::underlay::Rgba {
        width: 1,
        height: 1,
        data: vec![1, 2, 3, 255],
    };
    ed.execute(&Command::AddObject {
        shape: Shape::Underlay(Box::new(cadektor_core::underlay::Underlay {
            name: "map".into(),
            min: DVec2::ZERO,
            max: DVec2::splat(1000.0),
            width: 1,
            height: 1,
            png: cadektor_core::underlay::PngData(std::sync::Arc::new(img.to_png())),
            opacity: 1.0,
            source: String::new(),
        })),
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let (_, report) = export(ed.doc(), &IfcOptions::default());
    assert!(report.skipped.iter().any(|s| s.entity == "underlay"));
}
