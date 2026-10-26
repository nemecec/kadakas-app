//! The welcome dialog in the launcher (PLAT-26): the step before the first start, the window,
//! and `Show welcome`.
//!
//! The dialog is a window of its own process (`kadakas-launcher welcome`): the same way works on
//! every system, also on macOS, where the first process runs the system event loop on its main
//! thread. The launcher waits for that process before it starts any version, so the first
//! start always waits for a button, also when it came from a double click on a `.kad` file.
//!
//! The file reaches the app after the dialog. On macOS the system sends a second double click to
//! the one launcher, and its inbox keeps the file. On Windows and Linux a second double click
//! starts a second launcher. It writes one line to stderr, waits for the lock of the first one
//! (up to an hour), looks again, finds the dialog done and starts its own app with its own file.
//! There is no hand-over of the file between the two launchers on these systems.
//!
//! A dialog that ends with an error (the window could not open, a crash at start) does not stop
//! the start: the launcher logs it, writes no mark and starts the app with the settings as they
//! are. A dialog that is open but hidden still holds the start; `KADAKAS_WELCOME_TIMEOUT_SECS`
//! sets a limit (for tests).

use crate::welcome_ui::{self, WelcomeState};
use kadakas_l10n::tr;
use kadakas_welcome::gate::{self, Button, ENV_TIMEOUT, Gate, Kind};
use kadakas_welcome::{Dirs, ENV_SKIP, smoke, stores, sysloc};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The argument that tells the dialog process why it is shown.
pub fn kind_arg(kind: Kind) -> &'static str {
    match kind {
        Kind::FirstRun => "--first-run",
        Kind::Update => "--update",
        Kind::Manual => "--manual",
    }
}

pub fn kind_from_arg(arg: &str) -> Option<Kind> {
    [Kind::FirstRun, Kind::Update, Kind::Manual]
        .into_iter()
        .find(|k| kind_arg(*k) == arg)
}

/// Runs a child process and waits for it. An error: it could not start, it ended with a failure
/// status, or it did not end within `timeout` (it is killed then).
pub fn run_child(cmd: &mut Command, timeout: Option<Duration>) -> std::io::Result<()> {
    let mut child = cmd.spawn()?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(std::io::Error::other(format!(
                    "the dialog ended with {status}"
                )))
            };
        }
        if timeout.is_some_and(|t| start.elapsed() >= t) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "the dialog did not end in time",
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Runs the dialog in a process of its own and waits until it ends.
pub fn run_dialog_process(kind: Kind) -> std::io::Result<()> {
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("welcome")
        .arg(kind_arg(kind))
        .stdin(std::process::Stdio::null());
    run_child(
        &mut cmd,
        gate::timeout(std::env::var(ENV_TIMEOUT).ok().as_deref()),
    )
}

/// The step before the start: shows the dialog when it is due. Never when `KADAKAS_SKIP_WELCOME`
/// is set, and never without a display.
pub fn before_start(root: &Path) -> Gate {
    gate::run_gate(
        &Dirs::from_env(root.to_path_buf()),
        std::env::var(ENV_SKIP).ok().as_deref(),
        gate::has_display(std::env::consts::OS, |v| std::env::var_os(v)),
        run_dialog_process,
    )
}

/// True while a dialog that the person asked for is open (one at a time).
static MANUAL_OPEN: AtomicBool = AtomicBool::new(false);
/// Set when such a dialog ended; the manage window reads its settings again then.
static FINISHED: AtomicBool = AtomicBool::new(false);

/// True once after a dialog that the person asked for has ended.
pub fn take_finished() -> bool {
    FINISHED.swap(false, Ordering::SeqCst)
}

/// Show welcome: opens the dialog with the current settings, without waiting. A second request
/// while the dialog is open does nothing. When the dialog stores choices, it moves the stamp in
/// `welcome.json`; the running programs read their preferences again then.
pub fn show_again() {
    if MANUAL_OPEN.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        if !gate::skipped(std::env::var(ENV_SKIP).ok().as_deref())
            && let Err(e) = run_dialog_process(Kind::Manual)
        {
            eprintln!("kadakas-launcher: the welcome dialog did not finish: {e}");
        }
        FINISHED.store(true, Ordering::SeqCst);
        MANUAL_OPEN.store(false, Ordering::SeqCst);
    });
}

struct WelcomeApp {
    state: WelcomeState,
    dirs: Dirs,
    /// How many times the choices were stored (a press of a button, or the close button).
    attempts: u32,
    /// The mark could not be written: the launcher exits with an error.
    mark_failed: Arc<AtomicBool>,
    /// The language of the window title, to change the title when the language changes.
    title_language: &'static str,
    /// The test mode of the smoke test (PLAT-28). `None` in every normal run.
    smoke: Option<smoke::Auto>,
}

impl WelcomeApp {
    /// The test mode: writes the state file, waits, and answers with the second button.
    /// Returns true when the window can close.
    fn smoke_step(&mut self, ctx: &egui::Context) -> bool {
        let Some(auto) = self.smoke.as_mut() else {
            return false;
        };
        ctx.request_repaint_after(Duration::from_millis(100));
        match auto.step(Instant::now()) {
            smoke::Step::Wait => false,
            smoke::Step::Write => {
                let state = smoke::state_json(
                    env!("CARGO_PKG_VERSION"),
                    self.state.kind,
                    &self.state.choices,
                    self.title_language,
                    self.state.modeler_installed,
                );
                match smoke::write_state(auto.path(), &state) {
                    Ok(()) => auto.written(Instant::now()),
                    Err(e) => eprintln!(
                        "kadakas-launcher: the smoke state {} was not written: {e}",
                        auto.path().display()
                    ),
                }
                false
            }
            smoke::Step::Answer => self.finish(Button::Secondary),
        }
    }

    /// Stores what the button says. Returns true when the window can close.
    ///
    /// When something could not be written, the problem is shown in the dialog and the window
    /// stays. A later press tries again. The window closes when the storing worked (a Cancel
    /// stores nothing, so it always works) or after the second failed try, so a file that never
    /// can be written does not keep the person in the dialog.
    fn finish(&mut self, button: Button) -> bool {
        if self.attempts >= MAX_ATTEMPTS {
            return true;
        }
        self.attempts += 1;
        let problems = gate::finish(
            &self.dirs,
            self.state.kind,
            button,
            &self.state.choices,
            &self.state.initial,
            &self.state.detected,
        );
        self.mark_failed
            .store(gate::mark_failed(&problems), Ordering::SeqCst);
        if problems.is_empty() {
            self.state.problem = None;
            return true;
        }
        let text = tr!("welcome-failed", problems = problems.join("; "));
        eprintln!("kadakas-launcher: {text}");
        self.state.problem = Some(text);
        false
    }
}

/// The most tries to store the choices. After the last failed try the next press closes the window.
const MAX_ATTEMPTS: u32 = 2;

impl eframe::App for WelcomeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // The dialog speaks the language that is chosen in it, at once, title included.
        let language = self.state.language_tag();
        kadakas_l10n::set_language(language);
        if language != self.title_language {
            self.title_language = language;
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(tr!("welcome-title")));
        }
        // The close button of the window counts as the second button.
        if ctx.input(|i| i.viewport().close_requested()) {
            if !self.finish(Button::Secondary) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
            return;
        }
        if let Some(button) = welcome_ui::show(ui, &mut self.state)
            && self.finish(button)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if self.smoke_step(&ctx) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// Opens the dialog window and returns when it is closed. `Ok(false)`: the mark could not be
/// written, so the dialog would come again at every start.
pub fn run_window(root: PathBuf, kind: Kind, modeler_installed: bool) -> eframe::Result<bool> {
    let dirs = Dirs::from_env(root);
    let system = sysloc::read_system();
    let detected = kadakas_welcome::choices::detect(&system, modeler_installed);
    let choices = if kind == Kind::FirstRun {
        detected.clone()
    } else {
        stores::current(&dirs, &system.languages, modeler_installed)
    };
    let follows = kind != Kind::FirstRun && stores::number_follows_language(&dirs);
    let state = WelcomeState::new(kind, choices, detected, system.languages, modeler_installed)
        .follow_language_for_numbers(follows);
    let language = state.language_tag();
    kadakas_l10n::set_language(language);
    let title = tr!("welcome-title");
    let mark_failed = Arc::new(AtomicBool::new(false));
    let app = WelcomeApp {
        state,
        dirs,
        attempts: 0,
        mark_failed: mark_failed.clone(),
        title_language: language,
        smoke: smoke::Auto::from_env(|k| std::env::var(k).ok()),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([welcome_ui::WIDTH, 560.0])
            .with_min_inner_size([480.0, 420.0])
            .with_title(title)
            .with_active(true),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "Kadakas welcome",
        options,
        Box::new(move |_cc| Ok(Box::new(app))),
    )?;
    Ok(!mark_failed.load(Ordering::SeqCst))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_update::inbox::Inbox;
    use kadakas_welcome::choices::detect;
    use std::cell::RefCell;
    use std::ffi::OsString;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "kadakas-launcher-welcome-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn dirs(root: &Path) -> Dirs {
        Dirs {
            launcher: root.join("launcher"),
            prefs: Some(root.join("config")),
            feedback: Some(root.join("feedback")),
        }
    }

    #[test]
    fn the_arguments_of_the_dialog_process_round_trip() {
        for k in [Kind::FirstRun, Kind::Update, Kind::Manual] {
            assert_eq!(kind_from_arg(kind_arg(k)), Some(k));
        }
        assert_eq!(kind_from_arg("--other"), None);
    }

    #[test]
    fn a_first_start_with_a_file_shows_the_dialog_first_and_the_file_opens_after() {
        let base = tmp("queue");
        let d = dirs(&base);
        let inbox = Inbox::new(&d.launcher);
        let log: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let start = |skip: Option<&str>, display: bool, files: Vec<OsString>| {
            let gate = gate::run_gate(&d, skip, display, |kind| {
                log.borrow_mut().push(format!("dialog {kind:?}"));
                // The person double clicked the file once more while the dialog was open: the
                // launcher queues it. Then a button is pressed.
                inbox.deliver(vec![OsString::from("/docs/second.kad")]);
                let c = detect(&kadakas_welcome::SystemLocale::default(), true);
                gate::finish(&d, kind, Button::Secondary, &c, &c, &c);
                Ok(())
            });
            // The supervisor starts the app with the files of the start and the queued ones.
            let mut all = files;
            all.extend(inbox.take_pending());
            log.borrow_mut().push(format!("app {all:?}"));
            gate
        };
        let first = start(None, true, vec![OsString::from("/docs/first.kad")]);
        assert_eq!(first, Gate::Shown(Kind::FirstRun));
        assert_eq!(
            *log.borrow(),
            vec![
                "dialog FirstRun".to_string(),
                r#"app ["/docs/first.kad", "/docs/second.kad"]"#.to_string()
            ],
            "the dialog first, then the app with both files"
        );
        // The second start has no dialog.
        log.borrow_mut().clear();
        let second = start(None, true, vec![OsString::from("/docs/first.kad")]);
        assert_eq!(second, Gate::NotDue);
        assert_eq!(log.borrow().len(), 1, "{:?}", log.borrow());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_variable_and_a_missing_display_keep_the_dialog_away_from_a_fresh_install() {
        let base = tmp("never");
        let d = dirs(&base);
        let shown = std::cell::Cell::new(false);
        let show = |_k: Kind| {
            shown.set(true);
            Ok(())
        };
        assert_eq!(gate::run_gate(&d, Some("1"), true, show), Gate::Skipped);
        assert_eq!(gate::run_gate(&d, None, false, show), Gate::NoDisplay);
        assert!(!shown.get());
        assert!(!base.exists(), "nothing is written");
    }

    #[test]
    fn a_dialog_process_that_fails_or_does_not_end_is_an_error_not_a_hang() {
        // The test program itself is the child: it runs no test when the name matches none.
        let me = std::env::current_exe().unwrap();
        let mut ok = Command::new(&me);
        ok.args(["--exact", "no_such_test_name", "--quiet"]);
        assert!(run_child(&mut ok, Some(Duration::from_secs(60))).is_ok());
        // A flag that the test harness refuses ends with a failure status.
        let mut bad = Command::new(&me);
        bad.arg("--definitely-not-a-flag");
        let e = run_child(&mut bad, Some(Duration::from_secs(60))).unwrap_err();
        assert!(e.to_string().contains("ended with"), "{e}");
        // A program that cannot start.
        let mut missing = Command::new(me.with_file_name("no-such-program-xyz"));
        assert!(run_child(&mut missing, None).is_err());
        // A child that outlives the limit is killed.
        let mut slow = Command::new(&me);
        slow.args([
            "--exact",
            "welcome::tests::sleeper_for_the_timeout_test",
            "--ignored",
            "--quiet",
        ]);
        let start = Instant::now();
        let e = run_child(&mut slow, Some(Duration::from_millis(400))).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(20));
    }

    /// Run by the test above as a child process; it sleeps longer than the limit.
    #[test]
    #[ignore = "a child of a test"]
    fn sleeper_for_the_timeout_test() {
        std::thread::sleep(Duration::from_secs(60));
    }

    #[test]
    fn the_gate_survives_a_failing_dialog_and_the_next_start_asks_again() {
        let base = tmp("fails");
        let d = dirs(&base);
        let r = gate::run_gate(&d, None, true, |_| {
            let mut cmd = Command::new(std::env::current_exe().unwrap());
            cmd.arg("--definitely-not-a-flag");
            run_child(&mut cmd, Some(Duration::from_secs(60)))
        });
        assert_eq!(r, Gate::Failed);
        assert_eq!(kadakas_update::welcome::done_version(&d.launcher), 0);
        assert_eq!(gate::kind_for(&d), Kind::FirstRun);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_dialog_that_cannot_store_its_choices_says_so_and_closes_on_the_second_press() {
        let base = tmp("unwritable");
        std::fs::create_dir_all(&base).unwrap();
        let blocker = base.join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        // The install folder cannot be made: the mark cannot be written.
        let dirs = Dirs {
            launcher: blocker.join("l"),
            prefs: None,
            feedback: None,
        };
        let c = detect(&kadakas_welcome::SystemLocale::default(), true);
        let state = WelcomeState::new(Kind::FirstRun, c.clone(), c, vec![], true);
        let failed = Arc::new(AtomicBool::new(false));
        let mut app = WelcomeApp {
            state,
            dirs,
            attempts: 0,
            mark_failed: failed.clone(),
            title_language: "en-US",
            smoke: None,
        };
        assert!(!app.finish(Button::Primary), "the window stays open");
        let shown = app.state.problem.clone().unwrap();
        assert!(shown.contains("welcome.json"), "{shown}");
        assert!(
            failed.load(Ordering::SeqCst),
            "the exit status will be an error"
        );
        // A second press tries again; the file still cannot be written: the window stays.
        assert!(!app.finish(Button::Primary), "the second try fails too");
        // After the last failed try the next press closes the window.
        assert!(app.finish(Button::Primary));
        // A problem that goes away: the second press stores the choices and closes.
        let c = detect(&kadakas_welcome::SystemLocale::default(), true);
        let again_dirs = Dirs {
            launcher: base.join("again").join("l"),
            prefs: None,
            feedback: None,
        };
        std::fs::write(base.join("again"), b"x").unwrap();
        let ok_flag = Arc::new(AtomicBool::new(false));
        let mut again = WelcomeApp {
            state: WelcomeState::new(Kind::FirstRun, c.clone(), c, vec![], true),
            dirs: again_dirs.clone(),
            attempts: 0,
            mark_failed: ok_flag.clone(),
            title_language: "en-US",
            smoke: None,
        };
        assert!(!again.finish(Button::Primary));
        assert!(ok_flag.load(Ordering::SeqCst));
        std::fs::remove_file(base.join("again")).unwrap();
        assert!(
            again.finish(Button::Primary),
            "stored at last, so it closes"
        );
        assert!(again.state.problem.is_none());
        assert!(!ok_flag.load(Ordering::SeqCst), "no error exit any more");
        assert_eq!(
            kadakas_update::welcome::done_version(&again_dirs.launcher),
            1
        );
        // Files that were written do not make an error exit.
        let ok_dirs = dirs_for(&base.join("ok"));
        let c = detect(&kadakas_welcome::SystemLocale::default(), true);
        let mut fine = WelcomeApp {
            state: WelcomeState::new(Kind::FirstRun, c.clone(), c, vec![], true),
            dirs: ok_dirs,
            attempts: 0,
            mark_failed: Arc::new(AtomicBool::new(false)),
            title_language: "en-US",
            smoke: None,
        };
        assert!(fine.finish(Button::Secondary));
        assert!(fine.state.problem.is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    fn dirs_for(root: &Path) -> Dirs {
        dirs(root)
    }

    #[test]
    fn the_test_mode_writes_the_state_and_answers_use_defaults_on_its_own() {
        let base = tmp("smoke");
        let state_path = base.join("out").join("state.json");
        let env = |k: &str| match k {
            smoke::ENV_STATE => Some(state_path.to_string_lossy().into_owned()),
            smoke::ENV_HOLD => Some("0".to_string()),
            _ => None,
        };
        let c = detect(&kadakas_welcome::SystemLocale::default(), true);
        let d = dirs(&base);
        let mut app = WelcomeApp {
            state: WelcomeState::new(Kind::FirstRun, c.clone(), c, vec![], true),
            dirs: d.clone(),
            attempts: 0,
            mark_failed: Arc::new(AtomicBool::new(false)),
            title_language: "en-US",
            smoke: smoke::Auto::from_env(env),
        };
        let ctx = egui::Context::default();
        let mut closed = false;
        for _ in 0..20 {
            if app.smoke_step(&ctx) {
                closed = true;
                break;
            }
        }
        assert!(closed, "the dialog answered by itself");
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(v["kind"], "first_run");
        assert_eq!(v["shown"]["mode"], "kadakas-modeler");
        // "Use defaults" marked the dialog as seen and wrote the settings.
        assert_eq!(kadakas_update::welcome::done_version(&d.launcher), 1);
        assert!(d.launcher.join("welcome.json").is_file());
        // Without the variable nothing happens.
        let c = detect(&kadakas_welcome::SystemLocale::default(), true);
        let mut off = WelcomeApp {
            state: WelcomeState::new(Kind::FirstRun, c.clone(), c, vec![], true),
            dirs: dirs(&base.join("off")),
            attempts: 0,
            mark_failed: Arc::new(AtomicBool::new(false)),
            title_language: "en-US",
            smoke: smoke::Auto::from_env(|_| None),
        };
        for _ in 0..20 {
            assert!(!off.smoke_step(&ctx));
        }
        assert!(!base.join("off").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_second_show_request_while_one_is_open_does_nothing() {
        // The flag is process wide: set it by hand, as if a dialog were open.
        MANUAL_OPEN.store(true, Ordering::SeqCst);
        FINISHED.store(false, Ordering::SeqCst);
        show_again();
        assert!(!take_finished(), "no second dialog was started");
        MANUAL_OPEN.store(false, Ordering::SeqCst);
        assert!(!take_finished());
    }
}
