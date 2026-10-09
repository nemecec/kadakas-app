//! The application: one `Session`, a view, the active tool and the panels.

use cadektor_core::api::Session;
use cadektor_core::command::Command;
use cadektor_core::model::{Document, ObjectId};
use egui::{Key, Sense};
use glam::DVec2;
use serde_json::Value;

use crate::canvas;
use crate::databar::{self, DataEntry};
use crate::file_io;
use crate::keymap::{self, Action, Chord, Keymap};
use crate::panels::{self, InfoEdit, MenuAction, NavState};
use crate::snap::{self, SnapConfig, SnapResult};
use crate::tools::{Ctx, Effect, Input, Tool, ToolMachine, ToolSettings};
use crate::view::{View, grid_step};

/// Pan step of an arrow key, in px.
const PAN_STEP_PX: f32 = 60.0;

/// Snap and pick distance in screen px.
const PICK_PX: f64 = 8.0;

pub struct CadektorApp {
    session: Session,
    view: View,
    needs_fit: bool,
    machine: ToolMachine,
    settings: ToolSettings,
    snap_cfg: SnapConfig,
    show_grid: bool,
    cursor: DVec2,
    snap_hit: Option<SnapResult>,
    status: String,
    path: Option<std::path::PathBuf>,
    /// The window title must be set in the next frame.
    title_dirty: bool,
    info: InfoEdit,
    nav: NavState,
    keys: Keymap,
    entry: DataEntry,
    /// Cursor before Data bar values are applied.
    live_cursor: DVec2,
    /// View changes from keys; applied in the next canvas pass.
    pending_zoom: f64,
    pending_pan: egui::Vec2,
    #[cfg(not(target_arch = "wasm32"))]
    agent: Option<crate::agent::AgentServer>,
}

impl CadektorApp {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(cc: &eframe::CreationContext<'_>, agent_port: Option<u16>) -> Self {
        let mut app = Self::blank();
        let (keys, warnings) = Keymap::load();
        app.keys = keys;
        for w in &warnings {
            eprintln!("cadektor: {w}");
        }
        let key_note = match warnings.as_slice() {
            [] => String::new(),
            [one] => one.clone(),
            [first, rest @ ..] => format!("{first} (+{} more keymap warnings, see stderr)", rest.len()),
        };
        app.status = key_note.clone();
        if let Some(port) = agent_port {
            let ctx = cc.egui_ctx.clone();
            let info_path = cadektor_core::fsio::agent_info_path();
            match crate::agent::start(port, std::sync::Arc::new(move || ctx.request_repaint()), info_path) {
                Ok(server) => {
                    app.status = format!("Agent server listening on {} (token in the agent runtime file) {key_note}", server.addr);
                    app.agent = Some(server);
                }
                Err(e) => app.status = format!("Agent server failed on port {port}: {e}"),
            }
        }
        app
    }

    fn blank() -> Self {
        CadektorApp {
            session: Session::default(),
            view: View::default(),
            needs_fit: true,
            machine: ToolMachine::default(),
            settings: ToolSettings::default(),
            snap_cfg: SnapConfig::default(),
            show_grid: true,
            cursor: DVec2::ZERO,
            snap_hit: None,
            status: "Ready".into(),
            path: None,
            title_dirty: false,
            info: InfoEdit::default(),
            nav: NavState::default(),
            keys: Keymap::defaults(),
            entry: DataEntry::default(),
            live_cursor: DVec2::ZERO,
            pending_zoom: 1.0,
            pending_pan: egui::Vec2::ZERO,
            #[cfg(not(target_arch = "wasm32"))]
            agent: None,
        }
    }

    fn exec(&mut self, cmd: &Command) {
        panels::run_command(&mut self.session, cmd, &mut self.status);
    }

    fn replace_document(&mut self, doc: Document, path: Option<std::path::PathBuf>) {
        self.session = Session::with_document(doc);
        self.path = path;
        self.title_dirty = true;
        self.machine.cancel();
        self.needs_fit = true;
    }

    fn undo(&mut self) {
        let _ = self.session.handle("undo", Value::Null);
    }

    fn redo(&mut self) {
        let _ = self.session.handle("redo", Value::Null);
    }

    fn delete_selection(&mut self) {
        if !self.session.selection.is_empty() {
            let ids = self.session.selection.clone();
            self.exec(&Command::DeleteObjects { ids });
        }
    }

    fn select_all(&mut self) {
        let doc = &self.session.editor.doc;
        self.session.selection = doc.objects.values().filter(|o| doc.is_visible(o)).map(|o| o.id).collect();
    }

    fn apply_menu(&mut self, ctx: &egui::Context, action: MenuAction) {
        match action {
            MenuAction::New => self.replace_document(Document::default(), None),
            MenuAction::Undo => self.undo(),
            MenuAction::Redo => self.redo(),
            MenuAction::Delete => self.delete_selection(),
            MenuAction::SelectAll => self.select_all(),
            MenuAction::ZoomFit => self.needs_fit = true,
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::Open => self.open_file(ctx),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::Save => self.save(false, ctx),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::SaveAs => self.save(true, ctx),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::ExportSvg => self.export_svg(),
            #[cfg(target_arch = "wasm32")]
            _ => {
                let _ = ctx;
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn set_title(&self, ctx: &egui::Context) {
        let name = self.path.as_ref().and_then(|p| p.file_name()).map_or("Untitled".into(), |n| n.to_string_lossy().into_owned());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{name} - Cadektor")));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_file(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new().add_filter("Cadektor", &["json"]).pick_file() else { return };
        match file_io::read_document(&path) {
            Ok(doc) => {
                self.status = format!("Opened {}", path.display());
                self.replace_document(doc, Some(path));
                self.set_title(ctx);
            }
            Err(e) => self.status = e,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save(&mut self, as_new: bool, ctx: &egui::Context) {
        let path = match (&self.path, as_new) {
            (Some(p), false) => p.clone(),
            _ => {
                let Some(p) = rfd::FileDialog::new().add_filter("Cadektor", &["json"]).set_file_name("drawing.cdk.json").save_file() else {
                    return;
                };
                file_io::with_extension(&p)
            }
        };
        match file_io::write_document(&path, &self.session.editor.doc) {
            Ok(()) => {
                self.status = format!("Saved {}", path.display());
                self.path = Some(path);
                self.set_title(ctx);
            }
            Err(e) => self.status = e,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn export_svg(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("SVG", &["svg"]).set_file_name("drawing.svg").save_file() else { return };
        self.status = match file_io::write_svg(&p, &self.session.editor.doc) {
            Ok(()) => format!("Exported {}", p.display()),
            Err(e) => e,
        };
    }

    fn nudge(&mut self, dir: DVec2) {
        if self.session.selection.is_empty() {
            return;
        }
        let step = grid_step(self.view.zoom, 12.0);
        let ids = self.session.selection.clone();
        self.exec(&Command::MoveObjects { ids, delta: dir * step });
    }

    fn run_action(&mut self, ctx: &egui::Context, action: Action) {
        if let Some(t) = action.tool() {
            self.machine.set_tool(t);
            return;
        }
        match action {
            Action::FileNew => self.apply_menu(ctx, MenuAction::New),
            Action::FileOpen => self.apply_menu(ctx, MenuAction::Open),
            Action::FileSave => self.apply_menu(ctx, MenuAction::Save),
            Action::FileSaveAs => self.apply_menu(ctx, MenuAction::SaveAs),
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            // While drawing, Delete and Backspace take back the last clicked point.
            Action::Delete if self.machine.is_busy() => self.machine.pop_point(),
            Action::Delete if self.machine.tool() == Tool::Select => self.delete_selection(),
            Action::SelectAll => self.select_all(),
            Action::Cancel => {
                if self.machine.is_busy() {
                    self.machine.cancel();
                } else {
                    self.machine.set_tool(Tool::Select);
                }
            }
            Action::Finish => self.send(Input::Enter),
            Action::NudgeLeft => self.nudge(DVec2::NEG_X),
            Action::NudgeRight => self.nudge(DVec2::X),
            Action::NudgeUp => self.nudge(DVec2::Y),
            Action::NudgeDown => self.nudge(DVec2::NEG_Y),
            Action::ZoomFit => self.needs_fit = true,
            Action::ZoomIn => self.pending_zoom *= 2.0,
            Action::ZoomOut => self.pending_zoom *= 0.5,
            // Arrow keys move the view like the scroll bars would: the drawing follows the key.
            Action::PanLeft => self.pending_pan.x += PAN_STEP_PX,
            Action::PanRight => self.pending_pan.x -= PAN_STEP_PX,
            Action::PanUp => self.pending_pan.y += PAN_STEP_PX,
            Action::PanDown => self.pending_pan.y -= PAN_STEP_PX,
            Action::Delete | Action::PanHold | Action::SnapSuspend | Action::DataBarFocus | Action::DataBarPrev => {}
            a if !a.implemented() => self.status = format!("{} ({}) is planned, not built yet", a.label(), self.keys.hint(a)),
            _ => {}
        }
    }

    /// Tab and Shift+Tab: enter the Data bar fields, or move between them.
    fn data_bar_key(&mut self, ctx: &egui::Context, action: Action, typing: bool) {
        if self.entry.active {
            self.entry.cycle(action == Action::DataBarPrev);
        } else if !typing && action == Action::DataBarFocus && databar::supported(self.machine.tool()) {
            self.entry.activate();
        } else {
            return;
        }
        // Keep egui from moving its own focus.
        ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, Key::Tab);
            i.consume_key(egui::Modifiers::SHIFT, Key::Tab);
        });
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        // egui drops focus on Esc before this runs, so the focus state of the last frame counts too.
        let typing = ctx.egui_wants_keyboard_input() || self.entry.active;
        let events: Vec<(Key, egui::Modifiers, bool)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, repeat, modifiers, .. } => Some((*key, *modifiers, *repeat)),
                    _ => None,
                })
                .collect()
        });
        for (key, mods, repeat) in events {
            let chord = Chord::from_event(key, mods);
            let Some(action) = self.keys.action_for(chord) else { continue };
            if repeat && !action.repeats() {
                continue;
            }
            if matches!(action, Action::DataBarFocus | Action::DataBarPrev) {
                self.data_bar_key(ctx, action, typing);
            } else if !typing || keymap::allowed_in_text_field(action, chord) {
                self.run_action(ctx, action);
            }
        }
    }

    /// Enter in a Data bar field: click at the typed point, as if the user had clicked there.
    fn commit_entry(&mut self, ctx: &egui::Context) {
        let target = databar::target_point(self.machine.tool(), self.machine.anchor(), self.live_cursor, &self.entry.values());
        let Some(p) = target else { return };
        let was_anchored = self.machine.anchor().is_some();
        self.send(Input::Press { p, shift: false });
        self.entry.clear_text();
        if was_anchored && !self.machine.is_busy() {
            self.entry.release(ctx);
        } else {
            self.entry.want_focus = true;
        }
    }

    fn tol(&self) -> f64 {
        PICK_PX / self.view.zoom
    }

    fn send(&mut self, input: Input) {
        let effects = {
            let cx = Ctx { doc: &self.session.editor.doc, selection: &self.session.selection, settings: &self.settings, tol: self.tol() };
            self.machine.handle(input, &cx)
        };
        for e in effects {
            match e {
                Effect::SetSelection(s) => self.session.selection = s,
                Effect::Exec(cmd) => {
                    let is_add = matches!(cmd, Command::AddObject { .. });
                    if let Some(out) = panels::run_command(&mut self.session, &cmd, &mut self.status)
                        && is_add
                        && !out.created.is_empty()
                    {
                        self.session.selection = out.created.into_iter().map(ObjectId).collect();
                    }
                }
            }
        }
    }

    fn fit_view(&mut self, rect: egui::Rect) {
        let (lo, hi) = cadektor_core::svg::drawing_bounds(&self.session.editor.doc)
            .unwrap_or((DVec2::new(-1000.0, -1000.0), DVec2::new(19000.0, 14000.0)));
        self.view.fit(lo, hi, rect);
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        if self.needs_fit {
            self.fit_view(rect);
            self.needs_fit = false;
        }
        let typing = ui.ctx().egui_wants_keyboard_input() || self.entry.active;
        let (latest, hover, mods, space, suspend, scroll, pinch, pressed, released) = ui.input(|i| {
            (
                i.pointer.latest_pos(),
                i.pointer.hover_pos(),
                i.modifiers,
                !typing && self.keys.held(Action::PanHold, i),
                !typing && self.keys.held(Action::SnapSuspend, i),
                i.smooth_scroll_delta.y,
                i.zoom_delta(),
                i.pointer.primary_pressed(),
                i.pointer.primary_released(),
            )
        });

        // Pan and zoom from keys.
        if self.pending_zoom != 1.0 {
            self.view.zoom_at(rect.center(), std::mem::replace(&mut self.pending_zoom, 1.0));
        }
        if self.pending_pan != egui::Vec2::ZERO {
            self.view.pan(std::mem::take(&mut self.pending_pan));
        }

        // Pan and zoom.
        let panning = resp.dragged_by(egui::PointerButton::Middle) || (space && resp.dragged_by(egui::PointerButton::Primary));
        if panning {
            self.view.pan(resp.drag_delta());
        }
        if let (true, Some(h)) = (resp.hovered(), hover) {
            let factor = (scroll as f64 * 0.0025).exp() * pinch as f64;
            if (factor - 1.0).abs() > 1e-9 {
                self.view.zoom_at(h, factor);
            }
        }

        // Cursor and snapping.
        if let Some(p) = latest.filter(|p| rect.contains(*p) || self.machine.is_busy()) {
            let raw = self.view.to_model(p);
            let busy_ids = self.machine.moving_ids().to_vec();
            self.snap_hit = if suspend {
                None
            } else {
                let pts = snap::snap_points(&self.session.editor.doc, &busy_ids);
                snap::snap(raw, self.tol(), grid_step(self.view.zoom, 12.0), self.snap_cfg, &pts)
            };
            self.cursor = self.snap_hit.map_or(raw, |s| s.point);
            self.live_cursor = self.cursor;
            // Typed Data bar values override the mouse.
            if self.entry.any_typed()
                && let Some(p) = databar::target_point(self.machine.tool(), self.machine.anchor(), self.live_cursor, &self.entry.values())
            {
                self.cursor = p;
            }
        } else {
            self.snap_hit = None;
        }

        // Tool events.
        if !panning && !space {
            if pressed && resp.hovered() {
                self.send(Input::Press { p: self.cursor, shift: mods.shift });
            } else if hover.is_some() || self.machine.is_busy() {
                self.send(Input::Move(self.cursor));
            }
            if released {
                self.send(Input::Release(self.cursor));
            }
            if resp.double_clicked() {
                self.send(Input::DoubleClick);
            }
        }
        if resp.hovered() && self.machine.tool() != Tool::Select {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }

        let cx = Ctx { doc: &self.session.editor.doc, selection: &self.session.selection, settings: &self.settings, tol: self.tol() };
        let preview = if resp.hovered() || self.machine.is_busy() { self.machine.preview(self.cursor, &cx) } else { crate::tools::Preview::None };
        let frame = canvas::Frame {
            doc: &self.session.editor.doc,
            selection: &self.session.selection,
            preview: &preview,
            snap: self.snap_hit,
            show_grid: self.show_grid,
        };
        canvas::paint(&ui.painter_at(rect), rect, &self.view, &frame);
    }
}

impl eframe::App for CadektorApp {
    #[cfg(not(target_arch = "wasm32"))]
    fn on_exit(&mut self) {
        // Stops the agent server and deletes its runtime file.
        self.agent = None;
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(agent) = &self.agent {
            let mut fx = file_io::AgentEffects::default();
            agent.poll(&mut |line| file_io::agent_line(&mut self.session, &mut self.path, line, &mut fx));
            if fx.replaced {
                self.machine.cancel();
                self.needs_fit = true;
            }
            self.title_dirty |= fx.path_changed;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if std::mem::take(&mut self.title_dirty) {
            self.set_title(&ctx);
        }
        self.entry.sync(self.machine.tool(), self.machine.anchor().is_some());
        self.handle_keys(&ctx);

        let (can_undo, can_redo) = (self.session.editor.can_undo(), self.session.editor.can_redo());
        let mut actions = Vec::new();
        egui::Panel::top("menu").show(ui, |ui| {
            actions = panels::menu_bar(ui, &self.keys, can_undo, can_redo, &mut self.snap_cfg, &mut self.show_grid);
        });
        for a in actions {
            self.apply_menu(&ctx, a);
        }
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let d = &self.session.editor.doc;
                ui.label(&self.status);
                ui.separator();
                ui.label(self.machine.hint());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!(
                        "{} objects, {} selected, zoom {:.3} px/mm, 1:{}",
                        d.objects.len(),
                        self.session.selection.len(),
                        self.view.zoom,
                        d.layer(d.active_layer).map_or(100.0, |l| l.scale)
                    ));
                });
            });
        });
        let mut commit = false;
        egui::Panel::bottom("databar").show(ui, |ui| {
            commit = panels::data_bar(ui, &self.machine, &mut self.settings, &mut self.entry, self.live_cursor, self.snap_hit, self.snap_cfg);
        });
        if commit {
            self.commit_entry(&ctx);
        }
        egui::Panel::left("tools").resizable(false).exact_size(170.0).show(ui, |ui| panels::tool_panel(ui, &mut self.machine, &self.keys));
        egui::Panel::right("palettes").default_size(280.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                panels::object_info(ui, &mut self.session, &mut self.info, &mut self.status);
                ui.add_space(12.0);
                panels::navigation(ui, &mut self.session, &mut self.nav, &mut self.status);
            });
        });
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key press followed by its release, so the next press is not seen as a repeat.
    fn key(k: Key, mods: egui::Modifiers) -> Vec<egui::Event> {
        let ev = |pressed| egui::Event::Key { key: k, physical_key: Some(k), pressed, repeat: false, modifiers: mods };
        vec![ev(true), ev(false)]
    }

    /// Runs one frame of the key handling and the Data bar, like `ui` does.
    fn frame(app: &mut CadektorApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        let input = egui::RawInput { events, ..Default::default() };
        let _ = ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            app.entry.sync(app.machine.tool(), app.machine.anchor().is_some());
            app.handle_keys(&ctx);
            egui::Panel::bottom("databar").show(ui, |ui| {
                panels::data_bar(ui, &app.machine, &mut app.settings, &mut app.entry, app.live_cursor, None, app.snap_cfg);
            });
        });
    }

    #[test]
    fn tab_enters_data_bar_and_esc_keeps_the_drawing() {
        let ctx = egui::Context::default();
        let mut app = CadektorApp::blank();
        let none = egui::Modifiers::NONE;
        app.machine.set_tool(Tool::Line);
        app.send(Input::Press { p: DVec2::ZERO, shift: false });
        assert!(app.machine.is_busy());
        frame(&mut app, &ctx, vec![]);
        // Tab moves into the fields.
        frame(&mut app, &ctx, key(Key::Tab, none));
        frame(&mut app, &ctx, vec![]);
        assert!(app.entry.active);
        assert_eq!(app.entry.focus, 0);
        // Tab and Shift+Tab cycle.
        frame(&mut app, &ctx, key(Key::Tab, none));
        frame(&mut app, &ctx, vec![]);
        assert_eq!(app.entry.focus, 1);
        frame(&mut app, &ctx, key(Key::Tab, egui::Modifiers::SHIFT));
        frame(&mut app, &ctx, vec![]);
        assert_eq!(app.entry.focus, 0);
        // A single-key shortcut does not fire while a field has focus.
        frame(&mut app, &ctx, key(Key::Num4, none));
        assert_eq!(app.machine.tool(), Tool::Line);
        // Esc leaves the fields and keeps the drawing.
        frame(&mut app, &ctx, key(Key::Escape, none));
        frame(&mut app, &ctx, vec![]);
        assert!(app.machine.is_busy(), "Esc in a Data bar field must not cancel the drawing");
        assert!(!app.entry.active);
        // A second Esc, now on the canvas, cancels.
        frame(&mut app, &ctx, key(Key::Escape, none));
        assert!(!app.machine.is_busy());
    }

    #[test]
    fn single_keys_work_on_the_canvas() {
        let ctx = egui::Context::default();
        let mut app = CadektorApp::blank();
        frame(&mut app, &ctx, key(Key::Num4, egui::Modifiers::NONE));
        assert_eq!(app.machine.tool(), Tool::Rect);
    }

    #[test]
    fn backspace_takes_back_the_last_point() {
        let ctx = egui::Context::default();
        let mut app = CadektorApp::blank();
        app.machine.set_tool(Tool::Polyline);
        app.send(Input::Press { p: DVec2::ZERO, shift: false });
        app.send(Input::Press { p: DVec2::X, shift: false });
        frame(&mut app, &ctx, key(Key::Backspace, egui::Modifiers::NONE));
        assert_eq!(app.machine.anchor(), Some(DVec2::ZERO));
        frame(&mut app, &ctx, key(Key::Backspace, egui::Modifiers::NONE));
        assert!(!app.machine.is_busy());
    }
}
