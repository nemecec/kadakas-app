//! Tests of the whole export: entity counts, the spatial tree, data, determinism and random documents.

use std::collections::BTreeMap;

use glam::{DVec2, DVec3};
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve;
use kadakas_core::datamap::{DataMap, EntityMap};
use kadakas_core::georef::Georeference;
use kadakas_core::hardscape::{Component, HardscapePatch, HardscapeStyle, Slope};
use kadakas_core::landscape::LandscapePatch;
use kadakas_core::landscape::style::{LandscapeAreaStyle, MixEntry};
use kadakas_core::model::*;
use kadakas_core::plants::style::{PlantCategory, PlantStyle};
use kadakas_core::plants::tree::{TreeAction, TreeCondition};
use kadakas_core::record::{FieldDef, FieldType, RecordFormat};
use kadakas_core::resource::ResourceBody;
use proptest::prelude::*;
use serde_json::{Map, json};

use crate::step::{self, StepFile, Value};
use crate::{IfcOptions, export_bytes};

fn v2(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn square(x0: f64, y0: f64, s: f64) -> Vec<Vertex> {
    curve::corners(&[
        v2(x0, y0),
        v2(x0 + s, y0),
        v2(x0 + s, y0 + s),
        v2(x0, y0 + s),
    ])
}

fn run(ed: &mut Editor, c: Command) -> Vec<kadakas_core::uid::Uid> {
    ed.execute(&c).unwrap().created
}

fn add(ed: &mut Editor, shape: Shape) -> ObjectId {
    ObjectId(
        run(
            ed,
            Command::AddObject {
                shape,
                layer: None,
                class: None,
                attrs: Attributes::default(),
            },
        )[0],
    )
}

fn site_shape() -> Shape {
    let mut points = Vec::new();
    for i in 0..9 {
        for j in 0..9 {
            let (x, y) = (i as f64 * 1000.0, j as f64 * 1000.0);
            points.push(DVec3::new(x, y, 200.0 + x * 0.05));
        }
    }
    Shape::SiteModel {
        points,
        lines: vec![],
        settings: Default::default(),
    }
}

/// A document with every kind of object that the export handles.
pub fn sample() -> Document {
    let mut ed = Editor::with_sequential_ids();
    run(
        &mut ed,
        Command::SetGeoreference {
            georef: Some(Georeference {
                crs: "EPSG:3067".into(),
                origin: v2(385000.0, 6672000.0),
                rotation: 0.05,
                user_origin: DVec2::ZERO,
            }),
        },
    );
    run(
        &mut ed,
        Command::SetProject {
            name: Some("Pähkinäpuisto 'North'".into()),
            client: Some("Client Oy".into()),
            address: Some("Street 1".into()),
            drawn_by: Some("NM".into()),
            date: Some("2026-10-10".into()),
            revision: Some("A".into()),
        },
    );
    run(
        &mut ed,
        Command::SetProjectGuid {
            guid: "1Gx3v0EYX3_fOnxLUh6gRb".into(),
        },
    );
    let planting = ClassId(
        run(
            &mut ed,
            Command::AddClass {
                name: "Planting".into(),
                attrs: Attributes {
                    fill: Some(Color([30, 160, 60, 255])),
                    ..Default::default()
                },
            },
        )[0],
    );
    let paving = ClassId(
        run(
            &mut ed,
            Command::AddClass {
                name: "Paving".into(),
                attrs: Attributes::default(),
            },
        )[0],
    );
    add(&mut ed, site_shape());

    let oak = kadakas_core::resource::ResourceId(
        run(
            &mut ed,
            Command::AddResource {
                name: "Oak".into(),
                body: ResourceBody::PlantStyle(Box::new(PlantStyle {
                    botanical_name: "Quercus robur".into(),
                    common_name: "Oak".into(),
                    id_code: "QR".into(),
                    category: PlantCategory::Tree,
                    spread: 6000.0,
                    height: 12000.0,
                    trunk_diameter: 400.0,
                    price: 120.0,
                    ..Default::default()
                })),
            },
        )[0],
    );
    let shrub = kadakas_core::resource::ResourceId(
        run(
            &mut ed,
            Command::AddResource {
                name: "Spirea".into(),
                body: ResourceBody::PlantStyle(Box::new(PlantStyle {
                    botanical_name: "Spiraea japonica".into(),
                    id_code: "SJ".into(),
                    category: PlantCategory::Shrub,
                    spread: 800.0,
                    height: 900.0,
                    ..Default::default()
                })),
            },
        )[0],
    );
    run(
        &mut ed,
        Command::AddPlants {
            style: Some(oak),
            overrides: Map::new(),
            positions: vec![v2(2000.0, 2000.0), v2(5000.0, 5000.0)],
            layer: None,
            class: Some(planting),
            variation: None,
        },
    );
    run(
        &mut ed,
        Command::AddPlants {
            style: Some(shrub),
            overrides: Map::new(),
            positions: vec![v2(3000.0, 1000.0)],
            layer: None,
            class: Some(planting),
            variation: None,
        },
    );
    add(
        &mut ed,
        Shape::ExistingTree {
            at: v2(7000.0, 1000.0),
            species: "Betula".into(),
            dbh: 300.0,
            spread: 5000.0,
            height: 11000.0,
            condition: TreeCondition::Fair,
            action: TreeAction::Retain,
            rpz_factor: None,
            rpz_cap: None,
        },
    );

    let hs = kadakas_core::resource::ResourceId(
        run(
            &mut ed,
            Command::AddResource {
                name: "Granite paving".into(),
                body: ResourceBody::HardscapeStyle(Box::new(HardscapeStyle {
                    material: "Granite".into(),
                    fill: Some(Color([190, 190, 180, 255])),
                    thickness: 60.0,
                    components: vec![
                        Component {
                            name: "Bedding".into(),
                            material: "Sand".into(),
                            thickness: 40.0,
                            color: None,
                        },
                        Component {
                            name: "Base".into(),
                            material: "Crushed stone".into(),
                            thickness: 200.0,
                            color: None,
                        },
                    ],
                    ..Default::default()
                })),
            },
        )[0],
    );
    let patch = |f: &dyn Fn(&mut HardscapePatch)| {
        let mut p = HardscapePatch {
            style: Some(hs),
            ..Default::default()
        };
        f(&mut p);
        Box::new(p)
    };
    let hard = |ed: &mut Editor, b: Vec<Vertex>, p: Box<HardscapePatch>| {
        run(
            ed,
            Command::AddHardscape {
                boundary: Some(b),
                patch: p,
                layer: None,
                class: Some(paving),
                attrs: Attributes::default(),
            },
        )
    };
    hard(&mut ed, square(500.0, 500.0, 1500.0), patch(&|_| {}));
    hard(
        &mut ed,
        square(3000.0, 3000.0, 1500.0),
        patch(&|p| {
            p.slope = Some(Slope::Cross {
                percent: 2.0,
                angle_deg: 0.0,
            })
        }),
    );
    hard(
        &mut ed,
        square(5000.0, 500.0, 1500.0),
        patch(&|p| p.drape = Some(true)),
    );
    run(
        &mut ed,
        Command::AddHardscape {
            boundary: Some(square(6500.0, 6500.0, 1000.0)),
            patch: Box::new(HardscapePatch {
                material: Some("Concrete".into()),
                thickness: Some(120.0),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        },
    );
    run(
        &mut ed,
        Command::AddHardscape {
            boundary: None,
            patch: Box::new(HardscapePatch {
                style: Some(hs),
                mode: Some(kadakas_core::hardscape::Mode::Path),
                center: Some(curve::corners(&[v2(1000.0, 7000.0), v2(5000.0, 7500.0)])),
                width: Some(1200.0),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        },
    );

    let ls = kadakas_core::resource::ResourceId(
        run(
            &mut ed,
            Command::AddResource {
                name: "Border planting".into(),
                body: ResourceBody::LandscapeAreaStyle(Box::new(LandscapeAreaStyle {
                    mix: vec![MixEntry {
                        plant: shrub,
                        percent: 100.0,
                        ..Default::default()
                    }],
                    ..Default::default()
                })),
            },
        )[0],
    );
    run(
        &mut ed,
        Command::AddLandscapeArea {
            boundary: square(1000.0, 3000.0, 1500.0),
            name: "Bed A".into(),
            patch: Box::new(LandscapePatch {
                style: Some(ls),
                holes: Some(vec![square(1500.0, 3500.0, 300.0)]),
                ..Default::default()
            }),
            layer: None,
            class: Some(planting),
            attrs: Attributes::default(),
        },
    );
    run(
        &mut ed,
        Command::AddLandscapeArea {
            boundary: square(5500.0, 3500.0, 1000.0),
            name: "Draped lawn".into(),
            patch: Box::new(LandscapePatch {
                drape: Some(true),
                holes: Some(vec![square(5800.0, 3800.0, 200.0)]),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        },
    );

    add(
        &mut ed,
        Shape::Line {
            a: v2(0.0, 0.0),
            b: v2(1000.0, 1000.0),
        },
    );
    add(
        &mut ed,
        Shape::Text {
            at: v2(100.0, 100.0),
            text: "Käyttö 'A' \\ \u{1F333}".into(),
            height: 250.0,
        },
    );
    add(
        &mut ed,
        Shape::Rect {
            min: v2(0.0, 0.0),
            max: v2(100.0, 100.0),
        },
    );
    add(
        &mut ed,
        Shape::Circle {
            center: v2(0.0, 0.0),
            radius: 500.0,
        },
    );
    add(
        &mut ed,
        Shape::Arc {
            center: v2(0.0, 0.0),
            radius: 500.0,
            start: 0.0,
            end: 1.5,
        },
    );
    add(
        &mut ed,
        Shape::Polyline {
            points: curve::corners(&[v2(0.0, 0.0), v2(100.0, 0.0), v2(100.0, 100.0)]),
            closed: true,
        },
    );
    add(
        &mut ed,
        Shape::Locus3d {
            p: DVec3::new(10.0, 20.0, 30.0),
        },
    );
    add(
        &mut ed,
        Shape::Polyline3d {
            points: vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(100.0, 0.0, 10.0)],
            closed: true,
        },
    );

    // A record on the first plant, and a data map.
    let rec = kadakas_core::resource::ResourceId(
        run(
            &mut ed,
            Command::AddResource {
                name: "Soil".into(),
                body: ResourceBody::RecordFormat(Box::new(RecordFormat {
                    fields: vec![
                        FieldDef::new("Type", FieldType::Text),
                        FieldDef::new("Depth", FieldType::Number),
                    ],
                })),
            },
        )[0],
    );
    let first_plant = ed
        .doc()
        .objects
        .values()
        .find(|o| matches!(o.shape, Shape::Plant { .. }))
        .unwrap()
        .id;
    run(
        &mut ed,
        Command::AttachRecord {
            ids: vec![first_plant],
            record: rec,
            values: json!({"Type": "Clay", "Depth": 450.5})
                .as_object()
                .unwrap()
                .clone(),
        },
    );
    ed.doc().clone()
}

/// Set KADAKAS_IFC_DUMP=/some/folder to keep the sample files, for example to check them with IfcOpenShell.
fn dump(name: &str, text: &str) {
    if let Some(dir) = std::env::var_os("KADAKAS_IFC_DUMP") {
        std::fs::write(std::path::Path::new(&dir).join(name), text).unwrap();
    }
}

fn export(doc: &Document, opts: &IfcOptions) -> (String, kadakas_io::Report) {
    let (bytes, report) = export_bytes(doc, opts).unwrap();
    (String::from_utf8(bytes).unwrap(), report)
}

fn parsed(doc: &Document, opts: &IfcOptions) -> StepFile {
    step::parse(&export(doc, opts).0).unwrap()
}

fn count(f: &StepFile) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for e in &f.entities {
        *m.entry(e.name.clone()).or_insert(0) += 1;
    }
    m
}

fn named<'a>(f: &'a StepFile, name: &str) -> Vec<&'a step::Entity> {
    f.entities.iter().filter(|e| e.name == name).collect()
}

fn str_of(v: &Value) -> &str {
    if let Value::Str(s) = v { s } else { "" }
}

#[test]
fn the_sample_exports_a_complete_valid_file() {
    let doc = sample();
    let (text, report) = export(&doc, &IfcOptions::default());
    dump("sample.ifc", &text);
    dump(
        "sample_storeys.ifc",
        &export(
            &doc,
            &IfcOptions {
                layers_as_storeys: true,
                include_2d: true,
                ..Default::default()
            },
        )
        .0,
    );
    assert!(text.is_ascii());
    let f = step::parse(&text).unwrap();
    assert_eq!(f.schemas(), vec!["IFC4".to_string()]);
    assert!(f.dangling().is_empty());
    let ids: std::collections::BTreeSet<u64> = f.entities.iter().map(|e| e.id).collect();
    assert_eq!(ids.len(), f.entities.len());
    let c = count(&f);
    assert_eq!(c["IFCPROJECT"], 1);
    assert_eq!(c["IFCSITE"], 1);
    assert!(!c.contains_key("IFCBUILDING"));
    // 4 slabs and 1 path.
    assert_eq!(c["IFCSLAB"], 5);
    // 2 oaks, 1 shrub, 1 tree, 2 landscape areas, 1 terrain.
    assert_eq!(c["IFCGEOGRAPHICELEMENT"], 7);
    assert_eq!(c["IFCMAPCONVERSION"], 1);
    assert_eq!(c["IFCPROJECTEDCRS"], 1);
    assert!(c["IFCTRIANGULATEDFACESET"] >= 4);
    assert!(c["IFCEXTRUDEDAREASOLID"] >= 10);
    assert!(c["IFCARBITRARYPROFILEDEFWITHVOIDS"] == 1);
    assert!(c["IFCBOUNDINGBOX"] == 4);
    assert!(c["IFCMATERIALLAYERSETUSAGE"] >= 1 && c["IFCMATERIALLAYERSET"] >= 1);
    assert_eq!(c["IFCPRESENTATIONLAYERASSIGNMENT"], 2);
    assert!(c["IFCSTYLEDITEM"] >= 10);
    assert!(!c.contains_key("IFCANNOTATION"));
    // The report counts what was written and lists what was skipped.
    assert_eq!(report.objects["hardscape"], 5);
    assert_eq!(report.objects["plant"], 3);
    assert_eq!(report.objects["site_model"], 1);
    assert!(report.georeferenced);
    for kind in ["line", "text", "rect", "circle", "arc", "polyline"] {
        assert!(report.skipped.iter().any(|s| s.entity == kind), "{kind}");
    }
    assert!(
        report
            .skipped
            .iter()
            .any(|s| s.entity == "locus3d" && s.reason.contains("survey"))
    );
}

#[test]
fn vegetation_is_a_user_defined_geographic_element() {
    let f = parsed(&sample(), &IfcOptions::default());
    let geo = named(&f, "IFCGEOGRAPHICELEMENT");
    let terrain = geo
        .iter()
        .filter(|e| e.attrs[8] == Value::e("TERRAIN"))
        .count();
    assert_eq!(terrain, 1);
    let plants: Vec<_> = geo
        .iter()
        .filter(|e| e.attrs[8] == Value::e("USERDEFINED"))
        .collect();
    assert_eq!(plants.len(), 6);
    assert!(plants.iter().all(|e| e.attrs[4] == Value::s("VEGETATION")));
    // Every enumeration literal is one that IFC4 knows.
    let slabs = named(&f, "IFCSLAB");
    assert!(slabs.iter().all(|e| e.attrs[8] == Value::e("BASESLAB")));
}

#[test]
fn project_and_site_carry_the_document_data() {
    let f = parsed(&sample(), &IfcOptions::default());
    let project = &named(&f, "IFCPROJECT")[0].attrs;
    assert_eq!(str_of(&project[2]), "Pähkinäpuisto 'North'");
    assert_eq!(str_of(&project[5]), "Client Oy");
    let units = f.get(project[8].as_ref_id().unwrap()).unwrap();
    assert_eq!(units.name, "IFCUNITASSIGNMENT");
    let unit_names: Vec<String> = match &units.attrs[0] {
        Value::List(l) => l
            .iter()
            .map(|r| format!("{:?}", f.get(r.as_ref_id().unwrap()).unwrap().attrs))
            .collect(),
        _ => vec![],
    };
    assert!(unit_names.iter().any(|u| u.contains("MILLI")));
    assert!(unit_names.iter().any(|u| u.contains("SQUARE_METRE")));
    let site = &named(&f, "IFCSITE")[0].attrs;
    // Latitude and longitude are compound angles.
    let Value::List(lat) = &site[9] else {
        panic!("no latitude")
    };
    assert_eq!(lat.len(), 4);
    assert!(matches!(lat[0], Value::Int(60)));
    let Value::List(lon) = &site[10] else {
        panic!("no longitude")
    };
    assert!(matches!(lon[0], Value::Int(24)));
    let conv = &named(&f, "IFCMAPCONVERSION")[0].attrs;
    assert_eq!(conv[2], Value::Real(385000.0));
    assert_eq!(conv[3], Value::Real(6672000.0));
    assert_eq!(conv[7], Value::Real(0.001));
    let crs = &named(&f, "IFCPROJECTEDCRS")[0].attrs;
    assert_eq!(str_of(&crs[0]), "EPSG:3067");
    // The map conversion links the model context to the CRS.
    assert_eq!(
        f.get(conv[0].as_ref_id().unwrap()).unwrap().name,
        "IFCGEOMETRICREPRESENTATIONCONTEXT"
    );
    assert_eq!(
        conv[1].as_ref_id(),
        Some(named(&f, "IFCPROJECTEDCRS")[0].id)
    );
    // True north follows the rotation of the model.
    let ctx = named(&f, "IFCGEOMETRICREPRESENTATIONCONTEXT")[0];
    let north = f.get(ctx.attrs[5].as_ref_id().unwrap()).unwrap();
    let Value::List(d) = &north.attrs[0] else {
        panic!()
    };
    let (Value::Real(x), Value::Real(y)) = (&d[0], &d[1]) else {
        panic!()
    };
    // Grid north turned by the model rotation, plus the convergence of about 1.8 degrees.
    let angle = x.atan2(*y).to_degrees();
    assert!(
        (angle - (0.05f64.to_degrees() + 1.8)).abs() < 0.4,
        "{angle}"
    );
}

#[test]
fn all_elements_are_in_the_site_and_the_tree_is_complete() {
    let f = parsed(&sample(), &IfcOptions::default());
    let site = named(&f, "IFCSITE")[0].id;
    let rels = named(&f, "IFCRELCONTAINEDINSPATIALSTRUCTURE");
    assert_eq!(rels.len(), 1);
    assert_eq!(rels[0].attrs[5].as_ref_id(), Some(site));
    let Value::List(items) = &rels[0].attrs[4] else {
        panic!()
    };
    // 5 slabs, 7 geographic elements.
    assert_eq!(items.len(), 12);
    let agg = named(&f, "IFCRELAGGREGATES");
    assert_eq!(agg.len(), 1);
    assert_eq!(
        agg[0].attrs[4].as_ref_id(),
        Some(named(&f, "IFCPROJECT")[0].id)
    );
    assert_eq!(agg[0].attrs[5], Value::refs([site]));
    // Every element has a placement, a shape and a tag with the object ID.
    for e in f
        .entities
        .iter()
        .filter(|e| e.name == "IFCSLAB" || e.name == "IFCGEOGRAPHICELEMENT")
    {
        assert!(e.attrs[5].as_ref_id().is_some());
        assert!(e.attrs[6].as_ref_id().is_some());
        assert!(kadakas_core::uid::Uid::parse(str_of(&e.attrs[7])).is_some());
        assert!(matches!(e.attrs[8], Value::Enum(_)));
    }
}

#[test]
fn layers_can_become_storeys() {
    let mut ed = Editor::new(sample());
    let upper = LayerId(
        run(
            &mut ed,
            Command::AddLayer {
                name: "Upper".into(),
                elevation: 3000.0,
                scale: None,
                kind: None,
            },
        )[0],
    );
    ed.execute(&Command::AddObject {
        shape: Shape::Plant {
            at: v2(100.0, 100.0),
            species: "Tree".into(),
            spread: 2000.0,
            height: 5000.0,
        },
        layer: Some(upper),
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let opts = IfcOptions {
        layers_as_storeys: true,
        ..Default::default()
    };
    let f = parsed(ed.doc(), &opts);
    let c = count(&f);
    assert_eq!(c["IFCBUILDING"], 1);
    assert_eq!(c["IFCBUILDINGSTOREY"], 2);
    let storeys = named(&f, "IFCBUILDINGSTOREY");
    let elevations: Vec<&Value> = storeys.iter().map(|s| &s.attrs[9]).collect();
    assert!(elevations.contains(&&Value::Real(0.0)) && elevations.contains(&&Value::Real(3000.0)));
    assert_eq!(named(&f, "IFCRELCONTAINEDINSPATIALSTRUCTURE").len(), 2);
    assert_eq!(named(&f, "IFCRELAGGREGATES").len(), 3);
    assert!(f.dangling().is_empty());
    // Without the option, the elevation sits in the placements.
    let flat = parsed(ed.doc(), &IfcOptions::default());
    assert!(!count(&flat).contains_key("IFCBUILDINGSTOREY"));
}

#[test]
fn classes_as_layers_can_be_turned_off() {
    let doc = sample();
    let with = parsed(&doc, &IfcOptions::default());
    let layers = named(&with, "IFCPRESENTATIONLAYERASSIGNMENT");
    let names: Vec<&str> = layers.iter().map(|l| str_of(&l.attrs[0])).collect();
    assert_eq!(names, vec!["Planting", "Paving"]);
    for l in &layers {
        let Value::List(items) = &l.attrs[2] else {
            panic!()
        };
        assert!(items.iter().all(|r| with.get(r.as_ref_id().unwrap()).unwrap().name == "IFCSHAPEREPRESENTATION"));
    }
    let without = parsed(
        &doc,
        &IfcOptions {
            classes_as_layers: false,
            ..Default::default()
        },
    );
    assert!(!count(&without).contains_key("IFCPRESENTATIONLAYERASSIGNMENT"));
}

#[test]
fn two_d_objects_become_annotations_on_request() {
    let doc = sample();
    let (text, report) = export(
        &doc,
        &IfcOptions {
            include_2d: true,
            ..Default::default()
        },
    );
    let f = step::parse(&text).unwrap();
    assert!(f.dangling().is_empty());
    assert_eq!(count(&f)["IFCANNOTATION"], 8);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(count(&f)["IFCTEXTLITERAL"], 1);
    // Text with quotes, a backslash, accents and an emoji survives.
    let literal = named(&f, "IFCTEXTLITERAL")[0];
    assert_eq!(str_of(&literal.attrs[0]), "Käyttö 'A' \\ \u{1F333}");
    let types: Vec<&str> = named(&f, "IFCSHAPEREPRESENTATION")
        .iter()
        .map(|r| str_of(&r.attrs[2]))
        .collect();
    for t in ["Curve2D", "Annotation2D", "Point", "Curve3D"] {
        assert!(types.contains(&t), "{t}");
    }
}

#[test]
fn guids_are_stable_and_follow_object_ids() {
    let doc = sample();
    let a = export(&doc, &IfcOptions::default()).0;
    let b = export(&doc, &IfcOptions::default()).0;
    assert_eq!(a, b);
    let f = step::parse(&a).unwrap();
    let guids: Vec<&str> = f
        .entities
        .iter()
        .filter(|e| {
            matches!(e.attrs.first(), Some(Value::Str(s)) if crate::guid::is_valid(s))
                && e.attrs.len() > 3
                && e.name != "IFCCOLOURRGB"
        })
        .map(|e| str_of(&e.attrs[0]))
        .collect();
    assert!(guids.len() > 30);
    let unique: std::collections::BTreeSet<&&str> = guids.iter().collect();
    assert_eq!(unique.len(), guids.len(), "GlobalIds are unique");
    // Another export after a change keeps the GlobalId of an untouched object.
    let mut ed = Editor::new(doc.clone());
    ed.execute(&Command::AddObject {
        shape: Shape::Plant {
            at: v2(9.0, 9.0),
            species: "x".into(),
            spread: 500.0,
            height: 500.0,
        },
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let g = parsed(ed.doc(), &IfcOptions::default());
    let tag_guid = |f: &StepFile, tag: &str| {
        f.entities
            .iter()
            .find(|e| e.name == "IFCSLAB" && str_of(&e.attrs[7]) == tag)
            .map(|e| str_of(&e.attrs[0]).to_string())
    };
    let slab = doc
        .objects
        .values()
        .find(|o| matches!(o.shape, Shape::Hardscape { .. }))
        .unwrap()
        .id
        .0
        .to_string();
    assert!(tag_guid(&f, &slab).is_some());
    assert_eq!(tag_guid(&f, &slab), tag_guid(&g, &slab));
    // The GlobalId of an object is its ID in the IFC alphabet: another project GUID leaves it.
    let id = doc
        .objects
        .values()
        .find(|o| matches!(o.shape, Shape::Hardscape { .. }))
        .unwrap()
        .id;
    assert_eq!(
        tag_guid(&f, &slab),
        Some(crate::guid::encode(id.0.as_u128()))
    );
    let mut other = Editor::new(doc);
    other
        .execute(&Command::SetProjectGuid {
            guid: "3Gx3v0EYX3_fOnxLUh6gRb".into(),
        })
        .unwrap();
    let h = parsed(other.doc(), &IfcOptions::default());
    assert_eq!(tag_guid(&f, &slab), tag_guid(&h, &slab));
    // The GUIDs of what belongs to the project (the site, the storeys, the relations) follow it.
    let site_guid = |f: &StepFile| {
        f.entities
            .iter()
            .find(|e| e.name == "IFCSITE")
            .map(|e| str_of(&e.attrs[0]).to_string())
    };
    assert_ne!(site_guid(&f), site_guid(&h));
}

#[test]
fn the_project_guid_command_is_checked_and_undoable() {
    let mut ed = Editor::new(sample());
    assert!(
        ed.execute(&Command::SetProjectGuid {
            guid: "nope".into()
        })
        .is_err()
    );
    ed.execute(&Command::SetProjectGuid {
        guid: "2Gx3v0EYX3_fOnxLUh6gRb".into(),
    })
    .unwrap();
    assert_eq!(ed.doc().drawing.project.guid, "2Gx3v0EYX3_fOnxLUh6gRb");
    assert!(ed.undo());
    assert_eq!(ed.doc().drawing.project.guid, "1Gx3v0EYX3_fOnxLUh6gRb");
}

#[test]
fn properties_quantities_and_records_are_written() {
    let doc = sample();
    let f = parsed(&doc, &IfcOptions::default());
    let sets: Vec<String> = named(&f, "IFCPROPERTYSET")
        .iter()
        .map(|s| str_of(&s.attrs[2]).to_string())
        .collect();
    for expected in [
        "Pset_SlabCommon",
        "Kadakas_Source",
        "Kadakas_Planting",
        "Kadakas_Hardscape",
        "Kadakas_Quantities",
        "Kadakas_Tree",
        "Kadakas_PlantMix",
        "ePset_Soil",
    ] {
        assert!(
            sets.contains(&expected.to_string()),
            "{expected} in {sets:?}"
        );
    }
    let qtos: Vec<String> = named(&f, "IFCELEMENTQUANTITY")
        .iter()
        .map(|s| str_of(&s.attrs[2]).to_string())
        .collect();
    for expected in [
        "Qto_SlabBaseQuantities",
        "Kadakas_HardscapeQuantities",
        "Kadakas_AreaQuantities",
        "Kadakas_TerrainQuantities",
    ] {
        assert!(
            qtos.contains(&expected.to_string()),
            "{expected} in {qtos:?}"
        );
    }
    // The record values arrive with their types.
    let single: Vec<&step::Entity> = named(&f, "IFCPROPERTYSINGLEVALUE");
    let find = |name: &str| {
        single
            .iter()
            .find(|p| str_of(&p.attrs[0]) == name)
            .map(|p| p.attrs[2].clone())
    };
    assert_eq!(
        find("Type"),
        Some(Value::typed("IFCLABEL", Value::s("Clay")))
    );
    assert_eq!(
        find("Depth"),
        Some(Value::typed("IFCREAL", Value::Real(450.5)))
    );
    assert_eq!(
        find("BotanicalName"),
        Some(Value::typed("IFCLABEL", Value::s("Quercus robur")))
    );
    assert_eq!(
        find("IsExternal"),
        Some(Value::typed("IFCBOOLEAN", Value::Bool(true)))
    );
    assert!(
        find("PitchAngle").is_some(),
        "the sloped slab has a pitch angle"
    );
    // Every property set is linked to an element by a relationship.
    assert_eq!(
        named(&f, "IFCRELDEFINESBYPROPERTIES").len(),
        sets.len() + qtos.len()
    );
    // The material layers of the styled slab: surface plus two components.
    let layers = named(&f, "IFCMATERIALLAYERSET");
    assert!(
        layers
            .iter()
            .any(|l| matches!(&l.attrs[0], Value::List(x) if x.len() == 3))
    );
    assert!(
        layers
            .iter()
            .any(|l| matches!(&l.attrs[0], Value::List(x) if x.len() == 1))
    );
    let materials: Vec<&str> = named(&f, "IFCMATERIAL")
        .iter()
        .map(|m| str_of(&m.attrs[0]))
        .collect();
    for m in ["Granite paving", "Sand", "Crushed stone", "Concrete"] {
        assert!(
            materials.iter().any(|x| x.contains(m) || m.contains(x)),
            "{m} in {materials:?}"
        );
    }
    assert_eq!(named(&f, "IFCRELASSOCIATESMATERIAL").len(), layers.len());
}

#[test]
fn the_data_map_of_the_document_decides_classes() {
    let mut ed = Editor::new(sample());
    let mut map = DataMap::landscape_template();
    // IfcPavement is not in IFC4: the writer falls back and says so.
    map.entities = vec![
        EntityMap {
            kind: "hardscape".into(),
            ifc_class: "IfcPavement".into(),
            predefined_type: None,
        },
        EntityMap {
            kind: "landscape_area".into(),
            ifc_class: "IfcCovering".into(),
            predefined_type: Some("FLOORING".into()),
        },
        EntityMap {
            kind: "plant".into(),
            ifc_class: "IfcBuildingElementProxy".into(),
            predefined_type: Some("NOTHING".into()),
        },
        EntityMap {
            kind: "existing_tree".into(),
            ifc_class: "IfcGeographicElement".into(),
            predefined_type: Some("userdefined".into()),
        },
    ];
    run(
        &mut ed,
        Command::AddResource {
            name: "Mapping".into(),
            body: ResourceBody::DataMap(Box::new(map)),
        },
    );
    let (text, report) = export(ed.doc(), &IfcOptions::default());
    let f = step::parse(&text).unwrap();
    let c = count(&f);
    assert_eq!(c["IFCSLAB"], 5);
    assert_eq!(c["IFCCOVERING"], 2);
    assert_eq!(c["IFCBUILDINGELEMENTPROXY"], 3);
    assert!(report.notes.iter().any(|n| n.contains("IfcPavement")));
    assert!(report.notes.iter().any(|n| n.contains("NOTHING")));
    let covering = named(&f, "IFCCOVERING")[0];
    assert_eq!(covering.attrs[8], Value::e("FLOORING"));
    let tree = f
        .entities
        .iter()
        .find(|e| e.name == "IFCGEOGRAPHICELEMENT" && e.attrs[8] == Value::e("USERDEFINED"));
    assert!(tree.is_some());
    // A named map, and a name that does not exist.
    let named_map = IfcOptions {
        data_map: Some("mapping".into()),
        ..Default::default()
    };
    assert!(export_bytes(ed.doc(), &named_map).is_ok());
    let missing = IfcOptions {
        data_map: Some("nothing".into()),
        ..Default::default()
    };
    assert!(matches!(
        export_bytes(ed.doc(), &missing),
        Err(crate::IfcError::Option(_))
    ));

    // Covering for hardscapes uses the covering sets.
    let mut map = DataMap::landscape_template();
    map.entities = vec![EntityMap {
        kind: "hardscape".into(),
        ifc_class: "IFCCOVERING".into(),
        predefined_type: Some("FLOORING".into()),
    }];
    let mut ed = Editor::new(sample());
    run(
        &mut ed,
        Command::AddResource {
            name: "Cov".into(),
            body: ResourceBody::DataMap(Box::new(map)),
        },
    );
    let f = parsed(ed.doc(), &IfcOptions::default());
    assert_eq!(count(&f)["IFCCOVERING"], 5);
    let sets: Vec<String> = named(&f, "IFCPROPERTYSET")
        .iter()
        .map(|s| str_of(&s.attrs[2]).to_string())
        .collect();
    assert!(sets.contains(&"Pset_CoveringCommon".to_string()));
    let qtos: Vec<String> = named(&f, "IFCELEMENTQUANTITY")
        .iter()
        .map(|s| str_of(&s.attrs[2]).to_string())
        .collect();
    assert!(qtos.contains(&"Qto_CoveringBaseQuantities".to_string()));
}

#[test]
fn hidden_objects_and_sheet_layers_are_skipped() {
    let mut ed = Editor::new(sample());
    let class = ed
        .doc()
        .classes
        .iter()
        .find(|c| c.name == "Planting")
        .unwrap()
        .id;
    ed.execute(&Command::ModifyClass {
        id: class,
        name: None,
        visible: Some(false),
        attrs: None,
    })
    .unwrap();
    let sheet = LayerId(
        run(
            &mut ed,
            Command::AddSheet {
                name: "A1".into(),
                paper: None,
                orientation: None,
                number: None,
                title: None,
            },
        )[0],
    );
    ed.execute(&Command::AddObject {
        shape: Shape::Line {
            a: v2(0.0, 0.0),
            b: v2(10.0, 10.0),
        },
        layer: Some(sheet),
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let (text, report) = export(
        ed.doc(),
        &IfcOptions {
            visible_only: true,
            ..Default::default()
        },
    );
    assert!(step::parse(&text).unwrap().dangling().is_empty());
    assert!(
        report
            .skipped
            .iter()
            .any(|s| s.reason == "hidden" && s.count >= 3)
    );
    assert!(
        report
            .skipped
            .iter()
            .any(|s| s.reason.contains("sheet layer"))
    );
    // Without visible_only, the hidden objects are written.
    let all = export(ed.doc(), &IfcOptions::default()).1;
    assert!(all.skipped.iter().all(|s| s.reason != "hidden"));
}

#[test]
fn degenerate_objects_are_skipped_with_a_reason() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    // A site model with too few points makes no surface.
    add(
        &mut ed,
        Shape::SiteModel {
            points: vec![DVec3::ZERO],
            lines: vec![],
            settings: Default::default(),
        },
    );
    // Collinear boundary, and a plant with no size.
    add(
        &mut ed,
        Shape::LandscapeArea {
            boundary: curve::corners(&[v2(0.0, 0.0), v2(10.0, 0.0), v2(20.0, 0.0)]),
            name: "flat".into(),
        },
    );
    add(
        &mut ed,
        Shape::Hardscape {
            boundary: curve::corners(&[v2(0.0, 0.0), v2(10.0, 0.0), v2(20.0, 0.0)]),
            material: "m".into(),
            thickness: None,
            elevation: None,
        },
    );
    add(
        &mut ed,
        Shape::Plant {
            at: v2(0.0, 0.0),
            species: "tiny".into(),
            spread: 5.0,
            height: 5.0,
        },
    );
    add(
        &mut ed,
        Shape::SiteModifier {
            modifier: kadakas_core::model::ModifierKind::Pad {
                elevation: 0.0,
                slope: None,
            },
            boundary: square(0.0, 0.0, 10.0),
        },
    );
    // A texture bed has no IFC form: IFC has no textures.
    add(
        &mut ed,
        Shape::SiteModifier {
            modifier: kadakas_core::model::ModifierKind::TextureBed {
                texture: None,
                mapping: None,
            },
            boundary: square(0.0, 0.0, 10.0),
        },
    );
    let (text, report) = export(ed.doc(), &IfcOptions::default());
    let f = step::parse(&text).unwrap();
    assert!(f.dangling().is_empty());
    assert!(
        report
            .skipped
            .iter()
            .any(|s| s.reason.starts_with("texture bed"))
    );
    assert_eq!(count(&f)["IFCGEOGRAPHICELEMENT"], 1, "only the plant");
    assert!(
        report
            .skipped
            .iter()
            .any(|s| s.entity == "site_model" && s.reason.starts_with("no surface"))
    );
    assert!(
        report
            .skipped
            .iter()
            .filter(|s| s.reason.contains("no area"))
            .count()
            == 2
    );
    assert!(report.skipped.iter().any(|s| s.entity == "site_modifier"));
    assert!(report.notes.iter().any(|n| n.contains("no size")));
    // Only the bounding box is written for the plant.
    assert_eq!(count(&f)["IFCBOUNDINGBOX"], 1);
    assert!(!count(&f).contains_key("IFCEXTRUDEDAREASOLID"));
    // A document without georeference has no map conversion.
    assert!(!count(&f).contains_key("IFCMAPCONVERSION"));
    assert!(!report.georeferenced);
}

#[test]
fn georeference_without_a_crs_is_noted() {
    let mut ed = Editor::new(Document::with_sequential_ids());
    run(
        &mut ed,
        Command::SetGeoreference {
            georef: Some(Georeference {
                crs: String::new(),
                origin: v2(5.0, 6.0),
                rotation: 0.0,
                user_origin: DVec2::ZERO,
            }),
        },
    );
    let (text, report) = export(ed.doc(), &IfcOptions::default());
    assert!(!text.contains("IFCMAPCONVERSION"));
    assert!(report.notes.iter().any(|n| n.contains("no CRS")));
    run(
        &mut ed,
        Command::SetGeoreference {
            georef: Some(Georeference {
                crs: "WEIRD".into(),
                origin: v2(5.0, 6.0),
                rotation: 0.0,
                user_origin: DVec2::ZERO,
            }),
        },
    );
    let (text, report) = export(ed.doc(), &IfcOptions::default());
    assert!(text.contains("IFCMAPCONVERSION") && text.contains("'Custom','WEIRD'"));
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("latitude and longitude"))
    );
}

#[test]
fn files_are_written_atomically() {
    let dir = std::env::temp_dir().join(format!("kadakas-ifc-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("out.ifc");
    let doc = sample();
    let report = crate::export_file(&path, &doc, &IfcOptions::default()).unwrap();
    assert!(report.total() > 10);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("FILE_NAME('out.ifc','2026-10-10T00:00:00',('NM'),('Client Oy')"));
    assert_eq!(
        text,
        export(
            &doc,
            &IfcOptions {
                file_name: Some("out.ifc".into()),
                ..Default::default()
            }
        )
        .0
    );
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
    // A folder that does not exist is an error and leaves nothing behind.
    let bad = dir.join("missing").join("x.ifc");
    assert!(matches!(
        crate::export_file(&bad, &doc, &IfcOptions::default()),
        Err(crate::IfcError::Io(_))
    ));
    // Bad options fail before anything is written.
    let bad_opts = IfcOptions {
        version: "IFC5".into(),
        ..Default::default()
    };
    assert!(crate::export_file(&dir.join("b.ifc"), &doc, &bad_opts).is_err());
    assert!(!dir.join("b.ifc").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn io_errors_carry_the_message() {
    struct Failing;
    impl std::io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let err = crate::export_to(&sample(), &IfcOptions::default(), Failing)
        .err()
        .unwrap();
    assert_eq!(err, crate::IfcError::Io("disk full".into()));
    assert_eq!(err.to_string(), "disk full");
}

#[test]
fn terrain_faces_point_up_and_cover_the_site() {
    let f = parsed(&sample(), &IfcOptions::default());
    let sets = named(&f, "IFCTRIANGULATEDFACESET");
    // The terrain is the one with the most triangles from a 9 x 9 grid: 128 triangles.
    let terrain = sets
        .iter()
        .find(|s| matches!(&s.attrs[3], Value::List(t) if t.len() == 128))
        .expect("terrain mesh");
    let pts = f.get(terrain.attrs[0].as_ref_id().unwrap()).unwrap();
    let Value::List(coords) = &pts.attrs[0] else {
        panic!()
    };
    assert_eq!(coords.len(), 81);
    let xyz = |i: i64| -> [f64; 3] {
        let Value::List(c) = &coords[(i - 1) as usize] else {
            panic!()
        };
        [0, 1, 2].map(|k| if let Value::Real(r) = c[k] { r } else { 0.0 })
    };
    let Value::List(tris) = &terrain.attrs[3] else {
        panic!()
    };
    for t in tris {
        let Value::List(ix) = t else { panic!() };
        let ids: Vec<i64> = ix
            .iter()
            .map(|v| if let Value::Int(i) = v { *i } else { 0 })
            .collect();
        let (a, b, c) = (xyz(ids[0]), xyz(ids[1]), xyz(ids[2]));
        let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        assert!(cross > 0.0, "counterclockwise seen from above");
    }
}

#[test]
fn draped_and_flat_slabs_use_the_right_bodies() {
    let f = parsed(&sample(), &IfcOptions::default());
    let reps: Vec<String> = named(&f, "IFCSHAPEREPRESENTATION")
        .iter()
        .filter(|r| str_of(&r.attrs[1]) == "Body")
        .map(|r| str_of(&r.attrs[2]).to_string())
        .collect();
    assert!(
        reps.iter().filter(|r| *r == "Tessellation").count() >= 4,
        "terrain, draped slab, sloped slab, draped area"
    );
    assert!(reps.iter().filter(|r| *r == "SweptSolid").count() >= 8);
    // A flat extrusion has the thickness of its layers: 60 + 40 + 200 mm.
    let depths: Vec<&Value> = named(&f, "IFCEXTRUDEDAREASOLID")
        .iter()
        .map(|e| &e.attrs[3])
        .collect();
    assert!(depths.contains(&&Value::Real(300.0)));
    assert!(depths.contains(&&Value::Real(120.0)));
    assert!(depths.contains(&&Value::Real(100.0)), "landscape area");
}

#[test]
fn layer_names_and_styles_are_shared() {
    let f = parsed(&sample(), &IfcOptions::default());
    // One surface style per distinct color.
    let styles = named(&f, "IFCSURFACESTYLE");
    let names: std::collections::BTreeSet<&str> =
        styles.iter().map(|s| str_of(&s.attrs[0])).collect();
    assert_eq!(names.len(), styles.len());
    assert!(names.contains("#1EA03C"), "{names:?}");
    // Colors are fractions.
    for c in named(&f, "IFCCOLOURRGB") {
        for k in 1..4 {
            let Value::Real(x) = c.attrs[k] else { panic!() };
            assert!((0.0..=1.0).contains(&x));
        }
    }
}

#[test]
fn the_view_definition_is_in_the_header() {
    let text = export(&sample(), &IfcOptions::default()).0;
    assert!(text.contains("FILE_DESCRIPTION(('ViewDefinition [ReferenceView_V1.2]'),'2;1');"));
}

#[test]
fn tiny_plants_do_not_panic() {
    let mut ed = Editor::with_sequential_ids();
    for (spread, height) in [
        (15.0, 15.0),
        (10.0, 10.0),
        (11.0, 5000.0),
        (9.9, 9.9),
        (1.0e-3, 1.0e-3),
        (1.0e9, 1.0e9),
    ] {
        add(
            &mut ed,
            Shape::ExistingTree {
                at: v2(0.0, 0.0),
                species: String::new(),
                dbh: 500.0,
                spread,
                height,
                condition: TreeCondition::Good,
                action: TreeAction::Retain,
                rpz_factor: None,
                rpz_cap: None,
            },
        );
    }
    let f = parsed(ed.doc(), &IfcOptions::default());
    assert!(f.dangling().is_empty());
}

#[test]
fn every_global_id_is_unique_even_when_property_sets_collide() {
    let mut ed = Editor::new(sample());
    // A rule that writes into a set that the exporter writes itself, and a set named like a record set.
    let mut map = DataMap::landscape_template();
    let ifc = |pset: &str, property: &str| kadakas_core::datamap::Target::IfcProperty {
        pset: pset.into(),
        property: property.into(),
        ty: Default::default(),
    };
    let constant = |v: &str| kadakas_core::datamap::Source::Constant { value: json!(v) };
    let rule = |source, target| kadakas_core::datamap::MapRule {
        name: String::new(),
        kinds: vec![],
        source,
        target,
    };
    let slab_rule = |source, target| kadakas_core::datamap::MapRule {
        kinds: vec!["hardscape".into()],
        ..rule(source, target)
    };
    map.rules.push(slab_rule(
        constant("mine"),
        ifc("Pset_SlabCommon", "Reference"),
    ));
    map.rules.push(slab_rule(
        constant("extra"),
        ifc("Pset_SlabCommon", "Extra"),
    ));
    map.rules
        .push(rule(constant("x"), ifc("ePset_Soil", "FromMap")));
    map.rules
        .push(rule(constant("t"), ifc("Kadakas_Tree", "Note")));
    run(
        &mut ed,
        Command::AddResource {
            name: "Collisions".into(),
            body: ResourceBody::DataMap(Box::new(map)),
        },
    );
    let (text, _) = export(ed.doc(), &IfcOptions::default());
    dump("collisions.ifc", &text);
    let f = step::parse(&text).unwrap();
    let guids: Vec<&str> = f
        .entities
        .iter()
        .filter_map(|e| match e.attrs.first() {
            Some(Value::Str(s)) if crate::guid::is_valid(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    assert!(guids.len() > 40);
    let unique: std::collections::BTreeSet<&&str> = guids.iter().collect();
    assert_eq!(unique.len(), guids.len(), "GlobalIds are unique");
    // Each set exists once per element, with the properties merged. The rule wins over the standard value.
    let sets = named(&f, "IFCPROPERTYSET");
    let slab_sets: Vec<_> = sets
        .iter()
        .filter(|s| str_of(&s.attrs[2]) == "Pset_SlabCommon")
        .collect();
    assert_eq!(slab_sets.len(), 5, "one per slab");
    let props = |set: &step::Entity| -> Vec<String> {
        let Value::List(ids) = &set.attrs[4] else {
            panic!()
        };
        ids.iter()
            .map(|r| str_of(&f.get(r.as_ref_id().unwrap()).unwrap().attrs[0]).to_string())
            .collect()
    };
    let names = props(slab_sets[0]);
    assert_eq!(names.iter().filter(|n| *n == "Reference").count(), 1);
    assert!(names.contains(&"Extra".to_string()) && names.contains(&"IsExternal".to_string()));
    let soil: Vec<_> = sets
        .iter()
        .filter(|s| str_of(&s.attrs[2]) == "ePset_Soil")
        .collect();
    assert_eq!(
        soil.len(),
        12,
        "one per element, because the rule applies to all kinds"
    );
    assert!(
        props(
            soil.iter()
                .find(|s| props(s).contains(&"Depth".to_string()))
                .unwrap()
        )
        .contains(&"FromMap".to_string())
    );
}

#[test]
fn a_document_without_a_project_guid_is_not_changed_and_gets_a_content_guid() {
    let mut doc = sample();
    doc.drawing.project.guid.clear();
    let before = doc.clone();
    let (a, report) = export(&doc, &IfcOptions::default());
    assert_eq!(doc, before, "export never changes the document");
    assert!(report.notes.iter().any(|n| n.contains("no project GUID")));
    assert_eq!(a, export(&doc, &IfcOptions::default()).0, "deterministic");
    let (guid, stored) = crate::project_guid(&doc);
    assert!(!stored && crate::guid::is_valid(&guid));
    let mut other = doc.clone();
    other.drawing.project.name.push('x');
    assert_ne!(crate::project_guid(&other).0, guid);
    // A document with a GUID reports none missing.
    let (guid, stored) = crate::project_guid(&sample());
    assert!(stored);
    assert_eq!(guid, "1Gx3v0EYX3_fOnxLUh6gRb");
    assert!(
        !export(&sample(), &IfcOptions::default())
            .1
            .notes
            .iter()
            .any(|n| n.contains("no project GUID"))
    );
    // Loading makes a GUID in memory, and a new one can be set by command.
    let mut loaded = Document::from_json(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert!(loaded.drawing.project.guid.is_empty());
    assert!(loaded.ensure_project_guid());
    assert!(!loaded.ensure_project_guid());
    assert!(crate::guid::is_valid(&loaded.drawing.project.guid));
}

// ---------------------------------------------------------------- random documents

fn arb_point() -> impl Strategy<Value = DVec2> {
    (-5.0e4..5.0e4f64, -5.0e4..5.0e4f64).prop_map(|(x, y)| DVec2::new(x, y))
}

fn arb_ring() -> impl Strategy<Value = Vec<Vertex>> {
    prop::collection::vec(arb_point(), 3..8).prop_map(|p| curve::corners(&p))
}

fn arb_shape() -> impl Strategy<Value = Shape> {
    let size = prop_oneof![0.0..30.0f64, 0.0..2.0e4f64, 1.0e6..1.0e9f64];
    prop_oneof![
        (arb_point(), arb_point()).prop_map(|(a, b)| Shape::Line { a, b }),
        (arb_point(), arb_point()).prop_map(|(min, max)| Shape::Rect { min, max }),
        (arb_point(), 1.0..1.0e4f64).prop_map(|(center, radius)| Shape::Circle { center, radius }),
        (arb_point(), 1.0..1.0e4f64, -7.0..7.0f64, -7.0..7.0f64).prop_map(
            |(center, radius, start, end)| Shape::Arc {
                center,
                radius,
                start,
                end
            }
        ),
        (arb_ring(), any::<bool>()).prop_map(|(points, closed)| Shape::Polyline { points, closed }),
        (arb_point(), "[a-zA-Zäö' \\\\]{0,12}").prop_map(|(at, text)| Shape::Text {
            at,
            text,
            height: 200.0
        }),
        (arb_point(), size.clone(), size.clone()).prop_map(|(at, spread, height)| Shape::Plant {
            at,
            species: "Pläntä".into(),
            spread,
            height
        }),
        (arb_point(), 0.0..800.0f64, size.clone(), size.clone()).prop_map(
            |(at, dbh, spread, height)| Shape::ExistingTree {
                at,
                species: String::new(),
                dbh,
                spread,
                height,
                condition: TreeCondition::Good,
                action: TreeAction::Retain,
                rpz_factor: None,
                rpz_cap: None
            }
        ),
        (arb_ring(), "[a-z]{0,6}")
            .prop_map(|(boundary, name)| Shape::LandscapeArea { boundary, name }),
        (
            arb_ring(),
            prop::option::of(1.0..500.0f64),
            prop::option::of(-500.0..500.0f64)
        )
            .prop_map(|(boundary, thickness, elevation)| Shape::Hardscape {
                boundary,
                material: "Stone".into(),
                thickness,
                elevation
            }),
        (-1.0e4..1.0e4f64, -1.0e4..1.0e4f64, -1.0e3..1.0e3f64).prop_map(|(x, y, z)| {
            Shape::Locus3d {
                p: DVec3::new(x, y, z),
            }
        }),
        prop::collection::vec((arb_point(), -100.0..100.0f64), 2..6).prop_map(|p| {
            Shape::Polyline3d {
                points: p.into_iter().map(|(a, z)| a.extend(z)).collect(),
                closed: false,
            }
        }),
        prop::collection::vec((0.0..1.0e4f64, 0.0..1.0e4f64, 0.0..500.0f64), 0..40).prop_map(|p| {
            Shape::SiteModel {
                points: p.into_iter().map(|(x, y, z)| DVec3::new(x, y, z)).collect(),
                lines: vec![],
                settings: Default::default(),
            }
        }),
        arb_ring().prop_map(|boundary| Shape::SiteModifier {
            modifier: ModifierKind::Pad {
                elevation: 10.0,
                slope: None
            },
            boundary
        }),
    ]
}

fn arb_options() -> impl Strategy<Value = IfcOptions> {
    (any::<bool>(), any::<bool>(), any::<bool>(), any::<bool>()).prop_map(|(a, b, c, d)| {
        IfcOptions {
            classes_as_layers: a,
            include_2d: b,
            layers_as_storeys: c,
            visible_only: d,
            ..Default::default()
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Plants and trees of any size export without panics, and the references resolve.
    #[test]
    fn plants_of_any_size_export(
        spread in prop_oneof![1.0e-3..30.0f64, 1.0..1.0e5f64, 1.0e6..1.0e9f64],
        height in prop_oneof![1.0e-3..30.0f64, 1.0..1.0e5f64, 1.0e6..1.0e9f64],
        dbh in prop_oneof![0.0..30.0f64, 0.0..1.0e4f64],
        tree in any::<bool>(),
    ) {
        let mut ed = Editor::with_sequential_ids();
        let shape = if tree {
            Shape::ExistingTree { at: v2(0.0, 0.0), species: String::new(), dbh, spread, height, condition: TreeCondition::Good, action: TreeAction::Retain, rpz_factor: None, rpz_cap: None }
        } else {
            Shape::Plant { at: v2(0.0, 0.0), species: "p".into(), spread, height }
        };
        if ed.execute(&Command::AddObject { shape, layer: None, class: None, attrs: Attributes::default() }).is_ok() {
            let f = parsed(ed.doc(), &IfcOptions::default());
            prop_assert!(f.dangling().is_empty());
            prop_assert!(!named(&f, "IFCBOUNDINGBOX").is_empty());
        }
    }

    #[test]
    fn random_documents_export_with_every_reference_resolved(
        shapes in prop::collection::vec(arb_shape(), 0..14),
        drape in any::<bool>(),
        extra_layer in any::<bool>(),
        opts in arb_options(),
    ) {
        let mut ed = Editor::with_sequential_ids();
        let class = ClassId(run(&mut ed, Command::AddClass { name: "Ü".into(), attrs: Attributes { fill: Some(Color([1, 2, 3, 200])), ..Default::default() } })[0]);
        let layer = if extra_layer { Some(LayerId(run(&mut ed, Command::AddLayer { name: "L2".into(), elevation: 1500.0, scale: None, kind: None })[0])) } else { None };
        for s in shapes {
            let is_area = matches!(s, Shape::LandscapeArea { .. } | Shape::Hardscape { .. });
            if let Ok(o) = ed.execute(&Command::AddObject { shape: s, layer, class: Some(class), attrs: Attributes::default() }) && is_area && drape {
                let id = ObjectId(o.created[0]);
                let _ = ed.execute(&Command::SetHardscape { ids: vec![id], patch: HardscapePatch { drape: Some(true), ..Default::default() } });
                let _ = ed.execute(&Command::SetLandscapeArea { ids: vec![id], patch: LandscapePatch { drape: Some(true), ..Default::default() } });
            }
        }
        let (bytes, report) = export_bytes(ed.doc(), &opts).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        prop_assert!(text.is_ascii());
        let f = step::parse(&text).unwrap();
        prop_assert!(f.dangling().is_empty(), "{:?}", f.dangling());
        let ids: std::collections::BTreeSet<u64> = f.entities.iter().map(|e| e.id).collect();
        prop_assert_eq!(ids.len(), f.entities.len());
        prop_assert!(!f.entities.is_empty() && f.entities.iter().all(|e| e.id >= 1));
        // Every object is either counted or skipped.
        let skipped: usize = report.skipped.iter().map(|s| s.count).sum();
        prop_assert_eq!(report.total() + skipped, ed.doc().objects.len());
        // Same input, same bytes.
        prop_assert_eq!(&text, &String::from_utf8(export_bytes(ed.doc(), &opts).unwrap().0).unwrap());
    }
}

// ---------------------------------------------------------------- schema versions

fn with_version(v: &str) -> IfcOptions {
    IfcOptions {
        version: v.into(),
        include_2d: true,
        ..IfcOptions::default()
    }
}

#[test]
fn ifc2x3_has_an_owner_history_proxies_and_surface_models() {
    let doc = sample();
    let f = parsed(&doc, &with_version("IFC2X3"));
    assert_eq!(f.schemas(), vec!["IFC2X3".to_string()]);
    assert!(f.dangling().is_empty(), "{:?}", f.dangling());
    let c = count(&f);
    assert_eq!(c["IFCOWNERHISTORY"], 1);
    for gone in [
        "IFCGEOGRAPHICELEMENT",
        "IFCTRIANGULATEDFACESET",
        "IFCMAPCONVERSION",
        "IFCPROJECTEDCRS",
        "IFCCARTESIANPOINTLIST3D",
    ] {
        assert!(!c.contains_key(gone), "{gone}");
    }
    assert!(c["IFCBUILDINGELEMENTPROXY"] >= 4);
    assert!(c["IFCFACEBASEDSURFACEMODEL"] >= 1 && c["IFCFACE"] >= 100);
    assert!(c["IFCPRESENTATIONSTYLEASSIGNMENT"] >= 1);
    // Every rooted entity names the owner history.
    let oh = f
        .entities
        .iter()
        .find(|e| e.name == "IFCOWNERHISTORY")
        .unwrap()
        .id;
    for e in &f.entities {
        if matches!(
            e.name.as_str(),
            "IFCSITE" | "IFCPROJECT" | "IFCSLAB" | "IFCPROPERTYSET" | "IFCELEMENTQUANTITY"
        ) {
            assert_eq!(e.attrs[1], Value::Ref(oh), "{}", e.name);
        }
    }
    // The georeference is in property sets of the site.
    let names: Vec<String> = f
        .entities
        .iter()
        .filter(|e| e.name == "IFCPROPERTYSET")
        .filter_map(|e| match &e.attrs[2] {
            Value::Str(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    assert!(names.contains(&"ePSet_MapConversion".to_string()));
    assert!(names.contains(&"ePSet_ProjectedCRS".to_string()));
    // The circle profiles have a position.
    for e in f
        .entities
        .iter()
        .filter(|e| e.name == "IFCCIRCLEPROFILEDEF")
    {
        assert!(matches!(e.attrs[2], Value::Ref(_)));
    }
    // No attribute of the IFC4-only kind is left.
    for e in f.entities.iter().filter(|e| e.name == "IFCMATERIAL") {
        assert_eq!(e.attrs.len(), 1);
    }
    for e in f
        .entities
        .iter()
        .filter(|e| e.name == "IFCSURFACESTYLERENDERING")
    {
        assert_eq!(e.attrs.len(), 9);
    }
}

#[test]
fn ifc4x3_names_vegetation_and_counts_in_integers() {
    let doc = sample();
    let f = parsed(&doc, &with_version("IFC4X3"));
    assert_eq!(f.schemas(), vec!["IFC4X3_ADD2".to_string()]);
    assert!(f.dangling().is_empty());
    let c = count(&f);
    assert!(c.contains_key("IFCMAPCONVERSION") && c.contains_key("IFCTRIANGULATEDFACESET"));
    let enums: Vec<String> = f
        .entities
        .iter()
        .filter(|e| e.name == "IFCGEOGRAPHICELEMENT")
        .filter_map(|e| match e.attrs.last() {
            Some(Value::Enum(p)) => Some(p.clone()),
            _ => None,
        })
        .collect();
    assert!(enums.contains(&"VEGETATION".to_string()) && enums.contains(&"TERRAIN".to_string()));
    assert!(!enums.contains(&"USERDEFINED".to_string()));
    for e in f.entities.iter().filter(|e| e.name == "IFCQUANTITYCOUNT") {
        assert!(matches!(e.attrs[3], Value::Int(_)));
    }
    for e in f.entities.iter().filter(|e| e.name == "IFCANNOTATION") {
        assert_eq!(e.attrs.len(), 8);
    }
    // Default IFC4 is unchanged.
    let f4 = parsed(&doc, &IfcOptions::default());
    assert_eq!(f4.schemas(), vec!["IFC4".to_string()]);
    assert!(!count(&f4).contains_key("IFCOWNERHISTORY"));
}

#[test]
fn ifc2x3_without_a_georeference_or_with_a_bad_crs_still_writes() {
    let mut ed = Editor::with_sequential_ids();
    add(&mut ed, site_shape());
    let f = parsed(ed.doc(), &with_version("IFC2X3"));
    assert!(!f.entities.iter().any(|e| e.name == "IFCPROPERTYSET"
        && matches!(&e.attrs[2], Value::Str(s) if s.starts_with("ePSet"))));
    run(
        &mut ed,
        Command::SetGeoreference {
            georef: Some(Georeference {
                crs: "EPSG:3067".into(),
                origin: v2(1.0, 2.0),
                rotation: 0.0,
                user_origin: DVec2::ZERO,
            }),
        },
    );
    let f = parsed(ed.doc(), &with_version("IFC2X3"));
    assert!(f.dangling().is_empty());
}
