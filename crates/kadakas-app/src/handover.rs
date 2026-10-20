//! macOS: the app inside the bundle `Kadakas Engine.app` is not meant to be started by itself. The
//! launcher (the outer `Kadakas.app`) starts it, and owns the updates, the choice of the version
//! and the `.kad` files. But the Dock offers "Keep in Dock" on the icon of the running app, and
//! that keeps the path of the inner bundle. So an app that finds itself in an `... Engine.app`
//! bundle and was not started by the launcher asks the system to start the launcher
//! (`open -b net.kadakas.Kadakas`, with the files) and ends. The launcher then starts the right
//! version. The decision is a pure function; the caller starts `open`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The bundle identifier of the outer bundle, which has the launcher.
pub const LAUNCHER_BUNDLE_ID: &str = "net.kadakas.Kadakas";

/// True for `.../<Name> Engine.app/Contents/MacOS/<program>`.
fn in_engine_bundle(exe: &Path) -> bool {
    let macos = exe.parent();
    let contents = macos.and_then(Path::parent);
    let bundle = contents.and_then(Path::parent);
    macos.is_some_and(|p| p.file_name().is_some_and(|n| n == "MacOS"))
        && contents.is_some_and(|p| p.file_name().is_some_and(|n| n == "Contents"))
        && bundle.is_some_and(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(" Engine.app"))
        })
}

/// The arguments for `open`, or `None` when the app should run on its own: it was started by the
/// launcher (`via_launcher`), or it is not in an engine bundle (a development build).
pub fn launcher_open_args(
    exe: Option<&Path>,
    via_launcher: bool,
    files: &[PathBuf],
) -> Option<Vec<OsString>> {
    if via_launcher || std::env::var_os("KADAKAS_NO_HANDOVER").is_some() {
        return None;
    }
    if !exe.is_some_and(in_engine_bundle) {
        return None;
    }
    let mut args = vec![OsString::from("-b"), OsString::from(LAUNCHER_BUNDLE_ID)];
    args.extend(files.iter().map(|f| f.clone().into_os_string()));
    Some(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INNER: &str =
        "/Applications/Kadakas.app/Contents/MacOS/Kadakas Engine.app/Contents/MacOS/kadakas-app";

    #[test]
    fn an_engine_started_alone_hands_over_with_its_files() {
        let files = [PathBuf::from("/a/b.kad")];
        let args = launcher_open_args(Some(Path::new(INNER)), false, &files).unwrap();
        assert_eq!(args, ["-b", "net.kadakas.Kadakas", "/a/b.kad"]);
        let none = launcher_open_args(Some(Path::new(INNER)), false, &[]).unwrap();
        assert_eq!(none, ["-b", "net.kadakas.Kadakas"]);
    }

    #[test]
    fn the_launcher_started_app_and_other_builds_run_on_their_own() {
        let exe = Some(Path::new(INNER));
        // Started by the launcher: never hands over (no loop).
        assert!(launcher_open_args(exe, true, &[]).is_none());
        // A build outside an engine bundle: `cargo run`, an old plain install, the outer bundle.
        for other in [
            "/repo/target/debug/kadakas-app",
            "/Applications/Kadakas.app/Contents/MacOS/kadakas-app",
            "/x/Other.app/Contents/MacOS/kadakas-app",
            "/Kadakas Engine.app/kadakas-app",
        ] {
            assert!(
                launcher_open_args(Some(Path::new(other)), false, &[]).is_none(),
                "{other}"
            );
        }
        assert!(launcher_open_args(None, false, &[]).is_none());
    }
}
