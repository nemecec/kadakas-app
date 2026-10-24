//! The runtime file that tells agent clients how to reach a running app.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fsio;

/// How to reach a running app: its port and the secret that clients must send first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub port: u16,
    pub token: String,
    /// The process id of the app. A client can see that the file is left over from an app that
    /// ended without cleaning up (a crash, a kill). Files of older versions have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

impl AgentInfo {
    /// The process id of the app if the file names one that no longer runs: the file is stale.
    pub fn stale_pid(&self) -> Option<u32> {
        self.pid.filter(|p| !crate::process::pid_alive(*p))
    }
}

/// The file that [`remove_file_on_signal`] removes, as a C string made before the handler is set.
#[cfg(unix)]
static SIGNAL_FILE: std::sync::OnceLock<std::ffi::CString> = std::sync::OnceLock::new();

/// The part of the signal handler that removes the file. Apart from the handler so that a test
/// can run it (a process that a signal ends writes no coverage data).
#[cfg(unix)]
fn unlink_signal_file() {
    if let Some(p) = SIGNAL_FILE.get() {
        // SAFETY: unlink may be called in a signal handler; the path was built before the
        // handler was set.
        unsafe {
            libc::unlink(p.as_ptr());
        }
    }
}

/// Deletes `path` when the process gets SIGTERM, SIGINT or SIGHUP, then lets the signal do what
/// it would have done (the process ends). A killed app then leaves no runtime file that points
/// to a port nobody listens on. Only the unlink runs in the handler, which is safe there.
/// A kill with SIGKILL, or a crash, still leaves the file: clients check the `pid` in it.
#[cfg(unix)]
pub fn remove_file_on_signal(path: &Path) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    extern "C" fn handler(sig: libc::c_int) {
        unlink_signal_file();
        // SAFETY: signal and raise may be called in a signal handler. The default action of the
        // signal ends the process as before.
        unsafe {
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
    }
    let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
        return;
    };
    if SIGNAL_FILE.set(c).is_err() {
        return;
    }
    for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        // SAFETY: the handler only calls functions that are safe in a signal handler.
        unsafe {
            libc::signal(sig, handler as *const () as libc::sighandler_t);
        }
    }
}

/// Where there are no Unix signals the file is removed when the app ends normally only.
#[cfg(not(unix))]
pub fn remove_file_on_signal(_path: &Path) {}

/// Where the running app publishes its `AgentInfo`: `agent.json` in the runtime folder.
pub fn agent_info_path() -> Option<PathBuf> {
    Some(crate::dirs::runtime_dir()?.join("agent.json"))
}

/// Writes the runtime file with owner-only permissions (0600, in a 0700 folder).
pub fn write_agent_info(path: &Path, info: &AgentInfo) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fsio::create_private_dir(dir)?;
    }
    let text = serde_json::to_vec(info).map_err(std::io::Error::other)?;
    fsio::write_private(path, &text)
}

pub fn read_agent_info(path: &Path) -> Result<AgentInfo, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_info_round_trips_and_is_private() {
        let dir = std::env::temp_dir().join(format!("kadakas-agentinfo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let f = dir.join("sub").join("agent.json");
        let info = AgentInfo {
            port: 4242,
            token: "ab".repeat(32),
            pid: None,
        };
        write_agent_info(&f, &info).unwrap();
        assert_eq!(read_agent_info(&f).unwrap(), info);
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            format!(r#"{{"port":4242,"token":"{}"}}"#, info.token)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&f), 0o600);
            assert_eq!(mode(f.parent().unwrap()), 0o700);
        }
        // Writing again replaces the file.
        let info2 = AgentInfo {
            port: 1,
            token: "cd".repeat(32),
            pid: None,
        };
        write_agent_info(&f, &info2).unwrap();
        assert_eq!(read_agent_info(&f).unwrap(), info2);
        assert!(
            read_agent_info(&dir.join("none"))
                .unwrap_err()
                .contains("cannot read")
        );
        std::fs::write(dir.join("bad"), "{").unwrap();
        assert!(read_agent_info(&dir.join("bad")).is_err());
        let _ = agent_info_path();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_of_a_dead_process_is_stale_and_one_without_a_pid_is_not() {
        let mut info = AgentInfo {
            port: 1,
            token: "x".into(),
            pid: None,
        };
        assert_eq!(info.stale_pid(), None);
        info.pid = Some(std::process::id());
        assert_eq!(info.stale_pid(), None, "this process runs");
        // The pid is written and read back.
        let dir = std::env::temp_dir().join(format!("kadakas-agentpid-{}", std::process::id()));
        let f = dir.join("agent.json");
        write_agent_info(&f, &info).unwrap();
        assert_eq!(read_agent_info(&f).unwrap().pid, Some(std::process::id()));
        // A process that has ended.
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        info.pid = Some(pid);
        assert_eq!(info.stale_pid(), Some(pid));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn the_handler_removes_the_file_that_was_given_first_and_only_that() {
        let dir = std::env::temp_dir().join(format!("kadakas-agentunlink-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (first, second) = (dir.join("first.json"), dir.join("second.json"));
        std::fs::write(&first, "x").unwrap();
        std::fs::write(&second, "x").unwrap();
        // This process arms the handlers (a signal here would end the test program, as it
        // should); the second call is ignored.
        remove_file_on_signal(&first);
        remove_file_on_signal(&second);
        unlink_signal_file();
        assert!(!first.exists(), "the first file is gone");
        assert!(second.exists(), "a later call changes nothing");
        // A file that is gone already is no error.
        unlink_signal_file();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn sigterm_deletes_the_file_and_the_process_still_ends_by_the_signal() {
        use std::io::BufRead;
        use std::os::unix::process::ExitStatusExt;
        if let Ok(path) = std::env::var("KADAKAS_TEST_SIGNAL_FILE") {
            // The child: arm the handler, say so, and wait for the signal.
            remove_file_on_signal(Path::new(&path));
            println!("armed");
            std::thread::sleep(std::time::Duration::from_secs(60));
            return;
        }
        let dir = std::env::temp_dir().join(format!("kadakas-agentsig-{}", std::process::id()));
        let f = dir.join("agent.json");
        write_agent_info(
            &f,
            &AgentInfo {
                port: 1,
                token: "x".into(),
                pid: None,
            },
        )
        .unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "agent::tests::sigterm_deletes_the_file_and_the_process_still_ends_by_the_signal",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("KADAKAS_TEST_SIGNAL_FILE", &f)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let out = child.stdout.take().unwrap();
        let mut lines = std::io::BufReader::new(out).lines();
        while let Some(Ok(l)) = lines.next() {
            if l.trim().ends_with("armed") {
                break;
            }
        }
        assert!(f.exists());
        // SAFETY: the pid is our own child.
        unsafe {
            libc::kill(child.id() as i32, libc::SIGTERM);
        }
        let status = child.wait().unwrap();
        assert!(!f.exists(), "the file is gone");
        assert_eq!(status.signal(), Some(libc::SIGTERM));
        let _ = std::fs::remove_dir_all(dir);
    }
}
