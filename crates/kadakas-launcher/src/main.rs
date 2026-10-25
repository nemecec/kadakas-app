//! The Kadakas launcher (PLAT-06, PLAT-16, PLAT-17). Native only.
//!
//! It is the entry point of Kadakas: shortcuts, the Dock or taskbar icon and `.kad` files start
//! it. It starts the chosen version at once and checks for updates in the background (no
//! window). `kadakas-launcher manage` opens the window with the versions, and
//! `kadakas-launcher add PATH` puts a local build into the store.
//!
//! All logic is in `kadakas-update`. This program has the window, the command line, the public
//! keys and the addresses. It holds no secret: the keys are public keys, and the downloads need
//! no login.

// A release build on Windows has no console window, so the headless launcher leaves none open.
// (The output of `add` and `--help` is not shown there; `manage` shows everything in the window.)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
mod appkit;
mod cli;
mod events;
mod model;
mod ui;

use kadakas_l10n::tr;
use kadakas_update::local::{self, RoutingHttp};
use kadakas_update::store::Bundled;
use kadakas_update::supervisor::{SystemSpawner, UpdaterFactory};
use kadakas_update::{
    Settings, Store, TrustedKeys, Updater, UreqHttp, Version, current_platform, settings,
};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

/// The public keys that signed manifests must match. One or two lines (two allow key rotation).
const TRUSTED_KEYS: &str = include_str!("../trusted-keys.txt");

/// Where the signed manifests are. CI uploads `manifest-stable.json` and `manifest-beta.json`
/// to the release `update-manifests`. The variable `KADAKAS_UPDATE_URL` replaces this folder
/// address (for tests). A changed address does not weaken the check: the signature still has to match.
const DEFAULT_MANIFEST_BASE: &str =
    "https://github.com/nemecec/kadakas-app/releases/download/update-manifests";

/// The version that the installer put next to the launcher, if the app is there. On Windows and
/// Linux the program `kadakas-app` is next to the launcher. On macOS the launcher is the main
/// executable of the outer bundle, and the packager puts the app bundle next to it
/// (`Kadakas.app/Contents/MacOS/Kadakas Engine.app`, `embedded-apps` in `packager-macos.toml`).
fn bundled_in(exe_dir: &Path) -> Option<Bundled> {
    kadakas_update::flavors::has_app(exe_dir).then(|| Bundled {
        version: Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is valid"),
        dir: exe_dir.to_path_buf(),
    })
}

/// How to reach the sites.
#[derive(Clone, Default)]
struct Sites {
    /// `--site`: a local folder for this run.
    cli_site: Option<String>,
    /// `KADAKAS_UPDATE_URL`.
    env_url: Option<String>,
    /// For local tests of the update flow only: allows http://127.0.0.1 for the manifest folder.
    loopback_http: bool,
}

/// Makes the updater for the settings that count now. The channel and the site of the settings
/// decide the address of the manifests (`local::resolve_base`).
fn updater_factory(root: PathBuf, exe_dir: Option<PathBuf>, sites: Sites) -> UpdaterFactory {
    Arc::new(move |s: &Settings| {
        let keys = TrustedKeys::parse_list(TRUSTED_KEYS).expect("bad trusted-keys.txt");
        let store = Store::new(root.clone(), exe_dir.as_deref().and_then(bundled_in))
            .with_keep_older(s.keep_older);
        let mut net = UreqHttp::new();
        if sites.loopback_http {
            net = net.allow_loopback_http();
        }
        let base = local::resolve_base(
            sites.cli_site.as_deref(),
            sites.env_url.as_deref(),
            s,
            DEFAULT_MANIFEST_BASE,
        )
        .unwrap_or_default();
        Updater::new(
            store,
            Arc::new(RoutingHttp::new(Arc::new(net))),
            keys,
            base,
            current_platform(),
        )
    })
}

fn sites_from_env(cli_site: Option<String>) -> Sites {
    Sites {
        cli_site,
        env_url: std::env::var("KADAKAS_UPDATE_URL").ok(),
        loopback_http: std::env::var("KADAKAS_UPDATE_ALLOW_LOOPBACK_HTTP").is_ok_and(|v| v == "1"),
    }
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
}

/// The language of the window: the language of the system, if we have it.
fn init_language() {
    let system: Vec<String> = sys_locale::get_locales().collect();
    kadakas_l10n::set_language(kadakas_l10n::negotiate(&system));
}

/// `kadakas-launcher add PATH`: puts a build into the store. Prints one line.
fn run_add(
    store: &Store,
    path: &Path,
    version: Option<Version>,
    out: &mut dyn std::io::Write,
    git: &dyn Fn(&Path) -> Option<local::GitInfo>,
) -> Result<(), String> {
    let base = version
        .unwrap_or_else(|| Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version"));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let added = local::add(store, path, &base, now, git).map_err(|e| e.to_string())?;
    writeln!(
        out,
        "Added version {} (label local{}). Use the channel local in Preferences > Updates, or \
         `kadakas-launcher manage` to start it.",
        added.version,
        added
            .commit
            .map(|c| format!(", commit {c}"))
            .unwrap_or_default()
    )
    .map_err(|e| e.to_string())
}

fn window(root: PathBuf, factory: &UpdaterFactory, status: Option<String>) -> eframe::Result {
    let updater = factory(&Settings::load(&root));
    let starter: model::Starter = Arc::new(|| {
        let exe = std::env::current_exe()?;
        std::process::Command::new(exe)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
    });
    let mut app = ui::LauncherApp::new(model::Model::new(Arc::new(updater), starter));
    if let Some(s) = status {
        app = app.with_status(s);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([620.0, 520.0])
            .with_title(tr!("launcher-title")),
        ..Default::default()
    };
    eframe::run_native(
        "Kadakas launcher",
        options,
        Box::new(move |_cc| Ok(Box::new(app))),
    )
}

/// macOS, from inside the app bundle: the launcher is the program of the outer `Kadakas.app`. It
/// runs the system event loop on the main thread, so that the open-documents Apple event of a
/// double click arrives (also while the app runs), and the supervisor on a worker thread. The app
/// starts as soon as the loop is up (a few tens of milliseconds), with the files of the event.
#[cfg(target_os = "macos")]
fn start_in_bundle(
    root: PathBuf,
    exe: PathBuf,
    files: Vec<std::ffi::OsString>,
    site: Option<String>,
) -> ExitCode {
    use kadakas_update::inbox::Inbox;
    use kadakas_update::supervisor::{Spawner, Supervisor};
    let factory = updater_factory(root.clone(), exe_dir(), sites_from_env(site));
    let inbox = Inbox::new(&root);
    let spawner: Arc<dyn Spawner> = Arc::new(SystemSpawner);
    let system = Arc::new(appkit::AppKitEvents);
    let forwarder = events::Forwarder::new(inbox.clone(), system.clone(), spawner.clone());
    let supervisor = Supervisor::new(root, factory, spawner).with_inbox(inbox);
    let ok = events::run_with_events(
        system,
        forwarder,
        files,
        std::time::Duration::from_secs(2),
        move |files| match supervisor.run(files) {
            Ok(_) => true,
            Err(e) => {
                // Nothing could start. The window needs the main thread, which runs the event
                // loop, so it opens as a new process, with the reason.
                let text = tr!("launcher-status-start-failed", error = e.to_string());
                eprintln!("kadakas-launcher: {text}");
                let _ = std::process::Command::new(exe)
                    .arg("manage")
                    .env("KADAKAS_LAUNCHER_STATUS", text)
                    .spawn();
                false
            }
        },
    );
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn main() -> ExitCode {
    init_language();
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let command = match cli::parse(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("kadakas-launcher: {e}\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    if command == cli::Command::Help {
        println!("{}", cli::USAGE);
        return ExitCode::SUCCESS;
    }
    let Some(root) = settings::launcher_root(std::env::var("KADAKAS_LAUNCHER_ROOT").ok()) else {
        eprintln!("kadakas-launcher: no per-user folder: set KADAKAS_LAUNCHER_ROOT");
        return ExitCode::FAILURE;
    };
    match command {
        cli::Command::Help => ExitCode::SUCCESS,
        cli::Command::Add { path, version } => {
            let store = Store::new(root, exe_dir().as_deref().and_then(bundled_in));
            match run_add(
                &store,
                &path,
                version,
                &mut std::io::stdout(),
                &local::git_info,
            ) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("kadakas-launcher: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        cli::Command::Manage => {
            let factory = updater_factory(root.clone(), exe_dir(), sites_from_env(None));
            // A launcher that could not start anything opens this window as a new process (macOS,
            // where the first process runs the system event loop) and says why in the variable.
            let status = std::env::var("KADAKAS_LAUNCHER_STATUS")
                .ok()
                .filter(|s| !s.is_empty());
            match window(root, &factory, status) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("kadakas-launcher: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        cli::Command::Start { files, site } => {
            #[cfg(target_os = "macos")]
            if let Some(exe) = std::env::current_exe()
                .ok()
                .filter(|e| events::in_app_bundle(e))
            {
                return start_in_bundle(root, exe, files, site);
            }
            let factory = updater_factory(root.clone(), exe_dir(), sites_from_env(site));
            let supervisor = kadakas_update::supervisor::Supervisor::new(
                root.clone(),
                factory.clone(),
                Arc::new(SystemSpawner),
            );
            match supervisor.run(files) {
                Ok(_) => ExitCode::SUCCESS,
                Err(e) => {
                    // Nothing could start. Show the window with the reason, so the person can
                    // check for updates or choose another version.
                    let text = tr!("launcher-status-start-failed", error = e.to_string());
                    eprintln!("kadakas-launcher: {text}");
                    match window(root, &factory, Some(text)) {
                        Ok(()) => ExitCode::SUCCESS,
                        Err(_) => ExitCode::FAILURE,
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_update::Channel;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "kadakas-launcher-main-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_key_list_in_the_source_is_valid() {
        let keys = TrustedKeys::parse_list(TRUSTED_KEYS).unwrap();
        assert!((1..=2).contains(&keys.len()));
    }

    #[test]
    fn bundled_version_and_the_updater_for_each_channel() {
        let d = tmp("factory");
        assert!(bundled_in(&d).is_none());
        std::fs::write(
            d.join(format!("kadakas-app{}", std::env::consts::EXE_SUFFIX)),
            b"x",
        )
        .unwrap();
        let b = bundled_in(&d).unwrap();
        assert_eq!(b.version.to_string(), env!("CARGO_PKG_VERSION"));

        let factory = updater_factory(d.join("data"), Some(d.clone()), Sites::default());
        let stable = factory(&Settings::default());
        assert!(stable.store().active().is_some());
        assert!(stable.can_check());
        assert!(!stable.is_local_site());
        // The channel local has no default site: nothing to ask. With a site: a local folder.
        let local = Settings {
            channel: Channel::Local,
            ..Settings::default()
        };
        assert!(!factory(&local).can_check());
        let with_site = Settings {
            site: "file:///tmp/site".into(),
            ..local
        };
        assert!(factory(&with_site).is_local_site());
        // The option --site is a local folder for any channel.
        let sites = Sites {
            cli_site: Some("file:///tmp/other".into()),
            ..Sites::default()
        };
        assert!(updater_factory(d.join("data"), None, sites)(&Settings::default()).is_local_site());
        // No app next to the launcher: no bundled version.
        let none = updater_factory(d.join("data2"), None, Sites::default());
        assert!(none(&Settings::default()).store().active().is_none());
        // The keep number of the settings reaches the store.
        let keep = Settings {
            keep_older: 4,
            ..Settings::default()
        };
        assert_eq!(factory(&keep).store().keep_older(), 4);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_bundled_app_of_the_outer_macos_bundle_is_found() {
        // Kadakas.app/Contents/MacOS/kadakas-launcher is the launcher. The base version is the
        // app bundle next to it, in the same folder.
        let d = tmp("outer");
        let macos = d.join("Kadakas.app/Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        assert!(bundled_in(&macos).is_none());
        let inner = macos.join("Kadakas Engine.app/Contents/MacOS");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("kadakas-app"), b"x").unwrap();
        let b = bundled_in(&macos).unwrap();
        assert_eq!(b.dir, macos);
        assert!(events::in_app_bundle(&macos.join("kadakas-launcher")));
        // The store starts the inner bundle's executable.
        let store = Store::new(d.join("data"), Some(b));
        let active = store.active().unwrap();
        let flavors = kadakas_update::flavors::discover(&active.dir);
        assert_eq!(active.dir.join(&flavors[0].exe), inner.join("kadakas-app"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn add_prints_one_line_and_reports_errors() {
        let d = tmp("add");
        let build = d.join("release");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::write(
            build.join(format!("kadakas-app{}", std::env::consts::EXE_SUFFIX)),
            b"x",
        )
        .unwrap();
        let store = Store::new(d.join("data"), None);
        let git = |_: &Path| {
            Some(local::GitInfo {
                commit: "0123456789abcdef".into(),
                dirty: false,
            })
        };
        let mut out = Vec::new();
        run_add(&store, &build, None, &mut out, &git).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("label local, commit 0123456789abcdef"),
            "{text}"
        );
        // A local build takes the numbers of the base version, without its pre-release part
        // (0.1.0-beta.1 gives 0.1.0-local.g...).
        let core = env!("CARGO_PKG_VERSION").split('-').next().unwrap();
        assert!(text.contains(&format!("{core}-local.g0123456789")));
        // Another base version, and no git.
        let mut out = Vec::new();
        run_add(
            &store,
            &build,
            Some(Version::parse("0.9.0").unwrap()),
            &mut out,
            &|_| None,
        )
        .unwrap();
        assert!(String::from_utf8(out).unwrap().contains("0.9.0-local.t"));
        // Not a build.
        let e = run_add(&store, &d, None, &mut Vec::new(), &git).unwrap_err();
        assert!(e.contains("not a build"), "{e}");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
