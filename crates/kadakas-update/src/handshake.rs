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
//! - the app writes `<id>.clean` when it closes in the normal way (the crash marker).

use crate::runs::{run_file, valid_run_id};
use crate::settings::UpdateMode;
use kadakas_sys::fsio::{self, Access};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The variable with the install folder.
pub const ENV_ROOT: &str = "KADAKAS_LAUNCHER_ROOT";
/// The variable with the run id.
pub const ENV_RUN: &str = "KADAKAS_RUN_ID";

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
