//! Sheet layers as layouts (IOP-16): the export writes layouts with viewports and annotations,
//! and an import of that file gives the sheets back.

mod support;

use kadakas_core::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::corners;
use kadakas_core::model::{
    Attributes, ClassId, Color, Document, FIRST_LAYER, LayerId, LayerKind, Shape,
};
use kadakas_core::sheets::{ClassOverride, Crop, Orientation, PaperSize, Viewport};
use kadakas_io::dxf::DxfFormat;
use kadakas_io::{DxfVersion, ExportOptions, Exporter, LayerMode};
use support::{import, pairs};

fn add(
    ed: &mut Editor,
    shape: Shape,
    layer: Option<LayerId>,
    class: Option<ClassId>,
) -> kadakas_core::uid::Uid {
    ed.execute(&Command::AddObject {
        shape,
        layer,
        class,
        attrs: Attributes {
            pen: Some(Color([0, 0, 255, 255])),
            ..Attributes::default()
        },
    })
    .unwrap()
    .created[0]
}

fn sheet(
    ed: &mut Editor,
    name: &str,
    paper: PaperSize,
    orientation: Orientation,
    title: &str,
) -> LayerId {
    LayerId(
        ed.execute(&Command::AddSheet {
            name: name.into(),
            paper: Some(paper),
            orientation: Some(orientation),
            number: None,
            title: Some(title.into()),
        })
        .unwrap()
        .created[0],
    )
}

/// A model with a hidden class, and two sheets.
fn document() -> Document {
    let mut ed = Editor::with_sequential_ids();
    let walls = ClassId(
        ed.execute(&Command::AddClass {
            name: "Walls".into(),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    let old = ClassId(
        ed.execute(&Command::AddClass {
            name: "Old work".into(),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    add(
        &mut ed,
        Shape::Line {
            a: DVec2::new(0.0, 0.0),
            b: DVec2::new(20_000.0, 0.0),
        },
        None,
        Some(walls),
    );
    add(
        &mut ed,
        Shape::Line {
            a: DVec2::new(0.0, 0.0),
            b: DVec2::new(0.0, 12_000.0),
        },
        None,
        Some(old),
    );
    let s1 = sheet(
        &mut ed,
        "A-101",
        PaperSize::A3,
        Orientation::Landscape,
        "Site plan",
    );
    let s2 = sheet(
        &mut ed,
        "A-102",
        PaperSize::A4,
        Orientation::Portrait,
        "Details",
    );
    add(
        &mut ed,
        Shape::Rect {
            min: DVec2::new(10.0, 10.0),
            max: DVec2::new(410.0, 287.0),
        },
        Some(s1),
        None,
    );
    let mut vp = Viewport::new(
        DVec2::new(20.0, 30.0),
        DVec2::new(220.0, 170.0),
        DVec2::new(10_000.0, 6_000.0),
        100.0,
        vec![FIRST_LAYER],
    );
    vp.class_overrides.insert(
        old,
        ClassOverride {
            visible: Some(false),
            ..ClassOverride::default()
        },
    );
    vp.crop = Some(Crop::Polygon {
        points: vec![
            DVec2::new(20.0, 30.0),
            DVec2::new(220.0, 30.0),
            DVec2::new(120.0, 170.0),
        ],
    });
    vp.show_frame = true;
    add(&mut ed, Shape::Viewport(Box::new(vp)), Some(s1), None);
    // An isometric viewport and a text above both viewports.
    let mut iso = Viewport::new(
        DVec2::new(230.0, 30.0),
        DVec2::new(400.0, 130.0),
        DVec2::new(5_000.0, 5_000.0),
        200.0,
        vec![FIRST_LAYER],
    );
    iso.view = "right_isometric".into();
    iso.mode = "hidden_line".into();
    add(&mut ed, Shape::Viewport(Box::new(iso)), Some(s1), None);
    add(
        &mut ed,
        Shape::Text {
            at: DVec2::new(300.0, 20.0),
            text: "Site plan 1:100".into(),
            height: 5.0,
        },
        Some(s1),
        Some(walls),
    );
    add(
        &mut ed,
        Shape::Circle {
            center: DVec2::new(50.0, 200.0),
            radius: 8.0,
        },
        Some(s1),
        None,
    );
    add(
        &mut ed,
        Shape::Polyline {
            points: corners(&[
                DVec2::new(0.0, 0.0),
                DVec2::new(50.0, 0.0),
                DVec2::new(50.0, 20.0),
            ]),
            closed: false,
        },
        Some(s2),
        None,
    );
    ed.doc().clone()
}

fn export(doc: &Document, opts: &ExportOptions) -> (String, kadakas_io::Report) {
    let r = DxfFormat.export(doc, opts).unwrap();
    (String::from_utf8(r.bytes).unwrap(), r.report)
}

fn count(t: &str, entity: &str) -> usize {
    pairs(t)
        .iter()
        .filter(|(c, v)| *c == 0 && v.trim() == entity)
        .count()
}

#[test]
fn the_sheets_are_layouts_with_viewports() {
    let doc = document();
    let (t, rep) = export(&doc, &ExportOptions::default());
    assert!(rep.skipped.is_empty(), "{:?}", rep.skipped);
    // Model, A-101 and A-102 as layouts. The overall viewport and two user viewports in the first.
    assert_eq!(count(&t, "LAYOUT"), 3);
    assert_eq!(
        count(&t, "VIEWPORT"),
        2 + 2,
        "two of the user and one of the paper itself for each layout"
    );
    // The names of the layouts are the titles.
    assert!(t.contains("\nSite plan\n") && t.contains("\nDetails\n"));
    assert!(
        t.contains("*Paper_Space0"),
        "the second layout has its own block"
    );
    // The entities of the paper space have the flag.
    let p = pairs(&t);
    assert!(p.iter().filter(|(c, v)| *c == 67 && v == "1").count() >= 8);
    assert_eq!(rep.objects.get("viewport"), Some(&2));
    assert!(
        rep.notes
            .iter()
            .any(|n| n == "2 sheet layers were written as layouts: Site plan, Details"),
        "{:?}",
        rep.notes
    );
    // The clip outline of the cropped viewport is an LWPOLYLINE on Defpoints.
    assert!(p.iter().any(|(c, v)| *c == 8 && v == "Defpoints"));
}

#[test]
fn an_export_and_an_import_give_the_sheets_back() {
    let doc = document();
    for version in [DxfVersion::R2000, DxfVersion::R2004, DxfVersion::R2018] {
        let (t, _) = export(
            &doc,
            &ExportOptions {
                version,
                ..ExportOptions::default()
            },
        );
        let (ed, rep) = import(t.as_bytes()).unwrap_or_else(|e| panic!("{version:?}: {e}"));
        let back = ed.doc();
        assert_eq!(
            rep.entities_read,
            rep.entities_imported + rep.entities_skipped,
            "{version:?} {rep:?}"
        );
        let sheets: Vec<_> = back
            .layers
            .iter()
            .filter(|l| l.kind == LayerKind::Sheet)
            .collect();
        assert_eq!(
            sheets.len(),
            2,
            "{version:?}: {:?} {:?}",
            rep.notes,
            rep.skipped
        );
        let a = sheets
            .iter()
            .find(|l| back.sheet(l.id).unwrap().title == "Site plan")
            .expect("Site plan");
        let b = sheets
            .iter()
            .find(|l| back.sheet(l.id).unwrap().title == "Details")
            .expect("Details");
        let sa = back.sheet(a.id).unwrap();
        assert_eq!(
            (sa.paper, sa.orientation),
            (PaperSize::A3, Orientation::Landscape),
            "{version:?}"
        );
        let sb = back.sheet(b.id).unwrap();
        assert_eq!(
            (sb.paper, sb.orientation),
            (PaperSize::A4, Orientation::Portrait),
            "{version:?}"
        );
        // The order of the objects of the first sheet: the frame, the viewports, text, circle.
        let kinds: Vec<&str> = back
            .objects
            .values()
            .filter(|o| o.layer == a.id)
            .map(|o| o.shape.kind())
            .collect();
        assert_eq!(
            kinds,
            vec!["polyline", "viewport", "viewport", "text", "circle"],
            "{version:?}"
        );
        let vps: Vec<_> = back.viewports_of(a.id).map(|(_, v)| v.clone()).collect();
        let v = &vps[0];
        assert_eq!(
            (v.min, v.max),
            (DVec2::new(20.0, 30.0), DVec2::new(220.0, 170.0)),
            "{version:?}"
        );
        assert!((v.scale - 100.0).abs() < 1e-6, "{}", v.scale);
        assert!(
            (v.center - DVec2::new(10_000.0, 6_000.0)).length() < 1e-6,
            "{:?}",
            v.center
        );
        assert!(v.is_plan() && v.show_frame);
        let Some(Crop::Polygon { points }) = &v.crop else {
            panic!("{version:?}: {:?}", v.crop)
        };
        assert_eq!(points.len(), 3);
        assert!((points[2] - DVec2::new(120.0, 170.0)).length() < 1e-6);
        // The hidden class is frozen in the viewport.
        let old = back.classes.iter().find(|c| c.name == "Old work").unwrap();
        assert_eq!(
            v.class_overrides[&old.id].visible,
            Some(false),
            "{version:?}"
        );
        let iso = &vps[1];
        assert_eq!(iso.view, "right_isometric");
        assert_eq!(iso.mode, "hidden_line");
        assert!((iso.scale - 200.0).abs() < 1e-6);
        assert!(!iso.show_frame, "no frame: the viewport was on Defpoints");
        // The text is in the class Walls, on the sheet, at its place.
        let text = back
            .objects
            .values()
            .find(|o| o.layer == a.id && o.shape.kind() == "text")
            .unwrap();
        assert_eq!(back.class(text.class).unwrap().name, "Walls");
        assert!(
            matches!(&text.shape, Shape::Text { at, text, .. } if *at == DVec2::new(300.0, 20.0) && text == "Site plan 1:100")
        );
    }
}

#[test]
fn units_and_the_georeference_do_not_touch_paper_space() {
    let mut doc = document();
    doc.georef = Some(kadakas_core::georef::Georeference {
        crs: "EPSG:3067".into(),
        origin: DVec2::new(385_000.0, 6_672_000.0),
        rotation: 0.0,
        user_origin: DVec2::ZERO,
    });
    let (t, _) = export(
        &doc,
        &ExportOptions {
            unit: kadakas_io::Unit::M,
            ..ExportOptions::default()
        },
    );
    let (ed, _) = import(t.as_bytes()).unwrap();
    let back = ed.doc();
    let a = back
        .layers
        .iter()
        .find(|l| l.kind == LayerKind::Sheet && back.sheet(l.id).unwrap().title == "Site plan")
        .unwrap();
    // The frame of the sheet is still in paper millimeters.
    let rect = back.objects.values().find(|o| o.layer == a.id).unwrap();
    assert!(
        matches!(&rect.shape, Shape::Polyline { points, .. } if points.iter().any(|p| p.p == DVec2::new(10.0, 10.0))),
        "{:?}",
        rect.shape
    );
    let (_, v) = back.viewports_of(a.id).next().unwrap();
    assert_eq!(
        (v.min, v.max),
        (DVec2::new(20.0, 30.0), DVec2::new(220.0, 170.0))
    );
    assert!((v.scale - 100.0).abs() < 1e-6);
    // 10,000 mm with the origin of the georeference shifted back in on export and out on import.
    assert!(v.center.is_finite());
}

#[test]
fn release_12_has_no_layouts_and_says_so() {
    let doc = document();
    let (t, rep) = export(
        &doc,
        &ExportOptions {
            version: DxfVersion::R12,
            ..ExportOptions::default()
        },
    );
    assert_eq!(count(&t, "VIEWPORT"), 0);
    assert!(
        rep.notes
            .iter()
            .any(|n| n.contains("sheet layers") && n.contains("release 12")),
        "{:?}",
        rep.notes
    );
}

#[test]
fn layers_mode_and_visible_only_work_on_sheets() {
    let doc = document();
    let (t, _) = export(
        &doc,
        &ExportOptions {
            layer_mode: LayerMode::Layers,
            ..ExportOptions::default()
        },
    );
    assert_eq!(count(&t, "LAYOUT"), 3);
    // In the mode layers the objects of a sheet are on the layer of the sheet.
    assert!(pairs(&t).iter().any(|(c, v)| *c == 8 && v == "A-101"));
    let mut hidden = doc.clone();
    let b = hidden.sheet_layers().nth(1).unwrap().id;
    hidden
        .layers
        .iter_mut()
        .find(|l| l.id == b)
        .unwrap()
        .visible = false;
    let (_, all) = export(&hidden, &ExportOptions::default());
    let (_, rep) = export(
        &hidden,
        &ExportOptions {
            visible_only: true,
            ..ExportOptions::default()
        },
    );
    // The polyline is on the second sheet, which is hidden.
    assert_eq!(all.objects.get("polyline"), Some(&1));
    assert_eq!(rep.objects.get("polyline"), None);
    assert_eq!(rep.objects.get("rect"), Some(&1));
}

fn sheet_doc() -> (Document, LayerId, LayerId) {
    // Two design layers, a sheet with a title block, a viewport with annotations and a crop.
    let mut ed = Editor::with_sequential_ids();
    let l2 = LayerId(
        ed.execute(&Command::AddLayer {
            name: "Roofs".into(),
            elevation: 3000.0,
            scale: None,
            kind: None,
        })
        .unwrap()
        .created[0],
    );
    add(
        &mut ed,
        Shape::Line {
            a: DVec2::new(0.0, 0.0),
            b: DVec2::new(5000.0, 0.0),
        },
        None,
        None,
    );
    add(
        &mut ed,
        Shape::Line {
            a: DVec2::new(0.0, 0.0),
            b: DVec2::new(0.0, 5000.0),
        },
        Some(l2),
        None,
    );
    let s = sheet(&mut ed, "S-1", PaperSize::A3, Orientation::Landscape, "");
    ed.execute(&Command::SetProject {
        name: Some("Riverside".into()),
        client: None,
        address: None,
        drawn_by: Some("NM".into()),
        date: None,
        revision: None,
    })
    .unwrap();
    add(
        &mut ed,
        Shape::TitleBlock(kadakas_core::sheets::TitleBlock {
            grid_columns: 4,
            grid_rows: 3,
            ..Default::default()
        }),
        Some(s),
        None,
    );
    let mut vp = Viewport::new(
        DVec2::new(20.0, 30.0),
        DVec2::new(220.0, 170.0),
        DVec2::new(2500.0, 2500.0),
        50.0,
        vec![FIRST_LAYER],
    );
    vp.mode = "wireframe".into();
    vp.view = "front".into();
    vp.layer_overrides.insert(FIRST_LAYER, true);
    vp.crop = Some(Crop::Rect {
        min: DVec2::new(30.0, 40.0),
        max: DVec2::new(200.0, 160.0),
    });
    for (i, item) in [
        kadakas_core::sheets::AnnotationItem::Line {
            a: DVec2::new(5.0, 5.0),
            b: DVec2::new(50.0, 5.0),
        },
        kadakas_core::sheets::AnnotationItem::Text {
            at: DVec2::new(5.0, 10.0),
            text: "North \u{d5}".into(),
            height: 3.0,
        },
        kadakas_core::sheets::AnnotationItem::Dimension {
            a: DVec2::new(10.0, 20.0),
            b: DVec2::new(60.0, 20.0),
            offset: 6.0,
            text: None,
        },
        kadakas_core::sheets::AnnotationItem::Dimension {
            a: DVec2::new(10.0, 30.0),
            b: DVec2::new(60.0, 30.0),
            offset: 6.0,
            text: Some("5 m".into()),
        },
        kadakas_core::sheets::AnnotationItem::Shape {
            shape: Box::new(Shape::Circle {
                center: DVec2::new(30.0, 60.0),
                radius: 4.0,
            }),
        },
        kadakas_core::sheets::AnnotationItem::Shape {
            shape: Box::new(Shape::Locus3d {
                p: kadakas_core::DVec3::ZERO,
            }),
        },
    ]
    .into_iter()
    .enumerate()
    {
        vp.annotations.push(kadakas_core::sheets::Annotation {
            id: i as u32 + 1,
            class: ClassId(kadakas_core::uid::Uid::NIL),
            attrs: Attributes::default(),
            item,
        });
    }
    add(&mut ed, Shape::Viewport(Box::new(vp)), Some(s), None);
    // A viewport that shows only the second layer, and one with a section.
    let only2 = Viewport::new(
        DVec2::new(230.0, 30.0),
        DVec2::new(400.0, 130.0),
        DVec2::new(0.0, 0.0),
        100.0,
        vec![l2],
    );
    add(&mut ed, Shape::Viewport(Box::new(only2)), Some(s), None);
    (ed.doc().clone(), s, l2)
}

#[test]
fn title_blocks_and_viewport_annotations_are_written() {
    let (doc, _, _) = sheet_doc();
    for mode in [LayerMode::Classes, LayerMode::Layers] {
        let (t, rep) = export(
            &doc,
            &ExportOptions {
                layer_mode: mode,
                ..ExportOptions::default()
            },
        );
        assert!(
            t.contains("Riverside"),
            "{mode:?}: the project in the title block"
        );
        // The border, the block and the cells are polylines. The grid letters are texts.
        assert!(
            count(&t, "LWPOLYLINE") >= 5,
            "{mode:?}: {}",
            count(&t, "LWPOLYLINE")
        );
        assert!(
            pairs(&t).iter().any(|(c, v)| *c == 1 && v == "A"),
            "{mode:?}: a grid letter"
        );
        // The annotations of the viewport: a line, two texts (and the text of two dimensions), a circle.
        assert!(t.contains("North \\U+00D5"), "{mode:?}");
        assert!(t.contains("5 m"), "{mode:?}");
        assert!(rep.skipped.is_empty(), "{mode:?}: {:?}", rep.skipped);
        // The viewports: wireframe front view with a rectangular crop.
        let p = pairs(&t);
        assert!(p.iter().any(|(c, v)| *c == 281 && v == "1"), "wireframe");
        assert!(p.iter().any(|(c, _)| *c == 340), "the crop");
        if mode == LayerMode::Layers {
            // The viewport that shows only Roofs freezes Layer 1, and the other freezes nothing.
            assert!(p.iter().filter(|(c, _)| *c == 331).count() >= 1);
        } else {
            assert!(
                rep.notes
                    .iter()
                    .any(|n| n.contains("shows some design layers only")),
                "{:?}",
                rep.notes
            );
        }
        // And the file reads again, with the title block as lines and text.
        let (ed, back) = import(t.as_bytes()).unwrap();
        assert_eq!(
            back.entities_read,
            back.entities_imported + back.entities_skipped,
            "{mode:?}"
        );
        assert!(ed.doc().layers.iter().any(|l| l.kind == LayerKind::Sheet));
    }
}

#[test]
fn a_section_viewport_and_odd_sheets_are_reported() {
    let (mut doc, s, _) = sheet_doc();
    // A section viewport has no DXF form.
    let line = doc
        .objects
        .values()
        .find(|o| matches!(o.shape, Shape::Line { .. }))
        .unwrap()
        .id;
    let mut v = Viewport::new(
        DVec2::new(10.0, 200.0),
        DVec2::new(100.0, 280.0),
        DVec2::ZERO,
        50.0,
        vec![FIRST_LAYER],
    );
    v.section = Some(kadakas_core::section::SectionView::new(line));
    let id = kadakas_core::model::ObjectId(kadakas_core::uid::Uid::from_num(9000));
    doc.objects.insert(
        id,
        kadakas_core::model::Object::new(
            id,
            s,
            ClassId(kadakas_core::uid::Uid::NIL),
            Shape::Viewport(Box::new(v)),
        ),
    );
    // A sheet with a name that is also the name of the model, and a hidden viewport layer.
    let mut ed = Editor::new(doc);
    let model = sheet(
        &mut ed,
        "Model",
        PaperSize::Letter,
        Orientation::Portrait,
        "Model",
    );
    add(
        &mut ed,
        Shape::Text {
            at: DVec2::new(10.0, 10.0),
            text: "m".into(),
            height: 3.0,
        },
        Some(model),
        None,
    );
    let again = sheet(
        &mut ed,
        "Other",
        PaperSize::A4,
        Orientation::Landscape,
        "Model",
    );
    add(
        &mut ed,
        Shape::Text {
            at: DVec2::new(10.0, 10.0),
            text: "m2".into(),
            height: 3.0,
        },
        Some(again),
        None,
    );
    let (t, rep) = export(ed.doc(), &ExportOptions::default());
    assert!(
        rep.skipped
            .iter()
            .any(|s| s.entity == "viewport" && s.reason.contains("section")),
        "{:?}",
        rep.skipped
    );
    // The layout names are unique, and none is "Model".
    let p = pairs(&t);
    let names: Vec<&String> = p.iter().filter(|(c, _)| *c == 1).map(|(_, v)| v).collect();
    assert!(names.iter().any(|n| *n == "Model 2"), "{names:?}");
    assert!(names.iter().any(|n| *n == "Model 3"), "{names:?}");
    let (back, _) = import(t.as_bytes()).unwrap();
    assert_eq!(
        back.doc()
            .layers
            .iter()
            .filter(|l| l.kind == LayerKind::Sheet)
            .count(),
        3
    );
}
