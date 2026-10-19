//! The state of the launcher window and its actions, without any drawing code.

use kadakas_l10n::tr;
use kadakas_update::flavors::{self, Flavor};
use kadakas_update::handshake::SkipReason;
use kadakas_update::settings::valid_name;
use kadakas_update::tasks::{Outcome, Task};
use kadakas_update::{Channel, Installed, Result, Settings, Slot, UpdateStatus, Updater};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

/// Starts Kadakas in a process of its own (`kadakas-launcher start`), so the window can close
/// while the app runs, and the crash mark still works.
pub type Starter = Arc<dyn Fn() -> std::io::Result<()> + Send + Sync>;

pub struct Model {
    updater: Arc<Updater>,
    starter: Starter,
    pub settings: Settings,
    pub installed: Vec<Installed>,
    pub flavors: Vec<Flavor>,
    /// Versions that Auto-update skips, and why.
    pub skipped: BTreeMap<Slot, SkipReason>,
    /// The row that the buttons act on.
    pub selected: Option<Slot>,
    /// The result of the last check, if there was one.
    pub update: Option<UpdateStatus>,
    /// One line for the status bar.
    pub status: String,
    running: Option<Receiver<Result<Outcome>>>,
}

/// The text of a slot: the version, or the version from the installer.
pub fn slot_text(slot: &Slot) -> String {
    match slot {
        Slot::Bundled => tr!("launcher-slot-bundled"),
        Slot::Version(v) => v.to_string(),
    }
}

/// A short line for the status bar, for the result of a task.
pub fn describe(result: &Result<Outcome>) -> String {
    match result {
        Ok(Outcome::Checked(UpdateStatus::UpToDate { .. })) => tr!("launcher-done-up-to-date"),
        Ok(Outcome::Checked(UpdateStatus::Available(o))) => tr!(
            "launcher-done-available",
            version = o.version.to_string(),
            mb = o.size.div_ceil(1_000_000)
        ),
        Ok(Outcome::Checked(UpdateStatus::Older { current, offered })) => tr!(
            "launcher-done-older",
            current = current.to_string(),
            offered = offered.to_string()
        ),
        Ok(Outcome::Updated(v)) => tr!("launcher-done-updated", version = v.to_string()),
        Ok(Outcome::RolledBack(s)) => tr!("launcher-done-rolled-back", version = slot_text(s)),
        Err(e) => tr!("launcher-done-failed", error = e.to_string()),
    }
}

impl Model {
    pub fn new(updater: Arc<Updater>, starter: Starter) -> Model {
        // A start after a crash: remove half-made folders and old downloads, and settle the
        // runs whose launcher is gone.
        updater.store().recover();
        updater.store().settle_runs();
        let settings = Settings::load(updater.store().root());
        let mut m = Model {
            updater,
            starter,
            settings,
            installed: Vec::new(),
            flavors: Vec::new(),
            skipped: BTreeMap::new(),
            selected: None,
            update: None,
            status: String::new(),
            running: None,
        };
        m.refresh();
        m
    }

    /// Reads the install folder again.
    pub fn refresh(&mut self) {
        let store = self.updater.store();
        self.installed = store.list();
        self.skipped = store
            .rolled_back_slots()
            .into_iter()
            .map(|s| (s, SkipReason::RolledBack))
            .chain(
                store
                    .crashed_slots()
                    .into_iter()
                    .map(|s| (s, SkipReason::Crashed)),
            )
            .collect();
        if self
            .selected
            .as_ref()
            .is_some_and(|s| !self.installed.iter().any(|i| &i.slot == s))
        {
            self.selected = None;
        }
        self.flavors = store
            .active()
            .map(|a| flavors::discover(&a.dir))
            .unwrap_or_default();
        if flavors::pick(&self.flavors, &self.settings.flavor).is_none() && !self.flavors.is_empty()
        {
            self.settings.flavor = self.flavors[0].id.clone();
        }
    }

    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    pub fn can_launch(&self) -> bool {
        !self.busy() && flavors::pick(&self.flavors, &self.settings.flavor).is_some()
    }

    pub fn can_update(&self) -> bool {
        !self.busy() && matches!(self.update, Some(UpdateStatus::Available(_)))
    }

    pub fn can_roll_back(&self) -> bool {
        !self.busy() && self.updater.store().rollback_target().is_some()
    }

    fn selected_item(&self) -> Option<&Installed> {
        let s = self.selected.as_ref()?;
        self.installed.iter().find(|i| &i.slot == s)
    }

    /// The selected version can become the one that starts.
    pub fn can_use(&self) -> bool {
        !self.busy() && self.selected_item().is_some_and(|i| !i.active)
    }

    /// The selected version can be kept or removed (not the bundled one).
    pub fn can_keep(&self) -> bool {
        !self.busy()
            && self
                .selected_item()
                .is_some_and(|i| i.slot != Slot::Bundled)
    }

    pub fn can_remove(&self) -> bool {
        self.can_keep() && self.selected_item().is_some_and(|i| !i.active)
    }

    /// The selected version is skipped by Auto-update, and the person may try it again.
    pub fn can_retry(&self) -> bool {
        !self.busy()
            && self
                .selected
                .as_ref()
                .is_some_and(|s| self.skipped.contains_key(s))
    }

    /// True when the selected version is kept.
    pub fn selected_kept(&self) -> bool {
        self.selected_item().is_some_and(|i| i.meta.kept)
    }

    pub fn select(&mut self, slot: Slot) {
        self.selected = Some(slot);
    }

    fn save(&mut self) {
        if let Err(e) = self.settings.save(self.updater.store().root()) {
            self.status = tr!("launcher-status-save-failed", error = e.to_string());
        }
    }

    pub fn set_channel(&mut self, channel: Channel) {
        if self.settings.channel != channel {
            self.settings.channel = channel;
            // What we know about the old channel does not hold for the new one.
            self.update = None;
            self.save();
        }
    }

    pub fn set_flavor(&mut self, id: &str) {
        if self.settings.flavor != id && self.flavors.iter().any(|f| f.id == id) {
            self.settings.flavor = id.to_string();
            self.save();
        }
    }

    /// Sets the profile. An empty name means none. Returns false for a bad name.
    pub fn set_profile(&mut self, name: &str) -> bool {
        let name = name.trim();
        if !name.is_empty() && !valid_name(name) {
            return false;
        }
        if self.settings.profile != name {
            self.settings.profile = name.to_string();
            self.save();
        }
        true
    }

    /// Makes the selected version the one that starts (in the modes where the person chooses),
    /// and takes it off the list of skipped versions.
    pub fn use_selected(&mut self) {
        let Some(slot) = self.selected.clone().filter(|_| self.can_use()) else {
            return;
        };
        self.status = match self.updater.store().activate(&slot) {
            Ok(()) => tr!("launcher-status-used", version = slot_text(&slot)),
            Err(e) => tr!("launcher-done-failed", error = e.to_string()),
        };
        self.refresh();
    }

    pub fn toggle_keep(&mut self) {
        let Some(item) = self.selected_item().filter(|_| self.can_keep()) else {
            return;
        };
        let (version, kept) = (item.version.clone(), item.meta.kept);
        self.status = match self.updater.store().set_kept(&version, !kept) {
            Ok(()) if kept => tr!("launcher-status-unkept", version = version.to_string()),
            Ok(()) => tr!("launcher-status-kept", version = version.to_string()),
            Err(e) => tr!("launcher-done-failed", error = e.to_string()),
        };
        self.refresh();
    }

    pub fn remove_selected(&mut self) {
        let Some(item) = self.selected_item().filter(|_| self.can_remove()) else {
            return;
        };
        let version = item.version.clone();
        self.status = match self.updater.store().remove(&version) {
            Ok(()) => tr!("launcher-status-removed", version = version.to_string()),
            Err(e) => tr!("launcher-done-failed", error = e.to_string()),
        };
        self.refresh();
    }

    pub fn retry_selected(&mut self) {
        let Some(slot) = self.selected.clone().filter(|_| self.can_retry()) else {
            return;
        };
        self.status = match self.updater.store().clear_crash(&slot) {
            Ok(()) => tr!("launcher-status-retry", version = slot_text(&slot)),
            Err(e) => tr!("launcher-done-failed", error = e.to_string()),
        };
        self.refresh();
    }

    /// Starts a check, an update or a rollback in a background thread.
    /// `wake` is called when the thread is done, so the window can redraw.
    pub fn start(&mut self, task: Task, wake: impl Fn() + Send + 'static) {
        if self.busy() {
            return;
        }
        let (tx, rx) = channel();
        let updater = self.updater.clone();
        let channel_choice = self.settings.channel;
        self.status = match task {
            Task::Check => tr!("launcher-status-checking"),
            Task::Update => tr!("launcher-status-downloading"),
            Task::Rollback => tr!("launcher-status-rolling-back"),
        };
        std::thread::spawn(move || {
            let _ = tx.send(kadakas_update::tasks::run(&updater, channel_choice, task));
            wake();
        });
        self.running = Some(rx);
    }

    /// Takes the result of the background task, if it is done. Returns true when it was.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.running else {
            return false;
        };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.running = None;
                self.status = tr!("launcher-status-task-stopped");
                return true;
            }
        };
        self.running = None;
        self.status = describe(&result);
        match result {
            Ok(Outcome::Checked(s)) => self.update = Some(s),
            Ok(Outcome::Updated(_) | Outcome::RolledBack(_)) => {
                self.update = None;
                self.refresh();
            }
            Err(_) => {}
        }
        true
    }

    /// Starts Kadakas (in a process of its own: the launcher chooses the version by the rules
    /// of the mode and watches the run).
    pub fn launch(&mut self) {
        if self.updater.store().active().is_none() {
            self.status = tr!("launcher-status-no-version");
            return;
        }
        if flavors::pick(&self.flavors, &self.settings.flavor).is_none() {
            self.status = tr!("launcher-status-no-flavor");
            return;
        }
        self.status = match (self.starter)() {
            Ok(()) => tr!("launcher-status-started"),
            Err(e) => tr!("launcher-status-start-failed", error = e.to_string()),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_update::store::Bundled;
    use kadakas_update::{MockHttp, Store, TrustedKeys, Version};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const KEY: &str = "f1517ae94a0eefd20144191f1583a8d648e3f91c059b0e4bba59c975d9871b7c";

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("kadakas-launcher-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn counting_starter(count: &Arc<AtomicUsize>, fail: bool) -> Starter {
        let count = count.clone();
        Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            if fail {
                Err(std::io::Error::other("no process"))
            } else {
                Ok(())
            }
        })
    }

    fn model_with(root: &Path, with_app: bool, starter: Starter) -> Model {
        let dir = root.join("bundled");
        std::fs::create_dir_all(&dir).unwrap();
        if with_app {
            let exe = dir.join(format!("kadakas-app{}", std::env::consts::EXE_SUFFIX));
            std::fs::write(exe, b"x").unwrap();
        }
        let bundled = Bundled {
            version: Version::parse("1.0.0").unwrap(),
            dir,
        };
        let store = Store::new(root.join("data"), Some(bundled));
        let updater = Updater::new(
            store,
            Arc::new(MockHttp::new()),
            TrustedKeys::new(&[KEY]).unwrap(),
            "https://updates.example.org/m",
            "linux-x86_64",
        );
        Model::new(Arc::new(updater), starter)
    }

    fn model(root: &Path, with_app: bool) -> Model {
        model_with(root, with_app, counting_starter(&Arc::default(), false))
    }

    fn wait(m: &mut Model) {
        for _ in 0..3000 {
            if m.poll() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the task did not finish");
    }

    /// Puts versions into the store of the model (not the bundled one).
    fn add_versions(m: &mut Model, root: &Path, versions: &[&str]) {
        for ver in versions {
            let dir = root.join(format!("build-{ver}"));
            std::fs::create_dir_all(&dir).unwrap();
            let exe = format!("kadakas-app{}", std::env::consts::EXE_SUFFIX);
            std::fs::write(dir.join(&exe), b"x").unwrap();
            let build = kadakas_update::local::LocalBuild {
                dir,
                files: vec![PathBuf::from(exe)],
            };
            let v = Version::parse(&format!("1.0.0-local.{}", ver.replace('.', "x"))).unwrap();
            m.updater
                .store()
                .add_local(&v, &build, Some("abc"))
                .unwrap();
        }
        m.refresh();
    }

    #[test]
    fn shows_the_bundled_version_and_flavor() {
        let root = tmp("show");
        let m = model(&root, true);
        assert_eq!(m.installed.len(), 1);
        assert!(m.installed[0].active);
        assert_eq!(m.installed[0].meta.label, "bundled");
        assert_eq!(m.flavors.len(), 1);
        assert!(m.can_launch());
        assert!(!m.can_update());
        assert!(!m.can_roll_back());
        assert!(!m.can_use() && !m.can_keep() && !m.can_remove() && !m.can_retry());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn settings_are_saved_and_checked() {
        let root = tmp("settings");
        let mut m = model(&root, true);
        m.set_channel(Channel::Beta);
        assert!(m.set_profile(" acad "));
        assert!(!m.set_profile("bad name!"));
        assert_eq!(m.settings.profile, "acad");
        m.set_flavor("not-installed");
        assert_eq!(m.settings.flavor, "kadakas-app");
        assert!(m.set_profile(""));
        assert!(m.set_profile(""));
        m.set_channel(Channel::Beta);
        let loaded = Settings::load(&root.join("data"));
        assert_eq!(loaded.channel, Channel::Beta);
        assert_eq!(loaded.profile, "");
        // A new window sees the same choices.
        let again = model(&root, true);
        assert_eq!(again.settings.channel, Channel::Beta);
        // The channel local is a choice too.
        m.set_channel(Channel::Local);
        assert_eq!(Settings::load(&root.join("data")).channel, Channel::Local);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_failed_check_reports_and_a_second_start_is_ignored() {
        let root = tmp("check");
        let mut m = model(&root, true);
        m.start(Task::Check, || {});
        assert!(m.busy());
        assert!(!m.can_launch());
        m.start(Task::Update, || {});
        wait(&mut m);
        assert!(m.status.starts_with("Failed"), "{}", m.status);
        assert!(!m.busy());
        assert!(!m.poll());
        assert!(m.update.is_none());
        m.start(Task::Rollback, || {});
        wait(&mut m);
        assert!(m.status.starts_with("Failed"), "{}", m.status);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn launch_hands_over_to_a_process_of_its_own() {
        let root = tmp("launch");
        let count = Arc::new(AtomicUsize::new(0));
        let mut m = model_with(&root, true, counting_starter(&count, false));
        m.launch();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(m.status, tr!("launcher-status-started"));
        std::fs::remove_dir_all(&root).unwrap();

        // The process cannot be started: a message, no panic.
        let root = tmp("launch-fail");
        let mut m = model_with(&root, true, counting_starter(&count, true));
        m.launch();
        assert!(m.status.contains("no process"), "{}", m.status);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn launch_needs_an_installed_program() {
        let root = tmp("nolaunch");
        let count = Arc::new(AtomicUsize::new(0));
        let mut m = model_with(&root, false, counting_starter(&count, false));
        assert!(!m.can_launch());
        m.launch();
        assert!(m.status.contains("not installed"), "{}", m.status);
        assert_eq!(count.load(Ordering::SeqCst), 0);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn launch_without_any_version() {
        let root = tmp("noversion");
        let store = Store::new(root.join("data"), None);
        let updater = Updater::new(
            store,
            Arc::new(MockHttp::new()),
            TrustedKeys::new(&[KEY]).unwrap(),
            "https://updates.example.org/m",
            "linux-x86_64",
        );
        let mut m = Model::new(Arc::new(updater), counting_starter(&Arc::default(), false));
        m.launch();
        assert!(m.status.contains("No Kadakas version"), "{}", m.status);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn local_versions_are_listed_with_their_label_and_can_be_used_kept_and_removed() {
        let root = tmp("local-rows");
        let mut m = model(&root, true);
        add_versions(&mut m, &root, &["a", "b"]);
        assert_eq!(m.installed.len(), 3);
        let local: Vec<_> = m
            .installed
            .iter()
            .filter(|i| i.meta.label == "local")
            .collect();
        assert_eq!(local.len(), 2);
        assert_eq!(local[0].meta.commit.as_deref(), Some("abc"));
        let slot = local[0].slot.clone();
        m.select(slot.clone());
        assert!(m.can_use() && m.can_keep() && m.can_remove());
        // Use: it becomes the version that starts.
        m.use_selected();
        assert_eq!(m.updater.store().active().unwrap().slot, slot);
        assert!(!m.can_use() && !m.can_remove(), "the active one stays");
        // Keep and unkeep.
        m.toggle_keep();
        assert!(m.selected_kept());
        m.toggle_keep();
        assert!(!m.selected_kept());
        // Remove another one.
        let other = m
            .installed
            .iter()
            .find(|i| i.meta.label == "local" && i.slot != slot)
            .unwrap()
            .slot
            .clone();
        m.select(other.clone());
        m.remove_selected();
        assert!(m.installed.iter().all(|i| i.slot != other));
        assert!(m.selected.is_none(), "the selection followed the row");
        // Nothing selected: the actions do nothing.
        m.use_selected();
        m.toggle_keep();
        m.remove_selected();
        m.retry_selected();
        // The bundled app cannot be kept or removed.
        m.select(Slot::Bundled);
        assert!(m.can_use() && !m.can_keep() && !m.can_remove());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_crashed_version_is_marked_and_can_be_tried_again() {
        let root = tmp("crashed-row");
        let mut m = model(&root, true);
        add_versions(&mut m, &root, &["a"]);
        let slot = m
            .installed
            .iter()
            .find(|i| i.meta.label == "local")
            .unwrap()
            .slot
            .clone();
        m.updater.store().note_run_end(&slot, false).unwrap();
        m.refresh();
        assert_eq!(m.skipped.get(&slot), Some(&SkipReason::Crashed));
        m.select(slot.clone());
        assert!(m.can_retry());
        m.retry_selected();
        assert!(m.skipped.is_empty());
        assert!(!m.can_retry());
        // A rollback is a skip too.
        m.updater.store().activate(&slot).unwrap();
        m.updater.store().rollback().unwrap();
        m.refresh();
        assert_eq!(m.skipped.get(&slot), Some(&SkipReason::RolledBack));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn outcomes_are_described_in_words() {
        use kadakas_update::Offer;
        let v = |s: &str| Version::parse(s).unwrap();
        let checked = |s| Ok(Outcome::Checked(s));
        assert!(
            describe(&checked(UpdateStatus::UpToDate { current: None })).contains("up to date")
        );
        let offer = Offer {
            version: v("1.1.0"),
            channel: Channel::Stable,
            size: 2_500_000,
        };
        let text = describe(&checked(UpdateStatus::Available(offer)));
        assert!(text.contains("1.1.0") && text.contains("3 MB"), "{text}");
        let older = describe(&checked(UpdateStatus::Older {
            current: v("2.0.0"),
            offered: v("1.0.0"),
        }));
        assert!(older.contains("older than 2.0.0"), "{older}");
        assert!(describe(&Ok(Outcome::Updated(v("1.1.0")))).contains("1.1.0"));
        assert!(describe(&Ok(Outcome::RolledBack(Slot::Bundled))).contains("installer"));
        assert!(describe(&Ok(Outcome::RolledBack(Slot::Version(v("1.0.5"))))).contains("1.0.5"));
        assert!(describe(&Err(kadakas_update::UpdateError::Busy)).starts_with("Failed"));
    }
}
