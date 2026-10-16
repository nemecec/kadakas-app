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

use kadakas_file::SeenFiles;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kadakas_core::model::Document;
use kadakas_l10n::tr;
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
    /// The files the document was opened from. Their pictures are kept (also those that only an
    /// undo step holds), and a save over one of them moves them to the new file. Without them a
    /// save over the file of the person would leave the undo steps without their pictures.
    pub sources: Vec<Arc<kadakas_file::FileSource>>,
    /// The drawing was edited and its versions wait in the file: the drawing of the file is
    /// written as their anchor.
    pub anchor: bool,
    /// The files that the session saw. The save over the file of the person checks the file
    /// against it (any form), and notes its own write.
    pub seen: Option<SeenFiles>,
}

/// What `write_version` did with the file of the user.
#[derive(Debug, PartialEq, Eq)]
pub enum Over {
    NotAsked,
    Written,
    /// A manual save came after the job was made. The file was not touched.
    Stale,
    /// Pictures or versions of the document cannot be found in the file they stay in (it changed
    /// on disk). The file of the person was not touched: a copy with placeholders never replaces
    /// it without the person.
    Missing(kadakas_file::Missing),
    /// Another program changed the file of the person since the document was opened from it (two
    /// instances, a sync conflict copy). The file was not touched: the recovery copy is on disk,
    /// and the person decides at the next Save.
    Changed(PathBuf),
}

/// What the recovery copy had to leave out or change.
#[derive(Default)]
struct CopyReport {
    /// What could not be found in the file the document was opened from. The copy has
    /// placeholders for the pictures and no versions that wait in that file.
    missing: Option<kadakas_file::Missing>,
    /// The copy is the plain JSON form, because the container form would be over a limit.
    json: Option<String>,
    /// Other things to tell (the versions that could not be read are not in the JSON form).
    notes: Vec<String>,
}

/// The copy of the document for the recovery folder. It never depends on the file of the person:
/// it writes what it can, and records what it could not (the meta file has it). A copy of a
/// document over the limits of the container form is written as the plain JSON form, so there
/// is always a copy.
fn write_recovery_copy(dir: &Path, id: &str, seq: u64, job: &Job) -> io::Result<CopyReport> {
    let path = dir.join(version_name(id, seq));
    let mut report = CopyReport::default();
    let mut patched: Option<Document> = None;
    let mut skip = false;
    loop {
        let doc: &Document = patched.as_ref().unwrap_or(&job.doc);
        let opts = kadakas_file::WriteOptions {
            generator: kadakas_file::generator(),
            keep: job.sources.clone(),
            anchor: job.anchor && doc.lazy_history.is_some(),
            skip_missing: skip,
            allow_overwrite: false,
        };
        let failure = std::cell::RefCell::new(None);
        let written = write_durable_with(&path, Access::Private, ".tmp", |w| {
            kadakas_file::write_container(w, doc, &opts)
                .map(|_| ())
                .map_err(|e| {
                    let io = io::Error::other(e.to_string());
                    *failure.borrow_mut() = Some(e);
                    io
                })
        });
        match (written, failure.into_inner()) {
            (Ok(()), _) => return Ok(report),
            (Err(_), Some(kadakas_file::FileError::Missing(m))) if !skip => {
                // Placeholders for the pictures that are gone and no versions that wait in the
                // file that changed. The meta file says so.
                let mut d = doc.clone();
                let set: std::collections::HashSet<String> = m.pictures.iter().cloned().collect();
                d.replace_missing_pictures(&set);
                if m.history {
                    d.history = None;
                    d.lazy_history = None;
                }
                patched = Some(d);
                report.missing = Some(m);
                skip = true;
            }
            (Err(_), Some(kadakas_file::FileError::Limit(why))) => {
                report.json = Some(why);
                return write_json_copy(&path, doc, &mut report).map(|()| report);
            }
            (Err(e), _) => return Err(e),
        }
    }
}

/// The plain JSON form as the recovery copy (the name of the file is the same: a reader finds the
/// form by the first bytes). The versions that wait in the file are read as they are.
fn write_json_copy(path: &Path, doc: &Document, report: &mut CopyReport) -> io::Result<()> {
    let mut d = doc.clone();
    let unreadable = d.unreadable_pictures();
    if !unreadable.is_empty() {
        let set: std::collections::HashSet<String> = unreadable.iter().cloned().collect();
        d.replace_missing_pictures(&set);
        report.missing.get_or_insert_with(Default::default).pictures = unreadable;
    }
    // The versions that wait in the file are read only up to the bound of a session (the same bound
    // as the Versions window): a history over it is not read into memory for a recovery copy.
    let cap = usize::try_from(kadakas_api::SessionLimits::default().history_read_bytes)
        .unwrap_or(usize::MAX)
        .saturating_mul(kadakas_review::history::JSON_FACTOR);
    let too_big = d
        .lazy_history
        .as_ref()
        .map(|l| l.bytes())
        .filter(|b| *b > cap && d.history.is_none());
    if let Some(bytes) = too_big {
        d.lazy_history = None;
        report.notes.push(format!(
            "the versions are not in this copy: they have {bytes} bytes, more than is read"
        ));
        report.missing.get_or_insert_with(Default::default).history = true;
    } else if let Err(e) = kadakas_file::resolve_history(&mut d) {
        d.lazy_history = None;
        d.history = None;
        report
            .notes
            .push(format!("the versions are not in this copy: {e}"));
        report.missing.get_or_insert_with(Default::default).history = true;
    }
    write_durable_with(path, Access::Private, ".tmp", |w| {
        kadakas_file::write_json(w, &d).map_err(|e| io::Error::other(e.to_string()))
    })
}

/// Writes one version, its meta file, and rotates. Runs on the writer thread.
pub fn write_version(dir: &Path, id: &str, seq: u64, job: &Job) -> io::Result<Over> {
    create_private_dir(dir)?;
    // The copy for the recovery is a container, like the file of the person: the pictures that are
    // still in the file of the person are copied from there without decoding them.
    let copy = write_recovery_copy(dir, id, seq, job)?;
    let secs = now_unix();
    let mut note = job.note.clone().unwrap_or_default();
    let mut add = |text: String| {
        if !note.is_empty() {
            note.push(' ');
        }
        note.push_str(&text);
    };
    if let Some(m) = &copy.missing {
        add(tr!(
            "autosave-copy-missing",
            file = m.file.clone(),
            what = missing_text(m)
        ));
    }
    if let Some(why) = &copy.json {
        add(tr!("autosave-copy-as-json", why = why.clone()));
    }
    for n in &copy.notes {
        add(n.clone());
    }
    let meta = json!({
        "original": job.original.as_ref().map(|p| p.display().to_string()),
        "saved_unix": secs,
        "note": (!note.is_empty()).then_some(note),
        "missing": copy.missing.as_ref().map(|m| json!({
            "file": m.file, "pictures": m.pictures, "history": m.history, "other": m.other,
        })),
        "form": if copy.json.is_some() { "json" } else { "container" },
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
    // The file at the path must still be the file that the document was opened from.
    if changed_by_another(path, &job.sources, job.seen.as_ref()) {
        return Ok(Over::Changed(path.clone()));
    }
    // The form follows the extension of the file of the person, like Save.
    let form = kadakas_file::Form::from_extension(path);
    let mut report = None;
    let failure = std::cell::RefCell::new(None);
    let staged = fsio::stage_durable(path, Access::KeepExisting, ".autosave.tmp", |w| {
        let fail = |e: kadakas_file::FileError| {
            let io = io::Error::other(e.to_string());
            *failure.borrow_mut() = Some(e);
            io
        };
        match form {
            kadakas_file::Form::Json => {
                // The plain form holds the versions inside: the ones that wait are read now.
                let mut d;
                let doc = if job.doc.lazy_history.is_some() && job.doc.history.is_none() {
                    d = job.doc.clone();
                    kadakas_file::resolve_history(&mut d).map_err(fail)?;
                    &d
                } else {
                    &job.doc
                };
                kadakas_file::write_json(w, doc).map_err(fail)
            }
            kadakas_file::Form::Container => {
                let opts = kadakas_file::WriteOptions {
                    generator: kadakas_file::generator(),
                    keep: job.sources.clone(),
                    anchor: job.anchor,
                    skip_missing: false,
                    allow_overwrite: false,
                };
                kadakas_file::write_container(w, &job.doc, &opts)
                    .map(|(_, r)| report = Some(r))
                    .map_err(fail)
            }
        }
    });
    // Looked at once more before the rename: the copy took time.
    if changed_by_another(path, &job.sources, job.seen.as_ref()) {
        return Ok(Over::Changed(path.clone()));
    }
    let staged = match (staged, failure.into_inner()) {
        (Ok(s), _) => s,
        (Err(_), Some(kadakas_file::FileError::Missing(m))) => return Ok(Over::Missing(m)),
        (Err(e), _) => return Err(e),
    };
    // The file under the pictures of the document is a new file at the moment of the rename:
    // the sources move to it under the same lock, so a manual save cannot come in between.
    commit_if_fresh(staged, path, job.manual_seen, || {
        if let Some(report) = &report {
            let _ = kadakas_file::native::rebind_sources(report, path);
        }
    })
    .inspect(|over| {
        // The file is what this program wrote: it is not "changed by another program" later.
        if *over == Over::Written
            && let Some(seen) = &job.seen
        {
            seen.note(path);
        }
    })
}

/// True if a source of the document is the file at `path` and the file has another content now.
fn changed_by_another(
    path: &Path,
    sources: &[Arc<kadakas_file::FileSource>],
    seen: Option<&SeenFiles>,
) -> bool {
    if !path.exists() {
        return false;
    }
    if seen.is_some_and(|s| s.changed(path)) {
        return true;
    }
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    sources.iter().any(|s| {
        s.path()
            .map(|p| std::fs::canonicalize(&p).unwrap_or(p))
            .is_some_and(|p| p == target)
            && !s.same_file()
    })
}

/// The text of what is missing, for a note.
pub(crate) fn missing_text(m: &kadakas_file::Missing) -> String {
    let mut what = Vec::new();
    if !m.pictures.is_empty() {
        what.push(tr!("autosave-missing-pictures", count = m.pictures.len()));
    }
    if m.history {
        what.push(tr!("autosave-missing-versions"));
    }
    if !m.other.is_empty() {
        what.push(tr!("autosave-missing-other", count = m.other.len()));
    }
    what.join(", ")
}

/// The rename of a staged copy, under the lock of `manual_save`: it happens only if no manual
/// save of the file came after the job was made (`seen`). Otherwise the copy is deleted. `after`
/// runs under the same lock, after the rename.
fn commit_if_fresh(
    staged: fsio::Staged,
    path: &Path,
    seen: u64,
    after: impl FnOnce(),
) -> io::Result<Over> {
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
    after();
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
                            Ok(Over::Changed(file)) => {
                                completed.store(seq, Ordering::SeqCst);
                                *errors.lock().unwrap() = Some(tr!(
                                    "autosave-over-changed",
                                    file = file.display().to_string()
                                ));
                            }
                            Ok(Over::Missing(m)) => {
                                // The copy is on disk; the file of the person was left alone.
                                completed.store(seq, Ordering::SeqCst);
                                *errors.lock().unwrap() = Some(tr!(
                                    "autosave-over-missing",
                                    file = m.file.clone(),
                                    what = missing_text(&m)
                                ));
                            }
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
            sources: Vec::new(),
            anchor: false,
            seen: None,
        }
    }

    /// A job that also writes `target`, as the app makes it.
    fn over_job(doc: &Document, target: &Path) -> Job {
        let mut j = job(doc, Some(target.to_path_buf()), 2);
        j.over = Some(target.to_path_buf());
        j.manual_seen = manual_saves(target);
        j
    }

    /// A document with an underlay whose picture is big enough to be a blob of a container.
    fn doc_with_picture() -> Document {
        use kadakas_core::underlay::{PngData, Underlay};
        let mut doc = doc_with_circles(2);
        let mut png = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
        png.extend((0..20_000u32).map(|i| i.wrapping_mul(2_654_435_761) as u8));
        let u = Underlay {
            name: "map".into(),
            min: DVec2::ZERO,
            max: DVec2::new(100.0, 100.0),
            width: 10,
            height: 10,
            png: PngData::new(png),
            opacity: 1.0,
            source: String::new(),
        };
        let mut e = Editor::new(doc);
        e.execute(&Command::AddObject {
            shape: Shape::Underlay(Box::new(u)),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        doc = e.doc().clone();
        doc
    }

    #[test]
    fn the_copy_over_my_file_is_a_container_and_the_pictures_keep_reading_from_it() {
        let dir = temp("over-lazy");
        let target = dir.join("mine.kad");
        let original = doc_with_picture();
        file_io::write_document_raw(&target, &original).unwrap();
        // The document of the person: opened lazily, so its picture is in the file.
        let opened = file_io::open_file(&target).unwrap();
        assert!(opened.source.is_some());
        let doc = opened.doc;
        assert_eq!(doc, original);
        // The autosave writes the file of the person (a container, copying the picture from
        // the file that it replaces) and a recovery copy.
        let seen = manual_saves(&target);
        let mut j = job(&doc, Some(target.clone()), 2);
        j.over = Some(target.clone());
        j.manual_seen = seen;
        let out = write_version(&dir.join("rec"), "s1", 1, &j).unwrap();
        assert_eq!(out, Over::Written);
        assert!(std::fs::read(&target).unwrap().starts_with(b"PK"));
        let copy = dir.join("rec").join("s1.000001.autosave.kad");
        assert!(std::fs::read(&copy).unwrap().starts_with(b"PK"));
        assert_eq!(file_io::read_document(&copy).unwrap(), original);
        // The picture of the open document is read from the new file, not reported as changed.
        assert_eq!(doc, original);
        assert_eq!(file_io::read_document(&target).unwrap(), original);
        // The form of the copy follows the extension: a JSON file gets the plain form.
        let json_target = dir.join("mine.json");
        file_io::write_document_raw(&json_target, &original).unwrap();
        let mut j = job(&doc, Some(json_target.clone()), 2);
        j.over = Some(json_target.clone());
        j.manual_seen = manual_saves(&json_target);
        assert_eq!(
            write_version(&dir.join("rec"), "s1", 2, &j).unwrap(),
            Over::Written
        );
        assert!(std::fs::read(&json_target).unwrap().starts_with(b"{"));
        assert_eq!(file_io::read_document(&json_target).unwrap(), original);
        let _ = std::fs::remove_dir_all(dir);
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
        // The copy is a container; it reads back through the same door as every other file.
        let copy = dir.join("s1.000001.autosave.kad");
        assert!(std::fs::read(&copy).unwrap().starts_with(b"PK"));
        assert_eq!(file_io::read_document(&copy).unwrap(), doc);
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
        assert_eq!(
            commit_if_fresh(staged, &target, seen, || ()).unwrap(),
            Over::Stale
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "newer save");
        assert!(!dir.join("late.kad.autosave.tmp").exists());
        let staged = fsio::stage_durable(&target, Access::KeepExisting, ".autosave.tmp", |w| {
            w.write_all(b"copy")
        })
        .unwrap();
        let seen = manual_saves(&target);
        assert_eq!(
            commit_if_fresh(staged, &target, seen, || ()).unwrap(),
            Over::Written
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "copy");
    }

    /// A session on a file with a picture, as the app has it.
    fn session_on(path: &Path) -> kadakas_api::Session {
        let opened = file_io::open_file(path).unwrap();
        let mut session = kadakas_api::Session::with_document(opened.doc);
        session.file_source = opened.source;
        session
    }

    fn underlay_id(session: &kadakas_api::Session) -> kadakas_core::model::ObjectId {
        session
            .editor
            .doc()
            .objects
            .iter()
            .find(|(_, o)| matches!(o.shape, Shape::Underlay(_)))
            .map(|(id, _)| *id)
            .unwrap()
    }

    /// The job that the app makes for a session.
    fn session_job(session: &mut kadakas_api::Session, target: &Path) -> Job {
        let anchor = session.needs_anchor();
        // As the app makes it: the drawing as it is when the session cannot make the file form.
        let doc = match session.document_for_save_in(true) {
            Ok(doc) => doc.into_owned(),
            Err(_) => session.editor.doc().clone(),
        };
        let mut j = over_job(&doc, target);
        j.sources = session.file_source.iter().cloned().collect();
        j.anchor = anchor;
        j
    }

    #[test]
    fn an_autosave_over_my_file_keeps_the_picture_that_only_an_undo_step_holds() {
        // B2: open with an underlay, delete it, autosave over the file, undo, save, reopen.
        let dir = temp("over-undo");
        let target = dir.join("mine.kad");
        let original = doc_with_picture();
        file_io::write_document_raw(&target, &original).unwrap();
        let mut session = session_on(&target);
        let id = underlay_id(&session);
        session
            .editor
            .execute(&Command::DeleteObjects { ids: vec![id] })
            .unwrap();
        let job = session_job(&mut session, &target);
        assert_eq!(
            write_version(&dir.join("rec"), "s1", 1, &job).unwrap(),
            Over::Written
        );
        // The file has no underlay in its drawing, but still the picture: the undo step needs it.
        assert!(session.editor.undo());
        let spec = file_io::SaveSpec {
            anchor: session.needs_anchor(),
            ..file_io::SaveSpec::from_extension(
                &target,
                session.file_source.iter().cloned().collect(),
            )
        };
        let doc = session.document_for_save_in(true).unwrap();
        file_io::write_document_as(&target, &doc, &spec).unwrap();
        drop(doc);
        let back = file_io::read_document(&target).unwrap();
        assert_eq!(back.objects, original.objects);
        let (before, after) = (underlay_png(&original), underlay_png(&back));
        assert_eq!(before, after, "the picture is there");
    }

    fn underlay_png(doc: &Document) -> Vec<u8> {
        doc.objects
            .values()
            .find_map(|o| match &o.shape {
                Shape::Underlay(u) => Some((*u.png.bytes().unwrap()).clone()),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn the_recovery_copy_and_the_autosave_over_my_file_survive_a_changed_file() {
        // B1: touch, rename, replace and delete the file under the open document, then autosave.
        for change in [
            "touch",
            "rename",
            "replace",
            "replace-same",
            "delete",
            "delete-bak",
        ] {
            let dir = temp(&format!("changed-{change}"));
            let target = dir.join("mine.kad");
            let original = doc_with_picture();
            file_io::write_document_raw(&target, &original).unwrap();
            let mut session = session_on(&target);
            // An edit (the picture stays in the drawing, so the copies need it).
            session
                .editor
                .execute(&Command::AddObject {
                    shape: Shape::Circle {
                        center: DVec2::new(5.0, 5.0),
                        radius: 1.0,
                    },
                    layer: None,
                    class: None,
                    attrs: Attributes::default(),
                })
                .unwrap();
            let old = std::fs::read(&target).unwrap();
            std::thread::sleep(Duration::from_millis(30));
            match change {
                "touch" => std::fs::write(&target, &old).unwrap(),
                "rename" => std::fs::rename(&target, dir.join("moved.kad")).unwrap(),
                "replace" => {
                    file_io::write_document_raw(&target, &doc_with_circles(5)).unwrap();
                    let _ = std::fs::remove_file(dir.join("mine.kad.bak"));
                }
                // The same pictures in another file.
                "replace-same" => {
                    let mut e = Editor::new(original.clone());
                    e.execute(&Command::AddObject {
                        shape: Shape::Circle {
                            center: DVec2::ZERO,
                            radius: 9.0,
                        },
                        layer: None,
                        class: None,
                        attrs: Attributes::default(),
                    })
                    .unwrap();
                    file_io::write_document_raw(&target, e.doc()).unwrap();
                    let _ = std::fs::remove_file(dir.join("mine.kad.bak"));
                }
                "delete" => std::fs::remove_file(&target).unwrap(),
                _ => {
                    std::fs::write(dir.join("mine.kad.bak"), &old).unwrap();
                    std::fs::remove_file(&target).unwrap();
                }
            }
            // What the copy of the recovery folder can find, and what the autosave over the file
            // does: it replaces the file only if it is still the file that was opened.
            let finds = matches!(change, "touch" | "replace-same" | "delete-bak");
            let theirs = std::fs::read(&target).ok();
            let job = session_job(&mut session, &target);
            let rec = dir.join("rec");
            // The recovery copy is always written, whatever happened to the file.
            let out =
                write_version(&rec, "s1", 1, &job).unwrap_or_else(|e| panic!("{change}: {e}"));
            let copy = rec.join("s1.000001.autosave.kad");
            assert!(copy.exists(), "{change}");
            let meta: Value =
                serde_json::from_slice(&std::fs::read(rec.join("s1.meta.json")).unwrap()).unwrap();
            if finds {
                assert!(meta["missing"].is_null(), "{change}: {meta}");
            } else {
                // The copy records what it could not find.
                assert!(
                    meta["missing"]["pictures"]
                        .as_array()
                        .is_some_and(|a| !a.is_empty()),
                    "{change}: {meta}"
                );
                assert!(!meta["note"].is_null(), "{change}: {meta}");
            }
            assert_eq!(
                file_io::read_document(&copy).unwrap().objects.len(),
                4,
                "{change}"
            );
            match change {
                "touch" | "delete-bak" => {
                    assert_eq!(out, Over::Written, "{change}");
                    assert_eq!(file_io::read_document(&target).unwrap().objects.len(), 4);
                }
                // Another program wrote the file: the file of the person is left alone.
                "replace" | "replace-same" => {
                    assert!(matches!(out, Over::Changed(_)), "{change}: {out:?}");
                    assert_eq!(std::fs::read(&target).ok(), theirs, "{change}");
                }
                // The file is gone, and the picture is nowhere: the file is not written with a
                // placeholder.
                _ => assert!(matches!(out, Over::Missing(_)), "{change}: {out:?}"),
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn the_json_copy_has_the_versions_that_wait_in_the_file_and_placeholders() {
        // The plain JSON form as the recovery copy: the versions that wait in the file are read
        // as they are, and a picture that is gone becomes a placeholder.
        let dir = temp("json-copy-small");
        let target = dir.join("mine.kad");
        let mut s = kadakas_api::Session::with_document(doc_with_picture());
        s.handle("version.create", serde_json::json!({"name": "v1"}))
            .unwrap();
        let saved = s.document_for_save_in(true).unwrap().into_owned();
        file_io::write_document_raw(&target, &saved).unwrap();
        let opened = file_io::open_file(&target).unwrap();
        assert!(opened.doc.lazy_history.is_some());
        let mut report = CopyReport::default();
        let json = dir.join("copy.json");
        write_json_copy(&json, &opened.doc, &mut report).unwrap();
        assert!(report.missing.is_none());
        let back = file_io::read_document(&json).unwrap();
        assert_eq!(
            back.history.as_ref().and_then(|h| h.version_count()),
            Some(1)
        );
        // The file goes away: the picture and the versions cannot be found.
        std::fs::remove_file(&target).unwrap();
        let mut report = CopyReport::default();
        write_json_copy(&json, &opened.doc, &mut report).unwrap();
        let missing = report.missing.expect("recorded");
        assert!(!missing.pictures.is_empty() && missing.history);
        let back = file_io::read_document(&json).unwrap();
        assert!(back.history.is_none());
        assert_eq!(
            underlay_png(&back),
            kadakas_core::underlay::placeholder_png()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "heavy: a picture of 100 MiB; run with --ignored (best in release mode)"]
    fn a_copy_over_the_limits_of_the_container_is_the_plain_json_form() {
        // S5: a picture of more than 128 MiB as text cannot be in a container. The recovery copy is
        // written as JSON, so that there is a copy.
        use kadakas_core::underlay::{PngData, Underlay};
        let dir = temp("json-copy");
        let mut bytes = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
        bytes.resize(100 * 1024 * 1024, 7);
        let mut doc = doc_with_circles(1);
        let mut e = Editor::new(std::mem::take(&mut doc));
        e.execute(&Command::AddObject {
            shape: Shape::Underlay(Box::new(Underlay {
                name: "huge".into(),
                min: DVec2::ZERO,
                max: DVec2::new(10.0, 10.0),
                width: 10,
                height: 10,
                png: PngData::new(bytes),
                opacity: 1.0,
                source: String::new(),
            })),
            layer: None,
            class: None,
            attrs: Attributes::default(),
        })
        .unwrap();
        let j = job(e.doc(), None, 2);
        write_version(&dir.join("rec"), "s1", 1, &j).unwrap();
        let copy = dir.join("rec").join("s1.000001.autosave.kad");
        assert!(
            std::fs::read(&copy).unwrap().starts_with(b"{"),
            "plain JSON"
        );
        let meta: Value =
            serde_json::from_slice(&std::fs::read(dir.join("rec").join("s1.meta.json")).unwrap())
                .unwrap();
        assert_eq!(meta["form"], "json");
        assert!(meta["note"].as_str().unwrap().contains("limit"), "{meta}");
        drop(e);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_copies_of_an_edited_document_keep_the_versions_that_wait_in_the_file() {
        // S1: the versions were not used, the drawing was edited, and the autosave writes the
        // recovery copy and the file of the person. The versions are in both, with the drawing
        // they belong to, and read without a note.
        let dir = temp("copies-waiting");
        let target = dir.join("mine.kad");
        let mut first = kadakas_api::Session::with_document(doc_with_picture());
        first
            .handle("version.create", serde_json::json!({"name": "v1"}))
            .unwrap();
        let saved = first.document_for_save_in(true).unwrap().into_owned();
        file_io::write_document_raw(&target, &saved).unwrap();
        let mut session = session_on(&target);
        assert!(session.editor.doc().lazy_history.is_some());
        session
            .editor
            .execute(&Command::AddObject {
                shape: Shape::Circle {
                    center: DVec2::new(9.0, 9.0),
                    radius: 3.0,
                },
                layer: None,
                class: None,
                attrs: Attributes::default(),
            })
            .unwrap();
        assert!(session.needs_anchor());
        let job = session_job(&mut session, &target);
        assert!(job.anchor);
        assert_eq!(
            write_version(&dir.join("rec"), "s1", 1, &job).unwrap(),
            Over::Written
        );
        for copy in [
            dir.join("rec").join("s1.000001.autosave.kad"),
            target.clone(),
        ] {
            let opened = file_io::open_file(&copy).unwrap();
            let mut back = kadakas_api::Session::with_document(opened.doc);
            back.file_source = opened.source;
            let list = back
                .handle("version.list", serde_json::json!({"current": false}))
                .unwrap();
            assert_eq!(list["count"], 1, "{copy:?}: {list}");
            assert_eq!(list["read_only"], false, "{copy:?}: {list}");
            assert!(back.load_notes().is_empty(), "{:?}", back.load_notes());
        }
    }
}
