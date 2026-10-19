//! Crash reports in the app (PLAT-20): the capture at the start, the question after a crash, and
//! the clean exit.
//!
//! The decisions and the text are in `kadakas-crash` (headless, tested). This file keeps the
//! state of the window and draws it.
//!
//! - At the start, [`install`] puts the panic hook and the native crash handler in place. The
//!   file goes to `runs/` of the launcher, next to the run record.
//! - After an unclean end the launcher has kept the report (`crash-reports/`). [`CrashUi::start`]
//!   reads them and acts by the setting (Preferences > Privacy > Crash reports): *Never send*
//!   deletes them, *Always send* sends them, *Ask each time* opens the window.
//! - The window shows what the report is and the exact text that goes out, with a note and a
//!   contact field of the person. *Send*, *Do not send*, *Always send* (which only sets the
//!   setting). Closing the window decides nothing: the report stays for the next start.
//! - A clean exit deletes the crash file of this run ([`clean_exit`]).

use kadakas_crash::queue::{self, Kept};
use kadakas_crash::report::{Extras, Review};
use kadakas_crash::scrub::Context;
use kadakas_feedback::{CrashPolicy, Hub};
use kadakas_l10n::tr;
use kadakas_update::handshake::{AppLink, ENV_ROOT, ENV_RUN};
use std::collections::VecDeque;
use std::path::PathBuf;

/// The launcher folder: from the launcher's variable, else the default one.
pub fn root(get: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    kadakas_update::settings::launcher_root(get(ENV_ROOT))
}

/// Puts the capture in place. `get` reads the launcher's variables (`app_updates::launch_env`).
/// Without a launcher the app makes a run id of its own, so a crash is still kept.
pub fn install(get: &dyn Fn(&str) -> Option<String>) {
    let Some(root) = root(get) else { return };
    let link = AppLink::from_env(get);
    let run_id = match &link {
        Some(l) => l.run_id().to_string(),
        None => queue::standalone_id(std::process::id()),
    };
    let channel = link
        .as_ref()
        .and_then(AppLink::status)
        .map(|s| s.label)
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "standalone".into());
    let _ = get(ENV_RUN);
    kadakas_crash::capture::install(kadakas_crash::capture::Config {
        root,
        run_id,
        version: env!("CARGO_PKG_VERSION").into(),
        channel,
        os: kadakas_crash::capture::default_os(),
    });
}

/// Tells the capture which graphics adapter runs (`wgpu`).
pub fn set_gpu(adapter: &str, driver: &str, backend: &str) {
    kadakas_crash::capture::set_gpu(kadakas_crash::Gpu {
        adapter: adapter.into(),
        driver: driver.into(),
        backend: backend.into(),
    });
}

/// The program ends in the normal way: the crash file of this run goes.
pub fn clean_exit() {
    kadakas_crash::capture::clean_exit();
}

struct Current {
    kept: Kept,
    review: Review,
    /// The text for the window, made again when the note or the contact change.
    text: String,
    text_for: Extras,
}

#[derive(Default)]
pub struct CrashUi {
    root: Option<PathBuf>,
    waiting: VecDeque<Kept>,
    current: Option<Current>,
    /// Reports at the start (for "report n of m").
    total: usize,
    done: usize,
    scrub: Context,
    /// A line for the status bar.
    note: Option<String>,
    /// The person closed the window: the reports stay for the next start.
    postponed: bool,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl CrashUi {
    pub fn new(root: Option<PathBuf>) -> CrashUi {
        CrashUi {
            root,
            scrub: Context::from_env(),
            ..CrashUi::default()
        }
    }

    /// Reads the kept reports and acts by the setting. Call it once, after the outbox folder is
    /// set.
    pub fn start(&mut self, hub: &mut Hub) {
        let Some(root) = self.root.clone() else {
            return;
        };
        queue::adopt_standalone(&root, &kadakas_sys::process::pid_alive);
        let kept = queue::list(&root);
        if kept.is_empty() {
            return;
        }
        match hub.crash_policy() {
            CrashPolicy::Never => {
                for k in &kept {
                    queue::remove(&root, &k.id);
                }
            }
            CrashPolicy::Always => {
                for k in kept {
                    self.send_without_asking(hub, &k);
                }
            }
            CrashPolicy::Ask => {
                self.total = kept.len();
                self.waiting = kept.into();
            }
        }
    }

    fn send_without_asking(&mut self, hub: &mut Hub, k: &Kept) {
        let Some(root) = self.root.clone() else {
            return;
        };
        match Review::open(hub, &k.file, &self.scrub, now()) {
            Ok(Some(r)) => match r.send(hub) {
                Ok(()) => {
                    self.note = Some(tr!("crash-ui-queued"));
                    queue::remove(&root, &k.id);
                }
                Err(e) => self.note = Some(tr!("crash-ui-store-failed", error = e.to_string())),
            },
            // The same crash is in the outbox already, or the outbox refuses: the file goes only
            // in the first case.
            Ok(None) => queue::remove(&root, &k.id),
            Err(e) => self.note = Some(tr!("crash-ui-store-failed", error = e.to_string())),
        }
    }

    /// True while a report waits for a decision in this run.
    #[cfg(test)]
    pub fn is_open(&self) -> bool {
        !self.postponed && (self.current.is_some() || !self.waiting.is_empty())
    }

    /// Takes the line for the status bar, once.
    pub fn take_note(&mut self) -> Option<String> {
        self.note.take()
    }

    /// Opens the next report in the outbox (waiting for review), if none is open.
    fn advance(&mut self, hub: &mut Hub) {
        let Some(root) = self.root.clone() else {
            return;
        };
        while self.current.is_none() {
            let Some(kept) = self.waiting.pop_front() else {
                return;
            };
            match Review::open(hub, &kept.file, &self.scrub, now()) {
                Ok(Some(review)) => {
                    self.current = Some(Current {
                        kept,
                        review,
                        text: String::new(),
                        text_for: Extras {
                            note: "\u{0}".into(),
                            contact: String::new(),
                        },
                    });
                }
                // Counted into a report that is on its way: nothing to ask.
                Ok(None) => {
                    queue::remove(&root, &kept.id);
                    self.done += 1;
                }
                Err(e) => {
                    self.note = Some(tr!("crash-ui-store-failed", error = e.to_string()));
                    self.done += 1;
                }
            }
        }
    }

    /// The text of the open report, made again when the note or the contact changed.
    fn refresh_text(&mut self, hub: &mut Hub) {
        let Some(c) = &mut self.current else { return };
        if c.text_for != c.review.extras || c.text.is_empty() {
            c.text = c
                .review
                .payload(hub)
                .unwrap_or_else(|e| tr!("crash-ui-store-failed", error = e.to_string()));
            c.text_for = c.review.extras.clone();
        }
    }

    /// Send: the exact text is stored as approved and goes out.
    pub fn press_send(&mut self, hub: &mut Hub) {
        let Some(c) = self.current.take() else { return };
        let id = c.kept.id.clone();
        match c.review.send(hub) {
            Ok(()) => {
                if let Some(root) = &self.root {
                    queue::remove(root, &id);
                }
                self.note = Some(tr!("crash-ui-queued"));
            }
            Err(e) => self.note = Some(tr!("crash-ui-store-failed", error = e.to_string())),
        }
        self.done += 1;
    }

    /// Do not send: the report is deleted.
    pub fn press_do_not_send(&mut self, hub: &mut Hub) {
        let Some(c) = self.current.take() else { return };
        c.review.discard(hub);
        if let Some(root) = &self.root {
            queue::remove(root, &c.kept.id);
        }
        self.done += 1;
    }

    /// Always send: sets the preference, and sends this report and the others that wait. It
    /// changes nothing else.
    pub fn press_always(&mut self, hub: &mut Hub) {
        if let Err(e) = hub.set_crash_policy(CrashPolicy::Always) {
            self.note = Some(tr!("crash-ui-store-failed", error = e.to_string()));
            return;
        }
        self.press_send(hub);
        let rest: Vec<Kept> = self.waiting.drain(..).collect();
        for k in rest {
            self.send_without_asking(hub, &k);
            self.done += 1;
        }
    }

    /// The window was closed: no decision. The reports stay for the next start, and nothing
    /// stays in the outbox for them.
    pub fn press_close(&mut self, hub: &mut Hub) {
        if let Some(c) = self.current.take() {
            c.review.discard(hub);
        }
        self.waiting.clear();
        self.postponed = true;
    }

    /// Draws the window. Call it every frame.
    pub fn show(&mut self, ctx: &egui::Context, hub: &mut Hub) {
        if self.postponed {
            return;
        }
        self.advance(hub);
        self.refresh_text(hub);
        let Some(c) = &mut self.current else { return };
        let summary = tr!(
            "crash-ui-summary",
            version = c.kept.file.version.clone(),
            channel = c.kept.file.channel.clone(),
            kind = match c.kept.file.kind {
                kadakas_crash::CrashKind::Panic => tr!("crash-ui-kind-panic"),
                kadakas_crash::CrashKind::Native => tr!("crash-ui-kind-native"),
            },
            signature = c.review.signature().to_string()
        );
        let position = tr!(
            "crash-ui-counter",
            n = self.done + 1,
            total = self.total.max(self.done + 1)
        );
        let mut open = true;
        let mut action = None;
        egui::Window::new(tr!("crash-ui-title"))
            .id(egui::Id::new("win-crash-report"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .default_width(560.0)
            .show(ctx, |ui| {
                if self.total > 1 {
                    ui.weak(&position);
                }
                ui.label(tr!("crash-ui-intro"));
                ui.add_space(4.0);
                ui.label(tr!("crash-ui-what"));
                ui.add_space(4.0);
                ui.strong(&summary);
                ui.add_space(6.0);
                ui.label(tr!("crash-ui-note-label"));
                ui.add(
                    egui::TextEdit::multiline(&mut c.review.extras.note)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
                ui.label(tr!("crash-ui-contact-label"));
                ui.add(
                    egui::TextEdit::singleline(&mut c.review.extras.contact)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(6.0);
                ui.strong(tr!("crash-ui-exact"));
                egui::ScrollArea::vertical()
                    .id_salt("crash-exact")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        let mut shown: &str = &c.text;
                        ui.add(
                            egui::TextEdit::multiline(&mut shown)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY),
                        );
                    });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(tr!("crash-ui-send")).clicked() {
                        action = Some(Action::Send);
                    }
                    if ui.button(tr!("crash-ui-do-not-send")).clicked() {
                        action = Some(Action::DoNotSend);
                    }
                    if ui.button(tr!("crash-ui-always-send")).clicked() {
                        action = Some(Action::Always);
                    }
                });
                ui.weak(tr!("crash-ui-always-hint"));
                ui.weak(tr!("crash-ui-close-hint"));
            });
        match action {
            Some(Action::Send) => self.press_send(hub),
            Some(Action::DoNotSend) => self.press_do_not_send(hub),
            Some(Action::Always) => self.press_always(hub),
            None if !open => self.press_close(hub),
            None => {}
        }
    }

    #[cfg(test)]
    pub fn current_text(&self) -> Option<&str> {
        self.current.as_ref().map(|c| c.text.as_str())
    }

    #[cfg(test)]
    pub fn edit(&mut self, f: impl FnOnce(&mut Extras)) {
        if let Some(c) = &mut self.current {
            f(&mut c.review.extras);
        }
    }

    #[cfg(test)]
    pub fn open_next(&mut self, hub: &mut Hub) {
        self.advance(hub);
        self.refresh_text(hub);
    }

    #[cfg(test)]
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }
}

enum Action {
    Send,
    DoNotSend,
    Always,
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_crash::{CrashFile, CrashKind, Frame};
    use kadakas_feedback::Status;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("kadakas-crash-ui-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn crash(n: u32) -> CrashFile {
        let mut f = CrashFile::new(CrashKind::Panic);
        f.version = format!("0.5.{n}");
        f.channel = "stable".into();
        f.created = 100 + u64::from(n);
        f.message = "boom with /Users/someone/Oak.kad".into();
        f.thread = "main".into();
        f.frames = vec![Frame {
            function: format!("kadakas_core::fail{n}"),
            file: Some("/x/crates/kadakas-core/src/a.rs".into()),
            line: Some(3),
        }];
        f.commands = vec!["edit.move".into()];
        f
    }

    fn keep(root: &std::path::Path, n: u32) {
        let id = format!("{n:x}-a");
        queue::store(root, &id, &crash(n)).unwrap();
    }

    fn ui(root: &std::path::Path) -> CrashUi {
        CrashUi::new(Some(root.to_path_buf()))
    }

    #[test]
    fn never_send_deletes_the_reports_without_a_question() {
        let d = tmp("never");
        keep(&d, 1);
        let mut hub = Hub::in_memory();
        hub.set_crash_policy(CrashPolicy::Never).unwrap();
        let mut c = ui(&d);
        c.start(&mut hub);
        assert!(!c.is_open());
        assert_eq!(queue::count(&d), 0);
        assert!(hub.list().is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn always_send_sends_without_a_question_and_keeps_the_text_scrubbed() {
        let d = tmp("always");
        keep(&d, 1);
        keep(&d, 2);
        let mut hub = Hub::in_memory();
        hub.set_crash_policy(CrashPolicy::Always).unwrap();
        let mut c = ui(&d);
        c.start(&mut hub);
        assert!(!c.is_open());
        assert_eq!(queue::count(&d), 0);
        let out = hub.outgoing(true, 10, 1_000);
        assert_eq!(out.len(), 2);
        for o in &out {
            assert!(!o.payload.contains("Oak"), "{}", o.payload);
            assert!(o.payload.contains("\"crash\""));
        }
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn ask_shows_the_exact_text_and_sends_nothing_before_the_choice() {
        let d = tmp("ask");
        keep(&d, 1);
        let mut hub = Hub::in_memory();
        hub.set_send(Some(true)).unwrap();
        let mut c = ui(&d);
        c.start(&mut hub);
        assert!(c.is_open());
        c.open_next(&mut hub);
        let shown = c.current_text().unwrap().to_string();
        assert!(shown.contains("\"crash\""), "{shown}");
        assert!(!shown.contains("Oak"), "{shown}");
        assert!(
            hub.outgoing(true, 10, 1_000).is_empty(),
            "nothing goes before Send"
        );
        // The note and the contact change the text.
        c.edit(|e| {
            e.note = "drawing a bed".into();
            e.contact = "ann@example.org".into();
        });
        c.open_next(&mut hub);
        let shown2 = c.current_text().unwrap().to_string();
        assert!(shown2.contains("drawing a bed") && shown2.contains("ann@example.org"));
        // Send: the text that was shown is the text that goes out.
        c.press_send(&mut hub);
        let out = hub.outgoing(true, 10, 1_000);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].payload, shown2);
        assert_eq!(queue::count(&d), 0);
        assert!(!c.is_open());
        // The policy did not change.
        assert_eq!(hub.crash_policy(), CrashPolicy::Ask);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn do_not_send_deletes_and_sends_nothing() {
        let d = tmp("dont");
        keep(&d, 1);
        let mut hub = Hub::in_memory();
        let mut c = ui(&d);
        c.start(&mut hub);
        c.open_next(&mut hub);
        c.press_do_not_send(&mut hub);
        assert_eq!(queue::count(&d), 0);
        assert!(hub.list().iter().all(|r| r.status == Status::Withdrawn));
        assert!(hub.outgoing(true, 10, 1_000).is_empty());
        assert_eq!(hub.crash_policy(), CrashPolicy::Ask);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn always_send_in_the_dialog_sets_the_preference_and_sends_all() {
        let d = tmp("always-dialog");
        keep(&d, 1);
        keep(&d, 2);
        keep(&d, 3);
        let mut hub = Hub::in_memory();
        let mut c = ui(&d);
        c.start(&mut hub);
        assert_eq!(c.waiting(), 3);
        c.open_next(&mut hub);
        c.press_always(&mut hub);
        assert_eq!(hub.crash_policy(), CrashPolicy::Always);
        assert_eq!(hub.outgoing(true, 10, 1_000).len(), 3);
        assert_eq!(queue::count(&d), 0);
        // The person takes it back in Preferences: the next crash asks again.
        hub.set_crash_policy(CrashPolicy::Ask).unwrap();
        keep(&d, 4);
        let mut c2 = ui(&d);
        c2.start(&mut hub);
        assert!(c2.is_open());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn closing_the_window_decides_nothing() {
        let d = tmp("close");
        keep(&d, 1);
        keep(&d, 2);
        let mut hub = Hub::in_memory();
        let mut c = ui(&d);
        c.start(&mut hub);
        c.open_next(&mut hub);
        c.press_close(&mut hub);
        assert!(!c.is_open());
        assert_eq!(queue::count(&d), 2, "the reports stay on disk");
        assert!(hub.outgoing(true, 10, 1_000).is_empty());
        assert!(hub.list().iter().all(|r| r.status == Status::Withdrawn));
        // The next start asks again.
        let mut c2 = ui(&d);
        c2.start(&mut hub);
        assert_eq!(c2.waiting(), 2);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_same_crash_twice_is_one_question_and_one_report() {
        let d = tmp("twice");
        let mut hub = Hub::in_memory();
        hub.set_send(Some(true)).unwrap();
        keep(&d, 1);
        let mut c = ui(&d);
        c.start(&mut hub);
        c.open_next(&mut hub);
        c.press_send(&mut hub);
        // The same crash again (same signature and version), while the first waits to go out.
        queue::store(&d, "ff-b", &crash(1)).unwrap();
        let mut c2 = ui(&d);
        c2.start(&mut hub);
        c2.open_next(&mut hub);
        assert!(!c2.is_open(), "counted into the first report");
        assert_eq!(queue::count(&d), 0);
        let reports: Vec<_> = hub.list();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].count, 2);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn without_a_folder_nothing_happens() {
        let mut hub = Hub::in_memory();
        let mut c = CrashUi::new(None);
        c.start(&mut hub);
        assert!(!c.is_open());
        c.open_next(&mut hub);
    }

    #[test]
    fn the_window_draws_in_both_languages() {
        for tag in ["en-US", "et"] {
            kadakas_l10n::with_language(tag, || {
                let d = tmp(&format!("draw-{tag}"));
                keep(&d, 1);
                let mut hub = Hub::in_memory();
                let mut c = ui(&d);
                c.start(&mut hub);
                let ctx = egui::Context::default();
                let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                    c.show(ui.ctx(), &mut hub);
                });
                assert!(c.current_text().is_some_and(|t| t.contains("\"crash\"")));
                std::fs::remove_dir_all(&d).unwrap();
            });
        }
    }
}
