//! Markup on the view in the app (REV-11, stage 1): the Markup palette, the ink on the plan and on
//! the 3D view, and the window of a note.
//!
//! The logic is in `kadakas_ui_core::markup` (the tools, the stroke, the hit test, the test that
//! tells whether the view is the one of a markup) and the data is in `kadakas_core::markup`. This
//! module draws it with egui and turns input into [`Request`]s. The app runs the requests at the
//! end of the frame (`app/markup.rs`), so every change is one undo step and goes through the same
//! guards as other edits (read only, a shared document that is not ready).
//!
//! The marks are painted over the canvas in screen space. They are not part of the scene and not
//! of the picture of the model. They are shown while a markup is open, or after a click on a
//! markup in the list, and only when the view is the view the markup was drawn in.

use egui::{Color32, CursorIcon, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui, vec2};
use kadakas_api::Session;
use kadakas_core::change::Origin;
use kadakas_core::command::Command;
use kadakas_core::markup::draw::{NOTE_BACKGROUND_ALPHA, Prim, markup_prims};
use kadakas_core::markup::{
    MAX_MARKS, Markup, MarkupId, MarkupPatch, MarkupViewpoint, PALETTE, Rgb,
};
use kadakas_core::model::Document;
use kadakas_l10n::tr;
use kadakas_ui_core::markup::{Brush, Draft, Tool, hit_mark, note_at, view_matches};
use kadakas_ui_number::UiNumber;

/// How far from a mark the eraser and the select tool still hit it, in points.
const HIT_POINTS: f64 = 6.0;
/// The size of a colour swatch, in points.
const SWATCH: f32 = 20.0;
/// The widest the width slider goes, in points. A mark can be wider (the API takes up to 64).
const SLIDER_MAX: f64 = 24.0;
/// The accent colour of the frame and the badge of the markup mode.
const ACCENT: Color32 = Color32::from_rgb(214, 40, 40);

/// What the app does for the markup UI. The view and the guards are the app's.
#[derive(Clone, Debug)]
pub enum Request {
    /// A change of the document, one undo step.
    Cmd(Command),
    /// Start a markup with the view as it is now.
    New,
    /// Close the open markup. An empty one is removed.
    Finish,
    /// Undo the last step (the Undo button of the palette).
    Undo,
    /// Show the viewpoint of a markup: its layers and classes, its view and its marks.
    GoTo(MarkupId),
    /// Go to the viewpoint and draw on the markup again.
    Resume(MarkupId),
}

/// A note that waits for its text.
struct PendingNote {
    /// The place in the canvas, in points.
    at: [f64; 2],
    canvas: [f64; 2],
    /// Where the window of the text opens, on the screen.
    screen: Pos2,
    text: String,
    focus: bool,
}

/// The state of the markup UI.
pub struct MarkupUi {
    /// The markup mode is on: the pointer draws on the open markup.
    pub mode: bool,
    pub tool: Tool,
    pub brush: Brush,
    /// The markup that the pointer draws on.
    pub open: Option<MarkupId>,
    /// The markup that is picked in the list.
    pub picked: Option<MarkupId>,
    /// The mark that the select tool picked, in the open markup.
    pub selected: Option<u32>,
    /// The person hid the marks of the picked markup.
    pub hide_marks: bool,
    draft: Option<Draft>,
    note: Option<PendingNote>,
    requests: Vec<Request>,
    /// The view of the last frame of the canvas that is on the screen.
    current: Option<MarkupViewpoint>,
    /// A line for the status bar, from the input code (it has no access to the status).
    notice: Option<String>,
    name_buf: (Option<MarkupId>, String, String),
    confirm_delete: Option<MarkupId>,
}

impl Default for MarkupUi {
    fn default() -> Self {
        MarkupUi {
            mode: false,
            tool: Tool::Pen,
            brush: Brush::default(),
            open: None,
            picked: None,
            selected: None,
            hide_marks: false,
            draft: None,
            note: None,
            requests: Vec::new(),
            current: None,
            notice: None,
            name_buf: (None, String::new(), String::new()),
            confirm_delete: None,
        }
    }
}

fn color32(c: Rgb, alpha: f64) -> Color32 {
    Color32::from_rgba_unmultiplied(
        c.0[0],
        c.0[1],
        c.0[2],
        (alpha.clamp(0.0, 1.0) * 255.0) as u8,
    )
}

/// The viewpoint of the plan canvas now (without the visibility of layers).
pub fn plan_viewpoint(view: &kadakas_ui_core::view::View, rect: Rect) -> MarkupViewpoint {
    let lo = glam::DVec2::new(f64::from(rect.min.x), f64::from(rect.min.y));
    let hi = glam::DVec2::new(f64::from(rect.max.x), f64::from(rect.max.y));
    MarkupViewpoint {
        view: kadakas_ui_core::issues::plan_spec(view, lo, hi),
        fov: None,
        canvas: [f64::from(rect.width()), f64::from(rect.height())],
        layers: Default::default(),
        classes: Default::default(),
    }
}

/// The viewpoint of the 3D canvas now. The camera works relative to the origin of the scene, a
/// viewpoint keeps drawing coordinates.
pub fn camera_viewpoint(
    camera: &kadakas_render::Camera,
    origin: glam::DVec3,
    rect: Rect,
) -> MarkupViewpoint {
    let t = glam::DVec3::new(
        f64::from(camera.target.x),
        f64::from(camera.target.y),
        f64::from(camera.target.z),
    ) + origin;
    MarkupViewpoint {
        view: kadakas_core::issue::ViewSpec::Camera {
            target: [t.x, t.y, t.z],
            yaw: f64::from(camera.yaw),
            pitch: f64::from(camera.pitch),
            distance: f64::from(camera.distance),
            perspective: camera.projection == kadakas_render::Projection::Perspective,
        },
        fov: Some(f64::from(camera.fov_y).to_degrees()),
        canvas: [f64::from(rect.width()), f64::from(rect.height())],
        layers: Default::default(),
        classes: Default::default(),
    }
}

fn tool_name(t: Tool) -> String {
    match t {
        Tool::Pen => tr!("markup-ui-tool-pen"),
        Tool::Highlighter => tr!("markup-ui-tool-highlighter"),
        Tool::Arrow => tr!("markup-ui-tool-arrow"),
        Tool::Rectangle => tr!("markup-ui-tool-rectangle"),
        Tool::Ellipse => tr!("markup-ui-tool-ellipse"),
        Tool::Note => tr!("markup-ui-tool-note"),
        Tool::Select => tr!("markup-ui-tool-select"),
        Tool::Eraser => tr!("markup-ui-tool-eraser"),
    }
}

fn tool_hint(t: Tool) -> Option<String> {
    match t {
        Tool::Select => Some(tr!("markup-ui-tool-select-hint")),
        Tool::Eraser => Some(tr!("markup-ui-tool-eraser-hint")),
        _ => None,
    }
}

/// The name of a markup for a list: its name, else "Markup" and its number.
pub fn display_name(doc: &Document, m: &Markup) -> String {
    match &m.name {
        Some(n) => n.clone(),
        None => {
            let number = doc.markups.position(m.id).map_or(0, |i| i + 1);
            tr!("markup-ui-default-name", number = number)
        }
    }
}

impl MarkupUi {
    /// The input of the canvas does what the markup says: true while the markup takes the
    /// pointer, so the canvas must not pan, orbit, select or give the press to a tool.
    pub fn captures(&self) -> bool {
        self.mode && self.open.is_some()
    }

    /// True if Delete removes the picked mark and not the selected objects.
    pub fn wants_delete(&self) -> bool {
        self.captures() && self.selected.is_some()
    }

    /// The requests that the UI made since the last call.
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    #[cfg(test)]
    pub fn push_request(&mut self, r: Request) {
        self.requests.push(r);
    }

    /// A line for the status bar, if the UI has one.
    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    /// The view of the canvas that was last on the screen.
    pub fn current_view(&self) -> Option<&MarkupViewpoint> {
        self.current.as_ref()
    }

    /// True if the marks of the markup that the canvas shows are drawn now: it is shown and the
    /// view is the view of the markup.
    pub fn marks_visible(&self, doc: &Document) -> bool {
        match (self.shown(doc), &self.current) {
            (Some(m), Some(now)) => view_matches(&m.viewpoint, now),
            _ => false,
        }
    }

    /// The note that waits for its text, for the tests: gives the text and its place.
    #[cfg(test)]
    pub fn finish_note_for_test(&mut self, text: &str) {
        if let (Some(n), Some(id)) = (self.note.take(), self.open) {
            self.requests.push(Request::Cmd(Command::AddMark {
                markup: id,
                mark: Box::new(note_at(self.brush, n.canvas, n.at, text)),
            }));
        }
    }

    /// True while a note waits for its text.
    #[cfg(test)]
    pub fn note_pending(&self) -> bool {
        self.note.is_some()
    }

    /// Stops drawing on the open markup and gives its ID.
    pub fn close_open(&mut self) -> Option<MarkupId> {
        self.draft = None;
        self.note = None;
        self.selected = None;
        self.open.take()
    }

    /// Switches the markup mode. Ending it drops a stroke that is being drawn.
    pub fn set_mode(&mut self, on: bool) {
        self.mode = on;
        if !on {
            self.draft = None;
            self.note = None;
        }
    }

    /// The markup that the canvas shows: the open one, else the one picked in the list.
    fn shown<'a>(&self, doc: &'a Document) -> Option<&'a Markup> {
        let id = self
            .open
            .or(if self.hide_marks { None } else { self.picked })?;
        doc.markups.get(id)
    }

    /// Called by a canvas in every frame in which it is on the screen. Gives true when the markup
    /// took the pointer. `suspend` is true while the person pans with the key held.
    pub fn canvas_frame(
        &mut self,
        ui: &Ui,
        resp: &egui::Response,
        rect: Rect,
        now: MarkupViewpoint,
        suspend: bool,
        doc: &Document,
        read_only: bool,
    ) -> bool {
        self.current = Some(now.clone());
        let Some(open) = self.open.and_then(|id| doc.markups.get(id)) else {
            self.draft = None;
            if self.open.is_some() {
                self.open = None;
            }
            return false;
        };
        if !self.mode {
            self.draft = None;
            return false;
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
        if suspend {
            self.draft = None;
            return true;
        }
        let canvas = [f64::from(rect.width()), f64::from(rect.height())];
        let local = |p: Pos2| {
            [
                f64::from((p.x - rect.min.x).clamp(0.0, rect.width())),
                f64::from((p.y - rect.min.y).clamp(0.0, rect.height())),
            ]
        };
        let (pressed, down, released, esc, moves, latest) = ui.input(|i| {
            let moves: Vec<Pos2> = i
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::PointerMoved(p) => Some(*p),
                    _ => None,
                })
                .collect();
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.key_pressed(egui::Key::Escape),
                moves,
                i.pointer.latest_pos(),
            )
        });
        if esc {
            self.draft = None;
            self.note = None;
        }
        let drawable = view_matches(&open.viewpoint, &now) && !read_only;
        if pressed && resp.hovered() && self.draft.is_none() && self.note.is_none() {
            if let (true, Some(p)) = (drawable, latest) {
                let at = local(p);
                match self.tool {
                    Tool::Select => {
                        self.selected = hit_mark(open, canvas, at, HIT_POINTS);
                    }
                    Tool::Eraser => {
                        if let Some(mark) = hit_mark(open, canvas, at, HIT_POINTS) {
                            if self.selected == Some(mark) {
                                self.selected = None;
                            }
                            self.requests.push(Request::Cmd(Command::DeleteMark {
                                markup: open.id,
                                mark,
                            }));
                        }
                    }
                    Tool::Note => {
                        if open.marks.len() >= MAX_MARKS {
                            self.notice = Some(tr!("markup-ui-limit", marks = MAX_MARKS));
                        } else {
                            self.note = Some(PendingNote {
                                at,
                                canvas,
                                screen: p + vec2(12.0, 12.0),
                                text: String::new(),
                                focus: true,
                            });
                        }
                    }
                    t => self.draft = Draft::begin(t, self.brush, canvas, at),
                }
            }
        }
        if let Some(d) = &mut self.draft {
            // Every move of this frame, not only the last one, so a quick stroke stays smooth.
            for p in moves {
                d.add(local(p));
            }
            if let Some(p) = latest {
                d.add(local(p));
            }
            ui.ctx().request_repaint();
            if released || !down {
                let marks = d.finish();
                self.draft = None;
                if open.marks.len() + marks.len() > MAX_MARKS {
                    self.notice = Some(tr!("markup-ui-limit", marks = MAX_MARKS));
                } else {
                    for mark in marks {
                        self.requests.push(Request::Cmd(Command::AddMark {
                            markup: open.id,
                            mark: Box::new(mark),
                        }));
                    }
                }
            }
        }
        true
    }

    /// Paints the marks, the stroke that is being drawn and the sign of the mode over a canvas.
    pub fn paint(&self, painter: &Painter, rect: Rect, doc: &Document) {
        let canvas = [f64::from(rect.width()), f64::from(rect.height())];
        if let (Some(m), Some(now)) = (self.shown(doc), &self.current) {
            if view_matches(&m.viewpoint, now) {
                paint_prims(painter, rect.min, &markup_prims(m, canvas));
                if let Some(mark) = self.selected.and_then(|id| m.mark(id))
                    && self.open == Some(m.id)
                    && let Some((lo, hi)) = mark.bounds()
                {
                    let r = Rect::from_min_max(
                        rect.min + vec2((lo[0] * canvas[0]) as f32, (lo[1] * canvas[1]) as f32),
                        rect.min + vec2((hi[0] * canvas[0]) as f32, (hi[1] * canvas[1]) as f32),
                    )
                    .expand(6.0);
                    let c = [
                        r.left_top(),
                        r.right_top(),
                        r.right_bottom(),
                        r.left_bottom(),
                    ];
                    painter.add(Shape::dashed_line(
                        &[c[0], c[1], c[2], c[3], c[0]],
                        Stroke::new(1.5, ACCENT),
                        5.0,
                        4.0,
                    ));
                }
            } else {
                let name = display_name(doc, m);
                let text = if self.open == Some(m.id) {
                    tr!("markup-ui-paused")
                } else {
                    tr!("markup-ui-view-differs", name = name)
                };
                label(painter, rect.left_bottom() + vec2(8.0, -8.0), &text);
            }
        }
        if let Some(d) = &self.draft {
            paint_prims(painter, rect.min, &d.preview());
        }
        if let Some(n) = &self.note {
            let mut m = note_at(self.brush, n.canvas, n.at, "...");
            m.points = vec![kadakas_core::markup::rel_of(n.at, n.canvas)];
            paint_prims(
                painter,
                rect.min,
                &kadakas_core::markup::draw::mark_prims(&m, canvas, 1.0),
            );
        }
        if self.captures() {
            painter.rect_stroke(
                rect.shrink(1.0),
                0.0,
                Stroke::new(2.0, ACCENT),
                egui::StrokeKind::Inside,
            );
            label(
                painter,
                rect.left_top() + vec2(8.0, 8.0),
                &tr!("markup-ui-badge", tool = tool_name(self.tool)),
            );
        }
    }

    /// The window of a note: its text.
    pub fn windows(&mut self, ctx: &egui::Context) {
        let Some(n) = &mut self.note else { return };
        let mut done = None;
        egui::Window::new(tr!("markup-ui-note-text"))
            .id(egui::Id::new("win-markup-note"))
            .fixed_pos(n.screen)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut n.text)
                        .desired_width(220.0)
                        .char_limit(kadakas_core::markup::MAX_MARK_TEXT_CHARS),
                );
                if n.focus {
                    edit.request_focus();
                    n.focus = false;
                }
                let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    let ok = ui
                        .add_enabled(
                            !n.text.trim().is_empty(),
                            egui::Button::new(tr!("markup-ui-note-ok")),
                        )
                        .clicked();
                    if (ok || enter) && !n.text.trim().is_empty() {
                        done = Some(true);
                    }
                    if ui.button(tr!("markup-ui-note-cancel")).clicked() {
                        done = Some(false);
                    }
                });
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        match done {
            Some(true) => {
                if let (Some(n), Some(id)) = (self.note.take(), self.open) {
                    let mark = note_at(self.brush, n.canvas, n.at, &n.text);
                    self.requests.push(Request::Cmd(Command::AddMark {
                        markup: id,
                        mark: Box::new(mark),
                    }));
                }
            }
            Some(false) => self.note = None,
            None => {}
        }
    }

    /// The Markup palette.
    pub fn palette(&mut self, ui: &mut Ui, session: &Session) {
        let doc = session.editor.doc();
        let mut mode = self.mode;
        if ui
            .checkbox(&mut mode, tr!("markup-ui-mode"))
            .on_hover_text(tr!("markup-ui-mode-hint"))
            .changed()
        {
            self.set_mode(mode);
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(tr!("markup-ui-new"))
                .on_hover_text(tr!("markup-ui-new-hint"))
                .clicked()
            {
                self.requests.push(Request::New);
            }
            let open = self.open.is_some();
            if ui
                .add_enabled(open, egui::Button::new(tr!("markup-ui-finish")))
                .on_hover_text(tr!("markup-ui-finish-hint"))
                .clicked()
            {
                self.requests.push(Request::Finish);
            }
        });
        ui.separator();
        // The tools.
        ui.horizontal_wrapped(|ui| {
            for t in Tool::ALL {
                let mut r = ui.selectable_label(self.tool == t, tool_name(t));
                if let Some(h) = tool_hint(t) {
                    r = r.on_hover_text(h);
                }
                if r.clicked() {
                    self.tool = t;
                    if t != Tool::Select {
                        self.draft = None;
                    }
                }
            }
        });
        // Colours: eight to choose from, and any other.
        ui.label(tr!("markup-ui-color"));
        ui.horizontal_wrapped(|ui| {
            for (i, c) in PALETTE.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::click());
                let on = self.brush.color == *c;
                ui.painter()
                    .rect_filled(rect.shrink(1.0), 3.0, color32(*c, 1.0));
                let ring = if on {
                    Stroke::new(2.0, ui.visuals().strong_text_color())
                } else {
                    Stroke::new(1.0, ui.visuals().weak_text_color())
                };
                ui.painter()
                    .rect_stroke(rect.shrink(1.0), 3.0, ring, egui::StrokeKind::Outside);
                if on {
                    // A sign that is not a color: a check mark in the contrasting color.
                    let ink = if c.0.iter().map(|v| u32::from(*v)).sum::<u32>() > 380 {
                        Color32::BLACK
                    } else {
                        Color32::WHITE
                    };
                    let m = rect.center();
                    ui.painter().add(Shape::line(
                        vec![
                            m + vec2(-5.0, 0.0),
                            m + vec2(-1.5, 4.0),
                            m + vec2(5.0, -4.0),
                        ],
                        Stroke::new(2.0, ink),
                    ));
                }
                if resp
                    .on_hover_text(tr!("markup-ui-color-name", index = i))
                    .clicked()
                {
                    self.set_color(*c);
                }
            }
            let mut rgb = self.brush.color.0;
            if ui
                .color_edit_button_srgb(&mut rgb)
                .on_hover_text(tr!("markup-ui-color-custom"))
                .changed()
            {
                self.set_color(Rgb(rgb));
            }
        });
        ui.horizontal(|ui| {
            ui.label(tr!("markup-ui-width"));
            let r = ui.add(egui::Slider::new(&mut self.brush.width, 1.0..=SLIDER_MAX).ui_number());
            // A slider that is dragged changes the width once, when the person lets go.
            if r.drag_stopped() || (r.changed() && !r.dragged() && !r.has_focus()) {
                self.set_width();
            }
        });
        // The open markup: undo, clear and delete a mark.
        ui.horizontal_wrapped(|ui| {
            let open = self.open.and_then(|id| doc.markups.get(id));
            let marks = open.map_or(0, |m| m.marks.len());
            if ui
                .add_enabled(
                    session.editor.can_undo(),
                    egui::Button::new(tr!("panels-undo")),
                )
                .clicked()
            {
                self.requests.push(Request::Undo);
            }
            if ui
                .add_enabled(marks > 0, egui::Button::new(tr!("markup-ui-clear")))
                .on_hover_text(tr!("markup-ui-clear-hint"))
                .clicked()
                && let Some(id) = self.open
            {
                self.selected = None;
                self.requests
                    .push(Request::Cmd(Command::ClearMarkup { markup: id }));
            }
            if ui
                .add_enabled(
                    self.selected.is_some(),
                    egui::Button::new(tr!("markup-ui-delete-mark")),
                )
                .clicked()
            {
                self.delete_selected();
            }
            let label = if self.hide_marks {
                tr!("markup-ui-show")
            } else {
                tr!("markup-ui-hide")
            };
            if ui
                .add_enabled(
                    self.picked.is_some() && self.open.is_none(),
                    egui::Button::new(label),
                )
                .clicked()
            {
                self.hide_marks = !self.hide_marks;
            }
        });
        ui.separator();
        ui.strong(tr!("markup-ui-list-heading"));
        if doc.markups.is_empty() {
            ui.label(tr!("markup-ui-none"));
        }
        egui::ScrollArea::vertical()
            .id_salt("markup-list")
            .max_height(220.0)
            .show(ui, |ui| {
                for m in doc.markups.iter() {
                    self.row_ui(ui, doc, m);
                }
            });
        let Some(m) = self.picked.and_then(|id| doc.markups.get(id)) else {
            return;
        };
        ui.separator();
        self.detail_ui(ui, doc, m);
    }

    fn row_ui(&mut self, ui: &mut Ui, doc: &Document, m: &Markup) {
        let selected = self.picked == Some(m.id);
        let mode = match m.viewpoint.mode() {
            kadakas_core::markup::MarkupMode::Plan => tr!("markup-ui-mode-plan"),
            kadakas_core::markup::MarkupMode::View3d => tr!("markup-ui-mode-3d"),
        };
        let author = if m.origin == Origin::Agent {
            tr!("markup-ui-agent")
        } else {
            m.author.clone()
        };
        let text = format!(
            "{}\n{}",
            display_name(doc, m),
            tr!(
                "markup-ui-row-info",
                mode = mode,
                marks = m.marks.len(),
                author = author
            )
        );
        if ui.selectable_label(selected, text).clicked() {
            self.picked = Some(m.id);
            self.hide_marks = false;
            self.requests.push(Request::GoTo(m.id));
        }
    }

    fn detail_ui(&mut self, ui: &mut Ui, doc: &Document, m: &Markup) {
        if self.name_buf.0 != Some(m.id) {
            self.name_buf = (
                Some(m.id),
                m.name.clone().unwrap_or_default(),
                m.note.clone(),
            );
        }
        ui.horizontal(|ui| {
            ui.label(tr!("markup-ui-name"));
            let r = ui.add(egui::TextEdit::singleline(&mut self.name_buf.1).desired_width(160.0));
            if r.lost_focus() && self.name_buf.1.trim() != m.name.clone().unwrap_or_default() {
                self.patch(
                    m.id,
                    MarkupPatch {
                        name: Some(self.name_buf.1.clone()),
                        ..Default::default()
                    },
                );
            }
        });
        ui.label(tr!("markup-ui-note"));
        let r = ui.add(
            egui::TextEdit::multiline(&mut self.name_buf.2)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        if r.lost_focus() && self.name_buf.2 != m.note {
            self.patch(
                m.id,
                MarkupPatch {
                    note: Some(self.name_buf.2.clone()),
                    ..Default::default()
                },
            );
        }
        ui.horizontal(|ui| {
            ui.label(tr!("markup-ui-issue"));
            let current = m
                .issue
                .and_then(|i| doc.issues.get(i))
                .map(|i| i.title.clone())
                .unwrap_or_else(|| tr!("markup-ui-no-issue"));
            let mut choice = m.issue;
            egui::ComboBox::from_id_salt("markup-issue")
                .selected_text(current)
                .width(160.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut choice, None, tr!("markup-ui-no-issue"));
                    for i in doc.issues.iter().take(200) {
                        ui.selectable_value(&mut choice, Some(i.id), i.title.clone());
                    }
                })
                .response
                .on_hover_text(tr!("markup-ui-issue-hint"));
            if choice != m.issue {
                self.patch(
                    m.id,
                    MarkupPatch {
                        issue: Some(choice),
                        ..Default::default()
                    },
                );
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(tr!("markup-ui-go-to"))
                .on_hover_text(tr!("markup-ui-go-to-hint"))
                .clicked()
            {
                self.requests.push(Request::GoTo(m.id));
            }
            if m.origin == Origin::User
                && self.open != Some(m.id)
                && ui.button(tr!("markup-ui-resume")).clicked()
            {
                self.requests.push(Request::Resume(m.id));
            }
            if ui.button(tr!("markup-ui-delete")).clicked() {
                if m.origin == Origin::User {
                    self.delete_markup(m.id);
                } else {
                    self.confirm_delete = Some(m.id);
                }
            }
        });
        if self.confirm_delete == Some(m.id) {
            ui.label(tr!("markup-ui-delete-agent"));
            ui.horizontal(|ui| {
                if ui.button(tr!("markup-ui-delete-yes")).clicked() {
                    self.confirm_delete = None;
                    self.delete_markup(m.id);
                }
                if ui.button(tr!("markup-ui-delete-no")).clicked() {
                    self.confirm_delete = None;
                }
            });
        }
    }

    fn patch(&mut self, id: MarkupId, patch: MarkupPatch) {
        self.requests.push(Request::Cmd(Command::ModifyMarkup {
            id,
            patch: Box::new(patch),
        }));
    }

    fn delete_markup(&mut self, id: MarkupId) {
        if self.open == Some(id) {
            self.open = None;
            self.draft = None;
        }
        if self.picked == Some(id) {
            self.picked = None;
        }
        self.requests
            .push(Request::Cmd(Command::DeleteMarkup { id }));
    }

    /// Deletes the mark that the select tool picked.
    pub fn delete_selected(&mut self) {
        if let (Some(markup), Some(mark)) = (self.open, self.selected.take()) {
            self.requests
                .push(Request::Cmd(Command::DeleteMark { markup, mark }));
        }
    }

    fn set_color(&mut self, c: Rgb) {
        self.brush.color = c;
        if let (Some(markup), Some(mark)) = (self.open, self.selected) {
            self.requests.push(Request::Cmd(Command::SetMarkStyle {
                markup,
                mark,
                color: Some(c),
                width: None,
            }));
        }
    }

    fn set_width(&mut self) {
        if let (Some(markup), Some(mark)) = (self.open, self.selected) {
            self.requests.push(Request::Cmd(Command::SetMarkStyle {
                markup,
                mark,
                color: None,
                width: Some(self.brush.width),
            }));
        }
    }
}

/// A line of text on a pale box, for a hint on the canvas.
fn label(painter: &Painter, at: Pos2, text: &str) {
    let galley =
        painter.layout_no_wrap(text.to_string(), FontId::proportional(12.0), Color32::WHITE);
    let r = Rect::from_min_size(at, galley.size()).expand2(vec2(6.0, 3.0));
    // The box grows up from the point when the text would leave the canvas at the bottom.
    let r = if r.max.y > painter.clip_rect().max.y {
        r.translate(vec2(0.0, painter.clip_rect().max.y - r.max.y))
    } else {
        r
    };
    painter.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(40, 40, 40, 215));
    painter.galley(r.min + vec2(6.0, 3.0), galley, Color32::WHITE);
}

/// Paints primitives (points of the canvas) with the top left corner of the canvas at `origin`.
pub fn paint_prims(painter: &Painter, origin: Pos2, prims: &[Prim]) {
    let at = |p: &[f64; 2]| origin + vec2(p[0] as f32, p[1] as f32);
    for prim in prims {
        match prim {
            Prim::Line {
                points,
                closed,
                width,
                color,
                alpha,
                round,
            } => {
                let pts: Vec<Pos2> = points.iter().map(at).collect();
                let c = color32(*color, *alpha);
                let stroke = Stroke::new(*width as f32, c);
                if *closed {
                    painter.add(Shape::closed_line(pts, stroke));
                } else {
                    if *round && *width >= 3.0 {
                        for p in [pts.first(), pts.last()].into_iter().flatten() {
                            painter.circle_filled(*p, *width as f32 / 2.0, c);
                        }
                    }
                    painter.add(Shape::line(pts, stroke));
                }
            }
            Prim::Fill {
                points,
                color,
                alpha,
            } => {
                painter.add(Shape::convex_polygon(
                    points.iter().map(at).collect(),
                    color32(*color, *alpha),
                    Stroke::NONE,
                ));
            }
            Prim::Dot {
                at: p,
                radius,
                color,
                alpha,
            } => {
                painter.circle_filled(at(p), (*radius as f32).max(0.5), color32(*color, *alpha));
            }
            Prim::Note {
                at: p,
                text,
                size,
                color,
            } => {
                let size = *size as f32;
                let dot = at(p);
                painter.circle_filled(dot, 4.0 * size / 15.0, color32(*color, 1.0));
                let galley = painter.layout_no_wrap(
                    text.clone(),
                    FontId::proportional(size),
                    Color32::BLACK,
                );
                let pad = vec2(size * 0.4, size * 0.3);
                let mut min = dot + vec2(size * 0.6, -galley.size().y / 2.0 - pad.y);
                let full = galley.size() + pad * 2.0;
                if min.x + full.x > painter.clip_rect().max.x {
                    min.x = (dot.x - size * 0.6 - full.x).max(painter.clip_rect().min.x);
                }
                let r = Rect::from_min_size(min, full);
                painter.rect_filled(
                    r,
                    3.0,
                    Color32::from_rgba_unmultiplied(
                        255,
                        255,
                        255,
                        (NOTE_BACKGROUND_ALPHA * 255.0) as u8,
                    ),
                );
                painter.rect_stroke(
                    r,
                    3.0,
                    Stroke::new(1.5, color32(*color, 1.0)),
                    egui::StrokeKind::Inside,
                );
                painter.galley(r.min + pad, galley, Color32::BLACK);
            }
        }
    }
}
