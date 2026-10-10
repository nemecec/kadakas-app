//! The Kadakas launcher (PLAT-06). Native only.
//!
//! All logic is in `kadakas-update`. This program has the window, the public keys and the
//! addresses. It holds no secret: the keys are public keys, and the downloads need no login.

mod model;
mod ui;

use kadakas_update::store::Bundled;
use kadakas_update::{Store, TrustedKeys, Updater, UreqHttp, Version, current_platform};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The public keys that signed manifests must match. One or two lines (two allow key rotation).
const TRUSTED_KEYS: &str = include_str!("../trusted-keys.txt");

/// Where the signed manifests are. CI uploads `manifest-stable.json` and `manifest-beta.json`
/// to the release `update-manifests`. The variable `KADAKAS_UPDATE_URL` replaces this folder
/// address (for tests). A changed address does not weaken the check: the signature still has to match.
const DEFAULT_MANIFEST_BASE: &str =
    "https://github.com/kadakas/kadakas/releases/download/update-manifests";

/// The install folder: `KADAKAS_LAUNCHER_ROOT`, else the per-user state folder.
fn install_root(env_root: Option<String>) -> Option<PathBuf> {
    env_root
        .filter(|r| !r.is_empty())
        .map(PathBuf::from)
        .or_else(|| kadakas_sys::dirs::state_dir().map(|d| d.join("launcher")))
}

/// The version that the installer put next to the launcher, if the app is there.
fn bundled_in(exe_dir: &Path) -> Option<Bundled> {
    let app = format!("kadakas-app{}", std::env::consts::EXE_SUFFIX);
    exe_dir.join(app).is_file().then(|| Bundled {
        version: Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is valid"),
        dir: exe_dir.to_path_buf(),
    })
}

fn build_updater(
    root: PathBuf,
    exe_dir: Option<&Path>,
    base: &str,
    loopback_http: bool,
) -> Result<Updater, String> {
    let keys = TrustedKeys::parse_list(TRUSTED_KEYS).map_err(|e| e.to_string())?;
    let store = Store::new(root, exe_dir.and_then(bundled_in));
    let mut http = UreqHttp::new();
    if loopback_http {
        http = http.allow_loopback_http();
    }
    Ok(Updater::new(
        store,
        Arc::new(http),
        keys,
        base,
        current_platform(),
    ))
}

fn main() -> eframe::Result {
    let root = install_root(std::env::var("KADAKAS_LAUNCHER_ROOT").ok())
        .expect("no per-user folder: set KADAKAS_LAUNCHER_ROOT");
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let base = std::env::var("KADAKAS_UPDATE_URL").unwrap_or_else(|_| DEFAULT_MANIFEST_BASE.into());
    // For local tests of the update flow only: allows http://127.0.0.1 for the manifest folder.
    let loopback = std::env::var("KADAKAS_UPDATE_ALLOW_LOOPBACK_HTTP").is_ok_and(|v| v == "1");
    let updater =
        build_updater(root, exe_dir.as_deref(), &base, loopback).expect("bad trusted-keys.txt");
    let app = ui::LauncherApp::new(model::Model::new(Arc::new(updater)));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 420.0])
            .with_title("Kadakas launcher"),
        ..Default::default()
    };
    eframe::run_native(
        "Kadakas launcher",
        options,
        Box::new(move |_cc| Ok(Box::new(app))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_list_in_the_source_is_valid() {
        let keys = TrustedKeys::parse_list(TRUSTED_KEYS).unwrap();
        assert!((1..=2).contains(&keys.len()));
    }

    #[test]
    fn root_and_bundled_version() {
        assert_eq!(
            install_root(Some("/x/y".into())),
            Some(PathBuf::from("/x/y"))
        );
        assert!(install_root(Some(String::new())) != Some(PathBuf::new()));
        let d = std::env::temp_dir().join(format!("kadakas-launcher-main-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(bundled_in(&d).is_none());
        std::fs::write(
            d.join(format!("kadakas-app{}", std::env::consts::EXE_SUFFIX)),
            b"x",
        )
        .unwrap();
        let b = bundled_in(&d).unwrap();
        assert_eq!(b.version.to_string(), env!("CARGO_PKG_VERSION"));
        let u = build_updater(d.join("data"), Some(&d), "https://example.org/m", false).unwrap();
        assert!(u.store().active().is_some());
        let none = build_updater(d.join("data2"), None, "https://example.org/m", false).unwrap();
        assert!(none.store().active().is_none());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
