//! File helpers shared by the CLI and the desktop app: safe saves, and the
//! runtime file that tells agent clients how to reach a running app.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The path for a sibling of `path`: same folder, `prefix` + file name + `suffix`.
fn sibling(path: &Path, prefix: &str, suffix: &str) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!("{prefix}{name}{suffix}"))
}

/// Writes `bytes` to a temporary file in the same folder, flushes it, and
/// renames it over `path`. A crash never leaves a half-written file.
/// With `keep_backup`, the previous file is first copied to `<path>.bak`.
/// With `private`, the file is readable by the owner only (Unix).
fn write_replacing(path: &Path, bytes: &[u8], keep_backup: bool, private: bool) -> std::io::Result<()> {
    // Write to the real target, so a symlink stays a symlink.
    let resolved = std::fs::canonicalize(path).ok();
    let path = resolved.as_deref().unwrap_or(path);
    let old_perms = std::fs::metadata(path).ok().map(|m| m.permissions());
    let tmp = sibling(path, ".", &format!(".{}.tmp", std::process::id()));
    let result = (|| {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let _ = private;
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        if let Some(p) = &old_perms {
            f.set_permissions(p.clone())?;
        }
        drop(f);
        if keep_backup && path.is_file() {
            std::fs::copy(path, sibling(path, "", ".bak"))?;
        }
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Saves a document file atomically and keeps the previous version as `<path>.bak`.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_replacing(path, bytes, true, false)
}

/// How to reach a running app: its port and the secret that clients must send first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub port: u16,
    pub token: String,
}

/// Where the running app publishes its `AgentInfo`.
/// Windows: `%LOCALAPPDATA%\cadektor\agent.json`. Elsewhere `$XDG_RUNTIME_DIR/cadektor/agent.json`,
/// or `~/Library/Application Support/cadektor/agent.json` on macOS,
/// or `~/.local/state/cadektor/agent.json`.
pub fn agent_info_path() -> Option<PathBuf> {
    agent_info_path_for(std::env::consts::OS, |k| std::env::var_os(k))
}

fn agent_info_path_for(os: &str, env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let var = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let dir = if os == "windows" {
        var("LOCALAPPDATA")?
    } else if let Some(x) = var("XDG_RUNTIME_DIR") {
        x
    } else if os == "macos" {
        var("HOME")?.join("Library").join("Application Support")
    } else {
        var("HOME")?.join(".local").join("state")
    };
    Some(dir.join("cadektor").join("agent.json"))
}

/// Writes the runtime file with owner-only permissions (0600, in a 0700 folder).
pub fn write_agent_info(path: &Path, info: &AgentInfo) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        let mut b = std::fs::DirBuilder::new();
        b.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            b.mode(0o700);
        }
        b.create(dir)?;
    }
    let text = serde_json::to_vec(info).map_err(std::io::Error::other)?;
    write_replacing(path, &text, false, true)
}

pub fn read_agent_info(path: &Path) -> Result<AgentInfo, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cadektor-fsio-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn atomic_write_replaces_file_keeps_backup_and_leaves_no_temp_file() {
        let dir = temp_dir("atomic");
        let f = dir.join("a.cdk.json");
        write_atomic(&f, b"one").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"one");
        assert!(!dir.join("a.cdk.json.bak").exists(), "no backup for a new file");
        write_atomic(&f, b"two").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"two");
        assert_eq!(std::fs::read(dir.join("a.cdk.json.bak")).unwrap(), b"one");
        write_atomic(&f, b"three").unwrap();
        assert_eq!(std::fs::read(dir.join("a.cdk.json.bak")).unwrap(), b"two");
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_keeps_permissions_and_symlinks() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("perm");
        let real = dir.join("real.json");
        std::fs::write(&real, "old").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
        write_atomic(&real, b"new").unwrap();
        assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o640);
        let link = dir.join("link.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomic(&link, b"via link").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&real).unwrap(), b"via link");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn failed_write_cleans_up_and_keeps_the_old_file() {
        let dir = temp_dir("fail");
        // The target is a folder, so the final rename fails.
        let target = dir.join("t");
        std::fs::create_dir(&target).unwrap();
        assert!(write_atomic(&target, b"x").is_err());
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1, "{names:?}");
        // A missing folder is an error too.
        assert!(write_atomic(&dir.join("missing").join("f"), b"x").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn agent_info_path_per_platform() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(*v))
        };
        let tail = |p: Option<PathBuf>| p.map(|p| p.to_string_lossy().replace('\\', "/"));
        assert_eq!(
            tail(agent_info_path_for("linux", env(&[("XDG_RUNTIME_DIR", "/run/user/1"), ("HOME", "/h")]))).as_deref(),
            Some("/run/user/1/cadektor/agent.json")
        );
        assert_eq!(tail(agent_info_path_for("linux", env(&[("HOME", "/h")]))).as_deref(), Some("/h/.local/state/cadektor/agent.json"));
        assert_eq!(
            tail(agent_info_path_for("macos", env(&[("HOME", "/Users/me")]))).as_deref(),
            Some("/Users/me/Library/Application Support/cadektor/agent.json")
        );
        assert!(tail(agent_info_path_for("macos", env(&[("XDG_RUNTIME_DIR", ""), ("HOME", "/h")]))).unwrap().contains("Library"));
        assert_eq!(
            tail(agent_info_path_for("windows", env(&[("LOCALAPPDATA", "C:/Users/me/AppData/Local")]))).as_deref(),
            Some("C:/Users/me/AppData/Local/cadektor/agent.json")
        );
        assert_eq!(agent_info_path_for("linux", env(&[])), None);
        assert_eq!(agent_info_path_for("windows", env(&[("HOME", "/h")])), None);
        assert!(agent_info_path().is_some() || std::env::var_os("HOME").is_none());
    }

    #[test]
    fn agent_info_round_trips_and_is_private() {
        let dir = temp_dir("info");
        let f = dir.join("sub").join("agent.json");
        let info = AgentInfo { port: 4242, token: "ab".repeat(32) };
        write_agent_info(&f, &info).unwrap();
        assert_eq!(read_agent_info(&f).unwrap(), info);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), format!(r#"{{"port":4242,"token":"{}"}}"#, info.token));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(f.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        }
        // Writing again replaces the file.
        let info2 = AgentInfo { port: 1, token: "cd".repeat(32) };
        write_agent_info(&f, &info2).unwrap();
        assert_eq!(read_agent_info(&f).unwrap(), info2);
        assert!(read_agent_info(&dir.join("none")).unwrap_err().contains("cannot read"));
        std::fs::write(dir.join("bad"), "{").unwrap();
        assert!(read_agent_info(&dir.join("bad")).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
