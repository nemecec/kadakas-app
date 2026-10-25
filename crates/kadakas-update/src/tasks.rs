//! The actions of the launcher, as values. The UI runs one in a background thread and shows the
//! text of the result. The UI code then has no update logic of its own.

use crate::error::Result;
use crate::manifest::Channel;
use crate::store::Slot;
use crate::updater::{UpdateStatus, Updater};
use crate::version::Version;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    /// Ask the server for a newer version.
    Check,
    /// Download, verify and install the newer version.
    Update,
    /// Go back to the previous version.
    Rollback,
}

#[derive(Debug)]
pub enum Outcome {
    Checked(UpdateStatus),
    Updated(Version),
    RolledBack(Slot),
}

/// Runs a task. This blocks, so call it from a thread.
pub fn run(updater: &Updater, channel: Channel, task: Task) -> Result<Outcome> {
    match task {
        Task::Check => updater.check(channel).map(Outcome::Checked),
        Task::Update => updater.update(channel).map(Outcome::Updated),
        Task::Rollback => updater.rollback().map(Outcome::RolledBack),
    }
}

/// A short line for the status bar.
pub fn describe(result: &Result<Outcome>) -> String {
    match result {
        Ok(Outcome::Checked(UpdateStatus::UpToDate { .. })) => "Kadakas is up to date.".into(),
        Ok(Outcome::Checked(UpdateStatus::Available(o))) => format!(
            "Version {} is available ({:.1} MB).",
            o.version,
            o.size as f64 / 1_000_000.0
        ),
        Ok(Outcome::Checked(UpdateStatus::Older { current, offered })) => {
            format!("The server offers {offered}, which is older than {current}. Ignored.")
        }
        Ok(Outcome::Updated(v)) => format!("Updated to version {v}."),
        Ok(Outcome::RolledBack(s)) => format!("Rolled back to {s}."),
        Err(e) => format!("Failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::MockHttp;
    use crate::store::tests::{tmp, v};
    use crate::updater::tests::{key, publish, updater};
    use std::sync::Arc;

    #[test]
    fn tasks_run_and_describe() {
        let root = tmp("tasks");
        let http = Arc::new(MockHttp::new());
        publish(&http, "1.1.0", Channel::Stable, &key(1));
        let u = updater(&root, http, &[1], Some("1.0.0"));

        let r = run(&u, Channel::Stable, Task::Check);
        assert!(
            describe(&r).starts_with("Version 1.1.0 is available"),
            "{}",
            describe(&r)
        );
        let r = run(&u, Channel::Stable, Task::Update);
        assert_eq!(describe(&r), "Updated to version 1.1.0.");
        let r = run(&u, Channel::Stable, Task::Check);
        assert_eq!(describe(&r), "Kadakas is up to date.");
        let r = run(&u, Channel::Stable, Task::Rollback);
        assert_eq!(describe(&r), "Rolled back to bundled.");
        let r = run(&u, Channel::Beta, Task::Check);
        assert!(describe(&r).starts_with("Failed: "), "{}", describe(&r));
        crate::store::tests::rm_dir(&root);
    }

    #[test]
    fn older_offer_text() {
        let r = Ok(Outcome::Checked(UpdateStatus::Older {
            current: v("2.0.0"),
            offered: v("1.0.0"),
        }));
        assert!(describe(&r).contains("older than 2.0.0"));
    }
}
