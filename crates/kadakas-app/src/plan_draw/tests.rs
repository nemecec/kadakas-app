//! Tests of the pattern plan on the canvas, in a headless egui frame: what is painted is what the
//! report counts, the simplifications say so, the laid plan is kept, a compare keeps the live plan,
//! twenty lots stay smooth, a check and a lay never block a frame, and the sheet view of the app
//! draws what the PDF has.

use super::*;
use crate::canvas::Style;
use crate::plan_worker::pushed;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::{Vertex, corners};
use kadakas_core::hardscape::HardscapePatch;
use kadakas_core::hardscape::style::{HardscapeStyle, JointKind, JointPattern};
use kadakas_core::layout::course::Side;
use kadakas_core::layout::plan::{self, CourseDef, Plan, SplitLine, ZoneSpec};
use kadakas_core::layout::summary::SummaryOptions;
use kadakas_core::layout::{Bond, MAX_UNITS, Pattern};
use kadakas_core::model::Attributes;
use kadakas_core::resource::{ResourceBody, ResourceId};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn vx(x: f64, y: f64) -> Vertex {
    Vertex::corner(v(x, y))
}

fn style_body(unit_length: f64, unit_width: f64) -> ResourceBody {
    ResourceBody::HardscapeStyle(Box::new(HardscapeStyle {
        joints: Some(JointPattern {
            kind: JointKind::RunningBond,
            unit_length,
            unit_width,
            spacing: 5.0,
            ..Default::default()
        }),
        ..Default::default()
    }))
}

thread_local! {
    /// A part of the plan cache that only this test thread sees (until the thread ends), so that
    /// tests that run in parallel and lay the same lot do not find, push out or empty each
    /// other's plans.
    static SCOPE: draw::CacheScope = draw::cache_scope();
}

/// Takes the private part of the plan cache for this test.
fn isolate() {
    SCOPE.with(|_| {});
}

/// A lot of `w` x `h` mm with running-bond units of 200 x 100 mm and a 5 mm joint.
fn lot(w: f64, h: f64, planned: bool) -> (Editor, ObjectId) {
    isolate();
    let mut ed = Editor::new(Document::with_sequential_ids());
    let style = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Pavers".into(),
            body: style_body(200.0, 100.0),
        })
        .unwrap()
        .created[0],
    );
    let id = add_lot(&mut ed, style, v(0.0, 0.0), w, h);
    if planned {
        let p = Plan {
            splits: vec![SplitLine {
                source: None,
                points: vec![vx(w / 2.0, -50.0), vx(w / 2.0, h + 50.0)],
            }],
            courses: vec![CourseDef {
                name: "Kerb".into(),
                path: vec![vx(0.0, 100.0), vx(w / 2.0, 100.0)],
                side: Side::Left,
                along: 100.0,
                depth: 200.0,
                gap: Some(5.0),
                ..Default::default()
            }],
            ..Default::default()
        };
        set_plan(&mut ed, id, &p);
    }
    (ed, id)
}

fn add_lot(ed: &mut Editor, style: ResourceId, at: DVec2, w: f64, h: f64) -> ObjectId {
    ObjectId(
        ed.execute(&Command::AddHardscape {
            boundary: Some(corners(&[at, at + v(w, 0.0), at + v(w, h), at + v(0.0, h)])),
            patch: Box::new(HardscapePatch {
                style: Some(style),
                origin: Some(at),
                angle: Some(0.0),
                ..Default::default()
            }),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap()
        .created[0],
    )
}

fn set_plan(ed: &mut Editor, id: ObjectId, p: &Plan) {
    ed.execute(&Command::SetData {
        id,
        set: [(plan::KEY.to_string(), serde_json::to_value(p).unwrap())]
            .into_iter()
            .collect(),
        remove: vec![],
    })
    .unwrap();
}

const PEN: Color32 = Color32::BLACK;

/// The style of the test lots has no color of its own: the pattern is the pen at half strength.
fn pattern_color() -> Color32 {
    PEN.gamma_multiply(0.5)
}

fn screen() -> egui::Rect {
    egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0))
}

/// What one frame painted.
#[derive(Default, Debug)]
struct Painted {
    /// Line segments in the color of the pattern.
    joints: usize,
    /// Cut units: polygons with the light fill.
    cut_fills: usize,
    /// Line segments of the dashed zone edges.
    zone_dashes: usize,
    /// Closed outlines in the color of the pattern (the outlines of hidden courses).
    outlines: usize,
    /// Rings of the findings.
    check_marks: usize,
    texts: Vec<String>,
}

impl Painted {
    fn says(&self, key: &str) -> bool {
        let t = kadakas_l10n::translate(key, &[]);
        self.texts.iter().any(|x| x == &t)
    }

    fn waiting(&self) -> bool {
        [
            "plan-draw-laying",
            "plan-draw-updating",
            "plan-draw-checking",
        ]
        .iter()
        .any(|k| self.says(k))
    }
}

fn flatten(shape: &egui::Shape, out: &mut Painted) {
    let fill = pattern_color().gamma_multiply(0.22);
    match shape {
        egui::Shape::Vec(v) => v.iter().for_each(|s| flatten(s, out)),
        egui::Shape::LineSegment { stroke, .. } => {
            if stroke.color == pattern_color() {
                out.joints += 1;
            } else if stroke.color == PEN.gamma_multiply(0.8) {
                out.zone_dashes += 1;
            }
        }
        egui::Shape::Path(p) => {
            if p.fill == fill {
                out.cut_fills += 1;
            } else if p.closed
                && p.fill == Color32::TRANSPARENT
                && matches!(p.stroke.color, egui::epaint::ColorMode::Solid(c) if c == pattern_color())
            {
                out.outlines += 1;
            }
        }
        // A concave piece, or one with a hole, is filled as triangles.
        egui::Shape::Mesh(m) => {
            if m.vertices.first().is_some_and(|v| v.color == fill) {
                out.cut_fills += 1;
            }
        }
        egui::Shape::Circle(c) => {
            if c.fill == Color32::TRANSPARENT && c.radius == 6.0 {
                out.check_marks += 1;
            }
        }
        egui::Shape::Text(t) => out.texts.push(t.galley.text().to_string()),
        _ => {}
    }
}

/// One frame of the objects `ids` at `view`. The revision names the document state.
fn frame_of(ed: &Editor, ids: &[ObjectId], view: &View, revision: u64) -> Painted {
    frame_with(ed.doc(), ids, view, revision, screen())
}

fn frame_with(
    doc: &Document,
    ids: &[ObjectId],
    view: &View,
    revision: u64,
    rect: egui::Rect,
) -> Painted {
    isolate();
    let ctx = egui::Context::default();
    let st = Style {
        pen: PEN,
        fill: Color32::TRANSPARENT,
        width: 1.0,
    };
    let out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        },
        |ui| {
            let painter = ui.painter_at(rect);
            crate::area_ui::set_revision(Some(revision));
            for id in ids {
                let o = doc.object(*id).unwrap();
                crate::area_ui::draw_extras(&painter, view, doc, o, st, DVec2::ZERO);
            }
        },
    );
    let mut painted = Painted::default();
    for s in &out.shapes {
        flatten(&s.shape, &mut painted);
    }
    painted
}

fn frame(ed: &Editor, id: ObjectId, view: &View, revision: u64) -> Painted {
    frame_of(ed, &[id], view, revision)
}

/// Frames until the background work is done and the frame has nothing to wait for.
fn settled(ed: &Editor, ids: &[ObjectId], view: &View, revision: u64) -> Painted {
    for _ in 0..4000 {
        wait_for_worker();
        let p = frame_of(ed, ids, view, revision);
        if !p.waiting() {
            return p;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!(
        "the canvas is still waiting: {:?}",
        frame_of(ed, ids, view, revision).texts
    );
}

fn settle(ed: &Editor, id: ObjectId, view: &View, revision: u64) -> Painted {
    settled(ed, &[id], view, revision)
}

/// A view that shows the model box `lo..hi` in the screen.
fn view_of(lo: DVec2, hi: DVec2) -> View {
    let mut v = View::default();
    v.fit(lo, hi, screen());
    v
}

fn report_counts(doc: &Document, id: ObjectId) -> (usize, usize) {
    let planned = plan::groups_of(doc, doc.object(id).unwrap(), 1.0, MAX_UNITS)
        .unwrap()
        .unwrap();
    planned.groups.iter().fold((0, 0), |(w, c), g| {
        let s = g.summarize(&SummaryOptions::default()).unwrap();
        (w + s.whole_units, c + s.cut_units)
    })
}

#[test]
fn the_canvas_draws_the_cut_units_of_the_report_and_the_lines_of_the_plan() {
    let (ed, id) = lot(4000.0, 2000.0, true);
    let view = view_of(v(-200.0, -200.0), v(4200.0, 2200.0));
    let doc = ed.doc();
    let (_, report_cut) = report_counts(doc, id);
    let prepared = draw::prepared(doc, doc.object(id).unwrap())
        .unwrap()
        .unwrap();
    let drawn = prepared.draw(None, &DrawOptions::canvas());
    assert!(!drawn.is_preview());
    // The zones and the course are in it.
    assert!(drawn.groups.len() >= 3, "{}", drawn.groups.len());
    let segments = drawn.segment_count();
    assert!(segments > 100 && report_cut > 5, "{segments} {report_cut}");
    let painted = settle(&ed, id, &view, 1);
    // The units that the canvas fills are the cut units that the report counts.
    assert_eq!(painted.cut_fills, report_cut);
    assert_eq!(painted.joints, segments);
    assert!(painted.zone_dashes > 0, "the zone edges are dashed");
    assert!(painted.texts.is_empty(), "{:?}", painted.texts);
}

#[test]
fn a_view_with_more_units_than_the_bound_paints_a_preview_that_turns_real_when_it_fits() {
    // 60 m x 30 m: 90,000 units. At 0.032 px/mm a unit of 100 mm is 3.2 px.
    let (ed, id) = lot(60_000.0, 30_000.0, false);
    let view = View {
        zoom: 0.032,
        origin: DVec2::new(-200.0, 700.0),
    };
    let painted = settle(&ed, id, &view, 1);
    assert!(painted.says("plan-draw-preview"), "{:?}", painted.texts);
    assert!(painted.joints > 100, "the preview draws joint lines");
    assert_eq!(painted.cut_fills, 0);
    // Zoomed in a little, with the same corner: the window fits the bound now, and the held
    // preview is looked at again.
    let before = redraws();
    let closer = View { zoom: 0.06, ..view };
    let painted = settle(&ed, id, &closer, 1);
    assert!(!painted.says("plan-draw-preview"), "{:?}", painted.texts);
    assert!(painted.joints > 100);
    assert!(redraws() > before);
    // Zoomed in on 2 m x 1 m the same lot is drawn with its real units and no label.
    let near = view_of(v(30_000.0, 15_000.0), v(32_000.0, 16_000.0));
    let painted = settle(&ed, id, &near, 1);
    assert!(painted.texts.is_empty(), "{:?}", painted.texts);
    assert!(painted.joints > 100);
}

#[test]
fn units_under_three_pixels_are_hidden_with_a_label_but_edges_and_courses_stay() {
    let (ed, id) = lot(8000.0, 4000.0, true);
    // 0.02 px/mm: a unit of 100 mm is 2 px. The course has units of 100 x 200 mm.
    let view = View {
        zoom: 0.02,
        origin: DVec2::new(100.0, 500.0),
    };
    let painted = settle(&ed, id, &view, 1);
    assert!(painted.says("plan-draw-hidden"), "{:?}", painted.texts);
    assert_eq!(painted.joints, 0);
    assert_eq!(painted.cut_fills, 0);
    assert!(painted.zone_dashes > 0, "the zone edges stay");
    assert!(painted.outlines > 0, "the course is drawn as a band");
}

#[test]
fn only_the_groups_with_small_units_are_hidden() {
    // The field has units of 200 x 100 mm; a zone has its own, of 3000 x 1500 mm.
    let (mut ed, id) = lot(8000.0, 4000.0, false);
    let p = Plan {
        splits: vec![SplitLine {
            source: None,
            points: vec![vx(4000.0, -50.0), vx(4000.0, 4050.0)],
        }],
        zones: vec![ZoneSpec {
            name: Some("Big".into()),
            seed: v(6000.0, 2000.0),
            pattern: Some(Pattern::new(Bond::Stack, 3000.0, 1500.0, 20.0)),
            origin: Some(v(4000.0, 0.0)),
            ..Default::default()
        }],
        ..Default::default()
    };
    set_plan(&mut ed, id, &p);
    // 0.04 px/mm: 100 mm is 4 px, so lower it to 0.02: 2 px for the field, 30 px for the zone.
    let view = View {
        zoom: 0.02,
        origin: DVec2::new(100.0, 500.0),
    };
    let painted = settle(&ed, id, &view, 1);
    assert!(painted.says("plan-draw-hidden"));
    let doc = ed.doc();
    let prepared = draw::prepared(doc, doc.object(id).unwrap())
        .unwrap()
        .unwrap();
    let big = prepared.draw(None, &DrawOptions::canvas()).groups;
    let big_lines: usize = big
        .iter()
        .filter(|g| g.name == "Big")
        .map(|g| g.joints.len())
        .sum();
    assert!(big_lines > 0);
    assert_eq!(
        painted.joints, big_lines,
        "only the zone with large units has lines"
    );
}

#[test]
fn the_canvas_keeps_the_laid_plan_and_the_window_until_the_view_or_the_object_changes() {
    let (mut ed, id) = lot(8000.0, 4000.0, true);
    let view = view_of(v(1000.0, 500.0), v(3000.0, 1500.0));
    settle(&ed, id, &view, 1);
    // The plan that the canvas of this thread keeps. (Not the shared cache: tests that run beside
    // this one empty it or push plans out of it, and then it lays the same plan again.)
    let first = held_prepared(id, false).unwrap();
    let before = redraws();
    frame(&ed, id, &view, 1);
    frame(&ed, id, &view, 1);
    assert_eq!(redraws(), before, "the same view is not cut again");
    // A small pan stays in the window.
    let mut panned = view;
    panned.pan(egui::vec2(20.0, -10.0));
    frame(&ed, id, &panned, 1);
    assert_eq!(redraws(), before);
    // Another object is added: a new revision of the document, the same plan.
    ed.execute(&Command::AddObject {
        shape: kadakas_core::model::Shape::Circle {
            center: v(0.0, 0.0),
            radius: 10.0,
        },
        layer: None,
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    frame(&ed, id, &view, 2);
    assert_eq!(redraws(), before, "an edit of something else costs nothing");
    let again = held_prepared(id, false).unwrap();
    assert!(Arc::ptr_eq(&first, &again));
    // A far pan leaves the window.
    let mut far = view;
    far.pan(egui::vec2(-900.0, 0.0));
    frame(&ed, id, &far, 2);
    assert_eq!(redraws(), before + 1);
    // The plan changes: the canvas says that it updates, draws the old plan meanwhile, and lays the
    // new one in the background.
    let mut p = plan::data(ed.doc().object(id).unwrap()).unwrap();
    p.courses.clear();
    set_plan(&mut ed, id, &p);
    let updating = frame(&ed, id, &far, 3);
    assert!(updating.says("plan-draw-updating") || updating.says("plan-draw-laying"));
    settle(&ed, id, &far, 3);
    assert_eq!(redraws(), before + 2);
}

#[test]
fn twenty_patterned_lots_stay_smooth() {
    // More lots than the shared cache once held: the canvas keeps each plan itself.
    let mut ed = Editor::new(Document::with_sequential_ids());
    let style = ResourceId(
        ed.execute(&Command::AddResource {
            name: "Pavers".into(),
            body: style_body(200.0, 100.0),
        })
        .unwrap()
        .created[0],
    );
    let ids: Vec<ObjectId> = (0..20)
        .map(|i| {
            add_lot(
                &mut ed,
                style,
                v((i % 5) as f64 * 3000.0, (i / 5) as f64 * 2500.0),
                2500.0,
                2000.0,
            )
        })
        .collect();
    let view = view_of(v(-100.0, -100.0), v(15_000.0, 10_000.0));
    let first = settled(&ed, &ids, &view, 1);
    assert!(first.joints > 20 * 20, "{}", first.joints);
    assert_eq!(held(), 20);
    // The shared cache is emptied (as if 13 other plans had pushed these out): the frames still
    // lay nothing and draw the same.
    draw::cache().clear();
    let redrawn = redraws();
    let jobs = pushed();
    let t = std::time::Instant::now();
    for _ in 0..10 {
        let p = frame_of(&ed, &ids, &view, 1);
        assert!(!p.waiting(), "{:?}", p.texts);
        assert_eq!(p.joints, first.joints);
    }
    assert_eq!(redraws(), redrawn);
    // The worker is shared by the tests that run beside this one, so count the jobs that this
    // thread handed over (not `busy()`, which also sees the jobs of the other tests).
    assert_eq!(pushed(), jobs, "no job was started");
    // Smooth means no work (no redraw, no job, the same joints: all checked above, whatever the
    // load of the machine). The time is only a bound against a hang.
    assert!(t.elapsed().as_secs_f64() < 120.0, "{:?}", t.elapsed());
    // A lot that is deleted is forgotten with the next revision.
    ed.execute(&Command::DeleteObjects { ids: vec![ids[0]] })
        .unwrap();
    frame_of(&ed, &ids[1..], &view, 2);
    assert_eq!(held(), 19);
}

#[test]
fn a_compare_ghost_has_its_own_plan_and_leaves_the_live_one_alone() {
    use kadakas_core::snapping::Smart;
    use kadakas_review::overlay::CompareOverlay;
    // The live lot has units of 200 x 100 mm; the old one, with the same ID, of 400 x 300 mm.
    let (ed, id) = lot(4000.0, 2000.0, false);
    let mut old = Editor::new(Document::with_sequential_ids());
    let style = ResourceId(
        old.execute(&Command::AddResource {
            name: "Pavers".into(),
            body: style_body(400.0, 300.0),
        })
        .unwrap()
        .created[0],
    );
    let old_id = add_lot(&mut old, style, v(0.0, 0.0), 4000.0, 2000.0);
    assert_eq!(old_id, id);
    let ghost_object = old.doc().object(old_id).unwrap().clone();
    let view = view_of(v(-200.0, -200.0), v(4200.0, 2200.0));
    let overlay = CompareOverlay::from_changes([]);
    let style_c = kadakas_review::overlay::CompareStyle::default();
    let paint_frame = |with_ghost: bool| {
        let ctx = egui::Context::default();
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen()),
                ..Default::default()
            },
            |ui| {
                let ghosts = [(ghost_object.clone(), "previous")];
                let frame = crate::canvas::Frame {
                    doc: ed.doc(),
                    selection: &[],
                    preview: &kadakas_ui_core::tools::Preview::None,
                    snap: None,
                    cues: false,
                    dark: false,
                    smart: &Smart::default(),
                    show_grid: false,
                    site: Some((&kadakas_site::SiteCache::default(), 7)),
                    sheet: None,
                    plugin_preview: &[],
                    compare: with_ghost.then_some(crate::compare_draw::CompareDraw {
                        overlay: &overlay,
                        style: &style_c,
                        ghosts: &ghosts,
                        ghost_doc: Some(old.doc()),
                        report: Default::default(),
                        focus: None,
                    }),
                };
                crate::canvas::paint(&ui.painter_at(screen()), screen(), &view, &frame);
            },
        )
    };
    let live_key = {
        let doc = ed.doc();
        draw::content_key(
            doc,
            doc.object(id).unwrap(),
            kadakas_core::layout::TOLERANCE,
        )
    };
    let ghost_key = draw::content_key(old.doc(), &ghost_object, kadakas_core::layout::TOLERANCE);
    assert_ne!(live_key, ghost_key);
    // Live only, then with the ghost (drawn first, with the same ID), then live only again.
    for with_ghost in [false, true, true, false] {
        wait_for_worker();
        paint_frame(with_ghost);
    }
    wait_for_worker();
    paint_frame(true);
    // The canvas keeps the live plan under the live key, and the ghost's under its own.
    assert_eq!(held_key(id, false), Some(live_key));
    assert_eq!(held_key(id, true), Some(ghost_key));
    // And the live object is still drawn with its own units: 200 x 100 mm give more lines than
    // 400 x 300 mm in the same box.
    let live_lines = {
        wait_for_worker();
        settle(&ed, id, &view, 7).joints
    };
    let doc = ed.doc();
    let prepared = draw::prepared(doc, doc.object(id).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        live_lines,
        prepared.draw(None, &DrawOptions::canvas()).segment_count()
    );
}

#[test]
fn the_findings_of_the_check_are_made_in_the_background_and_only_the_current_ones_are_shown() {
    // A strip of 3 mm is left at the right edge: small pieces.
    let (mut ed, id) = lot(8003.0, 400.0, false);
    let view = view_of(v(-100.0, -100.0), v(8100.0, 500.0));
    set_show_check(false);
    assert_eq!(settle(&ed, id, &view, 1).check_marks, 0);
    set_show_check(true);
    // The first frame does not wait for the check.
    let first = frame(&ed, id, &view, 1);
    assert!(first.says("plan-draw-checking"), "{:?}", first.texts);
    assert_eq!(first.check_marks, 0);
    let on = settle(&ed, id, &view, 1);
    let prepared = held_prepared(id, false).unwrap();
    let report = prepared.findings_ready().unwrap().as_ref().unwrap();
    let n = report.findings.len();
    assert!(n > 0);
    assert_eq!(on.check_marks, n);
    assert!(
        on.texts.iter().any(|t| t.contains("default options")),
        "{:?}",
        on.texts
    );
    // The plan changes: no ring of the old findings is shown while the new plan is laid.
    ed.execute(&Command::SetHardscape {
        ids: vec![id],
        patch: HardscapePatch {
            angle: Some(10.0),
            ..Default::default()
        },
    })
    .unwrap();
    let after = frame(&ed, id, &view, 2);
    assert_eq!(after.check_marks, 0, "{:?}", after.texts);
    let again = settle(&ed, id, &view, 2);
    let prepared = held_prepared(id, false).unwrap();
    assert_eq!(
        again.check_marks,
        prepared
            .findings_ready()
            .unwrap()
            .as_ref()
            .unwrap()
            .findings
            .len()
    );
    set_show_check(false);
}

#[test]
fn a_pattern_that_cannot_be_laid_says_why_on_the_canvas() {
    let (mut ed, id) = lot(200.0, 200.0, false);
    // A kerb border wider than the area.
    let style = kadakas_core::hardscape::data::style_of(ed.doc(), ed.doc().object(id).unwrap())
        .unwrap()
        .0;
    ed.execute(&Command::ModifyResource {
        id: style,
        name: None,
        body: Some(ResourceBody::HardscapeStyle(Box::new(HardscapeStyle {
            joints: Some(JointPattern::default()),
            border: Some(kadakas_core::hardscape::Border {
                width: 150.0,
                ..Default::default()
            }),
            ..Default::default()
        }))),
    })
    .unwrap();
    let view = view_of(v(-100.0, -100.0), v(300.0, 300.0));
    let painted = settle(&ed, id, &view, 1);
    assert!(
        painted
            .texts
            .iter()
            .any(|t| t.contains("Pattern not drawn")),
        "{:?}",
        painted.texts
    );
}

#[test]
fn the_sheet_view_draws_the_units_and_the_fills_that_the_pdf_has() {
    use kadakas_core::model::{FIRST_LAYER, LayerId, Shape};
    use kadakas_core::sheets::{PaperSize, Viewport};
    use kadakas_print::display::{PSeg, Prim};
    let (mut ed, id) = lot(6000.0, 3000.0, true);
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
        v(3000.0, 1500.0),
        20.0,
        vec![FIRST_LAYER],
    );
    ed.execute(&Command::AddObject {
        shape: Shape::Viewport(Box::new(vp)),
        layer: Some(sheet),
        class: None,
        attrs: Attributes::default(),
    })
    .unwrap();
    let page = kadakas_print::compose_page(ed.doc(), sheet).unwrap();
    // What the PDF has: units are subpaths of five points (four and the line back); cut units are
    // the paths with the light fill.
    let (mut units, mut cut_prims) = (0usize, 0usize);
    for p in &page.prims {
        let Prim::Path(p) = p else { continue };
        let subs = kadakas_print::display::sample_subpaths(&p.segs, 12);
        if p.fill.is_some_and(|f| f[3] == 46) {
            cut_prims += 1;
        }
        for s in &subs {
            let ring = s.len() == 5 && s.first() == s.last();
            let outline = p.closed && s.len() == 4 && subs.len() == 1;
            // A dashed ring (a zone edge) is drawn as dashes, not as a ring.
            units += usize::from((ring || outline) && p.stroke.is_some() && p.dash.is_none());
        }
    }
    assert!(units > 500 && cut_prims > 5, "{units} {cut_prims}");
    // What the app draws of the page.
    let fill = [0, 0, 0, 46];
    let _ = fill;
    let ctx = egui::Context::default();
    let sheets = crate::sheets_ui::SheetUi::default();
    let view = View {
        zoom: 1.0,
        origin: DVec2::new(0.0, 700.0),
    };
    let out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            ..Default::default()
        },
        |ui| {
            sheets.paint(&ui.painter_at(ui.max_rect()), &view, &page);
        },
    );
    fn walk(s: &egui::Shape, f: &mut dyn FnMut(&egui::Shape)) {
        match s {
            egui::Shape::Vec(v) => v.iter().for_each(|x| walk(x, f)),
            other => f(other),
        }
    }
    let (mut rings, mut meshes, mut longest) = (0usize, 0usize, 0usize);
    for cs in &out.shapes {
        walk(&cs.shape, &mut |s| match s {
            egui::Shape::Path(p) if p.closed && p.points.len() == 4 => rings += 1,
            egui::Shape::Path(p) => longest = longest.max(p.points.len()),
            egui::Shape::Mesh(m) if m.vertices.first().is_some_and(|v| v.color.a() == 46) => {
                meshes += 1
            }
            _ => {}
        });
    }
    // Every unit is a ring of its own, and the cut units are filled like the PDF fills them.
    assert!(rings >= units, "{rings} rings for {units} units");
    assert_eq!(meshes, cut_prims);
    // No path runs through the whole pattern: a path with many subpaths is split at each Move.
    assert!(longest < 400, "a path of {longest} points");
    let _ = id;
    let _ = PSeg::Move(DVec2::ZERO);
}

#[test]
fn a_new_plan_goes_ahead_of_the_checks_and_drops_the_checks_of_its_place() {
    use crate::plan_worker::{Worker, slot};
    let (ed, id) = lot(4000.0, 2000.0, true);
    let doc = ed.doc();
    let o = doc.object(id).unwrap();
    let key = draw::content_key(doc, o, kadakas_core::layout::TOLERANCE);
    let prepared =
        Arc::new(Prepared::new(doc, o, kadakas_core::layout::TOLERANCE, key, MAX_UNITS).unwrap());
    let ctx = egui::Context::default();
    let w = Worker::detached();
    let (a, b) = (slot(id, false), slot(id, true));
    let window = prepared.bounds();
    w.check(a, prepared.clone(), &ctx);
    w.draw(a, 1, prepared.clone(), window, DrawOptions::canvas(), &ctx);
    w.check(b, prepared.clone(), &ctx);
    assert_eq!(w.queued(None), ["window", "check", "check"]);
    // A newer plan for place a: it goes first, and the window and the check of the old plan of a
    // are dropped. The check of place b stays.
    w.prepare(a, draw::prepare_job(doc, o, key).unwrap(), &ctx);
    assert_eq!(w.queued(None), ["plan", "check"]);
    assert_eq!(w.queued(Some(a)), ["plan"]);
    // A second plan for b goes after the first, before any check.
    w.check(a, prepared.clone(), &ctx);
    w.prepare(b, draw::prepare_job(doc, o, key).unwrap(), &ctx);
    assert_eq!(w.queued(None), ["plan", "plan", "check"]);
    assert_eq!(w.queued(Some(b)), ["plan"]);
    // The newest job of a place and a kind replaces the one that waits.
    w.prepare(a, draw::prepare_job(doc, o, key).unwrap(), &ctx);
    assert_eq!(w.queued(None).iter().filter(|k| **k == "plan").count(), 2);
}

#[test]
fn a_plan_whose_job_went_is_asked_for_again() {
    // The plan cache of this test is its own (`isolate`): other tests lay the same lot, and neither
    // find this plan nor empty the cache under it. The results of the worker are kept by place and
    // thread.
    let (ed, id) = lot(4000.0, 2000.0, true);
    let view = view_of(v(-200.0, -200.0), v(4200.0, 2200.0));
    let doc = ed.doc();
    let key = draw::content_key(
        doc,
        doc.object(id).unwrap(),
        kadakas_core::layout::TOLERANCE,
    );
    let jobs = pushed();
    let first = frame(&ed, id, &view, 1);
    assert!(first.waiting());
    assert_eq!(pushed(), jobs + 1, "the first frame asked for the plan");
    // The plan arrives for the place of this thread.
    let slot = crate::plan_worker::slot(id, false);
    let t = std::time::Instant::now();
    let lost = loop {
        if let Some(l) = worker().take(slot, key) {
            break l;
        }
        assert!(t.elapsed().as_secs() < 60, "the plan never arrived");
        std::thread::sleep(std::time::Duration::from_millis(2));
    };
    assert_eq!(lost.key, key);
    assert!(lost.plan.is_ok());
    // The result is lost (taken and thrown away), and the shared cache is emptied.
    draw::cache().clear();
    assert!(draw::cached(key).is_none());
    assert!(!worker().pending_plan(slot, key), "nothing is on its way");
    let jobs = pushed();
    let settled = settle(&ed, id, &view, 1);
    assert!(pushed() > jobs, "the window asked for the plan again");
    assert!(settled.joints > 100, "{settled:?}");
    assert!(held_prepared(id, false).is_some(), "its own plan arrived");
}

#[test]
fn a_change_of_the_unit_limit_is_a_change_of_the_plan() {
    let (ed, id) = lot(4000.0, 2000.0, true);
    let view = view_of(v(-200.0, -200.0), v(4200.0, 2200.0));
    let before = settle(&ed, id, &view, 1);
    assert!(before.joints > 100);
    // Under the same revision of the document, a course of more units than the limit of the
    // session cannot be laid: the plan is another plan.
    draw::set_pattern_units(5);
    let limited = settle(&ed, id, &view, 1);
    draw::set_pattern_units(MAX_UNITS);
    assert!(
        limited
            .texts
            .iter()
            .any(|t| t.contains("Pattern not drawn")),
        "{:?}",
        limited.texts
    );
    let after = settle(&ed, id, &view, 1);
    assert_eq!(after.joints, before.joints);
}

#[test]
fn a_window_that_is_cut_in_the_background_leaves_the_held_one_on_the_screen() {
    let (ed, id) = lot(8000.0, 4000.0, true);
    let view = view_of(v(1000.0, 500.0), v(3000.0, 1500.0));
    let held = settle(&ed, id, &view, 1);
    assert!(held.joints > 100);
    // The view leaves the window: the frame does not wait for the cut. It draws what it has and
    // says that it updates.
    let mut far = view;
    far.pan(egui::vec2(-900.0, 0.0));
    let at_once = frame(&ed, id, &far, 1);
    assert!(at_once.says("plan-draw-updating"), "{:?}", at_once.texts);
    let done = settle(&ed, id, &far, 1);
    assert!(!done.says("plan-draw-updating"));
    assert!(done.joints > 0);
}

/// Measures the frame of the canvas for 10,000, 30,000 and 100,000 line segments of a pattern
/// (`cargo test -p kadakas-app --bin kadakas-app measure_canvas_frames -- --ignored --nocapture`,
/// with the egui crates optimized, as the release build has them). A frame is the drawing of the
/// plan from the kept window and the tessellation of the shapes into triangles.
#[test]
#[ignore = "a measurement"]
fn measure_canvas_frames() {
    set_bound(Some(usize::MAX / 8));
    let big = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(2400.0, 1400.0));
    for (revision, units) in [(1u64, 10_000usize), (2, 30_000), (3, 100_000)] {
        // A lot of 2:1 with `units` cells of 205 x 105 mm.
        let area = units as f64 * 205.0 * 105.0;
        let (w, h) = ((area * 2.0).sqrt(), (area / 2.0).sqrt());
        let (ed, id) = lot(w, h, false);
        let mut view = View::default();
        view.fit(v(0.0, 0.0), v(w, h), big);
        let doc = ed.doc();
        let o = doc.object(id).unwrap();
        let st = Style {
            pen: PEN,
            fill: Color32::TRANSPARENT,
            width: 1.0,
        };
        let ctx = egui::Context::default();
        let run = |view: &View| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(big),
                    ..Default::default()
                },
                |ui| {
                    let painter = ui.painter_at(big);
                    crate::area_ui::set_revision(Some(revision));
                    crate::area_ui::draw_extras(&painter, view, doc, o, st, DVec2::ZERO);
                },
            )
        };
        // The plan is laid in the background; the first frame with it lays the window.
        let mut cold = std::time::Duration::ZERO;
        let mut out = run(&view);
        for _ in 0..2000 {
            wait_for_worker();
            let t = std::time::Instant::now();
            out = run(&view);
            cold = t.elapsed();
            let mut p = Painted::default();
            for s in &out.shapes {
                flatten(&s.shape, &mut p);
            }
            if !p.waiting() && p.joints > 0 {
                break;
            }
        }
        let mut painted = Painted::default();
        for s in &out.shapes {
            flatten(&s.shape, &mut painted);
        }
        let mut frames = Vec::new();
        let mut tess = Vec::new();
        for _ in 0..10 {
            let t = std::time::Instant::now();
            let out = run(&view);
            frames.push(t.elapsed());
            let t = std::time::Instant::now();
            let meshes = ctx.tessellate(out.shapes, out.pixels_per_point);
            tess.push(t.elapsed());
            std::hint::black_box(meshes);
        }
        frames.sort();
        tess.sort();
        println!(
            "units {units}: segments {} cut fills {}; first frame with the plan (lay the window) {:.1} ms; kept frame: paint {:.2} ms, tessellate {:.2} ms",
            painted.joints,
            painted.cut_fills,
            cold.as_secs_f64() * 1e3,
            frames[5].as_secs_f64() * 1e3,
            tess[5].as_secs_f64() * 1e3,
        );
    }
    set_bound(None);
}
