//! The state of the launcher window and its actions, without any drawing code.

use kadakas_update::flavors::{self, Flavor};
use kadakas_update::launch;
use kadakas_update::settings::valid_name;
use kadakas_update::tasks::{self, Outcome, Task};
use kadakas_update::{Channel, Installed, Result, Settings, UpdateStatus, Updater};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

pub struct Model {
    updater: Arc<Updater>,
    pub settings: Settings,
    pub installed: Vec<Installed>,
    pub flavors: Vec<Flavor>,
    /// The result of the last check, if there was one.
    pub update: Option<UpdateStatus>,
    /// One line for the status bar.
    pub status: String,
    running: Option<Receiver<Result<Outcome>>>,
}

impl Model {
    pub fn new(updater: Arc<Updater>) -> Model {
        // A start after a crash: remove half-made folders and old downloads.
        updater.store().recover();
        let settings = Settings::load(updater.store().root());
        let mut m = Model {
            updater,
            settings,
            installed: Vec::new(),
            flavors: Vec::new(),
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

    fn save(&mut self) {
        if let Err(e) = self.settings.save(self.updater.store().root()) {
            self.status = format!("Could not save the settings: {e}");
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
            Task::Check => "Checking for updates...",
            Task::Update => "Downloading and installing...",
            Task::Rollback => "Rolling back...",
        }
        .into();
        std::thread::spawn(move || {
            let _ = tx.send(tasks::run(&updater, channel_choice, task));
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
                self.status = "The background task stopped without a result.".into();
                return true;
            }
        };
        self.running = None;
        self.status = tasks::describe(&result);
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

    /// Starts the chosen flavor from the active version.
    pub fn launch(&mut self) {
        let Some(active) = self.updater.store().active() else {
            self.status = "No Kadakas version is installed.".into();
            return;
        };
        let Some(flavor) = flavors::pick(&self.flavors, &self.settings.flavor) else {
            self.status = "The chosen UI flavor is not installed.".into();
            return;
        };
        self.status = match launch::launch(&active.dir, flavor, &self.settings.profile) {
            Ok(_) => format!("Started {} {}.", flavor.name, active.version),
            Err(e) => format!("Could not start {}: {e}", flavor.name),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kadakas_update::store::Bundled;
    use kadakas_update::{MockHttp, Store, TrustedKeys, Version};
    use std::path::{Path, PathBuf};

    const KEY: &str = "f1517ae94a0eefd20144191f1583a8d648e3f91c059b0e4bba59c975d9871b7c";

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("kadakas-launcher-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn model(root: &Path, with_app: bool) -> Model {
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
        Model::new(Arc::new(updater))
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

    #[test]
    fn shows_the_bundled_version_and_flavor() {
        let root = tmp("show");
        let m = model(&root, true);
        assert_eq!(m.installed.len(), 1);
        assert!(m.installed[0].active);
        assert_eq!(m.flavors.len(), 1);
        assert!(m.can_launch());
        assert!(!m.can_update());
        assert!(!m.can_roll_back());
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
    fn launch_needs_an_installed_program() {
        let root = tmp("nolaunch");
        let mut m = model(&root, false);
        assert!(!m.can_launch());
        m.launch();
        assert!(m.status.contains("not installed"), "{}", m.status);
        std::fs::remove_dir_all(&root).unwrap();

        // The program is a plain file, not a real program: the start fails with a message.
        let root = tmp("badlaunch");
        let mut m = model(&root, true);
        m.launch();
        #[cfg(unix)]
        assert!(m.status.starts_with("Could not start"), "{}", m.status);
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
        let mut m = Model::new(Arc::new(updater));
        m.launch();
        assert!(m.status.contains("No Kadakas version"), "{}", m.status);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
