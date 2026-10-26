//! The last try to save settings when a program ends, and a log line when it fails.
//!
//! A save that failed is tried again while the program runs (see `kadakas_ui_core::retry`). At the
//! end of the program there is no later try. [`final_attempt`] waits a moment, tries once more,
//! and if that fails too it writes the reason to stderr and to `save-failed.log` in the state
//! folder, so the person (or a bug report) can find out why settings did not stay.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

/// The name of the log file in the log folder.
pub const LOG_FILE: &str = "save-failed.log";

/// Appends one line to the log: the time in seconds, what was saved and the reason.
pub fn log_failure(log_dir: &Path, what: &str, reason: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(log_dir)?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join(LOG_FILE))?;
    // One line: new lines in the reason would break the format.
    writeln!(file, "{secs} {what}: {}", reason.replace(['\n', '\r'], " "))
}

/// Called when a save failed and the program ends: waits `pause`, tries once more with
/// `try_save`, and on a second failure logs it (stderr, and the log file in `log_dir` if there is
/// one). Returns the result of the last try.
pub fn final_attempt(
    mut try_save: impl FnMut() -> Result<(), String>,
    pause: Duration,
    what: &str,
    log_dir: Option<&Path>,
) -> Result<(), String> {
    std::thread::sleep(pause);
    let result = try_save();
    if let Err(reason) = &result {
        eprintln!("kadakas: {what} was not saved: {reason}");
        if let Some(dir) = log_dir {
            let _ = log_failure(dir, what, reason);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("kadakas-savelog-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_second_try_that_works_logs_nothing() {
        let d = tmp("ok");
        let mut calls = 0;
        let r = final_attempt(
            || {
                calls += 1;
                Ok(())
            },
            Duration::from_millis(1),
            "the preferences",
            Some(&d),
        );
        assert_eq!((r, calls), (Ok(()), 1));
        assert!(!d.join(LOG_FILE).exists());
    }

    #[test]
    fn a_second_failure_is_logged_on_one_line_each_time() {
        let d = tmp("fail");
        let start = std::time::Instant::now();
        let r = final_attempt(
            || Err("cannot write\nprefs.json".to_string()),
            Duration::from_millis(30),
            "the preferences",
            Some(&d),
        );
        assert!(
            start.elapsed() >= Duration::from_millis(30),
            "it waits first"
        );
        assert_eq!(r, Err("cannot write\nprefs.json".to_string()));
        let _ = final_attempt(
            || Err("again".into()),
            Duration::ZERO,
            "the settings",
            Some(&d),
        );
        let log = std::fs::read_to_string(d.join(LOG_FILE)).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines.len(), 2, "{log}");
        assert!(
            lines[0].ends_with("the preferences: cannot write prefs.json"),
            "{log}"
        );
        assert!(lines[1].ends_with("the settings: again"), "{log}");
        // No log folder: only stderr, no panic.
        assert!(final_attempt(|| Err("x".into()), Duration::ZERO, "w", None).is_err());
        // A log folder that cannot be made does not hide the result.
        let blocker = d.join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        assert!(
            final_attempt(
                || Err("x".into()),
                Duration::ZERO,
                "w",
                Some(&blocker.join("l"))
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
