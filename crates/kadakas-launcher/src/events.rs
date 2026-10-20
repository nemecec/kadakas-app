//! Documents and requests from the system while the launcher runs (PLAT-16).
//!
//! On macOS a double click on a `.kad` file is an Apple event, not a command line argument. The
//! launcher is the entry point and stays alive while the app runs, so the event goes to the
//! launcher, also when the app already runs. This file has the logic and no system code:
//!
//! - [`SystemEvents`] is the small interface to the system. The macOS code (`appkit.rs`) is one
//!   implementation; the tests have a scripted one.
//! - [`run_with_events`] runs the system loop on the main thread and the supervisor on a worker
//!   thread. Files that arrive before the loop is ready are started with the app. Files that
//!   arrive later go to the running app through the [`Inbox`].
//! - The [`Forwarder`] hands files to the running app, brings it to the front, and starts another
//!   app if the running one does not read its requests (an old version).

// The code is used on macOS only (`appkit.rs`); on other systems only the tests call it.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use kadakas_update::handshake::ENV_VIA;
use kadakas_update::inbox::{Delivery, Inbox, Running};
use kadakas_update::supervisor::{SpawnSpec, Spawner};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What the system loop tells the launcher.
pub trait EventHandler: Send + Sync {
    /// The loop is up. The files of a double click that started the launcher have arrived.
    fn ready(&self);
    /// The person opened documents (Finder, `open -a`).
    fn open(&self, files: Vec<PathBuf>);
    /// The person clicked the icon of the launcher (Finder, Launchpad) while it runs.
    fn reopen(&self);
}

/// The system side. Small on purpose: the glue is the only code that needs the system.
pub trait SystemEvents: Send + Sync {
    /// Runs the loop on the calling (main) thread and calls the handler. Returns after
    /// [`SystemEvents::stop`].
    fn run(&self, handler: Arc<dyn EventHandler>);
    /// Ends the loop. Callable from any thread. `success` is the exit status of the process.
    fn stop(&self, success: bool);
    /// Brings the program with this process id to the front.
    fn activate(&self, pid: u32);
}

/// True when the program runs from inside a macOS app bundle (`Kadakas Engine.app/Contents/MacOS/...`).
pub fn in_app_bundle(exe: &Path) -> bool {
    let parent = exe.parent();
    let contents = parent.and_then(Path::parent);
    parent.is_some_and(|p| p.file_name().is_some_and(|n| n == "MacOS"))
        && contents.is_some_and(|c| c.file_name().is_some_and(|n| n == "Contents"))
        && contents
            .and_then(Path::parent)
            .and_then(Path::extension)
            .is_some_and(|e| e == "app")
}

/// How long the launcher waits for the app to read a request, before it starts another app (only
/// an app without the alive stamp, see `Forwarder::open`).
pub const READ_WAIT: Duration = Duration::from_secs(10);

/// Hands files to the running app.
pub struct Forwarder {
    inbox: Arc<Inbox>,
    system: Arc<dyn SystemEvents>,
    spawner: Arc<dyn Spawner>,
    wait: Duration,
}

impl Forwarder {
    pub fn new(
        inbox: Arc<Inbox>,
        system: Arc<dyn SystemEvents>,
        spawner: Arc<dyn Spawner>,
    ) -> Arc<Forwarder> {
        Arc::new(Forwarder {
            inbox,
            system,
            spawner,
            wait: READ_WAIT,
        })
    }

    #[cfg(test)]
    fn with_wait(self: Arc<Self>, wait: Duration) -> Arc<Forwarder> {
        Arc::new(Forwarder {
            inbox: self.inbox.clone(),
            system: self.system.clone(),
            spawner: self.spawner.clone(),
            wait,
        })
    }

    /// Files from the system. The running app opens them (and comes to the front), or they wait
    /// for the next start.
    pub fn open(self: &Arc<Self>, files: Vec<OsString>) {
        let Delivery::Forwarded(run) = self.inbox.deliver(files) else {
            if self.inbox.unlinked() {
                // The app runs without a run record, so it cannot be told. There is no window
                // here to show it; the line is in the log of the launcher.
                eprintln!(
                    "kadakas-launcher: the running app has no run record; the file opens when it is closed"
                );
            }
            return;
        };
        if let Some(pid) = run.pid {
            self.system.activate(pid);
        }
        // An app that never reads requests (an old version, without the alive stamp) would leave
        // the person with a click that did nothing: after the wait, the request is taken back and
        // another app starts with the files. An app with the alive stamp is slow, not gone: a
        // file dialog blocks its window loop, a big file loads, the window is minimised. Then the
        // launcher waits as long as it lives. If the app ends, the supervisor takes the request
        // back and starts the app again with the file (`Inbox::end_run`).
        let me = self.clone();
        std::thread::spawn(move || {
            let deadline = Instant::now() + me.wait;
            while me.inbox.unread(&run.id) {
                if me.inbox.running().map(|r| r.id) != Some(run.id.clone()) {
                    return;
                }
                if Instant::now() >= deadline && !me.inbox.alive_stamp(&run.id) {
                    me.start_another(&run);
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
    }

    /// Starts another app with the files that run `run` did not read. It is not supervised (no run
    /// variables, no crash mark), but it is waited for, so that it never stays a zombie.
    fn start_another(&self, run: &Running) {
        let files = self.inbox.reclaim(&run.id);
        if files.is_empty() {
            return;
        }
        let spec = SpawnSpec {
            files,
            env: vec![(ENV_VIA.to_string(), "1".to_string())],
            ..run.spec.clone()
        };
        match self.spawner.spawn(&spec) {
            Ok(mut child) => {
                while matches!(child.try_wait(), Ok(None)) {
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
            Err(e) => eprintln!("kadakas-launcher: could not open the documents: {e}"),
        }
    }

    /// A click on the icon: brings the running app to the front.
    pub fn reopen(&self) {
        if let Some(pid) = self.inbox.running().and_then(|r| r.pid) {
            self.system.activate(pid);
        }
    }
}

struct Handler {
    forwarder: Arc<Forwarder>,
    ready: Mutex<Option<Sender<()>>>,
}

impl EventHandler for Handler {
    fn ready(&self) {
        if let Some(tx) = self.ready.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = tx.send(());
        }
    }

    fn open(&self, files: Vec<PathBuf>) {
        self.forwarder
            .open(files.into_iter().map(PathBuf::into_os_string).collect());
    }

    fn reopen(&self) {
        self.forwarder.reopen();
    }
}

/// Runs the system loop on this thread and `work` on a worker thread. `work` gets the files to
/// start with: `args` first, then the files that the system sent before the loop was ready
/// (the double click that started the launcher). It returns when the app is done; the loop then
/// stops. The loop gets `ready_timeout` to become ready; if it does not, the work starts anyway.
/// Returns whether `work` succeeded.
pub fn run_with_events(
    system: Arc<dyn SystemEvents>,
    forwarder: Arc<Forwarder>,
    args: Vec<OsString>,
    ready_timeout: Duration,
    work: impl FnOnce(Vec<OsString>) -> bool + Send + 'static,
) -> bool {
    let (ready_tx, ready_rx) = channel();
    let (done_tx, done_rx) = channel();
    let handler = Arc::new(Handler {
        forwarder: forwarder.clone(),
        ready: Mutex::new(Some(ready_tx)),
    });
    let stopper = system.clone();
    std::thread::spawn(move || {
        let _ = ready_rx.recv_timeout(ready_timeout);
        let mut files = args;
        for f in forwarder.inbox.take_pending() {
            if !files.contains(&f) {
                files.push(f);
            }
        }
        let ok = work(files);
        let _ = done_tx.send(ok);
        stopper.stop(ok);
    });
    system.run(handler);
    done_rx.recv().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_update::flavors::Flavor;
    use kadakas_update::inbox::Running;
    use kadakas_update::supervisor::Process;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::Receiver;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "kadakas-launcher-events-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn files(names: &[&str]) -> Vec<OsString> {
        names.iter().map(OsString::from).collect()
    }

    fn running(id: &str, pid: Option<u32>) -> Running {
        Running {
            id: id.into(),
            pid,
            spec: SpawnSpec {
                dir: PathBuf::from("/v/1.0.0"),
                flavor: Flavor {
                    id: "kadakas-app".into(),
                    name: "Kadakas".into(),
                    exe: "Kadakas Engine.app/Contents/MacOS/kadakas-app".into(),
                },
                profile: "p".into(),
                files: files(&["/old.kad"]),
                env: vec![("KADAKAS_RUN_ID".into(), id.into())],
            },
        }
    }

    /// A scripted system: it sends events when the loop runs, and records what the launcher does.
    struct FakeSystem {
        /// Events sent before the loop is ready (the double click that started the launcher).
        before: Vec<PathBuf>,
        /// Whether the loop becomes ready at all.
        becomes_ready: bool,
        /// Events sent while the loop runs. An empty list is a click on the icon.
        after: Mutex<Option<Receiver<Vec<PathBuf>>>>,
        activated: Mutex<Vec<u32>>,
        stopped: Mutex<Option<bool>>,
        done: AtomicBool,
        spawned: Mutex<Vec<SpawnSpec>>,
        /// How often the supervisor-less launcher looked at the second app.
        looks: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl FakeSystem {
        fn new(before: &[&str], becomes_ready: bool) -> Arc<FakeSystem> {
            Arc::new(FakeSystem {
                before: before.iter().map(PathBuf::from).collect(),
                becomes_ready,
                after: Mutex::new(None),
                activated: Mutex::new(Vec::new()),
                stopped: Mutex::new(None),
                done: AtomicBool::new(false),
                spawned: Mutex::new(Vec::new()),
                looks: Arc::default(),
            })
        }
    }

    impl SystemEvents for FakeSystem {
        fn run(&self, handler: Arc<dyn EventHandler>) {
            if !self.before.is_empty() {
                handler.open(self.before.clone());
            }
            if self.becomes_ready {
                handler.ready();
            }
            let rx = self.after.lock().unwrap().take();
            // The loop: events come in until stop is called.
            while !self.done.load(Ordering::SeqCst) {
                match rx
                    .as_ref()
                    .map(|r| r.recv_timeout(Duration::from_millis(5)))
                {
                    Some(Ok(f)) if f.is_empty() => handler.reopen(),
                    Some(Ok(f)) => handler.open(f),
                    _ => std::thread::sleep(Duration::from_millis(2)),
                }
            }
        }

        fn stop(&self, success: bool) {
            *self.stopped.lock().unwrap() = Some(success);
            self.done.store(true, Ordering::SeqCst);
        }

        fn activate(&self, pid: u32) {
            self.activated.lock().unwrap().push(pid);
        }
    }

    /// A program that runs for three looks.
    struct FakeProcess(Arc<std::sync::atomic::AtomicUsize>);
    impl Process for FakeProcess {
        fn try_wait(&mut self) -> std::io::Result<Option<bool>> {
            let n = self.0.fetch_add(1, Ordering::SeqCst) + 1;
            Ok((n >= 3).then_some(true))
        }
    }

    impl Spawner for FakeSystem {
        fn spawn(&self, spec: &SpawnSpec) -> kadakas_update::Result<Box<dyn Process>> {
            self.spawned.lock().unwrap().push(spec.clone());
            Ok(Box::new(FakeProcess(self.looks.clone())))
        }
    }

    fn forwarder(root: &Path, sys: &Arc<FakeSystem>, wait: Duration) -> Arc<Forwarder> {
        Forwarder::new(Inbox::new(root), sys.clone(), sys.clone()).with_wait(wait)
    }

    #[test]
    fn the_bundle_path_is_recognised() {
        assert!(in_app_bundle(Path::new(
            "/Applications/Kadakas Engine.app/Contents/MacOS/kadakas-launcher"
        )));
        assert!(in_app_bundle(Path::new(
            "/x/Other Name.app/Contents/MacOS/k"
        )));
        for no in [
            "/target/debug/kadakas-launcher",
            "/Applications/Kadakas Engine.app/Contents/Resources/k",
            "/Applications/Kadakas/Contents/MacOS/k",
            "/Contents/MacOS/k",
            "k",
        ] {
            assert!(!in_app_bundle(Path::new(no)), "{no}");
        }
    }

    #[test]
    fn files_of_the_double_click_that_started_the_launcher_start_the_app() {
        let d = tmp("start");
        let sys = FakeSystem::new(&["/double.kad", "/arg.kad"], true);
        let fwd = forwarder(&d, &sys, READ_WAIT);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        // Files named on the command line come first, then the event's, and none twice.
        let ok = run_with_events(
            sys.clone(),
            fwd,
            files(&["/arg.kad"]),
            Duration::from_secs(5),
            move |f| {
                *seen2.lock().unwrap() = f;
                true
            },
        );
        assert!(ok);
        assert_eq!(*seen.lock().unwrap(), files(&["/arg.kad", "/double.kad"]));
        assert_eq!(*sys.stopped.lock().unwrap(), Some(true));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_loop_that_is_never_ready_does_not_hold_the_app_back() {
        let d = tmp("noready");
        let sys = FakeSystem::new(&[], false);
        let fwd = forwarder(&d, &sys, READ_WAIT);
        let started = Instant::now();
        let ok = run_with_events(
            sys.clone(),
            fwd,
            files(&["/a.kad"]),
            Duration::from_millis(30),
            |f| f == files(&["/a.kad"]),
        );
        assert!(ok);
        assert!(started.elapsed() < Duration::from_secs(4));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_failed_start_is_reported_and_stops_the_loop() {
        let d = tmp("fail");
        let sys = FakeSystem::new(&[], true);
        let fwd = forwarder(&d, &sys, READ_WAIT);
        let ok = run_with_events(sys.clone(), fwd, vec![], Duration::from_secs(5), |_| false);
        assert!(!ok);
        assert_eq!(*sys.stopped.lock().unwrap(), Some(false));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_double_click_while_the_app_runs_is_forwarded_and_the_app_comes_to_the_front() {
        let d = tmp("forward");
        let sys = FakeSystem::new(&[], true);
        let fwd = forwarder(&d, &sys, READ_WAIT);
        let inbox = fwd.inbox.clone();
        let (tx, rx) = channel();
        *sys.after.lock().unwrap() = Some(rx);
        let probe = sys.clone();
        let root = d.clone();
        let ok = run_with_events(
            sys.clone(),
            fwd,
            vec![],
            Duration::from_secs(5),
            move |_| {
                // The app runs. A double click arrives, a click on the icon follows.
                inbox.set_running(running("1a-2b", Some(321)));
                tx.send(vec![PathBuf::from("/second.kad")]).unwrap();
                tx.send(Vec::new()).unwrap();
                let t = Instant::now();
                while probe.activated.lock().unwrap().len() < 2
                    && t.elapsed() < Duration::from_secs(5)
                {
                    std::thread::sleep(Duration::from_millis(5));
                }
                // The app side: it reads the request.
                let read = kadakas_update::handshake::take_open(&root, "1a-2b");
                read == [PathBuf::from("/second.kad")]
            },
        );
        assert!(ok, "the app got the file");
        assert_eq!(*sys.activated.lock().unwrap(), [321, 321]);
        assert!(sys.spawned.lock().unwrap().is_empty(), "no second app");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_request_that_is_not_read_starts_another_app_with_the_files() {
        let d = tmp("unread");
        let sys = FakeSystem::new(&[], true);
        let fwd = forwarder(&d, &sys, Duration::from_millis(60));
        fwd.inbox.set_running(running("1a", None));
        fwd.open(files(&["/third.kad"]));
        let t = Instant::now();
        while sys.spawned.lock().unwrap().is_empty() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let spawned = sys.spawned.lock().unwrap().clone();
        assert_eq!(spawned.len(), 1);
        // Same version and flavor, the new files only, and no run variables: it is not supervised.
        assert_eq!(spawned[0].files, files(&["/third.kad"]));
        // Not supervised (no run variables), but marked as started by the launcher.
        assert_eq!(spawned[0].env, [(ENV_VIA.to_string(), "1".to_string())]);
        assert_eq!(spawned[0].dir, PathBuf::from("/v/1.0.0"));
        assert_eq!(spawned[0].profile, "p");
        // The request was taken back: nothing is left.
        assert!(!fwd.inbox.unread("1a"));
        // The second app is waited for until it ends (no zombie): the launcher looks again and
        // again, 250 ms apart, until the program says it is done.
        let t = Instant::now();
        while sys.looks.load(Ordering::SeqCst) < 3 && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(sys.looks.load(Ordering::SeqCst), 3);
        // No pid: nothing to bring to the front.
        assert!(sys.activated.lock().unwrap().is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// The app has the alive stamp: it is slow (a file dialog), not gone. The launcher waits.
    #[test]
    fn a_slow_app_with_the_alive_stamp_gets_no_second_app() {
        let d = tmp("slow");
        let sys = FakeSystem::new(&[], true);
        let fwd = forwarder(&d, &sys, Duration::from_millis(60));
        fwd.inbox.set_running(running("1a-2b", Some(5)));
        let link = kadakas_update::handshake::AppLink::from_env(&|k| match k {
            kadakas_update::handshake::ENV_ROOT => Some(d.to_string_lossy().into_owned()),
            kadakas_update::handshake::ENV_RUN => Some("1a-2b".into()),
            _ => None,
        })
        .unwrap();
        let _beat = link.start_heartbeat();
        fwd.open(files(&["/slow.kad"]));
        // Well past the wait: still no second app, and the request waits for the app.
        std::thread::sleep(Duration::from_millis(400));
        assert!(sys.spawned.lock().unwrap().is_empty());
        assert!(fwd.inbox.unread("1a-2b"));
        // The app comes back to its loop and reads it.
        assert_eq!(link.take_open(), [PathBuf::from("/slow.kad")]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// The app ends before it reads: the supervisor takes the request back, not this thread.
    #[test]
    fn when_the_app_ends_the_waiting_thread_leaves_the_request_to_the_supervisor() {
        let d = tmp("ended");
        let sys = FakeSystem::new(&[], true);
        let fwd = forwarder(&d, &sys, Duration::from_millis(30));
        fwd.inbox.set_running(running("1a", None));
        fwd.open(files(&["/x.kad"]));
        let left = fwd.inbox.end_run();
        assert_eq!(left, files(&["/x.kad"]));
        std::thread::sleep(Duration::from_millis(200));
        assert!(sys.spawned.lock().unwrap().is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_request_that_is_read_in_time_starts_nothing() {
        let d = tmp("read");
        let sys = FakeSystem::new(&[], true);
        let fwd = forwarder(&d, &sys, Duration::from_millis(300));
        fwd.inbox.set_running(running("1a", Some(9)));
        fwd.open(files(&["/third.kad"]));
        assert_eq!(
            kadakas_update::handshake::take_open(&d, "1a"),
            [PathBuf::from("/third.kad")]
        );
        std::thread::sleep(Duration::from_millis(450));
        assert!(sys.spawned.lock().unwrap().is_empty());
        assert_eq!(*sys.activated.lock().unwrap(), [9]);
        // A click on the icon with no app running does nothing.
        fwd.inbox.end_run();
        fwd.reopen();
        assert_eq!(sys.activated.lock().unwrap().len(), 1);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn files_wait_when_no_app_runs() {
        let d = tmp("wait");
        let sys = FakeSystem::new(&[], true);
        let fwd = forwarder(&d, &sys, READ_WAIT);
        fwd.open(files(&["/a.kad"]));
        assert_eq!(fwd.inbox.take_pending(), files(&["/a.kad"]));
        assert!(sys.activated.lock().unwrap().is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
