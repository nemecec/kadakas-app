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

mod cli;
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
    "https://github.com/kadakas/kadakas/releases/download/update-manifests";

/// The version that the installer put next to the launcher, if the app is there.
fn bundled_in(exe_dir: &Path) -> Option<Bundled> {
    let app = format!("kadakas-app{}", std::env::consts::EXE_SUFFIX);
    exe_dir.join(app).is_file().then(|| Bundled {
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

fn window(
    root: PathBuf,
    factory: &UpdaterFactory,
    status: Option<String>,
) -> eframe::Result {
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
            match window(root, &factory, None) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("kadakas-launcher: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        cli::Command::Start { files, site } => {
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
        let d = std::env::temp_dir().join(format!("kadakas-launcher-main-{}-{name}", std::process::id()));
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
        assert!(stable.can_check() && !stable.is_local_site());
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
        assert!(text.contains("label local, commit 0123456789abcdef"), "{text}");
        assert!(text.contains(&format!("{}-local.g0123456789", env!("CARGO_PKG_VERSION"))));
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
