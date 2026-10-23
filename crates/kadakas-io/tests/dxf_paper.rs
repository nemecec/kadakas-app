//! Paper space (IOP-16): a layout becomes a sheet layer, its viewports become viewports with
//! their scale, view and layers, and its annotations stay on the sheet in the same order.

mod support;

use kadakas_core::model::{LayerKind, Shape};
use kadakas_core::sheets::{Orientation, PaperSize};
use kadakas_core::{DVec2, DVec3};
use kadakas_io::ImportOptions;
use support::{import, import_with};

fn layout_object(
    handle: &str,
    record: &str,
    name: &str,
    tab: i32,
    w: f64,
    h: f64,
    units: i32,
) -> String {
    format!(
        "0\nLAYOUT\n5\n{handle}\n330\n1D\n100\nAcDbPlotSettings\n1\n\n4\nA3\n40\n7.5\n41\n20\n42\n7.5\n43\n20\n44\n{w}\n45\n{h}\n72\n{units}\n73\n0\n\
100\nAcDbLayout\n1\n{name}\n70\n1\n71\n{tab}\n10\n0\n20\n0\n11\n{w}\n21\n{h}\n330\n{record}\n"
    )
}

fn viewport(layer: &str, id: i32, center: (f64, f64), size: (f64, f64), extra: &str) -> String {
    format!(
        "0\nVIEWPORT\n8\n{layer}\n67\n1\n10\n{}\n20\n{}\n30\n0\n40\n{}\n41\n{}\n68\n1\n69\n{id}\n{extra}",
        center.0, center.1, size.0, size.1
    )
}

/// A drawing in meters with a plan and one layout A101 (an A3 sheet in mm).
fn drawing(extra_paper: &str, extra_header: &str) -> String {
    let blocks = "0\nBLOCK\n5\n30\n330\n3A\n8\n0\n2\n*Paper_Space\n70\n0\n10\n0\n20\n0\n30\n0\n0\nENDBLK\n5\n31\n\
0\nBLOCK\n5\n32\n330\n3B\n8\n0\n2\n*Paper_Space0\n70\n0\n10\n0\n20\n0\n30\n0\n\
0\nTEXT\n8\nTITLE\n10\n20\n20\n20\n40\n5\n1\nSecond sheet\n0\nENDBLK\n5\n33\n\
0\nBLOCK\n5\n34\n8\n0\n2\nTITLEBLOCK\n10\n0\n20\n0\n30\n0\n\
0\nLINE\n8\nTITLE\n10\n0\n20\n0\n11\n100\n21\n0\n0\nLINE\n8\nTITLE\n10\n100\n20\n0\n11\n100\n21\n30\n\
0\nTEXT\n8\nTITLE\n10\n5\n20\n5\n40\n3.5\n1\nProject\n0\nENDBLK\n";
    let tables = "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n\
0\nLAYER\n5\n40\n2\nWalls\n70\n0\n62\n1\n\
0\nLAYER\n5\n41\n2\nOld\n70\n1\n62\n2\n\
0\nLAYER\n5\n42\n2\nTITLE\n70\n0\n62\n7\n0\nENDTAB\n0\nENDSEC\n";
    let objects = format!(
        "0\nSECTION\n2\nOBJECTS\n{}{}0\nENDSEC\n",
        layout_object("50", "3A", "A101", 1, 420.0, 297.0, 1),
        layout_object("51", "3B", "A102", 2, 297.0, 420.0, 1)
    );
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n9\n$INSUNITS\n70\n6\n{extra_header}0\nENDSEC\n{tables}0\nSECTION\n2\nBLOCKS\n{blocks}0\nENDSEC\n\
0\nSECTION\n2\nENTITIES\n\
0\nLINE\n8\nWalls\n10\n0\n20\n0\n11\n100\n21\n0\n\
0\nLINE\n8\nWalls\n10\n100\n20\n0\n11\n100\n21\n60\n\
0\nLINE\n8\nOld\n10\n0\n20\n0\n11\n0\n21\n60\n\
0\nLINE\n8\nTITLE\n67\n1\n10\n10\n20\n10\n11\n410\n21\n10\n\
{}\
0\nTEXT\n8\nTITLE\n67\n1\n10\n300\n20\n20\n40\n7\n1\nSite plan\n\
0\nINSERT\n8\nTITLE\n67\n1\n2\nTITLEBLOCK\n10\n300\n20\n12\n\
{extra_paper}0\nENDSEC\n{objects}0\nEOF\n",
        viewport("TITLE", 1, (210.0, 148.5), (420.0, 297.0), "")
            + &viewport(
                "TITLE",
                2,
                (150.0, 150.0),
                (200.0, 140.0),
                "12\n50\n22\n50\n45\n70\n17\n0\n27\n0\n37\n0\n16\n0\n26\n0\n36\n1\n"
            )
    )
}

#[test]
fn a_layout_becomes_a_sheet_layer_with_its_viewport_and_annotations() {
    let (ed, rep) = import(drawing("", "").as_bytes()).unwrap();
    let doc = ed.doc();
    assert_eq!(
        rep.entities_read,
        rep.entities_imported + rep.entities_skipped,
        "{rep:?}"
    );
    assert!(rep.skipped.is_empty(), "{:?}", rep.skipped);
    let sheets: Vec<_> = doc
        .layers
        .iter()
        .filter(|l| l.kind == LayerKind::Sheet)
        .collect();
    assert_eq!(sheets.len(), 2, "{:?}", rep.notes);
    let (a101, a102) = (sheets[0], sheets[1]);
    assert_eq!((a101.name.as_str(), a102.name.as_str()), ("A101", "A102"));
    assert_eq!(a101.scale, 1.0);
    // Page settings: A3 landscape, and A3 portrait.
    let s = doc.sheet(a101.id).unwrap();
    assert_eq!(
        (s.paper, s.orientation),
        (PaperSize::A3, Orientation::Landscape)
    );
    assert_eq!(s.title, "A101");
    let s = doc.sheet(a102.id).unwrap();
    assert_eq!(
        (s.paper, s.orientation),
        (PaperSize::A3, Orientation::Portrait)
    );
    // The viewport: 200 x 140 mm at 150, 150. The view is 70 m high: 70,000 mm on 140 mm of paper
    // is 1:500. The view center (50, 50) m is the middle of the frame.
    let vps: Vec<_> = doc.viewports_of(a101.id).collect();
    assert_eq!(vps.len(), 1);
    let (_, v) = vps[0];
    assert_eq!(
        (v.min, v.max),
        (DVec2::new(50.0, 80.0), DVec2::new(250.0, 220.0))
    );
    assert!((v.scale - 500.0).abs() < 1e-9, "{}", v.scale);
    assert!(v.is_plan());
    assert_eq!(v.layers.len(), 1);
    // The model was shifted by the import (the center is far from 0 only if the drawing is); the
    // numbers of the frame are the same as the objects of the model: 50 m is 50,000 mm.
    assert!(
        (v.center - DVec2::new(50_000.0, 50_000.0)).length() < 1e-6 || ed.doc().georef.is_some()
    );
    // The layer Old is off: the class is switched off in the viewport too.
    let off = doc.classes.iter().find(|c| c.name == "Old").unwrap();
    assert!(!off.visible);
    assert_eq!(v.class_overrides[&off.id].visible, Some(false));
    let walls = doc.classes.iter().find(|c| c.name == "Walls").unwrap();
    assert!(!v.class_overrides.contains_key(&walls.id));
    // Annotations: the border line, the text, and the title block (2 lines and a text) are on the
    // sheet, in the order of the file, with the viewport in its place after the border.
    let on_sheet: Vec<&Shape> = doc
        .objects
        .values()
        .filter(|o| o.layer == a101.id)
        .map(|o| &o.shape)
        .collect();
    let kinds: Vec<&str> = on_sheet.iter().map(|s| s.kind()).collect();
    assert_eq!(
        kinds,
        vec!["line", "viewport", "text", "line", "line", "text"],
        "{kinds:?}"
    );
    let Shape::Line { a, b } = on_sheet[0] else {
        panic!()
    };
    assert_eq!((*a, *b), (DVec2::new(10.0, 10.0), DVec2::new(410.0, 10.0)));
    // The title block was placed at (300, 12).
    let Shape::Line { a, b } = on_sheet[3] else {
        panic!()
    };
    assert_eq!((*a, *b), (DVec2::new(300.0, 12.0), DVec2::new(400.0, 12.0)));
    // The second layout has its text.
    assert_eq!(
        doc.objects.values().filter(|o| o.layer == a102.id).count(),
        1
    );
    // The model objects are not on a sheet.
    assert_eq!(
        doc.objects
            .values()
            .filter(|o| doc.layer(o.layer).unwrap().kind == LayerKind::Design)
            .count(),
        3
    );
    let n = rep.notes.join("\n");
    assert!(
        n.contains(
            "layout 'A101' of paper space became a sheet layer with 5 entities and 1 viewport"
        ),
        "{n}"
    );
    assert!(
        n.contains(
            "layout 'A102' of paper space became a sheet layer with 1 entity and 0 viewports"
        ),
        "{n}"
    );
    assert_eq!(rep.objects.get("viewport"), Some(&1));
}

#[test]
fn the_option_can_leave_paper_space_out_and_it_is_counted() {
    let (ed, rep) = import_with(
        drawing("", "").as_bytes(),
        &ImportOptions {
            paper_space: false,
            ..ImportOptions::default()
        },
    )
    .unwrap();
    assert!(ed.doc().layers.iter().all(|l| l.kind == LayerKind::Design));
    assert_eq!(
        rep.entities_read,
        rep.entities_imported + rep.entities_skipped
    );
    let n: usize = rep
        .skipped
        .iter()
        .filter(|s| s.reason == "in paper space")
        .map(|s| s.count)
        .sum();
    // Five entities of the first layout (border, two viewports, text, title block) and one of the second.
    assert_eq!(n, 6, "{:?}", rep.skipped);
}

#[test]
fn a_viewport_keeps_its_frozen_layers_its_view_and_its_crop() {
    // The viewport freezes the layer Walls (handle 40), looks from the right isometric
    // direction, and is cropped by a polyline (handle 70).
    let clip = "0\nLWPOLYLINE\n5\n70\n8\nDefpoints\n67\n1\n90\n3\n70\n1\n10\n100\n20\n80\n10\n240\n20\n80\n10\n170\n20\n210\n";
    let vp = viewport(
        "Defpoints",
        3,
        (170.0, 145.0),
        (140.0, 130.0),
        "12\n10\n22\n10\n45\n26\n17\n0\n27\n0\n37\n0\n16\n1\n26\n-1\n36\n1\n331\n40\n340\n70\n90\n17\n51\n15\n",
    );
    let (ed, rep) = import(drawing(&format!("{vp}{clip}"), "").as_bytes()).unwrap();
    assert_eq!(
        rep.entities_read,
        rep.entities_imported + rep.entities_skipped,
        "{rep:?}"
    );
    let doc = ed.doc();
    let a101 = doc.layers.iter().find(|l| l.name == "A101").unwrap();
    let vps: Vec<_> = doc.viewports_of(a101.id).collect();
    assert_eq!(vps.len(), 2);
    let v = vps[1].1;
    assert_eq!(v.view, "right_isometric");
    assert!(!v.show_frame, "the layer Defpoints does not plot");
    let walls = doc.classes.iter().find(|c| c.name == "Walls").unwrap();
    assert_eq!(v.class_overrides[&walls.id].visible, Some(false));
    // The first viewport still shows the layer Walls.
    assert!(!vps[0].1.class_overrides.contains_key(&walls.id));
    // 26 m high on 130 mm is 1:200.
    assert!((v.scale - 200.0).abs() < 1e-9, "{}", v.scale);
    // The crop is the polyline, in sheet mm. The polyline is not an object of its own.
    let Some(kadakas_core::sheets::Crop::Polygon { points }) = &v.crop else {
        panic!("{:?}", v.crop)
    };
    assert_eq!(
        points,
        &vec![
            DVec2::new(100.0, 80.0),
            DVec2::new(240.0, 80.0),
            DVec2::new(170.0, 210.0)
        ]
    );
    assert!(
        rep.skipped
            .iter()
            .any(|s| s.entity == "LWPOLYLINE" && s.reason.contains("crops a viewport")),
        "{:?}",
        rep.skipped
    );
    let n = rep.notes.join("\n");
    assert!(n.contains("1 viewport has a twist"), "{n}");
    assert!(n.contains("1 viewport has a perspective view"), "{n}");
}

#[test]
fn a_viewport_that_is_off_or_has_no_size_is_reported() {
    let off = viewport(
        "TITLE",
        4,
        (100.0, 100.0),
        (50.0, 50.0),
        "45\n10\n17\n0\n27\n0\n37\n0\n16\n0\n26\n0\n36\n1\n",
    )
    .replace("68\n1\n", "68\n0\n");
    let nosize = viewport(
        "TITLE",
        5,
        (100.0, 100.0),
        (50.0, 50.0),
        "17\n0\n27\n0\n37\n0\n",
    );
    let (ed, rep) = import(drawing(&format!("{off}{nosize}"), "").as_bytes()).unwrap();
    assert_eq!(
        rep.entities_read,
        rep.entities_imported + rep.entities_skipped
    );
    let reasons: Vec<&str> = rep
        .skipped
        .iter()
        .filter(|s| s.entity == "VIEWPORT")
        .map(|s| s.reason.as_str())
        .collect();
    assert_eq!(reasons.len(), 2, "{reasons:?}");
    assert!(reasons.iter().any(|r| r.contains("turned off")));
    assert!(reasons.iter().any(|r| r.contains("without a size")));
    let a101 = ed.doc().layers.iter().find(|l| l.name == "A101").unwrap();
    assert_eq!(ed.doc().viewports_of(a101.id).count(), 1);
}

#[test]
fn a_file_with_paper_space_entities_and_no_layout_object_gets_a_sheet() {
    // Release 12 style: entities with 67 = 1 and no blocks, no objects. Inches in paper space
    // would need a layout, so the sheet is A3 in mm.
    let text = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n0\n\
0\nLINE\n8\n0\n67\n1\n10\n0\n20\n0\n11\n100\n21\n0\n\
0\nPOLYLINE\n8\n0\n67\n1\n70\n8\n0\nVERTEX\n8\n0\n10\n0\n20\n0\n30\n5\n0\nVERTEX\n8\n0\n10\n10\n20\n10\n30\n7\n0\nSEQEND\n8\n0\n\
0\nPOINT\n8\n0\n67\n1\n10\n1\n20\n1\n30\n1\n0\nENDSEC\n0\nEOF\n";
    let (ed, rep) = import(text.as_bytes()).unwrap();
    assert_eq!(
        rep.entities_read,
        rep.entities_imported + rep.entities_skipped,
        "{rep:?}"
    );
    let sheet = ed
        .doc()
        .layers
        .iter()
        .find(|l| l.kind == LayerKind::Sheet)
        .expect("a sheet");
    assert_eq!(sheet.name, "Paper space");
    let kinds: Vec<&str> = ed
        .doc()
        .objects
        .values()
        .filter(|o| o.layer == sheet.id)
        .map(|o| o.shape.kind())
        .collect();
    // The 3D polyline is flattened. The point has no 2D shape: it is reported.
    assert_eq!(kinds, vec!["line", "polyline"]);
    assert!(
        rep.skipped
            .iter()
            .any(|s| s.entity == "POINT" && s.reason.contains("sheet layer"))
    );
    let n = rep.notes.join("\n");
    assert!(
        n.contains("3D polyline in paper space was flattened"),
        "{n}"
    );
    assert!(n.contains("no paper size"), "{n}");
    let _ = DVec3::ZERO;
}

#[test]
fn a_layout_in_inches_is_converted_to_mm() {
    let objects = format!(
        "0\nSECTION\n2\nOBJECTS\n{}0\nENDSEC\n",
        layout_object("50", "3A", "Plot", 1, 17.0, 11.0, 0)
    );
    let text = format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n5\n30\n330\n3A\n8\n0\n2\n*Paper_Space\n70\n0\n10\n0\n20\n0\n0\nENDBLK\n0\nENDSEC\n\
0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n67\n1\n10\n1\n20\n1\n11\n16\n21\n10\n\
{}0\nENDSEC\n{objects}0\nEOF\n",
        // A viewport of 8 by 6 inches at 4.25, 5.5, with a view 96 inches (8 feet) high: the scale
        // is 96 / 6 = 16, that is 1/16" = 1' (the viewport is in inches).
        viewport(
            "0",
            2,
            (4.25, 5.5),
            (8.0, 6.0),
            "12\n0\n22\n0\n45\n96\n17\n0\n27\n0\n37\n0\n16\n0\n26\n0\n36\n1\n"
        )
    );
    let (ed, rep) = import(text.as_bytes()).unwrap();
    let sheet = ed
        .doc()
        .layers
        .iter()
        .find(|l| l.kind == LayerKind::Sheet)
        .expect("a sheet");
    let s = ed.doc().sheet(sheet.id).unwrap();
    // 17 x 11 inch is 431.8 x 279.4 mm: the tabloid (11 x 17) paper in landscape.
    assert_eq!(
        (s.paper, s.orientation),
        (PaperSize::Tabloid, Orientation::Landscape)
    );
    let Shape::Line { a, b } = ed
        .doc()
        .objects
        .values()
        .find(|o| o.layer == sheet.id)
        .unwrap()
        .shape
    else {
        panic!()
    };
    assert!(
        (a - DVec2::new(25.4, 25.4)).length() < 1e-9
            && (b - DVec2::new(406.4, 254.0)).length() < 1e-9
    );
    let (_, v) = ed.doc().viewports_of(sheet.id).next().unwrap();
    assert!((v.max - v.min - DVec2::new(203.2, 152.4)).length() < 1e-9);
    assert!((v.scale - 16.0).abs() < 1e-9, "{}", v.scale);
    assert!(rep.skipped.is_empty(), "{:?}", rep.skipped);
}

#[test]
fn layers_mode_old_style_frozen_layers_circle_clips_and_render_modes() {
    use kadakas_io::LayerMode;
    // Viewports: render modes, an MVIEW list of frozen layers (releases 13 and 14), a circle clip,
    // a clip that is an ellipse, and a view direction that is no standard view.
    let circle = "0\nCIRCLE\n5\n71\n8\nDefpoints\n67\n1\n10\n150\n20\n150\n40\n40\n";
    let ellipse =
        "0\nELLIPSE\n5\n72\n8\nDefpoints\n67\n1\n10\n150\n20\n150\n11\n20\n21\n0\n40\n0.5\n";
    let vps = [
        viewport("TITLE", 2, (150.0, 150.0), (80.0, 80.0), "45\n20\n17\n0\n27\n0\n37\n0\n281\n1\n1001\nACAD\n1000\nMVIEW\n1002\n{\n1003\nOld\n1002\n}\n340\n71\n"),
        viewport("TITLE", 3, (150.0, 150.0), (80.0, 80.0), "45\n20\n17\n0\n27\n0\n37\n0\n281\n2\n16\n0.3\n26\n-1\n36\n0.5\n340\n72\n"),
        viewport("TITLE", 4, (150.0, 150.0), (80.0, 80.0), "45\n20\n17\n0\n27\n0\n37\n0\n281\n4\n"),
    ]
    .concat();
    let text = drawing(&format!("{vps}{circle}{ellipse}"), "");
    let (ed, rep) = import_with(
        text.as_bytes(),
        &ImportOptions {
            layer_mode: LayerMode::Layers,
            ..ImportOptions::default()
        },
    )
    .unwrap();
    let doc = ed.doc();
    assert_eq!(
        rep.entities_read,
        rep.entities_imported + rep.entities_skipped,
        "{rep:?}"
    );
    let a101 = doc.layers.iter().find(|l| l.name == "A101").unwrap();
    let vps: Vec<_> = doc.viewports_of(a101.id).map(|(_, v)| v.clone()).collect();
    assert_eq!(vps.len(), 4);
    // The first viewport of `drawing` is the one at 150, 150 from `drawing`; ours are the last three.
    let (v1, v2, v3) = (&vps[1], &vps[2], &vps[3]);
    assert_eq!(
        (v1.mode.as_str(), v2.mode.as_str(), v3.mode.as_str()),
        ("wireframe", "hidden_line", "shaded")
    );
    // In the mode layers the layers are design layers: Old is hidden in the file and in v1 by name.
    let old = doc.layers.iter().find(|l| l.name == "Old").unwrap();
    assert_eq!(v1.layer_overrides.get(&old.id), Some(&false));
    assert!(v1.layers.contains(&old.id));
    // The circle crops the first, the ellipse crop is lost and the whole frame shows.
    assert!(
        matches!(&v1.crop, Some(kadakas_core::sheets::Crop::Polygon { points }) if points.len() == 64)
    );
    assert!(v2.crop.is_none());
    let n = rep.notes.join("\n");
    assert!(
        n.contains("cropped by an outline that is not a polyline or a circle"),
        "{n}"
    );
    assert!(n.contains("not a standard view"), "{n}");
    assert!(!v2.is_plan());
}

#[test]
fn a_layout_without_a_block_takes_its_name_from_the_layout_object() {
    let objects = format!(
        "0\nSECTION\n2\nOBJECTS\n{}0\nENDSEC\n",
        layout_object("50", "99", "Sheet one", 1, 420.0, 297.0, 1)
    );
    let text = format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n67\n1\n10\n0\n20\n0\n11\n100\n21\n0\n0\nENDSEC\n{objects}0\nEOF\n"
    );
    let (ed, _) = import(text.as_bytes()).unwrap();
    assert!(
        ed.doc()
            .layers
            .iter()
            .any(|l| l.kind == LayerKind::Sheet && l.name == "Sheet one")
    );
}
