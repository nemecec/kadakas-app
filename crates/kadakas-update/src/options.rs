//! The options of the programs that the launcher starts (PLAT-32).
//!
//! A person can start Kadakas with options: `--agent-port 0 --allow-dir DIR`. The launcher and
//! the macOS handover (the app that finds itself in `Kadakas Engine.app` and asks the launcher to
//! start) must pass these options on to the app, or the app starts without them. This file is the
//! one list of the options and the pure functions that handle them. Nothing here starts a program.
//!
//! The app starts in the folder of its version, not in the folder where the person started it. So
//! the folders are made absolute here, and the start folder is named when the person turned on
//! the agent server and named no folder.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The options of the app that take a value (`--name VALUE` or `--name=VALUE`).
pub const WITH_VALUE: [&str; 7] = [
    "--agent-port",
    "--allow-dir",
    "--allow-url",
    "--allow-collab-url",
    "--collab-url",
    "--collab-token",
    "--profile",
];

/// The options of the app that stand alone.
pub const SWITCHES: [&str; 2] = ["--allow-converters", "--allow-agent-scripts"];

/// The variable with the folders that agents may use (`--allow-dir` as a list, separated like the
/// `PATH` of the system).
pub const ENV_ALLOW_DIR: &str = "KADAKAS_ALLOW_DIR";

/// The variable with the port of the agent server (`--agent-port`).
pub const ENV_AGENT_PORT: &str = "KADAKAS_AGENT_PORT";

/// If `arg` is an option of the app, the words that make it (`--name` and its value, or the one
/// word `--name=VALUE`), with the value taken from `rest`. `Ok(None)`: `arg` is no option of the
/// app. `Err`: the option needs a value and there is none.
pub fn take<'a>(
    arg: &OsStr,
    rest: &mut impl Iterator<Item = &'a OsString>,
) -> Result<Option<Vec<OsString>>, String> {
    let Some(text) = arg.to_str() else {
        return Ok(None);
    };
    if SWITCHES.contains(&text) {
        return Ok(Some(vec![arg.to_os_string()]));
    }
    let name = text.split_once('=').map_or(text, |(name, _)| name);
    if !WITH_VALUE.contains(&name) {
        return Ok(None);
    }
    if name.len() != text.len() {
        return Ok(Some(vec![arg.to_os_string()]));
    }
    match rest.next() {
        Some(value) => Ok(Some(vec![arg.to_os_string(), value.clone()])),
        None => Err(format!("{text} needs a value")),
    }
}

/// The options of the app in `args` (without the program name), in order. The documents and
/// everything after `--` are left out. A last option without a value is left out.
pub fn options_in(args: &[OsString]) -> Vec<OsString> {
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--" {
            break;
        }
        match take(a, &mut it) {
            Ok(Some(words)) => out.extend(words),
            Ok(None) => {}
            Err(_) => break,
        }
    }
    out
}

/// `dir` made absolute against `cwd`. An empty value stays empty (it names nothing).
fn absolute(dir: &Path, cwd: &Path) -> PathBuf {
    if dir.as_os_str().is_empty() || dir.is_absolute() {
        dir.to_path_buf()
    } else {
        cwd.join(dir)
    }
}

/// The folders of `--allow-dir` in `options`, in order.
pub fn allow_dirs_in(options: &[OsString]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut it = options.iter();
    while let Some(a) = it.next() {
        if a == "--allow-dir" {
            out.extend(it.next().map(PathBuf::from));
        } else if let Some(v) = a.to_str().and_then(|t| t.strip_prefix("--allow-dir=")) {
            out.push(PathBuf::from(v));
        } else if WITH_VALUE.iter().any(|w| a == w) {
            it.next();
        }
    }
    out
}

/// True when `options` turn on the agent server.
fn has_agent_port(options: &[OsString]) -> bool {
    let mut it = options.iter();
    while let Some(a) = it.next() {
        if a == "--agent-port" || a.to_str().is_some_and(|t| t.starts_with("--agent-port=")) {
            return true;
        }
        if WITH_VALUE.iter().any(|w| a == w) {
            it.next();
        }
    }
    false
}

/// The folders of the list in `KADAKAS_ALLOW_DIR`: split like `PATH`, without the empty entries.
pub fn split_env_dirs(value: &OsStr) -> Vec<PathBuf> {
    std::env::split_paths(value)
        .filter(|p| !p.as_os_str().is_empty())
        .collect()
}

/// The options for an app that starts in another folder than `cwd` (the folder where the person
/// started). The result has these changes:
///
/// - the folders of `--allow-dir` are absolute;
/// - `KADAKAS_AGENT_PORT` and `KADAKAS_ALLOW_DIR` become `--agent-port` and `--allow-dir` (a
///   flag wins over the variable for the port; the folders add up), so that a way to the app that
///   does not keep the environment keeps the setting;
/// - the agent server is on and no folder is named: `cwd` is the folder, as when the app starts
///   by itself. (The root of the disk is never taken: a start from the Finder has that `cwd`.)
///
/// `cwd` is `None` when it is not known: then relative folders stay as they are.
pub fn prepare(
    options: Vec<OsString>,
    env_port: Option<&str>,
    env_allow_dir: Option<&OsStr>,
    cwd: Option<&Path>,
) -> Vec<OsString> {
    let abs = |d: &Path| match cwd {
        Some(cwd) => absolute(d, cwd),
        None => d.to_path_buf(),
    };
    let mut out = Vec::with_capacity(options.len() + 4);
    let mut it = options.iter();
    while let Some(a) = it.next() {
        if a == "--allow-dir" {
            if let Some(v) = it.next() {
                out.push(a.clone());
                out.push(abs(Path::new(v)).into_os_string());
            }
        } else if let Some(v) = a.to_str().and_then(|t| t.strip_prefix("--allow-dir=")) {
            let mut word = OsString::from("--allow-dir=");
            word.push(abs(Path::new(v)));
            out.push(word);
        } else {
            out.push(a.clone());
            if WITH_VALUE.iter().any(|w| a == w) {
                out.extend(it.next().cloned());
            }
        }
    }
    if !has_agent_port(&out)
        && let Some(port) = env_port.map(str::trim).filter(|p| p.parse::<u16>().is_ok())
    {
        out.push("--agent-port".into());
        out.push(port.into());
    }
    let mut dirs = allow_dirs_in(&out);
    for d in env_allow_dir.map(split_env_dirs).unwrap_or_default() {
        let d = abs(&d);
        if !dirs.contains(&d) {
            out.push("--allow-dir".into());
            out.push(d.clone().into_os_string());
            dirs.push(d);
        }
    }
    if dirs.is_empty()
        && has_agent_port(&out)
        && let Some(cwd) = cwd.filter(|c| c.parent().is_some())
    {
        out.push("--allow-dir".into());
        out.push(cwd.as_os_str().to_os_string());
    }
    out
}

/// The value for `KADAKAS_ALLOW_DIR` with absolute folders, so that the app (which starts in
/// another folder) reads the same folders as the person meant. `None` when the list is empty.
pub fn absolute_env_dirs(value: &OsStr, cwd: &Path) -> Option<OsString> {
    let dirs: Vec<PathBuf> = split_env_dirs(value)
        .iter()
        .map(|d| absolute(d, cwd))
        .collect();
    std::env::join_paths(dirs).ok().filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn options_are_taken_with_their_values() {
        let args = os(&[
            "--agent-port",
            "0",
            "a.kad",
            "--allow-dir=/x",
            "--allow-converters",
            "--allow-dir",
            "/y",
            "--site",
            "--",
            "--agent-port",
            "9",
        ]);
        assert_eq!(
            options_in(&args),
            os(&[
                "--agent-port",
                "0",
                "--allow-dir=/x",
                "--allow-converters",
                "--allow-dir",
                "/y"
            ])
        );
        // A value that looks like a document is a value, not a document.
        assert_eq!(
            options_in(&os(&["--profile", "acad"])),
            os(&["--profile", "acad"])
        );
        // A last option without a value is left out; take says so.
        assert!(options_in(&os(&["--allow-dir"])).is_empty());
        let mut none = [].iter();
        assert!(take(OsStr::new("--allow-dir"), &mut none).is_err());
        assert_eq!(take(OsStr::new("--unknown"), &mut none), Ok(None));
        assert_eq!(take(OsStr::new("a.kad"), &mut none), Ok(None));
    }

    #[test]
    fn every_option_of_the_app_is_listed() {
        // The app reads these in `agent.rs`, `app.rs` and `kadakas-ui-core/src/collab/args.rs`.
        for flag in [
            "--agent-port",
            "--allow-dir",
            "--allow-url",
            "--allow-collab-url",
            "--collab-url",
            "--collab-token",
            "--profile",
            "--allow-converters",
            "--allow-agent-scripts",
        ] {
            let value = os(&["v"]);
            let got = take(OsStr::new(flag), &mut value.iter()).unwrap();
            assert!(got.is_some(), "{flag}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn folders_become_absolute_and_the_variables_become_options() {
        let cwd = Path::new("/work/plans");
        let out = prepare(
            os(&[
                "--allow-dir",
                "rel",
                "--allow-dir=/abs",
                "--agent-port",
                "7",
            ]),
            Some("9"),
            Some(&std::env::join_paths(["env1", "/env2", "rel"]).unwrap()),
            Some(cwd),
        );
        assert_eq!(
            out,
            os(&[
                "--allow-dir",
                "/work/plans/rel",
                "--allow-dir=/abs",
                "--agent-port",
                "7",
                "--allow-dir",
                "/work/plans/env1",
                "--allow-dir",
                "/env2",
            ])
        );
    }

    #[test]
    #[cfg(unix)]
    fn the_port_variable_counts_when_no_flag_has_it() {
        let cwd = Path::new("/w");
        let out = prepare(vec![], Some(" 0 "), None, Some(cwd));
        assert_eq!(out, os(&["--agent-port", "0", "--allow-dir", "/w"]));
        // Not a port: nothing.
        assert!(prepare(vec![], Some("x"), None, Some(cwd)).is_empty());
        assert!(prepare(vec![], Some("70000"), None, Some(cwd)).is_empty());
        // No agent server: no folder is added.
        assert!(prepare(vec![], None, None, Some(cwd)).is_empty());
        assert_eq!(
            prepare(os(&["--allow-converters"]), None, None, Some(cwd)),
            os(&["--allow-converters"])
        );
    }

    #[test]
    #[cfg(unix)]
    fn the_start_folder_is_the_default_folder_but_never_the_root() {
        let on = os(&["--agent-port=0"]);
        assert_eq!(
            prepare(on.clone(), None, None, Some(Path::new("/w"))),
            os(&["--agent-port=0", "--allow-dir", "/w"])
        );
        assert_eq!(prepare(on.clone(), None, None, Some(Path::new("/"))), on);
        assert_eq!(prepare(on.clone(), None, None, None), on);
        // A folder that the person named replaces the default.
        let named = os(&["--agent-port=0", "--allow-dir=/n"]);
        assert_eq!(
            prepare(named.clone(), None, None, Some(Path::new("/w"))),
            named
        );
        // The variable names the folder too.
        let env = std::env::join_paths(["/n"]).unwrap();
        assert_eq!(
            prepare(on, None, Some(&env), Some(Path::new("/w"))),
            os(&["--agent-port=0", "--allow-dir", "/n"])
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_folder_in_the_flag_and_in_the_variable_is_listed_once() {
        let env = std::env::join_paths(["/a", "", "/b"]).unwrap();
        let out = prepare(os(&["--allow-dir", "/a"]), None, Some(&env), None);
        assert_eq!(out, os(&["--allow-dir", "/a", "--allow-dir", "/b"]));
    }

    #[test]
    #[cfg(unix)]
    fn the_variable_gets_absolute_folders() {
        let env = std::env::join_paths(["x", "/y", ""]).unwrap();
        let got = absolute_env_dirs(&env, Path::new("/c")).unwrap();
        assert_eq!(
            split_env_dirs(&got),
            [PathBuf::from("/c/x"), PathBuf::from("/y")]
        );
        assert!(absolute_env_dirs(OsStr::new(""), Path::new("/c")).is_none());
    }
}
