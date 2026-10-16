//! Tests of the drawing source: what is drawn is what the report counts.

use super::*;
use crate::command::{Command, Editor};
use crate::curve::{Vertex, corners};
use crate::hardscape::HardscapePatch;
use crate::hardscape::style::{Border, HardscapeStyle, JointKind, JointPattern};
use crate::layout::course::Side;
use crate::layout::plan::{CourseDef, Plan, SplitLine, ZoneSpec};
use crate::layout::summary::SummaryOptions;
use crate::layout::{Bond, MAX_UNITS};
use crate::model::{Attributes, Document, ObjectId};
use crate::resource::{ResourceBody, ResourceId};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn vx(x: f64, y: f64) -> Vertex {
    Vertex::corner(v(x, y))
}

/// A slab of `w` x `h` mm with a running bond of 200 x 100 mm units and a 5 mm joint, starting at
/// the origin, in a document with sequential IDs.
pub(crate) fn slab(w: f64, h: f64, angle: f64, border: bool) -> (Editor, ObjectId) {
    slab_with(
        w,
        h,
        angle,
        border,
        JointPattern {
            kind: JointKind::RunningBond,
            unit_length: 200.0,
            unit_width: 100.0,
            spacing: 5.0,
            ..Default::default()
        },
    )
}

pub(crate) fn slab_with(
    w: f64,
    h: f64,
    angle: f64,
    border: bool,
    joints: JointPattern,
) -> (Editor, ObjectId) {
    let mut ed = Editor::new(Document::with_sequential_ids());
    let style = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Pavers".into(),
            body: ResourceBody::HardscapeStyle(Box::new(HardscapeStyle {
                joints: Some(joints),
                border: border.then(|| Border {
                    width: 150.0,
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
                angle: Some(angle),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    );
    (ed, id)
}

pub(crate) fn set_plan(ed: &mut Editor, id: ObjectId, p: &Plan) {
    ed.execute(&Command::SetData {
        id,
        set: [(plan::KEY.to_string(), serde_json::to_value(p).unwrap())]
            .into_iter()
            .collect(),
        remove: vec![],
    })
    .unwrap();
}

fn vline(x: f64, h: f64) -> SplitLine {
    SplitLine {
        source: None,
        points: vec![vx(x, -50.0), vx(x, h + 50.0)],
    }
}

/// A lot with two zones (the right one with its own pattern, angle and origin), a kerb course
/// along the bottom edge and a field.
fn lot_with_plan() -> (Editor, ObjectId) {
    let (mut ed, id) = slab(8000.0, 4000.0, 0.0, false);
    let mut p = Plan::default();
    p.splits = vec![vline(5000.0, 4000.0)];
    p.zones = vec![ZoneSpec {
        name: Some("Stalls".into()),
        seed: v(7000.0, 2000.0),
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
    set_plan(&mut ed, id, &p);
    (ed, id)
}

/// The units that the report counts, per group name: (whole, cut).
fn report_counts(doc: &Document, id: ObjectId) -> Vec<(String, usize, usize)> {
    let o = doc.object(id).unwrap();
    let planned = plan::groups_of(doc, o, layout::TOLERANCE, MAX_UNITS)
        .unwrap()
        .unwrap();
    planned
        .groups
        .iter()
        .map(|g| {
            let s = g.summarize(&SummaryOptions::default()).unwrap();
            (g.name.clone(), s.whole_units, s.cut_units)
        })
        .collect()
}

/// The units that an SVG draws, per group name: whole units are the subpaths of the whole-units
/// path, cut units are paths of their own.
fn svg_counts(svg: &str) -> Vec<(String, usize, usize)> {
    svg.split(r#"<g data-part="group""#)
        .skip(1)
        .map(|block| {
            let name = block
                .split(r#"data-group=""#)
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
                .to_string();
            let whole: usize = block
                .lines()
                .filter(|l| l.contains(r#"data-part="units-whole""#))
                .map(|l| l.matches("M ").count())
                .sum();
            let cut = block.matches(r#"data-part="unit-cut""#).count();
            // The counts that the group says are the counts drawn.
            let said = |a: &str| -> usize {
                block
                    .split(&format!(r#"{a}=""#))
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            };
            assert_eq!(said("data-whole"), whole);
            assert_eq!(said("data-cut"), cut);
            (name, whole, cut)
        })
        .collect()
}

fn svg_of(ed: &Editor, id: ObjectId, ctx: SvgContext) -> String {
    let o = ed.doc().object(id).unwrap();
    with_context(ctx, || {
        crate::hardscape::svg_extras(ed.doc(), o, 100.0, "black")
    })
}

#[test]
fn the_units_drawn_are_the_units_of_the_report_for_a_plan_with_zones_and_a_course() {
    let (ed, id) = lot_with_plan();
    let svg = svg_of(&ed, id, SvgContext::default());
    let drawn = svg_counts(&svg);
    let report = report_counts(ed.doc(), id);
    assert!(drawn.len() >= 3, "zones and a kerb course: {drawn:?}");
    assert_eq!(drawn, report);
    assert!(drawn.iter().all(|(_, w, c)| w + c > 0));
    assert!(drawn.iter().map(|(_, w, _)| w).sum::<usize>() > 100);
    assert!(svg.contains(r#"data-detail="full""#));
    assert!(svg.contains(r#"data-role="course""#) && svg.contains(r#"data-role="zone""#));
    // The zone edges are a thin dashed line.
    assert!(svg.contains(r#"data-part="zone-edges""#) && svg.contains("stroke-dasharray"));
}

#[test]
fn a_plain_lot_a_lot_with_a_border_and_a_module_draw_the_report_counts() {
    // No plan.
    let (ed, id) = slab(3000.0, 1500.0, 0.0, false);
    assert_eq!(
        svg_counts(&svg_of(&ed, id, SvgContext::default())),
        report_counts(ed.doc(), id)
    );
    // A kerb border and a turn.
    let (ed, id) = slab(3000.0, 1500.0, 30.0, true);
    assert_eq!(
        svg_counts(&svg_of(&ed, id, SvgContext::default())),
        report_counts(ed.doc(), id)
    );
    // A module: a square and a bar.
    let module = JointPattern {
        kind: JointKind::Module,
        spacing: 4.0,
        units: vec![
            crate::layout::ModuleUnit {
                name: "square".into(),
                outline: corners(&[v(0.0, 0.0), v(200.0, 0.0), v(200.0, 200.0), v(0.0, 200.0)]),
            },
            crate::layout::ModuleUnit {
                name: "bar".into(),
                outline: corners(&[
                    v(200.0, 0.0),
                    v(400.0, 0.0),
                    v(400.0, 100.0),
                    v(200.0, 100.0),
                ]),
            },
        ],
        a: Some([400.0, 0.0]),
        b: Some([0.0, 200.0]),
        ..Default::default()
    };
    let (ed, id) = slab_with(2000.0, 1200.0, 0.0, false, module);
    let drawn = svg_counts(&svg_of(&ed, id, SvgContext::default()));
    assert_eq!(drawn, report_counts(ed.doc(), id));
    assert!(drawn[0].1 > 10, "{drawn:?}");
}

#[test]
fn cut_units_are_marked_and_whole_units_are_plain() {
    let (ed, id) = slab(3000.0, 1550.0, 0.0, false);
    let svg = svg_of(&ed, id, SvgContext::default());
    let (_, whole, cut) = svg_counts(&svg)[0].clone();
    assert!(whole > 20 && cut > 5, "{whole} {cut}");
    for line in svg
        .lines()
        .filter(|l| l.contains(r#"data-part="unit-cut""#))
    {
        assert!(line.contains(r#"fill-opacity="0.18""#), "{line}");
    }
    for line in svg
        .lines()
        .filter(|l| l.contains(r#"data-part="units-whole""#))
    {
        assert!(line.contains(r#"fill="none""#) && !line.contains("fill-opacity"));
    }
}

#[test]
fn a_view_with_more_units_than_the_bound_is_a_preview_that_says_so() {
    let (ed, id) = slab(8000.0, 4000.0, 0.0, false);
    let ctx = SvgContext {
        bound: 100,
        ..Default::default()
    };
    let svg = svg_of(&ed, id, ctx);
    assert!(svg.contains(r#"data-detail="preview""#));
    assert!(svg.contains(PREVIEW_LABEL));
    assert!(svg.contains(r#"data-part="preview-note""#));
    // A preview draws the gap lines of the whole plan, and no units.
    assert!(svg.contains(r#"data-part="joints""#));
    assert!(!svg.contains(r#"data-part="units-whole""#));
    // The same lot with a bound above its units is full detail, without the label.
    let full = svg_of(&ed, id, SvgContext::default());
    assert!(full.contains(r#"data-detail="full""#) && !full.contains(PREVIEW_LABEL));
}

#[test]
fn a_zoomed_render_of_part_of_a_big_lot_shows_the_real_units() {
    // 5,000 m2 of 200 x 100 mm units: about 240,000 units, and a preview as a whole.
    let (ed, id) = slab(100_000.0, 50_000.0, 15.0, false);
    let whole = svg_of(&ed, id, SvgContext::default());
    assert!(whole.contains(r#"data-detail="preview""#));
    // A window of 2 m x 1 m in the middle.
    let view = (v(40_000.0, 20_000.0), v(42_000.0, 21_000.0));
    let zoomed = svg_of(
        &ed,
        id,
        SvgContext {
            view: Some(view),
            ..Default::default()
        },
    );
    assert!(zoomed.contains(r#"data-detail="full""#), "{zoomed:.300}");
    let (_, w, c) = svg_counts(&zoomed)[0].clone();
    // About (2000 x 1000) / (205 x 105) = 93 units, and the ones that touch the border of the
    // window. Nothing is cut: the window is inside the lot.
    assert!((80..160).contains(&(w + c)), "{w} {c}");
    assert!(zoomed.len() < 100_000, "only the window is written");
    // The units of the window are the units of the whole lot that are there: the same drawing in
    // the window and cut from the whole.
    let o = ed.doc().object(id).unwrap();
    let p = prepared(ed.doc(), o).unwrap().unwrap();
    let all = p.draw(
        None,
        &DrawOptions {
            bound: usize::MAX,
            ..DrawOptions::units(0)
        },
    );
    let key = |r: &[DVec2]| {
        let (lo, hi) = box_of_ring(r);
        (
            (lo.x * 10.0).round() as i64,
            (lo.y * 10.0).round() as i64,
            (hi.x * 10.0).round() as i64,
        )
    };
    let inside = |r: &[DVec2]| boxes_meet(box_of_ring(r), view);
    let mut a: Vec<_> = all.groups[0]
        .whole
        .iter()
        .filter(|r| inside(r))
        .map(|r| key(r))
        .collect();
    let win = p.draw(Some(view), &DrawOptions::units(SVG_BOUND));
    assert_eq!(win.detail, Detail::Full);
    let mut b: Vec<_> = win.groups[0].whole.iter().map(|r| key(r)).collect();
    a.sort();
    b.sort();
    assert!(!a.is_empty());
    assert_eq!(a, b);
}

#[test]
fn a_window_over_a_lot_with_a_hole_cuts_the_same_units_as_the_whole_lot() {
    let (mut ed, id) = slab(6000.0, 4000.0, 20.0, false);
    ed.execute(&Command::SetHardscape {
        ids: vec![id],
        patch: HardscapePatch {
            holes: Some(vec![corners(&[
                v(2000.0, 1000.0),
                v(2900.0, 1000.0),
                v(2900.0, 1700.0),
                v(2000.0, 1700.0),
            ])]),
            ..Default::default()
        },
    })
    .unwrap();
    let o = ed.doc().object(id).unwrap();
    let p = prepared(ed.doc(), o).unwrap().unwrap();
    let all = p.draw(None, &DrawOptions::units(usize::MAX / 8));
    let view = (v(1500.0, 600.0), v(3500.0, 2100.0));
    let win = p.draw(Some(view), &DrawOptions::units(SVG_BOUND));
    assert_eq!(win.detail, Detail::Full);
    let sig = |g: &DrawnGroup| {
        let mut s: Vec<(bool, i64, i64)> = g
            .whole
            .iter()
            .map(|r| (true, r))
            .chain(g.cut.iter().map(|u| (false, &u[0])))
            .filter(|(_, r)| boxes_meet(box_of_ring(r), view))
            .map(|(w, r)| {
                let (lo, hi) = box_of_ring(r);
                (
                    w,
                    (lo.x * 100.0).round() as i64,
                    (hi.y * 100.0).round() as i64,
                )
            })
            .collect();
        s.sort();
        s
    };
    assert_eq!(sig(&all.groups[0]), sig(&win.groups[0]));
    assert!(!win.groups[0].cut.is_empty(), "the hole cuts units");
}

#[test]
fn the_drawing_is_the_same_every_time() {
    let (ed, id) = lot_with_plan();
    let o = ed.doc().object(id).unwrap();
    let key = content_key(ed.doc(), o, layout::TOLERANCE);
    // Two plans laid apart draw the same.
    let a = Prepared::new(ed.doc(), o, layout::TOLERANCE, key).unwrap();
    let b = Prepared::new(ed.doc(), o, layout::TOLERANCE, key).unwrap();
    let opts = DrawOptions::units(SVG_BOUND);
    assert_eq!(a.draw(None, &opts), b.draw(None, &opts));
    let small = DrawOptions::units(10);
    assert_eq!(a.draw(None, &small), b.draw(None, &small));
    let once = svg_of(&ed, id, SvgContext::default());
    assert_eq!(once, svg_of(&ed, id, SvgContext::default()));
}

#[test]
fn the_laid_plan_is_kept_until_the_object_changes() {
    let cache = PlanCache::new(4);
    let get = |ed: &Editor, id: ObjectId| {
        let o = ed.doc().object(id).unwrap();
        let key = content_key(ed.doc(), o, layout::TOLERANCE);
        cache.get(ed.doc(), o, key).unwrap().unwrap()
    };
    let (mut ed, id) = lot_with_plan();
    let first = get(&ed, id);
    assert!(Arc::ptr_eq(&first, &get(&ed, id)));
    assert_eq!(cache.builds(), 1);
    // Another object is added: the plan is not laid again.
    ed.execute(&Command::AddObject {
        shape: crate::model::Shape::Circle {
            center: v(0.0, 0.0),
            radius: 10.0,
        },
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    assert!(Arc::ptr_eq(&first, &get(&ed, id)));
    assert_eq!(cache.builds(), 1);
    // The plan changes: it is laid again.
    let mut p = plan::data(ed.doc().object(id).unwrap()).unwrap();
    p.courses.clear();
    set_plan(&mut ed, id, &p);
    let fresh = get(&ed, id);
    assert!(!Arc::ptr_eq(&first, &fresh));
    assert_eq!(cache.builds(), 2);
    assert_eq!(
        fresh
            .planned
            .groups
            .iter()
            .filter(|g| g.role == Role::Course)
            .count(),
        0
    );
    // A change of the outline too.
    ed.execute(&Command::SetHardscape {
        ids: vec![id],
        patch: HardscapePatch {
            angle: Some(10.0),
            ..Default::default()
        },
    })
    .unwrap();
    get(&ed, id);
    assert_eq!(cache.builds(), 3);
}

#[test]
fn the_findings_of_the_check_are_marked_on_request() {
    // A strip of 3 mm is left at the right edge: small pieces.
    let (ed, id) = slab(8003.0, 400.0, 0.0, false);
    let plain = svg_of(&ed, id, SvgContext::default());
    assert!(!plain.contains(r#"data-part="check""#));
    let checked = svg_of(
        &ed,
        id,
        SvgContext {
            show_check: true,
            ..Default::default()
        },
    );
    assert!(checked.contains(r#"data-part="check""#));
    assert!(
        checked.contains(r#"data-kind="small_piece""#),
        "{checked:.200}"
    );
    // One marker for each finding of the report.
    let o = ed.doc().object(id).unwrap();
    let p = prepared(ed.doc(), o).unwrap().unwrap();
    let n = p.findings().as_ref().unwrap().findings.len();
    assert!(n > 0);
    assert_eq!(checked.matches(r#"data-part="finding""#).count(), n);
}

#[test]
fn a_pattern_that_cannot_be_laid_says_why_in_the_drawing() {
    // The border covers the whole area.
    let (ed, id) = slab(200.0, 200.0, 0.0, true);
    let svg = svg_of(&ed, id, SvgContext::default());
    assert!(svg.contains(r#"data-error=""#), "{svg}");
}

/// Measures the size and the time of the SVG of a lot of 5,000 m2 (`cargo test -p kadakas-core
/// --lib measure_bounds -- --ignored --nocapture`).
#[test]
#[ignore = "a measurement"]
fn measure_bounds() {
    for (label, w, h) in [
        ("1,250 m2, 60,000 units", 50_000.0, 25_000.0),
        ("5,000 m2, 240,000 units", 100_000.0, 50_000.0),
    ] {
        let (ed, id) = slab(w, h, 15.0, false);
        let o = ed.doc().object(id).unwrap();
        let t = std::time::Instant::now();
        let p = Prepared::new(ed.doc(), o, layout::TOLERANCE, 0).unwrap();
        let lay = t.elapsed();
        let t = std::time::Instant::now();
        let d = p.draw(None, &DrawOptions::units(usize::MAX / 8));
        let cut = t.elapsed();
        let (whole, cuts) = d.unit_counts();
        let t = std::time::Instant::now();
        let svg = svg_of(
            &ed,
            id,
            SvgContext {
                bound: usize::MAX / 8,
                ..Default::default()
            },
        );
        let svg_time = t.elapsed();
        let t = std::time::Instant::now();
        let preview = svg_of(
            &ed,
            id,
            SvgContext {
                bound: 10,
                ..Default::default()
            },
        );
        let preview_time = t.elapsed();
        println!(
            "{label}: {whole} whole + {cuts} cut units; plan {:.0} ms, units {:.0} ms; SVG with every unit {:.1} MB in {:.0} ms; preview SVG {:.2} MB in {:.0} ms",
            lay.as_secs_f64() * 1e3,
            cut.as_secs_f64() * 1e3,
            svg.len() as f64 / 1e6,
            svg_time.as_secs_f64() * 1e3,
            preview.len() as f64 / 1e6,
            preview_time.as_secs_f64() * 1e3,
        );
    }
}
