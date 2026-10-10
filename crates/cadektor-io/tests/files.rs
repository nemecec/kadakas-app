//! Imports of the hand-written files in `tests/fixtures`, through the public API.

use std::path::PathBuf;

use cadektor_core::DVec2;
use cadektor_core::command::Editor;
use cadektor_core::curve::VertexKind;
use cadektor_core::model::{Color, Document, Shape};
use cadektor_io::{ImportOptions, LayerMode, import_file};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run(name: &str, opts: &ImportOptions) -> (Editor, cadektor_io::Report) {
    let doc = Document::with_sequential_ids();
    let r = import_file(&fixture(name), None, opts, &doc).unwrap();
    let mut ed = Editor::new(doc);
    ed.execute(&r.command).unwrap();
    (ed, r.report)
}

#[test]
fn site_dxf_as_classes() {
    let (mut ed, report) = run("site.dxf", &ImportOptions::default());
    let doc = ed.doc();
    assert_eq!(doc.objects.len(), 7);
    assert_eq!(report.skipped_total(), 1);
    assert_eq!(report.skipped[0].entity, "DIMENSION");
    // One design layer, DXF layers are classes with their colors.
    assert_eq!(doc.layers.len(), 1);
    let names: Vec<&str> = doc.classes.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["None", "Walls", "Planting", "Survey"]);
    let walls = doc.classes.iter().find(|c| c.name == "Walls").unwrap();
    assert_eq!(walls.attrs.pen, Some(Color([255, 0, 0, 255])));
    assert_eq!(walls.attrs.line_weight, Some(0.5));
    // Units are meters: the circle has a radius of 750 mm.
    assert!(
        doc.objects.values().any(
            |o| matches!(o.shape, Shape::Circle { radius, .. } if (radius - 750.0).abs() < 1e-9)
        )
    );
    // The survey point keeps its Z in mm.
    assert!(
        doc.objects
            .values()
            .any(|o| matches!(o.shape, Shape::Locus3d { p } if p.z == 12_500.0))
    );
    // The room outline has one exact arc vertex.
    let room = doc.objects.values().find_map(|o| match &o.shape {
        Shape::Polyline {
            points,
            closed: true,
        } => Some(points.clone()),
        _ => None,
    });
    let room = room.unwrap();
    assert_eq!(
        room.iter()
            .filter(|v| v.kind == VertexKind::PointOnArc)
            .count(),
        1
    );
    // The bench is two lines, rotated by 90 degrees about the insertion point (5 m, 2 m).
    let lines: Vec<_> = doc
        .objects
        .values()
        .filter_map(|o| {
            if let Shape::Line { a, b } = o.shape {
                Some((a, b))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].0.distance(DVec2::new(5000.0, 2000.0)) < 1e-6);
    assert!(lines[0].1.distance(DVec2::new(5000.0, 3800.0)) < 1e-6);
    // The whole import is one undo step.
    assert!(ed.undo());
    assert!(ed.doc().objects.is_empty());
    assert_eq!(ed.doc().classes.len(), 1);
    assert!(!ed.undo());
}

#[test]
fn site_dxf_as_layers() {
    let opts = ImportOptions {
        layer_mode: LayerMode::Layers,
        ..Default::default()
    };
    let (ed, _) = run("site.dxf", &opts);
    let names: Vec<&str> = ed.doc().layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Layer 1", "Walls", "Planting", "Survey"]);
    assert_eq!(ed.doc().classes.len(), 1);
    // Objects take the pen of their layer when they have none.
    let circle = ed
        .doc()
        .objects
        .values()
        .find(|o| matches!(o.shape, Shape::Circle { .. }))
        .unwrap();
    assert_eq!(circle.attrs.pen, Some(Color([0, 255, 0, 255])));
}

#[test]
fn trees_geojson() {
    let (ed, report) = run("trees.geojson", &ImportOptions::default());
    let doc = ed.doc();
    assert_eq!(doc.objects.len(), 4);
    assert!(report.georeferenced);
    let g = doc.georef.as_ref().unwrap();
    assert_eq!(g.crs, "EPSG:32635");
    // Geometry stays near the model origin although UTM values are millions.
    for o in doc.objects.values() {
        let (lo, hi) = o.shape.bounds();
        assert!(lo.abs().max_element() < 1.0e6 && hi.abs().max_element() < 1.0e6);
    }
    let oak = doc
        .objects
        .values()
        .find(|o| o.data.get("species").and_then(|v| v.as_str()) == Some("Quercus robur"))
        .unwrap();
    assert_eq!(oak.data["trunk_cm"], 54);
    assert_eq!(oak.data["protected"], false);
    assert!(matches!(oak.shape, Shape::Locus3d { p } if p.z == 12_900.0));
    let hole = doc
        .objects
        .values()
        .find(|o| o.data.contains_key("_ring"))
        .unwrap();
    assert_eq!(hole.data["name"], "Plot 1");
    assert_eq!(doc.class(oak.class).unwrap().name, "trees");
}
