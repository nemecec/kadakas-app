//! The welcome dialog in the launcher (PLAT-26): the step before the first start, the window,
//! and `Show welcome`.
//!
//! The dialog is a window of its own process (`kadakas-launcher welcome`): the same way works on
//! every system, also on macOS, where the first process runs the system event loop on its main
//! thread. The launcher waits for that process before it starts any version, so the first
//! start always waits for a button, also when it came from a double click on a `.kad` file (the
//! inbox of the launcher keeps the file; the app gets it after the dialog).

use crate::welcome_ui::{self, WelcomeState};
use kadakas_l10n::tr;
use kadakas_welcome::gate::{self, Button, Gate, Kind};
use kadakas_welcome::{Dirs, ENV_SKIP, stores};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The locales of the system, the most wanted first.
pub fn system_locales() -> Vec<String> {
    sys_locale::get_locales().collect()
}

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

/// Runs the dialog in a process of its own and waits until it ends.
pub fn run_dialog_process(kind: Kind) -> std::io::Result<()> {
    std::process::Command::new(std::env::current_exe()?)
        .arg("welcome")
        .arg(kind_arg(kind))
        .stdin(std::process::Stdio::null())
        .status()
        .map(|_| ())
}

/// The step before the start: shows the dialog when it is due. Never when `KADAKAS_SKIP_WELCOME`
/// is set, and never without a display.
pub fn before_start(root: &Path) -> Gate {
    gate::run_gate(
        root,
        std::env::var(ENV_SKIP).ok().as_deref(),
        gate::has_display(std::env::consts::OS, |v| std::env::var_os(v)),
        run_dialog_process,
    )
}

/// True while a dialog that the person asked for is open (one at a time).
static MANUAL_OPEN: AtomicBool = AtomicBool::new(false);

/// Show welcome: opens the dialog with the current settings, without waiting. A second request
/// while the dialog is open does nothing.
pub fn show_again() {
    if MANUAL_OPEN.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        if !gate::skipped(std::env::var(ENV_SKIP).ok().as_deref()) {
            let _ = run_dialog_process(Kind::Manual);
        }
        MANUAL_OPEN.store(false, Ordering::SeqCst);
    });
}

struct WelcomeApp {
    state: WelcomeState,
    dirs: Dirs,
    done: bool,
}

impl WelcomeApp {
    fn finish(&mut self, button: Button) {
        if self.done {
            return;
        }
        self.done = true;
        let problems = gate::finish(
            &self.dirs,
            self.state.kind,
            button,
            &self.state.choices,
            &self.state.detected,
        );
        if !problems.is_empty() {
            eprintln!(
                "kadakas-launcher: {}",
                tr!("welcome-failed", problems = problems.join("; "))
            );
        }
    }
}

impl eframe::App for WelcomeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // The dialog speaks the language that is chosen in it, at once.
        kadakas_l10n::set_language(self.state.language_tag());
        // The close button of the window counts as the second button.
        if ctx.input(|i| i.viewport().close_requested()) {
            self.finish(Button::Secondary);
            return;
        }
        if let Some(button) = welcome_ui::show(ui, &mut self.state) {
            self.finish(button);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// Opens the dialog window and returns when it is closed.
pub fn run_window(root: PathBuf, kind: Kind, modeler_installed: bool) -> eframe::Result {
    let dirs = Dirs::from_env(root);
    let system = system_locales();
    let detected = kadakas_welcome::choices::detect(&system, modeler_installed);
    let choices = if kind == Kind::FirstRun {
        detected.clone()
    } else {
        stores::current(&dirs, &system, modeler_installed)
    };
    let state = WelcomeState::new(kind, choices, detected, system, modeler_installed);
    kadakas_l10n::set_language(state.language_tag());
    let title = tr!("welcome-title");
    let app = WelcomeApp {
        state,
        dirs,
        done: false,
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
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_update::inbox::Inbox;
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

    #[test]
    fn the_arguments_of_the_dialog_process_round_trip() {
        for k in [Kind::FirstRun, Kind::Update, Kind::Manual] {
            assert_eq!(kind_from_arg(kind_arg(k)), Some(k));
        }
        assert_eq!(kind_from_arg("--other"), None);
    }

    #[test]
    fn a_first_start_with_a_file_shows_the_dialog_first_and_the_file_opens_after() {
        let root = tmp("queue").join("launcher");
        let inbox = Inbox::new(&root);
        let log: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let start = |skip: Option<&str>, display: bool, files: Vec<OsString>| {
            let gate = gate::run_gate(&root, skip, display, |kind| {
                log.borrow_mut().push(format!("dialog {kind:?}"));
                // The person double clicked the file once more while the dialog was open: the
                // launcher queues it. Then a button is pressed.
                inbox.deliver(vec![OsString::from("/docs/second.kad")]);
                let dirs = Dirs {
                    launcher: root.clone(),
                    prefs: None,
                    feedback: None,
                };
                let c = kadakas_welcome::choices::detect(&[], true);
                gate::finish(&dirs, kind, Button::Secondary, &c, &c);
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
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn the_variable_and_a_missing_display_keep_the_dialog_away_from_a_fresh_install() {
        let root = tmp("never").join("launcher");
        let shown = std::cell::Cell::new(false);
        let show = |_k: Kind| {
            shown.set(true);
            Ok(())
        };
        assert_eq!(gate::run_gate(&root, Some("1"), true, show), Gate::Skipped);
        assert_eq!(gate::run_gate(&root, None, false, show), Gate::NoDisplay);
        assert!(!shown.get());
        assert!(!Path::new(&root).exists(), "nothing is written");
    }
}
