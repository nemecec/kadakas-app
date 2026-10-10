//! Section and elevation viewports on a sheet: cut faces with the hatch of their class, the
//! objects beyond, live updates by revision, and the outputs.

use cadektor_core::command::{Command, Editor};
use cadektor_core::graphics::{GraphicResource, HatchDef, HatchLine, PatternUnits};
use cadektor_core::model::*;
use cadektor_core::resource::{ResourceBody, ResourceId};
use cadektor_core::section::{KEY, SectionView};
use cadektor_core::sheets::Viewport;
use cadektor_print::display::{PSeg, PathPrim, Prim};
use cadektor_print::{SheetCache, compose_page, export_pdf, select_sheets, svg};
use glam::DVec2;
use serde_json::json;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn ring(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Vertex> {
    [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
        .iter()
        .map(|(x, y)| Vertex::corner(v(*x, *y)))
        .collect()
}

struct Project {
    ed: Editor,
    sheet: LayerId,
    line: ObjectId,
    wall: ObjectId,
}

/// A wall block 4000 x 600 mm in plan and 2000 mm tall (top at 2000), a class with a 100 mm
/// hatch, a section line across the wall, and a sheet with a section viewport at 1:50.
fn project(look: &str) -> Project {
    let mut ed = Editor::default();
    let hatch = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Diagonal".into(),
            body: ResourceBody::Graphic(Box::new(GraphicResource::Hatch(HatchDef {
                units: PatternUnits::World,
                lines: vec![HatchLine::solid(45.0, 200.0)],
            }))),
        })
        .unwrap()
        .created[0],
    );
    let class = ClassId(
        ed.execute(&Command::AddClass {
            name: "Concrete".into(),
            attrs: Attributes {
                fill: Some(Color([200, 200, 190, 255])),
                fill_pattern: Some(hatch),
                ..Default::default()
            },
        })
        .unwrap()
        .created[0],
    );
    let wall = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Hardscape {
                boundary: ring(0.0, 0.0, 4000.0, 600.0),
                material: "Concrete".into(),
                thickness: Some(2000.0),
                elevation: Some(2000.0),
            },
            layer: None,
            class: Some(class),
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    // A second block further away, to see beyond the cut.
    ed.execute(&Command::AddObject {
        shape: Shape::Hardscape {
            boundary: ring(4200.0, 3000.0, 4900.0, 4000.0),
            material: "Stone".into(),
            thickness: Some(500.0),
            elevation: Some(1000.0),
        },
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let line = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Line {
                a: v(-1000.0, 300.0),
                b: v(5000.0, 300.0),
            },
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    ed.execute(&Command::SetData {
        id: line,
        set: [(KEY.to_string(), json!({"name": "A", "look": look}))].into(),
        remove: vec![],
    })
    .unwrap();
    let sheet = LayerId(
        ed.execute(&Command::AddSheet {
            name: "S".into(),
            paper: None,
            orientation: None,
            number: Some("1".into()),
            title: None,
        })
        .unwrap()
        .created[0],
    );
    let design: Vec<LayerId> = ed
        .doc()
        .layers
        .iter()
        .filter(|l| l.kind == LayerKind::Design)
        .map(|l| l.id)
        .collect();
    let mut vp = Viewport::new(
        v(20.0, 20.0),
        v(220.0, 120.0),
        v(3000.0, 1000.0),
        50.0,
        design,
    );
    vp.view = "section".into();
    vp.section = Some(SectionView::new(line));
    vp.snapshot_visibility(ed.doc());
    let _viewport = ObjectId(
        ed.execute(&Command::AddObject {
            shape: Shape::Viewport(Box::new(vp)),
            layer: Some(sheet),
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    Project {
        ed,
        sheet,
        line,
        wall,
    }
}

fn paths(prims: &[Prim]) -> Vec<&PathPrim> {
    prims
        .iter()
        .filter_map(|p| if let Prim::Path(p) = p { Some(p) } else { None })
        .collect()
}

fn points(p: &PathPrim) -> Vec<DVec2> {
    p.segs
        .iter()
        .filter_map(|s| match s {
            PSeg::Move(q) | PSeg::Line(q) => Some(*q),
            _ => None,
        })
        .collect()
}

fn bounds(pts: &[DVec2]) -> (DVec2, DVec2) {
    pts.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(l, h), p| (l.min(*p), h.max(*p)),
    )
}

#[test]
fn the_cut_face_is_filled_hatched_and_drawn_to_scale() {
    let p = project("left");
    let page = compose_page(p.ed.doc(), p.sheet).unwrap();
    let all = paths(&page.prims);
    // The filled face: the class fill, a rectangle 4000 x 2000 mm at 1:50 = 80 x 40 mm on paper.
    let filled: Vec<&&PathPrim> = all
        .iter()
        .filter(|q| q.fill == Some([200, 200, 190, 255]))
        .collect();
    assert_eq!(filled.len(), 1);
    let (lo, hi) = bounds(&points(filled[0]));
    assert!(
        (hi.x - lo.x - 80.0).abs() < 0.01 && (hi.y - lo.y - 40.0).abs() < 0.01,
        "{lo} {hi}"
    );
    // The center of the viewport is the section point (3000, 1000). The wall runs from u = 1000
    // to u = 5000 (the line starts at x = -1000), and from height 0 to 2000.
    let frame_c = v(120.0, 70.0);
    assert!(
        (lo.x - (frame_c.x + (1000.0 - 3000.0) / 50.0)).abs() < 0.01,
        "{lo}"
    );
    assert!(
        (lo.y - (frame_c.y + (0.0 - 1000.0) / 50.0)).abs() < 0.01,
        "{lo}"
    );
    // Hatch lines at 45 degrees every 200 mm of model: a few dozen strokes, all inside the face.
    let hatch: Vec<&&PathPrim> = all
        .iter()
        .filter(|q| q.fill.is_none() && q.segs.len() == 2 && q.stroke.is_some())
        .filter(|q| {
            let pts = points(q);
            pts.iter().all(|c| {
                c.x >= lo.x - 1e-6 && c.x <= hi.x + 1e-6 && c.y >= lo.y - 1e-6 && c.y <= hi.y + 1e-6
            }) && (pts[0].x - pts[1].x).abs() > 1e-6
                && ((pts[1].y - pts[0].y) / (pts[1].x - pts[0].x) - 1.0).abs() < 1e-6
        })
        .collect();
    assert!(hatch.len() >= 20, "hatch strokes: {}", hatch.len());
    // The outline of the cut is heavier than the lines beyond.
    let outline = all.iter().find(|q| {
        q.closed && q.fill.is_none() && q.width >= 0.5 - 1e-9 && bounds(&points(q)) == (lo, hi)
    });
    assert!(outline.is_some());
    // The block beyond shows as thin lines.
    assert!(
        all.iter()
            .any(|q| q.width < 0.2 && q.fill.is_none() && !q.closed)
    );
}

#[test]
fn looking_the_other_way_shows_the_other_side() {
    let left = project("left");
    let right = project("right");
    let count_beyond = |p: &Project| {
        let page = compose_page(p.ed.doc(), p.sheet).unwrap();
        paths(&page.prims)
            .iter()
            .filter(|q| q.width < 0.2 && q.fill.is_none() && !q.closed)
            .count()
    };
    // The small block is on the left side of the line. Looking right, it is in front: not drawn.
    assert!(
        count_beyond(&left) > count_beyond(&right),
        "{} {}",
        count_beyond(&left),
        count_beyond(&right)
    );
}

#[test]
fn the_sheet_follows_the_model_and_is_cached_by_revision() {
    let mut p = project("left");
    let mut cache = SheetCache::default();
    let a = cache.page(p.ed.doc(), 1, p.sheet).unwrap();
    let b = cache.page(p.ed.doc(), 1, p.sheet).unwrap();
    assert!(std::sync::Arc::ptr_eq(&a, &b));
    assert_eq!(cache.builds, 1);
    // Make the wall taller: the next revision draws a taller face.
    p.ed.execute(&Command::ModifyObject {
        id: p.wall,
        shape: Some(Shape::Hardscape {
            boundary: ring(0.0, 0.0, 4000.0, 600.0),
            material: "Concrete".into(),
            thickness: Some(3000.0),
            elevation: Some(3000.0),
        }),
        layer: None,
        class: None,
        attrs: None,
    })
    .unwrap();
    let c = cache.page(p.ed.doc(), 2, p.sheet).unwrap();
    assert_eq!(cache.builds, 2);
    let height = |page: &cadektor_print::Page| {
        let f = paths(&page.prims)
            .into_iter()
            .find(|q| q.fill == Some([200, 200, 190, 255]))
            .map(points)
            .unwrap();
        let (lo, hi) = bounds(&f);
        hi.y - lo.y
    };
    assert!((height(&a) - 40.0).abs() < 0.01);
    assert!((height(&c) - 60.0).abs() < 0.01);
    // Moving the section line off the wall removes the cut.
    p.ed.execute(&Command::ModifyObject {
        id: p.line,
        shape: Some(Shape::Line {
            a: v(-1000.0, 2000.0),
            b: v(5000.0, 2000.0),
        }),
        layer: None,
        class: None,
        attrs: None,
    })
    .unwrap();
    let d = cache.page(p.ed.doc(), 3, p.sheet).unwrap();
    assert!(
        paths(&d.prims)
            .iter()
            .all(|q| q.fill != Some([200, 200, 190, 255]))
    );
}

#[test]
fn elevations_draw_lines_without_a_cut() {
    let mut p = project("left");
    p.ed.execute(&Command::SetData {
        id: p.line,
        set: [(
            KEY.to_string(),
            json!({"name": "E", "mode": "elevation", "depth": 10000.0}),
        )]
        .into(),
        remove: vec![],
    })
    .unwrap();
    let page = compose_page(p.ed.doc(), p.sheet).unwrap();
    let all = paths(&page.prims);
    assert!(
        all.iter().all(|q| q.fill != Some([200, 200, 190, 255])),
        "no cut face"
    );
    assert!(all.iter().filter(|q| q.width < 0.2).count() >= 4);
}

#[test]
fn a_missing_or_broken_line_draws_nothing() {
    let mut p = project("left");
    p.ed.execute(&Command::DeleteObjects { ids: vec![p.line] })
        .unwrap();
    let page = compose_page(p.ed.doc(), p.sheet).unwrap();
    assert!(
        paths(&page.prims)
            .iter()
            .all(|q| q.fill != Some([200, 200, 190, 255]))
    );
    // The viewport is still a valid object that prints.
    assert!(page.prims.iter().any(|q| matches!(q, Prim::PushClip(_))));
}

#[test]
fn sections_print_to_svg_and_pdf() {
    let p = project("left");
    let page = compose_page(p.ed.doc(), p.sheet).unwrap();
    let text = svg::page_svg(&page);
    assert!(
        text.contains("<path") && text.contains("rgb(200,200,190)") || text.contains("200,200,190")
    );
    let sheets = select_sheets(p.ed.doc(), None).unwrap();
    let mut cache = SheetCache::default();
    let bytes = export_pdf(p.ed.doc(), 1, &mut cache, &sheets).unwrap();
    assert!(bytes.starts_with(b"%PDF"));
    assert!(bytes.len() > 500);
}
