//! Process groups, kill of a process tree, and process liveness.

use std::process::Command;

/// Kills the process and its whole process group (on Unix), so child processes of the script die too.
pub fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // The child leads its own group (see `own_group`), so the group ID is its process ID.
        if let Ok(pid) = i32::try_from(child.id()) {
            // SAFETY: killpg only sends a signal.
            unsafe {
                libc::killpg(pid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
}

/// Makes the command start its own process group, so `kill_tree` reaches child processes of the child.
pub fn own_group(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(not(unix))]
    let _ = cmd;
}

/// Best effort: is a process with this id running? Unknown counts as running.
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // A process id is a signed number: a larger value names no process.
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        // SAFETY: signal 0 only checks that the process exists.
        let r = unsafe { libc::kill(pid, 0) };
        // EPERM means the process exists but belongs to someone else.
        r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        use std::process::Stdio;
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .stderr(Stdio::null())
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .split_whitespace()
                    .any(|w| w == pid.to_string())
            })
            .unwrap_or(true)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        true
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[test]
    fn liveness_of_this_process_and_of_a_finished_one() {
        assert!(pid_alive(std::process::id()));
        let mut c = Command::new("true").spawn().unwrap();
        let pid = c.id();
        c.wait().unwrap();
        assert!(!pid_alive(pid));
        assert!(
            !pid_alive(u32::MAX),
            "an id that does not fit names no process"
        );
    }

    #[test]
    fn kill_tree_reaches_the_children_of_the_child() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 60 & echo $!; wait"])
            .stdout(Stdio::piped());
        own_group(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        let mut line = String::new();
        std::io::BufRead::read_line(
            &mut std::io::BufReader::new(child.stdout.take().unwrap()),
            &mut line,
        )
        .unwrap();
        let grandchild: u32 = line.trim().parse().unwrap();
        assert!(pid_alive(grandchild));
        kill_tree(&mut child);
        child.wait().unwrap();
        for _ in 0..200 {
            if !pid_alive(grandchild) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the grandchild is still alive");
    }
}
