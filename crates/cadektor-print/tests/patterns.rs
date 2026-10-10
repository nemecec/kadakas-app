//! Hatches, line types and symbols in sheet output: the display list, SVG and PDF.

use cadektor_core::command::{Command, Editor};
use cadektor_core::graphics::{
    GraphicResource, HatchDef, HatchLine, LineElement, LineTypeDef, PatternUnits, SymbolDef,
    SymbolItem,
};
use cadektor_core::model::*;
use cadektor_core::resource::{ResourceBody, ResourceId};
use cadektor_core::sheets::Viewport;
use cadektor_print::display::{PSeg, Prim};
use cadektor_print::{PageCache, compose_page, export_pdf, select_sheets, svg};
use glam::DVec2;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn res(ed: &mut Editor, name: &str, g: GraphicResource) -> ResourceId {
    ResourceId(
        ed.execute(&Command::AddResource {
            name: name.into(),
            body: ResourceBody::Graphic(Box::new(g)),
        })
        .unwrap()
        .created[0],
    )
}

fn sheet(ed: &mut Editor) -> LayerId {
    LayerId(
        ed.execute(&Command::AddSheet {
            name: "S".into(),
            paper: None,
            orientation: None,
            number: Some("1".into()),
            title: None,
        })
        .unwrap()
        .created[0],
    )
}

fn paths(prims: &[Prim]) -> Vec<&cadektor_print::display::PathPrim> {
    prims
        .iter()
        .filter_map(|p| if let Prim::Path(p) = p { Some(p) } else { None })
        .collect()
}

fn lines_of(p: &cadektor_print::display::PathPrim) -> usize {
    p.segs.iter().filter(|s| matches!(s, PSeg::Line(_))).count()
}

#[test]
fn a_hatch_on_a_sheet_keeps_its_paper_size_and_stays_in_the_shape() {
    let mut ed = Editor::default();
    let hatch = res(
        &mut ed,
        "H",
        GraphicResource::Hatch(HatchDef {
            units: PatternUnits::Paper,
            lines: vec![HatchLine::solid(0.0, 5.0)],
        }),
    );
    let s = sheet(&mut ed);
    // A 50 x 30 mm rectangle on paper, hatched every 5 mm: 6 or 7 lines.
    ed.execute(&Command::AddObject {
        shape: Shape::Rect {
            min: v(20.0, 20.0),
            max: v(70.0, 50.0),
        },
        layer: Some(s),
        class: None,
        attrs: Attributes {
            fill_pattern: Some(hatch),
            ..Default::default()
        },
    })
    .unwrap();
    let page = compose_page(ed.doc(), s).unwrap();
    let lines: Vec<_> = paths(&page.prims)
        .into_iter()
        .filter(|p| lines_of(p) == 1 && p.segs.len() == 2)
        .collect();
    assert!((6..=7).contains(&lines.len()), "{}", lines.len());
    for p in lines {
        let pts = cadektor_print::display::sample(&p.segs, 1);
        assert!(pts.iter().all(|q| q.x >= 20.0 - 1e-6
            && q.x <= 70.0 + 1e-6
            && q.y >= 20.0 - 1e-6
            && q.y <= 50.0 + 1e-6));
    }
    let svg = svg::page_svg(&page);
    assert!(svg.matches("<path").count() >= 7);
}

#[test]
fn a_line_type_replaces_the_outline_with_dashes_and_the_pdf_has_them() {
    let mut ed = Editor::default();
    let lt = res(
        &mut ed,
        "Dashed",
        GraphicResource::LineType(LineTypeDef {
            units: PatternUnits::Paper,
            elements: vec![
                LineElement::Dash { length: 4.0 },
                LineElement::Gap { length: 2.0 },
            ],
        }),
    );
    let s = sheet(&mut ed);
    ed.execute(&Command::AddObject {
        shape: Shape::Line {
            a: v(10.0, 10.0),
            b: v(70.0, 10.0),
        },
        layer: Some(s),
        class: None,
        attrs: Attributes {
            line_type: Some(lt),
            ..Default::default()
        },
    })
    .unwrap();
    let page = compose_page(ed.doc(), s).unwrap();
    // 60 mm in 6 mm periods: ten dashes. The solid outline has no stroke.
    let dashes: Vec<_> = paths(&page.prims)
        .into_iter()
        .filter(|p| {
            p.stroke.is_some() && p.width > 0.0 && p.segs.len() == 2 && p.segs[0] != p.segs[1]
        })
        .collect();
    assert_eq!(dashes.len(), 10, "{}", dashes.len());
    assert!(
        paths(&page.prims).iter().any(|p| p.stroke.is_none()),
        "the plain outline lost its stroke"
    );
    let sheets = select_sheets(ed.doc(), None).unwrap();
    let pdf = export_pdf(ed.doc(), ed.revision(), &mut PageCache::default(), &sheets).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
}

#[test]
fn symbols_and_groups_draw_on_a_sheet_and_in_a_viewport() {
    let mut ed = Editor::default();
    let sym = res(
        &mut ed,
        "Cross",
        GraphicResource::Symbol(SymbolDef {
            items: vec![
                SymbolItem {
                    shape: Shape::Line {
                        a: v(-5.0, 0.0),
                        b: v(5.0, 0.0),
                    },
                    attrs: Attributes::default(),
                    class: None,
                },
                SymbolItem {
                    shape: Shape::Line {
                        a: v(0.0, -5.0),
                        b: v(0.0, 5.0),
                    },
                    attrs: Attributes::default(),
                    class: None,
                },
            ],
            ..Default::default()
        }),
    );
    // In the model, at 1:100: a 1 m cross is 10 mm on paper.
    let inst = Shape::Symbol {
        symbol: sym,
        at: v(5000.0, 5000.0),
        rotation: 0.0,
        scale: 100.0,
        scale_y: None,
    };
    let model = ed
        .execute(&Command::AddObject {
            shape: inst,
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0];
    ed.execute(&Command::Group {
        ids: vec![ObjectId(model)],
        name: None,
    })
    .unwrap();
    let s = sheet(&mut ed);
    let vp = Viewport::new(
        v(10.0, 10.0),
        v(110.0, 110.0),
        v(5000.0, 5000.0),
        100.0,
        vec![FIRST_LAYER],
    );
    ed.execute(&Command::AddObject {
        shape: Shape::Viewport(Box::new(vp)),
        layer: Some(s),
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    // A symbol on the sheet itself, at paper size.
    let paper = Shape::Symbol {
        symbol: sym,
        at: v(150.0, 50.0),
        rotation: 0.0,
        scale: 1.0,
        scale_y: None,
    };
    ed.execute(&Command::AddObject {
        shape: paper,
        layer: Some(s),
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let page = compose_page(ed.doc(), s).unwrap();
    let strokes: Vec<_> = paths(&page.prims)
        .into_iter()
        .filter(|p| p.segs.len() == 2)
        .collect();
    // Two lines from the viewport, two from the sheet (the empty group draws nothing).
    assert!(strokes.len() >= 4, "{}", strokes.len());
    let long: Vec<f64> = strokes
        .iter()
        .map(|p| match (p.segs[0], p.segs[1]) {
            (PSeg::Move(a), PSeg::Line(b)) => a.distance(b),
            _ => 0.0,
        })
        .collect();
    assert!(
        long.iter().filter(|l| (**l - 10.0).abs() < 1e-6).count() >= 4,
        "{long:?}"
    );
}
