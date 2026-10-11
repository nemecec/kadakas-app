//! Runs a child process once, with a time limit and size limits.
//!
//! The child gets `stdin` as input. Its output is read up to a limit. If the child runs too long,
//! or writes too much, it is killed (with its process group on Unix) and the call fails.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

/// Limits for one run.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub timeout: Duration,
    /// Most bytes of standard output.
    pub max_stdout: usize,
    /// Most bytes of standard error that are kept (the rest is dropped).
    pub max_stderr: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            timeout: Duration::from_secs(5),
            max_stdout: 1 << 20,
            max_stderr: 16 << 10,
        }
    }
}

/// What a finished run gave.
#[derive(Debug)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: String,
    /// The exit code, or `None` if a signal ended the process.
    pub code: Option<i32>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExecError {
    /// The program could not start.
    Spawn(String),
    /// The run took longer than the time limit. The process was killed.
    Timeout(Duration),
    /// The output was larger than the limit. The process was killed.
    OutputTooLarge(usize),
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecError::Spawn(e) => write!(f, "cannot start the script: {e}"),
            ExecError::Timeout(d) => write!(
                f,
                "the script took longer than {} ms and was stopped",
                d.as_millis()
            ),
            ExecError::OutputTooLarge(n) => write!(
                f,
                "the script wrote more than {n} bytes of output and was stopped"
            ),
        }
    }
}

/// The bytes a reader thread has read so far, and whether the thread has ended.
#[derive(Default)]
struct Sink {
    bytes: Mutex<Vec<u8>>,
    done: AtomicBool,
}

/// Reads a stream into the sink. Keeps at most `cap` bytes. Sets `over` when more arrives and
/// `stop_on_over` is set.
fn read_capped(
    mut r: impl Read,
    cap: usize,
    over: Arc<AtomicBool>,
    stop_on_over: bool,
    sink: Arc<Sink>,
) {
    let mut buf = [0u8; 8192];
    let mut kept = 0usize;
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let room = cap.saturating_sub(kept);
                let take = n.min(room);
                sink.bytes
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .extend_from_slice(&buf[..take]);
                kept += take;
                // Without `stop_on_over` we keep draining, so the child never blocks on a full pipe.
                if n > room && stop_on_over {
                    over.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
    }
    sink.done.store(true, Ordering::SeqCst);
}

/// Waits until the reader has ended, but not longer than `until`. Returns what it read.
fn collect(sink: &Sink, until: Instant) -> Vec<u8> {
    while !sink.done.load(Ordering::SeqCst) && Instant::now() < until {
        thread::sleep(Duration::from_millis(5));
    }
    std::mem::take(&mut *sink.bytes.lock().unwrap_or_else(|e| e.into_inner()))
}

pub use kadakas_sys::process::{kill_tree, own_group};

/// Runs the command to the end, within the limits.
pub fn run(mut cmd: Command, stdin: &[u8], limits: Limits) -> Result<Output, ExecError> {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    own_group(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| ExecError::Spawn(e.to_string()))?;
    let mut sin = child.stdin.take().expect("piped");
    let input = stdin.to_vec();
    let writer = thread::spawn(move || {
        // A script that does not read its input closes the pipe. That is not an error.
        let _ = sin.write_all(&input);
    });
    let over = Arc::new(AtomicBool::new(false));
    let (out_sink, err_sink) = (Arc::new(Sink::default()), Arc::new(Sink::default()));
    {
        let (r, over, sink) = (
            child.stdout.take().expect("piped"),
            over.clone(),
            out_sink.clone(),
        );
        let cap = limits.max_stdout;
        thread::spawn(move || read_capped(r, cap, over, true, sink));
    }
    {
        let r = child.stderr.take().expect("piped");
        let (cap, over, sink) = (
            limits.max_stderr,
            Arc::new(AtomicBool::new(false)),
            err_sink.clone(),
        );
        thread::spawn(move || read_capped(r, cap, over, false, sink));
    }
    let start = Instant::now();
    let mut failure = None;
    let status = loop {
        // Kill the group before the leader is collected (see `kill_tree`).
        if kadakas_sys::process::ended_not_collected(&child) {
            kill_tree(&mut child);
        }
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(e) => {
                failure = Some(ExecError::Spawn(e.to_string()));
                break None;
            }
        }
        if over.load(Ordering::SeqCst) {
            failure = Some(ExecError::OutputTooLarge(limits.max_stdout));
            break None;
        }
        if start.elapsed() >= limits.timeout {
            failure = Some(ExecError::Timeout(limits.timeout));
            break None;
        }
        thread::sleep(Duration::from_millis(5));
    };
    // Always kill the group: a script may leave a background process that holds the pipes open.
    kill_tree(&mut child);
    if status.is_none() {
        let _ = child.wait();
    }
    let _ = writer.join();
    // The readers get a short time to finish. A process that left its group may keep a pipe open;
    // we do not wait for it.
    let until = Instant::now() + READER_GRACE;
    let stdout = collect(&out_sink, until);
    let stderr = collect(&err_sink, until);
    // The output may have filled up just before the process ended.
    if failure.is_none() && over.load(Ordering::SeqCst) {
        failure = Some(ExecError::OutputTooLarge(limits.max_stdout));
    }
    if let Some(f) = failure {
        return Err(f);
    }
    Ok(Output {
        stdout,
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        code: status.and_then(|s| s.code()),
    })
}

/// How long we wait for the output readers after the process has ended.
const READER_GRACE: Duration = Duration::from_millis(500);

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Command {
        let mut c = Command::new("sh");
        c.args(["-c", script]);
        c
    }

    fn limits(ms: u64, out: usize) -> Limits {
        Limits {
            timeout: Duration::from_millis(ms),
            max_stdout: out,
            max_stderr: 64,
        }
    }

    #[test]
    fn collects_output_and_code() {
        let o = run(sh("cat; echo err >&2; exit 3"), b"hello", limits(5000, 100)).unwrap();
        assert_eq!(o.stdout, b"hello");
        assert_eq!(o.stderr.trim(), "err");
        assert_eq!(o.code, Some(3));
    }

    #[test]
    fn a_slow_process_is_killed() {
        let e = run(sh("sleep 30"), b"", limits(150, 100)).unwrap_err();
        assert!(matches!(e, ExecError::Timeout(_)));
        assert!(e.to_string().contains("150 ms"));
    }

    #[test]
    fn a_process_that_ignores_its_input_is_fine() {
        let big = vec![b'x'; 1 << 20];
        let o = run(sh("echo ok"), &big, limits(5000, 100)).unwrap();
        assert_eq!(o.stdout, b"ok\n");
    }

    #[test]
    fn too_much_output_is_refused() {
        let e = run(sh("yes"), b"", limits(5000, 1000)).unwrap_err();
        assert_eq!(e, ExecError::OutputTooLarge(1000));
        assert!(e.to_string().contains("1000"));
    }

    #[test]
    fn stderr_is_cut_at_its_limit() {
        let o = run(
            sh("head -c 5000 /dev/zero | tr '\\0' e >&2"),
            b"",
            limits(5000, 100),
        )
        .unwrap();
        assert_eq!(o.stderr.len(), 64);
    }

    #[test]
    fn a_missing_program_is_a_spawn_error() {
        let e = run(Command::new("/no/such/program"), b"", Limits::default()).unwrap_err();
        assert!(matches!(e, ExecError::Spawn(_)));
        assert!(e.to_string().contains("cannot start"));
    }

    #[test]
    fn child_processes_die_with_the_timeout() {
        // The shell starts a child that keeps the output pipe open. The kill must reach it.
        let e = run(sh("sleep 30 & sleep 30"), b"", limits(150, 100)).unwrap_err();
        assert!(matches!(e, ExecError::Timeout(_)));
    }

    #[test]
    fn a_background_process_does_not_stall_the_run() {
        // The shell starts a long background job that keeps the output pipe open, then ends.
        let o = run(sh("sleep 30 & echo done"), b"", limits(1000, 100)).unwrap();
        assert_eq!(o.stdout, b"done\n");
    }

    #[test]
    fn a_process_that_leaves_its_group_cannot_hold_us() {
        // `setsid` puts the job in a new group; the kill cannot reach it, but the reader grace ends.
        let o = run(
            sh("(setsid sleep 3 >&2 &) ; echo ok"),
            b"",
            limits(1000, 100),
        );
        if let Ok(o) = o {
            assert_eq!(o.stdout, b"ok\n");
        }
    }
}
