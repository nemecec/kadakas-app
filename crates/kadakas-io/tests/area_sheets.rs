//! Hardscapes and landscape areas that sit on a sheet layer (for example from a hand-made file)
//! are paper space. The exporters skip them like all other objects of sheets.

use kadakas_core::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::corners;
use kadakas_core::model::*;
use kadakas_io::{ExportOptions, Format, LayerMode, export_file};

#[test]
fn exporters_skip_hardscapes_and_landscape_areas_on_sheets() {
    let mut ed = Editor::with_sequential_ids();
    let tri = |x: f64| {
        corners(&[
            DVec2::new(x, 0.0),
            DVec2::new(x + 1000.0, 0.0),
            DVec2::new(x, 1000.0),
        ])
    };
    let shape = Shape::Hardscape {
        boundary: tri(0.0),
        material: "MODELHARD".into(),
        thickness: None,
        elevation: None,
    };
    ed.execute(&Command::AddObject {
        shape,
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let sheet = LayerId(
        ed.execute(&Command::AddSheet {
            name: "PAPER-SHEET".into(),
            paper: None,
            orientation: None,
            number: None,
            title: None,
        })
        .unwrap()
        .created[0],
    );
    let mut doc = ed.doc().clone();
    // The commands refuse these shapes on a sheet, so put them there directly.
    let mut next = 900;
    for shape in [
        Shape::Hardscape {
            boundary: tri(54321.0),
            material: "x".into(),
            thickness: None,
            elevation: None,
        },
        Shape::LandscapeArea {
            boundary: tri(65432.0),
            name: "PAPERBED".into(),
        },
    ] {
        let id = ObjectId(kadakas_core::uid::Uid::from_num(next));
        next += 1;
        doc.objects
            .insert(id, Object::new(id, sheet, NONE_CLASS, shape));
    }
    let dir = std::env::temp_dir().join(format!("kadakas-io-area-sheets-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for mode in [LayerMode::Classes, LayerMode::Layers] {
        let dxf = dir.join("a.dxf");
        let report = export_file(
            &dxf,
            Some(Format::Dxf),
            &ExportOptions {
                layer_mode: mode,
                ..Default::default()
            },
            &doc,
        )
        .unwrap();
        let text = std::fs::read_to_string(&dxf).unwrap();
        assert!(
            !text.contains("PAPERBED") && !text.contains("54321") && !text.contains("65432"),
            "{mode:?}: paper space in the DXF"
        );
        assert_eq!(report.total(), 1);
    }
    let gj = dir.join("a.geojson");
    let report = export_file(&gj, Some(Format::GeoJson), &ExportOptions::default(), &doc).unwrap();
    let text = std::fs::read_to_string(&gj).unwrap();
    assert_eq!(report.total(), 1);
    assert!(!text.contains("54321") && !text.contains("65432"));
    std::fs::remove_dir_all(&dir).unwrap();
}
