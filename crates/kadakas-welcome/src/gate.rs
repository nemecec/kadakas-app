//! When the welcome dialog is due, and what its buttons do.

use crate::choices::Choices;
use crate::stores::{self, Dirs};
use std::ffi::OsString;
use std::time::Duration;

/// The version of the dialog. A later dialog that asks something new has a higher number; the
/// person then sees it once more, with the current settings. The number that the person has
/// seen is kept in `welcome.json` (`kadakas_update::welcome`).
pub const WELCOME_VERSION: u32 = 1;

/// The variable that turns the dialog off (CI, tests, scripts): `KADAKAS_SKIP_WELCOME=1`.
pub const ENV_SKIP: &str = "KADAKAS_SKIP_WELCOME";

/// The variable with a time limit for the dialog window, in seconds (tests). Without it the
/// launcher waits as long as the window is there.
pub const ENV_TIMEOUT: &str = "KADAKAS_WELCOME_TIMEOUT_SECS";

/// How long a second launcher waits for the first one to finish the dialog.
const LOCK_WAIT: Duration = Duration::from_secs(60 * 60);

/// Why the dialog is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The very first start: no trace of an earlier use. The dialog starts with the defaults of
    /// the system. A button must be pressed.
    FirstRun,
    /// The dialog is newer than the one that the person saw, or there are traces of an earlier
    /// use and no mark (an install from before the dialog, a person who removed the mark). It
    /// starts with the current settings; Cancel only marks it as seen.
    Update,
    /// The person asked for it (Show welcome): it starts with the current settings, and Cancel
    /// changes nothing.
    Manual,
}

impl Kind {
    /// True when the launcher must show the dialog before it starts anything.
    pub fn is_due(self) -> bool {
        self != Kind::Manual
    }
}

/// The kind of the dialog for these folders.
///
/// - The mark is at least the version of the dialog: the person asks for it (`Manual`).
/// - A lower mark, or no mark but a trace of an earlier use (the install folder, `prefs.json`,
///   `modeler.json`, the feedback settings): `Update`, with the current values.
/// - No mark and no trace: `FirstRun`.
pub fn kind_for(dirs: &Dirs) -> Kind {
    kind_at(dirs, WELCOME_VERSION)
}

/// Like [`kind_for`], for a dialog with this version number.
pub fn kind_at(dirs: &Dirs, version: u32) -> Kind {
    let done = kadakas_update::welcome::done_version(&dirs.launcher);
    if done >= version {
        Kind::Manual
    } else if done > 0 || stores::has_traces(dirs) {
        Kind::Update
    } else {
        Kind::FirstRun
    }
}

/// True when this system has a display for a window. On Linux that is a display server; macOS
/// and Windows always have a window system for a logged-in user.
pub fn has_display(os: &str, env: impl Fn(&str) -> Option<OsString>) -> bool {
    match os {
        "linux" | "freebsd" | "netbsd" | "openbsd" | "dragonfly" => ["DISPLAY", "WAYLAND_DISPLAY"]
            .iter()
            .any(|v| env(v).is_some_and(|d| !d.is_empty())),
        _ => true,
    }
}

/// True when the value of `KADAKAS_SKIP_WELCOME` turns the dialog off.
pub fn skipped(value: Option<&str>) -> bool {
    value.is_some_and(|v| matches!(v.trim(), "1" | "true" | "yes"))
}

/// The time limit from `KADAKAS_WELCOME_TIMEOUT_SECS`: whole seconds, more than zero.
pub fn timeout(value: Option<&str>) -> Option<Duration> {
    value
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .map(Duration::from_secs)
}

/// What the launcher did before it started the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// The variable is set.
    Skipped,
    /// No display.
    NoDisplay,
    /// Nothing is due.
    NotDue,
    /// The dialog ran and ended in the normal way.
    Shown(Kind),
    /// The dialog could not run or ended with an error. The mark is not written; the start goes
    /// on with the settings as they are.
    Failed,
}

/// Takes the lock of the gate. If another launcher holds it, `on_wait` runs once and the wait
/// begins (up to `wait`).
fn gate_lock(
    dirs: &Dirs,
    wait: Duration,
    on_wait: impl FnOnce(),
) -> Option<kadakas_sys::fsio::FileLock> {
    use kadakas_sys::fsio::{lock_file, lock_file_or_skip};
    let path = dirs.launcher.join("welcome.lock");
    match lock_file(&path, Duration::ZERO) {
        Ok(lock) => Some(lock),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            on_wait();
            lock_file_or_skip(&path, wait).ok().flatten()
        }
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {
            eprintln!("kadakas-launcher: no lock for the welcome dialog on this file system: {e}");
            None
        }
        Err(_) => None,
    }
}

/// The step before the start. `show` runs the dialog and returns when the person is done; an
/// error (the window could not open, the process failed) makes the gate `Failed`.
///
/// The folder holds a lock while the gate runs, so two launchers that start together (a second
/// double click on a file at the first start) show one dialog: the second waits, looks again,
/// and finds nothing due.
pub fn run_gate(
    dirs: &Dirs,
    skip: Option<&str>,
    display: bool,
    show: impl FnOnce(Kind) -> std::io::Result<()>,
) -> Gate {
    if skipped(skip) {
        return Gate::Skipped;
    }
    if !display {
        return Gate::NoDisplay;
    }
    // A lock that cannot be made (a read-only folder, a file system without locks) does not stop
    // the start. Another launcher that shows the dialog holds the others here: say so.
    let _lock = gate_lock(dirs, LOCK_WAIT, || {
        eprintln!(
            "kadakas-launcher: another launcher shows the welcome dialog; waiting until it is done"
        );
    });
    let kind = kind_for(dirs);
    if !kind.is_due() {
        return Gate::NotDue;
    }
    match show(kind) {
        Ok(()) => Gate::Shown(kind),
        Err(e) => {
            eprintln!("kadakas-launcher: the welcome dialog did not finish: {e}");
            Gate::Failed
        }
    }
}

/// True when the problems of [`finish`] include the mark: the dialog would come again at every
/// start, so the launcher exits with an error.
pub fn mark_failed(problems: &[String]) -> bool {
    problems.iter().any(|p| p.starts_with("welcome.json"))
}

/// The two buttons. The window closed with the close button counts as `Secondary`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// Start (Enter): keep what the dialog shows.
    Primary,
    /// Use defaults (Esc), or Cancel when the dialog is shown again.
    Secondary,
}

/// What the dialog does with a button. `shown` are the choices on the screen, `initial` what the
/// dialog showed when it opened, `detected` the defaults of the system. Returns the problems with
/// files that could not be written.
///
/// - Start on a first start: every choice is stored. Start otherwise: the choices that the
///   person changed.
/// - Use defaults on a first start: the defaults for the keys that are missing; no value is
///   replaced.
/// - Cancel after a newer dialog: only the mark. Cancel when shown on request: nothing.
pub fn finish(
    dirs: &Dirs,
    kind: Kind,
    button: Button,
    shown: &Choices,
    initial: &Choices,
    detected: &Choices,
) -> Vec<String> {
    match (kind, button) {
        (Kind::FirstRun, Button::Primary) => stores::apply(dirs, shown, None),
        (_, Button::Primary) => stores::apply(dirs, shown, Some(initial)),
        (Kind::FirstRun, Button::Secondary) => stores::apply_missing(dirs, detected),
        (Kind::Update, Button::Secondary) => stores::mark_done(dirs),
        (Kind::Manual, Button::Secondary) => Vec::new(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::choices::detect;
    use crate::sysloc::SystemLocale;
    use kadakas_update::Settings;
    use kadakas_update::settings::UpdateMode;
    use std::cell::Cell;
    use std::path::PathBuf;

    pub fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kadakas-welcome-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn dirs(name: &str) -> Dirs {
        let base = tmp(name);
        Dirs {
            launcher: base.join("launcher"),
            prefs: Some(base.join("config")),
            feedback: Some(base.join("feedback")),
        }
    }

    fn cleanup(d: &Dirs) {
        let _ = std::fs::remove_dir_all(d.launcher.parent().unwrap());
    }

    fn defaults() -> Choices {
        detect(&SystemLocale::default(), true)
    }

    #[test]
    fn a_fresh_machine_is_a_first_run_and_the_mark_makes_it_once_only() {
        let d = dirs("gate-fresh");
        assert_eq!(kind_for(&d), Kind::FirstRun);
        let c = defaults();
        assert!(finish(&d, Kind::FirstRun, Button::Secondary, &c, &c, &c).is_empty());
        assert_eq!(
            kadakas_update::welcome::done_version(&d.launcher),
            WELCOME_VERSION
        );
        assert_eq!(kind_for(&d), Kind::Manual);
        assert!(!kind_for(&d).is_due());
        // Removing the mark brings the dialog back, with the current values.
        std::fs::remove_file(kadakas_update::welcome::mark_path(&d.launcher)).unwrap();
        assert_eq!(kind_for(&d), Kind::Update, "the files are traces of use");
        cleanup(&d);
    }

    #[test]
    fn settings_files_of_an_earlier_install_are_traces() {
        // A reinstall or a moved launcher folder: the launcher folder is empty, the settings of
        // the person are still there.
        for file in ["prefs.json", "modeler.json"] {
            let d = dirs("gate-trace-prefs");
            std::fs::create_dir_all(d.prefs.as_ref().unwrap()).unwrap();
            std::fs::write(d.prefs.as_ref().unwrap().join(file), "{}").unwrap();
            assert_eq!(kind_for(&d), Kind::Update, "{file}");
            cleanup(&d);
        }
        let d = dirs("gate-trace-feedback");
        std::fs::create_dir_all(d.feedback.as_ref().unwrap()).unwrap();
        std::fs::write(d.feedback.as_ref().unwrap().join("settings.json"), "{}").unwrap();
        assert_eq!(kind_for(&d), Kind::Update);
        cleanup(&d);
    }

    #[test]
    fn a_first_run_over_existing_preferences_never_resets_them() {
        let d = dirs("gate-no-reset");
        let prefs = d.prefs.clone().unwrap();
        std::fs::create_dir_all(&prefs).unwrap();
        std::fs::write(
            prefs.join("prefs.json"),
            r#"{"language":"et","theme":"dark","units":{"length":"ft_in","decimal":"comma"},"navigation":"trackpad"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(d.feedback.as_ref().unwrap()).unwrap();
        std::fs::write(
            d.feedback.as_ref().unwrap().join("settings.json"),
            r#"{"crash_policy":"never"}"#,
        )
        .unwrap();
        // The launcher folder is new, the dialog is an Update dialog with the current values.
        assert_eq!(kind_for(&d), Kind::Update);
        let initial = stores::current(&d, &["en-US".to_string()], true);
        assert_eq!(initial.language.as_deref(), Some("et"));
        // Cancel, and the close button: only the mark.
        let detected = defaults();
        assert!(
            finish(
                &d,
                Kind::Update,
                Button::Secondary,
                &initial,
                &initial,
                &detected
            )
            .is_empty()
        );
        let p: serde_json::Value =
            serde_json::from_slice(&std::fs::read(prefs.join("prefs.json")).unwrap()).unwrap();
        assert_eq!(p["language"], "et");
        assert_eq!(p["theme"], "dark");
        assert_eq!(p["units"]["length"], "ft_in");
        assert_eq!(p["navigation"], "trackpad");
        // Even a real FirstRun "Use defaults" over an existing value replaces nothing.
        assert!(
            finish(
                &d,
                Kind::FirstRun,
                Button::Secondary,
                &detected,
                &detected,
                &detected
            )
            .is_empty()
        );
        let p: serde_json::Value =
            serde_json::from_slice(&std::fs::read(prefs.join("prefs.json")).unwrap()).unwrap();
        assert_eq!(p["language"], "et");
        assert_eq!(p["units"]["length"], "ft_in");
        let hub = kadakas_feedback::Hub::new(Box::new(kadakas_feedback::store::DirStorage::new(
            d.feedback.clone().unwrap(),
        )));
        assert_eq!(hub.crash_policy(), kadakas_feedback::CrashPolicy::Never);
        cleanup(&d);
    }

    #[test]
    fn an_older_dialog_and_the_old_mark_key() {
        let d = dirs("gate-old");
        std::fs::create_dir_all(&d.launcher).unwrap();
        // The first form of the mark, in launcher.json.
        std::fs::write(Settings::path(&d.launcher), r#"{"welcome_done": 1}"#).unwrap();
        assert_eq!(kind_at(&d, 1), Kind::Manual);
        assert_eq!(kind_at(&d, 2), Kind::Update);
        assert!(Kind::Update.is_due() && Kind::FirstRun.is_due());
        // Traces without a mark: an install from before the dialog.
        std::fs::write(Settings::path(&d.launcher), "{}").unwrap();
        assert_eq!(kind_for(&d), Kind::Update);
        cleanup(&d);
    }

    #[test]
    fn the_gate_shows_once_and_never_with_the_variable_or_without_a_display() {
        let d = dirs("gate-run");
        let shown = Cell::new(0);
        let show = |_k: Kind| {
            shown.set(shown.get() + 1);
            Ok(())
        };
        assert_eq!(run_gate(&d, Some("1"), true, show), Gate::Skipped);
        assert_eq!(run_gate(&d, None, false, show), Gate::NoDisplay);
        assert_eq!(shown.get(), 0);
        assert!(!d.launcher.exists(), "nothing is written");
        assert_eq!(run_gate(&d, None, true, show), Gate::Shown(Kind::FirstRun));
        assert_eq!(shown.get(), 1);
        // The dialog stores the mark; the next start shows nothing.
        let c = defaults();
        finish(&d, Kind::FirstRun, Button::Primary, &c, &c, &c);
        assert_eq!(run_gate(&d, None, true, show), Gate::NotDue);
        assert_eq!(shown.get(), 1);
        cleanup(&d);
    }

    #[test]
    fn a_dialog_that_fails_does_not_write_the_mark_and_the_start_goes_on() {
        let d = dirs("gate-fail");
        let failing = run_gate(&d, None, true, |_| Err(std::io::Error::other("no window")));
        assert_eq!(failing, Gate::Failed);
        assert_eq!(kadakas_update::welcome::done_version(&d.launcher), 0);
        // The next start asks again.
        assert_eq!(kind_for(&d), Kind::FirstRun);
        cleanup(&d);
    }

    #[test]
    fn two_launchers_that_start_together_show_one_dialog() {
        let d = dirs("gate-two");
        let shown = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let results: Vec<_> = (0..2)
            .map(|_| {
                let (d, shown) = (d.clone(), shown.clone());
                std::thread::spawn(move || {
                    run_gate(&d, None, true, |k| {
                        shown.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        // The person reads the dialog for a moment, then presses Start.
                        std::thread::sleep(Duration::from_millis(150));
                        let c = defaults();
                        let problems = finish(&d, k, Button::Primary, &c, &c, &c);
                        assert!(problems.is_empty(), "{problems:?}");
                        Ok(())
                    })
                })
            })
            .map(|h| h.join().unwrap())
            .collect();
        assert_eq!(shown.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(
            results.contains(&Gate::Shown(Kind::FirstRun)),
            "{results:?}"
        );
        assert!(results.contains(&Gate::NotDue), "{results:?}");
        cleanup(&d);
    }

    #[test]
    fn a_mark_that_cannot_be_written_is_told() {
        let base = tmp("gate-mark-fails");
        let blocker = base.join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        let d = Dirs {
            launcher: blocker.join("l"),
            prefs: None,
            feedback: None,
        };
        let c = defaults();
        let problems = finish(&d, Kind::FirstRun, Button::Primary, &c, &c, &c);
        assert!(mark_failed(&problems), "{problems:?}");
        // A problem with only a settings file is not a failed mark.
        assert!(!mark_failed(&["prefs.json: no".to_string()]));
        assert!(!mark_failed(&[]));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_launcher_that_waits_for_another_says_so_once() {
        let d = dirs("gate-waits");
        let first = gate_lock(&d, Duration::from_millis(10), || panic!("nobody holds it")).unwrap();
        let said = Cell::new(0);
        // The lock is held: the notice comes once, and the wait ends with no lock.
        let second = gate_lock(&d, Duration::from_millis(80), || said.set(said.get() + 1));
        assert!(second.is_none());
        assert_eq!(said.get(), 1);
        drop(first);
        // Free again: no notice.
        let third = gate_lock(&d, Duration::from_millis(10), || said.set(99));
        assert!(third.is_some());
        assert_eq!(said.get(), 1);
        cleanup(&d);
    }

    #[test]
    fn the_variable_accepts_the_usual_spellings_only() {
        assert!(skipped(Some("1")) && skipped(Some(" true ")) && skipped(Some("yes")));
        assert!(!skipped(Some("0")) && !skipped(Some("")) && !skipped(None));
        assert_eq!(timeout(Some("30")), Some(Duration::from_secs(30)));
        assert_eq!(timeout(Some(" 5 ")), Some(Duration::from_secs(5)));
        for bad in [None, Some("0"), Some("-1"), Some("x"), Some("")] {
            assert_eq!(timeout(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn linux_needs_a_display_server() {
        let none = |_: &str| None;
        let only = |name: &'static str| move |v: &str| (v == name).then(|| OsString::from(":0"));
        assert!(!has_display("linux", none));
        assert!(has_display("linux", only("DISPLAY")));
        assert!(has_display("linux", only("WAYLAND_DISPLAY")));
        assert!(!has_display("linux", |_: &str| Some(OsString::new())));
        assert!(has_display("macos", none));
        assert!(has_display("windows", none));
    }

    #[test]
    fn each_button_does_what_its_kind_says() {
        let d = dirs("gate-buttons");
        let detected = detect(&SystemLocale::from_languages(vec!["en-US".into()]), true);
        let mut shown = detected.clone();
        shown.updates = UpdateMode::NotifyOnly;
        // First run, Use defaults: the detected values, not the ones on the screen.
        finish(
            &d,
            Kind::FirstRun,
            Button::Secondary,
            &shown,
            &detected,
            &detected,
        );
        let s = Settings::load(&d.launcher);
        assert_eq!(s.mode, UpdateMode::AutoUpdate);
        assert_eq!(
            kadakas_update::welcome::done_version(&d.launcher),
            WELCOME_VERSION
        );
        // Start when shown again: the values the person changed.
        finish(
            &d,
            Kind::Manual,
            Button::Primary,
            &shown,
            &detected,
            &detected,
        );
        assert_eq!(Settings::load(&d.launcher).mode, UpdateMode::NotifyOnly);
        // Cancel when shown on request: nothing changes.
        let before = std::fs::read(Settings::path(&d.launcher)).unwrap();
        let stamp = kadakas_update::welcome::changed_stamp(&d.launcher);
        finish(
            &d,
            Kind::Manual,
            Button::Secondary,
            &detected,
            &detected,
            &detected,
        );
        assert_eq!(std::fs::read(Settings::path(&d.launcher)).unwrap(), before);
        assert_eq!(kadakas_update::welcome::changed_stamp(&d.launcher), stamp);
        // A newer dialog, Cancel: only the mark.
        std::fs::remove_file(kadakas_update::welcome::mark_path(&d.launcher)).unwrap();
        finish(
            &d,
            Kind::Update,
            Button::Secondary,
            &detected,
            &detected,
            &detected,
        );
        assert_eq!(Settings::load(&d.launcher).mode, UpdateMode::NotifyOnly);
        assert_eq!(
            kadakas_update::welcome::done_version(&d.launcher),
            WELCOME_VERSION
        );
        cleanup(&d);
    }

    #[test]
    fn start_stores_a_changed_choice_and_moves_the_stamp_for_the_running_programs() {
        let d = dirs("gate-stamp");
        let c = defaults();
        finish(&d, Kind::FirstRun, Button::Primary, &c, &c, &c);
        let first = kadakas_update::welcome::changed_stamp(&d.launcher);
        let mut shown = c.clone();
        shown.theme = crate::choices::Theme::Dark;
        finish(&d, Kind::Manual, Button::Primary, &shown, &c, &c);
        assert_ne!(kadakas_update::welcome::changed_stamp(&d.launcher), first);
        // Start without a change moves nothing.
        let second = kadakas_update::welcome::changed_stamp(&d.launcher);
        finish(&d, Kind::Manual, Button::Primary, &shown, &shown, &c);
        assert_eq!(kadakas_update::welcome::changed_stamp(&d.launcher), second);
        cleanup(&d);
    }
}
