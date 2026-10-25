//! When the welcome dialog is due, and what its buttons do.

use crate::choices::Choices;
use crate::stores::{self, Dirs};
use kadakas_update::Settings;
use std::ffi::OsString;
use std::path::Path;

/// The version of the dialog. A later dialog that asks something new has a higher number; the
/// person then sees it once more, with the current settings. The launcher settings keep the
/// number that the person has seen as `welcome_done`.
pub const WELCOME_VERSION: u32 = 1;

/// The variable that turns the dialog off (CI, tests, scripts): `KADAKAS_SKIP_WELCOME=1`.
pub const ENV_SKIP: &str = "KADAKAS_SKIP_WELCOME";

/// Why the dialog is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The very first start: the dialog starts with the defaults of the system. A button must be
    /// pressed.
    FirstRun,
    /// The dialog is newer than the one that the person saw: it starts with the current settings.
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

/// The folders that a launcher that was used before has. A launcher that finds one of these on
/// the very first run of this feature belongs to someone who already uses Kadakas.
const INSTALL_TRACES: [&str; 4] = ["launcher.json", "versions", "current", "runs"];

fn has_install_traces(root: &Path) -> bool {
    INSTALL_TRACES.iter().any(|n| root.join(n).exists())
}

/// The kind of the dialog for this install folder.
///
/// - Nothing is stored, and no trace of an earlier use: the first run.
/// - A number lower than the version of the dialog, or none but traces of an earlier use (an
///   install from before the dialog, or a person who deleted `welcome_done`): an update. The
///   dialog starts with the current settings, so nothing changes unless the person changes it.
/// - Up to date: the person asks for it.
pub fn kind_for(root: &Path) -> Kind {
    kind_at(root, WELCOME_VERSION)
}

/// Like [`kind_for`], for a dialog with this version number.
pub fn kind_at(root: &Path, version: u32) -> Kind {
    let done = Settings::load(root).welcome_done;
    if done >= version {
        Kind::Manual
    } else if done > 0 || has_install_traces(root) {
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

/// What the launcher does before it starts the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// The variable is set.
    Skipped,
    /// No display.
    NoDisplay,
    /// Nothing is due.
    NotDue,
    /// The dialog ran (it may have been closed without a choice if it could not open).
    Shown(Kind),
    /// The dialog could not be started.
    Failed,
}

/// The step before the start. `show` runs the dialog and returns when the person is done. It is
/// called when the dialog is due, never when `skip` says so or there is no display.
pub fn run_gate(
    root: &Path,
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
    let kind = kind_for(root);
    if !kind.is_due() {
        return Gate::NotDue;
    }
    match show(kind) {
        Ok(()) => Gate::Shown(kind),
        Err(_) => Gate::Failed,
    }
}

/// The two buttons. The window closed with the close button counts as `Secondary`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// Start (Enter): keep what the dialog shows.
    Primary,
    /// Use defaults (Esc), or Cancel when the dialog is shown again.
    Secondary,
}

/// What the dialog does with a button. `shown` are the choices on the screen, `detected` the
/// defaults of the system. Returns the problems with files that could not be written.
pub fn finish(
    dirs: &Dirs,
    kind: Kind,
    button: Button,
    shown: &Choices,
    detected: &Choices,
) -> Vec<String> {
    match (kind, button) {
        (_, Button::Primary) => stores::apply(dirs, shown),
        (Kind::FirstRun, Button::Secondary) => stores::apply(dirs, detected),
        (Kind::Update, Button::Secondary) => stores::mark_done(dirs),
        (Kind::Manual, Button::Secondary) => Vec::new(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::choices::detect;
    use std::cell::Cell;
    use std::path::PathBuf;

    pub fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kadakas-welcome-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_fresh_folder_is_a_first_run_and_the_mark_makes_it_once_only() {
        let root = tmp("gate-fresh").join("launcher");
        assert_eq!(kind_for(&root), Kind::FirstRun);
        let dirs = Dirs {
            launcher: root.clone(),
            prefs: None,
            feedback: None,
        };
        let c = detect(&[], true);
        assert!(finish(&dirs, Kind::FirstRun, Button::Secondary, &c, &c).is_empty());
        assert_eq!(Settings::load(&root).welcome_done, WELCOME_VERSION);
        assert_eq!(kind_for(&root), Kind::Manual);
        assert!(!kind_for(&root).is_due());
        // Deleting the mark brings the dialog back; the other settings stay.
        Settings::update(&root, |s| s.welcome_done = 0).unwrap();
        assert_eq!(
            kind_for(&root),
            Kind::Update,
            "launcher.json is a trace of use"
        );
        std::fs::remove_file(Settings::path(&root)).unwrap();
        assert_eq!(kind_for(&root), Kind::FirstRun);
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn an_install_that_predates_the_dialog_sees_it_once_with_its_current_settings() {
        let root = tmp("gate-old");
        std::fs::create_dir_all(root.join("versions")).unwrap();
        assert_eq!(kind_for(&root), Kind::Update);
        let root2 = tmp("gate-old2");
        std::fs::write(Settings::path(&root2), r#"{"welcome_done": 0}"#).unwrap();
        assert_eq!(kind_for(&root2), Kind::Update);
        // The person saw dialog 1; dialog 2 is due, with the current settings.
        std::fs::write(Settings::path(&root2), r#"{"welcome_done": 1}"#).unwrap();
        assert_eq!(kind_at(&root2, 1), Kind::Manual);
        assert_eq!(kind_at(&root2, 2), Kind::Update);
        assert!(Kind::Update.is_due() && Kind::FirstRun.is_due());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root2);
    }

    #[test]
    fn the_gate_shows_once_and_never_with_the_variable_or_without_a_display() {
        let root = tmp("gate-run").join("l");
        let shown = Cell::new(0);
        let show = |_k: Kind| {
            shown.set(shown.get() + 1);
            Ok(())
        };
        assert_eq!(run_gate(&root, Some("1"), true, show), Gate::Skipped);
        assert_eq!(run_gate(&root, None, false, show), Gate::NoDisplay);
        assert_eq!(shown.get(), 0);
        assert_eq!(
            run_gate(&root, None, true, show),
            Gate::Shown(Kind::FirstRun)
        );
        assert_eq!(shown.get(), 1);
        // The dialog stores the mark; the next start shows nothing.
        let dirs = Dirs {
            launcher: root.clone(),
            prefs: None,
            feedback: None,
        };
        let c = detect(&[], true);
        finish(&dirs, Kind::FirstRun, Button::Primary, &c, &c);
        assert_eq!(run_gate(&root, None, true, show), Gate::NotDue);
        assert_eq!(shown.get(), 1);
        let failing = run_gate(&tmp("gate-fail").join("l"), None, true, |_| {
            Err(std::io::Error::other("no window"))
        });
        assert_eq!(failing, Gate::Failed);
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    #[test]
    fn the_variable_accepts_the_usual_spellings_only() {
        assert!(skipped(Some("1")) && skipped(Some(" true ")) && skipped(Some("yes")));
        assert!(!skipped(Some("0")) && !skipped(Some("")) && !skipped(None));
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
        let root = tmp("gate-buttons");
        let dirs = Dirs {
            launcher: root.clone(),
            prefs: None,
            feedback: None,
        };
        let detected = detect(&["en-US".to_string()], true);
        let mut shown = detected.clone();
        shown.updates = kadakas_update::settings::UpdateMode::NotifyOnly;
        // First run, Use defaults: the detected values, not the ones on the screen.
        finish(&dirs, Kind::FirstRun, Button::Secondary, &shown, &detected);
        let s = Settings::load(&root);
        assert_eq!(s.mode, kadakas_update::settings::UpdateMode::AutoUpdate);
        assert_eq!(s.welcome_done, WELCOME_VERSION);
        // Start: the values on the screen.
        finish(&dirs, Kind::Manual, Button::Primary, &shown, &detected);
        assert_eq!(
            Settings::load(&root).mode,
            kadakas_update::settings::UpdateMode::NotifyOnly
        );
        // Cancel when shown again: nothing changes.
        let before = std::fs::read(Settings::path(&root)).unwrap();
        finish(&dirs, Kind::Manual, Button::Secondary, &detected, &detected);
        assert_eq!(std::fs::read(Settings::path(&root)).unwrap(), before);
        // A newer dialog, Cancel: only the mark moves.
        Settings::update(&root, |s| s.welcome_done = 0).unwrap();
        finish(&dirs, Kind::Update, Button::Secondary, &detected, &detected);
        let s = Settings::load(&root);
        assert_eq!(s.mode, kadakas_update::settings::UpdateMode::NotifyOnly);
        assert_eq!(s.welcome_done, WELCOME_VERSION);
        let _ = std::fs::remove_dir_all(&root);
    }
}
