//! The application: one `Session`, a view, the active tool and the panels.

use cadektor_api::Session;
use cadektor_core::command::Command;
use cadektor_core::model::{Document, ObjectId, Shape};
use cadektor_render::{RenderMode, StandardView};
use egui::{Key, Sense};
use glam::DVec2;
use serde_json::Value;

use crate::array_ui::ArrayDialog;
use crate::canvas;
use crate::canvas3d;
use crate::databar::{self, DataEntry};
use crate::file_io;
use crate::gpu3d::Gpu3d;
use crate::keymap::{self, Action, Keymap};
use crate::layout::{self, Layouts, Tab};
use crate::panels::{self, InfoEdit, MenuAction, MenuExtras, MenuRequest, NavState};
use crate::plant_ui::{self, PlantUi};
use crate::prefs::{self, Prefs};
use crate::quick_search::{QuickCommand, QuickItem, QuickSearch};
use crate::resource_ui::{self, ResourceUi};
use crate::screenshot;
use crate::snap::{self, SnapResult};
use crate::theme::{self, ThemeChoice};
use crate::tools::{
    Ctx, DimToolMode, Effect, Input, Tool, ToolMachine, ToolSettings, VertexMode, has_vertex_modes,
};
use crate::view::{View, grid_step};
use crate::view_history::{self, ViewHistory};
use crate::view3d::{self, View3d};
use cadektor_core::edit2d::Transform;
use cadektor_core::edit2d::plan::{self, SurfaceOp};

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
    show_grid: bool,
    cursor: DVec2,
    snap_hit: Option<SnapResult>,
    /// The preferences of the window and the copy that was saved last.
    prefs: Prefs,
    prefs_saved: Prefs,
    theme_applied: Option<ThemeChoice>,
    /// The panels and the canvas use the dark theme.
    dark: bool,
    layouts: Layouts,
    /// A name being typed for Window > Save Layout As.
    layout_name: Option<String>,
    /// Time of the last check whether the layout changed (seconds).
    layout_checked: f64,
    quick: QuickSearch,
    array_dlg: ArrayDialog,
    view_history: ViewHistory,
    /// Screenshot requests of agents that wait for a picture.
    shots: Vec<screenshot::Pending>,
    shot_key: u64,
    status: String,
    path: Option<std::path::PathBuf>,
    /// The window title must be set in the next frame.
    title_dirty: bool,
    info: InfoEdit,
    nav: NavState,
    plant_ui: PlantUi,
    resource_ui: ResourceUi,
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
    /// The File > Map Sources and File > Change CRS dialogs.
    #[cfg(not(target_arch = "wasm32"))]
    gis_ui: crate::gis_dialogs::GisDialogs,
    /// The Scripts palette, its console, and the parameters of plug-in objects.
    #[cfg(not(target_arch = "wasm32"))]
    script_ui: crate::script_ui::ScriptUi,
    /// The coordinate readout of the Data bar.
    readout: crate::gis_ui::ReadoutState,
    /// Autosave, crash recovery and the Preferences dialog.
    autosave: crate::autosave_ui::AutosaveUi,
    /// Files that the browser has picked, waiting for the next frame.
    #[cfg(target_arch = "wasm32")]
    web_inbox: std::rc::Rc<std::cell::RefCell<Vec<(String, Vec<u8>)>>>,
    /// Set when a download of the document has finished. Then the browser copy is not needed.
    #[cfg(target_arch = "wasm32")]
    web_saved: std::rc::Rc<std::cell::Cell<bool>>,
}

impl CadektorApp {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        agent_port: Option<u16>,
        allow_dirs: Vec<std::path::PathBuf>,
    ) -> Self {
        let mut app = Self::blank();
        // Agents may only use these folders and the folder of the open document. Dialogs of the user are not limited.
        let args: Vec<String> = std::env::args().collect();
        // Agents start a DWG converter only with --allow-converters (default: off). The file dialogs are not limited.
        app.session.paths = cadektor_api::PathPolicy::restricted(allow_dirs)
            .with_converters(args.iter().any(|a| a == "--allow-converters"));
        // Agents may fetch only the addresses of --allow-url (default: none). Network calls never block the UI.
        app.session.gis.urls =
            cadektor_api::UrlPolicy::allow_list(crate::agent::parse_allow_urls(&args));
        app.session.gis.blocking = false;
        // Agents run their own scripts only if the user starts the app with this flag.
        app.session.scripting.allow_agent_scripts =
            args.iter().any(|a| a == "--allow-agent-scripts");
        // The scripts of plug-in objects that the user trusted before.
        app.session.scripting.trust = cadektor_api::TrustStore::load_default();
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            let gpu = Gpu3d::new(&rs.device, &rs.queue, rs.target_format);
            rs.renderer.write().callback_resources.insert(gpu);
            app.gpu_ready = true;
        }
        let (prefs, prefs_warning) = prefs::load();
        app.prefs = prefs;
        app.prefs_saved = prefs;
        app.session.snap = prefs.snap;
        theme::apply(&cc.egui_ctx, prefs.theme);
        app.theme_applied = Some(prefs.theme);
        let (layouts, layout_warning) = Layouts::load();
        app.layouts = layouts;
        for w in prefs_warning.iter().chain(layout_warning.iter()) {
            eprintln!("cadektor: {w}");
        }
        let (keys, warnings) = Keymap::load();
        app.keys = keys;
        for w in &warnings {
            eprintln!("cadektor: {w}");
        }
        let key_note = match warnings.as_slice() {
            [] => String::new(),
            [one] => one.clone(),
            [first, rest @ ..] => {
                format!("{first} (+{} more keymap warnings, see stderr)", rest.len())
            }
        };
        app.status = key_note.clone();
        app.autosave.begin();
        if let Some(port) = agent_port {
            let ctx = cc.egui_ctx.clone();
            let info_path = cadektor_sys::agent::agent_info_path();
            match crate::agent::start(
                port,
                std::sync::Arc::new(move || ctx.request_repaint()),
                info_path,
            ) {
                Ok(server) => {
                    app.status = format!(
                        "Agent server listening on {} (token in the agent runtime file) {key_note}",
                        server.addr
                    );
                    app.agent = Some(server);
                }
                Err(e) => app.status = format!("Agent server failed on port {port}: {e}"),
            }
        }
        app
    }

    /// The start of the web build: no agent server, no files, no Python.
    #[cfg(target_arch = "wasm32")]
    pub fn new_web(cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self::blank();
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            let gpu = Gpu3d::new(&rs.device, &rs.queue, rs.target_format);
            rs.renderer.write().callback_resources.insert(gpu);
            app.gpu_ready = true;
        }
        app.status = "Web build: files open and save through the browser".into();
        app.autosave.begin();
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
            show_grid: true,
            cursor: DVec2::ZERO,
            snap_hit: None,
            prefs: Prefs::default(),
            prefs_saved: Prefs::default(),
            theme_applied: None,
            dark: false,
            layouts: Layouts::default(),
            layout_name: None,
            layout_checked: 0.0,
            quick: QuickSearch::default(),
            array_dlg: ArrayDialog::default(),
            view_history: ViewHistory::default(),
            shots: Vec::new(),
            shot_key: 0,
            status: "Ready".into(),
            path: None,
            title_dirty: false,
            info: InfoEdit::default(),
            nav: NavState::default(),
            plant_ui: PlantUi::default(),
            resource_ui: ResourceUi::default(),
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
            #[cfg(not(target_arch = "wasm32"))]
            gis_ui: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            script_ui: Default::default(),
            readout: Default::default(),
            autosave: crate::autosave_ui::AutosaveUi::inert(),
            #[cfg(target_arch = "wasm32")]
            web_inbox: Default::default(),
            #[cfg(target_arch = "wasm32")]
            web_saved: Default::default(),
        }
    }

    fn exec(&mut self, cmd: &Command) {
        panels::run_command(&mut self.session, cmd, &mut self.status);
        crate::annot_ui::set_revision(self.session.editor.revision());
    }

    fn replace_document(&mut self, doc: Document, path: Option<std::path::PathBuf>) {
        let snap = self.session.snap;
        self.session = Session::with_document(doc);
        self.session.snap = snap;
        self.view_history.clear();
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

    /// Group the selection and select the group.
    fn group_selection(&mut self) {
        if self.session.selection.is_empty() {
            return;
        }
        let ids = self.session.selection.clone();
        match self
            .session
            .editor
            .execute(&Command::Group { ids, name: None })
        {
            Ok(out) => {
                self.session.selection = out.created.into_iter().map(ObjectId).collect();
                self.status = "Grouped".into();
            }
            Err(e) => self.status = format!("Error: {e}"),
        }
    }

    fn ungroup_selection(&mut self) {
        let groups: Vec<ObjectId> = self
            .session
            .selection
            .iter()
            .copied()
            .filter(|i| self.session.editor.doc().is_group(*i))
            .collect();
        if groups.is_empty() {
            self.status = "Select a group to ungroup".into();
            return;
        }
        let members: Vec<ObjectId> = groups
            .iter()
            .flat_map(|g| self.session.editor.doc().children(*g))
            .collect();
        match self
            .session
            .editor
            .execute(&Command::Ungroup { ids: groups })
        {
            Ok(_) => {
                self.session.selection = members;
                self.status = "Ungrouped".into();
            }
            Err(e) => self.status = format!("Error: {e}"),
        }
    }

    /// Edit group: enter the selected group so that clicks select its members. A selected symbol
    /// instance is edited in place instead.
    fn enter_group(&mut self) {
        let doc = self.session.editor.doc();
        let Some(&id) = self
            .session
            .selection
            .first()
            .filter(|_| self.session.selection.len() == 1)
        else {
            self.status = "Select one group or symbol to edit".into();
            return;
        };
        if doc.is_group(id) {
            self.session.context.push(id);
            self.session.selection.clear();
            self.status = "Editing group. Exit with the Exit group key".into();
        } else if matches!(doc.object(id).map(|o| &o.shape), Some(Shape::Symbol { .. })) {
            match self.session.editor.execute(&Command::EditSymbol { id }) {
                Ok(out) => {
                    self.session.context.push(ObjectId(out.created[0]));
                    self.session.selection.clear();
                    self.status =
                        "Editing the symbol. Exit to apply the changes to all instances".into();
                }
                Err(e) => self.status = format!("Error: {e}"),
            }
        } else {
            self.status = "Select one group or symbol to edit".into();
        }
    }

    /// Exit group. Leaving the edit group of a symbol applies the changes to the definition.
    fn exit_group(&mut self) {
        let Some(g) = self.session.edit_context() else {
            self.status = "You are not in a group".into();
            return;
        };
        let editing = self
            .session
            .editor
            .doc()
            .object(g)
            .is_some_and(|o| o.data.contains_key(cadektor_core::symbols::EDIT_GROUP_KEY));
        if editing {
            if let Err(e) = self
                .session
                .editor
                .execute(&Command::ExitSymbolEdit { group: g })
            {
                self.status = format!("Error: {e}");
                return;
            }
            self.session.context.pop();
            self.session.selection.clear();
            self.status = "Symbol updated".into();
        } else {
            self.session.context.pop();
            self.session.selection = vec![g];
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
        self.session.selection = doc
            .objects
            .values()
            .filter(|o| doc.is_pickable(o))
            .map(|o| o.id)
            .collect();
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
            MenuAction::Preferences => self.autosave.open_preferences(),
            MenuAction::DuplicateArray
            | MenuAction::AddSurface
            | MenuAction::SubtractSurface
            | MenuAction::ClipSurface
            | MenuAction::IntersectSurface
            | MenuAction::Compose
            | MenuAction::RotateLeft90
            | MenuAction::RotateRight90
            | MenuAction::FlipHorizontal
            | MenuAction::FlipVertical
            | MenuAction::PreviousView
            | MenuAction::NextView
            | MenuAction::QuickSearch => {
                if let Some(a) = panels::menu_key(action) {
                    self.run_action(ctx, a);
                }
            }
            MenuAction::Open => self.open_file(ctx),
            MenuAction::Save => self.save(false, ctx),
            MenuAction::SaveAs => self.save(true, ctx),
            MenuAction::ExportSvg => self.export_svg(),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::ExportPdf => self.export_pdf(),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::Import => self.io_ui.start_import(),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::Export => self.io_ui.start_export(),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::MapSources => self.gis_ui.open_sources(),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::ChangeCrs => self.gis_ui.open_crs(&self.session),
            #[cfg(not(target_arch = "wasm32"))]
            MenuAction::Scripts => self.script_ui.open(),
            #[cfg(target_arch = "wasm32")]
            _ => {
                let _ = ctx;
            }
        }
    }

    /// Makes sure that scripts have a live session to connect to. Without `--agent-port` the
    /// app starts the agent server on a free port now, without a runtime file.
    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_script_endpoint(&mut self, ctx: &egui::Context) {
        if self.agent.is_none() {
            let wake = ctx.clone();
            match crate::agent::start(0, std::sync::Arc::new(move || wake.request_repaint()), None)
            {
                Ok(server) => self.agent = Some(server),
                Err(e) => self.status = format!("Cannot start the session for scripts: {e}"),
            }
        }
        if let Some(a) = &self.agent {
            self.session.scripting.endpoint = Some((a.addr.to_string(), a.token.clone()));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn set_title(&self, ctx: &egui::Context) {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or("Untitled".into(), |n| n.to_string_lossy().into_owned());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{name} - Cadektor")));
    }

    /// Web: the browser asks for a file. The bytes arrive in `web_inbox` and `take_web_files` reads them.
    #[cfg(target_arch = "wasm32")]
    fn open_file(&mut self, ctx: &egui::Context) {
        let (inbox, ctx) = (self.web_inbox.clone(), ctx.clone());
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(f) = rfd::AsyncFileDialog::new()
                .add_filter("Cadektor", &["cdk", "json"])
                .pick_file()
                .await
            {
                let bytes = f.read().await;
                inbox.borrow_mut().push((f.file_name(), bytes));
                ctx.request_repaint();
            }
        });
    }

    /// Web: Save always downloads a file, because a browser page has no path to write to.
    #[cfg(target_arch = "wasm32")]
    fn save(&mut self, _as_new: bool, _ctx: &egui::Context) {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or("drawing.cdk".into(), |n| n.to_string_lossy().into_owned());
        match serde_json::to_vec_pretty(self.session.editor.doc()) {
            Ok(bytes) => self.download(&name, bytes, Some(self.web_saved.clone())),
            Err(e) => self.status = e.to_string(),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn export_svg(&mut self) {
        let svg = cadektor_core::svg::render(self.session.editor.doc(), 1600);
        self.download("drawing.svg", svg.into_bytes(), None);
    }

    #[cfg(target_arch = "wasm32")]
    fn download(
        &mut self,
        name: &str,
        bytes: Vec<u8>,
        done: Option<std::rc::Rc<std::cell::Cell<bool>>>,
    ) {
        let name = name.to_string();
        self.status = format!("Downloading {name}");
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(h) = rfd::AsyncFileDialog::new()
                .set_file_name(&name)
                .save_file()
                .await
                && h.write(&bytes).await.is_ok()
                && let Some(done) = done
            {
                done.set(true);
            }
        });
    }

    /// Web: opens the files that the browser handed over.
    #[cfg(target_arch = "wasm32")]
    fn take_web_files(&mut self) {
        // The browser copy goes only after a download really worked.
        if self.web_saved.take() {
            self.autosave.saved();
        }
        let files: Vec<_> = self.web_inbox.borrow_mut().drain(..).collect();
        for (name, bytes) in files {
            let parsed = String::from_utf8(bytes)
                .map_err(|e| e.to_string())
                .and_then(|t| file_io::parse_document(&t));
            match parsed {
                Ok(doc) => {
                    self.status = format!("Opened {name}");
                    self.replace_document(doc, Some(name.into()));
                }
                Err(e) => self.status = format!("Cannot open {name}: {e}"),
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_file(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Cadektor", &["cdk", "json"])
            .pick_file()
        else {
            return;
        };
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
                let Some(p) = rfd::FileDialog::new()
                    .add_filter("Cadektor", &["cdk"])
                    .set_file_name("drawing.cdk")
                    .save_file()
                else {
                    return;
                };
                file_io::with_extension(&p)
            }
        };
        match file_io::write_document(&path, self.session.editor.doc()) {
            Ok(()) => {
                self.status = if file_io::is_legacy_name(&path) {
                    format!(
                        "Saved {}. File > Save As makes a file with the new .cdk extension.",
                        path.display()
                    )
                } else {
                    format!("Saved {}", path.display())
                };
                self.path = Some(path);
                self.set_title(ctx);
            }
            Err(e) => self.status = e,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn export_svg(&mut self) {
        let Some(p) = rfd::FileDialog::new()
            .add_filter("SVG", &["svg"])
            .set_file_name("drawing.svg")
            .save_file()
        else {
            return;
        };
        self.status = match file_io::write_svg(&p, &self.session) {
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
        let Some(p) = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_file_name("drawing.pdf")
            .save_file()
        else {
            return;
        };
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
            self.exec(&Command::SaveView {
                view: Box::new(view),
            });
        }
        if let Some(v) = self.session.take_applied_view() {
            if let (Some(zoom), Some(pan)) = (v.zoom, v.pan)
                && let Some(r) = rect
            {
                self.view.zoom = zoom.clamp(crate::view::MIN_ZOOM, crate::view::MAX_ZOOM);
                let c = r.center();
                self.view.origin = DVec2::new(
                    c.x as f64 - pan.x * self.view.zoom,
                    c.y as f64 + pan.y * self.view.zoom,
                );
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
        let at = self.canvas_rect.map_or(DVec2::ZERO, |r| {
            self.view.to_model(r.left_top() + egui::vec2(40.0, 40.0))
        });
        let cmd = self.session.in_context(crate::annot_ui::legend_command(at));
        self.exec(&cmd);
    }

    fn nudge(&mut self, dir: DVec2) {
        if self.session.selection.is_empty() {
            return;
        }
        let step = grid_step(self.view.zoom, 12.0);
        let ids = self.session.selection.clone();
        self.exec(&Command::MoveObjects {
            ids,
            delta: dir * step,
        });
    }

    fn run_action(&mut self, ctx: &egui::Context, action: Action) {
        // The dimension keys pick the mode too: N is a linear dimension, `.` angular and `,` radial.
        let linear = |m: DimToolMode| {
            matches!(
                m,
                DimToolMode::Horizontal
                    | DimToolMode::Vertical
                    | DimToolMode::Aligned
                    | DimToolMode::Chain
                    | DimToolMode::Baseline
                    | DimToolMode::Ordinate
            )
        };
        match action {
            Action::ToolDimension if !linear(self.settings.annot.dim_mode) => {
                self.settings.annot.dim_mode = DimToolMode::Horizontal
            }
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
            Action::Group => self.group_selection(),
            Action::Ungroup => self.ungroup_selection(),
            Action::EnterGroup => self.enter_group(),
            Action::ExitGroup => self.exit_group(),
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
                self.status =
                    "Flyover: drag to orbit, Shift-drag or middle-drag to pan, wheel to zoom"
                        .into();
            }
            Action::ViewProjection => {
                self.session.view.projection = self.session.view.projection.toggled();
            }
            Action::ViewWireframe => self.session.view.mode = RenderMode::Wireframe,
            Action::ViewShaded => self.session.view.mode = RenderMode::Shaded,
            Action::ViewHiddenLine => self.session.view.mode = RenderMode::HiddenLine,
            Action::ViewDashedHiddenLine => self.session.view.mode = RenderMode::DashedHiddenLine,
            Action::ViewRealistic => {
                if let Err(e) = crate::present_ui::use_builtin_style(&mut self.session, "Realistic")
                {
                    self.status = e;
                }
            }
            Action::CmdDuplicateArray => {
                if self.session.selection.is_empty() {
                    self.status = "Select the objects to duplicate first".into();
                } else {
                    self.array_dlg.open();
                }
            }
            Action::CmdAddSurface => self.surface_op(SurfaceOp::Add),
            Action::CmdSubtractSurface => self.surface_op(SurfaceOp::Subtract),
            Action::CmdClipSurface => self.surface_op(SurfaceOp::Clip),
            Action::CmdIntersectSurface => self.surface_op(SurfaceOp::Intersect),
            Action::CmdCompose => self.surface_op(SurfaceOp::Compose),
            Action::CmdRotateLeft90 => self.turn_selection(std::f64::consts::FRAC_PI_2),
            Action::CmdRotateRight90 => self.turn_selection(-std::f64::consts::FRAC_PI_2),
            Action::CmdFlipHorizontal => self.flip_selection(true),
            Action::CmdFlipVertical => self.flip_selection(false),
            Action::QuickSearch => self.quick.open(),
            Action::ViewHistoryPrevious => self.step_view_history(true),
            Action::ViewHistoryNext => self.step_view_history(false),
            Action::SnapCues => {
                self.prefs.cues = !self.prefs.cues;
                self.status = format!(
                    "SmartCursor cues {}",
                    if self.prefs.cues { "on" } else { "off" }
                );
            }
            Action::SnapRelativeAngles => {
                let on = !self.session.snap.relative_angles;
                self.session.snap.relative_angles = on;
                self.status = format!(
                    "Angles relative to the prior segment {}",
                    if on { "on" } else { "off" }
                );
            }
            Action::SnapAcquire => self.acquire_smart(),
            Action::ZoomIn => self.pending_zoom *= 2.0,
            Action::ZoomOut => self.pending_zoom *= 0.5,
            // Arrow keys move the view like the scroll bars would: the drawing follows the key.
            Action::PanLeft => self.pending_pan.x += PAN_STEP_PX,
            Action::PanRight => self.pending_pan.x -= PAN_STEP_PX,
            Action::PanUp => self.pending_pan.y += PAN_STEP_PX,
            Action::PanDown => self.pending_pan.y -= PAN_STEP_PX,
            Action::Delete
            | Action::PanHold
            | Action::SnapSuspend
            | Action::DataBarFocus
            | Action::DataBarPrev => {}
            a if !a.implemented() => {
                self.status = format!(
                    "{} ({}) is planned, not built yet",
                    a.label(),
                    self.keys.hint(a)
                )
            }
            _ => {}
        }
    }

    /// Tab and Shift+Tab: enter the Data bar fields, or move between them.
    fn data_bar_key(&mut self, ctx: &egui::Context, action: Action, typing: bool) {
        if self.entry.active {
            self.entry.cycle(action == Action::DataBarPrev);
        } else if !typing
            && action == Action::DataBarFocus
            && databar::supported(self.machine.tool())
        {
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
                    egui::Event::Key {
                        key,
                        physical_key,
                        pressed: true,
                        repeat,
                        modifiers,
                    } => Some((*key, *physical_key, *modifiers, *repeat)),
                    _ => None,
                })
                .collect()
        });
        for (key, physical, mods, repeat) in events {
            let Some((action, chord)) = self.keys.action_for_event(key, physical, mods) else {
                continue;
            };
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
        if let Some(r) = databar::typed_arc_radius(
            self.machine.tool(),
            anchored,
            self.arc_fields(),
            &self.entry.values(),
        ) {
            self.settings.arc_radius = r;
        }
    }

    /// Keys U, I, O, P, [ and ]: cycle the modes of a group of mode buttons. The path tools
    /// have one group (the vertex modes), so only group 1 does anything for them.
    fn mode_group(&mut self, group: usize) {
        let tool = self.machine.tool();
        if tool == Tool::Irrigation && group == 1 {
            let irr = &mut self.settings.irrigation;
            irr.mode = irr.mode.next();
            self.machine.cancel();
            self.status = format!("Irrigation mode: {}", irr.mode.label());
        } else if tool == Tool::Dimension && group == 1 {
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
            self.status = format!(
                "The {} tool has only one group of mode buttons",
                tool.name()
            );
        }
    }

    /// Enter in a Data bar field: click at the typed point, as if the user had clicked there.
    fn commit_entry(&mut self, ctx: &egui::Context) {
        let target = databar::target_point(
            self.machine.tool(),
            self.machine.anchor(),
            self.live_cursor,
            &self.entry.values(),
            self.arc_fields(),
        );
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
            let cx = Ctx {
                doc: self.session.editor.doc(),
                selection: &self.session.selection,
                settings: &self.settings,
                tol: self.tol(),
                context: self.session.edit_context(),
            };
            self.machine.handle(input, &cx)
        };
        for e in effects {
            match e {
                Effect::SetSelection(s) => self.session.selection = s,
                Effect::Notice(m) => self.status = m,
                Effect::Exec(cmd) => {
                    let cmd = self.session.in_context(cmd);
                    let is_add = matches!(
                        cmd,
                        Command::AddObject { .. }
                            | Command::AddObjectIn { .. }
                            | Command::AddHardscape { .. }
                            | Command::AddLandscapeArea { .. }
                    );
                    if let Some(out) =
                        panels::run_command(&mut self.session, &cmd, &mut self.status)
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
        if let Some(page) = crate::sheets_ui::active_sheet(&self.session)
            .and_then(|l| self.session.editor.doc().sheet(l))
        {
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
        #[cfg(not(target_arch = "wasm32"))]
        let page = sheet.and_then(|l| self.session.sheet_page(l).ok());
        // Sheet output needs the print crate, which the web build leaves out.
        #[cfg(target_arch = "wasm32")]
        let page: Option<std::sync::Arc<cadektor_print::Page>> = None;
        self.canvas_rect = Some(rect);
        if self.needs_fit {
            self.fit_view(rect);
            self.needs_fit = false;
        }
        let typing = ui.ctx().egui_wants_keyboard_input() || self.entry.active;
        let (latest, hover, mods, space, suspend, scroll, pinch, pressed, released) =
            ui.input(|i| {
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
            self.view.zoom_at(
                rect.center(),
                std::mem::replace(&mut self.pending_zoom, 1.0),
            );
        }
        if self.pending_pan != egui::Vec2::ZERO {
            self.view.pan(std::mem::take(&mut self.pending_pan));
        }

        // Pan and zoom.
        let panning = resp.dragged_by(egui::PointerButton::Middle)
            || (space && resp.dragged_by(egui::PointerButton::Primary));
        if panning {
            self.view.pan(resp.drag_delta());
        }
        if let (true, Some(h)) = (resp.hovered(), hover) {
            let factor = (scroll as f64 * 0.0025).exp() * pinch as f64;
            if (factor - 1.0).abs() > 1e-9 {
                self.view.zoom_at(h, factor);
            }
        }

        // Mouse buttons 4 and 5 (back and forward) go through the view history.
        let (back, forward) = ui.input(|i| {
            (
                i.pointer.button_pressed(egui::PointerButton::Extra1),
                i.pointer.button_pressed(egui::PointerButton::Extra2),
            )
        });
        if back {
            self.step_view_history(true);
        } else if forward {
            self.step_view_history(false);
        }
        // A view that has rested counts as a view in the history.
        let waiting = self
            .view_history
            .observe(ui.input(|i| i.time), self.view_snapshot());
        if waiting {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(450));
        }

        // Cursor and snapping.
        if let Some(p) = latest.filter(|p| rect.contains(*p) || self.machine.is_busy()) {
            let raw = self.view.to_model(p);
            let busy_ids = self.machine.moving_ids().to_vec();
            self.snap_hit = if suspend {
                None
            } else {
                snap::snap_cursor(
                    self.session.editor.doc(),
                    raw,
                    self.tol(),
                    grid_step(self.view.zoom, 12.0),
                    &self.session.snap,
                    &self.session.smart,
                    self.machine.anchor(),
                    self.machine.prior_direction(),
                    &busy_ids,
                )
            };
            self.cursor = self.snap_hit.as_ref().map_or(raw, |s| s.point);
            self.live_cursor = self.cursor;
            // Typed Data bar values override the mouse.
            if self.entry.any_typed()
                && let Some(p) = databar::target_point(
                    self.machine.tool(),
                    self.machine.anchor(),
                    self.live_cursor,
                    &self.entry.values(),
                    self.arc_fields(),
                )
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
                self.send(Input::Press {
                    p: self.cursor,
                    shift: mods.shift,
                });
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

        let cx = Ctx {
            doc: self.session.editor.doc(),
            selection: &self.session.selection,
            settings: &self.settings,
            tol: self.tol(),
            context: self.session.edit_context(),
        };
        let preview = if resp.hovered() || self.machine.is_busy() {
            self.machine.preview(self.cursor, &cx)
        } else {
            crate::tools::Preview::None
        };
        let frame = canvas::Frame {
            doc: self.session.editor.doc(),
            selection: &self.session.selection,
            preview: &preview,
            snap: self.snap_hit.as_ref(),
            cues: self.prefs.cues,
            dark: self.dark,
            smart: &self.session.smart,
            show_grid: self.show_grid,
            site: Some((self.session.site_cache(), self.session.editor.revision())),
            sheet: page.as_deref().map(|p| (&self.nav.sheets, p)),
        };
        canvas::paint(&ui.painter_at(rect), rect, &self.view, &frame);
    }
}

/// The tabs of the dock draw through the app.
struct DockViewer<'a> {
    app: &'a mut CadektorApp,
}

impl egui_dock::TabViewer for DockViewer<'_> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        self.app.tab_ui(ui, *tab);
    }

    fn is_closeable(&self, tab: &Tab) -> bool {
        tab.closeable()
    }

    fn allowed_in_windows(&self, tab: &mut Tab) -> bool {
        *tab != Tab::Drawing
    }

    fn clear_background(&self, tab: &Tab) -> bool {
        *tab != Tab::Drawing
    }

    fn scroll_bars(&self, _tab: &Tab) -> [bool; 2] {
        [false, false]
    }
}

impl CadektorApp {
    /// The pan and zoom of the plan, for the view history.
    fn view_snapshot(&self) -> view_history::Snapshot {
        view_history::Snapshot {
            origin: self.view.origin,
            zoom: self.view.zoom,
        }
    }

    /// Previous View and Next View.
    fn step_view_history(&mut self, back: bool) {
        if !self.plan() {
            self.status = "The view history is for the plan. Switch to Top/Plan".into();
            return;
        }
        let here = self.view_snapshot();
        self.view_history.flush(here);
        let snapshot = if back {
            self.view_history.previous()
        } else {
            self.view_history.next()
        };
        match snapshot {
            Some(s) => {
                self.view.origin = s.origin;
                self.view.zoom = s.zoom;
                self.needs_fit = false;
            }
            None => {
                self.status = if back {
                    "No earlier view".into()
                } else {
                    "No later view".into()
                }
            }
        }
    }

    /// The middle of the box around the selected objects.
    fn selection_center(&self) -> Option<DVec2> {
        let doc = self.session.editor.doc();
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for id in &self.session.selection {
            if let Some(o) = doc.object(*id) {
                let (a, b) = doc.object_bounds(o);
                if a.is_finite() && b.is_finite() {
                    lo = lo.min(a);
                    hi = hi.max(b);
                }
            }
        }
        lo.is_finite().then(|| (lo + hi) / 2.0)
    }

    fn transform_selection(&mut self, transform: Transform, what: &str) {
        let ids = self.session.selection.clone();
        if ids.is_empty() {
            self.status = format!("Select the objects to {what} first");
            return;
        }
        self.exec(&Command::TransformObjects {
            ids,
            transform,
            copy: false,
        });
    }

    fn turn_selection(&mut self, angle: f64) {
        if let Some(center) = self.selection_center() {
            self.transform_selection(Transform::Rotate { center, angle }, "rotate");
        } else {
            self.status = "Select the objects to rotate first".into();
        }
    }

    /// Flip about the middle of the selection. Horizontal swaps left and right.
    fn flip_selection(&mut self, horizontal: bool) {
        if let Some(c) = self.selection_center() {
            let (a, b) = if horizontal {
                (c, c + DVec2::Y)
            } else {
                (c, c + DVec2::X)
            };
            self.transform_selection(Transform::Mirror { a, b }, "flip");
        } else {
            self.status = "Select the objects to flip first".into();
        }
    }

    /// Add, subtract, clip, intersect or compose the selected shapes. For Subtract the lowest shape
    /// of the drawing is the base and the others cut it. For Clip the topmost one cuts the others.
    fn surface_op(&mut self, op: SurfaceOp) {
        let doc = self.session.editor.doc();
        let mut ids = self.session.selection.clone();
        ids.sort_by_key(|id| doc.objects.get_index_of(id).unwrap_or(usize::MAX));
        if ids.len() < 2 {
            self.status = "Select two or more shapes first".into();
            return;
        }
        let result = if op == SurfaceOp::Subtract {
            let base = ids[0];
            plan::surface(doc, op, &ids[1..], Some(base), false)
        } else {
            plan::surface(doc, op, &ids, None, op == SurfaceOp::Clip)
        };
        match result {
            Ok(cmd) => {
                if panels::run_command(&mut self.session, &cmd, &mut self.status).is_some() {
                    self.status = "Done".into();
                }
            }
            Err(e) => self.status = format!("Error: {e}"),
        }
    }

    /// The T key: take the point or edge under the pointer as a smart point or smart edge.
    fn acquire_smart(&mut self) {
        let tol = self.tol();
        let geom = snap::geometry(
            self.session.editor.doc(),
            &[],
            &self.session.snap,
            Some((self.live_cursor, tol * 4.0)),
        );
        let edges = self.session.snap.set.smart_edges;
        self.status = match self
            .session
            .smart
            .acquire_at(self.live_cursor, &geom, tol, edges)
        {
            Some(cadektor_core::snapping::Acquired::Point(_)) => "Smart point set".into(),
            Some(cadektor_core::snapping::Acquired::Edge(..)) => "Smart edge set".into(),
            None => "No object point under the pointer. Move onto a corner, end or center".into(),
        };
    }

    /// The commands that quick search offers.
    fn quick_items(&self) -> Vec<QuickItem> {
        let mut out = Vec::new();
        let skip = |a: Action| {
            matches!(
                a,
                Action::PanHold
                    | Action::SnapSuspend
                    | Action::DataBarFocus
                    | Action::DataBarPrev
                    | Action::Cancel
                    | Action::Finish
                    | Action::Delete
                    | Action::QuickSearch
                    | Action::PanLeft
                    | Action::PanRight
                    | Action::PanUp
                    | Action::PanDown
                    | Action::NudgeLeft
                    | Action::NudgeRight
                    | Action::NudgeUp
                    | Action::NudgeDown
            ) || a.id().starts_with("mode.")
                || !a.implemented()
        };
        for a in Action::ALL.iter().copied().filter(|a| !skip(*a)) {
            out.push(QuickItem {
                label: a.label().to_string(),
                keys: self.keys.hint(a),
                command: QuickCommand::Action(a),
            });
        }
        for (label, m) in [
            ("Preferences", MenuAction::Preferences),
            ("Export SVG", MenuAction::ExportSvg),
            ("Select All", MenuAction::SelectAll),
        ] {
            out.push(QuickItem {
                label: label.into(),
                keys: String::new(),
                command: QuickCommand::Menu(m),
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        for (label, m) in [
            ("Import", MenuAction::Import),
            ("Export", MenuAction::Export),
            ("Map Sources", MenuAction::MapSources),
            ("Change CRS", MenuAction::ChangeCrs),
            ("Scripts", MenuAction::Scripts),
            ("Export PDF", MenuAction::ExportPdf),
        ] {
            out.push(QuickItem {
                label: label.into(),
                keys: String::new(),
                command: QuickCommand::Menu(m),
            });
        }
        for t in ThemeChoice::ALL {
            out.push(QuickItem {
                label: format!("Theme: {}", t.label()),
                keys: String::new(),
                command: QuickCommand::Theme(t),
            });
        }
        for (name, label, _) in crate::snap_ui::CATEGORIES {
            out.push(QuickItem {
                label: format!("Snapping: toggle {label}"),
                keys: String::new(),
                command: QuickCommand::SnapToggle(name),
            });
        }
        for tab in Tab::ALL.iter().copied().filter(|t| t.closeable()) {
            out.push(QuickItem {
                label: format!("Show {}", tab.title()),
                keys: String::new(),
                command: QuickCommand::ShowTab(tab),
            });
        }
        for (label, c) in [
            ("Save Layout As", QuickCommand::SaveLayout),
            ("Reset Layout", QuickCommand::ResetLayout),
        ] {
            out.push(QuickItem {
                label: label.into(),
                keys: String::new(),
                command: c,
            });
        }
        out
    }

    fn run_quick(&mut self, ctx: &egui::Context, cmd: QuickCommand) {
        match cmd {
            QuickCommand::Action(a) => self.run_action(ctx, a),
            QuickCommand::Menu(m) => self.apply_menu(ctx, m),
            QuickCommand::Theme(t) => self.prefs.theme = t,
            QuickCommand::SnapToggle(name) => {
                let on = self.session.snap.set.get(name).unwrap_or(false);
                self.session.snap.set.set(name, !on);
                self.status = format!("Snap {name}: {}", if on { "off" } else { "on" });
            }
            QuickCommand::ShowTab(t) => self.layouts.show_tab(t),
            QuickCommand::SaveLayout => self.layout_name = Some(String::new()),
            QuickCommand::ResetLayout => self.layouts.reset(),
        }
    }

    fn handle_requests(&mut self, requests: Vec<MenuRequest>) {
        for r in requests {
            match r {
                MenuRequest::ShowTab(t) => self.layouts.show_tab(t),
                MenuRequest::SaveLayoutAs => self.layout_name = Some(String::new()),
                MenuRequest::LoadLayout(n) => {
                    if !self.layouts.apply(&n) {
                        self.status = format!("No layout named {n}");
                    }
                }
                MenuRequest::DeleteLayout(n) => {
                    self.layouts.delete(&n);
                    self.save_layouts();
                }
                MenuRequest::ResetLayout => self.layouts.reset(),
            }
        }
    }

    /// The small window that asks for the name of a layout.
    fn layout_prompt(&mut self, ctx: &egui::Context) {
        let Some(name) = self.layout_name.as_mut() else {
            return;
        };
        let mut done = None;
        egui::Window::new("Save Layout As")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let r = ui.add(egui::TextEdit::singleline(name).hint_text("Name of the layout"));
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    done = Some(true);
                }
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        done = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        done = Some(false);
                    }
                });
            });
        match done {
            Some(true) => {
                let name = self.layout_name.take().unwrap_or_default();
                match self.layouts.save_as(&name) {
                    Ok(()) => {
                        self.save_layouts();
                        self.status = format!("Saved layout {}", name.trim());
                    }
                    Err(e) => self.status = e,
                }
            }
            Some(false) => self.layout_name = None,
            None => {}
        }
    }

    fn save_layouts(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(e) = self.layouts.save() {
            self.status = e;
        }
    }

    /// Saves the preferences and the layout when they changed.
    fn persist(&mut self, now: f64) {
        let _ = now;
        self.prefs.snap = self.session.snap;
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.prefs != self.prefs_saved {
                match prefs::save(&self.prefs) {
                    Ok(()) => self.prefs_saved = self.prefs,
                    Err(e) => {
                        self.status = e;
                        self.prefs_saved = self.prefs;
                    }
                }
            }
            // The layout is compared and saved at most twice a second, not on every frame.
            if now - self.layout_checked >= 0.5 {
                self.layout_checked = now;
                if self.layouts.changed() {
                    self.save_layouts();
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.prefs_saved = self.prefs;
        }
    }

    /// Answers the agents that wait for a screenshot, when the window has sent the picture.
    #[cfg(not(target_arch = "wasm32"))]
    fn finish_screenshots(&mut self, ctx: &egui::Context) {
        let ready: Vec<(u64, std::sync::Arc<egui::ColorImage>)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot {
                        user_data, image, ..
                    } => user_data
                        .data
                        .as_ref()
                        .and_then(|d| d.downcast_ref::<u64>())
                        .map(|k| (*k, image.clone())),
                    _ => None,
                })
                .collect()
        });
        for (key, image) in ready {
            let Some(i) = self.shots.iter().position(|p| p.key == key) else {
                continue;
            };
            let pending = self.shots.remove(i);
            let crop = match pending.request.region {
                screenshot::Region::Drawing => self.canvas_rect,
                screenshot::Region::Window => None,
            };
            let line = match screenshot::prepare(
                &image,
                crop,
                ctx.pixels_per_point(),
                pending.request.max_width,
            ) {
                Ok(img) => screenshot::answer(
                    &pending.id,
                    &img,
                    &pending.request,
                    self.dark,
                    self.session.view.view.id(),
                ),
                Err(e) => screenshot::failure_line(&pending.id, &e),
            };
            let _ = pending.reply.send(Some(line));
        }
        let now = ctx.input(|i| i.time);
        self.shots.retain(|p| {
            let alive = now - p.since <= screenshot::TIMEOUT;
            if !alive {
                let _ = p.reply.send(Some(screenshot::timeout_line(&p.id)));
            }
            alive
        });
        if !self.shots.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// The dock: the drawing and the palettes as tabs that the user can move.
    fn show_dock(&mut self, ui: &mut egui::Ui) {
        let mut state = self
            .layouts
            .current
            .take()
            .unwrap_or_else(layout::default_state);
        let style = egui_dock::Style::from_egui(ui.style());
        egui_dock::DockArea::new(&mut state)
            .style(style)
            .show_close_buttons(true)
            .show_inside(ui, &mut DockViewer { app: self });
        // A layout needs the drawing: a closed or dragged-out drawing tab brings the default back.
        if !layout::has_drawing(&state) {
            state = layout::default_state();
        }
        self.layouts.current = Some(state);
    }

    /// The content of one tab of the dock.
    fn tab_ui(&mut self, ui: &mut egui::Ui, tab: Tab) {
        match tab {
            Tab::Drawing => {
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
            }
            Tab::Tools => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    panels::tool_panel(
                        ui,
                        &mut self.machine,
                        &self.keys,
                        &mut self.settings,
                        self.session.editor.doc(),
                    )
                });
            }
            Tab::ObjectInfo => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    panels::object_info(ui, &mut self.session, &mut self.info, &mut self.status);
                    #[cfg(not(target_arch = "wasm32"))]
                    self.script_ui
                        .plugin_info(ui, &mut self.session, &mut self.status);
                });
            }
            Tab::Navigation => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    panels::navigation(ui, &mut self.session, &mut self.nav, &mut self.status);
                    ui.add_space(12.0);
                    crate::rules_ui::section(ui, &mut self.session, &mut self.status);
                });
            }
            Tab::Resources => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    plant_ui::resource_manager(
                        ui,
                        &mut self.session,
                        &mut self.plant_ui,
                        &mut self.status,
                    );
                    crate::data_ui::resource_sections(ui, &mut self.session, &mut self.status);
                    resource_ui::section(
                        ui,
                        &mut self.session,
                        &mut self.resource_ui,
                        &mut self.status,
                    );
                });
            }
            Tab::Schedule => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    plant_ui::plant_schedule(ui, &mut self.session, &mut self.status);
                });
            }
            Tab::Snapping => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    crate::snap_ui::palette(
                        ui,
                        &mut self.session.snap,
                        &mut self.session.smart,
                        &mut self.prefs.cues,
                    );
                });
            }
        }
    }
}

impl eframe::App for CadektorApp {
    #[cfg(not(target_arch = "wasm32"))]
    fn on_exit(&mut self) {
        // Stops the agent server and deletes its runtime file.
        self.agent = None;
        // A clean exit: the recovery copies are not needed any more.
        self.autosave.finish();
        self.persist(f64::INFINITY);
        self.save_layouts();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.theme_applied != Some(self.prefs.theme) {
            theme::apply(&ctx, self.prefs.theme);
            self.theme_applied = Some(self.prefs.theme);
        }
        self.dark = theme::is_dark(&ctx);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(agent) = &self.agent {
            let mut fx = file_io::AgentEffects::default();
            self.session.doc_dir = self
                .path
                .as_ref()
                .and_then(|p| p.parent())
                .map(std::path::Path::to_path_buf);
            // Scripts that the user starts connect to this address, with this token.
            self.session
                .scripting
                .endpoint
                .get_or_insert_with(|| (agent.addr.to_string(), agent.token.clone()));
            let now = ctx.input(|i| i.time);
            let (shots, shot_key) = (&mut self.shots, &mut self.shot_key);
            agent.poll_ext(&mut |line, reply, conn| {
                // A screenshot is answered in a later frame, when the window has sent the picture.
                match screenshot::parse(line) {
                    Some(Ok((id, request))) => {
                        *shot_key += 1;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                            egui::UserData::new(*shot_key),
                        ));
                        shots.push(screenshot::Pending {
                            id,
                            reply: reply.clone(),
                            request,
                            since: now,
                            key: *shot_key,
                        });
                        return crate::agent::Reply::Later;
                    }
                    Some(Err(answer)) => return crate::agent::Reply::Now(Some(answer)),
                    None => {}
                }
                crate::agent::Reply::Now(file_io::agent_line(
                    &mut self.session,
                    &mut self.path,
                    conn,
                    line,
                    &mut fx,
                ))
            });
            // Change events (`events.subscribe`) go to the clients that asked for them.
            for conn in self.session.subscribers() {
                for note in self.session.take_notifications(conn) {
                    if !agent.push(conn, note.to_string()) {
                        self.session.drop_connection(conn);
                        break;
                    }
                }
            }
            if fx.replaced {
                self.machine.cancel();
                self.needs_fit = true;
                self.view_history.clear();
                self.view3d.request_fit();
            }
            self.title_dirty |= fx.path_changed;
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.finish_screenshots(&ctx);
        #[cfg(not(target_arch = "wasm32"))]
        if std::mem::take(&mut self.title_dirty) {
            self.set_title(&ctx);
        }
        #[cfg(target_arch = "wasm32")]
        self.take_web_files();
        self.autosave.tick(
            &ctx,
            self.session.editor.revision(),
            self.path.as_deref(),
            self.session.editor.doc(),
        );
        if let Some(r) = self.autosave.show(&ctx) {
            self.replace_document(r.doc, None);
            self.status = r.status;
        }
        self.entry.sync(
            self.machine.tool(),
            self.machine.anchor().is_some(),
            self.arc_fields(),
        );
        self.handle_keys(&ctx);
        // The camera follows the chosen view, whoever chose it: a key, the view bar or an agent.
        let forced = self.session.take_view_changed();
        self.view3d.sync(self.session.view, forced);

        let (can_undo, can_redo) = (
            self.session.editor.can_undo(),
            self.session.editor.can_redo(),
        );
        let mut actions = Vec::new();
        let mut requests = Vec::new();
        egui::Panel::top("menu").show(ui, |ui| {
            let can_site =
                !crate::site_ui::source_ids(self.session.editor.doc(), &self.session.selection)
                    .is_empty();
            let can_update = !crate::site_ui::selected_site_models(
                self.session.editor.doc(),
                &self.session.selection,
            )
            .is_empty();
            let mut extras = MenuExtras {
                snap: &mut self.session.snap,
                cues: &mut self.prefs.cues,
                theme: &mut self.prefs.theme,
                layout_names: self.layouts.names(),
                can_back: self.view_history.can_go_back(),
                can_forward: self.view_history.can_go_forward(),
                has_selection: !self.session.selection.is_empty(),
                requests: Vec::new(),
            };
            actions = panels::menu_bar(
                ui,
                &self.keys,
                can_undo,
                can_redo,
                &mut extras,
                &mut self.show_grid,
                can_site,
                can_update,
            );
            requests = std::mem::take(&mut extras.requests);
        });
        for a in actions {
            self.apply_menu(&ctx, a);
        }
        self.handle_requests(requests);
        #[cfg(not(target_arch = "wasm32"))]
        if self.io_ui.show(&ctx, &mut self.session, &mut self.status)
            == crate::io_dialog::IoEvent::Imported
        {
            self.needs_fit = true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(script) = self
            .script_ui
            .show(&ctx, &mut self.session, &mut self.status)
        {
            self.ensure_script_endpoint(&ctx);
            self.script_ui
                .start(&mut self.session, script, &mut self.status);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.gis_ui.show(&ctx, &mut self.session, &mut self.status) {
            self.session
                .selection
                .retain(|id| self.session.editor.doc().object(*id).is_some());
            self.needs_fit = self.needs_fit || self.session.editor.doc().objects.len() <= 1;
        }
        let before = self.session.view.view;
        egui::Panel::top("viewbar").show(ui, |ui| {
            panels::view_bar(ui, &mut self.session.view, &self.keys, self.gpu_ready);
            if let Some(e) = crate::present_ui::show(ui, &mut self.session, self.gpu_ready) {
                self.status = e;
            }
        });
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
                        ui.label(format!(
                            "{} objects, {} selected, {}, {}",
                            d.objects.len(),
                            self.session.selection.len(),
                            v.view.label(),
                            v.projection.id()
                        ));
                    }
                });
            });
        });
        let mut commit = false;
        let readout = self
            .readout
            .text(self.session.editor.doc().georef.as_ref(), self.live_cursor);
        egui::Panel::bottom("databar").show(ui, |ui| {
            commit = panels::data_bar(
                ui,
                &self.machine,
                &mut self.settings,
                &mut self.entry,
                self.live_cursor,
                self.snap_hit.as_ref(),
                self.session.snap.set.any(),
                readout.as_deref().map(|t| (t, &mut self.readout.lonlat)),
            );
        });
        if commit {
            self.commit_entry(&ctx);
        }
        crate::data_ui::windows(&ctx, &mut self.session, &mut self.status);
        let rect = self.canvas_rect;
        self.sync_saved_views(rect);
        egui::CentralPanel::no_frame().show(ui, |ui| self.show_dock(ui));
        self.run_annot_requests();
        // The dialogs and the quick search.
        let center = self.selection_center();
        if let Some(cmd) = self.array_dlg.show(&ctx, &self.session.selection, center) {
            self.exec(&cmd);
        }
        let items = if self.quick.is_open() {
            self.quick_items()
        } else {
            Vec::new()
        };
        if let Some(cmd) = self.quick.show(&ctx, &items) {
            self.run_quick(&ctx, cmd);
        }
        self.layout_prompt(&ctx);
        let now = ctx.input(|i| i.time);
        self.persist(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadektor_render::Projection;

    /// A key press followed by its release, so the next press is not seen as a repeat.
    fn key(k: Key, mods: egui::Modifiers) -> Vec<egui::Event> {
        let ev = |pressed| egui::Event::Key {
            key: k,
            physical_key: Some(k),
            pressed,
            repeat: false,
            modifiers: mods,
        };
        vec![ev(true), ev(false)]
    }

    /// Runs one frame of the key handling and the Data bar, like `ui` does.
    fn frame(app: &mut CadektorApp, ctx: &egui::Context, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            app.entry.sync(
                app.machine.tool(),
                app.machine.anchor().is_some(),
                app.arc_fields(),
            );
            app.handle_keys(&ctx);
            egui::Panel::bottom("databar").show(ui, |ui| {
                panels::data_bar(
                    ui,
                    &app.machine,
                    &mut app.settings,
                    &mut app.entry,
                    app.live_cursor,
                    None,
                    app.session.snap.set.any(),
                    None,
                );
            });
        });
    }

    #[test]
    fn tab_enters_data_bar_and_esc_keeps_the_drawing() {
        let ctx = egui::Context::default();
        let mut app = CadektorApp::blank();
        let none = egui::Modifiers::NONE;
        app.machine.set_tool(Tool::Line);
        app.send(Input::Press {
            p: DVec2::ZERO,
            shift: false,
        });
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
        assert!(
            app.machine.is_busy(),
            "Esc in a Data bar field must not cancel the drawing"
        );
        assert!(!app.entry.active);
        // A second Esc, now on the canvas, cancels.
        frame(&mut app, &ctx, key(Key::Escape, none));
        assert!(!app.machine.is_busy());
    }

    /// Runs one frame of the whole dock (the drawing and every palette).
    fn dock_frame(app: &mut CadektorApp, ctx: &egui::Context) {
        let _ = ctx.run_ui(Default::default(), |ui| {
            egui::CentralPanel::no_frame().show(ui, |ui| app.show_dock(ui));
        });
    }

    #[test]
    fn the_dock_draws_every_tab_and_keeps_the_drawing() {
        let ctx = egui::Context::default();
        let mut app = CadektorApp::blank();
        dock_frame(&mut app, &ctx);
        assert!(app.canvas_rect.is_some(), "the drawing tab drew the canvas");
        // Every tab can be shown alone.
        for tab in Tab::ALL {
            let _ = ctx.run_ui(Default::default(), |ui| app.tab_ui(ui, tab));
        }
        // Closing every other tab and the drawing tab brings the default layout back.
        app.layouts.current = Some(egui_dock::DockState::new(vec![Tab::Tools]));
        dock_frame(&mut app, &ctx);
        assert!(layout::has_drawing(app.layouts.current.as_ref().unwrap()));
        // Layout requests from the menu.
        app.handle_requests(vec![MenuRequest::SaveLayoutAs]);
        assert!(app.layout_name.is_some());
        app.layout_name = Some("Mine".into());
        app.layouts.save_as("Mine").unwrap();
        app.handle_requests(vec![
            MenuRequest::ResetLayout,
            MenuRequest::LoadLayout("Mine".into()),
            MenuRequest::ShowTab(Tab::Snapping),
        ]);
        app.handle_requests(vec![MenuRequest::LoadLayout("Nope".into())]);
        assert!(app.status.contains("No layout named"));
        let _ = ctx.run_ui(Default::default(), |_| app.layout_prompt(&ctx));
    }

    #[test]
    fn view_history_keys_go_back_and_forward() {
        let ctx = egui::Context::default();
        let mut app = CadektorApp::blank();
        let cs = egui::Modifiers {
            command: true,
            shift: true,
            ..Default::default()
        };
        app.view.zoom = 0.5;
        app.view_history.flush(app.view_snapshot());
        app.view.zoom = 2.0;
        frame(&mut app, &ctx, key(Key::Comma, cs));
        assert_eq!(app.view.zoom, 0.5);
        frame(&mut app, &ctx, key(Key::Period, cs));
        assert_eq!(app.view.zoom, 2.0);
        frame(&mut app, &ctx, key(Key::Period, cs));
        assert_eq!(app.status, "No later view");
        // In 3D the history is for the plan only.
        app.session.view.view = StandardView::Front;
        frame(&mut app, &ctx, key(Key::Comma, cs));
        assert!(app.status.contains("plan"), "{}", app.status);
    }

    #[test]
    fn keys_for_quick_search_cues_and_smart_points() {
        let ctx = egui::Context::default();
        let mut app = CadektorApp::blank();
        let none = egui::Modifiers::NONE;
        frame(&mut app, &ctx, key(Key::F, none));
        assert!(app.quick.is_open());
        app.quick.close();
        assert!(app.prefs.cues);
        frame(&mut app, &ctx, key(Key::Y, none));
        assert!(!app.prefs.cues);
        assert!(!app.session.snap.relative_angles);
        frame(&mut app, &ctx, key(Key::Slash, none));
        assert!(app.session.snap.relative_angles);
        // T takes the point under the pointer.
        app.exec(&Command::AddObject {
            shape: Shape::Line {
                a: DVec2::new(0.0, 0.0),
                b: DVec2::new(1000.0, 0.0),
            },
            layer: None,
            class: None,
            attrs: Default::default(),
        });
        app.live_cursor = DVec2::new(3.0, 2.0);
        frame(&mut app, &ctx, key(Key::T, none));
        assert_eq!(app.session.smart.points, vec![DVec2::ZERO]);
        assert_eq!(app.status, "Smart point set");
        app.live_cursor = DVec2::new(5000.0, 5000.0);
        frame(&mut app, &ctx, key(Key::T, none));
        assert!(app.status.contains("No object point"), "{}", app.status);
    }

    #[test]
    fn modify_commands_work_on_the_selection() {
        let ctx = egui::Context::default();
        let mut app = CadektorApp::blank();
        let add = |app: &mut CadektorApp, shape: Shape| {
            let out = app
                .session
                .editor
                .execute(&Command::AddObject {
                    shape,
                    layer: None,
                    class: None,
                    attrs: Default::default(),
                })
                .unwrap();
            ObjectId(out.created[0])
        };
        let a = add(
            &mut app,
            Shape::Rect {
                min: DVec2::new(0.0, 0.0),
                max: DVec2::new(1000.0, 1000.0),
            },
        );
        let b = add(
            &mut app,
            Shape::Rect {
                min: DVec2::new(500.0, 500.0),
                max: DVec2::new(1500.0, 1500.0),
            },
        );
        // Nothing selected: a message.
        app.run_action(&ctx, Action::CmdAddSurface);
        assert!(app.status.contains("two or more"), "{}", app.status);
        app.run_action(&ctx, Action::CmdFlipHorizontal);
        assert!(app.status.contains("Select"), "{}", app.status);
        app.session.selection = vec![a, b];
        app.run_action(&ctx, Action::CmdFlipHorizontal);
        // The group of two rectangles is flipped about its middle (750).
        let Shape::Rect { min, .. } = app.session.editor.doc().object(a).unwrap().shape else {
            panic!()
        };
        assert_eq!(min.x, 500.0);
        app.run_action(&ctx, Action::CmdFlipVertical);
        app.run_action(&ctx, Action::CmdRotateLeft90);
        app.run_action(&ctx, Action::CmdRotateRight90);
        app.run_action(&ctx, Action::CmdDuplicateArray);
        assert!(app.array_dlg.open);
        app.array_dlg.open = false;
        // Surfaces.
        app.run_action(&ctx, Action::CmdSubtractSurface);
        assert_eq!(app.session.editor.doc().objects.len(), 1, "{}", app.status);
        app.undo();
        app.session.selection = vec![a, b];
        app.run_action(&ctx, Action::CmdAddSurface);
        assert_eq!(app.session.editor.doc().objects.len(), 1);
        app.undo();
        for action in [
            Action::CmdClipSurface,
            Action::CmdIntersectSurface,
            Action::CmdCompose,
        ] {
            app.session.selection = vec![a, b];
            app.run_action(&ctx, action);
            app.undo();
        }
        assert_eq!(app.session.editor.doc().objects.len(), 2);
    }

    #[test]
    fn quick_search_lists_commands_and_runs_them() {
        let ctx = egui::Context::default();
        let mut app = CadektorApp::blank();
        let items = app.quick_items();
        assert!(
            items
                .iter()
                .any(|i| i.label == "Fillet tool" && i.keys == "7")
        );
        assert!(items.iter().any(|i| i.label == "Theme: Dark"));
        assert!(items.iter().all(|i| !i.label.is_empty()));
        for (label, keys) in [
            ("Add Surface", "Cmd/Ctrl+Shift+Alt+A"),
            ("Subtract Surface", ""),
            ("Clip Surface", "Cmd/Ctrl+Shift+Alt+C"),
            ("Intersect Surface", "Cmd/Ctrl+Shift+Alt+I"),
            ("Compose", "Cmd/Ctrl+Shift+Alt+P"),
            ("Duplicate Array", "Cmd/Ctrl+Shift+Alt+D"),
            ("Flip horizontal", "Cmd/Ctrl+Shift+H"),
            ("Flip vertical", "Cmd/Ctrl+Shift+V"),
            ("Rotate left 90 degrees", "Cmd/Ctrl+L"),
            ("Rotate right 90 degrees", "Cmd/Ctrl+Shift+R"),
        ] {
            let item = items
                .iter()
                .find(|i| i.label == label)
                .unwrap_or_else(|| panic!("{label}"));
            // The hint shows the key of this system (Ctrl or Cmd).
            assert_eq!(
                item.keys.is_empty(),
                keys.is_empty(),
                "{label}: {}",
                item.keys
            );
        }
        app.run_quick(&ctx, QuickCommand::Theme(ThemeChoice::Dark));
        assert_eq!(app.prefs.theme, ThemeChoice::Dark);
        app.run_quick(&ctx, QuickCommand::SnapToggle("angle"));
        assert!(app.session.snap.set.angle);
        app.run_quick(&ctx, QuickCommand::SnapToggle("angle"));
        assert!(!app.session.snap.set.angle);
        app.run_quick(&ctx, QuickCommand::Action(Action::ToolFillet));
        assert_eq!(app.machine.tool(), Tool::Fillet);
        app.run_quick(&ctx, QuickCommand::ShowTab(Tab::Snapping));
        app.run_quick(&ctx, QuickCommand::SaveLayout);
        assert!(app.layout_name.is_some());
        app.run_quick(&ctx, QuickCommand::ResetLayout);
        app.run_quick(&ctx, QuickCommand::Menu(MenuAction::SelectAll));
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
        app.send(Input::Press {
            p: DVec2::ZERO,
            shift: false,
        });
        app.send(Input::Press {
            p: DVec2::X,
            shift: false,
        });
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
            [
                VertexMode::Bezier,
                VertexMode::Cubic,
                VertexMode::TangentArc,
                VertexMode::PointOnArc,
                VertexMode::Arc,
                VertexMode::Corner
            ]
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
        app.send(Input::Press {
            p: DVec2::new(200.0, 300.0),
            shift: false,
        });
        app.send(Input::Enter);
        let o = app.session.editor.doc().objects.values().next().unwrap();
        let vs = o.shape.vertices().unwrap();
        assert_eq!(
            vs[1].kind,
            cadektor_core::model::VertexKind::Arc { radius: 50.0 }
        );
    }

    /// A key press the way a real keyboard reports it: the logical key can differ from the physical one.
    fn real_key(logical: Key, physical: Key, mods: egui::Modifiers) -> Vec<egui::Event> {
        let ev = |pressed| egui::Event::Key {
            key: logical,
            physical_key: Some(physical),
            pressed,
            repeat: false,
            modifiers: mods,
        };
        vec![ev(true), ev(false)]
    }

    #[test]
    fn standard_view_keys_switch_between_plan_and_3d() {
        let ctx = egui::Context::default();
        let mut app = CadektorApp::blank();
        let m = egui::Modifiers {
            alt: true,
            shift: true,
            ..Default::default()
        };
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
        let cs = egui::Modifiers {
            shift: true,
            ..egui::Modifiers::COMMAND
        };
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
        app.send(Input::Press {
            p: DVec2::ZERO,
            shift: false,
        });
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
        app.session
            .handle(
                "view.set",
                serde_json::json!({"view": "back", "projection": "perspective"}),
            )
            .unwrap();
        let forced = app.session.take_view_changed();
        assert!(forced);
        app.view3d.sync(app.session.view, forced);
        assert!(!app.plan());
        assert!((app.view3d.camera.eye_dir() - glam::Vec3::Y).length() < 1e-5);
        assert_eq!(app.view3d.camera.projection, Projection::Perspective);
        app.session
            .handle("view.set", serde_json::json!({"view": "plan"}))
            .unwrap();
        assert!(app.session.take_view_changed());
        assert!(app.plan());
    }
}
