//! The script host: runs `.py` files that the user starts, and keeps their console output.
//!
//! A script gets the address and the token of the live session in environment variables:
//! `KADAKAS_ADDR` (`127.0.0.1:port`) and `KADAKAS_TOKEN`. The Python package `kadakas`
//! reads them with `kadakas.connect()`. The host never starts a script by itself.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use crate::exec::{kill_tree, own_group};
use crate::python::{self, Interpreter};

/// Most lines the console keeps for one run. The oldest lines go first.
pub const MAX_LINES: usize = 5_000;
/// Longest console line, in characters. The rest is cut.
pub const MAX_LINE_CHARS: usize = 4_000;
/// Most scripts that run at the same time.
pub const MAX_RUNNING: usize = 4;
/// Most finished runs that the host remembers.
pub const MAX_RUNS: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    /// Standard output of the script.
    Out,
    /// Standard error of the script (errors and tracebacks).
    Err,
    /// A message from the host, for example "finished".
    Info,
}

impl Stream {
    pub fn name(self) -> &'static str {
        match self {
            Stream::Out => "out",
            Stream::Err => "err",
            Stream::Info => "info",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// Number of the line in this run, from 0. It keeps counting when old lines are dropped.
    pub n: usize,
    pub stream: Stream,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    /// The script ended with this exit code.
    Exited(i32),
    /// The user stopped the script.
    Stopped,
    /// The script could not start, or a signal ended it.
    Failed(String),
}

impl Status {
    pub fn is_running(&self) -> bool {
        *self == Status::Running
    }

    pub fn name(&self) -> &'static str {
        match self {
            Status::Running => "running",
            Status::Exited(0) => "finished",
            Status::Exited(_) => "failed",
            Status::Stopped => "stopped",
            Status::Failed(_) => "failed",
        }
    }
}

/// What to run.
#[derive(Clone, Debug)]
pub struct RunSpec {
    pub script: PathBuf,
    pub args: Vec<String>,
    /// Address and token of the live session, for the script.
    pub endpoint: Option<(String, String)>,
    /// The interpreter. `None`: find one with [`python::discover`].
    pub interpreter: Option<Interpreter>,
}

struct Run {
    id: u64,
    name: String,
    started: Instant,
    lines: Vec<Line>,
    total: usize,
    status: Status,
    child: Option<Child>,
    stop_requested: bool,
}

#[derive(Default)]
struct State {
    next_id: u64,
    runs: Vec<Run>,
}

/// A summary of a run for lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunInfo {
    pub id: u64,
    pub name: String,
    pub status: Status,
    pub seconds: u64,
}

/// Runs scripts and keeps their console output. Clones share the same runs.
#[derive(Clone, Default)]
pub struct ScriptHost {
    state: Arc<Mutex<State>>,
}

/// How long the host waits for the output readers after the script has ended.
const READER_GRACE: std::time::Duration = std::time::Duration::from_millis(500);

fn lock(s: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

fn push(state: &Mutex<State>, id: u64, stream: Stream, text: &str) {
    let mut st = lock(state);
    let Some(run) = st.runs.iter_mut().find(|r| r.id == id) else {
        return;
    };
    let text: String = text.chars().take(MAX_LINE_CHARS).collect();
    run.lines.push(Line {
        n: run.total,
        stream,
        text,
    });
    run.total += 1;
    if run.lines.len() > MAX_LINES {
        run.lines.remove(0);
    }
}

fn pump(
    r: impl std::io::Read + Send + 'static,
    state: Arc<Mutex<State>>,
    id: u64,
    s: Stream,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(r).split(b'\n').map_while(Result::ok) {
            let text = String::from_utf8_lossy(&line);
            push(&state, id, s, text.trim_end_matches('\r'));
        }
    })
}

impl ScriptHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a script and returns the ID of the run.
    pub fn start(&self, spec: RunSpec) -> Result<u64, String> {
        if !spec.script.is_file() {
            return Err(format!("{} is not a file", spec.script.display()));
        }
        let interpreter = spec
            .interpreter
            .clone()
            .or_else(python::discover)
            .ok_or("no Python found: install python3, or uv, or set KADAKAS_PYTHON")?;
        let mut cmd = interpreter.command();
        cmd.arg("-u").arg(&spec.script).args(&spec.args);
        if let Some(dir) = python::package_dir() {
            cmd.env("PYTHONPATH", python::python_path_with(&dir));
        }
        cmd.env("PYTHONUNBUFFERED", "1");
        // No secret of this program reaches the script, except the token of the endpoint that it
        // is given on purpose (set below).
        kadakas_sys::process::scrub_secrets(&mut cmd);
        match &spec.endpoint {
            Some((addr, token)) => {
                cmd.env("KADAKAS_ADDR", addr).env("KADAKAS_TOKEN", token);
            }
            None => {
                cmd.env_remove("KADAKAS_ADDR").env_remove("KADAKAS_TOKEN");
            }
        }
        if let Some(dir) = spec.script.parent().filter(|d| d.is_dir()) {
            cmd.current_dir(dir);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        own_group(&mut cmd);
        let name = spec
            .script
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let id = {
            let mut st = lock(&self.state);
            if st.runs.iter().filter(|r| r.status.is_running()).count() >= MAX_RUNNING {
                return Err(format!("{MAX_RUNNING} scripts already run. Stop one first"));
            }
            st.next_id += 1;
            let id = st.next_id;
            st.runs.push(Run {
                id,
                name,
                started: Instant::now(),
                lines: vec![],
                total: 0,
                status: Status::Running,
                child: None,
                stop_requested: false,
            });
            // Forget the oldest finished runs.
            while st.runs.len() > MAX_RUNS {
                match st.runs.iter().position(|r| !r.status.is_running()) {
                    Some(i) => {
                        st.runs.remove(i);
                    }
                    None => break,
                }
            }
            id
        };
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("cannot start {}: {e}", interpreter.describe());
                self.finish(id, Status::Failed(msg.clone()));
                push(&self.state, id, Stream::Err, &msg);
                return Ok(id);
            }
        };
        push(
            &self.state,
            id,
            Stream::Info,
            &format!(
                "running {} with {}",
                spec.script.display(),
                interpreter.describe()
            ),
        );
        let pumps = [
            pump(
                child.stdout.take().expect("piped"),
                self.state.clone(),
                id,
                Stream::Out,
            ),
            pump(
                child.stderr.take().expect("piped"),
                self.state.clone(),
                id,
                Stream::Err,
            ),
        ];
        {
            let mut st = lock(&self.state);
            if let Some(r) = st.runs.iter_mut().find(|r| r.id == id) {
                r.child = Some(child);
            }
        }
        let host = self.clone();
        thread::spawn(move || {
            // Poll for the end: `stop` needs the lock on the child while we wait.
            let status = loop {
                {
                    let mut st = lock(&host.state);
                    let Some(r) = st.runs.iter_mut().find(|r| r.id == id) else {
                        return;
                    };
                    let stop_requested = r.stop_requested;
                    // Kill the group before the leader is collected: a process that the script
                    // left behind would hold the output pipes open.
                    if let Some(c) = r.child.as_mut()
                        && kadakas_sys::process::ended_not_collected(c)
                    {
                        kill_tree(c);
                    }
                    if let Some(Ok(Some(s))) = r.child.as_mut().map(|c| c.try_wait()) {
                        break if stop_requested {
                            Status::Stopped
                        } else {
                            s.code().map_or_else(
                                || Status::Failed("ended by a signal".into()),
                                Status::Exited,
                            )
                        };
                    }
                }
                thread::sleep(std::time::Duration::from_millis(20));
            };
            // The readers get a short time to finish. A process that left its group may keep a
            // pipe open; we do not wait for it.
            let until = Instant::now() + READER_GRACE;
            for p in pumps {
                while !p.is_finished() && Instant::now() < until {
                    thread::sleep(std::time::Duration::from_millis(5));
                }
                if p.is_finished() {
                    let _ = p.join();
                }
            }
            let note = match &status {
                Status::Exited(0) => "finished".to_string(),
                Status::Exited(c) => format!("failed with exit code {c}"),
                Status::Stopped => "stopped".to_string(),
                Status::Failed(m) => m.clone(),
                Status::Running => String::new(),
            };
            push(&host.state, id, Stream::Info, &note);
            host.finish(id, status);
        });
        Ok(id)
    }

    fn finish(&self, id: u64, status: Status) {
        let mut st = lock(&self.state);
        if let Some(r) = st.runs.iter_mut().find(|r| r.id == id) {
            r.status = status;
            r.child = None;
        }
    }

    /// Stops a running script (and the processes it started). Returns false if there is no such running script.
    pub fn stop(&self, id: u64) -> bool {
        let mut st = lock(&self.state);
        let Some(r) = st
            .runs
            .iter_mut()
            .find(|r| r.id == id && r.status.is_running())
        else {
            return false;
        };
        r.stop_requested = true;
        if let Some(child) = r.child.as_mut() {
            kill_tree(child);
        }
        true
    }

    /// Stops all scripts, for example when the app closes.
    pub fn stop_all(&self) {
        let ids: Vec<u64> = lock(&self.state).runs.iter().map(|r| r.id).collect();
        ids.into_iter().for_each(|id| {
            self.stop(id);
        });
    }

    /// The console lines with number `since` or higher, and the status. `None` for an unknown run.
    pub fn poll(&self, id: u64, since: usize) -> Option<(Vec<Line>, Status)> {
        let st = lock(&self.state);
        let r = st.runs.iter().find(|r| r.id == id)?;
        let lines = r.lines.iter().filter(|l| l.n >= since).cloned().collect();
        Some((lines, r.status.clone()))
    }

    /// All runs, oldest first.
    pub fn list(&self) -> Vec<RunInfo> {
        lock(&self.state)
            .runs
            .iter()
            .map(|r| RunInfo {
                id: r.id,
                name: r.name.clone(),
                status: r.status.clone(),
                seconds: r.started.elapsed().as_secs(),
            })
            .collect()
    }

    /// Removes finished runs from the list.
    pub fn clear_finished(&self) {
        lock(&self.state).runs.retain(|r| r.status.is_running());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn sh() -> Interpreter {
        Interpreter {
            program: PathBuf::from("sh"),
            prefix: vec![],
        }
    }

    /// A shell script in a temp folder. `sh -u file` works: -u is a shell option.
    fn script(name: &str, body: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kadakas-host-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("run.sh");
        std::fs::write(&p, body).unwrap();
        p
    }

    fn spec(p: PathBuf) -> RunSpec {
        RunSpec {
            script: p,
            args: vec![],
            endpoint: Some(("127.0.0.1:1".into(), "secret".into())),
            interpreter: Some(sh()),
        }
    }

    fn wait(h: &ScriptHost, id: u64) -> (Vec<Line>, Status) {
        for _ in 0..500 {
            let (lines, st) = h.poll(id, 0).unwrap();
            if !st.is_running() {
                return (lines, st);
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "script {id} did not end: {:?} {:?}",
            h.poll(id, 0),
            h.list()
        );
    }

    #[test]
    fn output_error_output_and_environment_reach_the_console() {
        let h = ScriptHost::new();
        let p = script(
            "env",
            "echo \"addr=$KADAKAS_ADDR token=$KADAKAS_TOKEN\"\necho oops >&2\nexit 2\n",
        );
        let id = h.start(spec(p)).unwrap();
        let (lines, st) = wait(&h, id);
        assert_eq!(st, Status::Exited(2));
        assert_eq!(st.name(), "failed");
        assert!(
            lines
                .iter()
                .any(|l| l.stream == Stream::Out && l.text == "addr=127.0.0.1:1 token=secret")
        );
        assert!(
            lines
                .iter()
                .any(|l| l.stream == Stream::Err && l.text == "oops")
        );
        assert!(lines.last().unwrap().text.contains("exit code 2"));
        assert_eq!(Stream::Info.name(), "info");
        assert_eq!(h.list()[0].name, "run.sh");
        // `since` skips old lines.
        let (rest, _) = h.poll(id, lines.len() - 1).unwrap();
        assert_eq!(rest.len(), 1);
    }

    #[test]
    fn a_running_script_can_be_stopped() {
        let h = ScriptHost::new();
        let id = h
            .start(spec(script("stop", "echo start\nsleep 30\n")))
            .unwrap();
        assert!(h.list()[0].status.is_running());
        assert!(h.stop(id));
        let (_, st) = wait(&h, id);
        assert_eq!(st, Status::Stopped);
        assert!(!h.stop(id), "a stopped script cannot be stopped again");
        h.clear_finished();
        assert!(h.list().is_empty());
        assert!(h.poll(id, 0).is_none());
    }

    #[test]
    fn missing_file_and_missing_program_are_reported() {
        let h = ScriptHost::new();
        assert!(
            h.start(spec(PathBuf::from("/no/such/file.py")))
                .unwrap_err()
                .contains("not a file")
        );
        let mut s = spec(script("noprog", "echo x\n"));
        s.interpreter = Some(Interpreter {
            program: PathBuf::from("/no/such/python"),
            prefix: vec![],
        });
        let id = h.start(s).unwrap();
        let (lines, st) = wait(&h, id);
        assert!(matches!(st, Status::Failed(_)));
        assert!(lines[0].text.contains("cannot start"));
    }

    #[test]
    fn long_lines_are_cut_and_the_number_of_running_scripts_is_limited() {
        let h = ScriptHost::new();
        let long = "x".repeat(MAX_LINE_CHARS * 2);
        let id = h
            .start(spec(script("long", &format!("echo {long}\n"))))
            .unwrap();
        let (lines, _) = wait(&h, id);
        assert!(lines.iter().any(|l| l.text.len() == MAX_LINE_CHARS));
        let sleeper = script("sleepy", "sleep 30\n");
        let ids: Vec<u64> = (0..MAX_RUNNING)
            .map(|_| h.start(spec(sleeper.clone())).unwrap())
            .collect();
        let e = h.start(spec(sleeper)).unwrap_err();
        assert!(e.contains("already run"));
        h.stop_all();
        for id in ids {
            wait(&h, id);
        }
    }

    #[test]
    fn the_console_keeps_the_newest_lines() {
        let h = ScriptHost::new();
        let body = format!(
            "i=0\nwhile [ $i -lt {} ]; do echo $i; i=$((i+1)); done\n",
            MAX_LINES + 100
        );
        let id = h.start(spec(script("many", &body))).unwrap();
        let (lines, _) = wait(&h, id);
        assert_eq!(lines.len(), MAX_LINES);
        assert!(lines.last().unwrap().n > MAX_LINES);
    }
}
