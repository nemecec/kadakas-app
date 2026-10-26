//! The handshake with the launcher (PLAT-16, PLAT-20), the part that every UI flavor owes it.
//!
//! The launcher starts the program with two variables: the id of the run and the folder of the
//! launcher. A program that was started that way must
//!
//! - keep the lock of its run for as long as it lives, so the launcher knows it runs;
//! - write the clean-exit mark when it closes in the normal way, as the very last thing. Without
//!   the mark the launcher counts the run as a crash of this version;
//! - leave a crash file when it panics or crashes (`kadakas-crash`), so the next start of
//!   `kadakas-app` can offer the report.
//!
//! Started by hand, without the launcher, the program does none of the first two and keeps crash
//! files under an id of its own, like the app.

use std::path::PathBuf;
use std::sync::OnceLock;

use kadakas_update::handshake::{AppLink, ENV_ROOT, ENV_RUN};

/// The variables of the launcher, taken out of the environment at the start, so that no child
/// process inherits the run id and writes the clean-exit mark for it.
static CAPTURED: OnceLock<Vec<(String, String)>> = OnceLock::new();

/// Reads the two variables of the launcher and removes them from the environment. Call it at the
/// top of `main`, before any thread starts.
pub fn capture_env() {
    let taken = kadakas_sys::process::take_env(&[ENV_RUN, ENV_ROOT]);
    let _ = CAPTURED.set(taken);
}

/// A variable of the launcher: from the captured ones, else from the environment.
pub fn launch_env(name: &str) -> Option<String> {
    match CAPTURED.get() {
        Some(v) => v.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()),
        None => std::env::var(name).ok(),
    }
}

/// Puts the crash capture in place (PLAT-20). Without a launcher the program makes a run id of its
/// own, so a crash is still kept.
pub fn install_crash_capture() {
    let get = &launch_env;
    let Some(root) = kadakas_update::settings::launcher_root(get(ENV_ROOT)) else {
        return;
    };
    let link = AppLink::from_env(&launch_env);
    let run_id = match &link {
        Some(l) => l.run_id().to_string(),
        None => kadakas_crash::queue::standalone_id(std::process::id()),
    };
    let channel = link
        .as_ref()
        .and_then(AppLink::status)
        .map(|s| s.label)
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "standalone".into());
    kadakas_crash::capture::install(kadakas_crash::capture::Config {
        root,
        run_id,
        program: "kadakas-modeler".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        channel,
        os: kadakas_crash::capture::default_os(),
    });
}

/// Tells the crash capture which graphics adapter runs.
pub fn set_gpu(adapter: &str, driver: &str, backend: &str) {
    kadakas_crash::capture::set_gpu(kadakas_crash::Gpu {
        adapter: adapter.into(),
        driver: driver.into(),
        backend: backend.into(),
    });
}

/// How the main app got the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Started {
    /// A new app process was started.
    Spawned,
    /// The app that runs already was asked to open the file (the open request of the launcher).
    Sent,
}

/// Opens `file` in the main Kadakas app (File > Open in the Kadakas App). If a main app runs
/// (it has an alive stamp that names `kadakas-app`, see `handshake::running_program`), the file
/// goes to it as an open request, the same as a double click through the launcher, and no second
/// app starts. Otherwise the program `kadakas-app` that is next to this one in the version folder
/// is started with the file (`kadakas_update::launch::sibling_app_command`). `root` is the folder
/// of the launcher and `own` the id of this run.
pub fn start_app_in(
    root: Option<&std::path::Path>,
    own: &str,
    exe: &std::path::Path,
    file: &std::path::Path,
) -> Result<Started, String> {
    use kadakas_l10n::tr;
    if let Some(root) = root
        && let Some(id) = kadakas_update::handshake::running_program(root, "kadakas-app", own)
        && kadakas_update::handshake::send_open(root, &id, &[file.to_path_buf()]).is_ok()
    {
        return Ok(Started::Sent);
    }
    let Some(mut cmd) =
        kadakas_update::launch::sibling_app_command(exe, &[file.as_os_str().to_owned()])
    else {
        return Err(tr!("modeler-app-missing"));
    };
    let mut child = cmd
        .spawn()
        .map_err(|e| tr!("modeler-app-failed", error = e.to_string()))?;
    // Collect the child when it ends, so that it does not stay a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(Started::Spawned)
}

/// [`start_app_in`] for this program: the launcher folder and the run id come from the
/// environment that the launcher gave (a program that was started by hand uses the usual folder).
pub fn start_app(file: &std::path::Path) -> Result<Started, String> {
    use kadakas_l10n::tr;
    let exe =
        std::env::current_exe().map_err(|e| tr!("modeler-app-failed", error = e.to_string()))?;
    let root = kadakas_update::settings::launcher_root(launch_env(ENV_ROOT));
    let own = launch_env(ENV_RUN).unwrap_or_default();
    start_app_in(root.as_deref(), &own, &exe, file)
}

/// How often the requests to open documents are looked at, in seconds.
const OPEN_POLL_SECONDS: f64 = 0.5;

/// The link to the launcher, if the launcher started this run.
pub struct Launch {
    link: Option<AppLink>,
    /// The lock of the run. It is held until the program ends.
    _lock: Option<std::fs::File>,
    /// The alive stamp: a timer thread tells the launcher that this program lives, also while the
    /// window loop waits (a file dialog is open, a big file loads). Dropped with the program.
    _heartbeat: Option<kadakas_update::handshake::Heartbeat>,
    next_open_poll: f64,
    /// The stamp of the welcome dialog that this run has seen (`kadakas_update::welcome`).
    welcome_seen: String,
    next_welcome_poll: f64,
}

impl Launch {
    /// The link from the captured variables.
    pub fn from_env() -> Launch {
        Launch::new(AppLink::from_env(&launch_env))
    }

    pub fn new(link: Option<AppLink>) -> Launch {
        Launch {
            _lock: link.as_ref().and_then(AppLink::hold_run_lock),
            _heartbeat: link.as_ref().map(AppLink::start_heartbeat),
            welcome_seen: link
                .as_ref()
                .map(|l| kadakas_update::welcome::changed_stamp(l.root()))
                .unwrap_or_default(),
            link,
            next_open_poll: 0.0,
            next_welcome_poll: 0.0,
        }
    }

    /// True once when the welcome dialog stored choices since this run last looked. Looks twice a
    /// second at most.
    pub fn take_settings_changed(&mut self, now: f64) -> bool {
        let Some(link) = &self.link else {
            return false;
        };
        if now < self.next_welcome_poll {
            return false;
        }
        self.next_welcome_poll = now + OPEN_POLL_SECONDS;
        let stamp = kadakas_update::welcome::changed_stamp(link.root());
        if stamp == self.welcome_seen {
            return false;
        }
        self.welcome_seen = stamp;
        true
    }

    /// The documents that the launcher sent (a double click on a `.kad` file while this program
    /// runs). Looks at most twice a second. The request files are deleted when they are read, so
    /// the launcher sees at once that this program has the request.
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

    /// Asks the launcher to show its welcome dialog again (Help > Show welcome). False when the
    /// launcher did not start this run, or the request could not be written.
    pub fn request_welcome(&self) -> bool {
        self.link
            .as_ref()
            .is_some_and(|l| l.request_welcome().is_ok())
    }

    /// True when the launcher started this run.
    pub fn is_launched(&self) -> bool {
        self.link.is_some()
    }

    /// The program ends in the normal way: the crash file of this run goes, and the launcher gets
    /// the clean-exit mark. Call it as the very last thing.
    pub fn finish(&self) {
        kadakas_crash::capture::clean_exit();
        if let Some(link) = &self.link {
            let _ = link.mark_clean_exit();
        }
    }
}

/// The files named on the command line: the paths that are not options. The launcher passes the
/// files of the person this way.
pub fn files_from_args(args: &[String]) -> Vec<PathBuf> {
    args.iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .map(PathBuf::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(dir: &std::path::Path, id: &str) -> impl Fn(&str) -> Option<String> {
        let (dir, id) = (dir.display().to_string(), id.to_string());
        move |k| match k {
            ENV_ROOT => Some(dir.clone()),
            ENV_RUN => Some(id.clone()),
            _ => None,
        }
    }

    fn run_file(root: &std::path::Path, id: &str, name: &str) -> PathBuf {
        kadakas_update::runs::run_file(root, id, name)
    }

    #[test]
    fn a_run_that_the_launcher_started_ends_with_the_clean_exit_mark() {
        let d = std::env::temp_dir().join(format!("kadakas-modeler-launch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let link = AppLink::from_env(&env_of(&d, "1a-2b")).expect("a run id and a folder");
        let launch = Launch::new(Some(link));
        assert!(launch.is_launched());
        assert!(
            !run_file(&d, "1a-2b", "clean").exists(),
            "not before the end"
        );
        // The lock of the run is held: a second holder is refused.
        let second = AppLink::from_env(&env_of(&d, "1a-2b")).unwrap();
        assert!(second.hold_run_lock().is_none());
        launch.finish();
        assert!(run_file(&d, "1a-2b", "clean").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "kadakas-modeler-start-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_running_main_app_gets_the_file_as_an_open_request_and_no_second_app_starts() {
        let root = temp("sent");
        let runs = kadakas_update::runs::runs_dir(&root);
        std::fs::create_dir_all(&runs).unwrap();
        // A main app runs: its name and a fresh alive stamp.
        std::fs::write(run_file(&root, "1a-1", "program"), "kadakas-app").unwrap();
        std::fs::write(run_file(&root, "1a-1", "alive"), "1").unwrap();
        // A second modeler runs too: it is not the main app.
        std::fs::write(run_file(&root, "3c-3", "program"), "kadakas-modeler").unwrap();
        std::fs::write(run_file(&root, "3c-3", "alive"), "1").unwrap();
        let file = root.join("site.kad");
        let how = start_app_in(
            Some(&root),
            "2b-2",
            &root.join("bin").join("kadakas-modeler"),
            &file,
        );
        assert_eq!(how, Ok(Started::Sent));
        assert_eq!(
            kadakas_update::handshake::take_open(&root, "1a-1"),
            vec![file.clone()]
        );
        assert!(kadakas_update::handshake::take_open(&root, "3c-3").is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn without_a_running_main_app_the_one_next_to_the_modeler_starts_with_the_file() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp("spawn");
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let app = bin.join("kadakas-app");
        std::fs::write(&app, "#!/bin/sh\necho \"$2\" > started.txt\n").unwrap();
        std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o755)).unwrap();
        let file = root.join("site.kad");
        let exe = bin.join("kadakas-modeler");
        let how = start_app_in(Some(&root), "2b-2", &exe, &file);
        assert_eq!(how, Ok(Started::Spawned));
        // The script ends by itself; wait for its note.
        let note = bin.join("started.txt");
        for _ in 0..250 {
            if note.is_file() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(
            std::fs::read_to_string(&note).unwrap().trim(),
            file.display().to_string()
        );
        // With no app next to the modeler, there is nothing to start.
        let lone = root.join("lone");
        std::fs::create_dir_all(&lone).unwrap();
        let err = start_app_in(None, "", &lone.join("kadakas-modeler"), &file).unwrap_err();
        assert_eq!(err, kadakas_l10n::tr!("modeler-app-missing"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_program_started_by_hand_has_no_link_and_finishes_quietly() {
        let launch = Launch::new(None);
        assert!(!launch.is_launched());
        launch.finish();
    }

    #[test]
    fn the_files_are_the_arguments_that_are_not_options() {
        let args: Vec<String> = ["kadakas-modeler", "a.kad", "--flag", "dir/b.kad"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            files_from_args(&args),
            vec![PathBuf::from("a.kad"), PathBuf::from("dir/b.kad")]
        );
        assert!(files_from_args(&args[..1]).is_empty());
    }
}
