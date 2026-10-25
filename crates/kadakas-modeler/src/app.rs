//! The window of the modeler: menu bar, tool bar, the 3D view, the status bar with the
//! Measurements box, and the tray. One `draw` per frame; the commands that the menu, the tool bar
//! and the keys give are run after the frame is laid out, so no panel holds the program while a
//! command changes it.

use std::path::PathBuf;

use kadakas_api::Session;
use kadakas_core::command::Command;
use kadakas_core::component::EditContext;
use kadakas_core::model::{ObjectId, Shape};
use kadakas_core::surface::IntersectScope;
use kadakas_l10n::tr;
use kadakas_render::StandardView;
use kadakas_ui_core::keymap::{Action, Keymap};
use kadakas_ui_core::materials::{MaterialRow, MaterialsPanel};
use kadakas_ui_core::modeler::{self, ModelerTool, ToolState};
use kadakas_ui_core::modeling;
use kadakas_ui_core::nav3d::{self, Frame, Nav3d, NavAction, NavStyle};
use kadakas_ui_core::profile::Profile;
use kadakas_ui_core::scene_panel;
use kadakas_view3d::input::action_for_event;
use kadakas_view3d::view3d::View3d;

use crate::about::About;
use crate::canvas::{self, Canvas};
use crate::edit3d::Edit3d;
use crate::files::{self, Ask};
use crate::launch::Launch;
use crate::prefs::{self, Prefs, ThemeChoice};
use crate::scenes_panel::{self, ScenesState};
use crate::tags_panel::{self, TagsState};
use crate::toolbar;
use crate::tray::{self, TrayAction, TrayData, TrayState, TrayUi};

/// The key of the Entity Info cache and its value.
type InfoKey = (
    (
        u64,
        Vec<kadakas_core::model::ObjectId>,
        kadakas_ui_core::modeling::SubSelection,
    ),
    modeler::EntityInfo,
);

/// A command from the menu, the tool bar or a key.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    New,
    Open,
    Save,
    SaveAs,
    /// File > Import: OBJ, STL, glTF, GLB or IFC (MOD-12).
    Import,
    /// File > Export: STL, OBJ, glTF, GLB, IFC or DXF (MOD-12).
    Export,
    Quit,
    Undo,
    Redo,
    SelectAll,
    SelectNone,
    /// Delete the selected faces and edges, or the selected objects.
    Delete,
    /// Make Group of the selected objects (MOD-07).
    MakeGroup,
    /// Make Component of the selected objects (MOD-07).
    MakeComponent,
    /// Explode the selected groups and components.
    Explode,
    /// Make Unique: the selected component instances get their own definition.
    MakeUnique,
    /// Edit Group or Edit Component: enter the selected one.
    EditSelected,
    /// Close Group or Close Component: leave the one that is open.
    CloseContext,
    /// Close Component after a conflict: keep my edit as a new component.
    CloseKeepMine,
    /// Close Component after a conflict: throw my edit away.
    CloseDiscardMine,
    /// Close Component after a conflict: replace the component with my edit.
    CloseOverwrite,
    /// Intersect Faces (MOD-08): with the whole model, or of the selected faces and edges.
    IntersectFaces(IntersectScope),
    /// A solid operation on the two solids of the selected faces (MOD-09).
    Solid(kadakas_core::surface::SolidOp),
    /// Check Solid (MOD-09): are the selected models closed solids? The open edges get selected.
    CheckSolid,
    /// Swap the order of the two solids of the solid tools.
    SolidSwap,
    /// Soften, Smooth or Unsoften the selected edges (MOD-08).
    EdgeStyle(kadakas_ui_core::modeling::EdgeStyle),
    /// Hide the selected objects, or the selected faces and edges (MOD-05).
    Hide,
    /// Unhide the selected hidden objects, faces and edges.
    Unhide,
    /// Unhide everything that is hidden.
    UnhideAll,
    /// Delete all guides (it asks first when there are more than a few).
    DeleteGuides,
    /// View > Guides.
    ToggleGuides,
    /// View > Hidden Geometry.
    ToggleHiddenGeometry,
    /// View > Hidden Objects.
    ToggleHiddenObjects,
    /// Open the model in the Kadakas app (MOD-05, round B): plants, hardscapes and site models
    /// are edited there.
    OpenInApp,
    /// A tool or command of the tool bar.
    Tool(ModelerTool),
    /// Zoom Window: the next drag of the Zoom tool is a window.
    ZoomWindow,
    Nav(NavAction),
    SetFov(f32),
    Projection(bool),
    ToggleTray,
    /// Switch the camera transition of scenes on or off (MOD-10).
    ToggleSceneAnimation,
    Theme(ThemeChoice),
    Navigation(NavStyle),
    /// A fixed language, or `None` for the language of the system.
    Language(Option<String>),
    About,
    /// Help > Show welcome: the launcher shows its welcome dialog again (PLAT-26).
    ShowWelcome,
}

/// A way to replace or end the model that waits for the answer to the question about unsaved
/// changes (the same question as in `kadakas-app`: save, do not save, or cancel).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Replace {
    /// File > Open, a file that the launcher sent, or a file on the command line.
    Open(PathBuf),
    /// File > New.
    New,
    /// File > Quit, or the close button of the window.
    Quit,
    /// File > Open in the Kadakas App: the app opens the saved file.
    OpenInApp,
}

/// Starts the Kadakas app with a file.
pub type AppStarter = Box<dyn FnMut(&std::path::Path) -> Result<crate::launch::Started, String>>;

/// What the person answers to the question about unsaved changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    SaveAndGo,
    DontSave,
    Cancel,
}

/// More guides than this are deleted only after the person says yes.
pub const FEW_GUIDES: usize = kadakas_core::guide::FEW_GUIDES;

/// A question that waits for a yes or a no. One at a time.
#[derive(Clone, Debug, PartialEq)]
pub enum Confirm {
    /// Delete all guides: how many.
    DeleteGuides(usize),
    /// Resize the model (the Tape Measure, after a measure and a typed length).
    Resize(
        kadakas_ui_core::modeling::Resize,
        kadakas_ui_core::modeling::ResizePlan,
    ),
}

/// The program.
pub struct ModelerApp {
    pub session: Session,
    pub view3d: View3d,
    pub nav: Nav3d,
    pub tools: ToolState,
    /// Modeling (MOD-08): the drawing tools, the selected parts, the inference scene.
    pub edit: Edit3d,
    /// Entity Info for a revision, a selection and a selection of parts.
    info_cache: Option<InfoKey>,
    pub keys: Keymap,
    pub prefs: Prefs,
    /// The settings as they are on disk. A change is written when the two differ.
    prefs_saved: Prefs,
    /// When a save failed, the pause before the next try.
    prefs_retry: kadakas_ui_core::retry::Retry,
    /// Where the settings are written. `None` writes nothing (the tests).
    prefs_dir: Option<PathBuf>,
    theme_applied: Option<ThemeChoice>,
    language_applied: Option<Option<String>>,
    /// The file of the model, if it has one.
    pub path: Option<PathBuf>,
    pub status: String,
    pub gpu_ready: bool,
    fit_pending: bool,
    pub tray: TrayState,
    pub tray_ui: TrayUi,
    /// The Materials panel: the open editor and the question about a used material.
    pub materials: MaterialsPanel,
    /// The materials of the model and where they are used, for a revision.
    rows_cache: Option<(u64, Vec<MaterialRow>)>,
    /// Starts the Kadakas app with a file. The tests put another one in.
    pub starter: AppStarter,
    /// The file while it is open in the Kadakas app too: its path and its last stamp.
    watch: Option<(PathBuf, (u64, Option<std::time::SystemTime>))>,
    watch_next: f64,
    pub about: About,
    /// The windows of File > Import and File > Export (MOD-12).
    pub interop: crate::interop::Interop,
    launch: Launch,
    ask: Box<dyn Ask>,
    /// What waits for the answer about unsaved changes. One question at a time.
    pub pending: Option<Replace>,
    close_confirmed: bool,
    title_shown: Option<String>,
    quit: bool,
    /// The rectangle of the 3D view in the last frame.
    pub canvas_rect: Option<egui::Rect>,
    /// The groups and components that are open for edit (MOD-07).
    pub context: EditContext,
    /// The Tags panel: the chosen row, a rename, a delete that waits for the answer.
    pub tags: TagsState,
    /// The Scenes panel: the chosen row, a rename, the scene that was shown last (MOD-10).
    pub scenes: ScenesState,
    /// The camera transition that runs, with the camera it set last. If the camera is not that
    /// one at the next frame, the person moved it and the transition stops.
    scene_anim: Option<(scene_panel::Transition, kadakas_render::Camera)>,
    /// A question about guides or a resize that waits for the answer.
    pub confirm: Option<Confirm>,
    /// The solid tools take the newer solid as the first one (MOD-09).
    pub solid_swap: bool,
    /// The previews of the solid operations for one revision, selection and order, by operation.
    /// An operation runs on a copy once for each key, whatever the menu asks for.
    solid_hint: std::cell::RefCell<SolidHints>,
    /// How many times a solid operation was run on a copy to make a preview (for the tests).
    pub solid_runs: std::cell::Cell<usize>,
}

/// The key of the previews of the solid tools: the revision, the selection (a hash) and the order.
type SolidKey = (u64, u64, bool);

/// The previews of the solid tools for one key.
#[derive(Default)]
struct SolidHints {
    key: Option<SolidKey>,
    done: Vec<(
        kadakas_core::surface::SolidOp,
        Result<String, modeling::solid::Problem>,
    )>,
}

impl ModelerApp {
    /// A program without a window and a GPU: the tests build it, and `new` builds on it.
    pub fn headless(prefs: Prefs, ask: Box<dyn Ask>) -> ModelerApp {
        let session = Session::default();
        let prefs_language = prefs.language.clone();
        ModelerApp {
            session,
            view3d: View3d::default(),
            nav: Nav3d::new(),
            tools: ToolState::default(),
            edit: Edit3d::default(),
            info_cache: None,
            keys: Profile::sketchup().keymap,
            prefs_saved: prefs.clone(),
            prefs_retry: Default::default(),
            prefs,
            prefs_dir: None,
            theme_applied: None,
            // The tests set the language for their own thread. The window sets it for the process.
            language_applied: Some(prefs_language),
            path: None,
            status: String::new(),
            gpu_ready: false,
            fit_pending: true,
            tray: TrayState::default(),
            tray_ui: TrayUi::default(),
            materials: MaterialsPanel::default(),
            rows_cache: None,
            starter: Box::new(crate::launch::start_app),
            watch: None,
            watch_next: 0.0,
            about: About::default(),
            interop: Default::default(),
            launch: Launch::new(None),
            ask,
            pending: None,
            close_confirmed: false,
            title_shown: None,
            quit: false,
            canvas_rect: None,
            context: EditContext::new(),
            tags: TagsState::default(),
            scenes: ScenesState::default(),
            scene_anim: None,
            confirm: None,
            solid_swap: false,
            solid_hint: std::cell::RefCell::default(),
            solid_runs: std::cell::Cell::new(0),
        }
    }

    /// The program in a window. `files` are the paths on the command line; the first one opens.
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        files: &[PathBuf],
        prefs: Prefs,
        warning: Option<String>,
    ) -> ModelerApp {
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

    /// Where the settings are written (the tests use a temporary folder).
    #[cfg(test)]
    pub fn set_prefs_dir(&mut self, dir: Option<PathBuf>) {
        self.prefs_dir = dir;
    }

    /// A launcher link for the tests (the run lock, the alive stamp and the open requests).
    #[cfg(test)]
    pub fn set_launch(&mut self, launch: Launch) {
        self.launch = launch;
    }

    /// True when the model has changes that nobody saved. The session knows: it is the one place
    /// that the title, the questions and the end of the program ask.
    pub fn dirty(&self) -> bool {
        self.session.has_unsaved_changes()
    }

    /// The name of the file of the model, or the word for a model without a file.
    fn file_name(&self) -> String {
        self.path.as_ref().and_then(|p| p.file_name()).map_or_else(
            || tr!("modeler-untitled"),
            |n| n.to_string_lossy().to_string(),
        )
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

    /// Parks a way to replace the model until the person has answered the question about unsaved
    /// changes. Only one question is open at a time. A second request does not replace the first:
    /// it is dropped, and the status line names both, so that nobody wonders.
    fn ask_replace(&mut self, what: Replace) {
        match &self.pending {
            Some(waiting) if *waiting != what => {
                self.status = tr!(
                    "modeler-open-dropped",
                    dropped = replace_name(&what),
                    waiting = replace_name(waiting)
                );
            }
            Some(_) => {}
            None => self.pending = Some(what),
        }
    }

    /// File > New. With unsaved changes the person is asked first.
    pub fn new_document(&mut self) {
        if self.dirty() {
            self.ask_replace(Replace::New);
        } else {
            self.new_clean();
        }
    }

    fn new_clean(&mut self) {
        self.session = Session::default();
        self.after_replace(None);
    }

    /// Opens a file of the person from File > Open, the command line or the launcher. With unsaved
    /// changes the file waits and the person is asked first: the open model is never replaced
    /// without that answer. Every way to open a file must come here.
    pub fn open_path(&mut self, path: PathBuf) {
        if self.dirty() {
            self.ask_replace(Replace::Open(path));
        } else {
            self.open_clean(path);
        }
    }

    /// [`ModelerApp::open_path`] after the question about unsaved changes.
    fn open_clean(&mut self, path: PathBuf) {
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

    /// The person answered the question about unsaved changes.
    pub fn answer(&mut self, answer: Answer) {
        let Some(what) = self.pending.take() else {
            return;
        };
        let name = replace_name(&what);
        match answer {
            Answer::SaveAndGo => {
                self.save(false);
                if self.dirty() {
                    // Not saved (cancelled or failed): the model stays and nothing else happens.
                    self.status = tr!("modeler-open-not-saved", name = name);
                    return;
                }
                self.go(what);
            }
            Answer::DontSave => self.go(what),
            Answer::Cancel => self.status = tr!("modeler-open-cancelled-by-you", name = name),
        }
    }

    fn go(&mut self, what: Replace) {
        match what {
            Replace::Open(p) => self.open_clean(p),
            Replace::New => self.new_clean(),
            Replace::Quit => {
                self.close_confirmed = true;
                self.quit = true;
            }
            Replace::OpenInApp => self.start_app_now(),
        }
    }

    /// File > Open in the Kadakas App. The app opens the file on disk, so a model with changes
    /// is saved first (the person is asked, as before a New or an Open), and a model that has no
    /// file yet gets one from Save As.
    pub fn open_in_app(&mut self) {
        if self.dirty() {
            self.ask_replace(Replace::OpenInApp);
        } else if self.path.is_none() {
            self.save(true);
            if self.path.is_some() && !self.dirty() {
                self.start_app_now();
            } else {
                self.status = tr!("modeler-app-save-first");
            }
        } else {
            self.start_app_now();
        }
    }

    fn start_app_now(&mut self) {
        let Some(path) = self.path.clone() else {
            self.status = tr!("modeler-app-save-first");
            return;
        };
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        self.status = match (self.starter)(&path) {
            Ok(how) => {
                // From now on the file is open in two programs: watch it for a change by the
                // other one.
                self.watch = Some((path.clone(), file_stamp(&path)));
                match how {
                    crate::launch::Started::Spawned => tr!("modeler-app-started", name = name),
                    crate::launch::Started::Sent => tr!("modeler-app-sent", name = name),
                }
            }
            Err(e) => e,
        };
    }

    /// While the file is open in the Kadakas app too (see [`ModelerApp::open_in_app`]): looks
    /// once a second whether the file changed on disk. A model with no unsaved changes is
    /// loaded again at once; with unsaved changes the status line says so, and a save asks
    /// before it replaces the file (`files::save`).
    fn check_external_change(&mut self, now: f64) {
        if now < self.watch_next {
            return;
        }
        self.watch_next = now + 1.0;
        let Some((path, stamp)) = self.watch.clone() else {
            return;
        };
        if self.path.as_ref() != Some(&path) {
            self.watch = None;
            return;
        }
        let current = file_stamp(&path);
        if current == stamp {
            return;
        }
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        if self.dirty() {
            self.status = tr!("modeler-file-changed-dirty", name = name);
        } else {
            self.open_clean(path.clone());
            self.status = tr!("modeler-file-changed-reloaded", name = name);
        }
        self.watch = Some((path.clone(), file_stamp(&path)));
    }

    /// A document that the launcher sent: a double click on a `.kad` file while this program
    /// runs. It opens like File > Open. The window comes to the front.
    fn open_sent(&mut self, ctx: &egui::Context, first: PathBuf, count: usize) {
        self.open_path(first.clone());
        if count > 1 && self.path.as_ref() == Some(&first) {
            self.status = tr!(
                "modeler-opened-first-of-several",
                name = first.display(),
                count = count - 1
            );
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// The question about unsaved changes, while something waits.
    fn unsaved_dialog(&mut self, ctx: &egui::Context) {
        let Some(what) = self.pending.clone() else {
            return;
        };
        let (question, save) = match &what {
            Replace::Open(_) => (
                tr!("modeler-unsaved-open", name = replace_name(&what)),
                tr!("modeler-unsaved-save-and-open"),
            ),
            Replace::New => (
                tr!("modeler-unsaved-new"),
                tr!("modeler-unsaved-save-and-new"),
            ),
            Replace::Quit => (
                tr!("modeler-unsaved-quit"),
                tr!("modeler-unsaved-save-and-quit"),
            ),
            Replace::OpenInApp => (
                tr!("modeler-unsaved-open-in-app", name = self.file_name()),
                tr!("modeler-unsaved-save-and-open-in-app"),
            ),
        };
        let mut answer = None;
        egui::Window::new(tr!("modeler-unsaved-title"))
            .id(egui::Id::new("win-modeler-unsaved"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(question);
                ui.horizontal(|ui| {
                    if ui.button(save).clicked() {
                        answer = Some(Answer::SaveAndGo);
                    }
                    if ui.button(tr!("modeler-unsaved-dont-save")).clicked() {
                        answer = Some(Answer::DontSave);
                    }
                    if ui.button(tr!("modeler-unsaved-cancel")).clicked() {
                        answer = Some(Answer::Cancel);
                    }
                });
            });
        if let Some(a) = answer {
            self.answer(a);
        }
    }

    /// What follows a new or opened model: nothing is selected, the view shows all of it.
    fn after_replace(&mut self, path: Option<PathBuf>) {
        self.watch = None;
        self.session.selection.clear();
        self.edit.reset();
        self.info_cache = None;
        self.context = EditContext::new();
        self.tags = TagsState::default();
        self.scenes = ScenesState::default();
        self.scene_anim = None;
        self.confirm = None;
        self.path = path;
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
                // Our own save is not a change by the other program.
                if let Some(w) = &mut self.watch
                    && w.0 == saved.path
                {
                    w.1 = file_stamp(&saved.path);
                }
                self.path = Some(saved.path);
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
            Cmd::Import => {
                if let Some(p) = crate::interop::pick_import() {
                    self.interop.start_import(p, self.prefs.import_soften);
                }
            }
            Cmd::Export => {
                if let Some(p) = crate::interop::pick_export(self.path.as_deref()) {
                    self.interop.start_export(p);
                }
            }
            Cmd::OpenInApp => self.open_in_app(),
            Cmd::Quit => {
                if self.dirty() {
                    self.ask_replace(Replace::Quit);
                } else {
                    self.quit = true;
                }
            }
            Cmd::Undo => {
                self.edit.modeling.abort();
                if !self.session.editor.undo() {
                    self.status = tr!("modeler-nothing-to-undo");
                }
                self.after_history();
            }
            Cmd::Redo => {
                self.edit.modeling.abort();
                if !self.session.editor.redo() {
                    self.status = tr!("modeler-nothing-to-redo");
                }
                self.after_history();
            }
            Cmd::MakeGroup | Cmd::MakeComponent | Cmd::Explode | Cmd::MakeUnique => {
                self.structure(&cmd)
            }
            Cmd::EditSelected => self.edit_selected(),
            Cmd::IntersectFaces(scope) => self.intersect(scope),
            Cmd::Solid(op) => self.solid(op),
            Cmd::CheckSolid => self.check_solid(),
            Cmd::SolidSwap => {
                self.solid_swap = !self.solid_swap;
                self.status = if self.solid_swap {
                    tr!("modeler-solid-order-swapped")
                } else {
                    tr!("modeler-solid-order-normal")
                };
            }
            Cmd::EdgeStyle(style) => self.edge_style(style),
            Cmd::Hide => self.set_hidden(true),
            Cmd::Unhide => self.set_hidden(false),
            Cmd::UnhideAll => self.unhide_all(),
            Cmd::DeleteGuides => self.delete_guides(false),
            Cmd::ToggleGuides => {
                let show = &mut self.edit.cache.show;
                show.guides = !show.guides;
            }
            Cmd::ToggleHiddenGeometry => {
                let show = &mut self.edit.cache.show;
                show.hidden_geometry = !show.hidden_geometry;
            }
            Cmd::ToggleHiddenObjects => {
                let show = &mut self.edit.cache.show;
                show.hidden_objects = !show.hidden_objects;
            }
            Cmd::CloseContext
            | Cmd::CloseKeepMine
            | Cmd::CloseDiscardMine
            | Cmd::CloseOverwrite => {
                self.context.validate(self.session.editor.doc());
                let resolve = match cmd {
                    Cmd::CloseKeepMine => Some(kadakas_core::component::Resolve::KeepAsNew),
                    Cmd::CloseDiscardMine => Some(kadakas_core::component::Resolve::Discard),
                    Cmd::CloseOverwrite => Some(kadakas_core::component::Resolve::Overwrite),
                    _ => None,
                };
                match self.context.exit_with(&mut self.session.editor, resolve) {
                    Ok(Some(id)) => {
                        self.session.selection = vec![id];
                        self.status.clear();
                    }
                    Ok(None) => {}
                    Err(e) => self.status = e.to_string(),
                }
            }
            Cmd::Delete => {
                self.edit.delete(&mut self.session);
                if let Some(m) = self.edit.message.take() {
                    self.status = m;
                }
            }
            Cmd::SelectAll => {
                self.edit.sub.clear();
                // Guides are taken only when they are shown, hidden objects only when Hidden
                // Objects shows them (MOD-05).
                let show = self.edit.cache.show;
                let doc = self.session.editor.doc();
                self.session.selection = doc
                    .objects
                    .values()
                    .filter(|o| o.parent.is_none())
                    .filter(|o| show.guides || !kadakas_core::guide::is_guide(o))
                    .filter(|o| {
                        if show.hidden_objects {
                            doc.is_visible_with_hidden(o)
                        } else {
                            doc.is_visible(o)
                        }
                    })
                    .map(|o| o.id)
                    .collect();
            }
            Cmd::SelectNone => {
                self.session.selection.clear();
                self.edit.sub.clear();
            }
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
            Cmd::ToggleSceneAnimation => {
                self.prefs.animate_scenes = !self.prefs.animate_scenes;
                if !self.prefs.animate_scenes {
                    self.scene_anim = None;
                }
            }
            Cmd::Theme(t) => self.prefs.theme = t,
            Cmd::Navigation(n) => self.prefs.navigation = Some(n),
            Cmd::Language(l) => self.prefs.language = l,
            Cmd::About => self.about.open(),
            Cmd::ShowWelcome => {
                if !self.launch.request_welcome() {
                    self.status = tr!("panels-show-welcome-needs-launcher");
                }
            }
        }
    }

    /// What follows an undo or a redo: the selection and the open groups keep only what exists.
    fn after_history(&mut self) {
        let doc = self.session.editor.doc();
        self.session
            .selection
            .retain(|id| doc.object(*id).is_some());
        self.context.validate(doc);
    }

    /// The selected objects that exist.
    fn selected(&self) -> Vec<ObjectId> {
        let doc = self.session.editor.doc();
        self.session
            .selection
            .iter()
            .copied()
            .filter(|id| doc.object(*id).is_some())
            .collect()
    }

    /// True if the structure command makes sense for the selection: any object for Make Group and
    /// Make Component, a group or an instance for Explode, an instance for Make Unique, one group
    /// or instance for Edit, and an open group or component for Close.
    pub fn can_structure(&self, cmd: &Cmd) -> bool {
        let doc = self.session.editor.doc();
        let sel = self.selected();
        let any = |f: fn(&Shape) -> bool| {
            sel.iter()
                .any(|id| doc.object(*id).is_some_and(|o| f(&o.shape)))
        };
        match cmd {
            Cmd::MakeGroup | Cmd::MakeComponent => !sel.is_empty(),
            Cmd::Explode => any(|s| matches!(s, Shape::Group { .. } | Shape::Component { .. })),
            Cmd::MakeUnique => any(|s| matches!(s, Shape::Component { .. })),
            Cmd::EditSelected => {
                sel.len() == 1
                    && any(|s| matches!(s, Shape::Group { .. } | Shape::Component { .. }))
            }
            Cmd::CloseContext
            | Cmd::CloseKeepMine
            | Cmd::CloseDiscardMine
            | Cmd::CloseOverwrite => !self.context.is_root(),
            _ => false,
        }
    }

    /// The surface models that Intersect Faces With Model works on: the selected ones (in the open
    /// group or component), else, with no part selected either, every surface model of the open
    /// level.
    fn intersect_models(&self) -> Vec<ObjectId> {
        let doc = self.session.editor.doc();
        let is_model = |id: &ObjectId| {
            doc.object(*id)
                .is_some_and(|o| matches!(o.shape, Shape::SurfaceModel(_)))
        };
        let chosen: Vec<ObjectId> = self
            .selected()
            .into_iter()
            .filter(|id| is_model(id) && self.context.contains(doc, *id))
            .collect();
        if !chosen.is_empty() || !self.edit.sub.is_empty() {
            return chosen;
        }
        let level = self.context.container();
        doc.objects
            .values()
            .filter(|o| o.parent == level && is_model(&o.id))
            .map(|o| o.id)
            .collect()
    }

    /// True if Intersect Faces has something to work on: selected faces or edges, or (With
    /// Model) a surface model.
    pub fn can_intersect(&self, scope: IntersectScope) -> bool {
        !self.edit.sub.is_empty()
            || (scope == IntersectScope::Model && !self.intersect_models().is_empty())
    }

    /// Whether With Model and With Selection of Intersect Faces are enabled.
    pub fn intersect_flags(&self) -> [bool; 2] {
        [
            self.can_intersect(IntersectScope::Model),
            self.can_intersect(IntersectScope::Selection),
        ]
    }

    /// The two solids of the selected faces, or why the solid tools cannot start (MOD-09).
    pub fn solid_operands(&self) -> Result<modeling::solid::Operands, modeling::solid::Problem> {
        modeling::solid::operands(self.session.editor.doc(), &self.edit.sub, self.solid_swap)
    }

    /// The preview of an operation (what it would do, or why it is refused). It runs the operation
    /// on a copy, once for each revision, selection and order, for each operation.
    fn solid_preview(
        &self,
        op: kadakas_core::surface::SolidOp,
    ) -> Result<String, modeling::solid::Problem> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.edit.sub.hash(&mut h);
        let key = (self.session.editor.revision(), h.finish(), self.solid_swap);
        let mut hints = self.solid_hint.borrow_mut();
        if hints.key != Some(key) {
            hints.key = Some(key);
            hints.done.clear();
        }
        if let Some((_, r)) = hints.done.iter().find(|(o, _)| *o == op) {
            return r.clone();
        }
        self.solid_runs.set(self.solid_runs.get() + 1);
        let r = modeling::solid::preview_result(
            self.session.editor.doc(),
            &self.edit.sub,
            self.solid_swap,
            op,
        );
        hints.done.push((op, r.clone()));
        r
    }

    /// The text for an item of the solid tools: what the operation would do, or why it cannot.
    pub fn solid_hint(&self, op: kadakas_core::surface::SolidOp) -> String {
        match self.solid_preview(op) {
            Ok(t) | Err(modeling::solid::Problem::Refused(t)) => t,
            Err(p) => p.text(),
        }
    }

    /// A solid operation: one undo step. The status line says what was done, or why not. The
    /// preview that the menu made is used, so the operation does not run twice.
    fn solid(&mut self, op: kadakas_core::surface::SolidOp) {
        let known = Some(self.solid_preview(op));
        match modeling::solid::out_with(
            self.session.editor.doc(),
            &self.edit.sub,
            self.solid_swap,
            op,
            known,
        ) {
            Ok(out) => {
                self.edit.run(&mut self.session, out);
                // The pieces have new ids: the selection of parts is cleared.
                self.edit.sub.clear();
                self.status = self.edit.message.take().unwrap_or_default();
            }
            Err(p) => self.status = p.text(),
        }
    }

    /// Check Solid: the status line says whether the selected models are closed solids, and the
    /// edges that make them open are selected.
    fn check_solid(&mut self) {
        let c = modeling::solid::check(
            self.session.editor.doc(),
            &self.edit.sub,
            &self.session.selection,
        );
        if !c.open.is_empty() {
            self.session.selection.clear();
            self.edit.sub.set(c.open);
        }
        self.status = c.text;
    }

    /// Soften, Smooth or Unsoften on the selected edges: one undo step.
    fn edge_style(&mut self, style: kadakas_ui_core::modeling::EdgeStyle) {
        let actions =
            modeling::edge_style_actions(self.session.editor.doc(), &self.edit.sub, style);
        if actions.is_empty() {
            self.status = tr!("modeler-soften-select");
            return;
        }
        self.edit.run(
            &mut self.session,
            modeling::Out {
                actions,
                ..modeling::Out::default()
            },
        );
        self.tray_ui.soften.forget();
        self.status = self.edit.message.take().unwrap_or_default();
    }

    /// A change of the Soften Edges panel: applied to the selection at once; a row of changes is
    /// one undo step.
    fn soften_changed(&mut self) {
        let rev = self.session.editor.revision();
        let out = self
            .tray_ui
            .soften
            .change(self.session.editor.doc(), &self.edit.sub, rev);
        if out.actions.is_empty() {
            self.status = tr!("modeler-soften-select");
            return;
        }
        let (redo, depth) = (out.redo, self.session.editor.undo_depth());
        self.edit.run(&mut self.session, out);
        let after = self.session.editor.revision();
        self.tray_ui
            .soften
            .ran(redo, depth, self.session.editor.undo_depth(), after);
        if let Some(m) = self.edit.message.take() {
            self.status = m;
        }
    }

    /// Intersect Faces: one undo step for all models; the notes go to the status line.
    fn intersect(&mut self, scope: IntersectScope) {
        let models = match scope {
            IntersectScope::Model => self.intersect_models(),
            IntersectScope::Selection => Vec::new(),
        };
        let actions = modeling::intersect_actions(&self.edit.sub, scope, &models);
        if actions.is_empty() {
            self.status = tr!("modeler-intersect-nothing");
            return;
        }
        self.edit.run(
            &mut self.session,
            modeling::Out {
                actions,
                ..modeling::Out::default()
            },
        );
        // The parts that the edit split have new ids: the selection of parts is cleared.
        self.edit.sub.clear();
        self.status = self.edit.message.take().unwrap_or_default();
    }

    // ---- guides, hide and tags (MOD-05) --------------------------------------------------------

    /// Delete Guides: all guides in one undo step. More than a few ask first.
    pub fn delete_guides(&mut self, confirmed: bool) {
        let ids = self.session.editor.doc().guide_ids();
        if ids.is_empty() {
            self.status = tr!("modeler-guides-none");
            return;
        }
        if ids.len() > FEW_GUIDES && !confirmed {
            self.confirm = Some(Confirm::DeleteGuides(ids.len()));
            return;
        }
        let count = ids.len();
        match self.session.editor.execute(&Command::DeleteObjects { ids }) {
            Ok(_) => {
                self.status = tr!("modeler-guides-deleted", count = count);
                self.after_history();
            }
            Err(e) => self.status = tr!("modeler-guides-failed", reason = e.to_string()),
        }
    }

    /// The faces and edges of the selection of parts, by model.
    fn parts_by_model(
        &self,
    ) -> std::collections::BTreeMap<
        ObjectId,
        (
            Vec<kadakas_core::surface::FaceId>,
            Vec<kadakas_core::surface::EdgeId>,
        ),
    > {
        let mut by: std::collections::BTreeMap<_, (Vec<_>, Vec<_>)> = Default::default();
        for (m, f) in self.edit.sub.faces() {
            by.entry(m).or_default().0.push(f);
        }
        for (m, e) in self.edit.sub.edges() {
            by.entry(m).or_default().1.push(e);
        }
        by
    }

    /// True if Hide has something to work on: selected objects, faces or edges.
    pub fn can_hide(&self) -> bool {
        !self.selected().is_empty() || !self.edit.sub.is_empty()
    }

    /// True if Unhide has something to work on: a selected hidden object, face or edge.
    pub fn can_unhide(&self) -> bool {
        let doc = self.session.editor.doc();
        self.selected().iter().any(|id| {
            doc.object(*id)
                .is_some_and(kadakas_core::visibility::is_hidden)
        }) || self.parts_by_model().into_iter().any(|(m, (fs, es))| {
            modeling::model_of(doc, m).is_some_and(|s| {
                fs.iter().any(|f| s.face_hidden(*f)) || es.iter().any(|e| s.edge_hidden(*e))
            })
        })
    }

    /// Hide (or Unhide) the selected objects, and the selected faces and edges, in one undo step.
    fn set_hidden(&mut self, hidden: bool) {
        let doc = self.session.editor.doc();
        let mut commands: Vec<Command> = Vec::new();
        let mut count = 0usize;
        for (id, (faces, edges)) in self.parts_by_model() {
            let Some(m) = modeling::model_of(doc, id) else {
                continue;
            };
            let faces: Vec<_> = faces
                .into_iter()
                .filter(|f| m.face_hidden(*f) != hidden)
                .collect();
            let edges: Vec<_> = edges
                .into_iter()
                .filter(|e| m.edge_hidden(*e) != hidden)
                .collect();
            count += faces.len() + edges.len();
            if !faces.is_empty() || !edges.is_empty() {
                commands.push(Command::EditSurface {
                    id,
                    edit: kadakas_core::surface::Edit::SetHidden {
                        faces,
                        edges,
                        hidden,
                    },
                });
            }
        }
        let objects = self.selected();
        if let Some(c) = kadakas_core::visibility::set_hidden_command(doc, &objects, hidden) {
            count += doc
                .with_members(&objects)
                .into_iter()
                .filter(|id| {
                    doc.object(*id)
                        .is_some_and(|o| kadakas_core::visibility::is_hidden(o) != hidden)
                })
                .count();
            commands.push(c);
        }
        if commands.is_empty() {
            self.status = if hidden {
                tr!("modeler-hide-nothing")
            } else {
                tr!("modeler-unhide-nothing")
            };
            return;
        }
        match self.session.editor.execute(&Command::Batch { commands }) {
            Ok(_) => {
                if hidden {
                    // What was hidden is not selected any more.
                    self.session.selection.clear();
                    self.edit.sub.clear();
                    self.status = tr!("modeler-hid", count = count);
                } else {
                    self.status = tr!("modeler-unhid", count = count);
                }
                self.after_history();
            }
            Err(e) => self.status = tr!("modeler-hide-failed", reason = e.to_string()),
        }
    }

    /// Unhide All: every hidden object, face and edge of the model, in one undo step.
    fn unhide_all(&mut self) {
        let Some((command, count)) =
            kadakas_core::visibility::unhide_all_command(self.session.editor.doc())
        else {
            self.status = tr!("modeler-unhide-all-nothing");
            return;
        };
        match self.session.editor.execute(&command) {
            Ok(_) => self.status = tr!("modeler-unhid", count = count),
            Err(e) => self.status = tr!("modeler-hide-failed", reason = e.to_string()),
        }
    }

    /// The objects that the tag selector of Entity Info changes: the selected objects. The tag
    /// belongs to the object, so a selection of faces and edges changes nothing here.
    fn tag_targets(&self) -> Vec<ObjectId> {
        self.selected()
    }

    #[cfg(test)]
    pub fn tag_targets_for_test(&self) -> Vec<ObjectId> {
        self.tag_targets()
    }

    /// Runs what the Tags panel asked for.
    pub fn apply_tags(&mut self, out: tags_panel::Outcome) {
        if let Some(m) = out.message {
            self.status = m;
        }
        // One batch: a command that fails leaves nothing applied, and it is one undo step.
        let command = match out.commands.len() {
            0 => None,
            1 => out.commands.into_iter().next(),
            _ => Some(Command::Batch {
                commands: out.commands,
            }),
        };
        if let Some(c) = command
            && let Err(e) = self.session.editor.execute(&c)
        {
            self.status = tr!("modeler-tags-failed", reason = e.to_string());
            return;
        }
        if let Some(m) = out.after {
            self.status = m;
        }
        self.after_history();
    }

    /// The answer to a question about guides or a resize.
    pub fn answer_confirm(&mut self, yes: bool) {
        let Some(what) = self.confirm.take() else {
            return;
        };
        if !yes {
            return;
        }
        match what {
            Confirm::DeleteGuides(_) => self.delete_guides(true),
            Confirm::Resize(r, plan) => self.resize_model(r, &plan),
        }
    }

    /// A resize that the Tape Measure asked for: the plan is found first. Locked layers and grey
    /// tags refuse it, with their names, before any question. Else the question lists what happens.
    fn ask_resize(&mut self, r: kadakas_ui_core::modeling::Resize) {
        let plan = modeling::resize_plan(self.session.editor.doc(), self.context.container());
        if plan.ids.is_empty() {
            self.status = tr!("modeler-resize-nothing");
        } else if !plan.locked_layers.is_empty() {
            self.status = tr!(
                "modeler-resize-locked",
                layers = plan.locked_layers.join(", ")
            );
        } else if plan.grey_objects > 0 {
            self.status = tr!("modeler-resize-grey", count = plan.grey_objects);
        } else {
            self.confirm = Some(Confirm::Resize(r, plan));
        }
    }

    /// Resize the model: the objects of the plan are scaled about the first point of the measure.
    /// One undo step. The heights of plan objects come from their layers and stay.
    fn resize_model(&mut self, r: kadakas_ui_core::modeling::Resize, plan: &modeling::ResizePlan) {
        let command = modeling::resize_command(plan, &r);
        match self.session.editor.execute(&command) {
            Ok(out) => {
                let mut text = tr!(
                    "modeler-resize-done",
                    factor = kadakas_units::num(r.factor, 4)
                );
                if !out.notes.is_empty() {
                    text = format!("{text} {}", out.notes.join("; "));
                }
                self.status = text;
                self.after_history();
            }
            Err(e) => self.status = tr!("modeler-resize-failed", reason = e.to_string()),
        }
    }

    /// The question that waits, as a window with Yes and No.
    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(what) = self.confirm.clone() else {
            return;
        };
        let (title, question, yes, no) = match &what {
            Confirm::DeleteGuides(count) => (
                tr!("modeler-guides-delete-title"),
                tr!("modeler-guides-delete-question", count = *count),
                tr!("modeler-guides-delete-yes"),
                tr!("modeler-tags-cancel"),
            ),
            Confirm::Resize(r, plan) => {
                let u = kadakas_units::current();
                (
                    tr!("modeler-resize-title"),
                    tr!(
                        "modeler-resize-question",
                        measured = u.length_text(r.measured),
                        typed = u.length_text(r.typed),
                        factor = kadakas_units::num(r.factor, 4),
                        count = plan.scaled,
                        flat = plan.plan_objects
                    ),
                    tr!("modeler-resize-yes"),
                    tr!("modeler-resize-no"),
                )
            }
        };
        let mut answer = None;
        egui::Window::new(title)
            .id(egui::Id::new("win-modeler-confirm"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(question);
                ui.horizontal(|ui| {
                    if ui.button(yes).clicked() {
                        answer = Some(true);
                    }
                    if ui.button(no).clicked() {
                        answer = Some(false);
                    }
                });
            });
        if let Some(a) = answer {
            self.answer_confirm(a);
        }
    }

    /// A name for a new component that no component has.
    fn new_component_name(&self) -> String {
        let doc = self.session.editor.doc();
        (1..)
            .map(|n| tr!("modeler-component-name", n = n))
            .find(|name| {
                !doc.resources
                    .iter()
                    .any(|r| r.name.eq_ignore_ascii_case(name) && r.body.type_name() == "symbol")
            })
            .expect("an endless range")
    }

    /// Make Group, Make Component, Explode and Make Unique on the selection. One undo step each.
    /// Only objects in the open group or component can be chosen.
    fn structure(&mut self, cmd: &Cmd) {
        self.context.validate(self.session.editor.doc());
        let doc = self.session.editor.doc();
        let sel: Vec<ObjectId> = self
            .selected()
            .into_iter()
            .filter(|id| self.context.contains(doc, *id) && Some(*id) != self.context.container())
            .collect();
        let of_kind = |f: fn(&Shape) -> bool| -> Vec<ObjectId> {
            sel.iter()
                .copied()
                .filter(|id| doc.object(*id).is_some_and(|o| f(&o.shape)))
                .collect()
        };
        let command = match cmd {
            Cmd::MakeGroup => Command::MakeGroup {
                model: None,
                faces: vec![],
                edges: vec![],
                objects: sel.clone(),
                name: None,
            },
            Cmd::MakeComponent => Command::MakeComponent {
                name: self.new_component_name(),
                model: None,
                faces: vec![],
                edges: vec![],
                objects: sel.clone(),
                replace: true,
            },
            Cmd::Explode => Command::Explode {
                ids: of_kind(|s| matches!(s, Shape::Group { .. } | Shape::Component { .. })),
            },
            _ => Command::MakeUnique {
                ids: of_kind(|s| matches!(s, Shape::Component { .. })),
            },
        };
        if sel.is_empty() {
            self.status = tr!("modeler-nothing-selected");
            return;
        }
        match self.session.editor.execute(&command) {
            Ok(out) => {
                // What was made is selected: the group or the instance.
                let made = match cmd {
                    Cmd::MakeGroup => out.created.first().copied(),
                    Cmd::MakeComponent => out.created.get(1).copied(),
                    _ => None,
                };
                self.session.selection = made.map(|u| vec![ObjectId(u)]).unwrap_or_default();
                self.status.clear();
            }
            Err(e) => self.status = e.to_string(),
        }
        self.after_history();
    }

    /// Edit Group or Edit Component: enters the selected group or instance.
    fn edit_selected(&mut self) {
        let sel = self.selected();
        let [id] = sel.as_slice() else {
            self.status = tr!("modeler-select-one-to-edit");
            return;
        };
        match self.context.enter(&mut self.session.editor, *id) {
            Ok(()) => {
                self.session.selection.clear();
                self.status.clear();
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    #[cfg(test)]
    /// True when the program is going to end (the answer to a question, or File > Quit).
    pub fn quitting(&self) -> bool {
        self.quit
    }

    #[cfg(test)]
    /// True when the modeler runs the key action. This is the set of actions of the modeler: the
    /// tools, the navigation, the files, undo and redo, and the selection.
    pub fn runs(action: Action) -> bool {
        ModelerApp::cmd_of(action).is_some()
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
            Action::Delete => Cmd::Delete,
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
        let drawing = kadakas_ui_core::modeling::takes_values(self.tools.active());
        let texts: Vec<String> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Text(t) => Some(t.clone()),
                    _ => None,
                })
                .collect()
        });
        if drawing {
            for t in &texts {
                for c in t.chars() {
                    self.edit.modeling.type_char(c);
                }
            }
        }
        for (key, physical, mods, repeat) in events {
            self.edit.follow(self.tools.active());
            if self.measurement_key(key, mods, drawing, size) {
                continue;
            }
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

    /// A key that belongs to the Measurements box or to the tool in progress. True if it was
    /// used (the key action is then not run). Characters come as text events; this decides which
    /// keys must not also run their action (digits do not choose standard views, the letters of a
    /// unit do not choose tools).
    fn measurement_key(
        &mut self,
        key: egui::Key,
        mods: egui::Modifiers,
        drawing: bool,
        size: glam::Vec2,
    ) -> bool {
        use egui::Key;
        let plain = !mods.command && !mods.ctrl && !mods.alt;
        let typed = !self.edit.modeling.typed().is_empty();
        match key {
            Key::Enter if drawing => {
                self.enter_typed(size);
                return true;
            }
            Key::Backspace | Key::Delete if drawing && typed => {
                self.edit.modeling.backspace();
                return true;
            }
            Key::Escape => {
                if self.edit.modeling.cancel() {
                    self.status.clear();
                    return true;
                }
            }
            Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown
                if self.edit.arrow(key) =>
            {
                return true;
            }
            _ => {}
        }
        if !(drawing && plain) {
            return false;
        }
        let value_key = matches!(
            key,
            Key::Num0
                | Key::Num1
                | Key::Num2
                | Key::Num3
                | Key::Num4
                | Key::Num5
                | Key::Num6
                | Key::Num7
                | Key::Num8
                | Key::Num9
                | Key::Period
                | Key::Comma
                | Key::Semicolon
                | Key::Colon
                | Key::Minus
                | Key::Slash
                | Key::Quote
                | Key::OpenBracket
                | Key::CloseBracket
        );
        let letter = key.name().len() == 1 && key.name().chars().all(|c| c.is_ascii_alphabetic());
        value_key || (letter && typed)
    }

    /// Enter in the Measurements box.
    fn enter_typed(&mut self, _size: glam::Vec2) {
        let rev = self.session.editor.revision();
        let selected = self.session.selection.first().copied();
        let out = {
            let doc = self.session.editor.doc();
            let scenes = self.edit.cache.get(doc, rev);
            let world = kadakas_ui_core::modeling::World { doc, scenes };
            self.edit.modeling.enter(&world, selected, rev)
        };
        self.edit.run(&mut self.session, out);
        if let Some(m) = self.edit.message.take() {
            self.status = m;
        }
    }

    // ---- settings ----------------------------------------------------------------------------

    /// Reads `modeler.json` and `prefs.json` again (the welcome dialog stored choices). The
    /// language, the theme and the rest apply in the next frame.
    fn reload_prefs(&mut self, ctx: &egui::Context) {
        let Some(dir) = self.prefs_dir.clone() else {
            return;
        };
        let (reloaded, warning) = prefs::load_from(&dir);
        // A change that could not be saved stays on top of what the files say now.
        let merged = prefs::with_pending(&reloaded, &self.prefs_saved, &self.prefs);
        // The language and the units are process wide: they are applied again only when they
        // changed.
        if prefs::locale_differs(&self.prefs, &merged) {
            self.language_applied = None;
        }
        self.prefs = merged;
        self.prefs_saved = reloaded;
        self.prefs_retry.succeeded();
        self.theme_applied = None;
        if let Some(w) = warning {
            self.status = w;
        }
        ctx.request_repaint();
    }

    /// Makes the theme and the language of the settings current, and writes a change to disk.
    fn apply_prefs(&mut self, ctx: &egui::Context) {
        if self.launch.take_settings_changed(ctx.input(|i| i.time)) {
            self.reload_prefs(ctx);
        }
        if self.language_applied.as_ref() != Some(&self.prefs.language) {
            self.prefs.apply_language(&prefs::system_languages());
            self.language_applied = Some(self.prefs.language.clone());
            ctx.request_repaint();
        }
        if self.theme_applied != Some(self.prefs.theme) {
            ctx.set_theme(self.prefs.theme.preference());
            self.theme_applied = Some(self.prefs.theme);
        }
        // A failed save keeps the changes unsaved and is tried again after a pause that grows.
        // The status line says what failed at each try.
        let now = ctx.input(|i| i.time);
        if self.prefs != self.prefs_saved && self.prefs_retry.due(now) {
            let result = match &self.prefs_dir {
                Some(dir) => prefs::save_changes_to(dir, &self.prefs_saved, &self.prefs),
                None => Ok(()),
            };
            match result {
                Ok(()) => {
                    self.prefs_saved = self.prefs.clone();
                    self.prefs_retry.succeeded();
                }
                Err(e) => {
                    self.status = e;
                    self.prefs_retry.failed(now);
                }
            }
        }
        // An unsaved change wakes the window for the next try, though nothing else happens.
        if self.prefs != self.prefs_saved {
            let wait = self.prefs_retry.remaining(now).max(0.05);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
    }

    /// The program ends and the settings are still unsaved: one more try after a short wait. A
    /// second failure goes to stderr and to `save-failed.log` in the state folder.
    fn save_prefs_last_try(&mut self) {
        let log = kadakas_sys::dirs::state_dir();
        self.save_prefs_last_try_to(std::time::Duration::from_millis(250), log.as_deref());
    }

    pub(crate) fn save_prefs_last_try_to(
        &mut self,
        pause: std::time::Duration,
        log: Option<&std::path::Path>,
    ) {
        if self.prefs == self.prefs_saved {
            return;
        }
        let Some(dir) = self.prefs_dir.clone() else {
            return;
        };
        let (saved, now) = (self.prefs_saved.clone(), self.prefs.clone());
        let result = kadakas_sys::savelog::final_attempt(
            || prefs::save_changes_to(&dir, &saved, &now),
            pause,
            "the settings",
            log,
        );
        if result.is_ok() {
            self.prefs_saved = self.prefs.clone();
        }
    }

    /// Entity Info for the selection. It is made again only when the revision or the selection
    /// changes (a closed-volume check of a big model is not cheap).
    fn info(&mut self, scene: &kadakas_core::mesh::Scene) -> modeler::EntityInfo {
        let key = (
            self.session.editor.revision(),
            self.session.selection.clone(),
            self.edit.sub.clone(),
        );
        if let Some((k, info)) = &self.info_cache
            && *k == key
        {
            return info.clone();
        }
        let info = modeler::entity_info_with_parts(
            self.session.editor.doc(),
            scene,
            &self.session.selection,
            &self.edit.sub,
        );
        self.info_cache = Some((key, info.clone()));
        info
    }

    /// The materials of the model with where they are used, for the revision of the document.
    fn material_rows(&mut self) -> Vec<MaterialRow> {
        let rev = self.session.editor.revision();
        if self.rows_cache.as_ref().is_none_or(|(r, _)| *r != rev) {
            let rows = kadakas_ui_core::materials::rows(self.session.editor.doc());
            self.rows_cache = Some((rev, rows));
        }
        self.rows_cache
            .as_ref()
            .map(|c| c.1.clone())
            .unwrap_or_default()
    }

    /// Runs what a panel of the tray asked for.
    pub fn tray_action(&mut self, a: TrayAction) {
        match a {
            TrayAction::OpenInApp => self.open_in_app(),
            TrayAction::Soften => {
                self.edit.soften_preview.clear();
                self.soften_changed();
            }
            TrayAction::SoftenPreview => {
                self.edit.soften_preview = self
                    .tray_ui
                    .soften
                    .preview(self.session.editor.doc(), &self.edit.sub);
            }
            TrayAction::CurveSegments {
                model,
                curve,
                segments,
            } => {
                self.status = match kadakas_ui_core::modeling::curve::set_segments(
                    &mut self.session.editor,
                    model,
                    curve,
                    segments,
                ) {
                    Ok(notes) => {
                        let done = tr!("modeler-curve-segments-set", segments = segments);
                        std::iter::once(done)
                            .chain(notes)
                            .collect::<Vec<_>>()
                            .join(" ")
                    }
                    Err(e) => tr!("modeler-edit-failed", reason = e),
                };
            }
            TrayAction::Material(req) => {
                let result = self.materials.apply(
                    &mut self.session.editor,
                    &mut self.edit.modeling.paint.bucket,
                    req,
                );
                match result {
                    Ok(msg) if !msg.is_empty() => self.status = msg,
                    Ok(_) => {}
                    Err(e) => self.status = e,
                }
            }
        }
    }

    // ---- scenes and styles (MOD-10) ----------------------------------------------------------

    /// Runs a command of the Scenes or Styles panel. `now` is the time of the frame in seconds
    /// and `size` the size of the 3D view. Every command is one undo step.
    pub fn scene_action(&mut self, a: scenes_panel::Action, now: f64, size: glam::Vec2) {
        use scenes_panel::Action;
        let origin = self.session.scene().origin;
        let doc = self.session.editor.doc();
        let camera = self.nav.camera;
        let mut added = None;
        let result: Result<(Option<Command>, String), scene_panel::Problem> = match &a {
            Action::Add => {
                let name = scene_panel::new_name(doc);
                added = Some(name.clone());
                scene_panel::add(doc, &name, &camera, origin)
                    .map(|c| (Some(c), tr!("modeler-scenes-added", name = name)))
            }
            Action::Update(n) => scene_panel::update(doc, n, &camera, origin)
                .map(|c| (Some(c), tr!("modeler-scenes-updated", name = n.clone()))),
            Action::Rename(n, new) => scene_panel::rename(doc, n, new).map(|c| {
                (
                    Some(c),
                    tr!("modeler-scenes-renamed", old = n.clone(), name = new.trim()),
                )
            }),
            Action::Delete(n) => scene_panel::delete(doc, n)
                .map(|c| (Some(c), tr!("modeler-scenes-deleted", name = n.clone()))),
            Action::Move(n, delta) => scene_panel::move_by(doc, n, *delta).map(|c| {
                let place = doc
                    .drawing
                    .saved_views
                    .iter()
                    .position(|v| v.name.eq_ignore_ascii_case(n))
                    .map_or(0, |i| (i as isize + delta).max(0) as usize + 1);
                (
                    c,
                    tr!("modeler-scenes-moved", name = n.clone(), place = place),
                )
            }),
            Action::Style(n) => {
                let name = n.clone();
                Ok((
                    scene_panel::use_style(doc, n),
                    tr!(
                        "modeler-style-changed",
                        name = scenes_panel::style_name_label(&name)
                    ),
                ))
            }
            Action::Activate(n) => {
                self.show_scene(n, now, size);
                return;
            }
            Action::Animate(on) => {
                self.prefs.animate_scenes = *on;
                if !*on {
                    self.scene_anim = None;
                }
                return;
            }
        };
        let (command, message) = match result {
            Ok(r) => r,
            Err(p) => {
                self.status = scenes_panel::problem_text(p);
                return;
            }
        };
        if let Some(c) = command {
            if self.session.editor.execute(&c).is_err() {
                self.status = match a {
                    Action::Style(_) => tr!("modeler-style-failed"),
                    _ => tr!("modeler-scenes-failed"),
                };
                return;
            }
            self.status = message;
        }
        // The panel follows what was done.
        match a {
            Action::Add => {
                if let Some(name) = added {
                    self.scenes.active = Some(name.clone());
                    self.scenes.begin_rename(&name);
                }
            }
            Action::Update(n) => self.scenes.active = Some(n),
            Action::Rename(old, new) => {
                let new = new.trim().to_string();
                if self
                    .scenes
                    .active
                    .as_deref()
                    .is_some_and(|x| x.eq_ignore_ascii_case(&old))
                {
                    self.scenes.active = Some(new.clone());
                }
                self.scenes.chosen = Some(new);
            }
            Action::Delete(n)
                if self
                    .scenes
                    .chosen
                    .as_deref()
                    .is_some_and(|x| x.eq_ignore_ascii_case(&n)) =>
            {
                self.scenes.chosen = None;
            }
            _ => {}
        }
        self.after_history();
    }

    /// Shows a scene: the tags, layers and style in one undo step, and the camera, moved in a
    /// short animation unless the person switched that off.
    fn show_scene(&mut self, name: &str, now: f64, size: glam::Vec2) {
        let origin = self.session.scene().origin;
        let act = match scene_panel::activate(self.session.editor.doc(), name, origin) {
            Ok(a) => a,
            Err(p) => {
                self.status = scenes_panel::problem_text(p);
                return;
            }
        };
        if self.session.editor.execute(&act.command).is_err() {
            self.status = tr!("modeler-scenes-failed");
            return;
        }
        self.scenes.active = Some(name.to_string());
        // The restore can change which layers show, and so the origin of the scene: the saved
        // camera is in drawing coordinates, so it is placed again for the new origin.
        let origin = self.session.scene().origin;
        let act = scene_panel::activate(self.session.editor.doc(), name, origin).unwrap_or(act);
        let goal = match act.goal {
            scene_panel::Goal::Camera(c) => Some(c),
            scene_panel::Goal::Standard(view, projection) => {
                let frame = self.frame(size);
                let mut probe = Nav3d::new();
                probe.camera = self.nav.camera;
                probe.standard_view(view, &frame);
                if let Some(p) = projection {
                    probe.set_projection(p);
                }
                Some(probe.camera)
            }
            scene_panel::Goal::Stay => None,
        };
        self.status = match goal {
            Some(_) => tr!("modeler-scenes-shown", name = name),
            None => tr!("modeler-scenes-view-only", name = name),
        };
        if let Some(to) = goal {
            if self.prefs.animate_scenes && to != self.nav.camera {
                let t = scene_panel::Transition::new(self.nav.camera, to, now);
                self.scene_anim = Some((t, self.nav.camera));
            } else {
                self.scene_anim = None;
                self.nav.camera = to;
            }
        }
        self.after_history();
    }

    /// One step of the camera transition. It stops when the camera is not the one that the
    /// transition set last: the person moved it.
    fn step_scene_animation(&mut self, ctx: &egui::Context, now: f64) {
        let Some((t, last)) = self.scene_anim else {
            return;
        };
        if self.nav.camera != last {
            self.scene_anim = None;
            return;
        }
        let (camera, done) = t.at(now);
        self.nav.camera = camera;
        if done {
            self.scene_anim = None;
        } else {
            self.scene_anim = Some((t, camera));
            ctx.request_repaint();
        }
    }

    /// True while a scene transition moves the camera.
    #[cfg(test)]
    pub fn scene_animating(&self) -> bool {
        self.scene_anim.is_some()
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
        let info = self.info(&scene);
        let measurement = self.edit.measurement().unwrap_or_else(|| {
            modeler::measurement(&info, self.tools.active(), self.nav.fov_deg())
        });
        if let Some(m) = self.edit.message.take() {
            self.status = m;
        }

        // While the Tape Measure runs, the status line shows the distance too.
        let status_line = if self.tools.active() == ModelerTool::TapeMeasure
            && self.edit.modeling.in_progress()
            && !measurement.text().is_empty()
        {
            tr!("modeler-tape-distance", value = measurement.text())
        } else {
            self.status.clone()
        };
        egui::Panel::bottom("status").show(ui, |ui| {
            crate::statusbar::show(ui, &status_line, self.tools.active(), &measurement);
        });
        let mut tag_out: Vec<tags_panel::Outcome> = Vec::new();
        let mut scene_out: Vec<scenes_panel::Action> = Vec::new();
        let mut tray_actions: Vec<TrayAction> = Vec::new();
        if self.prefs.tray {
            let rows = self.material_rows();
            let missing =
                kadakas_ui_core::materials::missing_defaults(self.session.editor.doc()).len();
            let targets = self.tag_targets();
            let scene_origin = self.session.scene().origin;
            egui::Panel::right("tray")
                .default_size(tray::WIDTH)
                .size_range(180.0..=420.0)
                .show(ui, |ui| {
                    let data = TrayData {
                        info: &info,
                        doc: self.session.editor.doc(),
                        rows: &rows,
                        missing,
                        panel: &self.materials,
                        bucket: self.edit.modeling.paint.bucket,
                        edges_selected: !self.edit.sub.is_empty(),
                    };
                    let view = tags_panel::View {
                        doc: self.session.editor.doc(),
                        objects: &targets,
                        rev: self.session.editor.revision(),
                        parts: !self.edit.sub.is_empty(),
                    };
                    let tags_state = &mut self.tags;
                    let scenes_state = &mut self.scenes;
                    let doc = self.session.editor.doc();
                    let scene_rows = scene_panel::rows(doc, scenes_state.active.as_deref());
                    let style_rows = scene_panel::styles(doc);
                    let animating = self.scene_anim.is_some();
                    let modified = scenes_state.active.as_deref().is_some_and(|n| {
                        scene_panel::is_modified(doc, n, &self.nav.camera, scene_origin)
                    });
                    let animate = self.prefs.animate_scenes;
                    tray_actions = tray::show(
                        ui,
                        &mut self.tray,
                        &mut self.tray_ui,
                        &data,
                        &mut |ui, slot| {
                            tag_out.push(tags_panel::show(ui, tags_state, slot, &view));
                        },
                        &mut |ui, slot| match slot {
                            tray::ScenesSlot::Scenes => {
                                let v = scenes_panel::SceneView {
                                    rows: &scene_rows,
                                    animating,
                                    modified,
                                    animate,
                                };
                                scene_out.extend(scenes_panel::show(ui, scenes_state, &v));
                            }
                            tray::ScenesSlot::Styles => {
                                scene_out.extend(scenes_panel::styles(ui, &style_rows));
                            }
                        },
                    );
                });
        }
        for out in tag_out {
            self.apply_tags(out);
        }
        for a in tray_actions {
            self.tray_action(a);
        }
        let mut shown = canvas::Shown::default();
        let intersect = self.intersect_flags();
        let edges = !self.edit.sub.is_empty();
        let hide = [self.can_hide(), self.can_unhide()];
        egui::CentralPanel::no_frame().show(ui, |ui| {
            shown = canvas::show(
                ui,
                Canvas {
                    session: &mut self.session,
                    view3d: &mut self.view3d,
                    nav: &mut self.nav,
                    tools: &mut self.tools,
                    edit: &mut self.edit,
                    gpu_ready: self.gpu_ready,
                    fit_pending: &mut self.fit_pending,
                    intersect,
                    edges,
                    hide,
                    nav_style: self.prefs.nav_style(),
                },
            );
        });
        cmds.extend(shown.command.take());
        self.canvas_rect = shown.rect;
        let size = shown
            .rect
            .map_or(size, |r| glam::Vec2::new(r.width(), r.height()));

        self.handle_keys(&ctx, size);
        for cmd in cmds {
            self.run(cmd, size);
        }
        let now = ctx.input(|i| i.time);
        for action in scene_out {
            self.scene_action(action, now, size);
        }
        self.step_scene_animation(&ctx, now);
        self.edit.follow(self.tools.active());
        // A view that has rested becomes an entry of the history (Previous and Next).
        let now = ctx.input(|i| i.time);
        if self.nav.observe(now) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                kadakas_ui_core::view_history::SETTLE,
            ));
        }
        // A document that the launcher sent, and the question about unsaved changes.
        let sent = self.launch.take_open(now);
        if let Some(first) = sent.first() {
            self.open_sent(&ctx, first.clone(), sent.len());
        }
        if self.launch.is_launched() {
            // Also the wait for a document that the launcher sends.
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        self.check_external_change(now);
        self.unsaved_dialog(&ctx);
        // A resize that the Tape Measure asked for becomes a question.
        if let Some(r) = self.edit.resize_request.take() {
            self.ask_resize(r);
        }
        self.confirm_dialog(&ctx);
        self.interop_windows(&ctx);
        self.about.show(&ctx);
        self.finish_frame(&ctx);
    }

    /// The windows of File > Import and File > Export. A finished import frames the new model.
    fn interop_windows(&mut self, ctx: &egui::Context) {
        let done = self.interop.show(
            ctx,
            &mut self.session,
            &mut self.status,
            &mut self.prefs.import_soften,
        );
        if done == crate::interop::Outcome::Imported {
            self.session.selection.clear();
            self.edit.reset();
            self.info_cache = None;
            self.fit_pending = true;
        }
    }

    /// The title, the close request and the end of the program.
    fn finish_frame(&mut self, ctx: &egui::Context) {
        let title = self.title();
        if self.title_shown.as_ref() != Some(&title) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title_shown = Some(title);
        }
        // The close button with unsaved changes: the window stays, and the person is asked.
        let close_asked = ctx.input(|i| i.viewport().close_requested());
        if close_asked && !self.close_confirmed {
            if self.dirty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.ask_replace(Replace::Quit);
            } else {
                self.close_confirmed = true;
            }
        }
        if self.quit {
            self.quit = false;
            self.close_confirmed = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// The program ends: the launcher gets the clean-exit mark, as the last thing.
    pub fn on_exit(&mut self) {
        self.save_prefs_last_try();
        self.launch.finish();
    }
}

/// The size and the time of the last change of a file: what tells that another program wrote it.
fn file_stamp(path: &std::path::Path) -> (u64, Option<std::time::SystemTime>) {
    std::fs::metadata(path).map_or((0, None), |m| (m.len(), m.modified().ok()))
}

/// What the questions and the status line call a way to replace the model: the file name, a new
/// model, or the end of the program.
fn replace_name(what: &Replace) -> String {
    match what {
        Replace::Open(p) => p.file_name().map_or_else(
            || p.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        ),
        Replace::New => tr!("modeler-open-new-model"),
        Replace::Quit => tr!("modeler-open-quit"),
        Replace::OpenInApp => tr!("modeler-open-in-app-name"),
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
