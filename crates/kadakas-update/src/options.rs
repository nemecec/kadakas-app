//! The options of the programs that the launcher starts (PLAT-32).
//!
//! A person can start Kadakas with options: `--agent-port 0 --allow-dir DIR`. The launcher and
//! the macOS handover (the app that finds itself in `Kadakas Engine.app` and asks the launcher to
//! start) must pass these options on to the app, or the app starts without them. This file is the
//! one list of the options and the pure functions that handle them. Nothing here starts a program
//! and nothing here reads the file system, except [`check_dirs`].
//!
//! The app starts in the folder of its version, not in the folder where the person started it. So
//! the folders are made absolute here. The rules for the allowed folders are strict:
//!
//! - The allowed folders are exactly the ones that the person names, with `--allow-dir` and
//!   `KADAKAS_ALLOW_DIR`. A start through the launcher never adds the start folder: it can be the
//!   home folder or a system folder. Only a direct start of the app from a terminal uses the
//!   current folder when the person names none (the parsers of the app).
//! - A relative folder needs a known start folder that is not the root. Otherwise it is refused
//!   with a note (`.` against `/` would be the whole disk).
//! - The root of a disk (`/`, `C:\`) is never allowed, whatever the source.
//! - `--collab-token` never goes on a command line that this code builds. A command line is
//!   visible to other users (`ps`, `/proc`). The token goes in the environment (`split_token`).

use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

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

/// The option with a secret value. It is known (so its value is not read as a document), but the
/// code that builds a command line for another program removes it ([`split_token`]).
pub const SECRET_OPTION: &str = "--collab-token";

/// The variable with the folders that agents may use (`--allow-dir` as a list, separated like the
/// `PATH` of the system).
pub const ENV_ALLOW_DIR: &str = "KADAKAS_ALLOW_DIR";

/// The variable with the port of the agent server (`--agent-port`).
pub const ENV_AGENT_PORT: &str = "KADAKAS_AGENT_PORT";

/// The variable with the token for the default collaboration server (`--collab-token`).
pub const ENV_COLLAB_TOKEN: &str = "KADAKAS_COLLAB_TOKEN";

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

/// The options of the app in `args` as pairs `(name, value)`, in order. This is the one parse
/// that the app uses for its own command line: it stops at `--` (what follows are documents),
/// it reads the value of an option as a value (never as another option), and it leaves out
/// documents and unknown words. A switch has no value. An option at the end has none either.
/// The first word of `args` (the program name) is no option, so the whole `std::env::args()`
/// can be passed.
pub fn pairs(args: &[String]) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--" {
            break;
        }
        if let Some((name, value)) = a.split_once('=')
            && WITH_VALUE.contains(&name)
        {
            out.push((name.to_string(), Some(value.to_string())));
        } else if WITH_VALUE.contains(&a.as_str()) {
            out.push((a.clone(), it.next().cloned()));
        } else if SWITCHES.contains(&a.as_str()) {
            out.push((a.clone(), None));
        }
    }
    out
}

/// The values of the option `name` in `args`, in order (see [`pairs`]).
pub fn values_of(args: &[String], name: &str) -> Vec<String> {
    pairs(args)
        .into_iter()
        .filter(|(n, _)| n == name)
        .filter_map(|(_, v)| v)
        .collect()
}

/// True when the switch `name` is in `args` (see [`pairs`]).
pub fn has_switch(args: &[String], name: &str) -> bool {
    pairs(args).iter().any(|(n, _)| n == name)
}

/// The words of the options in `names` (as `--name value`), for a parser that does its own reading
/// of them (the collaboration settings).
pub fn pick(args: &[String], names: &[&str]) -> Vec<String> {
    pairs(args)
        .into_iter()
        .filter(|(n, _)| names.contains(&n.as_str()))
        .flat_map(|(n, v)| [Some(n), v])
        .flatten()
        .collect()
}

/// Takes `--collab-token` out of `options`. Returns the options without it and the token (the last
/// one wins). The token never stays in a word list that becomes a command line.
pub fn split_token(options: Vec<OsString>) -> (Vec<OsString>, Option<OsString>) {
    let mut out = Vec::with_capacity(options.len());
    let mut token = None;
    let mut it = options.into_iter();
    while let Some(a) = it.next() {
        if a == SECRET_OPTION {
            token = it.next().or(token);
        } else if let Some(v) = a.to_str().and_then(|t| t.strip_prefix("--collab-token=")) {
            token = Some(OsString::from(v));
        } else {
            let takes_value = WITH_VALUE.iter().any(|w| a == *w);
            out.push(a);
            if takes_value {
                out.extend(it.next());
            }
        }
    }
    (out, token)
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

/// `path` with `.` and `..` resolved by the words alone (no file system). `..` at the root stays
/// at the root.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// True when `path` is the root of a disk by its words: `/`, `/.`, `/..`, `C:\`.
pub fn is_root_lexical(path: &Path) -> bool {
    path.is_absolute()
        && !lexical(path)
            .components()
            .any(|c| matches!(c, Component::Normal(_)))
}

/// A folder as `prepare` needs it: absolute and not the root. `Err` holds the note for the person.
fn absolute_dir(dir: &Path, cwd: Option<&Path>, source: &str) -> Result<PathBuf, String> {
    if dir.as_os_str().is_empty() {
        return Err(format!("{source}: an empty folder is ignored"));
    }
    let full = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        // The folder where the person started. The root is no start folder: a launcher that
        // the system started (the Dock, `open`) has it, and `.` there would be the whole disk.
        match cwd.filter(|c| c.parent().is_some()) {
            Some(cwd) => cwd.join(dir),
            None => {
                return Err(format!(
                    "{source}: the relative folder {} is refused, because the folder where you started is not known \
                     or is the root of the disk. Give an absolute folder",
                    dir.display()
                ));
            }
        }
    };
    if is_root_lexical(&full) {
        return Err(format!(
            "{source}: the folder {} is the root of the disk and is refused. Give a folder inside it",
            dir.display()
        ));
    }
    Ok(full)
}

/// The result of [`prepare`].
#[derive(Debug, PartialEq, Eq)]
pub struct Prepared {
    /// The options for the app, with absolute folders.
    pub options: Vec<OsString>,
    /// One line for each folder or setting that was refused. The caller prints them.
    pub notes: Vec<String>,
}

/// The options for an app that starts in another folder than `cwd` (the folder where the person
/// started). The result has these changes:
///
/// - the folders of `--allow-dir` are absolute, and a folder that cannot be made absolute (a
///   relative one without a start folder, or against the root) or that is the root of a disk is
///   refused with a note;
/// - `KADAKAS_AGENT_PORT` and `KADAKAS_ALLOW_DIR` become `--agent-port` and `--allow-dir` (a
///   flag wins over the variable for the port; the folders add up, each folder once), so that a
///   way to the app that does not keep the environment keeps the setting.
///
/// No folder is added: the start folder is no default here. `cwd` is `None` when it is not known.
/// `--collab-token` must be taken out before ([`split_token`]).
pub fn prepare(
    options: Vec<OsString>,
    env_port: Option<&str>,
    env_allow_dir: Option<&OsStr>,
    cwd: Option<&Path>,
) -> Prepared {
    let mut state = Prepared {
        options: Vec::with_capacity(options.len() + 4),
        notes: Vec::new(),
    };
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut it = options.iter();
    while let Some(a) = it.next() {
        if a == "--allow-dir" {
            if let Some(v) = it.next() {
                add_dir(&mut state, &mut dirs, PathBuf::from(v), cwd, "--allow-dir");
            }
        } else if let Some(v) = a.to_str().and_then(|t| t.strip_prefix("--allow-dir=")) {
            add_dir(&mut state, &mut dirs, PathBuf::from(v), cwd, "--allow-dir");
        } else {
            state.options.push(a.clone());
            if WITH_VALUE.iter().any(|w| a == w) {
                state.options.extend(it.next().cloned());
            }
        }
    }
    if !has_agent_port(&state.options)
        && let Some(port) = env_port.map(str::trim).filter(|p| p.parse::<u16>().is_ok())
    {
        state.options.push("--agent-port".into());
        state.options.push(port.into());
    }
    for d in env_allow_dir.map(split_env_dirs).unwrap_or_default() {
        add_dir(&mut state, &mut dirs, d, cwd, ENV_ALLOW_DIR);
    }
    state
}

/// Adds the folder `dir` as `--allow-dir` to `state` when it can be used, else adds a note.
fn add_dir(
    state: &mut Prepared,
    dirs: &mut Vec<PathBuf>,
    dir: PathBuf,
    cwd: Option<&Path>,
    source: &str,
) {
    match absolute_dir(&dir, cwd, source) {
        Ok(full) if !dirs.contains(&full) => {
            state.options.push("--allow-dir".into());
            state.options.push(full.clone().into_os_string());
            dirs.push(full);
        }
        Ok(_) => {}
        Err(note) => state.notes.push(note),
    }
}

/// The lines for the folders of `dirs` that do not exist: the app drops them
/// (`PathPolicy::restricted`), and the person should hear it.
pub fn missing_dir_notes(dirs: &[PathBuf]) -> Vec<String> {
    dirs.iter()
        .filter(|d| !d.exists())
        .map(|d| {
            format!(
                "--allow-dir: the folder {} does not exist and is dropped. Agents cannot use it",
                d.display()
            )
        })
        .collect()
}

/// Checks the allowed folders of a start of the app, with the file system: a folder that does not
/// exist is dropped, and so is a folder that is the root of the disk after links are followed
/// (`.` in `/`, a link to `/`). Returns the folders to use and one note for each dropped folder.
pub fn check_dirs(dirs: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<String>) {
    let mut kept = Vec::new();
    let mut notes = Vec::new();
    for d in dirs {
        match std::fs::canonicalize(&d) {
            Err(_) => notes.push(format!(
                "the allowed folder {} does not exist and is dropped. Agents cannot use it",
                d.display()
            )),
            Ok(c) if c.parent().is_none() || is_root_lexical(&c) => notes.push(format!(
                "the allowed folder {} is the root of the disk and is refused",
                d.display()
            )),
            Ok(_) => kept.push(d),
        }
    }
    (kept, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn s(list: &[&str]) -> Vec<String> {
        list.iter().map(|x| x.to_string()).collect()
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
    fn pairs_stop_at_the_double_dash_and_read_values_as_values() {
        let args = s(&[
            "app",
            "--allow-dir",
            "--agent-port",
            "0",
            "--profile=acad",
            "--allow-agent-scripts",
            "--",
            "--allow-dir=/",
            "--allow-agent-scripts",
        ]);
        assert_eq!(
            pairs(&args),
            [
                ("--allow-dir".to_string(), Some("--agent-port".to_string())),
                ("--profile".to_string(), Some("acad".to_string())),
                ("--allow-agent-scripts".to_string(), None),
            ]
        );
        // The names after `--` are documents.
        assert!(values_of(&args, "--allow-dir") == ["--agent-port"]);
        assert!(has_switch(&args, "--allow-agent-scripts"));
        let only_files = s(&["app", "--", "--allow-agent-scripts", "--allow-dir=/"]);
        assert!(!has_switch(&only_files, "--allow-agent-scripts"));
        assert!(values_of(&only_files, "--allow-dir").is_empty());
        // The value of another option is no switch.
        assert!(!has_switch(
            &s(&["app", "--profile", "--allow-converters"]),
            "--allow-converters"
        ));
        // An option at the end has no value.
        assert_eq!(
            pairs(&s(&["app", "--agent-port"])),
            [("--agent-port".to_string(), None)]
        );
    }

    #[test]
    fn pick_gives_the_words_of_the_named_options() {
        let args = s(&[
            "app",
            "--collab-url=u",
            "--profile",
            "--collab-token",
            "--allow-collab-url",
            "h",
            "--",
            "--collab-url=x",
        ]);
        assert_eq!(
            pick(&args, &["--collab-url", "--allow-collab-url"]),
            s(&["--collab-url", "u", "--allow-collab-url", "h"])
        );
    }

    #[test]
    fn the_token_leaves_the_options() {
        let (rest, token) = split_token(os(&[
            "--agent-port",
            "0",
            "--collab-token",
            "s3cret",
            "--allow-dir=/d",
            "--collab-token=other",
        ]));
        assert_eq!(rest, os(&["--agent-port", "0", "--allow-dir=/d"]));
        assert_eq!(token, Some(OsString::from("other")));
        // A value that is the name of the token option stays a value.
        let (rest, token) = split_token(os(&["--profile", "--collab-token"]));
        assert_eq!(rest, os(&["--profile", "--collab-token"]));
        assert_eq!(token, None);
        let (rest, token) = split_token(os(&["--collab-token"]));
        assert!(rest.is_empty() && token.is_none());
    }

    #[test]
    fn roots_are_found_by_their_words() {
        for root in ["/", "/.", "/..", "//", "/a/../.."] {
            assert!(is_root_lexical(Path::new(root)), "{root}");
        }
        for not in ["/a", "/a/..b", "a", ".", "..", "/a/./b"] {
            assert!(!is_root_lexical(Path::new(not)), "{not}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn folders_become_absolute_and_the_variables_become_options() {
        let cwd = Path::new("/work/plans");
        let got = prepare(
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
        assert!(got.notes.is_empty(), "{:?}", got.notes);
        assert_eq!(
            got.options,
            os(&[
                "--allow-dir",
                "/work/plans/rel",
                "--allow-dir",
                "/abs",
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
    fn a_relative_folder_against_the_root_or_nothing_is_refused() {
        let root = Some(Path::new("/"));
        for bad in [".", "./x", "..", "../x", "x", ""] {
            for cwd in [root, None] {
                // As a flag, as a flag with `=`, and in the variable.
                let flag = prepare(os(&["--allow-dir", bad]), None, None, cwd);
                assert!(flag.options.is_empty(), "{bad:?} {flag:?}");
                assert_eq!(flag.notes.len(), 1, "{bad:?}");
                let eq = prepare(vec![format!("--allow-dir={bad}").into()], None, None, cwd);
                assert!(eq.options.is_empty(), "{bad:?} {eq:?}");
                let env = std::env::join_paths([bad]).unwrap();
                let var = prepare(vec![], None, Some(&env), cwd);
                if bad.is_empty() {
                    // An empty list has no entry at all.
                    assert!(var.options.is_empty() && var.notes.is_empty());
                } else {
                    assert!(var.options.is_empty(), "{bad:?} {var:?}");
                    assert_eq!(var.notes.len(), 1, "{bad:?}");
                }
            }
        }
        // The same words are fine against a real folder.
        let ok = prepare(os(&["--allow-dir", "x"]), None, None, Some(Path::new("/w")));
        assert_eq!(ok.options, os(&["--allow-dir", "/w/x"]));
    }

    #[test]
    #[cfg(unix)]
    fn the_root_of_the_disk_is_refused_from_every_source() {
        let cwd = Some(Path::new("/w"));
        for root in ["/", "/.", "/..", "//", "/w/../.."] {
            let flag = prepare(os(&["--allow-dir", root]), None, None, cwd);
            assert!(flag.options.is_empty(), "{root} {flag:?}");
            assert_eq!(flag.notes.len(), 1);
            let env = std::env::join_paths([root]).unwrap();
            let var = prepare(os(&["--agent-port=0"]), None, Some(&env), cwd);
            assert_eq!(var.options, os(&["--agent-port=0"]), "{root}");
            assert_eq!(var.notes.len(), 1);
        }
        // A folder next to a refused one is kept.
        let mixed = prepare(
            os(&["--allow-dir", "/", "--allow-dir", "/ok"]),
            None,
            None,
            cwd,
        );
        assert_eq!(mixed.options, os(&["--allow-dir", "/ok"]));
    }

    #[test]
    #[cfg(unix)]
    fn the_port_variable_counts_when_no_flag_has_it() {
        let cwd = Some(Path::new("/w"));
        let got = prepare(vec![], Some(" 0 "), None, cwd);
        // No folder is added: the start folder is no default for a start through the launcher.
        assert_eq!(got.options, os(&["--agent-port", "0"]));
        // Not a port: nothing.
        assert!(prepare(vec![], Some("x"), None, cwd).options.is_empty());
        assert!(prepare(vec![], Some("70000"), None, cwd).options.is_empty());
        assert!(prepare(vec![], None, None, cwd).options.is_empty());
        // A flag wins over the variable.
        assert_eq!(
            prepare(os(&["--agent-port=5"]), Some("0"), None, cwd).options,
            os(&["--agent-port=5"])
        );
    }

    #[test]
    #[cfg(unix)]
    fn no_start_folder_is_added_for_a_start_through_the_launcher() {
        for cwd in [Some(Path::new("/home/me")), Some(Path::new("/")), None] {
            let got = prepare(os(&["--agent-port=0"]), None, None, cwd);
            assert_eq!(got.options, os(&["--agent-port=0"]));
            assert!(got.notes.is_empty());
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_folder_in_the_flag_and_in_the_variable_is_listed_once() {
        let env = std::env::join_paths(["/a", "", "/b"]).unwrap();
        let got = prepare(os(&["--allow-dir", "/a"]), None, Some(&env), None);
        assert_eq!(got.options, os(&["--allow-dir", "/a", "--allow-dir", "/b"]));
        assert!(got.notes.is_empty());
    }

    #[test]
    fn missing_and_root_folders_are_dropped_with_a_note() {
        let here = std::env::temp_dir();
        let gone = here.join("kadakas-options-no-such-folder");
        let (kept, notes) = check_dirs(vec![here.clone(), gone.clone()]);
        assert_eq!(kept, [here]);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("does not exist"), "{notes:?}");
        assert_eq!(missing_dir_notes(&[gone]).len(), 1);
        #[cfg(unix)]
        {
            // The words are different; the folder is the root.
            let (kept, notes) = check_dirs(vec![PathBuf::from("/"), PathBuf::from("/tmp/../..")]);
            assert!(kept.is_empty(), "{kept:?}");
            assert_eq!(notes.len(), 2);
            assert!(notes[0].contains("root"), "{notes:?}");
        }
    }
}
