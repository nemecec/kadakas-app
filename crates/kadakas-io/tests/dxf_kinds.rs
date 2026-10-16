//! DXF export of irrigation objects, massing models and plant tags (they were reported as
//! unsupported kinds).

use kadakas_core::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::Vertex;
use kadakas_core::irrigation::{IrrigationShape, LineType};
use kadakas_core::massing::{Massing, RoofStyle};
use kadakas_core::model::{Attributes, Shape};
use kadakas_core::plants::style::PlantStyle;
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_io::dxf::DxfFormat;
use kadakas_io::{ExportOptions, Exporter, MeshEntity, Report};

fn add(ed: &mut Editor, shape: Shape) -> kadakas_core::uid::Uid {
    ed.execute(&Command::AddObject {
        shape,
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap()
    .created[0]
}

fn export(ed: &Editor, opts: &ExportOptions) -> (String, Report) {
    let r = DxfFormat.export(ed.doc(), opts).unwrap();
    (String::from_utf8(r.bytes).unwrap(), r.report)
}

/// The number of entities of a type in the file.
fn count(text: &str, entity: &str) -> usize {
    text.lines()
        .zip(text.lines().skip(1))
        .filter(|(a, b)| a.trim() == "0" && b.trim() == entity)
        .count()
}

fn irrigation(ed: &mut Editor) {
    let at = |x: f64| DVec2::new(x, 0.0);
    for shape in [
        IrrigationShape::Outlet {
            at: at(0.0),
            model: "Spray-4.0".into(),
            arc: 90.0,
            direction: 45.0,
            radius: Some(3000.0),
            zone: Some(1),
        },
        IrrigationShape::Line {
            points: vec![at(0.0), at(5000.0)],
            line_type: LineType::Mainline,
            size: "50".into(),
            material: "PVC".into(),
            emitter_spacing: None,
            emitter_flow: None,
        },
        IrrigationShape::Line {
            points: vec![at(0.0), at(5000.0)],
            line_type: LineType::Drip,
            size: "16".into(),
            material: "PE".into(),
            emitter_spacing: Some(300.0),
            emitter_flow: Some(2.0),
        },
        IrrigationShape::Poc {
            at: at(100.0),
            name: "Meter".into(),
            static_pressure: 400.0,
            available_flow: 100.0,
            supply_loss: None,
            meter_loss: Some(10.0),
        },
        IrrigationShape::Valve {
            at: at(200.0),
            zone: 1,
            name: "V1".into(),
            kv: 6.0,
            controller: Some("C1".into()),
            station: Some(1),
        },
        IrrigationShape::Controller {
            at: at(300.0),
            name: "C1".into(),
            stations: 4,
        },
    ] {
        add(ed, Shape::Irrigation(Box::new(shape)));
    }
}

fn massing(ed: &mut Editor, roof: RoofStyle) -> kadakas_core::uid::Uid {
    let ring: Vec<Vertex> = [(0.0, 0.0), (8000.0, 0.0), (8000.0, 5000.0), (0.0, 5000.0)]
        .iter()
        .map(|(x, y)| Vertex::corner(DVec2::new(*x, *y)))
        .collect();
    let mut m = Massing::new(ring, 6000.0);
    m.roof = roof;
    m.name = "House".into();
    add(ed, Shape::Massing(Box::new(m)))
}

#[test]
fn irrigation_is_written_on_layers_per_kind_with_xdata() {
    let mut ed = Editor::default();
    irrigation(&mut ed);
    let (text, report) = export(&ed, &ExportOptions::default());
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    for layer in [
        "IRR-OUTLET",
        "IRR-MAINLINE",
        "IRR-DRIP",
        "IRR-POC",
        "IRR-VALVE",
        "IRR-CONTROLLER",
    ] {
        assert!(text.contains(&format!("\n{layer}\n")), "{layer}");
    }
    assert_eq!(report.objects.values().sum::<usize>(), 6);
    // One XDATA block per object, and the application is registered.
    assert_eq!(text.matches("\n1001\nKADAKAS\n").count(), 6);
    assert!(text.contains("\n2\nKADAKAS\n"), "APPID");
    assert!(text.contains("1000\nmodel=Spray-4.0\n"));
    assert!(text.contains("1000\nline_type=drip\n"));
    assert!(text.contains("1000\nkv=6\n"));
    assert!(text.contains("1000\nstations=4\n"));
    // The fan of the head is a polyline, the POC has a circle and a cross.
    assert_eq!(count(&text, "LWPOLYLINE"), 2 + 1 + 1 + 1);
    assert_eq!(count(&text, "LINE"), 2);
}

#[test]
fn massing_is_an_outline_and_a_mesh() {
    let mut ed = Editor::default();
    massing(&mut ed, RoofStyle::Gable);
    let (text, report) = export(&ed, &ExportOptions::default());
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(count(&text, "LWPOLYLINE"), 1);
    assert!(text.contains("1000\nheight_mm=6000\n"));
    assert!(text.contains("1000\nroof=gable\n"));
    assert_eq!(count(&text, "POLYLINE"), 1);
    let (faces, _) = export(
        &ed,
        &ExportOptions {
            mesh_entity: MeshEntity::Face3d,
            ..Default::default()
        },
    );
    assert!(count(&faces, "3DFACE") >= 10);
    assert_eq!(count(&faces, "POLYLINE"), 0);
}

#[test]
fn plant_tags_are_a_leader_and_text() {
    let mut ed = Editor::default();
    let style = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Lavender".into(),
            body: ResourceBody::plant_style(PlantStyle {
                id_code: "LA".into(),
                ..PlantStyle::default()
            }),
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::AddPlants {
        style: Some(style),
        overrides: Default::default(),
        positions: vec![DVec2::ZERO; 3],
        layer: None,
        class: None,
        variation: None,
    })
    .unwrap();
    add(
        &mut ed,
        Shape::PlantTag {
            at: DVec2::new(100.0, 200.0),
            anchor: DVec2::new(0.0, 50.0),
            style,
            height: 300.0,
        },
    );
    let (text, report) = export(&ed, &ExportOptions::default());
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert_eq!(report.objects.get("plant_tag"), Some(&1));
    assert!(text.contains("\nLA (3)\n"), "{text}");
    assert_eq!(count(&text, "TEXT"), 1);
}

#[test]
fn irrigation_layers_follow_the_layer_mode() {
    let mut ed = Editor::default();
    irrigation(&mut ed);
    let (text, _) = export(
        &ed,
        &ExportOptions {
            layer_mode: kadakas_io::LayerMode::Layers,
            ..Default::default()
        },
    );
    assert!(text.contains("-IRR-OUTLET\n"), "{text}");
    assert!(!text.contains("\nIRR-OUTLET\n"));
}

#[test]
fn xdata_is_cut_before_it_is_escaped() {
    let mut ed = Editor::default();
    add(
        &mut ed,
        Shape::Irrigation(Box::new(IrrigationShape::Controller {
            at: DVec2::ZERO,
            name: "\u{e9}".repeat(300),
            stations: 1,
        })),
    );
    let (text, _) = export(&ed, &ExportOptions::default());
    let lines: Vec<&str> = text.lines().collect();
    let name = lines
        .iter()
        .zip(lines.iter().skip(1))
        .find(|(a, b)| a.trim() == "1000" && b.starts_with("name="))
        .map(|(_, b)| *b)
        .unwrap();
    assert!(name.len() <= 250, "{}", name.len());
    // Every escape is whole: `\U+00E9`.
    let body = name.strip_prefix("name=").unwrap();
    assert!(!body.is_empty() && body.len() % 7 == 0, "{body}");
    assert!(body.as_bytes().chunks(7).all(|c| c.starts_with(b"\\U+")));
}

#[test]
fn a_building_and_the_ground_share_the_layer_elevation() {
    let mut ed = Editor::default();
    let layer = ed
        .execute(&Command::AddLayer {
            name: "Raised".into(),
            elevation: 2000.0,
            scale: None,
            kind: None,
        })
        .unwrap()
        .created[0];
    let on = |ed: &mut Editor, shape: Shape| {
        ed.execute(&Command::AddObject {
            shape,
            layer: Some(kadakas_core::model::LayerId(layer)),
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
    };
    let mut points = Vec::new();
    for i in 0..5 {
        for j in 0..5 {
            points.push(kadakas_core::DVec3::new(
                f64::from(i) * 4000.0,
                f64::from(j) * 4000.0,
                100.0,
            ));
        }
    }
    on(
        &mut ed,
        Shape::SiteModel {
            points,
            lines: vec![],
            settings: Default::default(),
        },
    );
    let ring: Vec<Vertex> = [
        (4000.0, 4000.0),
        (8000.0, 4000.0),
        (8000.0, 8000.0),
        (4000.0, 8000.0),
    ]
    .iter()
    .map(|(x, y)| Vertex::corner(DVec2::new(*x, *y)))
    .collect();
    on(
        &mut ed,
        Shape::Massing(Box::new(Massing::new(ring, 3000.0))),
    );
    let (text, _) = export(
        &ed,
        &ExportOptions {
            site_mesh: kadakas_io::SiteMesh::Existing,
            ..Default::default()
        },
    );
    let entities = &text[text.find("\nENTITIES\n").unwrap()..];
    let lines: Vec<&str> = entities.lines().collect();
    let zs: Vec<f64> = lines
        .iter()
        .zip(lines.iter().skip(1))
        .filter(|(a, _)| a.trim() == "30")
        .filter_map(|(_, b)| b.trim().parse().ok())
        .collect();
    // The face records of a polyface mesh have no height (0).
    let min = zs
        .iter()
        .copied()
        .filter(|z| *z != 0.0)
        .fold(f64::MAX, f64::min);
    assert!((min - 2100.0).abs() < 1e-6, "lowest z {min}");
    // The ground is at 2100 and the building starts there, and goes up by its height.
    assert!(zs.iter().any(|z| (z - 5100.0).abs() < 1e-6));
}
