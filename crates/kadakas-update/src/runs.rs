//! Runs and the crash marker (PLAT-16).
//!
//! Every start of the app by the launcher is a run. The launcher process that watches the app
//! (the supervisor, see `supervisor`) makes a record in `runs/` and holds an advisory lock on a
//! lock file for as long as it lives. The app writes a clean-exit mark when it closes in the
//! normal way. When the app ends, the supervisor looks for the mark. A start without the mark
//! is a crash: the version goes into `health.json` and is skipped at the next start
//! (see `choose`). A clean end takes the version out of that file again.
//!
//! If a supervisor itself dies, the system frees its lock. The app holds a lock of its own too
//! (`<id>.app.lock`, see `handshake::AppLink::hold_run_lock`), so an app that outlives its
//! launcher still counts as running. The next launcher sees a record whose locks are both free
//! (`Store::settle_runs`), reads the mark and does the same. A record with a lock that is held
//! belongs to an app that runs now. So a second launcher never takes a running app for a crash,
//! and pruning never deletes the folder of a running version.
//!
//! **What counts as a crash.** A run without the mark can also be a shutdown, a logout, a
//! force quit or a power loss. So one long run without the mark is not enough to skip a
//! version: it is skipped when the run was short (under `SHORT_RUN_SECONDS`, the start itself
//! failed) or when two runs in a row ended without the mark. A clean run clears the count. The
//! marks are kept by version (the bundled app by its version too), so a new installer does not
//! inherit the crash of the old one.
//!
//! Files in `runs/`, all named by the run id:
//!
//! | file | written by | content |
//! |---|---|---|
//! | `<id>.json` | supervisor | the record: version, slot, process id, start time |
//! | `<id>.lock` | supervisor | locked while the supervisor lives |
//! | `<id>.clean` | the app | the clean-exit mark |
//! | `<id>.status.json` | supervisor | what the app shows (`handshake::RunStatus`) |
//! | `<id>.restart.json` | the app | "restart to update", with the open documents |
//! | `<id>.download` | the app | "download the new version now" (Notify only) |
//! | `<id>.crash.json` | the app | what the app knew when it died (PLAT-20, `kadakas-crash`) |
//!
//! **Crash reports (PLAT-20).** A run that ends without the mark and has a crash file keeps it:
//! the file moves to `crash-reports/<id>.json` (at most 10 files, the oldest go first). The app
//! shows the report at its next start. A run that ends with the mark deletes the crash file (a
//! panic that the app survived is no crash). When the run ended by a signal or an exception that
//! the app could not report (it has no file), the launcher writes a small report of its own with
//! the name of the signal.

use crate::error::Result;
use crate::store::{Slot, Store};
use kadakas_sys::fsio::{self, Access};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const RUNS: &str = "runs";

/// A run without the mark that ended sooner than this after the start is a crash at once.
pub const SHORT_RUN_SECONDS: u64 = 60;
const HEALTH: &str = "health.json";

/// The folder with the files of the runs.
pub fn runs_dir(root: &Path) -> PathBuf {
    root.join(RUNS)
}

/// The path of one file of a run.
pub fn run_file(root: &Path, id: &str, suffix: &str) -> PathBuf {
    runs_dir(root).join(format!("{id}.{suffix}"))
}

/// A run id is made by the supervisor. Anything else (from an environment variable) is refused,
/// so an id can never name a path outside `runs/`.
pub fn valid_run_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

fn new_run_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "{:x}-{:x}-{:x}",
        std::process::id(),
        nanos,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// The record of a run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub slot: Slot,
    pub pid: u32,
    pub started: u64,
}

/// A run that this launcher process watches. Drop it or call [`RunGuard::finish`].
pub struct RunGuard {
    root: PathBuf,
    id: String,
    slot: Slot,
    started: u64,
    lock: Option<std::fs::File>,
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunEnd {
    /// The app wrote the clean-exit mark.
    pub clean: bool,
}

impl RunGuard {
    /// Starts a run of `slot`: makes the record and takes the lock.
    pub fn begin(store: &Store, slot: &Slot) -> Result<RunGuard> {
        let root = store.root().to_path_buf();
        std::fs::create_dir_all(runs_dir(&root))?;
        let id = new_run_id();
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(run_file(&root, &id, "lock"))?;
        // A new file is free. If this fails, no other process holds it for this new id.
        lock.try_lock().map_err(|e| match e {
            std::fs::TryLockError::Error(e) => e,
            std::fs::TryLockError::WouldBlock => std::io::Error::other("run lock is taken"),
        })?;
        let record = RunRecord {
            id: id.clone(),
            slot: slot.clone(),
            pid: std::process::id(),
            started: now_seconds(),
        };
        let text = serde_json::to_vec_pretty(&record).expect("record serializes");
        fsio::write_durable(
            &run_file(&root, &id, "json"),
            &text,
            Access::KeepExisting,
            ".tmp",
        )?;
        Ok(RunGuard {
            root,
            id,
            slot: slot.clone(),
            started: record.started,
            lock: Some(lock),
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn slot(&self) -> &Slot {
        &self.slot
    }

    /// The run ended. Reads the clean-exit mark, records the result in `health.json`, and
    /// deletes the files of the run.
    pub fn finish(self, store: &Store) -> RunEnd {
        self.finish_with(store, None)
    }

    /// Like [`RunGuard::finish`]. `signal` names the signal or exception that ended the app, if
    /// the system said so (`SIGSEGV`): without a crash file of the app, it becomes a small
    /// report.
    pub fn finish_with(mut self, store: &Store, signal: Option<&str>) -> RunEnd {
        let clean = run_file(&self.root, &self.id, "clean").is_file();
        let seconds = now_seconds().saturating_sub(self.started);
        let _ = store.note_run_end_after(&self.slot, clean, seconds);
        if !clean {
            keep_crash(store, &self.id, &self.slot, signal, seconds);
        }
        self.remove_files();
        RunEnd { clean }
    }

    /// The program never started (it could not be launched): delete the files of the run and
    /// record nothing. That is not a crash of the version.
    pub fn abandon(mut self) {
        self.remove_files();
    }

    fn remove_files(&mut self) {
        remove_run_files(&self.root, &self.id);
        self.lock = None;
        let _ = std::fs::remove_file(run_file(&self.root, &self.id, "lock"));
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        // Not finished: the lock goes, the files stay, and the next launcher settles the run.
        self.lock = None;
    }
}

/// Keeps the crash file of an unclean run for the next start of the app. When the app left none
/// but the system says a crash signal ended it, writes a small report from that.
fn keep_crash(store: &Store, id: &str, slot: &Slot, signal: Option<&str>, seconds: u64) {
    let root = store.root();
    if kadakas_crash::queue::keep(root, id).unwrap_or(false) {
        return;
    }
    let Some(signal) = signal else { return };
    let mut f = kadakas_crash::CrashFile::new(kadakas_crash::CrashKind::Native);
    f.run_id = id.to_string();
    f.created = now_seconds();
    f.version = match slot {
        Slot::Version(v) => v.to_string(),
        Slot::Bundled => store
            .bundled()
            .map(|b| b.version.to_string())
            .unwrap_or_default(),
    };
    f.channel = store.meta(slot).label;
    f.os = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    f.message = format!("{signal} (reported by the launcher; the app left no crash file)");
    f.uptime_secs = seconds;
    let _ = kadakas_crash::queue::store(root, id, &f);
}

fn remove_run_files(root: &Path, id: &str) {
    for suffix in [
        "crash.json",
        "json",
        "clean",
        "status.json",
        "restart.json",
        "download",
        "app.lock",
    ] {
        let _ = std::fs::remove_file(run_file(root, id, suffix));
    }
}

/// True while the file is locked by somebody.
fn is_locked(path: &Path) -> bool {
    let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) else {
        return false;
    };
    matches!(f.try_lock(), Err(std::fs::TryLockError::WouldBlock))
}

/// True while the supervisor or the app of the run lives (one of the two locks is held).
fn is_alive(root: &Path, id: &str) -> bool {
    is_locked(&run_file(root, id, "lock")) || is_locked(&run_file(root, id, "app.lock"))
}

fn read_record(root: &Path, id: &str) -> Option<RunRecord> {
    std::fs::read(run_file(root, id, "json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<RunRecord>(&b).ok())
        .filter(|r| r.id == id)
}

fn read_records(root: &Path) -> Vec<(String, Option<RunRecord>)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(runs_dir(root)) else {
        return out;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".json") else {
            continue;
        };
        if id.ends_with(".status") || id.ends_with(".restart") || !valid_run_id(id) {
            continue;
        }
        out.push((id.to_string(), read_record(root, id)));
    }
    out
}

/// What is known about the unclean runs of one version.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Strikes {
    /// Runs in a row that ended without the clean-exit mark.
    count: u32,
    /// The last of them was short.
    short: bool,
}

impl Strikes {
    fn skipped(self) -> bool {
        self.count >= 2 || (self.count >= 1 && self.short)
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Health {
    /// Unclean runs by version key (see `Store::health_key`).
    crashed: std::collections::BTreeMap<String, Strikes>,
    /// Versions that the person rolled back from. Auto-update does not start them again.
    rolled_back: BTreeSet<String>,
}

impl Store {
    fn health_path(&self) -> PathBuf {
        self.root().join(HEALTH)
    }

    /// The key of a version in `health.json`. The bundled app is keyed by its version too:
    /// a new installer brings a new bundled version, which starts clean.
    fn health_key(&self, slot: &Slot) -> String {
        match slot {
            Slot::Bundled => format!(
                "bundled@{}",
                self.bundled()
                    .map_or(String::new(), |b| b.version.to_string())
            ),
            Slot::Version(v) => v.to_string(),
        }
    }

    fn slot_of_key(&self, key: &str) -> Option<Slot> {
        match key.strip_prefix("bundled@") {
            Some(v) => {
                (self.health_key(&Slot::Bundled) == format!("bundled@{v}")).then_some(Slot::Bundled)
            }
            None => crate::version::Version::parse(key).ok().map(Slot::Version),
        }
    }

    fn read_health(&self) -> Health {
        std::fs::read(self.health_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn write_health(&self, h: &Health) -> Result<()> {
        std::fs::create_dir_all(self.root())?;
        let text = serde_json::to_vec_pretty(h).expect("health serializes");
        fsio::write_durable(&self.health_path(), &text, Access::KeepExisting, ".tmp")?;
        Ok(())
    }

    /// The versions that Auto-update skips because of unclean runs: the last run was short, or
    /// two runs in a row ended without the clean-exit mark.
    pub fn crashed_slots(&self) -> BTreeSet<Slot> {
        self.read_health()
            .crashed
            .iter()
            .filter(|(_, s)| s.skipped())
            .filter_map(|(k, _)| self.slot_of_key(k))
            .collect()
    }

    /// The versions that the person rolled back from.
    pub fn rolled_back_slots(&self) -> BTreeSet<Slot> {
        self.read_health()
            .rolled_back
            .iter()
            .filter_map(|k| self.slot_of_key(k))
            .collect()
    }

    /// Records how a run of `slot` ended. The length of the run is not known: it counts as a
    /// short one, so an unclean end skips the version at once. A clean end clears the crashes.
    pub fn note_run_end(&self, slot: &Slot, clean: bool) -> Result<()> {
        self.note_run_end_after(slot, clean, 0)
    }

    /// Like [`Store::note_run_end`], with the length of the run in seconds.
    pub fn note_run_end_after(&self, slot: &Slot, clean: bool, seconds: u64) -> Result<()> {
        let _guard = self.lock(true)?;
        self.note_run_end_locked(slot, clean, seconds)
    }

    fn note_run_end_locked(&self, slot: &Slot, clean: bool, seconds: u64) -> Result<()> {
        let mut h = self.read_health();
        let key = self.health_key(slot);
        let changed = if clean {
            h.crashed.remove(&key).is_some()
        } else {
            let e = h.crashed.entry(key).or_default();
            e.count += 1;
            e.short = seconds < SHORT_RUN_SECONDS;
            true
        };
        if changed {
            self.write_health(&h)?;
        }
        Ok(())
    }

    /// The person wants to try a version again: takes it off the lists of crashed and
    /// rolled back versions.
    pub fn clear_crash(&self, slot: &Slot) -> Result<()> {
        let _guard = self.lock(true)?;
        self.forget_crash(slot);
        Ok(())
    }

    /// Like `clear_crash`, for code that holds the install lock already.
    pub(crate) fn forget_crash(&self, slot: &Slot) {
        let mut h = self.read_health();
        let key = self.health_key(slot);
        let changed = h.crashed.remove(&key).is_some() | h.rolled_back.remove(&key);
        if changed {
            let _ = self.write_health(&h);
        }
    }

    /// A rollback leaves `slot`: Auto-update must not start it again at once.
    pub(crate) fn note_rolled_back(&self, slot: &Slot) {
        let mut h = self.read_health();
        if h.rolled_back.insert(self.health_key(slot)) {
            let _ = self.write_health(&h);
        }
    }

    /// How many crash reports wait for the person (PLAT-20): kept after an unclean end, not yet
    /// shown or decided in the app.
    pub fn kept_crash_reports(&self) -> usize {
        kadakas_crash::queue::count(self.root())
    }

    /// The versions that run now (the supervisor or the app holds the lock of the run).
    pub fn running_slots(&self) -> Vec<Slot> {
        read_records(self.root())
            .into_iter()
            .filter_map(|(id, r)| r.filter(|_| is_alive(self.root(), &id)))
            .map(|r| r.slot)
            .collect()
    }

    /// Settles the runs whose supervisor and app are gone: reads the clean-exit mark, records
    /// the result and deletes the files. Call it when the launcher starts. Each record is
    /// settled under the install lock and read again there, so two launchers that start at the
    /// same time count a run once.
    pub fn settle_runs(&self) {
        for (id, _) in read_records(self.root()) {
            let Ok(_guard) = self.lock(true) else {
                continue;
            };
            // Another launcher may have settled it while we waited for the lock.
            let record = read_record(self.root(), &id);
            if is_alive(self.root(), &id)
                || (record.is_none() && !run_file(self.root(), &id, "json").exists())
            {
                continue;
            }
            if let Some(r) = record {
                let clean = run_file(self.root(), &id, "clean").is_file();
                let seconds = now_seconds().saturating_sub(r.started);
                let _ = self.note_run_end_locked(&r.slot, clean, seconds);
                if !clean {
                    keep_crash(self, &id, &r.slot, None, seconds);
                }
            }
            remove_run_files(self.root(), &id);
            let _ = std::fs::remove_file(run_file(self.root(), &id, "lock"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::{bundled, tmp, v};

    fn store(root: &Path) -> Store {
        Store::new(root.join("r"), Some(bundled(root, "1.0.0")))
    }

    fn slot(s: &str) -> Slot {
        Slot::Version(v(s))
    }

    #[test]
    fn run_ids_are_safe_names() {
        for ok in ["1a-2b-3c", "ABC-def", "0"] {
            assert!(valid_run_id(ok), "{ok}");
        }
        for bad in ["", "../x", "a/b", "a.b", "g", &"1".repeat(65)] {
            assert!(!valid_run_id(bad), "{bad}");
        }
        assert!(valid_run_id(&new_run_id()));
        assert_ne!(new_run_id(), new_run_id());
    }

    #[test]
    fn a_clean_run_leaves_no_crash() {
        let root = tmp("run-clean");
        let s = store(&root);
        let g = RunGuard::begin(&s, &slot("1.1.0")).unwrap();
        assert_eq!(s.running_slots(), [slot("1.1.0")]);
        assert_eq!(g.slot(), &slot("1.1.0"));
        std::fs::write(run_file(s.root(), g.id(), "clean"), b"").unwrap();
        let id = g.id().to_string();
        assert_eq!(g.finish(&s), RunEnd { clean: true });
        assert!(s.crashed_slots().is_empty());
        assert!(s.running_slots().is_empty());
        // Everything of the run is gone.
        assert!(!run_file(s.root(), &id, "json").exists());
        assert!(!run_file(s.root(), &id, "lock").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_run_without_the_mark_is_a_crash_and_a_later_clean_run_clears_it() {
        let root = tmp("run-crash");
        let s = store(&root);
        let g = RunGuard::begin(&s, &slot("1.1.0")).unwrap();
        assert_eq!(g.finish(&s), RunEnd { clean: false });
        assert_eq!(
            s.crashed_slots().into_iter().collect::<Vec<_>>(),
            [slot("1.1.0")]
        );
        let g = RunGuard::begin(&s, &slot("1.1.0")).unwrap();
        std::fs::write(run_file(s.root(), g.id(), "clean"), b"").unwrap();
        g.finish(&s);
        assert!(s.crashed_slots().is_empty());
        // The explicit way: the person tries a version again.
        s.note_run_end(&Slot::Bundled, false).unwrap();
        assert_eq!(s.crashed_slots().len(), 1);
        s.clear_crash(&Slot::Bundled).unwrap();
        assert!(s.crashed_slots().is_empty());
        // A rollback is remembered the same way.
        s.note_rolled_back(&slot("1.1.0"));
        assert_eq!(s.rolled_back_slots().len(), 1);
        s.clear_crash(&slot("1.1.0")).unwrap();
        assert!(s.rolled_back_slots().is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_run_whose_supervisor_died_is_settled_by_the_next_launcher() {
        let root = tmp("run-settle");
        let s = store(&root);
        // Run 1: the supervisor died, the app had written the mark.
        let g1 = RunGuard::begin(&s, &slot("1.1.0")).unwrap();
        std::fs::write(run_file(s.root(), g1.id(), "clean"), b"").unwrap();
        // Run 2: the supervisor died, no mark.
        let g2 = RunGuard::begin(&s, &slot("1.2.0")).unwrap();
        // Run 3: still alive, no mark yet: it must not count as a crash.
        let g3 = RunGuard::begin(&s, &slot("1.3.0")).unwrap();
        let (id1, id2) = (g1.id().to_string(), g2.id().to_string());
        drop(g1);
        drop(g2);
        s.settle_runs();
        assert_eq!(
            s.crashed_slots().into_iter().collect::<Vec<_>>(),
            [slot("1.2.0")]
        );
        assert_eq!(s.running_slots(), [slot("1.3.0")]);
        assert!(!run_file(s.root(), &id1, "json").exists());
        assert!(!run_file(s.root(), &id2, "json").exists());
        assert!(run_file(s.root(), g3.id(), "json").exists());
        // A damaged record and a stranger's file are cleaned up or left alone.
        std::fs::write(runs_dir(s.root()).join("1f.json"), b"{ nope").unwrap();
        std::fs::write(runs_dir(s.root()).join("notes.txt"), b"x").unwrap();
        std::fs::write(runs_dir(s.root()).join("zz.json"), b"{}").unwrap();
        s.settle_runs();
        assert!(!runs_dir(s.root()).join("1f.json").exists());
        assert!(runs_dir(s.root()).join("notes.txt").exists());
        assert!(runs_dir(s.root()).join("zz.json").exists());
        assert_eq!(s.running_slots(), [slot("1.3.0")]);
        drop(g3);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_new_copy_of_a_version_forgets_its_crash() {
        let root = tmp("run-forget");
        let s = store(&root);
        s.note_run_end(&slot("1.1.0"), false).unwrap();
        s.install(
            &v("1.1.0"),
            &crate::store::tests::archive_file(&root, "1.1.0"),
            false,
        )
        .unwrap();
        assert!(s.crashed_slots().is_empty());
        // A damaged health file reads as "no crash".
        std::fs::write(s.root().join(HEALTH), b"{ nope").unwrap();
        assert!(s.crashed_slots().is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn one_long_unclean_run_is_not_a_crash_but_a_short_or_repeated_one_is() {
        let root = tmp("run-strikes");
        let s = store(&root);
        let a = slot("1.1.0");
        // A shutdown or a force quit after an hour: no crash.
        s.note_run_end_after(&a, false, 3600).unwrap();
        assert!(s.crashed_slots().is_empty());
        // A clean run clears the count: the next long unclean run is again the first.
        s.note_run_end_after(&a, true, 3600).unwrap();
        s.note_run_end_after(&a, false, 3600).unwrap();
        assert!(s.crashed_slots().is_empty());
        // Two in a row: a crash.
        s.note_run_end_after(&a, false, 3600).unwrap();
        assert!(s.crashed_slots().contains(&a));
        // A short run is a crash at once.
        s.clear_crash(&a).unwrap();
        s.note_run_end_after(&a, false, SHORT_RUN_SECONDS - 1)
            .unwrap();
        assert!(s.crashed_slots().contains(&a));
        s.clear_crash(&a).unwrap();
        s.note_run_end_after(&a, false, SHORT_RUN_SECONDS).unwrap();
        assert!(s.crashed_slots().is_empty());
        // The guard measures the run itself: a run that has just started is short.
        let g = RunGuard::begin(&s, &a).unwrap();
        g.finish(&s);
        assert!(s.crashed_slots().contains(&a));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn crash_marks_belong_to_a_version_also_for_the_bundled_app() {
        let root = tmp("run-by-version");
        let old = Store::new(root.join("r"), Some(bundled(&root, "1.0.0")));
        old.note_run_end(&Slot::Bundled, false).unwrap();
        old.note_rolled_back(&Slot::Bundled);
        assert!(old.crashed_slots().contains(&Slot::Bundled));
        // The person installs a newer Kadakas: its bundled app starts clean.
        let new = Store::new(root.join("r"), Some(bundled(&root, "2.0.0")));
        assert!(new.crashed_slots().is_empty());
        assert!(new.rolled_back_slots().is_empty());
        // The old marks are still there for the old version.
        assert!(old.crashed_slots().contains(&Slot::Bundled));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_app_that_outlives_its_launcher_still_runs() {
        use crate::handshake::{AppLink, ENV_ROOT, ENV_RUN};
        let root = tmp("run-app-lock");
        let s = store(&root);
        let g = RunGuard::begin(&s, &slot("1.1.0")).unwrap();
        let id = g.id().to_string();
        let (r, i) = (s.root().to_string_lossy().into_owned(), id.clone());
        let link = AppLink::from_env(&|k| match k {
            ENV_ROOT => Some(r.clone()),
            ENV_RUN => Some(i.clone()),
            _ => None,
        })
        .unwrap();
        let app_lock = link.hold_run_lock().expect("the app takes its lock");
        // A second lock of the same run is refused.
        assert!(link.hold_run_lock().is_none());
        // The launcher dies.
        drop(g);
        assert_eq!(s.running_slots(), [slot("1.1.0")]);
        s.settle_runs();
        assert!(s.crashed_slots().is_empty(), "a running app is no crash");
        assert!(run_file(s.root(), &id, "json").is_file());
        // The app ends without the mark: now the next launcher settles it.
        drop(app_lock);
        assert!(s.running_slots().is_empty());
        s.settle_runs();
        assert!(s.crashed_slots().contains(&slot("1.1.0")));
        assert!(!run_file(s.root(), &id, "json").exists());
        assert!(!run_file(s.root(), &id, "app.lock").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn two_launchers_settle_a_dead_run_once() {
        let root = tmp("run-settle-race");
        let s = store(&root);
        // Five dead runs, each without the mark and short.
        for _ in 0..5 {
            drop(RunGuard::begin(&s, &slot("1.1.0")).unwrap());
        }
        let r = s.root().to_path_buf();
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let (r, root) = (r.clone(), root.clone());
                std::thread::spawn(move || {
                    Store::new(r, Some(bundled(&root, "1.0.0"))).settle_runs();
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let text = std::fs::read_to_string(s.root().join(HEALTH)).unwrap();
        let h: serde_json::Value = serde_json::from_str(&text).unwrap();
        // Five runs, five strikes: every run counted once, none twice.
        assert_eq!(h["crashed"]["1.1.0"]["count"], 5, "{text}");
        assert!(read_records(s.root()).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
