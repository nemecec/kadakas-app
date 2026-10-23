use kadakas_core::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::model::*;
use kadakas_core::sheets::Viewport;
use kadakas_io::{ExportOptions, Format, LayerMode, export_file};

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
fn dxf_writes_sheet_layers_as_layouts_unless_told_not_to_and_geojson_skips_them() {
    let doc = doc_with_sheet();
    let dir = std::env::temp_dir().join(format!("kadakas-io-sheets-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for mode in [LayerMode::Classes, LayerMode::Layers] {
        // The default: the sheet is a layout with its line, its text and its viewport.
        let opts = ExportOptions {
            layer_mode: mode,
            ..Default::default()
        };
        let dxf = dir.join("a.dxf");
        let report = export_file(&dxf, Some(Format::Dxf), &opts, &doc).unwrap();
        let text = std::fs::read_to_string(&dxf).unwrap();
        assert!(
            text.contains("PAPERTEXT"),
            "{mode:?}: the text of the sheet"
        );
        assert!(text.contains("4321.5"), "{mode:?}: the line of the sheet");
        assert!(text.contains("\nVIEWPORT\n"), "{mode:?}: the viewport");
        assert_eq!(report.total(), 4, "{mode:?}: {:?}", report.objects);
        assert!(report.skipped.is_empty(), "{mode:?}: {:?}", report.skipped);
        // With the option off, paper space is left out and the report says so.
        let off = ExportOptions {
            layer_mode: mode,
            sheets: false,
            ..Default::default()
        };
        let report = export_file(&dxf, Some(Format::Dxf), &off, &doc).unwrap();
        let text = std::fs::read_to_string(&dxf).unwrap();
        assert!(
            !text.contains("PAPERTEXT") && !text.contains("4321.5"),
            "{mode:?}"
        );
        assert_eq!(report.total(), 1);
        assert!(
            report
                .notes
                .iter()
                .any(|n| n.contains("sheet layers were left out"))
        );
    }
    let gj = dir.join("a.geojson");
    let report = export_file(&gj, Some(Format::GeoJson), &ExportOptions::default(), &doc).unwrap();
    let text = std::fs::read_to_string(&gj).unwrap();
    assert_eq!(report.total(), 1);
    assert!(!text.contains("PAPERTEXT") && !text.contains("4.3215"));
    std::fs::remove_dir_all(&dir).unwrap();
}
