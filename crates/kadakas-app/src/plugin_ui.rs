//! WebAssembly plug-ins in the app (PLAT-10): the Plug-ins window, the trust dialog, the parameter
//! form of plug-in objects in Object Info, and the "Plug-ins" tool set.
//!
//! **The user decides about trust, and only here.** The dialog shows the name, the version, the
//! author, the SHA-256, the size and what the module can do, and it has Trust and Cancel. The
//! function that trusts (`kadakas_api::trust_wasm_plugin`) is called from this file only, with the
//! hash that the dialog showed. No API method trusts a module, so an agent cannot trust one.
//!
//! The logic of the form and of a tool run is in `kadakas-ui-core` (`plugin_form`, `plugin_tool`).
//! This file draws it and calls the `plugins.*` methods, so the app and an agent use the same path
//! and every change is one undo step with the change origin `script`.

use egui::{Color32, Ui};
use kadakas_api::Session;
use kadakas_core::model::{ObjectId, Shape};
use kadakas_l10n::tr;
use kadakas_ui_core::plugin_form::{Change, Field, Form, Kind};
use kadakas_ui_core::plugin_tool::{Outcome, StepOutput, ToolEntry, ToolRun};
use serde_json::{Value, json};

/// The code of a refusal by the host policy (a module that is not trusted).
const POLICY_CODE: i64 = 4;

/// The start of the message when the module itself says that it cannot do something.
const GUEST_PREFIX: &str = "the plug-in failed:";

/// A dialog that asks the user about one module.
struct Review {
    id: String,
    /// Start the tool of the module after the user trusts it.
    then_tool: bool,
    /// What the dialog shows, read once when the dialog opened. The Trust button trusts the hash in
    /// here, and nothing else: if the module under this id changes while the dialog is open (an
    /// agent installs another build), the dialog closes and asks again.
    info: Value,
}

/// State of the Plug-ins window, the trust dialog, the Object Info form and the running tool.
#[derive(Default)]
pub struct PluginUi {
    open: bool,
    review: Option<Review>,
    form: Option<(ObjectId, Form)>,
    run: Option<ToolRun>,
    /// The last pointer move that waits for the next frame. A frame sends at most one move to the
    /// module, however many the pointer made, so a slow tool cannot queue up calls.
    pending_move: Option<(glam::DVec2, egui::Modifiers)>,
    /// How many calls the tool runs have made (for tests).
    #[cfg(test)]
    calls: u64,
    /// Problems to show once: stored modules that were not loaded.
    notes: Vec<String>,
    /// Web: the bytes of modules that the browser has picked and compiled.
    #[cfg(target_arch = "wasm32")]
    inbox: std::rc::Rc<std::cell::RefCell<Vec<Vec<u8>>>>,
    /// Web: set when the stored modules are compiled and can be loaded.
    #[cfg(target_arch = "wasm32")]
    boot: std::rc::Rc<std::cell::Cell<bool>>,
}

fn ftext(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or("").to_string()
}

/// The shapes of a preview, from the JSON of `plugins.tool_step`. A shape that does not read is
/// left out.
fn preview_shapes(v: &Value) -> Vec<Shape> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| serde_json::from_value::<Shape>(s.clone()).ok())
        .collect()
}

/// Reads a module file for the install button. The size is checked while reading.
#[cfg(not(target_arch = "wasm32"))]
fn read_file(path: &std::path::Path, max: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > max {
        return Err(tr!("plugin-ui-too-big", max = max));
    }
    Ok(bytes)
}

/// Largest module that the install button reads, in bytes.
const MAX_MODULE: usize = 2 << 20;

/// The name of the file type in the file dialog. It is the name of a format, so it is not translated.
#[cfg(not(target_arch = "wasm32"))]
const WASM_FILTER: &str = "WebAssembly";
#[cfg(target_arch = "wasm32")]
const WASM_FILTER: &str = "WebAssembly";

impl PluginUi {
    /// File > Plug-ins.
    pub fn open(&mut self) {
        self.open = true;
    }

    /// True while a plug-in tool runs. The canvas then sends its events here.
    pub fn running(&self) -> bool {
        self.run.as_ref().is_some_and(|r| !r.is_over())
    }

    /// The preview of the running tool, in drawing coordinates.
    pub fn preview(&self) -> &[Shape] {
        self.run.as_ref().map_or(&[], ToolRun::preview)
    }

    // ------------------------------------------------------------------ storage

    /// Turns on the storage of installed modules and of trust, in the folder of the user. The app
    /// calls this once at the start (never a test: tests inject their own folder).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach_user_storage(&mut self, session: &mut Session) {
        if let Some(home) = kadakas_plugin::store::default_home() {
            self.attach_folder(session, &home);
        }
    }

    /// Like [`PluginUi::attach_user_storage`], for a folder that the caller names.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach_folder(&mut self, session: &mut Session, home: &std::path::Path) {
        let report = session.wasm_plugins.registry.open_home(home);
        self.take_report(report);
    }

    fn take_report(&mut self, report: kadakas_plugin::StoreReport) {
        for r in report.rejected {
            // A module built for another ABI version gets its own note, once.
            self.notes.push(if r.reason.contains("uses ABI version") {
                tr!("plugin-ui-old-abi", message = r.reason)
            } else {
                tr!("plugin-ui-stored-problem", name = r.name, reason = r.reason)
            });
        }
    }

    /// Web: starts the browser storage. The stored modules are compiled first (the browser compiles
    /// large modules only asynchronously), then the next frame loads them.
    #[cfg(target_arch = "wasm32")]
    pub fn begin_web_storage(&mut self, ctx: &egui::Context) {
        let (boot, ctx) = (self.boot.clone(), ctx.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let backend = kadakas_plugin::web::backend();
            let limits = kadakas_plugin::runtime::default_limits();
            let store = kadakas_plugin::ModuleStore::new(
                backend,
                limits.max_module_bytes,
                kadakas_plugin::store::WEB_TOTAL_CAP,
            );
            kadakas_plugin::web::precompile_stored(&store, &limits).await;
            boot.set(true);
            ctx.request_repaint();
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn finish_web_boot(&mut self, session: &mut Session) {
        if self.boot.replace(false) {
            let backend = kadakas_plugin::web::backend();
            let max = session.wasm_plugins.registry.limits().max_module_bytes;
            let report = session.wasm_plugins.registry.attach(
                backend,
                max,
                kadakas_plugin::store::WEB_TOTAL_CAP,
            );
            self.take_report(report);
        }
    }

    // ------------------------------------------------------------------ install

    /// Installs a module from its bytes, as the user picked it. A module that is not trusted opens
    /// the trust dialog at once.
    pub fn install_bytes(&mut self, session: &mut Session, bytes: &[u8], status: &mut String) {
        match kadakas_api::install_wasm_plugin(session, bytes) {
            Ok(v) => {
                let name = ftext(&v, "name");
                *status = match v["stale_objects"].as_u64().unwrap_or(0) {
                    0 => tr!("plugin-ui-installed", name = name),
                    n => tr!("plugin-ui-replaced", name = name, count = n),
                };
                if let Some(w) = v["warning"].as_str() {
                    *status = tr!("plugin-ui-not-kept", message = w);
                }
                if v["trusted"] != json!(true) {
                    self.open_review(session, &ftext(&v, "id"), false);
                }
            }
            Err(e) if e.message.contains("uses ABI version") => {
                *status = tr!("plugin-ui-old-abi", message = e.message)
            }
            Err(e) => *status = tr!("plugin-ui-install-failed", message = e.message),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn pick_and_install(&mut self, session: &mut Session, status: &mut String) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter(WASM_FILTER, &["wasm"])
            .pick_file()
        else {
            return;
        };
        match read_file(&path, MAX_MODULE) {
            Ok(bytes) => self.install_bytes(session, &bytes, status),
            Err(e) => *status = tr!("plugin-ui-read-failed", message = e),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn pick_and_install(&mut self, ctx: &egui::Context, status: &mut String) {
        let (inbox, ctx) = (self.inbox.clone(), ctx.clone());
        let _ = status;
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(f) = rfd::AsyncFileDialog::new()
                .add_filter(WASM_FILTER, &["wasm"])
                .pick_file()
                .await
            {
                let bytes = f.read().await;
                // Compile now, asynchronously. The install after it needs no synchronous compile.
                let limits = kadakas_plugin::runtime::default_limits();
                let _ = kadakas_plugin::web::precompile(&bytes, &limits).await;
                inbox.borrow_mut().push(bytes);
                ctx.request_repaint();
            }
        });
    }

    // ------------------------------------------------------------------- windows

    /// Draws the Plug-ins window and the trust dialog, and takes the files that the browser picked.
    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session, status: &mut String) {
        #[cfg(target_arch = "wasm32")]
        {
            self.finish_web_boot(session);
            let picked: Vec<Vec<u8>> = self.inbox.borrow_mut().drain(..).collect();
            for bytes in picked {
                self.install_bytes(session, &bytes, status);
            }
        }
        self.flush_move(session, status);
        if let Some(note) = self.notes.pop() {
            *status = note;
        }
        self.review_dialog(ctx, session, status);
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new(tr!("plugin-ui-plug-ins"))
            .id(egui::Id::new("win-plug-ins"))
            .open(&mut open)
            .default_width(480.0)
            .show(ctx, |ui| self.window_body(ctx, ui, session, status));
        self.open = open;
    }

    fn window_body(
        &mut self,
        ctx: &egui::Context,
        ui: &mut Ui,
        session: &mut Session,
        status: &mut String,
    ) {
        let _ = ctx;
        ui.small(tr!("plugin-ui-intro"));
        ui.small(if session.wasm_plugins.registry.persistent() {
            tr!("plugin-ui-kept")
        } else {
            tr!("plugin-ui-session-only")
        });
        if ui.button(tr!("plugin-ui-install")).clicked() {
            #[cfg(not(target_arch = "wasm32"))]
            self.pick_and_install(session, status);
            #[cfg(target_arch = "wasm32")]
            self.pick_and_install(ctx, status);
        }
        ui.separator();
        let list = session
            .handle("plugins.list", json!({}))
            .unwrap_or_else(|_| json!([]));
        let entries = list.as_array().cloned().unwrap_or_default();
        if entries.is_empty() {
            ui.label(tr!("plugin-ui-none"));
        }
        for e in &entries {
            let id = ftext(e, "id");
            ui.horizontal_wrapped(|ui| {
                ui.strong(ftext(e, "name"));
                ui.label(ftext(e, "version"));
                if e["trusted"] == json!(true) {
                    ui.colored_label(Color32::from_rgb(40, 140, 60), tr!("plugin-ui-trusted"));
                } else {
                    ui.colored_label(Color32::from_rgb(200, 120, 0), tr!("plugin-ui-not-trusted"));
                }
                let n = e["instances"].as_u64().unwrap_or(0);
                if n > 0 {
                    ui.small(tr!("plugin-ui-in-use", count = n));
                }
            });
            ui.horizontal(|ui| {
                if ui.button(tr!("plugin-ui-review")).clicked() {
                    self.open_review(session, &id.clone(), false);
                }
                if e["trusted"] == json!(true)
                    && ui.button(tr!("plugin-ui-stop-trusting")).clicked()
                    && let Err(m) = kadakas_api::forget_wasm_plugin(session, &id)
                {
                    *status = m;
                }
                if ui
                    .add_enabled(
                        e["instances"].as_u64().unwrap_or(0) == 0,
                        egui::Button::new(tr!("plugin-ui-remove")),
                    )
                    .on_disabled_hover_text(tr!("plugin-ui-remove-blocked"))
                    .clicked()
                {
                    match session.handle("plugins.remove", json!({"plugin": id})) {
                        Ok(_) => *status = tr!("plugin-ui-removed", name = ftext(e, "name")),
                        Err(err) => *status = err.message,
                    }
                    if let Some(w) = kadakas_api::wasm_plugin_warning(session) {
                        *status = w;
                    }
                }
            });
            ui.separator();
        }
    }

    /// Opens the trust dialog for a module. Reads what the dialog shows now; see [`Review`].
    fn open_review(&mut self, session: &Session, id: &str, then_tool: bool) {
        if let Some(info) = kadakas_api::review_wasm_plugin(session, id) {
            self.review = Some(Review {
                id: id.to_string(),
                then_tool,
                info,
            });
        }
    }

    /// The trust dialog. Trust is for the hash that the dialog showed when it opened.
    fn review_dialog(&mut self, ctx: &egui::Context, session: &mut Session, status: &mut String) {
        let Some(review) = &self.review else { return };
        // The module under this id must still be the one that the dialog shows.
        let info = review.info.clone();
        let now = kadakas_api::review_wasm_plugin(session, &review.id);
        if now.as_ref().map(|n| n["hash"].clone()) != Some(info["hash"].clone()) {
            self.review = None;
            *status = tr!("plugin-ui-changed");
            return;
        }
        let problem = ftext(&info, "problem");
        let mut decision = None;
        egui::Window::new(tr!("plugin-ui-trust-title"))
            .id(egui::Id::new("win-trust-wasm-plug-in"))
            .collapsible(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.heading(ftext(&info, "name"));
                egui::Grid::new("trust-facts")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label(tr!("plugin-ui-version"));
                        ui.label(ftext(&info, "version"));
                        ui.end_row();
                        ui.label(tr!("plugin-ui-author"));
                        match info["author"].as_str().filter(|a| !a.is_empty()) {
                            Some(a) => ui.label(a),
                            None => ui.label(tr!("plugin-ui-no-author")),
                        };
                        ui.end_row();
                        ui.label(tr!("plugin-ui-hash"));
                        ui.label(egui::RichText::new(ftext(&info, "hash")).monospace());
                        ui.end_row();
                        ui.label(tr!("plugin-ui-size"));
                        ui.label(tr!(
                            "plugin-ui-bytes",
                            size = info["size"].as_u64().unwrap_or(0)
                        ));
                        ui.end_row();
                        ui.label(tr!("plugin-ui-can-do"));
                        let kinds: Vec<String> = info["kinds"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|k| match k.as_str() {
                                Some("tool") => tr!("plugin-ui-kind-tool"),
                                _ => tr!("plugin-ui-kind-object"),
                            })
                            .collect();
                        ui.label(kinds.join(", "));
                        ui.end_row();
                    });
                let description = ftext(&info, "description");
                if !description.is_empty() {
                    ui.add_space(4.0);
                    ui.label(description);
                }
                ui.add_space(6.0);
                ui.label(tr!("plugin-ui-no-permissions"));
                ui.colored_label(Color32::from_rgb(200, 120, 0), tr!("plugin-ui-warning"));
                ui.small(tr!("plugin-ui-agents"));
                if !problem.is_empty() {
                    ui.colored_label(
                        Color32::from_rgb(200, 40, 40),
                        tr!("plugin-ui-problem", message = problem.as_str()),
                    );
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            problem.is_empty(),
                            egui::Button::new(tr!("plugin-ui-trust")),
                        )
                        .clicked()
                    {
                        decision = Some(true);
                    }
                    if ui.button(tr!("plugin-ui-cancel")).clicked() {
                        decision = Some(false);
                    }
                });
            });
        let then_tool = review.then_tool;
        let id = review.id.clone();
        match decision {
            Some(true) => {
                let shown = ftext(&info, "hash");
                // The hash is the one from when the dialog opened, not a fresh read.
                match kadakas_api::trust_wasm_plugin(session, &id, &shown) {
                    Ok(()) => {
                        *status = tr!("plugin-ui-trusted-status", name = ftext(&info, "name"))
                    }
                    // The trust holds for this session; the message says why it will not last.
                    Err(e) => *status = e,
                }
                self.review = None;
                if then_tool && kadakas_api::review_wasm_plugin(session, &id).is_some() {
                    self.start_tool(session, &id, status);
                }
            }
            Some(false) => self.review = None,
            None => {}
        }
    }

    // ------------------------------------------------------------- Object Info

    /// The parameters of the selected plug-in object. Draws nothing for other objects.
    pub fn object_info(&mut self, ui: &mut Ui, session: &mut Session, status: &mut String) {
        let [id] = session.selection[..] else {
            self.form = None;
            return;
        };
        let is_plugin_object = session
            .editor
            .doc()
            .object(id)
            .is_some_and(|o| o.data.contains_key(kadakas_api::WASM_PLUGIN_DATA_KEY));
        if !is_plugin_object {
            self.form = None;
            return;
        }
        let Ok(info) = session.handle("plugins.object", json!({"id": id})) else {
            self.form = None;
            return;
        };
        ui.separator();
        ui.strong(tr!(
            "plugin-ui-object-heading",
            name = ftext(&info, "name"),
            version = ftext(&info, "version")
        ));
        if info["installed"] != json!(true) {
            ui.small(tr!("plugin-ui-object-not-installed"));
            self.form = None;
            return;
        }
        if info["stale"] == json!(true) {
            ui.colored_label(
                Color32::from_rgb(200, 120, 0),
                tr!("plugin-ui-object-stale"),
            );
        }
        let trusted = info["trusted"] == json!(true);
        if !trusted {
            ui.colored_label(Color32::from_rgb(200, 120, 0), tr!("plugin-ui-needs-trust"));
            if ui.button(tr!("plugin-ui-review")).clicked() {
                self.open_review(session, &ftext(&info, "plugin"), false);
            }
        }
        if let Some(err) = info["error"].as_str() {
            ui.colored_label(
                Color32::from_rgb(200, 40, 40),
                tr!("plugin-ui-object-error", message = err),
            );
            ui.small(tr!("plugin-ui-object-error-note"));
        }
        if self
            .form
            .as_ref()
            .is_none_or(|(i, f)| *i != id || !f.matches(&info["schema"], &info["params"]))
        {
            self.form = Some((id, Form::new(&info["schema"], &info["params"])));
        }
        let mut changes = Vec::new();
        let mut run_again = false;
        if let Some((_, form)) = &mut self.form {
            ui.add_enabled_ui(trusted, |ui| {
                let fields = form.fields().to_vec();
                for f in &fields {
                    draw_field(ui, form, f, &f.name, &mut changes);
                }
            });
            run_again = ui
                .add_enabled(trusted, egui::Button::new(tr!("plugin-ui-run-again")))
                .on_hover_text(tr!("plugin-ui-run-again-hint"))
                .clicked();
        }
        for c in changes {
            apply_change(session, id, &c, status);
        }
        if run_again {
            match session.handle("plugins.reset", json!({"id": id})) {
                Ok(v) if v["results"][0]["ok"] == json!(false) => {
                    let message = ftext(&v["results"][0], "error");
                    *status = tr!("plugin-ui-object-error", message = message);
                }
                Ok(_) => {}
                Err(e) => *status = e.message,
            }
        }
    }

    // ----------------------------------------------------------------- tool set

    /// The "Plug-ins" tool set: one button for each installed plug-in that has a tool.
    pub fn tool_set(&mut self, ui: &mut Ui, session: &mut Session, status: &mut String) {
        let entries = tool_entries(session);
        ui.add_space(8.0);
        ui.label(tr!("plugin-ui-tool-set"));
        if entries.is_empty() {
            ui.small(tr!("plugin-ui-tool-none"));
        }
        let active = self
            .run
            .as_ref()
            .filter(|r| !r.is_over())
            .map(|r| r.plugin().to_string());
        for e in &entries {
            let on = active.as_deref() == Some(e.plugin.as_str());
            let mut b = ui.selectable_label(on, &e.name);
            if !e.trusted {
                b = b.on_hover_text(tr!("plugin-ui-tool-untrusted"));
            }
            if b.clicked() {
                if on {
                    self.cancel();
                } else {
                    self.start_tool(session, &e.plugin, status);
                }
            }
        }
        if let Some(run) = self.run.as_ref().filter(|r| !r.is_over()) {
            ui.small(run.hint());
        }
    }

    /// Starts the tool of a plug-in. A module that is not trusted opens the trust dialog first.
    pub fn start_tool(&mut self, session: &mut Session, id: &str, status: &mut String) {
        self.cancel();
        let trusted = kadakas_api::review_wasm_plugin(session, id)
            .is_some_and(|r| r["trusted"] == json!(true));
        if !trusted {
            self.open_review(session, &id.to_string(), true);
            return;
        }
        let name = kadakas_api::review_wasm_plugin(session, id)
            .map(|r| ftext(&r, "name"))
            .unwrap_or_default();
        self.run = Some(ToolRun::new(id));
        self.send(
            session,
            json!({"type": "start"}),
            egui::Modifiers::NONE,
            status,
        );
        if self.running() {
            *status = format!(
                "{} {}",
                tr!("plugin-ui-tool-started", name = name),
                self.hint()
            );
        }
    }

    fn hint(&self) -> String {
        self.run
            .as_ref()
            .map_or(String::new(), |r| r.hint().to_string())
    }

    /// Escape, or another tool was chosen: the run ends and the preview goes. The module is not asked.
    pub fn cancel(&mut self) {
        self.pending_move = None;
        if let Some(r) = &mut self.run {
            r.cancel();
        }
        self.run = None;
    }

    /// The pointer moved on the canvas (`pixel` is the size of a pixel in mm).
    pub fn pointer_moved(&mut self, p: glam::DVec2, pixel: f64, mods: egui::Modifiers) {
        let Some(p) = self.run.as_mut().and_then(|r| r.move_to(p, pixel)) else {
            return;
        };
        // Only the newest move is kept. `flush_move` sends it once per frame.
        self.pending_move = Some((p, mods));
    }

    /// Sends the pending pointer move, if there is one. The app calls this once per frame.
    pub fn flush_move(&mut self, session: &mut Session, status: &mut String) {
        if let Some((p, mods)) = self.pending_move.take() {
            self.send(
                session,
                json!({"type": "move", "at": [p.x, p.y]}),
                mods,
                status,
            );
        }
    }

    /// A click on the canvas.
    pub fn clicked(
        &mut self,
        session: &mut Session,
        p: glam::DVec2,
        mods: egui::Modifiers,
        status: &mut String,
    ) {
        // The click carries its own point: a move that waits is out of date.
        self.pending_move = None;
        if let Some(r) = &mut self.run {
            r.clicked();
        }
        self.send(
            session,
            json!({"type": "click", "at": [p.x, p.y]}),
            mods,
            status,
        );
    }

    /// Enter or a double click.
    pub fn finish(&mut self, session: &mut Session, status: &mut String) {
        self.pending_move = None;
        self.send(
            session,
            json!({"type": "finish"}),
            egui::Modifiers::NONE,
            status,
        );
    }

    /// Sends one event to the module through `plugins.tool_step` and applies the answer. A result
    /// is committed by the API as one undo step. A run that fails ends; a refusal by the module
    /// (its own message) only shows the message.
    fn send(
        &mut self,
        session: &mut Session,
        event: Value,
        mods: egui::Modifiers,
        status: &mut String,
    ) {
        #[cfg(test)]
        {
            self.calls += 1;
        }
        let Some(run) = self.run.as_mut().filter(|r| !r.is_over()) else {
            return;
        };
        let id = run.plugin().to_string();
        let request = json!({
            "plugin": id,
            "event": event,
            "state": run.state(),
            "modifiers": {"shift": mods.shift, "ctrl": mods.command, "alt": mods.alt},
            "commit": true,
        });
        let answer = session.handle("plugins.tool_step", request);
        let outcome = match answer {
            Ok(v) => {
                let result = v["result"].clone();
                run.apply(Ok(StepOutput {
                    state: v["state"].clone(),
                    preview: preview_shapes(&v["preview"]),
                    hint: ftext(&v, "hint"),
                    done: (v["done"] == json!(true)).then_some(result),
                }))
            }
            Err(e) if e.code == POLICY_CODE => {
                run.cancel();
                self.run = None;
                self.open_review(session, &id, true);
                return;
            }
            Err(e) if e.message.starts_with(GUEST_PREFIX) => {
                run.refused(&e.message);
                *status = e.message;
                return;
            }
            Err(e) => run.apply::<Value>(Err(e.message)),
        };
        match outcome {
            Outcome::Continue => *status = self.hint(),
            Outcome::Failed(message) => {
                self.run = None;
                *status = tr!("plugin-ui-tool-failed", message = message);
            }
            Outcome::Done(result) => {
                *status = match result["kind"].as_str() {
                    Some("object") => tr!("plugin-ui-tool-done-object"),
                    _ => tr!(
                        "plugin-ui-tool-done-shapes",
                        count = result["shapes"].as_u64().unwrap_or(0)
                    ),
                };
                // The tool stays active for the next object, as drawing tools do.
                self.run = Some(ToolRun::new(&id));
                self.send(
                    session,
                    json!({"type": "start"}),
                    egui::Modifiers::NONE,
                    &mut String::new(),
                );
            }
        }
    }
}

/// The plug-ins that have a tool.
pub fn tool_entries(session: &mut Session) -> Vec<ToolEntry> {
    session
        .handle("plugins.list", json!({}))
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter(|e| {
            e["kinds"]
                .as_array()
                .is_some_and(|k| k.iter().any(|k| k == "tool"))
        })
        .map(|e| ToolEntry {
            plugin: ftext(e, "id"),
            name: ftext(e, "name"),
            trusted: e["trusted"] == json!(true),
        })
        .collect()
}

/// Sends one change of the form to the plug-in object: one undo step.
fn apply_change(session: &mut Session, id: ObjectId, change: &Change, status: &mut String) {
    match session.handle(
        "plugins.set_params",
        json!({"id": id, "params": change.params()}),
    ) {
        Ok(v) if v["ok"] == json!(false) => {
            let message = ftext(&v, "error");
            *status = tr!("plugin-ui-object-error", message = message);
        }
        Ok(_) => {}
        Err(e) => *status = e.message,
    }
}

fn parse_color(s: &str) -> Color32 {
    let h = s.trim_start_matches('#');
    if h.len() != 6 && h.len() != 8 {
        return Color32::BLACK;
    }
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    let a = if h.len() >= 8 { byte(6) } else { 255 };
    Color32::from_rgba_unmultiplied(byte(0), byte(2), byte(4), a)
}

fn color_text(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

/// Draws one field of the form. Collects the changes.
fn draw_field(ui: &mut Ui, form: &mut Form, f: &Field, path: &str, changes: &mut Vec<Change>) {
    let before = changes.len();
    match f.kind {
        Kind::Number | Kind::Length | Kind::Text | Kind::Color => {
            ui.horizontal(|ui| {
                ui.label(&f.label);
                let mut text = form.text_at(path);
                let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(90.0));
                if r.changed() {
                    form.edit(path, &text);
                }
                let unit = f.unit_text();
                if !unit.is_empty() {
                    ui.label(unit);
                }
                if f.kind == Kind::Color {
                    let mut c = parse_color(&form.text_at(path));
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        changes.extend(form.set_color(path, &color_text(c)));
                    }
                }
                if r.lost_focus() {
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        form.revert(path);
                    } else {
                        changes.extend(form.commit(path));
                    }
                }
                if let Some(range) = f.range_text() {
                    r.on_hover_text(range);
                }
            });
        }
        Kind::Bool => {
            let mut b = form.value_at(path).as_bool().unwrap_or(false);
            if ui.checkbox(&mut b, &f.label).changed() {
                changes.extend(form.set_bool(path, b));
            }
        }
        Kind::Choice => {
            let current = form.value_at(path).as_str().unwrap_or("").to_string();
            ui.horizontal(|ui| {
                ui.label(&f.label);
                egui::ComboBox::from_id_salt(path)
                    .selected_text(&current)
                    .show_ui(ui, |ui| {
                        for o in &f.options {
                            if ui.selectable_label(*o == current, o).clicked() && *o != current {
                                changes.extend(form.set_choice(path, o));
                            }
                        }
                    });
            });
        }
        Kind::Record => {
            ui.collapsing(&f.label, |ui| {
                for sub in &f.fields {
                    draw_field(ui, form, sub, &format!("{path}.{}", sub.name), changes);
                }
            });
        }
    }
    if let Some(p) = form.problem(path) {
        ui.colored_label(Color32::from_rgb(200, 40, 40), p.text());
    }
    // A field of the top level can go back to its default.
    if !path.contains('.')
        && changes.len() == before
        && !matches!(f.kind, Kind::Record)
        && ui.small_button(tr!("plugin-ui-default")).clicked()
    {
        changes.extend(form.reset(path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STONES: &[u8] = include_bytes!("../../kadakas-plugin/tests/fixtures/stone_row.wasm");
    const PLANTER: &[u8] =
        include_bytes!("../../kadakas-plugin/tests/fixtures/raised_planter.wasm");
    const STONES_ID: &str = "org.kadakas.stone-row";

    fn frame(ctx: &egui::Context, f: impl FnOnce(&mut egui::Ui)) {
        let mut f = Some(f);
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(f) = f.take() {
                f(ui);
            }
        });
    }

    fn session_with_stones() -> (Session, PluginUi, String) {
        let mut s = Session::with_sequential_ids();
        let mut ui = PluginUi::default();
        let mut status = String::new();
        ui.install_bytes(&mut s, STONES, &mut status);
        (s, ui, status)
    }

    fn trust(s: &mut Session) {
        let hash = kadakas_api::review_wasm_plugin(s, STONES_ID).unwrap()["hash"]
            .as_str()
            .unwrap()
            .to_string();
        kadakas_api::trust_wasm_plugin(s, STONES_ID, &hash).unwrap();
    }

    #[test]
    fn installing_an_untrusted_module_opens_the_trust_dialog() {
        let (s, ui, status) = session_with_stones();
        assert!(status.contains("Stone row"), "{status}");
        assert_eq!(ui.review.as_ref().unwrap().id, STONES_ID);
        assert!(
            !s.wasm_plugins.registry.trust.is_trusted(
                kadakas_api::review_wasm_plugin(&s, STONES_ID).unwrap()["hash"]
                    .as_str()
                    .unwrap()
            )
        );
    }

    #[test]
    fn a_broken_file_is_reported_and_opens_no_dialog() {
        let mut s = Session::with_sequential_ids();
        let mut ui = PluginUi::default();
        let mut status = String::new();
        ui.install_bytes(&mut s, b"junk", &mut status);
        assert!(status.contains("Cannot install"), "{status}");
        assert!(ui.review.is_none());
    }

    #[test]
    fn the_dialog_trusts_only_when_the_user_presses_trust_and_cancel_keeps_it_untrusted() {
        let (mut s, mut ui, mut status) = session_with_stones();
        let ctx = egui::Context::default();
        // Drawing the dialog alone trusts nothing.
        frame(&ctx, |u| ui.show(u.ctx(), &mut s, &mut status));
        assert!(ui.review.is_some());
        assert!(
            !kadakas_api::review_wasm_plugin(&s, STONES_ID).unwrap()["trusted"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn the_review_data_has_what_the_dialog_shows_and_no_permission() {
        let (s, _ui, _) = session_with_stones();
        let r = kadakas_api::review_wasm_plugin(&s, STONES_ID).unwrap();
        for key in ["name", "version", "author", "hash", "size", "kinds"] {
            assert!(!r[key].is_null(), "{key}");
        }
        assert!(r.get("permissions").is_none());
    }

    #[test]
    fn the_tool_set_lists_plugins_that_have_a_tool() {
        let (mut s, mut ui, mut status) = session_with_stones();
        ui.install_bytes(&mut s, PLANTER, &mut status);
        let entries = tool_entries(&mut s);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].plugin, STONES_ID);
        assert!(!entries[0].trusted);
    }

    #[test]
    fn starting_an_untrusted_tool_asks_first_and_never_runs_the_module() {
        let (mut s, mut ui, mut status) = session_with_stones();
        ui.review = None;
        ui.start_tool(&mut s, STONES_ID, &mut status);
        assert!(!ui.running());
        let r = ui.review.as_ref().unwrap();
        assert!(r.then_tool && r.id == STONES_ID);
    }

    #[test]
    fn a_tool_run_draws_a_row_in_two_clicks_and_stays_active() {
        let (mut s, mut ui, mut status) = session_with_stones();
        trust(&mut s);
        ui.review = None;
        ui.start_tool(&mut s, STONES_ID, &mut status);
        assert!(ui.running());
        assert!(
            status.contains("Stone row") && status.contains("start"),
            "{status}"
        );
        let none = egui::Modifiers::NONE;
        ui.clicked(&mut s, glam::DVec2::new(0.0, 0.0), none, &mut status);
        ui.pointer_moved(glam::DVec2::new(3000.0, 0.0), 1.0, none);
        ui.flush_move(&mut s, &mut status);
        assert_eq!(ui.preview().len(), 4);
        assert_eq!(s.editor.doc().objects.len(), 0);
        ui.clicked(&mut s, glam::DVec2::new(3000.0, 0.0), none, &mut status);
        assert_eq!(s.editor.doc().objects.len(), 5, "{status}");
        assert!(status.contains("Placed"), "{status}");
        // The tool runs again for the next row.
        assert!(ui.running() && ui.preview().is_empty());
        // Undo takes the row away in one step.
        s.editor.undo();
        assert_eq!(s.editor.doc().objects.len(), 0);
    }

    #[test]
    fn shift_ends_with_plain_shapes_and_a_bad_click_only_shows_the_message() {
        let (mut s, mut ui, mut status) = session_with_stones();
        trust(&mut s);
        ui.review = None;
        ui.start_tool(&mut s, STONES_ID, &mut status);
        let none = egui::Modifiers::NONE;
        ui.clicked(&mut s, glam::DVec2::ZERO, none, &mut status);
        // Too close for one stone: the module refuses, the run goes on.
        ui.clicked(&mut s, glam::DVec2::new(10.0, 0.0), none, &mut status);
        assert!(ui.running(), "{status}");
        assert!(status.contains("shorter"), "{status}");
        let shift = egui::Modifiers {
            shift: true,
            ..egui::Modifiers::NONE
        };
        ui.clicked(&mut s, glam::DVec2::new(3000.0, 0.0), shift, &mut status);
        assert_eq!(s.editor.doc().objects.len(), 4);
        assert!(status.contains("4"), "{status}");
        // Finish with nothing started is the module's message.
        ui.finish(&mut s, &mut status);
        assert!(ui.running());
    }

    #[test]
    fn escape_and_a_failing_module_end_the_run() {
        let (mut s, mut ui, mut status) = session_with_stones();
        trust(&mut s);
        ui.review = None;
        ui.start_tool(&mut s, STONES_ID, &mut status);
        ui.cancel();
        assert!(!ui.running() && ui.preview().is_empty());
        // Events after the end do nothing.
        ui.clicked(
            &mut s,
            glam::DVec2::ZERO,
            egui::Modifiers::NONE,
            &mut status,
        );
        assert_eq!(s.editor.doc().objects.len(), 0);
        // Taking the trust away while a run is on: the next event asks again.
        ui.start_tool(&mut s, STONES_ID, &mut status);
        kadakas_api::forget_wasm_plugin(&mut s, STONES_ID).unwrap();
        ui.clicked(
            &mut s,
            glam::DVec2::ZERO,
            egui::Modifiers::NONE,
            &mut status,
        );
        assert!(!ui.running());
        assert!(ui.review.as_ref().unwrap().then_tool);
    }

    #[test]
    fn trusting_from_the_tool_dialog_starts_the_tool() {
        let (mut s, mut ui, mut status) = session_with_stones();
        ui.review = None;
        ui.start_tool(&mut s, STONES_ID, &mut status);
        assert!(!ui.running());
        // What the Trust button does.
        trust(&mut s);
        ui.review = None;
        ui.start_tool(&mut s, STONES_ID, &mut status);
        assert!(ui.running());
    }

    #[test]
    fn the_object_info_form_changes_a_parameter_in_one_undo_step() {
        let (mut s, mut ui, mut status) = session_with_stones();
        trust(&mut s);
        let placed = s
            .handle(
                "plugins.place",
                json!({"plugin": STONES_ID, "a": [0, 0], "b": [2000, 0]}),
            )
            .unwrap();
        let id: ObjectId = serde_json::from_value(placed["id"].clone()).unwrap();
        s.selection = vec![id];
        let ctx = egui::Context::default();
        // A frame draws the form for the selected object without a change.
        frame(&ctx, |u| ui.object_info(u, &mut s, &mut status));
        assert!(ui.form.is_some());
        // The form takes the typed text and gives one change.
        let (_, form) = ui.form.as_mut().unwrap();
        form.edit("gap", "100");
        let change = form.commit("gap").unwrap();
        let before = s.editor.revision();
        apply_change(&mut s, id, &change, &mut status);
        assert_eq!(s.editor.changes_since(before).changes.len(), 1);
        let info = s.handle("plugins.object", json!({"id": id})).unwrap();
        assert_eq!(info["params"]["gap"], 100.0);
        s.editor.undo();
        let info = s.handle("plugins.object", json!({"id": id})).unwrap();
        assert_eq!(info["params"]["gap"], 300.0);
    }

    #[test]
    fn colors_round_trip() {
        assert_eq!(color_text(parse_color("#6b4f3a")), "#6b4f3a");
        assert_eq!(parse_color("bad"), Color32::BLACK);
        assert_eq!(color_text(parse_color("#112233ff")), "#112233");
    }

    #[test]
    fn stored_modules_come_back_from_a_folder_that_the_test_names() {
        let dir = std::env::temp_dir().join(format!("kadakas-plugin-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        {
            let mut s = Session::with_sequential_ids();
            let mut ui = PluginUi::default();
            ui.attach_folder(&mut s, &dir);
            let mut status = String::new();
            ui.install_bytes(&mut s, STONES, &mut status);
            assert!(status.contains("Installed"));
            trust(&mut s);
        }
        let mut s = Session::with_sequential_ids();
        let mut ui = PluginUi::default();
        ui.attach_folder(&mut s, &dir);
        assert!(ui.notes.is_empty());
        let r = kadakas_api::review_wasm_plugin(&s, STONES_ID).unwrap();
        assert_eq!(r["trusted"], true);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_windows_draw_in_both_languages() {
        let (mut s, mut ui, mut status) = session_with_stones();
        let ctx = egui::Context::default();
        ui.open();
        for lang in ["en-US", "et"] {
            kadakas_l10n::with_language(lang, || {
                frame(&ctx, |u| ui.show(u.ctx(), &mut s, &mut status));
                frame(&ctx, |u| ui.tool_set(u, &mut s, &mut status));
            });
        }
    }

    #[test]
    fn the_message_of_a_module_has_the_prefix_that_the_run_looks_for() {
        let (mut s, _ui, _) = session_with_stones();
        trust(&mut s);
        let e = s
            .handle(
                "plugins.tool_step",
                json!({"plugin": STONES_ID, "event": {"type": "click", "at": [10, 0]}, "state": {"start": [0, 0]}}),
            )
            .unwrap_err();
        assert!(e.message.starts_with(GUEST_PREFIX), "{}", e.message);
    }

    #[test]
    fn many_pointer_moves_in_a_frame_make_one_call() {
        let (mut s, mut ui, mut status) = session_with_stones();
        trust(&mut s);
        ui.review = None;
        ui.start_tool(&mut s, STONES_ID, &mut status);
        let none = egui::Modifiers::NONE;
        ui.clicked(&mut s, glam::DVec2::ZERO, none, &mut status);
        let before = ui.calls;
        for i in 1..=50 {
            ui.pointer_moved(glam::DVec2::new(100.0 * f64::from(i), 0.0), 1.0, none);
        }
        // Nothing is sent while the frame draws.
        assert_eq!(ui.calls, before);
        ui.flush_move(&mut s, &mut status);
        assert_eq!(ui.calls, before + 1);
        // The newest point won: 5000 mm of row, 7 stones.
        assert_eq!(ui.preview().len(), 7);
        // A frame without a new move sends nothing.
        ui.flush_move(&mut s, &mut status);
        assert_eq!(ui.calls, before + 1);
        // A click drops the move that waits.
        ui.pointer_moved(glam::DVec2::new(9000.0, 0.0), 1.0, none);
        ui.clicked(&mut s, glam::DVec2::new(3000.0, 0.0), none, &mut status);
        ui.flush_move(&mut s, &mut status);
        assert_eq!(s.editor.doc().objects.len(), 5);
    }

    #[test]
    fn the_dialog_trusts_the_module_it_showed_and_closes_if_another_build_takes_its_place() {
        let (mut s, mut ui, mut status) = session_with_stones();
        let shown = ui.review.as_ref().unwrap().info["hash"]
            .as_str()
            .unwrap()
            .to_string();
        // Another build with the same id is installed while the dialog is open. A custom section
        // at the end changes the hash and nothing else.
        let mut swapped = STONES.to_vec();
        swapped.extend([0, 3, 1, b'x', 0]);
        kadakas_api::install_wasm_plugin(&mut s, &swapped).unwrap();
        let now = kadakas_api::review_wasm_plugin(&s, STONES_ID).unwrap();
        assert_ne!(now["hash"].as_str().unwrap(), shown);
        // The dialog closes with a note and trusts nothing.
        let ctx = egui::Context::default();
        frame(&ctx, |u| ui.show(u.ctx(), &mut s, &mut status));
        assert!(ui.review.is_none());
        assert!(status.contains("changed"), "{status}");
        assert!(
            !s.wasm_plugins
                .registry
                .trust
                .is_trusted(now["hash"].as_str().unwrap())
        );
        assert!(!s.wasm_plugins.registry.trust.is_trusted(&shown));
        // The function that trusts also refuses the old hash.
        assert!(kadakas_api::trust_wasm_plugin(&mut s, STONES_ID, &shown).is_err());
    }

    #[test]
    fn a_module_of_another_abi_version_gets_a_note() {
        let mut s = Session::with_sequential_ids();
        let mut ui = PluginUi::default();
        let mut status = String::new();
        // The version number is in the manifest that the module returns, so build a module that
        // returns a manifest of version 1.
        let manifest = r#"{"id":"old.one","name":"Old","version":"1","abi_version":1,"kinds":["object"],"object":{"shape_type":"point"}}"#;
        ui.install_bytes(&mut s, &old_module(manifest), &mut status);
        assert!(status.contains("another version"), "{status}");
        assert!(ui.review.is_none());
    }

    /// A tiny module (written by hand in binary) whose `describe` returns `manifest`.
    fn old_module(manifest: &str) -> Vec<u8> {
        fn leb(mut n: usize, out: &mut Vec<u8>) {
            loop {
                let b = (n & 0x7f) as u8;
                n >>= 7;
                if n == 0 {
                    out.push(b);
                    break;
                }
                out.push(b | 0x80);
            }
        }
        fn section(id: u8, body: &[u8], out: &mut Vec<u8>) {
            out.push(id);
            leb(body.len(), out);
            out.extend(body);
        }
        let packed = (1024u64 << 32) | manifest.len() as u64;
        let mut m = b"\0asm\x01\0\0\0".to_vec();
        // types: (i32)->i32, ()->i64, (i32,i32)->i64, (i32,i32)->()
        section(
            1,
            &[
                4, 0x60, 1, 0x7f, 1, 0x7f, 0x60, 0, 1, 0x7e, 0x60, 2, 0x7f, 0x7f, 1, 0x7e, 0x60, 2,
                0x7f, 0x7f, 0,
            ],
            &mut m,
        );
        // functions: alloc, dealloc, describe, reset, tool
        section(3, &[5, 0, 3, 1, 2, 2], &mut m);
        section(5, &[1, 0, 1], &mut m);
        let mut ex = vec![6u8];
        for (name, kind, idx) in [
            ("memory", 2u8, 0u8),
            ("alloc", 0, 0),
            ("dealloc", 0, 1),
            ("describe", 0, 2),
            ("reset", 0, 3),
            ("tool", 0, 4),
        ] {
            ex.push(name.len() as u8);
            ex.extend(name.as_bytes());
            ex.push(kind);
            ex.push(idx);
        }
        section(7, &ex, &mut m);
        let mut body_describe = vec![0u8, 0x42];
        // i64.const packed (signed LEB)
        let mut v = packed as i64;
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            let done = (v == 0 && b & 0x40 == 0) || (v == -1 && b & 0x40 != 0);
            body_describe.push(if done { b } else { b | 0x80 });
            if done {
                break;
            }
        }
        body_describe.push(0x0b);
        let mut code = vec![5u8];
        for body in [
            vec![0u8, 0x41, 0, 0x0b],
            vec![0u8, 0x0b],
            body_describe,
            vec![0u8, 0x42, 0, 0x0b],
            vec![0u8, 0x42, 0, 0x0b],
        ] {
            leb(body.len(), &mut code);
            code.extend(body);
        }
        section(10, &code, &mut m);
        let mut data = vec![1u8, 0, 0x41, 0x80, 0x08, 0x0b];
        leb(manifest.len(), &mut data);
        data.extend(manifest.as_bytes());
        section(11, &data, &mut m);
        m
    }
}
