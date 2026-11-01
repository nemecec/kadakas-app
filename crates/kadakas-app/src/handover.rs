//! macOS: the app inside the bundle `Kadakas Engine.app` is not meant to be started by itself. The
//! launcher (the outer `Kadakas.app`) starts it, and owns the updates, the choice of the version
//! and the `.kad` files. But the Dock offers "Keep in Dock" on the icon of the running app, and
//! that keeps the path of the inner bundle. So an app that finds itself in an `... Engine.app`
//! bundle and was not started by the launcher asks the system to start the launcher
//! (`open -b net.kadakas.Kadakas`, with the files) and ends. The launcher then starts the right
//! version. The decision is a pure function; the caller starts `open`.

use std::ffi::{OsStr, OsString};
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
///
/// `options` are the options of the app (`--agent-port 0`, `--allow-dir DIR`, see
/// `kadakas_update::options::prepare`). They go after `--args`, so the launcher gets them in its
/// command line and passes them to the app (PLAT-32). When a launcher runs already, `open` only
/// sends the files to it, and the options do not apply: an app runs, and it keeps its options.
pub fn launcher_open_args(
    exe: Option<&Path>,
    via_launcher: bool,
    files: &[PathBuf],
    options: &[OsString],
) -> Option<Vec<OsString>> {
    if via_launcher || std::env::var_os("KADAKAS_NO_HANDOVER").is_some() {
        return None;
    }
    if !exe.is_some_and(in_engine_bundle) {
        return None;
    }
    let mut args = vec![OsString::from("-b"), OsString::from(LAUNCHER_BUNDLE_ID)];
    args.extend(files.iter().map(|f| safe_file(f)));
    if !options.is_empty() {
        args.push(OsString::from("--args"));
        args.extend(options.iter().cloned());
    }
    Some(args)
}

/// A file argument for `open`: a name that starts with `-` would be read as an option of `open`
/// (`-name` or `--env=...`), so it gets `./` in front.
fn safe_file(f: &Path) -> OsString {
    let s = f.as_os_str();
    if s.to_string_lossy().starts_with('-') {
        let mut out = OsString::from("./");
        out.push(s);
        out
    } else {
        s.to_os_string()
    }
}

/// What the app tells the launcher when it hands over (PLAT-32).
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    /// The documents, with absolute paths when the start folder is known.
    pub files: Vec<PathBuf>,
    /// The options for the launcher, behind `--args`. Never `--collab-token`.
    pub options: Vec<OsString>,
    /// The collaboration token. It goes in the environment of `open` and nowhere else: a word on a
    /// command line is visible to other users.
    pub token: Option<String>,
    /// Lines for the person (stderr): folders that are refused.
    pub notes: Vec<String>,
}

/// The variables that the handover reads.
#[derive(Clone, Copy, Default)]
pub struct HandoverEnv<'a> {
    /// `KADAKAS_AGENT_PORT`.
    pub agent_port: Option<&'a str>,
    /// `KADAKAS_ALLOW_DIR`.
    pub allow_dir: Option<&'a OsStr>,
}

/// Decides what the handover passes on. `args` is the whole command line (with the program name),
/// `files` the documents of it, `collab_token` the token that the app parsed from the flag or the
/// variable (the variable leaves the environment early, so this is the only place to read it),
/// `cwd` the folder where the person started the app.
///
/// - The options are the options of the app in `args`, with absolute folders; the two variables
///   become options (`kadakas_update::options::prepare`). A relative folder with the root as
///   `cwd` (or without `cwd`) and the root of a disk are refused with a note.
/// - No start folder is added as an allowed folder.
/// - The token never is an option.
/// - A relative document gets `cwd` in front.
pub fn plan(
    args: &[String],
    files: &[PathBuf],
    env: HandoverEnv<'_>,
    collab_token: Option<&str>,
    cwd: Option<&Path>,
) -> Plan {
    use kadakas_update::options;
    let os_args: Vec<OsString> = args.iter().skip(1).map(OsString::from).collect();
    let (rest, flag_token) = options::split_token(options::options_in(&os_args));
    let prepared = options::prepare(rest, env.agent_port, env.allow_dir, cwd);
    let token = collab_token
        .map(str::to_string)
        .or_else(|| flag_token.and_then(|t| t.into_string().ok()))
        .filter(|t| !t.is_empty());
    let files = files
        .iter()
        .map(|f| match cwd {
            Some(cwd) if f.is_relative() => cwd.join(f),
            _ => f.clone(),
        })
        .collect();
    Plan {
        files,
        options: prepared.options,
        token,
        notes: prepared.notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INNER: &str =
        "/Applications/Kadakas.app/Contents/MacOS/Kadakas Engine.app/Contents/MacOS/kadakas-app";

    #[test]
    fn an_engine_started_alone_hands_over_with_its_files() {
        let files = [PathBuf::from("/a/b.kad")];
        let args = launcher_open_args(Some(Path::new(INNER)), false, &files, &[]).unwrap();
        assert_eq!(args, ["-b", "net.kadakas.Kadakas", "/a/b.kad"]);
        let none = launcher_open_args(Some(Path::new(INNER)), false, &[], &[]).unwrap();
        assert_eq!(none, ["-b", "net.kadakas.Kadakas"]);
    }

    #[test]
    fn the_options_of_the_app_go_after_the_files_behind_args() {
        let files = [PathBuf::from("/a/b.kad")];
        let options: Vec<OsString> = ["--agent-port", "0", "--allow-dir", "/d"]
            .iter()
            .map(OsString::from)
            .collect();
        let args = launcher_open_args(Some(Path::new(INNER)), false, &files, &options).unwrap();
        assert_eq!(
            args,
            [
                "-b",
                "net.kadakas.Kadakas",
                "/a/b.kad",
                "--args",
                "--agent-port",
                "0",
                "--allow-dir",
                "/d"
            ]
        );
        // Without a file: the options still go.
        let args = launcher_open_args(Some(Path::new(INNER)), false, &[], &options).unwrap();
        assert_eq!(args[..3], ["-b", "net.kadakas.Kadakas", "--args"]);
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    #[cfg(unix)]
    fn the_plan_for_the_handover_is_a_table() {
        let home = Some(Path::new("/home/me/plans"));
        let root = Some(Path::new("/"));
        let none = HandoverEnv::default();
        let a = |extra: &[&str]| {
            let mut v = vec!["kadakas-app"];
            v.extend_from_slice(extra);
            words(&v)
        };
        let os = |l: &[&str]| l.iter().map(OsString::from).collect::<Vec<_>>();

        // The case of PLAT-32: flags and a file. They all go on.
        let p = plan(
            &a(&["--agent-port", "0", "--allow-dir", "/d", "plan.kad"]),
            &[PathBuf::from("plan.kad")],
            none,
            None,
            home,
        );
        assert_eq!(p.options, os(&["--agent-port", "0", "--allow-dir", "/d"]));
        assert_eq!(p.files, [PathBuf::from("/home/me/plans/plan.kad")]);
        assert!(p.notes.is_empty() && p.token.is_none());

        // Relative folders get the start folder. The variables become options.
        let env = std::env::join_paths(["sub", "/abs"]).unwrap();
        let p = plan(
            &a(&["--allow-dir=rel"]),
            &[],
            HandoverEnv {
                agent_port: Some("0"),
                allow_dir: Some(&env),
            },
            None,
            home,
        );
        assert_eq!(
            p.options,
            os(&[
                "--allow-dir",
                "/home/me/plans/rel",
                "--agent-port",
                "0",
                "--allow-dir",
                "/home/me/plans/sub",
                "--allow-dir",
                "/abs"
            ])
        );

        // The start folder is the root (the Dock, `open`): relative folders are refused,
        // absolute ones stay, and the root itself is refused. No folder is added.
        let env = std::env::join_paths([".", "/ok"]).unwrap();
        let p = plan(
            &a(&["--agent-port=0", "--allow-dir", ".", "--allow-dir", "/"]),
            &[],
            HandoverEnv {
                agent_port: None,
                allow_dir: Some(&env),
            },
            None,
            root,
        );
        assert_eq!(p.options, os(&["--agent-port=0", "--allow-dir", "/ok"]));
        assert_eq!(p.notes.len(), 3, "{:?}", p.notes);

        // No start folder known: the same.
        let p = plan(&a(&["--allow-dir", "x"]), &[], none, None, None);
        assert!(p.options.is_empty());
        assert_eq!(p.notes.len(), 1);

        // Documents after `--` are documents, not options, and a name with a dash is safe.
        let p = plan(
            &a(&["--agent-port=0", "--", "--allow-dir=/", "-odd.kad"]),
            &[PathBuf::from("--allow-dir=/"), PathBuf::from("-odd.kad")],
            none,
            None,
            home,
        );
        assert_eq!(p.options, os(&["--agent-port=0"]));
        let args = launcher_open_args(Some(Path::new(INNER)), false, &p.files, &p.options).unwrap();
        assert!(
            args.iter().all(|w| !w.to_string_lossy().starts_with('-')
                || ["-b", "--args", "--agent-port=0"].contains(&w.to_string_lossy().as_ref())),
            "{args:?}"
        );

        // The token: from the flag or from the parsed value. Never in the options.
        let p = plan(
            &a(&["--collab-token", "flag-secret", "--agent-port=0"]),
            &[],
            none,
            None,
            home,
        );
        assert_eq!(p.token.as_deref(), Some("flag-secret"));
        assert_eq!(p.options, os(&["--agent-port=0"]));
        let p = plan(
            &a(&["--collab-token=flag-secret"]),
            &[],
            none,
            Some("env-secret"),
            home,
        );
        assert_eq!(p.token.as_deref(), Some("env-secret"));
        let args = launcher_open_args(Some(Path::new(INNER)), false, &p.files, &p.options).unwrap();
        assert!(
            args.iter().all(|w| !w.to_string_lossy().contains("secret")),
            "{args:?}"
        );
    }

    #[test]
    fn a_document_that_starts_with_a_dash_is_not_an_option_of_open() {
        let files = [PathBuf::from("-odd.kad"), PathBuf::from("--env=X=1")];
        let args = launcher_open_args(Some(Path::new(INNER)), false, &files, &[]).unwrap();
        assert_eq!(
            args,
            ["-b", "net.kadakas.Kadakas", "./-odd.kad", "./--env=X=1"]
        );
    }

    #[test]
    fn the_launcher_started_app_and_other_builds_run_on_their_own() {
        let exe = Some(Path::new(INNER));
        // Started by the launcher: never hands over (no loop).
        assert!(launcher_open_args(exe, true, &[], &[]).is_none());
        // A build outside an engine bundle: `cargo run`, an old plain install, the outer bundle.
        for other in [
            "/repo/target/debug/kadakas-app",
            "/Applications/Kadakas.app/Contents/MacOS/kadakas-app",
            "/x/Other.app/Contents/MacOS/kadakas-app",
            "/Kadakas Engine.app/kadakas-app",
        ] {
            assert!(
                launcher_open_args(Some(Path::new(other)), false, &[], &[]).is_none(),
                "{other}"
            );
        }
        assert!(launcher_open_args(None, false, &[], &[]).is_none());
    }
}
