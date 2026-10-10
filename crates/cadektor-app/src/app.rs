//! The application: one `Session`, a view, the active tool and the panels.

use cadektor_api::Session;
use cadektor_core::command::Command;
use cadektor_core::model::{Document, ObjectId};
use cadektor_render::{RenderMode, StandardView};
use egui::{Key, Sense};
use glam::DVec2;
use serde_json::Value;

use crate::canvas;
use crate::databar::{self, DataEntry};
use crate::file_io;
use crate::keymap::{self, Action, Keymap};
use crate::panels::{self, InfoEdit, MenuAction, NavState};
use crate::plant_ui::{self, PlantUi};
use crate::snap::{self, SnapConfig, SnapResult};
use crate::tools::{Ctx, DimToolMode, Effect, Input, Tool, ToolMachine, ToolSettings, VertexMode, has_vertex_modes};
use crate::canvas3d;
use crate::gpu3d::Gpu3d;
use crate::view::{View, grid_step};
use crate::view3d::{self, View3d};

/// Pan step of an arrow key, in px.
const PAN_STEP_PX: f32 = 60.0;

/// Snap and pick distance in screen px.
const PICK_PX: f64 = 8.0;

pub struct CadektorApp {
    session: Session,
    view: View,
    needs_fit: bool,
    /// Camera of the 3D view. What it shows is chosen in `session.view`.
    view3d: View3d,
    /// The 3D renderer is available (the app runs on wgpu).
    gpu_ready: bool,
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
    plant_ui: PlantUi,
    keys: Keymap,
    entry: DataEntry,
    /// Cursor before Data bar values are applied.
    live_cursor: DVec2,
    /// View changes from keys; applied in the next canvas pass.
    pending_zoom: f64,
    pending_pan: egui::Vec2,
    /// The area of the canvas in the last frame.
    canvas_rect: Option<egui::Rect>,
    #[cfg(not(target_arch = "wasm32"))]
    agent: Option<crate::agent::AgentServer>,
    /// The File > Import and File > Export dialogs.
    #[cfg(not(target_arch = "wasm32"))]
    io_ui: crate::io_dialog::IoUi,
}

impl CadektorApp {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(cc: &eframe::CreationContext<'_>, agent_port: Option<u16>, allow_dirs: Vec<std::path::PathBuf>) -> Self {
        let mut app = Self::blank();
        // Agents may only use these folders and the folder of the open document. Dialogs of the user are not limited.
        app.session.paths = cadektor_api::PathPolicy::restricted(allow_dirs);
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            let gpu = Gpu3d::new(&rs.device, rs.target_format);
            rs.renderer.write().callback_resources.insert(gpu);
            app.gpu_ready = true;
        }
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
            view3d: View3d::default(),
            gpu_ready: false,
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
            plant_ui: PlantUi::default(),
            keys: Keymap::defaults(),
            entry: DataEntry::default(),
            live_cursor: DVec2::ZERO,
            pending_zoom: 1.0,
            pending_pan: egui::Vec2::ZERO,
            canvas_rect: None,
            #[cfg(not(target_arch = "wasm32"))]
            agent: None,
            #[cfg(not(target_arch = "wasm32"))]
            io_ui: Default::default(),
        }
    }

    fn exec(&mut self, cmd: &Command) {
        panels::run_command(&mut self.session, cmd, &mut self.status);
        crate::annot_ui::set_revision(self.session.editor.revision());
    }

    fn replace_document(&mut self, doc: Document, path: Option<std::path::PathBuf>) {
        self.session = Session::with_document(doc);
        self.path = path;
        self.title_dirty = true;
        self.machine.cancel();
        self.needs_fit = true;
        self.view3d.request_fit();
    }

    /// True while the 2D plan canvas shows, not the 3D view.
    fn plan(&self) -> bool {
        view3d::is_plan(self.session.view)
    }

    fn zoom_fit(&mut self) {
        if self.plan() {
            self.needs_fit = true;
        } else {
            self.view3d.request_fit();
        }
    }

    /// Shows a standard view. Top/Plan is the 2D drawing; the others are the 3D view.
    fn set_standard_view(&mut self, view: StandardView) {
        self.session.view.view = view;
        if !view.is_plan() {
            self.machine.cancel();
            if !self.gpu_ready {
                self.status = "The 3D view needs the wgpu renderer".into();
            }
        }
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

    fn create_site_model(&mut self) {
        self.status = match crate::site_ui::create_site_model(&mut self.session) {
            Ok(id) => format!("Created site model {id}"),
            Err(e) => format!("Error: {e}"),
        };
    }

    fn update_site_model(&mut self) {
        self.status = crate::site_ui::update_site_models(&mut self.session);
    }

    fn select_all(&mut self) {
        let doc = &self.session.editor.doc();
        self.session.selection = doc.objects.values().filter(|o| doc.is_pickable(o)).map(|o| o.id).collect();
    }

    fn apply_menu(&mut self, ctx: &egui::Context, action: MenuAction) {
        match action {
            MenuAction::New => self.replace_document(Document::default(), None),
            MenuAction::Undo => self.undo(),
            MenuAction::Redo => self.redo(),
            MenuAction::Delete => self.delete_selection(),
            MenuAction::SelectAll => self.select_all(),
            MenuAction::ZoomFit => self.zoom_fit(),
            MenuAction::CreateSiteModel => self.create_site_model(),
            MenuAction::UpdateSiteModel => self.update_site_model(),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::Open => self.open_file(ctx),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::Save => self.save(false, ctx),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::SaveAs => self.save(true, ctx),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::ExportSvg => self.export_svg(),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::ExportPdf => self.export_pdf(),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::Import => self.io_ui.start_import(),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::Export => self.io_ui.start_export(),
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
        match file_io::write_document(&path, self.session.editor.doc()) {
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
        self.status = match file_io::write_svg(&p, self.session.editor.doc()) {
            Ok(()) => format!("Exported {}", p.display()),
            Err(e) => e,
        };
    }

    /// File > Export PDF: all sheet layers, one page each.
    #[cfg(not(target_arch = "wasm32"))]
    fn export_pdf(&mut self) {
        if self.session.editor.doc().sheet_layers().next().is_none() {
            self.status = "Add a sheet layer first (Navigation > Sheet Layers > +)".into();
            return;
        }
        let Some(p) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name("drawing.pdf").save_file() else { return };
        self.status = match self.session.export_file_trusted(serde_json::json!({"path": p.display().to_string(), "format": "pdf", "overwrite": true})) {
            Ok(r) => format!("Exported {} sheet(s) to {}", r["pages"], p.display()),
            Err(e) => format!("Error: {}", e.message),
        };
    }

    /// Saved view requests from the palette, and views that an agent or the palette restored.
    fn sync_saved_views(&mut self, rect: Option<egui::Rect>) {
        if let Some(name) = self.nav.sheets.save_request.take() {
            let center = rect.map_or(DVec2::ZERO, |r| self.view.to_model(r.center()));
            let view = crate::sheets_ui::capture(&self.session, &name, self.view.zoom, center);
            self.exec(&Command::SaveView { view: Box::new(view) });
        }
        if let Some(v) = self.session.take_applied_view() {
            if let (Some(zoom), Some(pan)) = (v.zoom, v.pan)
                && let Some(r) = rect
            {
                self.view.zoom = zoom.clamp(crate::view::MIN_ZOOM, crate::view::MAX_ZOOM);
                let c = r.center();
                self.view.origin = DVec2::new(c.x as f64 - pan.x * self.view.zoom, c.y as f64 + pan.y * self.view.zoom);
            }
            self.status = format!("Restored view '{}'", v.name);
        }
    }

    /// Runs what the buttons of the Annotation part of the tool panel asked for.
    fn run_annot_requests(&mut self) {
        for cmd in std::mem::take(&mut self.settings.annot.pending) {
            self.exec(&cmd);
        }
        if std::mem::take(&mut self.settings.annot.legend_requested) {
            self.add_legend();
        }
    }

    /// Adds a graphic legend at the top left of the view.
    fn add_legend(&mut self) {
        let at = self.canvas_rect.map_or(DVec2::ZERO, |r| self.view.to_model(r.left_top() + egui::vec2(40.0, 40.0)));
        self.exec(&crate::annot_ui::legend_command(at));
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
        // The dimension keys pick the mode too: N is a linear dimension, `.` angular and `,` radial.
        let linear = |m: crate::tools::DimToolMode| matches!(m, DimToolMode::Horizontal | DimToolMode::Vertical | DimToolMode::Aligned | DimToolMode::Chain | DimToolMode::Baseline | DimToolMode::Ordinate);
        match action {
            Action::ToolDimension if !linear(self.settings.annot.dim_mode) => self.settings.annot.dim_mode = DimToolMode::Horizontal,
            Action::ToolAngularDim => self.settings.annot.dim_mode = DimToolMode::Angular,
            Action::ToolRadialDim => self.settings.annot.dim_mode = DimToolMode::Radial,
            Action::CmdGraphicLegend => {
                self.add_legend();
                return;
            }
            _ => {}
        }
        if let Some(t) = action.tool() {
            // Drawing happens on the plan. Choosing a drawing tool in 3D goes back to it.
            if t != Tool::Select && !self.plan() {
                self.set_standard_view(StandardView::Plan);
            }
            self.machine.set_tool(t);
            return;
        }
        if let Some(v) = action.standard_view() {
            self.set_standard_view(v);
            return;
        }
        match action {
            Action::FileNew => self.apply_menu(ctx, MenuAction::New),
            Action::FileOpen => self.apply_menu(ctx, MenuAction::Open),
            Action::FileSave => self.apply_menu(ctx, MenuAction::Save),
            Action::FileSaveAs => self.apply_menu(ctx, MenuAction::SaveAs),
            Action::FileExportPdf => self.apply_menu(ctx, MenuAction::ExportPdf),
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
            Action::ToolMode1 => self.mode_group(1),
            Action::ToolMode2 => self.mode_group(2),
            Action::ToolMode3 => self.mode_group(3),
            Action::ToolMode4 => self.mode_group(4),
            Action::ToolMode5 => self.mode_group(5),
            Action::ToolMode6 => self.mode_group(6),
            Action::NudgeLeft => self.nudge(DVec2::NEG_X),
            Action::NudgeRight => self.nudge(DVec2::X),
            Action::NudgeUp => self.nudge(DVec2::Y),
            Action::NudgeDown => self.nudge(DVec2::NEG_Y),
            Action::ZoomFit => self.zoom_fit(),
            Action::ViewFlyover => {
                if self.plan() {
                    self.set_standard_view(StandardView::RightIsometric);
                }
                self.status = "Flyover: drag to orbit, Shift-drag or middle-drag to pan, wheel to zoom".into();
            }
            Action::ViewProjection => {
                self.session.view.projection = self.session.view.projection.toggled();
            }
            Action::ViewWireframe => self.session.view.mode = RenderMode::Wireframe,
            Action::ViewShaded => self.session.view.mode = RenderMode::Shaded,
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
        let events: Vec<(Key, Option<Key>, egui::Modifiers, bool)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, physical_key, pressed: true, repeat, modifiers } => Some((*key, *physical_key, *modifiers, *repeat)),
                    _ => None,
                })
                .collect()
        });
        for (key, physical, mods, repeat) in events {
            let Some((action, chord)) = self.keys.action_for_event(key, physical, mods) else { continue };
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

    /// True when the Data bar has the arc Radius field: a path tool in Arc vertex mode.
    fn arc_fields(&self) -> bool {
        has_vertex_modes(self.machine.tool()) && self.settings.vertex_mode == VertexMode::Arc
    }

    /// A valid radius typed in the Data bar becomes the default radius of new arc vertices.
    /// It stays after the point is placed.
    fn apply_typed_radius(&mut self) {
        let anchored = self.machine.anchor().is_some();
        if let Some(r) = databar::typed_arc_radius(self.machine.tool(), anchored, self.arc_fields(), &self.entry.values()) {
            self.settings.arc_radius = r;
        }
    }

    /// Keys U, I, O, P, [ and ]: cycle the modes of a group of mode buttons. The path tools
    /// have one group (the vertex modes), so only group 1 does anything for them.
    fn mode_group(&mut self, group: usize) {
        let tool = self.machine.tool();
        if tool == Tool::Dimension && group == 1 {
            self.settings.annot.dim_mode = self.settings.annot.dim_mode.next();
            self.machine.cancel();
            self.status = format!("Dimension mode: {}", self.settings.annot.dim_mode.label());
        } else if tool == Tool::Plant && group == 1 {
            self.settings.plant.mode = self.settings.plant.mode.next();
            self.machine.cancel();
            self.status = format!("Placement mode: {}", self.settings.plant.mode.label());
        } else if !has_vertex_modes(tool) {
            self.status = format!("The {} tool has no mode buttons", tool.name());
        } else if group == 1 {
            self.settings.vertex_mode = self.settings.vertex_mode.next();
            self.status = format!("Vertex mode: {}", self.settings.vertex_mode.label());
        } else {
            self.status = format!("The {} tool has only one group of mode buttons", tool.name());
        }
    }

    /// Enter in a Data bar field: click at the typed point, as if the user had clicked there.
    fn commit_entry(&mut self, ctx: &egui::Context) {
        let target = databar::target_point(self.machine.tool(), self.machine.anchor(), self.live_cursor, &self.entry.values(), self.arc_fields());
        let Some(p) = target else { return };
        self.apply_typed_radius();
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
            let cx = Ctx { doc: self.session.editor.doc(), selection: &self.session.selection, settings: &self.settings, tol: self.tol() };
            self.machine.handle(input, &cx)
        };
        for e in effects {
            match e {
                Effect::SetSelection(s) => self.session.selection = s,
                Effect::Notice(m) => self.status = m,
                Effect::Exec(cmd) => {
                    let is_add = matches!(cmd, Command::AddObject { .. } | Command::AddHardscape { .. } | Command::AddLandscapeArea { .. });
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
        if let Some(page) = crate::sheets_ui::active_sheet(&self.session).and_then(|l| self.session.editor.doc().sheet(l)) {
            self.view.fit(DVec2::ZERO, page.size_mm(), rect);
            return;
        }
        let (lo, hi) = cadektor_core::svg::drawing_bounds(self.session.editor.doc())
            .unwrap_or((DVec2::new(-1000.0, -1000.0), DVec2::new(19000.0, 14000.0)));
        self.view.fit(lo, hi, rect);
    }


    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        crate::annot_ui::set_revision(self.session.editor.revision());
        // A sheet layer shows its page. Changing the active sheet fits the page to the window.
        let sheet = crate::sheets_ui::active_sheet(&self.session);
        if sheet != self.nav.sheets.last_sheet {
            self.nav.sheets.last_sheet = sheet;
            self.needs_fit = true;
        }
        let page = sheet.and_then(|l| self.session.sheet_page(l).ok());
        self.canvas_rect = Some(rect);
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
                let pts = snap::snap_points(self.session.editor.doc(), &busy_ids);
                snap::snap(raw, self.tol(), grid_step(self.view.zoom, 12.0), self.snap_cfg, &pts)
            };
            self.cursor = self.snap_hit.map_or(raw, |s| s.point);
            self.live_cursor = self.cursor;
            // Typed Data bar values override the mouse.
            if self.entry.any_typed()
                && let Some(p) = databar::target_point(self.machine.tool(), self.machine.anchor(), self.live_cursor, &self.entry.values(), self.arc_fields())
            {
                self.cursor = p;
            }
        } else {
            self.snap_hit = None;
        }

        self.apply_typed_radius();

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

        let cx = Ctx { doc: self.session.editor.doc(), selection: &self.session.selection, settings: &self.settings, tol: self.tol() };
        let preview = if resp.hovered() || self.machine.is_busy() { self.machine.preview(self.cursor, &cx) } else { crate::tools::Preview::None };
        let frame = canvas::Frame {
            doc: self.session.editor.doc(),
            selection: &self.session.selection,
            preview: &preview,
            snap: self.snap_hit,
            show_grid: self.show_grid,
            site: Some((self.session.site_cache(), self.session.editor.revision())),
            sheet: page.as_deref().map(|p| (&self.nav.sheets, p)),
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
            self.session.doc_dir = self.path.as_ref().and_then(|p| p.parent()).map(std::path::Path::to_path_buf);
            agent.poll(&mut |line| file_io::agent_line(&mut self.session, &mut self.path, line, &mut fx));
            if fx.replaced {
                self.machine.cancel();
                self.needs_fit = true;
                self.view3d.request_fit();
            }
            self.title_dirty |= fx.path_changed;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if std::mem::take(&mut self.title_dirty) {
            self.set_title(&ctx);
        }
        self.entry.sync(self.machine.tool(), self.machine.anchor().is_some(), self.arc_fields());
        self.handle_keys(&ctx);
        // The camera follows the chosen view, whoever chose it: a key, the view bar or an agent.
        let forced = self.session.take_view_changed();
        self.view3d.sync(self.session.view, forced);

        let (can_undo, can_redo) = (self.session.editor.can_undo(), self.session.editor.can_redo());
        let mut actions = Vec::new();
        egui::Panel::top("menu").show(ui, |ui| {
            let can_site = !crate::site_ui::source_ids(self.session.editor.doc(), &self.session.selection).is_empty();
            let can_update = !crate::site_ui::selected_site_models(self.session.editor.doc(), &self.session.selection).is_empty();
            actions = panels::menu_bar(ui, &self.keys, can_undo, can_redo, &mut self.snap_cfg, &mut self.show_grid, can_site, can_update);
        });
        for a in actions {
            self.apply_menu(&ctx, a);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.io_ui.show(&ctx, &mut self.session, &mut self.status) == crate::io_dialog::IoEvent::Imported {
            self.needs_fit = true;
        }
        let before = self.session.view.view;
        egui::Panel::top("viewbar").show(ui, |ui| panels::view_bar(ui, &mut self.session.view, &self.keys, self.gpu_ready));
        if self.session.view.view != before {
            self.set_standard_view(self.session.view.view);
        }
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let d = &self.session.editor.doc();
                ui.label(&self.status);
                ui.separator();
                ui.label(self.machine.hint_for(&self.settings));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.plan() {
                        ui.label(format!(
                            "{} objects, {} selected, zoom {:.3} px/mm, 1:{}",
                            d.objects.len(),
                            self.session.selection.len(),
                            self.view.zoom,
                            d.layer(d.active_layer).map_or(100.0, |l| l.scale)
                        ));
                    } else {
                        let v = self.session.view;
                        ui.label(format!("{} objects, {} selected, {}, {}", d.objects.len(), self.session.selection.len(), v.view.label(), v.projection.id()));
                    }
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
        egui::Panel::left("tools").resizable(false).exact_size(170.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| panels::tool_panel(ui, &mut self.machine, &self.keys, &mut self.settings, self.session.editor.doc()));
        });
        self.run_annot_requests();
        egui::Panel::right("palettes").default_size(280.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                panels::object_info(ui, &mut self.session, &mut self.info, &mut self.status);
                ui.add_space(12.0);
                panels::navigation(ui, &mut self.session, &mut self.nav, &mut self.status);
                ui.add_space(12.0);
                plant_ui::resource_manager(ui, &mut self.session, &mut self.plant_ui, &mut self.status);
                crate::data_ui::resource_sections(ui, &mut self.session, &mut self.status);
                ui.add_space(12.0);
                plant_ui::plant_schedule(ui, &mut self.session, &mut self.status);
            });
        });
        crate::data_ui::windows(&ctx, &mut self.session, &mut self.status);
        let rect = self.canvas_rect;
        self.sync_saved_views(rect);
        egui::CentralPanel::no_frame().show(ui, |ui| {
            if self.plan() {
                self.canvas(ui);
            } else {
                canvas3d::show(
                    ui,
                    canvas3d::Canvas3d {
                        session: &mut self.session,
                        view3d: &mut self.view3d,
                        keys: &self.keys,
                        gpu_ready: self.gpu_ready,
                        pending_zoom: &mut self.pending_zoom,
                        pending_pan: &mut self.pending_pan,
                    },
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_render::Projection;

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
            app.entry.sync(app.machine.tool(), app.machine.anchor().is_some(), app.arc_fields());
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

    #[test]
    fn mode_group_keys_cycle_the_vertex_modes() {
        let ctx = egui::Context::default();
        let mut app = CadektorApp::blank();
        let none = egui::Modifiers::NONE;
        // Without a path tool the keys only say that there are no modes.
        frame(&mut app, &ctx, key(Key::U, none));
        assert_eq!(app.settings.vertex_mode, VertexMode::Corner);
        assert!(app.status.contains("no mode buttons"), "{}", app.status);
        app.machine.set_tool(Tool::Hardscape);
        let mut seen = Vec::new();
        for _ in 0..6 {
            frame(&mut app, &ctx, key(Key::U, none));
            seen.push(app.settings.vertex_mode);
        }
        assert_eq!(
            seen,
            [VertexMode::Bezier, VertexMode::Cubic, VertexMode::TangentArc, VertexMode::PointOnArc, VertexMode::Arc, VertexMode::Corner]
        );
        assert_eq!(app.status, "Vertex mode: Corner");
        // The other groups have no buttons for these tools. They do not change the mode.
        for k in [Key::I, Key::O, Key::P, Key::OpenBracket, Key::CloseBracket] {
            frame(&mut app, &ctx, key(k, none));
            assert_eq!(app.settings.vertex_mode, VertexMode::Corner);
            assert!(app.status.contains("only one group"), "{}", app.status);
        }
        // The tool keys still work: P is a mode key, 5 selects the Polyline tool.
        frame(&mut app, &ctx, key(Key::Num5, none));
        assert_eq!(app.machine.tool(), Tool::Polyline);
    }

    #[test]
    fn arc_mode_gives_a_radius_field_and_the_value_persists() {
        let ctx = egui::Context::default();
        let mut app = CadektorApp::blank();
        let none = egui::Modifiers::NONE;
        app.machine.set_tool(Tool::Polyline);
        // Corner mode: only X and Y.
        frame(&mut app, &ctx, vec![]);
        assert_eq!(app.entry.text.len(), 2);
        app.settings.vertex_mode = VertexMode::Arc;
        frame(&mut app, &ctx, vec![]);
        assert_eq!(app.entry.text.len(), 3);
        // Tab goes through X, Y and Radius.
        frame(&mut app, &ctx, key(Key::Tab, none));
        frame(&mut app, &ctx, key(Key::Tab, none));
        frame(&mut app, &ctx, key(Key::Tab, none));
        frame(&mut app, &ctx, vec![]);
        assert!(app.entry.active);
        assert_eq!(app.entry.focus, 2);
        // A typed radius becomes the default for the next arc vertices.
        app.entry.text[2] = "750".into();
        app.apply_typed_radius();
        assert_eq!(app.settings.arc_radius, 750.0);
        // Bad or negative text keeps the old value.
        app.entry.text[2] = "-5".into();
        app.apply_typed_radius();
        app.entry.text[2] = "abc".into();
        app.apply_typed_radius();
        assert_eq!(app.settings.arc_radius, 750.0);
        // Enter places the first point with X and Y typed, and uses the radius.
        app.entry.text = vec!["100".into(), "200".into(), "300".into()];
        app.commit_entry(&ctx);
        assert_eq!(app.machine.anchor(), Some(DVec2::new(100.0, 200.0)));
        assert_eq!(app.settings.arc_radius, 300.0);
        frame(&mut app, &ctx, vec![]);
        // After the first click the fields are Length, Angle and Radius.
        assert_eq!(app.entry.text.len(), 3);
        app.entry.text = vec!["100".into(), "0".into(), "50".into()];
        app.commit_entry(&ctx);
        assert_eq!(app.machine.anchor(), Some(DVec2::new(200.0, 200.0)));
        assert_eq!(app.settings.arc_radius, 50.0);
        // The radius is kept when the fields are empty again.
        frame(&mut app, &ctx, vec![]);
        assert!(!app.entry.any_typed());
        assert_eq!(app.settings.arc_radius, 50.0);
        app.send(Input::Press { p: DVec2::new(200.0, 300.0), shift: false });
        app.send(Input::Enter);
        let o = app.session.editor.doc().objects.values().next().unwrap();
        let vs = o.shape.vertices().unwrap();
        assert_eq!(vs[1].kind, cadektor_core::model::VertexKind::Arc { radius: 50.0 });
    }

    /// A key press the way a real keyboard reports it: the logical key can differ from the physical one.
    fn real_key(logical: Key, physical: Key, mods: egui::Modifiers) -> Vec<egui::Event> {
        let ev = |pressed| egui::Event::Key { key: logical, physical_key: Some(physical), pressed, repeat: false, modifiers: mods };
        vec![ev(true), ev(false)]
    }

    #[test]
    fn standard_view_keys_switch_between_plan_and_3d() {
        let ctx = egui::Context::default();
        let mut app = CadektorApp::blank();
        let m = egui::Modifiers { alt: true, shift: true, ..Default::default() };
        assert_eq!(app.session.view.view, StandardView::Plan);
        assert!(app.plan());
        // Vectorworks numbering. The logical key is what a US keyboard types with Shift held;
        // the physical key is the digit.
        for (digit, logical, view) in [
            (Key::Num2, Key::Num2, StandardView::Front),
            (Key::Num8, Key::Num8, StandardView::Back),
            (Key::Num4, Key::Num4, StandardView::Left),
            (Key::Num6, Key::Num6, StandardView::Right),
            (Key::Num1, Key::Exclamationmark, StandardView::LeftIsometric),
            (Key::Num3, Key::Num3, StandardView::RightIsometric),
            (Key::Num7, Key::Num7, StandardView::LeftRearIsometric),
            (Key::Num9, Key::Num9, StandardView::RightRearIsometric),
            (Key::Num5, Key::Num5, StandardView::Top),
            (Key::Num0, Key::Num0, StandardView::Plan),
        ] {
            frame(&mut app, &ctx, real_key(logical, digit, m));
            assert_eq!(app.session.view.view, view, "{digit:?} ({logical:?})");
            assert_eq!(app.plan(), view == StandardView::Plan);
        }
        // Cmd/Ctrl+5 is Top/Plan.
        app.set_standard_view(StandardView::Front);
        frame(&mut app, &ctx, key(Key::Num5, egui::Modifiers::COMMAND));
        assert!(app.plan());
        // Projection and render mode.
        frame(&mut app, &ctx, key(Key::J, m));
        assert_eq!(app.session.view.projection, Projection::Perspective);
        frame(&mut app, &ctx, key(Key::J, m));
        assert_eq!(app.session.view.projection, Projection::Orthographic);
        let cs = egui::Modifiers { shift: true, ..egui::Modifiers::COMMAND };
        frame(&mut app, &ctx, key(Key::W, cs));
        assert_eq!(app.session.view.mode, RenderMode::Wireframe);
        frame(&mut app, &ctx, key(Key::G, cs));
        assert_eq!(app.session.view.mode, RenderMode::Shaded);
        // Flyover (Shift+C) leaves the plan for an isometric view. In 3D it only says how to navigate.
        frame(&mut app, &ctx, key(Key::C, egui::Modifiers::SHIFT));
        assert_eq!(app.session.view.view, StandardView::RightIsometric);
        assert!(app.status.contains("orbit"), "{}", app.status);
        frame(&mut app, &ctx, real_key(Key::Num2, Key::Num2, m));
        frame(&mut app, &ctx, key(Key::C, egui::Modifiers::SHIFT));
        assert_eq!(app.session.view.view, StandardView::Front);
        // Choosing a drawing tool goes back to the plan; the Selection tool stays in 3D.
        frame(&mut app, &ctx, key(Key::X, egui::Modifiers::NONE));
        assert_eq!(app.session.view.view, StandardView::Front);
        frame(&mut app, &ctx, key(Key::Num4, egui::Modifiers::NONE));
        assert_eq!(app.machine.tool(), Tool::Rect);
        assert!(app.plan());
        // Zoom to fit asks the right canvas.
        app.needs_fit = false;
        app.zoom_fit();
        assert!(app.needs_fit);
        app.set_standard_view(StandardView::Front);
        app.view3d.fit(None, 1.0);
        app.zoom_fit();
        assert!(app.view3d.fit_pending());
    }

    #[test]
    fn view_keys_do_not_fire_while_typing() {
        let ctx = egui::Context::default();
        let mut app = CadektorApp::blank();
        app.machine.set_tool(Tool::Line);
        app.send(Input::Press { p: DVec2::ZERO, shift: false });
        frame(&mut app, &ctx, key(Key::Tab, egui::Modifiers::NONE));
        frame(&mut app, &ctx, vec![]);
        assert!(app.entry.active);
        frame(&mut app, &ctx, key(Key::C, egui::Modifiers::SHIFT));
        assert!(app.plan(), "Shift+C types a letter in a field");
    }

    #[test]
    fn a_3d_view_without_a_gpu_says_so() {
        let mut app = CadektorApp::blank();
        app.set_standard_view(StandardView::Front);
        assert!(app.status.contains("wgpu"));
        assert!(!app.plan());
    }

    #[test]
    fn agent_view_set_moves_the_camera() {
        let mut app = CadektorApp::blank();
        app.session.handle("view.set", serde_json::json!({"view": "back", "projection": "perspective"})).unwrap();
        let forced = app.session.take_view_changed();
        assert!(forced);
        app.view3d.sync(app.session.view, forced);
        assert!(!app.plan());
        assert!((app.view3d.camera.eye_dir() - glam::Vec3::Y).length() < 1e-5);
        assert_eq!(app.view3d.camera.projection, Projection::Perspective);
        app.session.handle("view.set", serde_json::json!({"view": "plan"})).unwrap();
        assert!(app.session.take_view_changed());
        assert!(app.plan());
    }
}
