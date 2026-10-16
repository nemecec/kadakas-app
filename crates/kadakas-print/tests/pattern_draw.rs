//! The pattern plan of a hardscape (zones, courses, their own patterns) on a sheet: the units drawn
//! in a viewport are the units of the report; a viewport with more units than the print bound
//! draws a preview and says so on the sheet.

use glam::DVec2;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::{Vertex, corners};
use kadakas_core::hardscape::draw::PREVIEW_LABEL;
use kadakas_core::hardscape::{HardscapePatch, HardscapeStyle, JointKind, JointPattern};
use kadakas_core::layout::course::Side;
use kadakas_core::layout::plan::{self, CourseDef, Plan, SplitLine, ZoneSpec};
use kadakas_core::layout::summary::SummaryOptions;
use kadakas_core::layout::{Bond, MAX_UNITS, Pattern};
use kadakas_core::model::*;
use kadakas_core::resource::{ResourceBody, ResourceId};
use kadakas_core::sheets::{PaperSize, Viewport};
use kadakas_print::compose_page;
use kadakas_print::display::*;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn vx(x: f64, y: f64) -> Vertex {
    Vertex::corner(v(x, y))
}

/// A lot of `w` x `h` mm with 200 x 100 mm units, on a sheet with one plan viewport at `scale`
/// that shows `center`.
fn lot(w: f64, h: f64, planned: bool, scale: f64, center: DVec2) -> (Editor, LayerId, ObjectId) {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let style = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Pavers".into(),
            body: ResourceBody::HardscapeStyle(Box::new(HardscapeStyle {
                joints: Some(JointPattern {
                    kind: JointKind::RunningBond,
                    unit_length: 200.0,
                    unit_width: 100.0,
                    spacing: 5.0,
                    ..Default::default()
                }),
                ..Default::default()
            })),
        })
        .unwrap()
        .created[0],
    );
    let id = ObjectId(
        ed.execute(&Command::AddHardscape {
            boundary: Some(corners(&[v(0.0, 0.0), v(w, 0.0), v(w, h), v(0.0, h)])),
            patch: Box::new(HardscapePatch {
                style: Some(style),
                origin: Some(v(0.0, 0.0)),
                angle: Some(0.0),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    if planned {
        let mut p = Plan::default();
        p.splits = vec![SplitLine {
            source: None,
            points: vec![vx(5000.0, -50.0), vx(5000.0, h + 50.0)],
        }];
        p.zones = vec![ZoneSpec {
            name: Some("Stalls".into()),
            seed: v(w - 500.0, h / 2.0),
            pattern: Some(Pattern::new(Bond::Stack, 300.0, 150.0, 4.0)),
            origin: Some(v(5000.0, 0.0)),
            angle: Some(90.0),
            ..Default::default()
        }];
        p.courses = vec![CourseDef {
            name: "Kerb".into(),
            path: vec![vx(0.0, 100.0), vx(5000.0, 100.0)],
            side: Side::Left,
            along: 100.0,
            depth: 200.0,
            gap: 5.0,
            ..Default::default()
        }];
        ed.execute(&Command::SetData {
            id,
            set: [(plan::KEY.to_string(), serde_json::to_value(&p).unwrap())]
                .into_iter()
                .collect(),
            remove: vec![],
        })
        .unwrap();
    }
    let sheet = LayerId(
        ed.execute(&Command::AddSheet {
            name: "Plan".into(),
            paper: Some(PaperSize::A1),
            orientation: None,
            number: None,
            title: None,
        })
        .unwrap()
        .created[0],
    );
    let vp = Viewport::new(
        v(20.0, 40.0),
        v(820.0, 560.0),
        center,
        scale,
        vec![FIRST_LAYER],
    );
    ed.execute(&Command::AddObject {
        shape: Shape::Viewport(Box::new(vp)),
        layer: Some(sheet),
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    (ed, sheet, id)
}

fn prims(ed: &Editor, sheet: LayerId) -> Vec<Prim> {
    compose_page(ed.doc(), sheet).unwrap().prims
}

/// The units drawn: the subpaths of the stroke-only paths of hairline weight (whole units), and
/// the paths with the light fill (cut units).
fn drawn_units(prims: &[Prim]) -> (usize, usize) {
    let (mut whole, mut cut) = (0, 0);
    for p in prims {
        let Prim::Path(p) = p else { continue };
        // A unit is a subpath of a start, three lines or more and the line back to the start.
        let mut moves = 0;
        let mut len = 0;
        for s in p.segs.iter().chain([&PSeg::Move(DVec2::ZERO)]) {
            if matches!(s, PSeg::Move(_)) {
                moves += usize::from(len > 3);
                len = 0;
            }
            len += 1;
        }
        if p.fill.is_some_and(|f| f[3] == 46) {
            cut += 1;
        } else if !p.closed
            && p.fill.is_none()
            && p.dash.is_none()
            && (p.width - 0.1).abs() < 1e-9
            && p.stroke.is_some()
            && p.segs.len() > 2
        {
            whole += moves;
        }
    }
    (whole, cut)
}

fn report(ed: &Editor, id: ObjectId) -> (usize, usize) {
    let doc = ed.doc();
    let planned = plan::groups_of(doc, doc.object(id).unwrap(), 1.0, MAX_UNITS)
        .unwrap()
        .unwrap();
    planned.groups.iter().fold((0, 0), |(w, c), g| {
        let s = g.summarize(&SummaryOptions::default()).unwrap();
        (w + s.whole_units, c + s.cut_units)
    })
}

fn notes(prims: &[Prim]) -> Vec<String> {
    prims
        .iter()
        .filter_map(|p| {
            if let Prim::Text(t) = p {
                Some(t.text.clone())
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn a_viewport_draws_the_units_of_the_report_with_zones_and_courses() {
    // 1:50 on an A1 sheet shows 40 m x 26 m.
    let (ed, sheet, id) = lot(8000.0, 4000.0, true, 20.0, v(4000.0, 2000.0));
    let all = prims(&ed, sheet);
    let (whole, cut) = drawn_units(&all);
    assert_eq!((whole, cut), report(&ed, id));
    assert!(whole > 500 && cut > 20, "{whole} {cut}");
    assert!(!notes(&all).iter().any(|t| t.contains(PREVIEW_LABEL)));
    // The zone edges are dashed.
    assert!(all.iter().any(
        |p| matches!(p, Prim::Path(p) if p.dash == Some([2.0, 1.0]) && p.closed && p.fill.is_none())
    ));
    // The same page twice is the same.
    assert_eq!(all, prims(&ed, sheet));
}

#[test]
fn a_viewport_over_a_plain_lot_draws_the_units_of_the_report() {
    let (ed, sheet, id) = lot(6000.0, 3000.0, false, 20.0, v(3000.0, 1500.0));
    assert_eq!(drawn_units(&prims(&ed, sheet)), report(&ed, id));
}

#[test]
fn a_viewport_with_more_units_than_the_bound_draws_a_preview_and_says_so() {
    // 1:100 shows 80 m x 52 m: a lot of 60 m x 30 m has 90,000 units.
    let (ed, sheet, _) = lot(60_000.0, 30_000.0, false, 100.0, v(30_000.0, 15_000.0));
    let all = prims(&ed, sheet);
    let t = notes(&all);
    assert!(t.iter().any(|t| t.contains(PREVIEW_LABEL)), "{t:?}");
    assert_eq!(drawn_units(&all), (0, 0), "a preview draws gap lines");
    // The note is on the sheet in the SVG too.
    let svg = kadakas_print::svg::page_svg(&compose_page(ed.doc(), sheet).unwrap());
    assert!(svg.contains(PREVIEW_LABEL));
}

#[test]
fn a_viewport_over_part_of_a_big_lot_draws_the_real_units() {
    // 1:20 shows 16 m x 10.4 m of the same lot.
    let (ed, sheet, _) = lot(60_000.0, 30_000.0, false, 20.0, v(30_000.0, 15_000.0));
    let all = prims(&ed, sheet);
    assert!(!notes(&all).iter().any(|t| t.contains(PREVIEW_LABEL)));
    let (whole, cut) = drawn_units(&all);
    // 16 m x 10.4 m over 205 x 105 mm cells, and the units that touch the frame.
    assert!((7000..9000).contains(&(whole + cut)), "{whole} {cut}");
}

#[test]
fn units_too_small_for_the_paper_are_not_drawn_and_the_sheet_says_so() {
    let (ed, sheet, _) = lot(6000.0, 3000.0, false, 2000.0, v(3000.0, 1500.0));
    let all = prims(&ed, sheet);
    assert_eq!(drawn_units(&all), (0, 0));
    assert!(notes(&all).iter().any(|t| t.contains("pattern not drawn")));
}
