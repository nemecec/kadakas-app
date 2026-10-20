//! The handshake between the launcher and the app (PLAT-16). Files only: the app has no network
//! code. The launcher downloads, the app asks and reads.
//!
//! The launcher starts the app with two variables: `KADAKAS_LAUNCHER_ROOT` (the install folder)
//! and `KADAKAS_RUN_ID` (the run, see `runs`). From these the app finds its files in `runs/`:
//!
//! - the app reads `<id>.status.json`: what the update check found (a newer version is available
//!   or ready), and a note when the launcher skipped a version that crashed;
//! - the app writes `<id>.download` to ask for the download (Notify only);
//! - the app writes `<id>.restart.json` with the open documents and then exits. The launcher
//!   starts the newest version again with the documents as arguments;
//! - the app writes `<id>.clean` when it closes in the normal way (the crash marker);
//! - the app keeps `<id>.alive` fresh from a timer thread (not the window loop), so the launcher
//!   knows that a version that does not answer a request at once (a file dialog is open, a big
//!   file loads) is alive, and that a version without the file never reads requests;
//! - the launcher writes `<id>.open-<n>.json` with documents to open (a double click on a `.kad`
//!   file while the app runs, see `inbox`). The app reads and deletes it, and opens the first
//!   document as File > Open does. A file that stays means the app does not read requests.

use crate::runs::{run_file, valid_run_id};
use crate::settings::UpdateMode;
use kadakas_sys::fsio::{self, Access};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The variable with the install folder.
pub const ENV_ROOT: &str = "KADAKAS_LAUNCHER_ROOT";
/// The variable with the run id.
pub const ENV_RUN: &str = "KADAKAS_RUN_ID";
/// Set to `1` by the launcher for every app it starts, with or without a run record. An app on
/// macOS that is started without it (Keep in Dock on the app, `open` on the inner bundle) hands
/// over to the launcher instead of running alone.
pub const ENV_VIA: &str = "KADAKAS_VIA_LAUNCHER";

/// What the update check found.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum UpdateState {
    /// The check is not done yet (the app is already running).
    #[default]
    Checking,
    UpToDate,
    /// A newer version exists and is not downloaded.
    Available {
        version: String,
        size: u64,
    },
    Downloading {
        version: String,
    },
    /// A newer version is downloaded and unpacked. A restart starts it.
    Ready {
        version: String,
    },
    Failed {
        message: String,
    },
}

/// A version that the launcher did not start, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skipped {
    pub version: String,
    pub reason: SkipReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkipReason {
    /// Its last start ended without the clean-exit mark.
    Crashed,
    /// The person rolled back from it.
    RolledBack,
    /// The program could not be started.
    NotStarted,
}

/// What the launcher tells the app about its run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunStatus {
    /// The version that runs.
    pub running: String,
    /// Its label (`stable`, `nightly`, `local`, `bundled`).
    pub label: String,
    pub update: UpdateState,
    /// Newer versions that were skipped to start `running`.
    pub skipped: Vec<Skipped>,
}

/// The documents to open again after a restart.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RestartRequest {
    pub documents: Vec<PathBuf>,
}

/// The most documents one open request may name. More are cut off.
pub const MAX_OPEN_PATHS: usize = 256;

/// The documents to open, as the launcher writes them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenRequest {
    pub paths: Vec<PathBuf>,
}

fn open_prefix(id: &str) -> String {
    format!("{id}.open-")
}

/// The open request files of a run, oldest first (the names carry the time).
fn open_files(root: &Path, id: &str) -> Vec<PathBuf> {
    let prefix = open_prefix(id);
    let mut out: Vec<PathBuf> = std::fs::read_dir(crate::runs::runs_dir(root))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with(&prefix) && n.ends_with(".json")
        })
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

/// Asks the running app of run `id` to open documents (the launcher does this).
pub fn send_open(root: &Path, id: &str, paths: &[PathBuf]) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    if !valid_run_id(id) {
        return Err(std::io::Error::other("bad run id"));
    }
    let req = OpenRequest {
        paths: paths.iter().take(MAX_OPEN_PATHS).cloned().collect(),
    };
    let text = serde_json::to_vec(&req).map_err(std::io::Error::other)?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let name = format!(
        "open-{nanos:040}-{:06}.json",
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    write_atomic(&run_file(root, id, &name), &text)
}

/// True while a request of run `id` has not been read.
pub fn open_pending(root: &Path, id: &str) -> bool {
    !open_files(root, id).is_empty()
}

/// How often the app refreshes its alive stamp.
const HEARTBEAT_EVERY: std::time::Duration = std::time::Duration::from_secs(1);

/// True when the app of run `id` writes an alive stamp (a version that reads open requests does).
pub fn heartbeat_exists(root: &Path, id: &str) -> bool {
    run_file(root, id, "alive").is_file()
}

/// The alive stamp of the app: a small file that a timer thread writes every second. It stops and
/// the file goes when this value is dropped.
pub struct Heartbeat {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    path: PathBuf,
}

impl Heartbeat {
    fn start(root: &Path, id: &str) -> Heartbeat {
        use std::sync::atomic::{AtomicBool, Ordering};
        let path = run_file(root, id, "alive");
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let stamp = |path: &Path| {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let _ = write_atomic(path, nanos.to_string().as_bytes());
        };
        // The first stamp is there when this returns.
        stamp(&path);
        let (flag, p) = (stop.clone(), path.clone());
        let thread = std::thread::Builder::new()
            .name("kadakas-alive".into())
            .spawn(move || {
                let mut waited = std::time::Duration::ZERO;
                while !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    waited += std::time::Duration::from_millis(50);
                    if waited >= HEARTBEAT_EVERY {
                        waited = std::time::Duration::ZERO;
                        stamp(&p);
                    }
                }
            })
            .ok();
        Heartbeat { stop, thread, path }
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Reads and deletes the open requests of a run. A damaged file is deleted and gives nothing.
/// The app calls this through [`AppLink::take_open`]; the launcher calls it to get back requests
/// that the app did not read.
pub fn take_open(root: &Path, id: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for f in open_files(root, id) {
        let bytes = std::fs::read(&f);
        // Delete first, so two readers never open the same request twice.
        if std::fs::remove_file(&f).is_err() {
            continue;
        }
        if let Some(req) = bytes
            .ok()
            .and_then(|b| serde_json::from_slice::<OpenRequest>(&b).ok())
        {
            out.extend(req.paths);
        }
    }
    out.truncate(MAX_OPEN_PATHS);
    out
}

/// Deletes the open requests of a run (its files are cleaned up).
pub(crate) fn remove_open(root: &Path, id: &str) {
    for f in open_files(root, id) {
        let _ = std::fs::remove_file(f);
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    fsio::write_durable(path, bytes, Access::KeepExisting, ".tmp")
}

/// Writes the status of a run (the launcher does this).
pub fn write_status(root: &Path, id: &str, status: &RunStatus) -> std::io::Result<()> {
    let text = serde_json::to_vec_pretty(status).expect("status serializes");
    write_atomic(&run_file(root, id, "status.json"), &text)
}

/// Reads the status of a run. `None` when there is none or it is damaged.
pub fn read_status(root: &Path, id: &str) -> Option<RunStatus> {
    std::fs::read(run_file(root, id, "status.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

/// Reads and deletes the restart request of a run (the launcher does this).
pub fn take_restart(root: &Path, id: &str) -> Option<RestartRequest> {
    let path = run_file(root, id, "restart.json");
    let bytes = std::fs::read(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    // A damaged request still means "restart"; only the documents are lost.
    Some(serde_json::from_slice(&bytes).unwrap_or_default())
}

/// True once when the app asked for the download (the launcher does this).
pub fn take_download_request(root: &Path, id: &str) -> bool {
    std::fs::remove_file(run_file(root, id, "download")).is_ok()
}

/// The app's side of the handshake.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppLink {
    root: PathBuf,
    id: String,
}

impl AppLink {
    /// Reads the two variables. `None` when the launcher did not start the app (or the id is
    /// not a run id).
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>) -> Option<AppLink> {
        let root = get(ENV_ROOT).filter(|r| !r.is_empty())?;
        let id = get(ENV_RUN).filter(|i| valid_run_id(i))?;
        Some(AppLink {
            root: PathBuf::from(root),
            id,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The id of the run (the name of its files in `runs/`).
    pub fn run_id(&self) -> &str {
        &self.id
    }

    /// The status that the launcher wrote, if any.
    pub fn status(&self) -> Option<RunStatus> {
        read_status(&self.root, &self.id)
    }

    /// The last change of the status file, to see whether it is new.
    pub fn status_stamp(&self) -> Option<std::time::SystemTime> {
        std::fs::metadata(run_file(&self.root, &self.id, "status.json"))
            .and_then(|m| m.modified())
            .ok()
    }

    /// Writes the clean-exit mark. Call it when the program closes in the normal way, at the
    /// very end.
    pub fn mark_clean_exit(&self) -> std::io::Result<()> {
        write_atomic(&run_file(&self.root, &self.id, "clean"), b"clean\n")
    }

    /// Takes a lock of the app on its own run (`<id>.app.lock`) and returns the file. Keep it
    /// for as long as the program lives: the launcher then knows that this app runs, also when
    /// the launcher itself has died (it must not mark a crash, nor prune the folder).
    pub fn hold_run_lock(&self) -> Option<std::fs::File> {
        let path = run_file(&self.root, &self.id, "app.lock");
        std::fs::create_dir_all(path.parent()?).ok()?;
        let f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .ok()?;
        f.try_lock().ok()?;
        Some(f)
    }

    /// Starts the alive stamp (see the module text). Keep the value as long as the app lives.
    pub fn start_heartbeat(&self) -> Heartbeat {
        Heartbeat::start(&self.root, &self.id)
    }

    /// Takes the documents that the launcher asked to open (a double click on a `.kad` file while
    /// the app runs). The app opens the first one, as File > Open does. The request files are
    /// **deleted before the app opens anything**: the launcher must see at once that the app has
    /// the request (it starts no second app), and the app then keeps the file in its own
    /// state while it asks the person about unsaved changes. A crash between the delete and the
    /// open loses the request, never the person's drawing.
    pub fn take_open(&self) -> Vec<PathBuf> {
        take_open(&self.root, &self.id)
    }

    /// Asks the launcher to download the new version (Notify only).
    pub fn request_download(&self) -> std::io::Result<()> {
        write_atomic(&run_file(&self.root, &self.id, "download"), b"download\n")
    }

    /// Tells the launcher to start again after this program exits, with these documents. The
    /// program must exit (and write the clean-exit mark) after this call.
    pub fn request_restart(&self, documents: &[PathBuf]) -> std::io::Result<()> {
        let req = RestartRequest {
            documents: documents.to_vec(),
        };
        let text = serde_json::to_vec_pretty(&req).expect("request serializes");
        write_atomic(&run_file(&self.root, &self.id, "restart.json"), &text)
    }

    /// Takes a request back (the person changed their mind before the program exited).
    pub fn cancel_restart(&self) {
        let _ = std::fs::remove_file(run_file(&self.root, &self.id, "restart.json"));
    }
}

/// What the app shows about the update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// "Kadakas X is ready. Restart to update" with Later.
    Ready { version: String },
    /// "Kadakas X is available" with Download (Notify only).
    Available { version: String },
    /// "Downloading Kadakas X..." (Notify only, after Download).
    Downloading { version: String },
}

/// The notice for a status. `later` is the version that the person put off with "Later" in this
/// run: its notice stays away until a newer one comes.
pub fn notice(mode: UpdateMode, state: &UpdateState, later: Option<&str>) -> Option<Notice> {
    match state {
        UpdateState::Ready { version } if later != Some(version) => Some(Notice::Ready {
            version: version.clone(),
        }),
        // Auto-update and Auto-download download by themselves: the person hears when the new
        // version is ready, not before.
        UpdateState::Available { .. } | UpdateState::Downloading { .. }
            if mode != UpdateMode::NotifyOnly =>
        {
            None
        }
        UpdateState::Available { version, .. } if later != Some(version) => {
            Some(Notice::Available {
                version: version.clone(),
            })
        }
        UpdateState::Downloading { version } => Some(Notice::Downloading {
            version: version.clone(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::tmp;

    fn env(root: &Path, id: &str) -> impl Fn(&str) -> Option<String> {
        let root = root.to_string_lossy().into_owned();
        let id = id.to_string();
        move |k| match k {
            ENV_ROOT => Some(root.clone()),
            ENV_RUN => Some(id.clone()),
            _ => None,
        }
    }

    #[test]
    fn the_link_needs_both_variables_and_a_safe_id() {
        let d = tmp("hs-env");
        assert!(AppLink::from_env(&|_| None).is_none());
        assert!(AppLink::from_env(&env(&d, "../x")).is_none());
        assert!(AppLink::from_env(&env(&d, "")).is_none());
        assert!(AppLink::from_env(&|k| (k == ENV_RUN).then(|| "1a".into())).is_none());
        assert!(AppLink::from_env(&|k| (k == ENV_ROOT).then(|| "/x".into())).is_none());
        let l = AppLink::from_env(&env(&d, "1a-2b")).unwrap();
        assert_eq!(l.root(), d);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn status_round_trip() {
        let d = tmp("hs-status");
        let l = AppLink::from_env(&env(&d, "1a")).unwrap();
        assert!(l.status().is_none());
        assert!(l.status_stamp().is_none());
        let s = RunStatus {
            running: "1.0.0".into(),
            label: "stable".into(),
            update: UpdateState::Ready {
                version: "1.1.0".into(),
            },
            skipped: vec![Skipped {
                version: "1.2.0".into(),
                reason: SkipReason::Crashed,
            }],
        };
        write_status(&d, "1a", &s).unwrap();
        assert_eq!(l.status(), Some(s));
        assert!(l.status_stamp().is_some());
        std::fs::write(run_file(&d, "1a", "status.json"), b"{ nope").unwrap();
        assert!(l.status().is_none());
        // Missing fields take defaults.
        std::fs::write(run_file(&d, "1a", "status.json"), br#"{"running":"2.0.0"}"#).unwrap();
        let s = l.status().unwrap();
        assert_eq!(
            (s.running.as_str(), s.update),
            ("2.0.0", UpdateState::Checking)
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn restart_download_and_clean_exit_files() {
        let d = tmp("hs-files");
        let l = AppLink::from_env(&env(&d, "1a")).unwrap();
        assert!(take_restart(&d, "1a").is_none());
        assert!(!take_download_request(&d, "1a"));
        l.request_download().unwrap();
        assert!(take_download_request(&d, "1a"));
        assert!(!take_download_request(&d, "1a"));

        let docs = vec![PathBuf::from("/a/b.kad"), PathBuf::from("/c d/e.kad")];
        l.request_restart(&docs).unwrap();
        assert_eq!(take_restart(&d, "1a").unwrap().documents, docs);
        assert!(take_restart(&d, "1a").is_none(), "taken once");
        l.request_restart(&docs).unwrap();
        l.cancel_restart();
        assert!(take_restart(&d, "1a").is_none());
        // A damaged request is a restart without documents.
        std::fs::write(run_file(&d, "1a", "restart.json"), b"nope").unwrap();
        assert_eq!(take_restart(&d, "1a"), Some(RestartRequest::default()));

        assert!(!run_file(&d, "1a", "clean").exists());
        l.mark_clean_exit().unwrap();
        assert!(run_file(&d, "1a", "clean").is_file());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn notices_follow_the_mode() {
        let v = |s: &str| s.to_string();
        let ready = UpdateState::Ready {
            version: v("1.1.0"),
        };
        let avail = UpdateState::Available {
            version: v("1.1.0"),
            size: 5,
        };
        let loading = UpdateState::Downloading {
            version: v("1.1.0"),
        };
        for mode in UpdateMode::ALL {
            assert_eq!(
                notice(mode, &ready, None),
                Some(Notice::Ready {
                    version: v("1.1.0")
                })
            );
            // "Later" hides the notice of that version.
            assert_eq!(notice(mode, &ready, Some("1.1.0")), None);
            // A newer one comes back.
            assert!(notice(mode, &ready, Some("1.0.5")).is_some());
            for quiet in [&UpdateState::Checking, &UpdateState::UpToDate] {
                assert_eq!(notice(mode, quiet, None), None);
            }
            assert_eq!(
                notice(mode, &UpdateState::Failed { message: v("x") }, None),
                None
            );
        }
        // Auto-update and Auto-download say nothing before the download is done.
        for mode in [UpdateMode::AutoUpdate, UpdateMode::AutoDownload] {
            assert_eq!(notice(mode, &avail, None), None);
            assert_eq!(notice(mode, &loading, None), None);
        }
        // Notify only says that a version exists, and then that it downloads.
        assert_eq!(
            notice(UpdateMode::NotifyOnly, &avail, None),
            Some(Notice::Available {
                version: v("1.1.0")
            })
        );
        assert_eq!(notice(UpdateMode::NotifyOnly, &avail, Some("1.1.0")), None);
        assert_eq!(
            notice(UpdateMode::NotifyOnly, &loading, None),
            Some(Notice::Downloading {
                version: v("1.1.0")
            })
        );
    }
}
