//! Tests of the pattern plan on the canvas, in a headless egui frame: what is painted is what the
//! drawing source says, the preview says so, the laid plan is kept, and the findings are marked on
//! request.

use super::*;
use crate::canvas::Style;
use kadakas_core::command::{Command, Editor};
use kadakas_core::curve::{Vertex, corners};
use kadakas_core::hardscape::HardscapePatch;
use kadakas_core::hardscape::style::{HardscapeStyle, JointKind, JointPattern};
use kadakas_core::layout::course::Side;
use kadakas_core::layout::plan::{self, CourseDef, Plan, SplitLine};
use kadakas_core::model::Attributes;
use kadakas_core::resource::{ResourceBody, ResourceId};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn vx(x: f64, y: f64) -> Vertex {
    Vertex::corner(v(x, y))
}

/// A lot of `w` x `h` mm with running-bond units of 200 x 100 mm and a 5 mm joint.
fn lot(w: f64, h: f64, planned: bool) -> (Editor, ObjectId) {
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
                gap: 5.0,
                ..Default::default()
            }],
            ..Default::default()
        };
        set_plan(&mut ed, id, &p);
    }
    (ed, id)
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

/// The style of the test lot has no color of its own: the pattern is the pen at half strength.
fn pattern_color() -> Color32 {
    PEN.gamma_multiply(0.5)
}

fn screen() -> Rect {
    Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0))
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
    /// Rings of the findings.
    check_marks: usize,
    texts: Vec<String>,
}

fn flatten(shape: &egui::Shape, out: &mut Painted) {
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
            if p.fill == pattern_color().gamma_multiply(0.22) {
                out.cut_fills += 1;
            }
        }
        // A concave piece is filled as triangles.
        egui::Shape::Mesh(m) => {
            if m.vertices
                .first()
                .is_some_and(|v| v.color == pattern_color().gamma_multiply(0.22))
            {
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

/// One frame of the pattern of `id` at `view`. The revision names the document state.
fn frame(ed: &Editor, id: ObjectId, view: &View, revision: u64) -> Painted {
    let ctx = egui::Context::default();
    let doc = ed.doc();
    let o = doc.object(id).unwrap();
    let st = Style {
        pen: PEN,
        fill: Color32::TRANSPARENT,
        width: 1.0,
    };
    let out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen()),
            ..Default::default()
        },
        |ui| {
            let painter = ui.painter_at(screen());
            crate::area_ui::set_revision(Some(revision));
            crate::area_ui::draw_extras(&painter, view, doc, o, st, DVec2::ZERO);
        },
    );
    let mut painted = Painted::default();
    for s in &out.shapes {
        flatten(&s.shape, &mut painted);
    }
    painted
}

/// A view that shows the model box `lo..hi` in the screen.
fn view_of(lo: DVec2, hi: DVec2) -> View {
    let mut v = View::default();
    v.fit(lo, hi, screen());
    v
}

#[test]
fn a_frame_paints_the_joint_lines_cut_units_and_zone_edges_of_the_plan() {
    let (ed, id) = lot(4000.0, 2000.0, true);
    let view = view_of(v(-200.0, -200.0), v(4200.0, 2200.0));
    let doc = ed.doc();
    let prepared = draw::prepared(doc, doc.object(id).unwrap())
        .unwrap()
        .unwrap();
    let drawn = prepared.draw(None, &DrawOptions::canvas());
    assert!(!drawn.is_preview());
    // The zones and the course are in it.
    assert!(drawn.groups.len() >= 3, "{}", drawn.groups.len());
    let (_, cut) = drawn.unit_counts();
    let segments = drawn.segment_count();
    assert!(segments > 100 && cut > 5, "{segments} {cut}");
    let painted = frame(&ed, id, &view, 1);
    assert_eq!(painted.joints, segments);
    assert_eq!(painted.cut_fills, cut);
    assert!(painted.zone_dashes > 0, "the zone edges are dashed");
    assert!(painted.texts.is_empty(), "no label at full detail");
}

#[test]
fn a_view_with_more_units_than_the_bound_paints_a_preview_with_its_label() {
    // 60 m x 30 m: 90,000 units. At 0.05 px/mm a unit of 100 mm is 5 px.
    let (ed, id) = lot(60_000.0, 30_000.0, false);
    let view = View {
        zoom: 0.05,
        origin: DVec2::new(-200.0, 700.0),
    };
    let painted = frame(&ed, id, &view, 1);
    let label = kadakas_l10n::translate("plan-draw-preview", &[]);
    assert!(
        painted.texts.iter().any(|t| t == &label),
        "{:?}",
        painted.texts
    );
    assert!(painted.joints > 100, "the preview draws joint lines");
    assert_eq!(painted.cut_fills, 0);
    // Zoomed in on 2 m x 1 m the same lot is drawn with its real units and no label.
    let near = view_of(v(30_000.0, 15_000.0), v(32_000.0, 16_000.0));
    let painted = frame(&ed, id, &near, 1);
    assert!(painted.texts.is_empty(), "{:?}", painted.texts);
    assert!(painted.joints > 100);
}

#[test]
fn the_canvas_keeps_the_laid_plan_and_the_window_until_the_view_or_the_object_changes() {
    let (mut ed, id) = lot(8000.0, 4000.0, true);
    let o = ed.doc().object(id).unwrap();
    let first = draw::prepared(ed.doc(), o).unwrap().unwrap();
    let view = view_of(v(1000.0, 500.0), v(3000.0, 1500.0));
    let before = redraws();
    frame(&ed, id, &view, 1);
    frame(&ed, id, &view, 1);
    assert_eq!(redraws() - before, 1, "the same view is not cut again");
    // A small pan stays in the window.
    let mut panned = view;
    panned.pan(egui::vec2(20.0, -10.0));
    frame(&ed, id, &panned, 1);
    assert_eq!(redraws() - before, 1);
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
    assert_eq!(
        redraws() - before,
        1,
        "an edit of something else costs nothing"
    );
    let again = draw::prepared(ed.doc(), ed.doc().object(id).unwrap())
        .unwrap()
        .unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &again));
    // A far pan leaves the window.
    let mut far = view;
    far.pan(egui::vec2(-900.0, 0.0));
    frame(&ed, id, &far, 2);
    assert_eq!(redraws() - before, 2);
    // The plan changes: it is laid and cut again.
    let mut p = plan::data(ed.doc().object(id).unwrap()).unwrap();
    p.courses.clear();
    set_plan(&mut ed, id, &p);
    frame(&ed, id, &far, 3);
    assert_eq!(redraws() - before, 3);
}

#[test]
fn the_findings_of_the_check_are_marked_only_when_asked() {
    // A strip of 3 mm is left at the right edge: small pieces.
    let (ed, id) = lot(8003.0, 400.0, false);
    let view = view_of(v(-100.0, -100.0), v(8100.0, 500.0));
    set_show_check(false);
    assert_eq!(frame(&ed, id, &view, 1).check_marks, 0);
    set_show_check(true);
    let on = frame(&ed, id, &view, 1);
    set_show_check(false);
    let doc = ed.doc();
    let prepared = draw::prepared(doc, doc.object(id).unwrap())
        .unwrap()
        .unwrap();
    let n = prepared.findings().as_ref().unwrap().findings.len();
    assert!(n > 0);
    assert_eq!(on.check_marks, n);
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
    let painted = frame(&ed, id, &view, 1);
    assert!(
        painted
            .texts
            .iter()
            .any(|t| t.contains("Pattern not drawn")),
        "{:?}",
        painted.texts
    );
}

/// Measures the frame of the canvas for 10,000, 30,000 and 100,000 line segments of a pattern
/// (`cargo test -p kadakas-app --bin kadakas-app measure_canvas_frames -- --ignored --nocapture`,
/// with the egui crates optimized, as the release build has them). A frame is the drawing of the
/// plan from the kept window and the tessellation of the shapes into triangles.
#[test]
#[ignore = "a measurement"]
fn measure_canvas_frames() {
    set_bound(Some(usize::MAX / 8));
    let big = Rect::from_min_size(Pos2::ZERO, egui::vec2(2400.0, 1400.0));
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
        let t = std::time::Instant::now();
        let out = run(&view);
        let cold = t.elapsed();
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
            "units {units}: segments {} cut fills {}; first frame (lay the window) {:.1} ms; kept frame: paint {:.2} ms, tessellate {:.2} ms",
            painted.joints,
            painted.cut_fills,
            cold.as_secs_f64() * 1e3,
            frames[5].as_secs_f64() * 1e3,
            tess[5].as_secs_f64() * 1e3,
        );
    }
    set_bound(None);
}
