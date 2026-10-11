//! The autosave state of the app, the recovery dialog and the Autosave settings (shown in `prefs_ui`).

use kadakas_core::model::Document;
use kadakas_l10n::tr;

use crate::autosave::{AutosaveSettings, Scheduler};

/// What the user chose in the recovery dialog.
pub struct Recovered {
    pub doc: Document,
    pub status: String,
}

pub struct AutosaveUi {
    pub settings: AutosaveSettings,
    sched: Scheduler,
    want_prefs: bool,
    /// The last problem, shown in the Preferences dialog.
    error: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    recovery: Option<crate::autosave::Recovery>,
    #[cfg(not(target_arch = "wasm32"))]
    crashed: Vec<crate::autosave::Crashed>,
    /// A recovered session whose files stay until the recovered document has been autosaved.
    #[cfg(not(target_arch = "wasm32"))]
    keep: Option<crate::autosave::Crashed>,
    /// The number of the autosave job that makes `keep` unnecessary.
    #[cfg(not(target_arch = "wasm32"))]
    keep_until: Option<u64>,
    /// Save at the next frame, whatever the schedule says (after a recovery).
    force: bool,
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
            want_prefs: false,
            error: None,
            #[cfg(not(target_arch = "wasm32"))]
            recovery: None,
            #[cfg(not(target_arch = "wasm32"))]
            crashed: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            keep: None,
            #[cfg(not(target_arch = "wasm32"))]
            keep_until: None,
            force: false,
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
                Err(e) => self.error = Some(tr!("autosave-ui-autosave-is-off", e = e)),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.web_copy = crate::autosave::web::get();
        }
    }

    /// Call once per frame. Saves a copy when one is due. The document is copied (cheap compared
    /// with writing JSON); the writer thread makes the JSON and writes the file.
    pub fn tick(
        &mut self,
        ctx: &egui::Context,
        revision: u64,
        path: Option<&std::path::Path>,
        doc: &Document,
    ) {
        let now = ctx.input(|i| i.time);
        if self.settings.enabled {
            // Wake up now and then, so that a time-based save also happens without input.
            ctx.request_repaint_after(std::time::Duration::from_secs(30));
        }
        self.release_recovered_files();
        let due = self.settings.enabled
            && (std::mem::take(&mut self.force) || self.sched.due(&self.settings, now, revision));
        if !due {
            return;
        }
        self.sched.mark(now, revision);
        self.submit(doc, path);
    }

    /// Deletes the files of a recovered session once the recovered document is safe in a new autosave.
    fn release_recovered_files(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(seq), Some(r)) = (self.keep_until, &self.recovery)
            && r.completed() >= seq
        {
            self.keep_until = None;
            if let Some(c) = self.keep.take() {
                c.discard();
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn submit(&mut self, doc: &Document, path: Option<&std::path::Path>) {
        if let Some(r) = &mut self.recovery {
            let over = path
                .filter(|_| self.settings.save_over_file)
                .map(std::path::Path::to_path_buf);
            let manual_seen = over.as_deref().map_or(0, crate::autosave::manual_saves);
            let seq = r.submit(crate::autosave::Job {
                doc: doc.clone(),
                original: path.map(std::path::Path::to_path_buf),
                keep: self.settings.keep as usize,
                over,
                manual_seen,
            });
            if self.keep.is_some() {
                self.keep_until = Some(seq);
            }
            self.error = r
                .last_error()
                .map(|e| tr!("autosave-ui-last-autosave-failed", e = e));
        }
    }

    /// The browser has no writer thread. Large documents are skipped: local storage holds about 5 MB.
    #[cfg(target_arch = "wasm32")]
    fn submit(&mut self, doc: &Document, _path: Option<&std::path::Path>) {
        self.error = match serde_json::to_string(doc) {
            Ok(text) if crate::autosave::fits_browser_storage(text.len()) => {
                crate::autosave::web::put(&text)
                    .err()
                    .map(|e| tr!("autosave-ui-last-autosave-failed", e = e))
            }
            Ok(text) => Some(tr!(
                "autosave-ui-no-autosave-the-drawing-is-mb-more",
                size = text.len() / 1_000_000 + 1
            )),
            Err(e) => Some(tr!("autosave-ui-no-autosave", e = e)),
        };
    }

    /// The user saved the document: an old browser copy is not needed any more.
    #[cfg(target_arch = "wasm32")]
    pub fn saved(&mut self) {
        crate::autosave::web::clear();
    }

    /// Clean exit: removes the lock and the autosaved versions.
    pub fn finish(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(r) = self.recovery.take() {
                r.finish();
            }
            if let Some(c) = self.keep.take() {
                c.discard();
            }
        }
    }

    /// Shows the dialogs. Returns a document when the user opens a recovered copy.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Recovered> {
        self.recovery_dialog(ctx)
    }

    /// The Autosave part of the Preferences dialog (see `prefs_ui`). A change is sanitized and
    /// saved at once.
    pub fn settings_section(&mut self, ui: &mut egui::Ui) {
        let before = self.settings.clone();
        ui.heading(tr!("autosave-ui-autosave"));
        let s = &mut self.settings;
        ui.checkbox(
            &mut s.enabled,
            tr!("autosave-ui-save-a-recovery-copy-while-i-work"),
        );
        ui.add_enabled_ui(s.enabled, |ui| {
            ui.horizontal(|ui| {
                ui.label(tr!("autosave-ui-every"));
                ui.add(
                    egui::DragValue::new(&mut s.interval_minutes)
                        .range(1..=AutosaveSettings::MAX_MINUTES),
                );
                ui.label(tr!("autosave-ui-minutes-with-changes"));
            });
            ui.horizontal(|ui| {
                ui.label(tr!("autosave-ui-or-after"));
                ui.add(egui::DragValue::new(&mut s.every_ops).range(0..=10_000));
                ui.label(tr!("autosave-ui-operations-0-off"));
            });
            ui.horizontal(|ui| {
                ui.label(tr!("autosave-ui-keep-the-last"));
                ui.add(egui::DragValue::new(&mut s.keep).range(1..=AutosaveSettings::MAX_KEEP));
                ui.label(tr!("autosave-ui-versions-label"));
            });
            #[cfg(not(target_arch = "wasm32"))]
            ui.checkbox(
                &mut s.save_over_file,
                tr!("autosave-ui-also-save-over-my-file-off-recovery"),
            );
        });
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(dir) = crate::autosave::recovery_dir() {
            ui.label(tr!("autosave-ui-recovery-folder", path = dir.display()));
            if ui.button(tr!("autosave-ui-show-recovery-folder")).clicked() {
                let _ = std::fs::create_dir_all(&dir);
                let _ = crate::autosave::reveal(&dir);
            }
        }
        #[cfg(target_arch = "wasm32")]
        ui.label(tr!("autosave-ui-the-browser-keeps-one-recovery-copy-in"));
        if let Some(e) = &self.error {
            ui.colored_label(ui.visuals().warn_fg_color, e);
        }
        self.settings = self.settings.clone().sanitized();
        if self.settings != before {
            #[cfg(not(target_arch = "wasm32"))]
            if let Err(e) = crate::autosave::save_settings(&self.settings) {
                self.error = Some(tr!("autosave-ui-cannot-save-the-preferences", e = e));
            }
        }
    }

    /// True once after the recovery dialog failed: the error shows in the Preferences dialog.
    pub fn take_prefs_request(&mut self) -> bool {
        std::mem::take(&mut self.want_prefs)
    }

    /// Opens a recovered copy. Its files stay until the first autosave of the document is on disk,
    /// so a second crash right after the recovery loses nothing.
    #[cfg(not(target_arch = "wasm32"))]
    fn open_recovered(&mut self, i: usize) -> Option<Recovered> {
        match self.crashed[i].load() {
            Ok((doc, from)) => {
                let status = tr!(
                    "autosave-ui-recovered-save-it-to-keep-it",
                    name = from.display()
                );
                let session = self.crashed.remove(i);
                if self.settings.enabled && self.recovery.is_some() {
                    // An older pending session is safe once the newer one is kept.
                    if let Some(old) = self.keep.replace(session) {
                        old.discard();
                    }
                    self.keep_until = None;
                    self.force = true;
                } else {
                    // No autosave: the user chose to keep no copies.
                    session.discard();
                }
                Some(Recovered { doc, status })
            }
            Err(e) => {
                self.error = Some(tr!("autosave-ui-cannot-open-the-recovered-copy", e = e));
                None
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
        egui::Window::new(tr!("autosave-ui-recover-unsaved-work"))
            .id(egui::Id::new("win-recover-unsaved-work"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(tr!("autosave-ui-kadakas-did-not-close-cleanly-last-time"));
                let now = crate::autosave::now_unix();
                for (i, c) in self.crashed.iter().enumerate() {
                    ui.separator();
                    let from = c
                        .original
                        .as_ref()
                        .map_or(tr!("autosave-ui-an-untitled-drawing"), |p| {
                            p.display().to_string()
                        });
                    let age = c.saved_unix.map_or(String::new(), |t| {
                        tr!(
                            "autosave-ui-saved",
                            when = crate::autosave::age_text(now.saturating_sub(t))
                        )
                    });
                    ui.label(tr!(
                        "autosave-ui-versions",
                        from = from,
                        age = age,
                        count = c.versions.len()
                    ));
                    ui.horizontal(|ui| {
                        if ui.button(tr!("autosave-ui-open-recovered-copy")).clicked() {
                            choice = Some(Choice::Open(i));
                        }
                        if ui.button(tr!("autosave-ui-discard")).clicked() {
                            choice = Some(Choice::Discard(i));
                        }
                        if ui.button(tr!("autosave-ui-show-location")).clicked() {
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
            Choice::Open(i) => {
                let r = self.open_recovered(i);
                if r.is_none() {
                    self.want_prefs = true;
                }
                r
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn recovery_dialog(&mut self, ctx: &egui::Context) -> Option<Recovered> {
        self.web_copy.as_ref()?;
        let mut choice = None;
        egui::Window::new(tr!("autosave-ui-recover-unsaved-work"))
            .id(egui::Id::new("win-recover-unsaved-work"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(tr!("autosave-ui-this-browser-kept-an-autosaved-copy-from"));
                ui.horizontal(|ui| {
                    if ui.button(tr!("autosave-ui-open-recovered-copy")).clicked() {
                        choice = Some(true);
                    }
                    if ui.button(tr!("autosave-ui-discard")).clicked() {
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
                status: tr!("autosave-ui-recovered-the-browser-copy-save-it-to"),
            }),
            Err(e) => {
                self.error = Some(tr!("autosave-ui-cannot-open-the-recovered-copy", e = e));
                None
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::autosave::{Job, Recovery, find_crashed_with};
    use std::time::Duration;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("kadakas-ui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn job(doc: &Document) -> Job {
        Job {
            doc: doc.clone(),
            original: None,
            keep: 3,
            over: None,
            manual_seen: 0,
        }
    }

    /// A session that wrote one version and then crashed.
    fn crashed_session(dir: &std::path::Path) {
        let mut old = Recovery::start(dir.to_path_buf()).unwrap();
        let n = old.submit(job(&Document::default()));
        while old.completed() < n {
            std::thread::sleep(Duration::from_millis(5));
        }
        old.simulate_crash();
    }

    #[test]
    fn recovery_files_stay_until_the_recovered_document_is_autosaved() {
        let dir = temp("keep");
        crashed_session(&dir);
        let mut ui = AutosaveUi::inert();
        ui.crashed = find_crashed_with(&dir, Duration::ZERO);
        assert_eq!(ui.crashed.len(), 1);
        ui.recovery = Some(Recovery::start(dir.clone()).unwrap());

        let r = ui.open_recovered(0).expect("opens");
        assert_eq!(r.doc, Document::default());
        let files = |dir: &std::path::Path| std::fs::read_dir(dir).unwrap().count();
        assert!(ui.keep.is_some(), "the recovery files are kept");
        assert!(
            files(&dir) >= 3,
            "lock, version and meta of the old session stay"
        );

        // The next frame saves the recovered document. Only then are the old files released.
        let ctx = egui::Context::default();
        ui.tick(&ctx, 0, None, &r.doc);
        let seq = ui.keep_until.expect("a job is waiting");
        while ui.recovery.as_ref().unwrap().completed() < seq {
            std::thread::sleep(Duration::from_millis(5));
        }
        ui.tick(&ctx, 0, None, &r.doc);
        assert!(ui.keep.is_none());
        assert!(find_crashed_with(&dir, Duration::ZERO).is_empty());
        ui.finish();
        assert_eq!(files(&dir), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn with_autosave_off_a_recovery_is_not_kept() {
        let dir = temp("off");
        crashed_session(&dir);
        let mut ui = AutosaveUi::inert();
        ui.settings.enabled = false;
        ui.crashed = find_crashed_with(&dir, Duration::ZERO);
        assert!(ui.open_recovered(0).is_some());
        assert!(ui.keep.is_none());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_damaged_recovery_reports_an_error_and_keeps_the_files() {
        let dir = temp("damaged");
        crashed_session(&dir);
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            if e.file_name().to_string_lossy().ends_with(".autosave.kad") {
                std::fs::write(e.path(), "{broken").unwrap();
            }
        }
        let mut ui = AutosaveUi::inert();
        ui.crashed = find_crashed_with(&dir, Duration::ZERO);
        assert!(ui.open_recovered(0).is_none());
        assert_eq!(ui.crashed.len(), 1);
        assert!(ui.error.is_some());
        let _ = std::fs::remove_dir_all(dir);
    }
}
