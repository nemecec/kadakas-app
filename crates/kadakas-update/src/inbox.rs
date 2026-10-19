//! Documents that arrive while the app runs (PLAT-16).
//!
//! The launcher is the entry point, and it stays alive while the app runs. A double click on a
//! `.kad` file (on macOS an Apple event, see the launcher) reaches this launcher process, not a
//! new one. The [`Inbox`] decides what happens to the files:
//!
//! - the app runs: the files go to the app as an open request (`handshake::send_open`). The app
//!   is a single-document app: it opens the first file as File > Open does. No second app starts;
//! - no app runs yet (the launcher has just started, or a restart is under way): the files wait,
//!   and the supervisor starts the app with them;
//! - the app ends and files arrive at the same time: the supervisor starts the app again with them.
//!
//! The inbox is shared between the thread of the system events and the supervisor. It holds no
//! thread of its own.

use crate::handshake;
use crate::supervisor::SpawnSpec;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The app that runs now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Running {
    /// The run id (`runs`).
    pub id: String,
    /// The process id of the app, if the system gave one. The launcher uses it to bring the
    /// window to the front.
    pub pid: Option<u32>,
    /// How the app was started. Used to start another one if this one reads no requests.
    pub spec: SpawnSpec,
}

/// What happened to files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// A request was written for the running app.
    Forwarded(Running),
    /// They wait for the next start of the app.
    Queued,
}

#[derive(Default)]
struct State {
    running: Option<Running>,
    pending: Vec<OsString>,
}

pub struct Inbox {
    root: PathBuf,
    state: Mutex<State>,
}

impl Inbox {
    /// `root` is the install folder (where `runs/` is).
    pub fn new(root: impl Into<PathBuf>) -> Arc<Inbox> {
        Arc::new(Inbox {
            root: root.into(),
            state: Mutex::new(State::default()),
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Files from the system or from the command line. Hands them to the running app, or keeps
    /// them for the next start. Empty lists are ignored.
    pub fn deliver(&self, files: Vec<OsString>) -> Delivery {
        if files.is_empty() {
            return Delivery::Queued;
        }
        let mut st = self.state();
        if let Some(run) = st.running.clone() {
            let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
            if handshake::send_open(&self.root, &run.id, &paths).is_ok() {
                return Delivery::Forwarded(run);
            }
        }
        for f in files {
            if !st.pending.contains(&f) {
                st.pending.push(f);
            }
        }
        Delivery::Queued
    }

    /// The files that wait. The supervisor takes them when it starts the app.
    pub fn take_pending(&self) -> Vec<OsString> {
        std::mem::take(&mut self.state().pending)
    }

    /// The app runs now (the supervisor calls this). Files that came in between the choice of the
    /// start files and this call go to the app as a request.
    pub fn set_running(&self, run: Running) {
        let mut st = self.state();
        if !st.pending.is_empty() {
            let paths: Vec<PathBuf> = st.pending.iter().map(PathBuf::from).collect();
            if handshake::send_open(&self.root, &run.id, &paths).is_ok() {
                st.pending.clear();
            }
        }
        st.running = Some(run);
    }

    /// The app has ended (the supervisor calls this). Returns the files that came in the same
    /// moment and were not read: the requests that the app left, and the waiting files. The
    /// supervisor starts the app again with them.
    pub fn end_run(&self) -> Vec<OsString> {
        let mut st = self.state();
        let Some(run) = st.running.take() else {
            return std::mem::take(&mut st.pending);
        };
        let mut left: Vec<OsString> = handshake::take_open(&self.root, &run.id)
            .into_iter()
            .map(PathBuf::into_os_string)
            .collect();
        for f in std::mem::take(&mut st.pending) {
            if !left.contains(&f) {
                left.push(f);
            }
        }
        left
    }

    /// The app that runs now, if any.
    pub fn running(&self) -> Option<Running> {
        self.state().running.clone()
    }

    /// True while a request of the run `id` is not read yet.
    pub fn unread(&self, id: &str) -> bool {
        handshake::open_pending(&self.root, id)
    }

    /// Takes back the requests that run `id` did not read.
    pub fn reclaim(&self, id: &str) -> Vec<OsString> {
        handshake::take_open(&self.root, id)
            .into_iter()
            .map(PathBuf::into_os_string)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flavors::Flavor;
    use crate::store::tests::tmp;

    fn run(id: &str) -> Running {
        Running {
            id: id.into(),
            pid: Some(7),
            spec: SpawnSpec {
                dir: PathBuf::from("/v"),
                flavor: Flavor {
                    id: "kadakas-app".into(),
                    name: "Kadakas".into(),
                    exe: "kadakas-app".into(),
                },
                profile: String::new(),
                files: Vec::new(),
                env: Vec::new(),
            },
        }
    }

    fn files(names: &[&str]) -> Vec<OsString> {
        names.iter().map(OsString::from).collect()
    }

    #[test]
    fn files_wait_until_the_app_runs() {
        let d = tmp("inbox-wait");
        let inbox = Inbox::new(&d);
        assert_eq!(
            inbox.deliver(files(&["/a.kad", "/b.kad"])),
            Delivery::Queued
        );
        // The same file twice is one file.
        assert_eq!(inbox.deliver(files(&["/b.kad"])), Delivery::Queued);
        assert_eq!(inbox.deliver(Vec::new()), Delivery::Queued);
        assert_eq!(inbox.take_pending(), files(&["/a.kad", "/b.kad"]));
        assert!(inbox.take_pending().is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn files_go_to_the_running_app_and_the_app_reads_them() {
        let d = tmp("inbox-run");
        let inbox = Inbox::new(&d);
        inbox.set_running(run("1a-2b"));
        let got = inbox.deliver(files(&["/a.kad"]));
        assert_eq!(got, Delivery::Forwarded(run("1a-2b")));
        assert_eq!(
            inbox.deliver(files(&["-odd.kad"])),
            Delivery::Forwarded(run("1a-2b"))
        );
        assert!(inbox.unread("1a-2b"));
        assert!(inbox.take_pending().is_empty());
        // The app side: both requests, in the order of sending, then nothing.
        let link = crate::handshake::AppLink::from_env(&|k| match k {
            crate::handshake::ENV_ROOT => Some(d.to_string_lossy().into_owned()),
            crate::handshake::ENV_RUN => Some("1a-2b".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(
            link.take_open(),
            [PathBuf::from("/a.kad"), PathBuf::from("-odd.kad")]
        );
        assert!(link.take_open().is_empty());
        assert!(!inbox.unread("1a-2b"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn files_that_came_in_just_before_the_start_go_to_the_new_app() {
        let d = tmp("inbox-between");
        let inbox = Inbox::new(&d);
        inbox.deliver(files(&["/a.kad"]));
        inbox.set_running(run("1a"));
        assert!(inbox.take_pending().is_empty());
        assert!(inbox.unread("1a"));
        assert_eq!(inbox.reclaim("1a"), files(&["/a.kad"]));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn requests_that_stay_are_given_back_at_the_end_or_on_demand() {
        let d = tmp("inbox-left");
        let inbox = Inbox::new(&d);
        inbox.set_running(run("1a"));
        inbox.deliver(files(&["/a.kad"]));
        // The app does not read: the launcher takes the request back.
        assert_eq!(inbox.reclaim("1a"), files(&["/a.kad"]));
        assert!(!inbox.unread("1a"));
        // The app ends before it read the request: the files come back for a new start.
        inbox.deliver(files(&["/b.kad"]));
        assert_eq!(inbox.end_run(), files(&["/b.kad"]));
        assert!(inbox.running().is_none());
        // After the end, files wait.
        assert_eq!(inbox.deliver(files(&["/c.kad"])), Delivery::Queued);
        assert_eq!(inbox.end_run(), files(&["/c.kad"]));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_bad_run_id_cannot_write_outside_the_runs_folder() {
        let d = tmp("inbox-bad");
        let inbox = Inbox::new(&d);
        inbox.set_running(run("../../x"));
        // The request cannot be written, so the files wait instead of being lost.
        assert_eq!(inbox.deliver(files(&["/a.kad"])), Delivery::Queued);
        assert_eq!(inbox.take_pending(), files(&["/a.kad"]));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn damaged_and_oversized_requests_are_safe() {
        let d = tmp("inbox-damaged");
        let runs = crate::runs::runs_dir(&d);
        std::fs::create_dir_all(&runs).unwrap();
        std::fs::write(runs.join("1a.open-0001.json"), b"not json").unwrap();
        assert!(crate::handshake::take_open(&d, "1a").is_empty());
        assert!(!crate::handshake::open_pending(&d, "1a"));
        let many: Vec<PathBuf> = (0..1000)
            .map(|i| PathBuf::from(format!("/f{i}.kad")))
            .collect();
        crate::handshake::send_open(&d, "1a", &many).unwrap();
        assert_eq!(
            crate::handshake::take_open(&d, "1a").len(),
            crate::handshake::MAX_OPEN_PATHS
        );
        // A request of another run is not touched.
        crate::handshake::send_open(&d, "1b", &[PathBuf::from("/x.kad")]).unwrap();
        assert!(crate::handshake::take_open(&d, "1a").is_empty());
        assert_eq!(crate::handshake::take_open(&d, "1b").len(), 1);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
