//! The headless launcher (PLAT-16): the entry point of shortcuts, the Dock or taskbar icon and
//! `.kad` files.
//!
//! 1. Settle the runs that ended while no launcher was watching (crash marks).
//! 2. Choose the version (`choose`) and **start it at once**. Nothing waits for the network.
//! 3. While the app runs, check the manifest in a background thread. The result goes to the app
//!    as a status file (`handshake`). By mode, the launcher also downloads (Auto-update and
//!    Auto-download) or waits for the app to ask (Notify only).
//! 4. When the app ends, read the clean-exit mark. If the app asked for a restart and ended
//!    clean, choose again and start the new version with the same documents. Else stop.
//!
//! The supervisor does not know what a window is. The program start is the trait [`Spawner`], so
//! the whole flow is tested with scripted programs.

use crate::choose::{self, Choice};
use crate::error::{Result, UpdateError};
use crate::flavors;
use crate::handshake::{self, ENV_ROOT, ENV_RUN, RunStatus, SkipReason, Skipped, UpdateState};
use crate::launch;
use crate::manifest::Channel;
use crate::runs::RunGuard;
use crate::settings::{Settings, UpdateMode};
use crate::store::{Slot, Store};
use crate::updater::{UpdateStatus, Updater};
use crate::version::Version;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

/// A program that runs.
pub trait Process: Send {
    /// `Ok(Some(success))` when the program has ended, `Ok(None)` while it runs.
    fn try_wait(&mut self) -> std::io::Result<Option<bool>>;
}

/// What to start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnSpec {
    pub dir: PathBuf,
    pub flavor: flavors::Flavor,
    pub profile: String,
    /// The documents to open.
    pub files: Vec<OsString>,
    /// Variables for the program.
    pub env: Vec<(String, String)>,
}

/// Starts programs.
pub trait Spawner: Send + Sync {
    fn spawn(&self, spec: &SpawnSpec) -> Result<Box<dyn Process>>;
}

struct SystemProcess(std::process::Child);

impl Process for SystemProcess {
    fn try_wait(&mut self) -> std::io::Result<Option<bool>> {
        Ok(self.0.try_wait()?.map(|s| s.success()))
    }
}

/// Starts real programs.
pub struct SystemSpawner;

impl Spawner for SystemSpawner {
    fn spawn(&self, spec: &SpawnSpec) -> Result<Box<dyn Process>> {
        let mut cmd = launch::command_with(
            &spec.dir,
            &spec.flavor,
            &spec.profile,
            &spec.files,
            &spec.env,
        )?;
        Ok(Box::new(SystemProcess(cmd.spawn()?)))
    }
}

/// How often the supervisor looks.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// Between two looks at the program and at the files of the app.
    pub poll: Duration,
    /// Between two checks of the manifest.
    pub recheck: Duration,
    /// After a failed check.
    pub retry: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            poll: Duration::from_millis(250),
            recheck: Duration::from_secs(6 * 3600),
            retry: Duration::from_secs(15 * 60),
        }
    }
}

/// Makes the updater for the settings that count now (the channel decides the site).
pub type UpdaterFactory = Arc<dyn Fn(&Settings) -> Updater + Send + Sync>;

/// One start of the app, for the report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunReport {
    pub slot: Slot,
    pub version: Version,
    pub files: Vec<OsString>,
    /// The app wrote the clean-exit mark.
    pub clean: bool,
    pub skipped: Vec<Skipped>,
}

pub struct Supervisor {
    root: PathBuf,
    make_updater: UpdaterFactory,
    spawner: Arc<dyn Spawner>,
    timing: Timing,
}

/// The newest version that restarting would start and that is newer than the running one:
/// installed, allowed by the channel, not skipped. This is "ready".
pub fn ready_version(store: &Store, channel: Channel, running: &Version) -> Option<Version> {
    let crashed = store.crashed_slots();
    let rolled = store.rolled_back_slots();
    store
        .list()
        .into_iter()
        .filter(|i| choose::eligible(&i.meta.label, channel))
        .filter(|i| !crashed.contains(&i.slot) && !rolled.contains(&i.slot))
        .map(|i| i.version)
        .filter(|v| v > running)
        .max()
}

/// One round of the background work: look at the site, and download when the mode says so.
/// `progress` gets the states on the way ("downloading").
fn cycle(
    updater: &Updater,
    settings: &Settings,
    running: &Version,
    force_download: bool,
    progress: &dyn Fn(UpdateState),
) -> UpdateState {
    let channel = settings.channel;
    let ready = |store: &Store| {
        ready_version(store, channel, running).map(|v| UpdateState::Ready {
            version: v.to_string(),
        })
    };
    if !updater.can_check() {
        // No site (the channel local without a local site): only what is installed counts.
        return ready(updater.store()).unwrap_or(UpdateState::UpToDate);
    }
    match updater.check(channel) {
        Err(e) => ready(updater.store()).unwrap_or(UpdateState::Failed {
            message: e.to_string(),
        }),
        Ok(UpdateStatus::UpToDate { .. } | UpdateStatus::Older { .. }) => {
            ready(updater.store()).unwrap_or(UpdateState::UpToDate)
        }
        Ok(UpdateStatus::Available(offer)) => {
            if settings.mode.downloads_by_itself() || force_download {
                progress(UpdateState::Downloading {
                    version: offer.version.to_string(),
                });
                match updater.stage(channel) {
                    Ok(_) => ready(updater.store()).unwrap_or(UpdateState::UpToDate),
                    Err(e) => UpdateState::Failed {
                        message: e.to_string(),
                    },
                }
            } else {
                UpdateState::Available {
                    version: offer.version.to_string(),
                    size: offer.size,
                }
            }
        }
    }
}

enum Msg {
    State(UpdateState),
    Done(UpdateState),
}

/// The background work of one run.
struct Worker {
    rx: Receiver<Msg>,
}

impl Supervisor {
    /// `root` is the install folder.
    pub fn new(
        root: PathBuf,
        make_updater: UpdaterFactory,
        spawner: Arc<dyn Spawner>,
    ) -> Supervisor {
        Supervisor {
            root,
            make_updater,
            spawner,
            timing: Timing::default(),
        }
    }

    pub fn with_timing(mut self, timing: Timing) -> Supervisor {
        self.timing = timing;
        self
    }

    /// Starts the app with `files` and watches it until it ends. A restart request starts it
    /// again (the newest version, the documents of the request). Returns one report per start.
    pub fn run(&self, files: Vec<OsString>) -> Result<Vec<RunReport>> {
        let settings = Settings::load(&self.root);
        let updater = (self.make_updater)(&settings);
        updater.store().recover();
        updater.store().settle_runs();
        let mut reports = Vec::new();
        let mut files = files;
        loop {
            let settings = Settings::load(&self.root);
            let updater = (self.make_updater)(&settings);
            updater.store().set_keep_older(settings.keep_older);
            let _ = updater.store().prune();
            let (mut child, guard, choice, base) = self.start(&updater, &settings, &files)?;
            let running = choice.active.version.clone();
            let (request, clean) = match guard {
                Some(guard) => {
                    self.watch(child.as_mut(), &guard, &running, base);
                    let id = guard.id().to_string();
                    let request = handshake::take_restart(&self.root, &id);
                    (request, guard.finish(updater.store()).clean)
                }
                // No record could be written: the app runs without the update features, and
                // nothing is known about how it ends. No restart.
                None => {
                    while matches!(child.try_wait(), Ok(None)) {
                        std::thread::sleep(self.timing.poll);
                    }
                    (None, false)
                }
            };
            reports.push(RunReport {
                slot: choice.active.slot.clone(),
                version: running.clone(),
                files: std::mem::take(&mut files),
                clean,
                skipped: choice.skipped,
            });
            let Some(request) = request.filter(|_| clean) else {
                return Ok(reports);
            };
            // Restart to update: in the manual modes the person's accept is the switch.
            let now = Settings::load(&self.root);
            if now.mode != UpdateMode::AutoUpdate {
                let up = (self.make_updater)(&now);
                if let Some(v) = ready_version(up.store(), now.channel, &running)
                    && let Err(e) = up.store().switch_to(&Slot::Version(v.clone()))
                {
                    // Busy or unwritable: the app still restarts, with its documents. The
                    // newest version still starts in Auto-update; here the old one does.
                    eprintln!("kadakas-launcher: could not switch to {v}: {e}");
                }
            }
            files = request.documents.into_iter().map(Into::into).collect();
        }
    }

    /// Chooses a version and starts it. A version that cannot be started is skipped.
    #[allow(clippy::type_complexity)]
    fn start(
        &self,
        updater: &Updater,
        settings: &Settings,
        files: &[OsString],
    ) -> Result<(Box<dyn Process>, Option<RunGuard>, Choice, RunStatus)> {
        let store = updater.store();
        let mut failed: Vec<Slot> = Vec::new();
        loop {
            // The choice and the run record happen under the install lock, so a prune of
            // another launcher cannot delete the folder between the two. A lock that stays busy
            // does not stop the start: the app starts, as it would without a launcher.
            let held = store.lock(true).ok().flatten();
            let choice = choose::choose(store, settings, &failed)
                .ok_or_else(|| UpdateError::NotInstalled("any version".into()))?;
            let found = flavors::discover(&choice.active.dir);
            let Some(flavor) = flavors::pick(&found, &settings.flavor).cloned() else {
                failed.push(choice.active.slot.clone());
                continue;
            };
            let guard = match RunGuard::begin(store, &choice.active.slot) {
                Ok(g) => Some(g),
                Err(e) => {
                    eprintln!("kadakas-launcher: no record of this run, starting anyway: {e}");
                    None
                }
            };
            drop(held);
            let base = RunStatus {
                running: choice.active.version.to_string(),
                label: store.meta(&choice.active.slot).label,
                update: UpdateState::Checking,
                skipped: choice.skipped.clone(),
            };
            let mut env = Vec::new();
            if let Some(g) = &guard {
                let _ = handshake::write_status(store.root(), g.id(), &base);
                env.push((ENV_ROOT.into(), store.root().to_string_lossy().into_owned()));
                env.push((ENV_RUN.into(), g.id().to_string()));
            }
            let spec = SpawnSpec {
                dir: choice.active.dir.clone(),
                flavor,
                profile: settings.profile.clone(),
                files: files.to_vec(),
                env,
            };
            match self.spawner.spawn(&spec) {
                Ok(child) => return Ok((child, guard, choice, base)),
                Err(_) => {
                    // Not a crash of the version: it never ran. Skip it for this start.
                    failed.push(choice.active.slot.clone());
                    if let Some(g) = guard {
                        g.abandon();
                    }
                }
            }
        }
    }

    /// Waits for the program, and does the background work meanwhile.
    fn watch(&self, child: &mut dyn Process, guard: &RunGuard, running: &Version, base: RunStatus) {
        let root = self.root.clone();
        let id = guard.id().to_string();
        let mut worker: Option<Worker> = None;
        let mut next_check = Instant::now();
        let publish = |state: &UpdateState| {
            let status = RunStatus {
                update: state.clone(),
                ..base.clone()
            };
            let _ = handshake::write_status(&root, &id, &status);
        };
        loop {
            match child.try_wait() {
                Ok(None) => {}
                // Ended, or the system cannot say any more: stop watching.
                Ok(Some(_)) | Err(_) => return,
            }
            if let Some(w) = &worker {
                loop {
                    match w.rx.try_recv() {
                        Ok(Msg::State(s)) => publish(&s),
                        Ok(Msg::Done(s)) => {
                            next_check = Instant::now()
                                + if matches!(s, UpdateState::Failed { .. }) {
                                    self.timing.retry
                                } else {
                                    self.timing.recheck
                                };
                            publish(&s);
                            worker = None;
                            break;
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            next_check = Instant::now() + self.timing.retry;
                            worker = None;
                            break;
                        }
                    }
                }
            }
            let asked = handshake::take_download_request(&root, &id);
            if worker.is_none() && (asked || Instant::now() >= next_check) {
                // The settings are read again: the person may have changed them in the app.
                let settings = Settings::load(&root);
                worker = Some(self.spawn_worker(&settings, running, asked));
            }
            std::thread::sleep(self.timing.poll);
        }
    }

    fn spawn_worker(&self, settings: &Settings, running: &Version, force: bool) -> Worker {
        let (tx, rx) = channel();
        let make = self.make_updater.clone();
        let settings = settings.clone();
        let running = running.clone();
        std::thread::spawn(move || {
            let updater = make(&settings);
            updater.store().set_keep_older(settings.keep_older);
            let progress = |s: UpdateState| {
                let _ = tx.send(Msg::State(s));
            };
            let done = cycle(&updater, &settings, &running, force, &progress);
            // Old versions go when a download is done (a lower number to keep works at once).
            let _ = updater.store().prune();
            let _ = tx.send(Msg::Done(done));
        });
        Worker { rx }
    }
}

/// Text for a skipped version, for logs.
pub fn describe_skip(s: &Skipped) -> String {
    let why = match s.reason {
        SkipReason::Crashed => "ended without a clean exit last time",
        SkipReason::RolledBack => "was rolled back",
        SkipReason::NotStarted => "could not be started",
    };
    format!("version {} {why}", s.version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handshake::AppLink;
    use crate::http::MockHttp;
    use crate::store::tests::{archive_file, tmp, v};
    use crate::updater::tests::{BASE, PLATFORM, key, publish, trusted};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// What a scripted program does.
    #[derive(Clone, Default)]
    struct Script {
        /// Polls before it ends.
        polls: u32,
        /// Writes the clean-exit mark when it ends.
        clean: bool,
        /// Asks for a restart with these documents when it ends.
        restart: Option<Vec<PathBuf>>,
        /// Asks for the download first.
        download: bool,
        /// Looks at the status file and keeps what it saw.
        watch_status: bool,
    }

    struct ScriptedProcess {
        script: Script,
        link: Option<AppLink>,
        polls: u32,
        seen: Arc<Mutex<Vec<UpdateState>>>,
        asked: bool,
    }

    impl Process for ScriptedProcess {
        fn try_wait(&mut self) -> std::io::Result<Option<bool>> {
            self.polls += 1;
            if let Some(l) = &self.link {
                if self.script.download && !self.asked {
                    // Wait until the launcher offers the download (Notify only).
                    if matches!(
                        l.status().map(|s| s.update),
                        Some(UpdateState::Available { .. })
                    ) {
                        l.request_download().unwrap();
                        self.asked = true;
                    }
                    return Ok(None);
                }
                if self.script.watch_status
                    && let Some(s) = l.status()
                {
                    let mut seen = self.seen.lock().unwrap();
                    if seen.last() != Some(&s.update) {
                        seen.push(s.update);
                    }
                }
            }
            if self.polls <= self.script.polls {
                return Ok(None);
            }
            if let Some(l) = &self.link {
                if let Some(docs) = &self.script.restart {
                    l.request_restart(docs).unwrap();
                }
                if self.script.clean {
                    l.mark_clean_exit().unwrap();
                }
            }
            Ok(Some(true))
        }
    }

    #[derive(Default)]
    struct FakeSpawner {
        scripts: Mutex<VecDeque<Script>>,
        started: Mutex<Vec<SpawnSpec>>,
        /// Versions (directory names) that cannot be started.
        broken: Mutex<Vec<String>>,
        seen: Arc<Mutex<Vec<UpdateState>>>,
    }

    impl Spawner for FakeSpawner {
        fn spawn(&self, spec: &SpawnSpec) -> Result<Box<dyn Process>> {
            let name = spec.dir.file_name().unwrap().to_string_lossy().into_owned();
            if self.broken.lock().unwrap().contains(&name) {
                return Err(UpdateError::Io(std::io::Error::other("no program")));
            }
            self.started.lock().unwrap().push(spec.clone());
            let script = self.scripts.lock().unwrap().pop_front().unwrap_or(Script {
                clean: true,
                ..Script::default()
            });
            let env = |k: &str| {
                spec.env
                    .iter()
                    .find(|(n, _)| n == k)
                    .map(|(_, v)| v.clone())
            };
            Ok(Box::new(ScriptedProcess {
                script,
                link: AppLink::from_env(&env),
                polls: 0,
                seen: self.seen.clone(),
                asked: false,
            }))
        }
    }

    struct World {
        root: PathBuf,
        http: Arc<MockHttp>,
        spawner: Arc<FakeSpawner>,
    }

    fn world(name: &str, installed: &str) -> (World, Supervisor) {
        let root = tmp(name);
        let http = Arc::new(MockHttp::new());
        let spawner = Arc::new(FakeSpawner::default());
        let (r, h) = (root.clone(), http.clone());
        let installed = installed.to_string();
        let factory: UpdaterFactory = Arc::new(move |s: &Settings| {
            let bundled = crate::store::tests::bundled(&r, &installed);
            let store =
                Store::new(r.join("r"), Some(bundled)).with_lock_wait(Duration::from_millis(100));
            // The channel local without a local site has no address to ask.
            let base = if s.channel == Channel::Local && s.site.is_empty() {
                ""
            } else {
                BASE
            };
            Updater::new(store, h.clone(), trusted(&[1]), base, PLATFORM)
                .with_clock(Arc::new(|| crate::updater::tests::NOW))
        });
        let sup = Supervisor::new(root.join("r"), factory, spawner.clone()).with_timing(Timing {
            poll: Duration::from_millis(2),
            recheck: Duration::from_millis(40),
            retry: Duration::from_millis(40),
        });
        (
            World {
                root,
                http,
                spawner,
            },
            sup,
        )
    }

    impl World {
        fn settings(&self, f: impl FnOnce(&mut Settings)) {
            Settings::update(&self.root.join("r"), f).unwrap();
        }
        fn store(&self) -> Store {
            Store::new(
                self.root.join("r"),
                Some(crate::store::tests::bundled(&self.root, "1.0.0")),
            )
            .with_lock_wait(Duration::from_millis(100))
        }
        fn script(&self, s: Script) {
            self.spawner.scripts.lock().unwrap().push_back(s);
        }
        fn started(&self) -> Vec<SpawnSpec> {
            self.spawner.started.lock().unwrap().clone()
        }
    }

    #[test]
    fn the_app_starts_at_once_with_the_files_and_the_run_variables() {
        let (w, sup) = world("sup-start", "1.0.0");
        w.settings(|s| s.profile = "acad".into());
        let files = vec![OsString::from("/a/one.kad")];
        let reports = sup.run(files.clone()).unwrap();
        assert_eq!(reports.len(), 1);
        assert!(reports[0].clean);
        assert_eq!(reports[0].files, files);
        assert_eq!(reports[0].slot, Slot::Bundled);
        let spec = &w.started()[0];
        assert_eq!(spec.files, files);
        assert_eq!(spec.profile, "acad");
        let env: std::collections::HashMap<_, _> = spec.env.iter().cloned().collect();
        assert_eq!(
            env[ENV_ROOT],
            w.root.join("r").to_string_lossy().into_owned()
        );
        assert!(crate::runs::valid_run_id(&env[ENV_RUN]));
        // The run is settled: nothing is left.
        assert!(w.store().running_slots().is_empty());
        std::fs::remove_dir_all(&w.root).unwrap();
    }

    #[test]
    fn no_network_wait_before_the_start() {
        // The manifest is not there at all: the app starts anyway, the check fails later.
        let (w, sup) = world("sup-nonet", "1.0.0");
        w.script(Script {
            polls: 30,
            clean: true,
            watch_status: true,
            ..Script::default()
        });
        sup.run(vec![]).unwrap();
        let seen = w.spawner.seen.lock().unwrap().clone();
        assert!(
            matches!(seen.last(), Some(UpdateState::Failed { .. })),
            "{seen:?}"
        );
        std::fs::remove_dir_all(&w.root).unwrap();
    }

    #[test]
    fn auto_update_downloads_in_the_background_and_tells_the_app() {
        let (w, sup) = world("sup-auto", "1.0.0");
        publish(&w.http, "1.1.0", Channel::Stable, &key(1));
        w.script(Script {
            polls: 500,
            clean: true,
            watch_status: true,
            ..Script::default()
        });
        // Stop early once the app has seen "ready": the script ends by polls, so make it long
        // enough and check the order of states.
        let reports = sup.run(vec![]).unwrap();
        assert_eq!(reports[0].version, v("1.0.0"));
        let seen = w.spawner.seen.lock().unwrap().clone();
        assert!(
            seen.contains(&UpdateState::Ready {
                version: "1.1.0".into()
            }),
            "{seen:?}"
        );
        // The new version is in the store and is not running.
        assert!(w.store().list().iter().any(|i| i.version == v("1.1.0")));
        std::fs::remove_dir_all(&w.root).unwrap();
    }

    #[test]
    fn restart_starts_the_new_version_with_the_documents() {
        let (w, sup) = world("sup-restart", "1.0.0");
        publish(&w.http, "1.1.0", Channel::Stable, &key(1));
        let docs = vec![PathBuf::from("/d/a.kad"), PathBuf::from("/d/b.kad")];
        // The first app waits for the update to be ready, then asks for the restart.
        w.script(Script {
            polls: 300,
            clean: true,
            restart: Some(docs.clone()),
            ..Script::default()
        });
        w.script(Script {
            clean: true,
            ..Script::default()
        });
        let reports = sup.run(vec![]).unwrap();
        assert_eq!(reports.len(), 2, "{reports:?}");
        assert_eq!(reports[0].version, v("1.0.0"));
        assert_eq!(reports[1].version, v("1.1.0"));
        let want: Vec<OsString> = docs.into_iter().map(Into::into).collect();
        assert_eq!(reports[1].files, want);
        assert_eq!(w.started()[1].files, want);
        std::fs::remove_dir_all(&w.root).unwrap();
    }

    #[test]
    fn a_restart_after_a_crash_is_not_followed() {
        let (w, sup) = world("sup-restart-crash", "1.0.0");
        w.script(Script {
            clean: false,
            restart: Some(vec![PathBuf::from("/d/a.kad")]),
            ..Script::default()
        });
        let reports = sup.run(vec![]).unwrap();
        assert_eq!(reports.len(), 1);
        assert!(!reports[0].clean);
        assert!(w.store().crashed_slots().contains(&Slot::Bundled));
        // The request was taken (deleted) all the same, and the run left no file.
        let left = std::fs::read_dir(crate::runs::runs_dir(&w.root.join("r")))
            .map(|rd| rd.count())
            .unwrap_or(0);
        assert_eq!(left, 0);
        std::fs::remove_dir_all(&w.root).unwrap();
    }

    #[test]
    fn a_crashed_version_is_skipped_at_the_next_start_and_the_app_is_told() {
        let (w, sup) = world("sup-skip", "1.0.0");
        // 1.1.0 is installed and starts first (Auto-update), then it crashes.
        let st = w.store();
        st.install(&v("1.1.0"), &archive_file(&w.root, "1.1.0"), false)
            .unwrap();
        // The installed archive has a program named kadakas-app: the folder is startable.
        w.script(Script {
            clean: false,
            ..Script::default()
        });
        let first = sup.run(vec![]).unwrap();
        assert_eq!(first[0].version, v("1.1.0"));
        assert!(!first[0].clean);

        // Next start: the newest version that did not crash.
        w.script(Script {
            polls: 20,
            clean: true,
            watch_status: true,
            ..Script::default()
        });
        let second = sup.run(vec![]).unwrap();
        assert_eq!(second[0].version, v("1.0.0"));
        assert_eq!(
            second[0].skipped,
            [Skipped {
                version: "1.1.0".into(),
                reason: SkipReason::Crashed
            }]
        );
        // A clean run of the bundled app leaves the skip in place (1.1.0 did not run again).
        assert!(
            w.store()
                .crashed_slots()
                .contains(&Slot::Version(v("1.1.0")))
        );
        std::fs::remove_dir_all(&w.root).unwrap();
    }

    #[test]
    fn a_version_that_cannot_start_is_passed_over() {
        let (w, sup) = world("sup-broken", "1.0.0");
        let st = w.store();
        st.install(&v("1.1.0"), &archive_file(&w.root, "1.1.0"), false)
            .unwrap();
        w.spawner.broken.lock().unwrap().push("1.1.0".into());
        let reports = sup.run(vec![]).unwrap();
        assert_eq!(reports[0].version, v("1.0.0"));
        assert_eq!(reports[0].skipped[0].reason, SkipReason::NotStarted);
        // It never ran, so it is not a crash.
        assert!(w.store().crashed_slots().is_empty());
        // Nothing can start: an error, not a hang.
        w.spawner.broken.lock().unwrap().push("bundled-dir".into());
        assert!(sup.run(vec![]).is_err());
        std::fs::remove_dir_all(&w.root).unwrap();
    }

    #[test]
    fn notify_only_waits_for_the_download_request() {
        let (w, sup) = world("sup-notify", "1.0.0");
        w.settings(|s| s.mode = UpdateMode::NotifyOnly);
        publish(&w.http, "1.1.0", Channel::Stable, &key(1));
        w.script(Script {
            download: true,
            polls: 300,
            clean: true,
            watch_status: true,
            ..Script::default()
        });
        sup.run(vec![]).unwrap();
        let seen = w.spawner.seen.lock().unwrap().clone();
        // Available first (the app asked for the download only after it saw this), then ready.
        let first = seen.first().cloned();
        assert!(
            matches!(first, Some(UpdateState::Available { .. })),
            "{seen:?}"
        );
        assert!(
            seen.contains(&UpdateState::Ready {
                version: "1.1.0".into()
            }),
            "{seen:?}"
        );
        // The download did not switch: the person chose the bundled app.
        assert_eq!(w.store().active().unwrap().slot, Slot::Bundled);
        std::fs::remove_dir_all(&w.root).unwrap();
    }

    #[test]
    fn auto_download_switches_at_the_restart_only() {
        let (w, sup) = world("sup-autodl", "1.0.0");
        w.settings(|s| s.mode = UpdateMode::AutoDownload);
        publish(&w.http, "1.1.0", Channel::Stable, &key(1));
        w.script(Script {
            polls: 300,
            clean: true,
            restart: Some(vec![PathBuf::from("/d/a.kad")]),
            ..Script::default()
        });
        let reports = sup.run(vec![]).unwrap();
        assert_eq!(reports.len(), 2, "{reports:?}");
        assert_eq!(reports[1].version, v("1.1.0"));
        assert_eq!(w.store().active().unwrap().version, v("1.1.0"));
        std::fs::remove_dir_all(&w.root).unwrap();
    }

    #[test]
    fn a_failed_check_is_tried_again_and_a_local_channel_without_a_site_checks_nothing() {
        let (w, sup) = world("sup-retry", "1.0.0");
        w.script(Script {
            polls: 120,
            clean: true,
            watch_status: true,
            ..Script::default()
        });
        sup.run(vec![]).unwrap();
        let before = w.http.requests().len();
        assert!(before >= 2, "the check ran again: {before}");
        std::fs::remove_dir_all(&w.root).unwrap();

        let (w, sup) = world("sup-nosite", "1.0.0");
        w.settings(|s| s.channel = Channel::Local);
        w.script(Script {
            polls: 20,
            clean: true,
            watch_status: true,
            ..Script::default()
        });
        sup.run(vec![]).unwrap();
        assert!(w.http.requests().is_empty(), "nobody was asked");
        let seen = w.spawner.seen.lock().unwrap().clone();
        assert_eq!(seen.last(), Some(&UpdateState::UpToDate), "{seen:?}");
        std::fs::remove_dir_all(&w.root).unwrap();
    }

    #[test]
    fn the_app_starts_even_when_no_record_of_the_run_can_be_written() {
        let (w, sup) = world("sup-norecord", "1.0.0");
        // `runs` is a file, not a folder: the record cannot be made.
        std::fs::create_dir_all(w.root.join("r")).unwrap();
        std::fs::write(w.root.join("r").join("runs"), b"in the way").unwrap();
        let reports = sup.run(vec![OsString::from("/a/one.kad")]).unwrap();
        assert_eq!(reports.len(), 1);
        assert!(!reports[0].clean, "nothing is known about how it ended");
        let spec = &w.started()[0];
        assert_eq!(spec.files, [OsString::from("/a/one.kad")]);
        assert!(
            spec.env.is_empty(),
            "the app gets no run id it could not use"
        );
        std::fs::remove_dir_all(&w.root).unwrap();
    }

    #[test]
    fn a_busy_install_lock_does_not_stop_the_start_or_the_restart() {
        let (w, sup) = world("sup-busy", "1.0.0");
        w.settings(|s| s.mode = UpdateMode::AutoDownload);
        // 1.1.0 is downloaded and waits for the restart.
        w.store()
            .stage(&v("1.1.0"), &archive_file(&w.root, "1.1.0"), "stable")
            .unwrap();
        // Another launcher holds the install lock all the time.
        let held = w.store().lock(false).unwrap().expect("the lock is free");
        let docs = vec![PathBuf::from("/d/a.kad")];
        w.script(Script {
            polls: 10,
            clean: true,
            restart: Some(docs.clone()),
            ..Script::default()
        });
        w.script(Script {
            clean: true,
            ..Script::default()
        });
        let reports = sup.run(vec![]).unwrap();
        drop(held);
        assert_eq!(reports.len(), 2, "{reports:?}");
        // The switch could not be written: the old version restarts, with the documents.
        assert_eq!(reports[1].version, v("1.0.0"));
        let want: Vec<OsString> = docs.into_iter().map(Into::into).collect();
        assert_eq!(reports[1].files, want);
        std::fs::remove_dir_all(&w.root).unwrap();
    }

    #[test]
    fn skips_are_described() {
        let s = |reason| {
            describe_skip(&Skipped {
                version: "1.2.0".into(),
                reason,
            })
        };
        assert!(s(SkipReason::Crashed).contains("clean exit"));
        assert!(s(SkipReason::RolledBack).contains("rolled back"));
        assert!(s(SkipReason::NotStarted).contains("could not be started"));
    }

    #[test]
    fn ready_means_newer_installed_not_skipped_and_allowed_by_the_channel() {
        let root = tmp("sup-ready");
        let st = Store::new(
            root.join("r"),
            Some(crate::store::tests::bundled(&root, "1.0.0")),
        );
        st.stage(&v("1.1.0"), &archive_file(&root, "1.1.0"), "stable")
            .unwrap();
        st.stage(
            &v("1.2.0-beta.1"),
            &archive_file(&root, "1.2.0-beta.1"),
            "nightly",
        )
        .unwrap();
        let now = v("1.0.0");
        assert_eq!(ready_version(&st, Channel::Stable, &now), Some(v("1.1.0")));
        assert_eq!(
            ready_version(&st, Channel::Beta, &now),
            Some(v("1.2.0-beta.1"))
        );
        assert_eq!(ready_version(&st, Channel::Stable, &v("1.1.0")), None);
        st.note_run_end(&Slot::Version(v("1.1.0")), false).unwrap();
        assert_eq!(ready_version(&st, Channel::Stable, &now), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
