//! The file side of autosave (native only): the recovery folder, the writer thread, the locks and
//! the search for crashed sessions. The settings and the schedule are in `autosave`.
//!
//! Rules for files:
//! - Every write is atomic (temporary file, flush to disk, rename, flush of the folder).
//! - The recovery folder has mode 0700 and its files 0600 (Unix).
//! - All writes of the user's own file go through `manual_save` (Save) or the writer thread
//!   (opt-in autosave). They share one lock and one counter. An autosave job that is older than the
//!   last manual save is dropped, so a late autosave never overwrites a newer Save.
//! - Autosave never touches the `.bak` file of the user.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kadakas_core::model::Document;
use kadakas_sys::fsio;
use serde_json::{Value, json};

use crate::autosave::AutosaveSettings;
use crate::file_io;

const VERSION_SUFFIX: &str = ".autosave.kad";

/// A lock file that nobody holds is stale at once if its process is gone. If the process id is
/// alive (or unknown), the lock must be this old first. This covers the start of another app,
/// which creates the lock file a moment before it takes the lock.
pub const LOCK_GRACE: Duration = Duration::from_secs(10);

/// Counts the saves of each file of the user. The lock is held during each write to such a file.
static MANUAL_SAVES: Mutex<Option<HashMap<PathBuf, u64>>> = Mutex::new(None);

/// The same file under the same key, also before it exists: the folder is resolved, the name is not.
fn file_key(path: &Path) -> PathBuf {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = parent.map_or_else(
        || PathBuf::from("."),
        |p| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()),
    );
    dir.join(path.file_name().unwrap_or_default())
}

/// Runs a write to the user's file (Save, Save As, an agent). It counts as a manual save of
/// that file.
pub fn manual_save<T>(path: &Path, write: impl FnOnce() -> T) -> T {
    let mut all = MANUAL_SAVES.lock().unwrap_or_else(|e| e.into_inner());
    *all.get_or_insert_default()
        .entry(file_key(path))
        .or_insert(0) += 1;
    write()
}

/// The number of manual saves of this file so far. A job stores it when it is made.
pub fn manual_saves(path: &Path) -> u64 {
    let all = MANUAL_SAVES.lock().unwrap_or_else(|e| e.into_inner());
    all.as_ref()
        .and_then(|m| m.get(&file_key(path)).copied())
        .unwrap_or(0)
}

/// The folder for recovery files of this user.
/// `KADAKAS_RECOVERY_DIR` overrides it. Windows: `%LOCALAPPDATA%\kadakas\recovery`.
/// macOS: `~/Library/Application Support/kadakas/recovery`.
/// Elsewhere: `$XDG_STATE_HOME/kadakas/recovery` or `~/.local/state/kadakas/recovery`.
pub fn recovery_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("KADAKAS_RECOVERY_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p));
    }
    Some(kadakas_sys::dirs::state_dir()?.join("recovery"))
}

/// The file for the preferences of this user: next to the keymap file.
pub fn preferences_file() -> Option<PathBuf> {
    let keymap = crate::keymap::user_file()?;
    Some(keymap.with_file_name("preferences.json"))
}

pub fn load_settings() -> AutosaveSettings {
    preferences_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| AutosaveSettings::from_json(&t))
        .unwrap_or_default()
}

pub fn save_settings(s: &AutosaveSettings) -> io::Result<()> {
    let path = preferences_file().ok_or_else(|| io::Error::other("no preferences folder"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    fsio::write_atomic(&path, s.to_json().to_string().as_bytes())
}

pub(crate) fn version_name(id: &str, seq: u64) -> String {
    format!("{id}.{seq:06}{VERSION_SUFFIX}")
}

fn lock_name(id: &str) -> String {
    format!("{id}.lock")
}

fn meta_name(id: &str) -> String {
    format!("{id}.meta.json")
}

use kadakas_sys::fsio::{Access, create_private_dir, write_durable, write_durable_with};

/// The versions of a session in the folder, oldest first, as (sequence number, path).
pub(crate) fn list_versions(dir: &Path, id: &str) -> Vec<(u64, PathBuf)> {
    let prefix = format!("{id}.");
    let mut out: Vec<(u64, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let seq = name
                .strip_prefix(&prefix)?
                .strip_suffix(VERSION_SUFFIX)?
                .parse::<u64>()
                .ok()?;
            Some((seq, e.path()))
        })
        .collect();
    out.sort();
    out
}

/// Deletes all but the newest `keep` versions.
pub fn rotate(dir: &Path, id: &str, keep: usize) {
    let versions = list_versions(dir, id);
    let old = versions.len().saturating_sub(keep.max(1));
    for (_, p) in &versions[..old] {
        let _ = std::fs::remove_file(p);
    }
}

/// One piece of work for the writer thread. The writer makes the JSON, not the UI thread.
pub struct Job {
    /// A copy of the document at the time of the job.
    pub doc: Document,
    /// The path of the document of the user, if it has one. Saved in the meta file.
    pub original: Option<PathBuf>,
    pub keep: usize,
    /// Also write the document here (the opt-in "save over my file").
    pub over: Option<PathBuf>,
    /// `manual_saves(over)` when the job was made. If a manual save of that file came later,
    /// `over` is skipped.
    pub manual_seen: u64,
    /// Something the person must know about this copy (for example that the versions are not in
    /// it). Saved in the meta file.
    pub note: Option<String>,
}

/// What `write_version` did with the file of the user.
#[derive(Debug, PartialEq, Eq)]
pub enum Over {
    NotAsked,
    Written,
    /// A manual save came after the job was made. The file was not touched.
    Stale,
}

/// Writes one version, its meta file, and rotates. Runs on the writer thread.
pub fn write_version(dir: &Path, id: &str, seq: u64, job: &Job) -> io::Result<Over> {
    create_private_dir(dir)?;
    write_durable_with(
        &dir.join(version_name(id, seq)),
        Access::Private,
        ".tmp",
        |w| job.doc.write_compact(w),
    )?;
    let secs = now_unix();
    let meta = json!({
        "original": job.original.as_ref().map(|p| p.display().to_string()),
        "saved_unix": secs,
        "note": job.note,
    });
    write_durable(
        &dir.join(meta_name(id)),
        meta.to_string().as_bytes(),
        Access::Private,
        ".tmp",
    )?;
    rotate(dir, id, job.keep);
    let Some(path) = &job.over else {
        return Ok(Over::NotAsked);
    };
    // The slow part, the write of the copy, runs outside the lock, so a manual Save never waits
    // for it. Only the check and the rename run under the lock of `manual_save`: a manual save
    // that came after the job was made makes the job stale, and the two renames never overlap.
    if manual_saves(path) != job.manual_seen {
        return Ok(Over::Stale); // No need to write a copy that is stale already.
    }
    let staged = fsio::stage_durable(path, Access::KeepExisting, ".autosave.tmp", |w| {
        job.doc.write_pretty(w)
    })?;
    commit_if_fresh(staged, path, job.manual_seen)
}

/// The rename of a staged copy, under the lock of `manual_save`: it happens only if no manual
/// save of the file came after the job was made (`seen`). Otherwise the copy is deleted.
fn commit_if_fresh(staged: fsio::Staged, path: &Path, seen: u64) -> io::Result<Over> {
    let all = MANUAL_SAVES.lock().unwrap_or_else(|e| e.into_inner());
    let now = all
        .as_ref()
        .and_then(|m| m.get(&file_key(path)).copied())
        .unwrap_or(0);
    if now != seen {
        // `staged` is dropped here and deletes the temporary file.
        return Ok(Over::Stale);
    }
    staged.commit()?;
    drop(all);
    Ok(Over::Written)
}

/// The autosave session of this app: the lock and the writer thread.
pub struct Recovery {
    dir: PathBuf,
    id: String,
    /// Held until the end of the session. The system releases it if the process dies.
    #[allow(dead_code)]
    lock: File,
    seq: u64,
    tx: Option<mpsc::Sender<(u64, Job)>>,
    worker: Option<JoinHandle<()>>,
    /// The last error of the writer thread, as text.
    errors: Arc<Mutex<Option<String>>>,
    /// The number of the last job that the writer finished without an error.
    completed: Arc<AtomicU64>,
}

impl Recovery {
    /// Takes the lock and starts the writer thread.
    pub fn start(dir: PathBuf) -> io::Result<Recovery> {
        create_private_dir(&dir)?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let pid = std::process::id();
        let id = format!("{nanos:x}-{pid}");
        // `create_new` fails if the name exists: two apps never share a lock file.
        let mut lock = kadakas_sys::fsio::create_new_private(&dir.join(lock_name(&id)))?;
        let content = json!({"pid": pid, "started_unix": now_unix()}).to_string();
        lock.write_all(content.as_bytes())?;
        lock.sync_all()?;
        lock.try_lock()
            .map_err(|e| io::Error::other(e.to_string()))?;
        let (tx, rx) = mpsc::channel::<(u64, Job)>();
        let errors = Arc::new(Mutex::new(None));
        let completed = Arc::new(AtomicU64::new(0));
        let worker = {
            let (dir, id) = (dir.clone(), id.clone());
            let (errors, completed) = (errors.clone(), completed.clone());
            std::thread::Builder::new()
                .name("kadakas-autosave".into())
                .spawn(move || {
                    for (seq, job) in rx {
                        match write_version(&dir, &id, seq, &job) {
                            Ok(_) => {
                                completed.store(seq, Ordering::SeqCst);
                                *errors.lock().unwrap() = None;
                            }
                            Err(e) => *errors.lock().unwrap() = Some(e.to_string()),
                        }
                    }
                })?
        };
        Ok(Recovery {
            dir,
            id,
            lock,
            seq: 0,
            tx: Some(tx),
            worker: Some(worker),
            errors,
            completed,
        })
    }

    /// Queues a write. Returns the number of the job.
    pub fn submit(&mut self, job: Job) -> u64 {
        self.seq += 1;
        if let Some(tx) = &self.tx {
            let _ = tx.send((self.seq, job));
        }
        self.seq
    }

    /// The error of the last finished write, if it failed.
    pub fn last_error(&self) -> Option<String> {
        self.errors.lock().unwrap().clone()
    }

    /// The number of the last job that is on disk.
    pub fn completed(&self) -> u64 {
        self.completed.load(Ordering::SeqCst)
    }

    pub(crate) fn stop_worker(&mut self) {
        self.tx = None;
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }

    /// Clean exit: waits for the writer, then deletes the versions and the lock file.
    pub fn finish(mut self) {
        self.stop_worker();
        remove_session(&self.dir, &self.id);
        let Recovery { dir, id, lock, .. } = self;
        drop(lock);
        let _ = std::fs::remove_file(dir.join(lock_name(&id)));
    }

    /// Test helper: the process "dies". The lock is released and all files stay.
    #[cfg(test)]
    pub fn simulate_crash(mut self) {
        self.stop_worker();
    }

    #[cfg(test)]
    pub fn id(&self) -> &str {
        &self.id
    }
}

fn remove_session(dir: &Path, id: &str) {
    for (_, p) in list_versions(dir, id) {
        let _ = std::fs::remove_file(p);
    }
    let _ = std::fs::remove_file(dir.join(meta_name(id)));
}

/// An earlier session that ended without a clean exit.
pub struct Crashed {
    dir: PathBuf,
    pub id: String,
    lock: File,
    /// Newest first.
    pub versions: Vec<PathBuf>,
    /// The file of the user that was open, if any.
    pub original: Option<PathBuf>,
    /// Unix time of the last autosave.
    pub saved_unix: Option<u64>,
}

impl Crashed {
    /// Reads the newest version that opens. Returns the document and the file it came from.
    pub fn load(&self) -> Result<(Document, PathBuf), String> {
        let mut last = "no versions".to_string();
        for p in &self.versions {
            match file_io::read_document(p) {
                Ok(d) => return Ok((d, p.clone())),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// The folder with the recovered files.
    pub fn folder(&self) -> &Path {
        &self.dir
    }

    /// Deletes the recovered files and the lock file.
    pub fn discard(self) {
        remove_session(&self.dir, &self.id);
        let Crashed { dir, id, lock, .. } = self;
        drop(lock);
        let _ = std::fs::remove_file(dir.join(lock_name(&id)));
    }
}

use kadakas_sys::process::pid_alive;

/// Finds the sessions that ended without a clean exit, with the default grace time.
pub fn find_crashed(dir: &Path) -> Vec<Crashed> {
    find_crashed_with(dir, LOCK_GRACE)
}

/// A lock file that no process holds is a crashed session when its process id is gone, or when
/// the file is at least `grace` old. Lock files that are stale and have no versions are deleted.
pub fn find_crashed_with(dir: &Path, grace: Duration) -> Vec<Crashed> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".lock"))
        .collect();
    names.sort();
    for name in names {
        let id = name.trim_end_matches(".lock").to_string();
        // Look at the file first, without a lock. The check of the process id starts a program,
        // and a child process must not inherit a lock for a moment.
        let Ok(mut peek) = File::open(dir.join(&name)) else {
            continue;
        };
        let mut text = String::new();
        let _ = peek.read_to_string(&mut text);
        let pid = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["pid"].as_u64())
            .and_then(|p| u32::try_from(p).ok());
        let age = peek
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .unwrap_or(Duration::MAX);
        drop(peek);
        let stale = age >= grace || pid.is_some_and(|p| !pid_alive(p));
        if !stale {
            continue;
        }
        let Ok(lock) = File::options().read(true).write(true).open(dir.join(&name)) else {
            continue;
        };
        // A lock that another process holds belongs to a running app.
        if lock.try_lock().is_err() {
            continue;
        }
        let versions: Vec<PathBuf> = list_versions(dir, &id)
            .into_iter()
            .rev()
            .map(|(_, p)| p)
            .collect();
        if versions.is_empty() {
            drop(lock);
            let _ = std::fs::remove_file(dir.join(&name));
            continue;
        }
        let meta: Value = std::fs::read_to_string(dir.join(meta_name(&id)))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null);
        out.push(Crashed {
            dir: dir.to_path_buf(),
            id,
            lock,
            versions,
            original: meta["original"].as_str().map(PathBuf::from),
            saved_unix: meta["saved_unix"].as_u64(),
        });
    }
    out
}

/// Opens the folder in the file manager of the system.
pub fn reveal(dir: &Path) -> io::Result<()> {
    let program = match std::env::consts::OS {
        "macos" => "open",
        "windows" => "explorer",
        _ => "xdg-open",
    };
    std::process::Command::new(program).arg(dir).spawn()?;
    Ok(())
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;
    use kadakas_core::command::{Command, Editor};
    use kadakas_core::model::{Attributes, Shape};

    /// A pid that no system uses.
    const DEAD_PID: u32 = 4_000_000_000;

    fn temp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("kadakas-autosave-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn doc_with_circles(n: usize) -> Document {
        let mut e = Editor::new(Document::default());
        for i in 0..n {
            e.execute(&Command::AddObject {
                shape: Shape::Circle {
                    center: DVec2::new(i as f64, 0.0),
                    radius: 1.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        }
        e.doc().clone()
    }

    fn job(doc: &Document, original: Option<PathBuf>, keep: usize) -> Job {
        Job {
            doc: doc.clone(),
            original,
            keep,
            over: None,
            manual_seen: 0,
            note: None,
        }
    }

    /// A job that also writes `target`, as the app makes it.
    fn over_job(doc: &Document, target: &Path) -> Job {
        let mut j = job(doc, Some(target.to_path_buf()), 2);
        j.over = Some(target.to_path_buf());
        j.manual_seen = manual_saves(target);
        j
    }

    fn wait_for(mut done: impl FnMut() -> bool) {
        for _ in 0..500 {
            if done() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out");
    }

    #[test]
    fn a_version_is_written_and_read_back() {
        let dir = temp("write");
        let doc = doc_with_circles(3);
        let out = write_version(&dir, "s1", 1, &job(&doc, Some("/a/b.kad".into()), 5)).unwrap();
        assert_eq!(out, Over::NotAsked);
        let text = std::fs::read_to_string(dir.join("s1.000001.autosave.kad")).unwrap();
        assert_eq!(Document::from_json(&text).unwrap(), doc);
        let meta: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("s1.meta.json")).unwrap())
                .unwrap();
        assert_eq!(meta["original"], "/a/b.kad");
        // No temporary files remain.
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn recovery_files_and_folder_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp("mode").join("new").join("recovery");
        let doc = doc_with_circles(1);
        write_version(&dir, "s", 1, &job(&doc, None, 3)).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join("s.000001.autosave.kad")), 0o600);
        assert_eq!(mode(&dir.join("s.meta.json")), 0o600);
        // A session lock is private too.
        let r = Recovery::start(dir.clone()).unwrap();
        let lock = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|e| e == "lock"))
            .unwrap();
        assert_eq!(mode(&lock), 0o600);
        r.finish();
        let _ = std::fs::remove_dir_all(dir.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn rotation_keeps_the_last_versions_of_one_session_only() {
        let dir = temp("rotate");
        let doc = doc_with_circles(1);
        for seq in 1..=7 {
            write_version(&dir, "a", seq, &job(&doc, None, 3)).unwrap();
        }
        write_version(&dir, "b", 1, &job(&doc, None, 3)).unwrap();
        let seqs: Vec<u64> = list_versions(&dir, "a").iter().map(|v| v.0).collect();
        assert_eq!(seqs, vec![5, 6, 7]);
        assert_eq!(list_versions(&dir, "b").len(), 1);
        // `keep` below 1 still keeps one version.
        rotate(&dir, "a", 0);
        assert_eq!(list_versions(&dir, "a").len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn opt_in_writes_the_user_file_but_never_its_backup() {
        let dir = temp("over");
        let target = dir.join("mine.kad");
        std::fs::write(&target, "old").unwrap();
        let doc = doc_with_circles(2);
        let j = over_job(&doc, &target);
        assert_eq!(write_version(&dir, "s", 1, &j).unwrap(), Over::Written);
        assert_eq!(file_io::read_document(&target).unwrap(), doc);
        assert!(
            !dir.join("mine.kad.bak").exists(),
            "autosave must not touch .bak"
        );
        // A real .bak of the user stays as it is.
        std::fs::write(dir.join("mine.kad.bak"), "mine").unwrap();
        write_version(&dir, "s", 2, &j).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("mine.kad.bak")).unwrap(),
            "mine"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_manual_save_after_the_job_makes_the_autosave_stale() {
        let dir = temp("stale");
        let target = dir.join("mine.kad");
        let old = doc_with_circles(1);
        let j = over_job(&old, &target);
        // The user saves a newer document before the writer gets to the job.
        let newer = doc_with_circles(5);
        manual_save(&target, || file_io::write_document_raw(&target, &newer)).unwrap();
        assert_eq!(write_version(&dir, "s", 1, &j).unwrap(), Over::Stale);
        assert_eq!(file_io::read_document(&target).unwrap(), newer);
        // The recovery copy is still written.
        assert_eq!(list_versions(&dir, "s").len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn manual_saves_and_autosaves_never_interleave() {
        let dir = temp("race");
        let target = dir.join("mine.kad");
        let a = doc_with_circles(2);
        let b = doc_with_circles(3);
        std::thread::scope(|s| {
            s.spawn(|| {
                for i in 0..30 {
                    let j = over_job(&a, &target);
                    write_version(&dir, "s", i + 1, &j).unwrap();
                }
            });
            s.spawn(|| {
                for _ in 0..30 {
                    manual_save(&target, || file_io::write_document_raw(&target, &b)).unwrap();
                }
            });
        });
        // Whatever the order, the file is a whole document.
        let got = file_io::read_document(&target).unwrap();
        assert!(got == a || got == b);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_clean_exit_leaves_nothing() {
        let dir = temp("clean");
        let mut r = Recovery::start(dir.clone()).unwrap();
        r.submit(job(&doc_with_circles(1), None, 3));
        r.finish();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        assert!(find_crashed_with(&dir, Duration::ZERO).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_running_session_is_not_reported_but_a_crash_is() {
        let dir = temp("crash");
        let doc = doc_with_circles(4);
        let mut r = Recovery::start(dir.clone()).unwrap();
        let id = r.id().to_string();
        r.submit(job(&doc_with_circles(1), None, 3));
        let last = r.submit(job(&doc, Some("/work/plan.kad".into()), 3));
        wait_for(|| r.completed() >= last);
        assert!(r.last_error().is_none());
        // The lock is still held: another app is running. Nothing to recover.
        assert!(find_crashed_with(&dir, Duration::ZERO).is_empty());
        // The process dies: the lock goes, the files stay.
        r.simulate_crash();
        let found = find_crashed_with(&dir, Duration::ZERO);
        assert_eq!(found.len(), 1);
        let c = &found[0];
        assert_eq!(c.id, id);
        assert_eq!(c.versions.len(), 2);
        assert_eq!(c.original.as_deref(), Some(Path::new("/work/plan.kad")));
        assert!(c.saved_unix.unwrap() <= now_unix());
        // The newest version comes first.
        let (loaded, from) = c.load().unwrap();
        assert_eq!(loaded, doc);
        assert!(from.to_string_lossy().contains("000002"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_fresh_unheld_lock_of_a_live_process_is_a_starting_app_not_a_crash() {
        let dir = temp("starting");
        // The app has made its lock file but not yet taken the lock. It lives (our own pid).
        let lock = format!(r#"{{"pid":{},"started_unix":1}}"#, std::process::id());
        std::fs::write(dir.join("new-1.lock"), lock).unwrap();
        write_version(&dir, "new-1", 1, &job(&doc_with_circles(1), None, 3)).unwrap();
        assert!(find_crashed(&dir).is_empty(), "inside the grace time");
        assert!(dir.join("new-1.lock").exists(), "and the lock file is kept");
        // Once the grace time is over, the lock counts as stale.
        assert_eq!(find_crashed_with(&dir, Duration::ZERO).len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_lock_of_a_dead_process_is_stale_at_once() {
        let dir = temp("deadpid");
        let lock = format!(r#"{{"pid":{DEAD_PID},"started_unix":1}}"#);
        std::fs::write(dir.join("old-1.lock"), lock).unwrap();
        write_version(&dir, "old-1", 1, &job(&doc_with_circles(1), None, 3)).unwrap();
        assert_eq!(find_crashed(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn recovery_falls_back_to_an_older_version_and_discard_cleans_up() {
        let dir = temp("fallback");
        let doc = doc_with_circles(2);
        let mut r = Recovery::start(dir.clone()).unwrap();
        let id = r.id().to_string();
        r.submit(job(&doc, None, 5));
        r.submit(job(&doc_with_circles(3), None, 5));
        // Finish the writes, then damage the newest version.
        r.stop_worker();
        std::fs::write(dir.join(version_name(&id, 2)), "{broken").unwrap();
        r.simulate_crash();
        let mut found = find_crashed_with(&dir, Duration::ZERO);
        let c = found.pop().unwrap();
        assert_eq!(c.load().unwrap().0, doc);
        assert_eq!(c.folder(), dir);
        c.discard();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_stale_lock_without_versions_is_cleaned_silently() {
        let dir = temp("marker");
        let lock = format!(r#"{{"pid":{DEAD_PID}}}"#);
        std::fs::write(dir.join("dead-1.lock"), lock).unwrap();
        assert!(find_crashed(&dir).is_empty());
        assert!(!dir.join("dead-1.lock").exists());
        // A folder that does not exist has nothing to recover.
        assert!(find_crashed(&dir.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_crash_marker_file_with_versions_counts_as_a_crash() {
        let dir = temp("simulated");
        std::fs::write(dir.join("old-9.lock"), "").unwrap();
        let doc = doc_with_circles(1);
        write_version(&dir, "old-9", 1, &job(&doc, None, 5)).unwrap();
        // An old marker without content: the grace time decides.
        let found = find_crashed_with(&dir, Duration::ZERO);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].load().unwrap().0, doc);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn two_sessions_get_two_lock_files() {
        let dir = temp("two");
        let a = Recovery::start(dir.clone()).unwrap();
        let b = Recovery::start(dir.clone()).unwrap();
        assert_ne!(a.id(), b.id());
        a.finish();
        b.finish();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn preferences_are_saved_next_to_the_keymap() {
        if let Some(f) = preferences_file() {
            assert_eq!(f.file_name().unwrap(), "preferences.json");
        }
    }

    #[test]
    fn a_manual_save_during_the_write_of_the_copy_makes_the_copy_stale() {
        let dir = temp("late-manual");
        let target = dir.join("late.kad");
        std::fs::write(&target, "mine").unwrap();
        let seen = manual_saves(&target);
        let staged = fsio::stage_durable(&target, Access::KeepExisting, ".autosave.tmp", |w| {
            w.write_all(b"copy")
        })
        .unwrap();
        // The manual save comes after the copy was staged and before its rename.
        manual_save(&target, || std::fs::write(&target, "newer save").unwrap());
        assert_eq!(commit_if_fresh(staged, &target, seen).unwrap(), Over::Stale);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "newer save");
        assert!(!dir.join("late.kad.autosave.tmp").exists());
        let staged = fsio::stage_durable(&target, Access::KeepExisting, ".autosave.tmp", |w| {
            w.write_all(b"copy")
        })
        .unwrap();
        let seen = manual_saves(&target);
        assert_eq!(
            commit_if_fresh(staged, &target, seen).unwrap(),
            Over::Written
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "copy");
    }
}
