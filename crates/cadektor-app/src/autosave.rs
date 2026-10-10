//! Autosave and crash recovery.
//!
//! The app serializes the document on the UI thread (one compact JSON string) and hands the bytes
//! to a worker thread. The worker writes them atomically to a per-user recovery folder, never to the
//! file of the user (unless the user opts in). The last K versions stay; older ones are deleted.
//!
//! Each running app holds a lock file in the recovery folder with an exclusive file lock. The
//! operating system drops the lock when the process dies for any reason. At the next start, a lock
//! file that nobody holds means an unclean shutdown, and the autosaved versions next to it are
//! offered to the user. A clean exit deletes the lock file and the versions.

use serde_json::{Value, json};

/// The preferences for autosave. `sanitized` keeps the values in a sensible range.
#[derive(Clone, Debug, PartialEq)]
pub struct AutosaveSettings {
    pub enabled: bool,
    /// Save after this many minutes with changes.
    pub interval_minutes: u32,
    /// Save after this many operations (document revisions). 0 turns this trigger off.
    pub every_ops: u32,
    /// How many versions to keep.
    pub keep: u32,
    /// Also save over the file of the user. Off unless the user chooses it.
    pub save_over_file: bool,
}

impl Default for AutosaveSettings {
    fn default() -> Self {
        AutosaveSettings {
            enabled: true,
            interval_minutes: 5,
            every_ops: 50,
            keep: 5,
            save_over_file: false,
        }
    }
}

impl AutosaveSettings {
    pub const MAX_KEEP: u32 = 50;
    pub const MAX_MINUTES: u32 = 24 * 60;

    pub fn sanitized(mut self) -> Self {
        self.interval_minutes = self.interval_minutes.clamp(1, Self::MAX_MINUTES);
        self.keep = self.keep.clamp(1, Self::MAX_KEEP);
        self
    }

    pub fn to_json(&self) -> Value {
        json!({
            "autosave": {
                "enabled": self.enabled,
                "interval_minutes": self.interval_minutes,
                "every_ops": self.every_ops,
                "keep": self.keep,
                "save_over_file": self.save_over_file,
            }
        })
    }

    /// Reads the settings from the text of the preferences file. Missing or wrong values keep
    /// their defaults.
    pub fn from_json(text: &str) -> Self {
        let d = Self::default();
        let Ok(v) = serde_json::from_str::<Value>(text) else {
            return d;
        };
        let a = &v["autosave"];
        let num = |k: &str, def: u32| {
            a[k].as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(def)
        };
        AutosaveSettings {
            enabled: a["enabled"].as_bool().unwrap_or(d.enabled),
            interval_minutes: num("interval_minutes", d.interval_minutes),
            every_ops: num("every_ops", d.every_ops),
            keep: num("keep", d.keep),
            save_over_file: a["save_over_file"].as_bool().unwrap_or(d.save_over_file),
        }
        .sanitized()
    }
}

/// Decides when the next autosave is due. Time is in seconds from any fixed start.
#[derive(Debug, Default)]
pub struct Scheduler {
    saved_revision: u64,
    saved_at: f64,
}

impl Scheduler {
    /// A document that is already on disk (or empty) at `now`, with this revision.
    pub fn new(now: f64, revision: u64) -> Self {
        Scheduler {
            saved_revision: revision,
            saved_at: now,
        }
    }

    /// True if the document changed and either the time or the operation count is reached.
    pub fn due(&self, s: &AutosaveSettings, now: f64, revision: u64) -> bool {
        if !s.enabled || revision == self.saved_revision {
            return false;
        }
        let by_time = now - self.saved_at >= f64::from(s.interval_minutes) * 60.0;
        let ops = revision.abs_diff(self.saved_revision);
        let by_ops = s.every_ops > 0 && ops >= u64::from(s.every_ops);
        by_time || by_ops
    }

    pub fn mark(&mut self, now: f64, revision: u64) {
        self.saved_revision = revision;
        self.saved_at = now;
    }
}

/// "5 minutes ago" for the recovery dialog.
pub fn age_text(secs: u64) -> String {
    let plural = |n: u64, unit: &str| format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match secs {
        0..=59 => "less than a minute ago".into(),
        60..=3599 => plural(secs / 60, "minute"),
        3600..=86399 => plural(secs / 3600, "hour"),
        _ => plural(secs / 86400, "day"),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::fs::File;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::thread::JoinHandle;
    use std::time::{SystemTime, UNIX_EPOCH};

    use cadektor_core::fsio;
    use cadektor_core::model::Document;
    use serde_json::{Value, json};

    use super::AutosaveSettings;
    use crate::file_io;

    const VERSION_SUFFIX: &str = ".cdk.json";

    /// The folder for recovery files of this user.
    /// `CADEKTOR_RECOVERY_DIR` overrides it. Windows: `%LOCALAPPDATA%\cadektor\recovery`.
    /// macOS: `~/Library/Application Support/cadektor/recovery`.
    /// Elsewhere: `$XDG_STATE_HOME/cadektor/recovery` or `~/.local/state/cadektor/recovery`.
    pub fn recovery_dir() -> Option<PathBuf> {
        recovery_dir_for(std::env::consts::OS, |k| {
            std::env::var_os(k).filter(|v| !v.is_empty())
        })
    }

    pub(crate) fn recovery_dir_for(
        os: &str,
        env: impl Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Option<PathBuf> {
        if let Some(p) = env("CADEKTOR_RECOVERY_DIR") {
            return Some(PathBuf::from(p));
        }
        let dir = match os {
            "windows" => PathBuf::from(env("LOCALAPPDATA")?),
            "macos" => PathBuf::from(env("HOME")?)
                .join("Library")
                .join("Application Support"),
            _ => match env("XDG_STATE_HOME") {
                Some(x) => PathBuf::from(x),
                None => PathBuf::from(env("HOME")?).join(".local").join("state"),
            },
        };
        Some(dir.join("cadektor").join("recovery"))
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

    /// One piece of work for the writer thread.
    pub struct Job {
        pub bytes: Vec<u8>,
        /// The path of the document of the user, if it has one. Saved in the meta file.
        pub original: Option<PathBuf>,
        pub keep: usize,
        /// Also write the bytes here (the opt-in "save over my file").
        pub over: Option<PathBuf>,
    }

    /// Writes a file atomically (temporary file, then rename), without a `.bak` copy.
    fn write_plain(path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        let result = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }

    /// Writes one version, its meta file, and rotates. Runs on the worker thread.
    pub fn write_version(dir: &Path, id: &str, seq: u64, job: &Job) -> io::Result<()> {
        std::fs::create_dir_all(dir)?;
        write_plain(&dir.join(version_name(id, seq)), &job.bytes)?;
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let meta = json!({
            "original": job.original.as_ref().map(|p| p.display().to_string()),
            "saved_unix": secs,
        });
        write_plain(&dir.join(meta_name(id)), meta.to_string().as_bytes())?;
        rotate(dir, id, job.keep);
        if let Some(p) = &job.over {
            fsio::write_atomic(p, &job.bytes)?;
        }
        Ok(())
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
        errors: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    }

    impl Recovery {
        /// Takes the lock and starts the writer thread.
        pub fn start(dir: PathBuf) -> io::Result<Recovery> {
            std::fs::create_dir_all(&dir)?;
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let id = format!("{nanos:x}-{}", std::process::id());
            let lock = File::options()
                .create_new(true)
                .write(true)
                .open(dir.join(lock_name(&id)))?;
            lock.try_lock()
                .map_err(|e| io::Error::other(e.to_string()))?;
            let (tx, rx) = mpsc::channel::<(u64, Job)>();
            let errors = std::sync::Arc::new(std::sync::Mutex::new(None));
            let worker = {
                let (dir, id, errors) = (dir.clone(), id.clone(), errors.clone());
                std::thread::Builder::new()
                    .name("cadektor-autosave".into())
                    .spawn(move || {
                        for (seq, job) in rx {
                            let r = write_version(&dir, &id, seq, &job);
                            *errors.lock().unwrap() = r.err().map(|e| e.to_string());
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
            })
        }

        /// Queues a write. Returns at once.
        pub fn submit(&mut self, job: Job) {
            self.seq += 1;
            if let Some(tx) = &self.tx {
                let _ = tx.send((self.seq, job));
            }
        }

        /// The error of the last finished write, if it failed.
        pub fn last_error(&self) -> Option<String> {
            self.errors.lock().unwrap().clone()
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

    /// Finds the sessions that ended without a clean exit. A lock file that no process holds
    /// is such a session. Lock files without versions are deleted.
    pub fn find_crashed(dir: &Path) -> Vec<Crashed> {
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
            let Ok(lock) = File::options().write(true).open(dir.join(&name)) else {
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
}

/// The browser keeps one autosaved copy in `localStorage`. A browser tab has no clean exit that
/// the page can rely on, so the copy stays until the user saves (downloads), discards or opens it.
#[cfg(target_arch = "wasm32")]
pub mod web {
    const KEY: &str = "cadektor.autosave";

    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok()?
    }

    /// Stores the document text. The browser limits the size (about 5 MB).
    pub fn put(text: &str) -> Result<(), String> {
        let s = storage().ok_or("browser storage is not available")?;
        s.set_item(KEY, text)
            .map_err(|_| "browser storage is full".to_string())
    }

    pub fn get() -> Option<String> {
        storage()?.get_item(KEY).ok()?
    }

    pub fn clear() {
        if let Some(s) = storage() {
            let _ = s.remove_item(KEY);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_have_the_documented_defaults() {
        let s = AutosaveSettings::default();
        assert_eq!((s.enabled, s.interval_minutes, s.keep), (true, 5, 5));
        assert!(
            !s.save_over_file,
            "never save over the user file by default"
        );
    }

    #[test]
    fn settings_roundtrip_and_clamp() {
        let s = AutosaveSettings {
            enabled: false,
            interval_minutes: 12,
            every_ops: 0,
            keep: 9,
            save_over_file: true,
        };
        assert_eq!(AutosaveSettings::from_json(&s.to_json().to_string()), s);
        let wild = r#"{"autosave":{"interval_minutes":0,"keep":9999,"enabled":"x"}}"#;
        let w = AutosaveSettings::from_json(wild);
        assert_eq!((w.interval_minutes, w.keep, w.enabled), (1, 50, true));
        assert_eq!(
            AutosaveSettings::from_json("{broken"),
            AutosaveSettings::default()
        );
        assert_eq!(
            AutosaveSettings::from_json("{}"),
            AutosaveSettings::default()
        );
    }

    #[test]
    fn scheduler_uses_time_or_operations() {
        let s = AutosaveSettings::default();
        let mut sch = Scheduler::new(0.0, 10);
        assert!(!sch.due(&s, 1000.0, 10), "no change, no save");
        assert!(!sch.due(&s, 60.0, 11), "too early and too few operations");
        assert!(sch.due(&s, 300.0, 11), "after five minutes");
        assert!(sch.due(&s, 1.0, 60), "after 50 operations");
        sch.mark(300.0, 11);
        assert!(!sch.due(&s, 301.0, 11));
        let off = AutosaveSettings {
            enabled: false,
            ..s.clone()
        };
        assert!(!sch.due(&off, 9999.0, 99));
        let no_ops = AutosaveSettings { every_ops: 0, ..s };
        assert!(!sch.due(&no_ops, 301.0, 5000));
    }

    #[test]
    fn age_text_reads_naturally() {
        assert_eq!(age_text(5), "less than a minute ago");
        assert_eq!(age_text(60), "1 minute ago");
        assert_eq!(age_text(300), "5 minutes ago");
        assert_eq!(age_text(7200), "2 hours ago");
        assert_eq!(age_text(86400 * 3), "3 days ago");
    }

    #[cfg(not(target_arch = "wasm32"))]
    mod native_tests {
        use super::super::*;
        use cadektor_core::command::Command;
        use cadektor_core::model::{Attributes, Document, Shape};
        use glam::DVec2;
        use std::path::PathBuf;

        fn temp(name: &str) -> PathBuf {
            let d = std::env::temp_dir()
                .join(format!("cadektor-autosave-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            d
        }

        fn doc_with_circles(n: usize) -> Document {
            let mut e = cadektor_core::command::Editor::new(Document::default());
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
                bytes: serde_json::to_vec(doc).unwrap(),
                original,
                keep,
                over: None,
            }
        }

        #[test]
        fn recovery_dir_per_system() {
            let env = |pairs: &'static [(&'static str, &'static str)]| {
                move |k: &str| {
                    pairs
                        .iter()
                        .find(|(n, _)| *n == k)
                        .map(|(_, v)| std::ffi::OsString::from(v))
                }
            };
            let tail = |p: Option<PathBuf>| p.map(|p| p.to_string_lossy().replace('\\', "/"));
            assert_eq!(
                tail(recovery_dir_for("macos", env(&[("HOME", "/Users/me")]))).as_deref(),
                Some("/Users/me/Library/Application Support/cadektor/recovery")
            );
            assert_eq!(
                tail(recovery_dir_for("linux", env(&[("HOME", "/h")]))).as_deref(),
                Some("/h/.local/state/cadektor/recovery")
            );
            assert_eq!(
                tail(recovery_dir_for("linux", env(&[("XDG_STATE_HOME", "/s")]))).as_deref(),
                Some("/s/cadektor/recovery")
            );
            assert_eq!(
                tail(recovery_dir_for(
                    "windows",
                    env(&[("LOCALAPPDATA", "C:/L")])
                ))
                .as_deref(),
                Some("C:/L/cadektor/recovery")
            );
            assert_eq!(
                tail(recovery_dir_for(
                    "linux",
                    env(&[("CADEKTOR_RECOVERY_DIR", "/x")])
                ))
                .as_deref(),
                Some("/x")
            );
            assert!(recovery_dir_for("linux", env(&[])).is_none());
        }

        #[test]
        fn a_version_is_written_and_read_back() {
            let dir = temp("write");
            let doc = doc_with_circles(3);
            write_version(&dir, "s1", 1, &job(&doc, Some("/a/b.cdk.json".into()), 5)).unwrap();
            let text = std::fs::read_to_string(dir.join("s1.000001.cdk.json")).unwrap();
            assert_eq!(Document::from_json(&text).unwrap(), doc);
            let meta: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(dir.join("s1.meta.json")).unwrap())
                    .unwrap();
            assert_eq!(meta["original"], "/a/b.cdk.json");
            // No temporary files remain.
            let names: Vec<String> = std::fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            assert_eq!(names.len(), 2, "{names:?}");
            let _ = std::fs::remove_dir_all(dir);
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
        fn opt_in_writes_the_user_file_too() {
            let dir = temp("over");
            let target = dir.join("mine.cdk.json");
            let doc = doc_with_circles(2);
            let mut j = job(&doc, Some(target.clone()), 2);
            j.over = Some(target.clone());
            write_version(&dir, "s", 1, &j).unwrap();
            assert_eq!(crate::file_io::read_document(&target).unwrap(), doc);
            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn a_clean_exit_leaves_nothing() {
            let dir = temp("clean");
            let mut r = Recovery::start(dir.clone()).unwrap();
            r.submit(job(&doc_with_circles(1), None, 3));
            r.finish();
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
            assert!(find_crashed(&dir).is_empty());
            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn a_running_session_is_not_reported_but_a_crash_is() {
            let dir = temp("crash");
            let doc = doc_with_circles(4);
            let mut r = Recovery::start(dir.clone()).unwrap();
            let id = r.id().to_string();
            r.submit(job(&doc_with_circles(1), None, 3));
            r.submit(job(&doc, Some("/work/plan.cdk.json".into()), 3));
            // Wait for the writer without ending the session.
            for _ in 0..200 {
                if list_versions(&dir, &id).len() == 2 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(r.last_error().is_none());
            // The lock is still held: another app is running. Nothing to recover.
            assert!(find_crashed(&dir).is_empty());
            // The process dies: the lock goes, the files stay.
            r.simulate_crash();
            let found = find_crashed(&dir);
            assert_eq!(found.len(), 1);
            let c = &found[0];
            assert_eq!(c.id, id);
            assert_eq!(c.versions.len(), 2);
            assert_eq!(
                c.original.as_deref(),
                Some(std::path::Path::new("/work/plan.cdk.json"))
            );
            assert!(c.saved_unix.unwrap() <= now_unix());
            // The newest version comes first.
            let (loaded, from) = c.load().unwrap();
            assert_eq!(loaded, doc);
            assert!(from.to_string_lossy().contains("000002"));
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
            let mut found = find_crashed(&dir);
            let c = found.pop().unwrap();
            assert_eq!(c.load().unwrap().0, doc);
            assert_eq!(c.folder(), dir);
            c.discard();
            assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn a_crash_marker_without_versions_is_cleaned_silently() {
            let dir = temp("marker");
            // The simulated crash marker: a lock file that no process holds.
            std::fs::write(dir.join("dead-1.lock"), "").unwrap();
            assert!(find_crashed(&dir).is_empty());
            assert!(!dir.join("dead-1.lock").exists());
            // A folder that does not exist has nothing to recover.
            assert!(find_crashed(&dir.join("missing")).is_empty());
            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn a_marker_file_with_versions_counts_as_a_crash() {
            let dir = temp("simulated");
            std::fs::write(dir.join("old-9.lock"), "").unwrap();
            let doc = doc_with_circles(1);
            write_version(&dir, "old-9", 1, &job(&doc, None, 5)).unwrap();
            // Another session that is not finished is ignored by name.
            let found = find_crashed(&dir);
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].load().unwrap().0, doc);
            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn preferences_are_saved_next_to_the_keymap() {
            let f = preferences_file();
            if let Some(f) = f {
                assert_eq!(f.file_name().unwrap(), "preferences.json");
            }
        }
    }
}
