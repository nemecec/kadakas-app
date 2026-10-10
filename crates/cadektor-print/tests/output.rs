//! Sheet output: the display list, PDF structure, SVG and PNG.

use cadektor_core::command::{Command, Editor};
use cadektor_core::curve::corners;
use cadektor_core::model::*;
use cadektor_core::resource::{ResourceBody, ResourceId};
use cadektor_core::sheets::*;
use cadektor_print::display::*;
use cadektor_print::png as sheet_png;
use cadektor_print::{
    PrintError, SheetCache, compose_page, export_pdf, select_sheets, svg, write_file,
};
use glam::{DVec2, DVec3};
use lopdf::Object as PdfObject;
use lopdf::content::Content;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

const RED: Color = Color([200, 0, 0, 255]);

fn run(ed: &mut Editor, c: Command) -> Vec<u64> {
    ed.execute(&c).unwrap().created
}

fn add(ed: &mut Editor, shape: Shape, layer: Option<LayerId>, attrs: Attributes) -> ObjectId {
    ObjectId(
        run(
            ed,
            Command::AddObject {
                shape,
                layer,
                class: None,
                attrs,
            },
        )[0],
    )
}

/// A model with one of most shapes on layer 1, and a sheet layer with a viewport.
fn project() -> (Editor, LayerId, ObjectId) {
    let mut ed = Editor::default();
    let red = Attributes {
        pen: Some(RED),
        fill: Some(Color([0, 200, 0, 255])),
        line_weight: Some(0.5),
        ..Default::default()
    };
    add(
        &mut ed,
        Shape::Rect {
            min: v(0.0, 0.0),
            max: v(10_000.0, 6000.0),
        },
        None,
        red.clone(),
    );
    add(
        &mut ed,
        Shape::Circle {
            center: v(3000.0, 3000.0),
            radius: 1000.0,
        },
        None,
        red.clone(),
    );
    add(
        &mut ed,
        Shape::Arc {
            center: v(6000.0, 3000.0),
            radius: 1000.0,
            start: 0.0,
            end: 3.0,
        },
        None,
        red.clone(),
    );
    add(
        &mut ed,
        Shape::Line {
            a: v(0.0, 0.0),
            b: v(10_000.0, 6000.0),
        },
        None,
        Attributes::default(),
    );
    let curved = vec![
        Vertex::corner(v(0.0, 7000.0)),
        Vertex::new(v(4000.0, 7000.0), VertexKind::Arc { radius: 500.0 }),
        Vertex::new(v(4000.0, 9000.0), VertexKind::Cubic),
        Vertex::corner(v(0.0, 9000.0)),
    ];
    add(
        &mut ed,
        Shape::Polyline {
            points: curved,
            closed: true,
        },
        None,
        red.clone(),
    );
    add(
        &mut ed,
        Shape::Polyline {
            points: corners(&[v(5000.0, 7000.0), v(6000.0, 8000.0), v(7000.0, 7000.0)]),
            closed: false,
        },
        None,
        Attributes::default(),
    );
    add(
        &mut ed,
        Shape::Text {
            at: v(100.0, 100.0),
            text: "Garden Bed".into(),
            height: 300.0,
        },
        None,
        Attributes::default(),
    );
    add(
        &mut ed,
        Shape::LandscapeArea {
            boundary: corners(&[v(8000.0, 0.0), v(9000.0, 0.0), v(9000.0, 1000.0)]),
            name: "Bed".into(),
        },
        None,
        Attributes::default(),
    );
    add(
        &mut ed,
        Shape::Hardscape {
            boundary: corners(&[v(8000.0, 2000.0), v(9000.0, 2000.0), v(9000.0, 3000.0)]),
            material: "Granite".into(),
            thickness: None,
            elevation: None,
        },
        None,
        Attributes::default(),
    );
    add(
        &mut ed,
        Shape::Locus3d {
            p: DVec3::new(500.0, 500.0, 2.0),
        },
        None,
        Attributes::default(),
    );
    add(
        &mut ed,
        Shape::Polyline3d {
            points: vec![DVec3::new(0.0, 0.0, 0.0), DVec3::new(1000.0, 0.0, 3.0)],
            closed: false,
        },
        None,
        Attributes::default(),
    );
    add(
        &mut ed,
        Shape::SiteModel {
            points: vec![
                DVec3::new(0.0, 0.0, 0.0),
                DVec3::new(1000.0, 0.0, 1.0),
                DVec3::new(0.0, 1000.0, 2.0),
            ],
            lines: vec![],
            settings: SiteSettings::default(),
        },
        None,
        Attributes::default(),
    );
    add(
        &mut ed,
        Shape::ExistingTree {
            at: v(2000.0, 5000.0),
            species: "Oak".into(),
            dbh: 400.0,
            spread: 5000.0,
            height: 9000.0,
            condition: Default::default(),
            action: cadektor_core::plants::tree::TreeAction::Remove,
            rpz_factor: None,
            rpz_cap: None,
        },
        None,
        Attributes::default(),
    );
    add(
        &mut ed,
        Shape::ExistingTree {
            at: v(9000.0, 5000.0),
            species: "Ash".into(),
            dbh: 300.0,
            spread: 4000.0,
            height: 9000.0,
            condition: Default::default(),
            action: Default::default(),
            rpz_factor: None,
            rpz_cap: None,
        },
        None,
        Attributes::default(),
    );
    let mut style = cadektor_core::plants::style::PlantStyle::default();
    style.id_code = "AC".into();
    style.fill = Some(Color([20, 140, 40, 255]));
    style.graphic = cadektor_core::plants::style::PlantGraphic::Cloud;
    let style_id = ResourceId(
        run(
            &mut ed,
            Command::AddResource {
                name: "Maple".into(),
                body: ResourceBody::PlantStyle(Box::new(style)),
            },
        )[0],
    );
    run(
        &mut ed,
        Command::AddPlants {
            style: Some(style_id),
            overrides: Default::default(),
            positions: vec![v(1000.0, 1500.0), v(1500.0, 1500.0)],
            layer: None,
            class: None,
            variation: None,
        },
    );
    add(
        &mut ed,
        Shape::Plant {
            at: v(4000.0, 1500.0),
            species: "Plain".into(),
            spread: 1000.0,
            height: 2000.0,
        },
        None,
        Attributes::default(),
    );
    add(
        &mut ed,
        Shape::PlantTag {
            at: v(1200.0, 2500.0),
            anchor: v(1000.0, 1500.0),
            style: style_id,
            height: 300.0,
        },
        None,
        Attributes::default(),
    );

    let sheet = LayerId(
        run(
            &mut ed,
            Command::AddSheet {
                name: "Plan".into(),
                paper: Some(PaperSize::A3),
                orientation: None,
                number: Some("L-101".into()),
                title: Some("Planting plan".into()),
            },
        )[0],
    );
    run(
        &mut ed,
        Command::SetProject {
            name: Some("Riverside garden".into()),
            client: None,
            address: None,
            drawn_by: Some("NM".into()),
            date: Some("2026-10-10".into()),
            revision: None,
        },
    );
    let mut vp = Viewport::new(
        v(20.0, 60.0),
        v(400.0, 280.0),
        v(5000.0, 4500.0),
        50.0,
        vec![FIRST_LAYER],
    );
    vp.fit(ed.doc());
    let vp_id = add(
        &mut ed,
        Shape::Viewport(Box::new(vp)),
        Some(sheet),
        Attributes::default(),
    );
    (ed, sheet, vp_id)
}

fn prims(ed: &Editor, sheet: LayerId) -> Vec<Prim> {
    compose_page(ed.doc(), sheet).unwrap().prims
}

fn count_paths(p: &[Prim]) -> usize {
    p.iter().filter(|x| matches!(x, Prim::Path(_))).count()
}

fn texts(p: &[Prim]) -> Vec<String> {
    p.iter()
        .filter_map(|x| {
            if let Prim::Text(t) = x {
                Some(t.text.clone())
            } else {
                None
            }
        })
        .collect()
}

fn set_viewport(ed: &mut Editor, id: ObjectId, f: impl FnOnce(&mut Viewport)) {
    let Shape::Viewport(vp) = &ed.doc().object(id).unwrap().shape else {
        panic!()
    };
    let mut vp = vp.clone();
    f(&mut vp);
    ed.execute(&Command::ModifyObject {
        id,
        shape: Some(Shape::Viewport(vp)),
        layer: None,
        class: None,
        attrs: None,
    })
    .unwrap();
}

// ---------------------------------------------------------------- display list

#[test]
fn the_plan_viewport_draws_every_shape_in_paper_space() {
    let (ed, sheet, _) = project();
    let page = compose_page(ed.doc(), sheet).unwrap();
    assert_eq!(page.size, v(420.0, 297.0));
    assert_eq!(page.title, "L-101 Planting plan");
    // The viewport is clipped to its frame.
    assert!(matches!(page.prims.first(), Some(Prim::PushClip(_))));
    assert!(matches!(page.prims.last(), Some(Prim::PopClip)));
    let t = texts(&page.prims);
    assert!(t.contains(&"Garden Bed".to_string()), "{t:?}");
    assert!(
        t.iter().any(|s| s.starts_with("AC (")),
        "the plant tag shows its code and count: {t:?}"
    );
    // Everything is drawn inside the frame (after the transform, plus line weight and arcs).
    for p in &page.prims {
        if let Prim::Path(pp) = p {
            for q in sample(&pp.segs, 8) {
                assert!(
                    q.x > -50.0 && q.x < 500.0 && q.y > -50.0 && q.y < 350.0,
                    "{q:?}"
                );
            }
        }
    }
    // Rect stroke weight and colors come from the object, in mm on paper.
    let rect = page.prims.iter().find_map(|p| match p {
        Prim::Path(pp) if pp.segs.len() == 4 && pp.closed && pp.fill == Some([0, 200, 0, 255]) => {
            Some(pp.clone())
        }
        _ => None,
    });
    let rect = rect.expect("the rectangle");
    assert_eq!((rect.stroke, rect.width), (Some([200, 0, 0, 255]), 0.5));
    // A viewport at 1:50 draws 10 m as 200 mm.
    let xs: Vec<f64> = rect
        .segs
        .iter()
        .map(|s| match s {
            PSeg::Move(p) | PSeg::Line(p) => p.x,
            _ => unreachable!(),
        })
        .collect();
    let width =
        xs.iter().cloned().fold(f64::MIN, f64::max) - xs.iter().cloned().fold(f64::MAX, f64::min);
    assert!(
        (width - 10_000.0 / 125.0).abs() < 1e-6 || (width - 10_000.0 / 50.0).abs() < 1e-6,
        "{width}"
    );
}

#[test]
fn objects_outside_the_window_are_skipped() {
    let (mut ed, sheet, vp) = project();
    let all = count_paths(&prims(&ed, sheet));
    set_viewport(&mut ed, vp, |v| {
        v.scale = 500.0;
        v.center = DVec2::new(-1.0e6, -1.0e6);
    });
    assert!(count_paths(&prims(&ed, sheet)) < all / 2);
}

#[test]
fn visibility_and_graphic_overrides_change_the_drawing() {
    let (mut ed, sheet, vp) = project();
    let trees = ClassId(
        run(
            &mut ed,
            Command::AddClass {
                name: "Trees".into(),
                attrs: Attributes {
                    pen: Some(Color([0, 0, 255, 255])),
                    fill: None,
                    line_weight: Some(0.1),
                    ..Default::default()
                },
            },
        )[0],
    );
    add_in_class(&mut ed, trees);
    let base = prims(&ed, sheet);
    let blue = |ps: &[Prim]| {
        ps.iter()
            .filter(|p| matches!(p, Prim::Path(pp) if pp.stroke == Some([0, 0, 255, 255])))
            .count()
    };
    assert!(blue(&base) > 0);
    // Class hidden in the viewport only.
    set_viewport(&mut ed, vp, |v| {
        v.class_overrides.insert(
            trees,
            ClassOverride {
                visible: Some(false),
                ..Default::default()
            },
        );
    });
    assert_eq!(blue(&prims(&ed, sheet)), 0);
    assert!(
        ed.doc().class(trees).unwrap().visible,
        "the document is unchanged"
    );
    // A graphic override beats the class, but not an attribute that the object sets itself.
    set_viewport(&mut ed, vp, |v| {
        v.class_overrides.insert(
            trees,
            ClassOverride {
                pen: Some(Color([1, 2, 3, 255])),
                line_weight: Some(0.9),
                ..Default::default()
            },
        );
    });
    let ps = prims(&ed, sheet);
    assert!(ps.iter().any(
        |p| matches!(p, Prim::Path(pp) if pp.stroke == Some([1, 2, 3, 255]) && pp.width == 0.9)
    ));
    // The circle with its own blue pen keeps it: an override acts on what is "by class" only.
    assert_eq!(blue(&ps), 1);
    // Layer hidden in the viewport.
    set_viewport(&mut ed, vp, |v| {
        v.layer_overrides.insert(FIRST_LAYER, false);
    });
    assert_eq!(count_paths(&prims(&ed, sheet)), 0);
    // A design layer that is not listed does not show.
    set_viewport(&mut ed, vp, |v| {
        v.layer_overrides.clear();
        v.layers.clear();
    });
    assert_eq!(count_paths(&prims(&ed, sheet)), 0);
}

fn add_in_class(ed: &mut Editor, class: ClassId) {
    run(
        ed,
        Command::AddObject {
            shape: Shape::Circle {
                center: v(5000.0, 4500.0),
                radius: 700.0,
            },
            layer: None,
            class: Some(class),
            attrs: Attributes {
                pen: Some(Color([0, 0, 255, 255])),
                ..Default::default()
            },
        },
    );
    // The object has its own pen, so the class pen does not matter here. Use a second object without one.
    run(
        ed,
        Command::AddObject {
            shape: Shape::Circle {
                center: v(5000.0, 4500.0),
                radius: 900.0,
            },
            layer: None,
            class: Some(class),
            attrs: Attributes::default(),
        },
    );
}

#[test]
fn crop_adds_a_clip_polygon() {
    let (mut ed, sheet, vp) = project();
    set_viewport(&mut ed, vp, |v| {
        v.crop = Some(Crop::Polygon {
            points: vec![v_(100.0, 100.0), v_(300.0, 100.0), v_(200.0, 250.0)],
        })
    });
    let Prim::PushClip(poly) = &prims(&ed, sheet)[0] else {
        panic!()
    };
    assert_eq!(poly.len(), 3);
    set_viewport(&mut ed, vp, |v| {
        v.crop = Some(Crop::Rect {
            min: v_(100.0, 100.0),
            max: v_(200.0, 200.0),
        })
    });
    let Prim::PushClip(poly) = &prims(&ed, sheet)[0] else {
        panic!()
    };
    assert_eq!(poly.len(), 4);
    // A crop outside the frame hides everything, and the frame is still drawn only if asked.
    set_viewport(&mut ed, vp, |v| {
        v.crop = Some(Crop::Rect {
            min: v_(1000.0, 1000.0),
            max: v_(1100.0, 1100.0),
        });
        v.show_frame = true;
    });
    assert!(prims(&ed, sheet).is_empty());
}

fn v_(x: f64, y: f64) -> DVec2 {
    v(x, y)
}

#[test]
fn annotations_draw_in_paper_units_from_the_frame_corner() {
    let (mut ed, sheet, vp) = project();
    let before = prims(&ed, sheet).len();
    let ann = |id, item| Annotation {
        id,
        class: NONE_CLASS,
        attrs: Attributes::default(),
        item,
    };
    set_viewport(&mut ed, vp, |v| {
        v.show_frame = true;
        v.annotations = vec![
            ann(
                1,
                AnnotationItem::Line {
                    a: v_(1.0, 1.0),
                    b: v_(11.0, 1.0),
                },
            ),
            ann(
                2,
                AnnotationItem::Text {
                    at: v_(5.0, 5.0),
                    text: "North".into(),
                    height: 4.0,
                },
            ),
            ann(
                3,
                AnnotationItem::Dimension {
                    a: v_(0.0, 10.0),
                    b: v_(40.0, 10.0),
                    offset: 6.0,
                    text: None,
                },
            ),
            ann(
                4,
                AnnotationItem::Dimension {
                    a: v_(0.0, 10.0),
                    b: v_(40.0, 10.0),
                    offset: -6.0,
                    text: Some("custom".into()),
                },
            ),
            ann(
                5,
                AnnotationItem::Dimension {
                    a: v_(0.0, 10.0),
                    b: v_(0.0, 10.0),
                    offset: 6.0,
                    text: None,
                },
            ),
            ann(
                6,
                AnnotationItem::Shape {
                    shape: Box::new(Shape::Circle {
                        center: v_(30.0, 30.0),
                        radius: 5.0,
                    }),
                },
            ),
        ];
    });
    let ps = prims(&ed, sheet);
    assert!(ps.len() > before + 8);
    let t = texts(&ps);
    assert!(t.contains(&"North".to_string()) && t.contains(&"custom".to_string()));
    // 40 mm of paper at 1:50 is 2000 mm of model (the scale of this viewport may differ).
    let Shape::Viewport(vpv) = &ed.doc().object(vp).unwrap().shape else {
        panic!()
    };
    let want = format!("{}", (40.0 * vpv.scale).round());
    assert!(t.contains(&want), "{t:?} should have {want}");
    // The text sits at the frame corner plus the offset.
    let north = ps
        .iter()
        .find_map(|p| match p {
            Prim::Text(t) if t.text == "North" => Some(t.at),
            _ => None,
        })
        .unwrap();
    assert_eq!(north, vpv.min + v_(5.0, 5.0));
}

#[test]
fn sheet_layer_shapes_are_paper_space() {
    let (mut ed, sheet, _) = project();
    add(
        &mut ed,
        Shape::Line {
            a: v(10.0, 10.0),
            b: v(60.0, 10.0),
        },
        Some(sheet),
        Attributes {
            line_weight: Some(1.0),
            ..Default::default()
        },
    );
    add(
        &mut ed,
        Shape::Text {
            at: v(10.0, 20.0),
            text: "Note".into(),
            height: 5.0,
        },
        Some(sheet),
        Attributes::default(),
    );
    let ps = prims(&ed, sheet);
    assert!(ps.iter().any(|p| matches!(p, Prim::Path(pp) if pp.width == 1.0 && pp.segs == vec![PSeg::Move(v(10.0, 10.0)), PSeg::Line(v(60.0, 10.0))])));
    assert!(ps.iter().any(|p| matches!(p, Prim::Text(t) if t.text == "Note" && t.height == 5.0 && t.at == v(10.0, 20.0))));
    // A hidden sheet class hides the paper-space shape.
    run(
        &mut ed,
        Command::ModifyClass {
            id: NONE_CLASS,
            name: None,
            visible: Some(false),
            attrs: None,
        },
    );
    assert!(prims(&ed, sheet).is_empty());
}

#[test]
fn title_block_shows_project_and_sheet_fields() {
    let (mut ed, sheet, _) = project();
    add(
        &mut ed,
        Shape::TitleBlock(TitleBlock {
            grid_columns: 4,
            grid_rows: 3,
            fields: [("zone".to_string(), "7b".to_string())].into(),
            ..Default::default()
        }),
        Some(sheet),
        Attributes::default(),
    );
    let t = texts(&prims(&ed, sheet));
    for want in [
        "Riverside garden",
        "L-101",
        "Planting plan",
        "NM",
        "2026-10-10",
        "1:50",
        "A",
        "D",
        "1",
        "3",
    ] {
        assert!(
            t.iter().any(|x| x == want) || want == "1:50",
            "{want} in {t:?}"
        );
    }
    assert!(t.iter().any(|x| x.starts_with("1:")), "{t:?}");
    // Own layout from JSON.
    let layout: TitleLayout = serde_json::from_str(
        r#"{"width": 100, "height": 30, "cells": [{"x": 0, "y": 0, "w": 100, "h": 15, "label": "Zone", "text": "{field.zone}/{sheet.index} of {sheet.count}", "size": 5},
                                              {"x": 0, "y": 15, "w": 100, "h": 15, "text": ""}]}"#,
    )
    .unwrap();
    let id = ed
        .doc()
        .objects
        .values()
        .find(|o| o.shape.kind() == "title_block")
        .unwrap()
        .id;
    let Shape::TitleBlock(tb) = &ed.doc().object(id).unwrap().shape else {
        panic!()
    };
    let mut tb = tb.clone();
    tb.layout = Some(layout);
    run(
        &mut ed,
        Command::ModifyObject {
            id,
            shape: Some(Shape::TitleBlock(tb)),
            layer: None,
            class: None,
            attrs: None,
        },
    );
    let t = texts(&prims(&ed, sheet));
    assert!(t.contains(&"7b/1 of 1".to_string()), "{t:?}");
    assert!(!t.contains(&"Riverside garden".to_string()));
}

#[test]
fn long_title_block_text_shrinks_to_fit() {
    let (mut ed, sheet, _) = project();
    run(
        &mut ed,
        Command::SetProject {
            name: Some("A very long project name that cannot fit in one cell of the block".into()),
            client: None,
            address: None,
            drawn_by: None,
            date: None,
            revision: None,
        },
    );
    add(
        &mut ed,
        Shape::TitleBlock(TitleBlock::default()),
        Some(sheet),
        Attributes::default(),
    );
    let ps = prims(&ed, sheet);
    let t = ps
        .iter()
        .find_map(|p| match p {
            Prim::Text(t) if t.text.starts_with("A very long") => Some(t.clone()),
            _ => None,
        })
        .unwrap();
    assert!(t.height < 6.0 && text_width(&t.text, t.height) <= 180.0);
}

// ---------------------------------------------------------------- cache

#[test]
fn pages_are_built_once_per_revision() {
    let (mut ed, sheet, _) = project();
    let mut cache = SheetCache::default();
    let a = cache.page(ed.doc(), ed.revision(), sheet).unwrap();
    let b = cache.page(ed.doc(), ed.revision(), sheet).unwrap();
    assert_eq!(cache.builds, 1);
    assert!(std::sync::Arc::ptr_eq(&a, &b));
    // The model changes: the viewport follows.
    add(
        &mut ed,
        Shape::Circle {
            center: v(100.0, 100.0),
            radius: 50.0,
        },
        None,
        Attributes::default(),
    );
    let c = cache.page(ed.doc(), ed.revision(), sheet).unwrap();
    assert_eq!(cache.builds, 2);
    assert_eq!(count_paths(&c.prims), count_paths(&a.prims) + 1);
    // Undo is a new revision too.
    ed.undo();
    let d = cache.page(ed.doc(), ed.revision(), sheet).unwrap();
    assert_eq!(cache.builds, 3);
    assert_eq!(d.prims, a.prims);
    assert!(cache.page(ed.doc(), ed.revision(), FIRST_LAYER).is_err());
    assert!(cache.page(ed.doc(), ed.revision(), LayerId(999)).is_err());
}

#[test]
fn errors_name_the_problem() {
    let (ed, _, _) = project();
    assert_eq!(
        compose_page(ed.doc(), FIRST_LAYER).unwrap_err(),
        PrintError::NoSheet(FIRST_LAYER)
    );
    assert!(
        PrintError::NoSheet(LayerId(4))
            .to_string()
            .contains("not a sheet layer")
    );
    assert_eq!(PrintError::Invalid("x".into()).to_string(), "x");
    assert_eq!(PrintError::Io("y".into()).to_string(), "y");
    assert!(select_sheets(&Editor::default().doc().clone(), None).is_err());
    assert_eq!(
        select_sheets(
            ed.doc(),
            Some(&[ed.doc().sheet_layers().next().unwrap().id])
        )
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        select_sheets(ed.doc(), Some(&[FIRST_LAYER])).unwrap_err(),
        PrintError::NoSheet(FIRST_LAYER)
    );
    assert_eq!(select_sheets(ed.doc(), None).unwrap().len(), 1);
    let e: &dyn std::error::Error = &PrintError::Io("z".into());
    assert_eq!(e.to_string(), "z");
}

// ---------------------------------------------------------------- PDF

fn pdf_of(ed: &Editor, sheets: &[LayerId]) -> Vec<u8> {
    let mut cache = SheetCache::default();
    export_pdf(ed.doc(), ed.revision(), &mut cache, sheets).unwrap()
}

fn page_ops(doc: &lopdf::Document, n: u32) -> Vec<lopdf::content::Operation> {
    let id = doc.get_pages()[&n];
    Content::decode(&doc.get_page_content(id))
        .unwrap()
        .operations
}

fn num(o: &PdfObject) -> f64 {
    match o {
        PdfObject::Integer(i) => *i as f64,
        PdfObject::Real(r) => *r as f64,
        other => panic!("{other:?}"),
    }
}

#[test]
fn pdf_parses_has_the_right_page_size_and_contains_the_text() {
    let (mut ed, sheet, _) = project();
    add(
        &mut ed,
        Shape::TitleBlock(TitleBlock::default()),
        Some(sheet),
        Attributes::default(),
    );
    let bytes = pdf_of(&ed, &[sheet]);
    assert!(bytes.starts_with(b"%PDF-"));
    let doc = lopdf::Document::load_mem(&bytes).expect("the PDF parses");
    assert_eq!(doc.get_pages().len(), 1);
    let page_id = doc.get_pages()[&1];
    let page = doc.get_dictionary(page_id).unwrap();
    let mb: Vec<f64> = page
        .get(b"MediaBox")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(num)
        .collect();
    // A3 landscape: 420 x 297 mm in points.
    assert!(
        (mb[2] - 420.0 * 72.0 / 25.4).abs() < 0.01 && (mb[3] - 297.0 * 72.0 / 25.4).abs() < 0.01,
        "{mb:?}"
    );
    let text = doc.extract_text(&[1]).unwrap();
    for want in [
        "Riverside garden",
        "Planting plan",
        "L-101",
        "Garden Bed",
        "NM",
    ] {
        assert!(text.contains(want), "{want} missing from {text:?}");
    }
    let info = doc.trailer.get(b"Info").unwrap().as_reference().unwrap();
    let title = doc
        .get_dictionary(info)
        .unwrap()
        .get(b"Title")
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(title, b"Riverside garden");
}

#[test]
fn pdf_has_vector_paths_curves_clips_weights_and_fills() {
    let (ed, sheet, _) = project();
    let doc = lopdf::Document::load_mem(&pdf_of(&ed, &[sheet])).unwrap();
    let ops = page_ops(&doc, 1);
    let names: Vec<&str> = ops.iter().map(|o| o.operator.as_str()).collect();
    // Circles and arcs are Bezier curves.
    assert!(
        names.iter().filter(|n| **n == "c").count() >= 8,
        "circle and arc curves"
    );
    // Straight pieces, a clip for the viewport, and balanced q/Q.
    assert!(names.contains(&"m") && names.contains(&"l") && names.contains(&"W"));
    assert_eq!(
        names.iter().filter(|n| **n == "q").count(),
        names.iter().filter(|n| **n == "Q").count()
    );
    // A line weight of 0.5 mm is 1.417 points. Colors are set as RGB.
    let widths: Vec<f64> = ops
        .iter()
        .filter(|o| o.operator == "w")
        .map(|o| num(&o.operands[0]))
        .collect();
    assert!(
        widths.iter().any(|w| (w - 0.5 * 72.0 / 25.4).abs() < 1e-3),
        "{widths:?}"
    );
    assert!(
        ops.iter()
            .any(|o| o.operator == "RG" && (num(&o.operands[0]) - 200.0 / 255.0).abs() < 1e-3)
    );
    assert!(
        ops.iter()
            .any(|o| o.operator == "rg" && (num(&o.operands[1]) - 200.0 / 255.0).abs() < 1e-3)
    );
    // Fill and stroke together, and transparency for the canopies.
    let mut uniq = names.clone();
    uniq.sort();
    uniq.dedup();
    assert!(
        names.iter().any(|n| matches!(*n, "B" | "b")) && names.contains(&"gs"),
        "{uniq:?}"
    );
    assert!(names.contains(&"d"), "dashed protection zones");
    // Text uses the standard font.
    let fonts = doc.get_page_fonts(doc.get_pages()[&1]).unwrap();
    assert_eq!(
        fonts
            .values()
            .next()
            .unwrap()
            .get(b"BaseFont")
            .unwrap()
            .as_name()
            .unwrap(),
        b"Helvetica"
    );
}

#[test]
fn pdf_has_one_page_per_sheet_with_its_own_size() {
    let (mut ed, sheet, _) = project();
    let second = LayerId(
        run(
            &mut ed,
            Command::AddSheet {
                name: "Detail".into(),
                paper: Some(PaperSize::A4),
                orientation: Some(Orientation::Portrait),
                number: Some("L-201".into()),
                title: Some("Detail".into()),
            },
        )[0],
    );
    add(
        &mut ed,
        Shape::Text {
            at: v(20.0, 20.0),
            text: "Euro \u{20ac} and \u{e4}\u{f6} and \u{4e2d}".into(),
            height: 5.0,
        },
        Some(second),
        Attributes::default(),
    );
    let doc = lopdf::Document::load_mem(&pdf_of(&ed, &[sheet, second])).unwrap();
    assert_eq!(doc.get_pages().len(), 2);
    let size = |n: u32| -> (f64, f64) {
        let mb = doc
            .get_dictionary(doc.get_pages()[&n])
            .unwrap()
            .get(b"MediaBox")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(num)
            .collect::<Vec<_>>();
        (mb[2], mb[3])
    };
    assert!(
        (size(2).0 - 210.0 * 72.0 / 25.4).abs() < 0.01
            && (size(2).1 - 297.0 * 72.0 / 25.4).abs() < 0.01
    );
    assert!(size(1).0 > size(1).1);
    assert!(doc.extract_text(&[2]).unwrap().contains("and"));
}

#[test]
fn pdf_embeds_3d_viewports_as_images() {
    let (mut ed, sheet, vp) = project();
    add(
        &mut ed,
        Shape::Hardscape {
            boundary: corners(&[
                v(0.0, 0.0),
                v(3000.0, 0.0),
                v(3000.0, 3000.0),
                v(0.0, 3000.0),
            ]),
            material: "x".into(),
            thickness: Some(300.0),
            elevation: None,
        },
        None,
        Attributes::default(),
    );
    set_viewport(&mut ed, vp, |v| {
        v.view = "right_isometric".into();
        v.dpi = 40.0;
        v.class_overrides.insert(
            NONE_CLASS,
            ClassOverride {
                fill: Some(Color([255, 0, 0, 255])),
                ..Default::default()
            },
        );
    });
    let page = compose_page(ed.doc(), sheet).unwrap();
    let img = page
        .prims
        .iter()
        .find_map(|p| {
            if let Prim::Image(i) = p {
                Some(i.clone())
            } else {
                None
            }
        })
        .expect("a raster viewport");
    assert_eq!(img.rgb.len(), (img.width * img.height * 3) as usize);
    // The model is drawn: not all pixels are the white background.
    assert!(img.rgb.chunks(3).any(|p| p != [255, 255, 255]));
    let bytes = pdf_of(&ed, &[sheet]);
    let doc = lopdf::Document::load_mem(&bytes).unwrap();
    let res = doc
        .get_page_resources(doc.get_pages()[&1])
        .unwrap()
        .0
        .unwrap();
    let x = res.get(b"XObject").unwrap().as_dict().unwrap();
    let id = x.iter().next().unwrap().1.as_reference().unwrap();
    let obj = doc.get_object(id).unwrap().as_stream().unwrap();
    assert_eq!(
        obj.dict.get(b"Subtype").unwrap().as_name().unwrap(),
        b"Image"
    );
    assert_eq!(
        obj.dict.get(b"Width").unwrap().as_i64().unwrap(),
        img.width as i64
    );
    assert!(page_ops(&doc, 1).iter().any(|o| o.operator == "Do"));
    // Wireframe, hidden class and an unknown view.
    set_viewport(&mut ed, vp, |v| v.mode = "wireframe".into());
    assert!(
        compose_page(ed.doc(), sheet)
            .unwrap()
            .prims
            .iter()
            .any(|p| matches!(p, Prim::Image(_)))
    );
    set_viewport(&mut ed, vp, |v| v.view = "sideways".into());
    assert!(
        !compose_page(ed.doc(), sheet)
            .unwrap()
            .prims
            .iter()
            .any(|p| matches!(p, Prim::Image(_)))
    );
}

#[test]
fn hidden_line_viewports_are_vector_lines_and_styles_apply() {
    let (mut ed, sheet, vp) = project();
    // A cube of 3000 mm in the model.
    add(
        &mut ed,
        Shape::Hardscape {
            boundary: corners(&[
                v(0.0, 0.0),
                v(3000.0, 0.0),
                v(3000.0, 3000.0),
                v(0.0, 3000.0),
            ]),
            material: "x".into(),
            thickness: Some(3000.0),
            elevation: Some(3000.0),
        },
        None,
        Attributes::default(),
    );
    let count = |ed: &Editor| {
        let page = compose_page(ed.doc(), sheet).unwrap();
        let images = page
            .prims
            .iter()
            .filter(|p| matches!(p, Prim::Image(_)))
            .count();
        let paths: Vec<&PathPrim> = page
            .prims
            .iter()
            .filter_map(|p| if let Prim::Path(p) = p { Some(p) } else { None })
            .collect();
        (
            images,
            paths.len(),
            paths.iter().filter(|p| p.dash.is_some()).count(),
        )
    };
    set_viewport(&mut ed, vp, |v| {
        v.view = "right_isometric".into();
        v.mode = "shaded".into();
        v.dpi = 40.0;
    });
    let (images, base_paths, _) = count(&ed);
    assert_eq!(images, 1);
    // Hidden line: no image, but the visible edges as vector paths (the cube has 9 of 12).
    set_viewport(&mut ed, vp, |v| v.mode = "hidden_line".into());
    let (images, paths, dashed) = count(&ed);
    assert_eq!(images, 0);
    assert!(paths >= base_paths + 9, "{paths} vs {base_paths}");
    assert_eq!(dashed, 0);
    // The same page twice gives the same display list.
    assert_eq!(
        compose_page(ed.doc(), sheet).unwrap().prims,
        compose_page(ed.doc(), sheet).unwrap().prims
    );
    // Dashed hidden line adds dashed paths for the covered edges (the cube alone has 3).
    set_viewport(&mut ed, vp, |v| v.mode = "dashed_hidden_line".into());
    let (_, paths2, dashed) = count(&ed);
    assert!(dashed >= 3, "{dashed}");
    assert_eq!(paths2, paths + dashed);
    // PDF and SVG write them without an image.
    let bytes = pdf_of(&ed, &[sheet]);
    assert!(lopdf::Document::load_mem(&bytes).is_ok());
    let svg_text = svg::page_svg(&compose_page(ed.doc(), sheet).unwrap());
    assert!(svg_text.contains("stroke-dasharray"));

    // A render style resource replaces the mode: line weight from the style.
    let style = ResourceId(
        run(
            &mut ed,
            Command::AddResource {
                name: "Plan lines".into(),
                body: ResourceBody::Render(Box::new(
                    cadektor_core::present::RenderResource::RenderStyle(
                        cadektor_core::present::RenderStyle {
                            mode: cadektor_core::present::RenderMode::HiddenLine,
                            line_weight: 0.7,
                            ..Default::default()
                        },
                    ),
                )),
            },
        )[0],
    );
    set_viewport(&mut ed, vp, |v| {
        v.mode = "shaded".into();
        v.render_style = Some(style);
    });
    let page = compose_page(ed.doc(), sheet).unwrap();
    assert!(!page.prims.iter().any(|p| matches!(p, Prim::Image(_))));
    assert!(
        page.prims
            .iter()
            .any(|p| matches!(p, Prim::Path(p) if (p.width - 0.7).abs() < 1e-9))
    );
    // Without the style the viewport falls back to its mode.
    set_viewport(&mut ed, vp, |v| v.render_style = None);
    assert_eq!(count(&ed).0, 1);

    // A material shows in the shaded viewport: the picture differs from the plain one.
    let plain = compose_page(ed.doc(), sheet)
        .unwrap()
        .prims
        .into_iter()
        .find_map(|p| {
            if let Prim::Image(i) = p {
                Some(i)
            } else {
                None
            }
        })
        .unwrap();
    let m = ResourceId(
        run(
            &mut ed,
            Command::AddResource {
                name: "Brick".into(),
                body: ResourceBody::Render(Box::new(
                    cadektor_core::present::RenderResource::Material(
                        cadektor_core::present::Material {
                            base_color: Color([160, 80, 60, 255]),
                            texture: Some(cadektor_core::present::Texture::Procedural {
                                kind: cadektor_core::present::Procedural::Brick,
                                seed: 1,
                            }),
                            ..Default::default()
                        },
                    ),
                )),
            },
        )[0],
    );
    run(
        &mut ed,
        Command::SetMaterialSlot {
            target: cadektor_core::present::SlotTarget::Class { class: NONE_CLASS },
            material: Some(m),
        },
    );
    let textured = compose_page(ed.doc(), sheet)
        .unwrap()
        .prims
        .into_iter()
        .find_map(|p| {
            if let Prim::Image(i) = p {
                Some(i)
            } else {
                None
            }
        })
        .unwrap();
    assert_ne!(plain.rgb, textured.rgb);
    // An invalid mode name is refused by the viewport check.
    let mut bad = Viewport::new(v(0.0, 0.0), v(10.0, 10.0), v(0.0, 0.0), 100.0, vec![]);
    bad.mode = "sketchy".into();
    assert!(bad.problem().unwrap().contains("hidden_line"));
}

#[test]
fn raster_viewport_of_an_empty_model_and_big_pages_still_render() {
    let mut ed = Editor::default();
    let sheet = LayerId(
        run(
            &mut ed,
            Command::AddSheet {
                name: "S".into(),
                paper: Some(PaperSize::A0),
                orientation: None,
                number: None,
                title: None,
            },
        )[0],
    );
    let mut vp = Viewport::new(
        v(10.0, 10.0),
        v(1100.0, 800.0),
        v(0.0, 0.0),
        10.0,
        vec![FIRST_LAYER],
    );
    vp.view = "top".into();
    vp.dpi = 1200.0;
    add(
        &mut ed,
        Shape::Viewport(Box::new(vp)),
        Some(sheet),
        Attributes::default(),
    );
    let page = compose_page(ed.doc(), sheet).unwrap();
    let Some(Prim::Image(i)) = page.prims.get(1) else {
        panic!("{:?}", page.prims.len())
    };
    assert!(i.width <= 3000 && i.height <= 3000);
}

// ---------------------------------------------------------------- SVG and PNG

#[test]
fn svg_is_in_mm_with_clip_text_and_images() {
    let (mut ed, sheet, vp) = project();
    set_viewport(&mut ed, vp, |v| {
        v.crop = Some(Crop::Rect {
            min: v_(50.0, 80.0),
            max: v_(300.0, 250.0),
        })
    });
    let page = compose_page(ed.doc(), sheet).unwrap();
    let s = svg::page_svg(&page);
    assert!(
        s.starts_with("<svg")
            && s.contains(r#"width="420mm" height="297mm""#)
            && s.contains("<title>L-101 Planting plan</title>")
    );
    assert!(
        s.contains("clipPath")
            && s.contains("clip-path=")
            && s.contains("Garden Bed")
            && s.contains("stroke-dasharray")
    );
    assert!(s.contains("C ") && s.contains("fill-opacity"));
    assert_eq!(s.matches("<g ").count(), s.matches("</g>").count());
    assert!(!s.contains("<image"));
    set_viewport(&mut ed, vp, |v| {
        v.view = "front".into();
        v.dpi = 30.0;
    });
    add(
        &mut ed,
        Shape::Text {
            at: v(10.0, 10.0),
            text: "a<b & \"c\"".into(),
            height: 4.0,
        },
        Some(sheet),
        Attributes::default(),
    );
    let s = svg::page_svg(&compose_page(ed.doc(), sheet).unwrap());
    assert!(
        s.contains("<image")
            && s.contains("data:image/png;base64,")
            && s.contains("a&lt;b &amp; &quot;c&quot;")
    );
    // Unbalanced pops and empty paths are tolerated.
    let odd = Page {
        size: v(100.0, 100.0),
        title: "t".into(),
        prims: vec![
            Prim::PopClip,
            Prim::PushClip(vec![v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0)]),
            Prim::Path(PathPrim {
                segs: vec![],
                closed: false,
                stroke: None,
                fill: None,
                width: 1.0,
                dash: None,
            }),
            Prim::Text(TextPrim {
                at: v(5.0, 5.0),
                text: "c".into(),
                height: 3.0,
                color: [0, 0, 0, 255],
                align: Align::Center,
            }),
        ],
    };
    let s = svg::page_svg(&odd);
    assert!(s.contains("text-anchor=\"middle\"") && s.matches("</g>").count() == 1);
}

fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let dec = ::png::Decoder::new(std::io::Cursor::new(bytes));
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    (info.width, info.height, buf)
}

#[test]
fn png_has_the_page_size_at_the_dpi_and_draws_the_content() {
    let (mut ed, sheet, vp) = project();
    let page = compose_page(ed.doc(), sheet).unwrap();
    let bytes = sheet_png::page_png(&page, 50.8).unwrap();
    let (w, h, px) = decode_png(&bytes);
    // 420 mm at 50.8 dpi is 840 px.
    assert_eq!((w, h), (840, 594));
    let at = |x: usize, y: usize| &px[(y * w as usize + x) * 4..(y * w as usize + x) * 4 + 4];
    assert_eq!(at(2, 2), [255, 255, 255, 255], "white paper");
    assert!(px.chunks(4).any(|p| p[..3] == [200, 0, 0]), "the red pen");
    assert!(
        px.chunks(4).any(|p| p[..3] == [0, 0, 0]),
        "black text and lines"
    );
    // A crop hides what is outside of it: the area left of the crop stays white.
    set_viewport(&mut ed, vp, |v| {
        v.crop = Some(Crop::Polygon {
            points: vec![v_(300.0, 100.0), v_(400.0, 100.0), v_(400.0, 250.0)],
        })
    });
    let page = compose_page(ed.doc(), sheet).unwrap();
    let (w, _, px) = decode_png(&sheet_png::page_png(&page, 50.8).unwrap());
    let left: Vec<&[u8]> = (0..594usize)
        .flat_map(|y| (0..560usize).map(move |x| (x, y)))
        .map(|(x, y)| &px[(y * w as usize + x) * 4..(y * w as usize + x) * 4 + 4])
        .collect();
    assert!(
        left.iter().all(|p| p[..3] == [255, 255, 255]),
        "nothing is drawn left of the crop"
    );
    // 3D viewport with an image.
    set_viewport(&mut ed, vp, |v| {
        v.crop = None;
        v.view = "left_isometric".into();
        v.dpi = 40.0;
    });
    let page = compose_page(ed.doc(), sheet).unwrap();
    let (_, _, px) = decode_png(&sheet_png::page_png(&page, 30.0).unwrap());
    assert!(px.chunks(4).any(|p| p[..3] != [255, 255, 255]));
}

#[test]
fn png_refuses_bad_sizes() {
    let (ed, sheet, _) = project();
    let page = compose_page(ed.doc(), sheet).unwrap();
    assert!(
        matches!(sheet_png::page_png(&page, 0.0), Err(PrintError::Invalid(m)) if m.contains("dpi"))
    );
    assert!(sheet_png::page_png(&page, f64::NAN).is_err());
    assert!(
        matches!(sheet_png::page_png(&page, 1200.0), Err(PrintError::Invalid(m)) if m.contains("lower dpi"))
    );
    assert!(sheet_png::measure("Hello", 10.0) > 10.0);
}

#[test]
fn text_rendering_covers_alignment_and_tiny_text() {
    let page = Page {
        size: v(100.0, 50.0),
        title: "t".into(),
        prims: vec![
            Prim::Text(TextPrim {
                at: v(50.0, 25.0),
                text: "Middle".into(),
                height: 8.0,
                color: [0, 0, 0, 255],
                align: Align::Center,
            }),
            Prim::Text(TextPrim {
                at: v(1.0, 1.0),
                text: "tiny".into(),
                height: 0.1,
                color: [0, 0, 0, 255],
                align: Align::Left,
            }),
            Prim::Text(TextPrim {
                at: v(-50.0, 25.0),
                text: "off".into(),
                height: 8.0,
                color: [0, 0, 0, 255],
                align: Align::Left,
            }),
            Prim::PushClip(vec![v(0.0, 0.0), v(30.0, 0.0), v(30.0, 50.0), v(0.0, 50.0)]),
            Prim::PushClip(vec![v(0.0, 0.0), v(20.0, 0.0), v(20.0, 50.0), v(0.0, 50.0)]),
            Prim::Text(TextPrim {
                at: v(10.0, 10.0),
                text: "In".into(),
                height: 6.0,
                color: [255, 0, 0, 255],
                align: Align::Left,
            }),
            Prim::Path(PathPrim {
                segs: vec![
                    PSeg::Move(v(0.0, 40.0)),
                    PSeg::Line(v(90.0, 40.0)),
                    PSeg::Cubic(v(95.0, 40.0), v(95.0, 45.0), v(90.0, 45.0)),
                ],
                closed: true,
                stroke: Some([0, 0, 255, 255]),
                fill: Some([0, 0, 255, 128]),
                width: 0.0,
                dash: Some([2.0, 1.0]),
            }),
            Prim::Path(PathPrim {
                segs: vec![PSeg::Move(v(0.0, 0.0))],
                closed: false,
                stroke: Some([0, 0, 0, 255]),
                fill: None,
                width: 1.0,
                dash: None,
            }),
            Prim::PopClip,
            Prim::PopClip,
        ],
    };
    let (w, h, px) = decode_png(&sheet_png::page_png(&page, 100.0).unwrap());
    let dark = |x0: usize, x1: usize| {
        (0..h as usize).any(|y| (x0..x1).any(|x| px[(y * w as usize + x) * 4] < 128))
    };
    assert!(dark(150, 250), "the centered text is around the middle");
    let red_right = (0..h as usize)
        .any(|y| (120..w as usize).any(|x| px[(y * w as usize + x) * 4..][..3] == [255, 0, 0]));
    assert!(!red_right);
    // The PDF of the same page parses, with centered text and a dash.
    let bytes = cadektor_print::pdf::write_pdf(&[&page], "T");
    let doc = lopdf::Document::load_mem(&bytes).unwrap();
    assert!(doc.extract_text(&[1]).unwrap().contains("Middle"));
}

#[test]
fn files_are_written_and_errors_reported() {
    let dir = std::env::temp_dir().join(format!("cadektor-print-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("out.bin");
    assert_eq!(write_file(&path, b"abc").unwrap(), path);
    assert_eq!(std::fs::read(&path).unwrap(), b"abc");
    let missing = dir.join("no/such/folder/x.pdf");
    assert!(matches!(write_file(&missing, b"x"), Err(PrintError::Io(m)) if m.contains("x.pdf")));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn display_helpers() {
    // A quarter turn is one curve, and a full circle is four. The curves stay on the circle.
    let one = arc_segs(DVec2::ZERO, 10.0, 0.0, std::f64::consts::FRAC_PI_2, true);
    assert_eq!(one.len(), 2);
    let circle = circle_segs(v(5.0, 5.0), 10.0);
    assert_eq!(circle.len(), 5);
    for p in sample(&circle, 16) {
        assert!((p.distance(v(5.0, 5.0)) - 10.0).abs() < 0.01, "{p:?}");
    }
    // Clockwise arcs work too.
    let cw = arc_segs(DVec2::ZERO, 10.0, 0.0, -std::f64::consts::PI, true);
    assert!(sample(&cw, 8).iter().all(|p| p.y <= 1e-9));
    assert_eq!(PathPrim::polyline(&[v(0.0, 0.0), v(1.0, 1.0)]).len(), 2);
    assert!(text_width("abcd", 10.0) > 0.0);
}

#[test]
fn winansi_keeps_estonian_and_typographic_characters() {
    use cadektor_print::pdf::winansi;
    assert_eq!(
        winansi("Šõõr Žürii äöü"),
        vec![
            0x8A, 0xF5, 0xF5, b'r', b' ', 0x8E, 0xFC, b'r', b'i', b'i', b' ', 0xE4, 0xF6, 0xFC
        ]
    );
    assert_eq!(
        winansi("š ž Œ \u{2013} \u{2014} \u{201C}x\u{201D} \u{2019}"),
        vec![
            0x9A, b' ', 0x9E, b' ', 0x8C, b' ', 0x96, b' ', 0x97, b' ', 0x93, b'x', 0x94, b' ',
            0x92
        ]
    );
    assert_eq!(winansi("\u{4e2d}\u{85}"), b"??");
}

#[test]
fn raster_viewports_keep_the_frame_proportions_and_the_budget() {
    let mut ed = Editor::default();
    let sheet = LayerId(
        run(
            &mut ed,
            Command::AddSheet {
                name: "S".into(),
                paper: Some(PaperSize::A0),
                orientation: None,
                number: None,
                title: None,
            },
        )[0],
    );
    // A wide frame at a high dpi: both sides would be clamped. The picture must stay 4:1.
    let mut vp = Viewport::new(
        v(10.0, 10.0),
        v(1100.0, 285.0),
        v(0.0, 0.0),
        10.0,
        vec![FIRST_LAYER],
    );
    vp.view = "top".into();
    vp.dpi = 600.0;
    add(
        &mut ed,
        Shape::Viewport(Box::new(vp.clone())),
        Some(sheet),
        Attributes::default(),
    );
    let page = compose_page(ed.doc(), sheet).unwrap();
    let Some(Prim::Image(i)) = page.prims.get(1) else {
        panic!()
    };
    assert!(i.width <= 3000 && i.height <= 3000);
    let want = 1090.0 / 275.0;
    assert!(
        (i.width as f64 / i.height as f64 - want).abs() < 0.01,
        "{}x{}",
        i.width,
        i.height
    );
    // Eight 3D viewports of 9 megapixels each share 64 megapixels: no page holds more.
    for k in 1..8 {
        vp.min = v(10.0, 10.0 + k as f64);
        add(
            &mut ed,
            Shape::Viewport(Box::new(vp.clone())),
            Some(sheet),
            Attributes::default(),
        );
    }
    let page = compose_page(ed.doc(), sheet).unwrap();
    let total: u64 = page
        .prims
        .iter()
        .filter_map(|p| {
            if let Prim::Image(i) = p {
                Some(i.width as u64 * i.height as u64)
            } else {
                None
            }
        })
        .sum();
    assert!(
        total <= cadektor_print::compose::MAX_RASTER_PIXELS,
        "{total}"
    );
    // The export refuses a set of sheets with more image pixels than the limit.
    let mut cache = SheetCache::default();
    let ids: Vec<LayerId> = (0..12)
        .map(|k| {
            let l = LayerId(
                run(
                    &mut ed,
                    Command::AddSheet {
                        name: format!("T{k}"),
                        paper: Some(PaperSize::A0),
                        orientation: None,
                        number: None,
                        title: None,
                    },
                )[0],
            );
            let mut v3 = vp.clone();
            v3.min = v(10.0, 10.0);
            v3.max = v(1100.0, 800.0);
            add(
                &mut ed,
                Shape::Viewport(Box::new(v3)),
                Some(l),
                Attributes::default(),
            );
            l
        })
        .collect();
    let err = export_pdf(ed.doc(), ed.revision(), &mut cache, &ids).unwrap_err();
    assert!(matches!(err, PrintError::Invalid(m) if m.contains("megapixels")));
}

#[test]
fn the_cache_keeps_a_few_pages_and_forgets_the_oldest() {
    let mut ed = Editor::default();
    let ids: Vec<LayerId> = (0..10)
        .map(|k| {
            LayerId(
                run(
                    &mut ed,
                    Command::AddSheet {
                        name: format!("S{k}"),
                        paper: None,
                        orientation: None,
                        number: None,
                        title: None,
                    },
                )[0],
            )
        })
        .collect();
    let mut cache = SheetCache::default();
    let rev = ed.revision();
    for id in &ids {
        cache.page(ed.doc(), rev, *id).unwrap();
    }
    assert_eq!(cache.builds, 10);
    // The newest pages are kept, the oldest was dropped.
    cache.page(ed.doc(), rev, ids[9]).unwrap();
    assert_eq!(cache.builds, 10);
    cache.page(ed.doc(), rev, ids[0]).unwrap();
    assert_eq!(cache.builds, 11);
    // A page that is used again stays.
    cache.page(ed.doc(), rev, ids[9]).unwrap();
    assert_eq!(cache.builds, 11);
}

#[test]
fn viewport_visibility_is_independent_of_the_document() {
    let (mut ed, sheet, vp) = project();
    let doc = ed.doc().clone();
    set_viewport(&mut ed, vp, |v| v.snapshot_visibility(&doc));
    let before = count_paths(&prims(&ed, sheet));
    assert!(before > 5);
    run(
        &mut ed,
        Command::ModifyLayer {
            id: FIRST_LAYER,
            name: None,
            visible: Some(false),
            locked: None,
            elevation: None,
            scale: None,
        },
    );
    assert_eq!(
        count_paths(&prims(&ed, sheet)),
        before,
        "hiding in the document does not hide in the viewport"
    );
}
