//! Autosave and crash recovery.
//!
//! The app copies the document on the UI thread and hands the copy to a worker thread. The worker
//! makes the JSON and writes it atomically to a per-user recovery folder, never to the file of the
//! user (unless the user opts in). The last K versions stay; older ones are deleted.
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

/// The browser keeps about 5 MB in local storage for a site. Autosave stays well below that.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub const BROWSER_STORAGE_LIMIT: usize = 4_000_000;

/// True if a document of this many bytes may be stored in the browser.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn fits_browser_storage(len: usize) -> bool {
    len <= BROWSER_STORAGE_LIMIT
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
pub use crate::autosave_fs::*;

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
    fn the_browser_limit_is_below_five_megabytes() {
        assert!(fits_browser_storage(4_000_000));
        assert!(!fits_browser_storage(4_000_001));
    }

    #[test]
    fn age_text_reads_naturally() {
        assert_eq!(age_text(5), "less than a minute ago");
        assert_eq!(age_text(60), "1 minute ago");
        assert_eq!(age_text(300), "5 minutes ago");
        assert_eq!(age_text(7200), "2 hours ago");
        assert_eq!(age_text(86400 * 3), "3 days ago");
    }
}
