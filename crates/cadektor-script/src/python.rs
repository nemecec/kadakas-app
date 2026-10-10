//! Finding Python. Cadektor does not embed CPython: it starts the Python of the system.
//!
//! Order of choice:
//! 1. The program in `CADEKTOR_PYTHON`, if set.
//! 2. `python3` on PATH.
//! 3. `uv run --no-project python`, if `uv` is on PATH.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A program and the arguments that come before the script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interpreter {
    pub program: PathBuf,
    pub prefix: Vec<OsString>,
}

impl Interpreter {
    /// A command that runs this interpreter. Add the script and its arguments after it.
    pub fn command(&self) -> Command {
        let mut c = Command::new(&self.program);
        c.args(&self.prefix);
        c
    }

    /// A short text for the user, for example `python3 (/usr/bin/python3)`.
    pub fn describe(&self) -> String {
        let mut s = self.program.display().to_string();
        for a in &self.prefix {
            s.push(' ');
            s.push_str(&a.to_string_lossy());
        }
        s
    }
}

/// The first file with this name in a PATH-style list of folders.
pub fn find_on_path(name: &str, path_var: &std::ffi::OsStr) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    for dir in std::env::split_paths(path_var) {
        for ext in exts {
            let p = dir.join(format!("{name}{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Chooses the interpreter. `env_python` is the value of `CADEKTOR_PYTHON`, `path_var` is PATH.
pub fn discover_with(env_python: Option<&str>, path_var: &std::ffi::OsStr) -> Option<Interpreter> {
    if let Some(p) = env_python.filter(|p| !p.trim().is_empty()) {
        return Some(Interpreter {
            program: PathBuf::from(p),
            prefix: vec![],
        });
    }
    if let Some(p) = find_on_path("python3", path_var) {
        return Some(Interpreter {
            program: p,
            prefix: vec![],
        });
    }
    find_on_path("uv", path_var).map(|uv| Interpreter {
        program: uv,
        prefix: ["run", "--no-project", "python"]
            .iter()
            .map(OsString::from)
            .collect(),
    })
}

/// Chooses the interpreter from the environment of this process.
pub fn discover() -> Option<Interpreter> {
    let env = std::env::var("CADEKTOR_PYTHON").ok();
    discover_with(
        env.as_deref(),
        &std::env::var_os("PATH").unwrap_or_default(),
    )
}

/// The folder that holds the `cadektor` Python package, if we can find it. A script finds the
/// package through PYTHONPATH when it is not installed. Order: `CADEKTOR_PYTHON_DIR`, then a
/// `python` folder next to the program or in a parent folder (up to 5 levels), then `./python`.
pub fn package_dir() -> Option<PathBuf> {
    let has_pkg = |d: &Path| d.join("cadektor").join("__init__.py").is_file();
    if let Some(d) = std::env::var_os("CADEKTOR_PYTHON_DIR").map(PathBuf::from) {
        return has_pkg(&d).then_some(d);
    }
    if let Ok(exe) = std::env::current_exe() {
        for dir in exe.ancestors().skip(1).take(6) {
            let d = dir.join("python");
            if has_pkg(&d) {
                return Some(d);
            }
        }
    }
    let d = std::env::current_dir().ok()?.join("python");
    has_pkg(&d).then_some(d)
}

/// PYTHONPATH with `dir` first, followed by the old value.
pub fn python_path_with(dir: &Path) -> OsString {
    let mut parts = vec![dir.to_path_buf()];
    if let Some(old) = std::env::var_os("PYTHONPATH") {
        parts.extend(std::env::split_paths(&old));
    }
    std::env::join_paths(parts).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cadektor-py-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn env_python_wins_and_empty_is_ignored() {
        let none = std::ffi::OsString::new();
        let i = discover_with(Some("/opt/py/bin/python"), &none).unwrap();
        assert_eq!(i.program, PathBuf::from("/opt/py/bin/python"));
        assert!(discover_with(Some("  "), &none).is_none());
        assert!(discover_with(None, &none).is_none());
    }

    #[test]
    fn python3_is_found_before_uv() {
        let d = temp("both");
        std::fs::write(d.join("python3"), "").unwrap();
        std::fs::write(d.join("uv"), "").unwrap();
        let i = discover_with(None, d.as_os_str()).unwrap();
        assert!(i.program.ends_with("python3"));
        assert!(i.prefix.is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn uv_is_the_fallback() {
        let d = temp("uv");
        std::fs::write(d.join("uv"), "").unwrap();
        let i = discover_with(None, d.as_os_str()).unwrap();
        assert!(i.program.ends_with("uv"));
        assert_eq!(i.prefix, ["run", "--no-project", "python"]);
        assert!(i.describe().contains("run --no-project python"));
        let _ = i.command();
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn python_path_puts_our_folder_first() {
        let p = python_path_with(Path::new("/x/python"));
        assert_eq!(
            std::env::split_paths(&p).next().unwrap(),
            PathBuf::from("/x/python")
        );
    }
}
