//! The window of the modeler: menu bar, tool bar, the 3D view, the status bar with the
//! Measurements box, and the tray. One `draw` per frame; the commands that the menu, the tool bar
//! and the keys give are run after the frame is laid out, so no panel holds the program while a
//! command changes it.

use std::path::PathBuf;

use kadakas_api::Session;
use kadakas_l10n::tr;
use kadakas_render::StandardView;
use kadakas_ui_core::keymap::{Action, Keymap};
use kadakas_ui_core::modeler::{self, ModelerTool, ToolState};
use kadakas_ui_core::nav3d::{self, Frame, Nav3d, NavAction};
use kadakas_ui_core::profile::Profile;
use kadakas_view3d::input::action_for_event;
use kadakas_view3d::view3d::View3d;

use crate::about::About;
use crate::canvas::{self, Canvas};
use crate::files::{self, Ask, Question};
use crate::launch::Launch;
use crate::prefs::{self, Prefs, ThemeChoice};
use crate::toolbar;
use crate::tray::{self, TrayState};

/// A command from the menu, the tool bar or a key.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    New,
    Open,
    Save,
    SaveAs,
    Quit,
    Undo,
    Redo,
    SelectAll,
    SelectNone,
    /// A tool or command of the tool bar.
    Tool(ModelerTool),
    /// Zoom Window: the next drag of the Zoom tool is a window.
    ZoomWindow,
    Nav(NavAction),
    SetFov(f32),
    Projection(bool),
    ToggleTray,
    Theme(ThemeChoice),
    /// A fixed language, or `None` for the language of the system.
    Language(Option<String>),
    About,
}

/// The program.
pub struct ModelerApp {
    pub session: Session,
    pub view3d: View3d,
    pub nav: Nav3d,
    pub tools: ToolState,
    pub keys: Keymap,
    pub prefs: Prefs,
    /// The settings as they are on disk. A change is written when the two differ.
    prefs_saved: Prefs,
    /// Where the settings are written. `None` writes nothing (the tests).
    prefs_dir: Option<PathBuf>,
    theme_applied: Option<ThemeChoice>,
    language_applied: Option<Option<String>>,
    /// The file of the model, if it has one.
    pub path: Option<PathBuf>,
    saved_revision: u64,
    pub status: String,
    pub gpu_ready: bool,
    fit_pending: bool,
    pub tray: TrayState,
    pub about: About,
    launch: Launch,
    ask: Box<dyn Ask>,
    close_confirmed: bool,
    title_shown: Option<String>,
    quit: bool,
}

impl ModelerApp {
    /// A program without a window and a GPU: the tests build it, and `new` builds on it.
    pub fn headless(prefs: Prefs, ask: Box<dyn Ask>) -> ModelerApp {
        let session = Session::default();
        let saved_revision = session.editor.revision();
        let prefs_language = prefs.language.clone();
        ModelerApp {
            session,
            view3d: View3d::default(),
            nav: Nav3d::new(),
            tools: ToolState::default(),
            keys: Profile::sketchup().keymap,
            prefs_saved: prefs.clone(),
            prefs,
            prefs_dir: None,
            theme_applied: None,
            // The tests set the language for their own thread. The window sets it for the process.
            language_applied: Some(prefs_language),
            path: None,
            saved_revision,
            status: String::new(),
            gpu_ready: false,
            fit_pending: true,
            tray: TrayState::default(),
            about: About::default(),
            launch: Launch::new(None),
            ask,
            close_confirmed: false,
            title_shown: None,
            quit: false,
        }
    }

    /// The program in a window. `files` are the paths on the command line; the first one opens.
    pub fn new(cc: &eframe::CreationContext<'_>, files: &[PathBuf]) -> ModelerApp {
        let (prefs, warning) = prefs::load();
        let mut app = ModelerApp::headless(prefs, Box::new(files::Dialogs));
        app.prefs_dir = prefs::dir();
        app.launch = Launch::from_env();
        app.language_applied = None;
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            let info = rs.adapter.get_info();
            crate::launch::set_gpu(
                &info.name,
                format!("{} {}", info.driver, info.driver_info).trim(),
                &format!("{:?}", info.backend),
            );
            let gpu = kadakas_view3d::gpu3d::Gpu3d::new(&rs.device, &rs.queue, rs.target_format);
            rs.renderer.write().callback_resources.insert(gpu);
            app.gpu_ready = true;
        }
        if let Some(w) = warning {
            app.status = w;
        }
        if let Some(first) = files.first() {
            app.open_path(first.clone());
        }
        app
    }

    /// True when the model changed since it was opened or saved.
    pub fn dirty(&self) -> bool {
        self.session.editor.revision() != self.saved_revision
    }

    /// The title of the window: the file, a star when it has unsaved changes, and the program.
    pub fn title(&self) -> String {
        let name = self.path.as_ref().and_then(|p| p.file_name()).map_or_else(
            || tr!("modeler-untitled"),
            |n| n.to_string_lossy().to_string(),
        );
        tr!(
            "modeler-title",
            name = name,
            changed = if self.dirty() { "*" } else { "" },
            app = tr!("modeler-window-title")
        )
    }

    // ---- files -------------------------------------------------------------------------------

    /// Asks what to do with unsaved changes before they are lost. True: go on.
    fn discard_ok(&mut self) -> bool {
        !self.dirty() || self.ask.ask(&Question::Discard)
    }

    pub fn new_document(&mut self) {
        if !self.discard_ok() {
            return;
        }
        let mut s = Session::default();
        std::mem::swap(&mut s, &mut self.session);
        self.after_replace(None);
    }

    pub fn open_path(&mut self, path: PathBuf) {
        if !self.discard_ok() {
            return;
        }
        match files::open(&mut self.session, &path, &mut *self.ask) {
            Ok(notes) => {
                self.after_replace(Some(path.clone()));
                self.status = if notes.is_empty() {
                    tr!("modeler-opened", name = path.display())
                } else {
                    tr!(
                        "modeler-opened-notes",
                        name = path.display(),
                        notes = notes.join("; ")
                    )
                };
            }
            Err(e) => self.status = e,
        }
    }

    /// What follows a new or opened model: nothing is selected, the view shows all of it.
    fn after_replace(&mut self, path: Option<PathBuf>) {
        self.session.selection.clear();
        self.path = path;
        self.saved_revision = self.session.editor.revision();
        self.nav.clear_history();
        self.fit_pending = true;
        self.tools.select();
    }

    pub fn save(&mut self, save_as: bool) {
        let path = match (&self.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => match files::pick_save(self.path.as_deref()) {
                Some(p) => p,
                None => return,
            },
        };
        self.save_to(path);
    }

    pub fn save_to(&mut self, path: PathBuf) {
        match files::save(&mut self.session, &path, &mut *self.ask) {
            Ok(saved) => {
                self.status = if saved.notes.is_empty() {
                    tr!("modeler-saved", name = saved.path.display())
                } else {
                    tr!(
                        "modeler-saved-notes",
                        name = saved.path.display(),
                        notes = saved.notes.join(" ")
                    )
                };
                self.path = Some(saved.path);
                self.saved_revision = self.session.editor.revision();
            }
            Err(e) => self.status = e,
        }
    }

    // ---- commands ----------------------------------------------------------------------------

    fn frame(&mut self, size: glam::Vec2) -> Frame {
        Frame {
            bounds: self.session.scene().bounds,
            size,
        }
    }

    /// Runs a command. `size` is the size of the 3D view in points.
    pub fn run(&mut self, cmd: Cmd, size: glam::Vec2) {
        match cmd {
            Cmd::New => self.new_document(),
            Cmd::Open => {
                if let Some(p) = files::pick_open() {
                    self.open_path(p);
                }
            }
            Cmd::Save => self.save(false),
            Cmd::SaveAs => self.save(true),
            Cmd::Quit => self.quit = true,
            Cmd::Undo => {
                if !self.session.editor.undo() {
                    self.status = tr!("modeler-nothing-to-undo");
                }
            }
            Cmd::Redo => {
                if !self.session.editor.redo() {
                    self.status = tr!("modeler-nothing-to-redo");
                }
            }
            Cmd::SelectAll => {
                self.session.selection = self
                    .session
                    .editor
                    .doc()
                    .objects
                    .values()
                    .filter(|o| o.parent.is_none())
                    .map(|o| o.id)
                    .collect();
            }
            Cmd::SelectNone => self.session.selection.clear(),
            Cmd::Tool(t) if t.is_command() => {
                let frame = self.frame(size);
                self.nav.zoom_extents(&frame);
            }
            Cmd::Tool(t) => {
                if self.tools.choose(t) {
                    self.status.clear();
                } else {
                    self.status = tr!(
                        "modeler-tool-later",
                        name = t.name(),
                        note = tr!("uic-modeler-coming-later")
                    );
                }
            }
            Cmd::ZoomWindow => self.tools.arm_zoom_window(),
            Cmd::Nav(n) => {
                let frame = self.frame(size);
                self.nav.apply(n, &frame);
            }
            Cmd::SetFov(deg) => self.nav.set_fov_deg(deg),
            Cmd::Projection(perspective) => {
                self.nav.set_projection(if perspective {
                    kadakas_render::Projection::Perspective
                } else {
                    kadakas_render::Projection::Orthographic
                });
            }
            Cmd::ToggleTray => self.prefs.tray = !self.prefs.tray,
            Cmd::Theme(t) => self.prefs.theme = t,
            Cmd::Language(l) => self.prefs.language = l,
            Cmd::About => self.about.open(),
        }
    }

    /// The command that a key action gives.
    pub fn cmd_of(action: Action) -> Option<Cmd> {
        if let Some(t) = ModelerTool::from_action(action) {
            return Some(Cmd::Tool(t));
        }
        if let Some(n) = nav3d::nav_action(action) {
            return Some(Cmd::Nav(n));
        }
        Some(match action {
            Action::NavZoomWindow => Cmd::ZoomWindow,
            Action::FileNew => Cmd::New,
            Action::FileOpen => Cmd::Open,
            Action::FileSave => Cmd::Save,
            Action::FileSaveAs => Cmd::SaveAs,
            Action::Undo => Cmd::Undo,
            Action::Redo => Cmd::Redo,
            Action::SelectAll => Cmd::SelectAll,
            Action::SelectNone => Cmd::SelectNone,
            // Escape and Space both go back to the Select tool.
            Action::Cancel => Cmd::Tool(ModelerTool::Select),
            _ => return None,
        })
    }

    /// Runs the key events of this frame.
    fn handle_keys(&mut self, ctx: &egui::Context, size: glam::Vec2) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let events: Vec<(egui::Key, Option<egui::Key>, egui::Modifiers, bool)> = ctx.input(|i| {
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
            let Some((action, _)) = action_for_event(&self.keys, key, physical, mods) else {
                continue;
            };
            if repeat && !action.repeats() {
                continue;
            }
            if let Some(cmd) = ModelerApp::cmd_of(action) {
                self.run(cmd, size);
            }
        }
    }

    // ---- settings ----------------------------------------------------------------------------

    /// Makes the theme and the language of the settings current, and writes a change to disk.
    fn apply_prefs(&mut self, ctx: &egui::Context) {
        if self.language_applied.as_ref() != Some(&self.prefs.language) {
            self.prefs.apply_language(&prefs::system_languages());
            self.language_applied = Some(self.prefs.language.clone());
            ctx.request_repaint();
        }
        if self.theme_applied != Some(self.prefs.theme) {
            ctx.set_theme(self.prefs.theme.preference());
            self.theme_applied = Some(self.prefs.theme);
        }
        if self.prefs != self.prefs_saved {
            if let Some(dir) = &self.prefs_dir
                && let Err(e) = prefs::save_to(dir, &self.prefs)
            {
                self.status = e;
            }
            self.prefs_saved = self.prefs.clone();
        }
    }

    // ---- the frame ---------------------------------------------------------------------------

    /// Draws the window. Runs once per frame.
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.apply_prefs(&ctx);
        let mut cmds: Vec<Cmd> = Vec::new();
        let size = canvas_size(&ctx);

        egui::Panel::top("menu").show(ui, |ui| {
            crate::menu::show(ui, self, &mut cmds);
        });
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(2.0);
            if let Some(t) = toolbar::show(ui, &self.tools, &self.keys) {
                cmds.push(Cmd::Tool(t));
            }
            ui.add_space(2.0);
        });

        // The scene is needed for Entity Info; it is made once per revision.
        let scene = self.session.scene();
        let info = modeler::entity_info(self.session.editor.doc(), &scene, &self.session.selection);
        let measurement = modeler::measurement(&info, self.tools.active(), self.nav.fov_deg());

        egui::Panel::bottom("status").show(ui, |ui| {
            crate::statusbar::show(ui, &self.status, self.tools.active(), &measurement);
        });
        if self.prefs.tray {
            egui::Panel::right("tray")
                .default_size(tray::WIDTH)
                .size_range(180.0..=420.0)
                .show(ui, |ui| tray::show(ui, &mut self.tray, &info));
        }
        let mut shown = canvas::Shown::default();
        egui::CentralPanel::no_frame().show(ui, |ui| {
            shown = canvas::show(
                ui,
                Canvas {
                    session: &mut self.session,
                    view3d: &mut self.view3d,
                    nav: &mut self.nav,
                    tools: &mut self.tools,
                    gpu_ready: self.gpu_ready,
                    fit_pending: &mut self.fit_pending,
                },
            );
        });
        let size = shown
            .rect
            .map_or(size, |r| glam::Vec2::new(r.width(), r.height()));

        self.handle_keys(&ctx, size);
        for cmd in cmds {
            self.run(cmd, size);
        }
        // A view that has rested becomes an entry of the history (Previous and Next).
        let now = ctx.input(|i| i.time);
        if self.nav.observe(now) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                kadakas_ui_core::view_history::SETTLE,
            ));
        }
        self.about.show(&ctx);
        self.finish_frame(&ctx);
    }

    /// The title, the close request and the end of the program.
    fn finish_frame(&mut self, ctx: &egui::Context) {
        let title = self.title();
        if self.title_shown.as_ref() != Some(&title) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title_shown = Some(title);
        }
        let close_asked = ctx.input(|i| i.viewport().close_requested());
        if close_asked && !self.close_confirmed {
            if self.discard_ok() {
                self.close_confirmed = true;
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        if self.quit {
            self.quit = false;
            if self.discard_ok() {
                self.close_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// The program ends: the launcher gets the clean-exit mark, as the last thing.
    pub fn on_exit(&mut self) {
        self.launch.finish();
    }
}

/// A first guess of the size of the 3D view, before the canvas has been laid out.
fn canvas_size(ctx: &egui::Context) -> glam::Vec2 {
    let r = ctx.content_rect();
    glam::Vec2::new(
        (r.width() - tray::WIDTH).max(100.0),
        (r.height() - 120.0).max(100.0),
    )
}

impl eframe::App for ModelerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }

    fn on_exit(&mut self) {
        ModelerApp::on_exit(self);
    }
}

/// The standard views of the Camera menu, in the order of the menu: Top, Bottom, Front, Back,
/// Left, Right, Iso.
pub const MENU_VIEWS: [StandardView; 7] = [
    StandardView::Top,
    StandardView::Bottom,
    StandardView::Front,
    StandardView::Back,
    StandardView::Left,
    StandardView::Right,
    StandardView::RightIsometric,
];
