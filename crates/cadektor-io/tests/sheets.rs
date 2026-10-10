use cadektor_core::DVec2;
use cadektor_core::command::{Command, Editor};
use cadektor_core::model::*;
use cadektor_core::sheets::Viewport;
use cadektor_io::{ExportOptions, Format, LayerMode, export_file};

fn doc_with_sheet() -> Document {
    let mut ed = Editor::with_sequential_ids();
    let add = |ed: &mut Editor, shape, layer| {
        ed.execute(&Command::AddObject {
            shape,
            layer,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
    };
    add(
        &mut ed,
        Shape::Line {
            a: DVec2::new(0.0, 0.0),
            b: DVec2::new(1000.0, 0.0),
        },
        None,
    );
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
    add(
        &mut ed,
        Shape::Line {
            a: DVec2::new(5.0, 5.0),
            b: DVec2::new(4321.5, 5.0),
        },
        Some(sheet),
    );
    add(
        &mut ed,
        Shape::Text {
            at: DVec2::new(10.0, 10.0),
            text: "PAPERTEXT".into(),
            height: 5.0,
        },
        Some(sheet),
    );
    add(
        &mut ed,
        Shape::Viewport(Box::new(Viewport::new(
            DVec2::new(10.0, 10.0),
            DVec2::new(200.0, 100.0),
            DVec2::ZERO,
            100.0,
            vec![FIRST_LAYER],
        ))),
        Some(sheet),
    );
    ed.doc().clone()
}

#[test]
fn exporters_skip_sheet_layers_and_their_objects() {
    let doc = doc_with_sheet();
    let dir = std::env::temp_dir().join(format!("cadektor-io-sheets-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for mode in [LayerMode::Classes, LayerMode::Layers] {
        let opts = ExportOptions {
            layer_mode: mode,
            ..Default::default()
        };
        let dxf = dir.join("a.dxf");
        let report = export_file(&dxf, Some(Format::Dxf), &opts, &doc).unwrap();
        let text = std::fs::read_to_string(&dxf).unwrap();
        assert!(
            !text.contains("PAPERTEXT") && !text.contains("PAPER-SHEET"),
            "{mode:?}: paper space in the DXF"
        );
        assert!(
            !text.contains("4321.5"),
            "{mode:?}: the paper line is not written"
        );
        assert_eq!(report.total(), 1);
    }
    let gj = dir.join("a.geojson");
    let report = export_file(&gj, Some(Format::GeoJson), &ExportOptions::default(), &doc).unwrap();
    let text = std::fs::read_to_string(&gj).unwrap();
    assert_eq!(report.total(), 1);
    assert!(!text.contains("PAPERTEXT") && !text.contains("4.3215"));
    std::fs::remove_dir_all(&dir).unwrap();
}
