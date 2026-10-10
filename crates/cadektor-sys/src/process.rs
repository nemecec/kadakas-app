//! Process groups, kill of a process tree, and process liveness.

use std::process::Command;

/// Kills the process and its whole process group (on Unix), so child processes of the script die too.
///
/// The signal to the group can miss a process that the group leader is forking at that moment
/// (seen on macOS: a `sleep` that started while a stop was under way outlived the stop and held
/// the output pipes open). So the signal goes out again a moment later, when such a process is
/// in the group. Call it before you collect the leader (`wait`, `try_wait`; see
/// [`ended_not_collected`]): then the leader's process ID cannot name another group in the
/// meantime.
pub fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // The child leads its own group (see `own_group`), so the group ID is its process ID.
        if let Ok(pid) = i32::try_from(child.id()) {
            for round in 0..GROUP_KILL_ROUNDS {
                // SAFETY: killpg only sends a signal.
                unsafe {
                    libc::killpg(pid, libc::SIGKILL);
                }
                if round == 0 {
                    // The leader dies first, so it forks nothing more.
                    let _ = child.kill();
                }
                if round + 1 < GROUP_KILL_ROUNDS {
                    std::thread::sleep(std::time::Duration::from_millis(3));
                }
            }
        }
    }
    let _ = child.kill();
}

/// True if the child has ended but is not collected yet (Unix). Use it to kill the group of a
/// child before you collect the child with `wait` or `try_wait`: until then the child's process
/// ID cannot name another group. Always false where it cannot tell.
pub fn ended_not_collected(child: &std::process::Child) -> bool {
    #[cfg(unix)]
    {
        let pid: libc::id_t = child.id();
        // SAFETY: waitid with WNOWAIT only looks at the state of the child; it does not collect it.
        unsafe {
            let mut info: libc::siginfo_t = std::mem::zeroed();
            let r = libc::waitid(
                libc::P_PID,
                pid,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            );
            // With WNOHANG, `si_pid` stays 0 when no child has ended.
            r == 0 && info.si_pid() != 0
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child;
        false
    }
}

/// How often `kill_tree` signals the group.
#[cfg(unix)]
const GROUP_KILL_ROUNDS: usize = 3;

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

/// Best effort: is a process with this id running? A process of another user counts as not
/// running (Unix). Where it cannot tell, it counts as running.
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // A process id is a signed number: a larger value names no process.
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        // SAFETY: signal 0 only checks that the process exists.
        let r = unsafe { libc::kill(pid, 0) };
        // EPERM means the process belongs to someone else. A crash record names our own
        // processes, so such a process is not the one that we look for: it counts as dead.
        r == 0
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
    fn kill_tree_catches_children_that_start_while_it_runs() {
        // A leader that forks children all the time: some are in flight when the kill comes.
        for _ in 0..10 {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", "while :; do sleep 30 & done"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            own_group(&mut cmd);
            let mut child = cmd.spawn().unwrap();
            let pid = child.id() as i32;
            std::thread::sleep(std::time::Duration::from_millis(15));
            kill_tree(&mut child);
            child.wait().unwrap();
            // Nobody of the group is left: the pipe closes at once.
            let mut out = Vec::new();
            let t = std::time::Instant::now();
            std::io::Read::read_to_end(&mut child.stdout.take().unwrap(), &mut out).unwrap();
            assert!(t.elapsed() < std::time::Duration::from_secs(5));
            // The killed processes may be zombies for a moment, until init collects them.
            let gone = (0..400).any(|_| {
                // SAFETY: signal 0 only checks that the group exists.
                let empty = unsafe { libc::killpg(pid, 0) } == -1;
                if !empty {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                empty
            });
            assert!(gone, "the group is empty");
        }
    }

    #[test]
    fn an_ended_child_is_seen_before_it_is_collected() {
        let mut child = Command::new("true").spawn().unwrap();
        let seen = (0..500).any(|_| {
            let ended = ended_not_collected(&child);
            if !ended {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            ended
        });
        assert!(seen);
        // Looking does not collect it.
        assert!(ended_not_collected(&child));
        assert!(child.try_wait().unwrap().is_some());
        assert!(!ended_not_collected(&child), "collected: nothing to see");
    }

    #[test]
    fn a_process_of_another_user_counts_as_not_running() {
        // SAFETY: getuid has no effect.
        if unsafe { libc::getuid() } != 0 {
            assert!(!pid_alive(1), "init belongs to root: EPERM");
        }
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
