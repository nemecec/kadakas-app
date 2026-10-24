//! Updates in the app (PLAT-16): Preferences > Updates, the notice in the window, and the
//! restart to update.
//!
//! The app has no network code. The launcher (`kadakas-launcher`) starts the app, checks and
//! downloads in the background, and writes what it found to a status file. The app reads that
//! file, shows a quiet notice, and talks back with small files (`kadakas_update::handshake`):
//! "download now" (Notify only), "restart" with the open documents, and the clean-exit mark
//! that the launcher reads to find out whether the last run crashed. The settings are in
//! `launcher.json`, where the launcher reads them (`kadakas_update::Settings`, one schema).
//!
//! The decisions are in `kadakas-update` (headless, tested). This file keeps the state of the
//! window and draws.

use kadakas_l10n::tr;
use kadakas_update::handshake::{AppLink, Notice, RunStatus, SkipReason, notice};
use kadakas_update::settings::{MAX_KEEP_OLDER, valid_site};
use kadakas_update::{Channel, Settings, UpdateMode};
use std::path::PathBuf;
use std::time::SystemTime;

/// The variables of the launcher, taken out of the environment at the start (`capture_env`), so
/// that no child process of the app (a script, a converter) inherits the run id and writes the
/// clean-exit mark for it.
static CAPTURED: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();

/// Reads `KADAKAS_RUN_ID` and `KADAKAS_LAUNCHER_ROOT` and removes them from the environment.
/// Call it at the top of `main`, with the secrets (one thread only).
pub fn capture_env() {
    let taken = kadakas_sys::process::take_env(&[
        kadakas_update::handshake::ENV_RUN,
        kadakas_update::handshake::ENV_ROOT,
        kadakas_update::handshake::ENV_VIA,
    ]);
    let _ = CAPTURED.set(taken);
}

/// A variable of the launcher: from the captured ones if `capture_env` ran, else from the
/// environment.
pub fn launch_env(name: &str) -> Option<String> {
    match CAPTURED.get() {
        Some(v) => v.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()),
        None => std::env::var(name).ok(),
    }
}

/// How often the status file is looked at, in seconds.
const POLL_SECONDS: f64 = 2.0;

/// How often the requests to open documents are looked at, in seconds.
const OPEN_POLL_SECONDS: f64 = 0.5;

/// Where the restart stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Restart {
    #[default]
    None,
    /// The person has unsaved changes and is asked what to do.
    Asking,
    /// The window closes now, and the launcher starts the new version.
    Pending,
}

/// What the person pressed in the notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeAction {
    Restart,
    Later,
    Download,
}

/// What the person chose in the question about unsaved changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveChoice {
    SaveAndRestart,
    RestartWithoutSaving,
    Cancel,
}

pub struct Updates {
    link: Option<AppLink>,
    /// The alive stamp: a timer thread tells the launcher that this app lives, also while the
    /// window loop waits (a file dialog is open, a big file loads). Dropped with the app.
    _heartbeat: Option<kadakas_update::handshake::Heartbeat>,
    /// The lock of this app on its run: the launcher sees that the app lives, also when the
    /// launcher itself is gone. Held until the program ends.
    _run_lock: Option<std::fs::File>,
    /// The install folder of the launcher, where `launcher.json` is.
    root: Option<PathBuf>,
    status: Option<RunStatus>,
    stamp: Option<SystemTime>,
    next_poll: f64,
    next_open_poll: f64,
    /// The version whose notice the person put off with "Later" in this run.
    later: Option<String>,
    restart: Restart,
    skipped_told: bool,
    /// The settings as the window edits them.
    pub settings: Settings,
    saved: Settings,
    /// The text of the local site field, which may be wrong while the person types.
    site_text: String,
    /// An error to show in the status line.
    error: Option<String>,
}

impl Updates {
    /// Reads the variables that the launcher sets, and the settings.
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>) -> Updates {
        let link = AppLink::from_env(get);
        let root = link
            .as_ref()
            .map(|l| l.root().to_path_buf())
            .or_else(|| kadakas_update::settings::launcher_root(get("KADAKAS_LAUNCHER_ROOT")));
        Updates::new(link, root)
    }

    pub fn new(link: Option<AppLink>, root: Option<PathBuf>) -> Updates {
        let settings = root.as_deref().map(Settings::load).unwrap_or_default();
        Updates {
            _heartbeat: link.as_ref().map(AppLink::start_heartbeat),
            _run_lock: link.as_ref().and_then(AppLink::hold_run_lock),
            link,
            root,
            status: None,
            stamp: None,
            next_poll: 0.0,
            next_open_poll: 0.0,
            later: None,
            restart: Restart::None,
            skipped_told: false,
            site_text: settings.site.clone(),
            saved: settings.clone(),
            settings,
            error: None,
        }
    }

    /// True when the launcher started this program, so notices and the restart work.
    pub fn has_launcher(&self) -> bool {
        self.link.is_some()
    }

    /// Looks at the status file now and then. Returns true when it has changed.
    pub fn poll(&mut self, now: f64) -> bool {
        let Some(link) = &self.link else {
            return false;
        };
        if now < self.next_poll {
            return false;
        }
        self.next_poll = now + POLL_SECONDS;
        let stamp = link.status_stamp();
        if stamp == self.stamp && self.status.is_some() {
            return false;
        }
        self.stamp = stamp;
        let status = link.status();
        let changed = status != self.status;
        self.status = status;
        changed
    }

    /// Takes the documents that the launcher sent (a double click on a `.kad` file while this app
    /// runs). Looks at most twice a second.
    pub fn take_open(&mut self, now: f64) -> Vec<PathBuf> {
        let Some(link) = &self.link else {
            return Vec::new();
        };
        if now < self.next_open_poll {
            return Vec::new();
        }
        self.next_open_poll = now + OPEN_POLL_SECONDS;
        link.take_open()
    }

    /// The notice to show, if any.
    pub fn notice(&self) -> Option<Notice> {
        let status = self.status.as_ref()?;
        notice(self.settings.mode, &status.update, self.later.as_deref())
    }

    /// A sentence about the versions that the launcher skipped to start this one. It is given
    /// once.
    pub fn skipped_note(&mut self) -> Option<String> {
        if self.skipped_told {
            return None;
        }
        let status = self.status.as_ref()?;
        self.skipped_told = true;
        let first = status.skipped.first()?;
        Some(tr!(
            "app-updates-skipped",
            running = status.running.clone(),
            skipped = first.version.clone(),
            reason = reason_text(first.reason)
        ))
    }

    /// "Later": the notice of this version stays away.
    pub fn put_off(&mut self) {
        if let Some(Notice::Ready { version } | Notice::Available { version }) = self.notice() {
            self.later = Some(version);
        }
    }

    /// Asks the launcher to download the new version (Notify only).
    pub fn request_download(&mut self) {
        if let Some(link) = &self.link
            && let Err(e) = link.request_download()
        {
            self.error = Some(tr!("app-updates-request-failed", error = e.to_string()));
        }
    }

    pub fn restart(&self) -> Restart {
        self.restart
    }

    pub fn set_restart(&mut self, r: Restart) {
        self.restart = r;
    }

    /// Takes the error for the status line, once.
    pub fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    /// The program ends. If a restart is pending, tells the launcher with the open documents.
    /// Then writes the clean-exit mark, as the very last thing of a normal exit.
    pub fn finish(&mut self, documents: &[PathBuf]) {
        // A clean end: the crash file of this run (if a thread panicked and the app lived on)
        // is no crash report (PLAT-20).
        crate::crash_ui::clean_exit();
        let Some(link) = &self.link else {
            return;
        };
        if self.restart == Restart::Pending {
            let _ = link.request_restart(documents);
        }
        let _ = link.mark_clean_exit();
    }

    /// The restart did not happen (the close was cancelled): takes the request back.
    pub fn cancel_restart(&mut self) {
        if let Some(link) = &self.link {
            link.cancel_restart();
        }
        self.restart = Restart::None;
    }

    /// Writes the settings when the window changed them. The launcher reads them at its next
    /// cycle. The write keeps the parts that only the launcher writes (flavor, profile).
    pub fn save_if_changed(&mut self) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if self.settings == self.saved {
            return;
        }
        let want = self.settings.clone();
        match Settings::update(&root, |s| {
            s.mode = want.mode;
            s.channel = want.channel;
            s.keep_older = want.keep_older;
            s.site = want.site.clone();
        }) {
            Ok(saved) => {
                self.settings = Settings {
                    flavor: saved.flavor.clone(),
                    profile: saved.profile.clone(),
                    ..self.settings.clone()
                };
                self.saved = self.settings.clone();
            }
            Err(e) => {
                self.error = Some(tr!("app-updates-save-failed", error = e.to_string()));
                self.saved = self.settings.clone();
            }
        }
    }

    #[cfg(test)]
    pub fn root(&self) -> Option<&std::path::Path> {
        self.root.as_deref()
    }
}

fn reason_text(reason: SkipReason) -> String {
    match reason {
        SkipReason::Crashed => tr!("app-updates-reason-crashed"),
        SkipReason::RolledBack => tr!("app-updates-reason-rolled-back"),
        SkipReason::NotStarted => tr!("app-updates-reason-not-started"),
    }
}

fn mode_name(m: UpdateMode) -> String {
    match m {
        UpdateMode::AutoUpdate => tr!("app-updates-mode-auto-update"),
        UpdateMode::AutoDownload => tr!("app-updates-mode-auto-download"),
        UpdateMode::NotifyOnly => tr!("app-updates-mode-notify-only"),
    }
}

fn mode_hint(m: UpdateMode) -> String {
    match m {
        UpdateMode::AutoUpdate => tr!("app-updates-hint-auto-update"),
        UpdateMode::AutoDownload => tr!("app-updates-hint-auto-download"),
        UpdateMode::NotifyOnly => tr!("app-updates-hint-notify-only"),
    }
}

fn channel_name(c: Channel) -> String {
    match c {
        Channel::Stable => tr!("app-updates-channel-stable"),
        Channel::Beta => tr!("app-updates-channel-nightly"),
        Channel::Local => tr!("app-updates-channel-local"),
    }
}

/// Preferences > Updates.
pub fn prefs_section(ui: &mut egui::Ui, u: &mut Updates) {
    ui.heading(tr!("app-updates-heading"));
    ui.horizontal(|ui| {
        ui.label(tr!("app-updates-mode"));
        egui::ComboBox::from_id_salt("pref_update_mode")
            .selected_text(mode_name(u.settings.mode))
            .show_ui(ui, |ui| {
                for m in UpdateMode::ALL {
                    ui.selectable_value(&mut u.settings.mode, m, mode_name(m));
                }
            });
    });
    ui.small(mode_hint(u.settings.mode));
    ui.horizontal(|ui| {
        ui.label(tr!("app-updates-channel"));
        egui::ComboBox::from_id_salt("pref_update_channel")
            .selected_text(channel_name(u.settings.channel))
            .show_ui(ui, |ui| {
                for c in [Channel::Stable, Channel::Beta, Channel::Local] {
                    ui.selectable_value(&mut u.settings.channel, c, channel_name(c));
                }
            });
    });
    if u.settings.channel == Channel::Local {
        ui.horizontal(|ui| {
            ui.label(tr!("app-updates-site"));
            let edit = ui.add(
                egui::TextEdit::singleline(&mut u.site_text)
                    .hint_text(tr!("app-updates-site-hint"))
                    .desired_width(260.0),
            );
            if edit.changed() && valid_site(u.site_text.trim()) {
                u.settings.site = u.site_text.trim().to_string();
            }
        });
        if !valid_site(u.site_text.trim()) {
            ui.colored_label(ui.visuals().error_fg_color, tr!("app-updates-site-bad"));
        }
        ui.small(tr!("app-updates-local-hint"));
    }
    ui.horizontal(|ui| {
        ui.label(tr!("app-updates-keep-older"));
        ui.add(egui::DragValue::new(&mut u.settings.keep_older).range(0..=MAX_KEEP_OLDER));
    });
    ui.small(tr!("app-updates-keep-older-hint"));
    if !u.has_launcher() {
        ui.small(tr!("app-updates-no-launcher"));
    }
}

/// The quiet notice. Returns what the person pressed.
pub fn notice_bar(ui: &mut egui::Ui, n: &Notice) -> Option<NoticeAction> {
    let mut action = None;
    ui.horizontal(|ui| match n {
        Notice::Ready { version } => {
            ui.label(tr!("app-updates-ready", version = version.clone()));
            if ui.button(tr!("app-updates-restart")).clicked() {
                action = Some(NoticeAction::Restart);
            }
            if ui.button(tr!("app-updates-later")).clicked() {
                action = Some(NoticeAction::Later);
            }
        }
        Notice::Available { version } => {
            ui.label(tr!("app-updates-available", version = version.clone()));
            if ui.button(tr!("app-updates-download")).clicked() {
                action = Some(NoticeAction::Download);
            }
            if ui.button(tr!("app-updates-later")).clicked() {
                action = Some(NoticeAction::Later);
            }
        }
        Notice::Downloading { version } => {
            ui.spinner();
            ui.label(tr!("app-updates-downloading", version = version.clone()));
        }
    });
    action
}

/// What the person chose in the question about unsaved changes before a file replaces the drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenChoice {
    SaveAndOpen,
    DiscardAndOpen,
    Cancel,
}

/// What waits behind the question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskKind {
    /// A file of the person (File > Open, the launcher).
    Open,
    /// File > New.
    New,
    /// A file that an agent wanted to open (the call was refused).
    Agent,
}

/// The question about unsaved changes before something replaces the drawing. `name` is the file
/// (or the new drawing) that waits.
pub fn open_dialog(ctx: &egui::Context, kind: AskKind, name: &str) -> Option<OpenChoice> {
    let mut choice = None;
    egui::Window::new(tr!("app-open-title"))
        .id(egui::Id::new("win-open-unsaved"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(match kind {
                AskKind::Open => tr!("app-open-unsaved-question", name = name),
                AskKind::New => tr!("app-open-unsaved-new"),
                AskKind::Agent => tr!("app-open-unsaved-agent", name = name),
            });
            ui.horizontal(|ui| {
                if ui.button(tr!("app-open-save-and-open")).clicked() {
                    choice = Some(OpenChoice::SaveAndOpen);
                }
                if ui.button(tr!("app-open-dont-save")).clicked() {
                    choice = Some(OpenChoice::DiscardAndOpen);
                }
                let cancel = if kind == AskKind::Agent {
                    tr!("app-open-refuse")
                } else {
                    tr!("app-updates-cancel")
                };
                if ui.button(cancel).clicked() {
                    choice = Some(OpenChoice::Cancel);
                }
            });
        });
    choice
}

/// The question about unsaved changes before the restart.
pub fn save_dialog(ctx: &egui::Context) -> Option<SaveChoice> {
    let mut choice = None;
    egui::Window::new(tr!("app-updates-restart"))
        .id(egui::Id::new("win-update-restart"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(tr!("app-updates-save-question"));
            ui.horizontal(|ui| {
                if ui.button(tr!("app-updates-save-and-restart")).clicked() {
                    choice = Some(SaveChoice::SaveAndRestart);
                }
                if ui
                    .button(tr!("app-updates-restart-without-saving"))
                    .clicked()
                {
                    choice = Some(SaveChoice::RestartWithoutSaving);
                }
                if ui.button(tr!("app-updates-cancel")).clicked() {
                    choice = Some(SaveChoice::Cancel);
                }
            });
        });
    choice
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_update::handshake::{ENV_ROOT, ENV_RUN, Skipped, UpdateState, write_status};
    use kadakas_update::runs::run_file;
    use std::path::Path;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("kadakas-app-updates-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn env_of(root: &Path) -> impl Fn(&str) -> Option<String> {
        let root = root.to_string_lossy().into_owned();
        move |k| match k {
            ENV_ROOT => Some(root.clone()),
            ENV_RUN => Some("1a-2b".into()),
            _ => None,
        }
    }

    fn status(update: UpdateState) -> RunStatus {
        RunStatus {
            running: "1.0.0".into(),
            label: "stable".into(),
            update,
            skipped: vec![],
        }
    }

    fn ready() -> UpdateState {
        UpdateState::Ready {
            version: "1.1.0".into(),
        }
    }

    #[test]
    fn the_app_holds_a_lock_on_its_run_and_takes_no_variable_from_a_child() {
        let d = tmp("runlock");
        let u = Updates::from_env(&env_of(&d));
        // The lock is held: a second try on the same run fails.
        assert!(u.link.as_ref().unwrap().hold_run_lock().is_none());
        assert!(run_file(&d, "1a-2b", "app.lock").is_file());
        drop(u);
        assert!(
            AppLink::from_env(&env_of(&d))
                .unwrap()
                .hold_run_lock()
                .is_some(),
            "the lock goes with the app"
        );
        // Without `capture_env` the variables are read from the environment; one that is not
        // set is not there.
        assert_eq!(launch_env("KADAKAS_TEST_NOT_SET_ANYWHERE"), None);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn without_the_launcher_there_is_no_notice_and_no_file() {
        let d = tmp("nolauncher");
        let mut u = Updates::new(None, Some(d.clone()));
        assert!(!u.has_launcher());
        assert!(!u.poll(10.0));
        assert!(u.notice().is_none());
        u.finish(&[PathBuf::from("/a.kad")]);
        assert!(!d.join("runs").exists(), "nothing is written");
        u.cancel_restart();
        assert_eq!(u.restart(), Restart::None);
        u.request_download();
        assert!(u.take_error().is_none());
        // The settings still work: they are read when the launcher starts the app.
        u.settings.keep_older = 3;
        u.save_if_changed();
        assert_eq!(Settings::load(&d).keep_older, 3);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_notice_follows_the_status_file_and_the_mode() {
        let d = tmp("notice");
        let mut u = Updates::from_env(&env_of(&d));
        assert!(u.has_launcher());
        assert_eq!(u.root(), Some(d.as_path()));
        assert!(!u.poll(0.0), "no file yet");
        assert!(u.notice().is_none());

        write_status(&d, "1a-2b", &status(ready())).unwrap();
        // Looks again only after the poll time.
        assert!(!u.poll(1.0));
        assert!(u.poll(2.5));
        assert_eq!(
            u.notice(),
            Some(Notice::Ready {
                version: "1.1.0".into()
            })
        );
        // Nothing changed: no new read.
        assert!(!u.poll(5.0));
        // "Later" hides it for this run.
        u.put_off();
        assert!(u.notice().is_none());
        // A newer version brings it back.
        write_status(
            &d,
            "1a-2b",
            &status(UpdateState::Ready {
                version: "1.2.0".into(),
            }),
        )
        .unwrap();
        // The stamp may be the same second: force the read by using a new file time.
        u.stamp = None;
        assert!(u.poll(8.0));
        assert!(u.notice().is_some());

        // Notify only: "available" shows, and Download asks the launcher.
        u.settings.mode = UpdateMode::NotifyOnly;
        write_status(
            &d,
            "1a-2b",
            &status(UpdateState::Available {
                version: "1.3.0".into(),
                size: 5,
            }),
        )
        .unwrap();
        u.stamp = None;
        u.poll(11.0);
        assert_eq!(
            u.notice(),
            Some(Notice::Available {
                version: "1.3.0".into()
            })
        );
        u.request_download();
        assert!(run_file(&d, "1a-2b", "download").is_file());
        u.put_off();
        assert!(u.notice().is_none());
        // In the other modes the same status says nothing.
        u.later = None;
        u.settings.mode = UpdateMode::AutoUpdate;
        assert!(u.notice().is_none());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_skipped_version_is_told_once_and_in_both_languages() {
        let d = tmp("skipped");
        let mut u = Updates::from_env(&env_of(&d));
        assert!(u.skipped_note().is_none(), "no status yet");
        let mut s = status(UpdateState::Checking);
        s.skipped = vec![Skipped {
            version: "1.2.0".into(),
            reason: SkipReason::Crashed,
        }];
        write_status(&d, "1a-2b", &s).unwrap();
        u.poll(10.0);
        for tag in ["en-US", "et"] {
            kadakas_l10n::with_language(tag, || {
                u.skipped_told = false;
                let note = u.skipped_note().unwrap();
                assert!(note.contains("1.2.0") && note.contains("1.0.0"), "{note}");
                for r in [
                    SkipReason::Crashed,
                    SkipReason::RolledBack,
                    SkipReason::NotStarted,
                ] {
                    assert!(!reason_text(r).is_empty());
                }
            });
        }
        assert!(u.skipped_note().is_none(), "only once");
        // Nothing skipped: nothing to tell.
        let mut v = Updates::from_env(&env_of(&d));
        write_status(&d, "1a-2b", &status(UpdateState::UpToDate)).unwrap();
        v.poll(10.0);
        assert!(v.skipped_note().is_none());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn restart_hands_the_documents_to_the_launcher_and_marks_a_clean_exit() {
        let d = tmp("restart");
        let mut u = Updates::from_env(&env_of(&d));
        let docs = vec![PathBuf::from("/work/plan.kad")];
        // A normal exit: the mark, and no request.
        u.finish(&docs);
        assert!(run_file(&d, "1a-2b", "clean").is_file());
        assert!(!run_file(&d, "1a-2b", "restart.json").exists());
        std::fs::remove_file(run_file(&d, "1a-2b", "clean")).unwrap();

        // A restart: the request with the documents, and the mark.
        u.set_restart(Restart::Pending);
        u.finish(&docs);
        let req = kadakas_update::handshake::take_restart(&d, "1a-2b").unwrap();
        assert_eq!(req.documents, docs);
        assert!(run_file(&d, "1a-2b", "clean").is_file());

        // The close was cancelled after the request was written: it is taken back.
        u.set_restart(Restart::Pending);
        u.link.as_ref().unwrap().request_restart(&docs).unwrap();
        u.cancel_restart();
        assert_eq!(u.restart(), Restart::None);
        assert!(!run_file(&d, "1a-2b", "restart.json").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn settings_keep_what_the_launcher_wrote_and_limits_hold() {
        let d = tmp("settings");
        Settings {
            flavor: "kadakas-app".into(),
            profile: "acad".into(),
            ..Settings::default()
        }
        .save(&d)
        .unwrap();
        let mut u = Updates::new(None, Some(d.clone()));
        assert_eq!(u.settings.mode, UpdateMode::AutoUpdate);
        assert_eq!(u.settings.keep_older, 1);
        assert_eq!(u.settings.channel, Channel::Stable);
        u.settings.mode = UpdateMode::NotifyOnly;
        u.settings.channel = Channel::Local;
        u.settings.site = "file:///tmp/site".into();
        u.settings.keep_older = 0;
        u.save_if_changed();
        let on_disk = Settings::load(&d);
        assert_eq!(on_disk.mode, UpdateMode::NotifyOnly);
        assert_eq!(on_disk.channel, Channel::Local);
        assert_eq!(on_disk.site, "file:///tmp/site");
        assert_eq!(on_disk.keep_older, 0);
        assert_eq!(on_disk.profile, "acad", "the launcher's part stays");
        // A number over the limit is cut by the settings schema.
        u.settings.keep_older = 9999;
        u.save_if_changed();
        assert_eq!(Settings::load(&d).keep_older, MAX_KEEP_OLDER);
        // Nothing changed: nothing is written.
        let stamp = std::fs::metadata(Settings::path(&d))
            .unwrap()
            .modified()
            .unwrap();
        u.save_if_changed();
        assert_eq!(
            std::fs::metadata(Settings::path(&d))
                .unwrap()
                .modified()
                .unwrap(),
            stamp
        );
        // A folder that cannot be written is an error for the status line.
        // (A folder below a plain file cannot be made, on every system.)
        let blocker = d.join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        let mut bad = Updates::new(None, Some(blocker.join("launcher")));
        bad.settings.keep_older = 5;
        bad.save_if_changed();
        assert!(bad.take_error().is_some());
        assert!(bad.take_error().is_none());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_window_pieces_draw_in_both_languages() {
        let ctx = egui::Context::default();
        let d = tmp("draw");
        let mut u = Updates::new(None, Some(d.clone()));
        u.settings.channel = Channel::Local;
        u.site_text = "http://not-a-file".into();
        let mut pressed = Vec::new();
        for tag in ["en-US", "et"] {
            kadakas_l10n::with_language(tag, || {
                let _ = ctx.run_ui(Default::default(), |ui| {
                    prefs_section(ui, &mut u);
                    for n in [
                        Notice::Ready {
                            version: "1.1.0".into(),
                        },
                        Notice::Available {
                            version: "1.1.0".into(),
                        },
                        Notice::Downloading {
                            version: "1.1.0".into(),
                        },
                    ] {
                        pressed.push(notice_bar(ui, &n));
                    }
                    let _ = save_dialog(ui.ctx());
                });
            });
        }
        // Nobody pressed anything.
        assert!(pressed.iter().all(Option::is_none));
        for m in UpdateMode::ALL {
            assert!(!mode_name(m).is_empty() && !mode_hint(m).is_empty());
        }
        assert_eq!(channel_name(Channel::Beta), "Nightly");
        // A wrong address is not taken.
        assert!(u.settings.site.is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
