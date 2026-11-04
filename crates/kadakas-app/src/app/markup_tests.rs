//! Markup on the plan (REV-11): headless egui frames with synthetic pointer events. The same ink
//! on the 3D view is tested in `canvas3d.rs`.

use egui::{Pos2, RawInput, Rect, vec2};
use kadakas_core::markup::MarkKind;
use kadakas_ui_core::markup::Tool;

use super::*;
use crate::markup_ui::Request;

const W: f32 = 1000.0;
const H: f32 = 700.0;

struct Rig {
    app: KadakasApp,
    ctx: egui::Context,
    time: f64,
}

impl Rig {
    fn new() -> Rig {
        let mut rig = Rig {
            app: KadakasApp::blank(),
            ctx: egui::Context::default(),
            time: 1.0,
        };
        rig.frame(vec![]);
        rig.frame(vec![]);
        rig
    }

    fn frame(&mut self, events: Vec<egui::Event>) {
        self.time += 0.05;
        let input = RawInput {
            time: Some(self.time),
            events,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(W, H))),
            ..Default::default()
        };
        let ctx = self.ctx.clone();
        let app = &mut self.app;
        let _ = ctx.run_ui(input, |ui| {
            egui::CentralPanel::no_frame().show(ui, |ui| app.show_dock(ui));
            app.markup_ui.windows(ui.ctx());
        });
        app.run_markup_requests();
    }

    fn rect(&self) -> Rect {
        self.app.canvas_rect.expect("the plan was drawn")
    }

    /// A point of the canvas, by its share of the width and the height.
    fn at(&self, u: f32, v: f32) -> Pos2 {
        let r = self.rect();
        r.min + vec2(r.width() * u, r.height() * v)
    }

    fn button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// A drag with the left button along the points, one frame for each step.
    fn drag(&mut self, points: &[Pos2]) {
        self.frame(vec![egui::Event::PointerMoved(points[0])]);
        self.frame(vec![Rig::button(points[0], true)]);
        for p in &points[1..] {
            self.frame(vec![egui::Event::PointerMoved(*p)]);
        }
        let last = *points.last().unwrap();
        self.frame(vec![Rig::button(last, false)]);
        self.frame(vec![]);
    }

    fn click(&mut self, p: Pos2) {
        self.drag(&[p]);
    }

    /// Starts a markup and switches the mode on.
    fn start(&mut self) -> kadakas_core::markup::MarkupId {
        self.app.markup_ui.push_request(Request::New);
        self.frame(vec![]);
        self.app.markup_ui.open.expect("a markup is open")
    }

    fn markup(&self, id: kadakas_core::markup::MarkupId) -> &kadakas_core::markup::Markup {
        self.app.session.editor.doc().markups.get(id).unwrap()
    }

    fn rect_object(&mut self) -> ObjectId {
        let out = self
            .app
            .session
            .editor
            .execute(&Command::AddObject {
                shape: kadakas_core::model::Shape::Rect {
                    min: DVec2::new(-1000.0, -1000.0),
                    max: DVec2::new(1000.0, 1000.0),
                },
                layer: None,
                class: None,
                attrs: Default::default(),
            })
            .unwrap();
        ObjectId(out.created[0])
    }
}

fn line(rig: &Rig, n: usize) -> Vec<Pos2> {
    (0..n)
        .map(|i| {
            rig.at(
                0.3 + 0.4 * i as f32 / n as f32,
                0.4 + 0.1 * (i as f32 * 0.7).sin(),
            )
        })
        .collect()
}

#[test]
fn a_stroke_in_the_plan_is_one_mark_and_one_undo_step_and_the_tools_wait() {
    let mut rig = Rig::new();
    let rect = rig.rect_object();
    rig.frame(vec![]);
    let before = rig.app.session.editor.doc().object(rect).unwrap().clone();
    let id = rig.start();
    assert!(rig.app.markup_ui.captures());
    let depth = rig.app.session.editor.undo_depth();
    let points = line(&rig, 30);
    rig.drag(&points);
    let m = rig.markup(id);
    assert_eq!(m.marks.len(), 1);
    assert_eq!(m.marks[0].kind, MarkKind::Pen);
    assert!(m.marks[0].points.len() > 5, "{}", m.marks[0].points.len());
    for p in &m.marks[0].points {
        assert!((0.0..=1.0).contains(&p[0]) && (0.0..=1.0).contains(&p[1]));
    }
    assert_eq!(rig.app.session.editor.undo_depth(), depth + 1, "one step");
    // The normal tools did not see the drag: nothing moved and nothing was selected.
    assert_eq!(rig.app.session.editor.doc().object(rect).unwrap(), &before);
    assert!(rig.app.session.selection.is_empty());
    // Undo and redo.
    rig.app.undo();
    assert!(rig.markup(id).marks.is_empty());
    rig.app.redo();
    assert_eq!(rig.markup(id).marks.len(), 1);
    // The marks are drawn while the view is the view of the markup.
    rig.frame(vec![]);
    assert!(
        rig.app
            .markup_ui
            .marks_visible(rig.app.session.editor.doc())
    );
}

#[test]
fn with_the_mode_off_the_normal_tools_work_as_before() {
    let mut rig = Rig::new();
    let rect = rig.rect_object();
    let id = rig.start();
    rig.app.markup_ui.set_mode(false);
    assert!(!rig.app.markup_ui.captures());
    // A click on the top edge of the rectangle selects it with the Select tool.
    let edge = rig.app.view.to_screen(DVec2::new(0.0, 1000.0));
    assert!(rig.rect().contains(edge));
    rig.click(edge);
    assert_eq!(rig.app.session.selection, vec![rect]);
    assert!(rig.markup(id).marks.is_empty(), "nothing was drawn");
}

#[test]
fn every_tool_draws_its_mark() {
    let mut rig = Rig::new();
    let id = rig.start();
    let drag = [rig.at(0.2, 0.2), rig.at(0.3, 0.25), rig.at(0.5, 0.4)];
    for (tool, kind, points) in [
        (Tool::Highlighter, MarkKind::Highlighter, None),
        (Tool::Arrow, MarkKind::Arrow, Some(2)),
        (Tool::Rectangle, MarkKind::Rectangle, Some(2)),
        (Tool::Ellipse, MarkKind::Ellipse, Some(2)),
    ] {
        rig.app.markup_ui.tool = tool;
        rig.app.markup_ui.brush.width = 7.0;
        rig.drag(&drag);
        let m = rig.markup(id).marks.last().unwrap().clone();
        assert_eq!(m.kind, kind, "{tool:?}");
        assert_eq!(m.width, 7.0);
        if let Some(n) = points {
            assert_eq!(m.points.len(), n, "{tool:?}");
        }
    }
    assert_eq!(rig.markup(id).marks.len(), 4);
    // A click with a shape tool makes nothing. A click with the pen makes a dot.
    rig.app.markup_ui.tool = Tool::Arrow;
    rig.click(rig.at(0.7, 0.7));
    assert_eq!(rig.markup(id).marks.len(), 4);
    rig.app.markup_ui.tool = Tool::Pen;
    rig.click(rig.at(0.7, 0.7));
    assert_eq!(rig.markup(id).marks.len(), 5);
    assert_eq!(rig.markup(id).marks[4].points.len(), 1);
}

#[test]
fn a_note_asks_for_its_text_and_is_one_step() {
    let mut rig = Rig::new();
    let id = rig.start();
    rig.app.markup_ui.tool = Tool::Note;
    rig.click(rig.at(0.4, 0.4));
    assert!(rig.app.markup_ui.note_pending());
    assert!(rig.markup(id).marks.is_empty(), "no mark before the text");
    let depth = rig.app.session.editor.undo_depth();
    rig.app.markup_ui.finish_note_for_test("a tree here");
    rig.frame(vec![]);
    assert_eq!(rig.markup(id).marks[0].text, "a tree here");
    assert_eq!(rig.app.session.editor.undo_depth(), depth + 1);
    // Escape drops a note that waits.
    rig.click(rig.at(0.6, 0.6));
    assert!(rig.app.markup_ui.note_pending());
    rig.frame(vec![egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    assert!(!rig.app.markup_ui.note_pending());
}

#[test]
fn the_eraser_the_select_tool_and_clear_change_marks_in_single_steps() {
    let mut rig = Rig::new();
    let id = rig.start();
    rig.drag(&[rig.at(0.2, 0.2), rig.at(0.4, 0.2)]);
    rig.drag(&[rig.at(0.2, 0.6), rig.at(0.4, 0.6)]);
    assert_eq!(rig.markup(id).marks.len(), 2);
    // The eraser removes the mark under the pointer.
    rig.app.markup_ui.tool = Tool::Eraser;
    let depth = rig.app.session.editor.undo_depth();
    rig.click(rig.at(0.3, 0.2));
    assert_eq!(rig.markup(id).marks.len(), 1);
    assert_eq!(rig.app.session.editor.undo_depth(), depth + 1);
    rig.click(rig.at(0.9, 0.9));
    assert_eq!(
        rig.markup(id).marks.len(),
        1,
        "a click on nothing removes nothing"
    );
    // The select tool picks a mark, a swatch recolours it, Delete removes it.
    rig.app.markup_ui.tool = Tool::Select;
    rig.click(rig.at(0.3, 0.6));
    let mark = rig.app.markup_ui.selected.expect("the mark is picked");
    assert!(rig.app.markup_ui.wants_delete());
    let blue = kadakas_core::markup::PALETTE[4];
    // The swatch click of the palette goes through the same request.
    rig.app
        .markup_ui
        .push_request(Request::cmd(Command::SetMarkStyle {
            markup: id,
            mark,
            color: Some(blue),
            width: None,
        }));
    rig.frame(vec![]);
    assert_eq!(rig.markup(id).marks[0].color, blue);
    rig.app.markup_ui.delete_selected();
    rig.frame(vec![]);
    assert!(rig.markup(id).marks.is_empty());
    // Clear: all marks in one step.
    rig.app.markup_ui.tool = Tool::Pen;
    rig.drag(&[rig.at(0.2, 0.2), rig.at(0.4, 0.2)]);
    rig.drag(&[rig.at(0.2, 0.6), rig.at(0.4, 0.6)]);
    let depth = rig.app.session.editor.undo_depth();
    rig.app
        .markup_ui
        .push_request(Request::cmd(Command::ClearMarkup { markup: id }));
    rig.frame(vec![]);
    assert!(rig.markup(id).marks.is_empty());
    assert_eq!(rig.app.session.editor.undo_depth(), depth + 1);
}

#[test]
fn marks_hide_when_the_view_moves_and_come_back_with_the_viewpoint() {
    let mut rig = Rig::new();
    let id = rig.start();
    rig.drag(&line(&rig, 10));
    rig.frame(vec![]);
    assert!(
        rig.app
            .markup_ui
            .marks_visible(rig.app.session.editor.doc())
    );
    // The view moves: the marks are hidden, and a drag does not draw.
    rig.app.view.pan(vec2(120.0, 40.0));
    rig.frame(vec![]);
    assert!(
        !rig.app
            .markup_ui
            .marks_visible(rig.app.session.editor.doc())
    );
    let depth = rig.app.session.editor.undo_depth();
    rig.drag(&line(&rig, 10));
    assert_eq!(
        rig.markup(id).marks.len(),
        1,
        "no drawing on a view that moved"
    );
    assert_eq!(rig.app.session.editor.undo_depth(), depth);
    // Go to the viewpoint: the view is back and the marks show.
    rig.app.markup_ui.push_request(Request::GoTo(id));
    rig.frame(vec![]);
    rig.frame(vec![]);
    assert!(
        rig.app
            .markup_ui
            .marks_visible(rig.app.session.editor.doc())
    );
    assert!(rig.app.status.contains("Markup"), "{}", rig.app.status);
}

#[test]
fn finish_keeps_a_markup_with_marks_and_removes_an_empty_one() {
    let mut rig = Rig::new();
    let empty = rig.start();
    rig.app.markup_ui.push_request(Request::Finish);
    rig.frame(vec![]);
    assert!(rig.app.session.editor.doc().markups.get(empty).is_none());
    assert!(rig.app.markup_ui.open.is_none());
    let id = rig.start();
    rig.drag(&line(&rig, 10));
    rig.app.markup_ui.push_request(Request::Finish);
    rig.frame(vec![]);
    assert_eq!(rig.markup(id).marks.len(), 1);
    assert!(rig.app.markup_ui.open.is_none());
    assert!(!rig.app.markup_ui.captures());
    // The viewpoint was saved with the markup: the plan view and the canvas size.
    let vp = &rig.markup(id).viewpoint;
    assert!(matches!(
        vp.view,
        kadakas_core::issue::ViewSpec::Plan { .. }
    ));
    assert!(vp.canvas[0] > 100.0);
}

#[test]
fn the_palette_draws_in_english_and_estonian_without_english_leaks() {
    let mut rig = Rig::new();
    rig.start();
    let ctx = egui::Context::default();
    for language in ["en-US", "et"] {
        kadakas_l10n::with_language(language, || {
            let mut texts = Vec::new();
            for _ in 0..2 {
                let out = ctx.run_ui(
                    RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 900.0))),
                        ..Default::default()
                    },
                    |ui| {
                        egui::CentralPanel::default().show(ui, |ui| {
                            rig.app.markup_ui.palette(ui, &rig.app.session);
                        });
                    },
                );
                texts.extend(crate::text_probe::shown_text(&out));
            }
            if language == "et" {
                assert!(texts.iter().any(|t| t == "Pliiats"), "{texts:?}");
                let leaks = crate::text_probe::english_leaks(&texts);
                assert!(leaks.is_empty(), "{leaks:?}");
            } else {
                assert!(texts.iter().any(|t| t == "Pen"), "{texts:?}");
            }
        });
    }
}
