//! The autosave state of the app, the recovery dialog and File > Preferences.

use cadektor_core::model::Document;

use crate::autosave::{AutosaveSettings, Scheduler};

/// What the user chose in the recovery dialog.
pub struct Recovered {
    pub doc: Document,
    pub status: String,
}

pub struct AutosaveUi {
    pub settings: AutosaveSettings,
    sched: Scheduler,
    prefs_open: bool,
    /// The last problem, shown in the Preferences dialog.
    error: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    recovery: Option<crate::autosave::Recovery>,
    #[cfg(not(target_arch = "wasm32"))]
    crashed: Vec<crate::autosave::Crashed>,
    /// The browser holds an autosaved copy from an earlier visit.
    #[cfg(target_arch = "wasm32")]
    web_copy: Option<String>,
}

impl AutosaveUi {
    /// No autosave and no files touched. `begin` turns it on.
    pub fn inert() -> Self {
        AutosaveUi {
            settings: AutosaveSettings::default(),
            sched: Scheduler::new(0.0, 0),
            prefs_open: false,
            error: None,
            #[cfg(not(target_arch = "wasm32"))]
            recovery: None,
            #[cfg(not(target_arch = "wasm32"))]
            crashed: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            web_copy: None,
        }
    }

    /// Loads the preferences, looks for the remains of a crashed session, and starts a new one.
    pub fn begin(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(dir) = crate::autosave::recovery_dir() {
            self.settings = crate::autosave::load_settings();
            self.crashed = crate::autosave::find_crashed(&dir);
            match crate::autosave::Recovery::start(dir) {
                Ok(r) => self.recovery = Some(r),
                Err(e) => self.error = Some(format!("Autosave is off: {e}")),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.web_copy = crate::autosave::web::get();
        }
    }

    pub fn open_preferences(&mut self) {
        self.prefs_open = true;
    }

    /// Call once per frame. Saves a copy when one is due. `doc_bytes` is called only then.
    pub fn tick(
        &mut self,
        ctx: &egui::Context,
        revision: u64,
        path: Option<&std::path::Path>,
        doc_bytes: impl FnOnce() -> Option<Vec<u8>>,
    ) {
        let now = ctx.input(|i| i.time);
        if self.settings.enabled {
            // Wake up now and then, so that a time-based save also happens without input.
            ctx.request_repaint_after(std::time::Duration::from_secs(30));
        }
        if !self.sched.due(&self.settings, now, revision) {
            return;
        }
        let Some(bytes) = doc_bytes() else { return };
        self.sched.mark(now, revision);
        self.submit(bytes, path);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn submit(&mut self, bytes: Vec<u8>, path: Option<&std::path::Path>) {
        if let Some(r) = &mut self.recovery {
            let over = path
                .filter(|_| self.settings.save_over_file)
                .map(std::path::Path::to_path_buf);
            r.submit(crate::autosave::Job {
                bytes,
                original: path.map(std::path::Path::to_path_buf),
                keep: self.settings.keep as usize,
                over,
            });
            self.error = r.last_error().map(|e| format!("Last autosave failed: {e}"));
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn submit(&mut self, bytes: Vec<u8>, _path: Option<&std::path::Path>) {
        let text = String::from_utf8_lossy(&bytes);
        self.error = crate::autosave::web::put(&text)
            .err()
            .map(|e| format!("Last autosave failed: {e}"));
    }

    /// The user saved the document: an old browser copy is not needed any more.
    #[cfg(target_arch = "wasm32")]
    pub fn saved(&mut self) {
        crate::autosave::web::clear();
    }

    /// Clean exit: removes the lock and the autosaved versions.
    pub fn finish(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(r) = self.recovery.take() {
            r.finish();
        }
    }

    /// Shows the dialogs. Returns a document when the user opens a recovered copy.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Recovered> {
        self.preferences(ctx);
        self.recovery_dialog(ctx)
    }

    fn preferences(&mut self, ctx: &egui::Context) {
        if !self.prefs_open {
            return;
        }
        let mut open = true;
        let before = self.settings.clone();
        egui::Window::new("Preferences")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Autosave");
                let s = &mut self.settings;
                ui.checkbox(&mut s.enabled, "Save a recovery copy while I work");
                ui.add_enabled_ui(s.enabled, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Every");
                        ui.add(
                            egui::DragValue::new(&mut s.interval_minutes)
                                .range(1..=AutosaveSettings::MAX_MINUTES),
                        );
                        ui.label("minutes with changes");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Or after");
                        ui.add(egui::DragValue::new(&mut s.every_ops).range(0..=10_000));
                        ui.label("operations (0 = off)");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Keep the last");
                        ui.add(
                            egui::DragValue::new(&mut s.keep).range(1..=AutosaveSettings::MAX_KEEP),
                        );
                        ui.label("versions");
                    });
                    #[cfg(not(target_arch = "wasm32"))]
                    ui.checkbox(
                        &mut s.save_over_file,
                        "Also save over my file (off: recovery copies only)",
                    );
                });
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(dir) = crate::autosave::recovery_dir() {
                    ui.label(format!("Recovery folder: {}", dir.display()));
                    if ui.button("Show recovery folder").clicked() {
                        let _ = std::fs::create_dir_all(&dir);
                        let _ = crate::autosave::reveal(&dir);
                    }
                }
                #[cfg(target_arch = "wasm32")]
                ui.label("The browser keeps one recovery copy in its local storage.");
                if let Some(e) = &self.error {
                    ui.colored_label(ui.visuals().warn_fg_color, e);
                }
            });
        self.prefs_open = open;
        self.settings = self.settings.clone().sanitized();
        if self.settings != before {
            #[cfg(not(target_arch = "wasm32"))]
            if let Err(e) = crate::autosave::save_settings(&self.settings) {
                self.error = Some(format!("Cannot save the preferences: {e}"));
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn recovery_dialog(&mut self, ctx: &egui::Context) -> Option<Recovered> {
        if self.crashed.is_empty() {
            return None;
        }
        enum Choice {
            Open(usize),
            Discard(usize),
        }
        let mut choice = None;
        egui::Window::new("Recover unsaved work")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(
                    "Cadektor did not close cleanly last time. It kept these recovery copies.",
                );
                let now = crate::autosave::now_unix();
                for (i, c) in self.crashed.iter().enumerate() {
                    ui.separator();
                    let from = c
                        .original
                        .as_ref()
                        .map_or("an untitled drawing".to_string(), |p| {
                            p.display().to_string()
                        });
                    let age = c.saved_unix.map_or(String::new(), |t| {
                        format!(
                            ", saved {}",
                            crate::autosave::age_text(now.saturating_sub(t))
                        )
                    });
                    ui.label(format!("{from}{age} ({} versions)", c.versions.len()));
                    ui.horizontal(|ui| {
                        if ui.button("Open recovered copy").clicked() {
                            choice = Some(Choice::Open(i));
                        }
                        if ui.button("Discard").clicked() {
                            choice = Some(Choice::Discard(i));
                        }
                        if ui.button("Show location").clicked() {
                            let _ = crate::autosave::reveal(c.folder());
                        }
                    });
                }
            });
        match choice? {
            Choice::Discard(i) => {
                self.crashed.remove(i).discard();
                None
            }
            Choice::Open(i) => match self.crashed[i].load() {
                Ok((doc, from)) => {
                    // The copy is in memory now. Keep the files until the next autosave.
                    let status = format!("Recovered {}. Save it to keep it.", from.display());
                    self.crashed.remove(i).discard();
                    Some(Recovered { doc, status })
                }
                Err(e) => {
                    self.error = Some(format!("Cannot open the recovered copy: {e}"));
                    self.prefs_open = true;
                    None
                }
            },
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn recovery_dialog(&mut self, ctx: &egui::Context) -> Option<Recovered> {
        self.web_copy.as_ref()?;
        let mut choice = None;
        egui::Window::new("Recover unsaved work")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("This browser kept an autosaved copy from an earlier visit.");
                ui.horizontal(|ui| {
                    if ui.button("Open recovered copy").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Discard").clicked() {
                        choice = Some(false);
                    }
                });
            });
        let open = choice?;
        let text = self.web_copy.take()?;
        if !open {
            crate::autosave::web::clear();
            return None;
        }
        match crate::file_io::parse_document(&text) {
            Ok(doc) => Some(Recovered {
                doc,
                status: "Recovered the browser copy. Save it to keep it.".into(),
            }),
            Err(e) => {
                self.error = Some(format!("Cannot open the recovered copy: {e}"));
                None
            }
        }
    }
}
